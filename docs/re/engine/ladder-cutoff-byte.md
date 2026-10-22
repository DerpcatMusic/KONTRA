# Ladder cutoff-path byte (flag 0x49) and HQ byte (Kontakt 8)

Status: the flag source is decoded for every Ladder record version the importer admits
(v0x90..0x92 and v0x95). Corpus gap 6 ("filter law") was 56 instruments, 72 of the 78 Ray Line
refusals being Ladder LP4. All twelve Ladder selectors are now Exact (see Gap table). Evidence is the Kontakt 8
decompile export and the `.rdata` tables; the binary was not re-opened.

## Record bytes (reader 0x140d00db0, BParFXFilter, container offset 0xA00)

The reader writes two single bytes into the record: HQ at +0x28 and the cutoff-path flag at +0x29.
Both are read from the stream only where noted. The stream is kind (4 bytes), byte, kind, float...
(float and int are 4 bytes, little-endian on the file).

| record | stream for a Ladder type (ids 22..33) | HQ (+0x28) | cutoff path (+0x29, flag 0x49) |
| --- | --- | --- | --- |
| v0x90, v0x91 | kind, kind, float | not read (record default) | constant 1 |
| v0x92 (Conflux) | kind, byte, kind, leading, cutoff, resonance | byte 1 | constant 1 |
| v0x93, v0x94 | as v0x92 for Ladder ids | byte 1 | constant 1 |
| v0x95 (Morphology) | kind, byte, kind, kind, kind, byte, kind, floats | byte 5 | byte 1 |

Why the constant 1: the reader's switch falls through for 0x90..0x94. For Ladder ids the case
body ends in a `break` to the shared tail, which unconditionally writes +0x29 = 1 and returns.
Only the v0x95 case returns from inside its own block, so it keeps the stream byte. The v0x95
block reads the cutoff byte first (after the first kind) and HQ second (after the fourth kind).

Correction: earlier notes said "v0x95 first flag = HQ". The stream order says the first flag is
the cutoff path and the second is HQ. The Morphology test bytes (`kind, 0, kind, kind, kind, 0,
kind, floats`) fit this layout.

The apply path (0x1409619b0) reads the record's +0x28 into the core's HQ setter argument and the
record's +0x29 through wrapper 0x140aefe50 into the core byte at +0x49. The wrapper is 12 bytes
and tail-jumps to the store 0x140b04a20 with the second argument unchanged, so the byte reaches
the core as written.

## Core behaviour of flag 0x49 (cutoff path)

- Core cutoff setter (0x140b00b80): flag 0 reads the ramped lane block at core+0x4fc0 (direct
  state, 4-frame cadence). Non-zero reads the lane block at core+0x5010 instead.
- Target setter (0x140b04d60): stores the normalized target at +0x501c, sets +0x5020 (armed),
  and sets the lane's coefficient at +0x5018.
- Lane state machine (0x140b036a0, one call per 32-frame tick). Armed branch: step =
  (target - current) * +0x5030. The step rate +0x5030 is zero: the core init (0x140afba80) writes
  0 there, and nothing else writes it. If the step squared is below the epsilon at 0x144fc3060
  (1e-15), the lane snaps: current = target, count = 0.
- With the step rate at zero the test always snaps. So flag 1 means: each new cutoff target is
  reached on the next 32-frame tick. Kernel: `Ladder::set_cutoff_path(true)` (modes bit 0),
  lane 1 snaps in `clock()`.
- Constant check: the core init writes +0x5018 = -1.0 (0xbf800000) and it is overwritten by the
  target setter before use.

## Gain, HQ, and the other Ladder laws

Unchanged from `filt-ladder-monark.md`: cutoff table and index, pole polynomial, feedback 4.16,
clip, gain law, tap weights, and the shared 2x Kaiser for HQ. The flag-0 steady and moving-cutoff
vectors (`ladder_kemu_vectors.json`) still pass.

