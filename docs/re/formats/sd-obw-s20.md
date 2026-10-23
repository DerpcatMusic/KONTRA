# Superior Drummer: `.obw` sound banks and `.s20` sample projects

Status: **stopped at the payload gate.** The `.s20` project text and the `.obw`
index are readable and documented below. The `.obw` audio payload does not
read as PCM, and I have not established whether it is encrypted or a
proprietary codec. Per the licence rule (no circumvention of DRM or
encryption), no decoder was written and no audio was decoded. No reader crate
was added in this PR.

Evidence is black-box, from the installed files under
`/data/main/Libraries/Superior Drummer/EZDrummer/SL-Avatar` (checked 2026-10-23):
13 `Sounds/soundb1..13.obw` (1.0–2.0 GB each) and 18 `SampleProjs/*.s20`
(15–17 KB each).

## `.s20` sample projects (text, readable)

Plain UTF-8, `#V 2.0` first line, `version 3`, then brace-delimited sections.
All 18 files have the same section set: `usage`, `mixer`, `midi`, `pads`,
`padsound`, `SnareTop`, `SnareBottom`, `SnareFX`, `OhR`, `Oh`, and others.

- `HQ { soundlib "SL-AVATAR"  mastervol …  enginevol … }`: global settings.
- `laylims { "Kick" 6 6 6 … }`: per-instrument layer limits (one row per instrument).
- `drums { sticks …  kicktool …  crmat 1  "Kick" "1822GMS"  "Snare" "14ROG" … }`:
  instrument name → kit piece id. Empty string = slot unused (`"Tom3" ""`).
  The ids name the drum model and microphone set (`1822GMS` = 18" kick, GMS mics).

These files are the only plain-text description of kit assignment in the
library. A reader would parse the sections above and map instrument → piece id.
Not implemented in this PR; the piece ids only resolve against `.obw` banks,
which are gated below.

## `.obw` sound banks (RIFF form `ttbw`)

Each bank is a RIFF file with form type **`ttbw`** (not `WAVE`):

| Offset | Chunk | Notes |
| ---: | --- | --- |
| 0 | `RIFF` size, `ttbw` | size = file size − 8 |
| 12 | `oidx` | index, size e.g. 103 388 B in `soundb1` |
| ~103 408 | `data` | one large chunk containing all samples |

Index (`oidx`): binary records followed by NUL-terminated ASCII names. The
first records name the instruments and mic layers (`Kick`, `KickExt`,
`KickSub`, `SnareTop`, `SnareBottom`, `SnareFX`, `Hats`, `Tom1…5`,
`AmbNearComp`, `AmbMidComp`, `AmbFarComp`, `AmbBrauner`, `AmbBullet`), then
sample names in the form `<PIECE>_<NN>_<ARTIC>_<MIC>_R` (e.g.
`KD01_01_FK_FH_R`, `SD02_01_DS_FH_R`, `TO01_05_DS_FH_R`). `soundb1` has 2939
name strings. The binary field layout of the records was not decoded.

Inside `data`: samples are back to back, each a nested RIFF with form type
**`ttpw`**: a 44-byte `fmt ` chunk (tag `0x0001`, mono, 44 100 Hz in the first
sample) and a `data` chunk. `soundb1` contains 1202 such blocks in the first
200 MB (sizes 23 KB and up, RIFF size + 8 = block gap, checked).

## Payload gate (why the audio is not decoded)

- The `fmt ` field says PCM (tag 1), but the `data` bytes do not behave like
  PCM. Measured on the first `ttpw` sample, mean |Δx| / RMS:
  16-bit mono 1.12, 16-bit stereo 1.13/1.13, 24-bit mono 1.00. A random
  reference gives 1.16. A sine reference gives 0.09. The sample starts with
  small values (−18, −19, −10, −15) and then jumps to full scale within a
  few samples. This is not plausible audio at 44.1 kHz.
- Byte entropy is 7.93 bits/byte over 1 MiB; the `oidx` index is 6.84.
- No codec signature (`OggS`, `fLaC`, `Vorbis`, `WAVE`, `Kontakt`) appears in
  the first 8 MB. `ENCR`, `AES` also absent.
- Those facts fit either a proprietary compressed stream or encrypted sample
  data. Neither has been ruled out, and I did not try to find out how the
  bytes are produced.

Next step needs a decision, not more analysis here: whether to treat `.obw`
payloads as encrypted (the documented stop, nothing further) or to pursue a
codec reading through the product's own documented format. The index and
`.s20` text are readable without that decision, so a reader for them can
proceed on its own.

## Open items

- `.obw` record field layout in `oidx` (ids, offsets, mic/articulation keys).
- Whether `data` payloads are encrypted or compressed (decision above).
- `.s20` reader: not written; the sections above are the spec.
