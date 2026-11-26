//! macOS input adapter and network service lifetime.
use crate::{
    config::{Config, Edge},
    shared::Shared,
};
use flock_net::{keymap, InputEvent, MouseButton, NetConfig, Os, Platform, Rect, Role, Service, Side};
use serde_json::{json, Value};
use std::{
    ffi::c_void,
    path::PathBuf,
    ptr::null_mut,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

type Ref = *mut c_void;
#[repr(C)]
#[derive(Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        mask: u64,
        callback: unsafe extern "C" fn(Ref, u32, Ref, Ref) -> Ref,
        user: Ref,
    ) -> Ref;
    fn CGEventTapEnable(tap: Ref, enable: bool);
    fn CGEventGetIntegerValueField(event: Ref, field: u32) -> i64;
    fn CGEventGetDoubleValueField(event: Ref, field: u32) -> f64;
    fn CGEventGetFlags(event: Ref) -> u64;
    fn CGEventSourceFlagsState(state: i32) -> u64;
    fn CGEventSetFlags(event: Ref, flags: u64);
    fn CGEventSetIntegerValueField(event: Ref, field: u32, value: i64);
    fn CGEventCreateMouseEvent(source: Ref, kind: u32, point: Point, button: u32) -> Ref;
    fn CGEventCreateKeyboardEvent(source: Ref, key: u16, down: bool) -> Ref;
    fn CGEventCreateScrollWheelEvent2(
        source: Ref,
        units: u32,
        count: u32,
        y: i32,
        x: i32,
        z: i32,
    ) -> Ref;
    fn CGEventPost(tap: u32, event: Ref);
    fn CGAssociateMouseAndMouseCursorPosition(connected: u32) -> i32;
    fn CGDisplayHideCursor(display: u32) -> i32;
    fn CGDisplayShowCursor(display: u32) -> i32;
    fn CGMainDisplayID() -> u32;

    // Private but long-stable WindowServer calls: let a background (menu bar) app hide
    // the cursor. Without this CGDisplayHideCursor is ignored while another app is active.
    fn _CGSDefaultConnection() -> i32;
    fn CGSSetConnectionProperty(cid: i32, target: i32, key: Ref, value: Ref) -> i32;
}
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: Ref) -> bool;
    static kAXTrustedCheckOptionPrompt: Ref;
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(value: *const c_void);
    fn CFMachPortCreateRunLoopSource(allocator: Ref, port: Ref, order: isize) -> Ref;
    fn CFMachPortInvalidate(port: Ref);
    fn CFRunLoopGetCurrent() -> Ref;
    fn CFRunLoopAddSource(loop_: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRemoveSource(loop_: Ref, source: Ref, mode: Ref);
    fn CFRunLoopRunInMode(mode: Ref, seconds: f64, return_after_source: bool) -> i32;
    fn CFDictionaryCreate(
        allocator: Ref,
        keys: *const Ref,
        values: *const Ref,
        count: isize,
        key_callbacks: Ref,
        value_callbacks: Ref,
    ) -> Ref;
    fn CFStringCreateWithCString(allocator: Ref, text: *const std::ffi::c_char, encoding: u32) -> Ref;
    static kCFRunLoopDefaultMode: Ref;
    static kCFBooleanTrue: Ref;
}
#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDCheckAccess(request_type: u32) -> u32;
    fn IOHIDRequestAccess(request_type: u32) -> bool;
    fn NXOpenEventStatus() -> u32;
    fn NXClickTime(handle: u32) -> f64;
    fn NXCloseEventStatus(handle: u32);
}
/// Lets this menu bar app hide the cursor while another app is in front (once per process).
fn allow_background_cursor_hiding() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        let key = CFStringCreateWithCString(null_mut(), c"SetsCursorInBackground".as_ptr(), 0x0800_0100);
        if !key.is_null() {
            let cid = _CGSDefaultConnection();
            CGSSetConnectionProperty(cid, cid, key, kCFBooleanTrue);
            CFRelease(key);
        }
    });
}

fn double_click_interval() -> f64 {
    unsafe {
        let handle = NXOpenEventStatus();
        if handle == 0 {
            return 0.5;
        }
        let interval = NXClickTime(handle);
        NXCloseEventStatus(handle);
        if interval.is_finite() && interval > 0.0 {
            interval
        } else {
            0.5
        }
    }
}

