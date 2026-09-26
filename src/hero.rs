//! HD-2D hero — loads [`assets/entities/hero.json`] when present, else procedural fallback.
//! Local axes: X right, Y up, −Z face-forward. Foot soles sit at [`FOOT_Y`].
//! Limb motion uses the hierarchical pose from [`crate::hero_pose`].

use crate::entity_model::{nearest_classic_index, EntityModel};
use crate::hero_pose::{
    classify_part, transform_normal, transform_point, BodyPart, HeroPivots, HeroPose,
};
use glam::Vec3;
use rustc_hash::FxHashMap;

/// Extra pick tilt while held in-hand (degrees, head raised).
const PICK_HELD_PITCH_DEG: f32 = 45.0;
/// Lowest solid boot voxel Y in the design grid (procedural / default asset).
pub const FOOT_Y: i32 = 6;
/// Top of the helmet / head volume (procedural).
pub const TOP_Y: i32 = 31;

const COLORS: Colors = Colors {
    skin: 0xFDBF8A,
    shirt: 0xE67E22,
    pants: 0x2C3E50,
    boots: 0x8B4513,
    metal: 0xC0C0C0,
    gold: 0xF1C40F,
    cape: 0xE74C3C,
    sword: 0xECF0F1,
    eye: 0x111111,
    mouth: 0x8B0000,
    belt: 0x4A2E1B,
};

struct Colors {
    skin: u32,
    shirt: u32,
    pants: u32,
    boots: u32,
    metal: u32,
    gold: u32,
    cape: u32,
    sword: u32,
    eye: u32,
    mouth: u32,
    belt: u32,
}

fn rgb(c: u32) -> [f32; 3] {
    [
        ((c >> 16) & 0xff) as f32 / 255.0,
        ((c >> 8) & 0xff) as f32 / 255.0,
        (c & 0xff) as f32 / 255.0,
    ]
}

/// Procedural colour (fallback if JSON missing).
pub fn voxel_color(x: i32, y: i32, z: i32) -> Option<[f32; 3]> {
    if x.abs() <= 5 && z.abs() <= 3 && (17..=24).contains(&y) {
        return Some(rgb(COLORS.shirt));
    }
    if x.abs() == 6 && z.abs() <= 4 && (23..=24).contains(&y) {
        return Some(rgb(COLORS.metal));
    }
    if z.abs() == 4 && x.abs() <= 5 && (23..=24).contains(&y) {
        return Some(rgb(COLORS.metal));
    }
    if x.abs() <= 5 && z.abs() <= 3 && y == 16 {
        return Some(rgb(COLORS.belt));
    }
    if x == 0 && z == -4 && y == 16 {
        return Some(rgb(COLORS.gold));
    }
    if (-8..=-5).contains(&x) && z.abs() <= 2 {
        if (19..=23).contains(&y) {
            return Some(rgb(COLORS.shirt));
        }
        if (16..=18).contains(&y) {
            return Some(rgb(COLORS.skin));
        }
    }
    if (5..=8).contains(&x) && z.abs() <= 2 {
        if (19..=23).contains(&y) {
            return Some(rgb(COLORS.shirt));
        }
        if (16..=18).contains(&y) {
            return Some(rgb(COLORS.skin));
        }
    }
    if (-4..=4).contains(&x) && z.abs() <= 2 && (8..=15).contains(&y) {
        return Some(rgb(COLORS.pants));
    }
    if (-4..=4).contains(&x) && z.abs() <= 2 && (6..=7).contains(&y) {
        return Some(rgb(COLORS.boots));
    }
    if x.abs() <= 4 && (4..=6).contains(&z) && (18..=22).contains(&y) {
        return Some(rgb(COLORS.cape));
    }
    if (-6..=-4).contains(&x) && (2..=3).contains(&z) && (14..=20).contains(&y) {
        return Some(rgb(COLORS.sword));
    }
    if x == -5 && z == 2 && (y == 13 || y == 12) {
        return Some(rgb(COLORS.gold));
    }
    if x == -5 && z == 2 && y == 17 {
        return Some(rgb(COLORS.gold));
    }
    if x.abs() <= 2 && z.abs() <= 2 && (24..=26).contains(&y) {
        return Some(rgb(COLORS.skin));
    }
    let hx = x as f32;
    let hy = (y - 27) as f32;
    let hz = z as f32;
    if (hx * hx + hy * hy + hz * hz).sqrt() <= 4.5 {
        return Some(rgb(COLORS.skin));
    }
    let hel_y = (y - 30) as f32;
    if (hx * hx + hel_y * hel_y + hz * hz).sqrt() <= 5.5 && y > 27 {
        return Some(rgb(COLORS.metal));
    }
    if x.abs() == 2 && y == 28 && z == -5 {
        return Some(rgb(COLORS.eye));
    }
    if x.abs() <= 1 && y == 26 && z == -5 {
        return Some(rgb(COLORS.mouth));
    }
    None
}

