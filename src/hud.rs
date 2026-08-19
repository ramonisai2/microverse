//! On-screen stair / tunnel controls (mouse hits UI, not the world).
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
const ITEM_ATLAS_ROW: u32 = 4;
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
}

/// What an inventory / hotbar slot holds (icons for now; gameplay hooks later).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HotbarItem {
    Empty,
    Sword,
    Pickaxe,
    Axe,
}

impl HotbarItem {
    /// Atlas column on row 3 (hotbar icons).
    fn atlas_col(self) -> Option<u32> {
        match self {
            Self::Empty => None,
            Self::Sword => Some(0),
            Self::Pickaxe => Some(1),
            Self::Axe => Some(2),
        }
    }
}

/// Player hotbar: 4 slots + optional equipped slot (`None` = hands empty / stored).
#[derive(Clone, Debug)]
pub struct Hotbar {
    pub slots: [HotbarItem; HOTBAR_SLOTS],
    /// Currently drawn / held item. Press the same key again to store it.
    pub selected: Option<usize>,
}

impl Default for Hotbar {
    fn default() -> Self {
        Self {
            slots: [
                HotbarItem::Sword,
                HotbarItem::Pickaxe,
                HotbarItem::Axe,
                HotbarItem::Empty,
            ],
            // Start with pickaxe in hand.
            selected: Some(1),
        }
    }
}

impl Hotbar {
    /// Select slot, or unequip if that slot is already equipped.
    pub fn press_slot(&mut self, index: usize) {
        if index >= HOTBAR_SLOTS {
            return;
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

    pub fn select(&mut self, index: usize) {
        self.press_slot(index);
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

    // Row 3: hotbar — snapshot of the real held mesh (same orient/grip/scale).
    blit_held_tool_snapshot(
        &mut img,
        0,
        3,
        crate::entity_model::sword_model(),
        &crate::items::spawn_special1_sword().def.attach,
        crate::hero::ToolIconBake::default(),
    );
    blit_held_tool_snapshot(
        &mut img,
        1,
        3,
        crate::entity_model::pickaxe_model(),
        &crate::items::spawn_wooden_pickaxe().def.attach,
        crate::hero::ToolIconBake {
            roll_deg: 90.0,
            size_mul: 0.9,
        },
    );
    blit_axe(&mut img, 2, 3);

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
    let (uv0, uv1) = atlas_uv(stack.kind.atlas_col(), ITEM_ATLAS_ROW, 1);
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

    #[test]
    fn atlas_nonempty() {
        let img = build_hud_atlas();
        assert_eq!(img.width(), HUD_TILE * ATLAS_COLS);
        assert!(img.pixels().any(|p| p.0[3] > 0));
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
        hb.press_slot(1);
        assert_eq!(hb.selected, None);
        assert!(!hb.holding_pickaxe());
        hb.press_slot(1);
        assert!(hb.holding_pickaxe());
        hb.press_slot(0);
        assert_eq!(hb.selected_item(), HotbarItem::Sword);
        assert!(hb.holding_sword());
        assert!(!hb.holding_pickaxe());
        hb.press_slot(0);
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
}
