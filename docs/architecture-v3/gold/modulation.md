# Gold standard: modulation (sources, targets, laws, envelopes, scripts)

Status: research draft, 2026-10-27. Read-only study; no code, build or commit.
Library-survey section (§8) is pending its agent; everything else is final.

Tags: `[source]` file:line read in this repo or a reference tree · `[binary]` string/address in a vendor binary · `[doc]` vendor or public manual (KM = Kontakt manual PDF page, KSP = KSP manual PDF page, UVI = lua.uvi.net / manual.uvi.net, via WebFetch summary unless noted) · `[inferred]` my reasoning · `[measured]` reproduced with a script in §9.

Reference trees: `Research/samplers/sfizz/src/sfizz/`, `Research/samplers/hise/hi_core/…`, `docs/re/`, `docs/architecture-v2/`, `crates/sampler-core/src/voice_mod/laws.rs`.

---

## 0. Summary

- Kontakt is the only reference with a measured, compiler-level law for modulation (family H, `x·(1−(1−e)·i)`, cube on external types 7–13). Its manual gives the UI and script surface, but no numeric ranges for most AHDSR and modulation controls.
- sfizz, HISE and SFZ state their combine rules cleanly. HISE (`g = (1−i) + i·x`) and SFZ (`amp_veltrack` = (v/127)² at 100) are the two public laws worth copying as data.
- UVI (Falcon) has the broadest source/envelope catalogue, but its curves are unstated and its script calls are partly stubs in KONTRA.
- KONTRA now models two targets (volume, group envelope) and one velocity law that does not match the measured Kontakt cube law (§4.1, −0.34 dB at velocity 64).

---

## 1. Kontakt 8 (the reference the library corpus was built for)

### 1.1 Sources
- [doc] KM p.305–318: envelopes (AHDSR, DBD, Flex), LFOs (Sine, Triangle, Rectangle, Saw, Random, Multi, Multi Digital), Step, Envelope follower, Glide, Zone envelopes (KM p.196), External (Pitch Bend, Poly/Mono Aftertouch, MIDI CC 0–127 with no standard meaning, Key Position, Velocity, Release Velocity, Release Trigger Counter, Constant, Random Uni/Bi, From Script).
- [doc] KM p.305: "Envelopes generally react to MIDI note messages." KM p.317: poly aftertouch only acts per key when voices are spread over round-robin Groups.
- [doc] KM p.308 CC source: value −1 means "do not modulate until the first CC arrives".
- [binary] Source-type names and classification: `0140a70000.c:3370-3392` (type 12 Random Unipolar, 13 Random Bipolar). [source] `crates/sampler-core/src/voice_mod/laws.rs:332-342` (external types 1–13 with proved/unproved domains).

### 1.2 Targets and routing
- [doc] KM p.306: routers live on per-Group modules (Source, Amplifier, Group insert effects). Instrument insert, send and main effects have no modulation (KM p.306, KM p.112 for buses).
- [doc] KM p.309: intensity range not given (silent). Invert reverses direction. Lag (smoothing) is a time constant in ms; default 250 ms for pitch assignments.
- [doc] KM p.308–309: Modulation Shaper is a 128-entry table indexed by MIDI value, or a segment curve.
- [binary] Target registry: `laws.rs:276-322` (`TARGET_IDS`, 166 entries; volume=2, pan=3, pitch=6, filter cutoff 31, startPhase 180, ahdsr_attack 184…187, dbd 191–194).
- [source] Route compiler law (family H, volume id 2): `crates/sampler-core/src/voice_mod/laws.rs:234-262`. H family: `x·C(1−(1−E)·i)` with `C = fcubic` for external types 7–13 (velocity is type 8), identity otherwise. Derived from `docs/re/kontakt-envelope-time-modulation.md` (instruction-level evidence, `0x140875ac8`, `0x140876753`).
- [inferred] The Kontakt volume-velocity law is therefore `fcubic(1 − (1−v)·i)` with `v` in 0–1, not a dB table. §4.1 measures it.

### 1.3 Velocity → volume law (the one measured law)
- [doc] KM p.308, p.317: velocity-to-volume is an example assignment; no default intensity or dB law stated (silent).
- [doc] KSP p.124 example: `$vol_amount := (($EVENT_VELOCITY − 1) × 12000 / 126) − 6000` → linear dB, −6 to +6 dB. A script example, not the default law.
- [doc] (repo) `docs/architecture-v2/KONTAKT_DSP_LAWS.md:433-462`: 10-point relative-dB table at a bare default route (v=1 → −12.9 dB … v=127 → 0 dB). Beat Machine route at intensity 50/127: −9.12, −5.71, −2.30 dB at v = 32, 64, 100.
- [measured] §9 test `vel_volume_fit`: fitting `(1−(1−x)·i)³` to the 10-point table gives i = 0.394 and RMS 0.027 dB. Fitting `Curve::Power` (KONTRA) gives RMS 0.26 dB; linear gives 0.87 dB. At the Beat Machine intensity the cube law is within 0.05 dB at all three points; `Curve::Power` is −0.34 dB at v=64.
- [inferred] The cube law is the Kontakt gold law. The fitted i for the default route is not known (silent in KM); it must be stored per route, not hard-coded.
- Open: full-intensity behaviour. The repo says "full intensity stays linear" (`KONTAKT_DSP_LAWS.md:453`), which contradicts the compiler form at i=1 (cube). Needs one oracle render at 127.

