# Gold standard: the voice's sound path (playback DSP)

Scope: resampling, loops, reverse, pitch, pan law, gain staging, click guards, denormals, per-voice
filters, bus/insert/send topology. Research only; no code, builds or commits.

Evidence tags: `[source]` code read in a repo or a vendor source tree; `[binary]` Kontakt8.exe or
UVIWorkstationx64.exe strings or RE notes; `[doc]` vendor manual, Lua/KSP API or web doc; `[inferred]`
reasoning from the above, not measured. Two KONTRA engines exist and both are described:
**fresh** = `crates/engine/*` (the v3 core), **old** = `crates/sampler-core` (+ `sampler-kontakt`).

No renders were made in this pass (read-only; no oracle run). Every "measured" claim cites an earlier
probe in the repo. The measurement plan at the end lists what still must be run.

---

## 1. What each reference does

### 1.1 Kontakt 8

**Resampling (HQI).**
- `[doc]` Kontakt manual, Source module, HQI: three algorithms. "Standard uses the algorithm of old Kontakt
  versions ... light on the CPU. High and Perfect ... eliminate virtually all audible artifacts (especially
  when transposing upwards) ... the further they need to transpose a Sample from its root key, the higher
  the CPU load." Only samples played away from root are resampled.
- `[doc]` Offline Interpolation Quality: a separate setting for bounces; default "like realtime".
- `[binary]` the group field is `interp_quality` with values 0 Standard, 1 High, 2 Perfect
  (`docs/re/engine/k-sampler-source.md:146`). Every surveyed native group has `interp_quality = 0`.
- `[binary]` The real-time kernels are **not recovered**: no named kernel, and Blackman/Kaiser/sinc constant
  searches failed (`k-sampler-source.md:147`; `docs/re/engine/KONTAKT_CPU_ARCHITECTURE.md:43-44`).
- `[inferred]` Because CPU cost rises with step for High/Perfect, Kontakt's High/Perfect are long
  windowed-sinc-class kernels. The manual's cost wording is the only hard evidence.

**Loops.**
- `[doc]` Kontakt manual, loop editor: "X-Fade: ... Kontakt can fade the end of the loop region into its
  beginning ... in milliseconds."
- `[doc]` Lua API loop modes: `off`, `until_end`, `until_end_alt`, `until_release`, `until_release_alt`.
  Crossfade is set in frames (`set_sample_loop_xfade`, LUA_API_Manual "sample_loop_modes").
- `[source]` Vendor parser: `ni-file/src/kontakt/objects/loop_data.rs` reads `mode, loop_start, loop_length,
  loop_count, alternating_loop, loop_tuning, x_fade_length`.
- `[measured, earlier]` Oracle probe on Audio Imperia CHORUS "09 Ensemble - Sustained Oh": Kontakt's
  crossfade matches the **linear complementary** law to 0.1 dB (residual -69.5 dBFS); equal-power sits 2–3 dB
  high mid-fade (`docs/re/engine/LOOP_XFADE_HOLD_PROBES.md` §1). Seam 2 shows a Kontakt dropout, not
  explained.
- `[binary]` The script constant `$NI_FADE_EQUAL_POWER` exists (strings in Kontakt8.exe). This is a
  script-side fade curve, not proof that the loop law is equal-power. The probe settles the loop law.

**Pan and gain.**
- `[doc]` Zone Pan: "placed independently in the stereo panorama"; Zone Volume ±36 dB; Zone Tune ±36
  semitones (Mapping editor). Group Volume/Pan in the Amplifier module. Instrument Volume default -6 or 0
  dB, chosen in Options.
- `[doc]` Lua: `set_instrument_volume`/`set_group_volume` range -inf..+12 dB; `set_zone_volume` ±36 dB;
  `set_*_pan` -100..+100 %; `set_zone_tune` ±36 semitones.
- `[doc]` Constant Power, Sinusoid and Logarithmic panning exist **only in the Surround Panner** effect. The
  manual does not give the stereo amplifier law.
