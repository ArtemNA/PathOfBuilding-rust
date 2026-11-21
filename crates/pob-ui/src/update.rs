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

/// Where the updates are downloaded, the record of the last one ([`RECORD`]) and the marks of the running windows ([`Running`]).
pub fn update_dir() -> PathBuf {
    std::env::temp_dir().join("pob-rust-update")
}

pub fn download_path(name: &str) -> PathBuf {
    update_dir().join(name)
}

/// The record of a finished download in [`update_dir`]: what the next start may install before its window opens.
pub const RECORD: &str = "ready.json";
/// The marks of the running windows in [`update_dir`].
const RUNNING: &str = "running";

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

/// Writes [`RECORD`] for a download whose file has the checksum `sha256` (what GitHub reported, and the download was checked against).
pub fn write_record(dir: &Path, p: &Pending, sha256: &str) -> Result<(), String> {
    let file = p.file.as_ref().ok_or("nothing was downloaded")?;
    let name = file.file_name().ok_or("the download has no name")?.to_string_lossy();
    let kind = match p.kind {
        Install::WindowsSetup => "windows",
        Install::LinuxScript => "linux",
        Install::Manual => return Ok(()),
    };
    let j = serde_json::json!({ "version": p.version, "kind": kind, "file": name, "sha256": sha256, "page": p.page });
    let tmp = dir.join(format!("{RECORD}.tmp"));
    std::fs::write(&tmp, j.to_string()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, dir.join(RECORD)).map_err(|e| format!("{RECORD}: {e}"))
}

/// The update a start can install before its window opens: [`RECORD`] names a later version than `current` of the way this copy is installed
/// (`kind`), and its file has the recorded checksum. A record that cannot be used any more (an older version: the update is in) goes with its file;
/// any other doubt leaves things as they are and the start goes on.
pub fn ready_at_start(dir: &Path, current: &str, kind: Install) -> Option<Pending> {
    let text = std::fs::read_to_string(dir.join(RECORD)).ok()?;
    let j: serde_json::Value = serde_json::from_str(&text).ok()?;
    let (version, name, sha) = (j["version"].as_str()?, j["file"].as_str()?, j["sha256"].as_str()?);
    // only a plain name: the record cannot point out of the folder
    if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
        return None;
    }
    let file = dir.join(name);
    if !is_newer(version, current) {
        let _ = std::fs::remove_file(dir.join(RECORD));
        let _ = std::fs::remove_file(&file);
        return None;
    }
    let recorded = match j["kind"].as_str()? {
        "windows" => Install::WindowsSetup,
        "linux" => Install::LinuxScript,
        _ => return None,
    };
    if recorded != kind || kind == Install::Manual {
        return None;
    }
    let bytes = std::fs::read(&file).ok()?;
    if pob_data::sha256::sha256_hex(&bytes) != sha.to_ascii_lowercase() {
        let _ = std::fs::remove_file(dir.join(RECORD));
        return None;
    }
    Some(Pending {
        version: version.to_string(),
        kind,
        file: Some(file),
        page: j["page"].as_str().unwrap_or("").to_string(),
    })
}

/// The mark of a running window: a file under [`update_dir`]`/running/` this process holds locked until it exits. The installer closes every
/// window of the application, so a start installs an update only when no other window is there to lose its work.
pub struct Running {
    _lock: std::fs::File,
    path: PathBuf,
}

impl Running {
    /// Marks this process as running. `None` when the mark cannot be made: then nothing may be installed at the start.
    pub fn mark(dir: &Path) -> Option<Running> {
        Self::mark_as(dir, &std::process::id().to_string())
    }

    /// [`Self::mark`] under a name of its own: one process plays several windows in the tests.
    fn mark_as(dir: &Path, name: &str) -> Option<Running> {
        let d = dir.join(RUNNING);
        std::fs::create_dir_all(&d).ok()?;
        let path = d.join(name);
        let f = std::fs::File::create(&path).ok()?;
        f.try_lock().ok()?;
        Some(Running { _lock: f, path })
    }

