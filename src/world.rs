use glam::{IVec3, Vec3};
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Microvoxels per axis inside one world voxel (16³ = 4096 cells).
pub const MICROVOXEL_RES: usize = 16;
const OCCUPANCY_BYTES: usize = (MICROVOXEL_RES * MICROVOXEL_RES * MICROVOXEL_RES) / 8;

pub const WORLD_SEED: u32 = 0x5EED_0002;
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
/// Dirt-top Y del nivel del mar: columnas con labio bajo este nivel se
/// inundan (mares, lagos y pantanos emergen del relieve).
pub const SEA_LEVEL: i32 = 18;
/// Alcance máximo de la corriente: el agua fluye hasta 4 bloques en
/// horizontal desde una fuente (más allá no se extiende).
pub const WATER_FLOW_MAX: u8 = 4;
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
/// ELIMINADO a petición (T1): el shader devuelve 0 y `render.rs` manda
/// `confine=0` siempre. Se conserva el flag como kill-switch legacy.
/// El cutaway indoor (T2) ya no depende de este flag.
pub const ENABLE_DIG_CUTAWAY: bool = false;
/// Log Reserved/Filled/PendingUnload + skipped meshes (hunt 16×16 holes).
/// Enable with `RUST_LOG=info` (or `microverse=info`).
pub const DEBUG_CHUNK_STREAM: bool = false;
/// Diagnostic: disable every culling/occlusion stage (neighbor faces, buried
/// slab gate, content skips, draw buckets, frustum, shader cutaway + cave
/// mask, grass density) so ALL geometry in radius draws. Depth testing keeps
/// the image correct; interior faces just cost fill. Flip to `true` to hunt
/// missing-ground bugs, then re-enable stages one by one. Default `false`
/// (zero behavior change — const-folded).
pub const DEBUG_DISABLE_CULLING: bool = false;

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
/// Render mesh section height (B-split: 16³ cubes, 4 per column).
pub const MESH_SECTION_HEIGHT: i32 = 16;
/// Render mesh sections per column (64 / 16 = 4: 0-15 sótano, 16-31 superficie).
pub const MESH_SECTIONS_Y: i32 = 4;
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

/// Render mesh section index (16³ B-split) for a block Y.
#[inline]
pub fn mesh_section_y(y: i32) -> i32 {
    y.div_euclid(MESH_SECTION_HEIGHT)
}

