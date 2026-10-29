//! Decides what is on screen: the background pattern, the objects drifting
//! over it, the palette and how fast it cycles, all following the current
//! preset. Everything here is resolution independent; the renderer maps it
//! to pixels.

use crate::palette::{self, ease, Palette};
use crate::presets::{self, MotionStyle, Recipe};
use crate::rng::Rng;
use std::f32::consts::TAU;

pub const MAX_OBJECTS: usize = 12;
/// Every object can be drawn up to this many times by symmetry.
pub const MAX_COPIES: usize = 12;
/// Stamps drawn per frame, all copies included. The GPU has one slot each.
pub const MAX_STAMPS: usize = MAX_OBJECTS * MAX_COPIES;
/// Seconds between glyph rain showers, and how long one lasts.
const RAIN_EVERY: (f32, f32) = (420.0, 780.0);
const RAIN_LASTS: (f32, f32) = (20.0, 40.0);
/// Where a finished rain drop is parked until it is removed.
const GONE: f32 = 100.0;
/// Seconds between small surprises within a preset.
const EVENT_EVERY: (f32, f32) = (4.0, 12.0);
/// Fastest colour cycling, in palette turns per second, before the speed
/// hotkeys scale it: surges and presets never go past this.
const MAX_CYCLE: f32 = 0.35;
/// The colour cycling holds a speed for SURGE_HOLD seconds, then glides over
/// SURGE_GLIDE seconds to a new one: SURGE times the preset's pace.
const SURGE: (f32, f32) = (0.5, 1.5);
const SURGE_HOLD: (f32, f32) = (6.0, 20.0);
const SURGE_GLIDE: (f32, f32) = (1.5, 4.0);

/// One step of the colour cycling's speed: gliding from rom to 	o,
/// then holding until 
ext.
struct Surge {
    from: f32,
    to: f32,
    start: f64,
    glide: f32,
    next: f64,
}

impl Surge {
    fn at(&self, t: f64) -> f32 {
        let k = (((t - self.start) as f32) / self.glide).clamp(0.0, 1.0);
        self.from + (self.to - self.from) * ease(k)
    }
}
/// The symmetries a preset can get, with how often (out of 40) it gets each.
/// The M hotkey steps through them in this order.
const SYMMETRIES: [(Symmetry, u32); 9] = [
    (Symmetry { turns: 1, mirror: false }, 6), // none
    (Symmetry { turns: 1, mirror: true }, 6),  // left/right
    (Symmetry { turns: 2, mirror: false }, 3), // half turn
    (Symmetry { turns: 2, mirror: true }, 7),  // left/right and top/bottom
    (Symmetry { turns: 3, mirror: false }, 2),
    (Symmetry { turns: 3, mirror: true }, 4),
    (Symmetry { turns: 4, mirror: true }, 5),
    (Symmetry { turns: 5, mirror: true }, 4),
    (Symmetry { turns: 6, mirror: true }, 3),
];

/// Synchronised copies of every object around the screen centre: `turns`
/// rotated copies, each also mirrored when `mirror` is set. With mirroring
/// that is `turns` mirror axes, the first one vertical (left/right).
#[derive(Clone, Copy, PartialEq)]
pub struct Symmetry {
    pub turns: u32,
    pub mirror: bool,
}

impl Symmetry {
    pub fn copies(&self) -> usize {
        self.turns as usize * if self.mirror { 2 } else { 1 }
    }

    /// As written in the settings file: "1" for none, "2" for a half turn,
    /// "4m" for four mirrors, and so on.
    pub fn to_setting(self) -> String {
        format!("{}{}", self.turns, if self.mirror { "m" } else { "" })
    }

    /// Reads to_setting's format; None for "random" or anything unknown.
    pub fn from_setting(s: &str) -> Option<Symmetry> {
        let s = s.trim();
        let (turns, mirror) = match s.strip_suffix('m') {
            Some(t) => (t, true),
            None => (s, false),
        };
        let wanted = Symmetry { turns: turns.parse().ok()?, mirror };
        SYMMETRIES.iter().any(|x| x.0 == wanted).then_some(wanted)
    }

