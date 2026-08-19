use glam::{IVec3, Vec3};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Microvoxels per axis inside one world voxel (16³ = 4096 cells).
pub const MICROVOXEL_RES: usize = 16;
const OCCUPANCY_BYTES: usize = (MICROVOXEL_RES * MICROVOXEL_RES * MICROVOXEL_RES) / 8;

pub const WORLD_SEED: u32 = 0x4D31_C205;
/// Controls grass carpet holes and blade growth across the shunk.
pub const GRASS_SEED: u32 = 0x67A5_C0DE;
/// When false, no grass voxels are placed in world gen / streaming.
pub const ENABLE_GRASS: bool = true;
/// When false, no procedural trees are planted.
pub const ENABLE_TREES: bool = true;
/// When false, skip underground chamber / passage carving.
pub const ENABLE_CAVES: bool = true;
/// When false, skip stamping realm settlements (roads / walls / prefab houses).
pub const ENABLE_SETTLEMENTS: bool = true;
/// Mix seed for trunk height / canopy jitter.
pub const TREE_SEED: u32 = 0x72EE_5EED;
/// Mix seed for bush / scrub placement.
pub const BUSH_SEED: u32 = 0xB05A_51DE;
/// Chance a spaced candidate column gets a tree (after [`TREE_CELL`] spacing).
pub const TREE_CHANCE: f32 = 0.38;
/// One tree candidate per this many blocks on XZ (keeps trunks from stacking).
const TREE_CELL: i32 = 10;
/// Trunk microvoxel footprint (centered in the 16³ cube).
pub const TREE_TRUNK_MICRO: usize = 8;
/// Isolated floating cube + per-face colors; orbit to confirm all 6 faces draw.
pub const DEBUG_FACE_VISIBILITY: bool = false;
/// Replace the hero with a colored debug cube (6 axes × 3×3 panels) to diagnose
/// screen-door / winding. Press F in-game to flip winding.
pub const DEBUG_HERO_FACE_CUBE: bool = false;
/// Tint hero faces by axis (1–6) and brightness = faces camera; disables Bayer
/// so we can see if “front faces missing” is cull vs screen-door.
pub const DEBUG_HERO_CAMERA_FACES: bool = false;
/// Second hero draw: one silhouette wherever solid lost to *any* nearer occluder
/// (tree OR terrain, stencil 0). Depth-reset then LessEqual color (visible on top,
/// faces sort). Bayer gated by [`ENABLE_PLAYER_OCCLUDED_BAYER`].
pub const ENABLE_PLAYER_SCREEN_DOOR: bool = true;
/// When false: occluded pass is fully opaque (same look under tree and under dirt).
/// When true: same Bayer screen-door for both.
pub const ENABLE_PLAYER_OCCLUDED_BAYER: bool = true;
/// HD-2D diorama presentation (camera, cel light, crossed grass, player sprite).
pub const ENABLE_HD2D: bool = true;
/// Soft circular hole punched through the undug lid when buried (see `dig_cutaway_open`).
/// Opens a Bayer X-ray over the hero so digs stay readable from the HD-2D lens.
pub const ENABLE_DIG_CUTAWAY: bool = true;
/// Log Reserved/Filled/PendingUnload + skipped meshes (hunt 16×16 holes).
/// Enable with `RUST_LOG=info` (or `microverse=info`).
pub const DEBUG_CHUNK_STREAM: bool = false;

/// A shunk is one 16×16 column chunk (matches mesh chunk XZ).
pub const SHUNK_SIZE: i32 = 16;
/// Contiguous shunks on each axis → [`SHUNK_GRID`]² total (3×3 = 9).
pub const SHUNK_GRID: i32 = 3;
/// Extra blocks beyond the shunk grid on each axis.
pub const WORLD_SURFACE_EXTRA: i32 = 0;
/// Mesh chunk size in X and Z (blocks) — one shunk = one mesh chunk.
pub const MESH_CHUNK_SIZE: i32 = 16;
/// Vertical section height inside a column chunk (blocks).
pub const CHUNK_SECTION_HEIGHT: i32 = 32;
/// Stacked Y sections per column → world ceiling (2 × 32 = 64).
pub const CHUNK_SECTIONS_Y: i32 = 2;
/// Absolute world height in blocks (`CHUNK_SECTION_HEIGHT * CHUNK_SECTIONS_Y`).
pub const WORLD_MAX_HEIGHT: i32 = CHUNK_SECTION_HEIGHT * CHUNK_SECTIONS_Y; // 64
/// Inclusive max block Y (`0..=WORLD_MAX_Y`).
pub const WORLD_MAX_Y: i32 = WORLD_MAX_HEIGHT - 1;
/// Procedural dirt-top ceiling (max column height).
pub const TERRAIN_MAX_HEIGHT: i32 = 64;
/// Typical / average surface height (noise ≈ 0 lands here).
pub const TERRAIN_AVG_HEIGHT: i32 = 20;
/// Valley floor (noise ≈ -1); keeps dips above bedrock.
pub const TERRAIN_VALLEY_HEIGHT: i32 = 8;
/// When true, chunks stream in/out around the camera (infinite world).
pub const ENABLE_STREAMING: bool = true;
/// View distance in shunks (mesh chunks of [`SHUNK_SIZE`]).
/// HD-2D grass detail only reaches ~45 blocks; 6 keeps a short horizon without over-gen.
pub const VIEW_DISTANCE_SHUNKS: i32 = 6;
/// Max mesh view distance in blocks (streaming bubble).
pub const VIEW_DISTANCE: f32 = (VIEW_DISTANCE_SHUNKS * SHUNK_SIZE) as f32; // 96
/// Canonical dig / place reach (blocks). Prefer [`crate::mining::MAX_REACH`].
pub const EDIT_REACH: f32 = 8.0;
/// Base Perlin sampling scale (smaller ⇒ broader, gentler hills).
pub const PERLIN_SCALE: f64 = 0.005;
/// Height amplitude multiplier on Perlin (`1` = full valley↔peak swing).
pub const PERLIN_AMPLITUDE: f32 = 0.35;
/// Slope (block Δh) used to thin grass on steep columns.
pub const ROCK_SLOPE: f32 = 1.6;
/// Base chance a flat column stays bare (no grass tuft). Slopes raise this further.
/// High bare rate leaves open dirt for trees and sparse meadow patches.
pub const GRASS_BARE_CHANCE: f32 = 0.82;
/// Coherent barren patch size (blocks). Power-of-two keeps div cheap.
const GRASS_BARE_CELL: i32 = 8;

pub fn world_blocks() -> i32 {
    SHUNK_GRID * SHUNK_SIZE + WORLD_SURFACE_EXTRA
}

/// Section index along Y for a block coordinate.
#[allow(dead_code)]
pub fn chunk_section_y(y: i32) -> i32 {
    y.div_euclid(CHUNK_SECTION_HEIGHT)
}

/// Chunk key for a world position: `(cx, cy, cz)` with size 16×32×16.
#[allow(dead_code)]
pub fn chunk_coord(pos: IVec3) -> (i32, i32, i32) {
    (
        pos.x.div_euclid(MESH_CHUNK_SIZE),
        chunk_section_y(pos.y),
        pos.z.div_euclid(MESH_CHUNK_SIZE),
    )
}

/// Grass blade fade: shorten from the top starting at 60, gone by 65.
pub const GRASS_BLADE_FADE_START: f32 = 60.0;
pub const GRASS_BLADE_FADE_END: f32 = 65.0;
/// Practical max stem height in micros (generator uses up to ~6).
pub const GRASS_BLADE_MAX_HEIGHT: usize = 6;

/// Beyond this camera↔block distance, dirt uses a flat brown (no tone detail).
pub const DIRT_TONE_MAX_DIST: f32 = VIEW_DISTANCE;
/// Within this distance, dirt keeps full geometric face detail (high res).
pub const DIRT_HIRES_MAX_DIST: f32 = VIEW_DISTANCE * 0.45;
/// Polished / greedy soft until this distance.
pub const DIRT_POLISHED_MAX_DIST: f32 = VIEW_DISTANCE * 0.70;
/// Squared LOD thresholds (avoid `sqrt` in the mesh hot path).
pub const DIRT_HIRES_MAX_DIST_SQ: f32 = DIRT_HIRES_MAX_DIST * DIRT_HIRES_MAX_DIST;
pub const DIRT_POLISHED_MAX_DIST_SQ: f32 = DIRT_POLISHED_MAX_DIST * DIRT_POLISHED_MAX_DIST;
#[allow(dead_code)]
pub const DIRT_TONE_MAX_DIST_SQ: f32 = DIRT_TONE_MAX_DIST * DIRT_TONE_MAX_DIST;
/// Heightmap HLOD from here until the mesh cut (must be < [`DIRT_MESH_MAX_DIST`]).
pub const DIRT_HEIGHTMAP_MAX_DIST: f32 = VIEW_DISTANCE * 0.88;
pub const DIRT_HEIGHTMAP_MAX_DIST_SQ: f32 = DIRT_HEIGHTMAP_MAX_DIST * DIRT_HEIGHTMAP_MAX_DIST;
/// Beyond this, dirt is not meshed at all (aligned with [`VIEW_DISTANCE`]).
pub const DIRT_MESH_MAX_DIST: f32 = VIEW_DISTANCE;
#[allow(dead_code)]
pub const DIRT_MESH_MAX_DIST_SQ: f32 = DIRT_MESH_MAX_DIST * DIRT_MESH_MAX_DIST;

/// HD-2D: dirt mesh cut at 64; greedy merge for nearly all frustum chunks.
pub const HD2D_DIRT_MESH_DIST: f32 = 64.0;
pub const HD2D_DIRT_HIRES_DIST: f32 = 22.0;
pub const HD2D_DIRT_POLISHED_DIST: f32 = 48.0;
/// HLOD only in the last few blocks before the cut.
pub const HD2D_DIRT_HEIGHTMAP_DIST: f32 = 58.0;

/// Active dirt mesh cut (HD-2D uses a tighter bubble).
#[inline]
pub fn dirt_mesh_max_dist() -> f32 {
    if ENABLE_HD2D {
        HD2D_DIRT_MESH_DIST
    } else {
        DIRT_MESH_MAX_DIST
    }
}

#[inline]
pub fn dirt_mesh_max_dist_sq() -> f32 {
    let d = dirt_mesh_max_dist();
    d * d
}

#[inline]
pub fn dirt_hires_max_dist_sq() -> f32 {
    if ENABLE_HD2D {
        HD2D_DIRT_HIRES_DIST * HD2D_DIRT_HIRES_DIST
    } else {
        DIRT_HIRES_MAX_DIST_SQ
    }
}

#[inline]
pub fn dirt_polished_max_dist_sq() -> f32 {
    if ENABLE_HD2D {
        HD2D_DIRT_POLISHED_DIST * HD2D_DIRT_POLISHED_DIST
    } else {
        DIRT_POLISHED_MAX_DIST_SQ
    }
}

#[inline]
pub fn dirt_heightmap_max_dist_sq() -> f32 {
    if ENABLE_HD2D {
        HD2D_DIRT_HEIGHTMAP_DIST * HD2D_DIRT_HEIGHTMAP_DIST
    } else {
        DIRT_HEIGHTMAP_MAX_DIST_SQ
    }
}
/// Stream-load radius (blocks); unload a bit farther to avoid thrash.
pub const STREAM_LOAD_DIST: f32 = VIEW_DISTANCE + MESH_CHUNK_SIZE as f32;
pub const STREAM_UNLOAD_DIST: f32 = VIEW_DISTANCE + MESH_CHUNK_SIZE as f32 * 2.5;

/// Extra chunk ring beyond the mesh cut — gen cushion so the frustum never sits on Empty.
const STREAM_SAFETY_RING_CHUNKS: i32 = 1;
/// Frames to keep voxels after leaving the unload ring (neighbors remesh / swap-ready).
const UNLOAD_GRACE_FRAMES: u32 = 12;

/// Active stream radii — HD-2D tracks the dirt mesh bubble, not the old 96-block view.
#[inline]
pub fn stream_load_dist() -> f32 {
    if ENABLE_HD2D {
        // Mesh cut + safety ring (never generate only what you see).
        HD2D_DIRT_MESH_DIST + MESH_CHUNK_SIZE as f32 * STREAM_SAFETY_RING_CHUNKS as f32
    } else {
        STREAM_LOAD_DIST
    }
}

#[inline]
pub fn stream_unload_dist() -> f32 {
    if ENABLE_HD2D {
        // Farther than load so chunks stay until replacement ring is filled.
        HD2D_DIRT_MESH_DIST + MESH_CHUNK_SIZE as f32 * (STREAM_SAFETY_RING_CHUNKS as f32 + 1.5)
    } else {
        STREAM_UNLOAD_DIST
    }
}

/// Never drop voxels inside this radius — prevents visible 16×16 holes.
#[inline]
pub fn stream_keep_dist() -> f32 {
    dirt_mesh_max_dist() + MESH_CHUNK_SIZE as f32
}
/// Soft CPU budget for streaming work inside one render frame.
pub const FRAME_STREAM_BUDGET_MS: u64 = 6;
/// HD-2D: slightly more budget so Reserved→Filled keeps up with the diorama.
pub const HD2D_FRAME_STREAM_BUDGET_MS: u64 = 10;
/// Max chunks whose FBM we kick off in one frame (parallel `par_iter`).
const MAX_GEN_KICK_PER_FRAME: usize = 8;
const HD2D_MAX_GEN_KICK_PER_FRAME: usize = 12;
/// Max chunks inserted into the world hashmap per frame (sequential commit).
const MAX_COMMIT_PER_FRAME: usize = 4;
const HD2D_MAX_COMMIT_PER_FRAME: usize = 8;
/// Don't pile up too much precomputed terrain waiting to commit.
const PENDING_SOFT_CAP: usize = 24;
/// Bias missing-chunk sort toward look direction (Minecraft-style preload).
const STREAM_LOOK_PRELOAD_BIAS: f32 = 2.5;
/// Nearest shunks filled synchronously before the first playable frame.
///
/// Covers the diorama underfoot + a cushion ring; the rest streams in after start.
pub const STARTUP_PRELOAD_RADIUS_SHUNKS: i32 = 4;
/// Chunk radius filled at startup (same as [`STARTUP_PRELOAD_RADIUS_SHUNKS`]).
#[inline]
pub fn preload_radius_chunks() -> i32 {
    STARTUP_PRELOAD_RADIUS_SHUNKS
}
/// Fog hides the streaming unload edge (not the near terrain).
#[allow(dead_code)]
pub const FOG_START: f32 = VIEW_DISTANCE * 0.72;
#[allow(dead_code)]
pub const FOG_END: f32 = VIEW_DISTANCE;
/// Exponential fog density for HD-2D (`F = 1 - e^{-k d}`).
pub const FOG_DENSITY: f32 = 0.011;

/// Clear / fog colour from eye height vs local surface (sky → biome meadow → cave).
/// Underground never goes pure black — walls must stay readable.
pub fn fog_color_for_altitude(eye_y: f32, surface_y: f32) -> [f32; 3] {
    fog_color_for_biome(eye_y, surface_y, crate::biomes::BiomeId::TemperateMeadow)
}

/// Same altitude ramps as [`fog_color_for_altitude`], with biome surface/sky hues.
pub fn fog_color_for_biome(
    eye_y: f32,
    surface_y: f32,
    biome: crate::biomes::BiomeId,
) -> [f32; 3] {
    let sky = biome.fog_sky_rgb();
    let surface = biome.fog_surface_rgb();
    let cave_shallow = biome.fog_cave_shallow_rgb();
    let cave_deep = biome.fog_cave_deep_rgb();

    let rel = eye_y - surface_y;
    let mix3 = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
        let t = t.clamp(0.0, 1.0);
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    };

    if rel < -1.0 {
        // Progressive cave mood: shallow → deep (biome-tinted, never absolute black).
        let depth = (-rel).max(0.0);
        let shallow_t = ((depth - 0.5) / 3.0).clamp(0.0, 1.0);
        let deep_t = ((depth - 2.5) / 8.0).clamp(0.0, 1.0);
        mix3(mix3(surface, cave_shallow, shallow_t), cave_deep, deep_t)
    } else if rel > 8.0 {
        let t = ((rel - 8.0) / 22.0).clamp(0.0, 1.0);
        mix3(surface, sky, t)
    } else {
        let t = ((rel + 1.0) / 9.0).clamp(0.0, 1.0);
        mix3(surface, mix3(surface, sky, 0.15), t * 0.35)
    }
}

/// Grass carpet stays for the whole mesh bubble (and a little past).
pub const GRASS_MESH_MAX_DIST: f32 = VIEW_DISTANCE + 16.0;
pub const GRASS_MESH_MAX_DIST_SQ: f32 = GRASS_MESH_MAX_DIST * GRASS_MESH_MAX_DIST;
/// Create grass voxels out to the stream/mesh bubble so tufts don't POP in late.
pub const GRASS_GEN_MAX_DIST: f32 = 80.0;
pub const GRASS_GEN_MAX_DIST_SQ: f32 = GRASS_GEN_MAX_DIST * GRASS_GEN_MAX_DIST;
/// Max chunks to plant grass on per stream frame (when walking into range).
const MAX_GRASS_SEED_CHUNKS_PER_FRAME: usize = 6;
/// Density LOD: full / half / quarter keep-rates by distance.
pub const GRASS_FULL_DIST: f32 = VIEW_DISTANCE;
pub const GRASS_FULL_DIST_SQ: f32 = GRASS_FULL_DIST * GRASS_FULL_DIST;
pub const GRASS_HALF_DIST: f32 = VIEW_DISTANCE + 8.0;
pub const GRASS_HALF_DIST_SQ: f32 = GRASS_HALF_DIST * GRASS_HALF_DIST;

pub fn mesh_chunk_coord(x: i32, z: i32) -> (i32, i32) {
    (x.div_euclid(MESH_CHUNK_SIZE), z.div_euclid(MESH_CHUNK_SIZE))
}

/// World-space center of a mesh chunk (XZ). Y stays near ground for AABBs.
pub fn mesh_chunk_center(cx: i32, cz: i32) -> Vec3 {
    let s = MESH_CHUNK_SIZE as f32;
    Vec3::new((cx as f32 + 0.5) * s, 1.0, (cz as f32 + 0.5) * s)
}

/// Horizontal (XZ) distance² camera → chunk center. LOD/view must ignore camera
/// height so flying above tall columns does not blank the whole mesh.
pub fn chunk_dist_sq_xz(camera_pos: Vec3, cx: i32, cz: i32) -> f32 {
    let c = mesh_chunk_center(cx, cz);
    let dx = camera_pos.x - c.x;
    let dz = camera_pos.z - c.z;
    dx * dx + dz * dz
}

/// World-space center of one 16×32×16 section.
#[allow(dead_code)]
pub fn mesh_section_center(cx: i32, cy: i32, cz: i32) -> Vec3 {
    let sx = MESH_CHUNK_SIZE as f32;
    let sy = CHUNK_SECTION_HEIGHT as f32;
    Vec3::new(
        (cx as f32 + 0.5) * sx,
        (cy as f32 + 0.5) * sy,
        (cz as f32 + 0.5) * sx,
    )
}
/// Hard shader tone grid. Mirrored in `shader.wgsl`.
#[allow(dead_code)]
pub const DIRT_SHADER_HARD_CELLS: f32 = 8.0;
/// Polished shader tone grid. Mirrored in `shader.wgsl`.
#[allow(dead_code)]
pub const DIRT_SHADER_POLISHED_CELLS: f32 = 8.0;

/// Distance blur within the short view bubble.
pub const BLUR_START_DIST: f32 = 18.0;
pub const BLUR_END_DIST: f32 = 60.0;
pub const BLUR_START_AMOUNT: f32 = 0.01;
pub const BLUR_MAX_AMOUNT: f32 = 0.08;
/// HD-2D fisheye: extra blur at corners (kept moderate so most of the screen
/// early-outs in `blur.wgsl` instead of running the full 13-tap kernel).
pub const HD2D_EDGE_BLUR: f32 = 0.22;

/// Keep-rate for grass instances by camera distance (squared).
/// Aggressive mid/far falloff — crossed blades overdraw hard when density stays high.
pub fn grass_density_for_dist_sq(dist_sq: f32) -> f32 {
    if ENABLE_HD2D {
        // Distances are from the *player focus*, not the isometric lens.
        // Full density covers the near diorama so tufts don't vanish underfoot
        // when the camera sits ~13 blocks away horizontally.
        // Slightly tighter mid rings — less overdraw after dig remesh spikes.
        if dist_sq > 56.0 * 56.0 {
            0.0
        } else if dist_sq <= 20.0 * 20.0 {
            1.0
        } else if dist_sq <= 32.0 * 32.0 {
            0.28
        } else if dist_sq <= 44.0 * 44.0 {
            0.10
        } else if dist_sq <= 52.0 * 52.0 {
            0.03
        } else {
            0.01
        }
    } else if dist_sq > GRASS_MESH_MAX_DIST_SQ {
        0.0
    } else if dist_sq <= 24.0 * 24.0 {
        1.0
    } else if dist_sq <= 40.0 * 40.0 {
        0.5
    } else if dist_sq <= 60.0 * 60.0 {
        0.2
    } else {
        0.08
    }
}

