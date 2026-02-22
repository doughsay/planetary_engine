# Architecture Overview: SDF Planet Rendering Engine

## Vision

Transform the current quadtree-based heightmap planet renderer into an SDF raymarching engine capable of rendering an entire planetary system with caves, overhangs, and arbitrary terrain topology — all at scales ranging from orbital views down to walking on the surface, with effectively infinite detail.

## Current Architecture vs Target

### What We Have Now (Phases 0, 1, 1.5 complete)

- **Quadtree LOD** on 6 cube faces, projected to a sphere
- **Heightmap terrain** — single elevation per point, no caves/overhangs
- **Multiple planets** with per-planet LOD and materials
- **Floating origin** via `big_space` — grid cells for interplanetary scales
- **Orbital mechanics** — Keplerian solver, hierarchical orbits
- **Starfield + Galaxy backdrop** — procedural rendering
- **2D surface mesh** — vertices displaced along sphere normals

### What We're Building

- **SDF raymarching** — terrain evaluated per-pixel on the GPU, no mesh generation
- **Signed Distance Field** — implicit function defining terrain shape, enabling caves, overhangs, arches
- **Distance-based LOD** — noise octaves scale with camera distance (implicit, no octree management)
- **Parameterized terrain** — WGSL building blocks driven by Rust configuration
- **Atmosphere integration** — existing atmosphere shader reads SDF depth for correct clipping
- **Physics-ready path** — future JIT local meshing for collision geometry

## Core Architectural Pillars

### 1. SDF Evaluation (replaces `TerrainConfig`)

A signed distance function `sdf(p) -> f32` where:
- Negative = inside solid
- Positive = outside (air)
- Zero = surface

For a planet: `sdf(p) = length(p - center) - radius - terrain_noise(normalize(p - center))`, modified by noise layers. This naturally handles:
- Mountains (noise pushes surface outward)
- Caves (3D noise carving interior volumes via Boolean subtraction)
- Overhangs (domain-warped noise)
- Multiple planets (different center/radius/noise per planet)

The SDF is evaluated entirely in WGSL. Rust controls the configuration (frequencies, amplitudes, layer enables) via uniform buffers.

### 2. Sphere-Mesh Raymarching (replaces quadtree + chunk mesh)

Each planet spawns an icosphere mesh at its bounding radius (planet_radius + max_elevation). The fragment shader:

1. Computes a ray from camera through the fragment
2. Sphere-traces inward using the SDF to guide step sizes
3. Finds the terrain surface (SDF = 0)
4. Computes normals via SDF gradient (finite differences)
5. Lights the surface and outputs color + depth

This is the same rendering pattern as the existing atmosphere shader. No mesh generation, no octree, no seam stitching.

### 3. Distance-Based Octave LOD (replaces octree LOD system)

The number of FBM noise octaves evaluated scales automatically with distance:
- Each octave's feature size is compared to the pixel size at the current distance
- Sub-pixel octaves are skipped (they'd be invisible anyway)
- At orbital distance: 2-3 octaves (smooth sphere with continents)
- At mountain scale: 6-8 octaves (mountain ridges, valleys)
- At ground level: 12-16 octaves (rocks, small features)

No explicit LOD management needed. The shader handles it automatically.

### 4. Floating Origin via `big_space` (complete)

`big_space` 0.12 provides grid-cell positioning for interplanetary scales. Already integrated in Phases 0/1.

### 5. Multi-Planet System with Orbital Mechanics (complete)

Keplerian orbital mechanics with hierarchical orbits. Already integrated in Phase 1.5.

### 6. Physics Integration (future, JIT Local Meshing)

When physics is needed (Phase 8), the approach is:
- Duplicate the SDF composition in Rust (same math as WGSL)
- For physics objects, evaluate the SDF on a small 3D grid around the object on the CPU
- Extract collision geometry via Marching Cubes
- Feed the invisible mesh to the physics engine
- Tiered simulation: full physics near player, simplified at mid-range, frozen at distance

See `SDF-vs-surface-extraction.md` for detailed analysis of this approach.

## System Diagram

```
                    +---------------------+
                    |   big_space Grid    |
                    |  (FloatingOrigin)   |
                    +----------+----------+
                               |
                        +------v------+
                        |    Star     |
                        | CellCoord(0)|
                        +------+------+
                               | Keplerian orbits
              +----------------+----------------+
              |                |                |
        +-----v-----+   +-----v-----+   +-----v-----+
        |  Planet A  |   |  Planet B  |   |  Planet C  |
        |  (Earth)   |   |  (Moon<-A) |   |  (Mars)    |
        +-----+------+   +-----+------+   +-----+------+
              |                |                |
        +-----v------+  +-----v------+  +-----v------+
        | SDF Shader  |  | SDF Shader  |  | SDF Shader  |
        | (per-pixel  |  | (per-pixel  |  | (per-pixel  |
        |  raymarch)  |  |  raymarch)  |  |  raymarch)  |
        +-----+------+  +-----+------+  +-----+------+
              |                |                |
        +-----v------+  +-----v------+
        | Atmosphere  |  |            |  (optional per planet)
        | (raymarch)  |  +------------+
        +------------+
```

## What We Keep

- **Camera system** (`camera.rs`) — 6DOF controls, unchanged
- **Starfield** (`starfield.rs` + `starfield.wgsl`) — unchanged
- **Galaxy** (`galaxy.rs` + `galaxy.wgsl`) — unchanged
- **Orbital mechanics** (`orbit.rs`) — unchanged
- **Planet component** (`planet.rs`) — adapted for SDF config
- **HDR pipeline** — `Exposure::SUNLIGHT` + `AcesFitted` tonemapping stays
- **Atmosphere concept** — re-integrated with SDF depth output

## What We Replace

- `terrain.rs` -> removed (terrain noise moves to WGSL)
- `quadtree.rs` -> removed (no spatial LOD structure needed)
- `chunk_mesh.rs` -> removed (no mesh generation)
- `lod.rs` -> removed (LOD is implicit in shader octave count)
- `mesh_task.rs` -> removed (no async mesh tasks)

## What We Add

- `planet_material.rs` — Bevy `Material` for SDF terrain rendering
- `assets/shaders/noise.wgsl` — reusable 3D simplex noise library
- `assets/shaders/planet_sdf.wgsl` — terrain SDF, sphere tracing, lighting, coloring
