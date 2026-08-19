//! Shared voxel entity models (editor JSON → in-game mesh).
use crate::hero_pose::{
    body_part_from_name, BodyPart, HeroPivots, ELBOW_SPLIT_Y, KNEE_SPLIT_Y,
};
use glam::Vec3;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;

pub const PALETTE_LEN: usize = 32;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntityGrid {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntityVoxel {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// Palette index `0..31`.
    pub c: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EntityFile {
    pub id: String,
    pub grid: EntityGrid,
    #[serde(default = "default_foot_y")]
    pub foot_y: i32,
    #[serde(default = "default_palette_name")]
    pub palette: String,
    #[serde(default)]
    pub voxels: Vec<EntityVoxel>,
}

fn default_foot_y() -> i32 {
    6
}
fn default_palette_name() -> String {
    "classic".into()
}

#[derive(Clone, Debug, Default, Deserialize)]
struct Vec3Serde {
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    #[serde(default)]
    z: f32,
}

#[derive(Clone, Debug, Deserialize)]
struct TransformSerde {
    #[serde(default)]
    pos: Vec3Serde,
    #[serde(default = "default_scale_serde")]
    scale: Vec3Serde,
}

fn default_scale_serde() -> Vec3Serde {
    Vec3Serde {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    }
}

impl Default for TransformSerde {
    fn default() -> Self {
        Self {
            pos: Vec3Serde {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            scale: default_scale_serde(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct PartSerde {
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    offset: Vec3Serde,
    #[serde(default)]
    transform: TransformSerde,
    #[serde(default)]
    voxels: Vec<EntityVoxel>,
}

/// Editor export (`sculpt.html`) — hierarchical parts with local voxels.
#[derive(Clone, Debug, Deserialize)]
struct EntityFileParts {
    #[serde(default = "default_hero_id")]
    id: String,
    #[serde(default)]
    grid: Option<EntityGrid>,
    #[serde(default = "default_palette_name")]
    palette: String,
    #[serde(default)]
    foot_y: Option<i32>,
    parts: FxHashMap<String, PartSerde>,
}

fn default_hero_id() -> String {
    "hero".into()
}

#[derive(Clone, Debug)]
pub struct EntityModel {
    #[allow(dead_code)]
    pub id: String,
    pub foot_y: i32,
    pub top_y: i32,
    #[allow(dead_code)]
    pub palette_name: String,
    /// RGB for each of 32 slots.
    pub palette: [[f32; 3]; PALETTE_LEN],
    /// Occupancy + colour index.
    pub cells: FxHashMap<(i32, i32, i32), u8>,
    /// Limb tag per cell (hierarchical editor export). Empty → classify by Y/X.
    pub part_of: FxHashMap<(i32, i32, i32), BodyPart>,
    /// Rest-pose joint centers used by walk animation.
    pub pivots: HeroPivots,
}

fn parse_hex(hex: &str) -> Option<[f32; 3]> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&h[0..2], 16).ok()? as f32 / 255.0;
    let g = u8::from_str_radix(&h[2..4], 16).ok()? as f32 / 255.0;
    let b = u8::from_str_radix(&h[4..6], 16).ok()? as f32 / 255.0;
    Some([r, g, b])
}

pub fn load_palette_json(json: &str) -> Option<[[f32; 3]; PALETTE_LEN]> {
    let arr: Vec<String> = serde_json::from_str(json).ok()?;
    if arr.len() != PALETTE_LEN {
        return None;
    }
    let mut out = [[0.0; 3]; PALETTE_LEN];
    for (i, hex) in arr.iter().enumerate() {
        out[i] = parse_hex(hex)?;
    }
    Some(out)
}

pub fn palette_by_name(name: &str) -> [[f32; 3]; PALETTE_LEN] {
    let json = match name {
        "pastel" => include_str!("../assets/palettes/pastel.json"),
        "earth" => include_str!("../assets/palettes/earth.json"),
        "neon" => include_str!("../assets/palettes/neon.json"),
        "castle" => include_str!("../assets/palettes/castle.json"),
        "ocean" => include_str!("../assets/palettes/ocean.json"),
        "sunset" => include_str!("../assets/palettes/sunset.json"),
        "forest" => include_str!("../assets/palettes/forest.json"),
        "mono" => include_str!("../assets/palettes/mono.json"),
        "candy" => include_str!("../assets/palettes/candy.json"),
        _ => include_str!("../assets/palettes/classic.json"),
    };
    load_palette_json(json).unwrap_or([[1.0, 0.0, 1.0]; PALETTE_LEN])
}

impl EntityModel {
    pub fn from_file(file: EntityFile) -> Self {
        let palette = palette_by_name(&file.palette);
        let mut cells = FxHashMap::default();
        let mut top_y = file.foot_y;
        for v in &file.voxels {
            let c = v.c.min((PALETTE_LEN - 1) as u8);
            cells.insert((v.x, v.y, v.z), c);
            top_y = top_y.max(v.y);
        }
        Self {
            id: file.id,
            foot_y: file.foot_y,
            top_y,
            palette_name: file.palette,
            palette,
            cells,
            part_of: FxHashMap::default(),
            pivots: HeroPivots::default_biped(),
        }
    }

    pub fn from_json_str(json: &str) -> Option<Self> {
        // 1) Flat game format: `{ id, grid, foot_y, palette, voxels }`.
        if let Ok(file) = serde_json::from_str::<EntityFile>(json) {
            if !file.voxels.is_empty() {
                return Some(Self::from_file(file));
            }
        }
        // 2) Editor hierarchical export: `{ parts: { torso: { voxels, offset… } } }`.
        if let Ok(parts_file) = serde_json::from_str::<EntityFileParts>(json) {
            return Self::from_parts_file(parts_file);
        }
        None
    }

    fn from_parts_file(file: EntityFileParts) -> Option<Self> {
        if file.parts.is_empty() {
            return None;
        }
        let (baked, part_of, pivots) = bake_parts_to_design(&file.parts);
        if baked.is_empty() {
            return None;
        }
        let foot_y = file.foot_y.unwrap_or_else(|| {
            baked.iter().map(|v| v.y).min().unwrap_or(default_foot_y())
        });
        let mut model = Self::from_file(EntityFile {
            id: file.id,
            grid: file.grid.unwrap_or(EntityGrid {
                x: 16,
                y: 32,
                z: 16,
            }),
            foot_y,
            palette: file.palette,
            voxels: baked,
        });
        model.part_of = part_of;
        model.pivots = pivots;
        Some(model)
    }

    /// Limb for a design cell — tagged parts from the editor, else Y/X classify.
    pub fn part_at(&self, x: i32, y: i32, z: i32) -> BodyPart {
        self.part_of
            .get(&(x, y, z))
            .copied()
            .unwrap_or_else(|| crate::hero_pose::classify_part(x, y, z))
    }

    #[allow(dead_code)]
    pub fn color_at(&self, x: i32, y: i32, z: i32) -> Option<[f32; 3]> {
        let idx = *self.cells.get(&(x, y, z))? as usize;
        Some(self.palette[idx.min(PALETTE_LEN - 1)])
    }

    pub fn occupied(&self, x: i32, y: i32, z: i32) -> bool {
        self.cells.contains_key(&(x, y, z))
    }

    pub fn voxel_scale(&self, body_height: f32) -> f32 {
        let span = (self.top_y - self.foot_y + 1).max(1) as f32;
        body_height / span
    }

    /// Emit exposed faces at `feet`, yaw `facing` (+X = 0).
    /// Prefer [`crate::hero::for_each_hero_face`] for posed limbs.
    #[allow(dead_code)]
    pub fn for_each_face(
        &self,
        feet: Vec3,
        facing: f32,
        body_height: f32,
        mut emit: impl FnMut([f32; 3], [f32; 3], [f32; 3]),
    ) {
        let scale = self.voxel_scale(body_height);
        let forward = Vec3::new(facing.cos(), 0.0, facing.sin());
        let right = Vec3::new(-facing.sin(), 0.0, facing.cos());
        let up = Vec3::Y;
        let foot_y = self.foot_y as f32;

        let to_world = |lx: f32, ly: f32, lz: f32| -> Vec3 {
            feet + right * (lx * scale) + up * ((ly - foot_y) * scale) + forward * (-lz * scale)
        };

        for (&(x, y, z), &ci) in &self.cells {
            let color = self.palette[ci as usize];
            for &(nx, ny, nz, corners) in &crate::hero::FACE_CORNERS {
                if self.occupied(x + nx, y + ny, z + nz) {
                    continue;
                }
                let n_local = Vec3::new(nx as f32, ny as f32, nz as f32);
                let n_world =
                    (right * n_local.x + up * n_local.y + forward * (-n_local.z)).normalize_or_zero();
                let na = n_world.to_array();
                let eps = scale * 0.02;
                let mut world_corners = [[0.0f32; 3]; 4];
                for (i, c) in corners.iter().enumerate() {
                    let mut p = to_world(x as f32 + c[0], y as f32 + c[1], z as f32 + c[2]);
                    p += n_world * eps;
                    world_corners[i] = p.to_array();
                }
                crate::hero::ensure_outward_quad(&mut world_corners, na);
                for p in &world_corners {
                    emit(*p, na, color);
                }
            }
        }
    }
}

/// Bake editor part-local voxels into design-space cells (rest pose).
/// Also returns per-cell limb tags and world-space joint pivots (group origins).
fn bake_parts_to_design(
    parts: &FxHashMap<String, PartSerde>,
) -> (
    Vec<EntityVoxel>,
    FxHashMap<(i32, i32, i32), BodyPart>,
    HeroPivots,
) {
    let mut origin: FxHashMap<String, Vec3> = FxHashMap::default();
    let mut scale: FxHashMap<String, Vec3> = FxHashMap::default();

    // Resolve parents first (repeat until stable — shallow skeletons).
    // `offset` from the editor is *relative to parent* (basePos), not absolute.
    for _ in 0..parts.len().saturating_add(1) {
        let mut progressed = false;
        for (name, part) in parts {
            if origin.contains_key(name) {
                continue;
            }
            let (parent_o, parent_s) = match part.parent.as_deref() {
                None => (Vec3::ZERO, Vec3::ONE),
                Some(p) => match (origin.get(p), scale.get(p)) {
                    (Some(o), Some(s)) => (*o, *s),
                    _ => continue,
                },
            };
            let local = Vec3::new(
                part.offset.x + part.transform.pos.x,
                part.offset.y + part.transform.pos.y,
                part.offset.z + part.transform.pos.z,
            );
            origin.insert(name.clone(), parent_o + parent_s * local);
            scale.insert(
                name.clone(),
                parent_s
                    * Vec3::new(
                        part.transform.scale.x,
                        part.transform.scale.y,
                        part.transform.scale.z,
                    ),
            );
            progressed = true;
        }
        if !progressed {
            break;
        }
    }

    let mut pivots = HeroPivots::default_biped();
    for (name, &o) in &origin {
        if let Some(part) = body_part_from_name(name) {
            pivots.set(part, o);
        }
    }

    // Auto elbow / knee pivots when the export is a single arm/leg mesh.
    let has_l_forearm = parts.contains_key("lForearm");
    let has_r_forearm = parts.contains_key("rForearm");
    let has_l_shin = parts.contains_key("lShin");
    let has_r_shin = parts.contains_key("rShin");
    if !has_l_forearm {
        if let Some(&o) = origin.get("lArm") {
            let sy = scale.get("lArm").map(|s| s.y).unwrap_or(1.0);
            pivots.set(
                BodyPart::LForearm,
                o + Vec3::new(0.0, ELBOW_SPLIT_Y as f32 * sy - 0.5, 0.0),
            );
        }
    }
    if !has_r_forearm {
        if let Some(&o) = origin.get("rArm") {
            let sy = scale.get("rArm").map(|s| s.y).unwrap_or(1.0);
            pivots.set(
                BodyPart::RForearm,
                o + Vec3::new(0.0, ELBOW_SPLIT_Y as f32 * sy - 0.5, 0.0),
            );
        }
    }
    if !has_l_shin {
        if let Some(&o) = origin.get("lLeg") {
            let sy = scale.get("lLeg").map(|s| s.y).unwrap_or(1.0);
            pivots.set(
                BodyPart::LShin,
                o + Vec3::new(0.0, KNEE_SPLIT_Y as f32 * sy, 0.0),
            );
        }
    }
    if !has_r_shin {
        if let Some(&o) = origin.get("rLeg") {
            let sy = scale.get("rLeg").map(|s| s.y).unwrap_or(1.0);
            pivots.set(
                BodyPart::RShin,
                o + Vec3::new(0.0, KNEE_SPLIT_Y as f32 * sy, 0.0),
            );
        }
    }

    let mut out = Vec::new();
    let mut seen = FxHashMap::<(i32, i32, i32), u8>::default();
    let mut part_of = FxHashMap::<(i32, i32, i32), BodyPart>::default();
    for (name, part) in parts {
        let Some(&o) = origin.get(name) else {
            continue;
        };
        let s = scale.get(name).copied().unwrap_or(Vec3::ONE);
        let base = body_part_from_name(name).unwrap_or(BodyPart::Torso);
        for v in &part.voxels {
            let wx = (o.x + v.x as f32 * s.x).round() as i32;
            let wy = (o.y + v.y as f32 * s.y).round() as i32;
            let wz = (o.z + v.z as f32 * s.z).round() as i32;
            let c = v.c.min((PALETTE_LEN - 1) as u8);
            // Split single-mesh arms/legs into upper + forearm / thigh + shin.
            let body = match base {
                BodyPart::LArm if !has_l_forearm => {
                    if v.y >= ELBOW_SPLIT_Y {
                        BodyPart::LArm
                    } else {
                        BodyPart::LForearm
                    }
                }
                BodyPart::RArm if !has_r_forearm => {
                    if v.y >= ELBOW_SPLIT_Y {
                        BodyPart::RArm
                    } else {
                        BodyPart::RForearm
                    }
                }
                BodyPart::LLeg if !has_l_shin => {
                    if v.y >= KNEE_SPLIT_Y {
                        BodyPart::LLeg
                    } else {
                        BodyPart::LShin
                    }
                }
                BodyPart::RLeg if !has_r_shin => {
                    if v.y >= KNEE_SPLIT_Y {
                        BodyPart::RLeg
                    } else {
                        BodyPart::RShin
                    }
                }
                other => other,
            };
            seen.insert((wx, wy, wz), c);
            part_of.insert((wx, wy, wz), body);
        }
    }
    for ((x, y, z), c) in seen {
        out.push(EntityVoxel { x, y, z, c });
    }
    (out, part_of, pivots)
}

fn hero_json_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    paths.push(PathBuf::from("assets/entities/hero.json"));
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join("assets/entities/hero.json"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join("assets/entities/hero.json"));
            paths.push(dir.join("../assets/entities/hero.json"));
        }
    }
    paths
}

fn load_hero_from_disk() -> Option<EntityModel> {
    for path in hero_json_candidates() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        match EntityModel::from_json_str(&text) {
            Some(model) => {
                eprintln!(
                    "hero: loaded {} ({} voxels, {} rigged) from {}",
                    model.id,
                    model.cells.len(),
                    model.part_of.len(),
                    path.display()
                );
                return Some(model);
            }
            None => {
                eprintln!(
                    "hero: failed to parse {} (need flat voxels or editor parts)",
                    path.display()
                );
            }
        }
    }
    None
}

/// Hero asset: prefers `assets/entities/hero.json` on disk (no rebuild needed),
/// then the copy embedded at compile time.
pub fn hero_model() -> Option<&'static EntityModel> {
    static HERO: OnceLock<Option<EntityModel>> = OnceLock::new();
    HERO.get_or_init(|| {
        load_hero_from_disk().or_else(|| {
            let embedded = include_str!("../assets/entities/hero.json");
            let model = EntityModel::from_json_str(embedded);
            if model.is_some() {
                eprintln!("hero: using embedded assets/entities/hero.json");
            } else {
                eprintln!("hero: embedded JSON invalid — procedural fallback");
            }
            model
        })
    })
    .as_ref()
}

