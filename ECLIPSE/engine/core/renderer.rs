use crate::asset_pack::AssetPack;
use crate::frustum_culling::{Aabb, Frustum};
use crate::instancing::{self, InstanceBatch};
use crate::mesh::{ChunkedMesh, InstanceTransform, MeshVertex};
use crate::occlusion_culling::OcclusionCuller;
use crate::window::Window;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::collections::VecDeque;
use std::error::Error;
use std::io::{self, Read, Seek};
use std::mem::size_of;
use windows::core::{s, w, BOOL, PCSTR};
use windows::Win32::Foundation::{COLORREF, HMODULE, RECT};
use windows::Win32::Graphics::Direct3D::Fxc::{
    D3DCompile, D3DCOMPILE_ENABLE_STRICTNESS, D3DCOMPILE_OPTIMIZATION_LEVEL3,
};
use windows::Win32::Graphics::Direct3D::{
    ID3DBlob, ID3DInclude, D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0,
    D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D_SRV_DIMENSION_TEXTURE2DMS,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DeleteDC, DeleteObject, DrawTextW,
    SelectObject, SetBkMode, SetTextColor, ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CENTER,
    DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_SEMIBOLD, HGDIOBJ, OUT_TT_PRECIS, TRANSPARENT,
};

const ENVIRONMENT_SHADER_SOURCE: &str = include_str!("../shader/Environment.hlsl");
const WORLD_SHADER_SOURCE: &str = include_str!("../shader/world.hlsl");
const WATER_SHADER_SOURCE: &str = include_str!("../shader/Water.hlsl");
const SKY_SHADER_SOURCE: &str = include_str!("../shader/Sky.hlsl");
const FOG_SHADER_SOURCE: &str = include_str!("../shader/Fog.hlsl");
const UI_SHADER_SOURCE: &str = include_str!("../shader/UI.hlsl");
const MAX_DRAW_DISTANCE: f32 = 180.0;
pub const MAX_UI_VERTICES: usize = 16384;
const MSAA_SAMPLE_COUNT: u32 = 2;
const WATER_RIPPLE_GRID_SIDE: u32 = 32;
const WATER_RIPPLE_GRID_CELLS: usize = (WATER_RIPPLE_GRID_SIDE * WATER_RIPPLE_GRID_SIDE) as usize;
const TERRAIN_MATERIALS: [(&str, &str); 6] = [
    ("grass_ground_1k", "grass_ground"),
    ("forest_ground_04_1k", "forest_ground_04"),
    ("brown_mud_02_1k", "brown_mud_02"),
    ("rocky_trail_1k", "rocky_trail"),
    ("sand_01_1k", "sand_01"),
    ("snow_01_1k", "snow_01"),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneLighting {
    pub sun_direction: Vec3,
    pub sun_color: [f32; 3],
    pub sun_intensity: f32,
    pub ambient_color: [f32; 3],
    pub moon_intensity: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct UiVertex {
    pub position: [f32; 2],
    pub texcoord: [f32; 2],
    pub color: [f32; 4],
    pub texture_slot: f32,
}

pub struct D3D11Renderer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    swap_chain: IDXGISwapChain,
    back_buffer: Option<ID3D11Texture2D>,
    scene_msaa_texture: Option<ID3D11Texture2D>,
    scene_msaa_target: Option<ID3D11RenderTargetView>,
    scene_color_texture: Option<ID3D11Texture2D>,
    scene_color_view: Option<ID3D11ShaderResourceView>,
    output_texture: Option<ID3D11Texture2D>,
    output_target: Option<ID3D11RenderTargetView>,
    depth_texture: Option<ID3D11Texture2D>,
    depth_view: Option<ID3D11DepthStencilView>,
    depth_shader_view: Option<ID3D11ShaderResourceView>,
    water_ripple_texture: ID3D11Texture2D,
    water_ripple_view: ID3D11ShaderResourceView,
    fog_sampler: ID3D11SamplerState,
    width: u32,
    height: u32,
    terrain: GpuMesh,
    water: Option<GpuMesh>,
    tree_mesh: GpuMesh,
    tree_local_bounds: Aabb,
    tree_instances: Option<ID3D11Buffer>,
    identity_instance: ID3D11Buffer,
    scene_constants: ID3D11Buffer,
    scene_vertex_shader: ID3D11VertexShader,
    scene_pixel_shader: ID3D11PixelShader,
    world_vertex_shader: ID3D11VertexShader,
    world_pixel_shader: ID3D11PixelShader,
    water_vertex_shader: ID3D11VertexShader,
    water_pixel_shader: ID3D11PixelShader,
    sky_vertex_shader: ID3D11VertexShader,
    sky_pixel_shader: ID3D11PixelShader,
    fog_vertex_shader: ID3D11VertexShader,
    fog_pixel_shader: ID3D11PixelShader,
    input_layout: ID3D11InputLayout,
    ui_input_layout: ID3D11InputLayout,
    ui_vertex_buffer: ID3D11Buffer,
    ui_vertex_shader: ID3D11VertexShader,
    ui_pixel_shader: ID3D11PixelShader,
    menu_texture_view: ID3D11ShaderResourceView,
    button_texture_view: ID3D11ShaderResourceView,
    inventory_texture_view: ID3D11ShaderResourceView,
    terrain_albedo_view: ID3D11ShaderResourceView,
    terrain_arm_view: ID3D11ShaderResourceView,
    terrain_ao_view: ID3D11ShaderResourceView,
    water_base_color_view: ID3D11ShaderResourceView,
    water_normal_view: ID3D11ShaderResourceView,
    water_roughness_view: ID3D11ShaderResourceView,
    water_ao_view: ID3D11ShaderResourceView,
    slot_texture_view: ID3D11ShaderResourceView,
    red_heart_texture_view: ID3D11ShaderResourceView,
    black_heart_texture_view: ID3D11ShaderResourceView,
    font_texture_view: ID3D11ShaderResourceView,
    ui_sampler: ID3D11SamplerState,
    texture_samplers: [Option<ID3D11SamplerState>; D3D11_COMMONSHADER_SAMPLER_SLOT_COUNT as usize],
    ui_blend_state: ID3D11BlendState,
    sky_depth_state: ID3D11DepthStencilState,
    terrain_rasterizer_state: ID3D11RasterizerState,
    ui_rasterizer_state: ID3D11RasterizerState,
    terrain_chunks: Vec<ChunkDraw>,
    water_chunks: Vec<ChunkDraw>,
    tree_batches: Vec<InstanceBatch>,
    occlusion_culler: OcclusionCuller,
    water_index_count: u32,
    tree_index_count: u32,
    tree_instance_count: u32,
}

struct GpuMesh {
    vertex_buffer: ID3D11Buffer,
    index_buffer: ID3D11Buffer,
}

enum TerrainTextureAssets {
    Pack(AssetPack),
    Folder(std::path::PathBuf),
}

struct WaterMaterialTextures {
    base_color: ID3D11ShaderResourceView,
    normal: ID3D11ShaderResourceView,
    roughness: ID3D11ShaderResourceView,
    ao: ID3D11ShaderResourceView,
}

#[derive(Clone, Copy)]
struct ChunkDraw {
    index_start: u32,
    index_count: u32,
    bounds: Aabb,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SceneConstants {
    view_projection: [[f32; 4]; 4],
    inverse_view_projection: [[f32; 4]; 4],
    camera_position: [f32; 4],
    sun_direction_intensity: [f32; 4],
    sun_color_time: [f32; 4],
    ambient_color: [f32; 4],
    ripple_grid: [f32; 4],
    environment_state: [f32; 4],
}

impl D3D11Renderer {
    pub fn new(
        window: &Window,
        terrain: &ChunkedMesh,
        water: &ChunkedMesh,
        tree_mesh: &ChunkedMesh,
        tree_instances: &[InstanceTransform],
        tree_batch_size_m: f32,
    ) -> Result<Self, Box<dyn Error>> {
        if terrain.vertices.is_empty() || terrain.indices.is_empty() || terrain.chunks.is_empty() {
            return Err(io::Error::other("terrain mesh is empty").into());
        }
        if tree_mesh.vertices.is_empty() || tree_mesh.indices.is_empty() {
            return Err(io::Error::other("tree mesh is empty").into());
        }

        let (width, height) = window.client_size();
        let width = width.max(1);
        let height = height.max(1);
        let swap_chain_desc = DXGI_SWAP_CHAIN_DESC {
            BufferDesc: DXGI_MODE_DESC {
                Width: width,
                Height: height,
                RefreshRate: DXGI_RATIONAL {
                    Numerator: 0,
                    Denominator: 1,
                },
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
                Scaling: DXGI_MODE_SCALING_UNSPECIFIED,
            },
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            OutputWindow: window.hwnd(),
            Windowed: BOOL(1),
            SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
            Flags: 0,
        };
        let feature_levels = [D3D_FEATURE_LEVEL_11_0];
        let mut swap_chain = None;
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDeviceAndSwapChain(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&swap_chain_desc),
                Some(&mut swap_chain),
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device =
            device.ok_or_else(|| io::Error::other("D3D11 device creation returned no device"))?;
        let context =
            context.ok_or_else(|| io::Error::other("D3D11 device creation returned no context"))?;
        let swap_chain = swap_chain
            .ok_or_else(|| io::Error::other("D3D11 device creation returned no swap chain"))?;
        let color_quality_levels = unsafe {
            device.CheckMultisampleQualityLevels(DXGI_FORMAT_R8G8B8A8_UNORM, MSAA_SAMPLE_COUNT)?
        };
        let depth_quality_levels = unsafe {
            device.CheckMultisampleQualityLevels(DXGI_FORMAT_D32_FLOAT, MSAA_SAMPLE_COUNT)?
        };
        if color_quality_levels == 0 || depth_quality_levels == 0 {
            return Err(io::Error::other(
                "the D3D11 device does not support 2x MSAA for scene color and depth",
            )
            .into());
        }

        let scene_vs = compile_shader(
            ENVIRONMENT_SHADER_SOURCE,
            "Environment.hlsl",
            "EnvironmentVS",
            "vs_5_0",
        )?;
        let scene_ps = compile_shader(
            ENVIRONMENT_SHADER_SOURCE,
            "Environment.hlsl",
            "EnvironmentPS",
            "ps_5_0",
        )?;
        let world_vs = compile_shader(WORLD_SHADER_SOURCE, "world.hlsl", "WorldVS", "vs_5_0")?;
        let world_ps = compile_shader(WORLD_SHADER_SOURCE, "world.hlsl", "WorldPS", "ps_5_0")?;
        let water_vs = compile_shader(WATER_SHADER_SOURCE, "Water.hlsl", "WaterVS", "vs_5_0")?;
        let water_ps = compile_shader(WATER_SHADER_SOURCE, "Water.hlsl", "WaterPS", "ps_5_0")?;
        let sky_vs = compile_shader(SKY_SHADER_SOURCE, "Sky.hlsl", "SkyVS", "vs_5_0")?;
        let sky_ps = compile_shader(SKY_SHADER_SOURCE, "Sky.hlsl", "SkyPS", "ps_5_0")?;
        let fog_vs = compile_shader(FOG_SHADER_SOURCE, "Fog.hlsl", "FogVS", "vs_5_0")?;
        let fog_ps = compile_shader(FOG_SHADER_SOURCE, "Fog.hlsl", "FogPS", "ps_5_0")?;
        let ui_vs = compile_shader(UI_SHADER_SOURCE, "UI.hlsl", "UiVS", "vs_5_0")?;
        let ui_ps = compile_shader(UI_SHADER_SOURCE, "UI.hlsl", "UiPS", "ps_5_0")?;
        let scene_vs_bytes = blob_bytes(&scene_vs);
        let scene_ps_bytes = blob_bytes(&scene_ps);
        let world_vs_bytes = blob_bytes(&world_vs);
        let world_ps_bytes = blob_bytes(&world_ps);
        let water_vs_bytes = blob_bytes(&water_vs);
        let water_ps_bytes = blob_bytes(&water_ps);
        let sky_vs_bytes = blob_bytes(&sky_vs);
        let sky_ps_bytes = blob_bytes(&sky_ps);
        let fog_vs_bytes = blob_bytes(&fog_vs);
        let fog_ps_bytes = blob_bytes(&fog_ps);
        let ui_vs_bytes = blob_bytes(&ui_vs);
        let ui_ps_bytes = blob_bytes(&ui_ps);

        let mut scene_vertex_shader = None;
        let mut scene_pixel_shader = None;
        let mut world_vertex_shader = None;
        let mut world_pixel_shader = None;
        let mut water_vertex_shader = None;
        let mut water_pixel_shader = None;
        let mut sky_vertex_shader = None;
        let mut sky_pixel_shader = None;
        let mut fog_vertex_shader = None;
        let mut fog_pixel_shader = None;
        let mut ui_vertex_shader = None;
        let mut ui_pixel_shader = None;
        unsafe {
            device.CreateVertexShader(
                scene_vs_bytes,
                None::<&ID3D11ClassLinkage>,
                Some(&mut scene_vertex_shader),
            )?;
            device.CreatePixelShader(
                scene_ps_bytes,
                None::<&ID3D11ClassLinkage>,
                Some(&mut scene_pixel_shader),
            )?;
            device.CreateVertexShader(
                world_vs_bytes,
                None::<&ID3D11ClassLinkage>,
                Some(&mut world_vertex_shader),
            )?;
            device.CreatePixelShader(
                world_ps_bytes,
                None::<&ID3D11ClassLinkage>,
                Some(&mut world_pixel_shader),
            )?;
            device.CreateVertexShader(
                water_vs_bytes,
                None::<&ID3D11ClassLinkage>,
                Some(&mut water_vertex_shader),
            )?;
            device.CreatePixelShader(
                water_ps_bytes,
                None::<&ID3D11ClassLinkage>,
                Some(&mut water_pixel_shader),
            )?;
            device.CreateVertexShader(
                sky_vs_bytes,
                None::<&ID3D11ClassLinkage>,
                Some(&mut sky_vertex_shader),
            )?;
            device.CreatePixelShader(
                sky_ps_bytes,
                None::<&ID3D11ClassLinkage>,
                Some(&mut sky_pixel_shader),
            )?;
            device.CreateVertexShader(
                fog_vs_bytes,
                None::<&ID3D11ClassLinkage>,
                Some(&mut fog_vertex_shader),
            )?;
            device.CreatePixelShader(
                fog_ps_bytes,
                None::<&ID3D11ClassLinkage>,
                Some(&mut fog_pixel_shader),
            )?;
            device.CreateVertexShader(
                ui_vs_bytes,
                None::<&ID3D11ClassLinkage>,
                Some(&mut ui_vertex_shader),
            )?;
            device.CreatePixelShader(
                ui_ps_bytes,
                None::<&ID3D11ClassLinkage>,
                Some(&mut ui_pixel_shader),
            )?;
        }

        let input_elements = [
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("POSITION"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 0,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("NORMAL"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 12,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("COLOR"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 24,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("TEXCOORD"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 36,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("INSTANCE_POSITION_SCALE"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 1,
                AlignedByteOffset: 0,
                InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
                InstanceDataStepRate: 1,
            },
        ];
        let mut input_layout = None;
        unsafe {
            device.CreateInputLayout(&input_elements, world_vs_bytes, Some(&mut input_layout))?;
        }
        let ui_input_elements = [
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("POSITION"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 0,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("TEXCOORD"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 8,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("COLOR"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 16,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("TEXCOORD"),
                SemanticIndex: 1,
                Format: DXGI_FORMAT_R32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 32,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
        ];
        let mut ui_input_layout = None;
        unsafe {
            device.CreateInputLayout(
                &ui_input_elements,
                ui_vs_bytes,
                Some(&mut ui_input_layout),
            )?;
        }

        let terrain_gpu = create_gpu_mesh(&device, terrain)?;
        let water_gpu = if water.indices.is_empty() {
            None
        } else {
            Some(create_gpu_mesh(&device, water)?)
        };
        let tree_gpu = create_gpu_mesh(&device, tree_mesh)?;
        let tree_local_bounds = mesh_bounds(tree_mesh)?;
        let tree_batch_data =
            instancing::build_batches(tree_instances, tree_local_bounds, tree_batch_size_m)?;
        let tree_instance_count = u32::try_from(tree_batch_data.instances.len())?;
        let ui_vertex_buffer = create_dynamic_buffer(
            &device,
            MAX_UI_VERTICES * size_of::<UiVertex>(),
            D3D11_BIND_VERTEX_BUFFER.0 as u32,
        )?;
        let asset_pack = find_icon_pack()?;
        let menu_texture_view = create_ui_asset_shader_resource(
            &device,
            &context,
            asset_pack.as_ref(),
            "main_bg.png",
            true,
        )?;
        let button_texture_view = create_ui_asset_shader_resource(
            &device,
            &context,
            asset_pack.as_ref(),
            "button.png",
            false,
        )?;
        let inventory_texture_view = create_ui_asset_shader_resource(
            &device,
            &context,
            asset_pack.as_ref(),
            "inventory.png",
            true,
        )?;
        let slot_texture_view = create_ui_asset_shader_resource(
            &device,
            &context,
            asset_pack.as_ref(),
            "slot.png",
            false,
        )?;
        let red_heart_texture_view = create_ui_asset_shader_resource(
            &device,
            &context,
            asset_pack.as_ref(),
            "RED HEART.png",
            false,
        )?;
        let black_heart_texture_view = create_ui_asset_shader_resource(
            &device,
            &context,
            asset_pack.as_ref(),
            "BLACK HEART.png",
            false,
        )?;
        let terrain_texture_assets = find_terrain_texture_assets()?;
        let (terrain_albedo_view, terrain_arm_view, terrain_ao_view) =
            create_terrain_material_textures(&device, &context, &terrain_texture_assets)?;
        let water_material_textures =
            create_water_material_textures(&device, &context, &terrain_texture_assets)?;
        let font_texture_view = create_font_shader_resource(&device)?;
        let mut ui_sampler = None;
        let ui_sampler_desc = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
            MipLODBias: 0.0,
            MaxAnisotropy: 1,
            ComparisonFunc: D3D11_COMPARISON_ALWAYS,
            BorderColor: [0.0; 4],
            MinLOD: 0.0,
            MaxLOD: f32::MAX,
        };
        unsafe {
            device.CreateSamplerState(&ui_sampler_desc, Some(&mut ui_sampler))?;
        }
        let texture_sampler_desc = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_ANISOTROPIC,
            AddressU: D3D11_TEXTURE_ADDRESS_WRAP,
            AddressV: D3D11_TEXTURE_ADDRESS_WRAP,
            AddressW: D3D11_TEXTURE_ADDRESS_WRAP,
            MipLODBias: 0.0,
            MaxAnisotropy: 16,
            ComparisonFunc: D3D11_COMPARISON_ALWAYS,
            BorderColor: [0.0; 4],
            MinLOD: 0.0,
            MaxLOD: f32::MAX,
        };
        let mut texture_sampler = None;
        unsafe {
            device.CreateSamplerState(&texture_sampler_desc, Some(&mut texture_sampler))?;
        }
        let texture_sampler = texture_sampler
            .ok_or_else(|| io::Error::other("D3D11 returned no anisotropic texture sampler"))?;
        let texture_samplers = std::array::from_fn(|_| Some(texture_sampler.clone()));
        let mut ui_blend_desc = D3D11_BLEND_DESC::default();
        ui_blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: BOOL(1),
            SrcBlend: D3D11_BLEND_SRC_ALPHA,
            DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };
        let mut ui_blend_state = None;
        unsafe {
            device.CreateBlendState(&ui_blend_desc, Some(&mut ui_blend_state))?;
        }
        let identity_instance = [InstanceTransform {
            position_scale: [0.0, 0.0, 0.0, 1.0],
        }];
        let identity_instance_buffer = create_immutable_buffer(
            &device,
            &identity_instance,
            D3D11_BIND_VERTEX_BUFFER.0 as u32,
        )?;
        let tree_instance_buffer = if tree_instances.is_empty() {
            None
        } else {
            Some(create_immutable_buffer(
                &device,
                &tree_batch_data.instances,
                D3D11_BIND_VERTEX_BUFFER.0 as u32,
            )?)
        };
        let constant_data = SceneConstants::zeroed();
        let scene_constants = create_buffer(
            &device,
            bytemuck::bytes_of(&constant_data),
            D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            D3D11_USAGE_DEFAULT,
        )?;
        let ripple_texture_desc = D3D11_TEXTURE2D_DESC {
            Width: WATER_RIPPLE_GRID_SIDE,
            Height: WATER_RIPPLE_GRID_SIDE,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R32_FLOAT,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut water_ripple_texture = None;
        unsafe {
            device.CreateTexture2D(&ripple_texture_desc, None, Some(&mut water_ripple_texture))?;
        }
        let water_ripple_texture = water_ripple_texture
            .ok_or_else(|| io::Error::other("D3D11 returned no water ripple texture"))?;
        let mut water_ripple_view = None;
        unsafe {
            device.CreateShaderResourceView(
                &water_ripple_texture,
                None,
                Some(&mut water_ripple_view),
            )?;
        }

        let mut sky_depth_state = None;
        let sky_depth_desc = D3D11_DEPTH_STENCIL_DESC {
            DepthEnable: BOOL(0),
            DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO,
            DepthFunc: D3D11_COMPARISON_LESS,
            StencilEnable: BOOL(0),
            ..Default::default()
        };
        unsafe {
            device.CreateDepthStencilState(&sky_depth_desc, Some(&mut sky_depth_state))?;
        }
        let mut terrain_rasterizer_state = None;
        let terrain_rasterizer_desc = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_BACK,
            FrontCounterClockwise: BOOL(1),
            DepthBias: 0,
            DepthBiasClamp: 0.0,
            SlopeScaledDepthBias: 0.0,
            DepthClipEnable: BOOL(1),
            ScissorEnable: BOOL(0),
            MultisampleEnable: BOOL(1),
            AntialiasedLineEnable: BOOL(0),
        };
        unsafe {
            device.CreateRasterizerState(
                &terrain_rasterizer_desc,
                Some(&mut terrain_rasterizer_state),
            )?;
        }
        let mut ui_rasterizer_state = None;
        let ui_rasterizer_desc = D3D11_RASTERIZER_DESC {
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
        unsafe {
            device.CreateRasterizerState(&ui_rasterizer_desc, Some(&mut ui_rasterizer_state))?;
        }
        let fog_sampler_desc = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
            MipLODBias: 0.0,
            MaxAnisotropy: 1,
            ComparisonFunc: D3D11_COMPARISON_ALWAYS,
            BorderColor: [0.0; 4],
            MinLOD: 0.0,
            MaxLOD: f32::MAX,
        };
        let mut fog_sampler = None;
        unsafe {
            device.CreateSamplerState(&fog_sampler_desc, Some(&mut fog_sampler))?;
        }

        let terrain_chunks = build_chunk_draws(terrain)?;
        let water_chunks = if water.indices.is_empty() {
            Vec::new()
        } else {
            build_chunk_draws(water)?
        };
        let water_index_count = u32::try_from(water.indices.len())?;
        let tree_index_count = u32::try_from(tree_mesh.indices.len())?;
        let mut occlusion_culler = OcclusionCuller::new(&device)?;
        occlusion_culler.reset_batches(tree_batch_data.batches.len())?;
        let mut renderer = Self {
            device,
            context,
            swap_chain,
            back_buffer: None,
            scene_msaa_texture: None,
            scene_msaa_target: None,
            scene_color_texture: None,
            scene_color_view: None,
            output_texture: None,
            output_target: None,
            depth_texture: None,
            depth_view: None,
            depth_shader_view: None,
            water_ripple_texture,
            water_ripple_view: water_ripple_view
                .ok_or_else(|| io::Error::other("D3D11 returned no water ripple view"))?,
            fog_sampler: fog_sampler
                .ok_or_else(|| io::Error::other("D3D11 returned no fog sampler"))?,
            width: 0,
            height: 0,
            terrain: terrain_gpu,
            water: water_gpu,
            tree_mesh: tree_gpu,
            tree_local_bounds,
            tree_instances: tree_instance_buffer,
            identity_instance: identity_instance_buffer,
            scene_constants,
            scene_vertex_shader: scene_vertex_shader
                .ok_or_else(|| io::Error::other("D3D11 returned no scene vertex shader"))?,
            scene_pixel_shader: scene_pixel_shader
                .ok_or_else(|| io::Error::other("D3D11 returned no scene pixel shader"))?,
            world_vertex_shader: world_vertex_shader
                .ok_or_else(|| io::Error::other("D3D11 returned no world vertex shader"))?,
            world_pixel_shader: world_pixel_shader
                .ok_or_else(|| io::Error::other("D3D11 returned no world pixel shader"))?,
            water_vertex_shader: water_vertex_shader
                .ok_or_else(|| io::Error::other("D3D11 returned no water vertex shader"))?,
            water_pixel_shader: water_pixel_shader
                .ok_or_else(|| io::Error::other("D3D11 returned no water pixel shader"))?,
            sky_vertex_shader: sky_vertex_shader
                .ok_or_else(|| io::Error::other("D3D11 returned no sky vertex shader"))?,
            sky_pixel_shader: sky_pixel_shader
                .ok_or_else(|| io::Error::other("D3D11 returned no sky pixel shader"))?,
            fog_vertex_shader: fog_vertex_shader
                .ok_or_else(|| io::Error::other("D3D11 returned no fog vertex shader"))?,
            fog_pixel_shader: fog_pixel_shader
                .ok_or_else(|| io::Error::other("D3D11 returned no fog pixel shader"))?,
            input_layout: input_layout
                .ok_or_else(|| io::Error::other("D3D11 returned no input layout"))?,
            ui_input_layout: ui_input_layout
                .ok_or_else(|| io::Error::other("D3D11 returned no UI input layout"))?,
            ui_vertex_buffer,
            ui_vertex_shader: ui_vertex_shader
                .ok_or_else(|| io::Error::other("D3D11 returned no UI vertex shader"))?,
            ui_pixel_shader: ui_pixel_shader
                .ok_or_else(|| io::Error::other("D3D11 returned no UI pixel shader"))?,
            menu_texture_view,
            button_texture_view,
            inventory_texture_view,
            terrain_albedo_view,
            terrain_arm_view,
            terrain_ao_view,
            water_base_color_view: water_material_textures.base_color,
            water_normal_view: water_material_textures.normal,
            water_roughness_view: water_material_textures.roughness,
            water_ao_view: water_material_textures.ao,
            slot_texture_view,
            red_heart_texture_view,
            black_heart_texture_view,
            font_texture_view,
            ui_sampler: ui_sampler
                .ok_or_else(|| io::Error::other("D3D11 returned no UI sampler"))?,
            texture_samplers,
            ui_blend_state: ui_blend_state
                .ok_or_else(|| io::Error::other("D3D11 returned no UI blend state"))?,
            sky_depth_state: sky_depth_state
                .ok_or_else(|| io::Error::other("D3D11 returned no sky depth state"))?,
            terrain_rasterizer_state: terrain_rasterizer_state
                .ok_or_else(|| io::Error::other("D3D11 returned no terrain rasterizer state"))?,
            ui_rasterizer_state: ui_rasterizer_state
                .ok_or_else(|| io::Error::other("D3D11 returned no UI rasterizer state"))?,
            terrain_chunks,
            water_chunks,
            tree_batches: tree_batch_data.batches,
            occlusion_culler,
            water_index_count,
            tree_index_count,
            tree_instance_count,
        };
        renderer.resize(width, height)?;
        Ok(renderer)
    }

    pub fn update_world_geometry(
        &mut self,
        terrain: &ChunkedMesh,
        water: &ChunkedMesh,
        tree_instances: &[InstanceTransform],
        tree_batch_size_m: f32,
    ) -> Result<(), Box<dyn Error>> {
        if terrain.vertices.is_empty() || terrain.indices.is_empty() || terrain.chunks.is_empty() {
            return Err(io::Error::other("streamed terrain mesh is empty").into());
        }

        let terrain_gpu = create_gpu_mesh(&self.device, terrain)?;
        let terrain_chunks = build_chunk_draws(terrain)?;
        let water_gpu = if water.indices.is_empty() {
            None
        } else {
            Some(create_gpu_mesh(&self.device, water)?)
        };
        let water_chunks = if water.indices.is_empty() {
            Vec::new()
        } else {
            build_chunk_draws(water)?
        };
        let tree_batch_data =
            instancing::build_batches(tree_instances, self.tree_local_bounds, tree_batch_size_m)?;
        let tree_instance_buffer = if tree_instances.is_empty() {
            None
        } else {
            Some(create_immutable_buffer(
                &self.device,
                &tree_batch_data.instances,
                D3D11_BIND_VERTEX_BUFFER.0 as u32,
            )?)
        };
        let tree_instance_count = u32::try_from(tree_batch_data.instances.len())?;
        self.occlusion_culler
            .reset_batches(tree_batch_data.batches.len())?;
        let water_index_count = u32::try_from(water.indices.len())?;

        self.terrain = terrain_gpu;
        self.water = water_gpu;
        self.terrain_chunks = terrain_chunks;
        self.water_chunks = water_chunks;
        self.water_index_count = water_index_count;
        self.tree_instances = tree_instance_buffer;
        self.tree_batches = tree_batch_data.batches;
        self.tree_instance_count = tree_instance_count;
        Ok(())
    }

    pub fn render(
        &mut self,
        window: &Window,
        view_projection: Mat4,
        camera_position: Vec3,
        camera_forward: Vec3,
        time_seconds: f32,
        underwater: bool,
        lighting: SceneLighting,
        ripple_origin: [f32; 2],
        ripple_heights: &[f32; WATER_RIPPLE_GRID_CELLS],
        ui_vertices: &[UiVertex],
    ) -> Result<(), Box<dyn Error>> {
        let (width, height) = window.client_size();
        if width == 0 || height == 0 {
            return Ok(());
        }
        if width != self.width || height != self.height {
            self.resize(width, height)?;
        }

        let inverse_view_projection = view_projection.inverse();
        let constants = SceneConstants {
            view_projection: view_projection.to_cols_array_2d(),
            inverse_view_projection: inverse_view_projection.to_cols_array_2d(),
            camera_position: [camera_position.x, camera_position.y, camera_position.z, 1.0],
            sun_direction_intensity: [
                lighting.sun_direction.x,
                lighting.sun_direction.y,
                lighting.sun_direction.z,
                lighting.sun_intensity,
            ],
            sun_color_time: [
                lighting.sun_color[0],
                lighting.sun_color[1],
                lighting.sun_color[2],
                time_seconds,
            ],
            ambient_color: [
                lighting.ambient_color[0],
                lighting.ambient_color[1],
                lighting.ambient_color[2],
                lighting.moon_intensity,
            ],
            ripple_grid: [
                ripple_origin[0],
                ripple_origin[1],
                1.0,
                WATER_RIPPLE_GRID_SIDE as f32,
            ],
            environment_state: [if underwater { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
        };
        let frustum = Frustum::from_view_projection(view_projection);
        let back_buffer = self
            .back_buffer
            .as_ref()
            .ok_or_else(|| io::Error::other("D3D11 back buffer is not initialized"))?;
        let output_texture = self
            .output_texture
            .as_ref()
            .ok_or_else(|| io::Error::other("D3D11 MSAA output texture is not initialized"))?;
        let output_target = self
            .output_target
            .as_ref()
            .ok_or_else(|| io::Error::other("D3D11 MSAA output target is not initialized"))?;
        let scene_msaa_texture = self
            .scene_msaa_texture
            .as_ref()
            .ok_or_else(|| io::Error::other("D3D11 MSAA scene texture is not initialized"))?;
        let scene_target = self
            .scene_msaa_target
            .as_ref()
            .ok_or_else(|| io::Error::other("D3D11 MSAA scene target is not initialized"))?;
        let scene_color_texture = self
            .scene_color_texture
            .as_ref()
            .ok_or_else(|| io::Error::other("D3D11 resolved scene texture is not initialized"))?;
        let scene_color_view = self
            .scene_color_view
            .as_ref()
            .ok_or_else(|| io::Error::other("D3D11 scene color view is not initialized"))?;
        let depth_view = self
            .depth_view
            .as_ref()
            .ok_or_else(|| io::Error::other("D3D11 depth view is not initialized"))?;
        let depth_shader_view = self
            .depth_shader_view
            .as_ref()
            .ok_or_else(|| io::Error::other("D3D11 depth shader view is not initialized"))?;

        unsafe {
            self.context.PSSetSamplers(0, Some(&self.texture_samplers));
            self.context.VSSetSamplers(0, Some(&self.texture_samplers));
            self.context.UpdateSubresource(
                &self.water_ripple_texture,
                0,
                None,
                ripple_heights.as_ptr().cast(),
                WATER_RIPPLE_GRID_SIDE * size_of::<f32>() as u32,
                0,
            );
            self.context.UpdateSubresource(
                &self.scene_constants,
                0,
                None,
                bytemuck::bytes_of(&constants).as_ptr().cast(),
                0,
                0,
            );
            self.context
                .OMSetRenderTargets(Some(&[Some(scene_target.clone())]), Some(depth_view));
            self.context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: self.width as f32,
                Height: self.height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            self.context
                .ClearRenderTargetView(scene_target, &[0.35, 0.55, 0.78, 1.0]);
            self.context
                .ClearDepthStencilView(depth_view, D3D11_CLEAR_DEPTH.0, 1.0, 0);
            self.context
                .VSSetConstantBuffers(0, Some(&[Some(self.scene_constants.clone())]));
            self.context
                .PSSetConstantBuffers(0, Some(&[Some(self.scene_constants.clone())]));
            self.context
                .OMSetDepthStencilState(Some(&self.sky_depth_state), 0);
            self.context.IASetInputLayout(None::<&ID3D11InputLayout>);
            self.context.VSSetShader(&self.sky_vertex_shader, None);
            self.context.PSSetShader(&self.sky_pixel_shader, None);
            self.context
                .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.context.Draw(3, 0);

            self.context
                .OMSetDepthStencilState(None::<&ID3D11DepthStencilState>, 0);
            self.context.IASetInputLayout(&self.input_layout);
            self.context.VSSetShader(&self.scene_vertex_shader, None);
            self.context.PSSetShader(&self.scene_pixel_shader, None);
            self.context.PSSetShaderResources(
                0,
                Some(&[
                    Some(self.terrain_albedo_view.clone()),
                    Some(self.terrain_arm_view.clone()),
                    Some(self.terrain_ao_view.clone()),
                ]),
            );
            self.context
                .IASetIndexBuffer(&self.terrain.index_buffer, DXGI_FORMAT_R32_UINT, 0);

            let terrain_buffers = [
                Some(self.terrain.vertex_buffer.clone()),
                Some(self.identity_instance.clone()),
            ];
            let strides = [
                size_of::<MeshVertex>() as u32,
                size_of::<InstanceTransform>() as u32,
            ];
            let offsets = [0_u32, 0_u32];
            self.context.IASetVertexBuffers(
                0,
                terrain_buffers.len() as u32,
                Some(terrain_buffers.as_ptr()),
                Some(strides.as_ptr()),
                Some(offsets.as_ptr()),
            );
            self.context
                .RSSetState(Some(&self.terrain_rasterizer_state));
            for chunk in &self.terrain_chunks {
                if chunk.bounds.distance_squared(camera_position)
                    <= MAX_DRAW_DISTANCE * MAX_DRAW_DISTANCE
                    && frustum.contains(chunk.bounds)
                {
                    self.context.DrawIndexedInstanced(
                        chunk.index_count,
                        1,
                        chunk.index_start,
                        0,
                        0,
                    );
                }
            }
            self.context.RSSetState(None::<&ID3D11RasterizerState>);
            self.context
                .PSSetShaderResources(0, Some(&[None, None, None]));

            if self.water_index_count > 0 {
                if let Some(water) = &self.water {
                    self.context
                        .IASetIndexBuffer(&water.index_buffer, DXGI_FORMAT_R32_UINT, 0);
                    let water_buffers = [
                        Some(water.vertex_buffer.clone()),
                        Some(self.identity_instance.clone()),
                    ];
                    self.context.IASetVertexBuffers(
                        0,
                        water_buffers.len() as u32,
                        Some(water_buffers.as_ptr()),
                        Some(strides.as_ptr()),
                        Some(offsets.as_ptr()),
                    );
                    self.context.VSSetShader(&self.water_vertex_shader, None);
                    self.context.PSSetShader(&self.water_pixel_shader, None);
                    self.context
                        .VSSetShaderResources(0, Some(&[Some(self.water_ripple_view.clone())]));
                    self.context.PSSetShaderResources(
                        0,
                        Some(&[
                            Some(self.water_normal_view.clone()),
                            Some(self.water_roughness_view.clone()),
                            Some(self.water_ao_view.clone()),
                            Some(self.water_base_color_view.clone()),
                        ]),
                    );
                    self.context
                        .VSSetSamplers(0, Some(&[Some(self.fog_sampler.clone())]));
                    self.context
                        .RSSetState(Some(&self.terrain_rasterizer_state));
                    for chunk in &self.water_chunks {
                        if chunk.bounds.distance_squared(camera_position)
                            <= MAX_DRAW_DISTANCE * MAX_DRAW_DISTANCE
                            && frustum.contains(chunk.bounds)
                        {
                            self.context.DrawIndexedInstanced(
                                chunk.index_count,
                                1,
                                chunk.index_start,
                                0,
                                0,
                            );
                        }
                    }
                    self.context.VSSetShaderResources(0, Some(&[None]));
                    self.context
                        .PSSetShaderResources(0, Some(&[None, None, None, None]));
                    self.context.VSSetSamplers(0, Some(&self.texture_samplers));
                    self.context.RSSetState(None::<&ID3D11RasterizerState>);
                }
            }

            if self.tree_instance_count > 0 {
                if let Some(tree_instances) = &self.tree_instances {
                    self.occlusion_culler.begin_frame(
                        &self.context,
                        camera_position,
                        camera_forward,
                    );
                    for (index, batch) in self.tree_batches.iter().enumerate() {
                        if batch.bounds.distance_squared(camera_position)
                            <= MAX_DRAW_DISTANCE * MAX_DRAW_DISTANCE
                            && frustum.contains(batch.bounds)
                        {
                            self.occlusion_culler.query_batch(
                                &self.context,
                                index,
                                batch.bounds,
                                view_projection,
                            )?;
                        }
                    }
                    self.occlusion_culler.end_frame(&self.context);
                    self.context
                        .VSSetConstantBuffers(0, Some(&[Some(self.scene_constants.clone())]));
                    self.context.IASetInputLayout(&self.input_layout);
                    self.context.IASetIndexBuffer(
                        &self.tree_mesh.index_buffer,
                        DXGI_FORMAT_R32_UINT,
                        0,
                    );
                    let tree_buffers = [
                        Some(self.tree_mesh.vertex_buffer.clone()),
                        Some(tree_instances.clone()),
                    ];
                    self.context.IASetVertexBuffers(
                        0,
                        tree_buffers.len() as u32,
                        Some(tree_buffers.as_ptr()),
                        Some(strides.as_ptr()),
                        Some(offsets.as_ptr()),
                    );
                    self.context.VSSetShader(&self.world_vertex_shader, None);
                    self.context.PSSetShader(&self.world_pixel_shader, None);
                    for (index, batch) in self.tree_batches.iter().enumerate() {
                        if batch.bounds.distance_squared(camera_position)
                            <= MAX_DRAW_DISTANCE * MAX_DRAW_DISTANCE
                            && frustum.contains(batch.bounds)
                            && self.occlusion_culler.batch_visible(index).ok_or_else(|| {
                                io::Error::other("tree batch has no occlusion visibility state")
                            })?
                        {
                            self.context.DrawIndexedInstanced(
                                self.tree_index_count,
                                batch.instance_count,
                                0,
                                0,
                                batch.first_instance,
                            );
                        }
                    }
                }
            }

            self.context.OMSetRenderTargets(None, None);
            // Fog samples single-sample color; the final UI remains multisampled until presentation.
            self.context.ResolveSubresource(
                scene_color_texture,
                0,
                scene_msaa_texture,
                0,
                DXGI_FORMAT_R8G8B8A8_UNORM,
            );
            self.context
                .OMSetRenderTargets(Some(&[Some(output_target.clone())]), None);
            self.context
                .OMSetDepthStencilState(Some(&self.sky_depth_state), 0);
            self.context.RSSetState(Some(&self.ui_rasterizer_state));
            self.context.IASetInputLayout(None::<&ID3D11InputLayout>);
            self.context.VSSetShader(&self.fog_vertex_shader, None);
            self.context.PSSetShader(&self.fog_pixel_shader, None);
            self.context
                .PSSetShaderResources(0, Some(&[Some(depth_shader_view.clone())]));
            self.context
                .PSSetShaderResources(1, Some(&[Some(scene_color_view.clone())]));
            self.context
                .PSSetSamplers(0, Some(&[Some(self.fog_sampler.clone())]));
            self.context.Draw(3, 0);
            self.context
                .PSSetShaderResources(0, Some(&[None, None, None, None, None, None, None]));

            self.draw_ui_vertices(ui_vertices)?;
            self.context.OMSetRenderTargets(None, None);
            self.context.ResolveSubresource(
                back_buffer,
                0,
                output_texture,
                0,
                DXGI_FORMAT_R8G8B8A8_UNORM,
            );
            self.swap_chain.Present(1, DXGI_PRESENT(0)).ok()?;
        }
        Ok(())
    }

    pub fn render_menu(
        &mut self,
        window: &Window,
        ui_vertices: &[UiVertex],
    ) -> Result<(), Box<dyn Error>> {
        let (width, height) = window.client_size();
        if width == 0 || height == 0 {
            return Ok(());
        }
        if width != self.width || height != self.height {
            self.resize(width, height)?;
        }

        let back_buffer = self
            .back_buffer
            .as_ref()
            .ok_or_else(|| io::Error::other("D3D11 back buffer is not initialized"))?;
        let output_texture = self
            .output_texture
            .as_ref()
            .ok_or_else(|| io::Error::other("D3D11 MSAA output texture is not initialized"))?;
        let output_target = self
            .output_target
            .as_ref()
            .ok_or_else(|| io::Error::other("D3D11 MSAA output target is not initialized"))?;
        unsafe {
            self.context
                .OMSetRenderTargets(Some(&[Some(output_target.clone())]), None);
            self.context.RSSetViewports(Some(&[D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: self.width as f32,
                Height: self.height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]));
            self.context
                .ClearRenderTargetView(output_target, &[0.025, 0.035, 0.055, 1.0]);
            self.context.RSSetState(Some(&self.ui_rasterizer_state));
        }
        self.draw_ui_vertices(ui_vertices)?;
        unsafe {
            self.context.OMSetRenderTargets(None, None);
            self.context.ResolveSubresource(
                back_buffer,
                0,
                output_texture,
                0,
                DXGI_FORMAT_R8G8B8A8_UNORM,
            );
            self.swap_chain.Present(1, DXGI_PRESENT(0)).ok()?;
        }
        Ok(())
    }

    fn draw_ui_vertices(&self, vertices: &[UiVertex]) -> Result<(), Box<dyn Error>> {
        if vertices.len() > MAX_UI_VERTICES {
            return Err(io::Error::other("UI frame exceeds the configured vertex capacity").into());
        }
        if vertices.len() % 3 != 0 {
            return Err(io::Error::other("UI geometry must contain complete triangles").into());
        }
        if vertices.is_empty() {
            return Ok(());
        }

        let vertex_count = u32::try_from(vertices.len())?;
        let vertex_data: &[u8] = bytemuck::cast_slice(vertices);
        unsafe {
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(
                &self.ui_vertex_buffer,
                0,
                D3D11_MAP_WRITE_DISCARD,
                0,
                Some(&mut mapped as *mut _),
            )?;
            if mapped.pData.is_null() {
                self.context.Unmap(&self.ui_vertex_buffer, 0);
                return Err(
                    io::Error::other("D3D11 mapped the UI buffer to a null pointer").into(),
                );
            }
            std::ptr::copy_nonoverlapping(
                vertex_data.as_ptr(),
                mapped.pData.cast::<u8>(),
                vertex_data.len(),
            );
            self.context.Unmap(&self.ui_vertex_buffer, 0);
            self.context
                .OMSetDepthStencilState(Some(&self.sky_depth_state), 0);
            self.context.RSSetState(Some(&self.ui_rasterizer_state));
            self.context.IASetInputLayout(&self.ui_input_layout);
            self.context
                .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            let buffers = [Some(self.ui_vertex_buffer.clone())];
            let strides = [size_of::<UiVertex>() as u32];
            let offsets = [0_u32];
            self.context.IASetVertexBuffers(
                0,
                1,
                Some(buffers.as_ptr()),
                Some(strides.as_ptr()),
                Some(offsets.as_ptr()),
            );
            self.context.VSSetShader(&self.ui_vertex_shader, None);
            self.context.PSSetShader(&self.ui_pixel_shader, None);
            self.context.PSSetShaderResources(
                0,
                Some(&[
                    Some(self.menu_texture_view.clone()),
                    Some(self.button_texture_view.clone()),
                    Some(self.inventory_texture_view.clone()),
                    Some(self.slot_texture_view.clone()),
                    Some(self.font_texture_view.clone()),
                    Some(self.red_heart_texture_view.clone()),
                    Some(self.black_heart_texture_view.clone()),
                ]),
            );
            self.context
                .PSSetSamplers(0, Some(&[Some(self.ui_sampler.clone())]));
            self.context
                .OMSetBlendState(Some(&self.ui_blend_state), None, u32::MAX);
            self.context.Draw(vertex_count, 0);
            self.context.OMSetBlendState(None, None, u32::MAX);
            self.context
                .PSSetShaderResources(0, Some(&[None, None, None, None, None, None, None]));
        }
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), Box<dyn Error>> {
        unsafe {
            self.context.OMSetRenderTargets(None, None);
            self.context
                .PSSetShaderResources(0, Some(&[None, None, None, None, None, None, None]));
        }
        self.back_buffer = None;
        self.scene_msaa_target = None;
        self.scene_msaa_texture = None;
        self.scene_color_view = None;
        self.scene_color_texture = None;
        self.output_target = None;
        self.output_texture = None;
        self.depth_view = None;
        self.depth_shader_view = None;
        self.depth_texture = None;
        unsafe {
            self.swap_chain.ResizeBuffers(
                0,
                width,
                height,
                DXGI_FORMAT_UNKNOWN,
                DXGI_SWAP_CHAIN_FLAG(0),
            )?;
        }

        let back_buffer: ID3D11Texture2D = unsafe { self.swap_chain.GetBuffer(0)? };
        self.back_buffer = Some(back_buffer);

        let scene_msaa_desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: MSAA_SAMPLE_COUNT,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut scene_msaa_texture = None;
        unsafe {
            self.device
                .CreateTexture2D(&scene_msaa_desc, None, Some(&mut scene_msaa_texture))?;
        }
        self.scene_msaa_texture = Some(
            scene_msaa_texture
                .ok_or_else(|| io::Error::other("D3D11 returned no multisampled scene texture"))?,
        );
        let mut scene_msaa_target = None;
        unsafe {
            self.device.CreateRenderTargetView(
                self.scene_msaa_texture.as_ref().ok_or_else(|| {
                    io::Error::other("D3D11 multisampled scene texture is not initialized")
                })?,
                None,
                Some(&mut scene_msaa_target),
            )?;
        }
        self.scene_msaa_target = Some(
            scene_msaa_target
                .ok_or_else(|| io::Error::other("D3D11 returned no multisampled scene target"))?,
        );

        let scene_color_desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32 | D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut scene_color_texture = None;
        unsafe {
            self.device
                .CreateTexture2D(&scene_color_desc, None, Some(&mut scene_color_texture))?;
        }
        self.scene_color_texture = Some(
            scene_color_texture
                .ok_or_else(|| io::Error::other("D3D11 returned no resolved scene texture"))?,
        );
        let mut scene_color_view = None;
        unsafe {
            self.device.CreateShaderResourceView(
                self.scene_color_texture.as_ref().ok_or_else(|| {
                    io::Error::other("D3D11 resolved scene texture is not initialized")
                })?,
                None,
                Some(&mut scene_color_view),
            )?;
        }
        self.scene_color_view = Some(
            scene_color_view
                .ok_or_else(|| io::Error::other("D3D11 returned no scene color view"))?,
        );

        let output_desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: MSAA_SAMPLE_COUNT,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut output_texture = None;
        unsafe {
            self.device
                .CreateTexture2D(&output_desc, None, Some(&mut output_texture))?;
        }
        self.output_texture =
            Some(output_texture.ok_or_else(|| {
                io::Error::other("D3D11 returned no multisampled output texture")
            })?);
        let mut output_target = None;
        unsafe {
            self.device.CreateRenderTargetView(
                self.output_texture.as_ref().ok_or_else(|| {
                    io::Error::other("D3D11 multisampled output texture is not initialized")
                })?,
                None,
                Some(&mut output_target),
            )?;
        }
        self.output_target = Some(
            output_target
                .ok_or_else(|| io::Error::other("D3D11 returned no multisampled output target"))?,
        );

        let depth_desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R32_TYPELESS,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: MSAA_SAMPLE_COUNT,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_DEPTH_STENCIL.0 as u32 | D3D11_BIND_SHADER_RESOURCE.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut depth_texture = None;
        unsafe {
            self.device
                .CreateTexture2D(&depth_desc, None, Some(&mut depth_texture))?;
        }
        self.depth_texture =
            Some(depth_texture.ok_or_else(|| io::Error::other("D3D11 returned no depth texture"))?);
        let depth_view_desc = D3D11_DEPTH_STENCIL_VIEW_DESC {
            Format: DXGI_FORMAT_D32_FLOAT,
            ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2DMS,
            Flags: 0,
            Anonymous: D3D11_DEPTH_STENCIL_VIEW_DESC_0 {
                Texture2DMS: D3D11_TEX2DMS_DSV {
                    UnusedField_NothingToDefine: 0,
                },
            },
        };
        let mut depth_view = None;
        unsafe {
            self.device.CreateDepthStencilView(
                self.depth_texture.as_ref().unwrap(),
                Some(&depth_view_desc),
                Some(&mut depth_view),
            )?;
        }
        self.depth_view =
            Some(depth_view.ok_or_else(|| io::Error::other("D3D11 returned no depth view"))?);
        let depth_shader_view_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_R32_FLOAT,
            ViewDimension: D3D_SRV_DIMENSION_TEXTURE2DMS,
            Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2DMS: D3D11_TEX2DMS_SRV {
                    UnusedField_NothingToDefine: 0,
                },
            },
        };
        let mut depth_shader_view = None;
        unsafe {
            self.device.CreateShaderResourceView(
                self.depth_texture.as_ref().unwrap(),
                Some(&depth_shader_view_desc),
                Some(&mut depth_shader_view),
            )?;
        }
        self.depth_shader_view = Some(
            depth_shader_view
                .ok_or_else(|| io::Error::other("D3D11 returned no depth shader view"))?,
        );
        self.width = width;
        self.height = height;
        Ok(())
    }
}

fn find_icon_pack() -> Result<Option<AssetPack>, Box<dyn Error>> {
    let executable = std::env::current_exe()?;
    let executable_dir = executable
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let packaged_assets = executable_dir
        .parent()
        .unwrap_or(executable_dir)
        .join("assets");
    let packaged = packaged_assets.join("icon.pak");
    if packaged.is_file() {
        return Ok(Some(AssetPack::open(packaged)?));
    }
    if packaged_assets.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("release asset pack is missing: {}", packaged.display()),
        )
        .into());
    }
    Ok(None)
}

fn find_ui_asset(file_name: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let source = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("game")
        .join("icons")
        .join(file_name);
    if source.is_file() {
        Ok(source)
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("UI asset not found: {}", source.display()),
        )
        .into())
    }
}

fn find_terrain_texture_assets() -> Result<TerrainTextureAssets, Box<dyn Error>> {
    let executable = std::env::current_exe()?;
    let executable_dir = executable
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let packaged_assets = executable_dir
        .parent()
        .unwrap_or(executable_dir)
        .join("assets");
    let packaged = packaged_assets.join("texture.pak");
    if packaged.is_file() {
        return Ok(TerrainTextureAssets::Pack(AssetPack::open(packaged)?));
    }

    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| io::Error::other("engine manifest directory has no workspace root"))?;
    let development_pack = workspace_root
        .join("release")
        .join("Eclipse")
        .join("assets")
        .join("texture.pak");
    if development_pack.is_file() {
        return Ok(TerrainTextureAssets::Pack(AssetPack::open(
            development_pack,
        )?));
    }

    let source_root = workspace_root.join("game").join("texture");
    if source_root.is_dir() {
        return Ok(TerrainTextureAssets::Folder(source_root));
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "terrain texture pack not found at {} and source textures are missing from {}",
            packaged.display(),
            source_root.display()
        ),
    )
    .into())
}

