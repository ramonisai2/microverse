// Screen-space HUD (clip quads + atlas icons).
struct HudVertexIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}

struct HudVertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}

@group(0) @binding(0) var hud_tex: texture_2d<f32>;
@group(0) @binding(1) var hud_samp: sampler;

@vertex
fn vs_main(in: HudVertexIn) -> HudVertexOut {
    var out: HudVertexOut;
    out.clip = vec4<f32>(in.pos, 0.0, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: HudVertexOut) -> @location(0) vec4<f32> {
    let tex = textureSample(hud_tex, hud_samp, in.uv);
    let rgba = tex * in.color;
    if rgba.a < 0.02 {
        discard;
    }
    return rgba;
}
