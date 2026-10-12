# UVI Info observations and Mode1 coefficient reuse

This source follow-up to clean `cebaf809` is a bounded interoperability correction,
not complete Falcon parity. The resulting local package records its clean revision
and build hash in build-info and VERIFICATION.

## Info ownership and scope

Info uses `PartView::uvi_matches`, including exact bank/member/UUID, saved restore
state, epoch/generation, current part generation, rate and host capacity. Failed
owners remain inspectable; live header metrics use their stricter terminal-owner
gate. Stale generations, restored states and replaced sources hide retained Info
activity. A matching canonical terminal report tests this without exposing private
failure flags or adding a production API.

Optional graph counts remain unknown when absent. Initial resource totals exclude
later authored loads; observed worker-owned PCM includes them and exposes actual
alias/revision counters. Last initial resources are distinct from current decoding.
Ready/phase observations use the existing 250 ms loader cache; they are not exact
endpoint or fault observations. Render wall means divide by observed attempts;
CPU means divide by measured CPU samples. Missing measurements are unavailable.
Render timing excludes queue waits and UI snapshot work.

Canonical failure timing and PCM summaries deduplicate independently. A nonempty
canonical activity summary owns its PCM line; a timing object alone cannot hide
PCM seeded before the first rendered attempt.

## Mode1 consumed coefficient

`ModulationGraph::control_updates` reuses its own `AbsoluteClock::producer.alpha32`,
whose formula is identical to the previous connection coefficient. Valid input,
rate/block equality, already-previewed and span-limit checks precede lookup. New
Program construction starts cold; retargets cannot change its rate. Mode0 retains
its separate clock map. There is no extra field/cache, scheduling change, source
invalidation assumption or admission widening.

A cold unchanged target can populate the computational cache earlier. Exactness
assumes the existing fixed worker floating-point environment; powf-call/exception
chronology is intentionally not claimed identical. One repeated coefficient powf
per admitted Mode1 producer logical block is removed after reuse. No throughput,
whole-bank or user-DAW benefit was measured.

## Verification

All 14 selected synthetic functional checks pass. Two new tests and one extended
editor fixture cover legacy reference output/filter/publication/state bits at five
rates and three block widths, cold/retarget/signed-zero/subnormal/duplicate input,
unchanged errors, Mode0 isolation, ownership/restore/replacement, truthful count
and timing scopes, seeded zero-attempt PCM and related existing clock/metrics gates.
Authored full-editor captures at 1180×760 and 900×640, plus an authored terminal
failure before rendering, were inspected and independently accepted. Compact row
bounds pass; metric text wraps and ordinary vertical scrolling remains. Synthetic
values are fixture data, not current-instrument measurements. Scrolling interaction,
real paid-bank playback, native oracle equivalence and current DAW behavior were
not exercised. Prior checkpoint checks remain historical evidence.
