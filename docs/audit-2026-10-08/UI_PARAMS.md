# UI parameters, builtins, persistence and callback audit

Scope 3, 2026-10-08. Baseline: `origin/integrate/core-v2@7e82b152`. No product fixes. The only code additions are a single-instrument metadata probe and authored checks. The production frontends, UI IR and core remain untouched.

## Verdict and evidence boundary

**Worse than v1 for parameter semantics and saved UI state.** Six runnable probes establish six v2 gaps: real-property truncation, indexed readback aliasing and lost init cells, unhandled runtime setter aliases, ignored typed getters, repeat restoration after an immediate persistence read, and lost alignment without a font property. This is not a claim that every instrument hits each branch. V1 handles runtime text/movement aliases and typed/indexed VALUE through live widget memory; v2 separates init state, numeric runtime mirrors and a presentation-effect consumer that disagree.

Native Kontakt/Falcon execution was not performed. “Correct” below means the inspected mechanism or authored check matches the stated contract, not 100% vendor fidelity. Requested corpus columns are **lexical incidence**, not executed calls or confirmed broken instruments; partial shared results are available, with explicit coverage below. A successful parse is not a rendered or playable UI.

Inputs read: `PRODUCT.md`, `UI.md`, `UI_FRONTENDS.md`, `CONTROL_STATE.md`, `KSP_SURFACE.json`, `KSP_SYMBOLS.md`, `KSP_SEMANTICS.md`; read-only RE `UI_NATIVE_PRESERVATION.md` and `FALCON_RUNTIME_UI_GROUNDWORK.md`; pinned v1 `0cb7a8a0:src/ksp/ui.rs`, `src/ksp/calls.rs`, `src/ui/perf_view.rs`; Kontakt `UI_V1_PARITY.md`, UVI `UVI_UI_REPORT.md`, persistence `KSP_PERSISTENCE_FORMAT.md` in the named prior worktrees. The older semantics document's 2,741-file counts include recovery saves and are not this audit's denominator. UI_FRONTENDS contains historical “renderer pending/no v2 build” text; current source evidence supersedes that status.

The repo's machine-readable KSP inventory defines the parameter/command names used below. Expected units are compact contract descriptions, independently checked against the existing IR, v1 and the RE persistence spec; uncertain extension semantics are explicitly marked. Current NI references: [parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-parameters), [UI commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands), [callbacks](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks). Two particularly relevant confirmations: `set_ui_color` and `set_skin_offset` are legal outside init; scalar VALUE writes do not dispatch the widget's callback. The manual's slider sensitivity sign differs from v2's mapping.

## Measurements and reproduction

The master list contains **781 Kontakt NKI + 53 Kontakt NKM = 834 paths**, plus 660 UVI programs owned by another scope. The shared scanner owns corpus collection. The requested extension must visit all script slots, embedded multi programs and bank scripts. Columns below are **observed NKI / NKM file counts from the shared cache**: a multi with several matching programs is counted once. They are not counts of distinct embedded instruments. No snapshots appear in this master list; the prior persistence audit separately measures 1,103 NKSN files.

The single-instrument helper skips KSP comments and string literals, canonicalizes historical leading-underscore builtin aliases, and counts whole tokens, including inactive preprocessor branches and unreachable functions. It separates bypassed-script counters and traverses bank/program script records. Source hashes are ephemeral Rust `DefaultHasher` identities, not cryptographic proofs. No sources, saved values, picture/font/audio bytes or access data are exported; panic payloads are suppressed. The helper is available to the shared scanner owner; it has no corpus runner or cache implementation.

**Whole-corpus incidence remains incomplete.** The frozen incidence snapshot below uses scanner source `tools/kontra-scan@e340c39a6666752866015b5ee8f06a3cc978c416`; pinned v1 adapter is `788f41fafa7e21ddf7b1917bc4cf43e0a83876b8`. The 2026-10-08 08:58 UTC published snapshot contains **53 matched IDs: 50 Kontakt NKI and three UVI**, with zero NKM. This scope's coverage is **50/834 Kontakt paths**, with metadata and lexical counts measured for all 50: **250 raw slots, all decoded**. The v2 digest is `742c24e295358a7631fd5ae4fb85d576e51d03ef669eaa2efbf7ed2dfa6030c4`; v1 is `ac5aed734bb7fca40d6d000ff1f3e89128436b8f38d9d674fd70466f90dcdf08`. Other cache revisions and null-pick auditions are excluded from this frozen snapshot. The full paired sweep is owned by the census agent; no duplicate sweep or independent collector runs here.

The installed section J scanner is now `tools/kontra-scan@f7b2a8cdc6773ff569799c256ad9b0d638e3a824`, with pinned v1 adapter `11db26e72650f373465b6534d6ddb3684e1e9970`. Its v2 digest is `2c7c50d150ef168f46924dc4f90ea3edc2d96fa9350a299922858be2031237d0`; v1 is `bdbd24d642aaacf4511bdf8b717db14676d2ae46dc922ef13f24b9734c4a1438`. The full **1,494-ID paired sweep remains pending** after this revision change. Do not combine its partial cache with the older published TSV rows or the 08:58 incidence snapshot. The strict native saved-table parser is connected in scanner telemetry; this scanner-only change does not repair the frozen product's persistence implementation. UVI section J sidecar staging remains pending and is outside this scope.

Individual parameter/builtin/widget-declaration cells consume **`results/v2/symbol-aggregates.tsv`**, using `nki_active`/`nkm_active` with its declared coverage, metadata, lexical, and digest fields. Fix reach uses the union of embedded-slot symbol presence per ID from the corresponding current-revision detailed cache; owner/program/wire-slot identity is retained, bypassed symbols are separate. Nonzero cells remain lower bounds on whole-corpus exposure, not counts of broken or unlocked instruments. Widget declaration tokens remain distinct from initialized/rendered NCKP widget counts. Raw/admitted saved sigils and actual phase completion are reported separately.

The generated **246-name whitelist fixes all 29 previous omissions**. Four audit surfaces remain outside the pinned exact whitelist: `set_listener`, `change_listener_par`, `load_native_ui`, `load_komplete_ui`. Their cells remain Unknown and the delta was sent to the coordinator. Native/profile extension semantics remain uncertain even when a token is counted. `disabled_block_errors` means v1 compiler-disabled callback blocks, not preprocessor-inactive code. No-safe-key, fallback-note, mismatched-note and unknown phantom-free-control records cannot establish sound/control parity; `bound_typed` establishes model/readback admission, not live typed edits. The independent pilot remains excluded.

```sh
~/.cache/kontakto-heavy cargo build --locked -p sampler-kontakt --example ui_params_probe
~/.cache/kontakto-heavy /mnt/Windows11/DEV_WORKSPACE/Toolchains/User/cargo-target/kontakto-audit-ui-params/debug/examples/ui_params_probe --probe '/mnt/MAIN_STORAGE/Libraries/Kontakt/Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki'
# Shared scanners, one targeted item each; whole sweep belongs to the census owner.
~/.cache/kontakto-heavy ~/.cache/kontra-scan/bin/kontra-scan-v2 --list ~/.cache/kontra-scan/kontakt-items.tsv --start 0 --count 1 --out ~/.cache/kontakto-audit-ui-params/shared-conflux
~/.cache/kontakto-heavy ~/.cache/kontra-scan/bin/kontra-scan-v1 --list ~/.cache/kontra-scan/kontakt-items.tsv --start 0 --count 1 --out ~/.cache/kontakto-audit-ui-params/shared-conflux-v1
~/.cache/kontakto-heavy cargo test --locked --no-run -p sampler-ksp -p sampler-kontakt
~/.cache/kontakto-heavy cargo test --locked -p sampler-ksp --test ui_params_audit
~/.cache/kontakto-heavy cargo test --locked -p sampler-kontakt --example ui_params_probe
```

Validation: the two relevant packages pass `cargo test --no-run`; **6/6 authored audit probes and 1/1 token check pass**. Tests deliberately assert the observed defects, rather than falsely testing desired behavior as implemented. When fixing a defect, replace its expected result with the target behavior. No full root/plugin build, native-host comparison or rendered interaction test is claimed.


## Conflux first witness: facts within this scope

The 08:58 incidence-snapshot Conflux record uses v2 binary SHA-256 `742c24e295358a7631fd5ae4fb85d576e51d03ef669eaa2efbf7ed2dfa6030c4`. It reports **loads yes, audible audition yes, Original UI missing-images, 107/113 visible interactive bindings and one missing image**. The main page is 970×592 and nonuniform. Its declared background is **RGBA (240,239,228,255)**; **93.8985%** of pixels match that cream background within the scanner's tolerance. Its pure-white fraction is 0.0. The earlier pure-white metric alone does **not** rule out the user's predominantly plain, near-white appearance. The render scope must explain that appearance, the missing picture and actual paint/input behavior; scalar binding readback does not certify gestures.

