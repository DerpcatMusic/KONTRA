//! Allocation-free reduction of an existing UI-side audio snapshot to display
//! columns. This module does not read audio-thread state or create a worker pool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reduction {
    pub samples: usize,
    pub columns: usize,
    /// Nonfinite encounters, not unique samples: zoomed-in duplicated columns
    /// may encounter the same source sample more than once.
    pub nonfinite: usize,
}
fn range(column: usize, samples: usize, columns: usize) -> std::ops::Range<usize> {
    // u128 avoids overflowing the index multiplication on 64-bit targets.
    let start = ((column as u128 * samples as u128) / columns as u128) as usize;
    let end = (((column + 1) as u128 * samples as u128) / columns as u128) as usize;
    start.min(samples)..end.max(start + 1).min(samples)
}
/// Preserve both extrema, rather than dropping a narrow transient while
/// selecting one sample per screen pixel. When zoomed in, repeat source samples;
/// this is a display envelope, not an audio resampler.
pub fn waveform_into(samples: &[f32], columns: &mut [[f32; 2]]) -> Reduction {
    let mut stats = Reduction {
        samples: samples.len(),
        columns: columns.len(),
        nonfinite: 0,
    };
    let n = columns.len();
    if samples.is_empty() {
        columns.fill([0., 0.]);
        return stats;
    }
    for (i, column) in columns.iter_mut().enumerate() {
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for &v in &samples[range(i, samples.len(), n)] {
            if v.is_finite() {
                lo = lo.min(v);
                hi = hi.max(v);
            } else {
                stats.nonfinite += 1;
            }
        }
        *column = if lo.is_finite() { [lo, hi] } else { [0., 0.] };
    }
    stats
}
/// Max-preserving spectrum columns. `floor` is explicit so this works for either
/// linear amplitudes (0) or dB values (e.g. -144), without turning empty bins
/// into a false 0 dB peak. The return value is None for a nonfinite floor; the
/// output is then unchanged. Nonfinite input values are skipped.
pub fn spectrum_into(samples: &[f32], columns: &mut [f32], floor: f32) -> Option<Reduction> {
    if !floor.is_finite() {
        return None;
    }
    let mut stats = Reduction {
        samples: samples.len(),
        columns: columns.len(),
        nonfinite: 0,
    };
    let n = columns.len();
    if samples.is_empty() {
        columns.fill(floor);
        return Some(stats);
    }
    for (i, column) in columns.iter_mut().enumerate() {
        let mut peak = floor;
        for &v in &samples[range(i, samples.len(), n)] {
            if v.is_finite() {
                peak = peak.max(v);
            } else {
                stats.nonfinite += 1;
            }
        }
        *column = peak;
    }
    Some(stats)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transient_is_not_dropped() {
        let mut data = [0.; 1024];
        data[523] = 1.;
        data[525] = -0.8;
        let mut out = [[0.; 2]; 16];
        waveform_into(&data, &mut out);
        assert_eq!(out[8], [-0.8, 1.]);
    }
    #[test]
    fn db_peak_stays_negative() {
        let mut out = [0.; 1];
        spectrum_into(&[-90., -12., -48.], &mut out, -144.).unwrap();
        assert_eq!(out, [-12.]);
    }
    #[test]
    fn last_sample_is_included() {
        let mut out = [0.; 3];
        spectrum_into(&[0., 0., 0., 0., 1.], &mut out, 0.).unwrap();
        assert_eq!(out, [0., 0., 1.]);
    }
    #[test]
    fn empty_buffers_are_valid() {
        waveform_into(&[1.], &mut []);
        spectrum_into(&[1.], &mut [], 0.).unwrap();
    }
    #[test]
    fn empty_snapshot_clears_old_data() {
        let mut out = [[1.; 2]; 2];
        waveform_into(&[], &mut out);
        assert_eq!(out, [[0.; 2]; 2]);
    }
    #[test]
    fn zoomed_in_values_repeat() {
        let mut out = [0.; 4];
        spectrum_into(&[1., 2.], &mut out, 0.).unwrap();
        assert_eq!(out, [1., 1., 2., 2.]);
    }
    #[test]
    fn invalid_floor_is_transactional() {
        let mut out = [7.];
        assert!(spectrum_into(&[1.], &mut out, f32::NAN).is_none());
        assert_eq!(out, [7.]);
    }
    #[test]
    fn nonfinite_input_does_not_poison_output() {
        let mut out = [[0.; 2]];
        let s = waveform_into(&[f32::NAN, 2., f32::INFINITY], &mut out);
        assert_eq!(s.nonfinite, 2);
        assert_eq!(out, [[2., 2.]]);
    }
    #[test]
    fn all_small_bucketings_preserve_global_extrema() {
        for n in 1..128 {
            for m in 1..64 {
                let src = (0..n)
                    .map(|i| ((i * 37) % 101) as f32 - 50.)
                    .collect::<Vec<_>>();
                let mut out = vec![[0.; 2]; m];
                waveform_into(&src, &mut out);
                assert_eq!(
                    out.iter().map(|x| x[0]).fold(f32::INFINITY, f32::min),
                    src.iter().copied().fold(f32::INFINITY, f32::min)
                );
                assert_eq!(
                    out.iter().map(|x| x[1]).fold(f32::NEG_INFINITY, f32::max),
                    src.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                );
            }
        }
    }
}
