use crate::camera::Camera;
use crate::world::World;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::Window;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    color: [f32; 3],
    uv: [f32; 2],
    /// Unused (kept for vertex layout compatibility). Always 0.
    flags: f32,
    seed: f32,
    /// Ambient occlusion factor in `[0, 1]` (1 = fully lit corners).
    ao: f32,
}

/// `flags >= 1.5` → sample terrain atlas; `seed` = tile index.
const FLAG_TERRAIN_TEX: f32 = 2.0;
/// Tiles: grass top, grass side, dirt, wood, leaves, stone A/B/C.
const TERRAIN_ATLAS_TILES: u32 = 12;
const TERRAIN_TILE_TOP: f32 = 0.0;
const TERRAIN_TILE_SIDE: f32 = 1.0;
const TERRAIN_TILE_DIRT: f32 = 2.0;
const TERRAIN_TILE_WOOD: f32 = 3.0;
const TERRAIN_TILE_LEAVES: f32 = 4.0;
/// First stone variant — shader picks among 5..7 per world cell.
const TERRAIN_TILE_STONE: f32 = 5.0;
const TERRAIN_TILE_PLANKS: f32 = 8.0;
const TERRAIN_TILE_VILLAGE_STONE: f32 = 9.0;
const TERRAIN_TILE_COBBLE: f32 = 10.0;
const TERRAIN_TILE_SAND: f32 = 11.0;

impl Vertex {
    fn solid(position: [f32; 3], normal: [f32; 3], color: [f32; 3]) -> Self {
        Self::solid_ao(position, normal, color, 1.0)
    }

    fn solid_ao(position: [f32; 3], normal: [f32; 3], color: [f32; 3], ao: f32) -> Self {
        Self {
            position,
            normal,
            color,
            uv: [0.0, 0.0],
            flags: 0.0,
            seed: 0.0,
            ao,
        }
    }

    /// Dirt/grass-block face with Minecraft-style atlas UVs (world-space tiling).
    fn terrain_face(
        position: [f32; 3],
        normal: [f32; 3],
        uv: [f32; 2],
        tile: f32,
        tint: [f32; 3],
        ao: f32,
    ) -> Self {
        Self {
            position,
            normal,
            color: tint,
            uv,
            flags: FLAG_TERRAIN_TEX,
            seed: tile,
            ao,
        }
    }

    /// Flat dirt color. In HD-2D, +Y tops use grass green only on the natural surface.
    /// With [`crate::world::DEBUG_FACE_VISIBILITY`], each axis gets a distinct tint.
    fn flat_face(position: [f32; 3], normal: [f32; 3], top_green: bool, ao: f32) -> Self {
        use crate::world::{Material, DEBUG_FACE_VISIBILITY, ENABLE_HD2D};
        let color = if DEBUG_FACE_VISIBILITY && !cfg!(test) {
            debug_face_color(normal)
        } else if top_green && ENABLE_HD2D {
            Material::Grass.color_rgb()
        } else {
            Material::Dirt.color_rgb()
        };
        Self::lit(position, normal, color, ao)
    }

    fn lit(position: [f32; 3], normal: [f32; 3], color: [f32; 3], ao: f32) -> Self {
        Self {
            position,
            normal,
            color,
            uv: [0.0, 0.0],
            flags: 0.0,
            seed: 0.0,
            ao,
        }
    }
}

/// World-space UV so greedy quads tile 1×1 per block.
fn terrain_uv(pos: [f32; 3], neighbor: glam::IVec3) -> [f32; 2] {
    if neighbor.y != 0 {
        [pos[0], pos[2]]
    } else if neighbor.x != 0 {
        [pos[2], -pos[1]]
    } else {
        [pos[0], -pos[1]]
    }
}

/// Atlas tile for terrain and fully-solid construction materials.
/// - Stone cells → stone grain tile.
/// - +Y (tapa): grass-top only on the natural undug surface; dug / buried lids = dirt.
/// - Sides / bottom: plain dirt (never the grass-block side with a green fringe stripe).
fn terrain_tile_for_block(world: &World, block_pos: glam::IVec3, neighbor: glam::IVec3) -> f32 {
    use crate::world::Material;
    match world.get_voxel(block_pos).map(|v| v.material) {
        Some(Material::Stone) => return TERRAIN_TILE_STONE,
        Some(Material::WoodPlanks) => return TERRAIN_TILE_PLANKS,
        Some(Material::VillageStone) => return TERRAIN_TILE_VILLAGE_STONE,
        Some(Material::Cobblestone) | Some(Material::RoadCobble) => return TERRAIN_TILE_COBBLE,
        Some(Material::Sand) => return TERRAIN_TILE_SAND,
        _ => {}
    }
    if neighbor.y > 0 && dirt_top_is_grassy(world, block_pos) {
        TERRAIN_TILE_TOP
    } else {
        // Walls, undersides, and subsurface lids: dirt only.
        TERRAIN_TILE_DIRT
    }
}

/// HD-2D: green tapa only on the natural world-gen surface (`terrain_height`).
/// Dug treads and anything below the lip stay dirt-top.
fn dirt_top_is_grassy(world: &World, block_pos: glam::IVec3) -> bool {
    use crate::world::{terrain_height, Material};
    if !crate::world::ENABLE_HD2D {
        return false;
    }
    if !matches!(
        world.get_voxel(block_pos).map(|v| v.material),
        Some(Material::Dirt)
    ) {
        return false;
    }
    if block_pos.y != terrain_height(block_pos.x, block_pos.z) {
        return false;
    }
    // Settlement ground is trodden dirt/sand, never a grass lid.
    !crate::settlements::settlement_claims_block_cached(block_pos.x, block_pos.z)
}

/// Grass density / state keys use the player focus in HD-2D (not the lens).
fn grass_density_origin(camera: &Camera) -> Vec3 {
    if crate::world::ENABLE_HD2D {
        camera.hd2d_focus()
    } else {
        camera.position
    }
}

/// Stream + dirt LOD distance origin. HD-2D must use focus — lens pull-in
/// (confine) would otherwise shift the bubble and punch chunk holes.
fn mesh_stream_origin(camera: &Camera) -> Vec3 {
    if crate::world::ENABLE_HD2D {
        camera.hd2d_focus()
    } else {
        camera.position
    }
}

/// Distinct RGB per face axis so orbiting a cube proves all 6 sides draw.
fn debug_face_color(normal: [f32; 3]) -> [f32; 3] {
    match (
        normal[0].round() as i32,
        normal[1].round() as i32,
        normal[2].round() as i32,
    ) {
        (1, 0, 0) => [0.95, 0.20, 0.20],  // +X red
        (-1, 0, 0) => [0.55, 0.10, 0.45], // -X magenta
        (0, 1, 0) => [0.95, 0.90, 0.25],  // +Y yellow
        (0, -1, 0) => [0.20, 0.35, 0.95], // -Y blue (floor)
        (0, 0, 1) => [0.25, 0.85, 0.30],  // +Z green
        (0, 0, -1) => [0.20, 0.75, 0.85], // -Z cyan
        _ => [1.0, 1.0, 1.0],
    }
}

/// Face axis id 1..6 for DEBUG_HERO_CAMERA_FACES (matches color legend).
fn hero_face_axis_id(normal: [f32; 3]) -> u32 {
    let ax = normal[0].abs();
    let ay = normal[1].abs();
    let az = normal[2].abs();
    if ax >= ay && ax >= az {
        if normal[0] >= 0.0 {
            1
        } else {
            2
        }
    } else if ay >= az {
        if normal[1] >= 0.0 {
            3
        } else {
            4
        }
    } else if normal[2] >= 0.0 {
        5
    } else {
        6
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GrassInstance {
    origin: [f32; 3],
    color: [f32; 3],
    /// 0..11: tile = variant % 6, horizontal flip when variant >= 6.
    variant: f32,
}

/// Tiles in `assets/grass1.png` … `grass6.png`; × mirror = 12 looks.
const GRASS_VARIANT_COUNT: u32 = 12;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniform {
    view_proj: [[f32; 4]; 4],
    light_view_proj: [[f32; 4]; 4],
    camera_pos: [f32; 3],
    time: f32,
    fog_color: [f32; 3],
    fog_density: f32,
    focus_dist: f32,
    shadow_bias: f32,
    surface_y: f32,
    eye_y: f32,
    /// Player / HD-2D focus XZ — dig cutaway bubble.
    focus_xz: [f32; 2],
    /// 0 open field → 1 buried/roofed/walled (`World::hd2d_confine_factor`).
    confine: f32,
    /// 0 outdoors → 1 inside four walls + roof (`World::indoors_factor`).
    indoors: f32,
}

const _: () = assert!(std::mem::size_of::<FrameUniform>() == 192);

const FOG_RGB: [f32; 3] = [0.62, 0.76, 0.90]; // init fallback; runtime uses scene_altitude

/// Natural terrain surface + eye Y + fog colour for altitude / underground wire.
/// Uses undug `terrain_height` (not column after dig) so pit walls stay "underground".
/// Returns `(fog_rgb, surface_y, eye_y, focus_xz)`.
fn scene_altitude(camera: &Camera, _world: &World) -> ([f32; 3], f32, f32, [f32; 2]) {
    use crate::biomes::biome_at;
    use crate::world::{fog_color_for_biome, terrain_height};
    let focus = if crate::world::ENABLE_HD2D {
        camera.hd2d_focus()
    } else {
        camera.position
    };
    let gx = focus.x.floor() as i32;
    let gz = focus.z.floor() as i32;
    // Natural height — digs must not collapse the reference or wire never appears.
    let surface = terrain_height(gx, gz) as f32;
    let eye_y = focus.y;
    (
        fog_color_for_biome(eye_y, surface, biome_at(gx, gz)),
        surface,
        eye_y,
        [focus.x, focus.z],
    )
}
/// Placeholder removed — hero uses [`crate::hero`] voxel colours.
/// Scene depth+stencil (stencil marks solid hero pixels so X-ray cannot self-hit).
/// `Depth24PlusStencil8` needs no extra device feature (unlike Depth32FloatStencil8).
const SCENE_DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;

fn stencil_mark_player() -> wgpu::StencilState {
    let face = wgpu::StencilFaceState {
        compare: wgpu::CompareFunction::Always,
        fail_op: wgpu::StencilOperation::Keep,
        // Behind a tree: depth fails → leave stencil 0 so the occluded pass can draw.
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op: wgpu::StencilOperation::Replace,
    };
    wgpu::StencilState {
        front: face,
        back: face,
        read_mask: 0xff,
        write_mask: 0xff,
    }
}

/// X-ray only where stencil is still 0 (no solid hero fragment won the pixel).
fn stencil_occluded_non_player() -> wgpu::StencilState {
    let face = wgpu::StencilFaceState {
        compare: wgpu::CompareFunction::Equal,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op: wgpu::StencilOperation::Keep,
    };
    wgpu::StencilState {
        front: face,
        back: face,
        read_mask: 0xff,
        write_mask: 0x00,
    }
}

/// 640² shadow map: ~30% menos fill que 768² en el shadow pass.
/// El PCF 9-tap de `shader.wgsl` compensa el texel más grande.
/// En Android (GPU de móvil) 256²: el kernel del shader es relativo
/// (texel 1/960 fijo); sombras blandas, ~6× menos fill que en PC.
const SHADOW_MAP_SIZE: u32 = if cfg!(target_os = "android") {
    256
} else {
    640
};

/// [Opt A — normalizado a 1.0] Antes el pase de escena iba a 2/3 y el blur
/// lo upscaleaba (Linear). El usuario pidió nitidez normalizada en ambos
/// modos, así que escena y blur van a resolución nativa; el FPS se gana
/// con shadow 640 + hierba leve en vez de con downscale.
/// En Android (GPU de móvil) 0.66 de base (la dinámica baja hasta 0.45):
/// en 6" denso el upscale lo enmascara el bokeh.
const SCENE_INTERNAL_SCALE: f32 = if cfg!(target_os = "android") {
    0.66
} else {
    1.0
};

fn scene_internal_size(w: u32, h: u32) -> (u32, u32) {
    (
        ((w as f32 * SCENE_INTERNAL_SCALE).round().max(1.0)) as u32,
        ((h as f32 * SCENE_INTERNAL_SCALE).round().max(1.0)) as u32,
    )
}

/// [A-ext] FP nítido + diorama interno: primera persona va a resolución
/// nativa (1:1), el diorama HD-2D sigue a 2/3. Mismo umbral que el culling
/// de secciones (`hd2d_amount >= 0.25`) para no mezclar estados.
/// NOTA: con `SCENE_INTERNAL_SCALE = 1.0` ambos modos son nativos y esta
/// función devuelve nativo en los dos casos (se mantiene para no reabrir
/// el downscale sin querer y para que el blur texel siga coherente).
fn scene_internal_size_for(w: u32, h: u32, diorama: bool) -> (u32, u32) {
    if diorama {
        scene_internal_size(w, h)
    } else {
        (w.max(1), h.max(1))
    }
}

/// Tope de superficie en Android: el lado largo se capa a 1440. El pase de
/// blur→swapchain y el HUD van a resolución nativa (2400+ en gama alta);
/// capar recorta ~64% del fill de esos pases con un upscale que el bokeh
/// enmascara. En PC devuelve el tamaño tal cual.
fn capped_surface_size(w: u32, h: u32) -> (u32, u32) {
    if !cfg!(target_os = "android") {
        return (w.max(1), h.max(1));
    }
    let longest = w.max(h).max(1) as f32;
    let s = (1440.0 / longest).min(1.0);
    (
        ((w as f32 * s).round().max(1.0)) as u32,
        ((h as f32 * s).round().max(1.0)) as u32,
    )
}

/// Peldaños de la escala dinámica (Android): 0.66 base, 0.55, 0.45 suelo.
/// Pura para tests; la aplica `Renderer::auto_scene_scale`.
fn dyn_scale_step(cur: f32, fps: f32) -> f32 {
    const LADDER: [f32; 3] = [0.66, 0.55, 0.45];
    if fps < 45.0 {
        LADDER
            .iter()
            .find(|&&s| s < cur - 1e-4)
            .copied()
            .unwrap_or(cur)
    } else if fps > 57.0 {
        LADDER
            .iter()
            .rev()
            .find(|&&s| s > cur + 1e-4)
            .copied()
            .unwrap_or(cur)
    } else {
        cur
    }
}

#[inline]
fn is_diorama_view(camera: &Camera) -> bool {
    crate::world::ENABLE_HD2D && camera.hd2d_amount() >= 0.25
}

fn light_view_proj(camera_pos: Vec3) -> Mat4 {
    let light_dir = Vec3::new(-0.4, 0.9, -0.2).normalize();
    // Follow camera XZ; slight Y lift so hills near the player stay in the volume.
    let center = Vec3::new(camera_pos.x, camera_pos.y * 0.15 + 4.0, camera_pos.z);
    let eye = center + light_dir * 55.0;
    let view = Mat4::look_at_rh(eye, center, Vec3::Y);
    // Tighter ortho → more texels per block (disguises stair-step edges).
    let extent = 48.0;
    let proj = Mat4::orthographic_rh(-extent, extent, -extent, extent, 1.0, 130.0);
    proj * view
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlurUniform {
    amount: f32,
    texel_x: f32,
    texel_y: f32,
    /// Corner / fisheye peripheral soft (HD-2D).
    edge_blur: f32,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    render_pipeline: wgpu::RenderPipeline,
    /// HD-2D hero (opaque) — writes stencil so occluded pass cannot self-hit.
    player_solid_pipeline: wgpu::RenderPipeline,
    /// Occluded: depth-reset (far) under silhouette, then LessEqual color (self-sort).
    player_occluded_depth_pipeline: wgpu::RenderPipeline,
    player_occluded_pipeline: wgpu::RenderPipeline,
    /// Castle Story stair blueprint (yellow translucent cubes).
    ghost_pipeline: wgpu::RenderPipeline,
    grass_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    #[allow(dead_code)] // kept for optional cutout shadows; grass uses blob contacts instead
    shadow_grass_pipeline: wgpu::RenderPipeline,
    grass_template_vb: wgpu::Buffer,
    grass_template_ib: wgpu::Buffer,
    grass_template_index_count: u32,
    /// Magenta-keyed grass sprite (group 2 on grass pipelines).
    _grass_texture: wgpu::Texture,
    _grass_view: wgpu::TextureView,
    _grass_sampler: wgpu::Sampler,
    grass_bind_group: wgpu::BindGroup,
    /// Minecraft-style dirt atlas (group 1 on solid dirt pipeline).
    _terrain_texture: wgpu::Texture,
    _terrain_view: wgpu::TextureView,
    _terrain_sampler: wgpu::Sampler,
    terrain_bind_group: wgpu::BindGroup,
    /// Per-section GPU meshes (16³ B-split keys `(cx, cy, cz)`) kept in VRAM
    /// (LRU beyond the loaded world bubble).
    chunk_meshes: std::collections::HashMap<(i32, i32, i32), ChunkMeshEntry>,
    /// Monotonic frame counter for LRU.
    frame_index: u64,
    /// Last frustum/horizon-visible set (draw order).
    visible_chunks: Vec<(i32, i32, i32)>,
    /// Sections waiting for a remesh (amortized). O(1) membership via hash set.
    chunk_rebuild_queue: rustc_hash::FxHashSet<(i32, i32, i32)>,
    /// Player-edit sections — rebuilt before streaming / LOD upgrades (anti ghost solid).
    edit_priority: rustc_hash::FxHashSet<(i32, i32, i32)>,
    /// Soft cap on dirty chunks meshed per frame (time budget is the real limit).
    chunk_rebuilds_per_frame: usize,
    /// Instrumentación (log `perf` en Android): ms de mallado, ms de upload
    /// y secciones reconstruidas del último `render()`. Solo medición, no
    /// cambian ninguna decisión del loop.
    last_mesh_ms: f32,
    last_upload_ms: f32,
    last_rebuilt: usize,
    /// Huecos del anillo keep en el último `render()` (pantalla de carga).
    /// Empieza en MAX para que el primer frame ya marque cargando.
    last_ring_holes: usize,
    /// Last blur amount uploaded (skip uniform write when stable).
    cached_blur_amount: Option<f32>,
    /// Fast-path key: skip the streaming scan + rebuild when the camera hasn't
    /// left its chunk (and hasn't drifted >8 blocks), nothing is dirty and no
    /// orbit preload waits. Streaming itself discovers new chunks, so it must
    /// still run on moves — just not on stationary frames.
    last_stream_origin: Option<Vec3>,
    last_stream_chunk: Option<(i32, i32)>,
    frame_buffer: wgpu::Buffer,
    frame_bind_group: wgpu::BindGroup,
    /// Uniform-only bind group for the shadow pass (must not bind the shadow map).
    shadow_frame_bind_group: wgpu::BindGroup,
    depth_view: wgpu::TextureView,
    /// Kept so the shadow map texture is not dropped while `shadow_view` is in use.
    #[allow(dead_code)]
    shadow_texture: wgpu::Texture,
    shadow_view: wgpu::TextureView,
    scene_texture: wgpu::Texture,
    scene_view: wgpu::TextureView,
    /// Qué escala tiene el target actual: true = diorama 2/3, false = FP nativo.
    /// Se recrea solo al cruzar el umbral (no cada frame del blend).
    scene_is_diorama: bool,
    /// Escala interna de escena efectiva (Android: la mueve `auto_scene_scale`
    /// 0.75↔0.68↔0.60 según fps; en PC siempre `SCENE_INTERNAL_SCALE`).
    dyn_scale: f32,
    blur_pipeline: wgpu::RenderPipeline,
    blur_bind_group_layout: wgpu::BindGroupLayout,
    blur_bind_group: wgpu::BindGroup,
    blur_uniform_buffer: wgpu::Buffer,
    blur_sampler: wgpu::Sampler,
    start_time: std::time::Instant,
    /// Dynamic HD-2D player billboard (rebuilt each frame when present).
    player_vb: Option<wgpu::Buffer>,
    player_ib: wgpu::Buffer,
    player_vb_cap: u64,
    player_ib_cap: u64,
    player_index_count: u32,
    /// Body index count — solid/shadow/occluded all draw this range (no footing disc).
    player_body_index_count: u32,
    /// Flip hero/debug-cube triangle winding (Key F). Tests CW vs CCW face orientation.
    pub hero_winding_flip: bool,
    /// Stair dig yellow ghost (rebuilt when the cell set changes).
    ghost_vb: Option<wgpu::Buffer>,
    ghost_ib: Option<wgpu::Buffer>,
    ghost_vb_cap: u64,
    ghost_ib_cap: u64,
    ghost_index_count: u32,
    /// FP block highlights (target white / stood-on cyan): same tinted cubes
    /// but outset and depth-tested — no X-ray, occluded by nearer terrain.
    highlight_pipeline: wgpu::RenderPipeline,
    /// Agua traslúcida real (mezcla alfa, sin dither): se dibuja tras lo opaco.
    water_pipeline: wgpu::RenderPipeline,
    hl_vb: Option<wgpu::Buffer>,
    hl_ib: Option<wgpu::Buffer>,
    hl_vb_cap: u64,
    hl_ib_cap: u64,
    hl_index_count: u32,
    /// Grieta de rotura (velo oscuro por etapas, alfa ~0.12).
    crack_pipeline: wgpu::RenderPipeline,
    crack_vb: Option<wgpu::Buffer>,
    crack_ib: Option<wgpu::Buffer>,
    crack_vb_cap: u64,
    crack_ib_cap: u64,
    crack_index_count: u32,
    /// On-screen stair controls (clip-space quads + icon atlas).
    hud_pipeline: wgpu::RenderPipeline,
    hud_bind_group: wgpu::BindGroup,
    _hud_texture: wgpu::Texture,
    _hud_sampler: wgpu::Sampler,
    hud_vb: Option<wgpu::Buffer>,
    hud_ib: Option<wgpu::Buffer>,
    hud_vb_cap: u64,
    hud_ib_cap: u64,
    hud_index_count: u32,
}

struct ChunkGpuMesh {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    num_vertices: u32,
    num_indices: u32,
    /// Per-direction index ranges `[(start, count); 6]` in [+X,-X,+Y,-Y,+Z,-Z]
    /// order — the main pass draws only front-facing buckets (Fase 2b), the
    /// shadow pass draws the full buffer.
    dir_ranges: [(u32, u32); 6],
    grass_instance_buffer: Option<wgpu::Buffer>,
    num_grass: u32,
    /// Agua separada: se dibuja traslúcida tras lo opaco (sin dither).
    water_vertex_buffer: Option<wgpu::Buffer>,
    water_index_buffer: Option<wgpu::Buffer>,
    num_water: u32,
}

struct ChunkMeshEntry {
    /// Distance-band key for this chunk.
    state_key: u64,
    /// Separate grass density band so dirt LOD can change without always regenerating grass.
    grass_key: u64,
    gpu: Option<ChunkGpuMesh>,
    /// Last [`Renderer::frame_index`] that drew or validated this entry.
    last_used: u64,
    /// Frame when we first noticed a filled chunk without GPU mesh (Chunk Guardian).
    hole_since: Option<u64>,
    /// Empty rebuilds seen while `section_may_have_content` still claims the
    /// section could hold geometry — bounded retries before sealing "clean".
    empty_retries: u32,
}

/// Soft CPU budget for meshing + GPU upload inside one render frame.
const FRAME_MESH_BUDGET_MS: u64 = 4;
/// Extra stream budget while a Q/E snap waits for its target view to preload
/// (faster fill of the bubble toward the pending direction).
const ORBIT_PRELOAD_STREAM_BUDGET_MS: u64 = 18;
/// Extra mesh budget per frame while the orbit preload gate is active.
const ORBIT_PRELOAD_MESH_BUDGET_MS: u64 = 8;
/// Max empty-mesh rebuild retries before a section is accepted as genuinely
/// empty (the Chunk Guardian stops retrying after that). Prevents a streaming
/// fill / spillover racing this mesh pass from being sealed as a permanent
/// 16×16 corner hole.
const EMPTY_MESH_RETRY_LIMIT: u32 = 3;
/// Max section meshes retained in VRAM (loaded + recently unloaded LRU).
/// 4× the old 512-column budget would be 2048; empty upper slabs are never
/// stored, so 1536 covers the same view ring with headroom for the split.
const MAX_VRAM_CHUNK_MESHES: usize = 1536;

/// Grow-only dynamic GPU buffer: recreate only when capacity is too small.
fn write_dynamic_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    buffer: &mut wgpu::Buffer,
    capacity: &mut u64,
    label: &str,
    usage: wgpu::BufferUsages,
    bytes: &[u8],
) {
    let need = bytes.len() as u64;
    if need == 0 {
        return;
    }
    if need > *capacity {
        let cap = need.next_power_of_two().max(4096);
        *buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: cap,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        *capacity = cap;
    }
    queue.write_buffer(buffer, 0, bytes);
}

fn write_dynamic_opt_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    slot: &mut Option<wgpu::Buffer>,
    capacity: &mut u64,
    label: &str,
    usage: wgpu::BufferUsages,
    bytes: &[u8],
) {
    let need = bytes.len() as u64;
    if need == 0 {
        return;
    }
    let grow = match slot {
        None => true,
        Some(_) => need > *capacity,
    };
    if grow {
        let cap = need.next_power_of_two().max(4096);
        *slot = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: cap,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        *capacity = cap;
    }
    if let Some(buf) = slot.as_ref() {
        queue.write_buffer(buf, 0, bytes);
    }
}

/// Index-bucket order: [+X, -X, +Y, -Y, +Z, -Z].
fn bucket_for_normal(n: [f32; 3]) -> usize {
    let ax = n[0].abs();
    let ay = n[1].abs();
    let az = n[2].abs();
    if ax >= ay && ax >= az {
        if n[0] >= 0.0 { 0 } else { 1 }
    } else if ay >= az {
        if n[1] >= 0.0 { 2 } else { 3 }
    } else if n[2] >= 0.0 {
        4
    } else {
        5
    }
}

/// Reorder triangles into 6 contiguous direction ranges for front-only draw.
/// Every emitter uses uniform-normal tris, so the first vertex decides the
/// bucket. Returns `(indices, [(start, count); 6])` covering each input index
/// exactly once (empty input → zeroed ranges).
fn sort_indices_by_direction(vertices: &[Vertex], indices: &[u32]) -> (Vec<u32>, [(u32, u32); 6]) {
    let mut buckets: [Vec<u32>; 6] = Default::default();
    for tri in indices.chunks_exact(3) {
        let b = tri
            .first()
            .copied()
            .and_then(|i| vertices.get(i as usize))
            .map(|v| bucket_for_normal(v.normal))
            .unwrap_or(0);
        buckets[b].extend_from_slice(tri);
    }
    let mut out = Vec::with_capacity(indices.len());
    let mut ranges = [(0u32, 0u32); 6];
    for (b, bucket) in buckets.iter().enumerate() {
        let start = out.len() as u32;
        out.extend_from_slice(bucket);
        ranges[b] = (start, bucket.len() as u32);
    }
    // Any trailing partial triangle (shouldn't happen) goes to bucket 0.
    let rem = indices.len() % 3;
    if rem > 0 {
        let start = out.len() as u32;
        out.extend_from_slice(&indices[indices.len() - rem..]);
        ranges[0].1 += rem as u32;
        let _ = start;
    }
    (out, ranges)
}

/// Front-facing direction buckets for one 16³ slab from `cam` (bit i =
/// draw bucket i). Padded by the slab half-diagonal so near/grazing quads are
/// never wrongly culled; camera inside the slab draws everything.
fn section_face_mask(cam: Vec3, cx: i32, cy: i32, cz: i32) -> u64 {
    use crate::world::{mesh_section_center, MESH_CHUNK_SIZE, MESH_SECTION_HEIGHT};
    let c = mesh_section_center(cx, cy, cz);
    let hx = MESH_CHUNK_SIZE as f32 * 0.5 + 1.0;
    let hy = MESH_SECTION_HEIGHT as f32 * 0.5 + 1.0;
    if (cam.x - c.x).abs() <= hx && (cam.y - c.y).abs() <= hy && (cam.z - c.z).abs() <= hx {
        return 0x3F;
    }
    // Half-diagonal of the 16³ slab: worst-case center-vs-corner divergence.
    const PAD: f32 = 13.9;
    let dx = cam.x - c.x;
    let dy = cam.y - c.y;
    let dz = cam.z - c.z;
    let mut mask = 0u64;
    if dx > -PAD { mask |= 1 << 0; }
    if dx < PAD { mask |= 1 << 1; }
    if dy > -PAD { mask |= 1 << 2; }
    if dy < PAD { mask |= 1 << 3; }
    if dz > -PAD { mask |= 1 << 4; }
    if dz < PAD { mask |= 1 << 5; }
    mask
}

fn upload_chunk_gpu(
    device: &wgpu::Device,
    vertices: &[Vertex],
    indices: &[u32],
    grass: &[GrassInstance],
    water_vertices: &[Vertex],
    water_indices: &[u32],
) -> Option<ChunkGpuMesh> {
    if indices.is_empty() && grass.is_empty() && water_indices.is_empty() {
        return None;
    }
    // Enforce GPU budget caps (same limits as mesh tests).
    let vert_cap = MAX_VERTICES as usize;
    let idx_cap = MAX_INDICES as usize;
    let vertices = if vertices.len() > vert_cap {
        log::warn!(
            "chunk mesh truncated: {} verts > MAX_VERTICES {}",
            vertices.len(),
            MAX_VERTICES
        );
        &vertices[..vert_cap]
    } else {
        vertices
    };
    let indices = if indices.len() > idx_cap {
        log::warn!(
            "chunk mesh truncated: {} indices > MAX_INDICES {}",
            indices.len(),
            MAX_INDICES
        );
        &indices[..idx_cap - (idx_cap % 3)]
    } else {
        indices
    };
    // Front-only draw buckets (same triangles, reordered — no extra memory).
    let (indices, dir_ranges) = sort_indices_by_direction(vertices, indices);
    let vertex_buffer = if vertices.is_empty() {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("chunk_vb_empty"),
            size: 4,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        })
    } else {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("chunk_vb"),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        })
    };
    let index_buffer = if indices.is_empty() {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("chunk_ib_empty"),
            size: 4,
            usage: wgpu::BufferUsages::INDEX,
            mapped_at_creation: false,
        })
    } else {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("chunk_ib"),
            contents: bytemuck::cast_slice(indices.as_slice()),
            usage: wgpu::BufferUsages::INDEX,
        })
    };
    let n_grass = grass.len().min(MAX_GRASS_INSTANCES as usize) as u32;
    let grass_instance_buffer = if n_grass > 0 {
        Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("chunk_grass_ib"),
                contents: bytemuck::cast_slice(&grass[..n_grass as usize]),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        )
    } else {
        None
    };
    let num_water = water_indices.len() as u32;
    let water_vertex_buffer = if water_vertices.is_empty() {
        None
    } else {
        Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("chunk_water_vb"),
                contents: bytemuck::cast_slice(water_vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        )
    };
    let water_index_buffer = if water_indices.is_empty() {
        None
    } else {
        Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("chunk_water_ib"),
                contents: bytemuck::cast_slice(water_indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
        )
    };
    Some(ChunkGpuMesh {
        vertex_buffer,
        index_buffer,
        num_vertices: vertices.len() as u32,
        num_indices: indices.len() as u32,
        dir_ranges,
        grass_instance_buffer,
        num_grass: n_grass,
        water_vertex_buffer,
        water_index_buffer,
        num_water,
    })
}

