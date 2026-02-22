# Design: SDF Planet Rendering

## Decision

Replace the planned density field + octree + surface extraction pipeline (Phases 2-7 of the original plan) with GPU-side SDF raymarching. Each planet is rendered as an icosphere mesh whose fragment shader sphere-traces through a signed distance field to find the terrain surface.

## Motivation

The original plan used density fields evaluated on the CPU with surface extraction (Dual Contouring / Marching Cubes) to produce triangle meshes. This works but has inherent resolution limits — terrain detail is bounded by mesh resolution and requires octree LOD management, seam stitching, and async mesh generation.

SDF raymarching evaluates the terrain function directly on the GPU per-pixel. Detail is limited only by the number of noise octaves evaluated, which scales naturally with distance — giving "infinite detail" at all zoom levels with no mesh management at all.

The tradeoff: physics integration is deferred (the GPU evaluates terrain, not the CPU). The path to physics is JIT local meshing — evaluating a matching SDF on the CPU near physics objects and extracting collision geometry locally. This is documented in `SDF-vs-surface-extraction.md` (Path 1).

## What Stays (Phases 0, 1, 1.5 — already complete)

- `big_space` floating origin integration
- `Planet` component + `PlanetPlugin`, multi-planet spawning
- Orbital mechanics (`orbit.rs`) — Keplerian solver, hierarchical orbits, `OrbitalTime`
- Camera (`camera.rs`) — 6DOF flight controls
- Starfield (`starfield.rs`) — procedural star rendering
- Galaxy (`galaxy.rs`) — Milky Way backdrop
- HDR pipeline — `Exposure::SUNLIGHT` + `AcesFitted`

## What Gets Removed

- `terrain.rs` — heightmap noise evaluation (replaced by WGSL SDF)
- `quadtree.rs` — 2D LOD structure (no longer needed)
- `chunk_mesh.rs` — per-node mesh generation (no meshes to generate)
- `lod.rs` — quadtree LOD systems (LOD is implicit in shader octave count)
- `mesh_task.rs` — async mesh generation (no async work needed)

## Architecture

### Rendering Approach: Sphere-Mesh Raymarching

Each planet spawns an icosphere mesh (radius = planet_radius + max_elevation). The fragment shader:

1. Computes a ray from the camera through the fragment
2. Sphere-traces from the icosphere surface inward using the SDF
3. Evaluates `sdf(p) = length(p - center) - radius - terrain_noise(normalize(p - center))`
4. Computes normals via SDF gradient (finite differences)
5. Lights the surface with sun direction
6. Writes correct `frag_depth` for depth buffer integration

This is the same pattern as the existing atmosphere shader, which already renders via raymarching on an icosphere mesh.

### Terrain Authoring: Parameterized WGSL with Rust Configuration

The WGSL shader implements terrain "building blocks" — noise primitives, cave carving, domain warping, Boolean operations. Rust controls *which* blocks run, in *what* order, with *what* parameters, via uniform buffers.

**Rust side** (creative iteration):
```rust
pub struct SdfConfig {
    pub radius: f32,
    pub max_elevation: f32,
    // Continental shape
    pub continental_frequency: f32,
    pub continental_amplitude: f32,
    pub continental_octaves: u32,
    // Mountain ridges
    pub ridge_frequency: f32,
    pub ridge_amplitude: f32,
    // Cave system
    pub cave_frequency: f32,
    pub cave_threshold: f32,
    // Domain warping
    pub warp_frequency: f32,
    pub warp_amplitude: f32,
    // ... extensible with more layers
}
```

**WGSL side** (stable building blocks):
```wgsl
fn planet_sdf(p: vec3f) -> f32 {
    let dir = normalize(p - planet_center);
    let r = length(p - planet_center);
    var d = r - u.radius;
    d -= fbm(dir * u.continental_freq, u.continental_octaves) * u.continental_amp;
    d -= ridge_noise(dir * u.ridge_freq) * u.ridge_amp;
    // Cave carving (3D, not limited to surface)
    let cave = fbm_3d(p * u.cave_freq);
    if (cave > u.cave_threshold) { d = max(d, cave - u.cave_threshold); }
    return d;
}
```

