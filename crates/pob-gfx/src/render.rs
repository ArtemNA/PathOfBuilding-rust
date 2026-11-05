//! `wgpu` renderer of a [`DrawList`]: one pipeline (position, uv, tint, viewport clip), standard alpha blending,
//! batches = runs of commands with the same texture in the sorted list.
//!
//! Like the original (GL ES on ANGLE, SG-DLL-FACTS §2) the frame is drawn **y-up** into an RGBA8 frame texture, then
//! either blitted (flipped) to the window surface — the original's second "Scaling" pass — or read back with the rows
//! reversed (oracle tests, `TakeScreenshot`). Drawing y-up reproduces the original's rasteriser tie rules (shader.wgsl).

use std::collections::HashMap;
use std::fmt;
use std::rc::{Rc, Weak};

use crate::draw::DrawList;
use crate::image::Texture;

/// The GPU API of each system, the ones `Cargo.toml` turns on (`WGPU_BACKEND` overrides).
const DEFAULT_BACKENDS: wgpu::Backends = if cfg!(windows) {
    wgpu::Backends::DX12
} else if cfg!(target_os = "macos") {
    wgpu::Backends::METAL
} else {
    wgpu::Backends::VULKAN.union(wgpu::Backends::GL)
};

/// Format of the frame texture the main pass draws into (non-sRGB: the original blends in gamma space).
const FRAME_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// The frame's clear colour (`SetClearColor`; opaque black by default — the Lua UI never sets it).
fn clear_color(list: &DrawList) -> wgpu::Color {
    let [r, g, b, a] = list.clear.map(f64::from);
    wgpu::Color { r, g, b, a }
}

#[derive(Debug)]
pub enum RenderError {
    NoAdapter(String),
    Device(String),
    Readback(String),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAdapter(e) => write!(f, "no wgpu adapter: {e}"),
            Self::Device(e) => write!(f, "wgpu device: {e}"),
            Self::Readback(e) => write!(f, "readback: {e}"),
        }
    }
}

impl std::error::Error for RenderError {}

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    tint: [f32; 4],
    clip: [f32; 4],
}

const VERTEX_SIZE: usize = std::mem::size_of::<Vertex>();

fn vertex_bytes(v: &[Vertex], out: &mut Vec<u8>) {
    out.clear();
    out.reserve(v.len() * VERTEX_SIZE);
    for x in v {
        for f in x.pos.iter().chain(&x.uv).chain(&x.tint).chain(&x.clip) {
            out.extend_from_slice(&f.to_ne_bytes());
        }
    }
}

struct GpuTexture {
    source: Weak<Texture>,
    bind: wgpu::BindGroup,
    _texture: wgpu::Texture,
}

/// GPU state shared by the window and offscreen paths.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    /// Format of the presentation target (the blit pass writes it).
    format: wgpu::TextureFormat,
    blit_pipeline: wgpu::RenderPipeline,
    blit_layout: wgpu::BindGroupLayout,
    /// The y-up frame texture of the current size (+ its view and the blit bind group).
    frame: Option<(wgpu::Texture, wgpu::TextureView, wgpu::BindGroup)>,
    tex_layout: wgpu::BindGroupLayout,
    screen_buf: wgpu::Buffer,
    screen_bind: wgpu::BindGroup,
    /// Index = clamp * 2 + nearest.
    samplers: [wgpu::Sampler; 4],
    textures: HashMap<u64, GpuTexture>,
    vbuf: Option<wgpu::Buffer>,
    ibuf: Option<wgpu::Buffer>,
    verts: Vec<Vertex>,
    bytes: Vec<u8>,
    adapter_info: wgpu::AdapterInfo,
    release_uploaded_pixels: bool,
}

