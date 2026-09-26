//! Deterministic settlement layouts for realm capitals and villages.
//!
//! Plans mark roads, walls, gates, crop/lake lots and [`FeatureKind::PrefabEgg`]
//! buildings. Worldgen stamps them via [`stamp_settlements_in_chunk`].

use crate::prefab::{load_prefab, placed_solids, voxel_for_material};
use crate::realms::{
    realm_at_block, realm_info, realm_name, shunk_coord, RealmCell, RealmId, RealmInfo,
    REALM_SEED_CELL,
};
use crate::world::{
    mesh_chunk_coord, mix_seed, terrain_height, Material, Voxel, World, MESH_CHUNK_SIZE,
    SHUNK_SIZE, WORLD_SEED,
};
use glam::IVec3;
use std::cell::RefCell;
use std::collections::HashMap;

pub const DEFAULT_HOUSE_PREFAB: &str = "assets/prefabs/house_basic.json";
const SETTLEMENT_SEED: u32 = 0x5E77_1E55;
/// Radio de muralla de aldea (también puerta de supresión total de ores).
pub const VILLAGE_WALL_RADIUS: i32 = 30;
/// Radio de muralla de capital.
pub const CAPITAL_WALL_RADIUS: i32 = 18;
/// Margen más allá de la muralla donde los ores escasean (rampa a 0).
pub const VILLAGE_ORE_RIM: i32 = 48;

const CAPITAL_PREFABS: &[&str] = &[
    "assets/prefabs/house_basic.json",
    "assets/prefabs/shop_general.json",
    "assets/prefabs/blacksmith.json",
    "assets/prefabs/pharmacy.json",
    "assets/prefabs/restaurant.json",
    "assets/prefabs/wizard_tower.json",
    "assets/prefabs/cottage.json",
    "assets/prefabs/house_basic.json",
];

const VILLAGE_PREFABS: &[&str] = &[
    "assets/prefabs/cottage.json",
    "assets/prefabs/house_basic.json",
    "assets/prefabs/shop_general.json",
    "assets/prefabs/restaurant.json",
    // Slot 4 of 8 → every village grows exactly one mage tower (magic seed
    // for the combat update).
    "assets/prefabs/wizard_tower.json",
];

pub const WATCHTOWER_PREFAB: &str = "assets/prefabs/watchtower.json";
pub const MISSION_BOARD_PREFAB: &str = "assets/prefabs/mission_board.json";

