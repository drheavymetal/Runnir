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
    /// Underline shape code (see `UnderlineStyle::code`): 0 none, 1 single,
    /// 2 double, 3 curly, 4 dotted, 5 dashed.
    underline_style: u32,
    /// Underline colour, premultiply-ready linear rgba. Alpha 0 is the sentinel
    /// for "use the foreground colour"; the shader falls back to `fg` then.
    underline_color: [f32; 4],
}

pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniforms: wgpu::Buffer,
    atlas_tex: wgpu::Texture,
    instances: wgpu::Buffer,
    capacity: usize,
    theme: crate::config::Theme,
    images: ImageLayer,
    background: Background,
    /// Window opacity in 0.1..=1.0. Applied to the clear colour and to every
    /// default-background cell, so the compositor shows through (and can blur).
    opacity: f32,
    pub font: FontAtlas,
}

/// A vertex of an image quad: clip-space position and texture uv.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ImgVertex {
    pos: [f32; 2],
    uv: [f32; 2],
}

/// Draws inline images as textured quads, on their own pipeline. Textures are
/// cached by the grid's monotonic image serial, so an image is uploaded once.
struct ImageLayer {
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    layout: wgpu::BindGroupLayout,
    /// serial -> (texture, bind group). Kept across frames.
    cache: HashMap<u64, (wgpu::Texture, wgpu::BindGroup)>,
    vertices: wgpu::Buffer,
    vcap: usize,
    /// This frame's draw order: (serial, first vertex index).
    frame: Vec<(u64, u32)>,
}

/// A background image drawn behind the terminal (fullscreen, dimmed, cover-cropped).
struct Background {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    /// Bind group (texture + sampler + uniforms), set when an image is loaded.
    bind: Option<wgpu::BindGroup>,
    /// Image aspect (w/h) for cover-cropping to the surface.
    aspect: f32,
    dim: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BgUniforms {
    scale: [f32; 2],
    dim: f32,
    _pad: f32,
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
    /// Cursor style, or `None` to draw no cursor (unfocused, hidden, or blinked off).
    pub cursor: Option<crate::config::CursorShape>,
    /// Search matches to highlight: absolute `(row, col)` starts, the match length,
    /// and which one is current. Empty when not searching.
    pub search: SearchHighlight<'a>,
    /// Drop cells that are blank with a default background instead of drawing them.
    ///
    /// A grid drawn on top of a pane is otherwise opaque everywhere: a blank cell
    /// still emits an instance filled with the pane background, so an annotation
    /// layer covering the pane hides everything it was meant to annotate. Set for
    /// the hint layer, which is a handful of labels over live output.
    pub transparent: bool,
}

/// Search highlight data for a pane. Cheap to pass empty.
#[derive(Clone, Copy, Default)]
pub struct SearchHighlight<'a> {
    pub matches: &'a [(usize, usize)],
    pub len: usize,
    pub current: Option<(usize, usize)>,
}

/// An overlay: a dimmed backdrop plus one or more panels drawn on top.
pub struct Overlay<'a> {
    pub dim: f32,
    pub panels: Vec<PaneDraw<'a>>,
}

