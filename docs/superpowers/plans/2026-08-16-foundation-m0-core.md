# Animus Live — Foundation: M0 Spikes and the Bevy-Free Core

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the Rust workspace, answer the four M0 feasibility questions with throwaway spikes, and build `animus-core` and `animus-project` — the complete, fully tested, Bevy-free foundation that every later milestone projects onto the screen.

**Architecture:** A Cargo workspace whose lower half (`animus-core`, `animus-project`) has **zero Bevy dependency** and tests with plain `cargo test` on any machine with no GPU. The document model is the single source of truth; vertex-index safety is enforced by the type system; the physics solver is a position-based Verlet relaxation over joints (not vertices). The four spikes in `spikes/` are throwaway binaries that answer Bevy/wgpu questions before the M1 plan is written — they are deliberately *not* TDD, because their deliverable is an answer, not code we keep.

**Tech Stack:** Rust (stable, MSVC), `glam`, `serde`/`serde_json`, `indexmap`, `spade` (constrained Delaunay), `i_overlay` (polygon booleans), `imageproc`/`image`, `sha2`, `zip`, `thiserror`, `proptest`, `insta`. Spikes only: `bevy 0.19.1`, `bevy_egui 0.40`, `egui 0.34`, `egui_dock 0.19.1`, `spout2-rs 0.1.1`, `wgpu-hal 29.0.3`.

**Spec:** `docs/superpowers/specs/2026-08-16-animus-live-design.md`

## Global Constraints

- **Clean-room:** Never read, fetch, or paste the original Animata C++ source (github.com/emervark/Animata `src/`). README, file listings, docs, screenshots and videos are permitted. This is a hard rule for every task. (Spec §1.3)
- **`animus-core` and `animus-project` must never depend on Bevy.** CI asserts `cargo tree -p animus-core | grep -q bevy` *fails*. (Spec §3.1)
- **Exact-pinned versions, `Cargo.lock` committed.** `bevy =0.19.1`, `egui =0.34`, `bevy_egui =0.40`, `egui_dock =0.19.1`, `bevy-inspector-egui =0.37`, `wgpu-hal =29.0.3`. Only one `egui` may exist in the graph. (Spec §2.1, §15.2)
- **`glam` must be the exact same semver Bevy's `bevy_math` uses**, or types will not unify across the Bevy boundary. Read it from `cargo tree` and pin it. (Spec §14)
- **Image space is pixels, origin top-left, Y down. World space is Y up.** Only positions flip; UVs do not. (Spec §7.1)
- **Skeleton is a graph of springs, not a hierarchy.** Joints are flat and independent; there is no parent/child bone relationship anywhere in the data model. (Spec §7.3)
- **License:** `MIT OR Apache-2.0` for code; `CC0-1.0` for `spec/`. `cargo deny` fails the build on any GPL/LGPL dependency. (Spec §20)
- **Floats are rejected at serialization time if NaN or Inf.** A NaN in a saved project is a corrupted show. (Spec §4.3)
- **Naming:** crates `animus-*`, binary `animus`, file extension `.animus`, project directory `MyShow.animus/`.
- **Commit after every task.** Conventional commit prefixes (`feat:`, `test:`, `chore:`, `spike:`).

---

## File Structure

**Created by this plan:**

| Path | Responsibility |
|---|---|
| `Cargo.toml` | Workspace root; `[workspace.dependencies]` is the single place versions are pinned |
| `rust-toolchain.toml` | Pins the stable toolchain and the `x86_64-pc-windows-msvc` target |
| `deny.toml` | License allowlist and advisory checks |
| `.cargo/config.toml` | `rust-lld` linker on Windows |
| `.github/workflows/ci.yml` | `lint`, `core-tests`, `no-bevy-check`, `deny` jobs |
| `spikes/m0_1_skinned_mesh/` | Throwaway: procedural skinned mesh in Bevy |
| `spikes/m0_2_egui_viewport/` | Throwaway: egui_dock + render-to-texture editor viewport |
| `spikes/m0_3_spout/` | Throwaway: Spout via wgpu-hal (Path A) and readback (Path B) |
| `spikes/m0_4_second_window/` | Throwaway: borderless fullscreen output + RenderLayers isolation |
| `crates/animus-core/src/ids.rs` | Newtype IDs and the monotonic allocator |
| `crates/animus-core/src/doc/` | `Project`, `Layer`, `Puppet`, `MeshData`, `SkeletonData`, `AttachmentTable`, `AssetRef`, `SolverConfig` |
| `crates/animus-core/src/remap.rs` | `IndexRemap` + `Remappable` — the vertex-deletion safety mechanism |
| `crates/animus-core/src/mesh/edit.rs` | The only path that mutates mesh topology |
| `crates/animus-core/src/mesh/invariants.rs` | `validate()` → `Vec<MeshDefect>` |
| `crates/animus-core/src/solver/` | `SolverState`, `CompiledRig`, `step`, NaN guard |
| `crates/animus-core/src/silhouette/` | alpha threshold, closing, marching squares, RDP, ring topology, fallbacks |
| `crates/animus-core/src/triangulate/` | Poisson-disc points, spade CDT, centroid/area filtering |
| `crates/animus-core/src/skeleton/` | radius-falloff attachment, top-4 GPU bake |
| `crates/animus-project/src/` | JSON codec, content-addressed asset store, safe save, migration chain |
| `spec/animus-project-format-v1.md` | The CC0 format specification |
| `spec/fixtures/` | Golden project directories used by round-trip and migration tests |

**Tasks 2–5 (spikes) are independent of Tasks 6–14 (core) and may be executed in parallel or in either order.** Task 1 blocks everything.

---

## Task 1: Toolchain, workspace skeleton, and CI

**Files:**
- Create: `rust-toolchain.toml`, `Cargo.toml`, `.cargo/config.toml`, `deny.toml`, `.github/workflows/ci.yml`, `LICENSE-MIT`, `LICENSE-APACHE`, `README.md`, `CONTRIBUTING.md`, `docs/heritage.md`
- Create: `crates/animus-core/Cargo.toml`, `crates/animus-core/src/lib.rs`
- Create: `crates/animus-project/Cargo.toml`, `crates/animus-project/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: a compiling workspace with two library crates; `cargo test -p animus-core -p animus-project` succeeds (vacuously); the `[workspace.dependencies]` table every later task adds to.

- [ ] **Step 1: Verify prerequisites, and install them if missing**

Run:
```powershell
rustc --version
cargo --version
where.exe link.exe
```

`rustc` is **not installed** on this machine as of 2026-08-16. Install:
1. `winget install Rustlang.Rustup` (or download from https://rustup.rs)
2. Visual Studio Build Tools with the "Desktop development with C++" workload — this provides `link.exe`, which Bevy requires on Windows: `winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"`
3. Reopen the shell, then `rustup default stable` and `rustup target add x86_64-pc-windows-msvc`

Expected after install: `rustc --version` prints a version, and `where.exe link.exe` finds a path under Visual Studio.

**Do not proceed until all three succeed.** Everything downstream fails without the MSVC linker, and the failure message (`error: linker 'link.exe' not found`) appears hundreds of lines into a Bevy build.

- [ ] **Step 2: Create `rust-toolchain.toml`**

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
targets = ["x86_64-pc-windows-msvc"]
```

- [ ] **Step 3: Create the workspace root `Cargo.toml`**

```toml
[workspace]
resolver = "3"
members = ["crates/*"]
exclude = ["spikes"]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT OR Apache-2.0"
repository = "https://github.com/emervark/animus-live"
rust-version = "1.85"

[workspace.dependencies]
# --- Bevy-free core ---
glam        = { version = "0.30", features = ["serde"] }
serde       = { version = "1", features = ["derive"] }
serde_json  = { version = "1", features = ["preserve_order"] }
indexmap    = { version = "2", features = ["serde"] }
thiserror   = "2"
tracing     = "0.1"

# --- geometry ---
spade       = "2.15"
i_overlay   = "8.1"
imageproc   = "0.27"
image       = { version = "0.25", default-features = false, features = ["png", "jpeg"] }

# --- project io ---
sha2        = "0.10"
zip         = "5"
tempfile    = "3"

# --- testing ---
proptest    = "1"
insta       = { version = "1", features = ["json"] }
approx      = "0.5"

# --- internal ---
animus-core    = { path = "crates/animus-core" }
animus-project = { path = "crates/animus-project" }

[profile.dev]
opt-level = 1
debug = 1

[profile.dev.package."*"]
opt-level = 3
debug = false

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
strip = "debuginfo"
panic = "unwind"       # load-bearing: per-subsystem catch_unwind depends on it

[profile.dist]
inherits = "release"
strip = "none"
debug = 1
```

**Note on `glam`:** `0.30` is a placeholder until Bevy is added in Task 2. Step 8 of Task 2 corrects it to Bevy's exact version. `animus-core` must not link a second copy of `glam`.

- [ ] **Step 4: Create `.cargo/config.toml`**

```toml
[target.x86_64-pc-windows-msvc]
linker = "rust-lld.exe"
rustflags = ["-Zshare-generics=off"]

[alias]
core-test = "test -p animus-core -p animus-project"
```

If `-Zshare-generics=off` is rejected on stable, delete that line — it is a compile-time optimization, not a correctness requirement.

- [ ] **Step 5: Create the two library crates**

`crates/animus-core/Cargo.toml`:
```toml
[package]
name = "animus-core"
description = "Document model, geometry and physics solver for Animus Live. No engine dependency."
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
glam.workspace = true
serde.workspace = true
indexmap.workspace = true
thiserror.workspace = true
spade.workspace = true
i_overlay.workspace = true
imageproc.workspace = true
image.workspace = true

[dev-dependencies]
proptest.workspace = true
approx.workspace = true
serde_json.workspace = true
```

`crates/animus-core/src/lib.rs`:
```rust
//! Document model, geometry and physics for Animus Live.
//!
//! This crate has **no engine dependency**. It compiles and tests on any
//! platform with no GPU. See the design spec, section 3.1.
#![forbid(unsafe_code)]

pub mod ids;
```

`crates/animus-project/Cargo.toml`:
```toml
[package]
name = "animus-project"
description = "On-disk project format for Animus Live: JSON document, content-addressed assets, migrations."
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
animus-core.workspace = true
serde.workspace = true
serde_json.workspace = true
indexmap.workspace = true
thiserror.workspace = true
sha2.workspace = true
zip.workspace = true

[dev-dependencies]
insta.workspace = true
tempfile.workspace = true
```

`crates/animus-project/src/lib.rs`:
```rust
//! On-disk project format for Animus Live.
#![forbid(unsafe_code)]
```

Create `crates/animus-core/src/ids.rs` as an empty file for now — Task 6 fills it.

- [ ] **Step 6: Create `deny.toml`**

```toml
[licenses]
version = 2
allow = [
  "MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
  "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib",
  "Unicode-3.0", "CC0-1.0", "MPL-2.0",
]
confidence-threshold = 0.9

[advisories]
version = 2
yanked = "deny"

[bans]
multiple-versions = "warn"
deny = []
```

GPL and LGPL are absent from `allow`, so any such dependency fails the build. This is the mechanical guarantee behind the clean-room and NDI licensing decisions.

- [ ] **Step 7: Create `.github/workflows/ci.yml`**

```yaml
name: CI
on:
  push: { branches: [main] }
  pull_request:

env:
  CARGO_TERM_COLOR: always

jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: rustfmt, clippy }
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy -p animus-core -p animus-project --all-targets -- -D warnings

  core-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test -p animus-core -p animus-project

  no-bevy-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: animus-core must not depend on bevy
        run: |
          if cargo tree -p animus-core | grep -qi bevy; then
            echo "::error::animus-core has acquired a Bevy dependency. See spec section 3.1."
            exit 1
          fi
      - name: animus-project must not depend on bevy
        run: |
          if cargo tree -p animus-project | grep -qi bevy; then
            echo "::error::animus-project has acquired a Bevy dependency. See spec section 3.1."
            exit 1
          fi

  deny:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: EmbarkStudios/cargo-deny-action@v2
```

- [ ] **Step 8: Write `README.md`, `CONTRIBUTING.md`, `docs/heritage.md`, and the two license files**

`README.md` must contain, verbatim:

> Animus Live is an independent, clean-room reimplementation inspired by [Animata](http://animata.kibu.hu/) (Kitchen Budapest, 2007). It is not affiliated with the original project and contains no code derived from it.

`CONTRIBUTING.md` must contain, as a rule and not a suggestion:

> **Do not read the original Animata C++ source.** Animus Live is a clean-room reimplementation. Reading the original's `src/` directory, or pasting it into an AI coding assistant, would compromise that. The original's README, documentation, published papers, screenshots and videos are fine and are the intended reference material.
>
> The easiest place to start contributing is `crates/animus-core` and `crates/animus-project`. They are plain Rust with no engine dependency — you can build and test them on any machine with `cargo test -p animus-core -p animus-project`, no GPU and no Bevy knowledge required.

`docs/heritage.md` credits Péter Németh, Gábor Papp and Bence Samu, and Kitchen Budapest, and records what was drawn from the original: the mass-spring puppet model, joints-as-graph-not-hierarchy, and the OSC-driven live workflow.

Fetch the standard MIT and Apache-2.0 texts for `LICENSE-MIT` and `LICENSE-APACHE`.

- [ ] **Step 9: Verify the workspace builds and the no-bevy check passes locally**

Run:
```bash
cargo fmt --all --check
cargo clippy -p animus-core -p animus-project --all-targets -- -D warnings
cargo test -p animus-core -p animus-project
cargo tree -p animus-core | grep -i bevy    # must print nothing, exit 1
```

Expected: fmt and clippy clean; `test` reports `0 passed` for both crates; the `grep` finds nothing.

- [ ] **Step 10: Commit**

```bash
git add -A
git commit -m "chore: scaffold cargo workspace, CI, and licensing

