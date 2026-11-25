//! Kontakt's measured laws, mapped onto the sampler's neutral shapes. Each
//! names the evidence it rests on; the IR and engine never see the vendor.

use kontra_ir as ir;
use kontra_law::common::{ATTACK_CURVE, CUBE};
use kontra_law::kontakt::{
    EQ_PEAK_DB, FILTER_NOTE_HZ, FLEX_SHAPE, LADDER_GAIN, LEGACY_CUTOFF_HZ, LEGACY_RESONANCE_LOSS,
    LEGACY_RESONANCE_Q, UNIT_FIFTH,
};
use ni_file::kontakt::objects::{
    EnvelopeAhdsr, EnvelopeDbd, EnvelopeFlex, Glide, Lfo, ShaperCurve, StepMod,
};

/// Kontakt's decay and release: the kernel runs its state from 1.075 down to
/// 0.075, a ratio of 3/43, so the stage is silent exactly at its time
/// (docs/re/engine/k-mod-envelopes.md).
const FALL: f32 = -2.662_588;

/// A volume AHDSR. Stored times are milliseconds and the sustain a gain:
/// Kontakt 8.13 plays a stored 0.25 at -12.0 dB (the cube is the KSP knob's
/// law, `par_laws::CUBE`; scratch/laneDSP/b22/env_law.lua case 7).
pub(crate) fn envelope(e: &EnvelopeAhdsr) -> ir::Envelope {
    let seconds = |ms: f32| ms.max(0.0) / 1000.0;
    ir::Envelope {
        attack: seconds(e.attack_ms),
        hold: seconds(e.hold_ms),
        decay: seconds(e.decay_ms),
        sustain: e.sustain_level(),
        release: seconds(e.release_ms),
        attack_shape: ATTACK_CURVE.eval_unclamped(e.attack_curve),
        decay_shape: FALL,
        release_shape: FALL,
        lag: ENVELOPE_LAG_S,
        delay: 0.0,
        floor: 0.0,
        depth: 1.0,
        one_shot: e.unknown_flag != 0,
    }
}

/// The amplitude envelope's depth from its volume route: intensity `i` and
/// byte 3 of the internal modulator's flags. Kontakt 8's legacy-modulation
/// compiler (FUN_140873040, AHDSR type 2 with mode +0x600 < 2) emits, with the
/// byte set (+0x548), `$out := $in * env * $intensity` (template 0x144f1da68),
/// a scaled envelope; clear, `$out := $in * (1.0 - (1.0 - env) * $intensity)`
/// (0x144f1daa0), the envelope above a floor of `1 − i`. Both are the plain
/// envelope at full intensity, which is every Areia, Dolce, Pacific and probe
/// route in the corpus census. Note: mode `+0x600 == 2` (`* 3.0` signal) is
/// `BParEnv_DBD` (`0140870000.c:2914-2922, 3970-3975`), translated via [`dbd`]
/// and `timed.rs` rather than AHDSR.
pub(crate) fn envelope_depth(e: &mut ir::Envelope, intensity: f32, scaled: bool) {
    let a = intensity.abs().min(1.0);
    if intensity < 0.0 {
        e.depth = -a;
        e.floor = if scaled { a } else { 1.0 };
    } else {
        e.depth = a;
        e.floor = if scaled { 0.0 } else { 1.0 - a };
    }
}

/// A Glide modulator on the source's pitch, at `depth` (its signed intensity).
/// Measured in Kontakt 8.13 (oracle, tools/kontakt-oracle/probes/glide.lua: a
/// 261.6 Hz sine, notes 4 and 12 semitones apart, YIN pitch track):
/// - the pitch moves linearly in semitones (within 0.02 st of a line);
/// - Time mode (stored 2) takes the stored milliseconds whatever the interval
///   (1224 ms stored: 1208 ms over 12 st, 1238 ms over 4 st; 47.5, 142.3 and
///   418.7 ms likewise within 4%);
/// - Speed mode (stored 1) takes them per semitone (142.3 ms: 140–144 ms a
///   semitone over 4 and 12; 418.7 ms: 413–423);
/// - it starts from the previous note's key even when that key is up (0.5 s
///   after its release) or its own glide is unfinished, never from the pitch
///   reached.
///
/// glide_intensity_groups.lua and glide_range.lua (same rig):
/// - intensity is a signed share of the interval over the same time: 50%
///   slides 6 st of a 12 st step in ~420 ms, 25% 3 st; a negative intensity
///   starts beyond the note (−50% on 60 → 72 from 78);
/// - a slide covers at most 12 st: jumps of 17, 19, 20, 24 and 38 st all slid
///   12 st in the same time. Applying the cap before the intensity is
///   inferred (an intensity on a jump wider than 12 st was not measured);
/// - every group glides from the instrument's previous note, one it never
///   played too (kept from it by `$EVENT_PAR_ALLOW_GROUP` or its key range),
///   and a note a script ignores still counts as the previous one: scope 0.
pub(crate) fn glide(g: &Glide, depth: f32) -> ir::Glide {
    ir::Glide {
        scope: 0,
        depth,
        range: 12.0,
        from_heard: true,
        seconds: g.values[0].max(0.0) / 1000.0,
        per_semitone: g.mode == 1,
        legato: false,
        resume: false,
        curve: 0.0,
    }
}

/// A Kontakt attack is silent for its first 33 frames at 48 kHz (exact
/// zeros) and then runs its curve in the time left, so it is still full on
/// time: a 10 ms attack leaves zero at frame 33 and is full at 480. A note
/// without attack is full on its first frame, and a key-up releases at once
/// (Kontakt 8.13, env_law.lua: attack curves -100..+100 %, 10 ms to 1 s;
/// note-offs scatter 9-37 frames, so no release lag is read into them).
pub(crate) const ENVELOPE_LAG_S: f32 = 33.0 / 48_000.0;

/// A route's time constant from its saved lag: the lag is the time its gain
/// takes to settle within 1%, so τ = lag / ln 100. Measured in Kontakt 8.13
/// on CC1 and CC11 volume routes with lag 120: a one-pole on the gain,
/// τ 25.4–26.1 ms both ways (oracle, 2 kHz sine zone in the library's group).
/// A pitch-bend route with lag 250 glides its semitones the same way: τ 53 ms
/// (63% at 52–53 ms, 90% at 122–124 ms, up and down, ±2 st).
pub(crate) fn route_tau_ms(lag_ms: i16) -> f32 {
    f32::from(lag_ms.max(0)) / 100f32.ln()
}

/// The volume law of a modulation source (Kontakt's "family H",
/// modulation compiler 0x140875ac8/0x140873120, `0140870000.c:2928`):
/// cubed (`fcubic`) for internal source types 7..=13, plain otherwise. The
/// loader FUN_140cf0420 maps saved codes to those types: key, velocity,
/// release velocity, the release counter, constant and the randoms land on
/// 7..=13; pitch bend (1), poly/mono aftertouch (2, 3), controllers (4) and
/// scripts (5) stay plain. The cube at 50/127 matches the measured velocity
/// table to 0.03 dB RMS.
/// Family H `fcubic` exponent selected by external source type (`Kontakt8.exe`
/// `0x140873120` / `0x140876753`):
/// - External types 7..=13 (key, velocity, release velocity, release counter,
///   constant, unipolar/bipolar random) apply `fcubic` (`power = 3`).
/// - Types 1..=4 (pitch bend, poly/mono aftertouch, MIDI CC), Script (5), and
///   ZoneEnvelope stay linear (`power = 1`).
pub(crate) fn source_power(source: ir::Source) -> u8 {
    if matches!(
        source,
        ir::Source::Controller(_)
            | ir::Source::ControllerFrom { .. }
            | ir::Source::Script(_)
            | ir::Source::ScriptBipolar(_)
            | ir::Source::PitchBend
            | ir::Source::ZoneEnvelope(_)
            | ir::Source::Pressure
    ) {
        kontra_law::kontakt::CONTROLLER_VOLUME_POWER
    } else {
        kontra_law::kontakt::VOICE_VOLUME_POWER
    }
}