- `[binary]` Instrument bus pan: left `1 - clamp(pan,0,1)`, right `clamp(pan+1,0,1)`, unity at centre, no
  crossfeed (`docs/re/kontakt-bus-pan.md`, same-SHA Ghidra and r2 evidence). This is a balance law.
- `[inferred]` Zone and group pan probably use the same balance law (the bus is an audio path, the group
  amp is one level up). Not measured.

**Buses and topology.**
- `[doc]` Signal path: Source → Group insert FX → Amplifier (volume, pan, phase invert, L/R swap, channel
  routing) → Bus (optional, 16 buses, insert chain with volume/pan/phase/swap, output selectable,
  "bypass insert" option) → Instrument insert chain (no amp; volume/pan in the header) → output.
- `[doc]` Bus and instrument insert effects are **monophonic** (summed signal); group insert effects run per
  voice.
- `[doc]` Send levels are per sample (Delay/Reverb), plus Macros.

**Click guards, release, reverse.**
- `[doc]` Release-trigger samples; "Monophonic" option; reverse is a per-sample playback direction (Reverse
  button in the Sample module).
- `[doc]` Zone/sample edit: "volume from -inf (no sound) to +12 dB".
- `[binary/doc]` The release-trigger path (Dolce, Staccato) is a separate open investigation
  (`docs/re/engine/corpus-gaps-voice.md`). Not in this subsystem's scope beyond the gate note.

### 1.2 UVI Workstation / Falcon

- `[binary]` UVIWorkstationx64.exe strings: `Equal Power (-3dB)`, `EqualPower`, `Balance`, `Balance (coarse)`,
  `Balance (fine)`, `CrossFade`, `Flatten Crossfade`, and the cross-fade curve description "(1.0: linear
  crossfade, 2.0: equal-power cross-fade)". These are UI and parameter strings only; the DSP is not decoded.
  Read: UVI exposes **both** a balance and an equal-power pan law, and a crossfade-curve parameter that
  chooses linear or equal-power.
- `[doc, web, KVR forum, 2015 era]` Falcon v1.0.2 fixed a cross-faded alternate-loop issue. The forum
  reports forward crossfades of 15+ seconds on long soundscapes, and the developer says the forward crossfade
  smooths the loop end into the loop start while alternate loops are already smooth. The developer also says
  crossfades work cleanly only on a zero crossing.
- `[doc]` UVIScript folder `/home/derpcat/Documents/UVI/Falcon/UVIScript` is **empty**. No UVI manual is on
  disk. The UVI library folders in `/data/main/Libraries/UVI` (Augmented Orchestra, VWinds Clarinets/Double
  Reeds/Flutes) were **not read** in this pass; `UVI Starter.ufs` was not found there.

### 1.3 HISE (`/data/dev/workspace/Research/samplers/hise`)

- `[source]` `hi_streaming/hi_streaming/StreamingSamplerVoice.cpp:724` `#define USE_CUBIC_INTERPOLATION 0`
  ("not there yet"). The streaming voice uses **linear** interpolation (`interpolateLinear`, lines 746–752,
  775–781, 826–827, 868–869).
