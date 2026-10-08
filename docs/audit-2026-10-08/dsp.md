# DSP audit — 2026-10-08

Audited v2 baseline **7e82b152** (`origin/integrate/core-v2` when this worktree was created); pinned v1 **0cb7a8a0**. Audit branch `audit/dsp-20261008`. This report changes no production DSP.

## Verdict

**Kontakt DSP coverage is worse than v1; native fidelity is mixed and remains incomplete. UVI coverage is better than pinned v1, which has no UVI engine, but v2 is not a faithful Workstation/Falcon replacement.** Across the manifest's 781 NKIs and 53 NKMs, v2 omits 5,681 enabled filter slots in **77 containers (27 NKIs + 50 NKMs)** and 723 enabled non-filter FX slots in **60 containers (10 NKIs + 50 NKMs)**. These unions overlap and must not be added as distinct instruments. The missing filter figure excludes approximated EQ, SV and Daft; missing FX excludes partially supported send, convolution, modern reverb, stereo, gain, inverter and compressor families. Scope and modulation restrictions cause additional omissions within those supported families.

The primary envelope *shape* is substantially correct: this audit's real-runtime unity-signal probe compares **45 cases / 3,150 outputs** to Kontakt 8.13.1 original-byte AHDSR lifecycle vectors, maximum absolute error **7.1811e-7**, worst case RMS error **1.7913e-7**, maximum level error **0.0000266 dB** where both amplitudes exceed -60 dB. This controls for equal tick durations; it **does not certify the host control clock**, tempo sync or ordinary library playback. V1 retains Kontakt's 32-frame clock, floor state and interpolation explicitly; v2 renders a mathematical envelope at the audio sample clock.

Fresh scripts-on comparisons to saved Kontakt captures (48 windows each) give mean/worst absolute steady level errors: **Una Pure +0.31 /0.80 dB**, **Vista3 Cellos +1.27 /4.07 dB**, **Analog Strings +14.48 /15.47 dB**. These are whole-instrument differences; sample selection and script state are not isolated. Current whole-instrument and isolated host-reference results are recorded below. Good static gain calibration does not establish correct missing filters, random modulation, DSP ordering or reverb tails.

## Evidence and counting rules

- The complete brief, local heavy-job rules and RE addendum were read. All measurements/builds use `~/.cache/kontakto-heavy`; no native host or shared reference rig was launched, and no decrypted XML, script or sample payload was written.
- Manifest: `/home/derpcat/.cache/kontakto-corpus/items.tsv`, **1,494 items**: 781 Kontakt NKIs, 53 Kontakt NKMs, 660 UVI programs (620 Augmented Orchestra, 40 VWinds). A Kontakt **container** is one manifest item. NKM variants heavily repeat DSP: Ladder LP4 is 2 NKIs + 50 NKMs, type6 LFO/random bipolar 1 NKI + 50 NKMs, and SV LP2 5 NKIs + 45 NKMs. Use the NKI column for single-instrument ranking, not the container total as a unique-product count. NKM membership produces **890 total programs** across 834 containers; module/container counts are unions across that item's programs, not 834 unique sounds. Counts exclude muted Kontakt groups and bypassed slots; separate saved bypass counts remain in the TSV. Script selection, live bypass changes, muted ancestors and macro defaults can alter actual audible use.
- Kontakt counts reuse the completed, metadata-only `gpt-decipher-dsp` saved census: **834/834** per-item caches, zero recorded parse failures, source `crates/sampler-kontakt/examples/dsp_census.rs` on **aa3430d6**. This is the lower-reader saved census, not the incomplete 453-item IR survey. Baseline silent slot decode losses can still undercount malformed records; zero file failures does not prove every FX decoded.
- UVI counts come from this branch's `audit_dsp_census` example, with per-item metadata cache and a 240-second shard ceiling. Direct `Inserts` children and `ControlSignalSources` children are counted; enabled means their own saved `Bypass` is absent/zero/false. These are saved module occurrences, including structural racks and unused sources, not a claim that each participates in playback. Translation status is established by tracing the current translator; this metadata census deliberately avoids full instrument translation.
- Reproduction: `cargo run -p sampler-core --example audit_dsp_envelope`; `cargo run -p sampler-uvi --example audit_dsp_census -- ITEMS CACHE START END`; then `python3 docs/audit-2026-10-08/dsp_metrics.py`. Use the heavy wrapper for cargo and library probes. Cache paths are documented in that script. Committed [Kontakt counts](data/dsp-kontakt-usage.tsv), [UVI counts](data/dsp-uvi-usage.tsv), and [envelope errors](data/dsp-envelope-comparison.json) contain aggregates only.
- Read-only primary RE sources: `t3code-80fe786b/docs/DSP_FORMAT_SPECIFICATION.md`, `DSP_SYSTEM_INVENTORY.md`, and `artifacts/engine-analysis-2026-10-07/{README.md,catalog.json,verification.json,deep-dsp,uvi-workstation}`. The inventory maps 59 filters, 65 FX and 167 UVI API module types. Original-byte tests validate specific kernels with stated helper substitutions; the 264,775 decompiled entries are **not an audio-equivalence proof**. Falcon executable was absent. Workstation 4.0.9 standalone and installed plugin are distinct reference images.
- Prior research: `v2/gpt-decipher-dsp` laws/vectors, `v2/gpt-format-fxmod` serialized payload map, `v2/kontakt-reference` native captures, and RE `NI_FILE_FX_DECODE_REPORTING.md` / `ALTERNATING_LOOPS.md`. The existing `DSP_COVERAGE.md` and old Falcon coverage document contain stale counts/statuses; they are not used as the census authority here.
- Shared scanner: `~/.cache/kontra-scan/bin/README.md` was read; pinned scanner bases match this audit. Its load/UI/one-note/RSS fields belong to the census report and do not measure native DSP equivalence or enumerate module usage. Published full-corpus `results/{v1,v2}.tsv` were not yet present at final inspection; the partial v2 cache is not used as a full result. No separate load/performance scanner was built here. DSP metadata counts and three targeted reference renders supply this report's measurements.

## Ranking by corpus use and audible error

A numerical corpus×error score is meaningful only with a measured error for the same module/state. Missing processors have **unknown dB error**, not zero error; it depends on their settings, source spectrum and routing. Consequently, the implementation ranking below uses affected-container union and confirmed loss of processing, with measured bounds where available. It does not fabricate a dB score for an unrendered effect.

For measured cases, use `U × E`, where U is active corpus containers and E is the reference case's maximum dB error. These are **exposure bounds**, not average corpus error: SV LP2 resonance passband loss gives **5 NKIs × up to 6 ≈ 30 NKI·dB** (or 50 manifest containers × 6 ≈ 300 container·dB); AHDSR equal-tick shape gives **781 NKIs × 0.0000266 ≈ 0.0208 NKI·dB** (834 containers ≈ 0.0222 container·dB); Classic compressor isolated on/bypass residual **1 × approximately 0.3–0.5 = 0.3–0.5 container·dB**. The fresh whole-Analog result is **1 NKI ×15.47 ≈15.47 NKI·dB**, but is **not** a per-DSP-module error and is kept separate. SV residual at resonance=1 is not incurred by every preset. EQ, convolution, modern reverb, most LFOs and omitted modules lack paired per-module error measurements and cannot honestly be ordered on this same scalar. A bypass-null sweep described below will supply the missing E values.

### 1. P0 — UVI FX coverage/scope and live writes are incomplete (L)

