# Embedded KSP semantic fault attribution

W5's RAM-only initializer probe reproduced W3's 45 baseline faults across 30
program-0 multi items. All 45 phase/line/offset triples exactly match
`~/.cache/kontakto-w3/native-caption/residual-ksp-faults.json`.

| Rank | Fixed public reason | Command | Faulted slots | Arguments |
|---|---|---|---:|---:|
| 1 | unknown-command | `subscribe_async` | 30 | 3 |
| 2 | unknown-command | `mf_get_first` | 15 | 1 |
| 3 | No additional kind observed | — | 0 | — |

These are compiler faults in embedded programs. The 45 slots overlap 30 items;
this receipt does not claim 45 broken multis or a new Native-paint regression.
W3's prior receipt deliberately retained only `sema/stage-error` and numeric
locations; command attribution here comes from the actual current error in RAM,
not an inference from those locations. The historical KSP audit independently
reported the same two command counts.

The feature-gated `sampler-kontakt` example `ksp_fault_kinds` accepts an owned
four-column manifest (item hash, path, program index, script slot). It decodes
once per program, evaluates selected slots, and emits only fixed public reason
categories, catalog command names and numeric metadata. Private identifiers,
diagnostic messages, authored source and decoder stderr are not exported. Its
self-checks cover private-name withholding and nested/quoted argument commas.
The environment is intentionally empty except for slot: this is a semantic
fault attribution probe, not a production initialization or playback verdict.

Numeric receipt: `~/.cache/kontakto-w5/ksp-coverage/fault-kinds-before.json`.
Frozen initial probe SHA256: `9a0e9c44dd1ae0f52945484f745e332f888f2ea9be67ff3cd7ad5536e83021fb`.

No runtime/compiler semantic fix is claimed by this diagnostic change. W11 owns
persistence callback Control-context admission separately. The CPU item stays
parked; Conflux/256's approximately +5 µs p50 disclosure remains open.

NEXT: establish `subscribe_async`'s three-argument contract and add a failing-first
native regression, then address the MIDI-object cursor family.
