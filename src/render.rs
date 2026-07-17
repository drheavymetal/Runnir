use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use unicode_width::UnicodeWidthChar;
use wgpu::util::DeviceExt;

use crate::font::{FontAtlas, Style};
use crate::grid::{Color, Flags, Grid};
use crate::selection::Selection;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    cell: [f32; 2],
    screen: [f32; 2],
    underline: [f32; 2],
    strike: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    /// Pixel top-left of the cell. Baked per instance so panes at different
    /// origins share one instance stream and one draw call.
    pos_px: [f32; 2],
    glyph_offset: [f32; 2],
    glyph_size: [f32; 2],
    uv_min: [f32; 2],
    uv_max: [f32; 2],
    fg: [f32; 4],
    bg: [f32; 4],
    flags: u32,
    width: f32,
    _pad: [u32; 2],
}

pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    atlas_tex: wgpu::Texture,
    instances: wgpu::Buffer,
    capacity: usize,
    theme: crate::config::Theme,
    pub font: FontAtlas,
}

/// One pane (or overlay panel) to draw: its grid, where it sits, and how.
pub struct PaneDraw<'a> {
    pub grid: &'a Grid,
    pub selection: Option<&'a Selection>,
    /// Pixel top-left of the pane.
    pub origin: (f32, f32),
    /// Background tint (ssh/root/docker), blended over the theme background.
    pub tint: Option<(u8, u8, u8)>,
    pub focused: bool,
}