**Exposure:** all **660 programs** contain at least one unsupported saved FX family; **598 programs / 185,211 connections** modulate OnePole Freq, and all 40 VWinds programs contain GainMatrix destinations including inputs beyond stereo. These are saved-enabled exposure counts, not measured native default audible error. Counts/statuses are in the UVI table below. `sampler-uvi/src/lib.rs:398` imports only Program Gain, `:476` walks Layers without translating Layer inserts; `inserts.rs:43` handles Keygroup/Aux inserts. Unsupported program/layer connections are dropped (`lib.rs:384`). Effects such as XpanderFilter, SparkVerb, Drive, DualDelay and TrackDelay are not translated. `inserts.rs:1` states later script parameter writes do not reach these processors; `scripted.rs:363` handles a restricted Program/Layer parameter set. This leaves macro/effect controls audibly inert even when a static chain exists.

**Fix:** translate scope-correct Program/Layer chains, use the existing InsertNode processor-address bookkeeping for script writes and modulation targets, then implement the highest-use UVI modules. Port Workstation byte-verified kernels only where their public/saved IDs and state initialization are confirmed. The original-byte OnePole/rectifier/Bitcrusher/LFO checks certify limited internal kernels, not the complete saved-node/host binding or Falcon.

### 2. P0 — Amplifier split and ordered send taps are not preserved (M/L)

**Exposure:** Send Levels: **810 containers / 819 slots**; exact affected subset is not yet measured. Root cause: group racks are attached to per-voice chains, all before amplitude (`crates/sampler-kontakt/src/library.rs:629`); saved `amp_insert_position` is not used. Group Send Levels are only translated in bus scope (`effects.rs:650`). Instrument sends are all emitted `SendPosition::PostChain` (`effects.rs:1176`), losing the slot's tap position. The ScopeBus-only compressor admission (`effects.rs:704`) also excludes valid per-voice group compression. The comment there incorrectly describes native group compression as processing a group sum.

Kontakt group inserts, including compressors and distortion, process **each voice separately**. Bus/instrument inserts process summed audio; send returns sum their explicitly tapped inputs. The group chain also has a pre/post amplifier split. [Native Instruments signal-flow manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/using-filters-and-effects-in-classic-view). Cached source: `t3code-80fe786b/artifacts/engine-analysis-2026-10-07/kontakt-manual.txt:9089–9120`, `:9168–9207`.

**Fix:** retain per-voice group chains, honor the amplitude split/send slot positions in `sampler-kontakt/library.rs`, `effects.rs`, IR/lowering and core chain routing, and admit supported group dynamics per voice. Use existing pre/post amplitude and send positioning support. Compare two simultaneous notes through a group compressor against the same compressor on a bus, and sends before/after amplitude and a nonlinear insert. V1's `src/engine/filter.rs:2062` checks ordered per-voice group send taps; `src/fx/processor.rs:394`, `:777` mixes those feeds into send returns. Port these routing behaviors before adding more effect kernels.

### 3. P1 — Common convolution/reverb is present but not native (L)

**Exposure:** Convolution **415 containers / 477 slots**; modern Reverb **373 / 376**. `effects.rs:858` approximates IR volume-envelope shaping and auto normalization, and explicitly leaves early/late size/filter/decimation unmodelled. Crossover, preserve-length and latency-compensation semantics are not faithfully represented. The importer excludes voice/group convolution; whether any saved supported native preset needs that scope remains unmeasured. `effects.rs:966` maps modern reverb to an **8-line generic FDN**, with native-measured RT60/predelay but guessed room/damping/modulation/diffusion; `sampler-core/src/dsp/reverb.rs` uses its own delay network. RT60 match alone does not match reflections, spectrum or modulation.

**Fix:** retain every serialized IR setting, port verified shaping/filter/latency laws into the IR loader, and compare impulse/null/tail spectra against the host for normalized/reversed/early-late variations. Reconstruct the native reverb topology rather than tuning the generic FDN indefinitely. V1 also used approximated reverb/convolution; no measured claim that its convolution sounds better is supported. Generic FIR convolution tests validate the FFT engine, not Kontakt's IR preparation.

### 4. P1 — Stereo/Gainer automation skips native smoothing and pseudo stereo (M)

**Exposure:** Stereo Modeller **363 / 21,619**, Gainer **194 / 194**, Inverter **30 / 1,787**. Static matrix/gain calibration is mostly supported; dynamic fidelity is not. `effects.rs:419`, `:774` fold modules into matrices/Mix wrappers. The core Gainer processor exists, but the Kontakt importer does not emit it. Native byte vectors establish width recurrence `1/180` and pan/Gainer `1/1800` per sample (with float32/SIMD details); pseudo stereo delays R by `min(1023,trunc(Fs*0.01*w³))`. Static matrices cannot produce this delay or transition history. Core generic Gainer uses an approximate 45-ms time constant rather than the exact byte recurrence. The host capture measured roughly43–49 ms; reconcile the wrapper/context discrepancy before treating the isolated byte constant as complete host timing.

**Fix:** emit dedicated stateful Stereo Modeller/Gainer processors in `effects.rs` and core DSP, retain mode, implement native recurrence/order and pseudo delay. Use current settled-matrix/output-gain behavior as the regression baseline. The measured settled Stereo matrix residual is about **-119 dB**; that verifies a static case only. Inverter Output is a **plain dB gain**, as reference section 27 measured; old coverage wording claiming it does not reach the signal is wrong. V1 also folded static stereo/gain; this is not a proven v1 advantage.

### 5. P1 — Legacy EQ uses an unverified generic coefficient law (M)

**Exposure:** EQ IDs 22/23/24 union **223 containers / 11,480 enabled slots**. `effects.rs:1230` builds RBJ peaking biquads from Hz/octaves/dB, explicitly flags `UnknownLaw`. Native original-byte coefficient/sample/control-cadence vectors on `v2/gpt-decipher-dsp` establish a different approximation and float32 update order. Potential gain-normalization differences require checking the wrapper binding before claiming a doubled audible boost. Static gain fixes do not resolve band shape or live updates.

**Fix:** adapt the native coefficients and 32/4-frame update masks from the prior vectors into core/Kontakt translation, then run white-noise/impulse sweeps and live frequency/gain writes across sample rates. Include saved zero output special-case (`effects.rs:640`) in the authored host comparison. V1's broader EQ/Solid-GEQ implementation is useful code to port; generic v1 EQ should not be described as native without those checks.

### 6. P0 — Analog Strings has a measured level mismatch; gain-stage attribution remains open (M)

**Exposure/measurement:** one NKI, 48 MIDI-grid note windows against the existing native capture; fresh baseline mean **+14.48 dB**, worst absolute **15.47 dB**, total stereo energy **+12.27 dB**. The cached older v2 audio under the **same metric** is +5.48 dB mean /6.47 dB worst, almost exactly9 dB lower; Una and Vista cached/fresh metrics are identical. The historical +4.10-dB report used a different analysis. Neither the cached binary nor its compressor/bypass state is pinned, so this is a **confirmed current mismatch**, not proof of a recently introduced regression.

**Root attribution:** unresolved between signal/routing state, sample/script behavior and gain laws. The nearly constant9-dB difference warrants isolating the compressor's +9-dB output first. Baseline includes init-time modulation intensity and bus-volume/routing handling absent from the older reference branch (`library.rs:308`, `:751`, `:859`; `effects.rs:1091`). The documented positive-only `MOD_TARGET_INTENSITY` clamp is not itself proof of a fault. A whole-instrument result cannot be assigned to the compressor, EQ or missing filters individually.

