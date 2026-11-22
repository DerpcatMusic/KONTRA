<!-- Verbatim copy of the IR vNext blueprint written by the "Future-Forward IR System Audit" agent (Gemini, 2026-11-22). Kept as the source for 13-IR-VNEXT.md, which tracks the status. File links point at the audit worktree of that day. -->

# KONTRA IR Architectural Audit: Ground Truth & vNext Blueprint

> **Vision bar:** *"One gold-standard engine. Every format is a translation into one IR. Faithful where it is audible, modern everywhere else. Then go beyond what the originals can do."* ([VISION.md:3-5](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/docs/architecture-v3/VISION.md:3))

---

## 1. Ground-Truth Inventory (`kontra-ir` & `kontra-plan` Today)

### 1.1 Crate Footprint & Type Inventory

`kontra-ir` is currently **3,525 LOC across 12 files**; `kontra-plan` is **3,273 LOC across 12 files**; `kontra-prepare` is **1,083 LOC across 2 files** (total **7,881 LOC** at the IR/Plan/Prepare boundary).

| Crate / File | Key Types & Exact Sizes / Counts |
|---|---|
| [ir/src/lib.rs](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/lib.rs:48) | `Instrument` (27 fields), `Group` (29 fields), `Zone` (17 fields; 18 on `zone-envelopes`), `Effect` (**≤ 232 B** asserted at [lib.rs:1032](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/lib.rs:1032)), `EffectKind` (**31 variants**), `VoiceGroup` (11 fields), `Glide` (9 fields), `Envelope` (10 fields), `Breakpoints` (5 fields), `Lfo` (5 fields), `Mod` (5 fields), `StartRule` (4 variants), `Trigger` (4 variants), `Sample` (8 fields), `Keystream` (2 variants), `Impulse` (10 fields), `Script` (7 fields), `Dialect` (2 variants) |
| [ir/src/mods.rs](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/mods.rs:7) | `ModKind` (**20 variants**), `Source` (**14 variants**; 15 w/ `ZoneEnvelope(u8)`), `Curve` (5 variants), `Wave` (8 variants), `Attractor` (2 variants), `Exp2` (2 variants), `Blend` / `Term` / `Factor` / `Transfer` |
| [ir/src/drive.rs](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/drive.rs:9) | `EffectParam` (**56 variants**), `SlotKnob` (**30 variants**), `KnobTarget` (9 variants), `RackAt` (5 variants), `Curve` (10 variants), `Shaper`, `Crush`, `Converter` (15 fields), `RingMod` (11 fields), `Overdrive`, `Clipper`, `SlotLaw`, `KnobModel`, `SlotShape`, `SlotShapes` (8 fields), `SlotType` (7 fields) |
| [ir/src/effects.rs](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/effects.rs:7) | `Compressor` (11 fields), `Strip` (2 fields), `Transient`, `Limiter`, `Scanner` (13 fields), `Delay` (18 fields), `Shifter`, `Phaser`, `Notches` (18 fields), `Ensemble` (13 fields), `CombBank` (16 fields), `Comb`, `Reverb` (11 fields), `Clocked`, `Formant`, `Cascade`, `ImpulseKnob` (4 variants) |
| [ir/src/engine_par.rs](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/engine_par.rs:10) | `EnginePar` (**38 variants**), `ZonePar` (**19 variants**), `Drives` (2 variants), `ScriptPar` (3 fields) |
| [ir/src/wavetable.rs](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/wavetable.rs:7) | `Wavetable` (8 fields), `WarpShape` (**15 variants**), `FormShaper` (**10 variants**), `ModOscKind` (9 variants), `TableValue` (7 variants), `Quality` (4 variants), `TuneUnit` (3 variants) |
| [ir/src/readout.rs](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/readout.rs:8), [bus.rs](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/bus.rs:4), [link.rs](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/link.rs:6), [output.rs](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/output.rs:7), [reverb.rs](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/reverb.rs:9), [route_names.rs](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/route_names.rs:6) | `Readout` + `Form` (9 variants), `Bus` (9 fields) + `BusSend`, `CcLink` + `KnobMod` + `Linked` (3 variants), `Output` + `Balance` + `OutputControllers`, `Network` (3 variants), `RouteNames` |
| [plan/src/lib.rs](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/lib.rs:493) & [zones.rs](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/zones.rs:13) | `Plan` (33 fields), `Group` (17 fields), `Zone` (24 fields; 25 w/ `zone-envelopes`), `ZoneFacts` (18 fields), `ZoneRules` (10 fields), `Bus` (10 fields), `ScriptPlan` (26 fields) |

### 1.2 Real Memory Footprint on Areia (42,636 Zones)

Computing exact 64-bit `repr(Rust)` struct layouts (before any audio sample bytes are loaded):

| Struct | Exact Est. Size | 42,636 Zones (Areia) | Dead / Wasted Bytes on PCM Orchestral Library |
|---|---:|---:|---|
| `ir::Zone` ([lib.rs:749](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/lib.rs:749)) | **120 B** (main) / **144 B** (`zone-envelopes`) | **5.12 MB** / **6.14 MB** | `mix: Vec<[f32; 2]>` (24 B) + `envelopes: Vec<ZoneEnvelope>` (24 B) = **48 B/zone (33%)** always empty; `start`/`end`/`start_mod` are `u64` (24 B vs `u32` 12 B) |
| `kontra_voice::Region` ([voice/src/zone.rs:67](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/voice/src/zone.rs:67)) | **~632 B** | (embedded in `plan::Zone`) | `loops: SmallVec<[Loop; 8]>` (**264 B inline!**), `mix: Option<[[f32; 2]; 16]>` (**132 B inline!**), `table: Option<Table>` (**208 B inline!**) — **604 B (95.6%)** empty on ordinary non-looped stereo PCM zones |
| `plan::Zone` ([plan/src/lib.rs:353](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/lib.rs:353)) | **~768 B** (main) / **~792 B** (`zone-envelopes`) | **~32.7 MB** / **~33.8 MB** | Embeds `Region` (~632 B) + `start: Vec` (24 B) + `crossfades: Vec` (24 B) + `envelopes: Vec` (24 B). Spans **12 cache lines** per zone! |
| `plan::ZoneFacts` ([plan/src/zones.rs:13](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/zones.rs:13)) | **~312 B** + `Box<str>` (~32 B) | **~14.7 MB** | `loop_pars: [LoopPar; 8]` = **192 B/zone (61.5%)** dead zeros on every saved zone (only ever populated for `user: true` zones!). Duplicates 14 fields already in `plan::Zone`. |
| `ir::Group` / `plan::Group` | **~480 B** / **~496 B** | ~0.5 MB (per ~500 groups) | Plus `inserts: Vec<Option<Effect>>` resized to 8×232 B = **1.86 KB/group** (~928 KB for 500 groups) whenever KSP references `$ENGINE_PAR_EFFECT_` ([kontakt/translate.rs:440-452](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/translate.rs:440)) |
| **Peak metadata during `prepare`** | `IR + Plan + ZoneFacts` | **~53–56 MB** + >120k heap allocs | And `prepare::wake` ([prepare/src/lib.rs:285](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/prepare/src/lib.rs:285)) / `user_zones` ([prepare/src/lib.rs:364](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/prepare/src/lib.rs:364)) call `plan.clone()`, deep-cloning all 47 MB of `zones` + `zone_facts`! |

### 1.3 Every Vendor Leak Currently Living in `kontra-ir` and `kontra-plan`

This violates `VISION.md`'s hard boundary (*"IR never names a vendor or a vendor-only unit"*):

