//! Melee damage table (placeholder values — refine later).
use crate::hud::HotbarItem;
use crate::items::ToolId;

/// Provisional combat damage. Special weapons hit hardest.
pub fn melee_damage_for_item(item: HotbarItem) -> u32 {
    match item {
        HotbarItem::Empty => 1, // bare fists
        HotbarItem::Pickaxe => 1,
        HotbarItem::Axe => 1,
        // Hotbar sword slot currently holds the special blade.
        HotbarItem::Sword => 20,
        // Secondary hand / non-weapon slots swing like bare hands.
        HotbarItem::Shield => 1,
        HotbarItem::Consumable => 1,
        HotbarItem::Magic => 1,
    }
}

pub fn melee_damage_for_tool(id: ToolId) -> u32 {
    match id {
        ToolId::WoodenPickaxe => 1,
        ToolId::AxeStub => 1,
        ToolId::Special1Sword => 20,
    }
}

/// Future sword tiers (not all equippable yet).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwordTier {
    Wood,
    Stone,
    Iron,
    Special,
}

impl SwordTier {
    pub fn damage(self) -> u32 {
        match self {
            Self::Wood => 3,
            Self::Stone => 6,
            Self::Iron => 9,
            Self::Special => 20,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_table_matches_design() {
        assert_eq!(SwordTier::Wood.damage(), 3);
        assert_eq!(SwordTier::Stone.damage(), 6);
        assert_eq!(SwordTier::Iron.damage(), 9);
        assert_eq!(SwordTier::Special.damage(), 20);
        assert_eq!(melee_damage_for_item(HotbarItem::Pickaxe), 1);
        assert_eq!(melee_damage_for_item(HotbarItem::Sword), 20);
        assert_eq!(melee_damage_for_item(HotbarItem::Empty), 1);
    }
}
