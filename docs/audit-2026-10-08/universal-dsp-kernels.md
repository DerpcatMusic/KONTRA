# Shared DSP kernel inventory and first consolidation slice

Source baseline: `608a20a1687a501e161d80af72280639c122fbc3` (runtime equal to
the frozen `5fc362f3` CPU candidate). Scope: effect/filter sample math, not
format parsing, parameter conversion, envelopes, resampling or UI responses.

Spec-only references, read without using decompiler exports:

- `t3code-80fe786b/docs/DSP_EFFECT_CATALOGUE.md`, specialized-processing table
  and verified-processing sections (239 registration/reference entries).
- `DSP_FORMAT_SPECIFICATION.md`, OnePole, WaveShaper, fractional decimation,
  BitCrusher, and gain/matrix sections.
- `DSP_SYSTEM_INVENTORY.md`, Gainer, Daft, compressor linking and filter/effect
  inventories.

Reference SHA-256: catalogue `46627d0ccb5b64588f56c59fa2a6c69b5476d6c9df5cf3677eff0c2fcfb4f2e2`,
format spec `24e35563c1847da1600b59185e191f644273b4611b4b06524746f441bb27f267`,
inventory `1754cf6d8f41cd3d39ae75185138eccf2db1c9c7f0e9e48c5f94b020a3f03310`.

The catalogue explicitly distinguishes interface identities from established
algorithms. Matching names alone never establish shared math or native parity.

## Format-to-core inventory

Neither `crates/sampler-kontakt/src/dsp` nor `crates/sampler-uvi/src/dsp` exists
at this baseline. Kontakt's `effects.rs` (plus `effects/formant.rs`) and UVI's
`inserts.rs` translate records into `sampler_ir::Processor`; `sampler-core`'s
`lower.rs` selects the kernels. There are **zero independent sample-processing
kernel copies in those two format crates**. The following is the complete
intersection of their effect/filter processing, including primitive gain math:

| Shared math already in core | Kontakt adapter | UVI adapter | Qualification |
| --- | --- | --- | --- |
| Linear amplitude multiplication | Slot output/dry levels; native Gainer's amplitude step | Gain, DigitalEq OverallGain, WaveShaper input/output gains, rack gains | Gainer's float32 smoother is distinct; only multiplication is common |
| Stereo matrix | Inverter, swap, combined static slot levels | GainMatrix stereo submatrix | Same two-row arithmetic; channel topology/public laws remain adapter-specific |
| Compressor | Params::Compressor | CompExp | Both use the existing shared approximation; native full effects are not established as equal |
| Convolution | Params::Convolution, summed buses | Convolver/SampledReverb, summed buses | One shared convolution implementation; resource/shaping/import semantics differ |
| TPT SVF lowpass, two poles | SV LP2; each section of SV LP4 | DigitalEq Type 0 | Same core recurrence and preparation for equal physical Hz/Q; native equivalence unproven |
| TPT SVF highpass, two poles | SV HP2; each section of SV HP4 | DigitalEq Type 1 | Same qualification as lowpass |
| Peaking RBJ biquad | Legacy EQ bands and Formant I's three-band proxy | DigitalEq peak bands | Same coefficients/recurrence for equal Hz/Q/dB; band laws, numbering and proxy fidelity remain unverified |

Other implemented adapter paths are already single implementations in core:
Kontakt Gainer smoothing, Stereo Modeller (including pseudo delay), Daft,
Ladder LP4, Lo-Fi, algorithmic reverb and send taps; UVI one-pole low/highpass,
DigitalEq bandpass/notch/shelves, ThreeBandShelves' shelf approximation,
rectification, TrackDelay and parallel rack branches. None is duplicated in
the other format crate. `effects/formant.rs` constructs IR peak sections;
it does not implement a second audio filter. Core's decimator is available
but neither of these effect adapters emits it at this baseline.

## Exact duplicate arithmetic inside core

Complete list within the scoped core DSP dispatchers. Scalar wrappers live in
`src/dsp.rs` or the named module; batch wrappers live in `src/dsp/lanes.rs`.
Different buffer layouts and scheduling are intentionally retained.