fn procedural_solids() -> Vec<(i32, i32, i32, [f32; 3])> {
    let mut v = Vec::with_capacity(2048);
    for x in -7..=8 {
        for y in 0..=31 {
            for z in -7..=8 {
                if let Some(color) = voxel_color(x, y, z) {
                    v.push((x, y, z, color));
                }
            }
        }
    }
    v
}

fn active_model() -> Option<&'static EntityModel> {
    crate::entity_model::hero_model()
}

/// World-space scale so the boot→crown stack matches `body_height` blocks.
pub fn voxel_scale(body_height: f32) -> f32 {
    if let Some(m) = active_model() {
        return m.voxel_scale(body_height);
    }
    body_height / (TOP_Y - FOOT_Y + 1) as f32
}

fn active_pivots() -> HeroPivots {
    active_model()
        .map(|m| m.pivots)
        .unwrap_or_else(HeroPivots::default_biped)
}

/// How mesh-local tool axes map into design space (relative to the fist).
#[derive(Clone, Copy)]
enum ToolOrient {
    Upright,
    MinecraftSword,
    MinecraftPick,
}

impl ToolOrient {
    fn from_kind(kind: crate::items::ToolOrientKind) -> Self {
        match kind {
            crate::items::ToolOrientKind::Pickaxe => Self::MinecraftPick,
            crate::items::ToolOrientKind::Sword => Self::MinecraftSword,
            crate::items::ToolOrientKind::Upright => Self::Upright,
        }
    }

    fn map(self, v: Vec3) -> Vec3 {
        // Flip local Y so tools sit upright in the hand (grip/handle orientation).
        let v = Vec3::new(v.x, -v.y, v.z);
        match self {
            Self::Upright => v,
            Self::MinecraftSword => {
                let (y, z) = {
                    const C: f32 = 0.766;
                    const S: f32 = 0.643;
                    (v.y * C - v.z * S, v.y * S + v.z * C)
                };
                let (x, z) = {
                    const C: f32 = 0.966;
                    const S: f32 = 0.259;
                    (v.x * C + z * S, -v.x * S + z * C)
                };
                Vec3::new(x, y, z)
            }
            Self::MinecraftPick => {
                let (y, z) = {
                    const C: f32 = 0.7071;
                    const S: f32 = 0.7071;
                    (v.y * C - v.z * S, v.y * S + v.z * C)
                };
                let (x, y) = {
                    const C: f32 = 0.985;
                    const S: f32 = 0.174;
                    (v.x * C - y * S, v.x * S + y * C)
                };
                Vec3::new(x, y, z)
            }
        }
    }
}

