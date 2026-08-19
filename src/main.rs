//! App loop: Input → player/tool state → mining Hit → dig → dirty → remesh → GPU.
mod animation;
mod biomes;
mod camera;
mod caves;
mod combat;
mod entity_model;
mod hero;
mod hero_pose;
mod hud;
mod inventory;
mod items;
mod mining;
mod player;
mod prefab;
mod realms;
mod render;
mod save;
mod settlements;
mod stair_dig;
mod world;

use camera::{Camera, HeldKeys, MAX_TICKS_PER_FRAME, TICKS_PER_SECOND};
use hud::{
    append_hud, build_compass_hud, build_hotbar_hud, build_inventory_hud, build_stair_hud,
    build_status_hud, hit_test, Hotbar, HudAction, HudMesh,
};
use items::{bare_hand_can_mine, bare_hand_dig_interval, spawn_wooden_pickaxe, ToolInstance};
use mining::{
    break_solid_cell, break_solid_cell_bare, raycast_reach, BreakOutcome, MiningProgress,
};
use player::{MAX_BOTTLES, MAX_HEARTS};
use player::Player;
use render::Renderer;
use stair_dig::{
    PadDir, StairPhase, StairTool, TunnelFacing, TunnelIncline, STAIR_DIG_INTERVAL,
};
use std::sync::Arc;
use std::time::Instant;
use inventory::{InvItem, PlayerInventory};
use world::{
    fragment_drop_for, Material, OreDrop, DEBUG_FACE_VISIBILITY, ENABLE_HD2D, Voxel, World,
};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{CursorGrabMode, Window, WindowId};

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    world: World,
    player: Player,
    camera: Camera,
    keys: HeldKeys,
    cursor_captured: bool,
    last_frame: Instant,
    tick_accum: f32,
    /// Throttle window title updates (avoids format work every frame).
    title_frames: u32,
    /// Smoothed frames-per-second for the title HUD.
    fps_ema: f32,
    /// Smoothed frame time in milliseconds.
    frame_ms_ema: f32,
    stair: StairTool,
    /// Equipped wooden pick (durability from `assets/items/wooden_pickaxe.json`).
    pickaxe: ToolInstance,
    /// Bottom-left weapon / tool hotbar (4 slots).
    hotbar: Hotbar,
    /// Last built stair HUD (for hit-testing clicks).
    stair_hud: HudMesh,
    /// Cursor position in logical pixels (top-left origin).
    mouse_logical: (f32, f32),
    /// Hold-to-break (LMB) while pickaxe is equipped in HD-2D.
    attack_held: bool,
    mining: MiningProgress,
    /// Minecraft-style 9×3 storage (+ UI cursor stack).
    inventory: PlayerInventory,
    /// Inventory panel open (INV button / G).
    inventory_open: bool,
    /// Location sign text (realm + nearest settlement), recomputed on block change.
    sign_line1: String,
    sign_line2: String,
    /// Cached meters to nearest settlement (drives sign opacity fade).
    sign_dist_m: Option<i32>,
    /// Block the sign was last computed for (`None` = never).
    sign_block: Option<(i32, i32)>,
    last_autosave: Instant,
}

impl App {
    fn new() -> Self {
        let mut player = Player::spawn_on_terrain();
        let mut world = if DEBUG_FACE_VISIBILITY {
            World::with_face_debug()
        } else {
            World::new()
        };
        let mut pickaxe = spawn_wooden_pickaxe();
        let mut hotbar = Hotbar::default();
        let mut inventory = PlayerInventory::default();
        if !DEBUG_FACE_VISIBILITY {
            if let Some(loaded) = save::try_load() {
                player.restore_from_save(
                    loaded.feet,
                    loaded.facing,
                    loaded.hearts,
                    loaded.bottles,
                );
                pickaxe.durability = loaded
                    .pickaxe_durability
                    .min(pickaxe.def.max_durability);
                hotbar = loaded.hotbar;
                inventory = loaded.inventory;
                world.load_player_edits(loaded.edits);
            }
        }
        let camera = if DEBUG_FACE_VISIBILITY {
            Camera::looking_at_face_debug()
        } else if ENABLE_HD2D {
            Camera::hd2d_follow(player.focus_position())
        } else {
            Camera::first_person(player.eye_position())
        };
        Self {
            window: None,
            renderer: None,
            world,
            player,
            camera,
            keys: HeldKeys::default(),
            cursor_captured: false,
            last_frame: Instant::now(),
            tick_accum: 0.0,
            title_frames: 0,
            fps_ema: 60.0,
            frame_ms_ema: 16.7,
            stair: StairTool::default(),
            pickaxe,
            hotbar,
            stair_hud: HudMesh::default(),
            mouse_logical: (0.0, 0.0),
            attack_held: false,
            mining: MiningProgress::default(),
            inventory,
            inventory_open: false,
            sign_line1: String::new(),
            sign_line2: String::new(),
            sign_dist_m: None,
            sign_block: None,
            last_autosave: Instant::now(),
        }
    }