### 1.4 AHDSR (time, curve, hold, sustain)
- [doc] KM p.310–311: Attack (with Curve: −1 concave, 0 linear, +1 convex, attack only), Hold (fixed time, not scaled), Decay (to Sustain), Sustain (level held while key down), Release. AHD Only drops sustain. Retrigger on/off. KM times: no numeric ranges (silent).
- [source] Stored times are ms: `docs/re/engine/k-mod-envelopes.md:31-37` (reader stores raw f32 ms; converters apply only to script/automation values). Sustain is cubed on read: `crates/format/kontakt/src/translate.rs:204` (`powi(3)`).
- [source] Time converters (for knob/script values only): `exp(x·C + ln 2) − 2` ms, C = 8.92 (attack/hold, top 15 s) and 9.43 (decay/release, top 25 s): `crates/sampler-core/src/voice_mod/laws.rs:75-93`, KSP tests `crates/engine/engine/src/params.rs:110-115`.
- [binary] Retrigger restarts at the first nonzero stage start; release captures current affine output (`0x140ae5820`, `0x140ae5e40`): `k-mod-envelopes.md:67-72`.
- [binary] AHDSR kernel: `(state − 0.075)·scale + bias` per sample, state *= multiplier per stage (`0x140ae40f0`). Control rate R stored in the object, owner not identified (`k-mod-envelopes.md:78-82`).
- [binary] Attack curve: parameter maps knob ±1 to a log-ratio with `b = exp((1−|c|)·ln 500000 − ln 20000)` (`engine_parameters.rs:263-272`, reader `whole_voice.rs:21-35`). Shape for decay/release is a fixed exponential `ln(3/43)` in the old IR (`whole_voice.rs:30`); not confirmed against a binary constant in this study.
- [doc] KSP p.343: flex stage time "1000000 is one second". KSP p.342: `ENGINE_PAR_ATTACK/HOLD/DECAY/RELEASE(_UNIT)`, `SUSTAIN`, `ATK_CURVE`, `ENV_AHD`.

### 1.5 DBD and Flex
- [doc] KM p.310–313 DBD: D1 zero→breakpoint, break level (negative allowed), D2 breakpoint→zero. Flex: up to 32 breakpoints, first 8 times/levels and first 8 slopes modulatable (KM p.312, slopes since 7.1 KM p.344), segment curve 0.5 = linear (note: AHDSR curve uses 0 = linear).
- [binary] DBD preparation law: `kontakt-envelope-time-modulation.md`, `k-mod-envelopes.md:52-60` (level, slopes `(level−1/3)/N0`, `−level/N1`).
- [binary] Flex retrigger restarts at last point's level: `k-mod-envelopes.md:86-90`.

### 1.6 LFO, step, follower, glide
- [doc] KM p.313–316: LFO 0.01–210 Hz, sync to host / MIDI clock / master tempo, fade-in up to 5 s, start phase in degrees, retrigger on/off = freewheel. Multi = bipolar weights; Multi Digital "more mathematically accurate" (KM p.315).
- [source] Multi post-stage fade `f0` clamp `[0,1]` legacy / `[0.3,1.3]` v73 and output `(f0−0.3)`: `docs/re/kontakt-lfo-fade-cap.md:17-26` and `digital_fade.rs`. Admission refuses fade > 5000 ms on a rate-independent bound (`kontakt-lfo-fade-cap.md:53-71`).
- [doc] KM p.315 Step: up to 32 values, 0.01–210 Hz, one-shot, snap 1/12 (KM p.316).
- [binary] Step render: table + length + one-shot, `k-mod-lfo-step.md:70-75`.
- [binary] Follower: runs one span behind (previous block) `k-mod-external.md` "Follower input and timing" (decompile order; oracle TODO).
- [binary] Glide ramp: interval from record `rec[0x458]−rec[0x454]`, unit unproven (`k-mod-external.md` "Glide start").
- [doc] KM p.316: Glide intended for pitch at full intensity, monophonic use.

### 1.7 Scripts (KSP)
- [doc] KSP p.105–107: `get_mod_idx(group, name)`, `get_target_idx(group, mod, target)`; `find_mod`/`find_target` are deprecated (KSP p.380, Kontakt 7.1). `$NI_NOT_FOUND` on miss.
- [source] **Conflict:** the shipped corpus uses `find_mod` at ~891 k call sites (§8). The deprecated name is therefore the working one in practice; the spec must accept both, with `find_mod` as an alias of `get_mod_idx`.
- [doc] KSP p.116: `set_engine_par(par, value, group, slot, generic)`; range 0–1000000 except switches. Slot = modulator index; generic = target index (MP intensity, Flex stage, Step value).
- [doc] KSP p.342: `$ENGINE_PAR_MOD_TARGET_INTENSITY` positive only; `$ENGINE_PAR_MOD_TARGET_MP_INTENSITY` bipolar, centre 500000, max 1000000, inverted 0. `$MOD_TARGET_INVERT_SOURCE` (no `ENGINE_PAR_`, not described).
- [doc] KSP p.117 example writes `$Knob * 1000` for knob −1000..1000 (values −1000000..1000000), conflicting with the stated 0..1000000 range. Clamping not stated.
- [doc] KSP p.140–141: `set_event_par_arr(EVENT_ID, EVENT_PAR_MOD_VALUE_ID, value, index)` for From Script modulators (index 0–1000, clamped ±1000000).
- [doc] KSP p.349: `ENGINE_PAR_INTMOD_TYPE`, `_SUBTYPE` (AHDSR/FLEX/DBD; LFO sine/rect/tri/saw/rand/multi; no Multi Digital constant listed, gap).
- [doc] KSP p.362–363: changing an effect type removes its assignments; modulators cannot be inserted or removed.
- [repo] `docs/re/engine/k-ksp-engine-params.md:23-38`: `_m` variants (`set/get_engine_par_m`, slot 0–63) are compile-only in this repo's model; `get_engine_par_disp_m` not modelled.

