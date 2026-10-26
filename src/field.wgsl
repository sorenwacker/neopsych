// Writes the pattern buffer. Each u32 holds a palette phase; the low 16 bits
// are a fraction of one full palette cycle. Runs only while a pattern is being
// revealed or a brush is stamping, never as a steady per-frame cost.

struct Params {
    size: vec2<u32>,
    origin: vec2<u32>,
    mode: u32,
    wipe: u32,
    progress: f32,
    seed: f32,
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
    shape: vec4<f32>,
    shape2: vec4<f32>,
};

@group(0) @binding(0) var<uniform> P: Params;
@group(0) @binding(1) var<storage, read_write> phase: array<u32>;
// Greyscale picture for the image field.
@group(0) @binding(2) var picture: texture_2d<f32>;
@group(0) @binding(3) var picture_sampler: sampler;

const TAU: f32 = 6.28318530718;
const BRUSH_MODE: u32 = 100u;

fn hash(p: vec2<u32>, s: u32) -> f32 {
    var h = p.x * 1664525u + p.y * 1013904223u + s * 2654435761u;
    h ^= h >> 16u;
    h *= 2246822519u;
    h ^= h >> 13u;
    h *= 3266489917u;
    h ^= h >> 16u;
    return f32(h >> 8u) / 16777216.0;
}

fn rot(p: vec2<f32>, a: f32) -> vec2<f32> {
    let c = cos(a);
    let s = sin(a);
    return vec2<f32>(c * p.x - s * p.y, s * p.x + c * p.y);
}

// Pattern value in palette cycles; only the fractional part matters.
// Multipliers of angle/TAU are whole numbers so there is no seam.
fn field(p: vec2<f32>, px: vec2<u32>) -> f32 {
    switch P.mode {
        case 0u: {
            return (sin(p.x * P.a.x + P.a.y) + sin(p.y * P.a.z + P.a.w)
                + sin((p.x + p.y) * P.b.x + P.b.y) + sin(length(p - P.b.zw) * P.c.x)) * P.c.y;
        }
        case 1u: {
            let q = p - P.a.xy;
            let ang = atan2(q.y, q.x);
            return length(q) * P.a.z + ang / TAU * P.a.w + sin(ang * P.b.x) * P.b.y;
        }
        case 2u: {
            let q = p - P.a.xy;
            let r = max(length(q), 1e-4);
            return log(r) * P.a.z + atan2(q.y, q.x) / TAU * P.a.w + r * P.b.x;
        }
        case 3u: {
            return (length(p - P.a.xy) + length(p - P.a.zw)) * P.b.x + length(p - P.b.yz) * P.b.w;
        }
        case 4u: {
            let r = length(p);
            let sector = TAU / P.a.x;
            let ang = abs(fract(atan2(p.y, p.x) / sector + 0.5) - 0.5) * sector;
            let q = r * vec2<f32>(cos(ang), sin(ang));
            return (sin(q.x * P.a.y + P.a.w) + sin(q.y * P.a.z) + sin(r * P.b.x)) * P.b.y;
        }
        case 5u: {
            let q = rot(p, P.a.w);
            return sin(q.x * P.a.x) * sin(q.y * P.a.x) * P.a.y + length(p) * P.a.z;
        }
        case 6u: {
            let q = p + P.b.z * vec2<f32>(sin(p.y * P.a.x + P.a.y), sin(p.x * P.a.z + P.a.w));
            return length(q) * P.b.x + atan2(q.y, q.x) / TAU * P.b.y;
        }
        // picture, stretched to fill the screen: brightness picks the palette entry.
        // a.x = cycles across black..white, a.y = start.
        default: {
            let uv = (vec2<f32>(px) + 0.5) / vec2<f32>(P.size);
            let lum = textureSampleLevel(picture, picture_sampler, uv, 0.0).r;
            return lum * P.a.x + P.a.y;
        }
    }
}

fn revealed(px: vec2<u32>, p: vec2<f32>) -> bool {
    if (P.progress >= 1.0) {
        return true;
    }
    switch P.wipe {
        case 0u: { return hash(px, u32(P.seed)) < P.progress; }
        case 1u: { return length(p) / 2.1 < P.progress; }
        case 2u: { return (atan2(p.y, p.x) + 3.14159265) / TAU < P.progress; }
        default: { return fract(p.y * 6.0) < P.progress; }
    }
}

