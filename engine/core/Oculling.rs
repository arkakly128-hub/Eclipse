use crate::frustum_culling::Aabb;
use crate::renderer::{blob_bytes, compile_shader, create_buffer, create_immutable_buffer};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::error::Error;
use std::io;
use std::mem::size_of;
use windows::core::{s, BOOL};
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;

const QUERY_RING_SIZE: usize = 3;
const QUERY_BOUNDS_MARGIN_M: f32 = 0.6;
const OCCLUSION_SHADER_SOURCE: &str = include_str!("../shader/Occlusion.hlsl");

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct OcclusionConstants {
    view_projection: [[f32; 4]; 4],
    center: [f32; 4],
    extent: [f32; 4],
}

struct QuerySlot {
    query: ID3D11Query,
    issued_frame: Option<u64>,
}

struct BatchVisibility {
    slots: Vec<QuerySlot>,
    next_slot: usize,
    visible: bool,
    draw_visible: bool,
    visibility_frame: Option<u64>,
}

impl BatchVisibility {
    fn new(device: &ID3D11Device) -> Result<Self, Box<dyn Error>> {
        let query_desc = D3D11_QUERY_DESC {
            Query: D3D11_QUERY_OCCLUSION,
            MiscFlags: 0,
        };
        let mut slots = Vec::with_capacity(QUERY_RING_SIZE);
        for _ in 0..QUERY_RING_SIZE {
            let mut query = None;
            unsafe {
                device.CreateQuery(&query_desc, Some(&mut query))?;
            }
            slots.push(QuerySlot {
                query: query
                    .ok_or_else(|| io::Error::other("D3D11 returned no occlusion query"))?,
                issued_frame: None,
            });
        }
        Ok(Self {
            slots,
            next_slot: 0,
            visible: true,
            draw_visible: true,
            visibility_frame: None,
        })
    }
}

pub struct OcclusionCuller {
    device: ID3D11Device,
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    input_layout: ID3D11InputLayout,
    vertex_buffer: ID3D11Buffer,
    index_buffer: ID3D11Buffer,
    constant_buffer: ID3D11Buffer,
    depth_state: ID3D11DepthStencilState,
    blend_state: ID3D11BlendState,
    rasterizer_state: ID3D11RasterizerState,
    batches: Vec<BatchVisibility>,
    frame_index: u64,
    camera_position: Vec3,
    camera_forward: Vec3,
    previous_camera: Option<(Vec3, Vec3)>,
    camera_compatible: bool,
}

impl OcclusionCuller {
    pub fn new(device: &ID3D11Device) -> Result<Self, Box<dyn Error>> {
        let vertex_shader_blob = compile_shader(
            OCCLUSION_SHADER_SOURCE,
            "Occlusion.hlsl",
            "OcclusionVS",
            "vs_5_0",
        )?;
        let pixel_shader_blob = compile_shader(
            OCCLUSION_SHADER_SOURCE,
            "Occlusion.hlsl",
            "OcclusionPS",
            "ps_5_0",
        )?;
        let mut vertex_shader = None;
        let mut pixel_shader = None;
        unsafe {
            device.CreateVertexShader(
                blob_bytes(&vertex_shader_blob),
                None::<&ID3D11ClassLinkage>,
                Some(&mut vertex_shader),
            )?;
            device.CreatePixelShader(
                blob_bytes(&pixel_shader_blob),
                None::<&ID3D11ClassLinkage>,
                Some(&mut pixel_shader),
            )?;
        }
        let input_elements = [D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("POSITION"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32B32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 0,
            InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
            InstanceDataStepRate: 0,
        }];
        let mut input_layout = None;
        unsafe {
            device.CreateInputLayout(
                &input_elements,
                blob_bytes(&vertex_shader_blob),
                Some(&mut input_layout),
            )?;
        }

        let vertices = [
            -1.0_f32, -1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 1.0, -1.0, -1.0, -1.0,
            1.0, 1.0, -1.0, 1.0, 1.0, 1.0, 1.0, -1.0, 1.0, 1.0,
        ];
        let indices = [
            0_u32, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 1, 2, 6, 1,
            6, 5, 0, 4, 7, 0, 7, 3,
        ];
        let vertex_buffer =
            create_immutable_buffer(device, &vertices, D3D11_BIND_VERTEX_BUFFER.0 as u32)?;
        let index_buffer =
            create_immutable_buffer(device, &indices, D3D11_BIND_INDEX_BUFFER.0 as u32)?;
        let constant_buffer = create_buffer(
            device,
            bytemuck::bytes_of(&OcclusionConstants::zeroed()),
            D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            D3D11_USAGE_DEFAULT,
        )?;

        let depth_desc = D3D11_DEPTH_STENCIL_DESC {
            DepthEnable: BOOL(1),
            DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO,
            DepthFunc: D3D11_COMPARISON_LESS_EQUAL,
            StencilEnable: BOOL(0),
            ..Default::default()
        };
        let mut depth_state = None;
        unsafe {
            device.CreateDepthStencilState(&depth_desc, Some(&mut depth_state))?;
        }

        let mut blend_desc = D3D11_BLEND_DESC::default();
        blend_desc.RenderTarget[0].RenderTargetWriteMask = 0;
        let mut blend_state = None;
        unsafe {
            device.CreateBlendState(&blend_desc, Some(&mut blend_state))?;
        }

        let rasterizer_desc = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            FrontCounterClockwise: BOOL(0),
            DepthBias: 0,
            DepthBiasClamp: 0.0,
            SlopeScaledDepthBias: 0.0,
            DepthClipEnable: BOOL(1),
            ScissorEnable: BOOL(0),
            MultisampleEnable: BOOL(1),
            AntialiasedLineEnable: BOOL(0),
        };
        let mut rasterizer_state = None;
        unsafe {
            device.CreateRasterizerState(&rasterizer_desc, Some(&mut rasterizer_state))?;
        }