Two library crates with no engine dependency, exact-pinned workspace
dependencies, and a CI job that fails if animus-core ever acquires one."
```

---

## Task 2: Spike M0-1 — procedural skinned mesh in Bevy

**This is a spike. Its deliverable is an answer plus measured numbers, not code we keep.** No TDD. Everything under `spikes/` is excluded from the workspace and will be deleted after M1.

**Files:**
- Create: `spikes/m0_1_skinned_mesh/Cargo.toml`, `spikes/m0_1_skinned_mesh/src/main.rs`
- Create: `spikes/m0_1_skinned_mesh/assets/top_marker.png` (a texture with the word "TOP" legible along its top edge — generate it, or draw it in any editor)
- Create: `docs/spikes/m0-1-skinned-mesh.md` (the findings)

**Interfaces:**
- Consumes: Task 1's toolchain.
- Produces: `docs/spikes/m0-1-skinned-mesh.md` recording (a) whether procedural skinned meshes work as the spec assumes, (b) the exact `glam` version Bevy uses, (c) per-frame cost at 50 puppets, (d) confirmation of the Y-flip and UV-orientation rules. **The M1 plan is written against these findings.**

- [ ] **Step 1: Create the spike crate**

`spikes/m0_1_skinned_mesh/Cargo.toml`:
```toml
[package]
name = "m0-1-skinned-mesh"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
bevy = "=0.19.1"
```

Because `spikes` is in the workspace `exclude` list, this crate has its own `Cargo.lock`. Build it with `cargo run --manifest-path spikes/m0_1_skinned_mesh/Cargo.toml --release`.

- [ ] **Step 2: Read Bevy's own reference implementation first**

Open https://github.com/bevyengine/bevy/blob/v0.19.1/examples/animation/custom_skinned_mesh.rs and read it before writing anything. It is the authoritative source for the exact API shape. Note in particular how `ATTRIBUTE_JOINT_INDEX` is inserted — it **must** be `VertexAttributeValues::Uint16x4`, because a bare `[u16; 4]` is ambiguous with `Unorm16x4` and produces silently wrong deformation.

- [ ] **Step 3: Build the spike scene**

Requirements, all of which must be visible on screen:

1. A **30 × 30 vertex textured grid quad** (900 vertices, 1682 triangles) built with `Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)`.
2. **40 joint entities, all direct children of the puppet root and siblings of each other — no `ChildOf` between them.** This is the critical thing to prove: the spec's skeleton is a spring graph, not a hierarchy.

   *Vocabulary:* Bevy calls the entries of `SkinnedMesh.joints` "joints". In Animus Live these correspond to **bones**, not to our `Joint` type (spec §7.3). The spike uses Bevy's word because it is exercising Bevy's API; keep the distinction in mind when reading the findings.
3. `SkinnedMesh { inverse_bindposes, joints }` where `joints: Vec<Entity>` is flat.
4. `mesh.with_generated_skinned_mesh_bounds()` applied, and `DynamicSkinnedMeshBounds` on the entity. **Without these the mesh is frustum-culled wrongly and will vanish when the camera moves** — verify by orbiting.
5. A system that drives each joint's `Transform.translation` with an offset sine so the grid visibly waves.
6. The texture is `top_marker.png`, applied with `unlit: true`, `cull_mode: None`, `double_sided: true`, and UVs assigned as `uv = (x/w, y/h)` with **no Y flip**, while positions use `Vec3::new(x/ppu, -y/ppu, 0.0)`.

- [ ] **Step 4: Verify the Y-flip and UV rule**

Run the spike. **The word "TOP" must appear at the top of the quad, right way up, not mirrored.**

If it appears at the bottom, the position flip and the UV convention disagree — fix by changing the *position* formula, never the UVs, and record which is correct in the findings doc. Spec §7.1 asserts positions flip and UVs do not; this step is what makes that assertion trustworthy instead of assumed.

- [ ] **Step 5: Verify the culling fix is actually necessary**

Comment out `with_generated_skinned_mesh_bounds()` and `DynamicSkinnedMeshBounds`, rerun, and orbit the camera until the mesh disappears. Restore them and confirm it no longer disappears. Record what you saw. This proves the requirement is real rather than cargo-culted from the release notes.

- [ ] **Step 6: Measure the joint ceiling**

Raise the joint count to 256 (Bevy's `MAX_JOINTS`) and confirm it still renders. Raise it to 257 and record exactly what happens — panic, silent corruption, or a warning. The M1 validator must produce a clear user-facing message *before* whatever this is.

- [ ] **Step 7: Measure per-frame cost at scale**

Spawn 50 independent puppets, each with its own mesh and 40 joints, all animating. Record with `bevy::diagnostic::FrameTimeDiagnosticsPlugin`:
- frame time at 50 puppets, release build
- frame time with the joint-driving system disabled (isolates CPU animation cost from render cost)

- [ ] **Step 8: Pin `glam` to Bevy's exact version**

Run:
```bash
cargo tree --manifest-path spikes/m0_1_skinned_mesh/Cargo.toml -i glam
```

Take the version printed and set it in the workspace root `[workspace.dependencies]`, replacing the `0.30` placeholder from Task 1. Then run `cargo test -p animus-core -p animus-project` to confirm nothing broke.

**This matters:** if `animus-core` links a different `glam` than Bevy, `Vec2` in the core is a *different type* from `Vec2` in Bevy, and every value crossing the boundary needs a conversion. Getting this wrong is invisible until Task 6's types meet Bevy in M1.

- [ ] **Step 9: Write the findings**

`docs/spikes/m0-1-skinned-mesh.md` must answer, each in one or two sentences with the evidence:
- Do flat (non-parented) joints skin correctly? **If no, the entire unified 2D/3D architecture in spec §7 needs revisiting — stop and escalate.**
- Which of positions/UVs flips in Y?
- What happens at 257 joints?
- Frame time at 50 puppets × 40 joints, and the split between animation and render.
- Bevy's exact `glam` version.
- Anything surprising.

- [ ] **Step 10: Commit**

```bash
git add spikes/m0_1_skinned_mesh docs/spikes/m0-1-skinned-mesh.md Cargo.toml
git commit -m "spike: M0-1 procedural skinned mesh in Bevy

Proves flat joint sets skin correctly, confirms the Y-flip/UV rule,
measures the joint ceiling and per-frame cost, and pins glam to Bevy's
exact version."
```

---

## Task 3: Spike M0-2 — egui dock with a render-to-texture viewport

**Spike. Deliverable is an answer.**

**Files:**
- Create: `spikes/m0_2_egui_viewport/Cargo.toml`, `spikes/m0_2_egui_viewport/src/main.rs`
- Create: `docs/spikes/m0-2-egui-viewport.md`

**Interfaces:**
- Consumes: Task 1's toolchain.
- Produces: findings on click accuracy, resize behaviour, and the verified four-crate egui version set. The M1 editor plan depends on all of it.

- [ ] **Step 1: Create the spike crate with the locked egui set**

```toml
[package]
name = "m0-2-egui-viewport"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
bevy      = "=0.19.1"
bevy_egui = "=0.40"
egui      = "=0.34"
egui_dock = "=0.19.1"
```

- [ ] **Step 2: Confirm exactly one `egui` is in the graph**

Run:
```bash
cargo tree --manifest-path spikes/m0_2_egui_viewport/Cargo.toml -i egui
```

Expected: **one** `egui v0.34.x` node. If two versions appear, the version set in spec §2.1 is wrong for these crates; find the working combination by reading each crate's README compatibility table on docs.rs and record the corrected set in the findings. Do not proceed with two `egui`s — the types will not interoperate and the failure mode is a confusing trait-mismatch error much later.

- [ ] **Step 3: Build the spike**

1. `EguiPlugin` + an `egui_dock::DockState` with three tabs: `Viewport`, `Left`, `Right`.
2. An `Image` render target: `Image::new_fill(...)` with `TextureFormat::Bgra8UnormSrgb`, `usage |= RENDER_ATTACHMENT | COPY_SRC`, added to `Assets<Image>` and registered via `EguiUserTextures::add_image`.
3. A `Camera3d` with `Camera { target: RenderTarget::Image(handle.into()), order: -1, .. }` and `Projection::Orthographic`.
4. In the `Viewport` tab, `ui.image(SizedTexture::new(tex_id, size))`.
5. A visible world-space reference grid with a marked origin, so click accuracy is measurable by eye.

- [ ] **Step 4: Implement pan/zoom with zoom-to-cursor in a single system**

The projection change and the second unprojection must happen in the same frame with manual math — letting the projection update propagate introduces a one-frame lag that makes zooming feel broken:

```rust
// pseudo-shape; the real unproject uses the camera + its GlobalTransform
let before = world_at(proj.scale, &cam_xf, cursor_in_image);
proj.scale = (proj.scale * (1.0 - scroll * 0.1)).clamp(MIN, MAX);
let after  = world_at(proj.scale, &cam_xf, cursor_in_image);
cam_xf.translation += (before - after).extend(0.0);
```

Middle-drag pans, scaled by `proj.scale / image_height` so the point under the cursor tracks 1:1.

- [ ] **Step 5: Implement and verify click accuracy**

Convert the egui pointer position into the image's pixel space and unproject:

```rust
let pixel_in_image = (pointer_pos - image_rect.min) * window.scale_factor();
let world = camera.viewport_to_world_2d(&cam_global_xf, pixel_in_image)?;
```

Draw a marker at the returned world position and read out the numeric world coordinate in the UI.

**Verify: click the grid origin at 1×, 2× and 4× zoom, on a 100% DPI display and a 150% DPI display. The marker must land within 1 pixel of the click every time.** DPI scaling is the usual failure; if it is off by exactly the scale factor, the `scale_factor()` multiply is in the wrong place.

Guard all viewport input on `response.hovered() && !ctx.wants_pointer_input()`.

- [ ] **Step 6: Implement debounced resize and verify no validation errors**

React to the panel rect changing, but: round to whole physical pixels, multiply by `scale_factor()`, clamp to ≥ 1, and only resize if the delta exceeds 2 px **or** the size has been stable for 2 frames.

**Verify:** drag the dock splitter back and forth rapidly for 10 seconds with `RUST_LOG=wgpu_core=warn`. Expected: no wgpu validation errors, no panic, frame rate stays interactive. Then comment the debounce out and repeat, to confirm it was actually load-bearing.

- [ ] **Step 7: Write the findings**

`docs/spikes/m0-2-egui-viewport.md`: the verified four-crate version set; click accuracy results at each zoom and DPI combination; what happened during undebounced resize; and how `egui_dock` felt to work with. If click accuracy cannot be made reliable, say so — the fallback is a plain `SubViewport`-style overlay layout rather than egui-hosted, and M1's UI plan changes substantially.

- [ ] **Step 8: Commit**

```bash
git add spikes/m0_2_egui_viewport docs/spikes/m0-2-egui-viewport.md
git commit -m "spike: M0-2 egui_dock with a render-to-texture viewport

Verifies the four-crate egui version lockstep, sub-pixel click accuracy
across zoom and DPI, and debounced render-target resize."
```

---

## Task 4: Spike M0-3 — Spout, both paths

**Spike, and the highest-risk one in the plan. Path A may fail; that is an acceptable outcome, but Path B must work.**

**Files:**
- Create: `spikes/m0_3_spout/Cargo.toml`, `spikes/m0_3_spout/src/main.rs`
- Create: `docs/spikes/m0-3-spout.md`

**Interfaces:**
- Consumes: Task 1's toolchain.
- Produces: a go/no-go on GPU-shared Spout, and a **measured** end-to-end latency figure for the readback path. Spec §12.2's claims become facts or get corrected.

**Prerequisite:** install OBS Studio with the Spout2 plugin (https://github.com/Off-World-Live/obs-spout2-plugin) so there is something to receive with.

- [ ] **Step 1: Answer the blocking unknown before writing any code**

Spec §2 flags this as unverified: *does `wgpu_hal::dx12::Texture` expose a public raw `ID3D12Resource` accessor in 29.0.3?* docs.rs cannot answer it because the module is Windows-gated and docs.rs builds on Linux.

Run:
```bash
cargo new --lib /tmp/halcheck && cd /tmp/halcheck
cargo add wgpu-hal@=29.0.3
cargo doc --target x86_64-pc-windows-msvc -p wgpu-hal --open
```

Read the `wgpu_hal::dx12` module. Record in the findings doc: the exact accessor name and signature if one exists, or "no public accessor" if not.

**If there is no public accessor, Path A is dead. Skip to Step 5 and record it.** Do not spend time trying to work around it — Path B is a shipping path, and spec §12.2 already commits to shipping it first.

- [ ] **Step 2: Create the spike crate**

```toml
[package]
name = "m0-3-spout"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
bevy      = "=0.19.1"
spout2-rs = { version = "0.1.1", features = ["dx12"] }
wgpu-hal  = "=29.0.3"
windows   = "..."   # MUST match the version wgpu-hal depends on:
                    # cargo tree -p wgpu-hal -i windows
```

**The `windows` crate version must match `wgpu-hal`'s exactly.** If it does not, `ID3D12Resource` from your `windows` and `ID3D12Resource` from `wgpu-hal`'s `windows` are nominally distinct types and will not unify — a genuinely baffling error if you are not expecting it.

- [ ] **Step 3: Force the DX12 backend**

```rust
DefaultPlugins.set(RenderPlugin {
    render_creation: WgpuSettings {
        backends: Some(Backends::DX12),
        ..default()
    }.into(),
    ..default()
})
```

**Verify the backend actually took**, by logging the adapter info at startup. Bevy may prefer Vulkan on Windows, and on Vulkan Path A is impossible. If DX12 cannot be forced, record that and go to Path B.

- [ ] **Step 4: Path A — GPU-shared send**

1. Render the scene to an `Image` render target.
2. In the render world, get the `GpuImage` from `RenderAssets<GpuImage>`.
3. `unsafe { gpu_image.texture.as_hal::<wgpu_hal::api::Dx12>(|t| ...) }` to reach the `wgpu_hal::dx12::Texture`.
4. Extract the raw `ID3D12Resource` using the accessor found in Step 1 and hand it to `spout2-rs`'s dx12 sender.

Expected failure mode, and the thing to watch for: **resource state.** D3D11On12 must acquire the texture in `D3D12_RESOURCE_STATE_COMMON` or with `ALLOW_SIMULTANEOUS_ACCESS`, and wgpu manages resource state internally without exposing it. The symptom is a D3D12 debug-layer error about an invalid state transition, or a black/garbage frame in OBS.

Timebox this to **one working day**. If it does not work in a day, it is not going to work without engine-level changes, and Path B ships anyway.

- [ ] **Step 5: Path B — CPU readback send (this one must work)**

```rust
commands.spawn(Readback::texture(image_handle.clone()))
    .observe(|trigger: On<ReadbackComplete>, mut sender: ResMut<SpoutSender>| {
        let bytes: &[u8] = &trigger.0;
        sender.send_image(bytes, width, height);
    });
```

- [ ] **Step 6: Measure the readback latency properly**

Render a large frame counter into the scene, incrementing every frame. Point a phone camera at both the Bevy window and the OBS preview in one shot, record video, then step through frame by frame and read the difference between the two counters.

**This is the honest way to measure it. Do not estimate.** Spec §12.2 claims 1–3 frames (16–50 ms); confirm or correct it, because that number goes into the user documentation.

Also record, with `FrameTimeDiagnosticsPlugin`: frame time with the readback active versus disabled, at 1080p60. Spec §12.2 budgets 3–4 ms of CPU.

- [ ] **Step 7: Test 4K**

Set the render target to 3840×2160 and repeat Step 6. Spec §12.2 predicts ~2 GB/s and ~12 ms of memcpy — "not viable". Confirm or correct it. The answer determines what resolutions the README advertises.

- [ ] **Step 8: Write the findings**

`docs/spikes/m0-3-spout.md`:
- Is there a public raw-resource accessor in `wgpu-hal` 29.0.3? Exact signature.
- Did DX12 get forced?
- Path A: worked / failed, and the precise failure.
- Path B: measured latency in frames and ms, measured CPU cost per frame, at 1080p and 4K.
- **Recommendation for M4:** ship Path B only, or attempt Path A again.

- [ ] **Step 9: Commit**

```bash
git add spikes/m0_3_spout docs/spikes/m0-3-spout.md
git commit -m "spike: M0-3 Spout via wgpu-hal and via CPU readback

