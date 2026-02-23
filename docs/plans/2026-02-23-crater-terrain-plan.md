# Crater Terrain Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add procedural Voronoi-cell crater fields as a composable terrain layer in the SDF shader, making the main planet look moon-like.

**Architecture:** Craters are placed via a Voronoi cell grid on the unit-sphere direction vector. Each cell may contain a crater with a parameterized profile (bowl + rim + central peak). Three scale tiers (large basins, medium craters, small pocks) are evaluated independently and summed. Crater parameters live in `SdfConfig` / `PlanetSdfUniforms` so any planet can opt in.

**Tech Stack:** Rust + Bevy 0.18 + WGSL shaders. See `CLAUDE.md` for Bevy 0.18 API notes and `docs/plans/2026-02-23-crater-terrain-design.md` for the full design rationale.

**Key reference:** The design doc at `docs/plans/2026-02-23-crater-terrain-design.md` has the full rationale for every decision below.

---

### Task 1: Rename Earth/Moon to Planet/Satellite

The current "Earth" and "Moon" are placeholders for fictional planets. Rename throughout the codebase to avoid confusion.

**Files:**
- Modify: `src/main.rs` (all Earth/Moon references — constants, variables, comments, marker component)
- Modify: `CLAUDE.md` (documentation references)

**Step 1: Rename in `src/main.rs`**

Constants:
```rust
// Before:
const EARTH_RADIUS: f32 = 1000.0;
const EARTH_ORBIT_RADIUS: f64 = 15000.0;
const EARTH_PERIOD: f64 = 30.0;
const MOON_RADIUS: f32 = 300.0;
const MOON_ORBIT_RADIUS: f64 = 4000.0;
const MOON_PERIOD: f64 = 10.0;

// After:
const PLANET_RADIUS: f32 = 1000.0;
const PLANET_ORBIT_RADIUS: f64 = 15000.0;
const PLANET_PERIOD: f64 = 30.0;
const SATELLITE_RADIUS: f32 = 300.0;
const SATELLITE_ORBIT_RADIUS: f64 = 4000.0;
const SATELLITE_PERIOD: f64 = 10.0;
```

Marker component:
```rust
// Before:
#[derive(Component)]
struct Moon;

// After:
#[derive(Component)]
pub struct Satellite;
```

Variables in `setup_scene`:
- `earth_sdf` → `planet_sdf_config`
- `earth_material_handle` → `planet_material_handle`
- `moon_sdf` → `satellite_sdf_config`
- `moon_material_handle` → `satellite_material_handle`
- `earth_id` → `planet_id`
- `moon_id` → `satellite_id`

In `camera_tracking_hotkeys`:
- `Has<Moon>` → `Has<Satellite>`
- `is_moon` → `is_satellite`

Update all comments: "Earth" → "planet", "Moon" → "satellite".

**Step 2: Update `CLAUDE.md`**

Replace Earth/Moon references in the entity hierarchy diagram, world scale section, and key descriptions with generic "planet" / "satellite" terminology. Update the `Moon` marker component reference to `Satellite`.

**Step 3: Verify**

Run: `cargo build`
Expected: Compiles without errors.

Run: `cargo run` (briefly)
Expected: Identical visual behavior — pure rename, no logic changes.

**Step 4: Commit**

```bash
git add src/main.rs CLAUDE.md
git commit -m "Rename Earth/Moon to planet/satellite

These are fictional placeholder bodies, not real celestial objects.
Generic names reduce confusion as we add diverse planet types."
```

---

### Task 2: Add `hash33` to noise.wgsl

A pseudorandom hash function mapping `vec3 → vec3` in [0,1], needed for Voronoi cell crater placement.

**Files:**
- Modify: `assets/shaders/noise.wgsl` (add function after `simplex3d`, before `fbm`)

**Step 1: Add `hash33` function**

Insert after line 89 (end of `simplex3d`), before the `fbm` function:

```wgsl
/// Hash a vec3 cell coordinate to 3 pseudorandom values in [0, 1].
/// Used for Voronoi cell jittering (crater placement, etc.).
fn hash33(p: vec3<f32>) -> vec3<f32> {
    var q = vec3<f32>(
        dot(p, vec3<f32>(127.1, 311.7, 74.7)),
        dot(p, vec3<f32>(269.5, 183.3, 246.1)),
        dot(p, vec3<f32>(113.5, 271.9, 124.6)),
    );
    return fract(sin(q) * 43758.5453123);
}
```