/// Random hamlet buildings for villages (1–12 of the catalogue).
const HAMLET_CATALOG: &[&str] = &[
    "assets/prefabs/tavern.json",
    "assets/prefabs/barn.json",
    "assets/prefabs/chapel.json",
    "assets/prefabs/herbalist.json",
    "assets/prefabs/stable.json",
    "assets/prefabs/sawmill.json",
    "assets/prefabs/library.json",
    "assets/prefabs/barracks.json",
    "assets/prefabs/lighthouse.json",
];
const MARKET_STALL_PREFAB: &str = "assets/prefabs/market_stall.json";
const SMITH_STALL_PREFAB: &str = "assets/prefabs/smith_stall.json";
const POTION_STALL_PREFAB: &str = "assets/prefabs/potion_stall.json";
const BARTER_STALL_PREFAB: &str = "assets/prefabs/barter_stall.json";
const CART_PREFAB: &str = "assets/prefabs/cart.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettlementKind {
    Capital,
    Village,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeatureKind {
    Road,
    Wall,
    Gate,
    /// Dirt farm plot centered on this cell (`half` = Chebyshev radius).
    CropPlot {
        half: i32,
    },
    /// Shallow pond centered on this cell.
    Lake {
        radius: i32,
    },
    PrefabEgg {
        prefab: &'static str,
        rotation_quarters: u8,
    },
    /// Village well / fountain: 3×3 stone rim with a water source center.
    Fountain,
    /// Torch lamp post (cobble foot + wood post + flame), every ~6 m.
    TorchPost,
    /// Watchtower prefab, always extra-muros (outside the wall).
    Watchtower {
        rotation_quarters: u8,
    },
    /// Orchard plot: apple trees at the corners, beehive at the center.
    OrchardPlot {
        half: i32,
    },
    /// Quest board prefab (decorative for now — no interaction yet).
    MissionBoard {
        rotation_quarters: u8,
    },
    /// House with a secret basement + chest under the floor.
    CellarHouse {
        rotation_quarters: u8,
    },
    /// Random hamlet building (tavern, barn, stalls…) on a free lot.
    Hamlet {
        prefab: &'static str,
        rotation_quarters: u8,
    },
    /// Emblem arch over one random gate (0=N,1=S,2=W,3=E).
    GateArch {
        side: u8,
    },
    /// Timber-framed mine shaft with crate.
    MineShaft,
    /// Plank dock from shore over water (dir = land→water step).
    Dock {
        dir_x: i8,
        dir_z: i8,
    },
    /// Benches + lamp + planters outside the wall (`half` ≈ wall/3).
    RestStop {
        half: i32,
    },
    /// Clothesline posts with cloth panels outside the wall.
    ClotheslinePlot {
        half: i32,
    },
    /// Walled cemetery with graves + dead tree outside the wall.
    Cemetery {
        half: i32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettlementFeature {
    pub x: i32,
    pub z: i32,
    pub kind: FeatureKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettlementPlan {
    pub realm: RealmId,
    pub kind: SettlementKind,
    pub center_shunk: (i32, i32),
    pub center_block: (i32, i32),
    pub wall_radius: i32,
    pub features: Vec<SettlementFeature>,
}

impl SettlementKind {
    /// Spanish label for the HUD sign.
    pub fn label_es(self) -> &'static str {
        match self {
            SettlementKind::Capital => "CAPITAL",
            SettlementKind::Village => "ALDEA",
        }
    }
}

#[inline]
fn shunk_center_block((sx, sz): (i32, i32)) -> (i32, i32) {
    (
        sx * SHUNK_SIZE + SHUNK_SIZE / 2,
        sz * SHUNK_SIZE + SHUNK_SIZE / 2,
    )
}

fn feature_priority(kind: &FeatureKind) -> u8 {
    match kind {
        FeatureKind::Road => 0,
        FeatureKind::CropPlot { .. }
        | FeatureKind::Lake { .. }
        | FeatureKind::OrchardPlot { .. }
        | FeatureKind::TorchPost
        | FeatureKind::MineShaft
        | FeatureKind::Dock { .. }
        | FeatureKind::RestStop { .. }
        | FeatureKind::ClotheslinePlot { .. }
        | FeatureKind::Cemetery { .. } => 1,
        FeatureKind::Wall => 2,
        FeatureKind::Gate => 3,
        FeatureKind::PrefabEgg { .. }
        | FeatureKind::Watchtower { .. }
        | FeatureKind::MissionBoard { .. }
        | FeatureKind::CellarHouse { .. }
        | FeatureKind::Hamlet { .. }
        | FeatureKind::GateArch { .. } => 4,
        // La fuente manda sobre casas: el agua no se tapa.
        FeatureKind::Fountain => 5,
    }
}

fn put(features: &mut Vec<SettlementFeature>, x: i32, z: i32, kind: FeatureKind) {
    if let Some(old) = features.iter_mut().find(|f| f.x == x && f.z == z) {
        if feature_priority(&kind) >= feature_priority(&old.kind) {
            old.kind = kind;
        }
        return;
    }
    features.push(SettlementFeature { x, z, kind });
}

/// Prefab entrance faces local −Z at `rotation_quarters == 0`. Aim that face at plaza.
fn rotation_facing_plaza(egg_x: i32, egg_z: i32, cx: i32, cz: i32) -> u8 {
    let dx = cx - egg_x;
    let dz = cz - egg_z;
    if dx.abs() >= dz.abs() {
        if dx > 0 {
            3
        } else {
            1
        }
    } else if dz > 0 {
        2
    } else {
        0
    }
}

/// Deterministic free lot inside the walls for hamlets: off the cross
/// roads, above sea, ≥10 from taken lots and clear of farm plots.
fn hamlet_free_lot(
    cx: i32,
    cz: i32,
    lim: i32,
    taken: &[(i32, i32)],
    plots: &[(i32, i32, i32)],
    seed: u32,
    salt: u32,
) -> Option<(i32, i32)> {
    use crate::world::SEA_LEVEL;
    let span = (2 * lim + 1) as u32;
    for attempt in 0..96u32 {
        let h = mix_seed(seed, salt, attempt);
        let x = cx + (h % span) as i32 - lim;
        let z = cz + ((h >> 16) % span) as i32 - lim;
        if (x - cx).abs() <= 2 || (z - cz).abs() <= 2 {
            continue; // cross roads
        }
        if terrain_height(x, z) < SEA_LEVEL {
            continue;
        }
        if taken
            .iter()
            .any(|(ex, ez)| (ex - x).abs().max((ez - z).abs()) < 10)
        {
            continue;
        }
        if plots
            .iter()
            .any(|(px, pz, c)| (px - x).abs().max((pz - z).abs()) < *c)
        {
            continue;
        }
        return Some((x, z));
    }
    None
}

/// Any water within `r` (shore check for the lighthouse).
fn water_near(x: i32, z: i32, r: i32) -> bool {
    use crate::world::SEA_LEVEL;
    for dz in -r..=r {
        for dx in -r..=r {
            if terrain_height(x + dx, z + dz) < SEA_LEVEL {
                return true;
            }
        }
    }
    false
}

/// Local wall + gates + roads + crops/lakes + prefab lots for one landmark.
pub fn settlement_plan(
    realm: RealmId,
    center_shunk: (i32, i32),
    kind: SettlementKind,
) -> SettlementPlan {
    let (cx, cz) = shunk_center_block(center_shunk);
    let radius = match kind {
        SettlementKind::Capital => CAPITAL_WALL_RADIUS,
        SettlementKind::Village => VILLAGE_WALL_RADIUS,
    };
    let mut features = Vec::new();

    // Square palisade. Three cardinal cells per side become a gate so the
    // 3-wide cross roads pass through a 3-wide opening (was 1-wide).
    for d in -radius..=radius {
        for (x, z) in [
            (cx + d, cz - radius),
            (cx + d, cz + radius),
            (cx - radius, cz + d),
            (cx + radius, cz + d),
        ] {
            let gate = ((z - cz).abs() == radius && (x - cx).abs() <= 1)
                || ((x - cx).abs() == radius && (z - cz).abs() <= 1);
            put(
                &mut features,
                x,
                z,
                if gate {
                    FeatureKind::Gate
                } else {
                    FeatureKind::Wall
                },
            );
        }
    }

    // Cross roads: center square reaches every gate and continues one block out.
    for d in -(radius + 1)..=(radius + 1) {
        for w in -1..=1 {
            put(&mut features, cx + d, cz + w, FeatureKind::Road);
            put(&mut features, cx + w, cz + d, FeatureKind::Road);
        }
    }

    // Crop plots and a pond — yards near the wall corners (clear of house lots).
    match kind {
        SettlementKind::Capital => {
            let y = radius - 4;
            put(
                &mut features,
                cx + y,
                cz - y,
                FeatureKind::CropPlot { half: 2 },
            );
            put(
                &mut features,
                cx - y,
                cz - y,
                FeatureKind::CropPlot { half: 2 },
            );
            put(
                &mut features,
                cx - y,
                cz + y,
                FeatureKind::Lake { radius: 2 },
            );
            put(
                &mut features,
                cx + y,
                cz + y,
                FeatureKind::CropPlot { half: 2 },
            );
        }
        SettlementKind::Village => {
            let y = radius - 4;
            put(
                &mut features,
                cx + y,
                cz - y,
                FeatureKind::CropPlot { half: 2 },
            );
            put(
                &mut features,
                cx - y,
                cz + y,
                FeatureKind::Lake { radius: 2 },
            );
        }
    }

    // Prefab eggs reserve lots between roads and wall. Village lots sit 16
    // apart on an outer ring so 5–9 wide houses keep air between walls;
    // hamlets fill the freed center (was 13 → houses choked the middle).
    let lot = match kind {
        SettlementKind::Capital => 8,
        SettlementKind::Village => 16,
    };
    let slots: &[(i32, i32)] = match kind {
        SettlementKind::Capital => &[
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
            (0, -1),
            (0, 1),
            (-1, 0),
            (1, 0),
        ],
        SettlementKind::Village => &[
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
            (0, -1),
            (0, 1),
            (-1, 0),
            (1, 0),
        ],
    };
    let catalog = match kind {
        SettlementKind::Capital => CAPITAL_PREFABS,
        SettlementKind::Village => VILLAGE_PREFABS,
    };
    let seed = mix_seed(
        SETTLEMENT_SEED ^ WORLD_SEED,
        center_shunk.0 as u32,
        center_shunk.1 as u32,
    );
    // Egg lots first (positions only) so neighbors can be spread apart below.
    let mut eggs: Vec<(i32, i32, &'static str)> = Vec::new();
    for (i, &(sx, sz)) in slots.iter().enumerate() {
        let jitter = mix_seed(seed, i as u32, realm.ix as u32 ^ realm.iz as u32);
        let jx = (jitter % 3) as i32 - 1;
        let jz = ((jitter >> 4) % 3) as i32 - 1;
        let mut x = cx + sx * lot + jx;
        let mut z = cz + sz * lot + jz;
        // A center-road slot is shifted into a quadrant.
        if x.abs_diff(cx) <= 2 {
            x += if sx >= 0 { lot / 2 } else { -lot / 2 };
        }
        if z.abs_diff(cz) <= 2 {
            z += if sz >= 0 { lot / 2 } else { -lot / 2 };
        }
        if (x - cx).abs() >= radius - 3 || (z - cz).abs() >= radius - 3 {
            continue;
        }
        eggs.push((x, z, catalog[i % catalog.len()]));
    }
    // Spread neighboring eggs: minimum Chebyshev gap 10 (widest prefab is 9,
    // so walls never touch). The farther-from-center egg moves — road-side
    // eggs never slide onto the road. Clamped inside the walls. Deterministic
    // and bounded; rotation is computed from the final spot.
    const MIN_EGG_GAP: i32 = 10;
    let lim = radius - 4;
    for _ in 0..8 {
        let mut moved = false;
        for a in 0..eggs.len() {
            for b in a + 1..eggs.len() {
                let dx = eggs[b].0 - eggs[a].0;
                let dz = eggs[b].1 - eggs[a].1;
                if dx.abs().max(dz.abs()) >= MIN_EGG_GAP {
                    continue;
                }
                let da = (eggs[a].0 - cx).abs() + (eggs[a].1 - cz).abs();
                let db = (eggs[b].0 - cx).abs() + (eggs[b].1 - cz).abs();
                // Try the farther egg first, then the other one.
                let order = if db >= da { [b, a] } else { [a, b] };
                for &m in &order {
                    let o = if m == b { 1 } else { -1 };
                    let (sx, sz) = if dx.abs() >= dz.abs() {
                        let s = (dx * o).signum();
                        (if s == 0 { o } else { s }, 0)
                    } else {
                        let s = (dz * o).signum();
                        (0, if s == 0 { o } else { s })
                    };
                    let nx = (eggs[m].0 + sx).clamp(cx - lim, cx + lim);
                    let nz = (eggs[m].1 + sz).clamp(cz - lim, cz + lim);
                    if (nx, nz) != (eggs[m].0, eggs[m].1) {
                        eggs[m].0 = nx;
                        eggs[m].1 = nz;
                        moved = true;
                        break;
                    }
                }
            }
        }
        if !moved {
            break;
        }
    }
    for &(x, z, prefab) in &eggs {
        put(
            &mut features,
            x,
            z,
            FeatureKind::PrefabEgg {
                prefab,
                rotation_quarters: rotation_facing_plaza(x, z, cx, cz),
            },
        );
    }
    // Lots already taken (houses + landmarks below push here too) — hamlets
    // keep ≥10 Chebyshev gap so walls never touch.
    let mut reserved: Vec<(i32, i32)> = eggs.iter().map(|&(x, z, _)| (x, z)).collect();

    // Pozo/fuente artificial solo en aldeas (la capital tiene las cuatro
    // esquinas con cultivos/lago): esquina libre opuesta, 3×3 con brocal
    // y agua infinita en el centro.
    if kind == SettlementKind::Village {
        let fy = radius - 4;
        put(&mut features, cx - fy, cz - fy, FeatureKind::Fountain);
    }

    // Antorchas cada ~6 m en el anillo exterior (fuera de los ejes de camino
    // para no plantarlas sobre la calzada).
    let ring = radius + 1;
    for d in -ring..=ring {
        if d.rem_euclid(6) != 0 {
            continue;
        }
        for (x, z) in [
            (cx + d, cz - ring),
            (cx + d, cz + ring),
            (cx - ring, cz + d),
            (cx + ring, cz + d),
        ] {
            if (x - cx).abs() <= 2 || (z - cz).abs() <= 2 {
                continue;
            }
            put(&mut features, x, z, FeatureKind::TorchPost);
        }
    }
    // Dos antorchas flanqueando cada puerta, fuera del muro.
    for s in [-1, 1] {
        for o in [-2, 2] {
            put(
                &mut features,
                cx + o,
                cz + s * (radius + 1),
                FeatureKind::TorchPost,
            );
            put(
                &mut features,
                cx + s * (radius + 1),
                cz + o,
                FeatureKind::TorchPost,
            );
        }
    }
    // Antorchas en los caminos locales cada 8, al lateral, lejos de casas.
    for d in (-(radius + 1)..=(radius + 1)).step_by(8) {
        for (x, z) in [
            (cx + d, cz + 2),
            (cx + d, cz - 2),
            (cx + 2, cz + d),
            (cx - 2, cz + d),
        ] {
            if eggs
                .iter()
                .any(|(ex, ez, _)| (ex - x).abs().max((ez - z).abs()) < 5)
            {
                continue;
            }
            put(&mut features, x, z, FeatureKind::TorchPost);
        }
    }

    // Atalaya siempre extramuros: diagonal NE a +9 del muro (los caminos
    // van en cruz, las diagonales están libres).
    {
        let (tx, tz) = (cx + (radius + 9), cz - (radius + 9));
        reserved.push((tx, tz));
        put(
            &mut features,
            tx,
            tz,
            FeatureKind::Watchtower {
                rotation_quarters: rotation_facing_plaza(tx, tz, cx, cz),
            },
        );
    }

    // Tablón de encargos cerca de la puerta este (decorativo por ahora):
    // espiral desde (cx+4, cz+3) hasta una celda libre de caminos y casas.
    {
        let mut board = None;
        'search: for r in 0..10i32 {
            for dz in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dz.abs()) != r {
                        continue;
                    }
                    let (x, z) = (cx + 4 + dx, cz + 3 + dz);
                    if (x - cx).abs() <= 1 || (z - cz).abs() <= 1 {
                        continue;
                    }
                    if (x - cx).abs() >= radius - 2 || (z - cz).abs() >= radius - 2 {
                        continue;
                    }
                    if eggs
                        .iter()
                        .any(|(ex, ez, _)| (ex - x).abs().max((ez - z).abs()) < 4)
                    {
                        continue;
                    }
                    board = Some((x, z));
                    break 'search;
                }
            }
        }
        if let Some((x, z)) = board {
            reserved.push((x, z));
            put(
                &mut features,
                x,
                z,
                FeatureKind::MissionBoard {
                    rotation_quarters: rotation_facing_plaza(x, z, cx, cz),
                },
            );
        }
    }

    if kind == SettlementKind::Village {
        // Huertos en las afueras: diagonales SO y SE a +5 (la atalaya va NE).
        for (ox, oz) in [(-(radius + 5), radius + 5), (radius + 5, radius + 5)] {
            put(
                &mut features,
                cx + ox,
                cz + oz,
                FeatureKind::OrchardPlot { half: 2 },
            );
        }
        // Casa con sótano secreto: espiral desde (-13,+13) hasta un lote
        // libre de caminos, casas (≥9), cultivos y fuente.
        let y = radius - 4; // esquinas de cultivo/lago/fuente
        let mut cellar = None;
        'cellar_search: for r in 0..10i32 {
            for dz in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dz.abs()) != r {
                        continue;
                    }
                    let (x, z) = (cx - 13 + dx, cz + 13 + dz);
                    if (x - cx).abs() <= 1 || (z - cz).abs() <= 1 {
                        continue;
                    }
                    if (x - cx).abs() >= radius - 5 || (z - cz).abs() >= radius - 5 {
                        continue;
                    }
                    if (x - (cx + y)).abs().max((z - (cz - y)).abs()) < 6 {
                        continue;
                    }
                    if (x - (cx - y)).abs().max((z - (cz + y)).abs()) < 6 {
                        continue;
                    }
                    if (x - (cx - y)).abs().max((z - (cz - y)).abs()) < 5 {
                        continue;
                    }
                    if eggs
                        .iter()
                        .all(|(ex, ez, _)| (ex - x).abs().max((ez - z).abs()) >= 9)
                    {
                        cellar = Some((x, z));
                        break 'cellar_search;
                    }
                }
            }
        }
        if let Some((x, z)) = cellar {
            reserved.push((x, z));
            put(
                &mut features,
                x,
                z,
                FeatureKind::CellarHouse {
                    rotation_quarters: rotation_facing_plaza(x, z, cx, cz),
                },
            );
        }

        // Hamlets aleatorios: 4 edificios del catálogo (faro solo con agua
        // cerca) + mercado de fin de semana + tenderetes + carreta, cada uno
        // en lote libre.
        let lim = radius - 8;
        let py = radius - 4;
        let plots = [
            (cx + py, cz - py, 5),
            (cx - py, cz + py, 5),
            (cx - py, cz - py, 4),
        ];
        let mut place_hamlet = |features: &mut Vec<SettlementFeature>,
                                reserved: &mut Vec<(i32, i32)>,
                                prefab: &'static str,
                                lot: (i32, i32)| {
            let (x, z) = lot;
            reserved.push((x, z));
            put(
                features,
                x,
                z,
                FeatureKind::Hamlet {
                    prefab,
                    rotation_quarters: rotation_facing_plaza(x, z, cx, cz),
                },
            );
        };
        let mut order: Vec<usize> = (0..HAMLET_CATALOG.len()).collect();
        order.sort_by_key(|&i| mix_seed(seed, i as u32, 0x9A15));
        let mut built = 0;
        for &i in &order {
            if built >= 4 {
                break;
            }
            let prefab = HAMLET_CATALOG[i];
            let Some(lot) = hamlet_free_lot(cx, cz, lim, &reserved, &plots, seed, 10 + i as u32)
            else {
                continue;
            };
            if prefab.contains("lighthouse") && !water_near(lot.0, lot.1, 10) {
                continue;
            }
            place_hamlet(&mut features, &mut reserved, prefab, lot);
            built += 1;
        }
        // Mercado de fin de semana: centro + 3 puestos alrededor.
        if let Some(center) = hamlet_free_lot(cx, cz, lim, &reserved, &plots, seed, 20) {
            place_hamlet(&mut features, &mut reserved, MARKET_STALL_PREFAB, center);
            for (ox, oz) in [(-3, 0), (3, 0), (0, 3)] {
                let (x, z) = (center.0 + ox, center.1 + oz);
                if (x - cx).abs() > lim || (z - cz).abs() > lim {
                    continue;
                }
                if (x - cx).abs() <= 2 || (z - cz).abs() <= 2 {
                    continue;
                }
                if terrain_height(x, z) < crate::world::SEA_LEVEL {
                    continue;
                }
                if reserved
                    .iter()
                    .any(|(ex, ez)| (ex - x).abs().max((ez - z).abs()) < 6)
                {
                    continue;
                }
                if plots
                    .iter()
                    .any(|(px, pz, c)| (px - x).abs().max((pz - z).abs()) < *c)
                {
                    continue;
                }
                place_hamlet(&mut features, &mut reserved, MARKET_STALL_PREFAB, (x, z));
            }
        }
        // Tenderetes (herrero, pociones, trueque) + carreta ambulante.
        for (prefab, salt) in [
            (SMITH_STALL_PREFAB, 21u32),
            (POTION_STALL_PREFAB, 22),
            (BARTER_STALL_PREFAB, 23),
            (CART_PREFAB, 24),
        ] {
            if let Some(lot) = hamlet_free_lot(cx, cz, lim, &reserved, &plots, seed, salt) {
                place_hamlet(&mut features, &mut reserved, prefab, lot);
            }
        }
        // Mina: intente lote libre primero; si no hay espacio, coloque en posición
        // garantizada al sur del centro (fuera del muro de cultivo, sobre tierra).
        let mine_pos = if let Some((x, z)) = hamlet_free_lot(cx, cz, lim, &reserved, &plots, seed, 25) {
            reserved.push((x, z));
            (x, z)
        } else {
            // Fallback: sur del centro, a 8 casillas del muro interior.
            let fx = cx;
            let fz = cz - radius + 8;
            reserved.push((fx, fz));
            (fx, fz)
        };
        reserved.push((mine_pos.0, mine_pos.1));
        put(&mut features, mine_pos.0, mine_pos.1, FeatureKind::MineShaft);
        // Arco con emblema sobre una puerta al azar (fuera del muro para no
        // romper la puerta: el paso queda libre).
        {
            let side = (mix_seed(seed, 7, 7) % 4) as u8;
            let (gx, gz) = match side {
                0 => (cx, cz - radius - 1),
                1 => (cx, cz + radius + 1),
                2 => (cx - radius - 1, cz),
                _ => (cx + radius + 1, cz),
            };
            put(&mut features, gx, gz, FeatureKind::GateArch { side });
        }
        // Muelle: primera orilla fuera del muro (tierra→agua), si existe.
        {
            let mut dock = None;
            'dock_search: for d in (radius + 2)..=(radius + 14) {
                for dz in -d..=d {
                    for dx in -d..=d {
                        if dx.abs().max(dz.abs()) != d {
                            continue;
                        }
                        let (x, z) = (cx + dx, cz + dz);
                        if terrain_height(x, z) >= crate::world::SEA_LEVEL {
                            continue;
                        }
                        for (ox, oz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                            let (lx, lz) = (x - ox, z - oz);
                            if terrain_height(lx, lz) >= crate::world::SEA_LEVEL {
                                dock = Some((lx, lz, ox as i8, oz as i8));
                                break 'dock_search;
                            }
                        }
                    }
                }
            }
            if let Some((x, z, dx, dz)) = dock {
                put(
                    &mut features,
                    x,
                    z,
                    FeatureKind::Dock { dir_x: dx, dir_z: dz },
                );
            }
        }
        // Extras fuera del muro a 1/3 del tamaño: descanso, tendedero y
        // cementerio en diagonales distintas.
        {
            let half = radius / 3;
            let dist = radius + 4 + half;
            let base = (mix_seed(seed, 9, 9) % 4) as i32;
            for (i, make) in [
                (0, FeatureKind::RestStop { half }),
                (1, FeatureKind::ClotheslinePlot { half }),
                (2, FeatureKind::Cemetery { half }),
            ] {
                let oct = (2 * ((base + i as i32) % 4) + 1) as f32;
                let ang = oct * std::f32::consts::FRAC_PI_4;
                let mut pick = None;
                for k in 0..4 {
                    let a = ang + k as f32 * std::f32::consts::FRAC_PI_2;
                    let x = cx + (a.cos() * dist as f32).round() as i32;
                    let z = cz + (a.sin() * dist as f32).round() as i32;
                    if terrain_height(x, z) >= crate::world::SEA_LEVEL {
                        pick = Some((x, z));
                        break;
                    }
                }
                if let Some((x, z)) = pick {
                    put(&mut features, x, z, make);
                }
            }
        }
    }

    SettlementPlan {
        realm,
        kind,
        center_shunk,
        center_block: (cx, cz),
        wall_radius: radius,
        features,
    }
}