| 08:58 Conflux extension measurement | Observed |
|---|---|
| Raw wire-slot partition | 5 total: 3 inline, 2 empty; 0 bypassed, linked-only or decode-failed |
| Empty source kinds | wire 0 absent; wire 1 whitespace-only |
| Executed runtime slots | wire/runtime 2, 3, 4; owner standalone-program, program 0 |
| Final compile admissions / clean flags | 3 / 3; “clean” does not assert full builtin fidelity or no warnings |
| Actual init completion | all 3 present callbacks completed |
| Actual persistence completion | both present callbacks in slots 2/3 completed; slot 4 absent |
| Load / note-time fault records | 0 / 0 observed in this matched run |
| Raw saved-table framing | decoded in all 5 raw slots |
| Raw saved-entry sigils | `$`:223, `%`:15, `@`:6, `!`:13; 257 total |
| Loader-admitted saved sigils | `$`:223, `%`:15, `@`:6; 244 total; `!`:0 |
| Source navigation / policy exposure | make_instr_persistent:16, set_snapshot_type:1, persistence_changed:2 references |
| All rendered view widgets | 411 + 22 + 1 = 434; distinct from source declaration-token counts |

The **13 raw string-array entries missing from admitted state** directly corroborate F6's production type omission. These are entry counts, not leaked values, distinct variables unlocked, or a successful lifecycle test. All Conflux init/persistence callbacks completing refutes fatal load-callback failure for this observed run, while F1/F2/F4's runtime semantic defects remain. Completion is taken from final runtime-preparation slot records; three import-harvest attempts and one dynamic-rack attempt are retained separately and are **not** added to the three runtime admissions. V1 retains all 13 string-array entries in admitted state; v2 retains none under the same raw-record identity. V1 main persistence callback is observed waiting, while its second callback completed; that is not a fault or proof it can never complete. V1 compacts wire slots 2/3/4 to runtime slots 0/1/2, whereas v2 retains runtime 2/3/4. Compare by wire ownership, not runtime index alone.

**08:58 matched single-item Conflux comparison** from the published pair:

| Measurement | v1 `0cb7a8a0` | v2 `7e82b152` |
|---|---:|---:|
| Pick / source | MIDI 60, velocity 64 / zone_coverage | MIDI 60, velocity 64 / zone_coverage |
| Load admitted / audible audition | yes / yes | yes / yes |
| Original UI classification | missing-images | missing-images |
| Visible scalar/ID bindings | 78 / 78 | 107 / 113 |
| Separate typed target admissions | 5 | 6 |
| Load time | 141.69 ms | 5,368.33 ms |
| Peak process RSS | 69.84 MB | 230.87 MB |
| Main authored widget inventory | 378 | 411 |
| Main missing pictures | 1 | 1 |
| Raw / admitted `!` entries | 13 / 13 | 13 / 0 |
| Actual init completions | 3 | 3 |
| Actual persistence completions / waiting | 1 / 1 | 2 / 0 |
| Phantom-free controls | unknown | unknown |

Both use the shared per-ID note plan; neither is a fallback audition. V1 measures an initial streaming bank and v2 includes asset metadata during load, per the shared README. Load/RSS are one-run corroboration for the performance scope, not a corpus benchmark. Different widget inventories and binding criteria prevent interpreting those fractions as matched gesture/native fidelity. In particular, typed admission does not repair F2/F10's live typed edit/callback gaps, and completion counts must preserve waiting versus fault status. The later UVI v1 sidecar uses a separate base and is excluded from this KSP comparison.

The **section J Conflux pair**, consumed from the shared detailed cache at 10:18 UTC using the new digests above, records the same MIDI 60/velocity 64 `zone_coverage` plan and matched audible auditions. Scalar bindings remain 78/78 versus 107/113, typed admissions 5 versus 6, and actual init/persistence completion counts remain 3/1 versus 3/2 (v1 also retains one waiting persistence callback).

| Section J single-item measurement | v1 | v2 |
|---|---:|---:|
| `load_ms` | 263.68 ms | 8,020.02 ms |
| `first_audio_ms` | 277.08 ms | 8,060.11 ms |
| `ui_first_frame_ms` | 903.43 ms | 8,064.81 ms |
| Product cache condition | cold | cold |
| Raw saved histogram completeness | unknown in all 5 slots | complete in all 5 slots |
| Raw saved entries | unknown | 257, including 13 `!` |
| Loader-admitted saved entries | 257, including 13 `!` | 244, with no `!` |

`first_audio_ms` starts at the first production program import and ends at the first finite, exactly nonzero output block; its criterion differs from the separate 1e-5 audible-audition threshold. `ui_first_frame_ms` ends at CPU Original-paint completion before image hashing/encoding. Audio and paint run concurrently in the isolated shared worker. Metadata prepass and worker startup are outside the clock; multi programs share the item clock. `load_ms` is unchanged and v1 includes its deferred initial sample-bank preload. `cold` describes product cache use, not the metrics cache or uncontrolled OS page cache. These are one-run observations for the performance owner, not native-host scheduling or corpus timing conclusions; absent onset measurements remain unknown.

The new v1 raw records explicitly report `saved_table_integrity=unknown`, `saved_entries_total=null` and `saved_histogram_complete=false` in all five slots, while three scripts compile/initialize and 257 entries enter loader state. Its flattened `saved_entry_sigils={}` and `slots_decode_failed=5` therefore establish neither an empty saved table nor five product script failures. This revision-specific scanner discrepancy was sent to the coordinator. The older raw v1 histogram above remains historical evidence; the new pair independently confirms v2's 13-entry string-array admission loss using its complete raw histogram and the v1 loader-admitted inventory.

The baseline main script is slot 2. It compiled in **1,869 ms**, producing **411 widgets, 313 scalar controls, 279 UI-control callbacks, four asset references and 220 unsupported property entries**. Slots 3 and 4 compiled in 12 ms and <1 ms, with 22 and one widgets, respectively; slot 3 has 13 callbacks. Compilation timing is one debug run and excludes container translation, queue wait, image resolution and rendering; asset metadata was intentionally omitted from `Script::ui` in this parameter probe.

Main-slot unsupported entries are NKS_TYPE 139, NKS_STYLE 32, NKS_NUM_VALUES 25, and indexed NKS_STR_VALUES 24. Runtime lowering reports 2,173 integer getter sites as Native, **16 string getter sites as Ignored**, 14 indexed integer getters as Native (the aliasing probe disproves faithful indexed semantics), 4 Native and 1,927 Host integer setter sites, 4,208 Host string setter sites and 1,533 Host indexed string setter sites. Also ignored: 21 menu-string getter sites and 86 menu-value getter sites. Host emission is not proof that the consumer applies the operation. The slot has two `set_text` sites; slot 3 adds three `set_text`, ten `move_control`, and two `move_control_px` sites, all emitted but unhandled by the baseline consumer.

This establishes declared/compiled control identity, not usable gestures. `sampler_ksp::compile_with` succeeds; neither “all scripts fail compilation” nor “all scalar controls are unbound” explains this witness. Parameter/callback fidelity remains incomplete even when controls bind. The white background and immovable-control root causes require the render/widgets/loop scopes' actual renderer and interaction traces; this scope does not claim to have reproduced those pixels or pointer behavior. In particular, UI feedback can remain wrong after a value moves because string getters and setter aliases are broken. The probe's 1,000 warning count is the compiler's warning cap, not an exhaustive count of unsupported runtime sites.

## Shared mechanisms: root cause, fix, and proof

Every matrix row points to these systemic fixes. Reach means observed files using the named surface: a lower bound on corpus exposure, not a count of proven broken instruments; **instruments fully unlocked is unknown until matched interaction/render tests pass**, because defects overlap.

