# Engine I/O: trims, solo, sidechain, focus, latency

Evidence is static analysis of the soothe3 restored image (base `0x180000000`).
Paths below are under `RunScratch/smooth/` on the dev drive unless noted.
Status per item: **verified** (implemented, the reference's own arithmetic
located), **named** (implemented from the declared name, the reference's
routing not located), **open** (not implemented, the reason is given).

## Parameter ids (verified)

`feat/params.tsv` is the reference's declared table, index order as the host
sees it. Items here: 14 Sidechain, 15 Sidechain listen, 16 Stereo mode,
17 Stereo link, 18 Stereo focus, 91 Band solo (choice 0..8), 92 Max cut,
93 Wet trim (-18..18 dB, linear), 94 Mix (0..100 %), 95 Out trim (-18..18
dB), 96 Delta, 97 Bypass, 100/101 LR solo/delta, the per-set blocks 98..122.

## 1. Wet trim (93): verified

- Set-param: the dB value is multiplied by `0.05` and raised to `10^x`
  (`set_param trims/mix`, `feat/dsp/B-key-excerpts.txt`, handlers at
  `0x8947e2` and `0x894820`; constants resolved from the image at
  `0x18209d0a0` = 0.05). Wet is stored at `+0x94`, out at `+0x9c`.
- Output stage (`feat/dsp/A-process-dis.txt` 891dd4..891f19): the stage
  builds four gain ramps, each `{target, g, rate}` with stride 0xc:
  - dry at `+0xbf28`: 1 when bypassed, `wet*mix` in delta, `1 - mix` otherwise;
  - wet at `+0xbf34`: 0 when bypassed, `-wet*mix` in delta, `wet*mix` otherwise;
  - out at `+0xbf58`: 1 when bypassed, else the out trim.
- The gain-ramp helper is `0x899ab0` (`callees/pdg-899ab0.c`), the same
  per-sample `g <- (t - g)*a + g` as `Engine::process_sample`'s mix ramp.

Implemented in `Engine::process_sample_with_detector`: normal
`((1 - mix) dry + wet_gain mix wet) * out`, delta `(dry - wet) wet_gain mix * out`,
bypass `dry`. The wet trim is `Settings::wet_gain`, from the plugin's
`Wet trim` parameter. Out trim stays after the mix. Tests:
`dsp::tests::the_wet_trim_scales_only_the_processed_signal`.

Not matched: the reference ramps the out trim (`+0xbf58`) with its own rate;
the port applies it unramped. The ramp rate field is not located.

## 2. Band solo (91): open

Located:
- `91` is stored at `+0x100` as `plain - 1` (`feat/params.tsv`, `band solo`
  state at `+0x100`). `-1` means no solo.
- At process start (`A-process-dis.txt` 890a35..890a5c) the solo index is
  read, and `+0xaa` (solo active) is set to the solo band's `used` byte
  (band record stride `0x20`, `used` at `+0x10c`). `+0x4c` holds the index.
- Output gating (`891dd4..891e21` and `891eb4`..`891ed1`): the non-solo
  stream's ramp `+0xbf40` is `(solo inactive)`, the solo stream's ramp
  `+0xbf4c` is `(solo active)`. The solo stream is skipped unless
  `[0xbf50] > 0.001` (`8923bc`), and runs in its own branch at `8923d2`.

Not located: which buffer the solo stream carries. It is produced by
`0x89a1a0` (`callees/pdg-89a1a0.c`, with a `shape == 5` branch) and the
`[0xbf50]` accumulator, neither traced to a buffer. Implementing solo before
that is traced would be a guess, so it is not in the port.

## 3. Sidechain (14, 15): named

Located:
- `14` is stored at `+0xa5`, `15` at `+0xa7` (`feat/params.tsv`, set-param
  `89460b`/`894628`).
- Process start reads `+0xa7` into a stack flag (`A-process-dis.txt` 890a7d),
  and the listen branch is at `892e43`.
- Moose (the plugin's framework) supports a sidechain bus: `BusLayout::with_sidechain_input`
  in `vendor/moose-*` and `crates/moose-core/src/bus.rs`.

Implemented: a stereo sidechain layout (`bus_layouts`), the detector reads
the aux pair, the sidechain switch and listen flags. The detector mux (the
reads of `+0xa5`) is not located, so the analysis switch follows the name.
Listen outputs the detector signal through the out trim; the reference's
listen output is not located.

Open: the sidechain meter (process publishes visitor slot 13,
`sidechain_input_level`, at `892d14`/`89392a`). Its level formula is not
located, so no meter.

Observed, unverified: a silent detector mutes the output
(`dsp::tests::the_sidechain_detector_replaces_the_analysis_input` records
the difference, not the value). Whether the reference mutes on silence is
open.

## 4. Focus and mid/side (16..18): partly verified

- Stereo focus: the reference's pair weights are `gL = clamp(2(1 - f))` and
  `gR = clamp(2f)` (`feat/dsp/C-formulas.txt`, "STEREO PAIR FOCUS"). That is
  `dsp::focus_weights`, already matching.
- Mid/side: `Stereo mode` (16) is stored at `+0xa6` and read at `892781`
  into a stack byte. Its use is not located, so the port's `1/sqrt(2)`
  matrix is unverified.

## 5. Plan switching and reported latency (Low latency, Quality, Linear phase): open

Located:
- `getLatency` (`B-key-excerpts.txt`, `88f610`): reports `[0x2c]*H + [0x278]`,
  where `H` is the latency helper at `894d20`, or just `[0x278]` when
  `[0xab]` (low latency) or `[0x237]` is set.
- `H = ((state[0x2c8] + state[0x2f10]) - state[0x2c0]) / 2 + state[0x2c78] + state[0x30]`
  (`894d20`, reading the `sar`/`add` tail).

Not located: the constants `[0x2c]`, `[0x278]`, `[0x2c78]`, `[0x30]` and the
list sizes come from the constructor, which is not traced, and `[0x278]` is
also written as a 64-bit pointer (`897677`). The port's latency is
`n - 1 (+ m/2 linear)` from its own plans, so it does not match the reference
until the constructor is traced. The plan-switch behaviour (`bank.select`)
is not analysed.

## Commands

```
build-slot cargo test -p smooth-core -p smooth-plugin --lib
```

The engine tests pin the wet trim, the sidechain switch and listen, and the
two-stream mix; the plugin tests pin the parameter defaults and the stereo
plus sidechain layout.
