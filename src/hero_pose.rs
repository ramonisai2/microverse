//! Hierarchical limb pose for the HD-2D hero (mirrors the Three.js animator).
//! Design space: X right, Y up, −Z face-forward.
//! Walk animates arms (shoulder + elbow) and legs (hip + knee + ankle) only.

use glam::Vec3;

/// Joint angles in radians (same axes as the HTML sliders).
#[derive(Clone, Copy, Debug)]
pub struct HeroPose {
    pub head_y: f32,
    pub l_arm_z: f32,
    pub r_arm_z: f32,
    pub l_arm_x: f32,
    pub r_arm_x: f32,
    /// Elbow flex (local X, positive = bend).
    pub l_elbow_x: f32,
    pub r_elbow_x: f32,
    pub l_leg_x: f32,
    pub r_leg_x: f32,
    /// Knee flex (local X, positive = bend).
    pub l_knee_x: f32,
    pub r_knee_x: f32,
    pub l_foot_x: f32,
    pub r_foot_x: f32,
}

impl Default for HeroPose {
    fn default() -> Self {
        Self::idle()
    }
}

impl HeroPose {
    pub fn idle() -> Self {
        Self {
            head_y: 0.0,
            l_arm_z: 0.0,
            r_arm_z: 0.0,
            l_arm_x: 0.0,
            r_arm_x: 0.0,
            l_elbow_x: 0.0,
            r_elbow_x: 0.0,
            l_leg_x: 0.0,
            r_leg_x: 0.0,
            l_knee_x: 0.0,
            r_knee_x: 0.0,
            l_foot_x: 0.0,
            r_foot_x: 0.0,
        }
    }

    /// Walk: arms + legs only (head stays still). Includes elbow and knee flex.
    /// `phase` is radians (`walkTime * 4.5`). `amount` blends 0 (idle) → 1 (full stride).
    pub fn from_walk(phase: f32, amount: f32) -> Self {
        let a = amount.clamp(0.0, 1.0);
        let s = phase.sin();
        let c = phase.cos();

        // Hip / shoulder swing (sagittal X), opposite sides.
        let leg = s * 28f32.to_radians() * a;
        let arm = -s * 22f32.to_radians() * a;

        // Knee bends backward (negative X in design space: Y up, −Z forward).
        let l_knee = -(-s).max(0.0) * 40f32.to_radians() * a;
        let r_knee = -s.max(0.0) * 40f32.to_radians() * a;

        // Elbow: resting bend + a bit more through the swing.
        let l_elbow = (18f32.to_radians() + s.abs() * 16f32.to_radians()) * a;
        let r_elbow = (18f32.to_radians() + s.abs() * 16f32.to_radians()) * a;

        // Light ankle follow.
        let ankle = c * 12f32.to_radians() * a;

        Self {
            head_y: 0.0,
            l_arm_z: 0.0,
            r_arm_z: 0.0,
            l_arm_x: arm,
            r_arm_x: -arm,
            l_elbow_x: l_elbow,
            r_elbow_x: r_elbow,
            l_leg_x: leg,
            r_leg_x: -leg,
            l_knee_x: l_knee,
            r_knee_x: r_knee,
            l_foot_x: ankle * 0.45,
            r_foot_x: -ankle * 0.45,
        }
    }

    /// Sprint: same gait as walk, but exaggerated swing / tuck / twist.
    /// `phase` shares the walk clock; `amount` blends 0 → full run caricature.
    pub fn from_run(phase: f32, amount: f32) -> Self {
        let a = amount.clamp(0.0, 1.0);
        let s = phase.sin();
        let c = phase.cos();
        let s2 = (phase * 2.0).sin(); // secondary bounce for shoulders / head

        // Big sagittal stride — almost cartoon.
        let leg = s * 52f32.to_radians() * a;
        let arm = -s * 58f32.to_radians() * a;

        // Deep knee tuck on the recovery leg.
        let l_knee = -(-s).max(0.0) * 78f32.to_radians() * a;
        let r_knee = -s.max(0.0) * 78f32.to_radians() * a;

        // Elbows pump hard (high guard + snap).
        let l_elbow = (32f32.to_radians() + s.abs() * 38f32.to_radians()) * a;
        let r_elbow = (32f32.to_radians() + s.abs() * 38f32.to_radians()) * a;

        // Wide arm arc + slight outward flare.
        let flare = 18f32.to_radians() * a;
        let ankle = c * 28f32.to_radians() * a;

        // Counter-twist: head yaws opposite the lead leg; slight bounce.
        let twist = -s * 14f32.to_radians() * a;
        let bounce = s2 * 6f32.to_radians() * a;

        Self {
            head_y: twist + bounce * 0.35,
            l_arm_z: flare + s * 8f32.to_radians() * a,
            r_arm_z: -flare - s * 8f32.to_radians() * a,
            l_arm_x: arm,
            r_arm_x: -arm,
            l_elbow_x: l_elbow,
            r_elbow_x: r_elbow,
            l_leg_x: leg,
            r_leg_x: -leg,
            l_knee_x: l_knee,
            r_knee_x: r_knee,
            l_foot_x: ankle * 0.55,
            r_foot_x: -ankle * 0.55,
        }
    }

