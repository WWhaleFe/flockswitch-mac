# Changelog

## 0.4.1 — 2026-10-31

- Keyboard shortcuts: one per channel (Settings → Switching → Shortcuts, off until turned on; ⌃⌥1 / ⌃⌥2 / ⌃⌥3 until changed). Pressed on any keyboard, they send the group and the master mouse to that channel. Only devices on this Mac can be moved. Registered with macOS, so no extra permission is needed; while you set a new one the shortcuts pause, so pressing it records it instead of switching
- Command line: `set hotkeys true|false`, `set hotkey.<1-3> <keys|none>`

## 0.4.0 — 2026-10-31

- flockswitch for macOS now has its own repository (WWhaleFe/flockswitch-mac); the Windows version moved to WWhaleFe/flockswitch-windows. Features and settings are the same as 0.3.5, and existing settings are kept
- Updates are looked up in this repository's releases
- Error messages that need attention appear as an alert instead of only on the command line
