#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod director;
mod gpu;
mod images;
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
use winit::event::{ElementState, MouseScrollDelta, WindowEvent};
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

const HOTKEYS: &str = "H hotkeys · Left/Right preset · Up/Down movement · Space pause · F fullscreen · Esc quit";

const HELP: &str = "\
Left / Right\tprevious / next preset
PgUp / PgDn\tjump a family (5 presets)
B\tnew background pattern
I\tshow a wallpaper picture
C\tblend into new colours
M\tnext symmetry
R\treverse colour cycling
+ / -\tcolour cycling faster / slower
Shift + wheel\tcolour cycling faster / slower
Up / Down\tmovement faster / slower
Mouse wheel\tmovement faster / slower
Space\tpause / resume
F or F11\ttoggle fullscreen
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
    /// Frames rendered since the last report; enabled by NEOPSYCH_STATS.
    stats: Option<(u32, Instant)>,
    vsync_paced: bool,
    /// Preset name last shown in the window title.
    title: String,
    images: Option<Images>,
}

impl App {
    fn new(mode: Mode, cfg: Config) -> App {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(1);
        let fixed = (cfg.preset > 0).then(|| cfg.preset as usize - 1);
        let director =
            Director::new(seed, cfg.speed, cfg.motion, cfg.preset_seconds, cfg.transition_seconds, fixed, cfg.image_chance);
        let images = if cfg.image_chance > 0.0 {
            let folder = cfg.image_folder.clone().unwrap_or_else(images::default_folder);
            Some(Images::open(&folder, seed))
        } else {
            None
        };
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
            stats: std::env::var_os("NEOPSYCH_STATS").map(|_| (0, now)),
            vsync_paced: false,
            title: String::new(),
            images,
        }
    }

    fn create_windows(&self, el: &ActiveEventLoop) -> Vec<Window> {
        let base = Window::default_attributes().with_title("neopsych");
        match self.mode {
            Mode::Saver => el
                .available_monitors()
                .filter_map(|m| {
                    // Always-on-top and fullscreen are applied once the surface
                    // is configured: DX12 rejects a surface on a window created topmost.
                    let attrs = base
                        .clone()
                        .with_decorations(false)
                        .with_position(m.position())
                        .with_inner_size(m.size());
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

    fn render_all(&mut self) {
        let Some(gpu) = &self.gpu else { return };
        let now = Instant::now();
        let dt = if self.paused { 0.0 } else { (now - self.last).as_secs_f32().min(0.25) };
        self.last = now;
        self.clock += dt as f64;
        self.director.image_ready = self.images.as_mut().is_some_and(|i| i.poll());
        let frame = self.director.update(self.clock, dt);
        if frame.new_image {
            if let Some(img) = self.images.as_mut().and_then(|i| i.take()) {
                for t in &mut self.targets {
                    t.set_picture(gpu, &img);
                }
            }
        }
        for t in &mut self.targets {
            t.render(gpu, &frame);
        }
        if matches!(self.mode, Mode::Windowed { .. }) && self.title != self.director.name() {
            self.title = self.director.name().to_string();
            for t in &self.targets {
                t.window.set_title(&format!("neopsych - {}   |   {HOTKEYS}", self.title));
            }
        }
        if let Some(s) = &mut self.stats {
            s.0 += 1;
            if now - s.1 >= Duration::from_secs(2) {
                let line = format!("{:.1} fps\n", s.0 as f64 / (now - s.1).as_secs_f64());
                if let Some(path) = std::env::var_os("NEOPSYCH_STATS") {
                    use std::io::Write;
                    let f = std::fs::OpenOptions::new().create(true).append(true).open(path);
                    let _ = f.and_then(|mut f| f.write_all(line.as_bytes()));
                }
                *s = (0, now);
            }
        }
    }
}

impl ApplicationHandler for App {
    /// Keeps speed changes made with hotkeys for next time.
    fn exiting(&mut self, _: &ActiveEventLoop) {
        if !matches!(self.mode, Mode::Windowed { .. }) {
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
            if self.mode == Mode::Saver {
                w.set_cursor_visible(false);
            }
            let surface = instance.create_surface(w.clone()).expect("failed to create surface");
            surfaces.push((w, surface));
        }
        // Highest refresh rate among our monitors; unknown counts as 60 Hz.
        let refresh_mhz = surfaces
            .iter()
            .filter_map(|(w, _)| w.current_monitor()?.refresh_rate_millihertz())
            .max()
            .unwrap_or(60_000);
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
            if matches!(self.mode, Mode::Windowed { .. }) {
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
                let t = self.clock;
                let d = &mut self.director;
                match &event.logical_key {
                    Key::Named(NamedKey::Escape) => el.exit(),
                    Key::Named(NamedKey::ArrowRight) => d.skip(1, t),
                    Key::Named(NamedKey::ArrowLeft) => d.skip(-1, t),
                    Key::Named(NamedKey::PageDown) => d.skip(presets::VARIATIONS as i32, t),
                    Key::Named(NamedKey::PageUp) => d.skip(-(presets::VARIATIONS as i32), t),
                    Key::Named(NamedKey::ArrowUp) => d.scale_motion(1.25),
                    Key::Named(NamedKey::ArrowDown) => d.scale_motion(0.8),
                    Key::Named(NamedKey::Space) => self.paused = !self.paused,
                    Key::Named(NamedKey::F11) => self.toggle_fullscreen(id),
                    Key::Character(c) => match c.to_ascii_lowercase().as_str() {
                        "b" => d.next_background(t),
                        "i" => d.show_picture(t),
                        "c" => d.next_palette(t),
                        "m" => d.next_symmetry(),
                        "r" => d.reverse(),
                        "+" | "=" => d.scale_speed(1.25),
                        "-" => d.scale_speed(0.8),
                        "f" => self.toggle_fullscreen(id),
                        "h" => show_help(),
                        _ => {}
                    },
                    _ => {}
                }
            }
            WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. } if saver => el.exit(),
            WindowEvent::ModifiersChanged(m) => self.shift = m.state().shift_key(),
            // Scroll: movement speed. Shift + scroll: colour speed.
            WindowEvent::MouseWheel { delta, .. } => {
                let up = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32,
                };
                if up != 0.0 {
                    let factor = if up > 0.0 { 1.15 } else { 1.0 / 1.15 };
                    if self.shift {
                        self.director.scale_speed(factor);
                    } else {
                        self.director.scale_motion(factor);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } if saver => {
                let origin = *self.cursor_origin.entry(id).or_insert(position);
                let (dx, dy) = (position.x - origin.x, position.y - origin.y);
                if dx * dx + dy * dy > MOVE_TOLERANCE * MOVE_TOLERANCE {
                    el.exit();
                }
            }
            WindowEvent::RedrawRequested => {}
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
