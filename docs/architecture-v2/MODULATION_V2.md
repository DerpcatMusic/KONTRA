# Shared modulation and DSP v2

**Approved design — phase 1; Q/Gain finishes before phase 2 plumbing.**
Kontakt and Falcon use one engine for pan, frequency, pitch, volume, sources,
parameter routing and shared DSP. An import selects an exact native preset;
custom shapes, larger cascades and improved processing are explicit extensions.
This builds on `dsp/control.rs` parameter lanes, the shared engine-parameter
service, prepared ownership/state pools, and W6's settled-value render reuse.

## Open addresses and compiled routes

Replace closed modulation-target dispatch with a prepared parameter registry.
An address is `(scope, node_id, param_id)`, resolved within one plan generation.
Nodes include processors, sources, routes, and voice/group/bus output controls.
A processor's identity remains stable across amplifier splits, wrapper stages,
sparse native slots and cascades; compilation resolves addresses to dense lanes.
Every prepared processor publishes its parameters, not a hand-picked target list.
Source parameters expose rate/phase/intensity; route parameters expose depth and
control cadence. Meta-modulation uses the same addresses and evaluator.

Each descriptor declares units/law (linear, dB, semitones, octaves, or normalized
plus native curve), domain/range, combine operation, clamp/wrap/quantization,
precision/order, smoothing and update cadence. Evaluation is saved/base value +
route contributions, followed by the declared law; native multiplicative volume
is a declared combine operation, not an assumed additive gain. Enabled zero-depth
routes remain observable when native clocks or clamps depend on their presence.
The registry also supplies the Sound editor: name, unit, range, law and scope
metadata let W4 generate controls and route any source to any address. W15 does
no UI work. The same metadata drives script/automation and signal tracing.

Translators map native identifiers and physical namespaces to addresses and
preset laws. Kontakt internal-mod-slot `startPhase` must address its source,
not an FX slot. Case-sensitive native names/aliases are resolved there. Unresolved
addresses/laws use W12 grouped diagnostics `{kind, reason, subject}`: one record
per kind with counts and a location sidecar, never per-slot lines. They never
silently remove a route. Compilation validates scope ownership and topologically orders
meta-modulation. Cycles require an explicitly specified delayed edge; otherwise
report them. A voice source cannot drive a summed bus via an invented reduction:
its native scope/aggregation must be declared first.

## Sources and filters

One source registry supplies LFOs, envelopes, step sequencers, held/interpolated
random and expression/controller/script values. LFO shape is prepared data:
breakpoints with curves, a wavetable, or an exact stock evaluator. Kontakt/UVI
stock shapes are presets, retaining native phase, waveform mixtures, width,
random state, retrigger/shared clock, fade, bypass and version behavior. Custom
shapes use the same routes and storage. Edits prepare replacement data offthread;
rendering retains immutable tables and bounded per-instance state.

One shared filter service supplies stable SVF, ladder and comb primitives with
arbitrary finite cascades; order is bounded by prepared resources, not 24 dB/oct.
For example, eight 12 dB sections can produce a 96 dB/oct design. Each preset
specifies section order, feedback topology, gain compensation and oversampling.
A native nonlinear ladder cannot be replaced by independent SVFs merely because
their slopes match. New designs can select improved kernels/quality; imported
presets retain measured native arithmetic and timing by default.

## Ownership and separation

| Shared `sampler-core/src/dsp/`, `mod/` | Format adapter `sampler-kontakt/src/dsp/`, `sampler-uvi/src/dsp/` |
| --- | --- |
| Registry/address compiler, lanes, transforms, lag, source graph and state pools | Native ID/slot mappings, parameter curves, legacy precision/cadence/bypass quirks |
| Generic LFO/curve/table/envelope/random primitives and filter cascades | Exact Kontakt/Falcon source and filter presets, measured native wrappers |
| SVF/biquad, delay, generic comb, gain/mix, rectifier and resampling primitives | Existing Kontakt `ladder_kernel`, Daft scheduling/approximate wrapper, Formant I proxy and LoFi laws |

Envelope, compressor, stereo, reverb and convolution implementations remain
shared primitives only where both formats' measurements support their behavior;
native mappings and any divergent kernels belong to their adapter. The Formant
proxy and Daft approximation retain their current unverified status until measured.

Avoid a dependency cycle: adapters register immutable prepared kernel/source
implementations through safe core interfaces during plan construction. Core owns
reserved state and invokes a block operation over borrowed state/audio/lanes;
no file reader or native ID enters its render loop. Built-ins keep direct/SIMD
fast paths. Adapter dispatch is at block boundaries, with a batch entry when
supported. Adopt indirect block dispatch only if W6 quiet benchmarks show no
regression; otherwise adapters supply built-in kernel enum presets.

## Fidelity, CPU and delivery

Native-equivalence tests remain release gates: import/routing witnesses, original
instruction checks, offline gate-item A/B metrics, block partition/voice reuse,
and zero allocation/deallocation. Recovery counts do not certify PCM parity.
W12 recounts each READY slice and preserves native identity/bypass flags.

## CPU execution and acceptance budgets

Compile addresses into dense lanes by scope. Propagate dirty dependencies in
prepared order; reuse settled values and coefficients. Use absolute 64-frame
modern control cells, verified native cadences, and explicit audio-rate lanes
where fidelity requires them. Smooth affected targets. Batch independent
voices/channels through SIMD while preserving cascade and sum order; preallocate
all state and scratch.

Adopt indirect adapter dispatch only after quiet paired benchmarks show no p50
or p99 regression against enum dispatch; otherwise use enum presets. Per-route
and per-section budgets, benchmark cells and v1/native acceptance protocols are
in the [CPU plan](MODULATION_V2_CPU.md). These are proposed targets, not measured
performance claims.

## READY sequence

**Slice 1 is plumbing only:** generic address/lane registry, migrating every
existing ModTarget before adding missing families. It must retain bit-exact PCM
on all 22 gate items. Then small READY slices follow W12's grouped ranked table,
largest enabled counts first: missing parameters, source/meta targets and generic
LFO presets, shared cascades and remaining native presets. The in-flight Q/Gain
port may finish first. Each slice includes a failing-first witness, measured
A/B, RT heap check and W12 recount; retire old dispatch after consumers migrate.