### 1.8 Kontakt facts that differ from common assumptions
- Modulation is per Group (routers, slots). Per-voice behaviour of LFOs/envelopes is not stated (KM silent); the note-triggered voice copy is the default.
- Lag is ms; default 250 ms on pitch. The engine's control grid is 64 frames (`crates/sampler-core/src/voice_mod.rs`), and the native routine uses 32-frame cells (`laws.rs:357-372`).

---

## 2. UVI Falcon / UVIScript

Source caveat: local UVIScript folder is empty; manual PDF 403; facts come from WebFetch summaries of lua.uvi.net and manual.uvi.net. Quote-check before implementing.

- [doc] UVI: sources include Velocity, Mono Velocity, Release Velocity, Key, Key Follow (C3-centred, curve), Linear Key Follow, Poly/Channel Aftertouch, Mod Wheel, MIDI CC, Pitch Bend, Macros, Random, Unison, Organ Pan, Data Envelope (audio-file trace), Voice Modulator, Script Event Modulation (`sendScriptModulation`, EventId 0–127).
- [doc] Envelopes: AHD (Attack, Hold, Decay with Attack/Decay curve −1..1), DAHDSR (Delay, Attack, Hold, Decay, Sustain, Release, curves ±1), Analog ADSR (with KeyToAttack/Decay −2..2, VelToAttack/Decay −1..1, Punch, DynamicRange 0–40 dB), Attack-Decay (attack 1–99 % of decay), Multi Envelope (128 segments, Speed, Smooth), Step (32 values; 1–128 steps in XML). Curve formulas: not stated.
- [binary] Tooltip: "−100% means that the attack is halved each octave" (key-to-time depth). The sign and scale are only given at −100 %.
- [doc] LFO: Freq 0–20 Hz (beat ratio synced), Depth 0–1, Phase 0–1 (Parametric LFO −1..1 — conflict), Delay, RiseTime (depth fade-in), Smooth, Retrigger 0–3 (per note, per legato group, never, host timeline), waveform index 0–9 (names listed, index map not stated).
- [doc] Macro: Value 0–1, ratio per assignment (e.g. 1.0 cutoff, −0.5 resonance); ratio range not given.
- [doc] `sendScriptModulation(id, target, rampTime_ms, voiceId?)`: target normalised (unipolar or bipolar); ramp default 20 ms; omitted voiceId = broadcast. Return value not documented. Repo notes disagree about whether it has audio effect (`UVI_SCRIPT_COVERAGE.md:37-38` vs `UVI_LUA.md:125-126`).
- [doc] `setParameter` on unknown names is silently ignored (UVI element reference). Ramp/clamp behaviour not stated.
- [repo] KONTRA: LFO and DAHDSR `setParameter` writes are stubs (`UVI_PARITY_LEDGER.md:4909-4920`).
- [binary] Only ASCII strings probed; no decompile. Modulation names not stored as plain text (`uvi-survey-dsp-gaps.md:26-27`).

---

## 3. Other references

### 3.1 sfizz (open reference implementation of SFZ)
- [source] ADSR: attack linear `currentValue += attackStep`, `attackStep = 1/(sr·t)` (`Research/…/sfizz/src/sfizz/ADSREnvelope.cpp:24-30`). Decay and release exponential with coefficient `exp(−9/(t·sr))` (`:32-39`), i.e. a 9-time-constant fall. Release threshold `1e-4`, fade-out ramp 50 ms (`Config.h:91,96`).
- [source] Free-running sustain threshold `0.0032` (`Config.h:155`). Dynamic EGs re-evaluate every 16 samples (`:73-86`).
- [source] Velocity: no curve → `gain = v²`; `amp_veltrack` at 100 → `1−(1−v²)` = v² (`RegionStateful.cpp:118-141`). `amp_velcurve_N` replaces it with linear interpolation of points (`Curve.cpp:55-80`).
- [source] Predefined curves 0–6: linear, bipolar, inverted, concave `x²`, `sqrt`, `sqrt(1−x)` (`Curve.cpp:83-124`).
- [source] Pitch velocity tracking is linear in cents: `pitch += v·veltrack` (`RegionStateful.cpp:156-165`).

### 3.2 HISE
- [source] Gain intensity: `g = (1 − i) + i·x` (`hi_core/hi_dsp/modules/Modulators.cpp:217-219`). Pitch, pan, global and offset intensity: `i·x` (`:221-235`). Chain combine: gain multiplies, offset adds, pitch adds in normalised range (`ModulatorChain.cpp:1527-1620`).
- [source] Velocity modulator: linear by default, dB mode `−100 + 100·v` dB → gain (`hi_modules/modulators/mods/VelocityModulator.h:148-149`).
- [source] Simple envelope: attack linear in time, decay and release exponential with per-voice modulated time (`SimpleEnvelope.cpp:187-232`). Attack modulation via a voice-start chain (`:189-198`).
- [doc] (forum, Christoph Hart) scaling is "the best modulation mode for gain modulation, otherwise you would always have to turn the volume down" (`forum.hise.audio/topic/10945` post 104901). Doc page lists the scaled/unipolar/bipolar modes and says scaling applies before offsets (`docs.hise.audio/.../matrixmodulator.html`).
- [doc] Scriptnode AHDSR: Attack 0–10000 ms (default 10), AttackCurve 0–1 (0.5), Decay 0–10000 (300), Sustain 0–1 (0.5), Release 0–10000 (20). DecayCurve absent in the node, present in the stock modulator (conflict, reported).

