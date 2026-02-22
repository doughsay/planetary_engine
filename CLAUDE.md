# Planetary Engine

A procedural planet rendering engine with quadtree LOD, atmospheric scattering, starfield, and galaxy backdrop, built with Rust and Bevy.

## Tech Stack

- **Rust** (edition 2024)
- **Bevy 0.18** - game engine / renderer
- **big_space 0.12** - floating origin for large-scale worlds
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
- **`FullscreenMaterial` trait** (`bevy::core_pipeline::fullscreen_material`) for custom post-process effects:
  - Bind group layout is fixed: `@binding(0)` screen texture, `@binding(1)` sampler, `@binding(2)` uniform buffer (your `ShaderType` struct)
  - Component must derive: `Component + ExtractComponent + Clone + Copy + ShaderType + Default`
  - `sub_graph()` returning `None` (default) uses dynamic `extract_on_add` path — auto-detects Camera3d/Camera2d
  - Node edges define render graph ordering; for pre-tonemap HDR: `StartMainPassPostProcessing → Self → Tonemapping`
  - WGSL struct must match Rust struct layout exactly — `vec3<f32>` has 16-byte alignment, pairs well with a trailing `f32` to fill the row
  - `target` is a **reserved keyword in WGSL** — do not use it as a variable name
- **`GlobalTransform::to_matrix()`** returns `Mat4` (renamed from `compute_matrix()` in older Bevy)
- **Avoid pre-composing projection + view matrices for inversion** — Bevy's own `ndc_to_world` explicitly avoids this (comments cite precision loss). Pass camera vectors (forward/right/up + FOV) to shaders instead of `world_from_clip` inverse matrices.
- **`Camera::clip_from_view()`** returns `self.computed.clip_from_view` — defaults to `Mat4::ZERO` (not identity) until `camera_system` populates it. Can produce NaN when inverted before camera initialization completes.
- **Material bind group is group 3** (`MATERIAL_BIND_GROUP_INDEX = 3`). In WGSL, use `@group(#{MATERIAL_BIND_GROUP}) @binding(N)` — the `#{MATERIAL_BIND_GROUP}` token is substituted by Bevy's shader preprocessor. Group 0 = view, group 1 = environment maps, group 2 = mesh data, group 3 = material. **Do NOT hardcode `@group(2)`** for material uniforms — that conflicts with mesh storage buffers.
- **`bevy_pbr::mesh_functions`** imports `mesh_bindings::mesh` which declares `@group(2) @binding(0)`. For custom Material shaders that don't need the model transform, avoid importing `mesh_functions` entirely — compute world positions from uniforms instead (see galaxy/starfield/atmosphere shaders for this pattern).
- **`AlphaMode::Premultiplied` and `AlphaMode::Add`** share the same blend state (`BLEND_PREMULTIPLIED_ALPHA`): `final = src.rgb + dst.rgb * (1 - src.a)`. The difference is purely in shader output — `alpha = 0` gives additive, `alpha > 0` gives premultiplied alpha with background attenuation.
- **Depth prepass texture** is available at `@group(0) @binding(20)` behind `#ifdef DEPTH_PREPASS` when the camera has `DepthPrepass` component. Type is `texture_depth_2d` (or `texture_depth_multisampled_2d` with MSAA). The `DEPTH_PREPASS` shader def is automatically set by `MeshPipeline::specialize` via `MeshPipelineKey::DEPTH_PREPASS`.

## IMPORTANT: big_space 0.12 API Notes

This project uses `big_space` 0.12 for floating-origin precision at interplanetary scales. **The API has many subtle gotchas — read these carefully.**

### Core Types (from `big_space::prelude::*`)
- **`CellCoord`** — grid cell position (`x: GridPrecision, y: GridPrecision, z: GridPrecision`). `GridPrecision` defaults to `i64`.
- **`Grid`** — component defining cell size. **`Grid::default()` has `cell_edge_length = 2000.0`** and `switching_threshold = 100.0`.
- **`FloatingOrigin`** — marker component on the camera entity. Everything recenters around it.
- **`BigSpace`** — marker component on the root entity (from `BigSpaceRootBundle`).
- **`BigSpaceRootBundle`** — includes `Grid`, `BigSpace`, `GlobalTransform`. Spawn as the root of the entity hierarchy.

