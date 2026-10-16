# Clarinet A sustain investigation

This records measured behavior of the published 0.3.503 source, separately
compiled opt-in observers, and a validated initial-loop correction. It is not
a claim of native equivalence, all-VWinds support, or completed channel routing.
The installed plugin remains 0.3.503; no diagnostic candidate has replaced it.

## Source and output evidence

The selected Clarinet A source is a six-channel, 44.1 kHz FLAC with 196,510
frames. Its encoded payload SHA-256 is
`06cc62e2308af040f85414f976a346326976bc402d9aecc0468f7ca0addf3bf5`.
The normal decoder's original-channel path was used in bounded, off-audio
memory; only numeric measurements and hashes were retained.

In the source-clock 1.75–2 second window, channel AC levels in dBFS were:

| Channel | AC dBFS |
| --- | ---: |
| 1 | -89.50 |
| 2 | -40.36 |
| 3 | -27.07 |
| 4 | -20.67 |
| 5 | -17.25 |
| 6 | -15.68 |

Channels 2–6 have strong periodic candidates near 261.6 Hz, with subharmonic
candidates also present. This does not identify a unique fundamental or prove
native rendered timbre. Initial resident and streamed loading currently keep
only the first two channels. The saved keygroup matrix routes all six inputs
to its first output. Its saved coefficients alone do not establish live
coefficients, bypass behavior, downstream channel width, or effect placement.
A decode-time sum would lose those semantics and is not an accepted repair.

The same file contains a continuous wrap loop at source frames
`[22050, 196510)`. The selected program has no stored PlaybackOptions. Published
initial loading does not apply that file playback metadata. The file's root
64 must remain distinct from the oscillator's authored BaseNote 60.

A normal-loader six-second held key 60, velocity 96, without injected CCs,
ended with zero physical voices despite continued note ownership and no note
end. Output AC levels in the left channel fell from -52.61 dBFS at 3.75–4
seconds to -117.49 dBFS at 5.75–6 seconds. Runtime health counters were zero.
No persistent cursor wrap was observed past the file's approximately 4.455
second duration. Earlier two-second output was faint but periodic; CC7/11
full and a separate CC1 condition did not restore its level.

## Authored regressions and admission boundaries

Unchanged authored WAV, AIFF and FLAC fixtures exercise missing file-loop
inheritance through both resident and streamed public loaders. Five cases
reached the expected missing-loop assertion. The AIFF streamed case instead
failed earlier during header admission; it is not counted as a sixth causal
loop failure. Source inspection found that the vendored AIFF demuxer includes
the eight-byte SSND prefix in its advertised audio length. A separate checked
decoder correction and admission regression were required before loop testing
could claim six causal failures. The isolated explicit-options regression
reproduced streamed admission failure, then passed after excluding the eight
prefix bytes. Three parser tests passed. With only that decoder correction,
all six unchanged cases reached the specific missing-loop assertion.

Both baseline controls passed: stored empty/nonempty playback options retain
precedence, and a player with explicit options can ignore unsupported file
sampler metadata. Independent review additionally requires metadata failure
to remain local to inheriting players when a valid audio asset is shared with
explicit-options players or impulse consumers. The corrected loop candidate
passed independent review with no findings and all 15 public-loader tests,
including mixed shared-asset and exact impulse regressions. Metadata conversion,
checked admission and IR-marker tests also passed.

The identical six-second Clarinet plan on the loop candidate retained one
physical voice, with no note end or runtime health errors. Its unwrapped source
traversal reached frame 264,251, beyond the file end; the actual cursor was
89,791, matching the exact wrap modulus for `[22050, 196510)`. Late output
remained periodic near 261 Hz, at -52.82/-55.03 dBFS in the 5.75–6 second
window. This verifies the loop correction on this normal-loader observation.
It remains quiet: source-channel truncation and native timbre/level are separate
unresolved work. No gain, controller default or response policy was changed.

The authored six-channel matrix regression now has four passing controls:
resident and streamed first-two-channel references, ordinary mono/stereo, and
a live program-matrix write on the same held source. The first test run omitted
normal streaming readiness service and failed two controls with underruns; that
run is retained as a fixture failure, not channel-loss evidence. After adding
the existing bounded offline readiness service, all four controls passed without
relaxing the health checks or changing fixture PCM and expected audio.

Against the unchanged stereo implementation, all 22 targeted cases then failed
at their required causal checks: 20 resident/streamed signed matrix PCM
comparisons, the missing keygroup Gain_3_1 control binding, and the corresponding
same-held-note controller write producing silence instead of the independent
stereo reference. These are baseline regressions awaiting the channel fix,
not a passing test suite or a native timbre comparison.

The optional live-owner observer passed its common build, all 28 targeted tests,
15 other lifecycle tests, and affected all-target Clippy. Review corrections
cover identity joins, overlay lookup refusal, inspection work limits and bounded
invalid-query reporting. Three fixture defects were retained and corrected:
test-helper ownership, ordinary render service for marker drain, and the ordinary
getter's two lookups. These tests do not establish actual Clarinet state.
Compiled route linkage is explicitly incomplete; a graph receipt cannot claim
complete routing or native scope admission from these observations.

The first source-bundle foundation passed 15 public admission tests and 13 IR
library tests, with no failures or ignores and unchanged source during execution.
It retains all three helper assets through narrowing and checks geometry,
physical identity, signed matrix cells, and initial preparation estimates.
Every valid new bundle still receives an explicit ExecutionPending refusal
before effect or behavior preparation. This checkpoint has no wide-source
producer or renderer, so these passing admission tests do not resolve the 22
audio/control regressions. The corresponding observer review corrections and
normal Zone constructor fixture now pass the targeted cohort. No actual
Clarinet run using that observer has yet been accepted.

## Findings that are not accepted explanations

- A boolean write to a float parameter is intentionally ignored; global
  coercion is not a sustain fix.
- The two IgnoredInput counts do not demonstrate dropped note/CC messages.
- A synthetic Part SignalConnection warning does not establish a muted
  physical voice.
- Saved layer gain zero and a separate generic gain-reopening regression do
  not establish the cause of this preset's body loss. Actual initialization
  and nested bus ownership must be distinguished from a flat synthetic test.
- Response normalization, gain boosts, and CC default changes have not been
  used to mask missing source channels or loops.

Full numeric receipts are retained under the dev-drive PluginArtifacts
directory `kontra-vwinds-sustain-root-20261016-r8`, including
`resource-channel-windows01`, `loop-boundary-baseline01`,
`loop-baseline-red01`, `loop-baseline-controls01`, `aiff-decoder-only01`,
`loop-candidate-green01`, `loop-boundary-candidate01`,
`multichannel-baseline01`, `multichannel-baseline02`, and
`source-bundle-m1-check01`, and `live-matrix-cohort08`. Protected program
source, XML, encoded audio and decoded PCM were not exported by these
measurements.
