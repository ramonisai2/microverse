//! Castle Story–style automatic stair / tunnel excavation tool.
use glam::IVec3;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Max stair length (steps along the facing axis).
/// Not a vertical dig-depth cap — that “10 down” limit was a misread instruction.
pub const STAIR_MAX_STEPS: i32 = 24;
/// Min steps so a trivial click still digs a short shaft ahead.
pub const STAIR_MIN_STEPS: i32 = 2;
/// Default delay between dirt cells (stone uses the equipped pick's slower rate).
pub const STAIR_DIG_INTERVAL: Duration = Duration::from_millis(160);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StairPhase {
    /// Tool off (normal break/place).
    Idle,
    /// Armed: waiting for first click (anchor).
    Armed,
    /// Anchor set; preview updates toward cursor target.
    Preview,
    /// Confirmed; removing voxels one-by-one.
    Digging,
}

/// World-space cardinal facing for the tunnel / stair (X or Z only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunnelFacing {
    /// −Z
    North,
    /// +Z
    South,
    /// +X
    East,
    /// −X
    West,
}

impl TunnelFacing {
    pub fn dir(self) -> IVec3 {
        match self {
            Self::North => IVec3::new(0, 0, -1),
            Self::South => IVec3::new(0, 0, 1),
            Self::East => IVec3::new(1, 0, 0),
            Self::West => IVec3::new(-1, 0, 0),
        }
    }

    pub fn opposite(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::South => Self::North,
            Self::East => Self::West,
            Self::West => Self::East,
        }
    }

    pub fn rot_cw(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
        }
    }

    pub fn rot_ccw(self) -> Self {
        self.rot_cw().opposite()
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::North => "N",
            Self::South => "S",
            Self::East => "E",
            Self::West => "W",
        }
    }

    /// Spanish cardinal (Oeste = O).
    pub fn label_es(self) -> &'static str {
        match self {
            Self::North => "N",
            Self::South => "S",
            Self::East => "E",
            Self::West => "O",
        }
    }

    /// Snap horizontal yaw (atan2(z, x), 0 = +X) to the nearest cardinal.
    pub fn from_yaw(yaw: f32) -> Self {
        let fx = yaw.cos();
        let fz = yaw.sin();
        if fx.abs() >= fz.abs() {
            if fx >= 0.0 {
                Self::East
            } else {
                Self::West
            }
        } else if fz >= 0.0 {
            Self::South
        } else {
            Self::North
        }
    }

    /// World yaw for this cardinal (radians, atan2 style).
    pub fn yaw(self) -> f32 {
        match self {
            Self::East => 0.0,
            Self::North => -std::f32::consts::FRAC_PI_2,
            Self::West => std::f32::consts::PI,
            Self::South => std::f32::consts::FRAC_PI_2,
        }
    }
}

/// On-screen D-pad slot (UI y-down).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadDir {
    /// Top of pad — camera back.
    Up,
    /// Bottom of pad — camera forward (into the scene).
    Down,
    Left,
    Right,
}

impl PadDir {
    /// Map a pad button to a world cardinal using view/move yaw.
    /// Screen-up = dig forward (into the scene); screen-down = back.
    pub fn to_facing(self, view_yaw: f32) -> TunnelFacing {
        let forward = TunnelFacing::from_yaw(view_yaw);
        match self {
            Self::Up => forward,
            Self::Down => forward.opposite(),
            Self::Right => forward.rot_cw(),
            Self::Left => forward.rot_ccw(),
        }
    }
}

/// Vertical profile of the dig.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunnelIncline {
    /// Stair descending along facing (1 block down per step).
    Down,
    /// Stair ascending along facing (1 block up per step).
    Up,
    /// Level corridor along facing (headroom tall).
    Flat,
}

impl TunnelIncline {
    pub fn label(self) -> &'static str {
        match self {
            Self::Down => "↓",
            Self::Up => "↑",
            Self::Flat => "→",
        }
    }
}

