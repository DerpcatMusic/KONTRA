# KSP semantics: held keys and `play_note`

Scope: what the interpreter does for scripts that gate on held keys. Verified by
`crates/sampler-kontakt/tests/pacific_trills.rs` on Pacific Ensemble Strings
Trills (16 Violins, Kontakt-compiled script `perf063`).

- `%KEY_DOWN[k]` is 1 only for a host input note on key `k` that is still held,
  from the same plan (`ReadKeyHeld`, `crates/sampler-core/src/behavior.rs`).
- `play_note(note, velocity, offset, frames)` creates a child event of the
  current `on note` event. The child is routed through the normal zone and group
  selection, so `allow_group`/`disallow_group` apply to it.
- The velocity argument is the raw note velocity (0..127). Scripts may pass a
  non-velocity value there: the trill script passes its interval index (1 or 2),
  so the child plays at velocity 1/127 or 2/127. Whether Kontakt applies the same
  velocity-to-level curve for such children is not yet checked against Kontakt.
- Velocity 0 is clamped to 1 and the child sounds; it is not refused. Evidence:
  Vista's `on release` plays its release samples as `play_note($EVENT_NOTE, $xfy15)`
  and `$xfy15` is 0 when CC1 was never received. Refusing it killed the release
  tail (no PlayMidi in the release window; forcing velocity restores the tail). A
  repeated same-key legato note (`$1e5yd = 0`) also plays velocity 0 and was silent
  while Kontakt sounds it. The Kontakt manual page for `play_note` was not
  reachable (HTTP 403), so the documented range is unverified; the clamp follows the
  captured Kontakt behaviour. Key outside 0..127 is still refused (returns -1).
- Admission for a probe must go through `Runtime::trigger` (the MIDI path). The
  lower-level `note_on` admits the note without starting its attack, so `on note`
  never runs.

Lua (Luau) lanes: `play_note` in the Luau path must follow the same held-key and
group rules.
