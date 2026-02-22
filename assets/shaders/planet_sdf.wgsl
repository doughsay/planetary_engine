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

const MAX_STEPS: u32 = 128u;
const SURFACE_EPSILON: f32 = 0.01; // 10m in km world units

/// Evaluate the planet SDF at point p.
/// Returns signed distance: negative inside the terrain, positive outside.
fn planet_sdf(p: vec3<f32>, cam_dist: f32, pixel_size: f32) -> f32 {
    let dir = normalize(p - uniforms.planet_center);
    let noise_val = fbm(
        dir * uniforms.noise_frequency,
        uniforms.noise_octaves,
        uniforms.noise_lacunarity,
        uniforms.noise_persistence,
        cam_dist,
        pixel_size,
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

fn compute_normal(p: vec3<f32>, cam_dist: f32, pixel_size: f32) -> vec3<f32> {
    // Scale epsilon with distance for stable normals at all ranges.
    let eps = max(cam_dist * 0.0001, SURFACE_EPSILON * 0.5);
    let dx = vec3(eps, 0.0, 0.0);
    let dy = vec3(0.0, eps, 0.0);
    let dz = vec3(0.0, 0.0, eps);

    let n = vec3(
        planet_sdf(p + dx, cam_dist, pixel_size) - planet_sdf(p - dx, cam_dist, pixel_size),
        planet_sdf(p + dy, cam_dist, pixel_size) - planet_sdf(p - dy, cam_dist, pixel_size),
        planet_sdf(p + dz, cam_dist, pixel_size) - planet_sdf(p - dz, cam_dist, pixel_size),
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

    // Approximate pixel size for octave culling.
    // Assumes ~2000px screen width, 60-degree FOV → pixel_angular_size ≈ 0.0005 rad.
    let pixel_angular_size = 0.0005;

    // Sphere tracing
    var t = t_start;
    var hit = false;

    for (var i = 0u; i < MAX_STEPS; i++) {
        if t > t_end {
            break;
        }

        let p = ray_origin + ray_dir * t;
        let cam_dist = length(p - ray_origin);
        let pixel_size = cam_dist * pixel_angular_size;
        let d = planet_sdf(p, cam_dist, pixel_size);

        if d < SURFACE_EPSILON {
            hit = true;
            break;
        }

        // Step by the SDF distance, but clamp minimum step to avoid
        // getting stuck on shallow-angle approaches.
        t += max(d, SURFACE_EPSILON * 0.5);
    }

    if !hit {
        discard;
    }

    // Hit point
    let hit_pos = ray_origin + ray_dir * t;
    let cam_dist_hit = length(hit_pos - ray_origin);
    let pixel_size_hit = cam_dist_hit * pixel_angular_size;

    // Normal from SDF gradient
    let normal = compute_normal(hit_pos, cam_dist_hit, pixel_size_hit);

    // Lambertian lighting
    let ambient = 0.03;
    let n_dot_l = max(dot(normal, uniforms.sun_direction), 0.0);
    let diffuse = 0.9 * n_dot_l;
    let brightness = ambient + diffuse;

    // Gray base color for now
    let base_color = vec3(0.5, 0.5, 0.5);
    let lit_color = base_color * brightness;

    // Compute depth: project hit point to clip space, extract depth.
    let clip_pos = view.clip_from_world * vec4(hit_pos, 1.0);
    let ndc_depth = clip_pos.z / clip_pos.w;

    var out: FragmentOutput;
    out.color = vec4(lit_color, 1.0);
    out.depth = ndc_depth;
    return out;
}
