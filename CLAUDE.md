# Planetary Engine

A procedural planet rendering engine with quadtree LOD and atmospheric scattering, built with Rust and Bevy.

## Tech Stack

- **Rust** (edition 2024)
- **Bevy 0.18** - game engine / renderer
- **noise 0.9** - procedural terrain generation (Fbm + SuperSimplex)

## IMPORTANT: Bevy 0.18 API Notes

This project uses Bevy 0.18, which has significant breaking changes from earlier versions. **Do not rely on internal knowledge of Bevy APIs — always verify against the actual crate source in `~/.cargo/registry/src/` when unsure.** Known differences from older Bevy versions:

- **Import paths changed**: Many types moved to new modules:
  - `Indices`, `PrimitiveTopology` -> `bevy::mesh::` (not `bevy::render::mesh::`)
  - `RenderAssetUsages` -> `bevy::asset::` (not `bevy::render::render_asset::`)
  - `Exposure` -> `bevy::camera::Exposure`
  - `Tonemapping` -> `bevy::core_pipeline::tonemapping::Tonemapping`
  - `Atmosphere`, `AtmosphereSettings`, `ScatteringMedium` -> `bevy::pbr::`
- **`EventReader` removed**: Replaced by `bevy::ecs::message::MessageReader`. Input events like `MouseWheel` derive `Message`, not `Event`.
- **`AmbientLight` is a component**, not a resource. Spawn it with `commands.spawn()`, not `commands.insert_resource()`.
- **`WireframePlugin` has fields**: Use `WireframePlugin::default()`, not `WireframePlugin` as a unit struct.
- **`Transform::forward()` returns `Dir3`**, not `Vec3`. Dereference with `*transform.forward()` to get a `Vec3`.
- **`Atmosphere` requires `AtmosphereSettings` and `Hdr`** via `#[require]` — they're auto-inserted.

## Project Structure

```
src/
  main.rs        - App setup, plugins, camera, lighting, atmosphere
  terrain.rs     - TerrainConfig: noise evaluation + normal computation
  quadtree.rs    - CubeFace, NodeId, FaceQuadtree (pure data, no ECS)
  chunk_mesh.rs  - Per-node mesh generation with seam handling
  lod.rs         - PlanetQuadtree resource, ChunkNode component, LOD systems
  mesh_task.rs   - Async mesh generation using AsyncComputeTaskPool
```

## Architecture

### World Scale

- **Radius: 6360.0** (1 world unit = 1 km)
- Camera range: 6370 km (surface) to 50,000 km (deep space)
- `AtmosphereSettings.scene_units_to_m = 1000.0`

### Terrain (`terrain.rs`)

`TerrainConfig` is the single source of truth for elevation:
- `get_displaced_position(normalized_dir)` — deterministic elevation for any point on the unit sphere
- `compute_normal(dir, tangent, bitangent, pos)` — finite-difference normals
- Same output regardless of which LOD level calls it

### Quadtree (`quadtree.rs`)

Pure data structure (no ECS dependencies):
- `CubeFace` enum (6 faces) with `axes()` returning (normal, axis_a, axis_b)
- `NodeId { face, depth, x, y }` — uniquely identifies any node
  - `uv_bounds()`, `center_on_sphere()`, `arc_length()`
  - `children()`, `parent()`, `neighbors()` with cross-face lookups
- `FaceQuadtree` — HashMap-based tree tracking Leaf vs Split states
- Cross-face adjacency table handles UV coordinate mapping across cube edges

### Chunk Meshes (`chunk_mesh.rs`)

- 33×33 vertices per chunk (32 quads per edge, ~2048 triangles)
- Vertices placed by mapping node's UV sub-range through cube-face projection + terrain displacement
- **Seam handling**: when a neighbor is 1 level coarser, odd-indexed edge vertices snap to interpolated positions

### LOD System (`lod.rs`)

**Screen-space error metric:**
```
geometric_error = node_arc_length * radius / CHUNK_RESOLUTION
pixel_error = geometric_error / distance * PERSPECTIVE_SCALE
Split if pixel_error > 1.0, Merge if < 0.5 (hysteresis)
```

**Constraints:**
- Max 1 level difference between adjacent leaves (forced splits propagate)
- Max 4 splits per frame to avoid hitches
- Max depth: 15 (~10m resolution at radius 6360)

**Systems (ordered):**
1. `update_lod` — evaluate screen error, decide splits/merges
2. `sync_chunk_entities` — spawn/despawn entities to match desired leaf set
3. `regenerate_dirty_chunks` — re-mesh chunks whose neighbor depths changed
4. `poll_mesh_tasks` — collect completed async meshes
5. `cleanup_retained_parents` — despawn old chunks once children are ready

### Async Mesh Generation (`mesh_task.rs`)

- Uses `AsyncComputeTaskPool` to generate meshes off the main thread
- `PendingMesh(Task<Mesh>)` component on chunks awaiting their mesh
- Parent chunks get `RetainUntilChildrenReady` — stay visible until all 4 children have meshes

### Atmosphere (`main.rs`)

- `Atmosphere::earthlike(medium)` with `ScatteringMedium::default()` (Rayleigh + Mie + Ozone)
- `Exposure::SUNLIGHT` + `Tonemapping::AcesFitted` for HDR rendering
- Directional sun at 120,000 lux (realistic sunlight)

## Key Constants

| Constant | Value | Rationale |
|---|---|---|
| `CHUNK_RESOLUTION` | 33 | 32 quads/edge, ~47 KB per chunk |
| `MAX_DEPTH` | 15 | ~10m ground resolution |
| `SPLIT_THRESHOLD` | 1.0 | Screen-space error trigger |
| `MERGE_THRESHOLD` | 0.5 | Hysteresis prevents thrashing |
| `MAX_SPLITS_PER_FRAME` | 4 | Prevents frame hitches |
| `PERSPECTIVE_SCALE` | 1000.0 | Converts world/distance ratio to pixel error |

## Build

```sh
cargo run
```

Dev profile uses `opt-level = 1` for the project and `opt-level = 3` for dependencies, which balances compile time with runtime performance for Bevy.
