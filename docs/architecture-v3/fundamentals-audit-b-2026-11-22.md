<!-- Verbatim copy of Core Multisampler Fundamentals Audit Part B (Gemini, 2026-11-22). Its fixes landed in #94 (Part A) and #95 (Part B); 13-IR-VNEXT.md tracks what remains. -->

# Core Multisampler Fundamentals Audit — Part B
**Scope:** Modulation Matrix, External/Internal Sources, Shapers & Lag, Gain/Pan/Crossfade Staging, Silent Drops, & Architecture  
**Checkout audited:** `/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra` (`origin/main`, read-only)  
**Ground truth:** Kontakt 8.13.1 decompiled C (`/data/dev/workspace/Research/KONTRA/source/kontakt8/*.c`) & extracted `.rdata` ([constants.tsv](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/docs/re/kontakt-rdata/constants.tsv))  
**Excluded:** `BZEnvMod` (`BParExternalMod` category `2`), already in progress.

---

## Executive Summary

KONTRA v3's engine foundation (zero-alloc voice playback, inline `SmallVec<[Loop; 8]>`, clean `kontra-ir` / `kontra-law` separation, preallocated bus/chain architecture) is strong, **but the modulation layer has severe, widespread correctness bugs** that break core sampler behavior across almost every non-trivial Kontakt instrument:

1. **Inverted/negative modulation depth on Volume & Intensity routes explodes into a massive boost (+18 dB on Volume, 2× on Intensity)** instead of inverting (`1 - x`).
2. **`KeyPosition` is evaluated as unipolar `key / 127.0` instead of Kontakt's C3-centered bipolar octaves `(key - 60) / 12`** (and loses `frange_bi` when shaped), breaking piano pan spreads, filter key-tracking, and pitch key-tracking.
3. **AHDSR envelope-time modulation (`+0x53d` `simulateK2EnvTimeModulation`) has both branches inverted and wrong** (>10× error on every modern Kontakt 3–8 library; 100× error on K2-compat patches), and drops `ahdsr_hold`/`ahdsr_decay`.
4. **External modulator `bypass` (`+0x53c`) is never read** (`bypass: false` hardcoded), causing bypassed external modulators to run live.
5. **Muted groups have all their zones silently dropped at load time**, and **`soloed` groups are dismissed as `COSMETIC_GROUP`**.
6. **`BParInternalMod` parser hard-errors on Step Modulators (cat 3) and Envelope Followers (cat 4)**, and **`BParExternalMod` hard-errors on source codes `0` and `13..=18`**, crashing instrument load.
7. **First volume AHDSR silently drops all non-volume targets, ignores `bypass`, and ignores `noteOffLessMode` (AHD-only)**; **all internal modulators silently drop their `ModShaper` and `lag_ms`**.

---

## P0: Audible / Broken Core Fundamentals

