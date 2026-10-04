# Augmented Orchestra program census

All 620 declared programs decoded and parsed successfully at source revision `384ed219784be46e69b546ac77a494c456896bb0`. Current static preflight admitted **0/620**; parsing does not establish playability. This is a functional program-format and static-admission census, not a playback or audio-parity test.

The owned catalog still contains 660 declarations: 620 Augmented Orchestra and 40 VWinds. The external free Starter bank adds 50 declarations, giving 710 across these datasets. The historical 98-program decode frontier was 8 Augmented, 40 VWinds and 50 Starter. This census newly checks the other 612 Augmented declarations, then rechecks the original eight with the same frozen source. The resulting historical unique decode coverage is 710; only the 620 Augmented programs have same-revision static-preflight coverage in this receipt. The historical 41 static admissions elsewhere were not reevaluated here and are not a count of playable programs.

## Static rejection distribution

A program can contain several rejected nodes and rejection kinds. The census recorded 487,576 issues across 60,160,934 parsed nodes and 47,304,523 serialized connections. Counts below are programs containing that rejection kind and total issues respectively.

| Rejection kind | Programs | Issues |
| --- | ---: | ---: |
| CombFilter | 620 | 241,180 |
| MS20 | 620 | 240,560 |
| Flanger | 620 | 2,480 |
| ControlGraph | 620 | 620 |
| Drive | 620 | 620 |
| DiodeClipper | 620 | 620 |
| FeedbackMachine | 620 | 620 |
| MultiLFO | 620 | 620 |
| SparkVerb | 143 | 143 |
| Layer | 53 | 112 |
| SampledReverb | 1 | 1 |

There are 21 distinct current-source diagnostic strings. The ControlGraph issue records the first graph-construction failure, so its causes are not an exhaustive inventory of every unsupported source setting. These first causes partition the 620 programs: LFO `Smooth` 364, LFO `WaveFormType` 11, StepEnvelope `Smooth` 230, `Retrigger` 11, `Bipolar` 2 and `SyncToHost` 2. Other explicitly named causes are Layer `PlayMode` in 53 programs, SparkVerb `RoomSize` near an uncertain prime-delay boundary in 143, and invalid SampledReverb `Time` in one.

Four bounded representative LFO inspections found serialized `Smooth=5.2776863e-09` with `WaveFormType=0`. The existing gate treats that finite nonzero value as unsupported deterministic smoothing. No epsilon, default substitution or admission change was introduced. This is a concrete next native-control behavior to investigate.

## Subsequent source attribution, verification pending

Later source preserves the same scalar gate order and error wording while
retaining a private typed identity for known LFO and StepEnvelope setting
failures. Preflight attaches those failures to the actual source node in the
existing three-field rejection record. The node-local report and Mapping index
can therefore identify the source instead of assigning it to the root
`ControlGraph`. Unknown or malformed construction errors retain the root fallback.

Index, kind, parameter and finite scalar checks validate the error's shape;
the immediate preflight invocation supplies its Program ownership. This is not
a cross-program fingerprint or an admission change. Compilation and the prepared
source, preflight, report and Mapping cases are unexecuted under the CPU
restriction. The census counts above belong to their original completed run;
no new corpus run or newly playable program is claimed.

The private receipt also classifies 43 route categories associated with rejected nodes. Repeated CombFilter/MS20 `Freq` and `Q` routes include StepEnvelope, LFO, MultiLFO, MIDI CC and voice velocity sources, generally Mode 0 with no mapper in these categories. Association with a rejected owner is not independent evidence that every route is unsupported. Representative program identities, scalar context and individual issue digests remain in private receipts; no vendor XML, scripts, curves or bank payloads are included in this document.

## Method and limits

The harness compiled the frozen revision's UVI library, parser and preflight leaves directly, with cached `191e481` shared engine/import/audio/fx ABI glue and the already locked dependencies. A visibility bridge copied the exact current `diagnostics::script_excerpt` body; program decode and static preflight did not execute it. This was not a full application build or an installed-binary census.

Each program was loaded separately through the supported reader namespaces and existing local access state, then dropped after aggregation. Encoded members and decoded program XML retained their 32 MiB limits, and the parser retained its 250,000-node bound. The largest encoded program was 506,318 bytes and the largest graph had 97,048 nodes. Combined encoded program member sizes were 293,849,536 bytes; that is requested member content, not total filesystem traffic. The two functional passes took about 287 seconds; this is not a controlled performance claim.

No sample residency preparation, sample decoding, rendering, script execution or official-reader launch occurred. No activation, account or system changes were made, and no ownership/license check was performed. Successful decoding is evidence for this dataset and supported access path only; it makes no universal protection or format-coverage claim. Every static playback gate remained unchanged.