The KSP audit owner checked both questions on 2026-10-08: the new harvesting/routing work came from WIP checkpoint `a511eade`, with unverified bus applicability of `18·log₂(v)−346.768234` (+12 dB at 1,000,000). The dynamic bus path sets outer gain to unity and does not visibly apply that fader twice. Compressor enablement `e9e97bef` is already an ancestor of both the owner's older `699758ab` base and this baseline. No recorded old/fresh effective-state dump establishes the +9-dB cause. Trace compressor bypass/output, harvested writes and final group-to-bus routing rather than treating either suspect as a proven regression.

**Concrete next fix/check:** record/trace the same native and v2 initial bus faders, compressor bypass/output, group gains and modulation intensities; capture output immediately before and after each bus/rack and use the existing on/bypass reference cases. Correct the first divergent state/law in `sampler-kontakt/library.rs`, `effects.rs`, and the KSP/core slot-parameter dispatch. Ask the KSP owner to reconcile init/persistence writes; do not blindly merge the older branch, which lacks newer live bus handling. A production fix is deferred until that attribution is measured.

### 7. P0 — Missing Kontakt filter families; supported SV filters also miss resonance gain (M/L)

**Exposure:** **77 containers (27 NKIs + 50 NKMs) / 5,681 enabled slots** lack a filter implementation. Ladder LP4 alone **52 (2 NKIs + 50 NKMs) / 3,593**; other Ladder forms, AR, legacy HP/LP, SV BP2 and Formant are listed below. V2 `effects.rs:523` admits only SV 52/54/55/57, with Daft 70/71 separately. Unsupported filters leave no equivalent processing. Legacy HP ID3 at saved zero is not "off": host GUI reads about **36.1 Hz in the observed modulated preset**. AR106 cutoff law is **8.2×4329^x**, not SV's **25×800^x** (native example 603 Hz vs substituted 774 Hz).

For supported SV52 (**5 NKIs +45 NKMs**), the measured native passband attenuation reaches about6 dB at resonance1; `effects.rs:479` explicitly omits that compensation, making this a quantified fidelity gap as well as a coverage gap.

**Fix:** first port v1 `src/engine/filter/ladder.rs` native LP4 recurrence/control timing, then its explicitly labelled proxy implementations where preferable to omission, keeping diagnostics until native parity is measured. Validate ladder resonance/drive at multiple input levels. Do not infer all Ladder IDs from one native LP4 kernel. `v1/src/engine/filter.rs:68` and `src/fx/blocks.rs:1018` show greater runnable filter coverage.

### 8. P0 — Missing Kontakt creative/dynamics FX used by 60 containers (L)

**Exposure:** **60-container union (10 NKIs + 50 NKMs) / 723 enabled slots**, 21 IDs; detailed table below. These include Psyche (19), Replika (18), Freak (17), Solid G-EQ (15), Raum (13), Lo-Fi (277 slots), Shaper, distortion, tape and modulation FX. Unknown payloads become absent processors, not approximate effects. ID **0x1d is Shaper**, not the baseline's misleading SurroundPanner label. V1 already rendered generic Lo-Fi, Skreamer, Shaper, distortion, tape, delay, chorus, flanger, phaser, limiter, Solid Bus Comp and Solid G-EQ (`src/fx/blocks.rs:376`, `:1016`).

**Fix:** merge typed layouts from `v2/gpt-format-fxmod`, then port v1's applicable kernels as explicitly approximated fallbacks and implement the high-use modern FX. Decode compressor mode as integer, Lo-Fi trailing byte correctly, and GEQ mixed byte flags; current blanket f32 assumptions misinterpret typed fields. Payload decode is necessary but is not an audio implementation. Start reference captures with Psyche/Replika/Freak/Raum and per-module default/on/bypass sweeps.

### 9. P0 — Dominant Multi-LFO type and random bipolar modulation are absent (M/L)

**Exposure:** LFO type6 **51 (1 NKI + 50 NKMs) / 9,282**, type5 **2 / 3,832**; RandomBipolar **51 / 28,050** external entries. `library.rs:1009` rejects type6 and admits only restricted type5 mixtures. Native current payload map also supports direct saw/random and both Multi variants. Core can synthesize more waves, but that does not make the importer complete. RandomBipolar is explicitly `UnknownLaw` (`library.rs:801`). V1's admitted sine-only Multi negates sine (`src/engine/lfo.rs:116`, `:142`); v2 maps an admitted sine to a positive analytic sine, a possible 180-degree polarity regression requiring host confirmation.

**Fix:** merge source-byte retention first, implement both Multi layouts and native weighting/polarity/random state; compare same saved preset at a fixed retrigger phase. Retain native fade state (floor .3) and 32-frame phase updates. Current core uses linear fade and analytic phase (`voice_mod.rs:511`); free-running tempo changes recompute phase from absolute time. Verify transport continuity and synced fade records rather than copying nominal Hz alone.

### 10. P1 — Modulation targets and decoded sync fields do not reach DSP (M)

AHDSR occurs in **all 834** containers; Flex **176 / 6,884**. External sources include Velocity737, PB795, CC1 400, CC11 396, CC111371 and CC113365 containers. `library.rs:915` routes a narrow set (volume, pitch, play position and mapped filter cutoff); other effect parameters, resonance, Daft cutoff and many shaped/signed routes are not rendered. Daft deliberately registers no filter-slot address (`effects.rs:662`). Current unipolar flag0x02 paths are rejected, while the newer reader identifies independent signed depth; shaper/invert behavior remains partly heuristic. AHDSR timing records and second LFO fade sync record are not fully consumed.

**Fix:** merge the typed modulation reader/source-retention work, implement signed intensity separately from inversion, add processor parameter addresses and route supported controls through the existing IR. Compare attack/release under tempo changes, cutoff+resonance modulation, bipolar shapers and note-off queue updates. V1 primary-envelope clock/native float32 recurrence is a better reference than a new curve rewrite: this audit already shows v2's equal-tick curve shape is accurate.

### 11. P1 — Malformed occupied FX slots disappear silently; fixes already available (S/M)

`effects.rs:94` uses `.ok()?`, program rack reads around `:132` skip failed siblings; `library.rs:615` uses `if let Ok`. An occupied malformed insert/send/main/bus FX can vanish without the unsupported report explaining it. That is a reporting/coverage failure even if the whole instrument loads.

**Available fix:** RE `NI_FILE_FX_DECODE_REPORTING.md` documents **30 passed / 0 failed / 3 ignored** checks and callback-based scoped/slot decode diagnostics. Code is **uncommitted** in read-only `decipher-readers-v2` on `feat/decipher-readers-v2`, base **2fb8c926dd39bb7ac26a84d4806f42de14b6630e**, 14 dirty files when inspected. It must be committed and merged/adapted; **it is not missing work**. Preserve current v2 engine writes/output_set behavior when adapting its older rack files. No commit SHA exists for those dirty fixes yet. Broader public Program reader changes in the same dirty set should be reviewed by the format owner.

## Kontakt modules observed in the corpus

Status uses three categories: **implemented** for a supported settled/simple path, **approximated** for native laws/scope/state not established or restricted, **missing** for no runnable processor. "Implemented" never means all native controls are certified. Per-module isolated reference gaps are explicit; whole-instrument errors are not assigned to every module it contains. Enabled slot/container counts follow the rules above. Bypassed counts and NKI/NKM splits are in the TSV.

