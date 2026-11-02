# KSP front-end bindings — 2026-11-01

`docs/re/gap-inventory-2026-11-01.md` §5 names six front-end gaps that need a
binding or a rule, not DSP. All six are closed. Every number below comes from
compiling the corpus, so nothing here needs Kontakt to check.

**728 failing script slots in 718 instruments → 478 in 470.** 250 slots and 248
instruments moved from failing to compiling; **none** moved the other way
(`comm` on the two failing sets is empty in that direction).

Nothing audible changed: `tools/verify.sh` prints `VERIFY PASSED`, Electric
Mint's fingerprint is still `f18acaab5b897588`, and
`tools/parity-corpus/run.py` is 20/20 with every case `moved +0.00/+0.00`.

## How this was measured

The harness is the gap lane's, still in `/var/tmp/gaps` (nothing was rebuilt):
`scripts/` holds the 8,016 slots extracted from 3,694 loaded instruments, and
`kspcheck` links the workspace's `kontra-ksp` by path and calls
`kontra_ksp::compile(src, &[])` on each. `compile-before.tsv` is the baseline on
`feat/integration` (`ca001adb2`), `compile-final.tsv` the run on this branch.

```sh
ls scripts/*/slot-*.ksp | kspcheck > compile-final.tsv     # 45 s for the corpus
awk -F'\t' '$1=="err"' compile-final.tsv | wc -l           # 478
```

Before → after, per gap:

| gap (one slot's first error) | slots before | slots after |
|---|---:|---:|
| `unknown command hide_inst_icon` | 188 | **0** |
| `$EVENT_VELOCITY is read-only` | 22 | **0** |
| `$NUM_PRESETS is a reserved name` | 9 | **0** |
| `redirect_output takes N arguments` | 12 | **0** |
| `not a UI control` | 10 | **0** |
| `expected a constant` | 29 | **0** |
| `unknown function num_slices_zone` | 362 | 362 |
| `unknown callback _pgs_changed` | 42 | 42 |
| `unknown function zone_slice_start` | 34 | 34 |
| `unknown command dont_use_machine_mode` | 5 | 25 |
| `unknown function mf_get_note_length` | 7 | 7 |
| `unknown command set_num_user_zones` | 4 | 4 |
| `?…_stage / ?home_vr_xypad is not declared` | 3 | 3 |
| `more values than array elements` | 1 | 1 |

`dont_use_machine_mode` reads 5 → 25 because 20 slots that failed earlier on a
constant now fail on it instead: those 20 of the 29 constant slots do not yet
compile, and that is the next one-line no-op, not a regression (see §3).

## 1. The bindings

| binding | file | citation | test |
|---|---|---|---|
| `hide_inst_icon` | `crates/script/ksp/src/builtins.rs`, `crates/engine/vm/src/action.rs`, `crates/engine/engine/src/script.rs` | **not in the manual.** The KSP reference 7.10 and 8.0 document the instrument icon only as `set_control_par($INST_ICON_ID, $CONTROL_PAR_HIDE, …)`; `hide_inst_icon` is undocumented. 188 corpus instruments call it with no arguments, and for all 188 it was the slot's only compile error (measured, not the inventory's 187). It hides the icon in Kontakt's own instrument header, and KONTRA's page draws no instrument header, so a no-op is sufficient for the instruments' own use | `hide_inst_icon_is_a_no_op_command` |
| `$EVENT_VELOCITY` is writable | `crates/script/ksp/src/compile/mod.rs` (`assign`) | KSP reference, `change_velo()`: *"Once the velocity of a particular note event is changed, it becomes the new `$EVENT_VELOCITY`"* — the variable is the event's velocity, so a write lowers to `change_velo($EVENT_ID, v)` | `event_velocity_can_be_written` |
| `$NUM_PRESETS` is not reserved | `crates/script/ksp/src/compile/declare.rs`, `crates/script/ksp/src/consts.rs` | KSP reference, `const $` example: `declare const $NUM_PRESETS := 10` / `declare %preset_data[$NUM_PRESETS * $NUM_PARAMETERS]` — the manual's own example declares exactly this name | `num_presets_is_a_script_constant` |
| `redirect_output` argument counts | `crates/script/ksp/src/builtins.rs` | KSP reference, `redirect_output()`: `redirect_output(<event-id>, <output-type>, <index>)`, and `<index>` *"Has no effect when `<output-type>` is set to `$OUTPUT_TYPE_DEFAULT`"* — so the index is optional and a two-argument call routes to 0 | `redirect_output_takes_two_or_three_arguments` |
| `hide_part` on a variable that is not a control | `crates/script/ksp/src/compile/expr.rs` | KSP reference, `get_ui_id()`: *"Even regular variables and constants (those that are not UI widgets) get a UI ID assigned, however this ID cannot be used with various `get_control_par()`/`set_control_par()` commands!"* — and `hide_part(<variable>, <hide-mask>)` documents its first argument as a variable name, not a control | `hide_part_takes_a_variable_that_is_not_a_control` |
| constants fold number functions and `num_elements` | `crates/script/ksp/src/compile/declare.rs` | see §2 | `constants_fold_number_functions`, `num_elements_folds_in_a_declaration` |

