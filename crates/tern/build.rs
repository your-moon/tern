//! Windows only: embeds the app icon in tern.exe. Everywhere else this does nothing.
#![allow(clippy::print_stdout)]

#[cfg(windows)]
fn main() {
    println!("cargo:rerun-if-changed=../../packaging/windows/tern.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../packaging/windows/tern.ico");
    if let Err(e) = res.compile() {
        // A missing resource compiler costs the icon, not the build.
        println!("cargo:warning=tern.exe icon not embedded: {e}");
    }
}

#[cfg(not(windows))]
fn main() {}
