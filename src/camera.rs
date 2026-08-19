use glam::{Mat4, Vec3};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
use winit::keyboard::KeyCode;

/// Fixed simulation rate (movement steps per second of wall time).
pub const TICKS_PER_SECOND: u32 = 20;
const MOUSE_SENSITIVITY: f32 = 0.0025;
const PITCH_LIMIT: f32 = FRAC_PI_2 - 0.01;
/// Cap catch-up so a hitch doesn't teleport the camera.
pub const MAX_TICKS_PER_FRAME: u32 = 5;

/// HD-2D artistic perspective (not true isometric 35.264°).
const HD2D_YAW: f32 = FRAC_PI_4; // 45°
const HD2D_PITCH: f32 = -35.0_f32.to_radians();
const HD2D_FOVY_DEG: f32 = 25.0;
/// Distance from focus along the look ray (world blocks).
const HD2D_DISTANCE_BASE: f32 = 16.0;
/// Pull in when the frustum is crowded / remeshing (hides far LOD pop-in).
const HD2D_DISTANCE_MIN: f32 = 10.5;
/// Slight pull-back when the scene is sparse.
const HD2D_DISTANCE_MAX: f32 = 17.5;
/// Tight dig / cave / under canopy — keep the hero readable.
const HD2D_DISTANCE_CONFINED: f32 = 7.0;
/// How fast the lens eases toward the adaptive distance.
const HD2D_DISTANCE_LERP: f32 = 0.10;
/// Soft-follow the focus point only — angles stay locked (no accidental orbit).
const HD2D_FOCUS_LERP: f32 = 0.22;
/// Q/E orbit snaps (radians).
pub const HD2D_ORBIT_STEP: f32 = FRAC_PI_4;

pub struct Camera {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub aspect: f32,
    pub fovy: f32,
    pub znear: f32,
    pub zfar: f32,
    /// Extra yaw from Q/E snaps (HD-2D only). Base yaw stays [`HD2D_YAW`].
    pub hd2d_orbit: f32,
    /// Smoothed look-at target; camera sits on a fixed isometric ray from this.
    hd2d_focus: Vec3,
    /// Smoothed follow distance (adaptive to scene load / pop pressure).
    hd2d_distance: f32,
}

impl Camera {
    /// First-person view synced from [`crate::player::Player`].
    pub fn first_person(eye: Vec3) -> Self {
        use crate::world::{TERRAIN_MAX_HEIGHT, VIEW_DISTANCE};
        let peak = TERRAIN_MAX_HEIGHT as f32;
        Self {
            position: eye,
            yaw: -std::f32::consts::FRAC_PI_2,
            pitch: -0.15,
            aspect: 16.0 / 9.0,
            fovy: 70.0_f32.to_radians(),
            znear: 0.05,
            zfar: (VIEW_DISTANCE * 3.0).max(peak * 2.5),
            hd2d_orbit: 0.0,
            hd2d_focus: eye,
            hd2d_distance: HD2D_DISTANCE_BASE,
        }
    }

    /// HD-2D diorama: fixed yaw/pitch/FOV; only Q/E change the orbit angle.
    pub fn hd2d_follow(focus: Vec3) -> Self {
        use crate::world::{TERRAIN_MAX_HEIGHT, VIEW_DISTANCE};
        let peak = TERRAIN_MAX_HEIGHT as f32;
        let mut cam = Self {
            position: focus,
            yaw: HD2D_YAW,
            pitch: HD2D_PITCH,
            aspect: 16.0 / 9.0,
            fovy: HD2D_FOVY_DEG.to_radians(),
            znear: 0.1,
            zfar: (VIEW_DISTANCE * 3.0).max(peak * 2.5),
            hd2d_orbit: 0.0,
            hd2d_focus: focus,
            hd2d_distance: HD2D_DISTANCE_BASE,
        };
        cam.seat_hd2d();
        cam
    }

