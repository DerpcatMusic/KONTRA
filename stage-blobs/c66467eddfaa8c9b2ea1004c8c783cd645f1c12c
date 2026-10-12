# Local library compatibility inventory

**Historical static inventory, 2026-09-29.** The implementation-gap counts below predate the playback runtime, DSP and filesystem repair. Use [LIBRARIES.md](LIBRARIES.md) for representative playback checks and [REPLACEMENT_REVIEW.md](REPLACEMENT_REVIEW.md) for the dated code review; these old counts are not the current support matrix.

Static NKI/NKM and resource-reference audit; no full audio decode or Kontakt behavioral validation. Chunk presence does not prove activation. Opaque private fields remain unmapped.

Generated with `kontakto audit` (plain JSON or gzip), `kontakto audit-structure`, `kontakto audit-scripts`, then `python3 tools/summarize_audit.py`.

| Library | Presets | Programs | Parse errors | Complete references | Complete groups | Init previews / scripts |
|---|---:|---:|---:|---:|---:|---:|
| Afflatus Chapter II Brass | 348 | 348 | 0 | 0 | 3514 | 0 / 348 |
| Areia 1.2.0 [Audio Imperia] | 155 | 155 | 0 | 0 | 10584 | 0 / 155 |
| Audio Imperia CHORUS | 42 | 48 | 0 | 3 | 642 | 0 / 48 |
| Audio Imperia Dolce | 77 | 77 | 0 | 0 | 1107 | 0 / 77 |
| Pacific Ensemble Strings | 49 | 49 | 0 | 11 | 738 | 29 / 49 |
| Performance Samples Vista | 7 | 7 | 0 | 7 | 308 | 7 / 7 |
| Solo | 100 | 100 | 0 | 100 | 20590 | 0 / 100 |
| Una Corda Library | 3 | 3 | 0 | 0 | 38 | 0 / 12 |

A complete reference set does not certify decoding or playback. Zero-zone controller instruments can have complete references and no playable groups.

## Implementation gaps

| Gap | Affected programs |
|---|---:|
| KSP runtime callbacks are not implemented | 787 |
| Kontakt effects and modulation are not imported; playback uses the sampler's envelope | 787 |
| Release-trigger monophony/counter behavior is not imported | 787 |
| Zone sample-start modulation is not imported | 743 |
| Missing/damaged sample references | 666 |
| Voice-group allocation/choke settings are not imported | 351 |
| Zone crossfades are not imported | 97 |
| KSP initialization: Keyboard state captured; script keyboard display is not connected | 36 |
| KSP initialization: Not executed: SET_CONDITION | 36 |
| KSP initialization: Not executed: message | 36 |
| KSP initialization: Not executed: set_knob_label | 36 |
| KSP initialization: Not executed: set_knob_unit | 36 |
| KSP initialization: PGS storage updated; pgs_changed callbacks are not dispatched | 36 |
| KSP initialization: Saved KSP variable values are not restored | 36 |
| Multiple loops are parsed; playback currently uses the first supported loop | 10 |
| Multi routing, master processing and multi scripts are not restored; parts use manual playback | 9 |
| KSP initialization: Listener registered; listener callbacks are not dispatched | 5 |
| Native group start conditions are not implemented | 4 |
| An unsupported alternating/counted/tuned loop was skipped | 3 |
| Archive member/directory damage (see per-program report) | 3 |

## Initialization blockers

| First error per script | Affected programs |
|---|---:|
| Unsupported KSP expression function: find_mod | 348 |
| Unsupported KSP expression function: get_font_id | 222 |
| KSP source exceeds 16 MiB | 152 |
| KSP array memory limit | 20 |
| Expected integer | 3 |
| Unknown PGS integer key | 3 |
| Unsupported KSP initialization command: set_snapshot_type | 3 |
| Unsupported KSP initialization command: show_library_tab | 3 |

## KSP requirements

All non-init callbacks remain unimplemented. Command presence below does not imply execution support; initialization-only behavior is limited to the subset described in the project README.

### Callbacks

| Observed name | Programs |
|---|---:|
| `init` | 787 |
| `note` | 787 |
| `release` | 787 |
| `controller` | 784 |
| `ui_control` | 784 |
| `listener` | 725 |
| `pgs_changed` | 433 |
| `persistence_changed` | 380 |
| `async_complete` | 20 |

### Calls

