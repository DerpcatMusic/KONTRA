# UVI packet cost and render-timer boundary

Static source review of `a410cd26acf7ea758813e64657121d0b95f3247f`. No build, test, playback, timing, emulation, decode or program census was performed. These facts describe the admitted implementation; they do not establish the cause or cost distribution of a particular playback failure.

## Production convolution call size

`Renderer` constructs `EffectProcessor` with `max_block = 1` (`src/uvi/playback.rs:629`). Its insert dispatch processes a single `Frame` (`:752`). `ImpulseProcessor` allocates scratch and wet-frame buffers at that maximum (`src/uvi/effects.rs:658`) and calls each channel convolver with the current block slice (`:836`). Thus the scratch-to-wet copy (`:841`) copies one frame in this production path. Narrowing that copy to the active slice would not reduce its current Renderer work; wider direct effect callers are a separate case.

`Convolver::new` clamps its head block to at least 32 frames (`src/fx/convolution.rs:288`). The head covers at most its first 512 IR frames. Each one-frame partial call copies its new input, recomputes the forward transform when the accumulated prefix is nonzero, and recomputes the inverse transform when current or historical spectra are nonquiet (`:128`). Silent prefixes/history already avoid transforms. Tail stages accumulate partition products progressively and run their transforms at stage boundaries (`:356`). The code therefore permits uneven per-packet work even without allocation. It supplies no evidence that those transforms caused a user's maximum render time.

Plainly batching insert calls would change the current zero-latency output and per-frame parameter/event schedule. Direct-FIR substitution or transform reuse would need bitwise waveform, error, partition, bypass/reset and history validation before adoption.

## Four-channel IRs

An IR's channel count is distinct from processing-bus width. Preparation reconstructs `kernel_channels`, applies the existing window/damping/width rules, and normalizes mean energy across every prepared kernel (`src/uvi/effects.rs:715`, `:788`). It then creates exactly one convolver per **bus** channel with kernel `bus_channel % kernel_channels` (`:807`). A four-channel IR on a stereo bus therefore retains two independent convolver states, not four. Skipping reconstruction of unused kernels can change normalization or admission/error behavior; it is not an established safe optimization.

`Convolver::process` and `Tail::process` use buffers allocated at construction (`src/fx/convolution.rs:343`, `:357`). The retained empty layer-mix map is also reused per frame; malformed unconsumed parent entries still discard that map (`src/uvi/playback.rs:2982`, `:3140`). Other allocating packet work includes Renderer output collection (`:3284`), Session command/control collections, resource installation when revision changes, and hosted completion collection. Allocation sites alone do not establish CPU attribution.

## What Worker render statistics cover

`serve` starts `Instant` after the `packet_render` phase lock and records elapsed time immediately after `Player::render_*_with_ui` returns (`src/uvi/worker.rs:1814`, `:1834`). This is wall time, not thread CPU time. Player work includes queued UI **edits**, Session processing, changed-resource installation, Renderer audio and hosted completion collection (`src/uvi/player.rs:500`, `:534`). Validation errors that return before elapsed time is recorded do not produce a render-duration entry.

The measured interval excludes:

- Request waiting and the no-request polling park (`src/uvi/worker.rs:1802`).
- Phase-lock acquisition before the timer and after rendering (`:1814`, `:1908`).
- Requested UI **snapshots**, saved-state capture and runtime-evidence serialization (`:1334`, `:1367`, `:1408`); these occur outside the timer only when their request flags are pending.
- Completion queue transfer/acknowledgement, copying the rendered Vec into fixed packet storage, and output-queue retry parks (`:1864`, `:1881`, `:1902`).

Consequently, a render mean below the 256-frame deadline does not prove worker service keeps pace. Maximum and over-budget counts describe timed attempts; neither identifies their CPU/scheduling ownership. Bridge pending-ring admission is a separate boundary. First-fault request/accepted/output frontiers and bounded whole-service CPU/wall attribution remain validation work; queue growth is not evidence of sustainable throughput.
