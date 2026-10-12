# W15 AR filter identity and numerical admission

Status: static association established; native numerical checks pending.
No AR subtype is admitted by this document alone.

Approved specification: read-only `t3code-80fe786b/artifacts/engine-analysis-2026-10-07`.
Native executable SHA-256:
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
Original instructions are read without launching the executable or changing its prefix.

## Identity

`BParFXFilter::read` (`0x140d00db0`) calls the serialized-kind converter
`0x140cf4c80`. Its range branch maps saved IDs 100..108 to internal 76..84.
The group effect factory `0x140978a00` sends this entire internal range to
constructor `0x1407c9360`, whose final wrapper vtable `0x144f11bb0` identifies
`BFilterDJ`. Its embedded core starts at wrapper offset 0x210 and is constructed
by `0x140af43f0` with `NI::KONTAKTFX::FilterDJ` vtable `0x144fcf4b8`.

Wrapper selector `0x140af0080` converts internal kinds 76..84 into modes 0..8
and invokes `0x140b044f0`. A single table therefore covers this family:

| Saved kind | Internal kind | Mode | Topology | Output |
|---|---|---|---|---|
|100|76|0|2-pole|low|
|101|77|1|2-pole|band|
|102|78|2|2-pole|high|
|103|79|3|4-pole|low|
|104|80|4|4-pole, distinct band branch|band|
|105|81|5|4-pole|high|
|106|82|6|combined 2/4|low|
|107|83|7|combined 2/4|band|
|108|84|8|combined 2/4|high|

The selector's output weights at core offsets 0x60/0x64/0x68 are respectively
`[1,0,0]`, `[0,1,0]`, `[0,0,1]`, repeated across the three topology groups.

## v1 and v2 comparison

Pinned v1 `0cb7a8a0:src/engine/filter.rs` maps 100..105 to its adaptive ladder
proxy and omits 106..108. It does not implement the recovered native FilterDJ
law. Current v2 shipping source `f199980d` drops 100 and 103 as well as 106;
it does not already map them. The existing v2 Daft processor explicitly uses
an unverified oversampled filter proxy and cannot establish AR parity.

## Numerical contract under examination

Core setter `0x140b05840` has cutoff, resonance and subtype controls.
Cutoff recomputation `0x140b04a30` uses the independently verified exponential
table `T[i] = 2^(i/60 - 20)`, adjacent f32 interpolation, and position
`min(145*x,140)*5 + 1381.881591796875`. Its pole coefficient is a fifth-order
polynomial in frequency/rate, not the Daft proxy's tangent law.

Processing dispatcher `0x140afedc0` distinguishes 2-pole, 4-pole and combined
branches, and a separate mode-4 band path. It selects static or ramping entry
points. The per-channel state has nine f32 cells at core offset 0x70, stride
0x24. The sections clamp integrator states before soft saturation. A shared
stereo detector follows band output, releases with rate scaling and reduces
feedback as signal level rises. This is a distinct native AR law.

`tools/w15-ar-native.py` executes bounded original converter, selector,
initialization, parameter, reset and processing routines. Its independent
equation model checks all nine modes across nine rate/control/level cases
(five rates, normalized endpoints and silence), including static, ramp-selected
and retuned paths. All eight enabled-lane masks are checked across three
initial countdowns and irregular blocks. Outer rack mixing helpers and CRT
memset/array traversal are replaced;
original ramp constructors execute. The exponential table is initialized from
the verified law, which does not claim native CRT pow last-bit equivalence.
Inputs are synthetic. Receipts contain metrics and selected synthetic
checkpoints, never library PCM or decrypted payloads.

Remaining admission gates: run the bounded check; verify real control ramps,
default controls and edge values; RED-to-GREEN translation/routing tests;
worker-reserved state with no render allocation; installed-library offline
witness and area no-run. CPU acceptance requires a separately owned quiet run.

Native clock `0x140b03300` keeps each ramp's countdown and active flag.
A pending target re-requests the full ramp duration, even if unchanged; a
step whose square is below 1e-15 snaps to its target and clears the active
flag. Completed ramps copy the exact target. Configured duration is
`max(1, trunc(rate * .001 / 32 + .5))` quanta. The wrapper
`0x1408f9d90` maintains its own 32-frame countdown and advances control-lane
pointers by one float per quantum, separately from its audio offsets.

Prepared, disconnected source: `dsp/ar_kernel.rs` (independent equations)
and `dsp/ar.rs` (22 worker-reserved stereo cells, no heap ownership in
rendering). The proposed IR/translation/routing patch is retained only in
the W15 receipt directory until numerical admission. Current AR slots remain
explicitly unsupported. The first target test is decisive RED on saved100.

The first expanded original-byte run failed at saved100/96kHz/resonance1:
0.3169 peak error in the independent model. Original instructions at
`0x140af1fd2` sign-extend AX for the second Hz-lane exponential lookup.
Its guards test a signed 16-bit index, rather than the wider converted integer.
The model and Rust candidate now preserve this wrapping conversion. This
explains the cap discrepancy; a fresh bounded run is required, not yet passed.
