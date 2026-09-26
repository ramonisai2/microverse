//! Horizontal realm patches on the shunk grid (biome-like kingdoms).
//!
//! - Area budget per realm: [`REALM_AREA_MIN`]..=[`REALM_AREA_MAX`] shunks².
//! - ~25–35% of shunks stay [`RealmCell::Wild`] (unclaimed).
//! - Each realm has ≥1 capital + ≥1 village (more villages on larger realms).
//! - Query is deterministic from [`WORLD_SEED`] — no stored map required.
//!
//! This module assigns ownership and landmark shunks. [`crate::settlements`]
//! expands those landmarks into roads, walls, gates and prefab-house eggs.

use crate::world::{mix_seed, SHUNK_SIZE, WORLD_SEED};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Mix seed for realm layout (independent of caves / trees).
pub const REALM_SEED: u32 = 0x8EA1_7001;
/// Minimum claimed area (shunks²).
pub const REALM_AREA_MIN: i32 = 55;
/// Maximum claimed area (shunks²).
pub const REALM_AREA_MAX: i32 = 220;
/// Candidate seed every N shunks on each axis.
pub const REALM_SEED_CELL: i32 = 16;
/// Chance a grid node hosts a realm seed (rest stay wild gaps).
const REALM_SPAWN_PCT: u32 = 88;
/// Inflate disk radius slightly so Voronoi+budget lands near ~65–75% coverage.
const RADIUS_AREA_MUL: f32 = 1.28;
/// Capital↔village separation (shunks) — wider than before so towns breathe.
const VILLAGE_DIST_MIN: i32 = 9;
const VILLAGE_DIST_MAX: i32 = 16;
/// Minimum village↔village gap (Chebyshev shunks) so bigger footprints
/// (walls + outskirts ≈ 7 shunks across) never overlap each other.
const VILLAGE_SEPARATION_MIN: i32 = 8;
/// Minimum village↔capital gap (Chebyshev shunks) so the 18-block capital
/// walls and 30-block village walls + outskirts never overlap (needs ≥5).
const VILLAGE_CAPITAL_CLEAR_MIN: i32 = 5;

/// Stable identity of a realm (seed-grid coordinates).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RealmId {
    pub ix: i32,
    pub iz: i32,
}