/// Emit a held tool using [`crate::items::ToolAttach`] (from item JSON).
/// `equip_blend` 0 = drawing from hip, 1 = settled in hand.
pub fn for_each_held_tool_face(
    model: &crate::entity_model::EntityModel,
    attach: &crate::items::ToolAttach,
    feet: Vec3,
    facing: f32,
    body_height: f32,
    pose: &HeroPose,
    swing: f32,
    equip_blend: f32,
    emit: impl FnMut([f32; 3], [f32; 3], [f32; 3]),
) {
    let equip = equip_blend.clamp(0.0, 1.0);
    // Smoothstep: quick rise then settle.
    let t = equip * equip * (3.0 - 2.0 * equip);
    let mut wrist = attach.wrist_vec();
    // Draw from hip: start lower / tucked, then snap into grip.
    wrist.y += (1.0 - t) * 3.2;
    wrist.z += (1.0 - t) * -1.8;
    wrist.x += (1.0 - t) * 0.6;
    // Dig / attack: pitch the tool in-hand by swing_arc (pick default 45°).
    let swing = (swing * attach.swing_speed).clamp(0.0, 1.0) * t;
    let arc = attach.swing_arc_deg.to_radians() * swing;
    wrist.z += arc.sin() * 0.2;
    wrist.y += (1.0 - (swing * std::f32::consts::PI).cos()) * 0.1;
    // Scale pop while deploying (reads larger in-hand once settled).
    let deploy_scale = attach.scale * (0.40 + 0.60 * t);
    emit_held_tool_faces(
        model,
        feet,
        facing,
        body_height,
        pose,
        wrist,
        attach.grip_vec(),
        deploy_scale,
        ToolOrient::from_kind(attach.orient),
        held_rest_pitch(attach.orient) + arc,
        emit,
    );
}

/// In-hand only rest tilt (hotbar icons bake their own angle).
fn held_rest_pitch(kind: crate::items::ToolOrientKind) -> f32 {
    match kind {
        crate::items::ToolOrientKind::Pickaxe => -PICK_HELD_PITCH_DEG.to_radians(),
        _ => 0.0,
    }
}

pub fn for_each_held_pickaxe_face(
    feet: Vec3,
    facing: f32,
    body_height: f32,
    pose: &HeroPose,
    emit: impl FnMut([f32; 3], [f32; 3], [f32; 3]),
) {
    for_each_held_pickaxe_face_ex(feet, facing, body_height, pose, 0.0, 1.0, emit);
}

fn pickaxe_attach() -> &'static crate::items::ToolAttach {
    use std::sync::OnceLock;
    static A: OnceLock<crate::items::ToolAttach> = OnceLock::new();
    A.get_or_init(|| crate::items::spawn_wooden_pickaxe().def.attach)
}

fn sword_attach() -> &'static crate::items::ToolAttach {
    use std::sync::OnceLock;
    static A: OnceLock<crate::items::ToolAttach> = OnceLock::new();
    A.get_or_init(|| crate::items::spawn_special1_sword().def.attach)
}

pub fn for_each_held_pickaxe_face_ex(
    feet: Vec3,
    facing: f32,
    body_height: f32,
    pose: &HeroPose,
    swing: f32,
    equip_blend: f32,
    emit: impl FnMut([f32; 3], [f32; 3], [f32; 3]),
) {
    let Some(tool) = crate::entity_model::pickaxe_model() else {
        return;
    };
    for_each_held_tool_face(
        tool,
        pickaxe_attach(),
        feet,
        facing,
        body_height,
        pose,
        swing,
        equip_blend,
        emit,
    );
}

pub fn for_each_held_sword_face(
    feet: Vec3,
    facing: f32,
    body_height: f32,
    pose: &HeroPose,
    equip_blend: f32,
    emit: impl FnMut([f32; 3], [f32; 3], [f32; 3]),
) {
    let Some(tool) = crate::entity_model::sword_model() else {
        return;
    };
    for_each_held_tool_face(
        tool,
        sword_attach(),
        feet,
        facing,
        body_height,
        pose,
        0.0,
        equip_blend,
        emit,
    );
}

