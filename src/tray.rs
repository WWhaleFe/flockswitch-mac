//! Menu-bar icon with the settings menu.
//! The engine runs on its own thread; the menu edits the shared config and the
//! engine picks the change up on its next loop.

use crate::engine::Engine;
use crate::i18n::{texts, Texts};
use crate::icon;
use crate::shared::{Shared, Status};
use anyhow::Result;
use auto_launch::{AutoLaunch, AutoLaunchBuilder, MacOSLaunchMode};
use std::collections::HashMap;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

#[derive(Clone)]
enum Action {
    Enabled,
    Settings,
    CheckUpdate,
    InstallUpdate,
    ShowDownload,
    ReleasePage,
    Report,
    Quit,
}

struct Ui {
    shared: Arc<Shared>,
    tray: Option<TrayIcon>,
    actions: HashMap<MenuId, Action>,
    shown: (Status, u64),
    uninstall_requested: bool,
    shortcuts: Option<crate::hotkey::Shortcuts>,
}

pub fn run(shared: Arc<Shared>) -> Result<()> {
    // Ctrl+C, logout or shutdown: let the loop below hand the button back and exit.
    let s = shared.clone();
    ctrlc::set_handler(move || s.stop.store(true, std::sync::atomic::Ordering::SeqCst))?;
    let engine = spawn_engine(shared.clone());
    spawn_update_check(shared.clone());
    spawn_replacement_watch(shared.clone());
    crate::edge::spawn(shared.clone());

    #[allow(unused_mut)]
    let mut event_loop = EventLoopBuilder::new().build();
    {
        // Menu-bar only: no Dock icon, no app switcher entry.
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
    }

    let mut ui = Ui {
        shared: shared.clone(),
        tray: None,
        actions: HashMap::new(),
        shown: (Status::default(), u64::MAX),
        uninstall_requested: false,
        shortcuts: None,
    };
    let mut engine = Some(engine);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(500));

        // The tray icon must be created once the event loop is running (macOS).
        if let Event::NewEvents(StartCause::Init) = event {
            crate::overlay::set_ready();
            match ui.create_tray() {
                Ok(()) => {}
                Err(e) => {
                    log::error!("cannot create the tray icon: {e:#}");
                    *control_flow = ControlFlow::Exit;
                }
            }
            match crate::hotkey::Shortcuts::new(shared.clone()) {
                Ok(s) => ui.shortcuts = Some(s),
                Err(e) => log::warn!("keyboard shortcuts unavailable: {e:#}"),
            }
        }
        // Follow the shortcut settings (and let go while the settings window records one).
        if let Some(s) = ui.shortcuts.as_mut() {
            if let Some(failed) = s.sync(&shared.config()) {
                shared.set_status(|st| st.hotkeys_failed = failed);
            }
        }

        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if let Some(action) = ui.actions.get(&ev.id).cloned() {
                if ui.handle(action) {
                    shared.stop.store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
        }
        // Uninstall asked for from the settings window; quit from `flockswitch quit`
        // or a newer copy taking over.
        let request = crate::shared::uninstall_request_path();
        if request.exists() {
            let _ = std::fs::remove_file(&request);
            ui.uninstall_requested = true;
            shared.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let request = crate::shared::quit_request_path();
        if request.exists() {
            let _ = std::fs::remove_file(&request);
            shared.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let destroyed = matches!(event, Event::LoopDestroyed);
        if shared.stopped() || destroyed {
            shared.stop.store(true, std::sync::atomic::Ordering::SeqCst);
            if let Some(h) = engine.take() {
                join_briefly(h); // lets the engine hand the button back first
            }
            if ui.uninstall_requested {
                crate::uninstall();
            }
            ui.tray = None;
            *control_flow = ControlFlow::Exit;
            return;
        }

        // Redraw when the engine found/lost devices or switched.
        let now = (shared.status(), shared.version());
        if ui.tray.is_some() && now != ui.shown {
            ui.refresh();
        }
    });
}

/// Waits for the engine to finish, but not forever: it may be in the middle of a slow
/// device request, and the UI must not hang at quit or logout.
fn join_briefly(h: JoinHandle<()>) {
    // Long enough for one slow device request plus handing the button back.
    let deadline = Instant::now() + Duration::from_secs(6);
    while !h.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    if h.is_finished() {
        let _ = h.join();
    }
}

fn spawn_engine(shared: Arc<Shared>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        while !shared.stopped() {
            // A panic must not leave the app looking alive with a dead engine: catch it,
            // let Engine's Drop hand the button back, and start again.
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Engine::new(shared.clone()).and_then(|mut e| e.run())
            }));
            let failure = match run {
                Ok(Ok(())) => break,
                Ok(Err(e)) => format!("{e:#}"),
                Err(_) => "internal error (panic)".to_string(),
            };
            log::error!("engine stopped: {failure}; restarting in 5 s");
            for _ in 0..50 {
                if shared.stopped() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    })
}

