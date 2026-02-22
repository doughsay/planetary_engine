# Crate Evaluation

## Current Dependencies

### `big_space` 0.12 — Floating Origin

**Status: In use (Bevy 0.18 compatible)**

- **Purpose**: Floating origin system that extends Bevy's Transform with integer grid cells for arbitrary precision
- **Version**: 0.12.0 (supports Bevy 0.18)
- **Key types**: `CellCoord`, `Grid`, `FloatingOrigin`, `BigSpace`, `BigSpaceRootBundle`
- **Why we need it**: f32 precision breaks down at ~10km from origin. Planetary systems span millions of km. `big_space` with `i64` cells gives us effectively infinite range.
- **No added dependencies** — zero extra transitive deps.

### `noise` 0.9 — Procedural Noise (Rust side)

**Status: In use, but role changing**

- Provides Fbm, SuperSimplex, and other noise functions
- Currently drives terrain generation on the CPU
- With SDF raymarching, terrain noise moves to WGSL (GPU). The `noise` crate is no longer needed for rendering.
- **Keep for now**: useful for prototyping, CPU-side terrain evaluation (future physics), and any non-shader procedural generation.
- **May remove later** if all noise evaluation moves to GPU.

## No Longer Needed

### `isosurface` — Surface Extraction

**Status: Removed from plan**

With SDF raymarching, terrain is rendered per-pixel on the GPU. No mesh extraction needed. The `isosurface` crate (Marching Cubes, Dual Contouring) is no longer part of the pipeline.

If physics (Phase 8) requires JIT local meshing, Marching Cubes would be needed on the CPU. At that point, re-evaluate whether to use `isosurface` or a simpler custom implementation for the small grids involved.

## Considered But Not Selected

### `fast-surface-nets` 0.2.1

- Fast Surface Nets implementation (~20M triangles/sec)
- **Decision**: Not needed — SDF raymarching eliminates mesh generation.

### `tessellation` 0.8.2

- Manifold Dual Contouring
- **Decision**: Not needed for same reason. May revisit for Phase 8 physics meshing.

### `building-blocks`

- Comprehensive voxel library
- **Decision**: Not needed — no voxel storage or octree in the SDF approach.

## Current Dependency List

```toml
[dependencies]
bevy = "0.18.0"
big_space = "0.12.0"
noise = "0.9.0"
```

Minimal — just 3 crates total. The SDF approach adds no new Rust dependencies; all new complexity is in WGSL shaders.

## Future Dependencies (Phase 8: Physics)

When physics integration is needed:
```toml
[dependencies]
avian3d = "0.x"  # or bevy_rapier3d — evaluate when the time comes
```

May also need a CPU-side marching cubes implementation for JIT local meshing. Evaluate `isosurface` or custom implementation at that time.
