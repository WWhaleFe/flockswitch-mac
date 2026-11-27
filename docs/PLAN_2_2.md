# flockswitch 2.2 (mac/windows/android 2.2.0, linux 2.3.0)

User requests of 2026-11-27, decided with the user:

1. Layout grid 6 x 4 (was 5 x 3).
2. Multi-monitor: **one cell per monitor**. A computer with two monitors has two cells; other
   computers are placed next to a specific monitor, and the cursor crosses only at that
   monitor's edge, arriving on the matching monitor of the other computer.
3. Per paired computer: **pause** ("잠시 연결 안 함" / "Pause connection") without unpairing.
4. Keyboard compatibility guide on the Keyboard page (what works between which systems,
   what does not, and why), all ports.
5. Reconnecting to a Mac on the same network often fails: audit and fix.
6. Android: clicks behave like touch, not like a Bluetooth mouse. Decision: improve the
   touch emulation only (no Shizuku). Android: Korean/English switching does not work: the
   app composes Hangul itself.
7. Extra bug review of everything.
8. Also in this release: mac TCC permission cache fix (already in mac/src/net.rs, uncommitted),
   linux 2.2.0 GNOME helper is already released.

All four repos carry an identical copy of `netcore/` (mac, linux, windows, android). Change
it once (in mac/netcore), test, then copy byte-for-byte to the other three. Never edit only
one copy.

## A. netcore

### A1. Grid 6 x 4
- `layout.rs`: `COLS = 6`, `ROWS = 4`, `MASTER_CELL = (2, 1)` (unchanged). Saved 5 x 3 layouts
  stay valid as they are (every old cell fits). Update tests and doc comments ("5x3").
- Every place that assumes 5 or 3 (UI code in each port too) must use the constants.

### A2. One cell per monitor
- `Cell` gains `#[serde(default)] pub display: u8` (monitor index, 0 = first). Old configs and
  old peers (no field) mean display 0, so a single-monitor computer is unchanged.
- Monitor index = position in the computer's display list sorted by (x, then y) of the
  top-left corner, so it is stable across runs while the arrangement stays the same.
  Add `geometry::sorted_displays(&[Rect]) -> Vec<Rect>` and use it everywhere an index is
  meant.
- Each computer tells the others its monitors: new message
  `Msg::Displays { rects: Vec<Rect> }` sent after Hello and whenever `platform.displays()`
  changes (poll it at most every 2 s). Feature string `"displays"` in `proto::FEATURES`;
  never send it to a peer that did not advertise the feature. The master keeps each peer's
  rects and puts them in `PeerStatus` (`displays: Vec<Rect>`, serde default) for the UI;
  `MeStatus` gets `displays` too.
- Validity: `valid_cells` keys a cell by (id, display); one cell per (id, display) and per
  position. `with_master` makes sure every monitor of the master has a cell.
