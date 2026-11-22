use std::ops::Range;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 3],
    pub texture_data: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct InstanceTransform {
    pub position_scale: [f32; 4],
}

#[derive(Clone, Debug, Default)]
pub struct MeshChunk {
    pub origin: [i64; 2],
    pub indices: Range<usize>,
}

#[derive(Debug, Default)]
pub struct ChunkedMesh {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
    pub chunks: Vec<MeshChunk>,
}

impl ChunkedMesh {
    pub fn with_capacity(vertex_count: usize, index_count: usize, chunk_count: usize) -> Self {
        Self {
            vertices: Vec::with_capacity(vertex_count),
            indices: Vec::with_capacity(index_count),
            chunks: Vec::with_capacity(chunk_count),
        }
    }
}