### Filters and EQ

| Saved ID / module | Enabled containers / slots | Enabled NKIs / NKMs | Bypassed containers / slots | Status | Native measurement / limitation |
|---|---:|---:|---:|---|---|
| Filter:2 — Legacy LP | 1 / 4 | 1 / 0 | 3 / 264 | missing | No isolated paired render; no translated processor. |
| Filter:3 — Legacy HP (GUI HP1) | 20 / 556 | 20 / 0 | 0 / 0 | missing | Saved zero reads GUI 36.1 Hz in the observed modulated preset, not off; DSP absent. |
| Filter:6 — Legacy HP4 | 3 / 6 | 3 / 0 | 3 / 18 | missing | No isolated paired render; no translated processor. |
| Filter:13 — Phaser | 0 / 0 | 0 / 0 | 1 / 10 | missing | No isolated paired render; no translated processor. |
| Filter:19 — Versatile | 0 / 0 | 0 / 0 | 2 / 2 | missing | No isolated paired render; no translated processor. |
| Filter:22 — 1-band EQ | 186 / 6736 | 186 / 0 | 348 / 5604 | approximated | RBJ band shape; native coefficient/update vectors available, host residual unmeasured. |
| Filter:23 — 2-band EQ | 139 / 3921 | 139 / 0 | 65 / 1800 | approximated | RBJ band shape; native coefficient/update vectors available, host residual unmeasured. |
| Filter:24 — 3-band EQ | 38 / 823 | 38 / 0 | 22 / 583 | approximated | RBJ band shape; native coefficient/update vectors available, host residual unmeasured. |
| Filter:30 — Ladder LP1 | 1 / 876 | 1 / 0 | 1 / 17 | missing | No isolated paired render; no translated processor. |
| Filter:32 — Ladder LP3 | 1 / 1 | 1 / 0 | 0 / 0 | missing | No isolated paired render; no translated processor. |
| Filter:33 — Ladder LP4 | 52 / 3593 | 2 / 50 | 52 / 438 | missing | V1 native nonlinear LP4 kernel exists; v2 absent. |
| Filter:35 — Ladder HP2 | 0 / 0 | 0 / 0 | 1 / 1 | missing | No isolated paired render; no translated processor. |
| Filter:39 — Ladder BP4 | 19 / 20 | 1 / 18 | 7 / 8 | missing | No isolated paired render; no translated processor. |
| Filter:41 — Ladder Notch | 1 / 1 | 1 / 0 | 1 / 4 | missing | No isolated paired render; no translated processor. |
| Filter:50 — SV LP1 | 0 / 0 | 0 / 0 | 3 / 6 | missing | No isolated paired render; no translated processor. |
| Filter:51 — SV HP1 | 0 / 0 | 0 / 0 | 3 / 12 | missing | No isolated paired render; no translated processor. |
| Filter:52 — SV LP2 | 50 / 280 | 5 / 45 | 51 / 120 | approximated | SV52 cutoff/Q measured; up to 6 dB resonance passband compensation missing. Other shapes borrow these laws. |
| Filter:53 — SV BP2 | 13 / 19 | 0 / 13 | 13 / 14 | missing | No isolated paired render; no translated processor. |
| Filter:54 — SV HP2 | 6 / 65 | 5 / 1 | 12 / 12 | approximated | SV52 cutoff/Q measured; up to 6 dB resonance passband compensation missing. Other shapes borrow these laws. |
| Filter:55 — SV LP4 | 1 / 20 | 1 / 0 | 0 / 0 | approximated | SV52 cutoff/Q measured; up to 6 dB resonance passband compensation missing. Other shapes borrow these laws. |
| Filter:57 — SV HP4 | 1 / 20 | 1 / 0 | 0 / 0 | approximated | SV52 cutoff/Q measured; up to 6 dB resonance passband compensation missing. Other shapes borrow these laws. |
| Filter:70 — Daft LP | 43 / 110 | 1 / 42 | 29 / 51 | approximated | Native control laws; proxy nonlinear/audio-resampling kernel; modulation address absent. |
| Filter:71 — Daft HP | 4 / 8 | 1 / 3 | 1 / 1 | approximated | Native control laws; proxy nonlinear/audio-resampling kernel; modulation address absent. |
| Filter:90 — Formant I | 1 / 291 | 1 / 0 | 1 / 192 | missing | No isolated paired render; no translated processor. |
| Filter:100 — AR LP2 | 1 / 10 | 1 / 0 | 1 / 167 | missing | No isolated paired render; no translated processor. |
| Filter:101 — AR BP2 | 23 / 24 | 0 / 23 | 1 / 1 | missing | No isolated paired render; no translated processor. |
| Filter:102 — AR HP2 | 10 / 12 | 1 / 9 | 1 / 1 | missing | No isolated paired render; no translated processor. |
| Filter:103 — AR LP4 | 1 / 1 | 1 / 0 | 1 / 1 | missing | No isolated paired render; no translated processor. |
| Filter:106 — AR LP2/4 | 1 / 267 | 1 / 0 | 1 / 16 | missing | Native cutoff 8.2×4329^x known; DSP absent. |

### Effects