| ID / severity | Root cause and baseline evidence | Systemic fix / effort | Smallest target proof |
|---|---|---|---|
| F1 / P0 | UI aliases emit Host effects but `apply_ui_effect` handles only `set_control_par`, `_real`, `_str`, `_arr`, `_str_arr` and some `set_key_*`: `sampler-ksp/src/lower.rs:2044`, `lib.rs:278`. `set_text`, `hide_part`, movement, menu changes, knob metadata and table steps are discarded. | Normalize aliases into the existing typed property/menu mutation mechanism before emission; consume each allowed service; **M**, `lower.rs`, `lib.rs`. Avoid implementing each alias in every renderer. | After one callback: new text, hidden label, moved panel, new menu item data, new default/unit, table step count. Existing audit alias probe emits 3 effects, applies 0. |
| F2 / P0 | `_arr` lowering uses the scalar PROPERTY_KEY `[id,par,tag,tag]`, no index (`lower.rs:312,2904,2928`); `lib.rs:741` seeds only scalar integer properties. Init indexed VALUE overlays IR cells but never writes table/XY VM cells (`eval.rs:886`, `ui.rs:360`). Real-array effects are not consumed (`lib.rs:307`). | One typed indexed key/value path shared by init/runtime; VALUE indexes resolve directly to typed widget storage; metadata keyed by index; **M**, `eval.rs`, `lower.rs`, `lib.rs`, `model.rs`. | Init cell 2 = 7; runtime read returns 7; writes 11/22 to indexes 1/2 read independently; actual table variable and IR agree. Current measured results are 0 and 22 for index 1. Add XY fractional VALUE and per-cursor image/automation/hide checks. |
| F3 / P0 | Persisted source values enter init, but current UI edits/global arrays/texts are absent from serializable Part/Selection: `src/plugin.rs:39,159,230,969`; persistent locations exist (`sampler-ksp/src/model.rs:164`) but no plugin KSP capture/restore consumer. | Capture typed scalar/array/text persistent locations off audio via existing bounded ownership handoff; persist source+slot+variable identity and restore after prepare; **M**, plugin state and core/script adapters. | Change a knob, menu semantic value, table, XY, text; save/reopen host state and `.kontra-multi`; restore exact values with no recursive UI callbacks. Separate source menu index from host semantic value. |
| F4 / P0 | Getter is a sparse mirror rather than current widget semantics: omitted dimensions return 0, IDENTIFIER returns numeric default text, SELECTED_ITEM_IDX never derives from menu; runtime string/real/menu getters return zero/empty (`eval.rs:589`, `lower.rs:1988,2087`). Modifier/index/mouse event state is not supplied by ControlContext. | Central parameter metadata: type, access/context, applicability, default/derived getter; supply interaction context; **M**, `builtins.rs`, `eval.rs`, `lower.rs`, core control admission. | Read stock dimensions before setters; declaration identifier without sigil; menu selection/index/count after mutation; exact string/real round trip; runtime dynamic TYPE; modifiers and array index from an actual edit. |
| F5 / P1 | `read_persistent_var` restores without consuming pending saved entry; end-of-init loop restores it again (`eval.rs:177,338,1132`). Persistence marking is static, including untaken branches, and misses function-body marking (`sema.rs:342,355`). | Consume pending entries on explicit read; mark persistence when executed in init, including called functions; **S/M**, evaluator/sema. Use RE lifecycle, not a library name. | Saved 7 → read → assign 9 → end-init remains 9 (currently 7); untaken mark does not persist; called init function does. Native pending-consumption reference trace still required. |
| F6 / P1 | Snapshot overlay ignores persistence kind; snapshot policy recorded as request but not retained/executed (`snapshot.rs:97`, `eval.rs:1125`). All recall rebuilds init and can overwrite instrument-only values. String arrays are dropped by `library.rs:1461`; malformed/bad-sigil records lack typed diagnostics. | Integrate prior typed persistence reader and declaration-aware scope/policy; preserve running state for policies 1/3; **M**, `library.rs`, `snapshot.rs`, `load.rs`, plugin snapshot lifecycle. | Four snapshot policies × persistent modes; snapshot missing entry keeps proper state; malformed entry fails explicitly; empty/newline string-array cells survive. |
| F7 / P1 | Recognized symbol, stored property and mapped UI meaning are different sets: only 45 CONTROL_PARS names are preinterned, more opaque names compile; MAPPED has no grid control parameters, state fonts, modifier/selection or richer waveform/XY fields (`builtins.rs:622`, `ui.rs:8,538`). Some metadata is incorrectly reported as unsupported visual meaning. | Explicit parameter table keyed by vendor profile, map each meaningful field or return a precise unsupported diagnostic; finish required neutral IR fields; **M/L**, builtin/UI IR adapters. Tag/custom/NKS metadata should remain typed metadata rather than a visual failure. | Exhaustive generated get/set/access/type tests for the named inventory, indexed variants and invalid-context rejection; census diagnostics distinguish metadata-only from discarded visible meaning. |
| F8 / P1 | Geometry defaults computed in renderer, unavailable to KSP; `auto_size = width missing OR height missing` makes renderer replace both axes (`ui.rs:423`, `src/ui/ir_view.rs:137`). `move_control` writes private grid tags while public GRID_* params are unsupported; mixed pixel/grid state never normalized. Parenting lookup is good but cyclic repair removes one edge only (`ui.rs:530,570`). | Resolve canonical geometry once per profile before getters and publication, preserve explicitly set axis, normalize public grid parameters and mode changes; validate complete parent graph; **M**, `ui.rs`, UI IR and layout adapter. | Width-only/height-only setup preserves supplied axis; grid and pixel setters have consistent readback; switching from move-grid to move-pixel clears old grid placement; nested parent offsets/visibility; reject multiple independent cycles with useful diagnostics. |
| F9 / P1 | Automation name/allowed/id is UI IR metadata (`ui.rs:498`); root `SamplerParams` exposes Volume and persisted Selection, not a dynamic KSP automation map (`src/plugin.rs:223`). Per-XY automation discarded with unsupported indexed metadata. | Bind stable vendor automation IDs to existing core controls and host gesture/parameter interfaces; **M**, plugin adapters and source control definitions. | Host enumerates each permitted control once by authored name/id; IDs stable across view/reload; disallowed controls absent; XY cursor mapping; host changes update scripts/UI and correct callback policy. |
| F10 / P1 | `on ui_control` admits scalar controls correctly but array/text/non-scalar widget edits have no public mutation/callback admission; `ui_controls` and `ui_update` rejected (`sema.rs:107`); event kind/column/cursor modifiers missing from control context (`sampler-core/src/control.rs:194`). | Extend existing ownership/admission to typed widget edits and callback families, carry source interaction fields, preserve same-sample ordering; **M/L**, sema/lower/core control and UI adapter. | Edit-before-callback, direct setters do not recurse, capacity rejection before mutation, waiting handler retains slot/plan, repeated and equal-time edits have native ordering, table/XY index, text/file payload and aggregate/periodic callbacks. |

Observed candidate reach (union of listed shared source-token mentions; no addition of overlapping counts; embedded slots only; bypassed slots excluded):

| Fix | Representative surface | NKI / NKM files | Fully unlocked |
|---|---|---:|---|
| F1 | `set_text`, `hide_part`, `move_control`… | >= 50 / 0 observed | Unmeasured |
| F2 | `set_control_par_arr`, `set_control_par_real_arr`, `set_control_par_str_arr`… | >= 2 / 0 observed | Unmeasured |
| F3 | `make_persistent`, `make_instr_persistent`… | >= 50 / 0 observed | Unmeasured |
| F4 | `get_control_par`, `get_control_par_str`, `get_control_par_real`… | >= 50 / 0 observed | Unmeasured |
| F5 | `read_persistent_var`… | >= 49 / 0 observed | Unmeasured |
| F6 | `make_instr_persistent`, `set_snapshot_type`… | >= 2 / 0 observed | Unmeasured |
| F7 | `set_control_par`, `set_control_par_str`, `set_control_par_arr`… | >= 50 / 0 observed | Unmeasured |
| F8 | `move_control`, `move_control_px`, `$CONTROL_PAR_POS_X`… | >= 50 / 0 observed | Unmeasured |
| F9 | `$CONTROL_PAR_ALLOW_AUTOMATION`, `$CONTROL_PAR_AUTOMATION_ID`, `$CONTROL_PAR_AUTOMATION_NAME`… | >= 50 / 0 observed | Unmeasured |
| F10 | `ui_control`, `ui_controls`, `ui_update`… | >= 50 / 0 observed | Unmeasured |

## Exhaustive parameter matrix

Names comprise the union of all `$CONTROL_PAR_*` identifiers in the pinned surface/symbol inventory and builtin adapter, plus the four NKS extension parameters observed in Conflux. The prefix-only `$CONTROL_PAR_` and `$CONTROL_PAR_WAVE_END_` are index patterns, not actual parameters, and excluded from the concrete count. “I” is integer, “S” string, “R” real; “idx” means indexed cursor/cell metadata. All getters/setters also inherit the shared matrix below: numeric explicitly set metadata can mirror; string/real/indexed getters at runtime are not thereby correct. `MAPPED` means only that IR has a field, not that painting, host automation or interaction uses it.

Evidence paths in tables omit the `crates/` prefix. `ksp/ui.rs` = `crates/sampler-ksp/src/ui.rs`, `ksp/eval.rs`, `ksp/lower.rs`, `ksp/lib.rs`, `ksp/model.rs` similarly. `ir/lib.rs` = `crates/sampler-ui-ir/src/lib.rs`. Files NKI/NKM are observed lower bounds from current-digest shared embedded-slot records; bypassed records are excluded. Unobserved or unwhitelisted names cannot support corpus-wide zero claims. Source tokens do not establish absence from NCKP/native UI/resources.

