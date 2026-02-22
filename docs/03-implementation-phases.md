# Implementation Phases

This is a large architectural shift. We break it into incremental phases where each phase produces a working, runnable program. No phase should leave the project in a broken state for long.

---

## Phase 0: Foundation — `big_space` Integration ✅ COMPLETE

**Goal**: Add floating origin without breaking anything. The planet still renders exactly as before, but now lives in a `big_space` grid cell.

**Tasks**:
1. ✅ Add `big_space = "0.12.0"` to Cargo.toml
2. ✅ Add `BigSpaceDefaultPlugins` to the app (replaces `TransformPlugin`)
3. ✅ Add `CellCoord` to the camera entity, mark it with `FloatingOrigin`
4. ✅ Add `CellCoord` to planet entities via `spawn_grid_default` / `spawn_spatial`
5. ✅ Rendering works with big_space
6. ⚠️ Far-distance precision not explicitly stress-tested yet

**What changed**: `main.rs` (BigSpaceRootBundle, Grid hierarchy, disable TransformPlugin)
**API notes**: big_space 0.12 uses `CellCoord` (not `GridCell<i64>`), `BigSpaceDefaultPlugins` (not `BigSpacePlugin`), and `BigSpaceRootBundle` for the root entity. See CLAUDE.md for detailed big_space API notes.
**Deliverable**: Floating origin infrastructure in place, multi-planet spawning works

---

## Phase 1: Planet Abstraction ✅ COMPLETE

**Goal**: Extract planet-related code into a `Planet` component/plugin so we can spawn multiple planets.

**Tasks**:
1. ✅ Create `planet.rs` with `Planet` component (holds TerrainConfig)
2. ✅ Create `PlanetPlugin` (registered, currently empty — systems live in lod.rs)
3. ✅ Planet spawning in `main.rs` setup_scene (Earth + Moon)
4. ✅ Two planets (Earth + Moon) with different terrain configs, radii, and materials
5. ✅ LOD systems operate per-planet — `update_lod` queries `With<Planet>`, each planet has its own `PlanetQuadtree`

**What changed**: New `planet.rs`, `lod.rs` (per-planet queries, `ChunkNode.planet` field), `main.rs` (multi-planet setup)
**Note**: Atmosphere scrapped for now — will be re-added much later after the terrain pipeline rewrite.
**Deliverable**: Two planets with independent LOD, terrain, and materials

---

## Phase 1.5: Orbital Mechanics ✅ COMPLETE

**Goal**: Planets orbit a central star. The sun becomes a physical entity at the origin, and each planet moves along a Keplerian orbit.

**Tasks**:
1. ✅ Create `orbit.rs` with `Orbit` component:
   - Full Keplerian parameters (semi-major axis, eccentricity, inclination, LAN, arg of periapsis, period, initial mean anomaly)
   - `OrbitalTime` resource with speed multiplier
   - `update_orbits` system: f64 Keplerian solver → `Grid::translation_to_grid()` → CellCoord + Transform
   - Hierarchical orbits via `parent: Option<Entity>` — resolved same-frame (no 1-frame lag)
2. ✅ Sun as a visible entity:
   - Emissive sphere mesh at origin with `Sun` marker component
   - Spawned via `spawn_spatial` in the root grid
3. ⬚ Per-planet `sun_direction` — **deferred (atmosphere scrapped for now)**
4. ⬚ Dynamic `DirectionalLight` tracking — **deferred, light still hardcoded at (5000, 5000, 5000)**
5. ✅ Camera tracking hotkeys: hold 1/2/3 to smoothly track Sun/Earth/Moon
6. ✅ Test: Moon orbits Earth, Earth orbits Sun, visible from camera child of Earth

**What changed**: New `orbit.rs`, `main.rs` (Sun entity, micro-scale test system, camera tracking hotkeys)
**Remaining gaps**:
- Directional light doesn't track the Sun (terrain lit from fixed direction)
- No time control keyboard shortcuts (speed is set to 1.0 in code)
- Per-planet sun_direction deferred (atmosphere scrapped for now, will revisit later)

**Key implementation detail**: Orbital positions (in world units / km) must be converted to CellCoord via `Grid::translation_to_grid()`, NOT via `floor()`. `Grid::default()` has `cell_edge_length = 2000`, so naive `floor()` places entities 2000x too far apart.

