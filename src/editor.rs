//! Native voxel/entity editor — scene model, transform math and persistence.
//!
//! Replaces the HTML5 editor (`editor/sculpt.html`): a scene is a plain JSON
//! file with one record per entity instance, each tagged with the **kind** of
//! entity it is (`kind`), the model it instantiates (`model`) and its
//! transform. Nothing here touches the GPU or the window — it is data plus
//! math, so it can be unit-tested on its own.
//!
//! Transform order (local → world), matching what the editor shows:
//! `scale` → `skew` (shear) → `rotation` (X, then Y, then Z) → `position`.
//!
//! Schema history: `v1` = a flat list of entities with one transform each.
//! `v2` adds [`EditorEntity::states`], so one entity can carry several visual
//! states (a fresh sword vs a worn one). A `v1` file loads unchanged: an entity
//! without `states` reads back as "one implicit state" (`states: []`), and the
//! migration only stamps the version.

use glam::{IVec3, Vec3};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Version of the scene file format (`schema_version` in the JSON).
pub const SCENE_SCHEMA_VERSION: u32 = 2;

/// Oldest scene version this build can read (migrated on load).
pub const SCENE_SCHEMA_OLDEST: u32 = 1;

/// Entity kinds offered by the picker. The `kind` field itself is free-form
/// (any tag in the JSON is accepted), these are just the shortcuts of the
/// preliminary UI.
pub const KINDS: [&str; 6] = ["mob", "prop", "npc", "item", "fx", "mark"];

/// The transform channels shared by an entity and by each of its states, so the
/// editor's step buttons can drive either one from a single step table.
///
/// Semantics are the documented ones — `scale` → `skew` → `rotation` → 
/// `position` — and a **state** transform is local: it is applied *before* the
/// entity's own transform, never instead of it. Pure helper (not serialized);
/// both owners store the four fields flat in JSON.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
    pub skew: [f32; 3],
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: one3(),
            skew: [0.0; 3],
        }
    }
}

/// One visual state of an editable entity: which mesh/palette it shows and how
/// that mesh is placed locally. An entity with `states: []` (every `v1` file)
/// behaves as a single state that inherits the entity's own `model`.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct EditorEntityState {
    /// Label shown in the state list (`normal`, `gastada`, …).
    #[serde(default)]
    pub name: String,
    /// Mesh for this state. Empty → inherit [`EditorEntity::model`].
    #[serde(default)]
    pub model: String,
    /// Palette name under `assets/palettes/`. Empty → the mesh's own palette.
    #[serde(default)]
    pub palette: String,
    /// Local transform of this state's mesh, applied before the entity's.
    #[serde(default)]
    pub position: [f32; 3],
    /// Euler degrees, applied X → Y → Z.
    #[serde(default)]
    pub rotation: [f32; 3],
    #[serde(default = "one3")]
    pub scale: [f32; 3],
    /// Shear: `x += skew.x*y`, `y += skew.y*z`, `z += skew.z*x`.
    #[serde(default)]
    pub skew: [f32; 3],
}

impl EditorEntityState {
    /// Label for the state list: the explicit name, else the mesh it shows.
    pub fn label(&self) -> &str {
        if self.name.is_empty() {
            if self.model.is_empty() {
                "sin mesh"
            } else {
                &self.model
            }
        } else {
            &self.name
        }
    }

    /// The four transform channels, copied out for read-modify-write.
    pub fn transform(&self) -> Transform {
        Transform {
            position: self.position,
            rotation: self.rotation,
            scale: self.scale,
            skew: self.skew,
        }
    }

    /// Write the four channels back (see [`Transform`] for the order).
    pub fn set_transform(&mut self, t: Transform) {
        self.position = t.position;
        self.rotation = t.rotation;
        self.scale = t.scale;
        self.skew = t.skew;
    }
}