**Step 2: Verify**

Run: `cargo build`
Expected: Compiles. The function exists but has no callers yet.

**Step 3: Commit**

```bash
git add assets/shaders/noise.wgsl
git commit -m "Add hash33 pseudorandom function to noise library

Maps vec3 cell coordinates to 3 random values in [0,1].
Will be used for Voronoi cell crater placement."
```

---

### Task 3: Expand Uniforms with Crater Parameters

Add crater fields to both the Rust uniform struct and the WGSL struct. Both must match exactly.

**Files:**
- Modify: `src/planet_material.rs` — `PlanetSdfUniforms` and `SdfConfig`
- Modify: `src/planet.rs` — `update_planet_materials` system
- Modify: `assets/shaders/planet_sdf.wgsl` — WGSL uniform struct

**Step 1: Expand `PlanetSdfUniforms` in `src/planet_material.rs`**

```rust
#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct PlanetSdfUniforms {
    pub planet_center: Vec3,
    pub planet_radius: f32,
    pub camera_position: Vec3,
    pub max_elevation: f32,
    pub sun_direction: Vec3,
    pub noise_frequency: f32,
    pub noise_amplitude: f32,
    pub noise_lacunarity: f32,
    pub noise_persistence: f32,
    pub noise_octaves: u32,
    pub debug_mode: u32,
    // ── Crater system ──
    pub crater_enabled: u32,
    pub crater_frequency_0: f32,
    pub crater_depth_0: f32,
    pub crater_rim_height_0: f32,
    pub crater_peak_height_0: f32,
    pub crater_density_0: f32,
    pub crater_frequency_1: f32,
    pub crater_depth_1: f32,
    pub crater_rim_height_1: f32,
    pub crater_peak_height_1: f32,
    pub crater_density_1: f32,
    pub crater_frequency_2: f32,
    pub crater_depth_2: f32,
    pub crater_rim_height_2: f32,
    pub crater_peak_height_2: f32,
    pub crater_density_2: f32,
}
```

**Step 2: Expand `SdfConfig` in `src/planet_material.rs`**

```rust
#[derive(Clone, Debug)]
pub struct SdfConfig {
    pub radius: f32,
    pub max_elevation: f32,
    pub noise_frequency: f32,
    pub noise_amplitude: f32,
    pub noise_lacunarity: f32,
    pub noise_persistence: f32,
    pub noise_octaves: u32,
    // ── Crater system ──
    pub crater_enabled: bool,
    // Tier 0: large basins
    pub crater_frequency_0: f32,
    pub crater_depth_0: f32,
    pub crater_rim_height_0: f32,
    pub crater_peak_height_0: f32,
    pub crater_density_0: f32,
    // Tier 1: medium craters
    pub crater_frequency_1: f32,
    pub crater_depth_1: f32,
    pub crater_rim_height_1: f32,
    pub crater_peak_height_1: f32,
    pub crater_density_1: f32,
    // Tier 2: small pocks
    pub crater_frequency_2: f32,
    pub crater_depth_2: f32,
    pub crater_rim_height_2: f32,
    pub crater_peak_height_2: f32,
    pub crater_density_2: f32,
}

impl Default for SdfConfig {
    fn default() -> Self {
        Self {
            radius: 1000.0,
            max_elevation: 50.0,
            noise_frequency: 4.0,
            noise_amplitude: 50.0,
            noise_lacunarity: 2.0,
            noise_persistence: 0.5,
            noise_octaves: 14,
            crater_enabled: false,
            crater_frequency_0: 0.0,
            crater_depth_0: 0.0,
            crater_rim_height_0: 0.0,
            crater_peak_height_0: 0.0,
            crater_density_0: 0.0,
            crater_frequency_1: 0.0,
            crater_depth_1: 0.0,
            crater_rim_height_1: 0.0,
            crater_peak_height_1: 0.0,
            crater_density_1: 0.0,
            crater_frequency_2: 0.0,
            crater_depth_2: 0.0,
            crater_rim_height_2: 0.0,
            crater_peak_height_2: 0.0,
            crater_density_2: 0.0,
        }
    }
}
```

**Step 3: Update `update_planet_materials` in `src/planet.rs`**

