//! A live spectrum of one strip: the audio thread's post-fader samples
//! ([`crate::plugin::Scope`]) windowed and transformed here, on the UI
//! thread, at most every [`EVERY`], then smoothed into log-spaced bands
//! with a falling peak hold. Only a spectrum on screen asks for samples.

use super::theme::*;
use super::viz;
use crate::plugin::Scope;
use moose::mui::mui::geometry::Path as DrawPath;
use moose::mui::mui::prelude::*;
use realfft::{RealFftPlanner, RealToComplex, num_complex::Complex32};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Samples per transform: 85 ms at 48 kHz, 12 Hz bins.
const SIZE: usize = 4096;
/// Bands across 20 Hz–20 kHz.
const BANDS: usize = 112;
/// The shown range, dBFS (after the tilt).
pub const FLOOR_DB: f32 = -84.;
pub const TOP_DB: f32 = 0.;
/// Transforms run this often at most.
const EVERY: Duration = Duration::from_millis(33);
/// Falls, in dB a second: the level, and the peak once held.
const FALL: f32 = 36.;
const PEAK_FALL: f32 = 18.;
const HOLD: f32 = 0.9;
/// Tilt, dB an octave about 1 kHz, so a mix's natural roll-off reads flat.
const TILT: f32 = 3.;

/// Levels and peaks to draw, `(x, y)` in unit coordinates.
#[derive(Default)]
pub struct Shape {
    pub level: Vec<[f32; 2]>,
    pub peak: Vec<[f32; 2]>,
}

pub struct Analyser {
    fft: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    input: Vec<f32>,
    bins: Vec<Complex32>,
    scratch: Vec<Complex32>,
    /// Which [`Scope::source`] the bands show.
    source: usize,
    at: Option<Instant>,
    /// Band levels and peaks in dB, and when each peak was set.
    level: Vec<f32>,
    peak: Vec<f32>,
    peak_at: Vec<Instant>,
    shape: Arc<Shape>,
}

impl Default for Analyser {
    fn default() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(SIZE);
        let (bins, scratch) = (fft.make_output_vec(), fft.make_scratch_vec());
        let window = (0..SIZE)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / SIZE as f32).cos())
            .collect();
        Self {
            fft,
            window,
            input: vec![0.; SIZE],
            bins,
            scratch,
            source: 0,
            at: None,
            level: vec![FLOOR_DB; BANDS],
            peak: vec![FLOOR_DB; BANDS],
            peak_at: vec![Instant::now(); BANDS],
            shape: Arc::default(),
        }
    }
}

/// Band `i`'s centre frequency.
fn band_hz(i: usize) -> f32 {
    20. * 1000f32.powf(i as f32 / (BANDS - 1) as f32)
}

pub fn db_y(db: f32) -> f32 {
    ((db - FLOOR_DB) / (TOP_DB - FLOOR_DB)).clamp(0., 1.)
}

impl Analyser {
    /// The spectrum of `source` from `scope` at `rate`, transformed again
    /// when [`EVERY`] has passed; a new source starts from silence.
    pub fn update(&mut self, scope: &Scope, source: usize, rate: f32) -> Arc<Shape> {
        let now = Instant::now();
        if source != self.source {
            self.source = source;
            self.level.fill(FLOOR_DB);
            self.peak.fill(FLOOR_DB);
            self.at = None;
        }
        if self.at.is_some_and(|t| now - t < EVERY) {
            return self.shape.clone();
        }
        let dt = self.at.map_or(0., |t| (now - t).as_secs_f32().min(0.25));
        self.at = Some(now);
        scope.latest(&mut self.input);
        for (x, w) in self.input.iter_mut().zip(&self.window) {
            *x *= w;
        }
        if self.fft.process_with_scratch(&mut self.input, &mut self.bins, &mut self.scratch).is_err() {
            return self.shape.clone();
        }
        // Hann's coherent gain is 1/2: a full-scale sine reads 0 dB.
        let norm = 4. / SIZE as f32;
        let power = |k: usize| self.bins.get(k).map_or(0., |c| c.norm_sqr());
        let bin_hz = rate / SIZE as f32;
        let step = 1000f32.powf(0.5 / (BANDS - 1) as f32);
        let bands: Vec<f32> = (0..BANDS)
            .map(|i| {
                let hz = band_hz(i);
                let (lo, hi) = ((hz / step / bin_hz) as usize, (hz * step / bin_hz).ceil() as usize);
                if hi <= lo + 1 {
                    // Narrower than a bin: between the two nearest.
                    let at = hz / bin_hz;
                    let (k, t) = (at.floor() as usize, at.fract());
                    power(k) * (1. - t) + power(k + 1) * t
                } else {
                    (lo..hi).map(power).fold(0., f32::max)
                }
            })
            .collect();
        for i in 0..BANDS {
            let hz = band_hz(i);
            // A quarter octave or so across neighbours: a smooth line, not bins.
            let p = 0.5 * bands[i] + 0.25 * (bands[i.saturating_sub(1)] + bands[(i + 1).min(BANDS - 1)]);
            let db = 10. * (p.max(1e-20)).log10() + 20. * norm.log10() + TILT * (hz / 1000.).log2();
            let db = db.max(FLOOR_DB);
            let level = &mut self.level[i];
            *level = if db > *level { *level + (db - *level) * 0.6 } else { db.max(*level - FALL * dt) };
            if *level >= self.peak[i] {
                (self.peak[i], self.peak_at[i]) = (*level, now);
            } else if (now - self.peak_at[i]).as_secs_f32() > HOLD {
                self.peak[i] = (self.peak[i] - PEAK_FALL * dt).max(*level);
            }
        }
        let points = |v: &[f32]| (0..BANDS).map(|i| [viz::freq_x(band_hz(i)), db_y(v[i])]).collect();
        self.shape = Arc::new(Shape { level: points(&self.level), peak: points(&self.peak) });
        self.shape.clone()
    }

