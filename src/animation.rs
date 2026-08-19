//! Animaciones del héroe basadas en archivos: **un JSON por animación**.
//!
//! Formato (compatible con el editor `editor/sculpt.html`):
//! ```json
//! {
//!   "version": "1.0",
//!   "kind": "animation",
//!   "id": "pick_overhead",
//!   "model": "assets/entities/hero.json",
//!   "duration_s": 0.38,
//!   "loop": false,
//!   "frames": [
//!     { "t": 0.0,  "pose": { "r_arm_x": -55.0, "r_elbow_x": 20.0 } },
//!     { "t": 0.19, "pose": { "r_arm_x": 15.0 } }
//!   ]
//! }
//! ```
//! - `t` en **segundos**; los fotogramas se interpolan linealmente.
//! - Ángulos en **grados** (mismos ejes que los sliders del editor).
//! - `model` referencia el mesh al que aplica el clip.
//! - Se aceptan alias del editor (`lArmX`, `rForearmX`, `lShinX`, …).
//!
//! Si `assets/animations/<id>.json` existe se usa el archivo; si no, la
//! animación procedural incorporada (fallback).

use crate::hero_pose::HeroPose;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Radianes/segundo de fase al caminar (`walk_phase` en player.rs).
pub const WALK_CADENCE: f32 = 4.5;
/// Radianes/segundo de fase al esprintar.
pub const SPRINT_CADENCE: f32 = 7.2;
/// Radianes/segundo de fase al picar (dig sostenido).
pub const DIG_CADENCE: f32 = 10.0;
/// Duración de un swing melee (igual a player::MELEE_SWING_SECS).
pub const MELEE_SECS: f32 = 0.38;

