//! Underground chambers + connecting passages (~10% of solid under-surface volume).
//! Chests are reserved markers only (no open / loot UI yet).

use crate::world::{
    mix_seed, terrain_height, Material, Voxel, World, ENABLE_CAVES, MESH_CHUNK_SIZE, WORLD_SEED,
};
use glam::IVec3;

/// Mix seed for chamber layout (independent of trees / grass).
pub const CAVE_SEED: u32 = 0xCA7E_5EED;
/// Target fraction of under-surface solid volume to carve (chambers + passages).
#[allow(dead_code)]
pub const CAVE_VOLUME_TARGET: f32 = 0.10;
/// Spacing between chamber grid nodes (blocks).
pub const CHAMBER_CELL: i32 = 8;
/// Fraction of chambers that get a reserved chest on the floor.
pub const CHEST_CHANCE: f32 = 0.20;
/// Passage corridor width (blocks).
pub const PASSAGE_WIDTH: i32 = 2;
/// Chance a grid node hosts a chamber (drives total carve ≈ [`CAVE_VOLUME_TARGET`]).
const CHAMBER_SPAWN_PCT: u32 = 82;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chamber {
    /// Floor-center XZ (standing cell of the room).
    pub cx: i32,
    pub cz: i32,
    /// Lowest air Y inside the chamber (chest sits here).
    pub floor_y: i32,
    /// Floor footprint side length (4 or 5).
    pub floor_size: i32,
    /// Interior air height (2..=5).
    pub height: i32,
    /// Passage height when this node anchors an edge (2..=3).
    pub passage_h: i32,
    pub has_chest: bool,
}

/// Stable chamber at grid node `(ix, iz)`, or `None` if this cell is skipped / too shallow.
pub fn chamber_node(ix: i32, iz: i32) -> Option<Chamber> {
    let h = mix_seed(
        CAVE_SEED ^ WORLD_SEED,
        ix as u32,
        iz as u32,
    );
    // Spawn rate tuned with CHAMBER_CELL for ~10% under-surface carve volume.
    if (h % 100) >= CHAMBER_SPAWN_PCT {
        return None;
    }

    let cx = ix * CHAMBER_CELL + CHAMBER_CELL / 2;
    let cz = iz * CHAMBER_CELL + CHAMBER_CELL / 2;
    let surface = terrain_height(cx, cz);
    let floor_size = if (h >> 8) & 1 == 0 { 4 } else { 5 };
    let height = 2 + (((h >> 9) % 4) as i32); // 2..=5
    let passage_h = 2 + (((h >> 13) % 2) as i32); // 2..=3
    let has_chest = ((h >> 16) % 1000) as f32 / 1000.0 < CHEST_CHANCE;

    // Leave ≥2 solid blocks under the surface lip when possible.
    let max_floor = surface - 1 - height;
    let min_floor = 2;
    if max_floor < min_floor {
        return None;
    }
    let span = (max_floor - min_floor) as u32;
    let floor_y = min_floor + ((h >> 20) % (span + 1)) as i32;

    Some(Chamber {
        cx,
        cz,
        floor_y,
        floor_size,
        height,
        passage_h,
        has_chest,
    })
}

#[inline]
fn in_chunk_xz(x: i32, z: i32, x0: i32, z0: i32) -> bool {
    x >= x0 && x < x0 + MESH_CHUNK_SIZE && z >= z0 && z < z0 + MESH_CHUNK_SIZE
}

fn floor_bounds(c: &Chamber) -> (i32, i32, i32, i32) {
    let half = c.floor_size / 2;
    if c.floor_size % 2 == 0 {
        (c.cx - half, c.cx + half - 1, c.cz - half, c.cz + half - 1)
    } else {
        (c.cx - half, c.cx + half, c.cz - half, c.cz + half)
    }
}

fn max_carve_y(surface: i32) -> i32 {
    // Keep surface and surface-1 solid → carve only up to surface-2.
    surface - 2
}

