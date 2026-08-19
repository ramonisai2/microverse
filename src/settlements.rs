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
    mix_seed, terrain_height, Material, Voxel, World, MESH_CHUNK_SIZE, SHUNK_SIZE, WORLD_SEED,
};
use glam::IVec3;

pub const DEFAULT_HOUSE_PREFAB: &str = "assets/prefabs/house_basic.json";
const SETTLEMENT_SEED: u32 = 0x5E77_1E55;

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
];

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
        FeatureKind::CropPlot { .. } | FeatureKind::Lake { .. } => 1,
        FeatureKind::Wall => 2,
        FeatureKind::Gate => 3,
        FeatureKind::PrefabEgg { .. } => 4,
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

/// Local wall + gates + roads + crops/lakes + prefab lots for one landmark.
pub fn settlement_plan(
    realm: RealmId,
    center_shunk: (i32, i32),
    kind: SettlementKind,
) -> SettlementPlan {
    let (cx, cz) = shunk_center_block(center_shunk);
    let radius = match kind {
        SettlementKind::Capital => 18,
        SettlementKind::Village => 12,
    };
    let mut features = Vec::new();

    // Square palisade. Four cardinal cells become gates.
    for d in -radius..=radius {
        for (x, z) in [
            (cx + d, cz - radius),
            (cx + d, cz + radius),
            (cx - radius, cz + d),
            (cx + radius, cz + d),
        ] {
            let gate = (x == cx && (z - cz).abs() == radius)
                || (z == cz && (x - cx).abs() == radius);
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

    // Prefab eggs reserve lots between roads and wall.
    let lot = match kind {
        SettlementKind::Capital => 8,
        SettlementKind::Village => 6,
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
        SettlementKind::Village => &[(-1, -1), (1, -1), (-1, 1), (1, 1)],
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
        let prefab = catalog[i % catalog.len()];
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

fn plans_cache() -> &'static std::sync::Mutex<std::collections::HashMap<RealmId, Vec<SettlementPlan>>> {
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
    world.set_voxel(IVec3::new(x, h, z), Voxel::stone());
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

fn pave_regional_road_shunk(world: &mut World, sx: i32, sz: i32) {
    let x0 = sx * SHUNK_SIZE;
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
            let r = plan.wall_radius + 12; // eggs + house / tower footprint + plots
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
    fn plans_are_deterministic_and_have_eggs() {
        let info = sample_info();
        let a = plans_for_realm(&info);
        let b = plans_for_realm(&info);
        assert_eq!(a, b);
        assert!(a.iter().all(|p| p.features.iter().any(|f| {
            matches!(f.kind, FeatureKind::PrefabEgg { .. })
        })));
    }

    #[test]
    fn every_plan_has_four_gates_and_walls() {
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
            assert_eq!(gates, 4);
            assert!(walls > 20);
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
            world_a.get_voxel(IVec3::new(egg.x, eh, egg.z)).map(|v| v.material),
            world_b.get_voxel(IVec3::new(egg.x, eh, egg.z)).map(|v| v.material)
        );
    }
}