| Saved ID / module | Enabled containers / slots | Enabled NKIs / NKMs | Bypassed containers / slots | Status | Native measurement / limitation |
|---|---:|---:|---:|---|---|
| 0x10 — Legacy Delay | 1 / 5 | 1 / 0 | 4 / 6 | missing | No isolated paired render; no translated processor. |
| 0x11 — Legacy Chorus | 1 / 1 | 1 / 0 | 3 / 3 | missing | No isolated paired render; no translated processor. |
| 0x12 — Legacy Flanger | 0 / 0 | 0 / 0 | 3 / 3 | missing | No isolated paired render; no translated processor. |
| 0x13 — Gainer | 194 / 194 | 144 / 50 | 1 / 4 | approximated | Static dry+wet×gain measured; imported matrix skips native recurrence. |
| 0x14 — Legacy Phaser | 0 / 0 | 0 / 0 | 4 / 4 | missing | No isolated paired render; no translated processor. |
| 0x15 — Legacy Reverb | 0 / 0 | 0 / 0 | 1 / 4 | missing | No isolated paired render; no translated processor. |
| 0x16 — Convolution | 415 / 477 | 367 / 48 | 7 / 10 | approximated | Bus FFT convolution; incomplete native IR shaping/settings; voice/group unsupported. |
| 0x17 — Send Levels | 810 / 819 | 777 / 33 | 3 / 3 | approximated | Bus send gains only; group/slot tap semantics missing. |
| 0x19 — Compressor | 1 / 1 | 1 / 0 | 7 / 13 | approximated | Bus-only generic compressor; isolated on/bypass residual about 0.3–0.5 dB; mode/ratio guess. |
| 0x1a — Inverter | 30 / 1787 | 30 / 0 | 4 / 576 | approximated | Plain dB Output validated by ±6-dB host cases; invert/swap flag order remains unverified. |
| 0x1c — Limiter | 1 / 1 | 1 / 0 | 0 / 0 | missing | No isolated paired render; no translated processor. |
| 0x1d — Shaper (baseline mislabels SurroundPanner) | 6 / 174 | 6 / 0 | 4 / 471 | missing | No isolated paired render; no translated processor. |
| 0x1e — Distortion | 1 / 8 | 1 / 0 | 4 / 478 | missing | No isolated paired render; no translated processor. |
| 0x1f — Stereo Modeller | 363 / 21619 | 363 / 0 | 3 / 6 | approximated | Settled matrix about -119 dB residual; smoothing/pseudo delay missing. |
| 0x20 — Lo-Fi | 1 / 277 | 1 / 0 | 4 / 209 | missing | No isolated paired render; no translated processor. |
| 0x21 — Skreamer | 1 / 80 | 1 / 0 | 3 / 6 | missing | No isolated paired render; no translated processor. |
| 0x22 — Rotator | 0 / 0 | 0 / 0 | 3 / 3 | missing | No isolated paired render; no translated processor. |
| 0x42 — Tape Saturator | 7 / 8 | 1 / 6 | 3 / 9 | missing | No isolated paired render; no translated processor. |
| 0x43 — Transient Master | 0 / 0 | 0 / 0 | 3 / 3 | missing | No isolated paired render; no translated processor. |
| 0x44 — Solid G-EQ | 15 / 41 | 3 / 12 | 3 / 30 | missing | No isolated paired render; no translated processor. |
| 0x46 — Solid Bus Comp | 6 / 6 | 0 / 6 | 3 / 3 | missing | No isolated paired render; no translated processor. |
| 0x4c — Feedback Compressor | 0 / 0 | 0 / 0 | 3 / 3 | missing | No isolated paired render; no translated processor. |
| 0x4d — Jump | 0 / 0 | 0 / 0 | 1 / 4 | missing | No isolated paired render; no translated processor. |
| 0x59 — Modern Reverb | 373 / 376 | 370 / 3 | 1 / 1 | approximated | Native RT60/predelay calibration; generic FDN, other parameters guessed. |
| 0x5a — Replika | 18 / 18 | 0 / 18 | 0 / 0 | missing | No isolated paired render; no translated processor. |
| 0x5b — Phasis | 5 / 5 | 0 / 5 | 0 / 0 | missing | No isolated paired render; no translated processor. |
| 0x5c — Flair | 9 / 9 | 0 / 9 | 0 / 0 | missing | No isolated paired render; no translated processor. |
| 0x5d — Choral | 6 / 6 | 0 / 6 | 0 / 0 | missing | No isolated paired render; no translated processor. |
| 0x60 — Supercharger | 8 / 8 | 0 / 8 | 0 / 0 | missing | No isolated paired render; no translated processor. |
| 0x63 — Psyche | 19 / 19 | 0 / 19 | 0 / 0 | missing | No isolated paired render; no translated processor. |
| 0x65 — Raum | 13 / 13 | 0 / 13 | 0 / 0 | missing | No isolated paired render; no translated processor. |
| 0x66 — Bite | 7 / 7 | 0 / 7 | 0 / 0 | missing | No isolated paired render; no translated processor. |
| 0x68 — Freak | 17 / 21 | 0 / 17 | 0 / 0 | missing | No isolated paired render; no translated processor. |
| 0x69 — Vibrato Chorus | 7 / 7 | 0 / 7 | 0 / 0 | missing | No isolated paired render; no translated processor. |
| 0x6a — Wow/Flutter | 9 / 9 | 0 / 9 | 0 / 0 | missing | No isolated paired render; no translated processor. |

### Modulators and external sources

| Source | Containers / instances | NKIs / NKMs | Status | Limitation |
|---|---:|---:|---|---|
| AHDSR | 834 / 236192 | 781 / 53 | approximated | 45 equal-tick native lifecycle comparisons: max error 7.1811e-7; audio/control-clock parity unmeasured. |
| Flex | 176 / 6884 | 174 / 2 | approximated | Breakpoint envelope subset; sync/full native curve and release laws unmeasured. |
| LFO:6 | 51 / 9282 | 1 / 50 | missing | Translator rejects predominant Multi variant. |
| LFO:5 | 2 / 3832 | 2 / 0 | approximated | Restricted single-wave Multi subset; sign/fade/control cadence require native comparison. |
| PitchBend | 795 / 234565 | 742 / 53 | approximated | Source translated; supported targets/shaper/queue laws remain restricted. |
| Velocity | 737 / 524853 | 684 / 53 | approximated | Source translated; supported targets/shaper/queue laws remain restricted. |
| MidiCc(1) | 400 / 66951 | 350 / 50 | approximated | Controller source supported; processor target coverage/signed depth incomplete. |
| MidiCc(11) | 396 / 24270 | 396 / 0 | approximated | Controller source supported; processor target coverage/signed depth incomplete. |
| MidiCc(111) | 371 / 201063 | 368 / 3 | approximated | Controller source supported; processor target coverage/signed depth incomplete. |
| MidiCc(113) | 365 / 199551 | 364 / 1 | approximated | Controller source supported; processor target coverage/signed depth incomplete. |
| MidiCc(110) | 337 / 107206 | 337 / 0 | approximated | Controller source supported; processor target coverage/signed depth incomplete. |
| Unassigned | 240 / 54654 | 240 / 0 | implemented (inert) | No assigned source; correctly skipped, not a missing audible modulator. |
| MidiCc(112) | 152 / 23304 | 152 / 0 | approximated | Controller source supported; processor target coverage/signed depth incomplete. |
| Constant | 90 / 21719 | 38 / 52 | approximated | Source translated; supported targets/shaper/queue laws remain restricted. |
| Script(1) | 83 / 9298 | 83 / 0 | approximated | Source translated; supported targets/shaper/queue laws remain restricted. |
| KeyPosition | 70 / 15445 | 18 / 52 | approximated | Source translated; supported targets/shaper/queue laws remain restricted. |
| ReleaseTriggerCounter | 60 / 3404 | 58 / 2 | approximated | Source translated; supported targets/shaper/queue laws remain restricted. |
| RandomBipolar | 51 / 28050 | 1 / 50 | missing | Explicit UnknownLaw; no source emitted. |
| RandomUnipolar | 51 / 9282 | 1 / 50 | approximated | Source translated; supported targets/shaper/queue laws remain restricted. |
| Script(0) | 51 / 37383 | 1 / 50 | approximated | Source translated; supported targets/shaper/queue laws remain restricted. |
| Script(3) | 41 / 6952 | 41 / 0 | approximated | Source translated; supported targets/shaper/queue laws remain restricted. |
| MidiCc(100) | 39 / 5160 | 36 / 3 | approximated | Controller source supported; processor target coverage/signed depth incomplete. |
| MidiCc(101) | 39 / 2288 | 36 / 3 | approximated | Controller source supported; processor target coverage/signed depth incomplete. |
| MidiCc(80) | 3 / 6 | 3 / 0 | approximated | Controller source supported; processor target coverage/signed depth incomplete. |
| Script(4) | 2 / 32 | 2 / 0 | approximated | Source translated; supported targets/shaper/queue laws remain restricted. |

No enabled direct sine/rectangle/triangle/saw/random Kontakt LFO types, mono/poly aftertouch or release-velocity source were observed by this saved census. Disabled AHDSR: 1 container / 505; disabled type5 LFO: 1 / 2,180. This does not establish that scripts never create them at runtime. Unlisted catalog-only modules have no positively identified saved occurrence in this manifest; unknown saved identity aliases remain unresolved.


## UVI modules observed in the corpus

**660/660 programs parsed, zero failures** in the corrected optimized metadata pass. This machine required `KONTRA_UVI_READER=/home/derpcat/.codex/cache/kontakto-uvi-official-reader/app/UVIWorkstationx64.exe`; default discovery misses that cache location (`sampler-uvi/src/access.rs:185`). Failed attempts without the reader were discarded. All final per-item caches use `census2`; aggregation asserts completeness and success.

