use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

const SHADER_PATH: &str = "shaders/planet_sdf.wgsl";

// ---------------------------------------------------------------------------
// Uniforms — must match the WGSL struct layout exactly.
// vec3<f32> has 16-byte alignment, so we pair each with a trailing f32.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct PlanetSdfUniforms {
    pub planet_center: Vec3,
    pub planet_radius: f32,
    pub camera_position: Vec3,
    pub max_elevation: f32,
    pub sun_direction: Vec3,
    pub noise_frequency: f32,
    pub noise_amplitude: f32,
    pub noise_lacunarity: f32,
    pub noise_persistence: f32,
    pub noise_octaves: u32,
    /// 0 = normal, 1 = octave count, 2 = ray steps, 3 = normals
    pub debug_mode: u32,
    // ── Crater system ──
    pub crater_enabled: u32,
    pub crater_frequency_0: f32,
    pub crater_depth_0: f32,
    pub crater_rim_height_0: f32,
    pub crater_peak_height_0: f32,
    pub crater_density_0: f32,
    pub crater_frequency_1: f32,
    pub crater_depth_1: f32,
    pub crater_rim_height_1: f32,
    pub crater_peak_height_1: f32,
    pub crater_density_1: f32,
    pub crater_frequency_2: f32,
    pub crater_depth_2: f32,
    pub crater_rim_height_2: f32,
    pub crater_peak_height_2: f32,
    pub crater_density_2: f32,
}

// ---------------------------------------------------------------------------
// Rust-side config (not sent to GPU — used to build uniforms each frame)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct SdfConfig {
    pub radius: f32,
    pub max_elevation: f32,
    pub noise_frequency: f32,
    pub noise_amplitude: f32,
    pub noise_lacunarity: f32,
    pub noise_persistence: f32,
    pub noise_octaves: u32,
    // ── Crater system ──
    pub crater_enabled: bool,
    // Tier 0: large basins
    pub crater_frequency_0: f32,
    pub crater_depth_0: f32,
    pub crater_rim_height_0: f32,
    pub crater_peak_height_0: f32,
    pub crater_density_0: f32,
    // Tier 1: medium craters
    pub crater_frequency_1: f32,
    pub crater_depth_1: f32,
    pub crater_rim_height_1: f32,
    pub crater_peak_height_1: f32,
    pub crater_density_1: f32,
    // Tier 2: small pocks
    pub crater_frequency_2: f32,
    pub crater_depth_2: f32,
    pub crater_rim_height_2: f32,
    pub crater_peak_height_2: f32,
    pub crater_density_2: f32,
}

impl Default for SdfConfig {
    fn default() -> Self {
        Self {
            radius: 1000.0,
            max_elevation: 50.0,
            noise_frequency: 4.0,
            noise_amplitude: 50.0,
            noise_lacunarity: 2.0,
            noise_persistence: 0.5,
            noise_octaves: 14,
            crater_enabled: false,
            crater_frequency_0: 0.0,
            crater_depth_0: 0.0,
            crater_rim_height_0: 0.0,
            crater_peak_height_0: 0.0,
            crater_density_0: 0.0,
            crater_frequency_1: 0.0,
            crater_depth_1: 0.0,
            crater_rim_height_1: 0.0,
            crater_peak_height_1: 0.0,
            crater_density_1: 0.0,
            crater_frequency_2: 0.0,
            crater_depth_2: 0.0,
            crater_rim_height_2: 0.0,
            crater_peak_height_2: 0.0,
            crater_density_2: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Material
// ---------------------------------------------------------------------------

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct PlanetMaterial {
    #[uniform(0)]
    pub uniforms: PlanetSdfUniforms,
}

impl Material for PlanetMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER_PATH.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Opaque
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
        // Position-only vertex layout — same pattern as galaxy.rs.
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];

        // Camera can be inside the bounding sphere — render both faces.
        descriptor.primitive.cull_mode = None;

        // Terrain participates in depth (written by frag_depth in the shader).
        if let Some(depth_stencil) = &mut descriptor.depth_stencil {
            depth_stencil.depth_write_enabled = true;
        }

        Ok(())
    }
}