Records whether GPU-shared sending is reachable through wgpu, and
measures readback latency and cost at 1080p and 4K."
```

---

## Task 5: Spike M0-4 — second window, RenderLayers isolation, vsync

**Spike. Requires a physically connected second display or projector.**

**Files:**
- Create: `spikes/m0_4_second_window/Cargo.toml`, `spikes/m0_4_second_window/src/main.rs`
- Create: `docs/spikes/m0-4-second-window.md`

**Interfaces:**
- Consumes: Task 1's toolchain.
- Produces: confirmation that one world can feed two windows with layer isolation, plus the vsync coupling answer that spec §11.3 flags as unresolved.

- [ ] **Step 1: Create the spike crate**

```toml
[package]
name = "m0-4-second-window"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
bevy = "=0.19.1"
```

- [ ] **Step 2: Build two windows over one world**

1. The primary window with a `Camera3d` at `order: 0` and `RenderLayers::from_layers(&[0, 1])`.
2. A rotating cube on `RenderLayers::layer(0)` — the "content".
3. A `Gizmos`-drawn wireframe box on `RenderLayers::layer(1)` — the "editor gizmo", configured via `GizmoConfig::render_layers`.
4. A second `Window` entity: `WindowMode::BorderlessFullscreen(MonitorSelection::Entity(monitor))`, `decorations: false`, cursor hidden.
5. A second `Camera3d` targeting it at `order: 10` with `RenderLayers::layer(0)` **only**.

- [ ] **Step 3: Verify layer isolation**

**The wireframe gizmo must be visible in the editor window and completely absent from the projector.** This one behaviour is what keeps editing overlays off a live projection; if it does not hold, the M1 output design needs a different mechanism (a second `World`, which is substantially more work).

- [ ] **Step 4: Verify monitor selection, then break it deliberately**

Enumerate monitors with `Query<(Entity, &Monitor)>` and print name, position, physical size and refresh rate for each. Open the output window on monitor 2 and confirm it lands there.

Then test the fallback path spec §11.2 requires: `WindowMode::Windowed` + `decorations: false` + `position: WindowPosition::At(IVec2)` + explicit `resolution`, with hand-typed coordinates. **Confirm this also produces a correct borderless fullscreen result**, because it is the escape hatch when a projector reports wrong EDID and it must be known to work before it is needed at a venue.

- [ ] **Step 5: Measure vsync coupling**

This is spec §11.3's open question. With the editor on a high-refresh panel and the output on a 60 Hz projector, measure the frame rate of each window with `FrameTimeDiagnosticsPlugin` under three configurations:

| Editor `PresentMode` | Output `PresentMode` | Editor fps | Output fps |
|---|---|---|---|
| `AutoVsync` | `AutoVsync` | | |
| `AutoNoVsync` | `AutoVsync` | | |
| `AutoNoVsync` | `AutoNoVsync` | | |

**What matters is that the output window never drops below its monitor's refresh rate.** If every configuration couples them, record it — the M1 fallback is rendering the editor viewport every other frame.

- [ ] **Step 6: Implement and verify Esc-to-close**

Bevy has no built-in `close_on_esc`. Write a system that despawns the output window entity and its camera when `Esc` is pressed while it has focus.

**Verify it works when the window is borderless, always-on-top and fullscreen** — this is exactly the situation where a stuck window is a live-show hazard, and it is the situation where it is most likely to misbehave.

- [ ] **Step 7: Ten-minute stability run**

Leave both windows running for 10 minutes while actively dragging and interacting with the editor window. Record the output window's frame time: minimum, maximum, and 99th percentile. Watch for drift.

- [ ] **Step 8: Write the findings and commit**

`docs/spikes/m0-4-second-window.md`: layer isolation yes/no; monitor selection and the manual fallback; the full vsync table; Esc behaviour; the 10-minute frame-time statistics.

```bash
git add spikes/m0_4_second_window docs/spikes/m0-4-second-window.md
git commit -m "spike: M0-4 second window, RenderLayers isolation, vsync coupling

Proves one world can feed an editor window and a clean projector output,
and measures whether the two windows' present modes couple."
```

---

## Task 6: Core — stable IDs

**Files:**
- Modify: `crates/animus-core/src/ids.rs`
- Modify: `crates/animus-core/src/lib.rs`

**Interfaces:**
- Consumes: Task 1's crate skeleton.
- Produces:
  - `pub struct LayerId(pub u64)`, and identically `PuppetId`, `BoneId`, `JointId`, `AssetId`, `BindingId` — each `Copy + Clone + Debug + PartialEq + Eq + Hash + PartialOrd + Ord + Serialize + Deserialize`, serialized as a bare JSON number.
  - `pub struct IdAlloc { next: u64 }` with `IdAlloc::new() -> Self`, `IdAlloc::from_next(next: u64) -> Self`, `IdAlloc::next(&mut self) -> u64`, `IdAlloc::peek(&self) -> u64`.

- [ ] **Step 1: Write the failing test**

Create `crates/animus-core/src/ids.rs` with the tests at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_never_reused() {
        let mut alloc = IdAlloc::new();
        let a = LayerId(alloc.next());
        let b = LayerId(alloc.next());
        let c = LayerId(alloc.next());
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
        assert_eq!(alloc.peek(), 4, "first id is 1, so next unallocated is 4");
    }

    #[test]
    fn id_zero_is_never_allocated() {
        // 0 is reserved as a sentinel meaning "unset".
        let mut alloc = IdAlloc::new();
        assert_ne!(alloc.next(), 0);
    }

    #[test]
    fn alloc_resumes_from_a_loaded_project() {
        let mut alloc = IdAlloc::from_next(500);
        assert_eq!(alloc.next(), 500);
        assert_eq!(alloc.next(), 501);
    }

    #[test]
    fn ids_serialize_as_bare_numbers() {
        let json = serde_json::to_string(&PuppetId(42)).unwrap();
        assert_eq!(json, "42");
        let back: PuppetId = serde_json::from_str("42").unwrap();
        assert_eq!(back, PuppetId(42));
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p animus-core ids`
Expected: FAIL — `cannot find type IdAlloc in this scope`.

- [ ] **Step 3: Write the implementation**

```rust
//! Stable identifiers for document entities.
//!
//! IDs are allocated monotonically and **never reused**, so a stale
//! reference is detectably dangling rather than silently pointing at a
//! different object. `0` is reserved as an "unset" sentinel and is never
//! handed out.

use serde::{Deserialize, Serialize};

macro_rules! define_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
            Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

define_id!(/// Identifies a `Layer` within a `Project`.  LayerId);
define_id!(/// Identifies a `Puppet` within a `Project`. PuppetId);
define_id!(/// Identifies a `Bone` within a `SkeletonData`. BoneId);
define_id!(/// Identifies a `Joint` within a `SkeletonData`. JointId);
define_id!(/// Identifies an `AssetRef` within a `Project`. AssetId);
define_id!(/// Identifies a `Binding` within a `Project`. BindingId);

/// Monotonic ID allocator. Serialized as `Project::next_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdAlloc {
    next: u64,
}

impl IdAlloc {
    /// A fresh allocator. The first ID handed out is 1.
    pub fn new() -> Self {
        Self { next: 1 }
    }

    /// Resume allocation for a project loaded from disk.
    pub fn from_next(next: u64) -> Self {
        Self { next: next.max(1) }
    }

    /// Allocate the next unused ID.
    pub fn next(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }

    /// The ID that would be allocated next, without allocating it.
    pub fn peek(&self) -> u64 {
        self.next
    }
}

impl Default for IdAlloc {
    fn default() -> Self {
        Self::new()
    }
}
```

Add to `lib.rs`: `pub mod ids;` (already present from Task 1).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p animus-core ids`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/animus-core/src/ids.rs crates/animus-core/src/lib.rs
git commit -m "feat(core): stable, never-reused entity IDs

IDs serialize as bare JSON numbers; 0 is a reserved 'unset' sentinel."
```

---

## Task 7: Core — the document model

**Files:**
- Create: `crates/animus-core/src/doc/mod.rs`, `layer.rs`, `puppet.rs`, `mesh_puppet.rs`, `model_puppet.rs`, `asset.rs`, `solver_cfg.rs`, `stage.rs`
- Modify: `crates/animus-core/src/lib.rs`

**Interfaces:**
- Consumes: `animus_core::ids::*` from Task 6.
- Produces: the complete document type tree. Later tasks depend on these exact names:
  - `Project { schema_version, meta, next_id, assets, layers, layer_data, puppets, bindings, solver, stage }`
  - `MeshData { positions: Vec<Vec2>, uvs: Vec<Vec2>, triangles: Vec<u32>, source: MeshSource }`
  - `SkeletonData { joints: IndexMap<JointId, Joint>, bones: IndexMap<BoneId, Bone> }`
  - `Joint { id, name, rest: Vec2, rest_angle: f32, inv_mass: f32, pinned: bool }`
  - `Bone { id, name, a: JointId, b: JointId, rest_length: Option<f32>, stiffness, damping, length_mul, attach_radius }`
  - `AttachmentTable { entries: Vec<Attachment> }`, `Attachment { vertex: u32, bone: BoneId, weight: f32, local: Vec2 }`
  - `SolverConfig { hz, iterations, gravity, global_damping, max_substeps_per_frame, enabled }`
  - `Project::new(name: &str) -> Self`, `Project::alloc_id(&mut self) -> u64`

- [ ] **Step 1: Write the failing test**

`crates/animus-core/src/doc/mod.rs`, tests at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_project_is_empty_and_current_schema() {
        let p = Project::new("Test Show");
        assert_eq!(p.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(p.meta.name, "Test Show");
        assert!(p.layers.is_empty());
        assert!(p.puppets.is_empty());
        assert!(p.assets.is_empty());
        assert_eq!(p.next_id, 1);
    }

    #[test]
    fn alloc_id_advances_next_id() {
        let mut p = Project::new("Test Show");
        let a = p.alloc_id();
        let b = p.alloc_id();
        assert_ne!(a, b);
        assert_eq!(p.next_id, 3);
    }

    #[test]
    fn solver_defaults_match_the_spec() {
        let s = SolverConfig::default();
        assert_eq!(s.hz, 120);
        assert_eq!(s.iterations, 8);
        assert_eq!(s.max_substeps_per_frame, 8);
        assert!(s.enabled);
        assert_eq!(s.gravity, glam::Vec2::ZERO);
    }

    #[test]
    fn bone_defaults_leave_length_mul_at_one() {
        let b = Bone {
            id: BoneId(1),
            name: "arm".into(),
            a: JointId(1),
            b: JointId(2),
            rest_length: None,
            stiffness: 0.8,
            damping: 0.1,
            length_mul: 1.0,
            attach_radius: 30.0,
        };
        assert_eq!(b.length_mul, 1.0);
        assert!(b.rest_length.is_none(), "None means: compute from rest positions");
    }

    #[test]
    fn project_round_trips_through_json() {
        let mut p = Project::new("Round Trip");
        let lid = LayerId(p.alloc_id());
        p.layers.push(lid);
        p.layer_data.insert(lid, Layer::new(lid, "Background"));

        let json = serde_json::to_string_pretty(&p).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.layers, p.layers);
        assert_eq!(back.layer_data[&lid].name, "Background");
        assert_eq!(back.next_id, p.next_id);
    }

    #[test]
    fn unknown_fields_are_ignored_for_forward_compatibility() {
        // A v1 reader must survive a file written by a later version that
        // added fields. deny_unknown_fields must stay OFF.
        let json = r#"{
            "schema_version": 1,
            "meta": { "name": "X", "created_by": "animus 0.1.0",
                      "created_utc": "2026-08-16T00:00:00Z",
                      "modified_utc": "2026-08-16T00:00:00Z" },
            "next_id": 1,
            "assets": {}, "layers": [], "layer_data": {}, "puppets": {},
            "bindings": [],
            "solver": { "hz": 120, "iterations": 8, "gravity": [0.0, 0.0],
                        "global_damping": 0.98, "max_substeps_per_frame": 8,
                        "enabled": true },
            "stage": { "canvas": [1920, 1080], "background": [0.0,0.0,0.0,1.0] },
            "a_field_from_the_future": 42
        }"#;
        let p: Project = serde_json::from_str(json).expect("must not reject unknown fields");
        assert_eq!(p.meta.name, "X");
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p animus-core doc`
Expected: FAIL — `Project` not found.

- [ ] **Step 3: Write the implementation**

Create the module files. `crates/animus-core/src/doc/mod.rs`:

```rust
//! The document model. This is the single source of truth for a show.
//!
//! Everything the renderer displays is a one-way projection of these
//! types. Nothing ever writes back into them from the scene graph.

mod asset;
mod layer;
mod mesh_puppet;
mod model_puppet;
mod puppet;
mod solver_cfg;
mod stage;

pub use asset::{AssetKind, AssetRef};
pub use layer::{BlendMode, Layer, Transform2Or3};
pub use mesh_puppet::{
    Attachment, AttachmentTable, AutoMeshMode, AutoMeshParams, Bone, Joint, MaterialCfg,
    MeshData, MeshPuppet, MeshSource, SkeletonData,
};
pub use model_puppet::{DrivenJoint, ModelPuppet};
pub use puppet::{Puppet, PuppetKind};
pub use solver_cfg::SolverConfig;
pub use stage::StageConfig;

use crate::ids::*;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Bump on any breaking change to the on-disk format, and add a migration.
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub schema_version: u32,
    pub meta: ProjectMeta,
    pub next_id: u64,
    pub assets: IndexMap<AssetId, AssetRef>,
    /// Paint order. Front of the Vec is the back of the scene.
    pub layers: Vec<LayerId>,
    pub layer_data: IndexMap<LayerId, Layer>,
    pub puppets: IndexMap<PuppetId, Puppet>,
    #[serde(default)]
    pub bindings: Vec<serde_json::Value>, // typed in the signal-bus milestone
    pub solver: SolverConfig,
    pub stage: StageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub created_by: String,
    pub created_utc: String,
    pub modified_utc: String,
}

impl Project {
    pub fn new(name: &str) -> Self {
        let stamp = "1970-01-01T00:00:00Z".to_string(); // callers overwrite; core has no clock
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            meta: ProjectMeta {
                name: name.to_string(),
                created_by: concat!("animus ", env!("CARGO_PKG_VERSION")).to_string(),
                created_utc: stamp.clone(),
                modified_utc: stamp,
            },
            next_id: 1,
            assets: IndexMap::new(),
            layers: Vec::new(),
            layer_data: IndexMap::new(),
            puppets: IndexMap::new(),
            bindings: Vec::new(),
            solver: SolverConfig::default(),
            stage: StageConfig::default(),
        }
    }

    /// Allocate a new never-reused ID and persist the watermark.
    pub fn alloc_id(&mut self) -> u64 {
        let mut alloc = IdAlloc::from_next(self.next_id);
        let id = alloc.next();
        self.next_id = alloc.peek();
        id
    }
}
```

`bindings` is deliberately `Vec<serde_json::Value>` for now — the typed `Binding` lands in the signal-bus milestone, and this keeps files written today loadable then. Note this in a code comment.

Write the remaining modules to match the interface block above and spec §4.3. `crates/animus-core/src/doc/mesh_puppet.rs` holds `MeshData`, `MeshSource`, `AutoMeshParams`, `AutoMeshMode`, `SkeletonData`, `Joint`, `Bone`, `AttachmentTable`, `Attachment`, `MeshPuppet`, `MaterialCfg`. `SolverConfig::default()` returns `hz: 120, iterations: 8, gravity: Vec2::ZERO, global_damping: 0.98, max_substeps_per_frame: 8, enabled: true`. `Layer::new(id, name)` returns a layer with `visible: true, opacity: 1.0, blend: BlendMode::Normal, depth: 0.0` and empty contents.

Add `pub mod doc;` to `lib.rs`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p animus-core doc`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/animus-core/src
git commit -m "feat(core): document model

Project, Layer, Puppet, MeshData, SkeletonData and attachments, with
structure-of-arrays mesh storage and forward-compatible serde."
```

---

## Task 8: Core — `IndexRemap` and `Remappable`

**This is the highest-value task in the plan.** Vertex deletion corrupting attachment and triangle indices is historically the most common serious bug in this class of software. The mechanism here makes forgetting a referrer a *compile error*.

