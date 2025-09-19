use bevy::prelude::*;
use bevy::math::primitives::Cylinder;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::window::FileDragAndDrop;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use futures_lite::future::{poll_once, block_on};
use forge_protocol::{SlicingRequest, InputModel, Profile, Bed, BedOrigin, Outputs};
use forge_exts_slicer_prusa::run_slice as run_prusa_slice;
use std::path::PathBuf;
mod camera;
mod stl;

// Use 10mm minor squares to cover 25x25 cm with 5x5 blocks (each block = 50mm)
pub(crate) const GRID_SQUARE_MM: f32 = 10.0; // 10mm squares
pub(crate) const GRID_BLOCK_SIZE: usize = 5; // 5x5 squares per block
pub(crate) const GRID_BLOCKS: usize = 5; // total 5x5 blocks

pub struct ForgeViewerPlugin;

impl Plugin for ForgeViewerPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<stl::StlMesh>()
            .init_asset_loader::<stl::StlLoader>()
            .insert_resource(FpsCounter { avg: 0.0 })
            .insert_resource(LoadedDefault(false))
            .insert_resource(DragState::default())
            .insert_resource(CurrentModelPath(None))
            .insert_resource(SliceJob::Idle)
            .add_systems(Startup, (camera::setup_camera, spawn_axes_arrows, setup_fps_ui))
            .add_systems(Update, (
                draw_gizmos,
                draw_selection_gizmos,
                drag_selected_model,
                camera::camera_orbit_controls,
                mouse_pick_select,
                stl_instantiate_ready,
                slice_hotkey,
                poll_slice_job,
                update_fps_ui,
                stl_file_drop_loader_assets,
                schedule_default_asset_load_once,
            ));
    }
}

// camera setup moved to camera.rs

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



// camera helpers moved to camera.rs

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

#[derive(Resource, Default)]
struct CurrentModelPath(Option<PathBuf>);

#[derive(Component, Clone)]
struct StlInstance { handle: Handle<stl::StlMesh> }

#[derive(Resource)]
enum SliceJob {
    Idle,
    Pending { model: PathBuf },
    InProgress { task: Task<anyhow::Result<()>> },
}

fn slice_hotkey(
    keys: Res<ButtonInput<KeyCode>>,
    current: Res<CurrentModelPath>,
    mut job: ResMut<SliceJob>,
) {
    if keys.just_pressed(KeyCode::KeyS) {
        if let (Some(model), true) = (&current.0, matches!(*job, SliceJob::Idle)) {
            *job = SliceJob::Pending { model: model.clone() };
        }
    }
}

fn poll_slice_job(
    mut job: ResMut<SliceJob>,
) {
    if let SliceJob::InProgress { task } = &mut *job {
        if let Some(res) = block_on(poll_once(task)) {
            if let Err(e) = res { eprintln!("Slice error: {e}"); }
            *job = SliceJob::Idle;
        }
    } else if let SliceJob::Pending { model } = &*job {
        // Kick off task
        let model_path = model.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move { run_slice_request(model_path).await });
        *job = SliceJob::InProgress { task };
    }
}

async fn run_slice_request(model_path: PathBuf) -> anyhow::Result<()> {
    use std::fs;
    // Build a simple request using provided model path
    let stem = PathBuf::from(&model_path).file_stem().and_then(|s| s.to_str()).unwrap_or("job").to_string();
    let out_dir = PathBuf::from("out");
    fs::create_dir_all(&out_dir)?;
    let req = SlicingRequest {
        engine: "prusa-slicer".to_string(),
        inputs: vec![InputModel { path: model_path.to_string_lossy().to_string(), transform: [
            1.0,0.0,0.0,0.0,
            0.0,1.0,0.0,0.0,
            0.0,0.0,1.0,0.0,
            0.0,0.0,0.0,1.0,
        ]}],
        profile: Profile { nozzle_mm: 0.4, layer_h_mm: 0.2, material: "PLA".into(), speed_preset: "standard".into() },
        bed: Bed { size_mm: [220.0, 220.0, 250.0], origin: BedOrigin::Min },
        outputs: Outputs { gcode: out_dir.join(format!("{}.gcode", stem)).to_string_lossy().to_string(), preview: out_dir.join("preview.json").to_string_lossy().to_string() },
    };
    let _ = fs::write(out_dir.join("request.json"), serde_json::to_vec_pretty(&req)?);
    // Call adapter library (still spawns PrusaSlicer out-of-process)
    run_prusa_slice(&req)
}


// old loading overlay/job systems removed

// camera controls moved to camera.rs

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

// (Removed) Loading overlay and job machinery; asset server handles reloads

fn schedule_default_asset_load_once(
    mut loaded: ResMut<LoadedDefault>,
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut current: ResMut<CurrentModelPath>,
) {
    if loaded.0 { return; }
    let fs_path = std::path::Path::new("assets/sample.stl");
    if fs_path.exists() {
        let handle: Handle<stl::StlMesh> = assets.load("sample.stl");
        commands.spawn((StlInstance { handle: handle.clone() }, Selectable));
        current.0 = Some(fs_path.to_path_buf());
        loaded.0 = true;
    }
}

fn stl_file_drop_loader_assets(
    mut ev: EventReader<FileDragAndDrop>,
    assets: Res<AssetServer>,
    mut commands: Commands,
    mut current: ResMut<CurrentModelPath>,
) {
    for e in ev.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = e {
            if path_buf.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("stl")).unwrap_or(false) {
                // Ingest into assets/imports for AssetServer to watch/reload
                let file_name = path_buf.file_name().and_then(|s| s.to_str()).unwrap_or("dropped.stl");
                let rel_path = format!("imports/{}", file_name);
                let dst_fs_path = std::path::Path::new("assets").join(&rel_path);
                let _ = std::fs::create_dir_all(dst_fs_path.parent().unwrap());
                let _ = std::fs::copy(&path_buf, &dst_fs_path);

                let handle: Handle<stl::StlMesh> = assets.load(rel_path.clone());
                commands.spawn((StlInstance { handle: handle.clone() }, Selectable, Selected));
                current.0 = Some(dst_fs_path);
            }
        }
    }
}

fn stl_instantiate_ready(
    stl_assets: Res<Assets<stl::StlMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
    q: Query<(Entity, &StlInstance), Without<Handle<Mesh>>>,
) {
    for (ent, inst) in &q {
        if let Some(data) = stl_assets.get(&inst.handle) {
            let mesh_h = meshes.add(data.to_bevy_mesh());
            let mat = materials.add(StandardMaterial { base_color: Color::rgb_u8(210, 210, 210), perceptual_roughness: 0.8, metallic: 0.02, ..Default::default() });
            let (min, max) = stl::compute_bounds(&data.positions);
            commands.entity(ent).insert((
                PbrBundle { mesh: mesh_h, material: mat, transform: Transform::from_xyz(0.0, 0.0, 0.0), ..Default::default() },
                ModelBounds { min, max },
            ));
        }
    }
}

// STL helpers moved to stl.rs