/// Ownership of one shunk column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RealmCell {
    Wild,
    Claimed(RealmId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RealmInfo {
    pub id: RealmId,
    /// Target claim area in shunks² (disk budget; actual Voronoi share varies).
    pub area_budget: i32,
    /// Shunk coordinates of the capital (realm center).
    pub capital: (i32, i32),
    /// Shunk coordinates of villages (≥1).
    pub villages: Vec<(i32, i32)>,
}

#[inline]
pub fn shunk_coord(block_x: i32, block_z: i32) -> (i32, i32) {
    (
        block_x.div_euclid(SHUNK_SIZE),
        block_z.div_euclid(SHUNK_SIZE),
    )
}

#[inline]
fn seed_hash(ix: i32, iz: i32) -> u32 {
    mix_seed(REALM_SEED ^ WORLD_SEED, ix as u32, iz as u32)
}

/// Realm seed at grid `(ix, iz)`, or `None` if this node is skipped.
fn realm_seed(ix: i32, iz: i32) -> Option<(RealmId, i32, i32, i32)> {
    let h = seed_hash(ix, iz);
    if (h % 100) >= REALM_SPAWN_PCT {
        return None;
    }
    let span = (REALM_AREA_MAX - REALM_AREA_MIN + 1) as u32;
    let area = REALM_AREA_MIN + ((h >> 8) % span) as i32;
    let ox = ((h >> 16) % REALM_SEED_CELL as u32) as i32;
    let oz = ((h >> 24) % REALM_SEED_CELL as u32) as i32;
    let cx = ix * REALM_SEED_CELL + ox;
    let cz = iz * REALM_SEED_CELL + oz;
    Some((RealmId { ix, iz }, area, cx, cz))
}

fn claim_radius_sq(area_budget: i32) -> f32 {
    let area = (area_budget as f32) * RADIUS_AREA_MUL;
    area / std::f32::consts::PI
}

/// Warped distance² so patch borders look organic (not perfect circles).
fn warped_dist_sq(sx: i32, sz: i32, cx: i32, cz: i32, id: RealmId) -> f32 {
    let dx = (sx - cx) as f32;
    let dz = (sz - cz) as f32;
    let w = mix_seed(
        REALM_SEED ^ 0xB0A7_D15C,
        (sx as u32).wrapping_mul(73856093) ^ (id.ix as u32),
        (sz as u32).wrapping_mul(19349663) ^ (id.iz as u32),
    );
    let jx = ((w % 1000) as f32 / 1000.0 - 0.5) * 1.8;
    let jz = (((w >> 10) % 1000) as f32 / 1000.0 - 0.5) * 1.8;
    let dx = dx + jx;
    let dz = dz + jz;
    dx * dx + dz * dz
}

/// Which realm owns this shunk, if any.
pub fn realm_at_shunk(sx: i32, sz: i32) -> RealmCell {
    let ix0 = sx.div_euclid(REALM_SEED_CELL) - 1;
    let iz0 = sz.div_euclid(REALM_SEED_CELL) - 1;
    let mut best: Option<(RealmId, f32)> = None;
    for iz in iz0..=iz0 + 2 {
        for ix in ix0..=ix0 + 2 {
            let Some((id, area, cx, cz)) = realm_seed(ix, iz) else {
                continue;
            };
            let d2 = warped_dist_sq(sx, sz, cx, cz, id);
            if d2 > claim_radius_sq(area) {
                continue;
            }
            if best.is_none_or(|(_, bd)| d2 < bd) {
                best = Some((id, d2));
            }
        }
    }
    match best {
        Some((id, _)) => RealmCell::Claimed(id),
        None => RealmCell::Wild,
    }
}

#[inline]
pub fn realm_at_block(x: i32, z: i32) -> RealmCell {
    let (sx, sz) = shunk_coord(x, z);
    realm_at_shunk(sx, sz)
}

fn village_count(area_budget: i32) -> i32 {
    (1 + area_budget / 40).clamp(1, 4)
}

/// Full realm description for a seed that exists. `None` if that seed was skipped.
/// Cached — village placement probes `realm_at_shunk` repeatedly and is hot on stream/sign.
pub fn realm_info(id: RealmId) -> Option<RealmInfo> {
    let cache = realm_info_cache();
    if let Ok(guard) = cache.lock() {
        if let Some(hit) = guard.get(&id) {
            return hit.clone();
        }
    }
    let computed = realm_info_compute(id);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(id, computed.clone());
    }
    computed
}

