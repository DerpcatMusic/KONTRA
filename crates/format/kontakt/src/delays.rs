//! Kontakt's legacy Delay (0x10), Chorus (0x11), Flanger (0x12) and Phaser
//! (0x14) onto `ir::Delay` and `ir::Phaser`. Saved fields in file order
//! (v2 sampler-kontakt effects/timemod.rs `read`, versions 0x50 and 0x51);
//! laws from the Kontakt 8 setters and audio (docs/re/engine/k-delays.md).
//! Kontakt counts its swings and one-pole coefficients in frames; they are
//! given here at 48 kHz.

use kontra_ir as ir;

/// A saved `unit` above this selects the tempo-synced value (reader flag,
/// 0x144737708).
const SYNC: f32 = 1e-9;
const RATE: f32 = 48_000.0;
/// Legacy Delay: the share of the feedback knob in the loop and the input
/// (setter case 0x43, 0x144714930), and of the damping knob in the one-pole
/// (0x144f38804). Raising either lengthens the tail or darkens it more.
const DELAY_LOOP: f32 = 0.98;
const DELAY_DAMPING: f32 = 0.7;
/// The legacy Chorus's per-head smoothing coefficient (0x144737744).
const CHORUS_SMOOTHING: f32 = 0.95;
/// The legacy Flanger's and Phaser's feedback scale (0x144737744,
/// 0x144f389b0); 1 would ring forever.
const FLANGER_LOOP: f32 = 0.95;
const PHASER_LOOP: f32 = 0.85;
/// The legacy LFO phase knob spans a quarter cycle (0x1447148e4).
const QUARTER: f32 = 0.25;