**Files:**
- Create: `crates/animus-core/src/remap.rs`
- Create: `crates/animus-core/src/mesh/mod.rs`, `crates/animus-core/src/mesh/edit.rs`, `crates/animus-core/src/mesh/invariants.rs`
- Create: `crates/animus-core/tests/remap_proptest.rs`
- Modify: `crates/animus-core/src/lib.rs`

**Interfaces:**
- Consumes: `doc::{MeshData, MeshPuppet, AttachmentTable}` from Task 7.
- Produces:
  - `IndexRemap` with `map(&self, old: u32) -> Option<u32>`, `is_deleted(&self, old: u32) -> bool`, `new_len(&self) -> u32`, `old_len(&self) -> u32`, `IndexRemap::from_deletions(old_len: u32, victims: &[u32]) -> Self`
  - `trait Remappable { fn remap_vertices(&mut self, r: &IndexRemap); }`
  - `MeshPuppet::remove_vertices(&mut self, victims: &[u32]) -> IndexRemap` — the **only** public deletion path
  - `MeshPuppet::empty(texture: AssetId) -> Self`
  - `pub enum MeshDefect { .. }` and `pub fn validate(m: &MeshData) -> Vec<MeshDefect>` — non-panicking, reports every defect

- [ ] **Step 1: Write the failing unit tests**

`crates/animus-core/src/remap.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remap_shifts_survivors_down_and_marks_victims() {
        // 5 vertices, delete 1 and 3 -> survivors 0,2,4 become 0,1,2
        let r = IndexRemap::from_deletions(5, &[1, 3]);
        assert_eq!(r.map(0), Some(0));
        assert_eq!(r.map(1), None);
        assert_eq!(r.map(2), Some(1));
        assert_eq!(r.map(3), None);
        assert_eq!(r.map(4), Some(2));
        assert_eq!(r.new_len(), 3);
    }

    #[test]
    fn duplicate_and_unsorted_victims_are_handled() {
        let r = IndexRemap::from_deletions(4, &[3, 1, 1, 3]);
        assert_eq!(r.new_len(), 2);
        assert_eq!(r.map(0), Some(0));
        assert_eq!(r.map(2), Some(1));
    }

    #[test]
    fn out_of_range_victims_are_ignored() {
        let r = IndexRemap::from_deletions(3, &[99]);
        assert_eq!(r.new_len(), 3);
    }

    #[test]
    fn deleting_nothing_is_the_identity() {
        let r = IndexRemap::from_deletions(3, &[]);
        for i in 0..3 {
            assert_eq!(r.map(i), Some(i));
        }
    }
}
```

`crates/animus-core/src/mesh/edit.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::*;
    use crate::ids::BoneId;
    use glam::Vec2;

    fn quad() -> MeshData {
        // 0---1
        // | \ |
        // 2---3
        MeshData {
            positions: vec![
                Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0),
                Vec2::new(0.0, 10.0), Vec2::new(10.0, 10.0),
            ],
            uvs: vec![
                Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0),
                Vec2::new(0.0, 1.0), Vec2::new(1.0, 1.0),
            ],
            triangles: vec![0, 2, 3, 0, 3, 1],
            source: MeshSource::Manual,
        }
    }

    #[test]
    fn deleting_a_vertex_drops_its_triangles_and_reindexes_the_rest() {
        let mut m = quad();
        let r = m.remove_vertices_internal(&[1]);
        assert_eq!(m.positions.len(), 3);
        assert_eq!(m.uvs.len(), 3, "uvs must stay parallel to positions");
        // Triangle [0,3,1] referenced the victim and is gone.
        // Triangle [0,2,3] survives, remapped to [0,1,2].
        assert_eq!(m.triangles, vec![0, 1, 2]);
        assert_eq!(r.new_len(), 3);
    }

    #[test]
    fn attachments_to_deleted_vertices_are_dropped_and_the_rest_reindexed() {
        let mut table = AttachmentTable {
            entries: vec![
                Attachment { vertex: 0, bone: BoneId(1), weight: 1.0, local: Vec2::ZERO },
                Attachment { vertex: 1, bone: BoneId(1), weight: 0.5, local: Vec2::ZERO },
                Attachment { vertex: 3, bone: BoneId(2), weight: 0.7, local: Vec2::ZERO },
            ],
        };
        let r = IndexRemap::from_deletions(4, &[1]);
        table.remap_vertices(&r);

        assert_eq!(table.entries.len(), 2, "the attachment on vertex 1 is gone");
        assert_eq!(table.entries[0].vertex, 0);
        assert_eq!(table.entries[1].vertex, 2, "vertex 3 shifted down to 2");
        assert_eq!(table.entries[1].weight, 0.7, "weights survive unchanged");
    }

    #[test]
    fn remove_vertices_updates_every_referrer_at_once() {
        let mut mp = MeshPuppet::empty(AssetId(1));
        mp.mesh = quad();
        mp.attachments.entries.push(Attachment {
            vertex: 3, bone: BoneId(1), weight: 1.0, local: Vec2::ZERO,
        });
        mp.remove_vertices(&[0]);
        assert_eq!(mp.mesh.positions.len(), 3);
        assert_eq!(mp.attachments.entries[0].vertex, 2, "3 shifted down to 2");
        for t in &mp.mesh.triangles {
            assert!((*t as usize) < mp.mesh.positions.len(), "no dangling triangle index");
        }
    }
}
```

`MeshPuppet::empty(texture: AssetId) -> Self` is a real (**not** `#[cfg(test)]`) constructor in `mesh_puppet.rs`, producing a puppet with an empty mesh, empty skeleton and empty attachments. It must be public and non-test-gated because the integration test in Step 6 cannot see `#[cfg(test)]` items.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p animus-core remap`
Expected: FAIL — `IndexRemap` not found.

- [ ] **Step 3: Implement `IndexRemap` and `Remappable`**

```rust
//! Vertex index remapping.
//!
//! Deleting mesh vertices invalidates every stored vertex index in the
//! puppet — triangles, attachments, selection, pins. `IndexRemap` is the
//! single object that describes such a deletion, and `Remappable` is how
//! every referrer applies it.
//!
//! The safety property is enforced in `MeshPuppet::remove_vertices`,
//! which destructures `Self` exhaustively **without `..`**. Adding a new
//! field that stores vertex indices will fail to compile until it is
//! handled there. Do not add `..` to that destructuring.

/// Describes a vertex deletion: which old indices survive, and where they moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRemap {
    old_to_new: Vec<Option<u32>>,
    new_len: u32,
}

impl IndexRemap {
    /// Build a remap for deleting `victims` from a mesh of `old_len` vertices.
    /// Victims may be unsorted, duplicated, or out of range.
    pub fn from_deletions(old_len: u32, victims: &[u32]) -> Self {
        let mut doomed = vec![false; old_len as usize];
        for &v in victims {
            if (v as usize) < doomed.len() {
                doomed[v as usize] = true;
            }
        }
        let mut old_to_new = Vec::with_capacity(old_len as usize);
        let mut next = 0u32;
        for is_doomed in doomed {
            if is_doomed {
                old_to_new.push(None);
            } else {
                old_to_new.push(Some(next));
                next += 1;
            }
        }
        Self { old_to_new, new_len: next }
    }

    /// The new index for an old one, or `None` if it was deleted.
    pub fn map(&self, old: u32) -> Option<u32> {
        self.old_to_new.get(old as usize).copied().flatten()
    }

    pub fn is_deleted(&self, old: u32) -> bool {
        self.map(old).is_none()
    }

    /// Vertex count after the deletion.
    pub fn new_len(&self) -> u32 {
        self.new_len
    }

    pub fn old_len(&self) -> u32 {
        self.old_to_new.len() as u32
    }
}

/// Implemented by every type that stores a vertex index.
pub trait Remappable {
    fn remap_vertices(&mut self, r: &IndexRemap);
}
```

- [ ] **Step 4: Implement `Remappable` for the referrers and the deletion path**

In `mesh/edit.rs`:

```rust
use crate::doc::{AttachmentTable, MeshData, MeshPuppet};
use crate::remap::{IndexRemap, Remappable};

impl MeshData {
    /// Private: use `MeshPuppet::remove_vertices`, which also updates
    /// every other referrer. Calling this alone leaves attachments dangling.
    pub(crate) fn remove_vertices_internal(&mut self, victims: &[u32]) -> IndexRemap {
        let r = IndexRemap::from_deletions(self.positions.len() as u32, victims);

        let mut positions = Vec::with_capacity(r.new_len() as usize);
        let mut uvs = Vec::with_capacity(r.new_len() as usize);
        for old in 0..r.old_len() {
            if r.map(old).is_some() {
                positions.push(self.positions[old as usize]);
                uvs.push(self.uvs[old as usize]);
            }
        }
        self.positions = positions;
        self.uvs = uvs;

        // A triangle touching a deleted vertex is dropped, never repaired.
        let mut tris = Vec::with_capacity(self.triangles.len());
        for tri in self.triangles.chunks_exact(3) {
            match (r.map(tri[0]), r.map(tri[1]), r.map(tri[2])) {
                (Some(a), Some(b), Some(c)) => tris.extend_from_slice(&[a, b, c]),
                _ => {}
            }
        }
        self.triangles = tris;

        r
    }
}

impl Remappable for AttachmentTable {
    fn remap_vertices(&mut self, r: &IndexRemap) {
        self.entries.retain_mut(|a| match r.map(a.vertex) {
            Some(new) => {
                a.vertex = new;
                true
            }
            None => false,
        });
    }
}

impl MeshPuppet {
    /// The ONLY public way to delete vertices.
    ///
    /// The destructuring below is exhaustive **on purpose**. Do not add
    /// `..`. If you add a field that stores vertex indices, this stops
    /// compiling until you handle it — which is the entire point.
    pub fn remove_vertices(&mut self, victims: &[u32]) -> IndexRemap {
        let MeshPuppet {
            texture: _,
            mesh,
            skeleton: _,           // stores JointIds and BoneIds, never vertex indices
            attachments,
            material: _,
            solver_override: _,
        } = self;

        let r = mesh.remove_vertices_internal(victims);
        attachments.remap_vertices(&r);
        r
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p animus-core`
Expected: all unit tests pass.

- [ ] **Step 6: Write the property test**

`crates/animus-core/tests/remap_proptest.rs`:

```rust
//! Property tests for vertex deletion.
//!
//! This is the highest-value test suite in the project: a dangling vertex
//! index is silent corruption that surfaces as a crash or garbled mesh
//! long after the edit that caused it.

use animus_core::doc::*;
use animus_core::ids::{AssetId, BoneId};
use glam::Vec2;
use proptest::prelude::*;

fn arb_mesh() -> impl Strategy<Value = MeshData> {
    (3usize..40).prop_flat_map(|n| {
        let positions = prop::collection::vec(
            (0.0f32..1000.0, 0.0f32..1000.0).prop_map(|(x, y)| Vec2::new(x, y)),
            n,
        );
        let tris = prop::collection::vec(
            (0..n as u32, 0..n as u32, 0..n as u32),
            0..60,
        );
        (positions, tris, Just(n)).prop_map(|(positions, tris, n)| {
            let uvs = positions.iter().map(|p| *p / 1000.0).collect();
            let triangles = tris
                .into_iter()
                // reject degenerate triangles up front; they are not what
                // this test is about
                .filter(|(a, b, c)| a != b && b != c && a != c)
                .flat_map(|(a, b, c)| [a, b, c])
                .collect();
            let _ = n;
            MeshData { positions, uvs, triangles, source: MeshSource::Manual }
        })
    })
}

fn arb_puppet() -> impl Strategy<Value = MeshPuppet> {
    arb_mesh().prop_flat_map(|mesh| {
        let n = mesh.positions.len() as u32;
        let atts = prop::collection::vec(
            (0..n, 1u64..5, 0.0f32..1.0),
            0..30,
        );
        (Just(mesh), atts).prop_map(|(mesh, atts)| {
            let mut mp = MeshPuppet::empty(AssetId(1));
            mp.mesh = mesh;
            mp.attachments.entries = atts
                .into_iter()
                .map(|(v, b, w)| Attachment {
                    vertex: v, bone: BoneId(b), weight: w, local: Vec2::ZERO,
                })
                .collect();
            mp
        })
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// After any deletion, nothing may reference a vertex that no longer exists.
    #[test]
    fn no_dangling_indices_after_deletion(
        mut mp in arb_puppet(),
        victims in prop::collection::vec(0u32..40, 0..10),
    ) {
        let n_before = mp.mesh.positions.len() as u32;
        let victims: Vec<u32> = victims.into_iter().filter(|v| *v < n_before).collect();

        mp.remove_vertices(&victims);
        let n_after = mp.mesh.positions.len() as u32;

        prop_assert_eq!(mp.mesh.uvs.len() as u32, n_after, "uvs stayed parallel");
        prop_assert_eq!(mp.mesh.triangles.len() % 3, 0, "triangles stayed whole");

        for t in &mp.mesh.triangles {
            prop_assert!(*t < n_after, "triangle index {} >= {}", t, n_after);
        }
        for a in &mp.attachments.entries {
            prop_assert!(a.vertex < n_after, "attachment vertex {} >= {}", a.vertex, n_after);
        }
    }

    /// Surviving vertices keep their positions. This catches an off-by-one
    /// in the compaction loop, which a dangling-index check alone would miss.
    #[test]
    fn survivors_keep_their_data(
        mut mp in arb_puppet(),
        victim in 0u32..40,
    ) {
        let n = mp.mesh.positions.len() as u32;
        prop_assume!(victim < n);

        let expected: Vec<Vec2> = mp.mesh.positions.iter().enumerate()
            .filter(|(i, _)| *i as u32 != victim)
            .map(|(_, p)| *p)
            .collect();

        mp.remove_vertices(&[victim]);
        prop_assert_eq!(mp.mesh.positions, expected);
    }

    /// Deleting {a, b} in one call equals deleting {a, b} in the other order.
    #[test]
    fn deletion_is_order_independent(
        mp in arb_puppet(),
        a in 0u32..40,
        b in 0u32..40,
    ) {
        let n = mp.mesh.positions.len() as u32;
        prop_assume!(a < n && b < n && a != b);

        let mut x = mp.clone();
        x.remove_vertices(&[a, b]);
        let mut y = mp.clone();
        y.remove_vertices(&[b, a]);

        prop_assert_eq!(x.mesh.positions, y.mesh.positions);
        prop_assert_eq!(x.mesh.triangles, y.mesh.triangles);
        prop_assert_eq!(
            x.attachments.entries.len(),
            y.attachments.entries.len()
        );
    }
}
```

- [ ] **Step 7: Run the property tests**

Run: `cargo test -p animus-core --test remap_proptest`
Expected: 3 passed, 500 cases each.

If proptest finds a counterexample it writes `crates/animus-core/tests/remap_proptest.proptest-regressions`. **Commit that file** — it makes the found case a permanent regression test.

- [ ] **Step 8: Verify the compile-time safety property actually works**

This is the step that proves the mechanism, not just the code. Temporarily add a field to `MeshPuppet`:

```rust
pub selection: Vec<u32>,
```

Run `cargo build -p animus-core`.

**Expected: a compile error in `remove_vertices` — `pattern does not mention field 'selection'`.** If it compiles, the destructuring has a `..` in it and the entire safety property is void. Fix it before continuing.

Then remove the temporary field and rebuild.

- [ ] **Step 9: Write the failing tests for `validate()`**

Spec §3.2 lists `mesh/invariants.rs`. It is the runtime counterpart to the property tests: it checks a mesh that arrived from disk, from a migration, or from a user edit, and reports every problem rather than panicking.

`crates/animus-core/src/mesh/invariants.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::*;
    use glam::Vec2;

    fn ok_mesh() -> MeshData {
        MeshData {
            positions: vec![Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0)],
            uvs: vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)],
            triangles: vec![0, 1, 2],
            source: MeshSource::Manual,
        }
    }

