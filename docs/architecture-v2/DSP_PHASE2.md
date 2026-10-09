# W6 DSP phase 2 checkpoint

Implementation branch: `v2/fix-dsp`. Primary audit: `audit/dsp-20261008@3ea7a97b`, `docs/audit-2026-10-08/dsp.md`. This is a checkpoint, not complete DSP acceptance.

## Changes and validation

- `60e82623`: summed UVI Program and Layer insert buses; original XML node identities retained. Nonzero TrackDelay now has a delay processor. ThreeBandShelves has a relative-shelf approximation with an explicit `UnknownLaw` diagnostic; public catalog gains must not bind independently to those relative stages.
- `f8c709eb`: removes the temporary single-filter restriction on OnePole frequency connections after W5's `f32e710b` per-stage lowering. A failing-first two-filter fixture verifies distinct authored stage addresses. UVI fixture suite: 12 passed.
- `157bc206`: `VoiceChain::with_taps` captures signal after a named number of pre/post-amplitude compiled stages into preallocated bus feeds. Voices sum into the return bus before its processors. Gains and bypasses use actual DSP controls. Tests cover two voices, delayed returns, real engine writes and block lengths 1/7/64/129 under the heap guard. Tapped plans currently use the audio-thread path; W9 has been notified.
- `22d5b2ed`: Kontakt retains the physical amplifier split and admits group compression per voice. Stateful native Gainer and Stereo Modeller replace static import matrices. Stereo includes native width/pan recurrences and optional right-channel pseudo delay.
- `c44a44b0`: static voice gain and existing script/modulation gain ramps now apply at the amplifier split. Pre-amplitude taps see the source before those gains; post-amplitude taps see them. Scalar, batched and parallel renderers share the placement. The existing tone path remains after the chain. Full core suite passed; 14 bus tests passed, including failing-first static/dynamic position probes with numeric tolerances and the heap guard.
- `b33ddb2a`: restores `ModTarget::signed_intensity` from frozen `v2/gpt-format-fxmod@b7f6af0b`. Flag `0x02` means negative depth independently of Invert. Positive envelope/velocity shortcuts now check signed depth. Kontakt unit suite: 41 passed, 3 ignored.

`f4f9b096` wires static physical-slot group taps into W5's shared IR lowering (`5c9e18e6`) and preserves ordered instrument send taps. `ee9ddf49` batches native Gainer and non-pseudo Stereo through existing lane state; the full core suite and 11-voice staggered scalar/batch bit-equality probe pass. Pseudo Stereo retains its existing scalar ring-buffer path. Resumed validation: Kontakt unit tests 54 passed / 4 ignored, shared lower integration tests 20 passed, and the installed-library saved-compressor probe passed separately.

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

The resumed raw-slot probe reads the original `BParFX` private state before script harvesting: physical insert slot 1, **bypass=false**, linear output gain **2.8163939**, or **+8.99386775 dB**. This agrees with pinned v1 `0cb7a8a0:src/engine/params.rs` (`effect_gain = 16*x³`, normalized x=0.560434) within 1e-6 linear gain. W5's runtime `CubicGain { unity: 396851 }` yields 2.8163783; its rounded unity differs from the saved/v1 law by less than **0.00005 dB**. The importer now reuses this shared conversion, including its normalized-input clamp, so init and live writes cannot diverge on out-of-range script values. No nine-decibel conversion error, default bypass inversion or duplicate output stage is established. The RE inventory certifies the Classic linked detector, but explicitly leaves complete compressor gain/envelope laws uncertified; the independent host on/bypass capture in `KONTAKT_REFERENCE.md` section 26 reports +8.4–9.0 dB, consistent with this saved output trim. Native grid switch state is still required to attribute the remaining whole-instrument mismatch.