### 3.3 SFZ (sfzformat.com)
- [doc] Naming: `ampeg_*`, `pitcheg_*`, `fileg_*` (SFZ 1); `egN_*` flex (SFZ 2). Modulation depth is set on the destination (`eg01_pitch=2400` = 2400 cents).
- [doc] `amp_veltrack` default 100 %, range ±100; LaTeX on the page: `20·log10((v/127)²)` dB at 100 (verified text in the LaTeX source). Velocity 127 → 0 dB.
- [doc] `amp_velcurve_N` interpolates linearly between defined points; default is the amp_veltrack curve; `amp_velcurve_127` defaults to 1 (sfzformat.com/opcodes/amp_velcurve_N).
- [doc] `curveccN` (0 default linear, built-in curve table 0–6) and `smoothccN` (ms, pitch and volume only): sfzformat.com/modulations/curveccN and smoothccN.
- [doc] `lfoN_freq` in Hz; LFOs are note-triggered, no free-running (sfzformat.com/modulations/lfo).
- [doc] SFZ 2 states "any number of each, with multiple targets" (tutorials/sfz2_modulations).

### 3.4 Decent Sampler
- [doc] `<modulators>` LFO: `delayTime` (default 0), `scope` (global default), `modAmount` 0–1 (default 1), `modBehavior` add | multiply | set (default set). Envelope: `attackCurve` −100 (log) … 100 (exp), default −100; `decayCurve` default 100; `sustain` 0–1.
- [doc] Binding: `translation` fixed_value | linear | table, linear scales to `translationOutputMin/Max` (no formula stated).
- [doc] `ampVelTrack` 0–1 linear: 127 → 1.0, 63 → 0.5 (groups page, 1.23.1 guide).

### 3.5 EastWest Opus, Ample, EZdrummer, MODO DRUM, Koda
- [doc] Opus: velocity is articulation ranges; velocity curves are on the controller; Auto Filter envelope attack 0.01–120 ms, depth ±100 %. No instrument envelope spec in the manual text.
- [doc] Ample Bass: velocity layer thresholds, humanization ±N velocity points. No curve or envelope modulation law.
- [doc] EZdrummer, MODO DRUM, Koda: no reachable velocity/volume law. Superior Drummer curve (not EZdrummer): breakpoint curve, upper limit 111.

---

## 4. KONTRA now

### 4.1 Model (what exists)
- `crates/model/ir/src/lib.rs:166-184`: `Mod { name, slot, kind }`; `ModKind::{Envelope, Volume { source, intensity, curve }}`. Two kinds only.
- `lib.rs:186-190`: `Source::{Velocity, Controller(u8)}`. No LFO, step, follower, glide, key, random, bend, aftertouch, release velocity, script event, macro.
- `lib.rs:193-216`: `Curve::{Decibels(range), Power}`. `Power` = `1 − d + d·x^e`, `d = min(1,2i)`, `e` from 1.285 at 50/127 (`lib.rs:209-222`). **Does not match the measured cube law** (§1.3, [measured] RMS 0.26 dB vs 0.027).
- `lib.rs:144-160`: `Envelope { attack, hold, decay, sustain, release }` in seconds. No curve, no shape, no delay, no ahd_only, no retrigger.
- `lib.rs:93-107`: `Group { envelope, mods, … }`; one envelope per group.
- `crates/engine/engine/src/plan.rs:92-106`: `Group::standard()` default velocity mod uses `Decibels(24.0)` (not what the translator emits).

### 4.2 Translator
- `crates/format/kontakt/src/translate.rs:190-212`: AHDSR on a volume target only; sets `out.envelope` (overwrites if several AHDSRs); `sustain = clamp(e).powi(3)`; times ms→s. `attack_curve` is dropped. Non-volume internal modulators are `Impact::Audible` diagnostics (`:213`).
- `translate.rs:218-238`: external modulators admitted only for velocity or MIDI CC on `volume` with no slot; all other external sources → `Impact::Audible` (`:237`). `curve: Power` is hard-coded (`:233`).

### 4.3 Engine
- `crates/engine/engine/src/lib.rs:748-785` `voice_gain`: product of crossfades × mod curves × expression factor; sources read per note (`:770`); CC source read from `behaviour.cc`.
- `lib.rs:629-657` `refresh_note`: re-computes gain on CC change; pitch step only on non-CC refresh (`step` update on `cc.is_none()`).
- `lib.rs:808-813` `gain`: linear balance pan `(1 ∓ pan).min(1)`, comment says equal-power/raised cosine not implemented.
- `crates/engine/voice/src/lib.rs:24,178-179`: gain one-pole smoothing 0.01/frame (~2 ms at 48 k). Pitch `set_step` jumps (`:95-99`), no smoothing.
- `crates/engine/voice/src/envelope.rs:5-13` `Ahdsr { attack, hold, decay, sustain, release }` in frames; `:89-127` `next()`: attack linear `(n+1)/attack`, hold count, decay exponential toward sustain via `coefficient(decay)` (60 dB over `decay` frames), release from current level, 60 dB over `release` frames.
- `crates/engine/engine/src/prepare.rs:256-264`: seconds → frames by `round(s·rate)`.
- `crates/engine/engine/src/params.rs:19-76`: `set_engine_par` (intensity on volume mods; sustain stored linear, no cube, `:59-63`; stage times via `stage_ms` shifted exponential 15 s/25 s `:11-14`, `:67-74`). Returns false for unknown par or group. No `_m`, no MP intensity, no invert, no target index. `EnginePars` in `script.rs:24-38`: attack, hold, decay, sustain, release, intensity.
- `crates/engine/engine/src/script.rs`: no modulation builtins beyond `set_engine_par` and `set_controller`.