// Object stamp at p: x = inside (1) or outside (0), y = 0..1 position across
// the shape, used to spread a colour gradient over it.
// shape = (x, y, radius, width), shape2 = (kind, colour, angle, gradient),
// c = (lobes, additive, -, -).
fn stamp(p: vec2<f32>) -> vec2<f32> {
    let s = P.shape;
    let q = p - s.xy;
    let r = length(q);
    switch u32(P.shape2.x) {
        // ring outline
        case 0u: {
            let d = abs(r - s.z);
            return vec2<f32>(select(0.0, 1.0, d < s.w), d / s.w);
        }
        // filled disc
        case 1u: {
            return vec2<f32>(select(0.0, 1.0, r < s.z), r / s.z);
        }
        // square outline
        case 2u: {
            let u = rot(q, P.shape2.z);
            let d = abs(max(abs(u.x), abs(u.y)) - s.z);
            return vec2<f32>(select(0.0, 1.0, d < s.w), d / s.w);
        }
        // spinning stick, radius is half its length
        case 3u: {
            let dir = vec2<f32>(cos(P.shape2.z), sin(P.shape2.z));
            let h = clamp(dot(q, dir), -s.z, s.z);
            let d = length(q - dir * h);
            return vec2<f32>(select(0.0, 1.0, d < s.w), abs(h) / s.z);
        }
        // filled square
        case 4u: {
            let u = rot(q, P.shape2.z);
            let m = max(abs(u.x), abs(u.y));
            return vec2<f32>(select(0.0, 1.0, m < s.z), m / s.z);
        }
        // filled flower
        case 5u: {
            let ang = atan2(q.y, q.x) + P.shape2.z;
            let edge = s.z * (0.55 + 0.45 * cos(ang * P.c.x));
            return vec2<f32>(select(0.0, 1.0, r < edge), r / s.z);
        }
        // regular polygon, outline (7) or filled (8), with c.x sides.
        // m is the polygon's own "radius": exactly s.z on its edge.
        case 7u, 8u: {
            let n = max(P.c.x, 3.0);
            let sector = TAU / n;
            let a = atan2(q.y, q.x) + P.shape2.z;
            let off = a - sector * (floor(a / sector) + 0.5);
            let m = r * cos(off) / cos(3.14159265 / n);
            if (u32(P.shape2.x) == 7u) {
                let d = abs(m - s.z);
                return vec2<f32>(select(0.0, 1.0, d < s.w), d / s.w);
            }
            return vec2<f32>(select(0.0, 1.0, m < s.z), m / s.z);
        }
        // plus-sign cross with arms a third as thick as they are long
        case 9u: {
            let u = abs(rot(q, P.shape2.z));
            let t = s.z * 0.33;
            let inside = (u.x < t && u.y < s.z) || (u.y < t && u.x < s.z);
            return vec2<f32>(select(0.0, 1.0, inside), max(u.x, u.y) / s.z);
        }
        // glyph rain: the cell of a fixed grid that holds the head, filled
        // with a random 5x7 bitmap picked by c.x. Radius is half the cell height.
        case 10u: {
            let size = vec2<f32>(s.z * 1.3, s.z * 2.0);
            let cell = floor(p / size);
            if (any(cell != floor(s.xy / size))) {
                return vec2<f32>(0.0, 0.0);
            }
            let local = (p - cell * size) / size;
            let g = vec2<f32>((local.x - 0.12) / 0.76 * 5.0, (local.y - 0.08) / 0.84 * 7.0);
            if (g.x < 0.0 || g.y < 0.0 || g.x >= 5.0 || g.y >= 7.0) {
                return vec2<f32>(0.0, 0.0);
            }
            let bit = hash(vec2<u32>(g), u32(P.c.x)) < 0.45;
            return vec2<f32>(select(0.0, 1.0, bit), 0.0);
        }
        // star outline
        default: {
            let ang = atan2(q.y, q.x) + P.shape2.z;
            let d = abs(r - s.z * (0.55 + 0.45 * cos(ang * P.c.x)));
            return vec2<f32>(select(0.0, 1.0, d < s.w), d / s.w);
        }
    }
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let px = gid.xy + P.origin;
    if (px.x >= P.size.x || px.y >= P.size.y) {
        return;
    }
    let i = px.y * P.size.x + px.x;
    let p = (vec2<f32>(px) + 0.5 - vec2<f32>(P.size) * 0.5) / (f32(P.size.y) * 0.5);

    if (P.mode >= BRUSH_MODE) {
        let st = stamp(p);
        if (st.x > 0.0) {
            let v = u32(fract(P.shape2.y + st.y * P.shape2.w) * 65536.0);
            if (P.c.y > 0.0) {
                phase[i] = phase[i] + v;
            } else {
                phase[i] = v;
            }
        }
        return;
    }
    if (revealed(px, p)) {
        phase[i] = u32(fract(field(p, px)) * 65536.0) & 0xFFFFu;
    }
}
