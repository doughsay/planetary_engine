# Phase 2: SDF Rendering Foundation — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the mesh-based terrain pipeline with GPU raymarching. Each planet renders as an icosphere whose fragment shader sphere-traces a noise-perturbed SDF.

**Architecture:** An icosphere mesh per planet is positioned by the vertex shader using uniforms (planet center + bounding radius), identically to how `galaxy.wgsl` positions its sphere. The fragment shader computes a ray from camera → fragment, intersects the planet's bounding sphere, then sphere-traces through `sdf(p) = length(p) - radius - fbm(normalize(p))` to find the terrain surface. Normals come from the SDF gradient; lighting uses the sun direction uniform.

**Tech Stack:** Rust (Bevy 0.18 `Material` trait, `AsBindGroup`, `ShaderType`), WGSL (simplex noise, FBM, sphere tracing)

**Existing patterns to follow:**
- `src/galaxy.rs` — Material impl with `specialize()`, `AsBindGroup`, vertex-only layout
- `assets/shaders/galaxy.wgsl` — vertex shader positioning from `view.world_position`
- `src/starfield.rs` — Material impl with custom pipeline specialization
- `CLAUDE.md` — Bevy 0.18 API notes (especially bind group `#{MATERIAL_BIND_GROUP}`, ShaderType alignment, no `mesh_functions` import)

---

### Task 1: Create WGSL simplex noise library

**Files:**
- Create: `assets/shaders/noise.wgsl`

This is a self-contained 3D simplex noise implementation in WGSL. Uses permutation polynomials (no texture lookups). Standard port of the Ashima/webgl-noise simplex3d.

**Step 1: Write the noise shader**

Create `assets/shaders/noise.wgsl` with:

```wgsl
// 3D Simplex noise — WGSL port of Ashima webgl-noise (simplex3d)
// https://github.com/ashima/webgl-noise
// MIT License

fn mod289_3(x: vec3<f32>) -> vec3<f32> {
    return x - floor(x * (1.0 / 289.0)) * 289.0;
}

fn mod289_4(x: vec4<f32>) -> vec4<f32> {
    return x - floor(x * (1.0 / 289.0)) * 289.0;
}

fn permute(x: vec4<f32>) -> vec4<f32> {
    return mod289_4(((x * 34.0) + 10.0) * x);
}

fn taylor_inv_sqrt(r: vec4<f32>) -> vec4<f32> {
    return 1.79284291400159 - 0.85373472095314 * r;
}

fn simplex3d(v: vec3<f32>) -> f32 {
    let C = vec2(1.0 / 6.0, 1.0 / 3.0);
    let D = vec4(0.0, 0.5, 1.0, 2.0);

    // First corner
    var i = floor(v + dot(v, C.yyy));
    let x0 = v - i + dot(i, C.xxx);

    // Other corners
    let g = step(x0.yzx, x0.xyz);
    let l = 1.0 - g;
    let i1 = min(g.xyz, l.zxy);
    let i2 = max(g.xyz, l.zxy);

    let x1 = x0 - i1 + C.xxx;
    let x2 = x0 - i2 + C.yyy;
    let x3 = x0 - D.yyy;

    // Permutations
    i = mod289_3(i);
    let p = permute(permute(permute(
        i.z + vec4(0.0, i1.z, i2.z, 1.0))
      + i.y + vec4(0.0, i1.y, i2.y, 1.0))
      + i.x + vec4(0.0, i1.x, i2.x, 1.0));

    // Gradients: 7x7 points over a square, mapped onto an octahedron.
    let n_ = 0.142857142857; // 1.0 / 7.0
    let ns = n_ * D.wyz - D.xzx;

    let j = p - 49.0 * floor(p * ns.z * ns.z);

    let x_ = floor(j * ns.z);
    let y_ = floor(j - 7.0 * x_);

    let x = x_ * ns.x + ns.yyyy;
    let y = y_ * ns.x + ns.yyyy;
    let h = 1.0 - abs(x) - abs(y);

    let b0 = vec4(x.xy, y.xy);
    let b1 = vec4(x.zw, y.zw);

    let s0 = floor(b0) * 2.0 + 1.0;
    let s1 = floor(b1) * 2.0 + 1.0;
    let sh = -step(h, vec4(0.0));

    let a0 = b0.xzyw + s0.xzyw * sh.xxyy;
    let a1 = b1.xzyw + s1.xzyw * sh.zzww;

    var p0 = vec3(a0.xy, h.x);
    var p1 = vec3(a0.zw, h.y);
    var p2 = vec3(a1.xy, h.z);
    var p3 = vec3(a1.zw, h.w);

    // Normalise gradients
    let norm = taylor_inv_sqrt(vec4(dot(p0, p0), dot(p1, p1), dot(p2, p2), dot(p3, p3)));
    p0 *= norm.x;
    p1 *= norm.y;
    p2 *= norm.z;
    p3 *= norm.w;

    // Mix contributions from the four corners
    var m = max(0.5 - vec4(dot(x0, x0), dot(x1, x1), dot(x2, x2), dot(x3, x3)), vec4(0.0));
    m = m * m;
    return 105.0 * dot(m * m, vec4(dot(p0, x0), dot(p1, x1), dot(p2, x2), dot(p3, x3)));
}

// FBM with distance-based octave culling.
// Skips octaves whose feature size is below the pixel threshold.
fn fbm(p: vec3<f32>, max_octaves: u32, lacunarity: f32, persistence: f32, cam_dist: f32, pixel_size: f32) -> f32 {
    var result = 0.0;
    var freq = 1.0;
    var amp = 1.0;
    var amp_sum = 0.0;

    for (var i = 0u; i < max_octaves; i++) {
        // Skip sub-pixel octaves
        if (cam_dist > 0.0 && amp < pixel_size * freq * 0.5) {
            break;
        }
        result += simplex3d(p * freq) * amp;
        amp_sum += amp;
        freq *= lacunarity;
        amp *= persistence;
    }

    // Normalize by the full amplitude sum for max_octaves so that adding
    // octaves adds detail without rescaling earlier frequencies.
    var full_sum = 0.0;
    var a = 1.0;
    for (var i = 0u; i < max_octaves; i++) {
        full_sum += a;
        a *= persistence;
    }

    if (full_sum > 0.0) {
        return result / full_sum;
    }
    return 0.0;
}
```