### Entity Hierarchy
```
BigSpaceRootBundle (root_id)          ← has Grid + BigSpace
  ├── Sun        (spawn_spatial)      ← CellCoord in root grid
  ├── Light      (spawn_spatial)      ← CellCoord in root grid
  ├── Earth      (spawn_grid_default) ← CellCoord in root grid + OWN Grid for children
  │   └── Chunks (children)           ← CellCoord in Earth's sub-grid
  │   └── Camera (child, FloatingOrigin)
  └── Moon       (spawn_grid_default) ← CellCoord in root grid + OWN Grid for children
      └── Chunks (children)           ← CellCoord in Moon's sub-grid
```

### Critical: Position ↔ CellCoord Conversion
**NEVER convert positions to CellCoord by simply calling `floor()`.** The Grid has `cell_edge_length = 2000`, so `floor(15000.0) = 15000` cells × 2000 units/cell = 30,000,000 units — **2000x too far**. Always use:
```rust
let (cell, offset) = grid.translation_to_grid(world_pos_dvec3);
```
This divides by `cell_edge_length`, rounds, and computes the fractional offset correctly.

### Critical: Multiple Grid Components
`spawn_grid_default()` creates entities with their OWN `Grid` component (for sub-grid children). The root entity also has a `Grid` from `BigSpaceRootBundle`. To query only the root grid, filter with `With<BigSpace>`:
```rust
grid_q: Query<&Grid, With<BigSpace>>  // root grid only
grid_q: Query<&Grid>                   // WRONG — matches root + all planet sub-grids
```
If `grid_q.single()` finds multiple matches it returns `Err` and silently fails.

### spawn_spatial vs spawn_grid_default
- **`spawn_spatial(bundle)`** — adds `CellCoord + Transform` to the entity. Use for simple positioned entities (Sun, lights, etc.) that don't need child grids.
- **`spawn_grid_default(bundle)`** — adds `CellCoord + Transform + Grid`. Use for entities whose children need their own grid-cell positioning (planets with chunks as children).

### Camera as Child Entity
The camera can be a child of a planet entity (e.g., Earth) with `FloatingOrigin`. big_space computes the camera's absolute grid position by composing parent cells, so the floating origin correctly recenters around the camera's actual world position. The camera's own `CellCoord::default()` is fine — the parent's cell provides the offset.

### TransformPlugin
big_space replaces Bevy's `TransformPlugin`. The app must disable it:
```rust
DefaultPlugins.build().disable::<TransformPlugin>()
```
Then add `BigSpaceDefaultPlugins` which provides big_space's own transform propagation.

## Project Structure

```
src/
  main.rs        - App setup, plugins, lighting, camera spawn, scene setup
  planet.rs      - Planet component + PlanetPlugin
  orbit.rs       - Orbit component, OrbitalTime resource, Keplerian solver
  atmosphere.rs  - Per-planet sphere-mesh atmosphere (Material trait)
  camera.rs      - SpaceCamera: 6DOF flight controls (mouse, keyboard, scroll)
  terrain.rs     - TerrainConfig: noise evaluation + normal computation
  quadtree.rs    - CubeFace, NodeId, FaceQuadtree (pure data, no ECS)
  chunk_mesh.rs  - Per-node mesh generation with seam handling
  lod.rs         - PlanetQuadtree component, ChunkNode component, per-planet LOD systems
  mesh_task.rs   - Async mesh generation using AsyncComputeTaskPool
  starfield.rs   - Procedural starfield (~250k stars) with spectral colors
  galaxy.rs      - Procedural Milky Way backdrop (dust lanes, spiral arms, bulge)
assets/shaders/
  atmosphere.wgsl - Rayleigh + Mie scattering ray march
  starfield.wgsl  - Star rendering with Airy PSF diffraction
  galaxy.wgsl     - Galactic rendering with multi-layer procedural noise
```

