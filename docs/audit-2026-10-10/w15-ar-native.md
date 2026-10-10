# W15 AR filter identity and numerical admission

Status: static association and bounded original-byte numerical checks GREEN;
Rust checkpoint and playback/runtime/address admission checks pending.
Takeover preserves the original 2e-6 admission tolerance.
No AR subtype is admitted by this document alone.

Approved specification: read-only `t3code-80fe786b/artifacts/engine-analysis-2026-10-07`.
Native executable SHA-256:
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
Original instructions are read without launching the executable or changing its prefix.

## Authoritative feature documentation

The current unversioned Kontakt Manual documents all nine adaptive-resonance
filters at <https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference>:
`#ar-lp2`, `#ar-lp4`, `#ar-lp2-4`, `#ar-hp2`, `#ar-hp4`, `#ar-hp2-4`,
`#ar-bp2`, `#ar-bp4`, `#ar-bp2-4`. Resonance decreases with high input amplitude
and increases at lower levels; two/four-pole slopes are 12/24 dB per octave,
and combined modes mix the two- and four-pole responses.

The KSP Manual at
<https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters#filters-and-eqs>
defines `$ENGINE_PAR_CUTOFF` and `$ENGINE_PAR_RESONANCE` for all filters, and
`$ENGINE_PAR_EFFECT_BYPASS` for all filters/EQs. The coordinator/scout retrieved
and archived these exact sections on 2026-10-10. The manuals establish feature
and control intent, not coefficient law. The KSP sidebar identifies additions
through Kontakt 8.12; applicability to the 8.13.1 reference binary is bounded,
not independently version-pinned by the unversioned manual.

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
explains the cap discrepancy. The subsequent signed-index run still failed
saved100/48kHz/control-ramp by 2.6226043701171875e-6; admission remains closed.

## Takeover: adaptation rounding

Static original instructions in ramp entry `0x140af1c10` establish that the
adaptation rate is rounded as `f32(rate_inverse * 2092.300048828125)` at
`0x140af2109`, before the target-minus-state delta multiplies that coefficient
at `0x140af2146`. The recovered model/kernel instead evaluated
`f32(f32(delta * rate_inverse) * 2092.300048828125)`. These expressions are
not interchangeable in f32. Both independent implementations now preserve
the native grouping. Static `0x140af0632` and ramp `0x140af1f30` also show
the two-pole detector adding an already doubled band into the stereo sum;
the recovered implementations instead added each band twice. Both now use
the observed two-pole sum, retaining the separate four-pole branch.
The detector release still uses its observed separate products; no generic smoothing helper or fused operation replaces either law.

The same static ramp path updates normalized resonance before applying its
scale (`0x140af1d54..0x140af1d78`), disproving the earlier scaled-domain-ramp
hypothesis. `tools/ar-ramp-diagnostic.py` records synthetic whole-block,
one-frame-partition and per-channel state evidence without starting a host.

The oracle and independent Rust equations were recovered from immutable WIP
`0e9a85c26b8341a240c35ecbad752441e5c9d45a`, rather than modifying the old dirty
checkout. The integration worker replayed exact source
`2252a3a869d50a10f0b894f22bad2460357885ca`: all 243 static/ramp/retune cases
passed with maximum error 0.0, plus the wrapper-clock checks. The tolerance
remained 2e-6. Receipt:
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/takeover-dsp-20261010/ar-native.json`
(SHA-256 `98316c475bb02cf040eec91b44156a04de38363fad112646f843a751b0b7d5bf`).
Run metadata is `ar-native-run.json`; its log SHA-256 is
`91c6b335724ecf4441bdeae0fcd70e5afefa0add8c3ee3ab02a91be65381233b`.

`dsp/ar_native_vectors.json` retains the binary identity, all 81 synthetic
scenarios and their static/ramp/retune checkpoints, frame indices and oracle
limitations from that receipt. Only the kernel's test-only module is enabled.
This is numerical replay evidence, not native host or library playback evidence.
The combined integration batch must run the Rust checkpoint test before
runtime/address admission. No build or numerical replay was run by the takeover
DSP worker. No AR processor or importer admission has been added.

## Test-only runtime preparation: pending-request priority

The immutable WIP runtime proposal was recovered from `0e9a85c2` into
`dsp/ar.rs`, enabled only under `cfg(test)`. It reserves 22 stereo f64 cells
(352 bytes) per instance; the exponential table is initialized during prepare,
not during processing. Rendering copies fixed state and performs O(frames)
work without heap ownership. This is source design, not allocator or CPU proof.

Native clock `0x140b03360..0x140b033a5` handles a pending request before the
old countdown completion branch at `0x140b03424`. The preserved proposal did
these in the opposite order, potentially snapping to the old target before
re-requesting. `Ramp::advance` now selects request OR completion; an unchanged
pending target still restarts the full duration. The tiny-step branch also
clears remaining time, as `0x140b0340f` does. Tests explicitly exercise
request-priority, repeated targets, tiny steps and instance-state partitioning.
These Rust tests await the combined integration batch; they are not native
runtime evidence on their own.

`tools/ar-runtime-native.py` prepares a natural-dispatcher wrapper receipt:
all nine modes, five rates, four cutoff/resonance enabled-lane masks, four
32-frame quanta with an unchanged repeated target. It compares original wrapper
processing partitions `[32]` vs `[1,7,24]`, and retains synthetic PCM checkpoints
and ramp-state snapshots. It never forces DSP active flags. Outer rack mixing
helpers remain replaced. Native replay and the Rust comparison are still
pending; importer/service-address/voice/bus/heap gates remain closed.