fn carve_cell(world: &mut World, x: i32, y: i32, z: i32) {
    if y <= 0 {
        return;
    }
    // Use natural terrain height — dense `column_height` drops as mid-column punches land.
    let surface = terrain_height(x, z);
    if y > max_carve_y(surface) {
        return;
    }
    let _ = world.remove_voxel(IVec3::new(x, y, z));
}

fn carve_chamber(world: &mut World, c: &Chamber, x0: i32, z0: i32) {
    let (x_lo, x_hi, z_lo, z_hi) = floor_bounds(c);
    for z in z_lo..=z_hi {
        for x in x_lo..=x_hi {
            if !in_chunk_xz(x, z, x0, z0) {
                continue;
            }
            for dy in 0..c.height {
                carve_cell(world, x, c.floor_y + dy, z);
            }
        }
    }
}

#[inline]
fn stair_y_along(y0: i32, y1: i32, t: f32) -> i32 {
    let t = t.clamp(0.0, 1.0);
    (y0 as f32 + (y1 - y0) as f32 * t).round() as i32
}

/// Axis-aligned L-path from A→B, then inflate to [`PASSAGE_WIDTH`].
fn passage_cells(a: &Chamber, b: &Chamber) -> Vec<(i32, i32, f32)> {
    let mut spine: Vec<(i32, i32)> = Vec::new();
    let mut x = a.cx;
    let mut z = a.cz;
    spine.push((x, z));
    let sx = if b.cx >= a.cx { 1 } else { -1 };
    while x != b.cx {
        x += sx;
        spine.push((x, z));
    }
    let sz = if b.cz >= a.cz { 1 } else { -1 };
    while z != b.cz {
        z += sz;
        spine.push((x, z));
    }
    let n = spine.len().max(1) as f32;
    let mut out: Vec<(i32, i32, f32)> = Vec::with_capacity(spine.len() * PASSAGE_WIDTH as usize);
    for (i, &(px, pz)) in spine.iter().enumerate() {
        let t = if spine.len() <= 1 {
            0.0
        } else {
            i as f32 / (n - 1.0)
        };
        // Width-2: offset along the secondary axis of the current leg.
        let along_x = i + 1 < spine.len() && spine[i + 1].0 != px
            || (i > 0 && spine[i - 1].0 != px);
        if along_x || spine.len() == 1 {
            out.push((px, pz, t));
            out.push((px, pz + 1, t));
        } else {
            out.push((px, pz, t));
            out.push((px + 1, pz, t));
        }
    }
    out
}

fn carve_passage(world: &mut World, a: &Chamber, b: &Chamber, x0: i32, z0: i32) {
    let ph = ((a.passage_h + b.passage_h) / 2).clamp(2, 3);
    for (px, pz, t) in passage_cells(a, b) {
        if !in_chunk_xz(px, pz, x0, z0) {
            continue;
        }
        let floor_y = stair_y_along(a.floor_y, b.floor_y, t);
        for dy in 0..ph {
            carve_cell(world, px, floor_y + dy, pz);
        }
    }
}

fn place_reserved_chest(world: &mut World, c: &Chamber, x0: i32, z0: i32) {
    if !c.has_chest || !in_chunk_xz(c.cx, c.cz, x0, z0) {
        return;
    }
    // 1×1×2 pocket: chest + air above (air already carved when height ≥ 2).
    let chest_pos = IVec3::new(c.cx, c.floor_y, c.cz);
    let above = IVec3::new(c.cx, c.floor_y + 1, c.cz);
    carve_cell(world, above.x, above.y, above.z);
    world.set_voxel(chest_pos, Voxel::chest());
}

