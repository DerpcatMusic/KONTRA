# Tester format-reader failures

**READY**, focused validation passed. The envelope prerequisite is validated
Atmoraffe `c1359117`. Local history also contains the earlier equivalent
`79d495db` pick (`ee8b7454`); integrators should take `4fb8bd5f` plus this final
framing/validation commit after `c1359117`, excluding duplicate envelope hunks.
W6's separate compact-snapshot slice remains preserved on
`v2/fix-tester-reader-versions`; this receipt does not claim its validation.

| Tester class | Shared cause and change | Failing-first evidence |
| --- | --- | --- |
| Parameter array 0x11 | Two native v0x11 contexts: presence-only, or presence plus opaque u32 per slot. Bounded records must fit exactly one complete layout; preserve revision and raw words on chunk writes. | Unsupported v11 RED; presence-only regression RED; both layouts GREEN across FX/internal/external arrays |
| Modulation name/boolean | Legacy v0x100/v0x80 nullable C-string names use -1; modern names reject it. Six native source targets omit the module-slot byte. | Name length 4294967295 RED→GREEN; graphical shaper kind 2 misread as boolean RED→GREEN |
| Zone cursor 48 | v0x9a..9c prefix is 42 common bytes, two flags, opaque u32. Flag at byte43 gates the sample suffix. Preserve absent zones in native metadata, skip PCM resolution. | Exact cursor48/required4 RED→GREEN; translator rejects absent zero-tune zone RED→GREEN |
| Group expected17/got16 | The only Kontakt-object VersionMismatch call in shipped199 is nested AHDSR. It is not a Group header revision. | Group slot3 nested AHDSR v0x10 reproduces expected17/got16 RED→GREEN |

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
contracts, not new DSP implementations or native-host audio parity. Inline FX
arrays lack serializer context and read the presence-only v0x11 form; the
extra-word form is admitted only when its complete chunk boundary is available.
No arbitrary byte recovery or permissive boolean parsing is used.

Both user library folders were searched for the named Evolve R2, Retro Machines,
Maverick, Chris Hein, KFL2, Ashlight, Pharlight, Straylight and TrueStrike items;
none is installed locally. Thus those exact tester files remain unverified.
Cached 834 Kontakt census items previously parsed successfully; do not relabel
them as a new post-fix corpus run. No Wine/native host was launched.

Receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w12-format-revisions/`:
`red.log` (three exact failures), `green.log` (3/3),
`nested-and-bool-red.log` (two exact failures), `translate-red.log` (absent
sample zone semantic rejection). Final checks: `final-reader-green.log` (7 format + 33 compatibility PASS),
`final-translate-green.log` (1 PASS), `final-object-green.log` (4 PASS),
`final-root-no-run.log` (plugin/shots root compile PASS). The additional
presence-only regression has its own `framing-red.log`. Two fixture/compile
setup failures are retained separately; neither is counted as behavioral RED.
The metadata probe privacy guard caught authored group names in existing IR
locations before emission; the successful probe emits only numeric indices.

NEXT: direct handoff to W6; W0 integrates the format slice after c1359117.
