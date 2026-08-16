# M0 spikes

Four throwaway Bevy crates that answer feasibility questions before the M1
editor plan is written. Excluded from the workspace (each has its own
`Cargo.lock`); see `spikes/.cargo/config.toml` for the shared target
directory that lets Bevy compile once instead of four times.

Findings for each spike, including what still needs a human to verify, live
in `docs/spikes/`.

## Run

Run these **from the `spikes/` directory**. Cargo finds `.cargo/config.toml`
by walking up from the working directory, so building from the repo root
would miss the shared target dir and compile Bevy once per spike.

```
cd spikes

# M0-1: procedural skinned mesh (grid, TOP-marker texture, joint ceiling, stress test)
cargo run --release --manifest-path m0_1_skinned_mesh/Cargo.toml -- [--auto-close <frames>] [--stress] [--joints <N>] [--no-bounds-fix] [--no-animate] [--edge-sweep] [--amplitude <N>]

# M0-2: egui_dock render-to-texture viewport (pan/zoom/click, world-grid readout)
cargo run --release --manifest-path m0_2_egui_viewport/Cargo.toml -- [--auto-close <frames>]

# M0-3: Spout sender, CPU readback path by default (frame counter, --path-a for the dead GPU-shared path)
cargo run --release --manifest-path m0_3_spout/Cargo.toml -- [--path-a] [--auto-close <frames>] [--width <px> --height <px>]

# M0-4: second window, RenderLayers isolation, vsync coupling
cargo run --release --manifest-path m0_4_second_window/Cargo.toml -- [--editor-vsync on|off] [--output-vsync on|off] [--auto-close <frames>]
```

Built binaries land in `spikes/target/release/` and can be started directly
without Cargo.

All four accept `--auto-close <frames>`, which closes the window after N
frames and prints measured diagnostics to stdout -- this is how each spike's
findings doc captures real numbers instead of estimates.

## Findings

- [`docs/spikes/m0-1-skinned-mesh.md`](../docs/spikes/m0-1-skinned-mesh.md)
- [`docs/spikes/m0-2-egui-viewport.md`](../docs/spikes/m0-2-egui-viewport.md)
- [`docs/spikes/m0-3-spout.md`](../docs/spikes/m0-3-spout.md)
- [`docs/spikes/m0-4-second-window.md`](../docs/spikes/m0-4-second-window.md)

Each findings doc ends with a "User checklist" section: things that need a
human's eyes (or a second monitor, or OBS) that could not be verified
programmatically.