    /// For on-screen messages, e.g. "4 mirrors".
    pub fn describe(&self) -> String {
        match (self.turns, self.mirror) {
            (1, false) => "none".into(),
            (1, true) => "left/right mirror".into(),
            (2, false) => "half turn".into(),
            (n, false) => format!("{n}-fold turn"),
            (n, true) => format!("{n} mirrors"),
        }
    }
}

/// A full-screen pattern, blended in over the old one while progress goes 0 -> 1.
#[derive(Clone, Copy)]
pub struct FieldJob {
    pub mode: u32,
    /// New for every background, so the renderer knows when to draw one.
    pub id: u32,
    pub progress: f32,
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
                for (p, v) in self.pos.iter_mut().zip(vel) {
                    *p += *v * dt;
                    if p.abs() > 1.0 {
                        *p = p.clamp(-1.0, 1.0);
                        *v = -*v;
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
        }
    }
}

pub struct Director {
    /// Picks the next preset; seeded from the clock.
    order: Rng,
    /// Drives everything inside a preset; seeded from the preset.
    rng: Rng,
    /// Picks the preset's backgrounds. Kept apart from `rng` because a
    /// background can wait for a fade, and the wait must not change what
    /// else the preset draws.
    field_rng: Rng,
    /// Hotkey multipliers for colour cycling and object movement.
    cycle_mult: f32,
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
    preset_end: f64,
    next_event: f64,
    /// When the next background transition within the preset starts.
    next_field: f64,
    transition_seconds: f32,
    field: FieldJob,
    field_start: f64,
    field_dur: f32,
    field_active: bool,
    /// A background waiting for the current fade to finish; true = picture.
    queued: Option<bool>,
    objects: Vec<Object>,
    /// Glyph rain columns, kept apart so events never swap them for objects.
    drops: Vec<Object>,
    /// When the current shower ends, or the next one starts.
    rain_switch: f64,
    pal_from: Palette,
    pal_to: Palette,
    pal_start: f64,
    pal_dur: f32,
    /// Palette rotation, and how fast it turns (cycles per second) on its
    /// way to the preset's target speed.
    offset: f64,
    cycle: f32,
    cycle_target: f32,
    /// The speed step the colour cycling is on; see update.
    surge: Surge,
    image_chance: f32,
    /// Set by the app each frame: a decoded picture is waiting.
    pub image_ready: bool,
    /// Set by the app: draw the moving shapes. While false they stand still,
    /// and carry on from there when switched back on. Rain is separate.
    pub show_objects: bool,
    /// Set by the app: keep the background, only changing it by hotkey.
    pub hold_background: bool,
    /// Glyph rain showers come now and then; when false, none start.
    pub rain_on: bool,
    /// A symmetry chosen with the hotkey, kept across presets; None lets
    /// every preset pick its own.
    symmetry_choice: Option<Symmetry>,
    new_image: bool,
}