## Engine mapping

- `sampler_ir::fx::filter::Config::cutoff_path` and `LadderLP4::cutoff_path` replace the version
  proxy `instant_cutoff(version)` and the `Binding::record_version` field.
- `sampler_core::LadderSettings::cutoff_path` drives `Ladder::set_cutoff_path` in the kernel.
- `sampler-kontakt/src/effects/mod.rs` returns `Params::Filter { hq, cutoff_path }` per
  version: v0x90..0x92 from the record reader (cutoff path true), v0x95 from the first and
  second flags, other Ladder layouts as v0x92 with cutoff path true.
- `skip_repeats` returns every flag in stream order (it returned only the first).

## Gap table

| item | engine has | status |
| --- | --- | --- |
| flag 0x49 source byte, v0x90..0x95 | per-version rule above | decoded (static, decompile + stream order) |
| v0x95 HQ / cutoff-path order | second flag = HQ, first = cutoff path | corrected (was first = HQ) |
| flag-1 cutoff law | snap on the 32-frame tick | decompile-confirmed (init 0x5030 = 0, epsilon 1e-15, 0x140b036a0 snap branch); kemu PCM with flag 1 (LP4 steady and moving, BP4 moving, HQ on moving) matches |
| LP4, HQ off | kernel + flag law | Exact: steady knobs bit-exact (84 native cells), moving cutoff 1-2 ulp (kemu, flag 0), flag 1 (kemu) |
| LP4, HQ on | shared 2x Kaiser | Exact (kemu): steady, moving and flag-1 moving match. HQ controls sample once per 32 host frames, as the native host does (per-frame sampling only differs when a control changes inside a block) |
| BP4, HQ off | kernel + taps 10 | Exact (kemu): steady, moving and flag-1 moving |
| BP4, HQ on | shared 2x Kaiser | Exact (kemu): steady, moving, flag-1 moving |
| LP1..LP3, HP1..HP4, BP2, Peak, Notch | kernel, taps 1..3, 5..8, 9, 11, 12 | Exact (kemu, modes 0,1,2,4..8,10,11): moving cutoff flag 0 within 2e-7 of peak; flag 1 and HQ on match. Steady knobs from the static PCM grid |
| v0x93 / v0x94 Ladder records | reader read, importer does not admit | open (no corpus record checked) |

## Open

1. Live Kontakt check of flag 1 and HQ on: the vectors come from the native core under kemu
   (no Wine render). A Wine render of a v0x92 Ladder LP4 with a moving cutoff would confirm them
   against the host, and is still open.
2. Host cadence of HQ controls rests on the decompile of the host caller (0x1408faa30: setters and
   tick once per 32-frame chunk). Not yet confirmed by a host render.
3. Admit v0x93/0x94 records if a corpus record needs them.

## Evidence (Kontakt 8 decompile export, `source/kontakt8`)

- Reader 0x140d00db0 (file 0140d00000.c): case 0x95 stream order and the shared tail
  (`+0x29 = 1`, end of function).
- Stream reads: int 0x142a78270, byte 0x142a780c0, float 0x142a780f0 (0142a70000.c).
- Container 0x140701a80 embeds the BParFXFilter at offset 0xA00 (0140700000.c).
- Apply 0x1409619b0 (0140960000.c), wrapper 0x140aefe50 (0140ae0000.c, 12 bytes), store
  0x140b04a20 (0140b00000.c).
- Lane code: 0x140b00b80 (flag branches), 0x140b04d60 (setter), 0x140b036a0 (state machine),
  0x140afba80 (core init, zeroes 0x5010/0x5030).
- Constant 0x144fc3060 = 1e-15 (`.rdata`, `constants.tsv`).
- Kernel tests: `native_ladder_legacy_cutoff_snap_retains_gain_resonance_clock`,
  `ladder_kemu_vectors` (flag 0), `ladder_original_static_pcm_grid` (flag 0).