/// One placed entity: a type tag plus a full transform.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditorEntity {
    /// Type tag: what this entity *is* (`mob`, `prop`, `npc`, …).
    pub kind: String,
    /// Model file this instance will use (`assets/entities/<model>.json`).
    #[serde(default)]
    pub model: String,
    /// Label shown in the entity list.
    #[serde(default)]
    pub name: String,
    /// World position of the local origin.
    pub position: [f32; 3],
    /// Local box size before any transform, in blocks.
    #[serde(default = "one3")]
    pub size: [f32; 3],
    /// Euler rotation in degrees, applied X → Y → Z.
    #[serde(default)]
    pub rotation: [f32; 3],
    /// Per-axis scale.
    #[serde(default = "one3")]
    pub scale: [f32; 3],
    /// Shear: `x += skew.x*y`, `y += skew.y*z`, `z += skew.z*x`.
    #[serde(default)]
    pub skew: [f32; 3],
    /// Visual states (schema v2). Empty = one implicit state that inherits
    /// `model`, which is exactly how a `v1` entity is read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<EditorEntityState>,
}

fn one3() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

impl Default for EditorEntity {
    fn default() -> Self {
        Self {
            kind: "prop".into(),
            model: "hero".into(),
            name: String::new(),
            position: [0.0, 24.0, 0.0],
            size: one3(),
            rotation: [0.0; 3],
            scale: one3(),
            skew: [0.0; 3],
            states: Vec::new(),
        }
    }
}

impl EditorEntity {
    /// New entity of `kind` at `position` with a `1×1×1` box.
    pub fn new(kind: &str, position: [f32; 3]) -> Self {
        Self {
            kind: kind.to_string(),
            position,
            ..Default::default()
        }
    }

    /// Label for the list: the explicit name, else `kind` + index-ish tag.
    pub fn label(&self) -> &str {
        if self.name.is_empty() {
            &self.kind
        } else {
            &self.name
        }
    }

    /// True when the entity owns explicit states, so the transform steps drive
    /// the active state instead of the entity itself. `states: []` (a `v1`
    /// entity) means "no states panel data", i.e. steps drive the entity.
    pub fn has_states(&self) -> bool {
        !self.states.is_empty()
    }

    /// State at `index`, if any.
    pub fn state(&self, index: usize) -> Option<&EditorEntityState> {
        self.states.get(index)
    }

    pub fn state_mut(&mut self, index: usize) -> Option<&mut EditorEntityState> {
        self.states.get_mut(index)
    }

    /// Clamp a state index to this entity: `0` when it has no states.
    pub fn clamp_state(&self, index: usize) -> usize {
        if self.states.is_empty() {
            0
        } else {
            index.min(self.states.len() - 1)
        }
    }

    /// Mesh the active state shows, falling back to the entity's own `model`.
    /// Empty state `model` means "inherit" — that is the rule that keeps states
    /// from colliding with the entity-level mesh.
    pub fn effective_model(&self, state: usize) -> &str {
        self.state(state)
            .map(|s| s.model.as_str())
            .filter(|m| !m.is_empty())
            .unwrap_or(self.model.as_str())
    }

    /// Palette of the active state, or `""` to keep the mesh's own palette.
    pub fn effective_palette(&self, state: usize) -> &str {
        self.state(state).map(|s| s.palette.as_str()).unwrap_or("")
    }

    /// The four transform channels copied out for read-modify-write.
    pub fn transform(&self) -> Transform {
        Transform {
            position: self.position,
            rotation: self.rotation,
            scale: self.scale,
            skew: self.skew,
        }
    }

    /// Write the four channels back (see [`Transform`] for the order).
    pub fn set_transform(&mut self, t: Transform) {
        self.position = t.position;
        self.rotation = t.rotation;
        self.scale = t.scale;
        self.skew = t.skew;
    }

    /// The 8 corners of the local box after the full transform.
    pub fn corners(&self) -> [Vec3; 8] {
        let h = [
            self.size[0].abs() * 0.5,
            self.size[1].abs() * 0.5,
            self.size[2].abs() * 0.5,
        ];
        let origin = Vec3::new(self.position[0], self.position[1], self.position[2]);
        let mut out = [Vec3::ZERO; 8];
        for (i, corner) in out.iter_mut().enumerate() {
            let local = Vec3::new(
                if i & 1 == 0 { -h[0] } else { h[0] },
                if i & 2 == 0 { -h[1] } else { h[1] },
                if i & 4 == 0 { -h[2] } else { h[2] },
            );
            *corner = origin + self.to_world_dir(local);
        }
        out
    }

