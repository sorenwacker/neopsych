//! Decides what is on screen: the background pattern, the objects drifting
//! over it, the palette and how fast it cycles, all following the current
//! preset. Everything here is resolution independent; the renderer maps it
//! to pixels.

use crate::palette::{self, Palette};
use crate::presets::{self, MotionStyle, Recipe};
use crate::rng::Rng;
use std::f32::consts::TAU;

pub const MAX_OBJECTS: usize = 12;
/// Seconds between glyph rain showers, and how long one lasts.
const RAIN_EVERY: (f32, f32) = (420.0, 780.0);
const RAIN_LASTS: (f32, f32) = (20.0, 40.0);
/// Where a finished rain drop is parked until it is removed.
const GONE: f32 = 100.0;
/// Every object can be drawn up to this many times by symmetry.
pub const MAX_COPIES: usize = 12;

/// Synchronised copies of every object around the screen centre: `turns`
/// rotated copies, each also mirrored when `mirror` is set. With mirroring
/// that is `turns` mirror axes, the first one vertical (left/right).
#[derive(Clone, Copy)]
pub struct Symmetry {
    pub turns: u32,
    pub mirror: bool,
}

impl Symmetry {
    pub fn copies(&self) -> usize {
        self.turns as usize * if self.mirror { 2 } else { 1 }
    }
}

/// A full-screen pattern, blended in over the old one while progress goes 0 -> 1.
#[derive(Clone, Copy)]
pub struct FieldJob {
    pub mode: u32,
    /// New for every background, so the renderer knows when to draw one.
    pub id: u32,
    pub progress: f32,
    pub seed: f32,
    pub a: [f32; 4],
    pub b: [f32; 4],
    pub c: [f32; 4],
}

/// One object's stamp for this frame. Position is in [-1, 1]^2; the
/// renderer stretches x to the screen's aspect ratio.
#[derive(Clone, Copy)]
pub struct Brush {
    pub shape: u32,
    pub pos: [f32; 2],
    pub radius: f32,
    pub width: f32,
    pub angle: f32,
    /// Palette position written at the shape's centre or edge, in cycles.
    pub color: f32,
    /// Palette cycles spread across the shape, for concentric bands.
    pub grad: f32,
    pub lobes: f32,
    /// true: add to the pattern underneath instead of overwriting it.
    pub additive: bool,
}

pub struct Frame {
    pub field: Option<FieldJob>,
    pub brushes: Vec<Brush>,
    /// The current pattern at progress 1, for windows that need a full redraw.
    pub base: FieldJob,
    pub palette: [u8; 1024],
    /// Palette rotation, 0..1 of a full cycle.
    pub offset: f32,
    /// The field is a new picture: upload the next image before drawing it.
    pub new_image: bool,
    pub symmetry: Symmetry,
}

enum Motion {
    /// Straight lines, bouncing off the screen edges.
    Bounce { vel: [f32; 2] },
    /// A Lissajous orbit.
    Orbit { freq: [f32; 4], phase: [f32; 4] },
    /// Falling straight down, restarting at the top in a random column.
    Rain { speed: f32, rng: Rng },
}

struct Object {
    shape: u32,
    motion: Motion,
    pos: [f32; 2],
    radius: f32,
    wobble: f32,
    wobble_freq: f32,
    width: f32,
    angle: f32,
    spin: f32,
    color: f32,
    color_rate: f32,
    grad: f32,
    lobes: f32,
    additive: bool,
    /// Seconds of movement so far, scaled by the movement speed.
    age: f32,
    /// Rain only: don't restart at the top after falling off the bottom.
    retire: bool,
}

