//! One definition of "matched" for every KONTRA-versus-Kontakt comparison, so
//! both sides are measured at the same level and settings. Included with
//! `#[path]` by each comparison test; use nothing else to set up or measure.
//!
//! Kontakt side (KONTAKT_REFERENCE.md protocol): master 0 dB (unity, measured),
//! 48 kHz, controllers as the scenario sends them (power-on state when it sends
//! none), instrument volume, pan and tune as saved in the .nki, levels per
//! channel.
//! KONTRA side: the plan at `RATE`, no output gain stage exists (0 dB), the
//! instrument volume/pan/tune are loaded as saved, and the scenario's
//! controllers are the only controller input.
#![allow(dead_code)]

pub const RATE: u32 = 48_000;

/// Fails unless `plan` renders at the reference rate.
pub fn assert_matched(plan: &sampler_core::Prepared) {
    assert_eq!(
        plan.sample_rate(),
        RATE,
        "reference comparisons run at 48 kHz"
    );
}

/// Load options for a comparison: the reference rate, everything else default.
pub fn options(keys: std::ops::RangeInclusive<u8>) -> sampler_kontakt::Options {
    sampler_kontakt::Options {
        rate: RATE,
        keys,
        ..Default::default()
    }
}

/// UMP MIDI 1.0 words sending `controllers` (cc, value) before a note.
pub fn controller_words(controllers: &[(u8, u8)]) -> Vec<u32> {
    controllers
        .iter()
        .map(|&(cc, value)| 0x20B0_0000 | u32::from(cc) << 8 | u32::from(value))
        .collect()
}

/// Per-channel levels of `out` over `from..to` seconds, in dBFS.
#[derive(Clone, Copy, Debug)]
pub struct Levels {
    pub peak: [f64; 2],
    pub rms: [f64; 2],
}

pub fn levels(out: &[[f32; 2]], from: f64, to: f64) -> Levels {
    let window =
        &out[(from * f64::from(RATE)) as usize..((to * f64::from(RATE)) as usize).min(out.len())];
    let db = |x: f64| 20.0 * (x + 1e-12).log10();
    let channel = |c: usize| {
        let peak = window
            .iter()
            .fold(0f64, |p, f| p.max(f64::from(f[c]).abs()));
        let power = window.iter().map(|f| f64::from(f[c]).powi(2)).sum::<f64>();
        (db(peak), db((power / window.len() as f64).sqrt()))
    };
    let (l, r) = (channel(0), channel(1));
    Levels {
        peak: [l.0, r.0],
        rms: [l.1, r.1],
    }
}

impl Levels {
    /// max(|L|, |R|) peak: the metric of the full-note references.
    pub fn max_peak(&self) -> f64 {
        self.peak[0].max(self.peak[1])
    }
}

/// Whether both channels of `got` are within `tolerance` dB of `want`.
pub fn within(got: [f64; 2], want: [f64; 2], tolerance: f64) -> bool {
    got.iter().zip(want).all(|(g, w)| (g - w).abs() < tolerance)
}
