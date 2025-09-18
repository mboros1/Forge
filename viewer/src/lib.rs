use anyhow::Result;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::math::primitives::Cylinder;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::window::FileDragAndDrop;
use std::collections::HashMap;
use std::path::PathBuf;

// Use 10mm minor squares to cover 25x25 cm with 5x5 blocks (each block = 50mm)
const GRID_SQUARE_MM: f32 = 10.0; // 10mm squares
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
        .insert_resource(FpsCounter { avg: 0.0 })
        .insert_resource(LoadedDefault(false))
        .add_systems(Startup, (setup_camera, spawn_axes_arrows, setup_fps_ui))
        .add_systems(Update, (
            draw_gizmos,
            camera_orbit_controls,
            update_fps_ui,
            stl_file_drop_loader,
            try_load_default_model,
        ))
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

fn spawn_axes_arrows(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Build a simple arrow composed of a cylinder shaft and a cone tip (custom mesh)
    let shaft_len = 24.0;
    let tip_len = 4.0;
    let shaft_r = 0.4;
    let tip_r = 1.0;

    let shaft_mesh = meshes.add(Mesh::from(Cylinder { radius: shaft_r, half_height: shaft_len * 0.5 }));
    let tip_mesh = meshes.add(build_cone_mesh(tip_r, tip_len, 32));

    let red = materials.add(StandardMaterial { base_color: Color::rgb(1.0, 0.1, 0.1), cull_mode: None, ..Default::default() });
    let green = materials.add(StandardMaterial { base_color: Color::rgb(0.1, 1.0, 0.1), cull_mode: None, ..Default::default() });
    let blue = materials.add(StandardMaterial { base_color: Color::rgb(0.1, 0.5, 1.0), cull_mode: None, ..Default::default() });

    // Helper to spawn one arrow oriented along +axis (we author meshes along +Y)
    let mut spawn_arrow = |axis: Vec3, color: &Handle<StandardMaterial>| {
        let rot = if axis == Vec3::X {
            Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2)
        } else if axis == Vec3::Z {
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)
        } else {
            Quat::IDENTITY
        };
        let shaft_translation = Vec3::Y * (shaft_len * 0.5);
        let tip_translation = Vec3::Y * (shaft_len);

        commands.spawn(PbrBundle {
            mesh: shaft_mesh.clone(),
            material: color.clone(),
            transform: Transform::from_rotation(rot).with_translation(rot * shaft_translation),
            ..Default::default()
        });
        commands.spawn(PbrBundle {
            mesh: tip_mesh.clone(),
            material: color.clone(),
            transform: Transform::from_rotation(rot).with_translation(rot * tip_translation),
            ..Default::default()
        });
    };

    spawn_arrow(Vec3::X, &red);
    spawn_arrow(Vec3::Y, &green);
    spawn_arrow(Vec3::Z, &blue);
}



fn offset_from_spherical(distance: f32, yaw: f32, pitch: f32) -> Vec3 {
    let x = distance * pitch.cos() * yaw.cos();
    let y = distance * pitch.cos() * yaw.sin();
    let z = distance * pitch.sin();
    Vec3::new(x, y, z)
}

