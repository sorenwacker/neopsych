//! 120 presets: 24 families of hand-tuned recipes, each in 5 variations.
//! A variation is the family recipe played with its own fixed seed, so
//! preset 37 looks the same every time it comes round.

/// Shapes, see `stamp` in field.wgsl.
pub const RING: u32 = 0;
pub const DISC: u32 = 1;
pub const SQUARE: u32 = 2;
pub const STICK: u32 = 3;
pub const TILE: u32 = 4;
pub const FLOWER: u32 = 5;
pub const STAR: u32 = 6;
pub const POLYGON: u32 = 7;
pub const SLAB: u32 = 8; // filled polygon
pub const CROSS: u32 = 9;
/// One cell of glyph rain: a random blocky glyph on a fixed grid. Only used
/// for rain drops, which the director adds to some presets.
pub const GLYPH: u32 = 10;
const ALL_SHAPES: &[u32] = &[RING, DISC, SQUARE, STICK, TILE, FLOWER, STAR, POLYGON, SLAB, CROSS];

/// Background patterns, see `field` in field.wgsl.
pub const PLASMA: u32 = 0;
pub const PETALS: u32 = 1;
pub const SPIRAL: u32 = 2;
pub const MOIRE: u32 = 3;
pub const KALEIDO: u32 = 4;
pub const GRID: u32 = 5;
pub const TUNNEL: u32 = 6;
/// A picture from the image folder, mapped onto the palette by brightness.
/// Never listed in a recipe; the director swaps it in by chance.
pub const IMAGE: u32 = 7;
const ALL_FIELDS: &[u32] = &[PLASMA, PETALS, SPIRAL, MOIRE, KALEIDO, GRID, TUNNEL];

/// Palette styles, see palette::random_of.
pub const COSINE: u32 = 0;
pub const SOFT_BANDS: u32 = 1;
pub const HARD_BANDS: u32 = 2;
pub const RAINBOW: u32 = 3;
pub const GUNS: u32 = 4;
const ALL_PALETTES: &[u32] = &[COSINE, SOFT_BANDS, HARD_BANDS, RAINBOW, GUNS];

#[derive(Clone, Copy, PartialEq)]
pub enum MotionStyle {
    Bounce,
    Orbit,
    Mixed,
}

pub struct Recipe {
    pub name: &'static str,
    pub fields: &'static [u32],
    pub shapes: &'static [u32],
    /// One shape for every object in a variation, rather than a mix.
    pub matched: bool,
    pub count: (u32, u32),
    pub motion: MotionStyle,
    /// Bounce speed in screen heights per second.
    pub speed: (f32, f32),
    pub radius: (f32, f32),
    pub width: (f32, f32),
    pub wobble: f32,
    /// Palette cycles per second each object's colour advances.
    pub color_rate: (f32, f32),
    /// Chance of a colour gradient across each shape.
    pub grad: f32,
    /// Sides of polygons and petals of flowers and stars.
    pub sides: (i32, i32),
    pub palettes: &'static [u32],
    /// Palette rotation speed, cycles per second.
    pub cycle: (f32, f32),
}

const BASE: Recipe = Recipe {
    name: "",
    fields: ALL_FIELDS,
    shapes: ALL_SHAPES,
    matched: false,
    count: (3, 6),
    motion: MotionStyle::Mixed,
    speed: (0.15, 0.6),
    radius: (0.04, 0.25),
    width: (0.006, 0.03),
    wobble: 0.4,
    color_rate: (0.15, 1.0),
    grad: 0.5,
    sides: (3, 8),
    palettes: ALL_PALETTES,
    cycle: (0.03, 0.25),
};