Add crater fields to the uniform assignment block (inside the `for` loop, in the `mat.uniforms = PlanetSdfUniforms { ... }` struct literal):

```rust
mat.uniforms = PlanetSdfUniforms {
    planet_center: center,
    planet_radius: planet.sdf.radius,
    camera_position: cam_pos,
    max_elevation: planet.sdf.max_elevation,
    sun_direction: planet_sun_dir,
    noise_frequency: planet.sdf.noise_frequency,
    noise_amplitude: planet.sdf.noise_amplitude,
    noise_lacunarity: planet.sdf.noise_lacunarity,
    noise_persistence: planet.sdf.noise_persistence,
    noise_octaves: planet.sdf.noise_octaves,
    debug_mode: debug_mode.0,
    crater_enabled: u32::from(planet.sdf.crater_enabled),
    crater_frequency_0: planet.sdf.crater_frequency_0,
    crater_depth_0: planet.sdf.crater_depth_0,
    crater_rim_height_0: planet.sdf.crater_rim_height_0,
    crater_peak_height_0: planet.sdf.crater_peak_height_0,
    crater_density_0: planet.sdf.crater_density_0,
    crater_frequency_1: planet.sdf.crater_frequency_1,
    crater_depth_1: planet.sdf.crater_depth_1,
    crater_rim_height_1: planet.sdf.crater_rim_height_1,
    crater_peak_height_1: planet.sdf.crater_peak_height_1,
    crater_density_1: planet.sdf.crater_density_1,
    crater_frequency_2: planet.sdf.crater_frequency_2,
    crater_depth_2: planet.sdf.crater_depth_2,
    crater_rim_height_2: planet.sdf.crater_rim_height_2,
    crater_peak_height_2: planet.sdf.crater_peak_height_2,
    crater_density_2: planet.sdf.crater_density_2,
};
```

**Step 4: Update WGSL struct in `assets/shaders/planet_sdf.wgsl`**

The WGSL struct must match field-for-field:

```wgsl
struct PlanetSdfUniforms {
    planet_center: vec3<f32>,
    planet_radius: f32,
    camera_position: vec3<f32>,
    max_elevation: f32,
    sun_direction: vec3<f32>,
    noise_frequency: f32,
    noise_amplitude: f32,
    noise_lacunarity: f32,
    noise_persistence: f32,
    noise_octaves: u32,
    debug_mode: u32,
    // Crater system
    crater_enabled: u32,
    crater_frequency_0: f32,
    crater_depth_0: f32,
    crater_rim_height_0: f32,
    crater_peak_height_0: f32,
    crater_density_0: f32,
    crater_frequency_1: f32,
    crater_depth_1: f32,
    crater_rim_height_1: f32,
    crater_peak_height_1: f32,
    crater_density_1: f32,
    crater_frequency_2: f32,
    crater_depth_2: f32,
    crater_rim_height_2: f32,
    crater_peak_height_2: f32,
    crater_density_2: f32,
}
```

**Step 5: Verify**

Run: `cargo build`
Expected: Compiles without errors.

Run: `cargo run` (briefly)
Expected: Identical visuals — all new crater fields default to 0/disabled.

**Step 6: Commit**

```bash
git add src/planet_material.rs src/planet.rs assets/shaders/planet_sdf.wgsl
git commit -m "Add crater uniform fields to SDF pipeline

Three tiers of crater params (frequency, depth, rim, peak, density)
plus master toggle. Defaults to disabled — no visual change yet."
```

---

### Task 4: Add Crater Functions to Shader

Implement `crater_profile()` and `crater_field()` in `planet_sdf.wgsl`. These are self-contained functions not yet called from `planet_sdf()`.

**Files:**
- Modify: `assets/shaders/planet_sdf.wgsl` — add functions after `planet_sdf`, before `ray_sphere`

**Step 1: Add the import**

Update the import line at the top of `planet_sdf.wgsl`:

```wgsl
// Before:
#import noise::{simplex3d, fbm}

// After:
#import noise::{simplex3d, fbm, hash33}
```

**Step 2: Add `crater_profile` function**

Insert after the `planet_sdf` function (after line 81), before `ray_sphere`:

