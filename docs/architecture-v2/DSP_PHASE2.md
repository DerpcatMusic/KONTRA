# W6 DSP phase 2 checkpoint

Implementation branch: `v2/fix-dsp`. Primary audit: `audit/dsp-20261008@3ea7a97b`, `docs/audit-2026-10-08/dsp.md`. This is a checkpoint, not complete DSP acceptance.

## Changes and validation

- `60e82623`: summed UVI Program and Layer insert buses; original XML node identities retained. Nonzero TrackDelay now has a delay processor. ThreeBandShelves has a relative-shelf approximation with an explicit `UnknownLaw` diagnostic; public catalog gains must not bind independently to those relative stages.
- `f8c709eb`: removes the temporary single-filter restriction on OnePole frequency connections after W5's `f32e710b` per-stage lowering. A failing-first two-filter fixture verifies distinct authored stage addresses. UVI fixture suite: 12 passed.
- `157bc206`: `VoiceChain::with_taps` captures signal after a named number of pre/post-amplitude compiled stages into preallocated bus feeds. Voices sum into the return bus before its processors. Gains and bypasses use actual DSP controls. Tests cover two voices, delayed returns, real engine writes and block lengths 1/7/64/129 under the heap guard. Tapped plans currently use the audio-thread path; W9 has been notified.
- `22d5b2ed`: Kontakt retains the physical amplifier split and admits group compression per voice. Stateful native Gainer and Stereo Modeller replace static import matrices. Stereo includes native width/pan recurrences and optional right-channel pseudo delay.
- `c44a44b0`: static voice gain and existing script/modulation gain ramps now apply at the amplifier split. Pre-amplitude taps see the source before those gains; post-amplitude taps see them. Scalar, batched and parallel renderers share the placement. The existing tone path remains after the chain. Full core suite passed; 14 bus tests passed, including failing-first static/dynamic position probes with numeric tolerances and the heap guard.
- `b33ddb2a`: restores `ModTarget::signed_intensity` from frozen `v2/gpt-format-fxmod@b7f6af0b`. Flag `0x02` means negative depth independently of Invert. Positive envelope/velocity shortcuts now check signed depth. Kontakt unit suite: 41 passed, 3 ignored.

The next checkpoint adds static physical-slot group tap import and ordered instrument send taps; Kontakt unit tests pass (52 passed, 3 ignored). Native Gainer and non-pseudo Stereo now use lane kernels with existing state cells; the full core suite and 11-voice staggered scalar/batch bit-equality probe pass. Pseudo Stereo retains its existing scalar ring-buffer path.

Every pushed checkpoint passed the required root `cargo test --no-run` gate through `kontakto-heavy`. No lower fork, new parameter identity hash, or unsupported parameter mirror was added.

## Family reach: audit exposure versus implemented paths

Counts below are the saved-enabled audit population, not a claim that every saved module is audible. Admission counts must be established by the candidate census before final acceptance; distinct families and NKM repeats must not be summed as unique libraries.

| Family | Audit exposure | Before | Current path / remaining limit |
|---|---:|---|---|
| UVI ThreeBandShelves | 660 programs / 2,977 modules | missing | static shared-shelf approximation; native crossover and dedicated public controls open |
| UVI OnePole Freq | 598 programs / 185,211 connections | dropped | keygroup-owned connections address each stage; Program/Layer frequency routes and native coefficient conversion open |
| UVI TrackDelay | 40 programs / 160 modules | nonzero delay missing | static seconds delay with shared lower tail bound; host sync open |
| Kontakt Gainer | 194 containers / 194 slots | static gain import | native stateful recurrence; public native gain binding law open |
| Kontakt Stereo Modeller | 363 containers / 21,619 slots | static matrix, pseudo missing | native stateful width/pan and pseudo delay; shared IR/lower controls and delayed tail available; importer bindings open; non-pseudo native lane kernel added |
| Kontakt Send Levels | 810 containers / 819 slots | ordered group taps absent; instrument feeds post-chain | authored group taps resolved to physical send returns; ordered instrument taps use serial summed bus segments; runtime tap parameter binding open |
| Kontakt legacy EQ | 223 containers / 11,480 slots | RBJ, `UnknownLaw` | still requires native coefficient/wrapper verification |
| Missing Kontakt filters | 77 containers / 5,681 slots | missing | additional kernels and SV resonance compensation pending |
| Missing Kontakt non-filter FX | 60 containers / 723 slots | missing | additional kernels pending |
| Kontakt LFO type 6 / random bipolar | 51 containers each | missing | typed source/shape extension coordinated with W5; pending |

## Matched Analog level evidence

