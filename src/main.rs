#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod director;
mod gpu;
mod images;
mod osd;
mod palette;
mod presets;
mod rng;

use config::Config;
use director::Director;
use gpu::{Gpu, Target};
use images::Images;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Fullscreen, Window, WindowId, WindowLevel};

/// Mouse travel (physical pixels) that ends the screensaver.
const MOVE_TOLERANCE: f64 = 12.0;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// /s: fullscreen on every monitor, exit on input.
    Saver,
    /// /p <hwnd>: draw inside the little monitor in Screen Saver Settings.
    Preview(isize),
    /// /c or no arguments: settings.
    Configure,
    /// --window or --fullscreen: interactive, with hotkeys.
    Windowed { fullscreen: bool },
}

impl Mode {
    /// Run by hand rather than by Windows: hotkeys work and input doesn't quit.
    fn interactive(self) -> bool {
        matches!(self, Mode::Windowed { .. })
    }
}

const HOTKEYS: &str = "H hotkeys · Left/Right preset · Up/Down movement · Space pause · F fullscreen · Esc quit";

const HELP: &str = "\
Left / Right\tprevious / next preset
PgUp / PgDn\tjump a family (5 presets)
B\tnew background pattern
I\tshow a wallpaper picture
S\tpictures from subfolders too on / off
P\tpictures on / off
O\tmoving shapes on / off
L\tkeep the background / let it change
C\tblend into new colours
M\tnext symmetry
G\tglyph rain on / off
R\treverse colour cycling
+ / -\tcolour cycling faster / slower
Shift + wheel\tcolour cycling faster / slower
Up / Down\tmovement faster / slower
Mouse wheel\tmovement faster / slower
Space\tpause / resume
F, F11 or double-click\ttoggle fullscreen
H\tthis help
Esc\tquit";

/// Windows passes "/s", "/p 1234", "/p:1234", "/c:1234" in any case.
fn parse_args() -> Mode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(first) = args.first() else {
        return if cfg!(windows) { Mode::Configure } else { Mode::Windowed { fullscreen: false } };
    };
    let lower = first.to_ascii_lowercase();
    let (flag, inline) = match lower.split_once(':') {
        Some((f, v)) => (f.to_string(), Some(v.to_string())),
        None => (lower.clone(), None),
    };
    match flag.trim_start_matches(['/', '-']) {
        "s" => Mode::Saver,
        "p" | "l" => {
            let hwnd = inline.or_else(|| args.get(1).cloned()).and_then(|v| v.trim().parse().ok());
            hwnd.map(Mode::Preview).unwrap_or(Mode::Windowed { fullscreen: false })
        }
        "c" => Mode::Configure,
        "fullscreen" | "f" => Mode::Windowed { fullscreen: true },
        _ => Mode::Windowed { fullscreen: false },
    }
}

struct App {
    mode: Mode,
    cfg: Config,
    gpu: Option<Gpu>,
    targets: Vec<Target>,
    director: Director,
    /// Animation time in seconds; stands still while paused.
    clock: f64,
    paused: bool,
    shift: bool,
    last: Instant,
    next_frame: Instant,
    cursor_origin: HashMap<WindowId, PhysicalPosition<f64>>,
    /// Frame rate log: the file named by NEOPSYCH_STATS, and frames
    /// rendered since the last line written to it.
    stats: Option<(std::ffi::OsString, u32, Instant)>,
    vsync_paced: bool,
    /// Preset name last shown in the window title.
    title: String,
    images: Option<Images>,
    /// On-screen message and when it appeared; interactive mode only.
    message: Option<(String, Instant)>,
    /// When the left mouse button last went down, to spot a double-click.
    last_click: Option<Instant>,
}

/// Two clicks closer together than this are a double-click (the Windows default).
const DOUBLE_CLICK: Duration = Duration::from_millis(500);

/// Seconds a message stays fully visible, then fades out over the next half.
const MESSAGE_SECONDS: f32 = 2.0;

