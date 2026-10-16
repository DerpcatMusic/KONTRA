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

References: [retained native topology](../re/DSP_FORMAT_SPECIFICATION.md),
[historical level investigation](../re/uvi-survey-vwinds-level-investigation.md),
[actual loop/source results](20261016-vwinds-sustain.md), and the
[official UVI structure](https://manual.uvi.net/falcon/en/structure/) and
[GainMatrix documentation](https://manual.uvi.net/falcon/en/effects/).
