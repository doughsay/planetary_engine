use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use crate::chunk_mesh::{CHUNK_RESOLUTION, NeighborDepths};
use crate::mesh_task::{PendingMesh, RetainUntilChildrenReady, spawn_mesh_task};
use crate::quadtree::{CubeFace, FaceQuadtree, NodeId, NodeState};
use crate::terrain::TerrainConfig;

/// Maximum quadtree depth. At radius 6360 and depth 15, each node covers ~10m.
pub const MAX_DEPTH: u8 = 15;

/// Split when estimated pixel error exceeds this threshold.
const SPLIT_THRESHOLD: f32 = 1.0;

/// Merge when all siblings' pixel error is below this (hysteresis).
const MERGE_THRESHOLD: f32 = 0.5;

/// Maximum number of splits per frame to avoid hitches.
const MAX_SPLITS_PER_FRAME: usize = 16;

/// Approximate perspective scale factor.
/// Converts (world_error / distance) into pixel error for a ~1080p viewport.
/// At 90° FOV on a 1920px-wide screen, 1 radian ≈ 1000 pixels.
/// We use a lower value to avoid over-subdivision.
const PERSPECTIVE_SCALE: f32 = 500.0;

/// Marker component for the planet root entity.
#[derive(Component)]
pub struct Planet;

/// Links a chunk entity to its quadtree node + caches neighbor depths.
#[derive(Component)]
pub struct ChunkNode {
    pub node_id: NodeId,
    pub neighbor_depths: NeighborDepths,
}

/// Resource holding the entire planet quadtree state.
#[derive(Resource)]
pub struct PlanetQuadtree {
    pub faces: HashMap<CubeFace, FaceQuadtree>,
    pub terrain: TerrainConfig,
    pub material: Handle<StandardMaterial>,
    /// World-space position of the planet center.
    pub planet_center: Vec3,
}

impl PlanetQuadtree {
    pub fn new(
        terrain: TerrainConfig,
        material: Handle<StandardMaterial>,
        planet_center: Vec3,
    ) -> Self {
        let mut faces = HashMap::new();
        for &face in &CubeFace::ALL {
            faces.insert(face, FaceQuadtree::new(face));
        }
        Self {
            faces,
            terrain,
            material,
            planet_center,
        }
    }

    pub fn all_leaves(&self) -> HashSet<NodeId> {
        let mut leaves = HashSet::new();
        for tree in self.faces.values() {
            for leaf in tree.leaves() {
                leaves.insert(leaf);
            }
        }
        leaves
    }

    pub fn leaf_depth_at(&self, node: &NodeId) -> u8 {
        if let Some(tree) = self.faces.get(&node.face) {
            tree.leaf_depth_at(node.face, node.depth, node.x, node.y)
        } else {
            0
        }
    }

    pub fn neighbor_depths(&self, node: &NodeId) -> NeighborDepths {
        let neighbors = node.neighbors();
        [
            neighbors[0].map(|n| self.leaf_depth_at(&n)),
            neighbors[1].map(|n| self.leaf_depth_at(&n)),
            neighbors[2].map(|n| self.leaf_depth_at(&n)),
            neighbors[3].map(|n| self.leaf_depth_at(&n)),
        ]
    }
}

/// Compute screen-space pixel error for a node.
fn screen_error(node: &NodeId, camera_pos: Vec3, radius: f32, planet_center: Vec3) -> f32 {
    // Node center in world space = planet_center + direction * radius
    let node_center_world = planet_center + node.center_on_sphere() * radius;
    let distance = camera_pos.distance(node_center_world).max(1.0);
    let geometric_error = node.arc_length() * radius / CHUNK_RESOLUTION as f32;
    geometric_error / distance * PERSPECTIVE_SCALE
}