impl Director {
    pub fn new(
        seed: u64,
        cycle_mult: f32,
        motion_mult: f32,
        preset_seconds: f32,
        transition_seconds: f32,
        fixed: Option<usize>,
        image_chance: f32,
    ) -> Self {
        let mut order = Rng::new(seed);
        let preset = pick_preset(&mut order, fixed);
        let (recipe, name, pseed) = presets::get(preset);
        let mut rng = Rng::new(pseed);
        let pal = palette::random_of(&mut rng, recipe.palettes);
        // Placeholders; start_preset sets the real ones.
        let field = random_field(&mut rng, recipe.fields);
        let mut d = Director {
            order,
            rng,
            field_rng: Rng::new(pseed),
            cycle_mult,
            motion_mult,
            preset_seconds,
            fixed,
            preset,
            recipe,
            name,
            theme: 0,
            symmetry: SYMMETRIES[0].0,
            preset_end: 0.0,
            next_event: 0.0,
            next_field: 0.0,
            transition_seconds,
            field,
            field_start: 0.0,
            field_dur: 2.0,
            field_active: false,
            queued: None,
            objects: Vec::new(),
            drops: Vec::new(),
            rain_switch: 0.0,
            pal_from: pal,
            pal_to: pal,
            pal_start: 0.0,
            pal_dur: 1.0,
            offset: 0.0,
            cycle: 0.0,
            cycle_target: 0.1,
            surge: Surge { from: 1.0, to: 1.0, start: 0.0, glide: 1.0, next: 0.0 },
            image_chance,
            image_ready: false,
            show_objects: true,
            hold_background: false,
            rain_on: true,
            symmetry_choice: None,
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
        (self.cycle_mult, self.motion_mult)
    }

    /// Jumps to a random preset other than the current one.
    pub fn random_preset(&mut self, t: f64) {
        let i = (self.preset + 1 + self.order.below(presets::COUNT as u32 - 1) as usize) % presets::COUNT;
        if self.fixed.is_some() {
            self.fixed = Some(i);
        }
        self.start_preset(i, t);
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
            let next = pick_preset(&mut self.order, self.fixed);
            self.start_preset(next, t);
        }
        if t >= self.next_event {
            self.event(t);
        }
        // Glyph rain on its own clock, independent of presets.
        if t >= self.rain_switch {
            let start = !self.raining() && self.rain_on;
            if start || self.raining() {
                self.set_rain(start);
            }
            let gap = if start { RAIN_LASTS } else { RAIN_EVERY };
            self.rain_switch = t + self.order.range(gap.0, gap.1) as f64;
        }
        // A fresh background now and then within the preset, sometimes a picture.
        if t >= self.next_field && !self.field_active && !self.hold_background {
            let picture = self.roll_picture();
            self.begin_background(t, picture);
        }
        // The palette moves on to a new one, quickly or slowly, and now and
        // then stays a while first, so the changes have no steady rhythm.
        if t >= self.pal_start + self.pal_dur as f64 {
            self.pal_from = self.pal_to;
            if self.rng.chance(0.25) {
                self.pal_start = t;
                self.pal_dur = self.rng.range(3.0, 12.0);
            } else {
                let dur = self.rng.range(5.0, 20.0);
                self.blend_palette_to_new(t, dur);
            }
        }

        let field = if self.field_active {
            let p = ((t - self.field_start) as f32 / self.field_dur).min(1.0);
            if p >= 1.0 {
                self.field_active = false;
            }
            Some(FieldJob { progress: ease(p), ..self.field })
        } else {
            None
        };
        // A background that arrived during the fade starts once it is over.
        if !self.field_active {
            if let Some(picture) = self.queued.take() {
                self.begin_background(t, picture);
            }
        }

        self.cycle += (self.cycle_target - self.cycle) * (dt * 0.7).min(1.0);
        // On top of the preset's pace the cycling holds a speed for a while,
        // then glides to a new one: a smoothed staircase, not a wave.
        if t >= self.surge.next {
            self.surge = Surge {
                from: self.surge.at(t),
                to: self.order.range(SURGE.0, SURGE.1),
                start: t,
                glide: self.order.range(SURGE_GLIDE.0, SURGE_GLIDE.1),
                next: t + self.order.range(SURGE_HOLD.0, SURGE_HOLD.1) as f64,
            };
        }
        let pace = (self.cycle * self.surge.at(t)).clamp(-MAX_CYCLE, MAX_CYCLE);
        self.offset = (self.offset + (pace * self.cycle_mult * dt) as f64).rem_euclid(1.0);

        let move_dt = dt * self.motion_mult;
        let shown = if self.show_objects { self.objects.len() } else { 0 };
        let brushes = self.objects[..shown].iter_mut().chain(&mut self.drops).map(|o| o.step(move_dt)).collect();
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
        palette::lerp(&self.pal_from, &self.pal_to, ease(k))
    }

    /// Starts a `dur` second blend from pal_from into a new random palette.
    fn blend_palette_to_new(&mut self, t: f64, dur: f32) {
        self.pal_to = palette::random_of(&mut self.rng, self.recipe.palettes);
        self.pal_start = t;
        self.pal_dur = dur;
    }

    /// How many objects and rain drops fit on the GPU with the current symmetry.
    fn stamp_room(&self) -> usize {
        MAX_STAMPS / self.symmetry.copies()
    }

