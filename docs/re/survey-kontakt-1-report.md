# survey-kontakt-1 handoff

Branch: gpt/survey-kontakt-1. Worktree:
/home/derpcat/.t3/worktrees/KONTAKTO/gpt-survey-kontakt-1.
Base: 3962616cafd554e394c5542c99883ee34ad40586.
No push, PR or integration-worktree mutation.

## Per item: result, evidence and limitations

1. Horns peak 0.214049831 -> 0.560724378 was reproduced exactly in one paired
   frozen old/current CLAP trace. Live group EQ is the first divergent stage;
   both builds have the same positive modulation gains, including +18.
   Existing measured native EQ law, same-SHA cache FUN_1409591c0 and radare2
   constants/branch establish the changed gain law. No patch gain correction.
   Morphology survey peak 0.253639489 -> 0.199760884; source inspection shows
   newly supported native Ladder taps in its active chain, but a stage A/B and
   native full-patch level were not verified. See survey-kontakt-levels.md.
   Conditions: 48 kHz, block 128, C4/velocity64, CC1=100/CC11=127, five notes,
   six-second peak window. Load average was not recorded; traced timing is
   contended/unknown and no performance improvement is claimed.

2. get_folder: live callbacks returned empty -> host-supplied bounded paths,
   including computed selectors and empty missing selectors. Evidence:
   FUN_14078d910 case 9; radare2 0x14078e0ac/0x14078e216; NI load/save manual.
   lower.rs:1290, lib.rs Unit construction, parity.rs and display-test Unit
   plumbing. Focused strings::callback_folder_lookup_uses_host_paths_and_empty_missing_paths
   failed before (selected empty-name group 2 instead of path-name group 0),
   passes after, with a no-heap callback assertion. Other requested builtins
   remain open for the precise reasons in survey-ksp-builtins.md.

3. Source-only probe: Areia already has 16 choices, Solo eight, and Behavior
   switching ownership. Reports concern an additional invalid overlay range
   and a further set, respectively. Conflux NKM utility has control-key hints,
   not a recognized articulation set. No warning suppressed. See
   survey-kontakt-switches.md and tests/survey_intent.rs.

4. Conflux live wavetable destinations and NKM authored performance flags are
   enumerated in survey-conflux-gaps.md. No synthetic UI selection or guessed
   source law. Native global services, signed random law, source-module routes
   and aggregate post-activation UI comparison remain open.

5. Reverb dispatch/table and fractional network leads recovered with the
   same-SHA cache and radare2; see kontakt-reverb.md:31. Complete network
   coefficient initialization, feedback transform and ramps still require
   extraction before a genuine replacement. Convolution early/late split,
   time scaling/filter order, Una Corda filter/Pro compressor and Morphology
   unsupported source modes remain unchanged. No guessed DSP law was added.

6. Journal omitted semantic value -> compact named effect/target detail.
   Module parameter/effect groups retain distinct semantic subjects; route
   detail includes destination physical slot. KSP warnings now include physical
   script slot and line; concrete indices are retained in safe message detail.
   Stale module 0x1d label Surround Panner -> Saturation. Typed feature names and
   unsupported reasons are preserved. See sampler-kontakt/src/diagnostics.rs:2,
   library.rs:2022, load.rs:889, effects.rs:33, src/diagnostics.rs:1898 and
   src/diagnostics/grouped.rs:248.

## Verification

All cargo calls used debug=0 profiles and kontakto-heavy, serialized.
Focused checks passed:
- sampler-core --lib bell_matches_the_measured_kontakt_response (existing law).
- sampler-ksp --test strings callback_folder_lookup_uses_host_paths_and_empty_missing_paths (red -> green).
- sampler-kontakt --lib compact_details_name_effects_targets_and_actual_script_indices (red -> green; privacy guard included).
- sampler-kontakt --lib classic_saturation_saved_slot_is_admitted_in_both_scopes (label red -> green).
- sampler-kontakt --lib initialization_warnings_keep_physical_script_slot_line_and_index (green; failing-first location direction not verified).
- sampler-kontakt --lib targets_translate_with_kontakt_laws_or_are_reported (green; failing-first new slot assertion not verified).
- sampler-kontakt --test survey_intent survey_switch_and_conflux_intent --ignored (source-only installed intent probe passed).
- Existing payload privacy diagnostic and digital-LFO diagnostic tests also
  passed under the diagnostic-name filter; no full suite was run.

One final cargo test --workspace --no-run passed, exit 0. Root journal grouping
unit test was compiled, not executed under the crate-scoped test restriction.
No corpus survey, new release build or native Kontakt recording was run.

## Shared files and coordinator follow-up

No ownership file list was supplied. Minimal shared root changes:
src/diagnostics.rs calls the tested semantic-detail selector;
src/diagnostics/grouped.rs avoids folding unlike effect/parameter messages.
KSP parity/display constructors receive the existing folders snapshot.

Coordinator: merge/cherry-pick the branch in order and run the single combined
suite/survey. Review potential overlap in Kontakt load/library/effects and root
diagnostic grouping. Native parity items above are not acceptance-ready; do not
mark them closed from documentation or source-compile coverage alone.
Owned target and scratch are removed after final receipt capture; no job or
server is left running. Scratch trace files were deleted after attribution.
NEXT: coordinator native reference/extraction lanes for the explicitly open items.
