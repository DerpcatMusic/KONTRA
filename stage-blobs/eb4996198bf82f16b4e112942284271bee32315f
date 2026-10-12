# Tester format-reader failures

**READY**, focused validation passed. The envelope prerequisite is validated
Atmoraffe `c1359117`. Local history also contains the earlier equivalent
`79d495db` pick (`ee8b7454`); integrators should take `4fb8bd5f` plus this final
framing/validation commit after `c1359117`, excluding duplicate envelope hunks.
W6's separate reader slice is superseded by W12's consolidated
`f3519531` → `4b8a8293` → `bd485fe5` → `f84a8777`; see
`w12-reader-consolidation.md` for its recorded validation.

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

## Tester-class handoff (2026-10-09)

All four assigned reader classes have authored failing-first coverage in the
already-pushed READY stack `c1359117` + `4fb8bd5f` → `7bc29b21`.
This maps failure classes to shared parser repairs; it does not certify the
exact tester files, which were absent from both installed-library roots.

- (5) Evolve R2, Retro Machines Mk2, Maverick: `tester_parameter_array_v11_keeps_holes_and_opaque_slot_words` and `tester_parameter_array_v11_presence_only_is_bounded_and_inline`; `red.log` / `framing-red.log` → `final-reader-green.log`.
- (6) Chris Hein Ensemble Strings, KFL2 Hurdy Gurdy: `tester_legacy_modulation_nullable_names_keep_target_alignment` and `tester_source_targets_do_not_consume_graphical_shaper_kind_as_boolean`; `red.log` reproduces name length 4294967295, `nested-and-bool-red.log` reproduces boolean byte 2 → both pass in `final-reader-green.log`.
- (7) Ashlight, Pharlight, Straylight: `tester_zone_without_sample_stops_at_native_presence_flag`; `red.log` reproduces cursor Some(48), required 4 bytes → `final-reader-green.log`. The translator guard `tester_sampleless_zone_preserves_native_mapping_without_a_file_reference` also has `translate-red.log` → `final-translate-green.log`.
- (10 in the assignment; TRIAGE table row 8) True Strike 2: `tester_group_nested_ahdsr_v10_keeps_revision_and_physical_slot`; `nested-and-bool-red.log` reproduces expected 17 / got 16 → `final-reader-green.log`, with envelope prerequisite `c1359117`.

The later extras READY `bd485fe5` keeps invalid booleans rejected with field
context; it is not a second fix for the graphical-shaper alignment case.
No additional reader mechanism is uncovered by these recorded signatures.
A tester retry of these exact programs on the integrated stack is still needed
to establish that their native records use the repaired layouts.

NEXT: W0 integrates the READY stack; exact tester-file retry remains open.
