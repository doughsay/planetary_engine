# Implementation Phases

Each phase produces a working, runnable program. No phase should leave the project in a broken state for long.

---

## Phase 0: Foundation — `big_space` Integration ✅ COMPLETE

**Goal**: Add floating origin without breaking anything. The planet still renders exactly as before, but now lives in a `big_space` grid cell.

**What changed**: `main.rs` (BigSpaceRootBundle, Grid hierarchy, disable TransformPlugin)
**Deliverable**: Floating origin infrastructure in place, multi-planet spawning works

---

## Phase 1: Planet Abstraction ✅ COMPLETE

**Goal**: Extract planet-related code into a `Planet` component/plugin so we can spawn multiple planets.

**What changed**: New `planet.rs`, `lod.rs` (per-planet queries, `ChunkNode.planet` field), `main.rs` (multi-planet setup)
**Deliverable**: Two planets with independent LOD, terrain, and materials

---

## Phase 1.5: Orbital Mechanics ✅ COMPLETE

**Goal**: Planets orbit a central star. The sun becomes a physical entity at the origin, and each planet moves along a Keplerian orbit.

**What changed**: New `orbit.rs`, `main.rs` (Sun entity, micro-scale test system, camera tracking hotkeys)
**Deliverable**: Planets visibly orbiting a central star with hierarchical orbit support

---

## Phase 2: SDF Rendering Foundation ✅ COMPLETE

**Goal**: Replace the mesh-based terrain pipeline with GPU raymarching. A single planet rendered via sphere tracing with basic noise terrain.

**What changed**: New `planet_material.rs`, new WGSL shaders (`noise.wgsl`, `planet_sdf.wgsl`), adapted `planet.rs` + `main.rs`. Removed `terrain.rs`, `quadtree.rs`, `chunk_mesh.rs`, `lod.rs`, `mesh_task.rs`.
**Deliverable**: Bumpy spherical planet rendered via raymarching, more detail as you zoom in. SDF gradient normals, correct `frag_depth`, multi-planet rendering, dynamic sun direction all working.

---

## Phase 3: Terrain Quality — Crater System ✅ IN PROGRESS

**Goal**: Composable terrain layers that produce distinct planet types (moon-like, earth-like, mars-like) via uniform parameters in a single shader.

**Completed tasks**:
1. ✅ SDF gradient normals (finite differences, epsilon scaled by distance) — done in Phase 2
2. ✅ Correct `frag_depth` output for depth buffer integration — done in Phase 2
3. ✅ Multi-planet rendering with different SDF configs — done in Phase 2
4. ✅ Dynamic sun direction from orbital position — shader uniforms update from orbit
5. ✅ Voronoi cell crater system — `crater_profile()` (bowl + rim + central peak) + `crater_field()` (3D cell placement)
6. ✅ Three-tier multi-scale craters (large basins, medium craters, small pocks) with per-tier uniforms
7. ✅ `hash33()` pseudorandom function in noise library
8. ✅ Reusable crater system — any planet can opt in via `SdfConfig` (`crater_enabled` toggle)

**Remaining tasks**:
1. Ridge noise (`1 - abs(noise)`) for sharp mountain ridges
2. Domain warping for organic continental shapes
3. `DirectionalLight` entity tracking sun position (currently static transform)
4. Continental noise layering (separate frequency tiers)

**What changed**: `planet_sdf.wgsl` (crater functions, layered SDF composition), `noise.wgsl` (hash33), `planet_material.rs` (crater uniforms in `PlanetSdfUniforms` + `SdfConfig`), `main.rs` (renamed Earth/Moon to planet/satellite, crater config on main planet)
**Risk**: Low — iterative shader improvement
**Deliverable**: Moon-like planet with multi-scale craters, distinct from satellite's smooth FBM terrain

---

## Phase 4: Terrain Coloring + Biomes

**Goal**: Planets with visual variety — oceans, grasslands, mountains, snow.