/// A solid-colour rectangle in pixels: pane borders and dividers.
#[derive(Clone, Copy)]
pub struct SolidRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: (u8, u8, u8),
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
                        7 => Uint32, 8 => Float32, 9 => Uint32, 10 => Float32x4
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
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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

        let images = ImageLayer::new(device, format);
        let background = Background::new(device, format);

        Self {
            pipeline,
            bind_group,
            images,
            background,
            uniforms,
            atlas_tex,
            instances,
            capacity: 0,
            theme: crate::config::Theme::default(),
            opacity: 1.0,
            font,
        }
    }

    /// Sets the window opacity (clamped 0.1..=1.0). 1.0 is fully opaque.
    pub fn set_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.1, 1.0);
    }

    /// Loads (or clears, with `None`) the background image drawn behind the terminal.
    pub fn set_background(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: Option<(&[u8], u32, u32)>,
        dim: f32,
    ) {
        self.background.set_image(device, queue, rgba, dim);
    }

    /// The inline images to draw this frame, from every pane, as (grid image,
    /// pixel rect). Uploads new textures and returns quads ready for the pass.
    fn prepare_images(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        panes: &[PaneDraw],
        screen: (f32, f32),
    ) -> usize {
        let (cw, ch) = (self.font.cell_w, self.font.cell_h);
        let mut quads: Vec<ImageQuad> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for pane in panes {
            let pane_rect = [
                pane.origin.0,
                pane.origin.1,
                pane.grid.cols() as f32 * cw,
                pane.grid.rows() as f32 * ch,
            ];
            for (img, row) in pane.grid.images() {
                let x = pane.origin.0;
                let y = pane.origin.1 + row as f32 * ch;
                let w = img.cols as f32 * cw;
                let h = img.rows as f32 * ch;
                // Clip to the pane: a partially scrolled image draws its visible
                // part only, instead of bleeding over neighbouring panes/chrome.
                let Some(quad) = clip_image_quad(img.serial, [x, y], [w, h], pane_rect) else {
                    continue;
                };
                self.images.upload(device, queue, &img);
                seen.insert(img.serial);
                quads.push(quad);
            }
        }
        self.images.retain(&seen);
        self.images.build_vertices(device, queue, &quads, screen)
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
        decorations: &[SolidRect],
        overlay: Option<&Overlay>,
        flash: f32,
        screen: (f32, f32),
    ) {
        // Build instances first: rasterizing a new glyph marks the atlas dirty, so
        // the upload must come after, or a glyph's first frame samples an empty
        // atlas and renders blank.
        let mut instances = Vec::new();
        for pane in panes {
            self.pane_instances(pane, &mut instances);
        }
        // Pane borders / dividers, over the panes.
        for d in decorations {
            instances.push(solid_rect(d));
        }
        // A bell flashes the whole surface briefly (white backdrop at low alpha).
        if flash > 0.0 {
            let mut q = backdrop(screen, flash);
            q.fg = [1.0, 1.0, 1.0, flash];
            q.bg = [1.0, 1.0, 1.0, flash];
            instances.push(q);
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

        // Upload/refresh image textures and build their quads before the pass;
        // resources cannot be created while a pass is recording.
        self.prepare_images(device, queue, panes, screen);
        // Update the background's cover-crop scale (also a pre-pass write).
        self.background.prepare(queue, screen);

        let clear = self.theme.background;
        // Premultiplied clear: the surface is composited with PreMultiplied alpha, so
        // the background colour is scaled by opacity and carries opacity as its alpha.
        let op = self.opacity as f64;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("terminal"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: to_linear(clear.0) as f64 * op,
                        g: to_linear(clear.1) as f64 * op,
                        b: to_linear(clear.2) as f64 * op,
                        a: op,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        // Background image first (behind everything); the translucent default-bg
        // cells then let it show through.
        self.background.record(&mut pass);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.instances.slice(..));
        pass.draw(0..4, 0..count as u32);

        // Inline images on top of the (blank) cells reserved for them. Prepared
        // before the pass; recorded here on their own pipeline.
        self.images.record(&mut pass);
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
                let blank = self.font.shaped_glyph(shaped[i], style).size[0] == 0.0;
                if !blank {
                    i += 1;
                    continue;
                }
                let mut j = i;
                while j < shaped.len()
                    && self.font.shaped_glyph(shaped[j], style).size[0] == 0.0
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
                    Ligature::Head { glyph_id: shaped[j], len, anchor: j - i },
                );
                for k in 1..len {
                    out.insert(start + i + k, Ligature::Tail);
                }
                i = j + 1;
            }
        }
        out
    }

    /// Draws a fold summary line standing in for `lines` collapsed rows (W2): a dim
    /// accent label like "⋯ 42 lines folded", left-aligned on the row.
    fn emit_fold_summary(
        &mut self,
        out: &mut Vec<Instance>,
        ox: f32,
        y: f32,
        cw: f32,
        cols: usize,
        lines: usize,
        base_bg: crate::config::Rgb,
        dim: f32,
    ) {
        let label = format!("\u{25b8} {lines} lines folded");
        let a = self.theme.accent;
        let mut fg = srgb(a.0, a.1, a.2);
        for c in &mut fg[..3] {
            *c *= 0.75 * dim;
        }
        let bg = {
            let [r, g, b, _] = srgb(base_bg.0, base_bg.1, base_bg.2);
            [r, g, b, self.opacity]
        };
        // Clamp to the pane width so a narrow pane's summary can't bleed into a
        // neighbour (there is no per-pane scissor for decorations).
        for (i, ch) in label.chars().take(cols).enumerate() {
            let glyph = self.font.glyph(ch, Style::REGULAR);
            out.push(Instance {
                pos_px: [ox + i as f32 * cw, y],
                glyph_offset: glyph.offset,
                glyph_size: glyph.size,
                uv_min: glyph.uv_min,
                uv_max: glyph.uv_max,
                fg,
                bg,
                flags: if glyph.color { FLAG_COLOR } else { 0 },
                width: 1.0,
                underline_style: 0,
                underline_color: [0.0; 4],
            });
        }
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

        // Fold-aware row plan (W2): with folds active a screen row may stand in for a
        // collapsed output region; without folds it is the plain viewport rows.
        let plan: Vec<crate::grid::PlanRow> = if grid.has_folds() {
            grid.display_plan()
        } else {
            (0..grid.rows()).map(|r| crate::grid::PlanRow::Real(grid.abs_row(r))).collect()
        };
        // Screen row the cursor lands on, accounting for folds above it (None if it
        // is hidden inside a fold — only finished output folds, so this is rare).
        let cursor_screen = plan
            .iter()
            .position(|p| matches!(p, crate::grid::PlanRow::Real(a) if *a == cursor_abs));

        for (row, prow) in plan.iter().enumerate() {
            let y = oy + row as f32 * ch;
            let abs = match prow {
                crate::grid::PlanRow::Real(a) => *a,
                crate::grid::PlanRow::Fold { lines, .. } => {
                    self.emit_fold_summary(out, ox, y, cw, grid.cols(), *lines, base_bg, dim);
                    continue;
                }
                crate::grid::PlanRow::Blank => continue,
            };
            let ligated = self.ligatures_for(grid, abs, cursor_abs, cur_col, selection);

            for col in 0..grid.cols() {
                let cell = grid.abs_cell(abs, col);
                if cell.is_spacer() {
                    continue;
                }
                // An annotation layer draws only its own marks; everywhere else the
                // pane below has to show through. Without this a blank cell still
                // emits a background-filled quad, and a full-pane hint grid hides
                // exactly the output it is labelling.
                if pane.transparent && cell.ch == ' ' && matches!(cell.pen.bg, Color::Default) {
                    continue;
                }
                let mut fg = self.resolve(cell.pen.fg, base_bg, true);
                // A default background uses the pane's (possibly tinted) base; any
                // explicit colour resolves normally.
                // A default background inherits the window opacity so the compositor
                // (and its blur) shows through; explicit colours stay fully opaque.
                let mut bg = match cell.pen.bg {
                    Color::Default => {
                        let [r, g, b, _] = srgb(base_bg.0, base_bg.1, base_bg.2);
                        [r, g, b, self.opacity]
                    }
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
                // Search-match highlight: amber for a match, brighter for the
                // current one so it stands out among the rest.
                if pane.search.len > 0 {
                    let in_match = |start: (usize, usize)| {
                        abs == start.0 && col >= start.1 && col < start.1 + pane.search.len
                    };
                    if pane.search.current.is_some_and(in_match) {
                        bg = srgb(0xf5, 0xa0, 0x23);
                        fg = srgb(0x0d, 0x0d, 0x0f);
                    } else if pane.search.matches.iter().any(|&m| in_match(m)) {
                        bg = srgb(0x6a, 0x54, 0x1a);
                    }
                }
                // A block cursor inverts the cell it sits on; beam/underline are
                // drawn as a separate bar after the loop so the character stays
                // legible under them.
                if grid.cursor_visible
                    && abs == cursor_abs
                    && col == cur_col
                    && matches!(pane.cursor, Some(crate::config::CursorShape::Block))
                {
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

                // Styled underline: the shape drives the shader's decoration path,
                // the colour follows the foreground unless SGR 58 set one. An
                // explicit colour carries alpha 1; the sentinel [0;4] (alpha 0)
                // tells the shader to reuse `fg`.
                let underline_color = match cell.pen.underline_color {
                    Color::Default => [0.0; 4],
                    other => {
                        let mut c = self.resolve(other, base_bg, true);
                        if dim < 1.0 {
                            for ch in &mut c[..3] {
                                *ch *= dim;
                            }
                        }
                        c
                    }
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
                    underline_style: cell.pen.underline.code(),
                    underline_color,
                });
            }
        }

        // Beam / underline cursor: a thin bar over the character, drawn with the
        // one-eighth block glyphs so it is exactly cell-aligned. Transparent bg
        // (alpha 0) means only the bar's pixels are opaque.
        use crate::config::CursorShape;
        if grid.cursor_visible && pane.focused {
            let bar = match pane.cursor {
                Some(CursorShape::Beam) => Some('\u{258F}'),   // ▏ left one-eighth
                Some(CursorShape::Underline) => Some('\u{2581}'), // ▁ lower one-eighth
                _ => None,
            };
            if let Some(bar_ch) = bar {
                // Screen row of the cursor from the fold-aware plan, so the bar tracks
                // the cursor whether scrolled back or shifted up by folds above it.
                if let Some(row) = cursor_screen {
                    let g = self.font.glyph(bar_ch, Style::REGULAR);
                    let cur = self.theme.cursor;
                    out.push(Instance {
                        pos_px: [ox + cur_col as f32 * cw, oy + row as f32 * ch],
                        glyph_offset: g.offset,
                        glyph_size: g.size,
                        uv_min: g.uv_min,
                        uv_max: g.uv_max,
                        fg: srgb(cur.0, cur.1, cur.2),
                        bg: [0.0, 0.0, 0.0, 0.0],
                        flags: 0,
                        width: 1.0,
                        underline_style: 0,
                        underline_color: [0.0; 4],
                    });
                }
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
                cursor: focused.then_some(crate::config::CursorShape::Block),
                search: Default::default(),
                transparent: false,
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
                    cursor: None,
                    search: Default::default(),
                    transparent: false,
                })
                .collect(),
        });
        renderer.render(&device, &queue, &mut encoder, &view, &panes, &[], overlay.as_ref(), 0.0, (width as f32, height as f32));
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
        ..Default::default()
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
            cursor: Some(crate::config::CursorShape::Block),
            search: Default::default(),
            transparent: false,
        }];
        renderer.render(
            &device,
            &queue,
            &mut encoder,
            &view,
            &panes,
            &[],
            None,
            0.0,
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
const FLAG_SOLID: u32 = 1 << 10;

/// Where a ligature sits on a row: the cell that draws it, or one it swallows.
#[derive(Clone, Copy)]
enum Ligature {
    Head { glyph_id: u16, len: usize, anchor: usize },
    Tail,
}

/// An image quad ready to draw: pixel rect plus the texture sub-range that
/// survived clipping to its pane.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ImageQuad {
    serial: u64,
    origin: [f32; 2],
    size: [f32; 2],
    uv_min: [f32; 2],
    uv_max: [f32; 2],
}

