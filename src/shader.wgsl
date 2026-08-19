struct FrameUniform {
    view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    camera_pos: vec3<f32>,
    time: f32,
    fog_color: vec3<f32>,
    fog_density: f32,
    focus_dist: f32,
    shadow_bias: f32,
    /// Local terrain surface Y (for underground wire edges).
    surface_y: f32,
    /// Eye / focus Y used with surface_y.
    eye_y: f32,
    /// Player focus XZ — dig cutaway bubble.
    focus_xz: vec2<f32>,
    /// 0 open → 1 confined (gates geometric dig cutaway).
    confine: f32,
    /// 0 outdoors → 1 enclosed (4 walls + roof). Same Bayer cutaway as dig.
    indoors: f32,
}

@group(0) @binding(0)
var<uniform> frame: FrameUniform;

@group(0) @binding(1)
var shadow_map: texture_depth_2d;
@group(0) @binding(2)
var shadow_sampler: sampler_comparison;

// Terrain atlas (grass | dirt | bark | leaves | stone | planks | village stone).
@group(1) @binding(0)
var terrain_tex: texture_2d<f32>;
@group(1) @binding(1)
var terrain_samp: sampler;

// Grass billboard atlas (6 tiles × mirrored = 12 variants). Magenta = cutout.
@group(2) @binding(0)
var grass_tex: texture_2d<f32>;
@group(2) @binding(1)
var grass_samp: sampler;

const GRASS_ATLAS_TILES: f32 = 6.0;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec3<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) flags: f32,
    @location(5) seed: f32,
    @location(6) ao: f32,
}

struct GrassInstanceInput {
    @location(7) origin: vec3<f32>,
    @location(8) color: vec3<f32>,
    /// 0..11 → tile = variant % 6, flip when variant >= 6.
    @location(9) variant: f32,
}

/// Map template UV [0,1]² into the atlas tile (optional horizontal flip).
fn grass_atlas_uv(local_uv: vec2<f32>, variant: f32) -> vec2<f32> {
    let v = floor(variant + 0.5);
    let tile = v % GRASS_ATLAS_TILES;
    let flip = v >= GRASS_ATLAS_TILES;
    let u = select(local_uv.x, 1.0 - local_uv.x, flip);
    return vec2<f32>((tile + u) / GRASS_ATLAS_TILES, local_uv.y);
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) flags: f32,
    @location(4) seed: f32,
    @location(5) ao: f32,
    @location(6) world_pos: vec3<f32>,
}

struct ShadowGrassOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

fn shadow_factor(world_pos: vec3<f32>, n: vec3<f32>, light_dir: vec3<f32>) -> f32 {
    var light_clip = frame.light_view_proj * vec4<f32>(world_pos, 1.0);
    let ndc = light_clip.xyz / max(light_clip.w, 1e-5);
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, -ndc.y * 0.5 + 0.5);
    let depth = ndc.z;
    if uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || depth < 0.0 || depth > 1.0 {
        return 1.0;
    }
    let ndl = max(dot(n, light_dir), 0.0);
    let bias = frame.shadow_bias + (1.0 - ndl) * 0.0035;
    let compare = depth - bias;
    // 9-tap PCF with a slightly wider kernel — soft penumbra hides texel stairs.
    let texel = 1.0 / 1536.0;
    var sum = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let off = vec2<f32>(f32(x), f32(y)) * 1.25 * texel;
            sum += textureSampleCompare(shadow_map, shadow_sampler, uv + off, compare);
        }
    }
    let raw = sum / 9.0;
    // Lift the umbra so hard edges read softer (stylized HD-2D).
    return mix(0.58, 1.0, raw);
}

