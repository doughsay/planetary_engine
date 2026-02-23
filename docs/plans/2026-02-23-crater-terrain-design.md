# Crater Terrain Design — Moon-Like Planetary Surfaces

**Date**: 2026-02-23
**Phase**: 3 (Terrain Quality), first pass
**Scope**: Voronoi cell crater system + entity rename

---

## Goal

Add a procedural crater system as a composable terrain layer in the SDF shader. The main planet (currently "Earth") becomes the test subject with moon-like cratered terrain. This proves the "one shader, different configs" architecture for supporting diverse planet types (moon-like, earth-like, mars-like) via uniform parameters rather than separate shaders.

## Context

- Solar system style: KSP-like — fictional planets, believable but not photorealistic
- Crater style: naturalistic but slightly stylized (between realistic lunar and simplified)
- The existing FBM roughness layer continues to work alongside craters
- Planets with `crater_enabled = 0` pay zero GPU cost for crater evaluation

## Rename: Earth/Moon → Planet/Satellite

The current "Earth" and "Moon" entities are placeholders for fictional planets. To avoid confusion, rename throughout the codebase:
- "Earth" → generic planet (no real-world name)
- "Moon" → satellite
- Update marker components, comments, variable names, camera tracking hotkey labels

## Crater Profile Function

Each crater is a 1D radial function of `r` (normalized distance from crater center, `r=0` at center, `r=1` at rim edge):

- **Bowl**: `depth * (r^2 - 1)` for `r < 1` — parabolic depression, deepest at center, zero at rim
- **Rim**: `rim_height * exp(-(r - 1.0)^2 / rim_width^2)` — Gaussian bump just outside the bowl
- **Central peak**: `peak_height * exp(-r^2 / peak_width^2)` — narrow Gaussian at center, only for larger craters (medium tier and above)
- **Smooth falloff**: `smoothstep` blend to zero beyond the rim to avoid hard edges

Final displacement: `bowl + rim + peak`, all smoothly blended.

## Spatial Placement: Voronoi Cell Grid

Craters are placed using a cell-based scheme on the **direction vector** (`normalize(p - planet_center)`), avoiding pole distortion from UV-based approaches.

1. Scale direction vector by `cell_frequency`, `floor()` to get cell ID
2. Hash cell ID to get: jittered crater center position, size variation, whether a crater exists (`hash < density`)
3. Check current cell + 26 neighbors (3x3x3) for nearby craters
4. For each crater found, evaluate the profile function at the sample point's distance to the crater center
5. Sum displacements (deepest bowl wins where craters overlap, rims and peaks accumulate)

## Multi-Scale Tiers

Three tiers evaluated independently and summed:

| Tier | Cell Frequency | Crater Character | Density | Central Peak |
|------|---------------|------------------|---------|--------------|
| Large (tier 0) | ~6-10 cells | Deep basins, prominent rims | 0.3-0.5 | Yes |
| Medium (tier 1) | ~20-40 cells | Standard craters, smaller rims | 0.4-0.6 | Small |
| Small (tier 2) | ~80-150 cells | Pockmarks, simple bowls | 0.5-0.7 | No |

Small craters naturally sit inside large basin floors. The existing FBM roughness adds texture to crater floors and rims.

## Shader Composition

`planet_sdf()` changes from single FBM to layered:

```
elevation = fbm(...) * amplitude
if crater_enabled:
    elevation += crater_field(dir, tier_0_params)
    elevation += crater_field(dir, tier_1_params)
    elevation += crater_field(dir, tier_2_params)
return length(p - center) - (radius + elevation)
```

`crater_field()` performs the Voronoi cell search + profile evaluation for one tier.

## Uniform / Config Data Layout

Flat fields in `PlanetSdfUniforms` (3 tiers x 5 params + 1 toggle = 16 floats + 1 u32):

```
crater_enabled: u32              — master toggle
crater_cell_frequency_0/1/2: f32 — cell grid density per tier
crater_depth_0/1/2: f32          — bowl depth per tier
crater_rim_height_0/1/2: f32     — rim raise amount per tier
crater_peak_height_0/1/2: f32    — central peak height per tier (0 for small tier)
crater_density_0/1/2: f32        — probability [0,1] a cell contains a crater
```

`SdfConfig` on the Rust side mirrors these as plain fields with a `Default` that has craters disabled.

## Files Touched

- `assets/shaders/planet_sdf.wgsl` — `crater_profile()`, `crater_field()`, compose into `planet_sdf()`
- `assets/shaders/noise.wgsl` — `hash33()` helper for cell-based pseudorandom values
- `src/planet_material.rs` — expand `PlanetSdfUniforms` and `SdfConfig` with crater fields
- `src/planet.rs` — pass crater params to uniforms in `update_planet_materials`
- `src/main.rs` — rename Earth/Moon, give main planet crater-heavy config

## Not In Scope (Deferred)

- Ridge noise, domain warping (later Phase 3 pass)
- Terrain coloring / biomes (Phase 4)
- DirectionalLight tracking from sun position (separate task)
- Continental noise layering

## Success Criteria

- Main planet has visually convincing multi-scale craters with bowls, rims, and central peaks on large craters
- Satellite renders unchanged with smooth FBM terrain
- Both planets render correctly with proper depth and lighting
- `crater_enabled = 0` completely skips crater evaluation (no perf cost)