#[derive(Clone, Debug)]
pub struct StairTool {
    pub phase: StairPhase,
    pub facing: TunnelFacing,
    pub incline: TunnelIncline,
    /// Planned length along facing (clamped).
    pub steps: i32,
    /// Corridor width in blocks (`STAIR_WIDTH_MIN`..=`STAIR_WIDTH_MAX`).
    pub width: i32,
    pub anchor: Option<IVec3>,
    /// Cells shown as yellow ghost / queued for dig (unique, ordered).
    pub cells: Vec<IVec3>,
    dig_queue: VecDeque<IVec3>,
    next_dig_at: Instant,
}

impl Default for StairTool {
    fn default() -> Self {
        Self {
            phase: StairPhase::Idle,
            facing: TunnelFacing::East,
            // Flat corridor by default — Down must be chosen explicitly (avoids accidental pits).
            incline: TunnelIncline::Flat,
            steps: STAIR_MIN_STEPS,
            width: STAIR_WIDTH_MIN,
            anchor: None,
            cells: Vec::new(),
            dig_queue: VecDeque::new(),
            next_dig_at: Instant::now(),
        }
    }
}

impl StairTool {
    pub fn is_active(&self) -> bool {
        self.phase != StairPhase::Idle
    }

    pub fn is_digging(&self) -> bool {
        self.phase == StairPhase::Digging
    }

    /// Compact HUD tag, e.g. `E↓×8·1`.
    pub fn hud_dir(&self) -> String {
        format!(
            "{}{}×{}·{}",
            self.facing.label(),
            self.incline.label(),
            self.steps,
            self.width
        )
    }

    pub fn toggle(&mut self) {
        if self.phase == StairPhase::Idle {
            self.phase = StairPhase::Armed;
            self.anchor = None;
            self.cells.clear();
            self.dig_queue.clear();
            self.steps = STAIR_MIN_STEPS;
            self.width = STAIR_WIDTH_MIN;
        } else {
            self.cancel_to_idle();
        }
    }

    /// Arm the tool and lock dig heading to the character’s current yaw.
    pub fn arm_from_yaw(&mut self, yaw: f32) {
        if self.phase == StairPhase::Idle {
            self.facing = TunnelFacing::from_yaw(yaw);
            self.phase = StairPhase::Armed;
            self.anchor = None;
            self.cells.clear();
            self.dig_queue.clear();
            self.steps = STAIR_MIN_STEPS;
            self.width = STAIR_WIDTH_MIN;
        } else {
            self.cancel_to_idle();
        }
    }

    pub fn cancel_preview(&mut self) {
        match self.phase {
            StairPhase::Preview | StairPhase::Armed => {
                self.phase = StairPhase::Armed;
                self.anchor = None;
                self.cells.clear();
            }
            StairPhase::Digging => {}
            StairPhase::Idle => {}
        }
    }

    pub fn cancel_to_idle(&mut self) {
        let facing = self.facing;
        let incline = self.incline;
        let width = self.width;
        *self = Self::default();
        self.facing = facing;
        self.incline = incline;
        self.width = width.clamp(STAIR_WIDTH_MIN, STAIR_WIDTH_MAX);
    }

    pub fn set_facing(&mut self, facing: TunnelFacing) {
        if self.phase == StairPhase::Idle || self.phase == StairPhase::Digging {
            return;
        }
        self.facing = facing;
        self.rebuild_cells();
    }

    /// Optional helper: snap dig heading to a yaw (pad / arm use explicit facing instead).
    #[allow(dead_code)]
    pub fn sync_facing_from_yaw(&mut self, yaw: f32) {
        if self.phase == StairPhase::Idle || self.phase == StairPhase::Digging {
            return;
        }
        let next = TunnelFacing::from_yaw(yaw);
        if next != self.facing {
            self.facing = next;
            self.rebuild_cells();
        }
    }

    pub fn set_incline(&mut self, incline: TunnelIncline) {
        if self.phase == StairPhase::Idle || self.phase == StairPhase::Digging {
            return;
        }
        self.incline = incline;
        self.rebuild_cells();
    }

    pub fn nudge_steps(&mut self, delta: i32) {
        if self.phase == StairPhase::Idle || self.phase == StairPhase::Digging {
            return;
        }
        self.steps = (self.steps + delta).clamp(STAIR_MIN_STEPS, STAIR_MAX_STEPS);
        self.rebuild_cells();
    }

