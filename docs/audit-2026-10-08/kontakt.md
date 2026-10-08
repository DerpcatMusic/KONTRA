# Kontakt playback audit — 2026-10-08

## Verdict and boundary

**Worse than v1 in important playback paths; better in some measured paths; neither has Kontakt parity.** The product requirement “better in every aspect, as Kontakt plays it” is not met. The strongest native-reference regression is Areia Sustained: v1's source detector identified 48/48 notes, historical v2 only 6/48; both chose the wrong dynamic/sample, and v2 added approximately 1.89 seconds of start displacement on high notes. Una Corda level and Vista source agreement improved substantially. Do not turn these mixed results into a blanket claim that every library regressed.

Audited integration: **`7e82b152`**, isolated branch `audit/kontakt-20261008`, worktree `audit-kontakt`. No playback fixes are included. Evidence is frozen in [kontakt-measurements.json](kontakt-measurements.json); it contains derived counts/names/numbers, not library payloads, scripts, samples or access material. `MAIN_STORAGE` was mounted. Corpus executable SHA256: `094d810365d2c24f3a1ad4005278c7ac9d14985e96fff4d3c39097f33c985d16`. Source references below are relative to this audited revision unless a branch is named. Later integration changes require rerunning the checks.

Evidence levels are deliberately separate:

- **Fresh:** two Kontakt-only quick shards, `0/16` and `1/16`: **106 completed, 106 loaded, 98 ok, 2 needs-controller, 6 note-on/selection failures**, no recorded runtime script faults. Seven focused full checks plus three full regression checks completed. Their short-note success establishes audible finite output, not the correct native performance.
- **Historical corpus corroboration:** `~/.cache/kontakto-corpus/full3.jsonl`: **834** instruments/multis, **781 NKI + 53 NKM**, 11 top-level library families. **726 ok, 51 needs-controller, 40 note-on/selection, 12 release, 5 render**. The cache does not establish its integration source revision; it is not a fresh whole-corpus result.
- **Historical native PCM:** `v2/kontakt-reference@7d15e5b8`, `docs/architecture-v2/probe/{una_cotton,areia_vln_sus,analog_strings,barbarian,vista_3cellos}.json` and its protocol/recordings. Five instruments, **48 notes each** (three keys × four velocities × four repeats), native Kontakt plus v1/v2. These recordings predate this fresh audit binary; do not present their scores as freshly rerendered `7e82b152`.
- **Fresh v1:** clean release binary `~/.cache/kontra-reference/bin/kontakto-v1`, build revision **`0cb7a8a0b4d43086596a64c77320caa1b26d6d98`**, hash `8d4a60d753a6d851`. Five `playback-audit` runs, four cases each, safe derived numeric output only. Its MIDI/pedal grid differs from corpus-health; cross-grid silence is a regression candidate, not a matched-note proof.
- **Fresh KSP:** copied the existing audit suite from `v2/gpt-ksp-audit@a1446ec6` into this worktree temporarily; **43 probes, 3 passed, 40 failed** with `--include-ignored`. Removed the temporary copy after measurement. These are executable semantic counterexamples, not native PCM comparisons.

## Ranked findings

Ranks prioritize breadth in **library families**, then audible severity and confidence. Exposure counts overlap and include bypassed/muted state. An exposure is not an independently confirmed bad performance. P0 means demonstrated user-visible breakage; P1 means a compatibility gap with a plausible audible path; P2 means metadata/diagnostic/coverage work. S/M/L are implementation effort, not research certainty.