**Tasks**:
1. Elevation-based coloring (ocean blue, lowland green, mountain gray, snow caps)
2. Slope-based coloring (steep cliffs vs flat surfaces)
3. Latitude-based variation (polar ice, tropical zones)
4. Per-planet color palette (Earth-like, Mars-like, Moon-like)
5. Ocean rendering (flat water plane within the SDF, specular reflections)

**What changes**: `planet_sdf.wgsl` (coloring functions), `planet_material.rs` (color config uniforms)
**Risk**: Low — shader art iteration
**Deliverable**: Visually distinct, colorful planets

---

## Phase 5: Atmosphere Re-integration

**Goal**: The existing atmosphere shader working with SDF terrain.

**Tasks**:
1. Re-add atmosphere entities per planet (icosphere + `AtmosphereMaterial`)
2. Camera gets `DepthPrepass` component — atmosphere shader reads terrain depth
3. Verify atmosphere correctly clips at terrain surface (no sky below ground)
4. Dynamic sun direction in atmosphere uniforms from orbital positions
5. Multi-planet atmospheres (Earth has one, Moon doesn't)

**What changes**: `main.rs` (atmosphere entity spawning), `atmosphere.rs` (sun direction update system)
**Risk**: Low — atmosphere shader already supports depth prepass
**Deliverable**: Planets with terrain + atmosphere

---

## Phase 6: Volumetric Features

**Goal**: Terrain that heightmaps can't do — caves, overhangs, arches.

**Tasks**:
1. Cave carving in WGSL (3D noise threshold + Boolean subtraction)
2. Overhang generation (domain-warped noise displacing the surface laterally)
3. Tune parameters for natural-looking cave entrances
4. Test and adjust raymarching convergence inside caves (may need more steps or bi-directional marching)
5. Expose cave/overhang parameters in Rust `SdfConfig`

**What changes**: `planet_sdf.wgsl` (cave functions), `planet_material.rs` (cave config uniforms)
**Risk**: Medium — ray marching inside caves requires careful tuning for convergence
**Deliverable**: Fly into caves, walk under arches

---

## Phase 7: Performance + Polish

**Goal**: Smooth performance at all scales, demo-ready.

**Tasks**:
1. Profile shader — optimize noise evaluation, tune step count vs quality
2. Early ray termination (fragments behind planet, beyond max distance)
3. Bounding sphere optimization for multi-planet (skip planets too far for detail)
4. Screen-space adaptive step size
5. Demo planetary system: Earth-like + Moon + rocky planet with craters

**What changes**: Shader optimization, `main.rs` (demo system configuration)
**Risk**: Low — optimization and tuning
**Deliverable**: Polished, performant planetary system

---

## Phase 8 (Future): Physics via JIT Local Meshing

**Goal**: Physical objects can interact with terrain.

**Tasks**:
1. Duplicate the SDF composition in Rust (must match WGSL output exactly)
2. Small 3D grid around physics objects, evaluate density on CPU
3. Marching Cubes on the CPU grid -> invisible collision mesh
4. Feed to physics engine (avian/rapier)
5. Tiered simulation: full physics near player, simplified at mid-range, frozen at distance

**What changes**: New `density.rs` (CPU SDF), new `physics_mesh.rs` (JIT meshing), physics crate dependency
**Risk**: High — CPU/GPU SDF parity, physics integration complexity
**Deliverable**: Walk on the planet surface, physics objects land on terrain

---

## Phase Dependency Graph

```
Phase 0 done --> Phase 1 done --> Phase 1.5 done (orbits)
                                       |
                                       v
                                  Phase 2 (SDF foundation)
                                       |
                                       v
                                  Phase 3 (terrain quality + lighting)
                                       |
                                       v
                              Phase 4 (coloring) -----> Phase 5 (atmosphere)
                                       |                      |
                                       v                      v
                                  Phase 6 (volumetric features)
                                       |
                                       v
                                  Phase 7 (performance + polish)
                                       |
                                       v
                                  Phase 8 (physics, future)
```

Phases 4 and 5 can be developed in parallel (coloring doesn't depend on atmosphere, and vice versa). Phase 6 benefits from both being complete.
