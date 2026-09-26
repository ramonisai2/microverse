//! JSON prefab loader (authored in `editor/prefab.html`).
//!
//! Cells are grid-aligned voxels stamped relative to an anchor. Rotation is
//! around +Y in 90° steps (`rotation_quarters`).

use crate::world::{Material, Voxel};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const HOUSE_BASIC_JSON: &str = include_str!("../assets/prefabs/house_basic.json");
const COTTAGE_JSON: &str = include_str!("../assets/prefabs/cottage.json");
const SHOP_GENERAL_JSON: &str = include_str!("../assets/prefabs/shop_general.json");
const BLACKSMITH_JSON: &str = include_str!("../assets/prefabs/blacksmith.json");
const PHARMACY_JSON: &str = include_str!("../assets/prefabs/pharmacy.json");
const RESTAURANT_JSON: &str = include_str!("../assets/prefabs/restaurant.json");
const WIZARD_TOWER_JSON: &str = include_str!("../assets/prefabs/wizard_tower.json");
const WATCHTOWER_JSON: &str = include_str!("../assets/prefabs/watchtower.json");
const MISSION_BOARD_JSON: &str = include_str!("../assets/prefabs/mission_board.json");
const TAVERN_JSON: &str = include_str!("../assets/prefabs/tavern.json");
const BARN_JSON: &str = include_str!("../assets/prefabs/barn.json");
const CHAPEL_JSON: &str = include_str!("../assets/prefabs/chapel.json");
const HERBALIST_JSON: &str = include_str!("../assets/prefabs/herbalist.json");
const STABLE_JSON: &str = include_str!("../assets/prefabs/stable.json");
const SAWMILL_JSON: &str = include_str!("../assets/prefabs/sawmill.json");
const LIBRARY_JSON: &str = include_str!("../assets/prefabs/library.json");
const BARRACKS_JSON: &str = include_str!("../assets/prefabs/barracks.json");
const LIGHTHOUSE_JSON: &str = include_str!("../assets/prefabs/lighthouse.json");
const MARKET_STALL_JSON: &str = include_str!("../assets/prefabs/market_stall.json");
const SMITH_STALL_JSON: &str = include_str!("../assets/prefabs/smith_stall.json");
const POTION_STALL_JSON: &str = include_str!("../assets/prefabs/potion_stall.json");
const BARTER_STALL_JSON: &str = include_str!("../assets/prefabs/barter_stall.json");
const CART_JSON: &str = include_str!("../assets/prefabs/cart.json");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefab {
    pub id: String,
    pub size: [i32; 3],
    pub anchor: [i32; 3],
    /// Local cells as (lx, ly, lz, material). Air is omitted.
    pub solids: Vec<(i32, i32, i32, Material)>,
}

#[derive(Deserialize)]
struct PrefabFile {
    size: [i32; 3],
    anchor: [i32; 3],
    palette: HashMap<String, String>,
    layers: Vec<Vec<String>>,
    #[serde(default)]
    id: String,
}

fn cache() -> &'static Mutex<HashMap<String, Prefab>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Prefab>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn material_from_name(name: &str) -> Option<Material> {
    match name {
        "air" | "." => None,
        "stone" => Some(Material::Stone),
        "village_stone" => Some(Material::VillageStone),
        "cobble" | "cobblestone" => Some(Material::Cobblestone),
        "road_cobble" | "road_cobblestone" => Some(Material::RoadCobble),
        "torch" | "torchflame" => Some(Material::Torch),
        "apple" | "apples" => Some(Material::Apple),
        "beehive" | "hive" => Some(Material::Beehive),
        "mana_crystal" | "mana" | "crystal" => Some(Material::ManaCrystal),
        "black_stone" | "blackstone" => Some(Material::BlackStone),
        "wood" | "wood_planks" | "wood_roof" => Some(Material::WoodPlanks),
        "wood_log" | "log" => Some(Material::Wood),
        "dirt" => Some(Material::Dirt),
        "glass" => Some(Material::Glass),
        "door" => Some(Material::Door),
        "water" => Some(Material::Water),
        "mud" => Some(Material::Mud),
        "thermal_water" => Some(Material::ThermalWater),
        "leaves" => Some(Material::Leaves),
        "sand" => Some(Material::Sand),
        "emerald" => Some(Material::Emerald),
        "sapphire" => Some(Material::Sapphire),
        "ruby" => Some(Material::Ruby),
        "coal" => Some(Material::Coal),
        other => {
            log::warn!("prefab: unknown material `{other}`, skipping");
            None
        }
    }
}

