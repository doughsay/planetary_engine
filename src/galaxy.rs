use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

const SHADER_PATH: &str = "shaders/galaxy.wgsl";

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct GalaxyPlugin;

impl Plugin for GalaxyPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<GalaxyMaterial>::default())
            .add_systems(Startup, spawn_galaxy);
    }
}

// ---------------------------------------------------------------------------
// Material
// ---------------------------------------------------------------------------

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct GalaxyMaterial {}

impl Material for GalaxyMaterial {
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
        // Position-only vertex layout — normals and UVs from the sphere mesh
        // are ignored since we compute everything from the direction vector.
        let vertex_layout = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
        ])?;
        descriptor.vertex.buffers = vec![vertex_layout];

        // Render behind opaque geometry.
        if let Some(depth_stencil) = &mut descriptor.depth_stencil {
            depth_stencil.depth_write_enabled = false;
        }

        // Camera is inside the sphere — render back faces.
        descriptor.primitive.cull_mode = None;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Startup system
// ---------------------------------------------------------------------------

fn spawn_galaxy(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GalaxyMaterial>>,
) {
    // Icosphere with 3 subdivisions = 642 vertices, 1280 triangles.
    // Enough tessellation that direction interpolation across triangles
    // is accurate (the fragment shader re-normalizes anyway).
    let mesh = Sphere::new(1.0).mesh().ico(3).unwrap();

    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(GalaxyMaterial {})),
        Transform::default(),
        NoFrustumCulling,
    ));
}