fn hash_u64_parts(parts: &[u64]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &p in parts {
        h ^= p;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Coarse LOD band for a chunk center (matches dirt/grass distance tiers).
fn chunk_distance_band(dist_sq: f32) -> u8 {
    use crate::world::{
        dirt_heightmap_max_dist_sq, dirt_hires_max_dist_sq, dirt_mesh_max_dist_sq,
        dirt_polished_max_dist_sq,
    };
    if dist_sq > dirt_mesh_max_dist_sq() {
        4
    } else if dist_sq >= dirt_heightmap_max_dist_sq() {
        3 // heightmap HLOD
    } else if dist_sq >= dirt_polished_max_dist_sq() {
        2 // greedy hard tones
    } else if dist_sq >= dirt_hires_max_dist_sq() {
        1 // greedy polished
    } else {
        0 // per-block hires
    }
}

fn chunk_state_key(camera: &Camera, cx: i32, cz: i32) -> u64 {
    use crate::world::chunk_dist_sq_xz;
    let origin = mesh_stream_origin(camera);
    let band = chunk_distance_band(chunk_dist_sq_xz(origin, cx, cz)) as u64;
    // Do not bake camera facing into the key — HD-2D orbit was remeshing the
    // whole bubble every few frames and leaving 16×16 holes.
    hash_u64_parts(&[cx as u64, cz as u64, band])
}

/// Per-section state key for the 16³ B-split (adds `cy` to the column key).
/// Camera-independent on purpose: NO facing is baked at build anymore (every
/// unoccluded face is emitted; GPU backface culling + draw-time direction
/// buckets hide backfaces per frame). Orbiting, rising or pulling the lens
/// therefore never invalidates meshes — only LOD band, grass density and
/// edits/streaming do. (An earlier `lens_sector` design requeued the bubble
/// on lens moves and caused remesh storms; measurement showed the bake saved
/// <1% of vertices.)
fn chunk_section_state_key(camera: &Camera, cx: i32, cy: i32, cz: i32) -> u64 {
    use crate::world::chunk_dist_sq_xz;
    let origin = mesh_stream_origin(camera);
    let band = chunk_distance_band(chunk_dist_sq_xz(origin, cx, cz)) as u64;
    hash_u64_parts(&[cx as u64, cy as u64, cz as u64, band])
}

fn grass_state_key(grass_origin: Vec3, cx: i32, cz: i32) -> u64 {
    use crate::world::{chunk_dist_sq_xz, grass_density_for_dist_sq};
    let dens = (grass_density_for_dist_sq(chunk_dist_sq_xz(grass_origin, cx, cz)) * 4.0) as u64;
    hash_u64_parts(&[cx as u64, cz as u64, dens])
}

fn grass_section_state_key(grass_origin: Vec3, cx: i32, cy: i32, cz: i32) -> u64 {
    use crate::world::{chunk_dist_sq_xz, grass_density_for_dist_sq};
    let dens = (grass_density_for_dist_sq(chunk_dist_sq_xz(grass_origin, cx, cz)) * 4.0) as u64;
    hash_u64_parts(&[cx as u64, cy as u64, cz as u64, dens])
}

/// Frustum cull around the camera; nearest chunks listed first.
fn cull_chunks(camera: &Camera, world: &mut World, candidates: &[(i32, i32)]) -> Vec<(i32, i32)> {
    use crate::world::MESH_CHUNK_SIZE;

    let origin = mesh_stream_origin(camera);
    let s = MESH_CHUNK_SIZE as f32;
    let mut out: Vec<(f32, i32, i32)> = candidates
        .iter()
        .copied()
        .filter(|&(cx, cz)| {
            let (h0, h1) = world.chunk_height_bounds(cx, cz);
            // Pad AABB so tall columns / steep skirts are not frustum-clipped early.
            let min = Vec3::new(cx as f32 * s - 1.0, 0.0, cz as f32 * s - 1.0);
            let max = Vec3::new(
                (cx as f32 + 1.0) * s + 1.0,
                (h1.max(h0) + 4) as f32,
                (cz as f32 + 1.0) * s + 1.0,
            );
            camera.aabb_visible(min, max)
        })
        .map(|(cx, cz)| {
            let d = crate::world::chunk_dist_sq_xz(origin, cx, cz);
            (d, cx, cz)
        })
        .collect();
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    out.into_iter().map(|(_, cx, cz)| (cx, cz)).collect()
}

/// Frustum cull per 16³ section with a tight 3D AABB (non-HD2D path).
/// Underground slabs outside the frustum never reach the rebuild queue.
fn cull_sections(
    camera: &Camera,
    world: &mut World,
    candidates: &[(i32, i32, i32)],
) -> Vec<(i32, i32, i32)> {
    use crate::world::{mesh_section_range, MESH_CHUNK_SIZE};

    let origin = mesh_stream_origin(camera);
    let s = MESH_CHUNK_SIZE as f32;
    let mut out: Vec<(f32, i32, i32, i32)> = candidates
        .iter()
        .copied()
        .filter(|&(cx, cy, cz)| {
            let (y0, y1) = mesh_section_range(cy);
            let min = Vec3::new(cx as f32 * s - 1.0, y0 as f32 - 1.0, cz as f32 * s - 1.0);
            let max = Vec3::new(
                (cx as f32 + 1.0) * s + 1.0,
                y1 as f32 + 1.0,
                (cz as f32 + 1.0) * s + 1.0,
            );
            // Touch the height cache so cull stays consistent with the column path.
            let _ = world.chunk_height_bounds(cx, cz);
            camera.aabb_visible(min, max)
        })
        .map(|(cx, cy, cz)| {
            let d = crate::world::chunk_dist_sq_xz(origin, cx, cz);
            (d, cx, cy, cz)
        })
        .collect();
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    out.into_iter().map(|(_, cx, cy, cz)| (cx, cy, cz)).collect()
}

/// GPU vertex/index caps — must stay under typical `max_buffer_size` (256 MiB).
/// Vertex is 56 bytes; leave headroom for the index buffer too.
const MAX_VERTICES: u64 = 4_194_304;
const MAX_INDICES: u64 = 6_291_456;
const MAX_GRASS_INSTANCES: u64 = 32_768;

/// List every wgpu adapter and pick one. Optional `MICROVERSE_GPU`:
/// index (`0`, `1`, …) or a name substring (`intel`, `amd`, `llvmpipe`).
fn pick_adapter(instance: &wgpu::Instance, surface: &wgpu::Surface<'_>) -> wgpu::Adapter {
    let adapters = instance.enumerate_adapters(wgpu::Backends::PRIMARY);
    if adapters.is_empty() {
        panic!("no se encontró ninguna GPU (Vulkan/DX12/Metal)");
    }
    for (i, adapter) in adapters.iter().enumerate() {
        let info = adapter.get_info();
        let ok = adapter.is_surface_supported(surface);
        log::info!(
            "GPU [{i}]: {} | {:?} | {:?} | driver={} | ventana={ok}",
            info.name,
            info.backend,
            info.device_type,
            info.driver
        );
        eprintln!(
            "GPU [{i}]: {} ({:?}, {:?}){}",
            info.name,
            info.backend,
            info.device_type,
            if ok {
                ""
            } else {
                " — no compatible con la ventana"
            }
        );
    }

    let idx = if let Ok(want) = std::env::var("MICROVERSE_GPU") {
        if let Ok(i) = want.parse::<usize>() {
            i.min(adapters.len().saturating_sub(1))
        } else {
            let want_l = want.to_lowercase();
            adapters
                .iter()
                .position(|a| a.get_info().name.to_lowercase().contains(&want_l))
                .unwrap_or(0)
        }
    } else {
        adapters
            .iter()
            .position(|a| {
                a.is_surface_supported(surface)
                    && a.get_info().device_type == wgpu::DeviceType::DiscreteGpu
            })
            .or_else(|| {
                adapters
                    .iter()
                    .position(|a| a.is_surface_supported(surface))
            })
            .unwrap_or(0)
    };

    let adapter = adapters.into_iter().nth(idx).expect("adapter index");
    let info = adapter.get_info();
    if !adapter.is_surface_supported(surface) {
        log::warn!(
            "GPU [{}] {} no declara soporte de superficie — se intenta igual",
            idx,
            info.name
        );
    }
    eprintln!("Usando GPU [{idx}]: {}", info.name);
    log::info!(
        "usando GPU [{idx}]: {} vendor={} device={:#x} {:?}",
        info.name,
        info.vendor,
        info.device,
        info.backend
    );
    adapter
}

/// Mesh to draw in the editor instead of the player.
///
/// The editor has no player, so the body pass was empty and the selection only
/// showed up as a coloured cell marker. This carries the selected entity's own
/// model so it is drawn with the *same* buffer, pipeline and occlusion rules as
/// the hero (CONVENTIONS: one occlusion rule, never a second one). The model
/// pointer is `&'static` because [`crate::entity_model::preview_model`] caches
/// and leaks one per path.
#[derive(Clone, Copy, Debug)]
pub struct EditorPreview {
    pub model: &'static crate::entity_model::EntityModel,
    /// Palette override for the active state (`None` = the mesh's own).
    /// `&'static` for the same reason as `model`: cached and leaked per name.
    pub palette: Option<&'static [[f32; 3]; crate::entity_model::PALETTE_LEN]>,
    /// World position of the model's foot soles.
    pub feet: Vec3,
    /// Yaw in radians, same convention as the hero (`+X` = 0).
    pub facing: f32,
    /// Height in blocks; the model is scaled so it spans this tall.
    pub body_height: f32,
}

impl Renderer {
    pub async fn new(window: Arc<Window>, world: &World, camera: &Camera) -> Self {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");
        let adapter = pick_adapter(&instance, &surface);

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("microverse_device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: Default::default(),
                },
                None,
            )
            .await
            .expect("request device");

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);

        // Fifo (v-sync): stable frame pacing; uncapped FPS exaggerates CPU hitches.
        let present_mode = wgpu::PresentMode::Fifo;
        log::info!("present_mode: {present_mode:?}");

        // En Android la superficie se capa a 1920 (ver `capped_surface_size`).
        let (cfg_w, cfg_h) = capped_surface_size(size.width, size.height);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: cfg_w.max(1),
            height: cfg_h.max(1),
            present_mode,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let (grass_tpl_verts, grass_tpl_indices) = grass_carpet_template();
        let grass_template_index_count = grass_tpl_indices.len() as u32;
        let grass_template_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grass_template_vb"),
            contents: bytemuck::cast_slice(&grass_tpl_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let grass_template_ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grass_template_ib"),
            contents: bytemuck::cast_slice(&grass_tpl_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // Warm per-section meshes and upload each section's GPU buffers independently.
        let mut chunk_meshes = std::collections::HashMap::new();
        let stream_origin = mesh_stream_origin(camera);
        let visible = world.chunk_sections_near(stream_origin, crate::world::dirt_mesh_max_dist());
        let grass_origin = grass_density_origin(camera);
        for &(cx, cy, cz) in &visible {
            let state_key = chunk_section_state_key(camera, cx, cy, cz);
            let grass_key = grass_section_state_key(grass_origin, cx, cy, cz);
            let (vertices, indices, grass, water_vertices, water_indices) =
                build_section_mesh(world, cx, cy, cz, camera.position, grass_origin);
            let gpu = upload_chunk_gpu(
                &device,
                &vertices,
                &indices,
                &grass,
                &water_vertices,
                &water_indices,
            );
            chunk_meshes.insert(
                (cx, cy, cz),
                ChunkMeshEntry {
                    state_key,
                    grass_key,
                    gpu,
                    last_used: 0,
                    hole_since: None,
                    empty_retries: 0,
                },
            );
        }
        let visible_chunks = visible;

        let frame_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("frame_buffer"),
            contents: bytemuck::bytes_of(&FrameUniform {
                view_proj: Mat4::IDENTITY.to_cols_array_2d(),
                light_view_proj: Mat4::IDENTITY.to_cols_array_2d(),
                camera_pos: [0.0; 3],
                time: 0.0,
                fog_color: FOG_RGB,
                fog_density: crate::world::FOG_DENSITY,
                focus_dist: 16.0,
                shadow_bias: 0.0018,
                surface_y: 20.0,
                eye_y: 20.0,
                focus_xz: [0.0, 0.0],
                confine: 0.0,
                indoors: 0.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let shadow_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow_map"),
            size: wgpu::Extent3d {
                // Android: mapa 1×1 (el shadow pass no corre; ver Pass 0).
                width: if cfg!(target_os = "android") {
                    1
                } else {
                    SHADOW_MAP_SIZE
                },
                height: if cfg!(target_os = "android") {
                    1
                } else {
                    SHADOW_MAP_SIZE
                },
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_texture.create_view(&wgpu::TextureViewDescriptor::default());
        // Android: sin shadow pass (ver Pass 0) — limpiar el mapa 1×1 una vez
        // a "todo iluminado" para que el PCF del shader lea luz.
        if cfg!(target_os = "android") {
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("shadow_clear_once"),
            });
            {
                let _pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("shadow_clear_once"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(
                        wgpu::RenderPassDepthStencilAttachment {
                            view: &shadow_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Clear(1.0),
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None,
                        },
                    ),
                    occlusion_query_set: None,
                    timestamp_writes: None,
                });
            }
            queue.submit(Some(enc.finish()));
        }
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow_sampler"),
            compare: Some(wgpu::CompareFunction::LessEqual),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let frame_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("frame_bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Depth,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                        count: None,
                    },
                ],
            });

        let frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame_bg"),
            layout: &frame_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&shadow_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&shadow_sampler),
                },
            ],
        });

        // Shadow pass needs uniforms but must NOT bind the shadow map (write conflict).
        let shadow_frame_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow_frame_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let shadow_frame_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow_frame_bg"),
            layout: &shadow_frame_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: frame_buffer.as_entire_binding(),
            }],
        });

        let shader_src = include_str!("shader.wgsl").replace(
            "{{ENABLE_OCCLUDED_BAYER}}",
            if crate::world::ENABLE_PLAYER_OCCLUDED_BAYER {
                "true"
            } else {
                "false"
            },
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shader"),
            source: wgpu::ShaderSource::Wgsl(shader_src.into()),
        });

        let (grass_bind_group_layout, grass_texture, grass_view, grass_sampler, grass_bind_group) =
            create_grass_texture_bind_group(&device, &queue);
        let (
            terrain_bind_group_layout,
            terrain_texture,
            terrain_view,
            terrain_sampler,
            terrain_bind_group,
        ) = create_terrain_texture_bind_group(&device, &queue);

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline_layout"),
            bind_group_layouts: &[&frame_bind_group_layout, &terrain_bind_group_layout],
            push_constant_ranges: &[],
        });
        let grass_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("grass_pipeline_layout"),
                bind_group_layouts: &[
                    &frame_bind_group_layout,
                    &terrain_bind_group_layout,
                    &grass_bind_group_layout,
                ],
                push_constant_ranges: &[],
            });
        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shadow_pipeline_layout"),
                bind_group_layouts: &[&shadow_frame_bgl],
                push_constant_ranges: &[],
            });
        let shadow_grass_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shadow_grass_pipeline_layout"),
                bind_group_layouts: &[
                    &shadow_frame_bgl,
                    &terrain_bind_group_layout,
                    &grass_bind_group_layout,
                ],
                push_constant_ranges: &[],
            });
        // Player stipple pass: no terrain sample — layout matches frame-only.
        let player_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("player_pipeline_layout"),
                bind_group_layouts: &[&frame_bind_group_layout],
                push_constant_ranges: &[],
            });

        let dirt_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x3,
                1 => Float32x3,
                2 => Float32x3,
                3 => Float32x2,
                4 => Float32,
                5 => Float32,
                6 => Float32,
            ],
        };
        let grass_instance_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<GrassInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                7 => Float32x3,
                8 => Float32x3,
                9 => Float32,
            ],
        };

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("render_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[dirt_vertex_layout.clone()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    // Alpha blend so buried exterior crop can fade as a gradient.
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: SCENE_DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Opaque hero: lit path, marks stencil=1 so the X-ray pass skips self.
        // Shared depth bias with the occluded pass — without it, Greater always
        // passes over the biased solid depth and the stencil is the only gate.
        let player_solid_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("player_solid_pipeline"),
                layout: Some(&player_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[dirt_vertex_layout.clone()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_player"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    // Outward CCW after ensure_outward_quad — Back cull is safe again.
                    cull_mode: Some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: SCENE_DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: stencil_mark_player(),
                    // No bias on solid — occluded pass uses a pull-in bias instead
                    // so Greater cannot self-hit the hero's own depth.
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // Occluded hero (ENABLE_PLAYER_SCREEN_DOOR): visible on top of trees/terrain
        // without see-through faces.
        // 1) Depth reset: Always + frag_depth=1.0 + no color — clears tree depth
        //    under the silhouette (stencil 0) so the next pass isn't depth-rejected.
        // 2) Color: LessEqual + depth write + Back cull — normal self-occlusion.
        let player_occluded_depth_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("player_occluded_depth"),
                layout: Some(&player_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[dirt_vertex_layout.clone()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_player_occluded_depth_reset"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::empty(),
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: SCENE_DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Always,
                    stencil: stencil_occluded_non_player(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });
        let player_occluded_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("player_occluded_pipeline"),
                layout: Some(&player_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[dirt_vertex_layout.clone()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_player_occluded"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: SCENE_DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: stencil_occluded_non_player(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        let ghost_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ghost_pipeline"),
            layout: Some(&player_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[dirt_vertex_layout.clone()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_ghost"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: SCENE_DEPTH_FORMAT,
                // X-ray: blueprint stays visible through dirt when the stair goes deep.
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // FP highlights: same tinted cubes as the ghost, but depth-tested
        // (LessEqual, no X-ray) so walls and floors occlude them properly.
        let highlight_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("highlight_pipeline"),
            layout: Some(&player_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[dirt_vertex_layout.clone()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_ghost"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: SCENE_DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Agua traslúcida real: mezcla alfa sobre lo opaco, sin escribir
        // profundidad (las capas se acumulan) y sin cull (visible al bucear).
        let water_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("water_pipeline"),
            layout: Some(&player_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[dirt_vertex_layout.clone()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_water"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: SCENE_DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Grieta de rotura: mismo cubo que el resaltado pero con su propio
        // alfa (~0.12, 90% más transparente) y sin X-ray.
        let crack_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("crack_pipeline"),
            layout: Some(&player_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[dirt_vertex_layout.clone()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_crack"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: SCENE_DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let grass_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("grass_pipeline"),
            layout: Some(&grass_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_grass"),
                buffers: &[dirt_vertex_layout.clone(), grass_instance_layout.clone()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_grass"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    // Blob contact shadows need soft alpha; blades use a=1.
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None, // crossed grass billboards visible from both sides
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: SCENE_DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow_pipeline"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_shadow"),
                buffers: &[dirt_vertex_layout.clone()],
                compilation_options: Default::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 1.5,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let shadow_grass_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("shadow_grass_pipeline"),
                layout: Some(&shadow_grass_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_shadow_grass"),
                    buffers: &[dirt_vertex_layout, grass_instance_layout],
                    compilation_options: Default::default(),
                },
                // Cutout shadows: chroma discard in fs (no color target).
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_shadow_grass"),
                    targets: &[],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState {
                        constant: 2,
                        slope_scale: 1.5,
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        let (scene_w, scene_h) = scene_internal_size(config.width, config.height);
        let depth_view = create_depth_view(&device, scene_w, scene_h);
        let (scene_texture, scene_view) =
            create_scene_target(&device, scene_w, scene_h, config.format);

        let blur_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blur_sampler"),
            // Linear so the HD-2D fisheye/blur upsamples the internal-res scene
            // to the swapchain in the same pass (upscale smoothing).
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let blur_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("blur_uniform"),
            contents: bytemuck::bytes_of(&BlurUniform {
                amount: 0.0,
                texel_x: 1.0 / scene_w.max(1) as f32,
                texel_y: 1.0 / scene_h.max(1) as f32,
                edge_blur: 0.0,
            }),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let blur_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("blur_bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let blur_bind_group = create_blur_bind_group(
            &device,
            &blur_bind_group_layout,
            &scene_view,
            &blur_sampler,
            &blur_uniform_buffer,
        );

        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blur_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blur.wgsl").into()),
        });

        let blur_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blur_pipeline_layout"),
            bind_group_layouts: &[&blur_bind_group_layout],
            push_constant_ranges: &[],
        });

        let blur_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blur_pipeline"),
            layout: Some(&blur_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &blur_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &blur_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Use available cores; [`FRAME_MESH_BUDGET_MS`] still caps hitch risk.
        // Android: tope 4 remeshes/frame — los núcleos pequeños se atragantan
        // con el greedy y el presupuesto de tiempo no basta contra el pico.
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let chunk_rebuilds_per_frame = if cfg!(target_os = "android") {
            4
        } else {
            cores.max(4)
        };
        log::info!("chunk rebuilds / frame: {chunk_rebuilds_per_frame} (cores={cores})");
        if crate::world::DEBUG_HERO_CAMERA_FACES {
            log::info!(
                "DEBUG_HERO_CAMERA_FACES: Bright=faces camera, dark=away. \
                 Dots 1=+X red 2=-X magenta 3=+Y yellow 4=-Y blue 5=+Z green 6=-Z cyan. \
                 Screen-door still on behind trees. Press F to flip winding."
            );
        }

        let player_ib = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("player_ib"),
            size: 256,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let (hud_pipeline, hud_bind_group, hud_texture, hud_sampler) =
            create_hud_pipeline(&device, &queue, config.format);

        Self {
            surface,
            device,
            queue,
            config,
            size,
            render_pipeline,
            player_solid_pipeline,
            player_occluded_depth_pipeline,
            player_occluded_pipeline,
            ghost_pipeline,
            highlight_pipeline,
            crack_pipeline,
            water_pipeline,
            grass_pipeline,
            shadow_pipeline,
            shadow_grass_pipeline,
            grass_template_vb,
            grass_template_ib,
            grass_template_index_count,
            _grass_texture: grass_texture,
            _grass_view: grass_view,
            _grass_sampler: grass_sampler,
            grass_bind_group,
            _terrain_texture: terrain_texture,
            _terrain_view: terrain_view,
            _terrain_sampler: terrain_sampler,
            terrain_bind_group,
            chunk_meshes,
            frame_index: 0,
            visible_chunks,
            chunk_rebuild_queue: rustc_hash::FxHashSet::default(),
            edit_priority: rustc_hash::FxHashSet::default(),
            chunk_rebuilds_per_frame,
            last_mesh_ms: 0.0,
            last_upload_ms: 0.0,
            last_rebuilt: 0,
            last_ring_holes: usize::MAX,
            cached_blur_amount: None,
            last_stream_origin: None,
            last_stream_chunk: None,
            frame_buffer,
            frame_bind_group,
            shadow_frame_bind_group,
            depth_view,
            shadow_texture,
            shadow_view,
            scene_texture,
            scene_view,
            scene_is_diorama: true,
            dyn_scale: SCENE_INTERNAL_SCALE,
            blur_pipeline,
            blur_bind_group_layout,
            blur_bind_group,
            blur_uniform_buffer,
            blur_sampler,
            start_time: std::time::Instant::now(),
            player_vb: None,
            player_ib,
            player_vb_cap: 0,
            player_ib_cap: 256,
            player_index_count: 0,
            player_body_index_count: 0,
            hero_winding_flip: false,
            ghost_vb: None,
            ghost_ib: None,
            ghost_vb_cap: 0,
            ghost_ib_cap: 0,
            ghost_index_count: 0,
            hl_vb: None,
            hl_ib: None,
            hl_vb_cap: 0,
            hl_ib_cap: 0,
            hl_index_count: 0,
            crack_vb: None,
            crack_ib: None,
            crack_vb_cap: 0,
            crack_ib_cap: 0,
            crack_index_count: 0,
            hud_pipeline,
            hud_bind_group,
            _hud_texture: hud_texture,
            _hud_sampler: hud_sampler,
            hud_vb: None,
            hud_ib: None,
            hud_vb_cap: 0,
            hud_ib_cap: 0,
            hud_index_count: 0,
        }
    }

    /// Toggle CW/CCW hero winding (debug). Returns the new state.
    pub fn toggle_hero_winding_flip(&mut self) -> bool {
        self.hero_winding_flip = !self.hero_winding_flip;
        self.hero_winding_flip
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        // En Android la superficie se capa (ver `capped_surface_size`):
        // `self.size` guarda la ventana real, `config` lo capado.
        self.size = new_size;
        let (cw, ch) = capped_surface_size(new_size.width, new_size.height);
        self.config.width = cw;
        self.config.height = ch;
        self.surface.configure(&self.device, &self.config);
        self.recreate_scene_targets();
    }

    /// Tamaño interno de escena con la escala dinámica (`dyn_scale`).
    fn internal_size_for(&self, w: u32, h: u32, diorama: bool) -> (u32, u32) {
        if diorama {
            (
                ((w as f32 * self.dyn_scale).round().max(1.0)) as u32,
                ((h as f32 * self.dyn_scale).round().max(1.0)) as u32,
            )
        } else if self.dyn_scale >= 1.0 {
            (w.max(1), h.max(1))
        } else {
            // FP con downscale (solo Android): misma escala, sin ventaja.
            (
                ((w as f32 * self.dyn_scale).round().max(1.0)) as u32,
                ((h as f32 * self.dyn_scale).round().max(1.0)) as u32,
            )
        }
    }

    /// Recrea depth + target de escena + blur group al tamaño interno actual.
    fn recreate_scene_targets(&mut self) {
        let diorama = self.scene_is_diorama;
        let (scene_w, scene_h) =
            self.internal_size_for(self.config.width, self.config.height, diorama);
        self.depth_view = create_depth_view(&self.device, scene_w, scene_h);
        let (scene_texture, scene_view) = create_scene_target(
            &self.device,
            scene_w,
            scene_h,
            self.config.format,
        );
        self.scene_texture = scene_texture;
        self.scene_view = scene_view;
        self.blur_bind_group = create_blur_bind_group(
            &self.device,
            &self.blur_bind_group_layout,
            &self.scene_view,
            &self.blur_sampler,
            &self.blur_uniform_buffer,
        );
        // Texel size changed — force blur uniform refresh next frame.
        self.cached_blur_amount = None;
    }

    /// Escala dinámica de escena (solo Android): baja peldaños si el fps
    /// cae bajo 45, los recupera sobre 57 (0.66↔0.55↔0.45). `App` la llama
    /// cada 60 frames. En PC no hace nada (siempre `SCENE_INTERNAL_SCALE`).
    pub fn auto_scene_scale(&mut self, fps: f32) {
        if !cfg!(target_os = "android") {
            return;
        }
        let next = dyn_scale_step(self.dyn_scale, fps);
        if (next - self.dyn_scale).abs() > 1e-4 {
            self.dyn_scale = next;
            self.recreate_scene_targets();
            log::info!("dyn_scale → {next:.2} (fps {fps:.0})");
        }
    }

    /// [A-ext] Recrea el target de escena solo al cruzar diorama↔FP.
    /// Durante el blend (0.45 s) el umbral 0.25 se cruza una vez: un solo
    /// recreate en la transición, no cada frame.
    fn ensure_scene_scale(&mut self, diorama: bool) {
        if self.scene_is_diorama == diorama {
            return;
        }
        self.scene_is_diorama = diorama;
        // Escala normalizada a 1.0: ambos modos usan el mismo tamaño, no hay
        // nada que recrear al cruzar el umbral (evita el hitch de transición).
        if self.dyn_scale >= 1.0 {
            return;
        }
        // Con downscale ambos modos comparten escala: recrear solo si el
        // tamaño realmente cambia.
        let (w, h) =
            self.internal_size_for(self.config.width, self.config.height, diorama);
        let cur = self.scene_texture.size();
        if cur.width == w && cur.height == h {
            return;
        }
        self.recreate_scene_targets();
    }

    pub fn size(&self) -> winit::dpi::PhysicalSize<u32> {
        self.size
    }

    /// Escala interna de escena efectiva (para el log de rendimiento).
    pub fn scene_scale(&self) -> f32 {
        self.dyn_scale
    }

    /// (secciones remalladas, ms de mallado, ms de upload a GPU del último frame).
    pub fn mesh_timings(&self) -> (usize, f32, f32) {
        (self.last_rebuilt, self.last_mesh_ms, self.last_upload_ms)
    }

    /// Huecos del anillo keep en el último frame (0 = mundo listo).
    pub fn ring_holes_open(&self) -> usize {
        self.last_ring_holes
    }

    /// Secciones esperando remallado (para el warn de slow frames).
    pub fn rebuild_queue_len(&self) -> usize {
        self.chunk_rebuild_queue.len()
    }

    /// GPU load of the last visible set — used to pull the HD-2D camera closer
    /// when the frustum is crowded (hides far LOD / streaming pop-in).
    pub fn pop_pressure(&self) -> f32 {
        let mut grass = 0u32;
        let mut indices = 0u32;
        for &(cx, cy, cz) in &self.visible_chunks {
            let Some(entry) = self.chunk_meshes.get(&(cx, cy, cz)) else {
                continue;
            };
            let Some(gpu) = entry.gpu.as_ref() else {
                continue;
            };
            grass = grass.saturating_add(gpu.num_grass);
            indices = indices.saturating_add(gpu.num_indices);
        }
        let chunks = self.visible_chunks.len() as f32 / crate::world::MESH_SECTIONS_Y as f32;
        let pending = self.chunk_rebuild_queue.len() as f32;
        // Soft targets: below → calm, above → pressure ramps to 1.
        let chunk_p = ((chunks - 20.0) / 50.0).clamp(0.0, 1.0);
        let grass_p = ((grass as f32 - 800.0) / 3500.0).clamp(0.0, 1.0);
        let mesh_p = ((indices as f32 - 40_000.0) / 160_000.0).clamp(0.0, 1.0);
        let pending_p = (pending / 12.0).clamp(0.0, 1.0);
        // Pending remesh weighs heaviest — that's when pop-in is most visible.
        (chunk_p * 0.25 + grass_p * 0.25 + mesh_p * 0.20 + pending_p * 0.55).clamp(0.0, 1.0)
    }

    #[allow(dead_code)]
    fn rebuild_chunk(&mut self, world: &World, camera: &Camera, cx: i32, cy: i32, cz: i32) {
        let origin = mesh_stream_origin(camera);
        let state_key = chunk_section_state_key(camera, cx, cy, cz);
        let grass_key = grass_section_state_key(origin, cx, cy, cz);
        let (vertices, indices, grass, water_vertices, water_indices) =
            if chunk_distance_band(crate::world::chunk_dist_sq_xz(origin, cx, cz)) >= 4 {
                (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new())
            } else {
                build_section_mesh(world, cx, cy, cz, camera.position, origin)
            };
        let gpu = upload_chunk_gpu(
            &self.device,
            &vertices,
            &indices,
            &grass,
            &water_vertices,
            &water_indices,
        );
        self.chunk_meshes.insert(
            (cx, cy, cz),
            ChunkMeshEntry {
                state_key,
                grass_key,
                gpu,
                last_used: self.frame_index,
                hole_since: None,
                empty_retries: 0,
            },
        );
    }

    /// Synchronously mesh every filled shunk inside the dirt mesh bubble.
    ///
    /// Call once after world preload + GPU init so the first playable frame already
    /// has terrain meshes (no progressive hole-fill at startup).
    pub fn warm_start_meshes(&mut self, world: &mut World, camera: &Camera) {
        use crate::world::{dirt_mesh_max_dist, MESH_SECTIONS_Y};
        use rayon::prelude::*;
        use std::time::Instant;

        let origin = mesh_stream_origin(camera);
        let grass_origin = grass_density_origin(camera);
        // Legacy column dirties (pre-section saves / tests) expand to all slabs.
        for (cx, cz) in world.take_dirty_chunks() {
            for cy in 0..MESH_SECTIONS_Y {
                self.chunk_rebuild_queue.insert((cx, cy, cz));
            }
        }
        for key in world.take_dirty_sections() {
            self.chunk_rebuild_queue.insert(key);
        }
        for key in world.take_dirty_sections_stream() {
            self.chunk_rebuild_queue.insert(key);
        }
        for &(cx, cy, cz) in &world.chunk_sections_near(origin, dirt_mesh_max_dist()) {
            if world.chunk_filled(cx, cz) {
                self.chunk_rebuild_queue.insert((cx, cy, cz));
            }
        }

        let mut ordered: Vec<(i32, i32, i32)> = self.chunk_rebuild_queue.drain().collect();
        ordered.sort_by_key(|&(cx, _, cz)| {
            crate::world::chunk_dist_sq_xz(origin, cx, cz).to_bits()
        });
        // Drop anything past the mesh LOD cut.
        ordered.retain(|&(cx, _, cz)| {
            world.chunk_filled(cx, cz)
                && chunk_distance_band(crate::world::chunk_dist_sq_xz(origin, cx, cz)) < 4
        });

        let n = ordered.len();
        log::info!("warm meshes: building {n} sections before first frame");
        let t0 = Instant::now();

        const BATCH: usize = 16;
        for chunk in ordered.chunks(BATCH) {
            let built: Vec<_> = chunk
                .par_iter()
                .map(|&(cx, cy, cz)| {
                    let (vertices, indices, grass, water_vertices, water_indices) =
                        build_section_mesh(world, cx, cy, cz, camera.position, origin);
                    (cx, cy, cz, vertices, indices, grass, water_vertices, water_indices)
                })
                .collect();
            for (cx, cy, cz, vertices, indices, grass, water_vertices, water_indices) in built {
                let gpu = upload_chunk_gpu(
                    &self.device,
                    &vertices,
                    &indices,
                    &grass,
                    &water_vertices,
                    &water_indices,
                );
                self.chunk_meshes.insert(
                    (cx, cy, cz),
                    ChunkMeshEntry {
                        state_key: chunk_section_state_key(camera, cx, cy, cz),
                        grass_key: grass_section_state_key(grass_origin, cx, cy, cz),
                        gpu,
                        last_used: self.frame_index,
                        hole_since: None,
                        empty_retries: 0,
                    },
                );
            }
        }

        log::info!(
            "warm meshes done in {:.0} ms — {} GPU sections ready",
            t0.elapsed().as_secs_f32() * 1000.0,
            self.chunk_meshes.len()
        );
    }

    /// Drop least-recently-used meshes that are no longer in the loaded world.
    /// Never evict meshes still in the frustum / keep ring (fallback until new Ready).
    fn evict_vram_cache(&mut self, world: &World) {
        let visible: rustc_hash::FxHashSet<(i32, i32, i32)> =
            self.visible_chunks.iter().copied().collect();
        while self.chunk_meshes.len() > MAX_VRAM_CHUNK_MESHES {
            let victim = self
                .chunk_meshes
                .iter()
                .filter(|(&(cx, cy, cz), _)| {
                    !visible.contains(&(cx, cy, cz)) && !world.chunk_filled(cx, cz)
                })
                .min_by_key(|(_, e)| e.last_used)
                .map(|(&k, _)| k)
                .or_else(|| {
                    // Second choice: unloaded + not visible (grace already elapsed).
                    self.chunk_meshes
                        .iter()
                        .filter(|(&(cx, cy, cz), _)| {
                            !visible.contains(&(cx, cy, cz)) && !world.has_chunk(cx, cz)
                        })
                        .min_by_key(|(_, e)| e.last_used)
                        .map(|(&k, _)| k)
                });
            let Some(key) = victim else {
                break;
            };
            self.chunk_meshes.remove(&key);
        }
    }

pub fn ensure_mesh_cached(&mut self, camera: &Camera, world: &mut World) {
    use crate::world::dirt_mesh_max_dist;
    use rayon::prelude::*;
    use rustc_hash::FxHashSet;
    use std::time::{Duration, Instant};

    self.frame_index = self.frame_index.wrapping_add(1);

    // Cheap origin/chunk key first — the streaming scan below (ring walk +
    // sort + rayon waves) is the most expensive part of a stationary frame,
    // so skip it entirely when nothing moved and nothing is dirty.
    let origin = mesh_stream_origin(camera);
    let c0 = crate::world::mesh_chunk_coord(origin.x.floor() as i32, origin.z.floor() as i32);
    // Frustum cull depends on facing in FP mode: only take the fast path when
    // the visible set is chunk-stable (diorama draws the whole bubble).
    let diorama =
        crate::world::ENABLE_HD2D && camera.hd2d_amount() >= 0.25
            || crate::world::DEBUG_DISABLE_CULLING;
    let has_edits = world.has_dirty_edits();
    let has_sections = world.has_dirty_sections();
    let has_stream = world.has_dirty_stream();
    let has_sections_stream = world.has_dirty_sections_stream();
    let has_pending = world.has_pending_chunks();
    let orbit_snap = camera.pending_orbit().is_some();
    let queues_empty =
        self.chunk_rebuild_queue.is_empty() && self.edit_priority.is_empty();
    let moved = match (self.last_stream_origin, self.last_stream_chunk) {
        (Some(prev), Some(pc)) => {
            pc != c0 || prev.distance_squared(origin) > 64.0 // >8 blocks
        }
        _ => true, // first frame: full path
    };
    if diorama
        && !has_edits
        && !has_sections
        && !has_stream
        && !has_sections_stream
        && !has_pending
        && !orbit_snap
        && !moved
        && queues_empty
    {
        // Stationary + clean: refresh LRU stamps, keep VRAM budget, return
        // WITHOUT the streaming ring scan or the rebuild pipeline.
        self.evict_vram_cache(world);
        for i in 0..self.visible_chunks.len() {
            let key = self.visible_chunks[i];
            if let Some(e) = self.chunk_meshes.get_mut(&key) {
                e.last_used = self.frame_index;
                e.hole_since = None;
            }
        }
        return;
    }

    // Stream around focus in HD-2D so lens confine/pull-in does not thrash chunks.
    // While a Q/E snap waits, bias streaming toward the pending view and give
    // it a bigger budget so the target-side shunks finish before the turn.
    let streaming_lock: Option<crate::camera::OrbitSnap> = camera.pending_orbit();
    let look = camera
        .pending_look()
        .unwrap_or_else(|| camera.forward());
    if crate::world::ENABLE_HD2D && streaming_lock.is_some() {
        world.stream_around_timed_look(
            origin,
            look,
            Duration::from_millis(ORBIT_PRELOAD_STREAM_BUDGET_MS),
        );
    } else {
        world.stream_around_look(origin, look);
    }
    // Keep unloaded chunk meshes in VRAM (LRU); only evict when over budget.
    self.evict_vram_cache(world);
    self.last_stream_origin = Some(origin);
    self.last_stream_chunk = Some(c0);

    // Player edits: keep GPU mesh until replacement uploads (never hole-punch).
    // Prioritize these so solid ghost meshes don't linger >1 frame near the dig.
    // Per-section dirties carry the precision (only the touched 16³ slab);
    // the legacy column sets are drained in parallel and discarded (mirrored).
    let _ = world.take_dirty_edits();
    let _ = world.take_dirty_stream();
    for (cx, cy, cz) in world.take_dirty_sections() {
        if let Some(entry) = self.chunk_meshes.get_mut(&(cx, cy, cz)) {
            entry.state_key = 0;
        }
        self.chunk_rebuild_queue.insert((cx, cy, cz));
        self.edit_priority.insert((cx, cy, cz));
    }
    // Streaming fills are soft: hot-gated below once `visible` is known.
    let stream_sections = world.take_dirty_sections_stream();

        let grass_origin = grass_density_origin(camera);
        let near = world.chunk_sections_near(origin, dirt_mesh_max_dist());
        // HD-2D FOV is 25° — frustum-culling dropped filled shunks that still
        // occupy the diorama (16×16 pits with exposed neighbor walls).
        // Frustum culling. Only once the lens is clearly first-person: the iso
        // diorama's 25° FOV + fisheye + edge ramp make the on-screen frustum much
        // wider than the math cone, so real culling there dropped filled shunks
        // that still station in the diorama (16×16 pits, exposed neighbor walls).
        // In FP the lens is a standard wide frustum — cull and reclaim the fills
        // behind/beside the hero (a big half of the 150→30 FPS cliff in caves).
        let diorama = crate::world::ENABLE_HD2D && camera.hd2d_amount() >= 0.25;
        let visible = if diorama || crate::world::DEBUG_DISABLE_CULLING {
            near.clone()
        } else {
            cull_sections(camera, world, &near)
        };
        let visible_set: FxHashSet<(i32, i32, i32)> = visible.iter().copied().collect();
        // Column visibility for the streaming hot-gate below.
        let visible_cols: FxHashSet<(i32, i32)> =
            visible.iter().map(|&(cx, _, cz)| (cx, cz)).collect();

        // Streaming dirties: queue rebuild but KEEP the old GPU mesh until the new
        // one uploads — removing first punched 16×16 holes (wireframe neighbor walls).
        // Hot-gate on the column: only sections near the visible set get queued.
        for (cx, cy, cz) in stream_sections {
            let hot = (-1..=1)
                .any(|dx| (-1..=1).any(|dz| visible_cols.contains(&(cx + dx, cz + dz))));
            if let Some(entry) = self.chunk_meshes.get_mut(&(cx, cy, cz)) {
                entry.state_key = 0; // invalidate so LOD can't look "clean" while queued
            }
            if hot {
                self.chunk_rebuild_queue.insert((cx, cy, cz));
            }
        }

        for &(cx, cy, cz) in &visible {
            // Reserved-but-empty chunks must not draw a stale VRAM mesh.
            if !world.chunk_filled(cx, cz) {
                continue;
            }
            let want = chunk_section_state_key(camera, cx, cy, cz);
            let want_grass = grass_section_state_key(grass_origin, cx, cy, cz);
            // Fresh-but-empty slabs (no GPU, keys current, no hole flag) are
            // clean — only placeholders (hole_since set) or stale keys rebuild.
            let dirty = match self.chunk_meshes.get(&(cx, cy, cz)) {
                Some(e) => {
                    e.hole_since.is_some() || e.state_key != want || e.grass_key != want_grass
                }
                None => true,
            };
            if dirty {
                let entry = self.chunk_meshes.entry((cx, cy, cz)).or_insert(ChunkMeshEntry {
                    state_key: 0,
                    grass_key: 0,
                    gpu: None,
                    last_used: self.frame_index,
                    hole_since: Some(self.frame_index),
                    empty_retries: 0,
                });
                if entry.hole_since.is_none() {
                    entry.hole_since = Some(self.frame_index);
                }
                self.chunk_rebuild_queue.insert((cx, cy, cz));
            } else if let Some(e) = self.chunk_meshes.get_mut(&(cx, cy, cz)) {
                e.last_used = self.frame_index;
                e.hole_since = None;
            }
        }

        // Chunk Guardian: force mesh-ring holes + long-missing GPU to the front.
        // Ring holes are columns; expand to the sections that may have content.
        let ring_holes = world.mesh_ring_holes(origin);
        self.last_ring_holes = ring_holes.len();
        let mesh_ring_has_holes = !ring_holes.is_empty();
        for (cx, cz) in &ring_holes {
            if world.chunk_filled(*cx, *cz) {
                for cy in 0..crate::world::MESH_SECTIONS_Y {
                    if !crate::world::DEBUG_DISABLE_CULLING
                        && !world.section_may_have_content(*cx, *cz, cy)
                    {
                        continue;
                    }
                    self.chunk_rebuild_queue.insert((*cx, cy, *cz));
                    self.edit_priority.insert((*cx, cy, *cz));
                }
            }
        }
        for &(cx, cy, cz) in &visible {
            if !world.chunk_filled(cx, cz) {
                continue;
            }
            let stuck = self.chunk_meshes.get(&(cx, cy, cz)).and_then(|e| {
                if e.gpu.is_some() {
                    None
                } else {
                    e.hole_since
                }
            });
            if let Some(since) = stuck {
                if self.frame_index.saturating_sub(since) >= 2 {
                    self.chunk_rebuild_queue.insert((cx, cy, cz));
                    self.edit_priority.insert((cx, cy, cz));
                }
            }
        }

        // Rebuild frustum sections first (nearest last for pop), drop off-screen leftovers.
        // Prefer: player edits / guardian holes → missing GPU mesh → LOD upgrades.
        let mut ordered: Vec<(i32, i32, i32)> = self
            .chunk_rebuild_queue
            .iter()
            .copied()
            .filter(|k| visible_set.contains(k) || self.edit_priority.contains(k))
            .collect();
        ordered.sort_by_key(|&(cx, cy, cz)| {
            let d = crate::world::chunk_dist_sq_xz(origin, cx, cz);
            let missing = self
                .chunk_meshes
                .get(&(cx, cy, cz))
                .is_none_or(|e| e.hole_since.is_some());
            let edit_pri = if self.edit_priority.contains(&(cx, cy, cz)) {
                0i64
            } else {
                1
            };
            let hole_pri = if missing { 0i64 } else { 1 };
            -((edit_pri << 41) + (hole_pri << 40) + (d * 1000.0) as i64)
        });
        self.chunk_rebuild_queue.clear();

        let mesh_budget_ms = if crate::world::ENABLE_HD2D {
            // Extra ms while dig edits or guardian holes are pending.
            if self.edit_priority.is_empty() && !mesh_ring_has_holes {
                FRAME_MESH_BUDGET_MS + 2
            } else {
                FRAME_MESH_BUDGET_MS + 6
            }
        } else if mesh_ring_has_holes {
            FRAME_MESH_BUDGET_MS + 3
        } else {
            FRAME_MESH_BUDGET_MS
        };
        // Q/E snap waiting on preload: spend more millis to close the bubble
        // faster so the deferred rotation happens sooner.
        let mesh_budget_ms = if camera.pending_orbit().is_some() {
            mesh_budget_ms + ORBIT_PRELOAD_MESH_BUDGET_MS
        } else {
            mesh_budget_ms
        };
        let mesh_budget = Duration::from_millis(mesh_budget_ms);
        let mesh_start = Instant::now();
        let mut rebuilt_count = 0usize;
        let batch_cap = self.chunk_rebuilds_per_frame.min(16);
        self.last_mesh_ms = 0.0;
        self.last_upload_ms = 0.0;

        let missing_gpu =
            |meshes: &std::collections::HashMap<(i32, i32, i32), ChunkMeshEntry>,
             cx: i32,
             cy: i32,
             cz: i32| {
                meshes
                    .get(&(cx, cy, cz))
                    .is_none_or(|e| e.hole_since.is_some())
            };

        loop {
            let forcing = ordered.iter().any(|&(cx, cy, cz)| {
                world.chunk_filled(cx, cz)
                    && chunk_distance_band(crate::world::chunk_dist_sq_xz(origin, cx, cz)) < 4
                    && missing_gpu(&self.chunk_meshes, cx, cy, cz)
            });
            if rebuilt_count > 0 && mesh_start.elapsed() >= mesh_budget && !forcing {
                break;
            }
            if !forcing && rebuilt_count >= self.chunk_rebuilds_per_frame {
                break;
            }

            let mut batch = Vec::new();
            let mut deferred = Vec::new();
            let limit = if forcing { 16 } else { batch_cap };
            while batch.len() < limit {
                if !forcing && rebuilt_count + batch.len() >= self.chunk_rebuilds_per_frame {
                    break;
                }
                let Some((cx, cy, cz)) = ordered.pop() else {
                    break;
                };
                if !world.chunk_filled(cx, cz) {
                    continue;
                }
                // Empty upper slabs never enter the queue, but edits above the
                // treetops (or unloaded extras) can still land here — skip.
                // (Diagnostic `DEBUG_DISABLE_CULLING` bypasses the skip.)
                if !crate::world::DEBUG_DISABLE_CULLING
                    && !world.section_may_have_content(cx, cz, cy)
                {
                    continue;
                }
                // Beyond mesh distance: keep any prior mesh, do not rebuild to empty.
                if chunk_distance_band(crate::world::chunk_dist_sq_xz(origin, cx, cz)) >= 4 {
                    continue;
                }
                // Neighbor gate: wait on Reserved neighbors ONLY when we already have
                // a GPU mesh (avoid remesh flicker). First mesh must not wait — a
                // Filled chunk with no mesh + Reserved neighbor was a permanent 16×16 hole.
                let has_gpu = self
                    .chunk_meshes
                    .get(&(cx, cy, cz))
                    .is_some_and(|e| e.gpu.is_some());
                if has_gpu && !world.neighbors_ready_for_mesh(origin, cx, cz) {
                    deferred.push((cx, cy, cz));
                    continue;
                }
                let want = chunk_section_state_key(camera, cx, cy, cz);
                let want_grass = grass_section_state_key(grass_origin, cx, cy, cz);
                if self.chunk_meshes.get(&(cx, cy, cz)).is_some_and(|e| {
                    e.gpu.is_some() && e.state_key == want && e.grass_key == want_grass
                }) {
                    continue;
                }
                batch.push((cx, cy, cz));
            }
            // Re-queue chunks waiting on neighbors.
            ordered.extend(deferred);
            if batch.is_empty() {
                break;
            }

            let m0 = Instant::now();
            let rebuilt: Vec<_> = batch
                .par_iter()
                .map(|&(cx, cy, cz)| {
                    let state_key =
                        chunk_section_state_key(camera, cx, cy, cz);
                    let grass_key = grass_section_state_key(grass_origin, cx, cy, cz);
                    let (vertices, indices, grass, water_vertices, water_indices) =
                        build_section_mesh(world, cx, cy, cz, camera.position, grass_origin);
                    (
                        (cx, cy, cz),
                        state_key,
                        grass_key,
                        vertices,
                        indices,
                        grass,
                        water_vertices,
                        water_indices,
                    )
                })
                .collect();
            self.last_mesh_ms += m0.elapsed().as_secs_f32() * 1000.0;

            let u0 = Instant::now();
            for (key, state_key, grass_key, vertices, indices, grass, water_vertices, water_indices) in rebuilt {
                let gpu = upload_chunk_gpu(
                    &self.device,
                    &vertices,
                    &indices,
                    &grass,
                    &water_vertices,
                    &water_indices,
                );
                if gpu.is_none() {
                    // Empty slab (e.g. upper air after an edit). Sealing it as
                    // "clean" makes the Chunk Guardian stop retrying; a section
                    // that still reads as possibly-contentful must retry a few
                    // frames first, or a streaming fill / spillover racing this
                    // mesh pass seals a permanent 16×16 corner hole.
                    let retries = self
                        .chunk_meshes
                        .get(&key)
                        .map(|e| e.empty_retries)
                        .unwrap_or(0);
                    let may_have = world.section_may_have_content(key.0, key.2, key.1);
                    if may_have && retries < EMPTY_MESH_RETRY_LIMIT {
                        self.chunk_meshes.insert(
                            key,
                            ChunkMeshEntry {
                                state_key,
                                grass_key,
                                gpu: None,
                                last_used: self.frame_index,
                                hole_since: Some(self.frame_index),
                                empty_retries: retries + 1,
                            },
                        );
                        self.edit_priority.insert(key);
                        rebuilt_count += 1;
                        continue;
                    }
                    // Keep any previous mesh entry untouched otherwise.
                    let had_entry = self.chunk_meshes.contains_key(&key);
                    if !had_entry {
                        self.chunk_meshes.insert(
                            key,
                            ChunkMeshEntry {
                                state_key,
                                grass_key,
                                gpu: None,
                                last_used: self.frame_index,
                                hole_since: None,
                                empty_retries: 0,
                            },
                        );
                    } else if let Some(entry) = self.chunk_meshes.get_mut(&key) {
                        entry.last_used = self.frame_index;
                    }
                    self.edit_priority.remove(&key);
                    rebuilt_count += 1;
                    continue;
                }
                self.edit_priority.remove(&key);
                self.chunk_meshes.insert(
                    key,
                    ChunkMeshEntry {
                        state_key,
                        grass_key,
                        gpu,
                        last_used: self.frame_index,
                        hole_since: None,
                        empty_retries: 0,
                    },
                );
                rebuilt_count += 1;
            }
            self.last_upload_ms += u0.elapsed().as_secs_f32() * 1000.0;
            // Ni siquiera forzando se drena sin límite: 32 secciones/frame
            // como máximo. Los huecos restantes esperan al siguiente frame
            // (el guardián ya tolera huecos breves vía `hole_since`); sin
            // este tope, arrancar con el anillo vacío metía decenas de
            // segundos en UN frame porque `forcing` ignoraba presupuesto y
            // tope de cantidad.
            if rebuilt_count >= 32 {
                break;
            }
        }
        self.last_rebuilt = rebuilt_count;

        // Leftover dirty *visible* chunks wait for a later frame.
        self.chunk_rebuild_queue.extend(ordered);

        self.visible_chunks = visible;
        self.evict_vram_cache(world);
    }

    /// True when every shunk inside the dirt mesh bubble is generated AND every
    /// content-bearing section is settled (has a GPU mesh, or the mesh pass has
    /// proven it genuinely empty). The HD-2D Q/E snap gates on this before
    /// rotating, so the turn never exposes a 16×16 hole.
    pub fn orbit_ready(&self, world: &World, camera: &Camera) -> bool {
        use crate::world::{DEBUG_DISABLE_CULLING, dirt_mesh_max_dist};

        if DEBUG_DISABLE_CULLING {
            return true;
        }
        let origin = mesh_stream_origin(camera);
        let radius = dirt_mesh_max_dist();
        // Every shunk must be Filled — an Empty/Reserved bucket anywhere in the
        // bubble would pop in as wireframe walls on the turn.
        for (cx, cz) in world.chunk_coords_within(origin, radius) {
            if !world.chunk_filled(cx, cz) {
                return false;
            }
        }
        // Every section the mesh pass could queue (same candidate list as
        // `chunk_sections_near`) must be settled. A "settled" section either
        // has a GPU mesh, or was meshed and proven empty (sealed clean), so a
        // legitimately-empty slab never blocks the gate forever.
        for (cx, cy, cz) in world.chunk_sections_near(origin, radius) {
            let settled = self
                .chunk_meshes
                .get(&(cx, cy, cz))
                .is_some_and(|e| e.gpu.is_some() || {
                    e.hole_since.is_none() && e.empty_retries >= EMPTY_MESH_RETRY_LIMIT
                });
            if !settled {
                return false;
            }
        }
        true
    }

    fn draw_visible_dirt<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        shadow: bool,
        world: &World,
        camera_pos: Vec3,
    ) {
        if shadow {
            pass.set_pipeline(&self.shadow_pipeline);
            pass.set_bind_group(0, &self.shadow_frame_bind_group, &[]);
        } else {
            pass.set_pipeline(&self.render_pipeline);
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            pass.set_bind_group(1, &self.terrain_bind_group, &[]);
        }
        for &(cx, cy, cz) in &self.visible_chunks {
            if !world.chunk_filled(cx, cz) {
                continue;
            }
            let Some(entry) = self.chunk_meshes.get(&(cx, cy, cz)) else {
                if crate::world::DEBUG_CHUNK_STREAM && self.frame_index % 45 == 0 {
                    log::warn!(
                        "skip mesh ({cx},{cy},{cz}) state={:?} — no ChunkMeshEntry",
                        world.chunk_state(cx, cz)
                    );
                }
                continue;
            };
            let Some(gpu) = entry.gpu.as_ref() else {
                if crate::world::DEBUG_CHUNK_STREAM && self.frame_index % 45 == 0 {
                    log::warn!(
                        "skip mesh ({cx},{cy},{cz}) state={:?} — gpu=None (hole_since={:?})",
                        world.chunk_state(cx, cz),
                        entry.hole_since
                    );
                }
                continue;
            };
            if gpu.num_indices == 0 {
                continue;
            }
            pass.set_vertex_buffer(0, gpu.vertex_buffer.slice(..));
            pass.set_index_buffer(gpu.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            if shadow {
                // Light comes from its own direction — draw every bucket.
                pass.draw_indexed(0..gpu.num_indices, 0, 0..1);
                continue;
            }
            // Front-only: backfacing buckets never reach the rasterizer.
            // (Diagnostic `DEBUG_DISABLE_CULLING` draws every bucket.)
            let mask = if crate::world::DEBUG_DISABLE_CULLING {
                0x3F
            } else {
                section_face_mask(camera_pos, cx, cy, cz)
            };
            for b in 0..6 {
                if mask & (1 << b) == 0 {
                    continue;
                }
                let (start, count) = gpu.dir_ranges[b];
                if count == 0 {
                    continue;
                }
                pass.draw_indexed(start..start + count, 0, 0..1);
            }
        }
    }

    fn draw_visible_grass<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        shadow: bool,
        world: &World,
    ) {
        // Grass uses cheap elliptical contact blobs instead of shadow-map casting.
        if shadow {
            return;
        }
        pass.set_pipeline(&self.grass_pipeline);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_bind_group(1, &self.terrain_bind_group, &[]);
        pass.set_bind_group(2, &self.grass_bind_group, &[]);
        pass.set_vertex_buffer(0, self.grass_template_vb.slice(..));
        pass.set_index_buffer(self.grass_template_ib.slice(..), wgpu::IndexFormat::Uint32);
        for &(cx, cy, cz) in &self.visible_chunks {
            if !world.chunk_filled(cx, cz) {
                continue;
            }
            let Some(entry) = self.chunk_meshes.get(&(cx, cy, cz)) else {
                continue;
            };
            let Some(gpu) = entry.gpu.as_ref() else {
                continue;
            };
            let Some(grass_buf) = gpu.grass_instance_buffer.as_ref() else {
                continue;
            };
            if gpu.num_grass == 0 {
                continue;
            }
            pass.set_vertex_buffer(1, grass_buf.slice(..));
            pass.draw_indexed(0..self.grass_template_index_count, 0, 0..gpu.num_grass);
        }
    }

    /// Sums uploaded chunk meshes — no rebuild.
    #[allow(dead_code)]
    pub fn mesh_within_gpu_budget(&self) -> bool {
        let (verts, idx) = self
            .chunk_meshes
            .values()
            .filter_map(|e| e.gpu.as_ref())
            .fold((0u64, 0u64), |(v, i), g| {
                (v + g.num_vertices as u64, i + g.num_indices as u64)
            });
        let ok = verts <= MAX_VERTICES && idx <= MAX_INDICES;
        if !ok {
            log::warn!(
                "mesh over GPU budget: verts {}/{} indices {}/{}",
                verts,
                MAX_VERTICES,
                idx,
                MAX_INDICES
            );
        }
        ok
    }

    pub fn render(
        &mut self,
        camera: &Camera,
        world: &mut World,
        player_feet: Option<Vec3>,
        player_facing: f32,
        player_pose: &crate::hero_pose::HeroPose,
        tool_id: Option<crate::items::ToolId>,
        tool_swing: f32,
        equip_blend: f32,
        ghost_cells: &[(glam::IVec3, [f32; 3])],
        highlight_cells: &[(glam::IVec3, [f32; 3])],
        stood_disc: Option<(Vec3, [f32; 3])>,
        crack_cells: &[(glam::IVec3, [f32; 3])],
        // Editor: mesh of the selected entity. `None` in game and in the menu.
        preview: Option<EditorPreview>,
        hud_mesh: Option<&crate::hud::HudMesh>,
    ) -> Result<(), wgpu::SurfaceError> {
        use crate::world::{blur_amount_for_distance, measure_shunk_distance, ENABLE_HD2D};

        // [A-ext] FP nativo / diorama 2/3: conmutar el target al cruzar el
        // mismo umbral que el culling (evita recrear cada frame del blend).
        self.ensure_scene_scale(is_diorama_view(camera));

        self.ensure_mesh_cached(camera, world);

        if ENABLE_HD2D {
            if let Some(feet) = player_feet {
                // 1ª persona: solo piernas/pies (1/3) para que al mirar abajo no
                // muestre torso/cabeza. Umbral hd2d_amount<0.5 evita el pop
                // durante el blend diorama→ojo.
                let fp_legs_only = camera.hd2d_amount() < 0.5;
                // Línea de flotación (mundo Y) — lo que queda por debajo se
                // oscurece como mojada.
                let wet_line = Self::water_surface_above(world, feet);
                self.upload_player_hero(
                    feet,
                    player_facing,
                    camera.position,
                    player_pose,
                    tool_id,
                    tool_swing,
                    equip_blend,
                    fp_legs_only,
                    wet_line,
                    None,
                );
            } else if let Some(p) = preview {
                // Editor: no player, but the selection has a mesh. It reuses the
                // body buffer so it gets the hero's occlusion for free.
                self.upload_player_hero(
                    p.feet,
                    p.facing,
                    camera.position,
                    player_pose,
                    None,
                    0.0,
                    0.0,
                    false,
                    None,
                    Some(p),
                );
            } else {
                self.player_vb = None;
            }
        } else {
            self.player_vb = None;
        }
        self.upload_ghost_cells(ghost_cells);
        self.upload_highlight_cells(highlight_cells, stood_disc);
        self.upload_crack_cells(crack_cells);
        self.upload_hud_mesh(hud_mesh);

        let focus_dist = player_feet
            .map(|f| (camera.position - (f + Vec3::Y)).length())
            .unwrap_or(16.0);

        let (mut fog_rgb, mut surface_y, mut eye_y, mut focus_xz) = scene_altitude(camera, world);
        // T1 cutaway-dig ELIMINADO: sin agujero en la tapa, sin pull hacia el
        // lens. focus_xz = pies del jugador (T2 indoor lo usa como centro).
        if ENABLE_HD2D {
            if let Some(feet) = player_feet {
                let gx = feet.x.floor() as i32;
                let gz = feet.z.floor() as i32;
                surface_y = crate::world::terrain_height(gx, gz) as f32;
                eye_y = feet.y + 0.35; // just above soles
                                       // Clear/fog must use the corrected underground eye — otherwise the
                                       // meadow green void shows through distant cave holes.
                fog_rgb = crate::world::fog_color_for_biome(
                    eye_y,
                    surface_y,
                    crate::biomes::biome_at(gx, gz),
                );
                focus_xz = [feet.x, feet.z];
            }
        } else if let Some(feet) = player_feet {
            focus_xz = [feet.x, feet.z];
            eye_y = feet.y + 0.35;
            let gx = feet.x.floor() as i32;
            let gz = feet.z.floor() as i32;
            fog_rgb = crate::world::fog_color_for_biome(
                eye_y,
                surface_y,
                crate::biomes::biome_at(gx, gz),
            );
        };
        // T1 borrado → confine del shader siempre 0 (el shader ignora la tapa).
        // T2 indoor se mantiene independiente de ENABLE_DIG_CUTAWAY, pero solo
        // en diorama: en 1ª persona la habitación se ve al natural y disolver
        // las paredes que miras es el bug reportado dentro de casas.
        let shader_confine = 0.0;
        // Same cutaway pipeline for house interiors (camera-facing walls/roof).
        let indoors = if camera.hd2d_amount() < 0.5 {
            0.0
        } else if let Some(feet) = player_feet {
            world.indoors_factor(feet + Vec3::Y * 1.1)
        } else {
            0.0
        };
        let time = self.start_time.elapsed().as_secs_f32();
        // Air density follows the eye biome (mist closes in, deserts read far).
        let eye_biome = player_feet.map(|feet| {
            crate::biomes::biome_at(feet.x.floor() as i32, feet.z.floor() as i32)
        });
        // Diagnostic `DEBUG_DISABLE_CULLING`: no shader discard or cave mask —
        // eye faked at the surface so burial/cutaway terms stay zero.
        let (shader_confine, indoors, eye_y) = if crate::world::DEBUG_DISABLE_CULLING {
            (0.0, 0.0, surface_y)
        } else {
            (shader_confine, indoors, eye_y)
        };
        let uniform = FrameUniform {
            view_proj: camera.view_proj().to_cols_array_2d(),
            light_view_proj: light_view_proj(camera.position).to_cols_array_2d(),
            camera_pos: camera.position.to_array(),
            time,
            fog_color: fog_rgb,
            fog_density: eye_biome
                .unwrap_or(crate::biomes::BiomeId::TemperateMeadow)
                .fog_density(),
            focus_dist,
            shadow_bias: 0.0018,
            surface_y,
            eye_y,
            focus_xz,
            confine: shader_confine,
            indoors,
        };
        self.queue
            .write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&uniform));

        let dist = measure_shunk_distance(camera.position).to_aabb;
        // HD-2D: light CoC + constant fisheye edge soft; FPS: distance ramp only.
        // [A-ext] FP nítido: sin edge fisheye fuera del diorama.
        let diorama_view = is_diorama_view(camera);
        let (blur_amount, edge_blur) = if ENABLE_HD2D && diorama_view {
            let coc = ((dist - focus_dist).abs() / focus_dist.max(8.0)).clamp(0.0, 1.0);
            (
                coc * crate::world::BLUR_MAX_AMOUNT * 0.45,
                crate::world::HD2D_EDGE_BLUR,
            )
        } else if ENABLE_HD2D {
            let coc = ((dist - focus_dist).abs() / focus_dist.max(8.0)).clamp(0.0, 1.0);
            (
                coc * crate::world::BLUR_MAX_AMOUNT * 0.45,
                0.0,
            )
        } else {
            (blur_amount_for_distance(dist), 0.0)
        };
        // Android: un cuarto de bokeh — la mayoría de píxeles caen en el
        // early-out de 1 tap y casi ninguno llega al 9-tap (ver blur.wgsl).
        let (blur_amount, edge_blur) = if cfg!(target_os = "android") {
            (blur_amount * 0.25, edge_blur * 0.25)
        } else {
            (blur_amount, edge_blur)
        };
        // Blur texel must match the internal scene target dimensions
        // (dinámico en Android vía `dyn_scale`; ver `recreate_scene_targets`).
        let (scene_w, scene_h) = self.internal_size_for(
            self.config.width.max(1),
            self.config.height.max(1),
            self.scene_is_diorama,
        );
        let blur_changed = self
            .cached_blur_amount
            .is_none_or(|prev| (prev - blur_amount).abs() > 0.001);
        if blur_changed || self.cached_blur_amount.is_none() {
            let blur_uniform = BlurUniform {
                amount: blur_amount,
                texel_x: 1.0 / scene_w.max(1) as f32,
                texel_y: 1.0 / scene_h.max(1) as f32,
                edge_blur,
            };
            self.queue.write_buffer(
                &self.blur_uniform_buffer,
                0,
                bytemuck::bytes_of(&blur_uniform),
            );
            self.cached_blur_amount = Some(blur_amount);
        }

        let output = self.surface.get_current_texture()?;
        let frame_view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("encoder"),
            });

        // Pass 0: shadow map.
        // Android: omitido — el mapa es 1×1 limpiado a plena luz en init,
        // así el PCF lee "iluminado" sin dibujar nada (se ahorra todo el
        // pase de vértices+fill de tierra/hierba/héroe). En PC normal.
        if !cfg!(target_os = "android") {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow_pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            self.draw_visible_dirt(&mut pass, true, world, camera.position);
            self.draw_visible_grass(&mut pass, true, world);
            self.draw_player_billboard(&mut pass, true);
        }

        // Pass 1: scene → offscreen.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.scene_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: fog_rgb[0] as f64,
                            g: fog_rgb[1] as f64,
                            b: fog_rgb[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0),
                        store: wgpu::StoreOp::Store,
                    }),
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            self.draw_visible_dirt(&mut pass, false, world, camera.position);
            // Solid first (marks stencil=1 where the hero wins depth). Then occluded:
            // Always + stencil==0 draws the full silhouette *in front of* trees and
            // applies Bayer screen-door transparency. Open air stays clean via stencil.
            // Grass after hero so blades don't punch holes into the body.
            // En 1ª persona no hay X-ray: la cámara está en el ojo y el stencil
            // del propio cuerpo generaría el bug de silueta fantasma.
            self.draw_player_billboard(&mut pass, false);
            if crate::world::ENABLE_PLAYER_SCREEN_DOOR && camera.hd2d_amount() >= 0.5 {
                self.draw_player_occluded(&mut pass);
            }
            self.draw_visible_grass(&mut pass, false, world);
            // Agua traslúcida tras lo opaco (héroe incluido): tinta suave,
            // sin tramado sobre el personaje.
            self.draw_visible_water(&mut pass, world);
            // Stair ghost last: X-ray yellow through terrain (depth Always).
            self.draw_ghost(&mut pass);
            // FP highlights last of all: depth-tested shells, no X-ray.
            self.draw_highlight(&mut pass);
            // Grieta de rotura encima de todo lo anterior (velo sutil).
            self.draw_crack(&mut pass);
        }

        // Pass 2: blur → swapchain.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blur_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.blur_pipeline);
            pass.set_bind_group(0, &self.blur_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        // Pass 3: on-screen HUD over the presented frame.
        if self.hud_index_count > 0 {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hud_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.hud_pipeline);
            pass.set_bind_group(0, &self.hud_bind_group, &[]);
            if let (Some(vb), Some(ib)) = (self.hud_vb.as_ref(), self.hud_ib.as_ref()) {
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..self.hud_index_count, 0, 0..1);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }

    /// Presenta SOLO el HUD sobre un fondo plano, sin tocar el mundo: es la
    /// ruta del menú y del editor. Nada de `ensure_mesh_cached` → nada de
    /// streaming, así el mundo sigue sin generarse hasta que se elige
    /// partida (y entonces los `player_edits` de la partida guardada se
    /// aplican al generar, no hace falta reescribir voxels vivos).
    pub fn render_ui_only(
        &mut self,
        hud_mesh: Option<&crate::hud::HudMesh>,
    ) -> Result<(), wgpu::SurfaceError> {
        self.upload_hud_mesh(hud_mesh);

        let output = self.surface.get_current_texture()?;
        let frame_view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ui_encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &frame_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.03,
                            g: 0.04,
                            b: 0.07,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            if self.hud_index_count > 0 {
                pass.set_pipeline(&self.hud_pipeline);
                pass.set_bind_group(0, &self.hud_bind_group, &[]);
                if let (Some(vb), Some(ib)) = (self.hud_vb.as_ref(), self.hud_ib.as_ref()) {
                    pass.set_vertex_buffer(0, vb.slice(..));
                    pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..self.hud_index_count, 0, 0..1);
                }
            }
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
        Ok(())
    }

    fn upload_hud_mesh(&mut self, mesh: Option<&crate::hud::HudMesh>) {
        let Some(mesh) = mesh else {
            self.hud_index_count = 0;
            return;
        };
        if mesh.indices.is_empty() {
            self.hud_index_count = 0;
            return;
        }
        write_dynamic_opt_buffer(
            &self.device,
            &self.queue,
            &mut self.hud_vb,
            &mut self.hud_vb_cap,
            "hud_vb",
            wgpu::BufferUsages::VERTEX,
            bytemuck::cast_slice(&mesh.vertices),
        );
        write_dynamic_opt_buffer(
            &self.device,
            &self.queue,
            &mut self.hud_ib,
            &mut self.hud_ib_cap,
            "hud_ib",
            wgpu::BufferUsages::INDEX,
            bytemuck::cast_slice(&mesh.indices),
        );
        self.hud_index_count = mesh.indices.len() as u32;
    }

    /// Superficie del agua sobre los pies (`None` = seco): la malla del héroe
    /// oscurece lo que quede por debajo (mitad hundida mojada).
    fn water_surface_above(world: &crate::world::World, feet: Vec3) -> Option<f32> {
        let x = feet.x.floor() as i32;
        let z = feet.z.floor() as i32;
        let mut top: Option<f32> = None;
        let base = feet.y.floor() as i32;
        // Como mucho el cuerpo entero + margen.
        let mut y = base;
        while y < base + 4 && y <= crate::world::WORLD_MAX_Y {
            if world.is_water_at(glam::IVec3::new(x, y, z)) {
                top = Some(y as f32 + 1.0);
            } else if top.is_some() {
                break;
            }
            y += 1;
        }
        top
    }

    fn upload_player_hero(
        &mut self,
        feet: Vec3,
        facing: f32,
        camera_pos: Vec3,
        pose: &crate::hero_pose::HeroPose,
        tool_id: Option<crate::items::ToolId>,
        tool_swing: f32,
        equip_blend: f32,
        fp_legs_only: bool,
        wet_line: Option<f32>,
        preview: Option<EditorPreview>,
    ) {
        let (verts, indices, body_indices) = build_player_hero_mesh(
            feet,
            facing,
            camera_pos,
            pose,
            self.hero_winding_flip,
            tool_id,
            tool_swing,
            equip_blend,
            fp_legs_only,
            wet_line,
            preview,
        );
        if verts.is_empty() {
            self.player_vb = None;
            self.player_vb_cap = 0;
            self.player_index_count = 0;
            self.player_body_index_count = 0;
            return;
        }
        write_dynamic_opt_buffer(
            &self.device,
            &self.queue,
            &mut self.player_vb,
            &mut self.player_vb_cap,
            "player_vb",
            wgpu::BufferUsages::VERTEX,
            bytemuck::cast_slice(&verts),
        );
        write_dynamic_buffer(
            &self.device,
            &self.queue,
            &mut self.player_ib,
            &mut self.player_ib_cap,
            "player_ib",
            wgpu::BufferUsages::INDEX,
            bytemuck::cast_slice(&indices),
        );
        self.player_index_count = indices.len() as u32;
        self.player_body_index_count = body_indices;
    }

    fn draw_player_billboard<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, shadow: bool) {
        let Some(vb) = self.player_vb.as_ref() else {
            return;
        };
        // Shadow + solid colour: full body.
        let end = self.player_body_index_count;
        if end == 0 {
            return;
        }
        if shadow {
            pass.set_pipeline(&self.shadow_pipeline);
            pass.set_bind_group(0, &self.shadow_frame_bind_group, &[]);
        } else {
            pass.set_pipeline(&self.player_solid_pipeline);
            pass.set_bind_group(0, &self.frame_bind_group, &[]);
            pass.set_stencil_reference(1);
        }
        pass.set_vertex_buffer(0, vb.slice(..));
        pass.set_index_buffer(self.player_ib.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..end, 0, 0..1);
    }

    /// Occluded silhouette: reset depth under body, then LessEqual color (self-sort).
    fn draw_player_occluded<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        let Some(vb) = self.player_vb.as_ref() else {
            return;
        };
        let end = self.player_body_index_count;
        if end == 0 {
            return;
        }
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_stencil_reference(0);
        pass.set_vertex_buffer(0, vb.slice(..));
        pass.set_index_buffer(self.player_ib.slice(..), wgpu::IndexFormat::Uint32);
        pass.set_pipeline(&self.player_occluded_depth_pipeline);
        pass.draw_indexed(0..end, 0, 0..1);
        pass.set_pipeline(&self.player_occluded_pipeline);
        pass.draw_indexed(0..end, 0, 0..1);
    }

    /// Agua traslúcida real (mezcla alfa): se dibuja tras lo opaco para que
    /// el héroe y el fondo se vean a través con tinta suave, sin tramado.
    fn draw_visible_water<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, world: &World) {
        pass.set_pipeline(&self.water_pipeline);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        for &(cx, cy, cz) in &self.visible_chunks {
            if !world.chunk_filled(cx, cz) {
                continue;
            }
            let Some(entry) = self.chunk_meshes.get(&(cx, cy, cz)) else {
                continue;
            };
            let Some(gpu) = entry.gpu.as_ref() else {
                continue;
            };
            if gpu.num_water == 0 {
                continue;
            }
            let (Some(vb), Some(ib)) =
                (gpu.water_vertex_buffer.as_ref(), gpu.water_index_buffer.as_ref())
            else {
                continue;
            };
            pass.set_vertex_buffer(0, vb.slice(..));
            pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..gpu.num_water, 0, 0..1);
        }
    }

    fn upload_ghost_cells(&mut self, cells: &[(glam::IVec3, [f32; 3])]) {
        if cells.is_empty() {
            self.ghost_index_count = 0;
            return;
        }
        let (verts, indices) = build_ghost_mesh(cells);
        write_dynamic_opt_buffer(
            &self.device,
            &self.queue,
            &mut self.ghost_vb,
            &mut self.ghost_vb_cap,
            "ghost_vb",
            wgpu::BufferUsages::VERTEX,
            bytemuck::cast_slice(&verts),
        );
        write_dynamic_opt_buffer(
            &self.device,
            &self.queue,
            &mut self.ghost_ib,
            &mut self.ghost_ib_cap,
            "ghost_ib",
            wgpu::BufferUsages::INDEX,
            bytemuck::cast_slice(&indices),
        );
        self.ghost_index_count = indices.len() as u32;
    }

    fn draw_ghost<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        let (Some(vb), Some(ib)) = (self.ghost_vb.as_ref(), self.ghost_ib.as_ref()) else {
            return;
        };
        if self.ghost_index_count == 0 {
            return;
        }
        pass.set_pipeline(&self.ghost_pipeline);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_vertex_buffer(0, vb.slice(..));
        pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.ghost_index_count, 0, 0..1);
    }

    fn upload_highlight_cells(
        &mut self,
        cells: &[(glam::IVec3, [f32; 3])],
        disc: Option<(Vec3, [f32; 3])>,
    ) {
        if cells.is_empty() && disc.is_none() {
            self.hl_index_count = 0;
            return;
        }
        let (mut verts, mut indices) = build_highlight_mesh(cells);
        // Disco de sombra redonda bajo los pies (sombra gris, no cubo cian).
        if let Some((center, color)) = disc {
            append_foot_disc(&mut verts, &mut indices, center, 0.5, color);
        }
        write_dynamic_opt_buffer(
            &self.device,
            &self.queue,
            &mut self.hl_vb,
            &mut self.hl_vb_cap,
            "hl_vb",
            wgpu::BufferUsages::VERTEX,
            bytemuck::cast_slice(&verts),
        );
        write_dynamic_opt_buffer(
            &self.device,
            &self.queue,
            &mut self.hl_ib,
            &mut self.hl_ib_cap,
            "hl_ib",
            wgpu::BufferUsages::INDEX,
            bytemuck::cast_slice(&indices),
        );
        self.hl_index_count = indices.len() as u32;
    }

    /// Depth-tested highlight shells (no X-ray): occluded by nearer terrain.
    fn draw_highlight<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        let (Some(vb), Some(ib)) = (self.hl_vb.as_ref(), self.hl_ib.as_ref()) else {
            return;
        };
        if self.hl_index_count == 0 {
            return;
        }
        pass.set_pipeline(&self.highlight_pipeline);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_vertex_buffer(0, vb.slice(..));
        pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.hl_index_count, 0, 0..1);
    }

    fn upload_crack_cells(&mut self, cells: &[(glam::IVec3, [f32; 3])]) {
        if cells.is_empty() {
            self.crack_index_count = 0;
            return;
        }
        let (verts, indices) = build_highlight_mesh(cells);
        write_dynamic_opt_buffer(
            &self.device,
            &self.queue,
            &mut self.crack_vb,
            &mut self.crack_vb_cap,
            "crack_vb",
            wgpu::BufferUsages::VERTEX,
            bytemuck::cast_slice(&verts),
        );
        write_dynamic_opt_buffer(
            &self.device,
            &self.queue,
            &mut self.crack_ib,
            &mut self.crack_ib_cap,
            "crack_ib",
            wgpu::BufferUsages::INDEX,
            bytemuck::cast_slice(&indices),
        );
        self.crack_index_count = indices.len() as u32;
    }

    /// Velo oscuro de grieta (alfa ~0.12): se dibuja tras el resaltado.
    fn draw_crack<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        let (Some(vb), Some(ib)) = (self.crack_vb.as_ref(), self.crack_ib.as_ref()) else {
            return;
        };
        if self.crack_index_count == 0 {
            return;
        }
        pass.set_pipeline(&self.crack_pipeline);
        pass.set_bind_group(0, &self.frame_bind_group, &[]);
        pass.set_vertex_buffer(0, vb.slice(..));
        pass.set_index_buffer(ib.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.crack_index_count, 0, 0..1);
    }
}

