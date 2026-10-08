//! Port from v1 0cb7a8a0:src/engine/filter/models.rs::formant.
//! ponytail: v1's vowel model; native Formant I coefficients need calibration.
use sampler_ir::{Filter, FilterKind, Frequency, Gain, Processor, Resonance};

const VOWELS: [[f32; 3]; 5] = [
    [730., 1090., 2440.],
    [530., 1840., 2480.],
    [270., 2290., 3010.],
    [570., 840., 2410.],
    [300., 870., 2240.],
];

pub(super) fn sections([talk, sharp, size]: [f32; 3]) -> Option<[Processor; 3]> {
    if ![talk, sharp, size]
        .iter()
        .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
    {
        return None;
    }
    let x = talk * (VOWELS.len() - 1) as f32;
    let (i, t) = ((x as usize).min(VOWELS.len() - 2), x.fract());
    let t = if x >= (VOWELS.len() - 1) as f32 {
        1.
    } else {
        t
    };
    let q = 2. + 18. * sharp;
    let bandwidth = 2. / std::f32::consts::LN_2 * (1. / (2. * q)).asinh();
    // Keep v1's float32 bandwidth conversion before the existing IR kernel.
    let q = 1. / (2. * (std::f32::consts::LN_2 * 0.5 * bandwidth).sinh());
    Some(std::array::from_fn(|band| {
        let hz = VOWELS[i][band] + (VOWELS[i + 1][band] - VOWELS[i][band]) * t;
        let hz = hz * (2. * (size - 0.5)).exp2();
        Processor::Filter(Filter {
            kind: FilterKind::Peak {
                gain: Gain::Decibels(15.),
            },
            cutoff: Frequency::Hertz(f64::from(hz)),
            resonance: Resonance::Q(f64::from(q)),
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vowel_endpoints_size_and_sharpness_match_v1() {
        for (talk, frequencies) in [
            (0., VOWELS[0]),
            (0.25, VOWELS[1]),
            (0.5, VOWELS[2]),
            (1., VOWELS[4]),
        ] {
            for size in [0., 0.5, 1.] {
                for (section, hz) in sections([talk, 0.5, size])
                    .unwrap()
                    .into_iter()
                    .zip(frequencies)
                {
                    let Processor::Filter(filter) = section else {
                        panic!()
                    };
                    assert_eq!(
                        filter.cutoff,
                        Frequency::Hertz(f64::from(hz * (2. * (size - 0.5)).exp2()))
                    );
                    let Resonance::Q(q) = filter.resonance else {
                        panic!()
                    };
                    assert!((q - 11.).abs() < 1e-5);
                }
            }
        }
        assert!(sections([f32::NAN, 0.5, 0.5]).is_none());
        assert!(sections([0.5, 1.1, 0.5]).is_none());
    }
}