pub(crate) fn volume(source: ir::Source) -> ir::Curve {
    ir::Curve::Depth {
        power: source_power(source),
    }
}

/// A Kontakt per-zone envelope (`BZoneLevelEnv`) mapped onto the neutral
/// sample-position envelope. `BZEnvMod` (`FUN_140ceb290`) shapes each segment
/// with `(e^(k·t) − 1)/(e^k − 1)` where `k = 16·curve` when rising (`y1 ≥ y0`)
/// and `k = −16·curve` when falling (`y1 < y0`) (`DAT_144714a10` /
/// `DAT_144fd0f08`).
pub(crate) fn zone_envelope(
    e: &ni_file::kontakt::objects::ZoneEnvelope,
) -> Option<ir::ZoneEnvelope> {
    let slot = u8::try_from(e.ext_mod_idx).ok()?;
    if e.points.is_empty() {
        return None;
    }
    let points = e
        .points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let falling = e.points.get(i + 1).is_some_and(|next| next.y < p.y);
            let scale = if falling { -16.0 } else { 16.0 };
            ir::ZonePoint {
                frame: p.x,
                value: p.y,
                curve: p.curve * scale,
            }
        })
        .collect();
    Some(ir::ZoneEnvelope {
        slot,
        points,
        loop_range: (e.loop_active && e.loop_length > 0).then_some((e.loop_start, e.loop_length)),
        speed_lock: e.speed_lock,
    })
}

/// A route to an AHDSR time parameter (attack, hold, decay, release) multiplies
/// its log knob `u` (where `t = 2·e^(C·u) − 2` ms, `fconv_u2n_env_a`/`d`) by
/// `m = 1 − (1 − x')·|i|` in modern Kontakt (`+0x53d == 0`, `0140870000.c:4325-4352`).
pub(crate) const ATTACK_FLOOR_MS: f32 = 2.0;

/// Envelope-time scaling law selected by external modulator flag `+0x53d`
/// (`simulate_k2_env_time`, `0140870000.c:4325-4352`) and source type
/// (`0x140873120` / `0x140876753`):
/// - `false` (modern K3–K8): scales the logarithmic time knob by `m^power`
///   (`LogKnob { floor_ms: 2.0, power }`), with `power = 3` (`fcubic`) for
///   external source types 7..=13 and `power = 1` for types 1..=5 / ZoneEnvelope.
/// - `true` (Kontakt 2 compat): wraps in `fconv_u2n`/`fconv_n2u` with `fcubic`,
///   scaling duration linearly by `m³` (`Linear { power: 3 }`).
pub(crate) fn time_scale(source: ir::Source, simulate_k2: bool) -> ir::TimeScale {
    if simulate_k2 {
        ir::TimeScale::Linear { power: 3 }
    } else {
        ir::TimeScale::LogKnob {
            floor_ms: ATTACK_FLOOR_MS,
            power: source_power(source),
        }
    }
}

/// The release-trigger counter: `max(T − held, 0)/T` in a release-trigger
/// group with a time T, else a constant 1 (binary 0x14084f380).
pub(crate) fn release_counter(release_trigger: bool, ms: i32) -> ir::Source {
    u32::try_from(ms)
        .ok()
        .filter(|&ms| release_trigger && ms > 0)
        .map_or(ir::Source::Constant, |span_ms| ir::Source::HeldTime {
            span_ms,
        })
}

/// Residency by source module (docs/re/engine/k-sampler-source.md): Sampler
/// (mode 0) keeps whole samples in memory; DFD (3) and the other modules keep
/// the program's preload per channel, plus twice the S.Mod range, in 6144-byte
/// grains. ponytail: a Sampler group whose resident flag is clear streams in
/// Kontakt too (0x140760000:19175); that flag is not decoded yet.
pub(crate) fn stream(mode: u32, preload: i32) -> Option<ir::Stream> {
    (mode != 0).then(|| ir::Stream {
        bytes_per_channel: u32::try_from(preload).unwrap_or(61_440),
        grain: 6144,
    })
}

/// Semitones at full intensity of a pitch route. The corpus's pitch-bend
/// routes store 2/12, 7/12 and 12/12 (bend ranges of 2, 7 and 12 semitones).
pub(crate) const PITCH_SEMITONES: f32 = 12.0;

/// Script index of a group's first external modulator: Kontakt numbers the
/// 16-slot internal array first, then each external slot after it, empty
/// slots kept. Real Kontakt 8 (`probes/find_mod_index.lua`, ANALOG STRINGS
/// group 0): `find_mod` answers 16 for external slot 0, 33 for slot 17.
pub(crate) const EXTERNAL_MOD_BASE: u16 = 16;

/// One band of the 1/2/3-band EQ. The native coefficients (generator
/// 0x1409591c0) are the cookbook bell with A = 2^(dB/6) and
/// alpha = sin(w)·sinh(octaves·ln 2): twice the knob's dB at the centre and
/// twice its octaves of bandwidth, unwarped (docs/re/engine/eq-kontakt.md
/// §1.1, fitted +12 dB/1 oct → 24.08 dB, Q 0.667).
pub(crate) fn eq_band(hz: f32, octaves: f32, db: f32) -> Option<ir::Bell> {
    if !(hz > 0.0 && octaves > 0.0) {
        return None;
    }
    let peak = EQ_PEAK_DB.eval_unclamped(db.clamp(-18.0, 18.0));
    Some(ir::Bell {
        shape: ir::Shape::Peak,
        hz: hz.min(20_000.0),
        q: 1.0 / (2.0 * (octaves * std::f32::consts::LN_2).sinh()),
        gain_db: peak,
    })
}

/// Convolution Auto Gain (0x1419b2bc0): from the louder channel's energy E,
/// sqrt(0.5 / E) capped at 2; unity at E ≤ 0.001.
pub(crate) const AUTO_GAIN: ir::Normalize = ir::Normalize {
    energy: 0.5,
    max_gain: 2.0,
    floor: 0.001,
    channels: ir::EnergyChannels::Loudest,
    after_resample: false,
};

/// An AHDSR as breakpoints, for targets other than the amplitude: the same
/// times, shapes and cubed sustain as [`envelope`]. In AHD-only mode
/// (`one_shot`, `0140ae0000.c:3170-3179`, `4197-4205`), sustain is `None` and
/// decay targets `0.0`.
pub(crate) fn ahdsr(e: &EnvelopeAhdsr) -> ir::Breakpoints {
    let a = envelope(e);
    let point = |s: f32, level, curve| ir::Breakpoint {
        ms: s * 1000.0,
        level,
        curve,
    };
    let (decay_level, release_ms, sustain) = if a.one_shot {
        (0.0, 0.0, None)
    } else {
        (a.sustain, a.release, Some(2))
    };
    ir::Breakpoints {
        start: 0.0,
        points: vec![
            point(a.attack, 1.0, a.attack_shape),
            point(a.hold, 1.0, 0.0),
            point(a.decay, decay_level, a.decay_shape),
            point(release_ms, 0.0, a.release_shape),
        ],
        sustain,
        loop_from: None,
        stages: true,
    }
}

