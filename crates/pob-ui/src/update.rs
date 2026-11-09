//! The application's own updates, from the GitHub releases of the port (not the original's `UpdateCheck.lua`, which updates the original's scripts).
//! The installer on Windows and `install.sh` on Linux can replace the application; any other copy (a zip, the macOS archive, a folder of its own) gets the release page.
//! What is run comes only from the checksum GitHub reports for the release.

use std::path::{Path, PathBuf};

pub const LATEST_URL: &str = "https://api.github.com/repos/ArtemNA/PathOfBuilding-rust/releases/latest";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    /// Lower-case hex.
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub page: String,
    pub assets: Vec<Asset>,
}

/// The reply of [`LATEST_URL`]. An asset without a checksum, or served from elsewhere, is left out: it could not be trusted.
pub fn parse_release(body: &str) -> Result<Release, String> {
    let j: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("not a release: {e}"))?;
    let tag = j["tag_name"].as_str().ok_or("the release has no tag")?;
    let version = tag.strip_prefix('v').unwrap_or(tag).to_string();
    let assets = j["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            let url = a["browser_download_url"].as_str()?;
            let sha256 = a["digest"].as_str()?.strip_prefix("sha256:")?;
            (url.starts_with("https://github.com/") && sha256.len() == 64 && sha256.bytes().all(|b| b.is_ascii_hexdigit())).then(|| Asset {
                name: a["name"].as_str().unwrap_or("").to_string(),
                url: url.to_string(),
                sha256: sha256.to_ascii_lowercase(),
            })
        })
        .collect();
    Ok(Release {
        version,
        page: j["html_url"].as_str().unwrap_or("").to_string(),
        assets,
    })
}

fn parts(v: &str) -> Option<[u64; 3]> {
    let mut it = v.split('.').map(|p| p.parse::<u64>().ok());
    let out = [it.next()??, it.next()??, it.next()??];
    it.next().is_none().then_some(out)
}

/// `candidate` is a later version than `current`. Anything that is not three numbers is not an update.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((parts(candidate), parts(current)), (Some(a), Some(b)) if a > b)
}

/// How this copy of the application can be replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Install {
    WindowsSetup,
    LinuxScript,
    /// Only the user can: the release page.
    Manual,
}

pub fn install_kind(exe_dir: &Path) -> Install {
    if cfg!(windows) && exe_dir.join("unins000.exe").is_file() {
        return Install::WindowsSetup;
    }
    if cfg!(target_os = "linux") && exe_dir.join("install.sh").is_file() && exe_dir.file_name().is_some_and(|n| n == "pob-rust") {
        return Install::LinuxScript;
    }
    Install::Manual
}

pub fn asset_name(kind: Install, version: &str) -> Option<String> {
    match kind {
        Install::WindowsSetup => Some(format!("pob-rust-{version}-windows-x64-setup.exe")),
        Install::LinuxScript => Some(format!("pob-rust-{version}-linux-x64.tar.gz")),
        Install::Manual => None,
    }
}

pub fn download_path(name: &str) -> PathBuf {
    std::env::temp_dir().join("pob-rust-update").join(name)
}

/// An update found, ready to be applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub version: String,
    pub kind: Install,
    pub file: Option<PathBuf>,
    pub page: String,
}

/// Starts what replaces the application, which must then exit. The new version starts by itself.
pub fn start_install(p: &Pending, exe: &Path) -> Result<(), String> {
    let file = p.file.as_ref().ok_or("nothing was downloaded")?;
    let spawned = match p.kind {
        Install::WindowsSetup => std::process::Command::new(file)
            .args([
                "/VERYSILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/CLOSEAPPLICATIONS",
                "/relaunch=1",
            ])
            .spawn(),
        Install::LinuxScript => std::process::Command::new("sh")
            .arg("-c")
            .arg(r#"sleep 1; d=$(mktemp -d) && tar -xzf "$0" -C "$d" && "$d"/*/install.sh && rm -rf "$d"; exec "$1""#)
            .arg(file)
            .arg(exe)
            .spawn(),
        Install::Manual => return Err("this copy cannot be updated in place".into()),
    };
    spawned.map(drop).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "5cf92a7f950dc835f50e0db377f0418fd7d1b9eece3e86cee85be7f19cf2991c";

    fn body(url: &str, digest: &str) -> String {
        format!(
            r#"{{"tag_name":"v0.2.0","html_url":"https://github.com/o/r/releases/tag/v0.2.0","assets":[{{"name":"a.exe","browser_download_url":"{url}","digest":"{digest}"}}]}}"#
        )
    }

    #[test]
    fn a_release_is_read() {
        let r = parse_release(&body("https://github.com/o/r/a.exe", &format!("sha256:{SHA}"))).expect("release");
        assert_eq!(r.version, "0.2.0");
        assert_eq!(r.assets.len(), 1);
        assert_eq!(r.assets[0].sha256, SHA);
    }

    #[test]
    fn an_asset_that_cannot_be_checked_or_comes_from_elsewhere_is_left_out() {
        let good = format!("sha256:{SHA}");
        assert!(
            parse_release(&body("https://evil.example/a.exe", &good))
                .expect("release")
                .assets
                .is_empty()
        );
        assert!(
            parse_release(&body("http://github.com/o/r/a.exe", &good))
                .expect("release")
                .assets
                .is_empty()
        );
        assert!(
            parse_release(&body("https://github.com/o/r/a.exe", "sha1:abc"))
                .expect("release")
                .assets
                .is_empty()
        );
        assert!(
            parse_release(&body("https://github.com/o/r/a.exe", "sha256:short"))
                .expect("release")
                .assets
                .is_empty()
        );
        assert!(parse_release("not json").is_err());
    }

    #[test]
    fn only_a_later_version_is_an_update() {
        assert!(is_newer("0.1.6", "0.1.5"));
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("0.1.10", "0.1.9"), "numbers, not text");
        assert!(!is_newer("0.1.5", "0.1.5"));
        assert!(!is_newer("0.1.4", "0.1.5"), "never back");
        assert!(!is_newer("0.2.0-rc1", "0.1.5"));
        assert!(!is_newer("nonsense", "0.1.5"));
    }

    #[test]
    fn the_right_file_for_each_way_of_installing() {
        assert_eq!(
            asset_name(Install::WindowsSetup, "1.2.3").as_deref(),
            Some("pob-rust-1.2.3-windows-x64-setup.exe")
        );
        assert_eq!(
            asset_name(Install::LinuxScript, "1.2.3").as_deref(),
            Some("pob-rust-1.2.3-linux-x64.tar.gz")
        );
        assert_eq!(asset_name(Install::Manual, "1.2.3"), None);
    }

    #[test]
    fn a_folder_without_an_uninstaller_is_updated_by_hand() {
        let d = std::env::temp_dir().join(format!("pob-update-kind-{}", std::process::id()));
        std::fs::create_dir_all(&d).expect("dir");
        assert_eq!(install_kind(&d), Install::Manual);
        if cfg!(windows) {
            std::fs::write(d.join("unins000.exe"), b"x").expect("file");
            assert_eq!(install_kind(&d), Install::WindowsSetup);
        }
        std::fs::remove_dir_all(&d).expect("clean");
    }
}