    /// Lock angles and place the lens on the isometric ray behind `hd2d_focus`.
    fn seat_hd2d(&mut self) {
        self.yaw = HD2D_YAW + self.hd2d_orbit;
        self.pitch = HD2D_PITCH;
        self.fovy = HD2D_FOVY_DEG.to_radians();
        self.position = self.hd2d_focus - self.forward() * self.hd2d_distance;
    }

    /// Soft-follow the player: angles never change (only Q/E may).
    /// `pop_pressure` in 0..=1 pulls closer for LOD; `confine` in 0..=1 pulls
    /// much closer in digs / under cover so the hero stays readable.
    /// `frame_half_span` (blocks) pulls the lens farther so a trail of that
    /// half-length around the focus stays readable (stair ghost framing).
    pub fn follow_hd2d(&mut self, focus: Vec3, pop_pressure: f32, confine: f32) {
        self.follow_hd2d_framed(focus, pop_pressure, confine, 0.0);
    }

    pub fn follow_hd2d_framed(
        &mut self,
        focus: Vec3,
        pop_pressure: f32,
        confine: f32,
        frame_half_span: f32,
    ) {
        self.hd2d_focus = self.hd2d_focus.lerp(focus, HD2D_FOCUS_LERP);
        let p = pop_pressure.clamp(0.0, 1.0);
        // Sparse → slightly farther; crowded / remeshing → closer.
        let open_target = if p < 0.25 {
            let t = p / 0.25;
            HD2D_DISTANCE_MAX + (HD2D_DISTANCE_BASE - HD2D_DISTANCE_MAX) * t
        } else {
            let t = ((p - 0.25) / 0.75).clamp(0.0, 1.0);
            HD2D_DISTANCE_BASE + (HD2D_DISTANCE_MIN - HD2D_DISTANCE_BASE) * t
        };
        let frame_boost = (frame_half_span * 0.9).clamp(0.0, 16.0);
        let open_target = open_target + frame_boost;
        let c = confine.clamp(0.0, 1.0);
        let c2 = c * c * (3.0 - 2.0 * c); // smoothstep
        let target = open_target + (HD2D_DISTANCE_CONFINED - open_target) * c2;
        // Dive into pits a bit faster than easing back out in the open.
        let lerp = HD2D_DISTANCE_LERP + c2 * 0.14;
        self.hd2d_distance += (target - self.hd2d_distance) * lerp;
        self.seat_hd2d();
    }

    /// Focus point the HD-2D lens orbits (player). Used for grass density so
    /// isometric camera offset does not strip tufts under the character.
    pub fn hd2d_focus(&self) -> Vec3 {
        self.hd2d_focus
    }

    /// Snap orbit ±45° (Q / E) to inspect terrain; stays isometric.
    pub fn orbit_hd2d(&mut self, steps: i32, focus: Vec3) {
        self.hd2d_orbit += steps as f32 * HD2D_ORBIT_STEP;
        while self.hd2d_orbit > std::f32::consts::PI {
            self.hd2d_orbit -= std::f32::consts::TAU;
        }
        while self.hd2d_orbit <= -std::f32::consts::PI {
            self.hd2d_orbit += std::f32::consts::TAU;
        }
        // Snap focus so the turn feels immediate.
        self.hd2d_focus = focus;
        self.seat_hd2d();
    }

    /// Flat XZ walk direction for WASD (locked camera yaw, pitch ignored).
    pub fn move_yaw(&self) -> f32 {
        self.yaw
    }

    #[allow(dead_code)]
    pub fn looking_at_shunk() -> Self {
        Self::first_person(crate::player::Player::spawn_on_terrain().eye_position())
    }

