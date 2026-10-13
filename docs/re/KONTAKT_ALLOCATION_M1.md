# M1 allocation implementation evidence (2026-10-13)

This package implements the documented allocation scopes, retaining the existing
victim law. It does not claim a recovered native allocator or measured CPU parity.

## Vendor contract

Kontakt 8 manual, [Classic view](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view),
checked 2026-10-13:

- Group Editor → Voice Groups: a voice group is an allocation setting shared by
  authored groups within an instrument; each playing sample takes a voice.
- Voice Groups → Excl.Grp: domains sharing an exclusive set cut all sounding
  samples in the *other* voice domains. A domain does not exclude itself.
- Voice Groups → Fade Time: fading victims can temporarily exceed the voice
  limit. They must leave the logical count when stolen, and remain members until
  their fade ends.
- Source Module → Sampler → Release Trigger / Monophonic: note repetitions cut
  previous release samples. Dense group IDs must be qualified by their instrument
  plan generation, and all previous layers must be considered.
- Instrument Options → Voices Handling → Time Machine Pro: standard and HQ TMP
  have separate budgets independent of instrument polyphony. The IR/runtime now
  permit a second source-resource domain alongside the voice-group domain.

The Any and Pref.Rel selection rules remain unchanged. The manual's released-note
priority differs from the existing implementation; resolving it requires the
labeled native probe specified in KONTAKT_TOPOLOGY.md, not an inferred correction.

## Binary and v1 checks

Binary SHA and recovered cache paths are pinned in
[KONTAKT_TOPOLOGY_EVIDENCE.md](KONTAKT_TOPOLOGY_EVIDENCE.md), B13. The recovered
`kontakt-engine/vtable-methods.csv` identifies BVoiceLimit slots 1, 2, 8, 9 at
0x140611850, 0x1406118f0, 0x140d049b0, 0x140d13370 respectively. The full
pseudocode export is absent; identity alone supplies no victim law.

Direct radare2 instruction reads on the same executable confirmed:

- 0x140611850 returns serialization type 0x2b.
- 0x1406118f0 returns version 0x60.
- Reader 0x140d049b0 checks version 0x60, reads the name, then stores the 16-bit
  kill mode at +0x2c, byte preference at +0x30, and successive 32-bit values at
  +0x28 (count), +0x34 (fade), +0x38 (exclusion). This confirms the existing
  decoded field order; it does not prove victim ordering or the exclusive fade.

Replay with `r2 -q -e scr.color=false -e bin.relocs.apply=true` on
`/mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe`, using
`pd 32 @ 0x140d049b0`, `pd 2 @ 0x140611850`, and `pd 2 @ 0x1406118f0`.

Our v1 commit 0cb7a8a0, `src/engine/mod.rs:1992` (`make_room`), implements
cross-domain exclusive cuts using the victim domain's saved fade. M1 retains
that policy. The existing monophonic 10 ms fade is also retained; the manual
specifies the cut, not its numeric duration. Neither fade policy is newly
certified as a native measured law.

## Limits still open

The native allocator victim algorithm and exact exclusive/monophonic fade need
live probes. No DSP kernel or script builtin law changes in M1. Synthetic labeled
constant-source tests validate scope, complete cuts, fade overlap and domain
accounting without asserting native PCM parity.

The Kontakt importer has no decoded TMP standard/HQ budget fields or verified
source quality membership. Program suffix data remain opaque, and TMP itself is
still an unsupported source mode (T04). M1 adds executable semantic source
budgets, but does not guess persisted offsets or suppress source diagnostics.
A format/source worker must recover and bind those fields when TMP is admitted.
