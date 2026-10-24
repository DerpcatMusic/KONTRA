//! Pair 4: Solid G-EQ bell and shelf sections. Ours is the Solid designer (sampler-core
//! `solid::{peak, low_shelf, high_shelf}`); the candidate is Signalsmith `BiquadStatic` with its
//! Vicanek design (feature `candidates`). Both are fitted per Kontakt row with the same three free
//! parameters (frequency, gain, Q or slope). Fidelity: `docs/re/kontakt-bindings/solid-geq-responses.tsv`
//! (Kontakt 8 G-EQ rendered through Kontakt, LL channel, 10 probes, 48 kHz).

use super::{Fit, PairReport, Quality, side};
use crate::band::Band;
use crate::block::{Block, Factory, Impl, param};
use crate::fit::nelder_mead;
use crate::measure::{self, PROBE_FRAMES};
use sampler_core::dspbench::{SolidShape, solid_section};

const TSV: &str = include_str!("../../../../docs/re/kontakt-bindings/solid-geq-responses.tsv");
const PROBES: [f64; 10] = [30., 60., 100., 200., 500., 1000., 2000., 5000., 10000., 15000.];
const FLOOR_DB: f64 = -60.0;

/// One measured Kontakt row: the section shape by its name, the starting guess, and the dB probes.
pub struct Row {
    pub name: String,
    pub shape: u8,
    pub hz: f64,
    pub db: f64,
    pub q: f64,
    pub target: [f64; 10],
}

/// Shape ids shared by both blocks: 0 peak, 1 low shelf, 2 high shelf.
pub fn rows() -> Vec<Row> {
    TSV.lines()
        .skip(1)
        .filter_map(|line| {
            let c: Vec<&str> = line.split('\t').collect();
            if c.len() < 20 || c[3] != "LL" {
                return None;
            }
            let name = c[2].to_string();
            // Classified by band, not by the word "bell": the HF bell rows are high shelves
            // (their response runs from -25 dB at 30 Hz to +22 dB at 15 kHz).
            let shape = if name.starts_with("LMF") || name.starts_with("HMF") {
                0
            } else if name.starts_with("LF") {
                1
            } else if name.starts_with("HF") {
                2
            } else {
                return None;
            };
            let num = |s: &str| s.parse::<f64>().ok();
            let mut target = [0.0; 10];
            for (i, t) in target.iter_mut().enumerate() {
                *t = num(c[10 + i])?;
            }
            // Start at the geometric mean of the half-power points: the peak finder sits on the probe
            // edge for some HF bells (TSV peak_hz = 30 Hz), the half points do not.
            let (lo, hi) = (num(c[6])?, num(c[7])?);
            let centre = if lo > 0.0 && hi > lo { (lo * hi).sqrt() } else { num(c[4])? };
            let q = num(c[8]).filter(|v| v.is_finite() && *v > 0.05 && *v < 20.0).unwrap_or(0.707);
            // Gain guess: the largest-magnitude probe for a bell (the TSV's peak_db can be the LF
            // minimum of a shelf), the top probe for a high shelf and the bottom probe for a low shelf.
            let db = match shape {
                0 => target.iter().copied().max_by(|a, b| a.abs().total_cmp(&b.abs())).unwrap_or(0.0),
                1 => target[0],
                _ => target[9],
            };
            Some(Row { name, shape, hz: centre.clamp(30.0, 15_000.0), db: db.clamp(-30.0, 30.0), q, target })
        })
        .collect()
}

/// Our side: Solid designer coefficients, direct form I. Keys: `shape`, `hz`, `db`, `q` (Q or slope).
struct Designer {
    rate: f64,
    shape: u8,
    hz: f64,
    db: f64,
    q: f64,
    c: [f64; 5],
    x: [f64; 2],
    y: [f64; 2],
}