pub fn config_dir() -> PathBuf {
    Config::base_dir()
}
pub fn permissions_ready() -> bool {
    let p = permissions();
    p.accessibility && p.input_monitoring
}
pub fn permissions() -> flock_net::Permissions {
    // kIOHIDRequestTypeListenEvent = 1, kIOHIDAccessTypeGranted = 0.
    unsafe { flock_net::Permissions { accessibility: AXIsProcessTrusted(), input_monitoring: IOHIDCheckAccess(1) == 0 } }
}
pub fn reported_permissions() -> Option<flock_net::Permissions> {
    let bytes = std::fs::read(flock_net::status_path(config_dir())).ok()?;
    let status: Value = serde_json::from_slice(&bytes).ok()?;
    serde_json::from_value(status.get("me")?.get("permissions")?.clone()).ok()
}
pub fn reset_permissions() -> anyhow::Result<()> {
    let mut errors = Vec::new();
    for service in ["Accessibility", "ListenEvent"] {
        match std::process::Command::new("tccutil").args(["reset", service, "io.github.wwhalefe.flockswitch2"]).output() {
            Ok(result) if result.status.success() => {},
            Ok(result) => errors.push(String::from_utf8_lossy(&result.stderr).into_owned()),
            Err(e) => errors.push(e.to_string()),
        }
    }
    // The tray, rather than the settings process, requests its own grants.
    std::fs::write(Config::path().with_file_name("permissions.request"), b"")?;
    if !errors.is_empty() { anyhow::bail!("{}", errors.join("\n")); }
    Ok(())
}
/// Clears this app's own stale grant for each permission that is missing, once per app
/// version. After an update with a changed signature, System Settings may still show
/// flockswitch2 as allowed while the grant no longer matches; turning that row on and off
/// does nothing until it is removed. Doing the removal here means the user only has to
/// switch the fresh row on. Working permissions are left alone.
fn clear_stale_grants() {
    let marker = Config::path().with_file_name(format!("permissions-cleared-{}", env!("CARGO_PKG_VERSION")));
    if marker.exists() { return; }
    let p = permissions();
    let mut services = Vec::new();
    if !p.accessibility { services.push("Accessibility"); }
    if !p.input_monitoring { services.push("ListenEvent"); }
    for service in services {
        match std::process::Command::new("tccutil").args(["reset", service, "io.github.wwhalefe.flockswitch2"]).output() {
            Ok(r) if r.status.success() => log::info!("permissions: cleared the old {service} entry"),
            Ok(r) => log::warn!("permissions: tccutil {service}: {}", String::from_utf8_lossy(&r.stderr).trim()),
            Err(e) => log::warn!("permissions: tccutil {service}: {e}"),
        }
    }
    let _ = std::fs::write(marker, b"");
}
fn request_permissions() {
    clear_stale_grants();
    unsafe {
        if !AXIsProcessTrusted() {
            let options = CFDictionaryCreate(
                null_mut(),
                &kAXTrustedCheckOptionPrompt,
                &kCFBooleanTrue,
                1,
                null_mut(),
                null_mut(),
            );
            if !options.is_null() {
                AXIsProcessTrustedWithOptions(options);
                CFRelease(options);
            }
        }
        if IOHIDCheckAccess(1) != 0 {
            IOHIDRequestAccess(1);
        }
    }
}
/// The running service and the config it runs with, for the edge watcher.
struct Live {
    service: Arc<Service>,
    config: NetConfig,
}
static LIVE: Mutex<Option<Live>> = Mutex::new(None);

fn side_of(edge: Edge) -> Side {
    match edge {
        Edge::Left => Side::Left,
        Edge::Right => Side::Right,
        Edge::Top => Side::Top,
        Edge::Bottom => Side::Bottom,
    }
}

fn displays() -> Vec<Rect> {
    crate::edge::platform::displays()
        .into_iter()
        .map(|r| Rect::new(r.x, r.y, r.w, r.h))
        .collect()
}

/// Whether the cursor can cross over `edge` now: this computer is in control and a
/// connected, paired computer sits on that side.
pub fn network_side_ready(edge: Edge) -> bool {
    let live = LIVE.lock().unwrap();
    let Some(live) = live.as_ref() else { return false };
    let side = side_of(edge);
    let status = live.service.status();
    status.running
        && status.state.mode == "local"
        && live.config.peers.iter().any(|p| {
            p.side == Some(side) && status.peers.iter().any(|s| s.id == p.id && s.connected)
        })
}

/// Moves control to the computer on `edge`, entering at the spot matching (x, y).
pub fn cross(edge: Edge, x: f64, y: f64) {
    let live = LIVE.lock().unwrap();
    let Some(live) = live.as_ref() else { return };
    if !live.config.on || !live.config.app_drives_edges { return; }
    let side = side_of(edge);
    let pos = flock_net::geometry::edge_ratio(&displays(), side, x, y);
    let _ = live.service.cross(side, pos);
}

/// The effect settings a master hands to its subs: the countdown look and the cursor
/// highlight.
const OWN_EDGE_KEYS: [&str; 5] = ["left", "right", "top", "bottom", "on"];
fn app_settings(config: &Config) -> Value {
    let mut edges = serde_json::to_value(&config.edges).unwrap_or_default();
    if let Some(map) = edges.as_object_mut() {
        for key in OWN_EDGE_KEYS {
            map.remove(key);
        }
    }
    json!({ "edges": edges, "haptics": config.haptics, "locate": config.locate,
        "enabled": config.enabled, "trigger_device": config.trigger_device, "follow": config.follow,
        "keep_wheel_mode": config.keep_wheel_mode, "buttons": config.buttons, "hotkeys": config.hotkeys })
}