impl Renderer {
    /// Creates an instance + adapter + device. `surface` (window path) constrains the adapter.
    pub fn new_instance() -> wgpu::Instance {
        wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::from_env().unwrap_or(DEFAULT_BACKENDS),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        })
    }

    /// The adapter (`POB_GFX_WARP=1` forces the software fallback, e.g. WARP on Windows).
    pub fn request_adapter(instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<wgpu::Adapter, RenderError> {
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::None,
            force_fallback_adapter: std::env::var_os("POB_GFX_WARP").is_some(),
            compatible_surface: surface,
            apply_limit_buckets: false,
        }))
        .map_err(|e| RenderError::NoAdapter(e.to_string()))
    }

    pub fn new(instance: &wgpu::Instance, surface: Option<&wgpu::Surface<'_>>, format: wgpu::TextureFormat) -> Result<Self, RenderError> {
        Self::with_adapter(&Self::request_adapter(instance, surface)?, format)
    }

    /// `format` = format of the presentation target (written by the blit pass).
    pub fn with_adapter(adapter: &wgpu::Adapter, format: wgpu::TextureFormat) -> Result<Self, RenderError> {
        Ok(Self::from_core(RendererCore::new(adapter)?, format))
    }

    /// The window's path: the core comes from its own thread, the format from the surface afterwards.
    pub fn from_core(core: RendererCore, format: wgpu::TextureFormat) -> Self {
        let blit_pipeline = core.blit_pipeline(format);
        let RendererCore {
            device,
            queue,
            pipeline,
            blit_shader: _,
            blit_pl: _,
            blit_layout,
            tex_layout,
            screen_buf,
            screen_bind,
            samplers,
            adapter_info,
        } = core;
        Self {
            device,
            queue,
            pipeline,
            format,
            blit_pipeline,
            blit_layout,
            frame: None,
            tex_layout,
            screen_buf,
            screen_bind,
            samplers,
            textures: HashMap::new(),
            vbuf: None,
            ibuf: None,
            verts: Vec::new(),
            bytes: Vec::new(),
            adapter_info,
            release_uploaded_pixels: false,
        }
    }

    /// Headless renderer for RGBA8 offscreen frames.
    pub fn new_offscreen() -> Result<Self, RenderError> {
        Self::new(&Self::new_instance(), None, FRAME_FORMAT)
    }

    /// The only renderer of the process: textures drop their CPU pixels once they are on the GPU.
    pub fn release_uploaded_pixels(&mut self) {
        self.release_uploaded_pixels = true;
    }

    /// Presents a surface texture rendered with [`Self::render`].
    pub fn present(&self, tex: wgpu::SurfaceTexture) {
        self.queue.present(tex);
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }
}

/// The slow part of a renderer (device, shaders, the main pipeline), free of `Rc`s: a thread can make it while the window's first frame prepares.
pub struct RendererCore {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    blit_shader: wgpu::ShaderModule,
    blit_pl: wgpu::PipelineLayout,
    blit_layout: wgpu::BindGroupLayout,
    tex_layout: wgpu::BindGroupLayout,
    screen_buf: wgpu::Buffer,
    screen_bind: wgpu::BindGroup,
    samplers: [wgpu::Sampler; 4],
    adapter_info: wgpu::AdapterInfo,
}

