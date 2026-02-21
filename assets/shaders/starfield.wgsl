#import bevy_pbr::mesh_view_bindings::view

struct Vertex {
    @location(0) position: vec3<f32>,
    @location(1) color_size: vec4<f32>,
    @location(2) corner: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) core_fraction: f32,
}

const FAR_RADIUS: f32 = 50000.0;
const REFERENCE_HEIGHT: f32 = 1080.0;

// ── Airy PSF constants ──────────────────────────────────────────────────────
//
// The Airy disk intensity is I(r) = [2·J₁(x)/x]² with x = π·r·D/λ.
// For large x, J₁(x) ~ √(2/πx)·cos(…), so the *envelope* falls as 1/r³.
// Near center (r < ~0.8 Airy radii), a Gaussian is a good fit.
//
// We approximate the full profile with a single smooth function:
//     I(r) = 1 / (1 + k·r²)^(3/2)
// which is Gaussian-like at small r and falls as 1/r³ at large r.

// Shape parameter. k ≈ 2.3 places the half-maximum at ~0.52 Airy radii.
const PSF_K: f32 = 2.3;

// Glow extent for a unit-brightness star, in multiples of core size.
// Visible radius follows r_max ∝ brightness^(1/3) from the 1/r³ law.
const GLOW_EXTENT: f32 = 8.0;

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;

    // Place star on the far sphere centred at the camera.
    let world_pos = v.position * FAR_RADIUS + view.world_position;
    var clip_pos = view.clip_from_world * vec4(world_pos, 1.0);

    // DPI-aware core size (scales with viewport height relative to 1080p).
    let dpi_scale = view.viewport.w / REFERENCE_HEIGHT;
    let core_size = v.color_size.w * dpi_scale;

    // Peak brightness from pre-multiplied colour channels.
    let brightness = max(v.color_size.x, max(v.color_size.y, v.color_size.z));

    // Glow radius: from Airy 1/r³ law, visible extent ∝ brightness^(1/3).
    // smoothstep fades glow for very dim stars (saves fill-rate).
    let glow_size = core_size * GLOW_EXTENT
        * pow(max(brightness, 0.001), 0.333)
        * smoothstep(0.0, 0.2, brightness);
    let total_size = core_size + glow_size;

    // Expand billboard quad in clip space.
    let viewport_size = view.viewport.zw;
    clip_pos = vec4(
        clip_pos.xy + v.corner * total_size * 2.0 / viewport_size * clip_pos.w,
        clip_pos.zw
    );

    out.clip_position = clip_pos;
    out.clip_position.z = 0.0;
    out.color = v.color_size.xyz;
    out.uv = v.corner * 0.5 + 0.5;
    out.core_fraction = core_size / total_size;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Radial distance: 0 at centre, 1 at quad edge midpoint.
    let d = distance(in.uv, vec2(0.5)) * 2.0;

    if d > 1.0 {
        discard;
    }

    // Map pixel distance → Airy PSF radii (r = 1 at the core edge ≈ first
    // Airy zero, where the central disk ends and diffraction rings begin).
    let r = d / max(in.core_fraction, 0.001);

    // Approximate Airy point-spread function:
    //   Small r: ≈ exp(-k·r²)    (Gaussian-like central disk)
    //   Large r: ≈ 1/(k·r²)^3/2  (1/r³ diffraction envelope)
    let psf = 1.0 / pow(1.0 + PSF_K * r * r, 1.5);

    return vec4(in.color * psf, 0.0);
}