    /// Set corridor width (`1` narrow default, `2` optional double).
    pub fn set_width(&mut self, width: i32) {
        if self.phase == StairPhase::Idle || self.phase == StairPhase::Digging {
            return;
        }
        self.width = width.clamp(STAIR_WIDTH_MIN, STAIR_WIDTH_MAX);
        self.rebuild_cells();
    }

    /// Toggle between 1-wide and 2-wide.
    pub fn cycle_width(&mut self) {
        if self.phase == StairPhase::Idle || self.phase == StairPhase::Digging {
            return;
        }
        self.width = if self.width >= STAIR_WIDTH_MAX {
            STAIR_WIDTH_MIN
        } else {
            STAIR_WIDTH_MAX
        };
        self.rebuild_cells();
    }

    /// Keep the yellow ghost rooted at the player while aiming via HUD (no mouse ray).
    pub fn sync_ghost_anchor(&mut self, anchor: IVec3) {
        match self.phase {
            StairPhase::Armed | StairPhase::Preview => {
                self.anchor = Some(anchor);
                self.phase = StairPhase::Preview;
                self.rebuild_cells();
            }
            _ => {}
        }
    }

    /// HUD / keyboard confirm: dig from current preview.
    pub fn confirm_dig(&mut self) -> bool {
        match self.phase {
            StairPhase::Armed | StairPhase::Preview => {
                // Always rebuild so facing / incline / length can't go stale.
                let Some(anchor) = self.anchor else {
                    return false;
                };
                self.cells =
                    compute_stair_cells(anchor, self.facing, self.incline, self.steps, self.width);
                if self.cells.is_empty() {
                    return false;
                }
                self.dig_queue = self.cells.iter().copied().collect();
                self.phase = StairPhase::Digging;
                self.next_dig_at = Instant::now();
                true
            }
            _ => false,
        }
    }

    /// First click: set anchor. Second click: start digging current preview.
    /// Kept for mouse-aim mode / tests (HUD dig is the live path).
    #[allow(dead_code)]
    pub fn on_primary_click(&mut self, hit: IVec3) {
        match self.phase {
            StairPhase::Armed => {
                self.anchor = Some(hit);
                self.phase = StairPhase::Preview;
                self.steps = self.steps.clamp(STAIR_MIN_STEPS, STAIR_MAX_STEPS);
                self.rebuild_cells();
            }
            StairPhase::Preview => {
                let _ = self.confirm_dig();
            }
            StairPhase::Idle | StairPhase::Digging => {}
        }
    }

    /// Refresh length from aim point projected onto the locked facing axis.
    #[allow(dead_code)]
    pub fn update_preview_target(&mut self, target: IVec3) {
        if self.phase != StairPhase::Preview {
            return;
        }
        let Some(anchor) = self.anchor else {
            return;
        };
        let d = self.facing.dir();
        let delta = target - anchor;
        let along = delta.x * d.x + delta.z * d.z;
        self.steps = along
            .abs()
            .max(STAIR_MIN_STEPS)
            .clamp(STAIR_MIN_STEPS, STAIR_MAX_STEPS);
        self.cells = compute_stair_cells(anchor, self.facing, self.incline, self.steps, self.width);
    }

    fn rebuild_cells(&mut self) {
        if self.phase != StairPhase::Preview {
            return;
        }
        let Some(anchor) = self.anchor else {
            return;
        };
        self.cells = compute_stair_cells(anchor, self.facing, self.incline, self.steps, self.width);
    }

    pub fn ghost_cells(&self) -> &[IVec3] {
        match self.phase {
            StairPhase::Preview | StairPhase::Digging => &self.cells,
            _ => &[],
        }
    }

    /// Center of the farthest ghost cells along dig facing (yellow trail tip).
    pub fn ghost_far_point(&self) -> Option<glam::Vec3> {
        let cells = self.ghost_cells();
        if cells.is_empty() {
            return None;
        }
        let d = self.facing.dir();
        let along = |c: IVec3| c.x * d.x + c.z * d.z;
        let best = cells.iter().map(|c| along(*c)).max()?;
        let mut sum = glam::Vec3::ZERO;
        let mut n = 0u32;
        for &c in cells {
            if along(c) != best {
                continue;
            }
            sum += glam::Vec3::new(c.x as f32 + 0.5, c.y as f32 + 0.5, c.z as f32 + 0.5);
            n += 1;
        }
        (n > 0).then(|| sum / n as f32)
    }

