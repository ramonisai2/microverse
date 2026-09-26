use crate::camera::HeldKeys;
use crate::combat::melee_damage_for_item;
use crate::hero_pose::HeroPose;
use crate::hud::HotbarItem;
use crate::world::{terrain_height, World};
use glam::{IVec3, Vec3};

/// Full body height in blocks (< 2).
pub const PLAYER_HEIGHT: f32 = 1.8;
/// Half-width of the collision box (full width 0.6).
pub const PLAYER_HALF_WIDTH: f32 = 0.3;
/// Eye height from feet (first-person camera).
pub const PLAYER_EYE_HEIGHT: f32 = 1.62;
/// Full health (on-screen heart icons + title strip).
pub const MAX_HEARTS: u32 = 4;
/// Magic bottle slots (empty until filled).
pub const MAX_BOTTLES: u32 = 3;
/// Feet below this Y → void rescue (world bottom is y=0).
pub const VOID_RESCUE_Y: f32 = -2.0;

const WALK_SPEED: f32 = 2.4;
const SPRINT_SPEED: f32 = 4.2;
const GRAVITY: f32 = 28.0;
const JUMP_SPEED: f32 = 8.4;
const TICK_DT: f32 = 1.0 / crate::camera::TICKS_PER_SECOND as f32;
const FACING_MOUSE_SENS: f32 = 0.0025;
/// Softer horizontal accel toward walk wish (blocks/s²).
const HORIZ_ACCEL: f32 = 12.0;
/// Softer slowdown when no input (blocks/s²).
const HORIZ_FRICTION: f32 = 16.0;
/// Max turn rate while walking (rad/s).
const FACING_TURN_SPEED: f32 = 10.0;
/// Auto step-up height (one block stairs / ledges).
const STEP_UP: f32 = 1.05;
/// Equip / stow blend speed (toward settled = 1).
const EQUIP_BLEND_RATE: f32 = 7.0;
/// Seconds for one melee swing (full 0→1).
const MELEE_SWING_SECS: f32 = 0.38;

/// Coarse locomotion / tool activity for HD-2D motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerActivity {
    Idle,
    Walk,
    Sprint,
    Jump,
    Dig,
    Attack,
    Equip,
}

/// Which melee animation to play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttackKind {
    PickOverhead,
    Sword { variant: u8 },
    /// Estocada baja con espada (Shift + clic): embestida frontal baja.
    SwordThrust,
    Fist { left: bool },
}

#[derive(Clone, Copy, Debug)]
pub struct MeleeAttack {
    pub kind: AttackKind,
    /// 0..1 through the swing.
    pub t: f32,
    /// Air swing = timid / smaller motion.
    pub timid: bool,
    pub damage: u32,
    pub active: bool,
}

impl Default for MeleeAttack {
    fn default() -> Self {
        Self {
            kind: AttackKind::Fist { left: false },
            t: 0.0,
            timid: true,
            damage: 1,
            active: false,
        }
    }
}

pub struct Player {
    /// Feet position (bottom-center of the AABB).
    pub feet: Vec3,
    pub velocity: Vec3,
    pub on_ground: bool,
    /// Horizontal facing (radians). Used for WASD in HD-2D; FPS uses camera yaw instead.
    pub facing: f32,
    /// Remaining hearts (`0..=MAX_HEARTS`).
    pub hearts: u32,
    /// Filled magic bottles (`0..=MAX_BOTTLES`); HUD shows the rest empty.
    pub bottles: u32,
    /// Walk-cycle phase (radians), advanced while moving on ground.
    pub walk_phase: f32,
    /// 0 = idle pose, 1 = full stride (smoothed).
    pub walk_amount: f32,
    /// 0 = walk gait, 1 = exaggerated sprint caricature (smoothed).
    pub run_amount: f32,
    /// Dig stroke phase (radians).
    pub dig_phase: f32,
    /// 0 = no dig pose, 1 = full dig.
    pub dig_amount: f32,
    /// 0 = grounded pose, 1 = full jump pose.
    pub jump_amount: f32,
    /// 0 = fuera del agua, 1 = nado total (suavizado).
    pub swim_amount: f32,
    /// Held-tool swing arc weight (`sin(π·t)` during dig).
    pub tool_swing: f32,
    /// Vertical bob offset (blocks) for walk/sprint.
    pub bob_y: f32,
    /// Botas de fondo en el inventario: pisan el lecho sin flotar.
    pub sink_boots: bool,
    /// 0 = just equipped (offset pop), 1 = settled grip.
    pub equip_blend: f32,
    /// Shift+derecho con espada: guardia de bloqueo (lo pone el input).
    pub blocking: bool,
    /// 0 = sin guardia, 1 = bloqueo total (suavizado).
    pub block_amount: f32,
    /// Discrete click melee (sword / pick / fists).
    pub melee: MeleeAttack,
    /// Cycles sword slash 0→1→2.
    sword_combo: u8,
    /// Alternates fist punches.
    next_fist_left: bool,
    pub activity: PlayerActivity,
    /// Last solid footing before a void fall (XZ + surface feet Y).
    last_safe_feet: Vec3,
    /// Physics snapshot from the previous tick (for render interpolation).
    prev_feet: Vec3,
    prev_facing: f32,
    prev_walk_phase: f32,
    prev_walk_amount: f32,
    prev_run_amount: f32,
    prev_dig_phase: f32,
    prev_dig_amount: f32,
    prev_jump_amount: f32,
    prev_swim_amount: f32,
    prev_tool_swing: f32,
    prev_equip_blend: f32,
    prev_block_amount: f32,
    prev_bob_y: f32,
}

