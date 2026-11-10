//! New-version check, installing a new version, and the developer report.
//!
//! The check reads the latest release from GitHub's public API (no account, nothing
//! about the user is sent) and compares it with this build. HTTP goes through the
//! system's `curl`, which keeps TLS out of the program, and only over https. A
//! downloaded file must match the SHA256SUMS.txt published with the release; nothing
//! else is ever unpacked or installed.
//!
//! Installing (`install`) replaces the running flockswitch.app in place: the release is
//! downloaded into a folder next to the app (same volume, so the swap is two renames),
//! unpacked with `ditto`, checked (version, signature), swapped in, and the old copy
//! deleted. The menu-bar app's replacement watch (tray.rs) then notices the new version
//! and restarts into it, as after a Homebrew upgrade. An app that cannot be replaced
//! where it is (a folder it cannot write to, App Translocation, inside Homebrew's
//! Caskroom) gets the file in Downloads instead.

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const RELEASES_API: &str = "https://api.github.com/repos/WWhaleFe/flockswitch-mac/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/WWhaleFe/flockswitch-mac/releases/latest";
pub const ISSUES_PAGE: &str = "https://github.com/WWhaleFe/flockswitch-mac/issues/new/choose";
pub const REPORT_EMAIL: &str = "feedback.wwhale@gmail.com";
/// How often the running app looks for a new version (plus once shortly after start).
pub const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Where the menu-bar app is with updates (shown in its menu).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Failed,
    Downloading(Release),
    /// Saved to Downloads (this copy could not replace itself).
    Downloaded(PathBuf),
    Installing(Release),
    /// The new version (this one) is in place; the app is about to restart into it.
    Installed(String),
    /// Downloading or installing failed, and why.
    UpdateFailed(Release, String),
}

impl UpdateState {
    /// Downloading or installing, or done with that: a check now would only get in the way.
    pub fn busy(&self) -> bool {
        matches!(self, UpdateState::Downloading(_) | UpdateState::Downloaded(_) | UpdateState::Installing(_) | UpdateState::Installed(_))
    }
}

/// Checks GitHub and records the outcome; returns it as well.
pub fn check_into(shared: &crate::shared::Shared) -> UpdateState {
    shared.set_status(|s| s.update = UpdateState::Checking);
    let next = match latest() {
        Ok(r) => {
            let newer = r.is_newer_than_this_build();
            log::info!("update check: latest {} ({})", r.version, if newer { "newer" } else { "up to date" });
            if newer { UpdateState::Available(r) } else { UpdateState::UpToDate }
        }
        Err(e) => {
            log::info!("update check failed: {e:#}");
            UpdateState::Failed
        }
    };
    shared.set_status(|s| s.update = next.clone());
    next
}

/// Installs `release` (see `install`) and records each step for the menu. With
/// `or_download` (the user asked for it) an app that cannot replace itself gets the file
/// in Downloads instead; without it (automatic) it stays at "new version available".
pub fn install_with_status(shared: &crate::shared::Shared, release: Release, or_download: bool) {
    let step = |s: Step| {
        let next = match s {
            Step::Downloading => UpdateState::Downloading(release.clone()),
            Step::Installing => UpdateState::Installing(release.clone()),
        };
        shared.set_status(|st| st.update = next);
    };
    let next = match install(&release, or_download, step) {
        Outcome::Installed(v) => UpdateState::Installed(v),
        Outcome::Downloaded(path, _) => UpdateState::Downloaded(path),
        Outcome::NotInstallable(_) => UpdateState::Available(release),
        Outcome::Failed(why) => UpdateState::UpdateFailed(release, why),
    };
    shared.set_status(|s| s.update = next);
}

/// `install_with_status` on its own thread (from a menu click).
pub fn install_into(shared: std::sync::Arc<crate::shared::Shared>, release: Release, or_download: bool) {
    std::thread::spawn(move || install_with_status(&shared, release, or_download));
}

/// The newest release on GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// "0.3.2" (without the leading v).
    pub version: String,
    pub page: String,
    /// The file for this kind of install and the checksums published with it:
    /// (file name, download URL, SHA256SUMS.txt URL). None when either is missing.
    pub asset: Option<(String, String, String)>,
}