/// Soft 4-tap for grass — cheap, still less blocky than a single compare.
fn shadow_factor_cheap(world_pos: vec3<f32>, n: vec3<f32>, light_dir: vec3<f32>) -> f32 {
    var light_clip = frame.light_view_proj * vec4<f32>(world_pos, 1.0);
    let ndc = light_clip.xyz / max(light_clip.w, 1e-5);
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, -ndc.y * 0.5 + 0.5);
    let depth = ndc.z;
    if uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0 || depth < 0.0 || depth > 1.0 {
        return 1.0;
    }
    let bias = frame.shadow_bias + (1.0 - max(dot(n, light_dir), 0.0)) * 0.0035;
    let compare = depth - bias;
    let texel = 1.0 / 1536.0;
    var sum = 0.0;
    let offsets = array<vec2<f32>, 4>(
        vec2<f32>(-0.7, -0.7),
        vec2<f32>(0.7, -0.7),
        vec2<f32>(-0.7, 0.7),
        vec2<f32>(0.7, 0.7),
    );
    for (var i = 0; i < 4; i++) {
        sum += textureSampleCompare(shadow_map, shadow_sampler, uv + offsets[i] * texel, compare);
    }
    return mix(0.62, 1.0, sum / 4.0);
}

/// Magenta (#FF00FF) cutout — cheapest “alpha” without an alpha channel.
fn grass_chroma_keep(tex_rgb: vec3<f32>) -> bool {
    let key = vec3<f32>(1.0, 0.0, 1.0);
    return distance(tex_rgb, key) >= 0.32;
}

fn shade_lit(in: VertexOutput, albedo: vec3<f32>) -> vec4<f32> {
    // Same 4-tap PCF as grass — 9-tap was a large always-on GPU cost for little HD-2D gain.
    return shade_lit_shadow(in, albedo, true);
}

fn shade_lit_grass(in: VertexOutput, albedo: vec3<f32>) -> vec4<f32> {
    return shade_lit_shadow(in, albedo, true);
}

