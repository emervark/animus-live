# Animus Live project format — version 1

Status: current. Schema version `1`.

This document specifies the on-disk format an Animus Live project is
stored in, independently of any particular implementation, so that other
tools can read and write it. It is released under CC0-1.0 (see
`LICENSE` in this directory) precisely so that no legal review is needed
to implement a reader or writer.

The reference implementation lives in the `animus-project` crate of the
[Animus Live](https://github.com/) source tree, but this document does not
assume you have read that code.

## 1. Overview

A project is a directory, conventionally named `<Show Name>.animus/`,
containing:

```
MyShow.animus/
  project.json          # the document: everything but asset bytes
  assets/
    <sha[0..2]>/
      <sha256>.<ext>     # one file per distinct asset, by content hash
      ...
    ...
```

`project.json` is UTF-8 text, pretty-printed with 2-space indentation and
stable key ordering (the order fields are declared in this document).
Pretty-printing and stable ordering are deliberate: the file is meant to
be readable and diffable in git, and a re-save of an unmodified project
must produce a byte-identical file — no spurious diffs from map
reordering or formatting churn.

There is no single-file archive format. A project is always a plain
directory; nothing prevents a tool from zipping one up for transport, but
that zip is not itself "the format" — unzip it and it's a directory like
any other.

## 2. Coordinate conventions

Every pixel-space position, mesh vertex, or UV coordinate in this format
uses **image space: pixels, origin top-left, +Y down**. This matches how
image files themselves are addressed and how most 2D authoring tools work,
but does not match OpenGL/wgpu's usual +Y-up NDC convention — a renderer
consuming this format must account for that at its own render boundary.
There is nothing in `project.json` itself that identifies this convention
implicitly; it is asserted here because a reader has no other way to know.

## 3. Versioning and migration policy

`project.json`'s top-level `schema_version` field is a single
monotonically increasing integer, currently `1`.

- A reader that finds `schema_version` **greater** than the highest
  version it supports must refuse to load the file with a clear error.
  It must never guess at an unknown format — attempting to interpret
  fields from a future schema version risks silent misinterpretation of
  a performer's show.
- A reader that finds `schema_version` **less** than its current version
  is expected to run a migration chain that upgrades the parsed document,
  version by version, to the reader's current schema before use. (As of
  schema version 1, no such chain exists yet, since there is no version 0
  to migrate from.)
- A reader must **ignore unknown fields** at every level. This is what
  lets a file written by a newer minor revision within the same major
  schema version still load in an older reader that hasn't been updated
  yet: it silently drops fields it doesn't understand rather than
  rejecting the file outright. Only a `schema_version` bump gates
  structural changes a reader cannot safely ignore.
