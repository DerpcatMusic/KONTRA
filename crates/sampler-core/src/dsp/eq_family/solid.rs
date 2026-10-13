//! Solid G-EQ parameter splines (BEffectSSLGEQ core descriptor): eleven-knot cubic splines,
//! initializer 0x141a9a510, evaluator 0x141a98340 (KONTAKT_DSP_LAWS "Solid EQ parameter splines",
//! 480 committed evaluations). PROVED: the knot tables, the grid D(F(i*F(.1))), the zero-slope
//! left / natural right boundary and the evaluation formula.
//!
//! INFERRED (not proved): which core table drives which section. The labels are core descriptor
//! indices, not saved-field indices; each frequency table covers a range unique to one SSL
//! section, and the gain/Q tables follow their frequency table in descriptor order. Bank 16
//! differs from bank 0 only at the HMF and LMF gain tables; the constructor-to-bank binding is
//! open, so bank 0 is used. The Q tables are read as `1/(2q)` (the RBJ damping), which maps their
//! 0.65..0.2 knots to Q 0.77..2.5; the unit itself is unproved.
use sampler_ir::fx::eq::SolidSection;
use std::sync::LazyLock;

const N: usize = 11;

/// The native grid: F(i * F(0.1)) widened, so it is not exact decimal tenths.
fn grid() -> [f64; N] {
    let tenth = 0.1_f32;
    std::array::from_fn(|i| f64::from(i as f32 * tenth))
}

#[derive(Clone, Copy)]
pub(super) struct Spline {
    x: [f64; N],
    y: [f64; N],
    m: [f64; N],
}

impl Spline {
    /// Second derivatives for a spline with zero slope at the left end and zero second
    /// derivative at the right (the checked boundary controls 11 and 1e32).
    pub(super) fn new(y: [f64; N]) -> Self {
        let x = grid();
        let (mut m, mut u) = ([0.; N], [0.; N]);
        m[0] = -0.5;
        u[0] = 3. / (x[1] - x[0]) * ((y[1] - y[0]) / (x[1] - x[0]));
        for i in 1..N - 1 {
            let sig = (x[i] - x[i - 1]) / (x[i + 1] - x[i - 1]);
            let p = sig * m[i - 1] + 2.;
            m[i] = (sig - 1.) / p;
            let d = (y[i + 1] - y[i]) / (x[i + 1] - x[i]) - (y[i] - y[i - 1]) / (x[i] - x[i - 1]);
            u[i] = (6. * d / (x[i + 1] - x[i - 1]) - sig * u[i - 1]) / p;
        }
        m[N - 1] = 0.;
        for k in (0..N - 1).rev() {
            m[k] = m[k] * m[k + 1] + u[k];
        }
        Self { x, y, m }
    }

    /// Evaluate at `u`, clamped to the grid's ends.
    pub(super) fn eval(&self, u: f64) -> f64 {
        let u = if u.is_nan() { 0. } else { u.clamp(self.x[0], self.x[N - 1]) };
        let i = (0..N - 1).rev().find(|&i| u >= self.x[i]).unwrap_or(0);
        let h = self.x[i + 1] - self.x[i];
        let (a, b) = ((self.x[i + 1] - u) / h, (u - self.x[i]) / h);
        a * self.y[i]
            + b * self.y[i + 1]
            + ((a * a * a - a) * self.m[i] + (b * b * b - b) * self.m[i + 1]) * h * h / 6.
    }
}

pub(super) const GAIN_HF: [f64; N] = [-20.5, -18., -11., -6., -2., 0., 2., 6., 11., 18., 20.5];
pub(super) const FREQ_HF: [f64; N] =
    [1539., 1710., 2400., 3980., 5460., 6820., 9160., 13100., 17813., 21000., 21800.];
pub(super) const GAIN_HMF: [f64; N] = [-20., -17.5, -11., -6., -2., 0., 2., 6., 11., 17.5, 20.];
pub(super) const FREQ_HMF: [f64; N] =
    [692.48, 730.72, 1022.48, 1614.67, 2112.54, 2619.29, 3291.53, 4544.29, 5634.35, 6444.74, 6892.65];
pub(super) const Q_HMF: [f64; N] = [0.65, 0.62, 0.58, 0.5, 0.45, 0.4, 0.37, 0.31, 0.26, 0.22, 0.22];
pub(super) const GAIN_LMF: [f64; N] = [-21.5, -18., -11., -6., -2., 0., 2., 6., 11., 18., 21.5];
pub(super) const FREQ_LMF: [f64; N] =
    [223., 239., 323., 494., 668., 809., 1050., 1479., 1841., 2083., 2200.];