fn shade_lit_shadow(in: VertexOutput, albedo: vec3<f32>, cheap_shadow: bool) -> vec4<f32> {
    let light_dir = normalize(vec3<f32>(-0.4, 0.9, -0.2));
    let n = normalize(in.normal);
    let ndl = max(dot(n, light_dir), 0.0);

    let band = select(0.42, select(0.72, 0.92, ndl > 0.75), ndl > 0.35);
    let diffuse = mix(ndl, band, 0.55);

    let up = clamp(n.y, 0.0, 1.0);
    let ambient = 0.22 + 0.10 * up;
    let ao = mix(1.0, clamp(in.ao, 0.42, 1.0), mix(1.0, 0.55, up));

    let view_dir = normalize(frame.camera_pos - in.world_pos);
    let half_v = normalize(light_dir + view_dir);
    let spec = pow(max(dot(n, half_v), 0.0), 8.0) * 0.08 * up;

    let shadow = select(
        shadow_factor(in.world_pos, n, light_dir),
        shadow_factor_cheap(in.world_pos, n, light_dir),
        cheap_shadow,
    );
    var lit = albedo * ao * (ambient + (1.0 - ambient) * diffuse * shadow) + vec3<f32>(spec) * shadow;

    let lum = dot(lit, vec3<f32>(0.299, 0.587, 0.114));
    let cool_shadow = mix(lit, vec3<f32>(lum * 0.85, lum * 0.90, lum * 1.05), 0.12 * (1.0 - diffuse));
    let warm_light = mix(cool_shadow, cool_shadow * vec3<f32>(1.05, 1.0, 0.92), 0.10 * diffuse);
    lit = warm_light;

    // Underground / dug pit: soft voxel wire + oriented hatch (not a hard X-ray cage).
    let under_t = underground_amount(in.world_pos.y);
    let below_eye = in.world_pos.y <= frame.eye_y + 0.15;
    let eye_under = max(0.0, frame.surface_y - frame.eye_y);
    // One depth curve for atmosphere (never pure black — floor ~55% lit).
    let cave_t = smoothstep(1.0, 18.0, eye_under);
    // Lid fragments above the eye: skip the wire cage — cutaway already punches the view.
    let lid_xray = !below_eye && eye_under > 0.25;
    if under_t > 0.001 && !lid_xray {
        let edge = voxel_edge_factor(in.world_pos, n); // 0 = edge, 1 = face center
        let line = 1.0 - edge;
        let darken = select(0.22, 0.0, below_eye);
        lit = lit * (1.0 - darken * under_t);
        // Cool lift, not pure white — keeps structure without screaming.
        let wire = mix(lit, vec3<f32>(0.78, 0.86, 0.94), 0.55);
        let wire_amt = select(under_t * 0.28, under_t * 0.18, below_eye);
        lit = mix(lit, wire, wire_amt * line);
    }
    // Soft fill when the eye is in a pit — lift floor/lower walls only (shallow digs).
    if below_eye && eye_under > 0.35 {
        let fill = smoothstep(0.35, 2.0, eye_under) * (1.0 - cave_t * 0.7) * 0.55;
        lit = lit * (1.0 + fill);
        // Hatch: horizontal strokes on floors, vertical on walls.
        let hatch = dig_hatch_factor(in.world_pos, n);
        let hatch_col = mix(lit, vec3<f32>(0.70, 0.82, 0.92), 0.45);
        lit = mix(lit, hatch_col, hatch * 0.22 * smoothstep(0.35, 2.0, eye_under) * (1.0 - cave_t * 0.5));
    }

    // Unified cave atmosphere: cool + desat + dim (walls stay readable).
    if cave_t > 0.001 {
        let cool = vec3<f32>(0.82, 0.90, 1.02);
        lit = mix(lit, lit * cool, cave_t * 0.55);
        let lum = dot(lit, vec3<f32>(0.299, 0.587, 0.114));
        lit = mix(lit, vec3<f32>(lum), cave_t * 0.32);
        lit = lit * mix(1.0, 0.58, cave_t);
        // Ceiling colder/darker; floor slightly warmer — same material, different mood.
        if n.y < -0.5 {
            lit = lit * mix(1.0, 0.76, cave_t);
            lit = mix(lit, lit * vec3<f32>(0.72, 0.82, 1.08), cave_t * 0.45);
        } else if n.y > 0.5 {
            lit = mix(lit, lit * vec3<f32>(1.10, 1.02, 0.90), cave_t * 0.28);
        }
    }

    // Minecraft-style cave mask: only the local bubble around the player stays
    // readable. Distant chambers paint as solid virtual rock / are discarded so
    // the underground clear color reads as closed stone (no see-through graph).
    let focus_dist_xz = length(in.world_pos.xz - frame.focus_xz);
    let frag_under = max(0.0, frame.surface_y - in.world_pos.y);
    let bury = max(eye_under, frame.confine * 4.0);
    let mask_on = smoothstep(0.45, 1.75, bury);
    // 8 clear → 14 fully masked: broad ease avoids a visible circular cut.
    let mask_t = smoothstep(8.0, 14.0, focus_dist_xz) * smoothstep(0.35, 1.25, frag_under) * mask_on;
    let virtual_rock = vec3<f32>(0.06, 0.07, 0.09);
    // Drop far cave fragments entirely — void uses underground clear (= rock).
    if mask_t > 0.985 {
        discard;
    }
    if mask_t > 0.001 {
        lit = mix(lit, virtual_rock, mask_t);
    }

    let dist = focus_dist_xz;
    // Depth²-ish fog from the player focus (not the elevated HD-2D lens).
    let fog_boost = 1.0 + cave_t * cave_t * 2.4;
    let fog_t = 1.0 - exp(-frame.fog_density * fog_boost * dist);
    let cave_distance_fog = max(
        smoothstep(8.0, 14.0, dist) * mask_on,
        mask_t,
    );
    let near_structure_fog = clamp(fog_t, 0.0, 1.0) * (1.0 - 0.15 * under_t * (1.0 - mask_on));
    let fog_amt = max(near_structure_fog, cave_distance_fog);
    let fog = mix(frame.fog_color, virtual_rock, mask_on * 0.85);
    return vec4<f32>(mix(lit, fog, fog_amt), 1.0);
}

