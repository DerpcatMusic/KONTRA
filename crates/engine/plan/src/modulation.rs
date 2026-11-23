//! Closed-form time-varying modulation sources.

use kontra_ir as ir;

/// Per-voice envelope stage time multipliers and sustain scale, latched at
/// note-on (and release at note-off) for breakpoint envelopes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvScale {
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    pub release: f32,
    pub sustain: f32,
}

impl EnvScale {
    pub const UNITY: Self = Self {
        attack: 1.0,
        hold: 1.0,
        decay: 1.0,
        release: 1.0,
        sustain: 1.0,
    };
}

/// When a voice is being read, in seconds.
#[derive(Clone, Copy, Debug)]
pub struct Age<'a> {
    /// Since the voice started.
    pub held: f32,
    /// Its age at key-up, once the key is up.
    pub released: Option<f32>,
    /// Since the engine started (free-running LFOs).
    pub now: f32,
    /// Cycles added to each of the group's LFOs by its live rate routes
    /// ([`ir::LfoParam::Hz`], [`ir::LfoParam::Division`]), integrated span
    /// by span; none past the slice's end.
    pub warp: &'a [f32],
    /// The voice's chaotic LFOs' states this span ([`Chaos`], its level last).
    pub chaos: &'a [Chaos],
    /// How much envelope modulation routes scaled each of the group's
    /// breakpoint envelopes (attack/hold/decay/sustain at note-on, release at
    /// note-off); unity past the slice's end.
    pub stretch: &'a [EnvScale],
    /// Voice position at note-on, in source frames.
    pub start_frame: f64,
    /// Voice position in source frames (start frame + playback travel), for
    /// pitch-locked zone envelopes.
    pub sample_pos: f64,
    /// Voice start frame + output frames elapsed, for unpitched zone envelopes.
    pub unpitched_pos: f64,
}

impl Age<'static> {
    pub const START: Self = Self {
        held: 0.0,
        released: None,
        now: 0.0,
        warp: &[],
        chaos: &[],
        stretch: &[],
        start_frame: 0.0,
        sample_pos: 0.0,
        unpitched_pos: 0.0,
    };

    /// Note-on timing at the voice's initial sample position.
    pub fn at_start(start_frame: f64) -> Self {
        Self {
            start_frame,
            sample_pos: start_frame,
            unpitched_pos: start_frame,
            ..Self::START
        }
    }
}

/// `state` of `l`, a chaotic LFO, `span` seconds on: one Euler step; any
/// other LFO's as it was.
pub fn chaos_step(l: &ir::Lfo, state: Chaos, span: f32, tempo: f32) -> Chaos {
    let ir::Wave::Chaos {
        attractor,
        step,
        scale,
    } = l.wave
    else {
        return state;
    };
    let h = span * hz(l, tempo) * step;
    let [x, y, z, _] = state;
    let [x, y, z] = match attractor {
        ir::Attractor::Lorenz { sigma, rho, beta } => [
            x + sigma * (y - x) * h,
            y + (x * (rho - z) - y) * h,
            z + (x * y - beta * z) * h,
        ],
        ir::Attractor::Rossler { a, b, c } => [
            x - (y + z) * h,
            y + (x + a * y) * h,
            z + (b + z * (x - c)) * h,
        ],
    };
    [x, y, z, x * scale]
}

/// An attractor's state (x, y, z) and its level (at the current span).
pub type Chaos = [f32; 4];

/// Where every attractor starts.
pub const CHAOS_START: Chaos = [1.0, 1.0, 1.0, 0.0];

/// The time the LFO's clock reads at `age`.
fn clock_time(l: &ir::Lfo, age: Age) -> f32 {
    match l.clock {
        ir::Clock::Voice => age.held,
        ir::Clock::Free => age.now,
    }
}

type Shared = Option<((u64, u64, u8, u64, u32, u32), Option<f32>)>;

thread_local! {
    /// [`smoothed_shared`]'s results by their inputs.
    static SMOOTHED: [std::cell::Cell<Shared>; 1024] =
        const { [const { std::cell::Cell::new(None) }; 1024] };
}