**Step 2: Verify it parses**

This is a standalone WGSL file with no Bevy imports — it will be imported by the planet shader. Verification comes in Task 3 when we compile the full shader. No separate test needed.

**Step 3: Commit**

```bash
git add assets/shaders/noise.wgsl
git commit -m "Add WGSL simplex noise library for SDF terrain"
```

---

### Task 2: Create PlanetMaterial (Rust side)

**Files:**
- Create: `src/planet_material.rs`
- Modify: `src/main.rs` (add `mod planet_material`)

**Step 1: Create the material module**

Create `src/planet_material.rs`:

```rust
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

const SHADER_PATH: &str = "shaders/planet_sdf.wgsl";

/// GPU-side uniforms for SDF planet rendering.
/// Layout must match the WGSL struct exactly (16-byte aligned vec3 rows).
#[derive(ShaderType, Clone, Copy, Debug, Default)]
pub struct PlanetSdfUniforms {
    // Row 1: planet geometry
    pub planet_center: Vec3,
    pub planet_radius: f32,
    // Row 2: camera
    pub camera_position: Vec3,
    pub max_elevation: f32,
    // Row 3: lighting
    pub sun_direction: Vec3,
    pub _pad0: f32,
    // Row 4: noise params
    pub noise_frequency: f32,
    pub noise_amplitude: f32,
    pub noise_lacunarity: f32,
    pub noise_persistence: f32,
    // Row 5: noise params continued
    pub noise_octaves: u32,
    pub _pad1: u32,
    pub _pad2: u32,
    pub _pad3: u32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct PlanetMaterial {
    #[uniform(0)]
    pub uniforms: PlanetSdfUniforms,
}

impl Material for PlanetMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Opaque
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Position-only vertex layout — we compute world pos from uniforms.
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];

        // Camera can be inside the bounding icosphere when on the surface.
        descriptor.primitive.cull_mode = None;

        // Terrain writes depth at the actual SDF hit point.
        if let Some(depth_stencil) = &mut descriptor.depth_stencil {
            depth_stencil.depth_write_enabled = true;
        }

        Ok(())
    }
}

/// Rust-side configuration for a planet's SDF terrain.
/// These values are copied into PlanetSdfUniforms each frame.
#[derive(Clone, Debug)]
pub struct SdfConfig {
    pub radius: f32,
    pub max_elevation: f32,
    pub noise_frequency: f32,
    pub noise_amplitude: f32,
    pub noise_lacunarity: f32,
    pub noise_persistence: f32,
    pub noise_octaves: u32,
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
        }
    }
}
```

