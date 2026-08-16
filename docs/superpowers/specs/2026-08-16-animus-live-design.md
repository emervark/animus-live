# Animus Live — Design Specification

**Date:** 2026-08-16
**Status:** Approved for planning
**Working directory:** `C:\dev\animata` (repository root; folder may be renamed — it is empty and not yet a git repo)

---

## 1. Context

[Animata](http://animata.kibu.hu/) (Kitchen Budapest, 2007 — Péter Németh, Gábor Papp, Bence Samu) is a real-time puppet animation tool for live visual accompaniment at concerts, theatre and dance. Its workflow: import a still image as a texture, place vertices, triangulate into a mesh, build a bone skeleton, attach bones to mesh vertices with weights, let a mass-spring physical model make the movement organic, drive joints in real time via OSC from external software and sensors, composite layers, and project fullscreen.

It is written in C++ with FLTK 1.1 and SCons, and has been effectively unmaintained since around 2010.

**Animus Live** is a modern, clean-room spiritual successor. It keeps Animata's core idea — a spring-driven cutout puppet driven live by external signals — and adds what a 2026 tool should have: rigged 3D glTF models from Sketchfab/Mixamo alongside the 2D puppets in one scene, built-in camera/audio/MIDI input, and Spout/NDI/video output for VJ integration.

**Intended outcome:** an open-source desktop application, Windows-first (macOS/Linux later), that a VJ or theatre visual artist can use to build and perform puppet shows.

### 1.1 Non-negotiable constraints

| Constraint | Decision |
|---|---|
| Stack | Rust + Bevy 0.19.1, used as a **library**, not as an engine container. The user explicitly rejected Godot on these grounds. |
| Platform | Windows first; macOS and Linux later. |
| Scope | 2D cutout puppets **and** rigged 3D glTF models in one unified scene, depth-interleaved. |
| Live inputs | Camera/body tracking, OSC in, MIDI/keyboard/gamepad, audio/mic analysis — all through one named-channel signal bus. |
| Outputs | Borderless fullscreen on a chosen monitor, Spout sender, NDI sender, video file export. |
| 2D mesh creation | Automatic (alpha silhouette + constrained Delaunay) **and** manual vertex/triangle editing. |
| 3D models | Drag & drop `.glb`/`.gltf` first; Sketchfab API search post-1.0. |
| Project character | Open source on GitHub. The code is written by an AI agent; contributor accessibility still matters. |
| First vertical slice | 2D puppet end-to-end: image → mesh → bones → physics → mouse drag → fullscreen output. |

### 1.2 Scope of this document

This spec covers the whole product through 1.0 so the architecture is decided once. **The first implementation plan covers M0 and M1 only** (§18); later milestones get their own plans.

Small helper types referenced but not defined here — `Transform2Or3`, `MaterialCfg`, `StageConfig`, `DrivenJoint`, `BlendMode`, `AssetKind`, `AutoMeshMode`, `Curve`, `BindMode`, `Smoothing`, `SmoothState`, `TargetPath`, `MeshDefect`, `DocChange`, `PixFmt` — are defined during implementation. Their shape follows from the types that use them; none of them carries an unresolved design decision.

The repository root is `C:\dev\animata` for now. It is empty and not yet a git repo, so renaming it to `animus` before the first commit is free and is the recommended first step.

### 1.3 Clean-room requirement — operational, not aspirational

The original Animata is GPL. Animus Live is a reimplementation, not a fork, and is therefore unencumbered **provided the project behaves accordingly**:

- **Nobody writing code for this project reads the original C++ source.** This applies to AI agents: original Animata source must never be pasted into a prompt. Reading the README, the file listing (names only), published papers, documentation, screenshots and videos is fine and is the intended reference material.
- `CONTRIBUTING.md` states this as a rule.
- `docs/heritage.md` records what was drawn from the original — the mass-spring model, the joints-as-graph-not-hierarchy idea, the OSC-driven workflow — and credits Kitchen Budapest and the original authors by name.

---

## 2. Verified ground truth

Every version claim below was checked against crates.io / docs.rs / bevy.org in August 2026. Anything unverified is marked **SPIKE** and must not be assumed.

| Claim | Status |
|---|---|
| Bevy 0.19 shipped 2026-06-19; **0.19.1** is current | verified |
| Bevy 0.19 uses **wgpu 29.0.3** | verified |
| `SkinnedMesh` / `SkinnedMeshInverseBindposes` in `bevy::mesh::skinning` | verified |
| Joints are a **flat `Vec<Entity>`**; skinning reads their global transforms; **parenting is not required** | verified |
| **4 influences per vertex**; `ATTRIBUTE_JOINT_INDEX` must be `VertexAttributeValues::Uint16x4` | verified |
| `MAX_JOINTS = 256`, hard-coded, not configurable without forking Bevy | verified (bevy#17128) |
| `Mesh::with_generated_skinned_mesh_bounds()` + `DynamicSkinnedMeshBounds` — new in 0.19, **required** or skinned meshes are mis-culled | verified |
| GPU readback is built in: `bevy::render::gpu_readback::{Readback, ReadbackComplete, GpuReadbackPlugin}` — `bevy_image_export` is **not** needed | verified |
| `MonitorSelection::{Current, Primary, Index, Entity}`; `WindowMode::BorderlessFullscreen(MonitorSelection)` | verified |
| 0.19 replaced `RenderGraph` with ECS schedules ("Render Graph as Systems") | verified |
| 0.19: `#[derive(Resource)]` now also implements `Component`; broad queries need `Without<IsResource>` | verified |
| `wgpu::Texture::as_hal::<A>()` exists in 29.0.3, supports Dx12, requires the `wgpu_core` feature | verified |
| Spout shares via **D3D11** textures; `spoutDX12` bridges D3D12 through **D3D11On12** (GPU-side copy, not literally zero-copy, but no PCIe readback) | verified |
| `spout2-rs` 0.1.1 — BSD-2, vendors Spout SDK 2.007.017 statically, has `dx` / `dx12` / `gl` backends | verified |
| `grafton-ndi` 0.13.0 (NDI 6 SDK) is the live Rust NDI binding | verified |
| `spade` 2.15.1 has real `ConstrainedDelaunayTriangulation` | verified |

**SPIKE — unverified:** whether `wgpu_hal::dx12::Texture` exposes a public raw `ID3D12Resource` accessor in 29.0.3. The module is Windows-gated and docs.rs builds on Linux, so it 404s. Must be checked locally with `cargo doc --target x86_64-pc-windows-msvc -p wgpu-hal`.

### 2.1 The egui version lockstep

Only one `egui` may exist in the dependency graph or the types do not interoperate. The verified consistent set for Bevy 0.19 is:

```
bevy 0.19.1 + egui 0.34 + bevy_egui 0.40 + bevy-inspector-egui 0.37 + egui_dock 0.19.1
```

`bevy-inspector-egui` 0.37 is the laggard and pins the whole set to egui 0.34. Do **not** reach for `bevy_egui` 0.41 (egui 0.35) or `egui_dock` 0.20/0.21. This table must be rechecked before every dependency bump; it is the most fragile line in the manifest.

---

## 3. Architecture

### 3.1 Workspace layout

Strictly downward dependency flow. Everything above `animus-runtime` is the **Bevy-free zone** — roughly 65% of the code, 90% of the logic, and 100% of the tests that matter. It compiles and tests on Linux CI with no GPU in under a minute.

```
animus/
├─ Cargo.toml                 # [workspace], resolver = "3", shared [workspace.dependencies]
├─ Cargo.lock                 # COMMITTED — this is an application
├─ rust-toolchain.toml        # pinned stable
├─ deny.toml                  # cargo-deny: license allowlist + advisories
├─ spec/                      # CC0 file-format specification (own LICENSE)
│  ├─ animus-project-format-v1.md
│  └─ fixtures/               # golden project directories used by tests
├─ crates/
│  ├─ animus-core/            # ── NO BEVY ── geometry, document model, solver
│  ├─ animus-project/         # ── NO BEVY ── filesystem, zip, sha256 CAS, migrations
│  ├─ animus-signal/          # ── NO BEVY ── channels, bindings, filters, curves
│  ├─ animus-sources/         # ── NO BEVY ── OSC/MIDI/audio input threads
│  ├─ animus-runtime/         # bevy: doc→ECS projection, skinning build, solver driver
│  ├─ animus-output/          # bevy + wgpu: output window, frame tap, Spout, NDI, recorder
│  ├─ animus-editor/          # bevy + egui: dock, viewport, tools, gizmos, undo
│  └─ animus-app/             # the binary; CLI, plugin wiring, panic handling
├─ spikes/                    # M0 throwaway binaries
└─ xtask/                     # release packaging, fixture regeneration
```

Legal dependency edges — and only these:

```
core      ← project, signal, runtime, editor
project   ← runtime, editor, app
signal    ← sources, runtime, editor
sources   ← runtime, app
runtime   ← output, editor, app
output    ← app
editor    ← app
```

`animus-core` depends on `glam`, `serde`, `spade`, `i_overlay`, `thiserror` and nothing else. `glam` is exactly the type Bevy uses (`Vec2`/`Vec3`/`Mat4` are re-exports), so core types cross into Bevy at zero cost with no conversion layer.

**Enforced in CI:** `cargo tree -p animus-core | grep -q bevy` must *fail*. The architectural invariant is a test.

### 3.2 `animus-core` module map

```
src/
├─ lib.rs
├─ ids.rs               PuppetId/LayerId/BoneId/JointId/AssetId (newtype u64) + IdAlloc
├─ doc/
│  ├─ mod.rs            Project, ProjectMeta
│  ├─ layer.rs          Layer, BlendMode
│  ├─ puppet.rs         Puppet, PuppetKind{Mesh,Model}
│  ├─ mesh_puppet.rs    MeshData, SkeletonData, AttachmentTable
│  ├─ model_puppet.rs   ModelPuppet (glTF)
│  ├─ asset.rs          AssetRef (sha256 + original name + kind)
│  └─ solver_cfg.rs     SolverConfig
├─ remap.rs             IndexRemap + Remappable trait   ← delete-safety mechanism
├─ mesh/
│  ├─ edit.rs           add/remove vertices & triangles, remap application
│  └─ invariants.rs     validate() -> Vec<MeshDefect>
├─ silhouette/
│  ├─ alpha.rs          threshold, dilate/erode closing
│  ├─ marching.rs       marching squares → rings
│  ├─ rdp.rs            Ramer–Douglas–Peucker simplification
│  ├─ topology.rs       outer vs hole classification, winding normalization
│  └─ fallback.rs       convex hull, bounding box, grid
├─ triangulate/
│  ├─ cdt.rs            spade CDT over boundary + holes + interior points
│  ├─ points.rs         Poisson-disc interior point sampling
│  └─ filter.rs         centroid-in-polygon, zero-area rejection
├─ skeleton/
│  ├─ rig.rs            bones as spring edges, joint rest positions
│  ├─ attach.rs         radius falloff → authored weights (unbounded influence count)
│  └─ bake.rs           authored → top-4 normalized GPU influences
├─ solver/
│  ├─ state.rs          SolverState (SoA: pos, prev, inv_mass, pinned)
│  ├─ step.rs           Verlet integrate + Gauss–Seidel relaxation
│  ├─ compiled.rs       CompiledRig (immutable, Arc-shared into the ECS)
│  └─ guard.rs          NaN/Inf detection → reset_to_rest()
├─ target.rs            TargetPath parsing/formatting ("puppet/7/joint/3/pos.x")
└─ migrate/
   ├─ mod.rs            migrate(value, from, to) chain
   └─ v1_to_v2.rs       placeholder — the chain exists from day one
```

---

## 4. Data model

### 4.1 IDs versus indices

Two mechanisms, chosen per entity type by cardinality:

- **Stable `u64` IDs** for anything the user names, selects, binds to, or that must survive a session: layers, puppets, bones, joints, assets, bindings. Allocated from a monotonic `IdAlloc` stored in the project as `next_id`. **IDs are never reused**, so a stale reference is detectably dangling rather than silently wrong. Stored in `IndexMap<Id, T>` — insertion-ordered, so JSON diffs are stable.
- **Dense `u32` indices** for vertices and triangles only, because there are 10³–10⁴ of them and they are referenced from three places. Structure-of-arrays; no per-vertex ID overhead.

### 4.2 Vertex deletion safety — solved by the compiler, not by discipline

This is historically the most bug-prone area in this class of software. The design makes forgetting a referrer a **compile error**.

```rust
/// Produced by any structural edit that removes vertices.
pub struct IndexRemap {
    old_to_new: Vec<Option<u32>>,   // None == deleted
    new_len: u32,
}

impl IndexRemap {
    pub fn map(&self, old: u32) -> Option<u32>;
    pub fn is_deleted(&self, old: u32) -> bool;
}

/// Every struct that stores a vertex index MUST implement this.
pub trait Remappable {
    fn remap_vertices(&mut self, r: &IndexRemap);
}
```

`MeshData::remove_vertices(&mut self, victims: &[u32]) -> IndexRemap` is the *only* deletion path and is private to `mesh::edit`. The public API is `MeshPuppet::remove_vertices`, which computes the remap and then calls `remap_vertices` on every field via **exhaustive destructuring without `..`**:

```rust
let MeshPuppet { mesh, attachments, selection, pins, uv_overrides } = self;
```

Adding a new field that holds vertex indices then fails to compile until it is handled. Triangles referencing a deleted vertex are dropped, not remapped.

### 4.3 Types

```rust
pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub schema_version: u32,
    pub meta: ProjectMeta,
    pub next_id: u64,
    pub assets: IndexMap<AssetId, AssetRef>,
    pub layers: Vec<LayerId>,                     // paint order
    pub layer_data: IndexMap<LayerId, Layer>,
    pub puppets: IndexMap<PuppetId, Puppet>,
    pub channels: IndexMap<String, ChannelDef>,   // remembered, not authoritative
    pub bindings: Vec<Binding>,
    pub solver: SolverConfig,
    pub stage: StageConfig,                       // canvas size, background, camera
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    pub created_by: String,      // "animus 0.4.2"
    pub created_utc: String,     // RFC3339
    pub modified_utc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetRef {
    pub id: AssetId,
    pub sha256: String,          // file at assets/<sha[0..2]>/<sha>.<ext>
    pub kind: AssetKind,         // Image | Gltf | Font
    pub original_name: String,   // UI only
    pub byte_len: u64,
    #[serde(default)] pub width: Option<u32>,
    #[serde(default)] pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub blend: BlendMode,        // Normal | Add | Multiply | Screen
    pub depth: f32,              // world Z — this is how 2D interleaves with 3D
    pub transform: Transform2Or3,
    pub contents: Vec<PuppetId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Puppet { pub id: PuppetId, pub name: String, pub kind: PuppetKind }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PuppetKind { Mesh(MeshPuppet), Model(ModelPuppet) }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshPuppet {
    pub texture: AssetId,
    pub mesh: MeshData,
    pub skeleton: SkeletonData,
    pub attachments: AttachmentTable,
    pub material: MaterialCfg,
    #[serde(default)] pub solver_override: Option<SolverConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPuppet {
    pub asset: AssetId,
    #[serde(default)] pub scene_index: usize,
    #[serde(default)] pub animation: Option<String>,
    #[serde(default)] pub driven_joints: Vec<DrivenJoint>,   // glTF joints driven from the bus
}

// ── Mesh: structure of arrays ──────────────────────────────────────────
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MeshData {
    pub positions: Vec<Vec2>,    // REST positions, IMAGE SPACE: pixels, origin top-left, Y down
    pub uvs: Vec<Vec2>,          // normalized 0..1, Y down — matches wgpu directly
    pub triangles: Vec<u32>,     // flat triples, CCW in image space
    pub source: MeshSource,      // provenance so "re-run auto-mesh" is reproducible
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeshSource { Manual, Auto(AutoMeshParams) }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoMeshParams {
    pub alpha_threshold: u8,       // default 8
    pub close_radius: u32,         // dilate-then-erode, default 2
    pub rdp_epsilon_px: f32,       // default 2.0
    pub min_region_area_px: f32,   // default 64.0 — kills speckle
    pub interior_spacing_px: f32,  // Poisson-disc radius, default 40.0
    pub mode: AutoMeshMode,        // Silhouette | ConvexHull | BoundingBox | Grid
}

// ── Skeleton: a GRAPH of springs, NOT a hierarchy ──────────────────────
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkeletonData {
    pub joints: IndexMap<JointId, Joint>,
    pub bones: IndexMap<BoneId, Bone>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Joint {
    pub id: JointId,
    pub name: String,
    pub rest: Vec2,                          // image space
    #[serde(default)] pub rest_angle: f32,   // radians, image space
    pub inv_mass: f32,                       // 0.0 == pinned
    #[serde(default)] pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bone {
    pub id: BoneId,
    pub name: String,
    pub a: JointId,
    pub b: JointId,
    #[serde(default)] pub rest_length: Option<f32>,   // None → computed from rest positions
    pub stiffness: f32,                               // 0..1
    pub damping: f32,                                 // 0..1
    #[serde(default = "one")] pub length_mul: f32,    // squash/stretch — animatable
    pub attach_radius: f32,
}

// ── Attachments: authored truth, unbounded influence count ─────────────
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AttachmentTable {
    pub entries: Vec<Attachment>,   // sorted by (vertex, bone) for deterministic output
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub vertex: u32,
    pub bone: BoneId,
    pub weight: f32,
    pub local: Vec2,   // vertex rest position in THIS bone's local frame at bind time
}
impl Remappable for AttachmentTable { /* drop entries whose vertex was deleted */ }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverConfig {
    pub hz: u32,                      // 120
    pub iterations: u32,              // 4..8 — INCOMPLETE convergence is the feel
    pub gravity: Vec2,
    pub global_damping: f32,          // 0.98
    pub max_substeps_per_frame: u32,  // 8 — accumulator clamp
    pub enabled: bool,
}
```

**Serde conventions, non-negotiable:**
- `deny_unknown_fields` is **off** — forward compatibility matters more than strictness.
- Every optional field carries `#[serde(default)]` so a v1 reader survives a v1.1 writer.
- Floats serialize through a helper that **rejects NaN/Inf at write time** — a NaN in a saved project is a corrupted show.
- `serde_json::to_writer_pretty`, 2-space indent, `IndexMap` throughout → git-diffable.

---

## 5. File format

```
MyShow.animus/
├─ project.json                 # schema_version is the first key
├─ assets/
│  ├─ 3f/3f8a…c1.png
│  └─ 9b/9b21…ee.glb
└─ .backup/
   └─ project.20260816-141233.json    # rolling 20, autosave every 2 min when dirty
```

Distribution format: `MyShow.animus.zip` — the same tree, zipped. Open handles both transparently.

**Safe save:** write `project.json.tmp` → `fsync` → atomic `rename`. Assets are written under their content hash, so a partially written asset has the wrong name and is ignored. `Project::save` never mutates in place.

**Migration chain:** `migrate::run(json: Value, from: u32) -> Result<Value>` applies `v1_to_v2`, `v2_to_v3`, … in order on the raw JSON *before* typed construction. Each step ships with a fixture pair in `spec/fixtures/` and an `insta` snapshot test. **A schema version bump without a fixture fails CI.** The chain and one no-op migration exist from day one so the mechanism is exercised before it is needed.

A file with a newer `schema_version` than the reader is refused with a clear message, never guessed at.

**The format spec (`spec/`) is licensed CC0-1.0**, separately from the code, so a competing tool can implement a reader with zero legal analysis.

---

## 6. Geometry pipeline

### 6.1 Silhouette extraction

1. Threshold the alpha channel at `alpha_threshold`.
2. **Dilate-then-erode closing** at `close_radius` — this is the cheap fix for anti-aliased-edge speckle and is not optional.
3. Marching squares → closed rings. Written in-house (~150 lines); the `contour` crate is stale (Apr 2024) and we need control over closing, hole classification and winding anyway.
4. Ramer–Douglas–Peucker simplification at `rdp_epsilon_px`.
5. Classify rings: largest by area = outer boundary; contained rings = holes. Normalize winding.
6. Drop regions below `min_region_area_px`.

### 6.2 Triangulation — `spade` CDT

`spade` 2.15.1 provides real constrained Delaunay, which is a genuine improvement over the heuristic approach:

1. Insert boundary and hole ring vertices, then `add_constraint_edge` for every ring segment. The CDT now **guarantees** the silhouette outline appears as mesh edges — no more "the triangulation cut the corner off the character's ear".
2. Insert interior sample points (Poisson-disc at `interior_spacing_px`) as free vertices. Poisson-disc rather than a grid, because a regular lattice produces visible artifacts when the mesh deforms.
3. Filter by centroid-in-polygon — now only to remove triangles inside *holes* and in the concave exterior, which CDT does not do for you. A cheap correctness post-pass, not the primary mechanism.
4. Reject near-zero-area triangles (`|cross| < eps`); normalize winding to CCW.

**Fallback ladder** — the user is never blocked. If constraint insertion fails (self-intersecting silhouette after RDP): (a) retry with a smaller RDP epsilon, (b) `i_overlay` self-union to remove self-intersections, (c) convex hull, (d) bounding-box grid. A toast reports which mode was used.

---

## 7. 2D puppet → Bevy skinned mesh

**Everything renders through the 3D pipeline.** Bevy's 2D path (`Mesh2d`/`bevy_sprite`) does not support skinning, and 2D puppets must depth-interleave with glTF models anyway. So: `Mesh3d` + `MeshMaterial3d` + `SkinnedMesh`, with an orthographic `Camera3d` by default. `bevy_sprite` is therefore not in the feature list.

### 7.1 Coordinate handling

Image space is pixels, origin top-left, **Y down**. World space is Y up.

```rust
fn img_to_world(p: Vec2, pivot: Vec2, ppu: f32) -> Vec3 {
    Vec3::new((p.x - pivot.x) / ppu, -(p.y - pivot.y) / ppu, 0.0)
}
```

**Only positions flip. UVs do not** — image-space `(u,v) = (x/w, y/h)` with v increasing downward is already wgpu's convention, so naive UV assignment is correct. Getting this backwards is the classic first bug; the M0-1 spike asserts it by rendering a texture with a visible "TOP" label.

Angles negate: `world_angle = -image_angle`.

The Y flip reverses triangle winding. Fix by setting **`cull_mode: None`** on the material — we want double-sided anyway, since a limb can flip over — rather than by reversing indices, which invites the bug to return after the next mesh edit.

### 7.2 Mesh construction

```rust
pub fn build_skinned_mesh(mp: &MeshPuppet, ppu: f32, pivot: Vec2) -> Result<Mesh, BuildError> {
    let n = mp.mesh.positions.len();
    let baked = skeleton::bake::bake_influences(&mp.attachments, &mp.skeleton, n)?;

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, /* img_to_world for each */);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, /* mp.mesh.uvs */);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n]);
    // MUST be explicitly Uint16x4 — [u16;4] is ambiguous with Unorm16x4
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX,
        VertexAttributeValues::Uint16x4(baked.joint_index));
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, baked.joint_weight);
    mesh.insert_indices(Indices::U32(mp.mesh.triangles.clone()));
    // New in 0.19 — without this, skinned meshes are frustum-culled wrongly
    Ok(mesh.with_generated_skinned_mesh_bounds()?)
}
```

**Limits enforced in `validate()` with a user-facing message, never a panic:**
- ≤ **256 bones per puppet** — Bevy's `MAX_JOINTS` counts entries in `SkinnedMesh.joints`, and per §7.3 those are our bones. We target 10–60. If a user ever exceeds it, the bounded workaround is splitting into two skinned meshes sharing one solver.
- ≤ **4 influences per vertex** on the GPU. Authored attachments may exceed 4; `bake_influences` takes the top 4 by weight, renormalizes, and reports the maximum dropped mass so the UI can warn "vertex 812 lost 18% of its influence".
- A mesh carrying `ATTRIBUTE_JOINT_INDEX` **without** a `SkinnedMesh` component panics at render time (bevy#22469). The sync system must spawn both in the same `commands` batch, never across frames.

### 7.3 Bind poses — the skinning palette is per BONE, not per joint

Bevy skinning computes `Σ_j w_j · (GlobalTransform_j · inverse_bindpose_j) · v`. It reads the entities in `SkinnedMesh.joints` by their **global** transforms and never inspects parentage, so a flat list with no `ChildOf` between them is exactly what Animata's spring-graph skeleton needs.

**The entities in that list are our `Bone`s, not our `Joint`s.** This is the point most likely to be got wrong, so it is worth stating plainly:

- A `Joint` is a point mass. It has a position and no meaningful orientation of its own — in a spring graph, nothing defines which way a joint "faces".
- A `Bone` spans two joints and therefore *does* have a frame: origin at joint A, X axis along A→B, Y perpendicular. That frame is what rotates when a limb swings.
- `Attachment.local` is already recorded in the **bone's** frame (§4.3). Linear blend skinning over bone frames is exactly Animata's vertex blend.

So the mapping is: **one skinning entity per bone.** Consequently **Bevy's `MAX_JOINTS = 256` limits bones per puppet**, not joints — which is the number `validate()` and `bake_influences` must check.

```rust
// Bind transform of BONE b in puppet-local space at rest.
let a = img_to_world(joints[bone.a].rest, pivot, ppu).truncate();
let b = img_to_world(joints[bone.b].rest, pivot, ppu).truncate();
let dir = b - a;
let bind = Mat4::from_rotation_translation(
    Quat::from_rotation_z(dir.y.atan2(dir.x)),
    a.extend(0.0),
);
inverse_bindposes.push(bind.inverse());
```

Each frame the solver produces joint positions; a writeback system derives each bone entity's `Transform` from its two joints the same way — translation at A, Z rotation from the A→B direction. `length_mul` squash/stretch appears as a scale along the bone's local X.

`Joint::rest_angle` therefore plays no part in skinning. It is retained only so a joint can carry an authored orientation for future tools (IK hints, driven rotations); if it is still unused at 1.0, remove it.

Bone entities are spawned as **children of the puppet root** (so layer transforms move the whole puppet for free) but are **siblings of each other**. The solver writes them in puppet-local space; `TransformSystems::Propagate` composes the root transform in.

### 7.4 Material and depth ordering

M1: `StandardMaterial` with `unlit: true`, `base_color_texture`, `cull_mode: None`, `double_sided: true`, opacity via `base_color` alpha.

The alpha mode is a real tradeoff and is exposed per layer:

| Mode | Phase | Writes depth | 3D occludes it | It occludes 3D | Edges |
|---|---|---|---|---|---|
| `AlphaMode::Blend` | Transparent, sorted back-to-front | no | yes | **no** | soft, correct AA |
| `AlphaMode::Mask(0.5)` | AlphaMask, depth pre-pass | yes | yes | **yes** | hard, aliased |

Default to **`Blend`** (better edges, matches Animata's look) with a per-layer **"Occludes 3D"** toggle that switches to `Mask`. Use `depth_bias` to break coplanar ties.

**Layer ordering rule:** `layer.depth` is authoritative world Z. The layer list UI reorders by rewriting depths with even spacing (`depth = index * 0.01`), so 2D-among-3D just works — put a 3D model at Z = 0.05 and the layer list shows it between the layers at 0.04 and 0.06.

Post-M1, replace `StandardMaterial` with a custom `AnimusMaterial: Material` for Add/Screen/Multiply blend modes. **Small spike:** confirm a custom `Material` still inherits the skinning vertex path in 0.19's schedule-based renderer.

---

## 8. ECS design

### 8.1 Resources, components, events

**Resources**

| Resource | Purpose |
|---|---|
| `DocumentRes(Project)` | **The single truth.** Never written from entities. |
| `DocRevision { global, per_puppet }` | dirty tracking |
| `PendingChanges(Vec<DocChange>)` | drained by the sync system each frame |
| `UndoStack { done, undone }` | |
| `ChannelBus` | live channel values, One Euro state, activity meters |
| `TargetValues(HashMap<TargetPath, f32>)` | binding output, consumed by the solver |
| `SignalRx(crossbeam::Receiver<SignalPacket>)` | drained in `PreUpdate` |
| `EntityIndex { puppets, joints, layers }` | doc ↔ world bridge |
| `EditorState` | tool mode, selection, dock layout, viewport image handle |
| `OutputConfig` | monitor choice, resolution, enabled sinks |
| `PerformanceMode(bool)` | `--perform` — skips all editor systems |

**Components**

| Component | On | Purpose |
|---|---|---|
| `PuppetRoot(PuppetId)` | puppet root | |
| `CompiledRigRef(Arc<CompiledRig>)` | puppet root | immutable solver input, read lock-free |
| `PuppetSolver(SolverState)` | puppet root | **mutable state — this is what enables parallelism** |
| `JointOf { puppet, joint, index }` | joint entity | |
| `Mesh3d`, `MeshMaterial3d`, `SkinnedMesh`, `DynamicSkinnedMeshBounds` | puppet mesh entity | |
| `EditorOnly` + `RenderLayers::layer(1)` | gizmos, helpers | keeps them off the projector |
| `OutputCamera` / `EditorViewportCamera` | cameras | |

**Events:** `DocChanged`, `LearnCaptured`, `SolverPanic(PuppetId)`, `FrameTapReady`, `RequestRebuildMesh(PuppetId)`.

**Bevy 0.19 gotcha:** `#[derive(Resource)]` now also implements `Component`, and resources live as components on abstract entities. Any broad query (`Query<Entity>`, `Query<()>`) must add `Without<IsResource>` or it will pick up resource entities. This bites "select all in scene" code — documented in the contributor guide.

### 8.2 Schedule

```
PreUpdate
  ├─ SignalSet::Drain        SignalRx → ChannelBus (bounded, no allocation)
  └─ SignalSet::Filter       One Euro / lowpass per channel; activity meters
RunFixedMainLoop  (Time<Fixed> at SolverConfig.hz, default 120)
  └─ FixedUpdate
       ├─ SolveSet::Bindings   Binding → TargetValues
       ├─ SolveSet::Apply      TargetValues → pinned joint targets, bone length_mul
       ├─ SolveSet::Step       par_iter_mut over puppets: Verlet + Gauss–Seidel
       └─ SolveSet::Guard      NaN/Inf → reset_to_rest + SolverPanic event
Update
  ├─ SyncSet::Apply           drain PendingChanges → spawn/despawn/rebuild entities
  ├─ EditorSet::Ui            egui: dock, panels, viewport, tool interaction
  └─ EditorSet::Commands      tool output → DocCommand → UndoStack → PendingChanges
PostUpdate
  ├─ SolveSet::Writeback      interpolated solver state → joint Transform
  │                             .before(TransformSystems::Propagate)
  ├─ EditorSet::Gizmos        bones/mesh/handles into Gizmos (RenderLayer 1)
  └─ OutputSet::Tap           queue Readback on the output image if any sink is active
Last
  └─ OutputSet::Dispatch      ReadbackComplete → fan out to Spout/NDI/Recorder
```

### 8.3 Fixed timestep — use Bevy's, don't roll our own

`FixedUpdate` already provides the clamped accumulator this design needs:

- `Time::<Fixed>::from_hz(120.0)` sets the timestep.
- `Time::<Virtual>::max_delta` clamps the frame delta *before* the accumulator — the spiral-of-death guard. Set it to `max_substeps_per_frame / hz` (8/120 ≈ 66 ms) so a hitch drops simulation time rather than stalling the render thread. **For a live show, dropping simulation time is always the right call.**
- `Time::<Fixed>::overstep_fraction()` gives the interpolation alpha, which is needed anyway.

Consequence to be aware of: `RunFixedMainLoop` sits **between `PreUpdate` and `Update`**, so signal ingest must be in `PreUpdate` (it is) and UI-driven document edits land one fixed tick late. Imperceptible at 120 Hz, and the correct tradeoff versus running the solver after the UI.

### 8.4 Interpolation

Solver at 120 Hz, display at 60/144/240 — without interpolation there is visible beating. `SolverState` keeps `pos` and `pos_prev_tick` (distinct from Verlet's `prev`); writeback lerps by `overstep_fraction()`.

### 8.5 Per-puppet parallelism

Because `PuppetSolver` is a **component**, not a field of a resource:

```rust
fn step_solvers(
    time: Res<Time<Fixed>>,
    targets: Res<TargetValues>,
    mut q: Query<(&CompiledRigRef, &mut PuppetSolver)>,
) {
    let dt = time.delta_secs();
    q.par_iter_mut().for_each(|(rig, mut st)| {
        animus_core::solver::step(&rig.0, &mut st.0, &targets.view(), dt);
    });
}
```

`CompiledRig` is `Arc`'d and immutable — sync builds a fresh one and swaps the `Arc`, so readers never lock. `TargetValues` is read-only during the step. With 10–60 joints per puppet the parallelism only matters past ~50 puppets, but it costs nothing to get right up front.

### 8.6 The one-way projection rule

`DocumentRes` → entities, **never back**. Enforcement:

1. `DocumentRes` is mutable only through `apply_command(&mut Project, cmd) -> PendingChanges`. Systems take `Res<DocumentRes>`, never `ResMut`, except the single `EditorSet::Commands` system.
2. Dragging a joint in the viewport does **not** write `Transform`. In edit mode it emits `DocCommand::SetJointRest`; in live mode it writes a *target* into `TargetValues`, which the solver honours as a constraint. Visual feedback comes from the solver responding — which is also more honest about how the puppet will actually behave on stage.
3. A dev-build debug assertion recomputes expected joint entity counts from the document and panics on mismatch.

---

## 9. Solver

Position-based Verlet with Gauss–Seidel constraint relaxation.

- **Joints** are 2D point masses with position, previous position, `inv_mass` (0.0 = pinned).
- **Bones** are distance springs between two joints with rest length, stiffness 0..1, damping, and an animatable `length_mul` (Animata's squash/stretch).
- Each substep: integrate (Verlet + gravity + global damping) → apply pinned/driven targets → **iterate** bone distance constraints N times, holding pinned joints → clamp velocity as an anti-explosion guard.

Structure-of-arrays in flat `Vec<f32>` (`px, py, prev_px, prev_py`) — allocation-free.

**Constraint order must be stable** (iterate bones in index order) so the golden determinism test is meaningful and `par_iter_mut` cannot introduce order dependence.

**The incomplete convergence at 4–8 iterations with stiffness < 1 is the organic feel. Do not "fix" it with a rigid solver.**

**NaN guard:** any non-finite value in a puppet's state resets *that puppet* to rest and emits `SolverPanic`. It never propagates and never takes down the show.

---

## 10. Editor UI

Stack: `bevy_egui 0.40` (egui 0.34) + `egui_dock 0.19.1` + `bevy-inspector-egui 0.37`.

### 10.1 Layout

`egui_dock` `DockState<TabKind>` in `EditorState`, serialized to `%APPDATA%/animus/layout.json` — layout is a user preference, not project data.

```
┌──────────────┬────────────────────────────┬──────────────┐
│ Layers       │  Viewport  (render target) │ Inspector    │
│ Assets       │                            │ Solver       │
├──────────────┤                            ├──────────────┤
│ Tools        │                            │ Channels     │
│              │                            │ Bindings     │
└──────────────┴────────────────────────────┴──────────────┘
```

### 10.2 Viewport = render-to-texture inside egui

A `Camera3d` renders to `RenderTarget::Image`; the image is registered with `egui_user_textures` and drawn with `ui.image(...)` in the Viewport tab.

**Resize handling is where this goes wrong.** React to the panel rect changing, but round to whole physical pixels, multiply by `window.scale_factor()`, clamp to ≥ 1, and **debounce** (resize only if the delta exceeds 2 px or the size has been stable for 2 frames). Reallocating the render target every frame during a drag tanks the framerate and can trip wgpu validation.

**Input routing is a genuine advantage** of this approach: because the camera renders *into* the image, `camera.viewport_to_world(&xf, pixel_in_image)` is exactly correct with no overlay offset math. Compute `pixel_in_image = (egui_pointer_pos − image_rect.min) * scale_factor`. Guard all viewport input on `response.hovered() && !ctx.wants_pointer_input()`.

**Pan/zoom with zoom-to-cursor** must be done in one system with manual math — unprojecting before and after the scale change in the same frame — or a one-frame lag appears.

### 10.3 Gizmos: which tool for which job

- **Bevy `Gizmos` (world space)** for bone lines, joint circles, mesh wireframe, attachment radius circles, selection highlight, stage frame. They render through the same camera into the same target, so they are pixel-correct at every zoom, respect depth against 3D models, and are excluded from the output window via `RenderLayers::layer(1)`. Line width is in screen pixels, so bones stay legible when zoomed out. **At 10k vertices the wireframe is 30k line segments per frame — measure it; if too slow, cache a single `LineList` mesh per mesh revision.**
- **egui painter (screen space)** for text labels, the selection marquee, drag tooltips, snap indicators, and any handle that must be exactly N pixels *and* clickable by egui. Clip to the viewport `Rect`.

The dividing line: **anything the artist needs to see registered against the puppet → Gizmos. Anything that is UI chrome about the puppet → egui.**

### 10.4 Inspector

`bevy-inspector-egui` is used **only for a developer-facing "ECS Debug" tab**, gated behind a Dev toggle. Its default rendering of arbitrary reflected data is a debugging tool, not something to put in front of an artist.

The artist-facing inspector is **hand-written egui against the document types**. The decisive reason: it must emit `DocCommand`s so edits are undoable, and a reflection-driven inspector mutates in place and cannot be undone. A `fn inspect_bone(ui, &Bone) -> Option<DocCommand>` per type is ~30 lines and worth every one.

Every inspector row shows a small **"◎"** that starts Learn for that `TargetPath`. That single affordance is what makes the signal bus usable.

### 10.5 Undo/redo

```rust
pub trait DocCommand: Send + Sync + 'static {
    fn label(&self) -> &str;
    fn apply(&mut self, p: &mut Project) -> Result<PendingChanges>;
    fn revert(&mut self, p: &mut Project) -> Result<PendingChanges>;
    fn merge(&mut self, next: &dyn DocCommand) -> bool { false }
}
```

**Hybrid strategy, deliberately:**
- **Inverse-pair commands** for high-frequency small-delta ops (`MoveJointRest`, `SetBoneParam`, `MoveVertex`, `SetLayerOpacity`). These implement `merge`, so a 200-event slider drag becomes one undo step.
- **Snapshot commands** for rare, structurally sweeping ops (`Retriangulate`, `ImportImage`, `AutoRig`, `DeleteVertices`). A 10k-vertex project is ~1–2 MB in memory and these happen a handful of times per session. Writing a correct inverse for "retriangulate" is a bug factory; a snapshot is provably correct.
- Stack capped at 100 entries **or** 500 MB, whichever comes first.

Every command bumps `DocRevision` and pushes into `PendingChanges` at the finest granularity it knows (`JointMoved(p,j)`, not `PuppetChanged(p)`), so the expensive `Mesh` asset rebuild happens only when topology actually changed.

### 10.6 Theme

Do not ship default egui visuals. `animus-editor/src/theme.rs` sets a dark neutral palette, `Rounding: 4.0`, generous `item_spacing`, and installs Inter + JetBrains Mono via `egui::FontDefinitions`. Wrap common widgets (`labelled_slider`, `section`, `danger_button`) so consistency is structural. **Budget two days for this in M1, not "later"** — an artist-facing tool that looks like a debug overlay loses users before they try it.

---

## 11. Multi-window output

### 11.1 The output window

A second `Window` entity in `WindowMode::BorderlessFullscreen(MonitorSelection::Entity(monitor))` with `decorations: false`, cursor hidden, and a second `Camera3d` targeting it with `order: 10`.

**Same `World`, same puppet entities, two cameras. Nothing is duplicated.**

`RenderLayers::layer(0)` on the output camera versus `layers(&[0, 1])` on the editor camera is the entire mechanism that keeps gizmos off the projector. **Get this right on day one** — discovering it during a soundcheck is unpleasant.

### 11.2 Monitor enumeration and the manual override

`Query<(Entity, &Monitor)>` gives name, physical size, position and refresh rate for a dropdown.

**Ship a manual geometry override, unconditionally.** Bevy has a long history of monitor selection being ignored for fullscreen (bevy#5875, #5889), and projectors and scalers routinely report wrong EDID. The fallback is `WindowMode::Windowed` + `decorations: false` + `position: WindowPosition::At(IVec2)` + explicit `resolution`, with numbers typed by the user. This always works because it bypasses winit's monitor logic entirely. **In the field this is the button that saves the show.**

### 11.3 Vsync across two windows — SPIKE

Bevy renders and presents both windows in one frame loop. If the editor is on a 144 Hz panel and the projector is 60 Hz Fifo, the loop may be throttled by the slower present. Plan: editor `PresentMode::AutoNoVsync`, output `AutoVsync`. If they remain coupled, fall back to rendering the editor viewport every other frame — **the output must never drop frames.** Measured in M0-4.

### 11.4 Panic affordances

- **`Esc`** while the output window has focus → despawn the output window and camera. Bevy has no built-in `close_on_esc`; write it.
- **`Ctrl+Shift+Backspace`** (global, hard to hit accidentally) → reset all solvers to rest, clear `TargetValues`, re-enable any auto-disabled puppet. The "everything exploded" button.
- **`F1`** → minimal on-projector status overlay (FPS, active sinks, OSC packet rate). Off by default.

---

## 12. Outputs

### 12.1 One tap, many sinks

Spout (fallback path), NDI and the recorder all need the same CPU-side frame. Read back **once**:

```rust
pub trait FrameSink: Send {
    fn name(&self) -> &str;
    fn wants_frame(&self) -> bool;
    fn submit(&mut self, f: &FrameView) -> Result<(), SinkError>;
}
pub struct FrameView<'a> { pub w: u32, pub h: u32, pub fmt: PixFmt, pub bytes: &'a [u8] }
```

The output camera renders to an `Image`; a blit copies it to the window, and the same image gets one `Readback` per frame. Sinks run on **their own threads**, fed by a bounded `crossbeam` channel with **drop-newest-on-full**. A stalled NDI encoder must never stall the projector. `FrameSink` is Bevy-free, so each sink is testable with a synthetic frame.

### 12.2 Spout

**Path A — GPU-shared (target, unproven):**
1. Force the DX12 backend via `WgpuSettings { backends: Some(Backends::DX12), .. }`. Bevy may otherwise pick Vulkan on Windows, which kills this path outright.
2. Get the `GpuImage` for the output render target from `RenderAssets<GpuImage>` in the render world.
3. `unsafe { gpu_image.texture.as_hal::<wgpu_hal::api::Dx12>() }` → `wgpu_hal::dx12::Texture`.
4. Extract the raw `ID3D12Resource`; hand to `spout2-rs`'s `dx12` sender (which bridges via D3D11On12).

**Known unknowns — this is why it is a spike:**
- Whether `wgpu_hal::dx12::Texture` exposes a public raw-resource accessor in 29.0.3. **Unverified.**
- Version skew: `wgpu-hal =29.0.3` must match Bevy's exactly, *and* the `windows` crate version used for the COM types must match `wgpu-hal`'s, or the `ID3D12Resource` types are nominally distinct and will not unify. This is the most likely thing to break on every Bevy bump.
- **Resource state / barrier coordination.** The texture must be in a state D3D11On12 can acquire (`D3D12_RESOURCE_STATE_COMMON` or `ALLOW_SIMULTANEOUS_ACCESS`), which wgpu manages internally and does not expose. There may be no safe way to guarantee this. **This is the most likely reason Path A fails.**
- Frame sync: no fence hand-off, so tearing is possible; Spout's own sync may cover it.

**Path B — CPU readback (ships first, in M4):** `Readback::texture(handle)` → `ReadbackComplete` → `SpoutSender::send_image(&bgra)`.

Honest cost at **1080p60 BGRA8**:
- 1920 × 1080 × 4 = 8.29 MB/frame → **497 MB/s** of PCIe read traffic. Within an x16 link, but a real tax on a laptop dGPU.
- ~1–2 ms CPU memcpy in the readback, plus another inside Spout's `SendImage`, plus the receiver's upload. Budget **3–4 ms of CPU per frame** out of 16.6 ms.
- **Latency: 1–3 frames (16–50 ms).** Bevy's `Readback` is async; the map completes at least one frame after submission and drivers often add another. Acceptable for VJ work; **noticeable for an interactive-mirror installation where a performer sees themselves. This must be stated in the docs.**
- **At 4K60 the readback path is ~2 GB/s and ~12 ms of memcpy — not viable.** Above 1080p, Path A is mandatory. Document the supported resolutions honestly.

**Decision: implement Path B first and ship it.** Path A is an M4 optimization with a hard timebox. `FrameSink` makes it a swap, not a rewrite.

### 12.3 NDI

NDI is a network protocol; frames must be on the CPU and compressed by the SDK. There is no GPU path — always `FrameSink` + readback.

- **`grafton-ndi` 0.13.0** (NDI 6 SDK, active June 2026). Feature-gated: `--features ndi`.
- **The NDI Runtime is not redistributable.** Do not bundle `Processing.NDI.Lib.x64.dll`. Detect at startup via `NDI_RUNTIME_DIR_V6`, then the standard install path, then `libloading`. If absent, the NDI sink is greyed out with a link to ndi.video. **Spike in M4: verify whether `grafton-ndi` links at load time — if so, a missing runtime is a startup crash, and NDI must move behind a separately-loaded DLL or a sidecar process.**
- Send BGRA initially; UYVY halves bandwidth and is what NDI prefers — add later.
- NDI's compressor is CPU-heavy at 1080p60. Dedicated thread, 2-frame queue, drop on overflow. Never on the main thread.
- EULA: attribution required — "NDI® is a registered trademark of Vizrt NDI AB" in the About box and README.

### 12.4 Video export

**Pipe raw frames to an `ffmpeg` subprocess over stdin. Do not link `ffmpeg-next`.**

```
ffmpeg -y -f rawvideo -pix_fmt bgra -s 1920x1080 -r 60 -i - \
       -c:v h264_nvenc -preset p5 -cq 20 -pix_fmt yuv420p out.mp4
```

Reasons in order of weight:
1. **License hygiene.** ffmpeg builds are GPL or LGPL depending on configuration; linking entangles the MIT/Apache story and specifically conflicts with the NDI situation. A subprocess is arm's-length — the user supplies ffmpeg.
2. **Windows build pain.** `ffmpeg-next` needs ffmpeg dev libraries at build time — a large, fragile addition to CI and to every contributor's machine.
3. **Crash isolation** — an encoder crash kills a child process, not the show.
4. **User-swappable encoders** (nvenc / qsv / amf / libx264) with no rebuild.

Cost: one extra 8 MB memcpy into a pipe per frame — negligible against the readback already happening.

Detection: `ffmpeg` on `PATH`, then next to the exe, then a user-configured path. If absent, offer **PNG sequence** export (which also serves people compositing in After Effects). Recording writes to a bounded queue; if the encoder falls behind, drop frames and report the count rather than stalling the render.

**Readback uses Bevy 0.19's built-in `gpu_readback` module.** `bevy_image_export` must not be added.

---

## 13. Signal bus

### 13.1 Types (`animus-signal`, Bevy-free)

```rust
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ChannelKey(pub Arc<str>);   // "osc:/head/x", "midi:cc:1:74", "audio:band:3"

pub struct SignalPacket { pub key: ChannelKey, pub value: f32, pub at: Instant }

pub struct ChannelState {
    pub raw: f32,
    pub filtered: f32,
    pub filter: SmoothState,
    pub first_seen: Instant,
    pub last_seen: Instant,
    pub observed_min: f32,
    pub observed_max: f32,
    pub activity: f32,          // EMA of |delta| — drives Learn and the UI meter
}

pub struct ChannelBus { map: IndexMap<ChannelKey, ChannelState> }
```

`ChannelKey` is `Arc<str>`: each source caches its own `HashMap<RawAddr, ChannelKey>`, so a repeated OSC address clones an `Arc` (a refcount bump) and never allocates. **Zero allocation in the steady-state hot path.**

```rust
pub struct Binding {
    pub id: BindingId,
    pub enabled: bool,
    pub channel: String,
    pub target: TargetPath,      // "puppet/7/joint/3/pos.x"
    pub in_range: (f32, f32),
    pub out_range: (f32, f32),
    pub invert: bool,
    pub deadzone: f32,
    pub curve: Curve,            // Linear | Exp | SCurve | Steps
    pub mode: BindMode,          // Absolute | Relative | Toggle | Trigger
    pub smoothing: Smoothing,    // None | OneEuro{min_cutoff,beta} | Lowpass{alpha}
}
```

Evaluation order: `raw → normalize(in_range) → deadzone → invert → curve → remap(out_range) → clamp → smooth → apply`.

### 13.2 How sources run

Each source is an **OS thread**, not a Bevy async task. OSC/MIDI/audio have blocking or callback-driven APIs with hard latency requirements; sharing Bevy's compute task pool with the renderer risks priority inversion during a frame spike.

```rust
pub trait Source: Send {
    fn name(&self) -> &str;
    fn run(self: Box<Self>, tx: Sender<SignalPacket>, stop: Arc<AtomicBool>);
}
```

- **OSC** (`rosc` 0.11.4): blocking `recv_from` with a read timeout; decode bundles recursively; flatten `/a/b` → `osc:/a/b` (single arg) or `osc:/a/b[1]` (multi-arg). Ints/floats → f32, bools → 0/1, strings surfaced as "seen". **OSC out** as well, so channel values and puppet state can round-trip to TouchDesigner/Max.
- **MIDI** (`midir` 0.11.0): the callback runs on a MIDI thread — `try_send` only, **never allocate, never lock**. Emits `midi:cc:<ch>:<num>`, `midi:note:<ch>:<num>` (velocity), `midi:pb:<ch>`.
- **Audio** (`cpal` 0.18.1 + `realfft`): the input callback writes into an `rtrb` SPSC ring and returns immediately. A worker thread windows 1024 samples (Hann, 50% overlap), runs the FFT, and publishes `audio:rms`, `audio:peak`, `audio:band:0..7` (log-spaced), `audio:onset` (spectral flux). **No FFT in the audio callback, ever.**
- **Keyboard/gamepad**: from Bevy's own input, injected by a `PreUpdate` system — `key:w`, `pad:0:axis:lx`, `mouse:x`.

Transport: `crossbeam_channel::bounded(4096)`, `try_send`, drop-on-full with a counter surfaced in the UI. **All source threads are wrapped in `catch_unwind`** — a panicking OSC parser disables that source and shows a toast; it does not take down the show.

### 13.3 Body tracking — separate process, speaking OSC

**Decision: a sidecar process that emits OSC into our own bus.** Designed so in-process `ort` can be added later without changing anything above the bus.

Against in-process `ort`: it is still `2.0.0-rc.12` after years of RCs; ONNX Runtime is a ~200 MB native dependency with CUDA/DirectML provider DLLs that balloon the installer and add driver-version failure modes; a native inference crash takes down the show; and camera capture via `nokhwa` 0.10.10 is the least-maintained dependency in the whole plan.

For the sidecar: crash isolation, a small installer, and users can substitute **any** tracker — MediaPipe via Python, a Kinect, a phone app, TouchDesigner, Rokoko, any OSC-emitting tool. **Since OSC ingest is a hard requirement anyway, the sidecar costs zero additional code in the main app.** That is decisive. It also designs away the `nokhwa` risk entirely.

Plan: ship `animus-pose`, a small separate binary (or a documented Python script) that reads a webcam, runs a pose model, and emits `/pose/<landmark>/<x|y|z|conf>` over OSC to `127.0.0.1:9001`. The main app auto-discovers `pose:*` channels via a prefix rename rule. The app can launch and supervise it (`Child` + restart-on-exit) so it feels integrated, while remaining fully optional.

### 13.4 The Learn flow

1. User clicks "◎" next to any bindable parameter → `EditorState.learn = Some(TargetPath)`.
2. A system reports the top 3 channels by `activity` over a 1.5 s window, live, in a small popup.
3. User wiggles the fader or waves an arm; the list reorders in real time.
4. User clicks a channel (or it auto-selects after 1 s of clear dominance) → a `Binding` is created with `in_range` pre-filled from `observed_min/max`, `curve: Linear`, and `smoothing: OneEuro` for anything tagged `pose:`/`audio:`, `None` for MIDI/keys.
5. `Esc` cancels.

**Channel names are never typed.** Channels appear in the Channels panel the instant a packet arrives, with a live value bar. `ChannelDef`s persist in the project so a saved show's bindings survive a session where the source has not yet sent anything (shown greyed, "waiting").

### 13.5 One Euro filter

Implemented in-house in `animus-signal/src/filter.rs` (~40 lines) with canonical parameters (`min_cutoff` 1.0 Hz, `beta` 0.007, `d_cutoff` 1.0). Existing crates are unmaintained one-offs. Default for `pose:*` and `audio:*`; off for MIDI/keys where latency matters more than smoothness.

**The UI exposes `min_cutoff` as "Steadiness" and `beta` as "Responsiveness". Artists should never see the words "cutoff frequency".**

---

## 14. Dependencies

`[workspace.dependencies]` for one-place bumping. Status checked August 2026.

### Bevy-free core

| Purpose | Crate | Note |
|---|---|---|
| Math | `glam` | Must be the **same semver** Bevy's `bevy_math` uses, or types do not unify. Read from `cargo tree`, pin. |
| Serialization | `serde` 1, `serde_json` 1 | `preserve_order` via `indexmap` so pretty-printed diffs are stable |
| Errors | `thiserror` 2 | lib crates; `anyhow` only in `animus-app` and `xtask` |
| Logging | `tracing` | Bevy already uses it; core emits events, libs never init a logger |
| Image decode | `image` 0.25.x | pin to whatever `bevy_image` pulls, to avoid two copies |
| Triangulation | **`spade` 2.15.1** | active; real CDT |
| Polygon booleans | **`i_overlay` 8.1.0** | very active; faster and better maintained than `geo-booleanop` |
| Contours | ~~`contour` 0.13.1~~ | **stale (Apr 2024) — do not depend on it.** Write marching squares in-house. |
| Morphology | `imageproc` 0.27.0 | dilate/erode/threshold only; consider hand-rolling later to shed the dep |
| Hashing | `sha2` 0.10 | content-addressed asset store |
| Zip | `zip` 5.x | store-only for images (already compressed) |
| Property tests | `proptest` 1.x | index-remap invariants |
| Snapshot tests | `insta` 1.x | JSON round-trip / migrations |

### Live inputs

| Purpose | Crate | Status |
|---|---|---|
| OSC | `rosc` 0.11.4 | last release Mar 2025 — **stable, not stale**; OSC 1.0 does not change |
| MIDI | `midir` 0.11.0 (Apr 2026) | active; WinMM/WinRT backends |
| Audio | `cpal` 0.18.1 | active (RustAudio); WASAPI; verify loopback per device |
| FFT | `realfft` 3.x | active; ~2× faster than complex FFT for real input |
| Camera | `nokhwa` 0.10.10 | **slow-moving.** Designed away by the pose sidecar; if ever needed, contain behind our own trait |
| Pose | `ort` 2.0.0-rc.12 | "production-ready, not API-stable" — sidecar only, never in the main app |
| Gamepad | `bevy_gilrs` | built in |
| Smoothing | in-house One Euro | ~40 lines; reference test vectors from Casiez et al. |

### Outputs

| Purpose | Crate | Status |
|---|---|---|
| Spout | `spout2-rs` 0.1.1 | BSD-2; vendors Spout SDK 2.007.017 statically (no runtime DLL). **0.1.1 = immature** — wrap behind our own trait, expect to fork. |
| NDI | `grafton-ndi` 0.13.0 | active (Jun 2026), NDI 6 |
| Dynamic loading | `libloading` 0.8 | detect the NDI runtime without a hard link |
| Video | `std::process::Command` → `ffmpeg` | no crate |
| Raw HAL | `wgpu-hal` `=29.0.3` + version-matched `windows` | Windows-only, optional feature `spout-zerocopy` |

### Bevy side

```toml
bevy = { version = "=0.19.1", default-features = false, features = [
  "bevy_asset","bevy_render","bevy_core_pipeline","bevy_pbr","bevy_gltf",
  "bevy_winit","bevy_window","bevy_gizmos","bevy_gilrs","bevy_log",
  "png","jpeg","ktx2","zstd",
  "multi_threaded","x11","wayland",
  "tonemapping_luts","default_font",
] }
bevy_egui           = { version = "=0.40", default-features = false, features = ["render","manage_clipboard","open_url"] }
egui                = "=0.34"
egui_dock           = "=0.19.1"
bevy-inspector-egui = { version = "=0.37", default-features = false, features = ["bevy_render","bevy_pbr"] }
```

Deliberately excluded: `bevy_audio`, `bevy_sprite` (everything uses the 3D path), `bevy_text`/`bevy_ui` (egui does the UI), `bevy_scene`/BSN. **BSN is not used for the document** — the document is our own engine-neutral JSON format, and adopting BSN would violate that.

`cargo deny` runs in CI with an allowlist of `MIT OR Apache-2.0 / BSD-2 / BSD-3 / Zlib / ISC / Unicode-3.0`. **Anything GPL/LGPL fails the build** — this is what mechanically keeps the project clean of the original Animata's license and out of ffmpeg-linking trouble.

---

## 15. Bevy 0.x churn mitigation

### 15.1 Where breakage actually lands

| Crate | Bevy exposure | Expected churn per minor |
|---|---|---|
| `animus-core`, `-project`, `-signal`, `-sources` | **none** | **zero** |
| `animus-runtime` | components, system params, schedule labels, mesh/skinning API | low-moderate, mostly mechanical renames |
| `animus-editor` | `bevy_egui` + **egui** API | **highest** — egui breaks more per release than Bevy does |
| `animus-output` | render-world internals, wgpu version, readback API | moderate but *sharp* — 0.19's RenderGraph→schedules rewrite is exactly this class of change |

The insight worth acting on: **egui is a bigger churn source than Bevy**, and it carries the four-crate lockstep constraint from §2.1.

### 15.2 Strategy

1. **Pin exact versions** and **commit `Cargo.lock`**. This is an application; reproducible builds beat automatic patch pickup. Dependabot may open PRs but never auto-merges.
2. **Stay pinned for a whole milestone.** Never migrate mid-milestone. Adopt the *second* patch release of each Bevy minor, during a dedicated migration sprint between milestones, tracked as one issue with the official guide as a checklist.
3. **`animus-runtime/src/compat.rs`** re-exports every Bevy item the codebase touches. The rest of the code imports from `compat`, never from `bevy::` directly (enforced by a clippy `disallowed_types` lint plus a CI grep). A module move like `bevy_render::mesh::skinning` → `bevy::mesh::skinning`, which actually happened recently, then becomes a one-line change.
4. **`FrameSink`, `Source`, `DocCommand` are Bevy-free traits.** All the genuinely hard logic — Spout FFI, NDI, encoders, OSC, solver — sits behind them and cannot be touched by a Bevy release.
5. **Golden tests live in the Bevy-free crates**, so a Bevy migration never invalidates the test suite. `cargo test -p animus-core -p animus-signal -p animus-project` on Linux with no GPU is the CI gate that keeps working during a migration.
6. The `bevy_egui` compatibility table check is a manual release-checklist item and a comment in `Cargo.toml` recording the verified set.

---

## 16. Testing

Everything valuable lives in the Bevy-free crates and runs with plain `cargo test` on Linux CI without a GPU.

| Target | Kind | What it asserts |
|---|---|---|
| **Index remapping** | `proptest` | Random mesh + random attachments + random deletion set. After `remove_vertices`: every triangle index `< positions.len()`; no attachment or triangle references a deleted vertex; surviving vertices keep positions and weights; the same deletions in a different order yield the same mesh. **Highest-value test in the project.** |
| **Solver determinism** | golden | Fixed rig + fixed input for 600 ticks → hash joint positions, match a committed value bit-for-bit. Catches HashMap iteration order, f32 reassociation, and thread-order dependence in `par_iter_mut`. |
| **JSON round-trip** | `insta` | `Project → JSON → Project` is identity for a rich fixture, and the JSON snapshot is stable (catches field renames that would silently break users' files). |
| **Migrations** | fixtures | For every `vN` fixture, `migrate(vN) == vCurrent`. Adding a schema version without a fixture fails CI. |
| **Silhouette + triangulation** | invariants | Over ~20 real PNGs: every centroid inside the polygon and outside all holes; no triangle area < ε; every boundary segment present as a mesh edge (CDT constraint held); total area within 2% of silhouette area; all windings CCW. |
| **One Euro** | reference | Against published reference outputs (Casiez et al.) for a known input sequence. |
| **OSC** | table + fuzz | Address flattening, bundle recursion, type coercion, malformed-packet rejection without panic. `cargo-fuzz` target — it parses network input. |
| **Bake** | unit | Top-4 selection stable under equal weights; weights sum to 1.0 ± 1e-6; >4 influences reports dropped mass. |
| **Sinks** | unit | Each `FrameSink` accepts a synthetic 64×64 frame; backpressure drops rather than blocks. |

**Not testable — covered by `docs/release-checklist.md`**, run on real hardware before every tagged release: Spout received in OBS and Resolume; NDI received on a second machine; projector at 1080p60 and 1280×800 for 10 minutes; fullscreen on monitors 1, 2 and 3; a real MIDI controller; unplug the OSC sender mid-show; unplug the camera mid-show; Esc from fullscreen; the panic hotkey; open a project saved by the previous release.

---

## 17. Packaging and CI

### Profiles

```toml
[profile.dev]
opt-level = 1                 # our code: fast enough to run, fast enough to compile
debug = 1

[profile.dev.package."*"]
opt-level = 3                 # dependencies: compiled once, must be fast
debug = false

[profile.release]
opt-level = 3
lto = "thin"                  # "fat" adds minutes for ~2% — not worth it
codegen-units = 1
strip = "debuginfo"
panic = "unwind"              # NOT abort
```

**`panic = "unwind"` is load-bearing**, not a default: the per-subsystem `catch_unwind` that keeps a show alive when one source or sink panics depends on it.

Add a `dist` profile inheriting release with `debug = 1` and a separately archived PDB, so a crash report from a venue can be symbolicated.

### Compile times

- `bevy/dynamic_linking` behind a `dev` feature: `cargo run --features dev`. Never in release.
- `rust-lld` as the Windows linker in `.cargo/config.toml`.
- Keep `animus-core`'s dependency set tiny so `cargo test -p animus-core` is a sub-10-second loop. **This is what makes TDD on the actual logic viable, and it is the main reason the Bevy-free split pays for itself daily rather than only at migration time.**

### CI (GitHub Actions)

| Job | Runner | Content |
|---|---|---|
| `lint` | ubuntu | `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` |
| `core-tests` | ubuntu | `cargo test -p animus-core -p animus-signal -p animus-project` — no Bevy, no GPU, ~1 min. **The gate that always works.** |
| `full-build` | windows | `cargo build --workspace --release` + `cargo test --workspace` |
| `no-bevy-check` | ubuntu | `cargo tree -p animus-core \| grep -q bevy` must fail |
| `deny` | ubuntu | `cargo deny check licenses advisories bans` |
| `release` | windows, on tag | build, `cargo packager`, upload installer + portable zip + PDB |

Cache with `Swatinem/rust-cache`. Expect the Windows full build at 15–25 min cold, 3–5 warm. **NDI is not built in CI** (no redistributable SDK) — a feature flag verified only on the maintainer's machine and covered by the manual checklist. Document this so contributors are not confused by a feature they cannot compile.

### Installer

**Ship both, and treat the portable zip as primary.** VJs work on locked-down venue machines and borrowed laptops; a zip that runs from a USB stick is more valuable than an installer.

- **Portable:** `animus-live-vX.Y.Z-win64.zip` — exe + assets, no install; settings live next to the exe when a `portable.txt` marker is present.
- **Installer:** `cargo-packager` 0.11.8 producing NSIS. Handles shortcuts, uninstall, and the `.animus` file association. Fall back to `cargo-wix` if it proves flaky.
- **Code signing:** unsigned builds trigger SmartScreen, which for an artist downloading a tool is a hard stop. Budget for Azure Trusted Signing or a cheap OV cert before 1.0; until then, a prominent "click More info → Run anyway" note with a screenshot on the download page.

---

## 18. Milestones

Ordered by **risk retired per week**, not by feature appeal.

### M0 — Spikes (2–3 weeks). Nothing here ships.

Four throwaway binaries in `spikes/`. Each is done when it works **or** when its fallback is documented and chosen.

- **M0-1 Procedural skinned mesh.** 40 flat joints, a 30×30 textured grid quad, joints driven by a sine. Verify the `Uint16x4` attribute, `with_generated_skinned_mesh_bounds` + `DynamicSkinnedMeshBounds`, no parenting, correct Y flip and UV orientation (render a texture with a "TOP" label). Measure the joint-count ceiling and per-frame cost with 50 such puppets.
  *Done when:* it animates correctly and we have numbers.
- **M0-2 egui editor viewport.** `egui_dock` + a camera rendering to `RenderTarget::Image` in a dock tab; pan/zoom to cursor; a click that hits a world point within 1 px at 4× zoom on a 150% DPI display; resize without validation errors.
  *Done when:* click accuracy verified at 3 zoom levels and 2 DPI scales.
- **M0-3 Spout.** (a) Force DX12, `as_hal::<Dx12>()`, determine whether the raw `ID3D12Resource` is reachable; if yes, feed `spout2-rs` dx12 and receive in OBS. (b) **Regardless of (a)**, implement the `Readback` → `SendImage` path and measure end-to-end latency with a frame-counter overlay and a camera.
  *Done when:* OBS shows a moving frame via at least one path, with measured latency for the fallback.
- **M0-4 Second window.** Borderless fullscreen on monitor 2, second camera, `RenderLayers` isolation proven (a gizmo visible in the editor, invisible on the projector), Esc closes it, vsync coupling measured.
  *Done when:* 10 minutes at a stable 60 fps on the output while the editor is in use.

### M1 — The 2D vertical slice (6–8 weeks). **The one that matters.**

Import PNG → auto-silhouette → CDT mesh → place joints and bones → auto-attach by radius → spring solver → drag a joint and watch the mesh deform organically → fullscreen output window on the second display → save and reload.

*Done when:* a person who has never seen the app can, in under 10 minutes with no instructions beyond tooltips, import a drawing, rig an arm, wave it with the mouse, and see it on a projector — and reopening the saved project reproduces it byte-identically.

**Also required in M1, because retrofitting them is expensive:** the `DocCommand`/undo spine, the `IndexRemap` mechanism with its proptest suite, the migration chain skeleton, autosave, the NaN guard, and the theme.

### M2 — Signal bus + OSC + MIDI (4 weeks)

Channel discovery, Learn, bindings with ranges/curves/One Euro, the Channels and Bindings panels, `--perform` booting straight to fullscreen with no editor.

*Done when:* a TouchDesigner patch drives three joints on two puppets over OSC, mapped entirely by wiggling, and `animus --perform show.animus` reaches the projector in under 3 seconds with the editor never appearing.

### M3 — 3D models in the unified scene (4 weeks)

Drag-and-drop `.glb`/`.gltf`; glTF animations playing; named glTF joints bindable from the bus; 2D puppets depth-interleaved with 3D models; both alpha modes; orthographic/perspective camera toggle.

*Done when:* a Mixamo character walks *between* two 2D cutout layers with correct occlusion, and one OSC channel drives both a 2D joint and a 3D bone.

### M4 — Outputs (4 weeks)

`FrameSink` architecture, one shared readback, Spout (best available path), NDI (feature-gated, runtime-detected), ffmpeg recorder, PNG sequence fallback.

*Done when:* OBS receives Spout, a second machine receives NDI on the LAN, and a 60-second 1080p60 recording has no dropped frames — all three simultaneously, with the output window still at 60 fps.

### M5 — Live inputs completed + show hardening (5 weeks)

Audio analysis, gamepad, the pose sidecar, panic hotkey, per-subsystem `catch_unwind`, autosave verification, an allocation audit of the frame path (counting allocator in a debug build, assert zero allocations in `FixedUpdate` + `PostUpdate`), the manual release checklist.

*Done when:* a 4-hour unattended soak test with all inputs and outputs active shows no leak, no frame-time drift, and no crash — and every input source has been killed and restarted mid-run without taking down the app.

### M6 — 1.0 (6 weeks)

Manual mesh/vertex editing tools, blend modes via a custom `Material`, layer groups, project templates, Sketchfab search, the installer, the docs site, the CC0 format spec published with a reference reader, contributor guide.

*Done when:* someone other than the author has shipped a live show with it, and someone other than the author has merged a PR.

**Total: ~31–34 weeks.** M1 alone is a complete, useful, shippable tool. Everything after M1 is additive, and shipping 1.0 without Sketchfab, without manual mesh editing, or without NDI is an acceptable outcome.

---

## 19. Risks

Ranked by expected damage (likelihood × impact).

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| 1 | **A live show fails on stage** — crash, freeze, black projector, or a puppet exploding into NaN | Medium | **Catastrophic** | `--perform` mode with no editor systems; per-subsystem `catch_unwind`; NaN guard resets one puppet rather than propagating; panic hotkey; manual window-geometry override; autosave + rolling backups; zero-allocation frame path verified by a counting allocator; 4-hour soak test as an M5 gate; manual on-hardware checklist per release |
| 2 | **egui reads as a debug tool to artists** | **High** | High | Custom `Visuals` + real fonts + wrapped widgets from M1, not "later"; hand-written inspectors; `bevy-inspector-egui` confined to a hidden Dev tab; the *output* window contains no egui at all. Accept the residual — the editor will look like a tool; compensate with density and speed, which egui is genuinely good at. |
| 3 | **Tiny contributor pool** — Rust ∩ Bevy ∩ graphics ∩ live visuals is a very small set | **High** | High | The Bevy-free core is the mitigation: `animus-core` is plain Rust with plain `cargo test` and no GPU, so a contributor can fix the triangulator or the OSC parser without learning Bevy or owning a projector. **Advertise that as the entry point.** Publish the format spec CC0 so the project survives even if the app does not. Keep `good-first-issue` stocked from the core crates. |
| 4 | **Spout zero-copy unreachable through wgpu** (resource-state/barrier coordination not exposed) | **Medium-High** | Medium | Readback path implemented **first** and shipped; `FrameSink` makes zero-copy a swap; timeboxed M0-3 spike with an explicit go/no-go; honest docs about 1–3 frame latency and the 1080p ceiling |
| 5 | **Compile times destroy iteration speed** | Certain | Medium | `dynamic_linking` in dev, `rust-lld`, `opt-level=3` for deps only, and — the real fix — a fast Bevy-free core where most development actually happens |
| 6 | **egui / bevy_egui / egui_dock / inspector-egui lockstep** blocks bumps | Certain | Medium | Pin all four exactly; record the verified set in a manifest comment; treat "bump the UI stack" as its own scheduled task; be willing to sit on an old egui for a release. **Keep the `bevy-inspector-egui` dependency deliberately shallow** — dropping it costs only the Dev tab. |
| 7 | **Bevy 0.x breaking changes** | Certain | Medium | §15 in full |
| 8 | **`spout2-rs` 0.1.1 is immature**, one maintainer from abandonment | Medium | Medium | BSD-2 with a vendored SDK — forking is cheap. Wrap behind our own `SpoutSink`. The C++ Spout SDK itself is stable; worst case we bind it ourselves. |
| 9 | **`nokhwa` stagnation / camera capture breaks** | Medium | Low-Medium | **Designed away** by the pose sidecar decision (§13.3) |
| 10 | **NDI runtime linking crashes on machines without it** | Medium | Medium | Feature gate + dynamic detection; verify linkage mode in M4; if it links at load time, move NDI behind a separately-loaded DLL or a sidecar |
| 11 | **Two-window vsync coupling** | Medium | Medium | Measured in M0-4; decouple present modes, then render the editor viewport at half rate |
| 12 | **256-joint Bevy cap** | Low | Low | We target 10–60; validate with a clear message; bounded workaround is splitting into two skinned meshes sharing a solver |
| 13 | **Scope** — 2D + 3D + four input classes + four output classes | High | Medium | Milestone ordering is the mitigation: M1 alone is complete and shippable |

---

## 20. Licensing

**Application: `MIT OR Apache-2.0` (dual).**

1. It matches Bevy, wgpu and essentially the entire Rust ecosystem; any other choice creates friction on every dependency and contribution.
2. **It is required for the NDI path.** The NDI SDK is proprietary and its runtime is not redistributable. Under GPL that becomes an argument about the system-library exception with every packager; under MIT/Apache there is no argument.
3. The same applies to any future proprietary plugin, bundled commercial codec, or venue integration.
4. Apache-2.0's explicit patent grant matters for anything touching video codecs and streaming protocols.
5. It maximizes the already-small contributor pool (risk #3) — some contributors' employers forbid GPL contributions outright.

`LICENSE-MIT` + `LICENSE-APACHE` at the root, `SPDX-License-Identifier: MIT OR Apache-2.0` in `Cargo.toml`, and the standard Rust contribution clause in `CONTRIBUTING.md`.

**File-format specification: `CC0-1.0`.** `spec/` gets its own LICENSE covering the prose, the JSON schema and the fixtures, so a competing tool can implement a reader with zero legal analysis. That is the whole point of having a documented format.

The **reference implementation** (`animus-core::doc` + `migrate` + `animus-project`) stays MIT/Apache-2.0 and is **published to crates.io as standalone documented crates from M1** — that is what turns "we have a format" into "the format is real".

**Third-party obligations to track:**
- `spout2-rs` — BSD-2-Clause, © Lynn Jarvis; reproduce the notice; static linking of the vendored SDK is permitted.
- NDI — attribution required; "NDI® is a registered trademark of Vizrt NDI AB" in About and README; runtime **not** bundled.
- ffmpeg — subprocess only, so no license obligation attaches; document that the user supplies it.
- Fonts (Inter, JetBrains Mono) — both OFL; include the license files.

`cargo deny check licenses` enforces all of this on every commit. `THIRD-PARTY-NOTICES.md` is generated by `cargo-about` in CI.

---

## 21. First files to create

In this order, because each unblocks the next:

1. `crates/animus-core/src/doc/mod.rs` — the `Project` type and everything hanging off it; the entire application is a projection of this file
2. `crates/animus-core/src/remap.rs` — `IndexRemap` + `Remappable`; the mechanism that makes vertex deletion safe by construction, and the target of the highest-value test suite
3. `crates/animus-core/src/solver/step.rs` — Verlet + Gauss–Seidel; where the organic feel lives or dies
4. `crates/animus-runtime/src/skinning.rs` — `build_skinned_mesh` + `SkinnedMesh`/inverse-bindpose setup; the single point where the data model meets Bevy's GPU skinning, and the subject of spike M0-1
5. `Cargo.toml` — the workspace `[workspace.dependencies]` block carrying the exact-pinned `bevy 0.19.1 / egui 0.34 / bevy_egui 0.40 / egui_dock 0.19.1 / bevy-inspector-egui 0.37` set

---

## 22. Prerequisites on this machine

- **Rust is not installed.** Needs `rustup` (stable toolchain, `x86_64-pc-windows-msvc`).
- **Visual Studio Build Tools with the MSVC linker** — mandatory for Bevy on Windows.
- Git 2.49 is present.
- Optional for later milestones: `ffmpeg` on PATH (recording), NDI Runtime (NDI output), OBS with the Spout2 plugin (verifying Spout).