impl Release {
    pub fn is_newer_than_this_build(&self) -> bool {
        is_newer(&self.version, env!("CARGO_PKG_VERSION"))
    }
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

/// The system's curl, quiet and following redirects, https only (redirects too).
fn curl() -> Command {
    let mut c = Command::new("curl");
    c.args(["--silent", "--show-error", "--fail", "--location", "--proto", "=https", "--proto-redir", "=https"]);
    c.arg("--user-agent").arg(concat!("flockswitch/", env!("CARGO_PKG_VERSION")));
    c
}

/// Asks GitHub for the latest release.
pub fn latest() -> Result<Release> {
    let out = curl()
        .args(["--max-time", "15", "--header", "Accept: application/vnd.github+json", RELEASES_API])
        .output()
        .context("cannot run curl")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let r: GhRelease = serde_json::from_slice(&out.stdout).context("unexpected reply from GitHub")?;
    let version = r.tag_name.trim().trim_start_matches(['v', 'V']).to_string();
    let names: Vec<&str> = r.assets.iter().map(|a| a.name.as_str()).collect();
    let url = |name: &str| r.assets.iter().find(|a| a.name == name).map(|a| a.browser_download_url.clone());
    let asset = pick_asset(&names).and_then(|name| Some((name.to_string(), url(name)?, url(SUMS)?)));
    Ok(Release { version, page: r.html_url, asset })
}

/// The checksum list published with every release (`sha256sum` output).
const SUMS: &str = "SHA256SUMS.txt";

/// The release file with the app (flockswitch-macos-…zip).
fn pick_asset<'a>(names: &[&'a str]) -> Option<&'a str> {
    names.iter().copied().find(|n| n.contains("macos") && n.ends_with(".zip"))
}

/// "1.2.10" > "1.2.9"; parts that are not numbers count as 0.
pub fn is_newer(a: &str, b: &str) -> bool {
    let parts = |s: &str| -> Vec<u64> {
        s.split('.').map(|p| p.chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap_or(0)).collect()
    };
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        let (l, r) = (x.get(i).copied().unwrap_or(0), y.get(i).copied().unwrap_or(0));
        if l != r {
            return l > r;
        }
    }
    false
}

/// Downloads a release file into the Downloads folder (never over an existing file),
/// marked as downloaded so Gatekeeper checks it. A file whose SHA-256 is not the one in
/// the release's checksum list is deleted.
pub fn download(name: &str, url: &str, sums_url: &str) -> Result<PathBuf> {
    let dir = dirs::download_dir().or_else(|| dirs::home_dir().map(|h| h.join("Downloads"))).ok_or_else(|| anyhow!("no Downloads folder"))?;
    std::fs::create_dir_all(&dir)?;
    let path = download_verified(&dir, name, url, sums_url)?;
    quarantine(&path);
    Ok(path)
}

/// Downloads a release file into `dir` (never over an existing file) and checks it
/// against the release's checksum list; a file that does not match is deleted.
fn download_verified(dir: &Path, name: &str, url: &str, sums_url: &str) -> Result<PathBuf> {
    let out = curl().args(["--max-time", "15", sums_url]).output().context("cannot run curl")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let want = expected_sha256(&String::from_utf8_lossy(&out.stdout), name).ok_or_else(|| anyhow!("{name} is not in {SUMS}"))?;
    let path = unique_path(dir, name);
    let out = curl().args(["--max-time", "600", "--output"]).arg(&path).arg(url).output().context("cannot run curl")?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&path);
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    match sha256(&path) {
        Ok(have) if have == want => {}
        Ok(have) => {
            let _ = std::fs::remove_file(&path);
            bail!("{name} does not match {SUMS} (got {have}, expected {want})");
        }
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
    }
    Ok(path)
}

/// The hash listed for `name` in `sha256sum` output ("<hex>  <name>", or "*<name>" in
/// binary mode), lowercased.
fn expected_sha256(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        let ok = file.trim_start().trim_start_matches('*') == name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
        ok.then(|| hash.to_ascii_lowercase())
    })
}

/// SHA-256 of a file, lowercase hex, from the system's `shasum`.
fn sha256(path: &Path) -> Result<String> {
    let out = Command::new("shasum").args(["-a", "256"]).arg(path).output().context("cannot run shasum")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let hash = text.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    if hash.len() != 64 {
        bail!("unexpected shasum output: {}", text.trim());
    }
    Ok(hash)
}

