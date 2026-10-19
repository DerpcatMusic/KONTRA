# One representative patch per installed library

Scope: one selected instrument/program from each installed Kontakt and UVI library. A fresh production V2Loader preparation is followed by four seconds of bounded plan activation, CC1=100/CC11=127, one source-covered note at velocity 100, one second held and one second released. PCM is measured and discarded. This is an offline production-engine exercise; it does not establish native Kontakt/Falcon equivalence, DAW behavior, UI gestures, or whole-library compatibility.

Each patch runs in an isolated process. Structured load and runtime diagnostics use the same actionable report pipeline as the product. UVI program identities are enumerated from current bank metadata. Script array-file reads are serviced read-only and MIDI-save requests complete as failed; the probe never writes library state. This diagnostic-host policy must be distinguished from an engine omission. A failed probe or operational timeout is incomplete evidence, not a library incompatibility verdict.

Private source-bound artifacts and the explicit selection are retained under `/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-one-per-library-20261019/`. Full scripts, artwork and PCM are not exported. Build/source identity, compiler profile and executable hashes must be retained with comparisons.

## Before-change contracts

| Entry point / formats | Thread and ownership | Bounds and recovery | Distinguishing evidence |
|---|---|---|---|
| `offline_library_diagnostics` / diagnostic helper | Runs on a 32 MiB worker; owns prepared part, stream, source metadata and diagnostics; retires all state off audio | One patch/process, fixed activation/note/release frame budgets, external 180-second process timeout | Actual production preparation, finite audio measurements and located runtime faults |
| Effect drain / helper only | Between render calls; runtime owns job admission, helper owns temporary completion payloads | Existing bounded outbox and job storage; saves refuse without modifying source files | Read-only completion and explicit diagnostic-host scope |
| Fault capture / helper only | Reads retained continuations before completion retirement, then formats off audio | Existing fixed-size fault snapshots and bounded report history | Reports retain source slot/line, opcode and operand evidence |

Priority is playback/load impact followed by the number of distinct libraries affected, not repeated warning count.

## Fresh baseline: 16 libraries

All 16 selected patches completed the probe. Eleven produced finite, nonzero audio; Shreddage produced zero for the selected key; four UVI programs failed preparation. No KSP runtime fault occurred in these note exercises. This does not reproduce Halpern Drums callback #6 (that preset is unavailable), certify every articulation, or establish that Shreddage is silent on every note.

The exact selection, current UVI catalogs, isolated reports and executable SHA-256 are in the private artifact directory. Cases 01–04 used the unoptimized corrected probe and cases 05–16 the optimized `ci` probe, each retained under an immutable filename. Timing comparisons across those profiles are not meaningful. Baseline report build identities include the uncommitted probe source; the retained executable hashes, `provenance.json`, and the committed probe implementation identify the executed artifacts.

| Library | Representative | Baseline | Conversion records |
|---|---|---|---:|
| ANALOG STRINGS | ANALOG STRINGS.nki | finite audio | 69 |
| Afflatus Chapter II Brass | 2 Horns KS.nki | finite audio | 1 |
| Areia 1.2.0 [Audio Imperia] | 01 Areia - 16 Violins - Core Techniques.nki | finite audio | 9 |
| Audio Imperia CHORUS | Chorus - 04 Men - Sustained Ah.nki | finite audio | 957 |
| Audio Imperia Dolce | Dolce - 02 7 1st Violins - Sustained.nki | finite audio | 44 |
| Conflux 1.1.0 [Native Instruments] | Conflux.nki | finite audio | 2665 |
| Morphology Evolved | Morphology Evolved.nki | finite audio | 1059 |
| Pacific Ensemble Strings | Pacific - Ens Strings - 16 Violins - Legato Sustains.nki | finite audio | 8 |
| Performance Samples Vista | Vista - 5 Violins.nki | finite audio | 10 |
| Shreddage 3.5 E-Upright 1.0.1 | Shreddage 3.5 E-Upright.nki | zero audio on selected key | 28 |
| Solo | Solo - 01 Solo Violin.nki | finite audio | 2 |
| Una Corda Library | Una Corda Felt.nki | finite audio | 87 |
| UVI - Augmented Orchestra v1.1.2-R2R | V Strings Sustain Octave.uvip | load failed | — |
| VWinds - Clarinets | Clarinet A.uvip | load failed | — |
| VWinds - Double Reeds | Oboe.uvip | load failed | — |
| VWinds - Flutes | Alto Flute.uvip | load failed | — |

