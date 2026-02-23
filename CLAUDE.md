# Planetary Engine

A procedural planet rendering engine with SDF raymarching, starfield, and galaxy backdrop, built with Rust and Bevy.

## Tech Stack

- **Rust** (edition 2024)
- **Bevy 0.18** - game engine / renderer
- **big_space 0.12** - floating origin for large-scale worlds
- **noise 0.9** - available for CPU-side noise (currently unused — terrain noise is in WGSL)

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
BigSpaceRootBundle (root_id)              ← has Grid + BigSpace
  ├── Sun            (spawn_spatial)      ← CellCoord in root grid
  ├── Light          (spawn_spatial)      ← CellCoord in root grid
  ├── Planet         (spawn_grid_default) ← CellCoord in root grid + OWN Grid for children
  │   └── Chunks (children)              ← CellCoord in planet's sub-grid
  │   └── Camera (child, FloatingOrigin)
  └── Satellite      (spawn_grid_default) ← CellCoord in root grid + OWN Grid for children
      └── Chunks (children)              ← CellCoord in satellite's sub-grid
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
The camera can be a child of a planet entity with `FloatingOrigin`. big_space computes the camera's absolute grid position by composing parent cells, so the floating origin correctly recenters around the camera's actual world position. The camera's own `CellCoord::default()` is fine — the parent's cell provides the offset.

### TransformPlugin
big_space replaces Bevy's `TransformPlugin`. The app must disable it:
```rust
DefaultPlugins.build().disable::<TransformPlugin>()
```
Then add `BigSpaceDefaultPlugins` which provides big_space's own transform propagation.

## Project Structure

```
src/
  main.rs            - App setup, plugins, lighting, camera spawn, scene setup
  planet.rs          - Planet component, PlanetPlugin, update_planet_materials system
  planet_material.rs - PlanetMaterial (Bevy Material), PlanetSdfUniforms, SdfConfig
  orbit.rs           - Orbit component, OrbitalTime resource, Keplerian solver
  camera.rs          - SpaceCamera: 6DOF flight controls (mouse, keyboard, scroll)
  starfield.rs       - Procedural starfield (~250k stars) with spectral colors
  galaxy.rs          - Procedural Milky Way backdrop (dust lanes, spiral arms, bulge)
assets/shaders/
  noise.wgsl       - 3D simplex noise + FBM with distance-based octave LOD
  planet_sdf.wgsl  - Terrain SDF sphere tracing, lighting, depth output
  starfield.wgsl   - Star rendering with Airy PSF diffraction
  galaxy.wgsl      - Galactic rendering with multi-layer procedural noise
```

## Architecture

### World Scale

- 1 world unit = 1 km
- big_space `Grid::default()` cell_edge_length = 2000 units (2000 km per cell)
- Current test system uses micro-scale constants (see main.rs):
  - Sun radius: 2000, planet radius: 1000, satellite radius: 300
  - Planet orbit: 15,000 km (30s period), satellite orbit: 4,000 km around planet (10s period)
- Original single-planet scale: Radius 6360, atmosphere shell 100 km thick

### SDF Terrain Rendering (`planet_material.rs` + `planet_sdf.wgsl`)

Terrain is rendered via GPU raymarching on an icosphere mesh per planet. No CPU-side mesh generation, no octree, no seam stitching.

**Rust side (`planet_material.rs`):**
- `PlanetMaterial` implements Bevy `Material` trait with `AsBindGroup` derive
- `PlanetSdfUniforms` (ShaderType): planet_center, planet_radius, camera_position, max_elevation, sun_direction, noise params
- `SdfConfig` — Rust-side config struct holding noise parameters per planet
- Pipeline specialization: `cull_mode = None`, `depth_write = true`, `AlphaMode::Opaque`
- `enable_prepass() -> false`, `enable_shadows() -> false`

