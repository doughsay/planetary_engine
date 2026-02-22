#import bevy_pbr::mesh_view_bindings::view

struct Vertex {
    @location(0) position: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_dir: vec3<f32>,
}

const FAR_RADIUS: f32 = 50000.0;

// ── Galactic coordinate frame ───────────────────────────────────────────────
// Fictional galaxy orientation. GALACTIC_NORTH is the disc pole;
// GALACTIC_CENTER is the direction toward the central bulge.
// These are orthonormal: dot(NORTH, CENTER) = 0, |NORTH| = |CENTER| = 1.
// EAST = cross(NORTH, CENTER) completes the right-handed frame.
//
// Derived from normalize(1, 4, -2) and normalize(-4, 1, 0) respectively,
// placing the band diagonally across the sky.
const GALACTIC_NORTH:  vec3<f32> = vec3( 0.2182, 0.8729, -0.4364);
const GALACTIC_CENTER: vec3<f32> = vec3(-0.9701, 0.2425,  0.0);

// ── Band shape ──────────────────────────────────────────────────────────────
// Uses sin²(lat) = gz² for a smooth Gaussian that fades gently to the poles.
// (The earlier tan²(lat) formulation diverges near ±45°, causing a hard edge.)
const BAND_WIDTH: f32 = 6.0;         // Gaussian sharpness (higher = narrower)
const BAND_BRIGHTNESS: f32 = 0.04;   // Peak brightness of the diffuse band

// ── Central bulge ───────────────────────────────────────────────────────────
const BULGE_BRIGHTNESS: f32 = 0.03;  // Extra brightness at galactic center
const BULGE_LAT_WIDTH: f32 = 3.5;    // Latitude falloff (softer than band)
const BULGE_LON_WIDTH: f32 = 8.0;    // Longitude falloff

// ── Noise: spiral arm structure ─────────────────────────────────────────────
const SPIRAL_SCALE: f32 = 3.0;       // FBM frequency
const SPIRAL_STRENGTH: f32 = 0.5;    // Modulation depth (0 = none, 1 = full)

// ── Dust lanes ──────────────────────────────────────────────────────────────
// Model: the midplane is filled with a continuous blanket of dust. FBM noise
// ERODES the edges, carving fractal boundaries. The center stays solid;
// moving away from the plane, noise eats through, creating irregular edges.
const DUST_SCALE: f32 = 5.5;         // Erosion noise frequency
const DUST_STRENGTH: f32 = 0.70;     // Max darkness where dust is solid (0 = none, 1 = black)
const DUST_WIDTH_NEAR: f32 = 14.0;   // Width near the bulge (lower = wider dust band)
const DUST_WIDTH_FAR: f32 = 80.0;    // Width at galactic edge (higher = thinner)
const DUST_FILL: f32 = 1.8;          // Coverage before erosion (>1 = solid center guaranteed)
const DUST_EROSION: f32 = 1.8;       // How aggressively noise eats into the edges
const DUST_TAPER: f32 = 0.8;         // How quickly dust narrows/fades away from the bulge

// ── Star cloud clumping ─────────────────────────────────────────────────────
// Medium-frequency brightness variation — denser and sparser star clouds
// within the band, breaking up the smooth Gaussian glow.
const CLOUD_SCALE: f32 = 7.0;        // Noise frequency
const CLOUD_STRENGTH: f32 = 0.4;     // Brightness variation (0 = uniform, 1 = full)

// ── Bulge structure ─────────────────────────────────────────────────────────
// Low-frequency noise to make the bulge irregular/lumpy instead of a perfect blob.
const BULGE_NOISE_SCALE: f32 = 2.0;
const BULGE_NOISE_STRENGTH: f32 = 0.35;

// ── Bright knots (star-forming regions) ─────────────────────────────────────
// High-frequency noise thresholded so only peaks produce small bright spots
// concentrated near the midplane.
const KNOT_SCALE: f32 = 25.0;        // High frequency for small spots
const KNOT_THRESHOLD: f32 = 0.62;    // Only noise peaks above this become visible
const KNOT_BRIGHTNESS: f32 = 0.06;   // Additional brightness of knots
const KNOT_COLOR: vec3<f32> = vec3(1.0, 0.95, 0.85); // Slightly warm

// ── Arm-crossing glow variation ──────────────────────────────────────────────
// Very low-frequency noise creating 2-3 broad bright/dim segments along the
// band, as if looking through different spiral arm crossings.
const ARM_GLOW_SCALE: f32 = 1.2;     // Very large features
const ARM_GLOW_STRENGTH: f32 = 0.45; // Modulation depth

