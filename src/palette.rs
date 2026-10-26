use crate::rng::Rng;
use std::f32::consts::TAU;

/// 256 cyclic colour entries in sRGB space, 0..1. Entry 255 wraps smoothly into entry 0.
pub type Palette = [[f32; 3]; 256];

/// No entry's brightest channel drops below this, so nothing goes black.
const MIN_BRIGHTNESS: f32 = 0.45;

/// A random palette in one of the given styles (see presets::COSINE etc.).
pub fn random_of(rng: &mut Rng, styles: &[u32]) -> Palette {
    let mut p = match styles[rng.below(styles.len() as u32) as usize] {
        0 => cosine(rng),
        1 => bands(rng, false),
        2 => bands(rng, true),
        3 => rainbow(rng),
        _ => guns(rng),
    };
    lift(&mut p);
    p
}


/// Brightens dark entries while keeping their hue.
fn lift(p: &mut Palette) {
    for e in p.iter_mut() {
        let m = e[0].max(e[1]).max(e[2]);
        if m < 0.02 {
            *e = [MIN_BRIGHTNESS; 3];
        } else if m < MIN_BRIGHTNESS {
            for c in e.iter_mut() {
                *c *= MIN_BRIGHTNESS / m;
            }
        }
    }
}

/// Each colour channel gets its own waveform, the way the classic
/// screensavers built palettes: ramp, fade, smooth or coarse.
fn guns(rng: &mut Rng) -> Palette {
    let mut p = [[0.0; 3]; 256];
    for k in 0..3 {
        let kind = rng.below(9);
        let freq = rng.int(1, 4);
        let ph = rng.f();
        let gain = rng.range(0.7, 1.0);
        let knots: Vec<f32> = (0..16).map(|_| rng.f()).collect();
        for (i, e) in p.iter_mut().enumerate() {
            let x = freq * t(i) + ph;
            e[k] = gain
                * match kind {
                    0..=1 => 1.0 - x.fract(),                              // ramp
                    2..=3 => (std::f32::consts::PI * x).sin().powi(2),     // fade
                    4..=6 => 0.5 + 0.5 * (TAU * x).cos(),                  // smooth
                    _ => coarse(&knots, x) * (0.5 + 0.5 * (TAU * x).cos()), // coarse
                };
        }
    }
    p
}

/// Cyclic value noise through 16 random knots.
fn coarse(knots: &[f32], x: f32) -> f32 {
    let n = knots.len();
    let y = x.rem_euclid(1.0) * n as f32;
    let i = y as usize % n;
    let f = y.fract();
    let f = f * f * (3.0 - 2.0 * f);
    knots[i] + (knots[(i + 1) % n] - knots[i]) * f
}

pub fn lerp(a: &Palette, b: &Palette, t: f32) -> Palette {
    let mut out = *a;
    for (o, c) in out.iter_mut().zip(b) {
        for k in 0..3 {
            o[k] += (c[k] - o[k]) * t;
        }
    }
    out
}

pub fn to_rgba8(p: &Palette) -> [u8; 1024] {
    let mut out = [255u8; 1024];
    for (i, c) in p.iter().enumerate() {
        for k in 0..3 {
            out[i * 4 + k] = (c[k].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
    }
    out
}

fn t(i: usize) -> f32 {
    i as f32 / 256.0
}

/// a + b*cos(2pi(c*t + d)) with whole-number c so the cycle is seamless.
fn cosine(rng: &mut Rng) -> Palette {
    let c = [rng.int(1, 3), rng.int(1, 3), rng.int(1, 3)];
    let d = [rng.f(), rng.f(), rng.f()];
    let mut p = [[0.0; 3]; 256];
    for (i, e) in p.iter_mut().enumerate() {
        for k in 0..3 {
            e[k] = 0.5 + 0.5 * (TAU * (c[k] * t(i) + d[k])).cos();
        }
    }
    p
}

/// A ring of vivid colour stops, blended softly or with hard edges.
fn bands(rng: &mut Rng, hard: bool) -> Palette {
    let n = 3 + rng.below(6) as usize;
    let stops: Vec<[f32; 3]> =
        (0..n).map(|_| hsv(rng.f(), rng.range(0.6, 1.0), rng.range(0.75, 1.0))).collect();
    let mut p = [[0.0; 3]; 256];
    for (i, e) in p.iter_mut().enumerate() {
        let x = t(i) * n as f32;
        let j = x as usize % n;
        let mut f = x.fract();
        f = if hard { smoothstep(0.8, 1.0, f) } else { f * f * (3.0 - 2.0 * f) };
        let (a, b) = (stops[j], stops[(j + 1) % n]);
        for k in 0..3 {
            e[k] = a[k] + (b[k] - a[k]) * f;
        }
    }
    p
}

/// Repeated hue sweeps with a brightness ripple.
fn rainbow(rng: &mut Rng) -> Palette {
    let reps = rng.int(1, 4);
    let vreps = rng.int(1, 6);
    let h0 = rng.f();
    let sat = rng.range(0.7, 1.0);
    let mut p = [[0.0; 3]; 256];
    for (i, e) in p.iter_mut().enumerate() {
        let v = 0.55 + 0.45 * (TAU * t(i) * vreps).cos();
        *e = hsv(h0 + t(i) * reps, sat, v);
    }
    p
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let h = h.rem_euclid(1.0) * 6.0;
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as u32 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}
