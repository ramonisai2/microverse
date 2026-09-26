//! App loop: Input → player/tool state → mining Hit → dig → dirty → remesh → GPU.
pub mod animation;
pub mod biomes;
pub mod camera;
pub mod caves;
pub mod combat;
pub mod editor;
pub mod editor_clip;
pub mod entity_model;
pub mod hero;
pub mod hero_pose;
pub mod hud;
pub mod inventory;
pub mod items;
pub mod mining;
pub mod player;
pub mod prefab;
pub mod realms;
pub mod render;
pub mod save;
pub mod settlements;
pub mod stair_dig;
pub mod touch;
pub mod world;

use camera::{Camera, HeldKeys, MAX_TICKS_PER_FRAME, TICKS_PER_SECOND};
use editor::{EditorEntity, EditorScene};
use hud::{
    append_hud, build_compass_hud, build_hotbar_hud, build_inventory_hud, build_loading_hud,
    build_stair_hud, build_status_hud, hit_test, EditorAction, EditorPanel, Hotbar, HudAction,
    HudMesh, MENU_ROWS,
};
use inventory::{InvItem, PlayerInventory};
use items::{bare_hand_can_mine, bare_hand_dig_interval, spawn_wooden_pickaxe, ToolInstance};
use mining::{
    break_solid_cell, break_solid_cell_bare, break_solid_cell_dulled, raycast_reach,
    BreakOutcome, MiningProgress,
};
use player::Player;
use player::{MAX_BOTTLES, MAX_HEARTS};
use render::Renderer;
use stair_dig::{PadDir, StairPhase, StairTool, TunnelFacing, TunnelIncline, STAIR_DIG_INTERVAL};
use std::sync::Arc;
use std::time::Instant;
use touch::{TouchAction, TouchControls};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};
use world::{
    fragment_drop_for, Material, OreDrop, Voxel, World, DEBUG_FACE_VISIBILITY, ENABLE_HD2D,
};

// Auto-FP: entrar en 1ª persona rápido, salir algo más tarde, para que las
// zonas donde confine/indoors oscila frame a frame no reinicien el blend.
// T5 restaurado a petición (solo V manual se sintió como "FP desaparecida").
const AUTO_FP_ENTER_DEBOUNCE: f32 = 0.18;
const AUTO_FP_EXIT_DEBOUNCE: f32 = 0.30;
// Hitch sensor for the HD-2D lens pull-in: a frame at/above SPIKE slams
// pressure to 1 (fast attack); anything below bleeds off proportionally to
// its headroom (slow release, faster when smooth). No hold band — an earlier
// version held pressure between OK and SPIKE, which pinned the lens in
// forever whenever the steady frame time sat in that band.
const HITCH_SPIKE_MS: f32 = 25.0;
const HITCH_RELEASE_SECS: f32 = 1.5;
// Mirada táctil en 1ª persona: el dedo recorre cientos de píxeles por gesto
// pero `apply_fp_turn` está afinado para deltas de ratón (pocos px). Sin
// boost, girar en el móvil se siente lento/pesado.
const TOUCH_FP_LOOK_BOOST: f32 = 3.0;

/// One sensor step: pure so it can be unit-tested. `inst_ms` is this frame's
/// time, `dt_secs` the frame delta. Returns pressure clamped to 0..=1.
fn hitch_pressure_step(prev: f32, inst_ms: f32, dt_secs: f32) -> f32 {
    if inst_ms >= HITCH_SPIKE_MS {
        return 1.0;
    }
    let headroom = ((HITCH_SPIKE_MS - inst_ms) / HITCH_SPIKE_MS).clamp(0.0, 1.0);
    (prev - dt_secs * (0.5 + 2.0 * headroom) / HITCH_RELEASE_SECS).max(0.0)
}

/// Top-level screen. The world is only streamed while `Playing`, so the menu
/// boots instantly (and Android never pays the preload before the user picks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen {
    /// First-screen selector (new game / saved game / editor).
    Menu,
    /// The world is being streamed and played.
    Playing,
    /// Native voxel editor (port of the old `editor/*.html`).
    Editor,
}

// ── Editor nativo ────────────────────────────────────────────────────────────

/// Blocks per second the editor focus flies (sprint doubles it).
const ED_FLY_SPEED: f32 = 14.0;
/// Fly height limits, so the focus can't be dumped under the world.
const ED_FLY_MIN_Y: f32 = 2.0;
const ED_FLY_MAX_Y: f32 = 60.0;
/// Distance from the camera at which an entity stops drawing its marker.
const ED_MARKER_RANGE: f32 = 72.0;
/// Cells per entity marker, and the ceiling for the whole frame.
const ED_CELLS_PER_ENTITY: usize = 192;
const ED_MAX_MARKER_CELLS: usize = 1200;
/// Transform step sizes (one button press each).
const ED_POS_STEP: f32 = 0.5;
const ED_ROT_STEP: f32 = 15.0;
const ED_SCALE_STEP: f32 = 0.1;
const ED_SKEW_STEP: f32 = 0.1;
const ED_SIZE_STEP: f32 = 0.5;
const ED_SCALE_RANGE: (f32, f32) = (0.1, 6.0);
const ED_SKEW_LIMIT: f32 = 2.0;
const ED_SIZE_RANGE: (f32, f32) = (0.5, 24.0);
/// How far from the spawn an entity may be placed.
const ED_POS_LIMIT: f32 = 512.0;
/// Undo depth of the editor document (scene + clip). Oldest entry is dropped.
const ED_UNDO_MAX: usize = 100;
/// Seconds one `KeyframeTime*` press moves the active keyframe.
const ED_KEYFRAME_TIME_STEP: f32 = 0.05;

/// What the transform steppers drive right now.
///
/// Polymorphic by design (`docs/plan_fase6.md` §4): the entity/state level
/// edits a `Transform` in blocks, the animation level edits **one joint of the
/// active keyframe**, in degrees, clamped by its own range. No payload: the
/// keyframe and joint always come from `kf_sel` / `joint_sel`, so moving a
/// cursor can't leave a stale target behind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StepTarget {
    /// The entity, or its active state when the entity owns states.
    EntityOrState,
    /// One joint of `kf_sel` in the open clip.
    Joint,
}

/// What an action does to the undo history. One source of truth, so the policy
/// is testable per action instead of implied.
///
/// The rule behind it: **moving a cursor or switching the stepper target is a
/// view action**; anything that changes the bytes of the scene or the clip is
/// covered; and an action that *replaces the document* resets the history
/// instead of pushing onto it (a push there would be erased immediately).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HistoryEffect {
    /// Push the current state, then apply.
    Push,
    /// Touch no document data.
    None,
    /// Replace the document and empty both stacks.
    Reset,
}

/// Whole-document snapshot for undo/redo: the scene, the clip and every cursor
/// that could dangle if only the data were restored (a `Delete` moves
/// `selected`, a `StateDel` moves `state_sel`).
#[derive(Clone, Debug)]
struct EditorSnapshot {
    scene: EditorScene,
    clip: Option<editor_clip::EditorClip>,
    selected: usize,
    state_sel: usize,
}

/// Editor state: the scene being arranged plus the free focus the HD-2D camera
/// frames. The world is the game's; only `EditorScene` is the editor's.
struct EditorState {
    scene: EditorScene,
    /// Selected entity index (`usize::MAX` = none).
    selected: usize,
    /// First row of the panel's entity list that is visible.
    scroll: usize,
    /// Last message shown in the panel's top strip.
    status: String,
    /// Point the camera frames; WASD / joystick flies it.
    focus: glam::Vec3,
    /// File this session has open, so `GUARDAR` rewrites it instead of
    /// creating `name_2.json`.
    path: Option<std::path::PathBuf>,
    /// Cursor over `editor::list_scenes()` for `CARGAR`.
    load_cursor: usize,
    /// Collapsible category currently open (`None` = menu closed, so the
    /// editor only shows the entity selection and the menu button).
    panel: Option<EditorPanel>,
    /// Active state inside the selected entity (`0` when it has none).
    state_sel: usize,
    /// Animation clip open in the editor (Fase 6 C: import / export).
    clip: Option<editor_clip::EditorClip>,
    /// Path of the last clip export, for the status line.
    clip_saved_to: Option<std::path::PathBuf>,
    /// Cursor over [`editor_clip::EditorClip::importable`] for `IMPORTAR`.
    clip_cursor: usize,
    /// Document history: `undo[i]` is the state *before* the i-th change.
    undo: Vec<EditorSnapshot>,
    redo: Vec<EditorSnapshot>,
    /// What the transform steppers drive (see [`StepTarget`]).
    target: StepTarget,
    /// Active keyframe of the open clip (`0` when none).
    kf_sel: usize,
    /// Active joint, index into `animation::joint_names()`.
    joint_sel: usize,
}

impl EditorState {
    fn new(focus: glam::Vec3) -> Self {
        // One entity in front of the focus so there is something to grab.
        let mut entity = EditorEntity::new("prop", [focus.x, focus.y, focus.z]);
        entity.size = [2.0, 2.0, 2.0];
        let mut scene = EditorScene::default();
        scene.entities.push(entity);
        Self {
            scene,
            selected: 0,
            scroll: 0,
            status: "WASD vuela · Q/E gira · F2 guarda · Esc vuelve".into(),
            focus,
            path: None,
            load_cursor: 0,
            panel: None,
            state_sel: 0,
            clip: None,
            clip_saved_to: None,
            clip_cursor: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            target: StepTarget::EntityOrState,
            kf_sel: 0,
            joint_sel: 0,
        }
    }

    fn selected_entity(&self) -> Option<&EditorEntity> {
        self.scene.entities.get(self.selected)
    }

    fn selected_entity_mut(&mut self) -> Option<&mut EditorEntity> {
        self.scene.entities.get_mut(self.selected)
    }

    fn clamp_scroll(&mut self) {
        let max = self
            .scene
            .entities
            .len()
            .saturating_sub(hud::EDITOR_VISIBLE_ROWS);
        if self.scroll > max {
            self.scroll = max;
        }
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + hud::EDITOR_VISIBLE_ROWS {
            self.scroll = self.selected + 1 - hud::EDITOR_VISIBLE_ROWS;
        }
    }

    /// Keep the active state inside the selected entity (0 when it has none).
    fn clamp_state_sel(&mut self) {
        self.state_sel = self
            .selected_entity()
            .map(|e| e.clamp_state(self.state_sel))
            .unwrap_or(0);
    }
    /// Number of states of the selection (0 when it has none).
    fn state_len(&self) -> usize {
        self.selected_entity().map_or(0, |e| e.states.len())
    }

    /// Number of keyframes of the open clip (0 when no clip is open).
    fn kf_len(&self) -> usize {
        self.clip.as_ref().map_or(0, |c| c.file.frames.len())
    }