/// A DBD (Decay-Break-Decay) envelope as breakpoints (`FUN_140cffd40` /
/// `FUN_1409651b0` / `FUN_140ae5d70`, `0140870000.c:3970-3975`).
///
/// The native kernel starts at `1/3`, ramps linearly over `values[0]` ms to
/// `values[1]` (break level), then linearly over `values[2]` ms to `0`; a
/// nonzero flag zeroes the level and both durations (`k-mod-envelopes.md`).
/// The modulation compiler multiplies the output by `3.0` on standard targets
/// (and `9.0` on pitch, handled in `timed.rs` as `3 × PITCH_SEMITONES`), so
/// scaling start (`1/3`) and break by `3.0` normalizes start to `1.0`.
pub(crate) fn dbd(e: &EnvelopeDbd) -> ir::Breakpoints {
    if e.flag.is_some_and(|f| f != 0) {
        return ir::Breakpoints {
            start: 0.0,
            points: Vec::new(),
            sustain: None,
            loop_from: None,
            stages: false,
        };
    }
    let decay1_ms = e.values[0].max(0.0);
    let break_level = e.values[1] * 3.0;
    let decay2_ms = e.values[2].max(0.0);
    ir::Breakpoints {
        start: 1.0,
        points: vec![
            ir::Breakpoint {
                ms: decay1_ms,
                level: break_level,
                curve: 0.0,
            },
            ir::Breakpoint {
                ms: decay2_ms,
                level: 0.0,
                curve: 0.0,
            },
        ],
        sustain: None,
        loop_from: None,
        stages: false,
    }
}

/// A flex envelope as breakpoints. Each point's curve knob `c` (0.5 linear)
/// sets the segment ratio `q` of the native recurrence (0x140ae5a10), whose
/// closed form is the shape `ln q`. The envelope starts from its last
/// point's level and holds at the sustain point, or with the saved loop flag
/// (the low byte of the v0x12 tail word) cycles from the point before it
/// (0x140ae57dd, docs/re/kontakt-flex-loop-jump.md).
pub(crate) fn flex(e: &EnvelopeFlex) -> ir::Breakpoints {
    let points: Vec<_> = e
        .points
        .iter()
        .map(|p| ir::Breakpoint {
            ms: p.time_ms.max(1.0),
            level: p.level.clamp(0.0, 1.0),
            curve: FLEX_SHAPE.eval_unclamped(p.curve),
        })
        .collect();
    let sustain = usize::try_from(e.sustain)
        .ok()
        .filter(|&s| s < points.len());
    let looped = e.unknown_version_word().is_some_and(|w| w & 0xFF != 0);
    let loop_from = usize::try_from(e.unknown_index)
        .ok()
        .filter(|&from| looped && sustain.is_some_and(|s| from <= s));
    ir::Breakpoints {
        start: points.last().map_or(0.0, |p| p.level),
        sustain,
        loop_from,
        points,
        stages: false,
    }
}

/// The amplitude envelope of a group whose volume a flex envelope shapes and
/// no AHDSR does: full level, held through the flex release, then cut.
pub(crate) fn flex_gate(e: &ir::Breakpoints) -> ir::Envelope {
    let release: f32 = e
        .sustain
        .map_or(&[][..], |s| e.points.get(s + 1..).unwrap_or_default())
        .iter()
        .map(|p| p.ms / 1000.0)
        .sum();
    ir::Envelope {
        release: release.max(ir::Envelope::GATE.release),
        // Stays near full until the end of the release, then falls.
        release_shape: 20.0,
        ..ir::Envelope::GATE
    }
}

/// The free LFO's rate knob, measured on real Kontakt 8.13.1
/// (scratch probe lfo_freq.lua: ANALOG STRINGS LFO_P1 unsynced, 14 knob
/// values saved and read back): Hz = 0.010415 · 20465^x, within 0.4 %,
/// 0.0104 to 213.14 Hz.
pub(crate) const LFO_RATE_HZ: [f32; 2] = [0.010_415, 0.010_415 * 20_465.0];

/// A route on an LFO's rate knob, `intensity` in knob units.
pub(crate) fn lfo_rate(source: ir::Source, lfo: u8, intensity: f32) -> ir::ModRoute {
    let [lo, hi] = LFO_RATE_HZ;
    ir::ModRoute::new(
        source,
        ir::ParamAddr::LfoParam {
            lfo,
            param: ir::LfoParam::Rate,
        },
        intensity,
    )
    .span((hi / lo).log2())
    .law(ir::RouteLaw::Range(LFO_RATE_HZ))
}

/// A free-running (unsynced) LFO. Waveforms by id: sine 0, rectangle 1,
/// triangle 2, sawtooth 3, random 4, and the Multi mixes 5 and 6, whose stored
/// weights (sine, rectangle, triangle, saw, random) reach the mixer as sine,
/// saw, triangle, pulse, random (0x140b07290, 0x140b078d0; the analog tables
/// play as their exact shapes). Values: fade-in ms, Hz (or a count of the
/// synced note value), pulse width, start phase in cycles. A synced note value
/// counts quarter notes (beats). Measured in Kontakt 8.13.1 at 120 bpm (ANALOG
/// STRINGS layer 88, LFO_P1_Retrigger, a saw stored as 1/24 × count): count 96
/// cycles in 2.0 s and count 32 in 0.667 s, so a cycle is count/24 beats; read
/// as whole notes it ran 4× slow. A synced fade is not mapped.
pub(crate) fn lfo(l: &Lfo, clock: ir::Clock) -> Option<ir::Lfo> {
    const FREE: f32 = -1.0;
    let [fade_ms, hz, width, phase] = l.initial_values;
    if !l.initial_values.iter().all(|v| v.is_finite()) {
        return None;
    }
    let wave = match l.waveform {
        0 => ir::Wave::Sine,
        1 => ir::Wave::Square { width },
        2 => ir::Wave::Triangle,
        3 => ir::Wave::Saw,
        4 => ir::Wave::Random,
        5 | 6 => {
            let [sine, pulse, triangle, saw, random] = l
                .trailing_values
                .filter(|w| w.iter().all(|v| v.is_finite()))?;
            ir::Wave::Multi {
                weights: [sine, saw, triangle, pulse, random, 0.0],
                width,
                normalize: l.records[0].flag,
            }
        }
        _ => return None,
    };
    let [rate, fade] = &l.records;
    let note = rate.values[0];
    let rate = if note == FREE {
        ir::Rate::Hz(hz)
    } else {
        ir::Rate::Steps {
            beats: note,
            count: hz,
        }
    };
    (fade.values[0] == FREE).then_some(ir::Lfo {
        wave,
        rate,
        phase,
        fade_ms,
        clock,
    })
}

fn step_mod_note(s: &StepMod) -> Option<f32> {
    let note = s
        .frequency_sync
        .map(|r| r.values[0])
        .or(s.legacy_note_value)?;
    note.is_finite().then_some(note)
}

fn step_mod_active(s: &StepMod) -> Option<&[f32]> {
    let n = usize::try_from(s.num_steps).ok()?.min(s.steps.len());
    if n == 0 {
        return None;
    }
    let active = s.steps.get(..n)?;
    active.iter().all(|v| v.is_finite()).then_some(active)
}

/// Maps one Kontakt `BParArp` step value (`-1..=1`, `0140740000.c:1357-1384`)
/// through an optional modulation shaper into either signed (`-1..=1`) or
/// unipolar (`0..=1`) IR route domain.
pub(crate) fn step_mod_value(raw: f32, curve: Option<&ShaperCurve>, signed: bool) -> f32 {
    let s = raw.clamp(-1.0, 1.0);
    match curve {
        Some(curve) => {
            let x = (s + 1.0) * 0.5;
            let y = match curve {
                ShaperCurve::Breakpoints(points) if !points.is_empty() => {
                    let pts: Vec<_> = points.iter().map(|p| (p.x, p.y, p.curve)).collect();
                    kontra_law::kontakt::modulation_shaper(&pts).eval(x)
                }
                _ => curve.evaluate(x),
            };
            if signed { 2.0 * y - 1.0 } else { y }
        }
        None => {
            if signed {
                s
            } else {
                (s + 1.0) * 0.5
            }
        }
    }
}

