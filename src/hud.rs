//! On-screen stair / tunnel controls (mouse hits UI, not the world).
use crate::editor::{EditorEntity, TransformField};
use crate::stair_dig::{
    facing_screen_angle, PadDir, StairPhase, StairTool, TunnelFacing, TunnelIncline,
    STAIR_WIDTH_MAX,
};
use bytemuck::{Pod, Zeroable};
use image::{Rgba, RgbaImage};

pub const HUD_TILE: u32 = 32;
const ATLAS_COLS: u32 = 10;
const ATLAS_ROWS: u32 = 6;
/// Hotbar capacity (weapon / tool slots).
pub const HOTBAR_SLOTS: usize = 4;
/// Atlas row for inventory item icons (dirt…emerald).
pub(crate) const ITEM_ATLAS_ROW: u32 = 4;
/// Atlas row for stack-count digits 0–9.
const DIGIT_ATLAS_ROW: u32 = 5;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct HudVertex {
    /// Clip-space XY (−1..1).
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudAction {
    Facing(TunnelFacing),
    Incline(TunnelIncline),
    LenMinus,
    LenPlus,
    /// Toggle corridor width 1 ↔ 2.
    WidthToggle,
    Dig,
    Cancel,
    /// Select hotbar slot `0..HOTBAR_SLOTS`.
    SelectSlot(u8),
    /// Open / close the inventory panel.
    ToggleInventory,
    /// Click storage slot `0..26` in the inventory grid.
    InvSlot(u8),
    /// Full-screen catch while inventory is open (blocks world / other HUD).
    InventoryBackdrop,
    /// Beige panel body — absorb clicks without closing.
    InventoryPanel,
    /// Main-menu row: brand new game (ignores the autosave).
    MenuNewGame,
    /// Main-menu row: continue the saved game.
    MenuLoadGame,
    /// Main-menu row: open the voxel editor.
    MenuEditor,
    /// Main-menu backdrop — swallow clicks, do nothing.
    MenuBackdrop,
    /// Editor placeholder: back to the main menu.
    MenuBack,
    /// Native editor: one intent (select, transform step, file action).
    Ed(EditorAction),
}

/// What a native-editor button does. Every button is one discrete step so the
/// same panel works with a mouse, a finger or the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorAction {
    /// Move the entity-list window.
    ScrollPrev,
    ScrollNext,
    /// Pick another entity from the list.
    SelPrev,
    SelNext,
    /// Pick entity `i` by clicking it in the 3D scene. Emitted by the mouse
    /// handler, not by a button: the panel is hit-tested first, so a click that
    /// reaches this one landed on the scene.
    Pick(usize),
    /// Cycle the type tag of the selected entity.
    KindPrev,
    KindNext,
    /// File / scene actions.
    Add,
    Duplicate,
    Delete,
    Save,
    Load,
    Back,
    /// Transform steps: position ±0.5 block.
    PosXDec,
    PosXInc,
    PosYDec,
    PosYInc,
    PosZDec,
    PosZInc,
    /// Rotation ±15°.
    RotXDec,
    RotXInc,
    RotYDec,
    RotYInc,
    RotZDec,
    RotZInc,
    /// Scale ±10 %.
    SclXDec,
    SclXInc,
    SclYDec,
    SclYInc,
    SclZDec,
    SclZInc,
    /// Shear ±0.1.
    SkewDec,
    SkewInc,
    /// Local box size ±0.5 block (bigger marker = more voxels in game).
    SizeDec,
    SizeInc,
    /// Open one collapsible category panel (replaces "all controls at once").
    OpenPanel(EditorPanel),
    /// Close the open panel: back to just the entity list + the menu button.
    ClosePanel,
    /// Pick state `i` of the selected entity (`StateSel`, not a step).
    StateSel(usize),
    /// Cycle the active state (wraps; a no-op without states).
    StatePrev,
    StateNext,
    /// Add a state seeded with the entity's own mesh, duplicate it, or drop it.
    StateAdd,
    StateDup,
    StateDel,
    /// Import the next animation clip from `assets/animations/`, or export the
    /// open one to the editor's clip folder. The import cycles like `Load`
    /// does for scenes (there is no file picker in the HUD).
    ClipLoad,
    ClipSave,
    /// Write the selected entity's mesh to `assets/entities/<id>.json`.
    /// Exports the file, not the document, and never over an existing one.
    MeshSave,
    /// Undo / redo the last document change (Fase 6: blocking for refining
    /// keyframes by hand).
    Undo,
    Redo,
    /// Pick the active keyframe of the open clip (wraps), or move it in time.
    KeyframeSel(usize),
    KeyframePrev,
    KeyframeNext,
    /// `t` of the active keyframe, ±`KF_TIME_STEP` seconds.
    KeyframeTimeDec,
    KeyframeTimeInc,
    /// Pick the joint the steppers drive (wraps over the 12 canonical joints).
    JointPrev,
    JointNext,
    /// What the transform steppers drive: the entity/state, or one joint of the
    /// active keyframe. Set from the ANIMAR panel.
    EditEntity,
    EditJoint,
}

impl TransformField {
    /// The field a step action moves, with its direction (`-1.0` dec, `+1.0`
    /// inc), or `None` when the action is not a transform step.
    ///
    /// This is the only place that knows which button belongs to which field:
    /// the step itself — its size and its clamp — is the descriptor in
    /// `lib.rs`, so a press, a key and a drag cannot drift apart.
    pub fn of(action: EditorAction) -> Option<(Self, f32)> {
        use EditorAction as A;
        Some(match action {
            A::PosXDec => (Self::PosX, -1.0),
            A::PosXInc => (Self::PosX, 1.0),
            A::PosYDec => (Self::PosY, -1.0),
            A::PosYInc => (Self::PosY, 1.0),
            A::PosZDec => (Self::PosZ, -1.0),
            A::PosZInc => (Self::PosZ, 1.0),
            A::RotXDec => (Self::RotX, -1.0),
            A::RotXInc => (Self::RotX, 1.0),
            A::RotYDec => (Self::RotY, -1.0),
            A::RotYInc => (Self::RotY, 1.0),
            A::RotZDec => (Self::RotZ, -1.0),
            A::RotZInc => (Self::RotZ, 1.0),
            A::SclXDec => (Self::SclX, -1.0),
            A::SclXInc => (Self::SclX, 1.0),
            A::SclYDec => (Self::SclY, -1.0),
            A::SclYInc => (Self::SclY, 1.0),
            A::SclZDec => (Self::SclZ, -1.0),
            A::SclZInc => (Self::SclZ, 1.0),
            A::SkewDec => (Self::SkewZ, -1.0),
            A::SkewInc => (Self::SkewZ, 1.0),
            A::SizeDec => (Self::Size, -1.0),
            A::SizeInc => (Self::Size, 1.0),
            _ => return None,
        })
    }

    /// Name the bar prints, the same words as the readout on the right of the
    /// screen. ASCII only: `glyph5x7` has no accented glyphs.
    pub fn label(self) -> &'static str {
        match self {
            Self::PosX => "pos X",
            Self::PosY => "pos Y",
            Self::PosZ => "pos Z",
            Self::RotX => "rot X",
            Self::RotY => "rot Y",
            Self::RotZ => "rot Z",
            Self::SclX => "esc X",
            Self::SclY => "esc Y",
            Self::SclZ => "esc Z",
            Self::SkewZ => "zes",
            Self::Size => "caja",
        }
    }

    /// Decimals of the value a bar prints: a block with one, a degree with
    /// none, a scale or a shear with two. Same precision as the readout.
    pub fn decimals(self) -> usize {
        match self {
            Self::RotX | Self::RotY | Self::RotZ => 0,
            Self::SclX | Self::SclY | Self::SclZ | Self::SkewZ => 2,
            Self::PosX | Self::PosY | Self::PosZ | Self::Size => 1,
        }
    }
}

/// One bar of the transform panel: the field it drives, where it sits and what
/// it shows.
///
/// A bar is **not** a hit region. A bar is something you press and drag, and a
/// hit region can only carry a one-shot action, so the geometry lives here as
/// its own value instead: the panel draws from it and the mouse handler asks it
/// what is under the pointer, and both read the same rectangles.
#[derive(Clone, Copy, Debug)]
pub struct TransformBar {
    pub field: TransformField,
    pub rect: HudRect,
    /// Current value of the field, in its own unit.
    pub value: f32,
    /// Where the value sits inside the field's range, `0..=1`: how much of the
    /// bar is filled. `None` for a field with no range to sit in (a rotation
    /// wraps instead of clamping), which then shows its number alone.
    pub fill: Option<f32>,
}

/// Layout of the transform panel: its background and the bars on top of it.
///
/// One value because the background has to fit the bars exactly, and because
/// the mouse handler hit-tests the same bars the panel draws.
#[derive(Clone, Copy, Debug)]
pub struct TransformPanel {
    /// Background of the whole panel: the bars plus the row of buttons under
    /// them.
    pub rect: HudRect,
    /// One bar per field, in [`TransformField::ALL`] order.
    pub bars: [TransformBar; 11],
}

/// Geometry of the transform panel, centred at the bottom of the screen where
/// the transform controls have always been: two columns of bars filled left to
/// right and top to bottom, with a row of buttons under them. Recomputed per
/// frame because the panel follows the window, and it is eleven rectangles.
pub fn editor_transform_panel(logical_w: f32, logical_h: f32) -> TransformPanel {
    let s = hud_scale();
    let bar_w = 214.0 * s;
    let bar_h = 24.0 * s;
    let gap = 6.0 * s;
    let pad = 10.0 * s;
    let cols = 2;
    let rows = TransformField::ALL.len().div_ceil(cols);
    let w = pad * 2.0 + bar_w * cols as f32 + gap;
    // The rows of bars plus the row of buttons under them.
    let h = pad * 2.0 + (rows as f32 + 1.0) * bar_h + rows as f32 * gap;
    let x0 = (logical_w - w) * 0.5;
    let y0 = logical_h - h - gap;
    let bars = std::array::from_fn(|i| TransformBar {
        field: TransformField::ALL[i],
        rect: HudRect {
            x: x0 + pad + (i % cols) as f32 * (bar_w + gap),
            y: y0 + pad + (i / cols) as f32 * (bar_h + gap),
            w: bar_w,
            h: bar_h,
        },
        value: 0.0,
        fill: None,
    });
    TransformPanel {
        rect: HudRect { x: x0, y: y0, w, h },
        bars,
    }
}

/// One transform bar: the track, how much of it is filled, and the value.
///
/// The label sits on the left and the value in a column of its own, so the
/// numbers line up down the panel instead of following the length of each one.
fn push_transform_bar(
    mesh: &mut HudMesh,
    bar: TransformBar,
    logical_w: f32,
    logical_h: f32,
    track: [f32; 4],
    fill_rgba: [f32; 4],
) {
    let s = hud_scale();
    let px = 2.0 * s;
    let text_y = bar.rect.y + (bar.rect.h - 7.0 * px) * 0.5;
    push_panel(mesh, bar.rect, logical_w, logical_h, track);
    if let Some(f) = bar.fill {
        // The fill is the value's place in the field's range; a field that wraps
        // has none, so its bar is only the track and the number.
        let w = (bar.rect.w * f).max(2.0 * s);
        push_panel(
            mesh,
            HudRect {
                x: bar.rect.x,
                y: bar.rect.y,
                w,
                h: bar.rect.h,
            },
            logical_w,
            logical_h,
            fill_rgba,
        );
    }
    push_text(
        mesh,
        bar.rect.x + 6.0 * s,
        text_y,
        logical_w,
        logical_h,
        bar.field.label(),
        px,
        [0.70, 0.76, 0.84, 1.0],
    );
    let value = format!("{:.*}", bar.field.decimals(), bar.value);
    push_text(
        mesh,
        bar.rect.x + 96.0 * s,
        text_y,
        logical_w,
        logical_h,
        &value,
        px,
        [0.98, 0.86, 0.42, 1.0],
    );
}

/// How long the editor's subtitle stays fully visible, and how long it then
/// takes to fade out. Time, not frames, so it looks the same at 30 fps and at
/// 144.
const ED_SUBTITLE_HOLD_SECS: f32 = 1.6;
const ED_SUBTITLE_FADE_SECS: f32 = 1.2;

/// Alpha of a subtitle `age` seconds old: solid while it holds, then a straight
/// fade to nothing. Pure, so the curve is testable without a clock, and in
/// seconds rather than frames so it does not depend on the frame rate.
fn subtitle_alpha(age: f32) -> f32 {
    if age < ED_SUBTITLE_HOLD_SECS {
        return 1.0;
    }
    (1.0 - (age - ED_SUBTITLE_HOLD_SECS) / ED_SUBTITLE_FADE_SECS).clamp(0.0, 1.0)
}

/// Collapsible categories of the native editor. `None` (in `EditorState::panel`)
/// means no panel is open, so the screen only carries the entity selection and
/// the menu button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorPanel {
    Transform,
    Estado,
    Voxels,
    Animacion,
    Archivo,
}

impl EditorPanel {
    /// The five categories, in menu order.
    pub const ALL: [EditorPanel; 5] = [
        Self::Transform,
        Self::Estado,
        Self::Voxels,
        Self::Animacion,
        Self::Archivo,
    ];

    /// Button label. ASCII caps only: `glyph5x7` has no accented glyphs, so
    /// "ANIMAR" (not "ANIMACIÓN") and no `Ñ` in new labels.
    pub fn label(self) -> &'static str {
        match self {
            Self::Transform => "TRANSFORM",
            Self::Estado => "ESTADO",
            Self::Voxels => "VOXELS",
            Self::Animacion => "ANIMAR",
            Self::Archivo => "ARCHIVO",
        }
    }
}

/// What an inventory / hotbar slot holds (icons for now; gameplay hooks later).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotbarItem {
    Empty,
    Sword,
    Shield,
    Pickaxe,
    Axe,
    Consumable,
    Magic,
}

/// Columnas de la fila 3 del atlas (iconos del hotbar).
///
/// Fuente única de verdad: `HotbarItem::atlas_col` las consulta y
/// `build_hud_atlas` las hornea. Si añades una columna, toca solo aquí y el
/// test `every_hotbar_item_has_an_icon` lo detecta si te olvidas de hornearla.
mod hotbar_col {
    pub const SWORD: u32 = 0;
    pub const SHIELD: u32 = 1;
    pub const PICKAXE: u32 = 2;
    pub const AXE: u32 = 3;
    pub const CONSUMABLE: u32 = 4;
    pub const MAGIC: u32 = 5;
}

impl HotbarItem {
    /// Atlas column on row 3 (hotbar icons).
    fn atlas_col(self) -> Option<u32> {
        use hotbar_col::*;
        match self {
            Self::Empty => None,
            Self::Sword => Some(SWORD),
            Self::Shield => Some(SHIELD),
            Self::Pickaxe => Some(PICKAXE),
            Self::Axe => Some(AXE),
            Self::Consumable => Some(CONSUMABLE),
            Self::Magic => Some(MAGIC),
        }
    }

    /// Nombre para depuración / log.
    #[allow(dead_code)]
    fn label(self) -> &'static str {
        match self {
            Self::Empty => "vacio",
            Self::Sword => "espada",
            Self::Shield => "escudo",
            Self::Pickaxe => "pico",
            Self::Axe => "hacha",
            Self::Consumable => "consumible",
            Self::Magic => "magia",
        }
    }
}

/// Player hotbar: 4 slots + optional equipped slot (`None` = hands empty / stored).
#[derive(Clone, Debug)]
pub struct Hotbar {
    pub slots: [HotbarItem; HOTBAR_SLOTS],
    /// Currently drawn / held item. Press the same key again to store it.
    pub selected: Option<usize>,
    /// Frame counter when slot 1 was last modified with shift+clic.
    /// Used for the "cool-down" of weapon+shield combination.
    pub shift_cooldown: u64,
    /// Número de combinaciones arma+escudo realizadas este "session".
    pub combo_count: u32,
}

impl Default for Hotbar {
    fn default() -> Self {
        Self {
            slots: [
                HotbarItem::Sword,
                HotbarItem::Pickaxe,
                HotbarItem::Consumable,
                HotbarItem::Magic,
            ],
            // Start with pickaxe in hand (slot 1 → herramienta).
            selected: Some(1),
            shift_cooldown: 0,
            combo_count: 0,
        }
    }
}

impl Hotbar {
    /// Select slot, or unequip if that slot is already equipped.
    /// Para el slot 1, si se presiona con shift, intenta combinar/alternar
    /// el elemento secundario (escudo) en estilo Zelda TotK/BOTW.
    pub fn press_slot(&mut self, index: usize, shift: bool) {
        if index >= HOTBAR_SLOTS {
            return;
        }
        // Slot 1 combinación arma+escudo (solo si shift está activo).
        if index == 0 && shift {
            // Cool-down simple: evitar múltiples combinaciones rápidas.
            if self.frame_count() - self.shift_cooldown < 10 {
                // Demasiado pronto; ignorar.
            } else {
                self.combine_weapon_shield();
                self.shift_cooldown = self.frame_count();
                self.combo_count = self.combo_count.saturating_add(1);
                return;
            }
        }
        if self.slots[index] == HotbarItem::Empty {
            return;
        }
        if self.selected == Some(index) {
            self.selected = None;
        } else {
            self.selected = Some(index);
        }
    }

    fn frame_count(&self) -> u64 {
        // Placeholder: en producción esto vendría del renderer.frame_index.
        // Por ahora usamos 0 para que la lógica compile.
        0
    }