/// Intersects an image's pixel rect with its pane's rect (`[x, y, w, h]`),
/// scaling the uv range to the surviving part. `None` when nothing is visible —
/// an image scrolled wholly above or below the pane must not draw at all, or it
/// paints over whatever lives beyond the pane's edge.
fn clip_image_quad(serial: u64, origin: [f32; 2], size: [f32; 2], pane: [f32; 4]) -> Option<ImageQuad> {
    if size[0] <= 0.0 || size[1] <= 0.0 {
        return None;
    }
    let x0 = origin[0].max(pane[0]);
    let y0 = origin[1].max(pane[1]);
    let x1 = (origin[0] + size[0]).min(pane[0] + pane[2]);
    let y1 = (origin[1] + size[1]).min(pane[1] + pane[3]);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(ImageQuad {
        serial,
        origin: [x0, y0],
        size: [x1 - x0, y1 - y0],
        uv_min: [(x0 - origin[0]) / size[0], (y0 - origin[1]) / size[1]],
        uv_max: [(x1 - origin[0]) / size[0], (y1 - origin[1]) / size[1]],
    })
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

impl ImageLayer {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("image_shader.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image bind layout"),
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
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("image pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<ImgVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                })],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image vertices"),
            size: 0,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { pipeline, sampler, layout, cache: HashMap::new(), vertices, vcap: 0, frame: Vec::new() }
    }

    /// Uploads an image's texture on first sight; a no-op once cached.
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, img: &crate::grid::GridImage) {
        if self.cache.contains_key(&img.serial) {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("inline image"),
            size: wgpu::Extent3d { width: img.width, height: img.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Non-srgb: image data is passed through untouched (the sampler feeds
            // the fragment which returns it directly).
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &img.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(img.width * 4),
                rows_per_image: Some(img.height),
            },
            wgpu::Extent3d { width: img.width, height: img.height, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image bind group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        self.cache.insert(img.serial, (texture, bind_group));
    }

    /// Drops cached textures for images no longer present.
    fn retain(&mut self, seen: &std::collections::HashSet<u64>) {
        self.cache.retain(|serial, _| seen.contains(serial));
    }

    /// Writes the frame's image quads (4 clip-space vertices each) and remembers the
    /// draw order. Returns the image count.
    fn build_vertices(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        quads: &[ImageQuad],
        screen: (f32, f32),
    ) -> usize {
        self.frame.clear();
        let mut verts: Vec<ImgVertex> = Vec::with_capacity(quads.len() * 4);
        for (i, q) in quads.iter().enumerate() {
            let to_ndc = |px: f32, py: f32| {
                [px / screen.0 * 2.0 - 1.0, 1.0 - py / screen.1 * 2.0]
            };
            let (x0, y0) = (q.origin[0], q.origin[1]);
            let (x1, y1) = (q.origin[0] + q.size[0], q.origin[1] + q.size[1]);
            let (u0, v0) = (q.uv_min[0], q.uv_min[1]);
            let (u1, v1) = (q.uv_max[0], q.uv_max[1]);
            // Triangle strip: TL, TR, BL, BR with matching uvs.
            verts.push(ImgVertex { pos: to_ndc(x0, y0), uv: [u0, v0] });
            verts.push(ImgVertex { pos: to_ndc(x1, y0), uv: [u1, v0] });
            verts.push(ImgVertex { pos: to_ndc(x0, y1), uv: [u0, v1] });
            verts.push(ImgVertex { pos: to_ndc(x1, y1), uv: [u1, v1] });
            self.frame.push((q.serial, i as u32 * 4));
        }
        if verts.len() > self.vcap {
            self.vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("image vertices"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            self.vcap = verts.len();
        } else if !verts.is_empty() {
            queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&verts));
        }
        quads.len()
    }

    fn record(&self, pass: &mut wgpu::RenderPass) {
        if self.frame.is_empty() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        for (serial, base) in &self.frame {
            if let Some((_, bind_group)) = self.cache.get(serial) {
                pass.set_bind_group(0, bind_group, &[]);
                pass.draw(*base..*base + 4, 0..1);
            }
        }
    }
}

impl Background {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bg shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("bg_shader.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bg bind layout"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    // The scale is read in the vertex stage, the dim in the fragment.
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bg pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bg pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bg sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bg uniforms"),
            size: std::mem::size_of::<BgUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { pipeline, layout, sampler, uniforms, bind: None, aspect: 1.0, dim: 0.35 }
    }

    fn set_image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rgba: Option<(&[u8], u32, u32)>,
        dim: f32,
    ) {
        self.dim = dim.clamp(0.0, 1.0);
        let Some((pixels, w, h)) = rgba else {
            self.bind = None;
            return;
        };
        if w == 0 || h == 0 || pixels.len() < (w as usize) * (h as usize) * 4 {
            self.bind = None;
            return;
        }
        self.aspect = w as f32 / h as f32;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bg texture"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg bind"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: self.uniforms.as_entire_binding() },
            ],
        });
        self.bind = Some(bind);
    }

    /// Updates the cover-crop scale for the current surface size. Call before the
    /// pass (write_buffer cannot run inside a pass).
    fn prepare(&self, queue: &wgpu::Queue, screen: (f32, f32)) {
        if self.bind.is_none() {
            return;
        }
        let surface_aspect = screen.0 / screen.1.max(1.0);
        let ratio = surface_aspect / self.aspect.max(0.001);
        let scale = [ratio.min(1.0), (1.0 / ratio).min(1.0)];
        queue.write_buffer(
            &self.uniforms,
            0,
            bytemuck::bytes_of(&BgUniforms { scale, dim: self.dim, _pad: 0.0 }),
        );
    }

    fn record(&self, pass: &mut wgpu::RenderPass) {
        if let Some(bind) = &self.bind {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        }
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

/// Mixes `over` (a raw tint) into `base` by `t`, in sRGB space. Only used for the
/// subtle context tint, where sRGB mixing is close enough and cheaper.
fn blend(base: crate::config::Rgb, over: (u8, u8, u8), t: f32) -> crate::config::Rgb {
    let mix = |a: u8, b: u8| (a as f32 * (1.0 - t) + b as f32 * t) as u8;
    crate::config::Rgb(mix(base.0, over.0), mix(base.1, over.1), mix(base.2, over.2))
}

/// A full-surface quad at `alpha`, black, to dim panes behind an overlay. Glyphless
/// (so only its background shows) and flagged fullscreen (so the vertex shader
/// spans the whole surface rather than one cell).
/// A solid-colour rectangle instance: pixel position in `pos_px`, pixel size in
/// `glyph_size`, flagged `FLAG_SOLID` so the shader fills it flat.
fn solid_rect(d: &SolidRect) -> Instance {
    Instance {
        pos_px: [d.x, d.y],
        glyph_offset: [0.0, 0.0],
        glyph_size: [d.w, d.h],
        uv_min: [0.0, 0.0],
        uv_max: [0.0, 0.0],
        fg: srgb(d.color.0, d.color.1, d.color.2),
        bg: srgb(d.color.0, d.color.1, d.color.2),
        flags: FLAG_SOLID,
        width: 1.0,
        underline_style: 0,
        underline_color: [0.0; 4],
    }
}

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
        underline_style: 0,
        underline_color: [0.0; 4],
    }
}

