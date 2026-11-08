//! Every build gets the app icon at 96 px for the settings window, made from
//! packaging/icon/flockswitch.png.

#[allow(dead_code)]
#[path = "packaging/icon/master.rs"]
mod master;

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=packaging/icon/flockswitch.png");
    println!("cargo:rerun-if-changed=packaging/icon/master.rs");
    println!("cargo:rerun-if-changed=src/ble_macos.m");
    println!("cargo:rerun-if-changed=build.rs");

    cc::Build::new()
        .file("src/ble_macos.m")
        .flag("-fobjc-arc")
        .compile("flockswitch_ble");
    println!("cargo:rustc-link-lib=framework=CoreBluetooth");
    println!("cargo:rustc-link-lib=framework=Foundation");

    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let window_icon = master::scaled(Path::new(MASTER), WINDOW_ICON);
    std::fs::write(out.join("window-icon.rgba"), window_icon).expect("writing window icon");
}

const MASTER: &str = "packaging/icon/flockswitch.png";
/// Side of the RGBA icon in OUT_DIR/window-icon.rgba (kept in sync with settings_ui.rs).
const WINDOW_ICON: u32 = 96;
