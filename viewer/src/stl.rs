use bevy::asset::{io::Reader, AssetLoader, BoxedFuture, LoadContext, LoadedAsset};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use futures_lite::io::AsyncReadExt;
use std::collections::HashMap;

#[derive(Asset, TypePath, Debug, Clone)]
pub struct StlMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl StlMesh {
    pub fn to_bevy_mesh(&self) -> Mesh {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, default());
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions.clone());
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals.clone());
        m.insert_indices(Indices::U32(self.indices.clone()));
        m
    }
}

#[derive(Default)]
pub struct StlLoader;

impl AssetLoader for StlLoader {
    type Asset = StlMesh;
    type Settings = ();
    type Error = anyhow::Error;

    fn load<'a>(
        &'a self,
        reader: &'a mut Reader<'_>,
        _settings: &'a Self::Settings,
        _load_context: &'a mut LoadContext<'_>,
    ) -> BoxedFuture<'a, Result<Self::Asset, Self::Error>> {
        Box::pin(async move {
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await?;
            let mut cursor = std::io::Cursor::new(&bytes);
            let mesh = stl_io::read_stl(&mut cursor)?;

            let positions: Vec<[f32; 3]> = mesh
                .vertices
                .iter()
                .map(|v| [v[0], v[1], v[2]])
                .collect();

            let mut indices: Vec<u32> = Vec::with_capacity(mesh.faces.len() * 3);
            for tri in &mesh.faces {
                indices.push(tri.vertices[0] as u32);
                indices.push(tri.vertices[1] as u32);
                indices.push(tri.vertices[2] as u32);
            }

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
            let mut normals: Vec<[f32; 3]> = Vec::with_capacity(positions.len());
            for i in 0..positions.len() {
                let n = norms_acc
                    .get(&i)
                    .map(|ns| {
                        let mut s = Vec3::ZERO;
                        for v in ns { s += *v; }
                        s.normalize_or_zero()
                    })
                    .unwrap_or(Vec3::Y);
                normals.push([n.x, n.y, n.z]);
            }

            Ok(StlMesh {
                positions,
                normals,
                indices,
            })
        })
    }

    fn extensions(&self) -> &[&str] { &["stl"] }
}

pub fn compute_bounds(positions: &[[f32; 3]]) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for p in positions {
        let v = Vec3::new(p[0], p[1], p[2]);
        min = min.min(v);
        max = max.max(v);
    }
    (min, max)
}
