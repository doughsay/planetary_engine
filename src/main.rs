mod camera;
mod galaxy;
mod starfield;
mod planet;
mod planet_material;
mod orbit;

use big_space::prelude::*;
use bevy::camera::Exposure;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::pbr::wireframe::{WireframeConfig, WireframePlugin};
use bevy::prelude::*;
use bevy::render::view::Hdr;
use camera::{SpaceCamera, SpaceCameraPlugin, SpaceCameraState};
use planet::PlanetPlugin;
use planet_material::{PlanetMaterial, PlanetSdfUniforms, SdfConfig};
use orbit::{Orbit, OrbitPlugin, OrbitalTime};

#[derive(Component)]
pub struct Sun;

#[derive(Component)]
pub struct Satellite;

/// MICRO SCALE constants for easy verification.
const SUN_RADIUS: f32 = 2000.0;
const PLANET_RADIUS: f32 = 1000.0;
const PLANET_ORBIT_RADIUS: f64 = 15000.0;
const PLANET_PERIOD: f64 = 30.0; // 30 second year

const SATELLITE_RADIUS: f32 = 300.0;
const SATELLITE_ORBIT_RADIUS: f64 = 4000.0;
const SATELLITE_PERIOD: f64 = 10.0; // 10 second month

const SUN_POSITION: Vec3 = Vec3::ZERO;

#[derive(Resource)]
struct BenchConfig;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bench_mode = args.iter().any(|arg| arg == "--bench");

    let mut app = App::new();
    app.insert_resource(BenchConfig)
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
        .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
        .add_plugins(BigSpaceDefaultPlugins)
        .add_plugins(PlanetPlugin)
        .add_plugins(OrbitPlugin)
        .add_plugins(starfield::StarfieldPlugin)
        .add_plugins(galaxy::GalaxyPlugin)
        .add_systems(Startup, (setup_scene, setup_fps_counter))
        .add_systems(Update, (toggle_wireframe, update_fps_counter));

    if bench_mode {
        app.add_plugins(BenchmarkPlugin);
    } else {
        app.add_plugins(SpaceCameraPlugin)
           .add_systems(Update, camera_tracking_hotkeys);
    }

    app.run();
}

struct BenchmarkPlugin;

#[derive(Resource)]
struct BenchmarkState {
    timer: f32,
    log_timer: f32,
}

impl Plugin for BenchmarkPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(BenchmarkState { timer: 0.0, log_timer: 0.0 })
           .add_systems(Update, (run_benchmark_flyby, log_benchmark_stats).chain());
    }
}

fn run_benchmark_flyby(
    time: Res<Time>,
    mut bench_state: ResMut<BenchmarkState>,
    mut camera_q: Query<(&mut Transform, &mut SpaceCameraState, &mut CellCoord), With<Camera3d>>,
) {
    bench_state.timer += time.delta_secs();
    let t = bench_state.timer;

    // Exit after 10s
    if t >= 10.0 {
        info!("[BENCH] Benchmark complete. Exiting.");
        std::process::exit(0);
    }

    let Ok((mut cam_transform, mut cam_state, mut cam_cell)) = camera_q.single_mut() else { return };
    
    // Reset velocity and cell coord to prevent accumulation/interference from big_space recentering
    cam_state.velocity = Vec3::ZERO;
    *cam_cell = CellCoord::default();

    // Camera is a child of the planet, so (0,0,0) is the planet center in local space.
    let planet_center = Vec3::ZERO;
    let skim_alt = 40.0; // Much closer to the surface (PLANET_RADIUS is 1000)
    let skim_radius = PLANET_RADIUS + skim_alt;

    let target_pos: Vec3;
    let look_at_target: Vec3;
    let up: Vec3;

    if t < 5.0 {
        // Phase 1: Approach (0s - 5s)
        let alpha = t / 5.0;
        let eased_alpha = 1.0 - (1.0 - alpha).powi(3);
        
        let start_pos = Vec3::new(0.0, 0.0, PLANET_RADIUS + 8000.0);
        let end_pos = Vec3::new(0.0, skim_alt, skim_radius);
        let line_pos = start_pos.lerp(end_pos, eased_alpha);
        
        // Pre-calculate the orbital path we'll be blending into
        let orbit_angle = (t - 5.0) * (std::f32::consts::PI / 15.0);
        let orbit_pos = Vec3::new(
            -skim_radius * orbit_angle.sin(), // Negated X for left turn
            skim_alt,
            skim_radius * orbit_angle.cos()
        );
        
        // Smoothly blend from the straight line approach into the circular orbit
        // between t=2.0 and t=5.0 to create a natural curve.
        let blend = ((t - 2.0) / 3.0).clamp(0.0, 1.0);
        let smooth_blend = blend * blend * (3.0 - 2.0 * blend);
        target_pos = line_pos.lerp(orbit_pos, smooth_blend);
        
        // --- Unified Look-At ---
        let surface_up = target_pos.normalize();
        let tangent = Vec3::new(
            -orbit_angle.cos(), // Negated X for left turn
            0.0,
            -orbit_angle.sin()
        ).normalize();
        let horizon_dir = (tangent * 2.0 - surface_up * 0.3).normalize();
        let horizon_target = target_pos + horizon_dir * 1000.0;
        
        // Transition look from planet center to horizon in the final seconds of approach
        let look_alpha = ((t - 3.5) / 1.5).clamp(0.0, 1.0);
        let smooth_look = look_alpha * look_alpha * (3.0 - 2.0 * look_alpha);
        look_at_target = planet_center.lerp(horizon_target, smooth_look);
        up = Vec3::Y.lerp(surface_up, smooth_look);
    } else {
        // Phase 2: Skim horizon (5s - 10s)
        let alpha = (t - 5.0) / 15.0; // Keep the same angular speed
        let angle = alpha * std::f32::consts::PI;
        
        target_pos = Vec3::new(
            -skim_radius * angle.sin(), // Negated X for left turn
            skim_alt,
            skim_radius * angle.cos()
        );
        
        let surface_up = target_pos.normalize();
        up = surface_up;
        
        let tangent = Vec3::new(
            -angle.cos(), // Negated X for left turn
            0.0,
            -angle.sin()
        ).normalize();
        
        let horizon_dir = (tangent * 2.0 - surface_up * 0.3).normalize();
        let horizon_target = target_pos + horizon_dir * 1000.0;
        
        look_at_target = horizon_target; 
    }

    cam_transform.translation = target_pos;
    cam_transform.look_at(look_at_target, up);
}

