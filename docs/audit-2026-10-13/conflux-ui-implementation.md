# Conflux UI implementation handoff — 2026-10-13

## Branch + worktree path + commit SHAs (oldest first)

- Branch: `gpt/conflux-ui`, based on `origin/main` at `ce0a2e61`.
- Worktree: `/home/derpcat/.t3/worktrees/KONTAKTO/gpt-conflux-ui`.
- `f844d5a4`: restart checkpoint; authored composition routing and NCKP presentation import.
- `13ac41a0`: per-program resource ownership, six-frame skin states, publication and capture corrections.
- This report and the installed NCKP inventory regression follow those implementation commits.

Implementation is committed. Complete Conflux UI parity is **not verified**. The user's final NEVER BUILD rule forbids plugin builds/tests and screenshot runs by this agent. The coordinator must validate the final renderer changes, gestures and host save/recall after merging.

## Per item: before → after, root cause, evidence

### Authored composition in Vector and multi source selection

Before: Vector painted raw KSP/NCKP rectangles with overlapping controls and no evaluated NativeUI graph. An earlier classic performance source could also displace a later `load_native_ui` request, producing a blank/default multi face.

After: Original and Vector submit the evaluated NativeUI graph as live MUI primitives, preserve its authored geometry and pointer identities, and give an explicit NativeUI request priority over classic performance intent. This uses the existing native painter; it does not flatten the graph to a screenshot. KSP controls remain engine parameter handles. See `src/ui/part.rs:288` and `src/ui/part.rs:620`.

Focused regressions: `ui::part::tests::native_composition_wins_over_an_earlier_classic_performance_source`; `ui::part::tests::original_and_vector_keep_the_same_native_composition_and_pointer_identity`. The latter was observed failing before the routing correction: Vector had no native pointer surface while Original retained `nui-1-root/component` at `(296,151,864,432)`. Receipt: [vector-red.log](/home/derpcat/.cache/kontakto-gpt-conflux-ui/vector-red.log). Final green direction and rendered screenshots await the coordinator. Source-priority failing-first direction was not run.

### Multi controller uses its own resource container

Before: a multi's controller and Conflux instrument both requested `main`, but resources were inferred solely from the multi's enclosing library. The controller consequently painted Conflux's skin with unbound/default controls. The old page capture even passed while all three pages retained only 19 targets: paint success did not establish correct ownership.

After: `Resources::for_program` follows the selected BProgram's existing F0 special-filename reference, resolves that exact container through the existing sample/content path resolver, and rebases loose resources to the same owner. An explicit absent/invalid owner fails instead of borrowing another program's `main`. NativeUI resource workers and face identity now include the selected program. See `crates/sampler-kontakt/src/resources.rs:71`, `src/ui/pictures.rs:84`, `src/ui/native_ui.rs:288`, and `src/ui/part.rs:60`.

Passing focused test: `resources::tests::program_container_reference_disambiguates_same_named_native_modules` (0.00 s). It exercises two same-named modules with distinct owners and invalid references. Failing-first direction was not run; the fixture explicitly demonstrates the old library-inferred module before asserting correct per-program reads.

Passing installed test: `installed_conflux_multi_resolves_each_authored_resource_owner`:

| Multi | Program 0 controller / NativeUI namespace members | Program 1 instrument | Test execution |
| --- | --- | --- | --- |
| Conflux Arps | F0=2, `Content/Tools/Phrases/Samples/Phrases 1.0.nkr`, 511 | F0=6, `Samples/Conflux.nkr`, 282 | 0.30 s |
| Healed Heart | F0=2, Phrases, 511 | F0=6, Conflux, 282 | 0.34 s |
| Mystery Box | F0=2, `Content/Tools/Patterns/Samples/Patterns 1.0.nkr`, 461 | F0=5, Conflux, 282 | 0.28 s |

These are resource-resolution measurements, **not** controller rendering or engine/persistence results. Logs: [Arps](/home/derpcat/.cache/kontakto-gpt-conflux-ui/resource-installed.log), [Healed Heart](/home/derpcat/.cache/kontakto-gpt-conflux-ui/resource-healed-heart.log), [Mystery Box](/home/derpcat/.cache/kontakto-gpt-conflux-ui/resource-mystery-box.log).

### NCKP fonts, press flags and visible table steps

Before: v2 ignored fields v1 already imported: custom font names, six responsive font states, caption press shifting, and visible table prefixes.

