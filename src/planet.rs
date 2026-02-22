use bevy::prelude::*;

use crate::planet_material::{PlanetMaterial, PlanetSdfUniforms, SdfConfig};
use crate::Sun;

#[derive(Component, Debug)]
pub struct Planet {
    pub sdf: SdfConfig,
    pub material_handle: Handle<PlanetMaterial>,
}

pub struct PlanetPlugin;

impl Plugin for PlanetPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<PlanetMaterial>::default())
            .add_systems(Update, update_planet_materials);
    }
}

/// Each frame, update every planet's material uniforms with current camera
/// position, sun direction, and the planet's world-space center.
fn update_planet_materials(
    camera_q: Query<&GlobalTransform, With<Camera3d>>,
    sun_q: Query<&GlobalTransform, With<Sun>>,
    planet_q: Query<(&Planet, &GlobalTransform)>,
    mut materials: ResMut<Assets<PlanetMaterial>>,
) {
    let Ok(cam_gt) = camera_q.single() else { return };
    let cam_pos = cam_gt.translation();

    // Sun direction: normalize vector from origin toward the sun.
    // Falls back to +Y if no sun entity exists.
    let sun_dir = sun_q
        .single()
        .ok()
        .map(|gt| gt.translation().normalize_or_zero())
        .unwrap_or(Vec3::Y);

    for (planet, planet_gt) in &planet_q {
        let Some(mat) = materials.get_mut(&planet.material_handle) else {
            continue;
        };

        let center = planet_gt.translation();

        // Sun direction relative to the planet: direction from planet toward sun.
        let planet_sun_dir = sun_q
            .single()
            .ok()
            .map(|gt| (gt.translation() - center).normalize_or_zero())
            .unwrap_or(sun_dir);

        mat.uniforms = PlanetSdfUniforms {
            planet_center: center,
            planet_radius: planet.sdf.radius,
            camera_position: cam_pos,
            max_elevation: planet.sdf.max_elevation,
            sun_direction: planet_sun_dir,
            noise_frequency: planet.sdf.noise_frequency,
            noise_amplitude: planet.sdf.noise_amplitude,
            noise_lacunarity: planet.sdf.noise_lacunarity,
            noise_persistence: planet.sdf.noise_persistence,
            noise_octaves: planet.sdf.noise_octaves,
        };
    }
}
