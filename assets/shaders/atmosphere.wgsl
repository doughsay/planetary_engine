#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;

struct AtmosphereUniforms {
    camera_position: vec3<f32>,
    planet_radius: f32,
    planet_center: vec3<f32>,
    atmo_radius: f32,
    sun_direction: vec3<f32>,
    scene_units_to_m: f32,
    camera_forward: vec3<f32>,
    fov_tan_half: f32,
    camera_right: vec3<f32>,
    aspect_ratio: f32,
    camera_up: vec3<f32>,
    _padding: f32,
};

@group(0) @binding(2) var<uniform> atmo: AtmosphereUniforms;

// ---------------------------------------------------------------------------
// Scattering constants (Earth-like, values in per-meter)
// ---------------------------------------------------------------------------

// Rayleigh scattering coefficients at sea level (wavelength-dependent).
const BETA_R: vec3<f32> = vec3(5.5e-6, 13.0e-6, 22.4e-6);
// Rayleigh scale height in meters.
const H_R: f32 = 8000.0;

// Mie scattering coefficient at sea level (wavelength-independent grey).
const BETA_M: f32 = 21e-6;
// Mie scale height in meters.
const H_M: f32 = 1200.0;
// Mie preferred scattering direction (Henyey-Greenstein g parameter).
const G_MIE: f32 = 0.76;

// Ray-march quality.
const VIEW_STEPS: i32 = 16;
const LIGHT_STEPS: i32 = 4;

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

// Ray-sphere intersection. Returns (t_near, t_far) or (-1, -1) on miss.
fn ray_sphere(origin: vec3<f32>, dir: vec3<f32>, center: vec3<f32>, radius: f32) -> vec2<f32> {
    let oc = origin - center;
    let b = dot(oc, dir);
    let c = dot(oc, oc) - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return vec2(-1.0, -1.0);
    }
    let d = sqrt(disc);
    return vec2(-b - d, -b + d);
}

// Rayleigh phase function.
fn phase_rayleigh(cos_theta: f32) -> f32 {
    return 3.0 / (16.0 * 3.14159265) * (1.0 + cos_theta * cos_theta);
}

// Henyey-Greenstein phase function for Mie scattering.
fn phase_mie(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    let denom = 1.0 + g2 - 2.0 * g * cos_theta;
    return 3.0 / (8.0 * 3.14159265) * (1.0 - g2) / (denom * sqrt(denom) * (2.0 + g2));
}

// ---------------------------------------------------------------------------
// Main fragment
// ---------------------------------------------------------------------------

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let scene_color = textureSample(screen_texture, screen_sampler, in.uv);

    // Reconstruct view ray from camera vectors and UV.
    let ndc = vec2(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let ray_dir = normalize(
        atmo.camera_forward
        + ndc.x * atmo.camera_right * atmo.fov_tan_half * atmo.aspect_ratio
        + ndc.y * atmo.camera_up * atmo.fov_tan_half
    );
    let ray_origin = atmo.camera_position;

    // Intersect with atmosphere shell.
    let t_atmo = ray_sphere(ray_origin, ray_dir, atmo.planet_center, atmo.atmo_radius);
    if t_atmo.y < 0.0 {
        // Ray misses atmosphere entirely.
        return scene_color;
    }

    // Intersect with planet surface to know where the ground blocks the view.
    let t_planet = ray_sphere(ray_origin, ray_dir, atmo.planet_center, atmo.planet_radius);

    // Determine the segment of the ray that passes through the atmosphere.
    let t_start = max(t_atmo.x, 0.0);
    var t_end = t_atmo.y;
    if t_planet.x > 0.0 {
        // Ray hits the planet; atmosphere ends at the planet surface.
        t_end = min(t_end, t_planet.x);
    }

    let step_len = (t_end - t_start) / f32(VIEW_STEPS);
    let scale = atmo.scene_units_to_m; // world units → meters

    // Accumulators.
    var sum_r = vec3(0.0);
    var sum_m = vec3(0.0);
    var optical_depth_r = 0.0;
    var optical_depth_m = 0.0;

    // March along the view ray through the atmosphere.
    for (var i = 0; i < VIEW_STEPS; i++) {
        let t = t_start + (f32(i) + 0.5) * step_len;
        let sample_pos = ray_origin + ray_dir * t;

        // Height above the planet surface in meters.
        let height_m = (length(sample_pos - atmo.planet_center) - atmo.planet_radius) * scale;

        // Local density at this height.
        let density_r = exp(-height_m / H_R) * step_len * scale;
        let density_m = exp(-height_m / H_M) * step_len * scale;
        optical_depth_r += density_r;
        optical_depth_m += density_m;

        // Light ray toward the sun: check if this sample point can see the sun.
        let t_light = ray_sphere(sample_pos, atmo.sun_direction, atmo.planet_center, atmo.atmo_radius);
        let light_step = t_light.y / f32(LIGHT_STEPS);

        var light_depth_r = 0.0;
        var light_depth_m = 0.0;
        for (var j = 0; j < LIGHT_STEPS; j++) {
            let t_l = (f32(j) + 0.5) * light_step;
            let light_pos = sample_pos + atmo.sun_direction * t_l;
            let light_h = (length(light_pos - atmo.planet_center) - atmo.planet_radius) * scale;
            light_depth_r += exp(-light_h / H_R) * light_step * scale;
            light_depth_m += exp(-light_h / H_M) * light_step * scale;
        }

        // Combined extinction along the full path (camera → sample → sun).
        let tau = BETA_R * (optical_depth_r + light_depth_r)
                + BETA_M * (optical_depth_m + light_depth_m);
        let attenuation = exp(-tau);

        sum_r += density_r * attenuation;
        sum_m += density_m * attenuation;
    }

    // Phase functions.
    let cos_theta = dot(ray_dir, atmo.sun_direction);
    let pr = phase_rayleigh(cos_theta);
    let pm = phase_mie(cos_theta, G_MIE);

    // Sun intensity (in HDR space, will be tonemapped later).
    let sun_intensity = vec3(20.0);

    // Final in-scattered light.
    let atmosphere_color = sun_intensity * (sum_r * BETA_R * pr + sum_m * BETA_M * pm);

    // Transmittance along the full view path through atmosphere.
    let transmittance = exp(-(BETA_R * optical_depth_r + BETA_M * optical_depth_m));

    // Blend: attenuate scene through atmosphere, add in-scattered light.
    let final_color = scene_color.rgb * transmittance + atmosphere_color;
    return vec4(final_color, scene_color.a);
}