/// Puts the master's effect settings over a sub's own, keeping its channels and `edges.on`.
fn apply_app_settings(config: &mut Config, app: &Value) {
    macro_rules! read { ($($field:ident),*) => { $(
        if let Some(value) = app.get(stringify!($field)).and_then(|v| serde_json::from_value(v.clone()).ok()) { config.$field = value; }
    )* }; }
    read!(enabled, trigger_device, follow, keep_wheel_mode, buttons, hotkeys);
    if let Some(Value::Object(master)) = app.get("edges") {
        let mut edges = serde_json::to_value(&config.edges).unwrap_or_default();
        if let Some(map) = edges.as_object_mut() {
            for (key, value) in master {
                if !OWN_EDGE_KEYS.contains(&key.as_str()) {
                    map.insert(key.clone(), value.clone());
                }
            }
        }
        if let Ok(edges) = serde_json::from_value(edges) {
            config.edges = edges;
        }
    }
    if let Some(haptics) = app.get("haptics").and_then(|v| serde_json::from_value(v.clone()).ok()) {
        config.haptics = haptics;
    }
    if let Some(locate) = app.get("locate").and_then(|v| serde_json::from_value(v.clone()).ok()) {
        config.locate = locate;
    }
}

/// What this computer is doing on the network, for the header and the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetState {
    Off,
    Idle,
    /// Controlling the computer of this name.
    Controlling(String),
    /// Being controlled by the computer of this name.
    Controlled(String),
}

impl NetState {
    /// From the settings and the service's status (None: no status yet).
    pub fn of(config: &Config, status: Option<&flock_net::Status>) -> NetState {
        let Some(status) = status.filter(|s| s.running && config.network.on && config.network.notice_accepted) else {
            return NetState::Off;
        };
        let name = || {
            let id = status.state.peer.as_deref().unwrap_or_default();
            config.network.peers.iter().find(|p| p.id == id).map_or_else(|| id.to_string(), |p| p.name.clone())
        };
        match status.state.mode.as_str() {
            "controlling" => NetState::Controlling(name()),
            "controlled" => NetState::Controlled(name()),
            _ => NetState::Idle,
        }
    }
}

impl NetState {
    /// The line the header and the tray menu show.
    pub fn text(&self, t: &crate::i18n::Texts) -> String {
        match self {
            NetState::Off => t.net_state_off.to_string(),
            NetState::Idle => t.net_state_idle.to_string(),
            NetState::Controlling(name) => format!("{}{name}{}", t.net_state_controlling_pre, t.net_state_controlling_post),
            NetState::Controlled(name) => format!("{}{name}{}", t.net_state_controlled_pre, t.net_state_controlled_post),
        }
    }
}

/// The running service's status, for the tray menu (which lives in the same process).
pub fn live_status() -> Option<flock_net::Status> {
    LIVE.lock().unwrap().as_ref().map(|live| live.service.status())
}

/// The network config the service runs with: sharing only after consent, the master's
/// effect settings attached, and the app driving the edges when there is a wait.
fn runtime_config(shared: &Shared) -> NetConfig {
    let full = shared.config();
    let mut config = full.network.clone();
    config.config_dir = config_dir();
    config.on &= config.notice_accepted;
    config.app_drives_edges = full.edges.on || config.cross_delay_ms > 0;
    if config.role == Role::Master {
        config.app_settings = app_settings(&full);
    }
    config
}