### Effects and filters

| Module | Enabled programs / instances | Saved programs / instances | Status | Limitation |
|---|---:|---:|---|---|
| Gain | 660 / 1606 | 660 / 1608 | approximated | Static Aux/Keygroup Gain and Program Gain; later node writes not bound. |
| OnePole | 660 / 243682 | 660 / 246878 | approximated | All observed in Keygroups; tracking uses middle key, coefficient law/float32 SIMD binding unverified. |
| ThreeBandShelves | 660 / 2977 | 660 / 3260 | missing | No translated processor. No UVI host reference render was available. |
| SampledReverb | 659 / 859 | 660 / 860 | approximated | Aux convolution approximation; Time/damping/predelay/width ignored, missing IR becomes unity. |
| CombFilter | 620 / 1784 | 620 / 241180 | missing | No translated processor. No UVI host reference render was available. |
| Drive | 620 / 620 | 620 / 620 | missing | No translated processor. No UVI host reference render was available. |
| Maximizer | 620 / 620 | 620 / 620 | missing | No translated processor. No UVI host reference render was available. |
| SparkVerb | 620 / 1234 | 620 / 1240 | missing | No translated processor. No UVI host reference render was available. |
| XpanderFilter | 620 / 79675 | 620 / 241180 | missing | No translated processor. No UVI host reference render was available. |
| DualDelay | 612 / 612 | 620 / 620 | missing | No translated processor. No UVI host reference render was available. |
| FeedbackMachine | 596 / 596 | 620 / 620 | missing | No translated processor. No UVI host reference render was available. |
| WaveShaper | 129 / 324 | 620 / 2480 | missing | All observed active slots on Layers, which are not translated. Rectifier modes6/7 exist only for supported insert scopes; public numbering unverified. |
| MS20 | 90 / 13095 | 620 / 240560 | missing | No translated processor. No UVI host reference render was available. |
| DiodeClipper | 86 / 86 | 620 / 620 | missing | No translated processor. No UVI host reference render was available. |
| Flanger | 68 / 148 | 620 / 2480 | missing | No translated processor. No UVI host reference render was available. |
| Phasor | 64 / 144 | 620 / 2480 | missing | No translated processor. No UVI host reference render was available. |
| Convolver | 40 / 376 | 40 / 376 | approximated | All observed on Aux; FFT IR loading, no native setting/null proof; unreadable IR passes input. |
| DigitalEq | 40 / 40 | 40 / 80 | approximated | Aux only; band numbering/slope/channel assumptions, live GainScale/routes absent. |
| EffectRack | 40 / 218 | 40 / 440 | approximated | Aux parallel branch/gain structure exists; dependent effects and later writes restricted. |
| GainMatrix | 40 / 3508 | 40 / 6180 | approximated | Stereo 2×2 only; native 12×12 and live matrix destinations missing. |
| TrackDelay | 40 / 160 | 40 / 160 | missing | Nonzero delay has no processor; zero-time inserts are accepted as an inert no-op. |

### Modulators

| Source | Enabled programs / instances | Saved programs / instances | Status | Limitation |
|---|---:|---:|---|---|
| ConstantModulation | 660 / 1531 | 660 / 1531 | approximated | Constant/boolean/bipolar values fold; live Value/modulation targets restricted. |
| ScriptEventModulation | 660 / 5988 | 660 / 5988 | approximated | Source IDs supported; bipolar negative values clamp to0; note/global script scope needs host verification. |
| LFO | 659 / 731 | 660 / 732 | approximated | Sine/triangle and retriggered square subset; lookup/control clock, shared/fade/rate changes not fully native. |
| AHD | 620 / 2480 | 620 / 2480 | approximated | Attack/hold/decay curve path exists; default release used at note-off, not measured. |
| DAHDSR | 620 / 4960 | 620 / 4960 | approximated | Curved envelope subset; shared/retrigger/note-off/velocity-amount modes approximated; no native host pair. |
| StepEnvelope | 618 / 1232 | 620 / 1240 | approximated | Step/level sequence subset exists; manual trigger/some retrigger/direction/smoothing modes restricted; shared scope inferred. |
| MultiLFO | 615 / 615 | 620 / 620 | approximated | Sine+noise normalized bipolar subset; other mixes missing; smoothing uses a continuous approximation to native32-frame Euler lag. |

### External sources and routing

| Built-in source | Programs / connections | Status |
|---|---:|---|
| @PitchBend | 659 / 4341055 | approximated: source available, destinations/scope/lag restricted |
| @VoiceParam Velocity | 640 / 450026 | approximated: source available, destinations/scope/lag restricted |
| @MIDI CC 1 | 615 / 573792 | approximated: source available, destinations/scope/lag restricted |
| @MIDI CC 102 | 40 / 2200 | approximated: source available, destinations/scope/lag restricted |
| @MIDI CC 103 | 40 / 9588 | approximated: source available, destinations/scope/lag restricted |
| @MIDI CC 104 | 40 / 733 | approximated: source available, destinations/scope/lag restricted |
| @MIDI CC 105 | 40 / 9588 | approximated: source available, destinations/scope/lag restricted |
| @VoiceParam Key | 40 / 20814 | approximated: source available, destinations/scope/lag restricted |
| @MIDI CC 106 | 31 / 31 | approximated: source available, destinations/scope/lag restricted |
| @MIDI CC 107 | 16 / 16 | approximated: source available, destinations/scope/lag restricted |
| @VoiceParam KeyFollow | 2 / 534 | approximated: source available, destinations/scope/lag restricted |

At least one unsupported FX/filter family is present in **660/660 programs** under these saved-enabled rules (zero-time TrackDelay and Layer WaveShaper included; this is exposure, not proof of a nonzero native default effect). Scope presence: Program 660, Layer 605, AuxEffect 660, Keygroup 660 programs. Counts of every module/scope and destination are retained in the TSV.

The strongest concrete destination gaps are: OnePole:Freq and key tracking parameters, GainMatrix inputs3–12, DigitalEq:GainScale, and Program/Layer Gain/Pan/Frequency scope connections. Exact serialized destination spellings/counts are listed below; enabled connections exclude their own bypass and zero Ratio, but do not prove an audible upstream source.

