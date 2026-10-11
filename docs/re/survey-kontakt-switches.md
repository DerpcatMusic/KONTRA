# Survey keyswitch reports: metadata versus script execution

The source-only `survey_switch_and_conflux_intent` probe reads installed picks
2/5/6/10 and emits counts, never script text. It does not render samples.

- Areia: 1122 groups, 16 articulation choices, Behavior switching owner.
  Its single performance script has 244 allow-group calls, four disallow-group
  calls and two ignore-event calls. The remaining metadata report is an indexed
  overlay range outside MIDI 0..127, rather than failure to compile the main set.
- Solo: 357 groups, eight articulation choices, Behavior switching owner.
  Its performance script has 132 allow-group calls, 34 disallow-group calls and
  two ignore-event calls. The report identifies a further switch set, keys 0..7.
- Conflux NKM program 0: one group, no articulation choices. MIDI Player marks
  three control keys and has three set-keyrange calls and six ignore-event calls;
  no allow/disallow-group calls. The recognizer does not identify an articulation
  set in its on-note callback. A control key is not sufficient evidence that a
  key selects an articulation: this script is a MIDI player.

`keyswitch::translate` in `crates/sampler-kontakt/src/keyswitch.rs` runs a static
articulation recognizer (`detect`), independently of KSP callback lowering.
`load::compile_ui_initialized` compiles the actual scripts afterwards. These
metadata reports therefore do not prove that note switching is unimplemented.
Their reasons must remain visible until the additional switching behavior is
modelled or proven irrelevant. No diagnostic was suppressed and no synthetic
articulation set was invented.

Open: the recognizer accepts one indexed articulation range, not independent
sets or callback-derived key layouts. Extending it safely requires tracing the
saved overlay variables and the secondary set's group selections; compile
coverage alone does not prove native note behavior. Conflux's control-key player
semantics additionally require its MIDI object/global scale services. The probe
is evidence, not an acceptance test of native switching.
