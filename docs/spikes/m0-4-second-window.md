# M0-4 — Second window, RenderLayers isolation, vsync

Spike crate: `spikes/m0_4_second_window`. Run from the `spikes/` directory (Cargo
discovers the shared-target-dir config by walking up from the working
directory, not from `--manifest-path`):

```
cargo run --release --manifest-path m0_4_second_window/Cargo.toml -- \
  [--editor-vsync on|off] [--output-vsync on|off] [--auto-close <frames>]
```

**This machine has only one physical monitor** (confirmed by the spike's own
monitor enumeration below), so the projector-specific checks (layer
isolation *by eye*, monitor selection onto a real second display, Esc-close
on a real fullscreen borderless window, and the 10-minute stability run) are
**entirely the user's half** -- see the checklist. Everything else (monitor
enumeration code path, the single-monitor fallback, RenderLayers wiring,
and the vsync-coupling measurement) was run and is recorded below.

## Monitor enumeration and the single-monitor fallback

```
$ m0-4-second-window.exe --auto-close 180
[m0-4] enumerating monitors:
[m0-4]   monitor 369v0: name="\\.\DISPLAY1" pos=(0, 0) size=3440x1440px refresh=144.0Hz scale=1.00
[m0-4] total monitors: 1
[m0-4] monitors detected: 1. only one monitor present: opening windowed-borderless
       at (80,80) 960x540 instead. Re-run with a projector/second display
       attached to exercise the real BorderlessFullscreen(MonitorSelection::Entity) path.
```

