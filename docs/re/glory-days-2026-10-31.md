# Glory Days – Big Band Horns: the failure taxonomy, and the one law it broke

Written 2026-10-31 on `feat/glory-days`. Everything here was produced by a
command on this machine; the commands are given so the numbers can be re-run
rather than believed. `STATUS.md` is the page of truth for what is true now.

Library: `~/Armory/VSTs/Kontakt1/Glory Days - Big Band Horns/`
— 643 `.nki` (49 under `Multi Instruments/`, 594 under `Single Instruments/`),
203,091 `.ncw`, 13 sections (trumpets 1–4, trombones 1–3, bass trombone,
altos 1–2, tenors 1–2, baritone).

---

## 1. The taxonomy, per `.nki`

### Load — no failures at all

`kontra-inspect load` on all 643 files:

| | |
|---|---|
| refused | **0** |
| `init unanswered` | `[]` on **643** |
| `diagnostics` | 0 on 641; 1 on 2 (both Trombone 2, `unassigned external modulator` ×11, impact *Cosmetic*) |
| wall clock | median **0.19 s**, p95 0.64 s, worst **2.36 s** (`Tenor Sax 1 Multi`, 27,996 samples / 696 groups), **175 s for the whole library** |
| scripts | 2 slots on the 594 singles, **5** on the 49 multis (all named `C.A.P.S.U.L.E.`) |
| unprepared zones | one reason only, everywhere: `sample_only_referenced_by_purged_groups` — saved purge state, not a parse gap |
| faults | every instrument faults `out_of_range` 5× in slot 0 (`(101, true, 32)`, a text-array read) and 5× in slot 1 (`(170699, false, 4096)`). Identical across the library; **open**, §5 |

### Sound — before the fix

`kontra-render --key 60 --hold 1 --tail 1`, once with CC1 at its default and
once with `--wheel 127`, on all 643:

| class | default CC1 | CC1 = 127 |
|---|---|---|
| silent (peak ≤ −199 dBFS) | 3 | 2 |
| near-silent (−199 … −60) | 27 | 18 |
| quiet (−60 … −30) | 96 | 95 |
| normal (−30 … −3) | 516 | 527 |
| loud (> −3) | 1 | 1 |

**9 of the 49 multis were silent at CC1 = 0 and sounded at 127** (Trumpet 1/2/3,
Trombone 1/2/3, Bass Trombone, Tenor Sax 1/2) while others of the same library
(Trumpet 4, Alto Sax 1/2, Baritone Sax) sounded at 0. That inconsistency is what
made it a bug rather than an instrument quirk.

### Sound — after the fix

| class | default CC1 | CC1 = 127 |
|---|---|---|
| silent | 0 | 2 |
| near-silent | 0 | 0 |
| quiet | 97 | 95 |
| normal | 545 | 545 |
| loud | 1 | 1 |

The two remaining "silent" files are **not** silent instruments: `20. Trumpet 1
Shakes` and `20. Trumpet 3 Shakes` answer nothing up to key 62 and sound from key
64 up (−11.6 dBFS at 64, −5.8 at 84). A shakes articulation starting above the
horn's low register. Key 60 was the wrong key, which is the trap the brief warned
about.

The 95 "quiet" files are mute and articulation variants (Bucket, Cup, Harmon,
Plunger, Staccatissimo). Three were checked against Kontakt and are **genuinely
quiet**: `Trumpet 1 Bucket Legato + Doits` −29.77 vs Kontakt −29.774,
`Trombone 2 Bucket Staccatissimo` −31.36 vs −31.362.

The one "loud" file is `14. Trombone 2 Sforzando Long` at −2.84 dBFS peak
(RMS −31.3): an accent whose peak is a transient, not clipping (0 clipped
samples).

### Slow — the 203,091 `.ncw` files are not a cost

`load` is 0.19 s at the median because nothing enumerates the sample folder:
KONTRA references zones and opens files only when a voice needs them. A full
12-second scenario on `01. Trumpet 1 Multi` reads

```
StreamStats { pages_read: 2994, read_errors: 0, requests_refused: 0,
              bytes_read: 18340620, cache_bytes: 134217728 }
```

— 18 MB through a 128 MB cache for 4,392 prepared zones. The I/O risk in the
user's notes is real for Kontakt (which stats the folder); in KONTRA it is not a
load cost and not a render cost.

