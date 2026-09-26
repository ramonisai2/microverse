//! Mining pipeline contracts for HD-2D.
//!
//! Flow: `Input → tool state → raycast Hit → break cell → dirty chunks → remesh → GPU`.
//! Stair dig and hold-to-break both call [`break_solid_cell`].

use crate::items::{bare_hand_can_mine, ToolInstance};
use crate::world::{Material, OreDrop, RayHit, World, EDIT_REACH};
use glam::IVec3;
use std::time::Duration;

/// Unified dig / place reach (blocks). Replaces ad-hoc `EDIT_REACH * 2.5`.
pub const MAX_REACH: f32 = EDIT_REACH;

/// Face of the hit block the ray entered through (outward normal of that face).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    PosX,
    NegX,
    PosY,
    NegY,
    PosZ,
    NegZ,
}

impl Face {
    pub fn from_delta(delta: IVec3) -> Self {
        // `prev - pos` points toward the empty cell the ray came from.
        if delta.x > 0 {
            Self::NegX
        } else if delta.x < 0 {
            Self::PosX
        } else if delta.y > 0 {
            Self::NegY
        } else if delta.y < 0 {
            Self::PosY
        } else if delta.z > 0 {
            Self::NegZ
        } else {
            Self::PosZ
        }
    }

    pub fn normal(self) -> IVec3 {
        match self {
            Self::PosX => IVec3::X,
            Self::NegX => IVec3::NEG_X,
            Self::PosY => IVec3::Y,
            Self::NegY => IVec3::NEG_Y,
            Self::PosZ => IVec3::Z,
            Self::NegZ => IVec3::NEG_Z,
        }
    }
}

/// Precise dig / place hit from DDA raycast.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub block: IVec3,
    pub face: Face,
    /// Empty neighbor to place against (`RayHit::prev`).
    pub prev: IVec3,
    pub dist: f32,
}

impl Hit {
    pub fn from_ray(hit: RayHit, origin: glam::Vec3) -> Self {
        let face = Face::from_delta(hit.prev - hit.pos);
        let center = hit.pos.as_vec3() + glam::Vec3::splat(0.5);
        let dist = (center - origin).length();
        Self {
            block: hit.pos,
            face,
            prev: hit.prev,
            dist,
        }
    }
}

/// Progressive single-block dig (hold-to-break).
#[derive(Clone, Debug, Default)]
pub struct MiningProgress {
    pub target: Option<IVec3>,
    /// 0..1 toward break.
    pub t: f32,
}

impl MiningProgress {
    pub fn clear(&mut self) {
        self.target = None;
        self.t = 0.0;
    }

