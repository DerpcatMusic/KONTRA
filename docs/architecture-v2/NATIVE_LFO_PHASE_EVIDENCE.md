# Saved LFO phase at a modulated onset

A represented `SourcePhase` route started its accumulator at zero after voice
initialization had already seeded the saved `Lfo.phase`. A zero-depth route
therefore discarded a nonzero saved phase. The correction starts that sum from
the seeded phase, preserving route order and the existing arithmetic.

## Original-code evidence

The reference is Kontakt 8.13.1 `Kontakt8.exe`, 178,386,944 bytes, SHA-256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
Private UNPACER 0.3.6 and REA 6.5.0/Ghidra 12.1.4 runs verified the original
bytes underlying the analyzed getter, binder and builder bodies: 3,977, 4,157 and
19,370 bytes respectively, with zero synthetic body bytes. This establishes byte
provenance, not the correctness of every decompiler inference.

The observed path is:

- Getter `0x1408efd60`, parameter 180, reads the owning physical source's saved
  float32 phase when its phase capability is enabled.
- Builder block `0x140977dc5..0x140977de0` preserves that float32 value and expands
  it to float64 reset state. The examined conversion has no additional codec.
- Emitter `0x140873040` combines the incoming base with the processed source times
  intensity. Sixteen captured template cases retain the source-domain operations
  as opaque operations; they do not prove a new inversion or normalization law.
- Binder `0x1408f1260` resolves the physical source and freshness stamp and binds
  an absolute phase lane independently of the frequency lane.
- Reset `0x1405f45a0` restores saved phase. Digital consumer `0x140b078d0` consumes
  the absolute phase lane on its first tick and ignores later phase-lane changes
  until another reset. It does not add the saved phase a second time.

Unchanged original instructions passed 128 getter cases, eight builder conversion
cases, 32 freshness/binding cases and 420 reset/consumer cells with three calls
each. Fragments were 1, 3, 17, 32 and 127 native control ticks. No DSP, arithmetic,
sine or lifecycle helper was substituted in these numeric probes. Cycle state
matched the independent instruction-order model exactly; waveform error relative
to mathematical sine was below `2e-6`. Failed draft negative-phase oracles were
retained and corrected to include the original return helper's negative fold.

## Validation status

Two authored unit regressions failed on unchanged code: saved phase `0.125` with
depth zero produced zero; saved phase `0.375` with depth `0.125` produced `0.125`
instead of `0.5`. Two additional exact-PCM regressions also failed before the fix.
All four failures and source/binary hashes are retained in the private RED receipt.

After the one-line correction, all 16 modulation unit tests and all 13 modulation
router tests pass in the optimized `ci` profile: 29 unique tests, with no failures.
The new checks cover literal original onset vectors, positive/negative offsets,
wrap boundaries, poisoned history, public depth/bypass readback and onset-only
consumption, late notes, released slot reuse and partitions of 1, 7, 31, 64, 127
and 256 frames. Runtime PCM matches independent no-route reference plans exactly;
event/control/render operations run under the existing zero-allocation guard.
The no-phase-route PCM checksum is `9a85afec958abbf1` before and after the fix for
all six partitions. These are correctness checks, not performance measurements.

## Scope

This correction changes an already represented core route. It adds no Kontakt
source aliases, saved-bypass admission, free-running admission or new waveform
law. Tests use retriggered per-voice owners. The native probes use synthetic leaf
objects; full factory lifetime, host retrigger/free-running scheduling, native
bypass graph removal and host partition compensation remain unverified. Native
control ticks are not audio frames. Float32 native lane storage is observed, but
the compiler expression's intermediate precision is not established; this change
preserves the existing float64 route operations rather than inventing rounding.

Private numeric receipts and harnesses reside under
`/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-native-phase-20261015`.
No vendor source or executable payload is committed.