**Shader side (`planet_sdf.wgsl`):**
- Vertex shader: positions unit icosphere at `planet_center` scaled by `planet_radius + max_elevation` (same pattern as galaxy.wgsl — no `mesh_functions` import)
- SDF: `sdf(p) = length(p - center) - radius - fbm(normalize(p - center) * frequency) * amplitude`
- Sphere tracing: ray from camera through fragment, bounded by bounding sphere and core sphere, MAX_STEPS=128, SURFACE_EPSILON=0.01 (10m)
- Normal: SDF gradient via central finite differences (6 extra SDF evaluations), epsilon scaled by camera distance
- Lighting: Lambertian (ambient 0.03 + diffuse 0.9) with `sun_direction`
- Depth: writes `frag_depth` at actual hit point (not icosphere surface) for correct depth testing
- LOD: FBM octave count scales automatically with distance — sub-pixel octaves are skipped

**Noise (`noise.wgsl`):**
- 3D simplex noise (Ashima webgl-noise port), `#define_import_path noise`
- FBM with distance-based octave culling: compares feature size (1/frequency) to pixel size, breaks when features are sub-pixel
- Normalizes by full amplitude sum so adding octaves adds detail without rescaling

**Entity setup (main.rs):**
- `Sphere::new(1.0).mesh().ico(5)` — unit icosphere, shader handles world positioning
- `Transform::default()` + `NoFrustumCulling` — mesh position is shader-driven, not transform-driven
- Camera has `DepthPrepass` component for future atmosphere integration

**`update_planet_materials` system (`planet.rs`):**
- Runs each frame, updates every planet's material uniforms with current camera position, planet center (from GlobalTransform), and sun direction

### Multi-Planet System

- `Planet` component on each planet entity holds `SdfConfig` + `material_handle: Handle<PlanetMaterial>`
- Each planet spawns an icosphere child entity with `PlanetMaterial`
- Planets are spawned via `spawn_grid_default` (each gets own sub-Grid for children)
- `Sun` marker component on the star entity, `Satellite` marker on the satellite
- Camera tracking hotkeys: hold 1/2/3 to smoothly look at Sun/planet/satellite

### Orbital Mechanics (`orbit.rs`)

- `Orbit` component: Keplerian parameters (semi_major_axis, eccentricity, inclination, LAN, arg_periapsis, period, initial_mean_anomaly, parent)
- `OrbitalTime` resource: `speed` multiplier + `elapsed` time
- `update_orbits` system runs each frame:
  1. Compute local orbital position for each body via Kepler equation (Newton iteration solver)
  2. Resolve parent chains same-frame (no 1-frame lag): child world_pos = child local + parent local
  3. Convert world positions to CellCoord + offset via `Grid::translation_to_grid()` on the root grid (`With<BigSpace>`)
- Hierarchical orbits: satellite has `parent: Some(planet_id)`, planet has `parent: None` (orbits origin)

### Camera (`camera.rs`)

`SpaceCameraPlugin` provides true 6DOF flight controls:
- **Mouse** (hold left click or M to capture): pitch + yaw
- **WASD**: fly forward/back/left/right, **Space/Ctrl**: up/down (camera-local)
- **Q/E**: roll, **Shift**: boost (50x), **Scroll**: adjust base speed
- Exponential friction smoothing, quaternion rotation (no gimbal lock)
- `SpaceCamera` config component + `SpaceCameraState` runtime state

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

| Constant | Value | Location | Rationale |
|---|---|---|---|
| `MAX_STEPS` | 128 | planet_sdf.wgsl | Ray march step budget |
| `SURFACE_EPSILON` | 0.01 | planet_sdf.wgsl | SDF hit threshold (~10m in km world units) |
| `pixel_angular_size` | 0.0005 | planet_sdf.wgsl | ~60° FOV at ~2000px width |

## Build

```sh
cargo run
```

Dev profile uses `opt-level = 1` for the project and `opt-level = 3` for dependencies, which balances compile time with runtime performance for Bevy.