/// Looping Step Modulator (`BParArp`, chunk `0x09`) clock as a rising ramp LFO.
///
/// Native `BArp32Step::Render` (`0140a70000.c:5399-5523`, setup `0140970000.c:5032-5049`)
/// advances phase `p in [0, numSteps)` at `frequency` steps/sec (free when note
/// value is `-1.0`) or `note * frequency` beats/step (synced), wrapping `p -= numSteps`.
/// One ramp cycle `[0, 1)` covers all `numSteps` steps.
pub(crate) fn step_mod_lfo(s: &StepMod, clock: ir::Clock) -> Option<ir::Lfo> {
    const FREE: f32 = -1.0;
    if s.single_trigger || !(s.frequency.is_finite() && s.frequency > 0.0) {
        return None;
    }
    let n = step_mod_active(s)?.len() as f32;
    let note = step_mod_note(s)?;
    let rate = if note == FREE {
        ir::Rate::Hz(s.frequency / n)
    } else if note > 0.0 {
        ir::Rate::Steps {
            beats: note * n,
            count: s.frequency,
        }
    } else {
        return None;
    };
    Some(ir::Lfo {
        wave: ir::Wave::Ramp,
        rate,
        phase: 0.0,
        fade_ms: 0.0,
        clock,
    })
}

/// Stepped transfer table for a looping Step Modulator.
///
/// `ir::Transfer::apply` indexes `floor(x * (len - 1))`, so appending one copy
/// of the final step gives all `n` steps equal `[k/n, (k+1)/n)` spans on `x in [0, 1)`.
pub(crate) fn step_mod_table(
    s: &StepMod,
    curve: Option<&ShaperCurve>,
    signed: bool,
) -> Option<Vec<f32>> {
    let active = step_mod_active(s)?;
    let mut table = Vec::with_capacity(active.len() + 1);
    for &v in active {
        table.push(step_mod_value(v, curve, signed));
    }
    if let Some(&last) = table.last() {
        table.push(last);
    }
    Some(table)
}

/// One-shot (`single_trigger`) unsynced per-voice Step Modulator as unipolar
/// `0..=1` hold-then-step breakpoints, holding the final step indefinitely
/// (`0140a70000.c:5438-5440`). Signed targets map `0..=1 → -1..=1` through a
/// linear `ir::Transfer` so `ir::Breakpoints` stays strictly within `0..=1`.
pub(crate) fn step_mod_oneshot(
    s: &StepMod,
    curve: Option<&ShaperCurve>,
) -> Option<ir::Breakpoints> {
    const FREE: f32 = -1.0;
    if !s.single_trigger
        || !(s.frequency.is_finite() && s.frequency > 0.0)
        || step_mod_note(s) != Some(FREE)
    {
        return None;
    }
    let active = step_mod_active(s)?;
    let step_ms = 1000.0 / s.frequency;
    let mapped: Vec<f32> = active
        .iter()
        .map(|&v| step_mod_value(v, curve, false))
        .collect();
    let (&start, rest) = mapped.split_first()?;
    let mut points = Vec::with_capacity(rest.len() * 2);
    let mut prev = start;
    for &next in rest {
        points.push(ir::Breakpoint {
            ms: step_ms,
            level: prev,
            curve: 0.0,
        });
        points.push(ir::Breakpoint {
            ms: 0.0,
            level: next,
            curve: 0.0,
        });
        prev = next;
    }
    Some(ir::Breakpoints {
        start,
        points,
        sustain: None,
        loop_from: None,
        stages: false,
    })
}

/// A legacy (types 2..=12) or SV (50..=62) filter; other kinds are not mapped.
/// Its `q_octaves` is the damping the sharpest stage gains over the knob's
/// whole travel, `log2(k(0) / k(1))` by this same law, so a resonance route
/// of depth 1 spans the knob's range.
pub(crate) fn filter(kind: i32, cutoff: f32, resonance: f32, gain: f32) -> Option<ir::Filter> {
    let mut f = filter_at(kind, cutoff, resonance, gain)?;
    let Some(stage) = usize::from(f.stages).checked_sub(1) else {
        return Some(f);
    };
    let at = |r: f32| {
        filter_at(kind, cutoff, r, gain)?
            .damping
            .get(stage)
            .copied()
    };
    if let (Some(lo), Some(hi)) = (at(0.0), at(1.0)) {
        let ratio = lo / hi;
        if ratio.is_finite() {
            f.q_octaves = ratio.log2().max(0.0);
        }
    }
    Some(f)
}

/// The filter types the module's types switch among (ids:
/// docs/re/kontakt-bindings/filter-type-ids.tsv): AR (100..=108), Daft (70,
/// 71), Ladder (30..=41) and SV (50..=62). One filter module stores its type
/// beside its 0..1 knobs (reader 0x140d00db0), so a switch keeps the knobs and
/// reads them through the new type's laws
/// ([`crate::script_knobs::shape_laws`]). Legacy types switch to nothing.
pub(crate) const SWITCHABLE: [std::ops::RangeInclusive<i32>; 4] =
    [100..=108, 70..=71, 30..=41, 50..=62];

/// Whether a filter of `kind` may switch to [`filter_shapes`].
pub(crate) fn switches(kind: i32) -> bool {
    SWITCHABLE.iter().any(|f| f.contains(&kind))
}

/// The shapes a [`switches`] filter may switch to.
pub(crate) fn filter_shapes() -> Vec<ir::SlotShape> {
    let ids = SWITCHABLE.into_iter().flatten();
    ids.filter_map(|id| {
        Some(ir::SlotShape {
            id,
            filter: filter_at(id, 0.5, 0.5, 0.0)?,
            laws: crate::script_knobs::shape_laws(id),
        })
    })
    .collect()
}

fn filter_at(kind: i32, cutoff: f32, resonance: f32, gain: f32) -> Option<ir::Filter> {
    use ir::Pass::{Band, High, Low, Notch};
    let (pass, stages) = match kind {
        // The Ladder's selector is `id − 29`; one damping holds its feedback.
        30..=41 => (Low, 1),
        2 | 50 => (Low, 0),
        3 | 51 => (High, 0),
        4 | 53 | 101 => (Band, 1),
        6 | 52 | 70 | 100 => (Low, 1),
        7 | 54 | 71 | 102 => (High, 1),
        8 | 16 | 17 | 55 | 103 | 106 => (Low, 2),
        9 | 57 | 105 | 108 => (High, 2),
        10 | 56 | 104 | 107 => (Band, 2),
        11 | 58 => (Notch, 2),
        12 | 59 => (Low, 3),
        60 => (Band, 3),
        61 => (High, 3),
        62 => (Notch, 3),
        _ => return None,
    };
    let (u, r) = (cutoff.clamp(0.0, 1.0), resonance.clamp(0.0, 1.0));
    let mut f = ir::Filter {
        pass,
        hz: 0.0,
        stages,
        ladder: u8::try_from(kind - 29)
            .ok()
            .filter(|_| (30..=41).contains(&kind))
            .map(ir::Ladder::ZeroDelay),
        damping: [2.0; 3],
        gain: 1.0,
        octaves: 0.0,
        q_octaves: 0.0,
    };
    match kind {
        3 => {
            legacy(&mut f, u, r);
            f.hz *= ONE_POLE_HIGH_PASS;
        }
        // Legacy Ladder (kind 17) uses the note law `100u + 20` (filt-legacy.md:123),
        // not the legacy Butterworth warp.
        ..=15 => legacy(&mut f, u, r),
        50..=62 => sv(&mut f, u, r),
        // The note each family's cutoff knob reaches (index `5n + 1381.88`
        // into the shared 2^(i/60 − 20) table): Ladder `115u + 20`
        // (filt-ladder-monark.md, its gain `2^(1.993g)`), Daft `125u + 20`
        // (filt-ar-daft.md, gain `2^(119.589409/60 * g)` = `LADDER_GAIN`),
        // AR `min(145u, 140)`, Pro-53 & Legacy Ladder `100u + 20`.
        30..=41 => {
            note(&mut f, 115.0 * u + 20.0, 115.0, r);
            f.gain = LADDER_GAIN.eval_unclamped(gain.clamp(-1.0, 1.0));
        }
        70 | 71 => {
            note(&mut f, 125.0 * u + 20.0, 125.0, r);
            f.gain = LADDER_GAIN.eval_unclamped(gain.clamp(-1.0, 1.0));
        }
        100..=108 => note(&mut f, (145.0 * u).min(140.0), 145.0, r),
        _ => note(&mut f, 100.0 * u + 20.0, 100.0, r),
    }
    Some(f)
}