fn create_terrain_material_textures(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    assets: &TerrainTextureAssets,
) -> Result<
    (
        ID3D11ShaderResourceView,
        ID3D11ShaderResourceView,
        ID3D11ShaderResourceView,
    ),
    Box<dyn Error>,
> {
    let albedo = load_terrain_material_layers(assets, &TERRAIN_MATERIALS, "diff")?;
    let arm = load_terrain_material_layers(assets, &TERRAIN_MATERIALS, "arm")?;
    let ao = load_terrain_material_layers(assets, &TERRAIN_MATERIALS, "ao")?;
    Ok((
        create_texture_array_shader_resource(
            device,
            context,
            &albedo,
            DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        )?,
        create_texture_array_shader_resource(device, context, &arm, DXGI_FORMAT_R8G8B8A8_UNORM)?,
        create_texture_array_shader_resource(device, context, &ao, DXGI_FORMAT_R8G8B8A8_UNORM)?,
    ))
}

fn create_water_material_textures(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    assets: &TerrainTextureAssets,
) -> Result<WaterMaterialTextures, Box<dyn Error>> {
    let root = "water-ocean-normal-map-1k-pbr/water-ocean-normal-map-1k";
    let base_color = load_terrain_image(assets, &format!("{root}/baseColor.png"))?;
    let normal = load_terrain_image(assets, &format!("{root}/normal.png"))?;
    let roughness = load_terrain_image(assets, &format!("{root}/roughness.png"))?;
    let ao = load_terrain_image(assets, &format!("{root}/ambientOcclusion.png"))?;
    Ok(WaterMaterialTextures {
        base_color: create_texture_array_shader_resource(
            device,
            context,
            &[base_color],
            DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        )?,
        normal: create_texture_array_shader_resource(
            device,
            context,
            &[normal],
            DXGI_FORMAT_R8G8B8A8_UNORM,
        )?,
        roughness: create_texture_array_shader_resource(
            device,
            context,
            &[roughness],
            DXGI_FORMAT_R8G8B8A8_UNORM,
        )?,
        ao: create_texture_array_shader_resource(
            device,
            context,
            &[ao],
            DXGI_FORMAT_R8G8B8A8_UNORM,
        )?,
    })
}