pub fn autostart() -> Option<AutoLaunch> {
    let exe = std::env::current_exe().ok()?;
    AutoLaunchBuilder::new()
        .set_app_name("flockswitch")
        .set_app_path(&exe.to_string_lossy())
        .set_macos_launch_mode(MacOSLaunchMode::LaunchAgent)
        .build()
        .ok()
}

/// Template image: macOS tints it for light/dark menu bars; fainter when paused.
fn tray_icon(enabled: bool) -> Result<Icon> {
    let alpha = if enabled { 255 } else { 110 };
    Ok(Icon::from_rgba(icon::arrows(36, [0, 0, 0, alpha], None), 36, 36)?)
}

impl Ui {
    fn create_tray(&mut self) -> Result<()> {
        let menu = self.build_menu();
        let cfg = self.shared.config();
        let builder = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("flockswitch");
        let builder = builder.with_icon_templated(tray_icon(cfg.enabled)?);
        let tray = builder.build()?;
        self.tray = Some(tray);
        self.shown = (self.shared.status(), self.shared.version());
        Ok(())
    }

    fn refresh(&mut self) {
        let menu = self.build_menu();
        let enabled = self.shared.config().enabled;
        if let Some(tray) = &self.tray {
            tray.set_menu(Some(Box::new(menu)));
            if let Ok(icon) = tray_icon(enabled) {
                let _ = tray.set_icon_templated(Some(icon));
            }
        }
        self.shown = (self.shared.status(), self.shared.version());
    }

    fn check(&mut self, text: impl AsRef<str>, checked: bool, enabled: bool, action: Action) -> CheckMenuItem {
        let item = CheckMenuItem::new(text, enabled, checked, None);
        self.actions.insert(item.id().clone(), action);
        item
    }

    fn item(&mut self, text: impl AsRef<str>, action: Action) -> MenuItem {
        let item = MenuItem::new(text, true, None);
        self.actions.insert(item.id().clone(), action);
        item
    }

    fn build_menu(&mut self) -> Menu {
        self.actions.clear();
        let t: &Texts = texts();
        let cfg = self.shared.config();
        let st = self.shared.status();
        let menu = Menu::new();

        let status = if !cfg.enabled {
            format!("○ {}", t.paused)
        } else if let Some(trigger) = &st.trigger {
            format!("● {} — {trigger}", t.running)
        } else {
            format!("◌ {}", t.searching)
        };
        let _ = menu.append(&MenuItem::new(status, false, None));
        if let Some(last) = &st.last_switch {
            let _ = menu.append(&MenuItem::new(format!("{}: {} {last}", t.last_switch, t.channel), false, None));
        }
        let _ = menu.append(&PredefinedMenuItem::separator());

        let enabled = self.check(t.enabled, cfg.enabled, true, Action::Enabled);
        let _ = menu.append(&enabled);
        // Everything else lives in the settings window, which stays open while you
        // change things (a native menu closes after every click).
        let settings = self.item(t.settings, Action::Settings);
        let _ = menu.append(&settings);
        let report = self.item(t.menu_report, Action::Report);
        let _ = menu.append(&report);
        let _ = menu.append(&PredefinedMenuItem::separator());

        // Version and updates.
        use crate::update::UpdateState as U;
        let _ = menu.append(&MenuItem::new(format!("flockswitch {}", env!("CARGO_PKG_VERSION")), false, None));
        let state_line = match &st.update {
            U::Idle => None,
            U::Checking => Some((t.update_checking.to_string(), false)),
            U::UpToDate => Some((format!("✓ {}", t.update_up_to_date), false)),
            U::Available(r) | U::Downloading(r) | U::Installing(r) => {
                Some((format!("● {} {}", t.update_available, r.version), false))
            }
            U::UpdateFailed(r, _) => Some((format!("● {} {} — {}", t.update_available, r.version, t.update_install_failed), false)),
            U::Downloaded(_) => Some((format!("✓ {}", t.update_downloaded), false)),
            U::Installed(_) => Some((format!("✓ {}", t.update_installed), false)),
            U::Failed => Some((t.menu_check_failed.to_string(), true)),
        };
        if let Some((text, retry)) = state_line {
            let item = if retry { self.item(text, Action::CheckUpdate) } else { MenuItem::new(text, false, None) };
            let _ = menu.append(&item);
        }
        let checking = matches!(st.update, U::Checking) || st.update.busy();
        let check = MenuItem::new(t.menu_check_update, !checking, None);
        self.actions.insert(check.id().clone(), Action::CheckUpdate);
        let _ = menu.append(&check);
        match &st.update {
            U::Available(_) | U::UpdateFailed(..) => {
                let _ = menu.append(&self.item(t.menu_install, Action::InstallUpdate));
            }
            U::Downloading(_) => {
                let _ = menu.append(&MenuItem::new(t.update_downloading, false, None));
            }
            U::Installing(_) => {
                let _ = menu.append(&MenuItem::new(t.update_installing, false, None));
            }
            U::Downloaded(_) => {
                let _ = menu.append(&self.item(t.menu_show_download, Action::ShowDownload));
            }
            _ => {}
        }
        let _ = menu.append(&self.item(t.menu_release_page, Action::ReleasePage));
        let _ = menu.append(&PredefinedMenuItem::separator());
        let quit = self.item(t.quit, Action::Quit);
        let _ = menu.append(&quit);
        menu
    }

