// Textured quad for inline images (kitty graphics). Positions arrive already in
// clip space; the image texture is sampled straight through.

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var img: texture_2d<f32>;
@group(0) @binding(1) var img_sampler: sampler;

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>) -> VsOut {
    var out: VsOut;
    out.clip = vec4<f32>(pos, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSampleLevel(img, img_sampler, in.uv, 0.0);
}
