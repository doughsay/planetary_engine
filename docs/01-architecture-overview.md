# Architecture Overview: Density Field Planetary Engine

## Vision

Transform the current quadtree-based heightmap planet renderer into a volumetric density field engine capable of rendering an entire planetary system with multiple planets, caves, overhangs, and arbitrary terrain topology — all at scales ranging from orbital views down to walking on the surface.

## Current Architecture vs Target

### What We Have Now

- **Quadtree LOD** on 6 cube faces, projected to a sphere
- **Heightmap terrain** — single elevation per point, no caves/overhangs
- **Single planet** with atmosphere, starfield, galaxy backdrop
- **f32 precision** — works fine for one planet (radius 6360 km) but will break at interplanetary scales
- **2D surface mesh** — vertices displaced along sphere normals

### What We're Building

- **Octree LOD** — 3D spatial subdivision, not limited to surface
- **Density field** — implicit function defining inside/outside, enabling caves, overhangs, arches, floating rocks
- **Surface extraction** via Dual Contouring (primary) and/or Marching Cubes (fallback)
- **Multiple planets** — planetary system with different terrain configs per body
- **Floating origin** via `big_space` — 128-bit precision grid cells for interplanetary travel
- **Physics-ready surfaces** — extracted meshes suitable for collision geometry
- **Seamless LOD transitions** — no visible cracks between different-resolution chunks

## Core Architectural Pillars

### 1. Density Field (replaces `TerrainConfig`)

A density function `f(x, y, z) -> f32` where:
- Negative = inside solid
- Positive = outside (air)
- Zero = surface

For a planet, the base density is `length(pos - center) - radius`, modified by noise layers for terrain features. This naturally handles:
- Mountains (negative noise pushes surface outward)
- Caves (positive bubbles inside the planet)
- Overhangs (laterally-displaced noise)
- Multiple planets (different center/radius/noise per planet)

### 2. Octree LOD (replaces `FaceQuadtree`)

A sparse octree per planet, rooted at the planet's bounding cube:
- Subdivides based on camera distance (screen-space error, similar to current approach)
- Only subdivides nodes that contain the surface (density sign change)
- Leaf nodes are meshed via surface extraction
- Max depth determines ground-level resolution

### 3. Surface Extraction (replaces `chunk_mesh.rs`)

Each octree leaf node samples its density field on a regular grid and extracts a mesh:
- **Dual Contouring** (preferred): produces sharp features, fewer triangles, quad-dominant
- **Marching Cubes** (fallback): simpler, more robust, triangle-only

The `isosurface` crate provides both algorithms with zero dependencies.

### 4. Floating Origin via `big_space` (new)

`big_space` 0.12 (Bevy 0.18 compatible) provides:
- `GridCell<i64>` or `GridCell<i128>` components on every entity
- `FloatingOrigin` marker on the camera
- Automatic recentering when camera crosses cell boundaries
- Standard Bevy `Transform` for local offsets within cells

This solves f32 precision at interplanetary scales without custom matrix math.

### 5. Multi-Planet System with Orbital Mechanics (new)

A central star sits at the grid origin. Planets orbit it via Keplerian mechanics:
- `Star` entity at `GridCell(0, 0, 0)` — the system's center of mass
- `Planet` component with density config, radius, atmosphere settings
- `Orbit` component with semi-major axis, eccentricity, inclination, period
- Hierarchical orbits: moons orbit planets, planets orbit the star
- Own octree, own set of chunk entities per planet
- Own atmosphere mesh entity (existing system adapts naturally)
- Sun direction computed dynamically per planet from `star_pos - planet_pos`
- Time control resource for speeding up/pausing orbital motion

### 6. Seam Stitching (replaces current neighbor-snapping)