### 4.4 Measured against the rest
- Velocity volume: [measured] §9. KONTRA `Power` is the outlier of the three candidate laws.
- Attack shape: linear, matches sfizz and HISE simple envelope; Kontakt has a curve, so the `attack_curve` drop is a mismatch for any library that uses it.
- Decay/release: exponential toward sustain (KONTRA, sfizz). Kontakt fall is not measured here.
- Sustain: Kontakt cube (translator), HISE linear, sfizz linear. The cube is the Kontakt law; keep it as a per-format law, not a global.

---

## 5. Gaps, ranked by audible impact

Impact: **B** = notes missing or wrong, **A** = audible level/timbre difference, **C** = cosmetic or script-only.

1. **B — Only two target families.** Pitch, pan, filter, sample start, effect params, LFO, step, follower, glide, key/CC-pitch: all `Impact::Audible` or absent (`translate.rs:213,237`). Libraries that route an LFO or pitch envelope lose it entirely. Ranked first because the gold model cannot translate them.
2. **A — Velocity volume law.** `Curve::Power` vs measured cube: −0.34 dB at v=64 (Beat Machine route), RMS 0.26 dB across the table. Systematic across all velocities, every volume-velocity route. Fix: `Curve::Cube` (or a data law) with per-route fitted intensity.
3. **A — AHDSR attack curve dropped.** `translate.rs:200-205` ignores `attack_curve`; the engine's attack is always linear. Affects every instrument with a non-zero curve (see §8 for library counts).
4. **A — Multiple envelopes per group.** `translate.rs:200` overwrites `out.envelope` with each AHDSR that targets volume, so only the last one survives; the earlier ones are neither applied nor reported. Affects any group with two volume AHDSRs.
5. **A — Stage changes from scripts.** `params.rs:73` writes frames once; mid-stage changes do not re-time the active stage (the native behaviour is also unproved, `kontakt-envelope-time-modulation.md`). Acceptable until a measurement shows it audible.
6. **A — Pitch step on CC refresh.** `voice/lib.rs:95-99` jumps pitch on each `set_step`; no smoothing. Matters once pitch targets exist.
7. **A — Pan law.** `lib.rs:808-813` linear balance. Kontakt pan law is not measured here; once pan is routed, it must be a data law.
8. **B — Script surface.** Only `set_engine_par` (intensity, five stage params). No `get_mod_idx`, no target index, no MP intensity, no `_disp`, no From Script values, no invert.
9. **C — `Group::standard` mod (`plan.rs:92-106`) uses `Decibels(24)`** which the translator never produces; dead path that can mislead tests.
10. **C — Kontakt AHDSR curve shape for decay/release** (`ln(3/43)` in the old IR) not represented.

---

## 6. Gold standard (format-neutral)

### 6.1 Data model
```
ModSource  = Const(f32)
           | Velocity{ law: LawId } | ReleaseVelocity | Key{ center, law } | KeyPosition
           | Cc{ n, default: f32, smooth_ms, curve: LawId }       // -1 default = unmodulated until first CC
           | PitchBend | ChannelPressure | PolyPressure | Mpe{ pressure, timbre, bend }
           | RandomUni | RandomBi                                  // per-note draw
           | ReleaseTriggerCounter{ t_ms }
           | Env(EnvId) | Lfo(LfoId) | Step(StepId) | Follower(FollowerId) | Glide(GlideId)
           | Script{ event_id }  | Macro(MacroId) | Opaque{ native_id }
Source def = { scope: Voice | Group | Instrument, kind: EnvDef | LfoDef | ... }
Route      = { source: SourceRef, target: TargetRef, depth: f32, law: LawId,
               mode: Scale | Offset | Bipolar | Pitch, invert: bool, shaper: Option<CurveId>,
               lag_ms: f32, scope: Voice | Group, enabled: bool }
Target     = { addr: (scope, node_id, param_id), unit: Unit, range, law: LawId, combine: Mode,
               clamp: Clamp, smooth_ms: f32, cadence: Control | Audio }
Law        = Linear | DbLinear{range} | Power{exp} | KontaktFamilyH{ intensity_ref, cube: bool }
           | Table(CurveId) | Exponential{ low, high } | ShiftedExponential{ top_ms }
```
- Laws are data. Format adapters only choose a `LawId` and parameters; the engine has one implementation per law (matches `docs/architecture-v2/MODULATION_V2.md` "law as data" rule).
- `Envelope = { delay, attack{time, curve}, hold, decay{time, shape}, sustain (law), release{time, shape}, ahd_only, retrigger, vel_amount, vel_sens }`. Shape is a `CurveId` (linear = identity). Time law is a `Law` (Kontakt: ms raw; script/knob: ShiftedExponential).
- `Lfo = { rate (Hz | beat), depth, phase, delay_ms, fade_in_ms, retrigger, shape: Waveform | Table, sync }`. Kontakt Multi and fade law are a shape variant with the `[0.3,1.3]` clamp data.

### 6.2 Evaluation
- Scope: a route belongs to a group (Kontakt, UVI) or a voice (sfz/Decent), declared in the route, not inferred.
- Cadence: control grid 64 frames for CC, LFO, envelope stage transitions, route evaluation; per-sample only for envelope internal stage and Filter FM (audio-rate target declared as `cadence = Audio`).
- Order per route: source value → invert → shaper → law → depth → combine into target (`MODULATION_V2.md` order) → lag (one-pole; coefficient from `laws.rs:362`).
- Combine:
  - `Scale`: `base · law(x)` (HISE gain, Kontakt H family).
  - `Offset`: `base + depth·law(x)` (HISE pitch/offset, SFZ pitch in cents).
  - `Bipolar`: `base + depth·(2x−1)` (HISE bipolar).
