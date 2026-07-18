// Fullscreen background image, drawn first (behind the terminal). A single
// oversized triangle covers the viewport; the texture is centre-cropped to the
// aspect ratio via `scale`, sampled, dimmed, and emitted premultiplied to match the
// text pipeline's PREMULTIPLIED_ALPHA_BLENDING.

struct BgUniforms {
    // Multiply on the sampled uv to centre-crop (cover) the image.
    scale: vec2<f32>,
    dim: f32,
    _pad: f32,
};

@group(0) @binding(0) var img: texture_2d<f32>;
@group(0) @binding(1) var img_sampler: sampler;
@group(0) @binding(2) var<uniform> u: BgUniforms;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    // Oversized triangle trick: covers the whole clip space with 3 vertices.
    var out: VsOut;
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    let uv = vec2<f32>(x, y);
    out.clip = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    // Flip Y for texture space, then centre-crop by `scale` around 0.5.
    let tex = vec2<f32>(uv.x, 1.0 - uv.y);
    out.uv = (tex - 0.5) * u.scale + 0.5;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(img, img_sampler, in.uv, 0.0).rgb * u.dim;
    // Opaque background, premultiplied (alpha 1 → rgb unchanged).
    return vec4<f32>(c, 1.0);
}
