# Animus Live — M1: The 2D Vertical Slice

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The first shippable tool. Import an image, get a mesh and a rig, drag a joint and watch the puppet deform organically, put it on a projector, save it, reopen it unchanged.

**Architecture:** `animus-core` and `animus-project` already exist and are Bevy-free, tested, and complete enough to build on — silhouette extraction, constrained-Delaunay triangulation, attachment weights, the GPU bake, the Verlet + Gauss–Seidel solver, the document model, `IndexRemap`, the migration chain, and safe save/load. **M1 adds the two things they do not have: a way to change the document (the `DocCommand` spine) and a way to see it (the Bevy half).** Four new crates: `animus-runtime` projects the document into ECS, `animus-editor` is the egui shell and the tools, `animus-output` owns the projector window, `animus-app` is the binary.

**Tech Stack:** Existing core stack, plus `bevy =0.19.1`, `bevy_egui =0.40`, `egui =0.34`, `egui_dock =0.19.1`, `bevy-inspector-egui =0.37`.

**Spec:** `docs/superpowers/specs/2026-08-16-animus-live-design.md` — §4 data model, §7 skinning, §8 ECS, §10 editor UI, §11 output, §18 M1 definition.

**M0 findings this plan is built on:** `docs/spikes/`. Where the spec and a spike disagree, **the spike wins** — the spec now carries the corrections inline.

---

## Global Constraints

Inherited from the foundation plan and still binding:

- **Clean-room.** Never read the original Animata C++ `src/`. A checkout of it may sit next to this one on a development machine. (Spec §1.3, `CONTRIBUTING.md`)
- **`animus-core` and `animus-project` never depend on Bevy.** CI asserts it. Anything M1 adds to core must hold to this — including the `DocCommand` spine.
- **Exact-pinned versions**, `Cargo.lock` committed, one `egui` in the graph.
- **`glam` must be the exact semver Bevy's `bevy_math` uses.**
- **Image space is pixels, origin top-left, Y down. World space is Y up. Only positions flip; UVs do not.** Verified by eye in M0-1: "TOP" renders at the top, right side up. **M1 needs no Y flip on this path.**
- **The skeleton is a graph of springs, not a hierarchy.** No parent/child bones anywhere.
- **One-way projection:** `DocumentRes` → entities, never back. (Spec §8.6)
- **Floats are rejected at serialization time if NaN or Inf.**
- **Commit after every task**, conventional prefixes.

New, and each one is a bug this project already made once:

- **The skinning palette is per BONE, not per joint.** `BoneId` is not a bone index — map it through `CompiledRig::bone_index()`. Fixtures must use non-contiguous, out-of-order IDs, or every test passes regardless. (Spec §7.3)
- **Do not hard-fail at 256 bones.** M0-1 ran 257 and 512 clean on two discrete GPUs; the 256 limit binds only Bevy's fallback uniform-buffer path. `validate()` warns, never blocks.
- **`DynamicSkinnedMeshBounds` + `with_generated_skinned_mesh_bounds()` are mandatory on every puppet.** Measured: without them the mesh is wrongly frustum-culled in 28.3% of frames once it nears a view edge, against 0.1% with them.
- **Never gate viewport input on `ctx.wants_pointer_input()`**, and create the viewport image with `.sense(Sense::click_and_drag())`. Both are load-bearing; without them clicking, zooming and panning silently do nothing. (Spec §10.2, corrected)
- **No resize debounce.** Resize the render target immediately.
- **Any performance number must record the machine's power state.** A laptop on battery silently clamps to 30.00 fps. A suspiciously exact frame rate with near-zero spread is a limiter, not a measurement.
- **Headless tests must synthesise input.** Every M0 spike passed its `--auto-close` runs while its input path was completely broken. An interaction with no test that actually clicks is untested.

---

## Decision taken: PNG with alpha, and the door left open

**Decided by the user, 2026-08-17.** M1 accepts **PNG with an alpha channel** and nothing else. No matte generation, no flood fill, no segmentation. An operator who has a JPEG removes the background in the tool they already use.

The reason this is the right call and not a shortcut: every alternative buys reach at the cost of the one thing M1 has to prove — that image in, puppet out, on a projector, actually works end to end. Background removal is a whole feature with its own failure modes, and it is not what makes this tool worth using.