    /// Applies a menu action. Returns true when the app should quit.
    fn handle(&mut self, action: Action) -> bool {
        match action {
            Action::Quit => return true,
            Action::Enabled => {
                if let Err(e) = self.shared.update_config(|c| c.enabled = !c.enabled) {
                    log::info!("setting not changed: {e:#}");
                }
            }
            Action::Settings => open_settings_window(),
            // Asked for: only look. A new version is then offered as "Update now";
            // automatic install is for the periodic check.
            Action::CheckUpdate => {
                let shared = self.shared.clone();
                std::thread::spawn(move || {
                    crate::update::check_into(&shared);
                });
            }
            // Asked for: an app that cannot replace itself gets the file in Downloads.
            Action::InstallUpdate => match self.shared.status().update {
                crate::update::UpdateState::Available(r) | crate::update::UpdateState::UpdateFailed(r, _) => {
                    crate::update::install_into(self.shared.clone(), r, true)
                }
                _ => {}
            },
            Action::ShowDownload => {
                if let crate::update::UpdateState::Downloaded(path) = self.shared.status().update {
                    crate::update::reveal(&path);
                }
            }
            Action::ReleasePage => {
                let page = match self.shared.status().update {
                    crate::update::UpdateState::Available(r) | crate::update::UpdateState::UpdateFailed(r, _) => r.page,
                    _ => crate::update::RELEASES_PAGE.to_string(),
                };
                crate::update::open_url(&page);
            }
            Action::Report => {
                let st = self.shared.status();
                let body = crate::update::report_body(&self.shared.config(), &st.devices, st.trigger.as_deref());
                crate::update::open_url(&crate::update::report_mailto(texts().report_subject, &body));
            }
        }
        self.refresh();
        false
    }
}

/// The periodic check: looks for a new version and, with automatic install on, installs
/// it (once: a failed install waits for the next check). An app that cannot replace
/// itself where it is just shows the new version, without opening Downloads unasked.
fn check_and_install(shared: &Shared) {
    if let crate::update::UpdateState::Available(r) = crate::update::check_into(shared) {
        if shared.config().auto_update {
            crate::update::install_with_status(shared, r, false);
        }
    }
}

/// Looks for a new version shortly after start and then once a day, while the
/// setting is on. The result shows in the menu.
fn spawn_update_check(shared: Arc<Shared>) {
    std::thread::spawn(move || {
        let mut wait = Duration::from_secs(20);
        loop {
            let until = Instant::now() + wait;
            while Instant::now() < until {
                if shared.stopped() {
                    return;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
            wait = crate::update::CHECK_INTERVAL;
            // Leave a download or install in progress or done alone.
            if shared.config().check_updates && !shared.status().update.busy() {
                check_and_install(&shared);
            }
        }
    });
}

/// How often to look whether a new version has replaced this app.
const REPLACEMENT_CHECK: Duration = Duration::from_secs(5);
/// Homebrew clears the new app's quarantine flag a moment after moving it into place.
const REPLACEMENT_SETTLE: Duration = Duration::from_secs(5);

/// `brew upgrade` swaps the app while this copy keeps running, and cannot start the new
/// one itself (Homebrew runs a cask's scripts sandboxed, where `open` fails). So the
/// running copy starts it; the new copy then asks this one to quit and takes over.
/// An update installed by flockswitch itself (update::install, from this process or
/// the settings window) swaps the same bundle and is picked up the same way.
fn spawn_replacement_watch(shared: Arc<Shared>) {
    let Some(bundle) = crate::update::running_bundle() else { return };
    std::thread::spawn(move || {
        while !shared.stopped() {
            std::thread::sleep(REPLACEMENT_CHECK);
            // Unreadable while the app is being swapped: look again.
            let Ok(text) = std::fs::read_to_string(bundle.join("Contents/Info.plist")) else { continue };
            if crate::update::plist_version(&text).is_none_or(|v| v == env!("CARGO_PKG_VERSION")) {
                continue;
            }
            log::info!("a new version has replaced this app; starting it");
            std::thread::sleep(REPLACEMENT_SETTLE);
            if let Err(e) = std::process::Command::new("/usr/bin/open").args(["-g", "-n"]).arg(&bundle).status() {
                log::warn!("cannot start the new version: {e}");
            }
            return;
        }
    });
}

/// Starts the settings window as a separate process of this same program.
pub fn open_settings_window() {
    match std::env::current_exe() {
        Ok(exe) => {
            if let Err(e) = std::process::Command::new(exe).arg("settings").spawn() {
                log::warn!("cannot open the settings window: {e}");
            }
        }
        Err(e) => log::warn!("cannot open the settings window: {e}"),
    }
}