/// Tinted translucent unit cubes. Color rides per cell (stair yellow, FP
/// target white, stood-on cyan). `grow` expands (>0, highlight shell hugging
/// the block from outside) or shrinks (<0, inset blueprint) the cube.
fn build_tinted_cubes(cells: &[(glam::IVec3, [f32; 3])], grow: f32) -> (Vec<Vertex>, Vec<u32>) {
    let mut verts = Vec::with_capacity(cells.len() * 24);
    let mut indices = Vec::with_capacity(cells.len() * 36);
    for &(cell, color) in cells {
        let o = Vec3::new(cell.x as f32, cell.y as f32, cell.z as f32);
        for &(nx, ny, nz, corners) in &crate::hero::FACE_CORNERS {
            let na = [nx as f32, ny as f32, nz as f32];
            let mut world = [[0.0f32; 3]; 4];
            for (i, c) in corners.iter().enumerate() {
                world[i] = [
                    o.x - grow + c[0] * (1.0 + 2.0 * grow),
                    o.y - grow + c[1] * (1.0 + 2.0 * grow),
                    o.z - grow + c[2] * (1.0 + 2.0 * grow),
                ];
            }
            crate::hero::ensure_outward_quad(&mut world, na);
            let base = verts.len() as u32;
            for p in &world {
                verts.push(Vertex::lit(*p, na, color, 1.0));
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
    (verts, indices)
}

/// Stair blueprint cubes: slightly inset so stacked steps don't z-fight.
fn build_ghost_mesh(cells: &[(glam::IVec3, [f32; 3])]) -> (Vec<Vertex>, Vec<u32>) {
    build_tinted_cubes(cells, -0.02)
}

/// FP highlight shells: slightly outset so the shell hugs the block faces
/// outside the terrain surface (depth-tested, no X-ray).
fn build_highlight_mesh(cells: &[(glam::IVec3, [f32; 3])]) -> (Vec<Vertex>, Vec<u32>) {
    build_tinted_cubes(cells, 0.02)
}

/// Sombra redonda bajo los pies: abanico plano (gris traslúcido vía el
/// pipeline de resaltado). `center` = punto del suelo + epsilon.
fn append_foot_disc(
    verts: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    center: Vec3,
    radius: f32,
    color: [f32; 3],
) {
    const SEGMENTS: usize = 20;
    let base = verts.len() as u32;
    verts.push(Vertex::lit(
        center.to_array(),
        [0.0, 1.0, 0.0],
        color,
        1.0,
    ));
    for i in 0..SEGMENTS {
        let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        verts.push(Vertex::lit(
            [center.x + a.cos() * radius, center.y, center.z + a.sin() * radius],
            [0.0, 1.0, 0.0],
            color,
            1.0,
        ));
    }
    // (centro, siguiente, actual): normal +Y.
    for i in 0..SEGMENTS {
        let cur = base + 1 + i as u32;
        let next = base + 1 + ((i + 1) % SEGMENTS) as u32;
        indices.extend_from_slice(&[base, next, cur]);
    }
}

fn push_quad_indices(indices: &mut Vec<u32>, base: u32, flip: bool) {
    if flip {
        indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    } else {
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Ensure triangle winding is CCW along `normal` (so Back cull keeps the face).
fn push_quad_indices_for_normal(
    indices: &mut Vec<u32>,
    base: u32,
    corners: [[f32; 3]; 4],
    normal: [f32; 3],
    flip_user: bool,
) {
    let p0 = Vec3::from_array(corners[0]);
    let p1 = Vec3::from_array(corners[1]);
    let p2 = Vec3::from_array(corners[2]);
    let cross = (p1 - p0).cross(p2 - p0);
    let n = Vec3::from_array(normal);
    let mut flip = cross.dot(n) < 0.0;
    if flip_user {
        flip = !flip;
    }
    push_quad_indices(indices, base, flip);
}

/// Axis-colored debug cube: each of the 6 faces is a 3×3 panel grid (9 tiles)
/// so winding / screen-door per face is obvious behind a tree. Press F to flip.
fn build_debug_hero_face_cube(feet: Vec3, facing: f32, flip: bool) -> (Vec<Vertex>, Vec<u32>, u32) {
    let mut verts = Vec::with_capacity(6 * 9 * 4);
    let mut indices = Vec::with_capacity(6 * 9 * 6);

    let forward = Vec3::new(facing.cos(), 0.0, facing.sin());
    let right = Vec3::new(-facing.sin(), 0.0, facing.cos());
    let up = Vec3::Y;
    let half = 0.55;
    let center = feet + up * half;

    // Local-space cube faces: normal + 4 corners in [-1,1] cube, CCW outward.
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [1.0, 0.0, 0.0],
            [
                [1.0, -1.0, -1.0],
                [1.0, 1.0, -1.0],
                [1.0, 1.0, 1.0],
                [1.0, -1.0, 1.0],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [-1.0, -1.0, 1.0],
                [-1.0, 1.0, 1.0],
                [-1.0, 1.0, -1.0],
                [-1.0, -1.0, -1.0],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [-1.0, 1.0, -1.0],
                [-1.0, 1.0, 1.0],
                [1.0, 1.0, 1.0],
                [1.0, 1.0, -1.0],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [-1.0, -1.0, -1.0],
                [1.0, -1.0, -1.0],
                [1.0, -1.0, 1.0],
                [-1.0, -1.0, 1.0],
            ],
        ),
        (
            [0.0, 0.0, 1.0],
            [
                [-1.0, -1.0, 1.0],
                [1.0, -1.0, 1.0],
                [1.0, 1.0, 1.0],
                [-1.0, 1.0, 1.0],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [1.0, -1.0, -1.0],
                [-1.0, -1.0, -1.0],
                [-1.0, 1.0, -1.0],
                [1.0, 1.0, -1.0],
            ],
        ),
    ];

    // Same basis as `hero::for_each_hero_face`: negate local Z so [right,up,forward]
    // is a proper rotation (det=+1). Without `-z` the basis is a mirror and
    // Back-face culling eats the top / other faces.
    let to_world = |local: [f32; 3]| -> [f32; 3] {
        let p = center
            + right * (local[0] * half)
            + up * (local[1] * half)
            + forward * (-local[2] * half);
        p.to_array()
    };
    let to_world_n = |n: [f32; 3]| -> [f32; 3] {
        (right * n[0] + up * n[1] + forward * (-n[2]))
            .normalize_or_zero()
            .to_array()
    };

    let lerp3 = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    };

    for (n_local, corners) in &faces {
        let n = to_world_n(*n_local);
        let base_rgb = debug_face_color(*n_local);
        // 3×3 tiles on each face (9 panels) — center tile is brightest.
        for ty in 0..3 {
            for tx in 0..3 {
                let u0 = tx as f32 / 3.0;
                let u1 = (tx + 1) as f32 / 3.0;
                let v0 = ty as f32 / 3.0;
                let v1 = (ty + 1) as f32 / 3.0;
                let mut shade = 0.55 + 0.45 * (((tx + ty * 3) as f32) / 8.0);
                if tx == 1 && ty == 1 {
                    shade = 1.15;
                }
                let rgb = [
                    (base_rgb[0] * shade).min(1.0),
                    (base_rgb[1] * shade).min(1.0),
                    (base_rgb[2] * shade).min(1.0),
                ];
                // corners CCW: u along c0→c3, v along c0→c1.
                let bilerp = |u: f32, v: f32| {
                    let a = lerp3(corners[0], corners[3], u);
                    let b = lerp3(corners[1], corners[2], u);
                    lerp3(a, b, v)
                };
                let tile = [
                    bilerp(u0, v0),
                    bilerp(u0, v1),
                    bilerp(u1, v1),
                    bilerp(u1, v0),
                ];
                let mut world_corners = [[0.0f32; 3]; 4];
                let base = verts.len() as u32;
                for (i, c) in tile.iter().enumerate() {
                    let mut p = to_world(*c);
                    p[0] += n[0] * 0.002;
                    p[1] += n[1] * 0.002;
                    p[2] += n[2] * 0.002;
                    world_corners[i] = p;
                    verts.push(Vertex::lit(p, n, rgb, 1.0));
                }
                push_quad_indices_for_normal(&mut indices, base, world_corners, n, flip);
            }
        }
    }

    let body_index_count = indices.len() as u32;
    // Tiny footing under the cube (same screen-door path as the hero).
    {
        let y = feet.y + 0.008;
        let r = 0.4;
        let n = [0.0, 1.0, 0.0];
        let color = [0.42, 0.22, 0.10];
        let corners = [
            [feet.x - r, y, feet.z - r],
            [feet.x + r, y, feet.z - r],
            [feet.x + r, y, feet.z + r],
            [feet.x - r, y, feet.z + r],
        ];
        let base = verts.len() as u32;
        for pos in corners {
            verts.push(Vertex {
                position: pos,
                normal: n,
                color,
                uv: [0.0, 0.0],
                flags: 1.0,
                seed: 0.0,
                ao: 1.0,
            });
        }
        push_quad_indices_for_normal(&mut indices, base, corners, n, flip);
    }
    (verts, indices, body_index_count)
}

/// Voxel hero mesh (body only — canopy and burial share one occluded pass).
/// Returns `(verts, indices, body_index_count)`.
/// `fp_legs_only`: en 1ª persona recorta todo por encima de ~1/3 (0.62m sobre
/// los pies) para que al mirar abajo solo se vean piernas/pies estilo Minecraft.
/// `wet_line`: línea de flotación (mundo Y) — lo que queda por debajo se
/// oscurece como mojado.
fn build_player_hero_mesh(
    feet: Vec3,
    facing: f32,
    camera_pos: Vec3,
    pose: &crate::hero_pose::HeroPose,
    flip_winding: bool,
    tool_id: Option<crate::items::ToolId>,
    tool_swing: f32,
    equip_blend: f32,
    fp_legs_only: bool,
    wet_line: Option<f32>,
    preview: Option<EditorPreview>,
) -> (Vec<Vertex>, Vec<u32>, u32) {
    if crate::world::DEBUG_HERO_FACE_CUBE {
        return build_debug_hero_face_cube(feet, facing, flip_winding);
    }

    let mut verts = Vec::with_capacity(8192);
    let mut indices = Vec::with_capacity(12288);
    let mut face: Vec<([f32; 3], [f32; 3], [f32; 3])> = Vec::with_capacity(4);
    let debug_cam = crate::world::DEBUG_HERO_CAMERA_FACES;

    let push_face_quad = |face: &mut Vec<([f32; 3], [f32; 3], [f32; 3])>,
                              verts: &mut Vec<Vertex>,
                              indices: &mut Vec<u32>| {
        if face.len() != 4 {
            return;
        }
        let base = verts.len() as u32;
        let corners = [face[0].0, face[1].0, face[2].0, face[3].0];
        // 1ª persona: solo 1/3 inferior (piernas/pies). Descarta el quad si su
        // centro queda por encima de feet + 0.62m (cabeza/torso/brazos/herramienta).
        if fp_legs_only {
            let cy = (corners[0][1] + corners[1][1] + corners[2][1] + corners[3][1]) * 0.25;
            if cy > feet.y + 0.62 {
                face.clear();
                return;
            }
        }
        let normal = face[0].1;
        // Mitad hundida mojada: oscurece + enfría por vértice (corte en la
        // línea de flotación, degradado natural en la malla).
        let wet_mul = |y: f32| -> [f32; 3] {
            if wet_line.is_some_and(|w| y < w) {
                [0.55, 0.60, 0.68]
            } else {
                [1.0, 1.0, 1.0]
            }
        };
        let (out_color, face_id, uvs) = if debug_cam {
            let center = Vec3::new(
                (corners[0][0] + corners[1][0] + corners[2][0] + corners[3][0]) * 0.25,
                (corners[0][1] + corners[1][1] + corners[2][1] + corners[3][1]) * 0.25,
                (corners[0][2] + corners[1][2] + corners[2][2] + corners[3][2]) * 0.25,
            );
            let n_v = Vec3::from_array(normal);
            let mut faces_cam = n_v.dot(camera_pos - center) > 1e-4;
            if flip_winding {
                faces_cam = !faces_cam;
            }
            let id = hero_face_axis_id(normal);
            let mut rgb = debug_face_color(normal);
            if !faces_cam {
                rgb = [rgb[0] * 0.12, rgb[1] * 0.12, rgb[2] * 0.12];
            }
            (
                rgb,
                id as f32,
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            )
        } else {
            (
                face[0].2,
                0.0,
                [[0.0, 0.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0]],
            )
        };
        for (i, (p, nn, _)) in face.iter().enumerate() {
            let m = wet_mul(p[1]);
            verts.push(Vertex {
                position: *p,
                normal: *nn,
                color: [out_color[0] * m[0], out_color[1] * m[1], out_color[2] * m[2]],
                uv: uvs[i],
                flags: 0.0,
                seed: face_id,
                ao: 1.0,
            });
        }
        let tri_flip = flip_winding && !debug_cam;
        push_quad_indices_for_normal(indices, base, corners, normal, tri_flip);
        face.clear();
    };

    // `push_face_quad` closure stays shared by both bodies below.
    if let Some(p) = preview {
        // Editor preview: the selection's own mesh, unrigged, at its entity
        // transform. No pose, no held tool — it is not the player. The palette
        // is the active state's override, falling back to the mesh's own.
        p.model.for_each_face(
            p.palette.unwrap_or(&p.model.palette),
            p.feet,
            p.facing,
            p.body_height,
            |pos, n, color| {
                face.push((pos, n, color));
                if face.len() == 4 {
                    push_face_quad(&mut face, &mut verts, &mut indices);
                }
            },
        );
    } else {
        crate::hero::for_each_hero_face(
            feet,
            facing,
            crate::player::PLAYER_HEIGHT,
            pose,
            |pos, n, color| {
                face.push((pos, n, color));
                if face.len() == 4 {
                    push_face_quad(&mut face, &mut verts, &mut indices);
                }
            },
        );

        // Equip blend drives draw-from-hip → settled grip (scale + wrist in hero).
        let equip = equip_blend.clamp(0.0, 1.0);
        let swing = tool_swing * equip;

        match tool_id {
            Some(crate::items::ToolId::Special1Sword) => {
                crate::hero::for_each_held_sword_face(
                    feet,
                    facing,
                    crate::player::PLAYER_HEIGHT,
                    pose,
                    equip,
                    |pos, n, color| {
                        face.push((pos, n, color));
                        if face.len() == 4 {
                            push_face_quad(&mut face, &mut verts, &mut indices);
                        }
                    },
                );
            }
            Some(crate::items::ToolId::WoodenPickaxe) => {
                crate::hero::for_each_held_pickaxe_face_ex(
                    feet,
                    facing,
                    crate::player::PLAYER_HEIGHT,
                    pose,
                    swing,
                    equip,
                    |pos, n, color| {
                        face.push((pos, n, color));
                        if face.len() == 4 {
                            push_face_quad(&mut face, &mut verts, &mut indices);
                        }
                    },
                );
            }
            Some(crate::items::ToolId::AxeStub) | None => {}
        }
    }

    let body_index_count = indices.len() as u32;
    (verts, indices, body_index_count)
}

fn create_hud_pipeline(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
) -> (
    wgpu::RenderPipeline,
    wgpu::BindGroup,
    wgpu::Texture,
    wgpu::Sampler,
) {
    use crate::hud::{build_hud_atlas, HudVertex};

    let atlas = build_hud_atlas();
    let size = wgpu::Extent3d {
        width: atlas.width(),
        height: atlas.height(),
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("hud_atlas"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        atlas.as_raw(),
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(4 * atlas.width()),
            rows_per_image: Some(atlas.height()),
        },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("hud_samp"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });

    let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("hud_bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("hud_bg"),
        layout: &bind_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("hud_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("hud.wgsl").into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("hud_pll"),
        bind_group_layouts: &[&bind_layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("hud_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<HudVertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x2,
                    1 => Float32x2,
                    2 => Float32x4,
                ],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });

    (pipeline, bind_group, texture, sampler)
}

fn create_blur_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    scene_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    uniform: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("blur_bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(scene_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniform.as_entire_binding(),
            },
        ],
    })
}

