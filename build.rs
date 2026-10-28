//! Builds the icon into the Windows executable, with rc.exe on MSVC or
//! windres on MinGW.

fn main() {
    println!("cargo:rerun-if-changed=assets/neopsych.rc");
    println!("cargo:rerun-if-changed=assets/neopsych.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/neopsych.rc", embed_resource::NONE).manifest_optional().unwrap();
    }
}