- A breaking change to the format (removing a field's old meaning,
  changing a field's type, restructuring a table) bumps
  `schema_version` and is accompanied by a migration from the previous
  version.

## 4. `project.json` schema

All field names are written exactly as they appear in the JSON. Optional
fields (marked "optional") may be omitted, in which case the stated
default applies; a reader must accept both presence-with-default-value
and absence.

Numbers: every floating-point number in this document must be finite. A
writer must reject `NaN` or `±Infinity` rather than write them — silently
writing `null` in their place (which is what a naive JSON serializer
does) would corrupt the file in a way that only surfaces as a confusing
error much later, when the file is loaded and some unrelated field turns
out to have the wrong JSON type.

IDs (`schema_version`'s neighbors like `next_id`, and every `*_id`-typed
value below) are non-negative integers. In *value* position (a field
whose value is an ID, e.g. a `Layer`'s `id`, or a puppet's `texture`)
they are written as plain JSON integers. In *key* position — the
`assets`, `layer_data`, and `puppets` maps, and a skeleton's `joints` and
`bones` maps, all of which are keyed by ID — they are written as decimal
numeric strings (`"10"`, not `10`), because JSON object keys are always
strings; there is no other representation available. A conforming reader
must accept both forms: a plain integer wherever an ID appears as a
value, and a numeric string wherever an ID appears as a map key. `0` is
reserved and never appears as an in-use ID — it means "unset" in contexts
that allow it. IDs are never reused within a project: once allocated
(tracked by the project-wide counter `next_id`), an ID is never handed
out again, even if the entity it named is deleted, so a stale reference
is detectably dangling rather than silently pointing at a different,
later object.

### 4.1 Top level

| Field | Type | Meaning |
|---|---|---|
| `schema_version` | integer | See §3. Currently `1`. |
| `meta` | object | See §4.2. |
| `next_id` | integer | Next unused ID; every ID in this file must be `< next_id`. |
| `assets` | object (map, asset id → object) | Asset metadata table. See §4.4. Keys are asset IDs as strings (JSON object keys are always strings), values as in §4.4. |
| `layers` | array of integer | Paint order of layer IDs. Index 0 is the **back** of the scene; later entries paint on top. |
| `layer_data` | object (map, layer id → object) | Per-layer data. See §4.5. Every ID in `layers` must have a matching key here. |
| `puppets` | object (map, puppet id → object) | Puppet definitions. See §4.6. |
| `bindings` | array | Signal-bus bindings. Reserved for a future milestone; currently an array of opaque objects a reader should preserve but need not interpret. Optional, defaults to `[]`. |
| `solver` | object | Project-wide spring-solver configuration. See §4.7. |
| `stage` | object | Output canvas configuration. See §4.8. |

### 4.2 `meta`

| Field | Type | Meaning |
|---|---|---|
| `name` | string | The show's display name. |
| `created_by` | string | Free-form provenance string, e.g. `"animus 0.1.0"`. |
| `created_utc` | string | ISO-8601 UTC timestamp of first save. |
| `modified_utc` | string | ISO-8601 UTC timestamp of the most recent save. A writer sets this at save time; the codec itself does not touch it — writing exactly what it is given is what makes round-trip comparison possible. |

### 4.3 Asset kind

An asset's `kind` (used in §4.4) is one of the strings:

- `"image"` — a raster image (PNG or similar), used as a `MeshPuppet`'s texture.
- `"gltf"` — an embedded glTF model (`.glb` recommended), used by a `ModelPuppet`.
- `"font"` — a font file, reserved for text layers.

### 4.4 `assets[id]` — `AssetRef`

| Field | Type | Meaning |
|---|---|---|
| `id` | integer | This asset's ID (also the map key, duplicated on the value for convenience). |
| `sha256` | string | Lowercase hex SHA-256 of the asset's bytes. Determines the file's location on disk — see §5. |
| `kind` | string | One of §4.3. |
| `original_name` | string | The filename the user imported this asset under. **Display only** — never used to locate the file. Renaming or moving the source file after import does not affect this value or the asset's resolvability. |
| `byte_len` | integer | Size of the asset's bytes, for UI display and integrity checks. |
| `width` | integer or `null` | Pixel width, for image/font-adjacent previews. Optional, defaults to `null`. |
| `height` | integer or `null` | Pixel height. Optional, defaults to `null`. |

### 4.5 `layer_data[id]` — `Layer`

| Field | Type | Meaning |
|---|---|---|
| `id` | integer | This layer's ID. |
| `name` | string | Display name. |
| `visible` | boolean | Whether the layer composites at all. |
| `opacity` | number (0..1) | Layer-wide opacity multiplier. |
| `blend` | string | One of `"normal"`, `"add"`, `"multiply"`, `"screen"`. |
| `depth` | number | Authoritative world Z. Used to interleave 2D layers with 3D glTF models sharing the scene. |
| `transform` | object | See below — a tagged union. |
| `locked` | boolean | Whether the layer ignores the pointer. Optional, defaults to `false`. |
| `contents` | array of integer | Puppet IDs this layer displays. |

`locked` is **not** `visible` inverted, and both are needed. `visible`
answers "does the audience see it"; `locked` answers "can the operator
grab it". A backdrop is normally visible and locked: on screen, worked
over, and never picked up by accident. A reader that predates the field
treats a file without it as unlocked, which is what every such file
meant.

`transform` is one of two shapes, **externally tagged**: the object has
exactly one key, `"flat"` or `"spatial"`, whose value holds the fields
below. (Externally tagged, not an inline/internal tag and not
`translation`-dimensionality sniffing — every enum in this format, this
one included, uses one consistent representation; see the note in the
field table for other enums below.)

- Flat (2D): `{ "flat": { "translation": [x, y], "rotation": <radians>, "scale": [x, y] } }`
- Spatial (3D): `{ "spatial": { "translation": [x, y, z], "rotation": [x, y, z, w], "scale": [x, y, z] } }` — `rotation` is a quaternion.

### 4.6 `puppets[id]` — `Puppet`

| Field | Type | Meaning |
|---|---|---|
| `id` | integer | This puppet's ID. |
| `name` | string | Display name. |
| `kind` | object | `{ "type": "mesh", ... }` or `{ "type": "model", ... }` — see below. |

#### 4.6.1 `kind` = `mesh` — `MeshPuppet`

| Field | Type | Meaning |
|---|---|---|
| `texture` | integer | Asset ID of the puppet's texture image. |
| `matte` | object | `{ "mode": "use_image_alpha" }` — where the silhouette's alpha comes from. Optional; omitted means `use_image_alpha`, so files written before this field existed load unchanged. **`mode` is an open set by design:** a reader that meets an unknown mode must report it as an unsupported project rather than guessing, and a writer may only emit modes it implements. New alpha sources are therefore new *values* here, never a schema change, and adding one needs no migration. |
| `mesh` | object | See below. |
| `skeleton` | object | See below. |
| `attachments` | object | `{ "entries": [ ... ] }`, see below. |
| `material` | object | `{ "tint": [r,g,b,a], "alpha_mode": "blend" \| "mask" }`. `tint` multiplies the texture; alpha scales opacity on top of the layer's own. `"mask"` is a hard cutout that both occludes and is occluded by 3D content; `"blend"` is soft alpha blending that never occludes 3D. |
| `solver_override` | object or `null` | Per-puppet override of §4.7's shape. `null`/omitted means "use the project's `solver`". Optional, defaults to `null`. |

`mesh`:

| Field | Type | Meaning |
|---|---|---|
| `positions` | array of `[x, y]` | Rest vertex positions, image space (§2). |
| `uvs` | array of `[u, v]` | Normalized 0..1, Y down — matches image-space convention directly. |
| `triangles` | array of integer | Flat index triples into `positions`/`uvs`, counter-clockwise in image space. Length is always a multiple of 3. |
| `source` | object | Provenance: `"manual"` or `{ "auto": { "alpha_threshold": u8, "close_radius": u32, "rdp_epsilon_px": number, "min_region_area_px": number, "interior_spacing_px": number, "mode": "silhouette" \| "convex_hull" \| "bounding_box" \| "grid" } }`. Lets a tool re-run auto-meshing reproducibly. |

`skeleton` is a spring **graph**, not a hierarchy — bones name their two
endpoint joints directly, with no parent/child relationship:

| Field | Type | Meaning |
|---|---|---|
| `joints` | object (map, joint id → object) | `{ "id", "name", "rest": [x,y], "rest_angle": number (optional, default 0), "inv_mass": number, "pinned": boolean (optional, default false) }`. `inv_mass = 0` means pinned (infinite mass) even if `pinned` is also false. `rest` is image space. |
| `bones` | object (map, bone id → object) | `{ "id", "name", "a": <joint id>, "b": <joint id>, "rest_length": number or null (optional; null means "compute from joints' rest positions"), "stiffness": number, "damping": number, "length_mul": number (optional, default 1.0 — squash/stretch multiplier, animatable), "attach_radius": number }`. `damping` is reserved for a future per-bone damping model; it is ignored by the v1 reference implementation, which applies only `solver.global_damping` to every bone uniformly. |

`attachments`: `{ "entries": [ { "vertex": <index into mesh.positions>, "bone": <bone id>, "weight": number, "local": [x, y] } , ... ] }`, sorted by `(vertex, bone)` for deterministic output. `local` is the vertex's rest position expressed in the bone's own local frame (the frame defined by the bone's A→B direction), recorded at bind time. This table is authored truth and may bind a vertex to more bones than a realtime skinning palette can hold; reducing to a bounded set is a separate baking step outside this format.

#### 4.6.2 `kind` = `model` — `ModelPuppet`

| Field | Type | Meaning |
|---|---|---|
| `asset` | integer | Asset ID of the embedded glTF/GLB model. |
| `scene_index` | integer | Which scene within the glTF to use. Optional, defaults to `0`. |
| `animation` | string or `null` | Name of the glTF animation clip to play, if any. Optional, defaults to `null`. |
| `driven_joints` | array of object | `{ "node_name": string, "channel": string }` — glTF skeleton nodes whose transform is overridden live from the signal bus, by name. Channel routing is untyped for now. Optional, defaults to `[]`. |

### 4.7 `solver` — `SolverConfig`

Also the shape of any `mesh_puppet.solver_override`.

| Field | Type | Meaning |
|---|---|---|
| `hz` | integer | Solver step rate. Default `120`. |
| `iterations` | integer | Constraint-relaxation passes per step. Deliberately incomplete convergence (4-8) is the intended feel. Default `8`. |
| `gravity` | `[x, y]` | Default `[0, 0]`. |
| `global_damping` | number | Default `0.98`. |
| `rest_pull` | number (0..1) | How far every unpinned, undriven joint is moved toward its rest position each step, as a fraction of the remaining distance. Default `0.08`. Optional: a v1 file written before this field existed omits it, and a reader **must** treat a missing value as the default rather than as `0`. `0` means a joint keeps whatever pose it was last left in; the rest position is otherwise the pose a puppet returns to when nothing is driving it. |
| `max_substeps_per_frame` | integer | Caps how many substeps one slow frame may run, so a stall can't trigger a burst of catch-up simulation. Default `8`. |
| `enabled` | boolean | Default `true`. |

### 4.8 `stage` — `StageConfig`

| Field | Type | Meaning |
|---|---|---|
| `canvas` | `[width, height]` (integers) | Output canvas size in pixels. Default `[1920, 1080]`. |
| `background` | `[r, g, b, a]` (numbers, 0..1) | Default `[0, 0, 0, 1]` (opaque black). |

### 4.9 Worked example

Prose drifts from the code that actually produces a wire format — the
"tagged union" shapes documented for `transform` (§4.5) and `mesh.source`
(§4.6.1) have both, historically, been wrong in an earlier draft of this
document. Do not trust prose alone for anything load-bearing; trust the
committed, generated example.

[`fixtures/sample-project/project.json`](fixtures/sample-project/project.json)
is a complete, non-trivial `project.json` — two layers (one with the
default flat 2D `transform`, one with a spatial 3D `transform`), one
content-addressed image asset, and one mesh puppet with a two-joint
one-bone skeleton, an attachment, and `mesh.source` using
`MeshSource::Auto` provenance. It is **normative**: generated by running
the real `animus-project` codec (`crates/animus-project/tests/
spec_fixture.rs`), not hand-written, and a test in that file
(`spec_worked_example_matches_the_committed_fixture`) regenerates it on
every `cargo test` run and fails the build if the two diverge by a single
byte. If this document's prose and the fixture ever disagree, the fixture
is correct and the prose has a bug.

## 5. Content-addressed assets

Every asset's bytes live at:

```
assets/<sha256[0..2]>/<sha256>.<ext>
```

where `sha256` is the lowercase hex SHA-256 digest of the asset's raw
bytes (the same value recorded in `assets[id].sha256`), and `ext` is a
fixed extension chosen from the asset's `kind` (`image` → `png`, `gltf`
→ `glb`, `font` → `ttf`) — not necessarily the extension of whatever file
the asset was originally imported from. `assets[id].original_name` is
retained separately, purely for the UI to show the user something
recognizable; it plays no role in locating the file.

Splitting on the hash's first two hex characters keeps any one directory
from accumulating thousands of entries in a large show, the same
convention used by git's object store and most content-addressed caches.

Two consequences of content addressing, both load-bearing for this
format's design goals:

- **Deduplication is automatic and free.** If the same bytes are
  imported under two different names (or the same puppet's texture is
  referenced twice), only one file is ever written; both `AssetRef`s
  share a `sha256` and therefore a path.
- **`project.json` never churns because a path changed.** A file's
  location is a pure function of its content, so renaming the source
  file the user imported from, or moving the whole project directory,
  never invalidates or rewrites any path recorded in `project.json`.

A reader that finds an `AssetRef` whose `assets/<prefix>/<hash>.<ext>`
file is missing should treat that as a specific, reportable error (a
"missing asset" condition) rather than a generic file-not-found — the
project is still structurally valid, just missing one payload, and a UI
can usefully say which asset and offer to relink it.

## 6. What this format deliberately does not specify

- Any particular renderer's internal representation. This is a
  *storage* format; converting `project.json` into GPU buffers, a scene
  graph, or anything else is entirely up to the consumer.
- Undo history, autosave cadence, or crash-recovery file naming. Those
  are application concerns layered on top of "atomically write
  `project.json`", not part of the format itself.
- A single-file (zipped) variant. Nothing precludes a tool from offering
  "export as .zip" for sharing, but the canonical, load-bearing form is
  always the directory described in §1.

## 7. License

This specification is released under the [Creative Commons CC0 1.0
Universal](./LICENSE) public domain dedication. You may implement a
reader or writer for this format, in any language, for any purpose,
without asking permission or attributing this document.
