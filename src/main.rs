mod camera;
mod chunk_mesh;
mod lod;
mod mesh_task;
mod quadtree;
mod galaxy;
mod starfield;
mod terrain;
mod planet;
mod orbit;

use big_space::prelude::*;
use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::wireframe::{WireframeConfig, WireframePlugin};
use bevy::prelude::*;
use bevy::render::view::Hdr;
use camera::{SpaceCamera, SpaceCameraPlugin, SpaceCameraState};
use lod::{LodPlugin, PlanetQuadtree};
use terrain::TerrainConfig;
use planet::PlanetPlugin;
use orbit::{Orbit, OrbitPlugin, OrbitalTime};

#[derive(Component)]
struct Sun;

#[derive(Component)]
struct Moon;

/// MICRO SCALE constants for easy verification.
const SUN_RADIUS: f32 = 2000.0;
const EARTH_RADIUS: f32 = 1000.0;
const EARTH_ORBIT_RADIUS: f64 = 15000.0; 
const EARTH_PERIOD: f64 = 30.0; // 30 second year

const MOON_RADIUS: f32 = 300.0;
const MOON_ORBIT_RADIUS: f64 = 4000.0; 
const MOON_PERIOD: f64 = 10.0; // 10 second month

const SUN_POSITION: Vec3 = Vec3::ZERO;

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
        .add_plugins(PlanetPlugin)
        .add_plugins(OrbitPlugin)
        .add_plugins(LodPlugin)
        .add_plugins(starfield::StarfieldPlugin)
        .add_plugins(galaxy::GalaxyPlugin)
        .add_systems(Startup, setup_scene)
        .add_systems(Update, (toggle_wireframe, camera_tracking_hotkeys))
        .run();
}

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut starfield_materials: ResMut<Assets<starfield::StarfieldMaterial>>,
    mut galaxy_materials: ResMut<Assets<galaxy::GalaxyMaterial>>,
    mut orbital_time: ResMut<OrbitalTime>,
) {
    orbital_time.speed = 1.0;
    commands.insert_resource(ClearColor(Color::BLACK));

    let root_id = commands.spawn(BigSpaceRootBundle::default()).id();

    {
        let mut grid_cmds = commands.grid(root_id, Grid::default());
        starfield::spawn_starfield(&mut grid_cmds, &mut meshes, &mut starfield_materials);
        galaxy::spawn_galaxy(&mut grid_cmds, &mut meshes, &mut galaxy_materials);

        grid_cmds.spawn_spatial((
            Sun,
            Mesh3d(meshes.add(Sphere::new(SUN_RADIUS).mesh().ico(5).unwrap())),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::WHITE,
                emissive: LinearRgba::WHITE * 100.0,
                ..default()
            })),
            Transform::from_translation(SUN_POSITION),
        ));

        grid_cmds.spawn_spatial((
            DirectionalLight {
                illuminance: 120_000.0,
                shadows_enabled: true,
                ..default()
            },
            Transform::from_xyz(5000.0, 5000.0, 5000.0)
                .looking_at(Vec3::ZERO, Vec3::Y),
        ));
    }

    let earth_terrain = TerrainConfig::new(EARTH_RADIUS, 4.0, 50.0, 0);
    let earth_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.3, 0.4, 0.6),
        perceptual_roughness: 0.8,
        ..default()
    });

    let moon_terrain = TerrainConfig::new(MOON_RADIUS, 8.0, 20.0, 123);
    let moon_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.6, 0.6),
        perceptual_roughness: 0.9,
        ..default()
    });

    let earth_id;

    {
        let mut grid_cmds = commands.grid(root_id, Grid::default());
        
        earth_id = grid_cmds.spawn_grid_default((
            Transform::default(),
            Visibility::default(),
            planet::Planet { config: earth_terrain.clone() },
            PlanetQuadtree::new(earth_terrain, earth_material),
            Orbit {
                semi_major_axis: EARTH_ORBIT_RADIUS,
                eccentricity: 0.0,
                inclination: 0.0,
                longitude_of_ascending_node: 0.0,
                argument_of_periapsis: 0.0,
                period: EARTH_PERIOD,
                initial_mean_anomaly: 0.0,
                parent: None,
            },
        )).id();

        grid_cmds.spawn_grid_default((
            Moon,
            Transform::default(),
            Visibility::default(),
            planet::Planet { config: moon_terrain.clone() },
            PlanetQuadtree::new(moon_terrain, moon_material),
            Orbit {
                semi_major_axis: MOON_ORBIT_RADIUS,
                eccentricity: 0.0,
                inclination: 0.0,
                longitude_of_ascending_node: 0.0,
                argument_of_periapsis: 0.0,
                period: MOON_PERIOD,
                initial_mean_anomaly: 0.0,
                parent: Some(earth_id),
            },
        ));
    }

    commands.entity(earth_id).with_children(|parent| {
        parent.spawn((
            CellCoord::default(),
            Camera3d::default(),
            bevy::core_pipeline::prepass::DepthPrepass,
            // Position camera slightly further back to see the Moon pass by
            Transform::from_xyz(0.0, 0.0, EARTH_RADIUS + 8000.0).looking_at(Vec3::ZERO, Vec3::Y),
            Projection::Perspective(PerspectiveProjection {
                far: 1_000_000.0,
                near: 1.0,
                ..default()
            }),
            Hdr,
            Exposure::SUNLIGHT,
            Tonemapping::AcesFitted,
            SpaceCamera {
                speed: 50.0,
                boost_multiplier: 10.0,
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

fn camera_tracking_hotkeys(
    input: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut camera_q: Query<(&mut Transform, &GlobalTransform, &mut SpaceCameraState), With<Camera3d>>,
    targets_q: Query<(&GlobalTransform, Has<Sun>, Has<Moon>, Has<planet::Planet>), Without<Camera3d>>,
) {
    let mut target_pos: Option<Vec3> = None;

    if input.pressed(KeyCode::Digit1) {
        for (global, is_sun, _, _) in &targets_q {
            if is_sun { target_pos = Some(global.translation()); break; }
        }
    } else if input.pressed(KeyCode::Digit2) {
        for (global, _, is_moon, is_planet) in &targets_q {
            if is_planet && !is_moon { target_pos = Some(global.translation()); break; }
        }
    } else if input.pressed(KeyCode::Digit3) {
        for (global, _, is_moon, _) in &targets_q {
            if is_moon { target_pos = Some(global.translation()); break; }
        }
    }

    if let Some(target_pos) = target_pos {
        let Ok((mut cam_transform, cam_global, mut cam_state)) = camera_q.single_mut() else { return };

        // Stop camera movement to prevent fighting with the camera input system
        cam_state.velocity = Vec3::ZERO;

        let cam_world_pos = cam_global.translation();
        let to_target = target_pos - cam_world_pos;
        if to_target.length_squared() < 1.0 { return; }
        let dir = to_target.normalize();

        // Compute desired rotation in parent-local space.
        // The parent (Earth) has no rotation, so world-space directions
        // are valid in parent-local space.
        let desired_rot = Transform::from_translation(cam_transform.translation)
            .looking_at(cam_transform.translation + dir, Vec3::Y)
            .rotation;

        // Smoothly interpolate toward the target orientation
        let t = (8.0 * time.delta_secs()).min(1.0);
        cam_transform.rotation = cam_transform.rotation.slerp(desired_rot, t);
    }
}

fn toggle_wireframe(input: Res<ButtonInput<KeyCode>>, mut config: ResMut<WireframeConfig>) {
    if input.just_pressed(KeyCode::F1) {
        config.global = !config.global;
    }
}
