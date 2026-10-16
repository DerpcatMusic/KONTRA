# VWinds playback parity contract

The target is correct playback of the actual instrument, including its supported
playing controls. A generic source fixture, nonzero output, or a candidate which
refuses the actual preset does not satisfy that target. This document records
required behavior and evidence; it does not certify an implemented feature.

## Established facts

- Clarinet A's inspected primary source is a six-channel FLAC. Its channels
  3–6 contain substantial sustained periodic content. The published stereo
  reduction discards them before the keygroup matrix can use them.
- Initial file-loop inheritance was repaired independently. The same six-second
  held-note probe keeps a voice alive and wraps its source cursor, but its quiet
  stereo result does not establish body-routing parity.
- The actual keygroup retains three authored players, GainMatrix and OnePole.
  Two player bypasses and the OnePole bypass were stable in the recorded owner
  snapshots. Those snapshots do not establish future exclusivity or justify
  removing the retained players or processor.
- Original GainMatrix static evidence uses input-major control handles, resolves
  all channels through owner descriptors, accumulates contributions in input
  order, and uses output scratch before copying back. Descriptor handling and
  unresolved runtime vector dispatch remain distinct evidence obligations.
- Official UVI documentation places keygroup effects per voice, followed by
  layer/program processing. Exact oscillator sum, envelope/FXPostGain placement,
  generated-event grouping and downstream dimensional behavior require their
  corresponding original/reference evidence.

## Required coherent implementation

1. Preserve the real source channels and their immutable identities through
   ordinary loading, resident storage, streaming, resampling and loop traversal.
   A storage pack is not another oscillator, cursor, envelope or voice.
2. Represent a keygroup voice's oscillator membership and processor ownership.
   Independent attacks must not share nonlinear history; independent oscillator
   chains must not replace shared processing where that changes the native law.
   Keep round-robin decisions distinct from processor-sharing identity.
3. Execute the matrix at its original signal boundary, with the established
   channel layout, signed coefficients, accumulation precision/order and live
   parameter clock. Preserve source buffers until all destination channels have
   been calculated. Define absent-input and bypass behavior from evidence.
4. Preserve gain/envelope placement, FXPostGain, insert/rack order, bypasses,
   pre/post-fader sends and downstream routing. Resolve their real compiled
   bindings. A diagnostic graph's incomplete join is not a topology certificate.
5. Implement the target instrument's required modulation and script actions
   through real owner/state bindings. Matching a setter's type or silencing its
   warning is not an implementation of its effect. Native ignores remain ignores;
   boolean writes to float parameters must not become global coercions.
6. Retain one logical lifetime through startup, scheduling, release, stealing,
   cancellation, source updates and retirement. Old notes must remain releasable
   after policy revocation or a Lua fault. Shared state and resources must return
   through their actual off-audio owners.
7. Admit resource use before allocation: codecs, resident/streamed payloads,
   heads/pages, readers, file handles, DSP state and overlapping old/incoming/
   retired owners. Counts and ceilings must be supported by accounting; a late
   refusal after unbounded preparation is not protection.

## Acceptance evidence

Use independent original/reference witnesses for dimensional routing and state
scope before promoting the model. Include non-dyadic and signed matrix cases,
live changes and bypass, multiple selected oscillators, overlapping attacks,
generated/release events, and envelope/processor-order contrasts. A quarter-gain
fixture alone cannot establish native floating-point accumulation.

Then run normal public loader/runtime paths, retaining numerical expectations,
source and executable identities, source/cursor/state continuity, healthy stream
service and callback allocation/free checks. Match native source/event/controller
identity before interpreting differences; do not force a convenient round-robin
choice and call it a causal comparison.

Finally compare the actual held-note and relevant controller behavior with a
reference at the same source state. Measure late AC level and periodic content
alongside the full route, release and faults. Do not normalize away unexplained
gain or restore a historical peak as a native target. Install only after coherent
combined, native/editor and packaging checks; report remaining limitations.

## Current disposition

The immutable M2 packet is source-review material. It still blocks every new
bundle at ExecutionPending, requires seven typed-core ports, has open aggregate
resource obligations and rejects the actual three-player keygroup. It is not
accepted as the user's playback fix and has not replaced the installed binaries.

The combined Doctor/file-loop baseline was separately validated at
`bc24c9250ae5db30a4f9e498296f1ab473d0d57b`, tree
`03372ad8d3d9e54027979c6421da47c6d176e177`. Core library tests passed 565 with
11 existing ignores; the nonactivating source-bundle admission tests passed 15;
normal file-playback inheritance tests passed 15. Affected core/UVI all-target
Clippy passed. A complete same-policy Doctor report passed with zero error
findings, zero producer errors, 11,551 warnings and 85 informational findings.
Warnings remain visible; the net increase of 76 over the Doctor-only `c41e24dc1`
report is not classified as false positives. Exact diagnostic-ID comparison
also includes moved sites, so its additions/removals are not a count of new bugs.

All these commands ran under the common global lock, with unchanged tracked
source inputs and a freshly compiled core test executable whose package identity
names the integration worktree. Receipts and the warning delta are under
`PluginArtifacts/kontra-vwinds-parity-reference-20261016-r17/combined-quality02/`.
This cohort did not apply M2, read an actual bank, execute a native target, build
shipping plugins or change the installation. Earlier native/editor/full-matrix
results keep their separate source identities.

Root's partial M2 review also identified a numerical mismatch: its output fold
uses float64 products and accumulation, whereas the retained original matrix's
dynamic accumulation branch has separate float32 multiply/add/store boundaries.
The quarter-gain authored fixtures cannot establish equivalence here. The
original constant/first-contribution helpers remain a separate evidence gate.

References: [retained native topology](../re/DSP_FORMAT_SPECIFICATION.md),
[historical level investigation](../re/uvi-survey-vwinds-level-investigation.md),
[actual loop/source results](20261016-vwinds-sustain.md), and the
[official UVI structure](https://manual.uvi.net/falcon/en/structure/) and
[GainMatrix documentation](https://manual.uvi.net/falcon/en/effects/).