- `[source]` Loop crossfade is a **precalculated buffer** (`ModulatorSamplerSound.h:212`, "the loop crossfade
  at the end of the loop (using a precalculated buffer)"). Sample properties: LoopStart/LoopEnd/LoopXFade
  (`ModulatorSamplerSound.cpp:44–61`).
- `[web, HISE forum, not docs]` A Hart reply says the crossfade is linear, and that enabling crossfade builds
  a short linear-fade buffer. Another thread mentions a user-adjustable shape. Forum reports of pops on HISE
  xfade loops exist. Weak evidence; treat as user reports.
- `[source]` Pan in `StreamingSamplerVoice.cpp`: no fixed law found in the streaming voice (grep miss);
  HISE routes pan through a modulator. Not characterised.

### 1.4 sfizz (`/data/dev/workspace/Research/samplers/sfizz/src/sfizz`)

- `[source]` Interpolators: nearest, linear, Hermite-3, B-spline-3, and windowed-sinc with 8, 12, 16, 24, 36,
  48, 60 and 72 points (`Interpolators.h:16–35`). Quality is a choice, not a fixed kernel.
- `[source]` Loop crossfade: `Voice.cpp:1755–1759` `xfSize = round(loop_crossfade * rate)`, clamped to
  `loop.start`; the crossfade is **before** the loop start (`xfInStart = start - xfSize`), read from the pre-loop
  guard. Curve: `Config.h:131 loopXfadeCurve = 2` (default S-curve), `Voice.cpp:1264–1290` also has linear
  (0) and a curve-table option (1).
- `[web, sfzformat.com]` `loop_crossfade` opcode: "Loop cross fade", seconds, no published range. The shape
  "depends on the player": sfizz S-curve `(1-cos(πx))/2`; OpenMPT `x^0.75`.
- `[source]` Pan: `Panning.cpp:23–40`. A cosine table with `piTwo` (read as π/2): left `cos(p·π/2)`, right
  `cos((1-p)·π/2)`. This is an **equal-power** law; centre is -3 dB per side.
- `[web, sfzformat.com]` `pan_law` opcode values `balance` and `mma`; law shapes are not stated on the page.

### 1.5 Other players (web, no measurements)

- Decent Sampler: search returned no manual section on loop crossfade or pan law. Not characterised.

---

## 2. KONTRA now

### 2.1 Fresh engine (`crates/engine/*`)

Voice sample path, `crates/engine/voice/src/lib.rs`:
- `:20` `MAX_STEP = 64.0` (+4 octaves at 4× rate). `:199–205` `clamp_step`: **silently clamps** any higher
  step to 64 (keys above the root by more than 48 semitones play at the wrong pitch; no count is kept).
- `:22` `LOOKAHEAD = 8192` source frames prefetched; `:24` `GAIN_SMOOTH = 0.01` (≈2 ms one-pole).
- `:207–215` `hermite` 4-point 3rd-order Hermite for **every** step, including step > 1 (no band limit).
  The comment at `:208` says a band-limited kernel waits for an aliasing measurement.
- `:130–196` render: linear interpolation between frames `y1,y2`, reads a 4-tap window `[base-1..base+3]`
  (`:174`), per-frame envelope and gain (`:177–181`).
- `:69–78` `start`: `anchor=0`, `gain = target = start.gain` (no gain ramp at start), `envelope` from the
  group. No start fade.
- `:80–82` `release` → envelope release; `:85–87` `fade(frames)` used for steal/choke.

Loops and reverse, `crates/engine/voice/src/zone.rs`:
- `:7–13` `Loop { start, end }`: forward only, no crossfade field, no mode, no count, no alternating.
- `:23–24` comment: "forward loop until end only; alternating and until-release loops and loop crossfades
  when a library needs them."
- `:30–35` `active_loop`: a loop is **disabled** when the region is reversed or when `l.start < region.start`.
- `:39–41` `finished`: never finished while a loop is active (until the envelope releases).
- `:47–77` `runs`: maps logical position to source frames; reverse descends; loop wraps by modulo.

Note and region start, `crates/engine/engine/src/lib.rs`:
- `:47` `FADE_SECONDS = 0.005` (steal/choke fade).
- `:404–438` note-on: `step(zone,&n,rate)`, `voice_gain(zone,&n,factor)`, envelope from group; start offset
  `n.offset` converted to frames and moves `region.start` (`:424–427`) without a fade.
- `:630–660` `set_*` path: updates gain and step on live notes (`set_step` clears the anchor).
- `:748–778` `voice_gain`: product of crossfade-control gains × group volume mods × release factor, then
  `gain()`.
- `:797–803` `step`: `2^(semitones/12) × zone.sample_rate / rate`; semitones = `(key-root)×keytrack + tune +
  note.tune + expression_tune + script_tune/100000`. **No pitch-bend term.**
- `:806–812` `gain`: `g = zone.gain × expression_gain × script`; pan `p = clamp(zone.pan + expression_pan +
  script_pan/1000, -1, 1)`; **left `g·min(1-p,1)`, right `g·min(1+p,1)`**. Balance law, unity at centre, no
  power compensation.
- `:82–86` `BusState { gain, enabled, mix }`: **no pan, no phase, no L/R swap** on buses.

Translation, `crates/format/kontakt/src/translate.rs`:
- `:134–135` reverse from the zone or criteria.
- `:339–362` loops: accepts `mode` 1 or 2 only (`until_end`, `until_release`); **`until_release` is treated as a
  continuous loop**. Several loops → only the first is kept (`notes.add "several loops"`). **`alternating_loop`**
  is noted ("alternating loop", Audible) and played as a plain forward loop. **`x_fade_length`** is noted
  ("loop crossfade", Audible) and **discarded**. `loop_count` and `loop_tuning` are discarded.
- `:370–371` `volume_db: db(zone_volume)`, `pan: clamp(zone_pan)`. `:150–152` `db = 20·log10(max(lin,1e-6))`
  (floor -120 dB). `:154–156` `semitones = 12·log2(ratio)`.
- `prepare.rs:248–249` group+zone volume summed in dB and pan summed then clamped. `prepare.rs:267–269`
  `gain(db) = 10^(db/20)`. `prepare.rs:256–264` envelope in seconds → frames, `release` ≥ 1 frame.

Envelope, `crates/engine/voice/src/envelope.rs`:
- `:17–23` `GATE`: attack 0, hold 0, decay 0, sustain 1, **release 64 frames** (≈1.3 ms at 48 kHz).
- `:37` `FLOOR = 1e-6`; `:48–55` decay/release coefficient = 60 dB over N frames, exponential, not Kontakt's
  curve (the file's own ponytail note says so).

Denormals: no flush-to-zero and no anti-denormal bias in `crates/engine/*` (grep found none). The envelope
floor (`FLOOR`) bounds the envelope state only; filters and reverbs do not exist in the fresh voice path.

### 2.2 Old engine (`crates/sampler-core`, `crates/sampler-kontakt`)

- `resample.rs:12–22`: `MIN_STEP = 1/2048`, `MAX_STEP = 2048` (11 octaves), `OCTAVES = 11`, `RADIUS = 48`,
  `SHORT_RADIUS = 12`, `LONG_CUTOFF = 0.9` Nyquist (-6 dB point), `LONG_BETA = 10` (Kaiser).
- `resample.rs:35–39` `ResampleQuality { Realtime (default), High }`. Realtime: four-point cubic for unity
  and upsampled voices, radius-12 Blackman-sinc for downsampled voices. High: radius-48 Kaiser-sinc
  everywhere non-unity (`docs/re/engine/k-sampler-source.md:48–51`). Polyphase bank for `1 < step ≤ 2`:
  8 stretches/octave, 65 rows.
- `resample/cubic_block.rs:5` `cubic_four` (block cubic). `dsp/space/resample.rs`: Kaiser 16-tap pair for convolution.
- `source.rs:31–45` `LoopShape { Wrap, Crossfade{frames}, EqualPowerCrossfade{frames}, PingPong }`.
  `Crossfade` blends linear complementary gains into the **pre-loop guard** (reverse: post-loop guard), keeps the
  loop period. `EqualPowerCrossfade` uses cos/sin gains. `PingPong` reflects.
- `source.rs:49–100` `Loop` with `LoopMode { Continuous, UntilRelease }`, `loop_count` (total passes), and
  per-slot `LoopSlot` (8 slots); `source.rs:120–157` shifts loops when the start is inside a loop.
- `sampler-kontakt/src/library.rs:3500–3560`: reads all 8 Kontakt loop slots. Maps `mode 1|2`
  (`until_end`/`until_release`), keeps `crossfade = Span::Frames(x_fade_length)`, `alternating`, `count`,
  `tuning`. Alternating + crossfade is **reported unsupported** (`alternating loop crossfade law`), the rest
  is kept. Invalid loops are reported. This is the reason Areia crossfades survive in the old engine.
- Denormal guards exist and cite native constants: `dsp/color/distortion.rs:58` `ANTI_DENORMAL = 1e-20`;
  `dsp/gater.rs:16` `1e-18` in the filter recursion (addresses `0x144f385a4`); `dsp/stereo_tune.rs:20`,
  `dsp/rotator.rs:39` (`0x144f385a8`).
- Pitch bend: `lower.rs:1191` `with_bend_range`, `receiver.rs:2046` `state::pitch(...)`, `:2346` bend
  range per receiver. The bend range is a real parameter in this engine.

Survey evidence for the old engine (`/data/dev/workspace/scratch/survey/full-20261022.jsonl`, 735 completed
instrument records, 1,395 listed): the "unsupported" counts for the playback DSP are:
- 37,203 "keys beyond the runtime's pitch range, narrowed" (Conflux 51 instruments, Areia 30, Dolce 11). This
  is the record from the era when the runtime covered four octaves; the old engine now covers 11 (see
  `corpus-gaps-voice.md`, "Keys beyond the runtime pitch range narrowed").
- 126 "loop crossfade longer than its loop, clamped" (Audio Imperia CHORUS, 4 instruments).
- 18 "loop starting before the zone start or past its end, dropped" (Performance Samples Vista, 1 instrument).
- 5,149 "Filter: filter law" (Conflux 51, Una Corda 3, ANALOG STRINGS 1). Outside this subsystem; noted.
- 8 "Wow/Flutter pitch law" (Conflux). Pitch-related; still unsupported.

---

## 3. Library evidence (what relies on what)

| Behaviour | Libraries that rely on it (where evidence exists) | Evidence |
|---|---|---|
| Loop crossfade (X-Fade in ms) | Audio Imperia CHORUS (126 clamped; probe on "09 Ensemble"), Areia 1.2.0 (coordinator: 6,228 zones with crossfades) | survey JSONL; LOOP_XFADE_HOLD_PROBES §1; coordinator note |
| Loop starting before zone start | Performance Samples Vista (18) | survey JSONL |
| Keys far above root (pitch range) | Conflux 1.1.0 (51), Areia (30), Dolce (11) | survey JSONL (old-era counts) |
| Pitch law (wow/flutter) | Conflux (8) | survey JSONL |
| Bus pan (balance, saved) | Not counted per library. Engine-level RE only | kontakt-bus-pan.md |
| Alternating / until-release loops | Not counted in this pass (census is a follow-up) | none |
| HQI setting | interp_quality = 0 in every surveyed native group | k-sampler-source.md:146 |
| Dolce, Pacific Strings, Afflatus Brass | Script-driven articulations, not a playback-DSP census | corpus-gaps-voice.md |

Not read in this pass (did not open presets or scripts): UVI libraries (Augmented Orchestra, VWinds), Ample,
Modo Drum (`.pak`), Superior Drummer (the folder name; the coordinator's "EZDrummer" is not on disk). A per-library
census of loop modes, alternating flags, crossfade lengths, pan values, and HQI values is the follow-up. It needs
`sampler-native`/`ni-file` reads of `.nki`/`.nkm`, not a build, and was not run here.

---

## 4. Gaps, ranked by audible impact

1. **Loop crossfades dropped in the fresh engine** (`translate.rs:351–353`; `zone.rs:7–13`). Seam clicks or
   a level step at every loop in libraries that use X-Fade (CHORUS, Areia). Old engine already does this
   (`source.rs:37`). Kontakt's law is linear (probe, 0.1 dB). Impact: high.
2. **Pitch clamped at +4 octaves in the fresh engine** (`voice/lib.rs:20`, `:199–205`). Wrong pitch above
   root+48 semitones with no diagnostic. Old engine covers 11 octaves. Impact: high on wide-keymapped
   libraries (Conflux, Areia, Dolce by the survey).
3. **Interpolation: Hermite everywhere, no band limit** (`voice/lib.rs:207–215`). Aliasing when transposing
   up. Kontakt's manual calls this out explicitly; old engine has sinc (Realtime cubic / High sinc48). Impact:
   high on up-pitched material. Kontakt HQI kernels unknown, so "better than Kontakt" must be measured.
4. **Alternating loops, until-release, loop count and tuning dropped** (`translate.rs:340–353`; `zone.rs:23`).
   Alternating plays as a repeated forward loop (audible turnaround artefacts). Until-release loops never
   continue past the loop after note-off (the release envelope cuts instead). Impact: medium–high, depends on
   how many libraries use them (census pending).
5. **Pan law is pure balance with 0 dB centre** (`engine lib.rs:806–812`). Centre is +3 dB in mono sum
   relative to an equal-power law; balance matches Kontakt's measured bus law but Kontakt's zone/group law is
   unmeasured. Impact: medium (level and width at centre), and a format-compat question.
6. **No pitch bend in the fresh engine** (`engine lib.rs:797–803`). Old engine has bend range
   (`receiver.rs:2046`). Impact: medium for performance, zero for static patches.
7. **Buses have no pan, phase invert or L/R swap** (`engine lib.rs:82–86`). Kontakt bus has all four
   (`kontakt-bus-pan.md`; manual). Impact: medium where a library routes stereo buses.
8. **No start fade; release is 64 frames** (`envelope.rs:17–23`; `lib.rs:404–438`). Note-on at a non-zero
   start offset (`n.offset`, `lib.rs:424–427`) or a loop wrap onto a non-zero sample can click. Impact: medium,
   [inferred] until measured on Areia / Vista. Stop side is covered by the 1.3 ms release.
9. **Loops disabled on reversed regions** (`zone.rs:31–34`). Kontakt reverse-with-loop behaviour is unmeasured.
   Impact: unknown; census needed.
10. **Denormals: no guard in the fresh path** (no flush-to-zero; no anti-denormal bias). The voice path is
    safe (envelope floor), but future filters and reverbs in the fresh engine will need the native-style bias
    (1e-18/1e-20). Impact: low now, high later.
11. **Unsupported filter and pitch-law items in the old engine** (survey: 5,149 filter-law, 8 wow/flutter
    pitch-law records). Outside this subsystem's core; listed for the filter owner.
12. **Gain staging is correct but unmeasured**: dB sums in `prepare.rs:248`, `gain()` at `:267`, `db()` floor
    at -120 dB (`translate.rs:150`). Kontakt's instrument default (-6 or 0 dB option) is not mirrored. Impact:
    low for level (per-library gain is set by the library), medium for loudness-matched A/B.

---

## 5. Gold standard design (format-neutral)

Each item states the Kontakt and sfizz/HISE references, the choice, and the reason. Items marked
**(measure first)** must not ship until the oracle or a probe says so.

### 5.1 Resampler (one kernel family, selectable quality)

- **Step ≤ 1 (source read slower than output, or unity):** no new spectral images, so cubic Hermite is enough
  (the fresh engine already does this; old engine's Realtime does too).
- **Step > 1:** band-limited polyphase windowed sinc. Quality tiers: Realtime = radius 12 (old `SHORT_RADIUS`,
  Blackman), High = radius 48 Kaiser β=10, cutoff 0.9 Nyquist (old `LONG_*`). The old engine's High kernel
  measured stop-band -102 dB at 1.02×Nyquist (-81 dB at Nyquist), so gold keeps that as the floor.
- **Quality mapping from Kontakt HQI** (format-neutral name: `interp`): Standard → Realtime; High → High;
  Perfect → High with radius ≥ 48 (the Kontakt kernels are unknown, so this mapping is **[inferred]**;
  **(measure first)** against an oracle sine sweep at +12, +24 and +36 semitones).
- **CPU rule:** Kontakt's manual says High/Perfect cost grows with transposition. Gold keeps that behaviour
  (radius independent of step, but cost is bounded by `MAX_STEP`). Pitch range: **11 octaves** (old MAX_STEP
  2048), not 4.
- **Why:** Kontakt's manual says the artefacts are "especially when transposing upwards"; sfizz offers
  8–72-point sinc as a quality knob; HISE's streaming voice uses linear (cheapest, worst at up-pitch). Gold keeps the cubic for downward pitch, sinc for
  upward, and never clamps silently: out-of-range steps are counted as a stat.

### 5.2 Loops (format-neutral model)

```
Loop { start, end, crossfade_frames, shape: Linear | EqualPower | Wrap,
       mode: Continuous | UntilRelease, alternating: bool, count: Option<u32>, tuning: f64 }
```
- **Crossfade shape:** default **Linear** complementary (Kontakt measured, 0.1 dB, CHORUS). EqualPower
  as an opt-in shape for formats that say so (UVI's crossfade-curve parameter; `$NI_FADE_EQUAL_POWER`
  exists in Kontakt's scripts). S-curve (sfizz default) is not used because it is not what Kontakt does.
- **Placement:** blend the pre-loop guard into the loop end, as old `source.rs:33` does. Length =
  `x_fade_length` in frames (no ms conversion inside the engine; the format layer converts).
- **Clamp:** crossfade ≤ loop length and ≤ frames available before `start` (old `library.rs` and sfizz
  `Voice.cpp:1757` both clamp). Report clamped loops as diagnostics.
- **Alternating:** ping-pong frame map (old `PingPong`), with endpoints once per turn.
- **UntilRelease:** keep the loop until note-off, then play to region end (matches the `until_release`
  name in the Kontakt Lua API). The release envelope then runs over the tail. **(measure first)** against an
  oracle render of an `until_release` loop.
- **Reverse:** allow loops on reversed regions with the same mapping; **(measure first)**.
- **Loop start before zone start:** keep (old engine), with the Vista case reported, not dropped.

### 5.3 Pitch

- `step = 2^((key-root)·keytrack/12 + tune/12 + bend/12) · sample_rate/rate` with the bend term added
  (pitch bend range from the plan; default 2 semitones, format-specific). Clamp at 2048 with a stat.
- **Wow/flutter:** keep as a separate modulator, not inside `step`.

### 5.4 Pan law

- Engine-level enum `PanLaw { Balance, EqualPower }`. Default **Balance** (the Kontakt bus law, RE-measured),
  so existing Kontakt libraries keep their image. EqualPower available for formats that name it (UVI "Equal
  Power (-3dB)", sfizz cosine table). **(measure first)**: Kontakt zone and group pan law, with a
  mono-noise probe at pan -1, -0.5, 0, 0.5, 1 against the oracle.
- Bus pan, phase invert and L/R swap in the bus model (gap 7).

### 5.5 Gain staging

- Internal linear gain (f32). dB only at load: `volume_db` per zone/group/instrument; sum in dB, one
  conversion (as `prepare.rs:248` does now).
- Ranges as Kontakt's Lua API: zone ±36 dB, group and instrument -inf..+12 dB. Instrument default is a
  format setting (Kontakt: -6 or 0 dB), not a DSP default.
- Headroom: no clamp inside the voice; the output stage is the only place that can clip; report clipped
  samples as a stat. Kontakt does not publish its output-stage clip law; **(measure first)**.
- Gain smoothing: keep the 2 ms one-pole on live gain changes (fresh `GAIN_SMOOTH`), and keep the envelope
  as the only start shaping.

### 5.6 Click guards

- **Start:** a 0.5–1 ms linear fade-in whenever the first frame is non-zero (start offsets and loop wrap
  onto non-zero frames). Cheap and inaudible in the normal case. **(measure first)** on Areia and Vista
  note-on at offset.
- **Stop:** keep the 64-frame release floor (1.3 ms at 48 kHz) and the `fade(frames)` steal path (5 ms).
- Steal/choke fade stays 5 ms (`FADE_SECONDS`).

### 5.7 Denormals

- Flush-to-zero and denormal-free state on the audio thread (`set` once per render thread), plus the native
  anti-denormal bias (1e-18 on filter recursions, 1e-20 on distortion; from the old engine) for every
  recursive DSP block. Envelope floor stays 1e-6.

### 5.8 Per-voice filters and topology

- Per-voice filters are group-level DSP nodes that run per voice (Kontakt: group insert effects "calculated
  per voice"). Bus and instrument inserts run once on the summed signal (manual: "monophonic"). Keep that.
- Signal path (format-neutral): Voice → Group insert (per voice) → Group amp (volume, pan, phase, swap,
  routing) → Bus (sum; insert chain; amp: volume, pan, phase, swap) → Instrument insert (sum) → Output.
  Sends are post-fader, per sample or per group, with an optional bypass.
- The fresh engine has buses with gain only (`lib.rs:82–86`); adding pan/phase/swap is gap 7.

---

## 6. Measurement plan (proves each claim; not run in this pass)

Existing probes that already prove a claim:
- Loop crossfade law is linear: `docs/re/engine/LOOP_XFADE_HOLD_PROBES.md` §1 (CHORUS "09 Ensemble", seams
  0 and 1 at -69.5 and -70.3 dBFS against the linear render).
- Bus pan law is balance: `docs/re/kontakt-bus-pan.md` (RE, plus a synthetic PCM test in KONTRA).
- HQI setting values: `docs/re/engine/k-sampler-source.md:146`.

Measurements still to run (oracle: `tools/kontakt-oracle/render.py`, `kontra_render.py`; probes in
`tools/kontakt-oracle/probes/`):

| # | Claim to prove | Stimulus | Metric | Pass criterion |
|---|---|---|---|---|
| M1 | Areia loop crossfade law and length | Areia zone with `x_fade_length` > 0, held 10 s | residual at each seam vs Kontakt, dBFS | < -60 dBFS residual, as CHORUS |
| M2 | Alternating loop behaviour | Any library with `alternating_loop` (census first) | seam spectrum, residual | matches Kontakt ping-pong |
| M3 | Until-release loop continues past loop after note-off | `until_release` sample, note 2 s, release | tail length and RMS after note-off | tail length within 10 ms of Kontakt |
| M4 | HQI sinc quality vs Kontakt (up-pitch) | sine 1 kHz at +12, +24, +36 st, Standard/High/Perfect | alias spectrum (dBc at non-harmonic bins), THD+N | Perfect ≤ Kontakt Perfect by ≥ 6 dB alias rejection |
| M5 | Pitch range: keys beyond +48 st | a Conflux or Areia keymap at +60 st | pitch error in cents vs Kontakt | < 1 cent after the 11-octave fix |
| M6 | Zone/group pan law | mono pink noise, pan -1..+1 steps 0.5, group and zone | L and R RMS vs Kontakt | < 0.1 dB difference |
| M7 | Gain staging | 1 kHz sine, zone volume 0, -6, +12 dB; instrument default -6 and 0 | RMS delta vs Kontakt | < 0.1 dB |
| M8 | Start fade need | note-on at `n.offset` 0 and 20 ms on Areia and Vista | first-frame click (peak in first 2 ms) | no click vs Kontakt |
| M9 | Denormal behaviour | silence after a 1 s decay in an IIR block | CPU time per block with FTZ on and off | no slowdown when FTZ is set |
| M10 | Old vs fresh engine parity on loops | Same instrument through `sampler-core` and `crates/engine` | null test residual | -60 dBFS after the loop-crossfade fix |

Rules for each: stage-tagged failures; real libraries only (no fixtures-only pass); Kontakt rendered by the
oracle; no build run here.

---

## 7. Open questions and follow-ups

- Kontakt's Standard/High/Perfect kernels are not recovered. Settle with M4 before naming them.
- Kontakt zone/group pan law is not in the manual or the RE notes; settle with M6.
- The library census (loop modes, alternating flags, crossfade lengths, pan values, HQI, reverse-with-loop)
  is not run. It needs the `ni-file` chunk reader over the Areia, CHORUS, Vista, Conflux, Dolce, Pacific,
  Afflatus, Una Corda and ANALOG STRINGS `.nki` files, and the UVI/Ample/Modo/Superior folders. Not done here.
- UVIScript and the UVI manual are not on disk; web sources are 2015-era forum posts. Treat UVI as
  "crossfade curve and balance vs equal-power exist" only.
- Decent Sampler: no usable web docs found. Excluded from the design.
- The coordinator named "EZDrummer" and "UVI Starter.ufs"; neither was found under `/data/main/Libraries`
  (the drum folder is "Superior Drummer").