| Rank | Severity / breadth | Evidence and root cause | Concrete fix / effort |
|---|---|---|---|
| 1 | **P0**, engine writes exposed in **11/11 families, 834/834 files** | KSP engine-write probe still fails: ordinary authored volume remains unity. The ranked audit finds 663 of 676 parameter identifiers lack a real consumer (recovery-inclusive 2,741-NKI census). `sampler-ksp` mirrors values but cannot make them sound; `crates/sampler-kontakt/src/effects.rs:174` harvests only five effect parameters at init. Getter/display mirrors and hashed `find_mod` addresses compound wrong mic, articulation, envelope and layer decisions. | One addressed parameter service shared by init/UI/note/listener paths; preserve physical group/mod/FX identity, bind getters to authored/live state, dispatch DSP updates in `sampler-core`/`script_params.rs` and host `src/plugin.rs`. Implement high-use parameters first; merge verified readers, not another independent mirror. **L**. |
| 2 | **P0** synthetic / **P1** library attribution; init writes **11 families**; UI forwarding 2,018 NKI paths in prior census | Init setters update isolated script state; only five writes are harvested for effects, with additional special cases for bus volume, bus routing and mod-target intensity (`library.rs:882–897`). Runtime event/service effects end at `src/plugin.rs:892` and `sampler-ksp/src/lib.rs:311` UI application. Persistence callbacks, listener-generated notes, IR async and stop/reset requests cannot all reach the engine through that path. Passing a one-note CLI test does not verify plugin control interaction. | Route all runtime effect requests through the same engine service and lifecycle as #1, including async completion and cancellation, and rerun live knob/keyswitch/pedal cases. **L**, overlap with #1. |
| 3 | **P0** native-reference mismatch / **P1** cause attribution; dropped FX exposed in **10 families / 87 files**, algorithm/shape warnings in 4/3 families | `effects.rs:208` has only nine parameter families; other decoded FX become gain-only/unmodeled signal paths. Group inserts are put wholly before the amplifier (`library.rs:628`), ignoring `fx_idx_amp_split_point`; group compressor sees individual voices, not the native group sum. Historical Areia v2 level error +26.95 dB and Analog +4.27 dB are real; no single dropped field has yet been causally isolated as the complete explanation. | Merge typed FX readers and explicit malformed-slot reporting; honor amplifier split/group bus sum, port native verified DSP laws, add on/bypass matched renders. `effects.rs`, group/processor IR, core group mixing. **L**. |
| 4 | **P1**, module modulation **9 families / 363 files**, `find_mod` **9 / 780** | `library.rs:900–950` executes only volume, pitch, play position and recognized filter cutoff; other module targets are dropped. Envelope-time/FX routes and name-based module lookup therefore cannot select/update their intended parameters. New `Instrument.source_parameters` is storage, not execution. | Merge `v2/gpt-format-fxmod@b7f6af0b` retention, then lower named/physical targets to real source/processor addresses; consume normalized laws only where verified. `library.rs`, IR route types, core modulation/processor update paths, KSP lookup. **L**. |
| 5 | **P1**, saved string arrays **5 families / 422 files** | Shared Conflux smoke observes 13 raw `!` entries and zero admitted `!` entries. `library.rs:1460` handles `$ ~ @ % ?`, not `!`; persistence branch's typed `SavedEntry` is not wired. `eval.rs:337` already restores menu index→value and ordinary repeated tails, but retains pending entries so early `read_persistent_var` can be overwritten at end-init. Persistence callback identity and four-valued recall policy still fail. Wrong saved articulation/CC routing can sound “loaded” but incorrect. | Merge `v2/gpt-decipher-persist@6ae15820` reader (`847d4670`), use declaration context and consume early-restored entries, restore string cells, distinguish UI arrays, run correct post-restore callback and snapshot engine policy. `library.rs`, KSP `eval.rs`, snapshot host service. **M/L**. |
| 6 | **P0** synthetic / **P1** library mapping; signed depth **4 families / 80 files**, pan **3 / 53** | `library.rs:945` drops negative unipolar targets instead of using flag `0x02` as independent signed intensity; pan is not a destination. ReleaseVelocity and RandomBipolar are decoded but absent from lowering. This reverses or removes authored velocity/CC/pitch/filter behavior. | Consume `ModTarget::signed_intensity`, preserve independent invert/shaper semantics, add pan and measured source/destination laws. `library.rs`, core `voice_mod.rs`/modulation. **M** for signed depth; **L** for all destinations. |
| 7 | **P0** synthetic / **P1** source-index and library attribution; KSP current-group selection 1,552 NKI paths, source identity 369; fresh silent patches in 3 families | Current-event `ALLOW_GROUP` still fails its probe; muted groups are omitted (`library.rs:566`), shifting source numeric group identity. `script_params.rs:580` event source is host/script Boolean instead of creator slot; `$CURRENT_SCRIPT_SLOT` init is zero. Native criteria only support one key row (`keyswitch.rs:59–72`); default keyswitch uses lowest key rather than saved default. | Keep source→runtime group/zone/mod/slot maps, apply current-event changes to current selection, implement composed native criteria and saved default, carry creator/callback identity. `library.rs`, `keyswitch.rs`, KSP event model and core selection. **L**. Do not infer all 347 “undefined voice group” Afflatus files are audibly bad: many are sentinel references. |
| 8 | **P0** historical release failures / **P1** current extent; loops **3 families / 61 files**, native start criteria **2 / 4** | `library.rs:1120–1146` picks one loop and rejects counts/tuning. Native RR/random/controller/compound criteria are dropped. Release-counter reset has no consumer (1,923 prior NKI paths); wait/callback/stop semantics fail. Historical corpus has 12 release-stage failures; full fresh Areia and Analog checks retain voices, but finite authored long tails cannot be called stuck solely by a fixed observation window. | Preserve all eight loop slots, count/tune/transition laws, native cycle state and criteria arbitration; consume release reset/stop-wait, test retrigger/release age, sustain/sostenuto and panic. Core loop cursor, selection, script scheduler. **L**. |
| 9 | **P1**, source engine **2 families / 52 files**; Conflux 51 including multis, Morphology 1 | Wavetable fields are decoded but no oscillator engine executes them (`library.rs:588–603`). Fresh Conflux default String Ensemble is audible; this does **not** validate its Wavetable layer. Most “source mode played as sampler” warnings are DFD (832 files), which uses streaming and is not evidence of 832 wrong engines. Tone/Time/Beat/Pro modes need separate laws. | Merge source readers, add mode-specific playback and KSP source controls, validate wave scan/form/phase/randomness with native mode-isolated probes. `library.rs`, source IR/core oscillator. **L**. |
| 10 | **P1** library correctness / **P2** mapping validation; resource/snapshot identities | Linked script names occur in 833/834 instrument/multi files but current translation uses embedded source. 100 Una Corda snapshots lack an established parent identity. Gaps branch's four-word Program-resource guess conflicts with complete RE reader grammar (see below). Incorrect binding can silently select old scripts/state/resources. | Merge linked-script resolution separately; commit/rebase complete RE Program reader, reconcile metadata parser to it, bind snapshots to verified parents and fail explicitly when ambiguous. `library.rs`, `metadata.rs`, resource/snapshot resolver, Program reader. **M** for linked resources; **L** for verified snapshot graph. |

Additional P2: the health harness overstates what “ok” or “round robin suspect” proves; improve it before using it as a parity gate. `tools/corpus-health/src/main.rs:1135` creates four short repetitions but collects script-generated attack records as additional takes. Its zone table reports stored `start/start_range/reverse`, not the final runtime cursor including `play_note` offsets. **52/106** quick records have heuristic suspects; these are not 52 proven native mismatches. Preserve host take/event parent identity, report executed source and cursor, and check event/render errors (**M**). `sampler-kontakt/tests/real_libraries.rs:90` explicitly disables scripts in basic rendering coverage.

## Measured libraries and v1 comparison

**Addendum 2 — shared scan is authoritative for load/play regressions.** The census agent owns `~/.cache/kontra-scan/bin/kontra-scan-{v1,v2}`, their adjacent README and `~/.cache/kontra-scan/results/{v1,v2}.tsv`. The installed extension is `tools/kontra-scan@9dcf05e59cbc67e45c56872e1418817693150085`, based on audited integration `7e82b152`; pinned Kontakt v1 remains `0cb7a8a0` with scanner source `8597f32ed5408cf202617627e77cd65b05ccb235`. The separate v1 UVI sidecar has product base `4bffbb18`, source `1c198e60`; it must not be presented as pinned Kontakt v1. The serial 1,494-ID sweep is running in ≤235-second shared shards. **Coverage is partial**: at this read both published TSVs contained headers only; the current v2 binary had three smoke cache records (one Kontakt, two UVI), while v1 had no cache records. An older v2 Conflux cache with a different binary digest is excluded. The matched per-instrument **“v1 plays, v2 does not” list remains pending paired current results**; absence of a v1 row is not failure.

No replacement collector or duplicate sweep was built. Join the documented instrument ID and inspect `audition_status`, per-program note plans, loader/compile/init/persistence/runtime status, sigil admission and sample counters in the shared detailed cache. V2 establishes the common note first; `audition-mismatch` prohibits an audio regression claim. The first nine columns stay stable. Missing/unknown metrics stay distinct from zero; load admission, one audible half-second note and successful paint remain distinct from native playback fidelity. Request extra instrumentation through the census owner/coordinator. Future completed results should report exact instrument paths, unique family counts, source/binary digests and result hashes.

