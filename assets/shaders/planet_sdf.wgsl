#import bevy_pbr::mesh_view_bindings::view
#import noise::{simplex3d, fbm}

// ═══════════════════════════════════════════════════════════════════════════
// Uniforms — must match PlanetSdfUniforms in planet_material.rs exactly.
// ═══════════════════════════════════════════════════════════════════════════

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
    // 0 = normal, 1 = octave count, 2 = ray steps, 3 = normals
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

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var<uniform> uniforms: PlanetSdfUniforms;

// ═══════════════════════════════════════════════════════════════════════════
// Vertex shader
// ═══════════════════════════════════════════════════════════════════════════

struct Vertex {
    @location(0) position: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
}

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;

    // Scale unit icosphere to bounding radius (planet_radius + max_elevation),
    // position at planet center. Same pattern as galaxy.wgsl — no mesh_functions.
    let bounding_radius = uniforms.planet_radius + uniforms.max_elevation;
    let world_pos = v.position * bounding_radius + uniforms.planet_center;

    out.clip_position = view.clip_from_world * vec4(world_pos, 1.0);
    out.world_position = world_pos;
    return out;
}

// ═══════════════════════════════════════════════════════════════════════════
// SDF evaluation
// ═══════════════════════════════════════════════════════════════════════════

const MAX_STEPS: u32 = 256u;
const SURFACE_EPSILON: f32 = 0.01; // 10m in km world units

// Our SDF (radial distance to displaced sphere) overestimates the true
// Euclidean distance for non-radial rays. The overestimation is bounded by
// the terrain gradient magnitude: ~sqrt(1 + slope²). For our noise params
// (amp=50, freq=4, radius=1000), max slope ≈ 1.4, gradient ≈ 1.72.
// We step at 1/gradient of the SDF value to prevent overshooting.
const STEP_RELAXATION: f32 = 0.6;

/// Evaluate the planet SDF at point p.
/// Returns signed distance: negative inside the terrain, positive outside.
/// `min_feature_size` is in noise-space units (pre-converted by caller).
fn planet_sdf(p: vec3<f32>, min_feature_size: f32) -> f32 {
    let dir = normalize(p - uniforms.planet_center);
    let noise_val = fbm(
        dir * uniforms.noise_frequency,
        uniforms.noise_octaves,
        uniforms.noise_lacunarity,
        uniforms.noise_persistence,
        min_feature_size,
    );
    let terrain_radius = uniforms.planet_radius + noise_val * uniforms.noise_amplitude;
    return length(p - uniforms.planet_center) - terrain_radius;
}

// ═══════════════════════════════════════════════════════════════════════════
// Ray-sphere intersection
// ═══════════════════════════════════════════════════════════════════════════

/// Intersect ray (origin, dir) with sphere (center, radius).
/// Returns (t_near, t_far). If no intersection, t_near > t_far.
fn ray_sphere(origin: vec3<f32>, dir: vec3<f32>, center: vec3<f32>, radius: f32) -> vec2<f32> {
    let oc = origin - center;
    let b = dot(oc, dir);
    let c = dot(oc, oc) - radius * radius;
    let discriminant = b * b - c;

    if discriminant < 0.0 {
        return vec2(1.0, -1.0); // No intersection sentinel
    }

    let sqrt_d = sqrt(discriminant);
    return vec2(-b - sqrt_d, -b + sqrt_d);
}

// ═══════════════════════════════════════════════════════════════════════════
// Normal computation via central finite differences
// ═══════════════════════════════════════════════════════════════════════════

fn compute_normal(p: vec3<f32>, cam_dist: f32, min_feature_size: f32) -> vec3<f32> {
    // Scale epsilon with distance for stable normals at all ranges.
    let eps = max(cam_dist * 0.0001, SURFACE_EPSILON * 0.5);
    let dx = vec3(eps, 0.0, 0.0);
    let dy = vec3(0.0, eps, 0.0);
    let dz = vec3(0.0, 0.0, eps);

    let n = vec3(
        planet_sdf(p + dx, min_feature_size) - planet_sdf(p - dx, min_feature_size),
        planet_sdf(p + dy, min_feature_size) - planet_sdf(p - dy, min_feature_size),
        planet_sdf(p + dz, min_feature_size) - planet_sdf(p - dz, min_feature_size),
    );

    return normalize(n);
}

// ═══════════════════════════════════════════════════════════════════════════
// Fragment shader
// ═══════════════════════════════════════════════════════════════════════════