impl Object {
    /// Advances by dt seconds of the object's own time, which runs faster or
    /// slower than the clock with the movement speed.
    fn step(&mut self, dt: f32) -> Brush {
        self.age += dt;
        let s = self.age;
        match &mut self.motion {
            Motion::Bounce { vel } => {
                for k in 0..2 {
                    self.pos[k] += vel[k] * dt;
                    if self.pos[k].abs() > 1.0 {
                        self.pos[k] = self.pos[k].clamp(-1.0, 1.0);
                        vel[k] = -vel[k];
                    }
                }
            }
            Motion::Orbit { freq, phase } => {
                self.pos = [
                    0.7 * (freq[0] * s + phase[0]).sin() + 0.3 * (freq[2] * s + phase[2]).sin(),
                    0.7 * (freq[1] * s + phase[1]).sin() + 0.3 * (freq[3] * s + phase[3]).sin(),
                ];
            }
            Motion::Rain { speed, rng } => {
                self.pos[1] += *speed * dt;
                if self.pos[1] > 1.05 {
                    if self.retire {
                        // Fell off the bottom while the shower is ending: gone.
                        self.pos[1] = GONE;
                    } else {
                        self.pos = [rng.range(-1.0, 1.0), -1.05 - rng.range(0.0, 0.5)];
                        *speed = rng.range(0.3, 1.3);
                    }
                }
                // A new glyph every frame makes the head flicker.
                self.lobes = rng.below(1 << 16) as f32;
            }
        }
        self.angle += self.spin * dt;
        self.color = (self.color + self.color_rate * dt).rem_euclid(1.0);
        Brush {
            shape: self.shape,
            pos: self.pos,
            radius: self.radius * (1.0 + self.wobble * (self.wobble_freq * s).sin()),
            width: self.width,
            angle: self.angle,
            color: self.color,
            grad: self.grad,
            lobes: self.lobes,
            additive: self.additive,
        }
    }
}

pub struct Director {
    /// Picks the next preset; seeded from the clock.
    order: Rng,
    /// Drives everything inside a preset; seeded from the preset.
    rng: Rng,
    speed_mult: f32,
    motion_mult: f32,
    preset_seconds: f32,
    /// Stay on this preset instead of rotating.
    fixed: Option<usize>,
    preset: usize,
    recipe: &'static Recipe,
    name: String,
    /// Shape shared by all objects when the recipe is matched.
    theme: u32,
    symmetry: Symmetry,
    preset_start: f64,
    preset_end: f64,
    next_event: f64,
    /// When the next background transition within the preset starts.
    next_field: f64,
    transition_seconds: f32,
    field: FieldJob,
    field_start: f64,
    field_dur: f32,
    field_active: bool,
    objects: Vec<Object>,
    /// Glyph rain columns, kept apart so events never swap them for objects.
    drops: Vec<Object>,
    /// When the current shower ends, or the next one starts.
    rain_switch: f64,
    pal_from: Palette,
    pal_to: Palette,
    pal_start: f64,
    pal_dur: f32,
    offset: f64,
    speed: f32,
    speed_target: f32,
    image_chance: f32,
    /// Set by the app each frame: a decoded picture is waiting.
    pub image_ready: bool,
    new_image: bool,
}

