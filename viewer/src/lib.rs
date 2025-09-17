use anyhow::Result;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;

const GRID_SQUARE_MM: f32 = 1.0; // 1mm squares
const GRID_BLOCK_SIZE: usize = 5; // 5x5 squares per block
const GRID_BLOCKS: usize = 5; // total 5x5 blocks

pub fn run() -> Result<()> {
    App::new()
        .insert_resource(ClearColor(Color::rgb_u8(20, 22, 25)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Forge Viewer".into(),
                ..Default::default()
            }),
            ..Default::default()
        }))
        .add_systems(Startup, (setup_camera,))
        .add_systems(Update, (draw_gizmos, camera_orbit_controls))
        .run();
    Ok(())
}

#[derive(Component)]
struct OrbitCamera {
    target: Vec3,
    distance: f32,
    yaw: f32,
    pitch: f32,
}

fn setup_camera(mut commands: Commands) {
    let total_mm = GRID_SQUARE_MM * (GRID_BLOCKS * GRID_BLOCK_SIZE) as f32;
    let center = Vec3::new(total_mm * 0.5, total_mm * 0.5, 0.0);
    let distance = (total_mm * 2.0).max(100.0);

    let yaw = -45f32.to_radians();
    let pitch = 30f32.to_radians();

    let mut cam_tf = Transform::from_translation(center + offset_from_spherical(distance, yaw, pitch));
    cam_tf.look_at(center, Vec3::Z);

    commands.spawn((
        Camera3dBundle {
            transform: cam_tf,
            ..Default::default()
        },
        OrbitCamera {
            target: center,
            distance,
            yaw,
            pitch,
        },
    ));

    // Basic light
    commands.spawn((
        DirectionalLightBundle {
            directional_light: DirectionalLight {
                illuminance: 10_000.0,
                shadows_enabled: false,
                ..Default::default()
            },
            transform: Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -45f32.to_radians(), 0.0, 45f32.to_radians())),
            ..Default::default()
        },
    ));
}

fn offset_from_spherical(distance: f32, yaw: f32, pitch: f32) -> Vec3 {
    let x = distance * pitch.cos() * yaw.cos();
    let y = distance * pitch.cos() * yaw.sin();
    let z = distance * pitch.sin();
    Vec3::new(x, y, z)
}

fn camera_orbit_controls(
    time: Res<Time>,
    mut ev_motion: EventReader<MouseMotion>,
    mut ev_scroll: EventReader<MouseWheel>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut q_cam: Query<(&mut Transform, &mut OrbitCamera)>,
) {
    let (mut tf, mut orb) = match q_cam.get_single_mut() {
        Ok(v) => v,
        Err(_) => return,
    };

    let mut delta = Vec2::ZERO;
    for ev in ev_motion.read() {
        delta += ev.delta;
    }

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

    // Small damping for nicer feel when not interacting
    let _ = time.delta_seconds();
}

fn draw_gizmos(mut gizmos: Gizmos) {
    // Axes: X red, Y green, Z blue
    let axis_len = 5.0;
    gizmos.ray(Vec3::ZERO, Vec3::X * axis_len, Color::rgb(1.0, 0.0, 0.0));
    gizmos.ray(Vec3::ZERO, Vec3::Y * axis_len, Color::rgb(0.0, 1.0, 0.0));
    gizmos.ray(Vec3::ZERO, Vec3::Z * axis_len, Color::rgb(0.0, 0.5, 1.0)); // bluish Z

    // Grid on XY plane (Z=0)
    let squares_per_axis = (GRID_BLOCKS * GRID_BLOCK_SIZE) as i32; // 25
    let extent = GRID_SQUARE_MM * squares_per_axis as f32; // 25mm

    // Minor lines every 1mm
    let minor = Color::rgb_u8(70, 74, 80);
    let major = Color::rgb_u8(110, 114, 120);

    for i in 0..=squares_per_axis {
        let x = i as f32 * GRID_SQUARE_MM;
        let y = i as f32 * GRID_SQUARE_MM;
        let is_major = i % (GRID_BLOCK_SIZE as i32) == 0;
        let col = if is_major { major } else { minor };

        // Vertical line at x
        gizmos.line(Vec3::new(x, 0.0, 0.0), Vec3::new(x, extent, 0.0), col);
        // Horizontal line at y
        gizmos.line(Vec3::new(0.0, y, 0.0), Vec3::new(extent, y, 0.0), col);
    }
}
