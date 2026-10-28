// Kaleidoscope for video: the picture is cut into mirrored wedges around
// the screen centre. Each wedge shows the video from the bottom edge up
// through its middle, where people usually are, so they stay recognisable.
// The wedges turn slowly. Combines with neopsych.glsl.
//
// It always draws a 16:9 picture, as tall as the video, so square and
// portrait videos fill the screen with the pattern instead of getting black
// bars; scripts/neopsych.lua tells mpv to show it as 16:9.

//!PARAM sectors
//!DESC number of mirrored wedge pairs
//!TYPE float
//!MINIMUM 1.0
//!MAXIMUM 16.0
3.0

//!PARAM spin
//!DESC turns per minute
//!TYPE float
//!MINIMUM -20.0
//!MAXIMUM 20.0
0.5

//!PARAM zoom
//!DESC how much of the video one wedge shows; smaller = closer
//!TYPE float
//!MINIMUM 0.1
//!MAXIMUM 2.0
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
//!DESC kaleidoscope
//!WIDTH HOOKED.h 1.7777778 *
//!HEIGHT HOOKED.h

#define TAU 6.28318530718
#define SCREEN_ASPECT 1.7777778
// mpv has no clock for shaders, only a frame counter; the script tells us
// how many frames it draws per second.
#define SECONDS (float(frame) / rate + offset)

vec4 hook() {
    float t = SECONDS;
    // The video's own proportions, so what it shows isn't stretched.
    vec2 aspect = vec2(HOOKED_size.x / HOOKED_size.y, 1.0);
    vec2 p = (HOOKED_pos - 0.5) * vec2(SCREEN_ASPECT, 1.0);
    float r = length(p);
    float wedge = TAU / (2.0 * sectors);
    // Fold the angle into one wedge, mirroring every other one.
    float a = mod(atan(p.y, p.x) + t * spin * TAU / 60.0, 2.0 * wedge);
    a = abs(a - wedge);
    // Aim the wedge straight up the video (y grows downwards).
    a += -0.5 * TAU / 2.0 - 0.5 * wedge;
    vec2 q = r * vec2(cos(a), sin(a)) * zoom;
    // The wedge's tip sits near the bottom edge, so the wedge reaches up
    // through the middle of the video. It sways a little.
    vec2 centre = vec2(0.5, 0.95) + 0.04 * vec2(sin(t * 0.07), sin(t * 0.053 + 1.0));
    vec2 uv = centre + q / aspect;
    // Mirror at the edges instead of running off the picture.
    uv = 1.0 - abs(1.0 - mod(uv, 2.0));
    return HOOKED_tex(uv);
}