fn load_terrain_material_layers(
    assets: &TerrainTextureAssets,
    materials: &[(&str, &str)],
    map: &str,
) -> Result<Vec<image::RgbaImage>, Box<dyn Error>> {
    materials
        .iter()
        .map(|(directory, name)| {
            let asset_path = format!("{directory}/textures/{name}_{map}_1k.jpg");
            load_terrain_image(assets, &asset_path)
        })
        .collect()
}

fn load_terrain_image(
    assets: &TerrainTextureAssets,
    asset_path: &str,
) -> Result<image::RgbaImage, Box<dyn Error>> {
    let bytes = match assets {
        TerrainTextureAssets::Pack(pack) => {
            let mut reader = pack.open_asset(asset_path)?;
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes)?;
            bytes
        }
        TerrainTextureAssets::Folder(root) => std::fs::read(root.join(asset_path))?,
    };
    image::load_from_memory(&bytes)
        .map(|image| image.into_rgba8())
        .map_err(|error| {
            io::Error::other(format!(
                "could not decode terrain material asset {asset_path}: {error}"
            ))
            .into()
        })
}

fn create_texture_array_shader_resource(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    layers: &[image::RgbaImage],
    format: DXGI_FORMAT,
) -> Result<ID3D11ShaderResourceView, Box<dyn Error>> {
    let first = layers
        .first()
        .ok_or_else(|| io::Error::other("terrain material texture array is empty"))?;
    let (width, height) = first.dimensions();
    if layers
        .iter()
        .any(|layer| layer.dimensions() != (width, height))
    {
        return Err(io::Error::other(
            "terrain material layers must have matching original dimensions",
        )
        .into());
    }

    let mip_levels = 32 - width.max(height).leading_zeros();
    let description = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: mip_levels,
        ArraySize: u32::try_from(layers.len())?,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32 | D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32,
    };
    let mut texture = None;
    unsafe {
        device.CreateTexture2D(&description, None, Some(&mut texture))?;
    }
    let texture =
        texture.ok_or_else(|| io::Error::other("D3D11 returned no terrain material texture"))?;
    for (slice, layer) in layers.iter().enumerate() {
        unsafe {
            context.UpdateSubresource(
                &texture,
                u32::try_from(slice)? * mip_levels,
                None,
                layer.as_raw().as_ptr().cast(),
                width * 4,
                0,
            );
        }
    }

    let mut view = None;
    unsafe {
        device.CreateShaderResourceView(&texture, None, Some(&mut view))?;
    }
    let view =
        view.ok_or_else(|| io::Error::other("D3D11 returned no terrain material texture view"))?;
    unsafe {
        context.GenerateMips(&view);
    }
    Ok(view)
}