**Step 2: Add the module declaration to `main.rs`**

In `src/main.rs`, add after the existing `mod` declarations (line 10):

```rust
mod planet_material;
```

**Step 3: Verify it compiles**

Run: `cargo check`
Expected: Compiles (the shader doesn't exist yet, that's fine — Bevy loads shaders at runtime).

**Step 4: Commit**

```bash
git add src/planet_material.rs src/main.rs
git commit -m "Add PlanetMaterial with SDF uniforms"
```

---

### Task 3: Create planet SDF shader

**Files:**
- Create: `assets/shaders/planet_sdf.wgsl`

**Step 1: Write the shader**

Create `assets/shaders/planet_sdf.wgsl`:

```wgsl
#import bevy_pbr::mesh_view_bindings::view
#import "shaders/noise.wgsl"::simplex3d
#import "shaders/noise.wgsl"::fbm

// Material uniforms — must match PlanetSdfUniforms in Rust exactly.
struct PlanetSdfUniforms {
    planet_center: vec3<f32>,
    planet_radius: f32,
    camera_position: vec3<f32>,
    max_elevation: f32,
    sun_direction: vec3<f32>,
    _pad0: f32,
    noise_frequency: f32,
    noise_amplitude: f32,
    noise_lacunarity: f32,
    noise_persistence: f32,
    noise_octaves: u32,
    _pad1: u32,
    _pad2: u32,
    _pad3: u32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> u: PlanetSdfUniforms;

// Raymarching constants
const MAX_STEPS: u32 = 128u;
const SURFACE_EPSILON: f32 = 0.01;   // ~10 meters in world units (km)
const NORMAL_EPSILON: f32 = 0.05;    // For gradient computation

struct Vertex {
    @location(0) position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
};

// ── Vertex shader ───────────────────────────────────────────────────────────
// Position icosphere at planet_center with bounding radius (radius + max_elevation).
// Same pattern as galaxy.wgsl — no mesh_functions import.

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    // Scale unit sphere to bounding radius and translate to planet center.
    let bounding_radius = u.planet_radius + u.max_elevation;
    let world_pos = v.position * bounding_radius + u.planet_center;
    out.clip_position = view.clip_from_world * vec4(world_pos, 1.0);
    out.world_position = world_pos;
    return out;
}

// ── SDF evaluation ──────────────────────────────────────────────────────────

fn planet_sdf(p: vec3<f32>, cam_dist: f32) -> f32 {
    let centered = p - u.planet_center;
    let r = length(centered);
    let dir = centered / max(r, 0.001);

    // Base sphere
    var d = r - u.planet_radius;

    // Terrain displacement from FBM noise
    let pixel_size = cam_dist * 0.00005; // Approximate pixel angular size
    let terrain = fbm(
        dir * u.noise_frequency,
        u.noise_octaves,
        u.noise_lacunarity,
        u.noise_persistence,
        cam_dist,
        pixel_size
    );
    d -= terrain * u.noise_amplitude;

    return d;
}

// ── Normal via SDF gradient ─────────────────────────────────────────────────

fn sdf_normal(p: vec3<f32>, cam_dist: f32) -> vec3<f32> {
    // Scale epsilon with distance for numerical stability
    let eps = max(cam_dist * 0.0001, NORMAL_EPSILON);
    let e = vec2(eps, 0.0);
    return normalize(vec3(
        planet_sdf(p + e.xyy, cam_dist) - planet_sdf(p - e.xyy, cam_dist),
        planet_sdf(p + e.yxy, cam_dist) - planet_sdf(p - e.yxy, cam_dist),
        planet_sdf(p + e.yyx, cam_dist) - planet_sdf(p - e.yyx, cam_dist),
    ));
}

// ── Ray-sphere intersection ─────────────────────────────────────────────────

fn ray_sphere(ro: vec3<f32>, rd: vec3<f32>, center: vec3<f32>, radius: f32) -> vec2<f32> {
    let oc = ro - center;
    let b = dot(oc, rd);
    let c = dot(oc, oc) - radius * radius;
    let discriminant = b * b - c;
    if (discriminant < 0.0) {
        return vec2(-1.0, -1.0);
    }
    let d = sqrt(discriminant);
    return vec2(-b - d, -b + d);
}

// ── Fragment shader ─────────────────────────────────────────────────────────

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let ray_origin = u.camera_position;
    let ray_dir = normalize(in.world_position - u.camera_position);

    // Intersect with bounding sphere (planet_radius + max_elevation)
    let bounding_radius = u.planet_radius + u.max_elevation;
    let bounds = ray_sphere(ray_origin, ray_dir, u.planet_center, bounding_radius);

    // Also intersect with the planet core (radius - max_elevation) for early termination
    let inner_radius = max(u.planet_radius - u.max_elevation, 0.0);
    let inner = ray_sphere(ray_origin, ray_dir, u.planet_center, inner_radius);

    // Determine march range
    var t_start = max(bounds.x, 0.0);
    var t_end = bounds.y;

    // If ray hits inner core, don't march past it
    if (inner.x > 0.0) {
        t_end = min(t_end, inner.x);
    }

    if (t_start >= t_end) {
        discard;
    }

    // ── Sphere tracing ──────────────────────────────────────────────────
    var t = t_start;
    var hit = false;
    var hit_pos = vec3(0.0);

    for (var i = 0u; i < MAX_STEPS; i++) {
        let p = ray_origin + ray_dir * t;
        let cam_dist = length(p - u.camera_position);
        let d = planet_sdf(p, cam_dist);

        if (d < SURFACE_EPSILON) {
            hit = true;
            hit_pos = p;
            break;
        }

        // SDF-guided step. Clamp minimum step to avoid getting stuck.
        t += max(d, SURFACE_EPSILON * 0.5);

        if (t > t_end) {
            break;
        }
    }

    if (!hit) {
        discard;
    }

    // ── Shading ─────────────────────────────────────────────────────────
    let cam_dist = length(hit_pos - u.camera_position);
    let normal = sdf_normal(hit_pos, cam_dist);

    // Basic Lambertian lighting
    let n_dot_l = max(dot(normal, u.sun_direction), 0.0);

    // Ambient + diffuse
    let ambient = 0.03;
    let diffuse = n_dot_l * 0.9;

    // Simple gray terrain color for now (Phase 4 adds biomes)
    let base_color = vec3(0.5, 0.5, 0.5);
    let lit_color = base_color * (ambient + diffuse);

    return vec4(lit_color, 1.0);
}
```

