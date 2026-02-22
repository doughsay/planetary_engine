mod atmosphere;
mod camera;
mod chunk_mesh;
mod lod;
mod mesh_task;
mod quadtree;
mod galaxy;
mod starfield;
mod terrain;

use atmosphere::{AtmosphereMaterial, AtmosphereUniforms};
use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::wireframe::{WireframeConfig, WireframePlugin};
use bevy::prelude::*;
use bevy::render::view::Hdr;
use camera::{SpaceCamera, SpaceCameraPlugin, SpaceCameraState};
use lod::{LodPlugin, Planet, PlanetQuadtree};
use terrain::TerrainConfig;

/// Planet radius in km (1 world unit = 1 km).
const PLANET_RADIUS: f32 = 6360.0;

/// Planet center position.
const PLANET_CENTER: Vec3 = Vec3::new(0.0, -PLANET_RADIUS, 0.0);

/// Sun position for lighting.
const SUN_POSITION: Vec3 = Vec3::new(40000.0, 50000.0, 40000.0);

/// Terrain noise parameters.
const NOISE_SCALE: f32 = 4.0;
const TERRAIN_AMPLITUDE: f32 = 150.0;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                resolution: UVec2::new(1920, 1080).into(),
                title: "Planetary Engine".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(WireframePlugin::default())
        .add_plugins(SpaceCameraPlugin)
        .add_plugins(LodPlugin)
        .add_plugins(starfield::StarfieldPlugin)
        .add_plugins(galaxy::GalaxyPlugin)
        .add_plugins(atmosphere::AtmospherePlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, toggle_wireframe)
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut atmo_materials: ResMut<Assets<AtmosphereMaterial>>,
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

    // Directional "sun" light
    let sun_direction = (SUN_POSITION - PLANET_CENTER).normalize();
    commands.spawn((
        DirectionalLight {
            illuminance: 120_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(SUN_POSITION.x, SUN_POSITION.y, SUN_POSITION.z)
            .looking_at(PLANET_CENTER, Vec3::Y),
    ));

    // SINGLE Planet root entity
    commands.spawn((
        Transform::from_translation(PLANET_CENTER),
        Visibility::default(),
        Planet,
    ));

    let atmo_radius = PLANET_RADIUS + 100.0;

    // Top-level Atmosphere entity
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(1.0).mesh().ico(5).unwrap())),
        MeshMaterial3d(atmo_materials.add(AtmosphereMaterial {
            uniforms: AtmosphereUniforms {
                planet_radius: PLANET_RADIUS,
                atmo_radius,
                planet_center: PLANET_CENTER,
                sun_direction,
                settings: Vec4::new(1000.0, 0.0, 0.0, 0.0),
            },
        })),
        Transform::default(),
        Visibility::default(),
        bevy::camera::visibility::NoFrustumCulling,
    ));

    // Camera with free-fly controls and depth prepass
    commands.spawn((
        Camera3d::default(),
        bevy::core_pipeline::prepass::DepthPrepass,
        Transform::from_xyz(0.0, PLANET_CENTER.y, 20_000.0).looking_at(PLANET_CENTER, Vec3::Y),
        Projection::Perspective(PerspectiveProjection {
            far: 100_000.0,
            near: 1.0, // Back to 1.0 for better depth precision
            ..default()
        }),
        Hdr,
        Exposure::SUNLIGHT,
        Tonemapping::AcesFitted,
        SpaceCamera {
            speed: 10.0,
            boost_multiplier: 50.0,
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