impl App {
    fn new(mode: Mode, cfg: Config) -> App {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        let fixed = (cfg.preset > 0).then(|| cfg.preset as usize - 1);
        let director = Director::new(
            seed,
            cfg.speed,
            cfg.motion,
            cfg.preset_seconds,
            cfg.transition_seconds,
            fixed,
            cfg.image_chance,
        );
        let images = open_images(&cfg, seed);
        let now = Instant::now();
        App {
            mode,
            cfg,
            gpu: None,
            targets: Vec::new(),
            director,
            clock: 0.0,
            paused: false,
            shift: false,
            last: now,
            next_frame: now,
            cursor_origin: HashMap::new(),
            stats: std::env::var_os("NEOPSYCH_STATS").map(|path| (path, 0, now)),
            vsync_paced: false,
            title: String::new(),
            images,
            message: None,
            last_click: None,
        }
    }

    /// Shows `text` briefly on screen.
    fn say(&mut self, text: impl Into<String>) {
        if self.mode.interactive() {
            self.message = Some((text.into(), Instant::now()));
        }
    }

    /// The current message and how visible it is, if one is showing.
    fn current_message(&mut self, now: Instant) -> Option<(&str, f32)> {
        let age = (now - self.message.as_ref()?.1).as_secs_f32();
        if age > MESSAGE_SECONDS + 0.5 {
            self.message = None;
            return None;
        }
        let alpha = (1.0 - (age - MESSAGE_SECONDS) / 0.5).min(1.0);
        self.message.as_ref().map(|m| (m.0.as_str(), alpha))
    }

    fn create_windows(&self, el: &ActiveEventLoop) -> Vec<Window> {
        let base = with_icon(Window::default_attributes().with_title("neopsych"));
        match self.mode {
            Mode::Saver => el
                .available_monitors()
                .filter_map(|m| {
                    // Always-on-top and fullscreen are applied once the surface
                    // is configured: DX12 rejects a surface on a window created topmost.
                    let attrs =
                        base.clone().with_decorations(false).with_position(m.position()).with_inner_size(m.size());
                    el.create_window(attrs).ok()
                })
                .collect(),
            Mode::Preview(hwnd) => preview_window(el, base, hwnd).into_iter().collect(),
            _ => el.create_window(base.with_inner_size(LogicalSize::new(1280.0, 720.0))).into_iter().collect(),
        }
    }

    fn toggle_fullscreen(&self, id: WindowId) {
        if let Some(t) = self.targets.iter().find(|t| t.id() == id) {
            let w = &t.window;
            let fs = if w.fullscreen().is_some() { None } else { Some(Fullscreen::Borderless(w.current_monitor())) };
            w.set_cursor_visible(fs.is_none());
            w.set_fullscreen(fs);
        }
    }

    /// Hotkey: take pictures from the image folder's subfolders too, or stop.
    /// Rescans in the background and remembers the choice for next time.
    fn toggle_subfolders(&mut self) {
        self.cfg.image_subfolders = !self.cfg.image_subfolders;
        self.images = open_images(&self.cfg, self.clock.to_bits());
        let _ = config::save_values(&[("image_subfolders", self.cfg.image_subfolders.to_string())]);
        self.say(if self.cfg.image_subfolders { "Pictures: with subfolders" } else { "Pictures: top folder only" });
    }

    /// Hotkey: pictures on or off. Off fades a picture that is showing over
    /// to a pattern. Remembered for next time.
    fn toggle_pictures(&mut self) {
        self.cfg.pictures = !self.cfg.pictures;
        if !self.cfg.pictures && self.director.showing_picture() {
            self.director.next_background(self.clock);
        }
        let _ = config::save_values(&[("pictures", self.cfg.pictures.to_string())]);
        self.say(if self.cfg.pictures { "Pictures: on" } else { "Pictures: off" });
    }

    /// Hotkey: keep the background, or let it change again. Remembered for next time.
    fn toggle_hold_background(&mut self) {
        self.cfg.hold_background = !self.cfg.hold_background;
        let _ = config::save_values(&[("hold_background", self.cfg.hold_background.to_string())]);
        self.say(if self.cfg.hold_background { "Background: kept" } else { "Background: changing" });
    }

    /// Hotkey: the moving shapes on or off. Off also fades in a clean
    /// background of the same kind, since the trails they painted would
    /// otherwise stay. Remembered for next time.
    fn toggle_objects(&mut self) {
        self.cfg.objects = !self.cfg.objects;
        if !self.cfg.objects && !(self.director.showing_picture() && self.director.show_picture(self.clock)) {
            self.director.next_background(self.clock);
        }
        let _ = config::save_values(&[("objects", self.cfg.objects.to_string())]);
        self.say(if self.cfg.objects { "Moving shapes: on" } else { "Moving shapes: off" });
    }