struct FragmentOutput {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

// ═══════════════════════════════════════════════════════════════════════════
// Debug helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Compute how many octaves have significant contribution at this feature size.
/// Returns a fractional count (e.g., 3.7) reflecting the smoothstep blend.
fn effective_octave_count(max_octaves: u32, lacunarity: f32, min_feature_size: f32) -> f32 {
    var count = 0.0;
    var frequency = 1.0;
    for (var i = 0u; i < max_octaves; i++) {
        let feature_size = 1.0 / frequency;
        if feature_size < min_feature_size * 0.5 {
            break;
        }
        let blend = smoothstep(0.5 * min_feature_size, 2.0 * min_feature_size, feature_size);
        count += blend;
        frequency *= lacunarity;
    }
    return count;
}

/// Heat-map: 0=blue, 0.25=cyan, 0.5=green, 0.75=yellow, 1=red.
fn heatmap(t: f32) -> vec3<f32> {
    let r = smoothstep(0.5, 0.75, t);
    let g = smoothstep(0.0, 0.25, t) - smoothstep(0.75, 1.0, t);
    let b = 1.0 - smoothstep(0.25, 0.5, t);
    return vec3(r, g, b);
}

// ═══════════════════════════════════════════════════════════════════════════
// Fragment shader
// ═══════════════════════════════════════════════════════════════════════════

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front_facing: bool) -> FragmentOutput {
    let ray_origin = uniforms.camera_position;
    let ray_dir = normalize(in.world_position - ray_origin);

    // Intersect ray with bounding sphere (radius + max_elevation)
    let bounding_radius = uniforms.planet_radius + uniforms.max_elevation;
    let bounds = ray_sphere(ray_origin, ray_dir, uniforms.planet_center, bounding_radius);

    // No intersection with bounding sphere — discard
    if bounds.x > bounds.y {
        discard;
    }

    // Also intersect with a core sphere (radius - max_elevation) to get an
    // early termination bound. If inside this sphere, we've definitely passed
    // through all terrain.
    let core_radius = max(uniforms.planet_radius - uniforms.max_elevation, 0.1);
    let core_bounds = ray_sphere(ray_origin, ray_dir, uniforms.planet_center, core_radius);

    // Determine march start: if camera is outside bounding sphere, start at
    // the near intersection. If inside, start from the camera (t=0).
    var t_start = max(bounds.x, 0.0);

    // End: the far side of the bounding sphere, or the near side of the core
    // sphere (whichever comes first, if core intersection is valid).
    var t_end = bounds.y;
    if core_bounds.x < core_bounds.y && core_bounds.x > t_start {
        t_end = min(t_end, core_bounds.x);
    }

    // Approximate pixel angular size for octave culling.
    // Assumes ~2000px screen width, 60-degree FOV → pixel_angular_size ≈ 0.0005 rad.
    let pixel_angular_size = 0.0005;

    // Sphere tracing
    var t = t_start;
    var hit = false;
    var steps_taken = 0u;

    for (var i = 0u; i < MAX_STEPS; i++) {
        if (t > t_end) {
            break;
        }

        let p = ray_origin + ray_dir * t;
        let cam_dist = length(p - ray_origin);
        
        // Adaptive epsilon: we don't need 10m precision if a pixel is 1km wide.
        // This significantly improves performance for skimming rays at distance.
        let world_pixel_size = cam_dist * pixel_angular_size;
        let epsilon = max(SURFACE_EPSILON, world_pixel_size * 0.5);

        // Convert pixel size from world-space to noise-space for octave culling.
        let min_feature_size = world_pixel_size * uniforms.noise_frequency / uniforms.planet_radius;
        let d = planet_sdf(p, min_feature_size);

        steps_taken = i + 1u;

        if (d < epsilon) {
            hit = true;
            break;
        }

        // Step by the SDF distance, but clamp minimum step to avoid
        // getting stuck on shallow-angle approaches.
        t += max(d * STEP_RELAXATION, epsilon * 0.5);
    }

    if !hit {
        discard;
    }

    // Hit point
    let hit_pos = ray_origin + ray_dir * t;
    let cam_dist_hit = length(hit_pos - ray_origin);
    let min_feature_hit = cam_dist_hit * pixel_angular_size * uniforms.noise_frequency / uniforms.planet_radius;

    // Normal from SDF gradient
    let normal = compute_normal(hit_pos, cam_dist_hit, min_feature_hit);

    // Compute depth: project hit point to clip space, extract depth.
    let clip_pos = view.clip_from_world * vec4(hit_pos, 1.0);
    let ndc_depth = clip_pos.z / clip_pos.w;

    // ── Debug modes ────────────────────────────────────────────────────
    var final_color: vec3<f32>;

    if uniforms.debug_mode == 1u {
        // Octave count: blue (0) → red (max_octaves)
        let octaves = effective_octave_count(
            uniforms.noise_octaves,
            uniforms.noise_lacunarity,
            min_feature_hit,
        );
        final_color = heatmap(octaves / f32(uniforms.noise_octaves));
    } else if uniforms.debug_mode == 2u {
        // Ray step count: blue (1 step) → red (MAX_STEPS)
        final_color = heatmap(f32(steps_taken) / f32(MAX_STEPS));
    } else if uniforms.debug_mode == 3u {
        // Surface normals mapped to RGB
        final_color = normal * 0.5 + 0.5;
    } else {
        // Normal rendering: Lambertian lighting
        let ambient = 0.03;
        let n_dot_l = max(dot(normal, uniforms.sun_direction), 0.0);
        let diffuse = 0.9 * n_dot_l;
        let brightness = ambient + diffuse;
        let base_color = vec3(0.5, 0.5, 0.5);
        final_color = base_color * brightness;
    }

    var out: FragmentOutput;
    out.color = vec4(final_color, 1.0);
    out.depth = ndc_depth;
    return out;
}