/// Capital and all village plans for one realm (cached — feature grids are heavy).
pub fn plans_for_realm(info: &RealmInfo) -> Vec<SettlementPlan> {
    let cache = plans_cache();
    if let Ok(guard) = cache.lock() {
        if let Some(hit) = guard.get(&info.id) {
            return hit.clone();
        }
    }
    let mut plans = Vec::with_capacity(1 + info.villages.len());
    plans.push(settlement_plan(
        info.id,
        info.capital,
        SettlementKind::Capital,
    ));
    plans.extend(
        info.villages
            .iter()
            .copied()
            .map(|p| settlement_plan(info.id, p, SettlementKind::Village)),
    );
    if let Ok(mut guard) = cache.lock() {
        guard.insert(info.id, plans.clone());
    }
    plans
}

fn plans_cache(
) -> &'static std::sync::Mutex<std::collections::HashMap<RealmId, Vec<SettlementPlan>>> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<RealmId, Vec<SettlementPlan>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Convenience query by realm id.
#[allow(dead_code)]
pub fn plans_for_realm_id(id: RealmId) -> Option<Vec<SettlementPlan>> {
    realm_info(id).map(|info| plans_for_realm(&info))
}

/// Regional road shunks connecting the capital to every village.
///
/// Uses deterministic L paths. The bend order alternates by seed so the road
/// network does not always look identical.
pub fn realm_road_shunks(info: &RealmInfo) -> Vec<(i32, i32)> {
    let mut road = Vec::new();
    for (i, &(vx, vz)) in info.villages.iter().enumerate() {
        let (mut x, mut z) = info.capital;
        let x_first = mix_seed(
            SETTLEMENT_SEED,
            info.id.ix as u32 ^ i as u32,
            info.id.iz as u32,
        ) & 1
            == 0;
        let mut push = |p| {
            if !road.contains(&p) {
                road.push(p);
            }
        };
        push((x, z));
        if x_first {
            while x != vx {
                x += (vx - x).signum();
                push((x, z));
            }
            while z != vz {
                z += (vz - z).signum();
                push((x, z));
            }
        } else {
            while z != vz {
                z += (vz - z).signum();
                push((x, z));
            }
            while x != vx {
                x += (vx - x).signum();
                push((x, z));
            }
        }
    }
    road
}

/// Nearest capital / village landmark to a world block, scanning nearby realms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NearestSettlement {
    pub realm: RealmId,
    pub kind: SettlementKind,
    pub center_block: (i32, i32),
    /// Straight-line distance in blocks (rounded).
    pub dist: i32,
}

/// What a location sign should read: realm + biome + closest settlement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignInfo {
    /// Realm you stand in (`None` = wild / unclaimed).
    pub realm: Option<RealmId>,
    pub realm_name: Option<String>,
    pub biome: crate::biomes::BiomeId,
    pub nearest: Option<NearestSettlement>,
}

/// Closest settlement (capital or village) to block `(x, z)`, if any is near.
///
/// Uses landmark shunks from [`RealmInfo`] only — does **not** rebuild full
/// settlement feature grids (those are for stamping).
pub fn nearest_settlement_to_block(x: i32, z: i32) -> Option<NearestSettlement> {
    let (sx, sz) = shunk_coord(x, z);
    let mut best: Option<NearestSettlement> = None;
    let mut consider = |realm: RealmId, kind: SettlementKind, center_shunk: (i32, i32)| {
        let (bx, bz) = shunk_center_block(center_shunk);
        let d = (((bx - x) as f32).hypot((bz - z) as f32)).round() as i32;
        if best.as_ref().is_none_or(|b| d < b.dist) {
            best = Some(NearestSettlement {
                realm,
                kind,
                center_block: (bx, bz),
                dist: d,
            });
        }
    };
    for info in nearby_realm_infos(sx, sz) {
        consider(info.id, SettlementKind::Capital, info.capital);
        for &v in &info.villages {
            consider(info.id, SettlementKind::Village, v);
        }
    }
    best
}

/// Distancia en bloques al centro de aldea más cercano (`None` si no hay
/// ninguna en el rango escaneado). Solo aldeas — las capitales no suprimen ores.
pub fn nearest_village_dist(x: i32, z: i32) -> Option<i32> {
    let (sx, sz) = shunk_coord(x, z);
    let mut best: Option<i32> = None;
    for info in nearby_realm_infos(sx, sz) {
        for &v in &info.villages {
            let (bx, bz) = shunk_center_block(v);
            let d = (((bx - x) as f32).hypot((bz - z) as f32)).round() as i32;
            if best.is_none_or(|b| d < b) {
                best = Some(d);
            }
        }
    }
    best
}

/// Supresión de ores por aldea en 0..=1 (0 = intacto, 1 = sin ores):
/// casi total dentro de la muralla, rampa a 0 en el borde exterior
/// (cerca de la aldea los ores escasean).
pub fn village_ore_suppression(x: i32, z: i32) -> f32 {
    let Some(d) = nearest_village_dist(x, z) else {
        return 0.0;
    };
    if d <= VILLAGE_WALL_RADIUS {
        return 0.98;
    }
    let t = (d - VILLAGE_WALL_RADIUS) as f32 / VILLAGE_ORE_RIM as f32;
    if t >= 1.0 {
        return 0.0;
    }
    0.98 * (1.0 - t)
}

/// True si (x,z) cae dentro de un asentamiento (muralla + 2 de margen):
/// ahí no se genera agua natural (mares, ríos ni termales). La fuente del
/// pueblo (artificial) sí se estampa.
pub fn settlement_claims_block(x: i32, z: i32) -> bool {
    let (sx, sz) = shunk_coord(x, z);
    for info in nearby_realm_infos(sx, sz) {
        let (bcx, bcz) = shunk_center_block(info.capital);
        if (bcx - x).abs().max((bcz - z).abs()) <= CAPITAL_WALL_RADIUS + 2 {
            return true;
        }
        for &v in &info.villages {
            let (bvx, bvz) = shunk_center_block(v);
            if (bvx - x).abs().max((bvz - z).abs()) <= VILLAGE_WALL_RADIUS + 2 {
                return true;
            }
        }
    }
    false
}

/// Per-mesh-chunk cache for [`settlement_claims_block`].
///
/// The claim test walks a 5x5 realm neighborhood and allocates a `Vec` per
/// call, which is far too slow to repeat once per column and once per meshing
/// face. Results are memoized per mesh chunk; a thread-local keeps this usable
/// from the rayon mesh/terrain passes without a lock.
const CLAIM_CACHE_MAX: usize = 8192;

thread_local! {
    static CLAIM_CACHE: RefCell<HashMap<(i32, i32), Vec<bool>>> =
        RefCell::new(HashMap::new());
}

/// [`settlement_claims_block`] memoized per mesh chunk — use this in hot loops.
pub fn settlement_claims_block_cached(x: i32, z: i32) -> bool {
    let s = MESH_CHUNK_SIZE;
    let (cx, cz) = mesh_chunk_coord(x, z);
    CLAIM_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.contains_key(&(cx, cz)) {
            if cache.len() >= CLAIM_CACHE_MAX {
                cache.clear();
            }
            let (x0, z0) = (cx * s, cz * s);
            let mut cells = Vec::with_capacity((s * s) as usize);
            for z in z0..z0 + s {
                for x in x0..x0 + s {
                    cells.push(settlement_claims_block(x, z));
                }
            }
            cache.insert((cx, cz), cells);
        }
        let cells = &cache[&(cx, cz)];
        let lx = x - cx * s;
        let lz = z - cz * s;
        cells[(lz * s + lx) as usize]
    })
}

/// Realm at this block + nearest settlement — everything a location sign needs.
pub fn sign_info_at_block(x: i32, z: i32) -> SignInfo {
    let realm = match realm_at_block(x, z) {
        RealmCell::Claimed(id) => Some(id),
        RealmCell::Wild => None,
    };
    SignInfo {
        realm,
        realm_name: realm.map(realm_name),
        biome: crate::biomes::biome_at(x, z),
        nearest: nearest_settlement_to_block(x, z),
    }
}

