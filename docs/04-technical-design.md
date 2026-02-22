# Technical Design Details

## 1. SDF Terrain Design

### Parameterized WGSL with Rust Configuration

The SDF is evaluated entirely on the GPU in WGSL. Rust controls configuration via a uniform buffer. This gives creative iteration in Rust (parameters) with infinite-detail rendering on the GPU.

### Rust Configuration Struct

```rust
#[derive(ShaderType, Clone, Copy)]
pub struct PlanetSdfUniforms {
    // Geometry
    pub planet_center: Vec3,
    pub planet_radius: f32,
    pub camera_position: Vec3,
    pub max_elevation: f32,
    pub sun_direction: Vec3,
    pub _padding: f32,

    // Continental noise
    pub continental_frequency: f32,
    pub continental_amplitude: f32,
    pub continental_octaves: u32,
    pub continental_lacunarity: f32,
    pub continental_persistence: f32,

    // Ridge noise
    pub ridge_frequency: f32,
    pub ridge_amplitude: f32,

    // Cave system
    pub cave_frequency: f32,
    pub cave_threshold: f32,
    pub cave_enabled: u32,

    // Domain warping
    pub warp_frequency: f32,
    pub warp_amplitude: f32,
}
```

Note: WGSL struct layout must match Rust struct layout exactly. `vec3<f32>` has 16-byte alignment — pair with a trailing `f32` to fill the row (see CLAUDE.md notes on ShaderType alignment).

### WGSL SDF Evaluation

```wgsl
fn planet_sdf(p: vec3f, cam_dist: f32) -> f32 {
    let dir = normalize(p - u.planet_center);
    let r = length(p - u.planet_center);

    // Base sphere
    var d = r - u.planet_radius;

    // Continental displacement
    d -= fbm_lod(dir * u.continental_freq, u.continental_octaves,
                 u.continental_lacunarity, u.continental_persistence,
                 cam_dist) * u.continental_amp;

    // Ridge noise
    d -= ridge_noise(dir * u.ridge_freq) * u.ridge_amp;

    // Cave carving (3D, operates on absolute position)
    if (u.cave_enabled != 0u) {
        let cave = fbm_3d(p * u.cave_freq, 4u);
        if (cave > u.cave_threshold) {
            d = max(d, cave - u.cave_threshold);
        }
    }

    return d;
}
```

### Gradient / Normal Computation

Normals are computed via central-difference gradient of the SDF:

```wgsl
fn sdf_normal(p: vec3f, cam_dist: f32) -> vec3f {
    // Epsilon scales with distance for numerical stability
    let eps = max(cam_dist * 0.0001, 0.001);
    let e = vec2(eps, 0.0);
    return normalize(vec3(
        planet_sdf(p + e.xyy, cam_dist) - planet_sdf(p - e.xyy, cam_dist),
        planet_sdf(p + e.yxy, cam_dist) - planet_sdf(p - e.yxy, cam_dist),
        planet_sdf(p + e.yyx, cam_dist) - planet_sdf(p - e.yyx, cam_dist),
    ));
}
```

The epsilon scales with camera distance — preventing jittery normals at orbital distance (where a tiny epsilon samples nearly identical density values) and overly smooth normals at surface level (where a large epsilon averages over real features).

---

## 2. Sphere Tracing (Raymarching)

### Algorithm

```wgsl
struct HitResult {
    hit: bool,
    position: vec3f,
    distance: f32,
    steps: u32,
}

fn sphere_trace(ro: vec3f, rd: vec3f, max_dist: f32) -> HitResult {
    var t = 0.0;
    let cam_pos = u.camera_position;

    for (var i = 0u; i < MAX_STEPS; i++) {
        let p = ro + rd * t;
        let cam_dist = length(p - cam_pos);
        let d = planet_sdf(p, cam_dist);

        if (d < SURFACE_EPSILON) {
            return HitResult(true, p, t, i);
        }

        // SDF-guided step — the distance tells us how far we can safely advance
        t += d;

        if (t > max_dist) { break; }
    }

    return HitResult(false, vec3(0.0), 0.0, MAX_STEPS);
}
```

### Key Parameters

| Parameter | Value | Rationale |
|-----------|-------|-----------|
| `MAX_STEPS` | 128 | Generous budget; most rays converge in 30-60 steps |
| `SURFACE_EPSILON` | 0.001 | Surface hit threshold (1 meter in world units of km) |
| `max_dist` | Derived from ray-sphere intersection | Don't march past the planet's far side |

### Ray Setup in Fragment Shader

