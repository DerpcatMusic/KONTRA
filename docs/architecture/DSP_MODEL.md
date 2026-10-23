# Universal DSP model and migration plan

Design audit, 2026-10-23, baseline `42db76a3a` (includes doctrine PR #531).
Scope: existing Rust DSP ownership, family implementation and configuration; no new native
reverse engineering, performance measurements or implementation in this PR. Follow
[ENGINE.md](../ENGINE.md). Format registry/features belong to `wave68-modular-engine`;
Kontakt translators, `lower.rs` and oracle tooling have other owners. Coordinate edits there.

## Decision

Keep the existing family architecture. Consolidate its execution primitives and make product
differences explicit data. Do not replace it with a runtime graph language, a plugin ABI,
inheritance hierarchy, or a second `Settings` system beside `sampler_ir::fx::*::Config`.
Use small typed enums for genuine algorithmic sub-models; dispatch when preparing a processor
or once per block, with concrete inner loops. Products select settings, never own processors.

The code is further along than the product names suggest: EQ, dynamics, reverb and modulation
already have family entry points. A product-named designer is not necessarily duplicated DSP;
a native object port containing its own sample loop is a consolidation candidate, not proof
that another implementation is equivalent. Preserve evidence and remove replaced code in the
same PR. Favor a clean measured approximation when it meets the acceptance contract below.

## Audit: where the code actually lives

Paths below are relative to `crates/`. **C** = `sampler-core/src`, **D** = `C/dsp`,
**I** = `sampler-ir/src`, **K** = `sampler-kontakt/src`, **U** = `sampler-uvi/src`.
LOC counts are physical Rust lines, including tests/comments, not removable-code estimates.

### Format boundaries

| Exact files | Finding and disposition |
|---|---|
| `K/effects/{mod,bus,test_support,record_audit}.rs` | Rack parsing, routing, bus assembly and tests. Matrix composition is preparation, not a second audio engine. Keep with the format. `bus.rs` is already 617 lines on this baseline. The relevant split from recovered commit `1acae0b88` is present; do not cherry-pick its unrelated corpus/tool changes. |
| `K/effects/{eq,filter,dynamics,space,color,timemod,ringmod,stereotune,surround,mangling}.rs` | Production code reads records and builds configuration; tests synthesize/process signals. No production sample-processing loop found here. Keep readers, slot addresses, units, ID routing and unsupported diagnostics. |
| `K/effects/eq.rs` | Calls `sampler_core::KontaktLegacyEqSettings::{frequency_knob,bandwidth_knob}`: mapping depends on an execution type. Put these pure domain conversions with the IR law and delete the core API dependency, retaining exactly one conversion implementation. |
| `K/effects/surround.rs` | Constructs `sampler_core::surround_panner::Panner`, evaluates gains, folds a static matrix. A preparation dependency, not per-sample DSP. Prefer IR spatial settings compiled by the core; retain the existing stereo-only admission restrictions. |
| `K/effects/timemod.rs::choral`, `MODES` | Builds seven voice-bank settings, rotations and feedback from saved knobs. Valid mapping today; mode data and reusable bank design can become a pure preset/designer boundary. Moving trigonometry alone is not a CPU win. |
| `U/fx/{mod,eq,filter,dynamics,space,timemod,color}.rs`, `U/inserts.rs` | Shared-IR mapping and placement; convolution preparation calls `sampler_core::shape_impulse`. Keep decoding/mapping, converge preparation ownership if it avoids duplicate work. Unrecovered UVI dynamics remain diagnosed. |
| `K/head_cache.rs`, `U/scripted.rs`, `U/scripted/thread.rs`, `U/script/live_graph.rs` | Search hits for `process`, `render`, `tick`, `step` are cache access, runtime delegation and script scheduling, not effect recurrences. Format/runtime orchestration is the modularity lane's audit, not grounds to duplicate DSP. |
| `I/fx/mod.rs` and family `lanes()` methods | Reverse boundary leak: `uvi_lane` emits `ENGINE_PAR_*` names; IR lanes and law variants contain product identities. Keep compatibility names at format bindings; core controls should have neutral roles plus stable IDs. This is a staged API/cache migration, not a global rename. |

### Existing modules → universal core and settings

Every family file at the audited baseline is covered below; braces denote the exact named files,
not an instruction to rewrite a directory at once. Support code is listed separately.

| Existing implementation | Target core + declarative settings; deletion opportunity |
|---|---|
| `D/eq_family/{mod,generic,kernel,legacy,solid,digital,tests}.rs` — 2,713 LOC | Section cascade + band designer + control schedule. Legacy 1/2/3-band → peak sections; Solid G-EQ → spline/designer preset, section ordering and glide; RBJ/Cookbook → matched/bilinear designer; UVI DigitalEq → root/prototype designer and section list. Already shares TPT execution for most paths. Solid actually runs DF1 in `generic::run_df1`, contrary to the module header. Share the recurrence first; do not promise to delete 2,713 lines. |
| `D/filter/{cascade,svf,linear,chamberlin,phaser,table}.rs` | SOS/TPT/explicit SVF bank + coefficient laws, output mix, cadence, precision. `cascade` includes Kontakt legacy/pass/vowel/SV/Versatile/Pro-53 and UVI Biquad/Moog/Rez/Xpander/Digital/Vowel/Comb. Factor linear section execution and design duplication; Chamberlin and TPT remain distinct discretizations. Phaser allpass → common allpass primitive. |
| `D/filter/{ladder,ladder_kernel,ar,ar_kernel,daft,formant,kontaktfx,shaper}.rs` | Nonlinear feedback family: ladder stages/global feedback, adaptive-resonance loop, saturating 2x SVF, parallel formant bank; Simple LP/HP, Dual SKF and Monark select corresponding topologies, drives, taps and laws. Extract shared one-pole/SVF/knee/oversampling stages. Do not reduce nonlinear feedback to linear SOS. Filter family total: 10,616 LOC, including native/research vector modules. |
| `D/dynamics/{mod,pipeline,detector,computer,ballistics,output,laws,logarithm,tests}.rs` — 4,162 LOC | Already one monomorphized staged pipeline. Classic/Enhanced/Pro → detector/computer/ballistics/output recipes; Limiter/Hilbert → peak or Hilbert detector + window/lookahead; Feedback → feedback topology; Transient → dual/multiple envelopes; Bus → feedback detector + spline laws; UVI Maximizer → peak/true-peak + knee + slew/window + ceiling. Preserve arithmetic/logarithm evidence; delete only repeated recipe glue. |
| `D/db_limiter.rs` | dB peak dynamics sub-model with its measured gain/release law. Absorb into dynamics execution/state ownership; do not conflate it with the existing Kontakt limiter recipe. |
| `D/timemod/mod.rs`, `D/timemod/{uvi_legacy,dual,dual_x,twin}.rs` | Multi-head delay + read interpolation + feedback matrix + ordered loop stages. Legacy Delay/Chorus/Flanger/Phaser and UVI modulation → tap/LFO/allpass settings; DualDelay/X and Twin → two independently controlled paths, cross-feedback, filters, dispersion/diffusion and pre-delay. Share rings/readers first; preserve per-model write/read order. |
| `D/timemod/{tape,buzz,wowflutter,psyche,velvet,diffuse,voices,mt}.rs` | TapeEcho/Buzz → moving transport heads + loop EQ/drive; WowFlutter → transport modulation; Psyche → crossfaded grain heads; Velvet → nested allpass; Diffuse → two-line diffusion/rotation; Choral → seven filtered/modulated reads + circulant bank. RNG algorithm, seed and draw schedule are settings/state. Timemod total: 3,935 LOC. |
| `D/replika/{mod,design,delay,head,chain,deposit,write,read,start,mode1,stage,tests}.rs`, `D/replika_head.rs` — directory 5,063 LOC | Delay-head bank, fractional read/deposit, loop filters/saturation, ducking and output mix. Designer/control trajectory are a preset; missing mode paths stay missing. `mode1.rs` explicitly documents incomplete wiring; `stage.rs` documents six heap rings (~12.6 MB at 48 kHz). Consolidate primitives without claiming all Replika is admitted or all memory can disappear. |
| `D/ringmod.rs`, `D/ring_ramp.rs` | Oscillator × input + dry/wet; four-lane ramp with explicit update schedule. Already mostly neutral; retain measured carrier/phase and reuse the ramp where identical. |
| `D/stereo_tune.rs`, `D/rotator.rs` | Stereo Tune → crossfaded variable-delay heads, phase/drift/spread and shared ramp. Rotator → crossover + two accelerating rotors + modulated delay/amplitude + output filters. Keep topology sub-models; delete native byte-offset state after typed-state/vector parity. Rotator has unbound controls/unported branches; no new support claim. |
| `D/gater.rs` | Pattern-gain/window scheduler + optional filter/drive/noise/delay recipe. Current kernel lacks full clock/setter scheduling; `K/effects/mangling.rs` remains unsupported. Share components without admitting Beat Masher/Slicer/Reverse Grain/Transpose Stretch. |
| `D/surround_panner.rs`, `D/color/{stereo,widener,stereorizer}.rs` | Spatial matrix/delay + gain normalization and smoothing. Static surround → compiled matrix; dynamic surround → prepared position/gain law, only after supported-layout evidence. Stereo Modeller → mid/side/balance/pseudo-delay; Widener → signed one-channel delay/side shelf; Stereorizer → routing wrapper, not a new shaper. |
| `D/color/{mod,shaping,distortion,saturator,drive,diode,exciter,skreamer,tubescreamer,dirt,tape,freak,bite}.rs` | Ordered color recipe: linear sections → transfer stage/feedback circuit → linear sections, with explicit rate domain and wet/dry. Classic/Enhanced/Drums, Tube/Transistor, GarageFuzz/SmoothOverdrive/StraightFuzz select curves and conditioning. Diode/Tubescreamer/Dirt/Tape/Freak are stateful sub-models; they cannot all be replaced by a memoryless `tanh`. |
| `D/color/{lofi,redux,tone,tonestack,hilbert}.rs` | Clocked hold/quantizer/noise + rounding/dither/clock laws; tilt/tone-stack designers → section coefficients; Hilbert → quadrature allpass designer. Reuse phase/interpolation/filter primitives while retaining distinct noise and rounding behavior. |
| `D/color/amp/{mod,chain,kernel,acbox,cat,twang,dstortion,jump,fuzz,bigfuzz,crywah,cabinet}.rs` | Amplifier recipe + table/circuit sub-model + oversampling + tone stack + latency. AC Box/Cat/Twang/DStortion/Fuzz/Big Fuzz are candidates for shared table evaluation/rate wrappers; Jump's stage topology must be measured separately. Cry Wah → controlled filter network; Cabinet → coefficient-bank cascade (not necessarily convolution). `chain::Spec` already provides a simpler configurable chain: use only where response evidence admits it. |
| `D/color/kolor/{mod,control,notify,chain,shaper_0_1_3,shaper_2_5,shaper_4,shaper_6_9}.rs`, `I/fx/kolor.rs` | Composable curve/table/recurrent shaper + control/mix schedule. Cases include recurrent/GRU-like state; preserve as a generic recurrent sub-model only if admitted behavior needs it. Post-shaper gaps remain explicit; do not turn partial evidence into ten supported modes. Color total: 10,328 LOC. |
| `D/machine.rs`, `D/machine_tables.rs` | Source coloration recipe: S1200 → fixed high-order IIR + mode filters/gain + 26 kHz hold/quantizer; MP60 → fixed-rate wrapper + IIR/12-bit/IIR. Tables become preset data. A high-order direct-form filter may remain a sub-model if SOS factorization changes verified response/state behavior. |
| `C/resample.rs`, `C/resample/{cubic_block,coefficient_reuse}.rs`, `C/source.rs` | Existing shared playback cursor, interpolation/sinc bank, loops and envelope/mix. Keep one playback implementation across formats; no new product resampler. Share coefficient/design helpers only where precision, latency and boundary rules agree. |
| `C/source/{time_machine,tone_machine,beat_slices}.rs` | Common grain reader/window/mixer with scheduler sub-models: two-reader, WSOLA, pitch-mark and slice sequencer. Time Machine II/Pro are labeled stand-ins; Tone Machine has a separate recovered kernel. Share plumbing, retain scheduler distinctions and fallback/stream-demand behavior. |

Support stays shared: `D/{biquad,tpt_svf,hiir,kaiser,kernels,lanes,control,live,taps}.rs`,
`D/kernel_tests.rs`, `C/dsp.rs`, and `C/blocks/{lfo,uvi_lfo,envelope,analog_adsr,follower,glide,arp,pattern_arp,step_table}.rs`.
These include further product laws (notably `uvi_lfo`) but are already engine-owned.
Source storage/demand/channel/reuse helpers are playback infrastructure, not independent DSP
families. Keep golden JSON/binary tables, `filter/*native*`, and
`filter/ladder_wrapper_research.rs` as evidence/research, not product implementations to delete.

## Minimal configuration contract

Extend the current `I/fx` types incrementally; names below describe fields, not an additional API.

| Family | Core | Settings/laws that must be explicit |
|---|---|---|
| EQ / linear filter | One section executor per required realization: TPT, DF1/DF2T, explicit SVF; serial/parallel bank | Band shape, order, Hz/Q/dB or typed native domain, designer (matched/bilinear/spline/prototype), realization/precision, coefficient update schedule, state retention on enable. |
| Nonlinear filter | Shared integrators/sections in typed feedback topology | Feedback placement, solver/sub-model, knee/curve, drive/bias, stage taps/mix, rate domain. Select ladder/SVF/adaptive topology at prepare; no product class dispatch per sample. |
| Dynamics | Detector → gain computer → ballistics → gain/output, optional feedback and lookahead | Peak/RMS/true-peak/Hilbert, stereo link, detector domain, knee/ratio/direction, attack/release convention and program dependence, window/hold, sidechain, latency, ceiling/makeup. |
| Delay/modulation/spatial | Ring/read/write + heads + LFO/allpass + routing/mix | Interpolation and write order, delay/tempo laws, head crossfade, feedback matrix and ordered loop recipe, transport, oscillator/RNG, smoothing; spatial normalization and layout. |
| Space | Existing feedback network plus partitioned convolver | Line lengths, matrix kind, allpass/comb/filter placement, modulation/seed, decay bands, early/output taps, rate wrapper; impulse identity/shaping and partition sizes. |
| Color | Small fixed ordered recipe of filter/transfer/quantizer/rate stages | Curve/table or stateful circuit model, ADAA choice with valid antiderivative, oversampler/rate/latency, pre/post/feedback placement, rounding/denormal/DC rules, dither/seed. No mandatory oversampling for an identity/linear stage. |
| Source | Cursor/resampler + scheduler/window/read/mix | Kernel/quality, boundaries/loop crossfade, pitch/time clocks, scheduler, seed, grain capacity, streaming demand, explicit fallback. |

Separate three concepts currently called “law”: **value mapping** (stored/script domain →
canonical control), **design** (control → coefficients/topology), **execution policy** (timing,
precision, denormals and bypass/state behavior). Existing product enum values may remain
compatibility presets during migration. Add a neutral selector only with its first consumer;
keep measured numeric tables immutable and identified by stable data IDs, not executable
callbacks or opaque product-name strings. Keep script names/addresses in bindings. Do not
normalize away rounding steps needed by an admitted control path.

Preparation validates all bounds and reserves rings, convolution partitions, grain pools,
state and scratch; it resolves tables/designers without lazy initialization on first process.
The render plan contains typed prepared stages, not parsing/mapping logic. Constructors using
`OnceLock<Vec<_>>` in amp table loaders and `LazyLock<Spline>` in dynamics warrant a cold-start
audit; their presence alone does not establish an audio-thread allocation bug.

State is owned by voice/group/bus, with stable stage identity, model/config version, histories,
control phase, RNG and transport position. Persist the format parameters and model version;
make any supported live snapshot restore histories too. A serialization round-trip must not
silently change topology or reset delay tails. Preserve existing capabilities; do not invent
an engine-state serializer just for this cleanup. Enabling/disabling a format must not change
the DSP identity of already prepared settings; coordinate registry/cache IDs with the module lane.

## Acceptance and measurement contract (applies to every step)

1. Freeze old output/state before removing a path. Existing bit-exact goldens remain bit-exact
   for structural extraction. Native/emulator vectors verify the port, not the entire hosted
   effect. No tolerance increase to hide a refactor regression.
2. For an explicitly measured algorithm replacement allowed by the user: compare old and
   native/oracle response, maximum magnitude error **0.5 dB**, wrapped phase error **5°**,
   attack/release/RT60 error **10%**, over supported controls/rates. Use 44.1/48/96 kHz,
   log-frequency grid 20 Hz to `min(20 kHz, 0.45 fs)`, endpoints and representative interior
   settings. Exclude bins below -80 dB relative to reference peak from phase comparison;
   separately verify notch position/depth, DC and Nyquist behavior. Preserve reported latency
   and impulse arrival exactly. Small-signal response alone does not admit a nonlinear model.
3. For nonlinear replacements add level sweeps (-60/-24/-6/0 dBFS), DC, impulses and two-tone
   tests; require harmonic/intermodulation/alias components above -80 dBFS within 0.5 dB
   and preserve stability under max supported feedback. If unavailable or failing, keep the
   sub-model and extract only shared primitives. A blind substitution of `tanh` is not a pass.
4. Compare scheduled automation at frames 0/31/32/63/64 and across partitions 1/17/32/64/257,
   reset/retrigger, bypass/re-enable and supported save/restore. Compare each new path with
   the old under the **same host-call schedule**; legacy EQ has host-call-sensitive updates,
   so universal partition invariance would be a false requirement. Other paths retain their
   existing partition guarantees. Structural migrations require output and next-state identity.
5. Use existing assets below and the parity lane's `docs/re/engine/fx-measured/` **when it
   lands** (absent on this baseline). Do not make up native vectors, redo its oracle rig or
   silently promote `Partial`/`Generic`/`NotModeled` to exact.
6. Each implementation PR records base/head SHA, model/settings, channels, rate, frame count,
   build/profile/ISA, old/new instructions per stereo 64-frame block and ns/frame, state bytes,
   startup preparation and net production LOC. Release-mode deterministic input/output hash,
   warm runs, at least five timing repeats, report minimum and range. Count CPU only after
   preparation, plus a separate cold-start allocation check; include static and automated paths.
   Reject >5% instruction regression unless evidence and a concrete quality gain justify it.
   No CPU numbers were measured for this design; estimates below are hypotheses, not promises.

Build only `nice -n 10 build-slot cargo ... -p <one crate>`, sequentially. For example run
`test -p sampler-core <family-filter>` and, if touched, separate `sampler-ir` and
`sampler-kontakt`/`sampler-uvi` tests. Run scoped fmt/clippy through the same wrapper.
Dynamics already has `dsp::dynamics::tests::bench_every_law` (ignored release test);
other families need a small deterministic scoped benchmark in their implementation PR.
Collect `perf stat -e instructions:u` on the built benchmark executable, excluding Cargo and
compilation; use the wrapper's target location, never set `CARGO_TARGET_DIR`/`--target-dir`.
If hardware counters are denied, record that and use the scoped benchmark without invented counts.

## Ranked implementation tickets

Each numbered item is a small-PR series, not permission for a directory-wide rewrite.
Finish one named sub-model per PR when a row has several. Net LOC estimates exclude fixtures
and are non-additive. “0% CPU” means architecture/maintenance value, not a speedup claim.
For every ticket use the acceptance contract above; take the baseline before edits, delete
the replaced production path with its switch-over, rebase on `origin/main` immediately before
opening the PR. Preserve fixtures and provenance. Resolve shared-file ownership first.

### 1. EQ section execution (high value, low risk)

Files: `D/eq_family/{generic,kernel,tests}.rs`, `D/biquad.rs` (shared executor home).
Move `generic::run_df1` into one coefficient-driven DF1 section executor with explicit history
and flush policy; wire Solid only first. Keep `solid.rs` design/glide and legacy TPT unchanged.
Test: existing `solid_df1_section_is_the_native_cascade_record`, `geq_ramp_vectors.json`,
and frozen stereo DF1 output/next state under automation; bit-exact. Delete the private loop.
Estimate: 0–40 net LOC removed, ~0% CPU. This is the first safe seam, not the largest deletion.

### 2. EQ designers and control separation (high value, medium risk)

**Status after implementation (2026-10-24): no net deletion; gate fails for the bell change.**
Measured on `origin/main` before any edit:

- Knob conversions have a single implementation: `KontaktLegacyEqSettings::{frequency_knob,
  bandwidth_knob, gain_knob, frequency_hz}` at `crates/sampler-core/src/dsp/eq_family/legacy.rs:22-40`.
  Callers (`K/effects/eq.rs:34-35`, `engine_parameters.rs:269`, `library.rs:1446`) only read it.
  Moving it to `sampler-ir` is a relocation (0 net LOC), not a deletion; there is no duplicate
  conversion to delete.
- Solid bell `peak_iterative` (`solid.rs:281`, used only for HighMid below 60 kHz, `solid.rs:401-403`)
  against the closed-form `peak` (`solid.rs:269`). Native probe rows 53-56 (HMF, 10 probe
  frequencies, max error in dB):

  | row | iterative | closed form |
  |---|---|---|
  | f 0.25 q 0 | 0.027 | 0.107 |
  | f 0.25 q 1 | 0.029 | 0.108 |
  | f 0.75 q 0 | 0.028 | 0.174 |
  | f 0.75 q 1 | 4.036 | 6.148 |

  The closed form is worse on every row and fails the 0.5 dB gate; the iterative bell stays.
  Its own failing row (f 0.75, q 1, 4.0 dB) is an open probe row, not fixed by this change.

Result: no ticket-2 deletion is admitted. Ticket 2 is closed without a PR; knob-domain relocation
remains optional and needs no measurement.

### 3. Remove duplicated linear design/section code (high value, medium risk)

**Status: done (PR #552).** The duplicate was the UVI DigitalFilter/DigitalEq routine, implemented
twice: `filter/cascade.rs::digital_design` and `eq_family/digital.rs::section` (both cite 0x1412840f0).
The bank now uses `eq_family::digital`. Deleted: `digital_design`, `bilinear`, `bilinear2`,
`band_roots`, `csqrt`, `re_mul`, the bank's f32 first-order stage. Net -147 LOC.
Measured against the deleted designer (grid: kinds 0-6, orders 1-16, fnorm 0.001-0.45, Q 0.2-8,
gain -12..6 dB, bandwidth 0.5-2): response max 8.8e-5 dB, phase 3.4e-4 deg. The same comparison found
a real bug in the EQ copy at Q = 0.5 (exact square quartic; wrong root pairing, up to 36 dB), fixed in
the same PR. Native DigitalFilter vectors pass at 1e-5.

Original scope (kept for reference): the remaining linear-section helpers (`filter/linear.rs`,
`chamberlin.rs`, `eq_family/kernel.rs`, `tpt_svf.rs`) were not consolidated; each realisation keeps its
own state and discretization.

### 4. Dynamics recipe/settings boundary (medium value, low–medium risk)

**Status: closed without a code PR (2026-10-24): no net deletion is admitted.**

Audit on `origin/main`:

- The dynamics pipeline is already composed of stages (`dynamics/pipeline.rs` `Stage`/`run`; recipes
  in `dynamics/mod.rs`, `output.rs`, `ballistics.rs`). Nothing duplicates a recipe that would be deleted
  by a neutral selector.
- `rbj_low_pass` (`dynamics/pipeline.rs:206`) and `high_pass_1` (`:233`) are Kontakt-measured f32
  approximations (polynomial sine/cosine, native rounding). They are not copies of `biquad::rbj` or
  `biquad::first_order`, which are f64 designs with different arithmetic. Keep.
- The only duplicated recurrence is the dB one-pole with separate rise and fall coefficients:
  `dynamics/ballistics.rs:28-31` (`SmoothBranch::channel`, f64, floors below 1e-12) and
  `db_limiter.rs:110-123` (`DbLimiter::process` envelope and hold, f32, with the instant mode and no
  floor). Sharing it would need a float-generic helper and would keep the instant mode and the floor as
  separate branches, so the net LOC change is about zero. Not done.
- Moving `db_limiter.rs` into `dynamics/` is a relocation (0 net LOC), and its public path is used by
  `dspbench.rs:7`. Not done.
- `laws.rs:276` and `:348` use `8.656_169_89` and `8.656_170_245` for the dB-per-neper constant, not
  the `DB_PER_NEPER` value (8.685889...) used in `computer.rs:8`. The difference is a measured law
  or a precision choice, not a duplicate. Not changed.

Result: no deletion. The cold-spline audit (`LazyLock<Spline>`) is a cold-start check, not a LOC change.

### 5. Common table-shaper/rate wrapper (high value, medium–high risk)

Files: `D/color/amp/{mod,kernel,chain,cat,dstortion,acbox,twang,fuzz,bigfuzz}.rs`,
`D/color/{mod,skreamer,drive}.rs`, `I/fx/color.rs`.
Start with Cat and DStortion table lookup/polyphase helpers; identify identical operations,
then make their coefficients/tables/routing data. Leave their differing filters/state explicit.
Audit table initialization in prepare; no first-render lazy allocation. Extend only after the
first pair passes. Test: existing amp kemu fixtures referenced by these modules, color golden
tests, first-call allocation check and frozen state; bit-exact extraction. A simpler
`amp::chain::Spec` replacement needs nonlinear admission, not just a frequency sweep.
Delete replaced table-evaluation and rate-wrapper loops, retaining preset table assets.
Estimate: 100–400 LOC across the series; ~0% CPU expected; SIMD/fusion wins unmeasured.

### 6. One nonlinear-stage vocabulary (high value, high risk)

Files: `D/color/{shaping,distortion,saturator,dirt,diode,tubescreamer,tape,freak,bite,mod}.rs`,
`D/filter/shaper.rs`, `D/color/kolor/{chain,control,notify,shaper_0_1_3,shaper_2_5,shaper_4,shaper_6_9}.rs`,
`I/fx/{color,kolor}.rs`.
Start with a genuinely identical stateless curve/table operation, preserving control and loop
placement. Add `Memoryless`, `Circuit` and `Recurrent` sub-model settings only as consumed.
ADAA is opt-in for supported antiderivatives; oversampling keeps an explicit latency/phase
contract. Test: `color/tables/*vectors.json`, Kolor fixtures under `tools/re/fixtures/` and
color preset tests; bit-exact factoring, nonlinear gate for changed algorithms. Delete only
the replaced curve/conditioning helpers, never the GRU/circuit recurrence on name similarity.
Estimate: 100–350 LOC; CPU ~0%, extra ADAA/oversampling can cost CPU and requires evidence.

### 7. Shared delay readers, then model recipes (high value, medium–high risk)

Files: `D/timemod/{mod,uvi_legacy,dual,dual_x,twin,tape,buzz,wowflutter,psyche,velvet,diffuse,voices,mt}.rs`,
`D/stereo_tune.rs`, `D/ring_ramp.rs`, `I/fx/{timemod,stereo_tune}.rs`.
First consolidate one identical fractional reader/ring-boundary operation (linear versus cubic
remains settings). Then expose two-head/crossfade/feedback stages for Twin/Stereo Tune/Psyche;
keep tape and nested-allpass topology as recipes. Never change read-before-write order or
RNG draw schedule. Test existing `timemod/tests.rs`, native delay fixtures referenced by the
modules/format tests, and `stereo_tune_native_vectors.json`; bit-exact extraction, response/time
gate for alternative interpolation. Delete matching reader/clock/ramp copies as adopted.
Estimate: 100–300 LOC; ~0% CPU; ring sharing is allowed only for identical ownership/lifetime.

### 8. Replika primitive adoption (high maintenance value, high risk)

Files: `D/replika/{delay,read,write,deposit,head,chain,design,stage,mod,start,mode1,tests}.rs`,
`D/replika_head.rs`, `D/timemod/mod.rs`, `D/tpt_svf.rs`.
After ticket 7, move one proven identical reader/filter/head primitive behind neutral settings.
Retain path-specific start state, head trajectories and ducking; postpone replacing its whole
chain until missing-path evidence exists. Test `D/replika/tests.rs` using
`docs/re/engine/vectors/replika-native-*.json` plus frozen multi-head state; bit-exact.
Delete only superseded primitives; do not delete rings that hold independent history.
Estimate: 50–200 LOC; CPU ~0%; memory savings unknown, report actual peak state separately.

### 9. Oversampling and reverb boundary (medium value, medium–high risk)

Files: `D/{hiir,kaiser}.rs`, `D/space/{resample,network,design,mod}.rs`,
`D/space/design/{shelf,plate,raum,cosmic}.rs`.
Keep the already shared network and convolver. Compare the two Kaiser implementations before
sharing design/helpers: f32 register-order and f64 polynomial paths are not interchangeable.
Make FIR/IIR pair, ratio, precision and delay explicit; then remove one duplicate helper if
admitted. Keep matrix specialization and Cosmic's serial topology. Test Galois native impulses,
line/RT60 tests in `space/design/tests.rs`, Plate/Raum fixtures referenced by their modules,
impulse latency at every ratio; bit-exact extraction or response/phase/RT60 gate.
Estimate: 40–150 LOC; CPU ~0%; no whole reverb rewrite or generic dense-matrix substitution.

### 10. Spatial and incomplete effect state (medium value, high integration risk)

Files: `D/{surround_panner,ringmod,ring_ramp,stereo_tune,rotator,gater}.rs`,
`D/color/{stereo,widener,stereorizer}.rs`, `I/fx/{mod,ringmod,stereo_tune}.rs`,
proposed `I/fx/spatial.rs`, `K/effects/{surround,ringmod,stereotune,mangling}.rs`.
First add neutral spatial config for the already supported fixed stereo matrix and remove
format construction of core `Panner`. Separately replace Rotator byte-offset state with typed
fields; Gater already has named state fields. Move either into the modulation family only
after dispatch/use-site audit.
Test `surround_{panner,record,panner_driver}_native_vectors.json`, ring tests,
`gater_kemu_vectors.json`, Stereo Tune vectors and frozen Rotator state; existing tolerances
or stricter, no new admission. Delete core-type dependency and replaced offset accessors.
Estimate: 0–150 LOC; CPU ~0%. New multichannel/gater scheduling is a separate capability PR.

### 11. Source coloration and scheduler sub-models (high reach, high risk; last)

Files: `D/{machine,machine_tables}.rs`, `C/source/{time_machine,tone_machine,beat_slices}.rs`,
`C/{source,resample}.rs`, `I/lib.rs`; integration in `C/lower.rs` only with its owner.
First move S1200/MP60 settings/tables behind a shared fixed-rate coloration recipe using
existing quantizer/filter stages; preserve fixed-rate and rounding semantics. Then share grain
read/window/mix plumbing while keeping scheduler sub-models. Test existing machine/source
goldens, unity/pitch/time/loop tests, demand/starvation/fallback and pool-exhaustion behavior;
bit-exact extraction. Scheduler algorithm changes need their own native/quality evidence,
outside the linear response gate. Delete only replaced machine loops/grain read helpers.
Estimate: 100–300 LOC; CPU ~0% until benchmarked; protect plain sampler fast paths entirely.

### 12. Neutralize bindings and retire compatibility names (after consumers migrate)

Files: `I/fx/{mod,eq,filter,dynamics,color,timemod,space,ringmod,stereo_tune}.rs`,
`I/lib.rs`, `K/effects/*.rs`, `U/fx/*.rs`; coordinate `C/lower.rs` and registry owner.
Replace `uvi_lane`'s Kontakt-name coupling with format-owned binding tables; convert one
family's product variant into a neutral preset/config at a time. Keep stable cache/parameter
IDs or explicitly version conversion. Test both format-to-IR mappings, lane addressing,
serialized config round-trip, unsupported diagnostics and existing native runtime tests;
bit-exact audio/control behavior. Delete consumed aliases, raw-index knob arrays and old
product selector arms only after callers are gone. Estimate: 50–200 LOC; ~0% CPU.

## Simplifications worth keeping

- Treat frequency/Q/gain mapping as data once, not as per-product processor classes. But do
  not add a generic expression interpreter to evaluate three equations.
- Share compile-time building blocks; do not allocate one heap object per band/stage or add
  a virtual call per sample. Preserve static EQ/voice batching and sparse reverb matrix paths.
- Cache coefficient designs by actual control changes; do not cache across a required native
  control tick. Measure automated redesign separately from held-coefficient processing.
- Keep the best simple designer as the default for new formats. Carry a legacy designer or
  recurrence only with a named response/control/state test explaining its necessity.
- A table asset is not duplicated code. A product-named file that only builds a preset may
  remain readable; renaming it produces no audio or CPU improvement.
- Require a deletion list and measured result in each implementation PR. If a proposed
  abstraction adds more policy branches than code it removes, keep the existing small core.

This plan changes neither supported formats nor advertised parity. Future Kontakt, Falcon/UVI,
Ample, IKM pak and Superior Drummer modules target these same family settings; absent format
readers or measurements are not a reason to prebuild unused DSP variants.
