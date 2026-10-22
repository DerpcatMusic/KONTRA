# Library gaps (full-tier corpus survey, 2026-10-22, origin/main 3485e567f)

Source: `tools/corpus-health/run.sh run --tier full --workers 3 --timeout 600 /data/main/Libraries`, 1395 instruments/programs, nice 15.
Raw: `scratch/survey/full-20261022-fresh.jsonl`. Summary: `full-20261022-fresh-summary.md`. Diff: `full-20261022-fresh-diff.txt`. Gap metrics: `full-20261022-fresh-gaps.txt`.
Baseline for regressions: `scratch/survey/full-20261022.jsonl` (main 9c19034+). The fresh run is written to a new file so the baseline is not overwritten.
Caveat: the machine was busy during the run (median loadavg1 14.2, baseline 6.3, max 48.9). Timing figures are not valid evidence (see Deadline misses).

## Zero-error scoreboard

| | now | baseline |
|---|---|---|
| instruments with no error | **646 / 1395** (46%) | 674 / 1395 (48%) |
| libraries with no error | **4 / 15** | 4 / 15 |
| instruments that load | 703 / 1395 | 735 |
| instruments that sound | 688 | 717 |

Zero-error libraries (all instruments clean): Areia 1.2.0 Audio Imperia (155), Audio Imperia CHORUS (42), Performance Samples Vista (7), Una Corda Library (3).
Closest to zero: Afflatus Chapter II Brass (1 of 348 with an audible tail).

Per library, errors/total: UVI Augmented Orchestra 620/620, VWinds Clarinets 16/16, VWinds Double Reeds 15/15, VWinds Flutes 9/9, Audio Imperia Dolce 46/77, Conflux 1.1.0 34/51, Pacific Ensemble Strings 5/49, Afflatus Brass 1/348, ANALOG STRINGS 1/1, Morphology Evolved 1/1, Shreddage E-Upright 1/1.

## Load errors (692 instruments, 49.6%)

By stage: `load` 412, `translate-to-IR` 280. Two families cover everything:

| instruments | stage | cause |
|---|---|---|
| 376 | load | UVI, Prepare: "Constant property requires explicit declared-Core target reader; native temporal law is unqualified" |
| 248 | translate-to-IR | UVI, Translate: SignalConnection "shared output connection has no support" (JSONL error text is cut at about 120 chars) |
| 32 | translate-to-IR | Conflux (Conflux.nki and 31 multis): "resident PCM requires at least N bytes, exceeding configured payload budget" (regression, see below) |
| 23 | load | UVI, Prepare: "wide UVI multipart stream needs owned admission" |
| 13 | load | UVI, Prepare: "native Frequency connection attachment and typed parameter cadence are unqualified" |

The old UVI Translate refusal ("live global ... requires the actual shared owner modulator") no longer appears. The 660 UVI programs still load 0, so this is a stage move, not a fix.

Update (branch `uvi-activation-ownership`, 2026-10-23): the "source activation ownership is unproved" refusal is gone. A shared Gain/Pan source gated by an unproved activation (non-plain DAHDSR, Program/Layer LFO/StepEnvelope) is approximated per voice, owned by the routing keygroup, and reported as an `UnknownLaw` warning (`shared_envelope_ownership.rs`). `V Strings Bartok.uvip` (Augmented Orchestra) now translates and renders (6984 zones, 11 warnings). The rest of the 248 translate-stage family (MultiLFO waveforms, synced LFO frequency, StepEnvelope smoothing, nonlinear depth) still refuses. VWinds Clarinet A now stops at Prepare on the 13-program Frequency family (`native Frequency connection attachment and typed parameter cadence are unqualified`), so VWinds does not load yet.

Update (branch `uvi-prepare-families`, 2026-10-23): the "native Frequency connection attachment and typed parameter cadence are unqualified" refusal is gone. A native one-pole whose Freq is bound by a parameter is approximated per voice and reported as an `UnknownLaw` warning (`load.rs` `bind_source_processing`); the runtime now accepts Cutoff bindings on the native one-pole (`stages.rs` `processor_parameter_lane`). `VWinds - Clarinets/VWinds-AClarinet.ufs::Presets/Clarinet A.uvip` renders (peak 0.596). The 23 "wide UVI multipart stream" rows remain until the next PR.

## Ranked gaps (instruments affected)