| Kernel/sub-kernel | Scalar | Batched | Same arithmetic |
| --- | --- | --- | --- |
| Constant gain | PreparedProcessor::Gain | Same arm | `x *= gain` |
| Control gain | PreparedProcessor::ControlGain | Same arm | `x *= ramp.value(frame)` |
| Stereo matrix | PreparedProcessor::StereoMatrix | Same arm | `m00*l+m01*r`, `m10*l+m11*r` |
| Gainer | PreparedProcessor::Gainer | Same arm | Float32 target/current recurrence and common stereo multiplier |
| Stereo Modeller without pseudo | stereo::Stereo::process | StereoModeller arm | Balance, width/pan smoothing and every-fourth-sample pan cadence; matrix() is already shared |
| Biquad | PreparedProcessor::Biquad | biquad | Transposed direct form II, same float64 operation order |
| One-pole | svf::one_pole | one_pole | `next=y+(x-y)*b`, LP/HP selection |
| TPT SVF | svf::run | recurrence | Band/low/state recurrence and response mix, same float64 order |
| Wet/dry/bypass mix | PreparedProcessor::Mix | Same arm | `direct*x+through*wet` sub-kernel; a shorter batch voice can advance inner state differently (existing documented limit) |

Nine scalar/batch duplicate sites before this slice, eight after. Rectification
already calls `Rectifier::apply` from both paths; delay, compressor, decimator,
Lo-Fi, Daft, Ladder, reverb and convolution already use one processor each.
Envelope amplitude application is outside this effect/filter inventory.

Two additional repeated sub-kernel families occur inside the individual effects:

| Same math | Occurrences | Boundary to preserve |
| --- | --- | --- |
| Float32 one-pole `s += (x-s)*b` | Gainer gain smoothing, Stereo Modeller width smoothing, reverb output low shelf | These share a finite-input recurrence, not targets, coefficient laws or control cadence. The scalar/batch copies of Gainer/Stereo are already counted above; reverb's low shelf is another use. Core's SVF one-pole uses float64 and separate block-end flushing. |
| Float32 biased one-pole `s += (x-s)*b + 1e-20` | Lo-Fi noise colour, reverb input filter, reverb tank damping | Coefficient/input generation differs. Lo-Fi reverses the two multiplication operands; exceptional NaN payload selection is not established as identical. Reverb's input/damping already use the same denormal bias. |

These primitives are consolidation candidates, with their float32 and biased
variants kept explicit. Whole Lo-Fi/reverb/Gainer/Stereo effects do not become
the same algorithm. Parameter-table interpolation (Daft/Ladder) and reverb's
fractional delay reads are shared algebraic interpolation patterns, not a
second complete effect/filter kernel; their tables, rounding and histories
must remain distinct.

Do not combine the following on a name or algebraic resemblance:

- Workstation scalar/stereo OnePole: the spec establishes different float32
  operation orders and separate histories. Core's float64 approximation is
  not that native stereo kernel.
- Workstation BitCrusher vs Kontakt Lo-Fi: quantizer boundaries and per-call
  sampling phase differ. There is no BitCrusher adapter/kernel here to merge.
- Daft vs TPT SVF: nonlinear state update, oversampling and control cadence
  differ; Daft's equations are not duplicates of the linear SVF.
- Native Workstation biquad sub-kernels vs core RBJ: direct-form histories,
  float32 rounding and coefficient laws differ. This slice only consolidates
  the existing float64 core recurrence.
- Dispersor, Robotizer, TalkBox, rotary, grains and other catalogue processors
  do not become implemented merely by sharing generic filters/gain/delay.

## Slice and gate

`Biquad::sample` owns the existing float64 recurrence. Scalar stereo and
batched lanes call it; coefficient design/validation, masks, flush points,
controls, trace, public exports and format adapters retain their contracts.
The helper uses scalar values/fixed arrays and is always inlined; preparation
and callback allocation boundaries are unchanged.

Gate items are synthetic, using the original `608a20a1` equations as an
independent frozen oracle. The test is added and run before extraction:
11 responses (LP/HP/BP/notch/allpass, peak/low shelf/high shelf +/-12 dB),
three rates (44.1/48/96 kHz), three frequencies (40/1000 Hz/0.49*rate), three
Q values (0.2/0.707/4), six block lengths (0/1/3/4/17/64), masked/uniform
lanes and three successive input blocks (impulse, sine, signed zero/subnormal).
Output and retained histories use bit comparisons, including untouched
buffer guards and histories of ended voices. This is 10,692 lane block cases
and 5,346 scalar block cases.