pub fn spawn(shared: Arc<Shared>) -> JoinHandle<()> {
    thread::Builder::new()
        .name("network".into())
        .spawn(move || {
            let platform = Arc::new(MacPlatform::default());
            let mut service: Option<Arc<Service>> = None;
            let mut mode = String::from("local");
            let mut previous: Option<NetConfig> = None;
            let mut prompted = false;
            let mut permission_poll = Instant::now() - Duration::from_secs(2);
            let mut was_ready = permissions_ready();
            let mut retry = Instant::now() - Duration::from_secs(5);
            while !shared.stopped() {
                shared.reload_if_edited();
                let version = shared.version();
                let config = runtime_config(&shared);
                platform.edges_allowed.store(shared.config().network_edges_allowed(), Ordering::Release);
                let request = Config::path().with_file_name("permissions.request");
                if request.exists() {
                    let _ = std::fs::remove_file(request);
                    request_permissions();
                }
                if config.on && permission_poll.elapsed() >= Duration::from_secs(2) {
                    permission_poll = Instant::now();
                    let ready = permissions_ready();
                    if ready && !was_ready {
                        // Recreate the capture owner after permissions change, without restarting the app.
                        LIVE.lock().unwrap().take();
                        if let Some(s) = service.take() { s.stop(); }
                        retry = Instant::now() - Duration::from_secs(5);
                    }
                    was_ready = ready;
                }
                if !config.on {
                    LIVE.lock().unwrap().take();
                    mode = "local".into();
                    if let Some(s) = service.take() {
                        s.stop();
                    }
                    prompted = false;
                } else {
                    if !prompted {
                        request_permissions();
                        prompted = true;
                    }
                    if service.is_none() && retry.elapsed() >= Duration::from_secs(5) {
                        retry = Instant::now();
                        let paired = shared.clone();
                        let unpaired = shared.clone();
                        let changed = shared.clone();
                        let master = shared.clone();
                        // No Logitech channels here: `switch_logitech` stays the no-op default.
                        let hooks = flock_net::Hooks {
                            switch_logitech: Box::new(|_| {}),
                            on_paired: Box::new(move |peer| {
                                if let Err(e) = paired.update_config(|c| {
                                    c.network.peers.retain(|p| p.id != peer.id);
                                    c.network.peers.push(peer);
                                }) {
                                    log::error!("save paired computer: {e:#}");
                                }
                            }),
                            on_unpaired: Box::new(move |id| {
                                if let Err(e) = unpaired
                                    .update_config(|c| c.network.peers.retain(|p| p.id != id))
                                {
                                    log::error!("save unpaired computer: {e:#}");
                                }
                            }),
                            on_peer_changed: Box::new(move |peer| {
                                if let Err(e) = changed.update_config(|c| {
                                    match c.network.peers.iter_mut().find(|p| p.id == peer.id) {
                                        Some(slot) => *slot = peer,
                                        None => c.network.peers.push(peer),
                                    }
                                }) {
                                    log::error!("save computer: {e:#}");
                                }
                            }),
                            on_master_settings: Box::new(move |settings| {
                                if let Err(e) = master.update_config(|c| {
                                    settings.apply_to(&mut c.network);
                                    apply_app_settings(c, &settings.app);
                                }) {
                                    log::error!("save master settings: {e:#}");
                                }
                            }),
                            ..flock_net::Hooks::default()
                        };
                        match Service::start(config.clone(), platform.clone(), hooks) {
                            Ok(s) => {
                                let s = Arc::new(s);
                                *LIVE.lock().unwrap() =
                                    Some(Live { service: s.clone(), config: config.clone() });
                                service = Some(s);
                            }
                            Err(e) => {
                                log::error!("start network: {e:#}");
                                let status = flock_net::Status {
                                    running: false,
                                    me: flock_net::MeStatus {
                                        id: String::new(),
                                        name: config.name.clone(),
                                        port: config.port,
                                        error: Some(format!("{e:#}")),
                                    },
                                    discovered: vec![],
                                    peers: vec![],
                                    state: flock_net::StateStatus {
                                        mode: "local".into(),
                                        peer: None,
                                    },
                                    pairing: None,
                                    transfer: None,
                                    following: None,
                                };
                                let _ = flock_net::write_status_with_permissions(config_dir(), &status, Some(permissions()));
                            }
                        }
                    } else if previous.as_ref() != Some(&config) {
                        if let Some(s) = &service {
                            let _ = s.update_config(config.clone());
                            if let Some(live) = LIVE.lock().unwrap().as_mut() {
                                live.config = config.clone();
                            }
                        }
                    }
                }
                previous = Some(config);
                for _ in 0..5 {
                    if shared.stopped() || shared.version() != version {
                        break;
                    }
                    thread::sleep(Duration::from_millis(50));
                    if let Some(s) = &service {
                        let now = s.status().state.mode;
                        // The cursor arrived here, or came back here.
                        let arrived = (mode == "local" && now == "controlled")
                            || (mode == "controlling" && now == "local");
                        if arrived {
                            let locate = shared.config().locate;
                            if locate.on && locate.on_arrival {
                                crate::overlay::locate(&locate);
                            }
                        }
                        mode = now;
                    }
                }
            }
            LIVE.lock().unwrap().take();
            if let Some(s) = service {
                s.stop();
            }
            platform.stop_capture();
        })
        .expect("spawning network service")
}

