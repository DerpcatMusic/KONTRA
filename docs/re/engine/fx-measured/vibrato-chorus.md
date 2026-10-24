# Kontakt Vibrato Chorus (Effects/VibratoChorus, BParFXVibratoChorus v0x10): measurement status

**Status: blocked. No verdict.** Kontakt 8.13.1 ignores every `ENGINE_PAR_VC_*` set made from an init script, so the
requested RATE / DEPTH / MIX / COLOR sweep cannot be measured with `fxrig.py`. No KONTRA mapping was added.

## Why it is blocked

Same steady-tone rig, same setup for every row: one bypassed reference plus instances with a 1 kHz tone, set by `on init`
(`set_engine_par($ENGINE_PAR_VC_*, raw, -1, 0, 0)`), 5 s tone, analysis over 3 s of steady output.

| instance | VC setting | RMS dB | carrier (1 kHz) dB | strongest sideband | dB re carrier |
|---|---|---|---|---|---|
| reference (bypassed) | - | -23.33 | -20.32 | none (the spectrum is clean) | -112.7 |
| VC, MIX 0 | MIX = 0 | -24.19 | -23.14 | 6.0 Hz | -5.87 |
| VC, MIX 1 | MIX = 1 | -24.19 | -23.14 | 6.0 Hz | -5.87 |
| VC, DEPTH 0 | DEPTH = 0 | -24.19 | -23.14 | 6.0 Hz | -5.87 |
| VC, DEPTH 5 | DEPTH = 5 | -24.19 | -23.14 | 6.0 Hz | -5.87 |
| VC, RATE 0 | RATE = 0 | -24.19 | -23.14 | 6.0 Hz | -5.87 |
| VC, RATE 1 | RATE = 1 | -24.19 | -23.14 | 6.0 Hz | -5.87 |
| VC, BLEND 1 | BLEND = 1 | -24.19 | -23.14 | 6.0 Hz | -5.87 |

Every VC instance is identical to 0.01 dB, including MIX 0 against MIX 1. The effect is on (the reference has no sidebands),
but none of the set values reach it. The parameters are inert on the init path.

Control, same rig and same session type: `Filters/Solid G-EQ`, `SEQ_LMF_GAIN` 0 against 1000000 gives RMS -42.30 dB against
-4.41 dB, a 37.9 dB change. So the rig works, and the failure is specific to the modern BParFX effects. The repo already records
the same for RAUM (`KONTAKT_RAUM_ORACLE_20261024.md`) and Replika (`replika_vector.py` header, `vectors/replika-init-reload-noop.tsv`):
set-engine-par and save/reload did not move them. Vibrato Chorus is in the same family.

What the table does measure: the INIT (default) state is a 6.0 Hz modulation, with sidebands 5.9 dB below the carrier, and
output RMS 0.87 dB below the bypassed reference. These are one state's numbers, not a response curve.

## Requested measurements

| Parameter | Values asked | Result |
|---|---|---|
| VC_RATE | 0.2, 0.5, 0.9 | not measured: no response to set |
| VC_DEPTH | 1, 3, 5 | not measured: no response to set (0 and 5 checked) |
| VC_MIX | 0.5, 1.0 | not measured: no response to set (0 and 1 checked) |
| VC_COLOR | 0, 2 | not measured: no response to set |

## KONTRA comparison

Not run. Two reasons:

1. There is no Vibrato Chorus mapping. `crates/sampler-kontakt/src/effects/timemod.rs:386` says 0x69 is unsupported
   because the core's wave-digital LC ladder (the delay line it models) is not extracted.
2. The generic KONTRA chorus is a different kernel. It is Kontakt Legacy Chorus (0x11): a `LegacyModDelay` with a second head,
   on a 4096-frame ring (`crates/sampler-ir/src/fx/timemod.rs:182-200`). Nothing here models the Vibrato Chorus ladder, so a
   comparison would only test a different effect.

Without a response to the saved fields, there is also no reference setting to compare against.

## Verdict

- Within tolerance: **not decided.** No metric was measured across settings, so no tolerance was applied.
- Mapping: **none added.** This is the correct state until a measurement shows a match.
- Worst metric error: **n/a**, nothing was compared.

## Next steps to unblock

1. Get the VC parameters into the core. Options: (a) factory `.nkp` presets for Vibrato Chorus, the RAUM route, giving
   several fixed states rather than a sweep; (b) reverse the Vibrato Chorus setter in the Kontakt 8.13.1 binary (rea or
   radare2) to find why `set_engine_par` on these IDs is dropped; (c) drive the parameters through automation (MIDI CC or
   on controller) and check that path first.
2. Then rerun `tools/kontakt-oracle/fxrig.py tone` with `specs/vibrato-chorus-steady.json` (already written: RATE 3, DEPTH 3,
   MIX 2, COLOR 2, one baseline).
3. Only after a clean response curve, compare against sampler-core's chorus fields. The KONTRA side is a generic
   modulated-delay chorus, so the Vibrato Chorus ladder would likely need its own kernel, not a parameter mapping.

## Other observations

- The first session (9 instances: reference plus 8 steps) rendered steps 1-5 and silence for steps 6-8. That is not explained.
  The 8-instance diagnostic rendered every step. A 9-instance session may be a limit to check.
- The reference RMS differs between runs: -31.52 dB in the first run, -23.33 dB in the diagnostic. The first run's deltas
  (+7.3 dB against the reference) are therefore not valid. `vibrato-chorus-first-run.tsv` is kept only for the record.
- The rate metric is the strongest peak within 0.5-30 Hz on each side of the carrier. On the reference it reports noise
  (0.67 Hz). It is valid only where the sidebands are real. Depth is reported in dB re carrier, not cents.

## Raw data

- `data/vibrato-chorus-param-response.tsv`: the diagnostic above (valid; reference and all VC instances, extreme settings).
- `data/vibrato-chorus-first-run.tsv`: the first sweep (see the reference caveat above; deltas not valid).
- `data/solid-geq-control.tsv`: the Solid G-EQ control (rig works).
- `specs/*.json`: the exact specs. Rerun with `oracle-slot python3 tools/kontakt-oracle/fxrig.py tone SPEC.json OUT.wav`.

## Harness change

`tools/kontakt-oracle/fxrig.py` gained a `tone` stimulus (5 s steady 1 kHz at -12 dBFS, 7 s per instance) and a `tone`
subcommand (`tone_lua`, `tone_metrics`, `tone_stats`). It writes RMS dB, carrier dB, rate Hz and sideband dB per instance,
with deltas against the bypassed reference.