The rebuilt three-state probe (`SHA256 4cef88cc78695ae6933a3cccae61a006ed7fbd9dceb08b1e4997af05e8eabe49`) renders the same C4 window with saved output, forced bypass, and enabled compressor with unity output. RMS is respectively **0.0932991158078 / 0.0331272247111 / 0.0331272247104**. Saved output versus unity is **8.99384949 dB**; unity versus bypass is **−2.1e-10 dB**. At this tested input level the entire difference is the output trim, with no measured compressor reduction. This does not generalize to higher-level signals or certify the native grid state.

Renderer digest: `9fc219822bddb2b38463992cced542314ca62beb9a8d71f18827c27657774c8b`. Matched audio and numeric JSON stay in `~/.cache/kontakto-w6/`; only aggregate numbers are reported here. `tools/dsp/compare_reference.py` reuses the audit's comparison from frozen `3ea7a97b`, with explicit output path and candidate provenance.

## Scanner provenance and limits

### Resumed script state and native compressor comparison

`103a46d1` fixes the shared rack detector: W5's `Op::EngineParameter`
writes now retain real FX control lanes. Register-computed addresses require
conservative detection. The failing importer fixture is green. After merging
W5 `f040122b`, Analog's physical script slot 2 completes persistence;
the global compressor's snapshot toggle is **1 (ON)** and output is **560434**.
Layer compressor toggles are both zero. Physical instrument insert slot 1
(`group=-1, generic=1`) remains bypass 0. No compensating trim is justified.

The read-only native `an_c4_b`, `an_e4_b`, `an_g4_b` captures were compared
with both corresponding `an_byp_*` repetitions. Onset-aligned stereo power
over 1–3 seconds gives native ON/BYPASS **+8.24757 to +8.78953 dB**,
mean **+8.43852 dB**. The rounded session RMS log gives +8.44367 dB.
This supports the saved output law; the 0.55 dB difference from the shorter
v2 probe is confounded by window, compressor reduction and sample choice.
Native capture calibration passed at 0.1 dB. Reference WAVs were read in place.

`4a49033f` ports v1 `src/fx/processor.rs` bypassed-send admission into a
return gate on the **existing** physical slot bypass control. V2's dynamic
Mix formerly returned unprocessed send input when bypassed, adding a dry copy.
The failing unity probe measured 0.25 → 0.5 (+6.0206 dB); it now preserves
0.25, and live engine enable/bypass writes correctly restore/mute the return.
Kontakt lib: 57 passed, 4 ignored; required root no-run passed.

Bounded C4 grid, keys 60, velocities 20/60/100/127, four repetitions each:
on W5 persistence head with native UI enabled, the send fix changes mean
native level error **+14.21787 → +8.37009 dB** (−5.84778 dB), zero native
audible windows silent in v2. Per-velocity changes are −5.85488, −5.84365,
−5.84364, −5.84894 dB. This attributes the restored-routing level increase
to bypassed send returns. The remaining excess remains open; this is a family/distribution comparison, not an exact RR match.

`4376a74e` directly copies pinned v1 `src/engine/filter/ladder.rs`, including
native float32 arithmetic and 32/4-frame control scheduling. Its adapter
reserves state only for Ladder stages in the worker-allocated arena; generic
ProcessorState is unchanged. Native physical probes and exact fragmented
adapter/voice-reuse parity pass (six selected core tests). `f699534d` adds
typed normalized Ladder LP4 IR, record version, and shared gain/cutoff/resonance
lowering; the public engine-parameter audio witness passes (core lower 21/21).
`130f0fe9` imports subtype33/version90–92 using the pinned v1 versioned reader, including static physical ownership. Per-family census and remaining kernels are not yet complete.

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

### Native GRID state and frozen v1 bisection

The GRID's four C4 velocity-100 repeats, onset-aligned 0.10–0.45 s stereo
RMS, are −24.295 / −23.199 / −22.795 / −22.457 dBFS. The active native
session is −25.177 dBFS; bypass repetitions are −33.838 / −34.176 dBFS.
This supports active, not bypassed; the GRID must not be relabelled off.
Session MIDI CC1 is 0 versus GRID 64; other initial CC values match. The
window comparison controls duration (GRID notes are shorter), but CC and
round robin differences prevent certifying identical state.