**Current shared Conflux witness:** binary SHA256 `18fe63fe07e62ea3c012ce29fc2d01b08f518ea808459446478cc46aca924b6d`; cache SHA256 `794f975cae0a997cde78459d8f2fcc6a1d7e174f9127e84b75cf046df485c5c6`. Load and audition **yes**, note60/velocity64 from zone coverage, `matched-note-plan`; three active/compiled/clean scripts, three init completions, two persistence completions, zero load faults and zero observed runtime fault records. Raw saved sigils include **13 `!` entries**, admitted sigils omit **all 13** (other scalar/integer-array/text counts match): a concrete Conflux witness for finding5, not only a warning-count exposure. **38,249,160 resident sample bytes, zero observed underruns**. Visible binding readback **107/113**, paint succeeds, but one of four requested images is not found (three decode successfully), so `ui=missing-images`. Declared main-page background `[240,239,228,255]` occupies **93.8985%** within the scanner’s pixel tolerance; this is not a native blank-UI diagnosis. Load17,338ms and peak RSS230.84MiB describe this scanner worker/protocol, not an apples-to-apples v1 comparison. None of these successes validates Wavetable mode, gestures, original-view default or the KSP semantic counterexamples.

Historical reference percentages use the detector's identified native samples as the denominator. Unidentified audio is not silence. “Same offset” permits **2,400 frames / 50 ms at 48 kHz**, so even 100% is not sample-accurate. Direction agreement is only on mutually identified sources and does not validate reverse playback.

| Library / 48-note native probe | v1 | Historical v2 | Conclusion |
|---|---|---|---|
| Areia 16 Violins Sustained | identified 48; same sample 0%; mean level error +4.55 dB | identified **6**; same sample **0%**; mean level error **+26.95 dB** | v1 plays it, v2 plays it wrong and materially worse in this reference. Both select wrong dynamics; v2 high-note starts ~90,601/90,742 frames versus v1 6–19 frames. |
| Una Corda Cotton | 48 identified; sample 91.7%; offset 81.8%; RR sequence 8/12; level −6.00 dB | 48 identified; sample 91.7%; offset 84.1%; RR 8/12; level +0.06 dB | v1 plays it; v2 still wrong on some selections/offsets, but improved level. At key60/velocity20 v2 starts ~3,086 frames versus Kontakt ~8–22; repeated source/start state needs isolation. |
| ANALOG STRINGS | 7 identified; sample 24.1%; offset 14.3%; RR 1/3; level −3.05 dB | 19 identified; sample 34.5%; offset 100%; RR 1/3; level +4.27 dB | Both play and select incorrectly. V2's source/offset coverage improves, level does not. Kontakt Tape Loop versus v2 Wide Strings Pad on repeated notes is a concrete wrong-source case. |
| Afflatus Barbarian Brass | 32 identified; sample 50%; offset 100%; RR 4/8; level −9.30 dB | 32 identified; sample 50%; offset 100%; RR 4/8; level +2.13 dB | V1 plays it; v2 still wrong, improved level. Key60 mic/articulation mismatches affect both. Fifteen silent notes also occur in the mapped-range experiment; do not count them as unexplained v2 regressions. |
| Vista 3 Cellos | no source identified; audible; sample 0%; RR 0/3; level −12.29 dB | 36 identified; sample 92.3%; offset 100%; RR 3/3; level +0.89 dB | Clear v2 improvement; still incomplete source coverage. Kontakt starts 0–722 frames while v2 starts zero; tolerance hides those differences. |

The old reference's unsent CC7 produced a native −6.0206 dB default gain. Use only its matched-protocol report scores with explicit CC1/7/10/11/64; do not combine earlier one-off loudness numbers with that grid. GUI header/load calibration also varies by run. Repeat-sequence equality alone is not sufficient for random RR: compare set/distribution and reset phase.

Fresh focused v2 results (`full`, 48 kHz, one worker):

| Instrument | load ms | peak dBFS | retained voices | status |
|---|---:|---:|---:|---|
| ANALOG STRINGS | 2,667 | +4.09 | 1 | ok, heuristic articulation change |
| Barbarian Brass / Performance | 329 / 158 | −24.45 / −16.12 | 0 / 4 | ok, stored-offset heuristic |
| Areia 16 Violins Sustained | 4,077 | −4.12 | 36 | ok, long release/articulation heuristic |
| Conflux | 5,366 | −15.94 | 0 | ok, default sample mode only |
| Vista 3 Cellos | 225 | −43.30 | 0 | ok, generated-note articulation heuristic |
| Una Corda Cotton | 149 | −24.68 | 0 | ok |
| Dolce 7 1st Violins Legato | 1,579 | −16.52 | 0 | ok, stored-offset heuristic |
| Solo Cello Tremolo | 658 | −1.34 | 0 | ok, generated-note articulation heuristic |
| Pacific Lite Full Strings Trills | 47 | no output | 0 | note-on/selection, script suppresses key60 |

**Named v1-playable / v2-wrong list supported by native probes:** Areia Sustained, Una Corda Cotton, ANALOG STRINGS, Afflatus Barbarian Brass. This includes shared defects and improvements, as indicated above. It is not a claim v1 is Kontakt-correct.

**Current silence candidate:** Pacific Lite Full Strings Trills. V1 full pedal suite produces peak **0.16194**, with 18 voices when its later key is played; v2 corpus key60 is suppressed in both quick and full runs. Crucially **v1 key60 is also silent initially**; the test grids differ. Thus this is not yet a matched-note regression or proof v2 cannot play any valid note. Scan authored range/articulation and repeat the same later-key sequence on both engines before assigning a root cause.

**Not regressions established by this audit:** Areia 4 Double Basses Core Techniques is silent in all four fresh v1 cases and rejected by v2 group selection; do not list it as v1-working. Dolce Legato and Solo Cello Tremolo play audibly in both fresh suites (v1 peaks 0.83061 / 2.53829), though neither has a matched native parity score here. Conflux plays in both (v1 peak 1.46671) but Wavetable/UI/host-service fidelity remains open. V1/v2 timings above are warm/different loaders and test protocols; no controlled speed/RSS verdict is inferred for this playback audit.

Historical full3 family accounting:

| Family | files | ok / needs-controller / selection / release / render |
|---|---:|---|
| Afflatus Brass | 348 | 348 / 0 / 0 / 0 / 0 |
| Areia | 155 | 85 / 37 / 31 / 0 / 2 |
| Solo | 100 | 90 / 0 / 0 / 10 / 0 |
| Dolce | 77 | 67 / 1 / 5 / 1 / 3 |
| Conflux (NKI + multis) | 51 | 51 / 0 / 0 / 0 / 0 |
| Pacific | 49 | 45 / 0 / 4 / 0 / 0 |
| CHORUS | 42 | 28 / 13 / 0 / 1 / 0 |
| Vista | 7 | 7 / 0 / 0 / 0 / 0 |
| Una Corda | 3 | 3 / 0 / 0 / 0 / 0 |
| ANALOG STRINGS | 1 | 1 / 0 / 0 / 0 / 0 |
| Morphology | 1 | 1 / 0 / 0 / 0 / 0 |