Two details worth keeping:

- **`hide_inst_icon` asks the host and the host answers.** It is an
  `Action::HideInstIcon` with an explicit arm in the engine's script host that
  returns nothing, so it is not counted as an unanswered action — the command is
  implemented, its result is just nothing. `Action` variants were appended, not
  inserted, because an action's id is its position and ids appear in traces.
- **`hide_part` still wants a variable.** The id is passed (`FIRST_UI_ID + n`,
  the same number `get_ui_id()` returns) and the engine finds no control behind
  it, which is what Kontakt does at runtime for a non-widget id. A non-variable
  argument is still refused, and `on ui_control($x)` on a non-control still
  says `not a UI control` — the tests assert both, so the rule was narrowed
  rather than removed.

## 2. The 29 `expected a constant` cases

The message does not name the token, so each was read. They are three
mechanisms, and **none of them was judged to be a genuine Kontakt refusal** — a
slot is only in this list when our folder was short of a shape Kontakt accepts:

| shape | slots | instruments | why it is a constant |
|---|---:|---:|---|
| `declare const ~LOG10 := log(10.0)` | 23 | 23 | a pure number function of a literal has one value, and the arithmetic functions are the manual's own list ("Arithmetic Commands & Operators"). 23 shipping instruments (Heavyocity FORZO/NOVO/Foundations, e-instruments, NI's Scene, ALLURE, AVANT) declare it, so Kontakt's folder evaluates it and ours was short of the list |
| `declare %a[num_elements(%b) + 5]` | 4 | 4 | `%` (integer array), "Remarks": *"The number of elements must be defined with a constant value, variables cannot be used for this purpose."* `num_elements(%b)` is not a variable — an array's size is fixed where it is declared — and Kontakt folds it (Heavyocity's `%cycle_copy_buffer`) |
| `declare %original_events[num_elements(%artificial_events)]`, `declare const $x := num_elements(…) + …` | 2 | 2 | as above (NI's Scene - Lotus; e-instruments' Session Keys Electric S) |

The folder now evaluates the arithmetic list (`abs`, `min`, `max` on reals too,
plus `sqrt`, `cbrt`, `exp`, `exp2`, `log`, `log2`, `log10`, `sin`, `cos`, `tan`,
`asin`, `acos`, `atan`, `pow`, `floor`, `ceil`, `round`, `sgn`, `signbit`) and
`num_elements()` of a declared or system array.

**What is still refused, deliberately**, and asserted by the same tests:

- a size or constant read from a variable — `declare %a[$n]`, `declare const $Y
  := $x`: the manual's "variables cannot be used for this purpose";
- a constant whose value needs the host — `declare const $Y := random(0, 1)`:
  no compile-time value exists, so Kontakt cannot fold it either.

## 3. What is left, and what it would take

