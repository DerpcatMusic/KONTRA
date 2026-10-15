# Skreamer live control consumer

The five `ENGINE_PAR_SK_*` names were already recognized by KSP, but the color IR
and lowerer supplied only saved constants. A script write therefore had no DSP
binding. Init writes also had no Skreamer case in the Kontakt effect adapter.

The field laws and destinations are the measured rows in
`kontakt-bindings/fx_dynamics_dist_tape.tsv`: Tone, Drive, Bass, Bright and Mix are
five normalized float fields in that order, all identity-bound. This is module
`0x21`, version `0x50`, with a 20-byte parameter payload. These controls are
distinct from the wrapper's bypass and output gain.

The update evidence is bounded static instruction analysis of Kontakt8.exe
SHA256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`:

- Setter `0x140968150` updates Tone/Bass/Bright coefficients directly. Drive and
  Mix mark the gain target dirty (`0x1409682b5`, `0x140968442`).
- Wrapper `0x140aa5810` preserves a 32-frame residual count across process calls.
  At a dirty tick, it derives the gain step from the current gain and new target
  (`0x140aa58e6` through `0x140aa59f4`), using the prepared inverse ramp length.
  The existing core uses `max(round(rate * 0.0003125), 1)` control blocks.
- Core `0x140aaf220` advances those gain lanes while preserving the channel
  histories. The wrapper also samples Tone/Drive modulation at control ticks.

The implementation adds physical engine lanes, uses the existing coefficient
laws, and retains gain/filter history when values change. Each voice or bus owns
its coefficient cache and residual clock. Static units still precompute their
coefficients. No native executable code or vendor script is included here.

Authored regressions cover each knob through a KSP UI callback, successful engine
write completion, readback, meaningful PCM changes in only the addressed effect,
held voice-controller modulation versus the corresponding static knob, heap
guards, stale revision/generation rejection, init ownership, and wrapper
bypass/output preservation. Core regressions retain the existing native static
vectors and check residual-clock/retarget behavior across host block partitions.
These checks do not establish native PCM equivalence for continuously moving
controls or native callback scheduling.

Tape Saturator remains separate. Its setter `0x140ae1e10` marks two smoothing
objects for gain/roll-off and warmth, and Quality changes its oversampling mode.
The current static Tape core does not implement those transitions. Its live
control diagnostic remains; the shared engine parameter name registry alone is
not evidence of a working consumer.