    fn close_inventory(&mut self) {
        if !self.inventory_open {
            return;
        }
        self.inventory.return_cursor();
        self.inventory_open = false;
        self.sync_cursor_for_ui();
    }

    fn persist(&mut self, why: &str) {
        match save::write_snapshot(
            &self.world,
            self.player.feet,
            self.player.facing,
            self.player.hearts,
            self.player.bottles,
            self.pickaxe.durability,
            &self.hotbar,
            &self.inventory,
        ) {
            Ok(path) => {
                self.last_autosave = Instant::now();
                log::info!("save ({why}): {}", path.display());
            }
            Err(e) => log::warn!("save ({why}) failed: {e}"),
        }
    }

    fn toggle_inventory(&mut self) {
        if self.inventory_open {
            self.close_inventory();
        } else {
            self.inventory_open = true;
            self.sync_cursor_for_ui();
        }
    }

    fn collect_ore(&mut self, drop: OreDrop) {
        let before_cube = self
            .inventory
            .count_of(InvItem::cube_for_embed(drop.kind));
        let added = self.inventory.add_ore_drop(drop);
        let micros = self
            .inventory
            .count_of(InvItem::micro_for_embed(drop.kind));
        let cubes = self
            .inventory
            .count_of(InvItem::cube_for_embed(drop.kind));
        let name = drop.kind.label();
        if added == 0 {
            log::info!("{name}: inventario lleno");
        } else if cubes > before_cube {
            log::info!(
                "{name}: +{added} micro → cubo (total {cubes} cubos, {micros} micros)"
            );
        } else {
            log::info!("{name}: +{added} micro ({micros} en inventario)");
        }
    }

    fn collect_fragments(&mut self, material: Material) {
        let Some((kind, amount)) = fragment_drop_for(material) else {
            return;
        };
        let added = self.inventory.add_fragments(kind, amount);
        let total = self.inventory.count_of(InvItem::from_fragment(kind));
        if added == 0 {
            log::info!("{}: inventario lleno ({total}/100)", kind.label());
        } else if added < amount {
            log::info!("{}: +{added} (lleno: {total}/100)", kind.label());
        } else {
            log::info!("{}: +{added} ({total}/100)", kind.label());
        }
    }

    fn collect_break_loot(&mut self, material: Material, ore: Option<OreDrop>) {
        self.collect_fragments(material);
        if let Some(drop) = ore {
            self.collect_ore(drop);
        }
    }

    /// Excavate queued stair cells via [`break_solid_cell`] (wear → remove → grass → dirty).
    fn tick_stair_dig(&mut self, now: Instant) {
        while let Some(pos) = self.stair.peek_dig_at(now) {
            let material = self.world.dig_material_at(pos);
            let interval = material
                .map(|m| self.pickaxe.def.dig_interval(m))
                .unwrap_or(STAIR_DIG_INTERVAL);

            match break_solid_cell(&mut self.world, pos, &mut self.pickaxe) {
                BreakOutcome::ToolBlocked => {
                    self.stair.abort_dig();
                    log::info!("pico roto — excavación detenida");
                    break;
                }
                BreakOutcome::Skipped => {
                    let _ = self.stair.commit_dig(now, interval);
                }
                BreakOutcome::Broke { material: mat, ore } => {
                    self.collect_break_loot(mat, ore);
                    let _ = self.stair.commit_dig(now, interval);
                    if matches!(
                        mat,
                        Material::Stone
                            | Material::BlackStone
                            | Material::Coal
                            | Material::Sapphire
                            | Material::Ruby
                            | Material::Emerald
                    ) {
                        break;
                    }
                }
            }
        }
    }