## Ranked implementation backlog

1. **UVI shared script-event output modulation (three libraries; load blocker).** Augmented Orchestra's four layers have Gain crossfades driven by `ScriptEventModulation` EventId 1–8. Oboe and Alto Flute have Program Gain driven by EventId 40. These need the actual shared source lifecycle, script-event input actor, update/reset ownership and prepublication installation. The current bus evaluator accepts controller/constant inputs only, while the ordinary loader also deliberately refuses controller inputs until their actor is installed. Replacing these sources with per-note values or removing a refusal would be incorrect. Retained `.connections.txt` files contain scoped metadata only, not XML/scripts. Improved refusal messages now identify the source family, EventId, polarity, destination and scope.
2. **UVI native Frequency connection attachment and typed parameter cadence (Clarinet; load blocker).** The loader refuses native processing-parameter recipes whose execution cadence/attachment has not been qualified. Needs the native law and real owner binding; a historical Core-law stand-in does not satisfy this contract.
3. **Shreddage zero-note result and omitted Console script.** The candidate report locates Console slot 1, line 808: `get_ui_id($last_id_anchor)` is rejected because the declared anchor is an ordinary integer (`get_ui_id requires a UI control`). This needs native behavior evidence for the anchor idiom before relaxing the existing typed-operand contract. Determine independently whether this compile failure causes the audition's silence. Static keyswitch extraction being unsupported is a separate diagnostic and is not proof that the runtime note callback failed. The report previously hid the compiler message and incorrectly said only “plays without it”; that reporting gap is fixed in this branch.
4. **Conflux wavetable route ownership (implemented first playback fix).** Position and warp destinations already have an admitted DSP path, but group translation inspected them before publishing the source group and physical-to-runtime map. Publish that owner before translating routes and wire normalized live depth/bypass controls for admitted wavetable targets. Unsupported wavetable FM/inharmonic targets remain reported.
5. **AHDSR attack destination modulation (Pacific, Vista, Una Corda).** Resolve physical envelope slots and implement the native envelope parameter combination/cadence. Existing time codecs alone do not implement the incoming modulation law.
6. **Tape Saturator live controls (Morphology, Una Corda), Solid G-EQ live controls/smoothing (Una Corda), and Jump amplifier (Analog Strings).** Retain saved sound while adding the measured runtime knob laws; live controls and native effect DSP are separate requirements.
7. **Source identity and remaining waveform/filter laws.** Native zone identities remain unavailable in five selected libraries; this blocks source-addressed features, while ordinary sample playback works. Ladder LP4, Daft HP, Legacy LP1, legacy reverb and convolution field warnings need qualification at their reported parameters. Narrowed pitch ranges and script warnings must be investigated at their sites, not counted as proof of missing DSP.

## Per-library reported gaps

These are the distinct retained diagnostic examples from the baseline, including compile warnings and admitted approximations. Repetition counts above are conversion records, not separate features. A runtime warning may reflect an authored script issue or the probe's limited host context.

### ANALOG STRINGS

