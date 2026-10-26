use crate::director::{Brush, FieldJob, Frame, MAX_COPIES, MAX_OBJECTS};
use crate::images::Gray;
use bytemuck::{Pod, Zeroable};
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::{Window, WindowId};

const BRUSH_MODE: u32 = 100;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    size: [u32; 2],
    origin: [u32; 2],
    mode: u32,
    wipe: u32,
    progress: f32,
    seed: f32,
    a: [f32; 4],
    b: [f32; 4],
    c: [f32; 4],
    shape: [f32; 4],
    shape2: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct View {
    size: [u32; 2],
    scale: u32,
    smoothing: u32,
    offset: f32,
    blend: f32,
    _pad: [f32; 2],
}

/// Device plus the pipelines every window shares.
pub struct Gpu {
    _instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    field_pipeline: wgpu::ComputePipeline,
    display_pipeline: wgpu::RenderPipeline,
}

impl Gpu {
    /// Picks an adapter that can present to `surface` and builds the pipelines for its format.
    pub fn new(instance: wgpu::Instance, surface: &wgpu::Surface) -> Gpu {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: Some(surface),
        }))
        .expect("no usable GPU adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("neopsych"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .expect("failed to open GPU device");

        // Prefer a non-sRGB surface so palette colours pass through untouched.
        let formats = surface.get_capabilities(&adapter).formats;
        let format = formats.iter().copied().find(|f| !f.is_srgb()).unwrap_or(formats[0]);

        let field_module = device.create_shader_module(wgpu::include_wgsl!("field.wgsl"));
        let field_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("field"),
            layout: None,
            module: &field_module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        let display_module = device.create_shader_module(wgpu::include_wgsl!("display.wgsl"));
        let display_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("display"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &display_module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &display_module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            multiview: None,
            cache: None,
        });

        Gpu { _instance: instance, adapter, device, queue, format, field_pipeline, display_pipeline }
    }
}

/// A uniform buffer and the bind groups pairing it with each pattern buffer.
/// Each dispatch in a frame needs its own, since queue writes land before the pass runs.
struct Slot {
    ubo: wgpu::Buffer,
    bg: [wgpu::BindGroup; 2],
}

/// One window's surface and pattern buffer.
pub struct Target {
    pub window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    scale: u32,
    smoothing: bool,
    dims: [u32; 2],
    view_ubo: wgpu::Buffer,
    palette: wgpu::Texture,
    sampler: wgpu::Sampler,
    /// Slot 0 is the background field, the rest one per object.
    slots: Vec<Slot>,
    /// Two pattern buffers: objects draw into the front one, and a new
    /// background goes into the other, which then fades in and becomes the front.
    phase: Vec<wgpu::Buffer>,
    front: usize,
    /// How far the front buffer has faded in, 0..1.
    blend: f32,
    /// Id of the background last drawn, to spot a new one arriving.
    field_id: Option<u32>,
    picture: wgpu::TextureView,
    picture_sampler: wgpu::Sampler,
    /// One per choice of front buffer; set by alloc, which always runs in new.
    display_bg: Vec<wgpu::BindGroup>,
    needs_full: bool,
}

impl Target {
    pub fn new(gpu: &Gpu, window: Arc<Window>, surface: wgpu::Surface<'static>, scale: u32, smoothing: bool) -> Target {
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .expect("surface not supported by adapter");
        config.format = gpu.format;
        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&gpu.device, &config);