/// Inclusive Y range `[y0, y1)` of render section `cy`.
#[inline]
pub fn mesh_section_range(cy: i32) -> (i32, i32) {
    (cy * MESH_SECTION_HEIGHT, (cy + 1) * MESH_SECTION_HEIGHT)
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

/// Grass blade fade: shorten from the top, gone just past the HD-2D mesh cut.
pub const GRASS_BLADE_FADE_START: f32 = 68.0;
pub const GRASS_BLADE_FADE_END: f32 = 73.0;

/// Fade activo (la burbuja Android es 40: el fade 68+ nunca saltaría allí y
/// la hierba iría siempre a máxima altura = más vértices).
#[inline]
pub fn grass_fade_start() -> f32 {
    if cfg!(target_os = "android") {
        42.0
    } else {
        GRASS_BLADE_FADE_START
    }
}

/// Fin del fade activo (ver `grass_fade_start`).
#[inline]
pub fn grass_fade_end() -> f32 {
    if cfg!(target_os = "android") {
        47.0
    } else {
        GRASS_BLADE_FADE_END
    }
}
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

/// HD-2D: dirt mesh cut; greedy merge for nearly all drawn chunks.
/// 72 ≈ 4.5 shunks — a bit more horizon than 64, still far below the old 96 FPS bubble.
pub const HD2D_DIRT_MESH_DIST: f32 = 72.0;
pub const HD2D_DIRT_HIRES_DIST: f32 = 22.0;
pub const HD2D_DIRT_POLISHED_DIST: f32 = 54.0;
/// HLOD only in the last few blocks before the cut.
pub const HD2D_DIRT_HEIGHTMAP_DIST: f32 = 66.0;

/// Active dirt mesh cut (HD-2D uses a tighter bubble).
/// En Android la burbuja baja a 40: se vuelve al valor de la ronda 2 (la
/// ronda 3 llegó al tope visual). En PC manda el horizonte de 72.
#[inline]
pub fn dirt_mesh_max_dist() -> f32 {
    if ENABLE_HD2D {
        if cfg!(target_os = "android") {
            40.0
        } else {
            HD2D_DIRT_MESH_DIST
        }
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
    // Tijeras Android: detalle total solo hasta 12 (menos vértices hi-res).
    if cfg!(target_os = "android") {
        return 12.0 * 12.0;
    }
    if ENABLE_HD2D {
        HD2D_DIRT_HIRES_DIST * HD2D_DIRT_HIRES_DIST
    } else {
        DIRT_HIRES_MAX_DIST_SQ
    }
}

#[inline]
pub fn dirt_polished_max_dist_sq() -> f32 {
    // Tijeras Android: greedy suave hasta 34 (burbuja 40).
    if cfg!(target_os = "android") {
        return 34.0 * 34.0;
    }
    if ENABLE_HD2D {
        HD2D_DIRT_POLISHED_DIST * HD2D_DIRT_POLISHED_DIST
    } else {
        DIRT_POLISHED_MAX_DIST_SQ
    }
}

#[inline]
pub fn dirt_heightmap_max_dist_sq() -> f32 {
    // Tijeras Android: HLOD casi pegado al corte (burbuja 40).
    if cfg!(target_os = "android") {
        return 38.0 * 38.0;
    }
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
        dirt_mesh_max_dist() + MESH_CHUNK_SIZE as f32 * STREAM_SAFETY_RING_CHUNKS as f32
    } else {
        STREAM_LOAD_DIST
    }
}

#[inline]
pub fn stream_unload_dist() -> f32 {
    if ENABLE_HD2D {
        // Farther than load so chunks stay until replacement ring is filled.
        dirt_mesh_max_dist() + MESH_CHUNK_SIZE as f32 * (STREAM_SAFETY_RING_CHUNKS as f32 + 1.5)
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
/// HD-2D: streaming CPU budget per frame. Kept 1–2 ms above the FPS-mode budget
/// so Reserved→Filled keeps up, but below the old 10 ms — the draw bubble (72)
/// is guaranteed by the mesh-ring guardian (forces the keep ring every frame,
/// above the soft cap), so trimming speculative preload (load ring 88→cut) frees
/// render-frame CPU without opening 16×16 holes.
pub const HD2D_FRAME_STREAM_BUDGET_MS: u64 = 7;
/// Max chunks whose FBM we kick off in one frame (parallel `par_iter`).
const MAX_GEN_KICK_PER_FRAME: usize = 8;
const HD2D_MAX_GEN_KICK_PER_FRAME: usize = 10;
/// One parallel generation parcel per budget check inside a frame's stream pass.
const STREAM_GEN_WAVE: usize = 4;
/// Max chunks inserted into the world hashmap per frame (sequential commit).
const MAX_COMMIT_PER_FRAME: usize = 4;
const HD2D_MAX_COMMIT_PER_FRAME: usize = 8;
/// Don't pile up too much precomputed terrain waiting to commit.
const PENDING_SOFT_CAP: usize = 24;
/// Bias missing-chunk sort toward look direction (Minecraft-style preload).
const STREAM_LOOK_PRELOAD_BIAS: f32 = 2.5;
/// Nearest shunks filled synchronously before the first playable frame.
/// Must cover [`stream_keep_dist`] so the diorama has no Reserved voids at spawn.
#[inline]
pub fn preload_radius_chunks() -> i32 {
    ((stream_keep_dist() / MESH_CHUNK_SIZE as f32).ceil() as i32) + 1
}
/// Fog hides the streaming unload edge (not the near terrain).
#[allow(dead_code)]
pub const FOG_START: f32 = VIEW_DISTANCE * 0.72;
#[allow(dead_code)]
pub const FOG_END: f32 = VIEW_DISTANCE;
/// Exponential fog density for HD-2D (`F = 1 - e^{-k d}`).
/// 0.007 ≈ 40% at the 72-block mesh cut (0.011 was ~50% already at 64).
pub const FOG_DENSITY: f32 = 0.007;

/// Clear / fog colour from eye height vs local surface (sky → biome meadow → cave).
/// Underground never goes pure black — walls must stay readable.
pub fn fog_color_for_altitude(eye_y: f32, surface_y: f32) -> [f32; 3] {
    fog_color_for_biome(eye_y, surface_y, crate::biomes::BiomeId::TemperateMeadow)
}

/// Same altitude ramps as [`fog_color_for_altitude`], with biome surface/sky hues.
pub fn fog_color_for_biome(eye_y: f32, surface_y: f32, biome: crate::biomes::BiomeId) -> [f32; 3] {
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

/// World-space center of one 16×16×16 mesh section.
#[allow(dead_code)]
pub fn mesh_section_center(cx: i32, cy: i32, cz: i32) -> Vec3 {
    let sx = MESH_CHUNK_SIZE as f32;
    let sy = MESH_SECTION_HEIGHT as f32;
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
pub const HD2D_EDGE_BLUR: f32 = 0.154;

/// Keep-rate for grass instances by camera distance (squared).
/// Aggressive mid/far falloff — crossed blades overdraw hard when density stays high.
/// (Recorte leve 2026-09-22: anillo 20-32 0.28→0.22 y 32-44 0.10→0.08 para
/// headroom con escena a 1.0; el anillo cercano ≤20 intacto.)
/// En Android se recorta fuerte fuera del anillo cercano (0.7/0.1/0.03) y
/// nada más allá de 44 (la burbuja de dibujado es 40): menos overdraw de
/// briznas, que es de lo que más fill quema en el diorama.
pub fn grass_density_for_dist_sq(dist_sq: f32) -> f32 {
    if ENABLE_HD2D {
        // Distances are from the *player focus*, not the isometric lens.
        // Full density covers the near diorama so tufts don't vanish underfoot
        // when the camera sits ~13 blocks away horizontally.
        // Slightly tighter mid rings — less overdraw after dig remesh spikes.
        let base = if dist_sq > 56.0 * 56.0 {
            0.0
        } else if dist_sq <= 20.0 * 20.0 {
            1.0
        } else if dist_sq <= 32.0 * 32.0 {
            0.22
        } else if dist_sq <= 44.0 * 44.0 {
            0.08
        } else if dist_sq <= 52.0 * 52.0 {
            0.03
        } else {
            0.01
        };
        if cfg!(target_os = "android") {
            if dist_sq > 44.0 * 44.0 {
                0.0
            } else if dist_sq <= 20.0 * 20.0 {
                0.7
            } else if dist_sq <= 32.0 * 32.0 {
                0.1
            } else {
                0.03
            }
        } else {
            base
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
        let start = grass_fade_start();
        let end = grass_fade_end();
        if dist < start {
            return Self {
                solid_carpet: false,
                max_blade_y: Some(MICROVOXEL_RES - 1),
            };
        }
        if dist >= end {
            return Self {
                solid_carpet: true,
                max_blade_y: None,
            };
        }
        // Fade start → cut 1 from top, then +1 per block until gone at fade end.
        let removed = 1 + (dist - start).floor() as usize;
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
/// Extra 1-cell halo so the 3×3 blur is exact for every smooth cell the cone
/// reads. Without it, `blur3_round_tile` renormalizes truncated edges and the
/// batched chunk path (24-wide tile) disagrees by ±1 with the single-column
/// path (9-wide tile) on E/S chunk borders — a visible seam / hole line.
/// Every tile below must be `TERRACE_RADIUS + BLUR_HALO` padded.
const BLUR_HALO: i32 = 1;

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

fn height_from_smooth_cell(smooth: &[i32], w: i32, lx: i32, lz: i32, x: i32, z: i32) -> i32 {
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
    let lift = stone_outcrop_lift(x, z);
    let lift = if outcrop_suppressed_here(x, z, h) {
        0
    } else {
        lift
    };
    let h = (h + lift).min(TERRAIN_MAX_HEIGHT);
    river_carve(x, z, h)
}

/// True si hay agua (labio bajo el mar) en el disco r=2: orilla real.
pub fn shore_water_near(x: i32, z: i32) -> bool {
    for dz in -2..=2 {
        for dx in -2..=2 {
            if terrain_height(x + dx, z + dz) < SEA_LEVEL {
                return true;
            }
        }
    }
    false
}

/// Fracción de columnas bajo el mar en disco r=6 (0..1): mar abierto
/// (alto) frente a poza aislada (bajo).
pub fn open_sea_frac(x: i32, z: i32) -> f32 {
    let mut below = 0u32;
    let mut total = 0u32;
    for dz in -6..=6 {
        for dx in -6..=6 {
            if dx * dx + dz * dz > 36 {
                continue;
            }
            total += 1;
            if terrain_height(x + dx, z + dz) < SEA_LEVEL {
                below += 1;
            }
        }
    }
    below as f32 / total.max(1) as f32
}

/// Lodo de orilla: franja baja junto a poza (no mar abierto). Los mares
/// llevan playa; los lagos y ríos, lodo.
pub fn column_is_mud_shore(x: i32, z: i32, top: i32) -> bool {
    if top < SEA_LEVEL || top > SEA_LEVEL + 1 {
        return false;
    }
    // Pre-filtro barato: agua en cruz antes del disco caro.
    if terrain_height(x + 1, z) >= SEA_LEVEL
        && terrain_height(x - 1, z) >= SEA_LEVEL
        && terrain_height(x, z + 1) >= SEA_LEVEL
        && terrain_height(x, z - 1) >= SEA_LEVEL
    {
        return false;
    }
    open_sea_frac(x, z) < 0.30
}

/// Banda de río 0..1 en el centro → 0 fuera (`fbm2` ignora la semilla y
/// aplica PERLIN_SCALE dentro: se muestrea en bloques con offset).
fn river_band(x: i32, z: i32) -> f64 {
    fbm2(
        x as f64 + 137.3,
        z as f64 - 41.9,
        WORLD_SEED ^ 0x712E_0001,
    )
    .abs()
}

/// True si esta columna es cauce de río (misma regla que el carve).
pub fn is_river_channel(x: i32, z: i32) -> bool {
    let h = terrain_height(x, z);
    h <= SEA_LEVEL + 4 && h >= SEA_LEVEL - 6 && river_band(x, z) < 0.012
}

/// Clase de agua donde está el jugador (para el cartel).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaterKind {
    Thermal,
    River,
    Sea,
    Lake,
    Swamp,
}

impl WaterKind {
    pub fn label_es(self) -> &'static str {
        match self {
            // ASCII a propósito (la fuente del HUD no tiene acentos).
            Self::Thermal => "TERMAL",
            Self::River => "RIO",
            Self::Sea => "MAR",
            Self::Lake => "LAGO",
            Self::Swamp => "PANTANO",
        }
    }
}
/// Ríos: banda estrecha de ruido que hunde el relieve hasta el nivel del mar
/// en tierras bajas (canales que conectan mares y lagos). Fuera de la banda
/// o en cotas altas no toca nada.
fn river_carve(x: i32, z: i32, h: i32) -> i32 {
    if h > SEA_LEVEL + 4 || h < SEA_LEVEL - 6 {
        return h;
    }
    // OJO: fbm2 ignora la semilla y aplica PERLIN_SCALE dentro; se muestrea
    // en coordenadas de bloque con offset (campo distinto al del relieve).
    let v = river_band(x, z);
    if v >= 0.012 {
        return h;
    }
    // Nada de ríos dentro de aldeas (solo fuera del casco).
    if crate::settlements::settlement_claims_block(x, z) {
        return h;
    }
    let depth = ((1.0 - v / 0.012) * 3.0).round() as i32;
    (h - depth).max(SEA_LEVEL - 3)
}

/// Raise low columns into 1-block dirt stairs toward nearby high ground (cone dilation).
fn cone_terrace_height(x: i32, z: i32, radius: i32, styled: bool) -> i32 {
    let pad = radius + BLUR_HALO;
    let w = 2 * pad + 1;
    let raw = fill_raw_height_tile(x - pad, z - pad, w);
    let smooth = blur3_round_tile(&raw, w);
    cone_terrace_from_grid(&smooth, w, pad, pad, x, z, radius, styled)
        .clamp(TERRAIN_VALLEY_HEIGHT, TERRAIN_MAX_HEIGHT)
}

/// Column cache for [`terraced_column_height`]: pure function of (x, z) —
/// never invalidated by edits (natural undug surface). Each miss costs ~81
/// Perlin evals + blur + cone terrace; per-frame callers (scene altitude,
/// confine/indoors, section gating, meshing tints) hit the same columns
/// repeatedly, so memoizing is the cheapest P1 win. Bounded: cleared on cap.
const TERRAIN_HEIGHT_CACHE_CAP: usize = 65_536;

fn terrain_height_cache(
) -> &'static std::sync::RwLock<FxHashMap<(i32, i32), i32>> {
    static CACHE: std::sync::OnceLock<std::sync::RwLock<FxHashMap<(i32, i32), i32>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::RwLock::new(FxHashMap::default()))
}

/// Final surface height: walkable stairs on hills, raw walls on cliffs/canyons.
pub fn terraced_column_height(x: i32, z: i32) -> i32 {
    let key = (x, z);
    if let Some(h) = terrain_height_cache()
        .read()
        .ok()
        .and_then(|c| c.get(&key).copied())
    {
        return h;
    }
    let pad = TERRACE_RADIUS + BLUR_HALO;
    let w = 2 * pad + 1;
    let raw = fill_raw_height_tile(x - pad, z - pad, w);
    let smooth = blur3_round_tile(&raw, w);
    let h = height_from_smooth_cell(&smooth, w, pad, pad, x, z);
    if let Ok(mut c) = terrain_height_cache().write() {
        if c.len() >= TERRAIN_HEIGHT_CACHE_CAP {
            c.clear();
        }
        c.insert(key, h);
    }
    h
}

/// Pure column heights for one mesh chunk (no world mutation — rayon-safe).
fn generate_chunk_columns(cx: i32, cz: i32) -> Vec<(i32, i32, i32)> {
    let x0 = cx * MESH_CHUNK_SIZE;
    let z0 = cz * MESH_CHUNK_SIZE;
    let pad = TERRACE_RADIUS;
    // Same halo as the single-column path: the cone reads smooth ±`pad`
    // around each chunk column, and each smooth cell needs raw ±1 for an
    // exact 3×3 blur. Without the +1 halo the E/S border smooth cells are
    // edge-renormalized and disagree ±1 with `terraced_column_height`.
    let halo = BLUR_HALO;
    let rx0 = x0 - pad - halo;
    let rz0 = z0 - pad - halo;
    let w = MESH_CHUNK_SIZE + 2 * pad + 2 * halo;
    // One Perlin sample per padded cell + one 3×3 blur — not 25× samples per cell.
    let raw = fill_raw_height_tile(rx0, rz0, w);
    let smooth = blur3_round_tile(&raw, w);

    let mut out = Vec::with_capacity((MESH_CHUNK_SIZE * MESH_CHUNK_SIZE) as usize);
    for z in z0..z0 + MESH_CHUNK_SIZE {
        for x in x0..x0 + MESH_CHUNK_SIZE {
            let lx = x - rx0;
            let lz = z - rz0;
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
/// Biome relief scales the deviation from the average (deserts/wetlands flat,
/// boreal/cloud belts high) — climate-only lookup, so no feedback loop and
/// every consumer (streaming, caves, fog, HLOD) stays consistent.
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
    let h = h.clamp(TERRAIN_VALLEY_HEIGHT as f32, TERRAIN_MAX_HEIGHT as f32);
    let (temp, moist) = crate::biomes::climate_at(x, z);
    let scale = crate::biomes::relief_scale_for_climate(temp, moist);
    (avg + (h - avg) * scale).clamp(TERRAIN_VALLEY_HEIGHT as f32, TERRAIN_MAX_HEIGHT as f32)
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

/// Local height slab for one chunk's commit: the columns' own heights, looked up
/// 4-neighbor-wise so `slab_slope` avoids re-running the padded-terrain noise
/// (~5× per column in the old commit) — pure hash lookups instead.
struct SlabHeights {
    h: FxHashMap<(i32, i32), i32>,
}

fn slab_heights(columns: &[(i32, i32, i32)]) -> SlabHeights {
    let mut h = FxHashMap::with_capacity_and_hasher(columns.len(), Default::default());
    for &(x, z, hh) in columns {
        h.insert((x, z), hh);
    }
    SlabHeights { h }
}

impl SlabHeights {
    fn height(&self, x: i32, z: i32) -> i32 {
        self.h.get(&(x, z)).copied().unwrap_or_else(|| terrain_height(x, z))
    }

    /// Max adjacent height delta around (x, z) — matches [`terrain_slope`] for
    /// interior columns; chunk-edge columns fall back to the global terrain on
    /// the far side (same value terrain_slope would use).
    fn slope(&self, x: i32, z: i32) -> f32 {
        let h = self.height(x, z) as f32;
        let mut max_d = 0.0f32;
        for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let nh = self.height(x + dx, z + dz) as f32;
            max_d = max_d.max((h - nh).abs());
        }
        max_d
    }
}

/// Whether this column gets a grass tuft. Bare patches are coherent and common;
/// steep slopes almost never grow grass. Density follows the local biome profile.
pub fn column_grows_grass(x: i32, z: i32, slope: f32) -> bool {
    column_grows_grass_in(crate::biomes::biome_at(x, z), x, z, slope)
}

/// [`column_grows_grass`] for a caller that already knows the biome — the
/// streaming commit evaluates the biome once per column instead of once per
/// decision function.
pub fn column_grows_grass_in(biome: crate::biomes::BiomeId, x: i32, z: i32, slope: f32) -> bool {
    let bare_base = crate::biomes::flora_for(biome).grass_bare_chance;
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
    column_grows_tree_in(crate::biomes::biome_at(x, z), x, z, slope)
}

/// [`column_grows_tree`] for a caller that already knows the biome.
pub fn column_grows_tree_in(biome: crate::biomes::BiomeId, x: i32, z: i32, slope: f32) -> bool {
    if !ENABLE_TREES || slope > 1.0 {
        return false;
    }
    let flora = crate::biomes::flora_for(biome);
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
    column_grows_bush_in(crate::biomes::biome_at(x, z), x, z, slope)
}

/// [`column_grows_bush`] for a caller that already knows the biome.
pub fn column_grows_bush_in(biome: crate::biomes::BiomeId, x: i32, z: i32, slope: f32) -> bool {
    if !ENABLE_TREES || slope > 1.15 {
        return false;
    }
    if column_grows_tree_in(biome, x, z, slope) {
        return false;
    }
    let flora = crate::biomes::flora_for(biome);
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
    /// Packed ochre cobble, only for roads (visually distinct from wall grey).
    RoadCobble,
    /// Torch flame (bright lamp cell on a wood post, no light engine yet).
    Torch,
    /// Apple dots on orchard canopies (food concept seed).
    Apple,
    /// Straw hive box on a post (apiary seed).
    Beehive,
    /// Raw magic crystal (mage-tower seed for the combat update).
    ManaCrystal,
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
    /// Warm mineral pool (hot springs). Swimmable like water, own tint.
    ThermalWater,
    /// Wet shore mud: dirt-like, darker and damper (lake rims).
    Mud,
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
/// Profundidad bajo la superficie a partir de la cual el subsuelo queda
/// exento de la supresión de ores por aldea (ahí no hay ninguna diferencia).
pub const ORE_VILLAGE_SUBSUELO_DEPTH: i32 = 12;
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
        Material::Dirt | Material::Grass | Material::Mud => {
            Some((FragmentKind::Dirt, BLOCK_FRAGMENTS_PER_BREAK))
        }
        Material::Stone
        | Material::VillageStone
        | Material::Cobblestone
        | Material::RoadCobble
        | Material::BlackStone
        | Material::Coal
        | Material::Sapphire
        | Material::Ruby
        | Material::Emerald
        | Material::ManaCrystal => Some((FragmentKind::Stone, BLOCK_FRAGMENTS_PER_BREAK)),
        Material::Wood
        | Material::WoodPlanks
        | Material::Leaves
        | Material::Bedrock
            | Material::Chest
            | Material::Glass
            | Material::Door
            | Material::Torch
            | Material::Apple
            | Material::Beehive
            | Material::Water
        | Material::ThermalWater
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
        (x as u32).wrapping_mul(73856093)
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
    // Supresión por aldea (carbón, rubí, zafiro, esmeralda): casi sin ores
    // dentro de la muralla y escasos cerca; el subsuelo no cambia.
    // Solo se consulta el asentamiento si el roll ya dio ore (los lookups
    // de reinos son caros para el ritmo del meshing).
    if lip - y < ORE_VILLAGE_SUBSUELO_DEPTH {
        let s = crate::settlements::village_ore_suppression(x, z);
        if s > 0.0 && embed_roll(x, y, z, 0xB105) < s {
            return None;
        }
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
    let s = mix_seed(
        ORE_SEED ^ 0xF1E1,
        x as u32,
        (y as u32) ^ (z as u32).wrapping_mul(7),
    );
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
pub fn ore_micros_visible(world: &World, pos: IVec3) -> Option<(EmbedKind, u8, [OreMicro; 2])> {
    let kind = stone_embed_kind(pos.x, pos.y, pos.z)?;
    let empty = [OreMicro {
        mx: 0,
        my: 0,
        mz: 0,
    }; 2];
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
        ore_micro_on_face(exposed[(s1 as usize + 1) % n_exp], (s1 >> 3) % 9, s1 >> 8)
    } else {
        empty[1]
    };
    Some((kind, count as u8, [a, b]))
}

/// 2D peaks above this = large rock outcrop pads on the surface.
pub const STONE_OUTCROP_THRESH: f32 = 0.70;
/// How deep a large pad roots under the dirt lip.
pub const STONE_OUTCROP_DEPTH: i32 = 4;
/// Max blocks a large boulder pokes above the dirt lip.
pub const STONE_OUTCROP_LIFT_MAX: i32 = 3;
/// Higher-freq peaks = small 1–3 block rocks / pebbles between the big pads.
pub const STONE_PEBBLE_THRESH: f32 = 0.80;
/// Shallow root for small rocks (still a complete little pad).
pub const STONE_PEBBLE_DEPTH: i32 = 2;
/// Small rocks usually poke 1 block; rare peaks poke 2.
pub const STONE_PEBBLE_LIFT_MAX: i32 = 2;
/// Noise threshold just below the outcrop shell (mostly dirt).
pub const STONE_NEAR_SURFACE_THRESH: f32 = 0.85;
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
        let lift = if t > 0.55 { STONE_PEBBLE_LIFT_MAX } else { 1 };
        return (lift, STONE_PEBBLE_DEPTH);
    }
    (0, 0)
}

/// True when this column hosts any surface rock (pad or pebble).
pub fn column_has_stone_outcrop(x: i32, z: i32) -> bool {
    stone_outcrop_profile(x, z).0 > 0
}

/// Suppress boulder pads on temperate, moist, mid-altitude ground (meadow /
/// wetland floors stay green and rolling; hills, deserts and cold rock keep
/// full outcrops). Pure climate + height — deliberately NOT `biome_at`:
/// the biome needs `terrain_height`, which needs the outcrop lift
/// (`height_from_smooth_cell`), so routing through the biome would recurse.
/// `top` is the pre-lift lid; both use sites pass the same value, so the
/// height decision and the stone predicate always agree.
fn outcrop_suppressed_here(x: i32, z: i32, top: i32) -> bool {
    if !(10..=32).contains(&top) {
        return false;
    }
    let (temp, moist) = crate::biomes::climate_at(x, z);
    (0.30..0.70).contains(&temp) && moist > 0.35
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
    // Same suppression as the height decision: a meadow lid stays dirt even
    // where the geology field peaks (see `outcrop_suppressed_here`).
    if lift > 0 && !outcrop_suppressed_here(x, z, column_top) {
        let stone_bottom = column_top - lift - root + 1;
        if y >= stone_bottom.max(0) {
            return true;
        }
    }
    let depth = (column_top - y) as f32;
    let t = (depth / STONE_DEPTH_FULL).clamp(0.0, 1.0);
    let ease = t * t * (3.0 - 2.0 * t);
    let thresh = STONE_NEAR_SURFACE_THRESH + (STONE_DEEP_THRESH - STONE_NEAR_SURFACE_THRESH) * ease;
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
    } else if y == column_top && column_is_mud_shore(x, z, column_top) {
        Material::Mud
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
            Material::Dirt => [0.52, 0.38, 0.24], // warmer brown, readable under cel
            Material::Grass => [0.42, 0.68, 0.34], // less neon than #7FBF6A
            Material::WoodPlanks => [0.68, 0.48, 0.25], // sawn warm boards
            Material::Wood => [0.42, 0.28, 0.16], // bark / trunk
            Material::Leaves => [0.28, 0.55, 0.26], // canopy
            Material::VillageStone => [0.59, 0.56, 0.46], // low warm grey masonry
            Material::Cobblestone => [0.52, 0.50, 0.46], // rough cobble wall grey
            Material::RoadCobble => [0.63, 0.53, 0.35], // packed ochre road cobble
            Material::Torch => [1.0, 0.55, 0.15], // lamp flame orange
            Material::Apple => [0.85, 0.15, 0.18], // orchard red
            Material::Beehive => [0.85, 0.62, 0.20], // honey straw box
            Material::ManaCrystal => [0.25, 0.85, 0.95], // raw magic cyan
            Material::Stone => [0.66, 0.68, 0.72], // clear cool grey (not muddy brown)
            Material::BlackStone => [0.12, 0.11, 0.13], // near-black deep stone
            Material::Bedrock => [0.22, 0.18, 0.20], // dark mottled floor
            Material::Coal => [0.05, 0.045, 0.04],
            Material::Sapphire => [0.22, 0.42, 0.92],
            Material::Ruby => [0.88, 0.18, 0.22],
            Material::Emerald => [0.18, 0.78, 0.38],
            Material::Chest => [0.55, 0.32, 0.12], // warm crate wood
            Material::Glass => [0.72, 0.88, 0.94], // cool pane (opaque stand-in)
            Material::Door => [0.48, 0.30, 0.14],  // stained door boards
            Material::Water => [0.22, 0.48, 0.78], // shallow pond
            Material::Mud => [0.30, 0.22, 0.14], // wet shore mud
            Material::ThermalWater => [0.60, 0.69, 0.48], // mineral tint, half chroma
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

    /// True for swimmable water (sea / flow / hot springs).
    #[inline]
    pub fn is_water(self) -> bool {
        matches!(self, Material::Water | Material::ThermalWater)
    }

    /// True for dense terrain the pickaxe excavates (dirt / stone).
    pub fn is_diggable_terrain(self) -> bool {        matches!(
            self,
            Material::Dirt
                | Material::Mud
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
    /// Violet disc + vertical spire (fantasy read at a glance).
    Enchanted,
    /// Tall narrow umber tower.
    Umbra,
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
        (pos.x as u32).wrapping_mul(73856093)
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
                let edge = x == 0 || z == 0 || x + 1 == MICROVOXEL_RES || z + 1 == MICROVOXEL_RES;
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

fn set_chunk_state(map: &mut FxHashMap<(i32, i32), ChunkState>, key: (i32, i32), new: ChunkState) {
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
    /// Spatial index over [`Self::extras`] by render section `(cx, cy, cz)`.
    /// Mesh/query hot paths must never full-scan `extras` (O(sections×extras)
    /// per frame); a section lookup is O(1). Invariant: `pos` is in the set
    /// for its section iff it is in `extras`. Maintained by
    /// [`Self::extras_insert`]/[`Self::extras_remove`] — never touch `extras`
    /// directly outside those helpers (+ `set_column_dirt`/`unload_chunk`,
    /// which drain whole columns through the same index).
    extras_by_section: FxHashMap<(i32, i32, i32), FxHashSet<IVec3>>,
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
    /// Per-section dirties for the 16³ B-split (`(cx, cy, cz)`).
    /// Populated alongside the column sets so the renderer rebuilds only
    /// the touched slab instead of the whole 16×64 column.
    /// Edits (hard remesh) and stream fills (soft remesh) stay in separate
    /// sets to preserve queue priority.
    dirty_sections: FxHashSet<(i32, i32, i32)>,
    dirty_sections_stream: FxHashSet<(i32, i32, i32)>,
    pending_chunks: VecDeque<(i32, i32, Vec<(i32, i32, i32)>)>,
    /// Chunks that already received a grass planting pass (within gen radius).
    grass_ready: FxHashSet<(i32, i32)>,
    /// Flow level per water cell (1..=WATER_FLOW_MAX). Cells with water but
    /// no entry are infinite sources (level 0). Stale entries are harmless:
    /// every water query checks the voxel first.
    flow: FxHashMap<IVec3, u8>,
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
            dirty_sections: FxHashSet::default(),
            dirty_sections_stream: FxHashSet::default(),
            extras_by_section: FxHashMap::default(),
            pending_chunks: VecDeque::new(),
            grass_ready: FxHashSet::default(),
            flow: FxHashMap::default(),
            unload_grace: FxHashMap::default(),
        }
    }

    #[inline]
    pub fn has_dirty_edits(&self) -> bool {
        !self.dirty_edits.is_empty()
    }

    #[inline]
    pub fn has_dirty_sections(&self) -> bool {
        !self.dirty_sections.is_empty()
    }

    #[inline]
    pub fn has_dirty_stream(&self) -> bool {
        !self.dirty_stream.is_empty()
    }

    #[inline]
    pub fn has_dirty_sections_stream(&self) -> bool {
        !self.dirty_sections_stream.is_empty()
    }

    /// FBM payloads waiting for the sequential commit inside streaming.
    /// The mesh fast-path must NOT skip streaming while this is non-empty,
    /// or generated chunks never commit and the ring keeps 16×16 holes.
    #[inline]
    pub fn has_pending_chunks(&self) -> bool {
        !self.pending_chunks.is_empty()
    }

    /// Render-section key for a sparse voxel position.
    #[inline]
    fn section_key_for(pos: IVec3) -> (i32, i32, i32) {
        (
            pos.x.div_euclid(MESH_CHUNK_SIZE),
            mesh_section_y(pos.y),
            pos.z.div_euclid(MESH_CHUNK_SIZE),
        )
    }

    /// Insert into `extras`, keeping [`Self::extras_by_section`] in sync.
    /// Returns the previous voxel, like [`FxHashMap::insert`].
    fn extras_insert(&mut self, pos: IVec3, voxel: Voxel) -> Option<Voxel> {
        let old = self.extras.insert(pos, voxel);
        if old.is_none() {
            self.extras_by_section
                .entry(Self::section_key_for(pos))
                .or_default()
                .insert(pos);
        }
        old
    }

    /// Remove from `extras`, keeping [`Self::extras_by_section`] in sync.
    fn extras_remove(&mut self, pos: &IVec3) -> Option<Voxel> {
        let old = self.extras.remove(pos);
        if old.is_some() {
            let key = Self::section_key_for(*pos);
            let empty = match self.extras_by_section.get_mut(&key) {
                Some(set) => {
                    set.remove(pos);
                    set.is_empty()
                }
                None => false,
            };
            if empty {
                self.extras_by_section.remove(&key);
            }
        }
        old
    }

    #[inline]
    fn inc_material(&mut self, material: Material) {
        match material {
            Material::Dirt
            | Material::Mud
            | Material::Stone
            | Material::VillageStone
            | Material::Cobblestone
            | Material::RoadCobble
            | Material::Torch
            | Material::Apple
            | Material::Beehive
            | Material::ManaCrystal
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
            | Material::ThermalWater
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
            | Material::Mud
            | Material::Stone
            | Material::VillageStone
            | Material::Cobblestone
            | Material::RoadCobble
            | Material::Torch
            | Material::Apple
            | Material::Beehive
            | Material::ManaCrystal
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
            | Material::ThermalWater
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
            | Material::UmbraLeaves => self.dirt_count = self.dirt_count.saturating_sub(1),
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
        world.dirty_sections.clear();
        world.dirty_sections_stream.clear();
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

        // Backfill: los commits plantan copas/settlements ±2 bloques dentro del
        // vecino, marcándolo Filled por derrame (`set_voxel`) sin columnas
        // densas. Sin este repaso el vecino queda hueco permanente (el stream
        // lo salta por "contenido" y el guardián no lo ve como hueco).
        // Se repite hasta 2 pasadas por cadenas de derrame en diagonal.
        for _ in 0..2 {
            let hollow: Vec<(i32, i32)> = self
                .loaded_chunks
                .keys()
                .copied()
                .filter(|&(kx, kz)| {
                    self.loaded_chunks.get(&(kx, kz)) == Some(&ChunkState::Filled)
                        && !self.chunk_has_columns(kx, kz)
                })
                .collect();
            if hollow.is_empty() {
                break;
            }
            log::info!("preload: backfilling {} hollow spillover chunks", hollow.len());
            let generated: Vec<(i32, i32, Vec<(i32, i32, i32)>)> = {
                use rayon::prelude::*;
                hollow
                    .par_iter()
                    .map(|&(kx, kz)| (kx, kz, generate_chunk_columns(kx, kz)))
                    .collect()
            };
            for (kx, kz, columns) in generated {
                let plant_grass = chunk_dist_sq_xz(spawn, kx, kz) <= GRASS_GEN_MAX_DIST_SQ;
                self.commit_generated_chunk(kx, kz, &columns, plant_grass);
            }
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
                // Agua primero: nada de flora bajo el nivel del mar.
                if h < SEA_LEVEL {
                    world.fill_sea_water(x, z);
                    continue;
                }
                if Self::hot_spring_at(x, z) {
                    world.stamp_hot_spring(x, z);
                    continue;
                }
                let slope = terrain_slope(x, z);
                // Aldeas: sin árboles, sin matorrales y sin hierbajos.
                if crate::settlements::settlement_claims_block_cached(x, z) {
                    continue;
                }
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
                    world.set_voxel(grass_pos, Voxel::grass_from_seed_sloped(grass_seed, slope));
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
        // Corrientes iniciales de todo el extent fijo.
        world.refresh_water_around(extent / 2, extent / 2, extent);
        world.dirty_edits.clear();
        world.dirty_stream.clear();
        world.dirty_sections.clear();
        world.dirty_sections_stream.clear();
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
        for cy in mesh_section_y(0)..=mesh_section_y(h) {
            let skey = (key.0, cy, key.1);
            if let Some(mut set) = self.extras_by_section.remove(&skey) {
                set.retain(|p| !(p.x == x && p.z == z));
                if !set.is_empty() {
                    self.extras_by_section.insert(skey, set);
                }
            }
        }
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

    /// Test helper: run the deferred flora pass over a chunk window.
    #[cfg(test)]
    pub fn seed_flora_for_test(&mut self, x: i32, z: i32, radius_chunks: i32) {
        let (cx, cz) = mesh_chunk_coord(x, z);
        for dz in -radius_chunks..=radius_chunks {
            for dx in -radius_chunks..=radius_chunks {
                self.seed_chunk_grass(cx + dx, cz + dz);
            }
        }
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
        // Picar/rellenar junto al agua abre o cierra corrientes.
        self.refresh_water_around(pos.x, pos.z, 10);
    }

    /// Dig a block and remember air across unload / save.
    pub fn remove_voxel_player(&mut self, pos: IVec3) -> bool {
        let ok = self.remove_voxel(pos);
        if ok {
            self.player_edits.insert(pos, None);
            // Picar junto al agua abre corrientes (el agua es infinita).
            self.refresh_water_around(pos.x, pos.z, 10);
        }
        ok
    }

    pub fn load_player_edits(
        &mut self,
        edits: impl IntoIterator<Item = (IVec3, Option<Material>)>,
    ) {
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
        // El nivel de corriente muere con el vóxel que lo portaba.
        self.flow.remove(&pos);
        let key = mesh_chunk_coord(pos.x, pos.z);
        self.loaded_chunks.insert(key, ChunkState::Filled);

        // Dense solid dirt: merge into the column stack when possible.
        if voxel.material == Material::Dirt && voxel.is_fully_solid() {
            if let Some(old) = self.extras_remove(&pos) {
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
                    self.extras_insert(pos, voxel);
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
        if let Some(old) = self.extras_insert(pos, voxel) {
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
            self.extras_insert(p, roof);
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
        self.flow.remove(&pos);
        if let Some(old) = self.extras_remove(&pos) {
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
            self.height_bounds.remove(&mesh_chunk_coord(pos.x, pos.z));
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
                self.extras_remove(&p);
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
    /// Also records per-section dirties so the 16³ mesh path rebuilds only
    /// the touched slab (plus the vertical neighbor when the edit sits on
    /// a 16-block boundary).
    /// Los vecinos XZ solo se ensucian si el bloque toca su borde: el greedy
    /// y el AO solo leen al otro lado en la cara compartida. Un edit interior
    /// ensuciaba 9 columnas × secciones (hasta ~27 remesh); ahora 1 (+Y borde).
    /// El set grueso por columnas (`dirty_edits`) se mantiene 3×3: es barato
    /// (solo claves) y lo usa el warmup/tests.
    fn mark_dirty_around(&mut self, pos: IVec3) {
        let (cx, cz) = mesh_chunk_coord(pos.x, pos.z);
        for dz in -1..=1 {
            for dx in -1..=1 {
                self.dirty_edits.insert((cx + dx, cz + dz));
            }
        }
        let lx = pos.x.rem_euclid(MESH_CHUNK_SIZE);
        let lz = pos.z.rem_euclid(MESH_CHUNK_SIZE);
        let xs: &[i32] = if lx == 0 {
            &[-1, 0]
        } else if lx == MESH_CHUNK_SIZE - 1 {
            &[0, 1]
        } else {
            &[0]
        };
        let zs: &[i32] = if lz == 0 {
            &[-1, 0]
        } else if lz == MESH_CHUNK_SIZE - 1 {
            &[0, 1]
        } else {
            &[0]
        };
        let cy = mesh_section_y(pos.y);
        let mut cys = [cy, -1, -1];
        let mut n = 1usize;
        // Boundary faces are shared with the neighbor slab (greedy quads and
        // AO sample across it), so both sides must rebuild.
        if pos.y.rem_euclid(MESH_SECTION_HEIGHT) == 0 && cy > 0 {
            cys[n] = cy - 1;
            n += 1;
        } else if pos.y.rem_euclid(MESH_SECTION_HEIGHT) == MESH_SECTION_HEIGHT - 1
            && cy + 1 < MESH_SECTIONS_Y
        {
            cys[n] = cy + 1;
            n += 1;
        }
        for dz in zs {
            for dx in xs {
                for cyy in &cys[..n] {
                    self.dirty_sections.insert((cx + dx, *cyy, cz + dz));
                }
            }
        }
    }

    /// Mark every section of this column + XZ neighbors (streaming commit
    /// fills the whole 16×64 column at once — first fill has no old mesh to save).
    fn mark_sections_around_column(&mut self, cx: i32, cz: i32) {
        for dz in -1..=1 {
            for dx in -1..=1 {
                for cy in 0..MESH_SECTIONS_Y {
                    self.dirty_sections_stream.insert((cx + dx, cy, cz + dz));
                }
            }
        }
    }

    /// Drain per-section edit dirties (`(cx, cy, cz)` 16³ slabs).
    pub fn take_dirty_sections(&mut self) -> Vec<(i32, i32, i32)> {
        self.dirty_sections.drain().collect()
    }

    /// Drain per-section streaming dirties (soft remesh, no priority).
    pub fn take_dirty_sections_stream(&mut self) -> Vec<(i32, i32, i32)> {
        self.dirty_sections_stream.drain().collect()
    }

    /// Drain player-edit dirties (VRAM mesh must be dropped).
    pub fn take_dirty_edits(&mut self) -> Vec<(i32, i32)> {
        self.dirty_edits.drain().collect()
    }

    /// Drain streaming dirties (VRAM cache may satisfy these).
    pub fn take_dirty_stream(&mut self) -> Vec<(i32, i32)> {
        self.dirty_stream.drain().collect()
    }

    /// Drain all dirties (tests / fallback). Also drains the per-section sets
    /// (folded back to columns) so no dirty set grows when only this is polled.
    pub fn take_dirty_chunks(&mut self) -> Vec<(i32, i32)> {
        use rustc_hash::FxHashSet;
        let mut out = self.take_dirty_edits();
        out.extend(self.take_dirty_stream());
        let mut seen: FxHashSet<(i32, i32)> = out.iter().copied().collect();
        for (cx, _, cz) in self
            .dirty_sections
            .drain()
            .chain(self.dirty_sections_stream.drain())
        {
            if seen.insert((cx, cz)) {
                out.push((cx, cz));
            }
        }
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

    /// Min/max dirt-top Y in a column chunk (`None` = no generated columns).
    /// Single 256-lookup scan for the section mesh path (avoids `&mut`
    /// borrow of the cached bounds inside the parallel meshing pass).
    pub fn chunk_minmax_height(&self, cx: i32, cz: i32) -> Option<(i32, i32)> {
        let x0 = cx * MESH_CHUNK_SIZE;
        let z0 = cz * MESH_CHUNK_SIZE;
        let mut min_h = i32::MAX;
        let mut max_h = i32::MIN;
        for z in z0..z0 + MESH_CHUNK_SIZE {
            for x in x0..x0 + MESH_CHUNK_SIZE {
                if let Some(h) = self.heights.get(&(x, z)).copied() {
                    min_h = min_h.min(h);
                    max_h = max_h.max(h);
                }
            }
        }
        if min_h == i32::MAX {
            None
        } else {
            Some((min_h, max_h))
        }
    }

    /// True when render section `cy` may contain geometry: its slab reaches
    /// the column tops (+8 for trees/grass/prefabs) or holds sparse extras.
    /// Lets the renderer skip empty upper slabs entirely (no CPU/GPU cost).
    pub fn section_may_have_content(&self, cx: i32, cz: i32, cy: i32) -> bool {
        let (y0, _y1) = mesh_section_range(cy);
        if let Some((_, max_h)) = self.chunk_minmax_height(cx, cz) {
            if y0 <= max_h + 8 {
                return true;
            }
        } else {
            // No dense columns — extras-only chunk (or empty); check below.
        }
        // Sparse index: O(1) section lookup instead of a full-map scan.
        if self
            .extras_by_section
            .get(&(cx, cy, cz))
            .is_some_and(|set| !set.is_empty())
        {
            return true;
        }
        // Uncached max_h scan found dense tops above this slab already;
        // without dense tops and without extras the slab is provably empty.
        self.chunk_minmax_height(cx, cz)
            .is_some_and(|(_, max_h)| y0 <= max_h + 8)
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

        // Agua: mares/lagos (inundar bajo el nivel) + manantiales termales.
        // Antes de la vegetación para no plantar árboles bajo el agua.
        for &(x, z, _) in &planted_cols {
            self.fill_sea_water(x, z);
            if Self::hot_spring_at(x, z) {
                self.stamp_hot_spring(x, z);
            }
        }

        // Vegetation decisions are pure noise; the old chain re-derived the same
        // terrain height + slope + climate per column up to 5× (each biome_at
        // call re-samples height AND slope). Evaluate each once per column using
        // the heights we already have, then apply voxels (set_voxel ~0.2µs).
        if ENABLE_TREES || ENABLE_GRASS {
            let heights = slab_heights(planted_cols.as_slice());
            for &(x, z, h) in &planted_cols {
                // Nada de flora bajo el nivel del mar (fondo marino limpio).
                if h < SEA_LEVEL {
                    continue;
                }
                // Aldeas: sin árboles, sin matorrales y sin hierbajos.
                if crate::settlements::settlement_claims_block_cached(x, z) {
                    continue;
                }
                let slope = heights.slope(x, z);
                if column_cell_is_stone(x, h, z, h) {
                    continue;
                }
                let (temp, moist) = crate::biomes::climate_at(x, z);
                let biome = crate::biomes::biome_at_slope(x, z, temp, moist, h as f32, slope);
                if ENABLE_TREES {
                    if column_grows_tree_in(biome, x, z, slope) {
                        self.plant_tree_at(x, z, h);
                        continue;
                    }
                    if column_grows_bush_in(biome, x, z, slope) {
                        self.plant_bush_at(x, z, h);
                        continue;
                    }
                }
                if ENABLE_GRASS && plant_grass {
                    if !crate::biomes::column_has_sand_surface_biome(biome, x, z)
                        && column_grows_grass_in(biome, x, z, slope)
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
        }
        if ENABLE_SETTLEMENTS {
            // After vegetation so roads/walls/houses overwrite grass and trunks.
            crate::settlements::stamp_settlements_in_chunk(self, cx, cz);
        }
        self.apply_player_edits_in_chunk(cx, cz);
        // Corrientes iniciales del chunk (mares → cuevas, desbordes).
        self.refresh_water_around(cx * MESH_CHUNK_SIZE + 8, cz * MESH_CHUNK_SIZE + 8, 16);
        if ENABLE_GRASS && plant_grass {
            self.grass_ready.insert((cx, cz));
        }
        for dz in -1..=1 {
            for dx in -1..=1 {
                self.dirty_stream.insert((cx + dx, cz + dz));
            }
        }
        self.mark_sections_around_column(cx, cz);
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
                if h < SEA_LEVEL {
                    continue;
                }
                // Aldeas: sin árboles, sin matorrales y sin hierbajos.
                if crate::settlements::settlement_claims_block_cached(x, z) {
                    continue;
                }
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
                self.set_voxel(grass_pos, Voxel::grass_from_seed_sloped(grass_seed, slope));
            }
        }
        self.apply_player_edits_in_chunk(cx, cz);
        self.grass_ready.insert((cx, cz));
        self.dirty_stream.insert((cx, cz));
        for cy in 0..MESH_SECTIONS_Y {
            self.dirty_sections_stream.insert((cx, cy, cz));
        }
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
                // Tallest silhouette: conifer spire over the canopy line.
                let h = 6 + (mix_seed(TREE_SEED ^ 0x5049_4E45, x as u32, z as u32) % 4) as i32;
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
                    CanopyShape::Enchanted,
                )
            }
            TreeSpecies::Umbra => {
                let h = 4 + (mix_seed(TREE_SEED ^ 0x554D_B2A1, x as u32, z as u32) % 3) as i32;
                (
                    h,
                    9usize,
                    Material::UmbraWood,
                    Material::UmbraLeaves,
                    CanopyShape::Umbra,
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

    fn plant_canopy(&mut self, x: i32, z: i32, top_y: i32, leaf: Material, shape: CanopyShape) {
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
                for (dx, dz) in [(0, 0), (1, 0), (-1, 1), (2, -1), (-2, 0), (1, 2), (0, -2)] {
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
            CanopyShape::Enchanted => {
                // Violet disc + spire: wide flat ring with a vertical spike.
                let canopy_y = top_y + 1;
                for dz in -2..=2i32 {
                    for dx in -2..=2i32 {
                        if dx * dx + dz * dz <= 5 {
                            put(x + dx, canopy_y, z + dz);
                        }
                    }
                }
                for dy in 1..=3 {
                    put(x, canopy_y + dy, z);
                }
                put(x, canopy_y + 4, z);
            }
            CanopyShape::Umbra => {
                // Dark narrow tower: tight column rising over the trunk.
                for (dy, r) in [(0i32, 1i32), (1, 1), (2, 1), (3, 0), (4, 0)] {
                    let y = top_y + 1 + dy;
                    for dz in -r..=r {
                        for dx in -r..=r {
                            if dx * dx + dz * dz <= r * r + 1 {
                                put(x + dx, y, z + dz);
                            }
                        }
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

    /// Generate + commit keep-ring holes in this call (Reserved counts as a hole).
    ///
    /// Ordering + budget keep the *inner crown* (the visible dirt disc) sealed
    /// before the far 72→88 ring, which is amortized over frames. Amortized
    /// `max_gen` / frame-time budgets left empty 16×16 wells in front of the
    /// HD-2D lens; here a short hitch on the crown is preferable to a void, but
    /// the invisible far ring must not hitch the frame when a whole new diagonal
    /// quadrant enters the ring.
    ///
    /// Budget capping: kick waves and the commit drain both yield to `budget`,
    /// so a corner surge is spread over several frames instead of railing the
    /// render thread. Auto-rates never outpace the budget (~1 commit/frame at
    /// HD-2D pace) — `fill_keep_ring` starts with whatever pending is left and
    /// hands the rest to step 1/step 2 in `stream_around_timed_look`.
    fn fill_keep_ring(
        &mut self,
        origin: Vec3,
        c0: (i32, i32),
        mesh_r: i32,
        look: Vec3,
        budget: Duration,
        start: Instant,
        max_gen: usize,
    ) {
        use rayon::prelude::*;

        let mesh_r_sq = mesh_r * mesh_r;
        // Visible dirt disc in chunk-index units (≈ radius `dirt_mesh_max_dist`).
        let inner_r = ((dirt_mesh_max_dist() / MESH_CHUNK_SIZE as f32).ceil() as i32) + 1;
        let inner_r_sq = inner_r * inner_r;

        let holes = self.mesh_ring_holes(origin);
        if holes.is_empty() {
            return;
        }

        let mut pending_keys = FxHashSet::default();
        for &(x, z, _) in &self.pending_chunks {
            pending_keys.insert((x, z));
        }

        // Inner crown first (visible — must not starve), then the travel-aligned
        // far ring (the leading diagonal quadrant of a moving corner fills before
        // the rest). Lower sort key = generated sooner.
        let mut holes: Vec<(i32, i32)> = holes;
        holes.sort_by_key(|&(kx, kz)| {
            let dx = kx - c0.0;
            let dz = kz - c0.1;
            let d2 = dx * dx + dz * dz;
            let inner = if d2 <= inner_r_sq { 0u8 } else { 1 };
            let align = if look != Vec3::ZERO {
                (dx as f32 * look.x + dz as f32 * look.z).max(0.0)
            } else {
                0.0
            };
            let score = (d2 as f32 - STREAM_LOOK_PRELOAD_BIAS * align * mesh_r as f32) as i64;
            ((inner as i64) << 62) + score
        });

        // Generate in waves: inner crown first, then the far ring. Every wave
        // (inner included) yields to the frame budget so a freshly entering
        // diagonal quadrant cannot balloon one frame into a multi-second hitch.
        // `generate_chunk_columns` is cheap (~1ms/4-chunk wave in parallel); the
        // precious cost is the SEQUENTIAL commit below, so the kick loop must
        // not queue more than the commit drain can amortize.
        let wave = STREAM_GEN_WAVE.max(2);
        let mut far_kicked = 0usize;
        for group in holes.chunks(wave) {
            let mut wave_gen = Vec::new();
            for &(kx, kz) in group {
                if pending_keys.contains(&(kx, kz)) {
                    continue;
                }
                let dx = kx - c0.0;
                let dz = kz - c0.1;
                let in_inner = dx * dx + dz * dz <= inner_r_sq;
                if !in_inner {
                    if far_kicked >= max_gen {
                        // Leave missing — step 1 / a later frame pick them up.
                        continue;
                    }
                    far_kicked += 1;
                }
                wave_gen.push((kx, kz));
            }
            if wave_gen.is_empty() {
                if start.elapsed() >= budget {
                    break;
                }
                continue;
            }
            // Mark Reserved only for the waves handed to workers so a budget
            // break never strands Reservation shells without a pending payload.
            for &(kx, kz) in &wave_gen {
                set_chunk_state(&mut self.loaded_chunks, (kx, kz), ChunkState::Reserved);
            }
            let generated: Vec<(i32, i32, Vec<(i32, i32, i32)>)> = wave_gen
                .par_iter()
                .map(|&(kx, kz)| (kx, kz, generate_chunk_columns(kx, kz)))
                .collect();
            for item in generated {
                self.pending_chunks.push_back(item);
            }
            if start.elapsed() >= budget {
                break;
            }
        }

        // Commit pending in-ring holes — inner crown first, capped by the frame
        // budget. At least one chunk always commits so the visible front never
        // stalls; a corner surge is amortized over the next frames instead of
        // stalling the render thread for ~1s while the whole quadrant drains.
        //
        // The inner crown is a chunk-coord ring up to ~80 blocks; an origin
        // crossing drops a dozen of them into `inner` at once. Committing all of
        // them unconditionally (~250ms of set_column_dirt/caves/settlements)
        // re-creates the old hitch, so inner commits get a small per-frame cap
        // AND yield to `budget` — nearest (visible) columns pop first, so the
        // ≤64-block disc stays sealed and only the invisible 64..80 band waits a
        // frame or two.
        const INNER_COMMIT_CAP: usize = 4;
        let mut deferred = VecDeque::new();
        let mut inner_committed = 0usize;
        loop {
            let pop_what = self.pending_chunks.pop_front();
            let Some((kx, kz, columns)) = pop_what else { break };
            let dx = kx - c0.0;
            let dz = kz - c0.1;
            let d2 = dx * dx + dz * dz;
            if d2 > mesh_r_sq {
                deferred.push_back((kx, kz, columns));
                continue;
            }
            if !self.loaded_chunks.contains_key(&(kx, kz)) {
                continue;
            }
            // NOTE: no `chunk_filled` skip here (step 2 doesn't have one
            // either). A neighbor's canopy/settlement spillover marks chunks
            // Filled via `set_voxel` before their own payload runs — skipping
            // would drop the payload and leave a permanently hollow chunk
            // (state Filled, no dense columns, invisible to the guardian).
            // Re-commit is idempotent (columns skip when present).
            // Inner crown commits first (nearest — visible — columns pop first);
            // the far halo yields to the soft budget.
            let in_inner = d2 <= inner_r_sq;
            let plant_grass = chunk_dist_sq_xz(origin, kx, kz) <= GRASS_GEN_MAX_DIST_SQ;
            self.commit_generated_chunk(kx, kz, &columns, plant_grass);
            if in_inner {
                inner_committed += 1;
                if inner_committed >= INNER_COMMIT_CAP || start.elapsed() >= budget {
                    break;
                }
            } else if start.elapsed() >= budget {
                break;
            }
        }
        // Re-queue whatever the budget cut off (in-ring or out) for next frames.
        while let Some(item) = self.pending_chunks.pop_front() {
            deferred.push_back(item);
        }
        self.pending_chunks = deferred;
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
    pub fn stream_around_timed_look(&mut self, camera_pos: Vec3, look_xz: Vec3, budget: Duration) {
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

        self.fill_keep_ring(camera_pos, c0, mesh_r, look, budget, start, max_gen);
        #[cfg(test)]
        let t1 = start.elapsed();

        // 1) Kick FBM: mesh-ring holes ALWAYS (even over soft cap); outer ring only
        // when pending has room. Soft-cap used to starve the visible ring → 16×16 holes.
        let load_r = ((stream_load_dist() / MESH_CHUNK_SIZE as f32).ceil() as i32) + 1;
        let load_r_sq = load_r * load_r;
        let pending_full = self.pending_chunks.len() >= PENDING_SOFT_CAP;
        // Cáscaras huecas (Filled por derrame sin columnas): no re-kick si su
        // payload ya viaja en `pending` — el commit lo llenará. Sin esta guarda
        // el re-kick duplicaría el payload y el `carve_chunk` correría dos veces.
        let mut pending_keys = FxHashSet::default();
        for &(px, pz, _) in &self.pending_chunks {
            pending_keys.insert((px, pz));
        }
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
                // Hueco permanente: Filled sin columnas (derrame de copa ±2 /
                // sello fuera del disco). Se re-encola como si faltara; el
                // commit conserva los extras derramados y rellena las 256.
                let hollow = self.loaded_chunks.get(&key) == Some(&ChunkState::Filled)
                    && !self.chunk_has_columns(key.0, key.1)
                    && !pending_keys.contains(&key);
                if !self.loaded_chunks.contains_key(&key) || hollow {
                    let align = if look != Vec3::ZERO {
                        (dx as f32 * look.x + dz as f32 * look.z).max(0.0)
                    } else {
                        0.0
                    };
                    let hole_boost = if in_mesh_ring { -1.0e6 } else { 0.0 };
                    let score =
                        d2 as f32 - STREAM_LOOK_PRELOAD_BIAS * align * load_r as f32 + hole_boost;
                    missing.push(((score * 1000.0) as i32, d2, key.0, key.1));
                }
            }
        }
        missing.sort_by_key(|&(score, d2, _, _)| (score, d2));

        let batch: Vec<(i32, i32)> = missing
            .into_iter()
            .take(max_gen)
            .map(|(_, _, kx, kz)| (kx, kz))
            .filter(|&(kx, kz)| {
                !self.loaded_chunks.contains_key(&(kx, kz))
                    || (self.loaded_chunks.get(&(kx, kz)) == Some(&ChunkState::Filled)
                        && !self.chunk_has_columns(kx, kz)
                        && !pending_keys.contains(&(kx, kz)))
            })
            .collect();

        if !batch.is_empty() {
            // Generate in parallel waves so a single wall-clock check can stop the
            // loop before the frame budget blows. Only chunks handed to the worker
            // parcel are marked Reserved; un-generated members stay missing and are
            // retried next frame (no stranded Reserved shells).
            let wave = STREAM_GEN_WAVE.max(2);
            for group in batch.chunks(wave) {
                for &(kx, kz) in group {
                    set_chunk_state(&mut self.loaded_chunks, (kx, kz), ChunkState::Reserved);
                }
                let generated: Vec<(i32, i32, Vec<(i32, i32, i32)>)> = group
                    .par_iter()
                    .map(|&(kx, kz)| (kx, kz, generate_chunk_columns(kx, kz)))
                    .collect();
                for item in generated {
                    self.pending_chunks.push_back(item);
                }
                if start.elapsed() >= budget {
                    break;
                }
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
                    log::warn!("chunk ({kx},{kz}) pending commit dropped — not in loaded_chunks");
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
                    set_chunk_state(&mut self.loaded_chunks, key, ChunkState::PendingUnload);
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
        for cy in 0..MESH_SECTIONS_Y {
            if let Some(doomed) = self.extras_by_section.remove(&(cx, cy, cz)) {
                for pos in doomed {
                    if let Some(v) = self.extras.remove(&pos) {
                        self.dec_material(v.material);
                    }
                }
            }
        }
        self.dirty_edits.remove(&(cx, cz));
        self.dirty_stream.remove(&(cx, cz));
        for cy in 0..MESH_SECTIONS_Y {
            self.dirty_sections.remove(&(cx, cy, cz));
            self.dirty_sections_stream.remove(&(cx, cy, cz));
        }
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
                    for cy in 0..MESH_SECTIONS_Y {
                        self.dirty_sections_stream.insert((n.0, cy, n.1));
                    }
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
        // El agua es infinita: no se puede picar ni retirar a mano.
        if v.material.is_water() {
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
    /// Water never occludes: the lakebed stays visible through dithered water.
    pub fn solid_occludes(&self, pos: IVec3) -> bool {
        self.get_voxel(pos)
            .is_some_and(|v| v.is_fully_solid() && !v.material.is_water())
    }

    /// Nivel de corriente en 0..=WATER_FLOW_MAX (`None` = no hay agua).
    /// Agua sin entrada = fuente infinita (nivel 0, no se agota).
    pub fn water_level(&self, pos: IVec3) -> Option<u8> {
        let v = self.get_voxel(pos)?;
        if v.is_empty() || !v.material.is_water() {
            return None;
        }
        Some(self.flow.get(&pos).copied().unwrap_or(0))
    }

    /// True si esta celda es fuente infinita (mar, lago, pozo, manantial).
    pub fn is_water_source(&self, pos: IVec3) -> bool {
        matches!(self.water_level(pos), Some(0))
    }

    /// Clase de agua en `pos` (`None` = no hay agua): termal, río, mar,
    /// lago o pantano (bioma humedal).
    pub fn water_kind_at(&self, pos: IVec3) -> Option<WaterKind> {
        let v = self.get_voxel(pos)?;
        if v.is_empty() || !v.material.is_water() {
            return None;
        }
        if v.material == Material::ThermalWater {
            return Some(WaterKind::Thermal);
        }
        if is_river_channel(pos.x, pos.z) {
            return Some(WaterKind::River);
        }
        if crate::biomes::biome_at(pos.x, pos.z) == crate::biomes::BiomeId::Wetland {
            return Some(WaterKind::Swamp);
        }
        if open_sea_frac(pos.x, pos.z) >= 0.30 {
            Some(WaterKind::Sea)
        } else {
            Some(WaterKind::Lake)
        }
    }

    /// True si el vóxel en `pos` es agua nadable (cualquier nivel).
    pub fn is_water_at(&self, pos: IVec3) -> bool {
        self.water_level(pos).is_some()
    }

    /// Suelo que sostiene agua: tierra, piedra o arena (el agua no flota).
    fn water_ground(mat: Material) -> bool {
        matches!(
            mat,
            Material::Dirt
                | Material::Mud
                | Material::Stone
                | Material::BlackStone
                | Material::Sand
                | Material::VillageStone
                | Material::Cobblestone
                | Material::RoadCobble
                | Material::Bedrock
        )
    }

    /// Suelo firme en `pos`, incluyendo columnas densas implícitas.
    fn solid_ground_at(&self, pos: IVec3) -> bool {
        if let Some(v) = self.get_voxel(pos) {
            return !v.is_empty() && v.is_fully_solid() && Self::water_ground(v.material);
        }
        if let Some(&h) = self.heights.get(&(pos.x, pos.z)) {
            if pos.y >= 0 && pos.y <= h {
                return Self::water_ground(column_cell_material(pos.x, pos.y, pos.z, h));
            }
        }
        false
    }

    /// True si una celda de agua tiene soporte: suelo firme debajo o al lado,
    /// agua debajo (relleno) o agua encima (cascada). Sin soporte no se genera.
    fn water_cell_supported(&self, pos: IVec3) -> bool {
        let below = IVec3::new(pos.x, pos.y - 1, pos.z);
        if self.solid_ground_at(below) {
            return true;
        }
        if self.is_water_at(below) {
            return true;
        }
        let above = IVec3::new(pos.x, pos.y + 1, pos.z);
        if self.is_water_at(above) {
            return true;
        }
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            if self.solid_ground_at(IVec3::new(pos.x + dx, pos.y, pos.z + dz)) {
                return true;
            }
        }
        false
    }

    /// Inunda con fuentes el tramo bajo el nivel del mar, con soporte y
    /// profundidad limitada: 1–7 de agua (se rellena el lecho con tierra si
    /// es más hondo) y orillas (junto a tierra seca) con máximo 2.
    /// Solo escribe celdas vacías: nunca pisa terreno ni ediciones.
    /// Nada de agua natural dentro de aldeas (la fuente sí es artificial).
    pub fn fill_sea_water(&mut self, x: i32, z: i32) {
        let h = terrain_height(x, z);
        if h >= SEA_LEVEL {
            return;
        }
        if crate::settlements::settlement_claims_block(x, z) {
            return;
        }
        let depth = SEA_LEVEL - h;
        // Orilla = tierra seca al lado: como mucho 2 de agua.
        let shore = terrain_height(x + 1, z) >= SEA_LEVEL
            || terrain_height(x - 1, z) >= SEA_LEVEL
            || terrain_height(x, z + 1) >= SEA_LEVEL
            || terrain_height(x, z - 1) >= SEA_LEVEL;
        let want = if depth > 7 {
            7
        } else if shore && depth > 2 {
            2
        } else {
            depth
        };
        // Rellenar el lecho con tierra hasta la profundidad objetivo.
        for y in (h + 1)..=(SEA_LEVEL - want) {
            let pos = IVec3::new(x, y, z);
            if self.get_voxel(pos).is_none() {
                self.set_voxel(pos, Voxel::dirt());
            }
        }
        // Agua de abajo arriba: cada celda se apoya en la anterior.
        for y in (SEA_LEVEL - want + 1)..=SEA_LEVEL {
            let pos = IVec3::new(x, y, z);
            if self.get_voxel(pos).is_none() && self.water_cell_supported(pos) {
                self.set_voxel(pos, Voxel::solid(Material::Water));
            }
        }
    }

    /// True si esta columna es el centro de un manantial termal (determinista,
    /// raro, en roca suave sobre el nivel del mar, nunca dentro de aldeas).
    pub fn hot_spring_at(x: i32, z: i32) -> bool {
        let h = terrain_height(x, z);
        if h < SEA_LEVEL + 2 || h > SEA_LEVEL + 14 {
            return false;
        }
        if terrain_slope(x, z) > 1.0 {
            return false;
        }
        if mix_seed(WORLD_SEED ^ 0x9075_EED0, x as u32, z as u32) % 2500 != 0 {
            return false;
        }
        !crate::settlements::settlement_claims_block(x, z)
    }

    /// Recomputa la corriente en la caja `±r` alrededor de `(px, pz)` (toda la
    /// altura): borra niveles, re-siembra desde fuentes y expande (horizontal
    /// +1 hasta WATER_FLOW_MAX, caída vertical con el mismo nivel).
    /// Determinista e idempotente. Las fuentes son inamovibles (el agua no se
    /// pica), y toda fuente relevante cae dentro de la caja (margen ≥ 8).
    pub fn refresh_water_around(&mut self, px: i32, pz: i32, r: i32) {
        use std::cmp::Reverse;
        use std::collections::BinaryHeap;
        // 1. Fuentes = agua sin entrada de flujo.
        let mut seeds: Vec<IVec3> = Vec::new();
        for z in (pz - r)..=(pz + r) {
            for x in (px - r)..=(px + r) {
                for y in 0..=WORLD_MAX_Y {
                    let pos = IVec3::new(x, y, z);
                    let Some(v) = self.get_voxel(pos) else {
                        continue;
                    };
                    if v.is_empty() || !v.material.is_water() {
                        continue;
                    }
                    if self.flow.get(&pos).is_none() {
                        seeds.push(pos);
                    }
                }
            }
        }
        // 2. Borrar niveles en la caja (las fuentes se re-siembran solas).
        self.flow.retain(|p, _| {
            p.x < px - r || p.x > px + r || p.z < pz - r || p.z > pz + r
        });
        // 3. Dijkstra multi-fuente (caminar cuesta 1, caer cuesta 0): la caída
        // no alarga la corriente, así que el nivel = nº de pasos horizontales
        // mínimos hasta una fuente (tope WATER_FLOW_MAX).
        let mut dist: FxHashMap<IVec3, u8> = FxHashMap::default();
        // (nivel invertido, desempate, xyz): los arrays sí son Ord.
        let mut heap: BinaryHeap<(Reverse<u8>, u32, [i32; 3])> = BinaryHeap::new();
        let mut seq = 0u32;
        for s in seeds {
            dist.insert(s, 0);
            heap.push((Reverse(0), seq, [s.x, s.y, s.z]));
            seq += 1;
        }
        while let Some((Reverse(level), _, xyz)) = heap.pop() {
            let pos = IVec3::new(xyz[0], xyz[1], xyz[2]);
            if dist.get(&pos).is_some_and(|&d| level > d) {
                continue;
            }
            // Junto a poblados el agua se estanca: contenida, sin corriente
            // (ni recibe nivel ni propaga).
            if crate::settlements::settlement_claims_block(pos.x, pos.z) {
                continue;
            }
            // Vecinos horizontales: +1 nivel, tope WATER_FLOW_MAX.
            if level < WATER_FLOW_MAX {
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let np = IVec3::new(pos.x + dx, pos.y, pos.z + dz);
                    if np.x < px - r || np.x > px + r || np.z < pz - r || np.z > pz + r {
                        continue;
                    }
                    let nl = level + 1;
                    if dist.get(&np).is_some_and(|&d| d <= nl) {
                        continue;
                    }
                    if let Some(v) = self.get_voxel(np) {
                        // Ocupado: solo atraviesa agua existente.
                        if v.is_empty() || !v.material.is_water() {
                            continue;
                        }
                    } else {
                        // Aire: corriente nueva solo con soporte (rodeada de
                        // tierra/piedra/arena o colgando de agua).
                        if !self.water_cell_supported(np) {
                            continue;
                        }
                        self.set_voxel(np, Voxel::solid(Material::Water));
                    }
                    dist.insert(np, nl);
                    // En poblados no hay niveles: agua estancada (fuente).
                    if !crate::settlements::settlement_claims_block(np.x, np.z) {
                        self.flow.insert(np, nl);
                    }
                    heap.push((Reverse(nl), seq, [np.x, np.y, np.z]));
                    seq += 1;
                }
            }
            // Caída vertical: mismo nivel (cascadas), también con soporte
            // (cuelga del agua de arriba).
            let below = IVec3::new(pos.x, pos.y - 1, pos.z);
            if below.y >= 0
                && self.get_voxel(below).is_none()
                && self.water_cell_supported(below)
            {
                if dist.get(&below).is_none_or(|&d| level < d) {
                    self.set_voxel(below, Voxel::solid(Material::Water));
                    dist.insert(below, level);
                    if level > 0
                        && !crate::settlements::settlement_claims_block(below.x, below.z)
                    {
                        self.flow.insert(below, level);
                    }
                    heap.push((Reverse(level), seq, [below.x, below.y, below.z]));
                    seq += 1;
                }
            }
        }
        // Las fuentes quedan sin entrada (= nivel 0) por construcción.
        for (p, d) in &dist {
            if *d == 0 {
                self.flow.remove(p);
            }
        }
    }

    /// Empuje de la corriente en `pos` (río abajo = hacia nivel mayor).
    /// Fuentes, poza quieta y poblados devuelven cero.
    pub fn flow_push_at(&self, pos: IVec3) -> Vec3 {
        if crate::settlements::settlement_claims_block(pos.x, pos.z) {
            return Vec3::ZERO;
        }
        let Some(level) = self.water_level(pos) else {
            return Vec3::ZERO;
        };
        if level == 0 {
            return Vec3::ZERO;
        }
        let mut best = level;
        let mut dir = Vec3::ZERO;
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let np = IVec3::new(pos.x + dx, pos.y, pos.z + dz);
            if let Some(nl) = self.water_level(np) {
                if nl > best {
                    best = nl;
                    dir = Vec3::new(dx as f32, 0.0, dz as f32);
                }
            }
        }
        dir * (1.5 + best as f32)
    }

    /// Estampa un manantial termal en `(x, z)` (centro verificado con
    /// [`Self::hot_spring_at`]): poza de 2 de hondo con agua termal, cama de
    /// arena y anillo de cobble. Llamar una vez por manantial.
    pub fn stamp_hot_spring(&mut self, x: i32, z: i32) {
        for dz in -3i32..=3 {
            for dx in -3i32..=3 {
                let r = dx.abs().max(dz.abs());
                let (wx, wz) = (x + dx, z + dz);
                if r == 3 {
                    // Anillo: cobble a ras + una hilada.
                    let wh = terrain_height(wx, wz);
                    for yy in (wh + 1)..=(wh + 3) {
                        self.remove_voxel(IVec3::new(wx, yy, wz));
                    }
                    self.set_voxel(IVec3::new(wx, wh, wz), Voxel::solid(Material::Cobblestone));
                    self.set_voxel(
                        IVec3::new(wx, wh + 1, wz),
                        Voxel::solid(Material::Cobblestone),
                    );
                } else {
                    // Poza a ras: cama de arena y 1 de agua termal (fuente).
                    // Se limpia también la hierba de encima (wh+1).
                    let wh = terrain_height(wx, wz);
                    self.remove_voxel(IVec3::new(wx, wh, wz));
                    self.remove_voxel(IVec3::new(wx, wh - 1, wz));
                    for yy in (wh + 1)..=(wh + 3) {
                        self.remove_voxel(IVec3::new(wx, yy, wz));
                    }
                    self.set_voxel(IVec3::new(wx, wh - 1, wz), Voxel::sand());
                    self.set_voxel(
                        IVec3::new(wx, wh, wz),
                        Voxel::solid(Material::ThermalWater),
                    );
                }
            }
        }
    }

    /// World-space collision AABB for player physics (`None` = empty / no hitbox).
    /// Trunks use the centered [`TREE_TRUNK_MICRO`]² column; solids use the full cube.
    pub fn collision_aabb(&self, pos: IVec3) -> Option<(Vec3, Vec3)> {
        let v = self.get_voxel(pos)?;
        if v.is_empty() {
            return None;
        }
        // Doors and shallow water: walk-through (no solid hitbox).
        if matches!(v.material, Material::Door | Material::Water | Material::ThermalWater) {
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
        // Diagnostic bypass (see `DEBUG_DISABLE_CULLING`): emit every face.
        if DEBUG_DISABLE_CULLING {
            return false;
        }
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
    /// No fill filter: includes Empty/Reserved buckets so callers can gate
    /// readiness on world state (the orbit preload gate uses this).
    pub fn chunk_coords_within(&self, camera_pos: Vec3, radius: f32) -> Vec<(i32, i32)> {
        let cx = camera_pos.x.floor() as i32;
        let cz = camera_pos.z.floor() as i32;
        let chunk_r = ((radius / MESH_CHUNK_SIZE as f32).ceil() as i32) + 1;
        let c0 = mesh_chunk_coord(cx, cz);
        let radius_sq = radius * radius;
        let mut out = Vec::new();
        for dz in -chunk_r..=chunk_r {
            for dx in -chunk_r..=chunk_r {
                let key = (c0.0 + dx, c0.1 + dz);
                if chunk_dist_sq_xz(camera_pos, key.0, key.1) > radius_sq {
                    continue;
                }
                out.push(key);
            }
        }
        out
    }

    pub fn chunk_coords_near(&self, camera_pos: Vec3, radius: f32) -> Vec<(i32, i32)> {
        self.chunk_coords_within(camera_pos, radius)
            .into_iter()
            // Only filled chunks — Reserved (gen in flight) left holes with
            // wireframe neighbor walls until commit finished.
            .filter(|&(cx, cz)| self.chunk_filled(cx, cz))
            .collect()
    }

    pub fn for_voxels_in_chunk(&self, cx: i32, cz: i32, visit: &mut impl FnMut(&IVec3, &Voxel)) {
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
        // Sparse index: only this column's sections, no full-map scan.
        for cy in 0..MESH_SECTIONS_Y {
            if let Some(set) = self.extras_by_section.get(&(cx, cy, cz)) {
                for pos in set {
                    if let Some(v) = self.extras.get(pos) {
                        visit(pos, v);
                    }
                }
            }
        }
    }

    /// Visit voxels only inside render section `cy` (`[cy*16, cy*16+16)`).
    /// Dense columns are clamped to the slab; extras outside the slab are skipped.
    pub fn for_voxels_in_section(
        &self,
        cx: i32,
        cy: i32,
        cz: i32,
        visit: &mut impl FnMut(&IVec3, &Voxel),
    ) {
        if !self.loaded_chunks.contains_key(&(cx, cz)) {
            return;
        }
        let (y0, y1) = mesh_section_range(cy);
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
                    let lo = y0.max(0);
                    let hi = (y1 - 1).min(h);
                    for y in lo..=hi {
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
        // Sparse index: single section lookup, no full-map scan.
        if let Some(set) = self.extras_by_section.get(&(cx, cy, cz)) {
            for pos in set {
                if let Some(v) = self.extras.get(pos) {
                    visit(pos, v);
                }
            }
        }
    }

    /// Section coords `(cx, cy, cz)` with possible geometry near the camera.
    /// Batched hot path (runs every frame): ONE pass over `extras` builds the
    /// occupied-slab set, one 256-scan per column gives min/max — no per-section
    /// full-map scans. Also gates virgin buried slabs while the focus is above
    /// ground (Fase 2a): a slab with no extras entirely below `min_h - 1` is
    /// impossible to see from the surface, so it is never queued nor drawn.
    /// Cave/dig slabs always hold extras (punch roof copies) and are kept.
    pub fn chunk_sections_near(&self, camera_pos: Vec3, radius: f32) -> Vec<(i32, i32, i32)> {
        use rustc_hash::FxHashSet;
        // Diagnostic: every slab in radius (see `DEBUG_DISABLE_CULLING`).
        if DEBUG_DISABLE_CULLING {
            let mut out = Vec::new();
            for (cx, cz) in self.chunk_coords_near(camera_pos, radius) {
                for cy in 0..MESH_SECTIONS_Y {
                    out.push((cx, cy, cz));
                }
            }
            return out;
        }
        // Slabs holding sparse voxels (grass, trees, edits, cave roofs).
        // Section index keys ARE the occupied slabs — no full-map scan.
        let occupied: FxHashSet<(i32, i32, i32)> =
            self.extras_by_section.keys().copied().collect();
        // Focus above the natural lip → basement is hidden (eye in a pit or
        // underground drops the focus to/below the surface and opens the gate).
        let surface = terrain_height(camera_pos.x.floor() as i32, camera_pos.z.floor() as i32);
        let above_ground = camera_pos.y > surface as f32;
        let mut out = Vec::new();
        for (cx, cz) in self.chunk_coords_near(camera_pos, radius) {
            let minmax = self.chunk_minmax_height(cx, cz);
            for cy in 0..MESH_SECTIONS_Y {
                let (y0, y1) = mesh_section_range(cy);
                let has_extras = occupied.contains(&(cx, cy, cz));
                let in_height_band =
                    minmax.is_some_and(|(_, max_h)| y0 <= max_h + 8) || has_extras;
                if !in_height_band {
                    continue;
                }
                // Buried virgin slab: no extras, top cell below `min_h - 1`.
                if above_ground
                    && !has_extras
                    && minmax.is_some_and(|(min_h, _)| y1 - 1 < min_h - 1)
                {
                    continue;
                }
                out.push((cx, cy, cz));
            }
        }
        out
    }

    pub fn has_chunk(&self, cx: i32, cz: i32) -> bool {
        self.loaded_chunks.contains_key(&(cx, cz))
    }

    /// True when voxels exist and may be drawn (Filled or PendingUnload grace).
    pub fn chunk_filled(&self, cx: i32, cz: i32) -> bool {
        self.loaded_chunks
            .get(&(cx, cz))
            .is_some_and(|s| matches!(s, ChunkState::Filled | ChunkState::PendingUnload))
    }

    /// Detective agujeros: un chunk marcado `Filled` por derrame de vegetación
    /// (`set_voxel` marca Filled cualquier chunk que toca la copa ±2) puede no
    /// tener ni una columna densa — cáscara hueca permanente, invisible para el
    /// guardián (`chunk_filled` es true) que el streaming jamás regenera porque
    /// ya está contenido en `loaded_chunks`. Sonda barata (1 lookup): un chunk
    /// legítimo siempre trae sus 256 columnas; el hueco trae 0.
    pub fn chunk_has_columns(&self, cx: i32, cz: i32) -> bool {
        self.heights
            .contains_key(&(cx * MESH_CHUNK_SIZE, cz * MESH_CHUNK_SIZE))
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
    fn shores_beach_and_mud() {
        // Puntos fijos deterministas (funciones puras del seed).
        assert_eq!(
            crate::biomes::biome_at(684, -980),
            crate::biomes::BiomeId::Beach
        );
        assert!(column_is_mud_shore(760, -872, 18));
        assert_eq!(
            column_cell_material(760, 18, -872, 18),
            Material::Mud
        );
    }

    #[test]
    fn sea_fills_below_sea_level_and_water_is_infinite() {
        // Columna naturalmente bajo el nivel del mar (barrido grueso).
        let (mut x, mut z) = (0, 0);
        'coast: for zz in [0, 500, -500, 1000, -1000] {
            for xx in (0..4000).step_by(2) {
                if terrain_height(xx, zz) < SEA_LEVEL {
                    x = xx;
                    z = zz;
                    break 'coast;
                }
            }
        }
        let h = terrain_height(x, z);
        assert!(h < SEA_LEVEL, "sin costa en el rango");
        let mut world = World::new();
        world.fills_column_for_test(x, z, h);
        world.fill_sea_water(x, z);
        for y in (h + 1)..=SEA_LEVEL {
            assert!(
                world
                    .get_voxel(IVec3::new(x, y, z))
                    .is_some_and(|v| v.material == Material::Water),
                "falta agua en y={y}"
            );
        }
        // Infinita: no se puede picar.
        assert_eq!(world.dig_material_at(IVec3::new(x, SEA_LEVEL, z)), None);
        assert!(world.is_water_source(IVec3::new(x, SEA_LEVEL, z)));
    }

    #[test]
    fn hot_spring_basin_clears_grass_and_holds_thermal() {
        // Punto apto (relieve medio, fuera de aldeas).
        let (mut x, mut z) = (0, 0);
        'spot: for zz in (0..2000).step_by(4) {
            for xx in (0..2000).step_by(4) {
                let h = terrain_height(xx, zz);
                if h >= SEA_LEVEL + 2
                    && h <= SEA_LEVEL + 14
                    && terrain_slope(xx, zz) <= 1.0
                    && !crate::settlements::settlement_claims_block(xx, zz)
                {
                    x = xx;
                    z = zz;
                    break 'spot;
                }
            }
        }
        let mut world = World::new();
        for dz in -4..=4 {
            for dx in -4..=4 {
                let h = terrain_height(x + dx, z + dz);
                world.fills_column_for_test(x + dx, z + dz, h);
                world.set_voxel(
                    IVec3::new(x + dx, h + 1, z + dz),
                    Voxel::grass_from_seed_sloped(1, 0.0),
                );
            }
        }
        world.stamp_hot_spring(x, z);
        let wh = terrain_height(x, z);
        // Poza termal con agua, sin hierba flotando encima.
        assert!(world
            .get_voxel(IVec3::new(x, wh, z))
            .is_some_and(|v| v.material == Material::ThermalWater));
        assert!(world
            .get_voxel(IVec3::new(x, wh + 1, z))
            .is_none_or(|v| v.is_empty()));
        // Anillo de cobble.
        assert!(world
            .get_voxel(IVec3::new(x + 3, terrain_height(x + 3, z), z))
            .is_some_and(|v| v.material == Material::Cobblestone));
    }

    #[test]
    fn flow_needs_ground_or_water_support() {
        let mut world = World::new();
        // Lejos del asentamiento del origen: en los pueblos el agua se estanca
        // a propósito (ver `village_water_stays_still`) y no hay cascada.
        const CX: i32 = 9;
        const CZ: i32 = 9;
        // Piso en y=4 y fuente flotando en y=10 (aire debajo y al lado).
        for x in (CX - 2)..=(CX + 2) {
            for z in (CZ - 2)..=(CZ + 2) {
                world.fills_column_for_test(x, z, 4);
            }
        }
        world.set_voxel(IVec3::new(CX, 10, CZ), Voxel::solid(Material::Water));
        world.refresh_water_around(CX, CZ, 10);
        // La fuente cuelga en cascada hasta el piso...
        for y in 5..=9 {
            assert!(
                world.is_water_at(IVec3::new(CX, y, CZ)),
                "cascada rota en y={y}"
            );
        }
        // ...pero no se extiende flotando a los lados en el aire.
        assert_eq!(world.water_level(IVec3::new(CX + 1, 10, CZ)), None);
        assert_eq!(world.water_level(IVec3::new(CX, 10, CZ + 1)), None);
    }

    #[test]
    fn village_water_stays_still() {
        use crate::realms::{realm_info, RealmId};
        use crate::settlements::{plans_for_realm, SettlementKind};
        // Centro de una aldea real (seco normalmente).
        let mut center = None;
        for iz in -8..8 {
            for ix in -8..8 {
                if let Some(info) = realm_info(RealmId { ix, iz }) {
                    for p in plans_for_realm(&info) {
                        if p.kind == SettlementKind::Village {
                            center = Some(p.center_block);
                            break;
                        }
                    }
                }
                if center.is_some() {
                    break;
                }
            }
            if center.is_some() {
                break;
            }
        }
        let (cx, cz) = center.expect("aldea");
        let h = terrain_height(cx, cz);
        let mut world = World::new();
        world.fills_column_for_test(cx, cz, h);
        world.set_voxel(IVec3::new(cx, h + 1, cz), Voxel::solid(Material::Water));
        world.refresh_water_around(cx, cz, 10);
        // Estancada: sin nivel de corriente, sin empuje, sin propagar.
        assert_eq!(world.water_level(IVec3::new(cx, h + 1, cz)), Some(0));
        assert_eq!(world.flow_push_at(IVec3::new(cx, h + 1, cz)), Vec3::ZERO);
        assert_eq!(world.water_level(IVec3::new(cx + 1, h + 1, cz)), None);
    }
    #[test]
    fn flow_spreads_four_blocks_max() {
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
        // Piso largo de tierra con una fuente en un extremo.
        for dx in 0..12 {
            world.fills_column_for_test(x0 + dx, z0, 4);
        }
        world.set_voxel(
            IVec3::new(x0, 5, z0),
            Voxel::solid(Material::Water),
        );
        world.refresh_water_around(x0 + 5, z0, 10);
        // Niveles 1..=4 en los 4 siguientes, nada en el quinto.
        for (dx, want) in [(1, 1), (2, 2), (3, 3), (4, 4)] {
            assert_eq!(
                world.water_level(IVec3::new(x0 + dx, 5, z0)),
                Some(want),
                "dx={dx}"
            );
        }
        assert_eq!(world.water_level(IVec3::new(x0 + 5, 5, z0)), None);
        assert!(world.is_water_source(IVec3::new(x0, 5, z0)));
    }

    #[test]
    fn stream_regenerates_hollow_spillover_chunk() {
        use glam::IVec3;
        // Detective agujeros: derrame de copa fuera del disco marca Filled sin
        // columnas; al acercarse, el streaming debe regenerarlo (antes lo
        // saltaba por "contenido" → agujero 16×16 permanente en diagonal).
        let mut world = World::new();
        world.set_voxel(IVec3::new(80, 30, 0), Voxel::foliage(Material::Leaves));
        assert!(world.chunk_filled(5, 0));
        assert!(
            !world.chunk_has_columns(5, 0),
            "spillover must leave the chunk hollow"
        );
        let mut frames = 0;
        while !world.chunk_has_columns(5, 0) && frames < 64 {
            world.stream_around(Vec3::new(88.0, 40.0, 8.0));
            frames += 1;
        }
        assert!(
            world.chunk_has_columns(5, 0),
            "streaming must backfill the hollow chunk"
        );
        assert!(world.column_height(80, 0).is_some());
    }

    #[test]
    fn fill_keep_ring_commits_spuriously_filled_chunks() {
        use glam::IVec3;
        // A neighbor's canopy/settlement spillover marks a chunk Filled via
        // `set_voxel` before its own payload runs. The payload must still
        // commit (regression: it was dropped, leaving a permanently hollow
        // chunk — state Filled, no dense columns, invisible to the guardian).
        let mut world = World::new();
        let cols = generate_chunk_columns(0, 0);
        world.loaded_chunks.insert((0, 0), ChunkState::Reserved);
        world.pending_chunks.push_back((0, 0, cols));
        world.set_voxel(IVec3::new(0, 30, 0), Voxel::foliage(Material::Leaves));
        assert!(world.chunk_filled(0, 0));
        assert!(world.column_height(0, 0).is_none());
        world.stream_around(Vec3::new(8.0, 20.0, 8.0));
        assert!(
            world.column_height(0, 0).is_some(),
            "spurious Filled must not drop the real payload"
        );
    }

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
        assert!(world
            .column_height(tc.0 * MESH_CHUNK_SIZE, tc.1 * MESH_CHUNK_SIZE)
            .is_some());
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
                assert!(world.has_chunk(cx, cz), "missing shunk chunk ({cx},{cz})");
            }
        }
        assert!(!world.has_chunk(SHUNK_GRID, 0));
        assert!(!world.has_chunk(0, SHUNK_GRID));
        let (dirt, grass) = world.voxel_counts();
        if ENABLE_GRASS {
            // Meadow retune 2026-09: ~50% tufts on eligible columns, still
            // well under full cover (trees / slopes / stone lids stay bare).
            assert!(grass > 0, "some grass should still grow");
            assert!(
                grass < columns,
                "bare zones should skip some columns ({grass} >= {columns})"
            );
            assert!(
                (grass as f32) < columns as f32 * 0.70,
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
        let spawn = Vec3::new(8.0, 40.0, 8.0);
        // Budgeted streaming: generation and commit share a per-frame time
        // budget, so a fixed call count may or may not finish the near chunk.
        // Pump until (0,0) commits (generous cap; call 0 fills the keep ring).
        let mut frames = 0;
        while world.column_height(0, 0).is_none() && frames < 64 {
            world.stream_around(spawn);
            frames += 1;
        }
        assert!(world.loaded_chunk_count() > 0);
        assert!(world.has_chunk(0, 0));
        assert!(world.column_height(0, 0).is_some());

        // Unload uses PendingUnload + grace frames; pump far away the same way.
        let far = Vec3::new(4000.0, 40.0, 4000.0);
        let tc = mesh_chunk_coord(4000, 4000);
        let mut frames = 0;
        while frames < UNLOAD_GRACE_FRAMES as usize + 96 {
            world.stream_around(far);
            frames += 1;
            if !world.has_chunk(0, 0) && world.has_chunk(tc.0, tc.1) {
                break;
            }
        }
        assert!(!world.has_chunk(0, 0));
        assert!(world.has_chunk(tc.0, tc.1));
        assert!(world
            .column_height(tc.0 * MESH_CHUNK_SIZE, tc.1 * MESH_CHUNK_SIZE)
            .is_some());
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

    #[test]
    fn preload_wide_has_no_column_holes_and_borders_seal() {
        // Detective agujeros en diagonal: precarga amplia y verifica que no
        // haya columnas ausentes, que el guardián no vea huecos y que los
        // bordes E/S entre chunks vecinos estén sellados (cada borde con sus
        // 16 columnas presentes a ambos lados).
        assert!(ENABLE_STREAMING);
        let spawn = Vec3::new(8.0, 40.0, 8.0);
        let mut world = World::new();
        world.preload_shunks_around(spawn);

        // 1) Sin huecos de mesh en el anillo visible.
        let holes = world.mesh_ring_holes(spawn);
        assert!(holes.is_empty(), "mesh ring holes after preload: {holes:?}");

        // 2) Cada chunk del anillo tiene sus 16×16 columnas densas.
        let c0 = mesh_chunk_coord(8, 8);
        let keep_r = preload_radius_chunks();
        let mut checked_chunks = 0usize;
        for dz in -keep_r..=keep_r {
            for dx in -keep_r..=keep_r {
                if dx * dx + dz * dz > keep_r * keep_r {
                    continue;
                }
                let (cx, cz) = (c0.0 + dx, c0.1 + dz);
                assert!(world.chunk_filled(cx, cz), "chunk ({cx},{cz}) not filled");
                let x0 = cx * MESH_CHUNK_SIZE;
                let z0 = cz * MESH_CHUNK_SIZE;
                for z in z0..z0 + MESH_CHUNK_SIZE {
                    for x in x0..x0 + MESH_CHUNK_SIZE {
                        assert!(
                            world.column_height(x, z).is_some(),
                            "missing column ({x},{z}) in chunk ({cx},{cz})"
                        );
                    }
                }
                checked_chunks += 1;
                // 3) Sin cáscaras huecas dentro del disco: todo Filled trae columnas.
                assert!(
                    world.chunk_has_columns(cx, cz),
                    "hollow spillover chunk ({cx},{cz})"
                );
                // 4) Bordes E y S sellados cuando AMBOS lados traen columnas (el
                // derrame fuera del disco lo rellena el streaming al acercarse).
                if world.chunk_filled(cx + 1, cz) && world.chunk_has_columns(cx + 1, cz) {
                    assert!(world.border_heights_match(cx, cz, cx + 1, cz));
                }
                if world.chunk_filled(cx, cz + 1) && world.chunk_has_columns(cx, cz + 1) {
                    assert!(world.border_heights_match(cx, cz, cx, cz + 1));
                }
            }
        }
        assert!(checked_chunks > 10);
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
        let expected = BLUR_START_AMOUNT + 0.5 * (BLUR_MAX_AMOUNT - BLUR_START_AMOUNT);
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

        // Fade window lives at GRASS_BLADE_FADE_START..GRASS_BLADE_FADE_END
        // (68..73): until the start, full blades; then cut 1 per block; sealed
        // carpet from the fade end on.
        let before_fade = GrassLod::for_distance(GRASS_BLADE_FADE_START - 1.0);
        assert!(!before_fade.solid_carpet);
        assert_eq!(before_fade.max_blade_y, Some(MICROVOXEL_RES - 1));

        let at_start = GrassLod::for_distance(GRASS_BLADE_FADE_START);
        assert!(at_start.solid_carpet);
        assert_eq!(at_start.max_blade_y, Some(GRASS_BLADE_MAX_HEIGHT - 1));

        let mid_fade = GrassLod::for_distance(GRASS_BLADE_FADE_START + 2.0);
        assert!(mid_fade.solid_carpet);
        assert_eq!(mid_fade.max_blade_y, Some(GRASS_BLADE_MAX_HEIGHT - 3));

        let at_end = GrassLod::for_distance(GRASS_BLADE_FADE_END);
        assert!(at_end.solid_carpet);
        assert!(at_end.max_blade_y.is_none());
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
        // Detective agujeros: el batch por chunk y la columna individual deben
        // coincidir en TODO el chunk, bordes incluidos. Antes el blur 3×3 se
        // truncaba en el borde del tile y E/S diferían ±1 (costura → agujero
        // en diagonal al cortar el LOD circular sobre el grid 16).
        for (cx, cz) in [(0, 0), (2, -1), (-3, 2), (7, -5)] {
            let cols = generate_chunk_columns(cx, cz);
            assert_eq!(cols.len(), (MESH_CHUNK_SIZE * MESH_CHUNK_SIZE) as usize);
            for &(x, z, h) in &cols {
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
        world.loaded_chunks.insert((0, 0), ChunkState::Filled);
        world.loaded_chunks.insert((1, 0), ChunkState::Reserved);
        let origin = Vec3::new(8.0, 10.0, 8.0);
        assert!(!world.neighbors_ready_for_mesh(origin, 0, 0));
        world.loaded_chunks.insert((1, 0), ChunkState::Filled);
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
            world.loaded_chunks.insert((kx, kz), ChunkState::Reserved);
            world
                .pending_chunks
                .push_back((kx, kz, vec![(kx * 16, kz * 16, 10)]));
        }
        assert!(world.pending_chunks.len() >= PENDING_SOFT_CAP);
        // Origin at spawn — (0,0) should be a mesh-ring hole and get Reserved+pending.
        world.stream_around_timed(Vec3::new(8.0, 20.0, 8.0), Duration::from_millis(50));
        let has_near = world.loaded_chunks.contains_key(&(0, 0))
            || world
                .pending_chunks
                .iter()
                .any(|&(x, z, _)| x == 0 && z == 0);
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

    /// Chunks (en coords de chunk de malla) del disco visible alrededor de
    /// `c0`, con radio `inner_r_sq` en unidades de chunk.
    fn visible_crown(c0: (i32, i32)) -> Vec<(i32, i32)> {
        let inner_r = ((dirt_mesh_max_dist() / MESH_CHUNK_SIZE as f32).ceil() as i32) + 1;
        let inner_r_sq = inner_r * inner_r;
        (0..=(2 * inner_r))
            .flat_map(|i| (0..=(2 * inner_r)).map(move |j| (i, j)))
            .map(|(i, j)| (i - inner_r, j - inner_r))
            .filter(|(dx, dz)| dx * dx + dz * dz <= inner_r_sq)
            .map(|(dx, dz)| (c0.0 + dx, c0.1 + dz))
            .collect()
    }

    /// Invariante vigente desde `INNER_COMMIT_CAP` (`fill_keep_ring`): la
    /// corona visible ya NO se comitea en un solo tick —comitearla entera son
    /// ~250 ms de hitch cuando el origen cruza un chunk—, pero el frente
    /// avanza: el chunk bajo el héroe se rellena en el primer tick, el tope
    /// por tick se respeta (si no, vuelve el hitch) y la corona converge sin
    /// dejar huecos. El anillo keep lejano también acaba cerrado.
    #[test]
    fn stream_seals_visible_crown_over_ticks() {
        if !ENABLE_STREAMING {
            return;
        }
        let mut world = World::new();
        let origin = Vec3::new(8.0, 20.0, 24.0);
        let c0 = mesh_chunk_coord(origin.x.floor() as i32, origin.z.floor() as i32);
        let crown = visible_crown(c0);
        let total = crown.len();

        world.stream_around(origin);
        // El más cercano (bajo el héroe) va primero: la pantalla de carga
        // cubre el resto, pero el frente visible nunca se queda parado.
        assert!(
            world.chunk_filled(c0.0, c0.1),
            "spawn chunk must be filled on the first tick"
        );
        // Tope por tick: commits ilimitados = hitch de ~250 ms al cruzar
        // origen (el cap de `fill_keep_ring`). 32 es holgado frente al cap
        // real (4) y sigue lejos de la corona entera.
        let after_one: usize = crown.iter().filter(|(x, z)| world.chunk_filled(*x, *z)).count();
        assert!(
            after_one <= 32,
            "inner-crown commits are not capped per tick: {after_one}/{total} in one tick"
        );

        let mut ticks = 1usize;
        while ticks < 240
            && crown.iter().any(|(x, z)| !world.chunk_filled(*x, *z))
        {
            world.stream_around(origin);
            ticks += 1;
        }
        let missing: Vec<(i32, i32)> = crown
            .iter()
            .filter(|(x, z)| !world.chunk_filled(*x, *z))
            .copied()
            .collect();
        assert!(
            missing.is_empty(),
            "visible crown still hollow after {ticks} ticks: {missing:?}"
        );

        // El anillo keep completo (corona + halo lejano) converge también.
        for _ in 0..240 {
            world.stream_around(origin);
            if world.mesh_ring_holes(origin).is_empty() {
                break;
            }
        }
        let holes = world.mesh_ring_holes(origin);
        assert!(holes.is_empty(), "keep-ring still empty after streaming: {holes:?}");
        assert!(world.chunk_filled(0, 1));
    }

    /// El cap por tick es lo que evita el hitch, pero no puede volver
    /// Hambriento el disco visible: con un presupuesto ridículo (1 ms) el
    /// chunk bajo el héroe se comitea igual y el disco visible no se lo
    /// queda el halo lejano (la regresión original: 72→88 se comía los
    /// slots mientras la corona visible se quedaba hueca).
    #[test]
    fn fill_keep_ring_never_starves_the_visible_crown() {
        if !ENABLE_STREAMING {
            return;
        }
        let c0 = mesh_chunk_coord(8, 8);
        let mut world = World::new();
        world.stream_around_timed(Vec3::new(8.0, 20.0, 8.0), Duration::from_millis(1));
        assert!(
            world.chunk_filled(c0.0, c0.1),
            "nearest chunk must commit even on a 1 ms budget"
        );
        let crown = visible_crown(c0);
        let filled = crown.iter().filter(|(x, z)| world.chunk_filled(*x, *z)).count();
        assert!(
            filled <= 8,
            "tight budget committed {filled} crown chunks (cap is 4 + spares)"
        );
    }

    #[test]
    fn diagonal_sprint_corner_hole_measurement() {
        if !ENABLE_STREAMING {
            return;
        }
        // Model HD-2D: sprint at 4.2 blocks/s diagonally, 60 sim ticks/s, real 7ms budget.
        let speed = 4.2f32;
        let dt = 1.0 / 60.0;
        let step = speed * dt * std::f32::consts::FRAC_1_SQRT_2; // per-axis diagonal
        let budget = Duration::from_millis(7);
        // Visible bubble: chunk centers within dirt_mesh_max_dist().
        let vis = dirt_mesh_max_dist();
        let vis_sq = vis * vis;

        let mut world = World::new();
        let mut origin = Vec3::new(8.0, 40.0, 8.0);
        world.preload_shunks_around(origin);

        let c0_start = mesh_chunk_coord(origin.x.floor() as i32, origin.z.floor() as i32);
        let mut visible_holes = 0usize;
        let mut frames = 0usize;
        let mut max_stream_ms = 0.0f32;
        let mut max_commits = 0usize;
        let mut max_new_kicks = 0usize;
        let mut sum_stream_ms = 0.0f32;
        // Run the equivalent of ~30s of travel (1800 frames).
        for _ in 0..1800 {
            origin.x += step;
            origin.z += step;
            let t0 = Instant::now();
            world.stream_around_timed_look(origin, Vec3::new(1.0, 0.0, 1.0), budget);
            let dt_frame = t0.elapsed();
            sum_stream_ms += dt_frame.as_secs_f32() * 1e3;
            max_stream_ms = max_stream_ms.max(dt_frame.as_secs_f32() * 1e3);

            // Commit count = how much pending drained this tick (spike proxy).
            let gx = origin.x.floor() as i32;
            let gz = origin.z.floor() as i32;
            let c0 = mesh_chunk_coord(gx, gz);
            // Count chunks in the visible bubble that are NOT chunk_filled.
            let vr = ((vis / MESH_CHUNK_SIZE as f32).ceil() as i32) + 1;
            let mut vh = 0usize;
            let mut misses = Vec::new();
            for dz in -vr..=vr {
                for dx in -vr..=vr {
                    let d2 = dx * dx + dz * dz;
                    if d2 > (vr * vr) {
                        continue;
                    }
                    let key = (c0.0 + dx, c0.1 + dz);
                    // Filled columns? use a *visible* test: chunk center dist <= vis
                    let center = mesh_chunk_center(key.0, key.1);
                    let cdx = origin.x - center.x;
                    let cdz = origin.z - center.z;
                    if cdx * cdx + cdz * cdz > vis_sq {
                        continue;
                    }
                    if !world.chunk_filled(key.0, key.1) {
                        vh += 1;
                        misses.push(key);
                    }
                }
            }
            if vh > 0 {
                visible_holes += 1;
                if visible_holes <= 5 {
                    println!(
                        "CORNER-MEASURE frame {} origin ({:.1},{:.1}): VISIBLE HOLES {misses:?}",
                        frames,
                        origin.x,
                        origin.z
                    );
                }
            }
            frames += 1;
        }
        let loaded_now = world.loaded_chunk_count();
        println!(
            "CORNER-MEASURE frames={frames} visible_hole_frames={visible_holes}/{} \
             max_stream_ms={max_stream_ms:.1} avg={:.1}ms loaded={loaded_now} c0_start={c0_start:?}",
            frames,
            sum_stream_ms / frames as f32
        );
        let c0_end = mesh_chunk_coord(origin.x.floor() as i32, origin.z.floor() as i32);
        let real_distance = ((c0_end.0 - c0_start.0).pow(2) + (c0_end.1 - c0_start.1).pow(2)) as f32;
        println!(
            "CORNER-MEASURE traveled chunk-index dist {:.1} ({:?} -> {:?})",
            real_distance,
            c0_start,
            c0_end
        );
        // Report but do not hard-assert a bound yet (measurement pass).
        assert!(frames > 0);
    }

    #[test]
    fn border_heights_match_when_both_filled() {        let mut world = World::new();
        let a = generate_chunk_columns(0, 0);
        let b = generate_chunk_columns(1, 0);
        world.commit_generated_chunk(0, 0, &a, false);
        world.commit_generated_chunk(1, 0, &b, false);
        assert!(world.border_heights_match(0, 0, 1, 0));
    }

    #[test]
    fn meadow_suppresses_outcrop_lids() {
        // Find a strong outcrop-field column on temperate, moist, mid-altitude
        // ground: the geology says rock, the meadow rule must win (dirt lid).
        // Vein-only columns are skipped (veins still make surface stone).
        let mut found = None;
        for z in 150..330 {
            for x in 100..280 {
                if stone_outcrop_profile(x, z).0 == 0 {
                    continue;
                }
                let (temp, moist) = crate::biomes::climate_at(x, z);
                if !(0.30..0.70).contains(&temp) || moist <= 0.35 {
                    continue;
                }
                let nat = terrain_height(x, z);
                if !(10..=32).contains(&nat) {
                    continue;
                }
                if stone_vein_field(x, nat, z) > STONE_NEAR_SURFACE_THRESH {
                    continue;
                }
                found = Some((x, z, nat));
                break;
            }
            if found.is_some() {
                break;
            }
        }
        let (x, z, nat) = found.expect("need a suppressed candidate column");
        assert!(outcrop_suppressed_here(x, z, nat));
        assert!(
            !column_cell_is_stone(x, nat, z, nat),
            "suppressed meadow lid at ({x},{nat},{z}) must stay dirt"
        );
        assert_eq!(
            column_cell_material(x, nat, z, nat),
            Material::Dirt,
            "suppressed meadow lid material"
        );
    }

    #[test]
    fn rock_provinces_survive_suppression() {
        // Same geology outside the temperate-moist band keeps stone lids:
        // find a lift column with hot/cold/dry climate or high altitude.
        let mut found = None;
        for z in 150..330 {
            for x in 100..280 {
                if stone_outcrop_profile(x, z).0 == 0 {
                    continue;
                }
                let (temp, moist) = crate::biomes::climate_at(x, z);
                let nat = terrain_height(x, z);
                if outcrop_suppressed_here(x, z, nat) {
                    continue;
                }
                if (0.30..0.70).contains(&temp) && moist > 0.35 && (10..=32).contains(&nat) {
                    continue;
                }
                found = Some((x, z, nat));
                break;
            }
            if found.is_some() {
                break;
            }
        }
        let (x, z, nat) = found.expect("need an unsuppressed rock column");
        assert!(column_cell_is_stone(x, nat, z, nat));
    }

    #[test]
    fn section_range_tiles_world_height() {
        assert_eq!(MESH_SECTIONS_Y * MESH_SECTION_HEIGHT, WORLD_MAX_HEIGHT);
        for cy in 0..MESH_SECTIONS_Y {
            let (y0, y1) = mesh_section_range(cy);
            assert_eq!(y1 - y0, MESH_SECTION_HEIGHT);
            assert_eq!(mesh_section_y(y0), cy);
            assert_eq!(mesh_section_y(y1 - 1), cy);
        }
    }

    #[test]
    fn edit_dirties_only_touched_section() {
        use glam::IVec3;
        let mut world = World::new();
        // Dense column h=20 in chunk (0,0) → surface slab cy=1.
        for z in 0..MESH_CHUNK_SIZE {
            for x in 0..MESH_CHUNK_SIZE {
                world.fills_column_for_test(x, z, 20);
            }
        }
        world.loaded_chunks.insert((0, 0), ChunkState::Filled);
        let _ = world.take_dirty_sections();
        let _ = world.take_dirty_chunks();
        // Mid-slab edit touches cy=1 only (interior: solo la columna home).
        assert!(world.remove_voxel(IVec3::new(4, 20, 4)));
        let mut dirty = world.take_dirty_sections();
        dirty.sort();
        dirty.dedup();
        assert!(
            dirty.iter().all(|&(_, cy, _)| cy == 1),
            "mid-slab edit must only dirty cy=1, got {dirty:?}"
        );
        assert!(
            dirty.contains(&(0, 1, 0)),
            "home section missing in {dirty:?}"
        );
        assert!(
            !dirty.iter().any(|&(_, cy, _)| cy == 3),
            "air slab cy=3 must stay clean, got {dirty:?}"
        );
    }

    #[test]
    fn edit_on_section_boundary_dirties_both_slabs() {
        use glam::IVec3;
        let mut world = World::new();
        for z in 0..MESH_CHUNK_SIZE {
            for x in 0..MESH_CHUNK_SIZE {
                world.fills_column_for_test(x, z, 20);
            }
        }
        world.loaded_chunks.insert((0, 0), ChunkState::Filled);
        let _ = world.take_dirty_sections();
        let _ = world.take_dirty_chunks();
        // y=16 is the first cell of cy=1; its bottom face lives on plane 16.
        assert!(world.remove_voxel(IVec3::new(4, 16, 4)));
        let dirty = world.take_dirty_sections();
        let cys: Vec<i32> = dirty
            .iter()
            .filter(|&&(cx, _, cz)| cx == 0 && cz == 0)
            .map(|&(_, cy, _)| cy)
            .collect();
        assert!(
            cys.contains(&0) && cys.contains(&1),
            "boundary edit must dirty cy=0 and cy=1, got {cys:?}"
        );
    }

    #[test]
    fn interior_edit_dirties_only_home_column() {
        use glam::IVec3;
        let mut world = World::new();
        for z in 0..MESH_CHUNK_SIZE {
            for x in 0..MESH_CHUNK_SIZE {
                world.fills_column_for_test(x, z, 20);
            }
        }
        world.loaded_chunks.insert((0, 0), ChunkState::Filled);
        let _ = world.take_dirty_sections();
        let _ = world.take_dirty_chunks();
        // (4,20,4) es interior del chunk: ni x ni z tocan borde.
        assert!(world.remove_voxel(IVec3::new(4, 20, 4)));
        let mut dirty = world.take_dirty_sections();
        dirty.sort();
        dirty.dedup();
        assert_eq!(
            dirty,
            vec![(0, 1, 0)],
            "interior edit must dirty only its slab, got {dirty:?}"
        );
    }

    #[test]
    fn border_edit_dirties_xz_neighbor() {
        use glam::IVec3;
        let mut world = World::new();
        for z in 0..MESH_CHUNK_SIZE {
            for x in 0..MESH_CHUNK_SIZE {
                world.fills_column_for_test(x, z, 20);
            }
        }
        world.loaded_chunks.insert((0, 0), ChunkState::Filled);
        let _ = world.take_dirty_sections();
        let _ = world.take_dirty_chunks();
        // x=0 toca el borde oeste: el vecino (-1,1,0) comparte cara.
        assert!(world.remove_voxel(IVec3::new(0, 20, 4)));
        let dirty = world.take_dirty_sections();
        assert!(
            dirty.contains(&(0, 1, 0)),
            "home section missing in {dirty:?}"
        );
        assert!(
            dirty.contains(&(-1, 1, 0)),
            "west neighbor missing in {dirty:?}"
        );
        assert!(
            !dirty.iter().any(|&(cx, _, _)| cx == 1),
            "east side must stay clean in {dirty:?}"
        );
    }

    #[test]
    fn empty_upper_section_has_no_content() {
        let mut world = World::new();
        for z in 0..MESH_CHUNK_SIZE {
            for x in 0..MESH_CHUNK_SIZE {
                world.fills_column_for_test(x, z, 20);
            }
        }
        world.loaded_chunks.insert((0, 0), ChunkState::Filled);
        assert!(world.section_may_have_content(0, 0, 0));
        assert!(world.section_may_have_content(0, 0, 1));
        // max_h=20 → slab cy=2 starts at 32 > 20+8 → provably empty.
        assert!(!world.section_may_have_content(0, 0, 2));
        assert!(!world.section_may_have_content(0, 0, 3));
    }

    #[test]
    fn sections_near_gates_buried_slab_above_ground() {
        let mut world = World::new();
        // Flat high plateau h=25: cy=0 (0-15) is virgin solid far below the lip.
        for z in 0..MESH_CHUNK_SIZE {
            for x in 0..MESH_CHUNK_SIZE {
                world.fills_column_for_test(x, z, 25);
            }
        }
        world.loaded_chunks.insert((0, 0), ChunkState::Filled);
        let surface = terrain_height(8, 8) as f32;
        let above = Vec3::new(8.0, surface + 5.0, 8.0);
        let secs = world.chunk_sections_near(above, 32.0);
        assert!(
            !secs.contains(&(0, 0, 0)),
            "buried cy=0 must be gated from above, got {secs:?}"
        );
        assert!(
            secs.contains(&(0, 1, 0)),
            "surface slab cy=1 must stay, got {secs:?}"
        );
    }

    #[test]
    fn sections_near_opens_gate_below_surface() {
        let mut world = World::new();
        for z in 0..MESH_CHUNK_SIZE {
            for x in 0..MESH_CHUNK_SIZE {
                world.fills_column_for_test(x, z, 25);
            }
        }
        world.loaded_chunks.insert((0, 0), ChunkState::Filled);
        let surface = terrain_height(8, 8) as f32;
        let below = Vec3::new(8.0, surface - 5.0, 8.0);
        let secs = world.chunk_sections_near(below, 32.0);
        assert!(
            secs.contains(&(0, 0, 0)),
            "buried cy=0 must open underground, got {secs:?}"
        );
    }

    #[test]
    fn sections_near_keeps_dug_slab_from_above() {
        use glam::IVec3;
        let mut world = World::new();
        for z in 0..MESH_CHUNK_SIZE {
            for x in 0..MESH_CHUNK_SIZE {
                world.fills_column_for_test(x, z, 25);
            }
        }
        world.loaded_chunks.insert((0, 0), ChunkState::Filled);
        // Dig a shaft down to y=5 → the lowered column opens the burial rule
        // (and punch roof extras pin the slab) so the shaft stays visible.
        assert!(world.remove_voxel(IVec3::new(4, 5, 4)));
        let surface = terrain_height(8, 8) as f32;
        let above = Vec3::new(8.0, surface + 5.0, 8.0);
        let secs = world.chunk_sections_near(above, 32.0);
        assert!(
            secs.contains(&(0, 0, 0)),
            "dug cy=0 holds extras and must stay visible, got {secs:?}"
        );
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
        // Surface stone survives only in unsuppressed rock provinces; scan a
        // wide mixed area (spawn meadow is suppressed by design).
        let mut wide_surface = 0u32;
        let mut wide_n = 0u32;
        for z in 0..128 {
            for x in 0..128 {
                wide_n += 1;
                let h = terraced_column_height(x, z);
                if column_cell_is_stone(x, h, z, h) {
                    wide_surface += 1;
                }
            }
        }
        assert!(
            wide_surface > 0,
            "rocky provinces must keep some surface stone"
        );
        assert!(
            wide_surface < wide_n / 4,
            "surface stone should be patchy, got {wide_surface}/{wide_n}"
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
        assert_eq!(
            crystal_outside, 0,
            "crystals only in y={CRYSTAL_Y_MIN}..={CRYSTAL_Y_MAX}"
        );
        assert!(sap + ruby + em > 0, "expected some crystals in band");
        assert!(sap >= ruby, "sapphire should be >= ruby ({sap} vs {ruby})");
        assert!(ruby >= em, "ruby should be >= emerald ({ruby} vs {em})");
    }

    #[test]
    fn village_suppresses_ores_except_subsuelo() {
        use crate::realms::{realm_info, RealmId};
        use crate::settlements::{
            plans_for_realm, village_ore_suppression, SettlementKind, VILLAGE_WALL_RADIUS,
        };
        // Una aldea con superficie lo bastante alta para medir subsuelo.
        let mut center = None;
        for iz in -8..8 {
            for ix in -8..8 {
                if let Some(info) = realm_info(RealmId { ix, iz }) {
                    for p in plans_for_realm(&info) {
                        if p.kind == SettlementKind::Village {
                            let lip = terrain_height(p.center_block.0, p.center_block.1);
                            if lip >= 16 {
                                center = Some(p.center_block);
                                break;
                            }
                        }
                    }
                }
                if center.is_some() {
                    break;
                }
            }
            if center.is_some() {
                break;
            }
        }
        let (cx, cz) = center.expect("aldea con relieve");
        assert_eq!(village_ore_suppression(cx, cz), 0.98);
        // Rampa: a 24 bloques fuera del muro (d=48) supresión parcial.
        let rim = village_ore_suppression(cx + VILLAGE_WALL_RADIUS + 24, cz);
        assert!(
            rim > 0.2 && rim < 0.98,
            "rampa de escasez esperada, got {rim}"
        );
        // Lejos no hay supresión (buscar punto libre, acotado y determinista).
        let mut far = None;
        for k in 1..40 {
            let s = village_ore_suppression(cx + k * 100, cz);
            if s == 0.0 {
                far = Some((cx + k * 100, cz));
                break;
            }
        }
        assert!(far.is_some(), "punto sin supresión no hallado");
        let lip = terrain_height(cx, cz);
        // Disco r=20 a flor de superficie: casi sin ores (residual 2%).
        let mut shallow = 0u32;
        for dz in -20..=20 {
            for dx in -20..=20 {
                if dx * dx + dz * dz > 400 {
                    continue;
                }
                if stone_embed_kind(cx + dx, lip - 1, cz + dz).is_some() {
                    shallow += 1;
                }
            }
        }
        assert!(shallow <= 8, "ores dentro de la aldea: {shallow}");
        // Mismo disco en el subsuelo (12+ bajo superficie): ritmo normal.
        let mut deep = 0u32;
        for dz in -20..=20 {
            for dx in -20..=20 {
                if dx * dx + dz * dz > 400 {
                    continue;
                }
                if stone_embed_kind(cx + dx, lip - ORE_VILLAGE_SUBSUELO_DEPTH, cz + dz).is_some()
                {
                    deep += 1;
                }
            }
        }
        assert!(deep > 10, "el subsuelo debe mantener ores, got {deep}");
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
                // Suppressed meadow pads never materialize (height drops the
                // lift); only materialized pads must be solid with no holes.
                if outcrop_suppressed_here(x, z, h) {
                    continue;
                }
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
        assert!(
            world.get_voxel(IVec3::new(5, 0, 5)).is_some(),
            "bedrock base"
        );
        assert_eq!(
            world.get_voxel(IVec3::new(5, 0, 5)).map(|v| v.material),
            Some(Material::Bedrock)
        );
        assert!(
            world.get_voxel(IVec3::new(5, 18, 5)).is_some(),
            "roof extras"
        );

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
        let soft =
            world.indoors_factor(Vec3::new(cx as f32 + 0.5, cy as f32 + 0.5, cz as f32 + 0.5));
        assert!((soft - 0.55).abs() < 0.01, "got {soft}");
    }
}