    /// Anything above the floor, still falling: frames are wanted.
    pub fn busy(&self) -> bool {
        self.source != 0 && self.peak.iter().any(|&p| p > FLOOR_DB + 0.5)
    }
}

/// The spectrum under a graph: one quiet fill with its edge, and the peak
/// hold as a hairline, placed by `place` (unit to canvas).
pub fn draw(out: &mut Vec<Draw>, shape: &Shape, place: impl Fn([f32; 2]) -> Point) {
    let (Some(first), Some(last)) = (shape.level.first(), shape.level.last()) else {
        return;
    };
    let floor = [[last[0], 0.], [first[0], 0.]];
    let area = DrawPath::polyline(shape.level.iter().chain(&floor).map(|&p| place(p)), true);
    out.push(Draw::fill(area, Role::Ink.alpha(0.08)));
    out.push(Draw::stroke(DrawPath::polyline(shape.level.iter().map(|&p| place(p)), false), Role::Ink.alpha(0.2), 1.));
    out.push(Draw::stroke(DrawPath::polyline(shape.peak.iter().map(|&p| place(p)), false), Role::Ink.alpha(0.28), 1.));
}

/// A spectrum on its own: a scale behind, frequencies under it.
pub fn panel(shape: Arc<Shape>, name: &str) -> El {
    let graph = canvas(move |s| {
        let place = |[x, y]: [f32; 2]| Point::new(f64::from(x) * s.width, (1. - f64::from(y)) * s.height);
        let mut out = Vec::new();
        for hz in [100., 1000., 10_000.] {
            let x = place([viz::freq_x(hz), 0.]).x.round();
            out.push(Draw::fill(rect(x, 0., 1., s.height), hairline()));
        }
        for db in [-24., -48., -72.] {
            let y = place([0., db_y(db)]).y.round();
            out.push(Draw::fill(rect(0., y, s.width, 1.), hairline()));
        }
        draw(&mut out, &shape, place);
        out
    })
    .flex(1)
    .min_h(CONTROL * 3.)
    .w(Len::Pct(100.))
    .fill(Role::Field)
    .clip()
    .named(name.to_owned());
    col![
        graph,
        row![caption("20 Hz").fill(Role::Dim), spacer(), caption("1 kHz").fill(Role::Dim), spacer(), caption("20 kHz").fill(Role::Dim)]
            .shrink(0)
    ]
    .gap(TIGHT)
    .flex(1)
    .min_h(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_peaks_in_its_band_at_its_level() {
        let scope = Scope::default();
        let rate = 48_000.;
        let hz = 1000.;
        let sine: Vec<f32> = (0..crate::plugin::SCOPE)
            .map(|n| 0.5 * (std::f32::consts::TAU * hz * n as f32 / rate).sin())
            .collect();
        scope.push(&sine);
        let mut a = Analyser::default();
        // Rising levels ease in: a few looks settle them.
        for _ in 0..12 {
            a.at = None;
            a.update(&scope, 1, rate);
        }
        let i = (0..BANDS).max_by(|&x, &y| a.level[x].total_cmp(&a.level[y])).unwrap();
        assert!((band_hz(i) / hz).log2().abs() < 0.1, "peak at {} Hz", band_hz(i));
        // -6 dBFS, no tilt at 1 kHz, a little under once spread over its neighbours.
        assert!((-10.0..=-5.0).contains(&a.level[i]), "{} dB", a.level[i]);
        assert!(a.level[0] < -60., "20 Hz reads {}", a.level[0]);
        assert!(a.busy());
    }
}