fn emit_held_tool_faces(
    tool: &crate::entity_model::EntityModel,
    feet: Vec3,
    facing: f32,
    body_height: f32,
    pose: &HeroPose,
    wrist_offset: Vec3,
    grip: Vec3,
    tool_scale: f32,
    orient: ToolOrient,
    // Rest tilt + swing pitch (radians) around design X.
    swing_rad: f32,
    mut emit: impl FnMut([f32; 3], [f32; 3], [f32; 3]),
) {
    let scale = voxel_scale(body_height);
    let tool_s = scale * tool_scale;
    let forward = Vec3::new(facing.cos(), 0.0, facing.sin());
    let right = Vec3::new(-facing.sin(), 0.0, facing.cos());
    let up = Vec3::Y;
    let fy = foot_y() as f32;
    let pivots = active_pivots();
    let part = BodyPart::RForearm;
    let (sc, ss) = (swing_rad.cos(), swing_rad.sin());

    let map_local = |v: Vec3| -> Vec3 {
        let v = orient.map(v);
        if swing_rad.abs() < 1e-5 {
            return v;
        }
        // Pitch around local X so the pick head arcs with the strike.
        Vec3::new(v.x, v.y * sc - v.z * ss, v.y * ss + v.z * sc)
    };

    let to_world = |design: Vec3| -> Vec3 {
        feet + right * (design.x * scale)
            + up * ((design.y - fy) * scale)
            + forward * (-design.z * scale)
    };

    let wrist = pivots.r_forearm + wrist_offset;

    for (&(x, y, z), &ci) in &tool.cells {
        let color = tool.palette[ci as usize];
        for &(nx, ny, nz, corners) in &FACE_CORNERS {
            if tool.occupied(x + nx, y + ny, z + nz) {
                continue;
            }
            let n_local = map_local(Vec3::new(nx as f32, ny as f32, nz as f32));
            let n_design = transform_normal(part, pose, n_local);
            let n_world = (right * n_design.x + up * n_design.y + forward * (-n_design.z))
                .normalize_or_zero();
            let na = n_world.to_array();
            let eps = scale * 0.02;
            let mut world_corners = [[0.0f32; 3]; 4];
            for (i, c) in corners.iter().enumerate() {
                let tc = Vec3::new(x as f32 + c[0], y as f32 + c[1], z as f32 + c[2]);
                let lc = (tc - grip) * (tool_s / scale);
                let design = wrist + map_local(lc);
                let posed = transform_point(part, pose, &pivots, design);
                let mut p = to_world(posed);
                p += n_world * eps;
                world_corners[i] = p.to_array();
            }
            ensure_outward_quad(&mut world_corners, na);
            for p in &world_corners {
                emit(*p, na, color);
            }
        }
    }
}

/// Extra icon tweaks on top of the held-tool bake (hotbar / inventory).
#[derive(Clone, Copy, Debug)]
pub struct ToolIconBake {
    /// Roll in the icon plane (degrees, CCW).
    pub roll_deg: f32,
    /// Uniform size vs fitted bounds (`1` = fill tile, `0.9` = 10% smaller).
    pub size_mul: f32,
}

impl Default for ToolIconBake {
    fn default() -> Self {
        Self {
            roll_deg: 0.0,
            size_mul: 1.0,
        }
    }
}

