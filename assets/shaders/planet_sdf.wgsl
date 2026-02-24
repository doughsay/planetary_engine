#import bevy_pbr::mesh_view_bindings::view
#import noise::{simplex3d, fbm, hash33}

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
    min_feature_size: f32, // Distance-based culling
) -> f32 {
    let cell_size = 1.0 / cell_freq;
    if (cell_size < min_feature_size * 0.2) {
        return 0.0;
    }

    let p = dir * cell_freq;
    let cell = floor(p);

    var displacement = 0.0;

    for (var dx: i32 = -1; dx <= 1; dx += 1) {
        for (var dy: i32 = -1; dy <= 1; dy += 1) {
            for (var dz: i32 = -1; dz <= 1; dz += 1) {
                let neighbor = cell + vec3<f32>(f32(dx), f32(dy), f32(dz));
                let h = hash33(neighbor);

                if (h.z > density) { continue; }

                let crater_pos = neighbor + h * 0.8 + 0.1;
                let d = length(p - crater_pos);
                let crater_radius = 0.3 + fract(h.x * 13.7 + h.y * 7.3) * 0.2;
                let r = d / crater_radius;

                if (r > 2.0) { continue; }

                displacement += crater_profile(r, depth, rim_height, peak_height);
            }
        }
    }

    // Blend out as we reach the pixel size limit
    return displacement * smoothstep(min_feature_size * 0.2, min_feature_size * 0.5, cell_size);
}

// ═══════════════════════════════════════════════════════════════════════════
// SDF evaluation
// ═══════════════════════════════════════════════════════════════════════════

const MAX_STEPS: u32 = 256u; 
const SURFACE_EPSILON: f32 = 0.005; // 5m in km units

/// Evaluate the planet SDF with full detail.
fn planet_sdf(p: vec3<f32>, min_feature_size: f32) -> f32 {
    let dist_to_center = length(p - uniforms.planet_center);
    let dir = (p - uniforms.planet_center) / dist_to_center;

    var elevation = fbm(
        dir * uniforms.noise_frequency,
        uniforms.noise_octaves,
        uniforms.noise_lacunarity,
        uniforms.noise_persistence,
        min_feature_size,
    ) * uniforms.noise_amplitude;

    if (uniforms.crater_enabled != 0u) {
        elevation += crater_field(dir,
            uniforms.crater_frequency_0, uniforms.crater_depth_0,
            uniforms.crater_rim_height_0, uniforms.crater_peak_height_0,
            uniforms.crater_density_0, min_feature_size);
        
        elevation += crater_field(dir,
            uniforms.crater_frequency_1, uniforms.crater_depth_1,
            uniforms.crater_rim_height_1, uniforms.crater_peak_height_1,
            uniforms.crater_density_1, min_feature_size);
            
        elevation += crater_field(dir,
            uniforms.crater_frequency_2, uniforms.crater_depth_2,
            uniforms.crater_rim_height_2, uniforms.crater_peak_height_2,
            uniforms.crater_density_2, min_feature_size);
    }

    return dist_to_center - (uniforms.planet_radius + elevation);
}

// ═══════════════════════════════════════════════════════════════════════════
// Normal computation
// ═══════════════════════════════════════════════════════════════════════════

fn compute_normal(p: vec3<f32>, cam_dist: f32, min_feature_size: f32) -> vec3<f32> {
    // Scale epsilon with distance for stability.
    let eps = max(cam_dist * 0.0001, SURFACE_EPSILON * 0.5);
    
    let d = planet_sdf(p, min_feature_size);
    let n = vec3(
        planet_sdf(p + vec3(eps, 0.0, 0.0), min_feature_size) - d,
        planet_sdf(p + vec3(0.0, eps, 0.0), min_feature_size) - d,
        planet_sdf(p + vec3(0.0, 0.0, eps), min_feature_size) - d,
    );

    return normalize(n);
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

    if (discriminant < 0.0) {
        return vec2(1.0, -1.0);
    }

    let sqrt_d = sqrt(discriminant);
    return vec2(-b - sqrt_d, -b + sqrt_d);
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

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front_facing: bool) -> FragmentOutput {
    let ray_origin = uniforms.camera_position;
    let ray_dir = normalize(in.world_position - ray_origin);

    let bounding_radius = uniforms.planet_radius + uniforms.max_elevation;
    let bounds = ray_sphere(ray_origin, ray_dir, uniforms.planet_center, bounding_radius);

    if (bounds.x > bounds.y) { discard; }

    var t = max(bounds.x, 0.0);
    var t_end = bounds.y;

    let pixel_angular_size = 0.0005;
    var hit = false;
    var steps_taken = 0u;

    for (var i = 0u; i < MAX_STEPS; i++) {
        if (t > t_end) { break; }

        let p = ray_origin + ray_dir * t;
        let dist_to_center = length(p - uniforms.planet_center);
        let d_sphere = dist_to_center - uniforms.planet_radius;

        // Adaptive quality traversal:
        // Use a strictly conservative bound when far from the possible surface.
        let safe_bound = d_sphere - uniforms.max_elevation;
        
        var d: f32;
        var in_detail_zone = false;

        if (safe_bound > 2.0) {
            d = safe_bound;
        } else {
            in_detail_zone = true;
            let world_pixel_size = t * pixel_angular_size;
            let min_feature_size = world_pixel_size * uniforms.noise_frequency / uniforms.planet_radius;
            d = planet_sdf(p, min_feature_size);
        }

        steps_taken = i + 1u;

        if (in_detail_zone) {
            let world_pixel_size = t * pixel_angular_size;
            let epsilon = clamp(world_pixel_size * 0.5, SURFACE_EPSILON, 0.05);
            
            if (d < epsilon) {
                hit = true;
                break;
            }
            
            // Be more conservative at grazing angles to fix holes.
            // We can detect grazing by checking how 'd' changes, but a simple 
            // relaxation factor that scales with distance to center is easier.
            let relaxation = mix(0.4, 0.8, smoothstep(0.0, 5.0, d));
            t += max(d * relaxation, epsilon * 0.5);
        } else {
            // Far jump: no relaxation needed for the conservative bound.
            t += d; 
        }
    }

    if (!hit) { discard; }

    let hit_pos = ray_origin + ray_dir * t;
    let cam_dist_hit = t;
    let world_pixel_hit = cam_dist_hit * pixel_angular_size;
    let min_feature_hit = world_pixel_hit * uniforms.noise_frequency / uniforms.planet_radius;

    let normal = compute_normal(hit_pos, cam_dist_hit, min_feature_hit);
    let clip_pos = view.clip_from_world * vec4(hit_pos, 1.0);
    let ndc_depth = clip_pos.z / clip_pos.w;

    var final_color: vec3<f32>;

    if (uniforms.debug_mode == 1u) {
        let octaves = effective_octave_count(uniforms.noise_octaves, uniforms.noise_lacunarity, min_feature_hit);
        final_color = heatmap(octaves / f32(uniforms.noise_octaves));
    } else if (uniforms.debug_mode == 2u) {
        final_color = heatmap(f32(steps_taken) / f32(MAX_STEPS));
    } else if (uniforms.debug_mode == 3u) {
        final_color = normal * 0.5 + 0.5;
    } else {
        let ambient = 0.05;
        let n_dot_l = max(dot(normal, uniforms.sun_direction), 0.0);
        let diffuse = 0.9 * n_dot_l;
        final_color = vec3(0.5, 0.5, 0.5) * (ambient + diffuse);
    }

    var out: FragmentOutput;
    out.color = vec4(final_color, 1.0);
    out.depth = ndc_depth;
    return out;
}