fn nearby_realm_infos(sx: i32, sz: i32) -> Vec<RealmInfo> {
    let ix0 = sx.div_euclid(REALM_SEED_CELL) - 2;
    let iz0 = sz.div_euclid(REALM_SEED_CELL) - 2;
    let mut out = Vec::new();
    for iz in iz0..=iz0 + 4 {
        for ix in ix0..=ix0 + 4 {
            if let Some(info) = realm_info(RealmId { ix, iz }) {
                out.push(info);
            }
        }
    }
    out
}

fn clear_column_above(world: &mut World, x: i32, z: i32, from_y: i32, to_y: i32) {
    for y in from_y..=to_y {
        world.remove_voxel(IVec3::new(x, y, z));
    }
}

fn pave_road_cell(world: &mut World, x: i32, z: i32) {
    let h = terrain_height(x, z);
    clear_column_above(world, x, z, h + 1, h + 6);
    // Ochre road cobble — visually distinct from the grey wall cobble.
    world.set_voxel(IVec3::new(x, h, z), Voxel::solid(Material::RoadCobble));
}

fn stamp_wall_cell(world: &mut World, x: i32, z: i32) {
    let h = terrain_height(x, z);
    clear_column_above(world, x, z, h + 1, h + 6);
    for dy in 0..=3 {
        world.set_voxel(
            IVec3::new(x, h + dy, z),
            Voxel::solid(Material::Cobblestone),
        );
    }
}

fn stamp_gate_cell(world: &mut World, x: i32, z: i32) {
    let h = terrain_height(x, z);
    clear_column_above(world, x, z, h + 1, h + 6);
    world.set_voxel(IVec3::new(x, h, z), Voxel::stone());
}

fn stamp_crop_plot(
    world: &mut World,
    cx: i32,
    cz: i32,
    half: i32,
    x0: i32,
    z0: i32,
    x1: i32,
    z1: i32,
) {
    if terrain_height(cx, cz) < crate::world::SEA_LEVEL {
        return;
    }
    for dz in -half..=half {
        for dx in -half..=half {
            let x = cx + dx;
            let z = cz + dz;
            if x < x0 || x >= x1 || z < z0 || z >= z1 {
                continue;
            }
            let h = terrain_height(x, z);
            clear_column_above(world, x, z, h + 1, h + 6);
            world.set_voxel(IVec3::new(x, h, z), Voxel::dirt());
            // Checkerboard crop plants (leaves stand-in).
            if ((dx + dz) & 1) == 0 && dx.abs() < half && dz.abs() < half {
                world.set_voxel(IVec3::new(x, h + 1, z), Voxel::leaves());
            }
        }
    }
}

/// Pozo de aldea: brocal 3×3 de piedra de aldea con fuente infinita al centro.
fn stamp_fountain(
    world: &mut World,
    cx: i32,
    cz: i32,
    x0: i32,
    z0: i32,
    x1: i32,
    z1: i32,
) {
    use crate::world::SEA_LEVEL;
    for dz in -1..=1 {
        for dx in -1..=1 {
            let x = cx + dx;
            let z = cz + dz;
            if x < x0 || x >= x1 || z < z0 || z >= z1 {
                continue;
            }
            let h = terrain_height(x, z);
            // Nada de pozos bajo el mar (sería redundante).
            if h < SEA_LEVEL {
                continue;
            }
            clear_column_above(world, x, z, h + 1, h + 6);
            if dx == 0 && dz == 0 {
                world.set_voxel(IVec3::new(x, h, z), Voxel::solid(Material::Water));
            } else {
                world.set_voxel(
                    IVec3::new(x, h, z),
                    Voxel::solid(Material::VillageStone),
                );
            }
        }
    }
}
fn stamp_lake(
    world: &mut World,
    cx: i32,
    cz: i32,
    radius: i32,
    x0: i32,
    z0: i32,
    x1: i32,
    z1: i32,
) {
    if terrain_height(cx, cz) < crate::world::SEA_LEVEL {
        return;
    }
    let r2 = radius * radius;
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            let d2 = dx * dx + dz * dz;
            if d2 > r2 {
                continue;
            }
            let x = cx + dx;
            let z = cz + dz;
            if x < x0 || x >= x1 || z < z0 || z >= z1 {
                continue;
            }
            let h = terrain_height(x, z);
            clear_column_above(world, x, z, h, h + 6);
            let shore = d2 > (radius - 1).max(0) * (radius - 1).max(0);
            if shore {
                world.set_voxel(IVec3::new(x, h, z), Voxel::solid(Material::Sand));
            } else {
                world.set_voxel(IVec3::new(x, h - 1, z), Voxel::dirt());
                world.set_voxel(IVec3::new(x, h, z), Voxel::solid(Material::Water));
            }
        }
    }
}

/// Poste de antorcha: pie de roca, mástil de leño y llama.
fn stamp_torch_post(world: &mut World, x: i32, z: i32) {
    use crate::world::SEA_LEVEL;
    let h = terrain_height(x, z);
    if h < SEA_LEVEL {
        return;
    }
    clear_column_above(world, x, z, h + 1, h + 6);
    world.set_voxel(
        IVec3::new(x, h, z),
        Voxel::solid(Material::Cobblestone),
    );
    for dy in 1..=2 {
        world.set_voxel(IVec3::new(x, h + dy, z), Voxel::solid(Material::Wood));
    }
    world.set_voxel(IVec3::new(x, h + 3, z), Voxel::solid(Material::Torch));
}

/// Manzano de huerto: tronco bajo, copa 3×3 con 4 manzanas al borde.
fn stamp_apple_tree(world: &mut World, x: i32, z: i32) {
    use crate::world::SEA_LEVEL;
    let h = terrain_height(x, z);
    if h < SEA_LEVEL {
        return;
    }
    clear_column_above(world, x, z, h + 1, h + 8);
    for dy in 1..=3 {
        world.set_voxel(IVec3::new(x, h + dy, z), Voxel::solid(Material::Wood));
    }
    for dz in -1i32..=1 {
        for dx in -1i32..=1 {
            let mat = if dx == 0 && dz == 0 {
                Material::Leaves
            } else if dx.abs() + dz.abs() == 1 {
                Material::Apple
            } else {
                Material::Leaves
            };
            world.set_voxel(IVec3::new(x + dx, h + 4, z + dz), Voxel::solid(mat));
        }
    }
}

/// Huerto: manzanos en las esquinas y colmena central sobre poste.
fn stamp_orchard_plot(
    world: &mut World,
    cx: i32,
    cz: i32,
    half: i32,
    x0: i32,
    z0: i32,
    x1: i32,
    z1: i32,
) {
    use crate::world::SEA_LEVEL;
    if terrain_height(cx, cz) < SEA_LEVEL {
        return;
    }
    for dz in -half..=half {
        for dx in -half..=half {
            let x = cx + dx;
            let z = cz + dz;
            if x < x0 || x >= x1 || z < z0 || z >= z1 {
                continue;
            }
            if dx.abs() == half && dz.abs() == half {
                stamp_apple_tree(world, x, z);
            } else if dx == 0 && dz == 0 {
                let h = terrain_height(x, z);
                clear_column_above(world, x, z, h + 1, h + 6);
                world.set_voxel(IVec3::new(x, h + 1, z), Voxel::solid(Material::Wood));
                world.set_voxel(
                    IVec3::new(x, h + 2, z),
                    Voxel::solid(Material::Beehive),
                );
            }
        }
    }
}

/// Casa con sótano secreto: prefab arriba + cuarto 3×3 tallado abajo
/// (técnica cuevas: remove_voxel conserva el techo en extras), cofre al
/// centro y tiro de entrada en la esquina bajo la casa.
fn stamp_cellar_house(
    world: &mut World,
    hx: i32,
    hz: i32,
    rotation_quarters: u8,
    x0: i32,
    z0: i32,
    x1: i32,
    z1: i32,
) {
    use crate::world::SEA_LEVEL;
    stamp_prefab_egg(
        world,
        hx,
        hz,
        DEFAULT_HOUSE_PREFAB,
        rotation_quarters,
        x0,
        z0,
        x1,
        z1,
    );
    let h = terrain_height(hx, hz);
    if h < SEA_LEVEL + 2 || h - 5 <= 1 {
        return;
    }
    let in_chunk = |x: i32, z: i32| x >= x0 && x < x1 && z >= z0 && z < z1;
    // Cuarto 3×3, aire en h-4..=h-2 (techo h-1..=h queda en extras).
    for dz in -1..=1 {
        for dx in -1..=1 {
            if !in_chunk(hx + dx, hz + dz) {
                continue;
            }
            for y in (h - 4)..=(h - 2) {
                let _ = world.remove_voxel(IVec3::new(hx + dx, y, hz + dz));
            }
        }
    }
    // Cofre al centro con aire encima (como los cofres de cueva, sin loot UI).
    if in_chunk(hx, hz) {
        world.set_voxel(IVec3::new(hx, h - 4, hz), Voxel::chest());
    }
    // Tiro de entrada en la esquina: agujero hasta el cielo.
    if in_chunk(hx + 1, hz + 1) {
        let _ = world.remove_voxel(IVec3::new(hx + 1, h - 1, hz + 1));
        let _ = world.remove_voxel(IVec3::new(hx + 1, h, hz + 1));
    }
}

/// Arco con emblema sobre la puerta: pilares a ±3 (las antorchas quedan en
/// ±2), dintel a la altura del pilar más alto + cristal de maná al centro.
/// El paso central queda libre.
fn stamp_gate_arch(world: &mut World, gx: i32, gz: i32, side: u8) {
    use crate::world::SEA_LEVEL;
    let (lx, lz) = match side {
        2 | 3 => (0, 1),
        _ => (1, 0),
    };
    let mut top = 0;
    let mut base = Vec::new();
    for o in [-3, 3] {
        let (x, z) = (gx + lx * o, gz + lz * o);
        let h = terrain_height(x, z);
        if h < SEA_LEVEL {
            return;
        }
        base.push((x, z, h));
        top = top.max(h);
    }
    let t = top + 4;
    for (x, z, h) in &base {
        clear_column_above(world, *x, *z, h + 1, t + 2);
        for y in *h..=t {
            world.set_voxel(IVec3::new(*x, y, *z), Voxel::solid(Material::Cobblestone));
        }
    }
    for o in -3..=3 {
        let (x, z) = (gx + lx * o, gz + lz * o);
        clear_column_above(world, x, z, t + 1, t + 2);
        world.set_voxel(
            IVec3::new(x, t + 1, z),
            Voxel::solid(Material::VillageStone),
        );
    }
    clear_column_above(world, gx, gz, t + 2, t + 3);
    world.set_voxel(
        IVec3::new(gx, t + 2, gz),
        Voxel::solid(Material::ManaCrystal),
    );
}

/// Mina: brocal de roca 3×3, tiro 1×1 de 4 de hondo, postes con antorcha y
/// cajón al lado.
fn stamp_mine_shaft(world: &mut World, x: i32, z: i32) {
    use crate::world::SEA_LEVEL;
    let h = terrain_height(x, z);
    if h < SEA_LEVEL + 1 {
        return;
    }
    for dz in -1..=1 {
        for dx in -1..=1 {
            if dx == 0 && dz == 0 {
                continue;
            }
            let (wx, wz) = (x + dx, z + dz);
            let wh = terrain_height(wx, wz);
            clear_column_above(world, wx, wz, wh + 1, wh + 6);
            world.set_voxel(
                IVec3::new(wx, wh, wz),
                Voxel::solid(Material::Cobblestone),
            );
        }
    }
    for y in (h - 3)..=h {
        let _ = world.remove_voxel(IVec3::new(x, y, z));
    }
    // Postes en las esquinas del brocal + antorcha en una + cajón al lado.
    for (dx, dz) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
        let (wx, wz) = (x + dx, z + dz);
        let wh = terrain_height(wx, wz);
        for dy in 1..=2 {
            world.set_voxel(
                IVec3::new(wx, wh + dy, wz),
                Voxel::solid(Material::Wood),
            );
        }
    }
    let (tx, tz) = (x - 1, z - 1);
    let th = terrain_height(tx, tz);
    world.set_voxel(
        IVec3::new(tx, th + 3, tz),
        Voxel::solid(Material::Torch),
    );
    let (bx, bz) = (x + 2, z);
    let bh = terrain_height(bx, bz);
    clear_column_above(world, bx, bz, bh + 1, bh + 3);
    world.set_voxel(
        IVec3::new(bx, bh + 1, bz),
        Voxel::solid(Material::WoodPlanks),
    );
}

