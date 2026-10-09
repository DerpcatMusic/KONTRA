# W12 format, family and slot recovery

Shipping source reviewed: `f199980d` (393). The private 383 batch is a separate
source candidate. This recovery uses existing receipts; it does not relabel
the old native census as a current shipping run.

The complete native Kontakt slot inventory has **834 items** on `5172a9ef`.
The remaining **660 UVI items are UNKNOWN** at the reader boundary; all 26
bank files exist. Its enabled/bypassed losses are FX slots 459/1037, filters
1799/545, modulator sources 158209/2685, and target routes 222757/18054.
These are separate units. `NativeLawUnverified` denotes approximation, not
a dropped slot or a native-audio pass.

Rank by audible impact and established advantage over v1:

| Priority | Recorded loss / evidence | v1 comparison and next action |
|---|---|---|
| Whole-item admission | 660 UVI inventories UNKNOWN; supplied legacy XML NKI 1/784 | Reader failure is not proven missing data. UVI belongs to W10; v1 also lacks XML-to-IR. Neither is a verified v1 port win in this receipt. |
| Sounding modulation / source behavior | Conflux intensity 64974 and frequency 37128 enabled target losses; RandomBipolar 28050 and LFO6 9282 enabled source losses | Largest historical sounding queue. V1 retains module assignments but does not execute all these laws; `src/modulation.rs` explicitly refuses externally driven LFOs. Do not claim v1 playback support from retained metadata. W6/W15 own the laws. |
| Sounding filters / FX | Filter30 876/17, Filter3 556/0, Filter106 267/16; Surround Panner 174/471 | Historical revision/kernel admission losses. Recount W15's newer READY blocks before calling these current defects. |
| Verified v1 resource-resolution mechanism | Convolution ResourceUnavailable 13/7 across Areia, Conflux and Una Corda | `0cb7a8a0:src/import.rs:918` maps an authored Resources suffix into the special-file-table NKR. Shipping v2 discards that table and only performs sample lookup. W12 owns the failing-first resolver port; actual recovered slot count requires affected-item measurement. |
| Family / cursor evidence | Cluster Risers 3 historical MATCH cells, 63 SCRIPT_DRIVEN UNKNOWN; priority Una Corda/Conflux 104 scripted programs UNKNOWN | RR is evaluated by support/distribution, never exact hit order. Cluster Risers is separate from Trills, whose held-key audition requires independent capture. Script group masks and scripted offsets do not prove executed samples/cursors. |
| Bypassed slots / unassigned format fields | Counts above; NIS controller-assignment properties and filename tails remain semantically unverified | Bypassed drops still prevent enabling native features. Nonzero bytes or object presence are not activation counts. Existing program-private script automation must not be mislabeled absent because NIS properties are opaque. |

The v1 resource mechanism is the first confirmed v1 advantage in W12's reader
scope. The larger DSP queues remain ahead by theoretical impact, with no
invented v1 superiority or new current totals.

Comparable historical **22-item** admission measurements are available:

| Dropped slots | Original enabled/bypassed | Last measured enabled/bypassed |
|---|---|---|
| FX | 377/1521 | 100/1315 |
| Filter | 2109/4962 | 1818/4770 |
| Modulator | 20404/24478 | 14960/24478 |

Last measured source is `5172a9ef`: four affected items/five programs were
remeasured, 18 unchanged receipts retained, authored identities/states matched,
and no new drops appeared. Recovered admission remains approximated. This is
not an eight-hour before/current comparison: no matched native-slot endpoint
on `f199980d` survives in W12's receipts. No current coverage percentage or
eight-hour improvement is certified. EQ's cached 18818 gain-route candidates
are projections, not measurements, and are excluded from the table.

Vista/Pacific **56/56, zero translated sample/IR failures** remains established
on `54d9a5c5`. Corrected taxonomy earns no new resolution or playback gain.
The four tester reader classes and compact snapshot fixtures already have
READY RED/GREEN receipts; exact absent tester files remain unverified.

Sources: `w12-fidelity/{grouped-corpus-native.json,corpus-42062adc-provenance.json,
target-summary.json,target-summary-after-42062adc.json,native-family-metrics.json,
cached-c49-eq-summary.json}`, `w12-family-22ed/SOURCE_LEAD_DISPOSITION.json`,
`w12-format-revisions/legacy-header-census.json`, and
`w12-vista-pacific-resources/RESOURCE_CENSUS.json` under the run ledger.

NEXT: authored NKR impulse lookup RED, v1 resolver port, affected-item recount.