/// A legacy EQ band's GAIN is stored in dB, −18..=18, its peak twice that (eq-kontakt.md
/// §1.1). A route of depth 1 moves the stored value by 12 dB and the peak by 24 dB.
/// Afflatus CC1 route-on/off: 4 kHz lift 11.37 dB native, 5.68 with a 12 dB span.
pub(crate) const EQ_GAIN_STORED_DB: f32 = 18.0;
pub(crate) const EQ_GAIN_LIMIT_DB: f32 = 2.0 * EQ_GAIN_STORED_DB;
pub(crate) const EQ_GAIN_SPAN_DB: f32 = 24.0;

/// The band (0..=2) a legacy EQ's `eqGainN` target moves, N in 1..=3.
/// The route a modulator's `eqGainN` target makes on EQ `insert`.
pub(crate) fn eq_gain(
    source: ir::Source,
    insert: u8,
    param: &str,
    intensity: f32,
) -> Option<ir::ModRoute> {
    eq_band_index(param).map(|band| {
        ir::ModRoute::new(
            source,
            ir::ParamAddr::EqBandGain { insert, band },
            intensity * EQ_GAIN_SPAN_DB,
        )
        .span(EQ_GAIN_SPAN_DB)
        .law(ir::RouteLaw::Clamp(EQ_GAIN_LIMIT_DB))
    })
}

pub(crate) fn eq_band_index(param: &str) -> Option<u8> {
    let n: u8 = param.strip_prefix("eqGain")?.parse().ok()?;
    (1..=3).contains(&n).then(|| n - 1)
}

/// A cutoff at MIDI note `n` that a knob moves `per_unit` notes; the last
/// stage resonates by `1 − (1 − r)²`, the others stay critically damped.
///
/// ponytail: these families' feedback loops, knees and passband loss
/// (AR, Daft, Pro-53) play as state-variable stages; measure
/// against Kontakt to add the loss where a library needs it.
fn note(f: &mut ir::Filter, n: f32, per_unit: f32, r: f32) {
    f.hz = FILTER_NOTE_HZ.eval_unclamped(n.min(160.0));
    f.octaves = per_unit / 12.0;
    if let Some(last) = f.damping.get_mut(usize::from(f.stages).saturating_sub(1)) {
        *last = (2.0 * (1.0 - r) * (1.0 - r)).max(MIN_DAMPING);
    }
}

/// The legacy one-pole high-pass (HP1) sits below the cascade's cutoff: an
/// oracle probe (`coverage_probe.py hp1`, $ENGINE_PAR_CUTOFF 0, .1, .5, .75)
/// fits a pole-mapped one-pole within 0.02 dB at 0.819, 0.823, 0.825 and
/// 0.837 of [`legacy`]'s Hz (35.6 Hz at 0, where the cascade gives 43.5).
/// LP1 is not probed yet and keeps the cascade's Hz.
const ONE_POLE_HIGH_PASS: f32 = 0.825;

/// The legacy cascade (docs/re/engine/filt-legacy.md): the knob through the
/// setters' exp series and warp, `(e^(6.15u) − 1) 0.0025728 + 0.002` of
/// 21759.5 Hz (Kontakt: −3 dB at 249 / 1189 Hz for .25 / .5; this law gives
/// 248 / 1200), Butterworth stages.
///
/// ponytail: resonance is fitted, not the native feedback: the sharpest
/// stage's Q grows by `e^(6.4 r)` and the passband falls 5.5 / 11 / 24 dB at
/// full resonance for 2 / 4 / 6 poles, which lands the measured LP4 peaks
/// (+20.9 / +37.3 dB at .5 / .9) within 2 dB. Measure LP2 and HP to refine.
fn legacy(f: &mut ir::Filter, u: f32, r: f32) {
    f.hz = LEGACY_CUTOFF_HZ.eval(u);
    // log2(f(1) / f(0)); the law is close to exponential above the bottom.
    f.octaves = 8.97;
    let poles = 2.0 * f32::from(f.stages);
    for (i, k) in f.damping.iter_mut().take(usize::from(f.stages)).enumerate() {
        *k = 2.0 * ((2.0 * i as f32 + 1.0) * std::f32::consts::PI / (2.0 * poles)).cos();
    }
    if let Some(sharpest) = usize::from(f.stages).checked_sub(1) {
        if let Some(k) = f.damping.get_mut(sharpest) {
            *k = (*k / LEGACY_RESONANCE_Q.eval(r)).max(MIN_DAMPING);
        }
        f.gain = LEGACY_RESONANCE_LOSS
            .get(sharpest)
            .map_or(1.0, |loss| loss.eval(r));
    }
}

/// The SV filters (docs/re/engine/filt-sv.md): cutoff is the note
/// `115u + 20` (at most 160), resonance `R = 1 − (1 − r)³` (six poles:
/// `1 − (1 − 0.9r)³`) damps each stage by `2(1 − R)` (four poles: the second
/// by `2(1 − 0.666R)`), and the tap's gain falls with `R`.
///
/// ponytail: the native state knees (`±2L`) only act on near self-oscillating
/// states; damping stops at 0.01 instead.
fn sv(f: &mut ir::Filter, u: f32, r: f32) {
    let note = (115.0 * u + 20.0).min(160.0);
    f.hz = FILTER_NOTE_HZ.eval_unclamped(note);
    f.octaves = 115.0 / 12.0;
    if f.stages == 0 {
        return;
    }
    let big = if f.stages == 3 {
        1.0 - CUBE.eval(1.0 - 0.9 * r)
    } else {
        1.0 - CUBE.eval(1.0 - r)
    };
    f.damping = [2.0 * (1.0 - big); 3];
    if f.stages == 2 {
        f.damping[1] = 2.0 * (1.0 - 0.666 * big);
    }
    for k in &mut f.damping {
        *k = k.max(MIN_DAMPING);
    }
    let fall = match (f.stages, f.pass) {
        (1, _) => 0.5 * UNIT_FIFTH.eval(big),
        (2, ir::Pass::Band) => 0.9334 * big,
        (2, ir::Pass::Notch) => 0.88 * big,
        (2, _) => 0.77 * big,
        (_, ir::Pass::Band) => (2.0 - big) * big * 0.991,
        (_, ir::Pass::Notch) => (1.482 - 0.494 * big) * big,
        _ => (1.2125 - 0.2425 * big) * big,
    };
    f.gain = 1.0 - fall;
}

const MIN_DAMPING: f32 = 0.01;

#[rustfmt::skip]
pub(crate) fn semitones(ratio: f32) -> f64 {
    kontra_law::common::SEMITONE_RATIO.inverse_f64(f64::from(ratio.max(1e-6))).unwrap_or(0.0)
}

#[rustfmt::skip]
pub(crate) fn speed() -> ir::Law { ir::Law::Breakpoints((0u8..).zip(kontra_law::kontakt::STRETCH_SPEED).map(|(k, y)| (f32::from(k) / 20.0, y, 0.0)).collect()) }

#[rustfmt::skip]
mod solid;

