// The only per-frame GPU work: look up each pixel's phase, rotate it by the
// palette offset, and fetch the colour. This is the modern stand-in for the
// 8-bit hardware palette that the old screensavers relied on.

struct View {
    size: vec2<u32>,
    scale: u32,
    offset: f32,
    // How far the front buffer has faded in over the back one, 0..1.
    blend: f32,
    _p0: f32,
    _p1: f32,
    _p2: f32,
};

@group(0) @binding(0) var<uniform> V: View;
@group(0) @binding(1) var<storage, read> phase: array<u32>;
@group(0) @binding(2) var pal: texture_2d<f32>;
// Repeat addressing wraps the palette; linear filtering blends neighbours.
@group(0) @binding(3) var pal_sampler: sampler;
// The previous background, fading out during a transition.
@group(0) @binding(4) var<storage, read> back: array<u32>;

fn color(ph: u32) -> vec4<f32> {
    let x = f32(ph & 0xFFFFu) / 65536.0;
    // Entry i sits at texel centre (i + 0.5) / 256.
    return textureSampleLevel(pal, pal_sampler, vec2<f32>(x + V.offset + 0.5 / 256.0, 0.5), 0.0);
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
    let front = color(phase[i]);
    // Outside transitions the back buffer is not read at all.
    if (V.blend >= 1.0) {
        return front;
    }
    return mix(color(back[i]), front, V.blend);
}