    #[test]
    fn a_well_formed_mesh_has_no_defects() {
        assert!(validate(&ok_mesh()).is_empty());
    }

    #[test]
    fn a_dangling_triangle_index_is_reported() {
        let mut m = ok_mesh();
        m.triangles = vec![0, 1, 99];
        assert!(validate(&m).iter().any(|d| matches!(
            d, MeshDefect::TriangleIndexOutOfRange { index: 99, .. }
        )));
    }

    #[test]
    fn uvs_that_are_not_parallel_to_positions_are_reported() {
        let mut m = ok_mesh();
        m.uvs.pop();
        assert!(validate(&m).iter().any(|d| matches!(d, MeshDefect::UvCountMismatch { .. })));
    }

    #[test]
    fn a_triangle_list_that_is_not_a_multiple_of_three_is_reported() {
        let mut m = ok_mesh();
        m.triangles = vec![0, 1];
        assert!(validate(&m).iter().any(|d| matches!(d, MeshDefect::RaggedTriangleList { len: 2 })));
    }

    #[test]
    fn a_degenerate_triangle_is_reported() {
        let mut m = ok_mesh();
        m.positions[2] = Vec2::new(20.0, 0.0);   // all three collinear
        assert!(validate(&m).iter().any(|d| matches!(d, MeshDefect::DegenerateTriangle { .. })));
    }

    #[test]
    fn a_non_finite_position_is_reported() {
        let mut m = ok_mesh();
        m.positions[0] = Vec2::new(f32::NAN, 0.0);
        assert!(validate(&m).iter().any(|d| matches!(d, MeshDefect::NonFinitePosition { vertex: 0 })));
    }

    #[test]
    fn validate_reports_every_defect_rather_than_stopping_at_the_first() {
        let mut m = ok_mesh();
        m.triangles = vec![0, 1, 99, 0, 1, 98];
        assert_eq!(validate(&m).len(), 2);
    }
}
```

- [ ] **Step 10: Run the tests to verify they fail**

Run: `cargo test -p animus-core invariants`
Expected: FAIL — `validate` not found.

- [ ] **Step 11: Implement `validate()`**

```rust
//! Mesh integrity checks.
//!
//! `validate` never panics and never stops at the first problem — it
//! returns every defect it finds, so the UI can show a complete report
//! for a mesh that arrived from disk, from a migration, or from an edit.

#[derive(Debug, Clone, PartialEq)]
pub enum MeshDefect {
    UvCountMismatch { positions: usize, uvs: usize },
    RaggedTriangleList { len: usize },
    TriangleIndexOutOfRange { triangle: usize, index: u32, vertex_count: usize },
    DegenerateTriangle { triangle: usize, area: f32 },
    NonFinitePosition { vertex: usize },
    NonFiniteUv { vertex: usize },
}

pub fn validate(m: &MeshData) -> Vec<MeshDefect> { /* checks in the order above */ }
```

Degeneracy threshold: `|perp_dot| / 2.0 < 1e-3`. Skip the degeneracy and area checks for any triangle that already has an out-of-range index, to avoid a cascade of noise from one real problem.

- [ ] **Step 12: Run the tests to verify they pass**

Run: `cargo test -p animus-core invariants`
Expected: 7 passed.

- [ ] **Step 13: Commit**

```bash
git add crates/animus-core/src crates/animus-core/tests
git commit -m "feat(core): vertex deletion safe by construction