- Not translated: native source zone identity (not modelled yet): 95624 identities unavailable; no ordinal alias was created; 95624 zones: version=0x98 private_bytes=83 private_u16_at_56=Some(0)
- Not translated: Filter: filter law (scaling or curve unverified): Daft HP (Partial): saved controls and ramp scheduling extracted; native audio core and resampler not recovered, saturation and interpolation are stand-ins (CORE-FILTERS-029..031)
- Not translated: LFO waveform (id, multi weights, pulse width) (scaling or curve unverified)
- Not translated: modulation of a module parameter (not modelled yet): startPhase (<none>) destination slot 4
- Not translated: Convolution: IR size (not modelled yet)
- Not translated: Convolution: IR early/late crossover (not modelled yet)
- Not translated: effect (not modelled yet): Jump v0x10 len 26: amp, cabinet or pedal model: cores and tables are not extracted, so no topology is proved
- Not translated: Reverb (legacy): legacy reverb input lowpass (scaling or curve unverified)
- KSP warning: SignatureNum read in on init takes the host transport the loader supplied
- KSP warning: SignatureDenom read in on init takes the host transport the loader supplied
- KSP warning: DurationQuarter read in on init takes the host transport the loader supplied
- KSP warning: later ui_control callback replaces an earlier one
- Attack preload budget exceeded: resident PCM requires at least 7794695022 bytes, exceeding configured payload budget 268435456 bytes. Resident start ranges: 7770 of 25063. Base attacks: 39420 of 39420. Constant onset windows: 12355 of 12355, covering 654 output frames. Uncached starts may wait for disk reads.

### Afflatus Chapter II Brass

- KSP warning: polyphonic variables start at N; initializer ignored

### Areia 1.2.0 [Audio Imperia]

- Not translated: native source zone identity (not modelled yet): 13464 identities unavailable; no ordinal alias was created; 13446 zones: version=0x9a private_bytes=140 private_u16_at_56=Some(1); 18 zones: version=0x9a private_bytes=152 private_u16_at_56=Some(1)
- KSP warning: find_group: group not found; -1
- KSP warning: polyphonic variable read outside a note callback is N
- Attack preload budget exceeded: resident PCM requires at least 639789144 bytes, exceeding configured payload budget 268435456 bytes. Resident start ranges: 0 of 24576. Base attacks: 24576 of 24576. Uncached starts may wait for disk reads.

### Audio Imperia CHORUS

- Not translated: keys beyond the runtime's pitch range, narrowed (value cannot be represented)
- KSP warning: polyphonic variable read outside a note callback is N
- Attack preload budget exceeded: resident PCM requires at least 696464524 bytes, exceeding configured payload budget 268435456 bytes. Resident start ranges: 0 of 26306. Base attacks: 26306 of 26306. Uncached starts may wait for disk reads.

### Audio Imperia Dolce

- KSP warning: array index 72 out of bounds for %_legato_event_id (length 72); read returns zero or empty text
- KSP warning: find_group: group not found; -1
- KSP warning: polyphonic variable read outside a note callback is N

### Conflux 1.1.0 [Native Instruments]

- Not translated: Filter: filter law (scaling or curve unverified): Ladder LP4 (Partial): core FUN_140b00be2/FUN_140b06180 checked (cutoff law and table, pole polynomial, resonance, feedback correction, clip, stages, tap weights and compensation, 2 ms ramp); the gain  [truncated]
- Not translated: modulation target (not modelled yet): wavetablePosition (MOD ENV -> WT POS)
- Not translated: modulation target (not modelled yet): warpFactor (MOD ENV -> WT FORM 1)
- Not translated: modulation target (not modelled yet): warpFactor2 (MOD ENV -> WT FORM 2)
- Not translated: modulation target (not modelled yet): wavetableModFrequency (MOD ENV -> WT MOD FREQ)
- Not translated: modulation target (not modelled yet): wavetableModAmount (MOD ENV -> WT MOD AMT)
- Not translated: modulation target (not modelled yet): wavetableInharmonic (MOD ENV -> WT INHARMONIC)
- Not translated: external modulation from RandomBipolar (scaling or curve unverified): "RANDOM -> PITCH" -> ["pitch"]
- Not translated: modulation of a module parameter (not modelled yet): frequency (MOD ENV -> LFO 1 RATE) destination slot 2
- Not translated: modulation of a modulator's intensity (not modelled yet): route intensity -0.00006400010170182213
- Not translated: keys beyond the runtime's pitch range, narrowed (value cannot be represented)
- KSP warning: $name is not in the performance view description; retained as an unbound handle

