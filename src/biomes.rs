//! Regional climate biomes (Americas / Mexico–inspired + fantasy pockets)
//! and autochthonous flora.
//!
//! Independent of political [`crate::realms`]. Spawn `(8, 24)` is forced to
//! [`BiomeId::TemperateMeadow`] so the starting look stays the current meadow.

use crate::world::{mix_seed, terrain_height_f, terrain_slope, WORLD_SEED};
use noise::{NoiseFn, Perlin};
use std::sync::OnceLock;

pub const BIOME_SEED: u32 = 0xB10B_E001;
/// Player spawn column — must remain temperate meadow.
pub const SPAWN_X: i32 = 8;
pub const SPAWN_Z: i32 = 24;
/// Hard meadow disk around spawn (blocks).
pub const SPAWN_MEADOW_RADIUS: i32 = 32;
/// Slow climate scale (larger ⇒ broader regions).
const CLIMATE_SCALE: f64 = 0.0022;
/// Fantasy pocket scale (rarer, smaller islands).
const FANTASY_SCALE: f64 = 0.0045;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BiomeId {
    TemperateMeadow,
    PineOakForest,
    DeciduousForest,
    BorealForest,
    Hills,
    CloudForest,
    TropicalDryForest,
    DesertScrub,
    /// Hot sandy desert (dunes / bare sand patches).
    HotDesert,
    /// Cold arid flats (pale ground, sparse conifers).
    IceDesert,
    Wetland,
    /// Orilla arenosa junto al mar (arena, poca hierba, casi sin árboles).
    Beach,
    Savanna,
    /// Fantasy: violet canopy / mist.
    EnchantedForest,
    /// Fantasy: deep umber canopy, low light fog.
    DarkForest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TreeSpecies {
    /// Encino / American oak — current default tree.
    Oak,
    Pine,
    Birch,
    Willow,
    Mesquite,
    CeibaLite,
    /// Fantasy violet canopy.
    Enchanted,
    /// Fantasy dark canopy.
    Umbra,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BushKind {
    /// Bajo sotobosque (1 tallo + mata de hojas).
    LeafClump,
    /// Matorral seco tipo mezquite bajo.
    Scrub,
    /// Juncos / cañas de humedal.
    Reed,
}

#[derive(Clone, Copy, Debug)]
pub struct FloraProfile {
    pub tree_chance: f32,
    pub tree_cell: i32,
    pub grass_bare_chance: f32,
    /// (species, weight) — weights need not sum to 1.
    pub species: &'static [(TreeSpecies, u32)],
    pub bush_chance: f32,
    pub bush_cell: i32,
    pub bushes: &'static [(BushKind, u32)],
    /// Chance a surface cell becomes sand (coherent patches).
    pub sand_chance: f32,
    pub sand_cell: i32,
}

impl BiomeId {
    pub fn label_es(self) -> &'static str {
        match self {
            Self::TemperateMeadow => "PRADERA",
            Self::PineOakForest => "PINO-ENCINO",
            Self::DeciduousForest => "BOSQUE",
            Self::BorealForest => "BOSQUE BOREAL",
            Self::Hills => "COLINAS",
            Self::CloudForest => "BOSQUE DE NIEBLA",
            Self::TropicalDryForest => "SELVA SECA",
            Self::DesertScrub => "MATORRAL",
            Self::HotDesert => "DESIERTO",
            Self::IceDesert => "DESIERTO HELADO",
            Self::Wetland => "HUMEDAL",
            Self::Beach => "PLAYA",
            Self::Savanna => "SABANA",
            Self::EnchantedForest => "BOSQUE ENCANTADO",
            Self::DarkForest => "BOSQUE OSCURO",
        }
    }

    /// Top-face / grass tapa albedo. Meadow matches [`Material::Grass`] exactly.
    /// Neighbor biomes are pushed apart on purpose (2026-09): pine = deep
    /// conifer, deciduous = bright lime, hills = dry khaki, dry band split
    /// straw (tropical) / olive (savanna) / sage (scrub).
    pub fn grass_rgb(self) -> [f32; 3] {
        use crate::world::Material;
        match self {
            Self::TemperateMeadow => Material::Grass.color_rgb(),
            Self::PineOakForest => [0.24, 0.50, 0.30],
            Self::DeciduousForest => [0.40, 0.64, 0.24],
            Self::BorealForest => [0.28, 0.48, 0.36],
            Self::Hills => [0.47, 0.55, 0.31],
            Self::CloudForest => [0.26, 0.55, 0.38],
            Self::TropicalDryForest => [0.62, 0.58, 0.22],
            Self::DesertScrub => [0.52, 0.54, 0.32],
            Self::HotDesert => [0.72, 0.62, 0.34],
            Self::IceDesert => [0.62, 0.72, 0.70],
            Self::Wetland => [0.24, 0.50, 0.32],
            Self::Beach => [0.55, 0.60, 0.34],
            Self::Savanna => [0.55, 0.62, 0.26],
            Self::EnchantedForest => [0.42, 0.28, 0.62],
            Self::DarkForest => [0.18, 0.28, 0.22],
        }
    }

    /// Exposed dirt / skirt tone for this climate. Meadow matches [`Material::Dirt`].
    /// Pine duff reads dark, deciduous loam rich; dry band split red / pale.
    pub fn dirt_rgb(self) -> [f32; 3] {
        use crate::world::Material;
        match self {
            Self::TemperateMeadow => Material::Dirt.color_rgb(),
            Self::PineOakForest => [0.38, 0.28, 0.20],
            Self::DeciduousForest => [0.52, 0.38, 0.24],
            Self::BorealForest => [0.42, 0.34, 0.28],
            Self::Hills => [0.50, 0.40, 0.28],
            Self::CloudForest => [0.40, 0.32, 0.24],
            Self::TropicalDryForest => [0.60, 0.42, 0.22],
            Self::DesertScrub => [0.66, 0.52, 0.32],
            Self::HotDesert => [0.70, 0.52, 0.28],
            Self::IceDesert => [0.55, 0.58, 0.60],
            Self::Wetland => [0.36, 0.30, 0.20],
            Self::Beach => [0.72, 0.62, 0.40],
            Self::Savanna => [0.58, 0.44, 0.26],
            Self::EnchantedForest => [0.38, 0.28, 0.42],
            Self::DarkForest => [0.22, 0.18, 0.16],
        }
    }

    /// Multiply tint for chroma-keyed grass PNGs (meadow stays white).
    pub fn grass_billboard_tint(self) -> [f32; 3] {
        if self == Self::TemperateMeadow {
            return [1.0, 1.0, 1.0];
        }
        let m = Self::TemperateMeadow.grass_rgb();
        let g = self.grass_rgb();
        [
            (g[0] / m[0].max(1e-3)).clamp(0.45, 1.55),
            (g[1] / m[1].max(1e-3)).clamp(0.45, 1.55),
            (g[2] / m[2].max(1e-3)).clamp(0.45, 1.55),
        ]
    }

    /// Near-surface fog / clear colour. Meadow matches legacy `fog_color_for_altitude`.
    /// Pine haze cool, deciduous warm, cloud forest bright mist.
    pub fn fog_surface_rgb(self) -> [f32; 3] {
        match self {
            Self::TemperateMeadow => [0.48, 0.70, 0.42],
            Self::PineOakForest => [0.36, 0.56, 0.44],
            Self::DeciduousForest => [0.46, 0.64, 0.38],
            Self::BorealForest => [0.40, 0.52, 0.50],
            Self::Hills => [0.50, 0.62, 0.44],
            Self::CloudForest => [0.60, 0.70, 0.66],
            Self::TropicalDryForest => [0.62, 0.60, 0.34],
            Self::DesertScrub => [0.68, 0.58, 0.40],
            Self::HotDesert => [0.78, 0.64, 0.38],
            Self::IceDesert => [0.62, 0.72, 0.78],
            Self::Wetland => [0.36, 0.58, 0.48],
            Self::Beach => [0.50, 0.68, 0.62],
            Self::Savanna => [0.58, 0.68, 0.38],
            Self::EnchantedForest => [0.48, 0.32, 0.62],
            Self::DarkForest => [0.22, 0.26, 0.24],
        }
    }

    /// High-altitude sky fog. Meadow matches legacy sky blue.
    pub fn fog_sky_rgb(self) -> [f32; 3] {
        match self {
            Self::TemperateMeadow => [0.42, 0.62, 0.92],
            Self::PineOakForest => [0.40, 0.58, 0.88],
            Self::DeciduousForest => [0.44, 0.64, 0.90],
            Self::BorealForest => [0.48, 0.60, 0.82],
            Self::Hills => [0.46, 0.64, 0.90],
            Self::CloudForest => [0.62, 0.72, 0.82],
            Self::TropicalDryForest => [0.55, 0.68, 0.88],
            Self::DesertScrub => [0.62, 0.72, 0.88],
            Self::HotDesert => [0.70, 0.78, 0.92],
            Self::IceDesert => [0.55, 0.68, 0.88],
            Self::Wetland => [0.40, 0.58, 0.78],
            Self::Beach => [0.46, 0.64, 0.90],
            Self::Savanna => [0.55, 0.70, 0.90],
            Self::EnchantedForest => [0.45, 0.38, 0.72],
            Self::DarkForest => [0.28, 0.32, 0.40],
        }
    }

    /// Shallow cave / dig fog — same pipeline as surface, biome-tinted.
    pub fn fog_cave_shallow_rgb(self) -> [f32; 3] {
        match self {
            Self::TemperateMeadow => [0.22, 0.28, 0.30],
            Self::HotDesert | Self::DesertScrub | Self::Savanna => [0.32, 0.26, 0.18],
            Self::IceDesert | Self::BorealForest => [0.20, 0.26, 0.34],
            Self::EnchantedForest => [0.28, 0.18, 0.36],
            Self::DarkForest => [0.12, 0.14, 0.16],
            Self::Wetland | Self::CloudForest => [0.18, 0.28, 0.28],
            _ => [0.22, 0.28, 0.30],
        }
    }

    /// Deep cave fog (still readable, never pure black).
    pub fn fog_cave_deep_rgb(self) -> [f32; 3] {
        match self {
            Self::EnchantedForest => [0.08, 0.05, 0.12],
            Self::DarkForest => [0.04, 0.05, 0.06],
            Self::HotDesert | Self::DesertScrub => [0.10, 0.07, 0.05],
            Self::IceDesert => [0.06, 0.08, 0.12],
            _ => [0.07, 0.09, 0.12],
        }
    }

    /// Air density for the exponential fog: mist forests close in, clear dry
    /// lands read far. Meadow returns [`crate::world::FOG_DENSITY`].
    pub fn fog_density(self) -> f32 {
        match self {
            Self::CloudForest => 0.012,
            Self::DarkForest => 0.010,
            Self::EnchantedForest | Self::Wetland => 0.009,
            Self::HotDesert | Self::IceDesert | Self::DesertScrub | Self::Savanna => 0.004,
            _ => crate::world::FOG_DENSITY,
        }
    }
}

fn climate_perlin() -> &'static Perlin {
    static P: OnceLock<Perlin> = OnceLock::new();
    P.get_or_init(|| Perlin::new(BIOME_SEED ^ WORLD_SEED))
}

