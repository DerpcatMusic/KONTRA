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

Update (branch `uvi-prepare-families`, 2026-10-23): the "native Frequency connection attachment and typed parameter cadence are unqualified" refusal is gone. A native one-pole whose Freq is bound by a parameter is approximated per voice and reported as an `UnknownLaw` warning (`load.rs` `bind_source_processing`); the runtime now accepts Cutoff bindings on the native one-pole (`stages.rs` `processor_parameter_lane`). `VWinds - Clarinets/VWinds-AClarinet.ufs::Presets/Clarinet A.uvip` renders (peak 0.596). The "wide UVI multipart stream needs owned admission" refusal is gone for multipart banks of up to 12 mono members: each member is an owned stream charged to the stream budget before any read, the first two members are routed to stereo, and the rest are reported as an `UnknownLaw` warning ("multipart channel routing"). Piccolo (10 members) renders, rms 0.0027, so its routing is quiet and unverified. Result: VWinds 36/40 load (was 0/40); the 4 remaining Contrabass Clarinet rows stop at Translate on "shared Frequency connection has no unique real Freq control", which is the translate-stage family.
Update (branch `uvi-translate-families`, 2026-10-23): MultiLFO waveform mixes (Triangle/Saw/Square with Sine) play as weighted routes normalized by `|weights| + noise`; `Smooth` on shared, non-retriggered or nonzero-start LFO/MultiLFO/StepEnvelope sources is applied as a per-route lag; a program-level retriggered LFO plays per voice (the native runtime refuses Master-scope retrigger outside MPE). Each is one `UnknownLaw` warning, not an error. Augmented Orchestra sample (42 programs, shard 0/15): quick-tier load 35 → 38 of 42 (90.5%), parse tier 32 → 38. Full-tier render of Future Contrabass, Crossbow MW and V Strings Bartok: finite, peaks -3.9, -6.6, -6.6 dBFS. Remaining sample refusals (translate): synchronized LFO Freq via MIDI CC, a modulated MultiLFO source, live LFO depth through a nonlinear connection, and LFO delay/rise with bipolar + retrigger.

Update (branch `uvi-multipart-routing`, 2026-10-23): the "UVI multipart channel routing" warning is gone. Every admitted bundle member (up to 12 mono) now mixes into stereo. The placement comes from the program itself: row `k` of the keygroup's first active `GainMatrix` gives member `k` as `[Gain_k_1, Gain_k_2]` (input `k` to left and right; `ir::BundlePlacement`, built in `lib.rs` `bundle_placement`). A member the matrix leaves silent (both cells zero, for example one driven only by a Macro) takes an equal-power even spread (`stream.rs` `even_spread`). The reader sums members with those gains (`stream.rs` `Sample::open_counted`), and the keygroup's 2x2 matrix subset is not applied again. Not modelled: Macro-modulated matrix cells use their stored value, not the modulated one, and the spread applies only where the program places no member. Piccolo (`VWinds-Piccolo.ufs`) renders at peak -12.6 dBFS (`Piccolo 2.uvip` -9.5), against -12.9 and -16.4 for the sibling VWinds programs `Piccolo Flute` and `Piccolo Noise` (the earlier rms 0.0027 is gone; the corpus render reports peak and tail, not rms). Render run: `corpus-health run --only '*Piccolo*'` with one VWinds library, 5 programs loaded ok. `stuck_notes` (8 on Piccolo) is recorded and not yet attributed. Its effect on the Contrabass Clarinet rows is not measured here; the translate-stage shared-Frequency family is unchanged.

Update (branch `uvi-multipart-followup`, 2026-10-23): every read path now places bundle members through one implementation (`placement.rs`: `member_gains`, `mix_member`). The resident decode (`audio::decode`, used by `decode_resource`, census and loose-file decode) still folded wide bundles to their first two members; it now mixes all members with the even spread, matching the streamed and dynamic (`stream_source`) readers. Test: `stream.rs` `every_bundle_member_mixes_by_its_placement_or_the_even_spread` (fails on the old fold). Corpus render of `VWinds-Piccolo.ufs` is unchanged: its bundles are mono, and the corpus renders through the streamed reader (`Piccolo 2` peak -9.46 dBFS, tail -19.8; `Piccolo` peak -12.64, tail -24.45; before and after). The corpus record reports peak and tail, not rms.