/// An overlay: a dimmed backdrop plus one or more panels drawn on top.
pub struct Overlay<'a> {
    pub dim: f32,
    pub panels: Vec<PaneDraw<'a>>,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, font: FontAtlas) -> Self {
        let size = FontAtlas::atlas_size();
        let atlas_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph atlas"),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Srgb, not Unorm: emoji bitmaps are sRGB-encoded and must be decoded
            // to linear on sample, exactly like every other colour. Alpha is never
            // sRGB-encoded, so mask coverage passes through untouched.
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas_tex.create_view(&Default::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bind layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bind group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas_view),
                },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terminal shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("terminal pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2, 1 => Float32x2, 2 => Float32x2,
                        3 => Float32x2, 4 => Float32x2, 5 => Float32x4, 6 => Float32x4,
                        7 => Uint32, 8 => Float32
                    ],
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                // Alpha blending so a dimmed backdrop under an overlay composites
                // over the panes rather than replacing them.
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });

        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instances"),
            size: 0,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            bind_group,
            uniforms,
            atlas_tex,
            instances,
            capacity: 0,
            theme: crate::config::Theme::default(),
            font,
        }
    }

    pub fn set_theme(&mut self, theme: crate::config::Theme) {
        self.theme = theme;
    }

    /// Swaps in a new font atlas (a font-size change). The GPU atlas texture is
    /// re-uploaded on the next frame because the new atlas starts dirty.
    pub fn replace_font(&mut self, _device: &wgpu::Device, font: FontAtlas) {
        self.font = font;
        self.font.dirty = true;
    }

    pub fn cell_size(&self) -> (f32, f32) {
        (self.font.cell_w, self.font.cell_h)
    }

    /// Draws every pane, then every overlay, in one render pass and one draw call.
    /// Rebuilding the instance list each redraw is fine: with `ControlFlow::Wait`,
    /// redraws happen on real changes, not on a compositor clock.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        panes: &[PaneDraw],
        overlay: Option<&Overlay>,
        screen: (f32, f32),
    ) {
        // Build instances first: rasterizing a new glyph marks the atlas dirty, so
        // the upload must come after, or a glyph's first frame samples an empty
        // atlas and renders blank.
        let mut instances = Vec::new();
        for pane in panes {
            self.pane_instances(pane, &mut instances);
        }
        if let Some(ov) = overlay {
            // A dimming quad over the whole surface, then the overlay panels on top.
            instances.push(backdrop(screen, ov.dim));
            for panel in &ov.panels {
                self.pane_instances(panel, &mut instances);
            }
        }
        self.upload_atlas(queue);

        queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::bytes_of(&Uniforms {
                cell: [self.font.cell_w, self.font.cell_h],
                screen: [screen.0, screen.1],
                underline: [self.font.underline_y, self.font.stroke],
                strike: [self.font.strike_y, self.font.stroke],
            }),
        );

        let count = instances.len();
        if count > self.capacity {
            self.instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("instances"),
                contents: bytemuck::cast_slice(&instances),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            self.capacity = count;
        } else if count > 0 {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&instances));
        }

        let clear = self.theme.background;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("terminal"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(to_wgpu(clear, 1.0)),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..4, 0..count as u32);
    }

    /// Finds the ligatures on one row, keyed by the column each one starts at.
    ///
    /// Runs are maximal spans of printable ASCII sharing one pen. The cursor cell
    /// and any selected cell are excluded on purpose: a ligature under the cursor
    /// hides which character you are actually on, and one straddling a selection
    /// edge would paint the wrong background across half of itself.
    fn ligatures_for(
        &mut self,
        grid: &Grid,
        abs: usize,
        cursor_abs: usize,
        cursor_col: usize,
        selection: Option<&Selection>,
    ) -> HashMap<usize, Ligature> {
        let mut out = HashMap::new();
        if !self.font.ligatures {
            return out;
        }

        let joinable = |col: usize, cell: &crate::grid::Cell| {
            cell.ch.is_ascii_graphic()
                && !(abs == cursor_abs && col == cursor_col && grid.cursor_visible)
                && !selection.is_some_and(|s| s.contains(grid, (abs, col)))
        };

        let mut col = 0;
        while col < grid.cols() {
            let first = grid.abs_cell(abs, col);
            if !joinable(col, &first) {
                col += 1;
                continue;
            }
            let pen = first.pen;
            let start = col;
            let mut text = String::new();
            while col < grid.cols() {
                let cell = grid.abs_cell(abs, col);
                if cell.pen != pen || !joinable(col, &cell) {
                    break;
                }
                text.push(cell.ch);
                col += 1;
            }
            if text.len() < 2 {
                continue;
            }
            let style = Style::from_flags(pen.flags);
            let shaped = self.font.shape(&text, style);

            // A monospace face does not ligate by mapping N characters to one
            // glyph. It maps the leading characters to *blank* glyphs and the last
            // one to the full ligature, carrying a large negative left bearing so
            // it reaches back over them. That keeps the advance grid intact.
            //
            // So a ligature is: a run of blank glyphs followed by a real one.
            let mut i = 0;
            while i < shaped.len() {
                let blank = self.font.shaped_glyph(shaped[i].glyph_id, style).size[0] == 0.0;
                if !blank {
                    i += 1;
                    continue;
                }
                let mut j = i;
                while j < shaped.len()
                    && self.font.shaped_glyph(shaped[j].glyph_id, style).size[0] == 0.0
                {
                    j += 1;
                }
                if j >= shaped.len() {
                    break; // Trailing blanks with no glyph to anchor them.
                }
                let len = j - i + 1;
                out.insert(
                    start + i,
                    // `anchor` is how many cells right of the head the glyph's own
                    // cell sits; its bearing is measured from there.
                    Ligature::Head { glyph_id: shaped[j].glyph_id, len, anchor: j - i },
                );
                for k in 1..len {
                    out.insert(start + i + k, Ligature::Tail);
                }
                i = j + 1;
            }
        }
        out
    }

    fn pane_instances(&mut self, pane: &PaneDraw, out: &mut Vec<Instance>) {
        let grid = pane.grid;
        let (ox, oy) = pane.origin;
        let (cw, ch) = (self.font.cell_w, self.font.cell_h);
        let selection = pane.selection;

        // The cursor lives on the screen, which sits after the scrollback.
        let (cur_row, cur_col) = grid.cursor();
        let cursor_abs = grid.total_rows() - grid.rows() + cur_row;

        // Blend the context tint over the theme background for this pane only.
        let base_bg = match pane.tint {
            Some(t) => blend(self.theme.background, t, 0.5),
            None => self.theme.background,
        };
        // An unfocused pane dims so the eye finds the active one at a glance.
        let dim = if pane.focused { 1.0 } else { 0.62 };

        for row in 0..grid.rows() {
            let abs = grid.abs_row(row);
            let ligated = self.ligatures_for(grid, abs, cursor_abs, cur_col, selection);
            let y = oy + row as f32 * ch;

            for col in 0..grid.cols() {
                let cell = grid.abs_cell(abs, col);
                if cell.is_spacer() {
                    continue;
                }
                let mut fg = self.resolve(cell.pen.fg, base_bg, true);
                // A default background uses the pane's (possibly tinted) base; any
                // explicit colour resolves normally.
                let mut bg = match cell.pen.bg {
                    Color::Default => srgb(base_bg.0, base_bg.1, base_bg.2),
                    other => self.resolve(other, base_bg, false),
                };

                if cell.pen.flags.contains(Flags::REVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if cell.pen.flags.contains(Flags::DIM) {
                    fg = [fg[0] * 0.6, fg[1] * 0.6, fg[2] * 0.6, fg[3]];
                }
                if cell.pen.flags.contains(Flags::HIDDEN) {
                    fg = bg;
                }
                if selection.is_some_and(|s| s.contains(grid, (abs, col))) {
                    bg = srgb(self.theme.selection.0, self.theme.selection.1, self.theme.selection.2);
                }
                if grid.cursor_visible && abs == cursor_abs && col == cur_col && pane.focused {
                    std::mem::swap(&mut fg, &mut bg);
                }

                if dim < 1.0 {
                    for c in &mut fg[..3] {
                        *c *= dim;
                    }
                }

                let style = Style::from_flags(cell.pen.flags);
                let (glyph, span) = match ligated.get(&col) {
                    Some(&Ligature::Head { glyph_id, len, anchor }) => {
                        let mut g = self.font.shaped_glyph(glyph_id, style);
                        g.offset[0] += anchor as f32 * cw;
                        (g, len as f32)
                    }
                    Some(Ligature::Tail) => continue,
                    None => (
                        self.font.glyph(cell.ch, style),
                        cell.ch.width().unwrap_or(1).max(1) as f32,
                    ),
                };

                out.push(Instance {
                    pos_px: [ox + col as f32 * cw, y],
                    glyph_offset: glyph.offset,
                    glyph_size: glyph.size,
                    uv_min: glyph.uv_min,
                    uv_max: glyph.uv_max,
                    fg,
                    bg,
                    flags: cell.pen.flags.bits() as u32 | if glyph.color { FLAG_COLOR } else { 0 },
                    width: span,
                    _pad: [0; 2],
                });
            }
        }
    }

    fn upload_atlas(&mut self, queue: &wgpu::Queue) {
        if !self.font.dirty {
            return;
        }
        let size = FontAtlas::atlas_size();
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.atlas_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            self.font.pixels(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size * 4),
                rows_per_image: Some(size),
            },
            wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        );
        self.font.dirty = false;
    }
}