**But the requirement attached to it is real: adding formats later must be a small change, not a rewrite.** That is a design constraint on Task 2, and it is cheap to honour now and expensive to retrofit.

Three doors, held open by construction:

1. **Decoding.** All image loading goes through one `decode` module with an explicit format table, not scattered `image::open` calls. Adding AVIF or JPEG later is a feature flag plus one row. `image` stays feature-narrowed to PNG (spec §3.1) and `imageproc` stays `default-features = false`, so the AVIF and OpenEXR codec stacks stay out of the build until someone asks for them.
2. **The alpha source.** `MeshPuppet` carries `MatteParams { mode: MatteMode }` from day one, with `MatteMode::UseImageAlpha` as the only variant M1 implements. Because the field exists in the **v1 file format** from the first save, adding `BorderFloodFill` later is a new enum value in an existing field — a value addition, not a schema change, and therefore no migration.
3. **The failure message.** An opaque image is rejected with a message that names the actual problem and the actual fix, not a silent rectangle. That message is also where a future matte step gets offered.

**Explicitly not in M1:** border flood-fill, chroma key, manual outline, ML segmentation. Manual outline arrives with manual mesh editing in M6; the rest stay out of process.

## File Structure

**Created by this plan:**

| Path | Responsibility |
|---|---|
| `crates/animus-core/src/doc/command.rs` | `DocCommand`, `DocChange`, `PendingChanges`, `apply_command` |
| `crates/animus-core/src/doc/undo.rs` | `UndoStack` with merge, entry cap and memory cap |
| `crates/animus-core/src/image_in/` | Decode table, import errors, `MatteParams` |
| `crates/animus-runtime/` | Bevy: doc→ECS projection, `build_skinned_mesh`, solver driver, writeback |
| `crates/animus-editor/` | Bevy + egui: dock, theme, viewport, tools, gizmos, inspector, undo UI |
| `crates/animus-output/` | Bevy: output window, monitor selection, vsync toggle, layer isolation |
| `crates/animus-app/` | The `animus` binary: CLI, plugin wiring, panic handling, autosave |
| `spec/fixtures/m1-*/` | Golden projects for the round-trip test |

---

## Task 1: Core — the `DocCommand` spine and undo stack

> **DONE 2026-08-17** — `crates/animus-core/src/doc/{command,undo}.rs`, 20 tests including two properties. Merging is bounded by `break_merge()` from the caller rather than a time window: core has no clock, and a gesture ends when the mouse comes up, not when a timer expires.

Nothing in M1 can edit anything until this exists, and §18 flags it as required in M1 precisely because retrofitting undo is expensive.

**Files:** create `crates/animus-core/src/doc/command.rs`, `undo.rs`; modify `doc/mod.rs`, `lib.rs`

**Interfaces:**
- `pub enum DocChange { PuppetAdded(PuppetId), PuppetRemoved(PuppetId), JointMoved(PuppetId, JointId), MeshRebuilt(PuppetId), SkeletonChanged(PuppetId), LayerReordered, MaterialChanged(PuppetId), … }`
- `pub struct PendingChanges(Vec<DocChange>)`
- `pub trait DocCommand: Send + Sync + 'static { fn label(&self) -> &str; fn apply(&mut self, p: &mut Project) -> Result<PendingChanges, CommandError>; fn revert(&mut self, p: &mut Project) -> Result<PendingChanges, CommandError>; fn merge(&mut self, next: &dyn DocCommand) -> bool { false } }`
- `pub struct UndoStack { … }` with `push_applied`, `undo(&mut Project)`, `redo(&mut Project)`, `len()`, `memory_bytes()`

- [ ] **Step 1: Failing tests for the invariants that matter**
  - `apply` then `revert` restores a project that compares equal to the original, for every concrete command
  - `merge` collapses 200 synthetic `MoveJointRest` events into one undo entry
  - a snapshot command (`Retriangulate`) round-trips a 5k-vertex mesh
  - the stack caps at 100 entries **and** at 500 MB, dropping oldest first
  - `PendingChanges` granularity: `MoveJointRest` emits `JointMoved`, never `MeshRebuilt` — the expensive rebuild must not be triggered by a drag
