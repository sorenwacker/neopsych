//! Picks random pictures from a folder (optionally with its subfolders) and decodes them to greyscale on
//! a background thread, always keeping one ready so showing it never stalls.

use crate::rng::Rng;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

/// Larger pictures are scaled down to this many pixels on their long side.
const MAX_SIDE: u32 = 4096;

pub struct Gray {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub struct Images {
    requests: Sender<()>,
    results: Receiver<Gray>,
    in_flight: bool,
    ready: Option<Gray>,
}

impl Images {
    /// Starts scanning `folder` and its subfolders in the background. If no
    /// pictures turn up, none ever become ready and patterns are shown instead.
    pub fn open(folder: &Path, subfolders: bool, seed: u64) -> Images {
        let (requests, jobs) = channel::<()>();
        let (done, results) = channel();
        let folder = folder.to_path_buf();
        std::thread::spawn(move || {
            let files = scan(&folder, subfolders);
            if files.is_empty() {
                return;
            }
            let mut rng = Rng::new(seed);
            for () in jobs {
                // Skip unreadable files, but give up rather than spin if none decode.
                let picture = (0..20).find_map(|_| decode(&files[rng.below(files.len() as u32) as usize]));
                let Some(picture) = picture else { return };
                if done.send(picture).is_err() {
                    return;
                }
            }
        });
        let mut images = Images { requests, results, in_flight: false, ready: None };
        images.prefetch();
        images
    }

    /// True once a picture is decoded and waiting.
    pub fn poll(&mut self) -> bool {
        if self.ready.is_none() && self.in_flight {
            if let Ok(img) = self.results.try_recv() {
                self.in_flight = false;
                self.ready = Some(img);
            }
        }
        self.ready.is_some()
    }

    /// Hands over the waiting picture and starts decoding the next one.
    pub fn take(&mut self) -> Option<Gray> {
        let img = self.ready.take();
        self.prefetch();
        img
    }

    fn prefetch(&mut self) {
        if !self.in_flight {
            self.in_flight = self.requests.send(()).is_ok();
        }
    }
}

/// Every picture file in `root`, and in its subfolders if asked (not following links).
fn scan(root: &Path, subfolders: bool) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                if subfolders {
                    dirs.push(path);
                }
            } else if kind.is_file() {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                if matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp" | "bmp") {
                    files.push(path);
                }
            }
        }
    }
    files
}

fn decode(path: &Path) -> Option<Gray> {
    let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
    let img = if img.width().max(img.height()) > MAX_SIDE {
        img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let gray = img.into_luma8();
    Some(Gray { width: gray.width(), height: gray.height(), pixels: gray.into_raw() })
}

/// The user's Pictures folder, used when no folder is configured.
pub fn default_folder() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
    PathBuf::from(home).join("Pictures")
}
