//! Controles táctiles (Android) — SEPARADOS del menú de PC.
//!
//! Regla: este módulo nunca toca `stair_hud.hits` ni el menú de PC. El HUD
//! táctil se dibuja con `push_solid_quad` (sin hit regions) y el input
//! táctil usa sus propios rects + `TouchControls`. El movimiento reutiliza
//! `HeldKeys` (mismo `player.tick`), y la mirada reutiliza `apply_fp_turn`
//! / `request_orbit`, pero el menú de PC no ve ni un dedo.

use crate::camera::HeldKeys;
use crate::hud::{hud_scale, push_solid_quad, HudMesh, HudRect};
use winit::event::TouchPhase;

/// Botones táctiles v1 (esquina derecha). El usuario ajustará layout luego.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchButton {
    /// Mantener = como Space (salto).
    Jump,
    /// Mantener = como LMB (golpe + hold-to-mine).
    Mine,
    /// Toque = como V (alterna 1ª persona).
    Cam,
    /// Toque = como G (abre/cierra inventario).
    Inv,
}

/// Acciones de borde (un disparo por toque) que `App` debe aplicar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TouchAction {
    ToggleCam,
    ToggleInv,
    /// Toque rápido en el mundo (coords lógicas): menú PC primero, si no
    /// hay nada, picado apuntado por `screen_ray`.
    Tap(f32, f32),
    /// El dedo de picado-por-sostener se levantó: detener hold-to-mine.
    TapMineEnd,
}

/// Toque rápido: menos de esto en tiempo y movimiento.
const TAP_MAX_MS: u128 = 350;
const TAP_MAX_PX: f32 = 20.0;
/// Pellizco → zoom del diorama (bloques por píxel de separación).
const PINCH_SENS: f32 = 0.02;

/// Geometría táctil en píxeles lógicos (origen arriba-izquierda).
/// Misma instancia para hit-test y dibujo: una sola fuente de verdad.
#[derive(Clone, Copy, Debug)]
pub struct TouchLayout {
    /// Base del joystick cuando está inactivo (pista visual).
    pub joy_home: (f32, f32),
    pub joy_radius: f32,
    pub btn_mine: HudRect,
    pub btn_jump: HudRect,
    pub btn_cam: HudRect,
    pub btn_inv: HudRect,
}