    fn render_all(&mut self) {
        if self.gpu.is_none() {
            return;
        }
        let now = Instant::now();
        // A new preset, by time or by hotkey: name it in the title and on screen.
        if self.mode.interactive() && self.title != self.director.name() {
            self.title = self.director.name().to_string();
            for t in &self.targets {
                t.window.set_title(&format!("neopsych - {}   |   {HOTKEYS}", self.title));
            }
            self.say(self.title.clone());
        }
        let message = self.current_message(now).map(|(text, alpha)| (text.to_string(), alpha));
        let Some(gpu) = &self.gpu else { return };
        let dt = if self.paused { 0.0 } else { (now - self.last).as_secs_f32().min(0.25) };
        self.last = now;
        self.clock += dt as f64;
        self.director.image_ready = self.cfg.pictures && self.images.as_mut().is_some_and(|i| i.poll());
        self.director.show_objects = self.cfg.objects;
        self.director.hold_background = self.cfg.hold_background;
        let frame = self.director.update(self.clock, dt);
        if frame.new_image {
            if let Some(img) = self.images.as_mut().and_then(|i| i.take()) {
                for t in &mut self.targets {
                    t.set_picture(gpu, &img);
                }
            }
        }
        for t in &mut self.targets {
            t.render(gpu, &frame, message.as_ref().map(|(text, alpha)| (text.as_str(), *alpha)));
        }
        if let Some((path, frames, since)) = &mut self.stats {
            *frames += 1;
            if now - *since >= Duration::from_secs(2) {
                use std::io::Write;
                let line = format!("{:.1} fps\n", *frames as f64 / (now - *since).as_secs_f64());
                let f = std::fs::OpenOptions::new().create(true).append(true).open(&*path);
                let _ = f.and_then(|mut f| f.write_all(line.as_bytes()));
                (*frames, *since) = (0, now);
            }
        }
    }
}

