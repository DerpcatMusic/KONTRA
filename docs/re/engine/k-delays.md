# Kontakt 8 delays: Legacy Delay, Twin Delay, Psyche Delay, Replika Delay

Status: gap analysis and state of the shared engine. No DSP changed in this unit.
Kontakt 8 decompiled C (`source/kontakt8/READY`) was not present when this was
written, so every constant below is either from the repo's existing evidence
(binding TSVs, reader schemas, earlier kernel notes) or marked unverified.

## Shared core: is twin/dual/mt one delay core?

Partly. The shared core is `sampler_core::dsp::timemod` (`mod.rs`) and its config
`sampler_ir::fx::timemod::Config`:

- **Shared (one core):** a `Line` with up to `MAX_TAPS` taps (time as frames, ms
  or beats; depth; pan; level), a 2x2 feedback matrix and input matrix, a
  voice/voice-spread layer, an LFO, an optional allpass cascade, dry/wet. Kontakt
  Chorus, Flanger, Legacy Phaser and the generic Delay are configs of this.
- **Not one core:** each product delay is a `Native` variant with its own
  process method, ring size and state. Twin, Psyche, UVI Dual, Velvet, Buzz, Tape,
  Diffuse and Legacy Delay all dispatch from `TimeMod::process` (`mod.rs:464-493`)
  and do not reuse each other's state.
- `mt.rs` is not a delay. It is MT19937 plus MSVC `generate_canonical`, used for
  UVI's random draws. It is shared random-number code only.

Per kernel:

- **Twin** (`twin.rs`): a 4-lane ring (line L, line R, predelay L, predelay R) with
  linear fractional reads and cross feedback `fb*y + cross*y_other`. This is
  expressible as a `Line` with a cross term in the feedback matrix, two predelay
  taps and `Interpolation::Linear`. Consolidating it would be a real refactor, and
  it must first be shown bit-identical against `twin.rs` vectors. Not done here.
- **Psyche** (`psyche.rs`): a grain pitch shifter with two crossfaded heads per
  side and a 6-tap cubic read stretched by `1/r`. It is not a delay-line
  configuration and cannot be collapsed into `Line` without losing the grain
  behaviour. Keep it as a kernel of the shared module.
- **UVI Dual** (`dual.rs`): block-rate modulation with rotation and width matrices
  around a ring. Its matrices could map to the feedback/input matrices, but the
  per-block glide and the `min(64, ...)` block rule are not in `Line`. Not
  consolidated.

Conclusion: the twin/dual/mt variants are not settings of one core. The honest
consolidation target is Twin into `Line` (cross feedback matrix, predelay taps),
gated on a bit-exact test. Psyche and Dual stay kernels.

## Parameters and evidence

Binding TSVs: `docs/re/kontakt-bindings/fx_legacy_delay_reverb_mod.tsv`,
`fx_bite_dirt_freak_wowflutter_twindelay.tsv`,
`fx_raum_delight_psyche_vibrato_phasis_replika.tsv`. Readers:
`docs/re/kontakt-catalog/reader_schemas.tsv` (lines 192-194, 202, 210).

### Legacy Delay (module 0x10, versions 0x50/0x51)

| Parameter | Stored field | Law (repo evidence) |
|---|---|---|
| DL_TIME | f32 @8 | ms, or synced note count `round(1 + 11u)`, 1..12 (table) |
| DL_TIME_UNIT | f32 @ unit field | sync unit factor (table: 5:4, 6:2.667, 7:2, 8:1.333) |
| DL_FEEDBACK | f32 @12 | stored x 0.98 |
| DL_DAMPING | f32 @76 | one-pole mix, stored x 0.7 |
| DL_PAN | f32 @80 | pan crossing, ping-pong ring of 2^18 frames |

Implementation: `sampler-kontakt/src/effects/timemod.rs:280-294`,
`sampler-core/src/dsp/timemod/mod.rs:801-841`. Synced delay is
`unit * 60 / tempo * time * 1000` ms, clamped 0..5800.

### Twin Delay (module 0x6B, version 0x10)

Parameters: TDL_TIME_L/R (+unit), TDL_PREDELAY_L/R (+unit), TDL_FEEDBACK_L/R,
TDL_LEVEL_L/R, TDL_CROSS_FEEDBACK, TDL_WIDTH. Reader
`read_twin` (`effects/timemod.rs:163`). Laws: `twin.rs` header. Sync via the
shared getter.

