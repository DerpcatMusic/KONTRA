# Per-voice cost: envelopes, LFOs, modulation, filters, amp stage

Lane: `perf-research-modulation-filters`. Status: **negative result for the
lane's own code paths; the plain-voice cost lives in the sample cursor and
per-block bookkeeping, not here.** No engine change landed.

## Metric

CPU cost of one plain sample voice (no filter, linear/cubic interpolation,
amp envelope, stereo out, resident sample), per 64-frame block at 48 kHz,
single thread, niced, measured with `sampler-perf` plus `perf stat`.

## Baseline (measured)

`sampler-perf run --only dense-strings --cell 64,1` on `origin/main` at
8547a6de7, release build, host load ~34 (other lanes running). Raw output:
`/data/dev/workspace/BuildScratch/perf-lane-base-dense.json`.

| Counter (whole run, 6000 blocks) | Value |
|---|---|
| instructions | 34.87 G |
| cycles | 12.93 G (IPC 2.70) |
| cache-misses | 56.1 M |
| core render p50 / p99 / max per block | 427 / 3482 / 15870 µs |

Per-voice µs is **not yet isolated**: the bench reports whole-run counters and
does not expose average active voices per block, so ns/voice-frame cannot be
derived from it honestly. The 16/64/256-voice sweep needs a voice-count knob
in the bench (or a single-note micro-bench) before any before/after claim.
The load was too high for the wall-clock p50 to mean anything.

## What the code already does (verified in source)

- **Control-rate modulation exists.** Modulation advance is cached on the
  absolute control grid and prepared per block
  (`crates/sampler-core/src/render.rs:369-400`, `prepare_voice` at
  `render.rs:664`). Sparse routes are compiled into `voice_modulation`; a voice
  with no program skips projection (`render.rs:698`).
- **Steady-state envelope is already free.** `EnvelopeState::constant_level`
  (`crates/sampler-core/src/blocks/envelope.rs:524`) returns the sustain level
  with no per-frame call; the mix loops (`source.rs:1859-1880`,
  `source.rs:1727-1757`) take the `constant` path. Per-sample
  `envelope.next()` runs only in attack/decay/release, and only for native
  envelopes (`shape.native`) it always runs per sample (`envelope.rs:524-528`).
- **Filters are control-rate already.** The AR kernel recomputes its
  coefficients once per 32-frame tick with precomputed resonance terms
  (`crates/sampler-core/src/dsp/filter/ar_kernel.rs:1-120`, `Resonant` at
  line 50). `ArKernel::tick_resonant` (~11.5% of the convolution profile per
  the baseline) runs its per-sample zero-delay sections only when a filter
  processor is in the chain; a plain voice has none.
- **Batching exists.** Consecutive same-plan, unmodulated voices render as a
  lane batch (`render.rs:440-470`, `render_batch`).
- **Pan/gain laws** are already per-block (`expression.rendered.gains`).

## Candidate techniques (evaluated, not prototyped)

| Technique | Verdict | Why |
|---|---|---|
| Control-rate modulation (16/32/64-frame ticks, linear ramps) | Already in place | `prepare_voice` per block; `Ramp` in `voice_mod.rs`. |
| Sparse modulation matrix compiled at note-on | Already in place | `voice_modulation` program per voice; no-program voices skip. |
| Closed-form per-block envelope segments | Not worth it for plain voices | Steady state is constant; attack/decay is short. Native envelopes would change output bits against vectors. |
| Skip filter when bypassed/neutral | Already in place | No filter stage in chain = no filter cost. |
| SVF/ladder/biquad vectorised across voices | Deferred | Only matters for filtered voices; the lane's filter work is not on the plain path. Needs a filtered-voice bench first. |
| Coefficient tan/exp tables, per-block recompute | Partly in place | AR already uses `table::exponential` lookups per tick. Remaining exp/tan in other cores is unmeasured. |
| One-pole parameter smoothing at control rate | Already in place | Ramps from `voice_mod` and the AR `adaptation` one-pole. |
| FTZ/DAZ, DC-offset denormal guard | Not evaluated | Needs a denormal-tail measurement; not on the plain path. |
| Equal-power pan table | Not needed | Gains are per-block already. |

## What would settle the plain-voice number

1. A bench knob for simultaneous voices (16/64/256) on one resident
   sample, so instructions per voice-block can be derived.
2. A micro-bench around `Cursor::render` (`source.rs:1184`) with a fixed
   envelope and gain, timed per 64 frames. The interpolation and per-block
   setup, not the envelope, are the likely cost here. That belongs to the
   sample-cursor lane, not this one.

## Sources

Not fetched in this session. Listed from working knowledge; **verify before
citing**:

- Zavalishin, *The Art of VA Filter Design* (Native Instruments, TPT/ZDF
  filter design): the zero-delay kernel behind `ar_kernel.rs` and
  `tpt_svf.rs`.
- Simper, Cytomic SVF notes (`cytomic.com/files/dsp/SvfLinearTrapOptimised2.pdf`).
- sfizz, `github.com/sfztools/sfizz` (BSD-2): sample voice and control-rate
  modulation design. Ideas only.
- Surge XT, `github.com/surge-synthesizer/surge` (GPL-3): voice SIMD and
  envelope handling. Ideas only; no code copied.
- Vital, `github.com/mtytel/vital` (GPL-3): envelope and filter voice layout.
  Ideas only.
- LinuxSampler, `linuxsampler.org` (GPL): voice and envelope lineage. Ideas
  only.
- FluidSynth, `github.com/FluidSynth/fluidsynth` (LGPL): modulation
  processing per block.

No code was copied from any of these. `LICENSES.md` is unchanged.

## Decision

No PR. The lane's own paths are already at their steady-state floor for a
plain voice, so no change here clears the 20% bar. The next measurement
should target the sample cursor and per-block voice bookkeeping.