    #[allow(dead_code)]
    pub fn looking_at_origin() -> Self {
        Self {
            position: Vec3::new(2.8, 2.4, 4.2),
            yaw: -2.05,
            pitch: -0.28,
            aspect: 16.0 / 9.0,
            fovy: 60.0_f32.to_radians(),
            znear: 0.1,
            zfar: 200.0,
            hd2d_orbit: 0.0,
            hd2d_focus: Vec3::ZERO,
            hd2d_distance: HD2D_DISTANCE_BASE,
        }
    }

    /// Orbit a floating debug cube at (0, 8, 0).
    pub fn looking_at_face_debug() -> Self {
        Self {
            position: Vec3::new(4.5, 10.5, 4.5),
            yaw: -2.35,
            pitch: -0.35,
            aspect: 16.0 / 9.0,
            fovy: 60.0_f32.to_radians(),
            znear: 0.1,
            zfar: 200.0,
            hd2d_orbit: 0.0,
            hd2d_focus: Vec3::new(0.0, 8.0, 0.0),
            hd2d_distance: HD2D_DISTANCE_BASE,
        }
    }

    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        )
        .normalize()
    }

    #[allow(dead_code)]
    pub fn right(&self) -> Vec3 {
        self.forward().cross(Vec3::Y).normalize()
    }

    pub fn view_proj(&self) -> Mat4 {
        // look_to with locked yaw/pitch — never re-derives angles from position.
        let view = Mat4::look_to_rh(self.position, self.forward(), Vec3::Y);
        let proj = Mat4::perspective_rh(self.fovy, self.aspect, self.znear, self.zfar);
        proj * view
    }

    /// Six frustum planes as `ax+by+cz+d >= 0` inside (from clip matrix).
    pub fn frustum_planes(&self) -> [glam::Vec4; 6] {
        let m = self.view_proj();
        let r0 = m.row(0);
        let r1 = m.row(1);
        let r2 = m.row(2);
        let r3 = m.row(3);
        let mut planes = [
            r3 + r0, // left
            r3 - r0, // right
            r3 + r1, // bottom
            r3 - r1, // top
            r3 + r2, // near
            r3 - r2, // far
        ];
        for p in &mut planes {
            let len = glam::Vec3::new(p.x, p.y, p.z).length().max(1e-6);
            *p /= len;
        }
        planes
    }

    /// True if the AABB intersects the view frustum.
    pub fn aabb_visible(&self, min: Vec3, max: Vec3) -> bool {
        for plane in self.frustum_planes() {
            let nx = if plane.x >= 0.0 { max.x } else { min.x };
            let ny = if plane.y >= 0.0 { max.y } else { min.y };
            let nz = if plane.z >= 0.0 { max.z } else { min.z };
            if plane.x * nx + plane.y * ny + plane.z * nz + plane.w < 0.0 {
                return false;
            }
        }
        true
    }

    pub fn apply_mouse_delta(&mut self, dx: f64, dy: f64) {
        self.yaw += dx as f32 * MOUSE_SENSITIVITY;
        // Mouse up (negative dy on Windows) → look up.
        self.pitch -= dy as f32 * MOUSE_SENSITIVITY;
        self.pitch = self.pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    pub fn sync_from_player(&mut self, eye: Vec3) {
        self.position = eye;
    }
}

#[derive(Default, Clone, Copy)]
pub struct HeldKeys {
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
    pub up: bool,
    /// Crouch / descend (Ctrl).
    pub down: bool,
    /// Sprint (Shift).
    pub sprint: bool,
}

impl HeldKeys {
    pub fn set(&mut self, key: KeyCode, pressed: bool) {
        match key {
            KeyCode::KeyW => self.forward = pressed,
            KeyCode::KeyS => self.back = pressed,
            KeyCode::KeyA => self.left = pressed,
            KeyCode::KeyD => self.right = pressed,
            KeyCode::Space => self.up = pressed,
            KeyCode::ControlLeft | KeyCode::ControlRight => self.down = pressed,
            KeyCode::ShiftLeft | KeyCode::ShiftRight => self.sprint = pressed,
            _ => {}
        }
    }
}