fn create_ui_asset_shader_resource(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    asset_pack: Option<&AssetPack>,
    file_name: &str,
    generate_mips: bool,
) -> Result<ID3D11ShaderResourceView, Box<dyn Error>> {
    if let Some(asset_pack) = asset_pack {
        let reader = asset_pack.open_asset(file_name)?;
        create_png_shader_resource_from_reader(device, context, reader, file_name, generate_mips)
    } else {
        create_png_shader_resource_with_mips(
            device,
            context,
            find_ui_asset(file_name)?,
            generate_mips,
        )
    }
}

fn create_png_shader_resource_with_mips(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    path: std::path::PathBuf,
    generate_mips: bool,
) -> Result<ID3D11ShaderResourceView, Box<dyn Error>> {
    let file = std::fs::File::open(&path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot open {}: {error}", path.display()),
        )
    })?;
    create_png_shader_resource_from_reader(
        device,
        context,
        file,
        &path.display().to_string(),
        generate_mips,
    )
}

fn create_png_shader_resource_from_reader<R: Read + Seek>(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    reader: R,
    asset_path: &str,
    generate_mips: bool,
) -> Result<ID3D11ShaderResourceView, Box<dyn Error>> {
    let decoder = png::Decoder::new(reader);
    let mut reader = decoder.read_info()?;
    let mut source = vec![0_u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut source)?;
    if info.bit_depth != png::BitDepth::Eight {
        return Err(io::Error::other(format!("{asset_path} must be 8-bit PNG")).into());
    }
    let pixels: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => source,
        png::ColorType::Rgb => source
            .chunks_exact(3)
            .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
            .collect(),
        png::ColorType::Grayscale => source
            .iter()
            .flat_map(|&value| [value, value, value, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => source
            .chunks_exact(2)
            .flat_map(|gray| [gray[0], gray[0], gray[0], gray[1]])
            .collect(),
        png::ColorType::Indexed => {
            return Err(
                io::Error::other(format!("{asset_path} must not use an indexed palette")).into(),
            )
        }
    };
    let is_button = std::path::Path::new(asset_path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("button.png"))
        .unwrap_or(false);
    let mut keyed_pixels = pixels;
    if is_button {
        remove_connected_dark_background(&mut keyed_pixels, info.width, info.height);
    }

    let desc = D3D11_TEXTURE2D_DESC {
        Width: info.width,
        Height: info.height,
        MipLevels: if generate_mips { 0 } else { 1 },
        ArraySize: 1,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: if generate_mips {
            D3D11_USAGE_DEFAULT
        } else {
            D3D11_USAGE_IMMUTABLE
        },
        BindFlags: if generate_mips {
            D3D11_BIND_RENDER_TARGET.0 as u32 | D3D11_BIND_SHADER_RESOURCE.0 as u32
        } else {
            D3D11_BIND_SHADER_RESOURCE.0 as u32
        },
        CPUAccessFlags: 0,
        MiscFlags: if generate_mips {
            D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32
        } else {
            0
        },
    };
    let initial = D3D11_SUBRESOURCE_DATA {
        pSysMem: keyed_pixels.as_ptr().cast(),
        SysMemPitch: info.width * 4,
        SysMemSlicePitch: 0,
    };
    let mut texture = None;
    unsafe {
        device.CreateTexture2D(
            &desc,
            if generate_mips { None } else { Some(&initial) },
            Some(&mut texture),
        )?;
    }
    let texture = texture
        .ok_or_else(|| io::Error::other(format!("D3D11 returned no texture for {asset_path}")))?;
    if generate_mips {
        unsafe {
            context.UpdateSubresource(
                &texture,
                0,
                None,
                keyed_pixels.as_ptr().cast(),
                info.width * 4,
                0,
            );
        }
    }
    let mut view = None;
    unsafe {
        device.CreateShaderResourceView(&texture, None, Some(&mut view))?;
    }
    let view = view.ok_or_else(|| {
        io::Error::other(format!(
            "D3D11 returned no shader resource view for {asset_path}"
        ))
    })?;
    if generate_mips {
        unsafe {
            context.GenerateMips(&view);
        }
    }
    Ok(view)
}

fn create_font_shader_resource(
    device: &ID3D11Device,
) -> Result<ID3D11ShaderResourceView, Box<dyn Error>> {
    const CELL: i32 = 64;
    const COLUMNS: i32 = 16;
    const ROWS: i32 = 6;
    const WIDTH: u32 = (CELL * COLUMNS) as u32;
    const HEIGHT: u32 = (CELL * ROWS) as u32;

    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.0.is_null() {
        return Err(io::Error::last_os_error().into());
    }
    let font = unsafe {
        CreateFontW(
            -48,
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_TT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            w!("Segoe UI"),
        )
    };
    if font.0.is_null() {
        unsafe {
            let _ = DeleteDC(dc);
        }
        return Err(io::Error::last_os_error().into());
    }

    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: WIDTH as i32,
            biHeight: -(HEIGHT as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: WIDTH * HEIGHT * 4,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bitmap_bits = std::ptr::null_mut();
    let bitmap = match unsafe {
        CreateDIBSection(
            Some(dc),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut bitmap_bits,
            None,
            0,
        )
    } {
        Ok(bitmap) => bitmap,
        Err(error) => {
            unsafe {
                let _ = DeleteObject(HGDIOBJ(font.0));
                let _ = DeleteDC(dc);
            }
            return Err(error.into());
        }
    };
    if bitmap_bits.is_null() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteObject(HGDIOBJ(font.0));
            let _ = DeleteDC(dc);
        }
        return Err(io::Error::other("GDI returned an empty font bitmap").into());
    }

    let previous_bitmap = unsafe { SelectObject(dc, HGDIOBJ(bitmap.0)) };
    let previous_font = unsafe { SelectObject(dc, HGDIOBJ(font.0)) };
    let atlas_result = (|| -> Result<Vec<u8>, Box<dyn Error>> {
        let byte_count = WIDTH as usize * HEIGHT as usize * 4;
        let pixels =
            unsafe { std::slice::from_raw_parts_mut(bitmap_bits.cast::<u8>(), byte_count) };
        pixels.fill(0);
        unsafe {
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, COLORREF(0x00ff_ffff));
        }
        let flags = windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT(
            DT_CENTER.0 | DT_VCENTER.0 | DT_SINGLELINE.0,
        );
        for code in 32_u32..127 {
            let mut glyph = [code as u16];
            let column = ((code - 32) as i32) % COLUMNS;
            let row = ((code - 32) as i32) / COLUMNS;
            let mut bounds = RECT {
                left: column * CELL + 2,
                top: row * CELL + 2,
                right: (column + 1) * CELL - 2,
                bottom: (row + 1) * CELL - 2,
            };
            let drawn = unsafe { DrawTextW(dc, &mut glyph, &mut bounds, flags) };
            if drawn == 0 && code != 32 {
                return Err(io::Error::other(format!(
                    "GDI could not rasterize font glyph {}",
                    char::from_u32(code).unwrap_or('?')
                ))
                .into());
            }
        }

        for pixel in pixels.chunks_exact_mut(4) {
            let coverage = pixel[0].max(pixel[1]).max(pixel[2]);
            pixel[..3].fill(255);
            pixel[3] = coverage;
        }
        Ok(pixels.to_vec())
    })();

    unsafe {
        SelectObject(dc, previous_font);
        SelectObject(dc, previous_bitmap);
        let _ = DeleteObject(HGDIOBJ(font.0));
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(dc);
    }
    let pixels = atlas_result?;
    let desc = D3D11_TEXTURE2D_DESC {
        Width: WIDTH,
        Height: HEIGHT,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_IMMUTABLE,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let initial = D3D11_SUBRESOURCE_DATA {
        pSysMem: pixels.as_ptr().cast(),
        SysMemPitch: WIDTH * 4,
        SysMemSlicePitch: 0,
    };
    let mut texture = None;
    unsafe {
        device.CreateTexture2D(&desc, Some(&initial), Some(&mut texture))?;
    }
    let texture =
        texture.ok_or_else(|| io::Error::other("D3D11 returned no font atlas texture"))?;
    let mut view = None;
    unsafe {
        device.CreateShaderResourceView(&texture, None, Some(&mut view))?;
    }
    view.ok_or_else(|| io::Error::other("D3D11 returned no font atlas view").into())
}