/// System: evaluate LOD and decide splits/merges.
pub fn update_lod(
    mut quadtree: ResMut<PlanetQuadtree>,
    camera_q: Query<&Transform, With<Camera3d>>,
) {
    let Ok(cam_transform) = camera_q.single() else {
        return;
    };
    let camera_pos = cam_transform.translation;
    let radius = quadtree.terrain.radius;
    let center = quadtree.planet_center;

    let leaves: Vec<NodeId> = quadtree.all_leaves().into_iter().collect();

    // Phase 1: Determine which nodes want to split
    let mut to_split: Vec<NodeId> = Vec::new();
    for &leaf in &leaves {
        let err = screen_error(&leaf, camera_pos, radius, center);
        if err > SPLIT_THRESHOLD && leaf.depth < MAX_DEPTH {
            to_split.push(leaf);
        }
    }

    // Sort by error (highest first) and limit splits per frame
    to_split.sort_by(|a, b| {
        let err_a = screen_error(a, camera_pos, radius, center);
        let err_b = screen_error(b, camera_pos, radius, center);
        err_b
            .partial_cmp(&err_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    to_split.truncate(MAX_SPLITS_PER_FRAME);

    // Phase 2: Always attempt merges (not just when splits are empty).
    // This ensures chunks coarsen when the camera moves away, even while
    // other parts of the quadtree are still refining.
    let all_leaves = quadtree.all_leaves();
    let mut merge_candidates: HashSet<NodeId> = HashSet::new();
    for &leaf in &all_leaves {
        if let Some(parent) = leaf.parent() {
            merge_candidates.insert(parent);
        }
    }

    for parent in merge_candidates {
        let children = parent.children();
        if !children.iter().all(|c| all_leaves.contains(c)) {
            continue;
        }
        let max_err = children
            .iter()
            .map(|c| screen_error(c, camera_pos, radius, center))
            .fold(0.0f32, f32::max);
        if max_err < MERGE_THRESHOLD {
            let can_merge = check_merge_constraint(&quadtree, &parent);
            if can_merge {
                if let Some(tree) = quadtree.faces.get_mut(&parent.face) {
                    tree.merge(parent);
                }
            }
        }
    }

    // Apply splits after merges
    for node in to_split {
        split_with_constraint(&mut quadtree, node);
    }
}

fn split_with_constraint(quadtree: &mut PlanetQuadtree, node: NodeId) {
    if let Some(tree) = quadtree.faces.get(&node.face) {
        if tree.nodes.get(&node) != Some(&NodeState::Leaf) {
            return;
        }
    }

    let neighbors = node.neighbors();
    for neighbor_opt in &neighbors {
        if let Some(neighbor) = neighbor_opt {
            let neighbor_leaf_depth = quadtree.leaf_depth_at(neighbor);
            if neighbor_leaf_depth < node.depth {
                let ancestor = ancestor_at_depth(neighbor, neighbor_leaf_depth);
                split_with_constraint(quadtree, ancestor);
            }
        }
    }

    if let Some(tree) = quadtree.faces.get_mut(&node.face) {
        tree.split(node);
    }
}

fn ancestor_at_depth(node: &NodeId, target_depth: u8) -> NodeId {
    let mut result = *node;
    while result.depth > target_depth {
        if let Some(parent) = result.parent() {
            result = parent;
        } else {
            break;
        }
    }
    result
}

fn check_merge_constraint(quadtree: &PlanetQuadtree, parent: &NodeId) -> bool {
    let neighbors = parent.neighbors();
    for neighbor_opt in &neighbors {
        if let Some(neighbor) = neighbor_opt {
            let nd = quadtree.leaf_depth_at(neighbor);
            if nd > parent.depth + 1 {
                return false;
            }
        }
    }
    true
}

/// System: synchronize chunk entities with the quadtree's desired leaf set.
/// Uses async mesh generation — new chunks start with PendingMesh.
/// Parent chunks being split get RetainUntilChildrenReady to avoid visual holes.
pub fn sync_chunk_entities(
    mut commands: Commands,
    quadtree: Res<PlanetQuadtree>,
    planet_q: Query<Entity, With<Planet>>,
    existing_chunks: Query<(Entity, &ChunkNode, Option<&Mesh3d>)>,
) {
    if !quadtree.is_changed() {
        return;
    }

    let Ok(planet_entity) = planet_q.single() else {
        return;
    };

    let desired_leaves = quadtree.all_leaves();

    // Build map of existing chunks
    let mut existing: HashMap<NodeId, (Entity, bool)> = HashMap::new();
    for (entity, chunk, mesh) in &existing_chunks {
        existing.insert(chunk.node_id, (entity, mesh.is_some()));
    }

    // Identify which nodes are being split (existing chunk no longer a leaf, but its children are)
    let mut nodes_being_split: HashSet<NodeId> = HashSet::new();
    for (&node_id, &(_, has_mesh)) in &existing {
        if !desired_leaves.contains(&node_id) && has_mesh {
            // Check if its children are all in desired leaves
            let children = node_id.children();
            if children.iter().all(|c| desired_leaves.contains(c)) {
                nodes_being_split.insert(node_id);
            }
        }
    }

    // Despawn chunks that are no longer leaves (unless retained for transition)
    for (&node_id, &(entity, _)) in &existing {
        if !desired_leaves.contains(&node_id) {
            if nodes_being_split.contains(&node_id) {
                // Keep visible until children are ready
                commands.entity(entity).insert(RetainUntilChildrenReady);
            } else {
                commands.entity(entity).despawn();
            }
        }
    }

    // Spawn new chunks with async mesh tasks
    for &leaf in &desired_leaves {
        if !existing.contains_key(&leaf) {
            let neighbor_depths = quadtree.neighbor_depths(&leaf);

            let chunk_entity = commands
                .spawn((
                    Transform::default(),
                    Visibility::default(),
                    ChunkNode {
                        node_id: leaf,
                        neighbor_depths,
                    },
                ))
                .id();

            spawn_mesh_task(
                &mut commands,
                chunk_entity,
                leaf,
                &quadtree.terrain,
                neighbor_depths,
            );

            commands.entity(planet_entity).add_child(chunk_entity);
        }
    }
}

/// System: clean up retained parent chunks once all children have meshes.
pub fn cleanup_retained_parents(
    mut commands: Commands,
    retained: Query<(Entity, &ChunkNode), With<RetainUntilChildrenReady>>,
    all_chunks: Query<(&ChunkNode, Option<&PendingMesh>)>,
) {
    for (entity, chunk) in &retained {
        let children = chunk.node_id.children();
        let all_children_ready = children.iter().all(|child_id| {
            all_chunks
                .iter()
                .any(|(cn, pending)| cn.node_id == *child_id && pending.is_none())
        });

        if all_children_ready {
            commands.entity(entity).despawn();
        }
    }
}

/// System: regenerate meshes for existing chunks whose neighbor depths changed.
pub fn regenerate_dirty_chunks(
    mut commands: Commands,
    quadtree: Res<PlanetQuadtree>,
    mut chunks: Query<(Entity, &mut ChunkNode), Without<PendingMesh>>,
) {
    if !quadtree.is_changed() {
        return;
    }

    for (entity, mut chunk) in &mut chunks {
        let current_depths = quadtree.neighbor_depths(&chunk.node_id);
        if current_depths != chunk.neighbor_depths {
            chunk.neighbor_depths = current_depths;
            spawn_mesh_task(
                &mut commands,
                entity,
                chunk.node_id,
                &quadtree.terrain,
                current_depths,
            );
        }
    }
}

/// Plugin that sets up the LOD system.
pub struct LodPlugin;

impl Plugin for LodPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                update_lod,
                sync_chunk_entities.after(update_lod),
                regenerate_dirty_chunks.after(sync_chunk_entities),
                crate::mesh_task::poll_mesh_tasks.after(regenerate_dirty_chunks),
                cleanup_retained_parents.after(crate::mesh_task::poll_mesh_tasks),
            ),
        );
    }
}