## Branch consolidation: decoded does not mean consumed

| Available work | Pin | Reuse and remaining work |
|---|---|---|
| format-gaps | `v2/gpt-format-gaps@56ca1e44` | SaveSettings/QuickBrowse/FileTable/metadata census and linked-script loader. Reconcile Program resource parser rather than merging its four-word suffix assumption wholesale. |
| objects | `v2/gpt-format-objects@c8e2fbce` | Versioned borrowed Program/Group/Zone/loop/source readers, full 128-slot voice-limit metadata and IR retention. This does not implement native criteria, source engines or extra loops. Local research checkout was older `cadd1a67`; latest remote commits checked. |
| FX/mod | `v2/gpt-format-fxmod@b7f6af0b` | Typed 35-layout FX coverage, corrected integer compressor mode and packed fields, signed-depth alias, snapshot modulation overlays and located retention. DSP mostly still absent. Ladder leading-field/v0x92 correction is already in integration (`097e4bc9`); do not redo it. |
| legacy | `v2/gpt-format-legacy@df7e322f` | Bounded FileContainer preset/sample resolution, streaming/resident source agreement, AIFF/AIFC PCM and corrected NKS extraction. No installed legacy fixture proves playback; no XML-to-IR translator is supplied. |
| persistence | `v2/gpt-decipher-persist@6ae15820`, reader `847d4670` | Typed `SavedEntry`, string arrays, declaration-context array policy and bounds. Menu restore and ordinary repeated-tail filling are already integrated (`88fdfd6b`); do not call those missing. |
| KSP audit | `v2/gpt-ksp-audit@a1446ec6` | Existing 43-probe counterexample suite and ranked findings. Evidence, not a semantic fix branch. |
| native DSP laws | `v2/gpt-decipher-dsp@aa3430d6` | 1,222 original-machine-code result vectors and specification (including AHDSR lifecycle follow-up), not a full kernel implementation. Control cadence, host note-off and full reset remain open. |
| reference | `v2/kontakt-reference@7d15e5b8` | Matched MIDI/reference protocols, probes/recordings. Historical suite, not current integration PCM. |
| RE addendum | `feat/decipher-readers-v2`, base `2fb8c926`, **uncommitted** | Complete Program public reader and FX silent-failure reporting in read-only `decipher-readers-v2` (14 dirty files). Available work needs owner commit/rebase/merge. Program reader SHA256 `b95ebc29885b57483db5bb655f00f500631e5bfc51e075841bc53d468003ab84`. Parent reports 15 reader, 32 compatibility and 30 importer checks; no owner changes made here. |

The following ledger names every decoded field family in those branches that lacks executable translation/runtime consumption, separating fields already consumed and fields whose meaning is still unknown. Opaque private/tail bytes are **not** decoded fields, and no audible fix is promised for unassigned scalars.

### Program, resources, files, bank/container and RE public tails

Already consumed: Program name, transpose, gain/pan/tune; sample filenames; script embedded source and most scalar/numeric-array saved entries. Not consumed or incomplete:

| Exact fields | What consuming them would fix / limits |
|---|---|
| Program `low_key`, `high_key`, `low_velocity`, `high_velocity` | Enforce instrument-wide note/velocity acceptance independently of zone mapping. |
| `default_key_switch`, `group_solo` / new `group_solo_byte` | Saved initial native articulation and native solo arbitration. Currently lowest detected key wins; group mute alone is insufficient. |
| `dfd_channel_preload_size` | Authored preload policy; needs validated units/global override, not a guessed start offset. |
| `num_bytes_samples_total`, `library_id`, `fingerprint`, `loading_flags`, `cat_icon_idx`, `instrument_credits`, `instrument_author`, `instrument_url`, `instrument_cat1/2/3` | Saved estimates/resource identity/UI metadata; unassigned flag/fingerprint laws do not establish an audio defect. |
| RE `resource_container_filename_ref` (F0), `filename_ref_1` (F1), `terminal_filename_ref` (F2) | Exact versioned file references enable safe resource binding; F2 artwork use is an inference needing validation. Old prefix fields `resource_container_filename` / `wallpaper_filename` are not a complete suffix model. |
| RE `discarded_strings[2]`, `tail_string_0`, `tail_string_1`, `word_0`, `byte_0`, `word_1`, `bytes_0`, `word_2` | Complete versioned preservation; meaning unassigned. `bytes_0` is counted bytes, not text. Cannot assert these fix voices/articulation. |
| RE `sound_data_0`, `sound_data_1`: `presence`, `body.metadata`, `body.groups`, `body.value_presence`, `body.value[16]` | Inline sound/resource descriptor retention; nonzero presence is not restricted to 1; 16-byte value is unassigned, not certified UUID. No source selection law established. |
| RE sound metadata `leading_words[2]`, `strings[5]`, `words[7]`, `string_groups`, `string_list`, `pair_list_0`, `pair_list_1`; sound group `string`, `items`; item `string`, `float_0`, `float_1`, `word_0`, `word_1` | Every nested field is available in the reader but absent from translator/runtime. Bind metadata/host resource services after semantics are established; no oscillator/DSP claim. |
| Bank `name,volume,tune,tempo` (owned API `master_volume/master_tune/master_tempo`), ProgramList program-number i16 and SlotList physical-slot u8; Container name/gain/pan | Bank/container master context and program selection. Flattening programs loses authored slot/address context. Installed NKB count is zero; no native bank parity claim. |
| FileTable v2 sample timestamps (`u64`) and retained per-record unknown word (`u32`), v3 eight-byte prefix/twenty-byte suffix, special/other filename namespace metadata and segment tags | Integrity/provenance and unambiguous sample/resource lookup. Timestamp/unknown bytes have no proved audio law; path flattening loses namespace/anchor context. |
| Script `linked_script_filename`, editor-open/touched flags, password/hash metadata | Linked filename must resolve the intended resource script; editor/password flags are authoring metadata. Bypass is already used. Resolver branch prefers bounded loose/NKR resources; validate native precedence rather than assume any nonempty link means embedded script is wrong. |
| SaveSettings v0x10 `translated:u32` (BFNtrns), `original:i32` (BFNorigi), `unknown:i32` and `flags[3]` (15-byte body); QuickBrowse v1 unknown i32 | Preserve exact authoring settings (semantic names incomplete), no proven playback correction. |