/// Bake the held tool mesh (same grip / scale / orient as in-world, rest pose)
/// into a square RGBA buffer (`size×size`, premultiplied-unaware straight alpha).
pub fn bake_held_tool_icon(
    model: &crate::entity_model::EntityModel,
    attach: &crate::items::ToolAttach,
    pixels: &mut [u8],
    size: u32,
    icon: ToolIconBake,
) {
    let size = size.max(8);
    let n_px = (size * size) as usize;
    assert!(pixels.len() >= n_px * 4);
    for p in pixels.iter_mut().take(n_px * 4) {
        *p = 0;
    }

    let orient = ToolOrient::from_kind(attach.orient);
    let grip = attach.grip_vec();
    let tool_scale = attach.scale.max(0.01);

    // Collect exposed quads in held design-space (no body pose / swing).
    let mut quads: Vec<([Vec3; 4], Vec3, [f32; 3])> = Vec::new();
    for (&(x, y, z), &ci) in &model.cells {
        let color = model.palette[ci as usize];
        for &(nx, ny, nz, corners) in &FACE_CORNERS {
            if model.occupied(x + nx, y + ny, z + nz) {
                continue;
            }
            let n = orient
                .map(Vec3::new(nx as f32, ny as f32, nz as f32))
                .normalize_or_zero();
            if n.length_squared() < 1e-8 {
                continue;
            }
            let mut pts = [Vec3::ZERO; 4];
            for (i, c) in corners.iter().enumerate() {
                let tc = Vec3::new(x as f32 + c[0], y as f32 + c[1], z as f32 + c[2]);
                pts[i] = orient.map((tc - grip) * tool_scale);
            }
            quads.push((pts, n, color));
        }
    }
    if quads.is_empty() {
        return;
    }

    // Fixed camera: same mesh as in hand, flattering 3/4 (HD-2D-like).
    let yaw = 0.72f32;
    let pitch = -0.48f32;
    let roll = icon.roll_deg.to_radians();
    let (yc, ys) = (yaw.cos(), yaw.sin());
    let (pc, ps) = (pitch.cos(), pitch.sin());
    let (rc, rs) = (roll.cos(), roll.sin());
    let view = |p: Vec3| -> Vec3 {
        let x = p.x * yc + p.z * ys;
        let z = -p.x * ys + p.z * yc;
        let y = p.y * pc - z * ps;
        let z = p.y * ps + z * pc;
        // Icon-plane roll (hotbar tweak).
        let (x, y) = (x * rc - y * rs, x * rs + y * rc);
        Vec3::new(x, y, z)
    };
    let view_n = |n: Vec3| -> Vec3 {
        let x = n.x * yc + n.z * ys;
        let z = -n.x * ys + n.z * yc;
        let y = n.y * pc - z * ps;
        let z = n.y * ps + z * pc;
        let (x, y) = (x * rc - y * rs, x * rs + y * rc);
        Vec3::new(x, y, z).normalize_or_zero()
    };
    let light = Vec3::new(-0.35, 0.85, -0.4).normalize_or_zero();

    let mut viewed: Vec<([Vec3; 4], Vec3, [f32; 3])> = Vec::with_capacity(quads.len());
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for (pts, n, color) in quads {
        let mut vp = [Vec3::ZERO; 4];
        for i in 0..4 {
            vp[i] = view(pts[i]);
            min = min.min(vp[i]);
            max = max.max(vp[i]);
        }
        viewed.push((vp, view_n(n), color));
    }

    let pad = 1.5f32;
    let bw = (max.x - min.x).max(0.5);
    let bh = (max.y - min.y).max(0.5);
    let span = bw.max(bh);
    let s = (size as f32 - pad * 2.0) / span * icon.size_mul.clamp(0.2, 2.0);
    let mid = (min + max) * 0.5;
    let cx = size as f32 * 0.5;
    let cy = size as f32 * 0.52;

    let to_screen = |p: Vec3| -> (f32, f32, f32) {
        (
            cx + (p.x - mid.x) * s,
            cy - (p.y - mid.y) * s,
            p.z, // larger z = closer after our yaw/pitch
        )
    };

    let mut zbuf = vec![f32::NEG_INFINITY; n_px];
    let put =
        |pixels: &mut [u8], zbuf: &mut [f32], x: i32, y: i32, z: f32, rgb: [f32; 3], shade: f32| {
            if x < 0 || y < 0 || x >= size as i32 || y >= size as i32 {
                return;
            }
            let i = (y as u32 * size + x as u32) as usize;
            if z <= zbuf[i] {
                return;
            }
            zbuf[i] = z;
            let o = i * 4;
            pixels[o] = (rgb[0] * shade * 255.0).clamp(0.0, 255.0) as u8;
            pixels[o + 1] = (rgb[1] * shade * 255.0).clamp(0.0, 255.0) as u8;
            pixels[o + 2] = (rgb[2] * shade * 255.0).clamp(0.0, 255.0) as u8;
            pixels[o + 3] = 255;
        };

    let rast_tri = |pixels: &mut [u8],
                    zbuf: &mut [f32],
                    a: (f32, f32, f32),
                    b: (f32, f32, f32),
                    c: (f32, f32, f32),
                    rgb: [f32; 3],
                    shade: f32| {
        let min_x = a.0.min(b.0).min(c.0).floor() as i32;
        let max_x = a.0.max(b.0).max(c.0).ceil() as i32;
        let min_y = a.1.min(b.1).min(c.1).floor() as i32;
        let max_y = a.1.max(b.1).max(c.1).ceil() as i32;
        let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        if area.abs() < 1e-5 {
            return;
        }
        let inv = 1.0 / area;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let w0 = ((b.0 - px) * (c.1 - py) - (b.1 - py) * (c.0 - px)) * inv;
                let w1 = ((c.0 - px) * (a.1 - py) - (c.1 - py) * (a.0 - px)) * inv;
                let w2 = 1.0 - w0 - w1;
                if w0 < -0.01 || w1 < -0.01 || w2 < -0.01 {
                    continue;
                }
                let z = w0 * a.2 + w1 * b.2 + w2 * c.2;
                put(pixels, zbuf, x, y, z, rgb, shade);
            }
        }
    };

    for (pts, n, color) in viewed {
        // Back-face cull in view space (camera looks down −Z of viewed coords… we use +z nearer).
        // After view(), +Z is toward camera-ish; cull if normal.z < 0.
        if n.z < -0.05 {
            continue;
        }
        let shade = (0.52 + 0.48 * n.dot(light).max(0.0)).clamp(0.4, 1.15);
        let a = to_screen(pts[0]);
        let b = to_screen(pts[1]);
        let c = to_screen(pts[2]);
        let d = to_screen(pts[3]);
        rast_tri(pixels, &mut zbuf, a, b, c, color, shade);
        rast_tri(pixels, &mut zbuf, a, c, d, color, shade);
    }
}

