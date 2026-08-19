struct BlurParams {
    /// Base / depth-of-field blur mix (0 = sharp).
    amount: f32,
    texel_x: f32,
    texel_y: f32,
    /// Extra blur strength at screen corners (fisheye / peripheral soft).
    edge_blur: f32,
}

@group(0) @binding(0) var scene_tex: texture_2d<f32>;
@group(0) @binding(1) var scene_samp: sampler;
@group(0) @binding(2) var<uniform> params: BlurParams;

struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VsOut {
    // Fullscreen triangle.
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let p = positions[idx];
    var out: VsOut;
    out.clip_position = vec4<f32>(p, 0.0, 1.0);
    // NDC → UV (wgpu render targets: y=0 at top).
    out.uv = vec2<f32>(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let center = textureSample(scene_tex, scene_samp, in.uv);

    // Aspect-correct radius: 0 at center → ~1 at mid-edges → >1 at corners.
    let aspect = params.texel_y / max(params.texel_x, 1e-6);
    let p = (in.uv - vec2<f32>(0.5, 0.5)) * vec2<f32>(1.0, aspect);
    let r = length(p) * 2.0;
    // Fisheye: small sharp center, blur ramps toward edges/corners.
    let edge = smoothstep(0.18, 0.85, r);
    let edge_w = mix(edge, edge * edge, 0.35);

    let local_amount = params.amount + params.edge_blur * edge_w;
    if local_amount < 0.001 {
        return center;
    }

    let radius = 1.2 + local_amount * 10.0;
    let step = vec2<f32>(params.texel_x, params.texel_y) * radius;

    // Cheap 5-tap cross for mild blur; full 9-tap only at strong periphery.
    var acc = center * 4.0;
    var w = 4.0;
    let cross = array<vec2<f32>, 4>(
        vec2<f32>(1.0, 0.0),
        vec2<f32>(-1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );
    for (var i = 0; i < 4; i = i + 1) {
        acc += textureSample(scene_tex, scene_samp, in.uv + cross[i] * step) * 2.0;
        w += 2.0;
    }
    if local_amount > 0.18 {
        let diag = array<vec2<f32>, 4>(
            vec2<f32>(1.0, 1.0),
            vec2<f32>(-1.0, 1.0),
            vec2<f32>(1.0, -1.0),
            vec2<f32>(-1.0, -1.0),
        );
        for (var i = 0; i < 4; i = i + 1) {
            acc += textureSample(scene_tex, scene_samp, in.uv + diag[i] * step) * 1.1;
            w += 1.1;
        }
    }

    let blurred = acc / w;
    return mix(center, blurred, clamp(local_amount, 0.0, 1.0));
}