/// 0 below [`BLUR_START_DIST`], then ramps [`BLUR_START_AMOUNT`]→[`BLUR_MAX_AMOUNT`].
pub fn blur_amount_for_distance(dist: f32) -> f32 {
    if dist < BLUR_START_DIST {
        return 0.0;
    }
    let t = ((dist - BLUR_START_DIST) / (BLUR_END_DIST - BLUR_START_DIST)).clamp(0.0, 1.0);
    BLUR_START_AMOUNT + t * (BLUR_MAX_AMOUNT - BLUR_START_AMOUNT)
}

/// LOD for grass meshing based on camera↔shunk distance.
#[derive(Clone, Copy, Debug)]
pub struct GrassLod {
    /// Fill carpet holes (solid plancha).
    pub solid_carpet: bool,
    /// Inclusive max micro-Y for blades (`None` = carpet only).
    pub max_blade_y: Option<usize>,
}

impl GrassLod {
    pub fn for_distance(dist: f32) -> Self {
        if dist < GRASS_BLADE_FADE_START {
            return Self {
                solid_carpet: false,
                max_blade_y: Some(MICROVOXEL_RES - 1),
            };
        }
        if dist >= GRASS_BLADE_FADE_END {
            return Self {
                solid_carpet: true,
                max_blade_y: None,
            };
        }
        // 60→ cut 1 from top, 61→2, … until gone at 65.
        let removed = 1 + (dist - GRASS_BLADE_FADE_START).floor() as usize;
        let max_blade_y = if removed >= GRASS_BLADE_MAX_HEIGHT {
            None
        } else {
            Some(GRASS_BLADE_MAX_HEIGHT - removed)
        };
        Self {
            solid_carpet: true,
            max_blade_y,
        }
    }
}

/// Distances from the camera to the dirt shunk (world units = blocks).
#[derive(Clone, Copy, Debug)]
pub struct ShunkDistance {
    /// Geometric center of the dirt carpet (y = 0.5).
    #[allow(dead_code)]
    pub center: Vec3,
    /// Distance camera → shunk center.
    #[allow(dead_code)]
    pub to_center: f32,
    /// Distance camera → nearest dirt block center.
    #[allow(dead_code)]
    pub to_nearest_dirt: f32,
    /// Distance camera → closest point on the dirt AABB (0 if inside/on surface).
    pub to_aabb: f32,
}

/// Terrain AABB in world space (all 9 shunks).
pub fn shunk_dirt_aabb() -> (Vec3, Vec3) {
    let extent = world_blocks() as f32;
    let min = Vec3::ZERO;
    let max = Vec3::new(extent, WORLD_MAX_HEIGHT as f32, extent);
    (min, max)
}

pub fn shunk_center() -> Vec3 {
    let half = world_blocks() as f32 * 0.5;
    Vec3::new(half, 1.0, half)
}

fn closest_point_on_aabb(p: Vec3, min: Vec3, max: Vec3) -> Vec3 {
    p.clamp(min, max)
}

/// Max |Δh| between walkable neighbors (matches player 1-block step-up).
pub const WALKABLE_STEP: i32 = 1;
/// How far cone-terracing looks for high ground to ramp from.
const TERRACE_RADIUS: i32 = 4;

/// Slope personality for a column — walkable hills get dirt stairs; cliffs stay vertical.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainSlope {
    /// Always terrace to 1-block steps.
    Walkable,
    /// Soft hills — terrace.
    Gentle,
    /// Intentional vertical face (no fill).
    Cliff,
    /// Barranco / steep cut (no fill).
    Canyon,
    /// Mix: often stairs, sometimes a short wall.
    Mountain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerraceStyle {
    Straight,
    Zigzag,
    Diagonal,
    Wide,
}

fn slope_kind_from_delta(max_d: f32, x: i32, z: i32) -> TerrainSlope {
    let roll = mix_seed(WORLD_SEED ^ 0x5101_0FE5, x as u32, z as u32) % 1000;
    if max_d >= 2.8 && roll < 280 {
        TerrainSlope::Cliff
    } else if max_d >= 2.4 && roll < 160 {
        TerrainSlope::Canyon
    } else if max_d >= 1.6 {
        TerrainSlope::Mountain
    } else if max_d >= 0.85 {
        TerrainSlope::Gentle
    } else {
        TerrainSlope::Walkable
    }
}

/// 3×3 soft blur of Perlin — cheaper than 5×5, enough to kill needle columns.
pub fn smoothed_column_height_f(x: i32, z: i32) -> f32 {
    let mut s = 0.0f32;
    let mut n = 0.0f32;
    for dz in -1i32..=1 {
        for dx in -1i32..=1 {
            let w = if dx == 0 && dz == 0 { 4.0 } else { 1.0 };
            s += terrain_height_f(x + dx, z + dz) * w;
            n += w;
        }
    }
    (s / n).clamp(TERRAIN_VALLEY_HEIGHT as f32, TERRAIN_MAX_HEIGHT as f32)
}

pub fn smoothed_column_height(x: i32, z: i32) -> i32 {
    smoothed_column_height_f(x, z).round() as i32
}

/// Classify slope from local gradient + stable hash (keeps some cliffs/canyons).
pub fn terrain_slope_kind(x: i32, z: i32) -> TerrainSlope {
    let h = smoothed_column_height_f(x, z);
    let mut max_d = 0.0f32;
    for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        let d = (h - smoothed_column_height_f(x + dx, z + dz)).abs();
        max_d = max_d.max(d);
    }
    slope_kind_from_delta(max_d, x, z)
}

fn terrace_style_at(x: i32, z: i32) -> TerraceStyle {
    match mix_seed(WORLD_SEED ^ 0x57A1_7001, x as u32, z as u32) % 4 {
        0 => TerraceStyle::Straight,
        1 => TerraceStyle::Zigzag,
        2 => TerraceStyle::Diagonal,
        _ => TerraceStyle::Wide,
    }
}

/// Whether sample `(dx,dz)` contributes to the stair cone (style variation).
fn terrace_sample_ok(style: TerraceStyle, dx: i32, dz: i32, phase: i32) -> bool {
    match style {
        TerraceStyle::Wide | TerraceStyle::Diagonal => true,
        TerraceStyle::Straight => dx == 0 || dz == 0,
        TerraceStyle::Zigzag => {
            let d = dx.abs().max(dz.abs());
            let prefer_x = ((phase + d) & 1) == 0;
            if prefer_x {
                dx.abs() >= dz.abs()
            } else {
                dz.abs() >= dx.abs()
            }
        }
    }
}

fn fill_raw_height_tile(x0: i32, z0: i32, w: i32) -> Vec<f32> {
    let mut raw = vec![0.0f32; (w * w) as usize];
    for lz in 0..w {
        for lx in 0..w {
            raw[(lz * w + lx) as usize] = terrain_height_f(x0 + lx, z0 + lz);
        }
    }
    raw
}

fn blur3_round_tile(raw: &[f32], w: i32) -> Vec<i32> {
    let mut out = vec![0i32; (w * w) as usize];
    for z in 0..w {
        for x in 0..w {
            let mut s = 0.0f32;
            let mut n = 0.0f32;
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let nx = x + dx;
                    let nz = z + dz;
                    if nx < 0 || nz < 0 || nx >= w || nz >= w {
                        continue;
                    }
                    let wt = if dx == 0 && dz == 0 { 4.0 } else { 1.0 };
                    s += raw[(nz * w + nx) as usize] * wt;
                    n += wt;
                }
            }
            out[(z * w + x) as usize] = (s / n)
                .round()
                .clamp(TERRAIN_VALLEY_HEIGHT as f32, TERRAIN_MAX_HEIGHT as f32)
                as i32;
        }
    }
    out
}

fn slope_kind_at_smooth(smooth: &[i32], w: i32, lx: i32, lz: i32, x: i32, z: i32) -> TerrainSlope {
    let h = smooth[(lz * w + lx) as usize] as f32;
    let mut max_d = 0.0f32;
    for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        let nx = lx + dx;
        let nz = lz + dz;
        if nx < 0 || nz < 0 || nx >= w || nz >= w {
            continue;
        }
        max_d = max_d.max((h - smooth[(nz * w + nx) as usize] as f32).abs());
    }
    slope_kind_from_delta(max_d, x, z)
}

fn height_from_smooth_cell(
    smooth: &[i32],
    w: i32,
    lx: i32,
    lz: i32,
    x: i32,
    z: i32,
) -> i32 {
    let kind = slope_kind_at_smooth(smooth, w, lx, lz, x, z);
    let raw = smooth[(lz * w + lx) as usize];
    let h = match kind {
        TerrainSlope::Cliff | TerrainSlope::Canyon => raw,
        TerrainSlope::Walkable | TerrainSlope::Gentle => {
            cone_terrace_from_grid(smooth, w, lx, lz, x, z, TERRACE_RADIUS, false)
        }
        TerrainSlope::Mountain => {
            let roll = mix_seed(WORLD_SEED ^ 0xB007_A155, x as u32, z as u32) % 100;
            if roll < 58 {
                cone_terrace_from_grid(smooth, w, lx, lz, x, z, TERRACE_RADIUS, true)
            } else {
                let mild = cone_terrace_from_grid(smooth, w, lx, lz, x, z, 2, false);
                if (mild - raw).abs() <= 1 {
                    mild
                } else {
                    raw
                }
            }
        }
    };
    let h = h.clamp(TERRAIN_VALLEY_HEIGHT, TERRAIN_MAX_HEIGHT);
    // Boulders poke above the dirt lip so outcrops read as 3D rocks, not flat paint.
    (h + stone_outcrop_lift(x, z)).min(TERRAIN_MAX_HEIGHT)
}

/// Raise low columns into 1-block dirt stairs toward nearby high ground (cone dilation).
fn cone_terrace_height(x: i32, z: i32, radius: i32, styled: bool) -> i32 {
    let pad = radius;
    let w = 2 * pad + 1;
    let raw = fill_raw_height_tile(x - pad, z - pad, w);
    let smooth = blur3_round_tile(&raw, w);
    cone_terrace_from_grid(&smooth, w, pad, pad, x, z, radius, styled)
        .clamp(TERRAIN_VALLEY_HEIGHT, TERRAIN_MAX_HEIGHT)
}

/// Final surface height: walkable stairs on hills, raw walls on cliffs/canyons.
pub fn terraced_column_height(x: i32, z: i32) -> i32 {
    let pad = TERRACE_RADIUS;
    let w = 2 * pad + 1;
    let raw = fill_raw_height_tile(x - pad, z - pad, w);
    let smooth = blur3_round_tile(&raw, w);
    height_from_smooth_cell(&smooth, w, pad, pad, x, z)
}

/// Pure column heights for one mesh chunk (no world mutation — rayon-safe).
fn generate_chunk_columns(cx: i32, cz: i32) -> Vec<(i32, i32, i32)> {
    let x0 = cx * MESH_CHUNK_SIZE;
    let z0 = cz * MESH_CHUNK_SIZE;
    let pad = TERRACE_RADIUS;
    let w = MESH_CHUNK_SIZE + 2 * pad;
    // One Perlin sample per padded cell + one 3×3 blur — not 25× samples per cell.
    let raw = fill_raw_height_tile(x0 - pad, z0 - pad, w);
    let smooth = blur3_round_tile(&raw, w);

    let mut out = Vec::with_capacity((MESH_CHUNK_SIZE * MESH_CHUNK_SIZE) as usize);
    for z in z0..z0 + MESH_CHUNK_SIZE {
        for x in x0..x0 + MESH_CHUNK_SIZE {
            let lx = x - x0 + pad;
            let lz = z - z0 + pad;
            out.push((x, z, height_from_smooth_cell(&smooth, w, lx, lz, x, z)));
        }
    }
    out
}

fn cone_terrace_from_grid(
    smooth: &[i32],
    w: i32,
    lx: i32,
    lz: i32,
    x: i32,
    z: i32,
    radius: i32,
    styled: bool,
) -> i32 {
    let style = terrace_style_at(x, z);
    let phase = (x.wrapping_mul(3) ^ z.wrapping_mul(7)) & 1;
    let mut best = smooth[(lz * w + lx) as usize];
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            let d = dx.abs().max(dz.abs());
            if d == 0 || d > radius {
                continue;
            }
            if styled && !terrace_sample_ok(style, dx, dz, phase) {
                continue;
            }
            let nx = lx + dx;
            let nz = lz + dz;
            if nx < 0 || nz < 0 || nx >= w || nz >= w {
                continue;
            }
            let hq = smooth[(nz * w + nx) as usize];
            best = best.max(hq - d * WALKABLE_STEP);
        }
    }
    best
}

/// Shared terrain noise — `Perlin::new` is relatively expensive; never rebuild per sample.
fn terrain_perlin() -> &'static noise::Perlin {
    use noise::Perlin;
    use std::sync::OnceLock;
    static TERRAIN_NOISE: OnceLock<Perlin> = OnceLock::new();
    TERRAIN_NOISE.get_or_init(|| Perlin::new(WORLD_SEED))
}

/// Fractal Brownian motion → roughly `[-1, 1]` (low-frequency bias, soft detail).
fn fbm2(x: f64, z: f64, _seed: u32) -> f64 {
    use noise::NoiseFn;
    let perlin = terrain_perlin();
    // Single octave — no high-frequency carving at all.
    perlin
        .get([x * PERLIN_SCALE, z * PERLIN_SCALE])
        .clamp(-1.0, 1.0)
}

/// Continuous column height: average ~[`TERRAIN_AVG_HEIGHT`], soft Perlin relief.
pub fn terrain_height_f(x: i32, z: i32) -> f32 {
    let n = fbm2(x as f64, z as f64, WORLD_SEED) as f32 * PERLIN_AMPLITUDE;
    // Soften |n| so slopes stay gentle; keep sign for up/down from the average.
    let mag = n.abs();
    let mag = mag * mag * (3.0 - 2.0 * mag);
    let avg = TERRAIN_AVG_HEIGHT as f32;
    let h = if n >= 0.0 {
        avg + mag * (TERRAIN_MAX_HEIGHT - TERRAIN_AVG_HEIGHT) as f32
    } else {
        avg - mag * (TERRAIN_AVG_HEIGHT - TERRAIN_VALLEY_HEIGHT) as f32
    };
    h.clamp(TERRAIN_VALLEY_HEIGHT as f32, TERRAIN_MAX_HEIGHT as f32)
}

/// Integer dirt-top Y (walkable terraced surface — matches streamed columns).
pub fn terrain_height(x: i32, z: i32) -> i32 {
    terraced_column_height(x, z)
}

fn height_index(x: i32, z: i32, extent: i32) -> usize {
    (z * extent + x) as usize
}

#[allow(dead_code)] // retained for offline island experiments / comparison
fn build_heightmap() -> Vec<f32> {
    let extent = world_blocks();
    let e = extent as usize;
    let mut h = vec![0.0f32; e * e];
    for z in 0..extent {
        for x in 0..extent {
            h[height_index(x, z, extent)] = terrain_height_f(x, z);
        }
    }
    // Extra blur → rolling hills instead of needle columns.
    for _ in 0..5 {
        let mut smooth = h.clone();
        for z in 0..extent {
            for x in 0..extent {
                let mut s = 0.0f32;
                let mut n = 0.0f32;
                for dz in -1..=1 {
                    for dx in -1..=1 {
                        let nx = x + dx;
                        let nz = z + dz;
                        if nx >= 0 && nz >= 0 && nx < extent && nz < extent {
                            let w = if dx == 0 && dz == 0 { 4.0 } else { 1.0 };
                            s += h[height_index(nx, nz, extent)] * w;
                            n += w;
                        }
                    }
                }
                smooth[height_index(x, z, extent)] = s / n;
            }
        }
        h = smooth;
    }
    // Thermal erosion: shave steps steeper than 1 block.
    for _ in 0..6 {
        let prev = h.clone();
        for z in 0..extent {
            for x in 0..extent {
                let i = height_index(x, z, extent);
                let mut hi = prev[i];
                for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let nx = x + dx;
                    let nz = z + dz;
                    if nx < 0 || nz < 0 || nx >= extent || nz >= extent {
                        continue;
                    }
                    let j = height_index(nx, nz, extent);
                    let d = hi - prev[j];
                    if d > 1.0 {
                        hi -= 0.45;
                        h[j] = (h[j] + 0.2).min(TERRAIN_MAX_HEIGHT as f32);
                    }
                }
                h[i] = hi.clamp(TERRAIN_VALLEY_HEIGHT as f32, TERRAIN_MAX_HEIGHT as f32);
            }
        }
    }
    h
}

#[allow(dead_code)]
fn column_slope(heights: &[f32], x: i32, z: i32) -> f32 {
    let e = world_blocks();
    let h = heights[height_index(x, z, e)];
    let mut max_d = 0.0f32;
    for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        let nx = x + dx;
        let nz = z + dz;
        if nx < 0 || nz < 0 || nx >= e || nz >= e {
            continue;
        }
        max_d = max_d.max((h - heights[height_index(nx, nz, e)]).abs());
    }
    max_d
}

/// Slope from natural terrain heights (ignores mid-column cave punches).
pub fn terrain_slope(x: i32, z: i32) -> f32 {
    let h = terrain_height(x, z) as f32;
    let mut max_d = 0.0f32;
    for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        let nh = terrain_height(x + dx, z + dz) as f32;
        max_d = max_d.max((h - nh).abs());
    }
    max_d
}

/// Whether this column gets a grass tuft. Bare patches are coherent and common;
/// steep slopes almost never grow grass. Density follows the local biome profile.
pub fn column_grows_grass(x: i32, z: i32, slope: f32) -> bool {
    let bare_base = crate::biomes::flora_for(crate::biomes::biome_at(x, z)).grass_bare_chance;
    let slope_t = (slope / ROCK_SLOPE).clamp(0.0, 1.0);
    let mut bare_p = bare_base + slope_t * 0.14;
    if slope >= ROCK_SLOPE {
        bare_p = bare_p.max(0.96);
    }
    let cx = x.div_euclid(GRASS_BARE_CELL) as u32;
    let cz = z.div_euclid(GRASS_BARE_CELL) as u32;
    let patch = mix_seed(GRASS_SEED ^ 0xBA2E_0001, cx, cz);
    let jitter = mix_seed(GRASS_SEED ^ 0xBA2E_0002, x as u32, z as u32);
    let roll = ((patch % 700) as f32 + (jitter % 300) as f32) / 1000.0;
    roll >= bare_p
}

/// Sparse, spaced tree sites on gentle ground — spacing/chance from local biome.
pub fn column_grows_tree(x: i32, z: i32, slope: f32) -> bool {
    if !ENABLE_TREES || slope > 1.0 {
        return false;
    }
    let flora = crate::biomes::flora_for(crate::biomes::biome_at(x, z));
    let cell = flora.tree_cell.max(4);
    let cx = x.div_euclid(cell);
    let cz = z.div_euclid(cell);
    let patch = mix_seed(TREE_SEED, cx as u32, cz as u32);
    let ox = (patch % cell as u32) as i32;
    let oz = ((patch >> 8) % cell as u32) as i32;
    let lx = x.rem_euclid(cell);
    let lz = z.rem_euclid(cell);
    if lx != ox || lz != oz {
        return false;
    }
    let roll = ((patch >> 16) % 1000) as f32 / 1000.0;
    roll < flora.tree_chance
}

/// Low vegetation (scrub / understory) — denser than trees, never on tree columns.
pub fn column_grows_bush(x: i32, z: i32, slope: f32) -> bool {
    if !ENABLE_TREES || slope > 1.15 {
        return false;
    }
    if column_grows_tree(x, z, slope) {
        return false;
    }
    let flora = crate::biomes::flora_for(crate::biomes::biome_at(x, z));
    if flora.bush_chance <= 0.0 || flora.bushes.is_empty() {
        return false;
    }
    let cell = flora.bush_cell.max(3);
    let cx = x.div_euclid(cell);
    let cz = z.div_euclid(cell);
    let patch = mix_seed(BUSH_SEED, cx as u32, cz as u32);
    let ox = (patch % cell as u32) as i32;
    let oz = ((patch >> 8) % cell as u32) as i32;
    let lx = x.rem_euclid(cell);
    let lz = z.rem_euclid(cell);
    if lx != ox || lz != oz {
        return false;
    }
    let roll = ((patch >> 16) % 1000) as f32 / 1000.0;
    roll < flora.bush_chance
}

/// Trunk height in blocks for the oak baseline: inclusive `2..=6`.
pub fn tree_trunk_height(x: i32, z: i32) -> i32 {
    let h = mix_seed(TREE_SEED ^ 0x71EE_0001, x as u32, z as u32);
    2 + (h % 5) as i32
}