/// Marks a download the way a browser does, so Gatekeeper checks the app in it before
/// it first opens (curl leaves no such mark).
fn quarantine(path: &Path) {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let value = format!("0081;{now:08x};flockswitch;");
    match Command::new("xattr").args(["-w", "com.apple.quarantine", &value]).arg(path).status() {
        Ok(s) if s.success() => {}
        Ok(s) => log::warn!("cannot mark {} as downloaded: xattr {s}", path.display()),
        Err(e) => log::warn!("cannot mark {} as downloaded: {e}", path.display()),
    }
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    if !path.exists() {
        return path;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (name.to_string(), String::new()),
    };
    (1..).map(|i| dir.join(format!("{stem}-{i}{ext}"))).find(|p| !p.exists()).unwrap_or(path)
}

/// How far an update being installed has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Downloading,
    Installing,
}

/// How installing an update ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// This version is now in place of the running app; the menu-bar app notices and
    /// restarts into it (its replacement watch, tray.rs).
    Installed(String),
    /// The app cannot be replaced where it is (why); the file was saved to Downloads and
    /// shown in Finder instead.
    Downloaded(PathBuf, String),
    /// The app cannot be replaced where it is (why), and a download was not asked for.
    NotInstallable(String),
    Failed(String),
}

/// Held while an update is installed, so the menu-bar app and the settings window never
/// install at once (the app holds 47851, the settings window 47852).
const INSTALL_LOCK_PORT: u16 = 47_853;
/// The folder made next to the app while installing: ".flockswitch-update-<pid>".
const STAGING_PREFIX: &str = ".flockswitch-update-";

/// Downloads `release`, checks it against SHA256SUMS.txt, and puts it in place of the
/// running app. Reports each step through `step`. When the app cannot be replaced where
/// it is and `or_download` is set, saves the file to Downloads and shows it instead.
/// Tries once; a later check tries again.
pub fn install(release: &Release, or_download: bool, step: impl Fn(Step)) -> Outcome {
    let version = &release.version;
    log::info!("update {version}: installing");
    let Some((name, url, sums)) = release.asset.clone() else {
        log::info!("update {version}: the release has no macOS file");
        if or_download {
            open_url(&release.page);
        }
        return Outcome::Failed(format!("the release {version} has no macOS file"));
    };
    let Ok(_lock) = std::net::TcpListener::bind(("127.0.0.1", INSTALL_LOCK_PORT)) else {
        log::info!("update {version}: another flockswitch is installing it already");
        return Outcome::Failed("another flockswitch window is installing the update already".into());
    };
    let (bundle, staging) = match prepare() {
        Ok(found) => found,
        Err(why) => {
            log::info!("update {version}: the app cannot be replaced where it is: {why}");
            if !or_download {
                return Outcome::NotInstallable(why);
            }
            step(Step::Downloading);
            log::info!("update {version}: downloading {name} to Downloads instead");
            return match download(&name, &url, &sums) {
                Ok(path) => {
                    log::info!("update {version}: downloaded {}", path.display());
                    reveal(&path);
                    Outcome::Downloaded(path, why)
                }
                Err(e) => {
                    log::warn!("update {version}: download failed: {e:#}");
                    Outcome::Failed(format!("{e:#}"))
                }
            };
        }
    };
    let result = put_in_place(&bundle, &staging, release, (&name, &url, &sums), &step);
    if let Err(e) = std::fs::remove_dir_all(&staging) {
        log::warn!("cannot delete {}: {e}", staging.display());
    }
    match result {
        Ok(()) => {
            log::info!("update {version}: installed at {}; restarting into it", bundle.display());
            Outcome::Installed(version.clone())
        }
        Err(e) => {
            log::warn!("update {version}: not installed: {e:#}");
            Outcome::Failed(format!("{e:#}"))
        }
    }
}