/// `t` smoothed ([`ir::Transfer::smoothed`]) over `l`, a wave that only
/// the clock decides (sine, triangle, saw, square with no fade-in), shared:
/// the 20 taps are evaluated once for every voice, rack and span that reads
/// the same transfer, wave and time. Only an exact repeat of every input hits
/// (`extra` holds what else the caller scales it by, as bits); anything else
/// runs `eval`, which must be pure in those inputs.
pub fn smoothed_shared(
    t: &ir::Transfer,
    l: &ir::Lfo,
    age: Age,
    tempo: f32,
    extra: u32,
    eval: impl FnOnce() -> Option<f32>,
) -> Option<f32> {
    let (width, tag) = match l.wave {
        ir::Wave::Sine => (0, 0),
        ir::Wave::Triangle => (0, 1),
        ir::Wave::Saw => (0, 2),
        ir::Wave::Ramp => (0, 4),
        ir::Wave::Square { width } => (width.to_bits(), 3),
        _ => return eval(),
    };
    if l.fade_ms > 0.0 || t.smooth_s <= 0.0 {
        return eval();
    }
    let mut table = 0xcbf2_9ce4_8422_2325_u64 ^ t.table.len() as u64 ^ u64::from(t.stepped) << 63;
    for v in &t.table {
        table = (table ^ u64::from(v.to_bits())).wrapping_mul(0x0100_0000_01b3);
    }
    let key = (
        u64::from(clock_time(l, age).to_bits()) << 32 | u64::from(l.phase.to_bits()),
        u64::from(hz(l, tempo).to_bits()) << 32 | u64::from(width),
        tag,
        table,
        t.smooth_s.to_bits(),
        extra,
    );
    let hash = (key.0.wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ key.1.wrapping_mul(0xc2b2_ae3d_27d4_eb4f)
        ^ key.3.wrapping_mul(0x1656_67b1_9e37_79f9))
        ^ u64::from(key.4 ^ key.5.rotate_left(13));
    SMOOTHED.with(|cache| {
        let slot = cache.get((hash >> 40) as usize % cache.len());
        if let Some(Some((k, v))) = slot.map(std::cell::Cell::get)
            && k == key
        {
            return v;
        }
        let v = eval();
        if let Some(slot) = slot {
            slot.set(Some((key, v)));
        }
        v
    })
}

/// An LFO's cycles a second at `tempo`.
pub fn hz(l: &ir::Lfo, tempo: f32) -> f32 {
    match l.rate {
        ir::Rate::Hz(hz) => hz,
        ir::Rate::Beats(beats) => tempo / 60.0 / beats.max(1e-3),
        ir::Rate::Steps { beats, count } => tempo / 60.0 / (beats * count).max(1e-3),
    }
}

/// The envelope's level at `age`.
pub fn envelope(e: &ir::Breakpoints, age: Age) -> f32 {
    stretched(e, age, EnvScale::UNITY)
}

/// [`envelope`] with its stage durations and sustain scaled by `scale`.
/// For staged (AHDSR) envelopes, points 0/1/2/3 scale by attack/hold/decay/release
/// and point 2's sustain level scales by `scale.sustain`. For free breakpoint
/// envelopes, point 0 scales by `scale.attack` and post-sustain points by
/// `scale.release`.
pub fn stretched(e: &ir::Breakpoints, age: Age, scale: EnvScale) -> f32 {
    let held_time_scale = |i: usize| -> f32 {
        match (e.stages, i) {
            (_, 0) => scale.attack.max(0.0),
            (true, 1) => scale.hold.max(0.0),
            (true, 2) => scale.decay.max(0.0),
            _ => 1.0,
        }
    };
    let held_level = |i: usize, level: f32| -> f32 {
        if e.stages && i == 2 {
            (level * scale.sustain).clamp(0.0, 1.0)
        } else {
            level
        }
    };
    let Some((sustain, released)) = e.sustain.zip(age.released) else {
        return held_scaled(e, age.held, held_time_scale, held_level);
    };
    let start_level = held_scaled(e, released, held_time_scale, held_level);
    let after = e.points.get(sustain + 1..).unwrap_or_default();
    let rel = scale.release.max(0.0);
    walk_scaled(start_level, after, age.held - released, |_| rel, |_, l| l).0
}

/// The level `t` seconds after the start with the key down: through the
/// points to the sustain point, then holding there or cycling its loop.
fn held_scaled(
    e: &ir::Breakpoints,
    t: f32,
    time_scale: impl Fn(usize) -> f32,
    level_of: impl Fn(usize, f32) -> f32,
) -> f32 {
    let Some(sustain) = e.sustain else {
        return walk_scaled(e.start, &e.points, t, time_scale, level_of).0;
    };
    let (level, left) = walk_scaled(
        e.start,
        e.points.get(..=sustain).unwrap_or(&e.points),
        t,
        time_scale,
        level_of,
    );
    let cycle = e.loop_from.and_then(|from| e.points.get(from..=sustain));
    let (Some(left), Some(cycle)) = (left, cycle) else {
        return level;
    };
    let period: f32 = cycle.iter().map(|p| p.ms / 1000.0).sum();
    if period > 0.0 {
        walk_scaled(level, cycle, left % period, |_| 1.0, |_, l| l).0
    } else {
        level
    }
}