/// Clips que el juego busca en `assets/animations/`.
pub const CLIP_NAMES: [&str; 9] = [
    "walk",
    "sprint",
    "dig",
    "pick_overhead",
    "sword_slash_1",
    "sword_slash_2",
    "sword_slash_3",
    "fist_left",
    "fist_right",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClipFile {
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub id: String,
    /// Mesh al que se refiere la animación.
    pub model: String,
    /// Duración total en segundos (por defecto: `t` del último fotograma).
    #[serde(default)]
    pub duration_s: f32,
    #[serde(rename = "loop", default)]
    pub looped: bool,
    pub frames: Vec<FrameFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FrameFile {
    /// Tiempo del fotograma en segundos.
    pub t: f32,
    /// Articulación → grados. Claves canónicas o alias del editor.
    pub pose: BTreeMap<String, f32>,
}

fn default_version() -> String {
    "1.0".into()
}
fn default_kind() -> String {
    "animation".into()
}

/// Clip listo para muestrear (poses ya en radianes, fotogramas ordenados).
#[derive(Clone, Debug)]
pub struct AnimationClip {
    pub id: String,
    pub model: String,
    pub duration_s: f32,
    pub looped: bool,
    frames: Vec<(f32, HeroPose)>,
}

impl AnimationClip {
    pub fn from_file(file: ClipFile) -> Self {
        let mut frames: Vec<(f32, HeroPose)> = file
            .frames
            .iter()
            .map(|f| (f.t.max(0.0), pose_from_map(&f.pose)))
            .collect();
        frames.sort_by(|a, b| a.0.total_cmp(&b.0));
        let last_t = frames.last().map(|f| f.0).unwrap_or(0.0);
        let duration_s = if file.duration_s > 0.0 {
            file.duration_s.max(last_t)
        } else {
            last_t.max(1e-3)
        };
        Self {
            id: file.id,
            model: file.model,
            duration_s,
            looped: file.looped,
            frames,
        }
    }

    pub fn from_json_str(json: &str) -> Result<Self, String> {
        let file: ClipFile = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if file.frames.is_empty() {
            return Err("clip sin fotogramas".into());
        }
        Ok(Self::from_file(file))
    }

    /// Pose interpolada en `t` segundos (loop envuelve; one-shot se clampa).
    pub fn sample(&self, t: f32) -> HeroPose {
        let Some(first) = self.frames.first() else {
            return HeroPose::idle();
        };
        let dur = self.duration_s.max(1e-4);
        let t = if self.looped {
            t.rem_euclid(dur)
        } else {
            t.clamp(0.0, dur)
        };
        if t <= first.0 {
            return first.1;
        }
        for w in self.frames.windows(2) {
            let (t0, p0) = w[0];
            let (t1, p1) = w[1];
            if t <= t1 {
                let span = (t1 - t0).max(1e-6);
                return p0.lerp(p1, (t - t0) / span);
            }
        }
        let last = *self.frames.last().unwrap();
        if self.looped && dur > last.0 + 1e-6 {
            // Cierre del ciclo: último fotograma → primero.
            let span = dur - last.0;
            return last.1.lerp(first.1, (t - last.0) / span);
        }
        last.1
    }
}

/// Nombre canónico de una articulación (acepta alias del editor biped).
fn canonical_joint(name: &str) -> Option<&'static str> {
    Some(match name {
        "head_y" | "headY" => "head_y",
        "l_arm_z" | "lArmZ" => "l_arm_z",
        "r_arm_z" | "rArmZ" => "r_arm_z",
        "l_arm_x" | "lArmX" => "l_arm_x",
        "r_arm_x" | "rArmX" => "r_arm_x",
        "l_elbow_x" | "lForearmX" => "l_elbow_x",
        "r_elbow_x" | "rForearmX" => "r_elbow_x",
        "l_leg_x" | "lLegX" => "l_leg_x",
        "r_leg_x" | "rLegX" => "r_leg_x",
        "l_knee_x" | "lShinX" => "l_knee_x",
        "r_knee_x" | "rShinX" => "r_knee_x",
        "l_foot_x" | "lFootX" => "l_foot_x",
        "r_foot_x" | "rFootX" => "r_foot_x",
        _ => return None,
    })
}

fn pose_from_map(map: &BTreeMap<String, f32>) -> HeroPose {
    let mut p = HeroPose::idle();
    for (k, deg) in map {
        let Some(name) = canonical_joint(k) else {
            continue;
        };
        let rad = deg.to_radians();
        match name {
            "head_y" => p.head_y = rad,
            "l_arm_z" => p.l_arm_z = rad,
            "r_arm_z" => p.r_arm_z = rad,
            "l_arm_x" => p.l_arm_x = rad,
            "r_arm_x" => p.r_arm_x = rad,
            "l_elbow_x" => p.l_elbow_x = rad,
            "r_elbow_x" => p.r_elbow_x = rad,
            "l_leg_x" => p.l_leg_x = rad,
            "r_leg_x" => p.r_leg_x = rad,
            "l_knee_x" => p.l_knee_x = rad,
            "r_knee_x" => p.r_knee_x = rad,
            "l_foot_x" => p.l_foot_x = rad,
            "r_foot_x" => p.r_foot_x = rad,
            _ => {}
        }
    }
    p
}

fn pose_to_map(p: &HeroPose) -> BTreeMap<String, f32> {
    let deg = |r: f32| (r.to_degrees() * 1000.0).round() / 1000.0;
    let mut m = BTreeMap::new();
    m.insert("head_y".into(), deg(p.head_y));
    m.insert("l_arm_z".into(), deg(p.l_arm_z));
    m.insert("r_arm_z".into(), deg(p.r_arm_z));
    m.insert("l_arm_x".into(), deg(p.l_arm_x));
    m.insert("r_arm_x".into(), deg(p.r_arm_x));
    m.insert("l_elbow_x".into(), deg(p.l_elbow_x));
    m.insert("r_elbow_x".into(), deg(p.r_elbow_x));
    m.insert("l_leg_x".into(), deg(p.l_leg_x));
    m.insert("r_leg_x".into(), deg(p.r_leg_x));
    m.insert("l_knee_x".into(), deg(p.l_knee_x));
    m.insert("r_knee_x".into(), deg(p.r_knee_x));
    m.insert("l_foot_x".into(), deg(p.l_foot_x));
    m.insert("r_foot_x".into(), deg(p.r_foot_x));
    m
}

// ---------------------------------------------------------------------------
// Registro: carga assets/animations/<id>.json una vez, con fallback procedural.
// ---------------------------------------------------------------------------

fn clip_search_paths(rel: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(rel));
        paths.push(cwd.join("microvoxel").join(rel));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join(rel));
            paths.push(dir.join("..").join(rel));
            paths.push(dir.join("../..").join(rel));
        }
    }
    paths.push(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel));
    paths
}