/// Muelle: pasarela de tablones desde la orilla (tierra→agua), pilotes a los
/// lados y antorcha al final.
fn stamp_dock(
    world: &mut World,
    x: i32,
    z: i32,
    dx: i32,
    dz: i32,
    x0: i32,
    z0: i32,
    x1: i32,
    z1: i32,
) {
    use crate::world::SEA_LEVEL;
    let in_chunk = |x: i32, z: i32| x >= x0 && x < x1 && z >= z0 && z < z1;
    let deck = SEA_LEVEL + 1;
    for k in 0..=4 {
        let (wx, wz) = (x + dx * k, z + dz * k);
        if !in_chunk(wx, wz) {
            continue;
        }
        let wh = terrain_height(wx, wz);
        let y = wh.max(SEA_LEVEL) + 1;
        clear_column_above(world, wx, wz, y, y + 3);
        world.set_voxel(IVec3::new(wx, y, wz), Voxel::solid(Material::WoodPlanks));
        if wh < SEA_LEVEL && (k == 1 || k == 3) {
            // Pilotes a ambos lados, hundidos en el agua.
            for s in [-1, 1] {
                let (px, pz) = (wx + dz * s, wz + dx * s);
                for yy in (deck - 2)..=deck {
                    world.set_voxel(
                        IVec3::new(px, yy, pz),
                        Voxel::solid(Material::Wood),
                    );
                }
            }
        }
    }
    // Antorcha al final, al lateral (sobre la última losa del muelle).
    let (ex, ez) = (x + dx * 4, z + dz * 4);
    if in_chunk(ex, ez) {
        for yy in (deck - 1)..=deck {
            world.set_voxel(IVec3::new(ex, yy, ez), Voxel::solid(Material::Wood));
        }
        world.set_voxel(
            IVec3::new(ex, deck + 1, ez),
            Voxel::solid(Material::Torch),
        );
    }
}

/// Descanso fuera del muro: 2 bancos + lámpara + 2 jardineras.
fn stamp_rest_stop(
    world: &mut World,
    cx: i32,
    cz: i32,
    _half: i32,
    x0: i32,
    z0: i32,
    x1: i32,
    z1: i32,
) {
    use crate::world::SEA_LEVEL;
    let in_chunk = |x: i32, z: i32| x >= x0 && x < x1 && z >= z0 && z < z1;
    for bx in [cx - 3, cx + 3] {
        for o in -1..=1 {
            let (x, z) = (bx + o, cz);
            if !in_chunk(x, z) {
                continue;
            }
            let h = terrain_height(x, z);
            if h < SEA_LEVEL {
                continue;
            }
            clear_column_above(world, x, z, h + 1, h + 3);
            world.set_voxel(
                IVec3::new(x, h + 1, z),
                Voxel::solid(Material::WoodPlanks),
            );
        }
    }
    if in_chunk(cx, cz) {
        stamp_torch_post(world, cx, cz);
    }
    for (dx, dz) in [(-3, -3), (3, 3)] {
        let (x, z) = (cx + dx, cz + dz);
        if !in_chunk(x, z) {
            continue;
        }
        let h = terrain_height(x, z);
        if h < SEA_LEVEL {
            continue;
        }
        clear_column_above(world, x, z, h + 1, h + 3);
        world.set_voxel(
            IVec3::new(x, h, z),
            Voxel::solid(Material::Cobblestone),
        );
        world.set_voxel(IVec3::new(x, h + 1, z), Voxel::leaves());
    }
}

/// Tendedero: 2 líneas con postes, travesaño y paños alternos.
fn stamp_clothesline_plot(
    world: &mut World,
    cx: i32,
    cz: i32,
    _half: i32,
    x0: i32,
    z0: i32,
    x1: i32,
    z1: i32,
) {
    use crate::world::SEA_LEVEL;
    let in_chunk = |x: i32, z: i32| x >= x0 && x < x1 && z >= z0 && z < z1;
    for oz in [-2, 2] {
        for px in [cx - 3, cx + 3] {
            let (x, z) = (px, cz + oz);
            if !in_chunk(x, z) {
                continue;
            }
            let h = terrain_height(x, z);
            if h < SEA_LEVEL {
                continue;
            }
            clear_column_above(world, x, z, h + 1, h + 5);
            for dy in 1..=3 {
                world.set_voxel(
                    IVec3::new(x, h + dy, z),
                    Voxel::solid(Material::Wood),
                );
            }
        }
        let h0 = terrain_height(cx, cz + oz);
        for o in -3..=3 {
            let (x, z) = (cx + o, cz + oz);
            if !in_chunk(x, z) {
                continue;
            }
            world.set_voxel(
                IVec3::new(x, h0 + 3, z),
                Voxel::solid(Material::WoodPlanks),
            );
            if o == -2 || o == 0 || o == 2 {
                let mat = if o == 0 { Material::Glass } else { Material::Sand };
                world.set_voxel(IVec3::new(x, h0 + 2, z), Voxel::solid(mat));
            }
        }
    }
}

/// Cementerio: anillo de piedra, 5 lápidas, árbol muerto y 2 lámparas.
fn stamp_cemetery(
    world: &mut World,
    cx: i32,
    cz: i32,
    half: i32,
    x0: i32,
    z0: i32,
    x1: i32,
    z1: i32,
) {
    use crate::world::SEA_LEVEL;
    let in_chunk = |x: i32, z: i32| x >= x0 && x < x1 && z >= z0 && z < z1;
    for dz in -half..=half {
        for dx in -half..=half {
            if dx.abs().max(dz.abs()) != half {
                continue;
            }
            let (x, z) = (cx + dx, cz + dz);
            if !in_chunk(x, z) {
                continue;
            }
            let h = terrain_height(x, z);
            if h < SEA_LEVEL {
                continue;
            }
            clear_column_above(world, x, z, h + 1, h + 3);
            world.set_voxel(
                IVec3::new(x, h, z),
                Voxel::solid(Material::VillageStone),
            );
        }
    }
    // Lápidas en posiciones fijas del plano (determinista sin semilla).
    for (i, (ox, oz)) in [(-4, -3), (0, -4), (4, -3), (-2, 1), (2, 1)]
        .into_iter()
        .enumerate()
    {
        let (x, z) = (cx + ox, cz + oz);
        if !in_chunk(x, z) {
            continue;
        }
        let h = terrain_height(x, z);
        if h < SEA_LEVEL {
            continue;
        }
        clear_column_above(world, x, z, h + 1, h + 4);
        world.set_voxel(
            IVec3::new(x, h + 1, z),
            Voxel::solid(Material::VillageStone),
        );
        if i % 2 == 0 {
            world.set_voxel(
                IVec3::new(x, h + 2, z),
                Voxel::solid(Material::VillageStone),
            );
        }
    }
    // Árbol muerto al fondo + lámparas en la entrada sur.
    let (tx, tz) = (cx, cz - half + 2);
    if in_chunk(tx, tz) {
        let h = terrain_height(tx, tz);
        if h >= SEA_LEVEL {
            clear_column_above(world, tx, tz, h + 1, h + 6);
            for dy in 1..=4 {
                world.set_voxel(
                    IVec3::new(tx, h + dy, tz),
                    Voxel::solid(Material::Wood),
                );
            }
        }
    }
    for ox in [-2, 2] {
        let (x, z) = (cx + ox, cz + half);
        if in_chunk(x, z) {
            stamp_torch_post(world, x, z);
        }
    }
}

fn pave_regional_road_shunk(world: &mut World, sx: i32, sz: i32) {    let x0 = sx * SHUNK_SIZE;
    let z0 = sz * SHUNK_SIZE;
    let mid_x = x0 + SHUNK_SIZE / 2;
    let mid_z = z0 + SHUNK_SIZE / 2;
    for t in 0..SHUNK_SIZE {
        for w in -1..=1 {
            pave_road_cell(world, mid_x + w, z0 + t);
            pave_road_cell(world, x0 + t, mid_z + w);
        }
    }
}

fn stamp_prefab_egg(
    world: &mut World,
    egg_x: i32,
    egg_z: i32,
    prefab_path: &str,
    rotation_quarters: u8,
    x0: i32,
    z0: i32,
    x1: i32,
    z1: i32,
) {
    // Nada de casas bajo el mar: la aldea deja la laguna libre.
    if terrain_height(egg_x, egg_z) < crate::world::SEA_LEVEL {
        return;
    }
    let Ok(prefab) = load_prefab(prefab_path) else {
        log::warn!("settlement: failed to load prefab `{prefab_path}`");
        return;
    };
    let h = terrain_height(egg_x, egg_z);
    // Clear vegetation across rotated lot footprint (approx size box).
    let [sx, _sy, sz] = prefab.size;
    let half = sx.max(sz) / 2 + 1;
    for lz in -half..=half {
        for lx in -half..=half {
            let (rdx, rdz) = crate::prefab::rotate_xz(lx, lz, rotation_quarters);
            let wx = egg_x + rdx;
            let wz = egg_z + rdz;
            if wx < x0 || wx >= x1 || wz < z0 || wz >= z1 {
                continue;
            }
            let th = terrain_height(wx, wz);
            clear_column_above(world, wx, wz, th + 1, th + 8);
        }
    }
    for (wx, wy, wz, mat) in placed_solids(&prefab, egg_x, h, egg_z, rotation_quarters) {
        if wx < x0 || wx >= x1 || wz < z0 || wz >= z1 {
            continue;
        }
        world.set_voxel(IVec3::new(wx, wy, wz), voxel_for_material(mat));
    }
}