Not done, with findings:
- Macro-modulated GainMatrix cells still use their stored (or init-folded) value. Following a macro at runtime needs live per-member gains inside the bundle reader (`SampleReader` is stereo and its gains are fixed at open); this is an architecture change, not a translate-stage fix.
- Fixed (branch `uvi-clamp-loop-end`, 2026-10-23): the 70 `unreadable initial sample` refusals per Piccolo program were the loop check `end > frames` in `sample_metadata.rs` (`finish`). The stored RIFF `smpl` end is exclusive `frames*channels`, which is past EOF in the stereo FLAC noise samples (`thePFLUTEM-noise*.flac`, 176400 frames). Verified on the corpus: stored end 352800 = 2 x 176400, start 26460 is in frame units, so the clamp target is the frame count. The loader now clamps the end to the frame count (`UVI-LOOP-CLAMPED` warning, one per asset, stored and clamped values in the message) and keeps the sample playable. A start at or past the clamped end, and missing, corrupt or zero-frame files, are still refused. Corpus run `corpus-health run --only '*VWinds-Piccolo.ufs::Presets/Piccolo 2.uvip'` (tier full): load ok, 70 `UVI-LOOP-CLAMPED` warnings, 0 `unreadable initial sample`, peak -9.46 dBFS, tail -19.8. The Kontakt loader already clamps sample ends to the decoded audio (`load.rs` `fit`), so it had no matching unit bug.
- `stuck_notes` (8 on both Piccolo programs, `stuck_voices` 0) was a harness artifact, not an engine leak. Scripted UVI hosts retire ended notes in `Driver::render` -> `flush_ended`; the corpus scripted render called `Runtime::render` directly and never flushed, so retire-when-silent children that had already ended still counted. `scripted_render` now flushes like the host. Piccolo: `stuck_notes` 8 -> 0, peak and tail unchanged (`Piccolo 2` -9.46 / -19.80 dBFS, `Piccolo` -12.64 / -24.45). Regression test: `scripted_render_flushes_generated_notes_after_release`. Kontakt `stuck_notes` is a different cause; see the next entry.

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

Update (branch `fix-stuck-notes-piccolo`, 2026-10-23): `stuck_notes` on the Kontakt survey rows (88 rows in `full-20261022-fresh.jsonl`, 3 each or 2 each, all `status: done`) is not the Piccolo cause. The Kontakt rig already flushes, so the notes are live: `2 Horns AccSusCresc` and `2 Tubas AccSusCresc` each keep one script-owned `ScriptKeyUp` (`work=1`) scheduled 0.5 s past the harness stop (`minimum` = hold + tail), with `voices` 0. Rendering until the queue drains gives `stuck_notes` 0. That changes the tail window, so it is not applied to the metric. `ANALOG STRINGS` keeps 2 voices and a pending `Resume` at 6.5 s, which does not drain even after 10 s more render. That case is still open: check whether the voices are a looped sustain that never reaches its release (a real "loop never ends" leak) or only a long script wait.

Update (branch `fix-stuck-kontakt`, 2026-10-23): the Kontakt `stuck_notes` question is closed as a **metric** problem, not an engine leak. Classes below are for the 88 rows of `full-20261022-fresh.jsonl` with `stuck_notes` > 0.

