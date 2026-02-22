mod camera;
mod chunk_mesh;
mod lod;
mod mesh_task;
mod quadtree;
mod galaxy;
mod starfield;
mod terrain;

use big_space::prelude::*;
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
const PLANET_CENTER: Vec3 = Vec3::ZERO;

/// Sun position for lighting.
const SUN_POSITION: Vec3 = Vec3::new(10000.0, 10000.0, 10000.0);

/// Terrain noise parameters.
const NOISE_SCALE: f32 = 4.0;
const TERRAIN_AMPLITUDE: f32 = 150.0;

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        resolution: UVec2::new(1920, 1080).into(),
                        title: "Planetary Engine".into(),
                        ..default()
                    }),
                    ..default()
                })
                .build()
                .disable::<TransformPlugin>(),
        )
        .add_plugins(WireframePlugin::default())
        .add_plugins(BigSpaceDefaultPlugins)
        .add_plugins(SpaceCameraPlugin)
        .add_plugins(LodPlugin)
        .add_plugins(starfield::StarfieldPlugin)
        .add_plugins(galaxy::GalaxyPlugin)
        .add_systems(Startup, setup_full)
        .add_systems(Update, toggle_wireframe)
        .run();
}

fn setup_full(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut starfield_materials: ResMut<Assets<starfield::StarfieldMaterial>>,
    mut galaxy_materials: ResMut<Assets<galaxy::GalaxyMaterial>>,
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
    commands.insert_resource(ClearColor(Color::BLACK));

    // spawn_big_space_default is the recommended way.
    commands.spawn_big_space_default(|root_grid| {
        // Background
        starfield::spawn_starfield(root_grid, &mut meshes, &mut starfield_materials);
        galaxy::spawn_galaxy(root_grid, &mut meshes, &mut galaxy_materials);

        // Sun Visual
        root_grid.spawn_spatial((
            Mesh3d(meshes.add(Sphere::new(500.0).mesh().ico(5).unwrap())),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::WHITE,
                emissive: LinearRgba::WHITE * 100.0,
                ..default()
            })),
            Transform::from_translation(SUN_POSITION),
        ));

        // Light
        root_grid.spawn_spatial((
            DirectionalLight {
                illuminance: 120_000.0,
                shadows_enabled: true,
                ..default()
            },
            Transform::from_xyz(SUN_POSITION.x, SUN_POSITION.y, SUN_POSITION.z)
                .looking_at(PLANET_CENTER, Vec3::Y),
        ));

        // SINGLE Planet root entity
        root_grid.spawn_grid_default((
            Transform::from_translation(PLANET_CENTER),
            Visibility::default(),
            Planet,
        ));

        // Camera with free-fly controls
        root_grid.spawn_spatial((
            Camera3d::default(),
            bevy::core_pipeline::prepass::DepthPrepass,
            Transform::from_xyz(0.0, 0.0, 20_000.0).looking_at(PLANET_CENTER, Vec3::Y),
            Projection::Perspective(PerspectiveProjection {
                far: 1_000_000.0,
                near: 1.0,
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
            FloatingOrigin,
        ));
    });
}

fn toggle_wireframe(input: Res<ButtonInput<KeyCode>>, mut config: ResMut<WireframeConfig>) {
    if input.just_pressed(KeyCode::F1) {
        config.global = !config.global;
    }
}