impl Director {
    pub fn new(
        seed: u64,
        speed_mult: f32,
        motion_mult: f32,
        preset_seconds: f32,
        transition_seconds: f32,
        fixed: Option<usize>,
        image_chance: f32,
    ) -> Self {
        let mut order = Rng::new(seed);
        let preset = fixed.unwrap_or_else(|| order.below(presets::COUNT as u32) as usize);
        let (recipe, name, pseed) = presets::get(preset);
        let mut rng = Rng::new(pseed);
        let pal = palette::random_of(&mut rng, recipe.palettes);
        let field = random_field(&mut rng, recipe.fields);
        let mut d = Director {
            order,
            rng,
            speed_mult,
            motion_mult,
            preset_seconds,
            fixed,
            preset,
            recipe,
            name,
            theme: 0,
            symmetry: Symmetry { turns: 1, mirror: false },
            preset_start: 0.0,
            preset_end: 0.0,
            next_event: 0.0,
            next_field: 0.0,
            transition_seconds,
            field,
            field_start: 0.0,
            field_dur: 2.0,
            field_active: false,
            objects: Vec::new(),
            drops: Vec::new(),
            rain_switch: 0.0,
            pal_from: pal,
            pal_to: pal,
            pal_start: 0.0,
            pal_dur: 1.0,
            offset: 0.0,
            speed: 0.0,
            speed_target: 0.1,
            image_chance,
            image_ready: false,
            new_image: false,
        };
        d.start_preset(preset, 0.0);
        d.rain_switch = d.order.range(RAIN_EVERY.0, RAIN_EVERY.1) as f64;
        d
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Colour cycling and movement speed multipliers, as adjusted by hotkeys.
    pub fn speeds(&self) -> (f32, f32) {
        (self.speed_mult, self.motion_mult)
    }

    /// Steps `delta` presets forward or back from the current one.
    pub fn skip(&mut self, delta: i32, t: f64) {
        let n = presets::COUNT as i32;
        let i = (self.preset as i32 + delta).rem_euclid(n) as usize;
        if self.fixed.is_some() {
            self.fixed = Some(i);
        }
        self.start_preset(i, t);
    }

    pub fn update(&mut self, t: f64, dt: f32) -> Frame {
        if t >= self.preset_end {
            let next = self.fixed.unwrap_or_else(|| self.order.below(presets::COUNT as u32) as usize);
            self.start_preset(next, t);
        }
        if t >= self.next_event {
            self.event(t);
        }
        // Glyph rain on its own clock, independent of presets.
        if t >= self.rain_switch {
            if self.raining() {
                self.stop_rain();
                self.rain_switch = t + self.order.range(RAIN_EVERY.0, RAIN_EVERY.1) as f64;
            } else {
                self.start_rain();
                self.rain_switch = t + self.order.range(RAIN_LASTS.0, RAIN_LASTS.1) as f64;
            }
        }
        // A fresh background now and then within the preset, sometimes a picture.
        if t >= self.next_field {
            if self.image_ready && self.order.chance(self.image_chance) {
                self.show_picture(t);
            } else {
                self.new_field(t);
            }
        }
        // The palette is always on its way to the next one.
        if t >= self.pal_start + self.pal_dur as f64 {
            self.pal_from = self.pal_to;
            self.pal_to = palette::random_of(&mut self.rng, self.recipe.palettes);
            self.pal_start = t;
            self.pal_dur = self.rng.range(8.0, 16.0);
        }

        let field = if self.field_active {
            let p = ((t - self.field_start) as f32 / self.field_dur).min(1.0);
            if p >= 1.0 {
                self.field_active = false;
            }
            Some(FieldJob { progress: p * p * (3.0 - 2.0 * p), ..self.field })
        } else {
            None
        };

        self.speed += (self.speed_target - self.speed) * (dt * 0.7).min(1.0);
        self.offset = (self.offset + (self.speed * self.speed_mult * dt) as f64).rem_euclid(1.0);

        let move_dt = dt * self.motion_mult;
        let brushes = self.objects.iter_mut().chain(&mut self.drops).map(|o| o.step(move_dt)).collect();
        self.drops.retain(|d| d.pos[1] < GONE);

        Frame {
            field,
            brushes,
            base: FieldJob { progress: 1.0, ..self.field },
            palette: palette::to_rgba8(&self.current_palette(t)),
            offset: self.offset as f32,
            new_image: std::mem::take(&mut self.new_image),
            symmetry: self.symmetry,
        }
    }

    fn current_palette(&self, t: f64) -> Palette {
        let k = (((t - self.pal_start) as f32) / self.pal_dur).clamp(0.0, 1.0);
        palette::lerp(&self.pal_from, &self.pal_to, k * k * (3.0 - 2.0 * k))
    }

    fn start_preset(&mut self, index: usize, t: f64) {
        let (recipe, name, seed) = presets::get(index);
        self.preset = index;
        self.recipe = recipe;
        self.name = name;
        self.rng = Rng::new(seed);
        self.preset_start = t;
        self.preset_end = t + self.preset_seconds as f64;
        self.next_event = t + self.rng.range(4.0, 12.0) as f64;

        self.new_field(t);

        // Decided with the order rng so the preset's own sequence is unchanged.
        let picture = self.image_ready && self.order.chance(self.image_chance);
        if picture {
            // a.x: palette cycles across black..white, a.y: starting colour.
            self.field.mode = presets::IMAGE;
            self.field.a = [self.order.range(0.7, 2.0), self.order.f(), 0.0, 0.0];
            self.new_image = true;
        }

        let r = &mut self.rng;
        self.theme = recipe.shapes[r.below(recipe.shapes.len() as u32) as usize];
        let mut count = r.range(recipe.count.0 as f32, recipe.count.1 as f32 + 0.999) as usize;
        let (turns, mirror) = match r.below(40) {
            0..=5 => (1, false),   // none
            6..=11 => (1, true),   // left/right
            12..=14 => (2, false), // half turn
            15..=21 => (2, true),  // left/right and top/bottom
            22..=23 => (3, false),
            24..=27 => (3, true),
            28..=32 => (4, true),
            33..=36 => (5, true),
            _ => (6, true),
        };
        self.symmetry = Symmetry { turns, mirror };
        // Keep roughly the same amount on screen however many copies there are.
        let copies = self.symmetry.copies();
        if copies > 2 {
            count = (count * 2).div_ceil(copies).max(1);
        }
        if picture {
            // Leave the picture visible for a while.
            count = count.min(2);
        }
        let theme = self.theme;
        // Every stamp needs a slot on the GPU; see gpu::Target.
        let limit = MAX_OBJECTS * MAX_COPIES / copies;
        self.objects = (0..count.min(limit)).map(|_| random_object(r, recipe, theme)).collect();


        // Blend from whatever is showing into the preset's own palette.
        self.pal_from = self.current_palette(t);
        self.pal_to = palette::random_of(&mut self.rng, recipe.palettes);
        self.pal_start = t;
        self.pal_dur = self.rng.range(3.0, 6.0);
        self.speed_target = self.rng.sign() * self.rng.range(recipe.cycle.0, recipe.cycle.1);
    }

    /// Hotkey: sweep in a new background now.
    pub fn next_background(&mut self, t: f64) {
        self.new_field(t);
    }

    /// Hotkey: sweep in a picture now, if one is decoded.
    pub fn show_picture(&mut self, t: f64) {
        if !self.image_ready {
            return;
        }
        self.new_field(t);
        self.field.mode = presets::IMAGE;
        self.field.a = [self.order.range(0.7, 2.0), self.order.f(), 0.0, 0.0];
        self.new_image = true;
    }

    /// Hotkey: blend quickly into a new palette.
    pub fn next_palette(&mut self, t: f64) {
        self.pal_from = self.current_palette(t);
        self.pal_to = palette::random_of(&mut self.rng, self.recipe.palettes);
        self.pal_start = t;
        self.pal_dur = 2.0;
    }

    /// Hotkey: step through the symmetry options.
    pub fn next_symmetry(&mut self) {
        const ORDER: [(u32, bool); 9] =
            [(1, false), (1, true), (2, false), (2, true), (3, false), (3, true), (4, true), (5, true), (6, true)];
        let i = ORDER.iter().position(|&s| s == (self.symmetry.turns, self.symmetry.mirror)).unwrap_or(0);
        let (turns, mirror) = ORDER[(i + 1) % ORDER.len()];
        self.symmetry = Symmetry { turns, mirror };
    }

    /// Hotkey (and events): start a shower of glyph rain, or let the current
    /// one end.
    pub fn toggle_rain(&mut self) {
        if self.raining() {
            self.stop_rain();
        } else {
            self.start_rain();
        }
    }

    fn raining(&self) -> bool {
        self.drops.iter().any(|d| !d.retire)
    }

    fn start_rain(&mut self) {
        let room = (MAX_OBJECTS * MAX_COPIES / self.symmetry.copies()).saturating_sub(self.objects.len());
        let cell = self.rng.range(0.02, 0.032);
        let count = (self.rng.range(20.0, 45.0) as usize).min(room);
        self.drops = (0..count).map(|_| random_drop(&mut self.rng, cell)).collect();
    }

    /// Drops already falling finish their fall; none start again.
    fn stop_rain(&mut self) {
        for d in &mut self.drops {
            d.retire = true;
        }
    }

    /// Hotkey: run the colour cycling the other way.
    pub fn reverse(&mut self) {
        self.speed_target = -self.speed_target;
    }

    /// Hotkey: multiply how fast objects move.
    pub fn scale_motion(&mut self, factor: f32) {
        self.motion_mult = (self.motion_mult * factor).clamp(0.05, 20.0);
    }

    /// Hotkey: multiply the colour cycling speed.
    pub fn scale_speed(&mut self, factor: f32) {
        self.speed_mult = (self.speed_mult * factor).clamp(0.05, 20.0);
    }

    /// Starts revealing a new background pattern from the preset's list.
    fn new_field(&mut self, t: f64) {
        let r = &mut self.rng;
        let id = self.field.id.wrapping_add(1);
        self.field = FieldJob { id, ..random_field(r, self.recipe.fields) };
        self.field_start = t;
        self.field_dur = r.range(2.0, 5.0);
        self.field_active = true;
        self.next_field = t + (self.transition_seconds * r.range(0.67, 1.33)) as f64;
    }

    /// Small surprises within a preset: reverse the cycling, change its pace,
    /// or swap one object for a new one.
    fn event(&mut self, t: f64) {
        let r = &mut self.rng;
        self.next_event = t + r.range(4.0, 12.0) as f64;
        match r.below(3) {
            0 => self.speed_target = -self.speed_target,
            1 => self.speed_target = self.speed_target.signum() * r.range(self.recipe.cycle.0, self.recipe.cycle.1),
            _ => {
                if !self.objects.is_empty() {
                    let i = r.below(self.objects.len() as u32) as usize;
                    self.objects[i] = random_object(r, self.recipe, self.theme);
                }
            }
        }
    }
}

/// One column of glyph rain. All drops in a preset share `cell` (half a
/// glyph's height) so they fall on the same grid.
fn random_drop(r: &mut Rng, cell: f32) -> Object {
    Object {
        shape: presets::GLYPH,
        motion: Motion::Rain { speed: r.range(0.3, 1.3), rng: Rng::new(r.next_u64()) },
        pos: [r.range(-1.0, 1.0), r.range(-1.6, 1.0)],
        radius: cell,
        wobble: 0.0,
        wobble_freq: 0.0,
        width: 0.0,
        angle: 0.0,
        spin: 0.0,
        color: r.f(),
        color_rate: r.sign() * r.range(0.2, 1.2),
        grad: 0.0,
        lobes: 0.0,
        additive: false,
        age: 0.0,
        retire: false,
    }
}

fn random_object(r: &mut Rng, recipe: &Recipe, theme: u32) -> Object {
    let shape =
        if recipe.matched || recipe.shapes.len() == 1 { theme } else { recipe.shapes[r.below(recipe.shapes.len() as u32) as usize] };
    let orbit = match recipe.motion {
        MotionStyle::Bounce => false,
        MotionStyle::Orbit => true,
        MotionStyle::Mixed => r.chance(0.4),
    };
    let motion = if orbit {
        // Orbit frequencies scale with the recipe's speed so calm presets stay calm.
        let k = (recipe.speed.0 + recipe.speed.1) / 0.75;
        Motion::Orbit {
            freq: [r.range(0.15, 0.8) * k, r.range(0.15, 0.8) * k, r.range(0.4, 2.0) * k, r.range(0.4, 2.0) * k],
            phase: [r.range(0.0, TAU), r.range(0.0, TAU), r.range(0.0, TAU), r.range(0.0, TAU)],
        }
    } else {
        let speed = r.range(recipe.speed.0, recipe.speed.1);
        let dir = r.range(0.0, TAU);
        Motion::Bounce { vel: [speed * dir.cos(), speed * dir.sin()] }
    };
    Object {
        shape,
        motion,
        pos: [r.range(-0.9, 0.9), r.range(-0.9, 0.9)],
        radius: r.range(recipe.radius.0, recipe.radius.1),
        wobble: if r.chance(recipe.wobble) { r.range(0.1, 0.7) } else { 0.0 },
        wobble_freq: r.range(0.3, 3.0),
        width: r.range(recipe.width.0, recipe.width.1),
        angle: r.range(0.0, TAU),
        spin: r.sign() * r.range(0.0, 3.0),
        color: r.f(),
        color_rate: r.sign() * r.range(recipe.color_rate.0, recipe.color_rate.1),
        grad: if r.chance(recipe.grad) { r.sign() * r.range(0.05, 1.0) } else { 0.0 },
        lobes: r.int(recipe.sides.0, recipe.sides.1),
        additive: recipe.additive,
        age: 0.0,
        retire: false,
    }
}

fn random_field(r: &mut Rng, modes: &[u32]) -> FieldJob {
    let mode = modes[r.below(modes.len() as u32) as usize];
    let (a, b, c) = match mode {
        presets::PLASMA => (
            [r.range(2.0, 9.0), r.range(0.0, TAU), r.range(2.0, 9.0), r.range(0.0, TAU)],
            [r.range(2.0, 9.0), r.range(0.0, TAU), r.range(-0.5, 0.5), r.range(-0.5, 0.5)],
            [r.range(3.0, 14.0), r.range(0.3, 1.5), 0.0, 0.0],
        ),
        presets::PETALS => (
            [r.range(-0.4, 0.4), r.range(-0.3, 0.3), r.range(1.0, 6.0), r.int(-4, 4)],
            [r.int(0, 8), r.range(0.0, 0.6), 0.0, 0.0],
            [0.0; 4],
        ),
        presets::SPIRAL => (
            [r.range(-0.3, 0.3), r.range(-0.2, 0.2), r.sign() * r.range(0.5, 3.0), r.sign() * r.int(1, 6)],
            [r.range(0.0, 4.0), 0.0, 0.0, 0.0],
            [0.0; 4],
        ),
        presets::MOIRE => (
            [r.range(-0.8, 0.8), r.range(-0.6, 0.6), r.range(-0.8, 0.8), r.range(-0.6, 0.6)],
            [r.range(3.0, 12.0), r.range(-0.8, 0.8), r.range(-0.6, 0.6), r.range(0.0, 6.0)],
            [0.0; 4],
        ),
        presets::KALEIDO => (
            [r.int(3, 12), r.range(2.0, 10.0), r.range(2.0, 10.0), r.range(0.0, TAU)],
            [r.range(1.0, 8.0), r.range(0.3, 1.2), 0.0, 0.0],
            [0.0; 4],
        ),
        presets::GRID => (
            [r.range(3.0, 12.0), r.range(0.3, 1.2), r.range(0.0, 3.0), r.range(0.0, TAU)],
            [0.0; 4],
            [0.0; 4],
        ),
        _ => (
            [r.range(1.0, 6.0), r.range(0.0, TAU), r.range(1.0, 6.0), r.range(0.0, TAU)],
            [r.range(1.0, 5.0), r.int(-4, 4), r.range(0.1, 0.5), 0.0],
            [0.0; 4],
        ),
    };
    FieldJob { mode, id: 0, progress: 0.0, seed: r.below(1 << 20) as f32, a, b, c }
}
