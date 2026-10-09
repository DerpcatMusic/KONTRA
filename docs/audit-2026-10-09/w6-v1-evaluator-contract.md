# v1 evaluator boundary for the whole-voice port

Status: READY support source; numeric baseline RED, nine adapter bit/heap tests GREEN and sampler-core CI compile-only gate GREEN. No timed runs. Existing `6556184c` remains in the 382 sweep. New branch `v2/w6-v1-voice-controls-381` starts at exact 381 `54d9a5c5` and does not edit W9's resample/mix/voice loop.

## Pinned source inputs and arithmetic

Reference is our `0cb7a8a0`, not a Kontakt capture. Copy the following admitted implementations rather than derive replacements:

| Consumer | Pinned source | Required preparation inputs | Caller-owned result |
|---|---|---|---|
| Ordinary amplitude / pitch envelope | `src/engine/voice.rs:158` Envelope, affine_until, skip/render | Ahdsr seconds, attack curve, sustain, AHD flag; optional immutable Flex points/sustain | one f32 gain per audio frame, or exact skip state |
| Primary native amplitude | `src/engine/ahdsr.rs:13` Source/Native | original Ahdsr f32 times/curve; output rate | f32 gain per audio frame; source recurrence at rate/32, retained destination interpolation |
| Saved retriggered sine Multi LFO | `src/engine/lfo.rs:12` Clock::positions | source slot, start phase, count/note value, sine weight, fade, depth, bypass; volume depth/sign/lag | Q32 relative source positions, optional f32 volume buffer; travel and last reach |
| External modulation | `src/engine/params.rs:116` Mod, ModTable::start/modulate; Voice::plan settled cache | ordered prepared route/source/target, f32 intensity/lag, optional shared 128-point table; note/controller/MPE inputs | amplitude multiplier, semitone offset, settled flag |

Native AHDSR must not be selected merely because a generic v2 curve resembles its law. Preparation must explicitly preserve admission and the original parameters (or prove their exact reconstruction). Kontakt currently emits an exponential curve and sample-frame durations, losing the source model tag and original setter representation. Ordinary v1 decay/release are thresholded exponential recurrences; current v2 finite normalized ramps have different observable endpoints. Preserve generic v2 semantics as the fallback instead of silently rewriting them.

## Agreed boundary

W9 owns preparation integration, voice scheduling, source cursors, streaming, routing, filters, resample/mix and lane admission. W6 owns only prepared evaluator descriptors/state and their bounded render/skip/reset/release calls. No per-block format conversion, no hashed parameter identities, no second engine parameter service, no callback allocation. Existing source/catalog/control identities must remain lawful.

The proposed evaluator returns amplitude and LFO-volume arrays in storage supplied by W9 and pitch positions/offsets compatible with its copied v1 voice. State belongs to the voice slot; immutable descriptors belong to the prepared plan. Source phase, source control offset and audio interpolation offset must not advance twice when one source has multiple destinations. Bypass pauses the native source and retains its lag state; short event fragments retain the native clock. Slots beyond v1 admission use the existing v2 model.

## Bit-exact validation

- Native AHDSR: pinned coefficient/state/checkpoint bits for curves -1, -.5, 0, .5, 1; zero stages, arbitrary-stage release, AHD and 32-frame interpolation boundaries. Render versus skip must reach identical bits.
- Ordinary/Flex: copied eight-frame power arithmetic, threshold crossings, exact stop frame, 0/1/7/8/9/17/31/32/33/77/128 frames, release/reuse and immutable Flex endpoints. V1 changes grouping rounding at arbitrary split lengths; compare against the same call sequence, not an invented partition-invariance requirement.
- LFO: pinned saved-phase pitch positions and volume target checkpoints, shared pitch+volume source, positive lag, fade, bypass/resume, short fragments and preview-versus-render reach. Preserve v1's fixed-point conversion order.
- Modulation: every MIDI step, signed depth, ordered products/sums, MPE pitch bend, static/settled inputs and controller changes. Keep v1 ordinary lag distinct from native target lag (one time constant versus 99% arrival).
- Zero allocation/deallocation during render, skip, release, reset and voice reuse using the existing sampler-core allocation guard.

