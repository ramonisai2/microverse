//! Minecraft-style player storage: 9×3 slots + UI cursor stack.
use crate::world::{
    EmbedKind, FragmentKind, OreDrop, BLOCK_FRAGMENTS_PER_BREAK, MAX_BLOCK_FRAGMENTS,
    ORE_MICROS_PER_CUBE,
};

pub const INV_COLS: usize = 9;
pub const INV_ROWS: usize = 3;
pub const INV_STORAGE: usize = INV_COLS * INV_ROWS; // 27
pub const STACK_MAX_DEFAULT: u32 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvItem {
    DirtFrag,
    StoneFrag,
    CoalMicro,
    SapphireMicro,
    RubyMicro,
    EmeraldMicro,
    CoalCube,
    SapphireCube,
    RubyCube,
    EmeraldCube,
}

impl InvItem {
    pub fn label(self) -> &'static str {
        match self {
            Self::DirtFrag => "tierra",
            Self::StoneFrag => "piedra",
            Self::CoalMicro => "carbón",
            Self::SapphireMicro => "zafiro",
            Self::RubyMicro => "rubí",
            Self::EmeraldMicro => "esmeralda",
            Self::CoalCube => "cubo carbón",
            Self::SapphireCube => "cubo zafiro",
            Self::RubyCube => "cubo rubí",
            Self::EmeraldCube => "cubo esmeralda",
        }
    }

    pub fn max_stack(self) -> u32 {
        match self {
            Self::DirtFrag | Self::StoneFrag => MAX_BLOCK_FRAGMENTS,
            _ => STACK_MAX_DEFAULT,
        }
    }

    /// Atlas column on the item row (row 4).
    pub fn atlas_col(self) -> u32 {
        match self {
            Self::DirtFrag => 0,
            Self::StoneFrag => 1,
            Self::CoalMicro | Self::CoalCube => 2,
            Self::SapphireMicro | Self::SapphireCube => 3,
            Self::RubyMicro | Self::RubyCube => 4,
            Self::EmeraldMicro | Self::EmeraldCube => 5,
        }
    }

    pub fn is_cube(self) -> bool {
        matches!(
            self,
            Self::CoalCube | Self::SapphireCube | Self::RubyCube | Self::EmeraldCube
        )
    }

    pub fn micro_for_embed(kind: EmbedKind) -> Self {
        match kind {
            EmbedKind::Coal => Self::CoalMicro,
            EmbedKind::Sapphire => Self::SapphireMicro,
            EmbedKind::Ruby => Self::RubyMicro,
            EmbedKind::Emerald => Self::EmeraldMicro,
        }
    }

    pub fn cube_for_embed(kind: EmbedKind) -> Self {
        match kind {
            EmbedKind::Coal => Self::CoalCube,
            EmbedKind::Sapphire => Self::SapphireCube,
            EmbedKind::Ruby => Self::RubyCube,
            EmbedKind::Emerald => Self::EmeraldCube,
        }
    }

    pub fn cube_pair(self) -> Option<(EmbedKind, Self)> {
        match self {
            Self::CoalMicro => Some((EmbedKind::Coal, Self::CoalCube)),
            Self::SapphireMicro => Some((EmbedKind::Sapphire, Self::SapphireCube)),
            Self::RubyMicro => Some((EmbedKind::Ruby, Self::RubyCube)),
            Self::EmeraldMicro => Some((EmbedKind::Emerald, Self::EmeraldCube)),
            _ => None,
        }
    }

    pub fn from_fragment(kind: FragmentKind) -> Self {
        match kind {
            FragmentKind::Dirt => Self::DirtFrag,
            FragmentKind::Stone => Self::StoneFrag,
        }
    }

    pub fn tint_rgb(self) -> [f32; 3] {
        match self {
            Self::DirtFrag => [0.52, 0.38, 0.24],
            Self::StoneFrag => [0.66, 0.68, 0.72],
            Self::CoalMicro | Self::CoalCube => [0.12, 0.10, 0.09],
            Self::SapphireMicro | Self::SapphireCube => [0.22, 0.42, 0.92],
            Self::RubyMicro | Self::RubyCube => [0.88, 0.18, 0.22],
            Self::EmeraldMicro | Self::EmeraldCube => [0.18, 0.78, 0.38],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemStack {
    pub kind: InvItem,
    pub count: u32,
}

impl ItemStack {
    pub fn new(kind: InvItem, count: u32) -> Option<Self> {
        if count == 0 {
            return None;
        }
        Some(Self {
            kind,
            count: count.min(kind.max_stack()),
        })
    }

    pub fn room(self) -> u32 {
        self.kind.max_stack().saturating_sub(self.count)
    }
}

#[derive(Clone, Debug)]
pub struct PlayerInventory {
    pub storage: [Option<ItemStack>; INV_STORAGE],
    /// Stack held by the UI cursor while the inventory is open.
    pub cursor: Option<ItemStack>,
}

impl Default for PlayerInventory {
    fn default() -> Self {
        Self {
            storage: [None; INV_STORAGE],
            cursor: None,
        }
    }
}

impl PlayerInventory {
    /// Global cap for dirt/stone fragments (player may only hold this many total).
    fn global_room(&self, kind: InvItem) -> u32 {
        match kind {
            InvItem::DirtFrag | InvItem::StoneFrag => {
                MAX_BLOCK_FRAGMENTS.saturating_sub(self.count_of(kind))
            }
            _ => u32::MAX,
        }
    }

    /// Insert as many as possible; returns how many were stored.
    pub fn try_add(&mut self, kind: InvItem, mut amount: u32) -> u32 {
        if amount == 0 {
            return 0;
        }
        amount = amount.min(self.global_room(kind));
        if amount == 0 {
            return 0;
        }
        let mut added = 0u32;
        let max = kind.max_stack();

        // Fill partial stacks first.
        for slot in self.storage.iter_mut() {
            if amount == 0 {
                break;
            }
            if let Some(stack) = slot {
                if stack.kind != kind {
                    continue;
                }
                let room = max.saturating_sub(stack.count);
                if room == 0 {
                    continue;
                }
                let n = amount.min(room);
                stack.count += n;
                amount -= n;
                added += n;
            }
        }
        // Then empty slots.
        for slot in self.storage.iter_mut() {
            if amount == 0 {
                break;
            }
            if slot.is_some() {
                continue;
            }
            let n = amount.min(max);
            *slot = Some(ItemStack { kind, count: n });
            amount -= n;
            added += n;
        }

        if added > 0 {
            self.craft_micros_to_cubes();
        }
        added
    }

    pub fn add_fragments(&mut self, kind: FragmentKind, amount: u32) -> u32 {
        self.try_add(InvItem::from_fragment(kind), amount)
    }

    /// Add ore flecks; auto-craft 9 micros → 1 cube into storage.
    pub fn add_ore_drop(&mut self, drop: OreDrop) -> u32 {
        let kind = InvItem::micro_for_embed(drop.kind);
        let added = self.try_add(kind, drop.micros as u32);
        added
    }

    /// Convert every 9 micros in storage into a cube (may spill to new slots).
    pub fn craft_micros_to_cubes(&mut self) {
        loop {
            let mut progressed = false;
            for i in 0..INV_STORAGE {
                let Some(stack) = self.storage[i] else {
                    continue;
                };
                let Some((_embed, cube_kind)) = stack.kind.cube_pair() else {
                    continue;
                };
                if stack.count < ORE_MICROS_PER_CUBE {
                    continue;
                }
                let crafts = stack.count / ORE_MICROS_PER_CUBE;
                let remain = stack.count % ORE_MICROS_PER_CUBE;
                if remain == 0 {
                    self.storage[i] = None;
                } else {
                    self.storage[i] = Some(ItemStack {
                        kind: stack.kind,
                        count: remain,
                    });
                }
                // Place cubes without re-entering craft recursion via try_add's craft call.
                let mut left = crafts;
                for slot in self.storage.iter_mut() {
                    if left == 0 {
                        break;
                    }
                    if let Some(s) = slot {
                        if s.kind == cube_kind {
                            let room = cube_kind.max_stack().saturating_sub(s.count);
                            let n = left.min(room);
                            s.count += n;
                            left -= n;
                        }
                    }
                }
                for slot in self.storage.iter_mut() {
                    if left == 0 {
                        break;
                    }
                    if slot.is_none() {
                        let n = left.min(cube_kind.max_stack());
                        *slot = Some(ItemStack {
                            kind: cube_kind,
                            count: n,
                        });
                        left -= n;
                    }
                }
                // If cubes didn't fit, put leftover micros back (shouldn't happen often).
                if left > 0 {
                    let micros_back = left * ORE_MICROS_PER_CUBE;
                    let _ = self.force_add_no_craft(stack.kind, micros_back);
                }
                progressed = true;
                break;
            }
            if !progressed {
                break;
            }
        }
    }

    fn force_add_no_craft(&mut self, kind: InvItem, mut amount: u32) -> u32 {
        let mut added = 0u32;
        let max = kind.max_stack();
        for slot in self.storage.iter_mut() {
            if amount == 0 {
                break;
            }
            if let Some(stack) = slot {
                if stack.kind != kind {
                    continue;
                }
                let room = max.saturating_sub(stack.count);
                let n = amount.min(room);
                stack.count += n;
                amount -= n;
                added += n;
            }
        }
        for slot in self.storage.iter_mut() {
            if amount == 0 {
                break;
            }
            if slot.is_none() {
                let n = amount.min(max);
                *slot = Some(ItemStack { kind, count: n });
                amount -= n;
                added += n;
            }
        }
        added
    }

    pub fn count_of(&self, kind: InvItem) -> u32 {
        self.storage
            .iter()
            .filter_map(|s| *s)
            .filter(|s| s.kind == kind)
            .map(|s| s.count)
            .sum::<u32>()
            + self
                .cursor
                .filter(|s| s.kind == kind)
                .map(|s| s.count)
                .unwrap_or(0)
    }

    /// Left-click: pick up / place / merge / swap. Right-click: place one or take half.
    pub fn click_slot(&mut self, index: usize, right: bool) {
        if index >= INV_STORAGE {
            return;
        }
        if right {
            self.click_slot_right(index);
        } else {
            self.click_slot_left(index);
        }
        self.craft_micros_to_cubes();
    }

    fn click_slot_left(&mut self, index: usize) {
        match (self.cursor, self.storage[index]) {
            (None, None) => {}
            (None, Some(stack)) => {
                self.cursor = Some(stack);
                self.storage[index] = None;
            }
            (Some(held), None) => {
                self.storage[index] = Some(held);
                self.cursor = None;
            }
            (Some(held), Some(slot)) if held.kind == slot.kind => {
                let room = slot.kind.max_stack().saturating_sub(slot.count);
                if room == 0 {
                    // Swap when full.
                    self.storage[index] = Some(held);
                    self.cursor = Some(slot);
                    return;
                }
                let n = held.count.min(room);
                self.storage[index] = Some(ItemStack {
                    kind: slot.kind,
                    count: slot.count + n,
                });
                let left = held.count - n;
                self.cursor = if left == 0 {
                    None
                } else {
                    Some(ItemStack {
                        kind: held.kind,
                        count: left,
                    })
                };
            }
            (Some(held), Some(slot)) => {
                self.storage[index] = Some(held);
                self.cursor = Some(slot);
            }
        }
    }

    fn click_slot_right(&mut self, index: usize) {
        match (self.cursor, self.storage[index]) {
            (None, None) => {}
            (None, Some(stack)) => {
                // Take half (ceil).
                let take = (stack.count + 1) / 2;
                let leave = stack.count - take;
                self.cursor = Some(ItemStack {
                    kind: stack.kind,
                    count: take,
                });
                self.storage[index] = if leave == 0 {
                    None
                } else {
                    Some(ItemStack {
                        kind: stack.kind,
                        count: leave,
                    })
                };
            }
            (Some(held), None) => {
                // Place one.
                self.storage[index] = Some(ItemStack {
                    kind: held.kind,
                    count: 1,
                });
                let left = held.count - 1;
                self.cursor = if left == 0 {
                    None
                } else {
                    Some(ItemStack {
                        kind: held.kind,
                        count: left,
                    })
                };
            }
            (Some(held), Some(slot)) if held.kind == slot.kind => {
                let room = slot.kind.max_stack().saturating_sub(slot.count);
                if room == 0 {
                    return;
                }
                self.storage[index] = Some(ItemStack {
                    kind: slot.kind,
                    count: slot.count + 1,
                });
                let left = held.count - 1;
                self.cursor = if left == 0 {
                    None
                } else {
                    Some(ItemStack {
                        kind: held.kind,
                        count: left,
                    })
                };
            }
            (Some(_held), Some(_slot)) => {
                // Different kinds: right-click does nothing (Minecraft places one only on empty/same).
            }
        }
    }

    /// Return cursor stack into storage (on close). Leftover stays on cursor if full.
    pub fn return_cursor(&mut self) {
        let Some(held) = self.cursor.take() else {
            return;
        };
        let added = self.try_add(held.kind, held.count);
        let left = held.count.saturating_sub(added);
        if left > 0 {
            self.cursor = Some(ItemStack {
                kind: held.kind,
                count: left,
            });
        }
    }

    pub fn summary_title(&self) -> (u32, u32, u32, u32, u32, u32) {
        (
            self.count_of(InvItem::StoneFrag),
            self.count_of(InvItem::DirtFrag),
            self.count_of(InvItem::CoalMicro),
            self.count_of(InvItem::CoalCube),
            self.count_of(InvItem::SapphireMicro) + self.count_of(InvItem::RubyMicro)
                + self.count_of(InvItem::EmeraldMicro),
            self.count_of(InvItem::SapphireCube)
                + self.count_of(InvItem::RubyCube)
                + self.count_of(InvItem::EmeraldCube),
        )
    }
}

/// Helper for drop wiring.
pub fn fragment_amount_default() -> u32 {
    BLOCK_FRAGMENTS_PER_BREAK
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::EmbedKind;

    #[test]
    fn try_add_caps_dirt_at_100() {
        let mut inv = PlayerInventory::default();
        assert_eq!(inv.try_add(InvItem::DirtFrag, 98), 98);
        assert_eq!(inv.try_add(InvItem::DirtFrag, 4), 2);
        assert_eq!(inv.count_of(InvItem::DirtFrag), 100);
        assert_eq!(inv.try_add(InvItem::DirtFrag, 4), 0);
    }

    #[test]
    fn left_click_pickup_place_merge_swap() {
        let mut inv = PlayerInventory::default();
        inv.storage[0] = ItemStack::new(InvItem::StoneFrag, 10);
        inv.click_slot(0, false);
        assert_eq!(inv.cursor, ItemStack::new(InvItem::StoneFrag, 10));
        assert!(inv.storage[0].is_none());

        inv.click_slot(1, false);
        assert!(inv.cursor.is_none());
        assert_eq!(inv.storage[1], ItemStack::new(InvItem::StoneFrag, 10));

        inv.storage[2] = ItemStack::new(InvItem::StoneFrag, 5);
        inv.cursor = ItemStack::new(InvItem::StoneFrag, 3);
        inv.click_slot(2, false);
        assert_eq!(inv.storage[2], ItemStack::new(InvItem::StoneFrag, 8));
        assert!(inv.cursor.is_none());

        inv.cursor = ItemStack::new(InvItem::DirtFrag, 2);
        inv.storage[3] = ItemStack::new(InvItem::StoneFrag, 1);
        inv.click_slot(3, false);
        assert_eq!(inv.storage[3], ItemStack::new(InvItem::DirtFrag, 2));
        assert_eq!(inv.cursor, ItemStack::new(InvItem::StoneFrag, 1));
    }

    #[test]
    fn right_click_split_and_place_one() {
        let mut inv = PlayerInventory::default();
        inv.storage[0] = ItemStack::new(InvItem::StoneFrag, 5);
        inv.click_slot(0, true);
        assert_eq!(inv.cursor, ItemStack::new(InvItem::StoneFrag, 3));
        assert_eq!(inv.storage[0], ItemStack::new(InvItem::StoneFrag, 2));

        inv.click_slot(1, true);
        assert_eq!(inv.storage[1], ItemStack::new(InvItem::StoneFrag, 1));
        assert_eq!(inv.cursor, ItemStack::new(InvItem::StoneFrag, 2));
    }

    #[test]
    fn ore_micros_craft_to_cube() {
        let mut inv = PlayerInventory::default();
        let added = inv.add_ore_drop(OreDrop {
            kind: EmbedKind::Sapphire,
            micros: 8,
        });
        assert_eq!(added, 8);
        assert_eq!(inv.count_of(InvItem::SapphireMicro), 8);
        let added = inv.add_ore_drop(OreDrop {
            kind: EmbedKind::Sapphire,
            micros: 2,
        });
        assert_eq!(added, 2);
        assert_eq!(inv.count_of(InvItem::SapphireMicro), 1);
        assert_eq!(inv.count_of(InvItem::SapphireCube), 1);
    }

    #[test]
    fn return_cursor_puts_back() {
        let mut inv = PlayerInventory::default();
        inv.cursor = ItemStack::new(InvItem::CoalMicro, 4);
        inv.return_cursor();
        assert!(inv.cursor.is_none());
        assert_eq!(inv.count_of(InvItem::CoalMicro), 4);
    }
}
