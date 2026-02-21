use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::quadtree::NodeId;
use crate::terrain::{self, TerrainConfig};

/// Resolution of each chunk: 33 vertices per edge = 32 quads per edge.
pub const CHUNK_RESOLUTION: u32 = 33;

/// Neighbor depths for the 4 edges: [left(u=0), right(u=1), bottom(v=0), top(v=1)].
/// `None` means no neighbor (shouldn't happen on a closed cube, but defensive).
/// Values represent the depth of the neighboring leaf node.
pub type NeighborDepths = [Option<u8>; 4];

/// Generates a mesh for a single quadtree chunk.
///
/// The mesh vertices are in planet-local space (already displaced by terrain).
/// Normals are computed from the vertex grid (no extra noise samples needed).
/// Seam handling: when a neighbor is coarser (lower depth), we snap that edge's
/// odd-indexed vertices to the midpoint of their even-indexed neighbors,
/// matching the coarse grid and eliminating T-junction cracks.
pub fn generate_chunk_mesh(
    node_id: &NodeId,
    terrain: &TerrainConfig,
    neighbor_depths: &NeighborDepths,
) -> Mesh {
    let res = CHUNK_RESOLUTION;
    let max_idx = res - 1;
    let (u_min, v_min, u_max, v_max) = node_id.uv_bounds();
    let (normal, axis_a, axis_b) = node_id.face.axes();

    // LOD-aware octave count: coarse chunks use fewer octaves, fine chunks use more.
    let max_octaves = (terrain::BASE_OCTAVES + node_id.depth as usize).min(terrain::TOTAL_OCTAVES);

    let num_verts = (res * res) as usize;

    // Pass 1: compute all vertex positions and apply seam snapping
    let mut raw_positions: Vec<Vec3> = Vec::with_capacity(num_verts);

    for vy in 0..res {
        for vx in 0..res {
            let u = u_min + (vx as f32 / max_idx as f32) * (u_max - u_min);
            let v = v_min + (vy as f32 / max_idx as f32) * (v_max - v_min);

            let point_on_cube = normal + (u - 0.5) * 2.0 * axis_a + (v - 0.5) * 2.0 * axis_b;
            let dir = point_on_cube.normalize();
            let pos = terrain.get_displaced_position_lod(dir, max_octaves);

            raw_positions.push(pos);
        }
    }

    // Pass 2: apply seam snapping, build final positions and UVs
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(num_verts);
    let mut uvs: Vec<[f32; 2]> = Vec::with_capacity(num_verts);
    let mut final_positions: Vec<Vec3> = Vec::with_capacity(num_verts);

    for vy in 0..res {
        for vx in 0..res {
            let idx = (vy * res + vx) as usize;
            let pos = snap_edge_vertex(
                vx, vy, res, node_id.depth, neighbor_depths, &raw_positions, raw_positions[idx],
            );

            positions.push(pos.into());
            uvs.push([vx as f32 / max_idx as f32, vy as f32 / max_idx as f32]);
            final_positions.push(pos);
        }
    }

    // Pass 3: compute normals from the vertex grid using central differences.
    // No noise evaluation needed — just cross products of neighbor positions.
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(num_verts);

    for vy in 0..res {
        for vx in 0..res {
            let idx = (vy * res + vx) as usize;
            let p = final_positions[idx];

            // Tangent (u direction): central difference, one-sided at edges
            let tangent = if vx == 0 {
                final_positions[idx + 1] - p
            } else if vx == max_idx {
                p - final_positions[idx - 1]
            } else {
                final_positions[idx + 1] - final_positions[idx - 1]
            };

            // Bitangent (v direction): central difference, one-sided at edges
            let bitangent = if vy == 0 {
                final_positions[idx + res as usize] - p
            } else if vy == max_idx {
                p - final_positions[idx - res as usize]
            } else {
                final_positions[idx + res as usize] - final_positions[idx - res as usize]
            };

            let mut n = tangent.cross(bitangent).normalize_or_zero();

            // Ensure the normal points outwards (away from planet center)
            if n.dot(p) < 0.0 {
                n = -n;
            }

            normals.push(n.into());
        }
    }

    // Generate triangle indices
    let mut indices: Vec<u32> = Vec::with_capacity(((res - 1) * (res - 1) * 6) as usize);
    for vy in 0..(res - 1) {
        for vx in 0..(res - 1) {
            let i = vy * res + vx;
            indices.push(i);
            indices.push(i + res + 1);
            indices.push(i + res);
            indices.push(i);
            indices.push(i + 1);
            indices.push(i + res + 1);
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));

    mesh
}

/// Snap edge vertices when a neighbor is coarser (1 level difference).
/// Odd-indexed vertices on the shared edge get interpolated to the midpoint
/// of their even-indexed neighbors, matching the coarser neighbor's grid.
fn snap_edge_vertex(
    vx: u32,
    vy: u32,
    res: u32,
    my_depth: u8,
    neighbor_depths: &NeighborDepths,
    raw_positions: &[Vec3],
    original_pos: Vec3,
) -> Vec3 {
    let max = res - 1;

    // Left edge: vx == 0, parametric coordinate is vy
    if vx == 0 {
        if let Some(nd) = neighbor_depths[0] {
            if nd < my_depth && vy % 2 == 1 {
                let below = raw_positions[((vy - 1) * res) as usize];
                let above = raw_positions[((vy + 1) * res) as usize];
                return (below + above) * 0.5;
            }
        }
    }

    // Right edge: vx == max
    if vx == max {
        if let Some(nd) = neighbor_depths[1] {
            if nd < my_depth && vy % 2 == 1 {
                let below = raw_positions[((vy - 1) * res + max) as usize];
                let above = raw_positions[((vy + 1) * res + max) as usize];
                return (below + above) * 0.5;
            }
        }
    }

    // Bottom edge: vy == 0, parametric coordinate is vx
    if vy == 0 {
        if let Some(nd) = neighbor_depths[2] {
            if nd < my_depth && vx % 2 == 1 {
                let left = raw_positions[(vx - 1) as usize];
                let right = raw_positions[(vx + 1) as usize];
                return (left + right) * 0.5;
            }
        }
    }

    // Top edge: vy == max
    if vy == max {
        if let Some(nd) = neighbor_depths[3] {
            if nd < my_depth && vx % 2 == 1 {
                let left = raw_positions[(max * res + vx - 1) as usize];
                let right = raw_positions[(max * res + vx + 1) as usize];
                return (left + right) * 0.5;
            }
        }
    }

    original_pos
}
