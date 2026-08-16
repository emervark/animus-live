# M0-3 — Spout, both paths

Spike crate: `spikes/m0_3_spout`. Run with:

```
cargo run --release --manifest-path spikes/m0_3_spout/Cargo.toml -- [--path-a] [--auto-close <frames>] [--width <px> --height <px>]
```

**OBS was not installed by this work** (per the task's instruction: "Do not
install OBS, receiving is the user's half"). Everything below is what could
be verified without a receiver: the wgpu-hal documentation question, DX12
backend forcing, and readback-path cost/timing measured from inside the
sender process. **End-to-end latency (Step 6, filming both screens) is
entirely the user's half — see the checklist.**

## Step 1: does `wgpu_hal::dx12::Texture` expose a public raw `ID3D12Resource` accessor in 29.0.3?

**Yes.** Read directly from the vendored source (fetched via
`cargo new --lib` + `cargo add wgpu-hal@=29.0.3` + inspecting the registry
cache, since `docs.rs` cannot build the Windows-gated `dx12` module on
Linux):

```rust
// wgpu-hal-29.0.3/src/dx12/mod.rs, line 979
pub struct Texture {
    resource: Direct3D12::ID3D12Resource,
    ...
}

impl Texture {
    pub unsafe fn raw_resource(&self) -> &Direct3D12::ID3D12Resource {
        &self.resource
    }
}
```

The accessor is `wgpu_hal::dx12::Texture::raw_resource(&self) -> &windows::Win32::Graphics::Direct3D12::ID3D12Resource`,
`unsafe fn`. `windows::core::Interface::as_raw(&self) -> *mut c_void` (from
`windows-core-0.62.2`) converts it to the raw pointer `spout2-rs`'s FFI
layer wants.

`wgpu-hal`'s exact `windows` dependency, confirmed via
`cargo tree -p wgpu-hal -i windows` on a scratch crate with
`wgpu-hal = { version = "=29.0.3", features = ["dx12"] }`:

```
windows v0.62.2
├── gpu-allocator v0.28.0
│   └── wgpu-hal v29.0.3
└── wgpu-hal v29.0.3
```

`spikes/m0_3_spout/Cargo.toml` pins `windows = "=0.62.2"` to match.

## Path A is dead anyway -- a different reason than the plan anticipated

The plan expected Path A's failure mode, if any, to be a **D3D12 resource-state
mismatch** (wgpu manages resource state internally without exposing it, and
D3D11On12 needs the texture in `D3D12_RESOURCE_STATE_COMMON` or
`ALLOW_SIMULTANEOUS_ACCESS`). That may still be true, but this spike found a
**more fundamental blocker first**, from reading `spout2-rs` 0.1.1's own
source (`spout2-rs-0.1.1/src/dx12.rs`) before writing any GPU-shared code:

**`spout2::dx12::Sender` always creates and owns its own D3D12 device.**
`Sender::new()` calls `spout_dx12_open_directx12(raw, ptr::null_mut(),
ptr::null_mut())` -- both the device and command-queue arguments are
hard-coded null, so Spout creates a private device internally. There is
**no `Sender::with_device` constructor** -- the crate exposes device sharing
only on the *receive* side (`Receiver::with_device(sender_name, device,
command_queue)`, explicitly documented: *"D3D11On12 can only wrap a resource
that belongs to the same D3D12 device it was created from"*).

D3D11On12's `CreateWrappedResource` (which `Sender::wrap_resource` calls
under the hood) requires the `ID3D12Resource` to live on the **same**
`ID3D12Device` as the D3D11On12 bridge it's being wrapped into. Bevy/wgpu
creates and owns its own `ID3D12Device` internally (confirmed via the
adapter log below); the Spout sender's device is a *different* device. So
`wrap_resource(bevy_texture_raw_ptr, ...)` would be handed a resource from
the wrong device and fail (or worse, silently misbehave) regardless of
resource state.

**Interesting wrinkle**: the underlying C shim function *does* accept a
device parameter (`spout_dx12_open_directx12(spout_dx12_t*, void* device,
void** command_queue)` in `spout2-sys-0.1.1/shim/spout_shim.h`) -- so
device-sharing on the sender side is only one FFI call away if someone
extended `spout2-rs`'s safe wrapper with a `Sender::with_device` matching
`Receiver::with_device`. That is a real path forward, but it means Path A is
not achievable with `spout2-rs` 0.1.1 **as published** -- it would require
either a fork/patch of `spout2-rs`, or dropping to `spout2_sys` unsafe FFI
directly and reimplementing what `Sender::new` does with a caller-supplied
device pointer.

**Per the plan's own instruction ("timebox to one focused attempt... Path B
ships anyway"), this was the one focused attempt: this spike does not carry
a working Path A implementation.** The `--path-a` flag exists and logs this
finding, then runs Path B anyway so the frame counter and measurable send
path exist either way.

## Did DX12 get forced?

**Yes**, confirmed by the adapter log on every run, `--path-a` or not:

```
[m0-3] adapter backend=Dx12 name="NVIDIA GeForce RTX 4080 SUPER" device_type=DiscreteGpu
```

`WgpuSettings { backends: Some(Backends::DX12), .. }` via
`RenderCreation::Automatic(Box::new(WgpuSettings { .. }))` (the plan's sketch
used a bare `WgpuSettings`; 0.19.1's `RenderCreation::Automatic` variant
takes `Box<WgpuSettings>`).

## Path B: measured cost (this machine, RTX 4080 SUPER, DX12 backend)

Cost is measured as wall time inside the `readback_and_send` observer (from
receiving the `ReadbackComplete` event's bytes to `Sender::send_image`
returning), via `std::time::Instant`, over 177 samples per run
(`--auto-close 180`):

```
$ m0-3-spout.exe --auto-close 180
[m0-3] readback+send: samples=177 avg_us=1124.5 last_us=818

$ m0-3-spout.exe --auto-close 180 --width 3840 --height 2160
[m0-3] readback+send: samples=177 avg_us=3367.1 last_us=3271
```

| Resolution | Avg readback+send cost |
|---|---|
| 1920x1080 | 1.12 ms |
| 3840x2160 (4K) | 3.37 ms |

**Reading this against spec §12.2's predictions:** the spec predicted ~3-4ms
CPU cost at 1080p (matches: 1.12ms measured is comfortably under budget) and
~12ms / "not viable" at 4K based on an assumed ~2 GB/s memcpy rate. The
**measured** 4K cost is 3.37ms, well under the "not viable" prediction. This
machine's PCIe/memory bandwidth and Spout's internal path are evidently
faster than the spec's back-of-envelope estimate. **Correction for the
user docs:** 4K CPU-readback Spout output looks viable on this class of
hardware, not just "not viable" as spec §12.2 currently states -- but see the
honesty caveat below.

**Honesty caveat:** this number is the cost of the readback-observer system
only, on one machine (RTX 4080 SUPER, PCIe 4.0/5.0-class), measured via
`Instant`, not the full end-to-end capture-to-OBS latency (that requires a
receiver and is the user's half, Step 6/7). It also does not include the
GPU-side wait for the readback to actually be ready (Bevy's
`gpu_readback` triggers the observer only once the async map-and-copy
completes, so pipeline stalls upstream of this measurement aren't captured
here). Weaker/integrated GPUs were not available to test and would likely
show materially different numbers, especially at 4K.

## Delta from the plan's API sketch

- `RenderCreation::Automatic` takes `Box<WgpuSettings>`, not a bare
  `WgpuSettings`.
- `bevy::render::render_resource::Backend` is not a path that resolves
  directly under that module in 0.19.1 in a way easy to name without extra
  digging; this spike compares `format!("{:?}", adapter_info.backend)` to
  `"Dx12"` instead of importing the enum, which is simpler and just as
  reliable for a spike.
- `Camera { target: RenderTarget::Image(...), .. }` is not a field on
  `Camera` in 0.19.1 -- `RenderTarget` is a separate component spawned
  alongside `Camera`, matching the M0-2 finding.
- `Assets<Image>` behind a `spout2::dx12::Sender` resource needs explicit
  `unsafe impl Send + Sync` -- the sender wraps a raw `*mut
  spout2_sys::ffi::spout_dx12_t`, which Rust correctly refuses to treat as
  `Send`/`Sync` automatically. All access here goes through a `Mutex`, so
  the unsafe impl is sound, but it is not implicit.
- `TextFont::font_size` is a `FontSize` enum (`Px`/`Vw`/`Vh`), not a bare
  `f32` -- `220.0.into()` works since `FontSize: From<f32>`.

## Recommendation for M4

**Ship Path B (CPU readback) as the only implemented path**, per the plan's
own default. The measured cost (1.12ms @ 1080p, 3.37ms @ 4K on this
hardware) is well within a 16.6ms (60fps) frame budget, so CPU readback is
not the bottleneck spec §12.2 worried about, at least on a discrete
mid/high-end GPU. Path A would need either a patched/forked `spout2-rs`
adding `Sender::with_device` (mirroring the existing `Receiver::with_device`),
or direct `spout2_sys` FFI reimplementing sender construction with a
caller-supplied device -- both are real engineering work, not a Bevy/wgpu
problem, and out of scope until CPU readback is shown to be a bottleneck on
real target hardware (which this spike could not test: no
integrated-GPU/older-hardware machine was available).

## User checklist — the receiving half (needs OBS + Spout2 plugin installed by the user)

- [ ] Install OBS Studio + the Spout2 plugin
      (https://github.com/Off-World-Live/obs-spout2-plugin), add a Spout2
      Capture source, and confirm it connects to `"AnimusLive-M0-3"`.
- [ ] **Visual sanity**: OBS should show the dark background and the large
      white frame-counter digits incrementing.
- [ ] **End-to-end latency (Task 4 Step 6)**: point a phone camera at both
      the Bevy window and the OBS preview in one shot, record video, step
      through frame by frame, and read the difference between the two
      visible frame-counter values. Record the frame delta and convert to
      ms at the display's refresh rate. Spec §12.2 claims 1-3 frames
      (16-50ms) -- confirm or correct it with this measurement; do not
      estimate.
- [ ] **4K visual check**: re-run with `-- --width 3840 --height 2160` and
      confirm OBS still receives a clean (non-garbled, non-dropped-frame)
      image at that resolution.
- [ ] If you have access to a machine with an integrated or older
      discrete GPU, re-run the `--auto-close` measurements above on it and
      compare -- this spike's numbers are from one high-end GPU only.