| Destination | Programs / connections |
|---|---:|
| SamplePlayer:Pitch | 660 / 6977288 |
| CombFilter:Freq | 620 / 242829 |
| Keygroup:Gain | 620 / 509250 |
| Layer:Gain | 620 / 4960 |
| XpanderFilter:Freq | 620 / 242829 |
| OnePole:Freq | 598 / 185211 |
| MS20:Freq | 559 / 242209 |
| LFO:Depth | 431 / 443 |
| MultiLFO:Depth | 365 / 367 |
| Keygroup:Pan | 234 / 141426 |
| BusRouter:Gain | 144 / 522 |
| ConstantModulation:Value | 40 / 40 |
| DigitalEq:GainScale | 40 / 145 |
| GainMatrix:Gain_1_1 | 40 / 5594 |
| GainMatrix:Gain_2_1 | 40 / 3960 |
| GainMatrix:Gain_3_1 | 40 / 5514 |
| GainMatrix:Gain_4_1 | 40 / 5172 |
| GainMatrix:Gain_5_1 | 40 / 5172 |
| GainMatrix:Gain_6_1 | 40 / 4488 |
| SamplePlayer:Gain | 40 / 19576 |
| SignalConnection:Ratio | 40 / 42310 |
| Flanger:Mix | 38 / 82 |
| LFO:Freq | 38 / 40 |
| WaveShaper:Mix | 37 / 86 |
| CombFilter:Q | 35 / 6305 |
| MS20:Q | 35 / 6305 |
| XpanderFilter:Q | 35 / 6305 |
| Program:Gain | 32 / 32 |
| WaveShaper:Amount | 31 / 80 |
| GainMatrix:Gain_10_2 | 27 / 3012 |
| GainMatrix:Gain_12_2 | 27 / 3012 |
| GainMatrix:Gain_8_2 | 27 / 3012 |
| GainMatrix:Gain_11_2 | 25 / 2660 |
| GainMatrix:Gain_2_2 | 25 / 1634 |
| GainMatrix:Gain_9_2 | 25 / 2660 |
| Phasor:Depth | 25 / 66 |
| MultiLFO:Freq | 23 / 23 |
| GainMatrix:Gain_7_1 | 18 / 1644 |
| GainMatrix:Gain_4_2 | 9 / 342 |
| GainMatrix:Gain_6_2 | 9 / 684 |
| GainMatrix:Gain_7_2 | 9 / 1368 |
| Flanger:Speed | 5 / 12 |
| Flanger:Feedback | 4 / 8 |
| GainMatrix:Gain_11_1 | 4 / 352 |
| GainMatrix:Gain_9_1 | 4 / 352 |
| Phasor:Speed | 4 / 8 |
| Phasor:Feedback | 2 / 4 |
| ScriptProcessor:XpanderDrive_4 | 2 / 2 |
| ScriptProcessor:MIDICurve_1 | 1 / 1 |

No CompExp or AnalogADSR source was observed in this manifest; the core has restricted support for them. The scope table is essential: a supported kernel on an Aux/Keygroup does not imply that a similarly named Layer or Program module is translated. Unknown API catalog entries are not assigned a fabricated audible error.


## Reference render comparisons

Native renders already exist in `~/.cache/kontra-reference` (23 instrument probe sets plus isolated FX captures). This audit does **not** relaunch the recorder. The reference branch documents the following isolated Kontakt 8.13.1 measurements:

| Module / native capture | Measured native behavior | V2 comparison and limit |
|---|---|---|
| SV LP2, noise sweep (reference §2) | cutoff 25×800^x; Q inverse `(2-.013)×(1-r)^3.1+.013`; resonance passband loss up to 6 dB | Cutoff/Q used; missing compensation can leave v2 up to about 6 dB hot below cutoff at maximum resonance. LP4/HP variants not independently calibrated. |
| AR LP2/4 (reference §8) | cutoff 8.2×4329^x, 603 Hz in the saved example | Missing processor; SV's 774-Hz substitution would also be wrong. |
| Stereo Modeller (reference §20) | Settled matrix, unity at width .5, linear balance pan | Static formula residual about -119 dB; pseudo mode and automated width/pan not certified by that recording. |
| Reverb Room/Hall burst (reference §21, §28) | Default time display3.2 s produces RT60≈2.60 s; size/diffusion change reflections without changing RT60; 8-kHz decay at damping50 ≈1.63 s | Imported time calibration matches intended law; no paired spectrum/early-reflection proof for the generic FDN. Damping/topology remain approximated. |
| Compressor Analog C4/E4/G4 on vs bypass (reference §26) | About +8.4…9.0 dB RMS net, dominated by +9-dB output gain | Earlier v2 +8.2…8.4 dB, isolated residual about .3… .5 dB around the documented +8.7-dB case. Different whole-instrument selection is not attributed to the compressor. |
| Fresh Gainer automation (reference §25) | dry+.wet×gain, fresh default `.5+.5g`; roughly43–49-ms transition | Settled mix honored; native state/smoothing bypassed by importer. Kernel `1/1800` and host timing discrepancy needs binding verification. |
| Inverter Output (reference §27) | ±6-dB Output produces a plain ±6-dB gain | Current output gain reaches the signal. This corrects stale coverage wording. |
| Shaper Vista CC100 sweep (reference §11/current translator) | Curvature K≈17, fitted K18 residual .08 dB | Piecewise approximation in routing; .08 dB is the earlier host-fit residual, not a fresh full-corpus guarantee. |

Fresh baseline comparison:

Fresh **7e82b152** `sampler-native`, optimized `ci` build, scripts enabled, existing `scen.mid`, 48-kHz stereo output. Each reference and v2 file is aligned to its first sample above peak-50 dB; each grid note is measured over **0.10–0.45 s after that aligned onset**. Power is mean `(L²+R²)/2`, avoiding mono cancellation. Attack is the first20-ms/body power ratio. This is a grid/level diagnostic, **not a waveform null or matched-sample DSP-only test**. First-onset offsets include native capture lead and are not host-latency measurements. Native recordings are about104 s vs98.5-s CLI files, so whole-file energy ratios include different captured tails; use held-note windows for the level comparison.

| Instrument | Windows | Mean steady error dB | Max absolute error dB | Mean absolute relative attack error dB | Native audible/v2 silent |
|---|---:|---:|---:|---:|---:|
| analog_strings | 48 | +14.48 | 15.47 | 1.49 | 0 |
| una_pure | 48 | +0.31 | 0.80 | 0.93 | 0 |
| vista_3cellos | 48 | +1.27 | 4.07 | 2.27 | 0 |

Rechecking the older cached v2 WAV with the same stereo/window metric yields Analog **+5.48 /6.47 dB**, Una **+.31 /.80 dB**, Vista **+1.27 /4.07 dB**. Analog fresh-vs-cached differs almost exactly9 dB; the other two metrics are identical. The cached binary and FX/bypass state are unpinned, so attribute this only after inspecting the initial compressor/output/bus state. The KSP owner corroborated the available +9-dB compressor reference and code differences, but has no old/fresh state dump or proven causal fix (finding 6). The archived NI engine-parameters Modulation section explicitly distinguishes positive-only MOD_TARGET_INTENSITY from bipolar MOD_TARGET_MP_INTENSITY (zero modulation at500000); the positive-only clamp alone is not a fault.

Data: [fresh baseline metrics](data/dsp-fresh-reference.json), [same-metric cached comparison](data/dsp-cached-same-metric.json), [baseline/capture provenance](data/dsp-provenance.json). Audio remains outside Git in the task cache. Reproduce with `sampler-native render-kontakt-midi NKI CACHE/fresh-NAME.wav ~/.cache/kontra-reference/probe/NAME/scen.mid` under the heavy wrapper; then run `dsp_reference_metrics.py CACHE` under the wrapper. Old v1 WAVs use a half-spacing note timeline, so their original matching reports below must not be compared as raw sample-aligned recordings.


For context, the original probe reports show the following older renders. The cached renderer binaries are **not pinned to 7e82b152 or 0cb7a8a0**, so these are historical results, not a fresh version comparison. Metrics follow that recorder's per-note matching algorithm and differ from this audit's fixed stereo-power windows. Complete scalar-only summary: [historical reference data](data/dsp-historical-reference.json).