fn stamp_feature_in_chunk(
    world: &mut World,
    feature: &SettlementFeature,
    x0: i32,
    z0: i32,
    x1: i32,
    z1: i32,
) {
    match &feature.kind {
        FeatureKind::Road => {
            if feature.x >= x0 && feature.x < x1 && feature.z >= z0 && feature.z < z1 {
                pave_road_cell(world, feature.x, feature.z);
            }
        }
        FeatureKind::Wall => {
            if feature.x >= x0 && feature.x < x1 && feature.z >= z0 && feature.z < z1 {
                stamp_wall_cell(world, feature.x, feature.z);
            }
        }
        FeatureKind::Gate => {
            if feature.x >= x0 && feature.x < x1 && feature.z >= z0 && feature.z < z1 {
                stamp_gate_cell(world, feature.x, feature.z);
            }
        }
        FeatureKind::CropPlot { half } => {
            stamp_crop_plot(world, feature.x, feature.z, *half, x0, z0, x1, z1);
        }
        FeatureKind::Lake { radius } => {
            stamp_lake(world, feature.x, feature.z, *radius, x0, z0, x1, z1);
        }
        FeatureKind::Fountain => {
            stamp_fountain(world, feature.x, feature.z, x0, z0, x1, z1);
        }
        FeatureKind::TorchPost => {
            if feature.x >= x0 && feature.x < x1 && feature.z >= z0 && feature.z < z1 {
                stamp_torch_post(world, feature.x, feature.z);
            }
        }
        FeatureKind::Watchtower {
            rotation_quarters,
        } => {
            stamp_prefab_egg(
                world,
                feature.x,
                feature.z,
                WATCHTOWER_PREFAB,
                *rotation_quarters,
                x0,
                z0,
                x1,
                z1,
            );
        }
        FeatureKind::OrchardPlot { half } => {
            stamp_orchard_plot(world, feature.x, feature.z, *half, x0, z0, x1, z1);
        }
        FeatureKind::MissionBoard {
            rotation_quarters,
        } => {
            stamp_prefab_egg(
                world,
                feature.x,
                feature.z,
                MISSION_BOARD_PREFAB,
                *rotation_quarters,
                x0,
                z0,
                x1,
                z1,
            );
        }
        FeatureKind::CellarHouse {
            rotation_quarters,
        } => {
            stamp_cellar_house(
                world,
                feature.x,
                feature.z,
                *rotation_quarters,
                x0,
                z0,
                x1,
                z1,
            );
        }
        FeatureKind::Hamlet {
            prefab,
            rotation_quarters,
        } => {
            stamp_prefab_egg(
                world,
                feature.x,
                feature.z,
                prefab,
                *rotation_quarters,
                x0,
                z0,
                x1,
                z1,
            );
        }
        FeatureKind::GateArch { side } => {
            if feature.x >= x0 && feature.x < x1 && feature.z >= z0 && feature.z < z1 {
                stamp_gate_arch(world, feature.x, feature.z, *side);
            }
        }
        FeatureKind::MineShaft => {
            if feature.x >= x0 && feature.x < x1 && feature.z >= z0 && feature.z < z1 {
                stamp_mine_shaft(world, feature.x, feature.z);
            }
        }
        FeatureKind::Dock { dir_x, dir_z } => {
            stamp_dock(
                world,
                feature.x,
                feature.z,
                *dir_x as i32,
                *dir_z as i32,
                x0,
                z0,
                x1,
                z1,
            );
        }
        FeatureKind::RestStop { half } => {
            stamp_rest_stop(world, feature.x, feature.z, *half, x0, z0, x1, z1);
        }
        FeatureKind::ClotheslinePlot { half } => {
            stamp_clothesline_plot(world, feature.x, feature.z, *half, x0, z0, x1, z1);
        }
        FeatureKind::Cemetery { half } => {
            stamp_cemetery(world, feature.x, feature.z, *half, x0, z0, x1, z1);
        }
        FeatureKind::PrefabEgg {
            prefab,
            rotation_quarters,
        } => {
            stamp_prefab_egg(
                world,
                feature.x,
                feature.z,
                prefab,
                *rotation_quarters,
                x0,
                z0,
                x1,
                z1,
            );
        }
    }
}

