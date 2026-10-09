# vendor/vello: MUI's patches

Upstream: [`vello` 0.10.0](https://crates.io/crates/vello/0.10.0), from
linebender/vello at commit `fc0baddd06c63287ef516180d276333aa2401e6e`
(the crate's `.cargo_vcs_info.json`; `path_in_vcs = "vello"`). The shader
crates (`vello_encoding`, `vello_shaders`) are unpatched registry 0.10.0.

Diff it against the registry copy:

```sh
diff -ru ~/.cargo/registry/src/index.crates.io-*/vello-0.10.0 vendor/vello
```

Every source patch carries a `MUI patch:` comment (`grep -rn "MUI patch" vendor/vello`).

## 1. wgpu 30 port

Upstream pins wgpu 29.0.3; MUI's workspace is on wgpu 30.

- `Cargo.toml`: `wgpu = "30.0.1"`; the `wgpu-profiler` feature and dependency
  are cut (no wgpu-30 release of it). Its `cfg`s stay in the source so upstream
  diffs stay small, hence `unexpected_cfgs = "allow"`. Upstream's clippy table
  is trimmed to what compiles clean under the workspace toolchain.
- `src/lib.rs`, `src/debug/renderer.rs`: `get_mapped_range()` returns a
  `Result` in wgpu 30; `.expect` on buffers that were just mapped.
- `src/util.rs`: `SurfaceConfiguration` gains `color_space: Auto`.
- `src/wgpu_engine.rs`: an absent vertex buffer is zero slots, not one empty slot.

## 2. Bind-group cache (`src/wgpu_engine.rs`, `BindGroupCache`)

Bind groups are kept across recordings, keyed by layout plus the bound
buffers/views (the key holds handle clones, so an address cannot be reused
while its entry lives), evicted after 4 recordings unused. Buffers go back to
the pool in reverse free order (`free_bufs` is a `Vec`, not a `HashSet`), so a
frame like the last pops the same buffers for the same proxies and hits the
cache. Adds the `rustc-hash` dependency for the per-dispatch maps.

## 3. Shared compute pass (`src/wgpu_engine.rs`, `run_recording`)

Consecutive dispatches share one `ComputePass` (`forget_lifetime`); any other
encoder command (clear, copy, upload) ends it first. Upstream opened a pass per
dispatch, which cost a hal command buffer and a usage scope each.

## 4. Image texture pool (`src/wgpu_engine.rs`, `ResourcePool::get_image`)

Images a recording frees go into `pool.images` for the next recording to reuse
(a gradient ramp is a fresh image every frame); whatever the next recording
does not take is destroyed when it ends. Upstream destroyed and re-created
them every frame (its own `TODO: have a pool`).

## 5. KONTAKTO: keep the image atlas across image-free frames (`src/render.rs`)

MUI re-encodes only the damaged part of a frame. When that part holds no image
or gradient, `Resolver::resolve` returns a 0x0 atlas, and `render_encoding_coarse`
freed the persistent atlas for a 1x1 one. The resolver's `ImageCache` still marks
its resident images clean, so the next full-size atlas came back empty and those
images never drew again, while newly seen images did. A 0x0 resolve now reuses
the persistent atlas. Grep `KONTAKTO patch`. Belongs upstream in MUI.

The atlas fix remains absent from MUI `cc4e61e6` (2026-10-04), so KONTRA
retains this override when updating the remaining MUI packages.

## 6. KONTRA: recover coarse GPU allocation before fine/present

`src/lib.rs` now reads the 32-byte GPU bump allocator after coarse work in both
render entry points. Failed stages grow their buffers and rerun coarse, with
power-of-two headroom, at most eight attempts, and the existing device storage
buffer limit. `src/render.rs` applies matching buffer and shader-uniform sizes,
and frees intermediate resources between attempts. Successful capacities are
retained; failed recovery returns an error and never presents empty/stale fine
work. Only allocator metadata is downloaded in production, never scene pixels.

The failing-first `mui-vello` `gpu_hidden` example uses a fresh target/renderer
and nested invisible or translucent layers. It requires an opaque first frame,
a changed subsequent frame, full-redraw encoding and an opaque resized frame.
