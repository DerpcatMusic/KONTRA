//! Port from v1 0cb7a8a0:src/ui/viz.rs, drawn through native v2 binding laws.
pub use super::editor_model::Model;
use super::editor_model::{Ahdsr, GroupSettings};
use crate::sound::edits::Param;
#[derive(Clone, Copy)]
pub enum Phase {
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    Flex,
    Done,
}
impl Phase {
    pub fn from_u8(p: u8) -> Self {
        match p {
            0 => Self::Attack,
            1 => Self::Hold,
            2 => Self::Decay,
            3 => Self::Sustain,
            4 => Self::Release,
            5 => Self::Flex,
            _ => Self::Done,
        }
    }
}
const LOW_HZ: f32 = 20.;
const HIGH_HZ: f32 = 20_000.;
pub const TOP_DB: f32 = 24.;
pub const BOTTOM_DB: f32 = -36.;
const STAGE_POINTS: usize = 40;
const RESPONSE_POINTS: usize = 160;
/// Horizontal place of `hz` on the response's log axis.
pub fn freq_x(hz: f32) -> f32 {
    (hz / LOW_HZ).log10() / (HIGH_HZ / LOW_HZ).log10()
}

/// Vertical place of `db` on the response.
pub fn db_y(db: f32) -> f32 {
    (db - BOTTOM_DB) / (TOP_DB - BOTTOM_DB)
}

/// The chain's magnitude over the audible range, `(x, y)`.
pub fn response(s: &GroupSettings) -> Vec<[f32; 2]> {
    (0..=RESPONSE_POINTS)
        .map(|i| {
            let x = i as f32 / RESPONSE_POINTS as f32;
            let hz = LOW_HZ * (HIGH_HZ / LOW_HZ).powf(x);
            [
                x,
                db_y(20. * s.magnitude(hz).max(1e-6).log10()).clamp(-0.02, 1.02),
            ]
        })
        .collect()
}

/// An envelope drawn stage by stage: attack, hold, decay, sustain and
/// release, each as wide as its (compressed) time.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvelopeShape {
    pub line: Vec<[f32; 2]>,
    /// Points of each stage in `line`, attack through release.
    pub stages: [std::ops::Range<usize>; 5],
}

/// A stage's width: log-compressed time, so a 2 ms attack and a 20 s
/// release both read, with a floor so an instant stage still has a handle.
fn span(seconds: f32) -> f32 {
    (1. + seconds.max(0.) / 0.05).ln().max(0.12)
}

/// Width of the sustain plateau, in [`span`]'s units.
const SUSTAIN: f32 = 1.4;

fn widths(env: &Ahdsr) -> [f32; 5] {
    [
        span(env.attack),
        span(env.hold),
        span(env.decay),
        SUSTAIN,
        span(env.release),
    ]
}

/// How long `env` draws, in [`span`]'s units: two envelopes laid out over
/// the longer one's share one time axis.
pub fn envelope_width(env: &Ahdsr) -> f32 {
    widths(env).iter().sum()
}

#[cfg(test)]
pub fn envelope(env: &Ahdsr) -> EnvelopeShape {
    envelope_over(env, envelope_width(env))
}

/// `env` laid out with `total` (at least its own width) as the full width.
pub fn envelope_over(env: &Ahdsr, total: f32) -> EnvelopeShape {
    let [attack, decay, release] = env.trace(STAGE_POINTS);
    let widths = widths(env);
    let total = total.max(widths.iter().sum());
    let mut line = Vec::new();
    let mut stages: [std::ops::Range<usize>; 5] = Default::default();
    let mut x0 = 0.;
    let sustain = env.sustain.clamp(0., 1.);
    let flat = |level: f32| vec![level, level];
    for (i, levels) in [attack, flat(1.), decay, flat(sustain), release]
        .into_iter()
        .enumerate()
    {
        let w = widths[i] / total;
        let start = line.len();
        let n = (levels.len() - 1).max(1) as f32;
        // A stage of no time still spans its floor: a vertical step, then level.
        for (k, level) in levels.iter().enumerate() {
            let x = if levels.len() == 1 {
                x0 + w
            } else {
                x0 + w * k as f32 / n
            };
            line.push([x, *level]);
        }
        stages[i] = start..line.len();
        x0 += w;
    }
    EnvelopeShape { line, stages }
}