fn build_cone_mesh(radius: f32, height: f32, segments: usize) -> Mesh {
    // Cone along +Y axis, base at y=0, apex at y=height
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let _apex_index = 0u32;
    positions.push([0.0, height, 0.0]);
    normals.push([0.0, 1.0, 0.0]); // rough; overwritten per-face for sides by duplication

    // Base center
    let base_center_index = 1u32;
    positions.push([0.0, 0.0, 0.0]);
    normals.push([0.0, -1.0, 0.0]);

    // Rim points
    let mut rim_indices: Vec<u32> = Vec::with_capacity(segments);
    for i in 0..segments {
        let th = (i as f32) / (segments as f32) * std::f32::consts::TAU;
        let x = radius * th.cos();
        let z = radius * th.sin();
        positions.push([x, 0.0, z]);
        normals.push([0.0, -1.0, 0.0]); // for base cap; side normals are handled via duplication below
        rim_indices.push((2 + i) as u32);
    }

    // Base cap triangles (fan)
    for i in 0..segments {
        let i0 = base_center_index;
        let i1 = rim_indices[i];
        let i2 = rim_indices[(i + 1) % segments];
        indices.extend_from_slice(&[i0, i2, i1]); // winding so normal faces -Y
    }

    // Side triangles: duplicate vertices per-face for flat shading
    let _start_side = positions.len() as u32;
    for i in 0..segments {
        let i_rim0 = rim_indices[i] as usize;
        let i_rim1 = rim_indices[(i + 1) % segments] as usize;
        let p_apex = Vec3::new(0.0, height, 0.0);
        let p0 = Vec3::from(positions[i_rim0]);
        let p1 = Vec3::from(positions[i_rim1]);
        // Compute outward normal for the side face and orient triangle winding CCW as seen from outside
        let n = (p1 - p_apex).cross(p0 - p_apex).normalize_or_zero();
        let base = positions.len() as u32;
        // apex duplicate
        positions.push([p_apex.x, p_apex.y, p_apex.z]);
        normals.push([n.x, n.y, n.z]);
        // rim0 duplicate
        positions.push([p0.x, p0.y, p0.z]);
        normals.push([n.x, n.y, n.z]);
        // rim1 duplicate
        positions.push([p1.x, p1.y, p1.z]);
        normals.push([n.x, n.y, n.z]);
        indices.extend_from_slice(&[base, base + 2, base + 1]);
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_indices(Indices::U32(indices));
    mesh
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
    // Grid on XY plane (Z=0), 25x25 cm total, 10mm minor, bold every 50mm

    // Grid on XY plane (Z=0)
    let squares_per_axis = (GRID_BLOCKS * GRID_BLOCK_SIZE) as i32; // 25
    let extent = GRID_SQUARE_MM * squares_per_axis as f32; // 250mm (25cm)

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

#[derive(Component)]
struct FpsText;

#[derive(Resource)]
struct FpsCounter { avg: f32 }

fn setup_fps_ui(mut commands: Commands) {
    commands.spawn((
        TextBundle::from_sections([
            TextSection::new("FPS: ", TextStyle { font_size: 14.0, color: Color::WHITE, ..Default::default() }),
            TextSection::new("--", TextStyle { font_size: 14.0, color: Color::YELLOW, ..Default::default() }),
        ])
        .with_style(Style {
            position_type: PositionType::Absolute,
            left: Val::Px(10.0),
            top: Val::Px(8.0),
            ..Default::default()
        }),
        FpsText,
    ));
}

fn update_fps_ui(time: Res<Time>, mut fps: ResMut<FpsCounter>, mut q: Query<&mut Text, With<FpsText>>) {
    let dt = time.delta_seconds().max(1e-6);
    let inst = 1.0 / dt;
    // exponential moving average
    let alpha = 0.1;
    fps.avg = if fps.avg == 0.0 { inst } else { fps.avg * (1.0 - alpha) + inst * alpha };
    if let Ok(mut text) = q.get_single_mut() {
        text.sections[1].value = format!("{:.1}", fps.avg);
    }
}

#[derive(Resource)]
struct LoadedDefault(bool);

fn try_load_default_model(
    mut loaded: ResMut<LoadedDefault>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    if loaded.0 { return; }
    let path = PathBuf::from("assets/sample.stl");
    if path.exists() {
        if let Ok(mesh) = load_stl_mesh(&path) {
            let handle = meshes.add(mesh);
            let mat = materials.add(StandardMaterial { base_color: Color::rgb_u8(200, 200, 200), perceptual_roughness: 0.7, metallic: 0.05, ..Default::default() });
            commands.spawn(PbrBundle { mesh: handle, material: mat, transform: Transform::from_xyz(0.0, 0.0, 0.0), ..Default::default() });
            loaded.0 = true;
        }
    }
}

fn stl_file_drop_loader(
    mut ev: EventReader<FileDragAndDrop>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    for e in ev.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = e {
            if path_buf.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("stl")).unwrap_or(false) {
                match load_stl_mesh(path_buf) {
                    Ok(mesh) => {
                        let handle = meshes.add(mesh);
                        let mat = materials.add(StandardMaterial { base_color: Color::rgb_u8(210, 210, 210), perceptual_roughness: 0.8, metallic: 0.02, ..Default::default() });
                        commands.spawn(PbrBundle { mesh: handle, material: mat, transform: Transform::from_xyz(0.0, 0.0, 0.0), ..Default::default() });
                    }
                    Err(err) => {
                        eprintln!("Failed to load STL {}: {err}", path_buf.display());
                    }
                }
            }
        }
    }
}

fn load_stl_mesh(path: &PathBuf) -> Result<Mesh> {
    use std::fs::File;
    let mut f = File::open(path)?;
    let mesh = stl_io::read_stl(&mut f)?;

    // Build positions
    let positions: Vec<[f32;3]> = mesh.vertices.iter().map(|v| [v[0], v[1], v[2]]).collect();

    // Indices (triangles)
    let mut indices: Vec<u32> = Vec::with_capacity(mesh.faces.len() * 3);
    for tri in &mesh.faces {
        indices.push(tri.vertices[0] as u32);
        indices.push(tri.vertices[1] as u32);
        indices.push(tri.vertices[2] as u32);
    }

    // Compute per-vertex normals from face normals
    let mut norms_acc: HashMap<usize, Vec<Vec3>> = HashMap::new();
    for tri in &mesh.faces {
        let i0 = tri.vertices[0] as usize;
        let i1 = tri.vertices[1] as usize;
        let i2 = tri.vertices[2] as usize;
        let p0 = Vec3::from(positions[i0]);
        let p1 = Vec3::from(positions[i1]);
        let p2 = Vec3::from(positions[i2]);
        let n = (p1 - p0).cross(p2 - p0).normalize_or_zero();
        norms_acc.entry(i0).or_default().push(n);
        norms_acc.entry(i1).or_default().push(n);
        norms_acc.entry(i2).or_default().push(n);
    }
    let mut normals: Vec<[f32;3]> = Vec::with_capacity(positions.len());
    for i in 0..positions.len() {
        let n = norms_acc.get(&i).map(|ns| {
            let mut s = Vec3::ZERO;
            for v in ns { s += *v; }
            s.normalize_or_zero()
        }).unwrap_or(Vec3::Y);
        normals.push([n.x, n.y, n.z]);
    }

    let mut bevy_mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    bevy_mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    bevy_mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    bevy_mesh.insert_indices(Indices::U32(indices));
    Ok(bevy_mesh)
}
