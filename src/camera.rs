use glam::{Mat4, Vec3, Vec4};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
use winit::keyboard::KeyCode;

use crate::player::Player;

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
/// Pull-in target while frames hitch (fill-bound slowness with clean queues,
/// where `pop_pressure` reads 0). Between BASE and MIN: reads as "push toward
/// the hero", never confused with the 7.0 cave confine.
const HD2D_DISTANCE_HITCH: f32 = 12.0;
/// How fast the lens eases toward the adaptive distance.
const HD2D_DISTANCE_LERP: f32 = 0.10;
/// Soft-follow the focus point only — angles stay locked (no accidental orbit).
const HD2D_FOCUS_LERP: f32 = 0.22;
/// Q/E orbit snaps (radians).
pub const HD2D_ORBIT_STEP: f32 = FRAC_PI_4;
/// Seconds for the smooth HD-2D rotation once the target view is preloaded.
const ORBIT_ANIM_SECS: f32 = 0.45;
/// Camera-facing weight above which Q/E defers its snap (diorama lens).
pub const ORBIT_WAIT_HD2D_MIN: f32 = 0.25;

/// A Q/E snap that waits for the target-view shunks before turning.
#[derive(Clone, Copy)]
pub struct OrbitSnap {
    /// Steps asked (`-1` Q / `1` E); the target orbit angle is precomputed.
    pub to: f32,
    /// Absolute yaw of the pending view (HD-2D pitch/fov), used to bias streaming.
    pub look_yaw: f32,
}

/// In-progress smooth rotation from `from` to `to` (angle ease-over-time).
#[derive(Clone, Copy)]
struct OrbitAnim {
    from: f32,
    to: f32,
    t: f32,
}

/// Seconds for the HD-2D ↔ first-person transition (distance collapse/ease).
const FP_BLEND_SECS: f32 = 0.45;
/// FOV pulled in during the HD-2D phase so the collapse keeps one horizon.
const HD2D_FOVY: f32 = HD2D_FOVY_DEG.to_radians();
/// First-person FOV (Minecraft-like default ~70°).
const FP_FOVY: f32 = 70.0_f32.to_radians();

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
    /// Zoom manual táctil (pellizco) en bloques sobre la distancia objetivo.
    /// 0 = encuadre artístico por defecto; el confine de cueva sigue mandando.
    zoom_bias: f32,
    /// `1` = full HD-2D diorama, `0` = first person. Smoothed over [`FP_BLEND_SECS`].
    hd2d_amount: f32,
    /// Requested HD-2D weight target (`hd2d_amount` eases toward this).
    hd2d_target: f32,
    /// First-person look pitch (only driven by mouse while in FP).
    pub fp_pitch: f32,
    /// Deferred Q/E snap: preloading the target view; rotation happens when ready.
    pending_orbit: Option<OrbitSnap>,
    /// Smooth rotation currently animating (from → to).
    orbit_anim: Option<OrbitAnim>,
}