### Psyche Delay (module 0x63, version 0x10)

Parameters: PSYDL_TIME (+unit), PSYDL_FEEDBACK, PSYDL_DETUNE, PSYDL_DETUNE_STEREO,
PSYDL_PITCH, PSYDL_LR_OFFSET, PSYDL_CROSS_FEEDBACK, PSYDL_REVERSE,
PSYDL_REVERSE_STEREO. Reader `read_psyche`. Laws: `psyche.rs` header. Reverse and
detune-stereo flags are used (`psyche.rs:82-84`).

Note: the PSYDL_TIME_UNIT table has three field placements (fields 0, 9, 10 in
the TSV). The engine reads the single stored factor the saved file keeps. The
other two rows look like the UI index-to-factor mapping, which the file does not
store. Confirm against the decompiled setter before treating this as closed.

### Replika Delay (module 0x5A, version 0x12)

Parameters (53 controls in `fx_raum_delight_psyche_vibrato_phasis_replika.tsv`
and `RDL_*` rows): TIME (linear 0..2000 ms, unit table), FEEDBACK, LOWCUT,
HIGHCUT, SATURATION, TAPEAGE, FLUTTER, DEPTH, BBDTYPE, QUALITY, RATE, AMOUNT,
MODULATION, SIZE, DENSE, FEEL, ACCENT, SHUFFLE, DUCKING_SENSE/RELEASE/AMOUNT,
PINGPONG and PINGPONG_FLIP, STEREO, WIDTH, PAN, LR_OFFSET, NOISE, TYPE.

Repo evidence: `kontakt-bindings/timemod-reads.tsv` (sync factor table is
proved for Replika), `KONTAKT_DSP_LAWS.md:297` (outer wrapper, 53 controls),
`KONTAKT_DSP_LAWS.md:466`. Setter laws known: time 8/9, type 28, gains 0/1, mix 4.
Not extracted: the saved-field to core-index binding, the line read/write
recurrence, the line filter, saturation, BBD type tables, ducking and the
stereo modes.

## Gap table

| Module | Engine covers | Missing / unverified | Action |
|---|---|---|---|
| Legacy Delay | Time (ms and sync), feedback, damping, pan ping-pong, 2^18 ring, native kernel | Constants not confirmed in decompiled C; v0x51 extra fields unread | Confirm on READY; keep existing native kernel |
| Twin Delay | Full native kernel, reader, sync, cross feedback, width | Not consolidated into `Line`; no oracle vector cross-check in this unit | Candidate for consolidation after bit-exact test |
| Psyche Delay | Full native kernel, reader, sync, reverse, detune stereo, reverse stereo | TIME_UNIT placements unresolved (see note) | Confirm on READY |
| Replika Delay | Nothing: `unproved(0x5a)` emits `Reason::NotModeled` (`effects/timemod.rs:237`) | Entire core: line, filter, saturation, BBD, ducking, stereo, pingpong | Blocked on decompile. Do not fabricate. |
| UVI Dual / Velvet / Buzz / Tape / Diffuse | Native kernels (UVI side, out of scope) | Not in this unit | None |

## Blockers and next steps

1. Replika needs the Kontakt 8 decompile (`source/kontakt8/READY`) or a Wine
   oracle measurement of its line and ducking behaviour. Until then it stays
   diagnosed and unmapped, which is the correct state.
2. Confirm Legacy and Psyche constants against decompiled C once READY appears.
3. Twin into `Line` only with a bit-exact golden test against `twin.rs`.

## Replika oracle measurement (Wine, `tools/kontakt-oracle/stimrender.py`, 2026-10-21)

Two impulse captures of `Effects/RDelay` (INIT preset). Result: **not usable for
the feedback, time, cut, saturation or ping-pong laws.**

- TYPE is applied: the Tape preset adds an early tap near 27 ms, the other four types
  are indistinguishable from each other.
- RDL_TIME 0.25 and 0.5 and RDL_FEEDBACK 0, 0.5 and 1.0 leave the echo train identical
  (repeats at ~500, 1005, 1500 ms, gain 0.7182 per repeat). Either these writes do not
  reach the wrapper in this harness, or the core ignores them. Unresolved.
- RDL_HIGHCUT, RDL_LOWCUT, RDL_SATURATION and RDL_PINGPONG show no change in the first capture.

Mapped, with evidence level:
- RDL_TIME free: ms = 2000 x stored, **documented** (binding TSV, "linear a=0 b=2000").
  Not confirmed by the oracle (see above).