- Volume laws: Kontakt velocity uses `KontaktFamilyH{ cube: true }`; SFZ amp_veltrack uses `Power{exp: 2}` at 100 %; HISE uses `Scale` with `(1−i)+i·x`; Decent uses `Linear`.

### 6.3 Real-time rules
- No allocation in render; envelopes and LFO state are SoA sized at prepare (as in `voice_mod.rs`).
- Note-on: sources sampled once for velocity/key/random, envelopes start on the note's sample.
- CC changes: re-evaluate only routes whose source is that CC; smooth gain (2 ms), smooth pitch with 2 ms before audio.
- Retrigger/legato per envelope/LFO flag, not per format.

### 6.4 Scripts: how each dialect addresses modulation
- KSP: `get_mod_idx(group, name)` → slot; `get_target_idx(group, slot, target)` → target; `set_engine_par(par, value, group, slot, generic)` writes into the same `ModRef` table. The translator provides `(group, slot)`; the engine answers `false` for unknown refs (matches `params.rs:29-37`).
- UVI Lua: `Element:setParameter(name, value)` on the modulator element → same `ModRef` by name; `sendScriptModulation(id, target, ramp_ms, voice_id?)` → a `Script{event_id}` source with `ramp_ms` ramp.
- HISE: modulator chain id → `ModRef`; intensity uses the `Scale`/`Offset` modes above.
- SFZ: opcodes `ampeg_*`, `eg01_*`, `lfo01_*`, `*_oncc*`, `*_curvecc*`, `*_smoothcc*` → routes with `Cc` source and `curve`/`smooth_ms` taken from the opcode; no script surface.
- Decent Sampler: `<binding type="modulator">` → route with `translation` as `Linear` or `Table` law.
- Every dialect ends in the same `Route` list; dialect-specific names live only in the translator's diagnostic table.

### 6.5 Kontakt-specific laws kept as data
- Family H cube (`KontaktFamilyH{ cube: true }` for external types 7–13); per-route intensity fit (§1.3) stored in the route, not in the engine.
- AHDSR sustain cube, attack shape (`attack_curve`), ms storage.
- Invert, shaper and lag as in §6.2.

---

## 7. What is still open (needs a measurement, not an opinion)

- Full-intensity Kontakt volume law (cube vs linear at i=1): one oracle render at velocity 1, 32, 64, 127.
- Kontakt default velocity→volume intensity for a new instrument (`KONTAKT_DSP_LAWS.md:437`, silent in KM).
- AHDSR attack-curve formula and decay/release shape in Kontakt (we have a log-ratio attack law in the old code, not measured).
- LFO and envelope scope (per voice vs per group) in Kontakt: the manual says only "react to MIDI note messages".
- KSP `MOD_TARGET_MP_INTENSITY` out-of-range clamping (manual example conflicts with 0–1000000).
- UVI `sendScriptModulation` audio effect (repo notes conflict).

---

## 8. Library evidence

Method: KSP text from the repo's decompiled corpus (`/data/dev/workspace/scratch/ksp-corpus-696/`, 696 slots, 658 mapped to NKI files) [source]. NKI, UFS, Ample `.cf/.ascf` and MODO `.pak` are opaque here, so NKI-side envelope values, LFO settings and zone velocity ranges are **not** measured. Counts are slots (script files), not instruments. Scratch: `/data/dev/scratch-modulation/`.

### 8.1 Behaviour → library (Kontakt, KSP corpus)