    fn joint_name(&self) -> Option<&'static str> {
        crate::animation::joint_names().get(self.joint_sel).copied()
    }

    /// Keep every cursor inside its collection: `kf_sel` needs an open clip,
    /// `joint_sel` the 12 canonical joints.
    fn clamp_cursors(&mut self) {
        self.state_sel = self
            .selected_entity()
            .map(|e| e.clamp_state(self.state_sel))
            .unwrap_or(0);
        self.kf_sel = self.kf_len().saturating_sub(1).min(self.kf_sel);
        let joints = crate::animation::joint_names().len();
        if self.joint_sel >= joints {
            self.joint_sel = joints - 1;
        }
    }

    /// What an action does to the undo history. One source of truth, so the
    /// policy is testable per action instead of implied.
    ///
    /// The rule behind it: **moving a cursor or switching the stepper target is
    /// a view action**; anything that changes the bytes of the scene or the clip
    /// is covered; and an action that *replaces the document* resets the history
    /// instead of pushing onto it (a push there would be erased immediately).
    fn history_effect(action: EditorAction) -> HistoryEffect {
        use EditorAction::*;
        match action {
            // Cursores y modos: elegir dónde estás no es cambiar el documento.
            ScrollPrev
            | ScrollNext
            | SelPrev
            | SelNext
            | OpenPanel(_)
            | ClosePanel
            | StateSel(_)
            | StatePrev
            | StateNext
            | KeyframeSel(_)
            | KeyframePrev
            | KeyframeNext
            | JointPrev
            | JointNext
            | EditEntity
            | EditJoint
            | Undo
            | Redo
            // Escriben a disco, no al documento en memoria.
            | Save
            | ClipSave
            // `Back` es una transición de pantalla: `apply_hud_action` la
            // intercepta antes de llegar aquí, así que no toca el documento.
            | Back => HistoryEffect::None,
            // Cambian el documento entero: la historia empieza de cero aquí.
            Load | ClipLoad => HistoryEffect::Reset,
            _ => HistoryEffect::Push,
        }
    }

    /// Everything the undo/redo stacks have to carry.
    fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            scene: self.scene.clone(),
            clip: self.clip.clone(),
            selected: self.selected,
            state_sel: self.state_sel,
        }
    }

    fn restore(&mut self, snap: EditorSnapshot) {
        self.scene = snap.scene;
        self.clip = snap.clip;
        self.selected = snap.selected;
        self.state_sel = snap.state_sel;
        self.clamp_scroll();
        self.clamp_state_sel();
    }

    /// Record the current state before a change, and invalidate the redo path.
    fn push_undo(&mut self) {
        if self.undo.len() == ED_UNDO_MAX {
            self.undo.remove(0);
        }
        self.undo.push(self.snapshot());
        self.redo.clear();
    }

    /// `Load` / `IMPORTAR` replace the whole document: history across that line
    /// would lie about what the previous steps were undoing.
    fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Move / rotate / scale / shear the selection, or run a file action.
    fn apply(&mut self, action: EditorAction) {
        // Undo first: it must capture the state *before* anything changes.
        match Self::history_effect(action) {
            HistoryEffect::Push => self.push_undo(),
            HistoryEffect::None | HistoryEffect::Reset => {}
        }
        match action {
            EditorAction::Undo => {
                let Some(snap) = self.undo.pop() else {
                    self.status = "nada que deshacer".into();
                    return;
                };
                let current = self.snapshot();
                self.restore(snap);
                self.redo.push(current);
                self.status = "deshecho".into();
                return;
            }
            EditorAction::Redo => {
                let Some(snap) = self.redo.pop() else {
                    self.status = "nada que rehacer".into();
                    return;
                };
                let current = self.snapshot();
                self.restore(snap);
                self.undo.push(current);
                self.status = "rehecho".into();
                return;
            }
            EditorAction::ScrollPrev => {
                self.scroll = self.scroll.saturating_sub(1);
                return;
            }
            EditorAction::ScrollNext => {
                self.scroll += 1;
                self.clamp_scroll();
                return;
            }
            EditorAction::SelPrev | EditorAction::SelNext => {
                let n = self.scene.entities.len();
                if n == 0 {
                    self.selected = usize::MAX;
                    return;
                }
                let step = if action == EditorAction::SelNext {
                    1i64
                } else {
                    -1
                };
                let cur: i64 = if self.selected >= n {
                    // Nothing (or stale) selected: enter from one end.
                    if step > 0 {
                        0
                    } else {
                        n as i64 - 1
                    }
                } else {
                    self.selected as i64
                };
                self.selected = ((cur + step).rem_euclid(n as i64)) as usize;
                self.clamp_scroll();
                self.clamp_state_sel();
                return;
            }
            EditorAction::KindPrev | EditorAction::KindNext => {
                let step = if action == EditorAction::KindNext { 1 } else { -1 };
                if let Some(e) = self.selected_entity_mut() {
                    let n = editor::KINDS.len() as i64;
                    let cur = editor::KINDS
                        .iter()
                        .position(|k| *k == e.kind)
                        .map(|i| i as i64)
                        .unwrap_or(0);
                    let next = ((cur + step).rem_euclid(n)) as usize;
                    e.kind = editor::KINDS[next].to_string();
                }
                return;
            }
            EditorAction::Add => {
                let kind = self
                    .selected_entity()
                    .map(|e| e.kind.clone())
                    .unwrap_or_else(|| editor::KINDS[0].to_string());
                let mut e = EditorEntity::new(&kind, [0.0, 0.0, 0.0]);
                e.size = [2.0, 2.0, 2.0];
                e.position = [
                    self.focus.x,
                    self.focus.y,
                    self.focus.z,
                ];
                self.scene.entities.push(e);
                self.selected = self.scene.entities.len() - 1;
                self.clamp_scroll();
                self.clamp_state_sel();
                self.status = format!("entidad {} añadida", self.scene.entities.len());
                return;
            }
            EditorAction::Duplicate => {
                if let Some(e) = self.selected_entity().cloned() {
                    let mut copy = e;
                    copy.position[1] += copy.size[1].max(1.0);
                    self.scene.entities.push(copy);
                    self.selected = self.scene.entities.len() - 1;
                    self.clamp_scroll();
                    self.clamp_state_sel();
                    self.status = "entidad copiada".into();
                } else {
                    self.status = "nada que copiar".into();
                }
                return;
            }
            EditorAction::Delete => {
                if self.selected < self.scene.entities.len() {
                    self.scene.entities.remove(self.selected);
                    self.selected = self.selected.min(self.scene.entities.len().saturating_sub(1));
                    self.clamp_scroll();
                    self.clamp_state_sel();
                    self.status = format!("entidades: {}", self.scene.entities.len());
                } else {
                    self.status = "nada que borrar".into();
                }
                return;
            }
            EditorAction::Save => {
                let result = match self.path.clone() {
                    Some(path) => editor::write_scene_to(&path, &self.scene).map(|_| path),
                    None => editor::save_scene(&self.scene, true),
                };
                match result {
                    Ok(path) => {
                        self.path = Some(path.clone());
                        self.status = format!("guardado {}", path.display());
                    }
                    Err(e) => self.status = format!("error: {e}"),
                }
                return;
            }
            EditorAction::Load => {
                let names = editor::list_scenes();
                if names.is_empty() {
                    self.status = "no hay escenas guardadas".into();
                    return;
                }
                let name = names[self.load_cursor % names.len()].clone();
                self.load_cursor = (self.load_cursor + 1) % names.len();
                match editor::load_scene(&name) {
                    Ok(scene) => {
                        let n = scene.entities.len();
                        self.scene = scene;
                        self.path = Some(editor::scene_file(&name));
                        self.selected = 0;
                        self.scroll = 0;
                        self.clamp_state_sel();
                        self.clear_history();
                        self.status = format!("cargada {name} · {n} entidades");
                    }
                    Err(e) => self.status = format!("error: {e}"),
                }
                return;
            }
            // ── States of the selected entity (schema v2) ───────────────────
            EditorAction::StateSel(i) => {
                self.state_sel = i;
                self.clamp_state_sel();
                return;
            }
            EditorAction::StatePrev | EditorAction::StateNext => {
                let n = self.state_len();
                if n > 0 {
                    let step = if action == EditorAction::StateNext {
                        1i64
                    } else {
                        -1
                    };
                    self.state_sel = ((self.state_sel as i64 + step).rem_euclid(n as i64)) as usize;
                }
                return;
            }
            EditorAction::StateAdd => {
                let Some(e) = self.selected_entity_mut() else {
                    self.status = "sin entidad seleccionada".into();
                    return;
                };
                let n = e.states.len();
                e.states.push(editor::EditorEntityState {
                    name: format!("estado {}", n + 1),
                    // Seed with the entity's own mesh so a new state starts
                    // looking like the base one instead of an empty shell.
                    model: e.model.clone(),
                    ..Default::default()
                });
                self.state_sel = n;
                self.status = format!("estados: {}", n + 1);
                return;
            }
            EditorAction::StateDup => {
                let Some(st) = self
                    .selected_entity()
                    .and_then(|e| e.state(self.state_sel))
                    .cloned()
                else {
                    self.status = "sin estados que copiar".into();
                    return;
                };
                if let Some(e) = self.selected_entity_mut() {
                    e.states.push(st);
                    let n = e.states.len();
                    self.state_sel = n - 1;
                    self.status = format!("estados: {n}");
                }
                return;
            }
            EditorAction::StateDel => {
                if self.state_len() == 0 {
                    self.status = "sin estados que borrar".into();
                    return;
                }
                let idx = self.state_sel;
                if self.selected_entity_mut().is_some() {
                    self.scene.entities[self.selected]
                        .states
                        .remove(idx);
                }
                self.clamp_state_sel();
                self.status = format!("estados: {}", self.state_len());
                return;
            }
            // ── Animation clip (Fase 6 C: import / export) ───────────────────
            EditorAction::ClipLoad => {
                // Sin selector de ficheros en el HUD: se cicla como `Load` con
                // las escenas.
                let files = editor_clip::EditorClip::importable();
                if files.is_empty() {
                    self.status = "no hay clips en assets/animations".into();
                    return;
                }
                let path = files[self.clip_cursor % files.len()].clone();
                self.clip_cursor = (self.clip_cursor + 1) % files.len();
                match editor_clip::EditorClip::load(&path) {
                    Ok(clip) => {
                        let summary = clip.summary();
                        self.clip = Some(clip);
                        self.clip_saved_to = None;
                        self.kf_sel = 0;
                        self.joint_sel = 0;
                        self.clear_history();
                        self.status = format!("clip {summary}");
                    }
                    Err(e) => self.status = format!("error: {e}"),
                }
                return;
            }
            EditorAction::ClipSave => {
                let Some(clip) = self.clip.as_ref() else {
                    self.status = "no hay clip que exportar".into();
                    return;
                };
                match clip.save_to(&editor_clip::clips_dir()) {
                    Ok(path) => {
                        self.status = format!("clip exportado {}", path.display());
                        self.clip_saved_to = Some(path);
                    }
                    Err(e) => self.status = format!("error: {e}"),
                }
                return;
            }
            // ── Keyframe / articulación (niveles 3 y 4) ─────────────────────
            EditorAction::KeyframeSel(i) => {
                self.kf_sel = i;
                self.clamp_cursors();
                return;
            }
            EditorAction::KeyframePrev | EditorAction::KeyframeNext => {
                let n = self.kf_len();
                if n > 0 {
                    let step = if action == EditorAction::KeyframeNext {
                        1i64
                    } else {
                        -1
                    };
                    self.kf_sel = ((self.kf_sel as i64 + step).rem_euclid(n as i64)) as usize;
                }
                return;
            }
            EditorAction::KeyframeTimeDec | EditorAction::KeyframeTimeInc => {
                let n = self.kf_len();
                if n == 0 {
                    self.status = "no hay clip abierto".into();
                    return;
                }
                let dir = if action == EditorAction::KeyframeTimeInc {
                    1.0
                } else {
                    -1.0
                };
                let dur = self
                    .clip
                    .as_ref()
                    .map(|c| c.file.duration_s)
                    .unwrap_or(0.0)
                    .max(0.0);
                if let Some(c) = self.clip.as_mut() {
                    let kf = self.kf_sel.min(c.file.frames.len() - 1);
                    let t = &mut c.file.frames[kf].t;
                    *t = (*t + dir * ED_KEYFRAME_TIME_STEP).clamp(0.0, dur);
                }
                return;
            }
            EditorAction::JointPrev | EditorAction::JointNext => {
                let n = crate::animation::joint_names().len() as i64;
                let step = if action == EditorAction::JointNext { 1 } else { -1 };
                self.joint_sel = ((self.joint_sel as i64 + step).rem_euclid(n)) as usize;
                return;
            }
            EditorAction::EditEntity => {
                self.target = StepTarget::EntityOrState;
                return;
            }
            EditorAction::EditJoint => {
                if self.kf_len() == 0 {
                    self.status = "no hay clip abierto".into();
                    return;
                }
                self.target = StepTarget::Joint;
                return;
            }
            EditorAction::OpenPanel(p) => {
                self.panel = Some(p);
                return;
            }
            EditorAction::ClosePanel => {
                self.panel = None;
                return;
            }
            // Back to the menu is an app-level transition (lib.rs).
            EditorAction::Back => return,
            // The transform steps fall through to the block below.
            _ => {}
        }

        // Transform steps: dispatch on the active target (polymorphic target,
        // `docs/plan_fase6.md` §4).
        if self.target == StepTarget::Joint {
            self.apply_joint_step(action);
            return;
        }

        // Transform steps need a selection. They drive the ACTIVE STATE when the
        // entity owns states — a state's transform is local and is applied
        // *before* the entity's, same documented order — and the entity itself
        // otherwise. `size` always stays on the entity: it is the marker box,
        // not part of the mesh.
        let Some(e) = self.selected_entity() else {
            self.status = "sin entidad seleccionada".into();
            return;
        };
        let in_state = e.has_states();
        let state_idx = e.clamp_state(self.state_sel);
        // `state()` is `None` exactly when the entity has no states, so this
        // picks the active state when there is one and the entity otherwise.
        let mut t = e
            .state(state_idx)
            .map(|s| s.transform())
            .unwrap_or_else(|| e.transform());
        let mut size = e.size;
        let bump = |v: &mut f32, d: f32, lo: f32, hi: f32| {
            *v = (*v + d).clamp(lo, hi);
        };
        match action {
            EditorAction::PosXDec => bump(&mut t.position[0], -ED_POS_STEP, -ED_POS_LIMIT, ED_POS_LIMIT),
            EditorAction::PosXInc => bump(&mut t.position[0], ED_POS_STEP, -ED_POS_LIMIT, ED_POS_LIMIT),
            EditorAction::PosYDec => bump(&mut t.position[1], -ED_POS_STEP, 0.0, ED_POS_LIMIT),
            EditorAction::PosYInc => bump(&mut t.position[1], ED_POS_STEP, 0.0, ED_POS_LIMIT),
            EditorAction::PosZDec => bump(&mut t.position[2], -ED_POS_STEP, -ED_POS_LIMIT, ED_POS_LIMIT),
            EditorAction::PosZInc => bump(&mut t.position[2], ED_POS_STEP, -ED_POS_LIMIT, ED_POS_LIMIT),
            EditorAction::RotXDec => t.rotation[0] = wrap_deg(t.rotation[0] - ED_ROT_STEP),
            EditorAction::RotXInc => t.rotation[0] = wrap_deg(t.rotation[0] + ED_ROT_STEP),
            EditorAction::RotYDec => t.rotation[1] = wrap_deg(t.rotation[1] - ED_ROT_STEP),
            EditorAction::RotYInc => t.rotation[1] = wrap_deg(t.rotation[1] + ED_ROT_STEP),
            EditorAction::RotZDec => t.rotation[2] = wrap_deg(t.rotation[2] - ED_ROT_STEP),
            EditorAction::RotZInc => t.rotation[2] = wrap_deg(t.rotation[2] + ED_ROT_STEP),
            EditorAction::SclXDec => bump(&mut t.scale[0], -ED_SCALE_STEP, ED_SCALE_RANGE.0, ED_SCALE_RANGE.1),
            EditorAction::SclXInc => bump(&mut t.scale[0], ED_SCALE_STEP, ED_SCALE_RANGE.0, ED_SCALE_RANGE.1),
            EditorAction::SclYDec => bump(&mut t.scale[1], -ED_SCALE_STEP, ED_SCALE_RANGE.0, ED_SCALE_RANGE.1),
            EditorAction::SclYInc => bump(&mut t.scale[1], ED_SCALE_STEP, ED_SCALE_RANGE.0, ED_SCALE_RANGE.1),
            EditorAction::SclZDec => bump(&mut t.scale[2], -ED_SCALE_STEP, ED_SCALE_RANGE.0, ED_SCALE_RANGE.1),
            EditorAction::SclZInc => bump(&mut t.scale[2], ED_SCALE_STEP, ED_SCALE_RANGE.0, ED_SCALE_RANGE.1),
            EditorAction::SkewDec => bump(&mut t.skew[2], -ED_SKEW_STEP, -ED_SKEW_LIMIT, ED_SKEW_LIMIT),
            EditorAction::SkewInc => bump(&mut t.skew[2], ED_SKEW_STEP, -ED_SKEW_LIMIT, ED_SKEW_LIMIT),
            EditorAction::SizeDec => {
                for a in size.iter_mut() {
                    *a = (*a - ED_SIZE_STEP).max(ED_SIZE_RANGE.0);
                }
            }
            EditorAction::SizeInc => {
                for a in size.iter_mut() {
                    *a = (*a + ED_SIZE_STEP).min(ED_SIZE_RANGE.1);
                }
            }
            EditorAction::ScrollPrev
            | EditorAction::ScrollNext
            | EditorAction::SelPrev
            | EditorAction::SelNext
            | EditorAction::KindPrev
            | EditorAction::KindNext
            | EditorAction::Add
            | EditorAction::Duplicate
            | EditorAction::Delete
            | EditorAction::Save
            | EditorAction::Load
            | EditorAction::StateSel(_)
            | EditorAction::StatePrev
            | EditorAction::StateNext
            | EditorAction::StateAdd
            | EditorAction::StateDup
            | EditorAction::StateDel
            | EditorAction::ClipLoad
            | EditorAction::ClipSave
            | EditorAction::Undo
            | EditorAction::Redo
            | EditorAction::KeyframeSel(_)
            | EditorAction::KeyframePrev
            | EditorAction::KeyframeNext
            | EditorAction::KeyframeTimeDec
            | EditorAction::KeyframeTimeInc
            | EditorAction::JointPrev
            | EditorAction::JointNext
            | EditorAction::EditEntity
            | EditorAction::EditJoint
            | EditorAction::OpenPanel(_)
            | EditorAction::ClosePanel
            | EditorAction::Back => {}
        }
        let Some(e) = self.selected_entity_mut() else {
            return;
        };
        e.size = size;
        if in_state {
            if let Some(s) = e.state_mut(state_idx) {
                s.set_transform(t);
            }
        } else {
            e.set_transform(t);
        }
    }

    /// One `Rot*` press on the active joint of the active keyframe, in
    /// **degrees** (that is what `FrameFile.pose` stores; `HeroPose` radians
    /// only appear at sample time, via `AnimationClip::from_file`).
    ///
    /// A joint rotates around exactly one axis — the last letter of its
    /// canonical name — so a `Rot*` step for another axis has **no target** and
    /// is ignored, as are `Pos*`, `Scl*`, `Size*` and `Skew*` (an angle has
    /// neither position nor size nor shear).
    fn apply_joint_step(&mut self, action: EditorAction) {
        let (axis, dir) = match action {
            EditorAction::RotXDec => ('x', -1.0),
            EditorAction::RotXInc => ('x', 1.0),
            EditorAction::RotYDec => ('y', -1.0),
            EditorAction::RotYInc => ('y', 1.0),
            EditorAction::RotZDec => ('z', -1.0),
            EditorAction::RotZInc => ('z', 1.0),
            // Channels with no meaning for a joint angle.
            _ => return,
        };
        let Some(name) = self.joint_name() else {
            return;
        };
        if crate::animation::joint_axis(name) != Some(axis) {
            return;
        }
        let Some(c) = self.clip.as_mut() else {
            self.status = "no hay clip abierto".into();
            return;
        };
        let kf = self.kf_sel.min(c.file.frames.len() - 1);
        let pose = &mut c.file.frames[kf].pose;
        let current = pose.get(name).copied().unwrap_or(0.0);
        pose.insert(name.to_string(), wrap_deg(current + dir * ED_ROT_STEP));
    }

    /// Pose of the open clip sampled at the active keyframe's `t`, in radians.
    ///
    /// This is the bridge to the runtime: it goes through the game's own
    /// `AnimationClip::from_file` + `sample`, so it proves the edited clip is
    /// valid and consumable. It does **not** draw anything — that needs the rig,
    /// which is why it is test-only for now: it becomes the rig's entry point
    /// (degrees written by the steppers → radians sampled here) when the
    /// preview can be posed.
    #[cfg(test)]
    fn sample_pose(&self) -> Option<crate::hero_pose::HeroPose> {
        let c = self.clip.as_ref()?;
        if c.file.frames.is_empty() {
            return None;
        }
        let clip = crate::animation::AnimationClip::from_file(c.file.clone());
        let t = c.file.frames[self.kf_sel.min(c.file.frames.len() - 1)].t;
        Some(clip.sample(t))
    }

    /// Fly the focus point: WASD relative to the camera, Space/Ctrl vertical,
    /// Shift sprint. The camera orbit itself is driven with Q/E.
    fn tick_fly(&mut self, keys: &HeldKeys, camera: &Camera, dt: f32) {
        let f = camera.forward();
        let flat_f = glam::Vec3::new(f.x, 0.0, f.z);
        let fwd = if flat_f.length_squared() > 1e-6 {
            flat_f.normalize()
        } else {
            glam::Vec3::X
        };
        let right = glam::Vec3::new(-fwd.z, 0.0, fwd.x);
        let speed = if keys.sprint { ED_FLY_SPEED * 2.0 } else { ED_FLY_SPEED };
        let mut delta = glam::Vec3::ZERO;
        if keys.forward {
            delta += fwd;
        }
        if keys.back {
            delta -= fwd;
        }
        if keys.right {
            delta += right;
        }
        if keys.left {
            delta -= right;
        }
        if keys.up {
            delta += glam::Vec3::Y;
        }
        if keys.down {
            delta -= glam::Vec3::Y;
        }
        if delta.length_squared() > 1e-6 {
            self.focus += delta.normalize() * speed * dt;
        }
        self.focus.y = self.focus.y.clamp(ED_FLY_MIN_Y, ED_FLY_MAX_Y);
    }

    /// Cells to draw as entity markers, nearest entity first so the budget
    /// goes to what the player is looking at. The selected entity is drawn
    /// bright white — unless it has a real mesh on screen (`has_preview`), in
    /// which case the mesh is the feedback and the cage would just overlap it.
    fn marker_cells(
        &self,
        cam_pos: glam::Vec3,
        has_preview: bool,
    ) -> Vec<(glam::IVec3, [f32; 3])> {
        let mut visible: Vec<(f32, usize)> = self
            .scene
            .entities
            .iter()
            .enumerate()
            .filter(|(i, _)| !(*i == self.selected && has_preview))
            .map(|(i, e)| {
                let (min, max) = e.aabb();
                let center = (min + max) * 0.5;
                (center.distance(cam_pos), i)
            })
            .filter(|(d, _)| *d <= ED_MARKER_RANGE)
            .collect();
        visible.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let mut out = Vec::new();
        for (_, i) in visible {
            if out.len() >= ED_MAX_MARKER_CELLS {
                break;
            }
            let e = &self.scene.entities[i];
            let selected = i == self.selected;
            let base = e.kind_color();
            let color = if selected {
                [1.0, 1.0, 1.0]
            } else {
                base
            };
            for c in e.cover_cells(ED_CELLS_PER_ENTITY) {
                if out.len() >= ED_MAX_MARKER_CELLS {
                    break;
                }
                out.push((c, color));
            }
        }
        out
    }

    /// Mesh of the selected entity, for the editor's 3D preview. `None` when
    /// there is no selection, the entity has no model path, or the file does
    /// not resolve — the caller then keeps drawing the selection cage.
    ///
    /// MVP scope, deliberately: yaw + height only. `for_each_face` places the
    /// model itself, so the other three transform channels (rot X/Z, skew,
    /// non-uniform scale), the rig/pose and the state's palette override are
    /// NOT applied here.
    fn preview(&self) -> Option<crate::render::EditorPreview> {
        let e = self.selected_entity()?;
        let state = e.clamp_state(self.state_sel);
        let model = crate::entity_model::preview_model(e.effective_model(state))?;
        Some(crate::render::EditorPreview {
            model,
            // Empty palette name (a state that inherits, or a v1 entity with no
            // states at all) → `None` → the mesh keeps its own colours.
            palette: crate::entity_model::preview_palette(e.effective_palette(state)),
            feet: glam::Vec3::from_array(e.position),
            facing: e.rotation[1].to_radians(),
            // `size[1]` is the CAJA de la entidad, así que la preview respeta el
            // mismo botón del panel Transform.
            body_height: e.size[1].max(0.1),
        })
    }
}

