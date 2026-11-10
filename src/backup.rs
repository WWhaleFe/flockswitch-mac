//! Saving the settings to a file and loading them again, to move a setup to another
//! computer. The files live in Documents/flockswitch, where they are easy to find and
//! copy; they hold the settings file as it is (same TOML in every port: macOS, Windows,
//! Linux) after a comment saying where it came from. A file from another computer may
//! have settings this one does not know; those are left out, not refused.
//!
//! At most `MAX_SAVED` files are kept (every .toml file in the folder counts); they can
//! be renamed, and deleted (moved to the Trash), from the settings window.

use crate::config::Config;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const PREFIX: &str = "flockswitch-settings-";

/// How many settings files the folder may hold before Save refuses to add one.
pub const MAX_SAVED: usize = 10;

/// A name (without ".toml") is at most this many characters.
pub const MAX_NAME: usize = 60;

/// Characters a name may not have: not allowed in file names on Windows, or a folder
/// separator.
pub const BAD_NAME_CHARS: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Save refused: the folder already has `MAX_SAVED` settings files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Full;

impl std::fmt::Display for Full {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "there are already {MAX_SAVED} saved settings files")
    }
}

impl std::error::Error for Full {}

/// Why a file could not be renamed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    /// One of `BAD_NAME_CHARS`, or a control character.
    BadChar(char),
    /// Another file already has that name; it is never overwritten.
    Exists,
    /// The system refused, and why.
    Failed(String),
}

/// How a file went away.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Removed {
    /// Into the Trash, where it can be put back from.
    Trashed,
    /// Deleted for good: this volume has no Trash.
    Deleted,
}

/// Documents/flockswitch.
pub fn folder() -> PathBuf {
    dirs::document_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join("Documents")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("flockswitch")
}

/// A settings file found in the folder.
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub path: PathBuf,
    /// The file name without ".toml".
    pub name: String,
    pub modified: Option<SystemTime>,
}

/// Writes `cfg` to a new file in the folder and returns its path. Refuses (with `Full`)
/// when the folder already has `MAX_SAVED` settings files.
pub fn export(cfg: &Config) -> Result<PathBuf> {
    export_in(&folder(), cfg, &host_name(), SystemTime::now())
}

fn export_in(dir: &Path, cfg: &Config, host: &str, now: SystemTime) -> Result<PathBuf> {
    if list_in(dir).len() >= MAX_SAVED {
        return Err(Full.into());
    }
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let text = export_text(cfg, host, now)?;
    let base = file_stem(host, now);
    let path = (0..)
        .map(|i| dir.join(if i == 0 { format!("{base}.toml") } else { format!("{base}-{i}.toml") }))
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join(format!("{base}.toml")));
    std::fs::write(&path, text).with_context(|| format!("cannot write {}", path.display()))?;
    log::info!("settings saved to {}", path.display());
    Ok(path)
}

/// The file's content: a comment naming the app, system and version, then the settings.
pub fn export_text(cfg: &Config, host: &str, at: SystemTime) -> Result<String> {
    let body = toml::to_string_pretty(cfg)?;
    Ok(format!(
        "# flockswitch settings\n\
         # Saved by flockswitch {} on {} ({}) at {}.\n\
         # Load it in Settings > General > Settings backup, on macOS, Windows or Linux.\n\
         # Channels differ from computer to computer: check them after loading elsewhere.\n\n{body}",
        env!("CARGO_PKG_VERSION"),
        crate::update::os_version(),
        host,
        format_time(at, true),
    ))
}

/// "flockswitch-settings-<host>-<YYYYMMDD-HHMMSS>".
pub fn file_stem(host: &str, at: SystemTime) -> String {
    let t = local_time(at);
    format!("{PREFIX}{}-{:04}{:02}{:02}-{:02}{:02}{:02}", safe_name(host), t[0], t[1], t[2], t[3], t[4], t[5])
}

