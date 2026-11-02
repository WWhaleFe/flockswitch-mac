# Changelog

## 0.4.3 — 2026-11-02

- “Go to channel”: this Mac's own channel is locked as soon as flockswitch starts. In 0.4.2 it was only found after the first press or after the mouse reconnected, so until then every channel could be chosen
- “Go to channel”: this Mac's own channel is drawn as a grayed-out, sunken box, so it is clearly not available

## 0.4.2 — 2026-11-02

- Switching: each button (and each gesture direction) now goes to one channel, “Go to channel” (이동할 채널), instead of stepping through several. Settings are kept per computer, so choose on each Mac where it goes; this Mac's own channel is shown but cannot be chosen. Settings from earlier versions keep working and show where a press goes from this Mac

## 0.4.1 — 2026-10-31

- Keyboard shortcuts: one per channel (Settings → Switching → Shortcuts, off until turned on; ⌃⌥1 / ⌃⌥2 / ⌃⌥3 until changed). Pressed on any keyboard, they send the group and the master mouse to that channel. Only devices on this Mac can be moved. Registered with macOS, so no extra permission is needed; while you set a new one the shortcuts pause, so pressing it records it instead of switching
- Command line: `set hotkeys true|false`, `set hotkey.<1-3> <keys|none>`
- Settings → General and the README say which mice flockswitch was built and tested with (MX Master 3 and 4); other Logitech mice are not tested yet

## 0.4.0 — 2026-10-31

- flockswitch for macOS now has its own repository (WWhaleFe/flockswitch-mac); the Windows version moved to WWhaleFe/flockswitch-windows. Features and settings are the same as 0.3.5, and existing settings are kept
- Updates are looked up in this repository's releases
- Error messages that need attention appear as an alert instead of only on the command line