/// Stamp settlement roads / walls / gates / houses into one mesh chunk.
///
/// Call after terrain (+ caves) are filled so surface Y is valid. Safe to call
/// more than once for the same chunk (idempotent for the same seed).
pub fn stamp_settlements_in_chunk(world: &mut World, cx: i32, cz: i32) {
    let x0 = cx * MESH_CHUNK_SIZE;
    let z0 = cz * MESH_CHUNK_SIZE;
    let x1 = x0 + MESH_CHUNK_SIZE;
    let z1 = z0 + MESH_CHUNK_SIZE;

    for info in nearby_realm_infos(cx, cz) {
        if realm_road_shunks(&info).contains(&(cx, cz)) {
            pave_regional_road_shunk(world, cx, cz);
        }
        for plan in plans_for_realm(&info) {
            let (pcx, pcz) = plan.center_block;
            // Eggs + tower + outskirt plots (1/3 size) reach wall+26.
            let r = plan.wall_radius + 26;
            if pcx + r < x0 || pcx - r >= x1 || pcz + r < z0 || pcz - r >= z1 {
                continue;
            }
            let mut features = plan.features;
            features.sort_by_key(|f| feature_priority(&f.kind));
            for feature in &features {
                stamp_feature_in_chunk(world, feature, x0, z0, x1, z1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::realms::{realm_info, RealmId};
    use crate::world::{terrain_height, Material};

    fn sample_info() -> RealmInfo {
        for iz in -8..8 {
            for ix in -8..8 {
                if let Some(info) = realm_info(RealmId { ix, iz }) {
                    return info;
                }
            }
        }
        panic!("expected realm in sample");
    }

    fn fill_around(world: &mut World, cx: i32, cz: i32, radius_chunks: i32) {
        for dz in -radius_chunks..=radius_chunks {
            for dx in -radius_chunks..=radius_chunks {
                let mc = cx + dx;
                let mz = cz + dz;
                let x0 = mc * MESH_CHUNK_SIZE;
                let z0 = mz * MESH_CHUNK_SIZE;
                for z in z0..z0 + MESH_CHUNK_SIZE {
                    for x in x0..x0 + MESH_CHUNK_SIZE {
                        if world.column_height(x, z).is_none() {
                            world.fills_column_for_test(x, z, terrain_height(x, z));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn villages_claim_their_footprint() {
        let info = sample_info();
        let plans = plans_for_realm(&info);
        let village = plans
            .iter()
            .find(|p| p.kind == SettlementKind::Village)
            .expect("aldea");
        let (cx, cz) = village.center_block;
        assert!(settlement_claims_block(cx, cz));
        assert!(settlement_claims_block(
            cx + VILLAGE_WALL_RADIUS + 2,
            cz
        ));
        assert!(!settlement_claims_block(
            cx + VILLAGE_WALL_RADIUS + 40,
            cz + VILLAGE_WALL_RADIUS + 40
        ));
    }

    #[test]
    fn cached_claim_matches_uncached_everywhere() {
        let info = sample_info();
        let plans = plans_for_realm(&info);
        let village = plans
            .iter()
            .find(|p| p.kind == SettlementKind::Village)
            .expect("aldea");
        let (cx, cz) = village.center_block;
        let r = VILLAGE_WALL_RADIUS + 6;
        for z in cz - r..=cz + r {
            for x in cx - r..=cx + r {
                assert_eq!(
                    settlement_claims_block_cached(x, z),
                    settlement_claims_block(x, z),
                    "cache disagree at {x},{z}"
                );
            }
        }
    }

    #[test]
    fn settlements_stay_free_of_worldgen_flora() {
        let info = sample_info();
        let plans = plans_for_realm(&info);
        let village = plans
            .iter()
            .find(|p| p.kind == SettlementKind::Village)
            .expect("aldea");
        let (vx, vz) = village.center_block;
        // Fruit trees / crop leaves are stamped by the settlement itself, so they
        // are legal inside the walls. Everything else must not grow there.
        let planted: Vec<(i32, i32, i32)> = village
            .features
            .iter()
            .filter(|f| {
                matches!(
                    f.kind,
                    FeatureKind::OrchardPlot { .. } | FeatureKind::CropPlot { .. }
                )
            })
            .map(|f| {
                let half = match f.kind {
                    FeatureKind::OrchardPlot { half } | FeatureKind::CropPlot { half } => half,
                    _ => 0,
                };
                (f.x, f.z, half + 1)
            })
            .collect();

        let mut world = World::new();
        let (mcx, mcz) = crate::world::mesh_chunk_coord(vx, vz);
        // Fill around the *village*, not the origin, or the scan sees no columns
        // and passes vacuously.
        let window = VILLAGE_WALL_RADIUS / MESH_CHUNK_SIZE + 2;
        fill_around(&mut world, mcx, mcz, window);
        world.seed_flora_for_test(vx, vz, window);

        let r = VILLAGE_WALL_RADIUS - 2;
        let mut flora_inside = 0usize;
        for z in vz - r..=vz + r {
            for x in vx - r..=vx + r {
                let settlement_planted = planted
                    .iter()
                    .any(|&(px, pz, half)| (px - x).abs() <= half && (pz - z).abs() <= half);
                let h = terrain_height(x, z);
                for y in h..h + 8 {
                    let Some(v) = world.get_voxel(IVec3::new(x, y, z)) else {
                        continue;
                    };
                    // Settlements never stamp grass — always worldgen.
                    assert_ne!(
                        v.material,
                        Material::Grass,
                        "hierbajos de worldgen dentro de la aldea en {x},{y},{z}"
                    );
                    if settlement_planted {
                        continue;
                    }
                    assert!(
                        !matches!(
                            v.material,
                            Material::Wood
                                | Material::Leaves
                                | Material::BirchWood
                                | Material::BirchLeaves
                        ),
                        "árbol de worldgen dentro de la aldea en {x},{y},{z}: {:?}",
                        v.material
                    );
                }
            }
        }
        assert_eq!(flora_inside, 0, "flora de worldgen dentro de la aldea");

        // Positive control: the same pass must still grow things in the
        // wilderness, otherwise this test would pass vacuously.
        let wild = VILLAGE_WALL_RADIUS + 6;
        let wild_span = 40;
        let mut flora_outside = 0usize;
        for z in vz + wild..vz + wild + wild_span {
            for x in vx - wild_span / 2..vx + wild_span / 2 {
                for y in terrain_height(x, z)..terrain_height(x, z) + 8 {
                    if world
                        .get_voxel(IVec3::new(x, y, z))
                        .is_some_and(|v| matches!(v.material, Material::Grass))
                    {
                        flora_outside += 1;
                    }
                }
            }
        }
        assert!(
            flora_outside > 0,
            "sin hierbajos en la naturaleza selvatique: el test no probaria nada"
        );
    }

    #[test]
    fn plans_are_deterministic_and_have_eggs() {
        let info = sample_info();
        let a = plans_for_realm(&info);
        let b = plans_for_realm(&info);
        assert_eq!(a, b);
        assert!(a.iter().all(|p| p
            .features
            .iter()
            .any(|f| { matches!(f.kind, FeatureKind::PrefabEgg { .. }) })));
    }

    #[test]
    fn every_plan_has_wide_gates_and_walls() {
        let info = sample_info();
        for plan in plans_for_realm(&info) {
            let gates = plan
                .features
                .iter()
                .filter(|f| f.kind == FeatureKind::Gate)
                .count();
            let walls = plan
                .features
                .iter()
                .filter(|f| f.kind == FeatureKind::Wall)
                .count();
            // 3-wide opening per cardinal side (matches the 3-wide roads).
            assert_eq!(gates, 12);
            assert!(walls > 20);
        }
    }

    #[test]
    fn villages_are_roomier_with_three_wide_gates() {
        let info = sample_info();
        let villages: Vec<_> = plans_for_realm(&info)
            .into_iter()
            .filter(|p| p.kind == SettlementKind::Village)
            .collect();
        assert!(!villages.is_empty());
        for plan in &villages {
            assert_eq!(plan.wall_radius, 30);
            let (cx, cz) = plan.center_block;
            let r = plan.wall_radius;
            // Each cardinal side: 3 contiguous gate cells, flanked by wall.
            for (gx, gz) in [(cx, cz - r), (cx, cz + r), (cx - r, cz), (cx + r, cz)] {
                let (px, pz) = if gx == cx { (1, 0) } else { (0, 1) };
                for o in -1..=1 {
                    let cell = plan
                        .features
                        .iter()
                        .find(|f| f.x == gx + px * o && f.z == gz + pz * o)
                        .expect("gate cell");
                    assert_eq!(cell.kind, FeatureKind::Gate);
                }
                for o in [2, -2] {
                    let flank = plan
                        .features
                        .iter()
                        .find(|f| f.x == gx + px * o && f.z == gz + pz * o)
                        .expect("flank cell");
                    assert_eq!(flank.kind, FeatureKind::Wall);
                }
            }
            let eggs = plan
                .features
                .iter()
                .filter(|f| matches!(f.kind, FeatureKind::PrefabEgg { .. }))
                .count();
            assert_eq!(eggs, 8);
            // Houses keep air: pairwise lots never touch (widest prefab is 9).
            let lots: Vec<(i32, i32)> = plan
                .features
                .iter()
                .filter_map(|f| match f.kind {
                    FeatureKind::PrefabEgg { .. } => Some((f.x, f.z)),
                    _ => None,
                })
                .collect();
            for a in 0..lots.len() {
                // Clear of the walls too (house half-width ≤ 5).
                assert!((lots[a].0 - cx).abs() <= r - 6);
                assert!((lots[a].1 - cz).abs() <= r - 6);
                for b in a + 1..lots.len() {
                    let d = (lots[a].0 - lots[b].0)
                        .abs()
                        .max((lots[a].1 - lots[b].1).abs());
                    assert!(d >= 10, "eggs {a}/{b} overlap in {plan:?}");
                }
            }
        }
    }

    #[test]
    fn roads_connect_capital_and_villages() {
        let info = sample_info();
        let road = realm_road_shunks(&info);
        assert!(road.contains(&info.capital));
        for village in &info.villages {
            assert!(road.contains(village), "road misses village {village:?}");
        }
    }

    #[test]
    fn eggs_stay_inside_walls_and_off_center() {
        let info = sample_info();
        for plan in plans_for_realm(&info) {
            for f in &plan.features {
                if !matches!(f.kind, FeatureKind::PrefabEgg { .. }) {
                    continue;
                }
                let dx = (f.x - plan.center_block.0).abs();
                let dz = (f.z - plan.center_block.1).abs();
                assert!(dx < plan.wall_radius - 1);
                assert!(dz < plan.wall_radius - 1);
                assert!(dx > 1 || dz > 1);
            }
        }
    }

    #[test]
    fn plans_include_crops_lakes_and_diverse_prefabs() {
        let info = sample_info();
        let capital = settlement_plan(info.id, info.capital, SettlementKind::Capital);
        assert!(capital
            .features
            .iter()
            .any(|f| matches!(f.kind, FeatureKind::CropPlot { .. })));
        assert!(capital
            .features
            .iter()
            .any(|f| matches!(f.kind, FeatureKind::Lake { .. })));
        let prefabs: Vec<_> = capital
            .features
            .iter()
            .filter_map(|f| match &f.kind {
                FeatureKind::PrefabEgg { prefab, .. } => Some(*prefab),
                _ => None,
            })
            .collect();
        assert!(prefabs.len() >= 4);
        let unique: std::collections::HashSet<_> = prefabs.iter().copied().collect();
        assert!(
            unique.len() >= 3,
            "capital should mix building types, got {unique:?}"
        );
    }

    #[test]
    fn stamp_is_deterministic_and_places_wall_gate_prefab() {
        let info = sample_info();
        let plan = settlement_plan(info.id, info.capital, SettlementKind::Capital);
        let (bcx, bcz) = plan.center_block;
        let mcx = bcx.div_euclid(MESH_CHUNK_SIZE);
        let mcz = bcz.div_euclid(MESH_CHUNK_SIZE);

        let wall = plan
            .features
            .iter()
            .find(|f| f.kind == FeatureKind::Wall)
            .expect("wall");
        let gate = plan
            .features
            .iter()
            .find(|f| f.kind == FeatureKind::Gate)
            .expect("gate");
        let egg = plan
            .features
            .iter()
            .find(|f| matches!(f.kind, FeatureKind::PrefabEgg { .. }))
            .expect("egg");

        let mut world_a = World::new();
        fill_around(&mut world_a, mcx, mcz, 2);
        for dz in -2..=2 {
            for dx in -2..=2 {
                stamp_settlements_in_chunk(&mut world_a, mcx + dx, mcz + dz);
            }
        }

        let mut world_b = World::new();
        fill_around(&mut world_b, mcx, mcz, 2);
        for dz in -2..=2 {
            for dx in -2..=2 {
                stamp_settlements_in_chunk(&mut world_b, mcx + dx, mcz + dz);
            }
        }

        let wh = terrain_height(wall.x, wall.z);
        let wall_mat = world_a
            .get_voxel(IVec3::new(wall.x, wh + 2, wall.z))
            .map(|v| v.material);
        assert_eq!(wall_mat, Some(Material::Cobblestone));
        assert_eq!(
            wall_mat,
            world_b
                .get_voxel(IVec3::new(wall.x, wh + 2, wall.z))
                .map(|v| v.material)
        );

        let gh = terrain_height(gate.x, gate.z);
        assert!(
            world_a
                .get_voxel(IVec3::new(gate.x, gh + 2, gate.z))
                .is_none(),
            "gate should leave an opening above the road"
        );
        assert_eq!(
            world_a
                .get_voxel(IVec3::new(gate.x, gh, gate.z))
                .map(|v| v.material),
            Some(Material::Stone)
        );

        let eh = terrain_height(egg.x, egg.z);
        let mut found_house = false;
        let mut found_glass = false;
        let mut found_door = false;
        for dy in 0..=8 {
            for dz in -5..=5 {
                for dx in -5..=5 {
                    let pos = IVec3::new(egg.x + dx, eh + dy, egg.z + dz);
                    if let Some(v) = world_a.get_voxel(pos) {
                        if v.material == Material::WoodPlanks {
                            found_house = true;
                        }
                        if v.material == Material::Glass {
                            found_glass = true;
                        }
                        if v.material == Material::Door {
                            found_door = true;
                        }
                    }
                }
            }
        }
        assert!(found_house, "prefab egg should stamp wood walls/roof");
        assert!(found_glass, "prefab egg should stamp glass window panes");
        assert!(found_door, "prefab egg should stamp a door");
        assert_eq!(
            world_a
                .get_voxel(IVec3::new(egg.x, eh, egg.z))
                .map(|v| v.material),
            world_b
                .get_voxel(IVec3::new(egg.x, eh, egg.z))
                .map(|v| v.material)
        );
    }

    /// Stamp helper: cover the mesh chunk holding (x, z) plus neighbors.
    fn stamp_covering(world: &mut World, x: i32, z: i32) {
        let mcx = x.div_euclid(MESH_CHUNK_SIZE);
        let mcz = z.div_euclid(MESH_CHUNK_SIZE);
        fill_around(world, mcx, mcz, 1);
        for dz in -1..=1 {
            for dx in -1..=1 {
                stamp_settlements_in_chunk(world, mcx + dx, mcz + dz);
            }
        }
    }

    fn sample_village() -> (RealmInfo, SettlementPlan) {
        let info = sample_info();
        let village = plans_for_realm(&info)
            .into_iter()
            .find(|p| p.kind == SettlementKind::Village)
            .expect("aldea");
        (info, village)
    }

    #[test]
    fn roads_use_distinct_road_cobble() {
        let (_info, village) = sample_village();
        let road = village
            .features
            .iter()
            .find(|f| f.kind == FeatureKind::Road)
            .expect("camino");
        let mut world = World::new();
        stamp_covering(&mut world, road.x, road.z);
        let h = terrain_height(road.x, road.z);
        assert_eq!(
            world
                .get_voxel(IVec3::new(road.x, h, road.z))
                .map(|v| v.material),
            Some(Material::RoadCobble),
            "roads must pave ochre cobble, not wall grey"
        );
        assert_ne!(
            Material::RoadCobble.color_rgb(),
            Material::Cobblestone.color_rgb()
        );
    }

    #[test]
    fn torch_posts_ring_gates_and_avoid_roads() {
        let (_info, village) = sample_village();
        let (cx, cz) = village.center_block;
        let r = village.wall_radius;
        let torches: Vec<_> = village
            .features
            .iter()
            .filter(|f| f.kind == FeatureKind::TorchPost)
            .collect();
        assert!(!torches.is_empty(), "village needs torches");
        let roads: Vec<(i32, i32)> = village
            .features
            .iter()
            .filter(|f| f.kind == FeatureKind::Road)
            .map(|f| (f.x, f.z))
            .collect();
        for t in &torches {
            assert!(
                !roads.contains(&(t.x, t.z)),
                "torch on road cell ({},{})",
                t.x,
                t.z
            );
        }
        // Ring torches sit exactly one out of the wall.
        assert!(
            torches.iter().any(|t| {
                let dx = (t.x - cx).abs();
                let dz = (t.z - cz).abs();
                dx.max(dz) == r + 1
            }),
            "expected wall-ring torches"
        );
        // Gate flanks: ±2 lateral, one out.
        for (gx, gz) in [(cx, cz - r), (cx, cz + r), (cx - r, cz), (cx + r, cz)] {
            let flanked = if gx == cx {
                torches
                    .iter()
                    .any(|t| t.z == gz + (gz - cz).signum() && (t.x - cx).abs() == 2)
            } else {
                torches
                    .iter()
                    .any(|t| t.x == gx + (gx - cx).signum() && (t.z - cz).abs() == 2)
            };
            assert!(flanked, "gate ({gx},{gz}) needs flank torches");
        }
        // Stamp one: cobble foot + wood post + flame.
        let t = torches[0];
        let mut world = World::new();
        stamp_covering(&mut world, t.x, t.z);
        let h = terrain_height(t.x, t.z);
        assert_eq!(
            world
                .get_voxel(IVec3::new(t.x, h + 3, t.z))
                .map(|v| v.material),
            Some(Material::Torch)
        );
    }

    #[test]
    fn watchtower_is_extra_muros_with_flame() {
        let (_info, village) = sample_village();
        let (cx, cz) = village.center_block;
        let r = village.wall_radius;
        let tower = village
            .features
            .iter()
            .find(|f| matches!(f.kind, FeatureKind::Watchtower { .. }))
            .expect("atalaya");
        let d = (tower.x - cx).abs().max((tower.z - cz).abs());
        assert!(d > r, "watchtower must stand outside the wall (d={d}, r={r})");
        let mut world = World::new();
        stamp_covering(&mut world, tower.x, tower.z);
        let h = terrain_height(tower.x, tower.z);
        let mut flame = false;
        for dy in 0..=9 {
            for dz in -3..=3 {
                for dx in -3..=3 {
                    if world
                        .get_voxel(IVec3::new(tower.x + dx, h + dy, tower.z + dz))
                        .is_some_and(|v| v.material == Material::Torch)
                    {
                        flame = true;
                    }
                }
            }
        }
        assert!(flame, "watchtower should crown a torch flame");
    }

    #[test]
    fn village_grows_a_mage_tower_with_mana() {
        let (_info, village) = sample_village();
        let tower_egg = village
            .features
            .iter()
            .filter_map(|f| match &f.kind {
                FeatureKind::PrefabEgg { prefab, .. } => Some(*prefab),
                _ => None,
            })
            .any(|p| p.contains("wizard_tower"));
        assert!(tower_egg, "every village grows one mage tower");
        let prefab =
            crate::prefab::load_prefab("assets/prefabs/wizard_tower.json").expect("load");
        assert!(
            prefab
                .solids
                .iter()
                .any(|&(_, _, _, m)| m == Material::ManaCrystal),
            "mage tower must crown raw mana crystal"
        );
        let board =
            crate::prefab::load_prefab("assets/prefabs/watchtower.json").expect("load");
        assert!(
            board
                .solids
                .iter()
                .any(|&(_, _, _, m)| m == Material::Torch),
            "watchtower needs its flame"
        );
        let missions =
            crate::prefab::load_prefab("assets/prefabs/mission_board.json").expect("load");
        assert!(
            missions
                .solids
                .iter()
                .any(|&(_, _, _, m)| m == Material::Sand),
            "mission board pins sand notes"
        );
    }

    #[test]
    fn orchard_board_and_cellar_join_the_village() {
        let (_info, village) = sample_village();
        let (cx, cz) = village.center_block;
        let r = village.wall_radius;
        let orchards = village
            .features
            .iter()
            .filter(|f| matches!(f.kind, FeatureKind::OrchardPlot { .. }))
            .count();
        assert_eq!(orchards, 2, "two outskirt orchards");
        assert!(village
            .features
            .iter()
            .any(|f| matches!(f.kind, FeatureKind::MissionBoard { .. })));
        let cellar = village
            .features
            .iter()
            .find(|f| matches!(f.kind, FeatureKind::CellarHouse { .. }))
            .expect("casa con sótano");
        let dc = (cellar.x - cx).abs().max((cellar.z - cz).abs());
        assert!(dc < r, "cellar house inside the walls");
    }

    #[test]
    fn orchard_grows_apples_and_hive() {
        let (_info, village) = sample_village();
        let orchard = village
            .features
            .iter()
            .find(|f| matches!(f.kind, FeatureKind::OrchardPlot { .. }))
            .expect("huerto");
        let mut world = World::new();
        stamp_covering(&mut world, orchard.x, orchard.z);
        let h = terrain_height(orchard.x, orchard.z);
        let mut apples = 0;
        let mut hive = false;
        for dy in 0..=7 {
            for dz in -4..=4 {
                for dx in -4..=4 {
                    match world
                        .get_voxel(IVec3::new(orchard.x + dx, h + dy, orchard.z + dz))
                        .map(|v| v.material)
                    {
                        Some(Material::Apple) => apples += 1,
                        Some(Material::Beehive) => hive = true,
                        _ => {}
                    }
                }
            }
        }
        assert!(apples >= 4, "orchard needs apple dots, got {apples}");
        assert!(hive, "orchard needs its beehive");
    }

    #[test]
    fn cellar_house_hides_chest_and_shaft() {        let (_info, village) = sample_village();
        let cellar = village
            .features
            .iter()
            .find(|f| matches!(f.kind, FeatureKind::CellarHouse { .. }))
            .expect("sótano");
        let mut world = World::new();
        stamp_covering(&mut world, cellar.x, cellar.z);
        let h = terrain_height(cellar.x, cellar.z);
        // Chest below the floor with air above (cave-crate rules).
        assert_eq!(
            world
                .get_voxel(IVec3::new(cellar.x, h - 4, cellar.z))
                .map(|v| v.material),
            Some(Material::Chest)
        );
        assert!(
            world
                .get_voxel(IVec3::new(cellar.x, h - 3, cellar.z))
                .is_none(),
            "chest needs air above"
        );
        // Entrance shaft open to the sky.
        assert!(
            world
                .get_voxel(IVec3::new(cellar.x + 1, h, cellar.z + 1))
                .is_none(),
            "shaft must punch the floor"
        );
        // Deterministic: second world matches.
        let mut world_b = World::new();
        stamp_covering(&mut world_b, cellar.x, cellar.z);
        assert_eq!(
            world
                .get_voxel(IVec3::new(cellar.x, h - 4, cellar.z))
                .map(|v| v.material),
            world_b
                .get_voxel(IVec3::new(cellar.x, h - 4, cellar.z))
                .map(|v| v.material)
        );
    }

    #[test]
    fn hamlet_jsons_load_with_props() {
        for (path, mat) in [
            ("assets/prefabs/tavern.json", Material::VillageStone),
            ("assets/prefabs/barn.json", Material::WoodPlanks),
            ("assets/prefabs/chapel.json", Material::Glass),
            ("assets/prefabs/herbalist.json", Material::Leaves),
            ("assets/prefabs/stable.json", Material::WoodPlanks),
            ("assets/prefabs/sawmill.json", Material::Wood),
            ("assets/prefabs/library.json", Material::Glass),
            ("assets/prefabs/barracks.json", Material::Cobblestone),
            ("assets/prefabs/lighthouse.json", Material::Torch),
            ("assets/prefabs/market_stall.json", Material::Apple),
            ("assets/prefabs/smith_stall.json", Material::BlackStone),
            ("assets/prefabs/potion_stall.json", Material::Glass),
            ("assets/prefabs/barter_stall.json", Material::Emerald),
            ("assets/prefabs/cart.json", Material::WoodPlanks),
        ] {
            let p = crate::prefab::load_prefab(path)
                .unwrap_or_else(|e| panic!("{path}: {e}"));
            assert!(
                p.solids.iter().any(|&(_, _, _, m)| m == mat),
                "{path} should contain {mat:?}"
            );
        }
    }

    fn hamlet_lots(plan: &SettlementPlan) -> Vec<(i32, i32)> {
        plan.features
            .iter()
            .filter_map(|f| match &f.kind {
                FeatureKind::Hamlet { .. } => Some((f.x, f.z)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn hamlets_populate_free_lots() {
        let (_info, village) = sample_village();
        let (cx, cz) = village.center_block;
        let r = village.wall_radius;
        let lots = hamlet_lots(&village);
        // 4 buildings + market cluster + 3 stalls + cart (mine/dock separate).
        assert!(
            lots.len() >= 4,
            "village should grow hamlets, got {}",
            lots.len()
        );
        let eggs: Vec<(i32, i32)> = village
            .features
            .iter()
            .filter_map(|f| match f.kind {
                FeatureKind::PrefabEgg { .. } => Some((f.x, f.z)),
                _ => None,
            })
            .collect();
        for (x, z) in &lots {
            // Inside the walls, off the cross roads, above sea.
            assert!((x - cx).abs() <= r - 8 && (z - cz).abs() <= r - 8);
            assert!((x - cx).abs() > 2 && (z - cz).abs() > 2);
            assert!(terrain_height(*x, *z) >= crate::world::SEA_LEVEL);
            for (ex, ez) in &eggs {
                let d = (ex - x).abs().max((ez - z).abs());
                assert!(d >= 10, "hamlet ({x},{z}) crowds house ({ex},{ez})");
            }
        }
        for a in 0..lots.len() {
            for b in a + 1..lots.len() {
                // Market arms sit ±3 by design; distinct lots keep air.
                let d = (lots[a].0 - lots[b].0)
                    .abs()
                    .max((lots[a].1 - lots[b].1).abs());
                assert!(d >= 3, "hamlets {a}/{b} overlap");
            }
        }
        // Every hamlet prefab stamps wood walls (smoke test one).
        let (hx, hz) = lots[0];
        let mut world = World::new();
        stamp_covering(&mut world, hx, hz);
        let h = terrain_height(hx, hz);
        let mut wood = false;
        for dy in 0..=10 {
            for dz in -6..=6 {
                for dx in -6..=6 {
                    if world
                        .get_voxel(IVec3::new(hx + dx, h + dy, hz + dz))
                        .is_some_and(|v| {
                            v.material == Material::WoodPlanks || v.material == Material::Wood
                        })
                    {
                        wood = true;
                    }
                }
            }
        }
        assert!(wood, "hamlet should stamp wooden walls");
    }

    #[test]
    fn gate_arch_crowns_a_random_gate() {
        let (_info, village) = sample_village();
        let (cx, cz) = village.center_block;
        let r = village.wall_radius;
        let arches: Vec<_> = village
            .features
            .iter()
            .filter_map(|f| match f.kind {
                FeatureKind::GateArch { side } => Some((f.x, f.z, side)),
                _ => None,
            })
            .collect();
        assert_eq!(arches.len(), 1, "one emblem arch per village");
        let (gx, gz, side) = arches[0];
        assert!(side < 4);
        let (ex, ez) = match side {
            0 => (cx, cz - r - 1),
            1 => (cx, cz + r + 1),
            2 => (cx - r - 1, cz),
            _ => (cx + r + 1, cz),
        };
        assert_eq!((gx, gz), (ex, ez));
        // Stamp: cobble pillars + village-stone lintel + mana emblem.
        let mut world = World::new();
        stamp_covering(&mut world, gx, gz);
        let h = terrain_height(gx, gz);
        let mut pillars = 0;
        let mut lintel = false;
        let mut emblem = false;
        for dy in 0..=9 {
            for dz in -4..=4 {
                for dx in -4..=4 {
                    match world
                        .get_voxel(IVec3::new(gx + dx, h + dy, gz + dz))
                        .map(|v| v.material)
                    {
                        Some(Material::Cobblestone) => {
                            if dx.abs() == 3 {
                                pillars += 1;
                            }
                        }
                        Some(Material::VillageStone) => lintel = true,
                        Some(Material::ManaCrystal) => emblem = true,
                        _ => {}
                    }
                }
            }
        }
        assert!(pillars >= 4, "arch needs tall pillars");
        assert!(lintel, "arch needs its lintel");
        assert!(emblem, "arch crowns a mana emblem");
    }

    #[test]
    fn mine_shaft_has_pit_beams_and_crate() {
        let (_info, village) = sample_village();
        let mine = village
            .features
            .iter()
            .find(|f| f.kind == FeatureKind::MineShaft)
            .expect("mina");
        let mut world = World::new();
        stamp_covering(&mut world, mine.x, mine.z);
        let h = terrain_height(mine.x, mine.z);
        // Open pit below the rim.
        assert!(
            world.get_voxel(IVec3::new(mine.x, h - 1, mine.z)).is_none()
                || world
                    .get_voxel(IVec3::new(mine.x, h - 1, mine.z))
                    .is_some_and(|v| v.material == Material::Torch),
            "shaft must be open"
        );
        // Rim + crate + torch around it.
        let mut rim = false;
        let mut torch = false;
        for dz in -2..=2 {
            for dx in -2..=2 {
                for dy in 0..=4 {
                    match world
                        .get_voxel(IVec3::new(mine.x + dx, h + dy, mine.z + dz))
                        .map(|v| v.material)
                    {
                        Some(Material::Cobblestone) => rim = true,
                        Some(Material::Torch) => torch = true,
                        _ => {}
                    }
                }
            }
        }
        assert!(rim, "mine needs its cobble rim");
        assert!(torch, "mine needs its lamp");
    }

    #[test]
    fn dock_links_shore_or_skips_cleanly() {
        let (_info, village) = sample_village();
        let Some(dock) = village.features.iter().find(|f| {
            matches!(
                f.kind,
                FeatureKind::Dock {
                    dir_x: 1 | -1,
                    dir_z: 0
                } | FeatureKind::Dock {
                    dir_x: 0,
                    dir_z: 1 | -1
                }
            )
        }) else {
            // Landlocked village: no dock, no panic — clean skip.
            return;
        };
        let (dx, dz) = match dock.kind {
            FeatureKind::Dock { dir_x, dir_z } => (dir_x as i32, dir_z as i32),
            _ => unreachable!(),
        };
        assert!(dx.abs() + dz.abs() == 1);
        let mut world = World::new();
        stamp_covering(&mut world, dock.x, dock.z);
        let h = terrain_height(dock.x, dock.z);
        assert!(h >= crate::world::SEA_LEVEL, "dock starts on land");
        let mut planks = 0;
        let mut torch = false;
        for k in 0..=5 {
            for dy in -3..=3 {
                match world
                    .get_voxel(IVec3::new(dock.x + dx * k, h + dy, dock.z + dz * k))
                    .map(|v| v.material)
                {
                    Some(Material::WoodPlanks) => planks += 1,
                    Some(Material::Torch) => torch = true,
                    _ => {}
                }
            }
        }
        assert!(planks >= 3, "dock needs its walkway");
        assert!(torch, "dock needs its end lamp");
    }

    #[test]
    fn outskirts_scale_third_and_stay_outside() {
        let (_info, village) = sample_village();
        let (cx, cz) = village.center_block;
        let r = village.wall_radius;
        let mut found = 0;
        for f in &village.features {
            let (kind_half, name) = match f.kind {
                FeatureKind::RestStop { half } => (half, "rest"),
                FeatureKind::ClotheslinePlot { half } => (half, "line"),
                FeatureKind::Cemetery { half } => (half, "graves"),
                _ => continue,
            };
            found += 1;
            assert_eq!(kind_half, r / 3, "{name} sized at a third of the wall");
            let d = (f.x - cx).abs().max((f.z - cz).abs());
            assert!(d > r, "{name} must stand outside the walls");
        }
        assert_eq!(found, 3, "rest + line + cemetery outside");
        // Stamp the cemetery: graves + lamps + dead tree.
        let grave = village
            .features
            .iter()
            .find(|f| matches!(f.kind, FeatureKind::Cemetery { .. }))
            .expect("cementerio");
        let mut world = World::new();
        stamp_covering(&mut world, grave.x, grave.z);
        let h = terrain_height(grave.x, grave.z);
        let mut slabs = 0;
        let mut torch = false;
        for dz in -12..=12 {
            for dx in -12..=12 {
                for dy in 0..=4 {
                    match world
                        .get_voxel(IVec3::new(grave.x + dx, h + dy, grave.z + dz))
                        .map(|v| v.material)
                    {
                        Some(Material::VillageStone) => slabs += 1,
                        Some(Material::Torch) => torch = true,
                        _ => {}
                    }
                }
            }
        }
        assert!(slabs >= 5, "cemetery needs its slabs");
        assert!(torch, "cemetery needs lamps");
    }
}
