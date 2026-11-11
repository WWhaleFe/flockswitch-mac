# Changelog

## 0.13.0 — 2026-11-12

- Tidy-up release: nothing changes in what flockswitch does, its settings or its command line, and settings from earlier versions load as before
- The menu-bar app no longer collects the menu-bar icon's hover and click notices, which nothing read, in a list that grew for as long as it ran
- A smaller program: `RUST_LOG` still filters the log by module and level, but no longer takes regular expressions
- README: Bluetooth is marked as tested

## 0.12.0 — 2026-11-10

- Edge countdown: each effect (ring, semicircle, bar, pulse, laser, vortex, scan, liquid) keeps its own colour and size, and its own direction (ring, semicircle, bar) and shrink-away (ring, semicircle). Settings → Switching → Edges shows and changes the chosen effect's; choosing "both" for the ring no longer changes the bar. Older settings give every effect the look they had. In the file each effect has its own table, `[edges.effects.ring]` and so on (the same in the Windows and Linux versions, so backups still move between them). Command line: `set edge.ring.color #FF3B30`, `set edge.bar.size 125`, `set edge.semicircle.direction both`, `set edge.ring.shrink true`; `set edge.anim_color …` and the other old names change the chosen effect (every effect with none or random chosen)
- **Random** draws each countdown as a different effect in that effect's own colour and size, and picks its direction and shrink-away at random where it has them, so it varies more
- The countdown's glow grows and shrinks with the chosen size like the rest of the effect
- Settings backup: shorter buttons — **Save**, **Load…**, **Manage…**, **Open folder** — and at most 10 saved files (every .toml file in `~/Documents/flockswitch` counts; the card shows how many, e.g. 3/10). With 10 there, Save writes nothing and says so, with a way to Manage. **Manage…** lists the files, newest first, to **Rename** (in place; up to 60 characters, none of / \ : * ? " < > |, never over another file) or **Delete** (after a confirmation, into the Trash)
- The settings window opens 970 high, so the General tab fits without scrolling with every find-the-cursor option on and a note under the shortcut or the update. When a page still does not fit, the window grows once to fit it (up to the display's height); it never shrinks a window you made bigger
- English: the bar's third direction is now "Both ends", which fits its button

## 0.11.1 — 2026-11-10

- More room between rows and the lines between them. Rows with a segment bar or buttons on the right (size, language, shortcut) sat almost on the line below, because the control pulled the row's height in; now the space above and below is even
- The settings window opens 960 tall to fit the extra room, so the General tab still fits without scrolling

## 0.11.0 — 2026-11-10

- Settings backup (Settings → General): **Save settings** writes the settings to `~/Documents/flockswitch/flockswitch-settings-<computer>-<YYYYMMDD-HHMMSS>.toml`, **Load settings…** lists the files there, newest first, and after a confirmation applies the chosen one, and **Open folder** shows the folder in Finder. The file is the same on macOS, Windows and Linux; settings another computer has and this one does not are skipped, and a file that is not a flockswitch settings file is refused. Channels differ from computer to computer, so check them after loading on another one
- Find the cursor by shaking: it no longer reacts so easily. The mouse must be shaken without a pause for the time chosen (**0.5 / 1 / 1.5 / 2 s**, `set locate.shake_time 1000`), and it can be made to count only while a key is held (**None / ⌃ Control / ⌥ Option / ⇧ Shift / ⌘ Command**, `set locate.shake_key ctrl`). Older settings get 1 s and no key
- Keyboard shortcuts: an ⓘ next to the shortcut settings explains which combinations work on macOS and which do not. Combinations macOS uses itself (⌘Q, ⌘W, ⌘H, ⌘M, ⌘⇧3/4/5, ⌃⌘Q and the like) are now refused
- Updates: **Check now** only looks, in the settings window and in the ⇄ menu; a new version is then offered as **Update now**, which installs it in the background whatever **Install updates automatically** says (that setting is for the daily check)
- The settings window and the haptics window keep their size in points when moved between displays with different scaling (macOS already did; this is a guard)
- Settings → General is tidier and fits the window: the update and report explanations moved into ⓘ tips, and **Show log** is gone (the log is still written)

## 0.10.0 — 2026-11-10

- Automatic updates: when flockswitch finds a new version it downloads it, checks it against the release's SHA256SUMS.txt, puts it in place of the app and restarts into it. **Install updates automatically** in Settings → General (on by default, `set auto_update true|false`); when off, **Update now** in the settings window or the ⇄ menu installs it. If the app cannot be replaced where it is (for example a folder it may not write to), Update now saves the new version to Downloads as before. This first works for the version after 0.10.0; 0.10.0 itself is installed the old way
- The haptics window's Feel button is now called Test

## 0.9.1 — 2026-11-10

- Haptics window: each situation on one line (name, pulse, Feel, switch), and 'use haptics / strength' set apart from the situations with more room between rows
- The language choice matches the other choice rows, and the settings window starts 870 high
- Fixed Feel with a random pulse playing nothing, and the spotlight and laser falling short of the screen ends at small sizes

## 0.9.0 — 2026-11-10

- Haptics settings tidied: the pulse as the mouse leaves and the one when it is back are switched on and off on their own, each with its pulse beside it. Waiting at an edge offers **Rising / Even**; the edge waveform, which went unused by default, is folded into that choice (`set haptics.arrive true|false`)
- **Feel**: the haptics window plays the chosen pulse on the mouse at the chosen strength, leaving the mouse's own strength as it was
- The window says plainly that only a Logitech mouse with a haptic motor (currently the MX Master 4) vibrates
- Random pulse: choose **Random** for the leaving or arriving pulse and each one is a different pulse the mouse offers
- Effect size: the edge countdown and the find-the-cursor highlight each have a size, 75 to 150% (`set edge.anim_size`, `set locate.size`; 50 to 200% on the command line)
- A more consistent settings window: buttons, drop-downs and segment bars share one height, buttons a minimum width, row labels one weight, and the other-colour button is round like the rest

## 0.8.0 — 2026-11-09

- Effect colours: the screen-edge countdown and the find-the-cursor highlight each have their own colour (seven ready-made, any other from the picker). `set edge.anim_color #0A84FF`, `set locate.color #34C759`
- Random effects: choose **Random** for the edge countdown or the highlight and each one shows a different effect
- A larger settings window (840×840 to start) with two columns on the Switching and General tabs; the Switching tab fits without scrolling

## 0.7.1 — 2026-11-09

- After a Homebrew upgrade the running app starts the new version and hands over, so there is no need to open it again (this takes effect from the upgrade after 0.7.1)

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