## Architecture

### World Scale

- 1 world unit = 1 km
- big_space `Grid::default()` cell_edge_length = 2000 units (2000 km per cell)
- Current test system uses micro-scale constants (see main.rs):
  - Sun radius: 2000, Earth radius: 1000, Moon radius: 300
  - Earth orbit: 15,000 km (30s period), Moon orbit: 4,000 km around Earth (10s period)
- Original single-planet scale: Radius 6360, atmosphere shell 100 km thick

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

### Multi-Planet System

- `Planet` component on each planet entity, `PlanetQuadtree` component holds per-planet quadtree state + material
- `ChunkNode.planet: Entity` links each chunk to its owning planet
- Planets are spawned via `spawn_grid_default` (each gets own sub-Grid for chunk children)
- `Sun` marker component on the star entity, `Moon` marker on the moon
- Camera tracking hotkeys: hold 1/2/3 to smoothly look at Sun/Earth/Moon

### Orbital Mechanics (`orbit.rs`)

- `Orbit` component: Keplerian parameters (semi_major_axis, eccentricity, inclination, LAN, arg_periapsis, period, initial_mean_anomaly, parent)
- `OrbitalTime` resource: `speed` multiplier + `elapsed` time
- `update_orbits` system runs each frame:
  1. Compute local orbital position for each body via Kepler equation (Newton iteration solver)
  2. Resolve parent chains same-frame (no 1-frame lag): child world_pos = child local + parent local
  3. Convert world positions to CellCoord + offset via `Grid::translation_to_grid()` on the root grid (`With<BigSpace>`)
- Hierarchical orbits: Moon has `parent: Some(earth_entity)`, Earth has `parent: None` (orbits origin)

### LOD System (`lod.rs`)

**Per-planet LOD**: `update_lod` queries `With<Planet>`, each planet's `PlanetQuadtree` is evaluated independently against the camera position.

**Screen-space error metric:**
```
geometric_error = node_arc_length * radius / CHUNK_RESOLUTION
pixel_error = geometric_error / distance * PERSPECTIVE_SCALE
Split if pixel_error > 1.0, Merge if < 0.5 (hysteresis)
```

**Constraints:**
- Max 1 level difference between adjacent leaves (forced splits propagate)
- Max 32 splits per frame to avoid hitches
- Max depth: 15 (~10m resolution at radius 6360)

**Systems (ordered):**
1. `update_lod` — evaluate screen error per planet, decide splits/merges
2. `sync_chunk_entities` — spawn/despawn entities to match desired leaf set (uses `Changed<PlanetQuadtree>`)
3. `regenerate_dirty_chunks` — re-mesh chunks whose neighbor depths changed
4. `poll_mesh_tasks` — collect completed async meshes
5. `cleanup_retained_parents` — despawn old chunks once children are ready

### Async Mesh Generation (`mesh_task.rs`)

- Uses `AsyncComputeTaskPool` to generate meshes off the main thread
- `PendingMesh(Task<Mesh>)` component on chunks awaiting their mesh
- Parent chunks get `RetainUntilChildrenReady` — stay visible until all 4 children have meshes

### Camera (`camera.rs`)

`SpaceCameraPlugin` provides true 6DOF flight controls:
- **Mouse** (hold left click or M to capture): pitch + yaw
- **WASD**: fly forward/back/left/right, **Space/Ctrl**: up/down (camera-local)
- **Q/E**: roll, **Shift**: boost (50x), **Scroll**: adjust base speed
- Exponential friction smoothing, quaternion rotation (no gimbal lock)
- `SpaceCamera` config component + `SpaceCameraState` runtime state

### Atmosphere (`atmosphere.rs` + `atmosphere.wgsl`)

Per-planet sphere-mesh atmosphere using Bevy's `Material` trait. Each planet gets its own atmosphere entity with an icosphere mesh, enabling multi-planet support without fullscreen shader interference.

