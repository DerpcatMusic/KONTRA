# 03 DSP: one folder per DSP type

Every DSP type has a folder. The folder holds the type's parameters (via the
catalog), its code, its laws and the vectors that prove it. A format never
contains DSP; it picks a module and a law.

## 1. Folder anatomy

```
crates/dsp/dynamics/                 kontra-dsp-dynamics
├── Cargo.toml
├── src/
│   ├── lib.rs                       pub use compressor::Compressor; … (one screen)
│   ├── detector.rs                  peak / RMS / log-domain detectors (shared parts)
│   ├── computer.rs                  gain computer: threshold, ratio, knee
│   ├── ballistics.rs                attack/release smoothing (linear, log, program-dependent)
│   ├── compressor.rs                Compressor = detector → computer → ballistics → makeup
│   ├── limiter.rs                   Limiter = compressor with lookahead + ceiling law
│   ├── gate.rs                      Gate / Expander (computer below threshold)
│   └── transient.rs                 Transient shaper (dual envelope difference)
├── laws/                            data, loaded at prepare, never on audio
│   ├── kontakt/solid_bus_comp.toml
│   ├── kontakt/supercharger.toml
│   ├── uvi/thorus_comp.toml
│   └── generic.toml
├── tests/
│   ├── compressor.rs                behaviour tests through the public API
│   └── vectors.rs                   vectors!("vectors/*.json")  (08-MACROS.md)
└── vectors/                         native captures: input, params, expected output
    ├── kontakt_solid_bus_comp_ratio4.json
    └── …
```

The module's parameters live in the catalog, `crates/model/params/src/dynamics.rs`,
so the IR and formats can name them without compiling DSP code (01-LAYOUT.md §2).

## 2. The module contract

`kontra-dsp-core`:

```rust
/// A DSP module. Prepared off-audio; process/set/reset are real-time safe.
pub trait Module: Send + 'static {
    type Params: ParamSet;                       // from kontra-params, #[derive(Params)]
    const KIND: ModuleKind;

    fn new(params: &Self::Params, law: &Law, ctx: &PrepareCtx) -> Self;
    fn set(&mut self, id: <Self::Params as ParamSet>::Id, value: f32);   // smoothed internally
    fn process(&mut self, io: Io<'_>);           // stereo planar, len ≤ MAX_BLOCK, in place
    fn reset(&mut self);
    fn latency(&self) -> u32 { 0 }
    fn tail(&self) -> Tail { Tail::None }        // for voice/bus end detection
}

pub struct Io<'a> { pub l: &'a mut [f32], pub r: &'a mut [f32], pub side: Option<[&'a [f32]; 2]> }
pub struct PrepareCtx { pub rate: f32, pub max_block: usize, pub oversample: Oversample, pub poly: bool }
```