    /// Transform a local offset (no translation): scale → skew → rotation.
    pub fn to_world_dir(&self, local: Vec3) -> Vec3 {
        let mut p = Vec3::new(
            local.x * self.scale[0],
            local.y * self.scale[1],
            local.z * self.scale[2],
        );
        // Shear from the unskewed components (pure linear map).
        let src = p;
        p.x += self.skew[0] * src.y;
        p.y += self.skew[1] * src.z;
        p.z += self.skew[2] * src.x;
        rot_x(
            rot_y(rot_z(p, self.rotation[2]), self.rotation[1]),
            self.rotation[0],
        )
    }

    /// World-space axis-aligned bounds of the transformed box.
    pub fn aabb(&self) -> (Vec3, Vec3) {
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for c in self.corners() {
            min = min.min(c);
            max = max.max(c);
        }
        (min, max)
    }

    /// Distance from `origin` to where `dir` first enters the box, or `None`
    /// when the ray misses it. Slab test, one axis at a time.
    ///
    /// The same box the marker is drawn from ([`Self::aabb`]), so what a click
    /// can pick is what the user can see. Two details are deliberate:
    ///
    /// - An axis the ray runs parallel to (`|dir| < 1e-8`) does not bound `t` if
    ///   the origin is between the planes, and misses otherwise. The classic
    ///   `1/dir` form is avoided on purpose: `0 * inf` is `NaN` and poisons the
    ///   comparisons.
    /// - A non-finite box (a zero scale on an axis, broken data) is never
    ///   picked, same as [`Self::cover_cells`]. The bounds are compared too,
    ///   not just checked for finiteness: a `NaN` in the transform survives
    ///   `aabb()` (`f32::min`/`max` skip the `NaN`) and leaves the box
    ///   inverted, which would otherwise read as a box around the origin.
    pub fn ray_hit(&self, origin: Vec3, dir: Vec3) -> Option<f32> {
        let (min, max) = self.aabb();
        if !min.is_finite() || !max.is_finite() || !min.cmple(max).all() {
            return None;
        }
        let mut t_enter = 0.0f32;
        let mut t_exit = f32::INFINITY;
        for axis in 0..3 {
            let (o, d, lo, hi) = (origin[axis], dir[axis], min[axis], max[axis]);
            if d.abs() < 1e-8 {
                if o < lo || o > hi {
                    return None;
                }
                continue;
            }
            let (t0, t1) = ((lo - o) / d, (hi - o) / d);
            let (near, far) = if t0 <= t1 { (t0, t1) } else { (t1, t0) };
            t_enter = t_enter.max(near);
            t_exit = t_exit.min(far);
            if t_enter > t_exit {
                return None;
            }
        }
        Some(t_enter)
    }

    /// Cells of the voxel grid that cover the box, at most `max_cells`.
    ///
    /// A small box is filled solid. A big one is drawn as its **shell** (the
    /// border of the AABB), which reads as a box outline instead of a solid
    /// blob, and only if even that does not fit is it sampled with a uniform
    /// stride — the extents are always covered, so the marker never lies
    /// about the entity's size.
    pub fn cover_cells(&self, max_cells: usize) -> Vec<IVec3> {
        let (min, max) = self.aabb();
        if !min.is_finite() || !max.is_finite() {
            return Vec::new();
        }
        let (x0, y0, z0) = (
            min.x.floor() as i32,
            min.y.floor() as i32,
            min.z.floor() as i32,
        );
        let (x1, y1, z1) = (
            (max.x.ceil() as i32 - 1).max(x0),
            (max.y.ceil() as i32 - 1).max(y0),
            (max.z.ceil() as i32 - 1).max(z0),
        );
        let (nx, ny, nz) = (
            (x1 - x0 + 1) as usize,
            (y1 - y0 + 1) as usize,
            (z1 - z0 + 1) as usize,
        );
        let total = nx.saturating_mul(ny).saturating_mul(nz);
        if total == 0 {
            return Vec::new();
        }
        let cap = max_cells.max(1);

        // Volume: the whole box (small boxes, and anything degenerate).
        if total <= cap {
            let mut cells = Vec::with_capacity(total);
            for x in x0..=x1 {
                for y in y0..=y1 {
                    for z in z0..=z1 {
                        cells.push(IVec3::new(x, y, z));
                    }
                }
            }
            return cells;
        }

        // Shell: only the border cells of the AABB (volume minus its inside).
        let shell: usize = total
            .saturating_sub((nx - 2).saturating_mul(ny - 2).saturating_mul(nz - 2));
        if shell <= cap {
            let mut cells = Vec::with_capacity(shell);
            for x in x0..=x1 {
                for y in y0..=y1 {
                    for z in z0..=z1 {
                        let border = x == x0
                            || x == x1
                            || y == y0
                            || y == y1
                            || z == z0
                            || z == z1;
                        if border {
                            cells.push(IVec3::new(x, y, z));
                        }
                    }
                }
            }
            return cells;
        }

        // Too big even for a shell: sample with a stride, plus the far corner
        // so the marker still reaches the end of the box.
        let step = ((total as f64 / cap as f64).cbrt()).ceil().max(2.0) as i32;
        let mut cells = Vec::new();
        let mut x = x0;
        while x <= x1 {
            let mut y = y0;
            while y <= y1 {
                let mut z = z0;
                while z <= z1 {
                    cells.push(IVec3::new(x, y, z));
                    z += step;
                }
                y += step;
            }
            x += step;
        }
        let far = IVec3::new(x1, y1, z1);
        if cells.last() != Some(&far) {
            cells.push(far);
        }
        cells
    }

