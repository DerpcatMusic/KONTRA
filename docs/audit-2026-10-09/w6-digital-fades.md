# Saved Digital Multi fades

Base: `6fa559d4`, branch `v2/w6-conflux-modfx-381`. W6 owns the saved DSP law and importer; W5 owns lowering and live bindings; W9 owns native whole-voice timing. The coordinator authorized exactly one existing lowerer match arm: `IR DigitalSine { level, fade_ms } → core DigitalSine { level, fade: KontaktLfoFade::from_saved(...) }`. No alternate lowerer, service, parameter identity, or native whole-voice admission is introduced.

## Authored Conflux state

The installed Conflux 1.1.0 instrument contains 182 Digital Multi sources. All are normalized, retriggered, subunit sine-only sources, with the alternate mode false and unsynchronized fade records. Of these, 124 save zero milliseconds and 58 save the same f32 bits `0x3f24ca3c` (about 0.644 ms). One zero-fade source has a synchronized frequency; the 58 nonzero fades have unsynchronized frequency.

Before this change, 124 sources survive translation and 58 produce an unsupported-waveform diagnostic. The new shape retains the saved milliseconds exactly, independently of the generic linear `fade_in`. Zero-fade imports keep their existing `SineScaled` representation. Positive saved fades admit only finite sine levels in [-1,1], finite phases in [0,1], retriggered unsynchronized frequency/fade, false alternate mode, and duration 0..5000 ms. Mixed waves, alternate mode, synchronized/free-running/shared fades, separate delay/linear-fade state, and invalid durations remain rejected. Existing zero-fade behavior is unchanged.

## Original law

Pinned original PE SHA256: `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.

The original BNoteValueTime getter at `0x14072b380`/`0x140a74bf0` returns unsynchronized saved milliseconds unchanged. Production preparation at `0x140977d85` truncates the f32 product `(milliseconds * (audio_rate / 32)) * 0.001` to obtain the fade tick count N, and prepares factor `(1 + 1 / 0.3f32)^(1/N)` rounded to f32. Reset at `0x1405f45a0` restores the saved phase, N and level 0.3f32. Kernel `0x140b06c90` emits `(level - 0.3f32) * signal`, then multiplies level by factor and clamps it to [0,1] in f32. After N points the signal is unscaled. This has a 0.7 ceiling during the fade and a terminal step to full scale; it is not a linear ramp.

The Conflux saved value gives zero ticks at 44.1/48 kHz and one tick at 96 kHz. Preparation of the original preview graph at `0x140605430` uses a different duration path; it is not the production source law used here.

`tools/dsp/verify_digital_fade.py` executes the original getter, production arithmetic slice, reset, and recurrence. Its 27 configurations cover 44.1/48/96 kHz, zero/short/long durations, and the exact Conflux saved bits, checking 88,903 output values plus reset and chunk-boundary states. The only helper substitution is CRT `pow` with system `libm.pow`; native CRT rounding, full constructor/host scheduling, and live setters are not certified. The committed numeric fixture is synthetic; no library payload or PCM is written.

## Shared playback and boundaries

The existing v1 fade implementation, originally from pinned `0cb7a8a0:src/engine/lfo.rs` and already ported in `v1_voice_controls::lfo::Fade`, is extracted into the shared `digital_fade` module. The parked v1 adapter path-includes this same small source module and retains its original private constructor, bounds, and recurrence. Production shared playback uses the checked constructor. The parked v1 whole-voice file remains test-only. The shared evaluator prepares an immutable gain table once per source, reads it by voice age, and applies f32 waveform multiplication. Initial sample-start reads use the same first fade point. Trigger, render, release and voice reuse allocate/free no memory in the targeted guards.

The shared evaluator still samples control points every 64 audio frames and interpolates through its existing route timing. It reads the native 32-frame fade law at those points. This proves saved conversion and gain law in shared playback, not native host scheduling or whole-voice PCM parity. W9's native admission gate is unchanged; W5's initialized/live overlay is unchanged.

## RED → GREEN

Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w6-digital-fades/`.

The installed census RED exits 101 with 58 rejected sources. The rendered RED exits 101 at its first sample (actual 0.24984375 versus expected 0.2499241), demonstrating the linear-ramp mismatch. GREEN passes 180 exact original numeric checkpoint comparisons, three-rate rendered checks for zero/authored/10 ms fades with five block partitions and two reuse cycles, both trust-boundary rejection tests, the sample-start check, existing voice-modulation and copied v1 controls, installed census, and touched-area no-run (five crates, 121 test binaries). There are 36 passing targeted tests; one existing timing test remains ignored. Results are recorded in `SUMMARY.json` beside the logs.

Corpus-wide scanner acceptance, timed CPU/RSS and native host scheduling remain W0/W9 batch gates. No quiet-window or timing claim is made by this slice.

NEXT: Digital Multi fades READY; W0 batch gate and W5 lowerer review.