/// 0 at natural grass top, →1 for dug pits / caves (vs undug surface_y).
fn underground_amount(world_y: f32) -> f32 {
    let depth = frame.surface_y - world_y;
    // Side walls just under the lip also count (shallow digs in the screenshot).
    // Eye below natural surface (standing in a pit) boosts the effect.
    let eye_depth = frame.surface_y - frame.eye_y;
    let d = max(depth, eye_depth * 0.5);
    return smoothstep(0.15, 4.0, d);
}

/// Voxel wire lines — thin soft stroke via fwidth (readable in HD-2D, not hard white).
fn voxel_edge_factor(world_pos: vec3<f32>, n: vec3<f32>) -> f32 {
    var uv2: vec2<f32>;
    if abs(n.y) > 0.5 {
        uv2 = world_pos.xz;
    } else if abs(n.x) > 0.5 {
        uv2 = world_pos.zy;
    } else {
        uv2 = world_pos.xy;
    }
    // Narrower + softer falloff so edges read as a whisper, not a cage.
    let fw = max(fwidth(uv2), vec2<f32>(0.018)) * 1.85;
    let f = fract(uv2);
    let e0 = smoothstep(vec2<f32>(0.0), fw, f);
    let e1 = smoothstep(vec2<f32>(0.0), fw, 1.0 - f);
    let e = min(e0, e1);
    let m = min(e.x, e.y);
    return smoothstep(0.0, 0.85, m);
}

/// Dig hatch: 1 on stroke. Horizontal faces → horizontal lines; vertical → vertical.
fn dig_hatch_factor(world_pos: vec3<f32>, n: vec3<f32>) -> f32 {
    let freq = 6.0;
    var t: f32;
    if abs(n.y) > 0.5 {
        // Floor/ceiling: horizontal strokes (constant Z bands).
        t = world_pos.z * freq;
    } else {
        // Walls: vertical strokes (constant Y is wrong — use Y for vertical lines).
        t = world_pos.y * freq;
    }
    let f = fract(t);
    let fw = max(fwidth(t), 0.03) * 2.2;
    let band = min(f, 1.0 - f);
    // Soft band so hatch stays a hint, not chalk strokes.
    return 1.0 - smoothstep(0.0, fw * 1.35, band);
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.world_pos = in.position;
    out.clip_position = frame.view_proj * vec4<f32>(in.position, 1.0);
    out.normal = in.normal;
    out.color = in.color;
    out.uv = in.uv;
    out.flags = in.flags;
    out.seed = in.seed;
    out.ao = in.ao;
    return out;
}

@vertex
fn vs_shadow(in: VertexInput) -> @builtin(position) vec4<f32> {
    return frame.light_view_proj * vec4<f32>(in.position, 1.0);
}

@vertex
fn vs_shadow_grass(v: VertexInput, inst: GrassInstanceInput) -> ShadowGrassOut {
    var out: ShadowGrassOut;
    let world = v.position + inst.origin;
    out.clip_position = frame.light_view_proj * vec4<f32>(world, 1.0);
    // Blob verts keep local UV; blades sample the atlas tile.
    out.uv = select(grass_atlas_uv(v.uv, inst.variant), v.uv, v.flags > 0.5);
    return out;
}

@fragment
fn fs_shadow_grass(in: ShadowGrassOut) {
    let tex = textureSample(grass_tex, grass_samp, in.uv);
    if !grass_chroma_keep(tex.rgb) {
        discard;
    }
}

@vertex
fn vs_grass(v: VertexInput, inst: GrassInstanceInput) -> VertexOutput {
    var out: VertexOutput;
    let world = v.position + inst.origin;
    out.world_pos = world;
    out.clip_position = frame.view_proj * vec4<f32>(world, 1.0);
    out.normal = v.normal;
    // Blob verts carry their own dark color; blades tint from the instance.
    out.color = select(inst.color, v.color, v.flags > 0.5);
    // Contact blob keeps [0,1] UV; crossed blades pick atlas tile + optional flip.
    out.uv = select(grass_atlas_uv(v.uv, inst.variant), v.uv, v.flags > 0.5);
    out.flags = v.flags;
    out.seed = 0.0;
    out.ao = 1.0;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let cut = apply_dig_cutaway(in.world_pos, in.clip_position, in.normal);
    var col = shade_lit(in, terrain_albedo(in));
    // Exterior crop gradient: fade lid toward fog + alpha (blend on).
    if cut > 0.001 {
        let fog = mix(frame.fog_color, vec3<f32>(0.78, 0.90, 0.76), 0.18);
        let rgb = mix(col.rgb, fog, smoothstep(0.0, 1.0, cut) * 0.85);
        let a = clamp(1.0 - cut, 0.04, 1.0);
        return vec4<f32>(rgb, a);
    }
    return vec4<f32>(col.rgb, 1.0);
}