fn load_registry() -> HashMap<String, AnimationClip> {
    let mut map = HashMap::new();
    for name in CLIP_NAMES {
        let rel = format!("assets/animations/{name}.json");
        for p in clip_search_paths(&rel) {
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            match AnimationClip::from_json_str(&text) {
                Ok(clip) => {
                    log::info!("animation: loaded {}", p.display());
                    map.insert(name.to_string(), clip);
                }
                Err(e) => log::warn!("animation: bad JSON {}: {e}", p.display()),
            }
            break;
        }
    }
    map
}

pub fn clip(name: &str) -> Option<&'static AnimationClip> {
    static REG: OnceLock<HashMap<String, AnimationClip>> = OnceLock::new();
    REG.get_or_init(load_registry).get(name)
}

/// Escala una pose desde reposo (idle es todo ceros → lerp = escala lineal).
fn scaled(pose: HeroPose, k: f32) -> HeroPose {
    HeroPose::idle().lerp(pose, k.clamp(0.0, 1.0))
}

// ---------------------------------------------------------------------------
// Poses con override por archivo (mismas firmas que los builtin).
// ---------------------------------------------------------------------------

pub fn walk_pose(phase: f32, amount: f32) -> HeroPose {
    match clip("walk") {
        Some(c) => scaled(c.sample(phase / WALK_CADENCE), amount),
        None => HeroPose::from_walk(phase, amount),
    }
}

pub fn run_pose(phase: f32, amount: f32) -> HeroPose {
    match clip("sprint") {
        Some(c) => scaled(c.sample(phase / SPRINT_CADENCE), amount),
        None => HeroPose::from_run(phase, amount),
    }
}

pub fn dig_pose(phase: f32, amount: f32) -> HeroPose {
    match clip("dig") {
        Some(c) => scaled(c.sample(phase / DIG_CADENCE), amount),
        None => HeroPose::from_dig(phase, amount),
    }
}

/// `t01` ∈ 0..1 dentro del swing; `timid` = golpe al aire (motion reducido).
pub fn pick_overhead_pose(t01: f32, amount: f32, timid: bool) -> HeroPose {
    match clip("pick_overhead") {
        Some(c) => {
            let k = amount * if timid { 0.55 } else { 1.0 };
            scaled(c.sample(t01.clamp(0.0, 1.0) * c.duration_s), k)
        }
        None => HeroPose::from_pick_overhead(t01, amount, timid),
    }
}

pub fn sword_slash_pose(variant: u8, t01: f32, amount: f32, timid: bool) -> HeroPose {
    let name = match variant % 3 {
        0 => "sword_slash_1",
        1 => "sword_slash_2",
        _ => "sword_slash_3",
    };
    match clip(name) {
        Some(c) => {
            let k = amount * if timid { 0.5 } else { 1.0 };
            scaled(c.sample(t01.clamp(0.0, 1.0) * c.duration_s), k)
        }
        None => HeroPose::from_sword_slash(variant, t01, amount, timid),
    }
}