/// Where a voice at `phase` and `level` sits on `shape`.
pub fn playhead(shape: &EnvelopeShape, phase: Phase, level: f32) -> Option<[f32; 2]> {
    let find = |stage: usize, above: bool| {
        let r = shape.stages[stage].clone();
        let points = &shape.line[r.clone()];
        let at = points
            .iter()
            .position(|p| if above { p[1] >= level } else { p[1] <= level })
            .unwrap_or(points.len().saturating_sub(1));
        points.get(at).map(|p| [p[0], level])
    };
    match phase {
        Phase::Attack => find(0, true),
        Phase::Hold => shape.line.get(shape.stages[1].start).copied(),
        Phase::Decay => find(2, false),
        Phase::Sustain => {
            let r = &shape.stages[3];
            let (a, b) = (shape.line.get(r.start)?, shape.line.get(r.end - 1)?);
            Some([(a[0] + b[0]) / 2., level])
        }
        Phase::Release => find(4, false),
        Phase::Flex | Phase::Done => None,
    }
}

/// A point to drag: which parameters its axes and the wheel move, and by
/// how much of their normalized range per graph width or height.
#[derive(Clone, Debug, PartialEq)]
pub struct Handle {
    pub at: [f32; 2],
    pub x: Option<(Param, f32)>,
    pub y: Option<(Param, f32)>,
    pub wheel: Option<Param>,
    /// Off (a bypassed unit): drawn hollow.
    pub active: bool,
}

impl Handle {
    pub fn params(&self) -> impl Iterator<Item = Param> + '_ {
        self.x
            .map(|x| x.0)
            .into_iter()
            .chain(self.y.map(|y| y.0))
            .chain(self.wheel)
    }
}

/// Attack (time, and its curve at the middle of the rise), hold, decay
/// with sustain, and release.
pub fn envelope_handles(shape: &EnvelopeShape, env: &Ahdsr) -> Vec<Handle> {
    let end = |stage: usize| shape.line[shape.stages[stage].end - 1];
    let mid = shape.line[shape.stages[0].clone()][shape.stages[0].len() / 2];
    let h = |at, x, y| Handle {
        at,
        x,
        y,
        wheel: x.or(y).map(|(p, _)| p),
        active: true,
    };
    vec![
        h(end(0), Some((Param::Attack, 1.)), None),
        h(mid, None, Some((Param::Curve, 1.))),
        h(end(1), Some((Param::Hold, 1.)), None),
        h(
            [end(2)[0], env.sustain.clamp(0., 1.)],
            Some((Param::Decay, 1.)),
            Some((Param::Sustain, 1.)),
        ),
        h(end(4), Some((Param::Release, 1.)), None),
    ]
}

/// One handle per filter (cutoff across, resonance up) and per EQ band
/// (frequency across, gain up, bandwidth on the wheel), on the curve.
pub fn filter_handles(s: &GroupSettings) -> Vec<Handle> {
    s.values
        .iter()
        .filter_map(|&(p, n)| {
            let x = match p {
                Param::Cutoff(_) | Param::Freq(..) => p,
                _ => return None,
            };
            let hz = s.frequency(p, n)?;
            // port from v1 0cb7a8a0:src/ui/viz.rs: graph octaves / native knob octaves.
            let low = s.frequency(p, 0.)?;
            let high = s.frequency(p, 1.)?;
            let scale = if low > 0. && high > low {
                (HIGH_HZ / LOW_HZ).log2() / (high / low).log2()
            } else {
                1.
            };
            let (y, wheel) = match p {
                Param::Cutoff(slot) => (Param::Resonance(slot), None),
                Param::Freq(slot, b) => (Param::Gain(slot, b), Some(Param::Bandwidth(slot, b))),
                _ => unreachable!(),
            };
            // port from v1 0cb7a8a0:src/ui/viz.rs: EQ handles show their own gain.
            let (db, y_scale) = if matches!(p, Param::Freq(..)) {
                let db = y.read(s).and_then(|n| s.gain_db(y, n));
                let range = s
                    .gain_db(y, 1.)
                    .zip(s.gain_db(y, 0.))
                    .map(|(high, low)| high - low)
                    .filter(|range| *range > 0.);
                (db, range.map(|range| (TOP_DB - BOTTOM_DB) / range))
            } else {
                (None, Some(1.))
            };
            Some(Handle {
                at: [
                    freq_x(hz),
                    db_y(db.unwrap_or_else(|| 20. * s.magnitude(hz).max(1e-6).log10()))
                        .clamp(0., 1.),
                ],
                x: Some((x, scale)),
                y: y_scale
                    .filter(|_| y.read(s).is_some())
                    .map(|scale| (y, scale)),
                wheel: wheel.filter(|w| s.values.iter().any(|(q, _)| q == w)),
                active: true,
            })
        })
        .collect()
}