Adding new terrain operations: implement the function in WGSL, expose parameters in the Rust uniform struct. The composition space from existing building blocks (noise + ridges + caves + warping + Boolean ops) is already enormous.

### Distance-Based Octave LOD

The number of FBM octaves evaluated scales with distance from the camera to the sample point:

```wgsl
fn fbm_lod(p: vec3f, max_octaves: u32, distance: f32) -> f32 {
    let pixel_size = distance * pixel_angular_size;
    var result = 0.0;
    var freq = base_frequency;
    var amp = base_amplitude;
    for (var i = 0u; i < max_octaves; i++) {
        if (amp / freq < pixel_size) { break; }
        result += simplex3d(p * freq) * amp;
        freq *= lacunarity;
        amp *= persistence;
    }
    return result;
}
```

- Orbital distance: 2-3 octaves (smooth, fast)
- Mountain scale: 6-8 octaves
- Surface level: 12-16 octaves (full detail)

No explicit LOD system, no octree management, no chunk spawning/despawning.

### Material Pipeline (Bevy)

`PlanetMaterial` implements Bevy's `Material` trait:
- `AlphaMode::Opaque` (terrain is solid)
- `cull_mode = None` (camera may be inside bounding icosphere when on surface)
- `depth_write = true` (terrain participates in depth testing)
- `depth_compare = GreaterEqual` or per Bevy's reverse-Z convention
- `NoFrustumCulling` (shader positions vertices via uniforms)
- Fragment shader writes `frag_depth` at the actual SDF hit point

### Entity Structure Per Planet

```
Planet entity (CellCoord, Orbit, Planet component with SdfConfig)
  +-- SDF Terrain mesh (icosphere at bounding radius, PlanetMaterial)
  +-- Atmosphere mesh  (icosphere at atmo radius, AtmosphereMaterial)  [optional]
```

### Depth Integration with Atmosphere

The atmosphere shader already reads from the depth prepass texture (`DEPTH_PREPASS` ifdef) to clip rays at terrain. Since the SDF shader writes correct `frag_depth`, the atmosphere automatically works — it sees the terrain depth and stops ray marching there. No special integration needed beyond ensuring the camera has the `DepthPrepass` component.

## Implementation Phases

### Phase 2: SDF Rendering Foundation

**Goal**: A planet rendered via GPU raymarching with basic noise terrain.

**Tasks**:
1. Create `noise.wgsl` — 3D simplex noise
2. Create `planet_sdf.wgsl` — sphere tracing + FBM displacement + Lambertian lighting
3. Create `planet_material.rs` — Bevy `Material` impl, uniform struct, pipeline config
4. Update `planet.rs` — replace `TerrainConfig` with `SdfConfig`
5. Update `main.rs` — spawn terrain icosphere per planet with `PlanetMaterial`
6. Remove `terrain.rs`, `quadtree.rs`, `chunk_mesh.rs`, `lod.rs`, `mesh_task.rs`

**Deliverable**: Bumpy spherical planet, more detail as you zoom in.

### Phase 3: Terrain Quality + Lighting

**Goal**: Realistic terrain shapes, properly lit by the sun.

**Tasks**:
1. Add WGSL building blocks: ridge noise, domain warping
2. Tune continental-scale noise for realistic landmass shapes
3. SDF gradient normals (distance-scaled finite differences)
4. Dynamic sun direction from orbital position
5. Correct `frag_depth` output
6. Multi-planet: Earth + Moon with different SDF configs

**Deliverable**: Multiple planets with distinct, realistic terrain lit by the sun.

### Phase 4: Terrain Coloring + Biomes

**Goal**: Planets with visual variety.