- [ ] **Step 2: Inverse-pair commands** — `MoveJointRest`, `SetBoneParam`, `MoveVertex`, `SetLayerOpacity`, `SetLayerDepth`, `RenameLayer`. Each stores old and new values and implements `merge` on identical target within a time window.
- [ ] **Step 3: Snapshot commands** — `ImportImage`, `Retriangulate`, `AutoRig`, `DeleteVertices`, `DeleteJoint`. Snapshot the affected puppet, not the whole project, unless the op is project-wide.
- [ ] **Step 4: `apply_command(&mut Project, Box<dyn DocCommand>) -> Result<PendingChanges>`** as the single mutation path, plus `DocRevision { global, per_puppet }` bumping.
- [ ] **Step 5: proptest** — a random sequence of 200 commands, then undo everything, and assert the project equals its initial state. This is the test that catches asymmetric commands.

**Done when:** the proptest passes, and no path exists to mutate `Project` outside `apply_command`.

---

## Task 2: Core — the import contract, and the hinges for later formats

> **DONE 2026-08-17** — `crates/animus-core/src/image_in/`. Regenerating both golden fixtures removed the only corpus lacking the new field, so `additive_fields.rs` was added to keep the no-migration guarantee tested.

Small task, and the whole point of it is what it makes possible later.

**Files:** create `crates/animus-core/src/image_in/{mod.rs,decode.rs,matte.rs}`; modify `doc/mesh_puppet.rs`, `silhouette/mod.rs`

**Interfaces:**
- `pub enum ImageFormat { Png }` — the table that later grows
- `pub fn decode(bytes: &[u8], name: &str) -> Result<RgbaImage, ImportError>`
- `pub enum ImportError { UnsupportedFormat { ext: String }, NoAlphaChannel, FullyTransparent, TooLarge { w: u32, h: u32 } }`
- `pub struct MatteParams { pub mode: MatteMode }` and `pub enum MatteMode { UseImageAlpha }`
- `pub fn resolve_alpha(img: &mut RgbaImage, params: &MatteParams) -> Result<MatteReport, ImportError>`
- `pub fn is_effectively_opaque(img: &RgbaImage) -> bool`

- [ ] **Step 1: Failing tests**
  - a PNG with alpha imports and its silhouette is not the image rectangle
  - a fully opaque PNG returns `NoAlphaChannel` — **not** a rectangle mesh
  - a fully transparent PNG returns `FullyTransparent`
  - a `.jpg` returns `UnsupportedFormat { ext: "jpg" }`, naming the extension
  - `MatteParams` round-trips through save/load with `mode: "image_alpha"`
- [ ] **Step 2: The decode table.** One module, one match on format, one error type. No `image::open` anywhere else in the codebase — a lint-level rule, because this is exactly the kind of thing that gets scattered by the third contributor.
- [ ] **Step 3: `resolve_alpha`** with the single `UseImageAlpha` arm, returning `MatteReport { covered_fraction, touched_border }`. The report is unused in M1 beyond a sanity warning; it exists so a matte mode can populate it later without changing the call site.
- [ ] **Step 4: Serialize `MatteParams` into the v1 format** and add it to `spec/animus-project-format-v1.md` as an object with a `mode` string, documenting that unknown modes are a load error and new modes are additive. **This is the step that makes format work later cheap** — do not skip it because the enum has one variant.
- [ ] **Step 5: The rejection message.** `NoAlphaChannel` renders as: *"This image has no transparency, so there is nothing to cut out. Remove the background first and save as PNG."* Write it once, in core, so the editor and any future CLI say the same thing.

**Done when:** a PNG with alpha becomes a silhouette, everything else fails with a message a person can act on, and adding a second format touches exactly two places — the format table and the feature flag.

## Task 3: `animus-runtime` — the skinning build

> **DONE 2026-08-17** — `crates/animus-runtime/src/{coords,skinning}.rs`, 15 headless tests. Verified by mutation: flipping the sign in the bind-pose rotation fails the frame-convention test.

The single point where the document meets Bevy's GPU skinning. Spec §7.

**Files:** create `crates/animus-runtime/` (`Cargo.toml`, `src/lib.rs`, `src/skinning.rs`, `src/coords.rs`)

