# flockswitch for macOS

**English** · [한국어](README.ko.md)

Press one mouse button and move your Logitech mouse **and** keyboard to another computer together.

flockswitch turns buttons of an MX Master mouse — the wheel mode-shift button, the gesture button and/or the wheel click (pressing the wheel) — into an Easy-Switch key. When you press one, every device in your group (keyboard, other mice) switches to the same Easy-Switch channel, then the mouse follows. It needs no network between the computers, so it works where Logitech Flow cannot (guest/public or locked-down networks).

This repository is the macOS version. The Windows version lives in [flockswitch-windows](https://github.com/WWhaleFe/flockswitch-windows).

> Not affiliated with Logitech. "Logitech", "MX Master", "Easy-Switch", "Bolt" and "Unifying" are trademarks of Logitech.

## Supported mice

flockswitch was built and tested with the **MX Master 3** and **MX Master 4**. Other Logitech mice with the same buttons (HID++ 2.0 *Change Host* and reprogrammable controls) may work but have **not been tested yet**; testing them is planned. Reports on other mice are welcome.

## How it works

flockswitch talks HID++ directly to the Logi Bolt / Unifying receiver (or a Bluetooth-connected device):

1. It diverts the buttons you turn on (controls `0x00C4` wheel mode-shift, `0x00C3` gesture, `0x0052` wheel click) so presses come to flockswitch.
2. On a press it sends *Change Host* (feature `0x1814`) to the group devices first, then to the mouse.
3. When the devices come back, it notices the link notification and diverts the button again.

Install flockswitch on **every** computer you switch between (flockswitch-windows on Windows PCs), and pair each device to the same channel number on each computer (e.g. channel 1 = Windows PC, channel 2 = Mac).

## Download

With [Homebrew](https://brew.sh):

```sh
brew install --cask wwhalefe/tap/flockswitch
```

This puts `flockswitch.app` in Applications and the `flockswitch` command on your PATH; `brew upgrade flockswitch` updates it. The first time, open it as in step 2 below.

Or get the latest build from [Releases](../../releases).

1. Unzip `flockswitch-macos-arm64-<version>.zip` and move `flockswitch.app` to Applications (Apple silicon).
2. The first time, **right-click → Open → Open** (or System Settings → Privacy & Security → *Open Anyway*).
3. An ⇄ icon appears in the menu bar; flockswitch has no Dock icon.

## Use

The ⇄ menu shows the status, turns switching on or off, checks for updates, opens a report to the developer, and opens **Settings…** — a window with three tabs where every change applies at once (choosing Settings… again brings it back to the front):

- **Switching**: on/off, and per button (wheel mode-shift, gesture, wheel click — several can be on): the channel it goes to — one channel, chosen on each computer since settings are kept per computer; this Mac's own channel is shown but cannot be chosen. The gesture button can also switch by holding it and moving the mouse up, down, left or right, each direction with its own channel (the cursor stays still while the button is held).
- **Shortcuts** (on the same tab, next to the buttons): keyboard shortcuts, one per channel (⌃⌥1 / ⌃⌥2 / ⌃⌥3 until you change them), pressed on any keyboard. They send the group and the master mouse to that channel. Only devices on this Mac can be moved, so a keyboard that does not move along (a wired one, say) can send them away but not bring them back — press the shortcut on the other computer for that. No extra permission is needed.
- **Edges** (same tab): rest the cursor against an outer edge of the screen for a moment (0.3 to 2 s, your choice) and the group and the master mouse go to the channel set for that side. No network: each computer watches its own screen, so turn it on on every computer and give each side the channel of the computer that stands there. When the mouse comes back, the cursor appears at the edge it left by. The corners do not count, and with several displays an edge that continues onto another display does not switch.
- **Devices**: the master mouse, and the devices that move along (Bolt, Unifying and Bluetooth devices can be mixed; devices on another computer are listed too)
- **General**: the note on Logi Options+, updates (check for a new version, download it), report to the developer, language (Korean or English; the first run follows the system), start at login, uninstall

flockswitch is meant to run alongside Logi Options+. Before setting it up, set each button you turn on to **No action** in Options+, so pressing it does not also run an Options+ action. When flockswitch quits or a button is turned off, the button is put back exactly as it was (including Options+'s own setting).

Command line: `/Applications/flockswitch.app/Contents/MacOS/flockswitch help` (`list`, `switch <N>`, `set <key> <value>`, `run`; e.g. `set hotkeys true`, `set hotkey.2 ctrl+alt+2`).

Settings live in `~/Library/Application Support/flockswitch/`.

If something does not work, use ⇄ menu → **Report to the developer…** (or Settings → General), or open a [problem report](../../issues/new/choose) with the output of `flockswitch list`.

## Uninstall

1. Open **Settings…** → **Uninstall flockswitch…** → **Uninstall**. This puts the mouse button back to normal, removes start-at-login and deletes the settings folder, then quits.
   (Command line alternative: quit flockswitch, then run `flockswitch uninstall`.)
2. Delete `flockswitch.app` (installed with Homebrew: `brew uninstall --cask flockswitch`).

flockswitch installs no drivers or services and changes no system settings.

## Privacy

flockswitch talks to the Logitech devices connected to this Mac. Its only internet connection is to GitHub, to see whether a newer version exists (at start and once a day; turn it off in Settings → General) and to download it when you press the button; nothing about you or your devices is sent. "Report to the developer" opens your own mail app, so you see everything before sending. (A port on `127.0.0.1` is held only to stop a second copy from running; nothing is sent or received on it.)

## Status

| Connection | Status |
|---|---|
| Logi Bolt receiver | tested |
| Unifying receiver | tested (MX Master 3) |
| Bluetooth | implemented, untested |

## Related projects

Others have tackled the same "don't flip the mouse over" problem; flockswitch builds on the same HID++ *Change Host* (`0x1814`) idea:

- Logi Options+ itself: Easy-Switch tiles in the **Actions Ring**, and "linked devices" so the keyboard's Easy-Switch keys also move the mouse.
- [Solaar](https://github.com/pwr-Solaar/Solaar) (Linux): divert a button and add a rule that runs `change-host`.
- [marcocosta97/mxswitch](https://github.com/marcocosta97/mxswitch), [btaner/logi-switch](https://github.com/btaner/logi-switch): command-line switching of one or all devices.
- [happy4ed/LogiFlow](https://github.com/happy4ed/LogiFlow): switches when the cursor hits a screen edge.
- [slaycio/logitech-easyswitch-sync](https://github.com/slaycio/logitech-easyswitch-sync), [omar16100/logi_mx_auto_switch](https://github.com/omar16100/logi_mx_auto_switch): the mouse follows the keyboard.

What flockswitch adds: it catches the mouse's own buttons directly (no Options+ binding, hotkey tool or launcher), moves a configurable group of Bolt / Unifying / Bluetooth devices together, coexists with Options+ and always gives the button back.

## Build

```sh
cargo build --release
packaging/macos/make-app.sh target/release/flockswitch /Applications   # flockswitch.app
```

## License

MIT