/// The Solid G-EQ's four bands from its knobs (each 0..=1): LF gain and Hz,
/// LMF and HMF gain, Hz and Q, HF gain and Hz. The LF and HF bands are
/// shelves unless their shape is [`ir::Shape::Peak`] (the bell switch).
/// Each bell's centre is scaled by its gain knob and its gain by its Hz and
/// Q knobs, as the core does (at full LMF boost 21.5 dB becomes 20.05 dB,
/// Kontakt's native sweep, docs/re/engine/fx-measured/solid-geq.tsv).
// ponytail: the tables on standard cookbook bands, linearly interpolated.
// The core's splines, its Orfanidis peaks, HMF's iterative bandwidth, the
// LF shelf's two shaping peaks and the HF shelf's DC cut are left out (v2
// has them all).
pub(crate) fn solid_eq(
    [lf_gain, lf_hz]: [f32; 2],
    lf_shape: ir::Shape,
    lmf: [f32; 3],
    hmf: [f32; 3],
    [hf_gain, hf_hz]: [f32; 2],
    hf_shape: ir::Shape,
) -> [ir::Bell; 4] {
    use solid::*;
    let t = |table: [f32; 11], x: f32| kontra_law::Law::Table(table.into()).eval(x);
    let peak = |hz, gain_db, q| ir::Bell {
        shape: ir::Shape::Peak,
        hz,
        q,
        gain_db,
    };
    let lf = if lf_shape == ir::Shape::Peak {
        peak(
            (1.0 - (1.0 - t(C_9BD8, lf_gain)) * 0.5 * lf_hz) * t(FREQ_LF, lf_hz),
            t(C_97A8, lf_hz) * t(GAIN_LF, lf_gain),
            t(C_9378, lf_gain) * 1.08,
        )
    } else {
        ir::Bell {
            shape: ir::Shape::LowShelf,
            hz: t(C_A868, lf_gain) * t(LF_CORNER, lf_hz),
            q: 0.5,
            gain_db: t(LF_SHELF_GAIN, lf_gain) * t(C_AC98, lf_hz),
        }
    };
    let [g, hz, q] = lmf;
    let lmf = peak(
        t(C_8F48, g) * t(FREQ_LMF, hz),
        t(C_86E8, hz) * t(GAIN_LMF, g) * t(C_8B18, q),
        t(C_71F8, g) * t(Q_LMF, q),
    );
    let [g, hz, q] = hmf;
    let hmf = peak(
        t(C_7E88, g) * t(FREQ_HMF, hz),
        t(C_7628, hz) * t(GAIN_HMF, g) * t(C_7A58, q),
        t(C_71F8, g) * t(Q_HMF, q),
    );
    let hf = if hf_shape == ir::Shape::Peak {
        let q = t(C_4818, hf_gain) * 1.2 * t(C_5078, hf_hz);
        peak(
            t(C_58D8, hf_gain) * t(FREQ_HF, hf_hz),
            t(C_54A8, hf_hz) * t(GAIN_HF, hf_gain),
            q,
        )
    } else {
        ir::Bell {
            shape: ir::Shape::HighShelf,
            hz: t(C_6780, hf_gain) * t(HF_SHELF_FREQ, hf_hz),
            q: 0.55,
            gain_db: t(HF_SHELF_GAIN, hf_gain),
        }
    };
    [lf, lmf, hmf, hf]
}

/// True dB per Kontakt dB: its levels are `2^(dB/6)`, so a saved or scripted dB is
/// `20·log10(2)/6` true dB (the Compressor setter 0x140ab9220 `powf(2, dB/6)`).
pub(crate) const KONTAKT_DB: f32 = 20.0 * std::f32::consts::LOG10_2 / 6.0;

/// The Compressor (0x19): saved mode, threshold dB, ratio knob, attack and
/// release ms, link byte. The Pro core's law (docs/re/engine/dyn-compressors.md,
/// setter 0x140ab9220): the knob `r` is the output slope over the threshold
/// (ratio `1/r`), a 12 dB knee, and Pro's own
/// make-up `−0.666·(1 − r)·threshold` dB (setter `+0x524`, constant at
/// 0x144fc4c90, added to the gain exponent in core 0x140ab35d0). Classic and
/// Enhanced have no make-up (their setters write no offset). The slot's Output
/// knob applies on top in every mode. Pro always links the channels. Every
/// threshold is `2^(dB/6)`. Classic is a [`ir::Detector::Trigger`] to the
/// fixed depth `thr^(1 − r)` (setter `+0x2e0`; core in dyn-compressors.md).
///
/// The caller (0x1409619b0) passes the saved attack and release ms unscaled;
/// each mode turns them into its own ballistics, given here as time
/// constants: Pro clamps attack to 0.1..=10 ms as a low-pass at `1000/ms` Hz
/// and release to 10..=1000 ms as a halving time; Enhanced (core 0x140ab2c80)
/// runs a two-pole detector at `2513.2/max(ms, 1)` rad/s with damping 1.8 and
/// a peak hold falling by `1778.3/max(ms, 50)` per second; Classic (core
/// 0x140ab2250) steps by `0.625^(1/n)` and `(1/3)^(1/n)` over the n frames.
// ponytail: Enhanced's piecewise curve runs as the soft knee; unmeasured.
pub(crate) fn compressor(
    mode: i32,
    threshold_db: f32,
    r: f32,
    attack_ms: f32,
    release_ms: f32,
    link: bool,
) -> ir::Compressor {
    let r = r.clamp(0.0, 1.0);
    let (attack_ms, release_ms) = match mode {
        0 => (
            attack_ms.max(0.0) / 1.6f32.ln(),
            release_ms.max(0.0) / 3f32.ln(),
        ),
        1 => (
            attack_ms.max(1.0) * 1800.0 / 2513.2,
            release_ms.max(50.0) * 1000.0 / 1778.3,
        ),
        _ => (
            attack_ms.clamp(0.1, 10.0) / std::f32::consts::TAU,
            release_ms.clamp(10.0, 1000.0) / std::f32::consts::LN_2,
        ),
    };
    ir::Compressor {
        threshold_db: threshold_db * KONTAKT_DB,
        ratio: 1.0 / r.max(1e-3),
        knee_db: 12.0,
        attack_ms,
        release_ms,
        makeup_db: 0.0,
        // The IR threshold is `2^(dB/6)` in true dB; Pro's slope reads the saved dB.
        makeup_slope: if mode == 2 { 0.666 / KONTAKT_DB } else { 0.0 },
        link: link || mode == 2,
        mix: 1.0,
        detector: if mode == 0 {
            ir::Detector::Trigger
        } else {
            ir::Detector::Input
        },
        sidechain_hz: None,
    }
}

/// The Limiter (0x1c): saved input gain (dB, as `2^(x/6)`) and release ms;
/// the output never passes full scale (docs/re/engine/dyn-limiters-transient.md).
pub(crate) fn limiter(input_db: f32, release_ms: f32) -> ir::Limiter {
    ir::Limiter {
        input_db,
        ceiling_db: 0.0,
        release_ms: release_ms.max(0.0),
        ..ir::Limiter::PEAK
    }
}

/// The Transparent Limiter (0x5f): saved drive, release and ceiling knobs
/// 0..=1. Drive `(1 − x)·40` dB, release `max(x, 0.001)` s as a one-pole time
/// constant, ceiling `−0.35 − (1 − x)·39.7` dB.
pub(crate) fn transparent_limiter(drive: f32, release: f32, ceiling: f32) -> ir::Limiter {
    let [drive, release, ceiling] = [drive, release, ceiling].map(|x| x.clamp(0.0, 1.0));
    ir::Limiter {
        input_db: (1.0 - drive) * 40.0,
        ceiling_db: -0.35 - (1.0 - ceiling) * 39.7,
        // A one-pole time constant τ halves in τ·ln 2.
        release_ms: 1000.0 * release.max(0.001) * std::f32::consts::LN_2,
        ..ir::Limiter::PEAK
    }
}