pub fn layout(lw: f32, lh: f32) -> TouchLayout {
    // El píxel lógico ya es independiente de la densidad (≈1/160 pulgada),
    // así que los tamaños fijos SON tamaños físicos en todas las pantallas:
    // botón grande 80 ≈ 12.7 mm, pequeño 60 ≈ 9.5 mm (mínimo usable ~9 mm).
    // En pantallas bajas (teléfono apaisado ~360-430 lpx) se compacta ×0.85.
    // En Android además ×hud_scale (0.8): los botones tapan menos mundo.
    // En PC hud_scale = 1.0 (idéntico).
    let compact = lh < 520.0;
    let k = (if compact { 0.85 } else { 1.0 }) * hud_scale();
    // Safe-area: márgenes laterales simétricos (el notch queda en un
    // lateral apaisado, pero no sabemos en cuál sin JNI).
    let side = (lw * 0.035).max(22.0);
    let top = 16.0;
    let bottom = 20.0;
    let big = 80.0 * k;
    let small = 60.0 * k;
    let gap = 14.0 * k;
    TouchLayout {
        joy_home: (side + 95.0 * k, lh - bottom - 95.0 * k),
        joy_radius: 88.0 * k,
        btn_mine: HudRect {
            x: lw - side - big,
            y: lh - bottom - big,
            w: big,
            h: big,
        },
        btn_jump: HudRect {
            x: lw - side - big,
            y: lh - bottom - big - gap - big,
            w: big,
            h: big,
        },
        btn_cam: HudRect {
            x: lw - side - small,
            y: top,
            w: small,
            h: small,
        },
        btn_inv: HudRect {
            x: lw - side - small - 12.0 - small,
            y: top,
            w: small,
            h: small,
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FingerRole {
    Joy,
    Look,
    Pinch,
    Button(TouchButton),
}

/// Candidato a toque: dedo quieto en zona de mirada.
#[derive(Clone, Copy, Debug)]
struct TapCandidate {
    id: u64,
    x: f32,
    y: f32,
    t0: std::time::Instant,
}

pub struct TouchControls {
    /// En Android nace activo; en PC se enciende con el primer toque.
    /// Con `false` no se dibuja nada ni se escribe ninguna tecla.
    pub enabled: bool,
    joy_id: Option<u64>,
    joy_origin: (f32, f32),
    joy_vec: (f32, f32),
    look_id: Option<u64>,
    look_last: (f32, f32),
    /// Deltas de mirada acumulados (los drena `App` cada frame).
    look_delta: (f32, f32),
    /// Arrastre horizontal en diorama → pasos de órbita Q/E.
    pub orbit_carry: f32,
    jump_id: Option<u64>,
    mine_id: Option<u64>,
    /// Roles vivos por dedo (id → rol), para soltar el correcto.
    roles: Vec<(u64, FingerRole)>,
    /// La última vez que este módulo escribió teclas (para limpiarlas).
    owned_keys: bool,
    /// Dedo quieto en zona de mirada: toque rápido o picado-por-sostener.
    tap_cand: Option<TapCandidate>,
    /// Dedo que sostiene picado apuntado (maduró de candidato).
    tap_mining_id: Option<u64>,
    /// Segundo dedo de mirada → pellizco (zoom).
    pinch_id: Option<u64>,
    pinch_pos: (f32, f32),
    /// Zoom acumulado del pellizco (lo drena `App` cada frame).
    zoom_delta: f32,
}

impl TouchControls {
    pub fn new() -> Self {
        Self {
            enabled: cfg!(target_os = "android"),
            joy_id: None,
            joy_origin: (0.0, 0.0),
            joy_vec: (0.0, 0.0),
            look_id: None,
            look_last: (0.0, 0.0),
            look_delta: (0.0, 0.0),
            orbit_carry: 0.0,
            jump_id: None,
            mine_id: None,
            roles: Vec::new(),
            owned_keys: false,
            tap_cand: None,
            tap_mining_id: None,
            pinch_id: None,
            pinch_pos: (0.0, 0.0),
            zoom_delta: 0.0,
        }
    }

    pub fn jump_held(&self) -> bool {
        self.jump_id.is_some()
    }

    pub fn mine_held(&self) -> bool {
        self.mine_id.is_some()
    }

    /// Drena la mirada acumulada desde el último frame.
    pub fn take_look_delta(&mut self) -> (f32, f32) {
        let d = self.look_delta;
        self.look_delta = (0.0, 0.0);
        d
    }

    /// Drena el zoom acumulado del pellizco desde el último frame.
    pub fn take_zoom_delta(&mut self) -> f32 {
        let z = self.zoom_delta;
        self.zoom_delta = 0.0;
        z
    }

    /// Si el candidato a toque lleva quieto lo suficiente, madura a
    /// picado-por-sostener. `App` lo llama cada frame; devuelve el dedo
    /// y su posición una sola vez.
    pub fn poll_hold_mine(&mut self) -> Option<(u64, f32, f32)> {
        let cand = self.tap_cand?;
        if self.tap_mining_id.is_some() {
            return None;
        }
        if cand.t0.elapsed().as_millis() >= TAP_MAX_MS {
            self.tap_cand = None;
            self.tap_mining_id = Some(cand.id);
            // Ya no es mirada: congela su delta para no girar al picar.
            self.look_delta = (0.0, 0.0);
            return Some((cand.id, cand.x, cand.y));
        }
        None
    }

    fn pinch_gap(&self) -> f32 {
        let dx = self.pinch_pos.0 - self.look_last.0;
        let dy = self.pinch_pos.1 - self.look_last.1;
        (dx * dx + dy * dy).sqrt().max(1.0)
    }

    fn set_role(&mut self, id: u64, role: FingerRole) {
        if !self.roles.iter().any(|&(i, _)| i == id) {
            self.roles.push((id, role));
        }
    }

    fn take_role(&mut self, id: u64) -> Option<FingerRole> {
        let pos = self.roles.iter().position(|&(i, _)| i == id)?;
        Some(self.roles.remove(pos).1)
    }

    /// Entrada de un evento táctil (coords ya en píxeles lógicos).
    /// Devuelve acciones de borde (toques que alternan algo).
    pub fn handle(
        &mut self,
        phase: TouchPhase,
        id: u64,
        x: f32,
        y: f32,
        lw: f32,
        lh: f32,
    ) -> Vec<TouchAction> {
        let mut out = Vec::new();
        let lay = layout(lw, lh);
        match phase {
            TouchPhase::Started => {
                if lay.btn_cam.contains(x, y) {
                    self.set_role(id, FingerRole::Button(TouchButton::Cam));
                    out.push(TouchAction::ToggleCam);
                } else if lay.btn_inv.contains(x, y) {
                    self.set_role(id, FingerRole::Button(TouchButton::Inv));
                    out.push(TouchAction::ToggleInv);
                } else if lay.btn_jump.contains(x, y) {
                    if self.jump_id.is_none() {
                        self.jump_id = Some(id);
                        self.set_role(id, FingerRole::Button(TouchButton::Jump));
                    }
                } else if lay.btn_mine.contains(x, y) {
                    if self.mine_id.is_none() {
                        self.mine_id = Some(id);
                        self.set_role(id, FingerRole::Button(TouchButton::Mine));
                    }
                } else if x < lw * 0.45 && y > lh * 0.40 {
                    // Zona joystick: origen flotante donde cae el dedo.
                    if self.joy_id.is_none() {
                        self.joy_id = Some(id);
                        self.joy_origin = (x, y);
                        self.joy_vec = (0.0, 0.0);
                        self.set_role(id, FingerRole::Joy);
                    }
                } else if self.look_id.is_none() {
                    self.look_id = Some(id);
                    self.look_last = (x, y);
                    self.set_role(id, FingerRole::Look);
                    // Candidato a toque: si se queda quieto será tap o
                    // picado-por-sostener; si se mueve será mirada.
                    self.tap_cand = Some(TapCandidate {
                        id,
                        x,
                        y,
                        t0: std::time::Instant::now(),
                    });
                } else if self.pinch_id.is_none() && Some(id) != self.tap_mining_id {
                    // Segundo dedo en zona libre → pellizco (zoom).
                    self.pinch_id = Some(id);
                    self.pinch_pos = (x, y);
                    self.set_role(id, FingerRole::Pinch);
                    // El pellizco absorbe al candidato (no era un tap) y
                    // congela la mirada: los dos dedos mandan zoom.
                    self.tap_cand = None;
                    self.look_delta = (0.0, 0.0);
                }
            }
            TouchPhase::Moved => {
                if Some(id) == self.joy_id {
                    let r = lay.joy_radius;
                    let mut dx = (x - self.joy_origin.0) / r;
                    let mut dy = (y - self.joy_origin.1) / r;
                    let m = (dx * dx + dy * dy).sqrt();
                    if m > 1.0 {
                        dx /= m;
                        dy /= m;
                    }
                    self.joy_vec = (dx, dy);
                } else if Some(id) == self.look_id {
                    if self.pinch_id.is_some() {
                        // Pellizco activo: el primer dedo solo reubica,
                        // no gira (el zoom manda).
                        let before = self.pinch_gap();
                        self.look_last = (x, y);
                        let after = self.pinch_gap();
                        self.zoom_delta += (after - before) * PINCH_SENS;
                    } else {
                        let dxm = x - self.look_last.0;
                        let dym = y - self.look_last.1;
                        self.look_delta.0 += dxm;
                        self.look_delta.1 += dym;
                        self.look_last = (x, y);
                        // Se movió: ya no es candidato a toque.
                        if let Some(cand) = self.tap_cand {
                            if cand.id == id
                                && ((x - cand.x).abs() > TAP_MAX_PX
                                    || (y - cand.y).abs() > TAP_MAX_PX)
                            {
                                self.tap_cand = None;
                            }
                        }
                    }
                } else if Some(id) == self.pinch_id {
                    // Pellizco: la separación manda zoom (abrir = acercar).
                    let before = self.pinch_gap();
                    self.pinch_pos = (x, y);
                    let after = self.pinch_gap();
                    self.zoom_delta += (after - before) * PINCH_SENS;
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                match self.take_role(id) {
                    Some(FingerRole::Joy) => {
                        self.joy_id = None;
                        self.joy_vec = (0.0, 0.0);
                    }
                    Some(FingerRole::Look) => {
                        // ¿Era picado-por-sostener? Terminar hold-to-mine.
                        if self.tap_mining_id == Some(id) {
                            self.tap_mining_id = None;
                            self.look_id = None;
                            self.tap_cand = None;
                            out.push(TouchAction::TapMineEnd);
                        } else {
                            self.look_id = None;
                            // ¿Fue un toque rápido y quieto? Tap.
                            if let Some(cand) = self.tap_cand {
                                if cand.id == id
                                    && cand.t0.elapsed().as_millis() < TAP_MAX_MS
                                {
                                    self.look_delta = (0.0, 0.0);
                                    out.push(TouchAction::Tap(cand.x, cand.y));
                                }
                            }
                            self.tap_cand = None;
                        }
                    }
                    Some(FingerRole::Pinch) => {
                        self.pinch_id = None;
                    }
                    Some(FingerRole::Button(TouchButton::Jump)) => {
                        if self.jump_id == Some(id) {
                            self.jump_id = None;
                        }
                    }
                    Some(FingerRole::Button(TouchButton::Mine)) => {
                        if self.mine_id == Some(id) {
                            self.mine_id = None;
                        }
                    }
                    Some(FingerRole::Button(_)) => {}
                    None => {
                        // Dedo sin rol (p.ej. segundo dedo en mismo botón):
                        // limpiar por id por si acaso.
                        if self.jump_id == Some(id) {
                            self.jump_id = None;
                        }
                        if self.mine_id == Some(id) {
                            self.mine_id = None;
                        }
                        if self.joy_id == Some(id) {
                            self.joy_id = None;
                            self.joy_vec = (0.0, 0.0);
                        }
                        if self.look_id == Some(id) {
                            self.look_id = None;
                        }
                        if self.pinch_id == Some(id) {
                            self.pinch_id = None;
                        }
                        if self.tap_mining_id == Some(id) {
                            self.tap_mining_id = None;
                            out.push(TouchAction::TapMineEnd);
                        }
                    }
                }
            }
        }
        out
    }

    /// Joystick + salto → `HeldKeys`. Solo escribe mientras hay dedos
    /// activos; al soltar limpia lo que escribió (el teclado de PC queda
    /// intacto cuando el táctil está inactivo).
    pub fn apply_to_keys(&mut self, keys: &mut HeldKeys) {
        const DEAD: f32 = 0.25;
        if self.joy_id.is_some() || self.jump_id.is_some() {
            let (dx, dy) = self.joy_vec;
            keys.forward = dy < -DEAD;
            keys.back = dy > DEAD;
            keys.left = dx < -DEAD;
            keys.right = dx > DEAD;
            keys.sprint = dx * dx + dy * dy > 0.92 * 0.92;
            keys.up = self.jump_id.is_some();
            self.owned_keys = true;
        } else if self.owned_keys {
            keys.forward = false;
            keys.back = false;
            keys.left = false;
            keys.right = false;
            keys.sprint = false;
            keys.up = false;
            self.owned_keys = false;
        }
    }

    /// HUD táctil (sin hit regions — ver regla del módulo).
    pub fn build_touch_hud(&self, lw: f32, lh: f32) -> HudMesh {
        let mut mesh = HudMesh::default();
        if !self.enabled {
            return mesh;
        }
        let lay = layout(lw, lh);
        // Base del joystick: donde está el dedo, o la pista home en reposo.
        let (cx, cy) = if self.joy_id.is_some() {
            self.joy_origin
        } else {
            lay.joy_home
        };
        let r = lay.joy_radius;
        push_solid_quad(
            &mut mesh,
            HudRect {
                x: cx - r,
                y: cy - r,
                w: r * 2.0,
                h: r * 2.0,
            },
            lw,
            lh,
            [1.0, 1.0, 1.0, 0.18],
        );
        // Nub: sigue al dedo dentro del radio.
        let kx = cx + self.joy_vec.0 * r * 0.55;
        let ky = cy + self.joy_vec.1 * r * 0.55;
        let kr = r * 0.34;
        push_solid_quad(
            &mut mesh,
            HudRect {
                x: kx - kr,
                y: ky - kr,
                w: kr * 2.0,
                h: kr * 2.0,
            },
            lw,
            lh,
            [1.0, 1.0, 1.0, 0.45],
        );
        let mine_a = if self.mine_id.is_some() { 0.85 } else { 0.55 };
        let jump_a = if self.jump_id.is_some() { 0.85 } else { 0.55 };
        push_solid_quad(&mut mesh, lay.btn_mine, lw, lh, [0.85, 0.25, 0.25, mine_a]);
        push_solid_quad(&mut mesh, lay.btn_jump, lw, lh, [0.30, 0.80, 0.35, jump_a]);
        push_solid_quad(&mut mesh, lay.btn_cam, lw, lh, [0.30, 0.55, 0.90, 0.55]);
        push_solid_quad(&mut mesh, lay.btn_inv, lw, lh, [0.90, 0.70, 0.20, 0.55]);
        mesh
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> HeldKeys {
        HeldKeys::default()
    }

    #[test]
    fn joystick_up_is_forward() {
        let mut t = TouchControls::new();
        t.enabled = true;
        let (lw, lh) = (1280.0, 720.0);
        t.handle(TouchPhase::Started, 1, 100.0, 600.0, lw, lh);
        t.handle(TouchPhase::Moved, 1, 100.0, 600.0 - 95.0, lw, lh);
        let mut k = keys();
        t.apply_to_keys(&mut k);
        assert!(k.forward && !k.back && !k.left && !k.right);
    }

    #[test]
    fn joystick_diagonal_sets_both_axes_and_sprint_at_edge() {
        let mut t = TouchControls::new();
        let (lw, lh) = (1280.0, 720.0);
        t.handle(TouchPhase::Started, 1, 100.0, 600.0, lw, lh);
        // Empujón largo a la derecha-arriba (se recorta al radio = sprint).
        t.handle(TouchPhase::Moved, 1, 100.0 + 200.0, 600.0 - 200.0, lw, lh);
        let mut k = keys();
        t.apply_to_keys(&mut k);
        assert!(k.forward && k.right);
        assert!(k.sprint);
    }

    #[test]
    fn joystick_deadzone_writes_nothing_but_owns() {
        let mut t = TouchControls::new();
        let (lw, lh) = (1280.0, 720.0);
        t.handle(TouchPhase::Started, 1, 100.0, 600.0, lw, lh);
        t.handle(TouchPhase::Moved, 1, 105.0, 602.0, lw, lh);
        let mut k = keys();
        t.apply_to_keys(&mut k);
        assert!(!k.forward && !k.back && !k.left && !k.right && !k.sprint);
        // Al soltar limpia y libera.
        t.handle(TouchPhase::Ended, 1, 105.0, 602.0, lw, lh);
        k.forward = true; // simula resto previo: debe limpiarlo
        t.apply_to_keys(&mut k);
        assert!(!k.forward);
    }

    #[test]
    fn buttons_hold_and_tap() {
        let mut t = TouchControls::new();
        let (lw, lh) = (1280.0, 720.0);
        let lay = layout(lw, lh);
        // MINE mantiene.
        let mx = lay.btn_mine.x + 10.0;
        let my = lay.btn_mine.y + 10.0;
        assert!(t.handle(TouchPhase::Started, 7, mx, my, lw, lh).is_empty());
        assert!(t.mine_held());
        t.handle(TouchPhase::Ended, 7, mx, my, lw, lh);
        assert!(!t.mine_held());
        // CAM dispara un borde.
        let cx = lay.btn_cam.x + 10.0;
        let cy = lay.btn_cam.y + 10.0;
        let acts = t.handle(TouchPhase::Started, 8, cx, cy, lw, lh);
        assert_eq!(acts, vec![TouchAction::ToggleCam]);
    }

    #[test]
    fn look_drag_accumulates_and_drains() {
        let mut t = TouchControls::new();
        let (lw, lh) = (1280.0, 720.0);
        // Centro de pantalla: ni joystick ni botones → mirada.
        t.handle(TouchPhase::Started, 3, 640.0, 300.0, lw, lh);
        t.handle(TouchPhase::Moved, 3, 660.0, 290.0, lw, lh);
        t.handle(TouchPhase::Moved, 3, 670.0, 285.0, lw, lh);
        assert_eq!(t.take_look_delta(), (30.0, -15.0));
        assert_eq!(t.take_look_delta(), (0.0, 0.0));
    }

    #[test]
    fn quick_tap_emits_tap_without_look() {
        let mut t = TouchControls::new();
        let (lw, lh) = (1280.0, 720.0);
        assert!(t.handle(TouchPhase::Started, 3, 640.0, 300.0, lw, lh).is_empty());
        let acts = t.handle(TouchPhase::Ended, 3, 641.0, 301.0, lw, lh);
        assert_eq!(acts.len(), 1);
        assert!(matches!(acts[0], TouchAction::Tap(x, y) if (x - 640.0).abs() < 1.0 && (y - 300.0).abs() < 1.0));
        // El tap no giró la cámara.
        assert_eq!(t.take_look_delta(), (0.0, 0.0));
    }

    #[test]
    fn held_finger_matures_to_hold_mine() {
        let mut t = TouchControls::new();
        let (lw, lh) = (1280.0, 720.0);
        t.handle(TouchPhase::Started, 3, 640.0, 300.0, lw, lh);
        assert!(t.poll_hold_mine().is_none());
        std::thread::sleep(std::time::Duration::from_millis(TAP_MAX_MS as u64 + 60));
        let matured = t.poll_hold_mine();
        assert!(matured.is_some());
        assert!(t.poll_hold_mine().is_none(), "madura una sola vez");
        // Al levantar, termina el picado.
        let acts = t.handle(TouchPhase::Ended, 3, 640.0, 300.0, lw, lh);
        assert!(acts.contains(&TouchAction::TapMineEnd));
    }

    #[test]
    fn pinch_accumulates_zoom_not_look() {
        let mut t = TouchControls::new();
        let (lw, lh) = (1280.0, 720.0);
        t.handle(TouchPhase::Started, 3, 640.0, 300.0, lw, lh);
        // Segundo dedo → pellizco, no segunda mirada.
        assert!(t.handle(TouchPhase::Started, 4, 740.0, 300.0, lw, lh).is_empty());
        t.handle(TouchPhase::Moved, 4, 790.0, 300.0, lw, lh);
        let z = t.take_zoom_delta();
        assert!(z > 0.5, "abrir dedos = acercar, got {z}");
        assert_eq!(t.take_look_delta(), (0.0, 0.0));
        // Soltar el pellizco no emite tap.
        let acts = t.handle(TouchPhase::Ended, 4, 790.0, 300.0, lw, lh);
        assert!(!acts.iter().any(|a| matches!(a, TouchAction::Tap(..))));
    }

    #[test]
    fn touch_hud_has_no_hit_regions() {        // Regla dura: el menú de PC nunca ve los botones táctiles.
        let mut t = TouchControls::new();
        t.enabled = true;
        let mesh = t.build_touch_hud(1280.0, 720.0);
        assert!(!mesh.vertices.is_empty());
        assert!(mesh.hits.is_empty());
        let mut off = TouchControls::new();
        off.enabled = false;
        assert!(off.build_touch_hud(1280.0, 720.0).vertices.is_empty());
    }
}