/// The running app's bundle and a new, empty folder beside it, or why the app cannot
/// be replaced where it is.
fn prepare() -> std::result::Result<(PathBuf, PathBuf), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find the running program: {e}"))?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let bundle = replaceable_bundle(&exe)?;
    let parent = bundle.parent().ok_or_else(|| format!("{} has no parent folder", bundle.display()))?;
    remove_leftovers(parent);
    let staging = parent.join(format!("{STAGING_PREFIX}{}", std::process::id()));
    std::fs::create_dir(&staging).map_err(|e| format!("cannot write to {}: {e}", parent.display()))?;
    log::info!("update: working in {}", staging.display());
    // The new folder is ours and on the folder's volume: the app must be both, or the
    // renames fail half way, or the old copy cannot be deleted.
    let check = || -> std::result::Result<(), String> {
        let ours = std::fs::metadata(&staging).map_err(|e| e.to_string())?;
        for p in [bundle.clone(), bundle.join("Contents"), bundle.join("Contents/MacOS")] {
            let m = std::fs::metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            if m.uid() != ours.uid() {
                return Err(format!("{} belongs to another user", p.display()));
            }
            if m.dev() != ours.dev() {
                return Err(format!("{} is on another volume", p.display()));
            }
        }
        Ok(())
    };
    if let Err(why) = check() {
        let _ = std::fs::remove_dir(&staging);
        return Err(why);
    }
    Ok((bundle, staging))
}

/// The bundle of `exe` (symlinks already resolved), if it is one this program may
/// replace by itself.
fn replaceable_bundle(exe: &Path) -> std::result::Result<PathBuf, String> {
    let bundle = bundle_of(exe).ok_or_else(|| format!("{} is not inside an .app bundle", exe.display()))?;
    if bundle.to_string_lossy().contains("/AppTranslocation/") {
        return Err("macOS runs this copy from a temporary read-only place (App Translocation); move flockswitch.app to Applications first".into());
    }
    // Homebrew's own copies: leave them to `brew`.
    if bundle.components().any(|c| c.as_os_str() == "Caskroom") {
        return Err(format!("{} is inside Homebrew's Caskroom", bundle.display()));
    }
    Ok(bundle)
}