| Observed name | Programs |
|---|---:|
| `get_ui_id` | 787 |
| `inc` | 787 |
| `make_perfview` | 787 |
| `message` | 787 |
| `set_control_par` | 787 |
| `set_control_par_str` | 787 |
| `set_key_color` | 787 |
| `set_script_title` | 787 |
| `set_ui_height_px` | 787 |
| `add_menu_item` | 784 |
| `get_control_par` | 784 |
| `get_engine_par_disp` | 784 |
| `in_range` | 784 |
| `make_persistent` | 784 |
| `read_persistent_var` | 784 |
| `set_engine_par` | 784 |
| `find_mod` | 781 |
| `move_control_px` | 781 |
| `purge_group` | 781 |
| `ignore_event` | 780 |
| `play_note` | 780 |
| `SET_CONDITION` | 779 |
| `abs` | 778 |
| `num_elements` | 778 |
| `output_channel_name` | 778 |
| `allow_group` | 776 |
| `disallow_group` | 776 |
| `note_off` | 763 |
| `set_controller` | 760 |
| `search` | 747 |
| `wait` | 743 |
| `dec` | 738 |
| `set_listener` | 730 |
| `set_key_pressed` | 728 |
| `set_key_pressed_support` | 728 |
| `set_key_type` | 728 |
| `set_ui_width_px` | 728 |
| `change_tune` | 725 |
| `exit` | 725 |
| `get_event_par` | 725 |
| `set_key_name` | 725 |
| `sh_left` | 725 |
| `sh_right` | 725 |
| `set_event_par` | 613 |
| `fade_out` | 610 |
| `set_text` | 502 |
| `add_text_line` | 479 |
| `pgs_create_key` | 436 |
| `pgs_set_key_val` | 436 |
| `int_to_real` | 419 |
| `real_to_int` | 419 |
| `ignore_controller` | 387 |
| `remove_keyrange` | 380 |
| `set_keyrange` | 380 |
| `set_menu_item_str` | 380 |
| `random` | 377 |
| `set_menu_item_visibility` | 377 |
| `disable_logging` | 375 |
| `group_name` | 375 |
| `get_font_id` | 374 |
| `round` | 374 |
| `stop_wait` | 374 |
| `load_ir_sample` | 351 |
| `set_skin_offset` | 351 |
| `set_control_help` | 348 |
| `set_ui_color` | 348 |
| `ticks_to_ms` | 348 |
| `save_array` | 347 |
| `set_event_par_arr` | 327 |
| `reset_rls_trig_counter` | 289 |
| `change_note` | 274 |
| `sin` | 274 |
| `change_vol` | 265 |
| `get_engine_par` | 235 |
| `find_group` | 232 |
| `load_array` | 106 |
| `set_table_steps_shown` | 100 |
| `set_knob_label` | 56 |
| `set_knob_unit` | 56 |
| `log` | 45 |
| `exp` | 41 |
| `get_folder` | 14 |
| `fs_get_filename` | 12 |
| `fs_navigate` | 12 |
| `load_array_str` | 12 |
| `save_array_str` | 12 |
| `wait_async` | 12 |
| `attach_zone` | 3 |
| `event_status` | 3 |
| `fade_in` | 3 |
| `find_target` | 3 |
| `get_menu_item_str` | 3 |
| `make_instr_persistent` | 3 |
| `pgs_get_key_val` | 3 |
| `set_snapshot_type` | 3 |
| `set_ui_wf_property` | 3 |

### Controls

| Observed name | Programs |
|---|---:|
| `ui_label` | 787 |
| `ui_menu` | 784 |
| `ui_slider` | 784 |
| `ui_switch` | 784 |
| `ui_value_edit` | 781 |
| `ui_button` | 778 |
| `ui_text_edit` | 374 |
| `ui_table` | 141 |
| `ui_knob` | 56 |
| `ui_file_selector` | 12 |
| `ui_waveform` | 3 |


## Source structures

Presence includes bypassed/default objects. Counts are presets, not active effects. The JSON sidecar retains affected paths and every observed KSP symbol.

| Chunk | Presets |
|---|---:|
| 0x06 BParScript | 781 |
| 0x07 BParEnv | 781 |
| 0x0d BParInternalMod | 781 |
| 0x17 BParFXSendLevel | 781 |
| 0x25 BParFX | 781 |
| 0x32 VoiceGroups | 781 |
| 0x38 StartCriteriaList | 781 |
| 0x3a BParameterArraySerBParFX8 | 781 |
| 0x3b BParameterArraySerBParInternalMod16 | 781 |
| 0x3c BParameterArraySerBParExternalMod32 | 781 |
| 0x3f BParEnvAhdsr | 781 |
| 0x45 BInsertBus | 781 |
| 0x4a BParGroupDynamics | 781 |
| 0x4e QuickBrowseData | 781 |
| 0x0c BParExternalMod | 777 |
| 0x59 BParFXGaloisReverb | 374 |
| 0x16 BParFXIRC | 366 |
| 0x40 BParEnvFm7 | 176 |
| 0x13 BParFXGainer | 143 |
| 0x19 BParFXCompressor | 6 |
| 0x10 BParFXDelay | 3 |
| 0x11 BParFXChorus | 3 |
| 0x12 BParFXFlanger | 3 |
| 0x14 BParFXPhaser | 3 |
| 0x18 BParFXFilter | 3 |
| 0x1d BParFXSurroundPanner | 3 |
| 0x1e BParFXDistortion | 3 |
| 0x1f BParFXStereoSpread | 3 |
| 0x20 BParFXLofi | 3 |
| 0x21 BParFXSkreamer | 3 |
| 0x22 BParFXRotator | 3 |
| 0x42 BParFXTape | 3 |
| 0x43 BParFXTrans | 3 |
| 0x44 BParFXSSLGEQ | 3 |
| 0x46 BParFXSSLGBusComp | 3 |
| 0x4c BParFXFBComp | 3 |

Source inspection error categories: 0.

## Acceptance gates

- Reference resolution, bounded sample decoding and finite audio renders.
- Restore source envelopes, modulation, effects, group triggers and multi routing.
- Execute KSP callbacks and restore saved control values; initialization previews alone do not satisfy this.
- Compare controlled MIDI renders and UI actions against Kontakt reference results.
- Keep missing or zero-filled resources blocked until intact data is available.