When adjacent octree leaves are at different LOD levels, the boundary between them needs stitching to prevent cracks. Approaches:
- **Transition cells** (Lengyel's Transvoxel): special cell configurations at LOD boundaries
- **Skirt geometry**: extend chunk edges downward to hide gaps (simpler but less clean)
- **Shared boundary sampling**: ensure adjacent chunks agree on boundary density values

### 7. Physics Integration (new, future)

Extracted surface meshes can be used directly as collision geometry:
- Mesh collider generation from chunk meshes
- Only generate colliders for chunks near the player/objects
- Collider LOD can be coarser than visual LOD

## System Diagram

```
                    ┌─────────────────────┐
                    │   big_space Grid     │
                    │  (FloatingOrigin)    │
                    └──────────┬──────────┘
                               │
                        ┌──────▼──────┐
                        │    Star     │
                        │ GridCell(0) │
                        └──────┬──────┘
                               │ Keplerian orbits
              ┌────────────────┼────────────────┐
              │                │                │
        ┌─────▼─────┐   ┌─────▼─────┐   ┌─────▼─────┐
        │  Planet A  │   │  Planet B  │   │  Planet C  │
        │  (Earth)   │   │  (Moon←A)  │   │  (Mars)    │
        └─────┬──────┘   └─────┬──────┘   └─────┬──────┘
              │                │                │
        ┌─────▼──────┐  ┌─────▼──────┐  ┌─────▼──────┐
        │  Octree     │  │  Octree     │  │  Octree     │
        │  LOD System │  │  LOD System │  │  LOD System │
        └─────┬──────┘  └─────┬──────┘  └─────┬──────┘
              │                │                │
        ┌─────▼──────┐  ┌─────▼──────┐  ┌─────▼──────┐
        │  Density    │  │  Density    │  │  Density    │
        │  Field      │  │  Field      │  │  Field      │
        └─────┬──────┘  └─────┬──────┘  └─────┬──────┘
              │                │                │
        ┌─────▼──────┐  ┌─────▼──────┐  ┌─────▼──────┐
        │  Surface    │  │  Surface    │  │  Surface    │
        │  Extraction │  │  Extraction │  │  Extraction │
        │  (DC / MC)  │  │  (DC / MC)  │  │  (DC / MC)  │
        └─────┬──────┘  └─────┬──────┘  └─────┬──────┘
              │                │                │
        ┌─────▼──────┐  ┌─────▼──────┐  ┌─────▼──────┐
        │  Chunk      │  │  Chunk      │  │  Chunk      │
        │  Meshes     │  │  Meshes     │  │  Meshes     │
        └─────┬──────┘  └─────┬──────┘  └─────┬──────┘
              │                │                │
        ┌─────▼──────┐  ┌─────▼──────┐
        │  Atmosphere │  │            │  (optional per planet)
        │  Shell      │  └────────────┘
        └────────────┘
```

## What We Keep

- **Camera system** (`camera.rs`) — 6DOF controls, mostly unchanged (add `big_space` integration)
- **Atmosphere shader** (`atmosphere.rs` + `atmosphere.wgsl`) — per-planet sphere mesh approach works perfectly for multi-planet
- **Starfield** (`starfield.rs` + `starfield.wgsl`) — unchanged
- **Galaxy** (`galaxy.rs` + `galaxy.wgsl`) — unchanged
- **Async mesh generation** (`mesh_task.rs`) — pattern stays, implementation changes for new mesh format
- **HDR pipeline** — `Exposure::SUNLIGHT` + `AcesFitted` tonemapping stays

## What We Replace

- `terrain.rs` -> `density.rs` (density field evaluation)
- `quadtree.rs` -> `octree.rs` (3D LOD structure)
- `chunk_mesh.rs` -> `surface_extraction.rs` (DC/MC meshing)
- `lod.rs` -> `planet_lod.rs` (octree-based LOD with per-planet octrees)
- Parts of `main.rs` -> `planet.rs` (planet setup, multi-planet spawning)

## What We Add

- `big_space` integration for floating origin
- `planet.rs` — planet components, spawning, configuration
- `orbit.rs` — Keplerian orbital mechanics, hierarchical orbits, time control
- `density.rs` — density field trait + implementations
- `octree.rs` — sparse octree data structure
- `surface_extraction.rs` — DC/MC mesh generation
- `seam.rs` — LOD boundary stitching
- Eventually: physics collider generation