- **Metric fix.** After the tail window (peak/tail numbers unchanged), the probe now settles: it renders silence until no note-owned event is scheduled and no voice is left, or 30 s pass (`settle` in `tools/corpus-health/src/main.rs`). Only then are `stuck_voices`/`stuck_notes` counted. Owed events are `Runtime::pending_note_events` (`crates/sampler-core/src/schedule.rs`): note-off, voice start, and resumes of callbacks a note owns. Plan timers (`on listener`) are excluded: they re-arm every 1 ms for the whole plan and never drain. A horizon hit is reported as `pending_script_events`. Regression test: `settle_waits_for_a_script_event_owed_after_release`.
- **ANALOG STRINGS (root cause).** The 2 voices are a slow release, not a leak: release stage, level falling from -27 dBFS at 7 s to silence by 27 s. The pending `Resume` is `prog549@pc12`, a non-entry helper program with a `wait` that re-arms itself every 1 ms (`due` = now + 1 ms at every 1 s sample). The `on listener` timer is the likely owner (callers are not recorded), so it is a plan-level timer, not a note event. The wait unit is correct: `wait` lowers to `WaitMicros`, so `wait(1000)` = 1 ms. Before the fix it read as stuck; now `stuck_voices` 0, `stuck_notes` 0 (`pending_script_events` 0, the timer excluded).
- **Re-run (single instruments).** `2 Horns AccSusCresc`: stuck_notes 3 -> 0. `2 Tubas AccSusCresc`: 3 -> 0. `ANALOG STRINGS`: 3/2 -> 0/0. Tail dBFS unchanged (-73.6, -52.4, -31.3).
- **Random sample of three (different libraries, seed 2026).** `Areia - 01 Pads`: notes 2, voices 1 -> 0/0; class **metric**. `Dark Context.nkm` (Conflux multi, program 1): notes 1, voices 2 -> 0/0; class **metric**. `Pacific - Ens Strings - Solo Harp - Normale Pluck`: notes 2, voices 2 -> 0/0; class **metric**. No engine or script fault found in these three.
- **Counts for the 88 (from the survey, not re-run).** 28 rows have `stuck_voices` 0 and only pending notes (mostly `AccSusCresc` in Afflatus and Dolce): class **metric** by construction. 60 rows have voices > 0: 41 with audible tail (> -60 dBFS after 5 s) and 19 quiet. Of the voice rows, 4 were re-run (ANALOG STRINGS, Areia, Dark Context, Solo Harp) and drained to 0, so they are **metric**. The other 56 are **not yet re-run**: a full survey is out of scope for this change. Classify them with `corpus-health run --only` per library after merge; any that still hold voices past the 30 s horizon are candidate **engine** leaks.
- **Not this branch's concern.** `Dark Context` program 1 peaks at -11.9 dBFS in the survey and -56.6 dBFS on current `origin/main` as well: the survey predates the current main, so that gap is not from this change.

Update (branch `fix-conflux-dark-context-level`, 2026-10-23): **Conflux 'Dark Context' program 1 level regression, fixed.** Survey -11.9 dBFS, `origin/main` -56.6 dBFS, all from the same instrument.
- **Culprit.** PR #356 (`61019154d`, "$EFFECT_TYPE_* and $FILTER_TYPE_* resolve to the native Kontakt 8 values") made `$FILTER_TYPE_*` resolve to PE registration values (e.g. `$FILTER_TYPE_SV_LP2` = 728). The setter stores subtype ids (`docs/re/kontakt-bindings/filter-type-ids.tsv`: SV_LP2 = 52), and Kontakt coerces unaccepted ids to 0. With the values resolved, Dark Context's `on init` takes the filter-subtype branch and stops applying its later LFO, envelope and effect writes (9.5k of 18.6k statements run).
- **Fix.** `$FILTER_TYPE_*` stays unresolved (opaque, as before #356). `$EFFECT_TYPE_*` still resolve. Test: `module_type_constants_use_the_native_values` (`crates/sampler-ksp/src/builtins.rs`).
- **Open.** Resolving filter names to the measured subtype ids is still correct per the measurement, but the init cut it exposes is not fixed (the path leaves `on init` before its tail). Re-enable once that cut is fixed; the test pins the current behaviour.
- **Other Conflux multis, single-instrument run after the fix (survey peak → now, program 1):** Big Screen -13.81 → -13.81; Blessed Emotions -11.47 → -11.91; Cold Antarctica -17.68 → -17.68; Conflux Arps -13.14 → -15.39; Different Places -17.38 → -17.38. No drop of the Dark Context kind. Conflux Arps and Blessed Emotions moved 0.4 to 2.3 dB; not investigated.
