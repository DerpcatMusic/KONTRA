//! Pitch of a sample with no note in its name: YIN (de Cheveigné and
//! Kawahara, 2002) over a few windows of its body.

/// The detected pitch as a fractional MIDI note (69 = A at 440 Hz), and a
/// confidence 0..=1: one minus YIN's aperiodicity, the median over windows.
pub fn detect(mono: &[f32], rate: u32) -> Option<(f32, f32)> {
    const WINDOW: usize = 2048;
    // Skip the attack: the body of the note is steadier.
    let start = (rate as usize / 20).min(mono.len().saturating_sub(2 * WINDOW));
    let body = &mono[start..];
    // ponytail: fixed four windows; vote over more if long samples disagree.
    let mut found: Vec<(f32, f32)> = (0..4)
        .map(|n| n * WINDOW / 2)
        .filter(|at| at + 2 * WINDOW <= body.len())
        .filter_map(|at| yin(&body[at..at + 2 * WINDOW], WINDOW, rate))
        .collect();
    if found.is_empty() {
        return None;
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (hz, _) = found[found.len() / 2];
    let mut confidence: Vec<f32> = found.iter().map(|f| f.1).collect();
    confidence.sort_by(f32::total_cmp);
    // Windows that disagree by more than a semitone cost confidence.
    let agree = found.iter().filter(|f| (12.0 * (f.0 / hz).log2()).abs() < 0.5).count() as f32 / found.len() as f32;
    Some((69.0 + 12.0 * (hz / 440.0).log2(), confidence[confidence.len() / 2] * agree))
}

/// One YIN estimate over `x` (twice `w` long): (frequency, 1 - aperiodicity).
fn yin(x: &[f32], w: usize, rate: u32) -> Option<(f32, f32)> {
    // Silence has no pitch.
    if x.iter().map(|s| s * s).sum::<f32>() / (x.len() as f32) < 1e-8 {
        return None;
    }
    let max_tau = w;
    // Difference function d(tau), then its cumulative mean normalized form.
    let mut d = vec![0.0f32; max_tau];
    for (tau, slot) in d.iter_mut().enumerate().skip(1) {
        *slot = (0..w).map(|j| (x[j] - x[j + tau]).powi(2)).sum();
    }
    let mut cmnd = vec![1.0f32; max_tau];
    let mut running = 0.0;
    for tau in 1..max_tau {
        running += d[tau];
        cmnd[tau] = if running > 0.0 { d[tau] * tau as f32 / running } else { 1.0 };
    }
    // The first dip under the threshold, followed to its bottom; else the global minimum.
    let min_tau = (rate / 5000).max(2) as usize;
    let tau = (min_tau..max_tau - 1)
        .find(|&t| cmnd[t] < 0.15)
        .map(|mut t| {
            while t + 1 < max_tau && cmnd[t + 1] < cmnd[t] {
                t += 1;
            }
            t
        })
        .or_else(|| (min_tau..max_tau - 1).min_by(|&a, &b| cmnd[a].total_cmp(&cmnd[b])))?;
    // Parabolic interpolation around the dip.
    let (a, b, c) = (cmnd[tau - 1], cmnd[tau], cmnd[tau + 1]);
    let shift = if a + c - 2.0 * b != 0.0 { 0.5 * (a - c) / (a + c - 2.0 * b) } else { 0.0 };
    let period = tau as f32 + shift.clamp(-1.0, 1.0);
    Some((rate as f32 / period, (1.0 - b).clamp(0.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, rate: u32, seconds: f32, harmonics: bool) -> Vec<f32> {
        (0..(rate as f32 * seconds) as usize)
            .map(|n| {
                let t = n as f32 / rate as f32;
                let p = std::f32::consts::TAU * hz * t;
                if harmonics { 0.5 * p.sin() + 0.3 * (2.0 * p).sin() + 0.2 * (3.0 * p).sin() } else { 0.5 * p.sin() }
            })
            .collect()
    }

    #[test]
    fn synthetic_tones_are_found() {
        for (hz, midi) in [(65.41, 36.0), (110.0, 45.0), (261.63, 60.0), (440.0, 69.0), (1046.5, 84.0)] {
            for harmonics in [false, true] {
                let (note, confidence) = detect(&tone(hz, 44100, 0.5, harmonics), 44100).unwrap();
                assert!((note - midi).abs() < 0.1, "{hz} Hz read as {note}");
                assert!(confidence > 0.8, "{hz} Hz confidence {confidence}");
            }
        }
        // Noise has a pitch of sorts, but no confidence in it.
        let mut seed = 1u32;
        let noise: Vec<f32> = (0..22050)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / (1 << 24) as f32 - 0.5
            })
            .collect();
        assert!(detect(&noise, 44100).is_none_or(|(_, c)| c < 0.5));
        assert!(detect(&vec![0.0; 22050], 44100).is_none());
    }
}
