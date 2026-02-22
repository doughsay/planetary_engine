use big_space::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, MeshVertexAttribute, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, RenderPipelineDescriptor, SpecializedMeshPipelineError, VertexFormat,
};
use bevy::shader::ShaderRef;

/// Faintest apparent magnitude to generate. Star count is derived automatically
/// from the real catalog formula N(<m) ≈ 10^(0.6*m).
/// mag 8 → ~63k, mag 9 → ~250k, mag 10 → ~1M (gets heavy — use with care).
const MAX_MAGNITUDE: f32 = 9.0;
const SEED: u32 = 0xDEAD_CAFE;

/// Brightness multiplier vs physically-accurate magnitudes.
/// 1.0 = realistic (too dim for a monitor), 8.0 = good for HDR tonemapping.
const BRIGHTNESS_BOOST: f32 = 1.0;

/// Skews the magnitude distribution toward brighter stars.
/// 1.0 = realistic power-law, <1.0 = more bright stars (try 0.5–0.8).
const BRIGHT_SKEW: f32 = 0.8;

/// Skews spectral class selection toward hotter (whiter/bluer) stars.
/// 0.0 = realistic weighting, 1.0 = fully uniform across classes.
const COLOR_WARMTH: f32 = 0.8;

/// Fixed core size in pixels (at 1080p). All stars are unresolved point
/// sources — their apparent "size" comes from the Airy PSF glow, which
/// scales with brightness. This just sets the central dot radius.
const CORE_PIXELS: f32 = 1.0;

const SHADER_PATH: &str = "shaders/starfield.wgsl";

const ATTRIBUTE_STAR_COLOR_SIZE: MeshVertexAttribute = MeshVertexAttribute::new(
    "StarColorSize",
    0x5354_4152_0000_0001,
    VertexFormat::Float32x4,
);

const ATTRIBUTE_STAR_CORNER: MeshVertexAttribute =
    MeshVertexAttribute::new("StarCorner", 0x5354_4152_0000_0002, VertexFormat::Float32x2);

/// Spectral class colors (approximate linear RGB) and cumulative weights.
/// Weighted: M(50%), K(20%), G(15%), F(8%), A(5%), OB(2%)
const SPECTRAL_CLASSES: &[(f32, [f32; 3])] = &[
    (0.50, [1.0, 0.6, 0.3]),   // M — red
    (0.70, [1.0, 0.8, 0.5]),   // K — orange
    (0.85, [1.0, 0.95, 0.8]),  // G — yellow (sun-like)
    (0.93, [1.0, 0.97, 0.95]), // F — yellow-white
    (0.98, [0.8, 0.85, 1.0]),  // A — white
    (1.00, [0.6, 0.7, 1.0]),   // O/B — blue-white
];

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct StarfieldPlugin;

impl Plugin for StarfieldPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<StarfieldMaterial>::default())
            .insert_resource(ClearColor(Color::BLACK));
    }
}

// ---------------------------------------------------------------------------
// Material
// ---------------------------------------------------------------------------

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct StarfieldMaterial {}