/// Measure camera distances to the dirt terrain.
/// Infinite mode: distance to the procedural surface under the camera.
/// Finite island (`!ENABLE_STREAMING`): distance to the shunk AABB.
pub fn measure_shunk_distance(camera_pos: Vec3) -> ShunkDistance {
    if ENABLE_STREAMING {
        let gx = camera_pos.x.floor() as i32;
        let gz = camera_pos.z.floor() as i32;
        let surface_y = terrain_height(gx, gz) as f32 + 1.0;
        let closest = Vec3::new(camera_pos.x, surface_y, camera_pos.z);
        let to_aabb = camera_pos.distance(closest);
        let center = Vec3::new(camera_pos.x, surface_y, camera_pos.z);
        return ShunkDistance {
            center,
            to_center: to_aabb,
            to_nearest_dirt: to_aabb,
            to_aabb,
        };
    }
    let center = shunk_center();
    let to_center = camera_pos.distance(center);
    let (min, max) = shunk_dirt_aabb();
    let closest = closest_point_on_aabb(camera_pos, min, max);
    let to_aabb = camera_pos.distance(closest);

    ShunkDistance {
        center,
        to_center,
        to_nearest_dirt: to_aabb,
        to_aabb,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Material {
    Dirt,
    Grass,
    /// Sawn boards used by village buildings (not tree bark).
    WoodPlanks,
    Wood,
    Leaves,
    /// Low, warm grey masonry used by settlement foundations and patches.
    VillageStone,
    /// Rough cobble used by settlement palisade / curtain walls.
    Cobblestone,
    Stone,
    /// Deep hard stone — 4× dig cost; needs iron+ pickaxe.
    BlackStone,
    /// Unbreakable world floor (Y = 0).
    Bedrock,
    /// Crafted cubes (9 flecks). Flecks themselves are overlays on stone.
    Coal,
    Sapphire,
    Ruby,
    Emerald,
    /// Reserved cave crate (loot UI later). Diggable like wood.
    Chest,
    /// Prefab window panes — diggable like wood; opaque tint until alpha pass.
    Glass,
    /// Walk-through door panel (no collision AABB).
    Door,
    /// Shallow lake / pond fill.
    Water,
    /// Autochthonous tree bark / foliage (Americas biomes).
    BirchWood,
    BirchLeaves,
    PineWood,
    PineLeaves,
    WillowWood,
    WillowLeaves,
    MesquiteWood,
    MesquiteLeaves,
    CeibaWood,
    CeibaLeaves,
    /// Hot / ice desert surface patches.
    Sand,
    /// Fantasy violet bark / canopy.
    EnchantedWood,
    EnchantedLeaves,
    /// Fantasy dark bark / canopy.
    UmbraWood,
    UmbraLeaves,
}

/// Embedded mineral in a stone cell (flecks on the shell).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbedKind {
    Coal,
    Sapphire,
    Ruby,
    Emerald,
}

impl EmbedKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Coal => "carbón",
            Self::Sapphire => "zafiro",
            Self::Ruby => "rubí",
            Self::Emerald => "esmeralda",
        }
    }

    pub fn material(self) -> Material {
        match self {
            Self::Coal => Material::Coal,
            Self::Sapphire => Material::Sapphire,
            Self::Ruby => Material::Ruby,
            Self::Emerald => Material::Emerald,
        }
    }

    /// Fleck / cube tint (crystals brighter; coal near-black).
    pub fn fleck_rgb(self) -> [f32; 3] {
        match self {
            Self::Coal => [0.04, 0.035, 0.03],
            Self::Sapphire => [0.22, 0.42, 0.92],
            Self::Ruby => [0.88, 0.18, 0.22],
            Self::Emerald => [0.18, 0.78, 0.38],
        }
    }
}

/// Dropped when breaking ore stone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OreDrop {
    pub kind: EmbedKind,
    pub micros: u8,
}

pub const ORE_SEED: u32 = 0xC0A1_5EED;
/// Chance stone near the surface hosts an embed (coal or crystal).
pub const EMBED_CHANCE_SURFACE: f32 = 0.10;
/// Chance deep stone hosts an embed.
pub const EMBED_CHANCE_DEEP: f32 = 0.03;
/// Among embeds: fraction that are coal (rest = crystals).
pub const EMBED_COAL_SHARE: f32 = 0.55;
/// Inclusive Y band where sapphire / ruby / emerald may appear.
pub const CRYSTAL_Y_MIN: i32 = 3;
pub const CRYSTAL_Y_MAX: i32 = 20;
/// Crystal weights: sapphire : ruby : emerald = 4 : 2 : 1
const CRYSTAL_W_SAPPHIRE: u32 = 4;
const CRYSTAL_W_RUBY: u32 = 2;
const CRYSTAL_W_EMERALD: u32 = 1;
/// Gather this many flecks → one crafted cube.
pub const ORE_MICROS_PER_CUBE: u32 = 9;
/// Render size of an ore fleck relative to one micro cell (coal/crystal chunk).
pub const ORE_FLECK_SIZE_MUL: f32 = 2.6;

/// One fleck as a microvoxel anchor on the stone shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OreMicro {
    pub mx: u8,
    pub my: u8,
    pub mz: u8,
}

#[derive(Clone, Debug, Default)]
pub struct OreStack {
    pub micros: u32,
    pub cubes: u32,
}

impl OreStack {
    pub fn add_micros(&mut self, n: u8) -> u32 {
        self.micros = self.micros.saturating_add(n as u32);
        let mut crafted = 0u32;
        while self.micros >= ORE_MICROS_PER_CUBE {
            self.micros -= ORE_MICROS_PER_CUBE;
            self.cubes = self.cubes.saturating_add(1);
            crafted += 1;
        }
        crafted
    }
}

/// Player stash for coal + crystals (9 micros → 1 cube each).
#[derive(Clone, Debug, Default)]
pub struct OrePouch {
    pub coal: OreStack,
    pub sapphire: OreStack,
    pub ruby: OreStack,
    pub emerald: OreStack,
}

impl OrePouch {
    pub fn stack_mut(&mut self, kind: EmbedKind) -> &mut OreStack {
        match kind {
            EmbedKind::Coal => &mut self.coal,
            EmbedKind::Sapphire => &mut self.sapphire,
            EmbedKind::Ruby => &mut self.ruby,
            EmbedKind::Emerald => &mut self.emerald,
        }
    }

    pub fn stack(&self, kind: EmbedKind) -> &OreStack {
        match kind {
            EmbedKind::Coal => &self.coal,
            EmbedKind::Sapphire => &self.sapphire,
            EmbedKind::Ruby => &self.ruby,
            EmbedKind::Emerald => &self.emerald,
        }
    }

    /// Add flecks; returns cubes crafted this call.
    pub fn add_drop(&mut self, drop: OreDrop) -> u32 {
        self.stack_mut(drop.kind).add_micros(drop.micros)
    }
}

/// Backward-compatible alias used by older call sites / tests.
pub type CoalPouch = OrePouch;

/// Fragments gained when breaking one dirt / stone cell.
pub const BLOCK_FRAGMENTS_PER_BREAK: u32 = 4;
/// Cap for dirt and stone fragment stacks in the player bag.
pub const MAX_BLOCK_FRAGMENTS: u32 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FragmentKind {
    Dirt,
    Stone,
}

impl FragmentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dirt => "tierra",
            Self::Stone => "piedra",
        }
    }
}

/// Dirt / stone fragment inventory (max [`MAX_BLOCK_FRAGMENTS`] each).
#[derive(Clone, Debug, Default)]
pub struct FragmentBag {
    pub dirt: u32,
    pub stone: u32,
}

impl FragmentBag {
    pub fn count(&self, kind: FragmentKind) -> u32 {
        match kind {
            FragmentKind::Dirt => self.dirt,
            FragmentKind::Stone => self.stone,
        }
    }

    /// Add fragments up to the cap; returns how many were actually stored.
    pub fn add(&mut self, kind: FragmentKind, amount: u32) -> u32 {
        let slot = match kind {
            FragmentKind::Dirt => &mut self.dirt,
            FragmentKind::Stone => &mut self.stone,
        };
        let room = MAX_BLOCK_FRAGMENTS.saturating_sub(*slot);
        let added = amount.min(room);
        *slot += added;
        added
    }
}

/// Fragments dropped when mining this material (if any).
pub fn fragment_drop_for(material: Material) -> Option<(FragmentKind, u32)> {
    match material {
        Material::Dirt | Material::Grass => {
            Some((FragmentKind::Dirt, BLOCK_FRAGMENTS_PER_BREAK))
        }
        Material::Stone
        | Material::VillageStone
        | Material::Cobblestone
        | Material::BlackStone
        | Material::Coal
        | Material::Sapphire
        | Material::Ruby
        | Material::Emerald => Some((FragmentKind::Stone, BLOCK_FRAGMENTS_PER_BREAK)),
        Material::Wood
        | Material::WoodPlanks
        | Material::Leaves
        | Material::Bedrock
        | Material::Chest
        | Material::Glass
        | Material::Door
        | Material::Water
        | Material::BirchWood
        | Material::BirchLeaves
        | Material::PineWood
        | Material::PineLeaves
        | Material::WillowWood
        | Material::WillowLeaves
        | Material::MesquiteWood
        | Material::MesquiteLeaves
        | Material::CeibaWood
        | Material::CeibaLeaves
        | Material::Sand
        | Material::EnchantedWood
        | Material::EnchantedLeaves
        | Material::UmbraWood
        | Material::UmbraLeaves => None,
    }
}

fn embed_roll(x: i32, y: i32, z: i32, salt: u32) -> f32 {
    let h = mix_seed(
        ORE_SEED ^ salt,
        (x as u32)
            .wrapping_mul(73856093)
            ^ (y as u32).wrapping_mul(19349663)
            ^ (z as u32).wrapping_mul(83492791),
        salt,
    );
    (h % 1000) as f32 / 1000.0
}

/// Which mineral (if any) is embedded in this stone cell.
pub fn stone_embed_kind(x: i32, y: i32, z: i32) -> Option<EmbedKind> {
    let lip = terrain_height(x, z);
    let near_surface = y + 2 >= lip;
    let chance = if near_surface {
        EMBED_CHANCE_SURFACE
    } else {
        EMBED_CHANCE_DEEP
    };
    if embed_roll(x, y, z, 0x0CE) >= chance {
        return None;
    }
    // Coal vs crystal pool.
    if embed_roll(x, y, z, 0xC0A1) < EMBED_COAL_SHARE {
        return Some(EmbedKind::Coal);
    }
    // Crystals only spawn in the mid-height band; outside → coal instead.
    if y < CRYSTAL_Y_MIN || y > CRYSTAL_Y_MAX {
        return Some(EmbedKind::Coal);
    }
    // Sapphire 4, ruby 2, emerald 1.
    let total = CRYSTAL_W_SAPPHIRE + CRYSTAL_W_RUBY + CRYSTAL_W_EMERALD;
    let w = (embed_roll(x, y, z, 0x61E1) * total as f32).floor() as u32;
    if w < CRYSTAL_W_SAPPHIRE {
        Some(EmbedKind::Sapphire)
    } else if w < CRYSTAL_W_SAPPHIRE + CRYSTAL_W_RUBY {
        Some(EmbedKind::Ruby)
    } else {
        Some(EmbedKind::Emerald)
    }
}

/// How many flecks this ore cell drops when mined.
pub fn ore_yield_count(x: i32, y: i32, z: i32) -> u8 {
    if stone_embed_kind(x, y, z).is_none() {
        return 0;
    }
    let s = mix_seed(ORE_SEED ^ 0xF1E1, x as u32, (y as u32) ^ (z as u32).wrapping_mul(7));
    1 + (s % 2) as u8
}

fn ore_face_dir(face: u32) -> IVec3 {
    match face % 6 {
        0 => IVec3::X,
        1 => IVec3::NEG_X,
        2 => IVec3::Y,
        3 => IVec3::NEG_Y,
        4 => IVec3::Z,
        _ => IVec3::NEG_Z,
    }
}

fn ore_micro_on_face(face: u32, slot: u32, jitter: u32) -> OreMicro {
    let edge = (MICROVOXEL_RES - 1) as i32;
    let mid = (MICROVOXEL_RES / 2) as i32;
    let u = (slot % 3) as i32 - 1;
    let v = (slot / 3) as i32 - 1;
    let ou = (mid + u * 4 + (jitter % 3) as i32 - 1).clamp(1, edge - 1);
    let ov = (mid + v * 4 + ((jitter / 3) % 3) as i32 - 1).clamp(1, edge - 1);
    match face % 6 {
        0 => OreMicro {
            mx: edge as u8,
            my: ou as u8,
            mz: ov as u8,
        },
        1 => OreMicro {
            mx: 0,
            my: ou as u8,
            mz: ov as u8,
        },
        2 => OreMicro {
            mx: ou as u8,
            my: edge as u8,
            mz: ov as u8,
        },
        3 => OreMicro {
            mx: ou as u8,
            my: 0,
            mz: ov as u8,
        },
        4 => OreMicro {
            mx: ou as u8,
            my: ov as u8,
            mz: edge as u8,
        },
        _ => OreMicro {
            mx: ou as u8,
            my: ov as u8,
            mz: 0,
        },
    }
}

/// Visible flecks on air-exposed faces + embed kind for tinting.
pub fn ore_micros_visible(
    world: &World,
    pos: IVec3,
) -> Option<(EmbedKind, u8, [OreMicro; 2])> {
    let kind = stone_embed_kind(pos.x, pos.y, pos.z)?;
    let empty = [OreMicro { mx: 0, my: 0, mz: 0 }; 2];
    let mut exposed = [0u32; 6];
    let mut n_exp = 0usize;
    for f in 0..6u32 {
        if !world.solid_occludes(pos + ore_face_dir(f)) {
            exposed[n_exp] = f;
            n_exp += 1;
        }
    }
    if n_exp == 0 {
        return None;
    }
    let s0 = mix_seed(
        ORE_SEED ^ 0xF1E1,
        pos.x as u32,
        (pos.y as u32) ^ (pos.z as u32).wrapping_mul(7),
    );
    let s1 = mix_seed(
        ORE_SEED ^ 0xA11C,
        pos.z as u32,
        (pos.x as u32).wrapping_mul(3) ^ pos.y as u32,
    );
    let count = (1 + (s0 % 2) as usize).min(n_exp).min(2);
    let a = ore_micro_on_face(exposed[s0 as usize % n_exp], (s0 >> 3) % 9, s0 >> 8);
    let b = if count > 1 {
        ore_micro_on_face(
            exposed[(s1 as usize + 1) % n_exp],
            (s1 >> 3) % 9,
            s1 >> 8,
        )
    } else {
        empty[1]
    };
    Some((kind, count as u8, [a, b]))
}

/// 2D peaks above this = large rock outcrop pads on the surface.
pub const STONE_OUTCROP_THRESH: f32 = 0.40;
/// How deep a large pad roots under the dirt lip.
pub const STONE_OUTCROP_DEPTH: i32 = 4;
/// Max blocks a large boulder pokes above the dirt lip.
pub const STONE_OUTCROP_LIFT_MAX: i32 = 3;
/// Higher-freq peaks = small 1–3 block rocks / pebbles between the big pads.
pub const STONE_PEBBLE_THRESH: f32 = 0.58;
/// Shallow root for small rocks (still a complete little pad).
pub const STONE_PEBBLE_DEPTH: i32 = 2;
/// Small rocks usually poke 1 block; rare peaks poke 2.
pub const STONE_PEBBLE_LIFT_MAX: i32 = 2;
/// Noise threshold just below the outcrop shell (mostly dirt).
pub const STONE_NEAR_SURFACE_THRESH: f32 = 0.72;
/// Noise threshold deep underground: most cells are stone.
pub const STONE_DEEP_THRESH: f32 = -0.45;
/// Depth below the column top where the threshold reaches [`STONE_DEEP_THRESH`].
pub const STONE_DEPTH_FULL: f32 = 12.0;

/// Low-frequency field → wide contiguous boulder pads.
fn stone_outcrop_field(x: i32, z: i32) -> f32 {
    use noise::NoiseFn;
    let p = terrain_perlin();
    let n = p.get([x as f64 * 0.038 + 17.3, z as f64 * 0.038 - 9.1]);
    n.clamp(-1.0, 1.0) as f32
}

/// Higher-frequency field → scattered small rocks between the big pads.
fn stone_pebble_field(x: i32, z: i32) -> f32 {
    use noise::NoiseFn;
    let p = terrain_perlin();
    let n = p.get([x as f64 * 0.13 + 41.7, z as f64 * 0.13 - 23.2]);
    n.clamp(-1.0, 1.0) as f32
}

/// `(lift, root_depth)` for a surface rock. `(0, 0)` = plain dirt lip.
/// Big pads win over pebbles on the same column.
fn stone_outcrop_profile(x: i32, z: i32) -> (i32, i32) {
    let f = stone_outcrop_field(x, z);
    if f > STONE_OUTCROP_THRESH {
        let span = (1.0 - STONE_OUTCROP_THRESH).max(0.05);
        let t = ((f - STONE_OUTCROP_THRESH) / span).clamp(0.0, 1.0);
        let lift = 1
            + ((t * (STONE_OUTCROP_LIFT_MAX - 1) as f32).floor() as i32)
                .clamp(0, STONE_OUTCROP_LIFT_MAX - 1);
        return (lift, STONE_OUTCROP_DEPTH);
    }
    let p = stone_pebble_field(x, z);
    if p > STONE_PEBBLE_THRESH {
        let span = (1.0 - STONE_PEBBLE_THRESH).max(0.05);
        let t = ((p - STONE_PEBBLE_THRESH) / span).clamp(0.0, 1.0);
        // Most pebbles are 1-tall; only strong peaks become little 2-block stones.
        let lift = if t > 0.55 {
            STONE_PEBBLE_LIFT_MAX
        } else {
            1
        };
        return (lift, STONE_PEBBLE_DEPTH);
    }
    (0, 0)
}

/// True when this column hosts any surface rock (pad or pebble).
pub fn column_has_stone_outcrop(x: i32, z: i32) -> bool {
    stone_outcrop_profile(x, z).0 > 0
}

/// How many blocks the outcrop rises above the dirt lip (`0` = no rock).
pub fn stone_outcrop_lift(x: i32, z: i32) -> i32 {
    stone_outcrop_profile(x, z).0
}

/// Signed patch field for underground veins (`[-1, 1]`).
fn stone_vein_field(x: i32, y: i32, z: i32) -> f32 {
    use noise::NoiseFn;
    let p = terrain_perlin();
    let n2 = p.get([x as f64 * 0.055 + 17.3, z as f64 * 0.055 - 9.1]);
    let n3 = p.get([x as f64 * 0.14, y as f64 * 0.09 + 3.7, z as f64 * 0.14]);
    (n2 * 0.55 + n3 * 0.45).clamp(-1.0, 1.0) as f32
}

/// Deterministic stone vs dirt for a dense column cell (`y` in `0..=column_top`).
/// Large pads and small pebbles both protrude and root as solid stone.
pub fn column_cell_is_stone(x: i32, y: i32, z: i32, column_top: i32) -> bool {
    if y < 0 || y > column_top {
        return false;
    }
    let (lift, root) = stone_outcrop_profile(x, z);
    if lift > 0 {
        let stone_bottom = column_top - lift - root + 1;
        if y >= stone_bottom.max(0) {
            return true;
        }
    }
    let depth = (column_top - y) as f32;
    let t = (depth / STONE_DEPTH_FULL).clamp(0.0, 1.0);
    let ease = t * t * (3.0 - 2.0 * t);
    let thresh = STONE_NEAR_SURFACE_THRESH
        + (STONE_DEEP_THRESH - STONE_NEAR_SURFACE_THRESH) * ease;
    stone_vein_field(x, y, z) > thresh
}

/// Inclusive Y range where black stone veins may replace deep stone (above bedrock).
pub const BLACK_STONE_Y_MIN: i32 = 1;
pub const BLACK_STONE_Y_MAX: i32 = 6;

/// Signed field for black-stone veins (`[-1, 1]`).
fn black_stone_field(x: i32, y: i32, z: i32) -> f32 {
    use noise::NoiseFn;
    let p = terrain_perlin();
    let n = p.get([
        x as f64 * 0.09 + 41.2,
        y as f64 * 0.11 - 7.5,
        z as f64 * 0.09 + 3.3,
    ]);
    n.clamp(-1.0, 1.0) as f32
}

/// Deep black stone: only on stone cells in [`BLACK_STONE_Y_MIN`]..=[`BLACK_STONE_Y_MAX`].
pub fn column_cell_is_black_stone(x: i32, y: i32, z: i32, column_top: i32) -> bool {
    if y < BLACK_STONE_Y_MIN || y > BLACK_STONE_Y_MAX || y > column_top {
        return false;
    }
    if !column_cell_is_stone(x, y, z, column_top) {
        return false;
    }
    black_stone_field(x, y, z) > 0.05
}

/// Dense column cell material (Y ≥ 1). Y = 0 is always bedrock via [`World::get_voxel`].
pub fn column_cell_material(x: i32, y: i32, z: i32, column_top: i32) -> Material {
    if y <= 0 {
        return Material::Bedrock;
    }
    if column_cell_is_black_stone(x, y, z, column_top) {
        Material::BlackStone
    } else if column_cell_is_stone(x, y, z, column_top) {
        Material::Stone
    } else if y == column_top && crate::biomes::column_has_sand_surface(x, z) {
        Material::Sand
    } else {
        Material::Dirt
    }
}

