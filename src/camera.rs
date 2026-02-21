use bevy::ecs::message::MessageReader;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};

/// 6DOF space camera controller. No fixed "up" — all rotations are in camera-local space.
///
/// Controls:
///   Mouse          - Pitch + yaw (hold left click or press M to capture)
///   WASD           - Fly forward/back/left/right
///   Q/E            - Roll left/right
///   Space/Ctrl     - Thrust up/down (camera-local)
///   Shift          - Boost (10x speed)
///   Scroll wheel   - Adjust base speed
#[derive(Component)]
pub struct SpaceCamera {
    pub speed: f32,
    pub boost_multiplier: f32,
    pub sensitivity: f32,
    pub roll_speed: f32,
    pub friction: f32,
    pub scroll_factor: f32,
}

impl Default for SpaceCamera {
    fn default() -> Self {
        Self {
            speed: 10.0,
            boost_multiplier: 50.0,
            sensitivity: 0.15,
            roll_speed: 1.5,
            friction: 5.0,
            scroll_factor: 1.2,
        }
    }
}

/// Tracks velocity and cursor grab state.
#[derive(Component, Default)]
pub struct SpaceCameraState {
    pub velocity: Vec3,
    pub grabbed: bool,
}

pub struct SpaceCameraPlugin;

impl Plugin for SpaceCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (space_camera_input, space_camera_cursor_toggle));
    }
}

fn space_camera_cursor_toggle(
    input: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut camera_q: Query<&mut SpaceCameraState, With<SpaceCamera>>,
    mut window_q: Query<&mut CursorOptions, With<Window>>,
) {
    let Ok(mut state) = camera_q.single_mut() else { return };
    let Ok(mut cursor) = window_q.single_mut() else { return };

    // M toggles grab
    if input.just_pressed(KeyCode::KeyM) {
        state.grabbed = !state.grabbed;
    }

    // Left mouse button holds grab
    let hold_grab = mouse_button.pressed(MouseButton::Left);

    let should_grab = state.grabbed || hold_grab;
    if should_grab {
        cursor.grab_mode = CursorGrabMode::Locked;
        cursor.visible = false;
    } else {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}

fn space_camera_input(
    time: Res<Time>,
    input: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: MessageReader<MouseMotion>,
    mut scroll_events: MessageReader<MouseWheel>,
    mut query: Query<(&mut Transform, &mut SpaceCamera, &mut SpaceCameraState)>,
) {
    let Ok((mut transform, mut cam, mut state)) = query.single_mut() else { return };
    let dt = time.delta_secs();

    // --- Scroll wheel adjusts base speed ---
    let scroll: f32 = scroll_events.read().map(|e| e.y).sum();
    if scroll != 0.0 {
        cam.speed *= cam.scroll_factor.powf(scroll);
        cam.speed = cam.speed.clamp(0.01, 100_000.0);
    }

    // --- Mouse look (only when grabbed) ---
    let is_grabbed = state.grabbed || mouse_button.pressed(MouseButton::Left);
    if is_grabbed {
        let mouse_delta: Vec2 = mouse_motion.read().map(|e| e.delta).sum();
        if mouse_delta != Vec2::ZERO {
            let yaw = -mouse_delta.x * cam.sensitivity * dt;
            let pitch = -mouse_delta.y * cam.sensitivity * dt;

            // Rotate around camera-local axes (no world-up constraint)
            let right = transform.right();
            let up = transform.up();
            transform.rotate(Quat::from_axis_angle(*right, pitch));
            transform.rotate(Quat::from_axis_angle(*up, yaw));
        }
    } else {
        // Drain events even when not grabbed
        mouse_motion.read().for_each(drop);
    }

    // --- Roll with Q/E ---
    let mut roll = 0.0;
    if input.pressed(KeyCode::KeyQ) { roll += 1.0; }
    if input.pressed(KeyCode::KeyE) { roll -= 1.0; }
    if roll != 0.0 {
        let forward = transform.forward();
        transform.rotate(Quat::from_axis_angle(*forward, roll * cam.roll_speed * dt));
    }

    // --- Movement input in camera-local space ---
    let mut wish_dir = Vec3::ZERO;
    if input.pressed(KeyCode::KeyW) { wish_dir += *transform.forward(); }
    if input.pressed(KeyCode::KeyS) { wish_dir -= *transform.forward(); }
    if input.pressed(KeyCode::KeyA) { wish_dir -= *transform.right(); }
    if input.pressed(KeyCode::KeyD) { wish_dir += *transform.right(); }
    if input.pressed(KeyCode::Space) { wish_dir += *transform.up(); }
    if input.pressed(KeyCode::ControlLeft) { wish_dir -= *transform.up(); }

    let speed = if input.pressed(KeyCode::ShiftLeft) {
        cam.speed * cam.boost_multiplier
    } else {
        cam.speed
    };

    if wish_dir != Vec3::ZERO {
        wish_dir = wish_dir.normalize();
        state.velocity = wish_dir * speed;
    } else {
        // Apply friction (exponential decay)
        state.velocity *= (-cam.friction * dt).exp();
        if state.velocity.length_squared() < 0.0001 {
            state.velocity = Vec3::ZERO;
        }
    }

    transform.translation += state.velocity * dt;

    // Re-normalize quaternion to prevent drift from accumulated rotations
    transform.rotation = transform.rotation.normalize();
}