| Parameter | Expected type / units and behavior | v2 status, root-cause evidence → fix | Files NKI / NKM |
|---|---|---|---:|
| `$CONTROL_PAR_ACTIVE_INDEX` | I; active XY X-coordinate index or none | missing; opaque mirror, no active-cursor state; ui:538 → F2/F10 | Not observed / coverage incomplete |
| `$CONTROL_PAR_ALLOW_AUTOMATION` | I boolean, idx for XY; init applicability | partial; scalar IR flag, no host export/per-cursor path; ui:498 → F2/F9 | >= 50 / 0 observed |
| `$CONTROL_PAR_AUTOMATION_ID` | I; vendor host parameter ID, idx for XY | partial; scalar metadata, no host mapping or conflict/range policy; ui:498 → F2/F9 | >= 1 / 0 observed |
| `$CONTROL_PAR_AUTOMATION_NAME` | S; host name, idx for XY | partial; scalar IR name only; ui:498 → F2/F9 | >= 50 / 0 observed |
| `$CONTROL_PAR_BAR_COLOR` | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | >= 1 / 0 observed |
| `$CONTROL_PAR_BASEPATH` | S; file-selector root path | partial; IR field only; no browsing or path-boundary service; ui:408 / eval:1259 → F4/F7/F10 | >= 1 / 0 observed |
| `$CONTROL_PAR_BG_ALPHA` | I; opacity component | missing; unsupported independent alpha and gradient fields; ui:538 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_BG_COLOR` | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | >= 1 / 0 observed |
| `$CONTROL_PAR_COLUMN_WIDTH` | I; file-selector column pixels | partial; IR metadata only; ui:416 → F7/F10 | >= 1 / 0 observed |
| `$CONTROL_PAR_CURSOR_PICTURE` | S; asset name, XY cursor may be indexed | partial; scalar asset references emitted; typed getters and indexed cursor pictures missing; ui:508 / lib:307 → F2/F4; resolution: render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_CUSTOM_ID` | I; user metadata tag | partial; numeric store/readback works, IR labels metadata unsupported; ui:538 → F7 | >= 1 / 0 observed |
| `$CONTROL_PAR_DEFAULT_VALUE` | I; reset target in raw control units | partial; IR field mapped; missing defaults/readback not unified; ui:309 → F4/F1 | >= 50 / 0 observed |
| `$CONTROL_PAR_DISABLE_TEXT_SHIFTING` | I boolean; pressed caption shift policy | missing; unsupported property; ui:538 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_DND_ACCEPT_ARRAY` | I policy; accepted drop count/type | missing; no mouse-area drop service/context; ui:538 → F7/F10 | Not observed / coverage incomplete |
| `$CONTROL_PAR_DND_ACCEPT_AUDIO` | I policy; accepted drop count/type | missing; no mouse-area drop service/context; ui:538 → F7/F10 | Not observed / coverage incomplete |
| `$CONTROL_PAR_DND_ACCEPT_MIDI` | I policy; accepted drop count/type | missing; no mouse-area drop service/context; ui:538 → F7/F10 | Not observed / coverage incomplete |
| `$CONTROL_PAR_DND_BEHAVIOUR` | I; label MIDI export policy/area identity | missing; opaque unsupported metadata; ui:538 → F7/F10 | Not observed / coverage incomplete |
| `$CONTROL_PAR_FILEPATH` | S; selected file under basepath | missing; unsupported projection and no selected-file state; ui:538 / eval:1249 → F4/F10 | Not observed / coverage incomplete |
| `$CONTROL_PAR_FILE_TYPE` | I enum; selector filter | partial; IR filter mapped; no picker/callback service; ui:410 → F7/F10 | >= 1 / 0 observed |
| `$CONTROL_PAR_FONT_TYPE` | I; factory/custom font ID | partial; default/state font lookup and rendering incomplete; ui:504 → F7; render scope | >= 49 / 0 observed |
| `$CONTROL_PAR_FONT_TYPE_OFF_HOVER` | I; font ID for interaction state | missing; stored as unsupported, no state style projection; ui:538 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_FONT_TYPE_OFF_PRESSED` | I; font ID for interaction state | missing; stored as unsupported, no state style projection; ui:538 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_FONT_TYPE_ON` | I; font ID for interaction state | missing; stored as unsupported, no state style projection; ui:538 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_FONT_TYPE_ON_HOVER` | I; font ID for interaction state | missing; stored as unsupported, no state style projection; ui:538 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_FONT_TYPE_ON_PRESSED` | I; font ID for interaction state | missing; stored as unsupported, no state style projection; ui:538 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_GRID_HEIGHT` | I; grid position/size | missing; opaque store; private move_control tags differ; eval:963 / ui:8 → F8 | Not observed / coverage incomplete |
| `$CONTROL_PAR_GRID_WIDTH` | I; grid position/size | missing; opaque store; private move_control tags differ; eval:963 / ui:8 → F8 | Not observed / coverage incomplete |
| `$CONTROL_PAR_GRID_X` | I; grid position/size | missing; opaque store; private move_control tags differ; eval:963 / ui:8 → F8 | Not observed / coverage incomplete |
| `$CONTROL_PAR_GRID_Y` | I; grid position/size | missing; opaque store; private move_control tags differ; eval:963 / ui:8 → F8 | Not observed / coverage incomplete |
| `$CONTROL_PAR_HEIGHT` | I; pixel size | partial; omitted getter = 0; one missing axis resets both; ui:423 / ir_view:137 → F8 | >= 49 / 0 observed |
| `$CONTROL_PAR_HELP` | S; caption/lines/value label/help/short caption | partial; init scalar/textline overlay mapped; runtime getters empty; aliases discarded; eval:925 / ui:479 / lower:2087 → F1/F2/F4 | >= 49 / 0 observed |
| `$CONTROL_PAR_HIDE` | I mask; hide whole/parts or indexed XY cursor | partial; whole/inherited hide mapped; mod-light and indexed hide unavailable; ui:433,538 → F7/F10 | >= 50 / 0 observed |
| `$CONTROL_PAR_IDENTIFIER` | S read-only; declaration name without sigil | wrong; never synthesized from Widget.name; init getter returns "0", runtime ignored; eval:589,911 / lower:2087 → F4 | Not observed / coverage incomplete |
| `$CONTROL_PAR_KEY` | Uncertain historical/vendor extension; needs profile-specific reference | missing; opaque symbol, no semantic handler; ui:538 → F7; do not invent units | Not observed / coverage incomplete |
| `$CONTROL_PAR_KEY_ALT` | I read-only; interaction modifier snapshot | missing; no modifier fields in callback admission; control.rs:194 / lower:2920 → F4/F10 | >= 1 / 0 observed |
| `$CONTROL_PAR_KEY_CONTROL` | I read-only; interaction modifier snapshot | missing; no modifier fields in callback admission; control.rs:194 / lower:2920 → F4/F10 | >= 1 / 0 observed |
| `$CONTROL_PAR_KEY_SHIFT` | I read-only; interaction modifier snapshot | missing; no modifier fields in callback admission; control.rs:194 / lower:2920 → F4/F10 | >= 1 / 0 observed |
| `$CONTROL_PAR_LABEL` | S; caption/lines/value label/help/short caption | partial; init scalar/textline overlay mapped; runtime getters empty; aliases discarded; eval:925 / ui:479 / lower:2087 → F1/F2/F4 | >= 50 / 0 observed |
| `$CONTROL_PAR_MAX_VALUE` | I read-only; declared bounds | partial; declared getters seeded; illegal writes override IR but not core domain; eval:621 / lib:745 / ui:305 → F4/F7 | >= 1 / 0 observed |
| `$CONTROL_PAR_MIDI_EXPORT_AREA_IDX` | I; label MIDI export policy/area identity | missing; opaque unsupported metadata; ui:538 → F7/F10 | Not observed / coverage incomplete |
| `$CONTROL_PAR_MIN_VALUE` | I read-only; declared bounds | partial; declared getters seeded; illegal writes override IR but not core domain; eval:621 / lib:745 / ui:305 → F4/F7 | >= 1 / 0 observed |
| `$CONTROL_PAR_MOUSE_BEHAVIOUR` | I signed sensitivity; source gesture axis/travel | wrong; negative maps horizontal in IR; source/v1 slider semantics differ; ui:448 → F7; widgets scope | >= 49 / 0 observed |
| `$CONTROL_PAR_MOUSE_BEHAVIOUR_X` | I; XY axis sensitivities | partial; IR stores absolute magnitude, not full gesture/coordinate semantics; ui:388 → F2/F10 | Not observed / coverage incomplete |
| `$CONTROL_PAR_MOUSE_BEHAVIOUR_Y` | I; XY axis sensitivities | partial; IR stores absolute magnitude, not full gesture/coordinate semantics; ui:388 → F2/F10 | Not observed / coverage incomplete |
| `$CONTROL_PAR_MOUSE_MODE` | I enum; XY click/drag policy | partial; IR field only, no typed XY input admission; ui:392 → F10 | Not observed / coverage incomplete |
| `$CONTROL_PAR_NKS_NUM_VALUES` | Uncertain vendor extension; resolve its legal type/index/context | missing; interned/stored but unsupported semantic projection; ui:538 → F7 | >= 1 / 0 observed |
| `$CONTROL_PAR_NKS_STR_VALUES` | Uncertain vendor extension; resolve its legal type/index/context | missing; interned/stored but unsupported semantic projection; ui:538 → F7 | >= 1 / 0 observed |
| `$CONTROL_PAR_NKS_STYLE` | Uncertain vendor extension; resolve its legal type/index/context | missing; interned/stored but unsupported semantic projection; ui:538 → F7 | >= 1 / 0 observed |
| `$CONTROL_PAR_NKS_TYPE` | Uncertain vendor extension; resolve its legal type/index/context | missing; interned/stored but unsupported semantic projection; ui:538 → F7 | >= 1 / 0 observed |
| `$CONTROL_PAR_NONE` | I sentinel; no operation | wrong; generic stores and unsupported emission instead of no-op; eval:567 / ui:538 → F7 | Not observed / coverage incomplete |
| `$CONTROL_PAR_NUM_ITEMS` | I read-only; current menu item count | partial; init computed, runtime not seeded/derived; eval:600 / lower:2928 → F4 | >= 1 / 0 observed |
| `$CONTROL_PAR_OFF_COLOR` | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | >= 1 / 0 observed |
| `$CONTROL_PAR_ON_COLOR` | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | >= 1 / 0 observed |
| `$CONTROL_PAR_OVERLOAD_COLOR` | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | Not observed / coverage incomplete |
| `$CONTROL_PAR_PARALLAX_X` | I; wavetable view displacement | partial; IR retains integer pair, no visualization; ui:397 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_PARALLAX_Y` | I; wavetable view displacement | partial; IR retains integer pair, no visualization; ui:397 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_PARENT_PANEL` | I panel UI ID; child local geometry/visibility | partial; valid lookup/nesting correct; default detach and cycle cases unverified; ui:530 / ir:557,567 → F8 | Not observed / coverage incomplete |
| `$CONTROL_PAR_PEAK_COLOR` | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | >= 1 / 0 observed |
| `$CONTROL_PAR_PICTURE` | S; asset name, XY cursor may be indexed | partial; scalar asset references emitted; typed getters and indexed cursor pictures missing; ui:508 / lib:307 → F2/F4; resolution: render scope | >= 50 / 0 observed |
| `$CONTROL_PAR_PICTURE_STATE` | I; explicit picture frame where supported | partial; scalar frame mapped, applicability/state behavior not enforced; ui:522 → F7; paint states: render scope | >= 49 / 0 observed |
| `$CONTROL_PAR_POS_X` | I; local pixel position | partial; explicit mirror/rect, absent defaults and grid precedence differ; ui:423 → F4/F8 | >= 50 / 0 observed |
| `$CONTROL_PAR_POS_Y` | I; local pixel position | partial; explicit mirror/rect, absent defaults and grid precedence differ; ui:423 → F4/F8 | >= 50 / 0 observed |
| `$CONTROL_PAR_RANGE_MAX` | I; level-meter display bounds | missing; no meter-range field in IR kind; ui:401,538 → F7 | Not observed / coverage incomplete |
| `$CONTROL_PAR_RANGE_MIN` | I; level-meter display bounds | missing; no meter-range field in IR kind; ui:401,538 → F7 | Not observed / coverage incomplete |
| `$CONTROL_PAR_RECEIVE_DRAG_EVENTS` | I boolean; drag vs drop callback policy | missing; no source event payload; ui:538 / control.rs:194 → F10 | Not observed / coverage incomplete |
| `$CONTROL_PAR_SELECTED_ITEM_IDX` | I; menu selected position | wrong; generic metadata independent of semantic menu value; eval:589 / ui:538 → F4 | Not observed / coverage incomplete |
| `$CONTROL_PAR_SHORT_NAME` | S; caption/lines/value label/help/short caption | partial; init scalar/textline overlay mapped; runtime getters empty; aliases discarded; eval:925 / ui:479 / lower:2087 → F1/F2/F4 | >= 1 / 0 observed |
| `$CONTROL_PAR_SHOW_ARROWS` | I boolean; value-edit arrow visibility | partial; IR boolean, value edit lacks full native interaction; ui:353 → F7; widgets scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_SLICEMARKERS_COLOR` | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | Not observed / coverage incomplete |
| `$CONTROL_PAR_TEXT` | S; caption/lines/value label/help/short caption | partial; init scalar/textline overlay mapped; runtime getters empty; aliases discarded; eval:925 / ui:479 / lower:2087 → F1/F2/F4 | >= 50 / 0 observed |
| `$CONTROL_PAR_TEXTLINE` | S; caption/lines/value label/help/short caption | partial; init scalar/textline overlay mapped; runtime getters empty; aliases discarded; eval:925 / ui:479 / lower:2087 → F1/F2/F4 | >= 2 / 0 observed |
| `$CONTROL_PAR_TEXTPOS_Y` | I; caption/value vertical pixel offset | partial for TEXTPOS_Y, missing VALUEPOS_Y; ui:446,538; renderer does not consume offset → F7; render scope | >= 49 / 0 observed |
| `$CONTROL_PAR_TEXT_ALIGNMENT` | I; horizontal text alignment | wrong when set alone; style only created if FONT_TYPE exists; ui:504; audit alignment probe → F7 | >= 50 / 0 observed |
| `$CONTROL_PAR_TYPE` | I read-only; vendor control type | partial; static UI lookup correct; dynamic ID sees sparse mirror default; eval:595 / lower:2914 → F4 | Not observed / coverage incomplete |
| `$CONTROL_PAR_UNIT` | I enum; native display unit | partial; integer unit translated to string; invalid enum/context unchecked; eval:948 / ui:218,319 → F1/F7 | Not observed / coverage incomplete |
| `$CONTROL_PAR_VALUE` | I scalar, I table cells or R XY coordinates; no recursive callback | partial; scalar values native; indexed table/XY VM and presentation diverge; eval:567,886 / lower:2874 → F2/F10 | >= 50 / 0 observed |
| `$CONTROL_PAR_VALUEPOS_Y` | I; caption/value vertical pixel offset | partial for TEXTPOS_Y, missing VALUEPOS_Y; ui:446,538; renderer does not consume offset → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_VERTICAL` | I boolean; meter orientation | partial; IR mapped; no live meter source; ui:401 / ui:598 → F7/F10 | >= 1 / 0 observed |
| `$CONTROL_PAR_WAVETABLE` | Uncertain historical/vendor extension; needs profile-specific reference | missing; opaque symbol, no semantic handler; ui:538 → F7; do not invent units | Not observed / coverage incomplete |
| `$CONTROL_PAR_WAVETABLE_ALPHA` | I; opacity component | missing; unsupported independent alpha and gradient fields; ui:538 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_WAVETABLE_COLOR` | I RGB; wavetable/gradient end color | missing; unsupported style fields; ui:538 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_WAVETABLE_END_ALPHA` | I; opacity component | missing; unsupported independent alpha and gradient fields; ui:538 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_WAVETABLE_END_COLOR` | I RGB; wavetable/gradient end color | missing; unsupported style fields; ui:538 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_WAVE_ALPHA` | I; opacity component | missing; unsupported independent alpha and gradient fields; ui:538 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_WAVE_COLOR` | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | Not observed / coverage incomplete |
| `$CONTROL_PAR_WAVE_CURSOR_COLOR` | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | Not observed / coverage incomplete |
| `$CONTROL_PAR_WAVE_END_ALPHA` | I; opacity component | missing; unsupported independent alpha and gradient fields; ui:538 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_WAVE_END_COLOR` | I RGB; wavetable/gradient end color | missing; unsupported style fields; ui:538 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_WF_VIS_MODE` | I enum; waveform display mode | missing; Kind::Waveform contains no mode; ui:394,538 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_WIDTH` | I; pixel size | partial; omitted getter = 0; one missing axis resets both; ui:423 / ir_view:137 → F8 | >= 50 / 0 observed |
| `$CONTROL_PAR_WT_VIS_MODE` | I enum; wavetable visualization | partial; metadata retained, no real source/display; ui:396 → F7; render scope | Not observed / coverage incomplete |
| `$CONTROL_PAR_WT_ZONE` | I; attached source-zone ID | missing; opaque unsupported property; ui:538 → F7/F10 | Not observed / coverage incomplete |
| `$CONTROL_PAR_ZERO_LINE_COLOR` | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | >= 1 / 0 observed |
| `$CONTROL_PAR_Z_LAYER` | I; layer then vendor widget/declaration order | partial; IR stores z; draw_order only sorts sibling z/declaration, missing widget priority; ui:432 / ir:583 → F7; render scope | >= 48 / 0 observed |

