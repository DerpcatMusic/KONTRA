# Kontakt library/format load parity (k-library-load)

Status: **in progress**. The Kontakt 8 decompile is READY
(`/data/dev/workspace/Research/KONTRA/source/kontakt8/`), but this pass has not yet
confirmed any load-path law against it. Everything below is still from repo
evidence, the public Kontakt manual, and focused crate-scoped tests.

## What is already on main

The 2026-10-13 load package (`docs/audit-2026-10-13/kontakt-load.md`) lived on
`feat/kontakt-sampler-support`, not main. Its four production corrections are
present on `origin/main` in equivalent form. Checked by reading the code, and
by re-running the focused tests on this branch (crate-scoped, `-j 4`):

| Correction | Main location | Focused test | Result |
|---|---|---|---|
| Bounded child reads, no duplicate section buffer | `vendor/ni-file/src/kontakt/structured_object.rs:45` | `container_allocations::structured_children_allocate_each_retained_payload_once_and_keep_outer_bounds` | pass |
| NKX/NKR archive identity by directory spelling, moved prefix resolution | `crates/sampler-kontakt/src/samples.rs:159,245,263` | `sample_resolution_allocations` (5 tests) | pass |
| Predefined voice groups without serialized overrides bind the native 1-voice / oldest / prefer-released / 10 ms limit | `crates/sampler-kontakt/src/library.rs:1549-1576` | `library::voice_group_default_tests::omitted_voice_group_override_still_binds_its_native_shared_limit` | pass |
| Snapshot other-files table, per-occurrence resources (`docs/re/KONTAKT_SNAPSHOT_RESOURCES.md`) | `crates/sampler-kontakt/src/snapshot.rs` | `--lib snapshot` (9 pass, 3 opt-in ignored) | pass |

Cherry-picking the three commits from `feat/kontakt-sampler-support` is not
needed: `e2dd5c63` and `d29003e1` are already applied, and `be0a7c5b` is
already in main's `voice_limit` path (main also keeps `exclusive_set: None`).

## Gap table (from the 2026-10-13 ranked evidence, not re-measured)

Reach figures come from the 40-file cohort and the ten non-Conflux representatives.
They are a lower bound, not a corpus census. No corpus run was done here.

| # | Item | Reach / impact (from audit) | Status on main | Owner / blocker |
|---|---|---|---|---|
| 1 | Duplicate child-section allocation | all catalog files | fixed | — |
| 2 | Assigned voice groups without overrides | 38/40 files | fixed | — |
| 3 | Legacy one-band EQ lifecycle | 39/40 files | open | native EQ/FX worker; needs Kontakt 8 READY |
| 4 | Native multiband EQ | 22/40 files, 6/10 reps | open | native EQ/FX worker |
| 5 | Reverb algorithm dispatch | 4 reps | open | reverb law; needs READY |
| 6 | Module-parameter modulation routes | 1/40 files, 3/10 reps (6k diagnostics) | open | destination laws + FX translator |
| 7 | Unsupported FX | 1/40, 3/10 reps | open | FX translator and kernels |
| 8 | LFO waveform admission | 1/40, 2/10 reps | open | exact waveform/sync laws |
| 9 | Convolution controls (early/late, IR size) | 2 reps | open | native IR prep laws |
| 10 | Keyswitch migration recognition | 6/40 files | limitation, not a load failure | KSP callbacks retained |
| 11 | Wavetable source fallback (mode 9) | Morphology rep | open | form/modulation laws; do not guess |
| 12 | Filter selector and compressor mode | Una Corda rep | open | native laws |
| 13 | Archive member case identity | — | fixed | — |
| 14 | Moved NKX/NKR prefix resolution | — | fixed | — |
| — | Retained unparsed tails (40/40 files) | not an audio-failure count | retained | unknown semantics |

Items 3–9 and 11–12 are DSP/format-law work for the engine units. They are not
load-path mechanism bugs. Making them correct needs Kontakt 8 decompiled evidence
(READY) and a measured render comparison, neither of which exists yet.

## Not established here

- No corpus-health run, no per-library load/play check on this branch, and no
  cold/warm timing. The brief forbids local corpus work; the coordinator owns it.
- Full sample-level and native-sound parity for any library.
- Snapshot lifecycle modes 1/3 (script/UI retention) are not implemented in the
  full-read product path (see `docs/re/KONTAKT_SNAPSHOT_RESOURCES.md`).
- Wallpaper/UI resource resolution and saved-state roundtrip were not reviewed
  in this unit.

## Next step when READY exists

Run the coordinator's per-library load survey (`docs/audit-2026-10-13/kontakt-load.md`,
"Coordinator") on the merged build, then rank items 3–9 by measured reach. Each
new law needs its own golden or recorded-vector test before it is admitted.
