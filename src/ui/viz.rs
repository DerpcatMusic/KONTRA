//! The sound editor's pictures as numbers: a group's envelope and filter
//! chain laid out in unit coordinates (x and y in 0..=1, y up), from the
//! engine's own laws. Nothing here draws; `editor.rs` scales and paints it.

use crate::engine::filter::{Shape, band_settings, filter_settings};
use crate::engine::overrides::{Edits, PROBE_VALUES, Param};
use crate::engine::{Ahdsr, GroupSettings, Phase};
use crate::import::Group;

/// The rate responses are drawn at; the audio runs at the host's.
pub const RATE: f32 = 48_000.;
const LOW_HZ: f32 = 20.;
const HIGH_HZ: f32 = 20_000.;
/// The response's decibel span.
pub const TOP_DB: f32 = 24.;
pub const BOTTOM_DB: f32 = -36.;
/// Samples per envelope stage and across the response.
const STAGE_POINTS: usize = 40;
const RESPONSE_POINTS: usize = 160;

/// One group as it plays and under the player's edits: the library's
/// values as its scripts set them.
#[derive(Clone, PartialEq)]
pub struct Model {
    pub playing: GroupSettings,
    pub base: GroupSettings,
    /// What the group has to edit, with its place in the probe's values.
    pub params: Vec<(usize, Param)>,
}