**RE conflict to resolve before merging:** `gpt-format-gaps/crates/sampler-kontakt/src/metadata.rs:202–240` reads four consecutive u32 values as container/snapshot-directory/full-path/wallpaper after the Program prefix. The native-versioned public grammar is F0, then versioned UTF-16 strings, sound data, scalar additions and F1/F2—not four consecutive filename IDs. Its word-based interpretation can read string lengths/code units as refs. The complete reader handles `{0x80,0x82,0x90,0x91,0x92} ∪ [0xa0,0xb5]`, preserves W1 unconditionally from a8, and has 25 real public fixtures with zero errors (four b5 templates); present inline S bodies currently have authored fixtures only. `Program::params()` remains prefix-compatible, so merely merging the new `public_record()` API does not consume any new field. **Program private reader still returns Unsupported**; no private voice/HQ policies have suddenly been decoded.

Snapshots: 1,103 state files are distinct from 834 independently loadable instruments/multis. Metadata candidates associate 701 Analog + 201 Conflux + 101 Morphology snapshots; 100 Una Corda parents remain unestablished. Candidate identity is not a tested preset binding. NIS ControllerAssignments appear in all 1,937 containers but are still unread: **not a decoded-unconsumed field**.

### Groups, zones, voice limits, criteria and loops

| Exact unused / partially consumed fields | Correction enabled |
|---|---|
| Group `midi_channel`, `soloed`, `interp_quality`, `fx_idx_amp_split_point` | Channel filtering, solo arbitration, interpolation policy and native pre/post-amplifier insert order. Group gain/pan/tune, reverse, key tracking, release flags, voice assignment and mute are already used; mute's omission causes identity compaction. |
| Voice-limit `name`, `exclusion_group` | Name-based identity/UI and exclusion/choke classes. Full 128-bit mask and occupied slots are already read by executable translation; `kill_mode`, `prefer_released`, `max_num_voices`, `ms_fade_time` are used. “Any”→quietest needs native validation, not another reader. |
| Criteria `mode` except one mode1 row; `next_criteria`, `controller`, `cc_min/max`, `cycle_class`, `slice_zone_idx`, `slice_zone_slice_idx`, `sequencer_only`; list mask/physical rows | Controller/RR/random/slice and combined criteria; retain source row addresses for KSP. `key_min/max` only consumed for single native key row; other rows are ignored. Numeric operator association still needs controlled native checks. |
| Zone `filename_prefix[6]`, `sample_data_type`, `sample_rate`, `num_channels`, `num_frames`, `reserved1/2/3/4`, sample metadata `root_note`, `tuning`, source group/zone IDs | Validate authored source metadata versus actual decoded sample; preserve stable KSP identity and versioned reference boundaries. Sample metadata root/tuning must not be blindly added to the already consumed zone root/tune. Unknown reserved/prefix semantics have no audio law. |
| Loop original physical slot, every occupied loop after the first; `loop_count`, `loop_tuning`; unsupported mode values | Counted/multi-loop transitions and tuning after first jump. First-loop start/length/crossfade, alternating direction and supported lifetime are already used. Serialized mode2↔native lifetime remains unverified; KSP microseconds versus stored frames requires sample-rate conversion. |

Zone sample start/end/modulation range, inclusive key/velocity ranges/fades, root key, zone gain/pan/tune and filename ID are consumed. Group and zone reverse reach playback; a missing reverse engine is **not** established. Correctness of native reverse start/end and script offsets requires an actual reverse fixture. Group private triangular masks and retained extension bytes remain opaque, outside this decoded ledger.

### All decoded source-mode parameters

`SrcMode` storage is available; none of these fields controls an executable mode-specific engine:

- Common: `common_float_7`, `common_flag_11`, `common_enum_12`, `common_flag_16`, `timing_value`, `timing_unit`, `timing_free`, `timing_flag`.
- Machine modes: `machine_float_1`, `machine_float_2`, `machine_flag`.
- Slice mode: `slice_float_1`, `slice_float_2`, `slice_flag_1`, `slice_flag_2` (newer version).
- DFD: `dfd_flag`, `dfd_integer_1`, `dfd_integer_2`.
- Pro: `pro_flag_1`, `pro_flag_2`, `legacy_pro_integer`, `pro_float_1`, `pro_float_2`.
- Wavetable: `position`, `form1`, `phase`, `phase_random`, `form_type`, `quality`, `inharmonic_enabled`, `inharmonic`, `form2`, `form2_type`, `mod_wave`, `mod_type`, `mod_amount`, `mod_tune`, `nested_word_83/87/91/95`.

Stored `mode` is read for diagnostics, then played as sampler. The serialized/internal enum permutation is established; non-Wavetable names and many scalar units remain provisional. Wavetable fields would drive scanning/shaping/phase-per-trigger; machine/slice fields require mode DSP and independently established units; DFD fields concern streaming policy. Unassigned common/nested words enable retention/research, not safe audible execution. Do not treat snapshot's 32-byte source identity as this whole schema.

### FX fields not executed

All names below are the typed branch reader's serialized field names, not an assertion of exact physical/KSP normalization. Unsupported modules currently may retain wrapper output gain/bypass; that does not execute their parameter body. For these complete bodies, **every listed field is unconsumed**:

| Module | Exact fields / fix enabled |
|---|---|
| Delay `10` | `time_ms,damping,pan,feedback,time_unit,time_free_ms,sync_unknown,sync_flag`: delay/filter/feedback/pan and sync processing. Version50 omits timing additions. |
| Chorus `11` | `depth,speed,phase,speed_unit,speed_free,sync_unknown,sync_flag`: chorus voices and synchronization. Version50 omits timing additions. |
| Flanger `12` | `depth,speed,phase,feedback,color,speed_unit,speed_free,sync_unknown,sync_flag`: flange/filter/feedback. Version50 omits timing additions. |
| Phaser `14` | `depth,param_1,speed,param_3,speed_unit,speed_free,sync_unknown,sync_flag`: phaser plus sync; neutral parameters still need units. Version50 omits timing additions. |
| Limiter `1c` | `in_gain_db,release_ms`: native limiting and release. |
| Shaper `1d/v80` | `param_0,mode`: distortion shape/mode. Version-sensitive: older v70 Surround Panner is different and not decoded by this schema. |
| Distortion `1e` | `mode,drive,damping`: integer mode and drive/filter. |
| Lo-Fi `20` | `bits,frequency,flag_2,noise_level,noise_color`: bit/sample-rate/noise processing; packed byte correction must be merged before use. |
| Skreamer `21` | `tone,drive,bass,bright,mix`: distortion/tone/wet blend. |
| Rotator `22` | `speed,balance,accel_hi,accel_lo,distance,mix`: rotary stereo dynamics. |
| Tape `42` | `gain,warmth,hf_rolloff,quality`: tape curve/filter/quality. |
| Transient `43` | `input,attack,sustain,mode`: transient processing; mode is integer. |
| Solid G-EQ `44` | `lf_gain,lf_freq,lf_bell,lmf_gain,lmf_freq,lmf_q,hmf_gain,hmf_freq,hmf_q,hf_gain,hf_freq,hf_bell`; v11 also `param_12,param_13,flag_14,flag_15`: native EQ sections; added field semantics remain open. |
| Solid Bus Comp `46` | `threshold,ratio,attack,release,makeup,mix,link`; v11 adds `flag_7`, v12 adds `param_8`: compressor/mix/link with version-safe decoding. |
| Feedback Comp `4c` | `input,ratio,attack,release,makeup,mix,param_6,hq_mode,link,flag_9`: feedback detector/compression/link. |