/// FP horizontal sensitivity — finer than FPS-mode for gentle nudges.
const FP_MOUSE_SENS: f32 = MOUSE_SENSITIVITY * 0.55;
/// FP vertical sensitivity — slightly softer than horizontal.
const FP_MOUSE_SENS_Y: f32 = MOUSE_SENSITIVITY * 0.4;

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
            zoom_bias: 0.0,
            hd2d_amount: 0.0,
            hd2d_target: 0.0,
            fp_pitch: -0.15,
            pending_orbit: None,
            orbit_anim: None,
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
            zoom_bias: 0.0,
            hd2d_amount: 1.0,
            hd2d_target: 1.0,
            fp_pitch: -0.15,
            pending_orbit: None,
            orbit_anim: None,
        };
        cam.seat_hd2d();
        cam
    }

    /// Request full HD-2D diorama mode (`amount` eases toward 1).
    pub fn request_hd2d(&mut self) {
        self.hd2d_target = 1.0;
    }

    /// Request first-person view (Minecraft-style). `hd2d_amount` eases to 0.
    pub fn request_first_person(&mut self) {
        self.hd2d_target = 0.0;
    }

    /// Current HD-2D weight (1 = diorama, 0 = first person).
    pub fn hd2d_amount(&self) -> f32 {
        self.hd2d_amount
    }

    /// True when the lens is (almost) fully first-person.
    pub fn is_first_person(&self) -> bool {
        self.hd2d_amount <= 0.01
    }

    /// Advance the HD-2D ↔ first-person blend toward its target.
    /// Also returns the eased weight so callers can gate visuals.
    pub fn update_mode(&mut self, dt: f32) -> f32 {
        let rate = (FP_BLEND_SECS.max(1e-4)).recip();
        if self.hd2d_amount < self.hd2d_target {
            self.hd2d_amount = (self.hd2d_amount + rate * dt).min(self.hd2d_target);
        } else {
            self.hd2d_amount = (self.hd2d_amount - rate * dt).max(self.hd2d_target);
        }
        self.hd2d_amount
    }

    /// Place the lens for the current blend: HD-2D ray seat when blended in,
    /// player eye + walk-facing + fp_pitch when fully first-person.
    pub fn seat_mixed(&mut self, eye: Vec3, facing: f32) {
        if self.hd2d_amount >= 0.999 {
            self.seat_hd2d();
            return;
        }
        if self.hd2d_amount <= 0.001 {
            // Pure first person: hero yaw + mouse pitch.
            self.position = eye;
            self.yaw = facing;
            self.pitch = self.fp_pitch;
            self.fovy = FP_FOVY;
            return;
        }
        // In transition: slide from isometric ray toward the eye as HD-2D
        // weight fades, keeping the same look target → no frame pop.
        let fp_weight = 1.0 - self.hd2d_amount;
        let ray_pos = self.hd2d_focus - self.forward() * self.hd2d_distance;
        self.position = ray_pos.lerp(eye, fp_weight);
        self.yaw = HD2D_YAW + self.hd2d_orbit;
        self.pitch = HD2D_PITCH + (self.fp_pitch - HD2D_PITCH) * fp_weight;
        self.fovy = HD2D_FOVY + (FP_FOVY - HD2D_FOVY) * fp_weight;
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
    /// `fps_pull` in 0..=1 pulls toward [`HD2D_DISTANCE_HITCH`] while frames
    /// hitch (fill-bound slowness); `confine` still wins underground.
    pub fn follow_hd2d(&mut self, focus: Vec3, pop_pressure: f32, confine: f32, dt: f32) {
        self.follow_hd2d_framed(focus, pop_pressure, confine, 0.0, 0.0, dt);
    }

    pub fn follow_hd2d_framed(
        &mut self,
        focus: Vec3,
        pop_pressure: f32,
        confine: f32,
        frame_half_span: f32,
        fps_pull: f32,
        dt: f32,
    ) {
        let t = dt.max(0.0) * 60.0;
        let focus_lerp = 1.0 - (1.0 - HD2D_FOCUS_LERP).powf(t);
        self.hd2d_focus = self.hd2d_focus.lerp(focus, focus_lerp);
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
        let mut open_target = open_target + frame_boost;
        // Zoom manual táctil: desplaza el objetivo (el confine de cueva,
        // aplicado después, sigue mandando bajo tierra). Bias 0 = idéntico.
        open_target = (open_target + self.zoom_bias).clamp(HD2D_DISTANCE_MIN, 32.0);
        // Hitch pull-in: only ever pulls closer (never pushes out past a
        // stair-ghost frame), so planning a dig is not yanked around.
        let h = fps_pull.clamp(0.0, 1.0);
        if h > 0.0 && open_target > HD2D_DISTANCE_HITCH {
            let h2 = h * h * (3.0 - 2.0 * h); // smoothstep
            open_target += (HD2D_DISTANCE_HITCH - open_target) * h2;
        }
        let c = confine.clamp(0.0, 1.0);
        let c2 = c * c * (3.0 - 2.0 * c); // smoothstep
        let target = open_target + (HD2D_DISTANCE_CONFINED - open_target) * c2;
        // Dive into pits a bit faster than easing back out in the open.
        let dist_lerp = HD2D_DISTANCE_LERP + c2 * 0.14;
        let dist_lerp = 1.0 - (1.0 - dist_lerp).powf(t);
        self.hd2d_distance += (target - self.hd2d_distance) * dist_lerp;
        self.seat_hd2d();
    }

    /// Focus point the HD-2D lens orbits (player). Used for grass density so
    /// isometric camera offset does not strip tufts under the character.
    pub fn hd2d_focus(&self) -> Vec3 {
        self.hd2d_focus
    }

    /// Reorienta el diorama al salir por una puerta: gira la órbita al paso de
    /// 45° más cercano para que la referencia quede en Arriba-Derecha de
    /// pantalla (esa diagonal en mundo es `yaw + 45°`). La referencia es el
    /// rumbo, salvo en reversa (`reverse` = saliendo de espaldas) donde es
    /// el opuesto y el rumbo queda en Abajo-Izquierda. Instantáneo a
    /// propósito: se ejecuta aún en 1ª persona y el blend a diorama revela
    /// la vista ya alineada con el personaje.
    pub fn snap_orbit_to_face(&mut self, facing: f32, reverse: bool) {
        self.cancel_orbits();
        let reference = if reverse {
            facing + std::f32::consts::PI
        } else {
            facing
        };
        let steps = ((reference - FRAC_PI_4 - HD2D_YAW) / HD2D_ORBIT_STEP).round() as i32;
        self.hd2d_orbit = Self::normalize_orbit_angle(steps as f32 * HD2D_ORBIT_STEP);
    }

    /// Snap orbit ±45° (Q / E) to inspect terrain; stays isometric.
    pub fn orbit_hd2d(&mut self, steps: i32, focus: Vec3) {        self.hd2d_orbit += steps as f32 * HD2D_ORBIT_STEP;
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

    /// Normalize an orbit angle to `(-π, π]` (same convention as snaps).
    fn normalize_orbit_angle(a: f32) -> f32 {
        let pi = std::f32::consts::PI;
        let tau = std::f32::consts::TAU;
        let mut a = a.rem_euclid(tau);
        if a > pi {
            a -= tau;
        }
        if a <= -pi {
            a += tau;
        }
        a
    }

    /// Queue a Q/E snap that starts rotating on the next frame (`update_orbit_anim`
    /// + gate in `main.rs`). Only the diorama lens defers one frame (first
    /// person keeps the instant snap).
    /// Rapid presses accumulate: a second Q/E while a snap waits extends the
    /// pending target (2×45° = 90°), and one mid-rotation extends the anim —
    /// nothing is silently dropped.
    pub fn request_orbit(&mut self, steps: i32, focus: Vec3) -> bool {
        if steps == 0 {
            return false;
        }
        if let Some(snap) = self.pending_orbit {
            let to = Self::normalize_orbit_angle(snap.to + steps as f32 * HD2D_ORBIT_STEP);
            self.pending_orbit = Some(OrbitSnap {
                to,
                look_yaw: HD2D_YAW + to,
            });
            return true;
        }
        if let Some(mut a) = self.orbit_anim.take() {
            a.to = Self::normalize_orbit_angle(a.to + steps as f32 * HD2D_ORBIT_STEP);
            self.orbit_anim = Some(a);
            return true;
        }
        if self.hd2d_amount < ORBIT_WAIT_HD2D_MIN {
            self.orbit_hd2d(steps, focus);
            return true;
        }
        let to = Self::normalize_orbit_angle(self.hd2d_orbit + steps as f32 * HD2D_ORBIT_STEP);
        let look_yaw = HD2D_YAW + to;
        self.pending_orbit = Some(OrbitSnap { to, look_yaw });
        true
    }

    /// The deferred target orbit angle, if a Q/E snap is waiting on preload.
    pub fn pending_orbit(&self) -> Option<OrbitSnap> {
        self.pending_orbit
    }

    /// True while a Q/E snap is either waiting to preload or rotating.
    pub fn orbit_busy(&self) -> bool {
        self.pending_orbit.is_some() || self.orbit_anim.is_some()
    }

    /// Flat forward vector of the pending view (used to bias streaming).
    pub fn pending_look(&self) -> Option<Vec3> {
        self.pending_orbit.map(|s| {
            Vec3::new(
                s.look_yaw.cos() * HD2D_PITCH.cos(),
                HD2D_PITCH.sin(),
                s.look_yaw.sin() * HD2D_PITCH.cos(),
            )
            .normalize()
        })
    }

    /// Start the smooth rotation toward the preloaded view. No-op with no request.
    pub fn begin_orbit(&mut self, focus: Vec3) {
        let Some(snap) = self.pending_orbit.take() else {
            return;
        };
        if (self.hd2d_orbit - snap.to).abs() <= 1e-4 {
            // Already at the target angle (e.g. mouse drag aligned) → snap focus.
            self.hd2d_orbit = snap.to;
            self.hd2d_focus = focus;
            self.seat_hd2d();
            return;
        }
        self.hd2d_focus = focus;
        self.orbit_anim = Some(OrbitAnim {
            from: self.hd2d_orbit,
            to: snap.to,
            t: 0.0,
        });
    }

    /// Cancel a waiting snap (mode change / player override).
    pub fn cancel_orbits(&mut self) {
        self.pending_orbit = None;
        self.orbit_anim = None;
    }

    /// Advance the smooth rotation; drives the HD-2D ray seat while animating.
    pub fn update_orbit_anim(&mut self, dt: f32) {
        let Some(mut a) = self.orbit_anim.take() else {
            return;
        };
        let rate = (ORBIT_ANIM_SECS.max(1e-4)).recip();
        a.t = (a.t + dt * rate).min(1.0);
        let e = a.t * a.t * (3.0 - 2.0 * a.t); // smoothstep ease
        self.hd2d_orbit = Self::normalize_orbit_angle(a.from + (a.to - a.from) * e);
        self.seat_hd2d();
        if a.t >= 1.0 {
            self.hd2d_orbit = a.to;
        } else {
            self.orbit_anim = Some(a);
        }
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
            zoom_bias: 0.0,
            hd2d_amount: 1.0,
            hd2d_target: 1.0,
            fp_pitch: -0.28,
            pending_orbit: None,
            orbit_anim: None,
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
            zoom_bias: 0.0,
            hd2d_amount: 1.0,
            hd2d_target: 1.0,
            fp_pitch: -0.35,
            pending_orbit: None,
            orbit_anim: None,
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

    /// Rayo por un píxel de pantalla (coords lógicas, origen arriba-izquierda).
    /// Para tap-to-mine táctil: el centro devuelve `forward()`. Pura y testeable.
    pub fn screen_ray(&self, x: f32, y: f32, lw: f32, lh: f32) -> (Vec3, Vec3) {
        let inv = self.view_proj().inverse();
        // Pantalla y-abajo → NDC y-arriba; wgpu usa profundidad 0..1.
        // (transform_point3 es afín: aquí SÍ hay que dividir por w.)
        let unproject = |px: f32, py: f32, z: f32| {
            let p = inv
                * Vec4::new(
                    (px / lw.max(1.0)) * 2.0 - 1.0,
                    1.0 - (py / lh.max(1.0)) * 2.0,
                    z,
                    1.0,
                );
            (p.truncate() / p.w.max(1e-6)).into()
        };
        let near: Vec3 = unproject(x, y, 0.0);
        let far: Vec3 = unproject(x, y, 1.0);
        (near, (far - near).normalize_or_zero())
    }

    /// Zoom manual del diorama (pellizco táctil): `delta` en bloques sobre la
    /// distancia objetivo (+ = alejar). El confine de cueva sigue mandando.
    pub fn zoom_hd2d(&mut self, delta: f32) {
        self.zoom_bias = (self.zoom_bias + delta).clamp(-5.0, 8.0);
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

    /// First-person mouse turn: rotate hero facing + lens pitch.
    /// Finer sensitivity than FPS-mode: small nudges up/down/left/right.
    pub fn apply_fp_turn(&mut self, dx: f64, dy: f64, player: &mut Player) {
        player.facing += dx as f32 * FP_MOUSE_SENS;
        self.fp_pitch -= dy as f32 * FP_MOUSE_SENS_Y;
        self.fp_pitch = self.fp_pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT);
        self.pitch = self.fp_pitch;
    }

    pub fn apply_mouse_delta(&mut self, dx: f64, dy: f64) {
        // Free mouse orbit supersedes a waiting Q/E snap (target could drift).
        self.cancel_orbits();
        // First person: mouse turns the hero and the lens pitch.
        self.yaw += dx as f32 * MOUSE_SENSITIVITY;
        // Mouse up (negative dy on Windows) → look up.
        self.pitch -= dy as f32 * MOUSE_SENSITIVITY;
        self.pitch = self.pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT);
        self.fp_pitch = self.pitch;
        self.hd2d_orbit += dx as f32 * MOUSE_SENSITIVITY;
        while self.hd2d_orbit > std::f32::consts::TAU {
            self.hd2d_orbit -= std::f32::consts::TAU;
        }
        while self.hd2d_orbit < 0.0 {
            self.hd2d_orbit += std::f32::consts::TAU;
        }
    }

    pub fn sync_from_player(&mut self, eye: Vec3) {
        self.position = eye;
    }

    /// Flat XZ walk direction source for WASD: hero facing in first person
    /// (mouse-driven), locked camera yaw in HD-2D.
    pub fn effective_move_yaw(&self, facing: f32, hd2d_amount: f32) -> f32 {
        if hd2d_amount >= 0.999 {
            self.yaw
        } else if hd2d_amount <= 0.001 {
            facing
        } else {
            // Rotate toward hero facing as the diorama fades.
            let mut d = facing - self.yaw;
            let tau = std::f32::consts::TAU;
            while d > std::f32::consts::PI {
                d -= tau;
            }
            while d < -std::f32::consts::PI {
                d += tau;
            }
            self.yaw + d * (1.0 - hd2d_amount)
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_matches_original_factors_at_60_fps() {
        let mut camera = Camera::hd2d_follow(Vec3::ZERO);
        let focus = Vec3::new(4.0, 2.0, -3.0);
        camera.follow_hd2d(focus, 0.0, 0.0, 1.0 / 60.0);
        assert!(camera.hd2d_focus.distance(focus * HD2D_FOCUS_LERP) < 1e-5);
        let expected =
            HD2D_DISTANCE_BASE + (HD2D_DISTANCE_MAX - HD2D_DISTANCE_BASE) * HD2D_DISTANCE_LERP;
        assert!((camera.hd2d_distance - expected).abs() < 1e-5);
    }

    #[test]
    fn follow_converges_equally_across_frame_rates() {
        let focus = Vec3::new(4.0, 2.0, -3.0);
        for (pressure, confine, span) in [(0.0, 0.0, 0.0), (0.8, 0.7, 3.0)] {
            let mut expected = Camera::hd2d_follow(Vec3::ZERO);
            expected.follow_hd2d_framed(focus, pressure, confine, span, 0.0, 0.25);
            for fps in [20, 60, 120, 144, 240] {
                let mut camera = Camera::hd2d_follow(Vec3::ZERO);
                for _ in 0..fps / 4 {
                    camera.follow_hd2d_framed(focus, pressure, confine, span, 0.0, 1.0 / fps as f32);
                }
                assert!(
                    camera.position.distance(expected.position) < 1e-4,
                    "fps={fps}"
                );
                assert!(camera.hd2d_focus.distance(expected.hd2d_focus) < 1e-4);
                assert!((camera.hd2d_distance - expected.hd2d_distance).abs() < 1e-4);
            }
            let mut camera = Camera::hd2d_follow(Vec3::ZERO);
            for dt in [0.01, 0.04, 0.02, 0.08, 0.10] {
                camera.follow_hd2d_framed(focus, pressure, confine, span, 0.0, dt);
            }
            assert!(camera.position.distance(expected.position) < 1e-4);
        }
    }

    #[test]
    fn screen_ray_center_matches_forward() {
        let mut camera = Camera::hd2d_follow(Vec3::ZERO);
        camera.aspect = 16.0 / 9.0;
        let (lw, lh) = (1280.0, 720.0);
        let (origin, dir) = camera.screen_ray(lw * 0.5, lh * 0.5, lw, lh);
        assert!((origin - camera.position).length() < 0.5);
        assert!((dir - camera.forward()).length() < 1e-3);
        // Esquina: el rayo diverge del centro pero sigue normalizado.
        let (_, corner) = camera.screen_ray(0.0, 0.0, lw, lh);
        assert!((corner.length() - 1.0).abs() < 1e-4);
        assert!((corner - camera.forward()).length() > 0.05);
    }

    #[test]
    fn zoom_bias_clamps_and_defaults_to_zero() {
        let mut camera = Camera::hd2d_follow(Vec3::ZERO);
        camera.zoom_hd2d(100.0);
        camera.zoom_hd2d(-100.0);
        // Último: -100 recortado a -5.
        camera.zoom_hd2d(5.0);
        // -5 + 5 = 0 → distancia idéntica a la de fábrica.
        let mut plain = Camera::hd2d_follow(Vec3::ZERO);
        camera.follow_hd2d_framed(Vec3::ZERO, 0.0, 0.0, 0.0, 0.0, 1.0);
        plain.follow_hd2d_framed(Vec3::ZERO, 0.0, 0.0, 0.0, 0.0, 1.0);
        assert!((camera.hd2d_distance - plain.hd2d_distance).abs() < 1e-4);
    }

    #[test]
    fn follow_without_elapsed_time_does_not_move() {
        let mut camera = Camera::hd2d_follow(Vec3::ZERO);
        let position = camera.position;
        for dt in [0.0, -0.01] {
            camera.follow_hd2d_framed(Vec3::ONE, 1.0, 1.0, 4.0, 0.0, dt);
            assert_eq!(camera.position, position);
            assert_eq!(camera.hd2d_focus, Vec3::ZERO);
            assert_eq!(camera.hd2d_distance, HD2D_DISTANCE_BASE);
        }
    }

    #[test]
    fn orbit_presses_queue_while_waiting() {
        use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
        // Diorama defers the snap (hd2d_amount = 1 ≥ ORBIT_WAIT_HD2D_MIN).
        let mut camera = Camera::hd2d_follow(Vec3::ZERO);
        assert!(camera.request_orbit(1, Vec3::ZERO));
        assert!(camera.request_orbit(1, Vec3::ZERO));
        let snap = camera.pending_orbit.expect("snap waits on preload");
        assert!((snap.to - FRAC_PI_2).abs() < 1e-5, "Q,Q = 90°, got {}", snap.to);
        // Opposite press subtracts back to one step.
        assert!(camera.request_orbit(-1, Vec3::ZERO));
        let snap = camera.pending_orbit.expect("still waiting");
        assert!((snap.to - FRAC_PI_4).abs() < 1e-5, "got {}", snap.to);
    }

    #[test]
    fn fps_pull_draws_lens_toward_hero_and_releases() {
        let focus = Vec3::ZERO;
        let mut camera = Camera::hd2d_follow(focus);
        // Sustained hitch → distance settles near HD2D_DISTANCE_HITCH (12).
        for _ in 0..240 {
            camera.follow_hd2d_framed(focus, 0.0, 0.0, 0.0, 1.0, 1.0 / 60.0);
        }
        assert!((camera.hd2d_distance - HD2D_DISTANCE_HITCH).abs() < 0.05);
        // Pressure gone → eases back to the sparse pull-back (MAX).
        for _ in 0..240 {
            camera.follow_hd2d_framed(focus, 0.0, 0.0, 0.0, 0.0, 1.0 / 60.0);
        }
        assert!((camera.hd2d_distance - HD2D_DISTANCE_MAX).abs() < 0.3);
    }

    #[test]
    fn fps_pull_never_pushes_out_and_confine_wins() {
        let focus = Vec3::ZERO;
        // Crowded scene already closer than HITCH: fps pull must not push out.
        let mut camera = Camera::hd2d_follow(focus);
        for _ in 0..240 {
            camera.follow_hd2d_framed(focus, 1.0, 0.0, 0.0, 1.0, 1.0 / 60.0);
        }
        assert!((camera.hd2d_distance - HD2D_DISTANCE_MIN).abs() < 0.05);
        // Buried: confine (7.0) wins over hitch pull.
        let mut camera = Camera::hd2d_follow(focus);
        for _ in 0..240 {
            camera.follow_hd2d_framed(focus, 0.0, 1.0, 0.0, 1.0, 1.0 / 60.0);
        }
        assert!((camera.hd2d_distance - HD2D_DISTANCE_CONFINED).abs() < 0.05);
    }

    #[test]
    fn snap_orbit_puts_facing_up_right() {
        use std::f32::consts::{PI, TAU};
        // Arriba-Derecha en mundo = yaw + 45°; debe coincidir con facing
        // ± medio paso (22.5°) y la órbita queda en la rejilla de 45°.
        // En reversa la referencia es el opuesto: facing cae en Abajo-Izquierda
        // (yaw + 225°) con la misma tolerancia.
        for deg in [0, 30, 90, 135, 180, 250, 315] {
            let facing = (deg as f32).to_radians();
            for reverse in [false, true] {
                let mut camera = Camera::hd2d_follow(Vec3::ZERO);
                camera.snap_orbit_to_face(facing, reverse);
                let steps = camera.hd2d_orbit / HD2D_ORBIT_STEP;
                assert!(
                    (steps - steps.round()).abs() < 1e-4,
                    "deg={deg} rev={reverse} orbit off-grid: {}",
                    camera.hd2d_orbit
                );
                let yaw = HD2D_YAW + camera.hd2d_orbit;
                let want = if reverse { yaw + FRAC_PI_4 + PI } else { yaw + FRAC_PI_4 };
                let mut d = want - facing;
                while d > PI {
                    d -= TAU;
                }
                while d < -PI {
                    d += TAU;
                }
                assert!(
                    d.abs() <= FRAC_PI_4 / 2.0 + 1e-4,
                    "deg={deg} rev={reverse} off by {d}"
                );
            }
        }
    }

    #[test]
    fn orbit_press_extends_flight_anim() {        use std::f32::consts::FRAC_PI_2;
        let mut camera = Camera::hd2d_follow(Vec3::ZERO);
        assert!(camera.request_orbit(1, Vec3::ZERO));
        camera.begin_orbit(Vec3::ZERO);
        assert!(camera.orbit_anim.is_some());
        assert!(camera.request_orbit(1, Vec3::ZERO));
        let a = camera.orbit_anim.expect("anim in flight");
        // 45° pending + 45° chained = 90° target, clock untouched.
        assert!((a.to - FRAC_PI_2).abs() < 1e-5, "got {}", a.to);
        assert_eq!(a.t, 0.0);
        // Anim still converges on the extended target.
        camera.update_orbit_anim(10.0);
        assert!((camera.hd2d_orbit - FRAC_PI_2).abs() < 1e-5);
    }
}
