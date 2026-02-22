use big_space::prelude::*;
use std::collections::{HashMap, HashSet};

use bevy::prelude::*;

use crate::chunk_mesh::{CHUNK_RESOLUTION, NeighborDepths};
use crate::mesh_task::{PendingMesh, RetainUntilChildrenReady, spawn_mesh_task};
use crate::quadtree::{CubeFace, FaceQuadtree, NodeId, NodeState};
use crate::terrain::TerrainConfig;
use crate::planet::Planet;

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
const PERSPECTIVE_SCALE: f32 = 500.0;

/// Links a chunk entity to its quadtree node + caches neighbor depths.
#[derive(Component)]
pub struct ChunkNode {
    pub node_id: NodeId,
    pub neighbor_depths: NeighborDepths,
    pub planet: Entity,
}

/// Component holding the entire planet quadtree state.
#[derive(Component)]
pub struct PlanetQuadtree {
    pub faces: HashMap<CubeFace, FaceQuadtree>,
    pub terrain: TerrainConfig,
    pub material: Handle<StandardMaterial>,
}

impl PlanetQuadtree {
    pub fn new(
        terrain: TerrainConfig,
        material: Handle<StandardMaterial>,
    ) -> Self {
        let mut faces = HashMap::new();
        for &face in &CubeFace::ALL {
            faces.insert(face, FaceQuadtree::new(face));
        }
        Self {
            faces,
            terrain,
            material,
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
    let node_center_world = planet_center + node.center_on_sphere() * radius;
    let distance = camera_pos.distance(node_center_world).max(1.0);
    let geometric_error = node.arc_length() * radius / CHUNK_RESOLUTION as f32;
    geometric_error / distance * PERSPECTIVE_SCALE
}

/// System: evaluate LOD and decide splits/merges for all planets.
pub fn update_lod(
    mut planet_q: Query<(Entity, &GlobalTransform, &mut PlanetQuadtree), With<Planet>>,
    camera_q: Query<&GlobalTransform, (With<Camera3d>, Without<Planet>)>,
) {
    let Ok(cam_global) = camera_q.single() else { return };
    let camera_pos: Vec3 = cam_global.translation().into();

    for (_planet_entity, planet_global, mut quadtree) in &mut planet_q {
        let center: Vec3 = planet_global.translation().into();
        let radius = quadtree.terrain.radius;

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

        // Phase 2: Attempt merges
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

        // Phase 3: Apply splits
        let mut split_budget = MAX_SPLITS_PER_FRAME;
        for node in to_split {
            if split_budget == 0 {
                break;
            }
            split_with_constraint(&mut quadtree, node, &mut split_budget);
        }
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

/// System: synchronize chunk entities for all planets.
pub fn sync_chunk_entities(
    mut commands: Commands,
    planet_q: Query<(Entity, &PlanetQuadtree), Changed<PlanetQuadtree>>,
    existing_chunks: Query<(Entity, &ChunkNode, Option<&Mesh3d>)>,
) {
    // Build map of existing chunks grouped by planet
    let mut existing_by_planet: HashMap<Entity, HashMap<NodeId, (Entity, bool)>> = HashMap::new();
    for (entity, chunk, mesh) in &existing_chunks {
        existing_by_planet
            .entry(chunk.planet)
            .or_default()
            .insert(chunk.node_id, (entity, mesh.is_some()));
    }

    for (planet_entity, quadtree) in &planet_q {
        let desired_leaves = quadtree.all_leaves();
        let existing = existing_by_planet.get(&planet_entity);

        // Despawn or retain non-leaf chunks
        if let Some(existing_map) = existing {
            for (&node_id, &(entity, has_mesh)) in existing_map {
                if !desired_leaves.contains(&node_id) {
                    if has_mesh {
                        commands.entity(entity).insert(RetainUntilChildrenReady);
                    } else {
                        commands.entity(entity).despawn();
                    }
                }
            }
        }

        // Spawn new chunks
        for &leaf in &desired_leaves {
            let already_exists = existing.map_or(false, |m| m.contains_key(&leaf));
            if !already_exists {
                let neighbor_depths = quadtree.neighbor_depths(&leaf);

                let chunk_entity = commands
                    .spawn((
                        Transform::default(),
                        Visibility::default(),
                        ChunkNode {
                            node_id: leaf,
                            neighbor_depths,
                            planet: planet_entity,
                        },
                        CellCoord::default(),
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
}

pub fn cleanup_retained_parents(
    mut commands: Commands,
    retained: Query<(Entity, &ChunkNode), With<RetainUntilChildrenReady>>,
    all_chunks: Query<(&ChunkNode, Option<&PendingMesh>)>,
    planet_q: Query<&PlanetQuadtree>,
) {
    if retained.is_empty() {
        return;
    }

    // Build lookup: (planet, node_id) -> has_completed_mesh
    let mut chunk_ready: HashMap<(Entity, NodeId), bool> = HashMap::new();
    for (cn, pending) in &all_chunks {
        chunk_ready.insert((cn.planet, cn.node_id), pending.is_none());
    }

    for (entity, chunk) in &retained {
        if let Ok(quadtree) = planet_q.get(chunk.planet) {
            let desired_leaves = quadtree.all_leaves();
            if is_region_covered(&chunk.node_id, chunk.planet, &desired_leaves, &chunk_ready) {
                commands.entity(entity).despawn();
            }
        }
    }
}

fn is_region_covered(
    node: &NodeId,
    planet: Entity,
    desired_leaves: &HashSet<NodeId>,
    chunk_ready: &HashMap<(Entity, NodeId), bool>,
) -> bool {
    let mut ancestor_opt = node.parent();
    while let Some(a) = ancestor_opt {
        if desired_leaves.contains(&a) {
            return chunk_ready.get(&(planet, a)).copied().unwrap_or(false);
        }
        ancestor_opt = a.parent();
    }

    let mut found_any = false;
    for leaf in desired_leaves {
        if is_ancestor_of(node, leaf) {
            found_any = true;
            if !chunk_ready.get(&(planet, *leaf)).copied().unwrap_or(false) {
                return false;
            }
        }
    }
    found_any
}

fn is_ancestor_of(ancestor: &NodeId, node: &NodeId) -> bool {
    if ancestor.face != node.face || ancestor.depth >= node.depth {
        return false;
    }
    let depth_diff = node.depth - ancestor.depth;
    (node.x >> depth_diff) == ancestor.x && (node.y >> depth_diff) == ancestor.y
}

pub fn regenerate_dirty_chunks(
    mut commands: Commands,
    planet_q: Query<&PlanetQuadtree>,
    mut chunks: Query<(Entity, &mut ChunkNode), Without<PendingMesh>>,
) {
    let mut remesh_count = 0;
    for (entity, mut chunk) in &mut chunks {
        if remesh_count >= MAX_REMESHES_PER_FRAME {
            break;
        }
        if let Ok(quadtree) = planet_q.get(chunk.planet) {
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
}

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
