// ═══════════════════════════════════════════════════════════════════════════
// 3D Simplex Noise — WGSL port of Ashima webgl-noise (simplex3d)
// https://github.com/ashima/webgl-noise
// ═══════════════════════════════════════════════════════════════════════════

#define_import_path noise

fn mod289_3(x: vec3<f32>) -> vec3<f32> {
    return x - floor(x * (1.0 / 289.0)) * 289.0;
}

fn mod289_4(x: vec4<f32>) -> vec4<f32> {
    return x - floor(x * (1.0 / 289.0)) * 289.0;
}

fn permute4(x: vec4<f32>) -> vec4<f32> {
    return mod289_4(((x * 34.0) + vec4(10.0)) * x);
}

fn taylor_inv_sqrt4(r: vec4<f32>) -> vec4<f32> {
    return vec4(1.79284291400159) - 0.85373472095314 * r;
}

/// Single-octave 3D simplex noise. Returns values in approximately [-1, 1].
fn simplex3d(v: vec3<f32>) -> f32 {
    let C = vec2(1.0 / 6.0, 1.0 / 3.0);
    let D = vec4(0.0, 0.5, 1.0, 2.0);

    // First corner
    var i = floor(v + dot(v, vec3(C.y)));
    let x0 = v - i + dot(i, vec3(C.x));

    // Other corners
    let g = step(x0.yzx, x0.xyz);
    let l = 1.0 - g;
    let i1 = min(g.xyz, l.zxy);
    let i2 = max(g.xyz, l.zxy);

    let x1 = x0 - i1 + vec3(C.x);
    let x2 = x0 - i2 + vec3(C.y);   // 2.0 * C.x = 1/3
    let x3 = x0 - vec3(D.y);         // -1.0 + 3.0 * C.x = -0.5

    // Permutations
    i = mod289_3(i);
    let p = permute4(permute4(permute4(
        i.z + vec4(0.0, i1.z, i2.z, 1.0))
      + i.y + vec4(0.0, i1.y, i2.y, 1.0))
      + i.x + vec4(0.0, i1.x, i2.x, 1.0));

    // Gradients: 7x7 points over a square, mapped onto an octahedron.
    let n_ = 0.142857142857; // 1.0 / 7.0
    let ns = n_ * D.wyz - D.xzx;

    let j = p - 49.0 * floor(p * ns.z * ns.z); // mod(p, 7*7)

    let x_ = floor(j * ns.z);
    let y_ = floor(j - 7.0 * x_); // mod(j, N)

    let x = x_ * ns.x + vec4(ns.y);
    let y = y_ * ns.x + vec4(ns.y);
    let h = 1.0 - abs(x) - abs(y);

    let b0 = vec4(x.xy, y.xy);
    let b1 = vec4(x.zw, y.zw);

    let s0 = floor(b0) * 2.0 + vec4(1.0);
    let s1 = floor(b1) * 2.0 + vec4(1.0);
    let sh = -step(h, vec4(0.0));

    let a0 = b0.xzyw + s0.xzyw * sh.xxyy;
    let a1 = b1.xzyw + s1.xzyw * sh.zzww;

    var p0 = vec3(a0.xy, h.x);
    var p1 = vec3(a0.zw, h.y);
    var p2 = vec3(a1.xy, h.z);
    var p3 = vec3(a1.zw, h.w);

    // Normalise gradients
    let norm = taylor_inv_sqrt4(vec4(dot(p0, p0), dot(p1, p1), dot(p2, p2), dot(p3, p3)));
    p0 = p0 * norm.x;
    p1 = p1 * norm.y;
    p2 = p2 * norm.z;
    p3 = p3 * norm.w;

    // Mix final noise value
    var m = max(vec4(0.5) - vec4(dot(x0, x0), dot(x1, x1), dot(x2, x2), dot(x3, x3)), vec4(0.0));
    m = m * m;
    return 105.0 * dot(m * m, vec4(dot(p0, x0), dot(p1, x1), dot(p2, x2), dot(p3, x3)));
}

/// Hash a vec3 cell coordinate to 3 pseudorandom values in [0, 1].
/// Used for Voronoi cell jittering (crater placement, etc.).
fn hash33(p: vec3<f32>) -> vec3<f32> {
    var q = fract(p * vec3<f32>(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yxz + 33.33);
    return fract((q.xxy + q.yzz) * q.zyx);
}

/// Fractal Brownian Motion with distance-based octave culling.
///
/// - `p`: sample position (in noise-space coordinates)
/// - `max_octaves`: maximum number of octaves to evaluate
/// - `lacunarity`: frequency multiplier per octave (typically 2.0)
/// - `persistence`: amplitude multiplier per octave (typically 0.5)
/// - `min_feature_size`: minimum feature size to render (in the same coordinate
///   space as `p`). Octaves with smaller features smoothly fade out.
///   Pass 0.0 to evaluate all octaves.
///
/// Normalizes by the full amplitude sum for max_octaves so adding octaves adds
/// detail without rescaling the overall height range.
fn fbm(
    p: vec3<f32>,
    max_octaves: u32,
    lacunarity: f32,
    persistence: f32,
    min_feature_size: f32,
) -> f32 {
    // Pre-compute the full amplitude sum for normalization using geometric series formula.
    // sum = (1 - r^n) / (1 - r)
    let full_amp_sum = (1.0 - pow(persistence, f32(max_octaves))) / (1.0 - persistence);

    var value = 0.0;
    var amplitude = 1.0;
    var frequency = 1.0;
    var pos = p;

    for (var i = 0u; i < max_octaves; i++) {
        let feature_size = 1.0 / frequency;

        if feature_size < min_feature_size * 0.5 {
            // Well below pixel threshold — skip remaining octaves.
            break;
        }

        // Smooth fade: full contribution when feature_size >= 2*min_feature_size,
        // fading to zero when feature_size <= 0.5*min_feature_size.
        let blend = smoothstep(0.5 * min_feature_size, 2.0 * min_feature_size, feature_size);
        value += amplitude * blend * simplex3d(pos);

        amplitude *= persistence;
        frequency *= lacunarity;
        pos = pos * lacunarity;
    }

    return value / full_amp_sum;
}
