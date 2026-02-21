use bevy::camera::CameraUpdateSystems;
use bevy::core_pipeline::core_3d::graph::Node3d;
use bevy::core_pipeline::fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin};
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_graph::{InternedRenderLabel, RenderLabel};
use bevy::render::render_resource::ShaderType;
use bevy::shader::ShaderRef;

/// Atmosphere post-process effect. Add this component to a Camera3d entity.
///
/// Camera-derived fields (`camera_position`, `camera_forward`, `camera_right`,
/// `camera_up`, `fov_tan_half`, `aspect_ratio`, `sun_direction`) are updated
/// automatically each frame by [`update_atmosphere_uniforms`].
#[derive(Component, ExtractComponent, Clone, Copy, ShaderType, Default, Debug)]
pub struct AtmosphereEffect {
    pub camera_position: Vec3,
    pub planet_radius: f32,
    pub planet_center: Vec3,
    pub atmo_radius: f32,
    pub sun_direction: Vec3,
    pub scene_units_to_m: f32,
    pub camera_forward: Vec3,
    pub fov_tan_half: f32,
    pub camera_right: Vec3,
    pub aspect_ratio: f32,
    pub camera_up: Vec3,
    pub _padding: f32,
}

impl FullscreenMaterial for AtmosphereEffect {
    fn fragment_shader() -> ShaderRef {
        "shaders/atmosphere.wgsl".into()
    }

    fn node_edges() -> Vec<InternedRenderLabel> {
        vec![
            Node3d::StartMainPassPostProcessing.intern(),
            Self::node_label().intern(),
            Node3d::Tonemapping.intern(),
        ]
    }
}

pub struct AtmospherePlugin;

impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FullscreenMaterialPlugin::<AtmosphereEffect>::default())
            .add_systems(
                PostUpdate,
                update_atmosphere_uniforms.after(CameraUpdateSystems),
            );
    }
}

fn update_atmosphere_uniforms(
    mut atmo_q: Query<(&mut AtmosphereEffect, &GlobalTransform, &Projection)>,
    sun_q: Query<&GlobalTransform, With<DirectionalLight>>,
) {
    let Ok((mut atmo, cam_transform, projection)) = atmo_q.single_mut() else {
        return;
    };

    // Extract camera vectors from the world transform matrix.
    // Column 0 = right (+X local), column 1 = up (+Y local),
    // column 2 = back (+Z local, camera looks along -Z).
    let m = cam_transform.to_matrix();
    atmo.camera_position = cam_transform.translation();
    atmo.camera_right = m.x_axis.truncate().normalize();
    atmo.camera_up = m.y_axis.truncate().normalize();
    atmo.camera_forward = (-m.z_axis.truncate()).normalize();

    // FOV and aspect from the perspective projection.
    if let Projection::Perspective(persp) = projection {
        atmo.fov_tan_half = (persp.fov / 2.0).tan();
        atmo.aspect_ratio = persp.aspect_ratio;
    }

    if let Ok(sun_transform) = sun_q.single() {
        atmo.sun_direction = (-*sun_transform.forward()).normalize_or_zero();
    }
}