const TERRAIN_ATLAS_N: f32 = 12.0;

fn hash_u32(x: u32) -> u32 {
    var n = x;
    n = (n ^ (n >> 16u)) * 0x7FEB352Du;
    n = (n ^ (n >> 15u)) * 0x846CA68Bu;
    return n ^ (n >> 16u);
}

fn hash_cell2(p: vec2<i32>) -> u32 {
    return hash_u32(
        u32(p.x) * 374761393u + u32(p.y) * 668265263u + 0x9E3779B9u,
    );
}

/// Rotate local UV by k*90° around tile center (k = 0..3).
fn rot90_uv(uv: vec2<f32>, k: u32) -> vec2<f32> {
    let c = uv - vec2<f32>(0.5, 0.5);
    let r = k % 4u;
    var o = c;
    if r == 1u {
        o = vec2<f32>(-c.y, c.x);
    } else if r == 2u {
        o = vec2<f32>(-c.x, -c.y);
    } else if r == 3u {
        o = vec2<f32>(c.y, -c.x);
    }
    return o + vec2<f32>(0.5, 0.5);
}

fn sample_terrain_tile(tile: f32, local: vec2<f32>) -> vec3<f32> {
    let inset = 0.5 / 16.0;
    let u = clamp(local.x, inset, 1.0 - inset);
    let v = clamp(local.y, inset, 1.0 - inset);
    let atlas_uv = vec2<f32>((tile + u) / TERRAIN_ATLAS_N, v);
    return textureSample(terrain_tex, terrain_samp, atlas_uv).rgb;
}

/// Break stone tiling: per-block variant + rot/flip + dual-scale blend.
fn stone_albedo_varied(
    world_pos: vec3<f32>,
    n: vec3<f32>,
    uv: vec2<f32>,
    tint: vec3<f32>,
) -> vec3<f32> {
    // Stable cell id on the face plane (matches 1×1 world UV tiling).
    var cell: vec2<i32>;
    if abs(n.y) > 0.5 {
        cell = vec2<i32>(i32(floor(world_pos.x)), i32(floor(world_pos.z)));
    } else if abs(n.x) > 0.5 {
        cell = vec2<i32>(i32(floor(world_pos.z)), i32(floor(world_pos.y)));
    } else {
        cell = vec2<i32>(i32(floor(world_pos.x)), i32(floor(world_pos.y)));
    }

    let h = hash_cell2(cell);
    let variant = 5.0 + f32(h % 3u); // tiles 5,6,7
    var local = fract(uv);
    local = rot90_uv(local, h % 4u);
    if ((h >> 3u) & 1u) == 1u {
        local.x = 1.0 - local.x;
    }
    if ((h >> 4u) & 1u) == 1u {
        local.y = 1.0 - local.y;
    }

    // Second layer at incommensurate scale — kills large-area wallpaper look.
    let h2 = hash_cell2(cell + vec2<i32>(19, 37));
    let off = vec2<f32>(f32(h2 & 255u), f32((h2 >> 8u) & 255u)) / 255.0;
    var local2 = fract(uv * 0.618034 + off);
    local2 = rot90_uv(local2, (h2 >> 2u) % 4u);
    let variant2 = 5.0 + f32(h2 % 3u);

    let a = sample_terrain_tile(variant, local);
    let b = sample_terrain_tile(variant2, fract(local2));
    let w = 0.28 + f32((h >> 8u) & 255u) / 255.0 * 0.40;
    var tex = mix(a, b, w);

    // Soft per-block tone so neighbors don't match even with same rot.
    let tone = 0.86 + f32((h >> 16u) & 255u) / 255.0 * 0.28;
    tex *= tone;
    return mix(tint, tex, 0.90);
}