- Auto-placement (master, in `my_cells` or a helper in layout.rs, pure and tested): for each
  computer whose monitor count (known from Displays, or 1) exceeds its cells, add the missing
  monitor cells next to its existing cells following the real arrangement (a monitor whose
  rect is to the right of monitor k goes to the right of k's cell, below goes below, etc.),
  first free cell nearest to that spot. Cells for monitor indices that no longer exist are
  ignored (kept in config, not used, not shown).
- `layout::neighbour` / `side_of` work on (id, display) cells: add
  `neighbour_of(cells, id, display, side) -> Option<(&str, u8)>`; a neighbour cell that is
  the same computer (another of its own monitors) is not a crossing (the OS handles it).
  Keep the old functions for display 0 where they are still used.
- Crossing out: when the cursor is at an outer edge, find which monitor it is on and the
  position along **that monitor's** edge: `geometry::edge_at_display(displays, x, y, margin)
  -> Option<(u8, Side)>` and `geometry::display_edge_ratio(display_rect, side, x, y)`. For
  platforms that only report `take_edge_hit() -> (side, pos over the whole edge)` (Wayland
  barriers), convert with `geometry::display_for_edge_pos(displays, side, pos) -> Option<(u8,
  f64)>` using the same exposed edge segments `edge_ratio` uses. Then the neighbour is
  `neighbour_of(cells, me, display, side)`.
- `Msg::Enter { side, pos }` gains `#[serde(default)] display: Option<u8>` = the target
  computer's monitor index; `pos` is then relative to that monitor's edge. The receiver places
  the cursor two points inside that monitor's opposite edge
  (`geometry::display_edge_position`). Without `display` (old peer) keep today's
  whole-edge behaviour. `Msg::Leave` likewise if it carries a position.
- Coming back: a Controlled computer leaving at an edge uses the same rules from its side.
- Arming edges (`arm_edges`) stays per side: arm a side when any monitor has a neighbour on
  that side; when a hit lands on a monitor without a neighbour there, hand it back at once
  (same as today for an absent computer).

### A3. Pause a computer
- `PeerConfig` gains `#[serde(default)] pub paused: bool`. A paused computer: no dialing,
  any open link to it is closed with Bye "paused", incoming links from it are refused after
  Hello (send Bye "paused"), it is never a neighbour (crossing skips it), it stays paired
  (fingerprint kept) and keeps its cells. Clearing the flag reconnects at once.
- `PeerStatus` gains `paused: bool` for the UI. A sub cannot pause the master's members for
  the whole group; pausing is local to the computer that sets it.
- Request API: add a `Request` (or config field write, whichever the ports use for other per
  peer switches like `can_send`) so each UI can toggle it.

### A4. Reconnecting (Mac)
Audit `service.rs` dialing and retry: discovery address choice (prefer IPv4 over IPv6
link-local; a link-local address needs the right scope id; try every advertised address, not
only the first), stale saved addresses after DHCP changes, the retry back-off after many
failures (must not grow so long that "same network, it never comes back" is what the user
sees; cap it at a few seconds while the peer is discovered), duplicate links when both sides
dial at once, a half-open TCP link after sleep (keepalive / ping timeout), and the listener
after the Mac wakes or the network changes (re-bind if needed). Fix what is wrong, add tests
for the pure parts, and log one INFO line per dial attempt result and per link close reason
so the next report has evidence.

### A5. Tests
All new pure functions get unit tests in netcore. `cargo test` in mac/netcore must pass.

## B. Ports (after A is copied in)

Each port: version bump with its own script (`scripts/bump-version.sh`), CHANGELOG ko/en,
README where layout/pause/keyboard is described, Korean and English texts, never the em dash.

### B1. Layout page (mac, windows, linux: egui `settings_ui/connect.rs`; android: Kotlin)
- 6 x 4 grid.
- One tile per monitor: label "<name> · 1", "<name> · 2" (or a small monitor number badge);
  tiles of the same computer share its colour/style. A computer with one monitor looks like
  today.
- Moving: click a tile, then an empty cell (as today), per monitor tile.
- Android stays one tile (one display).

### B2. Pause toggle
- In each paired computer's row: a switch or button "잠시 연결 안 함" / "Pause connection"
  next to "해제"/"Unpair"; paused rows say "일시 중지됨" / "Paused" and are dimmed; tiles of a
  paused computer are dimmed on the map.

### B3. Keyboard compatibility guide
- Keyboard page: a short always-visible note plus an (i) or "자세히" with a table: modifier
  keys, Korean/English switching, Hanja, Caps Lock, function/media keys, screen capture,
  between each pair (mac/win/linux/android), what works, what is mapped, what cannot work and
  why (for example Android receives text through the accessibility service and switches
  Korean/English inside flockswitch). Ask the code for the truth (keymap.rs, key rules), do not
  promise what is not implemented.

### B4. Android only
- Touch emulation like a mouse, within AccessibilityService limits: a click is a short tap at
  the pointer with no drift; press-and-move is a drag (one continuous gesture, stroke
  continuation via `GestureDescription.StrokeDescription.continueStroke` on API 26+); right
  button = Back by default (option: long press); middle button = Home or nothing (option);
  wheel = smooth scroll gestures at the pointer, proportional to wheel clicks, no jumps;
  double click = two taps within the system double-tap timeout; long press threshold not
  triggered by a normal click. The pointer overlay must not swallow taps.
- Hangul: when the paired computer sends the Korean/English key (the receiver-side key the
  rules map to; check keymap and key rules for which HID usage arrives, e.g. LANG1 0x90 or
  the Hangul key), toggle a Korean mode inside flockswitch; in Korean mode letters typed are
  composed with a 2-set (dubeolsik) automaton and sent to the focused field through the
  service's input connection with setComposingText / commitText (Android 13+) or the existing
  12L-and-older path; Backspace edits the composition; any non-letter commits. Pure Kotlin
  automaton with unit tests (syllable composition, double consonant finals, splitting a final
  into the next syllable, compound vowels, backspace).
- Show the current mode (한/A) in the notification or the overlay corner.

## C. Review and release
- Cross-review netcore changes (another model) before copying.
- Each port: local checks (cargo test/clippy, windows `cargo check --target
  x86_64-pc-windows-gnu`, linux check script + host tests, android `./gradlew
  testDebugUnitTest assembleRelease`), then PR, CI green, merge, tag, release, tap bump.
- Windows Store: new msix submission after 2.1.1 certification finishes.