impl ApplicationHandler for App {
    /// Keeps speed changes made with hotkeys for next time.
    fn exiting(&mut self, _: &ActiveEventLoop) {
        if !self.mode.interactive() {
            return;
        }
        let (speed, motion) = self.director.speeds();
        if (speed - self.cfg.speed).abs() > 1e-4 || (motion - self.cfg.motion).abs() > 1e-4 {
            let _ = config::save_values(&[("speed", format!("{speed:.2}")), ("motion", format!("{motion:.2}"))]);
        }
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        if !self.targets.is_empty() {
            return;
        }
        let windows = self.create_windows(el);
        if windows.is_empty() {
            el.exit();
            return;
        }
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let mut surfaces = Vec::new();
        for w in windows {
            let w = Arc::new(w);
            let surface = instance.create_surface(w.clone()).expect("failed to create surface");
            surfaces.push((w, surface));
        }
        // Highest refresh rate among our monitors; unknown counts as 60 Hz.
        let refresh_mhz =
            surfaces.iter().filter_map(|(w, _)| w.current_monitor()?.refresh_rate_millihertz()).max().unwrap_or(60_000);
        self.vsync_paced = self.cfg.fps * 1000 + 1000 >= refresh_mhz;
        let gpu = Gpu::new(instance, &surfaces[0].1);
        for (w, s) in surfaces {
            let target = Target::new(&gpu, w.clone(), s, self.cfg.pixel_scale, self.cfg.smooth);
            if self.mode == Mode::Saver {
                w.set_window_level(WindowLevel::AlwaysOnTop);
            }
            if matches!(self.mode, Mode::Saver | Mode::Windowed { fullscreen: true }) {
                w.set_cursor_visible(false);
                w.set_fullscreen(Some(Fullscreen::Borderless(w.current_monitor())));
            }
            // Started from another program the window may open without
            // keyboard focus, and then hotkeys go nowhere.
            if self.mode.interactive() {
                w.focus_window();
            }
            self.targets.push(target);
        }
        self.gpu = Some(gpu);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let saver = self.mode == Mode::Saver;
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) => {
                if let (Some(gpu), Some(t)) = (&self.gpu, self.targets.iter_mut().find(|t| t.id() == id)) {
                    t.resize(gpu, size.width, size.height, self.cfg.pixel_scale);
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if saver {
                    el.exit();
                    return;
                }
                // Holding a key down must not toggle things again and again;
                // only the stepping keys (arrows, +/-) may repeat.
                let steps = match &event.logical_key {
                    Key::Named(k) => matches!(
                        k,
                        NamedKey::ArrowLeft | NamedKey::ArrowRight | NamedKey::ArrowUp | NamedKey::ArrowDown
                    ),
                    Key::Character(c) => matches!(c.as_str(), "+" | "-" | "="),
                    _ => false,
                };
                if event.repeat && !steps {
                    return;
                }
                let t = self.clock;
                let d = &mut self.director;
                // Preset changes announce themselves (see render_all).
                let said: Option<String> = match &event.logical_key {
                    Key::Named(NamedKey::Escape) => {
                        el.exit();
                        None
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        d.skip(1, t);
                        None
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        d.skip(-1, t);
                        None
                    }
                    Key::Named(NamedKey::PageDown) => {
                        d.skip(presets::VARIATIONS as i32, t);
                        None
                    }
                    Key::Named(NamedKey::PageUp) => {
                        d.skip(-(presets::VARIATIONS as i32), t);
                        None
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        d.scale_motion(1.25);
                        Some(format!("Movement speed: {:.2}", d.speeds().1))
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        d.scale_motion(0.8);
                        Some(format!("Movement speed: {:.2}", d.speeds().1))
                    }
                    Key::Named(NamedKey::Space) => {
                        self.paused = !self.paused;
                        Some(if self.paused { "Paused" } else { "Playing" }.into())
                    }
                    Key::Named(NamedKey::F11) => {
                        self.toggle_fullscreen(id);
                        None
                    }
                    Key::Character(c) => match c.to_ascii_lowercase().as_str() {
                        "b" => {
                            d.next_background(t);
                            Some("New background".into())
                        }
                        "i" => Some(if d.show_picture(t) { "Picture" } else { "No picture ready yet" }.into()),
                        "c" => {
                            d.next_palette(t);
                            Some("New colours".into())
                        }
                        "m" => Some(format!("Symmetry: {}", d.next_symmetry().describe())),
                        "g" => {
                            d.toggle_rain();
                            Some(if d.raining() { "Glyph rain: on" } else { "Glyph rain: off" }.into())
                        }
                        "r" => {
                            d.reverse();
                            Some("Colours reversed".into())
                        }
                        "+" | "=" => {
                            d.scale_cycle(1.25);
                            Some(format!("Colour speed: {:.2}", d.speeds().0))
                        }
                        "-" => {
                            d.scale_cycle(0.8);
                            Some(format!("Colour speed: {:.2}", d.speeds().0))
                        }
                        "f" => {
                            self.toggle_fullscreen(id);
                            None
                        }
                        "s" => {
                            self.toggle_subfolders();
                            None
                        }
                        "p" => {
                            self.toggle_pictures();
                            None
                        }
                        "o" => {
                            self.toggle_objects();
                            None
                        }
                        "l" => {
                            self.toggle_hold_background();
                            None
                        }
                        "h" => {
                            show_help();
                            None
                        }
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(text) = said {
                    self.say(text);
                }
            }
            WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. } if saver => el.exit(),
            // Double-click: fullscreen on / off.
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. }
                if self.mode.interactive() =>
            {
                let now = Instant::now();
                if self.last_click.is_some_and(|t| now - t < DOUBLE_CLICK) {
                    self.toggle_fullscreen(id);
                    self.last_click = None;
                } else {
                    self.last_click = Some(now);
                }
            }
            WindowEvent::ModifiersChanged(m) => self.shift = m.state().shift_key(),
            // Scroll: movement speed. Shift + scroll: colour speed.
            WindowEvent::MouseWheel { delta, .. } => {
                let up = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32,
                };
                if up != 0.0 {
                    let factor = if up > 0.0 { 1.15 } else { 1.0 / 1.15 };
                    let (cycle, motion) = if self.shift {
                        self.director.scale_cycle(factor);
                        self.director.speeds()
                    } else {
                        self.director.scale_motion(factor);
                        self.director.speeds()
                    };
                    self.say(if self.shift {
                        format!("Colour speed: {cycle:.2}")
                    } else {
                        format!("Movement speed: {motion:.2}")
                    });
                }
            }
            WindowEvent::CursorMoved { position, .. } if saver => {
                let origin = *self.cursor_origin.entry(id).or_insert(position);
                let (dx, dy) = (position.x - origin.x, position.y - origin.y);
                if dx * dx + dy * dy > MOVE_TOLERANCE * MOVE_TOLERANCE {
                    el.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if let Mode::Preview(hwnd) = self.mode {
            if !parent_alive(hwnd) {
                el.exit();
                return;
            }
        }
        // At or above the display rate, vsync paces us; a timer on top of it
        // drifts against the vblank and halves the frame rate.
        if self.vsync_paced {
            self.render_all();
            el.set_control_flow(ControlFlow::Poll);
            return;
        }
        let now = Instant::now();
        if now >= self.next_frame {
            self.render_all();
            let period = Duration::from_secs_f64(1.0 / self.cfg.fps as f64);
            self.next_frame = (self.next_frame + period).max(now);
        }
        el.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

#[cfg(windows)]
fn preview_window(el: &ActiveEventLoop, base: winit::window::WindowAttributes, hwnd: isize) -> Option<Window> {
    use std::num::NonZeroIsize;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect;
    use winit::raw_window_handle::{RawWindowHandle, Win32WindowHandle};

    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: hwnd comes from Windows on the command line; GetClientRect fails cleanly if it is bogus.
    if unsafe { GetClientRect(hwnd as _, &mut rect) } == 0 {
        return None;
    }
    let handle = RawWindowHandle::Win32(Win32WindowHandle::new(NonZeroIsize::new(hwnd)?));
    // SAFETY: the parent window outlives us; we exit as soon as it is gone.
    let attrs = unsafe { base.with_parent_window(Some(handle)) }
        .with_decorations(false)
        .with_position(PhysicalPosition::new(0, 0))
        .with_inner_size(winit::dpi::PhysicalSize::new(
            (rect.right - rect.left).max(1) as u32,
            (rect.bottom - rect.top).max(1) as u32,
        ));
    el.create_window(attrs).ok()
}

#[cfg(not(windows))]
fn preview_window(_: &ActiveEventLoop, _: winit::window::WindowAttributes, _: isize) -> Option<Window> {
    None
}

/// Starts loading pictures in the background, unless they are turned off.
fn open_images(cfg: &Config, seed: u64) -> Option<Images> {
    (cfg.image_chance > 0.0).then(|| {
        let folder = cfg.image_folder.clone().unwrap_or_else(images::default_folder);
        Images::open(&folder, cfg.image_subfolders, seed)
    })
}

/// The icon built into the exe (see build.rs), for the title bar and taskbar.
#[cfg(windows)]
fn with_icon(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    use winit::platform::windows::{IconExtWindows, WindowAttributesExtWindows};
    use winit::window::Icon;
    let small = Icon::from_resource(1, Some(winit::dpi::PhysicalSize::new(32, 32))).ok();
    let large = Icon::from_resource(1, Some(winit::dpi::PhysicalSize::new(256, 256))).ok();
    attrs.with_window_icon(small).with_taskbar_icon(large)
}

#[cfg(not(windows))]
fn with_icon(attrs: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    attrs
}

#[cfg(windows)]
fn parent_alive(hwnd: isize) -> bool {
    // SAFETY: IsWindow accepts any value.
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindow(hwnd as _) != 0 }
}

#[cfg(not(windows))]
fn parent_alive(_: isize) -> bool {
    true
}

/// Lists the hotkeys in a message box on its own thread, so the animation keeps running.
#[cfg(windows)]
fn show_help() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_SETFOREGROUND, MB_TOPMOST};
    std::thread::spawn(|| {
        let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let (text, title) = (wide(HELP), wide("neopsych hotkeys"));
        // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call.
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                MB_ICONINFORMATION | MB_TOPMOST | MB_SETFOREGROUND,
            );
        }
    });
}

#[cfg(not(windows))]
fn show_help() {
    println!("{HELP}");
}

fn configure() {
    match config::write_default_if_missing() {
        Ok(path) => {
            if cfg!(windows) {
                let _ = std::process::Command::new("notepad.exe").arg(&path).spawn();
            } else {
                println!("Settings file: {}", path.display());
            }
        }
        Err(e) => eprintln!("could not write settings: {e}"),
    }
}

fn main() {
    let mode = parse_args();
    if mode == Mode::Configure {
        configure();
        return;
    }
    let event_loop = EventLoop::new().expect("failed to start event loop");
    let mut app = App::new(mode, config::load());
    event_loop.run_app(&mut app).expect("event loop failed");
}