/// Design voxels tagged with their limb (editor parts, or classify fallback).
fn collect_solids_rigged() -> Vec<(i32, i32, i32, [f32; 3], BodyPart)> {
    if let Some(m) = active_model() {
        return m
            .cells
            .iter()
            .map(|(&(x, y, z), &ci)| {
                let part = m.part_at(x, y, z);
                (x, y, z, m.palette[ci as usize], part)
            })
            .collect();
    }
    procedural_solids()
        .into_iter()
        .map(|(x, y, z, c)| (x, y, z, c, classify_part(x, y, z)))
        .collect()
}

fn foot_y() -> i32 {
    active_model().map(|m| m.foot_y).unwrap_or(FOOT_Y)
}

/// Unit-cube face table: neighbor offset `(nx,ny,nz)` + 4 corners in [0,1]³.
/// Every quad is **CCW when viewed from outside** (along the outward normal).
pub const FACE_CORNERS: [(i32, i32, i32, [[f32; 3]; 4]); 6] = [
    (
        1,
        0,
        0,
        [
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0],
            [1.0, 0.0, 1.0],
        ],
    ),
    (
        -1,
        0,
        0,
        [
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0],
        ],
    ),
    (
        0,
        1,
        0,
        [
            [0.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 0.0],
        ],
    ),
    (
        0,
        -1,
        0,
        [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ],
    ),
    (
        0,
        0,
        1,
        [
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ],
    ),
    (
        0,
        0,
        -1,
        [
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        ],
    ),
];

/// Reorder a world-space quad so `(c1-c0)×(c2-c0)` aligns with `normal` (CCW outward).
/// Call after pose / `to_world` so limb mirrors cannot invert winding.
pub fn ensure_outward_quad(corners: &mut [[f32; 3]; 4], normal: [f32; 3]) {
    let p0 = Vec3::from_array(corners[0]);
    let p1 = Vec3::from_array(corners[1]);
    let p2 = Vec3::from_array(corners[2]);
    let cross = (p1 - p0).cross(p2 - p0);
    if cross.dot(Vec3::from_array(normal)) < 0.0 {
        corners.swap(1, 3);
    }
}

/// Part pairs that genuinely separate during animation (a limb swinging
/// away from its parent) — these keep the old per-part occlusion so the
/// moving limb reveals its cut cross-section instead of leaving a hole.
/// Everything else (armor plates, hair, belts, ...) is treated as a seam
/// that never separates, so its internal face against a neighboring part
/// is always hidden — this is what prevents z-fighting at those seams.
const JOINT_PAIRS: &[(BodyPart, BodyPart)] = &[
    (BodyPart::Torso, BodyPart::LArm),
    (BodyPart::Torso, BodyPart::RArm),
    (BodyPart::LArm, BodyPart::LForearm),
    (BodyPart::RArm, BodyPart::RForearm),
    (BodyPart::Torso, BodyPart::LLeg),
    (BodyPart::Torso, BodyPart::RLeg),
    (BodyPart::LLeg, BodyPart::LShin),
    (BodyPart::RLeg, BodyPart::RShin),
    (BodyPart::LShin, BodyPart::LFoot),
    (BodyPart::RShin, BodyPart::RFoot),
];

