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
- Admission for a probe must go through `Runtime::trigger` (the MIDI path). The
  lower-level `note_on` admits the note without starting its attack, so `on note`
  never runs.

Lua (Luau) lanes: `play_note` in the Luau path must follow the same held-key and
group rules.