fn climate_sample(x: i32, z: i32, salt: f64) -> f32 {
    let p = climate_perlin();
    let n = p.get([
        x as f64 * CLIMATE_SCALE + salt,
        z as f64 * CLIMATE_SCALE - salt * 0.37,
    ]);
    ((n as f32) * 0.5 + 0.5).clamp(0.0, 1.0)
}

fn fantasy_sample(x: i32, z: i32, salt: f64) -> f32 {
    let p = climate_perlin();
    let n = p.get([
        x as f64 * FANTASY_SCALE + salt + 40.0,
        z as f64 * FANTASY_SCALE - salt * 0.51,
    ]);
    ((n as f32) * 0.5 + 0.5).clamp(0.0, 1.0)
}

/// Temperature / moisture in \[0, 1\], biased so spawn sits in temperate mid-range.
pub fn climate_at(x: i32, z: i32) -> (f32, f32) {
    // Offsets chosen so (8,24) is temperate + mid moisture → meadow.
    let mut temp = climate_sample(x, z, 0.0) * 0.92 + 0.04;
    let mut moist = climate_sample(x, z, 17.3) * 0.90 + 0.05;
    // Soft pull toward meadow near spawn (outside the hard disk).
    let dx = (x - SPAWN_X) as f32;
    let dz = (z - SPAWN_Z) as f32;
    let dist = (dx * dx + dz * dz).sqrt();
    if dist < 96.0 {
        let t = (1.0 - dist / 96.0).clamp(0.0, 1.0);
        temp = temp * (1.0 - 0.35 * t) + 0.48 * (0.35 * t);
        moist = moist * (1.0 - 0.35 * t) + 0.42 * (0.35 * t);
    }
    (temp, moist)
}

