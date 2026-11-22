use crate::mesh::{ChunkedMesh, MeshChunk, MeshVertex};
use glam::{Mat4, Vec3};
use gltf::mesh::Mode;
use std::convert::TryFrom;
use std::error::Error;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

pub struct ModelLoader;

impl ModelLoader {
    pub fn load_glb(path: impl AsRef<Path>) -> Result<ChunkedMesh, Box<dyn Error>> {
        let path = path.as_ref();
        let bytes = fs::read(path)?;
        load_glb_bytes(&bytes).map_err(|error| {
            io::Error::new(error.kind(), format!("{}: {error}", path.display())).into()
        })
    }

    pub fn load_glb_from_reader<R: Read>(mut reader: R) -> Result<ChunkedMesh, Box<dyn Error>> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        load_glb_bytes(&bytes).map_err(|error| error.into())
    }
}

fn load_glb_bytes(bytes: &[u8]) -> io::Result<ChunkedMesh> {
    let gltf = gltf::Gltf::from_slice(bytes).map_err(invalid_data)?;
    let binary = gltf
        .blob
        .as_deref()
        .ok_or_else(|| invalid_data("GLB has no embedded binary buffer"))?;

    if gltf
        .document
        .buffers()
        .any(|buffer| !matches!(buffer.source(), gltf::buffer::Source::Bin))
    {
        return Err(invalid_data(
            "external glTF buffers are not supported; use a self-contained GLB",
        ));
    }

    let scene = gltf
        .document
        .default_scene()
        .or_else(|| gltf.document.scenes().next())
        .ok_or_else(|| invalid_data("GLB contains no scene"))?;
    let mut mesh = ChunkedMesh::default();

    for node in scene.nodes() {
        append_node(node, Mat4::IDENTITY, binary, &mut mesh)?;
    }

    if mesh.vertices.is_empty() || mesh.indices.is_empty() {
        return Err(invalid_data("GLB scene contains no triangle geometry"));
    }
    mesh.chunks.push(MeshChunk {
        origin: [0, 0],
        indices: 0..mesh.indices.len(),
    });
    Ok(mesh)
}

fn append_node(
    node: gltf::Node<'_>,
    parent_transform: Mat4,
    binary: &[u8],
    output: &mut ChunkedMesh,
) -> io::Result<()> {
    if node.skin().is_some() {
        return Err(invalid_data(
            "skinned models are not supported by the static mesh renderer",
        ));
    }

    let transform = parent_transform * Mat4::from_cols_array_2d(&node.transform().matrix());
    let determinant = transform.determinant();
    if !transform.is_finite() || !determinant.is_finite() || determinant.abs() < 1.0e-8 {
        return Err(invalid_data(
            "scene contains a singular or non-finite node transform",
        ));
    }

    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            let primitive_index = primitive.index();
            append_primitive(primitive, transform, binary, output).map_err(|error| {
                invalid_data(format!(
                    "mesh {} primitive {}: {error}",
                    mesh.index(),
                    primitive_index
                ))
            })?;
        }
    }

    for child in node.children() {
        append_node(child, transform, binary, output)?;
    }
    Ok(())
}