pub const FAMILIES: &[Recipe] = &[
    Recipe { name: "Ring Dance", shapes: &[RING], count: (3, 6), motion: MotionStyle::Bounce, ..BASE },
    Recipe { name: "Disc Bloom", shapes: &[DISC], count: (2, 5), motion: MotionStyle::Orbit, grad: 0.9, ..BASE },
    Recipe { name: "Square Waves", shapes: &[SQUARE], count: (4, 8), motion: MotionStyle::Bounce, ..BASE },
    Recipe {
        name: "Stick Swarm",
        shapes: &[STICK],
        count: (6, 12),
        motion: MotionStyle::Bounce,
        speed: (0.3, 0.9),
        radius: (0.05, 0.18),
        ..BASE
    },
    Recipe {
        name: "Flower Garden",
        shapes: &[FLOWER],
        count: (3, 6),
        motion: MotionStyle::Orbit,
        wobble: 0.8,
        grad: 0.9,
        ..BASE
    },
    Recipe { name: "Star Field", shapes: &[STAR], count: (4, 8), motion: MotionStyle::Bounce, ..BASE },
    Recipe { name: "Tile Storm", shapes: &[TILE], count: (3, 6), grad: 0.8, ..BASE },
    Recipe { name: "Mixed Bag", count: (5, 10), ..BASE },
    Recipe {
        name: "Lone Wanderer",
        count: (1, 1),
        motion: MotionStyle::Orbit,
        radius: (0.2, 0.4),
        width: (0.02, 0.05),
        ..BASE
    },
    Recipe {
        name: "Twin Orbits",
        shapes: &[DISC, FLOWER],
        matched: true,
        count: (2, 2),
        motion: MotionStyle::Orbit,
        radius: (0.15, 0.35),
        grad: 1.0,
        ..BASE
    },
    Recipe {
        name: "Plasma Drift",
        fields: &[PLASMA],
        shapes: &[RING, DISC],
        count: (1, 3),
        radius: (0.03, 0.12),
        palettes: &[COSINE, SOFT_BANDS, GUNS],
        ..BASE
    },
    Recipe { name: "Kaleido Rings", fields: &[KALEIDO], shapes: &[RING], count: (2, 5), ..BASE },
    Recipe {
        name: "Spiral Painters",
        fields: &[SPIRAL],
        shapes: &[STICK],
        count: (3, 7),
        motion: MotionStyle::Orbit,
        ..BASE
    },
    Recipe {
        name: "Moire Bubbles",
        fields: &[MOIRE],
        shapes: &[DISC],
        count: (3, 8),
        radius: (0.03, 0.15),
        ..BASE
    },
    Recipe { name: "Tunnel Flowers", fields: &[TUNNEL], shapes: &[FLOWER, STAR], count: (2, 5), ..BASE },
    Recipe {
        name: "Grid Sparks",
        fields: &[GRID],
        shapes: &[DISC, TILE],
        count: (8, 12),
        motion: MotionStyle::Bounce,
        speed: (0.4, 1.0),
        radius: (0.01, 0.04),
        grad: 0.0,
        ..BASE
    },
    Recipe {
        name: "Halo Chain",
        shapes: &[RING],
        count: (9, 12),
        motion: MotionStyle::Orbit,
        radius: (0.02, 0.08),
        ..BASE
    },
    Recipe {
        name: "Pulse Hearts",
        fields: &[PETALS, KALEIDO],
        shapes: &[FLOWER],
        count: (2, 4),
        radius: (0.12, 0.3),
        wobble: 1.0,
        grad: 1.0,
        ..BASE
    },
    Recipe {
        name: "Neon Trails",
        shapes: &[RING, STICK],
        count: (4, 8),
        width: (0.003, 0.01),
        color_rate: (0.8, 2.0),
        palettes: &[HARD_BANDS, RAINBOW],
        cycle: (0.15, 0.4),
        ..BASE
    },
    Recipe {
        name: "Calm Tides",
        fields: &[PLASMA, PETALS, TUNNEL],
        shapes: &[DISC],
        count: (2, 3),
        motion: MotionStyle::Orbit,
        speed: (0.05, 0.2),
        radius: (0.25, 0.45),
        color_rate: (0.05, 0.25),
        grad: 1.0,
        palettes: &[COSINE, SOFT_BANDS],
        cycle: (0.01, 0.06),
        ..BASE
    },
    Recipe {
        name: "Triangle Tumble",
        shapes: &[POLYGON, SLAB],
        count: (3, 7),
        motion: MotionStyle::Bounce,
        sides: (3, 3),
        ..BASE
    },
    Recipe {
        name: "Hex Hive",
        shapes: &[SLAB],
        count: (4, 9),
        radius: (0.03, 0.12),
        grad: 0.9,
        sides: (6, 6),
        ..BASE
    },
    Recipe { name: "Polygon Parade", shapes: &[POLYGON, SLAB], count: (4, 8), sides: (3, 8), ..BASE },
    Recipe {
        name: "Cross Fire",
        shapes: &[CROSS],
        count: (3, 7),
        motion: MotionStyle::Bounce,
        speed: (0.25, 0.8),
        grad: 0.7,
        ..BASE
    },
];

pub const VARIATIONS: usize = 5;
pub const COUNT: usize = FAMILIES.len() * VARIATIONS;

/// Preset index 0..100 -> its family recipe, display name and fixed seed.
pub fn get(index: usize) -> (&'static Recipe, String, u64) {
    let index = index % COUNT;
    let family = &FAMILIES[index / VARIATIONS];
    let variation = index % VARIATIONS;
    let roman = ["I", "II", "III", "IV", "V"][variation];
    let seed = 0x5EED_0000 + index as u64 * 7919;
    (family, format!("{:03} {} {}", index + 1, family.name, roman), seed)
}
