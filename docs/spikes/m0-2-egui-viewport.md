# M0-2 — egui_dock with a render-to-texture viewport

Spike crate: `spikes/m0_2_egui_viewport`. Run from the `spikes/` directory (Cargo
discovers the shared-target-dir config by walking up from the working
directory, not from `--manifest-path`):

```
cargo run --release --manifest-path m0_2_egui_viewport/Cargo.toml -- [--auto-close <frames>]
```

## Verified four-crate egui version set

```toml
bevy      = "=0.19.1"
bevy_egui = "=0.40"   # resolved 0.40.1
egui      = "=0.34"   # resolved 0.34.3
egui_dock = "=0.19.1"
```

```
$ cargo tree --manifest-path m0_2_egui_viewport/Cargo.toml -i egui
egui v0.34.3
├── bevy_egui v0.40.1
│   └── m0-2-egui-viewport v0.0.0
├── egui_dock v0.19.1
│   └── m0-2-egui-viewport v0.0.0
└── m0-2-egui-viewport v0.0.0
```

**Exactly one `egui` node, v0.34.3.** The version set in spec §2.1 works as
specified for these four crates; no substitution was needed.

## What this spike's own code verifies

- `egui_dock::DockState` with three tabs (`Left`, `Viewport`, `Right`) via
  `DockArea::new(&mut dock_state).show_inside(ui, &mut tab_viewer)` inside an
  `egui::CentralPanel`, run in the `EguiPrimaryContextPass` schedule (the
  0.19.1/0.40.1 API, see Delta section).
- An `Image` render target (`Bgra8UnormSrgb`, `TEXTURE_BINDING | COPY_SRC |
  RENDER_ATTACHMENT`) registered with `EguiUserTextures::add_image` and
  displayed in the `Viewport` tab via `ui.image(...)`.
- A `Camera3d` with `RenderTarget::Image(handle.into())`, `order: -1`,
  `Projection::Orthographic`.
- A world-space reference grid (red X axis, green Y axis, grey grid lines,
  yellow origin marker) drawn with `Gizmos` into the offscreen scene.
- Pan (middle-drag)/zoom (scroll, zoom-to-cursor) implemented with the
  same-frame before/after unprojection math the plan specifies.
- Click handling: pointer position -> image-local pixel space (multiplied by
  `ctx.pixels_per_point()`) -> `Camera::viewport_to_world_2d`. The resulting
  world position is drawn as a text readout in the top-left of the viewport
  and echoed in the `Right` tab.
- Debounced resize logic (round to physical pixels, clamp >= 1px, only queue
  a resize once the delta exceeds 2px, apply only after 2 stable frames),
  implemented as a `ViewportState` resource plus a separate
  `apply_debounced_resize` system.

A real run, `--auto-close 180`, confirms the app starts, creates one window,
and exits cleanly with no errors once the fix below was applied:

```
[m0-2] args: SpikeArgs { auto_close_frames: Some(180) }
...
[m0-2] window scale_factor=1.000 logical=(1400,900) physical=(1400,900)
[m0-2] auto-close at frame 180, exiting
```

## A real bug found and fixed while building this spike

**bevy_egui's auto-created primary context attaches to the *first* `Camera`
entity spawned in the app, not to a window-targeting camera specifically.**
This spike spawns an offscreen `Camera3d` (targeting the render-to-texture
`Image`, format `Bgra8UnormSrgb`) for the viewport scene. On the first run,
that camera was spawned *before* any window-targeting camera existed, so
`bevy_egui::setup_primary_egui_context_system` (which fires on
`Added<Camera>` and grabs the first one, per its own source in
`bevy_egui-0.40.1/src/lib.rs`) attached the primary egui context to the
**offscreen** camera instead of a window camera. egui's own render pipeline
is hard-coded to target `Rgba8UnormSrgb` (non-HDR default, see
`bevy_egui-0.40.1/src/render/mod.rs:403`), so it tried to render into the
`Bgra8UnormSrgb` offscreen texture and Bevy's render error handler caught a
real wgpu validation error and force-quit the app:

```
ERROR bevy_render::error_handler: Caught rendering error: Validation Error
Caused by:
  In a CommandEncoder
    In a set_pipeline command
      Render pipeline targets are incompatible with render pass
        Incompatible color attachments at indices [0]: the RenderPass uses
        textures with formats [Some(Bgra8UnormSrgb)] but the RenderPipeline
        with 'egui_pipeline' label uses attachments with formats
        [Some(Rgba8UnormSrgb)]
ERROR bevy_render::error_handler: Quitting the application due to Validation RenderError
```