/// From `level`, through `points` for `t` seconds: the level reached, and the
/// time left over once past the last point.
fn walk_scaled(
    mut level: f32,
    points: &[ir::Breakpoint],
    mut t: f32,
    time_scale: impl Fn(usize) -> f32,
    level_of: impl Fn(usize, f32) -> f32,
) -> (f32, Option<f32>) {
    for (i, p) in points.iter().enumerate() {
        let span = p.ms.max(0.0) * time_scale(i) / 1000.0;
        let to = level_of(i, p.level);
        if t < span {
            return (segment(level, to, t / span, p.curve), None);
        }
        t -= span;
        level = to;
    }
    (level, Some(t))
}

fn segment(from: f32, to: f32, x: f32, k: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let covered = if k.abs() < 1e-6 {
        x
    } else {
        (k * x).exp_m1() / k.exp_m1()
    };
    from + (to - from) * covered
}

/// A per-zone sample-position envelope's bipolar output (−1..=1) at frame `pos`.
/// Points must be in nondecreasing frame order, as enforced by Kontakt decoding.
#[must_use]
pub fn zone_envelope(e: &ir::ZoneEnvelope, mut pos: f64) -> f32 {
    let Some(first) = e.points.first() else {
        return 0.0;
    };
    if let Some((ls, ll)) = e.loop_range
        && ll > 0
        && pos >= f64::from(ls) + f64::from(ll)
    {
        pos = f64::from(ls) + (pos - f64::from(ls)).rem_euclid(f64::from(ll));
    }
    if pos <= f64::from(first.frame) {
        return first.value;
    }
    if let Some(last) = e.points.last()
        && (pos >= f64::from(last.frame) || pos.is_nan())
    {
        return last.value;
    }
    // At an interior breakpoint, use the last point at that frame. Duplicate
    // frames are possible when Kontakt's minimum spacing saturates at i32::MAX.
    let right = e.points.partition_point(|p| f64::from(p.frame) <= pos);
    let (Some(a), Some(b)) = (e.points.get(right.saturating_sub(1)), e.points.get(right)) else {
        return 0.0;
    };
    let x0 = f64::from(a.frame);
    let t = ((pos - x0) / (f64::from(b.frame) - x0)) as f32;
    segment(a.value, b.value, t, a.curve)
}

