//! The KSP the fixtures author, one script per audited pattern.
//!
//! The native creator wrote only a round-robin script; these sources carry what
//! the audited libraries do in `on init`/`on note` and the creator cannot express
//! (see [`Fixture::gaps`](super::Fixture::gaps) for what remains missing).

/// Areia-style: ignore the played note, then start one close, room and far voice
/// per key. Each group's CC113 sample-start route (see `Fixture::CcStart`) sets the
/// offset, so the script only chooses the round robin and mic group.
///
/// Groups are `round * 3 + mic`, mic order close, room, far.
pub(super) const CC_START: &str = r#"on init
  set_script_title("Probe CC start")
  declare $rr := 0
  declare $mic := 0
  declare const $MICS := 3
  declare const $ROUNDS := 4
end on

on note
  ignore_event($EVENT_ID)
  $mic := 0
  while ($mic < $MICS)
    disallow_group($ALL_GROUPS)
    allow_group($rr * $MICS + $mic)
    play_note($EVENT_NOTE, $EVENT_VELOCITY, 0, -1)
    inc($mic)
  end while
  $rr := ($rr + 1) mod $ROUNDS
end on
"#;

/// Dense chords: one suppressed attack and one `play_note` per played key.
///
/// Groups are one round robin each, so the script picks the group of the round.
pub(super) const DENSE: &str = r#"on init
  set_script_title("Probe dense")
  declare $rr := 0
  declare const $ROUNDS := 4
end on

on note
  ignore_event($EVENT_ID)
  disallow_group($ALL_GROUPS)
  allow_group($rr)
  play_note($EVENT_NOTE, $EVENT_VELOCITY, 0, -1)
  $rr := ($rr + 1) mod $ROUNDS
end on
"#;

/// Areia's effect traffic: 300 `set_control_par` writes per note through a
/// variable holding `get_ui_id` (the lowering only folds a direct knob
/// reference), plus one `message`. The original attack is not ignored, so the
/// note sounds only if the callback completes.
pub(super) const UI_LOAD: &str = r#"on init
  set_script_title("Probe UI load")
  declare ui_knob $gain(0, 100, 1)
  declare $id := 0
  declare $write := 0
end on

on note
  $id := get_ui_id($gain)
  $write := 0
  while ($write < 300)
    set_control_par($id, $CONTROL_PAR_VALUE, $write mod 100)
    inc($write)
  end while
  message("probe ui load")
end on
"#;
