# Kontakt sampler modes: corpus census (2026-10-24)

Measured with `format-census --modes <tree>` (tools/format-census). Each `.nki` in
`/data/main/Libraries/Kontakt` (11 library folders) is read and streamed-loaded through the
shipped reader; every zone and loop slot is tallied by mode. 682 files loaded, 2 failed
(Conflux, Shreddage: not installed/licensed here). Raw receipt: `vectors/kontakt-zone-modes-census.tsv`
(columns: zones, distinct libraries, key). Library names are not recorded.

## Ranking (by libraries using the mode)

| Mode | Zones / slots | Libraries | Engine state |
|---|---|---|---|
| Loop slots with crossfade | 1,329,529 | 8 | Crossfade implemented; law is **linear** (`lower.rs` `LoopShape::Crossfade`). Kontakt's shape is **unmeasured** (see below) |
| Loop slots, plain | 86,692 | 8 | Implemented |
| Until-release slots | 420 | 1 | Implemented, native-ordering unmeasured |
| Alternating (ping-pong) slots | 84 | 1 | Direction implemented; crossfade law unmeasured (`UnknownLaw`) |
| Counted (pass count) / tuned slots | 0 | 0 | Not in this corpus |
| Gate-release trigger | 319,986 (+56 slot-looped) | 5 | Implemented; monophonic release cut implemented (`steal.rs`) |
| Round-robin group start | 1,050 (+50 gate-release, +3 slot-looped) | 1 | Implemented |
| Time Machine Pro | 3,420 | 1 | Speed carried; Keep Formants / Formant Shift / Envelope Order carried but unused |
| Time Machine 1/2, Tone/Beat/S1200/MP60 machines | 0 | 0 | Not in this corpus |
| Key / CC group start | 0 | 0 | Keyswitch selection is articulation-based in these libraries |

Multi-slot loops (2 to 8 slots) appear in about 10 to 20k zones across 1 to 2 libraries each.

## Gap that matters most: loop crossfade law

The crossfade is used by 1.33 M loop slots across 8 libraries, so it is the most audible loop
behaviour in the corpus. KONTRA uses a linear complementary crossfade (`docs/architecture-v2/MODULATION.md`
records linear as measured for zone fades, not for loop crossfades). The engine already has
`EqualPowerCrossfade`, which fits uncorrelated material such as ensembles, but no measurement says
which law Kontakt uses for loop seams. Choosing it without a native vector would break the
evidence rule, so the engine is unchanged.

Next step to settle it: a loop-seam probe. Sustained note from an instrument with a crossfaded
slot (chosen from `vectors/kontakt-zone-modes-census.tsv`), oracle capture under
`flock /tmp/kontra-oracle.lock`, then fit the seam gain curve (linear vs equal-power vs other)
against the captured seam. A separate probe is needed for alternating-loop crossfade (84 slots).

## How to reproduce

    format-census --modes /data/main/Libraries/Kontakt > modes.tsv

The run loads every instrument and takes about 30 minutes.