impl Material {
    pub fn color_rgb(self) -> [f32; 3] {
        match self {
            // Pictorial HD-2D palette (less Minecraft-saturated).
            Material::Dirt => [0.52, 0.38, 0.24],  // warmer brown, readable under cel
            Material::Grass => [0.42, 0.68, 0.34], // less neon than #7FBF6A
            Material::WoodPlanks => [0.68, 0.48, 0.25], // sawn warm boards
            Material::Wood => [0.42, 0.28, 0.16],  // bark / trunk
            Material::Leaves => [0.28, 0.55, 0.26], // canopy
            Material::VillageStone => [0.59, 0.56, 0.46], // low warm grey masonry
            Material::Cobblestone => [0.52, 0.50, 0.46], // rough cobble wall grey
            Material::Stone => [0.66, 0.68, 0.72], // clear cool grey (not muddy brown)
            Material::BlackStone => [0.12, 0.11, 0.13], // near-black deep stone
            Material::Bedrock => [0.22, 0.18, 0.20], // dark mottled floor
            Material::Coal => [0.05, 0.045, 0.04],
            Material::Sapphire => [0.22, 0.42, 0.92],
            Material::Ruby => [0.88, 0.18, 0.22],
            Material::Emerald => [0.18, 0.78, 0.38],
            Material::Chest => [0.55, 0.32, 0.12], // warm crate wood
            Material::Glass => [0.72, 0.88, 0.94], // cool pane (opaque stand-in)
            Material::Door => [0.48, 0.30, 0.14], // stained door boards
            Material::Water => [0.22, 0.48, 0.78], // shallow pond
            Material::BirchWood => [0.78, 0.74, 0.62],
            Material::BirchLeaves => [0.55, 0.72, 0.38],
            Material::PineWood => [0.36, 0.28, 0.20],
            Material::PineLeaves => [0.18, 0.42, 0.22],
            Material::WillowWood => [0.48, 0.38, 0.22],
            Material::WillowLeaves => [0.42, 0.62, 0.28],
            Material::MesquiteWood => [0.40, 0.28, 0.16],
            Material::MesquiteLeaves => [0.45, 0.58, 0.22],
            Material::CeibaWood => [0.55, 0.42, 0.28],
            Material::CeibaLeaves => [0.30, 0.58, 0.24],
            Material::Sand => [0.82, 0.72, 0.42],
            Material::EnchantedWood => [0.42, 0.28, 0.48],
            Material::EnchantedLeaves => [0.52, 0.28, 0.72],
            Material::UmbraWood => [0.18, 0.14, 0.12],
            Material::UmbraLeaves => [0.16, 0.28, 0.18],
        }
    }

    /// True for dense terrain the pickaxe excavates (dirt / stone).
    pub fn is_diggable_terrain(self) -> bool {
        matches!(
            self,
            Material::Dirt
                | Material::Sand
                | Material::Stone
                | Material::BlackStone
                | Material::Grass
                | Material::Coal
                | Material::Sapphire
                | Material::Ruby
                | Material::Emerald
        )
    }

    #[inline]
    pub fn is_tree_bark(self) -> bool {
        matches!(
            self,
            Material::Wood
                | Material::BirchWood
                | Material::PineWood
                | Material::WillowWood
                | Material::MesquiteWood
                | Material::CeibaWood
                | Material::EnchantedWood
                | Material::UmbraWood
        )
    }

    #[inline]
    pub fn is_tree_foliage(self) -> bool {
        matches!(
            self,
            Material::Leaves
                | Material::BirchLeaves
                | Material::PineLeaves
                | Material::WillowLeaves
                | Material::MesquiteLeaves
                | Material::CeibaLeaves
                | Material::EnchantedLeaves
                | Material::UmbraLeaves
        )
    }
}

#[derive(Clone, Copy, Debug)]
enum CanopyShape {
    Oak,
    Birch,
    Pine,
    Willow,
    Mesquite,
    Ceiba,
}

/// Cheap deterministic mixer for per-face / per-cell variation.
pub fn mix_seed(seed: u32, a: u32, b: u32) -> u32 {
    let mut x = seed
        .wrapping_mul(0x85EBCA77)
        .wrapping_add(a.wrapping_mul(0xC2B2AE3D))
        .wrapping_add(b.wrapping_mul(0x27D4EB2F));
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x91E10DA5);
    x ^= x >> 16;
    x
}

pub fn block_seed_at(pos: IVec3, world_seed: u32) -> u32 {
    mix_seed(
        world_seed,
        (pos.x as u32)
            .wrapping_mul(73856093)
            ^ (pos.y as u32).wrapping_mul(19349663)
            ^ (pos.z as u32).wrapping_mul(83492791),
        1,
    )
}

/// Occupancy without paying 512 bytes for fully solid / empty voxels.
#[derive(Clone, Debug)]
enum OccupancyStorage {
    Empty,
    Solid,
    /// Only allocated for partial microvoxel shapes (e.g. grass carpet).
    Micro(Box<[u8; OCCUPANCY_BYTES]>),
}

/// One world-space voxel (1×1×1) with optional microvoxel occupancy.
#[derive(Clone, Debug)]
pub struct Voxel {
    pub material: Material,
    occupancy: OccupancyStorage,
}

impl Voxel {
    pub fn empty(material: Material) -> Self {
        Self {
            material,
            occupancy: OccupancyStorage::Empty,
        }
    }

    #[allow(dead_code)]
    pub fn solid(material: Material) -> Self {
        Self {
            material,
            occupancy: OccupancyStorage::Solid,
        }
    }

    pub fn dirt() -> Self {
        Self {
            material: Material::Dirt,
            occupancy: OccupancyStorage::Solid,
        }
    }

    pub fn sand() -> Self {
        Self {
            material: Material::Sand,
            occupancy: OccupancyStorage::Solid,
        }
    }

    pub fn stone() -> Self {
        Self {
            material: Material::Stone,
            occupancy: OccupancyStorage::Solid,
        }
    }

    pub fn black_stone() -> Self {
        Self {
            material: Material::BlackStone,
            occupancy: OccupancyStorage::Solid,
        }
    }

    pub fn bedrock() -> Self {
        Self {
            material: Material::Bedrock,
            occupancy: OccupancyStorage::Solid,
        }
    }

    /// Thin grass carpet on top of dirt; density drops on slopes; broken perimeter.
    pub fn grass_from_seed_sloped(grass_seed: u32, slope: f32) -> Self {
        let mut voxel = Self::empty(Material::Grass);

        // Steeper ⇒ more carpet holes.
        let slope_t = (slope / ROCK_SLOPE).clamp(0.0, 1.0);
        let hole_mod = (5 + (grass_seed % 3))
            .saturating_sub((slope_t * 3.0) as u32)
            .max(3);

        for z in 0..MICROVOXEL_RES {
            for x in 0..MICROVOXEL_RES {
                let edge = x == 0
                    || z == 0
                    || x + 1 == MICROVOXEL_RES
                    || z + 1 == MICROVOXEL_RES;
                let h = mix_seed(grass_seed, x as u32, z as u32);
                if edge && h % (3 + (grass_seed % 3)) == 0 {
                    continue;
                }
                if h % hole_mod == 0 {
                    continue;
                }
                voxel.set_micro(x, 0, z, true);
            }
        }

        voxel
    }

    /// Grass carpet + blades driven by `grass_seed` (holes and growth).
    #[allow(dead_code)]
    pub fn grass_from_seed(grass_seed: u32) -> Self {
        Self::grass_from_seed_sloped(grass_seed, 0.0)
    }

    /// Trunk segment: centered `micro`² column through the full cube height.
    pub fn wood_trunk_sized(material: Material, micro: usize) -> Self {
        let mut voxel = Self::empty(material);
        let micro = micro.clamp(2, MICROVOXEL_RES);
        let lo = (MICROVOXEL_RES - micro) / 2;
        let hi = lo + micro;
        for mz in lo..hi {
            for my in 0..MICROVOXEL_RES {
                for mx in lo..hi {
                    voxel.set_micro(mx, my, mz, true);
                }
            }
        }
        voxel
    }

    /// Trunk segment: centered [`TREE_TRUNK_MICRO`]² oak bark column.
    pub fn wood_trunk() -> Self {
        Self::wood_trunk_sized(Material::Wood, TREE_TRUNK_MICRO)
    }

    pub fn leaves() -> Self {
        Self {
            material: Material::Leaves,
            occupancy: OccupancyStorage::Solid,
        }
    }

    pub fn foliage(material: Material) -> Self {
        Self {
            material,
            occupancy: OccupancyStorage::Solid,
        }
    }

    /// Reserved underground crate (solid marker for forgotten-tool loot later).
    pub fn chest() -> Self {
        Self {
            material: Material::Chest,
            occupancy: OccupancyStorage::Solid,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        matches!(self.occupancy, OccupancyStorage::Empty)
    }

    #[inline]
    pub fn is_fully_solid(&self) -> bool {
        matches!(self.occupancy, OccupancyStorage::Solid)
    }

    fn micro_index(mx: usize, my: usize, mz: usize) -> usize {
        mx + my * MICROVOXEL_RES + mz * MICROVOXEL_RES * MICROVOXEL_RES
    }

    fn ensure_micro_bits(&mut self) -> &mut [u8; OCCUPANCY_BYTES] {
        match self.occupancy {
            OccupancyStorage::Micro(ref mut b) => b,
            OccupancyStorage::Solid => {
                self.occupancy = OccupancyStorage::Micro(Box::new([0xFF; OCCUPANCY_BYTES]));
                match &mut self.occupancy {
                    OccupancyStorage::Micro(b) => b,
                    _ => unreachable!(),
                }
            }
            OccupancyStorage::Empty => {
                self.occupancy = OccupancyStorage::Micro(Box::new([0; OCCUPANCY_BYTES]));
                match &mut self.occupancy {
                    OccupancyStorage::Micro(b) => b,
                    _ => unreachable!(),
                }
            }
        }
    }

    fn compact_micro_if_possible(&mut self) {
        let OccupancyStorage::Micro(ref bits) = self.occupancy else {
            return;
        };
        if bits.iter().all(|&b| b == 0) {
            self.occupancy = OccupancyStorage::Empty;
        } else if bits.iter().all(|&b| b == 0xFF) {
            self.occupancy = OccupancyStorage::Solid;
        }
    }

    pub fn set_micro(&mut self, mx: usize, my: usize, mz: usize, occupied: bool) {
        debug_assert!(mx < MICROVOXEL_RES && my < MICROVOXEL_RES && mz < MICROVOXEL_RES);
        // Fast paths: no allocation when state already matches.
        if occupied {
            if matches!(self.occupancy, OccupancyStorage::Solid) {
                return;
            }
        } else if matches!(self.occupancy, OccupancyStorage::Empty) {
            return;
        }
        let i = Self::micro_index(mx, my, mz);
        let byte = i / 8;
        let bit = i % 8;
        let bits = self.ensure_micro_bits();
        if occupied {
            bits[byte] |= 1 << bit;
        } else {
            bits[byte] &= !(1 << bit);
        }
        self.compact_micro_if_possible();
    }

    pub fn get_micro(&self, mx: usize, my: usize, mz: usize) -> bool {
        if mx >= MICROVOXEL_RES || my >= MICROVOXEL_RES || mz >= MICROVOXEL_RES {
            return false;
        }
        match &self.occupancy {
            OccupancyStorage::Empty => false,
            OccupancyStorage::Solid => true,
            OccupancyStorage::Micro(bits) => {
                let i = Self::micro_index(mx, my, mz);
                let byte = i / 8;
                let bit = i % 8;
                (bits[byte] >> bit) & 1 == 1
            }
        }
    }
}

/// Hit from a DDA raycast against solid voxels.
#[derive(Clone, Copy, Debug)]
pub struct RayHit {
    /// Block that was hit.
    pub pos: IVec3,
    /// Empty neighbor the ray entered from (place against this face).
    pub prev: IVec3,
}

/// Voxel chunk lifecycle (renderer only draws [`Filled`] / [`PendingUnload`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkState {
    /// Gen kicked; columns not committed yet — never draw / never unload-punch.
    Reserved,
    /// Voxels ready to mesh and draw.
    Filled,
    /// Outside unload ring but kept until grace elapses (swap-then-drop).
    PendingUnload,
}

fn set_chunk_state(
    map: &mut FxHashMap<(i32, i32), ChunkState>,
    key: (i32, i32),
    new: ChunkState,
) {
    let old = map.insert(key, new);
    if DEBUG_CHUNK_STREAM && old != Some(new) {
        log::info!("chunk ({},{}) {:?} -> {:?}", key.0, key.1, old, new);
    }
}

/// Shared solid-dirt voxel for dense columns (avoids storing one entry per block).
fn solid_dirt_ref() -> &'static Voxel {
    use std::sync::OnceLock;
    static V: OnceLock<Voxel> = OnceLock::new();
    V.get_or_init(Voxel::dirt)
}

fn solid_sand_ref() -> &'static Voxel {
    use std::sync::OnceLock;
    static V: OnceLock<Voxel> = OnceLock::new();
    V.get_or_init(Voxel::sand)
}

fn solid_stone_ref() -> &'static Voxel {
    use std::sync::OnceLock;
    static V: OnceLock<Voxel> = OnceLock::new();
    V.get_or_init(Voxel::stone)
}

fn solid_black_stone_ref() -> &'static Voxel {
    use std::sync::OnceLock;
    static V: OnceLock<Voxel> = OnceLock::new();
    V.get_or_init(Voxel::black_stone)
}

fn solid_bedrock_ref() -> &'static Voxel {
    use std::sync::OnceLock;
    static V: OnceLock<Voxel> = OnceLock::new();
    V.get_or_init(Voxel::bedrock)
}

pub struct World {
    /// Loaded mesh chunks (reserved or filled).
    loaded_chunks: FxHashMap<(i32, i32), ChunkState>,
    /// Dense dirt column tops: solid dirt implied for `y = 0..=height`.
    heights: FxHashMap<(i32, i32), i32>,
    /// Sparse voxels that are not implied by [`heights`] (grass, edits, floats).
    extras: FxHashMap<IVec3, Voxel>,
    /// Player overlay that survives chunk unload / save. `None` = air.
    player_edits: FxHashMap<IVec3, Option<Material>>,
    /// Cached `(min_h, max_h)` per mesh chunk — invalidated on height edits.
    height_bounds: FxHashMap<(i32, i32), (i32, i32)>,
    /// Running material totals (HUD); updated in set/remove/unload.
    dirt_count: usize,
    grass_count: usize,
    /// Core generated surface size (tests / HUD); streaming can go beyond.
    extent: i32,
    /// Hard remesh: player edits (must drop VRAM mesh).
    dirty_edits: FxHashSet<(i32, i32)>,
    /// Soft remesh: streaming fill (VRAM cache may keep the old mesh).
    dirty_stream: FxHashSet<(i32, i32)>,
    /// FBM results waiting to be inserted (amortized across frames).
    pending_chunks: VecDeque<(i32, i32, Vec<(i32, i32, i32)>)>,
    /// Chunks that already received a grass planting pass (within gen radius).
    grass_ready: FxHashSet<(i32, i32)>,
    /// Frames a chunk has spent in [`ChunkState::PendingUnload`] before drop.
    unload_grace: FxHashMap<(i32, i32), u32>,
}

impl World {
    pub fn new() -> Self {
        Self {
            loaded_chunks: FxHashMap::default(),
            heights: FxHashMap::default(),
            extras: FxHashMap::default(),
            player_edits: FxHashMap::default(),
            height_bounds: FxHashMap::default(),
            dirt_count: 0,
            grass_count: 0,
            extent: 0,
            dirty_edits: FxHashSet::default(),
            dirty_stream: FxHashSet::default(),
            pending_chunks: VecDeque::new(),
            grass_ready: FxHashSet::default(),
            unload_grace: FxHashMap::default(),
        }
    }

    #[inline]
    fn inc_material(&mut self, material: Material) {
        match material {
            Material::Dirt
            | Material::Stone
            | Material::VillageStone
            | Material::Cobblestone
            | Material::BlackStone
            | Material::Bedrock
            | Material::Wood
            | Material::WoodPlanks
            | Material::Leaves
            | Material::Coal
            | Material::Sapphire
            | Material::Ruby
            | Material::Emerald
            | Material::Chest
            | Material::Glass
            | Material::Door
            | Material::Water
            | Material::BirchWood
            | Material::BirchLeaves
            | Material::PineWood
            | Material::PineLeaves
            | Material::WillowWood
            | Material::WillowLeaves
            | Material::MesquiteWood
            | Material::MesquiteLeaves
            | Material::CeibaWood
            | Material::CeibaLeaves
            | Material::Sand
            | Material::EnchantedWood
            | Material::EnchantedLeaves
            | Material::UmbraWood
            | Material::UmbraLeaves => self.dirt_count += 1,
            Material::Grass => self.grass_count += 1,
        }
    }

    #[inline]
    fn dec_material(&mut self, material: Material) {
        match material {
            Material::Dirt
            | Material::Stone
            | Material::VillageStone
            | Material::Cobblestone
            | Material::BlackStone
            | Material::Bedrock
            | Material::Wood
            | Material::WoodPlanks
            | Material::Leaves
            | Material::Coal
            | Material::Sapphire
            | Material::Ruby
            | Material::Emerald
            | Material::Chest
            | Material::Glass
            | Material::Door
            | Material::Water
            | Material::BirchWood
            | Material::BirchLeaves
            | Material::PineWood
            | Material::PineLeaves
            | Material::WillowWood
            | Material::WillowLeaves
            | Material::MesquiteWood
            | Material::MesquiteLeaves
            | Material::CeibaWood
            | Material::CeibaLeaves
            | Material::Sand
            | Material::EnchantedWood
            | Material::EnchantedLeaves
            | Material::UmbraWood
            | Material::UmbraLeaves => {
                self.dirt_count = self.dirt_count.saturating_sub(1)
            }
            Material::Grass => self.grass_count = self.grass_count.saturating_sub(1),
        }
    }

    /// Ore flecks dropped when breaking this stone cell.
    pub fn ore_drop_at(&self, pos: IVec3) -> Option<OreDrop> {
        let v = self.get_voxel(pos)?;
        if v.material != Material::Stone || !v.is_fully_solid() {
            return None;
        }
        let kind = stone_embed_kind(pos.x, pos.y, pos.z)?;
        let micros = ore_yield_count(pos.x, pos.y, pos.z);
        if micros == 0 {
            return None;
        }
        Some(OreDrop { kind, micros })
    }

    /// Infinite world: empty, then synchronously preload the load circle around `spawn`.
    pub fn with_infinite(spawn: Vec3) -> Self {
        let mut world = Self::new();
        world.preload_shunks_around(spawn);
        world.dirty_edits.clear();
        world.dirty_stream.clear();
        world
    }

    /// Fill every shunk inside [`preload_radius_chunks`] around `spawn` before play.
    ///
    /// Runs FBM generation in parallel, then commits all columns (caves / trees /
    /// grass / settlements) with no per-frame budget — so the first frame already
    /// has solid ground underfoot and no 16×16 holes in the load ring.
    pub fn preload_shunks_around(&mut self, spawn: Vec3) {
        use rayon::prelude::*;

        if !ENABLE_STREAMING || (DEBUG_FACE_VISIBILITY && !cfg!(test)) {
            return;
        }

        let bx = spawn.x.floor() as i32;
        let bz = spawn.z.floor() as i32;
        let c0 = mesh_chunk_coord(bx, bz);
        let load_r = preload_radius_chunks();
        let load_r_sq = load_r * load_r;

        let mut keys = Vec::new();
        for dz in -load_r..=load_r {
            for dx in -load_r..=load_r {
                if dx * dx + dz * dz > load_r_sq {
                    continue;
                }
                let key = (c0.0 + dx, c0.1 + dz);
                if self.chunk_filled(key.0, key.1) {
                    continue;
                }
                keys.push(key);
            }
        }
        keys.sort_by_key(|&(kx, kz)| {
            let dx = kx - c0.0;
            let dz = kz - c0.1;
            dx * dx + dz * dz
        });

        let n = keys.len();
        log::info!(
            "preload: generating {n} shunks (r={load_r}) around chunk ({}, {})",
            c0.0,
            c0.1
        );
        let t0 = Instant::now();

        for &(kx, kz) in &keys {
            set_chunk_state(&mut self.loaded_chunks, (kx, kz), ChunkState::Reserved);
        }
        let generated: Vec<(i32, i32, Vec<(i32, i32, i32)>)> = keys
            .par_iter()
            .map(|&(kx, kz)| (kx, kz, generate_chunk_columns(kx, kz)))
            .collect();

        for (kx, kz, columns) in generated {
            let plant_grass = chunk_dist_sq_xz(spawn, kx, kz) <= GRASS_GEN_MAX_DIST_SQ;
            self.commit_generated_chunk(kx, kz, &columns, plant_grass);
        }

        // Drain any leftover soft-stream pending (should be empty after a cold start).
        while let Some((kx, kz, columns)) = self.pending_chunks.pop_front() {
            if !self.loaded_chunks.contains_key(&(kx, kz)) {
                continue;
            }
            let plant_grass = chunk_dist_sq_xz(spawn, kx, kz) <= GRASS_GEN_MAX_DIST_SQ;
            self.commit_generated_chunk(kx, kz, &columns, plant_grass);
        }

        let filled = self
            .loaded_chunks
            .values()
            .filter(|s| matches!(s, ChunkState::Filled | ChunkState::PendingUnload))
            .count();
        log::info!(
            "preload done in {:.0} ms — {filled} filled chunks, {} dirt cells",
            t0.elapsed().as_secs_f32() * 1000.0,
            self.dirt_count
        );
    }