        Ok(Self {
            device: device.clone(),
            vertex_shader: vertex_shader
                .ok_or_else(|| io::Error::other("D3D11 returned no occlusion vertex shader"))?,
            pixel_shader: pixel_shader
                .ok_or_else(|| io::Error::other("D3D11 returned no occlusion pixel shader"))?,
            input_layout: input_layout
                .ok_or_else(|| io::Error::other("D3D11 returned no occlusion input layout"))?,
            vertex_buffer,
            index_buffer,
            constant_buffer,
            depth_state: depth_state
                .ok_or_else(|| io::Error::other("D3D11 returned no occlusion depth state"))?,
            blend_state: blend_state
                .ok_or_else(|| io::Error::other("D3D11 returned no occlusion blend state"))?,
            rasterizer_state: rasterizer_state
                .ok_or_else(|| io::Error::other("D3D11 returned no occlusion rasterizer state"))?,
            batches: Vec::new(),
            frame_index: 0,
            camera_position: Vec3::ZERO,
            camera_forward: Vec3::Z,
            previous_camera: None,
            camera_compatible: false,
        })
    }

    pub fn reset_batches(&mut self, count: usize) -> Result<(), Box<dyn Error>> {
        let mut batches = Vec::with_capacity(count);
        for _ in 0..count {
            batches.push(BatchVisibility::new(&self.device)?);
        }
        self.batches = batches;
        Ok(())
    }

    pub fn begin_frame(
        &mut self,
        context: &ID3D11DeviceContext,
        camera_position: Vec3,
        camera_forward: Vec3,
    ) {
        self.frame_index = self.frame_index.wrapping_add(1);
        self.camera_position = camera_position;
        self.camera_forward = camera_forward.normalize_or_zero();
        self.camera_compatible = self
            .previous_camera
            .map(|(previous_position, previous_forward)| {
                previous_position.distance_squared(camera_position) <= 0.25 * 0.25
                    && previous_forward.dot(self.camera_forward) >= 0.995
            })
            .unwrap_or(false);

        let buffers = [Some(self.vertex_buffer.clone())];
        let strides = [size_of::<[f32; 3]>() as u32];
        let offsets = [0_u32];
        unsafe {
            context.IASetInputLayout(&self.input_layout);
            context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            context.IASetVertexBuffers(
                0,
                buffers.len() as u32,
                Some(buffers.as_ptr()),
                Some(strides.as_ptr()),
                Some(offsets.as_ptr()),
            );
            context.IASetIndexBuffer(&self.index_buffer, DXGI_FORMAT_R32_UINT, 0);
            context.VSSetShader(&self.vertex_shader, None);
            context.PSSetShader(&self.pixel_shader, None);
            context.VSSetConstantBuffers(0, Some(&[Some(self.constant_buffer.clone())]));
            context.OMSetDepthStencilState(Some(&self.depth_state), 0);
            context.OMSetBlendState(Some(&self.blend_state), None, u32::MAX);
            context.RSSetState(Some(&self.rasterizer_state));
        }
    }

    pub fn query_batch(
        &mut self,
        context: &ID3D11DeviceContext,
        batch_index: usize,
        bounds: Aabb,
        view_projection: Mat4,
    ) -> Result<bool, Box<dyn Error>> {
        let batch = self
            .batches
            .get_mut(batch_index)
            .ok_or_else(|| io::Error::other("tree instance batch has no occlusion query"))?;
        for slot in &mut batch.slots {
            let Some(issued_frame) = slot.issued_frame else {
                continue;
            };
            let mut samples = u64::MAX;
            unsafe {
                context.GetData(
                    &slot.query,
                    Some((&mut samples as *mut u64).cast()),
                    size_of::<u64>() as u32,
                    D3D11_ASYNC_GETDATA_DONOTFLUSH.0 as u32,
                )?;
            }
            if samples != u64::MAX {
                slot.issued_frame = None;
                if batch
                    .visibility_frame
                    .map(|latest_frame| issued_frame > latest_frame)
                    .unwrap_or(true)
                {
                    batch.visible = samples > 0;
                    batch.visibility_frame = Some(issued_frame);
                }
            }
        }

        let result_is_recent = batch
            .visibility_frame
            .map(|result_frame| self.frame_index.saturating_sub(result_frame) <= 1)
            .unwrap_or(false);
        batch.draw_visible = !self.camera_compatible || !result_is_recent || batch.visible;

        if let Some(slot_index) = (0..batch.slots.len())
            .map(|offset| (batch.next_slot + offset) % batch.slots.len())
            .find(|&index| batch.slots[index].issued_frame.is_none())
        {
            let slot = &mut batch.slots[slot_index];
            let expanded = bounds.expanded(QUERY_BOUNDS_MARGIN_M);
            let center = expanded.center();
            let extent = (expanded.max - expanded.min) * 0.5;
            let constants = OcclusionConstants {
                view_projection: view_projection.to_cols_array_2d(),
                center: [center.x, center.y, center.z, 1.0],
                extent: [extent.x, extent.y, extent.z, 0.0],
            };
            unsafe {
                context.UpdateSubresource(
                    &self.constant_buffer,
                    0,
                    None,
                    bytemuck::bytes_of(&constants).as_ptr().cast(),
                    0,
                    0,
                );
                context.Begin(&slot.query);
                context.DrawIndexed(36, 0, 0);
                context.End(&slot.query);
            }
            slot.issued_frame = Some(self.frame_index);
            batch.next_slot = (slot_index + 1) % batch.slots.len();
        }
        Ok(batch.draw_visible)
    }

    pub fn batch_visible(&self, batch_index: usize) -> Option<bool> {
        self.batches
            .get(batch_index)
            .map(|batch| batch.draw_visible)
    }

    pub fn end_frame(&mut self, context: &ID3D11DeviceContext) {
        unsafe {
            context.IASetInputLayout(None::<&ID3D11InputLayout>);
            context.VSSetShader(None, None);
            context.PSSetShader(None, None);
            context.OMSetBlendState(None, None, u32::MAX);
            context.OMSetDepthStencilState(None::<&ID3D11DepthStencilState>, 0);
            context.RSSetState(None::<&ID3D11RasterizerState>);
        }
        self.previous_camera = Some((self.camera_position, self.camera_forward));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn occlusion_proxy_shaders_compile() {
        compile_shader(
            OCCLUSION_SHADER_SOURCE,
            "Occlusion.hlsl",
            "OcclusionVS",
            "vs_5_0",
        )
        .expect("the occlusion proxy vertex shader should compile");
        compile_shader(
            OCCLUSION_SHADER_SOURCE,
            "Occlusion.hlsl",
            "OcclusionPS",
            "ps_5_0",
        )
        .expect("the occlusion proxy pixel shader should compile");
    }
}