fn realm_info_cache() -> &'static Mutex<HashMap<RealmId, Option<RealmInfo>>> {
    static CACHE: OnceLock<Mutex<HashMap<RealmId, Option<RealmInfo>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn realm_info_compute(id: RealmId) -> Option<RealmInfo> {
    let (id, area, cx, cz) = realm_seed(id.ix, id.iz)?;
    let capital = (cx, cz);
    let n = village_count(area);
    let mut villages = Vec::with_capacity(n as usize);
    let h0 = seed_hash(id.ix, id.iz);
    for i in 0..n {
        let h = mix_seed(h0 ^ 0x7111_A6E0, i as u32, area as u32);
        let ang = (h % 360) as f32 * (std::f32::consts::PI / 180.0);
        let dist =
            VILLAGE_DIST_MIN + ((h >> 9) % (VILLAGE_DIST_MAX - VILLAGE_DIST_MIN + 1) as u32) as i32;
        let mut vx = cx + (ang.cos() * dist as f32).round() as i32;
        let mut vz = cz + (ang.sin() * dist as f32).round() as i32;
        // Nudge into claimed territory if the ring landed in wild / foreign.
        let mut placed = false;
        for r in 0..5 {
            for (dx, dz) in [
                (0, 0),
                (r, 0),
                (-r, 0),
                (0, r),
                (0, -r),
                (r, r),
                (-r, -r),
                (r, -r),
                (-r, r),
            ] {
                let px = vx + dx;
                let pz = vz + dz;
                if (px, pz) == capital {
                    continue;
                }
                // Keep clear of the capital's walls (18 blocks) too.
                if (px - cx).abs().max((pz - cz).abs()) < VILLAGE_CAPITAL_CLEAR_MIN {
                    continue;
                }
                if villages.iter().any(|&p: &(i32, i32)| p == (px, pz)) {
                    continue;
                }
                // Spread villages around the ring: reject candidates too
                // close to an already placed village.
                if villages.iter().any(|&p: &(i32, i32)| {
                    (p.0 - px).abs().max((p.1 - pz).abs()) < VILLAGE_SEPARATION_MIN
                }) {
                    continue;
                }
                if realm_at_shunk(px, pz) == RealmCell::Claimed(id) {
                    vx = px;
                    vz = pz;
                    placed = true;
                    break;
                }
            }
            if placed {
                break;
            }
        }
        if !placed {
            // Fallback: axis offsets from capital (rotated per village so two
            // fallbacks don't collide). First claimed + clear spot wins; the
            // last resort keeps the old behavior.
            const OFFSETS: [(i32, i32); 4] = [
                (VILLAGE_DIST_MIN, 0),
                (0, VILLAGE_DIST_MIN),
                (-VILLAGE_DIST_MIN, 0),
                (0, -VILLAGE_DIST_MIN),
            ];
            let mut found = None;
            for k in 0..4 {
                let (ox, oz) = OFFSETS[((i + k) % 4) as usize];
                let px = cx + ox;
                let pz = cz + oz;
                if (px, pz) == capital
                    || villages.iter().any(|&p: &(i32, i32)| p == (px, pz))
                {
                    continue;
                }
                if (px - cx).abs().max((pz - cz).abs()) < VILLAGE_CAPITAL_CLEAR_MIN {
                    continue;
                }
                if villages.iter().any(|&p: &(i32, i32)| {
                    (p.0 - px).abs().max((p.1 - pz).abs()) < VILLAGE_SEPARATION_MIN
                }) {
                    continue;
                }
                if realm_at_shunk(px, pz) == RealmCell::Claimed(id) {
                    found = Some((px, pz));
                    break;
                }
            }
            let (fx, fz) = found.unwrap_or((cx, cz + VILLAGE_DIST_MIN));
            vx = fx;
            vz = fz;
        }
        // First village is always welcome (the ≥1 guarantee); the rest must
        // keep the minimum gap so footprints never overlap. Every village
        // must also clear the capital's walls.
        let clear = (vx - cx).abs().max((vz - cz).abs()) >= VILLAGE_CAPITAL_CLEAR_MIN
            && (villages.is_empty()
                || !villages.iter().any(|&p| {
                    (p.0 - vx).abs().max((p.1 - vz).abs()) < VILLAGE_SEPARATION_MIN
                }));
        if (vx, vz) != capital
            && !villages.iter().any(|&p| p == (vx, vz))
            && clear
        {
            villages.push((vx, vz));
        }
    }
    if villages.is_empty() {
        // Guarantee ≥1 village landmark even in pathological edges.
        villages.push((cx + VILLAGE_DIST_MIN, cz));
    }
    Some(RealmInfo {
        id,
        area_budget: area,
        capital,
        villages,
    })
}

/// Realm owning this shunk, with landmarks (if claimed).
#[allow(dead_code)]
pub fn realm_info_at_shunk(sx: i32, sz: i32) -> Option<RealmInfo> {
    match realm_at_shunk(sx, sz) {
        RealmCell::Wild => None,
        RealmCell::Claimed(id) => realm_info(id),
    }
}

