# Input admission and reset attribution

These are isolated stage measurements, not KONTRA's whole-plugin meter or proof
of the requested 300-voice / 1% target. The optimized `corpus` test executable
uses the existing compiler/quality rules, CPU 5, four alternating pairs. The
final combined window recorded 53 compiler-process observations with none
present. The host and OS background work remained uncontrolled.

## Input ownership lookup

The previous duplicate check scanned every allocated note slot. Input identity
includes protocol, port, group, channel, key, external ID and performance domain.
A released note still owns that identity until terminal retirement is accepted;
checking only keys currently down would change behavior.

The retained implementation reuses the existing free-slot bitmap while the arena
is at most half full, scans contiguous slots above that threshold, and returns
immediately for an empty arena. It adds no identity registry, retirement hooks,
allocation, locks or mutable bookkeeping. Padding bits are masked and quarantined
empty slots cannot match. The fixture compares against the original scan across
holes, retained terminals, reused generations, domains and all tuple fields,
including a non-word-aligned capacity and the dense fallback.

Median microseconds per query, 4,096 allocated slots, 25% hits where nonempty:

| Live owners | Original | Retained adaptive lookup |
| --- | ---: | ---: |
| 0 | 1.752 | 0.0028 |
| 32 | 1.374 | 0.0608 |
| 300 | 1.483 | 0.3414 |
| 512 | 1.597 | 0.5263 |
| 1,024 | 1.814 | 0.9827 |
| 2,048 | 2.409 | 2.0168 |
| 4,096 | 3.655 | 3.8133 |

At 300 owners the measured lookup stage falls 77.0%. At a full arena the extra
dispatch costs approximately 4.3% in this receipt; this is not a universal win.
Walking linked live notes was rejected: it was about 2.82 times slower when full.
Unconditionally traversing the bitmap was also rejected: 28.3% slower when full.
The density fallback avoids those larger regressions. Initial and externally
contended receipts remain preserved separately, not averaged into the final run.

## Reset footprint: prototype only

`ProcessorState` occupies 3,216 bytes. A four-stage chain across 300 voice slots
therefore holds 3,859,200 bytes. The isolated prototype resets common histories
for every incoming stage and full color histories only for Color/Filter stages;
another candidate resets only the incoming chain length instead of the maximum
stride of other chains in the plan. Production reset behavior is unchanged.

For three native EQ stages plus Stereo, four-stage stride, the median of 34
repetitions is 686 us for the original versus 190 us for the owned-history
prototype. That is **20.18 versus 5.59 us per 300-voice reset**, not a 496 us
callback saving. With a 16-stage maximum stride and the same four-stage incoming
chain, the 16-repetition medians are 1,242 / 391.5 / 99 us for full-stride,
incoming-count and owned-history candidates. Initialization is consequently not
the principal explanation for the previously measured approximately 1.9 ms
admission event stage. Any production change still needs chain-reuse PCM and
state-ownership validation; no reset shortcut is promoted from this microbenchmark.

Reproduce with the ignored `input_lookup_stage_evidence` and
`voice_reset_stage_evidence` core tests in an optimized build. Frozen executable,
raw four-way lookup / three-way reset receipts and compiler monitor:
`PluginArtifacts/kontra-cpu-20261014/admission-reset-final-*` on the dev drive.
