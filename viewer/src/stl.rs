use anyhow::Result;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use std::collections::HashMap;
use std::path::PathBuf;

pub fn load_stl_mesh(path: &PathBuf) -> Result<Mesh> {
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

pub fn compute_bounds(positions: &[[f32;3]]) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for p in positions {
        let v = Vec3::new(p[0], p[1], p[2]);
        min = min.min(v);
        max = max.max(v);
    }
    (min, max)
}

