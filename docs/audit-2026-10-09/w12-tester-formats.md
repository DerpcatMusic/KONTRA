# Tester format-reader failures

Source snapshot **HOLD**: final expanded fixture GREEN and root no-run remain
pending the direct machine handoff from W9. Do not merge this snapshot as READY.
The envelope prerequisite is the validated Atmoraffe `c1359117`; local history
also contains the earlier equivalent `79d495db` pick (`ee8b7454`). Integrators
should take only this branch's new format commit after that prerequisite.

| Tester class | Shared cause and change | Failing-first evidence |
| --- | --- | --- |
| Parameter array 0x11 | A presence byte and opaque u32 precede each slot, including holes. Keep revision and raw words on chunk writes. | Unsupported v11, then GREEN across FX/internal/external arrays |
| Modulation name/boolean | Legacy v0x100/v0x80 nullable C-string names use -1; modern names reject it. Six native source targets omit the module-slot byte. | Name length 4294967295 RED→GREEN; graphical shaper kind 2 misread as boolean RED, final GREEN pending |
| Zone cursor 48 | v0x9a..9c prefix is 42 common bytes, two flags, opaque u32. Flag at byte43 gates the sample suffix. Preserve absent zones in native metadata, skip PCM resolution. | Exact cursor48/required4 RED→GREEN; translator rejects absent zero-tune zone RED, final GREEN pending |
| Group expected17/got16 | The only Kontakt-object VersionMismatch call in shipped199 is nested AHDSR. It is not a Group header revision. | Group slot3 nested AHDSR v0x10 reproduces expected17/got16; final GREEN pending |

All fixtures are authored; no protected library bytes, sample bodies, script
text, or keys are persisted. Unknown array/zone revisions, truncated prefixes,
invalid presence/invert flags, and absent sample suffixes with presence set
remain errors. Sample-less zones keep their physical source-index hole and
original native mapping; zero placeholder sample metadata never resolves PCM.

Native layout evidence is read-only under
`t3code-80fe786b/artifacts/engine-analysis-2026-10-07/kontakt-engine/`:
`complete-export/pseudocode.c` has the array readers at lines3759380,
3759521,3759584; modulation dispatch/nullable reader at4724054,4724232,
3201496; zone reader/writer at4725944/4736367. Source target registry
initializer3803315 and predicates3733672 establish no module slot for native
IDs17/18/19/21/177/244 (`formantShift`, `overlap`, `grainSize`, `grainSpeed`,
`playDirection`, `legacyAddIntensity`). Names were checked against existing
static `strings-ascii.txt` through `pe.json` section offsets. These are wire
contracts, not new DSP implementations or native-host audio parity.

Both user library folders were searched for the named Evolve R2, Retro Machines,
Maverick, Chris Hein, KFL2, Ashlight, Pharlight, Straylight and TrueStrike items;
none is installed locally. Thus those exact tester files remain unverified.
Cached 834 Kontakt census items previously parsed successfully; do not relabel
them as a new post-fix corpus run. No Wine/native host was launched.

Receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w12-format-revisions/`:
`red.log` (three exact failures), `green.log` (3/3),
`nested-and-bool-red.log` (two exact failures), `translate-red.log` (absent
sample zone semantic rejection). Final checks will add new receipts here.

NEXT: final focused GREEN + root no-run after W9; then direct handoff to W6.
