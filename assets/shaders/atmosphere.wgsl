#import bevy_pbr::mesh_view_bindings::view

struct AtmosphereUniforms {
    planet_center: vec3<f32>,
    planet_radius: f32,
    sun_direction: vec3<f32>,
    atmo_radius: f32,
    settings: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: AtmosphereUniforms;

// ── Depth prepass texture for terrain awareness ─────────────────────────────
#ifdef DEPTH_PREPASS
#ifdef MULTISAMPLED
@group(0) @binding(20) var depth_prepass_texture: texture_depth_multisampled_2d;
#else
@group(0) @binding(20) var depth_prepass_texture: texture_depth_2d;
#endif
#endif

// ── Scattering constants (in km, 1 world unit = 1 km) ──────────────────────
const BETA_R: vec3<f32> = vec3(5.5e-3, 13.0e-3, 22.4e-3); // Rayleigh per km
const H_R: f32 = 8.0;                                       // Rayleigh scale height km
const BETA_M: f32 = 21e-3;                                  // Mie per km
const H_M: f32 = 1.2;                                       // Mie scale height km
const G_MIE: f32 = 0.76;                                    // Mie anisotropy

const NUM_VIEW_STEPS: i32 = 16;
const NUM_LIGHT_STEPS: i32 = 4;
const SUN_INTENSITY: f32 = 20.0;
const PI: f32 = 3.14159265;

struct Vertex {
    @location(0) position: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
}

// ── Helper functions ────────────────────────────────────────────────────────

// Returns (t_near, t_far). t_near > t_far means no intersection.
fn ray_sphere(origin: vec3<f32>, dir: vec3<f32>, center: vec3<f32>, radius: f32) -> vec2<f32> {
    let oc = origin - center;
    let b = dot(oc, dir);
    let c = dot(oc, oc) - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return vec2(1e20, -1e20);
    }
    let s = sqrt(disc);
    return vec2(-b - s, -b + s);
}

fn rayleigh_phase(cos_theta: f32) -> f32 {
    return 3.0 / (16.0 * PI) * (1.0 + cos_theta * cos_theta);
}

fn hg_phase(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    let denom = 1.0 + g2 - 2.0 * g * cos_theta;
    return (1.0 - g2) / (4.0 * PI * pow(denom, 1.5));
}

// Optical depth from a point toward the sun through the atmosphere.
fn sun_optical_depth(pos: vec3<f32>, sun_dir: vec3<f32>) -> vec3<f32> {
    let pc = material.planet_center;
    let pr = material.planet_radius;
    let ar = material.atmo_radius;

    // Check planet shadow
    let planet_hit = ray_sphere(pos, sun_dir, pc, pr);
    if planet_hit.x < planet_hit.y && planet_hit.x > 0.0 {
        return vec3(1e10);
    }

    let atmo_hit = ray_sphere(pos, sun_dir, pc, ar);
    let t_max = max(atmo_hit.y, 0.0);
    if t_max <= 0.0 {
        return vec3(0.0);
    }

    let step_size = t_max / f32(NUM_LIGHT_STEPS);
    var depth = vec3(0.0);

    for (var i: i32 = 0; i < NUM_LIGHT_STEPS; i++) {
        let t = (f32(i) + 0.5) * step_size;
        let p = pos + sun_dir * t;
        let alt = max(length(p - pc) - pr, 0.0);

        depth += (BETA_R * exp(-alt / H_R) + vec3(BETA_M) * exp(-alt / H_M)) * step_size;
    }

    return depth;
}

// ── Vertex shader ───────────────────────────────────────────────────────────

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    // Unit sphere vertex → world position on the atmosphere shell
    let world_pos = v.position * material.atmo_radius + material.planet_center;
    out.clip_position = view.clip_from_world * vec4(world_pos, 1.0);
    out.world_position = world_pos;
    return out;
}

// ── Fragment shader ─────────────────────────────────────────────────────────

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    let cam_pos = view.world_position;
    let pc = material.planet_center;
    let pr = material.planet_radius;
    let ar = material.atmo_radius;

    // Decide which face to shade to avoid double-draw.
    // Outside atmosphere: shade front faces only.
    // Inside atmosphere: shade back faces only.
    let cam_dist = length(cam_pos - pc);
    let inside = cam_dist < ar;

    if is_front && inside {
        discard;
    }
    if !is_front && !inside {
        discard;
    }

    let ray_dir = normalize(in.world_position - cam_pos);

    // Intersect the view ray with atmosphere and planet spheres
    let atmo_hit = ray_sphere(cam_pos, ray_dir, pc, ar);
    if atmo_hit.x > atmo_hit.y {
        discard;
    }

    let t_enter = max(atmo_hit.x, 0.0);
    var t_exit = atmo_hit.y;

    // Clamp to idealized planet sphere (fallback when depth prepass unavailable)
    let planet_hit = ray_sphere(cam_pos, ray_dir, pc, pr);
    if planet_hit.x < planet_hit.y && planet_hit.x > 0.0 {
        t_exit = min(t_exit, planet_hit.x);
    }

    // Clamp to actual terrain depth from the depth prepass
#ifdef DEPTH_PREPASS
    let pixel = vec2<i32>(in.clip_position.xy);
    let scene_depth = textureLoad(depth_prepass_texture, pixel, 0);
    if scene_depth > 0.0 {
        // Reconstruct terrain world position from depth buffer
        let uv = (in.clip_position.xy - view.viewport.xy) / view.viewport.zw;
        let ndc_xy = uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0);
        let world_h = view.world_from_clip * vec4(ndc_xy, scene_depth, 1.0);
        let terrain_pos = world_h.xyz / world_h.w;
        let t_terrain = length(terrain_pos - cam_pos);
        t_exit = min(t_exit, t_terrain);
    }
#endif

    let path_length = t_exit - t_enter;
    if path_length <= 0.0 {
        discard;
    }

    // Ray march through the atmosphere shell
    let step_size = path_length / f32(NUM_VIEW_STEPS);
    let sun_dir = material.sun_direction;
    let cos_theta = dot(ray_dir, sun_dir);
    let phase_r = rayleigh_phase(cos_theta);
    let phase_m = hg_phase(cos_theta, G_MIE);

    var transmittance = vec3(1.0);
    var in_scatter = vec3(0.0);

    for (var i: i32 = 0; i < NUM_VIEW_STEPS; i++) {
        let t = t_enter + (f32(i) + 0.5) * step_size;
        let pos = cam_pos + ray_dir * t;
        let altitude = max(length(pos - pc) - pr, 0.0);

        let rho_r = exp(-altitude / H_R);
        let rho_m = exp(-altitude / H_M);

        // Extinction for this step
        let tau_step = (BETA_R * rho_r + vec3(BETA_M) * rho_m) * step_size;

        // Light reaching this point from the sun
        let light_tau = sun_optical_depth(pos, sun_dir);
        let sun_atten = exp(-light_tau);

        // Accumulated in-scatter
        let scatter = (BETA_R * rho_r * phase_r + vec3(BETA_M) * rho_m * phase_m) * sun_atten;
        in_scatter += scatter * transmittance * step_size;

        transmittance *= exp(-tau_step);
    }

    // Premultiplied alpha output:
    //   blend equation: final = src.rgb + dst.rgb * (1 - src.a)
    //   result: in_scatter + scene * avg_transmittance
    let avg_transmittance = dot(transmittance, vec3(0.2126, 0.7152, 0.0722));
    return vec4(in_scatter * SUN_INTENSITY, 1.0 - avg_transmittance);
}