fn log_benchmark_stats(
    time: Res<Time>,
    mut bench_state: ResMut<BenchmarkState>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
) {
    bench_state.log_timer += time.delta_secs();
    if bench_state.log_timer >= 1.0 {
        bench_state.log_timer -= 1.0;
        
        let fps = diagnostics
            .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
            .and_then(|d| d.smoothed())
            .unwrap_or(0.0);
        let frame_time = diagnostics
            .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FRAME_TIME)
            .and_then(|d| d.smoothed())
            .unwrap_or(0.0);
            
        info!(
            "[BENCH] Time: {:.1}s | FPS: {:.1} | Frame: {:.2}ms", 
            bench_state.timer, fps, frame_time
        );
    }
}

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut planet_materials: ResMut<Assets<PlanetMaterial>>,
    mut starfield_materials: ResMut<Assets<starfield::StarfieldMaterial>>,
    mut galaxy_materials: ResMut<Assets<galaxy::GalaxyMaterial>>,
    mut orbital_time: ResMut<OrbitalTime>,
) {
    orbital_time.speed = 1.0;
    commands.insert_resource(ClearColor(Color::BLACK));

    let root_id = commands.spawn(BigSpaceRootBundle::default()).id();

    // Shared terrain icosphere mesh (unit sphere — shader handles positioning)
    let terrain_mesh = meshes.add(Sphere::new(1.0).mesh().ico(5).unwrap());

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

    // ── Planet ────────────────────────────────────────────────────────────
    let planet_sdf_config = SdfConfig {
        radius: PLANET_RADIUS,
        max_elevation: 50.0,
        noise_frequency: 4.0,
        noise_amplitude: 10.0,  // Reduced from 50 — craters are the main feature
        noise_lacunarity: 2.0,
        noise_persistence: 0.5,
        noise_octaves: 14,
        crater_enabled: true,
        // Tier 0: large basins
        crater_frequency_0: 6.0,
        crater_depth_0: 15.0,
        crater_rim_height_0: 5.0,
        crater_peak_height_0: 3.0,
        crater_density_0: 0.3,
        // Tier 1: medium craters
        crater_frequency_1: 20.0,
        crater_depth_1: 5.0,
        crater_rim_height_1: 2.0,
        crater_peak_height_1: 0.5,
        crater_density_1: 0.5,
        ..Default::default()
    };

    let planet_material_handle = planet_materials.add(PlanetMaterial {
        uniforms: PlanetSdfUniforms::default(),
    });

    // ── Satellite ─────────────────────────────────────────────────────────
    let satellite_sdf_config = SdfConfig {
        radius: SATELLITE_RADIUS,
        max_elevation: 20.0,
        noise_frequency: 8.0,
        noise_amplitude: 20.0,
        noise_lacunarity: 2.0,
        noise_persistence: 0.5,
        noise_octaves: 14,
        ..Default::default()
    };

    let satellite_material_handle = planet_materials.add(PlanetMaterial {
        uniforms: PlanetSdfUniforms::default(),
    });

    let planet_id;
    let satellite_id;

    {
        let mut grid_cmds = commands.grid(root_id, Grid::default());

        planet_id = grid_cmds.spawn_grid_default((
            Transform::default(),
            Visibility::default(),
            planet::Planet {
                sdf: planet_sdf_config,
                material_handle: planet_material_handle.clone(),
            },
            Orbit {
                semi_major_axis: PLANET_ORBIT_RADIUS,
                eccentricity: 0.0,
                inclination: 0.0,
                longitude_of_ascending_node: 0.0,
                argument_of_periapsis: 0.0,
                period: PLANET_PERIOD,
                initial_mean_anomaly: 0.0,
                parent: None,
            },
        )).id();

        satellite_id = grid_cmds.spawn_grid_default((
            Satellite,
            Transform::default(),
            Visibility::default(),
            planet::Planet {
                sdf: satellite_sdf_config,
                material_handle: satellite_material_handle.clone(),
            },
            Orbit {
                semi_major_axis: SATELLITE_ORBIT_RADIUS,
                eccentricity: 0.0,
                inclination: 0.0,
                longitude_of_ascending_node: 0.0,
                argument_of_periapsis: 0.0,
                period: SATELLITE_PERIOD,
                initial_mean_anomaly: 0.0,
                parent: Some(planet_id),
            },
        )).id();
    }

    // Spawn terrain icosphere as child of planet
    commands.entity(planet_id).with_children(|parent| {
        parent.spawn((
            CellCoord::default(),
            Mesh3d(terrain_mesh.clone()),
            MeshMaterial3d(planet_material_handle),
            Transform::default(),
            NoFrustumCulling,
        ));
    });

    // Spawn terrain icosphere as child of satellite
    commands.entity(satellite_id).with_children(|parent| {
        parent.spawn((
            CellCoord::default(),
            Mesh3d(terrain_mesh.clone()),
            MeshMaterial3d(satellite_material_handle),
            Transform::default(),
            NoFrustumCulling,
        ));
    });

    // Camera as child of planet
    commands.entity(planet_id).with_children(|parent| {
        parent.spawn((
            CellCoord::default(),
            Camera3d::default(),
            bevy::core_pipeline::prepass::DepthPrepass,
            // Position camera further back to see the satellite pass by
            Transform::from_xyz(0.0, 0.0, PLANET_RADIUS + 8000.0).looking_at(Vec3::ZERO, Vec3::Y),
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
    targets_q: Query<(&GlobalTransform, Has<Sun>, Has<Satellite>, Has<planet::Planet>), Without<Camera3d>>,
) {
    let mut target_pos: Option<Vec3> = None;

    if input.pressed(KeyCode::Digit1) {
        for (global, is_sun, _, _) in &targets_q {
            if is_sun { target_pos = Some(global.translation()); break; }
        }
    } else if input.pressed(KeyCode::Digit2) {
        for (global, _, is_satellite, is_planet) in &targets_q {
            if is_planet && !is_satellite { target_pos = Some(global.translation()); break; }
        }
    } else if input.pressed(KeyCode::Digit3) {
        for (global, _, is_satellite, _) in &targets_q {
            if is_satellite { target_pos = Some(global.translation()); break; }
        }
    }

    if let Some(target_pos) = target_pos {
        let Ok((mut cam_transform, cam_global, mut cam_state)) = camera_q.single_mut() else { return };

        // Stop camera movement to prevent fighting with the camera input system
        cam_state.velocity = Vec3::ZERO;

        let cam_world_pos: Vec3 = cam_global.translation();
        let to_target = target_pos - cam_world_pos;
        if to_target.length_squared() < 1.0 { return; }
        let dir = to_target.normalize();

        // Compute desired rotation in parent-local space.
        // The parent planet has no rotation, so world-space directions
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

#[derive(Component)]
struct FpsText;

fn setup_fps_counter(mut commands: Commands) {
    commands.spawn((
        FpsText,
        Text::new("-- fps\n-- ms"),
        TextFont::from_font_size(18.0),
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(10.0),
            top: Val::Px(10.0),
            ..default()
        },
    ));
}

fn update_fps_counter(
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    mut query: Query<&mut Text, With<FpsText>>,
) {
    let Ok(mut text) = query.single_mut() else { return };
    let fps = diagnostics
        .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let frame_time = diagnostics
        .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    text.0 = format!("{fps:.1} fps\n{frame_time:.1} ms");
}