    fn start_preset(&mut self, index: usize, t: f64) {
        let (recipe, name, seed) = presets::get(index);
        self.preset = index;
        self.recipe = recipe;
        self.name = name;
        self.rng = Rng::new(seed);
        self.field_rng = Rng::new(seed ^ 0xF1E1D);
        self.preset_end = t + self.preset_seconds as f64;
        self.next_event = t + self.rng.range(EVENT_EVERY.0, EVENT_EVERY.1) as f64;

        // A held background stays through preset changes too.
        let picture = !self.hold_background && self.roll_picture();
        if !self.hold_background {
            self.begin_background(t, picture);
        }

        let r = &mut self.rng;
        self.theme = r.pick(recipe.shapes);
        let mut count = r.range(recipe.count.0 as f32, recipe.count.1 as f32 + 0.999) as usize;
        // Picked even when one is chosen, so the preset's sequence stays the same.
        let picked = random_symmetry(r);
        self.symmetry = self.symmetry_choice.unwrap_or(picked);
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
        let count = count.min(self.stamp_room());
        let r = &mut self.rng;
        self.objects = (0..count).map(|_| random_object(r, recipe, theme)).collect();

        // Blend from whatever is showing into the preset's own palette.
        self.pal_from = self.current_palette(t);
        let dur = self.rng.range(3.0, 6.0);
        self.blend_palette_to_new(t, dur);
        self.cycle_target = self.rng.sign() * self.rng.range(recipe.cycle.0, recipe.cycle.1);
    }

    /// Hotkey: fade in a new background.
    pub fn next_background(&mut self, t: f64) {
        self.begin_background(t, false);
    }

    /// Hotkey: fade in a picture, if one is decoded.
    /// Returns false if no picture is ready yet.
    pub fn show_picture(&mut self, t: f64) -> bool {
        if self.image_ready {
            self.begin_background(t, true);
        }
        self.image_ready
    }

    /// The background showing or fading in is a picture.
    pub fn showing_picture(&self) -> bool {
        self.field.mode == presets::IMAGE
    }

    /// Whether the next background should be a picture. Decided with the
    /// order rng so the preset's own sequence is unchanged.
    fn roll_picture(&mut self) -> bool {
        self.image_ready && self.order.chance(self.image_chance)
    }

    /// Starts fading in a new pattern, or a picture. One already fading in
    /// is never cut short: the new one waits and starts when it is done.
    /// Only one waits; if a picture was asked for meanwhile, it is a picture.
    fn begin_background(&mut self, t: f64, picture: bool) {
        if self.field_active {
            self.queued = Some(picture || self.queued == Some(true));
            return;
        }
        self.new_field(t);
        if picture && self.image_ready {
            // a.x: palette cycles across black..white, a.y: starting colour.
            self.field.mode = presets::IMAGE;
            self.field.a = [self.order.range(0.7, 2.0), self.order.f(), 0.0, 0.0];
            self.new_image = true;
        }
    }

    /// Hotkey: blend quickly into a new palette.
    pub fn next_palette(&mut self, t: f64) {
        self.pal_from = self.current_palette(t);
        self.blend_palette_to_new(t, 2.0);
    }

    /// Hotkey: step through the symmetry options.
    /// Steps to the next symmetry choice: random (each preset picks), then
    /// each symmetry in turn, then random again.
    pub fn next_symmetry(&mut self) -> Option<Symmetry> {
        let next = match self.symmetry_choice {
            None => Some(SYMMETRIES[0].0),
            Some(s) => {
                let i = SYMMETRIES.iter().position(|x| x.0 == s).unwrap_or(0);
                SYMMETRIES.get(i + 1).map(|x| x.0)
            }
        };
        self.choose_symmetry(next);
        next
    }

    /// Keeps `choice` across presets, starting now; None lets presets pick.
    pub fn choose_symmetry(&mut self, choice: Option<Symmetry>) {
        self.symmetry_choice = choice;
        if let Some(s) = choice {
            self.symmetry = s;
        }
    }