fn is_joint(a: BodyPart, b: BodyPart) -> bool {
    JOINT_PAIRS
        .iter()
        .any(|&(x, y)| (x == a && y == b) || (x == b && y == a))
}

/// Emit exposed faces of the hero at `feet`, yaw `facing` (atan2 style: +X = 0).
/// Limbs are posed hierarchically (arms / legs / feet) before world placement.
pub fn for_each_hero_face(
    feet: Vec3,
    facing: f32,
    body_height: f32,
    pose: &HeroPose,
    mut emit: impl FnMut([f32; 3], [f32; 3], [f32; 3]),
) {
    let scale = voxel_scale(body_height);
    let forward = Vec3::new(facing.cos(), 0.0, facing.sin());
    let right = Vec3::new(-facing.sin(), 0.0, facing.cos());
    let up = Vec3::Y;
    let fy = foot_y() as f32;

    let to_world = |design: Vec3| -> Vec3 {
        feet + right * (design.x * scale)
            + up * ((design.y - fy) * scale)
            + forward * (-design.z * scale)
    };

    let pivots = active_pivots();
    let solids = collect_solids_rigged();
    let mut by_part: FxHashMap<BodyPart, Vec<(i32, i32, i32, [f32; 3])>> = FxHashMap::default();
    let mut pos_to_part: FxHashMap<(i32, i32, i32), BodyPart> = FxHashMap::default();
    for &(x, y, z, color, part) in &solids {
        by_part.entry(part).or_default().push((x, y, z, color));
        pos_to_part.insert((x, y, z), part);
    }

    for (part, voxels) in &by_part {
        for &(x, y, z, color) in voxels {
            for &(nx, ny, nz, corners) in &FACE_CORNERS {
                let npos = (x + nx, y + ny, z + nz);
                if let Some(&neighbor_part) = pos_to_part.get(&npos) {
                    if neighbor_part == *part || !is_joint(*part, neighbor_part) {
                        // Same rigid piece, or a seam that never separates
                        // (armor plates, hair, belts, ...): hide the internal face.
                        continue;
                    }
                    // Otherwise this is a genuine animatable joint boundary
                    // (e.g. torso/arm) — keep it exposed so a moving limb
                    // reveals its cut cross-section instead of leaving a hole.
                }
                let n_design =
                    transform_normal(*part, pose, Vec3::new(nx as f32, ny as f32, nz as f32));
                let n_world = (right * n_design.x + up * n_design.y + forward * (-n_design.z))
                    .normalize_or_zero();
                let na = n_world.to_array();
                let eps = scale * 0.02;
                let mut world_corners = [[0.0f32; 3]; 4];
                for (i, c) in corners.iter().enumerate() {
                    let corner = Vec3::new(x as f32 + c[0], y as f32 + c[1], z as f32 + c[2]);
                    let posed = transform_point(*part, pose, &pivots, corner);
                    let mut p = to_world(posed);
                    p += n_world * eps;
                    world_corners[i] = p.to_array();
                }
                ensure_outward_quad(&mut world_corners, na);
                for p in &world_corners {
                    emit(*p, na, color);
                }
            }
        }
    }
}