**94 concrete parameter names enumerated.** Extension rows do not claim undocumented NKS semantics. `CONTROL_PAR_*` names can be interned opaquely; absence from the fixed 45-name list is not a parse failure.

### Get/set variants and identity (applies to every parameter above)

| API | Init → runtime status | Root cause / evidence | Fix / target proof | Files NKI / NKM |
|---|---|---|---|---:|
| `get_ui_id` | correct for declared widgets and slot-local arithmetic | ksp/eval:880; lower:1980; FIRST_UI_ID=32768; NCKP tree order | Retain separate source IDs and u128 semantic IDs; test multiple slots and NCKP nesting | >= 50 / 0 observed |
| `set_control_par` | partial; scalar integer metadata/value | ksp/eval:567; lower:2874; lib:307; missing legality/schema | F1/F4/F7; read/write every legal numeric parameter | >= 50 / 0 observed |
| `set_control_par_str` | partial; init strings and direct runtime effects | ksp/eval:881; lib:313; typed getter missing and effect text bounded | F4/F7; caption/path/name roundtrip and long Unicode | >= 50 / 0 observed |
| `set_control_par_real` | wrong for fractional metadata at init; runtime effect scalar only | ksp/eval:580 casts non-string v.int(); lib:309 preserves runtime IEEE bits | F2/F4; distinguish legal real VALUE from unsupported real metadata | Not observed / coverage incomplete |
| `set_control_par_arr` | wrong; init separate map, runtime scalar mirror | ksp/eval:886; lower:2904; lib:314 | F2; independent indexes and actual variable storage | >= 2 / 0 observed |
| `set_control_par_str_arr` | partial; init textline map, runtime effect inserted | ksp/eval:895 capped at 65536 lines; lib:315; indexed images/automation not projected | F2/F7; per-cursor strings and virtualized multiline policy | >= 1 / 0 observed |
| `set_control_par_real_arr` | wrong; values retained only as indexed properties at init; runtime effect unhandled | ksp/eval:886; lower:2874; lib:307 lacks case | F2; two cursor coordinates with no scalar/index alias | Not observed / coverage incomplete |
| `get_control_par` | partial; static VALUE/TYPE native and sparse int readback | ksp/eval:589; lower:2910; default/derived/input fields absent | F4; dynamic TYPE, defaults, menu and interaction state | >= 50 / 0 observed |
| `get_control_par_str` | wrong; init only reads stored property or converts 0; runtime ignored | ksp/eval:910; lower:2087 | F4; actual captions plus generated declaration identifier | >= 1 / 0 observed |
| `get_control_par_real` | wrong; init int property converted to real; runtime ignored | ksp/eval:902; lower:2087 | F2/F4; legal fractional value roundtrip | Not observed / coverage incomplete |
| `get_control_par_arr` | wrong; init map lookup; runtime omits index and initial indexed store | ksp/eval:914; lower:2928; lib:741 | F2; seeded indexed get, no cross-index overwrite | >= 2 / 0 observed |
| `get_control_par_str_arr` | partial init map only; runtime ignored | ksp/eval:914; lower:2087 | F2/F4; current per-cursor/name/textline strings | Not observed / coverage incomplete |
| `get_control_par_real_arr` | partial init map only; runtime ignored | ksp/eval:914; lower:2087 | F2/F4; current XY VM coordinate after typed edit | Not observed / coverage incomplete |

