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
- **Material bind group is group 3** (`MATERIAL_BIND_GROUP_INDEX = 3`). In WGSL, use `@group(#{MATERIAL_BIND_GROUP}) @binding(N)` — the `#{MATERIAL_BIND_GROUP}` token is substituted by Bevy's shader preprocessor. Group 0 = view, group 1 = environment maps, group 2 = mesh data, group 3 = material. **Do NOT hardcode `@group(2)`** for material uniforms — that conflicts with mesh storage buffers.
- **`bevy_pbr::mesh_functions`** imports `mesh_bindings::mesh` which declares `@group(2) @binding(0)`. For custom Material shaders that don't need the model transform, avoid importing `mesh_functions` entirely — compute world positions from uniforms instead (see galaxy/starfield/atmosphere shaders for this pattern).
- **`AlphaMode::Premultiplied` and `AlphaMode::Add`** share the same blend state (`BLEND_PREMULTIPLIED_ALPHA`): `final = src.rgb + dst.rgb * (1 - src.a)`. The difference is purely in shader output — `alpha = 0` gives additive, `alpha > 0` gives premultiplied alpha with background attenuation.
- **Depth prepass texture** is available at `@group(0) @binding(20)` behind `#ifdef DEPTH_PREPASS` when the camera has `DepthPrepass` component. Type is `texture_depth_2d` (or `texture_depth_multisampled_2d` with MSAA). The `DEPTH_PREPASS` shader def is automatically set by `MeshPipeline::specialize` via `MeshPipelineKey::DEPTH_PREPASS`.

## Project Structure

```
src/
  main.rs        - App setup, plugins, lighting, camera spawn
  atmosphere.rs  - Per-planet sphere-mesh atmosphere (Material trait)
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
- Scattering constants work directly in km (world units) — no unit conversion needed in shader

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
