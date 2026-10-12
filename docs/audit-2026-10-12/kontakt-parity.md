# Kontakt and KSP parity audit — 2026-10-12

Audited base: `11ca186d`, on `feat/kontakt-falcon-parity`. The v1 source
reference is committed `0cb7a8a0`, not the large uncommitted v1 checkout.
Other worktrees and installed libraries were read only. Broad first-party
tests and the full installed-library scanner belong to the coordinator;
this lane owns the focused checks below.

## What appears unmerged but is already integrated

Git ancestry alone misclassifies the historical Kontakt branches because
the release commits squashed their work. Re-merging those branch histories
would reintroduce obsolete implementations and unrelated integration work.

- `fix/kontakt-fx-parity` (`fe10c5db`): measured EQ law, ladder LP selectors,
  AR admission, Classic Saturation, native Distortion and initialization
  parameter writes are present in the audited base. Their production tests
  remain in `sampler-kontakt/src/effects.rs` and `sampler-core/src/dsp/`.
- Historical `gpt/kontakt-fx-2`, through `11981562`: Morphology weighted
  selectors, Formant's parallel bandpasses, the Inverter assignment,
  native filter/legacy Phaser, Enhanced compressor, convolution Auto Gain,
  static instrument bus pan and physical-slot live drive controls are
  present. The current core Phaser, Formant and Enhanced-compressor files
  are byte-identical to that historical tip. The current Kontakt effects
  file differs from it only in the corrected Saturation module label and
  its assertion. Round-two research-only notes are superseded by the
  [round-three implementation receipt](../audit-2026-10-11/kontakt-fx-3-handoff.md),
  which is itself retained on main.
- `wp456-kontakt-parity` (`93916d2d`): saved script-switch automation is
  present, including `automation_tags_name_sliders_and_switches`. Its
  older shared-base history also contains code subsequently integrated or
  corrected on main; it is not a list of missing patches.
- Dirty `v2/w12-vista-pacific-resources` (`68052b42`) proposes independent
  NICNT directory indexing and bounded member reads. Main already carries
  that behavior as `NIFileContainer::read_table` (table plus completeness
  flag), used by `ResourceContainer::open`, with checked EOF/limit handling
  in `read_item`. The newer exact-item path also handles duplicate stored
  IDs correctly. The existing resource test distinguishes an absent name
  from a listed unavailable member on a NICNT stub. Copying the older
  `read_index`/`read_member` patch would duplicate and weaken this path.
  Its handoff says the missing Vista/Pacific `pic` and `admin_bg` requests
  were absent in supplied data, including frozen v1. That historical result
  is not an image-loading improvement or a new corpus measurement.

These are implementation-presence checks. Their vector tests do not become
whole-library or calibrated Kontakt host parity claims.

## Uncommitted Kontakt-related material

The worktree inventory at `/tmp/kontra-parity-audit-20261012/worktrees.json`
identifies two relevant leftovers:

- `v2/w12-dsp-recount`: a 16-line uncommitted ignored test in
  `sampler-kontakt/src/coverage.rs` prints numeric group-208/filter-33
  metadata for an externally supplied instrument. It changes no production
  translation or audio behavior. Leave it as historical probe evidence;
  there is no missing runtime fix to merge.
- `v2/ksp-frontend`: untracked `examples/ksp_v1_baseline.rs` is a v1 audit
  example. It does not provide a missing v2 runtime implementation.