impl Material for StarfieldMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            ATTRIBUTE_STAR_COLOR_SIZE.at_shader_location(1),
            ATTRIBUTE_STAR_CORNER.at_shader_location(2),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];

        // Disable depth write — stars are behind everything.
        if let Some(depth_stencil) = &mut descriptor.depth_stencil {
            depth_stencil.depth_write_enabled = false;
            depth_stencil.depth_compare = CompareFunction::GreaterEqual;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Startup system
// ---------------------------------------------------------------------------

pub fn spawn_starfield(
    root: &mut GridCommands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StarfieldMaterial>,
) {
    let mesh = build_star_mesh();
    let material = materials.add(StarfieldMaterial {});

    root.spawn_spatial((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
        Transform::default(),
        NoFrustumCulling,
    ));
}

// ---------------------------------------------------------------------------
// Mesh generation
// ---------------------------------------------------------------------------

fn build_star_mesh() -> Mesh {
    // Derive star count from real catalog formula: N(<m) ≈ 10^(0.6*m).
    let star_count = 10_f32.powf(0.6 * MAX_MAGNITUDE) as u32;

    let mut rng = Rng::new(SEED);

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(star_count as usize * 4);
    let mut color_sizes: Vec<[f32; 4]> = Vec::with_capacity(star_count as usize * 4);
    let mut corners: Vec<[f32; 2]> = Vec::with_capacity(star_count as usize * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(star_count as usize * 6);

    let corner_offsets: [[f32; 2]; 4] = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];

    // Magnitude buckets for logging.
    let num_buckets = MAX_MAGNITUDE.ceil() as usize + 1;
    let mut mag_buckets = vec![0u32; num_buckets];

    for i in 0..star_count {
        let dir = random_direction(&mut rng);

        // Magnitude distribution. BRIGHT_SKEW < 1 biases toward brighter stars:
        // powf(1/skew) with skew < 1 → exponent > 1 → pushes u toward 0 → lower magnitudes.
        let u = rng.next_f32().powf(1.0 / BRIGHT_SKEW);
        let mag = inv_cdf_magnitude(u);
        mag_buckets[(mag as usize).min(num_buckets - 1)] += 1;

        // Convert magnitude to linear brightness: each magnitude step is 2.512x.
        // Physical base is 5.0 at mag 0, scaled by BRIGHTNESS_BOOST for visibility.
        let brightness =
            (5.0 * BRIGHTNESS_BOOST * 2.512_f32.powf(-mag)).max(0.02 * BRIGHTNESS_BOOST);

        let color = spectral_color(&mut rng);

        // All stars are point sources — fixed core size. The shader's Airy PSF
        // makes bright stars *appear* larger via their extended 1/r³ glow.
        let pixel_size = CORE_PIXELS;

        let cs = [
            color[0] * brightness,
            color[1] * brightness,
            color[2] * brightness,
            pixel_size,
        ];

        let base = i * 4;
        for j in 0..4 {
            positions.push(dir.to_array());
            color_sizes.push(cs);
            corners.push(corner_offsets[j]);
        }
        // Two triangles: 0-1-2, 0-2-3
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    let bucket_str: String = mag_buckets
        .iter()
        .enumerate()
        .map(|(i, &count)| {
            if i == num_buckets - 1 {
                format!("{}+: {}", i, count)
            } else {
                format!("{}-{}: {}", i, i + 1, count)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    info!(
        "Starfield: {} stars ({}v, {}i), max_mag={}, distribution: [{}]",
        star_count,
        positions.len(),
        indices.len(),
        MAX_MAGNITUDE,
        bucket_str,
    );

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(ATTRIBUTE_STAR_COLOR_SIZE, color_sizes);
    mesh.insert_attribute(ATTRIBUTE_STAR_CORNER, corners);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

// ---------------------------------------------------------------------------
// RNG + star helpers (carried over from previous implementation)
// ---------------------------------------------------------------------------

struct Rng(u32);

impl Rng {
    fn new(seed: u32) -> Self {
        Self(seed)
    }

    fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u32() & 0x00FF_FFFF) as f32 / 16_777_216.0
    }
}

/// Inverse CDF for realistic apparent-magnitude sampling.
/// Real star counts follow N(<m) ~ 10^(0.6*m). Given uniform u in [0,1),
/// this returns a magnitude in [0, MAX_MAGNITUDE] heavily weighted toward the faint end.
fn inv_cdf_magnitude(u: f32) -> f32 {
    // CDF: F(m) = (10^(0.6*m) - 1) / (10^(0.6*MAX_MAGNITUDE) - 1)
    // Inverse: m = log10(u * (10^(0.6*MAX_MAGNITUDE) - 1) + 1) / 0.6
    let max_count = 10_f32.powf(0.6 * MAX_MAGNITUDE) - 1.0;
    let m = (u * max_count + 1.0).log10() / 0.6;
    m.clamp(0.0, MAX_MAGNITUDE)
}

fn spectral_color(rng: &mut Rng) -> [f32; 3] {
    let t = rng.next_f32();
    let n = SPECTRAL_CLASSES.len() as f32;
    for (i, &(cumulative, color)) in SPECTRAL_CLASSES.iter().enumerate() {
        // Lerp between realistic weighting and uniform (equal per class).
        let uniform = (i + 1) as f32 / n;
        let threshold = cumulative + COLOR_WARMTH * (uniform - cumulative);
        if t < threshold {
            return color;
        }
    }
    SPECTRAL_CLASSES.last().unwrap().1
}

fn random_direction(rng: &mut Rng) -> Vec3 {
    loop {
        let x1 = rng.next_f32() * 2.0 - 1.0;
        let x2 = rng.next_f32() * 2.0 - 1.0;
        let s = x1 * x1 + x2 * x2;
        if s < 1.0 {
            let root = (1.0 - s).sqrt() * 2.0;
            return Vec3::new(x1 * root, x2 * root, 1.0 - 2.0 * s);
        }
    }
}