---

## 2. What was wrong: the PGS key table stopped at 64

### The symptom

`01. Trumpet 1 Multi`, one note key 60 vel 100, CC11 (expression) at 127 and
CC1 (the instrument's dynamics knob) swept, same MIDI into both engines:

| CC1 | Kontakt peak | KONTRA peak (before) | KONTRA peak (after) |
|---|---|---|---|
| no CC sent at all | −28.198 | **silent** | −28.17 |
| 0 | −36.35 | **silent** | −36.31 |
| 32 | −32.535 | **silent** | −32.50 |
| 64 | −28.528 | **silent** | −28.50 |
| 96 | −23.703 | **silent** | −23.69 |
| 127 | −19.701 | −19.70 | −19.70 |

A cliff at exactly 127 — the one point the instrument defines for itself.

**The trap that cost an hour:** the first probe MIDI set CC1 *and* CC11 to the
same value. With CC11 = 0 both engines are silent (expression at zero is silence,
in Kontakt too — `oracle_render` reported "the render is silent (peak 0)" for the
same file), which looks exactly like "the instrument needs the mod wheel". Only
after the PGS fix did the no-controller render come out at −28.17 instead of
−205 dBFS, which is what showed the two controllers were being conflated. Sweep
one controller at a time.

### The chain, read from the instrument's own scripts

The `.nki` stores the script sources; `cargo run -p kontra-format-kontakt
--example dump_scripts` writes them out (2.2 MB for slot 1).

1. `slot-0.ksp:5185` — `pgs_set_key_val(pgs_xfade_arr_1/$_2, $i4, %tbl_ADMIN__test_tab{,2}_inst[$i4])`
   for 0..127. The two tables are saved persistent values of two hidden
   `ui_table`s and are parsed correctly (0…10000 rising, 10000…0 falling).
2. `slot-1.ksp:11994` — `%_2d_xfade_arr[...] := pgs_get_key_val(pgs_xfade_arr_1/$_2, $i4)`.
3. `slot-1.ksp:~13795` — `perf__xfade_arr_store` converts each entry through
   `_VR_to_mdb`: `x ≤ 0 → −180000`, else `6·log2(x)/1000 − 79726`
   (so 10000 → **0 dB**, 0 → **−180 dB**), then hard-sets four endpoints
   (`[0]=10000`, `[127..132]=0`, `[260..265]=10000`).
4. `slot-1.ksp:7614` — `%perf__vol[0/1] := %_2d_perf_xfade_arr[y] + …`, and
   `change_vol(id, %perf__vol[n])` with the other layer at `vol − 180000`.

With the PGS reads answering 0, steps 2–4 collapse: every index is −180000 except
the four endpoints the script sets *after* the loop. Only `y = 127` reaches a
defined one, which is why exactly the top of the mod wheel sounded.

### The law

`crates/engine/engine/src/pgs.rs` reserved `KEYS = 64` keys of each kind and, past
that, silently declined to create them — `pgs_set_key_val` on a key that was never
created does nothing, and `pgs_get_key_val` answers 0. This instrument creates
**409**.

The KSP Reference Manual (Kontakt 8, *PGS*, p. 355) is explicit:

> It is possible to have as many PGS keys as you want, however each key can only
> have up to 256 elements.

So the 64 was invented, and the 256 was already right. The table is now a plain
`Vec` that grows, with the manual quoted at the constant.

### Evidence the fix is the right one

- The unit test `a_pgs_key_past_the_sixty_fourth_carries_its_value` fails on the
  old table (it reads group 0, level 0.25) and passes now (group 1, level 0.5).
- The whole CC1 curve now tracks Kontakt to **0.04 dB** at every measured point
  (§3), which a wrong curve could not do.
- The instruments that already passed do not move: Electric Mint
  **318df4e82ab353d7**, Haunted Strings 2 **06cf13eee3f8f54e**, Cherry Electric
  Bass **0279ba0fd86927bd** — byte-identical.

---

## 3. Level, on the same MIDI, both engines

Key 60, velocity 100, note on at 0.10 s for 1.0 s. Kontakt 8.14.1 through
`tools/oracle-mcp`; KONTRA through `kontra-render`.

Trumpet 1 Multi, the whole dynamics curve (peak, dBFS):

| CC1 (CC11 = 127) | Kontakt | KONTRA | Δ |
|---|---|---|---|
| no controller sent | −28.198 | −28.17 | 0.03 |
| 0 | −36.35 | −36.31 | 0.04 |
| 32 | −32.535 | −32.50 | 0.04 |
| 64 | −28.528 | −28.50 | 0.03 |
| 96 | −23.703 | −23.69 | 0.01 |
| 127 | −19.701 | −19.70 | 0.00 |

The no-controller row is the real "out of the box" test: the instrument's saved
dynamics knob (`%_2d_gui_element_values[1]`, a persistent value) is **66**, and
both engines start there without being told.

Three more instruments, one note, CC1 = CC11 = 127:

| instrument | Kontakt peak | KONTRA peak | Δ | Kontakt RMS | KONTRA RMS | Δ |
|---|---|---|---|---|---|---|
| Alto Sax 1 Multi | −17.63 | −17.63 | **0.00** | −34.52 | −34.36 | **+0.16** |
| Trumpet 1 Bucket Legato + Doits | −29.774 | −29.77 | 0.00 | −44.91 | −46.15 | 1.24 |
| Trombone 2 Bucket Staccatissimo | −31.362 | −31.36 | 0.00 | −51.50 | −52.73 | 1.23 |

And a four-note voicing (keys 55/60/64/67 held together), aligned at the measured
onset over one second, Trombone 1 Multi: **RMS +0.07 dB, peak +0.11 dB**. The
interesting part of that render is what it does *not* do — the chord is only
0.4 dB above a single note on the same instrument, and Kontakt does exactly the
same (+0.39 dB), so the multis are per-instrument voices with level-compensated
layers rather than four independently summed samples. "Sounds like one sample"
was the right thing to check and the wrong thing to conclude: it is one
instrument, and it is the same instrument in both engines.

The RMS column is measured over a common window on the two WAVs for Alto Sax (the
oracle trims relative to its own onset); the other three RMS figures are the two
tools' own windows and are **not** aligned, so only the peaks are comparable
there. Peak ±0.00 dB on all four, across a 12 dB span of loudness, is the result
that matters: it says the level law is right and that the "quiet" class is the
assets.

---

## 4. What this library is, structurally

- **5 script slots, all named `C.A.P.S.U.L.E.`** — an Orange Tree Samples engine
  split as interface (0), performance/voice (1), and three layer slots (2–4).
- Slot 1 is 2.2 MB of KSP with 343 compiled callback/dispatch entries, a
  `ui_table`-driven crossfade engine, 89 knob laws and 8 reverb laws.
- 8 insert slots, of which slot 7 is a `Send` (all eight buses at 1.0, `dry 0.0`)
  and the rest are empty; `main` and `sends` are all `None`.
- The multis are 504–696 groups and 14,604–27,996 zones, of which 2,489–6,248
  prepare — the rest are saved purge state.
- Dynamics is **CC1**, not velocity: velocity is fixed at two layer notes (61 and
  91) which the script starts and crossfades by volume.

---

## 5. What is open

- **`kontra-inspect play` calls this instrument's render incomplete.** Its trace
  ring holds 262,144 events; this instrument emits about 448,000, so
  `trace_events_dropped` is 186,022 and `render_complete` — which includes
  `dropped == 0` — reads false, and the command exits 2. The audio side is clean
  on the same render (0 missing frames, 0 read errors, 0 refused requests,
  0 wait timeouts). Closing it is a decision about what `render_complete` is for,
  not a measurement.
- **The 5× out-of-range fault in slot 0 and slot 1** (§1). Identical on every
  instrument in the library and not audible, but it is a script asking for an
  array element we do not have. `first_out_of_range = (101, true, 32)` for slot 0
  and `(170699, false, 4096)` for slot 1 — disassemble those two offsets and name
  the array.
- **No articulation matrix.** Every parity number above is one note at key 60.
  The library is keyswitched and section-based; the multis played as chords, the
  slot 7 send, and the mic/articulation pages are unmeasured.
- **The RMS gap.** Kontakt and KONTRA agree on peak to 0.01 dB but KONTRA's RMS
  reads 1.1–1.3 dB lower on three of the four instruments. On Alto Sax, measured
  over a common window, it is +0.16 dB — so the gap is most likely the two tools'
  capture windows rather than a decay difference, but that has not been shown for
  the other three.