- Everything else is reported as `NotModeled`. No constants are fabricated.

Next step for the oracle: check the `set_engine_par` path for Replika (generic flag,
parameter index) with a known-good control such as the Legacy Delay DL_TIME, then redo
the capture.

## Replika core: setter binding (decompile, 2026-10-21, binding only)

Source: `FUN_141ac5980` (`0141ac0000.c`, size 3979), the Replika core parameter
setter. Core index `i` is not the saved stream field; index `i` writes to a
control block at `*(core+200)` plus `+0x10 + 4*(i-2)` for the plain linear ones. Verified
from the switch only (no core DSP read yet):

| Core index | Write | Kind (from the switch) |
|---|---|---|
| 0, 1 | `FUN_141a90910` conversion to control block +0x0 / +0x4 | gains (A, B) |
| 2, 3 | linear to +0x10 / +0x14 (live path calls `FUN_141acc980`/`FUN_141acc930`) | linear |
| 4 | equal-power style crossfade: v > 0.5 → +0x8 = 2(1−v), +0xc = 1; else +0x8 = 1, +0xc = 2v | mix |
| 5 | linear to +0x18 (`FUN_141ac9420`) | linear |
| 6, 7 | bool at core +0x914 / +0x915 (v ≥ 0.5) | flags |
| 8 | if mode 3: gain ×`DAT_14756bdf8`-clamped into +0x2c; else `FUN_141a9d4a0`/`9d7c0`/`9d660` | time / sync (not yet read) |
| 9 | integer 0..3 at core +0x93c (toggles `FUN_141acbfb0`, bypasses three sub-objects at +0xac0, +0x6380, +0xbc40) | mode |
| 10, 11, 12, 15 | toggles at core +0x14381 / +0x14382 / +0x14383 / +0x14384 (`FUN_141acc600`), 10/11 also write +0x30 / +0x34 | section enables |
| 12 | three sub-objects (+0xac0, +0x6380, +0xbc40) set from one law `FUN_141abba60` (filter-like) | three-section split |
| 13 | `pow(...)` law into core +0x13e28 | unknown |
| 25 | +0x1d ... | `FUN_141accd10` |
| 0x1c, 0x2b, 0x22 | flag combine then `FUN_141aca670` / `FUN_141acb2b0` / `FUN_141abbf60` | section updates |

Not yet read: the audio functions (`FUN_141abd210`, `FUN_141acc600`,
`FUN_141acc980`, `FUN_141a9d4a0`, `FUN_141a9d660`, `FUN_141a9d7c0`,
`FUN_141a90910`), the three sub-object filter laws (`FUN_141abba60`), and the
line read/write recurrence. The ducking, saturation, BBD and stereo laws are in
those. Confidence: the switch structure is high; the meaning of each index is medium
until the called functions are read.

## Legacy Delay: decompile check (2026-10-22)

Verified against the decompile, not measured:
- Audio kernel `FUN_140aa1130` (stereo branch, `0140aa0000.c`): ring L = in_L·g,
  ring R = (1−pan)·g·in_R, then cross terms g·pan·lp_R into L and (1−pan)·g·lp_L
  / g·pan·lp_L into R, with lp one-pole `lp += (ring[read] − lp)·c`. Matches
  `legacy_delay` in `sampler-core/src/dsp/timemod/mod.rs`.
- Setter case `0x43` in `FUN_1409619b0` (`0140960000.c:2570`): frames = int(time·rate·0.001), clamped to
  0x40000 (2^18), c = 1 − damping·0.7, g = feedback·0.98. Matches `timemod/effects` mapping.
- Sync getter `FUN_140a74bf0` (`0140a70000.c:3963`): ms = (unit·60 / tempo)·count·1000, tempo < 0.1 → 120,
  count ≥ 1 by default (flag +0x20 set in `FUN_140a723e0`), clamp to [0, 5800].

Constants read from the Kontakt 8 PE image (`.data`, not decompilation): 0.1, 60, 120,
1000, 0.001, 0.98, 0.7 (`144714xxx` / `144f38xxx` addresses). Verified.

Not verified: a native parameter vector. The Wine oracle capture with five Legacy Delay
steps (`legacy1.npz`) returned identical echoes in every step, including the 500 ms
preset when DL_TIME was set to 140 ms. The harness does not change float delay
parameters, so no parameter vector can be taken with it yet.

Matrix status: partial / decomp.
