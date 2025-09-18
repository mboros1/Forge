use anyhow::Result;
use bevy::input::mouse::{MouseMotion, MouseWheel};
use bevy::prelude::*;
use bevy::math::primitives::Cylinder;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::window::FileDragAndDrop;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use futures_lite::future::{poll_once, block_on};
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
        .insert_resource(DragState::default())
        .insert_resource(LoadingJob::Idle)
        .add_systems(Startup, (setup_camera, spawn_axes_arrows, setup_fps_ui))
        .add_systems(Update, (
            draw_gizmos,
            draw_selection_gizmos,
            drag_selected_model,
            camera_orbit_controls,
            mouse_pick_select,
            update_fps_ui,
            stl_file_drop_loader,
            schedule_default_load_once,
            kick_off_loading_job,
            poll_loading_job,
            update_loading_ui,
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

#[derive(Component, Clone, Copy)]
struct ModelBounds { min: Vec3, max: Vec3 }

#[derive(Component)]
struct Selectable;

#[derive(Component)]
struct Selected;

fn kick_off_loading_job(
    mut commands: Commands,
    mut job: ResMut<LoadingJob>,
    overlay_query: Query<Entity, With<LoadingOverlay>>,
) {
    // If we have a pending file and no overlay yet, spawn overlay and start task
    if let LoadingJob::Pending { file } = &*job {
        // If overlay not present, create it
        if overlay_query.get_single().is_err() {
            spawn_loading_overlay(&mut commands, file);
        }
        // Start task
        let file_path = file.clone();
        let pool = AsyncComputeTaskPool::get();
        let task: Task<Result<(Mesh, (Vec3, Vec3))>> = pool.spawn(async move {
            // Do sync parse in async pool
            let res = load_stl_mesh(&file_path);
            match res {
                Ok(mesh) => {
                    // Extract positions to compute bounds
                    use bevy::render::mesh::VertexAttributeValues;
                    let positions: Vec<[f32;3]> = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
                        Some(VertexAttributeValues::Float32x3(v)) => v.clone(),
                        _ => Vec::new(),
                    };
                    let bounds = compute_bounds(&positions);
                    Ok((mesh, bounds))
                }
                Err(e) => Err(e),
            }
        });
        let file2 = file.clone();
        *job = LoadingJob::InProgress { _file: file2, task };
    }
}

fn poll_loading_job(
    mut job: ResMut<LoadingJob>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    overlay: Query<Entity, With<LoadingOverlay>>,
) {
    // Handle cancelled state by clearing overlay and going idle
    if let LoadingJob::Cancelled = &*job {
        if let Ok(e) = overlay.get_single() { commands.entity(e).despawn_recursive(); }
        *job = LoadingJob::Idle;
        return;
    }
    if let LoadingJob::InProgress { _file: _, task } = &mut *job {
        if let Some(result) = block_on(poll_once(task)) {
            // Remove overlay
            if let Ok(e) = overlay.get_single() { commands.entity(e).despawn_recursive(); }
            match result {
                Ok((mesh, (min, max))) => {
                    let h = meshes.add(mesh);
                    let mat = materials.add(StandardMaterial { base_color: Color::rgb_u8(210, 210, 210), perceptual_roughness: 0.8, metallic: 0.02, ..Default::default() });
                    commands.spawn((
                        PbrBundle { mesh: h, material: mat, transform: Transform::from_xyz(0.0, 0.0, 0.0), ..Default::default() },
                        ModelBounds { min, max },
                        Selectable,
                        Selected,
                    ));
                    *job = LoadingJob::Idle;
                }
                Err(err) => {
                    eprintln!("Failed to load STL: {err}");
                    *job = LoadingJob::Idle;
                }
            }
        }
    }
}

fn spawn_loading_overlay(commands: &mut Commands, file: &PathBuf) {
    let file_name = file.file_name().and_then(|s| s.to_str()).unwrap_or("(file)");
    // Fullscreen overlay centered container
    commands
        .spawn((
            NodeBundle {
                style: Style {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    top: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                background_color: BackgroundColor(Color::rgba(0.0, 0.0, 0.0, 0.2)),
                ..default()
            },
            LoadingOverlay,
        ))
        .with_children(|root| {
            // The dialog panel
            root
                .spawn(NodeBundle {
                    style: Style {
                        width: Val::Px(320.0),
                        height: Val::Px(140.0),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(10.0),
                        ..default()
                    },
                    background_color: BackgroundColor(Color::rgba(0.08, 0.08, 0.1, 0.95)),
                    ..default()
                })
                .with_children(|p| {
                    p.spawn((
                        TextBundle::from_section(
                            format!("Loading {file_name}"),
                            TextStyle { font_size: 16.0, color: Color::WHITE, ..default() },
                        ),
                        LoadingFileText,
                    ));

                    // Progress bar background
                    p.spawn(NodeBundle {
                        style: Style { width: Val::Px(260.0), height: Val::Px(12.0), ..default() },
                        background_color: BackgroundColor(Color::rgb_u8(60, 60, 60)),
                        ..default()
                    })
                    .with_children(|p2| {
                        // Fill
                        p2.spawn((
                            NodeBundle {
                                style: Style { width: Val::Px(0.0), height: Val::Percent(100.0), ..default() },
                                background_color: BackgroundColor(Color::rgb_u8(180, 200, 255)),
                                ..default()
                            },
                            LoadingBarFill,
                        ));
                    });

                    // Cancel button
                    p.spawn((
                        ButtonBundle {
                            style: Style { padding: UiRect::axes(Val::Px(12.0), Val::Px(8.0)), ..default() },
                            background_color: BackgroundColor(Color::rgb_u8(120, 40, 40)),
                            ..default()
                        },
                        CancelButton,
                    ))
                    .with_children(|b| {
                        b.spawn(TextBundle::from_section(
                            "Cancel",
                            TextStyle { font_size: 14.0, color: Color::WHITE, ..default() },
                        ));
                    });
                });
        });
}

fn update_loading_ui(
    time: Res<Time>,
    mut bar: Query<&mut Style, With<LoadingBarFill>>,
    mut btns: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<CancelButton>)>,
    mut job: ResMut<LoadingJob>,
    overlay: Query<Entity, With<LoadingOverlay>>,
) {
    // Indeterminate animation: smooth loop from 40px..240px
    if let Ok(mut style) = bar.get_single_mut() {
        let t = time.elapsed_seconds();
        let phase = ((t * 0.6) % 1.0) as f32; // 0..1 sawtooth
        style.width = Val::Px(40.0 + 200.0 * phase);
    }

    for (interaction, mut color) in btns.iter_mut() {
        match *interaction {
            Interaction::Pressed => {
                *color = BackgroundColor(Color::rgb_u8(160, 60, 60));
                // Cancel job and remove overlay
                *job = LoadingJob::Cancelled;
                if overlay.get_single().is_ok() { /* overlay removal handled next frame */ }
            }
            Interaction::Hovered => *color = BackgroundColor(Color::rgb_u8(140, 50, 50)),
            Interaction::None => *color = BackgroundColor(Color::rgb_u8(120, 40, 40)),
        }
    }
}

fn camera_orbit_controls(
    time: Res<Time>,
    mut ev_motion: EventReader<MouseMotion>,
    mut ev_scroll: EventReader<MouseWheel>,
    buttons: Res<ButtonInput<MouseButton>>,
    drag: Res<DragState>,
    mut q_cam: Query<(&mut Transform, &mut OrbitCamera)>,
) {
    // Suppress orbit while dragging a model
    if drag.is_active() { 
        // still handle scroll zoom
        for ev in ev_scroll.read() {
            let (mut tf, mut orb) = match q_cam.get_single_mut() { Ok(v) => v, Err(_) => return };
            let scroll = match ev.unit { bevy::input::mouse::MouseScrollUnit::Line => ev.y * 50.0, bevy::input::mouse::MouseScrollUnit::Pixel => ev.y };
            orb.distance *= (1.0 - scroll * 0.001).clamp(0.1, 10.0);
            orb.distance = orb.distance.clamp(10.0, 10_000.0);
            let offset = offset_from_spherical(orb.distance, orb.yaw, orb.pitch);
            tf.translation = orb.target + offset;
            tf.look_at(orb.target, Vec3::Z);
        }
        return; 
    }
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

#[derive(Debug, Default, Resource)]
struct DragState { active: Option<DragData> }

impl DragState { fn is_active(&self) -> bool { self.active.is_some() } }

#[derive(Debug, Clone)]
struct DragData { entity: Entity, start_entity_pos: Vec3, start_hit_world: Vec3 }

fn drag_selected_model(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    q_cam: Query<(&Camera, &GlobalTransform)>,
    q_sel: Query<(Entity, &GlobalTransform, &ModelBounds), With<Selected>>,
    mut q_sel_mut: Query<&mut Transform>,
    mut drag: ResMut<DragState>,
) {
    let (camera, cam_tf) = match q_cam.get_single() { Ok(v) => v, Err(_) => return };
    let window = match windows.get_single() { Ok(w) => w, Err(_) => return };

    // Begin drag: left just pressed over selected model
    if buttons.just_pressed(MouseButton::Left) {
        if let Some(cursor) = window.cursor_position() {
            if let Some(ray) = camera.viewport_to_world(cam_tf, cursor) {
                // Check if we hit the selected model's AABB
                if let Ok((ent, gt, bounds)) = q_sel.get_single() {
                    if ray_hit_aabb_local(bounds.min, bounds.max, gt, &ray).is_some() {
                        if let Some(hit) = ray_plane_intersection_world(&ray, Vec3::ZERO, Vec3::Z) {
                            // Read current transform from mutable query (short-lived borrow)
                            if let Ok(tf) = q_sel_mut.get_mut(ent) {
                                let start = tf.translation;
                                drop(tf);
                                drag.active = Some(DragData { entity: ent, start_entity_pos: start, start_hit_world: hit });
                            }
                        }
                    }
                }
            }
        }
    }

    // End drag
    if buttons.just_released(MouseButton::Left) {
        drag.active = None;
    }

    // Update drag
    if let Some(d) = drag.active.clone() {
        if let Some(cursor) = window.cursor_position() {
            if let Some(ray) = camera.viewport_to_world(cam_tf, cursor) {
                if let Some(hit) = ray_plane_intersection_world(&ray, Vec3::ZERO, Vec3::Z) {
                    if let Ok(mut tf) = q_sel_mut.get_mut(d.entity) {
                        let delta = hit - d.start_hit_world;
                        let mut new_pos = d.start_entity_pos + delta;
                        new_pos.z = d.start_entity_pos.z; // constrain to plate plane
                        tf.translation = new_pos;
                    }
                }
            }
        }
    }
}

fn mouse_pick_select(
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    q_cam: Query<(&Camera, &GlobalTransform)>,
    mut q_selectables: Query<(Entity, &GlobalTransform, &ModelBounds, Option<&Selected>), With<Selectable>>,
    mut commands: Commands,
) {
    if !buttons.just_pressed(MouseButton::Left) { return; }
    let window = match windows.get_single() { Ok(w) => w, Err(_) => return };
    let cursor = match window.cursor_position() { Some(p) => p, None => return };
    let (camera, cam_tf) = match q_cam.get_single() { Ok(v) => v, Err(_) => return };
    let Some(ray) = camera.viewport_to_world(cam_tf, cursor) else { return };

    // Pick nearest
    let mut best: Option<(Entity, f32)> = None;
    for (ent, gt, bounds, _sel) in q_selectables.iter_mut() {
        if let Some(t) = ray_hit_aabb_local(bounds.min, bounds.max, gt, &ray) {
            if t >= 0.0 { // in front of camera
                if let Some((_, best_t)) = best {
                    if t < best_t { best = Some((ent, t)); }
                } else {
                    best = Some((ent, t));
                }
            }
        }
    }

    // Update selection
    if let Some((hit_ent, _)) = best {
        // Remove Selected from all others; add to hit
        for (ent, _, _, sel) in q_selectables.iter_mut() {
            if ent == hit_ent {
                if sel.is_none() { commands.entity(ent).insert(Selected); }
            } else if sel.is_some() {
                commands.entity(ent).remove::<Selected>();
            }
        }
    } else {
        // Clicked empty space: clear all selections
        for (ent, _, _, sel) in q_selectables.iter_mut() {
            if sel.is_some() { commands.entity(ent).remove::<Selected>(); }
        }
    }
}

fn ray_hit_aabb_local(min: Vec3, max: Vec3, model_gt: &GlobalTransform, ray: &bevy::math::Ray3d) -> Option<f32> {
    // Transform world ray into model local space
    let inv = model_gt.compute_matrix().inverse();
    let o = inv.transform_point3(ray.origin);
    let d = inv.transform_vector3((*ray.direction).into());
    // Slab intersection in local space
    let mut tmin = f32::NEG_INFINITY;
    let mut tmax = f32::INFINITY;
    for i in 0..3 {
        let origin_i = o[i];
        let dir_i = d[i];
        let (min_i, max_i) = (min[i], max[i]);
        if dir_i.abs() < 1e-8 {
            if origin_i < min_i || origin_i > max_i { return None; }
        } else {
            let invd = 1.0 / dir_i;
            let mut t1 = (min_i - origin_i) * invd;
            let mut t2 = (max_i - origin_i) * invd;
            if t1 > t2 { std::mem::swap(&mut t1, &mut t2); }
            tmin = tmin.max(t1);
            tmax = tmax.min(t2);
            if tmin > tmax { return None; }
        }
    }
    Some(tmin)
}

fn ray_plane_intersection_world(ray: &bevy::math::Ray3d, plane_origin: Vec3, plane_normal: Vec3) -> Option<Vec3> {
    let ro = ray.origin;
    let rd: Vec3 = (*ray.direction).into();
    let denom = plane_normal.dot(rd);
    if denom.abs() < 1e-6 { return None; }
    let t = plane_normal.dot(plane_origin - ro) / denom;
    if t < 0.0 { return None; }
    Some(ro + rd * t)
}

fn draw_gizmos(mut gizmos: Gizmos) {
    // Grid on XY plane (Z=0), 25x25 cm total, 10mm minor, bold every 50mm
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

fn draw_selection_gizmos(mut gizmos: Gizmos, q: Query<(&GlobalTransform, &ModelBounds), With<Selected>>) {
    let l = 8.0; // corner leg length in mm
    for (gt, b) in &q {
        let min = b.min;
        let max = b.max;
        for &cx in &[min.x, max.x] {
            for &cy in &[min.y, max.y] {
                for &cz in &[min.z, max.z] {
                    let p = gt.transform_point(Vec3::new(cx, cy, cz));
                    // Three axis legs
                    let sign_x = if cx == min.x { 1.0 } else { -1.0 };
                    let sign_y = if cy == min.y { 1.0 } else { -1.0 };
                    let sign_z = if cz == min.z { 1.0 } else { -1.0 };
                    gizmos.line(p, p + Vec3::X * l * sign_x, Color::rgb_u8(255, 220, 0));
                    gizmos.line(p, p + Vec3::Y * l * sign_y, Color::rgb_u8(255, 220, 0));
                    gizmos.line(p, p + Vec3::Z * l * sign_z, Color::rgb_u8(255, 220, 0));
                }
            }
        }
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

#[derive(Component)]
struct LoadingOverlay;
#[derive(Component)]
struct LoadingBarFill;
#[derive(Component)]
struct LoadingFileText;
#[derive(Component)]
struct CancelButton;

#[derive(Resource)]
enum LoadingJob {
    Idle,
    Pending { file: PathBuf },
    InProgress { _file: PathBuf, task: Task<Result<(Mesh, (Vec3, Vec3))>> },
    Cancelled,
}

fn schedule_default_load_once(mut loaded: ResMut<LoadedDefault>, mut job: ResMut<LoadingJob>) {
    if loaded.0 { return; }
    let path = PathBuf::from("assets/sample.stl");
    if path.exists() {
        *job = LoadingJob::Pending { file: path };
        loaded.0 = true; // ensure we only schedule once
    }
}

fn stl_file_drop_loader(
    mut ev: EventReader<FileDragAndDrop>,
    mut job: ResMut<LoadingJob>,
) {
    for e in ev.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = e {
            if path_buf.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("stl")).unwrap_or(false) {
                *job = LoadingJob::Pending { file: path_buf.clone() };
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

fn compute_bounds(positions: &[[f32;3]]) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for p in positions {
        let v = Vec3::new(p[0], p[1], p[2]);
        min = min.min(v);
        max = max.max(v);
    }
    (min, max)
}
