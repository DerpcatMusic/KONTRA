# v1 evaluator boundary for the whole-voice port

Status: source review; interface awaiting W9. No timed runs or builds. Existing `6556184c` remains in the 382 sweep. New branch `v2/w6-v1-voice-evaluator` starts at `051c11c0` and does not edit W9's resample/mix/voice loop.

## Pinned source inputs and arithmetic

Reference is our `0cb7a8a0`, not a Kontakt capture. Copy the following admitted implementations rather than derive replacements:

| Consumer | Pinned source | Required preparation inputs | Caller-owned result |
|---|---|---|---|
| Ordinary amplitude / pitch envelope | `src/engine/voice.rs:158` Envelope, affine_until, skip/render | Ahdsr seconds, attack curve, sustain, AHD flag; optional immutable Flex points/sustain | one f32 gain per audio frame, or exact skip state |
| Primary native amplitude | `src/engine/ahdsr.rs:13` Source/Native | original Ahdsr f32 times/curve; output rate | f32 gain per audio frame; source recurrence at rate/32, retained destination interpolation |
| Saved retriggered sine Multi LFO | `src/engine/lfo.rs:12` Clock::positions | source slot, start phase, count/note value, sine weight, fade, depth, bypass; volume depth/sign/lag | Q32 relative source positions, optional f32 volume buffer; travel and last reach |
| External modulation | `src/engine/params.rs:116` Mod, ModTable::start/modulate; Voice::plan settled cache | ordered prepared route/source/target, f32 intensity/lag, optional shared 128-point table; note/controller/MPE inputs | amplitude multiplier, semitone offset, settled flag |

Native AHDSR must not be selected merely because a generic v2 curve resembles its law. Preparation must explicitly preserve admission and the original parameters (or prove their exact reconstruction). Kontakt currently emits an exponential curve and sample-frame durations, losing the source model tag and original setter representation. Ordinary v1 decay/release are thresholded exponential recurrences; current v2 finite normalized ramps have different observable endpoints. Preserve generic v2 semantics as the fallback instead of silently rewriting them.

## Boundary proposal awaiting agreement

W9 owns preparation integration, voice scheduling, source cursors, streaming, routing, filters, resample/mix and lane admission. W6 owns only prepared evaluator descriptors/state and their bounded render/skip/reset/release calls. No per-block format conversion, no hashed parameter identities, no second engine parameter service, no callback allocation. Existing source/catalog/control identities must remain lawful.

The proposed evaluator returns amplitude and LFO-volume arrays in storage supplied by W9 and pitch positions/offsets compatible with its copied v1 voice. State belongs to the voice slot; immutable descriptors belong to the prepared plan. Source phase, source control offset and audio interpolation offset must not advance twice when one source has multiple destinations. Bypass pauses the native source and retains its lag state; short event fragments retain the native clock. Slots beyond v1 admission use the existing v2 model.

## Bit-exact tests planned

- Native AHDSR: pinned coefficient/state/checkpoint bits for curves -1, -.5, 0, .5, 1; zero stages, arbitrary-stage release, AHD and 32-frame interpolation boundaries. Render versus skip must reach identical bits.
- Ordinary/Flex: copied eight-frame power arithmetic, threshold crossings, exact stop frame, 0/1/7/8/9/31/32/33/127/128 frames, release/reuse and immutable Flex endpoints. V1 changes grouping rounding at arbitrary split lengths; compare against the same call sequence, not an invented partition-invariance requirement.
- LFO: pinned saved-phase pitch positions and volume target checkpoints, shared pitch+volume source, positive lag, fade, bypass/resume, short fragments and preview-versus-render reach. Preserve v1's fixed-point conversion order.
- Modulation: every MIDI step, signed depth, ordered products/sums, MPE pitch bend, static/settled inputs and controller changes. Keep v1 ordinary lag distinct from native target lag (one time constant versus 99% arrival).
- Zero allocation/deallocation during render, skip, release, reset and voice reuse using the existing sampler-core allocation guard.

No new native-host parity claim; native hosts are unavailable for Horns/Morphology. Whole-cell CPU remains HOLD. No new timing until the coordinator assigns it.

NEXT: W9 concrete contract → adapter + failing-first bit tests → targeted checks/no-run → READY handoff.