**Step 2: Verify shader compiles by running the app**

This will be tested in Task 5 after wiring everything together. The shader file must exist before then.

**Step 3: Commit**

```bash
git add assets/shaders/planet_sdf.wgsl
git commit -m "Add SDF planet terrain shader with sphere tracing"
```

---

### Task 4: Update Planet component for SDF

**Files:**
- Modify: `src/planet.rs`

Replace the entire contents of `src/planet.rs`. The `Planet` component now holds an `SdfConfig` and a material handle instead of the old `TerrainConfig`.

**Step 1: Rewrite planet.rs**

Replace the contents of `src/planet.rs` with:

```rust
use bevy::prelude::*;
use crate::planet_material::{PlanetMaterial, SdfConfig};

/// Marker + config component for planet entities.
#[derive(Component, Debug)]
pub struct Planet {
    pub sdf: SdfConfig,
    pub material_handle: Handle<PlanetMaterial>,
}

pub struct PlanetPlugin;

impl Plugin for PlanetPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<PlanetMaterial>::default())
            .add_systems(Update, update_planet_materials);
    }
}

/// Each frame, update the planet material uniforms with current camera position,
/// planet center (from GlobalTransform), and sun direction.
fn update_planet_materials(
    camera_q: Query<&GlobalTransform, With<Camera3d>>,
    planet_q: Query<(&Planet, &GlobalTransform)>,
    sun_q: Query<&GlobalTransform, With<crate::Sun>>,
    mut materials: ResMut<Assets<PlanetMaterial>>,
) {
    let Ok(cam_gt) = camera_q.single() else { return };
    let Ok(sun_gt) = sun_q.single() else { return };
    let cam_pos: Vec3 = cam_gt.translation();
    let sun_pos: Vec3 = sun_gt.translation();

    for (planet, planet_gt) in &planet_q {
        let center: Vec3 = planet_gt.translation();
        let sun_dir = (sun_pos - center).normalize_or_zero();

        if let Some(mat) = materials.get_mut(&planet.material_handle) {
            mat.uniforms.planet_center = center;
            mat.uniforms.planet_radius = planet.sdf.radius;
            mat.uniforms.max_elevation = planet.sdf.max_elevation;
            mat.uniforms.camera_position = cam_pos;
            mat.uniforms.sun_direction = sun_dir;
            mat.uniforms.noise_frequency = planet.sdf.noise_frequency;
            mat.uniforms.noise_amplitude = planet.sdf.noise_amplitude;
            mat.uniforms.noise_lacunarity = planet.sdf.noise_lacunarity;
            mat.uniforms.noise_persistence = planet.sdf.noise_persistence;
            mat.uniforms.noise_octaves = planet.sdf.noise_octaves;
        }
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: May fail because `main.rs` still uses the old `TerrainConfig` and `PlanetQuadtree`. That's expected — Task 5 will fix the wiring.

**Step 3: Commit**

```bash
git add src/planet.rs
git commit -m "Rewrite Planet component for SDF config"
```

---

### Task 5: Wire up main.rs — remove old pipeline, spawn SDF planets

**Files:**
- Modify: `src/main.rs`

This is the big switchover. Remove all references to the old mesh pipeline and spawn icosphere terrain meshes with PlanetMaterial instead.

**Step 1: Rewrite main.rs**

Replace the full contents of `src/main.rs`. Key changes from the original:
- Remove `mod chunk_mesh`, `mod lod`, `mod mesh_task`, `mod quadtree`, `mod terrain`
- Remove `use lod::{LodPlugin, PlanetQuadtree}`, `use terrain::TerrainConfig`
- Remove `.add_plugins(LodPlugin)` from the app
- Add `mod planet_material`
- Spawn icosphere meshes with `PlanetMaterial` instead of `PlanetQuadtree` + `StandardMaterial`
- Add `use planet_material::{PlanetMaterial, SdfConfig}`

New contents of `src/main.rs`:

```rust
mod camera;
mod galaxy;
mod starfield;
mod planet;
mod planet_material;
mod orbit;