// Device-specific modifier bits distinguish both sides of the keyboard.
const MODIFIERS: [(u16, u64, u64); 8] = [
    (0x3b, 1, 1 << 18),
    (0x38, 2, 1 << 17),
    (0x3a, 0x20, 1 << 19),
    (0x37, 8, 1 << 20),
    (0x3e, 0x2000, 1 << 18),
    (0x3c, 4, 1 << 17),
    (0x3d, 0x40, 1 << 19),
    (0x36, 0x10, 1 << 20),
];
fn modifier_changes(previous: u64, flags: u64, key: u16, held: &mut [bool; 8]) -> Vec<InputEvent> {
    let mut events = Vec::new();
    for (i, &(code, bit, group)) in MODIFIERS.iter().enumerate() {
        let down = if flags & group == 0 {
            false
        } else if (previous | flags) & bit != 0 {
            flags & bit != 0
        } else if code == key {
            !held[i]
        } else {
            held[i]
        };
        if down != held[i] {
            held[i] = down;
            if let Some(usage) = keymap::mac_to_hid(code) {
                events.push(InputEvent::Key { usage, down });
            }
        }
    }
    if (previous ^ flags) & (1 << 16) != 0 {
        events.push(InputEvent::Key {
            usage: 0x39,
            down: true,
        });
        events.push(InputEvent::Key {
            usage: 0x39,
            down: false,
        });
    }
    events
}
struct Capture {
    sink: Sender<InputEvent>,
    flags: u64,
    held: [bool; 8],
    tap: Ref,
}
unsafe extern "C" fn capture(_: Ref, kind: u32, event: Ref, user: Ref) -> Ref {
    let state = &mut *(user as *mut Capture);
    if kind == u32::MAX - 1 || kind == u32::MAX {
        CGEventTapEnable(state.tap, true);
        return event;
    }
    let field = |n| CGEventGetIntegerValueField(event, n);
    let ev = match kind {
        5 | 6 | 7 | 27 => Some(InputEvent::Motion {
            dx: CGEventGetDoubleValueField(event, 4),
            dy: CGEventGetDoubleValueField(event, 5),
        }),
        1 | 2 => Some(InputEvent::Button {
            button: MouseButton::Left,
            down: kind == 1,
        }),
        3 | 4 => Some(InputEvent::Button {
            button: MouseButton::Right,
            down: kind == 3,
        }),
        25 | 26 => match field(3) {
            2 => Some(MouseButton::Middle),
            3 => Some(MouseButton::Back),
            4 => Some(MouseButton::Forward),
            _ => None,
        }
        .map(|button| InputEvent::Button {
            button,
            down: kind == 25,
        }),
        10 | 11 => keymap::mac_to_hid(field(9) as u16).map(|usage| InputEvent::Key {
            usage,
            down: kind == 10,
        }),
        12 => {
            let flags = CGEventGetFlags(event);
            for ev in modifier_changes(state.flags, flags, field(9) as u16, &mut state.held) {
                let _ = state.sink.send(ev);
            }
            state.flags = flags;
            None
        }
        22 => {
            let (x, y) = if field(88) != 0 {
                (
                    CGEventGetDoubleValueField(event, 97) / 10.0,
                    CGEventGetDoubleValueField(event, 96) / 10.0,
                )
            } else {
                (field(12) as f64, field(11) as f64)
            };
            Some(InputEvent::Scroll { dx: x, dy: y })
        }
        _ => None,
    };
    if let Some(ev) = ev {
        let _ = state.sink.send(ev);
    }
    null_mut()
}
#[derive(Default)]
struct Injection {
    buttons: [bool; 5],
    modifiers: [bool; 8],
    caps: bool,
    scroll: (f64, f64),
    clicks: [Option<(Instant, f64, f64, i64)>; 5],
    click_counts: [i64; 5],
}
struct CaptureThread {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}
#[derive(Default)]
struct MacPlatform {
    edges_allowed: AtomicBool,
    capture: Mutex<Option<CaptureThread>>,
    injection: Mutex<Injection>,
}
impl Platform for MacPlatform {
    fn os(&self) -> Os {
        Os::Mac
    }
    fn displays(&self) -> Vec<Rect> {
        displays()
    }
    fn cursor(&self) -> Option<(f64, f64)> {
        if !self.edges_allowed.load(Ordering::Acquire) { return None; }
        crate::edge::platform::cursor()
    }
    fn warp(&self, x: f64, y: f64) {
        crate::edge::platform::place(x, y);
    }
    fn control_down(&self) -> bool {
        crate::edge::platform::control_down()
    }
    fn clipboard_text(&self) -> Option<String> {
        arboard::Clipboard::new().ok()?.get_text().ok()
    }
    fn set_clipboard_text(&self, text: &str) {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(text);
        }
    }
    fn clipboard_files(&self) -> Vec<PathBuf> {
        arboard::Clipboard::new()
            .ok()
            .and_then(|mut c| c.get().file_list().ok())
            .unwrap_or_default()
    }
    fn set_clipboard_files(&self, files: &[PathBuf]) {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set().file_list(files);
        }
    }
    fn clipboard_image_png(&self) -> Option<Vec<u8>> {
        let image = arboard::Clipboard::new().ok()?.get_image().ok()?;
        encode_png(image.width, image.height, &image.bytes)
    }
    fn set_clipboard_image_png(&self, data: &[u8]) {
        let Some((width, height, bytes)) = decode_png(data) else { return };
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_image(arboard::ImageData { width, height, bytes: bytes.into() });
        }
    }
    fn received_dir(&self) -> Option<PathBuf> {
        Some(dirs::download_dir()?.join("flockswitch2"))
    }
    fn permissions(&self) -> Option<flock_net::Permissions> { Some(permissions()) }
    fn ready(&self) -> Result<(), String> {
        if permissions_ready() {
            Ok(())
        } else {
            Err("Accessibility and Input Monitoring permission required".into())
        }
    }
    fn start_capture(&self, sink: Sender<InputEvent>) -> anyhow::Result<()> {
        anyhow::ensure!(self.edges_allowed.load(Ordering::Acquire), "Network edge crossing is paused");
        self.ready().map_err(anyhow::Error::msg)?;
        self.stop_capture();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let handle = thread::Builder::new()
            .name("network-input-tap".into())
            .spawn(move || unsafe {
                let mut state = Box::new(Capture {
                    sink,
                    flags: CGEventSourceFlagsState(0),
                    held: [false; 8],
                    tap: null_mut(),
                });
                let mask = [1, 2, 3, 4, 5, 6, 7, 10, 11, 12, 22, 25, 26, 27]
                    .iter()
                    .fold(0u64, |mask, n| mask | (1 << n));
                let tap =
                    CGEventTapCreate(0, 0, 0, mask, capture, (&mut *state as *mut Capture).cast());
                if tap.is_null() {
                    let _ = tx.send(false);
                    return;
                }
                state.tap = tap;
                let source = CFMachPortCreateRunLoopSource(null_mut(), tap, 0);
                if source.is_null() {
                    CFMachPortInvalidate(tap);
                    CFRelease(tap);
                    let _ = tx.send(false);
                    return;
                }
                CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopDefaultMode);
                CGEventTapEnable(tap, true);
                CGAssociateMouseAndMouseCursorPosition(0);
                allow_background_cursor_hiding();
                let display = CGMainDisplayID();
                CGDisplayHideCursor(display);
                for (i, &(code, bit, _)) in MODIFIERS.iter().enumerate() {
                    if state.flags & bit != 0 {
                        state.held[i] = true;
                        if let Some(usage) = keymap::mac_to_hid(code) {
                            let _ = state.sink.send(InputEvent::Key { usage, down: true });
                        }
                    }
                }
                let _ = tx.send(true);
                while !stopping.load(Ordering::Acquire) {
                    CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.05, false);
                }
                CGEventTapEnable(tap, false);
                CFMachPortInvalidate(tap);
                CFRunLoopRemoveSource(CFRunLoopGetCurrent(), source, kCFRunLoopDefaultMode);
                CGAssociateMouseAndMouseCursorPosition(1);
                CGDisplayShowCursor(display);
                CFRelease(source);
                CFRelease(tap);
            })?;
        if rx.recv().unwrap_or(false) {
            *self.capture.lock().unwrap() = Some(CaptureThread { stop, handle });
            Ok(())
        } else {
            let _ = handle.join();
            relaunch_after_tap_failure();
            anyhow::bail!("Cannot create input event tap")
        }
    }
    fn stop_capture(&self) {
        if let Some(capture) = self.capture.lock().unwrap().take() {
            capture.stop.store(true, Ordering::Release);
            let _ = capture.handle.join();
        }
    }
    fn inject(&self, ev: &InputEvent) {
        let mut s = self.injection.lock().unwrap();
        // Input injection continues even while Logitech owns the screen edges.
        let (x, y) = crate::edge::platform::cursor().unwrap_or((0.0, 0.0));
        let event = unsafe {
            match *ev {
                InputEvent::Motion { dx, dy } => {
                    let Some((x, y)) = flock_net::geometry::clamp(&self.displays(), x + dx, y + dy)
                    else {
                        return;
                    };
                    let (kind, button) = if s.buttons[0] {
                        (6, 0)
                    } else if s.buttons[1] {
                        (7, 1)
                    } else if let Some(i) = (2..5).find(|&i| s.buttons[i]) {
                        (27, i as u32)
                    } else {
                        (5, 0)
                    };
                    CGEventCreateMouseEvent(null_mut(), kind, Point { x, y }, button)
                }
                InputEvent::Button { button, down } => {
                    let i = match button {
                        MouseButton::Left => 0,
                        MouseButton::Right => 1,
                        MouseButton::Middle => 2,
                        MouseButton::Back => 3,
                        MouseButton::Forward => 4,
                    };
                    s.buttons[i] = down;
                    if down {
                        let count =
                            next_click(s.clicks[i], Instant::now(), x, y, double_click_interval());
                        s.clicks[i] = Some((Instant::now(), x, y, count));
                        s.click_counts[i] = count;
                    }
                    let kind = match (i, down) {
                        (0, true) => 1,
                        (0, false) => 2,
                        (1, true) => 3,
                        (1, false) => 4,
                        (_, true) => 25,
                        _ => 26,
                    };
                    let event = CGEventCreateMouseEvent(null_mut(), kind, Point { x, y }, i as u32);
                    if !event.is_null() {
                        CGEventSetIntegerValueField(event, 1, s.click_counts[i]);
                    }
                    event
                }
                InputEvent::Scroll { dx, dy } => {
                    let px = scroll_pixels(&mut s.scroll.0, dx);
                    let py = scroll_pixels(&mut s.scroll.1, dy);
                    if px == 0 && py == 0 {
                        return;
                    }
                    CGEventCreateScrollWheelEvent2(null_mut(), 0, 2, py, px, 0)
                }
                InputEvent::Key { usage, down } => {
                    let Some(key) = keymap::hid_to_mac(usage) else {
                        return;
                    };
                    if (0xe0..=0xe7).contains(&usage) {
                        s.modifiers[(usage - 0xe0) as usize] = down;
                    }
                    if usage == 0x39 && down {
                        s.caps = !s.caps;
                    }
                    CGEventCreateKeyboardEvent(null_mut(), key, down)
                }
            }
        };
        if !event.is_null() {
            unsafe {
                let flags = s
                    .modifiers
                    .iter()
                    .zip(MODIFIERS)
                    .filter(|(held, _)| **held)
                    .fold(
                        if s.caps { 1 << 16 } else { 0 },
                        |f, (_, (_, bit, group))| f | bit | group,
                    );
                CGEventSetFlags(event, flags);
                CGEventPost(0, event);
                CFRelease(event);
            }
        }
    }
}
impl Drop for MacPlatform {
    fn drop(&mut self) {
        self.stop_capture();
    }
}
/// RGBA pixels as a PNG file.
fn encode_png(width: usize, height: usize, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, u32::try_from(width).ok()?, u32::try_from(height).ok()?);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().ok()?.write_image_data(rgba).ok()?;
    Some(out)
}
/// A PNG file as width, height and RGBA pixels.
fn decode_png(data: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let (width, height) = (info.width as usize, info.height as usize);
    let rgba = match (info.color_type, info.bit_depth) {
        (png::ColorType::Rgba, png::BitDepth::Eight) => buf,
        (png::ColorType::Rgb, png::BitDepth::Eight) => {
            buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect()
        }
        (png::ColorType::GrayscaleAlpha, png::BitDepth::Eight) => {
            buf.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect()
        }
        (png::ColorType::Grayscale, png::BitDepth::Eight) => {
            buf.iter().flat_map(|&g| [g, g, g, 255]).collect()
        }
        _ => return None,
    };
    Some((width, height, rgba))
}
fn scroll_pixels(remainder: &mut f64, notches: f64) -> i32 {
    *remainder += notches * 10.0;
    // Avoid losing a pixel when fractional sums round just below an integer.
    let rounded = remainder.round();
    let pixels = if (*remainder - rounded).abs() < 1e-9 {
        rounded
    } else {
        remainder.trunc()
    } as i32;
    *remainder -= pixels as f64;
    pixels
}
fn next_click(
    last: Option<(Instant, f64, f64, i64)>,
    now: Instant,
    x: f64,
    y: f64,
    interval: f64,
) -> i64 {
    last.filter(|(time, px, py, _)| {
        now.duration_since(*time).as_secs_f64() <= interval && (x - px).hypot(y - py) <= 4.0
    })
    .map_or(1, |(_, _, _, count)| if count == 1 { 2 } else { 1 })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_modifier_sides_release_independently() {
        let mut held = [false; 8];
        let group = 1 << 17;
        assert_eq!(
            modifier_changes(0, group | 2, 0x38, &mut held),
            vec![InputEvent::Key {
                usage: 0xe1,
                down: true
            }]
        );
        assert_eq!(
            modifier_changes(group | 2, group | 6, 0x3c, &mut held),
            vec![InputEvent::Key {
                usage: 0xe5,
                down: true
            }]
        );
        assert_eq!(
            modifier_changes(group | 6, group | 4, 0x38, &mut held),
            vec![InputEvent::Key {
                usage: 0xe1,
                down: false
            }]
        );
        assert_eq!(
            modifier_changes(group | 4, 0, 0x3c, &mut held),
            vec![InputEvent::Key {
                usage: 0xe5,
                down: false
            }]
        );
    }
    #[test]
    fn caps_is_a_toggle_and_scroll_keeps_fractions() {
        assert_eq!(modifier_changes(0, 1 << 16, 0x39, &mut [false; 8]).len(), 2);
        let mut remainder = 0.0;
        assert_eq!(scroll_pixels(&mut remainder, 0.04), 0);
        assert_eq!(scroll_pixels(&mut remainder, 0.08), 1);
        assert!((remainder - 0.2).abs() < 0.0001);
        assert_eq!(scroll_pixels(&mut remainder, -0.12), -1);
    }
    #[test]
    fn double_click_requires_time_and_distance() {
        let now = Instant::now();
        let last = Some((now, 10.0, 10.0, 1));
        assert_eq!(
            next_click(last, now + Duration::from_millis(100), 12.0, 12.0, 0.5),
            2
        );
        assert_eq!(
            next_click(last, now + Duration::from_secs(1), 10.0, 10.0, 0.5),
            1
        );
        assert_eq!(next_click(last, now, 15.0, 10.0, 0.5), 1);
    }
    #[test]
    fn png_round_trip() {
        let pixels: Vec<u8> = (0..2 * 3 * 4).map(|i| i as u8).collect();
        let png = encode_png(2, 3, &pixels).unwrap();
        assert_eq!(decode_png(&png), Some((2, 3, pixels)));
        assert_eq!(decode_png(b"not a png"), None);
    }
    #[test]
    fn effect_settings_follow_the_master() {
        let mut master = Config::default();
        master.edges.anim_style = crate::config::EdgeAnimStyle::Bar;
        master.locate.on = true;
        let app = app_settings(&master);
        let mut sub = Config::default();
        apply_app_settings(&mut sub, &app);
        assert_eq!(sub.edges.anim_style, crate::config::EdgeAnimStyle::Bar);
        assert_eq!(sub.edges.effects, master.edges.effects);
        assert_eq!(sub.locate, master.locate);
        // Haptics follow the master, but local edge channels and ownership remain local.
        let mut old = app.clone();
        old["haptics"] = json!({ "intensity": 60 });
        old["edges"]["left"] = json!(2);
        let mut sub = Config::default();
        apply_app_settings(&mut sub, &old);
        assert_eq!(sub.edges, master.edges);
        assert_eq!(sub.haptics.intensity, 60);
        master.enabled = false;
        master.hotkeys.on = true;
        master.trigger_device = "B042".into();
        master.follow = vec!["MX Keys".into()];
        master.keep_wheel_mode = false;
        master.buttons.gesture.on = true;
        sub.edges.on = true; sub.edges.right = 3;
        apply_app_settings(&mut sub, &app_settings(&master));
        assert!(!sub.enabled && sub.hotkeys.on && !sub.keep_wheel_mode);
        assert_eq!(sub.buttons, master.buttons);
        assert_eq!(sub.follow, master.follow);
        assert_eq!(sub.trigger_device, master.trigger_device);
        assert!(sub.edges.on && sub.edges.right == 3);
    }
    #[test]
    fn network_state_text_source() {
        let mut config = Config::default();
        config.network.on = true;
        config.network.notice_accepted = true;
        config.network.peers.push(flock_net::PeerConfig { id: "p1".into(), name: "Laptop".into(), ..Default::default() });
        let status = |mode: &str, peer: Option<&str>| flock_net::Status {
            running: true,
            me: flock_net::MeStatus { id: String::new(), name: String::new(), port: 0, error: None },
            discovered: vec![],
            peers: vec![],
            state: flock_net::StateStatus { mode: mode.into(), peer: peer.map(Into::into) },
            pairing: None,
            transfer: None,
            following: None,
        };
        assert_eq!(NetState::of(&config, None), NetState::Off);
        assert_eq!(NetState::of(&config, Some(&status("local", None))), NetState::Idle);
        assert_eq!(NetState::of(&config, Some(&status("controlling", Some("p1")))), NetState::Controlling("Laptop".into()));
        assert_eq!(NetState::of(&config, Some(&status("controlled", Some("p1")))), NetState::Controlled("Laptop".into()));
        config.network.on = false;
        assert_eq!(NetState::of(&config, Some(&status("local", None))), NetState::Off);
    }
}

