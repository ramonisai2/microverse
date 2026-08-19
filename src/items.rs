//! Tool / item definitions (stats under `assets/items/`; meshes under entities/items).
//!
//! [`ToolData`] / [`ToolAttach`] drive dig rates and hand pose; hotbar selects [`ToolId`].
use crate::world::Material;
use glam::Vec3;
use serde::Deserialize;
use std::path::PathBuf;
use std::time::Duration;

pub const WOODEN_PICKAXE_ITEM: &str = "assets/items/wooden_pickaxe.json";
pub const SPECIAL1_SWORD_ITEM: &str = "assets/items/special1_sword_item.json";

/// Equippable tool identity (hotbar / mining).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolId {
    WoodenPickaxe,
    Special1Sword,
    AxeStub,
}

/// Harvest tier for pickaxes (wood < stone < iron < …).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PickTier {
    Wood = 0,
    Stone = 1,
    Iron = 2,
    Diamond = 3,
}

impl Default for PickTier {
    fn default() -> Self {
        Self::Wood
    }
}

/// Minimum pickaxe tier to break `material`. `None` = unbreakable.
pub fn required_pick_tier(material: Material) -> Option<PickTier> {
    match material {
        Material::Bedrock => None,
        Material::BlackStone => Some(PickTier::Iron),
        _ => Some(PickTier::Wood),
    }
}

/// Bare hands match wood-tier materials only (no blackstone / bedrock).
pub fn bare_hand_can_mine(material: Material) -> bool {
    matches!(required_pick_tier(material), Some(PickTier::Wood))
}

/// Hold-to-break without a dig tool takes this many × wooden-pick times.
pub const BARE_HAND_DIG_MULT: u64 = 10;

/// Wooden-pick baseline timings (from `wooden_pickaxe.json`) × [`BARE_HAND_DIG_MULT`].
pub fn bare_hand_dig_interval(material: Material) -> Duration {
    const DIRT_MS: u64 = 160;
    const STONE_MS: u64 = 640;
    let ms = match material {
        Material::Bedrock => u64::MAX / 4,
        Material::BlackStone => STONE_MS.saturating_mul(4).saturating_mul(BARE_HAND_DIG_MULT),
        Material::Stone
        | Material::VillageStone
        | Material::Cobblestone
        | Material::Coal
        | Material::Sapphire
        | Material::Ruby
        | Material::Emerald => STONE_MS.saturating_mul(BARE_HAND_DIG_MULT),
        _ => DIRT_MS.saturating_mul(BARE_HAND_DIG_MULT),
    };
    Duration::from_millis(ms.max(1))
}

impl ToolId {
    pub fn from_hotbar_item(item: crate::hud::HotbarItem) -> Option<Self> {
        match item {
            crate::hud::HotbarItem::Pickaxe => Some(Self::WoodenPickaxe),
            crate::hud::HotbarItem::Sword => Some(Self::Special1Sword),
            crate::hud::HotbarItem::Axe => Some(Self::AxeStub),
            crate::hud::HotbarItem::Empty => None,
        }
    }