- `v2/fix-w12-gate`: uncommitted `native_always_keeps_its_compound_operator`
  assumes a mode-0 row before a key condition should act as boolean true
  with its AND_NOT join. This expected behavior has no retained native
  evaluator proof. The [NI Group Start Options manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view)
  instead defines Always as a no-operation end/deletion marker, and
  excludes a trailing Always from a multi-row condition list. The
  [Kontakt API group-start condition inventory](https://docs.native-instruments.com/ni-tech-manuals/kontakt-api-reference-manual/en/group)
  does not include Always among predicates. Therefore do not port that
  fixture as a confirmed failing contract or add an Always-true IR/DSP
  law to satisfy it. Current skipping may be correct for legitimate lists;
  malformed interior mode-0 rows remain unverified. This is an inference
  from the documented marker semantics, not a measured native trace.

The named `fix/kontakt-fx-parity`, `wp456-kontakt-parity`, audit Kontakt/KSP,
and retained KSP follow-up worktrees are clean in this inventory. They
should be archived only after their content is classified, without
discarding the unrelated large dirty v1 checkout.

## Two concrete regressions addressed

**Warm-cache folder lookup.** Cold Kontakt initialization provisions library
and patch folders through `Environment.folders`. `CachedInit` omitted that
field, and `restore_initialized` reconstructed it as empty strings. The
product-cache path directly reused that restored initializer, so later
`get_folder` callbacks behaved differently after a warm load. Preserve all
four folder entries in the existing cache schema, capture and restore.
The Kontakt importer hash includes `sampler-ksp/src`, so this source change
invalidates existing product entries automatically. No DSP or script law
changes are needed. The regression executes literal and computed selectors
after cold initialization and serialized cache restoration, including an
unknown selector that must still produce empty text.

**Installed tests silently skipped real libraries.** The player's configured
root is `/mnt/MAIN_STORAGE/Libraries`, containing `Kontakt` and `UVI`.
The old helper appended Kontakt library names directly to that umbrella,
so individual tests returned early. Its first-per-library test appended
`Instruments` to `Libraries/Kontakt`, found no NKI files, and reported zero
translations while passing. Parse settings as JSON, expand an umbrella's
Kontakt child, deduplicate roots, support single-library roots, and include
root-level NKI files such as Morphology. Report an empty corpus explicitly;
an explicitly supplied empty Kontakt corpus fails instead of passing. A
temporary synthetic directory tree covers those discovery cases without
licensed data.

## v1/v2 strengths and remaining boundaries

The v1 code is useful as a verified implementation source for retained DSP
and runtime contracts. The current v2 has already absorbed substantial
parts of it: native Distortion/Saturation, bounded callback text lookups,
UI callback ordering and physical source/control ownership. The retained
[v1 KSP census](../audit-2026-10-10/w5-ksp-v1-parity.md) explains these ports
and the original gaps; its old counts are not today's execution verdict.

Three v1 keyboard-range getters remain absent in current v2:
`get_keyrange_min_note`, `get_keyrange_max_note`, `get_keyrange_name`.
`set_map_editor_event_color` is also absent, but was a v1 no-op. Recovering
the latter cannot improve fidelity. Current KSP ledger totals are 2,045
rows: 15 Implemented, 1,869 Approximate, 57 Stub, 104 Unsupported. Rows
include constants and language constructs; these are conservative
per-construct statuses, not command counts or a library pass percentage.

`set_global_key`, `set_global_scale` and `subscribe_async` still lack the
rack-wide service and subscriber delivery. Their old branch's accepted
calls were not complete behavior. Configured output-name callbacks,
native folder provisioning beyond library/patch inference, mutable zones,
MIDI input and analysis commands also retain explicit gaps. Source-only
parity reporting does not execute initialization and must not replace
production callback/load checks.

Kontakt DSP follow-up should start from the extracted evidence already in
`docs/re/`: modern Reverb's eight recovered dispatch targets, convolution
resize/filter/early-late preparation, Pro compressor scheduling, dynamic
Formant/filter-Phaser clocks, and live bus pan slew/invert/swap. Existing
approximations and diagnostics remain visible. The
[round-three receipt](../audit-2026-10-11/kontakt-fx-3-handoff.md) gives exact
functions and missing mappings. It explicitly reports computed-law
vectors, not new Kontakt recordings; a calibrated host comparison is
still needed before claiming native fidelity.

## Validation

Focused Cargo checks use `/home/derpcat/.cache/kontakto-heavy` and
`--profile ci`, serialized with the coordinator:

- `sampler-ksp --features cache --lib
  cached_initializer_preserves_callback_folder_lookups`: failing-first
  warm load returned the empty-name group 2 instead of library-name group
  0; cold initialization passed. After preserving folders, the entire
  `init_cache::tests` group passes 6/6.
- `sampler-kontakt --test real_libraries
  library_discovery_handles_umbrella_container_and_single_library_roots`:
  passes. The baseline zero-match problem was observed by the coordinator;
  this synthetic helper fixture was added with the repair.
- `KONTRA_KONTAKT_LIBRARIES=/mnt/MAIN_STORAGE/Libraries/Kontakt`,
  `sampler-kontakt --test real_libraries
  installed_instruments_translate_or_report_why_not`: **11/11** first
  instruments translated, 3.94 s. Libraries: ANALOG STRINGS, Afflatus,
  Areia, CHORUS, Dolce, Conflux, Morphology, Pacific, Vista, Solo, Una Corda.
  Unsupported-feature reports remain; this checks metadata translation,
  not every preset's load/render/native parity.
- `sampler-kontakt --test resources
  authored_resource_containers_are_bounded_and_case_insensitive`: passes,
  including the existing NICNT absent-name/listed-unavailable distinction.

The initial KSP command without `--features cache` selected zero tests;
it is excluded from validation. Existing compiler warnings remain.
No PCM, protected script source or raw library payload was written or
included in this report.