IndexRemap + Remappable, with MeshPuppet::remove_vertices destructuring
Self exhaustively so a new index-holding field fails to compile until it
is handled. Backed by 500-case property tests for dangling indices,
survivor data, and order independence, plus a non-panicking validate()
that reports every mesh defect at once."
```

---

## Task 9: Core — the solver

**Files:**
- Create: `crates/animus-core/src/solver/mod.rs`, `state.rs`, `compiled.rs`, `step.rs`, `guard.rs`
- Create: `crates/animus-core/tests/solver_golden.rs`
- Modify: `crates/animus-core/src/lib.rs`

**Interfaces:**
- Consumes: `doc::{SkeletonData, Joint, Bone, SolverConfig}` from Task 7.
- Produces:
  - `CompiledRig::build(skel: &SkeletonData, cfg: &SolverConfig) -> CompiledRig` — immutable, `Send + Sync`, shared by `Arc` into the ECS
  - `CompiledRig::joint_index(&self, id: JointId) -> Option<u32>`
  - `SolverState::rest(rig: &CompiledRig) -> SolverState`
  - `SolverState::set_target(&mut self, joint: u32, pos: Vec2)`, `clear_target(&mut self, joint: u32)`
  - `SolverState::positions(&self) -> &[Vec2]`, `prev_tick_positions(&self) -> &[Vec2]`
  - `SolverState::reset_to_rest(&mut self, rig: &CompiledRig)`
  - `solver::step(rig: &CompiledRig, st: &mut SolverState, dt: f32) -> StepOutcome` where `StepOutcome` is `Ok` or `ResetDueToNonFinite`

- [ ] **Step 1: Write the failing tests**

`crates/animus-core/src/solver/step.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::*;
    use crate::ids::{BoneId, JointId};
    use crate::solver::{CompiledRig, SolverState};
    use approx::assert_relative_eq;
    use glam::Vec2;

    /// Two joints 100px apart, joint 0 pinned. Bone rest length 100.
    fn two_joint_rig(stiffness: f32) -> (SkeletonData, SolverConfig) {
        let mut skel = SkeletonData::default();
        skel.joints.insert(JointId(1), Joint {
            id: JointId(1), name: "root".into(),
            rest: Vec2::new(0.0, 0.0), rest_angle: 0.0,
            inv_mass: 0.0, pinned: true,
        });
        skel.joints.insert(JointId(2), Joint {
            id: JointId(2), name: "tip".into(),
            rest: Vec2::new(100.0, 0.0), rest_angle: 0.0,
            inv_mass: 1.0, pinned: false,
        });
        skel.bones.insert(BoneId(1), Bone {
            id: BoneId(1), name: "bone".into(),
            a: JointId(1), b: JointId(2),
            rest_length: None, stiffness, damping: 0.0,
            length_mul: 1.0, attach_radius: 20.0,
        });
        let cfg = SolverConfig { gravity: Vec2::ZERO, global_damping: 1.0, ..Default::default() };
        (skel, cfg)
    }

    #[test]
    fn rest_state_is_stationary() {
        let (skel, cfg) = two_joint_rig(1.0);
        let rig = CompiledRig::build(&skel, &cfg);
        let mut st = SolverState::rest(&rig);
        let before = st.positions().to_vec();
        for _ in 0..100 { step(&rig, &mut st, 1.0 / 120.0); }
        for (a, b) in before.iter().zip(st.positions()) {
            assert_relative_eq!(a.x, b.x, epsilon = 1e-4);
            assert_relative_eq!(a.y, b.y, epsilon = 1e-4);
        }
    }

    #[test]
    fn rest_length_is_derived_from_rest_positions_when_none() {
        let (skel, cfg) = two_joint_rig(1.0);
        let rig = CompiledRig::build(&skel, &cfg);
        assert_relative_eq!(rig.rest_length(0), 100.0, epsilon = 1e-4);
    }

    #[test]
    fn a_pinned_joint_never_moves() {
        let (skel, cfg) = two_joint_rig(1.0);
        let rig = CompiledRig::build(&skel, &cfg);
        let mut st = SolverState::rest(&rig);
        // Yank the free joint far away.
        st.set_target(1, Vec2::new(5000.0, 5000.0));
        for _ in 0..50 { step(&rig, &mut st, 1.0 / 120.0); }
        assert_relative_eq!(st.positions()[0].x, 0.0, epsilon = 1e-4);
        assert_relative_eq!(st.positions()[0].y, 0.0, epsilon = 1e-4);
    }

    #[test]
    fn a_stretched_bone_relaxes_back_toward_its_rest_length() {
        let (skel, cfg) = two_joint_rig(1.0);
        let rig = CompiledRig::build(&skel, &cfg);
        let mut st = SolverState::rest(&rig);
        st.displace(1, Vec2::new(200.0, 0.0));   // now 300 apart
        let d0 = (st.positions()[1] - st.positions()[0]).length();
        for _ in 0..200 { step(&rig, &mut st, 1.0 / 120.0); }
        let d1 = (st.positions()[1] - st.positions()[0]).length();
        assert!(d1 < d0, "bone should contract: {d0} -> {d1}");
        assert_relative_eq!(d1, 100.0, epsilon = 2.0);
    }

    #[test]
    fn low_stiffness_relaxes_more_slowly_than_high_stiffness() {
        // The looseness IS the organic feel. Guard it against a future
        // "optimization" that makes the solver rigid.
        let mut lengths = vec![];
        for stiffness in [0.2f32, 1.0f32] {
            let (skel, cfg) = two_joint_rig(stiffness);
            let rig = CompiledRig::build(&skel, &cfg);
            let mut st = SolverState::rest(&rig);
            st.displace(1, Vec2::new(200.0, 0.0));
            for _ in 0..10 { step(&rig, &mut st, 1.0 / 120.0); }
            lengths.push((st.positions()[1] - st.positions()[0]).length());
        }
        assert!(lengths[0] > lengths[1],
            "soft bone must still be longer after 10 steps: {lengths:?}");
    }

    #[test]
    fn length_mul_changes_the_target_length() {
        let (mut skel, cfg) = two_joint_rig(1.0);
        skel.bones.get_mut(&BoneId(1)).unwrap().length_mul = 1.5;
        let rig = CompiledRig::build(&skel, &cfg);
        let mut st = SolverState::rest(&rig);
        for _ in 0..400 { step(&rig, &mut st, 1.0 / 120.0); }
        let d = (st.positions()[1] - st.positions()[0]).length();
        assert_relative_eq!(d, 150.0, epsilon = 2.0);
    }

    #[test]
    fn a_non_finite_state_resets_the_puppet_instead_of_propagating() {
        let (skel, cfg) = two_joint_rig(1.0);
        let rig = CompiledRig::build(&skel, &cfg);
        let mut st = SolverState::rest(&rig);
        st.poison_for_test(1, Vec2::new(f32::NAN, 0.0));
        let outcome = step(&rig, &mut st, 1.0 / 120.0);
        assert_eq!(outcome, StepOutcome::ResetDueToNonFinite);
        assert!(st.positions().iter().all(|p| p.is_finite()));
        assert_relative_eq!(st.positions()[1].x, 100.0, epsilon = 1e-4);
    }

    #[test]
    fn a_zero_length_bone_does_not_produce_nan() {
        let (mut skel, cfg) = two_joint_rig(1.0);
        skel.joints.get_mut(&JointId(2)).unwrap().rest = Vec2::ZERO;  // coincident
        let rig = CompiledRig::build(&skel, &cfg);
        let mut st = SolverState::rest(&rig);
        for _ in 0..50 { step(&rig, &mut st, 1.0 / 120.0); }
        assert!(st.positions().iter().all(|p| p.is_finite()));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p animus-core solver`
Expected: FAIL — `CompiledRig` not found.

- [ ] **Step 3: Implement `CompiledRig`**

`solver/compiled.rs`. Flattens the `IndexMap`-based `SkeletonData` into dense parallel arrays, in **deterministic order** (`IndexMap` insertion order), so the golden test is meaningful and `par_iter_mut` cannot introduce ordering effects.

```rust
/// Immutable, dense form of a skeleton, built once and shared by `Arc`.
#[derive(Debug, Clone)]
pub struct CompiledRig {
    pub(crate) rest: Vec<Vec2>,
    pub(crate) inv_mass: Vec<f32>,
    pub(crate) pinned: Vec<bool>,
    pub(crate) bone_a: Vec<u32>,
    pub(crate) bone_b: Vec<u32>,
    pub(crate) rest_length: Vec<f32>,
    pub(crate) stiffness: Vec<f32>,
    pub(crate) length_mul: Vec<f32>,
    pub(crate) gravity: Vec2,
    pub(crate) damping: f32,
    pub(crate) iterations: u32,
    joint_index: HashMap<JointId, u32>,
}
```

`build` walks `skel.joints` in order assigning dense indices, then walks `skel.bones` in order. `rest_length[i]` is `bone.rest_length.unwrap_or_else(|| (rest[b] - rest[a]).length())`. A bone whose endpoints are not in the joint map is skipped with a `tracing::warn!`. Provide `rest_length(&self, bone: usize) -> f32` and `joint_index(&self, id: JointId) -> Option<u32>`.

- [ ] **Step 4: Implement `SolverState`**

`solver/state.rs`. Structure-of-arrays, allocation-free per step:

```rust
pub struct SolverState {
    pos: Vec<Vec2>,
    prev: Vec<Vec2>,          // Verlet's previous position
    prev_tick: Vec<Vec2>,     // position at the previous FIXED tick, for display lerp
    target: Vec<Option<Vec2>>,
}
```

`prev_tick` is deliberately distinct from `prev`: `prev` is the integrator's history, `prev_tick` is what the renderer interpolates from. Conflating them makes the interpolation subtly wrong. Note this in a comment.

`rest(rig)` sets `pos = prev = prev_tick = rig.rest` and clears targets. `displace(i, d)` adds to `pos[i]` **and** `prev[i]` so it does not inject velocity — the tests depend on that. `poison_for_test` is `#[cfg(any(test, feature = "test-util"))]`.

- [ ] **Step 5: Implement `step`**

`solver/step.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcome { Ok, ResetDueToNonFinite }

pub fn step(rig: &CompiledRig, st: &mut SolverState, dt: f32) -> StepOutcome {
    let n = rig.rest.len();

    // 1. Verlet integrate.
    for i in 0..n {
        st.prev_tick[i] = st.pos[i];
        if rig.pinned[i] || rig.inv_mass[i] == 0.0 {
            st.prev[i] = st.pos[i];
            continue;
        }
        let vel = (st.pos[i] - st.prev[i]) * rig.damping;
        st.prev[i] = st.pos[i];
        st.pos[i] += vel + rig.gravity * dt * dt;
    }

    // 2. Apply driven targets (from the mouse or the signal bus).
    for i in 0..n {
        if let Some(t) = st.target[i] {
            st.pos[i] = t;
            st.prev[i] = t;
        }
    }

    // 3. Gauss-Seidel relaxation. Bone order is stable by construction.
    //    Incomplete convergence at these iteration counts is the organic
    //    feel — do NOT raise iterations to "fix" softness.
    for _ in 0..rig.iterations {
        for b in 0..rig.bone_a.len() {
            let (ia, ib) = (rig.bone_a[b] as usize, rig.bone_b[b] as usize);
            let d = st.pos[ib] - st.pos[ia];
            let len = d.length();
            if len < 1e-6 { continue; }        // coincident joints: no direction
            let target = rig.rest_length[b] * rig.length_mul[b];
            let err = (len - target) / len;
            let wa = if rig.pinned[ia] || st.target[ia].is_some() { 0.0 } else { rig.inv_mass[ia] };
            let wb = if rig.pinned[ib] || st.target[ib].is_some() { 0.0 } else { rig.inv_mass[ib] };
            let wsum = wa + wb;
            if wsum <= 0.0 { continue; }
            let corr = d * err * rig.stiffness[b];
            st.pos[ia] += corr * (wa / wsum);
            st.pos[ib] -= corr * (wb / wsum);
        }
    }

    // 4. Guard. A single non-finite value resets THIS puppet only.
    if !st.pos.iter().all(|p| p.is_finite()) {
        st.reset_to_rest(rig);
        return StepOutcome::ResetDueToNonFinite;
    }
    StepOutcome::Ok
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p animus-core solver`
Expected: 8 passed.

- [ ] **Step 7: Write the determinism and golden tests**

`crates/animus-core/tests/solver_golden.rs`:

```rust
//! Determinism and regression guards for the solver.
//!
//! Bit-exact cross-platform float equality is not portable, so we assert
//! two things instead: (1) the same input twice gives bit-identical output
//! on THIS machine, which catches iteration-order and HashMap-ordering
//! bugs; (2) a committed set of positions is reproduced within a tight
//! tolerance, which catches accidental changes to the physics.

use animus_core::doc::*;
use animus_core::ids::{BoneId, JointId};
use animus_core::solver::{step, CompiledRig, SolverState};
use glam::Vec2;

/// A 12-joint chain with a pinned root, deterministically constructed.
fn chain_rig(n: u32) -> (SkeletonData, SolverConfig) {
    let mut skel = SkeletonData::default();
    for i in 0..n {
        skel.joints.insert(JointId((i + 1) as u64), Joint {
            id: JointId((i + 1) as u64),
            name: format!("j{i}"),
            rest: Vec2::new(i as f32 * 40.0, 0.0),
            rest_angle: 0.0,
            inv_mass: if i == 0 { 0.0 } else { 1.0 },
            pinned: i == 0,
        });
    }
    for i in 0..n - 1 {
        skel.bones.insert(BoneId((i + 1) as u64), Bone {
            id: BoneId((i + 1) as u64),
            name: format!("b{i}"),
            a: JointId((i + 1) as u64),
            b: JointId((i + 2) as u64),
            rest_length: None,
            stiffness: 0.8,
            damping: 0.0,
            length_mul: 1.0,
            attach_radius: 25.0,
        });
    }
    let cfg = SolverConfig {
        gravity: Vec2::new(0.0, 980.0),
        global_damping: 0.98,
        iterations: 8,
        ..Default::default()
    };
    (skel, cfg)
}

fn run(ticks: usize) -> Vec<Vec2> {
    let (skel, cfg) = chain_rig(12);
    let rig = CompiledRig::build(&skel, &cfg);
    let mut st = SolverState::rest(&rig);
    for t in 0..ticks {
        // A reproducible driving signal, no RNG.
        let phase = t as f32 * 0.05;
        st.set_target(0, Vec2::new(phase.sin() * 30.0, phase.cos() * 15.0));
        step(&rig, &mut st, 1.0 / 120.0);
    }
    st.positions().to_vec()
}

#[test]
fn the_solver_is_deterministic() {
    let a = run(600);
    let b = run(600);
    assert_eq!(a, b, "identical input must give bit-identical output");
}

#[test]
fn the_solver_stays_finite_and_bounded_over_a_long_run() {
    let p = run(20_000);
    assert!(p.iter().all(|v| v.is_finite()), "no NaN or Inf after 20k ticks");
    // A 12-joint chain of 40px bones cannot legitimately reach 10_000px.
    assert!(p.iter().all(|v| v.length() < 10_000.0), "no explosion: {p:?}");
}

#[test]
fn a_violent_yank_does_not_destabilise_the_rig() {
    let (skel, cfg) = chain_rig(12);
    let rig = CompiledRig::build(&skel, &cfg);
    let mut st = SolverState::rest(&rig);
    // Simulate a performer flinging a handle across the stage in one frame.
    st.set_target(0, Vec2::new(100_000.0, -100_000.0));
    step(&rig, &mut st, 1.0 / 120.0);
    st.clear_target(0);
    for _ in 0..2000 { step(&rig, &mut st, 1.0 / 120.0); }
    assert!(st.positions().iter().all(|p| p.is_finite()));
}

#[test]
fn golden_positions_are_unchanged() {
    // Regenerate deliberately, never casually: any diff here means the
    // physics changed and every existing show will move differently.
    let got = run(600);
    let want = include_str!("fixtures/solver_golden_600.json");
    let want: Vec<[f32; 2]> = serde_json::from_str(want).unwrap();
    assert_eq!(got.len(), want.len());
    for (i, (g, w)) in got.iter().zip(&want).enumerate() {
        assert!(
            (g.x - w[0]).abs() < 1e-3 && (g.y - w[1]).abs() < 1e-3,
            "joint {i} moved: got {g:?}, want {w:?}"
        );
    }
}
```

Generate `crates/animus-core/tests/fixtures/solver_golden_600.json` by running `run(600)` once and serializing the result. **Read the numbers before committing them** — if the chain has not settled into something plausible under gravity, the solver is wrong and you would be enshrining the bug.

Add `serde_json` to `animus-core`'s `[dev-dependencies]` (already there from Task 1).

- [ ] **Step 8: Run the golden tests**

Run: `cargo test -p animus-core --test solver_golden`
Expected: 4 passed.

- [ ] **Step 9: Commit**

```bash
git add crates/animus-core/src/solver crates/animus-core/tests
git commit -m "feat(core): position-based Verlet solver over joints

Gauss-Seidel distance-constraint relaxation with stable bone ordering,
per-puppet NaN guard that resets rather than propagates, and golden
tests covering determinism, long-run stability and violent input."
```

---

## Task 10: Core — silhouette extraction

**Files:**
- Create: `crates/animus-core/src/silhouette/mod.rs`, `alpha.rs`, `marching.rs`, `rdp.rs`, `topology.rs`, `fallback.rs`
- Create: `crates/animus-core/tests/fixtures/images/` — `blob.png`, `blob_with_hole.png`, `two_islands.png`, `fully_opaque.png`, `fully_transparent.png`, `one_pixel.png`, `antialiased_edge.png`
- Modify: `crates/animus-core/src/lib.rs`

**Interfaces:**
- Consumes: `doc::AutoMeshParams` from Task 7.
- Produces:
  - `pub struct Ring { pub points: Vec<Vec2>, pub is_hole: bool }`
  - `pub fn extract(img: &image::RgbaImage, params: &AutoMeshParams) -> Result<Vec<Ring>, SilhouetteError>`
  - `pub fn convex_hull_ring(img: &image::RgbaImage, threshold: u8) -> Ring`
  - `pub fn bounding_box_ring(img: &image::RgbaImage, threshold: u8) -> Ring`
  - Rings are CCW for outer boundaries and CW for holes, in image space (Y down).

- [ ] **Step 1: Create the test fixture images**

Write a small `#[test]`-gated helper or an `xtask` command that generates the fixtures programmatically, so they are reproducible rather than mystery binaries:

- `blob.png` — 200×200, a filled circle radius 70 at centre, hard alpha edge
- `blob_with_hole.png` — the same circle with a radius-25 transparent hole at centre
- `two_islands.png` — two disjoint 40×40 squares, 60 px apart
- `fully_opaque.png` — 64×64, alpha 255 everywhere
- `fully_transparent.png` — 64×64, alpha 0 everywhere
- `one_pixel.png` — 1×1 opaque
- `antialiased_edge.png` — the blob with a 3 px alpha gradient at its edge and scattered alpha-1..8 speckle outside it

- [ ] **Step 2: Write the failing tests**

`crates/animus-core/src/silhouette/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{AutoMeshMode, AutoMeshParams};

    fn params() -> AutoMeshParams {
        AutoMeshParams {
            alpha_threshold: 8,
            close_radius: 2,
            rdp_epsilon_px: 2.0,
            min_region_area_px: 64.0,
            interior_spacing_px: 40.0,
            mode: AutoMeshMode::Silhouette,
        }
    }

    fn load(name: &str) -> image::RgbaImage {
        let path = format!("{}/tests/fixtures/images/{name}", env!("CARGO_MANIFEST_DIR"));
        image::open(path).unwrap().to_rgba8()
    }

    #[test]
    fn a_simple_blob_gives_one_outer_ring() {
        let rings = extract(&load("blob.png"), &params()).unwrap();
        assert_eq!(rings.len(), 1);
        assert!(!rings[0].is_hole);
        assert!(rings[0].points.len() >= 8, "a circle needs more than a few points");
    }

    #[test]
    fn a_blob_with_a_hole_gives_an_outer_ring_and_a_hole() {
        let rings = extract(&load("blob_with_hole.png"), &params()).unwrap();
        assert_eq!(rings.len(), 2);
        assert_eq!(rings.iter().filter(|r| !r.is_hole).count(), 1);
        assert_eq!(rings.iter().filter(|r| r.is_hole).count(), 1);
    }

    #[test]
    fn two_islands_give_two_outer_rings() {
        let rings = extract(&load("two_islands.png"), &params()).unwrap();
        assert_eq!(rings.iter().filter(|r| !r.is_hole).count(), 2);
    }

    #[test]
    fn a_fully_opaque_image_gives_a_ring_around_the_whole_frame() {
        let rings = extract(&load("fully_opaque.png"), &params()).unwrap();
        assert_eq!(rings.len(), 1);
        let area = signed_area(&rings[0].points).abs();
        assert!(area > 64.0 * 64.0 * 0.9, "area {area} should be close to 4096");
    }

    #[test]
    fn a_fully_transparent_image_is_an_error_not_a_panic() {
        let err = extract(&load("fully_transparent.png"), &params()).unwrap_err();
        assert!(matches!(err, SilhouetteError::NoOpaqueRegion));
    }

    #[test]
    fn a_one_pixel_image_does_not_panic() {
        // Below min_region_area_px, so it is treated as having no usable region.
        let r = extract(&load("one_pixel.png"), &params());
        assert!(r.is_err() || r.unwrap().is_empty());
    }

    #[test]
    fn the_closing_pass_removes_antialiasing_speckle() {
        let img = load("antialiased_edge.png");
        let with = extract(&img, &params()).unwrap();

        let mut p = params();
        p.close_radius = 0;
        p.min_region_area_px = 0.0;
        let without = extract(&img, &p).unwrap();

        assert!(with.len() < without.len(),
            "closing must merge speckle: {} rings with, {} without",
            with.len(), without.len());
    }

    #[test]
    fn outer_rings_are_ccw_and_holes_are_cw() {
        let rings = extract(&load("blob_with_hole.png"), &params()).unwrap();
        for r in &rings {
            let a = signed_area(&r.points);
            if r.is_hole {
                assert!(a < 0.0, "hole must be CW, got area {a}");
            } else {
                assert!(a > 0.0, "outer must be CCW, got area {a}");
            }
        }
    }

    #[test]
    fn rdp_simplification_reduces_point_count_without_losing_the_shape() {
        let mut coarse = params();
        coarse.rdp_epsilon_px = 8.0;
        let fine = params();

        let c = extract(&load("blob.png"), &coarse).unwrap();
        let f = extract(&load("blob.png"), &fine).unwrap();

        assert!(c[0].points.len() < f[0].points.len());
        let (ca, fa) = (signed_area(&c[0].points).abs(), signed_area(&f[0].points).abs());
        assert!((ca - fa).abs() / fa < 0.10, "area changed by more than 10%: {ca} vs {fa}");
    }
}
```

`signed_area(&[Vec2]) -> f32` is the shoelace formula, `pub` in `topology.rs` because the triangulation task needs it too.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p animus-core silhouette`
Expected: FAIL — `extract` not found.

- [ ] **Step 4: Implement the alpha mask and closing pass**

`silhouette/alpha.rs`: build a `image::GrayImage` where a pixel is 255 if `rgba.a >= threshold` else 0. Then `imageproc::morphology::dilate` followed by `erode`, both with `Norm::LInf` and distance `close_radius` — dilate first, then erode by the same amount. **That order is a closing, which fills small gaps and merges speckle into the body. Erode-then-dilate is an opening, which does the opposite and is a real bug if you get it backwards.** Skip both when `close_radius == 0`.

- [ ] **Step 5: Implement marching squares**

`silhouette/marching.rs`: standard marching squares over the binary mask, tracing closed rings at pixel-corner coordinates. Handle the ambiguous saddle cases (5 and 10) consistently — pick one resolution and comment it, because inconsistency there produces self-intersecting rings. Return `Vec<Vec<Vec2>>` in image space, Y down.

Written in-house rather than via the `contour` crate: it is stale (Apr 2024), and control over the ring output is needed for the topology step anyway.

- [ ] **Step 6: Implement RDP**

`silhouette/rdp.rs`: recursive Ramer–Douglas–Peucker on a **closed** ring. Split the ring at its two mutually most distant points and simplify each half, so the result does not depend on where the ring's array happens to start.

- [ ] **Step 7: Implement ring topology and sanitizing**

`silhouette/topology.rs`:
1. Drop rings whose `|signed_area| < min_region_area_px`.
2. A ring is a hole if it is strictly contained inside another ring (test one of its points with a point-in-polygon check against each candidate).
3. Normalize winding: outer → CCW (positive signed area in Y-down image space), hole → CW.
4. Run each ring through `i_overlay`'s self-union to remove self-intersections introduced by RDP. If that yields multiple pieces, keep the largest.
5. Sort outer rings by descending area.

- [ ] **Step 8: Implement the fallbacks**

`silhouette/fallback.rs`: `convex_hull_ring` (monotone chain over the opaque pixel coordinates) and `bounding_box_ring` (four corners of the opaque bounding box). Both always succeed on any image containing at least one opaque pixel. These exist so the user is never blocked by a bad silhouette.

- [ ] **Step 9: Run the tests to verify they pass**

Run: `cargo test -p animus-core silhouette`
Expected: 9 passed.

- [ ] **Step 10: Commit**

```bash
git add crates/animus-core/src/silhouette crates/animus-core/tests/fixtures/images
git commit -m "feat(core): alpha silhouette extraction

Dilate-then-erode closing, in-house marching squares, closed-ring RDP,
hole classification with winding normalization, and convex-hull and
bounding-box fallbacks so a bad silhouette never blocks the user."
```

---

## Task 11: Core — triangulation

**Files:**
- Create: `crates/animus-core/src/triangulate/mod.rs`, `points.rs`, `cdt.rs`, `filter.rs`
- Modify: `crates/animus-core/src/lib.rs`

**Interfaces:**
- Consumes: `silhouette::Ring` from Task 10, `doc::{MeshData, AutoMeshParams}` from Task 7.
- Produces:
  - `pub fn triangulate(rings: &[Ring], params: &AutoMeshParams, img_size: (u32, u32)) -> Result<MeshData, TriangulateError>`
  - `pub fn poisson_disc(rings: &[Ring], spacing: f32, seed: u64) -> Vec<Vec2>`

- [ ] **Step 1: Read the `spade` API before writing code**

Open https://docs.rs/spade/2.15/spade/struct.ConstrainedDelaunayTriangulation.html and confirm the exact names and signatures for: constructing a CDT, inserting a vertex and getting a handle back, adding a constraint edge between two handles, iterating the resulting faces, and the error type when constraint insertion fails on intersecting edges.

Write the confirmed signatures into a comment at the top of `cdt.rs` before implementing. **This step exists because getting a generic geometry crate's API wrong produces confusing errors much later; five minutes of reading prevents an hour of guessing.**

- [ ] **Step 2: Write the failing tests**

`crates/animus-core/src/triangulate/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::silhouette::{signed_area, Ring};
    use glam::Vec2;

    fn square(size: f32) -> Ring {
        Ring {
            points: vec![
                Vec2::new(0.0, 0.0), Vec2::new(size, 0.0),
                Vec2::new(size, size), Vec2::new(0.0, size),
            ],
            is_hole: false,
        }
    }

    fn hole(cx: f32, cy: f32, r: f32) -> Ring {
        // CW winding for a hole.
        Ring {
            points: vec![
                Vec2::new(cx - r, cy - r), Vec2::new(cx - r, cy + r),
                Vec2::new(cx + r, cy + r), Vec2::new(cx + r, cy - r),
            ],
            is_hole: true,
        }
    }

    fn params(spacing: f32) -> crate::doc::AutoMeshParams {
        crate::doc::AutoMeshParams {
            alpha_threshold: 8, close_radius: 2, rdp_epsilon_px: 2.0,
            min_region_area_px: 64.0, interior_spacing_px: spacing,
            mode: crate::doc::AutoMeshMode::Silhouette,
        }
    }

    #[test]
    fn a_square_triangulates_into_a_valid_mesh() {
        let m = triangulate(&[square(100.0)], &params(25.0), (100, 100)).unwrap();
        assert!(!m.triangles.is_empty());
        assert_eq!(m.triangles.len() % 3, 0);
        assert_eq!(m.uvs.len(), m.positions.len());
        for i in &m.triangles {
            assert!((*i as usize) < m.positions.len());
        }
    }

    #[test]
    fn every_triangle_centroid_lies_inside_the_shape() {
        let m = triangulate(&[square(100.0)], &params(25.0), (100, 100)).unwrap();
        for t in m.triangles.chunks_exact(3) {
            let c = (m.positions[t[0] as usize]
                   + m.positions[t[1] as usize]
                   + m.positions[t[2] as usize]) / 3.0;
            assert!(c.x > -0.01 && c.x < 100.01 && c.y > -0.01 && c.y < 100.01,
                "centroid {c:?} escaped the square");
        }
    }

    #[test]
    fn no_triangle_lands_inside_a_hole() {
        let rings = vec![square(100.0), hole(50.0, 50.0, 20.0)];
        let m = triangulate(&rings, &params(15.0), (100, 100)).unwrap();
        for t in m.triangles.chunks_exact(3) {
            let c = (m.positions[t[0] as usize]
                   + m.positions[t[1] as usize]
                   + m.positions[t[2] as usize]) / 3.0;
            let in_hole = c.x > 30.0 && c.x < 70.0 && c.y > 30.0 && c.y < 70.0;
            assert!(!in_hole, "triangle centroid {c:?} is inside the hole");
        }
    }

    #[test]
    fn the_boundary_survives_as_mesh_edges() {
        // This is what CDT buys us over plain Delaunay: an L-shaped
        // concavity must not be cut across.
        let l_shape = Ring {
            points: vec![
                Vec2::new(0.0, 0.0),   Vec2::new(100.0, 0.0),
                Vec2::new(100.0, 40.0), Vec2::new(40.0, 40.0),
                Vec2::new(40.0, 100.0), Vec2::new(0.0, 100.0),
            ],
            is_hole: false,
        };
        let m = triangulate(&[l_shape], &params(20.0), (100, 100)).unwrap();
        for t in m.triangles.chunks_exact(3) {
            let c = (m.positions[t[0] as usize]
                   + m.positions[t[1] as usize]
                   + m.positions[t[2] as usize]) / 3.0;
            // The notch is the region x>40 AND y>40.
            assert!(!(c.x > 41.0 && c.y > 41.0),
                "triangle centroid {c:?} spans the L-shape's notch");
        }
    }

    #[test]
    fn no_zero_area_triangles_are_emitted() {
        let m = triangulate(&[square(100.0)], &params(25.0), (100, 100)).unwrap();
        for t in m.triangles.chunks_exact(3) {
            let (a, b, c) = (m.positions[t[0] as usize],
                             m.positions[t[1] as usize],
                             m.positions[t[2] as usize]);
            let cross = (b - a).perp_dot(c - a);
            assert!(cross.abs() > 1e-3, "degenerate triangle, cross = {cross}");
        }
    }

    #[test]
    fn uvs_are_normalized_pixel_coordinates_with_no_y_flip() {
        let m = triangulate(&[square(100.0)], &params(25.0), (200, 400)).unwrap();
        for (p, uv) in m.positions.iter().zip(&m.uvs) {
            assert!((uv.x - p.x / 200.0).abs() < 1e-5);
            assert!((uv.y - p.y / 400.0).abs() < 1e-5, "UVs must NOT flip in Y");
        }
    }

    #[test]
    fn total_mesh_area_matches_the_silhouette_area() {
        let rings = vec![square(100.0), hole(50.0, 50.0, 20.0)];
        let m = triangulate(&rings, &params(10.0), (100, 100)).unwrap();
        let mesh_area: f32 = m.triangles.chunks_exact(3).map(|t| {
            let (a, b, c) = (m.positions[t[0] as usize],
                             m.positions[t[1] as usize],
                             m.positions[t[2] as usize]);
            ((b - a).perp_dot(c - a) / 2.0).abs()
        }).sum();
        let want = 100.0 * 100.0 - 40.0 * 40.0;   // 8400
        assert!((mesh_area - want).abs() / want < 0.02,
            "mesh area {mesh_area} vs expected {want}");
    }

    #[test]
    fn poisson_points_respect_the_minimum_spacing() {
        let pts = poisson_disc(&[square(200.0)], 20.0, 12345);
        assert!(pts.len() > 10);
        for (i, a) in pts.iter().enumerate() {
            for b in &pts[i + 1..] {
                assert!(a.distance(*b) >= 20.0 * 0.99,
                    "points {a:?} and {b:?} are too close");
            }
        }
    }

    #[test]
    fn poisson_sampling_is_reproducible_for_a_given_seed() {
        let a = poisson_disc(&[square(200.0)], 20.0, 7);
        let b = poisson_disc(&[square(200.0)], 20.0, 7);
        assert_eq!(a, b, "same seed must give the same points");
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p animus-core triangulate`
Expected: FAIL — `triangulate` not found.

- [ ] **Step 4: Implement Poisson-disc sampling**

`triangulate/points.rs`: Bridson's algorithm over the outer rings' bounding box, rejecting candidates that are outside an outer ring or inside a hole. Use a small deterministic PRNG seeded by the `seed` argument — write a 20-line xorshift rather than adding a `rand` dependency, so sampling is reproducible without a new crate and `MeshSource::Auto` can regenerate an identical mesh.

Poisson-disc rather than a grid: a regular lattice produces visible artifacts when the mesh deforms. Note that in a comment.

- [ ] **Step 5: Implement the CDT**

`triangulate/cdt.rs`, using the signatures confirmed in Step 1:
1. Insert every ring point, keeping its handle.
2. `add_constraint` between consecutive handles around each ring, closing the loop.
3. Insert the Poisson interior points as free vertices.
4. Collect the resulting faces as index triples into a `Vec<Vec2>` of the inserted positions.

Handle the constraint-insertion error by returning `TriangulateError::ConstraintFailed`, which the caller uses to walk the fallback ladder from spec §6.2.

- [ ] **Step 6: Implement filtering and UV assignment**

`triangulate/filter.rs`:
- Drop triangles whose centroid is outside every outer ring, or inside any hole.
- Drop triangles with `|perp_dot| < 1e-3`.
- Normalize winding to CCW.
- Then in `mod.rs`, drop vertices no surviving triangle references (reuse `IndexRemap` from Task 8 — do **not** write a second compaction path), and set `uv[i] = positions[i] / Vec2::new(w as f32, h as f32)` with **no Y flip**.

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p animus-core triangulate`
Expected: 9 passed.

- [ ] **Step 8: Commit**

```bash
git add crates/animus-core/src/triangulate
git commit -m "feat(core): constrained Delaunay triangulation

spade CDT with ring segments as constraints, so the silhouette outline is
guaranteed to appear as mesh edges. Deterministic Poisson-disc interior
sampling, centroid and area filtering, and UVs with no Y flip."
```

---

## Task 12: Core — attachment weights and the GPU bake

**Files:**
- Create: `crates/animus-core/src/skeleton/mod.rs`, `attach.rs`, `bake.rs`
- Modify: `crates/animus-core/src/lib.rs`

**Interfaces:**
- Consumes: `doc::{MeshData, SkeletonData, AttachmentTable, Attachment}`, `CompiledRig` from Task 9.
- Produces:
  - `pub fn auto_attach(mesh: &MeshData, skel: &SkeletonData) -> AttachmentTable` — radius falloff per bone, normalized per vertex
  - `pub struct BakedInfluences { pub joint_index: Vec<[u16; 4]>, pub joint_weight: Vec<[f32; 4]>, pub max_dropped_mass: f32 }` — `joint_index` holds **bone** indices; the name matches Bevy's `ATTRIBUTE_JOINT_INDEX`, which it feeds directly
  - `pub fn bake_influences(att: &AttachmentTable, rig: &CompiledRig, vertex_count: usize) -> Result<BakedInfluences, BakeError>`
  - `pub const MAX_SKIN_BONES: usize = 256;` (Bevy's `MAX_JOINTS`) and `pub const MAX_INFLUENCES: usize = 4;`
  - `pub enum BakeError { TooManyBones { count: usize, max: usize } }`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn a_vertex_on_a_bone_gets_full_weight_from_it() {
        // bone from (0,0) to (100,0), radius 30; vertex at (50,0)
        let (mesh, skel) = one_bone_with_vertices(&[Vec2::new(50.0, 0.0)]);
        let t = auto_attach(&mesh, &skel);
        assert_eq!(t.entries.len(), 1);
        assert_relative_eq!(t.entries[0].weight, 1.0, epsilon = 1e-5);
    }

    #[test]
    fn a_vertex_beyond_the_radius_gets_no_attachment() {
        let (mesh, skel) = one_bone_with_vertices(&[Vec2::new(50.0, 500.0)]);
        let t = auto_attach(&mesh, &skel);
        assert!(t.entries.is_empty());
    }

    #[test]
    fn weights_are_normalized_per_vertex_across_bones() {
        let (mesh, skel) = two_overlapping_bones_with_vertex(Vec2::new(50.0, 5.0));
        let t = auto_attach(&mesh, &skel);
        let sum: f32 = t.entries.iter().filter(|a| a.vertex == 0).map(|a| a.weight).sum();
        assert_relative_eq!(sum, 1.0, epsilon = 1e-5);
    }

    #[test]
    fn local_coords_are_recorded_in_each_bones_frame() {
        let (mesh, skel) = one_bone_with_vertices(&[Vec2::new(50.0, 10.0)]);
        let t = auto_attach(&mesh, &skel);
        // Bone frame origin is joint A at (0,0), X axis along the bone.
        assert_relative_eq!(t.entries[0].local.x, 50.0, epsilon = 1e-4);
        assert_relative_eq!(t.entries[0].local.y, 10.0, epsilon = 1e-4);
    }

    #[test]
    fn bake_keeps_the_top_four_influences_and_renormalizes() {
        let att = attachments_for_one_vertex(&[0.5, 0.3, 0.1, 0.05, 0.05]);
        let rig = rig_with_bones(5);
        let baked = bake_influences(&att, &rig, 1).unwrap();
        let sum: f32 = baked.joint_weight[0].iter().sum();
        assert_relative_eq!(sum, 1.0, epsilon = 1e-5);
        assert!(baked.max_dropped_mass > 0.04 && baked.max_dropped_mass < 0.06);
    }

    #[test]
    fn bake_pads_with_zeros_when_there_are_fewer_than_four_influences() {
        let att = attachments_for_one_vertex(&[1.0]);
        let rig = rig_with_bones(1);
        let baked = bake_influences(&att, &rig, 1).unwrap();
        assert_relative_eq!(baked.joint_weight[0][0], 1.0, epsilon = 1e-5);
        assert_eq!(&baked.joint_weight[0][1..], &[0.0, 0.0, 0.0]);
    }

    #[test]
    fn an_unattached_vertex_bakes_to_zero_weights_not_a_panic() {
        let att = AttachmentTable::default();
        let rig = rig_with_bones(1);
        let baked = bake_influences(&att, &rig, 3).unwrap();
        assert_eq!(baked.joint_weight.len(), 3);
        assert_eq!(baked.joint_weight[0], [0.0; 4]);
    }

    #[test]
    fn more_than_256_bones_is_a_clear_error_not_a_render_panic() {
        // Bevy's MAX_JOINTS counts entries in SkinnedMesh.joints, and per
        // spec section 7.3 those are our BONES, not our joints.
        let rig = rig_with_bones(300);
        let err = bake_influences(&AttachmentTable::default(), &rig, 1).unwrap_err();
        assert!(matches!(err, BakeError::TooManyBones { count: 300, max: 256 }));
    }

    #[test]
    fn top_four_selection_is_stable_under_equal_weights() {
        // Ties must break deterministically by bone index, or the golden
        // mesh changes between runs.
        let att = attachments_for_one_vertex(&[0.2, 0.2, 0.2, 0.2, 0.2]);
        let rig = rig_with_bones(5);
        let a = bake_influences(&att, &rig, 1).unwrap();
        let b = bake_influences(&att, &rig, 1).unwrap();
        assert_eq!(a.joint_index, b.joint_index);
    }
}
```

Write the four helpers (`one_bone_with_vertices`, `two_overlapping_bones_with_vertex`, `attachments_for_one_vertex`, `rig_with_bones`) as `#[cfg(test)]` functions in the same module. `rig_with_bones(n)` builds a `CompiledRig` with `n` bones spanning `n + 1` joints.

**Naming note that matters:** `BakedInfluences::joint_index` keeps Bevy's vocabulary because it feeds `Mesh::ATTRIBUTE_JOINT_INDEX` directly, but the values it holds are **bone indices into `CompiledRig`**. Say so in a doc comment on the struct — this is the single most confusable name pair in the codebase.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p animus-core skeleton`
Expected: FAIL — `auto_attach` not found.

- [ ] **Step 3: Implement `auto_attach`**

For each bone and each vertex, compute the distance from the vertex to the bone *segment* (not the infinite line). If `dist <= bone.attach_radius`, weight is `(1.0 - dist / radius).powf(falloff)` with `falloff = 2.0`. Record `local` as the vertex position expressed in the bone's rest frame: origin at joint A's rest position, X axis along A→B, Y perpendicular.

Then normalize: for each vertex, divide its weights by their sum. Vertices with no bone in range get no entries at all.

Sort `entries` by `(vertex, bone)` before returning, so serialization is deterministic.

- [ ] **Step 4: Implement `bake_influences`**

Group entries by vertex; sort each group by weight descending with **bone index ascending as the tie-break** (this is what makes the stability test pass); keep the first four; renormalize; pad to four with index 0 and weight 0. Track the largest dropped mass across all vertices for the UI warning.

Return `BakeError::TooManyBones { count, max: MAX_SKIN_BONES }` when the rig has more than 256 bones — spec §7.2 requires a clear user-facing message *before* Bevy's renderer would panic.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p animus-core skeleton`
Expected: 9 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/animus-core/src/skeleton
git commit -m "feat(core): attachment weighting and top-4 GPU bake

Radius-falloff auto-attach with per-vertex normalization and bone-local
rest coordinates; deterministic top-4 bake that reports dropped mass and
rejects rigs above Bevy's 256-joint limit with a clear error."
```

---

## Task 13: Project — JSON codec, safe save, and the asset store

**Files:**
- Create: `crates/animus-project/src/lib.rs` (rewrite), `json.rs`, `assets.rs`, `save.rs`, `load.rs`, `error.rs`
- Create: `crates/animus-project/tests/roundtrip.rs`
- Create: `spec/animus-project-format-v1.md`, `spec/LICENSE` (CC0-1.0)

**Interfaces:**
- Consumes: `animus_core::doc::Project` from Task 7.
- Produces:
  - `pub fn save(project: &Project, dir: &Path) -> Result<(), ProjectError>` — atomic
  - `pub fn load(dir: &Path) -> Result<Project, ProjectError>`
  - `pub struct AssetStore` with `import(&mut self, src: &Path, kind: AssetKind) -> Result<AssetRef, ProjectError>` and `path_for(&self, r: &AssetRef) -> PathBuf`
  - `pub fn to_json(project: &Project) -> Result<String, ProjectError>` — rejects non-finite floats

- [ ] **Step 1: Write the failing tests**

`crates/animus-project/tests/roundtrip.rs`:

```rust
use animus_core::doc::*;
use animus_core::ids::LayerId;
use animus_project::{load, save, to_json, AssetStore, ProjectError};
use std::fs;
use tempfile::tempdir;

fn sample() -> Project {
    let mut p = Project::new("Sample Show");
    let lid = LayerId(p.alloc_id());
    p.layers.push(lid);
    p.layer_data.insert(lid, Layer::new(lid, "Background"));
    p
}

#[test]
fn save_then_load_reproduces_the_document() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("MyShow.animus");
    let p = sample();
    save(&p, &root).unwrap();
    let back = load(&root).unwrap();
    assert_eq!(to_json(&back).unwrap(), to_json(&p).unwrap());
}

#[test]
fn saving_twice_produces_byte_identical_json() {
    // Key ordering must be stable, or every save churns the git diff.
    let p = sample();
    assert_eq!(to_json(&p).unwrap(), to_json(&p).unwrap());
}

#[test]
fn save_is_atomic_and_leaves_no_temp_file() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("MyShow.animus");
    save(&sample(), &root).unwrap();
    assert!(root.join("project.json").exists());
    assert!(!root.join("project.json.tmp").exists());
}