The vertex shader passes the icosphere mesh's world-space position. The fragment shader:
1. Computes the ray: `origin = camera_position`, `direction = normalize(fragment_world_pos - camera_position)`
2. Clips to the bounding sphere (ray-sphere intersection) to find the starting `t`
3. Sphere-traces from that starting point inward
4. If the camera is inside the bounding sphere, starts at `t = 0` (the fragment itself)

### Depth Output

After finding the hit point, project it to clip space and write fragment depth:

```wgsl
// In fragment shader, after finding hit_pos:
let clip_pos = view.clip_from_world * vec4(hit_pos, 1.0);
*frag_depth = clip_pos.z / clip_pos.w;
```

This makes the terrain participate correctly in depth testing. The atmosphere shader reads this depth to clip its ray march at the terrain surface.

---

## 3. Distance-Based Octave LOD

### The Core Idea

FBM (fractal Brownian motion) evaluates multiple octaves of noise, each at double the frequency and reduced amplitude. At orbital distance, the high-frequency octaves produce features smaller than a pixel — evaluating them wastes GPU cycles and can cause aliasing.

The shader skips octaves whose feature size is below the pixel threshold:

```wgsl
fn fbm_lod(p: vec3f, max_octaves: u32, lacunarity: f32,
           persistence: f32, cam_dist: f32) -> f32 {
    // Size of one pixel at this distance (approximate)
    let pixel_size = cam_dist * u_pixel_angular_size;

    var result = 0.0;
    var freq = 1.0;
    var amp = 1.0;

    for (var i = 0u; i < max_octaves; i++) {
        // Feature size for this octave
        let feature_size = amp / freq;
        if (feature_size < pixel_size * 0.5) { break; }

        result += simplex3d(p * freq) * amp;
        freq *= lacunarity;
        amp *= persistence;
    }

    return result;
}
```

### Octave Count at Various Distances

For a planet with radius 6360 km, frequency 1.0, lacunarity 2.0, persistence 0.5:

| Camera Distance | Approx Pixel Size | Active Octaves | Visual Detail |
|----------------|-------------------|----------------|---------------|
| 100,000 km | ~100 km | 2-3 | Smooth sphere with continental bumps |
| 10,000 km | ~10 km | 5-6 | Mountain ranges visible |
| 1,000 km | ~1 km | 8-9 | Individual peaks and valleys |
| 100 km | ~100 m | 10-11 | Terrain texture, ridges |
| 10 km | ~10 m | 12-14 | Rocks, boulders, fine detail |
| 1 km | ~1 m | 14-16 | Ground-level detail |

---

## 4. Material Pipeline (Bevy Integration)

### PlanetMaterial

```rust
#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct PlanetMaterial {
    #[uniform(0)]
    pub uniforms: PlanetSdfUniforms,
}

impl Material for PlanetMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/planet_sdf.wgsl".into()
    }

    fn vertex_shader() -> ShaderRef {
        "shaders/planet_sdf.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Opaque
    }

    fn specialize(
        _pipeline: &MaterialPipeline<Self>,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayout,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // No backface culling — camera can be inside bounding sphere
        descriptor.primitive.cull_mode = None;
        // Depth write enabled — terrain participates in depth testing
        if let Some(ref mut depth) = descriptor.depth_stencil {
            depth.depth_write_enabled = true;
        }
        Ok(())
    }
}
```

### Uniform Update System

A system runs each frame to update the material uniforms with current camera position, sun direction, etc.:

```rust
fn update_planet_materials(
    camera_q: Query<&GlobalTransform, With<FloatingOrigin>>,
    planet_q: Query<(&Planet, &GlobalTransform)>,
    sun_q: Query<&GlobalTransform, With<Sun>>,
    mut materials: ResMut<Assets<PlanetMaterial>>,
) {
    let cam_gt = camera_q.single();
    let sun_gt = sun_q.single();

    for (planet, planet_gt) in &planet_q {
        if let Some(mat) = materials.get_mut(&planet.material_handle) {
            mat.uniforms.camera_position = cam_gt.translation();
            mat.uniforms.planet_center = planet_gt.translation();
            mat.uniforms.sun_direction =
                (sun_gt.translation() - planet_gt.translation()).normalize();
        }
    }
}
```

### Entity Spawning

```rust
// Terrain icosphere — bounding sphere
let terrain_mesh = meshes.add(
    Sphere::new(1.0).mesh().ico(5)  // unit icosphere, shader positions it
);
let terrain_material = materials.add(PlanetMaterial {
    uniforms: PlanetSdfUniforms {
        planet_radius: 6360.0,
        max_elevation: 30.0,
        continental_frequency: 2.0,
        continental_amplitude: 15.0,
        continental_octaves: 14,
        // ...
    },
});

// Spawn as child of planet entity
commands.entity(planet_entity).with_children(|parent| {
    parent.spawn((
        Mesh3d(terrain_mesh),
        MeshMaterial3d(terrain_material),
        Transform::default(),
        NoFrustumCulling,
    ));
});
```