    /// Classic hold-to-break (HD-2D; skips stair mode).
    /// Pickaxe uses tool timings; bare hands / non-dig tools take 10× longer.
    fn tick_hold_break(&mut self, dt: f32) {
        if !ENABLE_HD2D || DEBUG_FACE_VISIBILITY {
            return;
        }
        if self.stair.is_active() || !self.attack_held {
            self.mining.clear();
            return;
        }

        let using_pick = self.hotbar.tool_id().is_some_and(|id| id.can_dig())
            && !self.pickaxe.is_broken();

        let origin = self.player.eye_position();
        let yaw = self.player.facing;
        let dir = glam::Vec3::new(yaw.cos(), -0.35, yaw.sin()).normalize_or_zero();
        let Some(hit) = raycast_reach(&self.world, origin, dir) else {
            self.mining.clear();
            return;
        };
        let Some(material) = self.world.dig_material_at(hit.block) else {
            self.mining.clear();
            return;
        };

        let interval = if using_pick {
            if !self.pickaxe.def.can_mine(material) {
                self.mining.clear();
                return;
            }
            self.pickaxe.def.dig_interval(material)
        } else {
            if !bare_hand_can_mine(material) {
                self.mining.clear();
                return;
            }
            bare_hand_dig_interval(material)
        };

        if self.mining.tick(hit.block, dt, interval) {
            let outcome = if using_pick {
                break_solid_cell(&mut self.world, hit.block, &mut self.pickaxe)
            } else {
                break_solid_cell_bare(&mut self.world, hit.block)
            };
            match outcome {
                BreakOutcome::ToolBlocked => {
                    if using_pick {
                        log::info!("pico roto — hold-break detenido");
                    }
                    self.mining.clear();
                }
                BreakOutcome::Broke { material, ore } => {
                    self.collect_break_loot(material, ore);
                }
                BreakOutcome::Skipped => {}
            }
        }
    }

    fn digging_for_pose(&self) -> bool {
        self.stair.is_digging()
            || (self.attack_held && self.mining.target.is_some())
    }