**Confirmed working exactly as designed**: with one monitor, the spike opens
the output window `WindowMode::Windowed` + `decorations: false` +
`position: WindowPosition::At(IVec2::new(80, 80))` + explicit `resolution`
-- the manual escape-hatch path spec §11.2 requires -- and logs clearly that
it did so instead of the real `BorderlessFullscreen(MonitorSelection::Entity(..))`
path. **This is also, incidentally, a live test of that escape-hatch path
itself** (Task 5 Step 4's second half: "confirm this also produces a correct
borderless fullscreen result"), since it's what actually ran here. The
window opened windowed-borderless at the requested position with no error.
Whether it visually *looks* like a clean borderless region (no unexpected
title bar sliver, correct size) is a **user checklist item** below, since
that's a screenshot judgment call.

## Layer isolation (code-level)

- Editor camera: `RenderLayers::from_layers(&[0, 1])`, `order: 0`.
- Output camera: `RenderLayers::layer(0)` only, `order: 10`, targets the
  output window via `RenderTarget::Window(WindowRef::Entity(output_window))`.
- Content (rotating cube): `RenderLayers::layer(0)`.
- Gizmo (wireframe cube via `Gizmos::cube`): configured onto layer 1 via
  `GizmoConfigStore::config_mut::<DefaultGizmoConfigGroup>().render_layers = RenderLayers::layer(1)`.

This compiles and runs without error, and the layer assignment is
structurally correct (the output camera's `RenderLayers` component
literally does not include layer 1). **Whether the gizmo is actually
invisible in the output window is a visual check** -- see checklist.

## Vsync coupling — measured

`WindowFrameStats` components (one per camera/window) record wall-clock
delta between consecutive `Update` ticks via `std::time::Instant`, since
`FrameTimeDiagnosticsPlugin` reports one app-global number, not a
per-window one. All three configurations below ran with `--auto-close 240`
on the single available monitor (3440x1440 @144Hz):

| Editor `PresentMode` | Output `PresentMode` | Editor avg dt (ms) | Output avg dt (ms) | Editor ~fps | Output ~fps |
|---|---|---|---|---|---|
| `AutoVsync` | `AutoVsync` | 9.273 | 9.273 | 107.8 | 107.8 |
| `AutoNoVsync` | `AutoVsync` | 9.322 | 9.322 | 107.3 | 107.3 |
| `AutoNoVsync` | `AutoNoVsync` | 7.551 | 7.551 | 132.4 | 132.4 |

Raw output:

```
=== AutoVsync / AutoVsync ===
[m0-4] window camera stats: frames=238 avg_dt_ms=9.273 min_dt_ms=4.161 max_dt_ms=393.409 approx_fps=107.8
[m0-4] window camera stats: frames=238 avg_dt_ms=9.273 min_dt_ms=4.161 max_dt_ms=393.409 approx_fps=107.8

=== AutoNoVsync / AutoVsync ===
[m0-4] window camera stats: frames=238 avg_dt_ms=9.322 min_dt_ms=4.441 max_dt_ms=373.034 approx_fps=107.3
[m0-4] window camera stats: frames=238 avg_dt_ms=9.322 min_dt_ms=4.441 max_dt_ms=373.034 approx_fps=107.3

=== AutoNoVsync / AutoNoVsync ===
[m0-4] window camera stats: frames=238 avg_dt_ms=7.551 min_dt_ms=4.409 max_dt_ms=393.409 approx_fps=132.4
[m0-4] window camera stats: frames=238 avg_dt_ms=7.551 min_dt_ms=4.409 max_dt_ms=393.409 approx_fps=132.4
```

**Finding: the two windows' present modes are fully coupled in every
configuration tested, and this spike's own architecture explains why.**
Both `WindowFrameStats` components are updated by the *same* `Update`
system (`track_window_frame_times`) on the *same* app tick, and Bevy's
default runner advances one single-threaded main schedule per iteration
regardless of how many windows exist. Both windows' swapchains are
presented within that one apptick. So even with the editor's present mode
set to `AutoNoVsync`, if the output window's `AutoVsync` swapchain blocks
waiting for its vblank, the whole app tick -- and therefore the editor's
measured rate too -- waits with it. The only configuration where both
windows ran faster was **both** set to `AutoNoVsync` (132.4fps vs ~107fps),
consistent with removing every vsync wait rather than decoupling the two
windows from each other.

**This directly answers spec §11.3's open question**: with Bevy 0.19.1's
default single-threaded App runner, the editor and output windows'
present modes are not independent -- setting the editor to `AutoNoVsync`
alone does not free it from the output window's vsync wait, or vice versa.
**Consequence for M1, per the plan's own fallback**: "If every configuration
couples them, record it -- the M1 fallback is rendering the editor viewport
every other frame" applies. This spike's single available monitor (144Hz)
could not reproduce the specific worry (a 60Hz projector holding back a
high-refresh editor panel), but the coupling mechanism observed here would
apply equally in that case -- a 60Hz output window's vblank wait would cap
the editor's measured rate too, under this architecture.

**Honesty caveat**: this was measured with one 144Hz monitor hosting both
windows, not a high-refresh editor panel plus a 60Hz projector as spec
§11.3 describes. The coupling *mechanism* (single-threaded main loop,
shared app tick) is confirmed and would apply in the two-refresh-rate case
too, but the actual capped frame rate numbers in that specific scenario are
not measured here -- **user checklist item**.

## Delta from the plan's API sketch

- `RenderTarget` (for both the offscreen-image case in M0-2/M0-3 and the
  second-window case here) is a standalone component spawned alongside
  `Camera`/`Camera3d`, not a field inside a `Camera { target: .. }` struct
  literal -- confirmed against Bevy's own `examples/window/multiple_windows.rs`
  and `examples/window/monitor_info.rs` (both fetched and read before writing
  code).
- `Window` has no `cursor_options` field; `CursorOptions` (with `visible:
  bool`) is a separate component spawned alongside `Window`.
- `Gizmos` has `.cube(transform, color)`, not `.cuboid(...)`.
- `WindowResolution` again has no `From<(f32, f32)>`; `(u32, u32)` only.
- Monitor enumeration and per-monitor info (`Monitor` component: `name`,
  `physical_width`/`physical_height`, `physical_position`,
  `refresh_rate_millihertz`, `scale_factor`) matches the plan's sketch
  closely; this part needed no correction, confirmed against Bevy's own
  `examples/window/monitor_info.rs`.

## User checklist — needs a second display/projector attached

- [x] **Layer isolation — VERIFIED 2026-08-16.** TV shows only the rotating
      cube; the green wireframe gizmo is present in the editor window and
      completely absent from the output window.
- [ ] **Layer isolation, by eye.** With a second monitor attached, confirm
      the wireframe gizmo cube is visible in the editor window and
      **completely absent** from the output window.
- [x] **Real monitor selection — VERIFIED 2026-08-16, after fixing a bug
      that put the output on the wrong screen.** See "The output window
      opened on the wrong monitor". Now lands borderless and full-bleed on
      the second display, confirmed both by eye and by reading the window
      rectangles.
- [ ] **Real monitor selection.** Confirm the output window actually lands
      on the second monitor via `BorderlessFullscreen(MonitorSelection::Entity(..))`
      -- correct position, correct size, no title bar, no border.
- [ ] **Manual fallback path, visual check.** The single-monitor run above
      already exercised `WindowMode::Windowed` + `decorations: false` +
      explicit position/resolution as a *substitute* for
      `BorderlessFullscreen`; with only one monitor there was nothing to
      compare it against. With a second monitor available, deliberately
      force the fallback path (temporarily hardcode it, or note it's the
      code path already in `spawn_output_window`'s single-monitor branch)
      and confirm it still looks correct on the real second display -- this
      is the escape hatch for a projector with a wrong/missing EDID at a
      venue, so it needs to be known-good before it's needed live.
- [x] **Esc-to-close — VERIFIED 2026-08-16.** With the output window
      genuinely fullscreen-borderless on the second display, Esc despawns it
      cleanly; nothing is left on screen. This matters more than it looks:
      the window is undecorated and always-on-top, so a failed despawn would
      strand a black rectangle on the projector with no way to close it by
      mouse.
- [ ] **Esc-to-close on a real fullscreen borderless window.** With the
      output window focused and actually fullscreen-borderless (not the
      single-monitor windowed fallback), press Esc and confirm the window
      and its camera despawn cleanly, with no leftover always-on-top stuck
      window.
- [ ] **Vsync table with mismatched refresh rates.** Repeat the three-row
      table above with the editor on a high-refresh panel and the output on
      an actual 60Hz projector, and confirm whether the output window's
      frame rate ever drops below 60Hz in any configuration -- that is the
      one thing that actually matters per the plan.
- [ ] **10-minute stability run.** Leave both windows running for 10
      minutes while actively dragging/interacting with the editor window.
      Record the output window's frame time: minimum, maximum, and 99th
      percentile (the spike's `WindowFrameStats` component already tracks
      min/max continuously; read it via `--auto-close <frames>` at the
      10-minute frame count, or watch for logged drift if extended to log
      periodically). Watch for drift.

## Second machine — RTX 3070 Laptop, 165Hz, 2026-08-16

Still a single-monitor machine, so the checklist above (real second display,
projector, `BorderlessFullscreen`) remains **open**. What this run adds is a
second confirmation of the vsync-coupling finding on different hardware and a
different refresh rate (165Hz vs 144Hz):

| Editor `PresentMode` | Output `PresentMode` | Editor avg dt | Output avg dt | ~fps |
|---|---|---|---|---|
| Vsync | Vsync | 7.676 ms | 7.676 ms | 130.3 |
| AutoNoVsync | Vsync | 7.478 ms | 7.478 ms | 133.7 |
| AutoNoVsync | AutoNoVsync | 4.642 ms | 4.642 ms | 215.4 |

The editor and output columns are **identical to three decimal places in
every row** — the same total coupling seen on the first machine. Setting only
the editor to `AutoNoVsync` again buys nothing (7.68 -> 7.48 ms is noise);
only turning vsync off in *both* windows changes the rate (215 fps).

Note that even the vsync-on rows run at ~130fps on a 165Hz panel rather than
locking to 165 — the windows are coupled to each other, not cleanly locked to
the display. This does not change the conclusion for M1 (render the editor
view every other frame), but it does mean "vsync on" should not be read as
"presenting exactly at refresh".

The single-monitor fallback path fired again and logged it:

```
[m0-4]   monitor 369v0: name="\.\DISPLAY1" pos=(0, 0) size=2560x1600px refresh=165.0Hz scale=1.25
[m0-4] monitors detected: 1. only one monitor present: opening windowed-borderless
```

## The output window opened on the wrong monitor

With a second display finally attached (a 4K TV over HDMI), the output
window opened **borderless-fullscreen on the laptop panel**, covering the
editor with a window that cannot be dragged away because it has no
decorations and the cursor is hidden.

The bug is in this spike, not in Bevy. `spawn_output_window` chose
`monitors.iter().nth(1)` — "the second monitor enumerated" — with a comment
admitting it was the simplest rule available. Enumeration order is not
specified and does not put the primary first. The actual order here:

```
[m0-4]   monitor 370v0: name="\.\DISPLAY2" pos=(2560, 0) size=3840x2160px refresh=30.0Hz scale=3.00
[m0-4]   monitor 369v0: name="\.\DISPLAY1" pos=(0, 0) size=2560x1600px refresh=165.0Hz scale=1.25
```

The TV enumerated **first**, so `nth(1)` selected the laptop panel. Bevy did
exactly what it was told.

This is the worst shape a bug can take for this project: it works on the
machine it was written on, and at a venue it covers the operator's screen
with an undismissable fullscreen window minutes before a show.

Fixed by selecting on the property actually meant — the monitor without the
`PrimaryMonitor` marker — and logging the chosen monitor's name, size,
refresh rate and scale factor. Added `--output-monitor <index>` as an
override, because the display that reports as primary at a venue is not
always the one the projector is on.

Verified 2026-08-16 by reading the real window rectangles:

| Window | Position | Size | Display |
|---|---|---|---|
| `m0-4 OUTPUT` | x=2048 | 1280x720 logical (3840x2160 physical) | TV |
| `m0-4 EDITOR` | x=205 | 974x638 | laptop |

**Layer isolation confirmed by eye**: the TV shows only the rotating cube;
the green wireframe gizmo appears in the editor window alone. Borderless and
full-bleed on the TV, no title bar.

## A 30fps cap that was not what it looked like

Re-running the vsync matrix with the TV attached produced ~28.5fps in every
row, including both windows on `AutoNoVsync`. The obvious reading — that a
30Hz output display drags the whole application down — was wrong, and so was
the second guess that the TV's mere presence was responsible.

A control run settled it: **m0-1, a single window on the 165Hz laptop panel,
also measured exactly 30.00fps** with the TV attached, and *still* did after
the TV was switched off in Windows. The machine was on battery at 35%, and
30.00fps / 33.33ms is the signature of a power-saving frame limiter, not of
load. With the charger connected — the HDMI cable still attached — the same
binary measured 350-358fps.

**The vsync table measured with the TV attached is therefore void**: every
row was clamped by the battery limiter, not by vsync or by window coupling.

**Methodological finding, and it applies to every measurement in these
documents taken on this laptop: record the power state.** A benchmark that
does not is not reproducible here, because the machine silently changes
performance mode partway through a session. Spot the limiter by its
signature — a suspiciously exact frame rate (30.00fps) with a near-zero
spread — rather than by trusting that the number reflects the code.

Re-measured on AC, the 4K Spout result from M0-3 survives the same scrutiny
(53-55fps at 4K, 97-128fps at 1080p), so that conclusion stands.


## Vsync coupling, measured properly (AC power, 30Hz TV active)

The table above, taken on battery, is void. Re-measured with the charger
connected and the TV live as a `BorderlessFullscreen` output:

| Editor `PresentMode` | Output `PresentMode` | App frame rate |
|---|---|---|
| Vsync | Vsync | 29.4 fps |
| AutoNoVsync | Vsync | 28.9 fps |
| AutoNoVsync | AutoNoVsync | **184.8 fps** |
| Vsync | AutoNoVsync | **138.0 fps** |

**Only the output window's present mode matters.** With the output window
synced to the 30Hz TV, the entire application — including the editor on a
165Hz panel — runs at ~29fps. The editor's own setting changes nothing.
Turning vsync off on the output alone restores 138-185fps.

The rule, stated generally: **the application's frame rate is set by the
slowest vsync-enabled window.** This is a sharper answer than the
single-monitor session could reach, where both windows sat on the same 165Hz
panel and every configuration looked similar. It also replaces the earlier
mitigation sketch (render the editor view every other frame), which
addresses editor cost — not the presentation coupling that actually causes
this.

Tearing with the output on `AutoNoVsync` was checked by eye on the TV:
present, but slight.

## Decision for M1: vsync is the operator's switch

Taken with the user 2026-08-16:

- **Expose vsync as a user-facing toggle on the output window.** It is a
  trade-off with no universally right answer — a tear-free projector at the
  cost of an editor clamped to the projector's refresh, or a fast editor
  with some tearing — and live-visuals operators already understand it.
  Animata-class tools let the operator make this call.
- **Expect Spout to be the path most operators actually use**, which sidesteps
  the coupling entirely: another process owns the projector window, and this
  application never synchronises to that display. M0-3 measured that path at
  ~1.2ms per frame at 1080p.
- The direct second-window output stays for the case where no compositor is
  in the chain.

Context for the numbers: the display used here was a hotel TV at 4K/30Hz,
close to the worst case obtainable. Projectors and LED walls in normal use
run at higher refresh rates, so 29fps is a floor, not a typical figure.