No new native-host parity claim; native hosts are unavailable for Horns/Morphology. Whole-cell CPU remains HOLD. No new timing until the coordinator assigns it.

NEXT: W9 installs the module, raw descriptor preparation and whole-voice/filter callers; integration parity and CPU remain W9 acceptance work.

## Actual adapter API

`sampler-core/src/v1_voice_controls.rs` contains public raw descriptor structs, `ControlDescription`, immutable `ControlPlan::prepare(description, rate)`, fixed-size `ControlState::new(plan)` and the agreed reset/release/plan_controls/preview_pitch/render/skip/shape calls. W9 owns the library module declaration, raw IR records, preparation mapping and voice callers. Tests path-include the module so this support commit cannot change the shipped renderer by itself.

Note-start attack/release time routes use the copied v1 scaling law; descriptors without time routes copy their already prepared amplitude template at reset. Native LFO fades and volume lag coefficients are prepared once, then their template state is copied on reset. Shape returns None for native amplitude. No raw library PCM is retained. Literal source arithmetic oracles are test-only fixtures, with pinned primary-AHDSR constants providing an independent bit checkpoint.

Admission rejects more than eight voiced external routes, more than sixteen pitch envelopes, invalid or conflicting physical LFO slots and malformed shapes/times. It never truncates extra sources; W9 keeps such voices on v2. Public descriptor targets/indices retain original physical identities.

## Filter projection bridge

Pinned `src/engine/filter.rs:1287` initializes external values with `Mod::start_value`; `:1306` follows each target lag once before `:1316` hold. Hold and process read these values without advancing again. `:1426` renders ordinary module envelopes and samples at `t * CONTROL`, where `CONTROL=32`; Sustain/Done uses skip and one held level. Bypass still advances the envelope but disconnects every target, including a shaper intercept. Filter source bounds are four envelopes and eight external target values.

The public copied `Envelope::{new,render,skip,release,phase,level,done}`, `Mod::{shape,start_value,follow}` and `Inputs::read` let W9 preserve this behavior in its own filter state. `Target::Module` retains the modelled source of an addressed module assignment; W9 owns its destination slot/knob map. These targets are not counted in the eight amplitude/pitch external routes. Unsupported source/module geometry still requires explicit fallback or refusal, never route deletion.

`ControlState::modulation_value(plan, assignment_index)` and `pitch_envelope_level(plan, source_index)` expose read-only endpoints with original identities. `amplitude_level` exposes the existing primary audio state; `amplitude_control_point` reads the last published native point before audio interpolation (ordinary amplitude uses its existing level). The amplitude render buffer remains the authoritative per-frame native interpolation output. Reading any projection never advances it.

V1 pitch and filter envelope consumers deliberately have separate clocks: pitch skips to a block endpoint, filter renders samples and uses 32-frame coefficient ticks. Their f32 rounding can differ. Sharing the copied implementation preserves v1; replacing both consumers with a single endpoint or re-evaluating generic v2 modulation does not. W9 initializes/advances only the admitted filter consumers and stores their lagged values, then reuses these values for hold and process.

The saved shared pitch/volume LFO carries different destination metadata (`depth`/`targets`) in each descriptor. Only clock geometry must agree. Volume admission still requires zero source fade and nonnegative intensity/lag, as pinned `src/modulation.rs:314` does.

## Validation receipt

Normal FIFO checks after W8 direct drain: generic 381 baseline assertion failed numerically at frame 64 (`0x36bae5c4` versus pinned `0x36368000`); nine adapter tests passed with exact f32/u64 comparisons; `cargo test --locked -p sampler-core --profile ci --no-run` passed. All cargo calls used kontakto-heavy. The transient baseline test is retained in the receipt, not shipped as a failing test. Its first compile attempt needed a crate-root EnvelopeStage import; that compiler failure is not counted as RED.

Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w6-v1-voice-controls-381/`. No full corpus/native-host/CPU validation was run for this support module. It is not declared in lib.rs yet; W9 owns that declaration and production callers. No engine behavior changes until those callers are integrated. Unsupported sources/routes must remain explicit fallback; native live timing/retargeting admission is not broadened.