pub fn fist_pose(left: bool, t01: f32, amount: f32, timid: bool) -> HeroPose {
    let name = if left { "fist_left" } else { "fist_right" };
    match clip(name) {
        Some(c) => {
            let k = amount * if timid { 0.45 } else { 1.0 };
            scaled(c.sample(t01.clamp(0.0, 1.0) * c.duration_s), k)
        }
        None => HeroPose::from_fist(left, t01, amount, timid),
    }
}

// ---------------------------------------------------------------------------
// Export: hornea las animaciones procedurales a un archivo por clip.
// ---------------------------------------------------------------------------

const HERO_MODEL: &str = "assets/entities/hero.json";

fn bake_clip(
    id: &str,
    duration_s: f32,
    looped: bool,
    n_frames: usize,
    f: impl Fn(f32) -> HeroPose,
) -> ClipFile {
    let n = n_frames.max(2);
    let frames = (0..n)
        .map(|i| {
            let t = duration_s * i as f32 / (n - 1) as f32;
            FrameFile {
                t: (t * 10000.0).round() / 10000.0,
                pose: pose_to_map(&f(t)),
            }
        })
        .collect();
    ClipFile {
        version: default_version(),
        kind: default_kind(),
        id: id.into(),
        model: HERO_MODEL.into(),
        duration_s,
        looped,
        frames,
    }
}

/// Todas las animaciones incorporadas como archivos (amount=1, sin timid;
/// el runtime escala amount/timid al muestrear).
pub fn builtin_clip_files() -> Vec<ClipFile> {
    use std::f32::consts::TAU;
    let walk_dur = TAU / WALK_CADENCE;
    let sprint_dur = TAU / SPRINT_CADENCE;
    let dig_dur = TAU / DIG_CADENCE;
    vec![
        bake_clip("walk", walk_dur, true, 13, |t| {
            HeroPose::from_walk(t * WALK_CADENCE, 1.0)
        }),
        bake_clip("sprint", sprint_dur, true, 13, |t| {
            HeroPose::from_run(t * SPRINT_CADENCE, 1.0)
        }),
        bake_clip("dig", dig_dur, true, 13, |t| {
            HeroPose::from_dig(t * DIG_CADENCE, 1.0)
        }),
        bake_clip("pick_overhead", MELEE_SECS, false, 11, |t| {
            HeroPose::from_pick_overhead(t / MELEE_SECS, 1.0, false)
        }),
        bake_clip("sword_slash_1", MELEE_SECS, false, 11, |t| {
            HeroPose::from_sword_slash(0, t / MELEE_SECS, 1.0, false)
        }),
        bake_clip("sword_slash_2", MELEE_SECS, false, 11, |t| {
            HeroPose::from_sword_slash(1, t / MELEE_SECS, 1.0, false)
        }),
        bake_clip("sword_slash_3", MELEE_SECS, false, 11, |t| {
            HeroPose::from_sword_slash(2, t / MELEE_SECS, 1.0, false)
        }),
        bake_clip("fist_left", MELEE_SECS, false, 11, |t| {
            HeroPose::from_fist(true, t / MELEE_SECS, 1.0, false)
        }),
        bake_clip("fist_right", MELEE_SECS, false, 11, |t| {
            HeroPose::from_fist(false, t / MELEE_SECS, 1.0, false)
        }),
    ]
}