For neutral extended layouts the exact field set is **`param_0` through `param_(N−1)`**, inclusive. `f/i/b` below is exact float32/int32/byte order; every position is retained but unconsumed. These fix reader loss first; native semantic binding and DSP are still needed before enabling the effect. This avoids inventing parameter names from the KSP manual order.

| Module/version | Complete order | N |
|---|---|---:|
| Legacy Reverb `15/50` | `fffff` | 5 |
| Jump `4d/10` | `ffffffbb` | 8 |
| DStortion `56/10` | `ffffffb` | 7 |
| Phasis `5b/51` | `fffffffffbbbfffb` | 16 |
| Flair `5c/51` | `ffffffffffbiiifffb` | 18 |
| Choral `5d/51` | `fffffffbbi` | 10 |
| Supercharger `60/51` | `fffffffiiiib` | 12 |
| Psyche Delay `63/10` | `ffffffbbbfffb` | 13 |
| Raum `65/10` | `iffffffffffffbbfffb` | 19 |
| Bite `66/10` | `iffffffffffb` | 12 |
| Freak `68/10` | `fffffffffffffbbbbbi` | 19 |
| Vibrato Chorus `69/10` | `iiffffb` | 7 |
| Wow/Flutter `6a/10` | `fffffffbf` | 9 |
| Gater `72/01` | `ffffbbbb` | 8 |
| Reverse Grain `74/01` | `ffffbbb` | 7 |
| Replika `5a/10` | `iffffffffffbbbfffb` | 17 |
| Replika `5a/11` | previous + `ffffb` | 22 |
| Replika `5a/12` | previous + `fffffffffb` | 32 |

Partially consumed bodies must not be called wholly absent:

- Stereo Modeller `1f`: `spread,pan` used; **`pseudo_stereo` unused**. Gainer `13 gain` and Inverter `1a flag_0/flag_1` are used.
- Compressor `19`: mode's **integer representation** is decoded correctly only on FX/mod branch; integration reads it as float at `effects.rs:307`. Classic bus threshold/ratio/attack/release/link are used. Enhanced/Pro mode law and all **group-summed compressor processing** are absent. Do not mark these bus parameters as unused.
- Send Levels `17`: `sends` used on buses; **`outputs` unused**, and group send topology is not executed as the native group sum.
- Filter/EQ `18`: filter subtype/cutoff/resonance and legacy EQ bands are translated where supported. **`native_flag`, Ladder `leading_value`, remaining `extra` fields** are not executed; Daft leading value is used. Non-modeled subtype laws still need kernels. Duplicate type word is validation, not a second parameter. Current legacy EQ curve/shape remains approximate.
- Convolution `16`: **`block_size,early_length,early_low_cut_hz,early_high_cut_hz,late_length,late_low_cut_hz,late_high_cut_hz,xpoint,preserve_length,bypass_latency_compensation`** are not executed; `decimation` is diagnostic only when >1. `ir_index,predelay_ms,reverse,auto_gain,volume_envelope,curve_x,curve_db` do affect preprocessing/bus convolution, with limited eight-knot envelope and approximate law. Group scope still lacks native convolution. Runtime IR load/completion requests remain unbound.
- Modern Reverb `59`: all ten `room_type,time,size,damping,modulation,diffusion,predelay,high_cut,low_shelf,stereo` values are consumed by an **approximate bus translation**, so they are not “unconsumed.” Native algorithm/topology and normalized laws remain mismatched; group reverb is unmodeled. Merge measured laws and verify impulse responses rather than reread floats.
- Wrapper/rack/bus physical slot, generic scope, bypass/output/dry are partially consumed. Bus `name` and extension bytes are metadata; saved bus volume/output routing are used, saved bus pan is not implemented. RE uncommitted **FX reporting** converts malformed rack/bus/group/slot silent drops into located errors while preserving siblings/indices; it adds no new audible FX fields.

### Modulation and envelopes

Already consumed: target parameter names for the supported subset; intensity on accepted routes, target smoothing, invert, shaper enabled/table/breakpoint points; AHDSR attack curve/times/hold/sustain/release and AHD flag; Flex delta-time/level/curve points and sustain index; LFO sine/50%-square/triangle, a restricted single-wave Multi case, rate/free fade/phase and frequency note value; internal bypass and LFO retrigger. Remaining fields/semantics:

| Exact fields not consumed / incomplete | What it enables |
|---|---|
| Target `unknown_flags & 0x02`, unsupported target `param`, `name`, `slot` bindings; `unknown_i16` and other flag bits | Signed-depth correction; actual module/envelope/FX address routing. Other flag/word semantics remain unassigned. Pan, loopStart/loopLength, warpFactor/warpFactor2, wavetablePosition/Inharmonic/ModAmount/ModFrequency and non-cutoff module destinations are absent. |
| Internal routers-open byte / fourth unknown byte, `unknown_id`, category/name for lookup; retrigger for shared envelope lifecycle | Stable KSP mod lookup and native retrigger semantics; UI-open/unknown flags have no sound law. Category already selects reader framing and is not an entirely unread field. |
| External `unknown_source_data` (4 or 2 bytes), `unknown_id`, `unknown_tail` (v103 one byte/v104 two); source codes 7 ReleaseVelocity and 11 RandomBipolar | Release velocity/random bipolar execution; footer/source metadata require verified meaning. Pitch bend, poly/channel AT, CC, key, velocity, constant, release counter with T>0, script value and random-unipolar already lower. |
| AHDSR four trailing packed timing records: each `values[3]` and raw flag; retained extension bytes | Native per-stage tempo synchronization after unit/flag meaning is established. Extension bytes remain opaque. |
| Flex `unknown_index`, tail timing record `values[3]/flag`, v12 extra word | Native loop/sync behavior, presently unassigned. `last_point` bounds the reader and is not a missing DSP parameter. |
| LFO wave3/4 and type6 interpretation, general Multi `trailing_values[5]`; `records[0].values[1/2]`, `records[1].flag`, all `records[1].values[0..2]`, `trailing_flag`, v73 `additional_flag` | Saw/random/general Multi and synced fade/delay; frequency/fade sync unknown words need laws. `records[0].flag` is already used as normalization eligibility for the restricted Multi case, not universally ignored. Type5 can still differ in weighting/ripples; exact native waveform law requires validation. |