fn remove_connected_dark_background(pixels: &mut [u8], width: u32, height: u32) {
    let width = width as usize;
    let height = height as usize;
    let pixel_count = width * height;
    if width == 0 || height == 0 || pixels.len() != pixel_count * 4 {
        return;
    }

    let mut visited = vec![false; pixel_count];
    let mut queue = VecDeque::new();

    for x in 0..width {
        enqueue_dark_pixel(pixels, &mut visited, &mut queue, x);
        enqueue_dark_pixel(pixels, &mut visited, &mut queue, (height - 1) * width + x);
    }
    for y in 0..height {
        enqueue_dark_pixel(pixels, &mut visited, &mut queue, y * width);
        enqueue_dark_pixel(pixels, &mut visited, &mut queue, y * width + width - 1);
    }

    while let Some(index) = queue.pop_front() {
        pixels[index * 4 + 3] = 0;
        let x = index % width;
        let y = index / width;
        if x > 0 {
            enqueue_dark_pixel(pixels, &mut visited, &mut queue, index - 1);
        }
        if x + 1 < width {
            enqueue_dark_pixel(pixels, &mut visited, &mut queue, index + 1);
        }
        if y > 0 {
            enqueue_dark_pixel(pixels, &mut visited, &mut queue, index - width);
        }
        if y + 1 < height {
            enqueue_dark_pixel(pixels, &mut visited, &mut queue, index + width);
        }
    }
}