fn parse_prefab(text: &str) -> Result<Prefab, String> {
    let file: PrefabFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let [sx, sy, sz] = file.size;
    if sx <= 0 || sy <= 0 || sz <= 0 {
        return Err("prefab size must be positive".into());
    }
    if file.layers.len() != sy as usize {
        return Err(format!(
            "prefab layers {} != size.y {sy}",
            file.layers.len()
        ));
    }

    let mut char_mat: HashMap<char, Option<Material>> = HashMap::new();
    for (k, v) in &file.palette {
        let ch = k.chars().next().ok_or("empty palette key")?;
        char_mat.insert(ch, material_from_name(v));
    }
    if !char_mat.contains_key(&'.') {
        char_mat.insert('.', None);
    }

    let mut solids = Vec::new();
    for (ly, layer) in file.layers.iter().enumerate() {
        if layer.len() != sz as usize {
            return Err(format!("layer {ly}: rows {} != size.z {sz}", layer.len()));
        }
        for (lz, row) in layer.iter().enumerate() {
            let chars: Vec<char> = row.chars().collect();
            if chars.len() != sx as usize {
                return Err(format!(
                    "layer {ly} row {lz}: cols {} != size.x {sx}",
                    chars.len()
                ));
            }
            for (lx, ch) in chars.into_iter().enumerate() {
                let mat = match char_mat.get(&ch) {
                    Some(m) => *m,
                    None => material_from_name(&ch.to_string()),
                };
                if let Some(material) = mat {
                    solids.push((lx as i32, ly as i32, lz as i32, material));
                }
            }
        }
    }

    Ok(Prefab {
        id: if file.id.is_empty() {
            "unnamed".into()
        } else {
            file.id
        },
        size: file.size,
        anchor: file.anchor,
        solids,
    })
}

fn bundled_prefab_json(path: &str) -> Option<&'static str> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name {
        "house_basic.json" => Some(HOUSE_BASIC_JSON),
        "cottage.json" => Some(COTTAGE_JSON),
        "shop_general.json" => Some(SHOP_GENERAL_JSON),
        "blacksmith.json" => Some(BLACKSMITH_JSON),
        "pharmacy.json" => Some(PHARMACY_JSON),
        "restaurant.json" => Some(RESTAURANT_JSON),
        "wizard_tower.json" => Some(WIZARD_TOWER_JSON),
        "watchtower.json" => Some(WATCHTOWER_JSON),
        "mission_board.json" => Some(MISSION_BOARD_JSON),
        "tavern.json" => Some(TAVERN_JSON),
        "barn.json" => Some(BARN_JSON),
        "chapel.json" => Some(CHAPEL_JSON),
        "herbalist.json" => Some(HERBALIST_JSON),
        "stable.json" => Some(STABLE_JSON),
        "sawmill.json" => Some(SAWMILL_JSON),
        "library.json" => Some(LIBRARY_JSON),
        "barracks.json" => Some(BARRACKS_JSON),
        "lighthouse.json" => Some(LIGHTHOUSE_JSON),
        "market_stall.json" => Some(MARKET_STALL_JSON),
        "smith_stall.json" => Some(SMITH_STALL_JSON),
        "potion_stall.json" => Some(POTION_STALL_JSON),
        "barter_stall.json" => Some(BARTER_STALL_JSON),
        "cart.json" => Some(CART_JSON),
        _ => None,
    }
}

/// Load a prefab by path (cached). Bundled settlement prefabs use `include_str`.
pub fn load_prefab(path: &str) -> Result<Prefab, String> {
    {
        let guard = cache().lock().map_err(|e| e.to_string())?;
        if let Some(p) = guard.get(path) {
            return Ok(p.clone());
        }
    }

    let text = if let Some(bundled) = bundled_prefab_json(path) {
        bundled.to_string()
    } else {
        std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?
    };
    let prefab = parse_prefab(&text)?;
    if let Ok(mut guard) = cache().lock() {
        guard.insert(path.to_string(), prefab.clone());
    }
    Ok(prefab)
}