use big_space::prelude::*;
use bevy::camera::Exposure;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::wireframe::{WireframeConfig, WireframePlugin};
use bevy::prelude::*;
use bevy::render::view::Hdr;
use camera::{SpaceCamera, SpaceCameraPlugin, SpaceCameraState};
use planet::PlanetPlugin;
use planet_material::{PlanetMaterial, SdfConfig};
use orbit::{Orbit, OrbitPlugin, OrbitalTime};

#[derive(Component)]
pub struct Sun;

#[derive(Component)]
struct Moon;

/// MICRO SCALE constants for easy verification.
const SUN_RADIUS: f32 = 2000.0;
const EARTH_RADIUS: f32 = 1000.0;
const EARTH_ORBIT_RADIUS: f64 = 15000.0;
const EARTH_PERIOD: f64 = 30.0; // 30 second year

const MOON_RADIUS: f32 = 300.0;
const MOON_ORBIT_RADIUS: f64 = 4000.0;
const MOON_PERIOD: f64 = 10.0; // 10 second month

const SUN_POSITION: Vec3 = Vec3::ZERO;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        resolution: UVec2::new(1920, 1080).into(),
                        title: "Planetary Engine".into(),
                        ..default()
                    }),
                    ..default()
                })
                .build()
                .disable::<TransformPlugin>(),
        )
        .add_plugins(WireframePlugin::default())
        .add_plugins(BigSpaceDefaultPlugins)
        .add_plugins(SpaceCameraPlugin)
        .add_plugins(PlanetPlugin)
        .add_plugins(OrbitPlugin)
        .add_plugins(starfield::StarfieldPlugin)
        .add_plugins(galaxy::GalaxyPlugin)
        .add_systems(Startup, setup_scene)
        .add_systems(Update, (toggle_wireframe, camera_tracking_hotkeys))
        .run();
}

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut std_materials: ResMut<Assets<StandardMaterial>>,
    mut planet_materials: ResMut<Assets<PlanetMaterial>>,
    mut starfield_materials: ResMut<Assets<starfield::StarfieldMaterial>>,
    mut galaxy_materials: ResMut<Assets<galaxy::GalaxyMaterial>>,
    mut orbital_time: ResMut<OrbitalTime>,
) {
    orbital_time.speed = 1.0;
    commands.insert_resource(ClearColor(Color::BLACK));

    let root_id = commands.spawn(BigSpaceRootBundle::default()).id();

    // Unit icosphere shared by all planet terrain meshes.
    // The vertex shader scales and positions it via uniforms.
    let terrain_mesh = meshes.add(Sphere::new(1.0).mesh().ico(5).unwrap());

    // Earth SDF config
    let earth_sdf = SdfConfig {
        radius: EARTH_RADIUS,
        max_elevation: 50.0,
        noise_frequency: 4.0,
        noise_amplitude: 50.0,
        noise_lacunarity: 2.0,
        noise_persistence: 0.5,
        noise_octaves: 14,
    };
    let earth_material = planet_materials.add(PlanetMaterial {
        uniforms: Default::default(), // Updated each frame by update_planet_materials
    });

    // Moon SDF config
    let moon_sdf = SdfConfig {
        radius: MOON_RADIUS,
        max_elevation: 20.0,
        noise_frequency: 8.0,
        noise_amplitude: 20.0,
        noise_lacunarity: 2.0,
        noise_persistence: 0.5,
        noise_octaves: 14,
    };
    let moon_material = planet_materials.add(PlanetMaterial {
        uniforms: Default::default(),
    });

    {
        let mut grid_cmds = commands.grid(root_id, Grid::default());
        starfield::spawn_starfield(&mut grid_cmds, &mut meshes, &mut starfield_materials);
        galaxy::spawn_galaxy(&mut grid_cmds, &mut meshes, &mut galaxy_materials);

        grid_cmds.spawn_spatial((
            Sun,
            Mesh3d(meshes.add(Sphere::new(SUN_RADIUS).mesh().ico(5).unwrap())),
            MeshMaterial3d(std_materials.add(StandardMaterial {
                base_color: Color::WHITE,
                emissive: LinearRgba::WHITE * 100.0,
                ..default()
            })),
            Transform::from_translation(SUN_POSITION),
        ));

        grid_cmds.spawn_spatial((
            DirectionalLight {
                illuminance: 120_000.0,
                shadows_enabled: true,
                ..default()
            },
            Transform::from_xyz(5000.0, 5000.0, 5000.0)
                .looking_at(Vec3::ZERO, Vec3::Y),
        ));
    }

    let earth_id;

    {
        let mut grid_cmds = commands.grid(root_id, Grid::default());

        earth_id = grid_cmds.spawn_grid_default((
            Transform::default(),
            Visibility::default(),
            planet::Planet {
                sdf: earth_sdf,
                material_handle: earth_material.clone(),
            },
            Orbit {
                semi_major_axis: EARTH_ORBIT_RADIUS,
                eccentricity: 0.0,
                inclination: 0.0,
                longitude_of_ascending_node: 0.0,
                argument_of_periapsis: 0.0,
                period: EARTH_PERIOD,
                initial_mean_anomaly: 0.0,
                parent: None,
            },
        )).id();

        grid_cmds.spawn_grid_default((
            Moon,
            Transform::default(),
            Visibility::default(),
            planet::Planet {
                sdf: moon_sdf,
                material_handle: moon_material.clone(),
            },
            Orbit {
                semi_major_axis: MOON_ORBIT_RADIUS,
                eccentricity: 0.0,
                inclination: 0.0,
                longitude_of_ascending_node: 0.0,
                argument_of_periapsis: 0.0,
                period: MOON_PERIOD,
                initial_mean_anomaly: 0.0,
                parent: Some(earth_id),
            },
        ));
    }

    // Spawn terrain icosphere meshes as children of each planet.
    // Earth terrain mesh
    commands.entity(earth_id).with_children(|parent| {
        parent.spawn((
            CellCoord::default(),
            Mesh3d(terrain_mesh.clone()),
            MeshMaterial3d(earth_material),
            Transform::default(),
            NoFrustumCulling,
        ));
    });

    // Camera — child of Earth, looking outward
    commands.entity(earth_id).with_children(|parent| {
        parent.spawn((
            CellCoord::default(),
            Camera3d::default(),
            bevy::core_pipeline::prepass::DepthPrepass,
            Transform::from_xyz(0.0, 0.0, EARTH_RADIUS + 8000.0)
                .looking_at(Vec3::ZERO, Vec3::Y),
            Projection::Perspective(PerspectiveProjection {
                far: 1_000_000.0,
                near: 1.0,
                ..default()
            }),
            Hdr,
            Exposure::SUNLIGHT,
            Tonemapping::AcesFitted,
            SpaceCamera {
                speed: 50.0,
                boost_multiplier: 10.0,
                sensitivity: 0.15,
                roll_speed: 1.5,
                friction: 5.0,
                scroll_factor: 1.2,
            },
            SpaceCameraState::default(),
            FloatingOrigin,
        ));
    });
}