| Behaviour | Libraries (slot count) | Example [source] |
|---|---|---|
| AHDSR addressed by mod name via `find_mod(g,"ENV_AHDSR")` | Afflatus 348, Areia 155, Dolce 77, CHORUS 35, Pacific 49, Vista 1, Morphology 1, Shreddage 1 | `2_Horns_AccSusCresc-0.ksp:803-805` |
| ATTACK / DECAY / SUSTAIN / RELEASE written by `set_engine_par` | Afflatus 348 (all four), Areia 155 (all four), Dolce 77, CHORUS 35, Pacific (DECAY 45, ATTACK 20, RELEASE 4, no SUSTAIN), Una Corda (RELEASE 3, ATTACK 6) | `2_Horns_AccSusCresc-0.ksp:803-805` |
| **ATK_CURVE read or written** | Afflatus 153, Areia 155, Dolce 77, CHORUS 35, Pacific 12, Vista 1, ANALOG 1 (table) | attack curve is used in **≈ 480 slots** → gap 3 is audible across the corpus |
| HOLD written | Areia 152, Dolce 77, Shreddage 1 | — |
| Direct `$ENGINE_PAR_MOD_TARGET_INTENSITY` write | Afflatus 348, Areia 155, Dolce 77, CHORUS 39, Pacific 41, Vista 7, Conflux 1, Morphology 1, Shreddage 1 | `Pacific_-_Ens_Strings_-_12_Violas_-_Legato_Sustains-2.ksp:1668` |
| Niente / Dyn. Range intensity with complementary velocity pair (`(1−x)·…` / `x·…`) | Areia 155, Dolce 77, CHORUS 39 | `Areia_-_01_4_Double_Basses_-_Legato_Fingered-0.ksp:12196-12203`, `:22211-22216` |
| BALANCE_* mod names (CC vs velocity dynamic range) | Pacific 41, Vista 7 | `Pacific_-_Ens_Strings_-_12_Violas_-_Legato_Sustains-2.ksp:1668` |
| Velocity→volume as a **scripted per-note dB offset** (not the engine volume mod) | Afflatus 348 (formula), 254 `change_vol` calls; Una Corda 12; Pacific 5; ANALOG 3; Conflux 1; Shreddage 1 | `2_Horns_AccSusCresc-0.ksp:1945`: `$poly_char_volume := ($vol_amount * 1000 * ($poly_velocity - 64)) / 64` (millidB, linear in dB) |
| Dynamic-layer velocity thresholds set by UI | Areia 152, Dolce 77 | `Areia_-_01_4_Double_Basses_-_Legato_Fingered-0.ksp:6641` |
| Per-note CC snapshot `set_event_par_arr(…,$EVENT_PAR_MOD_VALUE_ID,…)` | Areia 153, Dolce 77, CHORUS 39, Pacific 41, Vista 6, Una Corda 9, Conflux 2, Shreddage 1 | `01_Areia_-_16_Violins_-_Core_Techniques-0.ksp:22150` |
| `get_event_par($EVENT_PAR_VELOCITY)` | Areia 152, Dolce 77, CHORUS 39, Conflux 1 | `01_Areia_-_16_Violins_-_Core_Techniques-0.ksp:101162` |
| Sustain pedal `$CC_NUM=64` check | Areia 154, Dolce 77, CHORUS 39, Una Corda 3 | `01_Areia_-_16_Violins_-_Core_Techniques-0.ksp:101221` |
| `random()` | Areia 154, Dolce 77, CHORUS 39, Una Corda 6, ANALOG 3, Conflux 2, Morphology 1, Shreddage 2 | `01_Areia_-_16_Violins_-_Core_Techniques-0.ksp:22237`-area; `Areia…Legato_Fingered-0.ksp:54496` |
| Round robin state (`rr_mode`, `rr_counter`) | Areia 152, Dolce 77, CHORUS 39, Shreddage 1; Afflatus counter packed into velocity event word, no `random()` | `2_Horns_Stopped_Tenuto-0.ksp:1845`, `Dolce_-_01_3_Basses_-_Legato-2.ksp:830` |
| MIDI-learn remap to CC1 / CC11 | Afflatus 348, Areia 154, Dolce 77, CHORUS 39, Pacific 29, Vista 6, Una Corda 3 | `2_Horns_AccSusCresc-0.ksp:1810-1830` |
| `%CC[VCC_PITCH_BEND]` / `VCC_MONO_AT` reads | Una Corda 3/3, Conflux 1/1, Shreddage 1/1 | `Conflux-2.ksp:5243-5244` |
| LFO shape weights written (`LFO_SINE/TRI/SAW/RECT/RAND`, `LFO_MULTI_1..4`) | Conflux 3 slots, Morphology 1 (43 refs) | `Conflux-2.ksp:2901-2912`; `Morphology_Evolved-0.ksp:11381-11418` |
| LFO rate / delay / fade written by script | **none** (0 slots in all 696) | `ANALOG_STRINGS-2.ksp:513-515` only as a parameter table |
| Release-trigger flag read | ANALOG 2, Conflux 1 | `ANALOG_STRINGS-3.ksp:177-186` |
| `play_note` inside `on release` | Afflatus 119 lines, Pacific 71, Una Corda 12, Vista 7 | — |
| DBD, Flex, envelope follower, glide, step, MPE / `set_note_controller` | **0 slots each** | — |

### 8.2 Per-library profile (what each library relies on)

- **Afflatus Brass (348):** AHDSR per group with ATTACK, DECAY, SUSTAIN, RELEASE, ATK_CURVE. Velocity→volume is a scripted dB offset from a 0–6 "VDI" slider. CC1/CC11 remapped by MIDI-learn. Round robin counter packed into velocity. No `random()`, no LFO, no BALANCE, no HOLD.
- **Areia (155):** full AHDSR set incl. HOLD and ATK_CURVE; Niente / Dyn. Range complementary pairs; dynamic-layer thresholds; per-note CC snapshot; `rr_mode` and `random()`; sample-start mod; heavy `wait()` (9,296 lines). No LFO, no pitch bend.
- **Dolce (77), CHORUS (39):** same family as Areia without Sample Start.
- **Pacific Ensemble Strings (49):** DECAY and ATTACK per group, RELEASE in 4, no SUSTAIN/HOLD. BALANCE_* mod names. Per-note CC snapshot. `play_note` in on-release code. No round robin or `random()`.
- **Performance Samples Vista (7):** BALANCE_* and MOD_TARGET writes; per-note snapshot; no ATTACK, no RR, no `random()`.
- **Una Corda (3 NKI / 12 slots):** mechanical-noise controls via `set_controller`; `random()` for the player trigger; pitch-bend and aftertouch reads. No `find_mod`, no mod-matrix writes.
- **ANALOG STRINGS (1 NKI / 3 slots):** release-trigger flag, parameter tables with LFO_P1/P2, Phase, Loop, Sample_Start. No `find_mod`, no direct MOD_TARGET writes. Snapshots (`.nksn`, binary) carry `VEL_VOLUME`, `VEL_CUTOFF`, `ENV_AHDSR_VOLUME`, `ENV_AHDSR_PITCH`, `LFO_MULTI_1`, `PB_PITCH` mod-slot names [binary], so pitch and velocity routes exist in the NKI data but are not written by script.
- **Conflux (1 NKI / 3 slots):** script-built mod matrix (`LFO 2 -> LFO 1 AMT`, `MOD ENV -> LFO 1 AMT`, `MW -> LFO 1 AMT`, `KT -> CUTOFF SV/LDR`, `Conflux-2.ksp:281-306, :366`); LFO shape weights; pitch bend and aftertouch via `%CC`. No per-note MPE.
- **Morphology Evolved (1 slot):** four `LFO_MULTI_1..4` modulators with five shape weights each, written per source group (`Morphology_Evolved-0.ksp:11381-11418`); `ENV_AHDSR_1..3`; `VEL_VOLUME`; heavy `wait()` (1,129 lines).
- **Shreddage E-Upright (1 NKI / 2 slots):** AHDSR via `find_mod`, ATTACK write, `INTMOD_BYPASS` (`Shreddage_3.5_E-Upright-0.ksp:4726-4727`), `VIBRATO` and `PB -> PITCH` mod names.
- **UVI VWinds Clarinets / Double Reeds / Flutes [doc]:** vibrato source CC1 by default, CC2 for breath, optional pitch bend; velocity drives air-flow ("VEL", `uvi-clar.txt:196`), attack and sustain (`:322-333`); velocity-triggered glide (`:340-345`); MPE disables advanced pitch bend (`:552-555`); round-robin modes (`:579-581`). Mention counts across the three PDFs: velocity 13/14/15, vibrato 26/24/22, glide 10/10/10, CC2 6/6/8, MPE 5/5/5, round robin 3/3/3, LFO 0, CC11 0, CC64 0. The `.ufs` binaries carry no readable parameter names [binary]. No `.lua` file exists under `/data/main/Libraries/UVI`.
- **UVI Augmented Orchestra (18.4 GB `.ufs`):** not scanned.
- **Ample (`.ascf`, `.cf`):** no readable modulation keys [binary]; not recoverable without a decoder.
- **MODO DRUM (`.pak`, index only) [binary]:** velocity layers and round robin are baked into filenames (`_v<vel>_rr<n>`; 18–21 velocity layers, RR up to 23). No LFO, envelope, CC or mod-matrix keys. Modulation is not used.
- **EZDrummer (SD2 SL-Avatar text configs) [source]:** pad fade and `fadetime`; `aftertouchrelay` (216 in the compound preset) with an `AftertouchMute` template (`GM-padconf:68`); `attack`, `sustain`, `decay`; velocity-to-volume flag `velvol` (2 / 6); `cycle` and `alternate` keys (round-robin style); layer limit `laylim`. No LFO, CC or `random` keys in mic/mix/uber configs.