Curved shapers already influence evaluation; their native spline/curvature law is not exact merely because the point triples are decoded. Start-offset routing already exists at `sampler-core/src/voice_mod.rs:393`; it is incorrect to report all offsets missing. Need to test final script/native cursor timing and route combination, not just zone data. Native DSP-law branch verifies a geometric AHDSR floor and lifecycle vectors, but does not establish host control rate = sample rate/32 or MIDI note-off scheduling. Those must be measured before an “exact envelope” claim.

### Persistence, snapshot and legacy fields

`SavedEntry::parse/from_bytes` supplies `name,value,raw` plus `$` integer/UI state, `~` real, `%` integer array, `?` real array, `@` scalar text and `!` LF-terminated string cells. Typed **value/array-tail policy/raw offsets/bounds and `!` cells** are not consumed by integration's old parser. Existing scalar/numeric array values, spaces in text (split once), menu positions→values and ordinary repeated-tail fill already work. UI table/XY full-dimension handling must remain distinct from ordinary compressed tails. Widget declaration bounds/type context, early-restored entry consumption, snapshot `set_snapshot_type` policy 0/1/2/3, correct persistence callback identity, live recall and NKA file service remain open. Snapshot group/native FX state is partially applied; **snapshot group modulation-array replacement** is available on FX/mod branch but not current executable overlay. Empty replacement and absent-array inheritance must differ.

Legacy branch has no new secretly unconsumed semantic playback fields: its FileContainer TOC member index/name/cumulative end/data base are consumed **on that branch** for bounded preset selection and in-place sample streams; **not available on the audited importer**. AIFF/AIFC FORM/COMM/SSND sizes/rate/channels/frame counts/offset/PCM representation are consumed by branch decode/stream paths. AIFF MARK/INST loop metadata remains unmodeled, with Kontakt zone loops the preset authority. NKS v1 absolute compressed offset/expanded sizes and UTF-8 validation are used by extraction; legacy XML contents are **not translated to IR**, old NKS monoliths remain unsupported, and nested sample-bearing FileContainers lack a verified identity model. Installed census: no legacy XML/NKS monolith/FileContainer monolith/NKB/NKP or AIFF witnesses. Authored bounds/render tests prove the implementation path, not a real-library or v1 regression. V1 also rejects legacy monolith/XML translation.

## Ranked KSP audit reconciliation

Read full `KSP_AUDIT.md` on `v2/gpt-ksp-audit@a1446ec6`. At `7e82b152`, **all 40 failing opt-in probes still fail**. The three passing baselines are integer/polyphonic arithmetic, aliased-current-id `ignore_event`, and listener-generated notes without input. The 40 ranked findings below are source/audit findings, not a one-to-one mapping to 40 test functions; some lack installed usages or a complete reproducer. No audit item is declared fixed from a commit title alone.

Prior census counts below are **NKI paths including recovery**, often **2,741**, not the 834 manifest instruments/multis or unique families. Audio importance varies: display/label gaps are not sample-selection failures by themselves.

| ID | Still-open behavior; prior exposed NKI paths | Consumer / concrete next fix |
|---|---|---|
| 01 | 663/676 engine parameter identifiers lack real support; 2,741 | Core addressed engine service; parameter-specific setters/getters, **L**. |
| 02 | Engine display getter empty; 2,741 | Native unit formatting from same authored/live parameter binding, **M**. |
| 03 | Purge attenuates rather than residency; init requests ineffective; 2,738 | Streaming/residency service and init dispatch, **L**. |
| 04 | Host values placeholders; 2,738 | Production host tempo/transport/CC binding, **M**. |
| 05 | `find_mod` hashed fake; 2,737 | Stable actual source-slot/name lookup, return −1 on absence, **M**. |
| 06 | Init engine writes mostly isolated mirror; 2,625 | Shared init/live service, preserve existing five FX plus bus/route special cases, **L**. |
| 07 | `set_text`/label UI effects not consumed; 2,018 | Apply emitted UI mutations to live model/derived state, **M** (UI owner). |
| 08 | Release-counter reset no engine consumer; 1,923 | Reset group/note release-age state, **S/M**. |
| 09 | Current `EVENT_PAR_ALLOW_GROUP` only deferred; 1,552 | Apply current event's mask before selection, **M**. |
| 10 | Transport listener callbacks unbound; 717 | Host position/beat/tempo scheduling, **M**. |
| 11 | `change_vol/pan` mode2 treated relative; 467 | Native modes/units and saturation, **S/M**. |
| 12 | Persistence callback identity/init/live recall; 377 | Correct callback type and snapshot lifecycle service, **M/L**. |
| 13 | Menu visibility/mutation discarded; 373 | UI model and script getter shared state, **M**. |
| 14 | Callback ID aliases event ID; 373 | Separate invocation identity and scheduler map, **M**. |
| 15 | Event source Boolean/current-slot confusion; 369 | Creator slot on all generated event paths; current-slot lowering, **M**. |
| 16 | Init `$CURRENT_SCRIPT_SLOT` zero; 369 | Bind slot identity before init, **S**. |
| 17 | `%GROUPS_SELECTED` / `%GROUPS_AFFECTED` fabricated; 368 | Populate real source-group identities, **M**. |
| 18 | `stop_wait` not acted upon; 368 | Scheduler cancellation/resumption by callback ID, **M**. |
| 19 | IR async requests/completion absent; 354 | Resource loader plus async callback lifecycle, **L**. |
| 20 | `ticks_to_ms` uses fixed 1000/120; 349 | Host tempo/tick conversion, **S/M**. |
| 21 | NKA save (341)/load (107) not bound | Bounded authorized file/resource service, **M**. |
| 22 | Authored `get_engine_par` returns mirror zero; 237 | Reader-backed parameter service, **M/L**. |
| 23 | Poly-AT/RPN/virtual-CC callbacks unbound; 128 | MIDI ingress→script/controller dispatch, **M**. |
| 24 | Native group criteria incomplete; 4 | Composed keyswitch/CC/RR/random/slice selection, **L**. |
| 25 | `event_status` returns zero; 6 | Live/dead/queued event lookup, **S/M**. |
| 26 | Cross-slot init PGS sharing absent; 5 | Shared init environment; runtime PGS already exists, **M**. |
| 27 | String builtin getters empty; 4 | Runtime menu/control string reads, **S/M**. |
| 28 | Indexed control properties alias all cells; 3 | Per-index table/XY state and script array coherence, **M**. |
| 29 | Release velocity (1) / event play position (3) zero | Capture MIDI note-off velocity and actual source cursor, **M**. |
| 30 | Event-ID arrays not populated; 1 | Enumerate live IDs without truncating silently, **S/M**. |
| 31 | Marks/BY_MARKS (1) / ALL_EVENTS (6) ineffective | Event set expansion and mark lifecycle, **M**. |
| 32 | KSP timer reset no effect; 1 | Reset observable script clock, **S**. |
| 33 | Real sort uses integer bit operations; 1 | Numeric floating sort with finite policy, **S**. |
| 34 | String capacity 256 bytes vs 320 characters | Character-safe storage/limits, **M**; no library count established. |
| 35 | Sixteen custom event parameters absent | Per-event indexed user state, **M**; no installed count established. |
| 36 | MIDI-object commands do not compile; 0 observed | Implement only with real MIDI-object native fixture, **L**. |
| 37 | Global UI/new callback forms unsupported; 0 observed | Parser/model/host bindings on witnessed forms, **L**. |
| 38 | Real search wrong/accepted unsupported semantics; 0 observed | Numeric typed search and native edge cases, **S**. |
| 39 | Muted source groups compacted; candidate, no full repro | Stable source map and numeric/name equivalence probe, **M**. |
| 40 | Script modulation ID ceiling 12 | Extend address/state beyond12 with real fixture; existing clamping works, **M**. Do not count all 1,911 clamped-use paths as broken. |