```wgsl
// ═══════════════════════════════════════════════════════════════════════════
// Crater system — Voronoi cell placement with multi-scale tiers
// ═══════════════════════════════════════════════════════════════════════════

/// Crater cross-section profile as a function of normalized radial distance.
/// r=0 is crater center, r=1 is rim edge.
/// Returns displacement: negative (bowl), positive (rim/peak).
fn crater_profile(r: f32, depth: f32, rim_height: f32, peak_height: f32) -> f32 {
    // Bowl: parabolic depression, zero beyond r=1
    let bowl = -depth * max(1.0 - r * r, 0.0);

    // Rim: Gaussian bump centered at r=1
    let rim = rim_height * exp(-8.0 * (r - 1.0) * (r - 1.0));

    // Central peak: narrow Gaussian at center
    let peak = peak_height * exp(-50.0 * r * r);

    // Fade everything smoothly to zero beyond the rim
    let fade = 1.0 - smoothstep(1.2, 2.0, r);

    return (bowl + rim + peak) * fade;
}

/// Evaluate one tier of craters using Voronoi cell placement.
/// `dir` is the normalized surface direction (unit sphere).
/// Returns elevation displacement in world units (km).
fn crater_field(
    dir: vec3<f32>,
    cell_freq: f32,
    depth: f32,
    rim_height: f32,
    peak_height: f32,
    density: f32,
) -> f32 {
    let p = dir * cell_freq;
    let cell = floor(p);

    var displacement = 0.0;

    for (var dx: i32 = -1; dx <= 1; dx += 1) {
        for (var dy: i32 = -1; dy <= 1; dy += 1) {
            for (var dz: i32 = -1; dz <= 1; dz += 1) {
                let neighbor = cell + vec3<f32>(f32(dx), f32(dy), f32(dz));
                let h = hash33(neighbor);

                // Does this cell contain a crater?
                if (h.z > density) { continue; }

                // Jittered crater center within the cell
                let crater_pos = neighbor + h * 0.8 + 0.1;

                // Distance from sample to crater center
                let d = length(p - crater_pos);

                // Crater radius varies per cell (0.3–0.5 of cell size)
                let crater_radius = 0.3 + fract(h.x * 13.7 + h.y * 7.3) * 0.2;

                // Normalized radial distance
                let r = d / crater_radius;

                if (r > 2.0) { continue; }

                displacement += crater_profile(r, depth, rim_height, peak_height);
            }
        }
    }

    return displacement;
}
```

**Step 3: Verify**

Run: `cargo build`
Expected: Compiles. Functions exist but are not called yet.

**Step 4: Commit**

```bash
git add assets/shaders/planet_sdf.wgsl
git commit -m "Add crater_profile and crater_field shader functions

Voronoi cell placement with parabolic bowl, Gaussian rim, and
optional central peak. 27-cell neighbor search per sample point.
Not yet wired into planet_sdf() — next step."
```

---

### Task 5: Compose Craters into SDF and Configure Main Planet

Wire up the crater system: call `crater_field()` from `planet_sdf()`, and give the main planet a moon-like crater config.

**Files:**
- Modify: `assets/shaders/planet_sdf.wgsl` — update `planet_sdf()` to call crater fields
- Modify: `src/main.rs` — set crater-heavy `SdfConfig` on the main planet

**Step 1: Update `planet_sdf()` in the shader**

Replace the current `planet_sdf` function body with the layered version:

```wgsl
fn planet_sdf(p: vec3<f32>, min_feature_size: f32) -> f32 {
    let dir = normalize(p - uniforms.planet_center);

    // Base terrain roughness
    var elevation = fbm(
        dir * uniforms.noise_frequency,
        uniforms.noise_octaves,
        uniforms.noise_lacunarity,
        uniforms.noise_persistence,
        min_feature_size,
    ) * uniforms.noise_amplitude;

    // Crater displacement (3 tiers)
    if (uniforms.crater_enabled != 0u) {
        elevation += crater_field(dir,
            uniforms.crater_frequency_0, uniforms.crater_depth_0,
            uniforms.crater_rim_height_0, uniforms.crater_peak_height_0,
            uniforms.crater_density_0);
        elevation += crater_field(dir,
            uniforms.crater_frequency_1, uniforms.crater_depth_1,
            uniforms.crater_rim_height_1, uniforms.crater_peak_height_1,
            uniforms.crater_density_1);
        elevation += crater_field(dir,
            uniforms.crater_frequency_2, uniforms.crater_depth_2,
            uniforms.crater_rim_height_2, uniforms.crater_peak_height_2,
            uniforms.crater_density_2);
    }

    return length(p - uniforms.planet_center) - (uniforms.planet_radius + elevation);
}
```