**Interfaces:**
- `pub fn img_to_world(p: Vec2, pivot: Vec2, ppu: f32) -> Vec3`
- `pub fn build_skinned_mesh(mp: &MeshPuppet, ppu: f32, pivot: Vec2) -> Result<Mesh, BuildError>`
- `pub fn build_inverse_bindposes(mp: &MeshPuppet, ppu: f32, pivot: Vec2) -> Vec<Mat4>` — **one entry per bone**, in `SkeletonData.bones` insertion order

- [ ] **Step 1: Failing tests, all headless.** A `Mesh` can be built and inspected without a GPU, so these are ordinary unit tests:
  - `ATTRIBUTE_JOINT_INDEX` is `VertexAttributeValues::Uint16x4` — assert the variant, not just the values; `[u16;4]` is ambiguous with `Unorm16x4`
  - joint indices are **bone indices**, verified with a fixture whose `BoneId`s are `[17, 23, 24]` in deliberately shuffled order
  - UVs are *not* flipped while positions *are*
  - `inverse_bindposes.len() == bones.len()`
  - a puppet with 300 bones builds without error and emits a warning, not a failure
- [ ] **Step 2: `img_to_world`, and the bone bind transform** per §7.3 — origin at joint A, +X along A→B, Z rotation from `atan2`.
- [ ] **Step 3: Mesh assembly** with `cull_mode: None`, `double_sided: true`, `unlit: true`, and `with_generated_skinned_mesh_bounds()`.
- [ ] **Step 4: Limit reporting** — `bake_influences` already returns dropped mass; surface it as a structured warning ("vertex 812 lost 18% of its influence") rather than a log line.
- [ ] **Step 5: A visual smoke binary** `examples/skinned_puppet.rs` that loads a fixture project and renders it, so a human can confirm the puppet is not silently inside-out. M0-1 exists precisely because this class of error is invisible to assertions.

---

## Task 4: `animus-runtime` — document → ECS projection

> **DONE 2026-08-17** — `crates/animus-runtime/src/{components,index,project,plugin}.rs`, 11 tests through a real `App`. The first version lost change *ordering*, so a puppet survived its own deletion; changes are a sequence, not a set.

**Files:** `src/project.rs`, `src/index.rs`, `src/plugin.rs`

**Interfaces:** `DocumentRes`, `DocRevision`, `PendingChangesRes`, `EntityIndex { puppets, joints, layers }`, `PuppetRoot`, `JointOf`, `CompiledRigRef(Arc<CompiledRig>)`, `PuppetSolver(SolverState)`, `RuntimePlugin`

