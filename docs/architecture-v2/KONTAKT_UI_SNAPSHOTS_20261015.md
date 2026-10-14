# Kontakt snapshot UI observation, 2026-10-15

The [numeric receipt](receipts/kontakt-ui-snapshots-20261015.json) pins optimized
helper SHA `e977ef50f519ad0f8b3a9603ab3584ef504ac178cf91e849a4cbab02a03d0606`.
Each input used one production constructor, Auto streaming, silent plan activation
through an observed listener cycle, then the shared CPU bitmap/vector painters.
There was no plugin/DAW/device, note input, UI gesture, audition or original-host
pixel comparison. Timings were not isolated; they are diagnostic observations.
Private local pictures remain outside the repository.

| Input | Main widgets / bound scalar widgets | Visible before → after | UI effects | Changed widgets |
| --- | ---: | ---: | ---: | ---: |
| Una Corda Felt NKI | 118 / 101 | 7 → 7 | 0 | 0 |
| Analog Gates snapshot | 934 / 648 | 84 → 90 | 11,689 | 439 |
| 9001 Punch snapshot | 934 / 648 | 84 → 84 | 11,958 | 504 |
| Analog Harmonics snapshot | 934 / 648 | 84 → 93 | 12,110 | 494 |

Una Corda's picture shows the piano artwork, three main dials and three tab
icons. Seven visible controls alone is **not evidence of an incomplete UI**.
Its ten menus are hidden on that screen. Opening the tabs and executing their
callbacks remains a necessary interaction check. Initial and observed lifecycle
bitmap hashes match. All six requested resources decode; 57 authored resource
references are retained, so this does not establish every hidden-page asset.

Analog Strings has 20 menus in its main script and six in each of two auxiliary
scripts. All main-script menu values resolve to original item positions in each
snapshot; three menus are visible on the observed main screen. Menu semantic
values are not item positions: the report now preserves current exported scalar
state and structured original item indices/values/visibility separately from
`initial_value`. The renderer already used that current state correctly.
The diagnostic fixture includes nonconsecutive/negative values, a hidden selected
item and an unknown selection that remains unmodified.

All requested bitmap resources decode with zero missing/ambiguous/read/decode
failure. The Analog Gates post-activation picture is populated, including preset
name and waveform art. These facts do not establish tab/menu interaction fidelity.
Each Analog snapshot retains 14 `$CONTROL_PAR_VALUE` projection warnings on the
same button family (source IDs 33431–33444). The runtime effect consumer already
copies valid integer scalar values into the emitted widget. A narrow classification
fix recognizes represented scalar values/domains; malformed text, unsupported
shapes and out-of-domain button values keep their diagnostics. Its authored
fixture drives dynamic UI IDs through the real runtime effect path, checks the
emitted value, and verifies setters do not invoke the target UI callback, as
specified by [NI's control parameter manual](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-parameters).
This removes a false missing-render-consumer report, not a native parity gap.

All four bounded idle lifecycles report no runtime error, capacity drop, cold
start or underrun. Analog streaming reports zero resident heads and 50,156,846,256
full-source bytes; the separate numeric geometry audit is retained for admission
analysis. No note was played, so those idle counters cannot disprove the user's
mute or capacity failures.

The current user-session journal contains six `ui/control_rejected` warnings
with reason `Capacity` (requests 157–162) while 9001 Punch was loaded. They are
rejected operation results, not evidence of a renderer control-count limit.
The historical rows omit operation/capacity class; new bounded reply metadata
is required to identify the prepared resource that was exhausted. The same
session separately recorded runtime capacity drops and cold starts. Do not
infer their common cause from temporal proximity alone.

## Flat-popup pointer correction

A separate authored fixture found a general menu hit/clip defect in the shared
production painter. In a 320×200 viewport a three-visible-row popup occupied
y=128..200 (72 pixels), while its last row occupied y=189.964..220.946
(30.982 pixels). Its center lay below the viewport, and a real pointer click
produced no edit. The row hit area also measured only 32.277 pixels inside a
120-pixel popup, leaving row whitespace unclickable.

Each flat popup row now uses the same `CONTROL * scale` height as its container
calculation and fills the popup width. The focused pointer/pixel fixture passes
at 100% and 200%, with both an unknown current value and a hidden selected item.
It verifies a tab edit, no passive/opening edits, omission of a hidden row without
renumbering, last-row containment and selection through row whitespace. Original
item index 3 submits semantic value 60, never visible position 2 or index 3.
Production CPU pixels change when the popup opens. Authored private fixture PNGs
can be captured with `KONTRA_MENU_FIXTURE_SHOTS`; normal tests write no pictures.
This fixes the shared flat-popup geometry. It does not prove original Kontakt
pixels or the actual library tab/menu callbacks, which require the separate
bounded production lifecycle gesture witness.