    /// Marker colour for this kind. Known kinds have a fixed colour; any other
    /// tag gets a stable one derived from the string, so adding a kind to the
    /// JSON never needs a code change to stay visible.
    pub fn kind_color(&self) -> [f32; 3] {
        let known = match self.kind.as_str() {
            "mob" => [0.35, 0.85, 0.40],
            "prop" => [0.95, 0.65, 0.25],
            "npc" => [0.35, 0.65, 0.95],
            "item" => [0.95, 0.85, 0.35],
            "fx" => [0.80, 0.45, 0.95],
            "mark" => [0.95, 0.45, 0.45],
            _ => kind_hash_color(&self.kind),
        };
        known
    }
}

fn rot_x(v: Vec3, deg: f32) -> Vec3 {
    let (s, c) = deg.to_radians().sin_cos();
    Vec3::new(v.x, v.y * c - v.z * s, v.y * s + v.z * c)
}

fn rot_y(v: Vec3, deg: f32) -> Vec3 {
    let (s, c) = deg.to_radians().sin_cos();
    Vec3::new(v.x * c + v.z * s, v.y, -v.x * s + v.z * c)
}

fn rot_z(v: Vec3, deg: f32) -> Vec3 {
    let (s, c) = deg.to_radians().sin_cos();
    Vec3::new(v.x * c - v.y * s, v.x * s + v.y * c, v.z)
}

/// Stable colour for an unknown kind: FNV-1a of the tag → hue.
fn kind_hash_color(kind: &str) -> [f32; 3] {
    let mut h: u32 = 0x811c9dc5;
    for b in kind.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    let hue = (h % 360) as f32 / 360.0;
    let s = 0.55;
    let v = 0.95;
    let i = (hue * 6.0).floor();
    let f = hue * 6.0 - i;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [r, g, b]
}

/// A whole scene: the list of entities the editor is arranging.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditorScene {
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub entities: Vec<EditorEntity>,
}

impl Default for EditorScene {
    fn default() -> Self {
        Self {
            schema_version: SCENE_SCHEMA_VERSION,
            name: "scene".into(),
            entities: Vec::new(),
        }
    }
}

/// Bring a scene read from disk up to [`SCENE_SCHEMA_VERSION`].
///
/// `v1` → `v2` is a pure version stamp: `v1` has no `states`, and an entity
/// without `states` already *means* "one implicit state", so no field has to be
/// invented or dropped. Older-than-`v1` files are refused rather than guessed
/// at, and a newer-than-known version is loaded as-is (forward compatible: this
/// build ignores what it does not know).
pub fn migrate_scene(mut scene: EditorScene) -> Result<EditorScene, String> {
    if scene.schema_version < SCENE_SCHEMA_OLDEST {
        return Err(format!(
            "escena demasiado vieja (schema_version {}) — mínimo {SCENE_SCHEMA_OLDEST}",
            scene.schema_version
        ));
    }
    if scene.schema_version != SCENE_SCHEMA_VERSION {
        log::info!(
            "editor: escena migrada de schema v{} a v{}",
            scene.schema_version,
            SCENE_SCHEMA_VERSION
        );
        scene.schema_version = SCENE_SCHEMA_VERSION;
    }
    Ok(scene)
}

