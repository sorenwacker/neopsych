//! Draws assets/neopsych.ico: a rainbow spiral with a star in the middle, in
//! the screensaver's own palette style. Every size is rendered on its own,
//! supersampled, so the small ones stay crisp.
//!
//!     cargo run --example make_icon

use image::{codecs::png::PngEncoder, ImageEncoder};
use std::f32::consts::TAU;

const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
/// Samples per pixel along each axis.
const SS: u32 = 4;

fn main() {
    let pngs: Vec<(u32, Vec<u8>)> = SIZES.iter().map(|&s| (s, png(&render(s), s))).collect();

    // ICO: header, one directory entry per size, then the PNG data.
    let mut ico = Vec::new();
    ico.extend([0, 0, 1, 0]);
    ico.extend((pngs.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * pngs.len() as u32;
    for (size, data) in &pngs {
        let side = if *size >= 256 { 0 } else { *size as u8 };
        ico.extend([side, side, 0, 0]);
        ico.extend(1u16.to_le_bytes()); // colour planes
        ico.extend(32u16.to_le_bytes()); // bits per pixel
        ico.extend((data.len() as u32).to_le_bytes());
        ico.extend(offset.to_le_bytes());
        offset += data.len() as u32;
    }
    for (_, data) in &pngs {
        ico.extend(data);
    }
    std::fs::create_dir_all("assets").unwrap();
    std::fs::write("assets/neopsych.ico", ico).unwrap();
    std::fs::write("assets/neopsych.png", &pngs.last().unwrap().1).unwrap();
}

fn png(rgba: &[u8], size: u32) -> Vec<u8> {
    let mut out = Vec::new();
    PngEncoder::new(&mut out).write_image(rgba, size, size, image::ExtendedColorType::Rgba8).unwrap();
    out
}

fn render(size: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let mut sum = [0.0f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let u = (x as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32 * 2.0 - 1.0;
                    let v = (y as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32 * 2.0 - 1.0;
                    if let Some(c) = sample(u, v) {
                        for k in 0..3 {
                            sum[k] += c[k];
                        }
                        sum[3] += 1.0;
                    }
                }
            }
            let n = (SS * SS) as f32;
            let i = ((y * size + x) * 4) as usize;
            if sum[3] > 0.0 {
                for k in 0..3 {
                    rgba[i + k] = (sum[k] / sum[3] * 255.0 + 0.5) as u8;
                }
            }
            rgba[i + 3] = (sum[3] / n * 255.0 + 0.5) as u8;
        }
    }
    rgba
}

/// Colour at (u, v) in [-1, 1]^2, or None outside the rounded square.
fn sample(u: f32, v: f32) -> Option<[f32; 3]> {
    // Rounded square tile.
    let (half, corner) = (0.94, 0.32);
    let q = [(u.abs() - (half - corner)).max(0.0), (v.abs() - (half - corner)).max(0.0)];
    if q[0].hypot(q[1]) > corner {
        return None;
    }
    let r = u.hypot(v);
    let a = v.atan2(u);
    // Three-armed spiral, like the screensaver's spiral background.
    let phase = 1.6 * (r + 0.05).ln() + 3.0 * a / TAU;
    // A bold white six-pointed star in the middle, outlined so it reads on
    // every colour of the spiral.
    let star = 0.42 * (0.68 + 0.32 * (6.0 * (a + TAU / 4.0)).cos());
    if r < star {
        return Some([1.0, 1.0, 1.0]);
    }
    if r < star + 0.07 {
        return Some([0.22, 0.04, 0.32]);
    }
    Some(color(phase))
}

/// A rainbow cosine palette, lifted so nothing gets dark (see palette.rs).
fn color(phase: f32) -> [f32; 3] {
    let mut c = [0.0, 1.0 / 3.0, 2.0 / 3.0].map(|d| 0.5 + 0.5 * (TAU * (phase + d)).cos());
    let m = c[0].max(c[1]).max(c[2]);
    if m < 0.6 {
        c = c.map(|x| x * 0.6 / m.max(0.01));
    }
    c
}