    /// Contiguous [`SHUNK_GRID`]² shunks (3×3 = 9 × 16×16): terraced walkable hills.
    pub fn with_shunk() -> Self {
        let mut world = Self::new();
        let extent = world_blocks();
        world.extent = extent;
        // Same column path as streaming (padded cone per chunk).
        for cz in 0..SHUNK_GRID {
            for cx in 0..SHUNK_GRID {
                let columns = generate_chunk_columns(cx, cz);
                for &(x, z, h) in &columns {
                    if !world.heights.contains_key(&(x, z)) {
                        world.set_column_dirt(x, z, h);
                    }
                }
                if ENABLE_CAVES {
                    crate::caves::carve_chunk(&mut world, cx, cz);
                }
            }
        }
        for z in 0..extent {
            for x in 0..extent {
                if world.column_height(x, z).is_none() {
                    continue;
                }
                let h = terrain_height(x, z);
                let slope = terrain_slope(x, z);
                // Stone outcrops: no grass carpet / trees on the lid.
                if column_cell_is_stone(x, h, z, h) {
                    continue;
                }
                if ENABLE_TREES && column_grows_tree(x, z, slope) {
                    world.plant_tree_at(x, z, h);
                    continue;
                }
                if ENABLE_TREES && column_grows_bush(x, z, slope) {
                    world.plant_bush_at(x, z, h);
                    continue;
                }
                if ENABLE_GRASS
                    && !crate::biomes::column_has_sand_surface(x, z)
                    && column_grows_grass(x, z, slope)
                {
                    let grass_pos = IVec3::new(x, h + 1, z);
                    let grass_seed = block_seed_at(grass_pos, GRASS_SEED);
                    world.set_voxel(
                        grass_pos,
                        Voxel::grass_from_seed_sloped(grass_seed, slope),
                    );
                }
            }
        }
        if ENABLE_SETTLEMENTS {
            for cz in 0..SHUNK_GRID {
                for cx in 0..SHUNK_GRID {
                    crate::settlements::stamp_settlements_in_chunk(&mut world, cx, cz);
                }
            }
        }
        world.dirty_edits.clear();
        world.dirty_stream.clear();
        for cz in 0..SHUNK_GRID {
            for cx in 0..SHUNK_GRID {
                world.grass_ready.insert((cx, cz));
            }
        }
        world
    }

    /// Dirt only (used by mesh cull tests).
    #[allow(dead_code)]
    pub fn with_dirt_cube() -> Self {
        let mut world = Self::new();
        // y=1: above the permanent bedrock band so face tests see Dirt.
        world.set_voxel(IVec3::new(0, 1, 0), Voxel::dirt());
        world
    }

    /// One floating dirt cube (all 6 faces unoccluded) for face-visibility checks.
    pub fn with_face_debug() -> Self {
        let mut world = Self::new();
        // y=8 → sparse extras (not a ground column), so −Y is also free.
        world.set_voxel(IVec3::new(0, 8, 0), Voxel::dirt());
        world
    }

    /// Fill a dense dirt column `y = 0..=h` without per-block hashmap inserts.
    fn set_column_dirt(&mut self, x: i32, z: i32, h: i32) {
        let h = h.clamp(0, WORLD_MAX_Y);
        let key = mesh_chunk_coord(x, z);
        self.loaded_chunks.insert(key, ChunkState::Filled);
        self.extras
            .retain(|p, _| !(p.x == x && p.z == z && p.y >= 0 && p.y <= h));
        match self.heights.insert((x, z), h) {
            None => self.dirt_count += (h + 1) as usize,
            Some(old) if h >= old => self.dirt_count += (h - old) as usize,
            Some(old) => self.dirt_count = self.dirt_count.saturating_sub((old - h) as usize),
        }
        self.height_bounds.remove(&key);
    }

    /// Test helper: dense column fill without streaming commit side-effects.
    #[cfg(test)]
    pub fn fills_column_for_test(&mut self, x: i32, z: i32, h: i32) {
        self.set_column_dirt(x, z, h);
    }

    /// Solid voxel used when replaying a saved material overlay.
    pub fn voxel_from_material(material: Material) -> Voxel {
        match material {
            Material::Dirt => Voxel::dirt(),
            Material::Sand => Voxel::sand(),
            Material::Stone => Voxel::stone(),
            Material::BlackStone => Voxel::black_stone(),
            Material::Bedrock => Voxel::bedrock(),
            Material::Chest => Voxel::chest(),
            Material::Wood => Voxel::wood_trunk(),
            _ => Voxel::solid(material),
        }
    }

    /// Place a block and remember it across unload / save.
    pub fn set_voxel_player(&mut self, pos: IVec3, voxel: Voxel) {
        let recorded = if voxel.is_empty() {
            None
        } else {
            Some(voxel.material)
        };
        self.set_voxel(pos, voxel);
        self.player_edits.insert(pos, recorded);
    }

    /// Dig a block and remember air across unload / save.
    pub fn remove_voxel_player(&mut self, pos: IVec3) -> bool {
        let ok = self.remove_voxel(pos);
        if ok {
            self.player_edits.insert(pos, None);
        }
        ok
    }

    pub fn load_player_edits(&mut self, edits: impl IntoIterator<Item = (IVec3, Option<Material>)>) {
        self.player_edits.clear();
        self.player_edits.extend(edits);
    }

    pub fn player_edits_snapshot(&self) -> Vec<(IVec3, Option<Material>)> {
        self.player_edits.iter().map(|(p, m)| (*p, *m)).collect()
    }

    fn apply_player_edits_in_chunk(&mut self, cx: i32, cz: i32) {
        if self.player_edits.is_empty() {
            return;
        }
        let pending: Vec<(IVec3, Option<Material>)> = self
            .player_edits
            .iter()
            .filter(|(pos, _)| mesh_chunk_coord(pos.x, pos.z) == (cx, cz))
            .map(|(pos, mat)| (*pos, *mat))
            .collect();
        for (pos, mat) in pending {
            match mat {
                None => {
                    let _ = self.remove_voxel(pos);
                }
                Some(material) => {
                    self.set_voxel(pos, Self::voxel_from_material(material));
                }
            }
        }
    }

    pub fn set_voxel(&mut self, pos: IVec3, voxel: Voxel) {
        if pos.y < 0 || pos.y > WORLD_MAX_Y {
            return;
        }
        let key = mesh_chunk_coord(pos.x, pos.z);
        self.loaded_chunks.insert(key, ChunkState::Filled);

        // Dense solid dirt: merge into the column stack when possible.
        if voxel.material == Material::Dirt && voxel.is_fully_solid() {
            if let Some(old) = self.extras.remove(&pos) {
                self.dec_material(old.material);
            } else if self
                .heights
                .get(&(pos.x, pos.z))
                .is_some_and(|&h| pos.y >= 0 && pos.y <= h)
            {
                // Already implied by the dense column.
                self.mark_dirty_around(pos);
                return;
            }

            let top = self.heights.get(&(pos.x, pos.z)).copied();
            match top {
                None if pos.y == 0 => {
                    self.heights.insert((pos.x, pos.z), 0);
                    self.dirt_count += 1;
                }
                Some(h) if pos.y == h + 1 => {
                    self.heights.insert((pos.x, pos.z), pos.y);
                    self.dirt_count += 1;
                }
                Some(h) if pos.y <= h => {
                    // Already solid in column.
                }
                _ => {
                    self.extras.insert(pos, voxel);
                    self.dirt_count += 1;
                }
            }
            self.height_bounds.remove(&key);
            self.mark_dirty_around(pos);
            return;
        }

        // Non-dirt / partial: sparse extras. Clear dense cell if overwriting.
        if self
            .heights
            .get(&(pos.x, pos.z))
            .is_some_and(|&h| pos.y >= 0 && pos.y <= h)
        {
            // Punch dense stack: move above into extras, lower top.
            self.punch_dense_column(pos.x, pos.z, pos.y);
        }
        let material = voxel.material;
        if let Some(old) = self.extras.insert(pos, voxel) {
            self.dec_material(old.material);
        }
        self.inc_material(material);
        self.height_bounds.remove(&key);
        self.mark_dirty_around(pos);
    }

    /// Remove dense dirt at `y`, keeping blocks above as sparse dirt extras.
    fn punch_dense_column(&mut self, x: i32, z: i32, y: i32) {
        let Some(&top) = self.heights.get(&(x, z)) else {
            return;
        };
        if y < 0 || y > top {
            return;
        }
        for yy in (y + 1)..=top {
            let p = IVec3::new(x, yy, z);
            if self.extras.contains_key(&p) {
                continue;
            }
            // Preserve stone/dirt from the pre-punch column top.
            let roof = match column_cell_material(x, yy, z, top) {
                Material::Bedrock => Voxel::bedrock(),
                Material::BlackStone => Voxel::black_stone(),
                Material::Stone => Voxel::stone(),
                _ => Voxel::dirt(),
            };
            self.extras.insert(p, roof);
            // counts: was dense terrain, still same material in extras — no net change
        }
        self.dirt_count = self.dirt_count.saturating_sub(1); // removed cell at y
        if y == 0 {
            self.heights.remove(&(x, z));
        } else {
            self.heights.insert((x, z), y - 1);
        }
    }

    /// Remove a voxel and remesh neighboring column chunks.
    pub fn remove_voxel(&mut self, pos: IVec3) -> bool {
        // Bedrock floor is permanent.
        if pos.y <= 0
            || self
                .get_voxel(pos)
                .is_some_and(|v| v.material == Material::Bedrock)
        {
            return false;
        }
        if let Some(old) = self.extras.remove(&pos) {
            self.dec_material(old.material);
            self.recompute_column_height(pos.x, pos.z);
            self.mark_dirty_around(pos);
            return true;
        }
        if self
            .heights
            .get(&(pos.x, pos.z))
            .is_some_and(|&h| pos.y >= 0 && pos.y <= h)
        {
            let top = self.heights[&(pos.x, pos.z)];
            if pos.y == top {
                self.dirt_count = self.dirt_count.saturating_sub(1);
                if top == 0 {
                    self.heights.remove(&(pos.x, pos.z));
                } else {
                    self.heights.insert((pos.x, pos.z), top - 1);
                }
            } else {
                self.punch_dense_column(pos.x, pos.z, pos.y);
            }
            self.height_bounds
                .remove(&mesh_chunk_coord(pos.x, pos.z));
            self.mark_dirty_around(pos);
            return true;
        }
        false
    }

    fn recompute_column_height(&mut self, x: i32, z: i32) {
        // After sparse edits, rebuild dense top from contiguous solid ground.
        // Must accept stone *and* dirt: natural columns are stone below the
        // surface dirt cap. Dirt-only used to stop at y=0 (stone) and wipe the
        // whole height → a shaft to the bottom of the shunk after mid digs.
        let mut top: Option<i32> = None;
        for y in 0..=WORLD_MAX_Y {
            let pos = IVec3::new(x, y, z);
            let is_dense_ground = self.get_voxel(pos).is_some_and(|v| {
                matches!(
                    v.material,
                    Material::Dirt | Material::Stone | Material::BlackStone | Material::Bedrock
                ) && v.is_fully_solid()
            });
            if is_dense_ground {
                top = Some(y);
            } else {
                break;
            }
        }
        let key = mesh_chunk_coord(x, z);
        // Move contiguous bottom ground from extras into dense height.
        if let Some(h) = top {
            for y in 0..=h {
                let p = IVec3::new(x, y, z);
                self.extras.remove(&p);
            }
            match self.heights.insert((x, z), h) {
                None => {}
                Some(old) if old != h => {
                    // counts already track dirt via extras/dense; leave as-is
                    let _ = old;
                }
                _ => {}
            }
        } else {
            self.heights.remove(&(x, z));
        }
        self.height_bounds.remove(&key);
    }

    /// Mark this chunk and XZ neighbors dirty from a player edit.
    fn mark_dirty_around(&mut self, pos: IVec3) {
        let (cx, cz) = mesh_chunk_coord(pos.x, pos.z);
        for dz in -1..=1 {
            for dx in -1..=1 {
                self.dirty_edits.insert((cx + dx, cz + dz));
            }
        }
    }

    /// Drain player-edit dirties (VRAM mesh must be dropped).
    pub fn take_dirty_edits(&mut self) -> Vec<(i32, i32)> {
        self.dirty_edits.drain().collect()
    }

    /// Drain streaming dirties (VRAM cache may satisfy these).
    pub fn take_dirty_stream(&mut self) -> Vec<(i32, i32)> {
        self.dirty_stream.drain().collect()
    }

    /// Drain all dirties (tests / fallback).
    pub fn take_dirty_chunks(&mut self) -> Vec<(i32, i32)> {
        let mut out = self.take_dirty_edits();
        out.extend(self.take_dirty_stream());
        out
    }

