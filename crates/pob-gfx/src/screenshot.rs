//! `TakeScreenshot()` output (SG-oracle `lua/frame-probe.lua`, fixtures `frame-probe/*.png`): the original writes the
//! **frame after** the one that called it (shot of a red calling frame between green and blue frames is blue), as a
//! 24-bit RGB PNG of the whole client area, named `{%m%d%y_%H%M%S}.png` (SG-DLL-FACTS §3), into
//! `SimpleGraphic/Screenshots/` next to the exe; here the directory is the caller's
//! ([`crate::Gfx::set_screenshot_dir`]). The file name uses **UTC** — the local time zone needs the OS (FFI/crates are
//! not allowed here), a documented deviation.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Days since 1970-01-01 -> (year, month, day), proleptic Gregorian (H. Hinnant's `civil_from_days`).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// `%m%d%y_%H%M%S.png` of `t` (UTC).
pub fn file_name(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (y, mo, d) = civil_from_days(secs.div_euclid(86_400));
    let s = secs.rem_euclid(86_400);
    format!(
        "{mo:02}{d:02}{:02}_{:02}{:02}{:02}.png",
        y.rem_euclid(100),
        s / 3600,
        s / 60 % 60,
        s % 60
    )
}

/// Writes tightly packed RGBA rows (`w` x `h`, top row first) as a 24-bit RGB PNG `dir/<file_name(now)>`.
pub fn save(rgba: &[u8], w: u32, h: u32, dir: &Path) -> Result<PathBuf, image::ImageError> {
    std::fs::create_dir_all(dir)?;
    let rgb: Vec<u8> = rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let img = image::RgbImage::from_raw(w, h, rgb).expect("rgba size = w * h * 4");
    let path = dir.join(file_name(SystemTime::now()));
    img.save_with_format(&path, image::ImageFormat::Png)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn names_like_the_original() {
        // 2026-11-02 02:47:05 UTC -> "110226_024705.png" (the form of the oracle's `frame-probe` shots).
        let t = UNIX_EPOCH + Duration::from_secs(1_793_587_625);
        assert_eq!(file_name(t), "110226_024705.png");
        assert_eq!(file_name(UNIX_EPOCH), "010170_000000.png");
        assert_eq!(file_name(UNIX_EPOCH + Duration::from_secs(951_782_400)), "022900_000000.png");
    }
}