/// Degrees wrapped to `-180..180` so the panel never shows `1080°`.
fn wrap_deg(mut d: f32) -> f32 {
    d = d % 360.0;
    if d > 180.0 {
        d -= 360.0;
    }
    if d < -180.0 {
        d += 360.0;
    }
    d
}

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    world: World,
    player: Player,
    camera: Camera,
    keys: HeldKeys,
    cursor_captured: bool,
    last_frame: Instant,
    tick_accum: f32,
    /// Throttle window title updates (avoids format work every frame).
    title_frames: u32,
    /// Smoothed frames-per-second for the title HUD.
    fps_ema: f32,
    /// Smoothed frame time in milliseconds.
    frame_ms_ema: f32,
    stair: StairTool,
    /// Equipped wooden pick (durability from `assets/items/wooden_pickaxe.json`).
    pickaxe: ToolInstance,
    /// Bottom-left weapon / tool hotbar (4 slots).
    hotbar: Hotbar,
    /// Last built stair HUD (for hit-testing clicks).
    stair_hud: HudMesh,
    /// Cursor position in logical pixels (top-left origin).
    mouse_logical: (f32, f32),
    /// Hold-to-break (LMB) while pickaxe is equipped in HD-2D.
    attack_held: bool,
    /// Ese hold lo inició el táctil (botón MINE o sostener en el mundo):
    /// solo el táctil lo suelta (el LMB de PC no se toca).
    touch_hold: bool,
    /// Rayo apuntado del picado táctil (tap sostenido): el hold-to-mine
    /// continúa sobre ESE bloque. `None` = rake fijo del diorama (PC).
    touch_aim: Option<(glam::Vec3, glam::Vec3)>,
    /// Right button held (Shift+derecho con espada = guardia de bloqueo).
    rmb_held: bool,
    mining: MiningProgress,
    /// Minecraft-style 9×3 storage (+ UI cursor stack).
    inventory: PlayerInventory,
    /// Inventory panel open (INV button / G).
    inventory_open: bool,
    /// Location sign text (realm + nearest settlement), recomputed on block change.
    sign_line1: String,
    sign_line2: String,
    /// Cached meters to nearest settlement (drives sign opacity fade).
    sign_dist_m: Option<i32>,
    /// Block the sign was last computed for (`None` = never).
    sign_block: Option<(i32, i32)>,
    /// FP toggle pressed by hand (V). Auto-FP (casa/cueva) overrides while active.
    fp_manual: bool,
    /// Last auto indoor/cave state (hysteresis for cursor + entering FP).
    auto_fp: bool,
    /// Latched auto-FP decision. Changes only after the debounce timer fires, so
    /// boundary zones that flicker across a threshold cannot reverse the blend
    /// mid-transition (which made the FP→diorama blend chain/re-start ~4x).
    auto_fp_latch: bool,
    /// Seconds the current raw auto-FP state has been sustained (asymmetric
    /// debounce: enter FP fast, exit slow).
    mode_debounce: f32,
    /// True si el auto-FP actual se activó por interior (puerta) en vez de
    /// por cueva. Al salir por puerta se reorienta el diorama al rumbo.
    auto_fp_from_indoors: bool,
    /// Hitch pressure 0..=1: slow frames push the HD-2D lens toward the hero
    /// (hides pop + cuts fill while the GPU is behind). Fast attack on spikes,
    /// slow release so the lens doesn't breathe — see `HITCH_*` consts.
    hitch_pressure: f32,
    last_autosave: Instant,
    /// Controles táctiles (Android). Separados del menú de PC: no escriben
    /// en `stair_hud.hits` ni alteran el HUD de PC cuando están apagados.
    touch: TouchControls,
    /// Suelo del spawn confirmado (Android fast-boot): sin preload, la física
    /// no arranca hasta que la columna del jugador existe (si no, caía al
    /// vacío y la cámara quedaba bajo el terreno).
    boot_grounded: bool,
    /// Pantalla de carga: instante de arranque (timeout 15 s).
    boot_time: Instant,
    /// Peor nº de huecos visto durante la carga (base del % de progreso).
    load_total: usize,
    /// Carga inicial terminada (anillo keep sin huecos o timeout).
    load_done: bool,
    /// Cuándo terminó (fundido de salida de 0.6 s).
    load_done_at: Option<Instant>,
    /// Picos de remallado desde el último log `perf` (el muestreo cada 300
    /// frames perdería el tirón: se guarda el peor, no el último).
    peak_rebuilt: usize,
    peak_mesh_ms: f32,
    peak_up_ms: f32,
    /// La actividad volvió de suspensión (Android): hay que reconfigurar
    /// la superficie nativa, que el SO destruyó al pausar.
    was_suspended: bool,
    /// Which top-level screen is up.
    screen: Screen,
    /// Keyboard-highlighted main-menu row (`0..MENU_ROWS.len()`).
    menu_index: usize,
    /// The world has been preloaded + warm-started (guard for `enter_playing`).
    world_prepared: bool,
    /// Hay partida guardada en disco (menu la atenúa si no). Se calcula una
    /// vez: `save::exists` hace syscalls y el menú seibuilda cada frame.
    has_save: bool,
    /// Escena + foco del editor nativo (pantalla `Screen::Editor`).
    editor: EditorState,
}

/// Fila del menú tras mover el resaltado `delta` pasos: envuelve arriba →
/// abajo. Función suelta (y no método de `App`) para poder testearla sin
/// construir una `App`, que exige ventana y renderer.
fn menu_row_step(index: usize, delta: isize, rows: usize) -> usize {
    if rows == 0 {
        return 0;
    }
    (index as isize + delta).rem_euclid(rows as isize) as usize
}

/// Gris de grieta por progreso de picado `t` en 0..=1, en 10 etapas como
/// Minecraft: gris claro → casi negro, sin tintes de color.
fn mining_crack_gray(t: f32) -> [f32; 3] {
    const STAGES: u32 = 10;
    let stage = (t.clamp(0.0, 1.0) * STAGES as f32).floor().min(STAGES as f32 - 1.0);
    let v = 0.42 - 0.37 * (stage / (STAGES as f32 - 1.0));
    [v, v, v]
}

impl App {
    fn new() -> Self {
        let player = Player::spawn_on_terrain();
        let world = if DEBUG_FACE_VISIBILITY {
            World::with_face_debug()
        } else {
            World::new()
        };
        let pickaxe = spawn_wooden_pickaxe();
        let mut inventory = PlayerInventory::default();
        // Equipo inicial: botas pesadas (caminar por el lecho).
        inventory.try_add(InvItem::DepthBoots, 1);
        let camera = if DEBUG_FACE_VISIBILITY {
            Camera::looking_at_face_debug()
        } else if ENABLE_HD2D {
            Camera::hd2d_follow(player.focus_position())
        } else {
            Camera::first_person(player.eye_position())
        };
        Self {
            window: None,
            renderer: None,
            world,
            player,
            camera,
            keys: HeldKeys::default(),
            cursor_captured: false,
            last_frame: Instant::now(),
            tick_accum: 0.0,
            title_frames: 0,
            fps_ema: 60.0,
            frame_ms_ema: 16.7,
            stair: StairTool::default(),
            pickaxe,
            hotbar: Hotbar::default(),
            stair_hud: HudMesh::default(),
            mouse_logical: (0.0, 0.0),
            attack_held: false,
            touch_hold: false,
            touch_aim: None,
            rmb_held: false,
            mining: MiningProgress::default(),
            inventory,
            inventory_open: false,
            sign_line1: String::new(),
            sign_line2: String::new(),
            sign_dist_m: None,
            sign_block: None,
            fp_manual: false,
            auto_fp: false,
            auto_fp_latch: false,
            mode_debounce: 0.0,
            auto_fp_from_indoors: false,
            hitch_pressure: 0.0,
            last_autosave: Instant::now(),
            touch: TouchControls::new(),
            boot_grounded: false,
            boot_time: Instant::now(),
            load_total: 0,
            load_done: false,
            load_done_at: None,
            peak_rebuilt: 0,
            peak_mesh_ms: 0.0,
            peak_up_ms: 0.0,
            was_suspended: false,
            screen: Screen::Menu,
            menu_index: 0,
            world_prepared: false,
            has_save: save::exists(),
            editor: EditorState::new(glam::Vec3::new(8.0, 24.0, 24.0)),
        }
    }

    /// Fila "PARTIDA NUEVA": estado de jugador limpio y `player_edits`
    /// vacíos (la partida guardada no se ha tocado). El terreno es
    /// procedural de la semilla, así que "nueva" vs "guardada" solo
    /// difieren en pose, inventario, pico y edits.
    fn start_new_game(&mut self) {
        if !DEBUG_FACE_VISIBILITY {
            self.player = Player::spawn_on_terrain();
            self.pickaxe = spawn_wooden_pickaxe();
            self.hotbar = Hotbar::default();
            let mut inventory = PlayerInventory::default();
            inventory.try_add(InvItem::DepthBoots, 1);
            self.inventory = inventory;
            self.world.load_player_edits(Vec::new());
        }
        self.stair.cancel_to_idle();
        self.enter_playing();
        log::info!("menu: partida nueva");
    }

    /// Fila "PARTIDA GUARDADA". Los `player_edits` se cargan antes del
    /// preload (ver `enter_playing`) para que los shunks nazcan ya con los
    /// huecos del jugador: no hay que reescribir voxels de chunks vivos.
    fn start_saved_game(&mut self) {
        match save::try_load() {
            Some(loaded) => {
                if !DEBUG_FACE_VISIBILITY {
                    self.player.restore_from_save(
                        loaded.feet,
                        loaded.facing,
                        loaded.hearts,
                        loaded.bottles,
                    );
                    self.pickaxe = spawn_wooden_pickaxe();
                    self.pickaxe.durability = loaded
                        .pickaxe_durability
                        .min(self.pickaxe.def.max_durability);
                    self.hotbar = loaded.hotbar;
                    self.inventory = loaded.inventory;
                    if self.inventory.count_of(InvItem::DepthBoots) == 0 {
                        self.inventory.try_add(InvItem::DepthBoots, 1);
                    }
                    self.world.load_player_edits(loaded.edits);
                }
                self.enter_playing();
                log::info!("menu: partida guardada cargada");
            }
            None => {
                log::warn!("menu: no hay partida guardada — se empieza nueva");
                self.start_new_game();
            }
        }
    }