#[test]
fn an_existing_project_survives_a_second_save() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("MyShow.animus");
    save(&sample(), &root).unwrap();
    let mut p2 = sample();
    p2.meta.name = "Renamed".into();
    save(&p2, &root).unwrap();
    assert_eq!(load(&root).unwrap().meta.name, "Renamed");
}

#[test]
fn non_finite_floats_are_rejected_at_write_time() {
    let mut p = sample();
    p.solver.global_damping = f32::NAN;
    let err = to_json(&p).unwrap_err();
    assert!(matches!(err, ProjectError::NonFiniteFloat { .. }));
}

#[test]
fn a_newer_schema_version_is_refused_with_a_clear_error() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("MyShow.animus");
    save(&sample(), &root).unwrap();
    let path = root.join("project.json");
    let text = fs::read_to_string(&path).unwrap()
        .replace("\"schema_version\": 1", "\"schema_version\": 99");
    fs::write(&path, text).unwrap();

    match load(&root) {
        Err(ProjectError::SchemaTooNew { found: 99, supported: 1 }) => {}
        other => panic!("expected SchemaTooNew, got {other:?}"),
    }
}

#[test]
fn truncated_json_is_an_error_not_a_panic() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("MyShow.animus");
    save(&sample(), &root).unwrap();
    fs::write(root.join("project.json"), "{ \"schema_version\": 1, \"me").unwrap();
    assert!(load(&root).is_err());
}