---

## 5. Noise Implementation (WGSL)

### 3D Simplex Noise

Port the well-known Ashima/webgl-noise simplex noise to WGSL. This implementation is:
- Self-contained (no texture lookups)
- Deterministic
- GPU-optimized (uses permutation polynomials instead of lookup tables)

Key functions needed:
- `simplex3d(p: vec3f) -> f32` — single octave, range [-1, 1]
- `fbm_lod(p, octaves, lacunarity, persistence, cam_dist) -> f32` — distance-aware FBM
- `ridge_noise(p: vec3f) -> f32` — `1.0 - abs(simplex3d(p))` for sharp ridges
- `fbm_3d(p: vec3f, octaves: u32) -> f32` — non-LOD FBM for cave volumes

### Noise Quality Considerations

- Simplex noise avoids the axis-aligned artifacts of Perlin noise
- For planet terrain, the noise is evaluated on the normalized direction vector (unit sphere surface), not in 3D volume space — this prevents elevation-dependent frequency shifts
- Domain warping (`p + noise(p) * warp_amp`) adds organic, non-repetitive terrain shapes
- Ridge noise (`1 - |noise|`) produces mountain-ridge-like features

---

## 6. Atmosphere Integration

The existing atmosphere shader (`atmosphere.wgsl`) already supports depth-prepass-based ray clamping:
- Camera has `DepthPrepass` component
- Atmosphere fragment shader reads `texture_depth_2d` at `@group(0) @binding(20)`
- Reconstructs world position from depth to limit ray march distance

With SDF terrain writing correct `frag_depth`, the atmosphere automatically clips at the terrain surface. No special integration code needed beyond:
1. Ensuring the camera has `DepthPrepass`
2. Ensuring the SDF shader writes `frag_depth`
3. The atmosphere icosphere renders after the terrain (render graph ordering)

---

## 7. Orbital Mechanics (complete, unchanged)

See Phase 1.5 documentation. The `Orbit` component and `update_orbits` system work independently of the rendering pipeline. The only connection is that `sun_direction` is computed from the star's position relative to each planet and passed to the shader uniforms.

---

## 8. Per-Planet Configuration

```rust
#[derive(Component)]
pub struct Planet {
    pub name: String,
    pub sdf_config: SdfConfig,
    pub has_atmosphere: bool,
    pub atmosphere_config: Option<AtmosphereConfig>,
    pub material_handle: Handle<PlanetMaterial>,
}

pub struct SdfConfig {
    pub radius: f32,
    pub max_elevation: f32,
    // Noise layers
    pub continental_frequency: f32,
    pub continental_amplitude: f32,
    pub continental_octaves: u32,
    pub continental_lacunarity: f32,
    pub continental_persistence: f32,
    pub ridge_frequency: f32,
    pub ridge_amplitude: f32,
    // Volumetric features
    pub cave_frequency: f32,
    pub cave_threshold: f32,
    pub cave_enabled: bool,
    // Domain warping
    pub warp_frequency: f32,
    pub warp_amplitude: f32,
    // Coloring (Phase 4)
    pub ocean_level: f32,
    pub snow_line: f32,
    pub color_palette: ColorPalette,
}
```

Each planet spawns its own terrain icosphere with its own `PlanetMaterial`. The uniform update system iterates over all planets each frame.

---

## 9. Performance Considerations

### GPU Cost Per Pixel

Each terrain pixel requires:
- 1 ray-sphere intersection (cheap)
- N sphere-trace steps, each evaluating the SDF
- Each SDF evaluation: M noise octaves (distance-dependent)
- 6 extra SDF evaluations for the normal gradient
- 1 lighting calculation

Worst case (close to surface, 128 steps, 14 octaves, + 6 normal evaluations): ~128 * 14 + 6 * 14 = ~1876 simplex noise evaluations per pixel. In practice, sphere tracing converges much faster — 30-60 steps typical, with early octaves dominating the SDF value and allowing larger steps.

### Optimization Strategies (Phase 7)

1. **Bounding sphere early-out**: Skip planets whose bounding sphere is off-screen or too small
2. **Adaptive step count**: Reduce MAX_STEPS for distant planets
3. **Relaxed sphere tracing**: Multiply SDF distance by 1.2-1.5 for faster convergence (slight over-stepping, correct by bisection at the end)
4. **Temporal reprojection**: Reuse previous frame's depth to seed ray start position
5. **Lower resolution for distant planets**: Render small/far planets at reduced resolution