/// Pack `grass1.png`…`grass6.png` into a horizontal atlas (magenta cutout).
fn build_grass_atlas() -> image::RgbaImage {
    use image::{GenericImage, Rgba, RgbaImage};
    let sprites: [&[u8]; 6] = [
        include_bytes!("../assets/grass1.png"),
        include_bytes!("../assets/grass2.png"),
        include_bytes!("../assets/grass3.png"),
        include_bytes!("../assets/grass4.png"),
        include_bytes!("../assets/grass5.png"),
        include_bytes!("../assets/grass6.png"),
    ];
    let imgs: Vec<RgbaImage> = sprites
        .iter()
        .map(|bytes| {
            image::load_from_memory(bytes)
                .expect("grass atlas sprite")
                .to_rgba8()
        })
        .collect();
    let tile_w = imgs.iter().map(|i| i.width()).max().unwrap_or(32);
    let tile_h = imgs.iter().map(|i| i.height()).max().unwrap_or(28);
    let magenta = Rgba([255, 0, 255, 255]);
    let mut atlas = RgbaImage::from_pixel(tile_w * 6, tile_h, magenta);
    for (i, img) in imgs.iter().enumerate() {
        let ox = i as u32 * tile_w + (tile_w - img.width()) / 2;
        // Bottom-align so roots sit on the dirt top across uneven sprite sizes.
        let oy = tile_h - img.height();
        atlas.copy_from(img, ox, oy).expect("grass atlas tile blit");
    }
    atlas
}