Root data disagreement: setting TABLE VALUE through `_arr` changes the indexed property/IR overlay, not `%table`. Conversely script assignment to `%table` changes VM cells, not the immutable presentation's stored table values. XY coordinates have the same missing projection. The scalar interface's atomic numeric readback does not cover arrays, text or dynamic menu state. Invalid UI ID/type/parameter/applicability is not consistently rejected; unknown-property acceptance must never be mistaken for support. Core native scalar edit admission has correct type/range/stale-generation checks, which should be reused.

## Exhaustive UI-builtin matrix

All pinned User Interface Commands plus typed getter/setter flavors above, current frontend commands and persistence/keyboard-related services are enumerated. Init-only calls being ignored later is correct only where the vendor actually restricts the call. Aliases must share the same live state as their generic counterpart.

| Builtin | Expected result / timing | v2 status and evidence → fix | Files NKI / NKM |
|---|---|---|---:|
| `add_menu_item` | append ordered text + semantic value | ksp/partial init correct; runtime emitted, unhandled; eval:980 / lib:307 → F1/F4 | >= 50 / 0 observed |
| `add_text_line` | append label line | ksp/partial init concatenates; runtime discarded; eval:925 / lib:307 → F1 | >= 22 / 0 observed |
| `attach_level_meter` | bind meter to group/slot/channel/bus source | ksp/partial request/IR retains bus+channel, not complete source; ui:598 → F7/F10 | >= 2 / 0 observed |
| `attach_zone` | bind waveform to zone and flags | ksp/missing service; init request/runtime Host only; eval:1259 / lib:307 → F7/F10 | Not observed / coverage incomplete |
| `expose_controls` | expose declared identifiers across slots to Komplete UI | ksp/missing; init no-op, no exported registry; eval:1125 → F7/F10 | >= 1 / 0 observed |
| `fs_get_filename` | selected filename/path from file callback | ksp/missing; empty string/ignored; eval:1249 / lower:2087 → F4/F10 | >= 1 / 0 observed |
| `fs_navigate` | select neighboring file and invoke its handler | ksp/missing; Host effect discarded; lower:2065 / lib:307 → F10 | >= 1 / 0 observed |
| `get_font_id` | resource font name to font ID | ksp/partial init registers font name, not full font selection; eval:1053 / ui:504 → F7; render | Not observed / coverage incomplete |
| `get_menu_item_str` | current item caption by index | ksp/partial init correct; runtime ignored; eval:1007 / lower:2087 → F4 | >= 1 / 0 observed |
| `get_menu_item_value` | current semantic item value by index | ksp/partial init correct; runtime ignored; eval:1007 / lower:2087 → F4 | >= 1 / 0 observed |
| `get_menu_item_visibility` | current per-item visibility | ksp/partial init correct; runtime ignored; eval:1007 / lower:2087 → F4 | Not observed / coverage incomplete |
| `get_num_menu_items` | current item count (historical helper) | ksp/partial init correct; runtime ignored; eval:1018 / lower:2087 → F4 | Not observed / coverage incomplete |
| `get_ui_wf_property` | waveform cursor/flags/indexed slice state | ksp/missing; returns 0; eval:1252 / lower:2087 → F7/F10 | Not observed / coverage incomplete |
| `hide_part` | visibility mask immediately updates widget | ksp/partial init; runtime discarded; eval:946 / lib:307 → F1 | >= 1 / 0 observed |
| `load_performance_view` | init .nckp once per slot, widget declaration tree | ksp/partial literal pre-scan, partial type IDs and hierarchy; nckp:17,85 / load:583 → F7/F8 | >= 1 / 0 observed |
| `make_perfview` | init activates authored performance page | ksp/correct activation subset; conflict with NCKP not enforced; eval:1049 → F7 | >= 49 / 0 observed |
| `move_control` | grid placement, (0,0) hidden; all callbacks | ksp/partial private grid tags at init; runtime discarded; eval:963 / ui:436 → F1/F8 | >= 2 / 0 observed |
| `move_control_px` | local pixel placement; all callbacks | ksp/partial init pixel props; stale grid tags remain; runtime discarded; eval:963 → F1/F8 | >= 49 / 0 observed |
| `set_control_help` | tooltip content | ksp/partial init correct; runtime discarded; eval:925 → F1 | >= 50 / 0 observed |
| `set_knob_defval` | raw reset value | ksp/partial init property; runtime discarded; eval:946 → F1/F4 | Not observed / coverage incomplete |
| `set_knob_label` | formatted value text | ksp/partial init property; runtime discarded; eval:925 → F1 | >= 2 / 0 observed |
| `set_knob_unit` | display unit enum | ksp/partial init property; runtime discarded; eval:946 → F1 | >= 2 / 0 observed |
| `set_menu_item_str` | mutate existing item caption | ksp/partial init correct; runtime discarded; eval:988 → F1/F4 | >= 2 / 0 observed |
| `set_menu_item_value` | mutate existing semantic item value | ksp/partial init correct; runtime discarded; eval:988 → F1/F4 | Not observed / coverage incomplete |
| `set_menu_item_visibility` | mutate item visibility; selected hidden item contract | ksp/partial init data; runtime discarded; eval:988 → F1/F4 | >= 2 / 0 observed |
| `set_table_steps_shown` | display window/step count | ksp/partial init mapped; runtime discarded; eval:946 / ui:383 → F1/F2 | >= 2 / 0 observed |
| `set_script_title` | slot/page title, init | ksp/partial retained; native display/context limits unverified; eval:1035 → F7 | >= 50 / 0 observed |
| `set_skin_offset` | wallpaper crop/scroll pixels; runtime legal | ksp/wrong after init; lower calls init-only; eval:1019 / lower:2096 → F1; render | >= 49 / 0 observed |
| `set_text` | replace label content or widget caption | ksp/partial init; runtime Host effect discarded; eval:925 / lib:307 → F1 | >= 50 / 0 observed |
| `set_ui_color` | performance background color; runtime legal | ksp/wrong after init; lower calls init-only; eval:1125 / lower:2096 → F1 | >= 49 / 0 observed |
| `set_ui_height` | init view height in grid rows | ksp/partial retained; invalid value policy not enforced; eval:1023 / ui:280 → F7/F8 | >= 1 / 0 observed |
| `set_ui_height_px` | init view height in pixels | ksp/partial retained; invalid range/default/header semantics; eval:1027 / ui:286 → F7/F8; render | >= 50 / 0 observed |
| `set_ui_width_px` | init view width in pixels | ksp/partial retained; invalid range policy unchecked; eval:1031 / ui:291 → F7/F8 | >= 50 / 0 observed |
| `set_ui_wf_property` | waveform cursor/flags/slice state | ksp/missing; init request/runtime Host discarded; eval:1259 / lib:307 → F7/F10 | Not observed / coverage incomplete |
| `load_native_ui` | vendor UI frontend load by profile | ksp/missing runtime/front-end execution; init request only; eval:1259 → F7; render/loop | Unknown (not whitelisted) |
| `load_komplete_ui` | Komplete Script package/profile load | ksp/missing baseline command; builtins.rs from_name has no alias; newer UI branch identifies unavailable frontend → F7 | Unknown (not whitelisted) |
| `set_nks_nav_name` | NKS navigation metadata name | ksp/missing; Host request emitted, not consumed; eval:1259 / lib:307 → F7 | >= 1 / 0 observed |
| `set_nks_nav_par` | NKS navigation parameter metadata | ksp/missing; Host request emitted, not consumed; eval:1259 / lib:307 → F7 | >= 1 / 0 observed |
| `reset_nks_nav` | reset NKS navigation metadata | ksp/missing; Host request emitted, not consumed; eval:1259 / lib:307 → F7 | >= 1 / 0 observed |
| `make_persistent` | executed declaration flag: instrument + snapshots | ksp/partial static flag; current-state save path absent; sema:355 / model:164 → F3/F5 | >= 50 / 0 observed |
| `make_instr_persistent` | executed declaration flag: instrument only | ksp/partial static flag; recall ignores exclusion; sema:355 / snapshot:97 → F3/F5/F6 | >= 2 / 0 observed |
| `read_persistent_var` | immediate pending saved restore, then consume entry | ksp/wrong duplicate restoration; no persistence-kind check; eval:338,1132 → F5 | >= 49 / 0 observed |
| `set_snapshot_type` | four-valued recall/native-save policy across slots | ksp/missing host policy; init request retained only; eval:1125 / snapshot:97 → F6 | >= 2 / 0 observed |
| `set_listener` | configure listener signal/period; UI scripts may refresh through on listener | ksp/partial timer init model + runtime period store; only init-configured timer signals start drivers; beat driver hardcodes 120 BPM, idle polling hardcodes 480 frames; eval:1202 / lib:688 / lower:198,1966 → F10; use host tempo/rate and measure scheduling | Unknown (not whitelisted) |
| `change_listener_par` | change an existing listener period while callbacks run | ksp/partial runtime period store; stop/restart and wait ordering need differential timing proof; lower:1966 / lib:688 → F10 | Unknown (not whitelisted) |
| `set_key_color` | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | >= 50 / 0 observed |
| `set_key_type` | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | >= 50 / 0 observed |
| `set_key_pressed` | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | >= 50 / 0 observed |
| `set_key_name` | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | >= 48 / 0 observed |
| `set_key_pressed_support` | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | >= 49 / 0 observed |
| `set_keyrange` | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | Not observed / coverage incomplete |
| `remove_keyrange` | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | Not observed / coverage incomplete |
| `get_key_color` | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | Not observed / coverage incomplete |
| `get_key_type` | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | Not observed / coverage incomplete |
| `get_key_triggerstate` | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | Not observed / coverage incomplete |
| `get_key_name` | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | Not observed / coverage incomplete |
| `get_keyrange_min_note` | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | Not observed / coverage incomplete |
| `get_keyrange_max_note` | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | Not observed / coverage incomplete |
| `get_keyrange_name` | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | Not observed / coverage incomplete |

