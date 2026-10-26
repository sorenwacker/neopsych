//! Picks random pictures from a folder and decodes them to greyscale on a
//! background thread, always keeping one ready so showing it never stalls.

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
    files: Vec<PathBuf>,
    rng: Rng,
    requests: Sender<PathBuf>,
    results: Receiver<Option<Gray>>,
    in_flight: bool,
    ready: Option<Gray>,
}

impl Images {
    /// None if the folder has no usable pictures.
    pub fn open(folder: &Path, seed: u64) -> Option<Images> {
        let files: Vec<PathBuf> = std::fs::read_dir(folder)
            .ok()?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp" | "bmp")
            })
            .collect();
        if files.is_empty() {
            return None;
        }
        let (requests, jobs) = channel::<PathBuf>();
        let (done, results) = channel();
        std::thread::spawn(move || {
            for path in jobs {
                if done.send(decode(&path)).is_err() {
                    break;
                }
            }
        });
        let mut images = Images { files, rng: Rng::new(seed), requests, results, in_flight: false, ready: None };
        images.prefetch();
        Some(images)
    }

    /// True once a picture is decoded and waiting.
    pub fn poll(&mut self) -> bool {
        if self.ready.is_none() && self.in_flight {
            if let Ok(result) = self.results.try_recv() {
                self.in_flight = false;
                match result {
                    Some(img) => self.ready = Some(img),
                    // Unreadable file: try another.
                    None => self.prefetch(),
                }
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
        if self.in_flight {
            return;
        }
        let path = self.files[self.rng.below(self.files.len() as u32) as usize].clone();
        self.in_flight = self.requests.send(path).is_ok();
    }
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

/// Pictures\Wallpapers in the user's home, used when no folder is configured.
pub fn default_folder() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
    PathBuf::from(home).join("Pictures").join("Wallpapers")
}