impl Player {
    /// Spawn en tierra firme un poco lejos del mar, mirando a −Z.
    /// Espiral determinista desde (8, 24): primer parche 3×3 seco con el
    /// centro 2+ sobre el nivel del mar (nada de playas ni agua).
    pub fn spawn_on_terrain() -> Self {
        let (mut x, mut z) = (8i32, 24i32);
        'search: for r in (0..200).step_by(2) {
            let steps = (r.max(1) * 2).min(64);
            for k in 0..steps {
                let a = k as f32 / steps as f32 * std::f32::consts::TAU;
                let cx = 8 + (a.cos() * r as f32).round() as i32;
                let cz = 24 + (a.sin() * r as f32).round() as i32;
                if terrain_height(cx, cz) < crate::world::SEA_LEVEL + 2 {
                    continue;
                }
                let mut dry = true;
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        if terrain_height(cx + dx, cz + dz) < crate::world::SEA_LEVEL + 1 {
                            dry = false;
                            break;
                        }
                    }
                    if !dry {
                        break;
                    }
                }
                if dry {
                    x = cx;
                    z = cz;
                    break 'search;
                }
            }
        }
        let ground = terrain_height(x, z).max(0) as f32;
        let feet = Vec3::new(x as f32 + 0.5, ground + 1.0, z as f32 + 0.5);
        let facing = -std::f32::consts::FRAC_PI_2;
        Self {
            feet,
            velocity: Vec3::ZERO,
            on_ground: true,
            facing,
            hearts: MAX_HEARTS,
            bottles: 0,
            walk_phase: 0.0,
            walk_amount: 0.0,
            run_amount: 0.0,
            dig_phase: 0.0,
            dig_amount: 0.0,
            jump_amount: 0.0,
            swim_amount: 0.0,
            tool_swing: 0.0,
            bob_y: 0.0,
            sink_boots: false,
            equip_blend: 1.0,
            blocking: false,
            block_amount: 0.0,
            melee: MeleeAttack::default(),
            sword_combo: 0,
            next_fist_left: false,
            activity: PlayerActivity::Idle,
            last_safe_feet: feet,
            prev_feet: feet,
            prev_facing: facing,
            prev_walk_phase: 0.0,
            prev_walk_amount: 0.0,
            prev_run_amount: 0.0,
            prev_dig_phase: 0.0,
            prev_dig_amount: 0.0,
            prev_jump_amount: 0.0,
            prev_swim_amount: 0.0,
            prev_tool_swing: 0.0,
            prev_equip_blend: 1.0,
            prev_block_amount: 0.0,
            prev_bob_y: 0.0,
        }
    }

    /// Restore pose from a save (animation / velocity reset).
    pub fn restore_from_save(&mut self, feet: Vec3, facing: f32, hearts: u32, bottles: u32) {
        self.feet = feet;
        self.velocity = Vec3::ZERO;
        self.on_ground = true;
        self.facing = facing;
        self.hearts = hearts.min(MAX_HEARTS);
        self.bottles = bottles.min(MAX_BOTTLES);
        self.activity = PlayerActivity::Idle;
        self.melee = MeleeAttack::default();
        self.last_safe_feet = feet;
        self.prev_feet = feet;
        self.prev_facing = facing;
        self.walk_phase = 0.0;
        self.walk_amount = 0.0;
        self.run_amount = 0.0;
        self.dig_phase = 0.0;
        self.dig_amount = 0.0;
        self.jump_amount = 0.0;
        self.swim_amount = 0.0;
        self.tool_swing = 0.0;
        self.bob_y = 0.0;
        self.equip_blend = 1.0;
        self.blocking = false;
        self.block_amount = 0.0;
        self.sink_boots = false;
        self.prev_walk_phase = 0.0;
        self.prev_walk_amount = 0.0;
        self.prev_run_amount = 0.0;
        self.prev_dig_phase = 0.0;
        self.prev_dig_amount = 0.0;
        self.prev_jump_amount = 0.0;
        self.prev_swim_amount = 0.0;
        self.prev_tool_swing = 0.0;
        self.prev_equip_blend = 1.0;
        self.prev_block_amount = 0.0;
        self.prev_bob_y = 0.0;
    }

    /// Hotbar swap / equip — restart short grip settle blend.
    pub fn notify_equip(&mut self) {
        self.equip_blend = 0.0;
        self.activity = PlayerActivity::Equip;
    }

    /// Start a click melee. `timid` = swung at air (smaller motion).
    /// Returns damage that would apply on a connected hit.
    pub fn begin_melee(&mut self, item: HotbarItem, timid: bool) -> u32 {
        // Allow restart near the end of a swing so combos feel responsive.
        if self.melee.active && self.melee.t < 0.55 {
            return self.melee.damage;
        }
        let damage = melee_damage_for_item(item);
        let kind = match item {
            HotbarItem::Pickaxe => AttackKind::PickOverhead,
            HotbarItem::Sword => {
                let v = self.sword_combo % 3;
                self.sword_combo = self.sword_combo.wrapping_add(1);
                AttackKind::Sword { variant: v }
            }
            HotbarItem::Axe => AttackKind::PickOverhead, // stub: overhead until axe anim exists
            HotbarItem::Empty => {
                let left = self.next_fist_left;
                self.next_fist_left = !self.next_fist_left;
                AttackKind::Fist { left }
            }
            HotbarItem::Consumable => AttackKind::Fist { left: false },
            HotbarItem::Magic => AttackKind::Fist { left: false },
            HotbarItem::Shield => AttackKind::Fist { left: false },
        };
        self.melee = MeleeAttack {
            kind,
            t: 0.0,
            timid,
            damage,
            active: true,
        };
        self.activity = PlayerActivity::Attack;
        damage
    }

    /// Start a sword low thrust (Shift+click). Does not advance the slash combo.
    /// Returns damage that would apply on a connected hit.
    pub fn begin_sword_thrust(&mut self, timid: bool) -> u32 {
        if self.melee.active && self.melee.t < 0.55 {
            return self.melee.damage;
        }
        let damage = melee_damage_for_item(HotbarItem::Sword);
        self.melee = MeleeAttack {
            kind: AttackKind::SwordThrust,
            t: 0.0,
            timid,
            damage,
            active: true,
        };
        self.activity = PlayerActivity::Attack;
        damage
    }

    pub fn melee_active(&self) -> bool {
        self.melee.active
    }

    /// Tool-mesh swing weight from discrete melee (pick/sword) or dig hold.
    pub fn display_tool_swing(&self, alpha: f32) -> f32 {
        let a = alpha.clamp(0.0, 1.0);
        if self.melee.active {
            let t = self.melee.t;
            // Arc peaks mid-swing.
            return (t * std::f32::consts::PI).sin() * if self.melee.timid { 0.55 } else { 1.0 };
        }
        self.prev_tool_swing + (self.tool_swing - self.prev_tool_swing) * a
    }

    pub fn display_equip_blend(&self, alpha: f32) -> f32 {
        let a = alpha.clamp(0.0, 1.0);
        self.prev_equip_blend + (self.equip_blend - self.prev_equip_blend) * a
    }

    fn compose_pose(
        walk_phase: f32,
        walk_amount: f32,
        run_amount: f32,
        dig_phase: f32,
        dig_amount: f32,
        jump_amount: f32,
        block_amount: f32,
        swim_amount: f32,
        vel_y: f32,
        melee: &MeleeAttack,
    ) -> HeroPose {
        // Clips desde assets/animations/*.json (fallback procedural).
        let walk = crate::animation::walk_pose(walk_phase, walk_amount);
        let run = crate::animation::run_pose(walk_phase, walk_amount);
        let gait = walk.lerp(run, run_amount.clamp(0.0, 1.0));
        let dig = crate::animation::dig_pose(dig_phase, dig_amount);
        let jump = HeroPose::from_jump(vel_y, jump_amount);
        // Salto en sprint (Shift+salto): valla con rodilla alta y pierna
        // extendida, mezclada por sprint conservado en el aire.
        let sj_w = (jump_amount * run_amount).clamp(0.0, 1.0);
        let jump = jump.lerp(HeroPose::from_sprint_jump(vel_y, 1.0), sj_w);
        // Dig overrides gait; block guard overlays it; jump overlays when airborne.
        let base = gait.lerp(dig, dig_amount);
        let guarded = base.lerp(
            HeroPose::from_block(block_amount),
            block_amount.clamp(0.0, 1.0),
        );
        let with_jump = guarded.lerp(
            jump,
            jump_amount * (1.0 - dig_amount * 0.5) * (1.0 - block_amount * 0.5),
        );
        // Nado con animación propia: brazadas alternas + patada (gana al salto).
        let swim_w = swim_amount.clamp(0.0, 1.0);
        let with_swim =
            with_jump.lerp(HeroPose::from_swim(walk_phase, 1.0), swim_w);
        if !melee.active {
            return with_swim;
        }
        let atk = match melee.kind {
            AttackKind::PickOverhead => {
                crate::animation::pick_overhead_pose(melee.t, 1.0, melee.timid)
            }
            AttackKind::Sword { variant } => {
                crate::animation::sword_slash_pose(variant, melee.t, 1.0, melee.timid)
            }
            AttackKind::SwordThrust => {
                crate::animation::sword_thrust_pose(melee.t, 1.0, melee.timid)
            }
            AttackKind::Fist { left } => {
                crate::animation::fist_pose(left, melee.t, 1.0, melee.timid)
            }
        };
        // Full override while swinging so the strike reads clearly.
        with_jump.lerp(atk, 0.92)
    }

    /// Hierarchical limb pose for the hero mesh.
    pub fn hero_pose(&self) -> HeroPose {
        Self::compose_pose(
            self.walk_phase,
            self.walk_amount,
            self.run_amount,
            self.dig_phase,
            self.dig_amount,
            self.jump_amount,
            self.block_amount,
            self.swim_amount,
            self.velocity.y,
            &self.melee,
        )
    }

    /// Pose blended between the last two physics ticks (`alpha` in 0..=1).
    pub fn display_hero_pose(&self, alpha: f32) -> HeroPose {
        let a = alpha.clamp(0.0, 1.0);
        let phase = self.prev_walk_phase + (self.walk_phase - self.prev_walk_phase) * a;
        let amount = self.prev_walk_amount + (self.walk_amount - self.prev_walk_amount) * a;
        let run_amount = self.prev_run_amount + (self.run_amount - self.prev_run_amount) * a;
        let dig_phase = self.prev_dig_phase + (self.dig_phase - self.prev_dig_phase) * a;
        let dig_amount = self.prev_dig_amount + (self.dig_amount - self.prev_dig_amount) * a;
        let jump_amount = self.prev_jump_amount + (self.jump_amount - self.prev_jump_amount) * a;
        let block_amount = self.prev_block_amount + (self.block_amount - self.prev_block_amount) * a;
        let swim_amount = self.prev_swim_amount + (self.swim_amount - self.prev_swim_amount) * a;
        Self::compose_pose(
            phase,
            amount,
            run_amount,
            dig_phase,
            dig_amount,
            jump_amount,
            block_amount,
            swim_amount,
            self.velocity.y,
            &self.melee,
        )
    }

    pub fn eye_position(&self) -> Vec3 {
        self.feet + Vec3::Y * PLAYER_EYE_HEIGHT
    }

    /// Focus point for the HD-2D camera (chest height).
    pub fn focus_position(&self) -> Vec3 {
        self.feet + Vec3::Y * 1.0
    }

    /// Render/camera feet blended between ticks (`alpha` = leftover tick fraction).
    pub fn display_feet(&self, alpha: f32) -> Vec3 {
        let a = alpha.clamp(0.0, 1.0);
        let mut p = self.prev_feet.lerp(self.feet, a);
        p.y += self.prev_bob_y + (self.bob_y - self.prev_bob_y) * a;
        p
    }

    pub fn display_facing(&self, alpha: f32) -> f32 {
        let a = alpha.clamp(0.0, 1.0);
        let mut d = self.facing - self.prev_facing;
        let pi = std::f32::consts::PI;
        let tau = std::f32::consts::TAU;
        while d > pi {
            d -= tau;
        }
        while d < -pi {
            d += tau;
        }
        self.prev_facing + d * a
    }

    pub fn display_focus(&self, alpha: f32) -> Vec3 {
        self.display_feet(alpha) + Vec3::Y * 1.0
    }

    pub fn display_eye(&self, alpha: f32) -> Vec3 {
        self.display_feet(alpha) + Vec3::Y * PLAYER_EYE_HEIGHT
    }

    /// Compact heart strip for the window title (`♥` filled, `♡` empty).
    pub fn hearts_title(&self) -> String {
        let mut s = String::with_capacity(MAX_HEARTS as usize * 3);
        for i in 0..MAX_HEARTS {
            if i < self.hearts {
                s.push('♥');
            } else {
                s.push('♡');
            }
        }
        s
    }

    /// Blocks the player's body occupies (used to avoid trapping when placing).
    pub fn occupied_blocks(&self) -> [IVec3; 2] {
        let feet = IVec3::new(
            self.feet.x.floor() as i32,
            self.feet.y.floor() as i32,
            self.feet.z.floor() as i32,
        );
        let head_y = (self.feet.y + PLAYER_HEIGHT - 0.01).floor() as i32;
        [feet, IVec3::new(feet.x, head_y, feet.z)]
    }

    #[allow(dead_code)]
    pub fn apply_facing_delta(&mut self, dx: f64) {
        self.facing += dx as f32 * FACING_MOUSE_SENS;
    }

    /// Snap facing toward a world yaw (e.g. dig pad).
    pub fn face_yaw(&mut self, yaw: f32) {
        let dir = Vec3::new(yaw.cos(), 0.0, yaw.sin());
        self.face_toward_xz(dir);
        // Pad taps should feel snappy — finish the turn this tick.
        self.facing = yaw;
    }

    /// Ease facing toward the current walk wish (HD-2D).
    pub fn face_toward_xz(&mut self, dir: Vec3) {
        if dir.length_squared() <= 1e-6 {
            return;
        }
        let target = dir.z.atan2(dir.x);
        let mut d = target - self.facing;
        let pi = std::f32::consts::PI;
        let tau = std::f32::consts::TAU;
        while d > pi {
            d -= tau;
        }
        while d < -pi {
            d += tau;
        }
        let max_step = FACING_TURN_SPEED * TICK_DT;
        self.facing += d.clamp(-max_step, max_step);
    }

    pub fn tick(
        &mut self,
        world: &World,
        keys: &HeldKeys,
        yaw: f32,
        ticks: u32,
        digging: bool,
        first_person: bool,
    ) {
        for _ in 0..ticks {
            self.prev_feet = self.feet;
            self.prev_facing = self.facing;
            self.prev_walk_phase = self.walk_phase;
            self.prev_walk_amount = self.walk_amount;
            self.prev_run_amount = self.run_amount;
            self.prev_dig_phase = self.dig_phase;
            self.prev_dig_amount = self.dig_amount;
            self.prev_jump_amount = self.jump_amount;
            self.prev_swim_amount = self.swim_amount;
            self.prev_tool_swing = self.tool_swing;
            self.prev_equip_blend = self.equip_blend;
            self.prev_block_amount = self.block_amount;
            self.prev_bob_y = self.bob_y;
            self.tick_once(world, keys, yaw, digging, first_person);
        }
    }

    fn tick_once(
        &mut self,
        world: &World,
        keys: &HeldKeys,
        yaw: f32,
        digging: bool,
        first_person: bool,
    ) {
        // Camera-relative isometric axes: Forward=(cos θ, sin θ), Right=(-sin θ, cos θ).
        let flat_forward = Vec3::new(yaw.cos(), 0.0, yaw.sin()).normalize_or_zero();
        let right = Vec3::new(-yaw.sin(), 0.0, yaw.cos());

        let mut wish = Vec3::ZERO;
        if keys.forward {
            wish += flat_forward;
        }
        if keys.back {
            wish -= flat_forward;
        }
        if keys.left {
            wish -= right;
        }
        if keys.right {
            wish += right;
        }
        let speed = if keys.sprint {
            SPRINT_SPEED
        } else {
            WALK_SPEED
        };
        if wish.length_squared() > 0.0 {
            wish = wish.normalize() * speed;
            // Don't spin the body away from dig / melee while swinging.
            // First person never turns the body toward the wish: facing stays
            // mouse-driven so S backpedals and A/D strafe without rotating.
            if !digging && !self.melee.active && !first_person {
                self.face_toward_xz(wish);
            }
        }

        // Agua: pies u ojos mojados. En superficie -20%, sumergido -40%
        // (con botas también, que caminan el fondo).
        let feet_cell = IVec3::new(
            self.feet.x.floor() as i32,
            (self.feet.y + 0.3).floor() as i32,
            self.feet.z.floor() as i32,
        );
        let eye_cell = IVec3::new(
            self.feet.x.floor() as i32,
            (self.feet.y + PLAYER_EYE_HEIGHT).floor() as i32,
            self.feet.z.floor() as i32,
        );
        let in_water = world.is_water_at(feet_cell) || world.is_water_at(eye_cell);
        let head_wet = world.is_water_at(eye_cell);
        if in_water {
            wish *= if head_wet { 0.6 } else { 0.8 };
        }
        // Accelerate / brake on XZ — soft approach for smoother HD-2D glide.
        let target = Vec3::new(wish.x, 0.0, wish.z);
        let cur = Vec3::new(self.velocity.x, 0.0, self.velocity.z);
        let rate = if wish.length_squared() > 0.0 {
            if keys.sprint {
                HORIZ_ACCEL * 1.15
            } else {
                HORIZ_ACCEL
            }
        } else {
            HORIZ_FRICTION
        };
        let max_delta = rate * TICK_DT;
        let diff = target - cur;
        let dist = diff.length();
        if dist <= max_delta || dist < 1e-6 {
            self.velocity.x = target.x;
            self.velocity.z = target.z;
        } else {
            let step = diff * (max_delta / dist);
            self.velocity.x += step.x;
            self.velocity.z += step.z;
        }

        if keys.up && self.on_ground {
            self.velocity.y = JUMP_SPEED;
            self.on_ground = false;
        }

        if in_water && !self.sink_boots {
            // Nado/flote: gravedad suave + flotabilidad a la superficie.
            // Espacio nada hacia arriba; sin tocar nada flota con la
            // cabeza fuera.
            self.velocity.y -= GRAVITY * 0.22 * TICK_DT;
            if head_wet {
                self.velocity.y += 34.0 * TICK_DT;
            }
            if keys.up {
                self.velocity.y += 30.0 * TICK_DT;
            }
            self.velocity.y = self.velocity.y.clamp(-2.5, 3.4);
        } else {
            self.velocity.y -= GRAVITY * TICK_DT;
            self.velocity.y = self.velocity.y.max(-40.0);
            if in_water && keys.up {
                // Con botas (chapoteo en la orilla): impulso para salir.
                self.velocity.y = (self.velocity.y + 26.0 * TICK_DT).min(6.0);
            }
        }

        // La corriente empuja río abajo (hacia nivel mayor).
        if in_water {
            let mid = IVec3::new(
                self.feet.x.floor() as i32,
                (self.feet.y + 0.9).floor() as i32,
                self.feet.z.floor() as i32,
            );
            let push = world.flow_push_at(mid);
            self.velocity.x += push.x * TICK_DT;
            self.velocity.z += push.z * TICK_DT;
        }

        let delta = self.velocity * TICK_DT;
        self.move_and_collide(world, delta);

        let horiz = (self.velocity.x * self.velocity.x + self.velocity.z * self.velocity.z).sqrt();

        // Discrete melee swing clock.
        if self.melee.active {
            self.melee.t += TICK_DT / MELEE_SWING_SECS;
            if self.melee.t >= 1.0 {
                self.melee.active = false;
                self.melee.t = 0.0;
            }
            self.walk_amount = (self.walk_amount - TICK_DT * 8.0).max(0.0);
            self.run_amount = (self.run_amount - TICK_DT * 10.0).max(0.0);
        }

        if digging {
            self.dig_phase += TICK_DT * 10.0;
            self.dig_amount = (self.dig_amount + TICK_DT * 10.0).min(1.0);
            if !self.melee.active {
                self.walk_amount = (self.walk_amount - TICK_DT * 8.0).max(0.0);
                self.run_amount = (self.run_amount - TICK_DT * 10.0).max(0.0);
                // Stroke 0..1 → sin(π t) arc for arm / tool.
                let stroke = (self.dig_phase / std::f32::consts::PI).rem_euclid(1.0);
                self.tool_swing = (stroke * std::f32::consts::PI).sin();
            }
        } else {
            self.dig_amount = (self.dig_amount - TICK_DT * 8.0).max(0.0);
            if !self.melee.active {
                self.tool_swing = (self.tool_swing - TICK_DT * 8.0).max(0.0);
            }
            if self.on_ground && horiz > 0.15 && !self.melee.active {
                let cadence = if keys.sprint { 7.2 } else { 4.5 };
                self.walk_phase += TICK_DT * cadence;
                self.walk_amount = (self.walk_amount + TICK_DT * 8.0).min(1.0);
                if keys.sprint {
                    self.run_amount = (self.run_amount + TICK_DT * 12.0).min(1.0);
                } else {
                    self.run_amount = (self.run_amount - TICK_DT * 10.0).max(0.0);
                }
            } else if !self.melee.active {
                self.walk_amount = (self.walk_amount - TICK_DT * 6.0).max(0.0);
                // En el aire con Shift se conserva el sprint para la pose
                // de salto (from_sprint_jump); al soltarlo decae normal.
                if self.on_ground || !keys.sprint {
                    self.run_amount = (self.run_amount - TICK_DT * 10.0).max(0.0);
                }
                // Brazadas: el reloj de nado avanza flotando en movimiento.
                if in_water && !self.on_ground && horiz > 0.15 {
                    self.walk_phase += TICK_DT * 6.0;
                }
            }
        }

        // Equip settle (lerp toward 1).
        if self.equip_blend < 1.0 {
            self.equip_blend = (self.equip_blend + TICK_DT * EQUIP_BLEND_RATE).min(1.0);
        }

        // Guardia de bloqueo (Shift+derecho con espada): subida rápida,
        // bajada algo más lenta para que no parpadee al soltar.
        if self.blocking {
            self.block_amount = (self.block_amount + TICK_DT * 10.0).min(1.0);
        } else {
            self.block_amount = (self.block_amount - TICK_DT * 8.0).max(0.0);
        }

        // Walk/sprint bob (single sin phase; run uses a punchier hop).
        if self.on_ground && horiz > 0.15 && !digging && !self.melee.active {
            let amp = if keys.sprint { 0.055 } else { 0.018 };
            let bob_phase = if keys.sprint {
                self.walk_phase * 2.0
            } else {
                self.walk_phase
            };
            self.bob_y = bob_phase.sin() * amp;
        } else {
            self.bob_y *= 0.75;
            if self.bob_y.abs() < 1e-4 {
                self.bob_y = 0.0;
            }
        }

        if self.melee.active {
            self.activity = PlayerActivity::Attack;
        } else if digging {
            self.activity = PlayerActivity::Dig;
        } else if self.equip_blend < 0.95 {
            self.activity = PlayerActivity::Equip;
        } else if !self.on_ground {
            self.activity = PlayerActivity::Jump;
        } else if horiz > 0.15 {
            self.activity = if keys.sprint {
                PlayerActivity::Sprint
            } else {
                PlayerActivity::Walk
            };
        } else {
            self.activity = PlayerActivity::Idle;
        }

        if self.on_ground {
            self.jump_amount = (self.jump_amount - TICK_DT * 10.0).max(0.0);
            self.last_safe_feet = self.feet;
        } else {
            self.jump_amount = (self.jump_amount + TICK_DT * 12.0).min(1.0);
        }

        // Nado: flotando sin tocar fondo (con botas se camina, no se nada).
        if in_water && !self.on_ground && !self.sink_boots {
            self.swim_amount = (self.swim_amount + TICK_DT * 8.0).min(1.0);
        } else {
            self.swim_amount = (self.swim_amount - TICK_DT * 8.0).max(0.0);
        }

        if self.feet.y < VOID_RESCUE_Y {
            self.rescue_from_void(world);
        }
    }

    /// Snap back to surface at the last safe footing and lose one heart.
    fn rescue_from_void(&mut self, world: &World) {
        let x = self.last_safe_feet.x.floor() as i32;
        let z = self.last_safe_feet.z.floor() as i32;
        let ground = world
            .column_height(x, z)
            .unwrap_or_else(|| terrain_height(x, z))
            .max(0) as f32;
        self.feet = Vec3::new(x as f32 + 0.5, ground + 1.0, z as f32 + 0.5);
        self.prev_feet = self.feet;
        self.velocity = Vec3::ZERO;
        self.on_ground = true;
        self.last_safe_feet = self.feet;
        self.hearts = self.hearts.saturating_sub(1);
        if self.hearts == 0 {
            self.hearts = MAX_HEARTS;
        }
    }

    fn move_and_collide(&mut self, world: &World, delta: Vec3) {
        // Axis-separated sweeps keep wall slides stable; try 1-block step-up on X/Z.
        self.try_move_axis(world, Vec3::new(delta.x, 0.0, 0.0));
        self.try_move_axis(world, Vec3::new(0.0, 0.0, delta.z));

        self.feet.y += delta.y;
        self.on_ground = false;
        if overlaps_solid(world, self.feet) {
            self.feet.y -= delta.y;
            if delta.y < 0.0 {
                self.on_ground = true;
            }
            self.velocity.y = 0.0;
        }
    }

    fn try_move_axis(&mut self, world: &World, delta: Vec3) {
        if delta.x == 0.0 && delta.z == 0.0 {
            return;
        }
        let before = self.feet;
        self.feet += delta;
        if !overlaps_solid(world, self.feet) {
            return;
        }
        // Blocked — try stepping up one block if grounded / near ground.
        self.feet = before;
        if self.velocity.y > 0.5 {
            if delta.x != 0.0 {
                self.velocity.x = 0.0;
            }
            if delta.z != 0.0 {
                self.velocity.z = 0.0;
            }
            return;
        }
        let stepped = before + delta + Vec3::Y * STEP_UP;
        if !overlaps_solid(world, stepped) {
            // Ensure we actually landed on something (not stepped into empty air shaft).
            let probe = stepped - Vec3::Y * 0.15;
            if overlaps_solid(world, probe) || self.on_ground {
                self.feet = stepped;
                self.on_ground = true;
                self.velocity.y = 0.0;
                return;
            }
        }
        if delta.x != 0.0 {
            self.velocity.x = 0.0;
        }
        if delta.z != 0.0 {
            self.velocity.z = 0.0;
        }
    }
}