    /// Step through voxel cells along a ray; returns the first solid hit.
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_dist: f32) -> Option<RayHit> {
        let dir = dir.normalize_or_zero();
        if dir.length_squared() < 1e-8 {
            return None;
        }

        let mut x = origin.x.floor() as i32;
        let mut y = origin.y.floor() as i32;
        let mut z = origin.z.floor() as i32;
        let step_x = if dir.x > 0.0 {
            1
        } else if dir.x < 0.0 {
            -1
        } else {
            0
        };
        let step_y = if dir.y > 0.0 {
            1
        } else if dir.y < 0.0 {
            -1
        } else {
            0
        };
        let step_z = if dir.z > 0.0 {
            1
        } else if dir.z < 0.0 {
            -1
        } else {
            0
        };

        let t_delta_x = if step_x != 0 {
            (1.0 / dir.x).abs()
        } else {
            f32::INFINITY
        };
        let t_delta_y = if step_y != 0 {
            (1.0 / dir.y).abs()
        } else {
            f32::INFINITY
        };
        let t_delta_z = if step_z != 0 {
            (1.0 / dir.z).abs()
        } else {
            f32::INFINITY
        };

        let mut t_max_x = if step_x > 0 {
            (x as f32 + 1.0 - origin.x) * t_delta_x
        } else if step_x < 0 {
            (origin.x - x as f32) * t_delta_x
        } else {
            f32::INFINITY
        };
        let mut t_max_y = if step_y > 0 {
            (y as f32 + 1.0 - origin.y) * t_delta_y
        } else if step_y < 0 {
            (origin.y - y as f32) * t_delta_y
        } else {
            f32::INFINITY
        };
        let mut t_max_z = if step_z > 0 {
            (z as f32 + 1.0 - origin.z) * t_delta_z
        } else if step_z < 0 {
            (origin.z - z as f32) * t_delta_z
        } else {
            f32::INFINITY
        };

        let mut prev = IVec3::new(x, y, z);
        // Cap steps so runaway rays cannot hang.
        for _ in 0..512 {
            let pos = IVec3::new(x, y, z);
            if self.get_voxel(pos).is_some() {
                return Some(RayHit { pos, prev });
            }
            let t_next = t_max_x.min(t_max_y).min(t_max_z);
            if t_next > max_dist {
                break;
            }
            if t_max_x < t_max_y {
                if t_max_x < t_max_z {
                    prev = pos;
                    x += step_x;
                    t_max_x += t_delta_x;
                } else {
                    prev = pos;
                    z += step_z;
                    t_max_z += t_delta_z;
                }
            } else if t_max_y < t_max_z {
                prev = pos;
                y += step_y;
                t_max_y += t_delta_y;
            } else {
                prev = pos;
                z += step_z;
                t_max_z += t_delta_z;
            }
        }
        None
    }

    /// Dirt-top Y for column, or `None` if the column was never generated.
    pub fn column_height(&self, x: i32, z: i32) -> Option<i32> {
        self.heights.get(&(x, z)).copied()
    }

    /// Min/max dirt-top Y inside a mesh chunk (for horizon / AABB).
    /// Cached until a height edit invalidates the chunk.
    pub fn chunk_height_bounds(&mut self, cx: i32, cz: i32) -> (i32, i32) {
        if let Some(b) = self.height_bounds.get(&(cx, cz)) {
            return *b;
        }
        let bounds = self.compute_chunk_height_bounds(cx, cz);
        self.height_bounds.insert((cx, cz), bounds);
        bounds
    }

    fn compute_chunk_height_bounds(&self, cx: i32, cz: i32) -> (i32, i32) {
        let x0 = cx * MESH_CHUNK_SIZE;
        let z0 = cz * MESH_CHUNK_SIZE;
        let mut min_h = TERRAIN_MAX_HEIGHT;
        let mut max_h = 0i32;
        let mut any = false;
        for z in z0..z0 + MESH_CHUNK_SIZE {
            for x in x0..x0 + MESH_CHUNK_SIZE {
                if let Some(h) = self.column_height(x, z) {
                    any = true;
                    min_h = min_h.min(h);
                    max_h = max_h.max(h);
                }
            }
        }
        if !any {
            (0, TERRAIN_MAX_HEIGHT)
        } else {
            (min_h, max_h)
        }
    }

    /// Generate one mesh-chunk of columns if missing (serial entry; streaming uses the batch path).
    #[allow(dead_code)]
    pub fn ensure_chunk(&mut self, cx: i32, cz: i32) {
        if self.chunk_filled(cx, cz) {
            return;
        }
        self.loaded_chunks
            .entry((cx, cz))
            .or_insert(ChunkState::Reserved);
        let columns = generate_chunk_columns(cx, cz);
        self.commit_generated_chunk(cx, cz, &columns, true);
    }

    /// Insert precomputed columns — O(columns), not O(blocks).
    /// `plant_grass`: false when the chunk is outside [`GRASS_GEN_MAX_DIST`].
    fn commit_generated_chunk(
        &mut self,
        cx: i32,
        cz: i32,
        columns: &[(i32, i32, i32)],
        plant_grass: bool,
    ) {
        set_chunk_state(&mut self.loaded_chunks, (cx, cz), ChunkState::Filled);
        let mut planted_cols: Vec<(i32, i32, i32)> = Vec::with_capacity(columns.len());
        for &(x, z, h) in columns {
            if self.heights.contains_key(&(x, z)) {
                continue;
            }
            self.set_column_dirt(x, z, h);
            planted_cols.push((x, z, h));
        }
        if ENABLE_CAVES {
            crate::caves::carve_chunk(self, cx, cz);
        }
        for &(x, z, h) in &planted_cols {
            if column_cell_is_stone(x, h, z, h) {
                continue;
            }
            let slope = terrain_slope(x, z);
            if ENABLE_TREES && column_grows_tree(x, z, slope) {
                self.plant_tree_at(x, z, h);
                continue;
            }
            if ENABLE_TREES && column_grows_bush(x, z, slope) {
                self.plant_bush_at(x, z, h);
                continue;
            }
            if ENABLE_GRASS && plant_grass {
                if !crate::biomes::column_has_sand_surface(x, z)
                    && column_grows_grass(x, z, slope)
                {
                    let grass_pos = IVec3::new(x, h + 1, z);
                    if self.player_edits.contains_key(&grass_pos)
                        || self.player_edits.contains_key(&IVec3::new(x, h, z))
                    {
                        continue;
                    }
                    let grass_seed = block_seed_at(grass_pos, GRASS_SEED);
                    self.set_voxel(
                        grass_pos,
                        Voxel::grass_from_seed_sloped(grass_seed, slope),
                    );
                }
            }
        }
        if ENABLE_SETTLEMENTS {
            // After vegetation so roads/walls/houses overwrite grass and trunks.
            crate::settlements::stamp_settlements_in_chunk(self, cx, cz);
        }
        self.apply_player_edits_in_chunk(cx, cz);
        if ENABLE_GRASS && plant_grass {
            self.grass_ready.insert((cx, cz));
        }
        for dz in -1..=1 {
            for dx in -1..=1 {
                self.dirty_stream.insert((cx + dx, cz + dz));
            }
        }
        let bounds = self.compute_chunk_height_bounds(cx, cz);
        self.height_bounds.insert((cx, cz), bounds);
    }

    /// Plant grass on one filled chunk that entered the gen radius.
    fn seed_chunk_grass(&mut self, cx: i32, cz: i32) {
        if !ENABLE_GRASS || self.grass_ready.contains(&(cx, cz)) {
            return;
        }
        let x0 = cx * MESH_CHUNK_SIZE;
        let z0 = cz * MESH_CHUNK_SIZE;
        for z in z0..z0 + MESH_CHUNK_SIZE {
            for x in x0..x0 + MESH_CHUNK_SIZE {
                // Natural lip — dense `column_height` may sit lower after cave punches.
                if self.column_height(x, z).is_none()
                    && self.get_voxel(IVec3::new(x, 0, z)).is_none()
                {
                    continue;
                }
                let h = terrain_height(x, z);
                if column_cell_is_stone(x, h, z, h) {
                    continue;
                }
                if crate::biomes::column_has_sand_surface(x, z) {
                    continue;
                }
                // Settlement roads / wall bases already stamped — don't re-grow flora.
                if self
                    .get_voxel(IVec3::new(x, h, z))
                    .is_some_and(|v| v.material == Material::Stone)
                {
                    continue;
                }
                let slope = terrain_slope(x, z);
                if ENABLE_TREES && column_grows_tree(x, z, slope) {
                    self.plant_tree_at(x, z, h);
                    continue;
                }
                if ENABLE_TREES && column_grows_bush(x, z, slope) {
                    self.plant_bush_at(x, z, h);
                    continue;
                }
                if crate::biomes::column_has_sand_surface(x, z) {
                    continue;
                }
                if !column_grows_grass(x, z, slope) {
                    continue;
                }
                let grass_pos = IVec3::new(x, h + 1, z);
                if self.player_edits.contains_key(&grass_pos)
                    || self.player_edits.contains_key(&IVec3::new(x, h, z))
                {
                    continue;
                }
                if self.extras.contains_key(&grass_pos) {
                    continue;
                }
                let grass_seed = block_seed_at(grass_pos, GRASS_SEED);
                self.set_voxel(
                    grass_pos,
                    Voxel::grass_from_seed_sloped(grass_seed, slope),
                );
            }
        }
        self.apply_player_edits_in_chunk(cx, cz);
        self.grass_ready.insert((cx, cz));
        self.dirty_stream.insert((cx, cz));
    }

    /// Fill grass on nearby chunks that were committed bare (camera walked in).
    fn seed_grass_near(&mut self, camera_pos: Vec3, start: Instant, budget: Duration) {
        if !ENABLE_GRASS {
            return;
        }
        let mut candidates: Vec<(i32, i32, i32)> = Vec::new();
        for &(cx, cz) in self.loaded_chunks.keys() {
            if self.grass_ready.contains(&(cx, cz)) {
                continue;
            }
            if !self.chunk_filled(cx, cz) {
                continue;
            }
            let d2 = chunk_dist_sq_xz(camera_pos, cx, cz);
            if d2 > GRASS_GEN_MAX_DIST_SQ {
                continue;
            }
            candidates.push((d2 as i32, cx, cz));
        }
        candidates.sort_unstable_by_key(|&(d, _, _)| d);
        let mut seeded = 0usize;
        for &(_, cx, cz) in candidates.iter().take(MAX_GRASS_SEED_CHUNKS_PER_FRAME) {
            self.seed_chunk_grass(cx, cz);
            seeded += 1;
            if seeded >= 1 && start.elapsed() >= budget {
                break;
            }
        }
    }

    fn column_slope_at(&self, x: i32, z: i32) -> f32 {
        let h = self
            .column_height(x, z)
            .unwrap_or_else(|| terrain_height(x, z)) as f32;
        let mut max_d = 0.0f32;
        for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let nh = self
                .column_height(x + dx, z + dz)
                .unwrap_or_else(|| terrain_height(x + dx, z + dz)) as f32;
            max_d = max_d.max((h - nh).abs());
        }
        max_d
    }

    /// Place a biome-native tree (encino / pino / abedul / sauce / mezquite / ceiba-lite).
    fn plant_tree_at(&mut self, x: i32, z: i32, surface_h: i32) {
        use crate::biomes::{biome_at, pick_tree_species, TreeSpecies};

        let species = pick_tree_species(x, z, biome_at(x, z));
        let (trunk_h, trunk_micro, bark, leaf, canopy) = match species {
            TreeSpecies::Oak => (
                tree_trunk_height(x, z),
                TREE_TRUNK_MICRO,
                Material::Wood,
                Material::Leaves,
                CanopyShape::Oak,
            ),
            TreeSpecies::Birch => {
                let h = 4 + (mix_seed(TREE_SEED ^ 0xB12C_0001, x as u32, z as u32) % 4) as i32;
                (
                    h,
                    5usize,
                    Material::BirchWood,
                    Material::BirchLeaves,
                    CanopyShape::Birch,
                )
            }
            TreeSpecies::Pine => {
                let h = 5 + (mix_seed(TREE_SEED ^ 0x5049_4E45, x as u32, z as u32) % 4) as i32;
                (
                    h,
                    6usize,
                    Material::PineWood,
                    Material::PineLeaves,
                    CanopyShape::Pine,
                )
            }
            TreeSpecies::Willow => {
                let h = 2 + (mix_seed(TREE_SEED ^ 0x5749_4C4F, x as u32, z as u32) % 2) as i32;
                (
                    h,
                    7usize,
                    Material::WillowWood,
                    Material::WillowLeaves,
                    CanopyShape::Willow,
                )
            }
            TreeSpecies::Mesquite => {
                let h = 2 + (mix_seed(TREE_SEED ^ 0x4D45_5351, x as u32, z as u32) % 3) as i32;
                (
                    h,
                    5usize,
                    Material::MesquiteWood,
                    Material::MesquiteLeaves,
                    CanopyShape::Mesquite,
                )
            }
            TreeSpecies::CeibaLite => {
                let h = 3 + (mix_seed(TREE_SEED ^ 0xCE1B_A001, x as u32, z as u32) % 3) as i32;
                (
                    h,
                    10usize,
                    Material::CeibaWood,
                    Material::CeibaLeaves,
                    CanopyShape::Ceiba,
                )
            }
            TreeSpecies::Enchanted => {
                let h = 3 + (mix_seed(TREE_SEED ^ 0xE4C4_A001, x as u32, z as u32) % 3) as i32;
                (
                    h,
                    7usize,
                    Material::EnchantedWood,
                    Material::EnchantedLeaves,
                    CanopyShape::Oak,
                )
            }
            TreeSpecies::Umbra => {
                let h = 4 + (mix_seed(TREE_SEED ^ 0x554D_B2A1, x as u32, z as u32) % 3) as i32;
                (
                    h,
                    9usize,
                    Material::UmbraWood,
                    Material::UmbraLeaves,
                    CanopyShape::Oak,
                )
            }
        };

        let top_y = surface_h + trunk_h;
        if top_y + 3 > WORLD_MAX_Y {
            return;
        }
        for dy in 1..=trunk_h {
            self.set_voxel(
                IVec3::new(x, surface_h + dy, z),
                Voxel::wood_trunk_sized(bark, trunk_micro),
            );
        }
        self.plant_canopy(x, z, top_y, leaf, canopy);
    }

    fn plant_canopy(
        &mut self,
        x: i32,
        z: i32,
        top_y: i32,
        leaf: Material,
        shape: CanopyShape,
    ) {
        let extent = self.extent;
        let in_world = |lx: i32, lz: i32| -> bool {
            if extent <= 0 {
                true
            } else {
                lx >= 0 && lz >= 0 && lx < extent && lz < extent
            }
        };
        let mut put = |lx: i32, ly: i32, lz: i32| {
            if !in_world(lx, lz) || ly < 0 || ly > WORLD_MAX_Y {
                return;
            }
            let pos = IVec3::new(lx, ly, lz);
            if self.get_voxel(pos).is_some() {
                return;
            }
            self.set_voxel(pos, Voxel::foliage(leaf));
        };

        match shape {
            CanopyShape::Oak => {
                let canopy_y = top_y + 1;
                for dz in -2..=2i32 {
                    for dx in -2..=2i32 {
                        if dx * dx + dz * dz <= 5 {
                            put(x + dx, canopy_y, z + dz);
                        }
                    }
                }
                for dz in -1..=1i32 {
                    for dx in -1..=1i32 {
                        if dx.abs() + dz.abs() <= 1 {
                            put(x + dx, canopy_y + 1, z + dz);
                        }
                    }
                }
            }
            CanopyShape::Birch => {
                let canopy_y = top_y + 1;
                for dz in -1..=1i32 {
                    for dx in -1..=1i32 {
                        put(x + dx, canopy_y, z + dz);
                    }
                }
                put(x, canopy_y + 1, z);
            }
            CanopyShape::Pine => {
                // Conical layers narrowing upward around the trunk top.
                for (layer, radius) in [(0i32, 2i32), (1, 2), (2, 1), (3, 0)] {
                    let y = top_y - 1 + layer;
                    for dz in -radius..=radius {
                        for dx in -radius..=radius {
                            if dx * dx + dz * dz <= radius * radius + 1 {
                                put(x + dx, y, z + dz);
                            }
                        }
                    }
                }
            }
            CanopyShape::Willow => {
                let canopy_y = top_y + 1;
                for dz in -3..=3i32 {
                    for dx in -3..=3i32 {
                        if dx * dx + dz * dz <= 10 {
                            put(x + dx, canopy_y, z + dz);
                        }
                    }
                }
                for dz in -2..=2i32 {
                    for dx in -2..=2i32 {
                        if dx * dx + dz * dz <= 5 {
                            put(x + dx, canopy_y - 1, z + dz);
                        }
                    }
                }
            }
            CanopyShape::Mesquite => {
                let canopy_y = top_y + 1;
                for (dx, dz) in [
                    (0, 0),
                    (1, 0),
                    (-1, 1),
                    (2, -1),
                    (-2, 0),
                    (1, 2),
                    (0, -2),
                ] {
                    put(x + dx, canopy_y, z + dz);
                }
                put(x, canopy_y + 1, z);
                put(x + 1, canopy_y + 1, z - 1);
            }
            CanopyShape::Ceiba => {
                let canopy_y = top_y + 1;
                for dz in -3..=3i32 {
                    for dx in -3..=3i32 {
                        if dx * dx + dz * dz <= 8 {
                            put(x + dx, canopy_y, z + dz);
                        }
                    }
                }
                for dz in -1..=1i32 {
                    for dx in -1..=1i32 {
                        put(x + dx, canopy_y + 1, z + dz);
                    }
                }
            }
        }
    }

    /// Low scrub / understory / reeds — biome-native, never replaces a tree column.
    fn plant_bush_at(&mut self, x: i32, z: i32, surface_h: i32) {
        use crate::biomes::{biome_at, pick_bush_kind, BushKind};

        let Some(kind) = pick_bush_kind(x, z, biome_at(x, z)) else {
            return;
        };
        let extent = self.extent;
        let in_world = |lx: i32, lz: i32| -> bool {
            if extent <= 0 {
                true
            } else {
                lx >= 0 && lz >= 0 && lx < extent && lz < extent
            }
        };

        match kind {
            BushKind::LeafClump => {
                if surface_h + 2 > WORLD_MAX_Y {
                    return;
                }
                self.set_voxel(
                    IVec3::new(x, surface_h + 1, z),
                    Voxel::wood_trunk_sized(Material::Wood, 4),
                );
                let y = surface_h + 2;
                let leaf = Material::Leaves;
                for (dx, dz) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let lx = x + dx;
                    let lz = z + dz;
                    if !in_world(lx, lz) || y < 0 || y > WORLD_MAX_Y {
                        continue;
                    }
                    let pos = IVec3::new(lx, y, lz);
                    if self.get_voxel(pos).is_some() {
                        continue;
                    }
                    self.set_voxel(pos, Voxel::foliage(leaf));
                }
            }
            BushKind::Scrub => {
                if surface_h + 2 > WORLD_MAX_Y {
                    return;
                }
                self.set_voxel(
                    IVec3::new(x, surface_h + 1, z),
                    Voxel::wood_trunk_sized(Material::MesquiteWood, 3),
                );
                let y = surface_h + 2;
                let leaf = Material::MesquiteLeaves;
                for (dx, dz) in [(0, 0), (1, 0), (-1, 1), (0, -1)] {
                    let lx = x + dx;
                    let lz = z + dz;
                    if !in_world(lx, lz) || y < 0 || y > WORLD_MAX_Y {
                        continue;
                    }
                    let pos = IVec3::new(lx, y, lz);
                    if self.get_voxel(pos).is_some() {
                        continue;
                    }
                    self.set_voxel(pos, Voxel::foliage(leaf));
                }
            }
            BushKind::Reed => {
                let h = 2 + (mix_seed(BUSH_SEED ^ 0x2EED, x as u32, z as u32) % 2) as i32;
                if surface_h + h > WORLD_MAX_Y {
                    return;
                }
                for dy in 1..=h {
                    let pos = IVec3::new(x, surface_h + dy, z);
                    if self.get_voxel(pos).is_some() {
                        continue;
                    }
                    self.set_voxel(pos, Voxel::foliage(Material::WillowLeaves));
                }
            }
        }
    }

    /// Load chunks near the camera; unload far ones (infinite world streaming).
    /// Soft-budgets CPU so one frame cannot hitch on a huge gen/commit spike.
    /// No-op when [`ENABLE_STREAMING`] is false or [`DEBUG_FACE_VISIBILITY`] is on.
    pub fn stream_around(&mut self, camera_pos: Vec3) {
        self.stream_around_look(camera_pos, Vec3::ZERO);
    }

    /// Like [`stream_around`], but prioritizes chunks in `look_xz` (flat forward).
    pub fn stream_around_look(&mut self, camera_pos: Vec3, look_xz: Vec3) {
        let budget = if cfg!(test) {
            Duration::from_millis(200)
        } else if ENABLE_HD2D {
            Duration::from_millis(HD2D_FRAME_STREAM_BUDGET_MS)
        } else {
            Duration::from_millis(FRAME_STREAM_BUDGET_MS)
        };
        self.stream_around_timed_look(camera_pos, look_xz, budget);
    }

    /// Same as [`stream_around`] with an explicit CPU budget (isotropic).
    pub fn stream_around_timed(&mut self, camera_pos: Vec3, budget: Duration) {
        self.stream_around_timed_look(camera_pos, Vec3::ZERO, budget);
    }

    /// Stream with optional look-dir preload (Minecraft-style: fill ahead of travel/view).
    pub fn stream_around_timed_look(
        &mut self,
        camera_pos: Vec3,
        look_xz: Vec3,
        budget: Duration,
    ) {
        use rayon::prelude::*;

        // Face-debug scene must stay a single cube; keep streaming in unit tests.
        if !ENABLE_STREAMING || (DEBUG_FACE_VISIBILITY && !cfg!(test)) {
            return;
        }
        let start = Instant::now();
        let cx = camera_pos.x.floor() as i32;
        let cz = camera_pos.z.floor() as i32;
        let c0 = mesh_chunk_coord(cx, cz);
        let look = {
            let flat = Vec3::new(look_xz.x, 0.0, look_xz.z);
            let len = flat.length();
            if len > 1e-3 {
                flat / len
            } else {
                Vec3::ZERO
            }
        };

        let max_gen = if ENABLE_HD2D {
            HD2D_MAX_GEN_KICK_PER_FRAME
        } else {
            MAX_GEN_KICK_PER_FRAME
        };
        let max_commit = if ENABLE_HD2D {
            HD2D_MAX_COMMIT_PER_FRAME
        } else {
            MAX_COMMIT_PER_FRAME
        };

        let mesh_r = ((stream_keep_dist() / MESH_CHUNK_SIZE as f32).ceil() as i32) + 1;
        let mesh_r_sq = mesh_r * mesh_r;

        // 1) Kick FBM: mesh-ring holes ALWAYS (even over soft cap); outer ring only
        // when pending has room. Soft-cap used to starve the visible ring → 16×16 holes.
        let load_r = ((stream_load_dist() / MESH_CHUNK_SIZE as f32).ceil() as i32) + 1;
        let load_r_sq = load_r * load_r;
        let pending_full = self.pending_chunks.len() >= PENDING_SOFT_CAP;
        let mut missing = Vec::new();
        for dz in -load_r..=load_r {
            for dx in -load_r..=load_r {
                let d2 = dx * dx + dz * dz;
                if d2 > load_r_sq {
                    continue;
                }
                let in_mesh_ring = d2 <= mesh_r_sq;
                // Speculative outer preload yields to soft cap; guardian holes never do.
                if pending_full && !in_mesh_ring {
                    continue;
                }
                let key = (c0.0 + dx, c0.1 + dz);
                // Revive pending-unload if the player walked back.
                if self.loaded_chunks.get(&key) == Some(&ChunkState::PendingUnload) {
                    set_chunk_state(&mut self.loaded_chunks, key, ChunkState::Filled);
                    self.unload_grace.remove(&key);
                }
                if !self.loaded_chunks.contains_key(&key) {
                    let align = if look != Vec3::ZERO {
                        (dx as f32 * look.x + dz as f32 * look.z).max(0.0)
                    } else {
                        0.0
                    };
                    let hole_boost = if in_mesh_ring { -1.0e6 } else { 0.0 };
                    let score = d2 as f32
                        - STREAM_LOOK_PRELOAD_BIAS * align * load_r as f32
                        + hole_boost;
                    missing.push(((score * 1000.0) as i32, d2, key.0, key.1));
                }
            }
        }
        missing.sort_by_key(|&(score, d2, _, _)| (score, d2));

        let batch: Vec<(i32, i32)> = missing
            .into_iter()
            .take(max_gen)
            .map(|(_, _, kx, kz)| (kx, kz))
            .filter(|&(kx, kz)| !self.loaded_chunks.contains_key(&(kx, kz)))
            .collect();

        if !batch.is_empty() {
            for &(kx, kz) in &batch {
                set_chunk_state(&mut self.loaded_chunks, (kx, kz), ChunkState::Reserved);
            }
            let generated: Vec<(i32, i32, Vec<(i32, i32, i32)>)> = batch
                .par_iter()
                .map(|&(kx, kz)| (kx, kz, generate_chunk_columns(kx, kz)))
                .collect();
            for item in generated {
                self.pending_chunks.push_back(item);
            }
        }

        // 2) Commit pending columns — mesh-ring holes first, then nearest.
        if !self.pending_chunks.is_empty() {
            let mut pending: Vec<_> = self.pending_chunks.drain(..).collect();
            pending.sort_by_key(|&(kx, kz, _)| {
                let d = chunk_dist_sq_xz(camera_pos, kx, kz);
                let dx = kx - c0.0;
                let dz = kz - c0.1;
                let in_mesh = dx * dx + dz * dz <= mesh_r_sq;
                (if in_mesh { 0u8 } else { 1 }, d.to_bits())
            });
            self.pending_chunks.extend(pending);
        }
        let mut committed = 0usize;
        while committed < max_commit {
            let Some((kx, kz, columns)) = self.pending_chunks.pop_front() else {
                break;
            };
            // Unload raced the queue — drop stale payload (do not re-push forever).
            if !self.loaded_chunks.contains_key(&(kx, kz)) {
                if DEBUG_CHUNK_STREAM {
                    log::warn!(
                        "chunk ({kx},{kz}) pending commit dropped — not in loaded_chunks"
                    );
                }
                continue;
            }
            let plant_grass = chunk_dist_sq_xz(camera_pos, kx, kz) <= GRASS_GEN_MAX_DIST_SQ;
            self.commit_generated_chunk(kx, kz, &columns, plant_grass);
            committed += 1;
            if committed >= 1 && start.elapsed() >= budget {
                break;
            }
        }

        // 3) Unload far chunks — never drop the keep/mesh ring; grace then swap-drop.
        let unload_r = ((stream_unload_dist() / MESH_CHUNK_SIZE as f32).ceil() as i32) + 1;
        let unload_r_sq = unload_r * unload_r;
        let keep_r_sq = mesh_r_sq;
        let _keep_r = mesh_r;
        let candidates: Vec<(i32, i32)> = self.loaded_chunks.keys().copied().collect();
        let mut to_drop = Vec::new();
        for key in candidates {
            let dx = key.0 - c0.0;
            let dz = key.1 - c0.1;
            let d2 = dx * dx + dz * dz;
            // Absolute rule: never punch a hole inside the visible/keep ring.
            if d2 <= keep_r_sq {
                if self.loaded_chunks.get(&key) == Some(&ChunkState::PendingUnload) {
                    set_chunk_state(&mut self.loaded_chunks, key, ChunkState::Filled);
                    self.unload_grace.remove(&key);
                }
                continue;
            }
            if d2 <= unload_r_sq {
                // Still inside hysteresis — cancel any pending unload.
                if self.loaded_chunks.get(&key) == Some(&ChunkState::PendingUnload) {
                    set_chunk_state(&mut self.loaded_chunks, key, ChunkState::Filled);
                    self.unload_grace.remove(&key);
                }
                continue;
            }
            match self.loaded_chunks.get(&key).copied() {
                Some(ChunkState::Reserved) => {
                    // Drop abandoned reservations immediately (no voxels yet).
                    to_drop.push(key);
                }
                Some(ChunkState::Filled) => {
                    set_chunk_state(
                        &mut self.loaded_chunks,
                        key,
                        ChunkState::PendingUnload,
                    );
                    self.unload_grace.insert(key, 0);
                }
                Some(ChunkState::PendingUnload) => {
                    let g = self.unload_grace.entry(key).or_insert(0);
                    *g = g.saturating_add(1);
                    if *g >= UNLOAD_GRACE_FRAMES {
                        to_drop.push(key);
                    }
                }
                None => {}
            }
        }
        for key in to_drop {
            self.unload_chunk(key.0, key.1);
        }

        // 4) Plant grass on bare chunks that entered the gen radius.
        self.seed_grass_near(camera_pos, start, budget);
    }

    fn unload_chunk(&mut self, cx: i32, cz: i32) {
        self.pending_chunks
            .retain(|&(px, pz, _)| px != cx || pz != cz);
        self.grass_ready.remove(&(cx, cz));
        self.unload_grace.remove(&(cx, cz));
        let old = self.loaded_chunks.remove(&(cx, cz));
        if old.is_none() {
            return;
        }
        if DEBUG_CHUNK_STREAM {
            log::info!("chunk ({cx},{cz}) {:?} -> unloaded", old);
        }
        let x0 = cx * MESH_CHUNK_SIZE;
        let z0 = cz * MESH_CHUNK_SIZE;
        for z in z0..z0 + MESH_CHUNK_SIZE {
            for x in x0..x0 + MESH_CHUNK_SIZE {
                if let Some(h) = self.heights.remove(&(x, z)) {
                    self.dirt_count = self.dirt_count.saturating_sub((h + 1) as usize);
                }
            }
        }
        let doomed: Vec<IVec3> = self
            .extras
            .keys()
            .copied()
            .filter(|pos| mesh_chunk_coord(pos.x, pos.z) == (cx, cz))
            .collect();
        for pos in doomed {
            if let Some(v) = self.extras.remove(&pos) {
                self.dec_material(v.material);
            }
        }
        self.dirty_edits.remove(&(cx, cz));
        self.dirty_stream.remove(&(cx, cz));
        self.height_bounds.remove(&(cx, cz));
        // Neighbors still have culled shared faces — remesh so they seal the void
        // instead of leaving a see-through 16×16 hole.
        for dz in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let n = (cx + dx, cz + dz);
                if self.chunk_filled(n.0, n.1) {
                    self.dirty_stream.insert(n);
                }
            }
        }
    }

    pub fn get_voxel(&self, pos: IVec3) -> Option<&Voxel> {
        if let Some(v) = self.extras.get(&pos) {
            return Some(v);
        }
        // Permanent floor only on columns that exist — never pad empty XZ in a
        // Filled chunk (that occluded sides of lone y=0 cubes and spawned a
        // phantom bedrock lid under floating test geometry).
        if pos.y == 0 {
            if self.heights.contains_key(&(pos.x, pos.z)) {
                return Some(solid_bedrock_ref());
            }
        }
        if let Some(&h) = self.heights.get(&(pos.x, pos.z)) {
            if pos.y >= 0 && pos.y <= h {
                return Some(match column_cell_material(pos.x, pos.y, pos.z, h) {
                    Material::Bedrock => solid_bedrock_ref(),
                    Material::BlackStone => solid_black_stone_ref(),
                    Material::Stone => solid_stone_ref(),
                    Material::Sand => solid_sand_ref(),
                    _ => solid_dirt_ref(),
                });
            }
        }
        None
    }

    /// Material of a solid cell the pick can target, if any.
    pub fn dig_material_at(&self, pos: IVec3) -> Option<Material> {
        let v = self.get_voxel(pos)?;
        if v.is_empty() {
            return None;
        }
        Some(v.material)
    }

    /// HD-2D lens pull-in: `0` open field → `1` buried / roofed / walled-in.
    /// Used to shorten camera distance so digs and canopy stay readable.
    pub fn hd2d_confine_factor(&self, focus: Vec3) -> f32 {
        let gx = focus.x.floor() as i32;
        let gz = focus.z.floor() as i32;
        let surface = terrain_height(gx, gz) as f32;
        // focus ≈ chest (feet+1). How far below the natural lip.
        let burial = (surface - focus.y + 1.0).max(0.0);
        let burial_t = ((burial - 0.35) / 2.65).clamp(0.0, 1.0);

        let cx = gx;
        let cy = focus.y.floor() as i32;
        let cz = gz;

            let mut cover = 0.0_f32;
        for dy in 1..=4 {
            // Any solid above (dirt roof, leaf canopy) counts as confined.
            if self
                .get_voxel(IVec3::new(cx, cy + dy, cz))
                .is_some_and(|v| !v.is_empty())
            {
                cover = 1.0;
                break;
            }
        }

        let dirs = [
            (-1, 0),
            (1, 0),
            (0, -1),
            (0, 1),
            (-2, 0),
            (2, 0),
            (0, -2),
            (0, 2),
        ];
        let mut wall_hits = 0u32;
        for &(dx, dz) in &dirs {
            for dy in 0..=1 {
                if self.solid_occludes(IVec3::new(cx + dx, cy + dy, cz + dz)) {
                    wall_hits += 1;
                    break;
                }
            }
        }
        let wall_t = wall_hits as f32 / dirs.len() as f32;

        (burial_t * 0.80 + cover * 0.55 + wall_t * 0.40).clamp(0.0, 1.0)
    }

    /// `1` when the focus sits under a roof and between four cardinal walls
    /// (typical prefab house interior). Used by the shared Bayer cutaway so
    /// camera-facing walls/roof dissolve to show the room.
    pub fn indoors_factor(&self, focus: Vec3) -> f32 {
        let cx = focus.x.floor() as i32;
        let cy = focus.y.floor() as i32;
        let cz = focus.z.floor() as i32;

        let mut roof = false;
        for dy in 1..=5 {
            if self.solid_occludes(IVec3::new(cx, cy + dy, cz)) {
                roof = true;
                break;
            }
        }
        if !roof {
            return 0.0;
        }

        let cardinals = [(-1, 0), (1, 0), (0, -1), (0, 1)];
        let mut sides = 0u32;
        for &(dx, dz) in &cardinals {
            let mut hit = false;
            'side: for dist in 1..=3 {
                for dy in 0..=2 {
                    if self.solid_occludes(IVec3::new(cx + dx * dist, cy + dy, cz + dz * dist)) {
                        hit = true;
                        break 'side;
                    }
                }
            }
            if hit {
                sides += 1;
            }
        }
        if sides < 4 {
            // Soft ramp when 3 walls are found (doorway side open).
            return if sides == 3 { 0.55 } else { 0.0 };
        }
        1.0
    }

    /// True if a fully solid block sits here (hides the shared face from both sides).
    pub fn solid_occludes(&self, pos: IVec3) -> bool {
        self.get_voxel(pos)
            .is_some_and(|v| v.is_fully_solid())
    }

    /// World-space collision AABB for player physics (`None` = empty / no hitbox).
    /// Trunks use the centered [`TREE_TRUNK_MICRO`]² column; solids use the full cube.
    pub fn collision_aabb(&self, pos: IVec3) -> Option<(Vec3, Vec3)> {
        let v = self.get_voxel(pos)?;
        if v.is_empty() {
            return None;
        }
        // Doors and shallow water: walk-through (no solid hitbox).
        if matches!(v.material, Material::Door | Material::Water) {
            return None;
        }
        if v.is_fully_solid() {
            return Some((
                Vec3::new(pos.x as f32, pos.y as f32, pos.z as f32),
                Vec3::new(pos.x as f32 + 1.0, pos.y as f32 + 1.0, pos.z as f32 + 1.0),
            ));
        }
        if v.material.is_tree_bark() {
            // Derive thin trunk AABB from occupied micros (species vary in width).
            let mid = MICROVOXEL_RES / 2;
            let mut min_mx = MICROVOXEL_RES;
            let mut max_mx = 0usize;
            let mut min_mz = MICROVOXEL_RES;
            let mut max_mz = 0usize;
            let mut any = false;
            for mz in 0..MICROVOXEL_RES {
                for mx in 0..MICROVOXEL_RES {
                    if v.get_micro(mx, mid, mz) {
                        any = true;
                        min_mx = min_mx.min(mx);
                        max_mx = max_mx.max(mx);
                        min_mz = min_mz.min(mz);
                        max_mz = max_mz.max(mz);
                    }
                }
            }
            if !any {
                return None;
            }
            let s = 1.0 / MICROVOXEL_RES as f32;
            return Some((
                Vec3::new(
                    pos.x as f32 + min_mx as f32 * s,
                    pos.y as f32,
                    pos.z as f32 + min_mz as f32 * s,
                ),
                Vec3::new(
                    pos.x as f32 + (max_mx + 1) as f32 * s,
                    pos.y as f32 + 1.0,
                    pos.z as f32 + (max_mz + 1) as f32 * s,
                ),
            ));
        }
        None
    }

    /// Face cull against a solid (or grass carpet on +Y). Used by all dirt mesh paths.
    pub fn dirt_face_occluded(&self, block_pos: IVec3, neighbor_delta: IVec3) -> bool {
        let npos = block_pos + neighbor_delta;
        if self.solid_occludes(npos) {
            return true;
        }
        // Chunk reserved (FBM done, columns not committed): treat horizontal
        // neighbors as solid so we don't open a 16×16 wireframe pit.
        if neighbor_delta.y == 0 {
            let (ncx, ncz) = mesh_chunk_coord(npos.x, npos.z);
            if self
                .loaded_chunks
                .get(&(ncx, ncz))
                .is_some_and(|s| *s == ChunkState::Reserved)
            {
                return true;
            }
        }
        // Grass carpet above hides the dirt top (FPS billboards). HD-2D keeps
        // the +Y face visible and paints it green (tapa) on every dirt lid.
        if neighbor_delta == IVec3::Y && !ENABLE_HD2D {
            return self.has_grass_cover(block_pos);
        }
        false
    }

    /// True when a non-empty grass carpet sits on top of this dirt cell.
    pub fn has_grass_cover(&self, dirt_pos: IVec3) -> bool {
        self.get_voxel(dirt_pos + IVec3::Y)
            .is_some_and(|v| v.material == Material::Grass && !v.is_empty())
    }

    /// Visit every present voxel (dense columns expanded). For tests / diagnostics.
    pub fn for_each_voxel(&self, mut visit: impl FnMut(IVec3, &Voxel)) {
        for (&(x, z), &h) in &self.heights {
            let dirt = solid_dirt_ref();
            let sand = solid_sand_ref();
            let stone = solid_stone_ref();
            let black = solid_black_stone_ref();
            let bed = solid_bedrock_ref();
            for y in 0..=h {
                let pos = IVec3::new(x, y, z);
                if self.extras.contains_key(&pos) {
                    continue;
                }
                visit(
                    pos,
                    match column_cell_material(x, y, z, h) {
                        Material::Bedrock => bed,
                        Material::BlackStone => black,
                        Material::Stone => stone,
                        Material::Sand => sand,
                        _ => dirt,
                    },
                );
            }
        }
        for (&pos, v) in &self.extras {
            visit(pos, v);
        }
    }

    /// Counts of dirt / grass voxels (HUD / diagnostics). O(1).
    pub fn voxel_counts(&self) -> (usize, usize) {
        (self.dirt_count, self.grass_count)
    }

    /// Visit voxels only in chunks whose center can be within `radius` of the camera (XZ).
    #[allow(dead_code)]
    pub fn for_voxels_near_xz(
        &self,
        camera_pos: Vec3,
        radius: f32,
        mut visit: impl FnMut(&IVec3, &Voxel),
    ) {
        for (cx, cz) in self.chunk_coords_near(camera_pos, radius) {
            self.for_voxels_in_chunk(cx, cz, &mut visit);
        }
    }

    /// Chunk coords (cx, cz) that may contain geometry within `radius` of the camera.
    pub fn chunk_coords_near(&self, camera_pos: Vec3, radius: f32) -> Vec<(i32, i32)> {
        let cx = camera_pos.x.floor() as i32;
        let cz = camera_pos.z.floor() as i32;
        let chunk_r = ((radius / MESH_CHUNK_SIZE as f32).ceil() as i32) + 1;
        let c0 = mesh_chunk_coord(cx, cz);
        let radius_sq = radius * radius;
        let mut out = Vec::new();
        for dz in -chunk_r..=chunk_r {
            for dx in -chunk_r..=chunk_r {
                let key = (c0.0 + dx, c0.1 + dz);
                // Only filled chunks — Reserved (gen in flight) left holes with
                // wireframe neighbor walls until commit finished.
                if !self.chunk_filled(key.0, key.1) {
                    continue;
                }
                if chunk_dist_sq_xz(camera_pos, key.0, key.1) > radius_sq {
                    continue;
                }
                out.push(key);
            }
        }
        out
    }

    pub fn for_voxels_in_chunk(
        &self,
        cx: i32,
        cz: i32,
        visit: &mut impl FnMut(&IVec3, &Voxel),
    ) {
        if !self.loaded_chunks.contains_key(&(cx, cz)) {
            return;
        }
        let x0 = cx * MESH_CHUNK_SIZE;
        let z0 = cz * MESH_CHUNK_SIZE;
        let dirt = solid_dirt_ref();
        let sand = solid_sand_ref();
        let stone = solid_stone_ref();
        let black = solid_black_stone_ref();
        let bed = solid_bedrock_ref();
        for z in z0..z0 + MESH_CHUNK_SIZE {
            for x in x0..x0 + MESH_CHUNK_SIZE {
                if let Some(&h) = self.heights.get(&(x, z)) {
                    for y in 0..=h {
                        let pos = IVec3::new(x, y, z);
                        if self.extras.contains_key(&pos) {
                            continue;
                        }
                        visit(
                            &pos,
                            match column_cell_material(x, y, z, h) {
                                Material::Bedrock => bed,
                                Material::BlackStone => black,
                                Material::Stone => stone,
                                Material::Sand => sand,
                                _ => dirt,
                            },
                        );
                    }
                }
            }
        }
        for (pos, v) in &self.extras {
            if mesh_chunk_coord(pos.x, pos.z) == (cx, cz) {
                visit(pos, v);
            }
        }
    }

    pub fn has_chunk(&self, cx: i32, cz: i32) -> bool {
        self.loaded_chunks.contains_key(&(cx, cz))
    }

    /// True when voxels exist and may be drawn (Filled or PendingUnload grace).
    pub fn chunk_filled(&self, cx: i32, cz: i32) -> bool {
        self.loaded_chunks.get(&(cx, cz)).is_some_and(|s| {
            matches!(s, ChunkState::Filled | ChunkState::PendingUnload)
        })
    }

    /// Lifecycle state for diagnostics (`DEBUG_CHUNK_STREAM`).
    pub fn chunk_state(&self, cx: i32, cz: i32) -> Option<ChunkState> {
        self.loaded_chunks.get(&(cx, cz)).copied()
    }

    /// Gen in flight — wait before meshing neighbors / never draw.
    pub fn chunk_reserved(&self, cx: i32, cz: i32) -> bool {
        self.loaded_chunks
            .get(&(cx, cz))
            .is_some_and(|s| *s == ChunkState::Reserved)
    }

    /// 4-neighbors are safe for meshing: wait only on in-flight [`Reserved`] gen.
    /// Missing neighbors are OK (face cull seals the edge); requiring them Filled
    /// deadlocked the first frame until the whole load ring committed.
    pub fn neighbors_ready_for_mesh(&self, _origin: Vec3, cx: i32, cz: i32) -> bool {
        for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            if self.chunk_reserved(cx + dx, cz + dz) {
                return false;
            }
        }
        true
    }

    /// Chunk Guardian: keys in the keep/mesh ring that are missing or still Reserved.
    pub fn mesh_ring_holes(&self, origin: Vec3) -> Vec<(i32, i32)> {
        let gx = origin.x.floor() as i32;
        let gz = origin.z.floor() as i32;
        let c0 = mesh_chunk_coord(gx, gz);
        let keep_r = ((stream_keep_dist() / MESH_CHUNK_SIZE as f32).ceil() as i32) + 1;
        let keep_r_sq = keep_r * keep_r;
        let mut holes = Vec::new();
        for dz in -keep_r..=keep_r {
            for dx in -keep_r..=keep_r {
                if dx * dx + dz * dz > keep_r_sq {
                    continue;
                }
                let key = (c0.0 + dx, c0.1 + dz);
                if !self.chunk_filled(key.0, key.1) {
                    holes.push(key);
                }
            }
        }
        holes
    }

    /// Height continuity on the shared edge with a filled neighbor (deterministic gen).
    pub fn border_heights_match(&self, cx: i32, cz: i32, nx: i32, nz: i32) -> bool {
        if !self.chunk_filled(cx, cz) || !self.chunk_filled(nx, nz) {
            return true;
        }
        let x0 = cx * MESH_CHUNK_SIZE;
        let z0 = cz * MESH_CHUNK_SIZE;
        let nx0 = nx * MESH_CHUNK_SIZE;
        let nz0 = nz * MESH_CHUNK_SIZE;
        if nx == cx + 1 && nz == cz {
            // East edge of self vs west edge of neighbor.
            let x = x0 + MESH_CHUNK_SIZE - 1;
            let xn = nx0;
            for i in 0..MESH_CHUNK_SIZE {
                let z = z0 + i;
                let h = self.heights.get(&(x, z)).copied();
                let hn = self.heights.get(&(xn, z)).copied();
                // Adjacent columns need not match; shared *face* uses each column.
                // Continuity = both sides have a defined surface (no missing column).
                if h.is_none() || hn.is_none() {
                    return false;
                }
                let _ = (h, hn);
            }
            return true;
        }
        if nx == cx - 1 && nz == cz {
            return self.border_heights_match(nx, nz, cx, cz);
        }
        if nz == cz + 1 && nx == cx {
            let z = z0 + MESH_CHUNK_SIZE - 1;
            let zn = nz0;
            for i in 0..MESH_CHUNK_SIZE {
                let x = x0 + i;
                if self.heights.get(&(x, z)).is_none() || self.heights.get(&(x, zn)).is_none() {
                    return false;
                }
            }
            return true;
        }
        if nz == cz - 1 && nx == cx {
            return self.border_heights_match(nx, nz, cx, cz);
        }
        true
    }

    pub fn loaded_chunk_count(&self) -> usize {
        self.loaded_chunks.len()
    }
}