impl EditorScene {
    /// JSON as written to disk (2-space indent, one entity per block).
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let scene = serde_json::from_str::<EditorScene>(text).map_err(|e| e.to_string())?;
        migrate_scene(scene)
    }
}

/// Where scenes live. Android writes inside the app's external dir (the same
/// one `save` uses, so the player can pull the JSON off the device); on the
/// desktop it is `saves/editor/` next to the game.
pub fn scene_dir() -> PathBuf {
    match crate::save::external_dir() {
        Some(dir) => dir.join("editor"),
        None => PathBuf::from("saves").join("editor"),
    }
}

/// File-safe scene name: letters, digits, `-` and `_`; anything else becomes
/// `_`. Empty → `scene`. Keeps a scene name from escaping the folder.
pub fn safe_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(48)
        .collect();
    if clean.trim_matches('_').is_empty() {
        "scene".to_string()
    } else {
        clean
    }
}

/// Path for `name`, skipping to `name_2`, `name_3`… if the file already
/// exists and `avoid_clash` is set (so a save never silently overwrites a
/// scene that was not opened in this session).
pub fn scene_path(name: &str, avoid_clash: bool) -> PathBuf {
    let dir = scene_dir();
    let base = safe_name(name);
    let mut path = dir.join(format!("{base}.json"));
    if !avoid_clash {
        return path;
    }
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{base}_{n}.json"));
        n += 1;
    }
    path
}

/// Write the scene, creating the folder if needed. Returns the path written.
pub fn save_scene(scene: &EditorScene, avoid_clash: bool) -> Result<PathBuf, String> {
    let path = scene_path(&scene.name, avoid_clash);
    write_scene_to(&path, scene)?;
    Ok(path)
}

/// Write the scene to an exact path (overwrites). Used when re-saving a scene
/// that is already open in the session. The file is stamped with the current
/// schema on the way out, so a scene that reached memory as `v1` is written as
/// `v2`.
pub fn write_scene_to(path: &Path, scene: &EditorScene) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = migrate_scene(scene.clone())?.to_json();
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Path of the scene file with this name (no existence check).
pub fn scene_file(name: &str) -> PathBuf {
    scene_dir().join(format!("{}.json", safe_name(name)))
}

/// Load by scene name (without the `.json`).
pub fn load_scene(name: &str) -> Result<EditorScene, String> {
    let path = scene_file(name);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    EditorScene::from_json(&text)
}

