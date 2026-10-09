# W15 piano release and Cutoff readback — 2026-10-09

Branch: `v2/w15-eq-controls`. Production candidates: `46bfa562`
(computed event getters), then `e70d6053` (legacy `%EVENT_PAR` current-event
reads and writes), both ports from v1 `0cb7a8a0`. Their dependencies include
`c49a329e` (generated-note scheduling). No instrument-specific attenuation.

## Una Corda Cotton

The saved MAIN release callback creates a 1 ms dispatch note with custom tag
`3`. The next script selects on `%EVENT_PAR[0]`. Before the fix, that system
array did not read the current event's custom parameters, so the tag-3 branch
was skipped and dry group 39 restarted. Group 39 is not a native release group.
After the fix, the selector reads `3`, the authored branch executes, and the
dispatch note reaches playback with group 39 disabled. The later script creates
the intended release event with group 39 disabled and event volume −5840 mdB.

Failing-first fixtures cover computed selectors and legacy-array release
routing, shared read/write storage, and invalid indices. The resumed targeted
selection run passes 5 tests. The saved-script routing witness uses synthetic
PCM, passes its no-heap assertion, and reports zero nonfinite frames/preemptions.

Frozen-host comparison, MIDI key 60/velocity 100, 500 ms hold, four repetitions:

| Signal metric, settled repetitions | Frozen 306/326/344 | Candidate e70d6053 |
| --- | ---: | ---: |
| First 20 ms RMS | −22.7619 dB | −22.7559 dB |
| Held 500 ms RMS | −27.9641 dB | −27.9616 dB |
| Post-off 500 ms RMS | −33.3934 dB | −47.8048 dB |
| Attack peak position | 18.6042 ms | 18.6042 ms |

The release falls 14.4113 dB. Historical Kontakt post-off RMS is approximately
−48.2 to −49.2 dB; it is not a matched-sample null. Frozen v1 proxy post-off RMS
is −46.4943 dB and its attack peak is about 6 dB hotter. No attack regression
was observed between the three available frozen v2 builds. Noire and the exact
reported older build are unavailable locally; their attack complaint remains
open. Signal measurements exclude CPU/timing acceptance. Candidate is a debug
build, so no CPU claim is made.

Numeric receipts live outside Git in
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w15-piano-p1/`:
`event-array-ab-summary.json`, `event-array-candidate/binary.json`,
`release-resume-gate.log`, `resume-selection-green.log`, and
`resume-area-no-run.log`. Candidate plugin SHA256:
`5787f581497cc379eae4f9318e8c083154c7cca14dc8acfe2f682c3be6734d97`.
Authored source and sample PCM are not persisted by the routing probe.

## ModulationRoute Cutoff

The tester bundle records the Ethereal Earth rejection on old 0.3.199:
`chain 13, index 0, Cutoff`. The normalized native Ladder/Daft route support
from `b76e6d52` is already an ancestor of current integration. The regression
guard `09ad8787` checks both processors, pre/post amplitude positions, expanded
filters, dry/wet wrappers, and nonzero chain indices. It passes: normalized
depth is sample-exact with changing the saved knob before conversion, opening
the cutoff increases a 2 kHz tone by more than 3 dB, and rendering allocates
nothing. No additional production rewrite is justified by this old report.
Ethereal Earth itself is absent locally; its exact preset load is unverified.