        let view_ubo = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("view"),
            size: size_of::<View>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let palette_format =
            if gpu.format.is_srgb() { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm };
        let palette = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("palette"),
            size: wgpu::Extent3d { width: 256, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: palette_format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        // Repeat wraps the palette; linear filtering blends neighbouring entries.
        let filter = if smoothing { wgpu::FilterMode::Linear } else { wgpu::FilterMode::Nearest };
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            mag_filter: filter,
            min_filter: filter,
            ..Default::default()
        });

        let mut t = Target {
            window,
            surface,
            config,
            scale,
            smoothing,
            dims: [1, 1],
            view_ubo,
            palette,
            sampler,
            slots: Vec::new(),
            phase: Vec::new(),
            front: 0,
            blend: 1.0,
            field_id: None,
            picture: picture_texture(gpu, 1, 1, &[128]),
            picture_sampler: gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            display_bg: Vec::new(),
            needs_full: true,
        };
        t.alloc(gpu, scale);
        t
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    /// (Re)creates the pattern buffer for the current surface size. Raises the
    /// pixel scale if the buffer would exceed what the GPU can bind.
    fn alloc(&mut self, gpu: &Gpu, mut scale: u32) {
        let max = gpu.device.limits().max_storage_buffer_binding_size as u64;
        let dims = loop {
            let d = [self.config.width.div_ceil(scale), self.config.height.div_ceil(scale)];
            if d[0] as u64 * d[1] as u64 * 4 <= max {
                break d;
            }
            scale += 1;
        };
        self.phase = (0..2)
            .map(|_| {
                gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("phase"),
                    contents: &vec![0u8; dims[0] as usize * dims[1] as usize * 4],
                    usage: wgpu::BufferUsages::STORAGE,
                })
            })
            .collect();

        self.slots = (0..=MAX_OBJECTS * MAX_COPIES)
            .map(|_| {
                let ubo = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("params"),
                    size: size_of::<Params>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let bg = self.compute_bind_groups(gpu, &ubo);
                Slot { ubo, bg }
            })
            .collect();

        let palette_view = self.palette.create_view(&Default::default());
        self.display_bg = (0..2)
            .map(|front| {
                gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &gpu.display_pipeline.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: self.view_ubo.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: self.phase[front].as_entire_binding() },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&palette_view),
                        },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                        wgpu::BindGroupEntry { binding: 4, resource: self.phase[1 - front].as_entire_binding() },
                    ],
                })
            })
            .collect();
        self.blend = 1.0;
        self.dims = dims;
        self.scale = scale;
        self.needs_full = true;
    }

    /// Bind groups writing into pattern buffer 0 and 1 with this uniform buffer.
    fn compute_bind_groups(&self, gpu: &Gpu, ubo: &wgpu::Buffer) -> [wgpu::BindGroup; 2] {
        [0, 1].map(|k| {
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &gpu.field_pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: ubo.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.phase[k].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.picture) },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&self.picture_sampler),
                    },
                ],
            })
        })
    }

    /// Replaces the picture used by the image field.
    pub fn set_picture(&mut self, gpu: &Gpu, img: &Gray) {
        self.picture = picture_texture(gpu, img.width, img.height, &img.pixels);
        for i in 0..self.slots.len() {
            self.slots[i].bg = self.compute_bind_groups(gpu, &self.slots[i].ubo);
        }
    }

    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32, scale: u32) {
        if width == 0 || height == 0 || (width == self.config.width && height == self.config.height) {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&gpu.device, &self.config);
        self.alloc(gpu, scale);
    }

    pub fn render(&mut self, gpu: &Gpu, frame: &Frame) {
        let mut enc = gpu.device.create_command_encoder(&Default::default());

        // A new background is drawn once, into the back buffer, which then
        // becomes the front and fades in as the director's progress rises.
        // A full redraw (first frame, resize) goes straight to the front.
        let mut draw = None;
        if self.needs_full {
            draw = Some(frame.base);
            self.field_id = Some(frame.base.id);
            self.blend = 1.0;
        } else if let Some(f) = frame.field {
            if self.field_id != Some(f.id) {
                self.field_id = Some(f.id);
                self.front = 1 - self.front;
                draw = Some(f);
            }
            self.blend = f.progress;
        } else {
            self.blend = 1.0;
        }
        self.needs_full = false;
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&gpu.field_pipeline);
            if let Some(f) = draw {
                let slot = &self.slots[0];
                gpu.queue.write_buffer(&slot.ubo, 0, bytemuck::bytes_of(&self.field_params(&f)));
                pass.set_bind_group(0, &slot.bg[self.front], &[]);
                pass.dispatch_workgroups(self.dims[0].div_ceil(8), self.dims[1].div_ceil(8), 1);
            }
            let copies = self.symmetric_copies(frame);
            for (b, slot) in copies.iter().zip(&self.slots[1..]) {
                let Some((p, region)) = self.brush_params(b) else { continue };
                gpu.queue.write_buffer(&slot.ubo, 0, bytemuck::bytes_of(&p));
                pass.set_bind_group(0, &slot.bg[self.front], &[]);
                pass.dispatch_workgroups(region[0].div_ceil(8), region[1].div_ceil(8), 1);
            }
        }

        let view = View {
            size: self.dims,
            scale: self.scale,
            smoothing: self.smoothing as u32,
            offset: frame.offset,
            blend: self.blend,
            _pad: [0.0; 2],
        };
        gpu.queue.write_buffer(&self.view_ubo, 0, bytemuck::bytes_of(&view));
        gpu.queue.write_texture(
            self.palette.as_image_copy(),
            &frame.palette,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(1024), rows_per_image: None },
            wgpu::Extent3d { width: 256, height: 1, depth_or_array_layers: 1 },
        );

        let out = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(e) => {
                if matches!(e, wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) {
                    self.surface.configure(&gpu.device, &self.config);
                }
                gpu.queue.submit([enc.finish()]);
                return;
            }
        };
        let out_view = out.texture.create_view(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &out_view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&gpu.display_pipeline);
            pass.set_bind_group(0, &self.display_bg[self.front], &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([enc.finish()]);
        self.window.pre_present_notify();
        out.present();
    }

    fn field_params(&self, f: &FieldJob) -> Params {
        Params {
            size: self.dims,
            origin: [0, 0],
            mode: f.mode,
            wipe: 0,
            progress: f.progress,
            seed: f.seed,
            a: f.a,
            b: f.b,
            c: f.c,
            shape: [0.0; 4],
            shape2: [0.0; 4],
        }
    }

    /// Stretches brushes to the screen's aspect ratio, then adds their
    /// rotated and mirrored copies. Done here rather than in the director so
    /// rotations happen in true screen proportions.
    fn symmetric_copies(&self, frame: &Frame) -> Vec<Brush> {
        use std::f32::consts::{FRAC_PI_2, PI, TAU};
        let aspect = self.dims[0] as f32 / self.dims[1] as f32;
        let sym = frame.symmetry;
        let n = sym.turns.max(1);
        let rotate = |p: [f32; 2], a: f32| [a.cos() * p[0] - a.sin() * p[1], a.sin() * p[0] + a.cos() * p[1]];
        let mut out = Vec::with_capacity(frame.brushes.len() * sym.copies());
        for b in &frame.brushes {
            let p = [b.pos[0] * aspect, b.pos[1]];
            for k in 0..n {
                let a = TAU * k as f32 / n as f32;
                out.push(Brush { pos: rotate(p, a), angle: b.angle + a, ..*b });
                if sym.mirror {
                    // Reflect across the axis at angle phi: rotate the y-flipped point by 2 phi.
                    let phi = FRAC_PI_2 + PI * k as f32 / n as f32;
                    out.push(Brush { pos: rotate([p[0], -p[1]], 2.0 * phi), angle: 2.0 * phi - b.angle, ..*b });
                }
            }
        }
        out
    }

    /// Params plus the pixel region to dispatch over, or None if the stamp is
    /// off screen. `b.pos` is already in screen proportions.
    fn brush_params(&self, b: &Brush) -> Option<(Params, [u32; 2])> {
        let [w, h] = self.dims;
        let pos = b.pos;
        let extent = match b.shape {
            2 | 4 | 9 => b.radius * std::f32::consts::SQRT_2 + b.width,
            // A glyph cell around the head is at most 2 radii away.
            10 => b.radius * 2.5,
            _ => b.radius + b.width,
        };
        let half = h as f32 * 0.5;
        let cx = pos[0] * half + w as f32 * 0.5;
        let cy = pos[1] * half + h as f32 * 0.5;
        let e = extent * half + 2.0;
        let x0 = (cx - e).floor().clamp(0.0, w as f32) as u32;
        let y0 = (cy - e).floor().clamp(0.0, h as f32) as u32;
        let x1 = (cx + e).ceil().clamp(0.0, w as f32) as u32;
        let y1 = (cy + e).ceil().clamp(0.0, h as f32) as u32;
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        let p = Params {
            size: self.dims,
            origin: [x0, y0],
            mode: BRUSH_MODE,
            wipe: 0,
            progress: 1.0,
            seed: 0.0,
            a: [0.0; 4],
            b: [0.0; 4],
            c: [b.lobes, 0.0, 0.0, 0.0],
            shape: [pos[0], pos[1], b.radius, b.width],
            shape2: [b.shape as f32, b.color, b.angle, b.grad],
        };
        Some((p, [x1 - x0, y1 - y0]))
    }
}

/// A single-channel picture texture the compute shader can sample.
fn picture_texture(gpu: &Gpu, width: u32, height: u32, pixels: &[u8]) -> wgpu::TextureView {
    let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("picture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        pixels,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width), rows_per_image: None },
        size,
    );
    texture.create_view(&Default::default())
}