/// Renders `cmd`'s output to a PNG with no window involved. Verifying the GPU path
/// this way is deterministic and works headless — a screenshot of a live window is
/// neither.
fn shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
}

/// Renders an arbitrary set of grids into one image, for verifying multi-pane
/// layouts and overlays without a live window.
pub fn offscreen_scene(
    path: &str,
    width: u32,
    height: u32,
    font_px: f32,
    build: impl FnOnce(&Renderer) -> (Vec<(Grid, Rect, Option<(u8, u8, u8)>, bool)>, Option<Vec<(Grid, Rect)>>),
) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("no suitable GPU adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("runnir scene"),
        ..Default::default()
    }))
    .expect("device");

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let font = FontAtlas::new(font_px).expect("font");
    let mut renderer = Renderer::new(&device, format, font);

    let (pane_specs, overlay_specs) = build(&renderer);

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scene target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let padded_bpr = (width * 4).next_multiple_of(256);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded_bpr * height) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let panes: Vec<PaneDraw> = pane_specs
            .iter()
            .map(|(grid, rect, tint, focused)| PaneDraw {
                grid,
                selection: None,
                origin: (rect.x, rect.y),
                tint: *tint,
                focused: *focused,
            })
            .collect();
        let overlay = overlay_specs.as_ref().map(|panels| Overlay {
            dim: 0.55,
            panels: panels
                .iter()
                .map(|(g, r)| PaneDraw {
                    grid: g,
                    selection: None,
                    origin: (r.x, r.y),
                    tint: None,
                    focused: true,
                })
                .collect(),
        });
        renderer.render(&device, &queue, &mut encoder, &view, &panes, overlay.as_ref(), (width as f32, height as f32));
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bpr),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    queue.submit(Some(encoder.finish()));
    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
    let mapped = readback.slice(..).get_mapped_range().expect("map");
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height {
        let start = (row * padded_bpr) as usize;
        pixels.extend_from_slice(&mapped[start..start + (width * 4) as usize]);
    }
    drop(mapped);
    image::save_buffer(path, &pixels, width, height, image::ColorType::Rgba8).expect("png");
    eprintln!("runnir: wrote scene {path} ({width}x{height})");
}