/// Carve chambers / passages / reserved chests for one filled mesh-chunk.
pub fn carve_chunk(world: &mut World, cx: i32, cz: i32) {
    if !ENABLE_CAVES {
        return;
    }
    let x0 = cx * MESH_CHUNK_SIZE;
    let z0 = cz * MESH_CHUNK_SIZE;

    // Nodes whose rooms or +X/+Z edges can touch this chunk.
    let margin = CHAMBER_CELL + 8;
    let ix0 = (x0 - margin).div_euclid(CHAMBER_CELL) - 1;
    let ix1 = (x0 + MESH_CHUNK_SIZE + margin).div_euclid(CHAMBER_CELL) + 1;
    let iz0 = (z0 - margin).div_euclid(CHAMBER_CELL) - 1;
    let iz1 = (z0 + MESH_CHUNK_SIZE + margin).div_euclid(CHAMBER_CELL) + 1;

    let mut nodes: Vec<(i32, i32, Chamber)> = Vec::new();
    for iz in iz0..=iz1 {
        for ix in ix0..=ix1 {
            if let Some(c) = chamber_node(ix, iz) {
                nodes.push((ix, iz, c));
            }
        }
    }

    for &(_, _, ref c) in &nodes {
        carve_chamber(world, c, x0, z0);
    }

    for &(ix, iz, ref c) in &nodes {
        if let Some(n) = chamber_node(ix + 1, iz) {
            carve_passage(world, c, &n, x0, z0);
        }
        if let Some(n) = chamber_node(ix, iz + 1) {
            carve_passage(world, c, &n, x0, z0);
        }
    }

    for &(_, _, ref c) in &nodes {
        place_reserved_chest(world, c, x0, z0);
    }
}

/// Candidate solid cells under the surface in a chunk (Y `2..=h-2`), for volume tests.
pub fn candidate_solid_count(world: &World, cx: i32, cz: i32) -> usize {
    let x0 = cx * MESH_CHUNK_SIZE;
    let z0 = cz * MESH_CHUNK_SIZE;
    let mut n = 0usize;
    for z in z0..z0 + MESH_CHUNK_SIZE {
        for x in x0..x0 + MESH_CHUNK_SIZE {
            if world.column_height(x, z).is_none() && world.get_voxel(IVec3::new(x, 0, z)).is_none()
            {
                continue;
            }
            let h = terrain_height(x, z);
            let y_hi = (h - 2).max(1);
            if y_hi < 2 {
                continue;
            }
            for y in 2..=y_hi {
                if world.get_voxel(IVec3::new(x, y, z)).is_some_and(|v| {
                    matches!(
                        v.material,
                        Material::Dirt
                            | Material::Stone
                            | Material::BlackStone
                            | Material::Bedrock
                    ) && v.is_fully_solid()
                }) {
                    n += 1;
                }
            }
        }
    }
    n
}

