use bevy::prelude::*;
use bevy::tasks::{block_on, poll_once, AsyncComputeTaskPool, Task};

use crate::chunk_mesh::{generate_chunk_mesh, NeighborDepths};
use crate::lod::{ChunkNode, PlanetQuadtree};
use crate::quadtree::NodeId;
use crate::terrain::TerrainConfig;

/// Component on chunks awaiting their async mesh result.
#[derive(Component)]
pub struct PendingMesh(Task<Mesh>);

/// Component marking a parent chunk that should stay visible until
/// all 4 of its children have completed meshes.
#[derive(Component)]
pub struct RetainUntilChildrenReady;

/// System: poll pending mesh tasks and insert completed meshes.
pub fn poll_mesh_tasks(
    mut commands: Commands,
    mut pending: Query<(Entity, &mut PendingMesh, &ChunkNode)>,
    mut meshes: ResMut<Assets<Mesh>>,
    quadtree: Res<PlanetQuadtree>,
) {
    for (entity, mut pending_mesh, _chunk) in &mut pending {
        if pending_mesh.0.is_finished() {
            if let Some(mesh) = block_on(poll_once(&mut pending_mesh.0)) {
                commands.entity(entity).insert((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(quadtree.material.clone()),
                ));
                commands.entity(entity).remove::<PendingMesh>();
            }
        }
    }
}

/// Spawn an async mesh generation task for a chunk entity.
pub fn spawn_mesh_task(
    commands: &mut Commands,
    entity: Entity,
    node_id: NodeId,
    terrain: &TerrainConfig,
    neighbor_depths: NeighborDepths,
) {
    let terrain = terrain.clone();

    let task = AsyncComputeTaskPool::get().spawn(async move {
        generate_chunk_mesh(&node_id, &terrain, &neighbor_depths)
    });

    commands.entity(entity).insert(PendingMesh(task));
}
