# Changelog

## 0.7.0 — 2026-11-09

- Settings window: its size stays within fixed limits (540×680 to start, 480×580 to 680×820), it no longer maximizes, and it keeps its size when you change tabs (a long page scrolls inside). The General tab gains Show log and Reset settings
- Haptics window: **Haptics Settings…** on the General tab opens a modal window with on/off, strength and the waveforms for switching, arrival and waiting at an edge, closed with Apply (stay open), OK (save and close) or Close (revert). A waveform you apply is played once
- Screen-edge countdown: while the cursor waits, a yellow effect runs down near it and the switch happens as it vanishes. Choose ring, semicircle, bar, pulse wave, neon laser, particle vortex, cyber scan or liquid, with direction, shrink and preview. Moving away calls it off at once
- Option to switch at a screen edge **only while Control is held** (`set edge.control true`)
- Rising haptics: while waiting at an edge the pulses get quicker and stronger, with one more as it switches
- Find the cursor: a pulse ring, radar or spotlight around the cursor, from a shortcut, when the mouse arrives from another computer, or when it is shaken (Settings → General)
- Follow-ups to the Bluetooth and receiver work of 0.6.1: a device without switching is no longer read again forever, a Bluetooth device that answers slowly stays listed, Logitech devices that do not speak HID++ are left alone, and waiting for Bluetooth reports no longer spins
- Homebrew upgrades: a copy started at login is now quit too, the new version is started afterwards, and "Open Anyway" is no longer needed each time. To upgrade: `brew update && brew upgrade --cask flockswitch`

## 0.6.1 — 2026-11-08

- Bluetooth & direct device recognition on macOS: added a CoreBluetooth BLE GATT HID++ backend (`00010000-0000-1000-8000-011f2000046d`) and broadened `0xFF43` / `0xFF00` HID++ usage matching so Bolt-generation and Bluetooth-connected Logitech mice and keyboards (including MX Master 4 and MX Mechanical) are recognized reliably even when macOS does not route HID++ through `IOHIDDevice`
- Receiver & Logi Options+ coexistence: increased HID++ feature/control query timeouts with automatic retry, deduplicated stale multi-slot receiver pairings for the same device, and sped up re-probing of sleeping devices on startup

## 0.6.0 — 2026-11-08

- Haptics on a mouse with a motor (MX Master 4, HID++ feature 0x19B0): a pulse as the mouse is switched away and one when it comes back here, and short pulses while the cursor waits at a screen edge to switch (Settings → General → **Haptics**; both can be turned off). The strength follows the haptics setting in Logi Options+. A mouse without a motor is simply quiet; `list` shows the waveforms a mouse offers, and `set haptics.switch_waveform`, `haptics.arrive_waveform`, `haptics.edge_waveform` choose them (0-14, 27)
- Haptics: the strength can be chosen (Settings → General → Haptics: Options+'s own setting, or 25/50/75/100 %). A chosen strength goes to the mouse at once, with a pulse to judge it by, and is applied again whenever the mouse is here, since the mouse forgets it when it loses power; `set haptics.intensity <0-100>`

## 0.5.0 — 2026-11-08

- Switching at the screen edge, as Logi Flow does but without a network: rest the cursor against an outer edge of the screen for a moment and the group and the master mouse go to the channel set for that side (Settings → Switching → **Edges**: each side's channel, the time at the edge from 0.3 to 2 s, and whether the cursor is put at that edge when the mouse comes back). Each computer watches only its own screen; the corners do not count, nor does an edge that continues onto another display.
- Settings: the channel rows of the buttons, the gesture directions and the screen edges now start with a gray **Off** segment, which replaces the on/off switch above each of them. Gray, not the accent colour, so a chosen Off reads as switched off
- Command line: `set edges true|false`, `set edge.left|right|top|bottom <1-3|none>`, `set edge.dwell <ms>`, `set edge.cursor true|false`

## 0.4.4 — 2026-11-06

- Updates: a downloaded update is checked against the SHA-256 checksums published with the release (SHA256SUMS.txt) and deleted if it does not match. For a release without checksums, the release page opens instead
- Updates: the check and the download go over https only, redirects included
- Updates: the downloaded zip is marked as coming from the internet, as a browser download is, so macOS checks the app before it first opens

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
