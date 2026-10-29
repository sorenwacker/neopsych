// neopsych colour cycling for video: every pixel's brightness picks a colour
// from a palette that keeps rotating, and the palette itself keeps blending
// into new random ones, the way neopsych does it (see palette.rs there).
//
// Tune while playing, for example:
//   mpv --glsl-shader-opts=speed=0.2,bands=3,period=8 video.mp4

//!PARAM speed
//!DESC palette turns per second
//!TYPE float
//!MINIMUM 0.0
//!MAXIMUM 5.0
0.12

//!PARAM bands
//!DESC palette cycles from black to white
//!TYPE float
//!MINIMUM 0.1
//!MAXIMUM 10.0
1.5

//!PARAM period
//!DESC seconds to blend into the next palette
//!TYPE float
//!MINIMUM 1.0
//!MAXIMUM 120.0
12.0

//!PARAM denoise
//!DESC pixels between brightness samples that are averaged; 0 = off
//!TYPE float
//!MINIMUM 0.0
//!MAXIMUM 4.0
1.5

//!PARAM amount
//!DESC 0 = original video, 1 = full effect
//!TYPE float
//!MINIMUM 0.0
//!MAXIMUM 1.0
1.0

//!PARAM rate
//!DESC frames drawn per second, set by scripts/neopsych.lua
//!TYPE float
//!MINIMUM 1.0
//!MAXIMUM 1000.0
60.0

//!PARAM offset
//!DESC seconds added to the clock, set by scripts/neopsych.lua
//!TYPE float
0.0

//!HOOK MAIN
//!BIND HOOKED
//!DESC neopsych colour cycling

#define TAU 6.28318530718
// mpv has no clock for shaders, only a frame counter; the script tells us
// how many frames it draws per second.
#define SECONDS (float(frame) / rate + offset)
// No palette entry's brightest channel drops below this, so nothing goes black.
#define MIN_BRIGHTNESS 0.45

// Random number in [0, 1) for palette n, value i.
float rnd(float n, float i) {
    return fract(sin(n * 12.9898 + i * 78.233 + 0.5) * 43758.5453);
}

float whole(float n, float i, float lo, float hi) {
    return lo + floor(rnd(n, i) * (hi - lo + 1.0));
}

