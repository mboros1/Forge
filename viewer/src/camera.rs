use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;

#[derive(Component)]
pub struct OrbitCamera {
    pub target: Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
}

pub fn setup_camera(mut commands: Commands) {
    // Use the plate/grid extent from crate-level constants
    let total_mm = crate::GRID_SQUARE_MM * (crate::GRID_BLOCKS * crate::GRID_BLOCK_SIZE) as f32;
    let center = Vec3::new(total_mm * 0.5, total_mm * 0.5, 0.0);
    let distance = (total_mm * 2.0).max(100.0);

    let yaw = -45f32.to_radians();
    let pitch = 30f32.to_radians();

    let mut cam_tf = Transform::from_translation(offset_from_spherical(distance, yaw, pitch) + center);
    cam_tf.look_at(center, Vec3::Z);

    commands.spawn((
        Camera3dBundle { transform: cam_tf, ..Default::default() },
        OrbitCamera { target: center, distance, yaw, pitch },
    ));

    commands.spawn((
        DirectionalLightBundle {
            directional_light: DirectionalLight { illuminance: 10_000.0, shadows_enabled: false, ..Default::default() },
            transform: Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -45f32.to_radians(), 0.0, 45f32.to_radians())),
            ..Default::default()
        },
    ));
}

pub fn camera_orbit_controls(
    time: Res<Time>,
    mut ev_motion: EventReader<MouseMotion>,
    mut ev_scroll: EventReader<MouseWheel>,
    buttons: Res<ButtonInput<MouseButton>>,
    drag: Res<crate::DragState>,
    mut q_cam: Query<(&mut Transform, &mut OrbitCamera)>,
) {
    // Suppress orbit while dragging a model
    if drag.is_active() {
        // still handle scroll zoom
        for ev in ev_scroll.read() {
            if let Ok((mut tf, mut orb)) = q_cam.get_single_mut() {
                let scroll = match ev.unit {
                    bevy::input::mouse::MouseScrollUnit::Line => ev.y * 50.0,
                    bevy::input::mouse::MouseScrollUnit::Pixel => ev.y,
                };
                orb.distance *= (1.0 - scroll * 0.001).clamp(0.1, 10.0);
                orb.distance = orb.distance.clamp(10.0, 10_000.0);
                let offset = offset_from_spherical(orb.distance, orb.yaw, orb.pitch);
                tf.translation = orb.target + offset;
                tf.look_at(orb.target, Vec3::Z);
            }
        }
        return;
    }

    let (mut tf, mut orb) = match q_cam.get_single_mut() { Ok(v) => v, Err(_) => return };

    let mut delta = Vec2::ZERO;
    for ev in ev_motion.read() { delta += ev.delta; }

    // Orbit with left mouse
    if buttons.pressed(MouseButton::Left) {
        let sens = 0.3f32;
        orb.yaw -= delta.x.to_radians() * sens;
        orb.pitch += delta.y.to_radians() * sens;
        let limit = 89f32.to_radians();
        orb.pitch = orb.pitch.clamp(-limit, limit);
    }

    // Pan with right mouse
    if buttons.pressed(MouseButton::Right) {
        let pan_speed = orb.distance * 0.0015;
        // Right/Up vectors based on current yaw/pitch around Z-up
        let forward = (orb.target - tf.translation).normalize_or_zero();
        let right = forward.cross(Vec3::Z).normalize_or_zero();
        let up = Vec3::Z;
        orb.target += (-right * delta.x + up * delta.y) * pan_speed;
    }

    // Zoom with scroll
    for ev in ev_scroll.read() {
        let scroll = match ev.unit {
            bevy::input::mouse::MouseScrollUnit::Line => ev.y * 50.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => ev.y,
        };
        orb.distance *= (1.0 - scroll * 0.001).clamp(0.1, 10.0);
        orb.distance = orb.distance.clamp(10.0, 10_000.0);
    }

    // Recompute camera transform
    let offset = offset_from_spherical(orb.distance, orb.yaw, orb.pitch);
    tf.translation = orb.target + offset;
    tf.look_at(orb.target, Vec3::Z);

    // Small damping placeholder
    let _ = time.delta_seconds();
}

fn offset_from_spherical(distance: f32, yaw: f32, pitch: f32) -> Vec3 {
    let x = distance * pitch.cos() * yaw.cos();
    let y = distance * pitch.cos() * yaw.sin();
    let z = distance * pitch.sin();
    Vec3::new(x, y, z)
}

