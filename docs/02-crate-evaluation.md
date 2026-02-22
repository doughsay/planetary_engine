# Crate Evaluation

## Required Crates

### `big_space` 0.12 — Floating Origin

**Status: Ready to use (Bevy 0.18 compatible)**

- **Purpose**: Floating origin system that extends Bevy's Transform with integer grid cells for arbitrary precision
- **Version**: 0.12.0 (supports Bevy 0.18)
- **Key types**:
  - `BigSpacePlugin<P>` — plugin parameterized by grid precision (i8 to i128)
  - `GridCell<P>` — component storing integer cell coordinates
  - `FloatingOrigin` — marker component for the camera
- **How it works**: Space is divided into a grid of cells. Each entity gets a `GridCell` (which cell it's in) plus a standard `Transform` (offset within the cell). When the camera crosses a cell boundary, the world recenters so the camera stays near the origin. `GlobalTransform` is computed from the grid cell offset relative to the floating origin.
- **Why we need it**: f32 precision breaks down at ~10km from origin. Planetary systems span millions of km. `big_space` with `i64` cells gives us effectively infinite range.
- **No added dependencies** — zero extra transitive deps.
- **Links**: [crates.io](https://crates.io/crates/big_space), [GitHub](https://github.com/aevyrie/big_space), [docs.rs](https://docs.rs/big_space)

### `isosurface` 0.1.0-alpha.0 — Surface Extraction

**Status: Usable, zero-dependency, provides both DC and MC**

- **Purpose**: Isosurface extraction from density/distance fields
- **Version**: 0.1.0-alpha.0 (Bevy-independent, pure algorithms)
- **Algorithms provided**:
  - `MarchingCubes` — standard isosurface extraction
  - `ExtendedMarchingCubes` — enhanced accuracy variant
  - `LinearHashedMarchingCubes` — octree-optimized marching cubes
  - `DualContouring` — sharp feature preservation, quad-dominant output
  - Point cloud extraction
- **Key API pattern**: Source trait (density function) -> Sampler -> Algorithm -> Extractor (mesh output)
- **Zero dependencies** — pure Rust, no runtime deps
- **Why we need it**: Provides battle-tested surface extraction without reinventing the wheel. We can start with Marching Cubes for simplicity, then switch to Dual Contouring for better quality.
- **Links**: [crates.io](https://crates.io/crates/isosurface), [GitHub](https://github.com/swiftcoder/isosurface), [docs.rs](https://docs.rs/isosurface)

### `noise` 0.9 — Procedural Noise (already in use)

**Status: Already a dependency**

- Provides Fbm, SuperSimplex, and other noise functions
- Will continue to be used for density field perturbation
- No changes needed

## Considered But Not Selected

### `fast-surface-nets` 0.2.1

- Fast Surface Nets implementation (~20M triangles/sec)
- Produces smoother output than Marching Cubes but lacks sharp features
- Chunk-friendly API designed for voxel engines
- **Decision**: The `isosurface` crate covers our needs with more algorithm options. Could revisit if we need maximum meshing throughput.
- [crates.io](https://crates.io/crates/fast-surface-nets)

### `tessellation` 0.8.2

- Implements Manifold Dual Contouring
- More actively maintained than `isosurface`
- **Decision**: Worth evaluating alongside `isosurface` if we find its DC implementation lacking. Keep as a backup option.
- [crates.io](https://crates.io/crates/tessellation)

### `building-blocks`

- Comprehensive voxel library (storage, LOD, meshing)
- By bonsairobo (same author as `fast-surface-nets`)
- **Decision**: Possibly unmaintained, very opinionated about data layout. We prefer building our own octree to keep control over the LOD strategy.
- [GitHub](https://github.com/bonsairobo/building-blocks)

### `dual_contouring` (soundeffects)

- General-purpose dual contouring
- **Decision**: Archived on November 2025, incomplete implementation. Not suitable.
- [GitHub](https://github.com/soundeffects/dual_contouring)

### `voxelis`

- Sparse Voxel Octree DAG engine
- GreedyMesh, GPU frustum culling
- **Decision**: Interesting but very opinionated, would conflict with our own architecture. Better to build our octree from scratch.
- [GitHub](https://github.com/WildPixelGames/voxelis)

## Dependency Plan

### Phase 1 (immediate)
```toml
[dependencies]
bevy = "0.18.0"
big_space = "0.12.0"
noise = "0.9.0"
```

### Phase 2 (when we start surface extraction)
```toml
[dependencies]
bevy = "0.18.0"
big_space = "0.12.0"
noise = "0.9.0"
isosurface = "0.1.0-alpha.0"
```

### Notes

- We keep the dependency count minimal — just 3-4 crates total
- Both `big_space` and `isosurface` are zero-dependency crates (excluding Bevy itself)
- Our octree is custom-built (like our current quadtree) for full control over LOD strategy
- The `noise` crate continues to drive procedural generation
- If `isosurface`'s DC implementation proves insufficient, `tessellation` is our backup