### Morphology Evolved

- Not translated: Filter: filter law (scaling or curve unverified): Ladder LP4 (Partial): core FUN_140b00be2/FUN_140b06180 checked (cutoff law and table, pole polynomial, resonance, feedback correction, clip, stages, tap weights and compensation, 2 ms ramp); the gain  [truncated]
- Not translated: Tape Saturator: Creative FX live controls (not modelled yet)
- Not translated: wavetable source (not modelled yet)
- Not translated: keys beyond the runtime's pitch range, narrowed (value cannot be represented)
- KSP warning: $name clamped to its declared range
- Not translated: script interface: $CONTROL_PAR_AUTOMATION_ID[] (not modelled yet)

### Pacific Ensemble Strings

- Not translated: modulation of a module parameter (not modelled yet): ahdsr_attack (<none>) destination slot 0

### Performance Samples Vista

- Not translated: native source zone identity (not modelled yet): 12528 identities unavailable; no ordinal alias was created; 12528 zones: version=0x99 private_bytes=87 private_u16_at_56=Some(0)
- Not translated: modulation of a module parameter (not modelled yet): ahdsr_attack (<none>) destination slot 0
- KSP warning: write to %name lowered as set_controller

### Shreddage 3.5 E-Upright 1.0.1

- Not translated: time machine pro Elastique backend (stand-in granular engine) (not modelled yet)
- Not translated: keyswitch script (not modelled yet)
- KSP warning: polyphonic variables start at N; initializer ignored
- KSP warning: control parameter set on unknown UI id N
- KSP warning: $name is not in the performance view description; retained as an unbound handle
- Not translated: script (not modelled yet)

### Solo

- Not translated: native source zone identity (not modelled yet): 7980 identities unavailable; no ordinal alias was created; 7980 zones: version=0x9a private_bytes=140 private_u16_at_56=Some(1)
- Not translated: keyswitch script (not modelled yet)
- Attack preload budget exceeded: resident PCM requires at least 312757228 bytes, exceeding configured payload budget 268435456 bytes. Resident start ranges: 0 of 11858. Base attacks: 11858 of 11858. Constant onset windows: 5194 of 5194, covering 625 output frames. Uncached starts may wait for disk reads.

### Una Corda Library

- Not translated: native source zone identity (not modelled yet): 4402 identities unavailable; no ordinal alias was created; 4402 zones: version=0x98 private_bytes=83 private_u16_at_56=Some(0)
- Not translated: Solid G-EQ: live control (not modelled yet)
- Not translated: Solid G-EQ: Solid G-EQ smoothing (scaling or curve unverified)
- Not translated: Filter: filter law (scaling or curve unverified): Legacy LP1 (Partial): legacy cutoff coefficient saturation and raw slew state verified in native setters/wrapper; transition boundary phase for out-of-range saved targets is not yet calibrated
- Not translated: modulation of a module parameter (not modelled yet): ahdsr_attack () destination slot 1
- Not translated: Tape Saturator: Creative FX live controls (not modelled yet)
- KSP warning: $name clamped to its declared range
- KSP warning: menu item 45 out of range for UI id 32847 (item count 45); set_menu_item_visibility ignored
- KSP warning: array index -1 out of bounds for !mnuRoomAbr (length 30); read returns zero or empty text
- KSP warning: DurationSixteenth read in on init takes the host transport the loader supplied

### UVI - Augmented Orchestra v1.1.2-R2R

