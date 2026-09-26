//! Slot save: seed + player + inventory + sparse voxel overlay.
//!
//! Procedural world is regenerated from [`crate::world::WORLD_SEED`]; only
//! player edits are stored. File: `saves/microverse.json`.
use crate::hud::{Hotbar, HotbarItem, HOTBAR_SLOTS};
use crate::inventory::{InvItem, ItemStack, PlayerInventory, INV_STORAGE};
use crate::world::{Material, World, WORLD_SEED};
use glam::{IVec3, Vec3};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const SAVE_REL: &str = "saves/microverse.json";
pub const AUTOSAVE_SECS: f32 = 45.0;

/// Directorio externo (Android: almacenamiento interno de la app).
/// Si está fijado, el guardado vive ahí en vez de junto al cwd.
static EXTERNAL_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Fija el directorio de guardado (llamar desde `android_main`).
pub fn set_external_dir(dir: PathBuf) {
    let _ = EXTERNAL_DIR.set(dir);
}

/// Directorio externo fijado, si lo hay. Lo usa el editor para guardar sus
/// escenas en el mismo sitio que la partida (en Android, dentro del
/// almacenamiento de la app).
pub fn external_dir() -> Option<PathBuf> {
    EXTERNAL_DIR.get().cloned()
}

#[derive(Clone, Serialize, Deserialize)]
struct SaveFile {
    version: u32,
    seed: u32,
    player: PlayerSave,
    pickaxe_durability: u32,
    hotbar: HotbarSave,
    inventory: Vec<SlotSave>,
    cursor: Option<SlotSave>,
    edits: Vec<EditSave>,
}

#[derive(Clone, Serialize, Deserialize)]
struct PlayerSave {
    feet: [f32; 3],
    facing: f32,
    hearts: u32,
    bottles: u32,
}

#[derive(Clone, Serialize, Deserialize)]
struct HotbarSave {
    slots: [HotbarItem; HOTBAR_SLOTS],
    selected: Option<usize>,
    #[serde(default)]
    combo_count: u32,
    #[serde(default)]
    shift_cooldown: u64,
}
#[derive(Clone, Serialize, Deserialize)]
struct SlotSave {
    slot: usize,
    kind: InvItem,
    count: u32,
}

#[derive(Clone, Serialize, Deserialize)]
struct EditSave {
    x: i32,
    y: i32,
    z: i32,
    /// `null` = air (dug).
    material: Option<Material>,
}

#[derive(Clone, Debug)]
pub struct LoadedSave {
    pub feet: Vec3,
    pub facing: f32,
    pub hearts: u32,
    pub bottles: u32,
    pub pickaxe_durability: u32,
    pub hotbar: Hotbar,
    pub inventory: PlayerInventory,
    pub edits: Vec<(IVec3, Option<Material>)>,
}

fn search_paths() -> Vec<PathBuf> {
    // Android primero (si está fijado).
    if let Some(dir) = EXTERNAL_DIR.get() {
        return vec![dir.join("microverse.json")];
    }
    let mut paths = Vec::new();
    paths.push(PathBuf::from(SAVE_REL));
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(SAVE_REL));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            paths.push(dir.join(SAVE_REL));
            paths.push(dir.join("../").join(SAVE_REL));
        }
    }
    paths
}

fn preferred_path() -> PathBuf {
    // Android primero (si está fijado).
    if let Some(dir) = EXTERNAL_DIR.get() {
        return dir.join("microverse.json");
    }
    if let Ok(cwd) = std::env::current_dir() {
        return cwd.join(SAVE_REL);
    }
    PathBuf::from(SAVE_REL)
}

fn inventory_to_slots(inv: &PlayerInventory) -> (Vec<SlotSave>, Option<SlotSave>) {
    let mut slots = Vec::new();
    for (i, stack) in inv.storage.iter().enumerate() {
        if let Some(s) = stack {
            slots.push(SlotSave {
                slot: i,
                kind: s.kind,
                count: s.count,
            });
        }
    }
    let cursor = inv.cursor.map(|s| SlotSave {
        slot: usize::MAX,
        kind: s.kind,
        count: s.count,
    });
    (slots, cursor)
}

fn inventory_from_slots(slots: &[SlotSave], cursor: Option<&SlotSave>) -> PlayerInventory {
    let mut inv = PlayerInventory::default();
    for s in slots {
        if s.slot >= INV_STORAGE || s.count == 0 {
            continue;
        }
        inv.storage[s.slot] = ItemStack::new(s.kind, s.count);
    }
    if let Some(c) = cursor {
        inv.cursor = ItemStack::new(c.kind, c.count);
    }
    inv
}