/// Air cells in the under-surface band that used to be solid candidates (carved volume proxy).
pub fn carved_air_count(world: &World, cx: i32, cz: i32) -> usize {
    let x0 = cx * MESH_CHUNK_SIZE;
    let z0 = cz * MESH_CHUNK_SIZE;
    let mut n = 0usize;
    for z in z0..z0 + MESH_CHUNK_SIZE {
        for x in x0..x0 + MESH_CHUNK_SIZE {
            if world.column_height(x, z).is_none() && world.get_voxel(IVec3::new(x, 0, z)).is_none()
            {
                continue;
            }
            let h = terrain_height(x, z);
            let y_hi = (h - 2).max(1);
            if y_hi < 2 {
                continue;
            }
            for y in 2..=y_hi {
                if world.get_voxel(IVec3::new(x, y, z)).is_none() {
                    n += 1;
                }
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::ENABLE_CAVES;

    fn fill_chunk_only(world: &mut World, cx: i32, cz: i32) {
        let x0 = cx * MESH_CHUNK_SIZE;
        let z0 = cz * MESH_CHUNK_SIZE;
        for z in z0..z0 + MESH_CHUNK_SIZE {
            for x in x0..x0 + MESH_CHUNK_SIZE {
                if world.column_height(x, z).is_none() {
                    world.fills_column_for_test(x, z, terrain_height(x, z));
                }
            }
        }
    }

    #[test]
    fn chamber_nodes_are_deterministic() {
        let a = chamber_node(3, -2);
        let b = chamber_node(3, -2);
        assert_eq!(a, b);
        // Same seed → same presence across “worlds”.
        let c = chamber_node(7, 4);
        assert_eq!(c, chamber_node(7, 4));
    }

    #[test]
    fn carve_never_touches_bedrock_band() {
        if !ENABLE_CAVES {
            return;
        }
        let mut world = World::new();
        for cz in -1..=1 {
            for cx in -1..=1 {
                fill_chunk_only(&mut world, cx, cz);
                carve_chunk(&mut world, cx, cz);
            }
        }
        for cz in -1..=1 {
            for cx in -1..=1 {
                let x0 = cx * MESH_CHUNK_SIZE;
                let z0 = cz * MESH_CHUNK_SIZE;
                for z in z0..z0 + MESH_CHUNK_SIZE {
                    for x in x0..x0 + MESH_CHUNK_SIZE {
                        assert!(
                            world.get_voxel(IVec3::new(x, 0, z)).is_some(),
                            "bedrock/floor missing at ({x},0,{z})"
                        );
                        // Y=1 should stay solid when column exists (caves start at y≥2).
                        if world.column_height(x, z).is_some() {
                            assert!(
                                world.get_voxel(IVec3::new(x, 1, z)).is_some(),
                                "y=1 carved at ({x},1,{z})"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn carved_volume_near_target() {
        if !ENABLE_CAVES {
            return;
        }
        let mut world = World::new();
        // Fill a patch, measure solid candidates, carve, measure air holes.
        let mut before = 0usize;
        for cz in -2..=2 {
            for cx in -2..=2 {
                fill_chunk_only(&mut world, cx, cz);
                before += candidate_solid_count(&world, cx, cz);
            }
        }
        assert!(before > 500, "need enough solid candidates, got {before}");

        for cz in -2..=2 {
            for cx in -2..=2 {
                carve_chunk(&mut world, cx, cz);
            }
        }

        let mut air = 0usize;
        let mut still_solid = 0usize;
        for cz in -2..=2 {
            for cx in -2..=2 {
                air += carved_air_count(&world, cx, cz);
                still_solid += candidate_solid_count(&world, cx, cz);
            }
        }
        // Carved ≈ before - still_solid (chests re-fill a few cells).
        let carved = before.saturating_sub(still_solid);
        let frac = carved as f32 / before as f32;
        assert!(
            (0.07..=0.13).contains(&frac),
            "carve fraction {frac:.3} (carved={carved}, before={before}, air≈{air}) outside 7–13%"
        );
    }

    #[test]
    fn reserved_chest_appears_in_extras() {
        if !ENABLE_CAVES {
            return;
        }
        let mut world = World::new();
        let mut found = false;
        // Scan a wide area until a chest-bearing chamber lands in a generated chunk.
        for iz in -8..8 {
            for ix in -8..8 {
                let Some(c) = chamber_node(ix, iz) else {
                    continue;
                };
                if !c.has_chest {
                    continue;
                }
                let (cx, cz) = (
                    c.cx.div_euclid(MESH_CHUNK_SIZE),
                    c.cz.div_euclid(MESH_CHUNK_SIZE),
                );
                // Neighbor ring so passages don't matter; just fill host chunk.
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        fill_chunk_only(&mut world, cx + dx, cz + dz);
                    }
                }
                carve_chunk(&mut world, cx, cz);
                let pos = IVec3::new(c.cx, c.floor_y, c.cz);
                if world
                    .get_voxel(pos)
                    .is_some_and(|v| v.material == Material::Chest)
                {
                    found = true;
                    break;
                }
            }
            if found {
                break;
            }
        }
        assert!(found, "expected at least one reserved chest in extras");
    }
}