vec3 hsv(float h, float s, float v) {
    vec3 k = clamp(abs(fract(h + vec3(0.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0) - 1.0, 0.0, 1.0);
    return v * mix(vec3(1.0), k, s);
}

// a + b*cos(2pi(c*x + d)) with whole-number c so the cycle is seamless.
vec3 cosine(float n, float x) {
    vec3 c = vec3(whole(n, 1.0, 1.0, 3.0), whole(n, 2.0, 1.0, 3.0), whole(n, 3.0, 1.0, 3.0));
    vec3 d = vec3(rnd(n, 4.0), rnd(n, 5.0), rnd(n, 6.0));
    return 0.5 + 0.5 * cos(TAU * (c * x + d));
}

// A ring of 3 to 8 vivid colour stops, blended softly or with hard edges.
vec3 colour_bands(float n, float x, bool hard) {
    float count = whole(n, 1.0, 3.0, 8.0);
    float y = x * count;
    float j = mod(floor(y), count);
    float k = mod(j + 1.0, count);
    float f = fract(y);
    // Hard edges are a little softer than in neopsych: on video a sharp
    // edge makes noisy pixels flicker between two colours.
    f = hard ? smoothstep(0.6, 1.0, f) : smoothstep(0.0, 1.0, f);
    vec3 a = hsv(rnd(n, 10.0 + j), 0.6 + 0.4 * rnd(n, 20.0 + j), 0.75 + 0.25 * rnd(n, 30.0 + j));
    vec3 b = hsv(rnd(n, 10.0 + k), 0.6 + 0.4 * rnd(n, 20.0 + k), 0.75 + 0.25 * rnd(n, 30.0 + k));
    return mix(a, b, f);
}

// Repeated hue sweeps with a brightness ripple.
vec3 rainbow(float n, float x) {
    float reps = whole(n, 1.0, 1.0, 4.0);
    float vreps = whole(n, 2.0, 1.0, 6.0);
    float v = 0.55 + 0.45 * cos(TAU * x * vreps);
    return hsv(rnd(n, 3.0) + x * reps, 0.7 + 0.3 * rnd(n, 4.0), v);
}

// Each colour channel gets its own waveform, the way the classic
// screensavers built palettes: zigzag, fade or smooth. (neopsych's ramp
// drops abruptly, which flickers on video, so it is a zigzag here.)
vec3 guns(float n, float x) {
    vec3 c;
    for (int k = 0; k < 3; k++) {
        float s = 10.0 * float(k);
        float kind = rnd(n, s + 1.0);
        float y = whole(n, s + 2.0, 1.0, 4.0) * x + rnd(n, s + 3.0);
        float gain = 0.7 + 0.3 * rnd(n, s + 4.0);
        float w = kind < 0.25 ? abs(2.0 * fract(y) - 1.0)
                : kind < 0.5 ? pow(sin(3.14159265 * y), 2.0)
                : 0.5 + 0.5 * cos(TAU * y);
        c[k] = gain * w;
    }
    return c;
}

// Palette number n at position x (in cycles), in one of five styles.
vec3 palette(float n, float x) {
    x = fract(x);
    float style = floor(rnd(n, 0.0) * 5.0);
    vec3 c = style < 1.0 ? cosine(n, x)
           : style < 2.0 ? colour_bands(n, x, false)
           : style < 3.0 ? colour_bands(n, x, true)
           : style < 4.0 ? rainbow(n, x)
           : guns(n, x);
    // Brighten dark entries while keeping their hue.
    float m = max(c.r, max(c.g, c.b));
    return m < 0.02 ? vec3(MIN_BRIGHTNESS) : c * max(1.0, MIN_BRIGHTNESS / m);
}

float luma(vec3 c) {
    return dot(c, vec3(0.2126, 0.7152, 0.0722));
}

// How far the palette has turned, in cycles. It turns at `speed` on average,
// but a mix of two slow waves at unrelated frequencies makes it speed up to
// about three times that, slow down, sometimes stop or turn back, and never
// repeat exactly, like neopsych's changes of pace. This is the integral of
// 2 speed (0.5 + 0.7 sin(0.11 t) + 0.4 sin(0.047 t + 1.3)).
float rotation(float t) {
    return 2.0 * speed * (0.5 * t - 0.7 / 0.11 * cos(0.11 * t) - 0.4 / 0.047 * cos(0.047 * t + 1.3));
}

// Counts palettes: one per `period` seconds on average, but drifting
// between quicker and slower changes. Always increasing, since the wobble's
// slope (0.35 * 0.13 + 0.2 * 0.071) stays below 1 / 12.
float palette_clock(float t) {
    return t / period + (12.0 / period) * (0.35 * sin(0.13 * t) + 0.2 * sin(0.071 * t + 2.0));
}

vec4 hook() {
    vec4 src = HOOKED_tex(HOOKED_pos);
    float t = SECONDS;
    // Brightness averaged over a few neighbouring pixels, so compression
    // noise doesn't make single pixels flicker between colours.
    float lum = 0.0;
    for (int dy = -1; dy <= 1; dy++) {
        for (int dx = -1; dx <= 1; dx++) {
            float w = (dx == 0 ? 2.0 : 1.0) * (dy == 0 ? 2.0 : 1.0);
            lum += w * luma(HOOKED_texOff(vec2(dx, dy) * denoise).rgb);
        }
    }
    lum /= 16.0;
    float x = lum * bands + rotation(t);
    // Always on the way from palette n to palette n + 1, at a pace that
    // keeps drifting instead of a fixed beat.
    float u = palette_clock(t);
    float n = floor(u);
    float k = smoothstep(0.0, 1.0, fract(u));
    vec3 col = mix(palette(n, x), palette(n + 1.0, x), k);
    return vec4(mix(src.rgb, col, amount), src.a);
}
