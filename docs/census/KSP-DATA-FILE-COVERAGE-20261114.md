# KSP data and file side: coverage from the manual

Generated 2026-11-14 from the manual text (`PluginArtifacts/fresh-engine/docs-txt/ksp.txt`), `crates/script/ksp/src/builtins.rs` and the engine/host sources. "handled" only means the action is matched in the engine or host; behaviour is not verified.

| command | action | status |
|---|---|---|
| `array_equal()` | ArrayEqual | vm: pure |
| `attach_zone()` | AttachZone | engine: handled |
| `by_marks()` | ByMarks | engine: handled |
| `by_track()` | ByTrack | engine: **unhandled** |
| `detect_drum_type()` | - | frontend: missing |
| `detect_instrument_type()` | - | frontend: missing |
| `detect_key()` | - | frontend: missing |
| `detect_loudness()` | - | frontend: missing |
| `detect_peak()` | - | frontend: missing |
| `detect_pitch()` | - | frontend: missing |
| `detect_rms()` | - | frontend: missing |
| `detect_sample_type()` | - | frontend: missing |
| `detect_tempo()` | - | frontend: missing |
| `fs_get_filename()` | FsGetFilename | engine: handled |
| `get_folder()` | GetFolder | engine: handled |
| `get_keyrange_max_note()` | - | frontend: missing |
| `get_keyrange_min_note()` | - | frontend: missing |
| `get_keyrange_name()` | - | frontend: missing |
| `get_loop_par()` | - | frontend: missing |
| `get_num_zones()` | GetNumZones | engine: handled |
| `get_purge_state()` | GetPurgeState | engine: handled |
| `get_sample()` | - | frontend: missing |
| `get_sel_zones_idx()` | - | frontend: missing |
| `get_zone_id()` | GetZoneId | engine: **unhandled** |
| `get_zone_par()` | GetZonePar | engine: handled |
| `get_zone_status()` | - | frontend: missing |
| `load_array()` | LoadArray | engine: handled |
| `load_array_str()` | LoadArrayStr | engine: handled |
| `load_ir_sample()` | LoadIrSample | engine: handled |
| `load_komplete_ui()` | - | frontend: missing |
| `load_midi_file()` | LoadMidiFile | engine: **unhandled** |
| `load_performance_view()` | LoadPerformanceView | engine: handled |
| `make_instr_persistent()` | MakeInstrPersistent | engine: **unhandled** |
| `make_persistent()` | MakePersistent | engine: **unhandled** |
| `mf_copy_export_area()` | MfCopyExportArea | engine: **unhandled** |
| `mf_get_buffer_size()` | MfGetBufferSize | engine: **unhandled** |
| `mf_get_event_par()` | MfGetEventPar | engine: **unhandled** |
| `mf_get_first()` | MfGetFirst | engine: **unhandled** |
| `mf_get_id()` | MfGetId | engine: **unhandled** |
| `mf_get_last()` | MfGetLast | engine: **unhandled** |
| `mf_get_last_filename()` | MfGetLastFilename | engine: **unhandled** |
| `mf_get_mark()` | MfGetMark | engine: **unhandled** |
| `mf_get_next()` | MfGetNext | engine: **unhandled** |
| `mf_get_next_at()` | MfGetNextAt | engine: **unhandled** |
| `mf_get_num_tracks()` | MfGetNumTracks | engine: **unhandled** |
| `mf_get_prev()` | MfGetPrev | engine: **unhandled** |
| `mf_get_prev_at()` | MfGetPrevAt | engine: **unhandled** |
| `mf_insert_event()` | MfInsertEvent | engine: **unhandled** |
| `mf_insert_file()` | MfInsertFile | engine: **unhandled** |
| `mf_remove_event()` | MfRemoveEvent | engine: **unhandled** |
| `mf_reset()` | MfReset | engine: **unhandled** |
| `mf_set_buffer_size()` | MfSetBufferSize | engine: **unhandled** |
| `mf_set_event_par()` | MfSetEventPar | engine: **unhandled** |
| `mf_set_export_area()` | MfSetExportArea | engine: **unhandled** |
| `mf_set_mark()` | MfSetMark | engine: **unhandled** |
| `mf_set_num_export_areas()` | MfSetNumExportAreas | engine: **unhandled** |
| `purge_group()` | PurgeGroup | engine: handled |
| `read_persistent_var()` | ReadPersistentVar | engine: handled |
| `redirect_output()` | RedirectOutput | engine: **unhandled** |
| `remove_keyrange()` | RemoveKeyrange | engine: **unhandled** |
| `save_array()` | SaveArray | engine: handled |
| `save_array_str()` | SaveArrayStr | engine: handled |
| `save_midi_file()` | SaveMidiFile | engine: **unhandled** |
| `set_keyrange()` | SetKeyrange | engine: **unhandled** |
| `set_loop_par()` | - | frontend: missing |
| `set_midi()` | - | frontend: missing |
| `set_num_user_zones()` | - | frontend: missing |
| `set_sample()` | - | frontend: missing |
| `set_snapshot_type()` | SetSnapshotType | engine: handled |
| `set_zone_par()` | SetZonePar | engine: handled |
| `wait_async()` | WaitAsync | engine: handled |
| `watch_array_idx()` | WatchArrayIdx | engine: **unhandled** |

Caveats: "unhandled" means the action is not named in the engine or host sources; `make_persistent`/`make_instr_persistent` and `get_zone_id` are compiler or table answers that this scan cannot see, so check each before counting it a gap. The rows not listed as unhandled or missing are named by the engine, not verified against Kontakt.

## What the table shows

1. **The whole MIDI object is unimplemented**: 25 `mf_*` commands, `save_midi_file`, `load_midi_file`, `by_track`, the `$EVENT_PAR_MIDI_*` / `$EVENT_PAR_POS` / `$EVENT_PAR_NOTE_LENGTH` / `$EVENT_PAR_TRACK_NR` / `$CURRENT_EVENT` / `$MARK_n` / `$MIDI_COMMAND_*` constants (the table `crates/script/ksp/data/constants.tsv` has `$MIDI_COMMAND_*` and `$EVENT_PAR_MIDI_CHANNEL`=11 only; the other values are unmeasured symbols). A 1,171-line v2 implementation with an SMF reader and writer is in history (`git show 4c1e1d85e:crates/sampler-core/src/midi_object.rs`, GPT-authored); it guesses its parameter values (13, 17..23), so those must be measured in the oracle before it is ported (`tools/kontakt-oracle/ksp_constants.py`).
2. **Frontend gaps** (the compiler does not know the command): `get_sample`, `set_sample`, `set_num_user_zones`, `get_zone_status`, `get_sel_zones_idx`, `get_loop_par`/`set_loop_par`, `set_midi`, `detect_*` (9), `get_keyrange_*`, `load_komplete_ui`.
3. **Engine gaps behind a known command**: `set_keyrange`/`remove_keyrange`, `redirect_output`, `watch_array_idx`, `by_track`.
4. **Un-purge** (this table's `purge_group` row) is #1524.

## Order of work

MIDI object (measure constants, then port with the v3 file-request channel for `mf_insert_file`/`save_midi_file`/`load_midi_file`) -> user zones and samples (`set_sample`, `get_sample`, `set_num_user_zones`, zone status) -> persistence timing probes (`on persistence_changed`, `instrument_persistence`, snapshots) -> `load_array`/`save_array` mode and path audit -> `file_selector` -> keyranges/detect_*.