    /// Dig chop: right arm swings down, left steadies, legs planted.
    /// `phase` advances each dig stroke; `amount` blends 0→1.
    pub fn from_dig(phase: f32, amount: f32) -> Self {
        let a = amount.clamp(0.0, 1.0);
        let stroke = phase.sin();
        // High raise then strike (−X = overhead / forward in design space).
        let r_arm = (-85f32.to_radians() + stroke * 95f32.to_radians()) * a;
        let l_arm = (12f32.to_radians() - stroke.abs() * 8f32.to_radians()) * a;
        let r_elbow = (28f32.to_radians() + stroke.max(0.0) * 30f32.to_radians()) * a;
        let l_elbow = 22f32.to_radians() * a;
        let lean = stroke * 8f32.to_radians() * a;

        Self {
            head_y: lean * 0.35,
            l_arm_z: 8f32.to_radians() * a,
            r_arm_z: -14f32.to_radians() * a,
            l_arm_x: l_arm,
            r_arm_x: r_arm,
            l_elbow_x: l_elbow,
            r_elbow_x: r_elbow,
            l_leg_x: 4f32.to_radians() * a,
            r_leg_x: -6f32.to_radians() * a,
            l_knee_x: -8f32.to_radians() * a,
            r_knee_x: -12f32.to_radians() * a,
            l_foot_x: 0.0,
            r_foot_x: 4f32.to_radians() * a,
        }
    }

    /// Pickaxe: overhead raise → slam down. `t` in 0..=1 through one strike.
    /// `timid` scales down for air swings.
    pub fn from_pick_overhead(t: f32, amount: f32, timid: bool) -> Self {
        let a = amount.clamp(0.0, 1.0) * if timid { 0.55 } else { 1.0 };
        let t = t.clamp(0.0, 1.0);
        // Wind-up (high raise) then chop: map t through a raised-then-down curve.
        let raise = (t * 2.0).min(1.0);
        let chop = ((t - 0.45) / 0.55).clamp(0.0, 1.0);
        let r_arm = (-100f32.to_radians() * raise + 90f32.to_radians() * chop) * a;
        let r_elbow = (18f32.to_radians() + 42f32.to_radians() * chop) * a;
        let l_arm = (10f32.to_radians() - 6f32.to_radians() * chop) * a;
        let lean = (-6f32.to_radians() * raise + 12f32.to_radians() * chop) * a;

        Self {
            head_y: lean * 0.4,
            l_arm_z: 6f32.to_radians() * a,
            r_arm_z: -16f32.to_radians() * a,
            l_arm_x: l_arm,
            r_arm_x: r_arm,
            l_elbow_x: 18f32.to_radians() * a,
            r_elbow_x: r_elbow,
            l_leg_x: 5f32.to_radians() * a,
            r_leg_x: -8f32.to_radians() * a,
            l_knee_x: -10f32.to_radians() * a,
            r_knee_x: -14f32.to_radians() * a,
            l_foot_x: 0.0,
            r_foot_x: 5f32.to_radians() * a,
        }
    }