/// A parameter's short name.
pub fn label(p: Param) -> &'static str {
    match p {
        Param::Attack => "Attack",
        Param::Curve => "Curve",
        Param::Hold => "Hold",
        Param::Decay => "Decay",
        Param::Sustain => "Sustain",
        Param::Release => "Release",
        Param::Cutoff(_) => "Cutoff",
        Param::Resonance(_) => "Resonance",
        Param::Freq(..) => "Frequency",
        Param::Bandwidth(..) => "Width",
        Param::Gain(..) => "Gain",
    }
}

pub fn hz_text(hz: f32) -> String {
    if hz >= 1000. {
        format!("{:.2} kHz", hz / 1000.)
    } else {
        format!("{hz:.0} Hz")
    }
}

fn seconds_text(s: f32) -> String {
    if s >= 1. {
        format!("{s:.2} s")
    } else {
        format!("{:.1} ms", s * 1000.)
    }
}

/// A value as the player reads it.
pub fn readout(p: Param, v: f32) -> String {
    match p {
        Param::Attack | Param::Hold | Param::Decay | Param::Release => seconds_text(v),
        Param::Curve => format!("{v:+.2}"),
        Param::Sustain if v <= 1e-4 => "-inf dB".into(),
        Param::Sustain => format!("{:.1} dB", 20. * v.log10()),
        Param::Cutoff(_) => hz_text(v),
        Param::Resonance(_) => format!("{:.0}%", v * 100.),
        Param::Freq(..) => hz_text(v),
        Param::Bandwidth(..) => format!("{:.2} oct", v),
        Param::Gain(..) => format!("{:+.1} dB", v),
    }
}

/// What the player typed for `p` as its normalized value: a number in the
/// readout's units, "ms" or "s" after a time (bare, milliseconds), "k" or
/// "kHz" after a frequency, "-inf" for silence. None when it is not one.
pub fn typed(p: Param, text: &str, at: impl Fn(f32) -> f32) -> Option<f32> {
    let t = text.trim().to_lowercase();
    let end = t
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+')))
        .unwrap_or(t.len());
    let unit = t[end..].trim();
    let n: f32 = match &t[..end] {
        "-" if unit.starts_with("inf") => -200.,
        number => number.parse().ok().filter(|n: &f32| n.is_finite())?,
    };
    let target = match p {
        Param::Attack | Param::Hold | Param::Decay | Param::Release => match unit {
            "" | "ms" => n / 1000.,
            "s" | "sec" => n,
            _ => return None,
        },
        Param::Cutoff(_) | Param::Freq(..) => match unit {
            "" | "hz" => n,
            "k" | "khz" => n * 1000.,
            _ => return None,
        },
        _ => n,
    };
    // Every readout rises or falls with the knob: halve the knob's range.
    let rising = at(1.) >= at(0.);
    let (mut lo, mut hi) = (0f32, 1f32);
    for _ in 0..32 {
        let mid = (lo + hi) / 2.;
        if (at(mid) < target) == rising {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some((lo + hi) / 2.)
}

/// The widest readout of `p`, so its field keeps its width.
pub fn widest(p: Param) -> &'static str {
    match p {
        Param::Attack | Param::Hold | Param::Decay | Param::Release => "888.8 ms",
        Param::Curve => "+0.00",
        Param::Sustain => "-88.8 dB",
        Param::Cutoff(_) | Param::Freq(..) => "88.88 kHz",
        Param::Resonance(_) => "100%",
        Param::Bandwidth(..) => "8.88 oct",
        Param::Gain(..) => "+88.8 dB",
    }
}