/// The record's effect, `None` when a field is missing or out of range.
pub(crate) fn legacy(module: u16, version: u16, public: &[u8]) -> Option<ir::EffectKind> {
    if !matches!(version, 0x50 | 0x51) {
        return None;
    }
    let f = |n: usize| {
        let bytes = public.get(4 * n..4 * n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    Some(match module {
        0x10 => {
            let [time, feedback, pan, damping, unit] = saved(version, public)?;
            ir::EffectKind::Delay(delay(time, feedback, pan, damping, unit))
        }
        0x11 => ir::EffectKind::Delay(chorus(f(1)?, f(2)?, f(3)?)),
        0x12 => ir::EffectKind::Delay(flanger(f(0)?, f(1)?, f(2)?, f(3)?, f(4)?, f(5)?)),
        0x14 => ir::EffectKind::Phaser(phaser(f(0)?, f(1)?, f(2)?, f(3)?, f(4)?)),
        _ => return None,
    })
}

/// The legacy Delay's saved time, feedback, pan, damping and unit.
pub(crate) fn saved(version: u16, public: &[u8]) -> Option<[f32; 5]> {
    if !matches!(version, 0x50 | 0x51) {
        return None;
    }
    let mut out = [0.0; 5];
    for (n, v) in out.iter_mut().enumerate() {
        let bytes = public.get(4 * n..4 * n + 4)?.try_into().ok()?;
        *v = Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())?;
    }
    Some(out)
}

/// The LFO speed: Hz, or synced, `unit·max(speed, 1)` beats per cycle
/// (getter 0x140a736a0).
fn rate(speed: f32, unit: f32) -> ir::Rate {
    if unit > SYNC {
        ir::Rate::Beats(unit * speed.max(1.0))
    } else {
        ir::Rate::Hz(speed.max(0.0))
    }
}

/// The corner of a one-pole `y += (x − y)·c` at 48 kHz.
fn corner(c: f32) -> f32 {
    if c >= 1.0 {
        f32::INFINITY
    } else {
        -(1.0 - c.max(0.0)).ln() * RATE / std::f32::consts::TAU
    }
}

/// Legacy Delay: time ms (synced: a note count times the unit, at least one,
/// getter 0x140a74bf0), feedback, pan, damping. Setter case 0x43 and audio
/// 0x140aa1130: `g = 0.98·feedback` scales the input as well as the loop, the
/// right input by `1 − pan`; the loop keeps `(1 − pan)·g` and crosses
/// `pan·g`; the read is low-passed by `1 − 0.7·damping`.
pub(crate) fn delay(time: f32, feedback: f32, pan: f32, damping: f32, unit: f32) -> ir::Delay {
    let [feedback, pan, damping] = [feedback, pan, damping].map(|x| x.clamp(0.0, 1.0));
    let g = DELAY_LOOP * feedback;
    ir::Delay {
        time: if unit > SYNC {
            ir::DelayTime::Beats(unit * time.max(1.0))
        } else {
            ir::DelayTime::Ms(time.clamp(0.0, 5800.0))
        },
        depth_ms: 0.0,
        second_head: false,
        input: [[g, 0.0], [0.0, (1.0 - pan) * g]],
        feedback: [[1.0 - pan, pan], [pan, 1.0 - pan]].map(|row| row.map(|x| x * g)),
        output: ir::IDENTITY,
        damping_hz: corner(1.0 - DELAY_DAMPING * damping),
        low_cut_hz: 0.0,
        steep_low_cut: false,
        high_shelf: None,
        spread_ms: 0.0,
        sync: ir::DelaySync::FREE,
        rate: ir::Rate::Hz(0.0),
        phase: 0.0,
    }
}

/// Legacy Chorus: depth, speed, phase, unit. Setter 0x140963970 and audio
/// 0x140aa0510: the base delay field is never written, so both heads (the
/// second at half the delay) sit a frame or two back and Depth has no effect
/// (kemu); each head is smoothed by 0.95. Phase is a quarter cycle.
fn chorus(speed: f32, phase: f32, unit: f32) -> ir::Delay {
    ir::Delay {
        time: ir::DelayTime::Ms(0.0),
        depth_ms: 0.0,
        second_head: true,
        input: ir::IDENTITY,
        feedback: [[0.0; 2]; 2],
        output: ir::IDENTITY,
        damping_hz: corner(CHORUS_SMOOTHING),
        low_cut_hz: 0.0,
        steep_low_cut: false,
        high_shelf: None,
        spread_ms: 0.0,
        sync: ir::DelaySync::FREE,
        rate: rate(speed, unit),
        phase: QUARTER * phase.clamp(0.0, 1.0),
    }
}

/// Legacy Flanger: depth, feedback, speed, phase, colour, unit. Setter
/// 0x1409637f0 and audio 0x140aa2350: colour is the base delay in ms (0..=5),
/// the swing `50·depth` frames but less than the base, the loop subtracts
/// `0.95·feedback`. Phase is a quarter cycle.
fn flanger(depth: f32, feedback: f32, speed: f32, phase: f32, colour: f32, unit: f32) -> ir::Delay {
    let colour = colour.clamp(0.0, 5.0);
    let swing = 50.0 * depth.clamp(0.0, 1.0) / RATE * 1000.0;
    ir::Delay {
        time: ir::DelayTime::Ms(colour),
        depth_ms: swing.min((colour - 1000.0 / RATE).max(0.0)),
        second_head: false,
        input: ir::IDENTITY,
        feedback: ir::IDENTITY.map(|row| row.map(|x| -FLANGER_LOOP * feedback.clamp(0.0, 1.0) * x)),
        output: ir::IDENTITY,
        damping_hz: f32::INFINITY,
        low_cut_hz: 0.0,
        steep_low_cut: false,
        high_shelf: None,
        spread_ms: 0.0,
        sync: ir::DelaySync::FREE,
        rate: rate(speed, unit),
        phase: QUARTER * phase.clamp(0.0, 1.0),
    }
}

/// Legacy Phaser: depth, feedback, speed, phase, unit. Audio 0x140962c75:
/// four allpasses with `a = (1 − w)/(1 + w)`, `w = 0.0191667 +
/// 0.0170627·(e^(3·depth·lfo) − 1)` (constants 0x3f7b17e5, 0x3f82740e,
/// 0x3c8bc709; `w = π·f/48000` gives the Hz), loop `0.85·feedback`. Phase is a
/// quarter cycle.
fn phaser(depth: f32, feedback: f32, speed: f32, phase: f32, unit: f32) -> ir::Phaser {
    let hz = |w: f32| w * RATE / std::f32::consts::PI;
    ir::Phaser {
        stages: 4,
        triangle: false,
        hz: hz(0.019_166_7),
        spread_hz: hz(0.017_062_7),
        exponent: 3.0 * depth.clamp(0.0, 1.0),
        feedback: PHASER_LOOP * feedback.clamp(0.0, 1.0),
        rate: rate(speed, unit),
        phase: QUARTER * phase.clamp(0.0, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(fields: &[f32]) -> Vec<u8> {
        fields.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    #[test]
    fn the_legacy_delay_scales_its_input_by_the_feedback_and_syncs_to_notes() {
        let Some(ir::EffectKind::Delay(d)) =
            legacy(0x10, 0x50, &record(&[250.0, 0.5, 0.25, 0.0, -1.0]))
        else {
            panic!()
        };
        assert_eq!(d.time, ir::DelayTime::Ms(250.0));
        assert_eq!(d.input, [[0.49, 0.0], [0.0, 0.75 * 0.49]]);
        assert_eq!(
            d.feedback,
            [[0.75 * 0.49, 0.25 * 0.49], [0.25 * 0.49, 0.75 * 0.49]]
        );
        assert_eq!(d.damping_hz, f32::INFINITY);
        let Some(ir::EffectKind::Delay(d)) =
            legacy(0x10, 0x51, &record(&[3.0, 0.5, 0.5, 1.0, 0.5]))
        else {
            panic!()
        };
        assert_eq!(d.time, ir::DelayTime::Beats(1.5));
        assert!((d.damping_hz - 2724.6).abs() < 1.0, "{}", d.damping_hz);
        assert!(legacy(0x10, 0x52, &record(&[3.0; 5])).is_none());
    }

    #[test]
    fn the_flanger_swing_stays_inside_its_base_and_the_phaser_sweeps_from_293_hz() {
        let Some(ir::EffectKind::Delay(f)) =
            legacy(0x12, 0x50, &record(&[1.0, 0.5, 2.0, 1.0, 0.5, -1.0]))
        else {
            panic!()
        };
        assert!((f.depth_ms - (0.5 - 1.0 / 48.0)).abs() < 1e-6);
        assert_eq!(
            (f.feedback, f.rate, f.phase),
            ([[-0.475, -0.0], [-0.0, -0.475]], ir::Rate::Hz(2.0), 0.25)
        );
        let Some(ir::EffectKind::Phaser(p)) =
            legacy(0x14, 0x50, &record(&[0.5, 1.0, 1.0, 0.0, 0.25]))
        else {
            panic!()
        };
        assert!((p.hz - 292.8).abs() < 0.1 && (p.exponent - 1.5).abs() < 1e-6);
        assert_eq!(p.rate, ir::Rate::Beats(0.25));
    }
}