impl Designer {
    fn new(rate: f64, shape: u8, hz: f64, db: f64, q: f64) -> Self {
        let mut d = Self { rate, shape, hz, db, q, c: [1.0, 0.0, 0.0, 0.0, 0.0], x: [0.0; 2], y: [0.0; 2] };
        d.design();
        d
    }
    fn design(&mut self) {
        let shape = match self.shape {
            0 => SolidShape::Peak,
            1 => SolidShape::LowShelf,
            _ => SolidShape::HighShelf,
        };
        self.c = solid_section(shape, self.rate, self.hz, self.db, self.q);
    }
}

impl Block for Designer {
    fn process(&mut self, input: &[f32], out: &mut [f32]) {
        let [b0, b1, b2, a1, a2] = self.c;
        for (y, &v) in out.iter_mut().zip(input) {
            let x = f64::from(v);
            let r = b0 * x + b1 * self.x[0] + b2 * self.x[1] - a1 * self.y[0] - a2 * self.y[1];
            self.x = [x, self.x[0]];
            self.y = [r, self.y[0]];
            *y = r as f32;
        }
    }
    fn set(&mut self, key: &str, value: f64) -> bool {
        match key {
            "hz" => self.hz = value,
            "db" => self.db = value,
            "q" => self.q = value,
            "shape" => self.shape = value as u8,
            _ => return false,
        }
        self.design();
        true
    }
    fn reset(&mut self) {
        self.x = [0.0; 2];
        self.y = [0.0; 2];
    }
}

fn make_ours(rate: f64, params: &[(&str, f64)]) -> Box<dyn Block> {
    Box::new(Designer::new(rate, param(params, "shape", 0.0) as u8, param(params, "hz", 1000.0), param(params, "db", 6.0), param(params, "q", 1.0)))
}

#[cfg(feature = "candidates")]
fn make_signalsmith(rate: f64, params: &[(&str, f64)]) -> Box<dyn Block> {
    // design 0 is Vicanek, the default of the Signalsmith matched design.
    Box::new(crate::signalsmith::SigBiquad::new(
        rate,
        param(params, "shape", 0.0) as i32,
        0,
        param(params, "hz", 1000.0),
        param(params, "db", 6.0),
        param(params, "q", 1.0),
    ))
}

pub fn ours_impl() -> Impl {
    Impl { label: "sampler-core Solid G-EQ designer", make: make_ours }
}

#[cfg(feature = "candidates")]
pub fn cand_impl() -> Option<Impl> {
    Some(Impl { label: "Signalsmith BiquadStatic (Vicanek)", make: make_signalsmith })
}

#[cfg(not(feature = "candidates"))]
pub fn cand_impl() -> Option<Impl> {
    None
}

/// Fit one design to one Kontakt row: free `hz`, `db`, `q` from the row's own guess.
fn fit_row(make: Factory, rate: f64, row: &Row) -> (f64, f64) {
    let to_params = |x: &[f64]| {
        vec![
            ("shape", f64::from(row.shape)),
            ("hz", x[0].exp().clamp(20.0, rate * 0.45)),
            ("db", x[1]),
            // Peak: Q. Shelves: slope, which an RBJ shelf needs at or below 1.
            ("q", if row.shape == 0 { x[2].clamp(0.05, 20.0) } else { x[2].clamp(0.05, 1.0) }),
        ]
    };
    let objective = |x: &[f64]| {
        let h = measure::impulse_response(make, rate, &to_params(x), 64, PROBE_FRAMES);
        let db = measure::probe_db(&h, rate, &PROBES);
        db.iter().zip(&row.target).filter(|(_, t)| **t > FLOOR_DB).map(|(d, t)| (d - t).powi(2)).sum::<f64>()
    };
    let x0 = vec![row.hz.ln(), row.db, row.q];
    let best = nelder_mead(objective, &x0, &[0.3, 1.0, 0.3], 150);
    let h = measure::impulse_response(make, rate, &to_params(&best), 64, PROBE_FRAMES);
    let db = measure::probe_db(&h, rate, &PROBES);
    let err: Vec<f64> = db.iter().zip(&row.target).filter(|(_, t)| **t > FLOOR_DB).map(|(d, t)| d - t).collect();
    let mean = err.iter().sum::<f64>() / err.len().max(1) as f64;
    let shape = err.iter().map(|e| (e - mean).abs()).fold(0.0, f64::max);
    (shape, mean)
}