| slots | first error | what it is |
|---:|---|---|
| 362 | `unknown function num_slices_zone` | the zone-slice model (Spitfire Studio Brass, Vocalise 3) — a model, not a binding |
| 42 | `unknown callback _pgs_changed` | a callback alias, if Kontakt honours it |
| 34 | `unknown function zone_slice_start` | the same model |
| 25 | `unknown command dont_use_machine_mode` | one line on the same footing as `hide_inst_icon`, but its *effect* (Kontakt's machine mode) is not documented in the manual, so it was not in this pass's list |
| 7 | `unknown function mf_get_note_length` | one accessor on the MIDI-file object |
| 4 | `unknown command set_num_user_zones` | the user-zone model |
| 3 | `?…_stage`, `?home_vr_xypad is not declared` | undeclared text variables |
| 1 | `more values than array elements` | KSP lets the surplus be ignored |

## 4. What still needs a Kontakt probe

- **`hide_inst_icon`'s real effect.** The command is in neither manual. Our
  no-op rests on the corpus's own use (no arguments, and the instruments run
  without it affecting anything but Kontakt's header) and on our page having no
  header to hide. A probe could confirm Kontakt accepts zero arguments and what
  it does with a second call.
- **`$EVENT_VELOCITY` written after the first `wait()`.** The manual says
  `change_velo()` "only works before the first `wait()` statement; if the voice
  is already running, only the value of the variable changes". We lower the
  write to `change_velo`, so past the first wait the voice is left alone *and*
  a later read sees the event's velocity rather than the number written:
  Kontakt presumably keeps the variable changed. Matching that needs a
  script-side copy per event, and a probe to measure what Kontakt actually
  returns. All 22 corpus writers do it in `on note` (measured: one generator),
  which is the window the manual defines.
- **`redirect_output` with two arguments.** The manual documents three; that
  Kontakt accepts two is inferred from 11 shipping instruments calling it with
  two, not from the manual.
- **`hide_part` on a non-widget id at runtime.** Kontakt compiles it (10
  instruments); whether it also warns or errors when the id has no widget is not
  measured, and our engine records the parameter and draws nothing.

## 5. Instruments that now compile

248 instruments, by library and by the error that was blocking them (one
instrument, Scene - Lotus, had two different errors in two slots; every other
row is an instrument whose failing slot had one cause):

| library | instruments | the error that was blocking them |
|---|---:|---|
| `#Non NICNT` | 188 | `unknown command hide_inst_icon` — every VSTBuzz Power Move patch |
| Native Instruments Symphony Essentials Percussion | 22 | `$EVENT_VELOCITY is read-only` — the kit's velocity clamps |
| Session Horns Pro | 10 | `not a UI control` — `hide_part` on a plain variable |
| Session Guitarist Electric Ruby Deluxe (2), Electric Storm Deluxe (2), Orange Tree Evolution (6) | 10 | `redirect_output takes 2 arguments` |
| Action Strings 2 (3 instruments across two builds), Heavyocity Foundations (6) | 9 | `$NUM_PRESETS is a reserved name` |
| Heavyocity FORZO / NOVO / Aspire / Symphonic Destruction, ALLURE, AVANT, Session Keys Electric S, Scene - Lotus | 9 | `expected a constant` (`log(10.0)`, `num_elements()`) |

## 6. The list

Every instrument whose every slot compiled after this change and did not before,
in script-index order (the index is the harness's, over the 3,694 instruments
that load):

- Foundations Nylon Guitar — `Kontakt1/Foundations - Nylon Guitar [Heavyocity]/Instruments/Foundations Nylon Guitar.nki`
- II String Designer Essentials — `Kontakt1/Heavyocity - NOVO Essentials/Instruments/II String Designer Essentials.nki`
- Field Drum Off — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/1 Drums/Field Drum Off.nki`
- Tom 2 — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/1 Drums/Tom 2.nki`
- Cymbal 1 — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/3 Cymbals/Cymbal 1.nki`
- Shakers — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/5 Wood/Shakers.nki`
- AR Dark City Nights — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Dark City Nights.nki`
- AR Halogen Nights — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Halogen Nights.nki`
- AR Repeater — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Repeater.nki`
- AR Talk About — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Talk About.nki`
- BL Bell Keys — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BL Bell Keys.nki`
- BR Powerful Brass — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BR Powerful Brass.nki`
- BS Juno Fifth — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Juno Fifth.nki`
- BS Moog Tube — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Moog Tube.nki`
- BS Reso Bass — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Reso Bass.nki`
- BS Short Echo Drive — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Short Echo Drive.nki`
- BS Tube Driven Bass — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Tube Driven Bass.nki`
- DR Synth Hat Open — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Synth Hat Open.nki`
- DR Synth Kick F — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Synth Kick F.nki`
- DR Synth Tom C — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Synth Tom C.nki`
- AR Glass of Milk (MW) — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/AR Glass of Milk (MW).nki`
- AR Walkie Talkie (MW) — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/AR Walkie Talkie (MW).nki`
- PL Cheap Talk — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/PL Cheap Talk.nki`
- PL Voicebox — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/PL Voicebox.nki`
- KY Bright Keys — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/KY Bright Keys.nki`
- KY Soft FM Sync — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/KY Soft FM Sync.nki`
- LD Glide Syncer — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Glide Syncer.nki`
- LD Mono Drive — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Mono Drive.nki`
- LD Rotator Lead — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Rotator Lead.nki`
- LD Soft Tube Lead — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Soft Tube Lead.nki`
- OR DX Organ — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/OR DX Organ.nki`
- PD Deep Reso Pad — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Deep Reso Pad.nki`
- PD High Power — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD High Power.nki`
- PD Plutonium — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Plutonium.nki`
- PD Stratosphere — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Stratosphere.nki`
- PD Zardos — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Zardos.nki`
- ST Phasing Strings — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/ST Phasing Strings.nki`
- Showroom Violin — `Kontakt1/#Non NICNT/VSTbuzz.The.Showroom.Violin.KONTAKT/Instruments/Showroom Violin.nki`
- Electric Ruby Deluxe (Melody) — `Kontakt1/Session Guitarist - Electric Ruby Deluxe [Native Instruments]/Instruments/Electric Ruby Deluxe (Melody).nki`
- Flugelhorn — `Kontakt1/Session Horns Pro v1.5.0/Instruments/Solo Instruments/Flugelhorn.nki`
- Tuba — `Kontakt1/Session Horns Pro v1.5.0/Instruments/Solo Instruments/Tuba.nki`
- Evolution Steel Strings — `Kontakt1/Evolution Steel Strings 1.2.5 [Orange Tree Samples]/Instruments/Evolution Steel Strings.nki`
- Foundations Piano — `Kontakt1/Foundations - Piano [Heavyocity]/Instruments/Foundations Piano.nki`
- I Brass Designer — `Kontakt1/Heavyocity - FORZO Modern Brass/Instruments/II Evolved/I Brass Designer.nki`
- Snare 1 — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/1 Drums/Snare 1.nki`
- Tom 3 — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/1 Drums/Tom 3.nki`
- Cymbal 2 — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/3 Cymbals/Cymbal 2.nki`
- Orchestral Percussion Kit — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/Orchestral Percussion Kit.nki`
- AR Dataflow — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Dataflow.nki`
- AR High Voltage — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR High Voltage.nki`
- AR Reso Sequence — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Reso Sequence.nki`
- AR The Opening — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR The Opening.nki`
- BL DX Bell — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BL DX Bell.nki`
- BR Super Brass — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BR Super Brass.nki`
- BS Metal FM Bass — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Metal FM Bass.nki`
- BS Nice FM — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Nice FM.nki`
- BS Reso Detune Bass — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Reso Detune Bass.nki`
- BS Snap Bass — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Snap Bass.nki`
- BS Vintage Analog — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Vintage Analog.nki`
- DR Synth Kick A — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Synth Kick A.nki`
- DR Synth Snare A — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Synth Snare A.nki`
- DR Tom Menu — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Tom Menu.nki`
- AR Groove Heart (MW) — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/AR Groove Heart (MW).nki`
- AS Kit Ridder Perc (MW) — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/AS Kit Ridder Perc (MW).nki`
- PL Get Lucky — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/PL Get Lucky.nki`
- PL Wide One (MW) — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/PL Wide One (MW).nki`
- KY Filter Wave — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/KY Filter Wave.nki`
- KY Waterbubble — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/KY Waterbubble.nki`
- LD Hyperflex — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Hyperflex.nki`
- LD Nice Keys — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Nice Keys.nki`
- LD Saturator — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Saturator.nki`
- LD Super Poly — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Super Poly.nki`
- OR Retro Organ — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/OR Retro Organ.nki`
- PD Desert Sunrise — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Desert Sunrise.nki`
- PD Hyperspace — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Hyperspace.nki`
- PD Reso Sweeper — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Reso Sweeper.nki`
- PD Super Bright — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Super Bright.nki`
- ST Beautiful Strings — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/ST Beautiful Strings.nki`
- ST Stereo Section — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/ST Stereo Section.nki`
- Electric Ruby Deluxe — `Kontakt1/Session Guitarist - Electric Ruby Deluxe [Native Instruments]/Instruments/Electric Ruby Deluxe.nki`
- Mute Trumpet — `Kontakt1/Session Horns Pro v1.5.0/Instruments/Solo Instruments/Mute Trumpet.nki`
- Action Strings 2 — `Kontakt1/Action Strings 2 v1.2.0/Action Strings 2 Library/Instruments/Action Strings 2.nki`
- Evolution Bluegrass Banjo — `Kontakt1/Evolution Bluegrass Banjo 1.2.5 [Orange Tree Samples]/Instruments/Evolution Bluegrass Banjo.nki`
- Foundations Staccato Brass — `Kontakt1/Foundations - Staccato Brass [Heavyocity]/Instruments/Foundations Staccato Brass.nki`
- Snare 1 Off — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/1 Drums/Snare 1 Off.nki`
- Glockenspiel — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/2 Tuned Percussion/Glockenspiel.nki`
- Gong — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/3 Cymbals/Gong.nki`
- AR Deep Ocean — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Deep Ocean.nki`
- AR Magnetic — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Magnetic.nki`
- AR Scattered — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Scattered.nki`
- AR Third Flute Strikes — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Third Flute Strikes.nki`
- BL Glass Resonance — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BL Glass Resonance.nki`
- BS Another Reso Shorty — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Another Reso Shorty.nki`
- BS Minimum — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Minimum.nki`
- BS Noisy FM — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Noisy FM.nki`
- BS Resosquacker — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Resosquacker.nki`
- BS Snap Saw Bass — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Snap Saw Bass.nki`
- BS Vintager — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Vintager.nki`
- DR Synth Kick B — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Synth Kick B.nki`
- DR Synth Snare B — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Synth Snare B.nki`
- AR 80s Angle Chords — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/AR 80s Angle Chords.nki`
- AR Hollow Pulse — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/AR Hollow Pulse.nki`
- BS Hungry & Mean — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/BS Hungry & Mean.nki`
- PL High Low — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/PL High Low.nki`
- SFX Big Boom — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/SFX Big Boom.nki`
- KY JP Filter Pluck — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/KY JP Filter Pluck.nki`
- LD Bright Poly Lead — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Bright Poly Lead.nki`
- LD JX Lead — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD JX Lead.nki`
- LD Poly PWM — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Poly PWM.nki`
- LD Saw Glider — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Saw Glider.nki`
- LD Vacuum Tube — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Vacuum Tube.nki`
- OR Velo Organ — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/OR Velo Organ.nki`
- PD Disk Wave — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Disk Wave.nki`
- PD Jupiter Pad — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Jupiter Pad.nki`
- PD Sizzling Rain — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Sizzling Rain.nki`
- PD Super Swell — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Super Swell.nki`
- ST Brass And Strings — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/ST Brass And Strings.nki`
- ST Suspension — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/ST Suspension.nki`
- Electric Storm Deluxe (Melody) — `Kontakt1/Session Guitarist - Electric Storm Deluxe [Native Instruments]/Instruments/Electric Storm Deluxe (Melody).nki`
- Tenor Sax — `Kontakt1/Session Horns Pro v1.5.0/Instruments/Solo Instruments/Tenor Sax.nki`
- Action Strings 2 — `Kontakt1/Action Strings 2 v1.2.0/Instruments/Action Strings 2.nki`
- Evolution Django Jazz — `Kontakt1/Evolution Django Jazz 1.2.5 [Orange Tree Samples]/Instruments/Evolution Django Jazz.nki`
- Evolution Flatwound Bass — `Kontakt1/Evolution Flatwound Bass 1.2.5 [Orange Tree Samples]/Instruments/Evolution Flatwound Bass.nki`
- Foundations Staccato Strings — `Kontakt1/Foundations - Staccato Strings [Heavyocity]/Instruments/Foundations Staccato Strings.nki`
- ASPIRE Modern Mallets — `Kontakt1/Heavyocity - Aspire/Instruments/ASPIRE Modern Mallets.nki`
- I SD Designer — `Kontakt1/Heavyocity Symphonic Destruction v1.1.0/Symphonic Destruction/Instruments/I Designers/I SD Designer.nki`
- Snare 2 — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/1 Drums/Snare 2.nki`
- Marimba — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/2 Tuned Percussion/Marimba.nki`
- TamTam — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/3 Cymbals/TamTam.nki`
- AR Backdrift — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Backdrift.nki`
- AR Echowave — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Echowave.nki`
- AR Midnight Cop — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Midnight Cop.nki`
- AR Step Ahead — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Step Ahead.nki`
- AR Twilight — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Twilight.nki`
- BL Vibrabell — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BL Vibrabell.nki`
- BS Exhale — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Exhale.nki`
- BS Mono Degrador — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Mono Degrador.nki`
- BS Oneshot DX — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Oneshot DX.nki`
- BS Saw Long Open — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Saw Long Open.nki`
- BS Squack — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Squack.nki`
- DR Kick Menu — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Kick Menu.nki`
- DR Synth Kick C — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Synth Kick C.nki`
- DR Synth Snare C — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Synth Snare C.nki`
- AR Congo Mongo (MW) — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/AR Congo Mongo (MW).nki`
- AR Leapfrog — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/AR Leapfrog.nki`
- DR Weird Tom — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/DR Weird Tom.nki`
- PL Leading Lady — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/PL Leading Lady.nki`
- SFX Crusier Flyby — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/SFX Crusier Flyby.nki`
- KY Phasing Keys — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/KY Phasing Keys.nki`
- LD Classic Mono — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Classic Mono.nki`
- LD Lovely Flute — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Lovely Flute.nki`
- LD Poly Saw Love — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Poly Saw Love.nki`
- LD Screaming — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Screaming.nki`
- LD Valves — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Valves.nki`
- PD Alien Choir — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Alien Choir.nki`
- PD Distant Stars — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Distant Stars.nki`
- PD Neon Lake — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Neon Lake.nki`
- PD Skylights — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Skylights.nki`
- PD Vintager — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Vintager.nki`
- ST Fifth Section — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/ST Fifth Section.nki`
- ST Synth Orchestra — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/ST Synth Orchestra.nki`
- Scene - Lotus — `Kontakt1/Scene - Lotus [Native Instruments]/Instruments/Scene - Lotus.nki`
- Electric Storm Deluxe — `Kontakt1/Session Guitarist - Electric Storm Deluxe [Native Instruments]/Instruments/Electric Storm Deluxe.nki`
- Alto Sax — `Kontakt1/Session Horns Pro v1.5.0/Instruments/Solo Instruments/Alto Sax.nki`
- Tenor Trombone — `Kontakt1/Session Horns Pro v1.5.0/Instruments/Solo Instruments/Tenor Trombone.nki`
- ALLURE Modern Upright — `Kontakt1/ALLURE Modern Upright/Instruments/ALLURE Modern Upright.nki`
- Evolution Dracus — `Kontakt1/Evolution Dracus 1.2.5 [Orange Tree Samples]/Instruments/Evolution Dracus.nki`
- Evolution Jazz Archtop — `Kontakt1/Evolution Jazz Archtop/Instruments/Evolution Jazz Archtop.nki`
- Foundations Synth Bass — `Kontakt1/Foundations - Synth Bass [Heavyocity]/Instruments/Foundations Synth Bass.nki`
- Bass Drum — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/1 Drums/Bass Drum.nki`
- Snare 2 Off — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/1 Drums/Snare 2 Off.nki`
- Timpani — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/2 Tuned Percussion/Timpani.nki`
- Tambourines — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/4 Metal/Tambourines.nki`
- AR Charming — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Charming.nki`
- AR Freeway — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Freeway.nki`
- AR Process One — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Process One.nki`
- AR Strobe — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Strobe.nki`
- AR Unison Seq — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Unison Seq.nki`
- BR Famicon Brass — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BR Famicon Brass.nki`
- BS Hidden Stranger — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Hidden Stranger.nki`
- BS Mono Glide Vox — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Mono Glide Vox.nki`
- BS Pulsetronic — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Pulsetronic.nki`
- BS Saw Pluck Bass — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Saw Pluck Bass.nki`
- BS Superdrive — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Superdrive.nki`
- DR Snare Menu — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Snare Menu.nki`
- DR Synth Kick D — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Synth Kick D.nki`
- DR Synth Tom A — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Synth Tom A.nki`
- AR Faster Better Saw (MW) — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/AR Faster Better Saw (MW).nki`
- AR Pulse to Saw (MW) — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/AR Pulse to Saw (MW).nki`
- PD Barely There — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/PD Barely There.nki`
- PL Metal Mickey (MW) — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/PL Metal Mickey (MW).nki`
- ST Subtle Saw — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/ST Subtle Saw.nki`
- KY Poly FM Keys — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/KY Poly FM Keys.nki`
- LD Echo Vox — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Echo Vox.nki`
- LD Metallic Keys — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Metallic Keys.nki`
- LD Polyspace — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Polyspace.nki`
- LD Signal — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Signal.nki`
- LD Warm Tone — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Warm Tone.nki`
- PD Choir Room — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Choir Room.nki`
- PD Force Fields — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Force Fields.nki`
- PD Nice Reso Pad — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Nice Reso Pad.nki`
- PD Starglider — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Starglider.nki`
- PD Walk On The Moon — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Walk On The Moon.nki`
- ST Imperia — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/ST Imperia.nki`
- Baritone Sax — `Kontakt1/Session Horns Pro v1.5.0/Instruments/Solo Instruments/Baritone Sax.nki`
- Trumpet 1 — `Kontakt1/Session Horns Pro v1.5.0/Instruments/Solo Instruments/Trumpet 1.nki`
- AVANT Modern Keys — `Kontakt1/AVANT Modern Keys/Instruments/AVANT Modern Keys.nki`
- Session Keys - Electric S — `Kontakt1/e-instruments - Session Keys Electric S/Instruments/Session Keys - Electric S.nki`
- Foundations Emotive Choir — `Kontakt1/Foundations - Emotive Choir [Heavyocity]/Instruments/Foundations Emotive Choir.nki`
- Synth Designer — `Kontakt1/Heavyocity - Novo Pack 03 Synthetic Strings/Instruments/Synth Designer.nki`
- Action Strings 2 — `Kontakt1/Native Instruments Action Strings 2 v1.1.1/Action Strings 2 Library/Instruments/Action Strings 2.nki`
- Field Drum — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/1 Drums/Field Drum.nki`
- Tom 1 — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/1 Drums/Tom 1.nki`
- Xylophone — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/2 Tuned Percussion/Xylophone.nki`
- Triangle — `Kontakt1/Native.Instruments.Symphony.Essentials.Percussion.v1.3.0.KONTAKT-DVDR/Symphony Essentials Percussion Library/Instruments/4 Metal/Triangle.nki`
- AR Classic Arp — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Classic Arp.nki`
- AR Full Throttle — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Full Throttle.nki`
- AR Quick Runner — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Quick Runner.nki`
- AR Swinging Strings — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Swinging Strings.nki`
- AR Vintage Phrase — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/AR Vintage Phrase.nki`
- BR Galaxy Brass — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BR Galaxy Brass.nki`
- BS High Current — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS High Current.nki`
- BS Moog Bass — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Moog Bass.nki`
- BS Quick Bass — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Quick Bass.nki`
- BS Saw Pluck — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS Saw Pluck.nki`
- BS The Returner — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/BS The Returner.nki`
- DR Synth Hat — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Synth Hat.nki`
- DR Synth Kick E — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Synth Kick E.nki`
- DR Synth Tom B — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/DR Synth Tom B.nki`
- AR  Frantic Mario (MW) — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/AR  Frantic Mario (MW).nki`
- AR The Clickest — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/AR The Clickest.nki`
- PD Saw Pulse — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/PD Saw Pulse.nki`
- PL Popcorny (MW) — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/Extra Presets/PL Popcorny (MW).nki`
- KY Baked Filter Keys — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/KY Baked Filter Keys.nki`
- KY Quantum Pluck — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/KY Quantum Pluck.nki`
- LD Frog Babble — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Frog Babble.nki`
- LD Mono Detune — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Mono Detune.nki`
- LD Retro Flute — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Retro Flute.nki`
- LD Soft Punch — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Soft Punch.nki`
- LD Wide Saw Poly — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/LD Wide Saw Poly.nki`
- PD Chorus Saw — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Chorus Saw.nki`
- PD Formant Pad — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Formant Pad.nki`
- PD Orbtron — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Orbtron.nki`
- PD Starshine — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Starshine.nki`
- PD Warp Frequency — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/PD Warp Frequency.nki`
- ST Intersection — `Kontakt1/#Non NICNT/VSTBuzz Power Move/Power Move/ST Intersection.nki`
- Bass Trombone — `Kontakt1/Session Horns Pro v1.5.0/Instruments/Solo Instruments/Bass Trombone.nki`
- Trumpet 2 — `Kontakt1/Session Horns Pro v1.5.0/Instruments/Solo Instruments/Trumpet 2.nki`