// ── Stellar halo ────────────────────────────────────────────────────────────
// A very faint, very wide secondary Gaussian — the diffuse halo of old stars
// extending well beyond the disc. Gives the band a sense of depth.
const HALO_WIDTH: f32 = 1.5;         // Much wider than BAND_WIDTH (lower = wider)
const HALO_BRIGHTNESS: f32 = 0.006;  // Very faint relative to band

// ── Fine detail ─────────────────────────────────────────────────────────────
const DETAIL_SCALE: f32 = 12.0;      // Granularity frequency
const DETAIL_STRENGTH: f32 = 0.15;   // Subtle variation amplitude

// ── Color ───────────────────────────────────────────────────────────────────
const COLOR_COOL: vec3<f32> = vec3(0.75, 0.82, 1.0);   // Silver-blue in disc
const COLOR_WARM: vec3<f32> = vec3(1.0, 0.88, 0.65);   // Warm near bulge
const COLOR_STRENGTH: f32 = 0.5;     // Mix strength (0 = all cool, 1 = full warm at bulge)


// ═══════════════════════════════════════════════════════════════════════════
// Noise functions — all 3D to avoid seams at coordinate wrapping boundaries.
// ═══════════════════════════════════════════════════════════════════════════

// Dave Hoskins hash — fract-based, no sin(), robust across GPUs.
fn hash31(p: vec3<f32>) -> f32 {
    var q = fract(p * vec3(0.1031, 0.1030, 0.0973));
    q = q + vec3(dot(q, q.zyx + vec3(33.33)));
    return fract((q.x + q.y) * q.z);
}

// 3D value noise with quintic Hermite interpolation (C2 smooth).
fn vnoise3(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    // Quintic: 6t⁵ − 15t⁴ + 10t³ — eliminates grid artifacts.
    let u = f * f * f * (f * (f * 6.0 - vec3(15.0)) + vec3(10.0));

    return mix(
        mix(mix(hash31(i + vec3(0.0, 0.0, 0.0)), hash31(i + vec3(1.0, 0.0, 0.0)), u.x),
            mix(hash31(i + vec3(0.0, 1.0, 0.0)), hash31(i + vec3(1.0, 1.0, 0.0)), u.x), u.y),
        mix(mix(hash31(i + vec3(0.0, 0.0, 1.0)), hash31(i + vec3(1.0, 0.0, 1.0)), u.x),
            mix(hash31(i + vec3(0.0, 1.0, 1.0)), hash31(i + vec3(1.0, 1.0, 1.0)), u.x), u.y),
        u.z
    );
}

// Fractal Brownian Motion — layered noise with axis rotation between octaves.
fn fbm3(p: vec3<f32>, octaves: i32) -> f32 {
    var value = 0.0;
    var amplitude = 0.5;
    var pos = p;

    for (var i: i32 = 0; i < octaves; i++) {
        value += amplitude * vnoise3(pos);
        amplitude *= 0.5;
        // Permute axes + offset to decorrelate octaves, then double frequency.
        pos = vec3(pos.z + 13.0, pos.x + 7.0, pos.y + 31.0) * 2.0;
    }
    return value;
}


// ═══════════════════════════════════════════════════════════════════════════
// Vertex shader
// ═══════════════════════════════════════════════════════════════════════════

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;

    // Place the unit sphere at FAR_RADIUS, centred on the camera.
    let world_pos = v.position * FAR_RADIUS + view.world_position;
    out.clip_position = view.clip_from_world * vec4(world_pos, 1.0);
    out.clip_position.z = 1e-6;

    // Pass the direction (interpolated, re-normalised in fragment).
    out.world_dir = v.position;
    return out;
}


