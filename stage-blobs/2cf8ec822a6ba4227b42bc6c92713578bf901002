# Partial GPU vector repaint

Base: `5fc362f355c764fd043510b3ff8f031a45328e11` (0.3.326).
Installed 0.3.306 and this base pin MUI `dcf0796082feec053af1418e3a38a302ee61da0a`.

A partial frame is encoded with the damage origin subtracted and copied back
to that origin. `GpuRenderer::encode` passed the full-window transform to
`one`, whose vector path explicitly reinstalls the supplied transform.
Vectors consequently disappear or paint in the wrong part of a damage tile.
Ordinary text, bitmap and solid paints already retain the correct canvas
transform. The fix passes the same damage-local transform to `one`.

MUI is vendored with its MIT license and explicit manifest dependencies to
keep the one-line patch reviewable and avoid editing shared Cargo checkouts.
The existing Vello image-atlas persistence patch remains in use. V1 source
`4bffbb18` pins MUI `aa1f8f3`, which predates the explicit vector transform
argument; it did not have this regression.

The real GPU failing-first example compares every pixel of four successive
nonzero-origin updates against fresh full frames at scale 1, 1.5 and 2.
Before: all twelve vector frames differ by 676, 1500 or 2636 pixels; all twelve
ordinary-fill controls match. After: all 24 frames match exactly. Each update
renders 1536, 4096 or 5120 pixels, preserving partial repaint. The adapter is
AMD Radeon RX 6600, RADV/Vulkan, Mesa 26.2.4-arch3.1. Timestamp brackets and
submission durations are observational under contention, not CPU claims.

Focused verification: 51 renderer library tests passed, 2 native-adapter
tests ignored; root `cargo test --profile ci --features shots --lib --no-run`
passed. The `gpu_damage` example is the hardware regression; its command is
in [the vendor provenance](../../vendor/mui-vello/PATCHES.md).

Numeric receipts, synthetic before/after PNGs and their hashes:
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w3-gpu-blackout-20261009/`.
This witness fixes disappearing vectors. It does **not** establish the cause
of the user's entire duplicated chrome, black rectangle, pulsing or lag;
those remain under investigation with W11's presented-window detector.