/// Force-reload from disk (for tools / hot-reload). Next [`hero_model`] callers in
/// this process still see the OnceLock cache — restart the game after editing.
#[allow(dead_code)]
pub fn hero_json_path_hint() -> &'static str {
    "assets/entities/hero.json"
}

fn entity_json_candidates(rel: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    paths.push(PathBuf::from(rel));
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(rel));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join(rel));
            paths.push(dir.join("../").join(rel));
        }
    }
    paths
}

fn load_entity_from_disk(rel: &str) -> Option<EntityModel> {
    for path in entity_json_candidates(rel) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(model) = EntityModel::from_json_str(&text) {
            eprintln!(
                "entity: loaded {} ({} voxels) from {}",
                model.id,
                model.cells.len(),
                path.display()
            );
            return Some(model);
        }
    }
    None
}

/// Wooden pickaxe mesh (`assets/entities/picodemadera.json`).
pub fn pickaxe_model() -> Option<&'static EntityModel> {
    static PICK: OnceLock<Option<EntityModel>> = OnceLock::new();
    PICK.get_or_init(|| {
        load_entity_from_disk("assets/entities/picodemadera.json").or_else(|| {
            let embedded = include_str!("../assets/entities/picodemadera.json");
            let model = EntityModel::from_json_str(embedded);
            if model.is_some() {
                eprintln!("entity: using embedded assets/entities/picodemadera.json");
            }
            model
        })
    })
    .as_ref()
}