/// The .toml files in the folder, newest first.
pub fn list() -> Vec<Saved> {
    list_in(&folder())
}

fn list_in(dir: &Path) -> Vec<Saved> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<Saved> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let is_toml = path.extension().is_some_and(|x| x.eq_ignore_ascii_case("toml"));
            if !is_toml || !path.is_file() {
                return None;
            }
            let name = path.file_stem()?.to_string_lossy().into_owned();
            let modified = e.metadata().ok().and_then(|m| m.modified().ok());
            Some(Saved { path, name, modified })
        })
        .collect();
    found.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| b.name.cmp(&a.name)));
    found
}

/// How many settings files the folder has.
pub fn count() -> usize {
    list().len()
}

/// A name as it will be used for a file: without spaces at the ends or a ".toml" typed
/// after it, checked.
pub fn check_name(name: &str) -> Result<String, NameError> {
    let mut name = name.trim();
    let cut = name.len().saturating_sub(5);
    if name.get(cut..).is_some_and(|end| end.eq_ignore_ascii_case(".toml")) {
        name = name[..cut].trim_end();
    }
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if let Some(c) = name.chars().find(|c| BAD_NAME_CHARS.contains(c) || c.is_control()) {
        return Err(NameError::BadChar(c));
    }
    if name.chars().count() > MAX_NAME {
        return Err(NameError::TooLong);
    }
    if name == "." || name == ".." {
        return Err(NameError::BadChar('.'));
    }
    Ok(name.to_string())
}

/// Gives a saved file another name (still ending in .toml, in the same folder) and
/// returns its new path. Never replaces another file.
pub fn rename(path: &Path, new_name: &str) -> Result<PathBuf, NameError> {
    let name = check_name(new_name)?;
    let dir = path.parent().ok_or_else(|| NameError::Failed("no folder".into()))?;
    let target = dir.join(format!("{name}.toml"));
    if target == path {
        return Ok(target);
    }
    // On a case-insensitive disk "a.toml" exists when renaming "A.toml" to it: that is
    // the same file, getting another case.
    let same_file = path.file_name().zip(target.file_name()).is_some_and(|(a, b)| {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    });
    if !same_file && (target.exists() || target.symlink_metadata().is_ok()) {
        return Err(NameError::Exists);
    }
    std::fs::rename(path, &target).map_err(|e| NameError::Failed(e.to_string()))?;
    log::info!("settings file {} renamed to {}", path.display(), target.display());
    Ok(target)
}

/// Moves a saved file to the Trash, or deletes it where there is no Trash.
pub fn delete(path: &Path) -> Result<Removed> {
    remove(path, true)
}

fn remove(path: &Path, use_trash: bool) -> Result<Removed> {
    let is_toml = path.extension().is_some_and(|x| x.eq_ignore_ascii_case("toml"));
    if !is_toml || !path.is_file() {
        bail!("{} is not a settings file", path.display());
    }
    if use_trash && move_to_trash(path) {
        log::info!("settings file {} moved to the Trash", path.display());
        return Ok(Removed::Trashed);
    }
    std::fs::remove_file(path).with_context(|| format!("cannot delete {}", path.display()))?;
    log::info!("settings file {} deleted", path.display());
    Ok(Removed::Deleted)
}

/// NSFileManager's trashItemAtURL (files_macos.m).
fn move_to_trash(path: &Path) -> bool {
    extern "C" {
        fn fs_move_to_trash(path: *const std::ffi::c_char) -> bool;
    }
    let Some(c) = path.to_str().and_then(|p| std::ffi::CString::new(p).ok()) else { return false };
    // Safety: a valid C string for the length of the call; nothing is kept.
    unsafe { fs_move_to_trash(c.as_ptr()) }
}

/// Reads a saved file into settings that can be applied as they are.
pub fn read(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    from_text(&text)
}