    pub fn can_dig(self) -> bool {
        matches!(self, Self::WoodenPickaxe)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolOrientKind {
    Pickaxe,
    Sword,
    Upright,
}

impl Default for ToolOrientKind {
    fn default() -> Self {
        Self::Upright
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct DigCosts {
    pub dirt_cost: u32,
    pub stone_cost: u32,
    pub dirt_ms: u64,
    pub stone_ms: u64,
}

/// Hand / forearm attach for third-person held mesh.
#[derive(Clone, Debug, Deserialize)]
pub struct ToolAttach {
    #[serde(default = "default_scale")]
    pub scale: f32,
    #[serde(default = "default_wrist")]
    pub wrist: [f32; 3],
    #[serde(default = "default_grip")]
    pub grip: [f32; 3],
    #[serde(default)]
    pub orient: ToolOrientKind,
    #[serde(default = "default_swing_arc")]
    pub swing_arc_deg: f32,
    #[serde(default = "default_swing_speed")]
    pub swing_speed: f32,
}

fn default_scale() -> f32 {
    1.1
}
fn default_wrist() -> [f32; 3] {
    [0.25, -3.9, 0.35]
}
fn default_grip() -> [f32; 3] {
    [0.0, 2.0, 0.0]
}
fn default_swing_arc() -> f32 {
    45.0
}
fn default_swing_speed() -> f32 {
    1.0
}

impl Default for ToolAttach {
    fn default() -> Self {
        Self {
            scale: default_scale(),
            wrist: default_wrist(),
            grip: default_grip(),
            orient: ToolOrientKind::Pickaxe,
            swing_arc_deg: default_swing_arc(),
            swing_speed: default_swing_speed(),
        }
    }
}

impl ToolAttach {
    pub fn wrist_vec(&self) -> Vec3 {
        Vec3::from_array(self.wrist)
    }
    pub fn grip_vec(&self) -> Vec3 {
        Vec3::from_array(self.grip)
    }
}

/// Full tool definition: dig stats + hand attach (alias of loaded JSON).
#[allow(dead_code)]
pub type ToolData = ToolDef;

#[derive(Clone, Debug, Deserialize)]
pub struct ToolDef {
    pub id: String,
    pub name: String,
    pub model: String,
    pub max_durability: u32,
    pub dig: DigCosts,
    #[serde(default)]
    pub tier: PickTier,
    #[serde(default)]
    pub attach: ToolAttach,
}

impl ToolDef {
    pub fn can_mine(&self, material: Material) -> bool {
        match required_pick_tier(material) {
            None => false,
            Some(need) => self.tier >= need,
        }
    }

    pub fn durability_cost(&self, material: Material) -> u32 {
        match material {
            Material::Bedrock => u32::MAX, // never affordable
            Material::BlackStone => self.dig.stone_cost.saturating_mul(4).max(1),
            Material::Stone
            | Material::VillageStone
            | Material::Cobblestone
            | Material::Coal
            | Material::Sapphire
            | Material::Ruby
            | Material::Emerald => self.dig.stone_cost.max(1),
            Material::Dirt | Material::Grass | Material::Sand => self.dig.dirt_cost.max(1),
            Material::Wood
            | Material::WoodPlanks
            | Material::Leaves
            | Material::BirchWood
            | Material::BirchLeaves
            | Material::PineWood
            | Material::PineLeaves
            | Material::WillowWood
            | Material::WillowLeaves
            | Material::MesquiteWood
            | Material::MesquiteLeaves
            | Material::CeibaWood
            | Material::CeibaLeaves
            | Material::EnchantedWood
            | Material::EnchantedLeaves
            | Material::UmbraWood
            | Material::UmbraLeaves
            | Material::Chest
            | Material::Glass
            | Material::Door
            | Material::Water => {
                self.dig.dirt_cost.max(1)
            }
        }
    }

    pub fn dig_interval(&self, material: Material) -> Duration {
        let ms = match material {
            Material::Bedrock => u64::MAX / 4, // effectively never finishes
            Material::BlackStone => self.dig.stone_ms.saturating_mul(4),
            Material::Stone
            | Material::VillageStone
            | Material::Cobblestone
            | Material::Coal
            | Material::Sapphire
            | Material::Ruby
            | Material::Emerald => self.dig.stone_ms,
            _ => self.dig.dirt_ms,
        };
        Duration::from_millis(ms.max(1))
    }
}

#[derive(Clone, Debug)]
pub struct ToolInstance {
    pub def: ToolDef,
    pub durability: u32,
}

impl ToolInstance {
    pub fn from_def(def: ToolDef) -> Self {
        let durability = def.max_durability;
        Self { def, durability }
    }

    pub fn is_broken(&self) -> bool {
        self.durability == 0
    }

    pub fn try_wear(&mut self, material: Material) -> bool {
        let cost = self.def.durability_cost(material);
        if self.durability < cost {
            return false;
        }
        self.durability -= cost;
        true
    }

    pub fn hud_tag(&self) -> String {
        format!(
            "{} {}/{}",
            self.def.name, self.durability, self.def.max_durability
        )
    }
}

fn item_search_paths(rel: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    paths.push(PathBuf::from(rel));
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(rel));
        for dir in [cwd.as_path(), cwd.parent().unwrap_or(cwd.as_path())] {
            paths.push(dir.join(rel));
            paths.push(dir.join("microvoxel").join(rel));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join(rel));
            paths.push(dir.join("../").join(rel));
            paths.push(dir.join("../../").join(rel));
        }
    }
    paths
}

pub fn load_tool_def_json(json: &str) -> Result<ToolDef, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}