fn camera_tracking_hotkeys(
    input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut camera_q: Query<(&mut Transform, &GlobalTransform, &mut SpaceCameraState), With<Camera3d>>,
    targets_q: Query<(&GlobalTransform, Has<Sun>, Has<Moon>, Has<planet::Planet>), Without<Camera3d>>,
) {
    let mut target_pos: Option<Vec3> = None;

    if input.pressed(KeyCode::Digit1) {
        for (global, is_sun, _, _) in &targets_q {
            if is_sun { target_pos = Some(global.translation()); break; }
        }
    } else if input.pressed(KeyCode::Digit2) {
        for (global, _, is_moon, is_planet) in &targets_q {
            if is_planet && !is_moon { target_pos = Some(global.translation()); break; }
        }
    } else if input.pressed(KeyCode::Digit3) {
        for (global, _, is_moon, _) in &targets_q {
            if is_moon { target_pos = Some(global.translation()); break; }
        }
    }

    if let Some(target_pos) = target_pos {
        let Ok((mut cam_transform, cam_global, mut cam_state)) = camera_q.single_mut() else { return };
        cam_state.velocity = Vec3::ZERO;
        let cam_world_pos = cam_global.translation();
        let to_target = target_pos - cam_world_pos;
        if to_target.length_squared() < 1.0 { return; }
        let dir = to_target.normalize();
        let desired_rot = Transform::from_translation(cam_transform.translation)
            .looking_at(cam_transform.translation + dir, Vec3::Y)
            .rotation;
        let t = (8.0 * time.delta_secs()).min(1.0);
        cam_transform.rotation = cam_transform.rotation.slerp(desired_rot, t);
    }
}