fn append_primitive(
    primitive: gltf::Primitive<'_>,
    transform: Mat4,
    binary: &[u8],
    output: &mut ChunkedMesh,
) -> io::Result<()> {
    let material = primitive.material();
    if material.alpha_mode() != gltf::material::AlphaMode::Opaque {
        return Err(invalid_data(
            "transparent and masked materials are not supported yet",
        ));
    }
    let pbr = material.pbr_metallic_roughness();
    if pbr.base_color_texture().is_some() {
        return Err(invalid_data(
            "base-color textures are not supported yet; use material colors",
        ));
    }
    let base_color = pbr.base_color_factor();
    if !base_color.iter().all(|component| component.is_finite()) {
        return Err(invalid_data("material has a non-finite base color"));
    }

    let reader = primitive.reader(|_| Some(binary));
    let positions = reader
        .read_positions()
        .ok_or_else(|| invalid_data("primitive has no readable POSITION attribute"))?
        .map(|position| {
            transform
                .transform_point3(Vec3::from_array(position))
                .to_array()
        })
        .collect::<Vec<_>>();
    if positions.is_empty() || positions.iter().flatten().any(|value| !value.is_finite()) {
        return Err(invalid_data("primitive positions are empty or non-finite"));
    }
    let vertex_count = u32::try_from(positions.len())
        .map_err(|_| invalid_data("primitive has more than u32::MAX vertices"))?;

    let mut indices = reader
        .read_indices()
        .map(|indices| indices.into_u32().collect::<Vec<_>>())
        .unwrap_or_else(|| (0..vertex_count).collect());
    indices = triangulate(primitive.mode(), &indices)?;
    for index in &indices {
        if *index >= vertex_count {
            return Err(invalid_data("primitive index is outside its vertex range"));
        }
    }

    if transform.determinant() < 0.0 {
        for triangle in indices.chunks_exact_mut(3) {
            triangle.swap(1, 2);
        }
    }

    let normals = match reader.read_normals() {
        Some(normals) => {
            let normal_transform = transform.inverse().transpose();
            let normals = normals
                .map(|normal| {
                    normal_transform
                        .transform_vector3(Vec3::from_array(normal))
                        .normalize_or_zero()
                        .to_array()
                })
                .collect::<Vec<_>>();
            if normals.len() != positions.len()
                || normals.iter().flatten().any(|value| !value.is_finite())
            {
                return Err(invalid_data("primitive normals are invalid"));
            }
            normals
        }
        None => generate_normals(&positions, &indices),
    };

    let vertex_colors = reader
        .read_colors(0)
        .map(|colors| colors.into_rgba_f32().collect::<Vec<_>>());
    if vertex_colors
        .as_ref()
        .is_some_and(|colors| colors.len() != positions.len())
    {
        return Err(invalid_data(
            "primitive COLOR_0 count does not match POSITION count",
        ));
    }

    let base_vertex = u32::try_from(output.vertices.len())
        .map_err(|_| invalid_data("model has more than u32::MAX vertices"))?;
    if output.vertices.len() + positions.len() > u32::MAX as usize {
        return Err(invalid_data("model has more than u32::MAX vertices"));
    }

    for (index, position) in positions.into_iter().enumerate() {
        let vertex_color = vertex_colors
            .as_ref()
            .map(|colors| colors[index])
            .unwrap_or([1.0; 4]);
        let color = [
            base_color[0] * vertex_color[0],
            base_color[1] * vertex_color[1],
            base_color[2] * vertex_color[2],
        ];
        if !color.iter().all(|component| component.is_finite()) {
            return Err(invalid_data("primitive vertex color is non-finite"));
        }
        output.vertices.push(MeshVertex {
            position,
            normal: normals[index],
            color,
            texture_data: [0.0; 3],
        });
    }

    output.indices.reserve(indices.len());
    for index in indices {
        output.indices.push(
            base_vertex
                .checked_add(index)
                .ok_or_else(|| invalid_data("model index exceeds u32::MAX"))?,
        );
    }
    Ok(())
}

fn triangulate(mode: Mode, indices: &[u32]) -> io::Result<Vec<u32>> {
    match mode {
        Mode::Triangles => {
            if indices.len() % 3 != 0 {
                return Err(invalid_data(
                    "triangle-list index count is not divisible by three",
                ));
            }
            Ok(indices.to_vec())
        }
        Mode::TriangleStrip => {
            let mut triangles = Vec::with_capacity(indices.len().saturating_sub(2) * 3);
            for index in 0..indices.len().saturating_sub(2) {
                if index % 2 == 0 {
                    triangles.extend_from_slice(&[
                        indices[index],
                        indices[index + 1],
                        indices[index + 2],
                    ]);
                } else {
                    triangles.extend_from_slice(&[
                        indices[index + 1],
                        indices[index],
                        indices[index + 2],
                    ]);
                }
            }
            Ok(triangles)
        }
        Mode::TriangleFan => {
            let mut triangles = Vec::with_capacity(indices.len().saturating_sub(2) * 3);
            for index in 1..indices.len().saturating_sub(1) {
                triangles.extend_from_slice(&[indices[0], indices[index], indices[index + 1]]);
            }
            Ok(triangles)
        }
        mode => Err(invalid_data(format!(
            "primitive mode {mode:?} is not a triangle mode"
        ))),
    }
}

fn generate_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for triangle in indices.chunks_exact(3) {
        let a = Vec3::from_array(positions[triangle[0] as usize]);
        let b = Vec3::from_array(positions[triangle[1] as usize]);
        let c = Vec3::from_array(positions[triangle[2] as usize]);
        let face_normal = (b - a).cross(c - a);
        if face_normal.is_finite() && face_normal.length_squared() > 1.0e-12 {
            for index in triangle {
                normals[*index as usize] += face_normal;
            }
        }
    }
    normals
        .into_iter()
        .map(|normal| {
            if normal.length_squared() > 1.0e-12 {
                normal.normalize().to_array()
            } else {
                Vec3::Y.to_array()
            }
        })
        .collect()
}

fn invalid_data(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
