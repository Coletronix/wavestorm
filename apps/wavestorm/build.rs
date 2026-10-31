//! Embeds the application icon (and version details) in the Windows executable, so Explorer, the
//! taskbar and shortcuts show it. Other targets set their icons in their packages.

const ICON: &str = "../../crates/ui/assets/icon.ico";

fn main() {
    println!("cargo:rerun-if-changed={ICON}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(ICON).set("ProductName", "WAVEStorm").set("FileDescription", "WAVEStorm");
    // Without the Windows resource compiler the program still builds, just without its icon.
    if let Err(e) = res.compile() {
        println!("cargo:warning=no icon embedded in the executable: {e}");
    }
}
