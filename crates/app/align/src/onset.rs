//! Where an attack is in a rendered note.

use std::f64::consts::TAU;

/// Mean power of `x` over 5 ms windows a millisecond apart.
fn power(x: &[f32], rate: f64) -> Vec<f64> {
    let hop = (rate / 1000.0).round().max(1.0) as usize;
    let mut sum = Vec::with_capacity(x.len() + 1);
    let mut total = 0f64;
    sum.push(total);
    for &v in x {
        total += f64::from(v) * f64::from(v);
        sum.push(total);
    }
    let win = 5 * hop;
    (0..)
        .map(|i| i * hop)
        .map_while(|at| Some(sum.get(at + win)? - sum.get(at)?))
        .map(|e| e / win as f64)
        .collect()
}

/// When `x`, starting at a note-on, first comes within `db` of its loudest:
/// ms to the middle of the first 5 ms window that does. `None` when silent.
#[must_use]
pub fn onset_ms(x: &[f32], rate: f64, db: f32) -> Option<f32> {
    let p = power(x, rate);
    let peak = p.iter().copied().fold(0.0, f64::max);
    if peak < 1e-12 {
        return None;
    }
    let threshold = peak * 10f64.powf(f64::from(db) / 10.0);
    let at = p.iter().position(|&v| v >= threshold)?;
    Some(at as f32 + 2.5)
}

/// Energy of `x` at `hz` and its next two harmonics, Hann-windowed.
fn partials(x: &[f32], rate: f64, hz: f64) -> f64 {
    let n = x.len() as f64;
    (1..=3)
        .map(|k| {
            let w = TAU * hz * f64::from(k) / rate;
            let (c, s) = x.iter().enumerate().fold((0.0, 0.0), |(c, s), (i, &v)| {
                let i = i as f64;
                let v = f64::from(v) * (0.5 - 0.5 * (TAU * i / n).cos());
                (c + v * (w * i).cos(), s + v * (w * i).sin())
            });
            c * c + s * s
        })
        .sum()
}

/// Lead before the second note-on that a legato take keeps, ms.
pub const LEAD_MS: f64 = 60.0;

/// When the new note (MIDI `new`) of a legato pair takes over: `x` starts
/// [`LEAD_MS`] before its note-on, with only the old note sounding. Energy at
/// the new note's first three partials over 40 ms windows 2 ms apart; the
/// transition is the middle of the first window within `db` of the loudest,
/// ms after the note-on. `None` when the old note alone already comes within
/// 6 dB of that (the partials cannot tell them apart).
#[must_use]
pub fn transition_ms(x: &[f32], rate: f64, new: u8, db: f32) -> Option<f32> {
    let win = (0.040 * rate) as usize;
    let hop = (0.002 * rate) as usize;
    let lead = (LEAD_MS / 1000.0 * rate) as usize;
    if hop == 0 || x.len() < lead + win {
        return None;
    }
    let hz = 440.0 * 2f64.powf((f64::from(new) - 69.0) / 12.0);
    let e: Vec<f64> = (0..)
        .map(|i| i * hop)
        .map_while(|at| x.get(at..at + win))
        .map(|w| partials(w, rate, hz))
        .collect();
    let peak = e.iter().copied().fold(0.0, f64::max);
    let threshold = peak * 10f64.powf(f64::from(db) / 10.0);
    let before = e.first().copied()?;
    if peak < 1e-9 || before * 4.0 > threshold {
        return None;
    }
    let at = e.iter().position(|&v| v >= threshold)?;
    Some(((at * hop + win / 2) as f64 - lead as f64) as f32 / rate as f32 * 1000.0)
}