fn enqueue_dark_pixel(
    pixels: &[u8],
    visited: &mut [bool],
    queue: &mut VecDeque<usize>,
    index: usize,
) {
    let offset = index * 4;
    if !visited[index]
        && pixels[offset]
            .max(pixels[offset + 1])
            .max(pixels[offset + 2])
            <= 56
    {
        visited[index] = true;
        queue.push_back(index);
    }
}

fn create_gpu_mesh(device: &ID3D11Device, mesh: &ChunkedMesh) -> Result<GpuMesh, Box<dyn Error>> {
    Ok(GpuMesh {
        vertex_buffer: create_immutable_buffer(
            device,
            &mesh.vertices,
            D3D11_BIND_VERTEX_BUFFER.0 as u32,
        )?,
        index_buffer: create_immutable_buffer(
            device,
            &mesh.indices,
            D3D11_BIND_INDEX_BUFFER.0 as u32,
        )?,
    })
}

pub(crate) fn create_immutable_buffer<T: Pod>(
    device: &ID3D11Device,
    values: &[T],
    bind_flags: u32,
) -> Result<ID3D11Buffer, Box<dyn Error>> {
    let bytes = bytemuck::cast_slice(values);
    create_buffer(device, bytes, bind_flags, D3D11_USAGE_IMMUTABLE)
}