/// Deterministic uppercase realm name (ASCII, for the HUD sign font).
pub fn realm_name(id: RealmId) -> String {
    const ONSET: [&str; 12] = [
        "BR", "VAL", "MOR", "THA", "EL", "KOR", "AR", "DUN", "GAL", "ZER", "MIR", "OL",
    ];
    const CORE: [&str; 8] = ["A", "E", "I", "O", "AN", "OR", "EN", "UM"];
    const TAIL: [&str; 8] = ["DOR", "GARD", "HEIM", "IA", "OS", "WYN", "THAL", "MAR"];
    let h = seed_hash(id.ix, id.iz);
    let a = ONSET[(h as usize) % ONSET.len()];
    let b = CORE[((h >> 8) as usize) % CORE.len()];
    let c = TAIL[((h >> 16) as usize) % TAIL.len()];
    format!("{a}{b}{c}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realm_queries_are_deterministic() {
        let a = realm_at_shunk(12, -7);
        let b = realm_at_shunk(12, -7);
        assert_eq!(a, b);
        if let RealmCell::Claimed(id) = a {
            assert_eq!(realm_info(id), realm_info(id));
        }
    }

    #[test]
    fn every_realm_has_capital_and_village() {
        let mut found = 0u32;
        for iz in -6..6 {
            for ix in -6..6 {
                let Some(info) = realm_info(RealmId { ix, iz }) else {
                    continue;
                };
                found += 1;
                assert!(!info.villages.is_empty(), "realm {info:?} missing village");
                assert!(
                    (REALM_AREA_MIN..=REALM_AREA_MAX).contains(&info.area_budget),
                    "area budget {}",
                    info.area_budget
                );
                assert_eq!(
                    realm_at_shunk(info.capital.0, info.capital.1),
                    RealmCell::Claimed(info.id),
                    "capital must sit in its realm"
                );
            }
        }
        assert!(
            found >= 8,
            "expected several realm seeds in sample, got {found}"
        );
    }

    #[test]
    fn patch_map_has_wild_and_claimed() {
        let mut wild = 0u32;
        let mut claimed = 0u32;
        let span = 48i32;
        for sz in -span..span {
            for sx in -span..span {
                match realm_at_shunk(sx, sz) {
                    RealmCell::Wild => wild += 1,
                    RealmCell::Claimed(_) => claimed += 1,
                }
            }
        }
        let total = (wild + claimed) as f32;
        let wild_f = wild as f32 / total;
        assert!(claimed > 0, "expected claimed shunks");
        assert!(wild > 0, "expected wild shunks");
        // Soft band around the design target (~30% wild).
        assert!(
            (0.15..=0.55).contains(&wild_f),
            "wild fraction {wild_f:.3} outside 15–55%"
        );
    }

    #[test]
    fn larger_realms_tend_to_get_more_villages() {
        let mut small_v = 0u32;
        let mut big_v = 0u32;
        let mut small_n = 0u32;
        let mut big_n = 0u32;
        for iz in -10..10 {
            for ix in -10..10 {
                let Some(info) = realm_info(RealmId { ix, iz }) else {
                    continue;
                };
                if info.area_budget <= 55 {
                    small_v += info.villages.len() as u32;
                    small_n += 1;
                } else if info.area_budget >= 100 {
                    big_v += info.villages.len() as u32;
                    big_n += 1;
                }
            }
        }
        if small_n > 0 && big_n > 0 {
            let small_avg = small_v as f32 / small_n as f32;
            let big_avg = big_v as f32 / big_n as f32;
            assert!(
                big_avg + 0.01 >= small_avg,
                "big realms should not have fewer villages on average ({big_avg} vs {small_avg})"
            );
        }
    }

    #[test]
    fn villages_keep_minimum_gap() {
        for iz in -6..6 {
            for ix in -6..6 {
                let Some(info) = realm_info(RealmId { ix, iz }) else {
                    continue;
                };
                for a in 0..info.villages.len() {
                    let village = info.villages[a];
                    // Villages stay clear of the capital's walls too.
                    let dc = (village.0 - info.capital.0)
                        .abs()
                        .max((village.1 - info.capital.1).abs());
                    assert!(dc >= 3, "village {village:?} on capital {info:?}");
                    for b in a + 1..info.villages.len() {
                        let other = info.villages[b];
                        let d = (village.0 - other.0).abs().max((village.1 - other.1).abs());
                        assert!(
                            d >= VILLAGE_SEPARATION_MIN,
                            "villages {village:?} and {other:?} too close in {info:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn block_and_shunk_queries_agree() {
        let x = 100;
        let z = -40;
        let (sx, sz) = shunk_coord(x, z);
        assert_eq!(realm_at_block(x, z), realm_at_shunk(sx, sz));
    }
}