- [ ] **Step 1: Failing tests** driving a headless `App` (`MinimalPlugins`): spawning a puppet creates exactly `1 + bones` entities; removing it leaves none; a `JointMoved` change does **not** rebuild the `Mesh` asset; a `MeshRebuilt` change does; `EntityIndex` never holds a stale entity after a despawn.
- [ ] **Step 2: The sync system** draining `PendingChanges` in `SyncSet::Apply`, spawning `Mesh3d` + `SkinnedMesh` + `DynamicSkinnedMeshBounds` **in one `commands` batch** — a mesh with `ATTRIBUTE_JOINT_INDEX` and no `SkinnedMesh` panics at render time (bevy#22469).
- [ ] **Step 3: Bone entities** as children of the puppet root and siblings of each other; the puppet mesh entity carries the `SkinnedMesh` whose `joints` vector is the bone entities in index order.
- [ ] **Step 4: `CompiledRig` rebuild and `Arc` swap** on skeleton change, so readers never lock.
- [ ] **Step 5: The debug assertion** from §8.6 — in dev builds, recompute expected entity counts from the document and panic on mismatch.
- [ ] **Step 6:** Add `Without<IsResource>` to every broad query. In Bevy 0.19 `#[derive(Resource)]` also derives `Component`, and resources live on entities.

---

## Task 5: `animus-runtime` — the solver driver

> **DONE 2026-08-17** — `crates/animus-runtime/src/solve.rs`, 6 tests. Two of them encoded wrong beliefs rather than wrong code: a released joint does not return to its rest pose (distance constraints, no angular springs), and the guard must be asserted through its message because a 60Hz frame runs two 120Hz ticks.

**Files:** `src/solve.rs`

- [ ] **Step 1: Failing tests** — a pinned joint stays pinned across 1000 ticks; a released puppet returns to rest; a NaN injected into `SolverState` triggers `reset_to_rest` and one `SolverPanic` event and does not propagate to other puppets; interpolation output is continuous across a tick boundary.
- [ ] **Step 2: `FixedUpdate` at `SolverConfig.hz`** with `Time::<Virtual>::max_delta` set to `max_substeps / hz`. Dropping simulation time under load is correct for a live show.
- [ ] **Step 3: `SolveSet::{Apply, Step, Guard}`**, `par_iter_mut` over `(&CompiledRigRef, &mut PuppetSolver)`.
- [ ] **Step 4: Writeback in `PostUpdate`, `.before(TransformSystems::Propagate)`** — per bone: translation at joint A, Z rotation from A→B, `length_mul` as X scale — lerped by `overstep_fraction()`.
- [ ] **Step 5: A benchmark** at 50 puppets × 40 joints recording the power state alongside the number, per the constraint above.

---

## Task 6: `animus-editor` — shell, dock and theme

> **IN PROGRESS 2026-08-17** — theme, state and dock written; the visual system is Showmesh's, lifted from its `DESIGN.md`. Contrast, the readable floor and the Signal Rule are asserted arithmetically in `theme.rs`'s tests. Fonts are deliberately not shipped yet and the reason is recorded in `install_fonts`.

§10.6 budgets two days for the theme and says not to defer it. Keep that.

**Files:** `crates/animus-editor/` — `lib.rs`, `theme.rs`, `dock.rs`, `state.rs`

- [ ] **Step 1:** `EditorState` with `DockState<TabKind>`, serialized to `%APPDATA%/animus/layout.json` — a user preference, never project data.
- [ ] **Step 2:** Tabs: Layers, Assets, Tools, Viewport, Inspector, Solver. Channels and Bindings are stubs until M2.
- [ ] **Step 3: `theme.rs`** — dark neutral palette, `Rounding: 4.0`, generous spacing, Inter + JetBrains Mono via `FontDefinitions`, and the `labelled_slider` / `section` / `danger_button` wrappers so consistency is structural rather than remembered.
- [ ] **Step 4:** Spawn a plain window camera **before** the offscreen viewport camera. `bevy_egui`'s auto-created primary context attaches to the first camera spawned; attaching it to the offscreen camera produces a wgpu validation error from a format mismatch. Found the hard way in M0-2.

---

## Task 7: `animus-editor` — the viewport

> **DONE 2026-08-17** — `crates/animus-editor/src/viewport/`. Split into an egui half and a camera half, because M0-2 proved the maths can be right while the gate is wrong. Step 5's synthesised-input test exists: clicks are driven through a bare `egui::Context` at 1.0/1.25/1.5/2.0 display scales and land within 1px. Removing `.sense(click_and_drag())` — the original M0-2 bug — fails three of them.

M0-2 is the reference implementation for this task, including its three fixes. Reread `docs/spikes/m0-2-egui-viewport.md` before starting.

**Files:** `src/viewport/{mod.rs,camera.rs,input.rs}`

- [ ] **Step 1:** `Camera3d` → `RenderTarget::Image`, registered with `egui_user_textures`, drawn with `ui.add(Image::new(..).sense(Sense::click_and_drag()))`.
- [ ] **Step 2: Input gates are `response.hovered()` / `response.clicked()` / `response.dragged_by(..)`.** Never `ctx.wants_pointer_input()`.
- [ ] **Step 3: Resize immediately** — round to whole physical pixels, multiply by `scale_factor()`, clamp to ≥ 1. No debounce.
- [ ] **Step 4: Pan and zoom.** Zoom-to-cursor unprojects before and after the scale change in the same frame. Pan converts pointer delta with a **probed** world-per-pixel — unproject two points 100 px apart — never with a formula derived from `OrthographicProjection::scale`, whose meaning depends on `ScalingMode` and which was wrong by 28× in the spike.
- [ ] **Step 5: A click-accuracy test that synthesises pointer events** through egui's raw input, asserting the unprojected world position within 1 px at 1×, 2× and 4× zoom. This is the automated version of the check M0-2 left to a human, and it is the reason this plan can trust the viewport without a person present.
- [ ] **Step 6:** Report world-per-pixel and cursor world position in a status strip. It costs nothing and it is what makes every later coordinate bug diagnosable.

---

## Task 8: `animus-editor` — import to puppet

> **DONE 2026-08-17** — `animus-core/src/image_in/pipeline.rs` (Bevy-free, the whole import testable from an in-memory PNG) and `animus-editor/src/import.rs`. One `ImportImage` command carries asset, layer and puppet, so a bad import is one Ctrl+Z. Errors are refused before the asset store is touched, so a rejected import leaves nothing on disk.

The user-visible spine of M1: image in, puppet out.

**Files:** `src/import.rs`, `src/panels/assets.rs`

- [ ] **Step 1:** Drag-and-drop and a file dialog; import through `animus_project::AssetStore` so the asset is content-addressed.
- [ ] **Step 2: Reject early and clearly.** An opaque or transparent image never reaches triangulation; the import dialog shows the `ImportError` message and stops. This is the surface a matte step would later plug into.
- [ ] **Step 3:** `silhouette::extract` → `triangulate` → `MeshData`, with the `AutoMeshParams` controls (alpha threshold, close radius, RDP epsilon, min region area, interior spacing) live-previewed against the actual mesh.
- [ ] **Step 4:** The whole import is **one** `ImportImage` snapshot command, so a bad import is one Ctrl+Z.
- [ ] **Step 5:** Failure paths as messages, never panics: fully transparent image, fully opaque image with no matte, image larger than the GPU's max texture size, unsupported format — name the extension and say PNG is what M1 reads.

---

## Task 9: `animus-editor` — rigging tools and gizmos

> **DONE 2026-08-17** — `animus-core` gained `SetSkeleton` (one snapshot command for every rig edit, because deleting a joint cascades); `animus-editor/src/rig.rs` holds the tool logic as pure functions (egui-free, 7 tests) and `gizmos.rs` draws the rig on layer 1 in ink-ramp colours. Weight painting stays in M6, as planned; a wireframe budget of 8k triangles guards the §10.3 warning. Step 4's measurement at 10k vertices still needs a live scene — deferred to the moment the app first runs.

- [ ] **Step 1: Joint and bone placement tools** — click to place a joint, drag joint-to-joint to create a bone, with snapping to existing joints.
- [ ] **Step 2: `skeleton::auto_attach`** with a live attachment-radius gizmo, and a re-attach command after skeleton edits.
- [ ] **Step 3: Gizmos on `RenderLayers::layer(1)`** — bones, joints, mesh wireframe, attachment radii, selection. Layer 1 keeps them off the projector; M0-4 confirmed the isolation works.
- [ ] **Step 4: Measure the wireframe.** §10.3 warns that 10k vertices is 30k line segments per frame. Measure it at 10k and cache a `LineList` mesh per mesh revision if it does not hold up.
- [ ] **Step 5:** Weight painting is **not** in M1. Auto-attach with an adjustable radius is the whole rigging story for this milestone.

---

## Task 10: `animus-editor` — dragging, in both modes

The moment the tool becomes convincing, and the place the one-way projection rule earns its keep.

- [ ] **Step 1: Edit mode** — dragging a joint emits `MoveJointRest`, merged into one undo entry per drag.
- [ ] **Step 2: Live mode** — dragging writes a *target* into `TargetValues`; the solver honours it as a constraint and the mesh follows. **Never write `Transform` directly.** What the operator sees is then how the puppet will actually behave on stage.
- [ ] **Step 3: Failing test** — a synthesised drag in live mode leaves `Project` byte-identical while `SolverState` moves.
- [ ] **Step 4:** Release behaviour: the joint springs back unless pinned. Pinning is a toggle on the joint.

---

## Task 11: `animus-editor` — inspector, layers, undo UI

- [ ] **Step 1:** Hand-written `fn inspect_bone(ui, &Bone) -> Option<DocCommand>` per type, per §10.4. No reflection-driven editing: it mutates in place and cannot be undone.
- [ ] **Step 2:** Layer list with reordering that rewrites `depth` with even spacing (`index * 0.01`), so 2D and 3D interleave later for free.
- [ ] **Step 3:** Ctrl+Z / Ctrl+Shift+Z, a visible undo history with labels, and the stack caps enforced.
- [ ] **Step 4:** The `◎` Learn affordance renders on every inspector row but is inert until M2. Reserve the space now so the layout does not shift later.
- [ ] **Step 5:** `bevy-inspector-egui` behind a Dev toggle only.

---

## Task 12: `animus-output` — the projector window

M0-4 is the reference implementation, including its bug.

**Files:** `crates/animus-output/` — `lib.rs`, `window.rs`, `monitor.rs`

- [ ] **Step 1: Monitor selection by `PrimaryMonitor`, never by enumeration order.** `monitors.iter().nth(1)` put the output fullscreen on the operator's own screen, under a borderless window with a hidden cursor that cannot be dragged away. Enumeration order does not put the primary first.
- [ ] **Step 2: An explicit monitor override** in the UI and on the CLI. At a venue the display that reports as primary is not always the projector.
- [ ] **Step 3: Vsync as an operator-facing toggle**, defaulting to on. Measured: an output window synced to a 30 Hz display clamps the **whole application** to ~29 fps, and the editor's own present mode changes nothing. The trade-off is the operator's to make.
- [ ] **Step 4: `RenderLayers` isolation** — assert in a test that no `EditorOnly` entity is visible to the output camera, and keep the human check in the release checklist.
- [ ] **Step 5:** Esc closes the output window and despawns its camera cleanly. Verified in M0-4; keep a test that the entity count returns to baseline.
- [ ] **Step 6:** Log the chosen monitor's name, resolution, refresh rate and scale factor at startup. That one line is what turns a venue problem into a five-second diagnosis.

---

## Task 13: `animus-app` — the binary

- [ ] **Step 1:** `animus [project.animus]`, `--perform`, `--output-monitor <index>`, `--no-vsync`.
- [ ] **Step 2: Autosave** to a sidecar on a timer and before every risky operation, with recovery on next launch. Required in M1 per §18.
- [ ] **Step 3: Panic handling** — a panic in a subsystem must not take down a show. Catch, log, surface, and keep the output window alive.
- [ ] **Step 4:** Plugin wiring and the schedule from §8.2, with `PerformanceMode(true)` skipping every editor system.

---

## Task 14: Round-trip and the fixtures

- [ ] **Step 1:** Save a rigged puppet through the real UI, reload, and assert the reloaded `Project` equals the saved one — including `IdAlloc.next`, so reopening and adding a joint cannot collide with an existing ID.
- [ ] **Step 2:** Golden fixtures in `spec/fixtures/m1-*/` with deliberately non-contiguous IDs.
- [ ] **Step 3:** A migration test: hand-write a v1 file, load it through the chain, assert it lands in the current shape.
- [ ] **Step 4:** Assert that a saved project contains no NaN or Inf, using a corrupted in-memory project as the negative case.

---

## Task 15: The done-when test

§18 sets the bar as a person, not a metric: *someone who has never seen the app can, in under 10 minutes with no instructions beyond tooltips, import a drawing, rig an arm, wave it with the mouse, and see it on a projector — and reopening the saved project reproduces it.*

- [ ] **Step 1:** Write the tooltips first, then attempt the flow yourself against the clock, from a cold start, using one of the JPEGs rather than a friendly PNG.
- [ ] **Step 2:** Record where it stalls. Fix the top three stalls before showing anyone.
- [ ] **Step 3:** Then run it with a real person and watch without helping. The instinct to explain is the thing being tested.

---

## Done Criteria for This Plan

- [ ] A PNG with alpha becomes a rigged, deformable puppet; everything else fails with a message naming the fix
- [ ] Dragging a joint in live mode deforms the mesh organically and leaves the document untouched
- [ ] The puppet renders on a second display with no editor gizmos, at the display's refresh rate, with vsync switchable
- [ ] Save, quit, reopen — identical project, including ID allocation state
- [ ] Undo returns to the initial state after an arbitrary command sequence (proptest)
- [ ] `cargo tree -p animus-core | grep bevy` still fails
- [ ] Click accuracy is asserted by a test that synthesises input, not by a human
- [ ] The 10-minute flow has been attempted by someone other than its author

## Deferred into this milestone's shadow, deliberately

- M0-2's DPI matrix (100/125/150% × 1/2/4×) and M0-4's 60 fps run with the editor in use — both need hardware that was not available. Recorded in `docs/spikes/`.
- Weight painting, manual mesh editing, blend modes, Spout, NDI, recording, OSC, MIDI, audio, 3D models. All have their own milestones.

## Next Plan

M2 — the signal bus, OSC and MIDI, Learn, and `--perform`.