**Important:** The crater functions must be defined BEFORE `planet_sdf` in the file (WGSL requires functions to be declared before use). Move the crater section (from Task 4) above the `planet_sdf` function.

**Step 2: Configure the main planet with moon-like craters in `src/main.rs`**

Replace the main planet's `SdfConfig` with crater-heavy parameters. Reduce FBM amplitude so craters dominate the terrain character:

```rust
let planet_sdf_config = SdfConfig {
    radius: PLANET_RADIUS,
    max_elevation: 50.0,
    noise_frequency: 4.0,
    noise_amplitude: 10.0,  // Reduced from 50 — craters are the main feature
    noise_lacunarity: 2.0,
    noise_persistence: 0.5,
    noise_octaves: 14,
    crater_enabled: true,
    // Tier 0: large basins
    crater_frequency_0: 6.0,
    crater_depth_0: 15.0,
    crater_rim_height_0: 5.0,
    crater_peak_height_0: 3.0,
    crater_density_0: 0.4,
    // Tier 1: medium craters
    crater_frequency_1: 20.0,
    crater_depth_1: 5.0,
    crater_rim_height_1: 2.0,
    crater_peak_height_1: 0.5,
    crater_density_1: 0.5,
    // Tier 2: small pocks
    crater_frequency_2: 60.0,
    crater_depth_2: 1.5,
    crater_rim_height_2: 0.5,
    crater_peak_height_2: 0.0,
    crater_density_2: 0.6,
};
```

The satellite keeps its existing config (no craters):
```rust
let satellite_sdf_config = SdfConfig {
    radius: SATELLITE_RADIUS,
    max_elevation: 20.0,
    noise_frequency: 8.0,
    noise_amplitude: 20.0,
    noise_lacunarity: 2.0,
    noise_persistence: 0.5,
    noise_octaves: 14,
    ..Default::default()  // crater_enabled: false, all crater params: 0
};
```

**Step 3: Verify**

Run: `cargo run`
Expected:
- Main planet shows multi-scale craters — large basins with central peaks, medium craters, small pockmarks
- FBM roughness adds texture to crater floors and rims
- Satellite looks unchanged (smooth FBM terrain)
- Both render with correct depth and lighting

**Step 4: Commit**

```bash
git add assets/shaders/planet_sdf.wgsl src/main.rs
git commit -m "Wire up crater system — main planet gets moon-like terrain

Three tiers of Voronoi cell craters composed with base FBM roughness.
Satellite unchanged (craters disabled). Initial parameter values for
visual tuning."
```

---

### Task 6: Tune and Update Documentation

Visual tuning of crater parameters, then update project docs to reflect Phase 3 progress.

**Files:**
- Possibly modify: `src/main.rs` (tune crater params based on visual feedback)
- Modify: `docs/03-implementation-phases.md` (mark Phase 2 complete, update Phase 3 status)
- Modify: `CLAUDE.md` (add crater system to architecture description)

**Step 1: Visual tuning**

Run `cargo run` and fly around the main planet. Adjust these parameters in the `SdfConfig` if needed:

- If craters are too deep/shallow: adjust `crater_depth_N`
- If rims are too prominent/invisible: adjust `crater_rim_height_N`
- If craters are too sparse/dense: adjust `crater_density_N`
- If crater sizes look wrong: adjust `crater_frequency_N`
- If the surface is too bumpy under craters: reduce `noise_amplitude`
- If `max_elevation` is too small (craters clipped): increase it

This is iterative — make changes, rebuild, re-check.

**Step 2: Update `docs/03-implementation-phases.md`**

Mark Phase 2 as complete (add ✅). Update Phase 3 task list to reflect actual work done (crater system replaces the generic "terrain quality" framing).

**Step 3: Update `CLAUDE.md`**

Add crater system description to the "SDF Terrain Rendering" section:
- Crater placement via Voronoi cells on direction vector
- Three scale tiers with per-tier params in uniforms
- `crater_enabled` toggle for per-planet opt-in

**Step 4: Commit**

```bash
git add src/main.rs docs/03-implementation-phases.md CLAUDE.md
git commit -m "Tune crater params and update docs for Phase 3 progress"
```