| Severity | File:Line | Vendor Leak |
|---|---|---|
| **Critical** | [ir/src/lib.rs:231,266-306](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/lib.rs:231) | `Keystream::Blocks` embeds **UVI's proprietary decryption algorithm** (`word_stream` with exact `uvi-file` constants `0xc6a4_a793_5bd1_e995` / `0x5851_f42d_4c95_7f2d`) directly in `kontra-ir`! Format decryption belongs in `format-uvi`'s `Source` reader, not in the IR. |
| **Critical** | [ir/src/engine_par.rs:10-138](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/engine_par.rs:10) | `EnginePar` (38 variants), `ZonePar` (19 variants), `Drives`, `ScriptPar` are a **1:1 encoding of KSP's `0..=1_000_000` integer API**: hardcodes `1024 + n` bus offset (`OutputChannel`, line 46), `x²` / `(2x-1)³` modulation knobs (lines 18-20), 8 sends (`SendLevel(u8)`), `$NI_WT_MOD_WAVE_*` (line 69), `$ENGINE_PAR_WT_QUALITY` (line 73), KSP `Held(u32)` state bag (line 79), and `$ZONE_PAR_*` (`Selected`, `Bpm` × 1000). |
| **Critical** | [ir/src/drive.rs:651-672](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/drive.rs:651) | `KnobModel` embeds a **raw Rust function pointer `build: fn(&[f32]) -> Option<EffectKind>`** pointing into `format-kontakt`! Makes `ir::Instrument` **unserializable** for `format-native` and couples IR lifetimes to compiled vendor translator code. |
| **Critical** | [ir/src/drive.rs:30,37,211](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/drive.rs:30), [effects.rs:181,189,283](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/effects.rs:181), [reverb.rs:11,13](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/reverb.rs:11), [base/law/src/section.rs:33](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/base/law/src/section.rs:33) | **`&'static [...]` borrowed slices** inside `Curve::Table`, `Curve::Blend`, `Overdrive::curve`, `Delay::body`, `Delay::shape`, `Ensemble::voices`, `Network::Lines`/`Combs`, `Sections::fixed`. Blocks dynamic/native preset loading without `Box::leak`. |
| **Critical** | [plan/src/par_values.rs:1-124](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/par_values.rs:1) & [par_laws.rs:73-90](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/par_laws.rs:73) | Hardcodes **Kontakt's KSP laws directly in `kontra-plan`**: `UNITY_EFFECT_GAIN = 396_851.0`, `UNITY_VOLUME = 629_960.52`, `BUS_OFFSET = 1024`, `±36 st` tune, `STAGE_15S` / `STAGE_25S` / `SQUARE` / `SIGNED_CUBE`. |
| **Critical** | [plan/src/zones.rs:31-58,94-122,355-436](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/zones.rs:31) | Hardcodes **Kontakt 8.13 KSP zone quirks in `kontra-plan`**: centi-dB / milli-pan / cents units, `VOLUME_RANGE = (-3600, 3600)`, `USER_ROOT = 36`, raw KSP `$LOOP_PAR_*` numbers `520..=525`, and `guess_bpm` with Kontakt's exact `-1` float rounding bug (`zones.rs:405-407`). |
| **High** | [ir/src/route_names.rs:1-65](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/route_names.rs:1) | `RouteNames` parses `"#123"` tokens out of `Mod::name` — a **UVI Augmented Orchestra translator workaround** ([uvi/program.rs:526](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/uvi/src/program.rs:526)) promoted into a public `kontra-ir` type! |
| **High** | [ir/src/drive.rs:577-748](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/drive.rs:577) | `SlotKnob` (30 variants), `RackAt` (5 Kontakt racks), `SlotShapes` (raw `i32` Kontakt `$EFFECT_TYPE_*` / `$FILTER_TYPE_*` IDs, `held`, `fresh`, `fresh_normalize`). |
| **High** | [ir/src/wavetable.rs:26-93,109-128](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/wavetable.rs:26) | `Quality::from_index`, `TuneUnit::from_index`, raw `wave: u8` (`$NI_WT_MOD_WAVE_*` 0..=8), and hardcoded Kontakt wavetable constants (`0.62`, `0.24`, `0.5`, `2a`) in `ModOscKind`. |
| **High** | [ir/src/readout.rs:1-36](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/readout.rs:1) | `Readout` & `Form` (`Padded`, `Wide`, `WideWhole`, `Pan` = `"Center"`/`"n L"`) — Kontakt's `get_engine_par_disp` column-formatting rules inside `kontra-ir`. |
| **High** | [ir/src/lib.rs:471-502](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/lib.rs:471) & [mods.rs:105](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/mods.rs:105) | `NOTE_LADDER: [f32; 30]` and `29.0` multiplier in `LfoDivision` — Kontakt's exact 30-step tempo-sync ladder. |
| **High** | [plan/src/modulation.rs:269](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/modulation.rs:269) | `LFO_FADE_IN` (`0.3·((13/3)^x - 1)`, Kontakt's binary fade-in curve) applied unconditionally to **every format's** LFO fade-in. |
| **High** | [plan/src/lib.rs:107-135,408-433](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/lib.rs:107) | `Crossfade` hardcodes Kontakt's linear ramp law `(v - L + 1)/(F + 1)`; cannot express SFZ equal-power (`xf_*curve=power`) or custom crossfade curves. |
| **Medium** | [ir/src/lib.rs:69-74,93-98,332-338,348,373,573-576,838,954-970](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/lib.rs:69) | Kontakt's 3 fixed racks (`inserts`/`sends`/`main`), 8-slot `EffectKind::Sends([f32; 8])`, `Group::{repeat_cuts, offset_limited, purged, routes}`, `Mod::{slot, target}`, `Instrument::{knob_mods, knob_blends, knob_lfos}` (UVI Lua-only!), `link::Linked::{Knob, Slider, Switch}` (KSP UI types), `Script::{dialect: Ksp|Lua, saved: Vec<String>, data: Vec<(String, Vec<u8>)>}`. |
| **Medium** | [plan/src/script.rs:62-83,240-250](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/script.rs:62) | `folder(1..=4)` hardcodes KSP `$GET_FOLDER_*` indices; `Recall::of_type` hardcodes KSP `set_snapshot_type` parity; `hears_own_releases` / `held_release_ends_children` encode KSP vs UVI semantics as raw booleans. |

### 1.4 Non-Atomic, Redundant, and Overlapping Concepts

1. **Three separate envelope models** sharing the exact same exponential curve math `(e^(k·t) − 1)/(e^k − 1)`:
   - `ir::Envelope` ([lib.rs:533](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/lib.rs:533)): AHDSR amplitude-only, seconds, 10 fields, only one `fall_shape` (forcing UVI to drop distinct `DecayCurve` vs `ReleaseCurve` with an `Audible` diagnostic at [uvi/modulation.rs:44-47](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/uvi/src/program/modulation.rs:44)!).
   - `ir::Breakpoints` ([lib.rs:427](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/lib.rs:427)): time-based in **milliseconds** (`f32`), unipolar `0..=1`, with a `stages: bool` flag hack so KSP can mutate segments 0..3 as AHDSR.
   - `ir::ZoneEnvelope` (on `kontra-zone-envelopes` branch: `lib.rs:786-805`): sample-frame indexed (`i32`), bipolar `-1..=1`, `loop_range: Option<(i32, i32)>`, `speed_lock: bool`, living as a `Vec<ZoneEnvelope>` on every `Zone`.
2. **Ten overlapping parameter/target enums (~190+ variants) addressing the SAME controls:**
   - `ModKind` (20) + `EffectParam` (56) + `SlotKnob` (30) + `EnginePar` (38) + `ZonePar` (19) + `KnobTarget` (9) + `ReverbPar` (8) + `TableValue` (7) + `ImpulseKnob` (4) + `plan::{LevelPar, EffectPar}` (10).
   - Example of pure duplication: modulating filter cutoff uses `ModKind::Cutoff`; scripting it uses `SlotKnob::Cutoff` → `SlotLaw` → `EffectParam::FilterHz`; modulating EQ gain uses `ModKind::EqGain`, while scripting it uses `SlotKnob::BandGain` → `EffectParam::BellGain`; modulating LFO rate has **three** distinct `ModKind` variants (`LfoRate`, `LfoHz`, `LfoDivision`)!
3. **Four overlapping curve/shaping mechanisms:**
   - `kontra_law::Law` (12 variants, rich analytic + table + breakpoints + invertible) vs `ir::Curve` (5 volume-only variants in [mods.rs:265](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/mods.rs:265)) vs `ir::Transfer` (table-only `Vec<f32>` + `smooth_s` + `stepped` in [mods.rs:202](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/mods.rs:202)) vs `plan::Crossfade` (linear-only `u8` ramp in [plan/src/lib.rs:111](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/lib.rs:111)).
4. **Three parallel modulation containers (`Group`, `Bus`, `Instrument`):**
   - `Group` owns `mods: Vec<Mod>`, `lfos`, `envelopes`, `shapers`, `blends`, `routes`.
   - `Bus` duplicates `mods: Vec<ModKind>`, `lfos`, `blends` (no `shapers`, no `envelopes`, no `Mod` metadata).
   - `Instrument` duplicates `knob_mods`, `knob_lfos`, `knob_blends`.
   - Meanwhile `plan/src/modulation.rs:25` hardcodes `const WARPS: usize = 8`, silently dropping live LFO rate/chaos/envelope-stretch modulation past index 7!
5. **`EffectKind` proliferation (31 variants) violating `ENGINE.md` ("One implementation per family"):**
   - **6 filter/comb variants**: `Filter`, `Formant`, `Cascade`, `Clocked`, `Comb`, `CombBank`.
   - **6 drive/color variants**: `Shaper`, `Crush` (with nested `Converter`), `Overdrive`, `Clipper`, `RingMod`, plus `Strip` (just `Compressor + Shaper` glued into one slot, which single-handedly sets the 232 B ceiling on `ir::Effect`!).
   - **6 time-mod variants**: `Delay`, `Scanner`, `Shifter`, `Phaser`, `Notches`, `Ensemble`.
6. **Fragmented selection, trigger, and voice-limit rules:**
   - `Group::polyphony` vs `Group::voice_group` vs `Instrument::{voices, steal, steal_released_first, steal_fade_ms}` are three representations of the same voice-limit concept (`prepare` literally converts `Instrument`'s steal fields back into an `ir::VoiceGroup` at [prepare/src/lib.rs:156-161](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/prepare/src/lib.rs:156)!).
   - `Group` carries three unrelated `Option`s for generator mode (`stream`, `wavetable`, `speed`), allowing invalid states (`wavetable` + `stream`) that translators must manually reconcile ([kontakt/translate.rs:751](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/translate.rs:751)).
   - `StartRule` cannot express `RrCounter::Key` (which `Plan` already supports at [plan/src/lib.rs:72](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/lib.rs:72)!), nor random probability (`lorand/hirand`), nor CC crossfades (`Control::Cc` exists in `plan::Crossfade` at [plan/src/lib.rs:104](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/lib.rs:104) but is **unreachable** from `ir::Zone`!).

### 1.5 IR ↔ Plan Duplication & Impedance Mismatch

- **Verbatim cloning, minimal lowering:** In [prepare/src/lib.rs:110-167](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/prepare/src/lib.rs:110), `prepare` clones almost every IR container verbatim (`g.mods`, `g.envelopes`, `g.lfos`, `g.shapers`, `g.blends`, `g.routes`, `g.inserts`, `b.sends`, `b.mods`, `b.blends`, `b.lfos`, `slot_laws`, `slot_models`, `slot_shapes`, `voice_groups`, `output`, `route_pitch`). The only real lowering is:
  1. `dB → linear gain`, `seconds → frames` (for `Ahdsr`), `ir::Loop → voice::Loop`, `ir::Wavetable → voice::Read` (**exact 1:1 mirror structs** copied field-by-field across 95 lines at [prepare/src/lib.rs:617-712](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/prepare/src/lib.rs:617)!), and `ir::Trigger → plan::Trigger` (an identical 4-variant enum!).
  2. **Denormalizing group fields onto every zone:** `ZoneRules::apply` ([plan/src/zones.rs:160-178](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/zones.rs:160)) clones `group.start: Vec<(Join, Atom)>` onto **every single `plan::Zone`** (42,636 Vec allocations for ~500 groups!) plus copies `trigger`, `off_by`, `voice_group`, `keytrack`, `release_decay_db`, `repeat_cuts`, `offset_limited`, `one_shot` onto every zone — AND STILL keeps `ZoneRules` on `plan::Group`!
  3. **Duplicating every zone into `plan.zone_facts`** ([prepare/src/lib.rs:66-90](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/prepare/src/lib.rs:66)) — creating two parallel 42k-element vectors that `Plan::edit_zone` ([plan/src/zones.rs:192-286](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/zones.rs:192)) must linearly scan (`O(N)`) and keep in sync manually!
  4. **Purged groups hold raw `ir::Group` + `Vec<ir::Sample>` inside `plan::Dormant`** ([plan/src/lib.rs:568-573](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/plan/src/lib.rs:568)), re-running a miniature `prepare` inside `wake()` by constructing a fake `ir::Instrument`.

---

## 2. Format-Onboarding Ergonomics Audit

Walking through what a developer must write **today** to onboard DecentSampler (`.dspreset`), SF2/SF3, EXS24, HALion, or a custom synth reveals severe friction directly visible in `sfz`, `uvi`, `kontakt`, and `native`:

1. **`format-native` is still a 1-line empty stub** ([format/native/src/lib.rs:1](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/native/src/lib.rs:1)) because `kontra-ir` has no `serde`/`rkyv` support, contains function pointers (`KnobModel::build`) and `&'static` slices, and has no canonical schema.
2. **Group-vs-Zone Impedance Mismatch (SFZ, DecentSampler, EXS24, SF2):**
   - Formats like SFZ, SF2, and DecentSampler allow per-region/per-sample envelopes, filters, keytrack, triggers, and CC conditions. Because `kontra-ir` pins all of those strictly to `Group`, [sfz/src/translate.rs:370-384](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/sfz/src/translate.rs:370) has to synthesize groups with an **`O(N²)` linear dedup scan** (`self.entries.iter().position(|e| e.settings == settings)`). On a 10,000-region SFZ with unique filters/envelopes, this becomes quadratic and explodes group count!
3. **No Shared Format Author SDK / Utilities in `kontra-format`:**
   - `kontra-format` currently has only `Notes`, `moved()`, and `timing` ([format/common/src/lib.rs](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/common/src/lib.rs:1)).
   - **Sample header probing:** `sfz` imports `hound::WavReader` directly ([sfz/src/translate.rs:394](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/sfz/src/translate.rs:394)) — so SFZ cannot even probe FLAC/OGG/AIFF headers (despite `kontra-stream::header` existing!) because `format-*` cannot depend on `kontra-stream`. Every new format must re-roll WAV/FLAC/AIFF/OGG header parsing and sample deduplication (`HashMap<PathBuf, usize>` reimplemented in `sfz`, `uvi`, and `kontakt`).
   - **Common sampler math duplicated:** MIDI note name parsing (`c#4`), dB/amplitude conversion, pan laws, key/velocity fade normalization, and default preload sizing (`PRELOAD_BYTES = 61_440` copy-pasted from Kontakt into [uvi/program.rs:70-71](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/uvi/src/program.rs:70)) are re-implemented per crate.
   - **No builders or `Default` on `Zone` / `Mod` / `Effect`:** Every translator constructs 17-field `ir::Zone` literals filling in Kontakt-isms (`id: 0`, `mix: Vec::new()`, `start_mod: 0`, `slot: 0`).
4. **Fighting the IR for Standard Sampler Features (UVI & DecentSampler Pain Points):**
   - **No parallel racks / dry-wet splits:** UVI had to synthesize **4 dummy buses per `EffectRack` / `FeedbackMachine`** with `-200 dB` silent faders to fake bypassable parallel chains ([uvi/routing.rs:291-346](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/uvi/src/program/routing.rs:291)).
   - **No declarative parameter registry for UI/script bindings:** DecentSampler's `<binding type="effect" parameter="cutoff" translation="exponential" .../>` and UVI's `setParameter(name, val)` need uniform parameter addressing. Today UVI resorts to **black-box probing `PROBE_GRID` (23 rebuilds per XML attribute!)** at load time ([uvi/routing.rs:55-58,485-554](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/uvi/src/program/routing.rs:55)) just to infer which `EffectParam` a named parameter moves!
   - **No first-class macros / named controls:** UVI packs macros into magic `ir::Source::Script(0x8000 + 128*owner + n)` IDs ([uvi/matrix.rs:529-564](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/uvi/src/program/matrix.rs:529)) and encodes connection node IDs as `"#123"` strings inside `Mod::name` ([uvi/program.rs:526](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/uvi/src/program.rs:526)).
   - **No analytic `Transfer` transforms:** UVI allocates a 128-float `Vec<f32>` per group just to scale velocity `(v-1)/126` ([uvi/modulation.rs:261-271](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/uvi/src/program/modulation.rs:261)) and `vec![1.0, 0.0]` just to invert a CC ([uvi/modulation.rs:319](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/uvi/src/program/modulation.rs:319)).
5. **Fragile Diagnostics & Missing Validation Pass:**
   - `Notes::ignored_fields` ([format/common/src/lib.rs:68-92](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/common/src/lib.rs:68)) calls **`format!("{v:#?}")` and parses the pretty-printed Debug text**! On Areia, [kontakt/translate.rs:338](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/translate.rs:338) runs this **42,636 times** during load — massive needless allocation/CPU overhead and brittle against Debug formatting changes.
   - **Zero structural IR validation:** Nothing validates referential integrity (`Zone::sample`, `Group::bus`, `BusSend` DAG acyclicity, `Mod` target existence, `Loop` within `start..end`, LFO/env index bounds). Invalid IR silently drops or panics/no-ops downstream.

---

## 3. Atomicity, Orthogonality, & Dynamic Expressiveness

The root cause of bloat across `kontra-ir` is **cartesian-product modeling** (`ModKind`, `EffectParam`, `SlotKnob`, `EnginePar`, 3 envelope types, 3 rack vectors) instead of **orthogonal composition** (the MLIR / ECS / SFZ v2 principle: small orthogonal primitives composed by typed IDs).

### 3.1 Unified Modulation Graph: `(Source → Transform → Target)`

Replace the 20-variant `ModKind` + 3 parallel container sets (`Group`/`Bus`/`Instrument`) + `Blend`/`Transfer`/`Curve` overlap with **one orthogonal `ModRoute`** that works at **any scope** (`Voice`, `Group`, `Bus`, `Instrument`, `Control`):

- **`ModSource`** (small `Copy` enum, 8 bytes):
  - **Per-Note / 2026+ Expressions:** `Velocity`, `ReleaseVelocity`, `Key`, `NoteExpr(ExprKind)` (`Tune`, `Gain`, `Pan`, `Pressure`, `Brightness`, `Vibrato`, `Expression`), `PerNoteCc { bank: u8, index: u8, registered: bool }` (MIDI 2.0 UMP 32-bit!), `PolyMod(ParamId)` (CLAP `CLAP_EVENT_PARAM_MOD`), `HeldTime { span_ms: u32 }`, `Random { bipolar: bool }`, `ZoneEnv(ZoneEnvId)`.
  - **Channel / Global:** `Cc { cc: u8, default: Option<u8> }`, `PitchBend`, `ChannelPressure`, `Macro(MacroId)` (first-class — replaces UVI's `Script(0x8000+...)` hack!), `ScriptEvent(u16)`, `Constant`.
  - **Timed & Composed Modulators:** `Modulator(ModulatorId)` (indexes a unified modulator table: `Envelope`, `Lfo`, `StepSeq`, `Follower`), `Expr(SignalExprId)` (indexes an interned `SignalExpr` node for `Blend`/products/sums/shapers — replaces both `Source::Shaped` and `Source::Blend` with one composable DAG!).
- **`SignalOp` / `Transfer`** (unified on `kontra_law::Law` — zero duplication with `Curve` or `Transfer`!):
  - A route applies a `LawId` (referencing an **interned `Law` table** on `Instrument`, so 400 groups sharing the same velocity curve or Kontakt shaper store it **once**, not 400×128 floats!), plus `bipolar_in: bool`, `invert: bool`, `smooth_ms: f32`, `combine: CombineOp` (`Add`, `Mul`, `MulSigned`, `ScaleDepth`, `ExpOctaves`), `rate: EvalRate` (`NoteOn`, `Control`, `Audio`).
  - **Proof it doesn't lose 1 dB of fidelity:** Every existing `ir::Curve` (`Decibels`, `Depth{power}`, `Squared`, `Falling`, `Signed`), `Transfer` (LUT + `stepped` + `smooth_s`), and `Factor` is an exact, closed-form instance of `(LawId, CombineOp, smooth_ms)`!
- **`ParamAddr`** (ONE unified target address replacing `ModKind` targets, `EffectParam`, `SlotKnob`, `KnobTarget`, `EnginePar`, `TableValue`, `ImpulseKnob`):
  - `(Scope, Target)` where `Scope` is `Voice | Group(GroupId) | Zone(ZoneId) | Bus(BusId) | Node(NodeId) | Modulator(ModulatorId) | Route(RouteId) | Control(ControlId)` and `Target` is a **small, typed `ParamKey`** (`Volume`, `Pan`, `Pitch`, `SampleStart`, `LoopStart`, `LoopLength`, `MixCell { row, col }`, `EnvStage(Stage, Field)`, `LfoRate`, `LfoPhase`, `RouteDepth`, `RouteBypass`, `Generator(GenParam)`, `Dsp(DspParam)`).
  - **Mod-of-mod falls out for free:** `ModKind::Intensity`, `LfoPhase`, `LfoRate`, `LfoHz`, `LfoDivision`, `Attack`, `Release` ALL collapse into a regular `ModRoute` whose `target` is `ParamAddr { scope: Route(r) | Modulator(m), param: ... }`!
  - **CLAP polyphonic modulation falls out for free:** Any `ParamAddr` exposed to the host can receive a `CLAP_EVENT_PARAM_MOD` per `note_id`, and `Plan` automatically promotes a `Node` from bus/group-shared to per-voice whenever any per-note source or poly-mod targets it (fulfilling `VISION.md`'s *"Per-voice anything"*!).

### 3.2 Unified Envelope & Time-Source Model

Can `Envelope` (AHDSR), `Breakpoints` (Flex), and `ZoneEnvelope` (sample-frame) unify without sacrificing AHDSR's fast-path RT evaluation? **Yes — cleanly:**

```rust
pub enum EnvDomain {
    /// Wall-clock seconds since note-on (AHDSR & Flex).
    Seconds,
    /// Host quarter-note beats (tempo-synced envelopes).
    Beats,
    /// Sample frames: cursor-locked (pitch-follows) or output-rate.
    SampleFrames { speed_lock: bool },
}

pub enum EnvShape {
    /// Fast-path compact AHDSR (exact f32/u32 RT ticker; now supports distinct decay & release curves!).
    Ahdsr {
        delay_s: f32, attack_s: f32, hold_s: f32, decay_s: f32,
        sustain: f32, release_s: f32,
        attack_curve: f32, decay_curve: f32, release_curve: f32,
        floor: f32, depth: f32,
    },
    /// General piecewise exponential breakpoints (Flex, DAHDSR, ZoneEnvelope, UVI AHD).
    Breakpoints {
        domain: EnvDomain,
        bipolar: bool,
        start: f32,
        points: RangeSpan,       // offset+len into Instrument.env_points (flat Vec<EnvPoint>)!
        sustain: Option<u16>,
        loop_range: Option<(u32, u32)>,
        ahdsr_stage_map: bool,   // optional KSP stage-indexed addressing
    },
}
```
- **Why this wins:**
  1. **Zero per-zone bloat:** `ZoneEnvelope` points live in `Instrument.env_points` (one flat `Vec<EnvPoint>`), and zones without envelopes pay **0 bytes** (via a sparse `(ZoneId, Slot, EnvId)` side table) instead of +24 B on every `Zone` in both IR and Plan!
  2. **Fixes UVI's `DecayCurve != ReleaseCurve` bug** ([uvi/modulation.rs:44-47](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/uvi/src/program/modulation.rs:44)) by adding `decay_curve` alongside `release_curve`.
  3. **Keeps `Ahdsr` fast-path:** `prepare` lowers `EnvShape::Ahdsr` straight to `kontra_voice::Ahdsr` with zero overhead, and evaluates `Breakpoints` with one shared curve segment function across all three domains.

### 3.3 Unified Selection, Articulation, & Trigger Predicates

Instead of scattering trigger rules across `Zone`, `Group`, `VoiceGroup`, and `Plan`:
- **Compositional `MatchExpr` (interned boolean AST / RPN predicate table, referenced by a `u16` `RuleId` on Group OR Zone):**
  - `KeyRange(u8, u8)`, `VelRange(u8, u8)`, `ReleaseVelRange(u8, u8)`, `CcRange { cc: u8, lo: u8, hi: u8 }`, `Keyswitch { latch_group: u8, lo: u8, hi: u8, momentary: bool }`, `RoundRobin { scope: RrScope, pos: u8, len: u8, mode: RrMode }` (`RrScope::Instrument | Group | Key | Articulation`, `RrMode::Seq | RandomNoRepeat | Random`), `RandomChance { lo: f32, hi: f32 }` (SFZ `lorand/hirand`!), `Trigger(TriggerKind)` (`Attack | Release | First | Legato | OnCc(u8)`), `Articulation(u16)` (first-class articulation ID for DAW articulation maps / UACC / KS!).
  - Combinators: `And`, `Or`, `Not` (proper precedence, solving Kontakt's untraced mixed AND/OR `ponytail` at [kontakt/translate.rs:930](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/translate.rs:930)!).
- **Unified `Crossfade` table (interned `CrossfadeId`):** Supports `Key`, `Velocity`, `Cc(u8)`, `NoteExpr(ExprKind)`, with `curve: CrossfadeCurve` (`Linear`, `EqualPower`, `Law(LawId)`) — immediately unlocking SFZ `xf_cc`, `xf_velcurve=power`, and DecentSampler fades!
- **Unified `VoicePolicy` (replaces `Instrument::{voices, steal*}`, `Group::polyphony`, `Group::voice_group`, `Group::off_by`, `Group::repeat_cuts`):**
  - Every limit (instrument-wide, group-local, shared voice group, per-key `note_polyphony`, choke `off_by`, mono/legato/last-note-return) is ONE `VoiceGroup` entry with `choke_targets` and `stop_mode: Fade(ms) | Release`.

### 3.4 Dynamic Signal / Bus / FX Graph & Beyond-Stereo Multi-Channel Routing

- **Replace fixed 3-rack + bus list (`inserts`/`sends`/`main` + `buses` + `EffectKind::Sends([f32; 8])`) with a flat topological Audio DAG (`GraphNode` + `Edge`):**
  - Nodes: `VoiceSum { group }`, `Rack { slots: RangeSpan }`, `Split { parallel chains }`, `MixBus { channels: ChannelLayout, volume_db, pan }`, `Output { bus_index }`.
  - Edges: `{ from: NodeId, to: NodeId, gain: f32, tap: PreFader | PostFader, sidechain: bool, channel_map: ChannelMapId }`.
  - **Eliminates UVI's 4-dummy-bus hack** for `EffectRack`/`FeedbackMachine`, supports **Kontakt's `fx_idx_amp_split_point`** (currently dropped with an `ignored` diagnostic at [kontakt/translate.rs:696-698](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/kontakt/src/translate.rs:696)!), supports **sidechain routing** (compressor/ring-mod sidechain from another bus), and supports **arbitrary parallel send/return** without hardcoding 8 sends!
- **Channel-layout neutrality (Beyond Stereo):**
  - Replace hardcoded `[[f32; 2]; 2]` / `[f32; 2]` with `ChannelLayout` (`Mono`, `Stereo`, `Multi(u8)` for close/tree/out/ambient orchestral mic stems, `Surround51`, `Ambisonics(u8)`), with a **zero-overhead stereo fast path** in the audio engine (`ChannelLayout::Stereo` compiles to the exact existing `[f32; 2]` inner loop!).
  - Fixes UVI's `keygroup output past stereo` gap ([uvi/program.rs:730](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/uvi/src/program.rs:730)) and prepares KONTRA for multi-out DAW routing (CLAP/VST3 multi-bus outputs for orchestral mic stems).
- **Consolidate `EffectKind` into `ENGINE.md`'s 7 Universal DSP Families:**
  - `Filter` (unified SVF / Ladder / SallenKey / Cascade / Formant / Clocked / Comb — parameterized by topology & kernel), `Eq` (N-band via flat `Instrument.eq_bands` slice, fixing UVI's 4-band split hack at [uvi/routing.rs:561-567](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/format/uvi/src/program/routing.rs:561)!), `Dynamics` (Compressor / Limiter / Gate / Transient), `Drive` (Shaper / Clipper / Overdrive / Crush / Converter / RingMod), `TimeMod` (Delay / Shifter / Phaser / Notches / Ensemble / Scanner / CombBank), `Space` (Algorithmic Reverb / Convolution), `Utility` (Gain / Stereo / Matrix / Polarity), `BypassPlaceholder { pass_dry: bool }`.
  - Box or store heavy effect configs in a flat `Instrument.effects: Vec<EffectSpec>` indexed by `EffectId(u32)` so rack slots are **4-byte `Option<EffectId>`** instead of 232-byte `Option<Effect>`!

### 3.5 Sound Generators Beyond PCM

Replace `Group::{stream, wavetable, speed}` three-Option soup with a single orthogonal **`Generator`** enum (referenced by `GeneratorId` on Group or overridden on Zone):
- `Pcm { stream: Option<StreamPolicy>, stretch: Option<StretchMode> }` (DFD/RAM + TimeMachine/PSOLA/PhaseVocoder)
- `Wavetable(WavetableSpec)` (with `ModOsc` parameterized cleanly without Kontakt enum discriminants)
- `Oscillator(OscSpec)` (Band-limited VA Sine/Saw/Tri/Pulse/Noise — immediately unlocks SFZ `*sine`/`*noise` and synth presets!)
- `Granular(GranularSpec)` (Falcon / Kontakt granular / cloud playback)
- Future-extensible (`Spectral` / `Physical` / `Neural`) without touching `Zone` layout!

---

## 4. Compactness & Scale (50,000+ Zones) + Native Binary Format

### 4.1 Data-Oriented SoA + Interned Tables Analysis

Applying ECS / SoA + flat arena spans (`RangeSpan { start: u32, len: u16 }`) and small typed handles (`SampleId(u32)`, `GroupId(u16)`, `RuleId(u16)`, `LawId(u16)`, `EffectId(u32)`, `EnvId(u32)`):

| Data Structure | Today (Areia, 42,636 zones) | vNext SoA + Interned Arenas | Reduction |
|---|---:|---:|---:|
| Hot Note-On Match Table (`keys`, `vels`, `group`, `slot`, `flags`, `rule_id`) | Scans 768 B `plan::Zone` (~32.7 MB) + separate `ZoneIndex` | Contiguous `ZoneMatch` (**8 B/zone**) = **341 KB** (fits in L2 cache!) | **~96× smaller hot set** |
| Cold Playback Table (`sample_id`, `start: u32`, `end: u32`, `start_mod: u32`, `gain`, `pan`, `tune`, `root`, `fades`, `loop_span`, `flags`) | Embedded inside 768 B `plan::Zone` | Contiguous `ZonePlay` (**32 B/zone**) = **1.36 MB** | **~24× smaller** |
| Sparse Side Tables (`loops`, `mix`, `wavetable`, `zone_envelopes`, `user_loop_pars`) | Inline in every `Region` & `ZoneFacts`: **~34.2 MB** wasted | Flat arenas indexed only by zones that use them: **< 0.5 MB** on Areia (0 B for `mix`, `wavetable`, `zone_envelopes`, `user_loop_pars`!) | **~68× smaller** |
| Script Zone Metadata (`ZoneFacts`) | **14.7 MB** (separate duplicated array + 42k `Box<str>`) | Read directly from `ZonePlay` + `ZoneMatch` + `Sample` string arena; only **user zones** (≤1024) allocate a `UserZoneState` side table (**0 B** on Areia!) | **Eliminated (~14.7 MB saved)** |
| Interned Tables (`laws`, `transfers`, `rules`, `crossfades`, `effects`) | Cloned per group (e.g. 500× identical velocity/shaper LUTs) | Deduplicated once per `Instrument` via content-addressed arena (`LawId`, `RuleId`, `EffectId`) | **~5–20× smaller** |
| **Total Metadata RAM (IR + Plan)** | **~47–56 MB** + >120k heap allocs | **~2.2–3.0 MB** + ~20 contiguous `Vec` allocations | **~16–20× RAM cut, ~10× faster load/clone** |

*(Note: `start`/`end` as `u32` covers up to 4.29B frames = **24.8 hours at 48 kHz**, with a sparse `u64` overflow map if ever needed; or keep `u64` at 40 B/zone = 1.7 MB, still 19× smaller.)*

### 4.2 2026+ Native Binary Format & Zero-Copy Serialization

`docs/architecture-v3/06-FORMATS.md:69` originally suggested MessagePack for `format-native`. In 2026, for a 50,000-zone Rust sampler, **`rkyv` 0.8 (zero-copy + `bytecheck` validation)** paired with SoA arenas is decisively superior to MessagePack/JSON and more ergonomic than FlatBuffers:
- **Zero-copy `mmap` load (< 1 ms for 50k zones!):** `rkyv` 0.8 uses relative pointers (`Archived<T>`) + `bytecheck` validation (`rancor`), reading SoA `ArchivedVec` slices directly off a memory-mapped `.kontra` file with **zero deserialization `malloc`s** and **zero pointer chasing**.
- **Prerequisite unlocked by this audit:** Removing `KnobModel`'s raw `fn` pointer and replacing `&'static [...]` slices with `Arc<[T]>` / arena `RangeSpan`s makes `kontra-ir` **100% pure serializable data** — enabling:
  1. **`.kontra` native library format** (`rkyv` binary chunk + content-addressed sample pack, plus optional human-readable TOML/JSON export for `kontra-cli inspect --to native` and CI fixtures).
  2. **Instant translated-IR cache:** Translating Areia (`.nki`) or Augmented Orchestra (`.ufs`) once and caching its versioned `rkyv` IR hash turns repeat loads from seconds of XML/NKI parsing + 42k `ignored_fields` formatting into **< 2 ms `mmap` + `prepare`**.

---

## 5. Beyond-2026 Standards & Future Superpowers

### 5.1 MIDI 2.0 / UMP & CLAP 1.2+ Polyphonic Modulation as First-Class Citizens

Today, `kontra-events` already ingests `HostNote`, `NoteExpression { Tune, Gain, Pan, Pressure, Brightness }`, `NoteOffVelocity`, and `Event::Ump([u32; 2])` ([events/src/event.rs:9-82](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/events/src/event.rs:9)), **but `kontra-ir` lags behind**:
- **Missing CLAP 1.2+ capabilities:**
  - `CLAP_NOTE_EXPRESSION_VIBRATO` and `CLAP_NOTE_EXPRESSION_EXPRESSION` are absent from `kontra-events` and `kontra-ir`.
  - `NoteExpression::Brightness` is currently smuggled in via a hardcoded **CC 74 hijack** in [engine/src/voice_gain.rs:302-306](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/engine/engine/src/voice_gain.rs:302) instead of a first-class `ModSource::NoteExpr(ExprKind::Brightness)`.
  - **`CLAP_EVENT_PARAM_MOD` (Polyphonic Parameter Modulation):** Any `ExposedParam` in the IR with `polyphonic: true` can be modulated per-`note_id` by the DAW (`Final = Base + MonoMod + PolyMod`), automatically promoting targeted group DSP slots to per-voice execution.
- **Missing MIDI 2.0 / UMP (32-bit Per-Note) capabilities:**
  - MIDI 2.0 UMP Type 4 messages carry **32-bit per-note Registered Controllers** (128 banks × 128 indices: modulation, breath, timbre/brightness, release time, attack time, decay time, vibrato, etc.), **32-bit per-note Assignable Controllers**, **32-bit per-note Pitch Bend**, and **Note-On Attribute Type/Data** (e.g., Attribute Type 3 = 7.9 fixed-point per-note pitch for microtonal tunings!).
  - Making `ModSource::PerNoteCc { registered, bank, index }` and `ModSource::ReleaseVelocity` first-class in `kontra-ir` means **every legacy Kontakt/UVI/SFZ library** can immediately map MPE, CLAP expressions, or MIDI 2.0 per-note controllers onto any filter, pitch, wavetable, or FX parameter via user overrides — directly delivering `VISION.md`'s *"Go beyond what the originals can do"*.

### 5.2 Declarative Behaviour vs. Imperative Scripting

- **What should compile to declarative IR (zero script VM needed):**
  - DecentSampler `<binding>`s (UI knob → `ParamAddr` via `LawId`), keyswitches, round-robins, CC crossfades, velocity curves, macro-to-multi-target fanouts, simple legato/portamento, and release triggers.
  - Even for KSP/UVI: today `Instrument::knob_mods`, `Script::links` (`CcLink`), and `OutputControllers` (which Kontakt itself implements as generated KSP!) already prove that lifting static bindings out of scripts into declarative `ControlBinding`s eliminates VM overhead and makes them sample-accurate and inspectable.
- **How imperative scripts (KSP, UVI Lua, future WASM/native scripts) should bind to IR:**
  - Move **all** KSP `$ENGINE_PAR_*` / `$ZONE_PAR_*` / `$LOOP_PAR_*` / `0..=1_000_000` / `1024+bus` / `Readout` / `Held` / `SlotShapes` tables OUT of `kontra-ir` and `kontra-plan` into a **format-owned `ScriptAdapter` / `ParMap`** (compiled once at prepare time into a compact `(VendorId → ParamAddr + LawId)` table!).
  - Both KSP and UVI Lua then drive the **exact same `ParamAddr` API** — deleting `EnginePar`, `ZonePar`, `SlotKnob`, `SlotLaw`, `KnobModel`, `RouteNames`, `par_values.rs`, and UVI's 23-point `PROBE_GRID` hack in one stroke!

### 5.3 Deterministic Introspection, Diffing, & Law Provenance

- Because every `Law` is interned in `Instrument.laws: Vec<LawEntry { id: Option<LawId>, law: Law }>` and every `ModRoute`/`Effect`/`ControlBinding` carries clean typed IDs + `Diagnostic`s:
  - **`kontra-inspect` / `kontra-cli`** can dump, validate, and structurally diff any two `Instrument`s (e.g. `.nki` vs `.kontra` round-trip, or before/after a translator PR) deterministically!
  - **Provenance** becomes a zero-cost optional sidecar (`LawId` already on each interned `LawEntry`) rather than scattered `kontra_law::record(|| format!(...))` calls allocating strings per zone.

---

## 6. KONTRA IR vNext: Concrete Design & Phased Migration Plan

### 6.1 Concrete Rust Type Sketch (`kontra-ir` vNext)

Compact, 100% serializable (`serde` + `rkyv`), zero vendor leaks, zero function pointers, zero `&'static` slices:

```rust
// --- Typed compact handles (all Copy, Eq, Hash, rkyv::Archive) ---
#[repr(transparent)] pub struct SampleId(pub u32);
#[repr(transparent)] pub struct GroupId(pub u16);
#[repr(transparent)] pub struct ZoneId(pub u32);
#[repr(transparent)] pub struct NodeId(pub u16);
#[repr(transparent)] pub struct EffectId(pub u32);
#[repr(transparent)] pub struct ModulatorId(pub u16);
#[repr(transparent)] pub struct RouteId(pub u32);
#[repr(transparent)] pub struct LawId(pub u16);
#[repr(transparent)] pub struct RuleId(pub u16);
#[repr(transparent)] pub struct EnvId(pub u32);
#[repr(transparent)] pub struct MacroId(pub u16);
#[repr(transparent)] pub struct RangeSpan { pub start: u32, pub len: u16 }

pub struct Instrument {
    pub version: u16,
    pub name: String,
    pub source_path: Option<PathBuf>,
    pub library_path: Option<PathBuf>,

    // --- Interned flat arenas (zero per-zone heap allocs, rkyv-friendly) ---
    pub samples: Vec<Sample>,                 // NO Keystream::Blocks DRM inside IR!
    pub laws: Vec<InternedLaw>,               // unified Law (analytic, LUT, breakpoints)
    pub rules: Vec<MatchNode>,                // boolean AST for keyswitch/CC/RR/random/trigger
    pub crossfades: Vec<Crossfade>,           // key/vel/CC/expr + Linear/EqualPower/LawId
    pub loops: Vec<Loop>,                     // flat arena referenced by ZonePlay::loops
    pub env_points: Vec<EnvPoint>,            // flat arena for Breakpoints & ZoneEnvelopes
    pub envelopes: Vec<EnvSpec>,              // unified AHDSR + Breakpoints + SampleFrames
    pub modulators: Vec<Modulator>,           // LFOs, Envelopes, StepSeqs, SignalExprs
    pub effects: Vec<EffectSpec>,             // 7 universal DSP families (no &'static, no fn ptr)
    pub eq_bands: Vec<Bell>,                  // variable N-band EQ arena
    pub impulses: Vec<Impulse>,
    pub responses: Vec<(String, SampleId)>,

    // --- SoA Zone tables (hot match vs cold play + sparse extensions) ---
    pub zones_match: Vec<ZoneMatch>,          // 8 B/zone -> 341 KB for Areia!
    pub zones_play: Vec<ZonePlay>,            // 32–40 B/zone -> 1.4–1.7 MB for Areia!
    pub zones_mix: Vec<(ZoneId, RangeSpan)>,  // sparse channel matrix (0 B on Kontakt/SFZ)
    pub zones_env: Vec<(ZoneId, u8, EnvId)>,  // sparse ZoneEnvelopes (0 B on Areia)
    pub zone_script_ids: Vec<u32>,            // optional sparse/parallel vendor zone IDs

    // --- Musical hierarchy, routing DAG & voice policies ---
    pub groups: Vec<Group>,
    pub voice_groups: Vec<VoiceGroup>,        // unifies instrument/group/choke/mono limits
    pub graph: AudioGraph,                    // flat DAG: VoiceSum -> Rack/Split -> MixBus -> Out
    pub routes: Vec<ModRoute>,                // ONE unified route list (all scopes!)
    pub macros: Vec<Macro>,                   // first-class host/CLAP/UI/script macros
    pub bindings: Vec<ControlBinding>,        // declarative UI/CC/macro -> ParamAddr via LawId

    // --- Sandboxed scripting + opaque format adapter metadata ---
    pub scripts: Vec<Script>,
    pub script_bindings: Option<ScriptBindings>, // VendorId -> (ParamAddr, LawId, Readout)
    pub diagnostics: Vec<Diagnostic>,
}

/// 8 bytes — hot note-on scan table (fits L2 cache for 50k+ zones).
#[derive(Clone, Copy)]
pub struct ZoneMatch {
    pub key_lo: u8, pub key_hi: u8,
    pub vel_lo: u8, pub vel_hi: u8,
    pub group: GroupId,
    pub rule: RuleId,                         // RuleId::ALWAYS = 0 (inherits group rule)
}

/// 40 bytes — cold playback record (read only for matched zones).
#[derive(Clone, Copy)]
pub struct ZonePlay {
    pub sample: SampleId,
    pub start: u32, pub end: u32, pub start_mod: u32,
    pub gain_db: f32, pub pan: f32, pub tune_semitones: f32,
    pub root: u8, pub flags: u8,              // reverse, one_shot, offset_limited, ...
    pub vel_fade: [u8; 2], pub key_fade: [u8; 2],
    pub slot: u16,
    pub loops: RangeSpan,                     // slice into Instrument::loops
    pub crossfades: RangeSpan,                // slice into Instrument::crossfades
}

/// ONE atomic modulation route for voice, group, bus, FX, mod-of-mod, and CLAP poly-mod.
#[derive(Clone, Copy)]
pub struct ModRoute {
    pub scope: Scope,                         // Group(g) | Zone(z) | Bus(b) | Global
    pub source: ModSource,                    // Vel, CC, NoteExpr, PerNoteCc, PolyMod, Modulator, Macro...
    pub transform: LawId,                     // LawId::IDENTITY = 0; replaces Curve + Transfer!
    pub target: ParamAddr,                    // ONE address space for every engine/DSP/mod param
    pub depth: f32,
    pub span: f32,                            // physical units per unit depth (1.0 if direct)
    pub combine: CombineOp,                   // Add | Mul | MulSigned | ExpOctaves | ScaleDepth
    pub smooth_ms: f32,
    pub flags: RouteFlags,                    // bypassed, bipolar_in, invert, note_on_only
}
```

### 6.2 Side-by-Side Format Mapping onto IR vNext

| Concept | Kontakt (`.nki`) | UVI / Falcon (`.ufs`) | SFZ v2 (`.sfz`) | DecentSampler (`.dspreset`) | Native (`.kontra`) |
|---|---|---|---|---|---|
| **Zones & Ranges** | `ZoneMatch` + `ZonePlay` + `zone_script_ids` | `ZoneMatch` + `ZonePlay` + sparse `zones_mix` (GainMatrix) | Direct `ZoneMatch` + `ZonePlay` (no `O(N²)` group dedup needed when overrides live via `RuleId`!) | `<sample>` → `ZoneMatch` + `ZonePlay`; `<group>` → `Group` | Direct `rkyv` zero-copy `mmap` of `Instrument` |
| **Envelopes** | Amp → `EnvShape::Ahdsr`; Flex → `Breakpoints(Seconds)`; ZoneEnv → `zones_env` + `Breakpoints(SampleFrames)` | DAHDSR → `Ahdsr` (distinct `decay_curve`/`release_curve`!); AHD → `Breakpoints(Seconds)` | `ampeg_*` → `Ahdsr` (`delay_s` supported!) | `<modulator type="adsr">` → `Ahdsr` | Native `EnvSpec` |
| **Modulation** | Internal/External mods → `ModRoute` w/ interned `LawId` (shaper) + `ModRoute` targeting `Route(r)` for intensity/LFO rate | `SignalConnection` + `Mapper` + `Macro` → `ModRoute` + `MacroId` + `SignalExpr` (no `Script(0x8000)` or `#key` hacks!) | `amp_veltrack`, `cutoff_ccN`, `pitch_lfo`, `xfin_loccN` → `ModRoute` + `Crossfade` | `<modulator>` + `<binding>` → `ModRoute` + `ControlBinding` | Native `ModRoute` + CLAP `PolyMod` + MIDI 2.0 `PerNoteCc` |
| **Routing / FX** | 3-rack + 8 sends + `fx_idx_amp_split_point` → `AudioGraph` nodes/edges | `Layer` / `Aux` / `EffectRack` / `FeedbackMachine` → `AudioGraph` `Split` + `Edge` (kills 4-dummy-bus hack!) | Per-region/group filter & sends → `Rack` node + `Edge` | `<effects>` (global/group) → `Rack` node | Arbitrary multi-channel `AudioGraph` DAG |
| **Scripting & Params** | KSP `$ENGINE_PAR_*` / `$ZONE_PAR_*` → `ScriptBindings` table `(VendorId → ParamAddr + LawId)` | Lua `setParameter` → `ScriptBindings` by name → `(ParamAddr, LawId)` (kills 23-point `PROBE_GRID`!) | None (100% declarative!) | Declarative `<binding>`s → `ControlBinding` (zero VM!) | Optional WASM/Lua + direct `ParamAddr` |

### 6.3 Phased, Incremental Migration Roadmap (Vetted by Vision Design Test & KISS/YAGNI)

Every phase is independently shippable, preserves bit-exact parity tests, and earns its keep with zero speculative scaffolding:

#### **Phase 0 — Immediate Zero-Breakage Hygiene & Vendor-Leak Removal (Low Risk, High ROI)**
1. **Remove `Keystream::Blocks` decryption from `kontra-ir`** ([ir/src/lib.rs:231](/data/dev/workspace/worktrees/matari/kontra-fundamentals-audit/plugins/kontra/crates/model/ir/src/lib.rs:231)): move UVI decryption into `format-uvi`'s `kontra_stream::Source` wrapper.
2. **Replace `&'static [...]` slices in `ir` with `Arc<[T]>`** (`Curve::Table`, `Overdrive::curve`, `Delay::{body, shape}`, `Ensemble::voices`, `Network`, `Sections::fixed`) and replace `KnobModel`'s raw `fn` pointer with declarative `SlotLaw` / parametric descriptors so `kontra-ir` becomes 100% serializable data.
3. **Add `ir::validate(&Instrument) -> Vec<Diagnostic>`** in `kontra-ir` catching dangling indices, bus cycles, `start > end`, and `>WARPS` silently dropped modulators.
4. **Fix two immediate Areia performance/memory bugs:**
   - Remove `loop_pars: [LoopPar; 8]` (192 B/zone = **8.2 MB** dead RAM!) from `ZoneFacts` into a sparse `HashMap<u32, [LoopPar; 8]>` for user zones only.
   - Replace `Region`'s inline `SmallVec<[Loop; 8]>` (264 B) / `Option<Mix>` (132 B) / `Option<Table>` (208 B) with `Option<Box<...>>` or `Arc<...>` (cuts `plan::Zone` from **~768 B → ~160 B**, saving **~26 MB** on Areia immediately with almost zero API churn!).
   - Gate `Notes::ignored_fields` behind a debug/audit flag or typed field visitor so Areia doesn't run `format!("{z:#?}")` 42,636 times on normal load.
- *Vision Test:* (1) Single implementation, removes vendor code from IR; (2) Existing oracle/null tests remain bit-identical; (3) Saves ~34 MB RAM on Areia and cuts load allocations; (4) Unblocks `format-native` serialization; (5) Adds structural IR validation diagnostics.

#### **Phase 1 — Atomic Parameter Addressing, Unified Modulation & Envelope Core**
1. Introduce unified **`ParamAddr`** + **`ModRoute`** (replacing 20-variant `ModKind`, `EffectParam`, `SlotKnob`, `KnobTarget`, `EnginePar`, `ZonePar`, `TableValue`, `ImpulseKnob`) and unify `ir::Curve` + `ir::Transfer` onto interned `kontra_law::Law`.
2. Move KSP's `par_values.rs`, `par_laws.rs` fallbacks, `ZonePar` units (`-3600..3600`, `520..=525`, `guess_bpm`), `Readout`, and `NOTE_LADDER` out of `kontra-ir` / `kontra-plan` into `format-kontakt`'s `ScriptBindings` adapter.
3. Unify `Envelope` + `Breakpoints` + `ZoneEnvelope` (from the `kontra-zone-envelopes` branch!) into **`EnvSpec`** (`Ahdsr` w/ separate `decay_curve`/`release_curve` + `Breakpoints` w/ `EnvDomain::{Seconds, Beats, SampleFrames}`), storing `ZoneEnvelope`s sparsely instead of a `Vec` on every `Zone`.
4. Add first-class **`ModSource::{NoteExpr, ReleaseVelocity, PerNoteCc, PolyMod, Macro}`** — deleting the CC74 brightness hack in `voice_gain.rs:302`, UVI's `Script(0x8000+...)` macro hack, and `ir::RouteNames`!
5. Unify `MatchExpr` + `Crossfade` (supporting `EqualPower` and `Cc` crossfades from IR!) + `VoicePolicy` (unifying instrument/group/voice-group limits).
- *Vision Test:* (1) Collapses 10 overlapping target enums and 3 envelope types into 1 each; (2) Bit-exact against Kontakt & UVI golden vectors + unlocks UVI distinct decay/release curves; (3) Eliminates `WARPS = 8` truncation and UVI's 23× `PROBE_GRID` load-time probing; (4) First-class CLAP 1.2+ poly-mod & MIDI 2.0 UMP per-note CCs; (5) Every unmapped vendor parameter cleanly emits a structured `Diagnostic`.

#### **Phase 2 — Compact SoA Layout, Format-Author SDK & `format-native` (`rkyv`)**
1. Migrate `ir::Zone` + `plan::Zone` + `plan::ZoneFacts` to the **SoA `ZoneMatch` (8 B) + `ZonePlay` (40 B) + sparse side-table** architecture; remove `ZoneFacts` duplication (total Areia metadata **~2.5 MB**, ~18× reduction).
2. Build the **Format-Author SDK in `kontra-format`**: shared audio header probe (WAV/FLAC/AIFF/OGG via a lightweight header crate), `InstrumentBuilder` with sensible defaults & automatic arena deduplication (`intern_law`, `intern_rule`, `intern_sample`), shared MIDI note/dB/pan/fade helpers, and declarative `map_effects!` / `map_params!` tables (as envisioned in `06-FORMATS.md §2`).
3. Implement **`format-native` (`.kontra`)** using `rkyv` 0.8 + `bytecheck` for zero-copy `mmap` load, plus a JSON/TOML debug export for `kontra-cli inspect` and an optional translated-IR disk cache.
4. Prove ergonomics by implementing a clean DecentSampler (`.dspreset`) or SFZ v2 full-opcode translator in <1,000 LOC using the SDK.
- *Vision Test:* (1) Single SoA representation shared across IR and Plan; (2) Round-trip `.nki → .kontra → Plan` bit-identical render test; (3) ~18× metadata RAM reduction, sub-2 ms cached load, L2-resident zone lookup; (4) Delivers `VISION.md`'s open native format; (5) Diagnostics survive serialization intact.

#### **Phase 3 — Dynamic Multi-Channel `AudioGraph` DAG & Generalized Generators**
1. Replace `Instrument::{inserts, sends, main, buses}` + `EffectKind::Sends([f32; 8])` with flat **`AudioGraph` (`GraphNode` + `Edge`)**, supporting pre/post-amp splits (`fx_idx_amp_split_point`), parallel racks (`EffectRack`/`FeedbackMachine` without dummy buses), sidechain feeds, and **channel-layout neutrality** (`Mono` / `Stereo` fast path / `Multi(N)` orchestral stems / Surround / Ambisonics).
2. Replace `Group::{stream, wavetable, speed}` with **`Generator`** (`Pcm`, `Wavetable`, `Oscillator`, `Granular`), unlocking SFZ `*sine`/`*noise` and Falcon synthesis oscillators.
3. Consolidate `EffectKind`'s 31 variants into `ENGINE.md`'s 7 universal family configs (indexed by 4-byte `EffectId`) — keeping separate DSP recurrences **only** where oracle/null measurements prove an audible difference (honoring the KISS/YAGNI caveat!).
- *Vision Test:* (1) Single graph & generator abstraction for every format; (2) Unlocks Kontakt amp-split and UVI multi-out/parallel rack parity tests; (3) Zero overhead on stereo fast path, smaller rack slots (4 B vs 232 B); (4) Opens multi-out stems, sidechain, and synth/granular expansion; (5) Any unsupported channel layout or generator mode reports a clear `Diagnostic`.

---

### Summary Verdict

`kontra-ir` is well-disciplined in spirit (clean comments, strong oracle evidence, explicit `Diagnostic` reporting), **but under real Kontakt + UVI feature pressure it has drifted into a Kontakt-shaped, AoS, unserializable intermediate layer** with ~190+ overlapping parameter enum variants, 3 incompatible envelope types, a ~47 MB metadata footprint on Areia (80%+ dead inline bytes), raw function pointers blocking `format-native`, and UVI/Kontakt vendor leaks inside both `kontra-ir` and `kontra-plan`.

Executing **Phase 0 → Phase 3** cleanly restores KONTRA's Vision Mantra: **one compact, zero-copy, atomic, 2026+-native IR** where wiring a new library format is a thin, declarative mapping table and every legacy library automatically gains CLAP polyphonic modulation, MIDI 2.0 per-note expression, multi-channel routing, and 15–20× leaner memory.
