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

/// Maximum number of splits per frame (including constraint-forced ones) to avoid hitches.
const MAX_SPLITS_PER_FRAME: usize = 32;

/// Maximum number of dirty chunk re-meshes per frame (neighbor depth changes).
const MAX_REMESHES_PER_FRAME: usize = 16;

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

    // Apply splits after merges, respecting a total budget that includes
    // constraint-forced splits to prevent cascading from overwhelming mesh generation.
    let mut split_budget = MAX_SPLITS_PER_FRAME;
    for node in to_split {
        if split_budget == 0 {
            break;
        }
        split_with_constraint(&mut quadtree, node, &mut split_budget);
    }
}

fn split_with_constraint(quadtree: &mut PlanetQuadtree, node: NodeId, budget: &mut usize) {
    if *budget == 0 {
        return;
    }

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
                split_with_constraint(quadtree, ancestor, budget);
            }
        }
    }

    if *budget == 0 {
        return;
    }

    if let Some(tree) = quadtree.faces.get_mut(&node.face) {
        if tree.split(node).is_some() {
            *budget = budget.saturating_sub(1);
        }
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
/// Any existing visible chunk that is no longer a leaf is retained until
/// the desired leaves covering its region all have completed meshes.
/// This handles both directions: splitting (children replace parent) and
/// merging (parent replaces children).
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

    // Any non-leaf chunk with a visible mesh is retained.
    // cleanup_retained_parents will despawn it once its region is fully covered
    // by completed-mesh desired leaves.
    for (&node_id, &(entity, has_mesh)) in &existing {
        if !desired_leaves.contains(&node_id) {
            if has_mesh {
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

/// System: clean up retained chunks once their spatial region is fully covered
/// by completed-mesh desired leaves.
pub fn cleanup_retained_parents(
    mut commands: Commands,
    retained: Query<(Entity, &ChunkNode), With<RetainUntilChildrenReady>>,
    all_chunks: Query<(&ChunkNode, Option<&PendingMesh>)>,
    quadtree: Res<PlanetQuadtree>,
) {
    if retained.is_empty() {
        return;
    }

    let desired_leaves = quadtree.all_leaves();

    // Build a lookup: node_id -> has_completed_mesh
    let mut chunk_ready: HashMap<NodeId, bool> = HashMap::new();
    for (cn, pending) in &all_chunks {
        chunk_ready.insert(cn.node_id, pending.is_none());
    }

    for (entity, chunk) in &retained {
        if is_region_covered(&chunk.node_id, &desired_leaves, &chunk_ready) {
            commands.entity(entity).despawn();
        }
    }
}

/// Check if a retained chunk's spatial region is fully covered by ready desired leaves.
fn is_region_covered(
    node: &NodeId,
    desired_leaves: &HashSet<NodeId>,
    chunk_ready: &HashMap<NodeId, bool>,
) -> bool {
    // Merge case: an ancestor of this node is a desired leaf.
    // If that ancestor has a completed mesh, the coarser mesh covers this region.
    let mut ancestor_opt = node.parent();
    while let Some(a) = ancestor_opt {
        if desired_leaves.contains(&a) {
            return chunk_ready.get(&a).copied().unwrap_or(false);
        }
        ancestor_opt = a.parent();
    }

    // Split case: this node's descendants are desired leaves.
    // Check all descendant desired leaves have completed meshes.
    let mut found_any = false;
    for leaf in desired_leaves {
        if is_ancestor_of(node, leaf) {
            found_any = true;
            if !chunk_ready.get(leaf).copied().unwrap_or(false) {
                return false;
            }
        }
    }
    found_any
}

/// Returns true if `ancestor` is a strict ancestor of `node`.
fn is_ancestor_of(ancestor: &NodeId, node: &NodeId) -> bool {
    if ancestor.face != node.face || ancestor.depth >= node.depth {
        return false;
    }
    let depth_diff = node.depth - ancestor.depth;
    (node.x >> depth_diff) == ancestor.x && (node.y >> depth_diff) == ancestor.y
}

/// System: regenerate meshes for existing chunks whose neighbor depths changed.
/// Budgeted to avoid overwhelming the async task pool when many neighbors change at once.
pub fn regenerate_dirty_chunks(
    mut commands: Commands,
    quadtree: Res<PlanetQuadtree>,
    mut chunks: Query<(Entity, &mut ChunkNode), Without<PendingMesh>>,
) {
    if !quadtree.is_changed() {
        return;
    }

    let mut remesh_count = 0;
    for (entity, mut chunk) in &mut chunks {
        if remesh_count >= MAX_REMESHES_PER_FRAME {
            break;
        }
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
            remesh_count += 1;
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