**Current test system** (micro scale for visual verification):
- Sun: radius 2000, at origin
- Earth: radius 1000, orbit 15,000 km, period 30s
- Moon: radius 300, orbit 4,000 km around Earth, period 10s
- Camera: child of Earth, 8,000 km above surface

**Deliverable**: Planets visibly orbiting a central star with hierarchical orbit support

---

## Phase 2: Density Field

**Goal**: Replace heightmap terrain with a density field. Still using the existing quadtree/surface mesh for now — the density field just drives elevation.

**Tasks**:
1. Create `density.rs` with a `DensityField` trait:
   ```rust
   pub trait DensityField: Send + Sync {
       fn sample(&self, pos: Vec3) -> f32;        // negative = inside
       fn gradient(&self, pos: Vec3) -> Vec3;      // for normals
   }
   ```
2. Implement `SphericalDensity` — base sphere + noise layers (port from `TerrainConfig`)
3. Refactor `TerrainConfig::get_displaced_position()` to use the density field internally
4. Verify terrain looks the same as before (density field wrapping the existing noise)
5. Add a simple cave/overhang noise layer to prove the density field can represent non-heightmap geometry (this will look broken with the quadtree mesh — that's expected and motivates Phase 3)

**What changes**: New `density.rs`, `terrain.rs` (refactored to use density)
**What doesn't change**: Quadtree, chunk mesh, LOD, atmosphere, camera
**Risk**: Low — the density field is a pure data abstraction, doesn't affect rendering pipeline
**Deliverable**: Same terrain appearance (validating density field correctness), plus a demonstration of cave geometry that shows why we need volumetric meshing

---

## Phase 3: Octree Data Structure

**Goal**: Build the sparse octree that will replace the quadtree for LOD management.

**Tasks**:
1. Create `octree.rs` with core types:
   - `OctreeNodeId` — uniquely identifies a node (depth + position)
   - `OctreeNode` — Leaf or Split(children)
   - `SparseOctree` — HashMap-based tree (similar to current `FaceQuadtree` pattern)
2. Implement octree operations:
   - `split(node)`, `merge(node)`, `children(node)`, `parent(node)`
   - `neighbors(node)` — 6-face, 12-edge, 8-corner adjacency
   - `bounds(node)` -> AABB in local planet space
   - `contains_surface(node, density_field)` -> bool (sign change detection)
3. Implement screen-space error metric (adapt from current `lod.rs`):
   - `geometric_error = node_size / CHUNK_RESOLUTION`
   - `pixel_error = geometric_error / distance * PERSPECTIVE_SCALE`
4. Unit tests for octree operations, especially neighbor lookups
5. **Do not yet integrate with rendering** — this is pure data structure work

**What changes**: New `octree.rs`
**What doesn't change**: Everything else still uses the quadtree
**Risk**: Low — new code with no integration yet
**Deliverable**: Tested octree data structure ready for LOD integration

---

## Phase 4: Surface Extraction

**Goal**: Generate meshes from the density field within octree leaf nodes using Dual Contouring or Marching Cubes.

**Tasks**:
1. Add `isosurface = "0.1.0-alpha.0"` to Cargo.toml
2. Create `surface_extraction.rs`:
   - Implement the `isosurface` Source trait for our `DensityField`
   - Function: `extract_mesh(density_field, node_bounds, resolution) -> Mesh`
   - Support both DC and MC (feature flag or runtime switch)
3. Adapt output to Bevy `Mesh`:
   - Vertex positions, normals, indices
   - UV coordinates (triplanar projection from world position)
4. Async mesh generation (adapt `mesh_task.rs` pattern)
5. Test with a standalone sphere density to verify correct meshes
6. Test with the noise-perturbed planet density to verify terrain quality

**What changes**: New `surface_extraction.rs`, `mesh_task.rs` (adapted), Cargo.toml
**What doesn't change**: Still using quadtree for rendering; this is building the replacement pipeline
**Risk**: Medium — isosurface crate API may need adaptation, mesh quality tuning needed
**Deliverable**: Ability to generate correct meshes from density fields, not yet wired into rendering

---

## Phase 5: Octree LOD System (The Big Switch)

**Goal**: Replace the quadtree LOD system with the octree LOD system. This is the most impactful phase.

**Tasks**:
1. Create `planet_lod.rs` — octree-based LOD systems:
   - `update_octree_lod` — evaluate screen error, split/merge decisions
   - `sync_octree_chunks` — spawn/despawn chunk entities for leaf nodes
   - `request_octree_meshes` — trigger async mesh generation for new leaves
   - `poll_octree_meshes` — collect completed meshes
2. Chunk entity structure:
   - `OctreeChunk { planet: Entity, node_id: OctreeNodeId }`
   - `PendingMesh(Task<Mesh>)` (same pattern as current)
   - `RetainUntilChildrenReady` (same pattern)
3. Only mesh leaves that contain the surface (skip fully inside/outside nodes)
4. Apply per-planet material (initially `StandardMaterial` with vertex colors or triplanar texturing)
5. Wire into the Bevy schedule, replacing the old quadtree systems
6. Remove old quadtree systems from the schedule (keep files for reference initially)

**What changes**: New `planet_lod.rs`, `main.rs` (system registration), old `lod.rs` disabled
**What gets removed (eventually)**: `quadtree.rs`, `chunk_mesh.rs`, `lod.rs`, `terrain.rs`
**Risk**: High — this is the critical switchover. Plan for a period where the old and new systems coexist.
**Deliverable**: Planet rendered via octree + density field + surface extraction

---

## Phase 6: Seam Stitching

**Goal**: Eliminate cracks between chunks at different LOD levels.

**Tasks**:
1. Evaluate approaches:
   - **Transvoxel** (Lengyel): transition cells at LOD boundaries — cleanest results
   - **Skirt geometry**: extend edges below surface — simplest to implement
   - **Boundary agreement**: force adjacent chunks to share boundary samples — moderate complexity
2. Implement chosen approach in `seam.rs`
3. Integrate with mesh generation pipeline (seam info passed alongside density samples)
4. Verify visually: fly around the planet looking for cracks at LOD boundaries
5. Stress test: rapid camera movement to force frequent LOD transitions

**What changes**: New `seam.rs`, `surface_extraction.rs` (boundary handling)
**Risk**: Medium — seam stitching is a known hard problem, but well-documented solutions exist
**Deliverable**: Visually seamless LOD transitions

---

## Phase 7: Polish and Cleanup

**Goal**: Remove legacy code, optimize performance, add planetary system features.

**Tasks**:
1. Delete old files: `quadtree.rs`, `chunk_mesh.rs`, `lod.rs`, `terrain.rs` (if not already done)
2. Update `CLAUDE.md` with new architecture documentation
3. Performance tuning:
   - Chunk generation budget per frame
   - LOD distance thresholds
   - Mesh resolution per chunk
4. Add per-planet configuration variety:
   - Different noise parameters
   - Different radii
   - Different atmosphere colors/densities
5. Create a demo planetary system (earth-like planet + moon + rocky planet)

---

## Phase 8 (Future): Physics

**Goal**: Enable physics interactions on planet surfaces.

**Tasks**:
1. Evaluate physics crates (`avian` for Bevy 0.18, or `bevy_rapier`)
2. Generate collision meshes from chunk meshes (can be lower resolution)
3. Only generate colliders for chunks near physical entities
4. Handle collider updates when LOD changes
5. Gravity toward planet center based on distance

---

## Recommended Next Step

**Phases 0, 1, and 1.5 are complete.** The next step is **Phase 2** (density field) or **Phase 3** (octree), which can be developed in parallel. The multi-planet + orbital infrastructure is in place, so the new density/octree system can be tested on the second planet (Moon) while keeping Earth on the old quadtree system.

---

## Parallel Work Opportunities

Some phases have independent sub-tasks that can be developed in parallel:

- **Phase 2 (density) + Phase 3 (octree)**: These are independent data structures. The density field doesn't depend on the octree, and vice versa. Both can be built and tested in isolation before they're combined in Phase 4.
- **Phase 4 (surface extraction)** depends on both Phase 2 and Phase 3.
- **Phase 6 (seam stitching)** can be researched during Phases 3-5.

```
Phase 0 ✅ ──> Phase 1 ✅ ──┬──> Phase 1.5 ✅ (orbits)
                             │
                             ├──> Phase 2 (density) ──┬──> Phase 4 ──> Phase 5 ──> Phase 6 ──> Phase 7
                             │                        │
                             └──> Phase 3 (octree) ───┘
```

Phases 0, 1, and 1.5 are complete. Phase 2 (density) and Phase 3 (octree) can be developed in parallel.