/// Scene names present on disk, sorted (no extension).
pub fn list_scenes() -> Vec<String> {
    let mut names: Vec<String> = match std::fs::read_dir(scene_dir()) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let path = e.path();
                if path.extension()?.to_str()? != "json" {
                    return None;
                }
                Some(path.file_stem()?.to_str()?.to_string())
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-3
    }

    /// A hit distance within a hair of the expected one. `t` is a distance
    /// walked from the ray origin, so it is not the face's own coordinate.
    fn near_t(got: Option<f32>, want: f32) -> bool {
        got.is_some_and(|t| (t - want).abs() < 1e-3)
    }

    #[test]
    fn unit_box_aabb_is_the_size_around_the_position() {
        let e = EditorEntity::new("prop", [2.0, 24.0, -3.0]);
        let (min, max) = e.aabb();
        assert!(near(min, Vec3::new(1.5, 23.5, -3.5)), "{min:?}");
        assert!(near(max, Vec3::new(2.5, 24.5, -2.5)), "{max:?}");
    }

    #[test]
    fn scale_stretches_the_box() {
        let mut e = EditorEntity::new("prop", [0.0; 3]);
        e.size = [2.0, 1.0, 1.0];
        e.scale = [3.0, 1.0, 1.0];
        let (min, max) = e.aabb();
        assert!(near(max - min, Vec3::new(6.0, 1.0, 1.0)), "{:?}", max - min);
    }

    /// A ray from outside stops at the near face, not at the centre. `t` is a
    /// distance walked from `origin`, so it is not the face's coordinate.
    #[test]
    fn ray_hit_stops_at_the_near_face() {
        let e = EditorEntity::new("prop", [10.0, 0.0, 0.0]);
        // Box x in 9.5..10.5, walked from the origin: 9.5 blocks.
        assert!(near_t(e.ray_hit(Vec3::ZERO, Vec3::X), 9.5));
    }

    /// Same measurement from the other side: the box is symmetric.
    #[test]
    fn ray_hit_works_from_any_side() {
        let e = EditorEntity::new("prop", [0.0, 5.0, 0.0]);
        // Box y in 4.5..5.5, walked down from y = 20: 20 - 5.5 = 14.5.
        assert!(near_t(e.ray_hit(Vec3::new(0.0, 20.0, 0.0), -Vec3::Y), 14.5));
    }

    #[test]
    fn ray_hit_misses_beside_and_behind_the_box() {
        let e = EditorEntity::new("prop", [10.0, 0.0, 0.0]);
        // 1.5 above a box that spans y in -0.5..0.5.
        assert_eq!(e.ray_hit(Vec3::new(0.0, 1.5, 0.0), Vec3::X), None);
        // Pointing away from it.
        assert_eq!(e.ray_hit(Vec3::ZERO, -Vec3::X), None);
    }

    /// The camera inside the box still hits it, at distance 0.
    #[test]
    fn ray_hit_from_inside_is_zero() {
        let e = EditorEntity::new("prop", [0.0; 3]);
        assert_eq!(e.ray_hit(Vec3::new(0.1, 0.0, 0.0), Vec3::X), Some(0.0));
    }

    /// A ray parallel to an axis is unbounded on it, so only the other two
    /// axes decide. This is the case a `1/dir` implementation gets wrong.
    #[test]
    fn ray_hit_parallel_axis_does_not_bound_the_hit() {
        let mut e = EditorEntity::new("prop", [4.0, 0.0, 0.0]);
        e.size = [2.0, 1.0, 2.0];
        // Travelling along X at y = 100: parallel to X, but outside Y and Z.
        assert_eq!(e.ray_hit(Vec3::new(0.0, 100.0, 0.0), Vec3::X), None);
        // Travelling along X at y = 0 (inside Y and Z): the X slab decides,
        // box x in 3..5.
        assert!(near_t(e.ray_hit(Vec3::ZERO, Vec3::X), 3.0));
    }

    /// A zero scale leaves the box flat, and the flat plane is still a box: a
    /// ray in that plane hits it. What must never happen is a `NaN`.
    #[test]
    fn ray_hit_survives_a_degenerate_scale() {
        let mut e = EditorEntity::new("prop", [0.0; 3]);
        e.scale = [0.0, 1.0, 1.0];
        // Sheet on x = 0, walked from x = 5.
        assert!(near_t(e.ray_hit(Vec3::new(5.0, 0.0, 0.0), -Vec3::X), 5.0));
        // Off the sheet: no hit, and no NaN leaking out as `Some(NaN)`.
        assert_eq!(e.ray_hit(Vec3::new(5.0, 3.0, 0.0), -Vec3::X), None);
    }

    /// Broken data (NaN in the transform) is never pickable, even though
    /// `aabb()` washes the NaN out into an inverted box.
    #[test]
    fn ray_hit_rejects_a_non_finite_box() {
        let mut e = EditorEntity::new("prop", [0.0; 3]);
        e.position = [f32::NAN, 0.0, 0.0];
        assert_eq!(e.ray_hit(Vec3::ZERO, Vec3::X), None);
    }

    /// Picking uses the transformed box, not the local one: a rotated entity
    /// is hit where it is drawn.
    #[test]
    fn ray_hit_follows_the_rotation() {
        let mut e = EditorEntity::new("prop", [0.0; 3]);
        e.size = [4.0, 1.0, 1.0];
        e.rotation = [0.0, 90.0, 0.0];
        // Turned 90 deg: the long axis is now Z and X only spans -0.5..0.5, so
        // a ray down X from x = 20 enters at 0.5 — 19.5 blocks walked.
        assert!(near_t(e.ray_hit(Vec3::new(20.0, 0.0, 0.0), -Vec3::X), 19.5));
    }

    #[test]
    fn quarter_turn_swaps_the_horizontal_extents() {
        let mut e = EditorEntity::new("prop", [0.0; 3]);
        e.size = [4.0, 1.0, 2.0];
        let before = {
            let (min, max) = e.aabb();
            max - min
        };
        e.rotation = [0.0, 90.0, 0.0];
        let (min, max) = e.aabb();
        let after = max - min;
        assert!(near(before, Vec3::new(4.0, 1.0, 2.0)));
        // X and Z swap; Y untouched.
        assert!(near(after, Vec3::new(2.0, 1.0, 4.0)), "{after:?}");
    }

    #[test]
    fn skew_shears_along_one_axis() {
        let mut e = EditorEntity::new("prop", [0.0; 3]);
        e.size = [2.0, 2.0, 2.0];
        e.skew = [0.0, 0.0, 1.0]; // z += x
        let dir = e.to_world_dir(Vec3::new(1.0, 0.0, 0.0));
        assert!(near(dir, Vec3::new(1.0, 0.0, 1.0)), "{dir:?}");
        // The untouched axis stays put.
        let dir_y = e.to_world_dir(Vec3::new(0.0, 1.0, 0.0));
        assert!(near(dir_y, Vec3::new(0.0, 1.0, 0.0)), "{dir_y:?}");
    }

    #[test]
    fn rotation_and_skew_compose_in_documented_order() {
        // scale → skew → rotation, so a 90° yaw turns a sheared box too.
        let mut e = EditorEntity::new("prop", [0.0; 3]);
        e.skew = [0.0, 0.0, 1.0];
        e.rotation = [0.0, 90.0, 0.0];
        let sheared = Vec3::new(1.0, 0.0, 1.0);
        let want = rot_y(sheared, 90.0);
        assert!(near(e.to_world_dir(Vec3::X), want));
    }

    #[test]
    fn cover_cells_fills_small_boxes_and_samples_big_ones() {
        let mut e = EditorEntity::new("prop", [8.0, 24.0, 8.0]);
        e.size = [3.0, 2.0, 3.0];
        let cells = e.cover_cells(512);
        // 3×2×3 box, offset by half a block → ceil corners cover 4×3×4.
        assert!(cells.len() <= 4 * 3 * 4, "{}", cells.len());
        assert!(cells.contains(&IVec3::new(8, 24, 8)));

        // Un cubo enorme no cabe: sale su shell (borde), no el volumen.
        e.size = [400.0, 400.0, 400.0];
        let big = e.cover_cells(64);
        assert!(!big.is_empty() && big.len() <= 4000, "{}", big.len());
        // El shell incluye las caras extremas pero no el centro.
        let aabb = e.aabb();
        assert!(big.iter().any(|c| c.x == aabb.0.x.floor() as i32));
        assert!(big.iter().any(|c| c.x == (aabb.1.x.ceil() as i32 - 1)));
    }

    #[test]
    fn scene_json_round_trip() {
        let mut scene = EditorScene::default();
        scene.name = "my-scene".into();
        let mut e = EditorEntity::new("mob", [1.0, 2.0, 3.0]);
        e.rotation = [0.0, 45.0, 0.0];
        e.skew = [0.2, 0.0, 0.0];
        e.scale = [1.5, 1.0, 0.5];
        e.size = [2.0, 3.0, 1.0];
        e.model = "hero".into();
        e.name = "jefe".into();
        scene.entities.push(e.clone());
        let back = EditorScene::from_json(&scene.to_json()).expect("round trip");
        assert_eq!(back, scene);
        assert_eq!(back.entities[0].kind, "mob");
        assert_eq!(back.schema_version, SCENE_SCHEMA_VERSION);
    }

    #[test]
    fn unknown_kind_gets_a_stable_colour() {
        let a = kind_hash_color("dragón");
        let b = kind_hash_color("dragón");
        assert_eq!(a, b, "same tag must keep the same colour");
        assert!((0.0..=1.0).contains(&a[0]) && (0.0..=1.0).contains(&a[2]));
    }

    #[test]
    fn scene_names_cannot_escape_the_folder() {
        assert_eq!(safe_name("../../etc/passwd"), "______etc_passwd");
        assert_eq!(safe_name(""), "scene");
        assert_eq!(safe_name("mi escena 1"), "mi_escena_1");
        assert_eq!(safe_name("ok-name_2"), "ok-name_2");
    }

    /// Un archivo v1 tiene que seguir cargando tal cual, migrarse a v2 y al
    /// re-escribirse no ganar un `states` vacío (la forma se conserva).
    #[test]
    fn a_v1_scene_loads_migrates_and_keeps_its_shape() {
        let v1 = r#"{
            "schema_version": 1,
            "name": "mapa-viejo",
            "entities": [
                {
                    "kind": "prop",
                    "model": "hero",
                    "name": "jefe",
                    "position": [1.0, 2.0, 3.0],
                    "size": [2.0, 3.0, 1.0],
                    "rotation": [0.0, 45.0, 0.0],
                    "scale": [1.5, 1.0, 0.5],
                    "skew": [0.2, 0.0, 0.0]
                }
            ]
        }"#;
        let scene = EditorScene::from_json(v1).expect("v1 loads");
        assert_eq!(scene.schema_version, SCENE_SCHEMA_VERSION);
        assert_eq!(scene.entities[0].model, "hero");
        // Sin `states` = un estado implícito: los pasos de transform siguen
        // editando la entidad, y el mesh efectivo es el de la entidad.
        assert!(!scene.entities[0].has_states());
        assert_eq!(scene.entities[0].clamp_state(7), 0);
        assert_eq!(scene.entities[0].effective_model(0), "hero");
        assert_eq!(scene.entities[0].effective_palette(0), "");

        // Al re-escribir: versión al día y sin `states` espurio.
        let written = migrate_scene(scene.clone()).expect("migrate").to_json();
        assert!(written.contains(&format!("\"schema_version\": {SCENE_SCHEMA_VERSION}")));
        assert!(!written.contains("states"), "v1 no debe crecer un states vacío");
    }

    #[test]
    fn a_pre_v1_scene_is_refused_instead_of_guessed() {
        let bad = r#"{"schema_version": 0, "name": "x", "entities": []}"#;
        assert!(EditorScene::from_json(bad).is_err());
    }

    #[test]
    fn states_round_trip_and_inherit_the_entity_model() {
        let mut scene = EditorScene::default();
        scene.name = "estados".into();
        let mut e = EditorEntity::new("item", [0.0, 24.0, 0.0]);
        e.model = "special1_sword".into();
        e.states.push(EditorEntityState {
            name: "normal".into(),
            model: String::new(), // hereda el mesh de la entidad
            palette: "classic".into(),
            ..Default::default()
        });
        e.states.push(EditorEntityState {
            name: "gastada".into(),
            model: "assets/items/espada_worn.json".into(),
            palette: "mono".into(),
            position: [0.0, -0.25, 0.0],
            rotation: [0.0, 0.0, 12.0],
            scale: [0.9, 0.9, 0.9],
            ..Default::default()
        });
        scene.entities.push(e.clone());

        let mut back = EditorScene::from_json(&scene.to_json()).expect("round trip");
        assert_eq!(back, scene);
        let e2 = &mut back.entities[0];
        assert_eq!(e2.states.len(), 2);
        assert_eq!(e2.clamp_state(9), 1);
        // mesh propio gana; vacío hereda el de la entidad.
        assert_eq!(e2.effective_model(0), "special1_sword");
        assert_eq!(e2.effective_model(1), "assets/items/espada_worn.json");
        assert_eq!(e2.state(0).unwrap().label(), "normal");
        // El transform del estado es local, independiente del de la entidad.
        assert_eq!(e2.states[1].position, [0.0, -0.25, 0.0]);
        assert_eq!(e2.position, [0.0, 24.0, 0.0]);

        // Mismo orden documentado: scale → skew → rotation → position.
        let mut t = e2.transform();
        t.skew = [0.0, 0.0, 1.0];
        t.rotation = [0.0, 90.0, 0.0];
        e2.set_transform(t);
        assert!(near(
            e2.to_world_dir(Vec3::X),
            rot_y(Vec3::new(1.0, 0.0, 1.0), 90.0)
        ));
    }
}