| # | instruments | libraries | gap | lane |
|---|---|---|---|---|
| 1 | 660 | 4 UVI (Augmented Orchestra, VWinds x3) | UVI programs 0/660 load (table above) | UVI mapping / k-mod-external |
| 2 | 384 | many | Convolution: IR latency compensation bypass; IR preserve length | k-convolution |
| 3 | 349 | many | Polyphonic variable initializer ignored | k-ksp-engine-params |
| 4 | 271 | many | Polyphonic variable read outside a note callback | k-ksp-engine-params |
| 5 | 458 | many | find_group: named group not found (2 groups, index invalid) | k-ksp-engine-params |
| 6 | 105 | many | Keyswitch script: 2959 articulations found, 0 migrated to zone selectors | k-ksp-engine-params |
| 7 | 131 | many | Array index out of bounds (77 `!name`, 54 `%name`) | k-ksp-engine-params |
| 8 | 32 | 1 (Conflux) | Resident PCM over payload budget (regression) | sampler-kontakt stream budget |
| 9 | 23 / 19 / 19 / 19 | many | Modulation of module parameter / modulator intensity / modulation target; external RandomBipolar | k-mod-external, k-mod-lfo-step |
| 10 | 20 / 19 / 20 / 19 | many | set_engine_par; get_engine_par_disp_ext; `$name` not in performance view; load_array not loaded | k-ksp-engine-params |
| 11 | 20 | many | Wavetable source | k-wavetable-tonemachine |
| 12 | 17 / 14 / 12 / 10 | many | Effect; DFD preload override; keys beyond runtime pitch range narrowed; Solid G-EQ smoothing | owning FX / k-sampler-source |
| 13 | 15 | 2 (Dolce, Pacific) | Silent at a covered key | k-voice-engine |

Counts overlap across rows; do not sum them.

## Unsupported features (per code, instruments that reached the load report)

| instruments | code / feature |
|---|---|
| 384 | Convolution: IR latency compensation bypass |
| 384 | Convolution: IR preserve length |
| 349 | script Warning: polyphonic variables start at N; initializer ignored |
| 271 | script Warning: polyphonic variable read outside a note callback |
| 229 | script Warning: find_group(): group "NCllChnswStbsBrNClsDynNRRN" not found |
| 229 | script Warning: find_group(): group "NVlnsChnswStbsBrNFMpDynNRRN" not found |
| 105 | keyswitch script |
| 77 | script Warning: array index out of bounds for `!name` |
| 54 | script Warning: array index out of bounds for `%name` |
| 23 | modulation of a module parameter |
| 20 | script Unsupported: set_engine_par |
| 20 | script Warning: `$name` is not in the performance view; unbound handle |
| 20 | wavetable source |
| 19 | external modulation from RandomBipolar |
| 19 | modulation of a modulator's intensity |
| 19 | modulation target |
| 19 | script Unsupported: get_engine_par_disp_ext |
| 19 | script Warning: load_array: path not loaded |
| 17 | effect |
| 14 | DFD preload override |
| 12 | keys beyond the runtime's pitch range, narrowed |
| 10 | Solid G-EQ: smoothing |
| 8 | script Warning: `$name` clamped to its declared range |
| 7 | Solid Bus Comp: trailing field |
| 7 | Tape Saturator: Creative FX live controls |

Dropped out of the top list versus the baseline: Filter law (56), wavetable "complete N-frame cycles" (32), Freak Creative FX (17). The 412 UVI load failures never reach this report, so counts for UVI-only features are not visible yet.

## Silent instruments

15 silent at a covered key (baseline 18): Dolce 10 (Spiccato, Staccato, Pizzicato, Energetic Shorts), Pacific Ensemble Strings 5 (Trills x4, Solo Harp Harmonic Pluck).
These are the same 15 instruments that stop at stage note-on/selection with "note suppressed by its script" at key 60. Probe key 60 may be an artifact, or the script gap is real; check before fixing.

## Deadline misses

106 instruments show a 64-frame deadline miss (baseline 51). Not a valid comparison: the load average was about 2x higher.
On quiet samples (loadavg1 < 8): fresh 0 of 403 instruments, baseline 34 of 946. Re-run on a quiet machine before treating this as a gap.
Audio-thread allocations: 0.

## Script diagnostics

| instruments | kind |
|---|---|
| 653 | Warning (baseline 685) |
| 21 | Unsupported (baseline 53) |
| 1 | Approximate |
| 1 | interface |
| 1 | compile failed: Shreddage E-Upright, "get_ui_id requires a UI control" |

## Regressions versus the baseline (full-20261022)

1. **Conflux, 32 instruments** (Conflux.nki + 31 multis) went from `ok` to `translate-to-IR`: "resident PCM requires at least N bytes, exceeding configured payload budget N bytes". This is the only ok-to-failing transition in the diff. Likely cause: ef2b4313e "Bound the default streamed head budget to the host's Auto budget" (2026-10-22 18:03 +0300). Its fallback is for bounded initial-state windows, not resident PCM. Not bisected.
2. Totals: loads 735 to 703, sounding 717 to 688, zero-error instruments 674 to 646.
3. Not counted as regressions: load time median 210 to 526 ms and deadline misses 51 to 106. Both move with machine load.

Improvements: silent 18 to 15; script Unsupported 53 to 21; Convolution 416 to 384; UVI Translate "live global owner" refusal no longer reported (replaced by the stage moves above).

## Re-run

Re-run this when the machine is quiet, on each merge wave (see docs/library-inspector.md). Regenerate this file from the three summary outputs above.