    fn set_cursor_captured(&mut self, captured: bool) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        if captured {
            let _ = window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
            window.set_cursor_visible(false);
        } else {
            let _ = window.set_cursor_grab(CursorGrabMode::None);
            window.set_cursor_visible(true);
        }
        self.cursor_captured = captured;
    }

    /// Keep the cursor free for HUD clicks (hotbar, INV, stair panel).
    /// HD-2D never locks the mouse; FPS only unlocks while a menu is open.
    fn sync_cursor_for_ui(&mut self) {
        let need_free =
            ENABLE_HD2D || self.stair.is_active() || self.inventory_open;
        if need_free {
            if self.cursor_captured {
                self.set_cursor_captured(false);
            }
        }
    }

    fn sync_cursor_for_stair(&mut self) {
        self.sync_cursor_for_ui();
    }

    fn break_targeted_block(&mut self) {
        let Some(hit) = raycast_reach(
            &self.world,
            self.camera.position,
            self.camera.forward(),
        ) else {
            return;
        };
        if let BreakOutcome::Broke { material, ore } =
            break_solid_cell(&mut self.world, hit.block, &mut self.pickaxe)
        {
            self.collect_break_loot(material, ore);
        }
    }

    fn place_against_targeted_block(&mut self) {
        let Some(hit) = raycast_reach(
            &self.world,
            self.camera.position,
            self.camera.forward(),
        ) else {
            return;
        };
        let place = hit.prev;
        if place == hit.block {
            return;
        }
        let occupied = self.player.occupied_blocks();
        if occupied.contains(&place) {
            return;
        }
        if self.world.get_voxel(place).is_some() {
            return;
        }
        self.world.set_voxel_player(place, Voxel::dirt());
    }

    /// Block under the player’s feet — stair ghost root (no mouse aim).
    fn stair_anchor_block(&self) -> glam::IVec3 {
        let f = self.player.feet;
        glam::IVec3::new(
            f.x.floor() as i32,
            (f.y - 0.05).floor() as i32,
            f.z.floor() as i32,
        )
    }

    /// Look-at for Q/E orbit: stair midpoint when a yellow trail is up.
    fn stair_orbit_focus(&self) -> glam::Vec3 {
        let player = self.player.focus_position();
        if let Some(far) = self.stair.ghost_far_point() {
            (player + far) * 0.5
        } else {
            player
        }
    }

    fn move_yaw(&self) -> f32 {
        if ENABLE_HD2D {
            self.camera.move_yaw()
        } else {
            self.camera.yaw
        }
    }

    /// Turn the character (and dig heading) to a world cardinal from a pad slot.
    fn apply_pad_facing(&mut self, facing: TunnelFacing) {
        self.player.face_yaw(facing.yaw());
        self.stair.set_facing(facing);
    }

    fn apply_hud_action(&mut self, action: HudAction) {
        match action {
            HudAction::Facing(f) => self.apply_pad_facing(f),
            HudAction::Incline(i) => self.stair.set_incline(i),
            HudAction::LenMinus => self.stair.nudge_steps(-1),
            HudAction::LenPlus => self.stair.nudge_steps(1),
            HudAction::WidthToggle => self.stair.cycle_width(),
            HudAction::Dig => {
                if self.pickaxe.is_broken() {
                    log::info!("pico roto — no se puede cavar");
                    return;
                }
                // Keep pad-selected facing (N/S/E/W) — do not overwrite from look yaw.
                self.stair.sync_ghost_anchor(self.stair_anchor_block());
                let _ = self.stair.confirm_dig();
            }
            HudAction::Cancel => {
                self.stair.cancel_to_idle();
                self.sync_cursor_for_stair();
            }
            HudAction::SelectSlot(i) => {
                self.hotbar.press_slot(i as usize);
                self.player.notify_equip();
                if !self.hotbar.holding_pickaxe() && self.stair.is_active() {
                    self.stair.cancel_to_idle();
                    self.sync_cursor_for_stair();
                }
            }
            HudAction::ToggleInventory => {
                self.toggle_inventory();
            }
            HudAction::InvSlot(i) => {
                self.inventory.click_slot(i as usize, false);
            }
            HudAction::InventoryBackdrop => {
                // Click outside the panel closes (Minecraft-like).
                self.close_inventory();
            }
            HudAction::InventoryPanel => {}
        }
    }

    fn apply_hud_click(&mut self, action: HudAction, right: bool) {
        match action {
            HudAction::InvSlot(i) => {
                self.inventory.click_slot(i as usize, right);
            }
            HudAction::InventoryBackdrop => {
                if !right {
                    self.close_inventory();
                }
            }
            HudAction::InventoryPanel => {}
            _ if !right => self.apply_hud_action(action),
            _ => {}
        }
    }

    /// Arrow keys = camera-relative pad; R/F/C = up / down / flat; [/] = length; B = width.
    /// (`G` is inventory — see keyboard handler.)
    fn handle_stair_dir_key(&mut self, key: KeyCode) -> bool {
        let view = self.move_yaw();
        match key {
            KeyCode::ArrowUp => {
                self.apply_pad_facing(PadDir::Up.to_facing(view));
                true
            }
            KeyCode::ArrowDown => {
                self.apply_pad_facing(PadDir::Down.to_facing(view));
                true
            }
            KeyCode::ArrowRight => {
                self.apply_pad_facing(PadDir::Right.to_facing(view));
                true
            }
            KeyCode::ArrowLeft => {
                self.apply_pad_facing(PadDir::Left.to_facing(view));
                true
            }
            KeyCode::KeyR | KeyCode::PageUp => {
                self.stair.set_incline(TunnelIncline::Up);
                true
            }
            KeyCode::KeyF | KeyCode::PageDown => {
                self.stair.set_incline(TunnelIncline::Down);
                true
            }
            KeyCode::KeyC => {
                self.stair.set_incline(TunnelIncline::Flat);
                true
            }
            KeyCode::BracketLeft | KeyCode::Minus => {
                self.stair.nudge_steps(-1);
                true
            }
            KeyCode::BracketRight | KeyCode::Equal => {
                self.stair.nudge_steps(1);
                true
            }
            KeyCode::KeyB => {
                self.stair.cycle_width();
                true
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                if self.pickaxe.is_broken() {
                    log::info!("pico roto — no se puede cavar");
                    return true;
                }
                self.stair.sync_ghost_anchor(self.stair_anchor_block());
                let _ = self.stair.confirm_dig();
                true
            }
            _ => false,
        }
    }

    /// Recompute the location sign when the player steps onto a new block.
    fn refresh_location_sign(&mut self) {
        let feet = self.player.feet;
        let bx = feet.x.floor() as i32;
        let bz = feet.z.floor() as i32;
        let block = (bx, bz);
        if self.sign_block == Some(block) {
            return;
        }
        self.sign_block = Some(block);

        let info = settlements::sign_info_at_block(bx, bz);
        let biome = info.biome.label_es();
        self.sign_line1 = match &info.realm_name {
            Some(name) => format!("REINO {name} · {biome}"),
            None => format!("TIERRA SALVAJE · {biome}"),
        };
        self.sign_dist_m = info.nearest.as_ref().map(|n| n.dist);
        self.sign_line2 = match &info.nearest {
            Some(n) => format!("{} A {} M", n.kind.label_es(), n.dist),
            None => "SIN ASENTAMIENTOS".to_string(),
        };
    }

    fn logical_size(&self) -> (f32, f32) {
        let Some(window) = self.window.as_ref() else {
            return (1280.0, 720.0);
        };
        let s = window.inner_size();
        let scale = window.scale_factor() as f32;
        (
            s.width as f32 / scale.max(0.01),
            s.height as f32 / scale.max(0.01),
        )
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let title = if ENABLE_HD2D {
            "Microverse — HD-2D"
        } else {
            "Microverse — first person"
        };
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Microverse — precargando mundo…")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0)),
                )
                .expect("create window"),
        );

        // Fill nearest shunks before GPU init so the first frame has solid ground.
        if !DEBUG_FACE_VISIBILITY {
            self.world.preload_shunks_around(self.player.focus_position());
            // Edits none at boot; leave stream dirties for warm_start_meshes.
            let _ = self.world.take_dirty_edits();
        }

        window.set_title("Microverse — precargando meshes…");
        let mut renderer = pollster::block_on(Renderer::new(
            window.clone(),
            &self.world,
            &self.camera,
        ));

        let size = window.inner_size();
        self.camera.aspect = size.width as f32 / size.height.max(1) as f32;

        if !DEBUG_FACE_VISIBILITY {
            renderer.warm_start_meshes(&mut self.world, &self.camera);
        }
        window.set_title(title);

        self.window = Some(window);
        self.renderer = Some(renderer);
        // Visible cursor from the start so hotbar / INV / HUD stay clickable.
        self.set_cursor_captured(false);
        self.last_frame = Instant::now();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.persist("exit");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size);
                }
                self.camera.aspect = size.width as f32 / size.height.max(1) as f32;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self
                    .window
                    .as_ref()
                    .map(|w| w.scale_factor() as f32)
                    .unwrap_or(1.0)
                    .max(0.01);
                self.mouse_logical = (position.x as f32 / scale, position.y as f32 / scale);
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let frame_secs = (now - self.last_frame).as_secs_f32().min(0.25);
                self.last_frame = now;

                let inst_fps = if frame_secs > 1e-4 {
                    1.0 / frame_secs
                } else {
                    999.0
                };
                let inst_ms = frame_secs * 1000.0;
                self.fps_ema = self.fps_ema * 0.90 + inst_fps * 0.10;
                self.frame_ms_ema = self.frame_ms_ema * 0.90 + inst_ms * 0.10;

                self.tick_accum += frame_secs * TICKS_PER_SECOND as f32;
                let mut ticks = self.tick_accum.floor() as u32;
                self.tick_accum -= ticks as f32;
                ticks = ticks.min(MAX_TICKS_PER_FRAME);

                if !DEBUG_FACE_VISIBILITY {
                    let move_yaw = self.move_yaw();
                    let dig_pose = self.digging_for_pose();
                    self.player
                        .tick(&self.world, &self.keys, move_yaw, ticks, dig_pose);
                    let alpha = self.tick_accum;
                    if ENABLE_HD2D {
                        let pressure = self
                            .renderer
                            .as_ref()
                            .map(|r| r.pop_pressure())
                            .unwrap_or(0.0);
                        // Stair ghost must track feet before framing the lens.
                        if self.stair.is_active() && self.stair.phase != StairPhase::Digging {
                            self.stair.sync_ghost_anchor(self.stair_anchor_block());
                        }
                        let player_focus = self.player.display_focus(alpha);
                        let (focus, frame_half, confine) =
                            if let Some(far) = self.stair.ghost_far_point() {
                                // Midpoint between hero and yellow-trail tip so the
                                // whole planned dig stays on screen while lengthening.
                                let mid = (player_focus + far) * 0.5;
                                let half = player_focus.distance(far) * 0.5;
                                // Don't pull into confined dig zoom — we need the overview.
                                (mid, half, 0.0)
                            } else {
                                let confine = self.world.hd2d_confine_factor(player_focus);
                                (player_focus, 0.0, confine)
                            };
                        self.camera
                            .follow_hd2d_framed(focus, pressure, confine, frame_half);
                    } else {
                        self.camera
                            .sync_from_player(self.player.display_eye(alpha));
                    }

                    // Stair dig tick (ghost already synced above for HD-2D framing).
                    if self.stair.is_active()
                        && self.stair.phase != StairPhase::Digging
                        && !ENABLE_HD2D
                    {
                        self.stair.sync_ghost_anchor(self.stair_anchor_block());
                    }
                    self.tick_stair_dig(now);
                    let hold_dt = ticks as f32 / TICKS_PER_SECOND as f32;
                    self.tick_hold_break(hold_dt.max(frame_secs));
                }

                let (lw, lh) = self.logical_size();
                let mut hud = build_status_hud(
                    self.player.hearts,
                    MAX_HEARTS,
                    self.player.bottles,
                    MAX_BOTTLES,
                    lw,
                    lh,
                );
                let compass = build_compass_hud(self.player.facing, lw, lh);
                append_hud(&mut hud, &compass);
                self.refresh_location_sign();
                let sign_a = crate::hud::settlement_sign_opacity(self.sign_dist_m);
                let sign = crate::hud::build_realm_sign_hud(
                    &self.sign_line1,
                    &self.sign_line2,
                    lw,
                    lh,
                    sign_a,
                );
                append_hud(&mut hud, &sign);
                let hotbar = build_hotbar_hud(&self.hotbar, self.inventory_open, lw, lh);
                append_hud(&mut hud, &hotbar);
                let inv = build_inventory_hud(
                    self.inventory_open,
                    &self.inventory,
                    &self.hotbar,
                    self.mouse_logical,
                    lw,
                    lh,
                );
                append_hud(&mut hud, &inv);
                let stair = build_stair_hud(&self.stair, self.move_yaw(), lw, lh);
                append_hud(&mut hud, &stair);
                self.stair_hud = hud;

                self.title_frames = self.title_frames.wrapping_add(1);
                if self.last_autosave.elapsed().as_secs_f32() >= save::AUTOSAVE_SECS {
                    self.persist("auto");
                }
                if self.title_frames % 10 == 0 {
                    let (dirt_n, grass_n) = self.world.voxel_counts();
                    let pressure = self
                        .renderer
                        .as_ref()
                        .map(|r| r.pop_pressure())
                        .unwrap_or(0.0);
                    if let Some(window) = &self.window {
                        let mode = if ENABLE_HD2D { "HD-2D" } else { "FPS" };
                        let dir = self.stair.hud_dir();
                        let pick = self.pickaxe.hud_tag();
                        let stair = match self.stair.phase {
                            StairPhase::Idle => String::new(),
                            StairPhase::Armed | StairPhase::Preview => {
                                format!(" | STAIR {dir}: HUD · Enter=dig")
                            }
                            StairPhase::Digging => format!(" | STAIR {dir}: digging…"),
                        };
                        let pi = self.inventory.count_of(InvItem::StoneFrag);
                        let ti = self.inventory.count_of(InvItem::DirtFrag);
                        let c_m = self.inventory.count_of(InvItem::CoalMicro);
                        let c_c = self.inventory.count_of(InvItem::CoalCube);
                        let s_m = self.inventory.count_of(InvItem::SapphireMicro);
                        let s_c = self.inventory.count_of(InvItem::SapphireCube);
                        let r_m = self.inventory.count_of(InvItem::RubyMicro);
                        let r_c = self.inventory.count_of(InvItem::RubyCube);
                        let e_m = self.inventory.count_of(InvItem::EmeraldMicro);
                        let e_c = self.inventory.count_of(InvItem::EmeraldCube);
                        let facing = TunnelFacing::from_yaw(self.player.facing).label_es();
                        window.set_title(&format!(
                            "Microverse — {mode} | {}{stair} | mira:{facing} | {pick} | Pi:{pi}/100 Ti:{ti}/100 | C:{c_m}/{c_c} S:{s_m}/{s_c} R:{r_m}/{r_c} E:{e_m}/{e_c} | {:.0} fps · {:.1} ms | chunks:{} dirt:{} grass:{} cam:{:.0}%",
                            self.player.hearts_title(),
                            self.fps_ema,
                            self.frame_ms_ema,
                            self.world.loaded_chunk_count(),
                            dirt_n,
                            grass_n,
                            pressure * 100.0,
                        ));
                    }
                }

                if let Some(renderer) = self.renderer.as_mut() {
                    let alpha = self.tick_accum;
                    let feet = if ENABLE_HD2D && !DEBUG_FACE_VISIBILITY {
                        Some(self.player.display_feet(alpha))
                    } else {
                        None
                    };
                    let facing = self.player.display_facing(alpha);
                    let pose = self.player.display_hero_pose(alpha);
                    let tool_swing = self.player.display_tool_swing(alpha);
                    let equip = self.player.display_equip_blend(alpha);
                    let ghost = self.stair.ghost_cells();
                    let hud = Some(&self.stair_hud);
                    match renderer.render(
                        &self.camera,
                        &mut self.world,
                        feet,
                        facing,
                        &pose,
                        self.hotbar.tool_id(),
                        tool_swing,
                        equip,
                        ghost,
                        hud,
                    ) {
                        Ok(()) => {}
                        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                            let size = renderer.size();
                            renderer.resize(size);
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => {
                            log::error!("out of memory");
                            event_loop.exit();
                        }
                        Err(e) => log::warn!("surface error: {e:?}"),
                    }
                }

                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(key),
                        state,
                        ..
                    },
                ..
            } => {
                let pressed = state == ElementState::Pressed;
                if key == KeyCode::Escape && pressed {
                    if self.inventory_open {
                        self.close_inventory();
                    } else if self.stair.is_active() {
                        self.stair.cancel_to_idle();
                        self.sync_cursor_for_stair();
                    } else if self.cursor_captured {
                        self.set_cursor_captured(false);
                    } else {
                        self.persist("exit");
                        event_loop.exit();
                    }
                } else if key == KeyCode::F5 && pressed {
                    self.persist("F5");
                } else if key == KeyCode::KeyG && pressed {
                    self.toggle_inventory();
                } else if pressed && matches!(
                    key,
                    KeyCode::Digit1 | KeyCode::Digit2 | KeyCode::Digit3 | KeyCode::Digit4
                        | KeyCode::Numpad1 | KeyCode::Numpad2 | KeyCode::Numpad3 | KeyCode::Numpad4
                ) {
                    let slot = match key {
                        KeyCode::Digit1 | KeyCode::Numpad1 => 0,
                        KeyCode::Digit2 | KeyCode::Numpad2 => 1,
                        KeyCode::Digit3 | KeyCode::Numpad3 => 2,
                        _ => 3,
                    };
                    self.hotbar.press_slot(slot);
                    self.player.notify_equip();
                    // Stowing the pickaxe cancels an armed dig tool.
                    if !self.hotbar.holding_pickaxe() && self.stair.is_active() {
                        self.stair.cancel_to_idle();
                        self.sync_cursor_for_stair();
                    }
                } else if key == KeyCode::KeyT && pressed && !DEBUG_FACE_VISIBILITY {
                    // Stair tool only while the pickaxe is in hand.
                    if self.hotbar.holding_pickaxe() {
                        self.stair.arm_from_yaw(self.player.facing);
                        self.sync_cursor_for_stair();
                    } else {
                        log::info!("equipa el pico (2) para cavar — 2 otra vez para guardarlo");
                    }
                } else if self.stair.is_active()
                    && pressed
                    && !DEBUG_FACE_VISIBILITY
                    && self.handle_stair_dir_key(key)
                {
                    // Facing / incline / length / dig consumed.
                } else if key == KeyCode::KeyF && pressed {
                    if let Some(r) = self.renderer.as_mut() {
                        let flip = r.toggle_hero_winding_flip();
                        log::info!(
                            "hero camera-face flip = {flip} (swaps bright↔dark; no triangle flip)"
                        );
                    }
                } else if ENABLE_HD2D && !DEBUG_FACE_VISIBILITY && pressed {
                    match key {
                        KeyCode::KeyQ => {
                            let f = self.stair_orbit_focus();
                            self.camera.orbit_hd2d(-1, f);
                        }
                        KeyCode::KeyE => {
                            let f = self.stair_orbit_focus();
                            self.camera.orbit_hd2d(1, f);
                        }
                        _ => self.keys.set(key, pressed),
                    }
                } else {
                    self.keys.set(key, pressed);
                }
            }
            WindowEvent::MouseInput {
                state,
                button,
                ..
            } => {
                let pressed = state == ElementState::Pressed;
                if button == MouseButton::Left && !pressed {
                    self.attack_held = false;
                    self.mining.clear();
                    return;
                }

                if !pressed {
                    return;
                }

                if matches!(button, MouseButton::Left | MouseButton::Right) {
                    if let Some(action) =
                        hit_test(&self.stair_hud.hits, self.mouse_logical.0, self.mouse_logical.1)
                    {
                        self.apply_hud_click(action, button == MouseButton::Right);
                        return;
                    }
                }

                if self.stair.is_active() {
                    // Stair mode: mouse only talks to the on-screen panel.
                    if button == MouseButton::Right {
                        self.stair.cancel_preview();
                    }
                    return;
                }

                if self.inventory_open {
                    // Inventory open: clicks only hit HUD slots / INV button.
                    return;
                }

                // HD-2D: free cursor — click always swings; pick hold still mines.
                if ENABLE_HD2D && !DEBUG_FACE_VISIBILITY {
                    if button == MouseButton::Left {
                        let origin = self.player.eye_position();
                        let yaw = self.player.facing;
                        let dir =
                            glam::Vec3::new(yaw.cos(), -0.35, yaw.sin()).normalize_or_zero();
                        let hit = raycast_reach(&self.world, origin, dir);
                        let timid = hit.is_none();
                        let item = self.hotbar.selected_item();
                        let dmg = self.player.begin_melee(item, timid);
                        if !timid {
                            log::debug!("golpe conectado · daño {dmg} (tabla provisional)");
                        }
                        // Hold-to-mine: pickaxe at tool speed, otherwise bare hands (10×).
                        if hit.is_some() {
                            let diggable = hit
                                .as_ref()
                                .and_then(|h| self.world.dig_material_at(h.block))
                                .is_some_and(|m| {
                                    if self.hotbar.holding_pickaxe() && !self.pickaxe.is_broken() {
                                        self.pickaxe.def.can_mine(m)
                                    } else {
                                        bare_hand_can_mine(m)
                                    }
                                });
                            if diggable {
                                self.attack_held = true;
                            }
                        }
                    }
                    return;
                }

                // FPS: first world click captures for look; HUD already handled above.
                if !self.cursor_captured {
                    self.set_cursor_captured(true);
                    return;
                }
                match button {
                    MouseButton::Left => self.break_targeted_block(),
                    MouseButton::Right => self.place_against_targeted_block(),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: winit::event::DeviceId,
        event: DeviceEvent,
    ) {
        if let DeviceEvent::MouseMotion { delta } = event {
            if self.cursor_captured {
                // HD-2D: camera never free-looks; mouse is unused for facing.
                if !ENABLE_HD2D || DEBUG_FACE_VISIBILITY {
                    self.camera.apply_mouse_delta(delta.0, delta.1);
                }
            }
        }
    }
}

fn main() {
    env_logger::init();
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    rayon::ThreadPoolBuilder::new()
        .num_threads(cores)
        .build_global()
        .expect("rayon pool");
    log::info!("rayon pool: {cores} hilos");

    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new();
    event_loop.run_app(&mut app).expect("run app");
}