Shared building blocks in `kontra-dsp-core` (each one file, used by every family):
`smooth.rs` (one-pole, linear ramps, the one parameter smoother),
`oversample.rs` (HIIR halfband, today's `hiir.rs` + `kaiser.rs`),
`biquad.rs` (coefficients + DF2T/SVF forms), `db.rs`, `denormal.rs`,
`delay_line.rs` (interpolated, fractional), `allpass.rs`, `waveshape.rs`
(table and polynomial shapers), `lfo.rs` (the one LFO oscillator).

## 3. The families

Each row is one crate under `crates/dsp/`. "Classes" are the only DSP
implementations in that family. Everything a vendor adds is a law.

| Folder | Classes | Absorbs today's |
|---|---|---|
| `filter/` | `Svf` (TPT, LP/BP/HP/notch/peak/LP-BP morph, 1–4 pole cascades), `Ladder` (4-pole, drive, HQ oversampled), `BiquadCascade`, `Formant` (vowel bank), `Comb`, `Phaser` (allpass chain) | `filter/{svf,tpt_svf,cascade,chamberlin,linear,ladder,ladder_kernel,ladder_wrapper_research,ar,ar_kernel,daft,formant,phaser,shaper,table,kontaktfx}.rs` (~10.9k) |
| `eq/` | `Band` (bell/shelf/cut, Q laws incl. proportional-Q), `ParametricEq` (N bands), `GraphicEq` | `eq_family/{digital,generic,legacy,solid,kernel}.rs` (~2.4k) |
| `dynamics/` | `Compressor`, `Limiter`, `Gate`/`Expander`, `Transient` | `dynamics/**`, `db_limiter.rs`, `gater.rs`, `bus_amp.rs` (~4.2k) |
| `delay/` | `DelayLine` stereo/ping-pong/multi-tap, `ModDelay` (chorus/flanger/ensemble voice bank), `Tape` (wow/flutter + saturation law), `Diffuse` | `timemod/**`, `replika/**`, `replika_head.rs`, `taps.rs` (~9.3k) |
| `reverb/` | `Fdn` (network + design laws: room/hall/plate/cosmic), `Convolver` | `space/**` (~5.3k) |
| `drive/` | `Shaper` (oversampled waveshaper + pre/post tone), `Amp` (tonestack → shaper → cab IR), `Crush` (bit depth, rate, lofi), `Exciter` | `color/**` incl. amp models, kolor, skreamer, tubescreamer, freak, bite, dirt, redux, lofi, tape-sat, `machine.rs`, `machine_tables.rs` (~11k) |
| `spatial/` | `Pan` (laws), `Width`/`StereoMatrix`, `Rotator`, `SurroundPan`, `RingMod`, `StereoTune` | `rotator.rs`, `surround_panner.rs`, `stereo_tune.rs`, `ringmod.rs`, `ring_ramp.rs`, `color/{stereo,stereorizer,widener,hilbert}.rs` (~3.3k) |
| `modulators/` | `Envelope` (multi-segment: AHDSR, flex, analog curves), `Lfo` (shapes, multi-LFO sum, sync), `Step`, `Follower`, `Glide` | `blocks/{envelope,analog_adsr,lfo,uvi_lfo,step_table,follower,glide}.rs`, `voice_mod.rs` evaluation parts (~5k) |

Not DSP, moved out: `blocks/{arp,pattern_arp}.rs` → `kontra-vm` host services
(they generate events, not samples); `dsp/{lanes,kernels}.rs` → `kontra-simd`
and `kontra-voice`; `dsp/control.rs`, `live.rs` → registry parameter routing.

**Amp models and named units.** A guitar-amp model, a "Skreamer" or a
"Kolor" curve is not a class. It is a law for `drive::Amp` or
`drive::Shaper`: tonestack coefficients, a transfer table, pre/post EQ,
bias and the oversampling factor. Their files become
`laws/kontakt/<unit>.toml` plus vectors. A unit keeps its own class only if
a vector shows the topology cannot be expressed by the family's classes,
and the class doc comment names that vector.

## 4. Laws

`kontra-law`:

```rust
pub enum Law {
    Linear { lo: f32, hi: f32 },
    Exp { lo: f32, hi: f32 },                     // frequency-style tapers
    Db { lo: f32, hi: f32 },
    Table(Arc<[f32]>),                            // evenly spaced, interpolated
    Breakpoints(Arc<[(f32, f32, f32)]>),          // x, y, curve
    Formula(FormulaId),                           // a named, tested closed form
}
pub struct ModuleLaw { pub params: SmallMap<ParamId, Law>, pub tables: SmallMap<TableId, Arc<[f32]>>, pub flags: u32 }
```

- A law file is TOML next to the family (`laws/<vendor>/<unit>.toml`),
  embedded with `include_str!` and parsed at prepare (no build script).
- `format-*` crates refer to laws by id (`dynamics::laws::KONTAKT_SOLID_BUS_COMP`),
  generated by `laws!` (08-MACROS.md) from the `laws/` folder listing.
- Parameter mapping (knob 0..1 → Hz/dB/ms) is a law, applied once when a value
  arrives, never inside the sample loop.

## 5. The registry (`crates/dsp/registry`, crate `kontra-dsp`)

```rust
kontra_macros::modules! {
    filter::Svf, filter::Ladder, filter::BiquadCascade, filter::Formant, filter::Comb, filter::Phaser,
    eq::ParametricEq, eq::GraphicEq,
    dynamics::Compressor, dynamics::Limiter, dynamics::Gate, dynamics::Transient,
    delay::DelayLine, delay::ModDelay, delay::Tape, delay::Diffuse,
    reverb::Fdn, reverb::Convolver,
    drive::Shaper, drive::Amp, drive::Crush, drive::Exciter,
    spatial::Pan, spatial::Width, spatial::Rotator, spatial::SurroundPan, spatial::RingMod, spatial::StereoTune,
}
```

generates:

- `enum AnyModule { Svf(Svf), … }` with `process/set/reset/latency/tail`
  dispatched by one `match` (no `dyn`, inlinable);
- `fn build(spec: &ModuleSpec, ctx) -> AnyModule` (from the IR's data-only spec);
- `fn param(kind, name) -> Option<ParamId>` and the metadata table used by UI,
  automation and KSP `$ENGINE_PAR_*` mapping;
- `Chain` = `Box<[AnyModule]>` built at prepare; bypassed modules are not in it.

This replaces both `Processor` enums (IR and core), the hand mapping in
`sampler-core/src/lower.rs`, the parameter routing `match` in `dsp.rs`
(`(Processor::Eq(eq), Cutoff | Frequency) => …`), and most of the 824
`Processor::` sites. **Adding a module = one folder + one line here + its
params file in the catalog.**

## 6. Voice vs bus processing

The same module runs per voice or on a bus. `PrepareCtx::poly` tells it
which, so a module can share coefficient work across a voice batch. Voice
chains are batched by `(kind, law)` across voices into SIMD lanes
(`kontra-simd`); a bus chain runs scalar stereo. There is one chain runner,
not ~54 `render*` variants.

## 7. Quality bar

- Every class has vectors from a native capture or a closed-form reference,
  plus behaviour tests (stability at extreme params, no denormals, silence in →
  silence out, reset = fresh).
- "Close enough" is the measured bar (ENGINE.md KISS): a law's vector
  tolerance is stated in its TOML (`tolerance_db = 0.1`).
- Block-size independence: processing 1×N equals N×1 within float tolerance (a
  generated test per module, 08-MACROS.md `module_tests!`).