Already integrated partial work does not close related failures: menu index/tail fixes (`88fdfd6b`) improve loading, while persistence lifecycle probe still fails; event identity changes do not pass source/current-event allow tests; passing aliased `ignore_event` does not validate marks/all-events. Read-only exposure scan and tests do not certify undocumented native semantics.

## Reproduction and phase-2 acceptance

Build/run only through `~/.cache/kontakto-heavy`, one local heavy job at a time. Separate calls release the slot between shards. This audit used per-worker 50/60-second item bounds and outer 290-second calls, with at most two workers; no shared corpus cache writes.

```sh
~/.cache/kontakto-heavy cargo build --profile corpus -p corpus-health
# BINARY = wrapper-selected corpus/corpus-health path; do not set CARGO_TARGET_DIR.
~/.cache/kontakto-heavy timeout 290 "$BINARY" run "$OWN_CACHE/quick-0.jsonl" --tier quick --only '*/Kontakt/*' --shard 0/16 --workers 2 --timeout 60
~/.cache/kontakto-heavy timeout 290 "$BINARY" run "$OWN_CACHE/quick-1.jsonl" --tier quick --only '*/Kontakt/*' --shard 1/16 --workers 2 --timeout 60
# Focused full runs: --tier full --workers 1 --timeout 50 and explicit instrument globs.
# In an isolated audit worktree, copy the known audit.rs from a1446ec6, then:
~/.cache/kontakto-heavy cargo test -p sampler-ksp --test audit -- --include-ignored
~/.cache/kontakto-heavy cargo test --no-run
```

Fresh KSP test outcome is expected failure, not successful validation of parity. Root `cargo test --no-run` **passed** (31.75 s), corpus binary built (24.91 s). No development server or native host was started. Numeric evidence and input SHA256s are checked into the sidecar; raw measurements remain in `~/.cache/kontakto-audit-kontakt/`. Existing reference audio was only inspected via already produced probe metadata; no library audio/scripts were written.

Still unknown, with a concrete measurement plan:

1. **Current native parity:** rerender the five exact reference MIDI grids at 48 kHz against current integration and v1, explicit CC1/7/10/11/64, same preset/snapshot/load state and calibration. Report every note's matched source, articulation, velocity layer, final offset in frames, direction and gain; include unidentified/silent separately. Areia first.
2. **Full-corpus v1 regression list:** consume the census agent’s paired `kontra-scan` TSVs first, rather than building a collector or duplicating its corpus runs. Use the shared binaries for targeted reproduction under their documented MIDI/controllers and slot/shard rules. Ask the census owner through the coordinator if per-program NKM or valid-note identity columns are missing. Additional fidelity checks remain necessary: loading/one audible note does not prove native playback. Do not invent absent library names from compatibility claims.
3. **RR/keyswitch correctness:** retain parent MIDI event/take identity; separate attack, generated continuation and release selections. Reset/randomize state explicitly; compare native sequence when deterministic and distributions when random. Test source-index holes, every authored KS/CC/native criterion combination and saved default. The heuristic flags alone are not acceptance criteria.
4. **Offsets/direction/loops:** identify final cursor at first audible frame and after every jump; test reverse fixtures, negative relative sample end, script `play_note` offsets, start-range modulation, counted/ping-pong/crossfade/multi-loop transitions. Require exact offsets where deterministic, not 50 ms detector tolerance.
5. **Envelope/release:** isolated attack/hold/decay/sustain/release and curved Flex/sync tests, then pedal/sostenuto/retrigger/release-velocity and release-counter reset. Observe at least the authored tail budget and compare native voice termination/audio; fixed-window retained voices alone do not prove a leak. Host cadence/reset/note-off laws remain outside current native instruction vectors.
6. **Resource/persistence/source modes:** exercise linked-only/embedded+linked precedence and each of 1,103 snapshots with verified parent; script-only/native-state policies0–3, early reads, string arrays, UI table/XY and NKA. Isolate Conflux Wavetable from String Ensemble; measure every mode's native source controls. Preserve unknown fields without guesses.

Acceptance is **the right performance**, not “parsed”, “finite”, “has a nonzero peak”, “all source records retained” or “no KSP fault.” The reader branches reduce missing knowledge; the remaining translation/service/DSP work must consume it and pass native-host comparisons.

## Addendum 2: original UI default (outside playback scope)

The user specifies that v1 rendered most original library UIs correctly and that v2 must default to **Original**, with vector-by-default classified **P0**. This is an authoritative product requirement, not a new measured UI result in this playback audit. The Kontakt UI audit owner should verify and fix default/selection persistence; shared scan `ui` results establish coverage separately from `plays_note`. This audit does not silently treat a semi-vectorized fallback as original-UI success.
