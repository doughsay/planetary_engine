# Planetary Engine

A procedural planet rendering engine with quadtree LOD, atmospheric scattering, starfield, and galaxy backdrop, built with Rust and Bevy.

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

## Project Structure

```
src/
  main.rs        - App setup, plugins, lighting, camera spawn
  atmosphere.rs  - Custom fullscreen post-process atmosphere (FullscreenMaterial)
  camera.rs      - SpaceCamera: 6DOF flight controls (mouse, keyboard, scroll)
  terrain.rs     - TerrainConfig: noise evaluation + normal computation
  quadtree.rs    - CubeFace, NodeId, FaceQuadtree (pure data, no ECS)
  chunk_mesh.rs  - Per-node mesh generation with seam handling
  lod.rs         - PlanetQuadtree resource, ChunkNode component, LOD systems
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

- **Radius: 6360.0** (1 world unit = 1 km)
- Camera range: 6370 km (surface) to 50,000 km (deep space)
- Atmosphere shell: planet_radius (6360) to atmo_radius (6460), 100 km thick
- `scene_units_to_m = 1000.0` — converts world units (km) to meters for scattering math

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
- Max 16 splits per frame to avoid hitches
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

### Camera (`camera.rs`)

`SpaceCameraPlugin` provides true 6DOF flight controls:
- **Mouse** (hold left click or M to capture): pitch + yaw
- **WASD**: fly forward/back/left/right, **Space/Ctrl**: up/down (camera-local)
- **Q/E**: roll, **Shift**: boost (50x), **Scroll**: adjust base speed
- Exponential friction smoothing, quaternion rotation (no gimbal lock)
- `SpaceCamera` config component + `SpaceCameraState` runtime state

### Atmosphere (`atmosphere.rs` + `atmosphere.wgsl`)

Custom fullscreen post-process atmosphere replacing Bevy's built-in `Atmosphere` component, using the `FullscreenMaterial` trait for render graph integration.

**Rust side (`atmosphere.rs`):**
- `AtmosphereEffect` component: `Component + ExtractComponent + ShaderType + FullscreenMaterial`
- Uniform struct fields: planet params (center, radius, atmo_radius), camera vectors (position, forward, right, up), FOV (fov_tan_half, aspect_ratio), sun_direction, scene_units_to_m
- Runs in render graph between `StartMainPassPostProcessing` and `Tonemapping` (HDR space, pre-tonemap)
- `update_atmosphere_uniforms` system in `PostUpdate` after `CameraUpdateSystems` — extracts camera vectors from `GlobalTransform` matrix columns and FOV/aspect from `Projection`
- Camera entity requires `Hdr` component explicitly (the built-in `Atmosphere` had `#[require(Hdr)]` which auto-inserted it)

**Shader side (`atmosphere.wgsl`):**
- Ray reconstruction from camera vectors + FOV (no matrix inversion — see lessons learned below)
- Ray-sphere intersection for atmosphere shell and planet surface
- Rayleigh + Mie scattering ray march (16 view steps, 4 light steps)
- Earth-like constants: `BETA_R = (5.5e-6, 13.0e-6, 22.4e-6)`, `H_R = 8000m`, `BETA_M = 21e-6`, `H_M = 1200m`, `G_MIE = 0.76`
- Henyey-Greenstein phase function for Mie, analytical Rayleigh phase
- Transmittance blending: `scene_color * transmittance + atmosphere_emission`

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