    /// Sword: three slightly different horizontal/diagonal cuts. `variant` 0..2.
    pub fn from_sword_slash(variant: u8, t: f32, amount: f32, timid: bool) -> Self {
        let a = amount.clamp(0.0, 1.0) * if timid { 0.5 } else { 1.0 };
        let t = t.clamp(0.0, 1.0);
        let swing = (t * std::f32::consts::PI).sin(); // 0→1→0 arc
        let (r_arm_x, r_arm_z, l_arm_x, head) = match variant % 3 {
            0 => {
                // High diagonal right→left.
                (
                    (-25f32.to_radians() + swing * 55f32.to_radians()) * a,
                    (-20f32.to_radians() + swing * 45f32.to_radians()) * a,
                    (8f32.to_radians() - swing * 10f32.to_radians()) * a,
                    swing * 8f32.to_radians() * a,
                )
            }
            1 => {
                // Mid horizontal slash.
                (
                    (-10f32.to_radians() + swing * 35f32.to_radians()) * a,
                    (-35f32.to_radians() + swing * 70f32.to_radians()) * a,
                    (14f32.to_radians() - swing * 8f32.to_radians()) * a,
                    swing * 5f32.to_radians() * a,
                )
            }
            _ => {
                // Rising cut / reverse angle.
                (
                    (15f32.to_radians() - swing * 45f32.to_radians()) * a,
                    (15f32.to_radians() - swing * 40f32.to_radians()) * a,
                    (-6f32.to_radians() + swing * 12f32.to_radians()) * a,
                    -swing * 6f32.to_radians() * a,
                )
            }
        };
        let r_elbow = (28f32.to_radians() + swing * 22f32.to_radians()) * a;

        Self {
            head_y: head,
            l_arm_z: 10f32.to_radians() * a,
            r_arm_z: r_arm_z,
            l_arm_x: l_arm_x,
            r_arm_x,
            l_elbow_x: 20f32.to_radians() * a,
            r_elbow_x: r_elbow,
            l_leg_x: 6f32.to_radians() * a,
            r_leg_x: -10f32.to_radians() * a,
            l_knee_x: -8f32.to_radians() * a,
            r_knee_x: -12f32.to_radians() * a,
            l_foot_x: 0.0,
            r_foot_x: 4f32.to_radians() * a,
        }
    }

    /// Bare fists: alternating punches; pose changes each hit.
    pub fn from_fist(left_hand: bool, t: f32, amount: f32, timid: bool) -> Self {
        let a = amount.clamp(0.0, 1.0) * if timid { 0.45 } else { 1.0 };
        let t = t.clamp(0.0, 1.0);
        let punch = (t * std::f32::consts::PI).sin();
        let extend = punch * 48f32.to_radians() * a;
        let guard = 16f32.to_radians() * a;

        if left_hand {
            Self {
                head_y: punch * 5f32.to_radians() * a,
                l_arm_z: -8f32.to_radians() * a,
                r_arm_z: 12f32.to_radians() * a,
                l_arm_x: -extend,
                r_arm_x: guard,
                l_elbow_x: (10f32.to_radians() + punch * 30f32.to_radians()) * a,
                r_elbow_x: 28f32.to_radians() * a,
                l_leg_x: 8f32.to_radians() * a,
                r_leg_x: -6f32.to_radians() * a,
                l_knee_x: -10f32.to_radians() * a,
                r_knee_x: -8f32.to_radians() * a,
                l_foot_x: 0.0,
                r_foot_x: 3f32.to_radians() * a,
            }
        } else {
            Self {
                head_y: -punch * 5f32.to_radians() * a,
                l_arm_z: -12f32.to_radians() * a,
                r_arm_z: 8f32.to_radians() * a,
                l_arm_x: guard,
                r_arm_x: -extend,
                l_elbow_x: 28f32.to_radians() * a,
                r_elbow_x: (10f32.to_radians() + punch * 30f32.to_radians()) * a,
                l_leg_x: -6f32.to_radians() * a,
                r_leg_x: 8f32.to_radians() * a,
                l_knee_x: -8f32.to_radians() * a,
                r_knee_x: -10f32.to_radians() * a,
                l_foot_x: 3f32.to_radians() * a,
                r_foot_x: 0.0,
            }
        }
    }

    /// Jump / fall: tuck legs on ascent, reach on fall. `amount` 0→1 in air.
    pub fn from_jump(vel_y: f32, amount: f32) -> Self {
        let a = amount.clamp(0.0, 1.0);
        let ascending = (vel_y / 8.0).clamp(-1.0, 1.0);
        let arms = (25f32.to_radians() + ascending * 20f32.to_radians()) * a;
        let legs = (-15f32.to_radians() - ascending.max(0.0) * 25f32.to_radians()) * a;
        let knees = (-35f32.to_radians() - ascending.max(0.0) * 20f32.to_radians()) * a;

        Self {
            head_y: 0.0,
            l_arm_z: -12f32.to_radians() * a,
            r_arm_z: 12f32.to_radians() * a,
            l_arm_x: -arms,
            r_arm_x: -arms * 0.9,
            l_elbow_x: 20f32.to_radians() * a,
            r_elbow_x: 20f32.to_radians() * a,
            l_leg_x: legs,
            r_leg_x: legs * 0.85,
            l_knee_x: knees,
            r_knee_x: knees * 0.9,
            l_foot_x: 10f32.to_radians() * a,
            r_foot_x: 8f32.to_radians() * a,
        }
    }