Frozen v1 `bin/kontakto-v1` passes all frozen SHA256SUMS and reports clean
`0cb7a8a0`. Its CLI caps renders at 60 s, so the full GRID misses the last
C4 notes; the complete bounded 16-note C4 shard is used for both versions.
Both use GRID CC1=64, CC7=127, CC10=64, CC11=127 and CC64=0. Pinned v1
`src/main.rs` writes engine output ×0.25, whereas v2 writes unity: undoing
that diagnostic-only −12.0412 dB scale gives v1 mean native error **+7.50092
dB**, versus v2 **+8.37009 dB**. V1 per velocity is +5.82473 / +8.21732 /
+8.20747 / +7.75418 dB. Raw v1 WAV error is −4.54028 dB. The excess is
largely shared; the additional v2 mean is +0.86917 dB. Stage attribution
remains open; no compressor gain-law correction is justified by this result.

Native UI metadata confirms completed persistence re-applies compressor
bypass 0 and output 560434 on physical group −1, insert slot 1, generic 1.
The probe reports only target names, addresses and integer values, never
library script text. Its setter-site test excludes comments/strings/wrappers.

### Trace-based Analog upstream attribution

Shared traces `11ab86bd`, plugin mixer extension `32796758`, and live-envelope
correction `aac993e6` expose the contribution and parameter evidence. Bounded
C4 v100, GRID CC1=64/CC7=127/CC11=127, sounds two voices: zone28307/group88
on layer5 and zone46025/group139 on layer6. Their zone outputs are −26.304 /
−31.835 dBFS; post-layer outputs are −26.448 / −38.235. The coherent layer
sum is −26.360 dBFS, only +0.088 dB over the dominant layer. Static crossfade
and velocity gains are unity; CC1 0/64/127 does not change layer volumes.
The amp multiplier0.741651714 (−2.596dB) is entirely the script's per-note gain.
Group88 pre-gain is +2.007dB. Live amplitude-envelope attack times are5184 /
50324frames, matching native engine values449120 /702000 within one frame;
attack curve values413050 /411450 are applied. Saved instrument gain1.005088
(+0.044dB) cannot account for the residual. No equal/full-layer crossfade or
main-envelope init-loss explanation is supported by these observations.

Layer gains match the intended init VOLUME requests593624 /525135 numerically,
but the shared getter at group−1/slot−1/generic1002 /1003 returns unsupported.
Do not treat a static match as proof that every corresponding write succeeded.
The compressor trim560434 still gives +8.994dB and remains consistent with the
native on/off delta; it does not explain the upstream difference.

Both active groups report unmodeled source mode3. Their filter type100 at
normalized cutoff0.214 /0.0, zero resonance, and the associated cutoff targets
are omitted. Script init explicitly sets both slot0 bypasses to0 (enabled). The source-mode identity is retained numerically; no unverified
source-engine mapping or compensation is inferred. A sixteen-C4-window Hann
spectrum comparison shows v100 v2 excess~8.29dB below200Hz and~13.40dB above
5kHz; corrected frozen v1 excess is~6.88 /13.30dB. These are band-energy
comparisons across uncontrolled sample choices, not native internal taps. The
remaining level/tone cause is unresolved; source/filter/modulation behavior
needs attribution before changing a gain law. Remaining ports stay paused.

Complete, zero-loss trace and chart:
`~/.cache/kontakto-w6/analog-trace-envelope/signal-trace.json` and `.svg`.
`analog-c4-window.svg` / `.png` / `.json` summarize frames52800–69600, with
native final-output and explicitly inferred pre-compressor markers.
`tools/dsp/chart_signal_trace.py` rejects incomplete/lossy input and can render
any numeric node window, with measured/inferred reference markers.
Numeric intent/contribution/envelope receipts remain beside the run artifacts;
reference WAVs were only read in place, and no sample payload or script source
is exported in the trace.