/// The Rect type the scene builder uses, re-exported so callers need not reach
/// into the layout module.
pub use crate::layout::Rect;

pub fn offscreen(path: &str, cmd: &str, font_px: f32, delay_ms: Option<u64>) {
    use std::sync::{Arc, Mutex};

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("no suitable GPU adapter");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("runnir offscreen"),
        ..Default::default()
    }))
    .expect("failed to create device");

    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let font = FontAtlas::new(font_px).expect("font");
    let mut renderer = Renderer::new(&device, format, font);

    let (cols, rows) = (80usize, 24usize);
    let width = (cols as f32 * renderer.font.cell_w) as u32;
    let height = (rows as f32 * renderer.font.cell_h) as u32;

    let grid = Arc::new(Mutex::new(Grid::new(cols, rows)));
    let spawn = crate::pty::Spawn {
        command: Some(vec![shell(), "-c".into(), cmd.into()]),
        cwd: None,
    };
    let mut pty = crate::pty::Pty::spawn(grid.clone(), &spawn, || {}).expect("pty");
    match delay_ms {
        // Capture a full-screen app mid-flight: waiting for exit would only ever
        // show the primary screen it restores on the way out.
        Some(ms) => std::thread::sleep(std::time::Duration::from_millis(ms)),
        None => pty.wait(),
    }

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("offscreen target"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());

    // Texture-to-buffer copies demand rows padded to 256 bytes.
    let padded_bpr = (width * 4).next_multiple_of(256);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (padded_bpr * height) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let grid = grid.lock().unwrap();
        let panes = [PaneDraw {
            grid: &grid,
            selection: None,
            origin: (0.0, 0.0),
            tint: None,
            focused: true,
        }];
        renderer.render(
            &device,
            &queue,
            &mut encoder,
            &view,
            &panes,
            None,
            (width as f32, height as f32),
        );
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bpr),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    queue.submit(Some(encoder.finish()));

    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");

    let mapped = readback.slice(..).get_mapped_range().expect("map readback");
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for row in 0..height {
        let start = (row * padded_bpr) as usize;
        pixels.extend_from_slice(&mapped[start..start + (width * 4) as usize]);
    }
    drop(mapped);

    image::save_buffer(path, &pixels, width, height, image::ColorType::Rgba8).expect("png");
    eprintln!("runnir: wrote {path} ({width}x{height}, {cols}x{rows} cells)");
}

