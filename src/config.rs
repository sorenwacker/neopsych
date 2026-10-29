use std::path::PathBuf;

pub struct Config {
    /// Each pattern pixel covers pixel_scale x pixel_scale screen pixels.
    pub pixel_scale: u32,
    pub fps: u32,
    /// Blend between neighbouring palette entries instead of hard 8-bit steps.
    pub smooth: bool,
    /// Multiplier for colour cycling speed.
    pub speed: f32,
    /// Multiplier for how fast objects move.
    pub motion: f32,
    /// Seconds before moving on to the next preset.
    pub preset_seconds: f32,
    /// Average seconds between background transitions within a preset.
    pub transition_seconds: f32,
    /// 1..=presets::COUNT stays on that preset; 0 rotates through them in random order.
    pub preset: u32,
    /// Where to find pictures; None means images::default_folder.
    pub image_folder: Option<PathBuf>,
    /// Also take pictures from folders inside image_folder.
    pub image_subfolders: bool,
    /// Chance that a preset opens with a picture instead of a pattern.
    pub image_chance: f32,
    /// Pictures switched on; the P hotkey turns them off without losing image_chance.
    pub pictures: bool,
    /// The moving shapes switched on; the O hotkey switches them.
    pub objects: bool,
    /// Keep the background instead of changing it; the L hotkey switches this.
    pub hold_background: bool,
    /// Show messages on screen for hotkeys and new presets; the I hotkey switches this.
    pub messages: bool,
    /// Glyph rain showers now and then; the G hotkey switches this.
    pub rain: bool,
    /// "random" lets every preset pick its symmetry; otherwise one kept
    /// across presets, see director::Symmetry::to_setting. The M hotkey sets it.
    pub symmetry: String,
    /// Started with --play: fullscreen or in a window, as last left.
    pub fullscreen: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            pixel_scale: 1,
            fps: 60,
            smooth: true,
            speed: 1.0,
            motion: 1.0,
            preset_seconds: 60.0,
            transition_seconds: 45.0,
            preset: 0,
            image_folder: None,
            image_subfolders: false,
            image_chance: 0.4,
            pictures: true,
            objects: true,
            hold_background: false,
            messages: true,
            rain: true,
            symmetry: "random".into(),
            fullscreen: true,
        }
    }
}

const DEFAULT_FILE: &str = "\
# neopsych settings. Delete this file to restore defaults.

# 1 = full resolution, 2 or 4 = chunkier retro pixels and even less GPU work
pixel_scale = 1

# frame rate cap
fps = 60

# true = smooth gradients, false = hard 256-colour steps like the original
smooth = true

# colour cycling speed multiplier
speed = 1.0

# object movement speed multiplier
motion = 1.0

# seconds before moving on to the next preset
preset_seconds = 60

# average seconds between new backgrounds (independent of preset changes)
transition_seconds = 45

# 0 = play all 120 presets in random order, 1-120 = stay on one
preset = 0

# folder of pictures shown in the current palette
# (empty = the Pictures folder in your user folder)
image_folder =

# true = also use pictures in folders inside it
image_subfolders = false

# chance (0 to 1) that a new background is a picture; 0 turns pictures off
image_chance = 0.4

# false = patterns only for now (the P hotkey switches this)
pictures = true

# false = no moving shapes, just the backgrounds (the O hotkey switches this)
objects = true

# true = keep the background, only the shapes change (the L hotkey switches this)
hold_background = false

# false = no messages on screen for hotkeys and presets (the I hotkey switches this)
messages = true

# false = no glyph rain showers (the G hotkey switches this)
rain = true

# random = every preset picks its own; or keep one: 1 = none, 1m = left/right
# mirror, 2 = half turn, 2m to 6m = 2 to 6 mirrors, 3 = 3-fold turn (the M hotkey sets this)
symmetry = random

# started from the Start menu: true = fullscreen, false = in a window
# (F, F11 and double-click switch this)
fullscreen = true
";

/// The settings file; NEOPSYCH_CONFIG names another one, e.g. for tests.
pub fn path() -> PathBuf {
    if let Some(p) = std::env::var_os("NEOPSYCH_CONFIG") {
        return PathBuf::from(p);
    }
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("neopsych").join("neopsych.ini")
}

pub fn write_default_if_missing() -> std::io::Result<PathBuf> {
    let p = path();
    if !p.exists() {
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&p, DEFAULT_FILE)?;
    }
    Ok(p)
}

/// Rewrites the given keys in the settings file, keeping comments and
/// everything else as the user left it. Missing keys are appended.
pub fn save_values(values: &[(&str, String)]) -> std::io::Result<()> {
    let path = write_default_if_missing()?;
    let text = std::fs::read_to_string(&path)?;
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    for (key, value) in values {
        let line = format!("{key} = {value}");
        let existing = lines.iter_mut().find(|l| {
            let l = l.trim_start();
            !l.starts_with('#') && l.split_once('=').is_some_and(|(k, _)| k.trim() == *key)
        });
        match existing {
            Some(l) => *l = line,
            None => lines.push(line),
        }
    }
    std::fs::write(&path, lines.join("\n") + "\n")
}

pub fn load() -> Config {
    let mut c = Config::default();
    let Ok(text) = std::fs::read_to_string(path()) else { return c };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim();
        match k.trim() {
            "pixel_scale" => c.pixel_scale = v.parse().unwrap_or(c.pixel_scale).clamp(1, 16),
            "fps" => c.fps = v.parse().unwrap_or(c.fps).clamp(5, 240),
            "smooth" => c.smooth = v.parse().unwrap_or(c.smooth),
            "speed" => c.speed = v.parse().unwrap_or(c.speed).clamp(0.05, 20.0),
            "motion" => c.motion = v.parse().unwrap_or(c.motion).clamp(0.05, 20.0),
            "preset_seconds" => c.preset_seconds = v.parse().unwrap_or(c.preset_seconds).max(5.0),
            "transition_seconds" => c.transition_seconds = v.parse().unwrap_or(c.transition_seconds).max(3.0),
            "preset" => c.preset = v.parse().unwrap_or(c.preset).min(crate::presets::COUNT as u32),
            "image_folder" => c.image_folder = (!v.is_empty()).then(|| PathBuf::from(v.trim_matches('"'))),
            "image_subfolders" => c.image_subfolders = v.parse().unwrap_or(c.image_subfolders),
            "image_chance" => c.image_chance = v.parse().unwrap_or(c.image_chance).clamp(0.0, 1.0),
            "pictures" => c.pictures = v.parse().unwrap_or(c.pictures),
            "objects" => c.objects = v.parse().unwrap_or(c.objects),
            "hold_background" => c.hold_background = v.parse().unwrap_or(c.hold_background),
            "messages" => c.messages = v.parse().unwrap_or(c.messages),
            "rain" => c.rain = v.parse().unwrap_or(c.rain),
            "symmetry" => c.symmetry = v.to_string(),
            "fullscreen" => c.fullscreen = v.parse().unwrap_or(c.fullscreen),
            _ => {}
        }
    }
    c
}