fn toggle_wireframe(input: Res<ButtonInput<KeyCode>>, mut config: ResMut<WireframeConfig>) {
    if input.just_pressed(KeyCode::F1) {
        config.global = !config.global;
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo check`

Expected: Compiles. There may be warnings about unused files (`terrain.rs`, `quadtree.rs`, etc.) still existing on disk — that's fine, they're no longer referenced by `mod` declarations.

**Step 3: Run the app**

Run: `cargo run`

Expected: You should see planets rendered via SDF raymarching. They should appear as bumpy gray spheres lit from one side. The starfield and galaxy should still render behind them. Orbits should still work (Earth orbiting sun, Moon orbiting Earth). Camera controls should work (WASD + mouse).

**If it doesn't work:** Common issues:
- Shader import path: Bevy resolves `"shaders/noise.wgsl"` relative to the `assets/` directory. If imports fail, try `#import "shaders/noise.wgsl"` (with the path from assets root).
- Uniform alignment: If the shader complains about buffer layout, check that the WGSL struct padding matches the Rust `ShaderType` struct exactly.
- Bind group: Ensure `@group(#{MATERIAL_BIND_GROUP})` is used (Bevy preprocessor token), not a hardcoded group number.

**Step 4: Commit**

```bash
git add src/main.rs
git commit -m "Wire up SDF planet rendering, remove old mesh pipeline references"
```

---

### Task 6: Spawn Moon terrain mesh

**Files:**
- Modify: `src/main.rs`

Task 5's `setup_scene` only spawns a terrain mesh for Earth. We need to also spawn one for the Moon. Since the Moon is spawned via `spawn_grid_default` (which returns an entity), we need to capture its ID.

**Step 1: Add Moon terrain mesh spawning**

In `src/main.rs`, after the Moon is spawned via `grid_cmds.spawn_grid_default(...)`, capture its ID and spawn a terrain child. The Moon entity spawning already returns an ID — add `.id()` and spawn the mesh.

In the grid block where Moon is spawned, change to:

```rust
        let moon_id = grid_cmds.spawn_grid_default((
            Moon,
            // ... same as before ...
        )).id();
```

Then after the grid block closes, add:

```rust
    // Moon terrain mesh
    commands.entity(moon_id).with_children(|parent| {
        parent.spawn((
            CellCoord::default(),
            Mesh3d(terrain_mesh.clone()),
            MeshMaterial3d(moon_material),
            Transform::default(),
            NoFrustumCulling,
        ));
    });
```

Note: `moon_id` must be declared outside the grid block scope. Move its declaration to match `earth_id`.

**Step 2: Run and verify**

Run: `cargo run`
Expected: Both Earth and Moon should be visible as SDF-rendered bumpy spheres. Hold 3 to look at Moon.

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "Spawn Moon terrain mesh with SDF material"
```

---

### Task 7: Delete old mesh pipeline files

**Files:**
- Delete: `src/terrain.rs`
- Delete: `src/quadtree.rs`
- Delete: `src/chunk_mesh.rs`
- Delete: `src/lod.rs`
- Delete: `src/mesh_task.rs`

**Step 1: Remove the files**

```bash
git rm src/terrain.rs src/quadtree.rs src/chunk_mesh.rs src/lod.rs src/mesh_task.rs
```

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles cleanly with no warnings about dead code in removed files.

**Step 3: Run the app**

Run: `cargo run`
Expected: Same as before — SDF planets, orbits, camera, starfield, galaxy all working.

**Step 4: Commit**

```bash
git add -A
git commit -m "Remove old mesh-based terrain pipeline (quadtree, LOD, chunk mesh)"
```

---

### Task 8: Final verification and CLAUDE.md update

**Files:**
- Modify: `CLAUDE.md` (project structure section)

**Step 1: Full test run**

Run: `cargo run`

Verify:
- [ ] Earth renders as a bumpy sphere with noise terrain
- [ ] Moon renders as a bumpy sphere with different noise params
- [ ] Terrain gets more detailed as you fly closer (distance-based octave LOD)
- [ ] Sun is a bright emissive sphere at the center
- [ ] Starfield and galaxy render behind everything
- [ ] Orbits work: Earth orbits Sun, Moon orbits Earth
- [ ] Camera: WASD, mouse look, scroll for speed, shift for boost
- [ ] Hotkeys: hold 1/2/3 to track Sun/Earth/Moon
- [ ] F1 toggles wireframe

**Step 2: Update CLAUDE.md project structure**

Update the "Project Structure" section in `CLAUDE.md` to reflect the new file layout:

Old lines to replace:
```
  terrain.rs     - TerrainConfig: noise evaluation + normal computation
  quadtree.rs    - CubeFace, NodeId, FaceQuadtree (pure data, no ECS)
  chunk_mesh.rs  - Per-node mesh generation with seam handling
  lod.rs         - PlanetQuadtree component, ChunkNode component, per-planet LOD systems
  mesh_task.rs   - Async mesh generation using AsyncComputeTaskPool
```

Replace with:
```
  planet_material.rs - PlanetMaterial (Bevy Material), SdfConfig, PlanetSdfUniforms
```

And update `assets/shaders/` to include:
```
  noise.wgsl       - 3D simplex noise + FBM with distance-based octave LOD
  planet_sdf.wgsl  - Terrain SDF sphere tracing, lighting, depth output
```

Also update the Architecture section headers:
- Remove: "Terrain", "Quadtree", "Chunk Meshes", "LOD System", "Async Mesh Generation" sections
- Add a "SDF Terrain Rendering" section describing the new pipeline

**Step 3: Update Key Constants table**

Remove `CHUNK_RESOLUTION`, `MAX_DEPTH`, `SPLIT_THRESHOLD`, `MERGE_THRESHOLD`, `MAX_SPLITS_PER_FRAME`, `PERSPECTIVE_SCALE` — these are from the old mesh pipeline.

Add SDF-relevant constants:
```
| MAX_STEPS | 128 | Ray march step budget |
| SURFACE_EPSILON | 0.01 | SDF hit threshold (~10m) |
| NORMAL_EPSILON | 0.05 | Gradient finite difference step |
```

**Step 4: Commit**

```bash
git add CLAUDE.md
git commit -m "Update CLAUDE.md for SDF terrain pipeline"
```

---

## Summary of Changes

**Files created:**
- `assets/shaders/noise.wgsl` — simplex noise + FBM
- `assets/shaders/planet_sdf.wgsl` — terrain shader
- `src/planet_material.rs` — Bevy Material + SdfConfig

**Files modified:**
- `src/planet.rs` — rewritten for SdfConfig
- `src/main.rs` — rewritten for SDF pipeline
- `CLAUDE.md` — updated documentation

**Files deleted:**
- `src/terrain.rs`
- `src/quadtree.rs`
- `src/chunk_mesh.rs`
- `src/lod.rs`
- `src/mesh_task.rs`

**Dependencies changed:** None (noise crate kept but no longer used for rendering)
