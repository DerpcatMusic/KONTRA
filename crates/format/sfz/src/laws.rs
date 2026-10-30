//! SFZ value laws (sfzformat.com): units, defaults and the shapes the IR can
//! carry. Pure functions; `translate` calls them.

use std::ops::RangeInclusive;

use kontra_ir as ir;
use kontra_law::common::DB_GAIN;

/// The amplitude envelope a region gets without `ampeg_*`: attack, hold and
/// decay 0, sustain 100 %, release 1 ms.
pub const ENVELOPE: ir::Envelope = ir::Envelope {
    attack: 0.0,
    hold: 0.0,
    decay: 0.0,
    sustain: 1.0,
    release: 0.001,
    attack_shape: 0.0,
    fall_shape: 0.0,
};

/// A MIDI key from a number or a note name: `c4` is 60, `c-1` is 0.
pub fn midi_key(text: &str) -> Option<u8> {
    if let Ok(key) = text.parse::<u8>() {
        return (key <= 127).then_some(key);
    }
    let mut chars = text.chars();
    let letter = match chars.next()?.to_ascii_lowercase() {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (shift, digits) = if let Some(digits) = rest.strip_prefix('#') {
        (1, digits)
    } else if let Some(digits) = rest.strip_prefix('b') {
        (-1, digits)
    } else {
        (0, rest)
    };
    let octave: i32 = digits.parse().ok()?;
    let key = octave
        .checked_add(1)?
        .checked_mul(12)?
        .checked_add(letter + shift)?;
    u8::try_from(key).ok().filter(|key| *key <= 127)
}

/// Envelope times are seconds and never negative.
pub fn seconds(value: f64) -> f32 {
    value.max(0.0) as f32
}

/// `ampeg_sustain` is a percentage.
pub fn sustain(percent: f64) -> f32 {
    (percent / 100.0).clamp(0.0, 1.0) as f32
}

/// `amplitude` is a percentage that adds to `volume` (dB).
pub fn amplitude_db(percent: f64) -> f32 {
    (20.0 * (percent / 100.0).max(1e-7).log10()) as f32
}

/// `pan` runs −100..=100 (sfzformat.com); the IR runs −1..=1.
pub fn pan(percent: f64) -> f32 {
    (percent / 100.0).clamp(-1.0, 1.0) as f32
}

/// `amp_veltrack` is a percentage, default 100: the level follows velocity as
/// `1 − (1 − v²)·track` (sfizz RegionStateful.cpp `velocityCurve`; negative
/// tracking reads `1 − v²`). 0 is no tracking.
pub fn velocity_track(percent: f64) -> Option<ir::Mod> {
    let intensity = (percent / 100.0).clamp(-1.0, 1.0) as f32;
    (intensity != 0.0).then(|| ir::Mod {
        name: "amp_veltrack".into(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ir::ModKind::Volume {
            source: ir::Source::Velocity,
            intensity,
            curve: ir::Curve::Squared,
            tau_ms: 0.0,
        },
    })
}

/// `trigger` (sfzformat.com). `release_key` is a release trigger.
pub fn trigger(name: &str) -> Option<ir::Trigger> {
    match name {
        "attack" => Some(ir::Trigger::Attack),
        "release" | "release_key" => Some(ir::Trigger::Release),
        "first" => Some(ir::Trigger::First),
        "legato" => Some(ir::Trigger::Legato),
        _ => None,
    }
}

/// Whether `loop_mode` loops. `one_shot` and `no_loop` play once. `loop_continuous`
/// loops until the note expires, through release; `loop_sustain` loops until
/// release and then plays on (sfzformat.com/opcodes/loop_mode). `None` is a mode
/// SFZ does not name.
pub fn loops(mode: &str) -> Option<bool> {
    match mode {
        "no_loop" | "one_shot" => Some(false),
        "loop_continuous" | "loop_sustain" => Some(true),
        _ => None,
    }
}

/// `fil_type` (sfzformat.com). The default is `lpf_2p`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Lpf1p,
    Hpf1p,
    Lpf2p,
    Hpf2p,
    Bpf2p,
    Brf2p,
    Lpf4p,
    Hpf4p,
    Lpf6p,
    Hpf6p,
}

/// The filter `fil_type` names; `None` for a name the spec does not list.
pub fn filter(name: &str) -> Option<Filter> {
    Some(match name {
        "lpf_1p" => Filter::Lpf1p,
        "hpf_1p" => Filter::Hpf1p,
        "lpf_2p" => Filter::Lpf2p,
        "hpf_2p" => Filter::Hpf2p,
        "bpf_2p" => Filter::Bpf2p,
        "brf_2p" => Filter::Brf2p,
        "lpf_4p" => Filter::Lpf4p,
        "hpf_4p" => Filter::Hpf4p,
        "lpf_6p" => Filter::Lpf6p,
        "hpf_6p" => Filter::Hpf6p,
        _ => return None,
    })
}

/// The voice filter for `shape` at `hz` with damping `k`. Poles beyond two
/// are two-pole stages in series at the same damping; a band-pass is scaled
/// to unity at its peak.
// ponytail: 4- and 6-pole shapes repeat the resonance on every stage.
pub fn voice_filter(shape: Filter, hz: f64, k: f64) -> ir::Filter {
    let (pass, stages) = match shape {
        Filter::Lpf1p => (ir::Pass::Low, 0),
        Filter::Hpf1p => (ir::Pass::High, 0),
        Filter::Lpf2p => (ir::Pass::Low, 1),
        Filter::Hpf2p => (ir::Pass::High, 1),
        Filter::Bpf2p => (ir::Pass::Band, 1),
        Filter::Brf2p => (ir::Pass::Notch, 1),
        Filter::Lpf4p => (ir::Pass::Low, 2),
        Filter::Hpf4p => (ir::Pass::High, 2),
        Filter::Lpf6p => (ir::Pass::Low, 3),
        Filter::Hpf6p => (ir::Pass::High, 3),
    };
    let k = k as f32;
    ir::Filter {
        pass,
        hz: hz.clamp(1.0, 22_000.0) as f32,
        stages,
        ladder: None,
        damping: [k; 3],
        gain: if pass == ir::Pass::Band { k } else { 1.0 },
        octaves: 0.0,
        q_octaves: 0.0,
    }
}

/// Damping k = 1/Q for `resonance` in dB, with Q = 10^(dB/20) and at least
/// 0.707 (sfzformat.com: resonance is dB above flat, default 0).
pub fn damping(resonance_db: f64) -> f64 {
    1.0 / DB_GAIN.eval_unclamped_f64(resonance_db).max(0.707)
}

/// A key or velocity range with its crossfades. `xfin_lo*` fades in up to
/// `lo*` and `xfout_hi*` fades out from `hi*` (sfzformat.com). The IR fades
/// inside the zone, so the zone widens over the crossfade and the fade is its
/// width: `(start, fade in, fade out)` steps.
pub fn span(lo: u8, hi: u8, xfin_lo: u8, xfout_hi: u8) -> (RangeInclusive<u8>, (u8, u8)) {
    let start = xfin_lo.min(lo);
    let end = xfout_hi.max(hi);
    (start..=end, (lo - start, end - hi))
}