/// Sample terrain atlas when `flags` marks a textured face.
/// Tiles: 0 grass-top, 1 grass-side, 2 dirt, 3 bark, 4 leaves,
/// 5–7 natural stone variants, 8 planks, 9 village stone, 10 cobble.
fn terrain_albedo(in: VertexOutput) -> vec3<f32> {
    if in.flags < 1.5 {
        return in.color;
    }
    let tile = floor(in.seed + 0.5);
    if tile > 4.5 && tile < 7.5 {
        return stone_albedo_varied(in.world_pos, in.normal, in.uv, in.color);
    }
    let local = fract(in.uv);
    let tex = sample_terrain_tile(tile, local);
    return mix(in.color, tex, 0.78);
}

/// Burial under the natural lip → 0 (surface) .. 1 (~3+ stair treads deep).
fn dig_burial_deep_t() -> f32 {
    return smoothstep(0.35, 3.25, frame.surface_y - frame.eye_y);
}

/// Soft dig-cutaway openness in [0,1]. Radial bubble around focus_xz.
/// 0 = solid, 1 = fully see-through.
/// `confine` SHRINKS the bubble (more walled-in → tighter window on the hero).
fn dig_cutaway_open(world_pos: vec3<f32>) -> f32 {
    let conf = clamp(frame.confine, 0.0, 1.0);
    if conf < 0.03 {
        return 0.0;
    }
    let eye_under = frame.surface_y - frame.eye_y;
    if eye_under < 0.2 {
        return 0.0;
    }
    let deep_t = dig_burial_deep_t();
    // Confine shrinks the bubble (was inverted: mix(0.35, 1.0, conf)).
    let radius = (mix(3.5, 9.5, deep_t) + eye_under * 0.45) * mix(1.0, 0.35, conf);
    let d = length(world_pos.xz - frame.focus_xz);
    if d >= radius {
        return 0.0;
    }
    let t = clamp(d / max(radius, 0.001), 0.0, 1.0);
    // Long ease: open in the middle, slow fade to solid at the edge.
    let radial = pow(1.0 - t, 1.45);
    var open = radial * mix(0.50, 0.95, deep_t);
    open *= conf;
    return clamp(open, 0.0, 1.0);
}

/// Indoor house cutaway (same Bayer pipeline as dig). Opens only faces that
/// look toward the camera — front wall + roof — so the interior stays readable.
fn indoor_cutaway_open(world_pos: vec3<f32>, normal: vec3<f32>) -> f32 {
    let indoor = clamp(frame.indoors, 0.0, 1.0);
    if indoor < 0.02 {
        return 0.0;
    }
    let d = length(world_pos.xz - frame.focus_xz);
    // Local to the room — don't dissolve the whole settlement.
    let near = 1.0 - smoothstep(3.0, 8.5, d);
    if near < 0.01 {
        return 0.0;
    }
    let view = normalize(frame.camera_pos - world_pos);
    let n = normalize(normal);
    let face_cam = max(dot(n, view), 0.0);
    // Need a clear camera-facing face (walls/roof toward the lens).
    let face_t = smoothstep(0.08, 0.50, face_cam);
    return clamp(indoor * near * face_t * 0.95, 0.0, 1.0);
}

/// Unified occlusion cutaway: dig lid (burial) + indoor camera-facing shells.
/// Same Bayer dither for both — one presentation effect.
fn apply_dig_cutaway(world_pos: vec3<f32>, clip_pos: vec4<f32>, normal: vec3<f32>) -> f32 {
    var cut = 0.0;

    // Dig / canopy: lid above the eye only (existing behaviour).
    {
        let h = world_pos.y - frame.eye_y;
        if h >= -0.65 {
            let open = dig_cutaway_open(world_pos);
            let y_gate = smoothstep(-0.35, 1.85, h);
            cut = max(cut, open * y_gate);
        }
    }

    // Indoors: camera-facing walls + roof near the player.
    cut = max(cut, indoor_cutaway_open(world_pos, normal));

    if cut < 0.01 {
        return 0.0;
    }
    // Fine dither tracks the gradient (depth holes so the hero stays visible).
    let keep = pow(1.0 - cut, 1.2);
    let px = vec2<u32>(u32(clip_pos.x), u32(clip_pos.y));
    if bayer8(px) >= keep {
        discard;
    }
    return cut;
}