- load failed: [Translate at crates/sampler-uvi/src/lib.rs:1640] Presets/00 Orchestra/01 Strings/V Strings Sustain Octave.uvip: Program "Program"/Layer "Layer 0": live global Gain/Pan requires the actual shared owner modulator; no per-source fallback

### VWinds - Clarinets

- native Frequency connection attachment and typed parameter cadence are unqualified; historical Core law cannot activate the native profile

### VWinds - Double Reeds

- load failed: [Translate at crates/sampler-uvi/src/lib.rs:1640] Presets/Oboe.uvip: Program "Program": live global Gain/Pan requires the actual shared owner modulator; no per-source fallback

### VWinds - Flutes

- load failed: [Translate at crates/sampler-uvi/src/lib.rs:1640] Presets/Alto Flute.uvip: Program "Program": live global Gain/Pan requires the actual shared owner modulator; no per-source fallback

## Implementation contracts and validation

| Change | Thread / lifetime | Bounds / recovery | Falsifying check |
|---|---|---|---|
| Publish Kontakt group before modulation | Loader worker, temporary translation owns IR; failed translation is discarded | One real group and map entry per source group; sparse indices retain holes | Synthetic serialized group through full translator; supported routes survive, unadmitted forms still refuse |
| Wavetable depth/bypass lanes | Built on loader; existing preallocated control/modulation lanes execute on render | Reuses normalized route depth and one bypass per modulator; no new audio allocation | Saved-bypassed LFO retains all three routes, live controls and real group identity |
| Compile-failure detail and UVI refusal detail | Typed faults retained on loader; report formatting off audio | Existing bounded report and source excerpt limits | Slot/line/cause/callback impact and Error severity survive report construction; unsupported shared source still refuses |

The candidate executable is `probe-ci-v2`, SHA-256 `30ccdfff737b4637b7dbbbdacec5447886d031b9842141a0d88686445f996d21`, built from clean commit `83904ae7d8f9939a5fd786e3e20458ec2e9334e4`, `ci` profile, Linux x86_64, default plugin features plus `shots`. Exact build/import identities, lock hash and receipt hashes are retained in `candidate-manifest.json`.

- All 16 production probes were repeated. The same 11 patches produced finite nonzero audio; Shreddage's selected key remained zero; the same four UVI programs refused preparation. All 12 loaded patches had zero nonfinite samples and zero captured KSP runtime faults. Every loaded patch's measured peak was unchanged; PCM was not retained or compared.
- Conflux's conversion records fell from **2665 to 2646**. All **19** rejected `wavetablePosition`, `warpFactor`, and `warpFactor2` routes disappeared from the diagnostics. The three unsupported FM/inharmonic destination families remain reported. This verifies admission and exercised-note stability, not native synthesis equivalence or every modulator setting.
- Kontakt unit tests: **291 passed, 9 corpus tests ignored**. The new synthetic serialized group test exercises a sparse physical source map, an existing sampler sibling, a saved-bypassed LFO, three live routes and unadmitted form refusal.
- UVI located-refusal test: **1 passed**. It verifies that the unsupported shared actor still refuses and reports EventId, polarity, destination and actual output owner.
- Product diagnostics tests: **38 passed**, including located compile error cause, bounded source excerpt, unavailable-callback impact and Error severity.
- Rust Doctor baseline scan against `a022069e8`: **passed, zero errors**; 12861 warnings and 85 informational findings across its workspace scope. Four introduced production findings are nonblocking: three checked-by-construction indexing sites and increased complexity in report formatting. Ten additional orphan-file deltas concern ignored historical native-analysis artifacts copied with the worktree caches, not source changes in this branch. Raw receipt retained; this is a static gate, not audio deadline qualification.

Remaining top blockers are explicitly open: shared UVI script-event ownership, native Frequency attachment/cadence, and Shreddage's anchor operand/zero-note investigation. No guard was removed to make their load results appear successful.