// ═══════════════════════════════════════════════════════════════════════════
// Fragment shader
// ═══════════════════════════════════════════════════════════════════════════

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dir = normalize(in.world_dir);

    // ── Transform to galactic coordinate frame ──────────────────────
    let galactic_east = cross(GALACTIC_NORTH, GALACTIC_CENTER);
    let gz = dot(dir, GALACTIC_NORTH);    // sin(galactic latitude)
    let gx = dot(dir, GALACTIC_CENTER);   // toward galactic center
    let gy = dot(dir, galactic_east);     // perpendicular in-plane

    // Squared sine of galactic latitude — smooth, bounded, no hard edges.
    let sin_lat_sq = gz * gz;

    // Longitude angle from galactic center.
    let lon = atan2(gy, gx);

    // ── Galactic position for noise sampling ────────────────────────
    let gal_pos = vec3(gx, gy, gz);

    // ── 1. Band: Gaussian falloff from galactic plane ───────────────
    let band = exp(-BAND_WIDTH * sin_lat_sq);

    // ── 2. Bulge: brighter, softer blob near galactic center ────────
    // Low-frequency noise makes the bulge lumpy/irregular.
    let bulge_noise = fbm3(gal_pos * BULGE_NOISE_SCALE + vec3(97.0, 53.0, 71.0), 3);
    let bulge_mod = 1.0 + BULGE_NOISE_STRENGTH * (bulge_noise - 0.5);
    let bulge = exp(-BULGE_LAT_WIDTH * sin_lat_sq)
              * exp(-BULGE_LON_WIDTH * lon * lon)
              * max(bulge_mod, 0.0);

    // ── 3. Spiral arm structure (large-scale FBM) ───────────────────
    let spiral_noise = fbm3(gal_pos * SPIRAL_SCALE, 4);
    let spiral = 1.0 - SPIRAL_STRENGTH * (1.0 - spiral_noise);

    // ── 4. Dust: continuous blanket with fractal-eroded edges ───────
    // center_t drives both width tapering AND intensity fade:
    //   near bulge (center_t→1): wide dust, high fill (solid, hard to erode)
    //   far side   (center_t→0): narrow dust, low fill (erosion eats through)
    let center_t = clamp(exp(-DUST_TAPER * (1.0 - gx)), 0.0, 1.0);
    let dust_width = mix(DUST_WIDTH_FAR, DUST_WIDTH_NEAR, center_t);
    let dust_fill = DUST_FILL * center_t;
    let dust_base = exp(-dust_width * sin_lat_sq);
    let dust_coords = gal_pos * DUST_SCALE + vec3(7.0, 13.0, 23.0);
    let erosion = fbm3(dust_coords, 5);
    let dust_opacity = clamp(dust_base * dust_fill - erosion * DUST_EROSION, 0.0, 1.0);
    let dust = 1.0 - DUST_STRENGTH * dust_opacity;

    // ── 5. Star cloud clumping ───────────────────────────────────────
    // Medium-scale brightness variation — breaks up the smooth Gaussian glow
    // into denser and sparser star cloud regions.
    let cloud_noise = fbm3(gal_pos * CLOUD_SCALE + vec3(61.0, 37.0, 11.0), 3);
    let clouds = 1.0 + CLOUD_STRENGTH * (cloud_noise - 0.5);

    // ── 6. Fine detail ──────────────────────────────────────────────
    let detail_noise = fbm3(gal_pos * DETAIL_SCALE + vec3(43.0, 17.0, 31.0), 2);
    let detail = 1.0 + DETAIL_STRENGTH * (detail_noise - 0.5);

    // ── 7. Bright knots (star-forming regions) ──────────────────────
    // Only the highest noise peaks produce bright spots, concentrated
    // near the midplane.
    let knot_noise = fbm3(gal_pos * KNOT_SCALE + vec3(19.0, 83.0, 47.0), 2);
    let knot = max(knot_noise - KNOT_THRESHOLD, 0.0) / (1.0 - KNOT_THRESHOLD);
    let knot_intensity = knot * knot * KNOT_BRIGHTNESS * band;

    // ── 8. Arm-crossing glow variation ────────────────────────────
    // Very low-frequency modulation — broad bright and dim segments
    // along the band, as if viewing through different spiral arms.
    let arm_noise = fbm3(gal_pos * ARM_GLOW_SCALE + vec3(103.0, 59.0, 79.0), 3);
    let arm_glow = 1.0 + ARM_GLOW_STRENGTH * (arm_noise - 0.5);

    // ── 9. Stellar halo ───────────────────────────────────────────
    // Very wide, very faint glow extending beyond the disc — old
    // halo stars surrounding the galaxy.
    let halo = exp(-HALO_WIDTH * sin_lat_sq);

    // ── Combine layers ──────────────────────────────────────────────
    let base_brightness = (BAND_BRIGHTNESS * band + BULGE_BRIGHTNESS * bulge)
                        * spiral * dust * detail * clouds * arm_glow
                        + HALO_BRIGHTNESS * halo;

    // ── Color: warm near bulge, cool in disc ────────────────────────
    let bulge_factor = clamp(bulge / max(band, 0.001), 0.0, 1.0);
    let base_color = mix(COLOR_COOL, COLOR_WARM, bulge_factor * COLOR_STRENGTH);

    // Knots add warm-tinted brightness on top of the base glow.
    let final_color = base_color * max(base_brightness, 0.0) + KNOT_COLOR * knot_intensity;

    return vec4(final_color, 0.0);
}