Runtime gate items use the existing `dsp` and `svf` integration tests, including
`batched_voices_match_voices_rendered_alone` (11 staggered voices, mixed chain,
control ramps, release and batch boundaries) with allocator instrumentation.
These gates establish core refactor parity; native-host/corpus PCM and timing
remain separate acceptance axes. No library load or CPU measurement belongs
to this source slice, and the frozen CPU binary remains untouched.

Validation: **READY** for integration as a core refactor. All Cargo commands
ran serially through `/home/derpcat/.cache/kontakto-heavy` in the owned
`w6-universal-dsp` worktree, with no library or timing runs:

- Before extraction: `cargo test --locked -p sampler-core --lib
  biquad_scalar_and_lanes_match_frozen_pcm_and_state_bits`: 1 PASS.
- After extraction: `cargo test --locked -p sampler-core --lib biquad`:
  2 PASS, including the independent impulse/response test.
- `cargo test --locked --release -p sampler-core --lib --test dsp --test svf
  --no-run`: PASS.
- Same release command without `--no-run`: **79 unit + 7 DSP + 5 SVF = 91
  PASS, 0 FAIL**. The frozen scalar/lane bit oracle passes under optimization;
  the runtime allocation/deallocation assertions (including 11 staggered
  voices) pass with zero heap calls.
- `git diff --check`: PASS.

No new dependency, public API, importer law, control binding, CPU artifact or
release/install change. Corpus/native parity and callback timing are not
measured by this slice. Next: W8 direct handoff, frozen CPU window, then W13.

## Remaining consolidation slice (validation pending)

Implementation `494bb088`, test-only checkpoints `a6b17c70` / `a4c9e2cd`.
The remaining eight scalar/lane sample-math sites now call shared primitives:
constant/control gain, stereo matrix, Gainer multiplier/smoothing, Stereo
balance/advance, wet/dry/bypass mixing, float64 OnePole and TPT SVF.
Layouts, initialisation markers, coefficient preparation, lane end masks,
pseudo delay histories, control cadence and flush points stay with their callers.
`ControlRamp::settled` and coefficient hoisting are outside this slice (W9).

Float32 `one_pole32` is shared by Gainer, Stereo width and reverb's output shelf.
The biased primitive accepts a precomputed increment, so Lo-Fi keeps
coefficient-first multiplication while reverb keeps difference-first multiplication.
Its addition is explicitly `state + (increment + 1e-20)`; it is not
`(state + increment) + 1e-20`. The float64 OnePole remains distinct.

Independent frozen-608a oracles cover all scoped dispatchers, output guards,
retained states and ended voices. Dispatch cases include seven lengths
(0/1/3/4/5/17/64), uniform/masked ends, batches of 1/3/8 voices, four successive
input blocks (impulse, sine, signed zero/subnormal), control trajectories and
fresh/retained smoother state. SVF adds seven modes, three rates, three Q values,
and settled/moving cutoff. Additional oracles cover Lo-Fi's noise RNG/state,
reverb's three one-poles and feedback lines (8192 ticks at each of three rates),
and Stereo's pseudo ring/odd-block cadence (both pseudo settings, 32 calls).

Baseline gate on test-only `a4c9e2cd`: optimized `cargo test --locked --release
-p sampler-core --lib shared_` PASS (9 tests; five new frozen oracles plus four
existing shared tests), before validating the extracted math. Logs:
`~/.cache/kontakto-w6/universal-dsp/remaining-baseline.log`.
Post-extraction bit checks, allocator gates and area no-run are pending the
coordinator's serial validation slot at the start of W6's quiet window.
Do not treat this second slice as READY until that receipt is appended.

CPU acceptance continues to use the unchanged frozen exact-5fc artifact.
Updated quiet order: W9 → W8 (six cells) → W5 (≤20 min) → W6 (42 CPU cells) → W13.
W6 starts only after W5's direct release, validates the source slice before
any timing, and publishes READY independently of the frozen CPU verdict.