    /// Linear blend of joint angles (`t` in 0..=1).
    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Self {
            head_y: mix(self.head_y, other.head_y),
            l_arm_z: mix(self.l_arm_z, other.l_arm_z),
            r_arm_z: mix(self.r_arm_z, other.r_arm_z),
            l_arm_x: mix(self.l_arm_x, other.l_arm_x),
            r_arm_x: mix(self.r_arm_x, other.r_arm_x),
            l_elbow_x: mix(self.l_elbow_x, other.l_elbow_x),
            r_elbow_x: mix(self.r_elbow_x, other.r_elbow_x),
            l_leg_x: mix(self.l_leg_x, other.l_leg_x),
            r_leg_x: mix(self.r_leg_x, other.r_leg_x),
            l_knee_x: mix(self.l_knee_x, other.l_knee_x),
            r_knee_x: mix(self.r_knee_x, other.r_knee_x),
            l_foot_x: mix(self.l_foot_x, other.l_foot_x),
            r_foot_x: mix(self.r_foot_x, other.r_foot_x),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BodyPart {
    Torso,
    Head,
    LArm,
    RArm,
    LForearm,
    RForearm,
    LLeg,
    RLeg,
    LShin,
    RShin,
    LFoot,
    RFoot,
}

/// Seams that open when walking — internal faces stay visible at the cut.
/// Other part boundaries (hair↔head, pauldron↔torso, etc.) stay glued.
pub const JOINT_PAIRS: &[(BodyPart, BodyPart)] = &[
    (BodyPart::Torso, BodyPart::LArm),
    (BodyPart::Torso, BodyPart::RArm),
    (BodyPart::LArm, BodyPart::LForearm),
    (BodyPart::RArm, BodyPart::RForearm),
    (BodyPart::Torso, BodyPart::LLeg),
    (BodyPart::Torso, BodyPart::RLeg),
    (BodyPart::LLeg, BodyPart::LShin),
    (BodyPart::RLeg, BodyPart::RShin),
    (BodyPart::LShin, BodyPart::LFoot),
    (BodyPart::RShin, BodyPart::RFoot),
];

#[inline]
pub fn is_anim_joint(a: BodyPart, b: BodyPart) -> bool {
    JOINT_PAIRS
        .iter()
        .any(|&(x, y)| (x == a && y == b) || (x == b && y == a))
}

/// Local-Y split inside an arm part (editor space: arm hangs toward −Y).
pub const ELBOW_SPLIT_Y: i32 = -3;
/// Local-Y split inside a leg part.
pub const KNEE_SPLIT_Y: i32 = -5;

/// Partition a design-space voxel into a limb (procedural / flat fallback).
pub fn classify_part(x: i32, y: i32, z: i32) -> BodyPart {
    let _ = z;
    if y <= 7 {
        return if x <= 0 {
            BodyPart::LFoot
        } else {
            BodyPart::RFoot
        };
    }
    // Shin then thigh
    if y <= 11 {
        return if x <= 0 {
            BodyPart::LShin
        } else {
            BodyPart::RShin
        };
    }
    if y <= 15 {
        return if x <= 0 {
            BodyPart::LLeg
        } else {
            BodyPart::RLeg
        };
    }
    // Arms: upper near shoulder (higher Y), forearm lower
    if x <= -5 {
        return if y >= 20 {
            BodyPart::LArm
        } else {
            BodyPart::LForearm
        };
    }
    if x >= 5 {
        return if y >= 20 {
            BodyPart::RArm
        } else {
            BodyPart::RForearm
        };
    }
    if y >= 25 {
        return BodyPart::Head;
    }
    if y >= 24 && x.abs() <= 2 {
        return BodyPart::Head;
    }
    BodyPart::Torso
}

/// Design-space joint centers (rest pose).
#[derive(Clone, Copy, Debug)]
pub struct HeroPivots {
    pub torso: Vec3,
    pub head: Vec3,
    pub l_arm: Vec3,
    pub r_arm: Vec3,
    pub l_forearm: Vec3,
    pub r_forearm: Vec3,
    pub l_leg: Vec3,
    pub r_leg: Vec3,
    pub l_shin: Vec3,
    pub r_shin: Vec3,
    pub l_foot: Vec3,
    pub r_foot: Vec3,
}

impl Default for HeroPivots {
    fn default() -> Self {
        Self::default_biped()
    }
}

impl HeroPivots {
    pub fn default_biped() -> Self {
        Self {
            torso: Vec3::new(0.0, 20.0, 0.0),
            head: Vec3::new(0.0, 26.0, 0.0),
            l_arm: Vec3::new(-6.5, 23.0, 0.0),
            r_arm: Vec3::new(6.5, 23.0, 0.0),
            l_forearm: Vec3::new(-6.5, 19.5, 0.0),
            r_forearm: Vec3::new(6.5, 19.5, 0.0),
            l_leg: Vec3::new(-2.5, 15.0, 0.0),
            r_leg: Vec3::new(2.5, 15.0, 0.0),
            l_shin: Vec3::new(-2.5, 10.0, 0.0),
            r_shin: Vec3::new(2.5, 10.0, 0.0),
            l_foot: Vec3::new(-2.5, 5.0, 0.0),
            r_foot: Vec3::new(2.5, 5.0, 0.0),
        }
    }

    pub fn get(self, part: BodyPart) -> Vec3 {
        match part {
            BodyPart::Torso => self.torso,
            BodyPart::Head => self.head,
            BodyPart::LArm => self.l_arm,
            BodyPart::RArm => self.r_arm,
            BodyPart::LForearm => self.l_forearm,
            BodyPart::RForearm => self.r_forearm,
            BodyPart::LLeg => self.l_leg,
            BodyPart::RLeg => self.r_leg,
            BodyPart::LShin => self.l_shin,
            BodyPart::RShin => self.r_shin,
            BodyPart::LFoot => self.l_foot,
            BodyPart::RFoot => self.r_foot,
        }
    }

    pub fn set(&mut self, part: BodyPart, p: Vec3) {
        match part {
            BodyPart::Torso => self.torso = p,
            BodyPart::Head => self.head = p,
            BodyPart::LArm => self.l_arm = p,
            BodyPart::RArm => self.r_arm = p,
            BodyPart::LForearm => self.l_forearm = p,
            BodyPart::RForearm => self.r_forearm = p,
            BodyPart::LLeg => self.l_leg = p,
            BodyPart::RLeg => self.r_leg = p,
            BodyPart::LShin => self.l_shin = p,
            BodyPart::RShin => self.r_shin = p,
            BodyPart::LFoot => self.l_foot = p,
            BodyPart::RFoot => self.r_foot = p,
        }
    }
}

/// Map editor part key → limb.
pub fn body_part_from_name(name: &str) -> Option<BodyPart> {
    match name {
        "torso" => Some(BodyPart::Torso),
        "head" => Some(BodyPart::Head),
        "lArm" | "l_arm" | "leftArm" => Some(BodyPart::LArm),
        "rArm" | "r_arm" | "rightArm" => Some(BodyPart::RArm),
        "lForearm" | "l_forearm" => Some(BodyPart::LForearm),
        "rForearm" | "r_forearm" => Some(BodyPart::RForearm),
        "lLeg" | "l_leg" | "leftLeg" => Some(BodyPart::LLeg),
        "rLeg" | "r_leg" | "rightLeg" => Some(BodyPart::RLeg),
        "lShin" | "l_shin" => Some(BodyPart::LShin),
        "rShin" | "r_shin" => Some(BodyPart::RShin),
        "lFoot" | "l_foot" | "leftFoot" => Some(BodyPart::LFoot),
        "rFoot" | "r_foot" | "rightFoot" => Some(BodyPart::RFoot),
        _ => None,
    }
}

fn rot_x(p: Vec3, a: f32) -> Vec3 {
    let (s, c) = a.sin_cos();
    Vec3::new(p.x, p.y * c - p.z * s, p.y * s + p.z * c)
}

fn rot_y(p: Vec3, a: f32) -> Vec3 {
    let (s, c) = a.sin_cos();
    Vec3::new(p.x * c + p.z * s, p.y, -p.x * s + p.z * c)
}

fn rot_z(p: Vec3, a: f32) -> Vec3 {
    let (s, c) = a.sin_cos();
    Vec3::new(p.x * c - p.y * s, p.x * s + p.y * c, p.z)
}

fn apply_shoulder(p: Vec3, shoulder: Vec3, arm_x: f32, arm_z: f32) -> Vec3 {
    shoulder + rot_z(rot_x(p - shoulder, arm_x), arm_z)
}

fn apply_hip(p: Vec3, hip: Vec3, leg_x: f32) -> Vec3 {
    hip + rot_x(p - hip, leg_x)
}

/// Map a design-space point through the limb hierarchy (rest → posed).
pub fn transform_point(part: BodyPart, pose: &HeroPose, pivots: &HeroPivots, p: Vec3) -> Vec3 {
    match part {
        BodyPart::Torso => p,
        BodyPart::Head => {
            // Kept for posing tools; walk leaves head_y = 0.
            let pivot = pivots.head;
            pivot + rot_y(p - pivot, pose.head_y)
        }
        BodyPart::LArm => apply_shoulder(p, pivots.l_arm, pose.l_arm_x, pose.l_arm_z),
        BodyPart::RArm => apply_shoulder(p, pivots.r_arm, pose.r_arm_x, pose.r_arm_z),
        BodyPart::LForearm => {
            let elbow = pivots.l_forearm;
            let after = elbow + rot_x(p - elbow, pose.l_elbow_x);
            apply_shoulder(after, pivots.l_arm, pose.l_arm_x, pose.l_arm_z)
        }
        BodyPart::RForearm => {
            let elbow = pivots.r_forearm;
            let after = elbow + rot_x(p - elbow, pose.r_elbow_x);
            apply_shoulder(after, pivots.r_arm, pose.r_arm_x, pose.r_arm_z)
        }
        BodyPart::LLeg => apply_hip(p, pivots.l_leg, pose.l_leg_x),
        BodyPart::RLeg => apply_hip(p, pivots.r_leg, pose.r_leg_x),
        BodyPart::LShin => {
            let knee = pivots.l_shin;
            let after = knee + rot_x(p - knee, pose.l_knee_x);
            apply_hip(after, pivots.l_leg, pose.l_leg_x)
        }
        BodyPart::RShin => {
            let knee = pivots.r_shin;
            let after = knee + rot_x(p - knee, pose.r_knee_x);
            apply_hip(after, pivots.r_leg, pose.r_leg_x)
        }
        BodyPart::LFoot => {
            let foot = pivots.l_foot;
            let knee = pivots.l_shin;
            let after_ankle = foot + rot_x(p - foot, pose.l_foot_x);
            let after_knee = knee + rot_x(after_ankle - knee, pose.l_knee_x);
            apply_hip(after_knee, pivots.l_leg, pose.l_leg_x)
        }
        BodyPart::RFoot => {
            let foot = pivots.r_foot;
            let knee = pivots.r_shin;
            let after_ankle = foot + rot_x(p - foot, pose.r_foot_x);
            let after_knee = knee + rot_x(after_ankle - knee, pose.r_knee_x);
            apply_hip(after_knee, pivots.r_leg, pose.r_leg_x)
        }
    }
}

/// Rotate a design-space normal with the same joint chain (no translation).
pub fn transform_normal(part: BodyPart, pose: &HeroPose, n: Vec3) -> Vec3 {
    match part {
        BodyPart::Torso => n,
        BodyPart::Head => rot_y(n, pose.head_y),
        BodyPart::LArm => rot_z(rot_x(n, pose.l_arm_x), pose.l_arm_z),
        BodyPart::RArm => rot_z(rot_x(n, pose.r_arm_x), pose.r_arm_z),
        BodyPart::LForearm => {
            rot_z(
                rot_x(rot_x(n, pose.l_elbow_x), pose.l_arm_x),
                pose.l_arm_z,
            )
        }
        BodyPart::RForearm => {
            rot_z(
                rot_x(rot_x(n, pose.r_elbow_x), pose.r_arm_x),
                pose.r_arm_z,
            )
        }
        BodyPart::LLeg => rot_x(n, pose.l_leg_x),
        BodyPart::RLeg => rot_x(n, pose.r_leg_x),
        BodyPart::LShin => rot_x(rot_x(n, pose.l_knee_x), pose.l_leg_x),
        BodyPart::RShin => rot_x(rot_x(n, pose.r_knee_x), pose.r_leg_x),
        BodyPart::LFoot => rot_x(
            rot_x(rot_x(n, pose.l_foot_x), pose.l_knee_x),
            pose.l_leg_x,
        ),
        BodyPart::RFoot => rot_x(
            rot_x(rot_x(n, pose.r_foot_x), pose.r_knee_x),
            pose.r_leg_x,
        ),
    }
}