/// Rotate local (dx, dz) around the origin by `quarters` × 90° CW (looking down +Y).
#[inline]
pub fn rotate_xz(dx: i32, dz: i32, quarters: u8) -> (i32, i32) {
    match quarters & 3 {
        0 => (dx, dz),
        1 => (dz, -dx),
        2 => (-dx, -dz),
        _ => (-dz, dx),
    }
}

/// World-space solid cells for a prefab placed with its anchor at `(wx, wy, wz)`.
pub fn placed_solids(
    prefab: &Prefab,
    wx: i32,
    wy: i32,
    wz: i32,
    rotation_quarters: u8,
) -> Vec<(i32, i32, i32, Material)> {
    let [ax, ay, az] = prefab.anchor;
    prefab
        .solids
        .iter()
        .map(|&(lx, ly, lz, mat)| {
            let (rdx, rdz) = rotate_xz(lx - ax, lz - az, rotation_quarters);
            (wx + rdx, wy + (ly - ay), wz + rdz, mat)
        })
        .collect()
}

pub fn voxel_for_material(material: Material) -> Voxel {
    match material {
        Material::Dirt => Voxel::dirt(),
        Material::Stone => Voxel::stone(),
        Material::Wood
        | Material::WoodPlanks
        | Material::VillageStone
        | Material::Cobblestone
        | Material::RoadCobble
        | Material::Torch
        | Material::Apple
        | Material::Beehive
        | Material::ManaCrystal
        | Material::Glass
        | Material::Door
        | Material::Water => Voxel::solid(material),
        Material::Leaves => Voxel::leaves(),
        other => Voxel::solid(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settlements::DEFAULT_HOUSE_PREFAB;

    #[test]
    fn house_basic_loads_and_has_foundation() {
        let p = load_prefab(DEFAULT_HOUSE_PREFAB).expect("load");
        assert_eq!(p.size, [7, 5, 7]);
        assert_eq!(p.anchor, [3, 0, 3]);
        assert!(p
            .solids
            .iter()
            .any(|&(x, y, z, m)| { x == 0 && y == 0 && z == 0 && m == Material::VillageStone }));
        assert!(p
            .solids
            .iter()
            .any(|&(_, y, _, m)| y == 4 && m == Material::WoodPlanks));
        assert!(
            p.solids.iter().any(|&(_, _, _, m)| m == Material::Glass),
            "house_basic should include glass window cells"
        );
        assert!(
            p.solids.iter().any(|&(_, _, _, m)| m == Material::Door),
            "house_basic should include a door"
        );
    }

    #[test]
    fn bundled_settlement_prefabs_load() {
        for path in [
            "assets/prefabs/cottage.json",
            "assets/prefabs/shop_general.json",
            "assets/prefabs/blacksmith.json",
            "assets/prefabs/pharmacy.json",
            "assets/prefabs/restaurant.json",
            "assets/prefabs/wizard_tower.json",
        ] {
            let p = load_prefab(path).unwrap_or_else(|e| panic!("{path}: {e}"));
            assert!(
                p.solids.iter().any(|&(_, _, _, m)| m == Material::Door),
                "{path} should have a door"
            );
        }
    }

    #[test]
    fn rotate_xz_cycles() {
        assert_eq!(rotate_xz(2, 0, 0), (2, 0));
        assert_eq!(rotate_xz(2, 0, 1), (0, -2));
        assert_eq!(rotate_xz(2, 0, 2), (-2, 0));
        assert_eq!(rotate_xz(2, 0, 3), (0, 2));
        assert_eq!(rotate_xz(2, 0, 4), (2, 0));
    }

    #[test]
    fn placed_solids_honor_anchor_and_rotation() {
        let p = Prefab {
            id: "t".into(),
            size: [3, 1, 3],
            anchor: [1, 0, 1],
            solids: vec![(2, 0, 1, Material::Wood)], // +X from anchor
        };
        let cells = placed_solids(&p, 10, 5, 20, 1); // 90° CW → +X maps to -Z
        assert_eq!(cells, vec![(10, 5, 19, Material::Wood)]);
    }
}