## Widget-specific parameter and persistence obligations

This table covers the parameter/value/callback boundary of **all 16 UI types**; the widgets auditor owns pointer/key/wheel/drop rendering. Native saved-variable records contain values, not arbitrary serialized picture/font/parent properties. Rebuild those properties from init/persistence callbacks.

| Widget | Value / source persistence | v2 parameter and callback status | Files NKI / NKM |
|---|---|---|---:|
| `ui_knob` | I; bounded raw scalar | partial; scalar control + callback works; display/default/automation/getter gaps; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | >= 2 / 0 observed |
| `ui_slider` | I; bounded raw scalar | partial; scalar callback works; source axis sign/sensitivity projection wrong; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | >= 50 / 0 observed |
| `ui_button` | I; 0/1 | partial; scalar callback native; caption/menu/visibility feedback can be lost; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | >= 49 / 0 observed |
| `ui_switch` | I; 0/1 | partial; scalar callback native; state fonts/pressed behavior absent; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | >= 50 / 0 observed |
| `ui_menu` | I semantic value; native file stores selected position | partial; restore maps existing item index; invalid/early indexes fall to raw value; live getter/index/item updates missing; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | >= 50 / 0 observed |
| `ui_table` | all I cells, not ordinary-array tail compression | wrong `_arr`/VM/IR coherence; Binding::Variable has no public UI cell edit callback service; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | >= 1 / 0 observed |
| `ui_xy` | all R coordinate pairs, not ordinary-array tail compression | missing typed edit; sensitivity/mode only metadata; per-cursor property and readback gaps; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | Not observed / coverage incomplete |
| `ui_waveform` | native saved bounded I base state; not audio/zone/cursor serialization | partial declaration; attachment/getter/runtime cursor service missing; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | Not observed / coverage incomplete |
| `ui_wavetable` | native saved bounded I base state; not wavetable asset serialization | partial declaration; zone/mode/color/source service missing; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | Not observed / coverage incomplete |
| `ui_file_selector` | no native saved-variable serializer; persist separate path | missing selected-file state/context; BASEPATH/FILE_TYPE metadata only; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | >= 1 / 0 observed |
| `ui_level_meter` | no native saved-variable serializer; live source state | partial colors/orientation; source attachment/ranges/getters absent; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | >= 1 / 0 observed |
| `ui_value_edit` | I bounded raw scalar | partial scalar callback native; arrows/value offset and display editing incomplete; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | >= 50 / 0 observed |
| `ui_label` | no native saved-variable serializer; rebuild text from other vars | partial caption/indexed text projection; runtime aliases absent, no live string getter; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | >= 49 / 0 observed |
| `ui_text_edit` | S current bytes | partial restored text model; no public UI text edit/callback service; IR editor placeholder; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | >= 1 / 0 observed |
| `ui_panel` | no native saved-variable serializer; no scalar musical value | partial parent offsets + inherited hide correct; full geometry/cycles/Z policy incomplete; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | Not observed / coverage incomplete |
| `ui_mouse_area` | no native saved-variable serializer; source mouse/drop event | missing event fields/payload/drag policy; declaration/outline is not behavior; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | Not observed / coverage incomplete |

## Persistence and callback ordering matrix

