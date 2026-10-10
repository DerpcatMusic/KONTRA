# Authored value display parity

This audit compares Kontakt v1 `0cb7a8a0` and UVI v1 `4bffbb18` source with v2 integration `5498e631`, using one representative from each of 11 Kontakt libraries and each of 26 UVI banks. This is a source-backed display audit and a native-reader metadata census, not fresh native-host or frozen-plugin display acceptance. Authored script text, labels, and saved values stay in RAM.

## Closed readout behavior

`sampler_ui_ir::Display::format_value(&self, value: f64, integer: bool, source: Source) -> String` is the shared formatter. Kontakt keeps its existing raw/display ratio and suffix behavior. Falcon uses the v1 `number_text` / `unit_text` conventions for percent, normalized percent, seconds, milliseconds, frequency, decibels, linear gain, centered pan, and semitones. Existing emitted unit-name metadata is consumed without changing the Lua `Unit` shim, control range, raw value, sprite position, or numeric-entry conversion.

Knob and closed ValueEdit readouts use this formatter. Authored `value_text` remains the override; KSP's explicit empty knob label still suppresses its number. UVI's nonempty `displayText` overrides formatting. Pictured Falcon knobs now overlay the authored title/value captions on their complete skin frame, as v1 does. `showLabel` and `showValue` retain their independent meaning.