/// Settings from a saved file's text, checked as the settings file itself is. Refuses
/// text that is not TOML or has none of the settings in it; settings it does not know
/// (another port's, a newer version's) are left out.
pub fn from_text(text: &str) -> Result<Config> {
    let table: toml::Table = text.parse().context("not a TOML file")?;
    let known = toml::Value::try_from(Config::default())?;
    if !table.keys().any(|k| known.get(k).is_some()) {
        bail!("no flockswitch settings in it");
    }
    let mut cfg = Config::parse(text)?;
    // A shortcut fine elsewhere (Win+Q) may be one macOS keeps (⌘Q): left out.
    for n in crate::hotkey::LOCATE..=3 {
        if crate::hotkey::reserved(cfg.shortcut(n)) {
            cfg.set_shortcut(n, "");
        }
    }
    cfg.validate()?;
    Ok(cfg)
}

/// Shows the folder in Finder, creating it first.
pub fn open_folder() {
    let dir = folder();
    let _ = std::fs::create_dir_all(&dir);
    crate::update::open_url(&dir.to_string_lossy());
}

/// This computer's name, without ".local".
fn host_name() -> String {
    extern "C" {
        fn gethostname(name: *mut std::ffi::c_char, len: usize) -> i32;
    }
    let mut buf = [0u8; 256];
    // Safety: the buffer is as long as said; the name ends in a 0 within it.
    let ok = unsafe { gethostname(buf.as_mut_ptr().cast(), buf.len() - 1) } == 0;
    let end = buf.iter().position(|&b| b == 0).unwrap_or(0);
    let name = if ok { String::from_utf8_lossy(&buf[..end]).into_owned() } else { String::new() };
    let name = name.trim().trim_end_matches(".local").to_string();
    if name.is_empty() { "mac".into() } else { name }
}

/// A name that is a fine file name on every system: letters, digits, '-' and '_'.
fn safe_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        let c = if c.is_alphanumeric() || c == '_' { c } else { '-' };
        if !(c == '-' && out.ends_with('-')) {
            out.push(c);
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "computer".into() } else { out }
}

/// "2026-11-10 14:03", or with seconds.
pub fn format_time(at: SystemTime, seconds: bool) -> String {
    let t = local_time(at);
    let mut s = format!("{:04}-{:02}-{:02} {:02}:{:02}", t[0], t[1], t[2], t[3], t[4]);
    if seconds {
        s += &format!(":{:02}", t[5]);
    }
    s
}

