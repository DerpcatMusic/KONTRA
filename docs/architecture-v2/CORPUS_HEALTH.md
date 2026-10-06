# Corpus health

1494 instruments and programs checked.

| kind | total | loads | sounds | script compile failed | no failure |
|---|---|---|---|---|---|
| kontakt | 781 | 781 | 405 | 0 | 379 |
| kontakt-multi | 53 | 53 | 0 | 0 | 53 |
| uvi-program | 660 | 657 | 657 | 657 | 0 |
| all | 1494 | 1491 | 1062 | 657 | 432 |

Median peak of sounding instruments: -8.7 dBFS.

## Top failure reasons (instruments affected)

| instruments | reason |
|---|---|
| 657 | script compile: Lua has no frontend |
| 376 | silent at a covered key |
| 23 | script fault: Fault(ClosedNote) |
| 8 | audible output 5 s after release (> -60 dBFS) |
| 6 | script fault: Fault(InvalidInput) |
| 3 | load: no zone covers any key at any velocity |

## Top unsupported features in the load report (instruments affected)

| instruments | feature |
|---|---|
| 781 | script Unsupported: get_engine_par_disp |
| 781 | script Warning |
| 779 | source mode (played as a sampler) |
| 777 | script Approximate: find_mod |
| 766 | saved persistent arrays |
| 765 | script Approximate: set_engine_par |
| 721 | script Unsupported: get_event_par |
| 657 | keygroup oscillators all play (the script may pick one per note) |
| 657 | module |
| 657 | script |
| 649 | program or layer modulation |
| 479 | curved modulation shaper segment (linear used) |
| 396 | keyswitch script |
| 370 | Reverb: reverb algorithm |
| 352 | Stereo Modeller: stereo modeller <path> law |
| 349 | script Approximate: ticks_to_ms |
| 348 | script Unsupported: output_channel_name |
| 347 | undefined voice group |
| 322 | Program-level MultiLFO |
| 311 | modulation of a module parameter |
| 307 | Program-level LFO |
| 255 | monophonic release trigger |
| 202 | Filter: EQ band shape |
| 174 | flex envelope segment curve (linear used) |
| 99 | crossfades <path> velocity, <path> key) |

## Script diagnostics (instruments with at least one)

| instruments | kind |
|---|---|
| 781 | Approximate |
| 781 | Unsupported |
| 781 | Warning |