| Mechanism | Expected ordering/meaning | v2 status / evidence | Systemic fix / proving test |
|---|---|---|---|
| Source namespace | Saved name includes sigil, scoped by program/script slot | correct for current translated behaviors; slot retained in load environment (`library.rs:207`, `load.rs:534`); bank-script import is separate format obligation | Preserve namespace through all capture/recall; equal names in two slots/programs remain independent |
| Init → restore → persistence_changed → display | Defaults/menu creation precede remaining saved values; derived UI rebuilt before display | correct broad ordering (`eval.rs:164–196`); saved arrays tail filling now implemented (`eval.rs:361`), older doc is stale | F5/F6; menu with nonidentity values, compressed ordinary array, full table/XY, display from restored state |
| Immediate read | Saved entry applied once at call point; subsequent init assignments survive | wrong, measured 7 not 9; pending consumption is RE-derived, live reference unknown | F5; differential Kontakt trace on authored fixture |
| Menu native vs host state | NKI decimal is item position; host working state is semantic item value | correct native conversion when item exists (`eval.rs:352`); invalid position and read before menu construction unresolved; host capture missing | F3/F5; native index 2→value 80, later host restore 80→same item, no second conversion |
| Ordinary numeric array tails | Saved prefix extends with last token to declared dimension | correct existing path for Home::Cells; tables/XY separate serializer contract needs declaration-aware boundary | F6; [7,-3,0]→[7,-3,0,0,0,0], short malformed UI table distinct |
| Scalar strings | Split once; preserve spaces/newlines | mostly correct production `saved()` for `@`; byte validity lossy vendor boundary | Typed prior reader + tests for spaces/CR/invalid encoding, no payload logs |
| String arrays | LF-separated cells, including empty cells and final terminator | missing in production Saved enum/env; source table `!` dropped | F6; integrate typed parser and extend owned typed IR state |
| Persistent modes | instrument includes both; snapshot excludes instrument-only | wrong exclusion/application; model records modes but snapshot overlay does not check them | F3/F6; cross-slot policies and mode combinations |
| Snapshot types 0/1/2/3 | init yes/no × native state yes/no; remaining state then callback | missing recalled running-script policy; always reconstruction path | F6; verify all four without treating container flags as whole policy |
| Programmatic VALUE | write value, do not call UI handler recursively | correct scalar path; `write_var`/native WriteControl uses plain edit (`lower.rs:2874`, `core/control.rs:351`) | Retain; two controls whose handlers write one another execute only originating handler |
| User edit | due work settles, admission preflight, value commit, then handler | correct scalar native ownership (`core/control.rs:194–228`), app adapter queued; widgets that never emit typed edits cannot use it | F10; current value visible at handler entry; failed capacity leaves it unchanged |
| Handler waits | originating plan/slot/context retained; same-time queue order | correct native substrate, existing CONTROL_STATE checks; actual UI event timing not measured here | Retain; change plan during waiting handler, equal-time edits/resume and stale reply test |
| Duplicate ui_control handlers | current resolver keeps later definition | implemented with warning (`sema.rs:129`); vendor/profile exactness not differentially measured | Reference host test before calling fully correct |
| on ui_controls | aggregate UI edit/callback family | missing: unsupported callback (`sema.rs:107`) | F10; native trace defines grouping, value ordering and interaction event payload |
| on listener | configured timer callbacks, unit-specific period and start/stop policy | partial existing timer drivers for init-configured signals (`lib.rs:688`); runtime period mirror (`lower.rs:1966`), beat cadence hardcodes 120 BPM and idle poll hardcodes 480 frames (`lower.rs:198`); initial signal admission unverified | F10; use host transport/rate; UI refresh fixture tests enable/change/stop/restart, waits, same-time control edits |
| on ui_update | source periodic UI callback | missing: unsupported callback (`sema.rs:107`) | F10; reference trace defines timer interval and suspension behavior; no invented cadence |
| Per-edit metadata | modifiers, table/cursor index, mouse kind/inside state and file/drop payload belong to originating event | missing ControlContext fields; no typed edit admission for array/text/file widgets | F4/F10; overlapping callbacks retain their own metadata across waits |
| Presentation publication | mutated script state reaches UI after queued effects; plain scalar revision separate | partial generic setters update models; unrecognized aliases discarded; arrays/text lack live readback (`lib.rs:278`, `plugin.rs:892`) | F1/F2; ordered set/get/edit trace and published IR agree; timing/diff cost belongs to loop scope |

## Prior work: reuse and merge boundaries

- `origin/v2/gpt-decipher-persist@6ae158204b2972fd756a868652d0f68151249cb9` contains strict `SavedEntry`/`SavedValue`, menu position and array-tail distinctions, string arrays, borrowed source offsets and a corpus spec. Decoder implementation commit `847d4670`. **Available to merge; not connected to production `library::saved`/snapshot/runtime restoration.** Merge the decoder, then perform F5/F6 integration; do not claim the merge alone repairs recalled state. Its 810,541-entry large-tree denominator includes recovery saves and cannot be divided by 834.
- `origin/v2/gpt-kontakt-ui@0be9ed3f174d8925f1d8a142d3f709e5e26b840a` adds corpus tooling, scalar recall/passive rendering repair (`47f14b59`), NICNT routing (`bd7447db`), bounded library lookup and unavailable Komplete frontend detection (`7591f35e`, `0be9ed3f`). Its `Environment.control_values` override can repair host scalar recall without treating current menu values as native saved positions. These changes do not implement typed/indexed readback or every Host setter. **Merge selectively:** its older `restore` implementation removes the baseline ordinary-array repeated-tail fill; preserve the newer tail behavior while reusing the host-state override. The local `UI_V1_PARITY.md` read for this audit is prior-agent working-tree evidence, not a file in that fetched branch tip; verify its later commits with its owner before merging.
- `origin/v2/gpt-ksp-audit@a1446ec6be0162371948cef412dc9e9d0383e6fe` is a behavioral audit, not a parameter implementation. Its old snapshot/persistence observations partly overlap F5/F6; our new probes verify the current baseline rather than repeating its older line numbers/counts.
- UVI `UVI_UI_REPORT.md` describes typed Lua widgets, bounded callback edits and separate widget/custom state capture, useful shared-service examples. It is not a KSP implementation and its 660-program measurements are excluded here. Its runtime/profile and callback restoration rules must not be transplanted into KSP.
- V1 `0cb7a8a0` already implements UI aliases, live menu data, defaults and typed VALUE operations in `src/ksp/calls.rs:1166–1420`; `src/ksp/ui.rs:273` seeds stock geometry. Keep its semantics as evidence; reuse v2 ownership primitives, not the old laggy publication architecture. V1 real non-VALUE metadata was itself unavailable (`calls.rs:1238`), so a real-metadata probe is a generic storage defect, **not proof of a v1 regression for a documented real-valued parameter**.

## Other scopes

- Render: bitmap resource routing/strips/fonts, skin cropping, three-layer widget-type precedence, missing text_y consumption, state fonts, wallpaper across multiple script slots. Parent-relative geometry/visibility is present in UI IR; do not report it wholly absent.
- Widgets: slider MOUSE_BEHAVIOUR sign is reversed in the KSP adapter (`ui.rs:448`); renderer ignores sensitivity and uses fixed TRAVEL. Knobs' vertical-drag policy, default/reset, fine adjust, wheel routing, internal scrolling and actual pointer usability need its interaction traces.
- Loop/render **P0 default-view policy**: `src/ui/part.rs:59` chooses Vector when the main UI has no unsupported parameters, Bitmap otherwise. The authored Original view must be the default, like v1. Parameter completeness must not decide visual presentation: fixing F7 can currently flip a library to Vector simply by clearing diagnostics. Conflux's 220 NKS entries select Bitmap on this baseline; vector-by-default is not by itself its complete blank explanation. Test new load and property publication preserve Original and the user's explicit choice.
- Loop: publication rebuilds full interfaces after generic property effects (`plugin.rs:892`), failures from UI validation are silently filtered in `ScriptUi::interfaces` (`src/sound/mod.rs:257`), effect queues and revisions can affect ordering/latency, and view-mode reset is separate from parameter fidelity. Keep its target architecture and measured CPU/frame costs authoritative.
- Format/core: bank/multi script publication, full decoded native program state, engine parameter display strings and DSP parameter laws are independent obligations. The single-instrument metadata helper can read bank script records that the product translator may still not execute.
- Census: use our per-file symbol incidence as mechanism exposure, not a rendered failure taxonomy or fully unlocked count.

## Unknowns and measurements needed

1. **Fully unlocked instruments**: for each F1–F10 patch, rerun the renderer/callback corpus census on matched path+program+slot+saved-state identities. Report overlap-aware incremental gains; lexical reach is not an unlock forecast.
2. **Legal parameter schema**: confirm vendor-profile contexts/access/types/applicability/defaults, including NKS extensions, historical KEY/WAVETABLE tokens, level-meter range laws and custom font IDs. Reject unsupported combinations explicitly; never infer semantics from an interned integer.
3. **Reference callbacks**: record an authored scalar/table/XY/text/file/mouse fixture in Kontakt with modifiers, get/set round trips, duplicate handlers, on ui_controls/on ui_update and waits. Measure edit-to-handler sample boundary and publication latency, not just wall-clock script compile time.
4. **Persistence native trace**: distinguish immediate-read entry consumption, read before menu population, invalid selected positions, all four snapshot policies and instrument-only persistence. Static RE pending-consumption proof is available; no live comparison was done here.
5. **Metadata vs actual execution**: the supplied token helper omits comments/literals but includes dead branches and does not inspect NCKP/native-UI property trees as source tokens. Compile per unique source/resource/profile, collect reached get/set arguments by symbol+type+index and multiply by matched use identities. Linked script files/native packages require separate resource traversal.
6. **Long text**: runtime Text capacity is 256 bytes (`sampler-core/src/ops.rs:11`), init text lines cap at 65,536 (`eval.rs:895`). Measure legitimate visible text demands; use bounded off-audio text payload handles/virtualized line windows rather than truncating authored meaning silently.
7. **Platform and automation**: exercise host save/reopen, parameter gestures and enumeration in Linux CLAP/VST3 plus Windows/macOS. Pure IR assertions do not validate focus, accessibility or host event policy.

Target order: F1 and F2 restore the broad mutation/getter path; F3–F6 establish state lifecycle; F7/F8 complete schemas and geometry; F9/F10 connect automation and every typed widget/callback. Share the existing control and generation ownership, and finish each source-specific semantic obligation instead of replacing KSP with another library-specific UI.
