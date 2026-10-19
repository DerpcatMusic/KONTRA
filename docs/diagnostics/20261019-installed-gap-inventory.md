# Installed-library gap inventory (Kontakt)

Date: 2026-10-19. Tool: `crates/sampler-kontakt/examples/fx_survey.rs`.
Corpus: `/mnt/MAIN_STORAGE/Libraries/Kontakt` (2,742 `.nki` instruments).

This is the list of everything the Kontakt translator does not implement or does
not translate, measured on the installed libraries rather than on a hand
picked subset. It replaces the stale claims in
`docs/architecture-v2/DSP_COVERAGE.md`, whose module tables were written
against an older branch (it still lists Legacy HP1, the SV filters and the
Formant filters as missing; `crates/sampler-kontakt/src/effects/filter.rs`
implements all of them today, most of them bit-exact against the native
classes).

## How to reproduce

```sh
cargo run -p sampler-kontakt --release --example fx_survey -- /mnt/MAIN_STORAGE/Libraries/Kontakt
ALL=1 FILES=1 VALUES=all cargo run -p sampler-kontakt --release --example fx_survey -- /mnt/MAIN_STORAGE/Libraries/Kontakt
```

* the plain run prints the corpus totals, the caveat counts and the effect
  module table;
* `ALL=1` adds one `FEATURE` line per reported feature, with the number of
  instruments and the number of report entries;
* `FILES=1` adds the instrument path of every reported feature;
* `VALUES=<feature>` (or `VALUES=all`) prints the reported value, its scope
  and the instrument it belongs to.

The corpus contains a `KONTRA project recovery` tree that holds a second copy
of several libraries. The counts below are deduplicated by instrument path;
the raw run reports 29,677 entries because the copies are counted twice.

## Corpus totals

| Measure | Value |
|---|---|
| `.nki` files | 2,742 |
| read | 2,742 (0 unreadable) |
| instruments with at least one report | 371 (deduplicated) |
| report entries | 2,303 (deduplicated) |
| instruments with a dropped or untranslated effect | 6 |
| instruments with a translated impulse response | 366 |

Ten libraries contribute: Areia (155 instruments), Afflatus Chapter II
Brass (102), Solo (100), Performance Samples Vista (7), Una Corda Library
(3), and one each from Shreddage 3.5 E-Upright, Morphology Evolved,
Conflux and ANALOG STRINGS. Audio Imperia CHORUS no longer reports
anything: its only gap was the Flex loop jump.

## The gaps, ordered by entries

| Feature | Entries | Instruments | What is missing |
|---|---:|---:|---|
| `Filter: filter law` | 1,022 | 6 | The Ladder gain law and the 2x resampled (HQ) mode; the Daft audio core; the legacy cutoff boundary phase |
| `external modulation from RandomBipolar` | 550 | 1 | The bipolar random generator: interval, cadence, seed |
| `native source zone identity` | 251 | 251 | The native zone identity of a saved source zone, so no ordinal alias is created |
| `keyswitch script` | 202 | 201 | The static detection of a keyswitch script, so the articulation list stays empty |
| `Solid G-EQ: Solid G-EQ smoothing` | 59 | 3 | The native 20 ms coefficient smoothing on knob changes |
| `Solid G-EQ: live control` | 59 | 3 | Script access to the Solid G-EQ knobs |
| `modulation of a modulator's intensity` | 42 | 1 | Modulating the intensity of another modulator |
| `wavetable source` | 30 | 1 | The wavetable source mode 9 |
| `effect` | 17 | 6 | The Convolution payload layout, the Jump amplifier models and the Rotator core |
| `modulation of a module parameter` | 12 | 5 | The LFO rate and start phase targets of a module slot |
| `modulation target` | 16 | 1 | Wavetable targets whose stored intensity is zero |
| `Tape Saturator: Creative FX live controls` | 11 | 4 | Live Tape Saturator knobs and their smoothing |
| `Convolution: impulse response` | 10 | 2 | The impulse response resource |
| `time machine pro Elastique backend` | 10 | 1 | The Elastique granular backend |
| `Reverb (legacy): legacy reverb input lowpass` | 4 | 1 | The legacy reverb input lowpass |
| `LFO waveform (id, multi weights, pulse width)` | 4 | 1 | The multi-weight LFO waveform |
| `Convolution: IR size` | 2 | 1 | The impulse response block size |
| `saved automation target` | 1 | 1 | A saved automation tag |
| `Convolution: IR early/late crossover` | 1 | 1 | The early/late crossover point |

## 1. Envelope stage times as a modulation target

This was the largest gap: 962 entries over 36 instruments, 817 of them
`ahdsr_attack` and 136 `ahdsr_release`. The route is an external
modulation: the source is the release trigger counter and the destination is
the group's AHDSR modulator. For example, in
`Pacific - Ens Strings - 10 Cellos - Marcatos.nki` ten groups carry
`ReleaseTriggerCounter -> ahdsr_attack` at full depth with a shaper.

The stage times are executed now. The native compiler for these targets is
the same one the repository already models in `sampler-core/src/voice_mod/laws.rs`;
`docs/re/kontakt-envelope-time-modulation.md` records what it emits, and the
route binds to the group's own amplitude envelope, whose stage time follows
the family H law and the stage's own knob law.

953 of the 962 entries are closed. The remaining nine are the LFO rate and
start phase targets of a module slot, which are a different gap.

## 2. The Flex envelope's loop jump at the sustain row

This was the second largest gap: 504 entries over four instruments (Audio
Imperia CHORUS). The saved Flex record carries a byte at +0x194 that asks
the envelope to leave its sustain row instead of holding there, and the
native advance takes the point before the sustain row as the next point
(`0x140ae57dd`).

The loop is executed now: the translator sets the IR's loop range from the
saved flag and the `unknown_index`, and the core's segment state returns to
the loop's start instead of holding. See
[`docs/re/kontakt-flex-loop-jump.md`](../re/kontakt-flex-loop-jump.md).

## 3. The Ladder filter's gain law and HQ mode

1,022 entries, 6 instruments, 904 of them in Morphology Evolved. The Ladder
core itself is checked against the native code (`FUN_140b00be2`,
`FUN_140b06180`): the cutoff law and table, the pole polynomial, the
resonance, the feedback correction, the clip, the stages, the tap weights and
the compensation, and the 2 ms ramp. Two parts are still unproved: the gain
law, and the 2x resampled (HQ) mode. The Daft filter is weaker still: its
saved controls and ramp scheduling are extracted, but its audio core and
resampler are not recovered.

## 4. The bipolar random generator

550 entries, one instrument (Conflux). The external source type 13 has a
classification path only; no generator, interval, cadence, RNG or seed is
linked, so the source cannot produce a value.

## 5. Zone identity and keyswitch detection

`native source zone identity` (251 instruments) reports that a saved source
zone has no readable identity, so no ordinal alias is created and the zone
cannot be addressed by identity.

`keyswitch script` (201 instruments) reports that the static detector did not
recognise the keyswitch script of the behaviour. The script still runs; what
is missing is the articulation list the detector produces for the editor.

## Reporting rules used by the tool

* a report is one authored thing that the translator does not execute; the
  value keeps the authored numbers, the scope names the rack or group, and
  the path names the instrument;
* a zero-intensity modulation target is still reported: the route is not
  translated, even when it is inert at the saved state. The inventory counts
  them separately;
* a feature that is implemented but not measurable is not reported at all.
  The `Filter: filter law` entries are the exception: they name the part of
  the law that is still unproved.