**Rust side (`atmosphere.rs`):**
- `AtmosphereMaterial` implements `Material` trait with `AsBindGroup` derive
- `AtmosphereUniforms` (ShaderType): planet_center, planet_radius, sun_direction, atmo_radius, settings
- `AlphaMode::Premultiplied` — blend equation `src + dst * (1 - src.a)` gives `inscatter + scene * transmittance`
- Pipeline specialization: `cull_mode = None` (renders both faces), `depth_write = false`, `depth_compare = Always`
- `enable_prepass() -> false`, `enable_shadows() -> false` — atmosphere doesn't participate in depth/shadow passes

**Shader side (`atmosphere.wgsl`):**
- Vertex shader computes world position from uniforms: `v.position * atmo_radius + planet_center` (unit sphere mesh, no mesh_functions import needed)
- `@builtin(front_facing)` selects correct face: front when camera outside atmosphere, back when inside — prevents double-draw
- Ray-sphere intersection for atmosphere shell and planet surface
- Depth prepass texture (`#ifdef DEPTH_PREPASS`) for terrain-aware ray clamping: reconstructs terrain world position via `view.world_from_clip`, limits ray march to actual terrain depth
- Rayleigh + Mie scattering ray march (16 view steps, 4 light steps)
- Earth-like constants in km: `BETA_R = (5.5e-3, 13.0e-3, 22.4e-3)/km`, `H_R = 8km`, `BETA_M = 21e-3/km`, `H_M = 1.2km`, `G_MIE = 0.76`
- Henyey-Greenstein phase function for Mie, analytical Rayleigh phase
- Sun shadow detection on light path (secondary ray-sphere against planet)
- Premultiplied alpha output: `vec4(inscatter * SUN_INTENSITY, 1.0 - luminance(transmittance))`

**Entity setup (main.rs):**
- `Sphere::new(1.0).mesh().ico(5)` — unit icosphere, shader handles world positioning
- `Transform::default()` + `NoFrustumCulling` — mesh position is shader-driven, not transform-driven
- Camera requires `DepthPrepass` component for terrain-aware atmosphere

**HDR pipeline:**
- `Exposure::SUNLIGHT` + `Tonemapping::AcesFitted`
- Directional sun at 120,000 lux (realistic sunlight)

### Starfield (`starfield.rs`)

- ~250k procedural stars with magnitude-based brightness (inverse CDF sampling)
- 6 spectral classes (O/B through M) with weighted color distribution
- Billboard quads rendered at far distance with additive blending
- Custom `StarfieldMaterial` with Airy PSF diffraction shader (`starfield.wgsl`)
- Deterministic generation from fixed seed

### Galaxy (`galaxy.rs`)

- Milky Way-inspired backdrop rendered on an icosphere (camera inside)
- Custom `GalaxyMaterial` with multi-layer procedural shader (`galaxy.wgsl`)
- 9 noise layers: band, bulge, spiral arms, dust lanes, star clouds, fine detail, bright knots, arm-crossing glow, stellar halo
- Warm-to-cool color gradient (bulge to disc)
- ~30 tunable constants for artistic control

## Key Constants

| Constant | Value | Rationale |
|---|---|---|
| `CHUNK_RESOLUTION` | 33 | 32 quads/edge, ~47 KB per chunk |
| `MAX_DEPTH` | 15 | ~10m ground resolution |
| `SPLIT_THRESHOLD` | 1.0 | Screen-space error trigger |
| `MERGE_THRESHOLD` | 0.5 | Hysteresis prevents thrashing |
| `MAX_SPLITS_PER_FRAME` | 16 | Prevents frame hitches |
| `PERSPECTIVE_SCALE` | 500.0 | Converts world/distance ratio to pixel error |

## Build

```sh
cargo run
```

Dev profile uses `opt-level = 1` for the project and `opt-level = 3` for dependencies, which balances compile time with runtime performance for Bevy.