/// Build the default hero entity file from the procedural sculpt (classic palette indices).
#[allow(dead_code)]
pub fn procedural_hero_file() -> crate::entity_model::EntityFile {
    use crate::entity_model::{EntityFile, EntityGrid, EntityVoxel};
    let mut voxels = Vec::new();
    for x in -7..=8 {
        for y in 0..=31 {
            for z in -7..=8 {
                if let Some(color) = voxel_color(x, y, z) {
                    voxels.push(EntityVoxel {
                        x,
                        y,
                        z,
                        c: nearest_classic_index(color),
                    });
                }
            }
        }
    }
    EntityFile {
        id: "hero".into(),
        grid: EntityGrid {
            x: 16,
            y: 32,
            z: 16,
        },
        foot_y: FOOT_Y,
        palette: "classic".into(),
        voxels,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_corners_are_ccw_outward() {
        for &(nx, ny, nz, corners) in &FACE_CORNERS {
            let n = Vec3::new(nx as f32, ny as f32, nz as f32);
            let p0 = Vec3::from_array(corners[0]);
            let p1 = Vec3::from_array(corners[1]);
            let p2 = Vec3::from_array(corners[2]);
            let cross = (p1 - p0).cross(p2 - p0);
            assert!(
                cross.dot(n) > 0.0,
                "face normal ({nx},{ny},{nz}) winding is not CCW outward"
            );
        }
    }

    #[test]
    fn ensure_outward_quad_fixes_inverted() {
        let n = [0.0, 1.0, 0.0];
        let mut bad = [
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ];
        // Reverse to CW when looking down +Y.
        bad.swap(1, 3);
        ensure_outward_quad(&mut bad, n);
        let p0 = Vec3::from_array(bad[0]);
        let p1 = Vec3::from_array(bad[1]);
        let p2 = Vec3::from_array(bad[2]);
        assert!((p1 - p0).cross(p2 - p0).dot(Vec3::from_array(n)) > 0.0);
    }

    #[test]
    fn has_skin_head_and_orange_shirt() {
        assert!(voxel_color(0, 27, 0).is_some());
        let shirt = voxel_color(0, 20, 0).unwrap();
        assert!(shirt[0] > shirt[2]);
    }

    #[test]
    fn posed_faces_stay_ccw_outward() {
        let pose = HeroPose::from_walk(1.2, 1.0);
        let mut face: Vec<([f32; 3], [f32; 3])> = Vec::with_capacity(4);
        for_each_hero_face(Vec3::ZERO, 0.7, 1.8, &pose, |p, n, _| {
            face.push((p, n));
            if face.len() == 4 {
                let p0 = Vec3::from_array(face[0].0);
                let p1 = Vec3::from_array(face[1].0);
                let p2 = Vec3::from_array(face[2].0);
                let n = Vec3::from_array(face[0].1);
                let cross = (p1 - p0).cross(p2 - p0);
                assert!(cross.dot(n) > 0.0, "posed face winding not CCW outward");
                face.clear();
            }
        });
        assert!(face.is_empty());
    }

    #[test]
    fn emits_faces() {
        let mut n = 0;
        let pose = HeroPose::idle();
        for_each_hero_face(Vec3::ZERO, 0.0, 1.8, &pose, |_, _, _| n += 1);
        assert!(n > 100);
        assert_eq!(n % 4, 0);
    }

    #[test]
    fn walk_pose_moves_limbs() {
        let pose = HeroPose::from_walk(1.2, 1.0);
        assert!(pose.l_leg_x.abs() > 0.01 || pose.r_leg_x.abs() > 0.01);
        assert!(pose.l_arm_x.abs() > 0.01 || pose.r_arm_x.abs() > 0.01);
        assert!(pose.l_knee_x < -0.01 || pose.r_knee_x < -0.01);
        assert!(pose.l_elbow_x > 0.01 && pose.r_elbow_x > 0.01);
        assert_eq!(pose.head_y, 0.0);
    }

    #[test]
    fn procedural_export_nonempty() {
        let f = procedural_hero_file();
        assert!(f.voxels.len() > 100);
    }

    #[test]
    fn bake_held_tool_icon_writes_pixels() {
        let model = crate::entity_model::pickaxe_model().expect("pickaxe");
        let attach = crate::items::spawn_wooden_pickaxe().def.attach;
        let size = 32u32;
        let mut buf = vec![0u8; (size * size * 4) as usize];
        bake_held_tool_icon(model, &attach, &mut buf, size, ToolIconBake::default());
        let opaque = buf.chunks(4).filter(|c| c[3] > 0).count();
        assert!(opaque > 40, "expected visible tool pixels, got {opaque}");
    }
}
