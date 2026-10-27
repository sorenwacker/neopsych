//! Builds the icon into the Windows executable, using windres from MinGW.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/neopsych.rc");
    println!("cargo:rerun-if-changed=assets/neopsych.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("icon.o");
    let built = Command::new("windres")
        .args(["--include-dir", "assets", "assets/neopsych.rc", "-O", "coff", "-o"])
        .arg(&out)
        .status()
        .is_ok_and(|s| s.success());
    if built {
        println!("cargo:rustc-link-arg-bins={}", out.display());
    } else {
        println!("cargo:warning=windres not found; building without an icon");
    }
}
