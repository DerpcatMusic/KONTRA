# 0.3.344 blank editor until minimize/restore

Base: `ce714f75`, branch `v2/fix-first-present-344`. No install or reference-host launch.

The full-editor Conflux witness previously returned GPU success while every
pixel of the fresh 1180x760 target was zero. Coarse reported `failed=16`
(STAGE_COARSE): blend spill needed 5,908,480 words, but its fixed capacity was
1,048,576 words. Fine work aborted; the retained renderer nevertheless accepted
and presented the empty or previous texture. Numeric receipt:
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w3-gpu-blackout-20261009/GPU_STAGE_BEFORE.json`.

An independent current-344 reproducer uses self-authored nested layers on a
fresh renderer AND fresh target. Before, one 22-deep invisible subtree gave
896,800/896,800 transparent pixels despite success; an ordinary background gave
zero transparent pixels. This avoids the false success of reusing a previously
painted target. Before PNGs are under the receipt's `before/` directory.

## Fix

Both Vello render entry points share checked coarse work. They download only
the 32-byte allocation counters, grow all seven dynamic buffers together with
their shader uniforms, and rerun coarse before fine. Power-of-two headroom handles
work revealed after an earlier stage failed. Successful capacities persist;
intermediates are freed between attempts. Eight attempts and the device's buffer
and storage-binding limits bound recovery; exhaustion returns an error without
presenting failed fine work. No scene-pixel readback or bitmap fallback was added
to production.

The Windows dispatcher previously omitted native paint/show redraw events.
`WM_PAINT` now validates its update region with BeginPaint/EndPaint and forwards
`RedrawRequested`; `WM_SHOWWINDOW` forwards that event when shown. The native
host invalidates GPU retention on expose, explicit redraw and resize. GPU Host
requires a full first frame on open, actual resize, same-size hidden→visible
restore, and surface replacement/reparent. The first-frame flag remains set
until a frame is submitted; a cached Current result cannot satisfy it.

## Targeted evidence

Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w3-first-present-344/`.

- RX6600 / Vulkan, fresh invisible and translucent (alpha 0.9) depth-22 fixtures:
  opaque first frames, visible changed frames, full-redraw encoding, and opaque
  resized first frames. Each fresh fixture reported `failed=16`, blend demand
  16,367,616 words, recovered in one retry using 16,777,216 words. Subsequent
  changed/full/resize operations needed no allocation retry. `gpu-final.log`
  captured temporary numeric logging only; that logging is removed from READY.
- Native GPU host regression proves explicit expose and same-size restore clear
  a valid cached scene and force complete encoding; actual resize does likewise.
- Allocation test checks simultaneous buffer/uniform growth and rejection of
  demands beyond the device limit.
- Native adapter test checks even a same-size resize schedules presentation.
- Renderer library: 33 passed, zero failures; three hardware tests ignored in
  the ordinary run, with the new expose/restore hardware test run separately.
- Root `cargo test --profile ci -p kontakto --lib --no-run`: PASS.

- Windows native-host cross-check: `cargo check -p kontra-native-host --target
  x86_64-pc-windows-gnu` PASS, including paint/show forwarding.
- Retained vector and solid partial/full parity: 24/24 frames exactly identical
  at scales 1, 1.5, 2; partial repaint still exercised.
- Vello allocation test also passes with `debug_layers` enabled.

Native Windows DAW first-open/restore and
scanout are UNVERIFIED here; this is a real GPU output-texture correctness
witness, not a Windows screenshot claim. Timing is CONTENDED/UNKNOWN, with no
CPU/performance claim. Synthetic PNGs persist; authored-library pixels remain
in RAM. No extra full gate was run.

NEXT: W0 nightly integration gate, then Areia scanner lifecycle and corpus delta.