/// Folders a stopped install left next to the app.
fn remove_leftovers(parent: &Path) {
    let Ok(entries) = std::fs::read_dir(parent) else { return };
    for e in entries.flatten() {
        if e.file_name().to_string_lossy().starts_with(STAGING_PREFIX) {
            log::info!("update: deleting the leftover {}", e.path().display());
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Download, unpack, check and swap; `staging` is next to `bundle`.
fn put_in_place(bundle: &Path, staging: &Path, release: &Release, (name, url, sums): (&str, &str, &str), step: &impl Fn(Step)) -> Result<()> {
    let version = &release.version;
    step(Step::Downloading);
    log::info!("update {version}: downloading {url}");
    let zip = download_verified(staging, name, url, sums)?;
    log::info!("update {version}: {name} matches {SUMS}");

    step(Step::Installing);
    let unpacked = staging.join("unpacked");
    // curl leaves no quarantine flag and --noqtn passes none on: flockswitch is not
    // notarized, so a flagged copy would be stopped at the restart (the Homebrew cask
    // clears the flag for the same reason).
    run(Command::new("/usr/bin/ditto").args(["-x", "-k", "--noqtn"]).arg(&zip).arg(&unpacked)).context("cannot unpack the update")?;
    let app = find_app(&unpacked)?;
    log::info!("update {version}: unpacked {}", app.display());
    check_bundle(&app, version)?;
    log::info!("update {version}: the new app says version {version}");
    if is_signed(bundle) {
        run(Command::new("/usr/bin/codesign").args(["--verify", "--deep", "--strict"]).arg(&app))
            .context("the new app's code signature is not intact")?;
        log::info!("update {version}: code signature intact");
    } else {
        log::info!("update {version}: this copy is not signed, so the signature is not checked");
    }
    let _ = Command::new("/usr/bin/xattr").args(["-dr", "com.apple.quarantine"]).arg(&app).status();

    log::info!("update {version}: swapping {}", bundle.display());
    swap_bundle(bundle, &app, &staging.join("previous"))
}

/// Runs a command, failing with what it printed.
fn run(cmd: &mut Command) -> Result<()> {
    let out = cmd.output().with_context(|| format!("cannot run {}", cmd.get_program().to_string_lossy()))?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

/// The app in an unpacked release (`flockswitch/flockswitch.app` in the zip).
fn find_app(unpacked: &Path) -> Result<PathBuf> {
    ["flockswitch/flockswitch.app", "flockswitch.app"]
        .iter()
        .map(|p| unpacked.join(p))
        .find(|p| p.is_dir())
        .ok_or_else(|| anyhow!("the download has no flockswitch.app"))
}

/// The unpacked app is the release it claims to be, with its program in it.
fn check_bundle(app: &Path, version: &str) -> Result<()> {
    let plist = app.join("Contents/Info.plist");
    let text = std::fs::read_to_string(&plist).with_context(|| format!("cannot read {}", plist.display()))?;
    match plist_version(&text) {
        Some(v) if v == version => {}
        Some(v) => bail!("the downloaded app is version {v}, not {version}"),
        None => bail!("the downloaded app has no version"),
    }
    if !app.join("Contents/MacOS/flockswitch").is_file() {
        bail!("the downloaded app has no program in it");
    }
    Ok(())
}

/// Whether a bundle has a code signature at all (release builds are signed ad hoc).
fn is_signed(bundle: &Path) -> bool {
    Command::new("/usr/bin/codesign").arg("--display").arg(bundle).output().is_ok_and(|o| o.status.success())
}

/// Puts `new` where `current` is: the old bundle moves to `aside`, the new one takes its
/// name, and the old one is deleted. If the new one cannot move in, the old one goes
/// back. All three are on one volume, so each move is a rename.
fn swap_bundle(current: &Path, new: &Path, aside: &Path) -> Result<()> {
    std::fs::rename(current, aside).with_context(|| format!("cannot move {} aside", current.display()))?;
    if let Err(e) = std::fs::rename(new, current) {
        if let Err(back) = std::fs::rename(aside, current) {
            log::error!("cannot put {} back from {}: {back}", current.display(), aside.display());
        }
        return Err(e).with_context(|| format!("cannot move the new version to {}", current.display()));
    }
    if let Err(e) = std::fs::remove_dir_all(aside) {
        log::warn!("cannot delete the old version at {}: {e}", aside.display());
    }
    Ok(())
}

/// The .app bundle the running program is in (symlinks resolved, such as Homebrew's
/// `flockswitch` command), if any.
pub fn running_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    bundle_of(&std::fs::canonicalize(&exe).unwrap_or(exe))
}

/// The .app folder an executable inside one belongs to.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.ends_with("Contents/MacOS") && bundle.extension().is_some_and(|e| e == "app")).then(|| bundle.to_path_buf())
}

/// The version an Info.plist states.
pub fn plist_version(text: &str) -> Option<&str> {
    let (_, rest) = text.split_once("<key>CFBundleShortVersionString</key>")?;
    let (_, rest) = rest.split_once("<string>")?;
    Some(rest.split_once("</string>")?.0.trim())
}

/// Shows a file selected in Finder / Explorer.
pub fn reveal(path: &Path) {
    if let Err(e) = Command::new("open").arg("-R").arg(path).spawn() {
        log::warn!("cannot show {}: {e}", path.display());
    }
}

/// Opens a web or mailto link with the system's default app.
pub fn open_url(url: &str) {
    if let Err(e) = Command::new("open").arg(url).spawn() {
        log::warn!("cannot open {url}: {e}");
    }
}

/// The start of a report: room to write, then what helps to find the cause. Only
/// what the user can see in the mail before sending it.
pub fn report_body(cfg: &crate::config::Config, devices: &[crate::shared::DeviceInfo], master: Option<&str>) -> String {
    use crate::config::{button_name, BUTTONS};
    let mut s = String::from("\n\n\n---- flockswitch ----\n");
    s += &format!("version: {}\n", env!("CARGO_PKG_VERSION"));
    s += &format!("system: {} {}\n", os_version(), std::env::consts::ARCH);
    s += &format!("master mouse: {} (setting: {})\n", master.unwrap_or("not here"), cfg.trigger_device);
    for cid in BUTTONS {
        let b = cfg.button(cid);
        if b.on {
            s += &format!("button {}: channels {:?}", button_name(cid), b.channels);
            for (name, m) in [("up", &b.moves.up), ("down", &b.moves.down), ("left", &b.moves.left), ("right", &b.moves.right)] {
                if m.on {
                    s += &format!(", {name} {:?}", m.channels);
                }
            }
            s += "\n";
        }
    }
    for d in devices {
        let here = if d.online { "" } else { ", on another computer" };
        s += &format!("device: {} [{}] {}{here}\n", d.name, d.key, d.kind);
    }
    s
}

pub fn os_version() -> String {
    #[cfg(target_os = "macos")]
    if let Ok(out) = Command::new("sw_vers").arg("-productVersion").output() {
        return format!("macOS {}", String::from_utf8_lossy(&out.stdout).trim());
    }
    std::env::consts::OS.to_string()
}

/// A mailto: link to the developer with the subject and a body filled in.
pub fn report_mailto(subject: &str, body: &str) -> String {
    format!("mailto:{REPORT_EMAIL}?subject={}&body={}", percent_encode(subject), percent_encode(body))
}

fn percent_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("0.3.2", "0.3.1"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.99.99"));
        assert!(!is_newer("0.3.1", "0.3.1"));
        assert!(!is_newer("0.3.1", "0.3.2"));
        assert!(!is_newer("0.3", "0.3.0"));
        assert!(is_newer("0.10.0", "0.9.1"));
        assert!(!is_newer("0.9.1", "0.10.0"));
    }

    #[test]
    fn assets() {
        let names = [
            "flockswitch-setup-0.3.2.exe",
            "flockswitch-windows-portable-0.3.2.zip",
            "flockswitch-macos-arm64-0.3.2.zip",
            "SHA256SUMS.txt",
        ];
        assert_eq!(pick_asset(&names), Some("flockswitch-macos-arm64-0.3.2.zip"));
    }

    #[test]
    fn checksums() {
        let h = "a".repeat(64);
        let sums = format!("{}  flockswitch-macos-arm64-0.4.4.zip\n{h} *other.zip\n", "B".repeat(64));
        assert_eq!(expected_sha256(&sums, "flockswitch-macos-arm64-0.4.4.zip"), Some("b".repeat(64)));
        assert_eq!(expected_sha256(&sums, "other.zip"), Some(h));
        assert_eq!(expected_sha256(&sums, "missing.zip"), None);
        assert_eq!(expected_sha256("abc  short.zip", "short.zip"), None);
    }

    #[test]
    fn mailto_is_encoded() {
        let link = report_mailto("flockswitch 리포트", "a & b\nc=d");
        assert!(link.starts_with("mailto:feedback.wwhale@gmail.com?subject=flockswitch%20%EB%A6%AC"));
        assert!(link.ends_with("&body=a%20%26%20b%0Ac%3Dd"));
    }

    /// A new, empty folder for one test.
    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flockswitch-test-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A bundle with an Info.plist of `version`, a program, and a file saying `mark`.
    fn dummy_app(app: &Path, version: &str, mark: &str) {
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        let plist = format!("<dict>\n  <key>CFBundleShortVersionString</key><string>{version}</string>\n</dict>");
        std::fs::write(app.join("Contents/Info.plist"), plist).unwrap();
        std::fs::write(app.join("Contents/MacOS/flockswitch"), mark).unwrap();
    }

    #[test]
    fn finds_the_app_bundle() {
        let exe = Path::new("/Applications/flockswitch.app/Contents/MacOS/flockswitch");
        assert_eq!(bundle_of(exe), Some(PathBuf::from("/Applications/flockswitch.app")));
        assert_eq!(bundle_of(Path::new("/opt/homebrew/bin/flockswitch")), None);
        assert_eq!(bundle_of(Path::new("/x/target/release/flockswitch")), None);
    }

    #[test]
    fn reads_the_plist_version() {
        let text = "<key>CFBundleName</key><string>a</string>\n  <key>CFBundleShortVersionString</key><string>0.7.1</string>";
        assert_eq!(plist_version(text), Some("0.7.1"));
        assert_eq!(plist_version("<key>CFBundleName</key><string>a</string>"), None);
    }

    #[test]
    fn which_bundles_may_be_replaced() {
        let ok = |p: &str| replaceable_bundle(Path::new(p));
        assert_eq!(ok("/Applications/flockswitch.app/Contents/MacOS/flockswitch"), Ok(PathBuf::from("/Applications/flockswitch.app")));
        assert_eq!(ok("/Users/a/Apps/flock 2.app/Contents/MacOS/flockswitch"), Ok(PathBuf::from("/Users/a/Apps/flock 2.app")));
        assert!(ok("/x/target/debug/flockswitch").is_err());
        assert!(ok("/private/var/folders/ab/T/AppTranslocation/1234/d/flockswitch.app/Contents/MacOS/flockswitch").is_err());
        assert!(ok("/opt/homebrew/Caskroom/flockswitch/0.9.1/flockswitch/flockswitch.app/Contents/MacOS/flockswitch").is_err());
    }

    #[test]
    fn checks_the_unpacked_app() {
        let dir = scratch("check");
        let app = dir.join("unpacked/flockswitch/flockswitch.app");
        dummy_app(&app, "0.10.0", "new");
        assert_eq!(find_app(&dir.join("unpacked")).unwrap(), app);
        assert!(check_bundle(&app, "0.10.0").is_ok());
        assert!(check_bundle(&app, "0.10.1").is_err());
        std::fs::remove_file(app.join("Contents/MacOS/flockswitch")).unwrap();
        assert!(check_bundle(&app, "0.10.0").is_err());
        assert!(find_app(&dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn swaps_the_bundle() {
        let dir = scratch("swap");
        let current = dir.join("flockswitch.app");
        let staging = dir.join(format!("{STAGING_PREFIX}1"));
        let new = staging.join("unpacked/flockswitch/flockswitch.app");
        dummy_app(&current, "0.9.1", "old");
        dummy_app(&new, "0.10.0", "new");
        swap_bundle(&current, &new, &staging.join("previous")).unwrap();
        assert_eq!(std::fs::read_to_string(current.join("Contents/MacOS/flockswitch")).unwrap(), "new");
        assert_eq!(plist_version(&std::fs::read_to_string(current.join("Contents/Info.plist")).unwrap()), Some("0.10.0"));
        assert!(!new.exists());
        assert!(!staging.join("previous").exists());
        // What a stopped install leaves behind goes; nothing else does.
        remove_leftovers(&dir);
        assert!(!staging.exists());
        assert!(current.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_swap_puts_the_old_bundle_back() {
        let dir = scratch("swap-back");
        let current = dir.join("flockswitch.app");
        dummy_app(&current, "0.9.1", "old");
        let missing = dir.join("nothing/flockswitch.app");
        assert!(swap_bundle(&current, &missing, &dir.join("previous")).is_err());
        assert_eq!(std::fs::read_to_string(current.join("Contents/MacOS/flockswitch")).unwrap(), "old");
        assert!(!dir.join("previous").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole install against the latest real release, into a signed stand-in app in
    /// a temporary folder (never the running app). Needs the network:
    /// `cargo test -- --ignored installs_the_latest_release`.
    #[test]
    #[ignore]
    fn installs_the_latest_release() {
        let release = latest().unwrap();
        let asset = release.asset.clone().unwrap();
        let dir = scratch("install");
        let bundle = dir.join("flockswitch.app");
        dummy_app(&bundle, "0.0.1", "");
        std::fs::copy(std::env::current_exe().unwrap(), bundle.join("Contents/MacOS/flockswitch")).unwrap();
        run(Command::new("/usr/bin/codesign").args(["--force", "--sign", "-"]).arg(&bundle)).unwrap();
        assert!(is_signed(&bundle));
        let staging = dir.join(format!("{STAGING_PREFIX}1"));
        std::fs::create_dir(&staging).unwrap();
        let steps = std::sync::Mutex::new(Vec::new());
        put_in_place(&bundle, &staging, &release, (&asset.0, &asset.1, &asset.2), &|s| steps.lock().unwrap().push(s)).unwrap();
        assert_eq!(*steps.lock().unwrap(), [Step::Downloading, Step::Installing]);
        check_bundle(&bundle, &release.version).unwrap();
        let attrs = Command::new("/usr/bin/xattr").arg(&bundle).output().unwrap();
        assert!(!String::from_utf8_lossy(&attrs.stdout).contains("com.apple.quarantine"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn busy_states() {
        let r = Release { version: "1.0.0".into(), page: String::new(), asset: None };
        assert!(UpdateState::Installing(r.clone()).busy());
        assert!(UpdateState::Installed("1.0.0".into()).busy());
        assert!(!UpdateState::Available(r.clone()).busy());
        assert!(!UpdateState::UpdateFailed(r, "x".into()).busy());
    }

    #[test]
    fn unique_names() {
        let dir = std::env::temp_dir().join(format!("flockswitch-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.zip"), b"").unwrap();
        assert_eq!(unique_path(&dir, "a.zip"), dir.join("a-1.zip"));
        assert_eq!(unique_path(&dir, "b.zip"), dir.join("b.zip"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