    /// Menú → juego. El preload + warmup de meshes pasan a aquí (no al
    /// arrancar) para que el menú abra instantáneo y para que los
    /// `player_edits` de la partida elegida se apliquen al generar.
    /// Prepara el mundo una sola vez (preload + warmup de meshes), tanto para
    /// jugar como para editar: el editor necesita terreno alrededor del foco.
    fn prepare_world(&mut self) {
        if self.world_prepared {
            return;
        }
        // Android conserva su arranque rápido (sin preload: el streaming por
        // frame rellena el anillo con la pantalla de carga). En PC el preload
        // evita huecos en el primer frame.
        let fast_boot = cfg!(target_os = "android");
        if !DEBUG_FACE_VISIBILITY && !fast_boot {
            self.world
                .preload_shunks_around(self.player.focus_position());
            let _ = self.world.take_dirty_edits();
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.warm_start_meshes(&mut self.world, &self.camera);
            }
        }
        self.world_prepared = true;
        self.load_done = false;
        self.load_done_at = None;
        self.load_total = 0;
        self.boot_time = Instant::now();
        self.boot_grounded = false;
        self.last_frame = Instant::now();
    }

    fn enter_playing(&mut self) {
        self.prepare_world();
        self.screen = Screen::Playing;
    }

    /// Entra al editor nativo. El foco arranca sobre el terreno del spawn y la
    /// entidad inicial queda justo delante, para tener algo que editar.
    fn enter_editor(&mut self) {
        self.prepare_world();
        // Mismo punto que encuadra la cámara en juego, para que el editor
        // arranque con el encuadre conocido (sin héroe, eso sí).
        self.editor = EditorState::new(self.player.display_focus(0.0));
        self.inventory_open = false;
        self.screen = Screen::Editor;
    }

    /// Mueve el resaltado del menú, envuelto. `delta` en filas.
    fn move_menu_row(&mut self, delta: isize) {
        self.menu_index = menu_row_step(self.menu_index, delta, MENU_ROWS.len());
    }

    /// Activa la fila resaltada (teclado). El toque / ratón llegan por
    /// `HudAction` desde las hit regions del HUD.
    fn activate_menu_row(&mut self) {
        if let Some((_, action)) = MENU_ROWS.get(self.menu_index) {
            let action = *action;
            self.apply_hud_action(action);
        }
    }

    /// HUD completo de partida (estado, brújula, cartel, hotbar, inventario,
    /// herramienta, táctil, carga, crosshair). Vive en un método para que el
    /// bucle elija por pantalla sin reindentar un bloque enorme.
    fn build_play_hud(&mut self, lw: f32, lh: f32) -> HudMesh {
        let mut hud = build_status_hud(
            self.player.hearts,
            MAX_HEARTS,
            self.player.bottles,
            MAX_BOTTLES,
            lw,
            lh,
        );
        let compass = build_compass_hud(self.player.facing, lw, lh);
        append_hud(&mut hud, &compass);
        self.refresh_location_sign();
        let sign_a = crate::hud::settlement_sign_opacity(self.sign_dist_m);
        let sign = crate::hud::build_realm_sign_hud(
            &self.sign_line1,
            &self.sign_line2,
            lw,
            lh,
            sign_a,
        );
        append_hud(&mut hud, &sign);
        let hotbar = build_hotbar_hud(&self.hotbar, self.inventory_open, lw, lh);
        append_hud(&mut hud, &hotbar);
        let inv = build_inventory_hud(
            self.inventory_open,
            &self.inventory,
            &self.hotbar,
            self.mouse_logical,
            lw,
            lh,
        );
        append_hud(&mut hud, &inv);
        let stair = build_stair_hud(&self.stair, self.move_yaw(), lw, lh);
        append_hud(&mut hud, &stair);
        // Táctil: overlay separado, sin hit regions (el menú de PC
        // no lo ve; el táctil no ve el menú de PC).
        if self.touch.enabled {
            let touch_hud = self.touch.build_touch_hud(lw, lh);
            append_hud(&mut hud, &touch_hud);
        }
        // Pantalla de carga + fundido de salida (0.6 s): el mundo se
        // revela con el anillo ya cerrado en vez de "de repente".
        {
            let fade = self
                .load_done_at
                .map(|t| 1.0 - t.elapsed().as_secs_f32() / 0.6)
                .unwrap_or(1.0);
            if !self.load_done || fade > 0.0 {
                let open = self
                    .renderer
                    .as_ref()
                    .map(|r| r.ring_holes_open())
                    .unwrap_or(self.load_total);
                let pct = if self.load_total > 0 {
                    1.0 - open.min(self.load_total) as f32 / self.load_total as f32
                } else {
                    0.0
                };
                append_hud(
                    &mut hud,
                    &build_loading_hud(pct, lw, lh, fade.clamp(0.0, 1.0)),
                );
            }
        }
        if self.camera.is_first_person() {
            let cross = crate::hud::build_crosshair_hud(lw, lh);
            append_hud(&mut hud, &cross);
        }
        hud
    }

    /// Reacción a un error de superficie: común a las dos rutas de render
    /// (juego y `render_ui_only` del menú).
    fn surface_failed(&mut self, e: wgpu::SurfaceError, event_loop: &ActiveEventLoop) {
        match e {
            wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated => {
                if let Some(renderer) = self.renderer.as_mut() {
                    let size = renderer.size();
                    renderer.resize(size);
                }
            }
            wgpu::SurfaceError::OutOfMemory => {
                log::error!("out of memory");
                event_loop.exit();
            }
            other => log::warn!("surface error: {other:?}"),
        }
    }

    fn close_inventory(&mut self) {
        if !self.inventory_open {
            return;
        }
        self.inventory.return_cursor();
        self.inventory_open = false;
        self.sync_cursor_for_ui();
    }

    fn persist(&mut self, why: &str) {
        // Con el menú o el editor en pantalla el mundo todavía no se ha
        // elegido: guardar aquí pisaría la partida guardada del usuario con
        // un mundo recién spawneado (el autosave de 45 s lo haría solo).
        if self.screen != Screen::Playing {
            return;
        }
        match save::write_snapshot(
            &self.world,
            self.player.feet,
            self.player.facing,
            self.player.hearts,
            self.player.bottles,
            self.pickaxe.durability,
            &self.hotbar,
            &self.inventory,
        ) {
            Ok(path) => {
                self.last_autosave = Instant::now();
                log::info!("save ({why}): {}", path.display());
            }
            Err(e) => log::warn!("save ({why}) failed: {e}"),
        }
    }

    fn toggle_inventory(&mut self) {
        if self.inventory_open {
            self.close_inventory();
        } else {
            self.inventory_open = true;
            self.sync_cursor_for_ui();
        }
    }

    fn collect_ore(&mut self, drop: OreDrop) {
        let before_cube = self.inventory.count_of(InvItem::cube_for_embed(drop.kind));
        let added = self.inventory.add_ore_drop(drop);
        let micros = self.inventory.count_of(InvItem::micro_for_embed(drop.kind));
        let cubes = self.inventory.count_of(InvItem::cube_for_embed(drop.kind));
        let name = drop.kind.label();
        if added == 0 {
            log::info!("{name}: inventario lleno");
        } else if cubes > before_cube {
            log::info!("{name}: +{added} micro → cubo (total {cubes} cubos, {micros} micros)");
        } else {
            log::info!("{name}: +{added} micro ({micros} en inventario)");
        }
    }

    fn collect_fragments(&mut self, material: Material) {
        let Some((kind, amount)) = fragment_drop_for(material) else {
            return;
        };
        let added = self.inventory.add_fragments(kind, amount);
        let total = self.inventory.count_of(InvItem::from_fragment(kind));
        if added == 0 {
            log::info!("{}: inventario lleno ({total}/100)", kind.label());
        } else if added < amount {
            log::info!("{}: +{added} (lleno: {total}/100)", kind.label());
        } else {
            log::info!("{}: +{added} ({total}/100)", kind.label());
        }
    }

    fn collect_break_loot(&mut self, material: Material, ore: Option<OreDrop>) {
        self.collect_fragments(material);
        if let Some(drop) = ore {
            self.collect_ore(drop);
        }
    }

    /// Excavate queued stair cells via [`break_solid_cell`] (wear → remove → grass → dirty).
    fn tick_stair_dig(&mut self, now: Instant) {
        while let Some(pos) = self.stair.peek_dig_at(now) {
            let material = self.world.dig_material_at(pos);
            let interval = material
                .map(|m| self.pickaxe.def.dig_interval(m))
                .unwrap_or(STAIR_DIG_INTERVAL);

            match break_solid_cell(&mut self.world, pos, &mut self.pickaxe) {
                BreakOutcome::ToolBlocked => {
                    self.stair.abort_dig();
                    log::info!("pico roto — excavación detenida");
                    break;
                }
                BreakOutcome::Skipped => {
                    let _ = self.stair.commit_dig(now, interval);
                }
                BreakOutcome::Broke { material: mat, ore } => {
                    self.collect_break_loot(mat, ore);
                    let _ = self.stair.commit_dig(now, interval);
                    if matches!(
                        mat,
                        Material::Stone
                            | Material::BlackStone
                            | Material::Coal
                            | Material::Sapphire
                            | Material::Ruby
                            | Material::Emerald
                    ) {
                        break;
                    }
                }
            }
        }
    }

    /// Classic hold-to-break (HD-2D; skips stair mode).
    /// Pickaxe uses tool timings; bare hands / non-dig tools take 10× longer.
    fn tick_hold_break(&mut self, dt: f32) {
        if !ENABLE_HD2D || DEBUG_FACE_VISIBILITY {
            return;
        }
        if self.stair.is_active() || !self.attack_held {
            self.mining.clear();
            return;
        }

        let tool_digs = self
            .hotbar
            .tool_id()
            .is_some_and(|id| id.can_dig());
        let using_pick = tool_digs && !self.pickaxe.is_broken();
        // Pico roto (desafilado): pica lo mismo al doble de tiempo, sin desgaste.
        let dulled = tool_digs && self.pickaxe.is_broken();

        // First person aims with the view (mouse pitch); the diorama keeps
        // the fixed -0.35 hold-break rake. Must match the white FP highlight
        // or hold-LMB breaks a different block than the cursor shows.
        // El picado táctil apuntado manda sobre ambos: sigue el rayo del
        // píxel sostenido (en PC `touch_aim` siempre es None: idéntico).
        let (origin, dir) = if let Some((o, d)) = self.touch_aim {
            (o, d)
        } else if self.camera.is_first_person() {
            (self.camera.position, self.camera.forward())
        } else {
            let origin = self.player.eye_position();
            let yaw = self.player.facing;
            (
                origin,
                glam::Vec3::new(yaw.cos(), -0.35, yaw.sin()).normalize_or_zero(),
            )
        };
        let Some(hit) = raycast_reach(&self.world, origin, dir) else {
            self.mining.clear();
            return;
        };
        let Some(material) = self.world.dig_material_at(hit.block) else {
            self.mining.clear();
            return;
        };

        let interval = if using_pick {
            if !self.pickaxe.def.can_mine(material) {
                self.mining.clear();
                return;
            }
            self.pickaxe.def.dig_interval(material)
        } else if dulled {
            if !self.pickaxe.def.can_mine(material) {
                self.mining.clear();
                return;
            }
            self.pickaxe.def.dig_interval(material) * 2
        } else {
            if !bare_hand_can_mine(material) {
                self.mining.clear();
                return;
            }
            bare_hand_dig_interval(material)
        };

        if self.mining.tick(hit.block, dt, interval) {
            let outcome = if using_pick {
                break_solid_cell(&mut self.world, hit.block, &mut self.pickaxe)
            } else if dulled {
                break_solid_cell_dulled(&mut self.world, hit.block, &self.pickaxe)
            } else {
                break_solid_cell_bare(&mut self.world, hit.block)
            };
            match outcome {
                BreakOutcome::ToolBlocked => {
                    if using_pick {
                        log::info!("pico roto — hold-break detenido");
                    }
                    self.mining.clear();
                }
                BreakOutcome::Broke { material, ore } => {
                    self.collect_break_loot(material, ore);
                }
                BreakOutcome::Skipped => {}
            }
        }
    }

    fn digging_for_pose(&self) -> bool {        self.stair.is_digging() || (self.attack_held && self.mining.target.is_some())
    }

    /// Picado manual con pico o variante (herramienta que puede cavar):
    /// mientras se mantiene LMB con la herramienta en mano se fuerza la
    /// 1ª persona para poder moverse y picar con la vista.
    fn mining_with_tool(&self) -> bool {
        self.attack_held
            && !self.stair.is_active()
            && self.hotbar.tool_id().is_some_and(|id| id.can_dig())
    }

    /// Guardia con espada: Shift + clic derecho con espada en mano.
    fn wants_block(&self) -> bool {
        self.keys.sprint && self.hotbar.holding_sword()
    }

    fn set_cursor_captured(&mut self, captured: bool) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        if captured {
            let _ = window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
            window.set_cursor_visible(false);
        } else {
            let _ = window.set_cursor_grab(CursorGrabMode::None);
            window.set_cursor_visible(true);
        }
        self.cursor_captured = captured;
    }

    /// Keep the cursor free for HUD clicks (hotbar, INV, stair panel).
    /// HD-2D never locks the mouse; FPS only unlocks while a menu is open.
    fn sync_cursor_for_ui(&mut self) {
        let need_free = ENABLE_HD2D || self.stair.is_active() || self.inventory_open;
        if need_free {
            if self.cursor_captured {
                self.set_cursor_captured(false);
            }
        }
    }

    fn sync_cursor_for_stair(&mut self) {
        self.sync_cursor_for_ui();
    }

    /// Golpe + hold-to-mine del diorama (ramal HD-2D del clic izquierdo).
    /// Lo comparten el ratón de PC y el botón MINE táctil: mismo código,
    /// sin duplicar. (Extraído tal cual del brazo MouseInput.)
    fn press_mine_hd2d(&mut self) {
        let origin = self.player.eye_position();
        let yaw = self.player.facing;
        let dir = glam::Vec3::new(yaw.cos(), -0.35, yaw.sin()).normalize_or_zero();
        let hit = raycast_reach(&self.world, origin, dir);
        let timid = hit.is_none();
        let item = self.hotbar.selected_item();
        // Shift+clic con espada = estocada baja (no avanza combo).
        let dmg = if self.hotbar.holding_sword() && self.keys.sprint {
            self.player.begin_sword_thrust(timid)
        } else {
            self.player.begin_melee(item, timid)
        };
        if !timid {
            log::debug!("golpe conectado · daño {dmg} (tabla provisional)");
        }
        // Hold-to-mine: pickaxe (sano o desafilado 2×) at tool
        // speed, otherwise bare hands (10×).
        if hit.as_ref().is_some_and(|h| self.can_hold_mine(h.block)) {
            self.attack_held = true;
        }
    }

    /// ¿El bloque admite hold-to-mine con lo equipado? Misma regla del clic
    /// de PC; la comparten ratón y táctil (no duplicar).
    fn can_hold_mine(&self, block: glam::IVec3) -> bool {
        self.world.dig_material_at(block).is_some_and(|m| {
            if self.hotbar.tool_id().is_some_and(|id| id.can_dig()) {
                // Pico roto también pica (desafilado):
                // lo que el pico puede minar, al doble.
                self.pickaxe.def.can_mine(m)
            } else {
                bare_hand_can_mine(m)
            }
        })
    }

    /// Picado apuntado táctil: como el LMB del diorama pero con el rayo del
    /// píxel tocado (`Camera::screen_ray`). Con `start_hold` guarda el aim
    /// para que el hold continúe sobre ESE bloque mientras el dedo aguante;
    /// sin hold es solo el swing (toque rápido = golpe, como un clic de PC).
    fn press_mine_aimed(&mut self, origin: glam::Vec3, dir: glam::Vec3, start_hold: bool) {
        let hit = raycast_reach(&self.world, origin, dir);
        let timid = hit.is_none();
        let item = self.hotbar.selected_item();
        let dmg = if self.hotbar.holding_sword() && self.keys.sprint {
            self.player.begin_sword_thrust(timid)
        } else {
            self.player.begin_melee(item, timid)
        };
        if !timid {
            log::debug!("golpe táctil · daño {dmg}");
        }
        if start_hold && hit.as_ref().is_some_and(|h| self.can_hold_mine(h.block)) {
            self.touch_aim = Some((origin, dir));
            self.attack_held = true;
            self.touch_hold = true;
        }
    }

    /// Detiene el hold iniciado por el táctil (dedo levantado). No toca un
    /// posible hold del LMB de PC (`touch_hold` distingue al dueño).
    fn stop_touch_mine(&mut self) {
        self.touch_aim = None;
        if self.touch_hold {
            self.touch_hold = false;
            self.attack_held = false;
            self.mining.clear();
        }
    }

    /// Toque rápido en pantalla (x,y lógicos): primero el menú de PC con el
    /// mismo trato que un clic (hotbar, INV, panel stair); si no hay nada,
    /// golpe apuntado al mundo por el píxel tocado.
    fn tap_at(&mut self, x: f32, y: f32, lw: f32, lh: f32) {
        if let Some(action) = hit_test(&self.stair_hud.hits, x, y) {
            self.apply_hud_click(action, false);
            return;
        }
        // Mismas guardas que el ratón: con stair, inventario o en el editor,
        // los toques solo hablan con el HUD (en el editor, el panel).
        if self.stair.is_active() || self.inventory_open || self.screen != Screen::Playing {
            return;
        }
        let (origin, dir) = self.camera.screen_ray(x, y, lw, lh);
        self.press_mine_aimed(origin, dir, false);
    }

    fn break_targeted_block(&mut self) {
        let Some(hit) = raycast_reach(&self.world, self.camera.position, self.camera.forward())
        else {
            return;
        };
        if let BreakOutcome::Broke { material, ore } =
            break_solid_cell(&mut self.world, hit.block, &mut self.pickaxe)
        {
            self.collect_break_loot(material, ore);
        }
    }

    fn place_against_targeted_block(&mut self) {
        let Some(hit) = raycast_reach(&self.world, self.camera.position, self.camera.forward())
        else {
            return;
        };
        let place = hit.prev;
        if place == hit.block {
            return;
        }
        let occupied = self.player.occupied_blocks();
        if occupied.contains(&place) {
            return;
        }
        if self.world.get_voxel(place).is_some() {
            return;
        }
        self.world.set_voxel_player(place, Voxel::dirt());
    }

    /// Block under the player’s feet — stair ghost root (no mouse aim).
    fn stair_anchor_block(&self) -> glam::IVec3 {
        let f = self.player.feet;
        glam::IVec3::new(
            f.x.floor() as i32,
            (f.y - 0.05).floor() as i32,
            f.z.floor() as i32,
        )
    }

    /// Look-at for Q/E orbit: stair midpoint when a yellow trail is up.
    fn stair_orbit_focus(&self) -> glam::Vec3 {
        let player = self.player.focus_position();
        if let Some(far) = self.stair.ghost_far_point() {
            (player + far) * 0.5
        } else {
            player
        }
    }

    /// Advance the deferred Q/E orbit each frame: runs the smooth rotation and
    /// starts any waiting snap on the very next frame — Q/E tiene que girar
    /// ya, sin esperar al preload (el streaming rellena detrás; puede haber
    /// huecos 16×16 transitorios durante los 0.45 s del giro). Leaving the
    /// diorama lens (auto first-person in caves/houses) cancels the wait.
    fn update_orbit_gate(&mut self, dt: f32) {
        self.camera.update_orbit_anim(dt);
        let Some(_) = self.camera.pending_orbit() else {
            return;
        };
        if self.camera.hd2d_amount() < crate::camera::ORBIT_WAIT_HD2D_MIN {
            self.camera.cancel_orbits();
            return;
        }
        self.camera.begin_orbit(self.stair_orbit_focus());
    }

    fn move_yaw(&self) -> f32 {
        if ENABLE_HD2D {
            // First person: WASD relative to where the hero looks.
            let facing = self.player.facing;
            self.camera
                .effective_move_yaw(facing, self.camera.hd2d_amount())
        } else {
            self.camera.yaw
        }
    }

    /// Turn the character (and dig heading) to a world cardinal from a pad slot.
    fn apply_pad_facing(&mut self, facing: TunnelFacing) {
        self.player.face_yaw(facing.yaw());
        self.stair.set_facing(facing);
    }

    fn apply_hud_action(&mut self, action: HudAction) {
        match action {
            HudAction::Facing(f) => self.apply_pad_facing(f),
            HudAction::Incline(i) => self.stair.set_incline(i),
            HudAction::LenMinus => self.stair.nudge_steps(-1),
            HudAction::LenPlus => self.stair.nudge_steps(1),
            HudAction::WidthToggle => self.stair.cycle_width(),
            HudAction::Dig => {
                if self.pickaxe.is_broken() {
                    log::info!("pico roto — no se puede cavar");
                    return;
                }
                // Keep pad-selected facing (N/S/E/W) — do not overwrite from look yaw.
                self.stair.sync_ghost_anchor(self.stair_anchor_block());
                let _ = self.stair.confirm_dig();
            }
            HudAction::Cancel => {
                self.stair.cancel_to_idle();
                self.sync_cursor_for_stair();
            }
            HudAction::SelectSlot(i) => {
                self.hotbar.press_slot(i as usize, false);
                self.player.notify_equip();
                if !self.hotbar.holding_pickaxe() && self.stair.is_active() {
                    self.stair.cancel_to_idle();
                    self.sync_cursor_for_stair();
                }
            }
            HudAction::ToggleInventory => {
                self.toggle_inventory();
            }
            HudAction::InvSlot(i) => {
                self.inventory.click_slot(i as usize, false);
            }
            HudAction::InventoryBackdrop => {
                // Click outside the panel closes (Minecraft-like).
                self.close_inventory();
            }
            HudAction::InventoryPanel => {}
            HudAction::MenuNewGame => self.start_new_game(),
            HudAction::MenuLoadGame => self.start_saved_game(),
            HudAction::MenuEditor => self.enter_editor(),
            HudAction::MenuBack => self.screen = Screen::Menu,
            HudAction::MenuBackdrop => {}
            HudAction::Ed(action) => {
                if action == EditorAction::Back {
                    self.screen = Screen::Menu;
                } else {
                    self.editor.apply(action);
                }
            }
        }
    }

    fn apply_hud_click(&mut self, action: HudAction, right: bool) {
        match action {
            HudAction::InvSlot(i) => {
                self.inventory.click_slot(i as usize, right);
            }
            HudAction::InventoryBackdrop => {
                if !right {
                    self.close_inventory();
                }
            }
            HudAction::InventoryPanel => {}
            _ if !right => self.apply_hud_action(action),
            _ => {}
        }
    }

    /// Arrow keys = camera-relative pad; R/F/C = up / down / flat; [/] = length; B = width.
    /// (`G` is inventory — see keyboard handler.)
    fn handle_stair_dir_key(&mut self, key: KeyCode) -> bool {
        let view = self.move_yaw();
        match key {
            KeyCode::ArrowUp => {
                self.apply_pad_facing(PadDir::Up.to_facing(view));
                true
            }
            KeyCode::ArrowDown => {
                self.apply_pad_facing(PadDir::Down.to_facing(view));
                true
            }
            KeyCode::ArrowRight => {
                self.apply_pad_facing(PadDir::Right.to_facing(view));
                true
            }
            KeyCode::ArrowLeft => {
                self.apply_pad_facing(PadDir::Left.to_facing(view));
                true
            }
            KeyCode::KeyR | KeyCode::PageUp => {
                self.stair.set_incline(TunnelIncline::Up);
                true
            }
            KeyCode::KeyF | KeyCode::PageDown => {
                self.stair.set_incline(TunnelIncline::Down);
                true
            }
            KeyCode::KeyC => {
                self.stair.set_incline(TunnelIncline::Flat);
                true
            }
            KeyCode::BracketLeft | KeyCode::Minus => {
                self.stair.nudge_steps(-1);
                true
            }
            KeyCode::BracketRight | KeyCode::Equal => {
                self.stair.nudge_steps(1);
                true
            }
            KeyCode::KeyB => {
                self.stair.cycle_width();
                true
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                if self.pickaxe.is_broken() {
                    log::info!("pico roto — no se puede cavar");
                    return true;
                }
                self.stair.sync_ghost_anchor(self.stair_anchor_block());
                let _ = self.stair.confirm_dig();
                true
            }
            _ => false,
        }
    }

    /// Recompute the location sign when the player steps onto a new block.
    fn refresh_location_sign(&mut self) {
        let feet = self.player.feet;
        let bx = feet.x.floor() as i32;
        let bz = feet.z.floor() as i32;
        let block = (bx, bz);
        if self.sign_block == Some(block) {
            return;
        }
        self.sign_block = Some(block);

        let info = settlements::sign_info_at_block(bx, bz);
        let biome = info.biome.label_es();
        // En el agua el cartel dice qué agua es (termal/río/mar/lago/pantano)
        // y si corre corriente.
        let at = glam::IVec3::new(bx, feet.y.floor() as i32 + 1, bz);
        let water = self.world.water_kind_at(at).map(|k| {
            let flow = self
                .world
                .water_level(glam::IVec3::new(bx, feet.y.floor() as i32, bz))
                .is_some_and(|l| l > 0);
            if flow {
                format!("{} · CORRIENTE", k.label_es())
            } else {
                k.label_es().to_string()
            }
        });
        self.sign_line1 = match &info.realm_name {
            Some(name) => match &water {
                Some(w) => format!("REINO {name} · {biome} · {w}"),
                None => format!("REINO {name} · {biome}"),
            },
            None => match &water {
                Some(w) => format!("TIERRA SALVAJE · {biome} · {w}"),
                None => format!("TIERRA SALVAJE · {biome}"),
            },
        };
        self.sign_dist_m = info.nearest.as_ref().map(|n| n.dist);
        self.sign_line2 = match &info.nearest {
            Some(n) => format!("{} A {} M", n.kind.label_es(), n.dist),
            None => "SIN ASENTAMIENTOS".to_string(),
        };
    }

    fn logical_size(&self) -> (f32, f32) {
        let Some(window) = self.window.as_ref() else {
            return (1280.0, 720.0);
        };
        let s = window.inner_size();
        let scale = window.scale_factor() as f32;
        (
            s.width as f32 / scale.max(0.01),
            s.height as f32 / scale.max(0.01),
        )
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            // Vuelta de suspensión (Android): la superficie nativa se
            // destruyó al pausar; reconfigurar re-engancha el swapchain
            // sin reconstruir meshes. En PC no hace nada (nunca suspende).
            if self.was_suspended {
                self.was_suspended = false;
                if let Some(window) = &self.window {
                    let size = window.inner_size();
                    if let Some(renderer) = self.renderer.as_mut() {
                        renderer.resize(size);
                    }
                    self.camera.aspect = size.width as f32 / size.height.max(1) as f32;
                    window.request_redraw();
                }
            }
            return;
        }

        let title = if ENABLE_HD2D {
            "Microverse — HD-2D"
        } else {
            "Microverse — first person"
        };
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Microverse — precargando mundo…")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0)),
                )
                .expect("create window"),
        );

        // Fill nearest shunks before GPU init so the first frame has solid ground.
        // Android: arranque rápido — el preload + warmup bloquean el hilo
        // principal minutos en el móvil (build debug, pocos núcleos) y la
        // ventana se queda en negro (riesgo de ANR). El streaming por frame
        // rellena el anillo keep en los primeros frames; el primer frame
        // presenta color de niebla + HUD de inmediato.
        let fast_boot = cfg!(target_os = "android");
        // El preload se hace al entrar en partida (`enter_playing`), no al
        // arrancar: con el menú en pantalla el mundo sigue sin generarse
        // (el renderer se crea igual, con el mundo vacío).
        let preload_now = self.screen == Screen::Playing && !fast_boot;
        if !DEBUG_FACE_VISIBILITY && preload_now {
            self.world
                .preload_shunks_around(self.player.focus_position());
            // Edits none at boot; leave stream dirties for warm_start_meshes.
            let _ = self.world.take_dirty_edits();
        }

        if preload_now {
            window.set_title("Microverse — precargando meshes…");
        }
        let mut renderer =
            pollster::block_on(Renderer::new(window.clone(), &self.world, &self.camera));

        let size = window.inner_size();
        self.camera.aspect = size.width as f32 / size.height.max(1) as f32;

        if !DEBUG_FACE_VISIBILITY && preload_now {
            renderer.warm_start_meshes(&mut self.world, &self.camera);
        }
        window.set_title(title);

        self.window = Some(window);
        self.renderer = Some(renderer);
        // Visible cursor from the start so hotbar / INV / HUD stay clickable.
        self.set_cursor_captured(false);
        self.last_frame = Instant::now();
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        // Android destruye la superficie nativa al pausar; `resumed()`
        // la reconfigura al volver (ver `was_suspended`). Se guarda la
        // partida aquí: en móvil "salir" es mandar al fondo, no cerrar.
        self.persist("suspend");
        self.was_suspended = true;
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.persist("exit");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size);
                }
                self.camera.aspect = size.width as f32 / size.height.max(1) as f32;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self
                    .window
                    .as_ref()
                    .map(|w| w.scale_factor() as f32)
                    .unwrap_or(1.0)
                    .max(0.01);
                self.mouse_logical = (position.x as f32 / scale, position.y as f32 / scale);
            }
WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let frame_secs = (now - self.last_frame).as_secs_f32().min(0.25);
                self.last_frame = now;

                let inst_fps = if frame_secs > 1e-4 {
                    1.0 / frame_secs
                } else {
                    999.0
                };
                let inst_ms = frame_secs * 1000.0;
                self.fps_ema = self.fps_ema * 0.90 + inst_fps * 0.10;
                self.frame_ms_ema = self.frame_ms_ema * 0.90 + inst_ms * 0.10;
                // Hitch sensor → HD-2D lens pull-in (see HITCH_* consts).
                self.hitch_pressure =
                    hitch_pressure_step(self.hitch_pressure, inst_ms, frame_secs);

                self.tick_accum += frame_secs * TICKS_PER_SECOND as f32;
                let mut ticks = self.tick_accum.floor() as u32;
                self.tick_accum -= ticks as f32;
                ticks = ticks.min(MAX_TICKS_PER_FRAME);
                // Menú / editor: sin juego no hay física que tickear (el
                // mundo ni siquiera se ha generado todavía).
                if self.screen != Screen::Playing {
                    ticks = 0;
                }

                // Android fast-boot: sin preload, el suelo bajo el spawn aún
                // no existe al arrancar. Congelar la física hasta que la
                // columna del jugador esté generada; si no, el héroe cae al
                // vacío en los primeros frames y la cámara queda bajo el
                // terreno. En PC el preload ya lo garantiza (no hace nada).
                if cfg!(target_os = "android")
                    && self.screen == Screen::Playing
                    && !self.boot_grounded
                {
                    let fx = self.player.feet.x.floor() as i32;
                    let fz = self.player.feet.z.floor() as i32;
                    if self.world.chunk_filled(
                        fx.div_euclid(crate::world::SHUNK_SIZE),
                        fz.div_euclid(crate::world::SHUNK_SIZE),
                    ) {
                        self.boot_grounded = true;
                        log::info!("boot: suelo del spawn listo, física en marcha");
                    } else {
                        ticks = 0;
                    }
                }

                // Pantalla de carga: sin preload el mundo "aparece de repente".
                // Mientras el anillo keep tenga huecos se muestra el progreso
                // y se congela la física (el jugador mira, pero no anda).
                // Timeout 15 s: si el streaming se atasca, se juega igual.
                if !self.load_done && self.screen == Screen::Playing {
                    let open = self
                        .renderer
                        .as_ref()
                        .map(|r| r.ring_holes_open())
                        .unwrap_or(usize::MAX);
                    if open > 0 {
                        self.load_total = self.load_total.max(open);
                    }
                    if open == 0 || self.boot_time.elapsed().as_secs_f32() >= 15.0 {
                        self.load_done = true;
                        self.load_done_at = Some(Instant::now());
                    } else {
                        ticks = 0;
                    }
                }

                // Táctil: joystick+salto → teclas ANTES del tick. El teclado
                // de PC sigue su propio camino por eventos (touch solo
                // escribe mientras hay dedos; al soltar limpia lo suyo).
                if self.touch.enabled && self.screen != Screen::Menu {
                    self.touch.apply_to_keys(&mut self.keys);
                }

                // Todo lo de juego (física, cámara, herramienta) vive aquí:
                // en menú / editor el mundo ni existe todavía.
                if !DEBUG_FACE_VISIBILITY && self.screen == Screen::Playing {
                    // Guardia de bloqueo (Shift+derecho con espada): se
                    // recalcula cada frame desde el input para que soltar
                    // Shift, el botón o cambiar de arma la cancele ya.
                    self.player.blocking = self.rmb_held
                        && self.wants_block()
                        && !self.stair.is_active()
                        && !self.inventory_open;
                    // Botas de fondo en el inventario: pisan el lecho.
                    self.player.sink_boots =
                        self.inventory.count_of(InvItem::DepthBoots) > 0;
                    let move_yaw = self.move_yaw();
                    let dig_pose = self.digging_for_pose();
                    self.player.tick(
                        &self.world,
                        &self.keys,
                        move_yaw,
                        ticks,
                        dig_pose,
                        self.camera.is_first_person(),
                    );
                    let alpha = self.tick_accum;
                    if ENABLE_HD2D {
                        // Auto first-person: house interior (roof+walls) or cave
                        // (buried/roofed via confine). Manual V toggles in the open.
                        let feet = self.player.display_feet(alpha);
                        let indoors = self.world.indoors_factor(feet + glam::Vec3::Y * 1.1) >= 0.55;
                        let confine_now = self.world.hd2d_confine_factor(feet + glam::Vec3::Y);
                        let cave = confine_now >= 0.85;
                        // Asymmetric debounce around the raw fence: sustain the new
                        // reading for a while before flipping the latch, so zones
                        // where `confine` crosses 0.85 frame-to-frame don't keep the
                        // mode blend from ever finishing.
                        let raw_auto = indoors || cave;
                        let prev_latch = self.auto_fp_latch;
                        if raw_auto != self.auto_fp_latch {
                            self.mode_debounce += frame_secs;
                            let need = if raw_auto {
                                AUTO_FP_ENTER_DEBOUNCE
                            } else {
                                AUTO_FP_EXIT_DEBOUNCE
                            };
                            if self.mode_debounce >= need {
                                self.auto_fp_latch = raw_auto;
                                self.mode_debounce = 0.0;
                            }
                        } else {
                            self.mode_debounce = 0.0;
                        }
                        // Entrada en auto-FP: recordar si fue por interior
                        // (puerta) o por cueva, para orientar el diorama
                        // solo al salir por puerta.
                        if !prev_latch && self.auto_fp_latch {
                            self.auto_fp_from_indoors = indoors;
                        }
                        self.auto_fp = self.auto_fp_latch;
                        // Salida por puerta (auto-FP por interior → diorama):
                        // acomoda la órbita al rumbo del personaje con
                        // Arriba-Derecha como referencia; si sale de espaldas
                        // (retrocediendo) la referencia es el opuesto y el
                        // rumbo queda en Abajo-Izquierda.
                        if prev_latch && !self.auto_fp_latch && self.auto_fp_from_indoors {
                            let facing = self.player.display_facing(alpha);
                            let flat_vel = glam::Vec3::new(
                                self.player.velocity.x,
                                0.0,
                                self.player.velocity.z,
                            );
                            let face_dir =
                                glam::Vec3::new(facing.cos(), 0.0, facing.sin());
                            let reverse =
                                flat_vel.length() > 0.4 && flat_vel.dot(face_dir) < 0.0;
                            self.camera.snap_orbit_to_face(facing, reverse);
                        }
                        // V es contextual mientras se mantiene: en diorama mete
                        // en 1ª persona; en auto-FP (cueva/casa) hace lo
                        // contrario — vistazo al diorama para orientarse y al
                        // soltar vuelve a 1ª persona. Picando manda el pico.
                        // En táctil, picar NO fuerza 1ª persona: en el móvil
                        // se pica andando en diorama (el PC conserva el
                        // vistazo forzado al mantener LMB).
                        let touch_mining =
                            self.touch.enabled && (self.touch_hold || self.touch.mine_held());
                        let want_fp = (self.mining_with_tool() && !touch_mining)
                            || (if self.auto_fp {
                                !self.fp_manual
                            } else {
                                self.fp_manual
                            });
                        if want_fp {
                            self.camera.request_first_person();
                        } else {
                            self.camera.request_hd2d();
                        }
                        self.camera.update_mode(frame_secs);
                        // Deferred Q/E orbit: advance any smooth rotation in
                        // progress, then, while a snap waits, start it as soon
                        // as the whole view bubble is settled (no 16×16 holes
                        // on the turn). Lens leaving diorama cancels the wait.
                        self.update_orbit_gate(frame_secs);
                        if self.camera.is_first_person() && !self.cursor_captured {
                            self.set_cursor_captured(true);
                        }
                        if !want_fp && self.cursor_captured {
                            self.set_cursor_captured(false);
                        }
                        let pressure = self
                            .renderer
                            .as_ref()
                            .map(|r| r.pop_pressure())
                            .unwrap_or(0.0);
                        // Stair ghost must track feet before framing the lens.
                        if self.stair.is_active() && self.stair.phase != StairPhase::Digging {
                            self.stair.sync_ghost_anchor(self.stair_anchor_block());
                        }
                        let player_focus = self.player.display_focus(alpha);
                        let (focus, frame_half) = if let Some(far) = self.stair.ghost_far_point() {
                            // Midpoint between hero and yellow-trail tip so the
                            // whole planned dig stays on screen while lengthening.
                            let mid = (player_focus + far) * 0.5;
                            let half = player_focus.distance(far) * 0.5;
                            (mid, half)
                        } else {
                            (player_focus, 0.0)
                        };
                        // Hitch pull-in pauses while planning a dig: the ghost
                        // frame wants room, and the player is not running.
                        let fps_pull = if frame_half > 0.0 {
                            0.0
                        } else {
                            self.hitch_pressure
                        };
                        // Lens pull-in (confine) no longer applies in mixed mode:
                        // seat_mixed already collapses to the eye when blended out.
                        // Lens pull-in (confine) no longer applies in mixed mode:
                        // seat_mixed already collapses to the eye when blended out.
                        self.camera.follow_hd2d_framed(
                            focus,
                            pressure,
                            0.0,
                            frame_half,
                            fps_pull,
                            frame_secs,
                        );
                        self.camera.seat_mixed(
                            self.player.display_eye(alpha),
                            self.player.display_facing(alpha),
                        );
                    } else {
                        self.camera.sync_from_player(self.player.display_eye(alpha));
                    }

                    // Stair dig tick (ghost already synced above for HD-2D framing).
                    if self.stair.is_active()
                        && self.stair.phase != StairPhase::Digging
                        && !ENABLE_HD2D
                    {
                        self.stair.sync_ghost_anchor(self.stair_anchor_block());
                    }
                    self.tick_stair_dig(now);
                    let hold_dt = ticks as f32 / TICKS_PER_SECOND as f32;
                    self.tick_hold_break(hold_dt.max(frame_secs));
                }

                // Táctil: drenar mirada. En 1ª persona gira al héroe (como el
                // ratón); en diorama el arrastre horizontal da pasos de
                // órbita (como Q/E, 60 px por paso).
                if self.touch.enabled {
                    let (tdx, tdy) = self.touch.take_look_delta();
                    if tdx != 0.0 || tdy != 0.0 {
                        if fp_mouse_turn_active(&self.camera) {
                            // Boost táctil: el gesto recorre cientos de px;
                            // sin él girar en 1ª persona se siente lento.
                            self.camera.apply_fp_turn(
                                tdx as f64 * TOUCH_FP_LOOK_BOOST as f64,
                                tdy as f64 * TOUCH_FP_LOOK_BOOST as f64,
                                &mut self.player,
                            );
                        } else if ENABLE_HD2D && !DEBUG_FACE_VISIBILITY {
                            self.touch.orbit_carry += tdx;
                            let steps = (self.touch.orbit_carry / 60.0).trunc() as i32;
                            if steps != 0 {
                                self.touch.orbit_carry -= steps as f32 * 60.0;
                                let f = self.stair_orbit_focus();
                                for _ in 0..steps.abs() {
                                    self.camera.request_orbit(steps.signum(), f);
                                }
                            }
                        }
                    }
                    // Dedo quieto en el mundo el tiempo suficiente: picado
                    // apuntado con hold (progreso como el LMB de PC).
                    if let Some((_hid, hx, hy)) = self.touch.poll_hold_mine() {
                        let (hw, hh) = self.logical_size();
                        let (o, d) = self.camera.screen_ray(hx, hy, hw, hh);
                        self.press_mine_aimed(o, d, true);
                    }
                    // Pellizco → zoom del diorama (el bias sobrevive al
                    // modo: al volver del FP se mantiene el encuadre).
                    let z = self.touch.take_zoom_delta();
                    if z != 0.0 {
                        self.camera.zoom_hd2d(z);
                    }
                }

                // Editor: la misma cámara HD-2D del juego, pero encuadrando
                // el foco del editor (el jugador no se mueve ni se dibuja).
                if self.screen == Screen::Editor {
                    self.editor.tick_fly(&self.keys, &self.camera, frame_secs);
                    self.camera
                        .follow_hd2d_framed(self.editor.focus, 0.0, 0.0, 0.0, 0.0, frame_secs);
                    if ENABLE_HD2D && !DEBUG_FACE_VISIBILITY {
                        // Q/E orbitan la vista del editor igual que en juego.
                        self.update_orbit_gate(frame_secs);
                    }
                    self.camera
                        .seat_mixed(self.editor.focus + glam::Vec3::Y * 0.9, self.player.facing);
                }

                let (lw, lh) = self.logical_size();
                // El HUD depende de la pantalla: en partida todo el HUD de
                // juego; en menú/editor solo el de esa pantalla. Las hit
                // regions viajan igual en `stair_hud`, así que toque y ratón
                // funcionan sin ruta nueva.
                self.stair_hud = match self.screen {
                    Screen::Playing => self.build_play_hud(lw, lh),
                    Screen::Menu => crate::hud::build_main_menu_hud(
                        self.menu_index,
                        self.has_save,
                        lw,
                        lh,
                    ),
                    Screen::Editor => {
                        let mut hud = crate::hud::build_editor_hud(
                            &self.editor.scene.name,
                            &self.editor.status,
                            &self.editor.scene.entities,
                            crate::hud::EditorView {
                                selected: self.editor.selected,
                                scroll: self.editor.scroll,
                                state_sel: self.editor.state_sel,
                                kf_sel: self.editor.kf_sel,
                                joint_sel: self.editor.joint_sel,
                                editing_joint: self.editor.target == StepTarget::Joint,
                                panel: self.editor.panel,
                            },
                            self.editor.clip.as_ref(),
                            lw,
                            lh,
                        );
                        // Joystick táctil para volar el foco (sus botones de
                        // picar no hacen nada en el editor: el panel manda).
                        if self.touch.enabled {
                            append_hud(&mut hud, &self.touch.build_touch_hud(lw, lh));
                        }
                        hud
                    }
                };

                self.title_frames = self.title_frames.wrapping_add(1);
                if self.last_autosave.elapsed().as_secs_f32() >= save::AUTOSAVE_SECS {
                    self.persist("auto");
                }
                // Android: escala dinámica de escena cada 60 frames + fps a
                // logcat cada ~5 s (en el móvil no hay título de ventana).
                if cfg!(target_os = "android") {
                    if self.title_frames % 60 == 0 {
                        let fps = self.fps_ema;
                        if let Some(renderer) = self.renderer.as_mut() {
                            renderer.auto_scene_scale(fps);
                        }
                    }
                    if self.title_frames % 300 == 0 {
                        log::info!(
                            "perf {:.0} fps · {:.1} ms · chunks:{} (dyn_scale {:.2}) · peak remesh {}×{:.0}ms+up{:.0}ms",
                            self.fps_ema,
                            self.frame_ms_ema,
                            self.world.loaded_chunk_count(),
                            self.renderer.as_ref().map(|r| r.scene_scale()).unwrap_or(1.0),
                            self.peak_rebuilt,
                            self.peak_mesh_ms,
                            self.peak_up_ms,
                        );
                        self.peak_rebuilt = 0;
                        self.peak_mesh_ms = 0.0;
                        self.peak_up_ms = 0.0;
                    }
                }
                if self.title_frames % 10 == 0 && self.screen == Screen::Playing {
                    let (dirt_n, grass_n) = self.world.voxel_counts();
                    let pressure = self
                        .renderer
                        .as_ref()
                        .map(|r| r.pop_pressure())
                        .unwrap_or(0.0);
                    if let Some(window) = &self.window {
                        let mode = if ENABLE_HD2D { "HD-2D" } else { "FPS" };
                        let dir = self.stair.hud_dir();
                        let pick = self.pickaxe.hud_tag();
                        let stair = match self.stair.phase {
                            StairPhase::Idle => String::new(),
                            StairPhase::Armed | StairPhase::Preview => {
                                format!(" | STAIR {dir}: HUD · Enter=dig")
                            }
                            StairPhase::Digging => format!(" | STAIR {dir}: digging…"),
                        };
                        let pi = self.inventory.count_of(InvItem::StoneFrag);
                        let ti = self.inventory.count_of(InvItem::DirtFrag);
                        let c_m = self.inventory.count_of(InvItem::CoalMicro);
                        let c_c = self.inventory.count_of(InvItem::CoalCube);
                        let s_m = self.inventory.count_of(InvItem::SapphireMicro);
                        let s_c = self.inventory.count_of(InvItem::SapphireCube);
                        let r_m = self.inventory.count_of(InvItem::RubyMicro);
                        let r_c = self.inventory.count_of(InvItem::RubyCube);
                        let e_m = self.inventory.count_of(InvItem::EmeraldMicro);
                        let e_c = self.inventory.count_of(InvItem::EmeraldCube);
                        let facing = TunnelFacing::from_yaw(self.player.facing).label_es();
                        window.set_title(&format!(
                            "Microverse — {mode} | {}{stair} | mira:{facing} | {pick} | Pi:{pi}/100 Ti:{ti}/100 | C:{c_m}/{c_c} S:{s_m}/{s_c} R:{r_m}/{r_c} E:{e_m}/{e_c} | {:.0} fps · {:.1} ms | chunks:{} dirt:{} grass:{} cam:{:.0}%",
                            self.player.hearts_title(),
                            self.fps_ema,
                            self.frame_ms_ema,
                            self.world.loaded_chunk_count(),
                            dirt_n,
                            grass_n,
                            pressure * 100.0,
                        ));
                    }
                }

                // Pared CPU del render (mallado+uploads+draws+submit+present):
                // si supera mucho a (mesh+upload), el resto es dibujado,
                // submit o espera de vsync/GPU. Vive fuera del `if let`
                // para el slow-warn de abajo.
                let mut render_wall_ms = 0.0;
                if self.screen == Screen::Menu {
                    // Menú: solo HUD sobre fondo plano. Nada de `render` →
                    // nada de streaming ni mallado del mundo.
                    if let Some(renderer) = self.renderer.as_mut() {
                        let res = renderer.render_ui_only(Some(&self.stair_hud));
                        if let Err(e) = res {
                            self.surface_failed(e, event_loop);
                        }
                    }
                } else if self.screen == Screen::Editor {
                    // Editor: el mundo real con la cámara HD-2D, sin jugador
                    // (`feet = None`). La entidad seleccionada se dibuja con su
                    // propio mesh (`preview`); las demás siguen siendo celdas
                    // resaltadas. Nada de fantasma de escalera ni grieta.
                    if let Some(renderer) = self.renderer.as_mut() {
                        let alpha = self.tick_accum;
                        let facing = self.player.display_facing(alpha);
                        let pose = self.player.display_hero_pose(alpha);
                        let preview = self.editor.preview();
                        let markers = self
                            .editor
                            .marker_cells(self.camera.position, preview.is_some());
                        let hud = Some(&self.stair_hud);
                        let render_t0 = Instant::now();
                        let render_res = renderer.render(
                            &self.camera,
                            &mut self.world,
                            None,
                            facing,
                            &pose,
                            None,
                            0.0,
                            0.0,
                            &[],
                            &markers,
                            None,
                            &[],
                            preview,
                            hud,
                        );
                        render_wall_ms = render_t0.elapsed().as_secs_f32() * 1000.0;
                        if let Err(e) = render_res {
                            self.surface_failed(e, event_loop);
                        }
                    }
                } else if let Some(renderer) = self.renderer.as_mut() {
                    let alpha = self.tick_accum;
                    let feet = if ENABLE_HD2D && !DEBUG_FACE_VISIBILITY {
                        // 1ª persona recortada a 1/3 (ver render.rs): mirar abajo
                        // muestra solo piernas/pies, sin torso/cabeza.
                        Some(self.player.display_feet(alpha))
                    } else {
                        None
                    };
                    let facing = self.player.display_facing(alpha);
                    let pose = self.player.display_hero_pose(alpha);
                    let tool_swing = self.player.display_tool_swing(alpha);
                    let equip = self.player.display_equip_blend(alpha);
                    // Stair blueprint keeps its X-ray yellow ghost. FP block
                    // highlights (aimed white, stood-on cyan) ride the
                    // depth-tested highlight pass instead — no X-ray.
                    let ghost: Vec<(glam::IVec3, [f32; 3])> = self
                        .stair
                        .ghost_cells()
                        .iter()
                        .map(|&c| (c, [1.0, 0.92, 0.22]))
                        .collect();
                    let mut highlight: Vec<(glam::IVec3, [f32; 3])> = Vec::new();
                    // Sombra redonda bajo los pies (disco gris, no cubo).
                    let mut stood_disc: Option<(glam::Vec3, [f32; 3])> = None;
                    if self.camera.is_first_person() {
                        if let Some(hit) =
                            raycast_reach(&self.world, self.camera.position, self.camera.forward())
                        {
                            // Punto de mira: gris semi-transparente (el pass
                            // mezcla al 55%; la grieta de picado lo tiñe).
                            highlight.push((hit.block, [0.6, 0.6, 0.6]));
                        }
                        if let Some(f) = feet {
                            let stood = glam::IVec3::new(
                                f.x.floor() as i32,
                                (f.y - 0.05).floor() as i32,
                                f.z.floor() as i32,
                            );
                            stood_disc = Some((
                                glam::Vec3::new(
                                    stood.x as f32 + 0.5,
                                    stood.y as f32 + 1.0 + 0.02,
                                    stood.z as f32 + 0.5,
                                ),
                                [0.2, 0.2, 0.22],
                            ));
                        }
                    }
                    // Grieta de rotura estilo Minecraft: velo gris por etapas
                    // sobre el bloque picado (alfa ~0.12, sin tintes).
                    // Vale en diorama y 1ª persona.
                    let mut crack: Vec<(glam::IVec3, [f32; 3])> = Vec::new();
                    if let Some(target) = self.mining.target {
                        crack.push((target, mining_crack_gray(self.mining.t)));
                    }
                    let hud = Some(&self.stair_hud);
                    // Pared CPU del render (mallado+uploads+draws+submit+present):
                    // si supera mucho a (mesh+upload), el resto es dibujado,
                    // submit o espera de vsync/GPU. Solo se usa en el warn.
                    let render_t0 = Instant::now();
                    let render_res = renderer.render(
                        &self.camera,
                        &mut self.world,
                        feet,
                        facing,
                        &pose,
                        self.hotbar.tool_id(),
                        tool_swing,
                        equip,
                        &ghost,
                        &highlight,
                        stood_disc,
                        &crack,
                        None,
                        hud,
                    );
                    render_wall_ms = render_t0.elapsed().as_secs_f32() * 1000.0;
                    if let Err(e) = render_res {
                        self.surface_failed(e, event_loop);
                    }
                }

                // Picos de remallado para el log `perf` (barato: 3 floats).
                // Slow frames (>100 ms) se avisan con desglose: el `perf`
                // cada 300 frames pierde el arranque (1-2 frames en 30 s) y
                // el warn raro sobrevive al spam de MIUI en el buffer.
                if let Some(renderer) = self.renderer.as_ref() {
                    let (rb, mm, um) = renderer.mesh_timings();
                    self.peak_rebuilt = self.peak_rebuilt.max(rb);
                    if mm > self.peak_mesh_ms {
                        self.peak_mesh_ms = mm;
                    }
                    if um > self.peak_up_ms {
                        self.peak_up_ms = um;
                    }
                    if inst_ms > 100.0 {
                        log::warn!(
                            "slow frame {:.0} ms | rebuilt:{} mesh:{:.0}ms up:{:.0}ms render_wall:{:.0}ms pendientes:{}",
                            inst_ms,
                            rb,
                            mm,
                            um,
                            render_wall_ms,
                            renderer.rebuild_queue_len(),
                        );
                    }
                }

                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                if cfg!(target_os = "android") {
                    // Cap 60 fps en móvil: Fifo va al Hz del panel (hasta
                    // 120 Hz) y funde batería/termal sin dar más juego
                    // (la física va a 20 ticks). En PC no se toca.
                    let used = now.elapsed();
                    let budget = std::time::Duration::from_secs_f32(1.0 / 60.0);
                    if used < budget {
                        std::thread::sleep(budget - used);
                    }
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(key),
                        state,
                        repeat,
                        ..
                    },
                ..
            } => {
                let pressed = state == ElementState::Pressed;
                // Solo el flanco inicial: winit re-emite Pressed con
                // `repeat: true` al mantener. Las acciones edge-triggered
                // (F5/G/T/F) deben disparar una vez por pulsación física —
                // sin esto, rozar F al mashear V/Q/E ametralla
                // `hero_winding_flip` y el héroe queda inside-out
                // ("culling girado") hasta pulsar F de nuevo.
                let pressed_once = is_initial_press(pressed, repeat);
                // Editor: atajos + vuelo. El resto de teclas de juego (F5, G,
                // T, F, 1-4) NO se tocan aquí, pero WASD/Ratón/Shift sí
                // alimentan `self.keys`, que es lo que vuela el foco.
                if self.screen == Screen::Editor {
                    if pressed_once {
                        // Ctrl+Z / Ctrl+Shift+Z: historial del documento.
                        if self.keys.down {
                            let hist = if self.keys.sprint {
                                EditorAction::Redo
                            } else {
                                EditorAction::Undo
                            };
                            self.editor.apply(hist);
                            return;
                        }
                        let step: Option<EditorAction> = match key {
                            KeyCode::Escape => {
                                self.screen = Screen::Menu;
                                return;
                            }
                            KeyCode::KeyI => Some(EditorAction::Add),
                            KeyCode::KeyO => Some(EditorAction::SelPrev),
                            KeyCode::KeyP => Some(EditorAction::SelNext),
                            KeyCode::Delete | KeyCode::Backspace => Some(EditorAction::Delete),
                            KeyCode::F2 => Some(EditorAction::Save),
                            KeyCode::F4 => Some(EditorAction::Load),
                            KeyCode::BracketLeft => Some(EditorAction::SclXDec),
                            KeyCode::BracketRight => Some(EditorAction::SclXInc),
                            KeyCode::Semicolon => Some(EditorAction::RotYDec),
                            KeyCode::Quote => Some(EditorAction::RotYInc),
                            KeyCode::Minus => Some(EditorAction::SizeDec),
                            KeyCode::Equal => Some(EditorAction::SizeInc),
                            _ => None,
                        };
                        if let Some(a) = step {
                            self.editor.apply(a);
                            return;
                        }
                    }
                    // Q/E orbitan la vista del editor (igual que en juego).
                    if pressed && ENABLE_HD2D && !DEBUG_FACE_VISIBILITY {
                        let focus = self.editor.focus;
                        match key {
                            KeyCode::KeyQ => {
                                self.camera.request_orbit(-1, focus);
                                return;
                            }
                            KeyCode::KeyE => {
                                self.camera.request_orbit(1, focus);
                                return;
                            }
                            _ => {}
                        }
                    }
                    self.keys.set(key, pressed);
                    return;
                }
                // Menú: el teclado solo mueve el resaltado y activa la fila.
                if self.screen == Screen::Menu {
                    if pressed_once {
                        match key {
                            KeyCode::ArrowUp | KeyCode::KeyW => self.move_menu_row(-1),
                            KeyCode::ArrowDown | KeyCode::KeyS => self.move_menu_row(1),
                            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => {
                                self.activate_menu_row()
                            }
                            KeyCode::Escape => {
                                self.persist("exit");
                                event_loop.exit();
                            }
                            _ => {}
                        }
                    }
                    return;
                }
                if key == KeyCode::Escape && pressed {
                    if self.inventory_open {
                        self.close_inventory();
                    } else if self.stair.is_active() {
                        self.stair.cancel_to_idle();
                        self.sync_cursor_for_stair();
                    } else if self.cursor_captured {
                        self.set_cursor_captured(false);
                    } else {
                        self.persist("exit");
                        event_loop.exit();
                    }
                } else if key == KeyCode::F5 && pressed_once {
                    self.persist("F5");
                } else if key == KeyCode::KeyG && pressed_once {
                    self.toggle_inventory();
                } else if pressed
                    && matches!(
                        key,
                        KeyCode::Digit1
                            | KeyCode::Digit2
                            | KeyCode::Digit3
                            | KeyCode::Digit4
                            | KeyCode::Numpad1
                            | KeyCode::Numpad2
                            | KeyCode::Numpad3
                            | KeyCode::Numpad4
                    )
                {
                    let slot = match key {
                        KeyCode::Digit1 | KeyCode::Numpad1 => 0,
                        KeyCode::Digit2 | KeyCode::Numpad2 => 1,
                        KeyCode::Digit3 | KeyCode::Numpad3 => 2,
                        _ => 3,
                    };
                    self.hotbar.press_slot(slot, false);
                    self.player.notify_equip();
                    // Stowing the pickaxe cancels an armed dig tool.
                    if !self.hotbar.holding_pickaxe() && self.stair.is_active() {
                        self.stair.cancel_to_idle();
                        self.sync_cursor_for_stair();
                    }
                } else if key == KeyCode::KeyT && pressed_once && !DEBUG_FACE_VISIBILITY {
                    // Stair tool only while the pickaxe is in hand.
                    if self.hotbar.holding_pickaxe() {
                        self.stair.arm_from_yaw(self.player.facing);
                        self.sync_cursor_for_stair();
                    } else {
                        log::info!("equipa el pico (2) para cavar — 2 otra vez para guardarlo");
                    }
                } else if self.stair.is_active()
                    && pressed
                    && !DEBUG_FACE_VISIBILITY
                    && self.handle_stair_dir_key(key)
                {
                    // Facing / incline / length / dig consumed.
                } else if key == KeyCode::KeyV && ENABLE_HD2D && !DEBUG_FACE_VISIBILITY {
                    // V contextual (mantener): diorama→1ª persona, y en
                    // auto-FP al revés (vistazo al diorama para orientarse).
                    if self.fp_manual != pressed {
                        self.fp_manual = pressed;
                        log::info!("V = {}", self.fp_manual);
                    }
                } else if key == KeyCode::KeyF && pressed_once {
                    if let Some(r) = self.renderer.as_mut() {
                        let flip = r.toggle_hero_winding_flip();
                        // WARN a propósito cuando entra: sin esta línea un
                        // roce accidental deja al héroe inside-out
                        // persistente y el "culling girado" es un misterio.
                        if flip {
                            log::warn!(
                                "hero winding FLIP activo (F) — culling del revés hasta pulsar F otra vez"
                            );
                        } else {
                            log::info!("hero winding restaurado (F)");
                        }
                    }
                } else if ENABLE_HD2D && !DEBUG_FACE_VISIBILITY && pressed {
                    match key {
                        KeyCode::KeyQ => {
                            let f = self.stair_orbit_focus();
                            self.camera.request_orbit(-1, f);
                        }
                        KeyCode::KeyE => {
                            let f = self.stair_orbit_focus();
                            self.camera.request_orbit(1, f);
                        }
                        _ => self.keys.set(key, pressed),
                    }
                } else {
                    self.keys.set(key, pressed);
                }
            }
            WindowEvent::Touch(t) => {
                // Táctil: el menú de PC no se entera (sus hits no incluyen
                // botones táctiles; ver touch.rs). En PC el primer toque
                // enciende el modo; en Android nace encendido.
                if !self.touch.enabled {
                    self.touch.enabled = true;
                    log::info!("touch mode: HUD táctil activo, menú de PC intacto");
                }
                let scale = self
                    .window
                    .as_ref()
                    .map(|w| w.scale_factor() as f32)
                    .unwrap_or(1.0)
                    .max(0.01);
                let (lw, lh) = self.logical_size();
                let x = t.location.x as f32 / scale;
                let y = t.location.y as f32 / scale;
                let was_mine = self.touch.mine_held();
                for action in self.touch.handle(t.phase, t.id, x, y, lw, lh) {
                    match action {
                        TouchAction::ToggleCam => {
                            if ENABLE_HD2D && !DEBUG_FACE_VISIBILITY {
                                self.fp_manual = !self.fp_manual;
                            }
                        }
                        TouchAction::ToggleInv => self.toggle_inventory(),
                        TouchAction::Tap(tx, ty) => self.tap_at(tx, ty, lw, lh),
                        TouchAction::TapMineEnd => self.stop_touch_mine(),
                    }
                }
                // MINE por flanco: pulsar = como LMB; soltar = como soltar LMB.
                let mine_now = self.touch.mine_held();
                if mine_now && !was_mine {
                    if ENABLE_HD2D && !DEBUG_FACE_VISIBILITY {
                        self.press_mine_hd2d();
                        self.touch_hold = self.attack_held;
                    } else {
                        self.break_targeted_block();
                    }
                } else if !mine_now && was_mine {
                    self.stop_touch_mine();
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                if !pressed {
                    if button == MouseButton::Left {
                        self.attack_held = false;
                        self.mining.clear();
                    }
                    if button == MouseButton::Right {
                        self.rmb_held = false;
                    }
                    return;
                }

                if matches!(button, MouseButton::Left | MouseButton::Right) {
                    if let Some(action) = hit_test(
                        &self.stair_hud.hits,
                        self.mouse_logical.0,
                        self.mouse_logical.1,
                    ) {
                        self.apply_hud_click(action, button == MouseButton::Right);
                        return;
                    }
                }

                // Editor: el ratón solo pulsa botones del panel; el mundo no
                // se pica ni se coloca nada.
                if self.screen == Screen::Editor {
                    return;
                }

                if self.stair.is_active() {
                    // Stair mode: mouse only talks to the on-screen panel.
                    if button == MouseButton::Right {
                        self.stair.cancel_preview();
                    }
                    return;
                }

                if self.inventory_open {
                    // Inventory open: clicks only hit HUD slots / INV button.
                    return;
                }

                // HD-2D: free cursor — click always swings; pick hold still mines.
                if ENABLE_HD2D && !DEBUG_FACE_VISIBILITY {
                    if button == MouseButton::Right {
                        // Shift+derecho con espada = guardia de bloqueo
                        // (no coloca nada); sin espada/Shift no hace nada.
                        self.rmb_held = true;
                        return;
                    }
                    if button == MouseButton::Left {
                        self.press_mine_hd2d();
                    }
                    return;
                }

                // FPS: first world click captures for look; HUD already handled above.
                if !self.cursor_captured {
                    self.set_cursor_captured(true);
                    return;
                }
                match button {
                    MouseButton::Left => {
                        // Shift+clic con espada = estocada baja (además de picar).
                        if self.hotbar.holding_sword() && self.keys.sprint {
                            let hit = raycast_reach(
                                &self.world,
                                self.camera.position,
                                self.camera.forward(),
                            );
                            self.player.begin_sword_thrust(hit.is_none());
                        }
                        self.break_targeted_block();
                    }
                    MouseButton::Right => {
                        if self.wants_block() {
                            // Shift+derecho con espada = guardia (no coloca).
                            self.rmb_held = true;
                        } else {
                            self.place_against_targeted_block();
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: winit::event::DeviceId,
        event: DeviceEvent,
    ) {
        if let DeviceEvent::MouseMotion { delta } = event {
            if self.cursor_captured {
                // HD-2D: camera never free-looks; mouse is unused for facing.
                // In first-person (auto or manual) the mouse turns the hero.
                if !ENABLE_HD2D || DEBUG_FACE_VISIBILITY {
                    self.camera.apply_mouse_delta(delta.0, delta.1);
                } else if fp_mouse_turn_active(&self.camera) {
                    self.camera
                        .apply_fp_turn(delta.0, delta.1, &mut self.player);
                }
            }
        }
    }
}

/// Mouse turns the hero only in first person (cursor is captured there).
/// HD-2D diorama never free-looks. (This gate was once inverted, which left
/// FP view fixed and forced players to aim with strafe diagonals.)
fn fp_mouse_turn_active(camera: &Camera) -> bool {
    ENABLE_HD2D && !DEBUG_FACE_VISIBILITY && camera.is_first_person()
}

/// Flanco inicial de tecla (auto-repeat excluido): las acciones
/// edge-triggered (flip de winding con F, inventario con G, pico con T,
/// guardado con F5) deben disparar una vez por pulsación física. winit
/// re-emite `Pressed` con `repeat: true` al mantener — sin este filtro,
/// rozar F al mashear V/Q/E ametralla `hero_winding_flip` y el héroe queda
/// inside-out persistente ("culling girado").
#[inline]
fn is_initial_press(pressed: bool, repeat: bool) -> bool {
    pressed && !repeat
}

/// Punto de entrada compartido escritorio + Android.
pub fn run() {
    init_logging();
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    rayon::ThreadPoolBuilder::new()
        .num_threads(cores)
        .build_global()
        .expect("rayon pool");
    log::info!("rayon pool: {cores} hilos");

    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new();
    event_loop.run_app(&mut app).expect("run app");
}

fn init_logging() {
    #[cfg(target_os = "android")]
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info),
    );
    #[cfg(not(target_os = "android"))]
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
}

/// Entrada Android (NativeActivity): mismo `App`, event loop de winit.
#[cfg(target_os = "android")]
#[no_mangle]
pub fn android_main(app: winit::platform::android::activity::AndroidApp) {
    use winit::platform::android::EventLoopBuilderExtAndroid;
    // Guardado en almacenamiento interno de la app.
    if let Some(dir) = app.internal_data_path() {
        crate::save::set_external_dir(dir);
    }
    let event_loop = EventLoop::builder()
        .with_android_app(app)
        .build()
        .expect("event loop");
    run_with_loop(event_loop);
}

/// Corre el loop dado (Android construye el suyo con la activity).
#[cfg(target_os = "android")]
fn run_with_loop(event_loop: EventLoop<()>) {
    init_logging();
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    // build_global falla si ya existe: ignora en reintentos de la activity.
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(cores)
        .build_global();
    log::info!("rayon pool: {cores} hilos (android)");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new();
    event_loop.run_app(&mut app).expect("run app");
}

#[cfg(test)]
mod hitch_tests {
    use super::*;

    #[test]
    fn spike_slams_and_smooth_frames_release() {
        let dt = 1.0 / 60.0;
        assert_eq!(hitch_pressure_step(0.0, 40.0, dt), 1.0);
        // Steady 20 ms (old dead band) must drain, not pin.
        let mut p = 1.0;
        for _ in 0..240 {
            p = hitch_pressure_step(p, 20.0, dt);
        }
        assert_eq!(p, 0.0);
    }

    #[test]
    fn mouse_turns_hero_only_in_first_person() {
        use glam::Vec3;
        // Diorama: mouse never turns the hero (cursor drives the HUD).
        let diorama = Camera::hd2d_follow(Vec3::ZERO);
        assert!(!fp_mouse_turn_active(&diorama));
        // Pure first person: mouse turns.
        let fp = Camera::first_person(Vec3::new(8.0, 22.0, 24.0));
        assert!(fp_mouse_turn_active(&fp));
        // Blending in: still diorama until the weight settles.
        let mut blend = Camera::hd2d_follow(Vec3::ZERO);
        blend.request_first_person();
        blend.update_mode(0.1);
        assert!(!fp_mouse_turn_active(&blend));
        blend.update_mode(10.0);
        assert!(fp_mouse_turn_active(&blend));
    }

    #[test]
    fn release_is_faster_when_smooth() {        let dt = 1.0 / 60.0;
        let mut fast = 1.0;
        let mut slow = 1.0;
        for _ in 0..30 {
            fast = hitch_pressure_step(fast, 16.7, dt);
            slow = hitch_pressure_step(slow, 24.0, dt);
        }
        assert!(fast < slow);
        assert!(slow < 1.0, "near-limit frames still creep out");
    }

    #[test]
    fn edge_actions_ignore_key_repeat() {
        // Flanco inicial: dispara. Repetición por mantener: no (si no,
        // rozar F al mashear V/Q/E ametralla el flip de winding y el héroe
        // queda inside-out persistente). Soltar: nunca dispara.
        assert!(is_initial_press(true, false));
        assert!(!is_initial_press(true, true));
        assert!(!is_initial_press(false, false));
        assert!(!is_initial_press(false, true));
    }

    #[test]
    fn menu_highlight_wraps_both_ways() {
        let rows = crate::hud::MENU_ROWS.len();
        assert_eq!(rows, 3, "el menú sigue siendo partida nueva / guardada / editor");
        // Abajo del último → primero; arriba del primero → último.
        assert_eq!(menu_row_step(0, -1, rows), 2);
        assert_eq!(menu_row_step(2, 1, rows), 0);
        // Pasos normales y sin movimiento.
        assert_eq!(menu_row_step(0, 1, rows), 1);
        assert_eq!(menu_row_step(1, -1, rows), 0);
        assert_eq!(menu_row_step(1, 0, rows), 1);
        // Sin filas no hay índice que devolver (no debe panicear).
        assert_eq!(menu_row_step(0, 1, 0), 0);
    }

    #[test]
    fn editor_selection_wraps_and_scroll_follows() {
        let mut ed = EditorState::new(glam::Vec3::ZERO);
        assert_eq!(ed.scene.entities.len(), 1);
        // Add grows the list and selects the newcomer.
        ed.apply(EditorAction::Add);
        ed.apply(EditorAction::Add);
        assert_eq!(ed.scene.entities.len(), 3);
        assert_eq!(ed.selected, 2);
        // Backwards from the first lands on the last, and the panel scrolls
        // so the selection is always inside the visible window.
        ed.apply(EditorAction::SelPrev);
        ed.apply(EditorAction::SelPrev);
        ed.apply(EditorAction::SelPrev);
        assert_eq!(ed.selected, 2);
        assert!(ed.scroll + crate::hud::EDITOR_VISIBLE_ROWS > ed.selected);
        // Deleting the selection keeps a valid index.
        ed.apply(EditorAction::Delete);
        assert_eq!(ed.scene.entities.len(), 2);
        assert!(ed.selected < ed.scene.entities.len());
    }

    #[test]
    fn editor_transform_steps_are_clamped() {
        let mut ed = EditorState::new(glam::Vec3::ZERO);
        // Shrink scale to the floor and stop there.
        for _ in 0..200 {
            ed.apply(EditorAction::SclXDec);
        }
        let e = ed.selected_entity().expect("starter entity");
        assert!((e.scale[0] - ED_SCALE_RANGE.0).abs() < 1e-4, "{}", e.scale[0]);
        // Rotation wraps instead of growing forever.
        for _ in 0..30 {
            ed.apply(EditorAction::RotYInc);
        }
        let e = ed.selected_entity().expect("starter entity");
        assert!(e.rotation[1] <= 180.0 && e.rotation[1] >= -180.0, "{}", e.rotation[1]);
        // Box size stays inside its range too.
        for _ in 0..200 {
            ed.apply(EditorAction::SizeInc);
        }
        let e = ed.selected_entity().expect("starter entity");
        assert!((e.size[0] - ED_SIZE_RANGE.1).abs() < 1e-3, "{}", e.size[0]);
    }

    #[test]
    fn editor_kind_cycles_through_the_offered_tags() {
        let mut ed = EditorState::new(glam::Vec3::ZERO);
        let first = ed.selected_entity().unwrap().kind.clone();
        ed.apply(EditorAction::KindNext);
        let second = ed.selected_entity().unwrap().kind.clone();
        assert_ne!(first, second);
        assert!(crate::editor::KINDS.contains(&second.as_str()));
        // A full lap returns to the start (one step was already taken).
        for _ in 1..crate::editor::KINDS.len() {
            ed.apply(EditorAction::KindNext);
        }
        assert_eq!(ed.selected_entity().unwrap().kind, first);
    }

    #[test]
    fn editor_menu_starts_closed_and_toggles_panels() {
        let mut ed = EditorState::new(glam::Vec3::ZERO);
        assert!(ed.panel.is_none(), "el editor abre con el menú cerrado");
        ed.apply(EditorAction::OpenPanel(EditorPanel::Transform));
        assert_eq!(ed.panel, Some(EditorPanel::Transform));
        // Cambiar de categoría con el menú abierto no lo cierra.
        ed.apply(EditorAction::OpenPanel(EditorPanel::Archivo));
        assert_eq!(ed.panel, Some(EditorPanel::Archivo));
        ed.apply(EditorAction::ClosePanel);
        assert!(ed.panel.is_none());
        // El estado del panel no gatesa los pasos de transform (teclado).
        let before = ed.selected_entity().expect("starter").scale[0];
        ed.apply(EditorAction::SclXInc);
        assert!(ed.selected_entity().expect("starter").scale[0] > before);
        // Guardar / cargar no tocan el panel abierto.
        ed.apply(EditorAction::OpenPanel(EditorPanel::Transform));
        ed.apply(EditorAction::Add);
        assert_eq!(ed.panel, Some(EditorPanel::Transform));
    }

    #[test]
    fn editor_states_cycle_and_own_the_transform_while_they_exist() {
        let mut ed = EditorState::new(glam::Vec3::ZERO);
        // Entidad estilo v1: sin estados, los pasos editan la entidad.
        assert_eq!(ed.state_len(), 0);
        let ent = ed.selected_entity().expect("starter").scale[0];
        ed.apply(EditorAction::SclXInc);
        assert!((ed.selected_entity().unwrap().scale[0] - ent - ED_SCALE_STEP).abs() < 1e-5);

        // El primer estado nace con el mesh de la entidad.
        ed.apply(EditorAction::StateAdd);
        assert_eq!(ed.state_len(), 1);
        assert_eq!(ed.state_sel, 0);
        let model = ed.selected_entity().unwrap().model.clone();
        assert_eq!(ed.selected_entity().unwrap().effective_model(0), model);

        // A partir de ahí los pasos editan el ESTADO activo, no la entidad.
        let ent = ed.selected_entity().unwrap().scale[0];
        let st = ed.selected_entity().unwrap().state(0).unwrap().scale[0];
        ed.apply(EditorAction::SclXInc);
        assert!(
            (ed.selected_entity().unwrap().scale[0] - ent).abs() < 1e-6,
            "la entidad no debe moverse"
        );
        assert!(
            (ed.selected_entity().unwrap().state(0).unwrap().scale[0] - st - ED_SCALE_STEP).abs() < 1e-5
        );
        // `size` es la caja del marcador: siempre en la entidad.
        let size = ed.selected_entity().unwrap().size[0];
        ed.apply(EditorAction::SizeInc);
        assert!(ed.selected_entity().unwrap().size[0] > size);
        assert_eq!(ed.selected_entity().unwrap().state(0).unwrap().scale[0], st + ED_SCALE_STEP);

        // Ciclar, duplicar y borrar.
        ed.apply(EditorAction::StateAdd); // 2 estados, sel = 1
        assert_eq!(ed.state_sel, 1);
        ed.apply(EditorAction::StatePrev);
        assert_eq!(ed.state_sel, 0);
        ed.apply(EditorAction::StateNext);
        assert_eq!(ed.state_sel, 1);
        ed.apply(EditorAction::StateDup); // copia el 1 → 3 estados, sel = 2
        assert_eq!(ed.state_len(), 3);
        assert_eq!(ed.state_sel, 2);
        ed.apply(EditorAction::StateDel);
        assert_eq!(ed.state_len(), 2);
        assert_eq!(ed.state_sel, 1, "al borrar se recorta al último");
        ed.apply(EditorAction::StateSel(0));
        assert_eq!(ed.state_sel, 0);
        ed.apply(EditorAction::StateSel(9));
        assert_eq!(ed.state_sel, 1, "un índice fuera de rango se recorta al último");

        // Sin estados, los pasos vuelven a la entidad.
        ed.apply(EditorAction::StateDel);
        ed.apply(EditorAction::StateDel);
        assert_eq!(ed.state_len(), 0);
        assert_eq!(ed.state_sel, 0);
        let ent = ed.selected_entity().unwrap().scale[0];
        ed.apply(EditorAction::SclXDec);
        assert!((ed.selected_entity().unwrap().scale[0] - ent + ED_SCALE_STEP).abs() < 1e-5);
    }

    /// El bucle de autoría de la Fase 6 (C): `IMPORTAR` cicla los clips que el
    /// juego ya sabe leer y `EXPORTAR` no se dispara sin clip abierto. El
    /// export en sí (que no debe pisar el importado) se cubre en
    /// `editor_clip`, contra un directorio temporal: aquí no se escribe en
    /// `saves/`.
    #[test]
    fn editor_clip_load_cycles_and_save_needs_a_clip() {
        let mut ed = EditorState::new(glam::Vec3::ZERO);
        assert!(ed.clip.is_none());
        // Sin clip, exportar lo dice y no escribe nada.
        ed.apply(EditorAction::ClipSave);
        assert!(ed.clip_saved_to.is_none(), "no debe escribir sin clip");
        assert!(ed.status.contains("no hay clip"), "{}", ed.status);

        // Importar: los assets del repo existen, así que carga el primero.
        ed.apply(EditorAction::ClipLoad);
        let first = ed.clip.as_ref().expect("clip importado").summary();
        assert!(ed.status.contains("clip"), "{}", ed.status);
        // Y pulsando otra vez avanza al siguiente (no se queda siempre el mismo).
        ed.apply(EditorAction::ClipLoad);
        let second = ed.clip.as_ref().expect("clip importado").summary();
        let files = editor_clip::EditorClip::importable();
        if files.len() > 1 {
            assert_ne!(first, second, "IMPORTAR deberia ciclar");
        }
        // El clip importado es del juego: su id está en CLIP_NAMES.
        let id = &ed.clip.as_ref().unwrap().file.id;
        assert!(
            crate::animation::CLIP_NAMES.contains(&id.as_str()),
            "{id} no es un clip del juego"
        );
        // Volver a cargar limpia la marca de exportado anterior.
        assert!(ed.clip_saved_to.is_none());
    }

    /// Lo que hay que deshacer es el documento: escena **y** clip. Un undo que
    /// solo guardara la escena dejaría fuera justo los keyframes, que es lo que
    /// lo hace bloqueante (`docs/plan_fase6.md`, decisión 5).
    #[test]
    fn editor_undo_restores_the_whole_document() {
        let mut ed = EditorState::new(glam::Vec3::ZERO);
        let start = ed.selected_entity().unwrap().position[0];

        // Tres pasos de transform, luego tres deshacer.
        for _ in 0..3 {
            ed.apply(EditorAction::PosXInc);
        }
        let moved = ed.selected_entity().unwrap().position[0];
        assert!(moved > start);
        for _ in 0..3 {
            ed.apply(EditorAction::Undo);
        }
        assert!((ed.selected_entity().unwrap().position[0] - start).abs() < 1e-6);
        // Y no hay más historial que deshacer.
        ed.apply(EditorAction::Undo);
        assert!(ed.status.contains("nada que deshacer"), "{}", ed.status);

        // Rehacer devuelve el estado movido, paso a paso: el primer redo
        // deshace el último undo, no salta al final de la pila.
        let after_undo = ed.selected_entity().unwrap().position[0];
        ed.apply(EditorAction::Redo);
        let after_redo = ed.selected_entity().unwrap().position[0];
        assert!((after_redo - after_undo - ED_POS_STEP).abs() < 1e-6);
        ed.apply(EditorAction::Redo);
        ed.apply(EditorAction::Redo);
        assert!((ed.selected_entity().unwrap().position[0] - moved).abs() < 1e-6);
        ed.apply(EditorAction::Redo);
        assert!(ed.status.contains("nada que rehacer"), "{}", ed.status);
    }

    /// Un `Delete` deja el cursor apuntando a otra entidad: el undo tiene que
    /// traer de vuelta también la selección, o el panel miente.
    #[test]
    fn editor_undo_restores_the_cursors() {
        let mut ed = EditorState::new(glam::Vec3::ZERO);
        ed.apply(EditorAction::Add);
        ed.apply(EditorAction::Add);
        assert_eq!(ed.scene.entities.len(), 3);
        let sel = ed.selected;
        let total = ed.scene.entities.len();

        ed.apply(EditorAction::Delete);
        assert_eq!(ed.scene.entities.len(), total - 1);
        ed.apply(EditorAction::Undo);
        assert_eq!(ed.scene.entities.len(), total);
        assert_eq!(ed.selected, sel, "la selección vuelve con la escena");
        assert!(ed.selected < ed.scene.entities.len());
    }

    /// Cambiar de documento corta el historial: deshacer a través de un `Load`
    /// mentiría sobre qué paso se estaba deshaciendo.
    #[test]
    fn editor_loading_a_clip_clears_history() {
        let mut ed = EditorState::new(glam::Vec3::ZERO);
        ed.apply(EditorAction::PosXInc);
        assert!(!ed.undo.is_empty());
        ed.apply(EditorAction::ClipLoad);
        assert!(ed.clip.is_some(), "el repo trae clips para importar");
        assert!(ed.undo.is_empty() && ed.redo.is_empty());
        // Y deshacer ahora no toca lo importado.
        let before = ed.clip.as_ref().unwrap().summary();
        ed.apply(EditorAction::Undo);
        assert_eq!(ed.clip.as_ref().unwrap().summary(), before);
    }

    #[test]
    fn editor_undo_depth_is_capped() {
        let mut ed = EditorState::new(glam::Vec3::ZERO);
        for _ in 0..(ED_UNDO_MAX + 20) {
            ed.apply(EditorAction::PosXInc);
        }
        assert_eq!(ed.undo.len(), ED_UNDO_MAX, "el tope es ED_UNDO_MAX");
    }

    /// **El test que pidió el usuario.** Clasifica cada `EditorAction` por
    /// comportamiento real, no por la lista de `is_view_only`: si una acción
    /// muta el documento y no empuja undo (o al revés), falla.
    /// Editor con todas las precondiciones puestas: clip abierto, al menos un
    /// estado que duplicar/borrar, dos entidades, y el historial limpio para que
    /// la prueba mida solo la acción que se aplica.
    fn ready_editor() -> EditorState {
        let mut ed = EditorState::new(glam::Vec3::new(0.0, 24.0, 0.0));
        ed.apply(EditorAction::ClipLoad);
        // El estado va DESPUÉS del Add: `Add` selecciona la entidad nueva, y es
        // esa la que necesita estados para que `StateDup` tenga algo que copiar.
        ed.apply(EditorAction::Add);
        ed.apply(EditorAction::StateAdd);
        assert!(ed.clip.is_some());
        assert_eq!(ed.state_len(), 1, "la entidad seleccionada tiene un estado");
        // Y con el estado en un sitio del que todos los steppers puedan mover:
        // su posición nace en [0,0,0] y `PosY` está acotado a >= 0, así que un
        // `PosYDec` sobre el estado por defecto no cambiaría nada.
        if let Some(s) = ed.selected_entity_mut().and_then(|e| e.state_mut(0)) {
            s.position = [4.0, 4.0, 4.0];
        }
        ed.undo.clear();
        ed.redo.clear();
        ed
    }

    /// **El test que pidió el usuario.** Para cada `EditorAction` comprueba las
    /// tres propiedades de la política de historia contra el comportamiento real
    /// (no contra la lista):
    ///
    /// 1. `Push` → exactamente una entrada empujada.
    /// 2. `None` → ni una entrada ni una entrada de redo.
    /// 3. `Reset` → ambas pilas vacías.
    /// 4. **Cobertura:** toda acción que cambia el documento está cubierta por
    ///    la historia (`Push` o `Reset`). Esta es la que caza un olvido.
    #[test]
    fn editor_history_policy_matches_every_action() {
        // Única acción que no puede ejercitarse aquí: `Load` necesita una escena
        // en disco y el repo no tiene ninguna en `saves/editor/`. Con escenas
        // cargadas sí cambia el documento (y por tanto está en `Reset`).
        let needs_saved_scene = [EditorAction::Load];
        for action in all_editor_actions() {
            if needs_saved_scene.contains(&action) {
                continue;
            }
            let mut ed = ready_editor();
            let before = (ed.scene.clone(), ed.clip.clone());
            let undo_before = ed.undo.len();
            let redo_before = ed.redo.len();

            ed.apply(action);

            let changed = (ed.scene.clone(), ed.clip.clone()) != before;
            let undo_delta = ed.undo.len() as i64 - undo_before as i64;
            let redo_delta = ed.redo.len() as i64 - redo_before as i64;
            let effect = EditorState::history_effect(action);
            match effect {
                HistoryEffect::Push => {
                    assert!(
                        undo_delta > 0 || changed,
                        "{action:?}: Push sin entrada ni cambio (undo {undo_delta})"
                    );
                    // Undo/Redo son los únicos que además mueven la otra pila.
                    if !matches!(action, EditorAction::Undo | EditorAction::Redo) {
                        assert_eq!(
                            undo_delta, 1,
                            "{action:?}: Push debe empujar exactamente una entrada"
                        );
                        assert_eq!(redo_delta, 0, "{action:?}: Push limpia el redo");
                    }
                }
                HistoryEffect::None => {
                    assert_eq!(undo_delta, 0, "{action:?}: None no empuja undo");
                    assert_eq!(redo_delta, 0, "{action:?}: None no toca el redo");
                }
                HistoryEffect::Reset => {
                    assert!(
                        ed.undo.is_empty() && ed.redo.is_empty(),
                        "{action:?}: Reset debe vaciar ambas pilas"
                    );
                }
            }
            // 4. Cobertura: si el documento cambió, la historia tiene que saber.
            if changed {
                assert_ne!(
                    effect,
                    HistoryEffect::None,
                    "{action:?} cambia el documento y no está cubierto por la historia"
                );
            }
        }
    }

    /// Guarda de compilación: este `match` no tiene `_`, así que **añadir una
    /// variante de `EditorAction` rompe la compilación de este test** hasta que
    /// se añada al recuento y al comportamiento esperado. Es la versión
    /// verificable de "no lo dejes solo en tu memoria".
    fn variant_count() -> usize {
        match EditorAction::ScrollPrev {
            EditorAction::ScrollPrev
            | EditorAction::ScrollNext
            | EditorAction::SelPrev
            | EditorAction::SelNext
            | EditorAction::KindPrev
            | EditorAction::KindNext
            | EditorAction::Add
            | EditorAction::Duplicate
            | EditorAction::Delete
            | EditorAction::Save
            | EditorAction::Load
            | EditorAction::Back
            | EditorAction::PosXDec
            | EditorAction::PosXInc
            | EditorAction::PosYDec
            | EditorAction::PosYInc
            | EditorAction::PosZDec
            | EditorAction::PosZInc
            | EditorAction::RotXDec
            | EditorAction::RotXInc
            | EditorAction::RotYDec
            | EditorAction::RotYInc
            | EditorAction::RotZDec
            | EditorAction::RotZInc
            | EditorAction::SclXDec
            | EditorAction::SclXInc
            | EditorAction::SclYDec
            | EditorAction::SclYInc
            | EditorAction::SclZDec
            | EditorAction::SclZInc
            | EditorAction::SkewDec
            | EditorAction::SkewInc
            | EditorAction::SizeDec
            | EditorAction::SizeInc
            | EditorAction::OpenPanel(_)
            | EditorAction::ClosePanel
            | EditorAction::StateSel(_)
            | EditorAction::StatePrev
            | EditorAction::StateNext
            | EditorAction::StateAdd
            | EditorAction::StateDup
            | EditorAction::StateDel
            | EditorAction::ClipLoad
            | EditorAction::ClipSave
            | EditorAction::Undo
            | EditorAction::Redo
            | EditorAction::KeyframeSel(_)
            | EditorAction::KeyframePrev
            | EditorAction::KeyframeNext
            | EditorAction::KeyframeTimeDec
            | EditorAction::KeyframeTimeInc
            | EditorAction::JointPrev
            | EditorAction::JointNext
            | EditorAction::EditEntity
            | EditorAction::EditJoint => 55,
        }
    }

    /// One instance of every `EditorAction`, for the classification test.
    fn all_editor_actions() -> Vec<EditorAction> {
        use EditorAction::*;
        let all = vec![
            ScrollPrev,
            ScrollNext,
            SelPrev,
            SelNext,
            KindPrev,
            KindNext,
            Add,
            Duplicate,
            Delete,
            Save,
            Load,
            Back,
            PosXDec,
            PosXInc,
            PosYDec,
            PosYInc,
            PosZDec,
            PosZInc,
            RotXDec,
            RotXInc,
            RotYDec,
            RotYInc,
            RotZDec,
            RotZInc,
            SclXDec,
            SclXInc,
            SclYDec,
            SclYInc,
            SclZDec,
            SclZInc,
            SkewDec,
            SkewInc,
            SizeDec,
            SizeInc,
            OpenPanel(EditorPanel::Transform),
            ClosePanel,
            StateSel(0),
            StatePrev,
            StateNext,
            StateAdd,
            StateDup,
            StateDel,
            ClipLoad,
            ClipSave,
            Undo,
            Redo,
            KeyframeSel(0),
            KeyframePrev,
            KeyframeNext,
            KeyframeTimeDec,
            KeyframeTimeInc,
            JointPrev,
            JointNext,
            EditEntity,
            EditJoint,
        ];
        assert_eq!(
            all.len(),
            variant_count(),
            "la lista de acciones no cubre todas las variantes"
        );
        all
    }

    /// **Bucle de refinamiento manual, verificado numéricamente.** La verificación
    /// *visual* de un cambio de articulación está bloqueada por falta de rig (la
    /// preview usa `for_each_face`, sin esqueleto), así que aquí se comprueba
    /// por la vía que el juego usaría: el clip editado, pasado por
    /// `AnimationClip::from_file` + `sample`, da la pose en radianes que
    /// corresponde a los grados escritos.
    #[test]
    fn refining_a_joint_of_a_keyframe_reaches_the_runtime_pose() {
        let mut ed = ready_editor();
        ed.apply(EditorAction::EditJoint);
        assert_eq!(ed.target, StepTarget::Joint);
        // La articulación 3 de `joint_names()` es `l_arm_x` (eje X).
        ed.apply(EditorAction::JointNext);
        ed.apply(EditorAction::JointNext);
        ed.apply(EditorAction::JointNext);
        assert_eq!(ed.joint_name(), Some("l_arm_x"));

        // El clip ya viene con l_arm_x escrito; lo que importa es el delta.
        let deg_before = ed
            .clip
            .as_ref()
            .unwrap()
            .file
            .frames[ed.kf_sel]
            .pose
            .get("l_arm_x")
            .copied()
            .expect("l_arm_x en el clip del repo");
        let before = ed.sample_pose().expect("pose inicial");

        ed.apply(EditorAction::RotXInc);
        // En el documento: grados, +15.
        let deg = ed
            .clip
            .as_ref()
            .unwrap()
            .file
            .frames[ed.kf_sel]
            .pose
            .get("l_arm_x")
            .copied()
            .expect("l_arm_x escrito");
        assert!(
            (deg - deg_before - ED_ROT_STEP).abs() < 1e-4,
            "grados: {deg_before} → {deg}"
        );
        // Y en la pose muestreada: radianes, misma magnitud.
        let after = ed.sample_pose().expect("pose tras el paso");
        let delta = (after.l_arm_x - before.l_arm_x).abs();
        assert!(
            (delta - ED_ROT_STEP.to_radians()).abs() < 1e-4,
            "radianes esperados {}, medidos {delta}",
            ED_ROT_STEP.to_radians()
        );

        // Un `Rot*` de otro eje NO tiene destino en una articulación de un eje.
        let snapshot = ed.clip.clone();
        ed.apply(EditorAction::RotYInc);
        ed.apply(EditorAction::RotZDec);
        assert_eq!(ed.clip, snapshot, "eje equivocado no debe tocar nada");
        // Ni pos/escala/tamaño/cizalla.
        ed.apply(EditorAction::PosXInc);
        ed.apply(EditorAction::SclYInc);
        ed.apply(EditorAction::SizeInc);
        ed.apply(EditorAction::SkewInc);
        assert_eq!(ed.clip, snapshot, "canales sin destino no deben tocar nada");
    }

    /// El refinamiento es deshacible: es la razón de que el undo includa el
    /// clip y no solo la escena.
    #[test]
    fn undo_reverts_a_joint_edit() {
        let mut ed = ready_editor();
        ed.apply(EditorAction::EditJoint);
        ed.apply(EditorAction::JointNext);
        ed.apply(EditorAction::JointNext);
        ed.apply(EditorAction::JointNext);
        let pristine = ed.clip.clone();
        ed.apply(EditorAction::RotXInc);
        ed.apply(EditorAction::RotXInc);
        assert_ne!(ed.clip, pristine);
        ed.apply(EditorAction::Undo);
        ed.apply(EditorAction::Undo);
        assert_eq!(ed.clip, pristine, "undo debe devolver el clip exacto");
    }

    #[test]
    fn keyframe_and_joint_cursors_wrap_and_clamp() {
        let mut ed = ready_editor();
        let n = ed.kf_len();
        assert!(n >= 2, "el clip del repo tiene varios keyframes");
        ed.apply(EditorAction::KeyframePrev);
        assert_eq!(ed.kf_sel, n - 1, "KeyframePrev envuelve");
        ed.apply(EditorAction::KeyframeNext);
        assert_eq!(ed.kf_sel, 0, "KeyframeNext envuelve");
        ed.apply(EditorAction::KeyframeSel(999));
        assert_eq!(ed.kf_sel, n - 1, "un índice fuera de rango se recorta");

        ed.apply(EditorAction::JointPrev);
        assert_eq!(ed.joint_sel, 11);
        ed.apply(EditorAction::JointNext);
        assert_eq!(ed.joint_sel, 0);

        // El tiempo del keyframe se mueve y está acotado a [0, duration].
        let dur = ed.clip.as_ref().unwrap().file.duration_s;
        let t0 = ed.clip.as_ref().unwrap().frames_time(ed.kf_sel);
        ed.apply(EditorAction::KeyframeTimeDec);
        let t1 = ed.clip.as_ref().unwrap().frames_time(ed.kf_sel);
        assert!((t1 - (t0 - ED_KEYFRAME_TIME_STEP).max(0.0)).abs() < 1e-4, "{t0} → {t1}");
        for _ in 0..(2000) {
            ed.apply(EditorAction::KeyframeTimeDec);
        }
        assert!(
            ed.clip.as_ref().unwrap().frames_time(ed.kf_sel) >= 0.0,
            "no baja de 0"
        );
        for _ in 0..(2000) {
            ed.apply(EditorAction::KeyframeTimeInc);
        }
        assert!(
            (ed.clip.as_ref().unwrap().frames_time(ed.kf_sel) - dur).abs() < 1e-3,
            "no pasa de duration_s ({dur})"
        );
    }

    /// Sin clip no hay timeline: los cursores no tienen dónde pointed y
    /// `ARTIC` se niega a activarse.
    #[test]
    fn animation_needs_a_clip() {
        let mut ed = EditorState::new(glam::Vec3::ZERO);
        assert!(ed.clip.is_none());
        assert_eq!(ed.kf_len(), 0);
        ed.apply(EditorAction::EditJoint);
        assert_eq!(ed.target, StepTarget::EntityOrState, "no se activa sin clip");
        assert!(ed.status.contains("no hay clip"), "{}", ed.status);
        ed.apply(EditorAction::KeyframeNext);
        assert_eq!(ed.kf_sel, 0);
        ed.apply(EditorAction::KeyframeTimeInc);
        assert!(ed.status.contains("no hay clip"), "{}", ed.status);
    }

    #[test]
    fn editor_markers_prefer_the_closest_entities() {
        let mut ed = EditorState::new(glam::Vec3::new(0.0, 24.0, 0.0));
        for _ in 0..3 {
            ed.apply(EditorAction::Add);
        }
        // Push them apart; the nearest ones must be the ones drawn.
        for (i, e) in ed.scene.entities.iter_mut().enumerate() {
            e.position = [i as f32 * 6.0, 24.0, 0.0];
            e.size = [1.0, 1.0, 1.0];
        }
        let near_camera = glam::Vec3::new(0.0, 24.0, 0.0);
        let cells = ed.marker_cells(near_camera, false);
        assert!(!cells.is_empty());
        // The selected entity is drawn white, the rest in their kind colour.
        assert!(cells.iter().any(|(_, c)| c == &[1.0, 1.0, 1.0]));
        assert!(cells.iter().any(|(_, c)| c != &[1.0, 1.0, 1.0]));
        // Far entities (past the marker range) are skipped entirely.
        for e in ed.scene.entities.iter_mut() {
            e.position[0] = 5000.0;
        }
        assert!(ed.marker_cells(near_camera, false).is_empty());
    }

    /// Con preview real, la entidad seleccionada deja de dibujar su jaula: el
    /// mesh es la señal. Las demás (sin mesh cargado) siguen dibujándose.
    #[test]
    fn editor_marker_cage_hides_the_selected_entity_with_preview() {
        let mut ed = EditorState::new(glam::Vec3::new(0.0, 24.0, 0.0));
        ed.apply(EditorAction::Add);
        for (i, e) in ed.scene.entities.iter_mut().enumerate() {
            e.position = [i as f32 * 6.0, 24.0, 0.0];
            e.size = [1.0, 1.0, 1.0];
        }
        let sel_x = ed.scene.entities[ed.selected].position[0];
        let other_x = ed
            .scene
            .entities
            .iter()
            .map(|e| e.position[0])
            .find(|x| *x != sel_x)
            .expect("la otra entidad");
        let cam = glam::Vec3::new(0.0, 24.0, 0.0);
        let is_white = |c: [f32; 3]| c == [1.0, 1.0, 1.0];
        // Sin preview: los dos marcadores, el seleccionado en blanco.
        let cage = ed.marker_cells(cam, false);
        assert!(cage.iter().any(|(cell, c)| cell.x == sel_x as i32 && is_white(*c)));
        assert!(cage.iter().any(|(cell, _)| cell.x == other_x as i32));
        // Con preview: solo queda el otro, y la jaula desaparece entera.
        let mesh = ed.marker_cells(cam, true);
        assert!(!mesh
            .iter()
            .any(|(cell, c)| cell.x == sel_x as i32 && is_white(*c)));
        assert!(mesh.iter().any(|(cell, _)| cell.x == other_x as i32));
        assert!(mesh.len() < cage.len());
    }

    /// La entidad por defecto del editor trae `model: "hero"`, así que la
    /// preview debe resolver sin tocar la escena. Y el pie/alto/yaw salen de la
    /// entidad, no de valores fijos.
    #[test]
    fn editor_preview_resolves_the_default_entity_model() {
        let mut ed = EditorState::new(glam::Vec3::new(3.0, 25.0, 7.0));
        let p = ed.preview().expect("la entidad por defecto tiene model: hero");
        assert!(!p.model.cells.is_empty());
        assert_eq!(p.feet, glam::Vec3::new(3.0, 25.0, 7.0));
        assert!((p.body_height - ed.selected_entity().unwrap().size[1]).abs() < 1e-5);
        // Sin estados no hay override: la preview usa la paleta del mesh.
        assert!(p.palette.is_none());
        // Sin selección no hay preview.
        ed.selected = usize::MAX;
        assert!(ed.preview().is_none());
        // Con un model que no existe, tampoco (y la jaula se queda).
        ed.selected = 0;
        ed.selected_entity_mut().unwrap().model = "no/existe.json".into();
        assert!(ed.preview().is_none());
    }

    /// El override de paleta del estado activo llega a la preview; un estado que
    /// hereda (paleta vacía) no lo hace.
    #[test]
    fn editor_preview_uses_the_active_state_palette() {
        use crate::entity_model::palette_by_name;
        let mut ed = EditorState::new(glam::Vec3::new(0.0, 24.0, 0.0));
        let e = ed.selected_entity_mut().expect("starter");
        e.model = "special1_sword".into();
        let st = |name: &str, palette: &str| editor::EditorEntityState {
            name: name.into(),
            palette: palette.into(),
            ..Default::default()
        };
        e.states = vec![st("normal", ""), st("gastada", "mono")];
        ed.state_sel = 0;
        assert!(ed.preview().expect("preview").palette.is_none(), "hereda");
        ed.state_sel = 1;
        let p = ed.preview().expect("preview");
        assert_eq!(*p.palette.expect("override"), palette_by_name("mono"));
        // Al borrar el estado que traía el override, se vuelve a heredar.
        ed.apply(EditorAction::StateDel);
        assert!(ed.preview().expect("preview").palette.is_none());
    }
}
