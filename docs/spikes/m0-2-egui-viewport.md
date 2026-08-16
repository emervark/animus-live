# M0-2 — egui_dock with a render-to-texture viewport

Spike crate: `spikes/m0_2_egui_viewport`. Run with:

```
cargo run --release --manifest-path spikes/m0_2_egui_viewport/Cargo.toml -- [--auto-close <frames>]
```

## Verified four-crate egui version set

```toml
bevy      = "=0.19.1"
bevy_egui = "=0.40"   # resolved 0.40.1
egui      = "=0.34"   # resolved 0.34.3
egui_dock = "=0.19.1"
```

```
$ cargo tree --manifest-path spikes/m0_2_egui_viewport/Cargo.toml -i egui
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

Run: `cargo run --release --manifest-path spikes/m0_2_egui_viewport/Cargo.toml`

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
      --manifest-path spikes/m0_2_egui_viewport/Cargo.toml`. Expected: no
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