/// A timestamp claim prevents repeated tap failures from creating a restart loop.
fn claim_relaunch(path: &std::path::Path, now: u64) -> bool {
    use std::io::Write;
    let lock = path.with_extension("lock");
    let Ok(_claim) = std::fs::OpenOptions::new().write(true).create_new(true).open(&lock) else { return false };
    let previous = std::fs::read_to_string(path).ok().and_then(|s| s.trim().parse::<u64>().ok());
    let allowed = previous.is_none_or(|t| now.saturating_sub(t) >= 60);
    let saved = allowed && std::fs::File::create(path).and_then(|mut f| write!(f, "{now}")).is_ok();
    let _ = std::fs::remove_file(lock);
    saved
}
fn relaunch_after_tap_failure() {
    if !permissions_ready() { return; }
    let marker = Config::path().with_file_name("tap-relaunch.timestamp");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    if !claim_relaunch(&marker, now) { return; }
    if launch_tray().is_ok() { std::process::exit(0); }
}
pub fn launch_background() -> anyhow::Result<()> {
    if crate::main_instance_running() {
        crate::ask_running_copy(&crate::shared::quit_request_path())?;
    }
    launch_tray()
}
fn launch_tray() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    if let Some(bundle) = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")) {
        // Starting asynchronously lets this process release the single-instance lock first.
        std::process::Command::new("open").arg("-n").arg(bundle).args(["--args", "recover-tray"]).spawn()?;
    } else {
        std::process::Command::new(exe).arg("recover-tray").spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[test]
    fn recovery_claim_is_limited_to_once_per_minute() {
        let path = std::env::temp_dir().join(format!("flock-tap-{}-{}", std::process::id(), crate::config::random_below(1000000)));
        assert!(claim_relaunch(&path, 1000));
        assert!(!claim_relaunch(&path, 1001));
        assert!(!claim_relaunch(&path, 1059));
        assert!(!claim_relaunch(&path, 999));
        assert!(claim_relaunch(&path, 1060));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn logitech_owner_pauses_both_immediate_and_delayed_network_edges() {
        let platform = MacPlatform::default();
        assert!(platform.cursor().is_none(), "paused edges must not return across a network edge either");
        for delay in [0, 500] {
            let mut c = Config::default(); c.network.on = true; c.network.notice_accepted = true;
            c.network.cross_delay_ms = delay; c.edges.on = true;
            let shared = Shared::new(c);
            let runtime = runtime_config(&shared);
            assert!(runtime.on && runtime.app_drives_edges);
            assert!(!shared.config().network_edges_allowed());
        }
    }
}