impl Model {
    /// `group` (index `index`) from the library, with the audio thread's
    /// `probe` values (playing, base) when it publishes them, else the
    /// part's `edits` applied here as the engine would.
    pub fn new(group: &Group, index: u16, edits: &Edits, probe: Option<[[f32; PROBE_VALUES]; 2]>) -> Self {
        let mut base = GroupSettings::from(group);
        let params = Param::of_group(&base);
        let overlay = |s: &mut GroupSettings, values: &[f32; PROBE_VALUES]| {
            for &(i, p) in &params {
                if values[i].is_finite() {
                    p.write(s, values[i]);
                }
            }
        };
        if let Some([_, b]) = &probe {
            overlay(&mut base, b);
        }
        let mut playing = base.clone();
        match &probe {
            Some([p, _]) => overlay(&mut playing, p),
            None => {
                for &(_, p) in &params {
                    if let Some(v) = p.read(&base) {
                        p.write(&mut playing, p.apply(v, edits.offset(index, p)));
                    }
                }
            }
        }
        Self { playing, base, params }
    }
}

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
    let Some(f) = s.filter.as_deref() else {
        return Vec::new();
    };
    (0..=RESPONSE_POINTS)
        .map(|i| {
            let x = i as f32 / RESPONSE_POINTS as f32;
            let hz = LOW_HZ * (HIGH_HZ / LOW_HZ).powf(x);
            let db = 20. * f.magnitude(hz, RATE).max(1e-6).log10();
            [x, db_y(db).clamp(-0.02, 1.02)]
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

pub fn envelope(env: &Ahdsr) -> EnvelopeShape {
    let [attack, decay, release] = env.trace(STAGE_POINTS);
    let widths = [span(env.attack), span(env.hold), span(env.decay), SUSTAIN, span(env.release)];
    let total: f32 = widths.iter().sum();
    let mut line = Vec::new();
    let mut stages: [std::ops::Range<usize>; 5] = Default::default();
    let mut x0 = 0.;
    let sustain = env.sustain.clamp(0., 1.);
    let flat = |level: f32| vec![level, level];
    for (i, levels) in [attack, flat(1.), decay, flat(sustain), release].into_iter().enumerate() {
        let w = widths[i] / total;
        let start = line.len();
        let n = (levels.len() - 1).max(1) as f32;
        // A stage of no time still spans its floor: a vertical step, then level.
        for (k, level) in levels.iter().enumerate() {
            let x = if levels.len() == 1 { x0 + w } else { x0 + w * k as f32 / n };
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
        self.x.map(|x| x.0).into_iter().chain(self.y.map(|y| y.0)).chain(self.wheel)
    }
}

/// Attack (time, and its curve at the middle of the rise), hold, decay
/// with sustain, and release.
pub fn envelope_handles(shape: &EnvelopeShape, env: &Ahdsr) -> Vec<Handle> {
    let end = |stage: usize| shape.line[shape.stages[stage].end - 1];
    let mid = shape.line[shape.stages[0].clone()][shape.stages[0].len() / 2];
    let h = |at, x, y| Handle { at, x, y, wheel: None, active: true };
    vec![
        h(end(0), Some((Param::Attack, 1.)), None),
        h(mid, None, Some((Param::Curve, 1.))),
        h(end(1), Some((Param::Hold, 1.)), None),
        h([end(2)[0], env.sustain.clamp(0., 1.)], Some((Param::Decay, 1.)), Some((Param::Sustain, 1.))),
        h(end(4), Some((Param::Release, 1.)), None),
    ]
}

/// One handle per filter (cutoff across, resonance up) and per EQ band
/// (frequency across, gain up, bandwidth on the wheel), on the curve.
pub fn filter_handles(s: &GroupSettings) -> Vec<Handle> {
    let Some(f) = s.filter.as_deref() else {
        return Vec::new();
    };
    // The response axis spans 10 octaves; the cutoff knob 8.96, the EQ
    // frequency knob the same three decades; gain ±18 dB of the 60 shown.
    let octaves = (HIGH_HZ / LOW_HZ).log2();
    let mut out = Vec::new();
    for unit in f.units() {
        let slot = unit.slot;
        match unit.shape {
            Shape::Filter(_) => {
                let (hz, _) = filter_settings(unit.knobs[0], unit.knobs[1]);
                let db = 20. * f.magnitude(hz, RATE).max(1e-6).log10();
                out.push(Handle {
                    at: [freq_x(hz), db_y(db).clamp(0., 1.)],
                    x: Some((Param::Cutoff(slot), octaves / 8.96)),
                    y: Some((Param::Resonance(slot), 1.)),
                    wheel: None,
                    active: !unit.bypass,
                });
            }
            Shape::Eq => {
                for b in 0..unit.sections {
                    let k = &unit.knobs[3 * b as usize..];
                    let (hz, _, db) = band_settings(k[0], k[1], k[2]);
                    out.push(Handle {
                        at: [freq_x(hz), db_y(db)],
                        x: Some((Param::Freq(slot, b), 1.)),
                        y: Some((Param::Gain(slot, b), (TOP_DB - BOTTOM_DB) / 36.)),
                        wheel: Some(Param::Bandwidth(slot, b)),
                        active: !unit.bypass,
                    });
                }
            }
        }
    }
    out
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

fn hz_text(hz: f32) -> String {
    if hz >= 1000. { format!("{:.2} kHz", hz / 1000.) } else { format!("{hz:.0} Hz") }
}

fn seconds_text(s: f32) -> String {
    if s >= 1. { format!("{s:.2} s") } else { format!("{:.1} ms", s * 1000.) }
}

/// A value as the player reads it.
pub fn readout(p: Param, v: f32) -> String {
    match p {
        Param::Attack | Param::Hold | Param::Decay | Param::Release => seconds_text(v),
        Param::Curve => format!("{v:+.2}"),
        Param::Sustain if v <= 1e-4 => "-inf dB".into(),
        Param::Sustain => format!("{:.1} dB", 20. * v.log10()),
        Param::Cutoff(_) => hz_text(filter_settings(v, 0.).0),
        Param::Resonance(_) => format!("{:.0}%", v * 100.),
        Param::Freq(..) => hz_text(band_settings(v, 0., 0.).0),
        Param::Bandwidth(..) => format!("{:.2} oct", band_settings(0., v, 0.).1),
        Param::Gain(..) => format!("{:+.1} dB", band_settings(0., 0., v).2),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_lays_out_left_to_right_and_places_playheads() {
        let env = Ahdsr { attack: 0.01, curve: 0., hold: 0., decay: 1., sustain: 0.5, release: 3. };
        let shape = envelope(&env);
        assert!(shape.line.windows(2).all(|w| w[1][0] >= w[0][0]), "x only grows");
        assert_eq!(shape.line.last().unwrap()[0], 1.);
        assert_eq!(shape.line[shape.stages[3].start][1], 0.5, "the plateau is the sustain level");
        let head = playhead(&shape, Phase::Decay, 0.75).unwrap();
        let decay = &shape.line[shape.stages[2].clone()];
        assert!(head[0] > decay[0][0] && head[0] < decay[decay.len() - 1][0]);
        // A longer release takes more of the width than a short attack.
        let a = &shape.line[shape.stages[0].clone()];
        let r = &shape.line[shape.stages[4].clone()];
        assert!(r[r.len() - 1][0] - r[0][0] > a[a.len() - 1][0] - a[0][0]);
    }

    #[test]
    fn eq_handles_sit_on_their_band() {
        use crate::fx::{Chain, Effect, Kind, Params, params::{Eq, EqBand}};
        let band = |freq_hz, gain_db| EqBand { freq_hz, bandwidth_oct: 1., gain_db };
        let group = Group {
            fx: Chain {
                slots: vec![Effect {
                    slot: 0,
                    kind: Kind::Filter,
                    version: 0,
                    bypass: false,
                    output_gain: 1.,
                    dry_level: 0.,
                    params: Params::Eq(Eq { bands: vec![band(200., 6.), band(4000., -9.)] }),
                }],
            },
            ..Group::default()
        };
        let model = Model::new(&group, 0, &Edits::default(), None);
        let handles = filter_handles(&model.playing);
        assert_eq!(handles.len(), 2);
        assert!((handles[0].at[0] - freq_x(200.)).abs() < 1e-3);
        assert!((handles[1].at[1] - db_y(-9.)).abs() < 1e-3);
        // The curve peaks where the band does.
        let curve = response(&model.playing);
        let at = |hz: f32| curve.iter().min_by(|a, b| (a[0] - freq_x(hz)).abs().total_cmp(&(b[0] - freq_x(hz)).abs())).unwrap()[1];
        assert!((at(4000.) - db_y(-9.)).abs() < 0.02, "{} vs {}", at(4000.), db_y(-9.));
        // An edit moves the band where the engine would.
        let mut edits = Edits::default();
        edits.set(crate::engine::overrides::Override { group: None, param: Param::Gain(0, 0), offset: 0.1 });
        let edited = Model::new(&group, 0, &edits, None);
        let gain = Param::Gain(0, 0).read(&edited.playing).unwrap();
        assert!((band_settings(0., 0., gain).2 - (6. + 3.6)).abs() < 1e-3);
    }
}