/// Relief multiplier from climate only (no height feedback, no recursion —
/// safe to call inside `terrain_height_f`). Deserts and drowned lowlands
/// flatten toward the average; boreal and cloud belts rise. Smoothstep fences
/// so region borders never cut a wall. Spawn climate (~0.48, ~0.42) reads 1.0.
pub fn relief_scale_for_climate(temp: f32, moist: f32) -> f32 {
    let ss = |a: f32, b: f32, x: f32| -> f32 {
        let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    // Flat pull: hot/dry deserts fully, very wet lowlands partially.
    let desert_flat = ss(0.60, 0.72, temp) * (1.0 - ss(0.20, 0.32, moist));
    let wet_flat = ss(0.62, 0.74, moist);
    let flat = desert_flat.max(wet_flat * 0.7);
    // Rugged pull: cold moist belt (boreal) and warm wet belt (cloud forest).
    let boreal_up = (1.0 - ss(0.20, 0.30, temp)) * ss(0.30, 0.42, moist);
    let cloud_up = ss(0.55, 0.65, moist) * ss(0.30, 0.38, temp) * (1.0 - ss(0.62, 0.70, temp));
    let up = (boreal_up * 0.5 + cloud_up * 0.5).min(1.0);
    (1.0 - 0.55 * flat + 0.30 * up).clamp(0.45, 1.35)
}

/// Climate + height + slope → biome decision (the pure part of
/// [`biome_from_climate`] so commit paths that already know temp/moist/h/slope
/// skip re-sampling terrain noise per column).
pub fn biome_from_climate_slope(    x: i32,
    z: i32,
    temp: f32,
    moist: f32,
    h: f32,
    slope: f32,
) -> BiomeId {
    // Rare fantasy pockets (outside spawn pull) — same climate layer, separate noise.
    let enchant = fantasy_sample(x, z, 3.1);
    let umbra = fantasy_sample(x, z, 9.7);
    if enchant > 0.86 && moist > 0.40 && moist < 0.75 && temp > 0.30 && temp < 0.70 {
        return BiomeId::EnchantedForest;
    }
    if umbra > 0.87 && moist > 0.48 && temp > 0.28 && temp < 0.62 {
        return BiomeId::DarkForest;
    }

    if slope > 1.35 && h > 28.0 {
        return BiomeId::Hills;
    }
    // Orilla del mar: arena solo en la franja del nivel con agua al lado
    // (anillo de playa real, no todo el relieve medio).
    if h >= crate::world::SEA_LEVEL as f32 - 1.0
        && h <= crate::world::SEA_LEVEL as f32 + 2.0
        && crate::world::shore_water_near(x, z)
    {
        return BiomeId::Beach;
    }
    if moist > 0.72 && h < 16.0 {
        return BiomeId::Wetland;
    }
    // Cold arid → ice desert; cold wet → boreal.
    if temp < 0.26 {
        return if moist < 0.38 {
            BiomeId::IceDesert
        } else {
            BiomeId::BorealForest
        };
    }
    // Hot arid ladder: scrub → hot sand desert.
    if temp > 0.78 && moist < 0.22 {
        return BiomeId::HotDesert;
    }
    if temp > 0.70 && moist < 0.30 {
        return BiomeId::DesertScrub;
    }
    if temp > 0.62 && moist < 0.45 {
        return if moist < 0.34 {
            BiomeId::TropicalDryForest
        } else {
            BiomeId::Savanna
        };
    }
    if moist > 0.62 && h > 32.0 && temp > 0.35 && temp < 0.65 {
        return BiomeId::CloudForest;
    }
    if moist > 0.55 && temp > 0.32 && temp < 0.62 && h > 22.0 {
        return BiomeId::PineOakForest;
    }
    if moist > 0.50 && temp > 0.35 && temp < 0.68 {
        return BiomeId::DeciduousForest;
    }
    if temp > 0.55 && moist > 0.35 && moist < 0.55 {
        return BiomeId::Savanna;
    }
    BiomeId::TemperateMeadow
}

fn biome_from_climate(x: i32, z: i32) -> BiomeId {
    let (temp, moist) = climate_at(x, z);
    let h = terrain_height_f(x, z);
    let slope = terrain_slope(x, z);
    biome_from_climate_slope(x, z, temp, moist, h, slope)
}

/// Biome at block column. Spawn disk is always temperate meadow.
pub fn biome_at(x: i32, z: i32) -> BiomeId {
    let dx = x - SPAWN_X;
    let dz = z - SPAWN_Z;
    if dx * dx + dz * dz <= SPAWN_MEADOW_RADIUS * SPAWN_MEADOW_RADIUS {
        return BiomeId::TemperateMeadow;
    }
    biome_from_climate(x, z)
}

/// Same as [`biome_at`] but takes already-computed climate/height/slope — used
/// by the streaming commit so it samples each once per column, not once per
/// decision call.
pub fn biome_at_slope(x: i32, z: i32, temp: f32, moist: f32, h: f32, slope: f32) -> BiomeId {
    let dx = x - SPAWN_X;
    let dz = z - SPAWN_Z;
    if dx * dx + dz * dz <= SPAWN_MEADOW_RADIUS * SPAWN_MEADOW_RADIUS {
        return BiomeId::TemperateMeadow;
    }
    biome_from_climate_slope(x, z, temp, moist, h, slope)
}

pub fn flora_for(biome: BiomeId) -> FloraProfile {
    match biome {
        BiomeId::TemperateMeadow => FloraProfile {
            tree_chance: 0.38,
            tree_cell: 10,
            grass_bare_chance: 0.50,
            species: &[(TreeSpecies::Oak, 10)],
            bush_chance: 0.0,
            bush_cell: 12,
            bushes: &[],
            sand_chance: 0.0,
            sand_cell: 12,
        },
        BiomeId::PineOakForest => FloraProfile {
            tree_chance: 0.55,
            tree_cell: 8,
            grass_bare_chance: 0.70,
            species: &[(TreeSpecies::Oak, 5), (TreeSpecies::Pine, 5)],
            bush_chance: 0.32,
            bush_cell: 6,
            bushes: &[(BushKind::LeafClump, 10)],
            sand_chance: 0.0,
            sand_cell: 12,
        },
        BiomeId::DeciduousForest => FloraProfile {
            tree_chance: 0.62,
            tree_cell: 7,
            grass_bare_chance: 0.55,
            species: &[(TreeSpecies::Oak, 6), (TreeSpecies::Birch, 4)],
            bush_chance: 0.40,
            bush_cell: 5,
            bushes: &[(BushKind::LeafClump, 10)],
            sand_chance: 0.0,
            sand_cell: 12,
        },
        BiomeId::BorealForest => FloraProfile {
            tree_chance: 0.58,
            tree_cell: 7,
            grass_bare_chance: 0.88,
            species: &[(TreeSpecies::Pine, 10)],
            bush_chance: 0.18,
            bush_cell: 7,
            bushes: &[(BushKind::LeafClump, 10)],
            sand_chance: 0.0,
            sand_cell: 12,
        },
        BiomeId::Hills => FloraProfile {
            tree_chance: 0.22,
            tree_cell: 12,
            grass_bare_chance: 0.92,
            species: &[(TreeSpecies::Pine, 8), (TreeSpecies::Oak, 2)],
            bush_chance: 0.20,
            bush_cell: 8,
            bushes: &[(BushKind::Scrub, 6), (BushKind::LeafClump, 4)],
            sand_chance: 0.02,
            sand_cell: 14,
        },
        BiomeId::CloudForest => FloraProfile {
            tree_chance: 0.60,
            tree_cell: 7,
            grass_bare_chance: 0.48,
            species: &[
                (TreeSpecies::Oak, 5),
                (TreeSpecies::Willow, 3),
                (TreeSpecies::Pine, 2),
            ],
            bush_chance: 0.45,
            bush_cell: 5,
            bushes: &[(BushKind::LeafClump, 8), (BushKind::Reed, 2)],
            sand_chance: 0.0,
            sand_cell: 12,
        },
        BiomeId::TropicalDryForest => FloraProfile {
            tree_chance: 0.28,
            tree_cell: 11,
            grass_bare_chance: 0.90,
            species: &[(TreeSpecies::Mesquite, 8), (TreeSpecies::Oak, 2)],
            bush_chance: 0.38,
            bush_cell: 6,
            bushes: &[(BushKind::Scrub, 10)],
            sand_chance: 0.12,
            sand_cell: 8,
        },
        BiomeId::DesertScrub => FloraProfile {
            tree_chance: 0.04,
            tree_cell: 16,
            grass_bare_chance: 0.97,
            species: &[(TreeSpecies::Mesquite, 10)],
            bush_chance: 0.42,
            bush_cell: 5,
            bushes: &[(BushKind::Scrub, 10)],
            sand_chance: 0.28,
            sand_cell: 6,
        },
        BiomeId::HotDesert => FloraProfile {
            tree_chance: 0.01,
            tree_cell: 20,
            grass_bare_chance: 0.995,
            species: &[(TreeSpecies::Mesquite, 10)],
            bush_chance: 0.08,
            bush_cell: 10,
            bushes: &[(BushKind::Scrub, 10)],
            sand_chance: 0.72,
            sand_cell: 4,
        },
        BiomeId::IceDesert => FloraProfile {
            tree_chance: 0.06,
            tree_cell: 14,
            grass_bare_chance: 0.96,
            species: &[(TreeSpecies::Pine, 10)],
            bush_chance: 0.05,
            bush_cell: 12,
            bushes: &[(BushKind::LeafClump, 10)],
            sand_chance: 0.35,
            sand_cell: 5,
        },
        BiomeId::Wetland => FloraProfile {
            tree_chance: 0.35,
            tree_cell: 9,
            grass_bare_chance: 0.40,
            species: &[(TreeSpecies::Willow, 8), (TreeSpecies::Oak, 2)],
            bush_chance: 0.50,
            bush_cell: 4,
            bushes: &[(BushKind::Reed, 10)],
            sand_chance: 0.04,
            sand_cell: 10,
        },
        BiomeId::Beach => FloraProfile {
            tree_chance: 0.02,
            tree_cell: 18,
            grass_bare_chance: 0.90,
            species: &[(TreeSpecies::Willow, 10)],
            bush_chance: 0.06,
            bush_cell: 10,
            bushes: &[(BushKind::Scrub, 10)],
            sand_chance: 0.55,
            sand_cell: 5,
        },
        BiomeId::Savanna => FloraProfile {
            tree_chance: 0.14,
            tree_cell: 14,
            grass_bare_chance: 0.75,
            species: &[
                (TreeSpecies::Mesquite, 7),
                (TreeSpecies::CeibaLite, 2),
                (TreeSpecies::Oak, 1),
            ],
            bush_chance: 0.28,
            bush_cell: 7,
            bushes: &[(BushKind::Scrub, 9), (BushKind::LeafClump, 1)],
            sand_chance: 0.10,
            sand_cell: 9,
        },
        BiomeId::EnchantedForest => FloraProfile {
            tree_chance: 0.58,
            tree_cell: 7,
            grass_bare_chance: 0.50,
            species: &[(TreeSpecies::Enchanted, 8), (TreeSpecies::Oak, 2)],
            bush_chance: 0.35,
            bush_cell: 6,
            bushes: &[(BushKind::LeafClump, 10)],
            sand_chance: 0.0,
            sand_cell: 12,
        },
        BiomeId::DarkForest => FloraProfile {
            tree_chance: 0.70,
            tree_cell: 6,
            grass_bare_chance: 0.70,
            species: &[(TreeSpecies::Umbra, 9), (TreeSpecies::Oak, 1)],
            bush_chance: 0.45,
            bush_cell: 5,
            bushes: &[(BushKind::LeafClump, 10)],
            sand_chance: 0.0,
            sand_cell: 12,
        },
    }
}

pub fn pick_tree_species(x: i32, z: i32, biome: BiomeId) -> TreeSpecies {
    let profile = flora_for(biome);
    let total: u32 = profile.species.iter().map(|(_, w)| *w).sum();
    if total == 0 {
        return TreeSpecies::Oak;
    }
    let roll = mix_seed(BIOME_SEED ^ 0x75EE_C1E5, x as u32, z as u32) % total;
    let mut acc = 0u32;
    for &(sp, w) in profile.species {
        acc += w;
        if roll < acc {
            return sp;
        }
    }
    TreeSpecies::Oak
}

pub fn pick_bush_kind(x: i32, z: i32, biome: BiomeId) -> Option<BushKind> {
    let profile = flora_for(biome);
    let total: u32 = profile.bushes.iter().map(|(_, w)| *w).sum();
    if total == 0 {
        return None;
    }
    let roll = mix_seed(BIOME_SEED ^ 0x4255_5348, x as u32, z as u32) % total;
    let mut acc = 0u32;
    for &(kind, w) in profile.bushes {
        acc += w;
        if roll < acc {
            return Some(kind);
        }
    }
    profile.bushes.first().map(|(k, _)| *k)
}

/// Coherent surface sand patches driven by biome flora profile.
pub fn column_has_sand_surface(x: i32, z: i32) -> bool {
    column_has_sand_surface_biome(biome_at(x, z), x, z)
}

/// [`column_has_sand_surface`] for a caller that already knows the biome.
pub fn column_has_sand_surface_biome(biome: BiomeId, x: i32, z: i32) -> bool {
    let flora = flora_for(biome);
    if flora.sand_chance <= 0.0 {
        return false;
    }
    let cell = flora.sand_cell.max(3);
    let cx = x.div_euclid(cell);
    let cz = z.div_euclid(cell);
    let patch = mix_seed(BIOME_SEED ^ 0x5A4D_0001, cx as u32, cz as u32);
    let ox = (patch % cell as u32) as i32;
    let oz = ((patch >> 8) % cell as u32) as i32;
    let lx = x.rem_euclid(cell);
    let lz = z.rem_euclid(cell);
    // Soft blob around the patch center (not a single cell).
    let dx = (lx - ox).abs();
    let dz = (lz - oz).abs();
    if dx > 2 || dz > 2 {
        return false;
    }
    let roll = ((patch >> 16) % 1000) as f32 / 1000.0;
    let edge = 1.0 - (dx + dz) as f32 * 0.18;
    roll < flora.sand_chance * edge.clamp(0.35, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_is_temperate_meadow() {
        assert_eq!(biome_at(SPAWN_X, SPAWN_Z), BiomeId::TemperateMeadow);
        assert_eq!(
            biome_at(SPAWN_X + 10, SPAWN_Z - 5),
            BiomeId::TemperateMeadow
        );
        assert_eq!(
            biome_at(SPAWN_X + SPAWN_MEADOW_RADIUS, SPAWN_Z),
            BiomeId::TemperateMeadow
        );
    }

    #[test]
    fn relief_scale_flattens_deserts_and_keeps_spawn() {
        // Spawn climate must not reshape the meadow.
        let (t, m) = climate_at(SPAWN_X, SPAWN_Z);
        assert!((relief_scale_for_climate(t, m) - 1.0).abs() < 1e-5);
        // Hot arid flattens hard, wet lowlands partially, boreal rises.
        assert!(relief_scale_for_climate(0.85, 0.10) < 0.6);
        assert!(relief_scale_for_climate(0.50, 0.80) < 1.0);
        assert!(relief_scale_for_climate(0.15, 0.50) > 1.0);
        // Temperate mid stays neutral.
        assert!((relief_scale_for_climate(0.50, 0.45) - 1.0).abs() < 0.05);
        // Bounded everywhere on the climate square.
        for ti in 0..=10 {
            for mi in 0..=10 {
                let s = relief_scale_for_climate(ti as f32 / 10.0, mi as f32 / 10.0);
                assert!((0.45..=1.35).contains(&s), "s={s} at {ti},{mi}");
            }
        }
    }

    #[test]
    fn biome_queries_are_deterministic() {
        let a = biome_at(120, -80);
        let b = biome_at(120, -80);
        assert_eq!(a, b);
        let (t0, m0) = climate_at(50, 50);
        let (t1, m1) = climate_at(50, 50);
        assert!((t0 - t1).abs() < 1e-6);
        assert!((m0 - m1).abs() < 1e-6);
    }

    #[test]
    fn wide_area_has_several_biomes() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        for z in (-500..=500).step_by(35) {
            for x in (-500..=500).step_by(35) {
                set.insert(biome_at(x, z));
            }
        }
        assert!(
            set.len() >= 5,
            "expected diverse biomes, got {} ({set:?})",
            set.len()
        );
        assert!(set.contains(&BiomeId::TemperateMeadow));
    }

    #[test]
    fn meadow_flora_matches_legacy_density() {
        let f = flora_for(BiomeId::TemperateMeadow);
        assert!((f.tree_chance - 0.38).abs() < 1e-5);
        assert_eq!(f.tree_cell, 10);
        // Retuned 2026-09: meadow reads green (~50% tufts, was 0.82 bare).
        assert!((f.grass_bare_chance - 0.50).abs() < 1e-5);
        assert_eq!(
            pick_tree_species(SPAWN_X, SPAWN_Z, BiomeId::TemperateMeadow),
            TreeSpecies::Oak
        );
        assert_eq!(f.bush_chance, 0.0);
        assert!(f.bushes.is_empty());
        assert_eq!(f.sand_chance, 0.0);
        assert!(pick_bush_kind(SPAWN_X, SPAWN_Z, BiomeId::TemperateMeadow).is_none());
    }

    #[test]
    fn meadow_surface_matches_legacy_materials() {
        use crate::world::Material;
        assert_eq!(
            BiomeId::TemperateMeadow.grass_rgb(),
            Material::Grass.color_rgb()
        );
        assert_eq!(
            BiomeId::TemperateMeadow.dirt_rgb(),
            Material::Dirt.color_rgb()
        );
        assert_eq!(
            BiomeId::TemperateMeadow.grass_billboard_tint(),
            [1.0, 1.0, 1.0]
        );
        assert_ne!(
            BiomeId::DesertScrub.grass_rgb(),
            Material::Grass.color_rgb()
        );
        assert_ne!(BiomeId::Wetland.dirt_rgb(), Material::Dirt.color_rgb());
    }

    #[test]
    fn meadow_fog_matches_legacy_altitude_ramp() {
        use crate::world::{fog_color_for_altitude, fog_color_for_biome};
        let eye = 20.0;
        let surf = 18.0;
        let legacy = fog_color_for_altitude(eye, surf);
        let biome = fog_color_for_biome(eye, surf, BiomeId::TemperateMeadow);
        assert!((legacy[0] - biome[0]).abs() < 1e-5);
        assert!((legacy[1] - biome[1]).abs() < 1e-5);
        assert!((legacy[2] - biome[2]).abs() < 1e-5);
        let desert = fog_color_for_biome(eye, surf, BiomeId::DesertScrub);
        assert!((desert[0] - biome[0]).abs() > 0.05 || (desert[1] - biome[1]).abs() > 0.05);
    }

    #[test]
    fn neighbor_biomes_read_apart() {
        // Green family must not cluster: pine deep, deciduous lime, meadow mid.
        let pine = BiomeId::PineOakForest.grass_rgb();
        let deci = BiomeId::DeciduousForest.grass_rgb();
        let meadow = BiomeId::TemperateMeadow.grass_rgb();
        assert!((pine[1] - deci[1]).abs() > 0.08);
        assert!((deci[2] - meadow[2]).abs() > 0.05);
        // Dirt: pine duff dark vs deciduous loam.
        assert!(
            BiomeId::PineOakForest.dirt_rgb()[0]
                < BiomeId::DeciduousForest.dirt_rgb()[0] - 0.08
        );
        // Dry band: straw vs olive vs sage spread apart.
        let tdf = BiomeId::TropicalDryForest.grass_rgb();
        let sav = BiomeId::Savanna.grass_rgb();
        let scr = BiomeId::DesertScrub.grass_rgb();
        assert!(tdf[0] > sav[0] + 0.03 && sav[1] > scr[1] + 0.03);
    }

    #[test]
    fn fog_density_spread() {
        use crate::world::FOG_DENSITY;
        assert!((BiomeId::TemperateMeadow.fog_density() - FOG_DENSITY).abs() < 1e-6);
        assert!(BiomeId::CloudForest.fog_density() > FOG_DENSITY);
        assert!(BiomeId::HotDesert.fog_density() < FOG_DENSITY);
    }

    #[test]
    fn dry_biomes_prefer_scrub_bushes() {
        let d = flora_for(BiomeId::DesertScrub);
        assert!(d.bush_chance > 0.3);
        assert!(d.bushes.iter().any(|(k, _)| *k == BushKind::Scrub));
        let w = flora_for(BiomeId::Wetland);
        assert!(w.bushes.iter().any(|(k, _)| *k == BushKind::Reed));
    }

    #[test]
    fn hot_desert_is_mostly_sand() {
        let f = flora_for(BiomeId::HotDesert);
        assert!(f.sand_chance > 0.5);
        assert!(f.grass_bare_chance > 0.98);
        assert!(f.tree_chance < 0.05);
    }

    #[test]
    fn enchanted_and_dark_have_fantasy_species() {
        assert!(flora_for(BiomeId::EnchantedForest)
            .species
            .iter()
            .any(|(s, _)| *s == TreeSpecies::Enchanted));
        assert!(flora_for(BiomeId::DarkForest)
            .species
            .iter()
            .any(|(s, _)| *s == TreeSpecies::Umbra));
        let cave_e = BiomeId::EnchantedForest.fog_cave_shallow_rgb();
        let cave_m = BiomeId::TemperateMeadow.fog_cave_shallow_rgb();
        assert!((cave_e[2] - cave_m[2]).abs() > 0.03 || (cave_e[0] - cave_m[0]).abs() > 0.03);
    }
}