    fn combine_weapon_shield(&mut self) {
        // Lógica de combinación estilo Zelda: ciclar espada→escudo→espada→vacio...
        match self.selected_item() {
            HotbarItem::Sword => self.slots[0] = HotbarItem::Shield,
            HotbarItem::Shield => self.slots[0] = HotbarItem::Empty,
            HotbarItem::Empty => self.slots[0] = HotbarItem::Sword,
            _ => {} // consumible/magia: no afecta el ciclo weapon/shield
        }
    }

    pub fn select(&mut self, index: usize) {
        self.press_slot(index, false);
    }

    pub fn selected_item(&self) -> HotbarItem {
        self.selected
            .map(|i| self.slots[i.min(HOTBAR_SLOTS - 1)])
            .unwrap_or(HotbarItem::Empty)
    }

    pub fn holding_pickaxe(&self) -> bool {
        self.selected_item() == HotbarItem::Pickaxe
    }

    pub fn holding_sword(&self) -> bool {
        self.selected_item() == HotbarItem::Sword
    }

    /// Get the current weapon/shield state for slot 1.
    /// El slot 0 cicla Sword→Shield→Empty, así que nunca hay ambos a la vez.
    pub fn slot1_state(&self) -> &'static str {
        match self.selected_item() {
            HotbarItem::Sword => "espada",
            HotbarItem::Shield => "escudo",
            _ => "vacio",
        }
    }

    /// Equipped tool identity for mining / hero mesh (None = empty hands).
    pub fn tool_id(&self) -> Option<crate::items::ToolId> {
        crate::items::ToolId::from_hotbar_item(self.selected_item())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HudRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl HudRect {
    pub fn contains(self, px: f32, py: f32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

#[derive(Clone, Debug)]
pub struct HudHitRegion {
    pub action: HudAction,
    pub rect: HudRect,
}

#[derive(Clone, Default)]
pub struct HudMesh {
    pub vertices: Vec<HudVertex>,
    pub indices: Vec<u32>,
    pub hits: Vec<HudHitRegion>,
}

/// Pixel atlas for button icons (N/S/E/W, incline, +/−, dig, cancel).
pub fn build_hud_atlas() -> RgbaImage {
    let w = HUD_TILE * ATLAS_COLS;
    let h = HUD_TILE * ATLAS_ROWS;
    let mut img = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 0]));

    let ink = Rgba([245, 245, 250, 255]);
    let accent = Rgba([255, 210, 70, 255]);
    let dig = Rgba([90, 200, 110, 255]);
    let cancel = Rgba([220, 90, 90, 255]);
    let solid = Rgba([255, 255, 255, 255]);

    // Opaque white tile for tinted panels (col 7, row 0).
    {
        let (ox, oy) = tile_origin(7, 0);
        for y in 0..HUD_TILE {
            for x in 0..HUD_TILE {
                img.put_pixel(ox + x, oy + y, solid);
            }
        }
    }

    // Row 0: N S E W  Up Down Flat (blank)
    blit_arrow(&mut img, 0, 0, 0, -1, ink); // N
    blit_arrow(&mut img, 1, 0, 0, 1, ink); // S
    blit_arrow(&mut img, 2, 0, 1, 0, ink); // E
    blit_arrow(&mut img, 3, 0, -1, 0, ink); // W
    blit_incline(&mut img, 4, 0, true, accent); // up stair
    blit_incline(&mut img, 5, 0, false, accent); // down stair
    blit_flat(&mut img, 6, 0, ink); // flat

    // Row 1: − + DIG CANCEL
    blit_minus(&mut img, 0, 1, ink);
    blit_plus(&mut img, 1, 1, ink);
    blit_label(&mut img, 2, 1, b"DIG", dig);
    blit_label(&mut img, 4, 1, b"X", cancel);
    // Wide DIG uses tiles 2+3; X uses 4.

    // Row 2: status icons — heart full / empty, bottle empty / filled, INV
    blit_heart(&mut img, 0, 2, true);
    blit_heart(&mut img, 1, 2, false);
    blit_bottle(&mut img, 2, 2, false);
    blit_bottle(&mut img, 3, 2, true);
    blit_label(&mut img, 4, 2, b"INV", ink); // spans into col 5
    blit_boots(&mut img, 6, 2); // DepthBoots (la fila de items está llena)

    // Row 3: hotbar — snapshot of the real held mesh (same orient/grip/scale).
    // Las columnas salen de `hotbar_col` (fuente única de verdad).
    use hotbar_col as HC;
    blit_held_tool_snapshot(
        &mut img,
        HC::SWORD,
        3,
        crate::entity_model::sword_model(),
        &crate::items::spawn_special1_sword().def.attach,
        crate::hero::ToolIconBake::default(),
    );
    blit_shield(&mut img, HC::SHIELD, 3);
    blit_held_tool_snapshot(
        &mut img,
        HC::PICKAXE,
        3,
        crate::entity_model::pickaxe_model(),
        &crate::items::spawn_wooden_pickaxe().def.attach,
        crate::hero::ToolIconBake {
            roll_deg: 90.0,
            size_mul: 0.9,
        },
    );
    blit_axe(&mut img, HC::AXE, 3);
    blit_bottle(&mut img, HC::CONSUMABLE, 3, true);
    blit_magic(&mut img, HC::MAGIC, 3);

    // Row 4: inventory material icons
    blit_inv_block(&mut img, 0, ITEM_ATLAS_ROW, [132, 96, 60], false); // dirt
    blit_inv_block(&mut img, 1, ITEM_ATLAS_ROW, [168, 172, 180], false); // stone
    blit_inv_block(&mut img, 2, ITEM_ATLAS_ROW, [28, 24, 22], false); // coal
    blit_inv_block(&mut img, 3, ITEM_ATLAS_ROW, [56, 107, 235], true); // sapphire
    blit_inv_block(&mut img, 4, ITEM_ATLAS_ROW, [224, 46, 56], true); // ruby
    blit_inv_block(&mut img, 5, ITEM_ATLAS_ROW, [46, 198, 96], true); // emerald
    // Cardinal letters for the compass (N S E O).
    blit_label(&mut img, 6, ITEM_ATLAS_ROW, b"N", accent);
    blit_label(&mut img, 7, ITEM_ATLAS_ROW, b"S", ink);
    blit_label(&mut img, 8, ITEM_ATLAS_ROW, b"E", ink);
    blit_label(&mut img, 9, ITEM_ATLAS_ROW, b"O", ink);

    // Row 5: digits 0–9 for stack counts
    let digit_ink = Rgba([245, 245, 250, 255]);
    for d in 0u8..10 {
        let (ox, oy) = tile_origin(d as u32, DIGIT_ATLAS_ROW);
        blit_char5x7(&mut img, ox as i32 + 13, oy as i32 + 12, b'0' + d, digit_ink);
    }

    img
}

fn tile_origin(col: u32, row: u32) -> (u32, u32) {
    (col * HUD_TILE, row * HUD_TILE)
}

fn put(img: &mut RgbaImage, x: i32, y: i32, c: Rgba<u8>) {
    if x < 0 || y < 0 {
        return;
    }
    let (x, y) = (x as u32, y as u32);
    if x < img.width() && y < img.height() {
        img.put_pixel(x, y, c);
    }
}

fn blit_arrow(img: &mut RgbaImage, col: u32, row: u32, dx: i32, dy: i32, c: Rgba<u8>) {
    let (ox, oy) = tile_origin(col, row);
    let cx = ox as i32 + 16;
    let cy = oy as i32 + 16;
    // Shaft
    for i in -8..=8 {
        let x = cx + dx * i;
        let y = cy + dy * i;
        put(img, x, y, c);
        put(img, x + dy, y + dx, c); // thickness
        put(img, x - dy, y - dx, c);
    }
    // Head
    for t in 0..7 {
        let bx = cx + dx * 8;
        let by = cy + dy * 8;
        let px = bx - dx * (t / 2) + dy * (3 - t);
        let py = by - dy * (t / 2) - dx * (3 - t);
        put(img, px, py, c);
        let px2 = bx - dx * (t / 2) - dy * (3 - t);
        let py2 = by - dy * (t / 2) + dx * (3 - t);
        put(img, px2, py2, c);
    }
}

fn blit_incline(img: &mut RgbaImage, col: u32, row: u32, up: bool, c: Rgba<u8>) {
    let (ox, oy) = tile_origin(col, row);
    // Step silhouette
    for s in 0..4 {
        let x0 = ox as i32 + 6 + s * 5;
        let y_base = if up {
            oy as i32 + 22 - s * 4
        } else {
            oy as i32 + 10 + s * 4
        };
        for x in x0..x0 + 5 {
            for y in y_base..y_base + 3 {
                put(img, x, y, c);
            }
        }
    }
}

fn blit_flat(img: &mut RgbaImage, col: u32, row: u32, c: Rgba<u8>) {
    let (ox, oy) = tile_origin(col, row);
    for x in 6..26 {
        put(img, ox as i32 + x, oy as i32 + 15, c);
        put(img, ox as i32 + x, oy as i32 + 16, c);
    }
    // End caps
    for y in 12..20 {
        put(img, ox as i32 + 6, oy as i32 + y, c);
        put(img, ox as i32 + 25, oy as i32 + y, c);
    }
}

fn blit_minus(img: &mut RgbaImage, col: u32, row: u32, c: Rgba<u8>) {
    let (ox, oy) = tile_origin(col, row);
    for x in 8..24 {
        put(img, ox as i32 + x, oy as i32 + 15, c);
        put(img, ox as i32 + x, oy as i32 + 16, c);
    }
}

fn blit_plus(img: &mut RgbaImage, col: u32, row: u32, c: Rgba<u8>) {
    blit_minus(img, col, row, c);
    let (ox, oy) = tile_origin(col, row);
    for y in 8..24 {
        put(img, ox as i32 + 15, oy as i32 + y, c);
        put(img, ox as i32 + 16, oy as i32 + y, c);
    }
}

fn blit_label(img: &mut RgbaImage, col: u32, row: u32, text: &[u8], c: Rgba<u8>) {
    let (ox, oy) = tile_origin(col, row);
    let mut x = ox as i32 + 4;
    let y = oy as i32 + 12;
    for &ch in text {
        blit_char5x7(img, x, y, ch, c);
        x += 6;
    }
}