/// The oscillator's output at `age`, −1..=1, faded in.
pub fn lfo(l: &ir::Lfo, age: Age, seed: u32, tempo: f32) -> f32 {
    let t = clock_time(l, age);
    let hz = hz(l, tempo);
    let cycles = f64::from(l.phase) + f64::from(hz) * f64::from(t);
    // The fraction, exact and without libm's fmod: truncating is flooring for
    // a positive count.
    let fraction = if (0.0..4.0e18).contains(&cycles) {
        cycles - (cycles as i64) as f64
    } else {
        cycles.rem_euclid(1.0)
    };
    let p = fraction as f32;
    let random = || {
        // A new draw each half cycle: a hash of the step and the voice.
        let half = (2.0 * cycles).floor();
        let mut x = (half as i64 as u64) ^ (u64::from(seed) << 32);
        x = (x ^ (x >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd);
        x = (x ^ (x >> 33)).wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        ((x >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    };
    let pulse = |width: f32| if p < width { 1.0 } else { -1.0 };
    let sine = || (std::f32::consts::TAU * p).sin();
    let v = match l.wave {
        ir::Wave::Sine => sine(),
        ir::Wave::Triangle => 1.0 - 4.0 * ((p + 0.25).fract() - 0.5).abs(),
        ir::Wave::Saw => 1.0 - 2.0 * p,
        ir::Wave::Ramp => 2.0 * p - 1.0,
        ir::Wave::Square { width } => pulse(width),
        ir::Wave::Random => random(),
        // Integrated per voice, not a function of time (at the current span).
        ir::Wave::Chaos { .. } => 0.0,
        ir::Wave::Multi {
            weights: [w0, w1, w2, w3, w4, w5],
            width,
            normalize,
        } => {
            let saw = 2.0 * (2.0 * p).fract() - 1.0;
            let triangle = if p < 0.5 {
                4.0 * (p - 0.25)
            } else {
                4.0 * (0.75 - p)
            };
            let y = -w0 * sine() + w1 * saw + w2 * triangle - w3 * pulse(width)
                + w4 * random()
                + w5 * (1.0 - 2.0 * p);
            let sum = [w0, w1, w2, w3, w4, w5]
                .iter()
                .map(|w| w.abs())
                .sum::<f32>();
            if normalize { y / sum.max(1.0) } else { y }
        }
    };
    let fade = if l.fade_ms > 0.0 {
        crate::par_laws::LFO_FADE_IN.eval_unclamped((age.held * 1000.0 / l.fade_ms).min(1.0))
    } else {
        1.0
    };
    v * fade
}

/// `at` moved `frames` toward `to` as a one-pole of `tau_ms` (0: at once);
/// there once within 1e-4.
pub fn glide(at: f32, to: f32, tau_ms: f32, frames: f32, rate: u32) -> f32 {
    let tau = tau_ms * rate as f32 / 1000.0;
    let next = at + (to - at) * (1.0 - (-frames / tau).exp());
    if (to - next).abs() < 1e-4 { to } else { next }
}

/// `step` raised by the pitch routes' `semitones`, through `law`.
pub fn bent(law: ir::Exp2, step: f64, semitones: f32) -> f64 {
    step * law.of(f64::from(semitones) / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_curved_segment_bows_but_keeps_its_ends() {
        assert!((segment(0.0, 1.0, 0.0, 3.0)).abs() < 1e-6);
        assert!((segment(0.0, 1.0, 1.0, 3.0) - 1.0).abs() < 1e-6);
        assert!(segment(0.0, 1.0, 0.5, 3.0) < 0.5);
    }

    #[test]
    fn a_large_zone_envelope_keeps_late_segments_and_duplicate_endpoints() {
        let points = (0..32_000)
            .map(|i| ir::ZonePoint {
                frame: i * 32,
                value: if i % 2 == 0 { 1.0 } else { -1.0 },
                curve: 0.0,
            })
            .collect();
        let end = 31_999 * 32;
        let mut e = ir::ZoneEnvelope {
            slot: 4,
            points,
            loop_range: None,
            speed_lock: false,
        };
        assert_eq!(zone_envelope(&e, f64::from(end - 16)), 0.0);
        assert_eq!(zone_envelope(&e, f64::from(end + 1)), -1.0);
        e.points.push(ir::ZonePoint {
            frame: end,
            value: 0.5,
            curve: 0.0,
        });
        assert_eq!(zone_envelope(&e, f64::from(end - 16)), 0.0);
        assert_eq!(zone_envelope(&e, f64::from(end)), 0.5);
        e.loop_range = Some((end - 32, 32));
        assert_eq!(zone_envelope(&e, f64::from(end)), 1.0);
        assert_eq!(zone_envelope(&e, f64::from(end + 16)), 0.0);
    }

    #[test]
    fn zone_envelope_interpolates_curves_clamps_ends_and_loops() {
        let e = ir::ZoneEnvelope {
            slot: 4,
            points: vec![
                ir::ZonePoint {
                    frame: 100,
                    value: -1.0,
                    curve: 0.0,
                },
                ir::ZonePoint {
                    frame: 300,
                    value: 1.0,
                    curve: 4.0,
                },
                ir::ZonePoint {
                    frame: 500,
                    value: 0.0,
                    curve: 0.0,
                },
            ],
            loop_range: None,
            speed_lock: false,
        };
        assert_eq!(zone_envelope(&e, 0.0), -1.0);
        assert!((zone_envelope(&e, 200.0) - 0.0).abs() < 1e-6);
        // Curve +4 bows below linear midpoint (0.5) on a falling segment too:
        // segment(1.0, 0.0, 0.5, 4.0) > 0.5 because (e^2 - 1)/(e^4 - 1) < 0.5.
        assert!(zone_envelope(&e, 400.0) > 0.5);
        assert_eq!(zone_envelope(&e, 999.0), 0.0);

        let looped = ir::ZoneEnvelope {
            loop_range: Some((100, 200)),
            ..e
        };
        // Position 350 wraps into 100 + ((350 - 100) % 200) = 150 -> -0.5.
        assert!((zone_envelope(&looped, 350.0) - (-0.5)).abs() < 1e-6);
        assert!((zone_envelope(&looped, 550.0) - (-0.5)).abs() < 1e-6);
    }
}
