use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, CompareFunction, RenderPipelineDescriptor, ShaderType,
    SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

#[derive(Clone, Copy, ShaderType, Debug)]
pub struct AtmosphereUniforms {
    pub planet_center: Vec3,
    pub planet_radius: f32,
    pub sun_direction: Vec3,
    pub atmo_radius: f32,
    pub settings: Vec4, // x: scene_units_to_m, yzw: unused
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct AtmosphereMaterial {
    #[uniform(0)]
    pub uniforms: AtmosphereUniforms,
}

impl Default for AtmosphereMaterial {
    fn default() -> Self {
        Self {
            uniforms: AtmosphereUniforms {
                planet_center: Vec3::ZERO,
                planet_radius: 6360.0,
                sun_direction: Vec3::Y,
                atmo_radius: 6460.0,
                settings: Vec4::new(1000.0, 0.0, 0.0, 0.0),
            },
        }
    }
}

impl Material for AtmosphereMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/atmosphere.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/atmosphere.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
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
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(depth_stencil) = &mut descriptor.depth_stencil {
            depth_stencil.depth_write_enabled = false;
            depth_stencil.depth_compare = CompareFunction::Always;
        }
        Ok(())
    }
}

pub struct AtmospherePlugin;

impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<AtmosphereMaterial>::default());
    }
}