/// Load the grass atlas into a nearest-filtered sprite bind group.
fn create_grass_texture_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (
    wgpu::BindGroupLayout,
    wgpu::Texture,
    wgpu::TextureView,
    wgpu::Sampler,
    wgpu::BindGroup,
) {
    let img = build_grass_atlas();
    let (width, height) = img.dimensions();
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("grass_atlas_tex"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &img,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("grass_samp"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("grass_tex_bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("grass_tex_bg"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    (layout, texture, view, sampler, bind_group)
}

/// Procedural 16×16×12 atlas: grass×2 | dirt | bark | leaves | stone×3 |
/// planks | village stone | cobblestone | sand.
fn build_terrain_atlas() -> image::RgbaImage {
    use image::{Rgba, RgbaImage};
    let mut img = RgbaImage::new(16 * TERRAIN_ATLAS_TILES, 16);
    let hash = |x: u32, y: u32, salt: u32| -> u32 {
        let mut n = x
            .wrapping_mul(374761393)
            .wrapping_add(y.wrapping_mul(668265263))
            .wrapping_add(salt);
        n = (n ^ (n >> 13)).wrapping_mul(1274126177);
        n ^ (n >> 16)
    };
    let put = |img: &mut RgbaImage, tx: u32, x: u32, y: u32, rgb: [u8; 3]| {
        img.put_pixel(tx * 16 + x, y, Rgba([rgb[0], rgb[1], rgb[2], 255]));
    };

    // Tile 0 — grass top
    for y in 0..16u32 {
        for x in 0..16u32 {
            let n = hash(x, y, 0xA11CE);
            let g = 90 + (n % 40) as u8;
            let r = 40 + (n % 25) as u8;
            let b = 30 + (n % 20) as u8;
            put(&mut img, 0, x, y, [r, g, b]);
        }
    }
    // Tile 1 — grass block side (green fringe on top rows)
    for y in 0..16u32 {
        for x in 0..16u32 {
            let n = hash(x, y, 0x51DE);
            if y < 3 {
                let g = 85 + (n % 45) as u8;
                put(&mut img, 1, x, y, [45 + (n % 20) as u8, g, 35]);
            } else if y == 3 {
                let mix = n % 2;
                if mix == 0 {
                    put(&mut img, 1, x, y, [55, 110, 40]);
                } else {
                    let d = 95 + (n % 35) as u8;
                    put(&mut img, 1, x, y, [d, (d as i16 - 25).max(40) as u8, 55]);
                }
            } else {
                let d = 100 + (n % 40) as u8;
                let r = d;
                let g = (d as i16 - 30).clamp(50, 200) as u8;
                let b = (d as i16 - 50).clamp(30, 180) as u8;
                put(&mut img, 1, x, y, [r, g, b]);
            }
        }
    }
    // Tile 2 — plain dirt
    for y in 0..16u32 {
        for x in 0..16u32 {
            let n = hash(x, y, 0xD17);
            let d = 95 + (n % 45) as u8;
            let r = d;
            let g = (d as i16 - 28).clamp(45, 200) as u8;
            let b = (d as i16 - 48).clamp(25, 180) as u8;
            put(&mut img, 2, x, y, [r, g, b]);
        }
    }
    // Tile 3 — wood bark (vertical grain + darker cracks)
    for y in 0..16u32 {
        for x in 0..16u32 {
            let n = hash(x, y, 0xBA12);
            let grain = hash(x, y / 2, 0x67A1);
            let crack = (x + grain % 5) % 7 == 0;
            let base = 88 + (n % 28) as u8;
            let (r, g, b) = if crack {
                (
                    (base as i16 - 35).clamp(35, 120) as u8,
                    (base as i16 - 45).clamp(25, 100) as u8,
                    (base as i16 - 55).clamp(18, 80) as u8,
                )
            } else {
                let streak = ((y + x / 3) % 4) as u8;
                (
                    base.saturating_add(streak * 3),
                    (base as i16 - 18 + streak as i16).clamp(40, 160) as u8,
                    (base as i16 - 36).clamp(28, 120) as u8,
                )
            };
            put(&mut img, 3, x, y, [r, g, b]);
        }
    }
    // Tile 4 — leaves (dappled canopy greens)
    for y in 0..16u32 {
        for x in 0..16u32 {
            let n = hash(x, y, 0x1EAF);
            let blob = hash(x / 2, y / 2, 0xCA17);
            let hole = blob % 11 == 0;
            if hole {
                // Slightly darker gap so the canopy reads as foliage, not flat.
                put(&mut img, 4, x, y, [28, 52, 24]);
            } else {
                let g = 95 + (n % 55) as u8;
                let r = 35 + (n % 30) as u8 + (blob % 12) as u8;
                let b = 28 + (n % 22) as u8;
                put(&mut img, 4, x, y, [r, g.min(200), b]);
            }
        }
    }
    // Tiles 5–7 — three stone variants (different crack bias / grain) for de-tiling.
    let stone_variant = |img: &mut RgbaImage, tx: u32, salt: u32, crack_mod: u32, base0: u8| {
        for y in 0..16u32 {
            for x in 0..16u32 {
                let n = hash(x, y, salt);
                let crack_h = hash(x, y / 3, salt ^ 0xC2AC) % crack_mod == 0;
                let crack_v = hash(x / 3, y, salt ^ 0x51AB) % (crack_mod + 2) == 0;
                let diag = hash(x + y, x * 3 + y, salt ^ 0xD1A6) % (crack_mod + 4) == 0;
                let speck = n % 5;
                let base = base0 + (n % 28) as u8;
                let (r, g, b) = if crack_h || crack_v || diag {
                    (
                        (base as i16 - 48).clamp(65, 155) as u8,
                        (base as i16 - 44).clamp(70, 160) as u8,
                        (base as i16 - 36).clamp(78, 170) as u8,
                    )
                } else {
                    let lift = (speck as i16 - 2) * 4;
                    (
                        (base as i16 + lift).clamp(105, 200) as u8,
                        (base as i16 + lift + 2).clamp(110, 205) as u8,
                        (base as i16 + lift + 7).clamp(118, 212) as u8,
                    )
                };
                put(img, tx, x, y, [r, g, b]);
            }
        }
    };
    stone_variant(&mut img, 5, 0x57A8, 9, 138); // horizontal-biased cracks
    stone_variant(&mut img, 6, 0x6B01, 7, 132); // denser / cooler
    stone_variant(&mut img, 7, 0x71CE, 11, 145); // sparser / brighter

    // Tile 8 — horizontal sawn planks: broad boards, seams, sparse nail heads.
    for y in 0..16u32 {
        for x in 0..16u32 {
            let n = hash(x, y, 0xB04D);
            let seam = y % 5 == 0;
            let joint = (x + (y / 5) * 7) % 12 == 0;
            let nail = (x == 2 || x == 13) && y % 5 == 2;
            let rgb = if seam || joint {
                [92, 59, 29]
            } else if nail {
                [58, 48, 36]
            } else {
                let lift = (n % 25) as u8;
                [158 + lift, 108 + lift / 2, 53 + lift / 3]
            };
            put(&mut img, 8, x, y, rgb);
        }
    }

    // Tile 9 — lower, slightly yellow village stone with irregular mortar.
    for y in 0..16u32 {
        for x in 0..16u32 {
            let n = hash(x, y, 0xA11D);
            let row = y / 5;
            let mortar_h = y % 5 == 0;
            let mortar_v = (x + (row % 2) * 5) % 9 == 0;
            let rgb = if mortar_h || mortar_v {
                [112, 105, 84]
            } else {
                let lift = (n % 27) as u8;
                [142 + lift, 137 + lift, 108 + lift / 2]
            };
            put(&mut img, 9, x, y, rgb);
        }
    }

    // Tile 10 — cobblestone: irregular rounded stones with dark mortar.
    for y in 0..16u32 {
        for x in 0..16u32 {
            let cell_x = x / 4;
            let cell_y = y / 4;
            let ox = (cell_x * 3 + cell_y * 5) % 3;
            let oy = (cell_y * 2 + cell_x) % 3;
            let lx = (x + ox) % 4;
            let ly = (y + oy) % 4;
            let edge = lx == 0 || ly == 0 || lx == 3 || ly == 3;
            let n = hash(x, y, 0xC0BB);
            let stone_n = hash(cell_x, cell_y, 0x57A1);
            let rgb = if edge {
                [72, 68, 62]
            } else {
                let lift = (n % 22) as u8;
                let cool = (stone_n % 18) as u8;
                [
                    118 + lift,
                    114 + lift.saturating_sub(cool / 2),
                    104 + lift.saturating_sub(cool),
                ]
            };
            put(&mut img, 10, x, y, rgb);
        }
    }

    // Tile 11 — sand (warm grain)
    for y in 0..16u32 {
        for x in 0..16u32 {
            let n = hash(x, y, 0x5A4D);
            let streak = hash(x / 2, y, 0xD04E);
            let base = 175 + (n % 35) as u8;
            let r = base;
            let g = (base as i16 - 18 + (streak % 12) as i16).clamp(120, 220) as u8;
            let b = (base as i16 - 55).clamp(70, 160) as u8;
            put(&mut img, 11, x, y, [r, g, b]);
        }
    }
    img
}

fn create_terrain_texture_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (
    wgpu::BindGroupLayout,
    wgpu::Texture,
    wgpu::TextureView,
    wgpu::Sampler,
    wgpu::BindGroup,
) {
    let img = build_terrain_atlas();
    let (width, height) = img.dimensions();
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("terrain_atlas_tex"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &img,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(4 * width),
            rows_per_image: Some(height),
        },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("terrain_samp"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("terrain_tex_bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("terrain_tex_bg"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    (layout, texture, view, sampler, bind_group)
}

fn create_scene_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scene_color"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_depth_view(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: SCENE_DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

fn mesh_one_voxel(
    world: &World,
    pos: &glam::IVec3,
    voxel: &crate::world::Voxel,
    camera_pos: Vec3,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    grass: &mut Vec<GrassInstance>,
    near_hires: bool,
    grass_density: f32,
) {
    use crate::world::{dirt_mesh_max_dist_sq, grass_fade_end, mix_seed, GrassLod, Material};

    if voxel.is_empty() {
        return;
    }
    let origin = Vec3::new(pos.x as f32, pos.y as f32, pos.z as f32);
    // XZ only: tall columns must not vanish when the camera is lower than the peak.
    let dx = camera_pos.x - (pos.x as f32 + 0.5);
    let dz = camera_pos.z - (pos.z as f32 + 0.5);
    let dist_sq = dx * dx + dz * dz;
    let dist = dist_sq.sqrt();
    let grass_lod = GrassLod::for_distance(dist);
    let mesh_cut_sq = dirt_mesh_max_dist_sq();

    match voxel.material {
        Material::Stone
        | Material::VillageStone
        | Material::Cobblestone
        | Material::RoadCobble
        | Material::WoodPlanks
        | Material::BlackStone
        | Material::Bedrock => {
            // Dense stone quads come from greedy_mesh_solid_dirt.
            // Ore flecks: [`mesh_ore_overlays`].
        }
        Material::Coal | Material::Sapphire | Material::Ruby | Material::Emerald => {
            // Crafted cubes are greedy-meshed with other solids when fully solid.
        }
        Material::Chest | Material::Glass | Material::Door | Material::Water | Material::ThermalWater | Material::Sand => {
            // Solid crate / pane / door / pond / sand via greedy_mesh_solid_dirt.
        }
        Material::Torch | Material::Apple | Material::Beehive | Material::ManaCrystal => {
            // Settlement props (flame / fruit / hive / crystal) ride the same
            // greedy solid path as crates and panes.
        }
        Material::Dirt | Material::Mud if voxel.is_fully_solid() => {
            if !near_hires || dist_sq > mesh_cut_sq {
                return;
            }
            push_dirt_face_detail(vertices, indices, origin, *pos, world, camera_pos);
        }
        Material::Dirt | Material::Mud => {
            if dist_sq > mesh_cut_sq {
                return;
            }
            // Distance fade shortens micro detail via [`GrassLod::for_distance`].
            push_microvoxel_mesh(
                world,
                *pos,
                vertices,
                indices,
                origin,
                voxel,
                voxel.material.color_rgb(),
                camera_pos,
                grass_lod,
                None,
            );
        }
        Material::Wood
        | Material::BirchWood
        | Material::PineWood
        | Material::WillowWood
        | Material::MesquiteWood
        | Material::CeibaWood
        | Material::EnchantedWood
        | Material::UmbraWood => {
            if dist_sq > mesh_cut_sq {
                return;
            }
            let tint = voxel.material.color_rgb();
            if voxel.is_fully_solid() {
                push_textured_block_faces(
                    vertices,
                    indices,
                    origin,
                    *pos,
                    world,
                    TERRAIN_TILE_WOOD,
                    tint,
                    camera_pos,
                );
            } else {
                // Full micro height — grass LOD must not crop the trunk.
                let full = crate::world::GrassLod {
                    max_blade_y: Some(crate::world::MICROVOXEL_RES - 1),
                    solid_carpet: false,
                };
                push_microvoxel_mesh(
                    world,
                    *pos,
                    vertices,
                    indices,
                    origin,
                    voxel,
                    tint,
                    camera_pos,
                    full,
                    Some(TERRAIN_TILE_WOOD),
                );
            }
        }
        Material::Leaves
        | Material::BirchLeaves
        | Material::PineLeaves
        | Material::WillowLeaves
        | Material::MesquiteLeaves
        | Material::CeibaLeaves
        | Material::EnchantedLeaves
        | Material::UmbraLeaves => {
            if dist_sq > mesh_cut_sq {
                return;
            }
            push_textured_block_faces(
                vertices,
                indices,
                origin,
                *pos,
                world,
                TERRAIN_TILE_LEAVES,
                voxel.material.color_rgb(),
                camera_pos,
            );
        }
        Material::Grass => {
            if !crate::world::ENABLE_GRASS || grass_density <= 0.0 {
                return;
            }
            // Far carpet LOD: past fade end, only emit when solid_carpet kicks in.
            if grass_lod.max_blade_y.is_none() && !grass_lod.solid_carpet {
                return;
            }
            if dist >= grass_fade_end() && !grass_lod.solid_carpet {
                return;
            }
            if grass_density < 1.0 {
                let h = mix_seed(crate::world::GRASS_SEED, pos.x as u32, pos.z as u32);
                let roll = (h % 1000) as f32 / 1000.0;
                if roll > grass_density {
                    return;
                }
            }
            // Stable per-column pick across 7 sprites × horizontal flip (14 looks).
            let h = mix_seed(
                crate::world::GRASS_SEED ^ 0xA71A_5EED,
                pos.x as u32,
                pos.z as u32,
            );
            let variant = (h % GRASS_VARIANT_COUNT) as f32;
            grass.push(GrassInstance {
                origin: origin.to_array(),
                color: crate::biomes::biome_at(pos.x, pos.z).grass_billboard_tint(),
                variant,
            });
        }
    }
}

/// Minecraft-style corner AO: count solid side/side/corner neighbors (0..=3).
fn corner_ao(
    world: &World,
    block: glam::IVec3,
    normal: glam::IVec3,
    u_sign: i32,
    v_sign: i32,
) -> f32 {
    let (du, dv) = face_tangent_axes(normal);
    let side_u = world.solid_occludes(block + du * u_sign);
    let side_v = world.solid_occludes(block + dv * v_sign);
    let corner = world.solid_occludes(block + du * u_sign + dv * v_sign);
    let occluded = if side_u && side_v {
        3
    } else {
        (side_u as u8) + (side_v as u8) + (corner as u8)
    };
    // Soft AO — strong corner darkening looked plastic on grass tops.
    1.0 - 0.12 * occluded as f32
}

fn face_tangent_axes(normal: glam::IVec3) -> (glam::IVec3, glam::IVec3) {
    if normal.x != 0 {
        (glam::IVec3::Y, glam::IVec3::Z)
    } else if normal.y != 0 {
        (glam::IVec3::X, glam::IVec3::Z)
    } else {
        (glam::IVec3::X, glam::IVec3::Y)
    }
}

/// Greedy-mesh solid dirt for mid chunk LOD (one LOD for the whole chunk).
/// `y0..y1` clamps the slab (16³ B-split passes one 16-block section;
/// the legacy column path passes the full `0..WORLD_MAX_HEIGHT`).
fn greedy_mesh_solid_dirt(
    world: &World,
    cx: i32,
    cz: i32,
    y0: i32,
    y1: i32,
    band: u8,
    camera_pos: Vec3,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use crate::world::{Material, MESH_CHUNK_SIZE};

    let _ = band;
    // Fase 2 (cuevas solo-frente) usará `camera_pos` para cull por quad aquí.
    let _ = camera_pos;
    let x0 = cx * MESH_CHUNK_SIZE;
    let z0 = cz * MESH_CHUNK_SIZE;

    let solid = |x: i32, y: i32, z: i32| -> bool {
        world.get_voxel(glam::IVec3::new(x, y, z)).is_some_and(|v| {
            matches!(
                v.material,
                Material::Dirt
                    | Material::Stone
                    | Material::VillageStone
                    | Material::Cobblestone
                    | Material::RoadCobble
                    | Material::WoodPlanks
                    | Material::BlackStone
                    | Material::Bedrock
                    | Material::Coal
                    | Material::Sapphire
                    | Material::Ruby
                    | Material::Emerald
                    | Material::Chest
                    | Material::Glass
                    | Material::Door
                    | Material::Torch
                    | Material::Apple
                    | Material::Beehive
                    | Material::ManaCrystal
                    | Material::Sand
                    | Material::Mud
            ) && v.is_fully_solid()
        })
    };

    let dirs: [(usize, glam::IVec3, Vec3); 6] = [
        (0, glam::IVec3::new(1, 0, 0), Vec3::X),
        (0, glam::IVec3::new(-1, 0, 0), -Vec3::X),
        (1, glam::IVec3::new(0, 1, 0), Vec3::Y),
        (1, glam::IVec3::new(0, -1, 0), -Vec3::Y),
        (2, glam::IVec3::new(0, 0, 1), Vec3::Z),
        (2, glam::IVec3::new(0, 0, -1), -Vec3::Z),
    ];

    for (axis, neighbor, normal) in dirs {
        // `y0..y1` is the exclusive cell range of this slab — every emitted
        // face belongs to exactly one slab, so stacked 16³ sections tile
        // without gaps or duplicates (boundary faces are owned by their cell).
        let (u_axis, v_axis, slice_min, slice_max, u_min, u_max, v_min, v_max) = match axis {
            0 => (
                1usize,
                2usize,
                x0,
                x0 + MESH_CHUNK_SIZE,
                y0,
                y1,
                z0,
                z0 + MESH_CHUNK_SIZE,
            ),
            1 => (
                0usize,
                2usize,
                y0,
                y1,
                x0,
                x0 + MESH_CHUNK_SIZE,
                z0,
                z0 + MESH_CHUNK_SIZE,
            ),
            _ => (
                0usize,
                1usize,
                z0,
                z0 + MESH_CHUNK_SIZE,
                x0,
                x0 + MESH_CHUNK_SIZE,
                y0,
                y1,
            ),
        };

        for slice in slice_min..slice_max {
            let du = (u_max - u_min) as usize;
            let dv = (v_max - v_min) as usize;
            if du == 0 || dv == 0 {
                continue;
            }
            // Same material only — dirt/stone patches must not merge into one tint.
            let mut mask = vec![None::<Material>; du * dv];

            for iv in 0..dv {
                for iu in 0..du {
                    let mut p = [0i32; 3];
                    p[axis] = slice;
                    p[u_axis] = u_min + iu as i32;
                    p[v_axis] = v_min + iv as i32;
                    let pos = glam::IVec3::new(p[0], p[1], p[2]);
                    if !solid(pos.x, pos.y, pos.z) {
                        continue;
                    }
                    if world.dirt_face_occluded(pos, neighbor) {
                        continue;
                    }
                    let idx = iu + iv * du;
                    let mat = world
                        .get_voxel(pos)
                        .map(|v| v.material)
                        .unwrap_or(Material::Dirt);
                    mask[idx] = Some(mat);
                }
            }

            for iv in 0..dv {
                let mut iu = 0usize;
                while iu < du {
                    let idx = iu + iv * du;
                    let Some(mat0) = mask[idx] else {
                        iu += 1;
                        continue;
                    };
                    let mut w = 1usize;
                    while iu + w < du && mask[iu + w + iv * du] == Some(mat0) {
                        w += 1;
                    }
                    let mut h = 1usize;
                    'grow: while iv + h < dv {
                        for k in 0..w {
                            if mask[iu + k + (iv + h) * du] != Some(mat0) {
                                break 'grow;
                            }
                        }
                        h += 1;
                    }
                    for jj in 0..h {
                        for kk in 0..w {
                            mask[iu + kk + (iv + jj) * du] = None;
                        }
                    }

                    let mut origin = [0.0f32; 3];
                    let face_on_pos = if neighbor.x + neighbor.y + neighbor.z > 0 {
                        slice as f32 + 1.0
                    } else {
                        slice as f32
                    };
                    origin[axis] = face_on_pos;
                    origin[u_axis] = (u_min + iu as i32) as f32;
                    origin[v_axis] = (v_min + iv as i32) as f32;

                    let wu = w as f32;
                    let hv = h as f32;
                    let (c0, c1, c2, c3, flip) =
                        greedy_face_corners(origin, u_axis, v_axis, wu, hv, normal);

                    // AO at rectangle corners using the nearest solid cell under each corner.
                    let mut corner_blocks = [glam::IVec3::ZERO; 4];
                    for (i, (su, sv)) in
                        [(-1, -1), (1, -1), (1, 1), (-1, 1)].into_iter().enumerate()
                    {
                        let mut p = [0i32; 3];
                        p[axis] = slice;
                        let ou = if su < 0 { 0 } else { w as i32 - 1 };
                        let ov = if sv < 0 { 0 } else { h as i32 - 1 };
                        p[u_axis] = u_min + iu as i32 + ou;
                        p[v_axis] = v_min + iv as i32 + ov;
                        corner_blocks[i] = glam::IVec3::new(p[0], p[1], p[2]);
                    }
                    let aos_uv = [
                        corner_ao(world, corner_blocks[0], neighbor, -1, -1),
                        corner_ao(world, corner_blocks[1], neighbor, 1, -1),
                        corner_ao(world, corner_blocks[2], neighbor, 1, 1),
                        corner_ao(world, corner_blocks[3], neighbor, -1, 1),
                    ];
                    // Same UV→winding remap as [`greedy_face_corners`].
                    let aos = if flip {
                        [aos_uv[0], aos_uv[3], aos_uv[2], aos_uv[1]]
                    } else {
                        aos_uv
                    };

                    let n = normal.to_array();
                    let is_top = neighbor == glam::IVec3::Y;
                    let base = vertices.len() as u32;
                    for i in 0..4 {
                        let mut ao = aos[i];
                        let cell = corner_blocks[i];
                        let grassy_top = is_top && dirt_top_is_grassy(world, cell);
                        if grassy_top {
                            ao = 0.55 + 0.45 * ao;
                        }
                        let tile = terrain_tile_for_block(world, cell, neighbor);
                        let corner = [c0, c1, c2, c3][i];
                        let cell_mat = world
                            .get_voxel(cell)
                            .map(|v| v.material)
                            .unwrap_or(Material::Dirt);
                        let tint = if crate::world::DEBUG_FACE_VISIBILITY && !cfg!(test) {
                            debug_face_color(n)
                        } else if grassy_top {
                            let bx = cell.x;
                            let bz = cell.z;
                            let h = world.column_height(bx, bz).unwrap_or(cell.y);
                            surface_albedo(world, bx, bz, h)
                        } else if matches!(
                            cell_mat,
                            Material::Stone
                                | Material::VillageStone
                                | Material::Cobblestone
                                | Material::RoadCobble
                                | Material::WoodPlanks
                                | Material::BlackStone
                                | Material::Bedrock
                        ) {
                            cell_mat.color_rgb()
                        } else if matches!(
                            cell_mat,
                            Material::Coal
                                | Material::Sapphire
                                | Material::Ruby
                                | Material::Emerald
                                | Material::ManaCrystal
                                | Material::Chest
                                | Material::Glass
                                | Material::Door
                                | Material::Torch
                                | Material::Apple
                                | Material::Beehive
                                | Material::Water
                    | Material::ThermalWater
                                | Material::Sand
                        ) {
                            cell_mat.color_rgb()
                        } else {
                            // Dirt / biome-tinted sides: keep material albedo.
                            cell_mat.color_rgb()
                        };
                        if crate::world::DEBUG_FACE_VISIBILITY && !cfg!(test) {
                            vertices.push(Vertex::lit(corner, n, tint, ao));
                        } else if matches!(
                            cell_mat,
                            Material::Coal
                                | Material::Sapphire
                                | Material::Ruby
                                | Material::Emerald
                                | Material::Chest
                                | Material::Glass
                                | Material::Door
                        ) {
                            vertices.push(Vertex::lit(corner, n, tint, ao));
                        } else {
                            vertices.push(Vertex::terrain_face(
                                corner,
                                n,
                                terrain_uv(corner, neighbor),
                                tile,
                                tint,
                                ao,
                            ));
                        }
                    }
                    indices.extend_from_slice(&[
                        base,
                        base + 1,
                        base + 2,
                        base,
                        base + 2,
                        base + 3,
                    ]);
                    iu += w;
                }
            }
        }
    }
}

/// Soft terrain normal from neighboring column heights.
/// Kept for a future continuous heightmesh; flat voxel faces must not use this.
#[allow(dead_code)]
fn soft_normal_at(world: &World, x: i32, z: i32) -> [f32; 3] {
    let h0 = world.column_height(x, z).unwrap_or(0) as f32;
    let hx = |dx: i32| world.column_height(x + dx, z).unwrap_or(h0 as i32) as f32;
    let hz = |dz: i32| world.column_height(x, z + dz).unwrap_or(h0 as i32) as f32;
    let nx = hx(-1) - hx(1);
    let nz = hz(-1) - hz(1);
    Vec3::new(nx, 2.0, nz).normalize_or_zero().to_array()
}

/// Top-face albedo: green only on the natural surface lid; dug lids stay dirt.
fn surface_albedo(world: &World, x: i32, z: i32, h: i32) -> [f32; 3] {
    use crate::biomes::biome_at;
    use crate::world::{mix_seed, terrain_height, Material, WORLD_SEED};
    let biome = biome_at(x, z);
    let dirt = biome.dirt_rgb();
    if !crate::world::ENABLE_HD2D {
        return dirt;
    }
    if world
        .get_voxel(glam::IVec3::new(x, h, z))
        .is_some_and(|v| v.material == Material::Stone)
    {
        return Material::Stone.color_rgb();
    }
    // Subsurface / dug tread — brown dirt tapa.
    if h != terrain_height(x, z) {
        return dirt;
    }
    // Settlement ground is trodden dirt (or sand), never a grass lid.
    if crate::settlements::settlement_claims_block_cached(x, z) {
        return dirt;
    }
    let base = biome.grass_rgb();
    let h_l = world.column_height(x - 1, z).unwrap_or(h);
    let h_r = world.column_height(x + 1, z).unwrap_or(h);
    let h_d = world.column_height(x, z - 1).unwrap_or(h);
    let h_u = world.column_height(x, z + 1).unwrap_or(h);
    let slope =
        ((h_l - h).abs() + (h_r - h).abs() + (h_d - h).abs() + (h_u - h).abs()) as f32 * 0.25;
    let noise = (mix_seed(WORLD_SEED, x as u32, z as u32) % 1000) as f32 / 1000.0;
    let mut c = [
        base[0] + (noise - 0.5) * 0.03,
        base[1] + (noise - 0.5) * 0.03,
        base[2] + (noise - 0.5) * 0.02,
    ];
    // Steep natural surface can peek a little dirt, but stays mostly green.
    let dirt_t = (slope / 2.0).clamp(0.0, 0.35);
    c[0] = c[0] * (1.0 - dirt_t) + dirt[0] * dirt_t;
    c[1] = c[1] * (1.0 - dirt_t) + dirt[1] * dirt_t;
    c[2] = c[2] * (1.0 - dirt_t) + dirt[2] * dirt_t;
    c
}

/// Surface seal used by far HLOD.
///
/// `column_height` is the top of the remaining *dense* stack and drops to a
/// cave floor after `punch_dense_column`; using it here turns distant cave
/// chunks into open wells. HLOD represents the exterior shell, so it must use
/// the untouched natural surface while the chunk is loaded.
fn hlod_column_height(world: &World, x: i32, z: i32) -> Option<i32> {
    world
        .column_height(x, z)
        .map(|_| crate::world::terrain_height(x, z))
}

/// Far HLOD: one continuous, cave-sealed surface (+ vertical skirts).
fn heightmap_mesh_chunk(
    world: &World,
    cx: i32,
    cz: i32,
    _camera_pos: Vec3,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use crate::world::{Material, MESH_CHUNK_SIZE};

    let x0 = cx * MESH_CHUNK_SIZE;
    let z0 = cz * MESH_CHUNK_SIZE;

    for z in z0..z0 + MESH_CHUNK_SIZE {
        for x in x0..x0 + MESH_CHUNK_SIZE {
            let Some(h) = hlod_column_height(world, x, z) else {
                continue;
            };
            let skirt_color = crate::biomes::biome_at(x, z).dirt_rgb();
            let y = (h + 1) as f32;
            let xf = x as f32;
            let zf = z as f32;
            // Tops always emit (see state-key docs).
            let base = vertices.len() as u32;
            // CCW from above: +Z then +X ( +X×+Z is −Y, so swap U/V order).
            let c0 = [xf, y, zf];
            let c1 = [xf, y, zf + 1.0];
            let c2 = [xf + 1.0, y, zf + 1.0];
            let c3 = [xf + 1.0, y, zf];
            let block = glam::IVec3::new(x, h, z);
            let ao0 = 0.55 + 0.45 * corner_ao(world, block, glam::IVec3::Y, -1, -1);
            let ao1 = 0.55 + 0.45 * corner_ao(world, block, glam::IVec3::Y, -1, 1);
            let ao2 = 0.55 + 0.45 * corner_ao(world, block, glam::IVec3::Y, 1, 1);
            let ao3 = 0.55 + 0.45 * corner_ao(world, block, glam::IVec3::Y, 1, -1);
            let n_up = [0.0, 1.0, 0.0];
            let color = if crate::world::ENABLE_HD2D {
                surface_albedo(world, x, z, h)
            } else {
                skirt_color
            };
            let tile = terrain_tile_for_block(world, block, glam::IVec3::Y);
            for (c, ao) in [c0, c1, c2, c3].into_iter().zip([ao0, ao1, ao2, ao3]) {
                vertices.push(Vertex::terrain_face(
                    c,
                    n_up,
                    terrain_uv(c, glam::IVec3::Y),
                    tile,
                    color,
                    ao,
                ));
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);

            // Skirts where neighbor is lower (keeps cliffs from vanishing at HLOD).
            for (dx, dz, n, ni, a, b, c, d) in [
                (
                    0i32,
                    -1,
                    [0.0f32, 0.0, -1.0],
                    glam::IVec3::NEG_Z,
                    [xf, y, zf],
                    [xf + 1.0, y, zf],
                    [xf + 1.0, 0.0, zf],
                    [xf, 0.0, zf],
                ),
                (
                    0,
                    1,
                    [0.0, 0.0, 1.0],
                    glam::IVec3::Z,
                    [xf + 1.0, y, zf + 1.0],
                    [xf, y, zf + 1.0],
                    [xf, 0.0, zf + 1.0],
                    [xf + 1.0, 0.0, zf + 1.0],
                ),
                (
                    -1,
                    0,
                    [-1.0, 0.0, 0.0],
                    glam::IVec3::NEG_X,
                    [xf, y, zf + 1.0],
                    [xf, y, zf],
                    [xf, 0.0, zf],
                    [xf, 0.0, zf + 1.0],
                ),
                (
                    1,
                    0,
                    [1.0, 0.0, 0.0],
                    glam::IVec3::X,
                    [xf + 1.0, y, zf],
                    [xf + 1.0, y, zf + 1.0],
                    [xf + 1.0, 0.0, zf + 1.0],
                    [xf + 1.0, 0.0, zf],
                ),
            ] {
                let nh = hlod_column_height(world, x + dx, z + dz).unwrap_or(-1);
                if nh >= h {
                    continue;
                }
                // All 4 skirts emit; GPU + draw buckets cull backfaces.
                let skirt_y = (nh + 1).max(0) as f32;
                let mut ca = a;
                let mut cb = b;
                let mut cc = c;
                let mut cd = d;
                ca[1] = y;
                cb[1] = y;
                cc[1] = skirt_y;
                cd[1] = skirt_y;
                let sb = vertices.len() as u32;
                let skirt_tile = terrain_tile_for_block(world, glam::IVec3::new(x, h, z), ni);
                for p in [ca, cb, cc, cd] {
                    vertices.push(Vertex::terrain_face(
                        p,
                        n,
                        terrain_uv(p, ni),
                        skirt_tile,
                        skirt_color,
                        1.0,
                    ));
                }
                indices.extend_from_slice(&[sb, sb + 1, sb + 2, sb, sb + 2, sb + 3]);
            }
        }
    }
}

/// HLOD slab variant: same tops + skirts as [`heightmap_mesh_chunk`] clipped
/// to the 16-block section `[sy0, sy1)`. Stacked slabs tile into the full
/// column without gaps (top quads live in one slab, skirt bands are split).
fn heightmap_mesh_section(
    world: &World,
    cx: i32,
    cy: i32,
    cz: i32,
    _camera_pos: Vec3,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    water_vertices: &mut Vec<Vertex>,
    water_indices: &mut Vec<u32>,
) {
    use crate::world::{MESH_CHUNK_SIZE, MESH_SECTION_HEIGHT};

    let sy0 = cy * MESH_SECTION_HEIGHT;
    let sy1 = sy0 + MESH_SECTION_HEIGHT;
    let sy0f = sy0 as f32;
    let sy1f = sy1 as f32;

    let x0 = cx * MESH_CHUNK_SIZE;
    let z0 = cz * MESH_CHUNK_SIZE;

    for z in z0..z0 + MESH_CHUNK_SIZE {
        for x in x0..x0 + MESH_CHUNK_SIZE {
            let Some(h) = hlod_column_height(world, x, z) else {
                continue;
            };
            // Columna sumergida: el mar se ve a lo lejos como tapa de agua
            // (malla traslúcida) MÁS el lecho de tierra debajo. Antes el
            // `continue` saltaba tapa+skirts de tierra y la orilla quedaba con
            // una pared de una sola cara → agujero al mirar de lado/bajo.
            if h < crate::world::SEA_LEVEL {
                let wy = (crate::world::SEA_LEVEL + 1) as f32;
                if wy > sy0f && wy <= sy1f {
                    let base = water_vertices.len() as u32;
                    let mut tint = crate::world::Material::Water.color_rgb();
                    if crate::biomes::biome_at(x, z) == crate::biomes::BiomeId::Wetland {
                        tint = [0.36, 0.40, 0.20];
                    }
                    for c in [
                        [x as f32, wy, z as f32],
                        [x as f32, wy, z as f32 + 1.0],
                        [x as f32 + 1.0, wy, z as f32 + 1.0],
                        [x as f32 + 1.0, wy, z as f32],
                    ] {
                        water_vertices.push(Vertex::lit(c, [0.0, 1.0, 0.0], tint, 1.0));
                    }
                    water_indices.extend_from_slice(&[
                        base,
                        base + 1,
                        base + 2,
                        base,
                        base + 2,
                        base + 3,
                    ]);
                }
                // Sin `continue`: cae al lecho de tierra + skirts de abajo.
            }
            let skirt_color = crate::biomes::biome_at(x, z).dirt_rgb();
            let y = (h + 1) as f32;
            let xf = x as f32;
            let zf = z as f32;
            // Top lid belongs to the slab containing `h + 1` (always emits).
            if y > sy0f && y <= sy1f {
                let base = vertices.len() as u32;
                let c0 = [xf, y, zf];
                let c1 = [xf, y, zf + 1.0];
                let c2 = [xf + 1.0, y, zf + 1.0];
                let c3 = [xf + 1.0, y, zf];
                let block = glam::IVec3::new(x, h, z);
                let ao0 = 0.55 + 0.45 * corner_ao(world, block, glam::IVec3::Y, -1, -1);
                let ao1 = 0.55 + 0.45 * corner_ao(world, block, glam::IVec3::Y, -1, 1);
                let ao2 = 0.55 + 0.45 * corner_ao(world, block, glam::IVec3::Y, 1, 1);
                let ao3 = 0.55 + 0.45 * corner_ao(world, block, glam::IVec3::Y, 1, -1);
                let n_up = [0.0, 1.0, 0.0];
                let color = if crate::world::ENABLE_HD2D {
                    surface_albedo(world, x, z, h)
                } else {
                    skirt_color
                };
                let tile = terrain_tile_for_block(world, block, glam::IVec3::Y);
                for (c, ao) in [c0, c1, c2, c3].into_iter().zip([ao0, ao1, ao2, ao3]) {
                    vertices.push(Vertex::terrain_face(
                        c,
                        n_up,
                        terrain_uv(c, glam::IVec3::Y),
                        tile,
                        color,
                        ao,
                    ));
                }
                indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }

            // Skirts clipped to this slab.
            for (dx, dz, n, ni, a, b, c, d) in [
                (
                    0i32,
                    -1,
                    [0.0f32, 0.0, -1.0],
                    glam::IVec3::NEG_Z,
                    [xf, y, zf],
                    [xf + 1.0, y, zf],
                    [xf + 1.0, 0.0, zf],
                    [xf, 0.0, zf],
                ),
                (
                    0,
                    1,
                    [0.0, 0.0, 1.0],
                    glam::IVec3::Z,
                    [xf + 1.0, y, zf + 1.0],
                    [xf, y, zf + 1.0],
                    [xf, 0.0, zf + 1.0],
                    [xf + 1.0, 0.0, zf + 1.0],
                ),
                (
                    -1,
                    0,
                    [-1.0, 0.0, 0.0],
                    glam::IVec3::NEG_X,
                    [xf, y, zf + 1.0],
                    [xf, y, zf],
                    [xf, 0.0, zf],
                    [xf, 0.0, zf + 1.0],
                ),
                (
                    1,
                    0,
                    [1.0, 0.0, 0.0],
                    glam::IVec3::X,
                    [xf + 1.0, y, zf],
                    [xf + 1.0, y, zf + 1.0],
                    [xf + 1.0, 0.0, zf + 1.0],
                    [xf + 1.0, 0.0, zf],
                ),
            ] {
                let nh = hlod_column_height(world, x + dx, z + dz).unwrap_or(-1);
                if nh >= h {
                    continue;
                }
                let skirt_top = y.min(sy1f);
                let skirt_bot = (nh + 1).max(0) as f32;
                let skirt_bot = skirt_bot.max(sy0f);
                if skirt_top <= skirt_bot {
                    continue;
                }
                // All 4 skirts emit; GPU + draw buckets cull backfaces.
                let mut ca = a;
                let mut cb = b;
                let mut cc = c;
                let mut cd = d;
                ca[1] = skirt_top;
                cb[1] = skirt_top;
                cc[1] = skirt_bot;
                cd[1] = skirt_bot;
                let sb = vertices.len() as u32;
                let skirt_tile =
                    terrain_tile_for_block(world, glam::IVec3::new(x, h, z), ni);
                for p in [ca, cb, cc, cd] {
                    vertices.push(Vertex::terrain_face(
                        p,
                        n,
                        terrain_uv(p, ni),
                        skirt_tile,
                        skirt_color,
                        1.0,
                    ));
                }
                indices.extend_from_slice(&[sb, sb + 1, sb + 2, sb, sb + 2, sb + 3]);
            }
        }
    }
}

/// Build a quad with CCW winding when viewed along `normal` (outward).
/// Returns corners + whether U/V order was flipped (for matching AO remap).
fn greedy_face_corners(
    origin: [f32; 3],
    u_axis: usize,
    v_axis: usize,
    wu: f32,
    hv: f32,
    normal: Vec3,
) -> ([f32; 3], [f32; 3], [f32; 3], [f32; 3], bool) {
    let c0 = origin;
    let mut c1 = origin;
    let mut c2 = origin;
    let mut c3 = origin;
    c1[u_axis] += wu;
    c2[u_axis] += wu;
    c2[v_axis] += hv;
    c3[v_axis] += hv;

    // +U × +V must point along the outward normal. For Y slices (U=X,V=Z)
    // that cross is −Y, so +Y lids need a flip — the old "sum < 0" heuristic
    // got top/bottom inverted.
    let mut du = [0.0f32; 3];
    let mut dv = [0.0f32; 3];
    du[u_axis] = 1.0;
    dv[v_axis] = 1.0;
    let cross = Vec3::new(
        du[1] * dv[2] - du[2] * dv[1],
        du[2] * dv[0] - du[0] * dv[2],
        du[0] * dv[1] - du[1] * dv[0],
    );
    let flip = cross.dot(normal) < 0.0;
    if flip {
        (c0, c3, c2, c1, true)
    } else {
        (c0, c1, c2, c3, false)
    }
}

/// Altura de la superficie del agua (0..1 dentro de la celda): la fuente al
/// 100% y la corriente baja 25% por nivel hasta desaparecer en el 4º.
fn water_surface_h(level: u8) -> f32 {
    1.0 - 0.25 * level.min(4) as f32
}

/// Malla de agua por celda (sin merge): tapas en rampa del lado que conecta
/// con la fuente (100%) decayendo 25% por bloque hasta 0, laterales con el
/// borde alto escalonado y escalones entre niveles distintos. Nada de cubos.
fn mesh_water_section(
    world: &World,
    cx: i32,
    cz: i32,
    y0: i32,
    y1: i32,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use crate::world::MESH_CHUNK_SIZE;
    let x0 = cx * MESH_CHUNK_SIZE;
    let z0 = cz * MESH_CHUNK_SIZE;
    let mut quad = |corners: [[f32; 3]; 4], normal: [f32; 3], color: [f32; 3]| {
        let mut corners = corners;
        crate::hero::ensure_outward_quad(&mut corners, normal);
        let base = vertices.len() as u32;
        for p in &corners {
            vertices.push(Vertex::lit(*p, normal, color, 1.0));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    for y in y0..y1 {
        for z in z0..z0 + MESH_CHUNK_SIZE {
            for x in x0..x0 + MESH_CHUNK_SIZE {
                let pos = glam::IVec3::new(x, y, z);
                let Some(v) = world.get_voxel(pos) else {
                    continue;
                };
                if !v.is_fully_solid() || !v.material.is_water() {
                    continue;
                }
                let mut color = v.material.color_rgb();
                // Pantano: el agua luce sucia (tinte por bioma, no por material).
                if v.material == crate::world::Material::Water
                    && crate::biomes::biome_at(x, z) == crate::biomes::BiomeId::Wetland
                {
                    color = [0.36, 0.40, 0.20];
                }
                let level = world.water_level(pos).unwrap_or(0);
                let top = y as f32 + water_surface_h(level);                // Tapa (nivel 4 = altura 0: se omite).
                let above = glam::IVec3::new(x, y + 1, z);
                let above_water = world.get_voxel(above).is_some_and(|a| {
                    !a.is_empty() && a.material.is_water()
                });
                if !above_water && level < crate::world::WATER_FLOW_MAX {
                    let fx = x as f32;
                    let fz = z as f32;
                    quad(
                        [
                            [fx, top, fz],
                            [fx, top, fz + 1.0],
                            [fx + 1.0, top, fz + 1.0],
                            [fx + 1.0, top, fz],
                        ],
                        [0.0, 1.0, 0.0],
                        color,
                    );
                }
                // Laterales: al aire (borde escalonado) o escalón entre niveles.
                for (dx, dz, n) in [
                    (1, 0, [1.0f32, 0.0, 0.0]),
                    (-1, 0, [-1.0f32, 0.0, 0.0]),
                    (0, 1, [0.0f32, 0.0, 1.0]),
                    (0, -1, [0.0f32, 0.0, -1.0]),
                ] {
                    let np = glam::IVec3::new(x + dx, y, z + dz);
                    let nlevel = world.water_level(np);
                    if let Some(nl) = nlevel {
                        // Vecino agua: escalón solo si esta celda está más alta.
                        if nl >= level {
                            continue;
                        }
                        let ntop = y as f32 + water_surface_h(nl);
                        if my_top_le(top, ntop) {
                            continue;
                        }
                        let (fx0, fz0, fx1, fz1) = side_span(x, z, dx, dz);
                        quad(
                            [
                                [fx0, ntop, fz0],
                                [fx1, ntop, fz1],
                                [fx1, top, fz1],
                                [fx0, top, fz0],
                            ],
                            n,
                            color,
                        );
                        continue;
                    }
                    // Vecino no-agua: cara solo si hay aire (lo sólido oculta).
                    if world.get_voxel(np).is_some() {
                        continue;
                    }
                    let (fx0, fz0, fx1, fz1) = side_span(x, z, dx, dz);
                    let yb = y as f32;
                    quad(
                        [
                            [fx0, yb, fz0],
                            [fx1, yb, fz1],
                            [fx1, top, fz1],
                            [fx0, top, fz0],
                        ],
                        n,
                        color,
                    );
                }
            }
        }
    }
}

/// `true` si `a <= b` con epsilon (evita escalones degenerados).
fn my_top_le(a: f32, b: f32) -> bool {
    a <= b + 1e-4
}

/// Plano lateral de la celda en la dirección (`dx`, `dz`).
fn side_span(x: i32, z: i32, dx: i32, dz: i32) -> (f32, f32, f32, f32) {
    let fx = x as f32;
    let fz = z as f32;
    if dx == 1 {
        (fx + 1.0, fz, fx + 1.0, fz + 1.0)
    } else if dx == -1 {
        (fx, fz + 1.0, fx, fz)
    } else if dz == 1 {
        (fx + 1.0, fz + 1.0, fx, fz + 1.0)
    } else {
        (fx, fz, fx + 1.0, fz)
    }
}

fn build_section_mesh(
    world: &World,
    cx: i32,
    cy: i32,
    cz: i32,
    camera_pos: Vec3,
    grass_origin: Vec3,
) -> (
    Vec<Vertex>,
    Vec<u32>,
    Vec<GrassInstance>,
    Vec<Vertex>,
    Vec<u32>,
) {
    use crate::world::{chunk_dist_sq_xz, grass_density_for_dist_sq, mesh_section_range};

    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut grass = Vec::new();
    let mut water_vertices = Vec::new();
    let mut water_indices = Vec::new();

    // LOD distance uses grass_origin (= focus in HD-2D) so lens pull-in does not
    // thrash bands / empty the mesh cut. Face cull still uses camera_pos.
    let dist_sq = chunk_dist_sq_xz(grass_origin, cx, cz);
    let band = chunk_distance_band(dist_sq);
    if band >= 4 {
        return (vertices, indices, grass, water_vertices, water_indices);
    }
    // Empty upper slabs (air above the treetops) cost nothing: no scan, no GPU.
    if !world.section_may_have_content(cx, cz, cy) {
        return (vertices, indices, grass, water_vertices, water_indices);
    }

    // Density from player focus in HD-2D — camera zoom must not strip near grass.
    // (Diagnostic `DEBUG_DISABLE_CULLING` forces full density everywhere.)
    let grass_density = if crate::world::DEBUG_DISABLE_CULLING {
        1.0
    } else if band >= 3 {
        0.0
    } else {
        grass_density_for_dist_sq(chunk_dist_sq_xz(grass_origin, cx, cz))
    };

    match band {
        0 | 1 | 2 => {
            let (y0, y1) = mesh_section_range(cy);
            // Greedy solids at every near/mid band — per-block tops painted as
            // "grass" used to look like a separate jagged shell over the hill.
            greedy_mesh_solid_dirt(
                world,
                cx,
                cz,
                y0,
                y1,
                band,
                camera_pos,
                &mut vertices,
                &mut indices,
            );
            // Agua en rampa (malla traslúcida propia, sin cubos).
            mesh_water_section(
                world,
                cx,
                cz,
                y0,
                y1,
                &mut water_vertices,
                &mut water_indices,
            );
            // Coal / crystal flecks on exposed stone.
            if band <= 2 {
                mesh_ore_overlays_section(world, cx, cy, cz, camera_pos, &mut vertices, &mut indices);
            }
            world.for_voxels_in_section(cx, cy, cz, &mut |pos, voxel| {
                use crate::world::Material;
                let is_partial_dirt = voxel.material == Material::Dirt && !voxel.is_fully_solid();
                let is_tree = voxel.material.is_tree_bark() || voxel.material.is_tree_foliage();
                if is_partial_dirt || is_tree || voxel.material == Material::Grass {
                    mesh_one_voxel(
                        world,
                        pos,
                        voxel,
                        camera_pos,
                        &mut vertices,
                        &mut indices,
                        &mut grass,
                        band == 0,
                        grass_density,
                    );
                }
            });
        }
        _ => {
            // Band 3: heightmap HLOD slab — tops + skirts clipped to this section.
            heightmap_mesh_section(
                world,
                cx,
                cy,
                cz,
                camera_pos,
                &mut vertices,
                &mut indices,
                &mut water_vertices,
                &mut water_indices,
            );
        }
    }

    (vertices, indices, grass, water_vertices, water_indices)
}

fn build_chunk_mesh(
    world: &World,
    cx: i32,
    cz: i32,
    camera_pos: Vec3,
    grass_origin: Vec3,
) -> (
    Vec<Vertex>,
    Vec<u32>,
    Vec<GrassInstance>,
    Vec<Vertex>,
    Vec<u32>,
) {
    use crate::world::{chunk_dist_sq_xz, grass_density_for_dist_sq};

    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut grass = Vec::new();
    let mut water_vertices = Vec::new();
    let mut water_indices = Vec::new();

    // LOD distance uses grass_origin (= focus in HD-2D) so lens pull-in does not
    // thrash bands / empty the mesh cut. Face cull still uses camera_pos.
    let dist_sq = chunk_dist_sq_xz(grass_origin, cx, cz);
    let band = chunk_distance_band(dist_sq);
    if band >= 4 {
        return (vertices, indices, grass, water_vertices, water_indices);
    }

    // Density from player focus in HD-2D — camera zoom must not strip near grass.
    // (Diagnostic `DEBUG_DISABLE_CULLING` forces full density everywhere.)
    let grass_density = if crate::world::DEBUG_DISABLE_CULLING {
        1.0
    } else if band >= 3 {
        0.0
    } else {
        grass_density_for_dist_sq(chunk_dist_sq_xz(grass_origin, cx, cz))
    };

    match band {
        0 | 1 | 2 => {
            // Greedy solids at every near/mid band — per-block tops painted as
            // "grass" used to look like a separate jagged shell over the hill.
            greedy_mesh_solid_dirt(
                world,
                cx,
                cz,
                0,
                crate::world::WORLD_MAX_HEIGHT,
                band,
                camera_pos,
                &mut vertices,
                &mut indices,
            );
            // Agua en rampa (malla traslúcida propia, sin cubos).
            mesh_water_section(
                world,
                cx,
                cz,
                0,
                crate::world::WORLD_MAX_HEIGHT,
                &mut water_vertices,
                &mut water_indices,
            );
            // Coal / crystal flecks on exposed stone.
            if band <= 2 {
                mesh_ore_overlays(world, cx, cz, camera_pos, &mut vertices, &mut indices);
            }
            world.for_voxels_in_chunk(cx, cz, &mut |pos, voxel| {
                use crate::world::Material;
                let is_partial_dirt = voxel.material == Material::Dirt && !voxel.is_fully_solid();
                let is_tree = voxel.material.is_tree_bark() || voxel.material.is_tree_foliage();
                if is_partial_dirt || is_tree || voxel.material == Material::Grass {
                    mesh_one_voxel(
                        world,
                        pos,
                        voxel,
                        camera_pos,
                        &mut vertices,
                        &mut indices,
                        &mut grass,
                        band == 0,
                        grass_density,
                    );
                }
            });
        }
        _ => {
            // Band 3: heightmap HLOD — continuous surface + skirts.
            heightmap_mesh_chunk(world, cx, cz, camera_pos, &mut vertices, &mut indices);
        }
    }

    (vertices, indices, grass, water_vertices, water_indices)
}

/// Full visible mesh (unit tests).
fn build_mesh(world: &World, camera_pos: Vec3) -> (Vec<Vertex>, Vec<u32>) {
    use crate::world::dirt_mesh_max_dist;
    let visible = world.chunk_coords_near(camera_pos, dirt_mesh_max_dist());
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (cx, cz) in visible {
        let (cv, ci, grass, _, _) = build_chunk_mesh(world, cx, cz, camera_pos, camera_pos);
        let base = vertices.len() as u32;
        vertices.extend(cv);
        indices.extend(ci.into_iter().map(|i| i + base));
        #[cfg(test)]
        for g in &grass {
            push_grass_carpet_quads(
                &mut vertices,
                &mut indices,
                Vec3::from_array(g.origin),
                g.color,
                camera_pos,
            );
        }
        #[cfg(not(test))]
        let _ = grass;
    }

    debug_assert!(
        vertices.len() as u64 <= MAX_VERTICES,
        "vertex overflow: {}",
        vertices.len()
    );
    debug_assert!(
        indices.len() as u64 <= MAX_INDICES,
        "index overflow: {}",
        indices.len()
    );

    (vertices, indices)
}

/// Unit carpet in local space (origin = block corner); instances add world origin.
fn grass_carpet_template() -> (Vec<Vertex>, Vec<u32>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    push_grass_carpet_quads(
        &mut vertices,
        &mut indices,
        Vec3::ZERO,
        [1.0, 1.0, 1.0],
        Vec3::ZERO,
    );
    (vertices, indices)
}

/// Flat colored block faces (legacy / debug). Prefer [`push_textured_block_faces`].
#[allow(dead_code)]
fn push_dirt_flat_faces(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    origin: Vec3,
    block_pos: glam::IVec3,
    world: &World,
    color: [f32; 3],
    _camera_pos: Vec3,
) {
    for &(nx, ny, nz, corners) in &crate::hero::FACE_CORNERS {
        let neighbor = glam::IVec3::new(nx, ny, nz);
        if world.dirt_face_occluded(block_pos, neighbor) {
            continue;
        }
        let n = [nx as f32, ny as f32, nz as f32];
        let mut world_c = [[0.0f32; 3]; 4];
        for (i, c) in corners.iter().enumerate() {
            world_c[i] = [origin.x + c[0], origin.y + c[1], origin.z + c[2]];
        }
        crate::hero::ensure_outward_quad(&mut world_c, n);
        let base = vertices.len() as u32;
        for p in &world_c {
            vertices.push(Vertex::solid(*p, n, color));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Atlas-textured solid cube (leaves canopy, solid wood, etc.).
fn push_textured_block_faces(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    origin: Vec3,
    block_pos: glam::IVec3,
    world: &World,
    tile: f32,
    tint: [f32; 3],
    _camera_pos: Vec3,
) {
    for &(nx, ny, nz, corners) in &crate::hero::FACE_CORNERS {
        let neighbor = glam::IVec3::new(nx, ny, nz);
        if world.dirt_face_occluded(block_pos, neighbor) {
            continue;
        }
        let n = [nx as f32, ny as f32, nz as f32];
        let mut world_c = [[0.0f32; 3]; 4];
        for (i, c) in corners.iter().enumerate() {
            world_c[i] = [origin.x + c[0], origin.y + c[1], origin.z + c[2]];
        }
        crate::hero::ensure_outward_quad(&mut world_c, n);
        let base = vertices.len() as u32;
        for (i, pos) in world_c.iter().enumerate() {
            let (su, sv) = match i {
                0 => (-1, -1),
                1 => (1, -1),
                2 => (1, 1),
                _ => (-1, 1),
            };
            let ao = corner_ao(world, block_pos, neighbor, su, sv);
            vertices.push(Vertex::terrain_face(
                *pos,
                n,
                terrain_uv(*pos, neighbor),
                tile,
                tint,
                ao,
            ));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Near dirt: flat material colors (green top, brown sides).
fn push_dirt_face_detail(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    origin: Vec3,
    block_pos: glam::IVec3,
    world: &World,
    _camera_pos: Vec3,
) {
    for &(nx, ny, nz, corners) in &crate::hero::FACE_CORNERS {
        let neighbor = glam::IVec3::new(nx, ny, nz);
        if world.dirt_face_occluded(block_pos, neighbor) {
            continue;
        }
        let top_green = neighbor == glam::IVec3::Y && dirt_top_is_grassy(world, block_pos);
        let tile = terrain_tile_for_block(world, block_pos, neighbor);
        let n = [nx as f32, ny as f32, nz as f32];
        let mut world_c = [[0.0f32; 3]; 4];
        for (i, c) in corners.iter().enumerate() {
            world_c[i] = [origin.x + c[0], origin.y + c[1], origin.z + c[2]];
        }
        crate::hero::ensure_outward_quad(&mut world_c, n);
        let base = vertices.len() as u32;
        for (i, pos) in world_c.iter().enumerate() {
            let (su, sv) = match i {
                0 => (-1, -1),
                1 => (1, -1),
                2 => (1, 1),
                _ => (-1, 1),
            };
            let mut ao = corner_ao(world, block_pos, neighbor, su, sv);
            if top_green {
                ao = 0.55 + 0.45 * ao;
            }
            if crate::world::DEBUG_FACE_VISIBILITY && !cfg!(test) {
                vertices.push(Vertex::flat_face(*pos, n, top_green, ao));
            } else {
                use crate::biomes::biome_at;
                use crate::world::Material;
                let biome = biome_at(block_pos.x, block_pos.z);
                let tint = if top_green {
                    biome.grass_rgb()
                } else if matches!(
                    world.get_voxel(block_pos).map(|v| v.material),
                    Some(Material::Stone)
                ) {
                    Material::Stone.color_rgb()
                } else if matches!(
                    world.get_voxel(block_pos).map(|v| v.material),
                    Some(Material::Mud)
                ) {
                    // Lodo húmedo: tono propio, no tierra del bioma.
                    Material::Mud.color_rgb()
                } else {
                    biome.dirt_rgb()
                };
                vertices.push(Vertex::terrain_face(
                    *pos,
                    n,
                    terrain_uv(*pos, neighbor),
                    tile,
                    tint,
                    ao,
                ));
            }
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Crossed vertical grass billboards (UV-mapped to the grass2–7 atlas)
/// plus a cheap elliptical contact blob (no shadow-map casting).
fn push_grass_carpet_quads(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    origin: Vec3,
    color: [f32; 3],
    _camera_pos: Vec3,
) {
    let h = 28.0 / 32.0; // typical grass sprite aspect
    let mid = 0.5f32;
    // Quad A: spans X, faces ±Z. Quad B: spans Z, faces ±X.
    let quads: [([f32; 3], [[f32; 3]; 4]); 2] = [
        (
            [0.0, 0.0, 1.0],
            [
                [0.0, 0.0, mid],
                [1.0, 0.0, mid],
                [1.0, h, mid],
                [0.0, h, mid],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [mid, 0.0, 1.0],
                [mid, 0.0, 0.0],
                [mid, h, 0.0],
                [mid, h, 1.0],
            ],
        ),
    ];
    for (n, corners) in quads {
        let base = vertices.len() as u32;
        // v=1 at quad bottom so PNG roots sit on the dirt top.
        let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        for (i, c) in corners.into_iter().enumerate() {
            vertices.push(Vertex {
                position: [origin.x + c[0], origin.y + c[1], origin.z + c[2]],
                normal: n,
                color,
                uv: uvs[i],
                flags: 0.0,
                seed: 0.0,
                ao: 1.0,
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    // Soft elliptical blob under the tuft (light-biased on XZ). Cheap “sphere” shadow.
    let sy = 0.015f32;
    let cx = 0.5 - 0.08; // toward light (−X, −Z)
    let cz = 0.5 - 0.04;
    let rx = 0.38f32;
    let rz = 0.30f32;
    let blob_color = [0.05, 0.06, 0.07];
    let n_up = [0.0, 1.0, 0.0];
    let base = vertices.len() as u32;
    let corners = [
        ([cx - rx, sy, cz - rz], [0.0, 0.0]),
        ([cx - rx, sy, cz + rz], [0.0, 1.0]),
        ([cx + rx, sy, cz + rz], [1.0, 1.0]),
        ([cx + rx, sy, cz - rz], [1.0, 0.0]),
    ];
    for (c, uv) in corners {
        vertices.push(Vertex {
            position: [origin.x + c[0], origin.y + c[1], origin.z + c[2]],
            normal: n_up,
            color: blob_color,
            uv,
            flags: 1.0, // fs_grass: circular discard path
            seed: 0.0,
            ao: 1.0,
        });
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// Draw 1–2 ore flecks (coal / crystals) on exposed stone.
fn mesh_ore_overlays(
    world: &World,
    cx: i32,
    cz: i32,
    camera_pos: Vec3,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use crate::world::{chunk_dist_sq_xz, ore_micros_visible, Material, MICROVOXEL_RES, ORE_FLECK_SIZE_MUL};

    let cell = 1.0 / MICROVOXEL_RES as f32;
    let size = cell * ORE_FLECK_SIZE_MUL;

    // Skip ore flecks beyond 48 blocks from the camera — too small to see.
    let dist_sq = chunk_dist_sq_xz(camera_pos, cx, cz);
    if dist_sq > 48.0 * 48.0 {
        return;
    }

    world.for_voxels_in_chunk(cx, cz, &mut |pos, voxel| {
        if voxel.material != Material::Stone || !voxel.is_fully_solid() {
            return;
        }
        let Some((kind, n, bits)) = ore_micros_visible(world, *pos) else {
            return;
        };
        let color = kind.fleck_rgb();
        let origin = Vec3::new(pos.x as f32, pos.y as f32, pos.z as f32);
        for i in 0..n as usize {
            let m = bits[i];
            let edge = (MICROVOXEL_RES - 1) as u8;
            let mut push = Vec3::ZERO;
            if m.mx == edge {
                push.x = 1.0;
            } else if m.mx == 0 {
                push.x = -1.0;
            }
            if m.my == edge {
                push.y = 1.0;
            } else if m.my == 0 {
                push.y = -1.0;
            }
            if m.mz == edge {
                push.z = 1.0;
            } else if m.mz == 0 {
                push.z = -1.0;
            }
            // Anchor on the micro cell, grow the fleck, then push it out of the face.
            let micro_origin = origin
                + Vec3::new(m.mx as f32, m.my as f32, m.mz as f32) * cell
                + Vec3::splat((cell - size) * 0.5)
                + push * (size * 0.35);
            push_ore_fleck_cube(
                vertices,
                indices,
                micro_origin,
                size,
                color,
                camera_pos,
                push,
            );
        }
    });
}

/// Section variant of [`mesh_ore_overlays`]: flecks only inside slab `cy`.
fn mesh_ore_overlays_section(
    world: &World,
    cx: i32,
    cy: i32,
    cz: i32,
    camera_pos: Vec3,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
) {
    use crate::world::{chunk_dist_sq_xz, ore_micros_visible, Material, MICROVOXEL_RES, ORE_FLECK_SIZE_MUL};

    let cell = 1.0 / MICROVOXEL_RES as f32;
    let size = cell * ORE_FLECK_SIZE_MUL;

    // Skip ore flecks beyond 48 blocks from the camera — too small to see.
    let dist_sq = chunk_dist_sq_xz(camera_pos, cx, cz);
    if dist_sq > 48.0 * 48.0 {
        return;
    }

    world.for_voxels_in_section(cx, cy, cz, &mut |pos, voxel| {
        if voxel.material != Material::Stone || !voxel.is_fully_solid() {
            return;
        }
        let Some((kind, n, bits)) = ore_micros_visible(world, *pos) else {
            return;
        };
        let color = kind.fleck_rgb();
        let origin = Vec3::new(pos.x as f32, pos.y as f32, pos.z as f32);
        for i in 0..n as usize {
            let m = bits[i];
            let edge = (MICROVOXEL_RES - 1) as u8;
            let mut push = Vec3::ZERO;
            if m.mx == edge {
                push.x = 1.0;
            } else if m.mx == 0 {
                push.x = -1.0;
            }
            if m.my == edge {
                push.y = 1.0;
            } else if m.my == 0 {
                push.y = -1.0;
            }
            if m.mz == edge {
                push.z = 1.0;
            } else if m.mz == 0 {
                push.z = -1.0;
            }
            // Anchor on the micro cell, grow the fleck, then push it out of the face.
            let micro_origin = origin
                + Vec3::new(m.mx as f32, m.my as f32, m.mz as f32) * cell
                + Vec3::splat((cell - size) * 0.5)
                + push * (size * 0.35);
            push_ore_fleck_cube(
                vertices,
                indices,
                micro_origin,
                size,
                color,
                camera_pos,
                push,
            );
        }
    });
}

/// Protruding ore fleck — outward + side faces (back against stone).
fn push_ore_fleck_cube(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    origin: Vec3,
    size: f32,
    color: [f32; 3],
    _camera_pos: Vec3,
    outward: Vec3,
) {
    for &(nx, ny, nz, corners) in &crate::hero::FACE_CORNERS {
        let normal = [nx as f32, ny as f32, nz as f32];
        // Hide the face pressed into the parent stone.
        if outward.dot(Vec3::new(nx as f32, ny as f32, nz as f32)) < -0.5 {
            continue;
        }
        let mut world_c = [[0.0f32; 3]; 4];
        for (i, c) in corners.iter().enumerate() {
            world_c[i] = [
                origin.x + c[0] * size,
                origin.y + c[1] * size,
                origin.z + c[2] * size,
            ];
        }
        crate::hero::ensure_outward_quad(&mut world_c, normal);
        let base = vertices.len() as u32;
        for p in &world_c {
            vertices.push(Vertex::solid(*p, normal, color));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// A merged group of microvoxels inside one voxel, as inclusive cell ranges.
/// This is the "stretchable piece" the mesh works with: N cells in a run become
/// one box, so a stretched face costs 1 quad instead of N.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MicroRun {
    x0: usize,
    y0: usize,
    z0: usize,
    x1: usize,
    y1: usize,
    z1: usize,
}

/// One exposed face of a [`MicroRun`]: the plane it sits on (`cell` along
/// `axis`) and which way it points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MicroFace {
    run: MicroRun,
    axis: usize,
    positive: bool,
    cell: usize,
}

impl MicroFace {
    /// Area of the face in whole cells — the invariant that says merging only
    /// *grouped* exposed faces and never added or dropped one. Test-only: the
    /// mesher itself works from the geometry, not from the count.
    #[cfg(test)]
    fn area(&self) -> usize {
        let (u, v) = ((self.axis + 1) % 3, (self.axis + 2) % 3);
        let lo = [self.run.x0, self.run.y0, self.run.z0];
        let hi = [self.run.x1, self.run.y1, self.run.z1];
        (hi[u] - lo[u] + 1) * (hi[v] - lo[v] + 1)
    }

    /// True when the face steps out of the voxel, so the neighbour *block*
    /// decides whether it shows. The whole run shares one plane, so the test is
    /// uniform across the merged face.
    fn leaves_voxel(&self) -> bool {
        use crate::world::MICROVOXEL_RES;
        if self.positive {
            self.cell + 1 >= MICROVOXEL_RES
        } else {
            self.cell == 0
        }
    }

    fn block_delta(&self) -> glam::IVec3 {
        let mut d = [0i32; 3];
        d[self.axis] = if self.positive { 1 } else { -1 };
        glam::IVec3::new(d[0], d[1], d[2])
    }
}

/// Occupancy of one voxel's micro cells, in the same order as
/// `Voxel::micro_index`.
type MicroMask = [bool; crate::world::MICROVOXEL_RES * crate::world::MICROVOXEL_RES
    * crate::world::MICROVOXEL_RES];

/// Snapshot the micro mask with the LOD rules applied (`max_blade_y`,
/// `solid_carpet`), so the six face masks below read one array instead of
/// re-testing the voxel 6 × 4096 times.
fn micro_mask(voxel: &crate::world::Voxel, lod: crate::world::GrassLod) -> MicroMask {
    use crate::world::MICROVOXEL_RES;
    let mut mask = [false; MICROVOXEL_RES * MICROVOXEL_RES * MICROVOXEL_RES];
    for mz in 0..MICROVOXEL_RES {
        for my in 0..MICROVOXEL_RES {
            for mx in 0..MICROVOXEL_RES {
                let i = mx + my * MICROVOXEL_RES + mz * MICROVOXEL_RES * MICROVOXEL_RES;
                mask[i] = micro_present(voxel, mx as i32, my as i32, mz as i32, lod);
            }
        }
    }
    mask
}

/// Greedy-merge one voxel's exposed micro faces into the fewest rectangles.
///
/// Pure: it only knows the mask, so it cannot cull against the neighbour
/// *block* (the caller does that per merged face via [`MicroFace::leaves_voxel`]).
/// The exposed **set** is identical to the per-cell path — only the grouping
/// changes — which is what `greedy_micro_faces_preserve_exposed_area` pins.
fn greedy_micro_faces(mask: &MicroMask) -> Vec<MicroFace> {
    use crate::world::MICROVOXEL_RES;
    let n = MICROVOXEL_RES;
    let at = |x: i32, y: i32, z: i32| -> bool {
        if x < 0 || y < 0 || z < 0 {
            return false;
        }
        let (x, y, z) = (x as usize, y as usize, z as usize);
        if x >= n || y >= n || z >= n {
            return false;
        }
        mask[x + y * n + z * n * n]
    };
    let mut out = Vec::new();
    for axis in 0..3usize {
        let (u_axis, v_axis) = ((axis + 1) % 3, (axis + 2) % 3);
        for positive in [true, false] {
            for cell in 0..n {
                // Exposed along `axis` at plane `cell`?
                let step = if positive { 1i32 } else { -1i32 };
                let visible = |u: usize, v: usize| -> bool {
                    let mut c = [0i32; 3];
                    c[axis] = cell as i32;
                    c[u_axis] = u as i32;
                    c[v_axis] = v as i32;
                    if !at(c[0], c[1], c[2]) {
                        return false;
                    }
                    let mut nb = c;
                    nb[axis] += step;
                    !at(nb[0], nb[1], nb[2])
                };
                // Merge the n×n mask: runs along v, then carried across u while
                // the v-range matches exactly.
                let mut rects: Vec<([usize; 3], [usize; 3])> = Vec::new();
                // (v0, v1, u_start, rect index) still extendable on this row.
                let mut open: Vec<(usize, usize, usize, usize)> = Vec::new();
                for u in 0..n {
                    let mut next_open: Vec<(usize, usize, usize, usize)> = Vec::new();
                    let mut v = 0;
                    while v < n {
                        if !visible(u, v) {
                            v += 1;
                            continue;
                        }
                        let mut v1 = v;
                        while v1 + 1 < n && visible(u, v1 + 1) {
                            v1 += 1;
                        }
                        match open.iter().position(|(a, b, _, _)| *a == v && *b == v1) {
                            Some(k) => {
                                let (_, _, u_start, idx) = open[k];
                                let (lo, mut hi) = rects[idx];
                                hi[u_axis] = u;
                                rects[idx] = (lo, hi);
                                next_open.push((v, v1, u_start, idx));
                            }
                            None => {
                                let mut lo = [0usize; 3];
                                let mut hi = [0usize; 3];
                                lo[axis] = cell;
                                hi[axis] = cell;
                                lo[u_axis] = u;
                                hi[u_axis] = u;
                                lo[v_axis] = v;
                                hi[v_axis] = v1;
                                rects.push((lo, hi));
                                next_open.push((v, v1, u, rects.len() - 1));
                            }
                        }
                        v = v1 + 1;
                    }
                    open = next_open;
                }
                for (lo, hi) in rects {
                    out.push(MicroFace {
                        run: MicroRun {
                            x0: lo[0],
                            y0: lo[1],
                            z0: lo[2],
                            x1: hi[0],
                            y1: hi[1],
                            z1: hi[2],
                        },
                        axis,
                        positive,
                        cell,
                    });
                }
            }
        }
    }
    out
}

/// World quad for one merged micro face. The winding is fixed afterwards by
/// `ensure_outward_quad`, exactly like the per-cell path.
fn push_micro_run_quad(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    origin: Vec3,
    scale: f32,
    color: [f32; 3],
    face: &MicroFace,
) {
    let lo = [face.run.x0 as f32, face.run.y0 as f32, face.run.z0 as f32];
    let hi = [
        face.run.x1 as f32 + 1.0,
        face.run.y1 as f32 + 1.0,
        face.run.z1 as f32 + 1.0,
    ];
    let (a, b) = ((face.axis + 1) % 3, (face.axis + 2) % 3);
    let plane = if face.positive { hi[face.axis] } else { lo[face.axis] };
    let mut corners = [[0.0f32; 3]; 4];
    // Same corner order as FACE_CORNERS: (0,0) (1,0) (1,1) (0,1) on (a, b).
    for (i, (da, db)) in [(0.0f32, 0.0f32), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
        .into_iter()
        .enumerate()
    {
        let mut p = [0.0f32; 3];
        p[face.axis] = plane;
        p[a] = lo[a] + da * (hi[a] - lo[a]);
        p[b] = lo[b] + db * (hi[b] - lo[b]);
        corners[i] = [
            origin.x + p[0] * scale,
            origin.y + p[1] * scale,
            origin.z + p[2] * scale,
        ];
    }
    let mut normal = [0.0f32; 3];
    normal[face.axis] = if face.positive { 1.0 } else { -1.0 };
    crate::hero::ensure_outward_quad(&mut corners, normal);
    let base = vertices.len() as u32;
    for p in &corners {
        vertices.push(Vertex::solid(*p, normal, color));
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn push_microvoxel_mesh(
    world: &World,
    block_pos: glam::IVec3,
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    origin: Vec3,
    voxel: &crate::world::Voxel,
    color: [f32; 3],
    camera_pos: Vec3,
    lod: crate::world::GrassLod,
    atlas_tile: Option<f32>,
) {
    use crate::world::MICROVOXEL_RES;
    let scale = 1.0 / MICROVOXEL_RES as f32;

    // Atlas-textured micro geometry (tree bark) keeps one quad per cell on
    // purpose: its UVs come from world position (`terrain_uv`), so merging a
    // run would stretch the grain instead of tiling it. Untextured micro
    // geometry (grass / dirt detail) has no UVs and merges freely.
    if atlas_tile.is_some() {
        for mz in 0..MICROVOXEL_RES {
            for my in 0..MICROVOXEL_RES {
                if let Some(max_y) = lod.max_blade_y {
                    if my > max_y {
                        continue;
                    }
                } else if my > 0 {
                    continue;
                }
                for mx in 0..MICROVOXEL_RES {
                    let occupied = if lod.solid_carpet && my == 0 {
                        true
                    } else {
                        voxel.get_micro(mx, my, mz)
                    };
                    if !occupied {
                        continue;
                    }
                    let micro_origin =
                        origin + Vec3::new(mx as f32, my as f32, mz as f32) * scale;
                    push_exposed_micro_faces(
                        vertices,
                        indices,
                        micro_origin,
                        scale,
                        color,
                        camera_pos,
                        voxel,
                        mx,
                        my,
                        mz,
                        lod,
                        block_pos,
                        world,
                        atlas_tile,
                    );
                }
            }
        }
        return;
    }

    let mask = micro_mask(voxel, lod);
    for face in greedy_micro_faces(&mask) {
        if face.leaves_voxel() && world.solid_occludes(block_pos + face.block_delta()) {
            continue;
        }
        push_micro_run_quad(vertices, indices, origin, scale, color, &face);
    }
}

fn micro_present(
    voxel: &crate::world::Voxel,
    mx: i32,
    my: i32,
    mz: i32,
    lod: crate::world::GrassLod,
) -> bool {
    use crate::world::MICROVOXEL_RES;
    if mx < 0 || my < 0 || mz < 0 {
        return false;
    }
    let (mx, my, mz) = (mx as usize, my as usize, mz as usize);
    if mx >= MICROVOXEL_RES || my >= MICROVOXEL_RES || mz >= MICROVOXEL_RES {
        return false;
    }
    if let Some(max_y) = lod.max_blade_y {
        if my > max_y {
            return false;
        }
    } else if my > 0 {
        return false;
    }
    if lod.solid_carpet && my == 0 {
        return true;
    }
    voxel.get_micro(mx, my, mz)
}

fn push_exposed_micro_faces(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    origin: Vec3,
    size: f32,
    color: [f32; 3],
    _camera_pos: Vec3,
    voxel: &crate::world::Voxel,
    mx: usize,
    my: usize,
    mz: usize,
    lod: crate::world::GrassLod,
    block_pos: glam::IVec3,
    world: &World,
    atlas_tile: Option<f32>,
) {
    use crate::world::MICROVOXEL_RES;
    for &(nx, ny, nz, corners) in &crate::hero::FACE_CORNERS {
        let delta = [nx, ny, nz];
        let normal = [nx as f32, ny as f32, nz as f32];
        let neighbor = glam::IVec3::new(nx, ny, nz);
        let mx_i = mx as i32 + delta[0];
        let my_i = my as i32 + delta[1];
        let mz_i = mz as i32 + delta[2];
        // Hidden by another micro inside this voxel.
        if micro_present(voxel, mx_i, my_i, mz_i, lod) {
            continue;
        }
        // Stepped outside this voxel: hide if the neighboring world block is solid.
        let outside = mx_i < 0
            || my_i < 0
            || mz_i < 0
            || mx_i >= MICROVOXEL_RES as i32
            || my_i >= MICROVOXEL_RES as i32
            || mz_i >= MICROVOXEL_RES as i32;
        if outside {
            let nblock = block_pos + glam::IVec3::new(delta[0], delta[1], delta[2]);
            if world.solid_occludes(nblock) {
                continue;
            }
        }
        let mut world_c = [[0.0f32; 3]; 4];
        for (i, c) in corners.iter().enumerate() {
            world_c[i] = [
                origin.x + c[0] * size,
                origin.y + c[1] * size,
                origin.z + c[2] * size,
            ];
        }
        crate::hero::ensure_outward_quad(&mut world_c, normal);
        let base = vertices.len() as u32;
        for p in &world_c {
            if let Some(tile) = atlas_tile {
                vertices.push(Vertex::terrain_face(
                    *p,
                    normal,
                    terrain_uv(*p, neighbor),
                    tile,
                    color,
                    1.0,
                ));
            } else {
                vertices.push(Vertex::solid(*p, normal, color));
            }
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

#[allow(dead_code)]
fn push_camera_facing_box(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    origin: Vec3,
    size: f32,
    color: [f32; 3],
    _camera_pos: Vec3,
) {
    for &(nx, ny, nz, corners) in &crate::hero::FACE_CORNERS {
        let normal = [nx as f32, ny as f32, nz as f32];
        let mut world_c = [[0.0f32; 3]; 4];
        for (i, c) in corners.iter().enumerate() {
            world_c[i] = [
                origin.x + c[0] * size,
                origin.y + c[1] * size,
                origin.z + c[2] * size,
            ];
        }
        crate::hero::ensure_outward_quad(&mut world_c, normal);
        let base = vertices.len() as u32;
        for p in &world_c {
            vertices.push(Vertex::solid(*p, normal, color));
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

#[cfg(test)]
mod settlement_surface_tests {
    use super::*;
    use crate::realms::{realm_info, RealmId};
    use crate::settlements::{plans_for_realm, SettlementKind, VILLAGE_WALL_RADIUS};
    use crate::world::{terrain_height, Material, World, MESH_CHUNK_SIZE};
    use glam::IVec3;

    fn village_center() -> (i32, i32) {
        for iz in -8..8 {
            for ix in -8..8 {
                let Some(info) = realm_info(RealmId { ix, iz }) else {
                    continue;
                };
                if let Some(v) = plans_for_realm(&info)
                    .iter()
                    .find(|p| p.kind == SettlementKind::Village)
                {
                    return v.center_block;
                }
            }
        }
        panic!("aldea no encontrada");
    }

    fn world_around(x: i32, z: i32) -> World {
        let mut world = World::new();
        let (cx, cz) = crate::world::mesh_chunk_coord(x, z);
        let window = VILLAGE_WALL_RADIUS / MESH_CHUNK_SIZE + 2;
        for dz in -window..=window {
            for dx in -window..=window {
                let x0 = (cx + dx) * MESH_CHUNK_SIZE;
                let z0 = (cz + dz) * MESH_CHUNK_SIZE;
                for zz in z0..z0 + MESH_CHUNK_SIZE {
                    for xx in x0..x0 + MESH_CHUNK_SIZE {
                        if world.column_height(xx, zz).is_none() {
                            world.fills_column_for_test(xx, zz, terrain_height(xx, zz));
                        }
                    }
                }
            }
        }
        world
    }

    #[test]
    fn village_ground_is_dirt_not_grass() {
        let (vx, vz) = village_center();
        let world = world_around(vx, vz);
        let mut checked = 0usize;
        let r = VILLAGE_WALL_RADIUS - 2;
        for z in vz - r..=vz + r {
            for x in vx - r..=vx + r {
                let h = terrain_height(x, z);
                if !matches!(
                    world.get_voxel(IVec3::new(x, h, z)).map(|v| v.material),
                    Some(Material::Dirt)
                ) {
                    continue;
                }
                checked += 1;
                assert!(
                    !dirt_top_is_grassy(&world, IVec3::new(x, h, z)),
                    "tapa de pasto dentro de la aldea en {x},{h},{z}"
                );
                let dirt = crate::biomes::biome_at(x, z).dirt_rgb();
                let got = surface_albedo(&world, x, z, h);
                for (i, c) in [0usize, 1, 2].into_iter().enumerate() {
                    assert!(
                        (got[i] - dirt[i]).abs() < 1e-5,
                        "albedo verde dentro de la aldea en {x},{h},{z}: {got:?} vs dirt {dirt:?}"
                    );
                }
            }
        }
        assert!(checked > 100, "solo se revisaron {checked} columnas");
    }

    #[test]
    fn wilderness_ground_stays_grassy() {
        let (vx, vz) = village_center();
        let mut world = world_around(vx, vz);
        // Offset well clear of the claim box (radius + 2 margin).
        let (wx, wz) = (vx + VILLAGE_WALL_RADIUS + 24, vz);
        let h = terrain_height(wx, wz);
        world.fills_column_for_test(wx, wz, h);
        assert!(
            dirt_top_is_grassy(&world, IVec3::new(wx, h, wz)),
            "la naturaleza selvatica dejo de ser verde en {wx},{h},{wz}"
        );
    }
}

#[cfg(test)]
mod micro_greedy_tests {
    use super::*;
    use crate::world::{GrassLod, Material, Voxel, World, MICROVOXEL_RES};

    fn full_lod() -> GrassLod {
        GrassLod {
            max_blade_y: Some(MICROVOXEL_RES - 1),
            solid_carpet: false,
        }
    }

    /// Cuántas caras de celda expondría el camino por-celda (la referencia
    /// contra la que se mide la fusión): una por celda ocupada y dirección sin
    /// vecina ocupada.
    fn per_cell_face_count(mask: &MicroMask) -> usize {
        let n = MICROVOXEL_RES;
        let at = |x: i32, y: i32, z: i32| -> bool {
            if x < 0 || y < 0 || z < 0 {
                return false;
            }
            let (x, y, z) = (x as usize, y as usize, z as usize);
            if x >= n || y >= n || z >= n {
                return false;
            }
            mask[x + y * n + z * n * n]
        };
        let mut count = 0;
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    if !at(x as i32, y as i32, z as i32) {
                        continue;
                    }
                    for axis in 0..3 {
                        for step in [1i32, -1] {
                            let mut nb = [x as i32, y as i32, z as i32];
                            nb[axis] += step;
                            if !at(nb[0], nb[1], nb[2]) {
                                count += 1;
                            }
                        }
                    }
                }
            }
        }
        count
    }

    fn area(faces: &[MicroFace]) -> usize {
        faces.iter().map(|f| f.area()).sum()
    }

    /// La invariante fuerte: fusionar agrupa caras, no las inventa ni las
    /// borra. Se comprueba sobre un tronco, una hoja, hierba sembrada y dos
    /// máscarasadoñadas a mano (escalera y damasco).
    #[test]
    fn greedy_micro_faces_preserve_exposed_area() {
        let mut staircase = Voxel::empty(Material::Dirt);
        for z in 0..MICROVOXEL_RES {
            for x in 0..MICROVOXEL_RES {
                for y in 0..(x + z).min(MICROVOXEL_RES) {
                    staircase.set_micro(x, y, z, true);
                }
            }
        }
        let mut checker = Voxel::empty(Material::Dirt);
        for z in 0..MICROVOXEL_RES {
            for y in 0..MICROVOXEL_RES {
                for x in 0..MICROVOXEL_RES {
                    if (x + 2 * y + 3 * z) % 3 == 0 {
                        checker.set_micro(x, y, z, true);
                    }
                }
            }
        }
        for voxel in [
            Voxel::wood_trunk(),
            Voxel::wood_trunk_sized(Material::Wood, 4),
            Voxel::leaves(),
            Voxel::grass_from_seed(7),
            staircase,
            checker,
        ] {
            for lod in [full_lod(), GrassLod::for_distance(0.0)] {
                let mask = micro_mask(&voxel, lod);
                let before = per_cell_face_count(&mask);
                let faces = greedy_micro_faces(&mask);
                assert_eq!(
                    area(&faces),
                    before,
                    "la fusion cambio la superficie expuesta ({:?})",
                    voxel.material
                );
                assert!(
                    faces.len() <= before,
                    "fusionar nunca debe crecer el número de caras"
                );
            }
        }
    }

    /// Un tronco de 8×16×8 son 6 caras (una por lado) en vez de 640, que es lo
    /// que emitía el camino por-celda.
    #[test]
    fn a_tree_trunk_collapses_to_six_quads() {
        let mask = micro_mask(&Voxel::wood_trunk(), full_lod());
        let faces = greedy_micro_faces(&mask);
        assert_eq!(faces.len(), 6, "caja 8×16×8 → 6 caras: {faces:?}");
        // 2 tapas de 8×8 + 4 laterales de 8×16.
        assert_eq!(area(&faces), 2 * 8 * 8 + 4 * 8 * 16);
        // La referencia: el camino por-celda habría emitido una quad por celda expuesta.
        assert_eq!(per_cell_face_count(&mask), 640);
    }

    /// Una alfombra de hierba sólida (lod `solid_carpet`) son 256 celdas: la
    /// función pura las agrupa en 6 caras, y como la de arriba es un único quad
    /// de 16×16 en vez de 256.
    #[test]
    fn a_solid_carpet_collapses_to_one_top_quad() {
        let lod = GrassLod {
            max_blade_y: Some(0),
            solid_carpet: true,
        };
        let mask = micro_mask(&Voxel::empty(Material::Grass), lod);
        let faces = greedy_micro_faces(&mask);
        assert_eq!(faces.len(), 6, "{faces:?}");
        let top = faces
            .iter()
            .find(|f| f.axis == 1 && f.positive)
            .expect("cara superior");
        assert_eq!(top.area(), MICROVOXEL_RES * MICROVOXEL_RES);
        assert_eq!(per_cell_face_count(&mask), 2 * 16 * 16 + 4 * 16);

        // Encerrada de bloques sólidos, solo queda la tapa: 1 quad de 256 celdas.
        // (El suelo va en y=0 porque `set_voxel` ignora y < 0.)
        let mut world = World::new();
        let pos = glam::IVec3::new(0, 1, 0);
        for d in [
            glam::IVec3::new(1, 0, 0),
            glam::IVec3::new(-1, 0, 0),
            glam::IVec3::new(0, 1, 0),
            glam::IVec3::new(0, -1, 0),
            glam::IVec3::new(0, 0, 1),
            glam::IVec3::new(0, 0, -1),
        ] {
            world.set_voxel(pos + d, Voxel::solid(Material::Stone));
        }
        let (mut v, mut i) = (Vec::new(), Vec::new());
        push_microvoxel_mesh(
            &world,
            pos,
            &mut v,
            &mut i,
            Vec3::new(0.0, 1.0, 0.0),
            &Voxel::empty(Material::Grass),
            Material::Grass.color_rgb(),
            Vec3::ZERO,
            lod,
            None,
        );
        assert_eq!(i.len() / 6, 1, "solo la tapa: {} quads", i.len() / 6);
    }

    /// Una tira de 16×1×1 son 6 quads (4 laterales largos + 2 tapas de 1 celda)
    /// en vez de 16 cubitos.
    #[test]
    fn a_single_run_becomes_long_quads() {
        let mut v = Voxel::empty(Material::Dirt);
        for x in 0..MICROVOXEL_RES {
            v.set_micro(x, 5, 7, true);
        }
        let mask = micro_mask(&v, full_lod());
        let faces = greedy_micro_faces(&mask);
        assert_eq!(faces.len(), 6, "{faces:?}");
        assert_eq!(area(&faces), 4 * 16 + 2);
        assert_eq!(per_cell_face_count(&mask), 4 * 16 + 2);
        // Las cuatro caras largas son de 16×1 celdas: una quad cada una.
        let long = faces.iter().filter(|f| f.area() == 16).count();
        assert_eq!(long, 4, "{faces:?}");
    }

    /// El camino sin texturar emite un quad por cara fusionada; el de textura
    /// (corteza) sigue emitting uno por celda para no estirar el grano.
    #[test]
    fn untextured_micro_geometry_emits_merged_quads() {
        let mut world = World::new();
        world.fills_column_for_test(0, 0, 10);
        let pos = glam::IVec3::new(0, 11, 0);
        world.set_voxel(pos, Voxel::wood_trunk());
        let origin = Vec3::new(0.0, 11.0, 0.0);
        let color = Material::Dirt.color_rgb();
        let full = full_lod();

        let (mut v, mut i) = (Vec::new(), Vec::new());
        push_microvoxel_mesh(
            &world,
            pos,
            &mut v,
            &mut i,
            origin,
            world.get_voxel(pos).expect("tronco"),
            color,
            origin,
            full,
            None,
        );
        // 6 caras fundidas, menos la de abajo: el bloque de abajo es sólido.
        assert_eq!(i.len() / 6, 5, "sin texturar: 5 quads");
        assert_eq!(v.len(), 5 * 4);

        // Con atlas el tronco mantiene el grano: una quad por celda expuesta
        // (640 menos las 64 de la base, que el bloque de abajo recorta).
        let (mut v2, mut i2) = (Vec::new(), Vec::new());
        push_microvoxel_mesh(
            &world,
            pos,
            &mut v2,
            &mut i2,
            origin,
            world.get_voxel(pos).expect("tronco"),
            color,
            origin,
            full,
            Some(0.0),
        );
        assert_eq!(i2.len() / 6, 576, "con atlas: 576 quads por celda");
    }
}

#[cfg(test)]
mod face_cull_tests {
    use super::*;
    use crate::world::World;
    use glam::Vec3;

    #[test]
    fn water_faces_go_to_transparent_mesh() {
        use crate::world::Material;
        let mut world = World::new();
        world.fills_column_for_test(0, 0, 10);
        world.set_voxel(
            glam::IVec3::new(0, 11, 0),
            crate::world::Voxel::solid(Material::Water),
        );
        let cam = Vec3::new(8.0, 14.0, 8.0);
        let (_, _, _, water_v, water_i) = build_section_mesh(&world, 0, 0, 0, cam, cam);
        assert!(!water_i.is_empty(), "sin malla de agua");
        assert!(!water_v.is_empty());
        assert_eq!(water_i.len() % 3, 0);
    }

    #[test]
    fn flow_surface_ramps_down_25_percent_steps() {
        use crate::world::Material;
        // Tira sin aldeas (el agua junto a poblados se estanca).
        let (mut x0, mut z0) = (100, 100);
        'strip: for zz in (0..2000).step_by(8) {
            for xx in (0..2000).step_by(8) {
                if (0..14).all(|dx| {
                    !crate::settlements::settlement_claims_block(xx + dx, zz)
                }) {
                    x0 = xx;
                    z0 = zz;
                    break 'strip;
                }
            }
        }
        let mut world = World::new();
        for dx in 0..12 {
            world.fills_column_for_test(x0 + dx, z0, 4);
        }
        world.set_voxel(
            glam::IVec3::new(x0, 5, z0),
            crate::world::Voxel::solid(Material::Water),
        );
        world.refresh_water_around(x0 + 5, z0, 10);
        let cam = Vec3::new(x0 as f32 + 8.0, 10.0, z0 as f32 + 8.0);
        let section = crate::world::mesh_chunk_coord(x0, z0);
        let (_, _, _, water_v, _) =
            build_section_mesh(&world, section.0, 0, section.1, cam, cam);
        // Tapas: fuente al 100% (6.0) y corriente 75/50/25%.
        let mut tops = std::collections::HashSet::new();
        for v in &water_v {
            if v.normal == [0.0, 1.0, 0.0] {
                tops.insert((v.position[1] * 100.0).round() as i32);
            }
        }
        for want in [600, 575, 550, 525] {
            assert!(tops.contains(&want), "falta tapa {want}: {tops:?}");
        }
    }

    #[test]
    fn wet_hero_darkens_below_waterline() {        let feet = Vec3::new(0.5, 10.0, 0.5);
        let pose = crate::hero_pose::HeroPose::idle();
        let (verts, _, _) = build_player_hero_mesh(
            feet,
            0.0,
            Vec3::new(5.0, 12.0, 5.0),
            &pose,
            false,
            None,
            0.0,
            1.0,
            false,
            Some(feet.y + 0.9),
            None,
        );
        assert!(!verts.is_empty());
        // Vértices bajo la línea: oscuros; sobre ella: intactos (misma cara).
        let mut dark = 0u32;
        let mut bright = 0u32;
        for v in &verts {
            // El color base del héroe nunca es negro: < 0.5 implica mojado.
            let lum = v.color[0] + v.color[1] + v.color[2];
            if v.position[1] < feet.y + 0.9 {
                assert!(lum < 2.2, "bajo el agua sin oscurecer: {lum}");
                dark += 1;
            } else {
                bright += 1;
            }
        }
        assert!(dark > 0 && bright > 0, "sin corte en la línea");
    }

    #[test]
    fn ghost_cells_keep_per_cell_color() {
        let cells = [
            (glam::IVec3::new(0, 0, 0), [1.0, 1.0, 1.0]),
            (glam::IVec3::new(5, 5, 5), [0.35, 0.9, 1.0]),
        ];
        let (verts, indices) = build_ghost_mesh(&cells);
        assert_eq!(indices.len(), 72, "two cubes = 12 quads");
        assert!(verts.iter().any(|v| v.color == [1.0, 1.0, 1.0]));
        assert!(verts.iter().any(|v| v.color == [0.35, 0.9, 1.0]));
        assert!(verts.iter().all(|v| {
            v.color == [1.0, 1.0, 1.0] || v.color == [0.35, 0.9, 1.0]
        }));
    }

    #[test]
    fn highlight_shell_outsets_block_bounds() {
        // Outset shell hugs the block from outside so a depth-tested pass
        // can show it (an inset cube would hide inside solid terrain).
        let cells = [(glam::IVec3::new(0, 0, 0), [1.0, 1.0, 1.0])];
        let (verts, _) = build_highlight_mesh(&cells);
        assert_eq!(verts.len(), 24);
        let min_c = verts.iter().map(|v| v.position[0]).fold(1.0f32, f32::min);
        let max_c = verts.iter().map(|v| v.position[0]).fold(0.0f32, f32::max);
        assert!(min_c < 0.0 && max_c > 1.0, "shell must exceed the block");
        // Ghost blueprint stays inset (unchanged behavior).
        let (gverts, _) = build_ghost_mesh(&cells);
        let gmin = gverts.iter().map(|v| v.position[0]).fold(1.0f32, f32::min);
        let gmax = gverts.iter().map(|v| v.position[0]).fold(0.0f32, f32::max);
        assert!(gmin > 0.0 && gmax < 1.0);
    }

    #[test]
    fn dirt_emits_bottom_faces() {
        let world = World::with_dirt_cube();
        let (verts, _) = build_mesh(&world, Vec3::new(0.5, -2.0, 0.5));
        let bottoms = verts
            .iter()
            .filter(|v| v.normal == [0.0, -1.0, 0.0])
            .count();
        assert!(bottoms > 0, "floor faces must exist when viewed from below");
    }

    #[test]
    fn all_unoccluded_faces_are_emitted() {
        let world = World::with_dirt_cube();
        // No camera face-cull in the greedy path anymore: the lone cube's six
        // unoccluded faces all reach the mesh regardless of camera direction.
        let cam = Vec3::new(3.0, 3.0, 3.0);
        let (verts, indices) = build_mesh(&world, cam);
        assert_eq!(indices.len(), 36, "all six unoccluded quads");
        for n in [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ] {
            let count = verts.iter().filter(|v| v.normal == n).count();
            assert!(count >= 4, "missing face {n:?}");
        }
    }

    #[test]
    fn floating_cube_emits_all_six_faces() {
        let world = World::with_face_debug();
        // Floating cube at (0,8,0): every face reaches air, so all six emit.
        let (verts, indices) = build_mesh(&world, Vec3::new(4.5, 10.5, 4.5));
        assert_eq!(indices.len(), 36, "all six unoccluded quads");
        for n in [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ] {
            assert!(
                verts.iter().any(|v| v.normal == n),
                "floating cube missing face {n:?}"
            );
        }
    }

    #[test]
    fn cube_face_winding_matches_outward_normals() {
        let world = World::with_face_debug();
        let (verts, indices) = build_mesh(&world, Vec3::new(4.5, 10.5, 4.5));
        assert_eq!(indices.len() % 3, 0);
        for tri in indices.chunks_exact(3) {
            let a = verts[tri[0] as usize];
            let b = verts[tri[1] as usize];
            let c = verts[tri[2] as usize];
            let e1 = Vec3::from_array(b.position) - Vec3::from_array(a.position);
            let e2 = Vec3::from_array(c.position) - Vec3::from_array(a.position);
            let geo = e1.cross(e2);
            let n = Vec3::from_array(a.normal);
            assert!(
                geo.dot(n) > 0.0,
                "winding opposes normal {:?} (geo {:?})",
                a.normal,
                geo
            );
        }
        // Top lid at y=9 for the cube at y=8 (bottom culled from this camera).
        let top_y = verts
            .iter()
            .filter(|v| v.normal == [0.0, 1.0, 0.0])
            .map(|v| v.position[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((top_y - 9.0).abs() < 1e-3, "top lid y={top_y}");

        // From below, bottom lid must appear at y=8.
        let (verts_below, _) = build_mesh(&world, Vec3::new(0.5, 6.0, 0.5));
        let bot_y = verts_below
            .iter()
            .filter(|v| v.normal == [0.0, -1.0, 0.0])
            .map(|v| v.position[1])
            .fold(f32::INFINITY, f32::min);
        assert!((bot_y - 8.0).abs() < 1e-3, "bottom lid y={bot_y}");
    }

    #[test]
    fn solid_stack_hides_internal_faces() {
        use crate::world::Voxel;
        use glam::IVec3;

        let mut world = World::new();
        // Start at y=1 — y=0 is the permanent bedrock band (different material).
        for y in 1..=5 {
            world.set_voxel(IVec3::new(0, y, 0), Voxel::dirt());
        }
        let _ = world.take_dirty_chunks();
        assert!(world.has_chunk(0, 0));

        // High +X corner: all exterior faces emit (no camera cull) — 4 merged
        // vertical side quads + top lid + bottom (y=0 bedrock is absent in this
        // hand-built test world, so the bottom quad is also present).
        let (verts, indices) = build_mesh(&world, Vec3::new(3.0, 8.0, 3.0));
        assert_eq!(indices.len(), 36, "five-block stack exterior");

        // No +Y/-Y quads on the shared horizontal planes inside the stack.
        let internal_horiz = verts
            .iter()
            .filter(|v| {
                let y = v.position[1];
                let up_or_down = v.normal == [0.0, 1.0, 0.0] || v.normal == [0.0, -1.0, 0.0];
                up_or_down && y > 1.01 && y < 5.99
            })
            .count();
        assert_eq!(
            internal_horiz, 0,
            "no internal horizontal faces between solids"
        );
    }

    #[test]
    fn adjacent_solids_hide_shared_faces() {
        use crate::world::Voxel;
        use glam::IVec3;

        let mut world = World::new();
        for (x, z) in [(0, 0), (1, 0)] {
            world.set_voxel(IVec3::new(x, 1, z), Voxel::dirt());
        }
        let _ = world.take_dirty_chunks();

        let (verts, indices) = build_mesh(&world, Vec3::new(3.0, 4.0, 3.0));
        // Merged 2×1×1 box: six exterior quads (no camera cull, shared face gone).
        assert_eq!(indices.len(), 36, "shared face must not be emitted");

        // No faces on the shared plane x=1 with ±X normals.
        let shared = verts
            .iter()
            .filter(|v| {
                (v.normal == [1.0, 0.0, 0.0] || v.normal == [-1.0, 0.0, 0.0])
                    && (v.position[0] - 1.0).abs() < 1e-3
            })
            .count();
        assert_eq!(shared, 0, "faces behind a solid neighbor must be culled");
    }

    #[test]
    fn dirt_faces_are_solid_colors() {
        use crate::world::{Material, ENABLE_HD2D};
        let world = World::with_dirt_cube();
        let (verts, _) = build_mesh(&world, Vec3::new(3.0, 3.0, 3.0));
        let dirt = Material::Dirt.color_rgb();
        let grass = Material::Grass.color_rgb();

        if ENABLE_HD2D {
            let sides_ok = verts.iter().filter(|v| v.normal[1].abs() < 0.5).all(|v| {
                (v.color[0] - dirt[0]).abs() < 1e-3
                    && (v.color[1] - dirt[1]).abs() < 1e-3
                    && (v.seed - TERRAIN_TILE_DIRT).abs() < 0.1
            });
            // Floating test cube is not the natural surface → dirt tapa.
            let tops_ok = verts
                .iter()
                .filter(|v| v.normal == [0.0, 1.0, 0.0])
                .all(|v| {
                    (v.color[0] - dirt[0]).abs() < 1e-3
                        && (v.color[1] - dirt[1]).abs() < 1e-3
                        && (v.seed - TERRAIN_TILE_DIRT).abs() < 0.1
                });
            assert!(
                sides_ok,
                "HD-2D sides are brown dirt (no green fringe tile)"
            );
            assert!(tops_ok, "non-surface dirt lids use dirt tapa");
            let _ = grass;
        } else {
            let all_dirt = verts.iter().all(|v| {
                (v.color[0] - dirt[0]).abs() < 1e-3
                    && (v.color[1] - dirt[1]).abs() < 1e-3
                    && (v.color[2] - dirt[2]).abs() < 1e-3
            });
            assert!(all_dirt, "FPS: flat dirt color on all faces");
        }
    }

    #[test]
    fn surface_lid_is_green_dug_lid_is_dirt() {
        use crate::world::{terrain_height, Material, ENABLE_HD2D};
        use glam::IVec3;

        if !ENABLE_HD2D {
            return;
        }
        let mut world = World::with_shunk();
        let x = 4;
        let z = 4;
        let surface = terrain_height(x, z);
        let cam = Vec3::new(x as f32 + 0.5, surface as f32 + 8.0, z as f32 + 0.5);
        let (verts, _) = build_mesh(&world, cam);
        let g = Material::Grass.color_rgb();
        let dirt = Material::Dirt.color_rgb();
        let y_top = (surface + 1) as f32;
        let green_surface = verts.iter().any(|v| {
            v.normal == [0.0, 1.0, 0.0]
                && (v.position[1] - y_top).abs() < 0.02
                && v.color[1] > v.color[0]
                && (v.color[1] - g[1]).abs() < 0.08
        });
        assert!(green_surface, "natural surface tapa must be green");

        // Dig the surface cell → new lid is subsurface → dirt tapa.
        assert!(world.remove_voxel(IVec3::new(x, surface, z)));
        let (after, _) = build_mesh(&world, cam);
        let y_dug = surface as f32;
        let dug_green = after.iter().any(|v| {
            v.normal == [0.0, 1.0, 0.0]
                && (v.position[1] - y_dug).abs() < 0.02
                && v.color[1] > v.color[0] + 0.05
        });
        assert!(!dug_green, "dug tread must not keep a green tapa");
        let dug_dirt = after.iter().any(|v| {
            v.normal == [0.0, 1.0, 0.0]
                && (v.position[1] - y_dug).abs() < 0.02
                && (v.color[0] - dirt[0]).abs() < 0.08
                && (v.color[1] - dirt[1]).abs() < 0.08
        });
        assert!(dug_dirt, "dug tread uses dirt tapa");
    }

    #[test]
    fn grass_is_carpet_and_hides_dirt_top() {
        use crate::world::{terrain_height, Material, Voxel, ENABLE_GRASS, ENABLE_HD2D};
        use glam::IVec3;

        if !ENABLE_GRASS {
            let world = World::with_shunk();
            let (_, grass) = world.voxel_counts();
            assert_eq!(grass, 0, "grass generation is disabled");
            return;
        }

        let mut world = World::new();
        // Clear of the origin settlement: village ground is trodden dirt now,
        // so a green tapa needs real wilderness.
        let (x, z) = (9, 9);
        assert!(
            !crate::settlements::settlement_claims_block(x, z),
            "la columna de test cae dentro de una aldea"
        );
        let surface = terrain_height(x, z);
        // Build a short column whose top is the natural surface (green tapa).
        for y in 0..=surface {
            world.set_voxel(IVec3::new(x, y, z), Voxel::dirt());
        }
        world.set_voxel(
            IVec3::new(x, surface + 1, z),
            Voxel::grass_from_seed(0xABCD),
        );

        let cam = Vec3::new(8.0, surface as f32 + 3.0, 8.0);
        let (verts, _) = build_mesh(&world, cam);
        let g = Material::Grass.color_rgb();
        let y_top = (surface + 1) as f32;

        if ENABLE_HD2D {
            let green_top = verts.iter().any(|v| {
                v.normal == [0.0, 1.0, 0.0]
                    && (v.position[1] - y_top).abs() < 0.02
                    && v.color[1] > v.color[0]
                    && (v.color[1] - g[1]).abs() < 0.08
            });
            assert!(green_top, "HD-2D: natural surface +Y lid is green");
            let billboard = verts.iter().any(|v| v.normal[1].abs() < 0.1);
            assert!(billboard, "HD-2D: crossed grass billboards present");

            // Strip the carpet → surface lid stays green; sides stay plain dirt.
            assert!(world.remove_voxel(IVec3::new(x, surface + 1, z)));
            let (bare, _) = build_mesh(&world, cam);
            let green_after = bare.iter().any(|v| {
                v.normal == [0.0, 1.0, 0.0]
                    && (v.position[1] - y_top).abs() < 0.02
                    && v.color[1] > v.color[0]
            });
            assert!(
                green_after,
                "natural surface tapa stays green without tufts"
            );
            let bad: Vec<_> = bare
                .iter()
                .filter(|v| v.normal[1].abs() < 0.5)
                .filter(|v| {
                    // Reject green-fringe side tile / grass-top atlas on walls.
                    let green_fringe = (v.seed - TERRAIN_TILE_TOP).abs() < 0.1
                        || (v.seed - TERRAIN_TILE_SIDE).abs() < 0.1;
                    v.flags >= 1.5 && green_fringe
                })
                .map(|v| (v.normal, v.seed, v.flags, v.color))
                .collect();
            assert!(
                bad.is_empty(),
                "dirt sides must not use grass-top/fringe atlas; bad={bad:?}"
            );
        } else {
            let dirt_up = verts.iter().any(|v| {
                v.normal == [0.0, 1.0, 0.0]
                    && (v.position[1] - y_top).abs() < 0.02
                    && v.color[1] < 0.5
            });
            assert!(!dirt_up, "dirt top under grass should be culled");

            let grass_carpet = verts.iter().any(|v| {
                (v.color[0] - g[0]).abs() < 0.02
                    && (v.color[1] - g[1]).abs() < 0.02
                    && v.normal[1].abs() < 0.1
            });
            assert!(grass_carpet, "expected crossed green grass billboards");
        }
    }

    #[test]
    fn mid_range_dirt_uses_one_quad_per_face() {
        use crate::world::{
            DIRT_HIRES_MAX_DIST, DIRT_POLISHED_MAX_DIST, ENABLE_HD2D, HD2D_DIRT_HIRES_DIST,
            HD2D_DIRT_POLISHED_DIST,
        };
        let world = World::with_dirt_cube();
        // Chunk center is (8,1,8). Polished band: hires..polished from center.
        let d1 = if ENABLE_HD2D {
            (HD2D_DIRT_HIRES_DIST + HD2D_DIRT_POLISHED_DIST) * 0.5
        } else {
            (DIRT_HIRES_MAX_DIST + DIRT_POLISHED_MAX_DIST) * 0.5
        };
        // Elevated corner so +X/+Y/+Z face the lens (back faces still emitted:
        // greedy dirt does not camera-cull — that punched 16×16 holes).
        let (verts, indices) = build_mesh(&world, Vec3::new(8.0 + d1, 8.0, 8.0));
        assert_eq!(indices.len(), 36, "isolated cube: six greedy quads");
        assert_eq!(verts.len(), 24);
        // Isolated cube is not the natural surface lip → dirt tapa (not grass).
        let dirt = crate::world::Material::Dirt.color_rgb();
        assert!(
            verts.iter().any(|v| {
                v.normal == [0.0, 1.0, 0.0]
                    && (v.color[0] - dirt[0]).abs() < 0.05
                    && (v.color[1] - dirt[1]).abs() < 0.05
            }),
            "dirt top flat color"
        );

        // Mid / early-HLOD band still sealed (camera-facing only).
        // Heightmap HLOD only sees dense columns — sparse extras (with_dirt_cube) vanish.
        let d2 = if ENABLE_HD2D {
            use crate::world::{HD2D_DIRT_HEIGHTMAP_DIST, HD2D_DIRT_MESH_DIST};
            (HD2D_DIRT_HEIGHTMAP_DIST + HD2D_DIRT_MESH_DIST) * 0.5
        } else {
            use crate::world::{DIRT_HEIGHTMAP_MAX_DIST, DIRT_POLISHED_MAX_DIST};
            (DIRT_POLISHED_MAX_DIST + DIRT_HEIGHTMAP_MAX_DIST) * 0.5
        };
        let mut col = World::new();
        col.fills_column_for_test(0, 0, 4);
        let surface = crate::world::terrain_height(0, 0) as f32;
        // Camera must sit above the HLOD lid (natural surface), else +Y is culled.
        let (verts, indices) = build_mesh(&col, Vec3::new(8.0 + d2, surface + 24.0, 8.0));
        assert!(
            indices.len() >= 6,
            "HLOD column emits at least a top quad; got {}",
            indices.len()
        );
        assert!(
            verts.iter().any(|v| v.normal == [0.0, 1.0, 0.0]),
            "top face present"
        );
    }

    #[test]
    fn far_chunk_uses_heightmap_hlod() {
        use crate::world::{
            dirt_mesh_max_dist, DIRT_HEIGHTMAP_MAX_DIST, DIRT_MESH_MAX_DIST, ENABLE_HD2D,
            HD2D_DIRT_HEIGHTMAP_DIST, HD2D_DIRT_MESH_DIST,
        };
        let world = World::with_shunk();
        let mesh_cut = dirt_mesh_max_dist();
        // Past mesh cut: no geometry.
        let (verts, indices, grass, _, _) = build_chunk_mesh(
            &world,
            0,
            0,
            Vec3::new(8.0 + mesh_cut + 8.0, 40.0, 8.0),
            Vec3::new(8.0 + mesh_cut + 8.0, 40.0, 8.0),
        );
        assert!(grass.is_empty());
        assert!(verts.is_empty());
        assert!(indices.is_empty());

        // Heightmap band: surface tops still emit when viewed from above.
        let d_hm = if ENABLE_HD2D {
            (HD2D_DIRT_HEIGHTMAP_DIST + HD2D_DIRT_MESH_DIST) * 0.5
        } else {
            (DIRT_HEIGHTMAP_MAX_DIST + DIRT_MESH_MAX_DIST) * 0.5
        };
        let (verts, indices, _, _, _) = build_chunk_mesh(
            &world,
            0,
            0,
            Vec3::new(8.0 + d_hm, 40.0, 8.0),
            Vec3::new(8.0 + d_hm, 40.0, 8.0),
        );
        assert!(indices.len() >= 6);
        let dirt = crate::world::Material::Dirt.color_rgb();
        let grass = crate::world::Material::Grass.color_rgb();
        // Tops are green only on columns that still have grass cover; else brown.
        let has_top = verts.iter().any(|v| {
            if v.normal != [0.0, 1.0, 0.0] {
                return false;
            }
            let brown = (v.color[0] - dirt[0]).abs() < 0.08 && (v.color[1] - dirt[1]).abs() < 0.08;
            let green = (v.color[1] - grass[1]).abs() < 0.08 && v.color[1] > v.color[0];
            brown || green
        });
        assert!(has_top, "heightmap tops must remain inside the view bubble");
    }

    #[test]
    fn far_hlod_keeps_natural_lid_after_column_is_punched() {
        use crate::world::{terrain_height, Voxel};

        let x = 8;
        let z = 8;
        let natural_h = terrain_height(x, z);
        assert!(natural_h >= 5, "test column must have an underground band");

        let mut world = World::new();
        for y in 0..=natural_h {
            world.set_voxel(glam::IVec3::new(x, y, z), Voxel::dirt());
        }
        assert!(world.remove_voxel(glam::IVec3::new(x, natural_h - 3, z)));
        assert!(
            world.column_height(x, z).is_some_and(|h| h < natural_h),
            "punch must lower the dense-stack height"
        );
        assert_eq!(
            hlod_column_height(&world, x, z),
            Some(natural_h),
            "far HLOD must seal the cave at the natural exterior lid"
        );
    }

    #[test]
    fn full_shunk_mesh_fits_gpu_budget() {
        let world = World::with_shunk();
        let (verts, indices) = build_mesh(&world, Vec3::new(16.0, 9.0, 18.0));
        assert!(
            (verts.len() as u64) <= MAX_VERTICES,
            "verts {} > max {}",
            verts.len(),
            MAX_VERTICES
        );
        assert!(
            (indices.len() as u64) <= MAX_INDICES,
            "indices {} > max {}",
            indices.len(),
            MAX_INDICES
        );
    }

    /// Count dirt quads submitted for the HD-2D character camera (frustum + face cull).
    #[test]
    fn count_visible_dirt_faces_hd2d_camera() {
        use crate::camera::Camera;
        use crate::player::Player;
        use crate::world::{
            dirt_mesh_max_dist, World, ENABLE_HD2D, ENABLE_STREAMING, MESH_CHUNK_SIZE,
        };

        assert!(ENABLE_HD2D, "count assumes HD-2D camera");
        assert!(ENABLE_STREAMING, "count assumes infinite streaming world");

        let player = Player::spawn_on_terrain();
        let camera = Camera::hd2d_follow(player.focus_position());
        let origin = camera.hd2d_focus();
        let mut world = World::with_infinite(origin);
        for _ in 0..100 {
            world.stream_around(origin);
        }

        let candidates = world.chunk_coords_near(origin, dirt_mesh_max_dist());
        let mut meshed_faces = 0u64;
        let mut meshed_chunks = 0u64;
        for &(cx, cz) in &candidates {
            if !world.chunk_filled(cx, cz) {
                continue;
            }
            let (_, indices, _, _, _) = build_chunk_mesh(&world, cx, cz, camera.position, origin);
            if indices.is_empty() {
                continue;
            }
            meshed_chunks += 1;
            meshed_faces += (indices.len() / 6) as u64;
        }

        let visible_chunks = cull_chunks(&camera, &mut world, &candidates);
        let mut screen_faces = 0u64;
        let mut screen_verts = 0u64;
        let mut by_axis = [0u64; 6]; // +X -X +Y -Y +Z -Z
        let mut frustum_chunks = 0u64;

        for &(cx, cz) in &visible_chunks {
            if !world.chunk_filled(cx, cz) {
                continue;
            }
            let (verts, indices, _, _, _) = build_chunk_mesh(&world, cx, cz, camera.position, origin);
            if indices.is_empty() {
                continue;
            }
            frustum_chunks += 1;
            screen_faces += (indices.len() / 6) as u64;
            screen_verts += verts.len() as u64;
            for quad in indices.chunks_exact(6) {
                let n = verts[quad[0] as usize].normal;
                let axis = if n[0] > 0.5 {
                    0
                } else if n[0] < -0.5 {
                    1
                } else if n[1] > 0.5 {
                    2
                } else if n[1] < -0.5 {
                    3
                } else if n[2] > 0.5 {
                    4
                } else {
                    5
                };
                by_axis[axis] += 1;
            }
        }

        let s = MESH_CHUNK_SIZE as f32;
        eprintln!("=== DIRT FACES @ HD-2D spawn camera ===");
        eprintln!(
            "cam=({:.1},{:.1},{:.1}) focus≈player fovy={:.1}°",
            camera.position.x,
            camera.position.y,
            camera.position.z,
            camera.fovy.to_degrees()
        );
        eprintln!(
            "candidates={} meshed_chunks={} meshed_faces={} (no frustum)",
            candidates.len(),
            meshed_chunks,
            meshed_faces
        );
        eprintln!(
            "frustum_chunks={} screen_faces={} screen_verts={} (aabb frustum)",
            frustum_chunks, screen_faces, screen_verts
        );
        eprintln!(
            "by_axis +X={} -X={} +Y={} -Y={} +Z={} -Z={}",
            by_axis[0], by_axis[1], by_axis[2], by_axis[3], by_axis[4], by_axis[5]
        );
        eprintln!("chunk AABB pad uses s={s}; face cull keeps ≤3 dirs/chunk from camera");

        assert!(screen_faces > 0, "expected dirt faces on screen");
        assert!(
            screen_faces <= meshed_faces,
            "frustum set cannot exceed meshed set"
        );
        // HD-2D looks down-ish: tops (+Y) and two side axes dominate; -Y rare.
        assert!(
            by_axis[2] > by_axis[3],
            "expect more +Y tops than bottoms from isometric cam"
        );
    }

    }

#[cfg(test)]
mod scene_scale_tests {
    use super::{capped_surface_size, dyn_scale_step, is_diorama_view, scene_internal_size_for, SCENE_INTERNAL_SCALE};

    #[test]
    fn diorama_uses_internal_scale_and_fp_is_native() {
        // Escala normalizada a 1.0 (2026-09-22): ambos modos nativos.
        let (dw, dh) = scene_internal_size_for(1920, 1080, true);
        assert_eq!(dw, (1920.0 * SCENE_INTERNAL_SCALE).round() as u32);
        assert_eq!(dh, (1080.0 * SCENE_INTERNAL_SCALE).round() as u32);
        assert_eq!(scene_internal_size_for(1920, 1080, false), (1920, 1080));
        assert_eq!((dw, dh), (1920, 1080));
    }

    #[test]
    fn diorama_threshold_matches_section_cull() {
        // `ensure_scene_scale` y el culling usan el mismo umbral 0.25:
        // diorama = hd2d_amount >= 0.25.
        let mut cam = crate::camera::Camera::hd2d_follow(glam::Vec3::ZERO);
        assert!(is_diorama_view(&cam));
        cam.request_first_person();
        // Avanzar el blend 0.45 s completo → hd2d_amount = 0 → vista FP.
        cam.update_mode(0.45);
        assert!(!is_diorama_view(&cam));
    }

    #[test]
    fn surface_cap_only_kicks_in_above_1440() {
        // En host (no Android) el cap es identidad.
        assert_eq!(capped_surface_size(2400, 1080), (2400, 1080));
        assert_eq!(capped_surface_size(0, 0), (1, 1));
        if cfg!(target_os = "android") {
            assert_eq!(capped_surface_size(2400, 1080), (1440, 648));
            assert_eq!(capped_surface_size(1280, 720), (1280, 720));
        }
    }

    #[test]
    fn dyn_scale_ladder_has_hysteresis() {
        assert_eq!(dyn_scale_step(0.66, 40.0), 0.55);
        assert_eq!(dyn_scale_step(0.55, 40.0), 0.45);
        assert_eq!(dyn_scale_step(0.45, 10.0), 0.45, "suelo");
        assert_eq!(dyn_scale_step(0.45, 60.0), 0.55);
        assert_eq!(dyn_scale_step(0.66, 60.0), 0.66, "techo");
        assert_eq!(dyn_scale_step(0.55, 50.0), 0.55, "histéresis 45..57");
        assert_eq!(dyn_scale_step(0.66, 50.0), 0.66);
    }
}

#[cfg(test)]
mod section_split_tests {
    use super::*;
    use crate::world::{mesh_section_range, World, MESH_SECTIONS_Y};
    use glam::Vec3;

    /// Near top-down camera over chunk (0,0) → band 0 greedy path.
    fn near_cam() -> Vec3 {
        Vec3::new(8.0, 60.0, 8.0)
    }

    #[test]
    fn sections_stay_within_slab() {
        let world = World::with_shunk();
        let cam = near_cam();
        for cy in 0..MESH_SECTIONS_Y {
            let (v, _, _, _, _) = build_section_mesh(&world, 1, cy, 1, cam, cam);
            let (sy0, sy1) = mesh_section_range(cy);
            for vert in &v {
                let y = vert.position[1];
                assert!(
                    y >= sy0 as f32 - 1e-3 && y <= sy1 as f32 + 1e-3,
                    "cy={cy} vert y={y} outside [{sy0},{sy1})"
                );
            }
        }
    }

    #[test]
    fn section_tops_match_column_tops() {
        // +Y lids belong to exactly one slab → the multiset of top verts must
        // equal the column mesh tops (side quads may split, tops never do).
        let world = World::with_shunk();
        let cam = near_cam();
        let (col_v, _, _, _, _) = build_chunk_mesh(&world, 1, 1, cam, cam);
        let mut col_tops: Vec<u32> = col_v
            .iter()
            .filter(|v| v.normal == [0.0, 1.0, 0.0])
            .flat_map(|v| v.position.map(|c| c.to_bits()))
            .collect();
        col_tops.sort();
        let mut sec_tops = Vec::new();
        for cy in 0..MESH_SECTIONS_Y {
            let (v, _, _, _, _) = build_section_mesh(&world, 1, cy, 1, cam, cam);
            sec_tops.extend(
                v.iter()
                    .filter(|v| v.normal == [0.0, 1.0, 0.0])
                    .flat_map(|v| v.position.map(|c| c.to_bits())),
            );
        }
        sec_tops.sort();
        assert!(!col_tops.is_empty(), "column must emit tops");
        assert_eq!(
            sec_tops, col_tops,
            "split slabs must preserve every top lid"
        );
    }

    #[test]
    fn empty_upper_section_emits_nothing() {
        let mut world = World::new();
        // Flat low terrain: slabs cy=2,3 are provably empty.
        for z in 0..16 {
            for x in 0..16 {
                world.fills_column_for_test(x, z, 10);
            }
        }
        let cam = Vec3::new(8.0, 30.0, 8.0);
        for cy in 2..MESH_SECTIONS_Y {
            assert!(!world.section_may_have_content(0, 0, cy));
            let (v, i, g, _, _) = build_section_mesh(&world, 0, cy, 0, cam, cam);
            assert!(v.is_empty() && i.is_empty() && g.is_empty());
        }
        // Surface slab still meshes.
        let (v, i, _, _, _) = build_section_mesh(&world, 0, 0, 0, cam, cam);
        assert!(!v.is_empty() && !i.is_empty());
    }

    #[test]
    fn buckets_cover_every_index_once() {
        let world = World::with_shunk();
        let cam = near_cam();
        for cy in 0..MESH_SECTIONS_Y {
            let (v, i, _, _, _) = build_section_mesh(&world, 1, cy, 1, cam, cam);
            if i.is_empty() {
                continue;
            }
            let (sorted, ranges) = sort_indices_by_direction(&v, &i);
            assert_eq!(sorted.len(), i.len());
            // Ranges tile the reordered buffer without gaps/overlaps.
            let mut cursor = 0u32;
            for (start, count) in ranges {
                assert_eq!(start, cursor, "bucket ranges must tile");
                cursor += count;
            }
            assert_eq!(cursor as usize, i.len());
            // Same multiset of indices (reordered, none lost/duplicated).
            let mut a = sorted.clone();
            let mut b = i.clone();
            a.sort();
            b.sort();
            assert_eq!(a, b);
            // Every triangle sits in the bucket of its own normal.
            for (tri, b) in sorted.chunks_exact(3).zip(
                ranges
                    .iter()
                    .enumerate()
                    .flat_map(|(bb, &(_, c))| std::iter::repeat(bb).take(c as usize / 3)),
            ) {
                let n = v[tri[0] as usize].normal;
                assert_eq!(
                    bucket_for_normal(n),
                    b,
                    "triangle normal {n:?} in wrong bucket"
                );
            }
        }
    }

    #[test]
    fn face_mask_culls_only_backfaces() {        // Section (0,0,0) center = (8,8,8). Far +X camera sees +X/+Y/+Z...
        let mask = section_face_mask(Vec3::new(200.0, 8.0, 8.0), 0, 0, 0);
        assert!(mask & (1 << 0) != 0, "+X must draw from +X, got {mask:b}");
        assert!(mask & (1 << 1) == 0, "-X must cull from +X, got {mask:b}");
        let mask = section_face_mask(Vec3::new(8.0, 200.0, 8.0), 0, 0, 0);
        assert!(mask & (1 << 2) != 0, "+Y must draw from above");
        assert!(mask & (1 << 3) == 0, "-Y must cull from above");
        // Camera inside the slab → everything draws (never a hole).
        assert_eq!(section_face_mask(Vec3::new(8.0, 8.0, 8.0), 0, 0, 0), 0x3F);
    }

    #[test]
    fn section_key_ignores_lens() {
        // No facing is baked at build anymore: orbit, rise, distance and
        // FP/HD-2D must never invalidate meshes (only band/edits do).
        // This is the anti-remesh-storm guarantee.
        use crate::camera::Camera;
        use std::f32::consts::FRAC_PI_4;
        let focus = Vec3::new(8.0, 22.0, 8.0);
        let cam = Camera::hd2d_follow(focus);
        let base = chunk_section_state_key(&cam, 0, 1, 0);
        let mut orbit = Camera::hd2d_follow(focus);
        orbit.yaw += FRAC_PI_4;
        assert_eq!(chunk_section_state_key(&orbit, 0, 1, 0), base);
        let up = Camera::hd2d_follow(focus + Vec3::new(0.0, 8.0, 0.0));
        assert_eq!(chunk_section_state_key(&up, 0, 1, 0), base);
        let mut fp = Camera::hd2d_follow(focus);
        fp.request_first_person();
        fp.update_mode(10.0);
        assert_eq!(chunk_section_state_key(&fp, 0, 1, 0), base);
    }

        #[test]
    fn grassy_column_emits_instances_near_camera() {
        use crate::world::Voxel;
        use glam::IVec3;
        let mut world = World::new();
        world.fills_column_for_test(4, 4, 10);
        world.set_voxel(IVec3::new(4, 11, 4), Voxel::grass_from_seed(1));
        // Camera nearly overhead of the tuft → band 0, full density.
        let cam = Vec3::new(4.5, 14.0, 4.5);
        let (_, _, grass, _, _) = build_section_mesh(&world, 0, 0, 0, cam, cam);
        assert_eq!(grass.len(), 1, "tuft must emit one instance");
    }

    #[test]
    fn face_mask_top_bucket_uses_mesh_section_center() {
        use crate::world::{mesh_section_range, MESH_CHUNK_SIZE, MESH_SECTION_HEIGHT};
        // Regression: mesh sections are 16 tall (MESH_SECTION_HEIGHT), but the
        // mask center used CHUNK_SECTION_HEIGHT (32) → +Y tops of section cy=1
        // (y∈[16,32)) were culled below y≈34.1 even when the camera was above
        // the ground. Ground tops are front-facing from any camera above them.
        let cy = 1;
        let (y0, y1) = mesh_section_range(cy);
        assert_eq!((y0, y1), (16, 32));
        // Camera horizontally outside the slab, still above its tops at y=26.
        let cam = Vec3::new(8.0 + MESH_CHUNK_SIZE as f32 * 2.0, 26.0, 8.0);
        let mask = section_face_mask(cam, 0, cy, 0);
        assert!(
            mask & (1 << 2) != 0,
            "ground tops must draw while the camera is above the slab, got {mask:b}"
        );
        // Camera inside the slab → everything draws (never a hole).
        let inside = Vec3::new(8.0, 24.0, 8.0);
        assert_eq!(section_face_mask(inside, 0, cy, 0), 0x3F);
    }

    #[test]
    fn hlod_tops_emit_from_below() {
        // The missing-ground bug: far flat HLOD seen from a pit culled its
        // tops at build (baked facing) and emitted nothing. Horizontal faces
        // must always emit — GPU + draw buckets cull them when backfacing.
        let mut world = World::new();
        for z in 0..16 {
            for x in 0..16 {
                world.fills_column_for_test(x, z, 20);
            }
        }
        // XZ dist 68 → band 3 (HLOD); lens 16 under the lids.
        let cam = Vec3::new(8.0, 5.0, 76.0);
        let (v, i, _, _, _) = build_section_mesh(&world, 0, 1, 0, cam, cam);
        assert!(!i.is_empty(), "HLOD slab must mesh from below");
        assert!(
            v.iter().any(|vert| vert.normal == [0.0, 1.0, 0.0]),
            "tops must emit even when the lens is underneath"
        );
    }

    #[test]
    fn fresh_terrain_tops_use_grass_tile() {
        // "Tapa marrón" probe: in the SURFACE slab (cy=1) of fresh terrain,
        // grass-top lids (seed 0) must dominate dirt lids (seed 2). Cave
        // floors (cy=0) are correctly brown and live elsewhere.
        let world = World::with_shunk();
        let cam = near_cam();
        let (v, _, _, _, _) = build_section_mesh(&world, 1, 1, 1, cam, cam);
        let mut top = 0usize;
        let mut dirt = 0usize;
        for vert in &v {
            if vert.normal != [0.0, 1.0, 0.0] || vert.flags < 1.5 {
                continue;
            }
            let tile = vert.seed.floor() as i32;
            if tile == 0 {
                top += 1;
            } else if tile == 2 {
                dirt += 1;
            }
        }
        assert!(top > 0, "surface slab must emit grass-top lids");
        assert!(
            top as f32 >= dirt as f32,
            "grass tops ({top}) must dominate dirt tops ({dirt}) in the surface slab"
        );
    }

    /// Every emitted triangle must agree with its attribute normal (CCW
    /// outward) and every normal must be an exact world cardinal. Catches
    /// inverted winding (faces visible from inside = "2-sided squares") and
    /// skewed normals (flat wrong shading) on ANY emitter path.
    fn assert_mesh_orientation(verts: &[Vertex], indices: &[u32], ctx: &str) {
        const CARDINALS: [[f32; 3]; 6] = [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
        ];
        assert_eq!(indices.len() % 3, 0, "{ctx}: index count");
        for tri in indices.chunks_exact(3) {
            for &i in tri {
                assert!(
                    (i as usize) < verts.len(),
                    "{ctx}: index {i} out of bounds ({} verts)",
                    verts.len()
                );
            }
            let a = verts[tri[0] as usize];
            let n = Vec3::from_array(a.normal);
            assert!(
                CARDINALS.contains(&a.normal),
                "{ctx}: non-cardinal normal {:?}",
                a.normal
            );
            // All 3 verts of a flat-shaded tri share the normal.
            for &i in &tri[1..] {
                assert_eq!(
                    verts[i as usize].normal, a.normal,
                    "{ctx}: mixed normals in one triangle"
                );
            }
            let b = verts[tri[1] as usize];
            let c = verts[tri[2] as usize];
            let e1 = Vec3::from_array(b.position) - Vec3::from_array(a.position);
            let e2 = Vec3::from_array(c.position) - Vec3::from_array(a.position);
            let geo = e1.cross(e2);
            assert!(
                geo.length_squared() > 1e-12,
                "{ctx}: degenerate triangle"
            );
            assert!(
                geo.dot(n) > 0.0,
                "{ctx}: winding opposes normal {:?} (geo {:?})",
                a.normal,
                geo
            );
        }
    }

    #[test]
    fn all_section_emitters_orient_faces_outward() {
        let world = World::with_shunk();
        // Near: greedy + per-voxel + ore + trees. Far: HLOD tops + skirts.
        for (cam, ctx) in [
            (near_cam(), "near"),
            (Vec3::new(8.0, 30.0, -80.0), "hlod"),
        ] {
            for cy in 0..MESH_SECTIONS_Y {
                let (v, i, _, _, _) = build_section_mesh(&world, 1, cy, 1, cam, cam);
                if i.is_empty() {
                    continue;
                }
                assert_mesh_orientation(&v, &i, &format!("{ctx} cy={cy}"));
            }
            let (v, i, _, _, _) = build_chunk_mesh(&world, 1, 1, cam, cam);
            if !i.is_empty() {
                assert_mesh_orientation(&v, &i, &format!("{ctx} column"));
            }
        }
    }


}
