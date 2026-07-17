struct Uniforms {
    cell: vec2<f32>,
    screen: vec2<f32>,
    // (y from cell top, thickness)
    underline: vec2<f32>,
    strike: vec2<f32>,
};

const FLAG_UNDERLINE: u32 = 8u;
const FLAG_STRIKE: u32 = 64u;
const FLAG_COLOR: u32 = 256u;
const FLAG_FULLSCREEN: u32 = 512u;
// A solid rectangle at pixel position, sized by glyph_size (pixels): pane borders,
// dividers. No glyph is sampled.
const FLAG_SOLID: u32 = 1024u;

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct Instance {
    @location(0) pos_px: vec2<f32>,
    @location(1) glyph_offset: vec2<f32>,
    @location(2) glyph_size: vec2<f32>,
    @location(3) uv_min: vec2<f32>,
    @location(4) uv_max: vec2<f32>,
    @location(5) fg: vec4<f32>,
    @location(6) bg: vec4<f32>,
    @location(7) flags: u32,
    // Cells this glyph spans. A CJK glyph clipped to one cell loses its right half.
    @location(8) width: f32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) glyph_offset: vec2<f32>,
    @location(2) glyph_size: vec2<f32>,
    @location(3) uv_min: vec2<f32>,
    @location(4) uv_max: vec2<f32>,
    @location(5) fg: vec4<f32>,
    @location(6) bg: vec4<f32>,
    @location(7) @interpolate(flat) flags: u32,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Instance) -> VsOut {
    // Triangle strip: 0,0 -> 1,0 -> 0,1 -> 1,1
    let corner = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    // A fullscreen quad (the overlay backdrop) spans the whole surface; every
    // other instance spans `width` cells by one.
    var span = u.cell * vec2<f32>(inst.width, 1.0);
    if (inst.flags & FLAG_FULLSCREEN) != 0u {
        span = u.screen;
    } else if (inst.flags & FLAG_SOLID) != 0u {
        // A solid rect carries its pixel size in glyph_size.
        span = inst.glyph_size;
    }
    let px = inst.pos_px + corner * span;

    var out: VsOut;
    out.clip = vec4<f32>(
        px.x / u.screen.x * 2.0 - 1.0,
        1.0 - px.y / u.screen.y * 2.0,
        0.0,
        1.0,
    );
    // Interpolates to the pixel offset within the glyph's span.
    out.local = corner * span;
    out.glyph_offset = inst.glyph_offset;
    out.glyph_size = inst.glyph_size;
    out.uv_min = inst.uv_min;
    out.uv_max = inst.uv_max;
    out.fg = inst.fg;
    out.bg = inst.bg;
    out.flags = inst.flags;
    return out;
}

fn in_band(y: f32, band: vec2<f32>) -> bool {
    return y >= band.x && y < band.x + band.y;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // A solid rect (border/divider) is just its background, no glyph.
    if (in.flags & FLAG_SOLID) != 0u {
        return in.bg;
    }
    var color = in.bg;

    let g = in.local - in.glyph_offset;
    let inside = all(g >= vec2<f32>(0.0)) && all(g < in.glyph_size);
    // Output alpha is the greater of the background's own alpha and the glyph
    // coverage. For a normal cell (bg alpha 1) this is always 1. For a cursor
    // overlay (bg alpha 0) only the glyph's pixels become opaque, so a beam or
    // underline bar draws over the character beneath without a solid box.
    var out_a = in.bg.a;

    if inside && in.glyph_size.x > 0.0 {
        let uv = mix(in.uv_min, in.uv_max, g / in.glyph_size);
        let texel = textureSampleLevel(atlas, atlas_sampler, uv, 0.0);
        // Masks are stored white with coverage in alpha, so the cell's foreground
        // tints them; emoji bring their own colour and ignore it.
        let ink = select(in.fg.rgb, texel.rgb, (in.flags & FLAG_COLOR) != 0u);
        color = vec4<f32>(mix(color.rgb, ink, texel.a), max(out_a, texel.a));
    }

    // Decorations sit on top of the glyph, so a strikeout actually strikes it out.
    let underlined = (in.flags & FLAG_UNDERLINE) != 0u && in_band(in.local.y, u.underline);
    let struck = (in.flags & FLAG_STRIKE) != 0u && in_band(in.local.y, u.strike);
    if underlined || struck {
        color = vec4<f32>(in.fg.rgb, max(out_a, 1.0));
    }

    return color;
}
