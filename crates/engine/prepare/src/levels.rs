//! A wavetable zone's cycles, band-limited at every level a voice may read.

use kontra_stream::{Sample, StreamClient};
use kontra_voice::{CYCLE, LEVELS, Read, SIZE, TOP, Table};
use realfft::RealFftPlanner;
use realfft::num_complex::Complex;

/// A level whose dropped harmonics all stay under this share of the
/// cycle's loudest (−90 dB) keeps the next level's frames instead: the two
/// would differ only below 16-bit noise.
const QUIET: f32 = 3.16e-5;

/// The table of each whole cycle in `first..end`, read by `read`, at every
/// level. Level `k` keeps a cycle's harmonics up to
/// `TOP[k] / 2`, resynthesised at `SIZE[k]` frames; Kontakt builds the same
/// levels at the same sizes, but keeps them as 16-bit steps of the table's
/// peak, half its memory. `None` under one cycle in memory.
pub(crate) fn table(
    sample: &Sample,
    client: &StreamClient,
    (first, end): (u64, u64),
    read: Read,
) -> Option<Table> {
    let cycles = end.checked_sub(first)? / CYCLE;
    let mut planner = RealFftPlanner::<f32>::new();
    let forward = planner.plan_fft_forward(CYCLE as usize);
    let mut frames = vec![[0.0f32; 2]; CYCLE as usize];
    let mut cycle = forward.make_input_vec();
    let mut spectrum = forward.make_output_vec();
    let mut data = Vec::new();
    let mut spans = Vec::with_capacity(cycles as usize * LEVELS);
    for c in 0..cycles {
        if sample.read(client, first + c * CYCLE, &mut frames).missing > 0 {
            return None;
        }
        for (x, [l, r]) in cycle.iter_mut().zip(&frames) {
            *x = (l + r) * 0.5;
        }
        forward.process(&mut cycle, &mut spectrum).ok()?;
        let peak = spectrum
            .iter()
            .skip(1)
            .map(|h| h.norm())
            .fold(0.0, f32::max);
        // The level last built, its harmonics and where it lies: a level
        // with fewer harmonics shares it while the ones it would drop are
        // quiet.
        let mut kept: Option<(usize, (u32, u32))> = None;
        for (&top, &size) in TOP.iter().zip(&SIZE) {
            let harmonics = top as usize / 2;
            let quiet = |above: usize| {
                (spectrum.get(harmonics + 1..=above))
                    .is_none_or(|band| band.iter().all(|h| h.norm() <= peak * QUIET))
            };
            let span = match kept {
                Some((above, span)) if quiet(above) => span,
                _ => {
                    let span = (u32::try_from(data.len()).ok()?, size);
                    data.extend(resynth(&mut planner, &spectrum, harmonics, size as usize)?);
                    kept = Some((harmonics, span));
                    span
                }
            };
            spans.push(span);
        }
    }
    let peak = data
        .iter()
        .fold(0.0f32, |m, x| m.max(x.abs()))
        .max(f32::MIN_POSITIVE);
    let steps = data
        .iter()
        .map(|x| (x / peak * 32767.0).round() as i16)
        .collect();
    Table::new((steps, peak / 32767.0), spans.into(), read)
}

/// The harmonics of `spectrum` (a [`CYCLE`]-frame transform) up to
/// `harmonics`, as one cycle of `size` frames.
fn resynth(
    planner: &mut RealFftPlanner<f32>,
    spectrum: &[Complex<f32>],
    harmonics: usize,
    size: usize,
) -> Option<Vec<f32>> {
    let inverse = planner.plan_fft_inverse(size);
    let mut bins = inverse.make_input_vec();
    for (bin, h) in bins.iter_mut().zip(spectrum).take(harmonics + 1) {
        *bin = h / CYCLE as f32;
    }
    if let Some(dc) = bins.first_mut() {
        dc.im = 0.0;
    }
    let mut out = inverse.make_output_vec();
    inverse.process(&mut bins, &mut out).ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use realfft::RealFftPlanner;

    /// A saw's first 300 harmonics: each level keeps exactly its own.
    #[test]
    fn each_level_keeps_its_harmonics() {
        let mut planner = RealFftPlanner::<f32>::new();
        let mut spectrum = vec![Complex::new(0.0, 0.0); CYCLE as usize / 2 + 1];
        for (h, bin) in spectrum.iter_mut().enumerate().skip(1).take(300) {
            *bin = Complex::new(0.0, -(CYCLE as f32) / h as f32);
        }
        // Level 11 keeps 80 harmonics in 512 frames.
        let level = resynth(&mut planner, &spectrum, 80, 512).unwrap();
        let fft = planner.plan_fft_forward(512);
        let mut back = fft.make_output_vec();
        fft.process(&mut level.clone(), &mut back).unwrap();
        for (h, bin) in back.iter().enumerate().skip(1) {
            let want = if h <= 80 { 512.0 / h as f32 } else { 0.0 };
            assert!((bin.norm() - want).abs() < 1e-2 * 512.0, "harmonic {h}");
        }
    }
}