/// Sit above the `Flags` bits, which only reach 1<<6.
const FLAG_COLOR: u32 = 1 << 8;
const FLAG_FULLSCREEN: u32 = 1 << 9;

/// Where a ligature sits on a row: the cell that draws it, or one it swallows.
#[derive(Clone, Copy)]
enum Ligature {
    Head { glyph_id: u16, len: usize, anchor: usize },
    Tail,
}

impl Renderer {
    /// Resolves a cell colour to linear RGBA, taking the 16 ANSI colours and the
    /// defaults from the active theme. `base_bg` is the pane's own background,
    /// which a `Default` foreground never needs but a caller may pass anyway.
    fn resolve(&self, color: Color, base_bg: crate::config::Rgb, is_fg: bool) -> [f32; 4] {
        let t = &self.theme;
        let rgb = match color {
            Color::Default if is_fg => t.foreground,
            Color::Default => base_bg,
            Color::Rgb(r, g, b) => crate::config::Rgb(r, g, b),
            Color::Indexed(i) => xterm256(i, &t.ansi),
        };
        srgb(rgb.0, rgb.1, rgb.2)
    }
}

/// Converts an 8-bit sRGB channel to linear.
///
/// Everything — the xterm palette, SGR truecolour, the theme — is authored in
/// sRGB, but the surface format is `*UnormSrgb`, so the GPU encodes to sRGB on
/// write and expects linear from the shader. Skipping this step double-encodes and
/// washes every colour out.
fn to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn srgb(r: u8, g: u8, b: u8) -> [f32; 4] {
    [to_linear(r), to_linear(g), to_linear(b), 1.0]
}

fn to_wgpu(c: crate::config::Rgb, a: f64) -> wgpu::Color {
    wgpu::Color {
        r: to_linear(c.0) as f64,
        g: to_linear(c.1) as f64,
        b: to_linear(c.2) as f64,
        a,
    }
}

/// Mixes `over` (a raw tint) into `base` by `t`, in sRGB space. Only used for the
/// subtle context tint, where sRGB mixing is close enough and cheaper.
fn blend(base: crate::config::Rgb, over: (u8, u8, u8), t: f32) -> crate::config::Rgb {
    let mix = |a: u8, b: u8| (a as f32 * (1.0 - t) + b as f32 * t) as u8;
    crate::config::Rgb(mix(base.0, over.0), mix(base.1, over.1), mix(base.2, over.2))
}

/// A full-surface quad at `alpha`, black, to dim panes behind an overlay. Glyphless
/// (so only its background shows) and flagged fullscreen (so the vertex shader
/// spans the whole surface rather than one cell).
fn backdrop(_screen: (f32, f32), alpha: f32) -> Instance {
    Instance {
        pos_px: [0.0, 0.0],
        glyph_offset: [0.0, 0.0],
        glyph_size: [0.0, 0.0],
        uv_min: [0.0, 0.0],
        uv_max: [0.0, 0.0],
        fg: [0.0, 0.0, 0.0, alpha],
        bg: [0.0, 0.0, 0.0, alpha],
        flags: FLAG_FULLSCREEN,
        width: 1.0,
        _pad: [0; 2],
    }
}

/// The standard xterm 256-colour layout: 16 themeable ANSI colours, a 6x6x6 cube,
/// then 24 greys. The cube and greys are fixed by the spec; only 0-15 are themed.
fn xterm256(i: u8, ansi: &[crate::config::Rgb]) -> crate::config::Rgb {
    match i {
        0..=15 if (i as usize) < ansi.len() => ansi[i as usize],
        16..=231 => {
            let i = i - 16;
            let step = |v: u8| if v == 0 { 0 } else { v * 40 + 55 };
            crate::config::Rgb(step(i / 36), step((i / 6) % 6), step(i % 6))
        }
        232..=255 => {
            let v = (i - 232) * 10 + 8;
            crate::config::Rgb(v, v, v)
        }
        _ => crate::config::Rgb(0, 0, 0),
    }
}
