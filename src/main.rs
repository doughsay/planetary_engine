mod camera;
mod chunk_mesh;
mod lod;
mod mesh_task;
mod quadtree;
mod galaxy;
mod starfield;
mod terrain;

use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::wireframe::{WireframeConfig, WireframePlugin};
use bevy::pbr::{Atmosphere, AtmosphereSettings, ScatteringMedium};
use bevy::prelude::*;
use camera::{SpaceCamera, SpaceCameraPlugin, SpaceCameraState};
use lod::{LodPlugin, Planet, PlanetQuadtree};
use terrain::TerrainConfig;

/// Planet radius in km (1 world unit = 1 km).
const PLANET_RADIUS: f32 = 6360.0;

/// The atmosphere shader assumes planet center is at (0, -bottom_radius/s2m, 0).
const PLANET_CENTER: Vec3 = Vec3::new(0.0, -PLANET_RADIUS, 0.0);

/// Terrain noise parameters.
const NOISE_SCALE: f32 = 4.0;
const TERRAIN_AMPLITUDE: f32 = 50.0;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(WireframePlugin::default())
        .add_plugins(SpaceCameraPlugin)
        .add_plugins(LodPlugin)
        .add_plugins(starfield::StarfieldPlugin)
        .add_plugins(galaxy::GalaxyPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, (toggle_wireframe, toggle_atmosphere))
        .run();
}

fn setup(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut scattering_mediums: ResMut<Assets<ScatteringMedium>>,
) {
    let terrain = TerrainConfig::new(PLANET_RADIUS, NOISE_SCALE, TERRAIN_AMPLITUDE, 0);

    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.5, 0.5, 0.5),
        perceptual_roughness: 0.9,
        metallic: 0.0,
        reflectance: 0.3,
        ..default()
    });

    commands.insert_resource(PlanetQuadtree::new(terrain, material.clone(), PLANET_CENTER));

    // Planet root entity at the atmosphere-expected position
    commands.spawn((
        Transform::from_translation(PLANET_CENTER),
        Visibility::default(),
        Planet,
    ));

    // Directional "sun" light
    commands.spawn((
        DirectionalLight {
            illuminance: 120_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(40000.0, 50000.0, 40000.0).looking_at(PLANET_CENTER, Vec3::Y),
    ));

    // Camera with free-fly controls and atmosphere
    let medium = scattering_mediums.add(ScatteringMedium::default());
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, PLANET_CENTER.y, 20_000.0).looking_at(PLANET_CENTER, Vec3::Y),
        Projection::Perspective(PerspectiveProjection {
            far: 100_000.0,
            near: 1.0,
            ..default()
        }),
        Atmosphere::earthlike(medium),
        AtmosphereSettings {
            scene_units_to_m: 1000.0,
            ..default()
        },
        Exposure::SUNLIGHT,
        Tonemapping::AcesFitted,
        // 6DOF space camera: WASD to fly, QE to roll, mouse to look, scroll for speed, shift to boost
        SpaceCamera {
            speed: 10.0,             // 10 km/s — good for surface flying
            boost_multiplier: 50.0,  // 500 km/s — fast traversal space ↔ surface
            sensitivity: 0.15,
            roll_speed: 1.5,
            friction: 5.0,
            scroll_factor: 1.2,
        },
        SpaceCameraState::default(),
    ));
}

fn toggle_wireframe(input: Res<ButtonInput<KeyCode>>, mut config: ResMut<WireframeConfig>) {
    if input.just_pressed(KeyCode::F1) {
        config.global = !config.global;
    }
}

fn toggle_atmosphere(
    input: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
    camera_q: Query<(Entity, Option<&Atmosphere>), With<Camera3d>>,
    mut scattering_mediums: ResMut<Assets<ScatteringMedium>>,
) {
    if !input.just_pressed(KeyCode::F3) { return; }
    let Ok((entity, atmo)) = camera_q.single() else { return };

    if atmo.is_some() {
        commands.entity(entity).remove::<(Atmosphere, AtmosphereSettings)>();
    } else {
        let medium = scattering_mediums.add(ScatteringMedium::default());
        commands.entity(entity).insert((
            Atmosphere::earthlike(medium),
            AtmosphereSettings {
                scene_units_to_m: 1000.0,
                ..default()
            },
        ));
    }
}
