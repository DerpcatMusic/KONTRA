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

## Corpus survey, 2026-10-22: what `migrated` can and cannot count

`articulation.migrated` counts articulations with at least one zone tagged
`ArticulationRef`. The loader never sets that tag, so the count is 0 by
construction (2959 found, 0 migrated). Where switching is owned, the runtime
already does the work:

- Native owner (switch ranges from group start criteria): each articulation's
  groups are gated by `native_key`, which a keyswitch or a velocity/channel/CC
  driver sets to that articulation's first key. Zone tags would add nothing
  here.
- Behavior owner (KSP `on note` selects groups): drivers tap the switch key into
  the script, and the script's `allow_group`/`disallow_group` masks gate the
  zones. A zone tag would silence the zone, because a behavior-owned articulation
  never sets the native articulation state.

Declarative migration therefore has no applicable population in the corpus:

- Areia (154 instruments, 2030 switches): the switch branches only set a
  selection variable; the group masks come from about 157 `allow_group($groupcount)`
  calls inside a stateful performance callback (275k-line script, developer
  presets, RR and legato state). Group selection is not a constant per key.
- Afflatus brass (102 instruments, `keyswitch script` reports): the two control
  keys are `Reset RR` and a breath key. The breath branch also plays a note after
  selecting its groups, so it is not a silent switch set. The reports are hints,
  not articulations; the script handles both at runtime.
- Shreddage (1), Measured Tremolo (2): switch ranges depend on runtime engine
  parameters or a second key set. Unmapped, reported.

The articulation report's `unrecognised` list now matches only keyswitch,
articulation and group-start reports. A bare `start` substring had also matched
KSP's "polyphonic variables start at N" warning (349 instruments).

Open: a second key set (further switch keys) would need a selector axis, and the
Behavior owner would need the native articulation state observed without
consuming the key before any zone tag can be trusted. Neither is in scope until a
corpus instrument has a declarative set that needs it.