fn aabb_for_feet(feet: Vec3) -> (Vec3, Vec3) {
    let min = Vec3::new(
        feet.x - PLAYER_HALF_WIDTH,
        feet.y,
        feet.z - PLAYER_HALF_WIDTH,
    );
    let max = Vec3::new(
        feet.x + PLAYER_HALF_WIDTH,
        feet.y + PLAYER_HEIGHT,
        feet.z + PLAYER_HALF_WIDTH,
    );
    (min, max)
}

fn overlaps_solid(world: &World, feet: Vec3) -> bool {
    let (min, max) = aabb_for_feet(feet);
    let eps = 1e-4;
    let amin = Vec3::new(min.x + eps, min.y + eps, min.z + eps);
    let amax = Vec3::new(max.x - eps, max.y - eps, max.z - eps);
    let x0 = amin.x.floor() as i32;
    let x1 = amax.x.floor() as i32;
    let y0 = amin.y.floor() as i32;
    let y1 = amax.y.floor() as i32;
    let z0 = amin.z.floor() as i32;
    let z1 = amax.z.floor() as i32;

    for y in y0..=y1 {
        for z in z0..=z1 {
            for x in x0..=x1 {
                let Some((bmin, bmax)) = world.collision_aabb(IVec3::new(x, y, z)) else {
                    continue;
                };
                if amin.x < bmax.x
                    && amax.x > bmin.x
                    && amin.y < bmax.y
                    && amax.y > bmin.y
                    && amin.z < bmax.z
                    && amax.z > bmin.z
                {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Voxel;

    fn test_player(feet: Vec3) -> Player {
        Player {
            feet,
            velocity: Vec3::ZERO,
            on_ground: true,
            facing: 0.0,
            hearts: MAX_HEARTS,
            bottles: 0,
            walk_phase: 0.0,
            walk_amount: 0.0,
            run_amount: 0.0,
            dig_phase: 0.0,
            dig_amount: 0.0,
            jump_amount: 0.0,
            swim_amount: 0.0,
            tool_swing: 0.0,
            bob_y: 0.0,
            sink_boots: false,
            equip_blend: 1.0,
            blocking: false,
            block_amount: 0.0,
            melee: MeleeAttack::default(),
            sword_combo: 0,
            next_fist_left: false,
            activity: PlayerActivity::Idle,
            last_safe_feet: feet,
            prev_feet: feet,
            prev_facing: 0.0,
            prev_walk_phase: 0.0,
            prev_walk_amount: 0.0,
            prev_run_amount: 0.0,
            prev_dig_phase: 0.0,
            prev_dig_amount: 0.0,
            prev_jump_amount: 0.0,
            prev_swim_amount: 0.0,
            prev_tool_swing: 0.0,
            prev_equip_blend: 1.0,
            prev_block_amount: 0.0,
            prev_bob_y: 0.0,
        }
    }

    #[test]
    fn display_feet_interpolates_bob_between_ticks() {
        let mut p = test_player(Vec3::new(0.5, 1.0, 0.5));
        p.prev_bob_y = 0.0;
        p.bob_y = 0.5;
        let mid = p.display_feet(0.5);
        assert!((mid.y - 1.25).abs() < 1e-4, "got y={}", mid.y);
        assert_eq!(p.display_feet(0.0).y, 1.0);
        assert_eq!(p.display_feet(1.0).y, 1.5);
    }

    #[test]
    fn walk_bob_advances_smoothly_across_ticks() {
        let mut world = World::new();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::dirt());
        for x in 0..20 {
            world.set_voxel(IVec3::new(x, 0, 0), Voxel::dirt());
        }
        for sprint in [false, true] {
            let mut player = test_player(Vec3::new(0.5, 1.0, 0.5));
            let keys = HeldKeys {
                forward: true,
                sprint,
                ..HeldKeys::default()
            };
            for _ in 0..60 {
                let previous = player.display_feet(1.0);
                player.tick(&world, &keys, 0.0, 1, false, false);
                assert!(player.display_feet(0.0).distance(previous) < 1e-5);
                let midpoint = player.display_feet(0.5);
                let expected = previous.lerp(player.display_feet(1.0), 0.5);
                assert!(midpoint.distance(expected) < 1e-5);
                player.tick(&world, &keys, 0.0, 0, false, false);
                assert_eq!(player.display_feet(0.5), midpoint);
            }
        }
    }

    #[test]
    fn first_person_strafe_and_backpedal_keep_facing() {
        let mut world = World::new();
        for x in -8..8 {
            for z in -8..8 {
                world.set_voxel(IVec3::new(x, 0, z), Voxel::dirt());
            }
        }
        // Facing +X. A/D must translate sideways, S backwards — view fixed.
        for (keys, dx, dz) in [
            (
                HeldKeys { left: true, ..HeldKeys::default() },
                0.0,
                -1.0,
            ),
            (
                HeldKeys { right: true, ..HeldKeys::default() },
                0.0,
                1.0,
            ),
            (
                HeldKeys { back: true, ..HeldKeys::default() },
                -1.0,
                0.0,
            ),
        ] {
            let mut player = test_player(Vec3::new(0.5, 1.0, 0.5));
            for _ in 0..20 {
                player.tick(&world, &keys, 0.0, 1, false, true);
            }
            assert_eq!(player.facing, 0.0, "FP must not turn the body");
            let moved = player.feet - Vec3::new(0.5, 1.0, 0.5);
            assert!(moved.x * dx + moved.z * dz > 0.5, "no FP translation: {moved:?}");
        }
    }

    #[test]
    fn third_person_still_turns_toward_walk_wish() {
        let mut world = World::new();
        for x in -8..8 {
            for z in -8..8 {
                world.set_voxel(IVec3::new(x, 0, z), Voxel::dirt());
            }
        }
        let mut player = test_player(Vec3::new(0.5, 1.0, 0.5));
        let keys = HeldKeys { left: true, ..HeldKeys::default() };
        for _ in 0..60 {
            player.tick(&world, &keys, 0.0, 1, false, false);
        }
        // Walk wish points -Z; the body eases from +X toward it.
        assert!(
            player.facing < -0.5,
            "HD-2D should turn toward wish, got {}",
            player.facing
        );
    }

    #[test]
    fn restore_clears_bob_history() {
        let mut player = test_player(Vec3::ONE);
        player.bob_y = 0.05;
        player.prev_bob_y = -0.04;
        let feet = Vec3::new(2.0, 3.0, 4.0);
        player.restore_from_save(feet, 0.0, MAX_HEARTS, 0);
        for alpha in [0.0, 0.5, 1.0] {
            assert_eq!(player.display_feet(alpha), feet);
        }
    }

    #[test]
    fn player_height_under_two_blocks() {
        assert!(PLAYER_HEIGHT < 2.0);
        assert!(PLAYER_EYE_HEIGHT < PLAYER_HEIGHT);
        assert!(PLAYER_EYE_HEIGHT > 1.0);
    }

    #[test]
    fn stands_on_dirt_without_sinking() {
        let mut world = World::new();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::dirt());
        let mut player = test_player(Vec3::new(0.5, 1.0, 0.5));
        player.on_ground = false;
        let keys = HeldKeys::default();
        for _ in 0..40 {
            player.tick_once(&world, &keys, 0.0, false, false);
        }
        assert!(player.on_ground);
        assert!((player.feet.y - 1.0).abs() < 0.05);
        assert!(player.eye_position().y < player.feet.y + 2.0);
    }

    #[test]
    fn trunk_blocks_horizontal_move() {
        let mut world = World::new();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::dirt());
        world.set_voxel(IVec3::new(1, 0, 0), Voxel::dirt());
        // 2-tall wall — step-up may climb a single block, not a full barrier.
        world.set_voxel(IVec3::new(0, 1, 0), Voxel::wood_trunk());
        world.set_voxel(IVec3::new(0, 2, 0), Voxel::wood_trunk());
        let mut player = test_player(Vec3::new(0.5, 1.0, 0.5));
        assert!(
            overlaps_solid(&world, player.feet),
            "player AABB must hit the 8×8 micro trunk"
        );
        player.feet = Vec3::new(1.4, 1.0, 0.5);
        player.prev_feet = player.feet;
        player.last_safe_feet = player.feet;
        assert!(!overlaps_solid(&world, player.feet));
        let mut keys = HeldKeys::default();
        keys.back = true;
        for _ in 0..40 {
            player.tick_once(&world, &keys, 0.0, false, false);
        }
        assert!(
            player.feet.x > 0.95,
            "should stop against trunk, got x={}",
            player.feet.x
        );
    }

    #[test]
    fn void_fall_rescues_and_costs_a_heart() {
        let mut world = World::new();
        world.set_voxel(IVec3::new(3, 5, 3), Voxel::dirt());
        let mut player = test_player(Vec3::new(3.5, 6.0, 3.5));
        player.feet.y = -5.0;
        player.velocity.y = -20.0;
        player.tick_once(&world, &HeldKeys::default(), 0.0, false, false);
        assert!(player.feet.y > 0.0, "rescued above void");
        assert_eq!(player.hearts, MAX_HEARTS - 1);
        assert!(player.on_ground);
    }

    #[test]
    fn sprint_is_faster_than_walk() {
        let mut world = World::new();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::dirt());
        for x in 0..20 {
            world.set_voxel(IVec3::new(x, 0, 0), Voxel::dirt());
        }
        let mut walk = test_player(Vec3::new(0.5, 1.0, 0.5));
        let mut sprint = test_player(Vec3::new(0.5, 1.0, 0.5));
        let mut keys_w = HeldKeys::default();
        keys_w.forward = true;
        let mut keys_s = keys_w;
        keys_s.sprint = true;
        for _ in 0..30 {
            walk.tick_once(&world, &keys_w, 0.0, false, false);
            sprint.tick_once(&world, &keys_s, 0.0, false, false);
        }
        assert!(sprint.feet.x > walk.feet.x + 0.5);
    }

    #[test]
    fn melee_sword_cycles_three_variants() {
        let mut p = test_player(Vec3::new(0.5, 1.0, 0.5));
        assert_eq!(p.begin_melee(HotbarItem::Sword, false), 20);
        assert!(matches!(p.melee.kind, AttackKind::Sword { variant: 0 }));
        p.melee.active = false;
        assert_eq!(p.begin_melee(HotbarItem::Sword, true), 20);
        assert!(matches!(p.melee.kind, AttackKind::Sword { variant: 1 }));
        assert!(p.melee.timid);
        p.melee.active = false;
        p.begin_melee(HotbarItem::Sword, false);
        assert!(matches!(p.melee.kind, AttackKind::Sword { variant: 2 }));
        p.melee.active = false;
        p.begin_melee(HotbarItem::Sword, false);
        assert!(matches!(p.melee.kind, AttackKind::Sword { variant: 0 }));
    }

    #[test]
    fn melee_fists_alternate_hands() {
        let mut p = test_player(Vec3::new(0.5, 1.0, 0.5));
        assert_eq!(p.begin_melee(HotbarItem::Empty, false), 1);
        assert!(matches!(p.melee.kind, AttackKind::Fist { left: false }));
        p.melee.active = false;
        p.begin_melee(HotbarItem::Empty, false);
        assert!(matches!(p.melee.kind, AttackKind::Fist { left: true }));
        p.melee.active = false;
        assert_eq!(p.begin_melee(HotbarItem::Pickaxe, false), 1);
        assert!(matches!(p.melee.kind, AttackKind::PickOverhead));
    }

    #[test]
    fn sword_thrust_ignores_combo_cycle() {        let mut p = test_player(Vec3::new(0.5, 1.0, 0.5));
        assert_eq!(p.begin_sword_thrust(false), 20);
        assert!(matches!(p.melee.kind, AttackKind::SwordThrust));
        p.melee.active = false;
        // El ciclo de tajos sigue donde estaba (la estocada no lo avanza).
        assert_eq!(p.begin_melee(HotbarItem::Sword, false), 20);
        assert!(matches!(p.melee.kind, AttackKind::Sword { variant: 0 }));
    }

    #[test]
    fn blocking_raises_guard_and_releases() {
        let mut world = World::new();
        world.set_voxel(IVec3::new(0, 0, 0), Voxel::dirt());
        let mut p = test_player(Vec3::new(0.5, 1.0, 0.5));
        p.blocking = true;
        for _ in 0..10 {
            p.tick_once(&world, &HeldKeys::default(), 0.0, false, false);
        }
        assert!((p.block_amount - 1.0).abs() < 1e-4, "got {}", p.block_amount);
        p.blocking = false;
        p.tick_once(&world, &HeldKeys::default(), 0.0, false, false);
        assert!(p.block_amount < 1.0);
        // La guardia se ve en la pose (brazo cruzado + agache).
        p.block_amount = 1.0;
        let pose = p.display_hero_pose(1.0);
        assert!(pose.r_elbow_x > 0.5, "got {}", pose.r_elbow_x);
        assert!(pose.l_knee_x < -0.1, "got {}", pose.l_knee_x);
    }

    fn water_world() -> World {
        // Piso en y=0..2 y agua en y=3..6.
        let mut world = World::new();
        for z in -2..=2 {
            for x in -2..=2 {
                world.fills_column_for_test(x, z, 2);
                for y in 3..=6 {
                    world.set_voxel(
                        IVec3::new(x, y, z),
                        Voxel::solid(crate::world::Material::Water),
                    );
                }
            }
        }
        world
    }

    #[test]
    fn swimmer_floats_while_boots_sink() {
        let world = water_world();
        // Sin botas: la flotabilidad lo sube (pies en y=3, cabeza tapada).
        let mut swim = test_player(Vec3::new(0.5, 3.0, 0.5));
        swim.sink_boots = false;
        for _ in 0..40 {
            swim.tick_once(&world, &HeldKeys::default(), 0.0, false, false);
        }
        assert!(
            swim.velocity.y > -0.5 || swim.feet.y > 3.0,
            "flota: vy={} feet={}",
            swim.velocity.y,
            swim.feet
        );
        // Con botas: se hunde al fondo y hace pie.
        let mut dive = test_player(Vec3::new(0.5, 6.0, 0.5));
        dive.sink_boots = true;
        for _ in 0..120 {
            dive.tick_once(&world, &HeldKeys::default(), 0.0, false, false);
        }
        assert!(dive.on_ground, "con botas hace pie");
        assert!(dive.feet.y < 3.5, "feet={}", dive.feet);
    }

    #[test]
    fn spawn_is_dry_land_above_sea() {
        let p = Player::spawn_on_terrain();
        let x = p.feet.x.floor() as i32;
        let z = p.feet.z.floor() as i32;
        assert!(
            terrain_height(x, z) >= crate::world::SEA_LEVEL + 2,
            "spawn húmedo"
        );
        for dz in -1..=1 {
            for dx in -1..=1 {
                assert!(
                    terrain_height(x + dx, z + dz) >= crate::world::SEA_LEVEL + 1,
                    "playa en spawn"
                );
            }
        }
    }

    #[test]
    fn swimming_uses_swim_pose_and_slow_speeds() {
        let world = water_world();
        // Flotando en movimiento: sube swim_amount y nada con brazadas.
        let mut p = test_player(Vec3::new(0.5, 3.0, 0.5));
        let mut keys = HeldKeys::default();
        keys.forward = true;
        for _ in 0..30 {
            p.tick_once(&world, &keys, 0.0, false, false);
        }
        assert!(p.swim_amount > 0.5, "got {}", p.swim_amount);
        let pose = p.display_hero_pose(1.0);
        // Brazadas amplias: algún brazo bien arriba.
        assert!(
            pose.l_arm_x < -1.0 || pose.r_arm_x < -1.0,
            "sin brazada: {:?}",
            pose
        );
        // En superficie (-20%): avanza menos que en tierra con mismos ticks.
        let mut land = World::new();
        for z in -2..=2 {
            for x in -2..=2 {
                land.fills_column_for_test(x, z, 2);
            }
        }
        let mut lp = test_player(Vec3::new(0.5, 3.0, 0.5));
        for _ in 0..30 {
            lp.tick_once(&land, &keys, 0.0, false, false);
        }
        let swim_d = (p.feet.x - 0.5).abs() + (p.feet.z - 0.5).abs();
        let land_d = (lp.feet.x - 0.5).abs() + (lp.feet.z - 0.5).abs();
        assert!(
            swim_d < land_d * 0.9,
            "agua debería frenar: {swim_d} vs {land_d}"
        );
    }
}