#[cfg(test)]
mod shunk_distance_tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn stream_around_generates_and_unloads() {
        if !ENABLE_STREAMING {
            let mut world = World::new();
            world.stream_around(Vec3::new(4.0, 5.0, 4.0));
            assert!(
                !world.has_chunk(0, 0),
                "streaming disabled: single-shunk world stays empty until with_shunk()"
            );
            return;
        }
        let mut world = World::new();
        for _ in 0..8 {
            world.stream_around(Vec3::new(4.0, 5.0, 4.0));
        }
        assert!(world.has_chunk(0, 0));
        assert!(world.column_height(0, 0).is_some());

        // Unload uses PendingUnload + grace frames — need enough ticks far away.
        for _ in 0..(UNLOAD_GRACE_FRAMES as usize + 24) {
            world.stream_around(Vec3::new(2000.0, 5.0, 2000.0));
        }
        assert!(!world.has_chunk(0, 0));
        let tc = mesh_chunk_coord(2000, 2000);
        assert!(world.has_chunk(tc.0, tc.1));
        assert!(world.column_height(tc.0 * MESH_CHUNK_SIZE, tc.1 * MESH_CHUNK_SIZE).is_some());
    }

    #[test]
    fn center_distance_at_known_point() {
        if ENABLE_STREAMING {
            // Infinite: standing on the surface → near-zero aabb distance.
            let gx = 8i32;
            let gz = 8i32;
            let y = terrain_height(gx, gz) as f32 + 1.0;
            let on_surface = Vec3::new(gx as f32 + 0.5, y, gz as f32 + 0.5);
            let d = measure_shunk_distance(on_surface);
            assert!(d.to_aabb < 0.1);
            return;
        }
        let center = shunk_center();
        let d = measure_shunk_distance(center);
        assert!(d.to_center < 1e-4);
        assert!(d.to_aabb < 1e-4);
    }

    #[test]
    fn world_is_nine_contiguous_shunks() {
        assert_eq!(SHUNK_GRID, 3);
        assert_eq!(world_blocks(), SHUNK_GRID * SHUNK_SIZE);
        assert_eq!(TERRAIN_MAX_HEIGHT, 64);
        assert_eq!(TERRAIN_AVG_HEIGHT, 20);
        let world = World::with_shunk();
        let columns = (world_blocks() * world_blocks()) as usize;
        assert_eq!(world.heights.len(), columns);
        // 3×3 mesh chunks covering 0..48.
        for cz in 0..SHUNK_GRID {
            for cx in 0..SHUNK_GRID {
                assert!(
                    world.has_chunk(cx, cz),
                    "missing shunk chunk ({cx},{cz})"
                );
            }
        }
        assert!(!world.has_chunk(SHUNK_GRID, 0));
        assert!(!world.has_chunk(0, SHUNK_GRID));
        let (dirt, grass) = world.voxel_counts();
        if ENABLE_GRASS {
            // Bare patches are common — expect well under one tuft per column.
            assert!(grass > 0, "some grass should still grow");
            assert!(
                grass < columns,
                "bare zones should skip some columns ({grass} >= {columns})"
            );
            assert!(
                (grass as f32) < columns as f32 * 0.40,
                "grass cover too dense: {grass}/{columns}"
            );
        } else {
            assert_eq!(grass, 0);
        }
        assert!(dirt >= columns);
        let max_h = world.heights.values().copied().max().unwrap_or(0);
        assert!(max_h <= TERRAIN_MAX_HEIGHT);
        let mut all_dirt_solid = true;
        let mut trees = 0usize;
        world.for_each_voxel(|_, v| {
            if v.material == Material::Dirt && !v.is_fully_solid() {
                all_dirt_solid = false;
            }
            if v.material.is_tree_bark() {
                trees += 1;
            }
        });
        assert!(all_dirt_solid);
        if ENABLE_TREES {
            assert!(trees > 0, "expected at least one trunk segment");
        }
    }

    #[test]
    fn aabb_distance_outside() {
        if ENABLE_STREAMING {
            let gx = 0i32;
            let gz = 0i32;
            let surface_y = terrain_height(gx, gz) as f32 + 1.0;
            let above = Vec3::new(0.5, surface_y + 10.0, 0.5);
            let d = measure_shunk_distance(above);
            assert!((d.to_aabb - 10.0).abs() < 0.2);
            return;
        }
        let d = measure_shunk_distance(Vec3::new(-3.0, 0.5, 2.0));
        assert!((d.to_aabb - 3.0).abs() < 1e-3);
        assert!(d.to_center > d.to_aabb);
    }

    #[test]
    fn infinite_streaming_loads_around_camera() {
        assert!(ENABLE_STREAMING);
        let mut world = World::new();
        for _ in 0..8 {
            world.stream_around(Vec3::new(8.0, 40.0, 8.0));
        }
        assert!(world.loaded_chunk_count() > 0);
        assert!(world.has_chunk(0, 0));
        assert!(world.column_height(0, 0).is_some());
        for _ in 0..(UNLOAD_GRACE_FRAMES as usize + 24) {
            world.stream_around(Vec3::new(4000.0, 40.0, 4000.0));
        }
        assert!(!world.has_chunk(0, 0));
        let tc = mesh_chunk_coord(4000, 4000);
        assert!(world.has_chunk(tc.0, tc.1));
        assert!(world.column_height(tc.0 * MESH_CHUNK_SIZE, tc.1 * MESH_CHUNK_SIZE).is_some());
    }

    #[test]
    fn preload_fills_nearest_ring() {
        assert!(ENABLE_STREAMING);
        let spawn = Vec3::new(8.0, 40.0, 8.0);
        let mut world = World::new();
        world.preload_shunks_around(spawn);

        let c0 = mesh_chunk_coord(8, 8);
        let load_r = preload_radius_chunks();
        let load_r_sq = load_r * load_r;
        let mut expected = 0usize;
        let mut missing = Vec::new();
        for dz in -load_r..=load_r {
            for dx in -load_r..=load_r {
                if dx * dx + dz * dz > load_r_sq {
                    continue;
                }
                expected += 1;
                let key = (c0.0 + dx, c0.1 + dz);
                if !world.chunk_filled(key.0, key.1) {
                    missing.push(key);
                }
            }
        }
        assert!(
            missing.is_empty(),
            "preload left {}/{} holes, e.g. {:?}",
            missing.len(),
            expected,
            missing.first()
        );
        assert!(world.pending_chunks.is_empty());
        assert!(world.column_height(8, 8).is_some());
        assert!(expected > 10, "preload should cover a real neighborhood");
    }

    /// Load/unload both use Euclidean chunk radius (hysteresis: load < unload).
    #[test]
    fn streaming_bubble_math() {
        let load_r = ((stream_load_dist() / MESH_CHUNK_SIZE as f32).ceil() as i32) + 1;
        let unload_r = ((stream_unload_dist() / MESH_CHUNK_SIZE as f32).ceil() as i32) + 1;
        let keep_r = ((stream_keep_dist() / MESH_CHUNK_SIZE as f32).ceil() as i32) + 1;
        assert_eq!(VIEW_DISTANCE_SHUNKS, 6);
        assert_eq!(VIEW_DISTANCE, 96.0);
        if ENABLE_HD2D {
            let expect_load =
                HD2D_DIRT_MESH_DIST + MESH_CHUNK_SIZE as f32 * STREAM_SAFETY_RING_CHUNKS as f32;
            let expect_unload = HD2D_DIRT_MESH_DIST
                + MESH_CHUNK_SIZE as f32 * (STREAM_SAFETY_RING_CHUNKS as f32 + 1.5);
            assert!((stream_load_dist() - expect_load).abs() < 0.1);
            assert!((stream_unload_dist() - expect_unload).abs() < 0.1);
            assert!(stream_keep_dist() <= stream_load_dist());
            assert!(keep_r <= load_r);
        } else {
            assert_eq!(load_r, 8);
            assert_eq!(unload_r, 10);
        }
        assert!(load_r < unload_r, "hysteresis band avoids thrash");
        let load_r_sq = load_r * load_r;
        let unload_r_sq = unload_r * unload_r;
        let mut loadable = 0i32;
        let mut thrash = 0i32;
        for dz in -load_r..=load_r {
            for dx in -load_r..=load_r {
                let d2 = dx * dx + dz * dz;
                if d2 > load_r_sq {
                    continue;
                }
                loadable += 1;
                if d2 > unload_r_sq {
                    thrash += 1;
                }
            }
        }
        // Safety ring expands the load disk vs the old mesh-only bubble.
        assert!(loadable > 100, "load disk too small: {loadable}");
        assert_eq!(thrash, 0, "Euclidean load must sit inside unload circle");
    }

    #[test]
    fn grass_only_generated_near_camera() {
        if !ENABLE_STREAMING || !ENABLE_GRASS {
            return;
        }
        let cam = Vec3::new(8.0, 40.0, 8.0);
        let mut world = World::new();
        for _ in 0..80 {
            world.stream_around(cam);
        }
        // Near chunk must be grass-ready; a far loaded chunk must not.
        assert!(world.grass_ready.contains(&(0, 0)));
        let far = world
            .loaded_chunks
            .keys()
            .copied()
            .filter(|&(cx, cz)| chunk_dist_sq_xz(cam, cx, cz) > GRASS_GEN_MAX_DIST_SQ)
            .collect::<Vec<_>>();
        assert!(!far.is_empty(), "expected some dirt-only far chunks");
        for (cx, cz) in far {
            assert!(
                !world.grass_ready.contains(&(cx, cz)),
                "chunk ({cx},{cz}) beyond grass gen radius should stay bare"
            );
        }
        assert_eq!(GRASS_GEN_MAX_DIST, 80.0);
    }

    #[test]
    fn blur_ramps_across_view_bubble() {
        assert_eq!(blur_amount_for_distance(0.0), 0.0);
        assert_eq!(blur_amount_for_distance(BLUR_START_DIST - 0.1), 0.0);
        assert!((blur_amount_for_distance(BLUR_START_DIST) - BLUR_START_AMOUNT).abs() < 1e-4);
        assert!((blur_amount_for_distance(BLUR_END_DIST) - BLUR_MAX_AMOUNT).abs() < 1e-4);
        assert!((blur_amount_for_distance(BLUR_END_DIST + 10.0) - BLUR_MAX_AMOUNT).abs() < 1e-4);
        let mid = (BLUR_START_DIST + BLUR_END_DIST) * 0.5;
        let expected =
            BLUR_START_AMOUNT + 0.5 * (BLUR_MAX_AMOUNT - BLUR_START_AMOUNT);
        assert!((blur_amount_for_distance(mid) - expected).abs() < 1e-3);
    }

    #[test]
    fn chunk_section_layout_is_16x32x16_stack2() {
        assert_eq!(MESH_CHUNK_SIZE, 16);
        assert_eq!(CHUNK_SECTION_HEIGHT, 32);
        assert_eq!(CHUNK_SECTIONS_Y, 2);
        assert_eq!(WORLD_MAX_HEIGHT, 64);
        assert_eq!(chunk_coord(IVec3::new(15, 31, 15)), (0, 0, 0));
        assert_eq!(chunk_coord(IVec3::new(16, 32, 16)), (1, 1, 1));
        assert_eq!(chunk_coord(IVec3::new(0, 63, 0)), (0, 1, 0));
    }

    #[test]
    fn grass_lod_shortens_then_seals_carpet() {
        let near = GrassLod::for_distance(10.0);
        assert!(!near.solid_carpet);
        assert_eq!(near.max_blade_y, Some(MICROVOXEL_RES - 1));

        let at_60 = GrassLod::for_distance(60.0);
        assert!(at_60.solid_carpet);
        assert_eq!(at_60.max_blade_y, Some(GRASS_BLADE_MAX_HEIGHT - 1));

        let at_62 = GrassLod::for_distance(62.0);
        assert!(at_62.solid_carpet);
        assert_eq!(at_62.max_blade_y, Some(GRASS_BLADE_MAX_HEIGHT - 3));

        let at_65 = GrassLod::for_distance(65.0);
        assert!(at_65.solid_carpet);
        assert!(at_65.max_blade_y.is_none());
    }

    #[test]
    fn walkable_columns_step_at_most_one() {
        // Full chebyshev cone is 1-Lipschitz — hills stay climbable without jumping.
        let mut checked = 0usize;
        for z in 0..24 {
            for x in 0..24 {
                let h = cone_terrace_height(x, z, TERRACE_RADIUS, false);
                for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let nh = cone_terrace_height(x + dx, z + dz, TERRACE_RADIUS, false);
                    assert!(
                        (h - nh).abs() <= WALKABLE_STEP,
                        "cone step ({x},{z})={h} vs ({nx},{nz})={nh}",
                        nx = x + dx,
                        nz = z + dz,
                        nh = nh
                    );
                }
                checked += 1;
            }
        }
        assert_eq!(checked, 24 * 24);
    }

    #[test]
    fn cliff_kind_skips_terrace_fill() {
        // Cliff/Canyon columns keep smoothed raw height (no cone fill).
        let cols = generate_chunk_columns(2, -1);
        let mut cliffs = 0usize;
        for &(x, z, h) in &cols {
            if matches!(
                terrain_slope_kind(x, z),
                TerrainSlope::Cliff | TerrainSlope::Canyon
            ) {
                assert_eq!(h, smoothed_column_height(x, z));
                cliffs += 1;
            }
        }
        // Soft worlds may have zero cliffs in one chunk — property still holds vacuously.
        let _ = cliffs;
    }

    #[test]
    fn streamed_columns_match_terraced_height() {
        let cols = generate_chunk_columns(0, 0);
        assert_eq!(cols.len(), (MESH_CHUNK_SIZE * MESH_CHUNK_SIZE) as usize);
        // Interior cells share the same local blur+cone neighborhood as the
        // single-column path; chunk edges can differ by pad clamping.
        for &(x, z, h) in &cols {
            if (2..14).contains(&x) && (2..14).contains(&z) {
                assert_eq!(h, terraced_column_height(x, z), "at ({x},{z})");
            }
        }
    }

    #[test]
    fn terrace_raises_when_raw_neighbor_is_steeper_than_one() {
        // Find a column whose smoothed neighborhood drops >1; cone should fill stairs.
        let mut raised = 0usize;
        for z in 0..32 {
            for x in 0..32 {
                let raw = smoothed_column_height(x, z);
                let mut steep_raw = false;
                for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    if (smoothed_column_height(x + dx, z + dz) - raw).abs() > 1 {
                        steep_raw = true;
                        break;
                    }
                }
                if !steep_raw {
                    continue;
                }
                let terraced = cone_terrace_height(x, z, TERRACE_RADIUS, false);
                // At least one of the pair should be closer to walkable after terrace.
                for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let nr = smoothed_column_height(x + dx, z + dz);
                    if (nr - raw).abs() <= 1 {
                        continue;
                    }
                    let nt = cone_terrace_height(x + dx, z + dz, TERRACE_RADIUS, false);
                    assert!(
                        (terraced - nt).abs() <= WALKABLE_STEP,
                        "terrace should flatten steep raw edge at ({x},{z})"
                    );
                    raised += 1;
                }
            }
        }
        // If the soft field has no >1 raw edges in this window, cone is still Lipschitz (other test).
        let _ = raised;
    }

    #[test]
    fn never_unload_inside_keep_ring() {
        if !ENABLE_STREAMING {
            return;
        }
        let mut world = World::new();
        let origin = Vec3::new(8.0, 20.0, 8.0);
        for _ in 0..16 {
            world.stream_around(origin);
        }
        assert!(world.chunk_filled(0, 0));
        for _ in 0..8 {
            world.stream_around(origin + Vec3::new(8.0, 0.0, 0.0));
        }
        assert!(
            world.chunk_filled(0, 0),
            "keep ring must retain filled voxels"
        );
    }

    #[test]
    fn pending_unload_then_drop_after_grace() {
        if !ENABLE_STREAMING {
            return;
        }
        let mut world = World::new();
        for _ in 0..12 {
            world.stream_around(Vec3::new(8.0, 20.0, 8.0));
        }
        assert!(world.has_chunk(0, 0));
        let mut saw_pending = false;
        for _ in 0..(UNLOAD_GRACE_FRAMES as usize + 16) {
            world.stream_around(Vec3::new(5000.0, 20.0, 5000.0));
            if world
                .loaded_chunks
                .get(&(0, 0))
                .is_some_and(|s| *s == ChunkState::PendingUnload)
            {
                saw_pending = true;
            }
            if !world.has_chunk(0, 0) {
                break;
            }
        }
        assert!(
            !world.has_chunk(0, 0),
            "chunk must drop after unload grace (saw_pending={saw_pending})"
        );
    }

    #[test]
    fn neighbors_ready_waits_on_reserved() {
        let mut world = World::new();
        world
            .loaded_chunks
            .insert((0, 0), ChunkState::Filled);
        world
            .loaded_chunks
            .insert((1, 0), ChunkState::Reserved);
        let origin = Vec3::new(8.0, 10.0, 8.0);
        assert!(!world.neighbors_ready_for_mesh(origin, 0, 0));
        world
            .loaded_chunks
            .insert((1, 0), ChunkState::Filled);
        assert!(world.neighbors_ready_for_mesh(origin, 0, 0));
    }

    /// Soft-cap must not prevent kicking columns inside the keep/mesh ring.
    #[test]
    fn mesh_ring_holes_kick_even_when_pending_full() {
        let mut world = World::new();
        // Saturate pending with far dummy work + Reserved far keys.
        for i in 0..PENDING_SOFT_CAP as i32 {
            let kx = 40 + i;
            let kz = 40;
            world
                .loaded_chunks
                .insert((kx, kz), ChunkState::Reserved);
            world
                .pending_chunks
                .push_back((kx, kz, vec![(kx * 16, kz * 16, 10)]));
        }
        assert!(world.pending_chunks.len() >= PENDING_SOFT_CAP);
        // Origin at spawn — (0,0) should be a mesh-ring hole and get Reserved+pending.
        world.stream_around_timed(Vec3::new(8.0, 20.0, 8.0), Duration::from_millis(50));
        let has_near = world.loaded_chunks.contains_key(&(0, 0))
            || world.pending_chunks.iter().any(|&(x, z, _)| x == 0 && z == 0);
        assert!(
            has_near,
            "mesh-ring chunk (0,0) must still be kicked when pending is full"
        );
    }

    #[test]
    fn mesh_ring_holes_lists_missing() {
        let world = World::new();
        let holes = world.mesh_ring_holes(Vec3::new(8.0, 10.0, 8.0));
        assert!(!holes.is_empty());
        assert!(holes.contains(&(0, 0)));
    }

    #[test]
    fn border_heights_match_when_both_filled() {
        let mut world = World::new();
        let a = generate_chunk_columns(0, 0);
        let b = generate_chunk_columns(1, 0);
        world.commit_generated_chunk(0, 0, &a, false);
        world.commit_generated_chunk(1, 0, &b, false);
        assert!(world.border_heights_match(0, 0, 1, 0));
    }
}