impl RendererCore {
    pub(crate) fn new(adapter: &wgpu::Adapter) -> Result<Self, RenderError> {
        let adapter_info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("pob-gfx"),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
            ..Default::default()
        }))
        .map_err(|e| RenderError::Device(e.to_string()))?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pob-gfx shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let screen_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("screen"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pob-gfx"),
            bind_group_layouts: &[Some(&screen_layout), Some(&tex_layout)],
            ..Default::default()
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4, 3 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pob-gfx"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: VERTEX_SIZE as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attrs,
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FRAME_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pob-gfx blit"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()),
        });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let blit_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit"),
            bind_group_layouts: &[Some(&blit_layout)],
            ..Default::default()
        });
        let screen_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let screen_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("screen"),
            layout: &screen_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buf.as_entire_binding(),
            }],
        });
        let sampler = |clamp: bool, nearest: bool| {
            let mode = if clamp {
                wgpu::AddressMode::ClampToEdge
            } else {
                wgpu::AddressMode::Repeat
            };
            let filter = if nearest {
                wgpu::FilterMode::Nearest
            } else {
                wgpu::FilterMode::Linear
            };
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("pob-gfx"),
                address_mode_u: mode,
                address_mode_v: mode,
                mag_filter: filter,
                min_filter: filter,
                mipmap_filter: if nearest {
                    wgpu::MipmapFilterMode::Nearest
                } else {
                    wgpu::MipmapFilterMode::Linear
                },
                // SimpleGraphic filters anisotropically, 16x, without mip levels (scene `u3a-aniso`, ratios 4..87: 0 pixels differ at 16, 647 at 8):
                // uniformly scaled quads look bilinear, stretched ones (connectors, tooltip header tiles) do not.
                anisotropy_clamp: if nearest { 1 } else { 16 },
                ..Default::default()
            })
        };
        let samplers = [
            sampler(false, false),
            sampler(false, true),
            sampler(true, false),
            sampler(true, true),
        ];
        Ok(RendererCore {
            device,
            queue,
            pipeline,
            blit_shader,
            blit_pl,
            blit_layout,
            tex_layout,
            screen_buf,
            screen_bind,
            samplers,
            adapter_info,
        })
    }

    fn blit_pipeline(&self, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: Some(&self.blit_pl),
            vertex: wgpu::VertexState {
                module: &self.blit_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &self.blit_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }
}

impl Renderer {
    fn upload(&mut self, t: &Rc<Texture>) {
        if self.textures.contains_key(&t.id()) {
            return;
        }
        let size = wgpu::Extent3d {
            width: t.width,
            height: t.height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pob-gfx image"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &t.pixels().expect("a texture without a GPU copy keeps its pixels"),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * t.width),
                rows_per_image: Some(t.height),
            },
            size,
        );
        if self.release_uploaded_pixels {
            t.release_pixels();
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = &self.samplers[usize::from(t.clamp) * 2 + usize::from(t.nearest)];
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pob-gfx image"),
            layout: &self.tex_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        self.textures.insert(
            t.id(),
            GpuTexture {
                source: Rc::downgrade(t),
                bind,
                _texture: texture,
            },
        );
    }

    fn ensure_buffer(device: &wgpu::Device, buf: &mut Option<wgpu::Buffer>, size: u64, usage: wgpu::BufferUsages) {
        if buf.as_ref().is_none_or(|b| b.size() < size) {
            *buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("pob-gfx"),
                size: size.next_power_of_two().max(4096),
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
    }

    fn ensure_frame(&mut self, w: u32, h: u32) {
        if self.frame.as_ref().is_some_and(|f| f.0.width() == w && f.0.height() == h) {
            return;
        }
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pob-gfx frame"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FRAME_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit"),
            layout: &self.blit_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        self.frame = Some((tex, view, bind));
    }

    /// Draws `list` into the window `target`: main pass into the y-up frame texture, then the flipping blit.
    pub fn render(&mut self, list: &DrawList, target: &wgpu::TextureView) {
        self.draw_frame(list);
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("blit") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.blit_pipeline);
            pass.set_bind_group(0, &self.frame.as_ref().expect("frame").2, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([enc.finish()]);
    }