/// Special sword mesh (`assets/items/special1_sword.json`).
pub fn sword_model() -> Option<&'static EntityModel> {
    static SWORD: OnceLock<Option<EntityModel>> = OnceLock::new();
    SWORD
        .get_or_init(|| {
            load_entity_from_disk("assets/items/special1_sword.json").or_else(|| {
                let embedded = include_str!("../assets/items/special1_sword.json");
                let model = EntityModel::from_json_str(embedded);
                if model.is_some() {
                    eprintln!("entity: using embedded assets/items/special1_sword.json");
                }
                model
            })
        })
        .as_ref()
}

/// Map a linear RGB colour to the nearest classic-palette index (for procedural export).
#[allow(dead_code)]
pub fn nearest_classic_index(rgb: [f32; 3]) -> u8 {
    let pal = palette_by_name("classic");
    let mut best = 0u8;
    let mut best_d = f32::MAX;
    for (i, c) in pal.iter().enumerate() {
        let dr = rgb[0] - c[0];
        let dg = rgb[1] - c[1];
        let db = rgb[2] - c[2];
        let d = dr * dr + dg * dg + db * db;
        if d < best_d {
            best_d = d;
            best = i as u8;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_palette_loads() {
        let p = palette_by_name("classic");
        assert!(p[0][0] > 0.9); // skin warm
    }

    #[test]
    fn hero_asset_parses() {
        let m = hero_model().expect("hero.json");
        assert_eq!(m.id, "hero");
        assert!(!m.cells.is_empty());
    }

    #[test]
    fn sword_asset_parses() {
        let m = sword_model().expect("special1_sword.json");
        assert!(!m.cells.is_empty());
    }

    #[test]
    fn editor_parts_format_bakes() {
        let json = r#"{
            "id": "hero",
            "palette": "classic",
            "parts": {
                "torso": {
                    "parent": null,
                    "offset": { "x": 0, "y": 20, "z": 0 },
                    "transform": { "pos": { "x": 0, "y": 0, "z": 0 }, "scale": { "x": 1, "y": 1, "z": 1 } },
                    "voxels": [ { "x": 0, "y": 0, "z": 0, "c": 3 } ]
                },
                "head": {
                    "parent": "torso",
                    "offset": { "x": 0, "y": 6, "z": 0 },
                    "transform": { "pos": { "x": 0, "y": 0, "z": 0 }, "scale": { "x": 1, "y": 1, "z": 1 } },
                    "voxels": [ { "x": 0, "y": 0, "z": 0, "c": 0 } ]
                }
            }
        }"#;
        let m = EntityModel::from_json_str(json).expect("parts");
        assert!(m.occupied(0, 20, 0));
        assert!(m.occupied(0, 26, 0));
        assert_eq!(m.part_at(0, 20, 0), BodyPart::Torso);
        assert_eq!(m.part_at(0, 26, 0), BodyPart::Head);
        assert!((m.pivots.head.y - 26.0).abs() < 0.01);
        assert!((m.pivots.torso.y - 20.0).abs() < 0.01);
    }

    #[test]
    fn baked_pivots_respect_transform_pos() {
        let json = r#"{
            "id": "hero",
            "palette": "classic",
            "parts": {
                "torso": {
                    "parent": null,
                    "offset": { "x": 0, "y": 20, "z": 0 },
                    "transform": { "pos": { "x": 0, "y": -4, "z": 0 }, "scale": { "x": 1, "y": 1, "z": 1 } },
                    "voxels": [ { "x": 0, "y": 0, "z": 0, "c": 3 } ]
                },
                "lArm": {
                    "parent": "torso",
                    "offset": { "x": -6.5, "y": 3, "z": 0 },
                    "transform": { "pos": { "x": 0, "y": -1, "z": 0 }, "scale": { "x": 1, "y": 1, "z": 1 } },
                    "voxels": [ { "x": 0, "y": 0, "z": 0, "c": 0 } ]
                }
            }
        }"#;
        let m = EntityModel::from_json_str(json).expect("parts");
        // torso origin 16, arm = 16 + (-6.5, 3-1, 0) = (-6.5, 18, 0)
        assert!((m.pivots.torso.y - 16.0).abs() < 0.01);
        assert!((m.pivots.l_arm.x + 6.5).abs() < 0.01);
        assert!((m.pivots.l_arm.y - 18.0).abs() < 0.01);
    }
}