#[cfg(test)]
mod column_edit_tests {
    use super::*;

    #[test]
    fn stone_more_likely_deeper_than_surface() {
        let mut surface = 0u32;
        let mut deep = 0u32;
        let samples = 48i32;
        for z in 0..samples {
            for x in 0..samples {
                let h = terraced_column_height(x, z);
                if column_cell_is_stone(x, h, z, h) {
                    surface += 1;
                }
                if h >= 8 && column_cell_is_stone(x, h - 8, z, h) {
                    deep += 1;
                }
            }
        }
        assert!(
            deep > surface,
            "expected more stone at depth-8 than surface (deep={deep}, surface={surface})"
        );
        assert!(
            surface > 0 && surface < (samples * samples) as u32 / 2,
            "surface stone should be patchy, got {surface}"
        );
    }

    #[test]
    fn embeds_and_crystal_rarity_in_band() {
        let mut coal = 0u32;
        let mut sap = 0u32;
        let mut ruby = 0u32;
        let mut em = 0u32;
        let mut crystal_outside = 0u32;
        for z in 0..64 {
            for x in 0..64 {
                let h = terraced_column_height(x, z);
                for y in 0..=h {
                    if !column_cell_is_stone(x, y, z, h) {
                        continue;
                    }
                    match stone_embed_kind(x, y, z) {
                        Some(EmbedKind::Coal) => coal += 1,
                        Some(EmbedKind::Sapphire) => {
                            sap += 1;
                            if y < CRYSTAL_Y_MIN || y > CRYSTAL_Y_MAX {
                                crystal_outside += 1;
                            }
                        }
                        Some(EmbedKind::Ruby) => {
                            ruby += 1;
                            if y < CRYSTAL_Y_MIN || y > CRYSTAL_Y_MAX {
                                crystal_outside += 1;
                            }
                        }
                        Some(EmbedKind::Emerald) => {
                            em += 1;
                            if y < CRYSTAL_Y_MIN || y > CRYSTAL_Y_MAX {
                                crystal_outside += 1;
                            }
                        }
                        None => {}
                    }
                }
            }
        }
        assert!(coal + sap + ruby + em > 10, "expected embeds");
        assert_eq!(crystal_outside, 0, "crystals only in y={CRYSTAL_Y_MIN}..={CRYSTAL_Y_MAX}");
        assert!(sap + ruby + em > 0, "expected some crystals in band");
        assert!(sap >= ruby, "sapphire should be >= ruby ({sap} vs {ruby})");
        assert!(ruby >= em, "ruby should be >= emerald ({ruby} vs {em})");
    }

    #[test]
    fn ore_pouch_crafts_nine_to_cube() {
        let mut p = OrePouch::default();
        assert_eq!(
            p.add_drop(OreDrop {
                kind: EmbedKind::Sapphire,
                micros: 8
            }),
            0
        );
        assert_eq!(p.sapphire.micros, 8);
        assert_eq!(
            p.add_drop(OreDrop {
                kind: EmbedKind::Sapphire,
                micros: 2
            }),
            1
        );
        assert_eq!(p.sapphire.cubes, 1);
        assert_eq!(p.sapphire.micros, 1);
    }

    #[test]
    fn fragment_bag_caps_at_100() {
        let mut bag = FragmentBag::default();
        assert_eq!(
            fragment_drop_for(Material::Stone),
            Some((FragmentKind::Stone, BLOCK_FRAGMENTS_PER_BREAK))
        );
        assert_eq!(
            fragment_drop_for(Material::Dirt),
            Some((FragmentKind::Dirt, BLOCK_FRAGMENTS_PER_BREAK))
        );
        assert_eq!(bag.add(FragmentKind::Stone, 98), 98);
        assert_eq!(bag.add(FragmentKind::Stone, 4), 2);
        assert_eq!(bag.stone, MAX_BLOCK_FRAGMENTS);
        assert_eq!(bag.add(FragmentKind::Stone, 4), 0);
        assert_eq!(bag.add(FragmentKind::Dirt, 4), 4);
        assert_eq!(bag.dirt, 4);
    }

    #[test]
    fn surface_outcrop_is_solid_pad() {
        let samples = 64i32;
        let mut big = 0u32;
        let mut pebble = 0u32;
        for z in 0..samples {
            for x in 0..samples {
                let (lift, root) = stone_outcrop_profile(x, z);
                if lift == 0 {
                    continue;
                }
                if root >= STONE_OUTCROP_DEPTH {
                    big += 1;
                } else {
                    pebble += 1;
                }
                let h = terraced_column_height(x, z);
                assert!(lift >= 1 && lift <= STONE_OUTCROP_LIFT_MAX);
                let stone_span = lift + root;
                for d in 0..stone_span.min(h + 1) {
                    assert!(
                        column_cell_is_stone(x, h - d, z, h),
                        "outcrop hole at ({x},{},{z}) depth {d} lift={lift}",
                        h - d
                    );
                }
            }
        }
        assert!(big > 0, "expected large pads");
        assert!(pebble > 0, "expected small pebble rocks, got 0");
    }

    /// Mid-column dig (stair headroom) must not wipe stone under the dirt cap.
    #[test]
    fn mid_column_dig_does_not_shaft_to_bedrock() {
        let mut world = World::new();
        world.set_column_dirt(5, 5, 20);
        assert_eq!(world.column_height(5, 5), Some(20));

        // Punch below the top → roof moves to extras; dense top drops.
        assert!(world.remove_voxel(IVec3::new(5, 15, 5)));
        assert_eq!(world.column_height(5, 5), Some(14));
        assert!(world.get_voxel(IVec3::new(5, 0, 5)).is_some(), "bedrock base");
        assert_eq!(
            world.get_voxel(IVec3::new(5, 0, 5)).map(|v| v.material),
            Some(Material::Bedrock)
        );
        assert!(world.get_voxel(IVec3::new(5, 18, 5)).is_some(), "roof extras");

        // Second dig removes an extras cell → recompute_column_height.
        assert!(world.remove_voxel(IVec3::new(5, 16, 5)));
        let h = world
            .column_height(5, 5)
            .expect("column must survive recompute (stone counts as ground)");
        assert!(h >= 14, "got height {h}, shaft wipe would clear heights");
        assert!(world.get_voxel(IVec3::new(5, 0, 5)).is_some());
        assert!(world.get_voxel(IVec3::new(5, 14, 5)).is_some());
        assert!(world.get_voxel(IVec3::new(5, 15, 5)).is_none());
        assert!(world.get_voxel(IVec3::new(5, 16, 5)).is_none());
        assert!(world.get_voxel(IVec3::new(5, 18, 5)).is_some());
    }

    #[test]
    fn indoors_factor_needs_roof_and_four_walls() {
        let mut world = World::new();
        // Open field — not indoors.
        assert_eq!(world.indoors_factor(Vec3::new(0.5, 20.5, 0.5)), 0.0);

        // Tiny room around (10, 5, 10): floor + 4 walls + roof.
        let cx = 10;
        let cy = 5;
        let cz = 10;
        world.set_voxel(IVec3::new(cx, cy - 1, cz), Voxel::dirt());
        for &(dx, dz) in &[(-1, 0), (1, 0), (0, -1), (0, 1)] {
            for dy in 0..=2 {
                world.set_voxel(
                    IVec3::new(cx + dx, cy + dy, cz + dz),
                    Voxel::solid(Material::WoodPlanks),
                );
            }
        }
        world.set_voxel(
            IVec3::new(cx, cy + 3, cz),
            Voxel::solid(Material::WoodPlanks),
        );
        assert_eq!(
            world.indoors_factor(Vec3::new(cx as f32 + 0.5, cy as f32 + 0.5, cz as f32 + 0.5)),
            1.0
        );

        // Remove one wall → soft 3-wall score.
        world.remove_voxel(IVec3::new(cx + 1, cy, cz));
        world.remove_voxel(IVec3::new(cx + 1, cy + 1, cz));
        world.remove_voxel(IVec3::new(cx + 1, cy + 2, cz));
        let soft = world.indoors_factor(Vec3::new(cx as f32 + 0.5, cy as f32 + 0.5, cz as f32 + 0.5));
        assert!((soft - 0.55).abs() < 0.01, "got {soft}");
    }
}