#[test]
fn assets_are_stored_by_content_hash() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("MyShow.animus");
    fs::create_dir_all(&root).unwrap();
    let src = dir.path().join("pic.png");
    fs::write(&src, b"not really a png, but bytes are bytes").unwrap();

    let mut store = AssetStore::new(&root);
    let a = store.import(&src, AssetKind::Image).unwrap();
    assert_eq!(a.sha256.len(), 64);
    assert!(store.path_for(&a).exists());
    assert_eq!(a.original_name, "pic.png");
}

#[test]
fn importing_identical_bytes_twice_stores_one_file() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("MyShow.animus");
    fs::create_dir_all(&root).unwrap();
    let a_path = dir.path().join("a.png");
    let b_path = dir.path().join("b.png");
    fs::write(&a_path, b"same bytes").unwrap();
    fs::write(&b_path, b"same bytes").unwrap();

    let mut store = AssetStore::new(&root);
    let a = store.import(&a_path, AssetKind::Image).unwrap();
    let b = store.import(&b_path, AssetKind::Image).unwrap();
    assert_eq!(a.sha256, b.sha256);
    assert_eq!(store.path_for(&a), store.path_for(&b));

    let count = walkdir_count_files(&root.join("assets"));
    assert_eq!(count, 1, "identical bytes must be stored once");
}

fn walkdir_count_files(p: &std::path::Path) -> usize {
    fn rec(p: &std::path::Path, n: &mut usize) {
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                let path = e.path();
                if path.is_dir() { rec(&path, n) } else { *n += 1 }
            }
        }
    }
    let mut n = 0;
    rec(p, &mut n);
    n
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p animus-project`
Expected: FAIL — `save` not found.

- [ ] **Step 3: Implement the error type**

`error.rs`, a `thiserror` enum with `Io`, `Json`, `NonFiniteFloat { path: String }`, `SchemaTooNew { found: u32, supported: u32 }`, `MissingAsset { sha256: String }`, `Migration { from: u32, to: u32, reason: String }`.

- [ ] **Step 4: Implement `to_json` with the non-finite guard**

Serialize to `serde_json::Value` first, walk it recursively, and return `NonFiniteFloat` if any number fails `is_finite()`. Then `serde_json::to_string_pretty` with 2-space indent.

`serde_json` actually serializes `f32::NAN` as `null`, which would silently corrupt the file rather than fail — so the explicit walk is necessary, not belt-and-braces. Say so in a comment.

Key order is stable because `Project` uses `IndexMap` and `serde_json` has `preserve_order` enabled.

- [ ] **Step 5: Implement `save` atomically**

1. `create_dir_all(dir)` and `dir/assets`.
2. Write `to_json` output to `dir/project.json.tmp`.
3. `File::sync_all` on the temp file.
4. `fs::rename` over `dir/project.json` — atomic on both NTFS and POSIX.

A crash between 2 and 4 leaves the previous `project.json` intact. Never write `project.json` in place.

- [ ] **Step 6: Implement `load` with the schema gate**

1. Read `dir/project.json`.
2. Parse to `serde_json::Value`.
3. Read `schema_version`. If greater than `CURRENT_SCHEMA_VERSION`, return `SchemaTooNew` — **never guess at a future format**.
4. If less, run the migration chain (Task 14 fills it; for now assert it equals current).
5. Deserialize to `Project`.

- [ ] **Step 7: Implement `AssetStore`**

`import` reads the source file, computes `sha2::Sha256` over the bytes, writes to `assets/<sha[0..2]>/<sha>.<ext>` if not already present, and returns an `AssetRef` carrying the hash, the original filename for the UI, the byte length, and the kind. `path_for` reconstructs the path from the hash.

Content addressing means the project is self-contained and portable to a venue laptop, identical images cost one file, and `project.json` never churns because a path changed.

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cargo test -p animus-project`
Expected: 10 passed.

- [ ] **Step 9: Write the CC0 format specification**

`spec/animus-project-format-v1.md` documents the directory layout, `project.json`'s complete schema, the content-addressed asset naming rule, and the versioning and migration policy. `spec/LICENSE` is the CC0-1.0 text.

Add a note in the workspace `README.md` pointing at it: the format is CC0 so anyone can implement a reader without legal analysis.

- [ ] **Step 10: Commit**

```bash
git add crates/animus-project spec
git commit -m "feat(project): JSON codec, atomic save, content-addressed assets

Stable key ordering for diffable files, an explicit non-finite float
guard (serde_json would silently write null), tmp+rename safe save, and
a schema gate that refuses future versions instead of guessing.
Format spec published under CC0."
```

---

## Task 14: Project — the migration chain

**Files:**
- Create: `crates/animus-core/src/migrate/mod.rs`, `crates/animus-core/src/migrate/v1_to_v2.rs`
- Create: `crates/animus-project/tests/migrations.rs`
- Create: `spec/fixtures/v1_sample/project.json`
- Modify: `crates/animus-project/src/load.rs`

**Interfaces:**
- Consumes: `doc::CURRENT_SCHEMA_VERSION` from Task 7.
- Produces:
  - `pub fn run(value: &mut serde_json::Value, from: u32) -> Result<(), MigrateError>` — applies every step from `from` to `CURRENT_SCHEMA_VERSION`
  - `pub type Migration = fn(&mut serde_json::Value) -> Result<(), MigrateError>;`
  - `pub const MIGRATIONS: &[Migration]` — index `i` migrates version `i+1` to `i+2`

**Why now, when there is only one schema version:** the mechanism must be exercised before it is needed. Building it after the first real migration means building it under pressure, with users' files at stake.

- [ ] **Step 1: Write the failing tests**

`crates/animus-project/tests/migrations.rs`:

```rust
use animus_core::doc::CURRENT_SCHEMA_VERSION;
use animus_core::migrate::{run, MigrateError, MIGRATIONS};
use serde_json::json;

#[test]
fn the_chain_has_one_step_per_version_gap() {
    assert_eq!(
        MIGRATIONS.len() as u32,
        CURRENT_SCHEMA_VERSION - 1,
        "every schema bump needs exactly one migration"
    );
}

#[test]
fn migrating_from_the_current_version_is_a_no_op() {
    let mut v = json!({ "schema_version": CURRENT_SCHEMA_VERSION, "x": 1 });
    let before = v.clone();
    run(&mut v, CURRENT_SCHEMA_VERSION).unwrap();
    assert_eq!(v, before);
}

#[test]
fn migrating_from_a_future_version_is_an_error() {
    let mut v = json!({ "schema_version": 99 });
    assert!(matches!(run(&mut v, 99), Err(MigrateError::FromTheFuture { .. })));
}

#[test]
fn every_committed_fixture_migrates_to_the_current_version_and_loads() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../spec/fixtures");
    let mut checked = 0;
    for entry in std::fs::read_dir(root).unwrap().flatten() {
        let dir = entry.path();
        if !dir.is_dir() { continue; }
        let p = animus_project::load(&dir)
            .unwrap_or_else(|e| panic!("fixture {dir:?} failed to load: {e:?}"));
        assert_eq!(p.schema_version, CURRENT_SCHEMA_VERSION);
        checked += 1;
    }
    assert!(checked > 0, "no fixtures found — the migration guard is not guarding anything");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p animus-project --test migrations`
Expected: FAIL — `animus_core::migrate` not found.

- [ ] **Step 3: Implement the chain**

`crates/animus-core/src/migrate/mod.rs`:

```rust
//! Schema migrations.
//!
//! Migrations operate on the **raw JSON**, before any typed construction.
//! That way we never have to keep old versions of the document structs
//! around, and a migration can restructure freely.
//!
//! To add schema version N+1:
//!   1. Bump `CURRENT_SCHEMA_VERSION` to N+1.
//!   2. Add `vN_to_vN1.rs` and append it to `MIGRATIONS`.
//!   3. Add `spec/fixtures/vN_sample/` containing a project at version N.
//! Step 3 is not optional — CI fails a schema bump with no fixture.

mod v1_to_v2;

use crate::doc::CURRENT_SCHEMA_VERSION;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MigrateError {
    #[error("project schema version {found} is newer than this build supports ({supported})")]
    FromTheFuture { found: u32, supported: u32 },
    #[error("migration {from} -> {to} failed: {reason}")]
    Failed { from: u32, to: u32, reason: String },
}

pub type Migration = fn(&mut Value) -> Result<(), MigrateError>;

/// Index `i` migrates schema version `i + 1` to `i + 2`.
pub const MIGRATIONS: &[Migration] = &[];

pub fn run(value: &mut Value, from: u32) -> Result<(), MigrateError> {
    if from > CURRENT_SCHEMA_VERSION {
        return Err(MigrateError::FromTheFuture {
            found: from,
            supported: CURRENT_SCHEMA_VERSION,
        });
    }
    for v in from..CURRENT_SCHEMA_VERSION {
        let step = MIGRATIONS[(v - 1) as usize];
        step(value)?;
        value["schema_version"] = Value::from(v + 1);
    }
    Ok(())
}
```

`v1_to_v2.rs` currently holds only the documented shape of a migration function, unused, so the pattern is visible to whoever writes the first real one. Mark it `#[allow(dead_code)]` with a comment explaining why it exists.

- [ ] **Step 4: Wire the chain into `load`**

Replace Task 13 Step 6's placeholder assertion with a real `migrate::run(&mut value, found_version)?` call, mapping `MigrateError::FromTheFuture` to `ProjectError::SchemaTooNew` so the existing test from Task 13 still passes.

- [ ] **Step 5: Create the v1 fixture**

Save a small but non-trivial project — two layers, one mesh puppet with a real triangulated mesh and a three-bone skeleton with attachments — into `spec/fixtures/v1_sample/`. Generate it with a `#[test]`-gated helper or an `xtask` command so it can be regenerated rather than hand-maintained, and commit both `project.json` and any assets.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p animus-project`
Expected: all tests pass, including the Task 13 suite.

- [ ] **Step 7: Run the entire suite and the architectural check**

Run:
```bash
cargo fmt --all --check
cargo clippy -p animus-core -p animus-project --all-targets -- -D warnings
cargo test -p animus-core -p animus-project
cargo tree -p animus-core | grep -i bevy      # must print nothing
cargo deny check licenses advisories
```

Expected: everything green, `grep` finds nothing.

- [ ] **Step 8: Commit**

```bash
git add crates/animus-core/src/migrate crates/animus-project spec/fixtures
git commit -m "feat(core): schema migration chain

Raw-JSON migrations run before typed construction, so old document
structs never need to be kept. Built and tested at version 1 so the
mechanism is exercised before it is needed, with a CI guard that a
schema bump without a fixture fails."
```

---

## Done Criteria for This Plan

- [ ] `cargo test -p animus-core -p animus-project` passes on a machine with no GPU and no Bevy.
- [ ] `cargo tree -p animus-core | grep -i bevy` finds nothing, and CI enforces it.
- [ ] `cargo clippy --all-targets -- -D warnings` is clean.
- [ ] `cargo deny check licenses` passes with no GPL/LGPL in the graph.
- [ ] Adding a vertex-index field to `MeshPuppet` fails to compile until it is handled (verified in Task 8 Step 8).
- [ ] `validate()` reports every mesh defect at once and never panics on malformed input.
- [ ] All four spike findings documents exist in `docs/spikes/` and answer their questions.
- [ ] `docs/spikes/m0-1-skinned-mesh.md` confirms flat joint sets skin correctly. **If it does not, spec §7 needs revisiting before the M1 plan is written.**
- [ ] `docs/spikes/m0-3-spout.md` records a measured readback latency figure, not an estimate.
- [ ] `glam` in `[workspace.dependencies]` is pinned to Bevy's exact version.
- [ ] `spec/animus-project-format-v1.md` and `spec/LICENSE` (CC0) exist.

## Next Plan

The M1 plan — the Bevy side: `animus-runtime` (document→ECS projection, skinned mesh construction, solver driver), `animus-editor` (dock, viewport, tools, gizmos, undo), and the output window. **Write it only after the M0 findings are in**, because M0-1 governs the skinning architecture, M0-2 governs the editor UI structure, and M0-4 governs the output design.