After: custom names resolve through the existing runtime font registry after earlier script fonts; responsive states inherit omitted styles; `shiftOnPress` reaches UI IR and live patches; visible table steps keep the full array while displaying its authored prefix. The property constants are appended, preserving prior numeric identities. See `crates/sampler-ksp/src/nckp.rs:242`, `crates/sampler-ksp/src/eval.rs:875`, `crates/sampler-ksp/src/ui.rs:529`, and `crates/sampler-ui-ir/src/publication.rs:120`.

Port source: `0cb7a8a0:src/ksp/performance_view.rs`, with existing `get_font_id`/table-step semantics. Vendor references: [KSP Control Parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-parameters) and [UI Commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands).

All four added `sampler-ksp --test nckp nckp_` regressions were observed failing before import changes and passing afterward: `nckp_custom_font_uses_the_existing_runtime_font_registry`, `nckp_state_fonts_override_base_and_keep_absent_state_inheritance`, `nckp_press_shift_flag_reaches_the_authored_control_property`, and `nckp_visible_steps_keeps_storage_and_projects_the_authored_prefix`. [Receipt](/home/derpcat/.cache/kontakto-gpt-conflux-ui/nckp-final.log). The press test additionally passes its IR/no-unsupported assertions. `publication::tests::caption_press_behavior_publishes_and_reverts_without_changing_identity` passes (0.00 s; failing-first direction not run).

Passing installed NCKP regression: `installed_conflux_nckp_imports_every_control_and_presentation_field` imports **378/378** control objects with **zero parser diagnostics**, checks **68** responsive-font configurations, **68** press flags and **6** visible-table settings (0.16 s). [Receipt](/home/derpcat/.cache/kontakto-gpt-conflux-ui/nckp-installed.log). This corrects an earlier manual count of 384; the recursive resource count is 378. This does not remove the unrelated 213 initialized NKS metadata diagnostics.

`nckp::tests::imported_ui_fields_are_no_longer_reported_as_losses` and `parity::tests::imported_performance_view_fields_are_not_reported_as_losses` pass. Diagnostics disappear because these fields now import and project; unknown control types/resource failures remain reported.

### Captions and six-state skins

Before: bitmap buttons/switches lost authored captions; stock controls could ignore authored ink; a menu's numeric selected value picked its ON font; switches/buttons used only two strip frames and menus always used frame zero.

After: captions overlay skins using the authored style/text position and press flag; menus choose their ON state when their popup opens; Kontakt skins select off/on, pressed and hovered frames with held-pointer precedence. Explicit picture frames win. Falcon picture-state selection retains its existing policy. See `src/ui/ir_view.rs:1156`, `src/ui/ir_view.rs:1203`, and `crates/sampler-ui-ir/src/lib.rs:636`.