pub(super) const Q_LMF: [f64; N] = [0.65, 0.62, 0.58, 0.5, 0.45, 0.4, 0.37, 0.3, 0.25, 0.21, 0.2];
pub(super) const GAIN_LF: [f64; N] = [-19.5, -16., -10., -5.3, -2., 0., 2., 5.3, 10., 16., 19.5];
pub(super) const FREQ_LF: [f64; N] =
    [40.43, 44.64, 63.44, 101.36, 140.21, 183.73, 245.15, 351.55, 513.3, 614.68, 697.34];
pub(super) const FREQ_HP: [f64; N] = [14.5, 16., 23., 38., 57., 74., 100., 162., 280., 380., 460.];
pub(super) const FREQ_LP: [f64; N] =
    [31000., 29000., 25200., 19400., 15500., 12300., 10500., 8100., 4800., 3450., 3300.];

/// A section's splines. `gain` and `q` are absent where the section has no such knob.
pub(super) struct Section {
    pub gain: Option<Spline>,
    pub frequency: Spline,
    pub q: Option<Spline>,
}

pub(super) static SECTIONS: LazyLock<[Section; 6]> = LazyLock::new(|| {
    let s = Spline::new;
    [
        Section { gain: Some(s(GAIN_LF)), frequency: s(FREQ_LF), q: None },
        Section { gain: Some(s(GAIN_LMF)), frequency: s(FREQ_LMF), q: Some(s(Q_LMF)) },
        Section { gain: Some(s(GAIN_HMF)), frequency: s(FREQ_HMF), q: Some(s(Q_HMF)) },
        Section { gain: Some(s(GAIN_HF)), frequency: s(FREQ_HF), q: None },
        Section { gain: None, frequency: s(FREQ_HP), q: None },
        Section { gain: None, frequency: s(FREQ_LP), q: None },
    ]
});

pub(super) fn section(which: SolidSection) -> &'static Section {
    &SECTIONS[which as usize]
}

/// Hertz, Q and decibels for the three saved knobs (each 0..=1) of `which`. Sections without a
/// gain or Q knob return 0 dB and Q 1/sqrt(2).
pub(super) fn physical(which: SolidSection, frequency: f64, q: f64, gain: f64) -> (f64, f64, f64) {
    let s = section(which);
    (
        s.frequency.eval(frequency),
        s.q.as_ref().map_or(std::f64::consts::FRAC_1_SQRT_2, |t| 0.5 / t.eval(q)),
        s.gain.as_ref().map_or(0., |t| t.eval(gain)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 24 tables x committed grid knots, second derivatives and evaluations
    /// (KONTAKT_DSP_LAWS.vectors.json `solid_splines`, original-instruction run).
    #[test]
    fn splines_match_the_committed_native_vectors() {
        let all: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../docs/architecture-v2/KONTAKT_DSP_LAWS.vectors.json"
        ))
        .unwrap();
        let mut checked = 0;
        for row in all["solid_splines"].as_array().unwrap() {
            let name = row["parameter"].as_str().unwrap();
            let knots: [f64; N] = std::array::from_fn(|i| row["knots"][i].as_f64().unwrap());
            let spline = Spline::new(knots);
            for i in 0..N {
                let want = row["second_derivatives"][i].as_f64().unwrap();
                assert!((spline.m[i] - want).abs() <= 1e-9 * want.abs().max(1.), "{name} m[{i}]");
            }
            for v in row["vectors"].as_array().unwrap() {
                let (x, want) = (v["x"].as_f64().unwrap(), v["internal"].as_f64().unwrap());
                let got = spline.eval(x);
                assert!((got - want).abs() <= 1e-9 * want.abs().max(1.), "{name} x={x}: {got} vs {want}");
                checked += 1;
            }
        }
        assert!(checked >= 400, "{checked}");
    }

    #[test]
    fn section_tables_are_the_committed_ones() {
        let (hz, q, db) = physical(SolidSection::HighMid, 0.5, 0.5, 1.);
        assert!((hz - 2619.29).abs() < 1e-6);
        assert!((q - 0.5 / 0.4).abs() < 1e-6);
        assert!((db - 20.).abs() < 1e-9);
        assert_eq!(physical(SolidSection::LowPass, 0., 0., 0.).0, 31000.);
    }
}