pub(crate) fn create_buffer(
    device: &ID3D11Device,
    data: &[u8],
    bind_flags: u32,
    usage: D3D11_USAGE,
) -> Result<ID3D11Buffer, Box<dyn Error>> {
    if data.is_empty() {
        return Err(io::Error::other("cannot create an empty D3D11 buffer").into());
    }
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: u32::try_from(data.len())?,
        Usage: usage,
        BindFlags: bind_flags,
        CPUAccessFlags: 0,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let initial_data = D3D11_SUBRESOURCE_DATA {
        pSysMem: data.as_ptr().cast(),
        SysMemPitch: 0,
        SysMemSlicePitch: 0,
    };
    let mut buffer = None;
    unsafe {
        device.CreateBuffer(&desc, Some(&initial_data), Some(&mut buffer))?;
    }
    buffer.ok_or_else(|| io::Error::other("D3D11 returned no buffer").into())
}

fn create_dynamic_buffer(
    device: &ID3D11Device,
    byte_width: usize,
    bind_flags: u32,
) -> Result<ID3D11Buffer, Box<dyn Error>> {
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: u32::try_from(byte_width)?,
        Usage: D3D11_USAGE_DYNAMIC,
        BindFlags: bind_flags,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: 0,
        StructureByteStride: 0,
    };
    let mut buffer = None;
    unsafe {
        device.CreateBuffer(&desc, None, Some(&mut buffer))?;
    }
    buffer.ok_or_else(|| io::Error::other("D3D11 returned no dynamic buffer").into())
}

