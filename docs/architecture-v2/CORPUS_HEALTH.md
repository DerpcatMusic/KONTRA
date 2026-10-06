# Corpus health

1494 instruments and programs checked.

| kind | total | loads | sounds | script compile failed | no failure |
|---|---|---|---|---|---|
| kontakt | 781 | 780 | 405 | 0 | 379 |
| kontakt-multi | 53 | 53 | 0 | 0 | 53 |
| uvi-program | 660 | 657 | 657 | 657 | 0 |
| all | 1494 | 1490 | 1062 | 657 | 432 |

Median peak of sounding instruments: -8.7 dBFS.

## Top failure reasons (instruments affected)

| instruments | reason |
|---|---|
| 657 | script compile: Lua has no frontend |
| 375 | silent at a covered key |
| 23 | script fault: Fault(ClosedNote) |
| 7 | audible output 5 s after release (> -60 dBFS) |
| 6 | script fault: Fault(InvalidInput) |
| 3 | load: no zone covers any key at any velocity |
| 1 | load: lowering: zone N: FilterPoles(N) is not supported by the native runtime |

## Top unsupported features in the load report (instruments affected)

| instruments | feature |
|---|---|
| 780 | script Unsupported: get_engine_par_disp |
| 780 | script Warning |
| 779 | source mode (played as a sampler) |
| 776 | script Approximate: find_mod |
| 765 | saved persistent arrays |
| 765 | script Approximate: set_engine_par |
| 720 | script Unsupported: get_event_par |
| 657 | keygroup oscillators all play (the script may pick one per note) |
| 657 | module |
| 657 | script |
| 649 | program or layer modulation |
| 479 | curved modulation shaper segment (linear used) |
| 395 | keyswitch script |
| 370 | Reverb: reverb algorithm |
| 352 | Stereo Modeller: stereo modeller <path> law |
| 349 | script Approximate: ticks_to_ms |
| 348 | script Unsupported: output_channel_name |
| 347 | undefined voice group |
| 322 | Program-level MultiLFO |
| 311 | modulation of a module parameter |
| 307 | Program-level LFO |
| 202 | Filter: EQ band shape |
| 174 | flex envelope segment curve (linear used) |
| 99 | crossfades <path> velocity, <path> key) |
| 63 | LFO waveform |

## Script diagnostics (instruments with at least one)

| instruments | kind |
|---|---|
| 780 | Approximate |
| 780 | Unsupported |
| 780 | Warning |