/// Grass matches the soft exterior crop gradient (lid band only, same as solids).
fn apply_dig_cutaway_grass(world_pos: vec3<f32>, clip_pos: vec4<f32>) {
    let h = world_pos.y - frame.eye_y;
    if h < -0.65 {
        return;
    }
    var open = dig_cutaway_open(world_pos) * smoothstep(-0.35, 1.85, h);
    let under = underground_amount(world_pos.y);
    open = max(open, smoothstep(0.08, 0.55, under) * clamp(frame.confine, 0.0, 1.0) * 0.75);
    // Indoor grass tufts near feet also punch through with a soft radial.
    let indoor = clamp(frame.indoors, 0.0, 1.0);
    if indoor > 0.02 {
        let d = length(world_pos.xz - frame.focus_xz);
        open = max(open, indoor * (1.0 - smoothstep(2.5, 7.0, d)) * 0.7);
    }
    if open < 0.01 {
        return;
    }
    let keep = pow(1.0 - open, 1.2);
    let px = vec2<u32>(u32(clip_pos.x), u32(clip_pos.y));
    if bayer8(px) >= keep {
        discard;
    }
}

/// 4×4 Bayer → ordered dither threshold in [0,1).
fn bayer4(p: vec2<u32>) -> f32 {
    let x = p.x & 3u;
    let y = p.y & 3u;
    let i = y * 4u + x;
    let m = array<u32, 16>(
        0u, 8u, 2u, 10u,
        12u, 4u, 14u, 6u,
        3u, 11u, 1u, 9u,
        15u, 7u, 13u, 5u,
    );
    return f32(m[i]) / 16.0;
}

/// 8×8 Bayer — finer gradient for the exterior crop fade.
fn bayer8(p: vec2<u32>) -> f32 {
    // Nested 4×4: cheap approx of 8×8 ordered dither.
    let a = bayer4(p);
    let b = bayer4(vec2<u32>(p.x >> 1u, p.y >> 1u));
    return fract(a * 0.75 + b * 0.25 + f32((p.x ^ p.y) & 1u) * 0.03);
}

/// Shared hero shading (not an entry point — WGSL forbids calling `@fragment` fns).
fn shade_player(in: VertexOutput) -> vec4<f32> {
    let light_dir = normalize(vec3<f32>(-0.4, 0.9, -0.2));
    let n = normalize(in.normal);
    let ndl = max(dot(n, light_dir), 0.0);
    let band = select(0.42, select(0.72, 0.92, ndl > 0.75), ndl > 0.35);
    let diffuse = mix(ndl, band, 0.55);
    let ambient = 0.28 + 0.10 * clamp(n.y, 0.0, 1.0);
    let shadow = shadow_factor(in.world_pos, n, light_dir);
    let lit = in.color * (ambient + (1.0 - ambient) * diffuse * shadow);
    return vec4<f32>(lit, 1.0);
}

/// Overlay 1..6 white dots from `seed` (DEBUG_HERO_CAMERA_FACES axis id).
fn debug_face_id_dots(in: VertexOutput, lit: vec4<f32>) -> vec4<f32> {
    var out = lit;
    let id = i32(in.seed + 0.5);
    if id >= 1 && id <= 6 {
        let u = in.uv.x;
        let v = in.uv.y;
        for (var i = 0; i < id; i = i + 1) {
            let cx = 0.12 + f32(i) * 0.14;
            let cy = 0.82;
            let d = length(vec2<f32>(u - cx, v - cy));
            if d < 0.055 {
                out = vec4<f32>(1.0, 1.0, 1.0, 1.0);
            }
        }
    }
    return out;
}

