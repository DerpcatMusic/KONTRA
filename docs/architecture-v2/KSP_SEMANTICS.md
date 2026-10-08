# KSP semantics audit

Scope: behaviours where the KSP runtime parses and lowers a script but may do something different from Kontakt. [KSP_COVERAGE.md](KSP_COVERAGE.md) answers "does the symbol compile"; this file answers "does it mean the same thing". Line numbers are at branch `v2/ksp-runtime-engine-par` HEAD (`git show HEAD:<file>`); another agent had uncommitted edits to `eval.rs` and `library.rs` while this was written, so working-tree lines may be shifted.

Basis legend: **M** = measured (read from our code or a decrypted corpus script, or counted over the 52 distinct corpus scripts in `/dev/shm/ksp-all`, 2,741 instrument uses); **S** = stated by the KSP manual (`/tmp/kman/KSP_Manual.txt`) or by the RE specs (`DSP_FORMAT_SPECIFICATION.md`, `DSP_SYSTEM_INVENTORY.md`, `kontakt-ksp-identifiers.json`, written from NI's public reference); **I** = inferred. Nothing here was run: no cargo, no playback. Anything needing a run is marked unknown and listed in the experiments at the end. RE policy: prose is our own, no reference pseudocode.

## 1. Persistence and restore

### 1.1 What the .nki stores (script record 0x06)

The RE spec (Format spec, "Kontakt binary grammar", script record) documents the layout the *writer* emits (source, flags, password, title, linked file, saved-variable count and records) and says outright that the writer's empty table "is not a schema for populated persistent state". So the per-entry grammar below is **our** reading, not a spec fact. It is exercised by shipping instruments, which is the only evidence.

| behaviour | basis | what our code does | verdict |
|---|---|---|---|
| Record tail = count + length-prefixed strings, one per saved variable | S (spec gives count+records) / M (works on 40+ libraries) | `vendor/ni-file/src/kontakt/objects/bpar_script.rs:60-70` reads them; `crates/sampler-kontakt/src/script.rs:44-57` is a second, strict parser (used by `sampler-native`, tests) | match |
| Entry text is `"<sigil+name> <value...>"`; sigil `$` int, `~` real, `@` string, `%` int array, `?` real array, `!` string array | I (derived from corpus behaviour) | `library.rs:1413-1437` `saved()` handles `$ ~ @ % ?`; `!` and any entry with a bad token are dropped | partial: `!` arrays dropped (34 uses in 7 scripts) |
| A damaged table must not be silently accepted | I | `bpar_script.rs:70` `unwrap_or_default()` turns a truncated table into "nothing saved" with no diagnostic; the strict parser errors instead | **mismatch** (silent): all defaults then apply |
| A dropped entry should be visible | M | `library.rs:215-222` reports `persistent.len() - state.len()` as "saved persistent text arrays", which is also hit by int arrays with one bad token | match, but message is wrong for non-text drops |
| Values are UTF-8 | I | `bpar_script.rs:62` `from_utf8_lossy` silently replaces bytes; `script.rs` keeps exact bytes | unknown (no non-UTF-8 value seen) |
| Names are per script slot; slot index = order of 0x06 chunks | M | `library.rs:196-214`, `apply_snapshot` matches by slot | match |

### 1.2 Declaring and restoring

| behaviour | basis | what our code does | verdict |
|---|---|---|---|
| `make_persistent` marks the variable; it is saved with instrument and snapshots | S (manual p.34) | `sema.rs:355-362` records `Persistence::Snapshot` statically; runtime builtin is a no-op `eval.rs:1121` | match, but see next row |
| Marking happens only if the call executes | S | static: a `make_persistent` in an untaken `if` still marks the variable, and `declare_effect` runs only for calls written directly in `on init` (`sema.rs:342` `in_init`), not inside functions called from init | **mismatch** (low risk: corpus puts all calls at top level of init, M) |
| `make_instr_persistent` saved with the instrument but not by snapshots | S (manual p.33) | stored as `Persistence::Instrument`, but `snapshot.rs:97-115` overwrites any named entry regardless of kind | **mismatch** (5 instruments call it) |
| Values are restored at the end of `on init`, then `on persistence_changed` runs, before any `ui_control` | S (manual p.13/34) | `eval.rs:175-195`: init body, then `restore` for every persistent var, then the callback | match |
| `read_persistent_var` restores immediately, only inside init | S (manual p.35) | `eval.rs:1121-1125` calls the same `restore`, even for non-persistent variables | partial: no check that the variable was made persistent |
| Restoring does not fire `ui_control` for the restored widget | S/I (manual: "use persistence_changed") | `restore` calls `write_var` only | match |
| Because only `ui_control` ran the effect, anything a `ui_control` handler *does* is not applied at load unless `persistence_changed` repeats it | S | Same as Kontakt; our host applies saved bus state from the .nki | match |
| Snapshot load: type 0 reruns init then persistence_changed; type 1 only persistence_changed; types 2/3 save only KSP vars | S (manual p.89) | `library.rs:44-53` overlays snapshot entries on the instrument state **before** the script runs, so every snapshot behaves like type 0; `set_snapshot_type` is ignored (`eval.rs:1114`, reported Unsupported in the UI) | partial: result equal for type 0, wrong for type 1/3 scripts that rely on init not rerunning (5 instruments) |
| Restored arrays: prefix of the shorter, cut to declared length | I | `eval.rs:359-369` | unknown (Kontakt rule not documented) |
| `persisted` and `persisted_arrays` keys include the sigil | M | `load.rs:529-555` keeps the sigil, HIR names include it | match |

### 1.3 How each UI type saves its value

Persisted declarations in the corpus (count of `make_persistent` calls, scripts using it): ui_switch 1800/22, ui_slider 1542/44, plain `$` 781/41, ui_value_edit 606/23, plain `%` 551/36, ui_button 352/35, ui_label 267/9, ui_menu 234/25, ui_knob 90/13, ui_text_edit 35/8, plain `!` 34/7, ui_table 33/8, plain `@` 20/5, ui_waveform 2, ui_file_selector 2, ui_xy 1 (M).

| widget | what the file holds | what our code does (`eval.rs`) | verdict |
|---|---|---|---|
| ui_switch / ui_button | int 0/1 | `restore` -> `write_var` clamps to 0..1 (`:372-396`) | match |
| ui_slider / knob / value_edit | int in the declared range (display factor is not applied) | clamped to the declared range; a restored value outside the range is clamped with a warning | match (I) |
| ui_menu | item **position**, variable receives the item **value** | fixed in commit 8a3fb769, `eval.rs:343-358`; position out of range falls back to the raw number | match. Caveat: an immediate `read_persistent_var` before the items are added maps with an empty menu (raw number), but the end-of-init restore overwrites it, so the final value is right |
| ui_label | label variable is an int; text set by `set_text` / `set_control_par_str` is not saved | int restored, text untouched | match (I) |
| ui_table | `%` int array | `persisted_arrays` | match |
| ui_xy | `?` real arrays | handled by `Saved::Reals` | match (1 use) |
| ui_text_edit, plain `@` | `@` string | `Value::Text` -> `Home::Text` | match |
| plain `!` string arrays | `!` entries | dropped by `saved()` (reported) | **mismatch** |
| ui_file_selector, ui_waveform | not established; the file format of these entries is unverified | no special handling | unknown (2 uses each) |
| polyphonic variables | not persistable | write ignored at init | match |

### 1.4 `$switch_sordino` in script 778d5bb709976a72 (Audio Imperia Areia/Pyramid, 7+ Core and Legato patches)

| step | evidence | verdict |
|---|---|---|
| Declaration | line 5768 `declare ui_switch $switch_sordino`, no initial value, then `make_persistent` and `read_persistent_var` at 5769-5770 | Kontakt default with no saved entry is 0 (off); ours is also 0 (`eval.rs:168-174`) |
| Saved value in the .nki | **not read**: the .nki is library-encrypted and compressed, only `sampler-kontakt` can open it. The other agent has a temporary `TMPSAVED` print in `restore` that would show it | unknown |
| What consumes it | only `on ui_control($switch_sordino)` (103370-103485) which sets `ENGINE_PAR_SEND_EFFECT_DRY_LEVEL` = 397143*(1-s) and `SEND_EFFECT_OUTPUT_GAIN` = 397143*s on the sordino bus; `on persistence_changed` (15255) and the label code only read it for fonts and positions | **it never selects samples** |
| Our law for those two pars | `lower.rs:2176-2260` `slot_param` + `effect_gain`: gain = (v/396851)^3, so 397143 is unity (1.002) | consistent with the script's intent (M) |
| What Kontakt does by default | switch 0 -> dry 1, wet 0 on the sordino IR send; because no `ui_control` fires at load, the bus levels come from whatever the .nki saved for that send/insert, and that is what both Kontakt and v2 play | same in v2 *if* our rack import honours the saved levels |

Conclusion: "Sustained plays con-sordino samples" cannot be explained by the sordino toggle. The toggle is an IR-convolution send, not a group selector. Sample choice in this script goes through `disallow_group($ALL_GROUPS)` (81724) followed by `allow_group(g)` for groups where the persisted arrays `%_whichgroupsto_art` (18 articulations x 2352 groups, declared 7810, `make_persistent` 7811), `%_whichgroupsto_rep`, `%whichgroupsto_noteon` are 1 (87000-87056). The group table is therefore **built from restored persistent arrays**; if any of those entries is dropped, truncated, or the arrays are recomputed in init before restore overwrites them, the wrong groups (including con-sordino ones) sound. Next step is an experiment, not a code change (see Experiments, E1).

## 2. ENGINE_PAR value laws

Catalog: 1,024 distinct identifiers in 20 categories (Instrument/Source/Amplifier 121, Filters 55, Insert FX ~490 over 8 subcategories, Send FX 133, Modulation 51, Module types and subtypes 149, Group Start Options 20). Spec basis for conversions is S only for the identifier *names*; the inventory says numeric values, display conversion and laws "still require confirmation" ("Kontakt script-facing DSP and system identifiers"). Every law we implement was fitted from shipping-script displays (M, per comments), none from the reference engine.

| parameter (engine units 0..1,000,000) | law we implement | where | corpus use (set/get sites; scripts) | verdict |
|---|---|---|---|---|
| `ENGINE_PAR_VOLUME` group/instrument (slot -1, generic -1) | dB = 18*log2(v) - 346.768, 0 dB at 629,960, +12 dB at full; readback is the inverse, clamped | `lower.rs:2283-2330` `engine_units`/`engine_value`, dispatch `:1893-1915` | 373 / 289; 45 / 42 scripts | match to displays; **only when slot and generic are the literal -1 constants** (`engine_param` uses `const_int`), otherwise falls to the opaque mirror |
| `ENGINE_PAR_PAN` | linear, (v-500000)/500 | same | 292 / 76 | same caveat |
| `ENGINE_PAR_TUNE` | +-36 st linear about 500000 | same | 310 / - ; 5 scripts | same caveat |
| AHDSR `ATTACK`, `DECAY`, `RELEASE` on the amp envelope | ms = 2^(v*(log2(max+2)-1)/1e6+1) - 2, max 15,000 (attack) / 25,000 ms | `lower.rs:2160-2175`, `envelope_frames` | 4665 / 173; 38 scripts | match to display law; only for the `ENV_AHDSR` slot returned by `find_mod` (which is `Approximate`), other modulators go to the host |
| `SUSTAIN` | cubic amplitude, `1e-6*v`^3 ; only 1,000,000 confirmed (comment `lower.rs:2210`) | `envelope_sustain` | 829 / 144 | unknown below unity; coverage doc calls it "Effect" and the code has a native branch: the two disagree |
| `ATK_CURVE` | clamped copy, no curve shaping | `:1873` | 4054 / 56 | unknown (curve law not modelled) |
| `SEND_EFFECT_OUTPUT_GAIN`, `INSERT_EFFECT_OUTPUT_GAIN`, `SEND_EFFECT_DRY_LEVEL` | (v/396851)^3 | `effect_gain` | 82+150+67 / 170 | fitted to stored slots (M); readback of `INSERT_EFFECT_OUTPUT_GAIN` (170 sites) is **not** the inverse, only a mirror |
| `EFFECT_BYPASS`, `SEND_EFFECT_BYPASS` | 0/1 | `WriteSlot` | 185+69 / - | match |
| `MOD_TARGET_INTENSITY`, `OUTPUT_CHANNEL`, `STEREO`, `CUTOFF`, `HOLD`, `RESONANCE`, `GN_GAIN`, `TP_*`, `RV2_*`, `CHORAL_*`, `FLAIR_*`, `PHASIS_*`, `SENDLEVEL_0`, `INTMOD_*`, ... (everything else) | no law: value is stored, mirrored for `get_engine_par`, and handed to the host as an effect (`lower.rs:1921-1927`) | corpus counts: MOD_TARGET_INTENSITY 2036 (41 scripts), STEREO 224 (26), OUTPUT_CHANNEL 173 (40), CUTOFF 363 (5), HOLD 120 (2), the effect-parameter families 30-35 each in 1-2 scripts | **unknown** per parameter; opaque |
| `get_engine_par` of a never-set parameter | returns the mirrored value, or 0 when nothing was set (`lower.rs:1916-1920`); Kontakt returns the module's current (authored) value | 237 instruments call it; reads concentrate on VOLUME 289, ATTACK 173, INSERT_EFFECT_OUTPUT_GAIN 170, RELEASE 167, DECAY 162, SUSTAIN 144 | **mismatch** for ATTACK/DECAY/RELEASE/SUSTAIN/INSERT_EFFECT_OUTPUT_GAIN reads before any write |
| `get_engine_par_disp` | returns "" (`eval.rs:1218`); 2741 instruments call it | purely cosmetic except where scripts parse the string | mismatch (UI text only) |

## 3. Group start options and group selection

Catalog (S): 20 identifiers: `ENGINE_PAR_START_CRITERIA_MODE`, the modes `START_CRITERIA_NONE / ON_KEY / ON_CONTROLLER / CYCLE_ROUND_ROBIN / CYCLE_RANDOM / SLICE_TRIGGER`, key min/max, controller, cc min/max, cycle class, zone idx, slice idx, sequencer-only, next-criteria and the `AND_NEXT / AND_NOT_NEXT / OR_NEXT` operators. The .nki stores up to four criteria per group (`vendor/ni-file/src/kontakt/objects/start_criteria_list.rs`, 31 bytes each).

| behaviour | basis | what our code does | verdict |
|---|---|---|---|
| A group with one Start-On-Key criterion plays only while that key range was the last key-switch | S | `keyswitch.rs:51-90` turns a single on-key criterion into an `Articulation` (keys `key_min..=key_max`) and tags the zones | match |
| Cycle Round Robin / Cycle Random with cycle class | S | pushed to `unsupported` ("group start options", `keyswitch.rs:68-82`); **all cycle groups sound together** | **mismatch** |
| Start On Controller (cc min/max) | S | same: reported, not gated | **mismatch** |
| Multiple criteria with AND / AND NOT / OR next | S | same: reported, not gated | **mismatch** |
| Slice trigger, zone idx, sequencer-only | S | not modelled | unknown |
| Two groups with overlapping on-key ranges | I | reported, articulation map dropped entirely (`keyswitch.rs:88-100`) | mismatch (all start-on-key gating lost for that instrument) |
| Muted groups are not played | S | dropped at translation (`library.rs:560`) | match |
| MIDI channel filter | S | reported, ignored (`library.rs:575`) | mismatch where used |
| Scripts set start criteria at runtime | M | **0 of 52 corpus scripts** mention `START_CRITERIA` | n/a |
| `allow_group` / `disallow_group` / `$ALL_GROUPS` in `on note` | S | `lower.rs:1568` edits the note's group mask; selection takes it as `Rejection::Group` (`sampler-core/src/prepare/selection.rs:648`); an edit after the note started is ignored | match |

The "zones rejected by group selection" counts (Areia 1,122 zones; Dolce 7 1st Violins Harmonics 36 zones) are `Rejection::Group`: the script's mask excluded those zones. Because 0 corpus scripts use the start-criteria constants, the mask comes entirely from `allow_group` driven by restored arrays (section 1.4) or from the unmodelled static start options of section 3. For Areia the first is certain (87000-87056). For Dolce I could not open the script (not in `/dev/shm/ksp-all` by name); this part is **unknown**. Distinguish with E2.

## 4. v2 blockers listed by the inventory

| inventory statement ("Exact blockers found in v2") | current code | verdict |
|---|---|---|
| `src/sound/v2.rs:212` calls `assign_alternatives(32)`, "replacing imported CC/channel/program alternatives" | now `v2.rs:218`; it runs on a **clone** (`with_alternatives`) inside `set_drivers` and only feeds the per-driver tables used when the player remaps (`configure`, `:195-200`). The initial plan is lowered from the un-modified instrument (`:1253`). `assign_alternatives` itself (`sampler-ir/src/lib.rs:622-646`) overwrites all alternatives, so *after a remap* imported alternatives are lost, and it assigns values by lowest switch key, not by authored order | partial: true for remaps, **does not explain Barbarian key 60** |
| Barbarian key 60: Kontakt plays `MidHall_Dyn1_Marc`, v2 plays `Close_Dyn2_SusNonAcc`; key 48 matches | The default plan never calls `assign_alternatives`. A different dynamics layer (Dyn1 vs Dyn2) and mic (MidHall vs Close) at a single key points at script group/mic selection (mic mix and dynamics sliders are restored persistent values in this vendor family) or at velocity/CC conditions, not at the articulation switch. `set_drivers` only matters if the UI sent `switching & 0x80` | unknown; E3 |
| `sampler-core/src/lower.rs:422` rejects `Trigger::First` and `Legato` | stale: `lower.rs:1517-1531` `previous_key` lowers `First` (no other key held), `Legato` (any held, interval -127..127) and `Transition` to a `PREVIOUS_KEY` controller condition | match (inventory stale). Open: whether Legato should include interval 0 (same key retriggered while held) is unspecified |
| Single selector cannot express recursive dimensions | n/a to KSP | not audited |

## 5. Core language semantics

| behaviour | basis | what our code does | verdict |
|---|---|---|---|
| Integers are 32-bit signed (manual p.31) | S | `sampler-core/src/integer.rs:17-31` wraps add/sub/mul/neg/abs; init-time evaluator uses the same op (`eval.rs:30-41`) | match for wrap; Kontakt overflow behaviour itself is undocumented |
| Division / modulo by zero | not in manual | returns 0 (`integer.rs:23-25`); `real` division follows IEEE (`eval.rs:43-50`: inf/NaN) | unknown (no reference run); real result into `real_to_int` gives 0 for NaN, saturates for inf (`eval.rs:64`) |
| `/` on ints truncates toward zero, `mod` keeps dividend sign | I | `wrapping_div/rem` | match (I) |
| Reals are 64-bit doubles; `real_to_int` outside i32 overflows (manual p.46) | S | saturating (`as i32`) | **mismatch**: Kontakt overflows, we saturate; rare |
| Array read out of bounds | I (previous audit) | reads 0 / "" with a warning; writes dropped (`eval.rs:308-334`, `405-423`) | unknown (as audited) |
| Real to text | not documented | shortest round trip with `.0` for integral values (`eval.rs:69-75`, a `ponytail` comment) | unknown; affects scripts that build text from reals |
| String compare, `&` concat, `sh_left` etc. | S | native ops; only ASCII case rules for `find_group` (eq_ignore_ascii_case, `lower.rs:1655`) | unknown |
| `wait(t)` microseconds -> frames; `wait(0)` continues | S | `lower.rs:1608-1625`, `behavior.rs:1947` | match |
| `wait_ticks(n)` | S (tempo-relative) | fixed 520 us per tick (`lower.rs:1612`), i.e. 120 BPM; real value 500000/960 = 520.83 us, tempo ignored | mismatch; 0 corpus uses (M) |
| Callback continuation after note-off | not documented in the manual extract | `WaitLifetime::Gate` default (`behavior.rs:31-35`) | unknown |
| Polyphonic variables are per note event, readable in release of that note, usable only in note/release | S (manual p.32) | `Home::Note` cells (`lower.rs:597-599`, `790-794`): outside note callbacks writes are ignored, reads are 0 | match |
| Event IDs are reusable; stale IDs are no-ops | S/I | source-visible alias with generation check, unknown/retired IDs resolve to nothing (`BEHAVIOR.md` "Source-visible event identities") | match (I) |
| Init budget | n/a | `INIT_FUEL` 200,000,000 steps (`eval.rs:28`); on exhaustion the script is abandoned | by design |

## Top 10 concrete bugs, ranked by corpus reach

Reach is instruments out of 2,741 corpus uses unless stated; "scripts" is out of 52 distinct scripts. Ranking is by how many instruments the behaviour can change, not by confirmed audio impact.

1. **Static start options not modelled** (round robin / random cycle, on-controller, AND/OR next, MIDI channel filter): all such groups sound together. `keyswitch.rs:68-82`, `library.rs:575`. Reach: every instrument with non-script RR groups; unmeasured, a per-group scan is needed (E2).
2. **`get_engine_par` returns 0 for unwritten parameters** (ATTACK, DECAY, RELEASE, SUSTAIN, INSERT_EFFECT_OUTPUT_GAIN): scripts that read-then-scale start from 0. `lower.rs:1916`. 237 instruments, 30+ scripts for the AHDSR set.
3. **Opaque `set_engine_par` families** (MOD_TARGET_INTENSITY 2036 sites in 41 scripts, STEREO, OUTPUT_CHANNEL, CUTOFF, HOLD, ATK_CURVE, SUSTAIN below unity, reverb/chorus/phaser/flanger families): stored and forwarded as host effects with no value law. `lower.rs:1921`. Up to ~2,700 instruments call `set_engine_par`; the effect-parameter families affect fewer than 10 scripts each.
4. **`engine_param` / `envelope_param` require literal constants** for slot/generic: any variable slot falls to the opaque mirror, so group volume/pan/tune is silently not applied. `lower.rs:2146-2175`. Rate unknown (E4: count `set_engine_par` sites whose args 3/4 are non-literal).
5. **Saved arrays drop silently** when a token fails to parse, or are `!` string arrays; a damaged table becomes "nothing saved" without a message in the lenient parser. `library.rs:1413`, `bpar_script.rs:70`. 34 `!` persists in 7 scripts, 551 `%` persists in 36 scripts (Areia-family group tables ride on `%` arrays).
6. **Areia/Pyramid group selection depends on persisted arrays**; if restore order or content is off, wrong or all groups sound ("Sustained" with sordino character). `778d5bb709976a72` lines 7810-7811, 87000-87056; shared by every Audio Imperia patch using that script (152 instrument files in the manifest). The sordino switch is a red herring (section 1.4).
7. **`get_engine_par_disp` returns an empty string** (2,741 instruments call it): display only; scripts that parse it get "". `eval.rs:1218`.
8. **Snapshots always behave as snapshot type 0**; `set_snapshot_type` ignored; `make_instr_persistent` values are overwritten by snapshots. `library.rs:44-53`, `snapshot.rs:97-115`. 5 instruments each.
9. **`make_persistent` is static and only recognised directly in init**; `read_persistent_var` restores non-persistent variables. `sema.rs:342-362`, `eval.rs:1121`. No corpus instrument is known to be affected.
10. **Overlapping start-on-key ranges disable all start-key gating** for the instrument, and `wait_ticks` assumes 120 BPM; real-to-int saturates instead of overflowing. `keyswitch.rs:88-100`, `lower.rs:1612`, `eval.rs:64`. Rare or 0 corpus uses.

Stale items to retire from the inventory: the `lower.rs:422` First/Legato blocker, and the line number `v2.rs:212`.

## Experiments to settle the unknowns (all need a build or run, none done here)

- **E1 sordino**: with the `TMPSAVED` print, load Areia 16 Violins and print `$switch_sordino`, then the first 40 entries of `%_whichgroupsto_art[ART*2352 ..]` for "Sustained", and the group names allowed by the first note. Compare with Kontakt's group list for the same note. Expected: sordino is 0 and the allowed set contains no sordino group; if it does, the culprit is an array restore, not the switch.
- **E2 start options**: a one-shot census of `Vec<StartCriteriaParams>` per group over the library tree (counts by mode). That gives the real reach of bug 1 and the Dolce case.
- **E3 Barbarian key 60**: trace `allow_group` calls and `get/set_event_par_arr` for the note, and print the persisted mic and dynamics values. Compare to the Kontakt play-through already recorded.
- **E4 constant arguments**: count corpus `set_engine_par` sites by whether slot/generic are literal, and `get_engine_par` reads before a write.

## VERDICT

**Targeted fixes, not an overhaul.** The evaluator, scheduler, integer, array and polyphonic semantics are sound and tested; the serious gaps are narrow and sit at the boundary to the host: (a) static group start options that the IR cannot yet express, (b) engine-parameter laws and readback, (c) fidelity of saved state coming out of the .nki. None needs a new runtime design.

Order of work (each is a small, independently testable change):
1. E1/E2 first. They are cheap and decide whether the Areia and Dolce failures are a restore bug (fix `saved()`, `!` arrays, silent table drop: a day) or an IR gap (item 2).
2. IR support for cycle round robin/random and on-controller start criteria as `Trigger`/condition on groups (week; touches `sampler-ir`, `lower`, `selection.rs`). Risk: medium, changes sound for every RR instrument, so gate it behind the corpus health scoreboard.
3. `get_engine_par` returning authored values (needs the IR group/envelope/effect state; small) and accepting non-literal slot arguments by moving the `const_int` checks to runtime registers (small, low risk).
4. Per-parameter laws for the opaque families, one at a time, only with a reference measurement, starting with MOD_TARGET_INTENSITY and CUTOFF.

Overhaul risk if attempted anyway: high. The runtime is lowered to a typed IR and shared with UVI/native behaviour; a rewrite would invalidate the 19 test files in `sampler-ksp/tests` and the 432-of-1494 corpus-health baseline.