### 8.3 What this changes in the spec

- **ATK_CURVE is used across ≈ 480 slots** (Afflatus, Areia, Dolce, CHORUS, Pacific, Vista, ANALOG). Gap 3 is not a rare case; the attack-curve law (§6.5) is a day-one requirement.
- **Velocity-to-volume is scripted in Afflatus** (millidB offset, linear in dB) and is also an engine-side mod in others. The engine must not assume one place; the script path needs `change_vol`-style gain (`ENGINE_PAR_VOLUME`) at note level.
- **`find_mod` is the working API in shipped scripts** (~891 k call sites across the library corpus per the survey; Areia, Afflatus, Morphology, Shreddage), although the KSP manual documents `get_mod_idx` and deprecates `find_mod` (KSP p.380). §1.7 is corrected below: `find_mod` must be supported as an alias of `get_mod_idx`.
- **LFO rate/delay/fade are never written by script** in the corpus; the LFO route that matters is the shape-weights path (Conflux, Morphology), plus NKI-side routes that are not measured here.
- **Per-note CC snapshot events** (`$EVENT_PAR_MOD_VALUE_ID`) appear in Areia, Dolce, CHORUS, Pacific and Vista (≈ 350 slots); the event-keyed route in §6.4 is needed for these libraries.
- **No DBD, Flex, follower, glide, step or MPE** in any corpus slot: those laws are needed for correctness of the model but not for corpus coverage yet.

Caveats: the repo's KSP dump is itself a decompile and was not checked against raw NKI bytes; the `wait()` count does not show what is ramped.

---

## 9. Tests that prove each claim

| Claim | Test | Status |
|---|---|---|
| Cube law fits the Kontakt velocity table (§1.3) | `vel_volume_fit`: least-squares i for `(1−(1−x)i)³` vs `(1−(1−x)i)`, `Curve::Power`, against the 10-point table and the 3 Beat Machine points; print RMS and per-point error | run in this study by hand (numbers in §1.3); commit as a unit test in `crates/engine/engine` when the law is added |
| KONTRA `Power` error (§4.1) | same fit against `Curve::Power`, expect RMS 0.26 dB and −0.34 dB at v=64 | done (same run) |
| sfizz amp velocity default is v² (§3.1) | sfizz unit test: region with no `amp_veltrack`, velocity 0.5 → gain 0.25 | to run |
| HISE gain intensity `(1−i)+i·x` (§3.2) | HISE `ModulatorChain` gain test; or read code path, §3.2 | read-only, verified by source |
| Attack curve dropped (§4.2) | translator test: AHDSR with nonzero `attack_curve` → `ir::Envelope` has no curve; expect failure once curve is added | to write |
| Second AHDSR dropped (§5.4) | translator test with two volume AHDSRs → one envelope and one diagnostic | to write |
| Pitch/LFO routes lost (§5.1) | translator test: LFO → pitch → `Impact::Audible` diagnostic | to write |
| `set_engine_par` returns false for unknown par/group (§4.3) | unit test on `params.rs` | to write |
| Stage time frames rounding (§4.3) | `stage_ms` test exists (`params.rs:110-115`) | exists |
| Pitch CC smoothing (§5.6) | voice test: step change on CC refresh produces a click at 1 sample | to write when pitch targets land |

Measurement rule: each row in the gap table must be closed by the test above before the row is marked fixed; a test that passes on the current code (e.g. the envelope frame rounding) proves only the current behaviour, not the target law.
