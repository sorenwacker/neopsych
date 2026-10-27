// The only per-frame GPU work: look up each pixel's phase, rotate it by the
// palette offset, and fetch the colour. This is the modern stand-in for the
// 8-bit hardware palette that the old screensavers relied on.

struct View {
    size: vec2<u32>,
    scale: u32,
    offset: f32,
    // How far the front buffer has faded in over the back one, 0..1.
    blend: f32,
    // On-screen message: how visible (0 = none), and where: top-left corner
    // in screen pixels, screen pixels per texel, and its width in texels.
    osd_alpha: f32,
    _p0: f32,
    _p1: f32,
    osd_rect: vec4<f32>,
};

@group(0) @binding(0) var<uniform> V: View;
@group(0) @binding(1) var<storage, read> phase: array<u32>;
@group(0) @binding(2) var pal: texture_2d<f32>;
// Repeat addressing wraps the palette; linear filtering blends neighbours.
@group(0) @binding(3) var pal_sampler: sampler;
// The previous background, fading out during a transition.
@group(0) @binding(4) var<storage, read> back: array<u32>;
// The message text, see osd.rs: 1 = glyph, 0.5 = outline.
@group(0) @binding(5) var osd: texture_2d<f32>;

// Rows in the message bitmap; osd::HEIGHT.
const OSD_ROWS: f32 = 9.0;

fn color(ph: u32) -> vec4<f32> {
    let x = f32(ph & 0xFFFFu) / 65536.0;
    // Entry i sits at texel centre (i + 0.5) / 256.
    return textureSampleLevel(pal, pal_sampler, vec2<f32>(x + V.offset + 0.5 / 256.0, 0.5), 0.0);
}

fn message(c: vec4<f32>, pos: vec2<f32>) -> vec4<f32> {
    if (V.osd_alpha <= 0.0) {
        return c;
    }
    let t = (pos - V.osd_rect.xy) / V.osd_rect.z;
    if (t.x < 0.0 || t.y < 0.0 || t.x >= V.osd_rect.w || t.y >= OSD_ROWS) {
        return c;
    }
    let v = textureLoad(osd, vec2<i32>(t), 0).r;
    if (v > 0.75) {
        return mix(c, vec4<f32>(1.0), V.osd_alpha);
    }
    if (v > 0.25) {
        return mix(c, vec4<f32>(0.0, 0.0, 0.0, 1.0), V.osd_alpha * 0.85);
    }
    return c;
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let px = min(vec2<u32>(pos.xy) / V.scale, V.size - 1u);
    let i = px.y * V.size.x + px.x;
    var c = color(phase[i]);
    // Outside transitions the back buffer is not read at all.
    if (V.blend < 1.0) {
        c = mix(color(back[i]), c, V.blend);
    }
    return message(c, pos.xy);
}