fn blit_heart(img: &mut RgbaImage, col: u32, row: u32, filled: bool) {
    let (ox, oy) = tile_origin(col, row);
    // Wider chunky heart (fills more of the 32px tile).
    // (left inset from tile center−half, run width) — half-span ≈ 14.
    const ROWS: [(i32, i32); 11] = [
        (4, 8),
        (2, 12),
        (1, 14),
        (1, 14),
        (1, 14),
        (2, 12),
        (3, 10),
        (4, 8),
        (5, 6),
        (6, 4),
        (7, 2),
    ];
    let body = if filled {
        Rgba([220, 48, 64, 255])
    } else {
        Rgba([50, 32, 38, 200])
    };
    let outline = if filled {
        Rgba([120, 20, 36, 255])
    } else {
        Rgba([170, 100, 110, 230])
    };
    let cx = ox as i32 + 16;
    let y0 = oy as i32 + 6;
    let half = 14;
    for (row_i, &(left, width)) in ROWS.iter().enumerate() {
        let y = y0 + row_i as i32 * 2;
        let x0 = cx - half + left;
        for x in x0..x0 + width {
            let edge = x == x0 || x + 1 == x0 + width || row_i == 0 || row_i + 1 == ROWS.len();
            let c = if edge { outline } else { body };
            put(img, x, y, c);
            put(img, x, y + 1, c);
        }
        // Gap in top lobes
        if row_i == 0 {
            for x in cx - 1..=cx {
                put(img, x, y, Rgba([0, 0, 0, 0]));
                put(img, x, y + 1, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn blit_bottle(img: &mut RgbaImage, col: u32, row: u32, filled: bool) {
    let (ox, oy) = tile_origin(col, row);
    let glass = Rgba([160, 210, 230, 230]);
    let glass_dark = Rgba([70, 120, 150, 255]);
    let liquid = Rgba([70, 140, 220, 230]);
    let cork = Rgba([140, 100, 55, 255]);
    let x0 = ox as i32 + 11;
    let y0 = oy as i32 + 4;
    // Cork / neck
    for y in 0..4 {
        for x in 4..8 {
            put(img, x0 + x, y0 + y, cork);
        }
    }
    for y in 4..8 {
        for x in 5..7 {
            put(img, x0 + x, y0 + y, glass_dark);
        }
    }
    // Body outline
    for y in 8..26 {
        let inset = if y < 10 { 3 } else if y > 23 { 2 } else { 1 };
        for x in inset..(12 - inset) {
            let edge = x == inset || x + 1 == 12 - inset || y == 8 || y == 25;
            let c = if edge {
                glass_dark
            } else if filled && y > 12 {
                liquid
            } else {
                glass
            };
            put(img, x0 + x, y0 + y, c);
        }
    }
    // Empty: glass shine + no liquid
    if !filled {
        for y in 12..22 {
            put(img, x0 + 3, y0 + y, Rgba([220, 240, 255, 160]));
        }
    }
}

/// Copy a held-tool snapshot (same mesh/orient as in-world) into an atlas tile.
fn blit_held_tool_snapshot(
    img: &mut RgbaImage,
    col: u32,
    row: u32,
    model: Option<&crate::entity_model::EntityModel>,
    attach: &crate::items::ToolAttach,
    icon: crate::hero::ToolIconBake,
) {
    let Some(model) = model else {
        return;
    };
    let (ox, oy) = tile_origin(col, row);
    let mut buf = vec![0u8; (HUD_TILE * HUD_TILE * 4) as usize];
    crate::hero::bake_held_tool_icon(model, attach, &mut buf, HUD_TILE, icon);
    for y in 0..HUD_TILE {
        for x in 0..HUD_TILE {
            let i = ((y * HUD_TILE + x) * 4) as usize;
            let a = buf[i + 3];
            if a == 0 {
                continue;
            }
            img.put_pixel(
                ox + x,
                oy + y,
                Rgba([buf[i], buf[i + 1], buf[i + 2], a]),
            );
        }
    }
}

fn blit_axe(img: &mut RgbaImage, col: u32, row: u32) {
    // Stub axe — no mesh asset yet; simple voxel silhouette.
    let (ox, oy) = tile_origin(col, row);
    let wood = Rgba([120, 75, 40, 255]);
    let blade = Rgba([170, 175, 185, 255]);
    let edge = Rgba([90, 95, 105, 255]);
    for y in 2..30 {
        put(img, ox as i32 + 11, oy as i32 + y, wood);
        put(img, ox as i32 + 12, oy as i32 + y, wood);
        put(img, ox as i32 + 13, oy as i32 + y, wood);
    }
    for row_i in 0..14 {
        let y = oy as i32 + 2 + row_i;
        let w = 5 + row_i / 2;
        for x in 13..(13 + w).min(31) {
            let c = if x + 1 == 13 + w { edge } else { blade };
            put(img, ox as i32 + x, y, c);
        }
    }
}

/// Botas de profundidad: dos botas voxel con suela de goma.
fn blit_boots(img: &mut RgbaImage, col: u32, row: u32) {
    let (ox, oy) = tile_origin(col, row);
    let leather = Rgba([126, 84, 48, 255]);
    let dark = Rgba([84, 54, 30, 255]);
    let sole = Rgba([40, 42, 48, 255]);
    let hi = Rgba([156, 112, 68, 255]);
    for (ox_off, wide) in [(6i32, 9i32), (17, 9)] {
        // Suela
        for y in 24..27 {
            for x in ox_off..(ox_off + wide) {
                put(img, ox as i32 + x, oy as i32 + y, sole);
            }
        }
        // Cuerpo (con puntera más ancha abajo)
        for y in 12..24 {
            let extra = if y >= 20 { 1 } else { 0 };
            for x in (ox_off + 1 - extra)..(ox_off + wide - 1 + extra) {
                let c = if y < 15 {
                    hi
                } else if x <= ox_off + 1 || x >= ox_off + wide - 2 {
                    dark
                } else {
                    leather
                };
                put(img, ox as i32 + x, oy as i32 + y, c);
            }
        }
        // Boca
        for x in ox_off + 1..(ox_off + wide - 1) {
            put(img, ox as i32 + x, oy as i32 + 11, dark);
        }
    }
}

/// Escudo: silueta voxel tipo Zelda (kite/heater), sin asset de malla todavía.
fn blit_shield(img: &mut RgbaImage, col: u32, row: u32) {
    let (ox, oy) = tile_origin(col, row);
    let wood = Rgba([120, 75, 40, 255]);
    let face = Rgba([70, 96, 150, 255]);
    let hi = Rgba([96, 128, 190, 255]);
    let edge = Rgba([28, 32, 48, 255]);
    // Silueta: se estrecha hacia abajo (kite).
    for y in 4..27 {
        let t = y - 4;
        // Ancho: 20 en el centro, cónico en punta abajo.
        let half = if t < 10 {
            10 - t / 2
        } else {
            10 - (t - 10) * 2
        }
        .max(1);
        let x0 = 16 - half;
        for x in x0..(16 + half) {
            let c = if x == x0 || x + 1 == 16 + half {
                edge
            } else if x >= 18 && x <= 20 && t < 12 {
                hi
            } else {
                face
            };
            put(img, ox as i32 + x, oy as i32 + y, c);
        }
    }
    // Empuñadura de madera en la parte superior.
    for x in 11..21 {
        put(img, ox as i32 + x, oy as i32 + 2, wood);
        put(img, ox as i32 + x, oy as i32 + 3, wood);
    }
}

/// Magia: gema flotante con destello (stub, sin asset todavía).
fn blit_magic(img: &mut RgbaImage, col: u32, row: u32) {
    let (ox, oy) = tile_origin(col, row);
    let core = Rgba([236, 240, 255, 255]);
    let glow = Rgba([150, 130, 240, 255]);
    let deep = Rgba([88, 66, 180, 255]);
    // Rombo centrado.
    for y in 6..26i32 {
        let dy = (y - 16).abs();
        let half = 9 - dy;
        if half <= 0 {
            continue;
        }
        for x in (16 - half)..(16 + half) {
            let c = if x <= 13 || x >= 19 {
                deep
            } else if x == 15 || x == 16 {
                core
            } else {
                glow
            };
            put(img, ox as i32 + x, oy as i32 + y, c);
        }
    }
    // Chispas alrededor.
    for (x, y) in [(7, 9), (24, 11), (8, 23), (23, 22)] {
        put(img, ox as i32 + x, oy as i32 + y, core);
    }
}

/// Minecraft-like inventory block icon (top + side faces).
fn blit_inv_block(img: &mut RgbaImage, col: u32, row: u32, rgb: [u8; 3], gem: bool) {
    let (ox, oy) = tile_origin(col, row);
    let top = Rgba([
        rgb[0].saturating_add(30),
        rgb[1].saturating_add(30),
        rgb[2].saturating_add(30),
        255,
    ]);
    let face = Rgba([rgb[0], rgb[1], rgb[2], 255]);
    let side = Rgba([
        rgb[0].saturating_sub(35),
        rgb[1].saturating_sub(35),
        rgb[2].saturating_sub(35),
        255,
    ]);
    let outline = Rgba([20, 20, 24, 255]);
    // Top diamond-ish face
    for y in 6..14 {
        let t = y - 6;
        let x0 = 10 - t / 2;
        let x1 = 22 + t / 2;
        for x in x0..=x1 {
            put(img, ox as i32 + x, oy as i32 + y, top);
        }
    }
    // Front face
    for y in 14..26 {
        for x in 8..20 {
            put(img, ox as i32 + x, oy as i32 + y, face);
        }
    }
    // Right side
    for y in 14..26 {
        for x in 20..24 {
            put(img, ox as i32 + x, oy as i32 + y, side);
        }
    }
    // Outline
    for x in 8..24 {
        put(img, ox as i32 + x, oy as i32 + 13, outline);
        put(img, ox as i32 + x, oy as i32 + 26, outline);
    }
    for y in 14..26 {
        put(img, ox as i32 + 8, oy as i32 + y, outline);
        put(img, ox as i32 + 23, oy as i32 + y, outline);
    }
    if gem {
        // Sparkle
        put(img, ox as i32 + 12, oy as i32 + 17, Rgba([255, 255, 255, 220]));
        put(img, ox as i32 + 13, oy as i32 + 18, Rgba([255, 255, 255, 180]));
    }
}

/// 5×7 bitmap for a glyph (bits 4..0 = columns left→right). Blank for unknown.
fn glyph5x7(ch: u8) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        b'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        b'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        b'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        b'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        b'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        b'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        b'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'I' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1F],
        b'J' => [0x01, 0x01, 0x01, 0x01, 0x11, 0x11, 0x0E],
        b'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        b'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        b'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        b'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        b'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        b'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        b'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        b'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        b'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        b'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x1B, 0x11],
        b'X' => [0x11, 0x0A, 0x04, 0x04, 0x04, 0x0A, 0x11],
        b'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        b'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        b'0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        b'1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        b'2' => [0x0E, 0x11, 0x01, 0x06, 0x08, 0x10, 0x1F],
        b'3' => [0x1E, 0x01, 0x01, 0x0E, 0x01, 0x01, 0x1E],
        b'4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        b'5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        b'6' => [0x0E, 0x10, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        b'7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        b'8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        b'9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x01, 0x0E],
        b'-' => [0x00, 0x00, 0x00, 0x0E, 0x00, 0x00, 0x00],
        b',' => [0x00, 0x00, 0x00, 0x00, 0x04, 0x04, 0x08],
        b'.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        _ => [0; 7],
    }
}

/// Tiny 5×7 caps / digits for HUD labels and stack counts.
fn blit_char5x7(img: &mut RgbaImage, x: i32, y: i32, ch: u8, c: Rgba<u8>) {
    for (row, bits) in glyph5x7(ch).iter().enumerate() {
        for col in 0..5 {
            if bits & (1 << (4 - col)) != 0 {
                put(img, x + col, y + row as i32, c);
            }
        }
    }
}

/// Width in logical px of `text` drawn with pixel size `px` (5×7 glyphs + 1 gap).
fn text_width(text: &str, px: f32) -> f32 {
    (text.len() as f32) * 6.0 * px
}

/// Draw a string as solid colored quads (one per lit glyph pixel). Any ASCII.
fn push_text(
    mesh: &mut HudMesh,
    x: f32,
    y: f32,
    logical_w: f32,
    logical_h: f32,
    text: &str,
    px: f32,
    color: [f32; 4],
) {
    let mut cx = x;
    for ch in text.bytes() {
        for (row, bits) in glyph5x7(ch).iter().enumerate() {
            for col in 0..5u32 {
                if bits & (1 << (4 - col)) != 0 {
                    push_panel(
                        mesh,
                        HudRect {
                            x: cx + col as f32 * px,
                            y: y + row as f32 * px,
                            w: px,
                            h: px,
                        },
                        logical_w,
                        logical_h,
                        color,
                    );
                }
            }
        }
        cx += 6.0 * px;
    }
}

/// Top-left location sign: realm name + nearest settlement (two text lines).
/// `opacity` 0 = invisible, 1 = fully solid (near a settlement).
pub fn build_realm_sign_hud(
    line1: &str,
    line2: &str,
    logical_w: f32,
    logical_h: f32,
    opacity: f32,
) -> HudMesh {
    let mut mesh = HudMesh::default();
    if line1.is_empty() && line2.is_empty() {
        return mesh;
    }
    let a = opacity.clamp(0.0, 1.0);
    if a < 0.02 {
        return mesh;
    }

    let px1 = 3.0; // title pixel size
    let px2 = 2.0; // subtitle pixel size
    let pad = 12.0;
    let line_gap = 8.0;
    let h1 = 7.0 * px1;
    let h2 = 7.0 * px2;
    let content_w = text_width(line1, px1).max(text_width(line2, px2));
    let panel = HudRect {
        x: 14.0,
        y: 14.0,
        w: content_w + pad * 2.0,
        h: h1 + line_gap + h2 + pad * 2.0,
    };
    push_panel(
        &mut mesh,
        panel,
        logical_w,
        logical_h,
        [0.05, 0.06, 0.09, 0.72 * a],
    );
    // Thin accent underline below the title.
    let rule = HudRect {
        x: panel.x + pad,
        y: panel.y + pad + h1 + line_gap * 0.5 - 1.0,
        w: content_w,
        h: 1.5,
    };
    push_panel(
        &mut mesh,
        rule,
        logical_w,
        logical_h,
        [0.95, 0.82, 0.25, 0.75 * a],
    );

    push_text(
        &mut mesh,
        panel.x + pad,
        panel.y + pad,
        logical_w,
        logical_h,
        line1,
        px1,
        [0.98, 0.90, 0.55, a],
    );
    push_text(
        &mut mesh,
        panel.x + pad,
        panel.y + pad + h1 + line_gap,
        logical_w,
        logical_h,
        line2,
        px2,
        [0.85, 0.90, 0.96, a],
    );

    mesh
}

/// Sign opacity from meters to the nearest settlement.
/// Solid within ~20 m, nearly gone by ~70 m.
pub fn settlement_sign_opacity(dist_m: Option<i32>) -> f32 {
    match dist_m {
        Some(d) => {
            let t = ((d as f32 - 20.0) / 50.0).clamp(0.0, 1.0);
            (1.0 - t * 0.92).clamp(0.08, 1.0)
        }
        None => 0.14,
    }
}

fn atlas_uv(col: u32, row: u32, cols_span: u32) -> ([f32; 2], [f32; 2]) {
    let tw = 1.0 / ATLAS_COLS as f32;
    let th = 1.0 / ATLAS_ROWS as f32;
    let u0 = col as f32 * tw;
    let v0 = row as f32 * th;
    let u1 = (col + cols_span) as f32 * tw;
    let v1 = (row + 1) as f32 * th;
    ([u0, v0], [u1, v1])
}

fn push_quad(
    mesh: &mut HudMesh,
    rect: HudRect,
    logical_w: f32,
    logical_h: f32,
    uv0: [f32; 2],
    uv1: [f32; 2],
    color: [f32; 4],
) {
    push_quad_rotated(mesh, rect, logical_w, logical_h, uv0, uv1, color, 0.0);
}

/// `angle` rotates the icon in UI space (radians, clockwise from screen-up).
fn push_quad_rotated(
    mesh: &mut HudMesh,
    rect: HudRect,
    logical_w: f32,
    logical_h: f32,
    uv0: [f32; 2],
    uv1: [f32; 2],
    color: [f32; 4],
    angle: f32,
) {
    let to_clip = |x: f32, y: f32| -> [f32; 2] {
        [
            (x / logical_w) * 2.0 - 1.0,
            1.0 - (y / logical_h) * 2.0, // y-down UI → y-up clip
        ]
    };
    let cx = rect.x + rect.w * 0.5;
    let cy = rect.y + rect.h * 0.5;
    let (s, c) = angle.sin_cos();
    let rot = |x: f32, y: f32| -> [f32; 2] {
        let lx = x - cx;
        let ly = y - cy;
        to_clip(cx + lx * c - ly * s, cy + lx * s + ly * c)
    };
    // Atlas arrow points screen-up (N tile); corners in CW order from top-left.
    let p0 = rot(rect.x, rect.y);
    let p1 = rot(rect.x + rect.w, rect.y);
    let p2 = rot(rect.x + rect.w, rect.y + rect.h);
    let p3 = rot(rect.x, rect.y + rect.h);
    let base = mesh.vertices.len() as u32;
    mesh.vertices.push(HudVertex {
        pos: p0,
        uv: [uv0[0], uv0[1]],
        color,
    });
    mesh.vertices.push(HudVertex {
        pos: p1,
        uv: [uv1[0], uv0[1]],
        color,
    });
    mesh.vertices.push(HudVertex {
        pos: p2,
        uv: [uv1[0], uv1[1]],
        color,
    });
    mesh.vertices.push(HudVertex {
        pos: p3,
        uv: [uv0[0], uv1[1]],
        color,
    });
    mesh.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn push_panel(mesh: &mut HudMesh, rect: HudRect, logical_w: f32, logical_h: f32, rgba: [f32; 4]) {
    // Solid panel: sample transparent corner of atlas, multiply by color.
    let (uv0, uv1) = atlas_uv(7, 0, 1);
    push_quad(mesh, rect, logical_w, logical_h, uv0, uv1, rgba);
}

/// Global HUD scale: Android shrinks overlay furniture so buttons cover less
/// world. Desktop is 1.0 so the PC HUD is pixel-identical to before.
pub fn hud_scale() -> f32 {
    if cfg!(target_os = "android") {
        0.8
    } else {
        1.0
    }
}

/// Solid quad with no hit region — used by the touch overlay, which routes its
/// input through `TouchControls` rects instead of `HudMesh::hits`.
pub fn push_solid_quad(
    mesh: &mut HudMesh,
    rect: HudRect,
    logical_w: f32,
    logical_h: f32,
    rgba: [f32; 4],
) {
    push_panel(mesh, rect, logical_w, logical_h, rgba);
}

fn push_icon_btn(
    mesh: &mut HudMesh,
    rect: HudRect,
    logical_w: f32,
    logical_h: f32,
    col: u32,
    row: u32,
    cols_span: u32,
    selected: bool,
    action: HudAction,
) {
    push_icon_btn_rotated(
        mesh, rect, logical_w, logical_h, col, row, cols_span, selected, action, 0.0,
    );
}

fn push_icon_btn_rotated(
    mesh: &mut HudMesh,
    rect: HudRect,
    logical_w: f32,
    logical_h: f32,
    col: u32,
    row: u32,
    cols_span: u32,
    selected: bool,
    action: HudAction,
    icon_angle: f32,
) {
    let bg = if selected {
        [0.95, 0.75, 0.20, 0.92]
    } else {
        [0.12, 0.14, 0.18, 0.82]
    };
    push_panel(mesh, rect, logical_w, logical_h, bg);
    let inset = 4.0;
    let icon = HudRect {
        x: rect.x + inset,
        y: rect.y + inset,
        w: rect.w - inset * 2.0,
        h: rect.h - inset * 2.0,
    };
    let (uv0, uv1) = atlas_uv(col, row, cols_span);
    push_quad_rotated(
        mesh,
        icon,
        logical_w,
        logical_h,
        uv0,
        uv1,
        [1.0, 1.0, 1.0, 1.0],
        icon_angle,
    );
    mesh.hits.push(HudHitRegion { action, rect });
}

/// Build the stair control panel (bottom-right). Empty when tool is idle.
///
/// D-pad is **camera-relative**: top = dig forward (into the scene). Arrow
/// icons rotate from the character/camera heading (diagonal when offset 45°).
pub fn build_stair_hud(
    stair: &StairTool,
    view_yaw: f32,
    logical_w: f32,
    logical_h: f32,
) -> HudMesh {
    let mut mesh = HudMesh::default();
    if stair.phase == StairPhase::Idle {
        return mesh;
    }

    let pad = 14.0;
    let btn = 48.0;
    let gap = 8.0;
    let panel_w = btn * 4.0 + gap * 3.0 + pad * 2.0;
    let panel_h = btn * 4.0 + gap * 3.0 + pad * 2.0 + 28.0;
    let panel = HudRect {
        x: logical_w - panel_w - 18.0,
        y: logical_h - panel_h - 18.0,
        w: panel_w,
        h: panel_h,
    };
    push_panel(&mut mesh, panel, logical_w, logical_h, [0.06, 0.07, 0.10, 0.78]);

    // Status strip
    let status = HudRect {
        x: panel.x + pad,
        y: panel.y + pad,
        w: panel.w - pad * 2.0,
        h: 22.0,
    };
    push_panel(&mut mesh, status, logical_w, logical_h, [0.18, 0.20, 0.26, 0.95]);

    let ox = panel.x + pad;
    let mut oy = panel.y + pad + 28.0;

    // Camera-relative pad:   forward (↑)
    //                      left · right
    //                          back
    let cx = ox + btn + gap;
    let pad_slots = [
        (PadDir::Up, cx, oy),
        (PadDir::Left, ox, oy + btn + gap),
        (PadDir::Right, ox + (btn + gap) * 2.0, oy + btn + gap),
        (PadDir::Down, cx, oy + (btn + gap) * 2.0),
    ];
    for &(dir, x, y) in &pad_slots {
        let facing = dir.to_facing(view_yaw);
        // Screen angle of this world facing; N atlas arrow points UI-up (angle 0).
        // facing_screen_angle: 0 = right, +π/2 = down → CW rotation from up.
        let screen = facing_screen_angle(facing, view_yaw);
        let icon_angle = screen + std::f32::consts::FRAC_PI_2;
        push_icon_btn_rotated(
            &mut mesh,
            HudRect {
                x,
                y,
                w: btn,
                h: btn,
            },
            logical_w,
            logical_h,
            0, // N arrow tile — rotated to the real heading
            0,
            1,
            stair.facing == facing,
            HudAction::Facing(facing),
            icon_angle,
        );
    }
    oy += (btn + gap) * 2.0;

    // Width 1↔2 in the empty top-left of the d-pad (selected = double-wide).
    push_icon_btn(
        &mut mesh,
        HudRect {
            x: ox,
            y: panel.y + pad + 28.0,
            w: btn,
            h: btn,
        },
        logical_w,
        logical_h,
        6, // corridor / flat tile
        0,
        1,
        stair.width >= STAIR_WIDTH_MAX,
        HudAction::WidthToggle,
    );

    // Incline + length column to the right of the pad
    let rx = ox + (btn + gap) * 3.0;
    let mut ry = panel.y + pad + 28.0;
    push_icon_btn(
        &mut mesh,
        HudRect {
            x: rx,
            y: ry,
            w: btn,
            h: btn,
        },
        logical_w,
        logical_h,
        4,
        0,
        1,
        stair.incline == TunnelIncline::Up,
        HudAction::Incline(TunnelIncline::Up),
    );
    ry += btn + gap;
    push_icon_btn(
        &mut mesh,
        HudRect {
            x: rx,
            y: ry,
            w: btn,
            h: btn,
        },
        logical_w,
        logical_h,
        5,
        0,
        1,
        stair.incline == TunnelIncline::Down,
        HudAction::Incline(TunnelIncline::Down),
    );
    ry += btn + gap;
    push_icon_btn(
        &mut mesh,
        HudRect {
            x: rx,
            y: ry,
            w: btn,
            h: btn,
        },
        logical_w,
        logical_h,
        6,
        0,
        1,
        stair.incline == TunnelIncline::Flat,
        HudAction::Incline(TunnelIncline::Flat),
    );

    // Length − / + under cardinal S
    let ly = oy + btn + gap;
    push_icon_btn(
        &mut mesh,
        HudRect {
            x: ox,
            y: ly,
            w: btn,
            h: btn,
        },
        logical_w,
        logical_h,
        0,
        1,
        1,
        false,
        HudAction::LenMinus,
    );
    push_icon_btn(
        &mut mesh,
        HudRect {
            x: ox + btn + gap,
            y: ly,
            w: btn,
            h: btn,
        },
        logical_w,
        logical_h,
        1,
        1,
        1,
        false,
        HudAction::LenPlus,
    );

    // DIG (wide) + Cancel
    let dig = HudRect {
        x: ox + (btn + gap) * 2.0,
        y: ly,
        w: btn * 1.5 + gap * 0.5,
        h: btn,
    };
    push_icon_btn(
        &mut mesh,
        dig,
        logical_w,
        logical_h,
        2,
        1,
        2,
        stair.phase == StairPhase::Digging,
        HudAction::Dig,
    );
    let cancel = HudRect {
        x: rx,
        y: ly,
        w: btn,
        h: btn,
    };
    push_icon_btn(
        &mut mesh,
        cancel,
        logical_w,
        logical_h,
        4,
        1,
        1,
        false,
        HudAction::Cancel,
    );

    mesh
}

pub fn hit_test(hits: &[HudHitRegion], px: f32, py: f32) -> Option<HudAction> {
    // Topmost last — iterate reversed so later widgets win.
    hits.iter().rev().find(|h| h.rect.contains(px, py)).map(|h| h.action)
}

/// Append `src` geometry into `dst` (index rebase + hit regions).
pub fn append_hud(dst: &mut HudMesh, src: &HudMesh) {
    let base = dst.vertices.len() as u32;
    dst.vertices.extend_from_slice(&src.vertices);
    dst.indices
        .extend(src.indices.iter().map(|i| i + base));
    dst.hits.extend_from_slice(&src.hits);
}

/// Top-right compass: heading-up rose.
/// Top of the dial = where the character looks; N/S/E/O stay locked to world north
/// (player position does not move north — only facing rotates the rose).
pub fn build_compass_hud(facing_yaw: f32, logical_w: f32, logical_h: f32) -> HudMesh {
    use crate::stair_dig::{facing_screen_angle, TunnelFacing};

    let mut mesh = HudMesh::default();
    let facing = TunnelFacing::from_yaw(facing_yaw);
    let pad = 12.0;
    let size = 110.0;
    let panel = HudRect {
        x: logical_w - size - pad,
        y: pad,
        w: size,
        h: size,
    };
    push_panel(
        &mut mesh,
        panel,
        logical_w,
        logical_h,
        [0.05, 0.06, 0.09, 0.78],
    );

    let cx = panel.x + panel.w * 0.5;
    let cy = panel.y + panel.h * 0.5;
    let letter = 26.0;
    let radius = 34.0;

    // Fixed "mira" mark at top of dial (character forward).
    {
        let tip = HudRect {
            x: cx - 8.0,
            y: panel.y + 6.0,
            w: 16.0,
            h: 14.0,
        };
        let (uv0, uv1) = atlas_uv(0, 0, 1); // N arrow points screen-up
        push_quad(
            &mut mesh,
            tip,
            logical_w,
            logical_h,
            uv0,
            uv1,
            [0.95, 0.82, 0.25, 1.0],
        );
    }

    // Center hub.
    let hub = HudRect {
        x: cx - 14.0,
        y: cy - 14.0,
        w: 28.0,
        h: 28.0,
    };
    push_panel(
        &mut mesh,
        hub,
        logical_w,
        logical_h,
        [0.16, 0.17, 0.20, 0.96],
    );
    let hub_col = match facing {
        TunnelFacing::North => 6,
        TunnelFacing::South => 7,
        TunnelFacing::East => 8,
        TunnelFacing::West => 9,
    };
    let (uv0, uv1) = atlas_uv(hub_col, ITEM_ATLAS_ROW, 1);
    push_quad(
        &mut mesh,
        HudRect {
            x: hub.x + 3.0,
            y: hub.y + 3.0,
            w: hub.w - 6.0,
            h: hub.h - 6.0,
        },
        logical_w,
        logical_h,
        uv0,
        uv1,
        [0.95, 0.82, 0.25, 1.0],
    );

    // World cardinals orbit so N always faces world north relative to heading.
    // `facing_screen_angle`: 0 = screen-right, +π/2 = screen-down; top = character forward.
    let cards: [(TunnelFacing, u32); 4] = [
        (TunnelFacing::North, 6),
        (TunnelFacing::East, 8),
        (TunnelFacing::South, 7),
        (TunnelFacing::West, 9),
    ];
    for &(card, col) in &cards {
        let ang = facing_screen_angle(card, facing_yaw);
        let x = cx + radius * ang.cos() - letter * 0.5;
        let y = cy + radius * ang.sin() - letter * 0.5;
        let rect = HudRect {
            x,
            y,
            w: letter,
            h: letter,
        };
        let on = card == facing;
        push_panel(
            &mut mesh,
            rect,
            logical_w,
            logical_h,
            if on {
                [0.95, 0.82, 0.25, 0.95]
            } else {
                [0.12, 0.13, 0.16, 0.90]
            },
        );
        let (uv0, uv1) = atlas_uv(col, ITEM_ATLAS_ROW, 1);
        let tint = if card == TunnelFacing::North {
            [0.95, 0.32, 0.32, 1.0] // N always readable / true north
        } else if on {
            [1.0, 0.92, 0.40, 1.0]
        } else {
            [0.85, 0.88, 0.92, 1.0]
        };
        push_quad(
            &mut mesh,
            HudRect {
                x: rect.x + 2.0,
                y: rect.y + 2.0,
                w: rect.w - 4.0,
                h: rect.h - 4.0,
            },
            logical_w,
            logical_h,
            uv0,
            uv1,
            tint,
        );
    }

    mesh
}

/// Shared bottom-left stack metrics (status → hotbar → inventory button).
fn status_stack_h(pad: f32) -> f32 {
    let heart_h = 34.0;
    let bottle = 40.0;
    let row_gap = 8.0;
    heart_h + bottle + row_gap + pad * 2.0
}

/// Bottom-left hotbar: long inventory button above 4 weapon/tool slots.
pub fn build_hotbar_hud(
    hotbar: &Hotbar,
    inventory_open: bool,
    logical_w: f32,
    logical_h: f32,
) -> HudMesh {
    let mut mesh = HudMesh::default();
    let slot = 72.0;
    let gap = 8.0;
    let pad = 14.0;
    let inv_h = 34.0;
    let inv_gap = 8.0;
    let n = HOTBAR_SLOTS as f32;
    let row_w = n * slot + (n - 1.0) * gap;
    let panel_w = row_w + pad * 2.0;
    let slots_h = slot + pad * 2.0 + 6.0; // room for selected lift
    let panel_h = slots_h + inv_h + inv_gap;
    // Sit above the hearts/bottles plate.
    let status_h = status_stack_h(pad);
    let panel = HudRect {
        x: pad,
        y: logical_h - pad - status_h - 8.0 - panel_h,
        w: panel_w,
        h: panel_h,
    };
    push_panel(
        &mut mesh,
        panel,
        logical_w,
        logical_h,
        [0.05, 0.06, 0.09, 0.62],
    );

    // Long inventory toggle — full width of the object HUD.
    let inv = HudRect {
        x: panel.x + pad,
        y: panel.y + pad,
        w: row_w,
        h: inv_h,
    };
    let inv_bg = if inventory_open {
        [0.95, 0.82, 0.25, 0.98]
    } else {
        [0.28, 0.32, 0.40, 0.96]
    };
    push_panel(&mut mesh, inv, logical_w, logical_h, inv_bg);
    let inv_well = HudRect {
        x: inv.x + 3.0,
        y: inv.y + 3.0,
        w: inv.w - 6.0,
        h: inv.h - 6.0,
    };
    push_panel(
        &mut mesh,
        inv_well,
        logical_w,
        logical_h,
        [0.12, 0.14, 0.18, 0.94],
    );
    let label_w = 64.0;
    let label = HudRect {
        x: inv_well.x + (inv_well.w - label_w) * 0.5,
        y: inv_well.y + 2.0,
        w: label_w,
        h: inv_well.h - 4.0,
    };
    let (uv0, uv1) = atlas_uv(4, 2, 2);
    push_quad(
        &mut mesh,
        label,
        logical_w,
        logical_h,
        uv0,
        uv1,
        [1.0, 1.0, 1.0, 1.0],
    );
    mesh.hits.push(HudHitRegion {
        action: HudAction::ToggleInventory,
        rect: inv,
    });

    let ox = panel.x + pad;
    let oy = panel.y + pad + inv_h + inv_gap + 4.0;
    // Pass 1: slot plates. Pass 2: tool icons (may overhang the plate).
    let mut slot_rects = [(HudRect::default(), false); HOTBAR_SLOTS];
    for i in 0..HOTBAR_SLOTS {
        let selected = hotbar.selected == Some(i);
        let lift = if selected { -4.0 } else { 0.0 };
        let rect = HudRect {
            x: ox + i as f32 * (slot + gap),
            y: oy + lift,
            w: slot,
            h: slot,
        };
        slot_rects[i] = (rect, selected);
        let rim = if selected {
            [0.95, 0.82, 0.25, 0.98]
        } else {
            [0.22, 0.24, 0.28, 0.95]
        };
        push_panel(&mut mesh, rect, logical_w, logical_h, rim);
        let inset = if selected { 4.0 } else { 3.0 };
        let well = HudRect {
            x: rect.x + inset,
            y: rect.y + inset,
            w: rect.w - inset * 2.0,
            h: rect.h - inset * 2.0,
        };
        push_panel(
            &mut mesh,
            well,
            logical_w,
            logical_h,
            if selected {
                [0.14, 0.15, 0.20, 0.96]
            } else {
                [0.10, 0.11, 0.14, 0.92]
            },
        );
        mesh.hits.push(HudHitRegion {
            action: HudAction::SelectSlot(i as u8),
            rect,
        });
    }
    for i in 0..HOTBAR_SLOTS {
        let Some(col) = hotbar.slots[i].atlas_col() else {
            continue;
        };
        let (rect, selected) = slot_rects[i];
        // Near full slot size; selected slightly larger — OK to overflow neighbors.
        let scale = if selected { 1.42 } else { 1.28 };
        let iw = slot * scale;
        let ih = slot * scale;
        let icon = HudRect {
            x: rect.x + (slot - iw) * 0.5,
            y: rect.y + (slot - ih) * 0.5 - 2.0,
            w: iw,
            h: ih,
        };
        let (uv0, uv1) = atlas_uv(col, 3, 1);
        push_quad(
            &mut mesh,
            icon,
            logical_w,
            logical_h,
            uv0,
            uv1,
            [1.0, 1.0, 1.0, 1.0],
        );
    }

    mesh
}

/// Bottom-left status: hearts (life) + empty/filled bottles (magic).
/// Pantalla de carga: anillo de progreso + porcentaje, con fundido de salida.
///
/// `pct` 0..1 = progreso de carga. `fade` 1 = opaco, 0 = ya no se dibuja
/// (el mundo queda revelado con el anillo cerrado).
pub fn build_loading_hud(pct: f32, logical_w: f32, logical_h: f32, fade: f32) -> HudMesh {
    let mut mesh = HudMesh::default();
    let fade = fade.clamp(0.0, 1.0);
    if fade <= 0.01 {
        return mesh;
    }
    let pct = pct.clamp(0.0, 1.0);

    const SEGMENTS: u32 = 24;
    let filled = (pct * SEGMENTS as f32).round() as u32;
    let seg_angle = std::f32::consts::TAU / SEGMENTS as f32;

    // Velo a pantalla completa.
    push_panel(
        &mut mesh,
        HudRect {
            x: 0.0,
            y: 0.0,
            w: logical_w,
            h: logical_h,
        },
        logical_w,
        logical_h,
        [0.02, 0.03, 0.05, 0.72 * fade],
    );

    // Anillo: 24 segmentos, se rellenan en sentido horario desde las 12.
    let cx = logical_w * 0.5;
    let cy = logical_h * 0.5;
    let radius = (logical_w.min(logical_h) * 0.16).max(48.0);
    let thickness = radius * 0.18;
    let arc = (std::f32::consts::TAU / SEGMENTS as f32 * radius * 0.82).max(2.0);
    for i in 0..SEGMENTS {
        // Ángulo de las 12 en sentido horario (y hacia abajo ⇒ suma).
        let a = -std::f32::consts::FRAC_PI_2 + seg_angle * i as f32;
        let (s, c) = a.sin_cos();
        // El rect local tiene h radial; se rota para que apunte al radio.
        let quad = HudRect {
            x: cx + c * radius - arc * 0.5,
            y: cy + s * radius - thickness * 0.5,
            w: arc,
            h: thickness,
        };
        let on = i < filled;
        let alpha = if on { 0.95 * fade } else { 0.22 * fade };
        let color = if on {
            [0.95, 0.82, 0.45, alpha]
        } else {
            [0.75, 0.78, 0.85, alpha]
        };
        push_quad_rotated(
            &mut mesh,
            quad,
            logical_w,
            logical_h,
            [0.0, 0.0],
            [1.0, 1.0],
            color,
            a + std::f32::consts::FRAC_PI_2,
        );
    }

    // Porcentaje bajo el anillo.
    let label = format!("{}%", (pct * 100.0).round() as i32);
    let px = 4.0;
    let tw = text_width(&label, px);
    push_text(
        &mut mesh,
        cx - tw * 0.5,
        cy + radius + 18.0,
        logical_w,
        logical_h,
        &label,
        px,
        [0.92, 0.92, 0.95, 0.9 * fade],
    );
    mesh
}

/// Mira de primera persona: cruz centrada, sin hit regions.
pub fn build_crosshair_hud(logical_w: f32, logical_h: f32) -> HudMesh {
    let mut mesh = HudMesh::default();
    let cx = logical_w * 0.5;
    let cy = logical_h * 0.5;
    let arm = 7.0;
    let thick = 1.6;
    let gap = 4.0;
    let color = [1.0, 1.0, 1.0, 0.85];
    let rects = [
        HudRect {
            x: cx - arm * 0.5,
            y: cy - gap - thick,
            w: arm,
            h: thick,
        },
        HudRect {
            x: cx - arm * 0.5,
            y: cy + gap,
            w: arm,
            h: thick,
        },
        HudRect {
            x: cx - gap - thick,
            y: cy - arm * 0.5,
            w: thick,
            h: arm,
        },
        HudRect {
            x: cx + gap,
            y: cy - arm * 0.5,
            w: thick,
            h: arm,
        },
    ];
    for r in rects {
        push_panel(&mut mesh, r, logical_w, logical_h, color);
    }
    mesh
}

pub fn build_status_hud(
    hearts: u32,
    max_hearts: u32,
    bottles_filled: u32,
    max_bottles: u32,
    logical_w: f32,
    logical_h: f32,
) -> HudMesh {
    let mut mesh = HudMesh::default();
    // Wide hearts packed tight; bottles keep their square icons.
    let heart_w = 52.0;
    let heart_h = 34.0;
    let heart_gap = 2.0;
    let bottle = 40.0;
    let bottle_gap = 2.0;
    let row_gap = 8.0;
    let pad = 14.0;
    let heart_row_w =
        max_hearts as f32 * heart_w + (max_hearts.saturating_sub(1) as f32) * heart_gap;
    let bottle_row_w =
        max_bottles as f32 * bottle + (max_bottles.saturating_sub(1) as f32) * bottle_gap;
    let panel_w = heart_row_w.max(bottle_row_w) + pad * 2.0;
    let panel_h = heart_h + bottle + row_gap + pad * 2.0;
    let panel = HudRect {
        x: pad,
        y: logical_h - panel_h - pad,
        w: panel_w,
        h: panel_h,
    };
    // Semi-transparent dark plate behind the icons.
    push_panel(
        &mut mesh,
        panel,
        logical_w,
        logical_h,
        [0.04, 0.05, 0.08, 0.42],
    );

    let ox = panel.x + pad;
    let heart_y = panel.y + pad;
    for i in 0..max_hearts {
        let filled = i < hearts;
        let col = if filled { 0 } else { 1 };
        let rect = HudRect {
            x: ox + i as f32 * (heart_w + heart_gap),
            y: heart_y,
            w: heart_w,
            h: heart_h,
        };
        let (uv0, uv1) = atlas_uv(col, 2, 1);
        push_quad(
            &mut mesh,
            rect,
            logical_w,
            logical_h,
            uv0,
            uv1,
            [1.0, 1.0, 1.0, 1.0],
        );
    }

    let bottle_y = heart_y + heart_h + row_gap;
    for i in 0..max_bottles {
        let filled = i < bottles_filled;
        let col = if filled { 3 } else { 2 };
        let rect = HudRect {
            x: ox + i as f32 * (bottle + bottle_gap),
            y: bottle_y,
            w: bottle,
            h: bottle,
        };
        let (uv0, uv1) = atlas_uv(col, 2, 1);
        push_quad(
            &mut mesh,
            rect,
            logical_w,
            logical_h,
            uv0,
            uv1,
            [1.0, 1.0, 1.0, 1.0],
        );
    }

    mesh
}

fn push_mc_slot(
    mesh: &mut HudMesh,
    rect: HudRect,
    logical_w: f32,
    logical_h: f32,
    selected: bool,
) {
    let rim = if selected {
        [0.95, 0.82, 0.25, 0.98]
    } else {
        [0.22, 0.22, 0.22, 1.0]
    };
    push_panel(mesh, rect, logical_w, logical_h, rim);
    let well = HudRect {
        x: rect.x + 2.0,
        y: rect.y + 2.0,
        w: rect.w - 4.0,
        h: rect.h - 4.0,
    };
    push_panel(
        mesh,
        well,
        logical_w,
        logical_h,
        [0.545, 0.545, 0.545, 1.0], // #8B8B8B
    );
}

fn push_stack_icon(
    mesh: &mut HudMesh,
    rect: HudRect,
    stack: crate::inventory::ItemStack,
    logical_w: f32,
    logical_h: f32,
) {
    use crate::inventory::InvItem;
    let icon = HudRect {
        x: rect.x + 4.0,
        y: rect.y + 2.0,
        w: rect.w - 8.0,
        h: rect.h - 10.0,
    };
    let (col, row) = stack.kind.atlas_tile();
    let (uv0, uv1) = atlas_uv(col, row, 1);
    let tint = stack.kind.tint_rgb();
    let mul = if stack.kind.is_cube() {
        [1.0, 1.0, 1.0, 1.0]
    } else if matches!(
        stack.kind,
        InvItem::CoalMicro
            | InvItem::SapphireMicro
            | InvItem::RubyMicro
            | InvItem::EmeraldMicro
    ) {
        // Micros render slightly smaller via inset.
        push_quad(
            mesh,
            HudRect {
                x: icon.x + 3.0,
                y: icon.y + 3.0,
                w: icon.w - 6.0,
                h: icon.h - 6.0,
            },
            logical_w,
            logical_h,
            uv0,
            uv1,
            [tint[0], tint[1], tint[2], 1.0],
        );
        push_stack_count(mesh, rect, stack.count, logical_w, logical_h);
        return;
    } else {
        [tint[0], tint[1], tint[2], 1.0]
    };
    push_quad(mesh, icon, logical_w, logical_h, uv0, uv1, mul);
    if stack.kind.is_cube() {
        // Gold corner mark for crafted cubes.
        let mark = HudRect {
            x: rect.x + rect.w - 10.0,
            y: rect.y + 3.0,
            w: 6.0,
            h: 6.0,
        };
        push_panel(mesh, mark, logical_w, logical_h, [0.95, 0.82, 0.25, 1.0]);
    }
    push_stack_count(mesh, rect, stack.count, logical_w, logical_h);
}

fn push_stack_count(
    mesh: &mut HudMesh,
    slot: HudRect,
    count: u32,
    logical_w: f32,
    logical_h: f32,
) {
    if count <= 1 {
        return;
    }
    let text = count.min(999).to_string();
    let digit_w = 10.0;
    let digit_h = 14.0;
    let total_w = text.len() as f32 * digit_w;
    let mut x = slot.x + slot.w - 3.0 - total_w;
    let y = slot.y + slot.h - digit_h - 2.0;
    for ch in text.bytes() {
        let d = (ch - b'0') as u32;
        let (uv0, uv1) = atlas_uv(d, DIGIT_ATLAS_ROW, 1);
        push_quad(
            mesh,
            HudRect {
                x,
                y,
                w: digit_w,
                h: digit_h,
            },
            logical_w,
            logical_h,
            uv0,
            uv1,
            [1.0, 1.0, 1.0, 1.0],
        );
        x += digit_w;
    }
}

/// Rows of the main menu, in display order. Kept next to the builder so the
/// keyboard index and the hit regions can never drift apart.
pub const MENU_ROWS: [(&str, HudAction); 3] = [
    ("PARTIDA NUEVA", HudAction::MenuNewGame),
    ("PARTIDA GUARDADA", HudAction::MenuLoadGame),
    ("MODO EDITOR", HudAction::MenuEditor),
];

/// First-screen selector: new game / saved game / editor.
///
/// `selected` is the keyboard-highlighted row (wrapped by the caller). The
/// backdrop swallows every click so the world never reacts behind the menu.
pub fn build_main_menu_hud(
    selected: usize,
    has_save: bool,
    logical_w: f32,
    logical_h: f32,
) -> HudMesh {
    let mut mesh = HudMesh::default();
    let screen = HudRect {
        x: 0.0,
        y: 0.0,
        w: logical_w,
        h: logical_h,
    };
    push_panel(
        &mut mesh,
        screen,
        logical_w,
        logical_h,
        [0.03, 0.04, 0.07, 0.88],
    );
    mesh.hits.push(HudHitRegion {
        action: HudAction::MenuBackdrop,
        rect: screen,
    });

    let s = hud_scale();
    let row_h = 46.0 * s;
    let row_gap = 10.0 * s;
    let row_w = 300.0 * s;
    let block_h = MENU_ROWS.len() as f32 * row_h + (MENU_ROWS.len() as f32 - 1.0) * row_gap;
    // Title sits above the rows; the whole block is vertically centered.
    let title_h = 30.0 * s;
    let top = (logical_h - (title_h + 26.0 * s + block_h)) * 0.5;

    push_text(
        &mut mesh,
        (logical_w - text_width("MICROVERSE", 5.0 * s)) * 0.5,
        top,
        logical_w,
        logical_h,
        "MICROVERSE",
        5.0 * s,
        [0.98, 0.86, 0.42, 1.0],
    );

    let first_y = top + title_h + 26.0 * s;
    for (i, (label, action)) in MENU_ROWS.iter().enumerate() {
        let row = HudRect {
            x: (logical_w - row_w) * 0.5,
            y: first_y + i as f32 * (row_h + row_gap),
            w: row_w,
            h: row_h,
        };
        let is_sel = i == selected;
        // "Partida guardada" stays readable but dimmed when there is no save.
        let dim = *label == "PARTIDA GUARDADA" && !has_save;
        let bg = if is_sel {
            [0.16, 0.30, 0.46, 1.0]
        } else {
            [0.10, 0.12, 0.17, 0.92]
        };
        push_panel(&mut mesh, row, logical_w, logical_h, bg);
        // Accent bar on the left of the highlighted row.
        if is_sel {
            push_panel(
                &mut mesh,
                HudRect {
                    x: row.x,
                    y: row.y,
                    w: 5.0 * s,
                    h: row.h,
                },
                logical_w,
                logical_h,
                [0.98, 0.86, 0.42, 1.0],
            );
        }
        let text_px = 3.0 * s;
        let tx = row.x + 20.0 * s;
        let ty = row.y + (row_h - 7.0 * text_px) * 0.5;
        let color = if dim {
            [0.58, 0.60, 0.66, 1.0]
        } else if is_sel {
            [1.0, 1.0, 1.0, 1.0]
        } else {
            [0.84, 0.86, 0.92, 1.0]
        };
        push_text(&mut mesh, tx, ty, logical_w, logical_h, label, text_px, color);
        mesh.hits.push(HudHitRegion {
            action: *action,
            rect: row,
        });
    }
    mesh
}

/// Filas de la lista de entidades que caben en el panel del editor.
pub const EDITOR_VISIBLE_ROWS: usize = 5;

/// Botón de texto centrado: fondo + etiqueta + hit region.
fn push_text_btn(
    mesh: &mut HudMesh,
    rect: HudRect,
    logical_w: f32,
    logical_h: f32,
    label: &str,
    px: f32,
    bg: [f32; 4],
    action: HudAction,
) {
    push_panel(mesh, rect, logical_w, logical_h, bg);
    push_text(
        mesh,
        rect.x + (rect.w - text_width(label, px)) * 0.5,
        rect.y + (rect.h - 7.0 * px) * 0.5,
        logical_w,
        logical_h,
        label,
        px,
        [0.92, 0.94, 0.98, 1.0],
    );
    mesh.hits.push(HudHitRegion { action, rect });
}

/// The editor's view state, grouped so the panel builder does not grow one
/// parameter per selection level (`docs/plan_fase6.md` §3.3). Four levels
/// deep now: entity → state → keyframe → joint.
#[derive(Clone, Copy, Debug, Default)]
pub struct EditorView {
    /// Selected entity (`usize::MAX` = none).
    pub selected: usize,
    /// First visible row of the entity list.
    pub scroll: usize,
    /// Active state of the selected entity.
    pub state_sel: usize,
    /// Active keyframe of the open clip.
    pub kf_sel: usize,
    /// Active joint, index into `animation::joint_names()`.
    pub joint_sel: usize,
    /// True when the transform steppers drive the joint instead of the entity.
    pub editing_joint: bool,
    /// Collapsible category open (`None` = menu closed).
    pub panel: Option<EditorPanel>,
}

/// The four transform lines of the right-hand readout (`pos`, `rot`, `esc`,
/// `zes`), as they get printed.
///
/// Split out of [`build_editor_hud`] so the rule can be pinned: `HudMesh`
/// rasterises the glyphs on the spot and keeps no strings, so nothing else
/// can tell afterwards what the readout said.
///
/// All four channels come from `driven_transform`, the same object the step
/// buttons drive — the active state when the entity has states, the entity
/// itself otherwise. Reading them off `entity` outright made the readout and
/// the bars disagree as soon as an entity had states: the bars edit the state
/// and the readout printed the entity.
///
/// `caja` stays on the entity on purpose: it is the marker's box, not part of
/// the mesh, and the bars have no equivalent for it.
fn transform_readout(entity: &EditorEntity, state_idx: usize) -> [String; 4] {
    let t = entity.driven_transform(state_idx);
    [
        format!("pos {:.1} {:.1} {:.1}", t.position[0], t.position[1], t.position[2]),
        format!("rot {:.0} {:.0} {:.0}", t.rotation[0], t.rotation[1], t.rotation[2]),
        format!("esc {:.2} {:.2} {:.2}", t.scale[0], t.scale[1], t.scale[2]),
        format!("zes {:.2} · caja {:.1}", t.skew[2], entity.size[0]),
    ]
}

/// Native editor panel: entity list (left), selection readout (right) and the
/// menu button (bottom-left). Everything else lives inside a collapsible
/// category panel, drawn only when `view.panel` is `Some` — with `None` the
/// screen carries just the selection and the menu.
///
/// `entities` is the live scene, `view` the cursors and open panel, and `clip`
/// the animation clip open in the editor. `bars` is what each transform bar
/// shows, already read by the editor, so the panel only draws it. Only the
/// buttons are hit regions, so the world stays visible and tappable behind the
/// panel.
pub fn build_editor_hud(
    scene_name: &str,
    status: &str,
    // Transient line under the top strip, and how many seconds ago it was
    // written: the panel fades it out by time.
    subtitle: &str,
    subtitle_age: f32,
    entities: &[crate::editor::EditorEntity],
    view: EditorView,
    // Animation clip open in the editor (`None` = none imported yet).
    clip: Option<&crate::editor_clip::EditorClip>,
    // Value of each transform bar, in [`TransformField::ALL`] order.
    bars: &[TransformBar],
    logical_w: f32,
    logical_h: f32,
) -> HudMesh {
    let (selected, scroll, state_sel, kf_sel, joint_sel) = (
        view.selected, view.scroll, view.state_sel, view.kf_sel, view.joint_sel,
    );
    let panel = view.panel;
    let mut mesh = HudMesh::default();
    let s = hud_scale();
    // Ancho mínimo para la etiqueta más larga del panel ("tipo+" = 5 glifos
    // = 60 px con `text_px` 2.0). Con botones de 40 px las etiquetas se
    // solapaban.
    let btn = 68.0 * s;
    let gap = 6.0 * s;
    let pad = 10.0 * s;
    let text_px = 2.0 * s;
    let dim_bg = [0.08, 0.09, 0.13, 0.86];
    let btn_bg = [0.16, 0.19, 0.26, 0.95];
    let accent = [0.20, 0.38, 0.55, 0.98];

    // ── Top strip: scene name + last message ──────────────────────────────
    let top = HudRect {
        x: gap,
        y: gap,
        w: logical_w - gap * 2.0,
        // Alto para que el título (px 2.2) y el mensaje (px 1.8) no se
        // pisen: 4 + 15 + 3 + 12 + 2.
        h: 38.0 * s,
    };
    push_panel(&mut mesh, top, logical_w, logical_h, dim_bg);
    push_text(
        &mut mesh,
        top.x + pad,
        top.y + 4.0 * s,
        logical_w,
        logical_h,
        &format!("EDITOR · {scene_name}"),
        2.2 * s,
        [0.98, 0.86, 0.42, 1.0],
    );
    if !status.is_empty() {
        push_text(
            &mut mesh,
            top.x + pad,
            top.y + 23.0 * s,
            logical_w,
            logical_h,
            status,
            1.8 * s,
            [0.78, 0.82, 0.88, 1.0],
        );
    }

    // Subtítulo: su propia línea bajo la tira, con su propio fondo, y un
    // fundido por tiempo. No es `status` — esa línea es la memoria del editor y
    // no se va sola; esto cuenta lo que acaba de pasar y se desvanece.
    if !subtitle.is_empty() {
        let a = subtitle_alpha(subtitle_age);
        if a > 0.02 {
            let px = 2.0 * s;
            let tw = 6.0 * px * subtitle.bytes().count() as f32;
            let box_ = HudRect {
                x: (logical_w - tw) * 0.5 - pad * 0.5,
                y: top.y + top.h + gap * 0.5,
                w: tw + pad,
                h: 7.0 * px + pad * 0.5,
            };
            push_panel(
                &mut mesh,
                box_,
                logical_w,
                logical_h,
                [0.08, 0.09, 0.13, 0.86 * a],
            );
            push_text(
                &mut mesh,
                box_.x + pad * 0.5,
                box_.y + pad * 0.25,
                logical_w,
                logical_h,
                subtitle,
                px,
                [0.98, 0.86, 0.42, a],
            );
        }
    }

    // ── Left: entity list ─────────────────────────────────────────────────
    let list_w = 150.0 * s;
    let rows = EDITOR_VISIBLE_ROWS.min(entities.len().max(1));
    let list = HudRect {
        x: gap,
        y: top.y + top.h + gap,
        w: list_w,
        h: pad * 2.0 + 16.0 * s + rows as f32 * (btn + gap),
    };
    push_panel(&mut mesh, list, logical_w, logical_h, dim_bg);
    let mut oy = list.y + pad;
    push_text(
        &mut mesh,
        list.x + pad,
        oy,
        logical_w,
        logical_h,
        &format!("ENTIDADES ({})", entities.len()),
        text_px,
        [0.70, 0.76, 0.84, 1.0],
    );
    oy += 16.0 * s;
    for (i, entity) in entities
        .iter()
        .enumerate()
        .skip(scroll)
        .take(EDITOR_VISIBLE_ROWS)
    {
        let row = HudRect {
            x: list.x + pad,
            y: oy,
            w: list.w - pad * 2.0,
            h: btn,
        };
        let is_sel = i == selected;
        let bg = if is_sel { accent } else { btn_bg };
        push_panel(&mut mesh, row, logical_w, logical_h, bg);
        // Type tag colour chip + "label (kind)".
        let c = entity.kind_color();
        push_panel(
            &mut mesh,
            HudRect {
                x: row.x,
                y: row.y,
                w: 4.0 * s,
                h: row.h,
            },
            logical_w,
            logical_h,
            [c[0], c[1], c[2], 1.0],
        );
        let label = format!("{} {}", i + 1, entity.label());
        push_text(
            &mut mesh,
            row.x + 10.0 * s,
            row.y + (row.h - 7.0 * text_px) * 0.5,
            logical_w,
            logical_h,
            &label,
            text_px,
            [0.92, 0.94, 0.98, 1.0],
        );
        mesh.hits.push(HudHitRegion {
            action: HudAction::Ed(EditorAction::SelNext),
            rect: row,
        });
        oy += btn + gap;
    }
    // Scroll window (only when the list does not fit).
    if entities.len() > EDITOR_VISIBLE_ROWS {
        let sb = 22.0 * s;
        for (i, (label, act)) in [("◀", EditorAction::ScrollPrev), ("▶", EditorAction::ScrollNext)]
            .into_iter()
            .enumerate()
        {
            push_text_btn(
                &mut mesh,
                HudRect {
                    x: list.x + pad + i as f32 * (sb + gap),
                    y: list.y + list.h - sb - pad,
                    w: sb,
                    h: sb,
                },
                logical_w,
                logical_h,
                label,
                text_px,
                btn_bg,
                HudAction::Ed(act),
            );
        }
    }

    // ── Right: transform readout of the selection ─────────────────────────
    if let Some(entity) = entities.get(selected) {
        let state_idx = entity.clamp_state(state_sel);
        // "POS 8.5 25.2 24.0" son 15 glifos: a px 1.8 son 162 px, de ahí el
        // ancho (el panel anterior lo cortaba por la derecha).
        let info_w = 250.0 * s;
        let info = HudRect {
            x: logical_w - info_w - gap,
            y: top.y + top.h + gap,
            w: info_w,
            h: 104.0 * s,
        };
        push_panel(&mut mesh, info, logical_w, logical_h, dim_bg);
        let c = entity.kind_color();
        push_panel(
            &mut mesh,
            HudRect {
                x: info.x,
                y: info.y,
                w: 4.0 * s,
                h: info.h,
            },
            logical_w,
            logical_h,
            [c[0], c[1], c[2], 1.0],
        );
        let px = 1.6 * s;
        let mut iy = info.y + 8.0 * s;
        let line = |mesh: &mut HudMesh, txt: String, iy: &mut f32| {
            push_text(
                mesh,
                info.x + 10.0 * s,
                *iy,
                logical_w,
                logical_h,
                &txt,
                px,
                [0.88, 0.90, 0.95, 1.0],
            );
            *iy += 12.0 * s;
        };
        line(
            &mut mesh,
            format!("{} · {}", entity.label(), entity.kind),
            &mut iy,
        );
        line(
            &mut mesh,
            format!("model {}", entity.effective_model(state_idx)),
            &mut iy,
        );
        for text in transform_readout(entity, state_idx) {
            line(&mut mesh, text, &mut iy);
        }
        // Which object the step buttons drive right now: the active state when
        // the entity owns states, the entity itself otherwise.
        line(
            &mut mesh,
            if entity.has_states() {
                format!("editando estado {}/{}", state_idx + 1, entity.states.len())
            } else {
                "editando entidad".to_string()
            },
            &mut iy,
        );
    }

    // ── Bottom-left: the menu button (the only always-on control) ─────────
    let row_h = 24.0 * s;
    let open = panel.is_some();
    let menu = HudRect {
        x: gap,
        y: logical_h - row_h - gap,
        w: 100.0 * s,
        h: row_h,
    };
    push_text_btn(
        &mut mesh,
        menu,
        logical_w,
        logical_h,
        if open { "CERRAR" } else { "MENU" },
        text_px,
        if open { accent } else { btn_bg },
        // Closed → open the most-used category. Open → close everything.
        if open {
            HudAction::Ed(EditorAction::ClosePanel)
        } else {
            HudAction::Ed(EditorAction::OpenPanel(EditorPanel::Transform))
        },
    );

    // ── Category column + panel content (only while a panel is open) ──────
    if let Some(current) = panel {
        let cat_w = 110.0 * s;
        // px 1.8 so the longest label ("TRANSFORM", 9 glyphs) fits in cat_w.
        let cat_px = 1.8 * s;
        let col_bottom = menu.y - gap;
        for (i, cat) in EditorPanel::ALL.iter().enumerate() {
            let rect = HudRect {
                x: gap,
                y: col_bottom - (EditorPanel::ALL.len() - i) as f32 * (row_h + gap),
                w: cat_w,
                h: row_h,
            };
            let bg = if *cat == current { accent } else { btn_bg };
            push_text_btn(
                &mut mesh,
                rect,
                logical_w,
                logical_h,
                cat.label(),
                cat_px,
                bg,
                HudAction::Ed(EditorAction::OpenPanel(*cat)),
            );
        }

        match current {
            // Barras, no botones: el transform se ajusta apretando y arrastrando
            // cada campo (ver `editor_transform_panel`, la misma geometría que
            // pregunta el ratón). Debajo solo queda lo que no es transform: la
            // selección de entidad y el tipo.
            EditorPanel::Transform => {
                let layout = editor_transform_panel(logical_w, logical_h);
                push_panel(&mut mesh, layout.rect, logical_w, logical_h, dim_bg);
                for bar in bars {
                    push_transform_bar(&mut mesh, *bar, logical_w, logical_h, btn_bg, accent);
                }
                // Fila de botones: cuatro repartidos en el ancho del panel,
                // bajo las barras.
                let row_y = layout.bars[TransformField::ALL.len() - 1].rect.y + gap
                    + layout.bars[0].rect.h;
                let inner = layout.rect.w - pad * 2.0;
                let fw = (inner - gap * 3.0) * 0.25;
                let picks: [(&str, EditorAction); 4] = [
                    ("ant", EditorAction::SelPrev),
                    ("sig", EditorAction::SelNext),
                    ("tipo-", EditorAction::KindPrev),
                    ("tipo+", EditorAction::KindNext),
                ];
                for (i, (label, act)) in picks.iter().enumerate() {
                    push_text_btn(
                        &mut mesh,
                        HudRect {
                            x: layout.rect.x + pad + i as f32 * (fw + gap),
                            y: row_y,
                            w: fw,
                            h: layout.bars[0].rect.h,
                        },
                        logical_w,
                        logical_h,
                        label,
                        text_px,
                        btn_bg,
                        HudAction::Ed(*act),
                    );
                }
            }
            EditorPanel::Archivo => {
                let fw = 100.0 * s;
                let w = fw * 4.0 + gap * 3.0 + pad * 2.0;
                let h = pad * 2.0 + 16.0 * s + row_h * 3.0 + gap * 2.0;
                let bar = HudRect {
                    x: (logical_w - w) * 0.5,
                    y: logical_h - h - gap,
                    w,
                    h,
                };
                push_panel(&mut mesh, bar, logical_w, logical_h, dim_bg);
                push_text(
                    &mut mesh,
                    bar.x + pad,
                    bar.y + pad,
                    logical_w,
                    logical_h,
                    EditorPanel::Archivo.label(),
                    text_px,
                    [0.98, 0.86, 0.42, 1.0],
                );
                // Fila 1: clips (import/export) y salida. Fila 2: escena.
                // Fila 3: malla e historial (también en Ctrl+Z / Ctrl+Shift+Z).
                let file_actions: [(&str, EditorAction); 11] = [
                    ("IMPORTAR", EditorAction::ClipLoad),
                    ("EXPORTAR", EditorAction::ClipSave),
                    ("VOLVER", EditorAction::Back),
                    ("+AÑADIR", EditorAction::Add),
                    ("COPIAR", EditorAction::Duplicate),
                    ("BORRAR", EditorAction::Delete),
                    ("GUARDAR", EditorAction::Save),
                    ("CARGAR", EditorAction::Load),
                    ("MALLA", EditorAction::MeshSave),
                    ("DESHACER", EditorAction::Undo),
                    ("REHACER", EditorAction::Redo),
                ];
                for (i, (label, act)) in file_actions.iter().enumerate() {
                    let bg = match *act {
                        EditorAction::Back => [0.30, 0.16, 0.18, 0.95],
                        EditorAction::Save | EditorAction::ClipSave => [0.16, 0.34, 0.24, 0.95],
                        // Exportar la malla es lo mismo que exportar el clip:
                        // un fichero nuevo en disco, verde de "escribe".
                        EditorAction::MeshSave => [0.16, 0.34, 0.24, 0.95],
                        EditorAction::ClipLoad => [0.24, 0.22, 0.36, 0.95],
                        EditorAction::Undo => [0.30, 0.26, 0.14, 0.95],
                        EditorAction::Redo => [0.26, 0.20, 0.30, 0.95],
                        _ => btn_bg,
                    };
                    push_text_btn(
                        &mut mesh,
                        HudRect {
                            x: bar.x + pad + (i % 4) as f32 * (fw + gap),
                            y: bar.y + pad + 16.0 * s + (i / 4) as f32 * (row_h + gap),
                            w: fw,
                            h: row_h,
                        },
                        logical_w,
                        logical_h,
                        label,
                        text_px,
                        bg,
                        HudAction::Ed(*act),
                    );
                }
            }
            // Per-state mesh/palette/transform (schema v2). The rows are the
            // states of the selected entity; the readout is the active one.
            EditorPanel::Estado => {
                let entity = entities.get(selected);
                let states = entity.map(|e| e.states.as_slice()).unwrap_or(&[]);
                let idx = entity.map(|e| e.clamp_state(state_sel)).unwrap_or(0);
                let shown = states.len().min(EDITOR_VISIBLE_ROWS);
                let fw = 100.0 * s;
                let readout = 3.0 * 12.0 * s;
                let w = fw * 3.0 + gap * 2.0 + pad * 2.0;
                let h = pad * 2.0 + 16.0 * s + readout
                    + shown as f32 * (row_h + gap)
                    + (row_h + gap) * 2.0;
                let bar = HudRect {
                    x: (logical_w - w) * 0.5,
                    y: logical_h - h - gap,
                    w,
                    h,
                };
                push_panel(&mut mesh, bar, logical_w, logical_h, dim_bg);
                let px = 1.6 * s;
                push_text(
                    &mut mesh,
                    bar.x + pad,
                    bar.y + pad,
                    logical_w,
                    logical_h,
                    &format!("ESTADOS ({})", states.len()),
                    text_px,
                    [0.98, 0.86, 0.42, 1.0],
                );
                // Reserved readout block: name / mesh / palette of the active
                // state (or the hint when the entity has none).
                let mut ty = bar.y + pad + 16.0 * s;
                if states.is_empty() {
                    push_text(
                        &mut mesh,
                        bar.x + pad,
                        ty,
                        logical_w,
                        logical_h,
                        "SIN ESTADOS: USA +NUEVO",
                        px,
                        [0.78, 0.82, 0.88, 1.0],
                    );
                } else {
                    let st = &states[idx];
                    for row in [
                        format!("{}/{} · {}", idx + 1, states.len(), st.label()),
                        format!(
                            "mesh {}",
                            if st.model.is_empty() {
                                "hereda entidad"
                            } else {
                                &st.model
                            }
                        ),
                        format!(
                            "paleta {}",
                            if st.palette.is_empty() {
                                "del mesh"
                            } else {
                                &st.palette
                            }
                        ),
                    ] {
                        push_text(
                            &mut mesh,
                            bar.x + pad,
                            ty,
                            logical_w,
                            logical_h,
                            &row,
                            px,
                            [0.88, 0.90, 0.95, 1.0],
                        );
                        ty += 12.0 * s;
                    }
                }
                // State rows → jump straight to one.
                let mut ry = bar.y + pad + 16.0 * s + readout;
                for (i, st) in states.iter().enumerate().take(EDITOR_VISIBLE_ROWS) {
                    let rect = HudRect {
                        x: bar.x + pad,
                        y: ry,
                        w: w - pad * 2.0,
                        h: row_h,
                    };
                    push_text_btn(
                        &mut mesh,
                        rect,
                        logical_w,
                        logical_h,
                        st.label(),
                        1.8 * s,
                        if i == idx { accent } else { btn_bg },
                        HudAction::Ed(EditorAction::StateSel(i)),
                    );
                    ry += row_h + gap;
                }
                let buttons: [(&str, EditorAction); 5] = [
                    ("ANT", EditorAction::StatePrev),
                    ("SIG", EditorAction::StateNext),
                    ("+NUEVO", EditorAction::StateAdd),
                    ("COPIAR", EditorAction::StateDup),
                    ("BORRAR", EditorAction::StateDel),
                ];
                let by = bar.y + pad + 16.0 * s + readout + shown as f32 * (row_h + gap);
                for (i, (label, act)) in buttons.iter().enumerate() {
                    let bg = if *act == EditorAction::StateDel {
                        [0.30, 0.16, 0.18, 0.95]
                    } else {
                        btn_bg
                    };
                    push_text_btn(
                        &mut mesh,
                        HudRect {
                            x: bar.x + pad + (i % 3) as f32 * (fw + gap),
                            y: by + (i / 3) as f32 * (row_h + gap),
                            w: fw,
                            h: row_h,
                        },
                        logical_w,
                        logical_h,
                        label,
                        text_px,
                        bg,
                        HudAction::Ed(*act),
                    );
                }
            }
            // Timeline + los 4 niveles de selección (Fase 6). El widget es nuevo:
            // un eje temporal horizontal, no la lista vertical de entidades.
            EditorPanel::Animacion => {
                let fw = 100.0 * s;
                let px = 1.6 * s;
                let w = fw * 4.0 + gap * 3.0 + pad * 2.0;
                let strip_h = 26.0 * s;
                let h = pad * 2.0 + 16.0 * s + 12.0 * s + strip_h + 14.0 * s
                    + 12.0 * s * 2.0
                    + row_h * 3.0
                    + gap * 3.0;
                let bar = HudRect {
                    x: (logical_w - w) * 0.5,
                    y: logical_h - h - gap,
                    w,
                    h,
                };
                push_panel(&mut mesh, bar, logical_w, logical_h, dim_bg);
                push_text(
                    &mut mesh,
                    bar.x + pad,
                    bar.y + pad,
                    logical_w,
                    logical_h,
                    EditorPanel::Animacion.label(),
                    text_px,
                    [0.98, 0.86, 0.42, 1.0],
                );
                let row = |mesh: &mut HudMesh, txt: String, y: f32| {
                    push_text(
                        mesh,
                        bar.x + pad,
                        y,
                        logical_w,
                        logical_h,
                        &txt,
                        px,
                        [0.88, 0.90, 0.95, 1.0],
                    );
                };
                // Sin clip no hay timeline; el resto del panel no aplica.
                let Some(c) = clip else {
                    row(
                        &mut mesh,
                        "SIN CLIP: USA ARCHIVO > IMPORTAR".into(),
                        bar.y + pad + 16.0 * s,
                    );
                    return mesh;
                };
                let frames = &c.file.frames;
                let n = frames.len();
                let dur = c.file.duration_s.max(1e-3);
                let kf = kf_sel.min(n.saturating_sub(1));
                row(&mut mesh, c.summary(), bar.y + pad + 16.0 * s);

                // ── Timeline: regla + un diamante por keyframe ──────────────
                let strip_y = bar.y + pad + 16.0 * s + 12.0 * s;
                push_panel(
                    &mut mesh,
                    HudRect {
                        x: bar.x + pad,
                        y: strip_y + strip_h * 0.5,
                        w: w - pad * 2.0,
                        h: 1.0 * s,
                    },
                    logical_w,
                    logical_h,
                    [0.34, 0.38, 0.48, 0.9],
                );
                let slot = (w - pad * 2.0) / n.max(1) as f32;
                for (i, f) in frames.iter().enumerate() {
                    // Diamante dibujado en su posicion temporal (t / dur) …
                    let fx = bar.x + pad + (f.t / dur).clamp(0.0, 1.0) * (w - pad * 2.0);
                    let d = 8.0 * s;
                    let diamond = HudRect {
                        x: fx - d * 0.5,
                        y: strip_y + strip_h * 0.5 - d * 0.5,
                        w: d,
                        h: d,
                    };
                    // … y su hit region es una ranura igualitaria, que es lo
                    // que hace falta para acertar con el dedo.
                    let hit = HudRect {
                        x: bar.x + pad + i as f32 * slot,
                        y: strip_y,
                        w: slot,
                        h: strip_h,
                    };
                    if i == kf {
                        push_panel(&mut mesh, diamond, logical_w, logical_h, accent);
                    } else {
                        push_panel(
                            &mut mesh,
                            HudRect {
                                x: diamond.x,
                                y: diamond.y,
                                w: diamond.w,
                                h: 1.0 * s,
                            },
                            logical_w,
                            logical_h,
                            [0.70, 0.76, 0.86, 0.95],
                        );
                        push_panel(
                            &mut mesh,
                            HudRect {
                                x: diamond.x,
                                y: diamond.y + diamond.h - 1.0 * s,
                                w: diamond.w,
                                h: 1.0 * s,
                            },
                            logical_w,
                            logical_h,
                            [0.70, 0.76, 0.86, 0.95],
                        );
                    }
                    mesh.hits.push(HudHitRegion {
                        action: HudAction::Ed(EditorAction::KeyframeSel(i)),
                        rect: hit,
                    });
                }

                let y2 = strip_y + strip_h + 4.0 * s;
                row(
                    &mut mesh,
                    format!("clave {}/{} · t {:.2}s", kf + 1, n, frames[kf].t),
                    y2,
                );
                let jname = crate::animation::joint_names()
                    .get(joint_sel)
                    .copied()
                    .unwrap_or("-");
                let axis = crate::animation::joint_axis(jname)
                    .map(|a| a.to_ascii_uppercase())
                    .unwrap_or('?');
                row(
                    &mut mesh,
                    format!(
                        "artic {}/12 · {jname} · eje {axis}",
                        joint_sel + 1
                    ),
                    y2 + 12.0 * s,
                );

                // ── Botones: cursores, target y el subconjunto de steppers ──
                let by = y2 + 12.0 * s * 2.0 + 4.0 * s;
                let btn_at = |mesh: &mut HudMesh, i: usize, row_i: usize, label: &str, act: EditorAction, bg: [f32; 4]| {
                    push_text_btn(
                        mesh,
                        HudRect {
                            x: bar.x + pad + (i % 4) as f32 * (fw + gap),
                            y: by + row_i as f32 * (row_h + gap),
                            w: fw,
                            h: row_h,
                        },
                        logical_w,
                        logical_h,
                        label,
                        text_px,
                        bg,
                        HudAction::Ed(act),
                    )
                };
                btn_at(&mut mesh, 0, 0, "CLAVE<", EditorAction::KeyframePrev, btn_bg);
                btn_at(&mut mesh, 1, 0, "CLAVE>", EditorAction::KeyframeNext, btn_bg);
                btn_at(
                    &mut mesh,
                    2,
                    0,
                    "TIEMPO-",
                    EditorAction::KeyframeTimeDec,
                    btn_bg,
                );
                btn_at(
                    &mut mesh,
                    3,
                    0,
                    "TIEMPO+",
                    EditorAction::KeyframeTimeInc,
                    btn_bg,
                );
                btn_at(&mut mesh, 0, 1, "ARTIC<", EditorAction::JointPrev, btn_bg);
                btn_at(&mut mesh, 1, 1, "ARTIC>", EditorAction::JointNext, btn_bg);
                // Qué editan los steppers: entidad/estado o articulación.
                btn_at(
                    &mut mesh,
                    2,
                    1,
                    "OBJETO",
                    EditorAction::EditEntity,
                    if view.editing_joint { btn_bg } else { accent },
                );
                btn_at(
                    &mut mesh,
                    3,
                    1,
                    "ARTIC",
                    EditorAction::EditJoint,
                    if view.editing_joint { accent } else { btn_bg },
                );
                // Solo los dos steppers del eje de la articulación: los otros
                // canales (pos, escala, tamaño, cizalla) no tienen destino aquí.
                // Los botones mandan la acción del eje REAL, no una fija.
                let (dec, inc) = match crate::animation::joint_axis(jname) {
                    Some('x') => (EditorAction::RotXDec, EditorAction::RotXInc),
                    Some('y') => (EditorAction::RotYDec, EditorAction::RotYInc),
                    Some('z') => (EditorAction::RotZDec, EditorAction::RotZInc),
                    _ => (EditorAction::RotXDec, EditorAction::RotXInc),
                };
                btn_at(&mut mesh, 0, 2, &format!("{axis}-"), dec, btn_bg);
                btn_at(&mut mesh, 1, 2, &format!("{axis}+"), inc, btn_bg);
                if !view.editing_joint {
                    // Los steppers también existen en el panel OBJETO: aquí solo
                    // se avisa de que están apagados.
                    row(&mut mesh, "steppers: OBJETO".into(), by + row_h * 3.0);
                } else {
                    row(&mut mesh, "steppers: ARTIC".into(), by + row_h * 3.0);
                }
            }
            // Categories whose controls land in a later phase: the panel is
            // reachable, it just states what is not built yet.
            pending => {
                let (title, sub) = match pending {
                    EditorPanel::Voxels => ("VOXELS INDIVIDUALES", "PENDIENTE"),
                    EditorPanel::Transform
                    | EditorPanel::Estado
                    | EditorPanel::Animacion
                    | EditorPanel::Archivo => unreachable!(),
                };
                let w = 340.0 * s;
                let h = 64.0 * s;
                let bar = HudRect {
                    x: (logical_w - w) * 0.5,
                    y: logical_h - h - gap,
                    w,
                    h,
                };
                push_panel(&mut mesh, bar, logical_w, logical_h, dim_bg);
                push_text(
                    &mut mesh,
                    bar.x + pad,
                    bar.y + 12.0 * s,
                    logical_w,
                    logical_h,
                    title,
                    text_px,
                    [0.98, 0.86, 0.42, 1.0],
                );
                push_text(
                    &mut mesh,
                    bar.x + pad,
                    bar.y + 32.0 * s,
                    logical_w,
                    logical_h,
                    sub,
                    1.6 * s,
                    [0.78, 0.82, 0.88, 1.0],
                );
            }
        }
    }
    mesh
}

/// Minecraft-style inventory: dim + beige panel + 9×3 slots + tool hotbar mirror.
pub fn build_inventory_hud(
    open: bool,
    inventory: &crate::inventory::PlayerInventory,
    hotbar: &Hotbar,
    mouse: (f32, f32),
    logical_w: f32,
    logical_h: f32,
) -> HudMesh {
    use crate::inventory::{INV_COLS, INV_ROWS};

    let mut mesh = HudMesh::default();
    if !open {
        return mesh;
    }

    // Full-screen dim + hit catch (blocks world / lower HUD clicks).
    let screen = HudRect {
        x: 0.0,
        y: 0.0,
        w: logical_w,
        h: logical_h,
    };
    push_panel(
        &mut mesh,
        screen,
        logical_w,
        logical_h,
        [0.0, 0.0, 0.0, 0.55],
    );
    mesh.hits.push(HudHitRegion {
        action: HudAction::InventoryBackdrop,
        rect: screen,
    });

    let slot = 40.0;
    let gap = 4.0;
    let pad = 16.0;
    let tool_gap = 14.0;
    let grid_w = INV_COLS as f32 * slot + (INV_COLS as f32 - 1.0) * gap;
    let grid_h = INV_ROWS as f32 * slot + (INV_ROWS as f32 - 1.0) * gap;
    let tools_w = HOTBAR_SLOTS as f32 * slot + (HOTBAR_SLOTS as f32 - 1.0) * gap;
    let panel_w = grid_w + pad * 2.0;
    let panel_h = pad + 22.0 + grid_h + tool_gap + slot + pad;
    let panel = HudRect {
        x: (logical_w - panel_w) * 0.5,
        y: (logical_h - panel_h) * 0.5,
        w: panel_w,
        h: panel_h,
    };

    // Outer dark border + beige body (#C6C6C6).
    push_panel(
        &mut mesh,
        panel,
        logical_w,
        logical_h,
        [0.15, 0.15, 0.15, 1.0],
    );
    let body = HudRect {
        x: panel.x + 3.0,
        y: panel.y + 3.0,
        w: panel.w - 6.0,
        h: panel.h - 6.0,
    };
    push_panel(
        &mut mesh,
        body,
        logical_w,
        logical_h,
        [0.776, 0.776, 0.776, 1.0],
    );
    mesh.hits.push(HudHitRegion {
        action: HudAction::InventoryPanel,
        rect: body,
    });

    // Title chip
    let title = HudRect {
        x: body.x + pad - 4.0,
        y: body.y + 6.0,
        w: 56.0,
        h: 16.0,
    };
    let (uv0, uv1) = atlas_uv(4, 2, 2);
    push_quad(
        &mut mesh,
        title,
        logical_w,
        logical_h,
        uv0,
        uv1,
        [0.25, 0.25, 0.28, 1.0],
    );

    let ox = body.x + pad;
    let oy = body.y + 28.0;
    for row in 0..INV_ROWS {
        for col in 0..INV_COLS {
            let i = row * INV_COLS + col;
            let rect = HudRect {
                x: ox + col as f32 * (slot + gap),
                y: oy + row as f32 * (slot + gap),
                w: slot,
                h: slot,
            };
            push_mc_slot(&mut mesh, rect, logical_w, logical_h, false);
            if let Some(stack) = inventory.storage[i] {
                push_stack_icon(&mut mesh, rect, stack, logical_w, logical_h);
            }
            mesh.hits.push(HudHitRegion {
                action: HudAction::InvSlot(i as u8),
                rect,
            });
        }
    }

    // Tool hotbar mirror (4 slots — same as world hotbar).
    let tools_x = body.x + (body.w - tools_w) * 0.5;
    let tools_y = oy + grid_h + tool_gap;
    let mut tool_rects = [HudRect::default(); HOTBAR_SLOTS];
    for i in 0..HOTBAR_SLOTS {
        let selected = hotbar.selected == Some(i);
        let rect = HudRect {
            x: tools_x + i as f32 * (slot + gap),
            y: tools_y,
            w: slot,
            h: slot,
        };
        tool_rects[i] = rect;
        push_mc_slot(&mut mesh, rect, logical_w, logical_h, selected);
        mesh.hits.push(HudHitRegion {
            action: HudAction::SelectSlot(i as u8),
            rect,
        });
    }
    for i in 0..HOTBAR_SLOTS {
        let Some(col) = hotbar.slots[i].atlas_col() else {
            continue;
        };
        let selected = hotbar.selected == Some(i);
        let rect = tool_rects[i];
        let scale = if selected { 1.42 } else { 1.28 };
        let iw = slot * scale;
        let ih = slot * scale;
        let icon = HudRect {
            x: rect.x + (slot - iw) * 0.5,
            y: rect.y + (slot - ih) * 0.5 - 2.0,
            w: iw,
            h: ih,
        };
        let (uv0, uv1) = atlas_uv(col, 3, 1);
        push_quad(
            &mut mesh,
            icon,
            logical_w,
            logical_h,
            uv0,
            uv1,
            [1.0, 1.0, 1.0, 1.0],
        );
    }

    // Cursor-held stack follows the mouse.
    if let Some(stack) = inventory.cursor {
        let rect = HudRect {
            x: mouse.0 - slot * 0.5,
            y: mouse.1 - slot * 0.5,
            w: slot,
            h: slot,
        };
        push_stack_icon(&mut mesh, rect, stack, logical_w, logical_h);
    }

    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El readout de la derecha tiene que imprimir el mismo transform que
    /// mueven los pasos: el del estado activo si la entidad tiene estados, el
    /// de la entidad si no. Antes lo leía de `entity` a secas, así que en
    /// cuanto una entidad tenía estados el readout y las barras marcaban
    /// distinto, y solo se notaba con estados — sin ellos las dos fuentes son
    /// el mismo array y el fallo es invisible.
    ///
    /// Se fija sobre el texto porque es lo que se ve. La entidad tiene los
    /// cuatro canales muy distintos de los de sus estados, para que un lector
    /// que vuelva a la entidad falle en los cuatro a la vez y no solo en el eje
    /// que el autor movió por casualidad.
    #[test]
    fn transform_readout_prints_the_active_state_not_the_entity() {
        use crate::editor::{EditorEntity, EditorEntityState};

        let mut e = EditorEntity::new("prop", [8.0, 30.0, 24.0]);
        e.rotation = [0.0, 90.0, 0.0];
        e.scale = [1.0, 1.0, 1.0];
        e.skew = [0.0, 0.0, 0.0];
        e.size = [1.5, 2.0, 1.0];

        // Sin estados: los cuatro canales salen de la entidad.
        let plain = transform_readout(&e, 0);
        assert!(plain[0].contains("8.0 30.0 24.0"), "pos: {}", plain[0]);
        assert!(plain[1].contains("0 90 0"), "rot: {}", plain[1]);
        assert!(plain[2].contains("1.00 1.00 1.00"), "esc: {}", plain[2]);
        assert!(plain[3].contains("zes 0.00"), "zes: {}", plain[3]);
        // `caja` viene de la entidad y no del estado, y aquí coinciden.
        assert!(plain[3].contains("caja 1.5"), "caja: {}", plain[3]);

        // Con estados: el readout sigue al activo, no al 0 ni a la entidad.
        e.states.push(EditorEntityState {
            name: "a".into(),
            position: [1.0, 2.0, 3.0],
            rotation: [10.0, 20.0, 30.0],
            scale: [0.5, 0.5, 0.5],
            skew: [0.0, 0.0, 0.25],
            ..Default::default()
        });
        e.states.push(EditorEntityState {
            name: "b".into(),
            position: [-4.0, 50.0, 6.0],
            rotation: [40.0, 50.0, 60.0],
            scale: [2.0, 2.0, 2.0],
            skew: [0.0, 0.0, -1.5],
            ..Default::default()
        });

        let first = transform_readout(&e, 0);
        assert!(first[0].contains("1.0 2.0 3.0"), "pos estado 0: {}", first[0]);
        assert!(first[1].contains("10 20 30"), "rot estado 0: {}", first[1]);
        assert!(first[2].contains("0.50 0.50 0.50"), "esc: {}", first[2]);
        assert!(first[3].contains("zes 0.25"), "zes: {}", first[3]);

        let second = transform_readout(&e, 1);
        assert!(second[0].contains("-4.0 50.0 6.0"), "pos estado 1: {}", second[0]);
        assert!(second[1].contains("40 50 60"), "rot estado 1: {}", second[1]);
        assert!(second[2].contains("2.00 2.00 2.00"), "esc: {}", second[2]);
        assert!(second[3].contains("zes -1.50"), "zes: {}", second[3]);

        // Ninguna de las dos líneas puede seguir leyendo la entidad: sus
        // valores son los de arriba, y los de la entidad son otros muy distintos.
        for (i, line) in [first[0].clone(), second[0].clone()].into_iter().enumerate() {
            assert!(
                !line.contains("8.0 30.0 24.0"),
                "la línea {i} volvió a la entidad: {line}"
            );
        }
        // `caja` sí se queda en la entidad, en las dos lecturas.
        assert!(first[3].contains("caja 1.5") && second[3].contains("caja 1.5"));
    }

    /// El caso anterior se ve igual si el readout leyera de la entidad,
    /// siempre que la entidad no tenga estados. Este lo dice al revés: con
    /// estados, la entidad y el readout tienen que diferir. Si `transform_readout`
    /// volviera a la entidad, las dos cadenas comparadas aquí serían iguales.
    #[test]
    fn transform_readout_diverges_from_the_entity_once_states_exist() {
        use crate::editor::{EditorEntity, EditorEntityState};

        let mut e = EditorEntity::new("prop", [8.0, 30.0, 24.0]);
        let before = transform_readout(&e, 0);
        e.states.push(EditorEntityState {
            name: "a".into(),
            position: [1.0, 2.0, 3.0],
            ..Default::default()
        });
        let after = transform_readout(&e, 0);
        assert_ne!(
            before[0], after[0],
            "el readout no cambió al añadir un estado: {before:?} vs {after:?}"
        );
    }

    #[test]
    fn atlas_nonempty() {
        let img = build_hud_atlas();
        assert_eq!(img.width(), HUD_TILE * ATLAS_COLS);
        assert!(img.pixels().any(|p| p.0[3] > 0));
    }

    /// Regresión: `atlas_col` y `build_hud_atlas` se desincronizaron al añadir
    /// variantes al enum, y el pico acabó mostrando el icono del hacha. Este
    /// test recorre TODOS los `HotbarItem` y exige que el tile al que apuntan
    /// tenga píxeles opacos de verdad.
    #[test]
    fn every_hotbar_item_has_an_icon() {
        let img = build_hud_atlas();
        let all = [
            HotbarItem::Sword,
            HotbarItem::Shield,
            HotbarItem::Pickaxe,
            HotbarItem::Axe,
            HotbarItem::Consumable,
            HotbarItem::Magic,
        ];
        for item in all {
            let col = item
                .atlas_col()
                .unwrap_or_else(|| panic!("{} deberia tener icono", item.label()));
            let (ox, oy) = tile_origin(col, 3);
            let mut opaque = 0u32;
            for y in 0..HUD_TILE {
                for x in 0..HUD_TILE {
                    if img.get_pixel(ox + x, oy + y).0[3] > 8 {
                        opaque += 1;
                    }
                }
            }
            assert!(
                opaque > 20,
                "el tile del {} (col {col}) esta vacio: {opaque} px opacos",
                item.label()
            );
        }
    }

    /// El hacha y el pico son tiles distintos: comparten fila pero no columna.
    #[test]
    fn pickaxe_and_axe_icons_differ() {
        let img = build_hud_atlas();
        let sample = |col: u32| -> Vec<u8> {
            let (ox, oy) = tile_origin(col, 3);
            let mut v = Vec::new();
            for y in 0..HUD_TILE {
                for x in 0..HUD_TILE {
                    v.push(img.get_pixel(ox + x, oy + y).0[3]);
                }
            }
            v
        };
        assert_ne!(
            sample(hotbar_col::PICKAXE),
            sample(hotbar_col::AXE),
            "pico y hacha comparten tile"
        );
    }

    /// Regresión: `DepthBoots` pedía la columna 10 con solo 10 columnas, y
    /// ClampToEdge leía la "O" de la brújula. Ahora el tile se valida en rango.
    #[test]
    fn every_inventory_item_tile_is_in_range_and_baked() {
        use crate::inventory::InvItem;
        let img = build_hud_atlas();
        let all = [
            InvItem::DirtFrag,
            InvItem::StoneFrag,
            InvItem::CoalMicro,
            InvItem::CoalCube,
            InvItem::SapphireMicro,
            InvItem::SapphireCube,
            InvItem::RubyMicro,
            InvItem::RubyCube,
            InvItem::EmeraldMicro,
            InvItem::EmeraldCube,
            InvItem::DepthBoots,
        ];
        for item in all {
            let (col, row) = item.atlas_tile();
            assert!(
                col < ATLAS_COLS && row < ATLAS_ROWS,
                "{item:?} apunta fuera del atlas: ({col},{row})"
            );
            let (ox, oy) = tile_origin(col, row);
            let opaque = (0..HUD_TILE)
                .flat_map(|y| (0..HUD_TILE).map(move |x| (x, y)))
                .filter(|&(x, y)| img.get_pixel(ox + x, oy + y).0[3] > 8)
                .count();
            assert!(opaque > 20, "el tile de {item:?} esta vacio: {opaque} px");
        }
    }

    #[test]
    fn hud_builds_when_armed() {
        let mut t = StairTool::default();
        t.toggle();
        let mesh = build_stair_hud(&t, 0.0, 1280.0, 720.0);
        assert!(!mesh.vertices.is_empty());
        assert!(!mesh.hits.is_empty());
        assert!(hit_test(&mesh.hits, 10.0, 10.0).is_none());
    }

    #[test]
    fn status_hud_has_hearts_and_bottles() {
        let mesh = build_status_hud(4, 4, 0, 3, 1280.0, 720.0);
        // 1 panel + 4 hearts + 3 bottles = 8 quads → 32 verts
        assert_eq!(mesh.vertices.len(), 8 * 4);
    }

    #[test]
    fn hotbar_has_inv_button_and_four_slots() {
        let hb = Hotbar::default();
        assert_eq!(hb.slots.len(), 4);
        let mesh = build_hotbar_hud(&hb, false, 1280.0, 720.0);
        assert_eq!(mesh.hits.len(), 5);
        assert_eq!(mesh.hits[0].action, HudAction::ToggleInventory);
        assert!(!mesh.vertices.is_empty());
        let r = mesh.hits[1].rect;
        assert_eq!(
            hit_test(&mesh.hits, r.x + 4.0, r.y + 4.0),
            Some(HudAction::SelectSlot(0))
        );
    }

    #[test]
    fn hotbar_press_toggles_equip() {
        let mut hb = Hotbar::default();
        assert_eq!(hb.selected, Some(1));
        hb.press_slot(1, false);
        assert_eq!(hb.selected, None);
        assert!(!hb.holding_pickaxe());
        hb.press_slot(1, false);
        assert!(hb.holding_pickaxe());
        hb.press_slot(0, false);
        assert_eq!(hb.selected_item(), HotbarItem::Sword);
        assert!(hb.holding_sword());
        assert!(!hb.holding_pickaxe());
        hb.press_slot(0, false);
        assert!(!hb.holding_sword());
    }

    #[test]
    fn realm_sign_builds_text() {
        let mesh = build_realm_sign_hud("REINO BRANDOR", "CAPITAL A 34 M", 1280.0, 720.0, 1.0);
        // Panel + rule + many glyph-pixel quads.
        assert!(mesh.vertices.len() > 40 * 4);
        // Empty lines → empty mesh.
        let empty = build_realm_sign_hud("", "", 1280.0, 720.0, 1.0);
        assert!(empty.vertices.is_empty());
        let faded = build_realm_sign_hud("REINO X", "ALDEA A 80 M", 1280.0, 720.0, 0.0);
        assert!(faded.vertices.is_empty());
        assert!((settlement_sign_opacity(Some(10)) - 1.0).abs() < 0.01);
        assert!(settlement_sign_opacity(Some(70)) < 0.2);
    }

    #[test]
    fn compass_hud_builds() {
        let mesh = build_compass_hud(-std::f32::consts::FRAC_PI_2, 1280.0, 720.0);
        assert!(!mesh.vertices.is_empty());
    }

    #[test]
    fn compass_north_on_top_when_facing_north() {
        use crate::stair_dig::{facing_screen_angle, TunnelFacing};
        let yaw_n = TunnelFacing::North.yaw();
        let ang = facing_screen_angle(TunnelFacing::North, yaw_n);
        // Top of dial ≈ screen-up ⇒ angle near -π/2 (sin negative).
        assert!(
            ang.sin() < -0.7,
            "N should sit near top when facing north, ang={ang}"
        );
        let yaw_e = TunnelFacing::East.yaw();
        let ang_n = facing_screen_angle(TunnelFacing::North, yaw_e);
        // Facing east: world north is to the left of forward ⇒ screen-left ≈ π.
        assert!(
            ang_n.cos() < -0.7,
            "N should sit near left when facing east, ang={ang_n}"
        );
    }

    /// El editor colapsable: con el menú cerrado solo hay selección de entidad
    /// + botón de menú. La parrilla de transform y las acciones de archivo
    /// viven dentro de su panel, no siempre en pantalla.
    #[test]
    fn editor_hud_hides_controls_until_a_panel_opens() {
        let entities = [crate::editor::EditorEntity::new("prop", [0.0, 24.0, 0.0])];
        let has = |mesh: &HudMesh, act: EditorAction| {
            mesh.hits.iter().any(|h| h.action == HudAction::Ed(act))
        };
        let build = |panel| {
            build_editor_hud(
                "escena",
                "",
                "",
                0.0,
                &entities,
                EditorView {
                    selected: 0,
                    panel,
                    ..Default::default()
                },
                None,
                &[],
                1280.0,
                720.0,
            )
        };
        let closed = build(None);
        // El menú abre la categoría por defecto; la lista de entidades sigue viva.
        assert!(has(&closed, EditorAction::OpenPanel(EditorPanel::Transform)));
        assert!(has(&closed, EditorAction::SelNext));
        // Y nada de la parrilla ni de los archivos.
        for hidden in [
            EditorAction::PosXInc,
            EditorAction::RotYDec,
            EditorAction::SclZInc,
            EditorAction::SkewInc,
            EditorAction::SizeDec,
            EditorAction::KindNext,
            EditorAction::Add,
            EditorAction::Save,
            EditorAction::Load,
            EditorAction::Back,
            EditorAction::StateAdd,
        ] {
            assert!(!has(&closed, hidden), "{hidden:?} no debe verse sin panel");
        }

        // Transform abierto → la parrilla de barras vuelve (no son hit regions:
        // una barra se aprieta y se arrastra, y eso lo pregunta el ratón), y los
        // archivos siguen ocultos. Lo que sí son botones sigue siendo alcanzable.
        let transform = build(Some(EditorPanel::Transform));
        for shown in [
            EditorAction::SelPrev,
            EditorAction::SelNext,
            EditorAction::KindPrev,
            EditorAction::KindNext,
        ] {
            assert!(has(&transform, shown), "{shown:?} sigue siendo un botón");
        }
        for gone in [EditorAction::PosXInc, EditorAction::SkewInc] {
            assert!(
                !has(&transform, gone),
                "{gone:?} ya no es un botón: es una barra"
            );
        }
        assert!(!has(&transform, EditorAction::Save));
        // Con un panel abierto, las cinco categorías son alcanzables.
        for cat in EditorPanel::ALL {
            assert!(has(&transform, EditorAction::OpenPanel(cat)), "{cat:?}");
        }

        // Archivo abierto → acciones de archivo, sin la parrilla.
        let file = build(Some(EditorPanel::Archivo));
        for shown in [
            EditorAction::Add,
            EditorAction::Duplicate,
            EditorAction::Delete,
            EditorAction::Save,
            EditorAction::Load,
            EditorAction::Back,
        ] {
            assert!(has(&file, shown), "{shown:?} debe verse en ARCHIVO");
        }
        assert!(!has(&file, EditorAction::PosXInc));
        assert!(has(&file, EditorAction::ClosePanel));

        // Estado abierto → el gestor de estados, y nada de transform/archivo.
        let estados = build(Some(EditorPanel::Estado));
        for shown in [
            EditorAction::StatePrev,
            EditorAction::StateNext,
            EditorAction::StateAdd,
            EditorAction::StateDup,
            EditorAction::StateDel,
        ] {
            assert!(has(&estados, shown), "{shown:?} debe verse en ESTADO");
        }
        assert!(!has(&estados, EditorAction::PosXInc));
        assert!(!has(&estados, EditorAction::Save));
        // La entidad no tiene estados: no hay filas, solo el +NUEVO.
        assert!(!has(&estados, EditorAction::StateSel(0)));

        // Las categorías pendientes abren panel pero no emiten comandos.
        for cat in [EditorPanel::Voxels, EditorPanel::Animacion] {
            let mesh = build(Some(cat));
            assert!(has(&mesh, EditorAction::OpenPanel(EditorPanel::Archivo)));
            assert!(!has(&mesh, EditorAction::PosXInc));
            assert!(!has(&mesh, EditorAction::Save));
        }
    }

    /// Las barras del panel Transform: una por campo, dentro del panel y sin
    /// solaparse, cada una con el valor que le pasa el editor y su parte de
    /// barra llena. No son hit regions (eso lo pregunta el ratón), así que lo
    /// que se asserta es la geometría y lo que se dibuja encima.
    #[test]
    fn editor_transform_bars_cover_the_panel_and_carry_their_value() {
        let layout = editor_transform_panel(1280.0, 720.0);
        // Una barra por campo, en el orden del panel, y todas dentro de él.
        for (i, bar) in layout.bars.iter().enumerate() {
            assert_eq!(bar.field, crate::editor::TransformField::ALL[i]);
            assert!(layout.rect.contains(bar.rect.x + 1.0, bar.rect.y + 1.0));
            assert!(layout.rect.contains(bar.rect.x + bar.rect.w - 1.0, bar.rect.y + bar.rect.h - 1.0));
        }
        // Ninguna se pisa con otra.
        for (i, a) in layout.bars.iter().enumerate() {
            for b in layout.bars.iter().skip(i + 1) {
                let disjoint = a.rect.x + a.rect.w <= b.rect.x
                    || b.rect.x + b.rect.w <= a.rect.x
                    || a.rect.y + a.rect.h <= b.rect.y
                    || b.rect.y + b.rect.h <= a.rect.y;
                assert!(disjoint, "{:?} se pisa con {:?}", a.field, b.field);
            }
        }
        // El skew es solo el eje Z y el size es una sola barra para los tres.
        assert_eq!(layout.bars.iter().filter(|b| b.field == crate::editor::TransformField::SkewZ).count(), 1);
        assert_eq!(layout.bars.iter().filter(|b| b.field == crate::editor::TransformField::Size).count(), 1);

        // Con valores, el panel dibuja las barras: más geometría que sin ellas,
        // y los botones de selección / tipo siguen siendo hits.
        let entities = [crate::editor::EditorEntity::new("prop", [0.0, 24.0, 0.0])];
        let view = EditorView {
            selected: 0,
            panel: Some(EditorPanel::Transform),
            ..Default::default()
        };
        let bare = build_editor_hud("escena", "", "", 0.0, &entities, view, None, &[], 1280.0, 720.0);
        let mut bars = layout.bars;
        bars[0].value = 8.5;
        bars[0].fill = Some(0.25);
        let drawn = build_editor_hud("escena", "", "", 0.0, &entities, view, None, &bars, 1280.0, 720.0);
        assert!(drawn.vertices.len() > bare.vertices.len());
        let has = |mesh: &HudMesh, act: EditorAction| {
            mesh.hits.iter().any(|h| h.action == HudAction::Ed(act))
        };
        for a in [
            EditorAction::SelPrev,
            EditorAction::SelNext,
            EditorAction::KindPrev,
            EditorAction::KindNext,
        ] {
            assert!(has(&drawn, a), "{a:?} debe seguir siendo botón");
        }
        // Y una barra nunca se confunde con un botón de paso.
        assert!(!has(&drawn, EditorAction::PosXInc));
    }

    /// El subtítulo se mantiene y se desvanece por tiempo, no por frames, y solo
    /// se dibuja mientras le queda algo de alpha.
    #[test]
    fn editor_subtitle_holds_then_fades_out_by_time() {
        assert!((subtitle_alpha(0.0) - 1.0).abs() < 1e-6);
        assert!((subtitle_alpha(ED_SUBTITLE_HOLD_SECS - 0.01) - 1.0).abs() < 1e-6);
        let mid = ED_SUBTITLE_HOLD_SECS + ED_SUBTITLE_FADE_SECS * 0.5;
        assert!((subtitle_alpha(mid) - 0.5).abs() < 1e-6, "la mitad del fundido");
        assert_eq!(subtitle_alpha(ED_SUBTITLE_HOLD_SECS + ED_SUBTITLE_FADE_SECS), 0.0);
        assert_eq!(subtitle_alpha(99.0), 0.0, "y se queda en cero");

        let entities = [crate::editor::EditorEntity::new("prop", [0.0, 24.0, 0.0])];
        let view = EditorView::default();
        let fresh = build_editor_hud(
            "escena",
            "un estado",
            "pos X 8.5",
            0.0,
            &entities,
            view,
            None,
            &[],
            1280.0,
            720.0,
        );
        // Sin subtítulo, el panel dibuja lo mismo de siempre: la tira con su
        // status y nada más.
        let bare = build_editor_hud(
            "escena",
            "un estado",
            "",
            0.0,
            &entities,
            view,
            None,
            &[],
            1280.0,
            720.0,
        );
        assert!(fresh.vertices.len() > bare.vertices.len());
        // Y ya desvanecido no dibuja nada, aunque el texto siga ahí.
        let gone = build_editor_hud(
            "escena",
            "un estado",
            "pos X 8.5",
            ED_SUBTITLE_HOLD_SECS + ED_SUBTITLE_FADE_SECS,
            &entities,
            view,
            None,
            &[],
            1280.0,
            720.0,
        );
        assert_eq!(gone.vertices.len(), bare.vertices.len());
    }

    /// Con dos estados, el panel ESTADO ofrece una fila por estado (la activa
    /// resaltada) y el readout sale del subconjunto que se le pasa.
    #[test]
    fn editor_hud_lists_every_state_of_the_selected_entity() {
        let mut e = crate::editor::EditorEntity::new("item", [0.0, 24.0, 0.0]);
        e.model = "special1_sword".into();
        e.states = vec![
            crate::editor::EditorEntityState {
                name: "normal".into(),
                ..Default::default()
            },
            crate::editor::EditorEntityState {
                name: "gastada".into(),
                model: "assets/items/espada_worn.json".into(),
                palette: "mono".into(),
                position: [0.0, -0.5, 0.0],
                ..Default::default()
            },
        ];
        let entities = [e];
        let has = |mesh: &HudMesh, act: EditorAction| {
            mesh.hits.iter().any(|h| h.action == HudAction::Ed(act))
        };
        let mesh = build_editor_hud(
            "estados",
            "",
            "",
            0.0,
            &entities,
            EditorView {
                selected: 0,
                state_sel: 1,
                panel: Some(EditorPanel::Estado),
                ..Default::default()
            },
            None,
            &[],
            1280.0,
            720.0,
        );
        // Una fila por estado (solo 2 filas, no 5).
        assert!(has(&mesh, EditorAction::StateSel(0)));
        assert!(has(&mesh, EditorAction::StateSel(1)));
        assert!(!has(&mesh, EditorAction::StateSel(2)));
    }

    /// El panel ANIMAR (Fase 6 C) muestra el clip abierto y sus acciones de
    /// import/export viven en ARCHIVO, no en ANIMAR.
    #[test]
    fn editor_hud_shows_the_open_clip_and_its_file_actions() {
        let entities = [crate::editor::EditorEntity::new("prop", [0.0, 24.0, 0.0])];
        let has = |mesh: &HudMesh, act: EditorAction| {
            mesh.hits.iter().any(|h| h.action == HudAction::Ed(act))
        };
        let build = |panel, clip| {
            build_editor_hud(
                "escena",
                "",
                "",
                0.0,
                &entities,
                EditorView {
                    selected: 0,
                    panel,
                    ..Default::default()
                },
                clip,
                &[],
                1280.0,
                720.0,
            )
        };
        let clip_json = r#"{
            "id": "borrador_ia", "model": "assets/entities/hero.json",
            "duration_s": 0.4, "loop": true,
            "frames": [{ "t": 0.0, "pose": {} }, { "t": 0.4, "pose": {} }]
        }"#;
        let clip = crate::editor_clip::EditorClip::from_json(clip_json, None).expect("clip");

        // ARCHIVO: las 6 acciones de escena + import/export de clip + malla.
        let file = build(Some(EditorPanel::Archivo), None);
        for act in [
            EditorAction::ClipLoad,
            EditorAction::ClipSave,
            EditorAction::MeshSave,
            EditorAction::Add,
            EditorAction::Save,
            EditorAction::Load,
            EditorAction::Back,
        ] {
            assert!(has(&file, act), "{act:?} debe verse en ARCHIVO");
        }

        // ANIMAR sin clip ni con clip: mismos controles, distinto readout.
        // OJO: `HudMesh` no guarda el texto (son quads de píxeles de glifo), así
        // que aquí solo se assertan hit regions; el texto del readout se cubre
        // en `editor_clip::tests::a_clip_imports_and_summarises`.
        let none = build(Some(EditorPanel::Animacion), None);
        assert!(!has(&none, EditorAction::ClipLoad), "importar es de ARCHIVO");
        assert!(!has(&none, EditorAction::StateAdd));
        let with = build(Some(EditorPanel::Animacion), Some(&clip));
        assert!(!has(&with, EditorAction::PosXInc));
        // Con clip el panel dibuja más geometría (resumen + timeline).
        assert!(
            with.vertices.len() > none.vertices.len(),
            "el readout del clip debería dibujar más: {} vs {}",
            with.vertices.len(),
            none.vertices.len()
        );
    }

    /// El timeline es un widget NUEVO (eje temporal horizontal), no la lista
    /// vertical de entidades: un hit por keyframe, con ranuras iguales, y los
    /// cursores de keyframe y articulación siempre alcanzables.
    #[test]
    fn editor_timeline_gives_every_keyframe_its_own_hit() {
        let entities = [crate::editor::EditorEntity::new("prop", [0.0, 24.0, 0.0])];
        let clip_json = r#"{
            "id": "ia", "model": "assets/entities/hero.json",
            "duration_s": 0.4, "loop": false,
            "frames": [
                { "t": 0.0, "pose": {} }, { "t": 0.2, "pose": {} }, { "t": 0.4, "pose": {} }
            ]
        }"#;
        let clip = crate::editor_clip::EditorClip::from_json(clip_json, None).expect("clip");
        let n = clip.file.frames.len();
        let mesh = build_editor_hud(
            "anim",
            "",
            "",
            0.0,
            &entities,
            EditorView {
                selected: 0,
                panel: Some(EditorPanel::Animacion),
                kf_sel: 1,
                joint_sel: 3,
                editing_joint: true,
                ..Default::default()
            },
            Some(&clip),
            &[],
            1280.0,
            720.0,
        );
        let has = |a: EditorAction| mesh.hits.iter().any(|h| h.action == HudAction::Ed(a));
        // Un hit por keyframe, y solo esos tres.
        for i in 0..n {
            assert!(has(EditorAction::KeyframeSel(i)), "falta el hit del clave {i}");
        }
        assert!(!has(EditorAction::KeyframeSel(n)));
        // Los cursores de los niveles 3 y 4.
        for a in [
            EditorAction::KeyframePrev,
            EditorAction::KeyframeNext,
            EditorAction::KeyframeTimeDec,
            EditorAction::KeyframeTimeInc,
            EditorAction::JointPrev,
            EditorAction::JointNext,
            EditorAction::EditEntity,
            EditorAction::EditJoint,
        ] {
            assert!(has(a), "{a:?} debe ser alcanzable en ANIMAR");
        }
        // El subconjunto de steppers: el eje de `l_arm_x` (joint_sel 3) es X.
        assert!(has(EditorAction::RotXDec) && has(EditorAction::RotXInc));
        // Y no aparecen los canales que no tienen destino en una articulación.
        for dead in [
            EditorAction::PosXInc,
            EditorAction::SclXInc,
            EditorAction::SizeInc,
            EditorAction::SkewInc,
        ] {
            assert!(!has(dead), "{dead:?} no debe salir en ANIMAR");
        }
        // Con `editing_joint` apagado los steppers siguen siendo hit (mismo
        // panel), pero el target lo decide el boton, no la vista.
        assert!(has(EditorAction::EditEntity) && has(EditorAction::EditJoint));
    }

    /// Sin clip, el panel ANIMAR lo dice y no inventa keyframes.
    #[test]
    fn editor_timeline_without_a_clip_has_no_keyframe_hits() {
        let entities = [crate::editor::EditorEntity::new("prop", [0.0, 24.0, 0.0])];
        let mesh = build_editor_hud(
            "anim",
            "",
            "",
            0.0,
            &entities,
            EditorView {
                selected: 0,
                panel: Some(EditorPanel::Animacion),
                ..Default::default()
            },
            None,
            &[],
            1280.0,
            720.0,
        );
        let has = |a: EditorAction| mesh.hits.iter().any(|h| h.action == HudAction::Ed(a));
        assert!(!has(EditorAction::KeyframeSel(0)));
        assert!(!has(EditorAction::KeyframeNext));
        // Pero el panel sigue siendo alcanzable y la escena se sigue viendo.
        assert!(has(EditorAction::SelNext));
        assert!(has(EditorAction::ClosePanel));
    }
}