    /// Advance toward breaking `pos`. Returns true when the cell should break.
    pub fn tick(&mut self, pos: IVec3, dt: f32, interval: Duration) -> bool {
        if self.target != Some(pos) {
            self.target = Some(pos);
            self.t = 0.0;
        }
        let secs = interval.as_secs_f32().max(0.001);
        self.t += dt / secs;
        if self.t >= 1.0 {
            self.clear();
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakOutcome {
    /// Air / already gone — no wear.
    Skipped,
    /// Broke solid and applied wear. `ore` = coal/crystal flecks if any.
    Broke {
        material: Material,
        ore: Option<OreDrop>,
    },
    /// Tool cannot afford the strike.
    ToolBlocked,
}

/// Remove one solid diggable cell, wear the pick, clear floating grass above.
pub fn break_solid_cell(world: &mut World, pos: IVec3, pick: &mut ToolInstance) -> BreakOutcome {
    let Some(material) = world.dig_material_at(pos) else {
        // Still clear stray grass if present.
        let above = pos + IVec3::Y;
        if world
            .get_voxel(above)
            .is_some_and(|v| v.material == Material::Grass)
        {
            let _ = world.remove_voxel_player(above);
        }
        return BreakOutcome::Skipped;
    };

    if pick.is_broken() || !pick.def.can_mine(material) {
        return BreakOutcome::ToolBlocked;
    }
    if pick.durability < pick.def.durability_cost(material) {
        return BreakOutcome::ToolBlocked;
    }

    // Sample ore before the cell disappears.
    let ore = world.ore_drop_at(pos);

    if !world.remove_voxel_player(pos) {
        return BreakOutcome::Skipped;
    }
    let _ = pick.try_wear(material);
    let above = pos + IVec3::Y;
    if world
        .get_voxel(above)
        .is_some_and(|v| v.material == Material::Grass)
    {
        let _ = world.remove_voxel_player(above);
    }
    BreakOutcome::Broke { material, ore }
}

/// Dulled (broken) pick: mines what the pick could mine at 2× time, no wear.
/// Durability is already 0 — no further cost is charged.
pub fn break_solid_cell_dulled(
    world: &mut World,
    pos: IVec3,
    pick: &ToolInstance,
) -> BreakOutcome {
    let Some(material) = world.dig_material_at(pos) else {
        // Still clear stray grass if present.
        let above = pos + IVec3::Y;
        if world
            .get_voxel(above)
            .is_some_and(|v| v.material == Material::Grass)
        {
            let _ = world.remove_voxel_player(above);
        }
        return BreakOutcome::Skipped;
    };

    if !pick.def.can_mine(material) {
        return BreakOutcome::ToolBlocked;
    }

    // Sample ore before the cell disappears.
    let ore = world.ore_drop_at(pos);

    if !world.remove_voxel_player(pos) {
        return BreakOutcome::Skipped;
    }
    let above = pos + IVec3::Y;
    if world
        .get_voxel(above)
        .is_some_and(|v| v.material == Material::Grass)
    {
        let _ = world.remove_voxel_player(above);
    }
    BreakOutcome::Broke { material, ore }
}
pub fn break_solid_cell_bare(world: &mut World, pos: IVec3) -> BreakOutcome {
    let Some(material) = world.dig_material_at(pos) else {
        let above = pos + IVec3::Y;
        if world
            .get_voxel(above)
            .is_some_and(|v| v.material == Material::Grass)
        {
            let _ = world.remove_voxel_player(above);
        }
        return BreakOutcome::Skipped;
    };

    if !bare_hand_can_mine(material) {
        return BreakOutcome::ToolBlocked;
    }

    let ore = world.ore_drop_at(pos);
    if !world.remove_voxel_player(pos) {
        return BreakOutcome::Skipped;
    }
    let above = pos + IVec3::Y;
    if world
        .get_voxel(above)
        .is_some_and(|v| v.material == Material::Grass)
    {
        let _ = world.remove_voxel_player(above);
    }
    BreakOutcome::Broke { material, ore }
}

/// Raycast helper using [`MAX_REACH`].
pub fn raycast_reach(world: &World, origin: glam::Vec3, dir: glam::Vec3) -> Option<Hit> {
    world
        .raycast(origin, dir, MAX_REACH)
        .map(|h| Hit::from_ray(h, origin))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::spawn_wooden_pickaxe;
    use crate::world::Voxel;

    #[test]
    fn bare_hand_breaks_dirt_without_wear() {
        let mut world = World::new();
        world.set_voxel(IVec3::new(2, 3, 2), Voxel::dirt());
        assert_eq!(
            break_solid_cell_bare(&mut world, IVec3::new(2, 3, 2)),
            BreakOutcome::Broke {
                material: Material::Dirt,
                ore: None,
            }
        );
        assert!(world.get_voxel(IVec3::new(2, 3, 2)).is_none());
    }

    #[test]
    fn bare_hand_blocked_on_bedrock() {
        let mut world = World::new();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::bedrock());
        assert_eq!(
            break_solid_cell_bare(&mut world, IVec3::new(0, 0, 0)),
            BreakOutcome::ToolBlocked
        );
    }

    #[test]
    fn face_from_delta_matches_entry() {
        assert_eq!(Face::from_delta(IVec3::NEG_X), Face::PosX);
        assert_eq!(Face::from_delta(IVec3::Y), Face::NegY);
    }

    #[test]
    fn break_cell_wears_and_clears_grass() {
        let mut world = World::new();
        world.set_voxel(IVec3::new(0, 5, 0), Voxel::dirt());
        world.set_voxel(IVec3::new(0, 6, 0), Voxel::grass_from_seed(1));
        let mut pick = spawn_wooden_pickaxe();
        let before = pick.durability;
        assert_eq!(
            break_solid_cell(&mut world, IVec3::new(0, 5, 0), &mut pick),
            BreakOutcome::Broke {
                material: Material::Dirt,
                ore: None,
            }
        );
        assert_eq!(pick.durability, before - 1);
        assert!(world.get_voxel(IVec3::new(0, 5, 0)).is_none());
        assert!(world.get_voxel(IVec3::new(0, 6, 0)).is_none());
    }

    #[test]
    fn mining_progress_resets_on_target_change() {
        let mut p = MiningProgress::default();
        assert!(!p.tick(IVec3::ZERO, 0.05, Duration::from_millis(200)));
        assert!(p.t > 0.0);
        assert!(!p.tick(IVec3::X, 0.05, Duration::from_millis(200)));
        assert!(p.t < 0.5);
    }

    #[test]
    fn break_updates_column_height_for_collision() {
        let mut world = World::new();
        world.set_voxel(IVec3::new(3, 0, 3), Voxel::dirt());
        world.set_voxel(IVec3::new(3, 1, 3), Voxel::dirt());
        assert_eq!(world.column_height(3, 3), Some(1));
        let mut pick = spawn_wooden_pickaxe();
        assert_eq!(
            break_solid_cell(&mut world, IVec3::new(3, 1, 3), &mut pick),
            BreakOutcome::Broke {
                material: Material::Dirt,
                ore: None,
            }
        );
        assert_eq!(world.column_height(3, 3), Some(0));
        assert!(world.get_voxel(IVec3::new(3, 1, 3)).is_none());
    }

    #[test]
    fn max_reach_matches_edit_reach() {
        assert!((MAX_REACH - EDIT_REACH).abs() < 1e-6);
    }

    #[test]
    fn dulled_pick_breaks_without_wear() {
        let mut world = World::new();
        world.set_voxel(IVec3::new(4, 3, 4), Voxel::stone());
        let mut pick = spawn_wooden_pickaxe();
        pick.durability = 0;
        assert!(pick.is_broken());
        // El pico sano rechazaría la piedra rota...
        assert_eq!(
            break_solid_cell(&mut world, IVec3::new(4, 3, 4), &mut pick),
            BreakOutcome::ToolBlocked
        );
        // ...pero desafilado pica igual (al doble de tiempo, sin desgaste).
        assert!(matches!(
            break_solid_cell_dulled(&mut world, IVec3::new(4, 3, 4), &pick),
            BreakOutcome::Broke {
                material: Material::Stone,
                ..
            }
        ));
        assert_eq!(pick.durability, 0);
        assert!(world.get_voxel(IVec3::new(4, 3, 4)).is_none());
    }

    #[test]
    fn bedrock_cannot_be_broken() {
        let mut world = World::new();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::bedrock());
        world.set_voxel(IVec3::new(0, 1, 0), Voxel::dirt());
        let mut pick = spawn_wooden_pickaxe();
        assert_eq!(
            break_solid_cell(&mut world, IVec3::new(0, 0, 0), &mut pick),
            BreakOutcome::ToolBlocked
        );
        assert!(world.get_voxel(IVec3::new(0, 0, 0)).is_some());
    }

    #[test]
    fn wooden_pick_cannot_break_black_stone() {
        let mut world = World::new();
        world.set_voxel(IVec3::new(1, 2, 1), Voxel::black_stone());
        let mut pick = spawn_wooden_pickaxe();
        assert!(!pick.def.can_mine(Material::BlackStone));
        assert_eq!(
            break_solid_cell(&mut world, IVec3::new(1, 2, 1), &mut pick),
            BreakOutcome::ToolBlocked
        );
        assert_eq!(
            world.get_voxel(IVec3::new(1, 2, 1)).map(|v| v.material),
            Some(Material::BlackStone)
        );
    }
}