/// Escribe `assets/animations/<id>.json` por cada animación incorporada.
pub fn export_builtin_clips(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut written = Vec::new();
    for file in builtin_clip_files() {
        let path = dir.join(format!("{}.json", file.id));
        let json = serde_json::to_string_pretty(&file)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&path, json)?;
        written.push(path);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose_close(a: &HeroPose, b: &HeroPose, tol: f32) -> bool {
        [
            (a.head_y, b.head_y),
            (a.l_arm_z, b.l_arm_z),
            (a.r_arm_z, b.r_arm_z),
            (a.l_arm_x, b.l_arm_x),
            (a.r_arm_x, b.r_arm_x),
            (a.l_elbow_x, b.l_elbow_x),
            (a.r_elbow_x, b.r_elbow_x),
            (a.l_leg_x, b.l_leg_x),
            (a.r_leg_x, b.r_leg_x),
            (a.l_knee_x, b.l_knee_x),
            (a.r_knee_x, b.r_knee_x),
            (a.l_foot_x, b.l_foot_x),
            (a.r_foot_x, b.r_foot_x),
        ]
        .iter()
        .all(|(x, y)| (x - y).abs() < tol)
    }

    #[test]
    fn export_builtin_animation_files() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/animations");
        let written = export_builtin_clips(&dir).expect("write animation files");
        assert_eq!(written.len(), CLIP_NAMES.len());
        for p in &written {
            assert!(p.exists(), "missing {}", p.display());
        }
    }

    #[test]
    fn clip_roundtrip_matches_builtin_at_keyframes() {
        for file in builtin_clip_files() {
            let json = serde_json::to_string(&file).unwrap();
            let clip = AnimationClip::from_json_str(&json).unwrap();
            assert_eq!(clip.model, HERO_MODEL);
            // Muestrear en cada keyframe debe devolver el pose horneado.
            for f in &file.frames {
                let sampled = clip.sample(f.t);
                let expected = pose_from_map(&f.pose);
                assert!(
                    pose_close(&sampled, &expected, 5e-3),
                    "clip {} desvía en t={}",
                    clip.id,
                    f.t
                );
            }
        }
    }

    #[test]
    fn editor_aliases_map_to_joints() {
        let json = r#"{
            "id": "test",
            "model": "assets/entities/hero.json",
            "loop": false,
            "frames": [
                { "t": 0.0, "pose": { "lArmX": 90.0, "rForearmX": 45.0, "lShinX": -30.0 } },
                { "t": 0.5, "pose": { "lArmX": 0.0, "rForearmX": 0.0, "lShinX": 0.0 } }
            ]
        }"#;
        let clip = AnimationClip::from_json_str(json).unwrap();
        let p = clip.sample(0.0);
        assert!((p.l_arm_x - 90f32.to_radians()).abs() < 1e-4);
        assert!((p.r_elbow_x - 45f32.to_radians()).abs() < 1e-4);
        assert!((p.l_knee_x + 30f32.to_radians()).abs() < 1e-4);
        // Interpolación a mitad de camino.
        let mid = clip.sample(0.25);
        assert!((mid.l_arm_x - 45f32.to_radians()).abs() < 1e-3);
    }

    #[test]
    fn loop_wraps_and_oneshot_clamps() {
        let looped = AnimationClip::from_file(ClipFile {
            version: default_version(),
            kind: default_kind(),
            id: "l".into(),
            model: "m".into(),
            duration_s: 1.0,
            looped: true,
            frames: vec![
                FrameFile {
                    t: 0.0,
                    pose: BTreeMap::from([("head_y".to_string(), 0.0f32)]),
                },
                FrameFile {
                    t: 0.5,
                    pose: BTreeMap::from([("head_y".to_string(), 10.0f32)]),
                },
            ],
        });
        // t=1.25 envuelve a 0.25 (mitad de la subida).
        let p = looped.sample(1.25);
        assert!((p.head_y - 5f32.to_radians()).abs() < 1e-3);

        let oneshot = AnimationClip::from_file(ClipFile {
            version: default_version(),
            kind: default_kind(),
            id: "o".into(),
            model: "m".into(),
            duration_s: 0.0,
            looped: false,
            frames: vec![
                FrameFile {
                    t: 0.0,
                    pose: BTreeMap::from([("head_y".to_string(), 0.0f32)]),
                },
                FrameFile {
                    t: 0.4,
                    pose: BTreeMap::from([("head_y".to_string(), 20.0f32)]),
                },
            ],
        });
        let end = oneshot.sample(99.0);
        assert!((end.head_y - 20f32.to_radians()).abs() < 1e-3);
    }
}