/// Fit every row. `mag_err_db` is the 90th percentile of the per-row shape error (the verdict
/// basis); the worst row is returned beside it, since a few mid-band rows exceed what one biquad can do.
fn fit_all(make: Factory, rate: f64, rows: &[Row]) -> (Fit, f64) {
    let mut shapes = Vec::new();
    let mut offsets = Vec::new();
    for row in rows {
        let (shape, mean) = fit_row(make, rate, row);
        shapes.push(shape);
        offsets.push(mean);
    }
    shapes.sort_by(f64::total_cmp);
    let worst = shapes.last().copied().unwrap_or(f64::NAN);
    let p90 = shapes.get(shapes.len() * 9 / 10).copied().unwrap_or(f64::NAN);
    let fit = Fit {
        mag_err_db: p90,
        mag_err_median_db: shapes.get(shapes.len() / 2).copied().unwrap_or(f64::NAN),
        gain_offset_db: offsets.iter().map(|o| o.abs()).sum::<f64>() / offsets.len().max(1) as f64,
        identity_mag_err_db: None,
        phase_err_deg: None,
        time_err_pct: None,
        cases: shapes.len(),
    };
    (fit, worst)
}

/// Quality of one designer: alias and stability under a fast cutoff sweep of a bell at 1 kHz.
fn quality(make: Factory, rate: f64, block: usize) -> Quality {
    let params = [("shape", 0.0), ("hz", 1000.0), ("db", 6.0), ("q", 1.0)];
    let mod_out = measure::modulated_run(make, rate, &params, "hz", 300.0, 6000.0, block);
    let (stable, peak) = measure::is_stable(&mod_out);
    let (subnormal, spike) = measure::denormal_tail(make, rate, &params, block);
    Quality {
        alias_db: measure::alias_db(&mod_out, rate, 2000.0),
        thd_n_db: measure::thd_n_db(&measure::run(measure::build(make, rate, &params).as_mut(), &crate::signal::sine(rate, 1000.0, 0.5, rate as usize), block), rate, 1000.0),
        noise_dbfs: measure::noise_floor_dbfs(make, rate, &params, block),
        subnormal,
        denormal_spike: spike,
        stable,
        peak_dbfs: peak,
        latency: measure::latency_frames(make, rate, &params),
        ns_per_frame: measure::cpu_ns_per_frame(make, rate, &params, block, 5),
        instr_per_frame: None,
    }
}

pub fn run(band: &Band, ours: Impl, cand: Option<Impl>, rate: f64, block: usize, fit: bool) -> PairReport {
    let rs = rows();
    let reference = format!("Kontakt 8 Solid G-EQ, LL channel, {} rows x {} probes (solid-geq-responses.tsv)", rs.len(), PROBES.len());
    let qo = quality(ours.make, rate, block);
    let qc = cand.map(|c| quality(c.make, rate, block));
    let other_for_ours = qc.unwrap_or(qo);
    let fit_ours = fit.then(|| fit_all(ours.make, rate, &rs));
    let note = |w: Option<f64>| w.map_or("no fit (not the Kontakt rate)".into(), |w| format!("p90 over {} rows; worst row {w:.2} dB", rs.len()));
    let mut sides = vec![side(band, ours.label, qo, fit_ours.map(|f| f.0), &other_for_ours, note(fit_ours.map(|f| f.1)))];
    if let (Some(c), Some(qc)) = (cand, qc) {
        let fit_cand = fit.then(|| fit_all(c.make, rate, &rs));
        sides.push(side(band, c.label, qc, fit_cand.map(|f| f.0), &qo, note(fit_cand.map(|f| f.1))));
    }
    PairReport { id: "eq-bell", title: "Solid G-EQ bell and shelf: ours vs Signalsmith BiquadStatic", reference, rate, block, sides }
}