/// Opaque hero — no terrain fog / underground wire.
@fragment
fn fs_player(in: VertexOutput) -> @location(0) vec4<f32> {
    return debug_face_id_dots(in, shade_player(in));
}

/// One occluded silhouette for any nearer occluder (tree leaves OR terrain).
/// Color pass uses LessEqual after a depth-reset pass (see Rust) so hero faces
/// sort correctly while still sitting on top of the occluder color.
/// `seed` 1..6 → DEBUG_HERO_CAMERA_FACES: no Bayer.
const ENABLE_OCCLUDED_BAYER: bool = {{ENABLE_OCCLUDED_BAYER}};

/// Push depth to far under the hero silhouette (stencil 0) so the following
/// LessEqual color pass can self-occlude without losing to the tree/terrain depth.
@fragment
fn fs_player_occluded_depth_reset(_in: VertexOutput) -> @builtin(frag_depth) f32 {
    return 1.0;
}

@fragment
fn fs_player_occluded(in: VertexOutput) -> @location(0) vec4<f32> {
    let debug_faces = in.seed >= 1.0 && in.seed <= 6.5;

    // Same transparency for canopy and buried dirt — no burial-only curve.
    if ENABLE_OCCLUDED_BAYER && !debug_faces {
        let keep = 0.5;
        let px = vec2<u32>(u32(in.clip_position.x), u32(in.clip_position.y));
        if bayer4(px) >= keep {
            discard;
        }
    }

    return debug_face_id_dots(in, shade_player(in));
}

@fragment
fn fs_grass(in: VertexOutput) -> @location(0) vec4<f32> {
    // Soft elliptical contact blob — very transparent darkening.
    if in.flags > 0.5 {
        let p = (in.uv - vec2<f32>(0.5, 0.5)) * vec2<f32>(1.0, 1.35);
        let d = length(p);
        if d > 0.5 {
            discard;
        }
        apply_dig_cutaway_grass(in.world_pos, in.clip_position);
        let falloff = 1.0 - smoothstep(0.05, 0.5, d);
        // Max opacity ~12% at center; fades to 0 at the edge.
        let a = falloff * 0.12;
        let dist = length(in.world_pos.xz - frame.camera_pos.xz);
        let fog_t = 1.0 - exp(-frame.fog_density * dist);
        let rgb = mix(in.color, frame.fog_color, clamp(fog_t, 0.0, 1.0) * 0.35);
        return vec4<f32>(rgb, a);
    }

    apply_dig_cutaway_grass(in.world_pos, in.clip_position);
    // Hide distant underground grass with the same cave mask as solids.
    {
        let eye_under = max(0.0, frame.surface_y - frame.eye_y);
        let bury = max(eye_under, frame.confine * 4.0);
        let mask_on = smoothstep(0.45, 1.75, bury);
        let focus_d = length(in.world_pos.xz - frame.focus_xz);
        let frag_under = max(0.0, frame.surface_y - in.world_pos.y);
        let mask_t = smoothstep(8.0, 14.0, focus_d) * smoothstep(0.35, 1.25, frag_under) * mask_on;
        if mask_t > 0.98 {
            discard;
        }
    }
    let tex = textureSample(grass_tex, grass_samp, in.uv);
    if !grass_chroma_keep(tex.rgb) {
        discard;
    }
    var shaded = shade_lit_grass(in, tex.rgb * in.color);
    shaded.a = 1.0;
    return shaded;
}

/// Castle Story–style stair blueprint: flat yellow X-ray (drawn with depth Always).
/// Soft stripes along Y help read each step when the plan goes underground.
@fragment
fn fs_ghost(in: VertexOutput) -> @location(0) vec4<f32> {
    let band = fract(in.world_pos.y * 0.5);
    let stripe = select(1.0, 0.72, band < 0.45);
    let rgb = in.color * stripe;
    // High enough alpha to read through dirt; still translucent in open air.
    return vec4<f32>(rgb, 0.55);
}