| Instrument | Historical v2 mean / worst signed level error dB | Historical v1 mean / worst signed dB | Same sample v2 / v1 | Interpretation |
|---|---:|---:|---:|---|
| Una Pure | +.13 / +.96 | -5.92 / -6.03 |91.7% /91.7% | Strong evidence the newer gain calibration improved this captured case. |
| Una Cotton | +.06 / +.76 | -6.00 / -6.08 |91.7% /91.7% | Same conclusion, not blanket DSP parity. |
| Analog Strings | +4.10 / +5.28 | -3.05 / -4.30 |75.9% /24.1% | Both differ; selection confounds direct DSP attribution. |
| Vista3 Cellos | +.89 / +2.84 | -12.29 / -16.85 |92.3% /0% | Better newer capture, but old v1 selects different content. |
| Dolce Vln1 Sus | +23.68 / +25.98 | +8.16 / +12.25 |0% /0% | Severe whole-instrument mismatch; cannot label as a measured filter/effect residual. |
| Areia Vln Sus | +19.83 / +20.34 | +4.55 / +8.91 |0% /0% | Same selection/state confound. |


## V1 strengths and limits

1. Native **Ladder LP4** recurrence/control timing and broader executable filters (`src/engine/filter/ladder.rs`, `src/engine/filter.rs:68`). V2's Daft nonlinear proxy improves on v1's linear Daft proxy, but omitting the native Ladder is a regression.
2. Primary **AHDSR** native float32 floor/state, 32-frame controls and audio interpolation (`src/engine/ahdsr.rs:29`, `:182`); sine-only Multi's negative sine/fade/cadence (`src/engine/lfo.rs:106`). V2's equal-tick AHDSR shape already agrees closely, so port timing/state selectively.
3. Broader usable **drive/modulation/dynamics/GEQ** blocks (`src/fx/blocks.rs:1016`), **ordered per-voice group send taps into summed send returns** (`src/engine/filter.rs:2062`, `src/fx/processor.rs:394`, `:777`), and live parameter/IR updates (`:599`, `:618`). These are working approximations, not complete Kontakt fidelity.
4. V2 improves some settled gains substantially in existing Una Corda reference results. It also fixes positive Stereo Modeller width: at saved spread+1 the host-measured/v2 matrix is `[[2,-1],[-1,2]]`, versus v1's `[[1.5,-.5],[-.5,1.5]]` (`src/fx/processor.rs:917`). For a settled L-only unity input, that predicts v1 L/R magnitude errors -2.50/-6.02 dB; this is a coefficient comparison against the measured law, not a newly recorded v1 host render. No evidence establishes v1's reverb or convolution as more faithful. Pinned v1 has no UVI backend, so UVI parser/static-chain breadth is a v2 advantage.
5. RE `ALTERNATING_LOOPS.md` separately verifies v1 forward/backward reflection for zero-crossfade alternating loops: 31 active loops per inspected Una instrument, 28 alternating / 3 forward. Three real presets, 3,036,139 frames each, match independently unrolled PCM with **max absolute error 0** at the continuation snapshot. This is **not a Kontakt reference render** and not a DSP-effect proof; crossfaded alternating loops still fall back, mode2 release interpretation remains unverified. It is available sample-engine work, not a missing effect to reimplement.

## Existing work to integrate

| Branch / work | Availability | What it supplies | What it does not supply |
|---|---|---|---|
| `v2/gpt-decipher-dsp@aa3430d6` (incl. `3ffda10d`, `78a70f0d`) | Unmerged at audit baseline | Native laws/vectors, AHDSR lifecycle, external-modulator timing, full saved census | Production replacements for every processor |
| `v2/gpt-format-fxmod@b7f6af0b` (incl. `a490f7be`) | Unmerged at audit baseline | Typed/mixed FX fields, additional payload layouts, signed intensity and retained complete source/timing records | Audible FX kernels or established unknown laws |
| `v2/kontakt-reference@7d15e5b8` | Unmerged at audit baseline | Reference captures/tools/docs, gain/filter/reverb/shaper calibration evidence | Native parity for all modules, a host recording for UVI |
| `v2/dsp-kernels@d32a2ba5` | Already merged into baseline | Core Daft/Gainer/FDN/convolution/control implementations and coverage doc | Native full topology, complete importer wiring; do not count as a new merge fix |
| `feat/decipher-readers-v2` dirty, base `2fb8c926…` | Available, needs commit and adaptation | Public Program reader + occupied-FX decode diagnostics, tests documented by RE | A commit identifier; cannot cherry-pick an uncommitted patch |
| RE `ALTERNATING_LOOPS.md`, continuation `80dbb6f…` | Available, sample engine scope | Zero-crossfade loop reflection proof against unrolled PCM | Crossfade/mode2 native-host certification |

Integration should preserve current scripted FX write behavior and avoid replacing new files wholesale with older versions. No branch was merged into this audit just to make the baseline appear more complete.

## Unknowns and the measurements that resolve them

- **Per-module audible E for missing/approximate modules:** record an authored preset with unity gain and deterministic white noise/impulse/sine sweep; capture bypass and enabled at min/mid/max settings and 44.1/48/96 kHz. Render v2 and pinned v1 with identical MIDI. Report aligned stereo residual, magnitude response error, gain/tail error and latency; distinguish stochastic modulation from deterministic kernels. Join each module's measured E to the committed per-container census for a real `usage×error` ranking. Never extrapolate a one-preset worst case as corpus average.
- **Routing:** two overlapping voices into group compression/drive and tap an early send before a later filter; verify native per-voice group processing against summed bus processing. Include native amplitude insert position, main/instrument bus pan, ordered send taps and stereo cancellation input.
- **Envelope/LFO:** compare 32-frame host onset/phase, non-grid note-off, zero stages, repeated retrigger, shared sources, tempo changes, both Multi payload variants, fade sync, random seed/state and external control queue's last-write/flush behavior. Current original-byte vectors isolate native routines; stubbed math helpers, save-to-runtime conversion and host context can still differ.
- **Convolution:** match IR identity (metadata hash only), record impulse response for early/late crossover, filtering, stretch/decimation, envelope, auto-normalize, reverse, predelay and latency-compensation flags. Missing UVI IR is currently a unity impulse fallback; require an explicit unavailable-resource state rather than presenting it as a room effect.
- **UVI host:** no Falcon executable/reference audio was available. Workstation 4.0.9 bytes and plugin interface checks cannot certify Falcon or full Workstation playback. Capture representative AO/VWinds dry/effect-isolated presets with macros/CC automation using the recorder owner's rig.
- **Counts beyond this manifest:** the 2,741-library-file historical inventory and 1,937 structural payload corpus are different populations, with snapshots/muted/bypassed duplicates. Do not mix their module counts with this manifest. Extend the same per-item metadata census if broader installed-bank coverage is desired.

## Validation

- `cargo test --no-run`: **passed** (52.51 s build, all requested targets compiled); no production source changed afterwards.
- Both audit examples compile; UVI census optimized build passed. Native CLI `ci` build passed.
- Selected core suites `envelope`, `buses`, `dsp`, `svf`, `daft`, `gainer`, `compressor`: **34 passed /0 failed**. These check core routing/numerical behavior, not complete native-host parity.
- Envelope probe: 45 cases /3,150 outputs; comparison asserts max absolute error<1e-6 and finite/stereo-equal/final-zero outputs.
- Corpus aggregation asserts **834 Kontakt caches and 660 UVI caches**, final UVI cache format/success; all 660 parsed, zero failed. Dataset/source identity is in `dsp-provenance.json`.
- Three current-baseline real-library renders completed with exit0; 144 note windows compared, no native-audible/v2-silent window in those selected cases.
- Python syntax and Markdown/data-link checks passed; `git diff --check` passed before commit.
- No native host was launched or Wine settings changed; no decrypted library payload was persisted; no development server remains. A transient shared-disk threshold halted two shards/builds once; resumed jobs completed. Production fixes were intentionally left to phase2.