pub(crate) fn compile_shader(
    source: &str,
    source_label: &str,
    entry: &str,
    target: &str,
) -> Result<ID3DBlob, Box<dyn Error>> {
    let entry_name = entry;
    let entry = std::ffi::CString::new(entry)?;
    let target = std::ffi::CString::new(target)?;
    let source_name = std::ffi::CString::new(source_label)?;
    let entry = PCSTR(entry.as_ptr().cast());
    let target = PCSTR(target.as_ptr().cast());
    let source_name = PCSTR(source_name.as_ptr().cast());
    let mut bytecode = None;
    let mut diagnostics = None;
    let result = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            source_name,
            None,
            None::<&ID3DInclude>,
            entry,
            target,
            D3DCOMPILE_ENABLE_STRICTNESS | D3DCOMPILE_OPTIMIZATION_LEVEL3,
            0,
            &mut bytecode,
            Some(&mut diagnostics),
        )
    };
    if let Err(error) = result {
        let details = diagnostics.as_ref().map(blob_message).unwrap_or_default();
        return Err(io::Error::other(format!(
            "HLSL {entry_name} in {source_label} compilation failed: {error}; {details}"
        ))
        .into());
    }
    bytecode.ok_or_else(|| io::Error::other("D3DCompile returned no shader bytecode").into())
}

pub(crate) fn blob_bytes(blob: &ID3DBlob) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize())
    }
}

fn blob_message(blob: &ID3DBlob) -> String {
    String::from_utf8_lossy(blob_bytes(blob))
        .trim_end_matches('\0')
        .to_owned()
}

fn build_chunk_draws(mesh: &ChunkedMesh) -> Result<Vec<ChunkDraw>, Box<dyn Error>> {
    mesh.chunks
        .iter()
        .map(|chunk| {
            let indices = mesh
                .indices
                .get(chunk.indices.clone())
                .ok_or_else(|| io::Error::other("terrain chunk index range is invalid"))?;
            if indices.is_empty() {
                return Err(io::Error::other("terrain chunk contains no indices").into());
            }

            let mut bounds = Aabb::empty();
            for &index in indices {
                let vertex = mesh
                    .vertices
                    .get(index as usize)
                    .ok_or_else(|| io::Error::other("terrain index references no vertex"))?;
                let position = Vec3::from_array(vertex.position);
                bounds.include(position);
            }

            Ok(ChunkDraw {
                index_start: u32::try_from(chunk.indices.start)?,
                index_count: u32::try_from(indices.len())?,
                bounds,
            })
        })
        .collect()
}

fn mesh_bounds(mesh: &ChunkedMesh) -> Result<Aabb, Box<dyn Error>> {
    let mut bounds = Aabb::empty();
    for vertex in &mesh.vertices {
        let position = Vec3::from_array(vertex.position);
        if !position.is_finite() {
            return Err(io::Error::other("mesh contains a non-finite vertex position").into());
        }
        bounds.include(position);
    }
    if mesh.vertices.is_empty() {
        return Err(io::Error::other("cannot calculate bounds for an empty mesh").into());
    }
    Ok(bounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supplied_material_maps_decode_at_their_original_dimensions() {
        let assets = find_terrain_texture_assets().unwrap();
        for map in ["diff", "arm", "ao"] {
            let layers = load_terrain_material_layers(&assets, &TERRAIN_MATERIALS, map).unwrap();
            assert_eq!(layers.len(), TERRAIN_MATERIALS.len());
            assert!(layers
                .iter()
                .all(|layer| layer.dimensions() == (1024, 1024)));
        }
        let water_root = "water-ocean-normal-map-1k-pbr/water-ocean-normal-map-1k";
        for map in [
            "normal.png",
            "roughness.png",
            "ambientOcclusion.png",
            "baseColor.png",
        ] {
            let image = load_terrain_image(&assets, &format!("{water_root}/{map}")).unwrap();
            assert_eq!(image.dimensions(), (1024, 1024));
        }
    }

    #[test]
    fn terrain_material_shader_compiles_with_pbr_texture_arrays() {
        compile_shader(
            ENVIRONMENT_SHADER_SOURCE,
            "Environment.hlsl",
            "EnvironmentVS",
            "vs_5_0",
        )
        .expect("the terrain vertex shader should compile");
        compile_shader(
            ENVIRONMENT_SHADER_SOURCE,
            "Environment.hlsl",
            "EnvironmentPS",
            "ps_5_0",
        )
        .expect("the terrain PBR material shader should compile");
    }

    #[test]
    fn ui_shader_compiles_with_heart_textures() {
        compile_shader(UI_SHADER_SOURCE, "UI.hlsl", "UiVS", "vs_5_0")
            .expect("the UI vertex shader should compile");
        compile_shader(UI_SHADER_SOURCE, "UI.hlsl", "UiPS", "ps_5_0")
            .expect("the UI pixel shader should compile with heart textures");
    }

    #[test]
    fn fog_shaders_compile_for_the_fullscreen_msaa_composite() {
        compile_shader(FOG_SHADER_SOURCE, "Fog.hlsl", "FogVS", "vs_5_0")
            .expect("the fullscreen fog vertex shader should compile");
        compile_shader(FOG_SHADER_SOURCE, "Fog.hlsl", "FogPS", "ps_5_0")
            .expect("the MSAA fog pixel shader should compile");
    }

    #[test]
    fn water_shaders_compile_without_foam_inputs() {
        compile_shader(WATER_SHADER_SOURCE, "Water.hlsl", "WaterVS", "vs_5_0")
            .expect("the tessellated water vertex shader should compile");
        compile_shader(WATER_SHADER_SOURCE, "Water.hlsl", "WaterPS", "ps_5_0")
            .expect("the water pixel shader should compile");
    }

    #[test]
    fn button_keying_removes_only_outer_black_background() {
        let mut pixels = vec![0_u8; 5 * 5 * 4];
        for rgba in pixels.chunks_exact_mut(4) {
            rgba[3] = 255;
        }
        for y in 1..4 {
            for x in 1..4 {
                let offset = (y * 5 + x) * 4;
                pixels[offset..offset + 3].copy_from_slice(&[230, 160, 30]);
            }
        }
        let center = (2 * 5 + 2) * 4;
        pixels[center..center + 3].copy_from_slice(&[18, 12, 8]);

        remove_connected_dark_background(&mut pixels, 5, 5);

        assert_eq!(pixels[3], 0);
        assert_eq!(pixels[center + 3], 255);
        assert_eq!(&pixels[center..center + 3], &[18, 12, 8]);
        assert_eq!(pixels[(1 * 5 + 1) * 4 + 3], 255);
    }
}