**Tasks**:
1. Elevation-based coloring (ocean, lowland, mountain, snow)
2. Slope-based coloring (cliffs vs flats)
3. Latitude variation (polar ice, tropical zones)
4. Per-planet color palette
5. Ocean rendering (flat water within SDF)

**Deliverable**: Visually distinct, colorful planets.

### Phase 5: Atmosphere Re-integration

**Goal**: Atmosphere shader working with SDF terrain.

**Tasks**:
1. Re-add atmosphere entities per planet (icosphere + `AtmosphereMaterial`)
2. Camera `DepthPrepass` — atmosphere reads terrain depth
3. Verify atmosphere clips at terrain (no sky below ground)
4. Dynamic sun direction in atmosphere uniforms
5. Multi-planet atmospheres (some planets have them, some don't)

**Deliverable**: Planets with terrain + atmosphere.

### Phase 6: Volumetric Features

**Goal**: Caves, overhangs, arches — terrain that heightmaps can't do.

**Tasks**:
1. Cave carving in WGSL (3D noise threshold + Boolean subtraction)
2. Overhang generation (domain-warped noise)
3. Tune for natural-looking cave entrances
4. Raymarching convergence inside caves (may need adjusted step strategy)
5. Expose cave/overhang parameters in Rust SdfConfig

**Deliverable**: Fly into caves, walk under arches.

### Phase 7: Performance + Polish

**Goal**: Smooth performance, demo-ready planetary system.

**Tasks**:
1. Profile shader, optimize noise
2. Early ray termination, bounding sphere optimization
3. Screen-space adaptive step size
4. Multi-planet performance tuning
5. Demo system: Earth-like + Moon + rocky planet

**Deliverable**: Polished, performant planetary system.

### Phase 8 (Future): Physics via JIT Local Meshing

**Goal**: Physical interaction with terrain.

**Tasks**:
1. Duplicate SDF composition in Rust (matching WGSL output)
2. Small 3D grid around physics objects, CPU density evaluation
3. Marching Cubes on CPU grid -> invisible collision mesh
4. Feed to physics engine
5. Tiered simulation (full near player, simplified mid-range, frozen far)

**Deliverable**: Walk on planets, physics objects land on terrain.

## Phase Dependency Graph

```
Phase 0 done --> Phase 1 done --> Phase 1.5 done (orbits)
                                       |
                                       v
                                  Phase 2 (SDF foundation)
                                       |
                                       v
                                  Phase 3 (terrain quality)
                                       |
                                       v
                              Phase 4 (coloring) -----> Phase 5 (atmosphere)
                                                              |
                                                              v
                                                        Phase 6 (volumetric)
                                                              |
                                                              v
                                                        Phase 7 (polish)
                                                              |
                                                              v
                                                        Phase 8 (physics, future)
```

Phases 4 and 5 could be developed in parallel (coloring doesn't depend on atmosphere, and vice versa).

## Key Differences from Original Plan

| Aspect | Original (Density + Surface Extraction) | New (SDF Raymarching) |
|--------|----------------------------------------|----------------------|
| Terrain evaluation | CPU (Rust) | GPU (WGSL) |
| Mesh generation | Dual Contouring / Marching Cubes | None (raymarched per pixel) |
| LOD system | Octree with screen-space error | Implicit (noise octave count) |
| Seam stitching | Required (Transvoxel / skirts) | Not needed |
| Async tasks | Mesh generation on compute pool | None |
| Triangle count | Varies by LOD | Zero (no terrain mesh) |
| Detail limit | Mesh resolution at max depth | Effectively unlimited |
| Physics | Mesh colliders from chunks | JIT local meshing (future) |
| Terrain authoring | Rust density field trait | Rust params + WGSL building blocks |
| New Rust crates needed | `isosurface` | None |
| Files removed | 5 (terrain, quadtree, chunk_mesh, lod, mesh_task) | Same 5 |
| Files added | planet_material.rs, noise.wgsl, planet_sdf.wgsl | Same |
