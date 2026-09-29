//! Shared voxel entity models (editor JSON → in-game mesh).
use crate::hero_pose::{body_part_from_name, BodyPart, HeroPivots, ELBOW_SPLIT_Y, KNEE_SPLIT_Y};
use glam::Vec3;
use indexmap::IndexMap;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const PALETTE_LEN: usize = 40;

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
    /// Palette index `0..39`.
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
    /// File-ordered on purpose: the bake walks parts in this order, so the
    /// last part in the file wins any cell or pivot two parts contend for.
    parts: IndexMap<String, PartSerde>,
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
    /// RGB for each of the `PALETTE_LEN` slots.
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

    /// The flat file this model is, rebuilt: the inverse of [`Self::from_file`].
    /// Backs the editor's mesh export.
    ///
    /// The two fields the format has no default for are measured off the cells,
    /// because the model never kept them. `foot_y` is the lowest occupied row —
    /// the same fallback `from_parts_file` uses, and what `for_each_face` stands
    /// the mesh on. `grid` is the cell AABB, at least 1 per axis: `from_file`
    /// never reads it, so a wrong value cannot corrupt a re-read, but a *missing*
    /// one is a hard deserialization error, and a box that describes nothing is
    /// worse than no box at all.
    ///
    /// `cells` is an `FxHashMap`, so the voxels are written in sorted key order:
    /// the same model must always produce the same bytes, or an export diff is
    /// noise.
    pub fn to_file(&self, id: &str) -> EntityFile {
        let mut min = (i32::MAX, i32::MAX, i32::MAX);
        let mut max = (i32::MIN, i32::MIN, i32::MIN);
        let mut keys: Vec<&(i32, i32, i32)> = self.cells.keys().collect();
        keys.sort_unstable();
        let mut voxels = Vec::with_capacity(keys.len());
        for k in keys {
            min = (min.0.min(k.0), min.1.min(k.1), min.2.min(k.2));
            max = (max.0.max(k.0), max.1.max(k.1), max.2.max(k.2));
            voxels.push(EntityVoxel {
                x: k.0,
                y: k.1,
                z: k.2,
                c: self.cells[k],
            });
        }
        let (grid, foot_y) = if voxels.is_empty() {
            (EntityGrid { x: 1, y: 1, z: 1 }, self.foot_y)
        } else {
            (
                EntityGrid {
                    x: (max.0 - min.0 + 1).max(1),
                    y: (max.1 - min.1 + 1).max(1),
                    z: (max.2 - min.2 + 1).max(1),
                },
                min.1,
            )
        };
        EntityFile {
            id: id.to_string(),
            grid,
            foot_y,
            palette: self.palette_name.clone(),
            voxels,
        }
    }

    fn from_parts_file(file: EntityFileParts) -> Option<Self> {
        if file.parts.is_empty() {
            return None;
        }
        let (baked, part_of, pivots) = bake_parts_to_design(&file.parts);
        if baked.is_empty() {
            return None;
        }
        let foot_y = file
            .foot_y
            .unwrap_or_else(|| baked.iter().map(|v| v.y).min().unwrap_or(default_foot_y()));
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

    /// Emit exposed faces at `feet`, yaw `facing` (+X = 0), resolving each
    /// cell's colour index through `palette`.
    ///
    /// `palette` is a substitution table, not a re-bake: the cells keep the
    /// indices stored in the file, and index `i` is drawn with `palette[i]`.
    /// Pass `&model.palette` for the mesh's own palette; the editor passes the
    /// active state's override (see [`preview_palette`]) so a state can be
    /// re-coloured without touching the mesh file.
    ///
    /// This is the unrigged twin of [`crate::hero::for_each_hero_face`] (same
    /// emit signature, same `ensure_outward_quad` winding): no `HeroPose`, no
    /// pivots, no joint culling. Used by the editor preview to draw an
    /// arbitrary `EntityModel` in the world.
    pub fn for_each_face(
        &self,
        palette: &[[f32; 3]; PALETTE_LEN],
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
            let color = palette[ci as usize];
            for &(nx, ny, nz, corners) in &crate::hero::FACE_CORNERS {
                if self.occupied(x + nx, y + ny, z + nz) {
                    continue;
                }
                let n_local = Vec3::new(nx as f32, ny as f32, nz as f32);
                let n_world = (right * n_local.x + up * n_local.y + forward * (-n_local.z))
                    .normalize_or_zero();
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
///
/// `parts` keeps file order (`IndexMap`) and both tie-breaks below walk it, so
/// one rule covers cells and pivots: when two parts contend, the LAST part in
/// the file wins, silently. Overlap never errors and never panics.
fn bake_parts_to_design(
    parts: &IndexMap<String, PartSerde>,
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
    // Walk `parts` (file order), not `origin`: `origin` is filled parent-first
    // by the resolve loop above, so its order is resolution order, not file
    // order. Iterating it would resolve pivot ties on a different rule than
    // the cell bake below. Two names can map to the same `BodyPart` through
    // the aliases in `body_part_from_name` (`lArm` / `l_arm` / `leftArm`), and
    // the last one in the file wins, same as a cell.
    for name in parts.keys() {
        if let (Some(part), Some(&o)) = (body_part_from_name(name), origin.get(name)) {
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
            // `parts` is an `IndexMap`, so this loop walks the file's own part
            // order. Overlap resolves by paint order: the LAST part in the file
            // wins the cell, for colour and limb tag alike. It never errors.
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

/// Write `model` out as a new flat `<id>.json` in `dir`.
///
/// The writer twin of [`load_entity_from_disk`], and shaped like
/// [`crate::editor_clip::EditorClip::save_to`]: `dir` is a parameter so a test
/// can aim it somewhere harmless, and the editor passes `assets/entities/` — the
/// first candidate `preview_model` tries for a bare name, so a mesh just
/// exported is findable by the same lookup that found the original.
///
/// Two refusals, both deliberate. An existing path is never overwritten: this
/// creates new meshes, and silently replacing a hand-made one is the one
/// mistake the editor cannot undo. An empty model is never written either — a
/// flat file with no voxels does not come back (`from_json_str` needs a
/// non-empty `voxels`), so writing it would leave a file the game ignores.
pub fn write_flat(model: &EntityModel, id: &str, dir: &Path) -> Result<PathBuf, String> {
    if id.is_empty() {
        return Err("la malla no tiene nombre".into());
    }
    if model.cells.is_empty() {
        return Err("la malla no tiene voxels".into());
    }
    let path = dir.join(format!("{}.json", crate::editor::safe_name(id)));
    if path.exists() {
        return Err(format!("{} ya existe", path.display()));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let text = serde_json::to_string_pretty(&model.to_file(id))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

fn preview_cache() -> &'static Mutex<HashMap<String, &'static EntityModel>> {
    static CACHE: OnceLock<Mutex<HashMap<String, &'static EntityModel>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn preview_palette_cache() -> &'static Mutex<HashMap<String, &'static [[f32; 3]; PALETTE_LEN]>> {
    static CACHE: OnceLock<Mutex<HashMap<String, &'static [[f32; 3]; PALETTE_LEN]>>> =
        OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Resolved palette table for an editor state, leaked so the preview can carry
/// it as a `&'static` alongside the model.
///
/// Cached because `palette_by_name` parses a JSON array on every call and the
/// preview asks once per frame. Empty name → `None`, which means "keep the
/// mesh's own palette" (the `hereda` case the ESTADO panel shows).
///
/// Unknown names fall back to `classic`, exactly like the game does for a
/// model's own `palette` field — a typo is not an error here, it is stored
/// under its own key and resolves to the fallback.
pub fn preview_palette(name: &str) -> Option<&'static [[f32; 3]; PALETTE_LEN]> {
    if name.is_empty() {
        return None;
    }
    let mut guard = preview_palette_cache().lock().ok()?;
    if let Some(&palette) = guard.get(name) {
        return Some(palette);
    }
    let leaked: &'static [[f32; 3]; PALETTE_LEN] = Box::leak(Box::new(palette_by_name(name)));
    guard.insert(name.to_string(), leaked);
    Some(leaked)
}

/// Generic model loader for the editor preview: any file the game can already
/// read, addressed by the path the scene stores in `EditorEntity::model`.
///
/// A bare name is also looked up in the two folders the built-in loaders use
/// (`assets/entities/`, `assets/items/`), because the editor's own default
/// entity carries `model: "hero"`, not a full path.
///
/// Cached per path and handed out as `&'static`: `hero.json` is 185 KB and
/// `from_json_str` rebuilds a 1000-cell `FxHashMap`, so parsing it every frame
/// would be ruinous. One leak per distinct path — same lifetime as the
/// [`OnceLock`] loaders above.
///
/// DEUDA TÉCNICA (anotada a propósito, no un descuido): la cache está indexada
/// por la cadena **pedida**, no por la ruta que resolvió. Así que
/// `preview_model("hero")` y `preview_model("assets/entities/hero.json")` son
/// dos claves y cargan dos copias del mismo modelo. No duele mientras las
/// escenas usen una sola grafía por modelo; si algún día hay muchas entidades
/// escribiendo el mismo mesh de las dos formas, se arregla pasando la ruta
/// resuelta desde `load_entity_from_disk` (hoy no la devuelve) y cacheando por
/// esa. Los tres cargadores de arriba también habría que tocar.
pub fn preview_model(rel: &str) -> Option<&'static EntityModel> {
    if rel.is_empty() {
        return None;
    }
    let mut guard = preview_cache().lock().ok()?;
    if let Some(&model) = guard.get(rel) {
        return Some(model);
    }
    let mut candidates = vec![rel.to_string()];
    if !rel.contains('/') && !rel.ends_with(".json") {
        candidates.push(format!("assets/entities/{rel}.json"));
        candidates.push(format!("assets/items/{rel}.json"));
    }
    let model = candidates
        .iter()
        .find_map(|c| load_entity_from_disk(c))?;
    let leaked: &'static EntityModel = Box::leak(Box::new(model));
    guard.insert(rel.to_string(), leaked);
    Some(leaked)
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

    /// Overlap rule: the LAST part in the file wins the cell, for colour and
    /// limb tag alike, and never errors. Both `torso` and `lArm` place a voxel
    /// on design cell (0, 20, 0); `lArm` is written last, so it must win both.
    #[test]
    fn overlapping_parts_resolve_to_the_last_part_in_the_file() {
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
                "lArm": {
                    "parent": "torso",
                    "offset": { "x": -6, "y": 3, "z": 0 },
                    "transform": { "pos": { "x": 0, "y": -3, "z": 0 }, "scale": { "x": 1, "y": 1, "z": 1 } },
                    "voxels": [ { "x": 6, "y": 0, "z": 0, "c": 0 } ]
                }
            }
        }"#;
        let m = EntityModel::from_json_str(json).expect("parts");
        // One cell, not two: the loser is dropped, not merged.
        assert_eq!(m.cells.len(), 1);
        assert_eq!(m.part_at(0, 20, 0), BodyPart::LArm, "la última parte gana");
        assert_eq!(*m.cells.get(&(0, 20, 0)).unwrap(), 0, "gana su color");
    }

    /// Same rule for pivots, not just cells. `leftArm` is an alias of `lArm`
    /// (`body_part_from_name`), so both name `LArm` and contend for one pivot.
    /// Whichever key comes last in the file owns it, so swapping the two keys
    /// swaps the pivot — that is what makes this a file-order rule and not a
    /// hash accident.
    #[test]
    fn aliased_part_names_resolve_the_pivot_by_file_order() {
        let bake = |parts: &str| {
            let json = format!(
                r#"{{
                    "id": "hero",
                    "palette": "classic",
                    "parts": {parts}
                }}"#
            );
            EntityModel::from_json_str(&json).expect("parts").pivots.l_arm
        };
        let arm = r#""lArm": {
            "parent": null,
            "offset": { "x": -6, "y": 23, "z": 0 },
            "transform": { "pos": { "x": 0, "y": 0, "z": 0 }, "scale": { "x": 1, "y": 1, "z": 1 } },
            "voxels": [ { "x": 0, "y": 0, "z": 0, "c": 3 } ] }"#;
        let alias = r#""leftArm": {
            "parent": null,
            "offset": { "x": -9, "y": 11, "z": 0 },
            "transform": { "pos": { "x": 0, "y": 0, "z": 0 }, "scale": { "x": 1, "y": 1, "z": 1 } },
            "voxels": [ { "x": 0, "y": 0, "z": 5, "c": 0 } ] }"#;
        let arm_at = Vec3::new(-6.0, 23.0, 0.0);
        let alias_at = Vec3::new(-9.0, 11.0, 0.0);
        // Each part alone owns the pivot, so neither origin is a fallback.
        assert_eq!(bake(&format!("{{ {arm} }}")), arm_at);
        assert_eq!(bake(&format!("{{ {alias} }}")), alias_at);
        // Together they contend, and the file decides: last key wins.
        assert_eq!(bake(&format!("{{ {arm}, {alias} }}")), alias_at);
        assert_eq!(bake(&format!("{{ {alias}, {arm} }}")), arm_at);
    }

    /// El cargador genérico del editor: ruta completa, nombre corto (que es lo
    /// que guarda la entidad por defecto) y cache por ruta.
    #[test]
    fn preview_model_resolves_paths_and_short_names() {
        let full = preview_model("assets/entities/hero.json").expect("hero por ruta");
        assert!(!full.cells.is_empty(), "hero.json tiene celdas");
        // Misma clave dos veces → la segunda sale de la caché (misma referencia).
        assert_eq!(
            full as *const EntityModel,
            preview_model("assets/entities/hero.json").unwrap() as *const EntityModel
        );
        // `EditorEntity::default()` guarda `model: "hero"`: el nombre corto tiene
        // que resolver al mismo modelo. OJO: es otra clave de caché, así que
        // carga su propia copia (ver `preview_model`).
        let short = preview_model("hero").expect("hero por nombre corto");
        assert_eq!(full.id, short.id);
        assert_eq!(full.cells.len(), short.cells.len());
        // Un item real del directorio de items.
        assert!(preview_model("assets/items/special1_sword.json").is_some());
        // Y lo que no existe no inventa nada.
        assert!(preview_model("no/existe/este.json").is_none());
        assert!(preview_model("").is_none());
    }

    /// `for_each_face` es la ruta de la preview: sin rig, culling de caras
    /// expuestas contra las celdas vecinas, color de paleta y escala por
    /// `body_height`.
    #[test]
    fn for_each_face_emits_the_six_outer_faces_of_a_cell() {
        let m = EntityModel::from_file(EntityFile {
            id: "t".into(),
            grid: EntityGrid { x: 4, y: 4, z: 4 },
            foot_y: 0,
            palette: "classic".into(),
            voxels: vec![EntityVoxel {
                x: 0,
                y: 0,
                z: 0,
                c: 0,
            }],
        });
        let mut faces: Vec<([f32; 3], [f32; 3])> = Vec::new();
        m.for_each_face(&m.palette, Vec3::ZERO, 0.0, 1.0, |p, n, c| {
            assert_eq!(c, m.palette[0], "el color sale de la paleta");
            faces.push((p, n));
        });
        // 6 caras × 4 vértices, y una normal por cada eje y sentido.
        assert_eq!(faces.len(), 24);
        let mut normals: Vec<[f32; 3]> = faces.iter().map(|(_, n)| *n).collect();
        normals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        normals.dedup();
        assert_eq!(normals.len(), 6, "{normals:?}");
        for axis in 0..3 {
            for sign in [-1.0f32, 1.0] {
                let mut want = [0.0; 3];
                want[axis] = sign;
                assert!(
                    normals.contains(&want),
                    "falta la normal {want:?} en {normals:?}"
                );
            }
        }
        // voxel_scale = body_height / (top_y - foot_y + 1) = 1/1, así que la
        // celda sale como un cubo de 1 bloque. Ojo con el origen: el mapeo de
        // diseño es `world = right*lx + up*ly + forward*(-lz)`, con lo que la
        // celda (0,0,0) cae en [-1,0]×[0,1]×[0,1] con yaw 0. Lo que importa es
        // la extensión, no dónde está.
        let min = faces.iter().fold(Vec3::splat(f32::MAX), |acc, (p, _)| {
            acc.min(Vec3::from_array(*p))
        });
        let max = faces.iter().fold(Vec3::splat(f32::MIN), |acc, (p, _)| {
            acc.max(Vec3::from_array(*p))
        });
        let extent = max - min;
        // El epsilon anti-z-fighting (`scale * 0.02`) engorda el AABB 0.02 por
        // lado, así que la extensión sale 1.04 y no 1.0 exactos.
        for (i, e) in [extent.x, extent.y, extent.z].into_iter().enumerate() {
            assert!(
                (e - 1.0).abs() < 0.05,
                "eje {i}: esperadas 1 bloque, medido {e} (min {min:?}, max {max:?})"
            );
        }
    }

    /// Exportar y releer: el fichero que escribe el editor tiene que volver a
    /// entrar por el cargador del juego, celda por celda, y seguir dibujando.
    /// Dos celdas sueltas (no pegadas) para que el recuento de caras sea el de
    /// dos cajas, no el de una más la cara tapada.
    #[test]
    fn write_flat_exports_a_file_the_game_reads_back_and_draws() {
        // Insertadas fuera de orden a propósito: `cells` es un hash, y el
        // export tiene que salir siempre con los mismos bytes.
        let m = EntityModel::from_file(EntityFile {
            id: "origen".into(),
            grid: EntityGrid { x: 4, y: 4, z: 4 },
            foot_y: 0,
            palette: "candy".into(),
            voxels: vec![
                EntityVoxel {
                    x: 1,
                    y: 2,
                    z: 0,
                    c: 7,
                },
                EntityVoxel { x: 0, y: 0, z: 0, c: 3 },
            ],
        });
        let dir = std::env::temp_dir().join("microvoxel_mesh_export_test");
        let _ = std::fs::remove_dir_all(&dir);
        // El id va con acento y espacio: el fichero sale saneado, el campo `id`
        // del JSON lo lleva tal cual.
        let path = write_flat(&m, "pared nueva", &dir).expect("exporta");
        assert_eq!(path, dir.join("pared_nueva.json"));

        let text = std::fs::read_to_string(&path).unwrap();
        let back = EntityModel::from_json_str(&text).expect("el juego debe releerlo");
        assert_eq!(back.cells, m.cells, "mismas celdas con sus colores");
        assert_eq!(back.foot_y, 0, "el suelo es la fila ocupada más baja");
        assert_eq!(back.palette_name, "candy");
        assert_eq!(back.id, "pared nueva");

        // `from_file` no lee `grid`, pero es obligatorio al deserializar y tiene
        // que describir lo que hay: x 0..1, y 0..2, solo z 0.
        let file: EntityFile = serde_json::from_str(&text).unwrap();
        assert_eq!((file.grid.x, file.grid.y, file.grid.z), (2, 3, 1));
        assert_eq!(
            (file.voxels[0].x, file.voxels[1].x),
            (0, 1),
            "ordenadas, no en el orden del hash: {:?}",
            file.voxels
        );

        let mut faces = 0;
        back.for_each_face(&back.palette, Vec3::ZERO, 0.0, 1.0, |_, _, _| faces += 1);
        assert_eq!(faces, 12 * 4, "dos celdas sueltas = 12 caras");

        // No pisa un fichero anterior. La segunda llamada lleva OTRA malla a
        // propósito: si escribiera, los bytes cambiarían y el assert lo vería.
        // Con la misma malla el test pasaría igual, porque el contenido sería
        // idéntico y no probaría nada.
        let antes = std::fs::read_to_string(&path).unwrap();
        let otra = EntityModel::from_file(EntityFile {
            id: "otra".into(),
            grid: EntityGrid { x: 4, y: 4, z: 4 },
            foot_y: 0,
            palette: "classic".into(),
            voxels: vec![EntityVoxel { x: 9, y: 9, z: 9, c: 1 }],
        });
        let err = write_flat(&otra, "pared nueva", &dir).unwrap_err();
        assert!(err.contains("pared_nueva.json"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            antes,
            "el fichero existente no puede cambiar"
        );
        // Malla vacía: se niega en vez de escribir algo que no se relee.
        let empty = EntityModel::from_file(EntityFile {
            id: "vacia".into(),
            grid: EntityGrid { x: 1, y: 1, z: 1 },
            foot_y: 6,
            palette: "classic".into(),
            voxels: vec![],
        });
        assert!(write_flat(&empty, "vacia", &dir).is_err());
        assert!(!dir.join("vacia.json").exists());
        assert!(write_flat(&m, "", &dir).is_err(), "un id vacío no es un nombre");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Una celda pegada a otra oculta la cara compartida: es el mismo culling
    /// que usa el héroe, y es lo que hace que un modelo sea sólido por dentro.
    #[test]
    fn for_each_face_hides_faces_between_neighbours() {
        let m = EntityModel::from_file(EntityFile {
            id: "t".into(),
            grid: EntityGrid { x: 4, y: 4, z: 4 },
            foot_y: 0,
            palette: "classic".into(),
            voxels: vec![
                EntityVoxel { x: 0, y: 0, z: 0, c: 0 },
                EntityVoxel { x: 1, y: 0, z: 0, c: 0 },
            ],
        });
        let mut count = 0;
        m.for_each_face(&m.palette, Vec3::ZERO, 0.0, 1.0, |_, _, _| {
            count += 1
        });
        // 2 celdas pegadas: 10 caras en vez de 12 (la de contacto no sale).
        assert_eq!(count, 10 * 4);
    }

    /// La paleta que se pasa es una tabla de sustitución: el índice que guarda
    /// la celda no se toca, solo el RGB que sale por ese índice. Es lo que
    /// permite que un estado cambie de color sin rehornear el mesh.
    #[test]
    fn for_each_face_takes_a_palette_substitution() {
        let m = EntityModel::from_file(EntityFile {
            id: "t".into(),
            grid: EntityGrid { x: 4, y: 4, z: 4 },
            foot_y: 0,
            palette: "classic".into(),
            voxels: vec![EntityVoxel {
                x: 0,
                y: 0,
                z: 0,
                c: 5,
            }],
        });
        let own = m.palette[5];
        let mono = palette_by_name("mono");
        assert_ne!(own, mono[5], "si fueran iguales el test no probaría nada");

        let collect = |pal: &[[f32; 3]; PALETTE_LEN]| -> Vec<[f32; 3]> {
            let mut colors = Vec::new();
            m.for_each_face(pal, Vec3::ZERO, 0.0, 1.0, |_, _, c| colors.push(c));
            colors
        };
        // Con la suya: el RGB del índice 5 de la paleta del mesh.
        assert!(collect(&m.palette).iter().all(|c| *c == own));
        // Con otra: el RGB del índice 5 de esa otra, mismo índice.
        assert!(collect(&mono).iter().all(|c| *c == mono[5]));
        // Y la geometría no se mueve por cambiar la paleta.
        assert_eq!(collect(&m.palette).len(), collect(&mono).len());

        // La cache de paletas: mismo puntero, y el mismo fallback que el juego.
        let a = preview_palette("mono").expect("mono");
        assert_eq!(a as *const _, preview_palette("mono").unwrap() as *const _);
        assert_eq!(*a, palette_by_name("mono"));
        assert!(preview_palette("").is_none(), "vacío = hereda el mesh");
        assert_eq!(
            *preview_palette("no-existe").expect("fallback"),
            palette_by_name("classic")
        );
    }

    /// Cada paleta en disco tiene exactamente `PALETTE_LEN` entradas, todas
    /// hex de 6 dígitos. Sin esto, un archivo que no cuadre hace que
    /// `load_palette_json` devuelva `None` y `palette_by_name` caiga al
    /// fallback magenta, sin decir nada.
    #[test]
    fn every_palette_json_has_palette_len_entries() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/palettes");
        let entries = std::fs::read_dir(&dir).expect("assets/palettes");
        let mut checked = 0;
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("?");
            let text = std::fs::read_to_string(&path).expect("leer paleta");
            let arr: Vec<String> = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("{name}: JSON inválido: {e}"));
            assert_eq!(
                arr.len(),
                PALETTE_LEN,
                "{name} tiene {} entradas, hacen falta {PALETTE_LEN}",
                arr.len()
            );
            for (i, hex) in arr.iter().enumerate() {
                assert!(
                    hex.len() == 7 && hex.starts_with('#'),
                    "{name}[{i}] = {hex:?} no es #RRGGBB"
                );
            }
            checked += 1;
        }
        assert!(checked >= 10, "esperaba al menos 10 paletas, vi {checked}");
    }

    /// Ningún `c` de los assets apunta fuera de la paleta. `from_file` recorta
    /// con `.min(PALETTE_LEN - 1)`, así que un índice fuera de rango no
    /// revienta: se lee como el último color y nadie se entera.
    #[test]
    fn no_asset_voxel_points_outside_the_palette() {
        fn walk(v: &serde_json::Value, limit: u64, name: &str, seen: &mut usize) {
            match v {
                serde_json::Value::Object(map) => {
                    if let Some(c) = map.get("c").and_then(|x| x.as_u64()) {
                        assert!(
                            c < limit,
                            "{name}: un vóxel tiene c={c}, el tope es {limit}"
                        );
                        *seen += 1;
                    }
                    for child in map.values() {
                        walk(child, limit, name, seen);
                    }
                }
                serde_json::Value::Array(items) => {
                    for child in items {
                        walk(child, limit, name, seen);
                    }
                }
                _ => {}
            }
        }
        let mut files = 0;
        for root in ["assets/entities", "assets/items"] {
            let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(root);
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("?");
                let text = std::fs::read_to_string(&path).expect("leer asset");
                let value: serde_json::Value = serde_json::from_str(&text)
                    .unwrap_or_else(|e| panic!("{name}: JSON inválido: {e}"));
                let mut seen = 0usize;
                walk(&value, PALETTE_LEN as u64, name, &mut seen);
                if seen > 0 {
                    files += 1;
                }
            }
        }
        assert!(files >= 3, "esperaba al menos 3 mallas con vóxeles, vi {files}");
    }
}