### P0-1. Negative modulation depth (`unknown_flags & 2`) on Volume & Intensity causes massive gain/depth BOOST (+18 dB / 2×) instead of inversion
- **KONTRA location:**
  - [modulation.rs:73-79](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/vendor/ni-file/src/kontakt/objects/modulation.rs#L73-L79) (`ModTarget::signed_intensity()` returns `-intensity` when `unknown_flags & 2 != 0`)
  - [external.rs:97-102](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/external.rs#L97-L102), [external.rs:305-309](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/external.rs#L305-L309), [timed.rs:131-137](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/timed.rs#L131-L137) (pass signed negative `intensity` / `depth` straight into `ModKind::Volume` and `ModKind::Intensity`)
  - [mods.rs:288-290](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/mods.rs#L288-L290):
    ```rust
    Self::Depth { power } => (1.0 - (1.0 - x) * intensity).max(0.0).powi(*power),
    ```
  - [voice_gain.rs:435-436](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs#L435-L436):
    ```rust
    let value = 1.0 - (1.0 - input) * depth;
    ```
- **Ground-truth evidence:**
  - [0140920000.c:6479-6485](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140920000.c#L6479-L6485) & [0140970000.c:5137-5138](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140970000.c#L5137-L5138): Kontakt stores `$intensity >= 0` unsigned at `+0x10` and uses flag bit `0x02` at `+0xd` to invert the signal via `fcinv_flip(s) = 1 - s` (unipolar) or `fcinv_neg(s) = -s` (bipolar) in [0140870000.c:4075-4142](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140870000.c#L4075-L4142). *(Note: `ni-file`'s `ModTarget.invert` at `+0x16` is actually `"tableOpen"`, a UI drawer flag — verified at [0140920000.c:7098-7111](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140920000.c#L7098-L7111).)*
  - For Volume/Intensity (`family H`, [0140870000.c:4325-4352](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140870000.c#L4325-L4352)), inverted depth computes `fcubic(1 - (1 - (1 - x)) * |i|) = (1 - x * |i|)^p` (descending from `1.0` at `x=0` to `0.0` at `x=1`).
  - KONTRA instead evaluates `(1 - (1 - x) * (-|i|))^p = (1 + (1 - x)*|i|)^p`: at `x = 0, intensity = -1`, **`Curve::Depth { power: 3 }` outputs `2.0^3 = 8.0` (+18.06 dB gain boost!)**, `power: 1` outputs `2.0` (+6 dB), and `ModKind::Intensity` outputs `2.0` (2× depth when mod wheel is at 0). At `x = 0.5, i = -1, p = 3`, KONTRA gives `+10.57 dB` vs Kontakt's `-18.06 dB` (**28.6 dB error**).
  - Note also [translate.rs:819](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/translate.rs#L819): `envelope_depth` receives `t.signed_intensity()`, so a negative amp-envelope route sets `floor = 1 - (-i) = 1 + |i| > 1.0`.
- **Impact:** Breaks **every inverted volume crossfade** (e.g. two layers crossfaded via CC1 `+100%` and `-100%`, inverted velocity crossfades, inverted tremolo, inverted env→vol) and **every inverted CC→LFO/Env intensity route** (vibrato/mod-depth fading out as CC rises).
- **KISS/YAGNI Fix:** In `Curve::Depth { power }` ([mods.rs:288](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/mods.rs#L288)) and `Engine::scale` ([voice_gain.rs:436](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs#L436)), apply the exact same inversion rule `time_route` already uses at [voice_gain.rs:681-682](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs#L681-L682):
  ```rust
  let x = if intensity < 0.0 { 1.0 - x } else { x };
  (1.0 - (1.0 - x) * intensity.abs()).max(0.0).powi(*power)
  ```
  (This also subsumes `Curve::Falling` as `Curve::Depth { power: 1 }` with negative intensity, simplifying the IR!)

---

### P0-2. `KeyPosition` source uses unipolar `key / 127.0` instead of Kontakt's C3-centered bipolar octaves `(key - 60) / 12`, and drops `frange_bi` when shaped
- **KONTRA location:**
  - [external.rs:55](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/external.rs#L55): `ModSource::KeyPosition => Some(ir::Source::Key)`
  - [voice_gain.rs:320](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs#L320): `ir::Source::Key => Some(f32::from(note.key) / 127.0)`
  - [external.rs:75-77](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/external.rs#L75-L77): `let bipolar = [ir::Source::PitchBend, ir::Source::Random { bipolar: true }];` — omits `ir::Source::Key`!
- **Ground-truth evidence:**
  - [0140840000.c:9341](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140840000.c#L9341): `case 7:` (`KeyPosition`) computes `fVar23 = ((float)midi_key - DAT_144714a2c) * DAT_144737724;`.
  - [constants.tsv](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/docs/re/kontakt-rdata/constants.tsv): `144714a2c = 60.0f` (C3), `144737724 = 0.0833333358f` (`1.0 / 12.0`). Thus unshaped `KeyPosition` is **octaves relative to C3** (`-5.0` at C-2, `0.0` at C3, `+5.583` at G8).
  - [0140870000.c:4041](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140870000.c#L4041): When shaped (`local_5b6 == true`), Kontakt emits `"frange_bi(ftable($c, (($signal * 12.0) + 60.0) / 127.0))"` — converting back to `key / 127.0` to index the shaper, then applying `frange_bi` (`2*y - 1`) to restore bipolar output!
- **Impact:**
  - **Filter key-tracking (`KeyPosition -> filterCutoff`, 273× in corpus):** C3 shifts cutoff up by `+0.472 * depth` instead of `0`, notes below C3 never track downward, and the slope is `1/127` instead of `1/12` (off by ~10.6×).
  - **Keyboard stereo spread (`KeyPosition -> pan`, standard in pianos/harps/mallets):** C3 pans 47% right instead of center, and bass keys never pan left.
  - **Key-to-pitch (`KeyPosition -> pitch`):** C3 detunes by `+5.67 st` at 100% instead of `0 st`.
  - **Shaped `KeyPosition` on signed targets (`pan`, `pitch`, `filterCutoff`):** stays `[0, 1]` instead of `[-1, +1]`.
- **KISS/YAGNI Fix:** Keep `ir::Source::Key` neutral (`key / 127.0`, which UVI/SFZ use directly). In `crates/format/kontakt/src/external.rs`:
  1. For unshaped `KeyPosition` on signed targets, emit a 2-point linear `ir::Transfer` on `ir::Source::Key` mapping `0..=1` → `-5.0..=(67.0/12.0)` (zero new engine variants, zero RT cost).
  2. For shaped `KeyPosition`, add `ir::Source::Key` to `bipolar` at [external.rs:75](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/external.rs#L75) so `signed_route` emits `2*y - 1`. (And verify unshaped `KeyPosition -> volume` via `frange_uni` or clamp `[0, 1]`.)

---

### P0-3. AHDSR envelope-time modulation (`+0x53d`) has BOTH modes inverted and wrong, and drops `ahdsr_hold` & `ahdsr_decay`
- **KONTRA location:**
  - [laws.rs:135-144](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/laws.rs#L135-L144): `time_floor_ms` returns `2.0` when `+0x53d != 0`, and `0.0` when `+0x53d == 0`.
  - [voice_gain.rs:680-690](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs#L680-L690): `time_route` computes `ms * m` when `floor_ms == 0`, and `2 * ((ms + 2)/2)^(m^3) - 2` when `floor_ms > 0`.
  - [external.rs:166-211](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/external.rs#L166-L211): Only matches `"ahdsr_attack" | "ahdsr_release"` on the amp envelope (and only `"ahdsr_attack"` on non-amp AHDSRs); drops `"ahdsr_hold"` (`0xb9`) and `"ahdsr_decay"` (`0xba`) and non-amp `"ahdsr_release"`/`"ahdsr_hold"`/`"ahdsr_decay"`.
- **Ground-truth evidence:**
  - [0140920000.c:1719-1731](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140920000.c#L1719-L1731): `+0x53d` is `"simulateK2EnvTimeModulation"`.
  - [0140870000.c:4306-4322](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140870000.c#L4306-L4322) & [kontakt-envelope-time-modulation.md:91-146](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/docs/re/kontakt-envelope-time-modulation.md#L91-L146): `$in` is **already the normalized knob `u`**, NOT milliseconds!
    - **`+0x53d == 0` (modern Kontakt 3–8 — virtually EVERY modern library):** Kontakt omits `fconv_*` and emits `$out := $in * (1 - (1 - E)*i)` — scaling the **knob `u` by `m` (power 1)** → `ms' = 2 * ((ms + 2)/2)^m - 2`. KONTRA instead scales ms linearly (`ms * m`). Example: `1000 ms` attack at `m = 0.5` → Kontakt gives **`42.8 ms`**, KONTRA gives **`500 ms`** (>11× too slow!).
    - **`+0x53d != 0` (`simulateK2EnvTimeModulation = yes`):** Kontakt emits `fconv_n2u_env_a(fconv_u2n_env_a($in) * fcubic(1 - (1 - E)*i))` — scaling **time in ms by `m^3`** → `ms' = ms * m^3`. KONTRA instead scales the knob by `m^3`! Example: `1000 ms` at `m = 0.5` → Kontakt gives **`125 ms`**, KONTRA gives **`1.25 ms`** (**100× too fast!**).
- **Impact:** Every modern Kontakt library using velocity→attack, CC→attack, or key/velocity→release (strings, brass, synths, percussion) has drastically wrong envelope times (an order of magnitude off), and every velocity/key→decay or hold route is silently dropped.
- **KISS/YAGNI Fix:** Replace `floor_ms: f32` on envelope-time routes with a neutral `TimeScale` enum (`Linear { power: u8 }` vs `LogKnob { floor_ms: f32, power: u8 }`) and extend `ModKind` to cover all four AHDSR stages (`Attack | Hold | Decay | Release`) uniformly across amp and non-amp envelopes. For Kontakt: `+0x53d == 0` → `LogKnob { floor_ms: 2.0, power: 1 }`; `+0x53d != 0` → `Linear { power: 3 }`.

---

### P0-4. External modulator `bypass` flag (`+0x53c`) is never read — bypassed external modulators execute as active
- **KONTRA location:**
  - [modulation.rs:198,248](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/vendor/ni-file/src/kontakt/objects/modulation.rs#L198) bundles `+0x53c` and `+0x53d` into opaque `unknown_source_data` (`[cc_lo, cc_hi, bypass, k2_env]` for cat 1; `[bypass, k2_env]` for cat 2).
  - [external.rs:273](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/external.rs#L273) hardcodes `bypass: false`.
- **Ground-truth evidence:**
  - [0140d00000.c:7455](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140d00000.c#L7455) (`FUN_140d09620`) reads `+0x53c` from the binary stream; [0140920000.c:1698-1710](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140920000.c#L1698-L1710) names `+0x53c` `"bypass"`.
  - [0140970000.c:5107-5109](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140970000.c#L5107-L5109) copies `*(extmod + 0x53c)` directly into the runtime modulator's active bypass byte `+0xc` — identical to internal modulators at [0140970000.c:4983,5034,5052,5075](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140970000.c#L4983).
- **Impact:** Any bypassed external modulator (disabled CC/velocity/pitchbend/keytrack/script route in a patch or toggled off by default in a snapshot) runs live at full intensity in KONTRA, corrupting gain, pitch, filter, or pan.
- **KISS/YAGNI Fix:** Decode `bypass: bool` (`+0x53c`) and `simulate_k2_env_time: bool` (`+0x53d`) explicitly in `ni-file`'s `ExternalModParams`, and pass `bypass: p.bypass` at [external.rs:273](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/external.rs#L273).

---

### P0-5. Muted groups have all zones silently deleted at load time, and `soloed` groups are ignored as cosmetic
- **KONTRA location:**
  - [translate.rs:340](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/translate.rs#L340):
    ```rust
    if !z.sample_present || muted.get(group as usize).copied().unwrap_or(true) {
        continue;
    }
    ```
  - [translate.rs:63](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/translate.rs#L63): `const COSMETIC_GROUP: &[&str] = &["soloed"];`
- **Ground-truth evidence:**
  - Directly violates [VISION.md:113](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/docs/architecture-v3/VISION.md#L113) (*"Anti-patterns: Silent drops. Muted groups removed"*) and [KONTAKT_TOPOLOGY.md:254](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/docs/architecture-v2/KONTAKT_TOPOLOGY.md#L254) (T15).
  - In Kontakt, `muted` and `soloed` are live mix/eligibility flags (and if any group is `soloed`, all non-soloed groups are silenced). Dropping zones at load time makes unmuting a group impossible without reloading, breaks `get_zone_id`/`find_zone` counts, and plays non-soloed groups in patches saved with a soloed group.
- **KISS/YAGNI Fix:** Keep all zones of muted groups in the IR; carry `muted: bool` (and resolve `soloed` into initial mute/enable state or an explicit `Group::enabled`/`muted` flag) in `ir::Group`, and gate note admission in `start.rs` when muted.

---

### P0-6. First volume AHDSR silently drops all secondary targets, ignores `bypass`, and ignores `noteOffLessMode` (AHD-only)
- **KONTRA location:**
  - [translate.rs:809-835](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/translate.rs#L809-L835): When an AHDSR has a `"volume"` target, it sets `out.envelope`, emits ONLY `ir::ModKind::Envelope` with `bypass: false`, and **never loops over the remaining entries of `p.targets`** (nor emits any diagnostic for dropped targets!).
  - [laws.rs:20-34](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/laws.rs#L20-L34) & [laws.rs:212-238](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/laws.rs#L212-L238) never read `EnvelopeAhdsr.unknown_flag`.
- **Ground-truth evidence:**
  - [0140920000.c:993-1005](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140920000.c#L993-L1005) & [0140960000.c:2667](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140960000.c#L2667): `EnvelopeAhdsr.unknown_flag` (`BParEnv + 0x110` / `BParInternalMod + 0x708`) is **`"noteOffLessMode"`** (Kontakt's **AHD-Only** one-shot mode, ignoring note-off and ending after Decay).
  - [0140970000.c:4983,5034](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140970000.c#L4983): `unknown_flags[1]` (`+0x540`) is `bypass` on every internal modulator, including the amp AHDSR.
  - A single Kontakt AHDSR commonly targets both `volume` and `filterCutoff` or `pitch` simultaneously.
- **Impact:**
  - Any instrument sharing one AHDSR across `volume` + `filterCutoff`/`pitch`/`pan` silently loses the filter/pitch/pan envelope with **zero diagnostic**.
  - Any percussive/plucked/one-shot instrument using **AHD-Only (`noteOffLessMode`)** gets abruptly cut off on short MIDI note-offs instead of decaying naturally.
  - A bypassed amp AHDSR still shapes voice amplitude.
- **KISS/YAGNI Fix:** Add `one_shot: bool` to `ir::Envelope` & `Ahdsr` (skipping Sustain/Release on note-off); wire `bypass` from `p.unknown_flags[1]` on `ModKind::Envelope`; and if an amp AHDSR has additional targets beyond `"volume"`, also route them through `timed()` (or reference the amp envelope source).

---

### P0-7. `ni-file` hard-errors (aborts instrument load) on valid Kontakt modulators: Step Modulator (cat 3), Envelope Follower (cat 4), and External source codes `0` & `13..=18`
- **KONTRA location:**
  - [internal_mod.rs:20,81](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/vendor/ni-file/src/kontakt/objects/internal_mod.rs#L20): `const CATEGORIES: [u32; 3] = [1, 2, 5];` returns `Err("Unexpected internal modulator category ...")` on categories `3` and `4` — making `0x0A => Modulator::Follower(...)` at line 117 **completely unreachable dead code**!
  - [modulation.rs:239-247,290-310](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/vendor/ni-file/src/kontakt/objects/modulation.rs#L239-L247): rejects category `0` and source codes `0` and `13..=18` with hard `Err(...)`.
- **Ground-truth evidence:**
  - [0140970000.c:4980-5086](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140970000.c#L4980-L5086): All 5 internal categories are valid (`1` LFO, `2` Envelope, `3` Step Modulator `BParArp`/`0x09`, `4` Envelope Follower `BParEnvF`/`0x0A`, `5` Glide `0x0B`).
  - [0140cf0000.c:154-176](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140cf0000.c#L154-L176) (`FUN_140cf0360`) accepts external categories `0, 1, 2`.
  - [0140cf0000.c:208-278](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140cf0000.c#L208-L278) (`FUN_140cf0420`) accepts external source codes `0..=0x12` (`18`), mapping `13..=18` to internal polyphonic/per-note expression sources `0x10..=0x15` ([0140840000.c:9358-9363](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140840000.c#L9358-L9363), alongside PolyAT `2`).
- **Impact:** Any Kontakt preset containing a Step Modulator, Envelope Follower, unassigned category-0 external slot, or modern per-note/MPE external modulator **fails to load completely** instead of translating or diagnosing cleanly.
- **KISS/YAGNI Fix:** Accept categories `1..=5` in `internal_mod.rs` (decoding `0x09` StepMod and letting `0x0A` EnvFollower reach line 117), and accept category `0` and source codes `0` and `13..=18` in `modulation.rs` as typed `ModSource` variants (translating StepMod into KONTRA's existing stepped `ir::Lfo` + `ir::Transfer` table!).

---

### P0-8. Internal modulators silently drop ALL Mod Shapers and ALL `lag_ms` (and external modulators drop `lag_ms` on every target except Volume & Pitch, plus drop negative `lag_ms` everywhere)
- **KONTRA location:**
  - [timed.rs:110-207](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/timed.rs#L110-L207): Never reads `t.shaper` (zero diagnostic!) and hardcodes `tau_ms: 0.0` on Volume/Pitch while ignoring `t.lag_ms` on all other targets.
  - [external.rs:103-147](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/external.rs#L103-L147): Ignores `t.lag_ms` on `Pan`, `Cutoff`, `Resonance`, `EqGain`, `Wavetable`, `EffectParam`, `Intensity`, `LfoRate` (no diagnostic).
  - [laws.rs:101-103](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/laws.rs#L101-L103): `f32::from(lag_ms.max(0)) / 100f32.ln()` silently zeroes negative `lag_ms`.
- **Ground-truth evidence:**
  - [0140870000.c:2931-2955,3993-4039](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140870000.c#L2931-L2955): `FUN_140873040` compiles `t.shaper` identically for BOTH internal (`param_3 < 0x10`) and external (`param_3 >= 0x10`) modulators.
  - [0140870000.c:3789-3806,4208-4240](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140870000.c#L3789-L3806): `lag_ms` (`*(i16*)(target + 0xe)`) runs on **every** modulation target, with TWO modes: positive `lag_ms` = 1-pole exponential (`1 - exp(-ln(100) / (sr * lag / 32000))`, constants `144f38de0 = -4.605170186`, `144f387c8 = 3.125e-5`), negative `lag_ms` = discrete N-tick slew/boxcar (`N = round(sr * |lag| * 3.125e-5 * 0.25)`, constant `144e79a60 = 0.25`).
- **Impact:** Shaped LFOs/envelopes (e.g. quantized/stepped LFOs, curved filter envelopes) play raw unshaped waveforms; smoothed CC→filterCutoff/pan/EQ/wavetable routes zipper-step instead of gliding smoothly.
- **KISS/YAGNI Fix:** Reuse `external::shaped()` in `timed.rs` (wrapping internal `Lfo`/`Envelope` sources in `ir::Transfer`, which [voice_gain.rs:387-391](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs#L387-L391) already handles via `(x + 1)/2` for LFOs!); lift `smooth_ms` onto `ir::Mod` (or per-route smoother state) so pan/cutoff/resonance/EQ/wavetable get smoothing too, and at minimum map `|lag_ms|` for negative lag (or diagnose/implement linear slew).

---

### P0-9. `fx_idx_amp_split_point` is ignored — group insert effects run at the wrong side of the amplitude envelope & volume/pan stage
- **KONTRA location:**
  - [translate.rs:696-697](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/translate.rs#L696-L697) logs `fx_idx_amp_split_point != 8` as ignored.
  - [voice/src/lib.rs:535-580](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/voice/src/lib.rs#L535-L580) & [group_racks.rs:8](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/racks/src/group_racks.rs#L8): ALL per-voice rack effects (`voice_rack`) execute **pre-envelope**, while unmodulated LTI effects (filters/EQs) are hoisted onto summed group `Chain`s **post-envelope** regardless of where `fx_idx_amp_split_point` sits!
  - Furthermore, [voice_gain.rs:108-121](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs#L108-L121) commutes matrix/gain effects (`effects::rack(inserts)`) with `amp` (`product(group, amp)`) even across nonlinear per-voice effects!
- **Ground-truth evidence:**
  - [KONTAKT_FORMAT_GAPS.md:286](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/docs/architecture-v2/KONTAKT_FORMAT_GAPS.md#L286): `fx_idx_amp_split_point` is non-zero in **100% of measured group v0x95/v0x96 files (834/834)**.
  - [KONTAKT_TOPOLOGY.md:81-83,112](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/docs/architecture-v2/KONTAKT_TOPOLOGY.md#L81-L83): Group inserts `0..split` are **pre-amp** (before envelope/volume/pan); `split..8` are **post-amp** (after envelope/volume/pan).
- **Impact:** Any nonlinear post-amp group effect (compressor, saturator, tape, skreamer, lo-fi placed after the amp split) receives full-scale pre-envelope audio instead of envelope-shaped & velocity-scaled audio — completely altering distortion drive, threshold behavior, and decay tail dynamics! Conversely, a post-amp Send Levels tap moved pre-amp sends un-enveloped full-scale audio to reverb/delay.
- **KISS/YAGNI Fix:** Store `amp_split: u8` on `ir::Group` (default `inserts.len()`). In `Voice::render_through`, split `slot.rack` at `amp_split`: run `pre` units → multiply by envelope & `gain` → run `post` units. Only hoist LTI effects to summed `Chain`s when no nonlinear effect or amp boundary separates them.

---

## P1: Important Gaps / Edge-Case Bugs

### P1-1. `ModSource::Pressure` (Mono/Poly Aftertouch) gets cubic (`power = 3`) volume law instead of linear (`power = 1`), and Poly/Mono Aftertouch are conflated
- **KONTRA location:**
  - [laws.rs:112-126](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/laws.rs#L112-L126): `laws::volume()` returns `power = 1` for `Controller`, `ControllerFrom`, `Script`, `PitchBend`, and falls through to `power = 3` for **`ir::Source::Pressure`**!
  - [external.rs:54](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/external.rs#L54): Maps BOTH `ModSource::PolyAftertouch` (`2`) and `ModSource::MonoAftertouch` (`3`) to a single `ir::Source::Pressure`.
- **Ground-truth evidence:**
  - [0140870000.c:2927-2928,4325-4330](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140870000.c#L2927-L2928): `local_5b4 = (iVar10 - 7U < 7)` — ONLY sources `7..=13` get `fcubic`; PolyAT (`2`) and MonoAT (`3`) have `local_5b4 == false` (**linear, `power = 1`**).
- **Impact:** Aftertouch→Volume is **12 dB too quiet** at pressure 0.5 (`0.125` vs `0.5`). Conflating PolyAT and MonoAT causes channel-pressure routes to fire on polyphonic pressure and vice versa.
- **KISS/YAGNI Fix:** Split `ir::Source::Pressure` into `ChannelPressure` and `PolyPressure`, and include both in `laws::volume`'s `CONTROLLER_VOLUME_POWER` (`1`) arm.

---

### P1-2. `ModSource::ReleaseVelocity` is completely unimplemented and release velocity is discarded at engine ingress
- **KONTRA location:**
  - [engine/src/lib.rs:685](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/lib.rs#L685): `Event::NoteOff(pattern) | Event::NoteOffVelocity(pattern, _) => self.note_off(pattern)` discards the release velocity byte immediately!
  - [external.rs:64](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/external.rs#L64): `ModSource::ReleaseVelocity` hits `_ => None` and is dropped.
- **Ground-truth evidence:** [0140840000.c:9343-9350](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140840000.c#L9343-L9350) (`case 9:` `releaseVelocity`, normalized `0..=1`, cubic on volume).
- **Impact:** Piano/harpsichord/organ release-sample volume/timbre scaled by key-release speed fails.
- **KISS/YAGNI Fix:** Store `release_velocity: f32` on `Note` (populated in `note_off`, forwarded to release-triggered voices), and add `ir::Source::ReleaseVelocity`.

---

### P1-3. Zone key/velocity crossfade bug at `low == 0` silences MIDI note 0
- **KONTRA location:** [plan/src/lib.rs:423-426](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/lib.rs#L423-L426):
  ```rust
  low: low.saturating_sub(1),
  high: low.saturating_add(fade_in),
  ```
  combined with `Crossfade::gain` at [plan/src/lib.rs:124](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/lib.rs#L124): `if value <= lo { 0.0 }`.
- **Ground-truth evidence:** Measured Kontakt fade law ([BASIC_BEHAVIORS.md:29](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/docs/re/engine/BASIC_BEHAVIORS.md#L29)) is `(v - L + 1) / (F + 1)` — at `v == L == 0` with `F > 0`, gain must be `1 / (F + 1) > 0`. Because `0u8.saturating_sub(1) == 0`, `value <= lo` evaluates `0 <= 0` → **`0.0` (completely silent)**, and every step inside `[0..fade_in]` has denominator `F` instead of `F + 1`.
- **Impact:** Any zone starting at key `0` with `fade_low_key > 0` (or velocity `0` with `fade_low_velocity > 0`) silences its bottom note and shifts its fade ramp.
- **KISS/YAGNI Fix:** Store `Crossfade::{low, high}` as `i16` (or `lo = i16::from(low) - 1`, `hi = i16::from(high) + 1`) so `low == 0` yields `lo = -1` and `(0 - (-1)) / (F + 1) = 1 / (F + 1)`.

---

### P1-4. LFO Random wave uses only `note.key` as PRNG seed (identical random sequence on repeated notes, broken Free-clock sharing)
- **KONTRA location:** [voice_gain.rs:209](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs#L209):
  ```rust
  Some(modulators::lfo(&lfo, age, u32::from(note.key), self.tempo))
  ```
- **Ground-truth evidence:** Every voice played on the same MIDI key receives the exact same `seed = note.key`, so retriggered Random LFOs produce a **deterministic identical pattern on every keypress** (and two layered voices on the same key are 100% correlated). Conversely, for `Clock::Free` (non-retriggered) Random LFOs, different keys in a chord get **different** random sequences instead of sharing the free-running LFO.
- **KISS/YAGNI Fix:** Pass `note.draw` (per-note random seed) for `Clock::Note`, and a group/LFO-index seed (`group_idx * 32 + lfo_idx`) for `Clock::Free`.

---

### P1-5. Multiple lagging routes on Volume or Pitch collapse into a single lag smoother (`tau.max(tau_ms)`), and `LfoRate`/`Release` routes freeze at `Age::START`
- **KONTRA location:**
  - [voice_gain.rs:64-84](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs#L64-L84) & [voice_gain.rs:469-474](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs#L469-L474): All lagging volume routes are multiplied together and smoothed by one `Lag` at `tau.max(tau_ms)`; all lagging pitch routes are summed and smoothed by one `Bend` at `bend.tau_ms.max(tau_ms)`.
  - [voice_gain.rs:224-245](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs#L224-L245): `LfoRate` / `LfoHz` / `LfoDivision` routes evaluate ONLY at `Age::START` (note-on) instead of live!
  - [voice_gain.rs:586-620](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs#L586-L620): `ModKind::Release` routes evaluate ONLY at `Age::START` (note-on), missing CC/aftertouch/modwheel changes made while holding a note before release!
- **Ground-truth evidence:**
  - [0140870000.c:4208-4240](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140870000.c#L4208-L4240): Kontakt compiles an independent `$b` lag state per target route. If a group has CC1→Volume with `lag = 500 ms` and Velocity→Volume (or CC11) with `lag = 20 ms`, KONTRA smears the 20 ms route over 500 ms!
  - Live CC→LFO frequency (e.g. mod-wheel speeding up vibrato/tremolo while holding a note) does nothing after note-on!
- **Impact:** Slow-lagged routes hijack fast-lagged routes on the same voice; live vibrato-rate sweeps (CC1→LFO freq) are completely static during held notes; live CC→release tweaks while holding a note don't affect release.
- **KISS/YAGNI Fix:** Keep a small fixed per-voice smoother array (indexed by lagging route slot, zero RT alloc); evaluate `Release` routes on `note_off` (passing release `Age`); and integrate per-voice LFO phase incrementally per control span when an LFO has a live rate route (already noted in the `ponytail` comment at [voice_gain.rs:221](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs#L221)).

---

### P1-6. Missing DBD envelope (`0x41`), additive `intensity` flag (`unknown_flags & 4`), and UVI bipolar/inverted edge cases
- **KONTRA location:**
  - [translate.rs:836](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/translate.rs#L836): `Modulator::Dbd` (`BParEnv_DBD`, `0x41`) is decoded by `ni-file` ([envelope.rs](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/vendor/ni-file/src/kontakt/objects/envelope.rs)) yet **not matched** in `translate.rs` or `timed.rs` (falls through to unsupported diagnostic). Note [0140870000.c:2918-2921,3970-3975](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140870000.c#L2918-L2921): DBD multiplies its output by `* 3.0` (`* 9.0` for pitch `6` / `0x11`).
  - [external.rs:305](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/external.rs#L305): `("intensity", of) if t.unknown_flags & 4 == 0` drops additive/bipolar intensity routes (`0140870000.c:3910, 4356`).
  - [uvi/src/program/modulation.rs:244](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/uvi/src/program/modulation.rs#L244) & [uvi/src/program/lfo_routes.rs:147](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/uvi/src/program/lfo_routes.rs#L147): UVI `reach()` drops **every** mapped connection with `Inverted="1"` (`sign > 0.0`), drops inverted `AHD`/`DAHDSR`/`StepEnvelope` connections (`line 277`), and drops negative-ratio `Depth` routes sourced from envelopes/rising LFOs/blends.
  - [uvi/src/program/ignored.rs:109](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/uvi/src/program/ignored.rs#L109): UVI's unhandled-attribute detector skips every attribute whose value is `"0"` (`matches!(value, "" | "0")`) and matches attributes globally across all files via substring `include_str!` (`quoted()`), so an attribute read on one XML tag (e.g. `DelayTime` on `DAHDSR`) is **silently ignored without diagnostic** when present on another tag (e.g. `LFO` or `Delay`).
- **Impact:** Kontakt DBD pitch/filter envelopes (percussive/brass pitch dips) and UVI inverted mapped/envelope routes are dropped; UVI's diagnostic report has blind spots on reused XML attribute names and explicit `"0"` overrides of non-zero defaults.
- **KISS/YAGNI Fix:** Lower `Modulator::Dbd` to `ir::Breakpoints` (3 breakpoints, `0 -> break -> 0`, scaling route depth by `3.0` / `9.0` on pitch); allow `Inverted="1"` on UVI mapped/envelope connections by reversing the transfer table (`1 - y` or `table.reverse()`).

---

### P1-7. Block-rate modulation stepping (`MAX_SPAN = 256` frames = 5.33 ms at 48 kHz) & `GAIN_SMOOTH` smearing fast envelopes/LFOs
- **KONTRA location:**
  - [engine/src/lib.rs:34](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/lib.rs#L34): `MAX_SPAN = 256` (~5.33 ms at 48 kHz; 5.8 ms at 44.1 kHz).
  - [filters.rs:175](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/filters.rs#L175) & [modulators.rs:133,158](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/modulators.rs#L133): `refresh_timed` runs only on `now % 256 == 0` boundaries! Pitch, pan, filter cutoff/Q, EQ gain, wavetable position, and LFO-driven volume all step at **187.5 Hz** (8× coarser than Kontakt's 32-frame / 1500 Hz control rate in [0140870000.c:3798](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140870000.c#L3798) and [KONTAKT_DSP_LAWS.md:303](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/docs/architecture-v2/KONTAKT_DSP_LAWS.md#L303)).
  - Furthermore, if a voice starts mid-span (e.g. `now % 256 == 200`), on the very next tick (56 frames later) `refresh_timed` advances `glide` by a full **`MAX_SPAN = 256` frames** ([modulators.rs:133](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/modulators.rs#L133)), causing up to 5.3 ms glide-timing jitter!
  - [voice/src/lib.rs:24,562-565](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/voice/src/lib.rs#L24): `GAIN_SMOOTH = 0.01` (~2.1 ms one-pole at 48 kHz) smooths `self.gain` on **every** change — including fast LFO tremolo (>20 Hz) and sharp step modulators, low-passing them at ~76 Hz!
- **Impact:** Fast LFO vibrato/filter sweeps (>15 Hz, Kontakt LFOs go up to 213 Hz!) alias and zipper-step at 187.5 Hz; fast filter envelopes sound staircase-stepped; sharp step-modulator tremolo is rounded off by ~2 ms.
- **KISS/YAGNI Fix:** Lower the internal control sub-span to `32` or `64` frames (at 44.1/48 kHz; scale with sample rate so control cadence is rate-invariant, e.g. `~0.67–1.33 ms`), pass the **actual elapsed frames since last tick** to `glide()`, and linearly ramp gain/pitch/cutoff across the control sub-span instead of hard-stepping + unconditionally 2 ms-smoothing `self.gain`.

---

### P1-8. Pan law is linear balance everywhere — no equal-power or constant-power option for non-Kontakt formats (UVI/SFZ/future)
- **KONTRA location:** [voice_gain.rs:664-673](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs#L664-L673):
  ```rust
  // ponytail: balance pan law only; equal-power and Kontakt's raised cosine ...
  [g * (1.0 - pan).min(1.0), g * (1.0 + pan).min(1.0)]
  ```
- **Ground-truth evidence:**
  - For **Kontakt** buses & Stereo Modeller, linear stereo balance `L = 1 - clamp(pan, 0, 1)`, `R = clamp(pan + 1, 0, 1)` IS verified native ([kontakt-bus-pan.md:10](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/docs/re/kontakt-bus-pan.md#L10), [KONTAKT_DSP_LAWS.md:270](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/docs/architecture-v2/KONTAKT_DSP_LAWS.md#L270)).
  - For **SFZ**, **UVI**, and modern stereo/mono panning (where mono samples panned center lose/gain 3 dB under linear balance vs equal-power `-3 dB` sin/cos law), there is no `PanLaw` field in `ir::Group` / `ir::Instrument`.
- **Impact:** Mono sources in SFZ/UVI/future formats pan with a +3 dB center bump (or −3 dB hard-pan drop) compared to equal-power panning.
- **KISS/YAGNI Fix:** Add a small `ir::PanLaw { Balance, EqualPower }` data field on `ir::Group` (default `Balance` for Kontakt, `EqualPower` where formats specify it) evaluated inside `voice_gain::gain()`.

---

## P2: Architectural Fragility vs Future-Proof Multisampler Vision

### P2-1. Audio-thread heap allocations hidden in KSP/MIDI-object script callbacks
- **KONTRA location:**
  - [zone_commands.rs:98-104,152](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/zone_commands.rs#L98-L104): `to_string_lossy().into_owned()` / `.replace('\\', "/")` allocates `String` on the audio thread when a KSP callback queries zone sample paths/names.
  - [midi_object.rs:198](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/midi_object.rs#L198): `let mut spans: Vec<(i32, i32, i32)> = Vec::new();` allocates on `mf_insert_file`.
  - *(Note: Voice start/render, racks, chains, and `Loops` (`SmallVec<[Loop; 8]>`) were audited and are clean — `build_voice_racks`, `build_chains`, and `BusState::new` are load-time only.)*
- **Fix:** Precompute path/name slices in `Plan` at load time; use a fixed stack buffer or preallocated scratch `Vec` for `midi_object` spans.

---

### P2-2. Subtle Kontakt vendor leaks inside format-neutral `kontra-ir` and `kontra-plan`
- **KONTRA location:**
  - [plan/src/par_laws.rs](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/par_laws.rs) & [engine/src/params.rs:1-3](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/params.rs#L1-L3): Hardcodes KSP `0..=1_000_000` integer encoding, `BUS_OFFSET` (`$NI_BUS_OFFSET`), `STAGE_15S` / `STAGE_25S`, and `CUBE` directly inside `kontra-plan` and `kontra-engine` instead of keeping parameter encoding laws strictly in the format/script adapter layer.
  - [plan/src/zones.rs:513](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/zones.rs#L513): Kontakt's `guess_bpm` heuristic lives in `kontra-plan`.
  - [mods.rs:288-296](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/mods.rs#L288-L296): `ir::Curve` has overlapping ad-hoc variants (`Depth { power }`, `Falling`, `Signed`, `Linear`) for Kontakt vs UVI instead of one unified polarity/shaping model.
- **Vision alignment ([VISION.md](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/docs/architecture-v3/VISION.md)):** Consolidate `Curve` into a single neutral `(polarity, curve_power / transfer)` law, and move KSP `0..=1_000_000` & `BUS_OFFSET` conversions into the Kontakt script-host boundary so `kontra-engine` only receives normalized or physical values.

---

### P2-3. Hard 256-group cap on private live-edited chains & no per-note/MPE channel-vs-note separation for `PitchBend`/`CC`
- **KONTRA location:**
  - [filters.rs:25,103-119](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/filters.rs#L25): `PRIVATE_CHAINS = 256` silently drops post-init script writes to group insert filters/EQs for groups past index 255 in large orchestral instruments (many NI/Cinematic instruments exceed 256 groups!).
  - [voice_gain.rs:302-319](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs#L302-L319): Per-note MPE/CLAP expressions exist for `brightness` (CC74), `pressure`, `gain`, `pan`, `tune`, **but `ir::Source::PitchBend` reads only channel-level `self.behaviour.bend(channel)`** (ignoring `note.expression_tune` when a route explicitly uses `PitchBend` as a mod source, e.g., PitchBend→FilterCutoff or PitchBend→Wavetable in MPE/MIDI 2.0!).
- **Fix:** Replace the fixed `PRIVATE_CHAINS = 256` cap with a pool sized from the instrument's script-addressed groups (or dynamic off-audio growth), and thread per-note bend into `ir::Source::PitchBend` when present.

---

### P2-4. `BParGroupDynamics` (`0x4A`) is completely unread (low immediate risk, verify for K8 patches)
- **KONTRA location:** [group.rs:149](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/vendor/ni-file/src/kontakt/objects/group.rs#L149) notes `BParGroupDynamics` (`0x4A`) is 256 zero bytes in the local corpus and is never decoded or checked for non-zero bytes.
- **Ground-truth evidence:** [0140d00000.c:2370-2394](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140d00000.c#L2370-L2394) reads 64 floats (`256` bytes) into `BParGroupDynamics + 0x10`.
- **Fix:** Add a lightweight non-zero check in `translate.rs` that emits an `Impact::Audible` note if any future library saves non-zero `0x4A` bytes, preventing silent drops.

---

## Audited Subsystems Verified Solid

To avoid wasted rework, the following subsystems were verified against Kontakt 8.13.1 C source / DSP math and are **clean**:

1. **Modulation Shaper breakpoint curvature** ([kontakt.rs:197-210](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/base/law/src/kontakt.rs#L197-L210)): Properly scales curvature by `16.0` and flips sign on falling segments, matching `FUN_14087c7f0` ([0140870000.c:8608-8615](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140870000.c#L8608-L8615)).
2. **Volume source power split (`Controller`/`PitchBend`/`Script` vs `Velocity`/`Random`/`Constant`)** ([laws.rs:112-126](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/laws.rs#L112-L126)): Matches `FUN_140873040` (`local_5b4 = iVar10 - 7U < 7`, [0140870000.c:2928,4325-4330](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140870000.c#L2928)), modulo the missing `Pressure` arm noted in P1-1.
3. **Note-on smoother snapping** ([voice/src/lib.rs](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/voice/src/lib.rs), [start.rs:213-214](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/start.rs#L213-L214)): `self.gain`, `self.lagged`, `slot.bend`, and `slot.pan` all snap to their initial note-on values (matching Kontakt's `on note: $b := -1.0e6` first-tick snap at [0140870000.c:3851](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140870000.c#L3851)) — no ramp-from-zero attack clicks.
4. **Instrument volume & pan staging** ([voice_gain.rs:190-200](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs#L190-L200), [buses.rs](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/buses.rs)): Unlike v2 (T01), v3 keeps instrument volume/pan on the output fader after instrument inserts rather than folding them into zones!
5. **`BParLFO` `normalizeMultiLFO` alignment** ([lfo.rs:32-36,98-107](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/vendor/ni-file/src/kontakt/objects/lfo.rs#L32-L36), [laws.rs:326](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/laws.rs#L326)): Correctly decodes `+0x1c` (`normalizeMultiLFO`) matching [0140d00000.c:2445](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140d00000.c#L2445) & [0140920000.c:6810-6822](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140920000.c#L6810-L6822).
6. **Voice start & render real-time allocation discipline** ([zone.rs:10](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/voice/src/zone.rs#L10), [voice_rack.rs:36-52](file:///data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_rack.rs#L36-L52)): `Loops` uses inline `SmallVec<[Loop; 8]>`; per-voice racks and comb delay lines are pre-reserved at load time (`build_voice_racks`) with zero heap allocation on note-on or render.
7. **`ModTarget.invert` (`+0x16`) non-use**: Verified via [0140920000.c:7098-7111](file:///data/dev/workspace/Research/KONTRA/source/kontakt8/0140920000.c#L7098-L7111) to be the UI flag `"tableOpen"` (not audio inversion), so ignoring it is correct (rename in `ni-file` to prevent future confusion).