    /// Next cell waiting to be excavated when the dig timer has elapsed.
    pub fn peek_dig_at(&self, now: Instant) -> Option<IVec3> {
        if self.phase != StairPhase::Digging || now < self.next_dig_at {
            return None;
        }
        self.dig_queue.front().copied()
    }

    /// Commit removal of the front dig cell and schedule the next strike.
    pub fn commit_dig(&mut self, now: Instant, next_interval: Duration) -> Option<IVec3> {
        if self.phase != StairPhase::Digging {
            return None;
        }
        let Some(pos) = self.dig_queue.pop_front() else {
            self.cancel_to_idle();
            return None;
        };
        if let Some(i) = self.cells.iter().position(|c| *c == pos) {
            self.cells.remove(i);
        }
        self.next_dig_at = now + next_interval;
        if self.dig_queue.is_empty() {
            self.cancel_to_idle();
        }
        Some(pos)
    }

    /// Abort an in-progress dig (broken tool / cancelled).
    pub fn abort_dig(&mut self) {
        if self.phase == StairPhase::Digging {
            self.cancel_to_idle();
        }
    }
}

/// Blocks cleared above each tread (player ≈ 1.8 tall = 2 body cells).
/// Need 3: two for standing clearance + one so the head clears while still
/// on the previous (higher) tread before dropping into a Down step.
pub const STAIR_HEADROOM: i32 = 3;
/// Narrow corridor (default).
pub const STAIR_WIDTH_MIN: i32 = 1;
/// Optional double-wide corridor (path + one block to the right of facing).
pub const STAIR_WIDTH_MAX: i32 = 2;

/// Build blocks to excavate for a stair/tunnel from `anchor` along `facing`.
///
/// Same stepped profile for **Up / Down / Flat** and all **four cardinals**:
/// - `width` blocks wide ([`STAIR_WIDTH_MIN`]..=[`STAIR_WIDTH_MAX`]; default 1)
/// - [`STAIR_HEADROOM`] tall above each tread (fits a 2-block-tall body)
/// - Treads stay solid; **Down** = **Up** with vertical direction inverted (`±i`)
pub fn compute_stair_cells(
    anchor: IVec3,
    facing: TunnelFacing,
    incline: TunnelIncline,
    steps: i32,
    width: i32,
) -> Vec<IVec3> {
    let steps = steps.clamp(STAIR_MIN_STEPS, STAIR_MAX_STEPS);
    let width = width.clamp(STAIR_WIDTH_MIN, STAIR_WIDTH_MAX);
    let d = facing.dir();
    // Right-hand perpendicular (N→+X, E→+Z, S→−X, W→−Z).
    let side = IVec3::new(-d.z, 0, d.x);
    let mut cells = Vec::with_capacity((steps * STAIR_HEADROOM * width) as usize);
    let mut seen = rustc_hash::FxHashSet::default();

    let mut push = |p: IVec3| {
        if p.y < 0 || p.y > crate::world::WORLD_MAX_Y {
            return;
        }
        if seen.insert(p) {
            cells.push(p);
        }
    };

    // Up: dig corridor above tread raised by +i per step.
    // Down: identical profile with rise sign flipped (−i).
    let rise_sign: i32 = match incline {
        TunnelIncline::Up => 1,
        TunnelIncline::Down => -1,
        TunnelIncline::Flat => 0,
    };

    for i in 1..=steps {
        let along = d * i;
        let rise = rise_sign * i;
        // Tread at anchor.y + rise; clear [+1 ..= +HEADROOM] above it.
        for w in 0..width {
            let p = anchor + along + side * w;
            for h in 1..=STAIR_HEADROOM {
                push(IVec3::new(p.x, anchor.y + rise + h, p.z));
            }
        }
    }

    cells
}

/// Along-axis coordinate of a dig cell for the given facing (for tests).
fn along_coord(facing: TunnelFacing, p: IVec3, anchor: IVec3) -> i32 {
    let d = facing.dir();
    (p.x - anchor.x) * d.x + (p.z - anchor.z) * d.z
}