The same native capture and MIDI grid as the audit were used: 48 kHz, first-onset alignment, stereo-power windows 0.10–0.45 s after each note. Sample selection remains a confound; RR is not scored as an exact-sample identity. No gain trim was added.

| Candidate | Windows | Mean signed error | Maximum absolute error | Native-audible / candidate-silent |
|---|---:|---:|---:|---:|
| audited `7e82b152` | 48 | +14.4806 dB | 15.4691 dB | 0 |
| W6 `c44a44b0` | 48 | +8.5884 dB | 9.6270 dB | 0 |

The mean improves 5.8922 dB. The residual is not yet attributed. The cached audit render's approximately 9 dB difference still requires effective compressor bypass/output and initial-controller state isolation. W7's framed reader now confirms 287 host-only BAO records and zero MIDI-CC bindings on Analog, so saved CC automation does not explain this delta. Native grid compressor-switch state remains unproven.

The metadata probe exposes the existing static compressor and its output trim through the production Mix lowering at the independently verified physical address `group=-1, slot=1, generic=1`. It retains the saved gain and default state. Initial readback is `bypass=0`, `output_gain=560434`. At C4/velocity64/CC1=100/CC11=127, stereo RMS over 0.10–0.45 s is `0.0932991158` active and `0.0331272247` with forced bypass, a difference of **8.99384949 dB**. This isolates a stage with the required approximately 9 dB state effect; it does not establish the native grid's switch state or justify a production gain trim. Probe SHA256: `3eac91258ab263c6d5fc7f7557e90612193b6fc6f6d887f156781f681af8d449`.

Renderer digest: `9fc219822bddb2b38463992cced542314ca62beb9a8d71f18827c27657774c8b`. Matched audio and numeric JSON stay in `~/.cache/kontakto-w6/`; only aggregate numbers are reported here. `tools/dsp/compare_reference.py` reuses the audit's comparison from frozen `3ea7a97b`, with explicit output path and candidate provenance.

## Scanner provenance and limits

The candidate merges the shared adapter `9dcf05e5`. Frozen before binaries are copied into `~/.cache/kontakto-w6/bin/`, and are used only as before baselines.

| Artifact | SHA256 |
|---|---|
| frozen v2 before | `18fe63fe07e62ea3c012ce29fc2d01b08f518ea808459446478cc46aca924b6d` |
| frozen v1 Kontakt before | `a4b3f8c76483ea06b36b9fef46911093d7e8df8021bd9e54e711018dbf700399` |
| frozen v1 UVI before | `d565661afb3ae1cab3100d1e83b7f12c0f5b02c51b7593451a6fe77929af8ea1` |
| W6 `c44a44b0` release adapter | `537c9a18e9e5feacd74f57bd38317c8d79144f991d07d389d562bb457c171926` |
| adjacent shared Python driver | `cd472de4950506591f3a709eea19116445a1d41cbb2a9b0fb58267f41c7c5f93` |

First 25-item shard: 25/25 paired rows load, zero load regressions. Two matched-note auditions remain audible. The other 23 rows contain `pick:null`, so they are not auditioned and are excluded from audio parity even though this driver labels them silent. The coordinator is updating the scanner to distinguish these cases and add keyswitch/fallback handling. The installed README now pins `2355155b` with corrected audition handling (v2 digest `f1105599c13b14410ed891b29a014b8af43596e238850bb8e4439596e96e013a`, v1 `ac5aed734bb7fca40d6d000ff1f3e89128436b8f38d9d674fd70466f90dcdf08`). Candidate rebuild and corpus-wide acceptance remain pending.

## RE source reconciliation

The public Program reader and occupied-FX slot diagnostics listed as dirty work in the audit were already landed through frozen `refs/wip/feat/decipher-readers-v2/20261008T055026Z` (`a3c29f65`) → `be4c5c21`, and are present on the W6 base. No missing Program/diagnostic patch was found to copy; the Program diff against that snapshot was formatting. Dirty trees were not used as patch sources. `ALTERNATING_LOOPS` belongs to W7; its tested selection/loop and saved automation checkpoint `6baa167a` is merged in W6. The signed-depth helper is the separately identified missing frozen-reader change copied in `b33ddb2a` with attribution.

## Coordination and remaining acceptance

W5 owns shared engine parameters and generic lowering. W10 owns the typed UVI catalog, Lua and bindings. W9 owns callback CPU/streaming. W6 must finish runtime send controls and remaining bus-scope send positions, new FX controls through the shared service, native kernel/clock gaps, full per-family admission counts, Analog state attribution and the full load/play comparison. Existing generic convolution and FDN tests establish core numerical behavior, not Kontakt-native IR preparation or reverb topology.