    /// Main pass: `list` into the y-up frame texture of `list.size` (cleared first), submitted.
    fn draw_frame(&mut self, list: &DrawList) {
        self.ensure_frame(list.size.0.max(1), list.size.1.max(1));
        // GPU copies of textures whose last `Rc` is gone.
        self.textures.retain(|_, g| g.source.strong_count() > 0);
        let mut batches: Vec<(u64, u32, u32)> = Vec::new(); // texture id, first index, index count
        self.verts.clear();
        for c in &list.commands {
            self.upload(&c.texture);
            for k in 0..4 {
                self.verts.push(Vertex {
                    pos: c.pos[k],
                    uv: c.uv[k],
                    tint: c.tint,
                    clip: c.clip,
                });
            }
            match batches.last_mut() {
                Some(b) if b.0 == c.texture.id() => b.2 += 6,
                _ => {
                    let first = batches.last().map_or(0, |b| b.1 + b.2);
                    batches.push((c.texture.id(), first, 6));
                }
            }
        }
        let n = list.commands.len() as u32;
        let indices: Vec<u8> = (0..n)
            .flat_map(|q| [0, 1, 2, 0, 2, 3].map(|i| 4 * q + i))
            .flat_map(u32::to_ne_bytes)
            .collect();
        let size = [list.size.0 as f32, list.size.1 as f32, 0.0, 0.0];
        let size_bytes: Vec<u8> = size.iter().flat_map(|f| f.to_ne_bytes()).collect();
        self.queue.write_buffer(&self.screen_buf, 0, &size_bytes);
        if n > 0 {
            vertex_bytes(&self.verts, &mut self.bytes);
            Self::ensure_buffer(&self.device, &mut self.vbuf, self.bytes.len() as u64, wgpu::BufferUsages::VERTEX);
            Self::ensure_buffer(&self.device, &mut self.ibuf, indices.len() as u64, wgpu::BufferUsages::INDEX);
            self.queue.write_buffer(self.vbuf.as_ref().expect("vbuf"), 0, &self.bytes);
            self.queue.write_buffer(self.ibuf.as_ref().expect("ibuf"), 0, &indices);
        }
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("pob-gfx") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pob-gfx"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.frame.as_ref().expect("frame").1,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_color(list)),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            if n > 0 {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.screen_bind, &[]);
                pass.set_vertex_buffer(0, self.vbuf.as_ref().expect("vbuf").slice(..));
                pass.set_index_buffer(self.ibuf.as_ref().expect("ibuf").slice(..), wgpu::IndexFormat::Uint32);
                for &(id, first, count) in &batches {
                    pass.set_bind_group(1, &self.textures[&id].bind, &[]);
                    pass.draw_indexed(first..first + count, 0, 0..1);
                }
            }
        }
        self.queue.submit([enc.finish()]);
    }

    /// Renders `list` offscreen and returns tightly packed RGBA8 rows (`list.size`), top row first.
    pub fn render_to_rgba(&mut self, list: &DrawList) -> Result<Vec<u8>, RenderError> {
        self.draw_frame(list);
        let target = &self.frame.as_ref().expect("frame").0;
        // y-up frame: the last texture row is the top of the screen.
        self.read_texture(target, true)
    }

    /// Like the window path: main pass + flipping blit into a texture of the presentation format, read back as is
    /// (bytes in that format's channel order). For tests of the blit.
    pub fn render_blit_to_rgba(&mut self, list: &DrawList) -> Result<Vec<u8>, RenderError> {
        let target = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pob-gfx blit target"),
            size: wgpu::Extent3d {
                width: list.size.0,
                height: list.size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        self.render(list, &view);
        self.read_texture(&target, false)
    }

    /// Copies a 4-byte-per-texel texture to the CPU, rows tightly packed (`flip`: last row first).
    fn read_texture(&self, target: &wgpu::Texture, flip: bool) -> Result<Vec<u8>, RenderError> {
        let (w, h) = (target.width(), target.height());
        let size = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };
        let row = (4 * w).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pob-gfx readback"),
            size: u64::from(row) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            size,
        );
        self.queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        rx.recv()
            .map_err(|e| RenderError::Readback(e.to_string()))?
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        let data = slice.get_mapped_range().map_err(|e| RenderError::Readback(e.to_string()))?;
        let mut out = Vec::with_capacity((4 * w * h) as usize);
        let line = 4 * w as usize;
        for k in 0..h as usize {
            let y = if flip { h as usize - 1 - k } else { k };
            let start = y * row as usize;
            out.extend_from_slice(&data[start..start + line]);
        }
        drop(data);
        buf.unmap();
        Ok(out)
    }
}
