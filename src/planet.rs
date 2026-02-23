use bevy::prelude::*;
use bevy::shader::Shader;
use bevy::transform::TransformSystems;

use crate::planet_material::{PlanetMaterial, PlanetSdfUniforms, SdfConfig};
use crate::Sun;

#[derive(Component, Debug)]
pub struct Planet {
    pub sdf: SdfConfig,
    pub material_handle: Handle<PlanetMaterial>,
}

/// Debug visualization mode for planet SDF rendering.
/// Cycle with F2. Modes: 0=off, 1=octave count, 2=ray steps, 3=normals.
#[derive(Resource, Default)]
pub struct SdfDebugMode(pub u32);

const NUM_DEBUG_MODES: u32 = 4;
const DEBUG_MODE_NAMES: [&str; 4] = ["Off", "Octave count", "Ray steps", "Normals"];

pub struct PlanetPlugin;

impl Plugin for PlanetPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<PlanetMaterial>::default())
            .init_resource::<SdfDebugMode>()
            .add_systems(PreStartup, load_noise_shader)
            .add_systems(Update, toggle_debug_mode)
            .add_systems(
                PostUpdate,
                update_planet_materials.after(TransformSystems::Propagate),
            );
    }
}

fn toggle_debug_mode(input: Res<ButtonInput<KeyCode>>, mut mode: ResMut<SdfDebugMode>) {
    if input.just_pressed(KeyCode::F2) {
        mode.0 = (mode.0 + 1) % NUM_DEBUG_MODES;
        info!("SDF debug: {}", DEBUG_MODE_NAMES[mode.0 as usize]);
    }
}

/// Explicitly load the noise shader library so that `#import noise::{...}`
/// in planet_sdf.wgsl resolves. Bevy does not auto-discover shader files
/// with `#define_import_path` — they must be loaded via the asset server.
fn load_noise_shader(asset_server: Res<AssetServer>) {
    let handle: Handle<Shader> = asset_server.load("shaders/noise.wgsl");
    std::mem::forget(handle);
}

/// Each frame, update every planet's material uniforms with current camera
/// position, sun direction, and the planet's world-space center.
fn update_planet_materials(
    camera_q: Query<&GlobalTransform, With<Camera3d>>,
    sun_q: Query<&GlobalTransform, With<Sun>>,
    planet_q: Query<(&Planet, &GlobalTransform)>,
    mut materials: ResMut<Assets<PlanetMaterial>>,
    debug_mode: Res<SdfDebugMode>,
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
            debug_mode: debug_mode.0,
            crater_enabled: u32::from(planet.sdf.crater_enabled),
            crater_frequency_0: planet.sdf.crater_frequency_0,
            crater_depth_0: planet.sdf.crater_depth_0,
            crater_rim_height_0: planet.sdf.crater_rim_height_0,
            crater_peak_height_0: planet.sdf.crater_peak_height_0,
            crater_density_0: planet.sdf.crater_density_0,
            crater_frequency_1: planet.sdf.crater_frequency_1,
            crater_depth_1: planet.sdf.crater_depth_1,
            crater_rim_height_1: planet.sdf.crater_rim_height_1,
            crater_peak_height_1: planet.sdf.crater_peak_height_1,
            crater_density_1: planet.sdf.crater_density_1,
            crater_frequency_2: planet.sdf.crater_frequency_2,
            crater_depth_2: planet.sdf.crater_depth_2,
            crater_rim_height_2: planet.sdf.crater_rim_height_2,
            crater_peak_height_2: planet.sdf.crater_peak_height_2,
            crater_density_2: planet.sdf.crater_density_2,
        };
    }
}