/// Screen-space angle (radians) of a world cardinal relative to the view yaw.
/// 0 = screen-right, +π/2 = screen-down (UI y-down), matching pad layout.
pub fn facing_screen_angle(facing: TunnelFacing, view_yaw: f32) -> f32 {
    let d = facing.dir();
    let fx = d.x as f32;
    let fz = d.z as f32;
    let forward = glam::Vec2::new(view_yaw.cos(), view_yaw.sin());
    let right = glam::Vec2::new(-view_yaw.sin(), view_yaw.cos());
    let world = glam::Vec2::new(fx, fz);
    let sx = world.dot(right);
    // Camera-forward appears toward the top of the pad.
    let sy = -world.dot(forward);
    sy.atan2(sx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_steps_is_ten() {
        assert_eq!(STAIR_MAX_STEPS, 24);
        let a = IVec3::new(0, 20, 0);
        let cells = compute_stair_cells(
            a,
            TunnelFacing::East,
            TunnelIncline::Flat,
            99,
            STAIR_WIDTH_MIN,
        );
        let max_x = cells.iter().map(|c| c.x).max().unwrap();
        assert_eq!(max_x, a.x + STAIR_MAX_STEPS);
    }

    #[test]
    fn downward_stair_is_stepped_not_a_shaft() {
        let a = IVec3::new(0, 20, 0);
        let cells = compute_stair_cells(
            a,
            TunnelFacing::East,
            TunnelIncline::Down,
            8,
            STAIR_WIDTH_MIN,
        );
        assert!(cells.len() >= 8);
        // Never dig under the feet column.
        assert!(cells.iter().all(|c| c.x != 0));
        // Must span several X — a shaft would stack every cell on one column.
        let xs: std::collections::BTreeSet<_> = cells.iter().map(|c| c.x).collect();
        assert!(
            xs.len() >= 8,
            "stair must advance horizontally, got xs={xs:?}"
        );
        // Default 1-wide path along facing (East → +X at z=0).
        assert!(cells.iter().any(|c| c.z == 0));
        assert!(cells.iter().all(|c| c.z == 0), "default width is 1");
        // Each forward column: HEADROOM tall × 1 wide.
        let per_step = (STAIR_HEADROOM * STAIR_WIDTH_MIN) as usize;
        for x in 1..=8 {
            let n = cells.iter().filter(|c| c.x == x).count();
            assert_eq!(n, per_step, "column x={x} should be {per_step}, got {n}");
            let ys: std::collections::BTreeSet<_> =
                cells.iter().filter(|c| c.x == x).map(|c| c.y).collect();
            assert_eq!(
                ys.len(),
                STAIR_HEADROOM as usize,
                "column x={x} must be {STAIR_HEADROOM} tall, ys={ys:?}"
            );
        }
        // Down step 1: tread at 19; clear 20..=22 (3 headroom for 2-tall body).
        assert!(cells.contains(&IVec3::new(1, 20, 0)));
        assert!(cells.contains(&IVec3::new(1, 21, 0)));
        assert!(cells.contains(&IVec3::new(1, 22, 0)));
        assert!(!cells.contains(&IVec3::new(1, 20, 1))); // not 2-wide by default
        assert!(!cells.contains(&IVec3::new(1, 19, 0))); // tread stays
        assert!(cells.contains(&IVec3::new(2, 19, 0)));
        assert!(cells.contains(&IVec3::new(2, 20, 0)));
        assert!(cells.contains(&IVec3::new(2, 21, 0)));
        assert!(!cells.contains(&IVec3::new(2, 18, 0)));
        assert!(cells.contains(&IVec3::new(3, 18, 0)));
        assert!(cells.contains(&IVec3::new(3, 19, 0)));
        assert!(cells.contains(&IVec3::new(3, 20, 0)));
    }

    #[test]
    fn optional_double_width_adds_side_lane() {
        let a = IVec3::new(0, 20, 0);
        let cells = compute_stair_cells(
            a,
            TunnelFacing::East,
            TunnelIncline::Flat,
            4,
            STAIR_WIDTH_MAX,
        );
        assert!(cells.contains(&IVec3::new(1, 21, 0)));
        assert!(cells.contains(&IVec3::new(1, 21, 1))); // right-hand lane
        let n = cells.iter().filter(|c| c.x == 1).count();
        assert_eq!(n, (STAIR_HEADROOM * STAIR_WIDTH_MAX) as usize);
    }

    #[test]
    fn down_matches_up_with_inverted_rise() {
        let a = IVec3::new(0, 20, 0);
        let up = compute_stair_cells(a, TunnelFacing::East, TunnelIncline::Up, 6, STAIR_WIDTH_MIN);
        let down = compute_stair_cells(
            a,
            TunnelFacing::East,
            TunnelIncline::Down,
            6,
            STAIR_WIDTH_MIN,
        );
        assert_eq!(up.len(), down.len());
        for i in 1..=6 {
            let up_ys: std::collections::BTreeSet<_> =
                up.iter().filter(|c| c.x == i).map(|c| c.y).collect();
            let down_ys: std::collections::BTreeSet<_> =
                down.iter().filter(|c| c.x == i).map(|c| c.y).collect();
            let expected_up: std::collections::BTreeSet<_> =
                (1..=STAIR_HEADROOM).map(|h| a.y + i + h).collect();
            let expected_down: std::collections::BTreeSet<_> =
                (1..=STAIR_HEADROOM).map(|h| a.y - i + h).collect();
            assert_eq!(up_ys, expected_up, "Up step {i}");
            assert_eq!(down_ys, expected_down, "Down step {i}");
        }
    }

    #[test]
    fn upward_stair_keeps_tread() {
        let a = IVec3::new(0, 5, 0);
        let cells = compute_stair_cells(
            a,
            TunnelFacing::South,
            TunnelIncline::Up,
            8,
            STAIR_WIDTH_MIN,
        );
        assert!(cells.iter().any(|c| c.z > a.z));
        // First step: tread at y=6 stays; clears 7..=9.
        assert!(!cells.contains(&IVec3::new(0, 6, 1)));
        assert!(cells.contains(&IVec3::new(0, 7, 1)));
        assert!(cells.contains(&IVec3::new(0, 8, 1)));
        assert!(cells.contains(&IVec3::new(0, 9, 1)));
        // Second step rises one more.
        assert!(cells.contains(&IVec3::new(0, 8, 2)));
        assert!(cells.contains(&IVec3::new(0, 9, 2)));
        assert!(cells.contains(&IVec3::new(0, 10, 2)));
        assert_eq!(
            cells.iter().filter(|c| c.z == 1).count(),
            (STAIR_HEADROOM * STAIR_WIDTH_MIN) as usize
        );
    }

    #[test]
    fn flat_corridor_does_not_eat_floor() {
        let a = IVec3::new(0, 10, 0);
        let cells = compute_stair_cells(
            a,
            TunnelFacing::East,
            TunnelIncline::Flat,
            4,
            STAIR_WIDTH_MIN,
        );
        assert!(cells.iter().all(|c| c.y >= 11));
        assert!(cells.contains(&IVec3::new(1, 11, 0)));
        assert!(cells.contains(&IVec3::new(1, 12, 0)));
        assert!(cells.contains(&IVec3::new(1, 13, 0)));
        assert!(!cells.contains(&IVec3::new(1, 11, 1))); // default 1-wide
    }

    #[test]
    fn cardinals_point_correct_axes() {
        let a = IVec3::new(10, 10, 10);
        let n = compute_stair_cells(
            a,
            TunnelFacing::North,
            TunnelIncline::Flat,
            4,
            STAIR_WIDTH_MIN,
        );
        assert!(n.iter().any(|c| c.z < a.z));
        let w = compute_stair_cells(
            a,
            TunnelFacing::West,
            TunnelIncline::Flat,
            4,
            STAIR_WIDTH_MIN,
        );
        assert!(w.iter().any(|c| c.x < a.x));
    }

    #[test]
    fn stepped_on_all_facings_and_inclines() {
        let a = IVec3::new(0, 20, 0);
        let facings = [
            TunnelFacing::North,
            TunnelFacing::South,
            TunnelFacing::East,
            TunnelFacing::West,
        ];
        let inclines = [TunnelIncline::Down, TunnelIncline::Up, TunnelIncline::Flat];
        for &facing in &facings {
            for &incline in &inclines {
                let cells = compute_stair_cells(a, facing, incline, 6, STAIR_WIDTH_MIN);
                assert!(!cells.is_empty(), "{facing:?}/{incline:?}");
                // Advances along facing — never a single-column shaft.
                let alongs: std::collections::BTreeSet<_> =
                    cells.iter().map(|c| along_coord(facing, *c, a)).collect();
                assert_eq!(alongs.len(), 6, "{facing:?}/{incline:?} along={alongs:?}");
                assert!(alongs.iter().all(|&t| t >= 1));
                // Each step column: HEADROOM tall (× WIDTH ⇒ HEADROOM*WIDTH cells).
                let per_step = (STAIR_HEADROOM * STAIR_WIDTH_MIN) as usize;
                for &t in &alongs {
                    let ys: std::collections::BTreeSet<_> = cells
                        .iter()
                        .filter(|c| along_coord(facing, **c, a) == t)
                        .map(|c| c.y)
                        .collect();
                    assert_eq!(
                        ys.len(),
                        STAIR_HEADROOM as usize,
                        "{facing:?}/{incline:?} t={t} ys={ys:?}"
                    );
                    let n = cells
                        .iter()
                        .filter(|c| along_coord(facing, **c, a) == t)
                        .count();
                    assert_eq!(n, per_step, "{facing:?}/{incline:?} t={t} n={n}");
                }
                // Incline slope check on the lower dig Y of each step.
                if incline != TunnelIncline::Flat {
                    let mut prev_y0 = None;
                    for t in 1..=6 {
                        let y0 = cells
                            .iter()
                            .filter(|c| along_coord(facing, **c, a) == t)
                            .map(|c| c.y)
                            .min()
                            .unwrap();
                        if let Some(prev) = prev_y0 {
                            match incline {
                                TunnelIncline::Down => assert_eq!(y0, prev - 1),
                                TunnelIncline::Up => assert_eq!(y0, prev + 1),
                                TunnelIncline::Flat => {}
                            }
                        }
                        prev_y0 = Some(y0);
                    }
                }
            }
        }
    }

    #[test]
    fn headroom_fits_two_block_body_on_up_and_down() {
        // Player height 1.8 needs air in the two blocks above feet, plus one more
        // so the head clears while still on the previous tread (Down approach).
        assert!(crate::player::PLAYER_HEIGHT > 1.0 && crate::player::PLAYER_HEIGHT < 2.0);
        assert_eq!(STAIR_HEADROOM, 3);
        let a = IVec3::new(0, 20, 0);
        for incline in [TunnelIncline::Down, TunnelIncline::Up] {
            let cells = compute_stair_cells(a, TunnelFacing::East, incline, 4, STAIR_WIDTH_MIN);
            for i in 1..=4 {
                let tread = match incline {
                    TunnelIncline::Down => a.y - i,
                    TunnelIncline::Up => a.y + i,
                    TunnelIncline::Flat => a.y,
                };
                for h in 1..=STAIR_HEADROOM {
                    assert!(
                        cells.contains(&IVec3::new(i, tread + h, 0)),
                        "{incline:?} step {i}: missing clear at y={}",
                        tread + h
                    );
                }
                assert!(
                    !cells.contains(&IVec3::new(i, tread, 0)),
                    "{incline:?} step {i}: tread must stay solid"
                );
            }
        }
    }

    #[test]
    fn ghost_far_point_is_trail_tip() {
        let mut t = StairTool::default();
        t.toggle();
        t.set_facing(TunnelFacing::East);
        t.set_incline(TunnelIncline::Flat);
        t.sync_ghost_anchor(IVec3::new(0, 10, 0));
        // Default min steps is 2; +6 → 8.
        t.nudge_steps(6);
        assert_eq!(t.steps, 8);
        let far = t.ghost_far_point().expect("preview tip");
        assert!((far.x - 8.5).abs() < 0.01, "far.x={far:?}");
        assert!((far.z - 0.5).abs() < 0.01, "far.z={far:?}");
    }

    #[test]
    fn yaw_snap_is_cardinal() {
        assert_eq!(TunnelFacing::from_yaw(0.0), TunnelFacing::East);
        assert_eq!(
            TunnelFacing::from_yaw(std::f32::consts::FRAC_PI_2),
            TunnelFacing::South
        );
        assert_eq!(
            TunnelFacing::from_yaw(-std::f32::consts::FRAC_PI_2),
            TunnelFacing::North
        );
    }

    #[test]
    fn pad_up_is_camera_forward() {
        let yaw = 0.0; // look +X
        assert_eq!(PadDir::Up.to_facing(yaw), TunnelFacing::East);
        assert_eq!(PadDir::Down.to_facing(yaw), TunnelFacing::West);
        assert_eq!(PadDir::Right.to_facing(yaw), TunnelFacing::South);
        assert_eq!(PadDir::Left.to_facing(yaw), TunnelFacing::North);
    }

    #[test]
    fn tool_two_click_starts_dig() {
        let mut t = StairTool::default();
        t.toggle();
        assert_eq!(t.phase, StairPhase::Armed);
        t.set_facing(TunnelFacing::East);
        t.set_incline(TunnelIncline::Down);
        t.on_primary_click(IVec3::new(1, 10, 1));
        assert_eq!(t.phase, StairPhase::Preview);
        t.update_preview_target(IVec3::new(6, 4, 1));
        assert!(!t.cells.is_empty());
        t.on_primary_click(IVec3::new(6, 4, 1));
        assert_eq!(t.phase, StairPhase::Digging);
        let first = t.commit_dig(Instant::now(), STAIR_DIG_INTERVAL);
        assert!(first.is_some());
    }

    #[test]
    fn facing_keys_rebuild_preview() {
        let mut t = StairTool::default();
        t.toggle();
        t.on_primary_click(IVec3::new(0, 12, 0));
        t.set_facing(TunnelFacing::North);
        t.set_incline(TunnelIncline::Down);
        t.nudge_steps(4);
        assert!(t.cells.iter().any(|c| c.z < 0));
        t.set_facing(TunnelFacing::West);
        assert!(t.cells.iter().any(|c| c.x < 0));
    }

    #[test]
    fn empty_queue_returns_idle() {
        let mut t = StairTool::default();
        t.toggle();
        t.set_facing(TunnelFacing::East);
        t.set_incline(TunnelIncline::Flat);
        t.on_primary_click(IVec3::new(0, 8, 0));
        t.nudge_steps(2);
        assert!(t.confirm_dig());
        assert_eq!(t.phase, StairPhase::Digging);
        let now = Instant::now();
        while t.phase == StairPhase::Digging {
            let Some(_) = t.commit_dig(now, STAIR_DIG_INTERVAL) else {
                break;
            };
        }
        assert_eq!(t.phase, StairPhase::Idle);
        assert!(t.ghost_cells().is_empty());
        assert!(t.peek_dig_at(now).is_none());
    }

    #[test]
    fn facing_locked_while_digging() {
        let mut t = StairTool::default();
        t.toggle();
        t.set_facing(TunnelFacing::North);
        t.set_incline(TunnelIncline::Down);
        t.on_primary_click(IVec3::new(2, 10, 2));
        t.nudge_steps(3);
        assert!(t.confirm_dig());
        let locked = t.facing;
        t.set_facing(TunnelFacing::East);
        assert_eq!(t.facing, locked);
        t.abort_dig();
        assert_eq!(t.phase, StairPhase::Idle);
    }

    #[test]
    fn ghost_not_regenerated_while_digging() {
        let mut t = StairTool::default();
        t.toggle();
        t.set_facing(TunnelFacing::South);
        t.set_incline(TunnelIncline::Up);
        t.on_primary_click(IVec3::new(0, 5, 0));
        t.nudge_steps(4);
        assert!(t.confirm_dig());
        let before = t.ghost_cells().len();
        let _ = t.commit_dig(Instant::now(), STAIR_DIG_INTERVAL);
        // Digging shrinks ghost; sync_ghost_anchor must not rebuild a full plan.
        t.sync_ghost_anchor(IVec3::new(99, 99, 99));
        assert!(t.ghost_cells().len() <= before);
        assert_eq!(t.phase, StairPhase::Digging);
    }
}
