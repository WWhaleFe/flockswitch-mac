//! Saving the settings to a file and loading them again, to move a setup to another
//! computer. The files live in Documents/flockswitch, where they are easy to find and
//! copy; they hold the settings file as it is (same TOML in every port: macOS, Windows,
//! Linux) after a comment saying where it came from. A file from another computer may
//! have settings this one does not know; those are left out, not refused.

use crate::config::Config;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const PREFIX: &str = "flockswitch-settings-";

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

/// Writes `cfg` to a new file in the folder and returns its path.
pub fn export(cfg: &Config) -> Result<PathBuf> {
    let dir = folder();
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let now = SystemTime::now();
    let host = host_name();
    let text = export_text(cfg, &host, now)?;
    let base = file_stem(&host, now);
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
