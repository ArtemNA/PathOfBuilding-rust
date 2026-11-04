//! With the `package` feature on an MSVC target: the exe's own icon. `link.exe` takes a compiled resource file on its
//! command line, and the format is simple enough to write here, so neither a new crate nor `rc.exe` is needed.

use std::path::PathBuf;

const ICON: &str = "assets/icon.png";
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
/// `MOVEABLE | PURE | DISCARDABLE`, what `rc.exe` writes for an icon.
const MEMORY_FLAGS: u16 = 0x1030;
const LANG_EN_US: u16 = 0x0409;

/// One `RESOURCEHEADER` with numeric type and name, the data, the padding to 4 bytes.
fn resource(out: &mut Vec<u8>, ty: u16, name: u16, memory_flags: u16, language: u16, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    for v in [0xFFFF, ty, 0xFFFF, name] {
        out.extend_from_slice(&u16::to_le_bytes(v));
    }
    out.extend_from_slice(&0u32.to_le_bytes()); // DataVersion
    out.extend_from_slice(&memory_flags.to_le_bytes());
    out.extend_from_slice(&language.to_le_bytes());
    out.extend_from_slice(&[0; 8]); // Version, Characteristics
    out.extend_from_slice(data);
    out.resize(out.len().next_multiple_of(4), 0);
}

/// The `.res` image of one PNG icon entry (Windows reads PNG entries since Vista). `size` is the image's side, 256 at most.
fn res_of_png(png: &[u8], size: u32) -> Vec<u8> {
    let mut group = Vec::new();
    for v in [0u16, 1, 1] {
        group.extend_from_slice(&v.to_le_bytes()); // reserved, type = icon, one image
    }
    // 0 means 256 in the directory's byte
    let side = if size >= 256 { 0 } else { size as u8 };
    group.extend_from_slice(&[side, side, 0, 0]);
    group.extend_from_slice(&1u16.to_le_bytes()); // planes
    group.extend_from_slice(&32u16.to_le_bytes()); // bits
    group.extend_from_slice(&(png.len() as u32).to_le_bytes());
    group.extend_from_slice(&1u16.to_le_bytes()); // the id of the RT_ICON below

    // the first entry of every .res file is an empty one with type 0
    let mut out = Vec::new();
    resource(&mut out, 0, 0, 0, 0, &[]);
    resource(&mut out, RT_ICON, 1, MEMORY_FLAGS, LANG_EN_US, png);
    resource(&mut out, RT_GROUP_ICON, 1, MEMORY_FLAGS, LANG_EN_US, &group);
    out
}

fn main() {
    println!("cargo:rerun-if-changed={ICON}");
    println!("cargo:rerun-if-changed=build.rs");
    let env_is = |k: &str, v: &str| std::env::var(k).is_ok_and(|x| x == v);
    if std::env::var_os("CARGO_FEATURE_PACKAGE").is_none() || !env_is("CARGO_CFG_TARGET_ENV", "msvc") {
        return;
    }
    let png = std::fs::read(ICON).expect("the icon of the application");
    // IHDR: the width is the big-endian u32 at byte 16
    let size = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("icon.res");
    std::fs::write(&out, res_of_png(&png, size)).expect("write icon.res");
    println!("cargo:rustc-link-arg-bins={}", out.display());
}