/// Year, month, day, hour, minute, second in this computer's time zone.
fn local_time(at: SystemTime) -> [i64; 6] {
    #[repr(C)]
    struct Tm {
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        year: i32,
        wday: i32,
        yday: i32,
        isdst: i32,
        gmtoff: std::ffi::c_long,
        zone: *const std::ffi::c_char,
    }
    extern "C" {
        fn localtime_r(time: *const i64, out: *mut Tm) -> *mut Tm;
    }
    let secs = at.duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let mut tm = Tm { sec: 0, min: 0, hour: 0, mday: 1, mon: 0, year: 70, wday: 0, yday: 0, isdst: 0, gmtoff: 0, zone: std::ptr::null() };
    // Safety: both pointers are valid for the call; nothing is kept.
    if unsafe { localtime_r(&secs, &mut tm) }.is_null() {
        return [1970, 1, 1, 0, 0, 0];
    }
    [tm.year as i64 + 1900, tm.mon as i64 + 1, tm.mday as i64, tm.hour as i64, tm.min as i64, tm.sec as i64]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ShakeKey, BUTTON_GESTURE};

    #[test]
    fn export_and_import_round_trip() {
        let mut c = Config::default();
        c.set("gesture.on", "true").unwrap();
        c.set("gesture.channels", "2").unwrap();
        c.set("edges", "true").unwrap();
        c.set("edge.left", "3").unwrap();
        c.set("locate", "true").unwrap();
        c.set("locate.shake", "true").unwrap();
        c.set("locate.shake_key", "ctrl").unwrap();
        c.set("locate.shake_time", "1500").unwrap();
        c.set("haptics.intensity", "75").unwrap();
        c.set("language", "en").unwrap();
        let text = export_text(&c, "Test Mac", SystemTime::now()).unwrap();
        assert!(text.starts_with("# flockswitch settings\n# Saved by flockswitch "), "{text}");
        assert!(text.contains("Test Mac"));
        let back = from_text(&text).unwrap();
        assert_eq!(back.buttons, c.buttons);
        assert_eq!(back.edges, c.edges);
        assert_eq!(back.locate, c.locate);
        assert_eq!(back.haptics, c.haptics);
        assert_eq!(back.language, "en");
        assert!(back.button(BUTTON_GESTURE).on);
        assert_eq!(back.locate.shake_key, ShakeKey::Ctrl);
        // Exported again, the same file.
        assert_eq!(toml::to_string_pretty(&back).unwrap(), toml::to_string_pretty(&c).unwrap());
    }

    #[test]
    fn garbage_is_refused() {
        for bad in ["", "hello world", "{\"enabled\": true}", "[[[", "colour = 3\nshape = \"round\"\n", "\u{0}\u{1}binary"] {
            assert!(from_text(bad).is_err(), "{bad:?}");
        }
        // Known settings with impossible values.
        assert!(from_text("enabled = \"very\"").is_err());
        assert!(from_text("[edges]\ndwell_ms = 1\n").is_err());
        assert!(from_text("[buttons.wheel_mode]\non = true\nchannels = [7]\n").is_err());
    }

    #[test]
    fn settings_from_another_computer_load() {
        // Another port's file, with settings this one does not have.
        let text = "# flockswitch settings (Windows)\nenabled = true\nlanguage = \"ko\"\ntray_style = \"dark\"\n\
                    [edges]\non = true\nright = 2\nsome_windows_thing = 4\n\
                    [locate]\non = true\non_shake = true\nshake_key = \"win\"\nshake_time_ms = 500\n\
                    [hotkeys]\non = true\nkeys = [\"super+q\", \"ctrl+alt+2\", \"\"]\n";
        let c = from_text(text).unwrap();
        assert_eq!(c.edges.right, 2);
        assert_eq!((c.locate.shake_key, c.locate.shake_time_ms), (ShakeKey::Cmd, 500));
        assert_eq!(c.hotkeys.key(1), "", "⌘Q is left out on macOS");
        assert_eq!(c.hotkeys.key(2), "ctrl+alt+2");
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("flockswitch-backup-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn at_most_ten_saved() {
        let dir = scratch("limit");
        let c = Config::default();
        let start = SystemTime::now();
        for i in 0..MAX_SAVED {
            export_in(&dir, &c, "Mac", start + std::time::Duration::from_secs(i as u64)).unwrap();
        }
        assert_eq!(list_in(&dir).len(), MAX_SAVED);
        let refused = export_in(&dir, &c, "Mac", start + std::time::Duration::from_secs(99)).unwrap_err();
        assert!(refused.downcast_ref::<Full>().is_some(), "{refused:#}");
        assert_eq!(list_in(&dir).len(), MAX_SAVED, "nothing written");
        // Other files in the folder do not count; any .toml file does.
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        remove(&list_in(&dir)[0].path, false).unwrap();
        export_in(&dir, &c, "Mac", start + std::time::Duration::from_secs(100)).unwrap();
        assert!(export_in(&dir, &c, "Mac", start + std::time::Duration::from_secs(101)).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_checks_the_name() {
        assert_eq!(check_name("  home office  "), Ok("home office".into()));
        assert_eq!(check_name("work.toml"), Ok("work".into()), "a .toml typed after it is not doubled");
        assert_eq!(check_name("맥북 설정"), Ok("맥북 설정".into()));
        assert_eq!(check_name(""), Err(NameError::Empty));
        assert_eq!(check_name("   "), Err(NameError::Empty));
        assert_eq!(check_name(".toml"), Err(NameError::Empty));
        assert_eq!(check_name(&"가".repeat(60)), Ok("가".repeat(60)), "60 characters, not bytes");
        assert_eq!(check_name(&"a".repeat(61)), Err(NameError::TooLong));
        for c in BAD_NAME_CHARS {
            assert_eq!(check_name(&format!("a{c}b")), Err(NameError::BadChar(c)), "{c}");
        }
        assert_eq!(check_name("tab\there"), Err(NameError::BadChar('\t')));
        assert_eq!(check_name(".."), Err(NameError::BadChar('.')));

        let dir = scratch("rename");
        let a = dir.join("a.toml");
        std::fs::write(&a, "enabled = true\n").unwrap();
        std::fs::write(dir.join("taken.toml"), "enabled = false\n").unwrap();
        assert_eq!(rename(&a, "taken"), Err(NameError::Exists));
        assert_eq!(std::fs::read_to_string(dir.join("taken.toml")).unwrap(), "enabled = false\n", "never overwritten");
        assert_eq!(rename(&a, "bad/name"), Err(NameError::BadChar('/')));
        let moved = rename(&a, "office").unwrap();
        assert_eq!(moved, dir.join("office.toml"));
        assert!(!a.exists() && moved.is_file());
        // Another case of the same name is allowed.
        let upper = rename(&moved, "Office").unwrap();
        assert!(upper.is_file());
        let names: Vec<String> = list_in(&dir).into_iter().map(|s| s.name).collect();
        assert!(names.contains(&"Office".to_string()) && names.len() == 2, "{names:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_removes_only_settings_files() {
        let dir = scratch("delete");
        let file = dir.join("old.toml");
        std::fs::write(&file, "enabled = true\n").unwrap();
        std::fs::write(dir.join("keep.txt"), "x").unwrap();
        assert!(remove(&dir.join("keep.txt"), false).is_err());
        assert!(remove(&dir, false).is_err());
        assert!(remove(&dir.join("missing.toml"), false).is_err());
        assert_eq!(remove(&file, false).unwrap(), Removed::Deleted);
        assert!(!file.exists() && dir.join("keep.txt").exists());
        assert!(list_in(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "puts a file in this Mac's Trash"]
    fn delete_moves_to_the_trash() {
        let dir = scratch("trash");
        let file = dir.join("flockswitch-trash-test.toml");
        std::fs::write(&file, "enabled = true\n").unwrap();
        assert_eq!(delete(&file).unwrap(), Removed::Trashed);
        assert!(!file.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_and_listing() {
        assert_eq!(safe_name("Kim's MacBook Pro"), "Kim-s-MacBook-Pro");
        assert_eq!(safe_name("..//"), "computer");
        assert_eq!(safe_name("맥북"), "맥북");
        let stem = file_stem("Mac", SystemTime::now());
        assert!(stem.starts_with("flockswitch-settings-Mac-20"), "{stem}");
        let tail = stem.rsplit_once("Mac-").unwrap().1;
        assert_eq!(tail.len(), 15, "{tail}");
        assert_eq!(tail.as_bytes()[8], b'-');

        let dir = std::env::temp_dir().join(format!("flockswitch-backup-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, age) in [("older.toml", 100), ("newest.toml", 0), ("notes.txt", 0), ("middle.TOML", 50)] {
            let path = dir.join(name);
            std::fs::write(&path, "enabled = true\n").unwrap();
            let when = SystemTime::now() - std::time::Duration::from_secs(age);
            std::fs::File::options().write(true).open(&path).unwrap().set_modified(when).unwrap();
        }
        let names: Vec<String> = list_in(&dir).into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["newest", "middle", "older"]);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(list_in(&dir).is_empty());
    }
}