/// Solid Bus Comp (0x46): saved threshold, make-up, attack, release, ratio
/// and mix knobs 0..=1, and the link byte. Ratio, attack and release are
/// six-step switches (`floor(5x + 0.5)`); the threshold knob spans ±18.6 dB
/// around −20 dB less a per-ratio offset, make-up −5.25..=21 dB. All eight
/// parameter mappings verified against `FUN_141ac69e0` (`0141ac0000.c:3889-4130`,
/// tables `0x1479a7e18..0x1479a7ea8`, `docs/re/engine/dyn-compressors.md`).
/// Release step 5 is the auto release on a 75 ms base; the second value is
/// set then, for the caller to report.
// Core detector/auto-release state machine (`0x141aaa1e0`) is approximated by
// the neutral IR compressor at the binary-verified switch parameters (Wave 2 DSP).
pub(crate) fn bus_compressor(knobs: [f32; 6], link: bool) -> (ir::Compressor, bool) {
    const RATIO: [f32; 6] = [1.5, 2.0, 3.0, 4.0, 5.0, 10.0];
    const OFFSET_DB: [f32; 6] = [-12.0, -9.0, -5.0, -4.0, -3.3, -0.2];
    const ATTACK_MS: [f32; 6] = [0.1, 0.3, 1.0, 3.0, 10.0, 30.0];
    const RELEASE_MS: [f32; 6] = [12.5, 18.75, 37.5, 50.0, 100.0, 75.0];
    let [threshold, makeup, attack, release, ratio, mix] = knobs.map(|x| x.clamp(0.0, 1.0));
    let step = |x: f32| ((x * 5.0 + 0.5).floor() as usize).min(5);
    let at = |table: [f32; 6], x: f32| table.get(step(x)).copied().unwrap_or(1.0);
    let c = ir::Compressor {
        threshold_db: -18.6 + 37.2 * threshold - 20.0 + at(OFFSET_DB, ratio),
        ratio: at(RATIO, ratio),
        knee_db: 0.0,
        attack_ms: at(ATTACK_MS, attack),
        release_ms: at(RELEASE_MS, release),
        makeup_db: -5.25 + 26.25 * makeup,
        makeup_slope: 0.0,
        link,
        mix,
        detector: ir::Detector::Input,
        sidechain_hz: None,
    };
    (c, step(release) == 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_one_pole_high_pass_sits_at_kontakts_measured_corner() {
        // Probed: 35.6 Hz at knob 0 and 989 Hz at 0.5 (pole-mapped fit).
        for (u, hz) in [(0.0, 35.6), (0.5, 989.0)] {
            let f = filter_at(3, u, 0.0, 0.0).unwrap();
            assert!((f.hz / hz - 1.0).abs() < 0.02, "{u}: {}", f.hz);
        }
    }

    #[test]
    fn a_saved_sustain_is_a_gain() {
        for sustain in [-0.1_f32, 0.0, 0.43, 1.0, 1.2] {
            let saved = EnvelopeAhdsr {
                attack_curve: 0.0,
                attack_ms: 0.0,
                decay_ms: 0.0,
                hold_ms: 0.0,
                release_ms: 0.0,
                sustain,
                unknown_flag: 0,
                unknown_tail: Vec::new(),
            };
            assert_eq!(envelope(&saved).sustain, sustain.clamp(0.0, 1.0));
        }
    }

    #[test]
    fn state_variable_resonance_preserves_the_stage_mappings() {
        for r in [0.0_f32, 0.2, 0.7, 1.0] {
            let one = filter(52, 0.5, r, 0.0).unwrap();
            let one_resonance = 1.0 - (1.0 - r).powi(3);
            assert_eq!(one.gain, 1.0 - 0.5 * one_resonance.powi(5));
            let three = filter(59, 0.5, r, 0.0).unwrap();
            let three_resonance = 1.0 - (1.0 - 0.9 * r).powi(3);
            assert_eq!(
                three.damping[0],
                (2.0 * (1.0 - three_resonance)).max(MIN_DAMPING)
            );
            assert_eq!(
                three.gain,
                1.0 - (1.2125 - 0.2425 * three_resonance) * three_resonance
            );
        }
    }

    #[test]
    fn a_full_eq_gain_route_moves_its_band_24_db() {
        let kind = eq_gain(ir::Source::Velocity, 0, "eqGain1", 1.0);
        assert!(
            matches!(kind, Some(ir::ModRoute { target: ir::ParamAddr::EqBandGain { .. }, depth: depth_db, law: ir::RouteLaw::Clamp(limit_db), .. })
            if depth_db == 24.0 && limit_db == 36.0)
        );
    }

    #[test]
    fn a_filter_switches_to_any_modern_type_with_its_laws() {
        let ids: Vec<i32> = filter_shapes().iter().map(|s| s.id).collect();
        assert_eq!(ids, SWITCHABLE.into_iter().flatten().collect::<Vec<_>>());
        assert!(switches(103) && switches(52) && !switches(6));
        let law = |kind, field| {
            let laws = crate::script_knobs::shape_laws(kind);
            laws.iter()
                .find(|l| l.1 == field)
                .map(|l| l.2.clone())
                .unwrap()
        };
        let notch = law(58, ir::EffectParam::FilterGain);
        let want = filter(58, 0.5, 0.7, 0.0).unwrap().gain;
        assert!((notch.eval(0.7) - want).abs() < 1e-3);
        // Daft's cutoff law differs from AR's; each shape brings its own.
        for kind in [100, 70] {
            let hz = law(kind, ir::EffectParam::FilterHz).eval(0.375);
            let want = filter(kind, 0.375, 0.5, 0.0).unwrap().hz;
            assert!((hz / want - 1.0).abs() < 1e-3, "{kind}: {hz} vs {want}");
        }
        let shapes = filter_shapes();
        let notch = &shapes.iter().find(|s| s.id == 41).unwrap().filter;
        assert_eq!(notch.ladder, Some(ir::Ladder::ZeroDelay(12)));
    }

    /// Measured: a 1/24-note synced saw with count 96 cycles in 2 s at 120 bpm.
    #[test]
    fn a_synced_lfos_note_value_counts_quarter_notes() {
        use ni_file::kontakt::objects::LfoRecord;
        let record = |v| LfoRecord {
            flag: false,
            values: [v, 0.0, 0.0],
        };
        let saved = Lfo {
            structured: false,
            version: 0x73,
            waveform: 3,
            initial_values: [0.0, 96.0, 0.5, 0.0],
            records: [record(1.0 / 24.0), record(-1.0)],
            trailing_flag: false,
            trailing_values: None,
            additional_flag: None,
        };
        let Some(ir::Lfo {
            rate: ir::Rate::Steps { beats, count },
            ..
        }) = lfo(&saved, ir::Clock::Voice)
        else {
            panic!("a synced rate");
        };
        let seconds = beats * count * 60.0 / 120.0;
        assert!((seconds - 2.0).abs() < 1e-4, "{seconds}");
    }

    #[test]
    fn the_compressor_knob_is_the_output_slope_with_threshold_makeup() {
        let c = compressor(2, -20.0, 0.25, std::f32::consts::TAU, 69.3, false);
        assert!((c.ratio - 4.0).abs() < 1e-4);
        assert!((c.makeup() - 9.99).abs() < 1e-3);
        assert!((c.attack_ms - 1.0).abs() < 1e-3 && (c.release_ms - 100.0).abs() < 0.1);
        assert!(c.link);
        assert!(!compressor(1, -20.0, 0.25, 1.0, 100.0, false).link);
        // Pro's make-up follows a later threshold or ratio edit.
        let mut moved = c;
        moved.threshold_db *= 2.0;
        assert!((moved.makeup() - 2.0 * c.makeup()).abs() < 1e-3);
        // Only Pro adds its own make-up.
        assert_eq!(compressor(1, -20.0, 0.25, 1.0, 100.0, false).makeup(), 0.0);
        // Classic and Enhanced keep long saved times that Pro clamps.
        let classic = compressor(0, -20.0, 0.25, 100.0, 300.0, false);
        assert!(
            (classic.attack_ms - 212.8).abs() < 0.1 && (classic.release_ms - 273.1).abs() < 0.1
        );
        let enhanced = compressor(1, -20.0, 0.25, 100.0, 20.0, false);
        assert!(
            (enhanced.attack_ms - 71.6).abs() < 0.1 && (enhanced.release_ms - 28.1).abs() < 0.1
        );
        // A knob of 1 leaves the signal alone.
        assert!((compressor(0, -20.0, 1.0, 1.0, 100.0, true).ratio - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_transparent_limiter_and_bus_comp_reach_their_table_ends() {
        let l = transparent_limiter(0.0, 1.0, 1.0);
        assert!((l.input_db - 40.0).abs() < 1e-4 && (l.ceiling_db + 0.35).abs() < 1e-4);
        let (b, auto) = bus_compressor([0.5, 0.2, 0.0, 1.0, 1.0, 1.0], true);
        assert!((b.ratio - 10.0).abs() < 1e-6 && (b.threshold_db + 20.2).abs() < 1e-4);
        assert!((b.attack_ms - 0.1).abs() < 1e-6 && (b.release_ms - 75.0).abs() < 1e-4);
        assert!(auto && !bus_compressor([0.5; 6], true).1);
        assert!(b.makeup_db.abs() < 1e-4);
    }

    #[test]
    fn a_centred_solid_eq_is_flat_and_its_knobs_reach_the_table_ends() {
        let mid = [0.5; 3];
        let flat = solid_eq(
            [0.5; 2],
            ir::Shape::LowShelf,
            mid,
            mid,
            [0.5; 2],
            ir::Shape::HighShelf,
        );
        assert!(flat.iter().all(|b| b.gain_db == 0.0));
        let [lf, lmf, _, hf] = solid_eq(
            [0.0, 0.5],
            ir::Shape::LowShelf,
            [1.0, 0.5, 0.5],
            mid,
            [0.5; 2],
            ir::Shape::Peak,
        );
        assert_eq!(lf.shape, ir::Shape::LowShelf);
        assert_eq!(lf.gain_db, -20.0);
        // Kontakt's sweep at full LMF boost peaks near 20.1 dB at 809 Hz.
        assert!((lmf.gain_db - 20.05).abs() < 0.01 && lmf.hz == 809.0);
        assert_eq!((hf.shape, hf.hz), (ir::Shape::Peak, 6820.0 * 0.9791));
    }

    #[test]
    fn the_cutoff_laws_land_the_measured_corners() {
        let hz = |kind, u| filter(kind, u, 0.0, 0.0).map(|f| f.hz).unwrap_or_default();
        // Legacy LP1 at .25 / .5: Kontakt −3 dB at 249 / 1189 Hz.
        assert!((hz(2, 0.25) - 249.0).abs() < 5.0);
        assert!((hz(2, 0.5) - 1189.0).abs() < 20.0);
        // SV and Ladder at 0: 25.96 Hz; AR at 0 is note 0.
        assert!((hz(52, 0.0) - 25.96).abs() < 0.01);
        assert!((hz(33, 0.0) - 25.96).abs() < 0.01);
        assert!((hz(103, 0.0) - 8.176).abs() < 0.01);
    }

    #[test]
    fn the_resonance_span_is_the_damping_the_knob_moves_by() {
        // SV LP2 (52): k = 2(1 − R), R = 1 − (1 − r)³, clamped at 0.01 by the
        // law: the knob's travel is log2(2 / 0.01) octaves of Q.
        let f = filter(52, 0.5, 0.0, 0.0).unwrap();
        assert!(
            (f.q_octaves - 200f32.log2()).abs() < 0.01,
            "{}",
            f.q_octaves
        );
        // Legacy LP1 has no stage to resonate.
        assert_eq!(filter(2, 0.5, 0.0, 0.0).map(|f| f.q_octaves), Some(0.0));
    }

    #[test]
    fn the_legacy_eq_band_params_map_to_their_band() {
        assert_eq!(eq_band_index("eqGain1"), Some(0));
        assert_eq!(eq_band_index("eqGain3"), Some(2));
        assert_eq!(eq_band_index("eqGain4"), None);
        assert_eq!(eq_band_index("filterQ"), None);
    }

    #[test]
    fn one_shot_flag_sets_ahd_mode_on_envelope_and_ahdsr() {
        let saved = EnvelopeAhdsr {
            attack_curve: 0.0,
            attack_ms: 10.0,
            decay_ms: 50.0,
            hold_ms: 20.0,
            release_ms: 100.0,
            sustain: 0.7,
            unknown_flag: 1,
            unknown_tail: Vec::new(),
        };
        let env = envelope(&saved);
        assert!(env.one_shot);
        let bp = ahdsr(&saved);
        assert_eq!(bp.sustain, None);
        assert_eq!(bp.points[2].level, 0.0);
        assert_eq!(bp.points[3].ms, 0.0);
    }

    #[test]
    fn zone_envelope_law_scales_curvature_by_segment_direction() {
        let raw = ni_file::kontakt::objects::ZoneEnvelope {
            slot: 0,
            ext_mod_idx: 4,
            loop_start: 100,
            loop_length: 400,
            loop_active: true,
            speed_lock: true,
            target: String::from("volume"),
            target_idx: 0,
            points: vec![
                ni_file::kontakt::objects::ZoneEnvelopePoint {
                    x: 0,
                    y: -1.0,
                    curve: 0.5,
                },
                ni_file::kontakt::objects::ZoneEnvelopePoint {
                    x: 1000,
                    y: 1.0,
                    curve: 0.5,
                },
                ni_file::kontakt::objects::ZoneEnvelopePoint {
                    x: 2000,
                    y: -1.0,
                    curve: 0.25,
                },
            ],
        };
        let env = zone_envelope(&raw).expect("valid zone envelope");
        assert_eq!(env.slot, 4);
        assert_eq!(env.loop_range, Some((100, 400)));
        assert!(env.speed_lock);
        // Rising segment uses +16 * curve, falling segment uses -16 * curve.
        assert!((env.points[0].curve - 8.0).abs() < 1e-6);
        assert!((env.points[1].curve - (-8.0)).abs() < 1e-6);
        // Volume curve for ZoneEnvelope uses linear controller power (1).
        assert_eq!(
            volume(ir::Source::ZoneEnvelope(4)),
            ir::Curve::Depth {
                power: kontra_law::kontakt::CONTROLLER_VOLUME_POWER
            }
        );
        // Negative ext_mod_idx is ignored rather than wrapped.
        let mut unassigned = raw;
        unassigned.ext_mod_idx = -1;
        assert!(zone_envelope(&unassigned).is_none());
    }

    #[test]
    fn legacy_ladder_daft_gain_and_solid_hf_shelf_follow_binary_laws() {
        // Legacy Ladder (kind 17) uses 100u + 20 note law, not legacy Butterworth warp.
        let ll = filter(17, 0.5, 0.0, 0.0).unwrap();
        let expected_ll = FILTER_NOTE_HZ.eval_unclamped(70.0);
        assert!((ll.hz - expected_ll).abs() < 0.05, "{}", ll.hz);
        assert!((ll.octaves - 100.0 / 12.0).abs() < 1e-4);

        // Daft (70/71) wires its leading gain through LADDER_GAIN (unity at 0, +12 dB at 1).
        let daft_unity = filter(70, 0.5, 0.0, 0.0).unwrap();
        let daft_boost = filter(70, 0.5, 0.0, 1.0).unwrap();
        assert!((daft_unity.gain - 1.0).abs() < 1e-4, "{}", daft_unity.gain);
        assert!(
            (daft_boost.gain - LADDER_GAIN.eval_unclamped(1.0)).abs() < 1e-4,
            "{}",
            daft_boost.gain
        );

        // Solid G-EQ HF shelf uses +0x6350 (490..4500 Hz) * +0x6780 (0.6284..1.0),
        // never collapsing to 0 Hz at hf_hz == 0.
        let bands_min = solid_eq(
            [0.5, 0.5],
            ir::Shape::LowShelf,
            [0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5],
            [1.0, 0.0],
            ir::Shape::HighShelf,
        );
        assert!((bands_min[3].hz - 490.0).abs() < 1.0, "{}", bands_min[3].hz);
        let bands_max = solid_eq(
            [0.5, 0.5],
            ir::Shape::LowShelf,
            [0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5],
            [1.0, 1.0],
            ir::Shape::HighShelf,
        );
        assert!(
            (bands_max[3].hz - 4500.0).abs() < 1.0,
            "{}",
            bands_max[3].hz
        );
    }
}