/// The standard xterm 256-colour layout: 16 themeable ANSI colours, a 6x6x6 cube,
/// then 24 greys. The cube and greys are fixed by the spec; only 0-15 are themed.
pub(crate) fn xterm256(i: u8, ansi: &[crate::config::Rgb]) -> crate::config::Rgb {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_quad_clips_to_the_pane_rect() {
        let pane = [100.0, 50.0, 200.0, 100.0]; // x, y, w, h

        // Fully inside: untouched, full uv range.
        let q = clip_image_quad(1, [120.0, 60.0], [40.0, 20.0], pane).unwrap();
        assert_eq!(q.origin, [120.0, 60.0]);
        assert_eq!(q.size, [40.0, 20.0]);
        assert_eq!((q.uv_min, q.uv_max), ([0.0, 0.0], [1.0, 1.0]));

        // Scrolled half above the pane: only the lower half draws, upper uvs cut.
        let q = clip_image_quad(2, [120.0, 30.0], [40.0, 40.0], pane).unwrap();
        assert_eq!(q.origin[1], 50.0);
        assert_eq!(q.size[1], 20.0);
        assert_eq!(q.uv_min[1], 0.5);
        assert_eq!(q.uv_max[1], 1.0);

        // Hanging past the pane bottom: clipped there, not painted over what is
        // below the pane.
        let q = clip_image_quad(3, [120.0, 140.0], [40.0, 40.0], pane).unwrap();
        assert_eq!(q.origin[1] + q.size[1], 150.0);
        assert_eq!(q.uv_max[1], 0.25);

        // Regression: an image wholly below (or above) the pane must not draw.
        assert!(clip_image_quad(4, [120.0, 160.0], [40.0, 40.0], pane).is_none());
        assert!(clip_image_quad(5, [120.0, 0.0], [40.0, 40.0], pane).is_none());
    }
}