    /// Hotkey: glyph rain on (a shower starts now, more follow) or off
    /// (the current shower ends and no more start).
    pub fn set_rain_on(&mut self, on: bool) {
        self.rain_on = on;
        self.set_rain(on);
    }

    pub fn raining(&self) -> bool {
        self.drops.iter().any(|d| !d.retire)
    }

    /// Starts a shower, or lets drops already falling finish their fall
    /// without starting again.
    fn set_rain(&mut self, on: bool) {
        if on {
            let room = self.stamp_room().saturating_sub(self.objects.len());
            let cell = self.rng.range(0.02, 0.032);
            let count = (self.rng.range(20.0, 45.0) as usize).min(room);
            self.drops = (0..count).map(|_| random_drop(&mut self.rng, cell)).collect();
        } else {
            for d in &mut self.drops {
                d.retire = true;
            }
        }
    }

    /// Hotkey: run the colour cycling the other way.
    pub fn reverse(&mut self) {
        self.cycle_target = -self.cycle_target;
    }

    /// Hotkey: multiply how fast objects move.
    pub fn scale_motion(&mut self, factor: f32) {
        self.motion_mult = (self.motion_mult * factor).clamp(0.05, 20.0);
    }

    /// Hotkey: multiply the colour cycling speed.
    pub fn scale_cycle(&mut self, factor: f32) {
        self.cycle_mult = (self.cycle_mult * factor).clamp(0.05, 20.0);
    }

    /// Starts revealing a new background pattern from the preset's list.
    fn new_field(&mut self, t: f64) {
        let r = &mut self.field_rng;
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
        self.next_event = t + r.range(EVENT_EVERY.0, EVENT_EVERY.1) as f64;
        match r.below(3) {
            0 => self.cycle_target = -self.cycle_target,
            1 => self.cycle_target = self.cycle_target.signum() * r.range(self.recipe.cycle.0, self.recipe.cycle.1),
            _ => {
                if !self.objects.is_empty() {
                    let i = r.below(self.objects.len() as u32) as usize;
                    self.objects[i] = random_object(r, self.recipe, self.theme);
                }
            }
        }
    }
}

/// The fixed preset, or a random one.
fn pick_preset(order: &mut Rng, fixed: Option<usize>) -> usize {
    fixed.unwrap_or_else(|| order.below(presets::COUNT as u32) as usize)
}

fn random_symmetry(r: &mut Rng) -> Symmetry {
    let mut roll = r.below(SYMMETRIES.iter().map(|s| s.1).sum());
    for (symmetry, weight) in SYMMETRIES {
        if roll < weight {
            return symmetry;
        }
        roll -= weight;
    }
    unreachable!()
}

/// One column of glyph rain. All drops in a shower share `cell` (half a
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
        age: 0.0,
        retire: false,
    }
}

fn random_object(r: &mut Rng, recipe: &Recipe, theme: u32) -> Object {
    let shape = if recipe.matched || recipe.shapes.len() == 1 { theme } else { r.pick(recipe.shapes) };
    let orbit = match recipe.motion {
        MotionStyle::Bounce => false,
        MotionStyle::Orbit => true,
        MotionStyle::Mixed => r.chance(0.4),
    };
    let motion = if orbit {
        // Orbit frequencies scale with the recipe's speed so calm presets stay
        // calm; 0.75 is the default speed range's lo + hi, which gives k = 1.
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
        age: 0.0,
        retire: false,
    }
}

fn random_field(r: &mut Rng, modes: &[u32]) -> FieldJob {
    let mode = r.pick(modes);
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
        presets::GRID => {
            ([r.range(3.0, 12.0), r.range(0.3, 1.2), r.range(0.0, 3.0), r.range(0.0, TAU)], [0.0; 4], [0.0; 4])
        }
        presets::TUNNEL => (
            [r.range(1.0, 6.0), r.range(0.0, TAU), r.range(1.0, 6.0), r.range(0.0, TAU)],
            [r.range(1.0, 5.0), r.int(-4, 4), r.range(0.1, 0.5), 0.0],
            [0.0; 4],
        ),
        other => unreachable!("background pattern {other} is not in a recipe"),
    };
    FieldJob { mode, id: 0, progress: 0.0, a, b, c }
}