Six-frame source: NI-authored [Kontakt 4 Player Developer Guide §5.4.2](https://arkdev.radium-audio.com/assets/dl.php?f=Kontakt_4_Player_+Developer_Guide.pdf&t=pdf), p.34. `tests::kontakt_picture_strip_uses_all_six_responsive_states` was observed failing with `[0,0,0,1,1,1]`, then passing with `[0,2,4,1,3,5]` (0.00 s). [Red](/home/derpcat/.cache/kontakto-gpt-conflux-ui/picture-states-red.log), [green](/home/derpcat/.cache/kontakto-gpt-conflux-ui/picture-states-green.log).

Added renderer regressions, **not compiled/run under the final rule**: `ui::ir_view::render_tests::skinned_button_caption_keeps_its_text_and_state_ink`, `menu_responsive_font_uses_popup_state_instead_of_selected_numeric_value`, `button_press_caption_shift_obeys_disable_text_shifting`, and `kontakt_switch_skin_selects_pressed_hover_and_fixed_picture_states`. Their failing-first direction was not run. The caption displacement test asserts one authored source pixel down/right; that displacement still needs native-host confirmation.

### Library inventory and captures

The library KSP declarations, NCKP, picture sidecars and NativeUI resources were read in memory before changes; no decrypted scripts/samples/keys were retained. Conflux NKR has 134 `.nui` modules and 282 native namespace members. Picture sets: icon=1 frame, lock=1 with two-pixel fixed margins, overlay=6. Actual Conflux custom-font names are empty; synthetic tests cover custom names.

[Every initialized object, its declaration position, parent and binding](/home/derpcat/.cache/kontakto-gpt-conflux-ui/control-inventory.md): GUI slot 2 has 378 objects; Mono & Glide slot 3 has 22; Timer slot 4 has one hidden slider. [Full metadata](/home/derpcat/.cache/kontakto-gpt-conflux-ui/before-nki/inventory.json) includes ranges, menu choices and captions. These rectangles are **local parameter geometry, not the evaluated NativeUI layout**. [Evaluated page surfaces](/home/derpcat/.cache/kontakto-gpt-conflux-ui/before-nki/pages.json) retain the actual before-run positions and pointer identities.

Intended top-level selectors: Play=0, Edit=1, Settings=2, Credits=3. Classic KSP page selector: Synth/Modulation/FX/Macros/Settings. The NativeUI Edit page composes source, audio mod, filter, two LFOs and envelopes above FX/space/animator. The footer has Bend/Mod/Velocity/AT/Random, six macros and volume. Settings carries controller curves, tuning, mono/glide and voices; Mono & Glide contributes cross-script bindings. Tables include animator 32, macro amount/assignment 60 each, hidden sound-category 8, velocity/aftertouch 128 each. Synth wave displays are authored NativeUI graphics; the initialized NCKP has no standalone `ui_waveform`.

Before screenshots, captured before the final ban: [Play](/home/derpcat/.cache/kontakto-gpt-conflux-ui/before-nki/play.png), [Edit](/home/derpcat/.cache/kontakto-gpt-conflux-ui/before-nki/edit.png), [Settings](/home/derpcat/.cache/kontakto-gpt-conflux-ui/before-nki/settings.png), [overlapping Vector](/home/derpcat/.cache/kontakto-gpt-conflux-ui/before-vector/initial.png), [wrong-owner multi](/home/derpcat/.cache/kontakto-gpt-conflux-ui/before-arps-0/edit.png). NKI page captures observed 19/109/50 live targets. Original's overall panel arrangement matches the [vendor Edit-page overview](https://docs.native-instruments.com/ni-tech-manuals/conflux-manual/en/edit-page.html), but its source/filter labels visibly wrap; different patch state prevents a pixel-parity conclusion. Vendor images are retained privately in the same cache. Frozen v1 binaries were unavailable; comparison used v1 source.

After screenshots are intentionally pending. The existing product editor/CPU painter capture gate now records the actual selected resource root, preserves an initial-page receipt, and accepts `KONTRA_CONFLUX_PAGES` for additional tab paths. Factory controllers default to their own initial page instead of bogus Conflux tab expectations.

## Files touched outside your scope (and why)

- Shared `src/ui/native_runtime.rs`: only the small loader hunk introducing `load_cancel_for_program` and using program-owned resources. Necessary to disambiguate `main`; no frame-cost, interpreter, scheduling or DSP changes. Coordinator must reconcile this hunk with the perf thread.
- `crates/sampler-ksp/src/{eval.rs,builtins.rs,ui.rs}`: connect NCKP presentation properties to existing model/IR and preserve property identities; no script callback/engine algorithm changes.
- `crates/sampler-kontakt/src/resources.rs` and its tests: selected-program resource ownership, required for multi UI resources. `parity.rs` only adjusts the obsolete field-loss fixture.
- `tools/corpus-health/src/inspect.rs`: only the existing fixture's expected omitted-field count, because those fields now import. No corpus tool build/run was performed after the ban.
- No edits to `library.rs`, `lower.rs`, `mix_tree.rs`, `src/ui/tests.rs` or sampler-core/DSP files.

## Still open: item + concrete reason

- Final root UI compile/tests and after captures: explicitly forbidden to this agent by NEVER BUILD. Static review/rustfmt parsing and focused crate tests cannot establish root compilation or paint correctness.
- Complete native layout/pixel parity, wrapped labels, Credits, every popup, browser/snapshot and modulation-assignment state: no calibrated same-state Kontakt capture and no permitted final renderer run. The capture command below covers the three named top-level tabs; it does **not** certify every popup/substate.
- Every gesture reaching the engine and host save/recall: existing real-pointer/host-state harness is prepared, but final execution is coordinator-owned. No full persistence success is claimed.
- Correct Phrases/Patterns controller rendering and bindings: resource ownership is proven; evaluating their final NativeUI graphs remains unverified.
- Engine/script behavior and frame-rate cost: assigned to other threads, untouched. The readback/perf architecture was not changed or measured.

## Anything the coordinator must act on (conflicts, follow-ups)

Merge the commits in order and reconcile the minimal `native_runtime.rs` loader hunk. Run the coordinator's single root build/test pass with `shots`; it must include the six added unignored renderer/source-selection regressions. Retain a freshly merged test executable containing `ui::widget_gate::conflux_authored_pages_and_inventory`; do not reuse this agent's pre-fix executable.

The following is the exact finite capture command for Conflux.nki and both programs of three multis. **Not executed by this agent.** Set `KONTRA_TEST_BINARY` to the coordinator's newly compiled root test executable, then run once:

```bash
set -euo pipefail
: "${KONTRA_TEST_BINARY:?set this to the freshly merged shots-enabled test executable}"
export KONTRA_KONTAKT_CONTENT='/home/derpcat/.wine/drive_c/Program Files/Common Files/VST3/Portapotty/Kontakt 8/Content'
conflux_library='/mnt/MAIN_STORAGE/Libraries/Kontakt/Conflux 1.1.0 [Native Instruments]'
conflux_evidence='/home/derpcat/.cache/kontakto-gpt-conflux-ui'
for conflux_view in 1 3; do
  KONTRA_WIDGET_GATE_PATH="$conflux_library/Instruments/Conflux.nki" \
  KONTRA_WIDGET_GATE_PROGRAM=0 KONTRA_CONFLUX_VIEW="$conflux_view" \
  KONTRA_CONFLUX_SHOTS="$conflux_evidence/after-nki-$conflux_view" \
  "$KONTRA_TEST_BINARY" --ignored --exact ui::widget_gate::conflux_authored_pages_and_inventory --nocapture --test-threads=1 \
    > "$conflux_evidence/after-nki-$conflux_view.log" 2>&1
done
for conflux_multi in 'Conflux Arps' 'Healed Heart' 'Mystery Box'; do
  for conflux_program in 0 1; do
    KONTRA_WIDGET_GATE_PATH="$conflux_library/Multis/$conflux_multi.nkm" \
    KONTRA_WIDGET_GATE_PROGRAM="$conflux_program" KONTRA_CONFLUX_VIEW=1 \
    KONTRA_CONFLUX_SHOTS="$conflux_evidence/after-$conflux_multi-$conflux_program" \
    "$KONTRA_TEST_BINARY" --ignored --exact ui::widget_gate::conflux_authored_pages_and_inventory --nocapture --test-threads=1 \
      > "$conflux_evidence/after-$conflux_multi-$conflux_program.log" 2>&1
  done
done
```

Exact interaction/save-recall command (apply to the same seven preset/program cases as above, using the same freshly merged executable):

```bash
KONTRA_WIDGET_GATE_PATH="$conflux_library/Instruments/Conflux.nki" \
KONTRA_WIDGET_GATE_PROGRAM=0 \
"$KONTRA_TEST_BINARY" --ignored --exact ui::widget_gate::original_widget_gestures --nocapture --test-threads=1 \
  > "$conflux_evidence/after-nki-gestures.log" 2>&1
```

Inspect the JSON `status`, `coverage_complete`, per-target `parameter_reached`/`persistence`, faults and diagnostics. A test process exiting successfully only proves the harness ran. Navigation-only or unbound/default targets must not be silently accepted as parity. The harness keeps serialized host state in memory; do not export library script state. Browser/snapshot and Credits coverage may need additional real editor navigation after inspecting the captured graph; `KONTRA_CONFLUX_PAGES` can specify a comma-separated path of clickable captions.

The installed-resource tests use the existing `KONTRA_KONTAKT_CONTENT` override because the factory Content is present inside the user's Wine installation. No Wine discovery fallback or installation changes were added. If Content is absent, install/select the real resource root; do not fall back to Conflux's skin.

## Topology facts learned

- NativeUI composition, KSP parameter declarations and resource ownership are independent: equal entry names (`main`) do not imply equal programs or resources.
- Existing NI-file BProgram F0 decoding indexes the special filename table. In these multis it points to factory Phrases/Patterns for program 0 and Conflux for program 1. Program ordering follows the loader's sorted bank slots and flattened program lists.
- The NativeUI graph supplies final geometry/visibility. Raw NCKP geometry is not an authored layout oracle, and a successful CPU paint can render an entirely wrong resource owner.
- No new binary DSP law was inferred or implemented. Resource ownership uses the repository's existing verified public-record parser rather than new guessed binary offsets.