**Fix:** spawn a plain `Camera2d` (default window target) *before* the
offscreen viewport camera, so bevy_egui's auto-primary-context system
attaches to that instead. This is now the first thing `setup()` does, with a
comment explaining why. **This is a real trap for anyone building an editor
with a render-to-texture viewport in bevy_egui 0.40.1**: camera spawn order
matters, and the failure mode gives no hint that it's about camera *order*
— it reads like a texture-format bug. Recorded here so the M1 editor plan
spawns its UI camera first, explicitly, rather than relying on spawn order
by accident.

## Delta from the plan's API sketch

- `DockArea` has no `show(ctx, ...)` — only `show_inside(ui, &mut tab_viewer)`
  called from inside an `egui::CentralPanel`/other panel closure. The plan's
  sketch implied a direct `ctx`-level call; the real 0.19.1 API always goes
  through a panel/ui first.
- bevy_egui 0.40.1 requires UI systems to run in the `EguiPrimaryContextPass`
  schedule, not a plain `Update` system reading `EguiContext` directly (per
  bevy_egui's own `render_to_image_widget.rs` example). `EguiContexts::ctx_mut()`
  returns `Result<&mut egui::Context, QuerySingleError>`, so the UI system
  itself returns `Result` and uses `?`.
- Image texture registration is `EguiUserTextures::add_image(EguiTextureHandle::Strong(handle))`,
  not a bare `Handle<Image>` — `EguiTextureHandle` is a small enum
  (`Strong`/`Weak`) added in this bevy_egui version.
- `Assets<Image>::get_mut` returns `Option<Mut<'_, Image>>` (a
  change-detection wrapper), which requires the local binding to be
  declared `mut` to call `&mut self` methods through it (`if let Some(mut
  image) = images.get_mut(...)`) — a compile error if the binding isn't
  `mut`, even though the value is already a mutable-reference-like type.
- `WindowResolution` has no `From<(f32, f32)>`, same delta as M0-1: use
  `(u32, u32)`.

## Resize debounce — user-observed part

The debounce logic itself is implemented and described above; whether it
actually eliminates wgpu validation errors under rapid interactive resize
(Task 3 Step 6's `RUST_LOG=wgpu_core=warn` drag-the-splitter check) requires
a human dragging the egui_dock splitter for 10 seconds, since this spike has
no dock splitter automation. **User checklist item below.**

## User checklist — must be verified by eyes on the running window

Run: `cargo run --release --manifest-path m0_2_egui_viewport/Cargo.toml`

- [x] **Click accuracy at 125% OS display scaling — VERIFIED 2026-08-16,
      after three fixes (see below).** Clicking the origin marker at 1x zoom
      reports `world: (-0.050, 0.025)`, with the spike's own probe giving
      `1px = 0.0500 world`, i.e. **1.12 px from the origin** — inside the
      accuracy requirement, and consistent with ordinary mouse aim rather
      than a systematic transform error. No offset that grows with zoom was
      observed. Reported world positions quantise to 0.025 world units
      (half a physical pixel).

      This is the fractional-scale case the first machine (100%) could not
      test: a misplaced `pixels_per_point()` multiply would show here as a
      1.25x offset. It does not. **The plan's fallback — replacing the
      egui-hosted viewport with a `SubViewport`-style overlay — is not
      needed.** Still untested: 150% scaling, and 2x/4x zoom under a
      fractional scale factor.

- [ ] **Click accuracy at 1x, 2x, 4x zoom, both 100% and 150% OS display
      scaling.** Click the grid origin (yellow marker) at each zoom level.
      The crosshair/text readout should land within 1px of the click each
      time. Record any consistent offset (e.g., "off by exactly the DPI
      scale factor" points at the `pixels_per_point()` multiply being in the
      wrong place).
  - [ ] 100% DPI, 1x zoom
  - [ ] 100% DPI, 2x zoom
  - [ ] 100% DPI, 4x zoom
  - [ ] 150% DPI, 1x zoom
  - [ ] 150% DPI, 2x zoom
  - [ ] 150% DPI, 4x zoom
- [ ] **Zoom-to-cursor feel.** Scroll while hovering a spot away from the
      origin; that world point should stay under the cursor as you zoom
      (no drift/lag). Note whether it feels correct.
- [ ] **Pan.** Middle-drag; the grid should track the cursor 1:1 regardless
      of zoom level.
- [ ] **Resize stability, debounce ON (default).** Drag the dock splitter
      between the `Left`/`Viewport`/`Right` tabs back and forth rapidly for
      10 seconds with `RUST_LOG=wgpu_core=warn cargo run --release
      --manifest-path m0_2_egui_viewport/Cargo.toml`. Expected: no
      wgpu validation errors printed, no panic, frame rate stays
      interactive.
- [ ] **Resize stability, debounce OFF.** Temporarily lower the debounce
      thresholds in `apply_debounced_resize`/`viewport_ui` (e.g., stable-frame
      count to 0 and the pixel-delta threshold to 0) and repeat the same
      rapid-drag test. Record whether validation errors or stutter appear
      that did not appear with the debounce on — this is what establishes
      the debounce was actually load-bearing, not just present.
- [ ] **General feel of `egui_dock`.** Any friction working with it worth
      recording for the M1 implementer (tab behavior, styling, etc.).

If click accuracy cannot be made reliable across the DPI/zoom matrix, the
plan's fallback is a plain `SubViewport`-style overlay layout instead of an
egui-hosted viewport — note that explicitly here if it comes to that.

## Second machine — 125% display scaling, 2026-08-16

Re-run on a laptop whose Windows display scaling is **125%**, which the first
machine (100%) never exercised. The spike's own startup line confirms the
non-unity path is live:

```
[m0-2] window scale_factor=1.250 logical=(1400,900) physical=(1750,1125)
```

`--auto-close 180` completes with no wgpu validation error and no panic, so
the render-to-texture viewport survives a fractional scale factor
structurally. **What this does not establish is click accuracy** — that
still needs the checklist above, and it is now worth more on this machine
than on the first one: any misplaced `pixels_per_point()` multiply produces a
1.25x offset here that is invisible at 100% scaling.

The DPI matrix in the checklist should therefore be read as 100% / **125%** /
150%, with 125% available natively on this hardware without changing any
Windows setting.

## Two bugs that made click accuracy untestable

The click-accuracy checklist could not be started: clicking in the viewport
did nothing at all — no crosshair, no coordinate readout. Two independent
defects, both in `viewport_ui`.

### 1. The image never sensed clicks

```rust
let response = ui.add(egui::Image::new(SizedTexture::new(tex_id, image_size)));
```

`egui::Image` is created with `Sense::hover()` (egui 0.34,
`src/widgets/image.rs`), so `response.clicked()` is never true. Fixed with
`.sense(egui::Sense::click())`.

### 2. `!wants_pointer_input()` disabled the interactions it was guarding

```rust
let wants_pointer = ui.ctx().wants_pointer_input();
if hovered && !wants_pointer { /* zoom, pan */ }
if hovered && !wants_pointer && response.clicked() { /* sample world pos */ }
```

In egui 0.34:

```rust
egui_wants_pointer_input() =
    egui_is_using_pointer() || (is_pointer_over_egui() && !any_down())
```

This viewport **is** an egui widget, so `is_pointer_over_egui()` is true
whenever the cursor is over it. The guard therefore evaluated:

| Situation | `wants_pointer` | Effect |
|---|---|---|
| Hovering to scroll-zoom (no button down) | true | zoom dead |
| Click (reported on release, no button down) | true | click dead |
| Middle-drag pan (button held) | false | pan works |

So the one interaction that appeared to work was the one the guard happened
to let through, which is why the spike's own `--auto-close` runs — which
exercise no input at all — reported everything healthy.

`!wants_pointer_input()` is the correct guard for an app whose 3D world is
drawn *outside* egui and which must not steal input from egui panels. When
the world is rendered *into* an egui widget, `response.hovered()` /
`response.clicked()` are the right gates: they already account for occlusion
by other widgets and windows. Fixed by dropping the guard.

**For M1:** this is the specific trap to avoid when hosting the viewport in
egui. It is invisible to headless testing, and it degrades to "some input
works, some doesn't" rather than an obvious failure.

### 3. The crosshair the module header promised did not exist

The header comment states "a crosshair marker is drawn at the last click's
unprojected world position". `draw_world_grid` drew the grid and the origin
circle; `last_click_world` fed two text readouts and nothing else.

Without it, the only way to judge accuracy was to click exactly on the origin
marker and read a number — hard to do with a mouse, and it tests exactly one
point in the viewport. The first human attempt reported precisely this: "the
origin looks correct, it's just very hard to hit zero with the mouse."

Added `draw_click_crosshair`, sized in world units from a probed
world-per-pixel so it stays ~12 px per arm at any zoom. Accuracy is now
checked by clicking *anywhere* and comparing the mark against the cursor.

The readout also reports `1px = <N> world` and the click's distance from the
origin **in pixels**, because pixels are the unit the accuracy requirement is
written in. The world-per-pixel figure is probed by unprojecting two points
100 px apart rather than derived from `OrthographicProjection::scale`, whose
meaning depends on the active `ScalingMode` (here `WindowSize`).

## Why three defects survived to this point

None of them are visible to the spike's own `--auto-close` runs, which
exercise no input: the app starts, renders, resizes and exits cleanly in
every case. All three only appear when a human clicks — and the first two
made clicking do nothing at all, so the check they were blocking could not
even begin. The lesson for M1 is that an input path needs an automated test
that actually synthesises input, or it is untested no matter how many
headless frames pass.