pub fn load_tool_def(rel_path: &str) -> ToolDef {
    for p in item_search_paths(rel_path) {
        if let Ok(text) = std::fs::read_to_string(&p) {
            match load_tool_def_json(&text) {
                Ok(def) => {
                    log::info!("items: loaded {}", p.display());
                    return def;
                }
                Err(e) => log::warn!("items: bad JSON {}: {e}", p.display()),
            }
        }
    }
    match rel_path {
        SPECIAL1_SWORD_ITEM => {
            let embedded = include_str!("../assets/items/special1_sword_item.json");
            load_tool_def_json(embedded).expect("embedded special1_sword_item.json")
        }
        _ => {
            let embedded = include_str!("../assets/items/wooden_pickaxe.json");
            load_tool_def_json(embedded).expect("embedded wooden_pickaxe.json")
        }
    }
}

pub fn spawn_wooden_pickaxe() -> ToolInstance {
    ToolInstance::from_def(load_tool_def(WOODEN_PICKAXE_ITEM))
}

pub fn spawn_special1_sword() -> ToolInstance {
    ToolInstance::from_def(load_tool_def(SPECIAL1_SWORD_ITEM))
}

pub fn spawn_tool(id: ToolId) -> Option<ToolInstance> {
    match id {
        ToolId::WoodenPickaxe => Some(spawn_wooden_pickaxe()),
        ToolId::Special1Sword => Some(spawn_special1_sword()),
        ToolId::AxeStub => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wooden_pick_stats() {
        let t = spawn_wooden_pickaxe();
        assert_eq!(t.def.max_durability, 60);
        assert_eq!(t.def.attach.orient, ToolOrientKind::Pickaxe);
        assert!((t.def.attach.scale - 1.1).abs() < 1e-3);
        assert_eq!(t.def.durability_cost(Material::Stone), 4);
        assert_eq!(t.def.durability_cost(Material::BlackStone), 16);
        assert_eq!(t.def.tier, PickTier::Wood);
        assert!(t.def.can_mine(Material::Stone));
        assert!(!t.def.can_mine(Material::BlackStone));
        assert!(!t.def.can_mine(Material::Bedrock));
        assert!(t.def.dig_interval(Material::BlackStone) > t.def.dig_interval(Material::Stone));
    }

    #[test]
    fn bare_hand_is_ten_times_slower() {
        assert!(bare_hand_can_mine(Material::Dirt));
        assert!(bare_hand_can_mine(Material::Stone));
        assert!(!bare_hand_can_mine(Material::BlackStone));
        assert!(!bare_hand_can_mine(Material::Bedrock));
        let pick = spawn_wooden_pickaxe();
        assert_eq!(
            bare_hand_dig_interval(Material::Dirt).as_millis(),
            pick.def.dig_interval(Material::Dirt).as_millis() * BARE_HAND_DIG_MULT as u128
        );
        assert_eq!(
            bare_hand_dig_interval(Material::Stone).as_millis(),
            pick.def.dig_interval(Material::Stone).as_millis() * BARE_HAND_DIG_MULT as u128
        );
    }

    #[test]
    fn sword_loads_attach() {
        let t = spawn_special1_sword();
        assert_eq!(t.def.attach.orient, ToolOrientKind::Sword);
        assert!(t.def.max_durability > 0);
    }

    #[test]
    fn wear_breaks_on_stone() {
        let mut t = spawn_wooden_pickaxe();
        t.durability = 3;
        assert!(!t.try_wear(Material::Stone));
        assert!(t.try_wear(Material::Dirt));
        assert_eq!(t.durability, 2);
    }
}