pub fn write_snapshot(
    world: &World,
    feet: Vec3,
    facing: f32,
    hearts: u32,
    bottles: u32,
    pickaxe_durability: u32,
    hotbar: &Hotbar,
    inventory: &PlayerInventory,
) -> Result<PathBuf, String> {
    let path = preferred_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let (inv_slots, cursor) = inventory_to_slots(inventory);
    let file = SaveFile {
        version: 1,
        seed: WORLD_SEED,
        player: PlayerSave {
            feet: [feet.x, feet.y, feet.z],
            facing,
            hearts,
            bottles,
        },
        pickaxe_durability,
        hotbar: HotbarSave {
            slots: hotbar.slots,
            selected: hotbar.selected,
            combo_count: hotbar.combo_count,
            shift_cooldown: hotbar.shift_cooldown,
        },
        inventory: inv_slots,
        cursor,
        edits: world
            .player_edits_snapshot()
            .into_iter()
            .map(|(p, material)| EditSave {
                x: p.x,
                y: p.y,
                z: p.z,
                material,
            })
            .collect(),
    };
    let json = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(path)
}

fn parse_save(text: &str, path: &Path) -> Result<LoadedSave, String> {
    let file: SaveFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if file.seed != WORLD_SEED {
        log::warn!(
            "save: seed {} != WORLD_SEED {:#x} ({}) — applying overlay anyway",
            file.seed,
            WORLD_SEED,
            path.display()
        );
    }
    let mut selected = file.hotbar.selected;
    if selected.is_some_and(|i| i >= HOTBAR_SLOTS) {
        selected = None;
    }
    Ok(LoadedSave {
        feet: Vec3::from_array(file.player.feet),
        facing: file.player.facing,
        hearts: file.player.hearts,
        bottles: file.player.bottles,
        pickaxe_durability: file.pickaxe_durability,
hotbar: Hotbar {
                slots: file.hotbar.slots,
                selected,
                combo_count: file.hotbar.combo_count,
                shift_cooldown: file.hotbar.shift_cooldown,
            },
        inventory: inventory_from_slots(&file.inventory, file.cursor.as_ref()),
        edits: file
            .edits
            .into_iter()
            .map(|e| (IVec3::new(e.x, e.y, e.z), e.material))
            .collect(),
    })
}

/// ¿Hay alguna partida guardada? Solo mira si el archivo existe (no lo
/// parsea): el menú atenúa la fila "partida guardada" sin gastarse un
/// `try_load` en cada frame.
pub fn exists() -> bool {
    search_paths().iter().any(|p| p.is_file())
}

pub fn try_load() -> Option<LoadedSave> {
    for path in search_paths() {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        match parse_save(&text, &path) {
            Ok(loaded) => {
                log::info!(
                    "save: loaded {} ({} edits)",
                    path.display(),
                    loaded.edits.len()
                );
                return Some(loaded);
            }
            Err(e) => log::warn!("save: bad file {}: {e}", path.display()),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Voxel;

    #[test]
    fn json_roundtrip_player_and_edit() {
        let file = SaveFile {
            version: 1,
            seed: WORLD_SEED,
            player: PlayerSave {
                feet: [8.5, 12.0, 24.5],
                facing: 1.25,
                hearts: 3,
                bottles: 1,
            },
            pickaxe_durability: 40,
            hotbar: HotbarSave {
                slots: [
                    HotbarItem::Sword,
                    HotbarItem::Pickaxe,
                    HotbarItem::Axe,
                    HotbarItem::Empty,
                ],
                selected: Some(1),
                combo_count: 0,
                shift_cooldown: 0,
            },
            inventory: vec![SlotSave {
                slot: 0,
                kind: InvItem::DirtFrag,
                count: 12,
            }],
            cursor: None,
            edits: vec![
                EditSave {
                    x: 8,
                    y: 10,
                    z: 24,
                    material: None,
                },
                EditSave {
                    x: 9,
                    y: 11,
                    z: 24,
                    material: Some(Material::Dirt),
                },
            ],
        };
        let json = serde_json::to_string(&file).unwrap();
        let loaded = parse_save(&json, Path::new("mem")).unwrap();
        assert_eq!(loaded.hearts, 3);
        assert_eq!(loaded.inventory.storage[0].unwrap().count, 12);
        assert_eq!(loaded.edits.len(), 2);
        assert_eq!(loaded.edits[1].1, Some(Material::Dirt));
    }

    #[test]
    fn overlay_records_player_air() {
        let mut world = World::new();
        world.set_voxel(IVec3::new(3, 2, 3), Voxel::dirt());
        assert!(world.remove_voxel_player(IVec3::new(3, 2, 3)));
        let snap = world.player_edits_snapshot();
        assert_eq!(snap, vec![(IVec3::new(3, 2, 3), None)]);
    }
}