    /// Another window of the application is running: a mark that is still locked. A mark nobody holds (its process is gone) is removed.
    pub fn others(&self) -> bool {
        let Ok(rd) = std::fs::read_dir(self.path.parent().expect("a mark is in a folder")) else {
            return true;
        };
        let mut others = false;
        for e in rd.flatten() {
            let p = e.path();
            if p == self.path {
                continue;
            }
            let Ok(f) = std::fs::OpenOptions::new().write(true).open(&p) else {
                // it exists but cannot be opened: count it, rather than install over a window
                others = true;
                continue;
            };
            match f.try_lock() {
                Ok(()) => {
                    drop(f);
                    let _ = std::fs::remove_file(&p);
                }
                Err(_) => others = true,
            }
        }
        others
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// What a start does before its window opens: installs the update a previous run downloaded when this is the only window. `Ok(true)`: the
/// installer runs and this process must exit (the new version opens by itself). `uri`: a `pob://` link the start was asked to open goes nowhere
/// through the installer, so such a start leaves the update for later.
pub fn install_at_start(
    dir: &Path,
    current: &str,
    kind: Install,
    running: Option<&Running>,
    uri: bool,
    install: impl FnOnce(&Pending) -> Result<(), String>,
) -> Result<bool, String> {
    let Some(running) = running else { return Ok(false) };
    if uri {
        return Ok(false);
    }
    // the other windows first: the checksum reads the whole installer
    if running.others() {
        return Ok(false);
    }
    let Some(p) = ready_at_start(dir, current, kind) else {
        return Ok(false);
    };
    install(&p)?;
    Ok(true)
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

    /// A folder of its own for each test.
    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pob-update-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("mkdir");
        d
    }

    /// A downloaded "installer" of 0.9.0 with its record.
    fn downloaded(d: &Path) -> Pending {
        let file = d.join("pob-rust-0.9.0-windows-x64-setup.exe");
        std::fs::write(&file, b"an installer").expect("write");
        let p = Pending {
            version: "0.9.0".into(),
            kind: Install::WindowsSetup,
            file: Some(file),
            page: "https://github.com/o/r/releases/tag/v0.9.0".into(),
        };
        write_record(d, &p, &pob_data::sha256::sha256_hex(b"an installer")).expect("record");
        p
    }

    #[test]
    fn a_recorded_download_is_ready_only_while_it_is_the_one_recorded() {
        let d = dir("ready");
        let p = downloaded(&d);
        assert_eq!(ready_at_start(&d, "0.2.6", Install::WindowsSetup), Some(p.clone()));
        // another way of installing: not this copy's file, the record stays
        assert_eq!(ready_at_start(&d, "0.2.6", Install::Manual), None);
        assert_eq!(ready_at_start(&d, "0.2.6", Install::LinuxScript), None);
        assert!(d.join(RECORD).is_file());
        // other bytes than the checksum: never run, the record goes
        std::fs::write(p.file.as_ref().unwrap(), b"an installeR").expect("write");
        assert_eq!(ready_at_start(&d, "0.2.6", Install::WindowsSetup), None);
        assert!(!d.join(RECORD).exists());

        // the update is in (this is 0.9.0 or later): the record and the file go
        let p = downloaded(&d);
        assert_eq!(ready_at_start(&d, "0.9.0", Install::WindowsSetup), None);
        assert!(!d.join(RECORD).exists() && !p.file.unwrap().exists());

        // a record that points out of its folder
        downloaded(&d);
        let text = std::fs::read_to_string(d.join(RECORD))
            .unwrap()
            .replace("pob-rust-0.9.0", "../pob-rust-0.9.0");
        std::fs::write(d.join(RECORD), text).unwrap();
        assert_eq!(ready_at_start(&d, "0.2.6", Install::WindowsSetup), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_marks_of_the_windows() {
        let d = dir("marks");
        let me = Running::mark_as(&d, "1").expect("mark");
        assert!(!me.others());
        let other = Running::mark_as(&d, "2").expect("mark");
        assert!(me.others() && other.others());
        drop(other);
        assert!(!me.others());
        // the mark of a window that died without removing it: nobody holds it
        std::fs::write(d.join(RUNNING).join("3"), b"").unwrap();
        assert!(!me.others());
        assert!(!d.join(RUNNING).join("3").exists());
        drop(me);
        assert_eq!(std::fs::read_dir(d.join(RUNNING)).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_start_installs_only_when_it_is_the_only_window() {
        let d = dir("start");
        let p = downloaded(&d);
        let run = |running: Option<&Running>, uri: bool| {
            let mut got = None;
            let r = install_at_start(&d, "0.2.6", Install::WindowsSetup, running, uri, |q| {
                got = Some(q.clone());
                Ok(())
            });
            (r, got)
        };
        let me = Running::mark_as(&d, "1").expect("mark");
        let other = Running::mark_as(&d, "2").expect("mark");
        assert_eq!(run(Some(&me), false), (Ok(false), None), "another window is open");
        drop(other);
        assert_eq!(run(Some(&me), true), (Ok(false), None), "a pob:// link would be lost");
        assert_eq!(run(None, false), (Ok(false), None), "no mark: the others cannot be seen");
        assert_eq!(run(Some(&me), false), (Ok(true), Some(p)), "alone: the installer runs");
        // the installer did not start: the start goes on
        let r = install_at_start(&d, "0.2.6", Install::WindowsSetup, Some(&me), false, |_| Err("no".into()));
        assert_eq!(r, Err("no".to_string()));
        // nothing downloaded
        let empty = dir("start-empty");
        let r = install_at_start(&empty, "0.2.6", Install::WindowsSetup, Some(&me), false, |_| {
            panic!("nothing to install")
        });
        assert_eq!(r, Ok(false));
        drop(me);
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&empty);
    }

    /// The other window of [`a_window_of_another_process_is_seen`]: holds a mark until told to stop.
    #[test]
    #[ignore = "run by a_window_of_another_process_is_seen"]
    fn hold_a_mark_for_another_process() {
        let Some(d) = std::env::var_os("POB_MARK_DIR").map(PathBuf::from) else {
            return;
        };
        let m = Running::mark_as(&d, "child").expect("mark");
        std::fs::write(d.join("ready"), b"").unwrap();
        let t = std::time::Instant::now();
        while !d.join("stop").exists() && t.elapsed().as_secs() < 30 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        drop(m);
    }

    /// The real case: the other window is another process.
    #[test]
    fn a_window_of_another_process_is_seen() {
        let d = dir("process");
        let me = Running::mark_as(&d, "1").expect("mark");
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "update::tests::hold_a_mark_for_another_process",
                "--ignored",
                "--exact",
                "--test-threads=1",
            ])
            .env("POB_MARK_DIR", &d)
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("the other process");
        let t = std::time::Instant::now();
        while !d.join("ready").exists() {
            assert!(t.elapsed().as_secs() < 30, "the other process did not mark");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(me.others(), "the window of the other process");
        std::fs::write(d.join("stop"), b"").unwrap();
        assert!(child.wait().expect("wait").success());
        assert!(!me.others(), "the other process has exited");
        drop(me);
        let _ = std::fs::remove_dir_all(&d);
    }
}