This ports `4bffbb18:src/ui/uvi_instrument.rs:157–191` and `:628–649`. The known unit conversions agree with the [official UVI Unit reference](https://lua.uvi.net/class_unit.html); exact native-host precision and punctuation have not been measured. The official [UI page guide](https://lua.uvi.net/_u_i_page.html) identifies `displayText` as the authored display override.

## Mismatches

| Surface | v1 behavior | v2 baseline | Result |
|---|---|---|---|
| Falcon Knob / NumBox unit display | Converted unit readouts | Raw number followed by enum name, e.g. `PercentNormalized` | Fixed by shared formatter; 14 readouts × 2 widget kinds covered by actual rendered-pixel oracle |
| Pictured Falcon Knob captions | Captions overlay complete authored frame | Strip art only | Fixed by caption overlay; authored override and hide flag change actual pixels, frame/raw value unchanged |
| UVI unpictured `displayText` | Nonempty override | Nonempty override already published and drawn | Preserved |
| Kontakt `set_knob_label` / `CONTROL_PAR_LABEL` | Nonempty held interaction label; stock knob uses label | Metadata reaches IR, stock knob uses label; pictured interaction overlay absent | Pictured interaction gap remains open |
| Kontakt `set_knob_unit` / `CONTROL_PAR_UNIT` | Unit recorded; stock v1 readout often raw rounded | Knob / ValueEdit carry Display; Slider has no Display field | Knob / ValueEdit retained; slider metadata gap remains open if authored |
| `CONTROL_PAR_DISPLAY` / `CONTROL_PAR_DISPLAY_VALUE` | No applicable implementation found | No applicable compiler constant found | Token census reported below; no invented semantics. Neither appears in the current [NI control-parameter reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-parameters) |
| UVI noncenter Pan / UviFilter / Megabyte / Cents / MidiKey | v1 numeric fallback without native calibration | Enum-name suffix | Shared formatter retains v1 numeric fallback; native calibration remains open |
| Literal numeric Lua unit IDs | Native IDs supported in v1 | Current emitter reads unit-name strings | Not changed in this display slice; live census detects current emitted metadata only |
| Falcon Slider captions / unit metadata | v1 numeric control caption path | Slider IR has no Display metadata | Open; this fix does not extend the IR variant |

## Scope and incidence ranking

All 37 representatives loaded through the native readers: 11 Kontakt libraries and every one of the 26 `.ufs` banks physically present under the UVI library folder. The census found **583 unoverridden enum-name readouts across 26 UVI banks**, including 13 visible in the initialized UI. This is the largest confirmed declared readout gap, closed by the source-aware v1 formatter. The companion caption path affects **75 currently visible pictured knob values across 25 banks**. These are metadata incidence counts, not 658 distinct controls or fresh real-bank paint witnesses; the categories may overlap.

Kontakt's fresh API-reference counts agree with the historical v1 scan for all 11 representatives. The initialized IR contains 475 numeric authored labels, 12 nonempty unit fields, and 129 nondefault ratios. No sampled slider loses a nonzero UNIT property. Direct `CONTROL_PAR_UNIT`, `CONTROL_PAR_DISPLAY`, and `CONTROL_PAR_DISPLAY_VALUE` references are all zero in these scripts. This does not establish dynamic callback or native-host text parity.

UVI startup findings are reported without proprietary details. A successful load does not establish complete script execution or zero findings; those existing script gaps remain with the scripting lane. The display emitter was not modified, so this slice does not change finding counts or initial UI publication. No repeat candidate census was needed for unchanged metadata.

## Measurement

The metadata-only example `value_display_audit` accepts a TSV of `kontakt<TAB>path` / `uvi<TAB>bank::program`. It reads actual KSP / Lua UI results and exports only fixed unit categories and counts. The executable is frozen before production edits. Library probes run in bounded shards through the shared FIFO wrapper; receipts are under `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w2-value-display-403`.

The root regressions render real synthetic Lua controls and compare pixels with explicit v1 readout strings. The skin regression supplies a synthetic image and checks the authored frame and raw value. Baseline: both tests fail. Candidate: 2/2 display regressions pass (28 unit/widget combinations plus the skin witness), 1/1 formatter compatibility test passes, 6/6 existing IR-area tests pass, and root library no-run passes. All verification uses the restored per-worktree target through the shared wrapper. Production source is `3b64f0e87614fe5a901ccf9b4662192be72d084d`; test commits are `079a00a2` and `09b655e8`, with incidence probe extension `be5137a1`.

No native Kontakt/Falcon host capture, frozen v1 UVI live run, sound acceptance, or full preset sweep is claimed. Numeric-entry and modifier policy belong to W11; pictures/fonts belong to W3.

NEXT: route the formatter/caption commits to integration and retain the open display gaps for the next value-semantics slice.

## Fresh representative census

Completed 37/37 representatives. The v1 columns are historical frozen scanner incidence; the v2 columns are this fresh native-reader census. These are all authored pages, with inherited hiding applied; they are not active-page pointer or native-host witnesses. API references include callbacks, while widget counts reflect the initialized UI.

| Kontakt library | v1 label / unit / LABEL refs | v2 label / unit / LABEL refs | UNIT / DISPLAY / DISPLAY_VALUE refs | labels (empty) | Display unit fields | ratios | dropped slider units |
|---|---:|---:|---:|---:|---:|---:|---:|
| Audio Imperia Dolce | 0 / 0 / 390 | 0 / 0 / 390 | 0 / 0 / 0 | 29 (0) | 0 | 40 | 0 |
| ANALOG STRINGS | 40 / 4 / 31 | 40 / 4 / 31 | 0 / 0 / 0 | 285 (101) | 4 | 8 | 0 |
| Afflatus Chapter II Brass | 0 / 0 / 45 | 0 / 0 / 45 | 0 / 0 / 0 | 24 (4) | 0 | 0 | 0 |
| Areia 1.2.0 [Audio Imperia] | 0 / 0 / 390 | 0 / 0 / 390 | 0 / 0 / 0 | 30 (0) | 0 | 40 | 0 |
| Audio Imperia CHORUS | 0 / 0 / 353 | 0 / 0 / 353 | 0 / 0 / 0 | 29 (0) | 0 | 36 | 0 |
| Conflux 1.1.0 [Native Instruments] | 2 / 5 / 103 | 2 / 5 / 103 | 0 / 0 / 0 | 2 (0) | 5 | 0 | 0 |
| Morphology Evolved [Zero-G] rutracker.org | 0 / 0 / 0 | 0 / 0 / 0 | 0 / 0 / 0 | 0 (0) | 0 | 0 | 0 |
| Pacific Ensemble Strings | 14 / 2 / 0 | 14 / 2 / 0 | 0 / 0 / 0 | 4 (4) | 2 | 0 | 0 |
| Performance Samples Vista | 16 / 1 / 0 | 16 / 1 / 0 | 0 / 0 / 0 | 4 (4) | 1 | 1 | 0 |
| Solo | 0 / 0 / 548 | 0 / 0 / 548 | 0 / 0 / 0 | 30 (0) | 0 | 4 | 0 |
| Una Corda Library | 0 / 0 / 269 | 0 / 0 / 269 | 0 / 0 / 0 | 38 (0) | 0 | 0 | 0 |

| UVI bank | numeric controls | nonempty overrides | enum readouts (visible) | visible pictured knob values | Lua findings |
|---|---:|---:|---:|---:|---:|
| Augmented Orchestra.ufs | 871 | 578 | 120 (0) | 0 | 101295 |
| VWinds-AClarinet.ufs | 101 | 0 | 17 (0) | 3 | 149 |
| VWinds-AClarinet_V2.ufs | 108 | 1 | 21 (0) | 3 | 185 |
| VWinds-BassClarinet.ufs | 101 | 0 | 17 (0) | 3 | 157 |
| VWinds-BassClarinet_V2.ufs | 108 | 1 | 21 (0) | 3 | 187 |
| VWinds-BassetHorn.ufs | 101 | 0 | 17 (0) | 3 | 147 |
| VWinds-BassetHorn_V2.ufs | 108 | 1 | 21 (0) | 3 | 177 |
| VWinds-BbClarinet.ufs | 101 | 0 | 17 (0) | 3 | 147 |
| VWinds-BbClarinet_V2.ufs | 108 | 1 | 21 (0) | 3 | 194 |
| VWinds-ContrabassClarinet.ufs | 101 | 0 | 17 (0) | 3 | 131 |
| VWinds-ContrabassClarinet_V2.ufs | 108 | 1 | 21 (0) | 3 | 158 |
| VWinds-EBClarinet.ufs | 101 | 0 | 17 (0) | 3 | 139 |
| VWinds-EBClarinet_V2.ufs | 108 | 1 | 21 (0) | 3 | 162 |
| VWinds-Bassoon.ufs | 99 | 0 | 16 (1) | 3 | 145 |
| VWinds-Bassoon_V2.ufs | 106 | 0 | 19 (1) | 3 | 175 |
| VWinds-Contrabassoon.ufs | 99 | 0 | 16 (1) | 3 | 131 |
| VWinds-Contrabassoon_V2.ufs | 106 | 0 | 19 (1) | 3 | 165 |
| VWinds-EnglishHorn.ufs | 99 | 0 | 16 (1) | 3 | 123 |
| VWinds-EnglishHorn_V2.ufs | 106 | 0 | 19 (1) | 3 | 155 |
| VWinds-Oboe.ufs | 99 | 0 | 16 (1) | 3 | 125 |
| VWinds-Oboe_V2.ufs | 106 | 0 | 19 (1) | 3 | 163 |
| VWinds-Alto_Flute.ufs | 108 | 0 | 19 (1) | 3 | 157 |
| VWinds-Bass_Flute.ufs | 108 | 0 | 19 (1) | 3 | 152 |
| VWinds-C_Flute.ufs | 108 | 0 | 19 (1) | 3 | 139 |
| VWinds-Contrabass_Flute.ufs | 108 | 0 | 19 (1) | 3 | 153 |
| VWinds-Piccolo.ufs | 108 | 0 | 19 (1) | 3 | 142 |
