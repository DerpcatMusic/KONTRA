# VWinds V2 performance evidence

Nine bounded performance tapes completed on the actual Clarinet A V2 preset at source revision `3c8e5ae`. The runs reached authored controller and transition paths, produced commands and parameter changes, and rendered finite local PCM without runtime errors. This establishes more than successful sample loading. It does **not** establish native modeled-transition fidelity, an identical pitch trajectory, perceptual equivalence, or native PCM parity.

MPE setup failed before its performance tape began. The later unowned-context diagnostic correction at `483326a` is uncompiled and has not been tested or replayed against the bank. No shared core ownership integration is established.

## Source and method

- Instrument: VWinds A Clarinet V2, Clarinet A preset, from an authorized locally held bank. No bank assets or vendor script source are included here.
- Performance cases were informed by the 26-page [official VWinds Clarinets V2 user guide](https://www.acousticsamples.net/index.php?product_id=112&route=product/productmanual), consulted during the audit. The guide describes airflow control, overlap legato, velocity-controlled glide, advanced pitch bend, vibrato modes, and MPE.
- One Program, approved modules and decoded resource cache were prepared for the run. One retained hosted Player processed all nine successful tapes and the attempted MPE setup. A fresh Player was created once for the subsequent last-good-state restore.
- Audio ran at 48 kHz in 256-frame intervals. Tapes were bounded and unpaced; this is not a realtime CPU, latency, underrun, DAW, or end-to-end Worker/core integration proof.
- The private harness added read-only scalar/command observers, callback-entry logging wrappers and a deterministic seed. Wrappers retained authored return behavior. These were instrumented local runs, not an unchanged native-reader runtime.
- Performance settings used the actual initialized, visible/enabled control snapshot and admitted Player UI-edit path. Frontend pointer and keyboard gestures are outside this evidence set.
- Command counts and callback observations below are local to their tapes. Saved runtime node-dispatch counters were cumulative across the retained Player and cannot establish which node first processed in a particular tape. Retained voices can be silent or releasing.

The official reader used elsewhere in the investigation was UVI Workstation 4.0.9. No reference PCM from that reader was captured for these performance tapes.

## Behavior matrix

All nine completed tapes were parsed and admitted, reached the listed authored gesture/control path, and completed their requested frames. Their commands were consumed by the local renderer, PCM remained finite, and no observer logs were dropped. Exact native comparison is unverified in every row.

| Case | Frames | Control/transition path reached | Commands and output evidence | Remaining fidelity question |
|---|---:|---|---|---|
| Airflow | 73,728 | CC1 stepped through 0, 64, 127 and back to 0 during a held note; authored smoothing ran | 2,541 controller-all commands, 3 starts, 2 fades; rendered window energy varied across the CC steps | Native smoothing/timbre law. Zero airflow was not proven exactly silent: attacks, noise and tails remain |
| Overlap legato | 49,152 | Overlapping notes 61 and 64 reached the fast transition branch | 6 starts and 5 fades with hosted root attribution; finite output | Exact transition timing, pitch and timbre |
| Low velocity outside interval limit | 49,152 | Target velocity 1 with a three-semitone interval and default maximum interval 1 reached the fast branch | 5 starts and 5 fades; finite output | This case does not prove slow glide; it records the authored interval fallback |
| Low velocity within interval limit | 147,456 | Maximum interval changed to 6; target velocity 1, threshold 20 and a three-semitone overlap reached the slow branch | 7 starts and 13 fades; sustained transition rendered finite output | Native glide curve and acoustic transition |
| Advanced pitch bend | 49,152 | Advanced mode selected; held-note bends +0.7, -0.7 and 0 reached three authored callbacks | 9 bend commands, 4 starts and 4 fades; 28 scalar parameter deltas including harmonic-frequency values | Native pitch response and modeled transition fidelity |
| Vibrato Auto | 98,304 | Auto selected; held note and CC11 changes exercised the automatic path | 3,327 controller-all commands, 4 starts and 4 fades; finite output | Native vibrato waveform, depth and rate |
| Vibrato AutoTime | 98,304 | AutoTime selected using the same bounded held-note/controller pattern | 3,363 controller-all commands; controller envelopes differed from Auto; finite output | Native time-dependent vibrato law |
| Vibrato Manual | 98,304 | Manual selected; CC11 changes reached the manual-control path | 1,392 controller-all commands; manual updates differed from automatic modes; finite output | Native manual-control response |
| Sustain and release | 49,152 | Pedal down, note release, then pedal up reached controller and release paths | 6 controller commands, 690 controller-all commands, 4 starts and 4 fades; finite output | Authored release fades occurred before pedal-up. This does not prove ordinary sustain holds the wind voice, or native pedal/release equivalence |

This matrix records functional control and command execution. Finite PCM and command activity alone do not validate every processor's numerical law. No spectral pitch-trajectory comparison, reference transition alignment, or listening comparison was completed.

## MPE failure and ownership gap

The visible, enabled MPE selector reached its authored callback. That callback attempted to change the parent Part's MIDI selection. The first recorded runtime failure was `Invalid UVI parameter target`. No MPE note/bend/pressure tape ran, so there is no MPE output or expression-fidelity result.

Static tracing found that the preset graph contains the Program's nodes, while the Lua host separately creates parent Part and Synth context objects. Their writes were emitted as ordinary graph-node parameter commands. The renderer owns only preset nodes, so a parent-context target falls outside its graph. This is a host ownership gap, not evidence that the authored MPE model ran incorrectly.

Concrete host ownership also matters:

- Rack MIDI channel/port controls filter attacks before they reach the native Player. Storing an Omni value inside Lua cannot recover attacks already filtered by ingress. The current rack port selector does not represent the parent's all-input value.
- Rack gain is persisted in dB and converted for postmix; gain/pan/mute are applied after the UVI audio buffer. A native parent parameter needs a measured conversion and one actual consumer, not a direct copy into an unrelated field.
- No concrete global Synth owner or mapping has been established. A write cannot safely target arbitrary output buses or other rack parts.
- The current hosted adapter separately rejects MPE attacks and most MPE controllers. Correct parent targeting alone would not establish MPE admission or per-member ownership.

The source-only correction at `483326a` explicitly diagnoses an unowned Part/Synth write before that write changes host parameter state or emits a graph command. It preserves graph validation and does not provide a parent owner, routing transport or MPE implementation. Earlier callback/widget effects are not rolled back. Because this correction is uncompiled, its behavior is supported by static review only, not by these earlier runs.

A future real owner must be bound to the adopted rack destination and part generation as well as the worker activation epoch/generation. MIDI changes need ordered ingress application before later attacks, preserving original physical note tuples for releases. Current fixed PCM packets establish no external-context write payload. No shared core integration for this contract was implemented or validated by this audit.

## Saved-state evidence

The last successful state, captured before the failed MPE setup, restored the following eight recorded control values exactly in one fresh Player using the same prepared Program/modules/cache:

| Control | Restored value |
|---|---|
| Vibrato mode | Manual |
| Glide mode | 1 |
| Maximum glide interval | 6 |
| Glide velocity threshold | 20 |
| Advanced pitch bend | Enabled |
| MPE | Disabled |
| Vibrato controller | CC11 |
| Airflow controller | CC1 |

Construction succeeded and a subsequent 256-frame render produced finite PCM. This verifies those controls and the last-good local state path. It does not prove restoration of active voices, transition history, an MPE-enabled state, or parent Part/Synth context.

The current state format retains script/widget state and original-node parameter/resource deltas. It intentionally excludes the synthetic parent context. The diagnostic correction changes no saved-state schema; a future real parent owner needs explicit persistence and restoration before authored callbacks and input, without treating parent identities as preset-node IDs.

## Provenance limits

The evidence comes from private actual-file receipts created before the CPU-stop instruction. This public document contains only reviewed behavior summaries, counts, public control values, source revisions and a public manual link. It contains no bank payload, vendor code, account material, keys or machine-specific paths.

No builds, checks, tests, replay, probes or metadata decoding were performed to prepare this document. The nine tapes and eight-control restore describe instrumented local Player behavior at `3c8e5ae`; they do not validate later source changes, frontend gestures, shared core integration, MPE, realtime performance or native audio fidelity.
