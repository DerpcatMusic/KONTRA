//! Kontakt 8 Filter Reference: Formant I and II (Talk, Sharp, Size).
//! Same-SHA Ghidra FUN_140b05ac0/FUN_140b00750, radare2 tables
//! 0x144fcea10/0x144fcea90/0x144fceb10 and polynomials 0x149e745e0/0x149e74600.
//! The tables hold ten rows. The core setter FUN_140b05ac0 adds 5.0 to the Talk position
//! while its fourth control (index 3) is above 0.5, which selects rows 5..=9; the module
//! that drives that control is Formant II (the type 91 binding is inferred, not measured).
use super::super::{Planar, ProcessorState};

#[rustfmt::skip]
const FREQUENCIES: [[f32; 3]; 10] = [
    [233., 326., 1914.], [317., 451., 2270.], [726., 1190., 2383.],
    [429., 1920., 2987.], [370., 2578., 4731.],
    [247., 698., 2055.], [165., 830., 2082.], [632., 1095., 2463.],
    [695., 1760., 2470.], [165., 2160., 3235.],
];
#[rustfmt::skip]
const RADII: [[f32; 3]; 10] = [
    [0.9039, 0.9233, 0.9785], [0.9039, 0.9233, 0.9339], [0.9436, 0.9613, 0.9132],
    [0.9573, 0.9573, 0.9556], [0.9436, 0.9706, 0.9666],
    [0.9039, 0.9233, 0.9785], [0.9039, 0.9233, 0.9344], [0.9436, 0.9613, 0.9132],
    [0.9573, 0.9573, 0.9556], [0.9436, 0.973, 0.975],
];
#[rustfmt::skip]
const AMPLITUDES: [[f32; 3]; 10] = [
    [0.44669, 0.044976, 0.029107], [0.21727, 0.27102, 0.10339], [0.5176, 0.4384, 0.3303],
    [0.56235, 0.71781, 0.37498], [0.45, 0.6675, 0.5295],
    [0.153, 0.2353, 0.3523], [0.11831, 0.38681, 0.5389], [0.25792, 0.69024, 0.58885],
    [0.40272, 0.68783, 0.77625], [0.16481, 0.63096, 0.56235],
];

pub(crate) struct Formant {
    bands: [[f32; 4]; 3], // b0, a1, a2, amplitude (b2 = -b0)
    pub(crate) knobs: [f32; 3],
}

impl Formant {
    // shortcut: saved controls only; add native 15ms ramps when live Formant controls are bound.
    pub(crate) fn compile([talk, sharp, size]: [f32; 3], second: bool, rate: u32) -> Self {
        // FUN_140b054e0: row = (5 if second else 0) + 4 Talk, clamped to 9 - 1 ulp.
        let position = (if second { 5. } else { 0. } + talk * 4.).min(f32::from_bits(0x410f_ffff));
        let row = position as usize;
        let fraction = position - row as f32;
        let interpolate = |table: [[f32; 3]; 10], band| {
            table[row][band] + (table[row + 1][band] - table[row][band]) * fraction
        };
        let frequency_scale = (1. / rate as f32) * std::f32::consts::TAU;
        let size_scale = size + size.max(0.5);
        let sharpness = (2. - sharp) * sharp;
        let radius_scale = 0.8999999761581421 + 0.10000002384185791 * sharpness;
        let amplitude_scale = (1. + sharpness * sharpness) * 5.;
        let bands = std::array::from_fn(|band| {
            let z = (interpolate(FREQUENCIES, band) * frequency_scale) * size_scale;
            let z2 = z * z;
            let sine = (((((-2.39e-8 * z2 + 2.7526e-6) * z2 - 0.000198409) * z2 + 0.00833333)
                * z2
                - 0.166667)
                * z2
                + 1.)
                * z;
            let cosine = ((((-2.605e-7 * z2 + 2.47609e-5) * z2 - 0.00138884) * z2 + 0.0416666)
                * z2
                - 0.499923)
                * z2
                + 1.;
            let g = sine * (1. - interpolate(RADII, band) * radius_scale);
            let inverse = 1. / (1. + g);
            [
                inverse * g,
                cosine * -inverse * 2.,
                (1. - g) * inverse,
                interpolate(AMPLITUDES, band) * amplitude_scale,
            ]
        });
        Self {
            bands,
            knobs: [talk, sharp, size],
        }
    }

    pub(crate) fn process(&self, state: &mut ProcessorState, block: &mut Planar, len: usize) {
        for (channel, samples) in block.iter_mut().enumerate() {
            for sample in &mut samples[..len] {
                let input = *sample as f32;
                let mut output = 0_f32;
                for (band, &[b0, a1, a2, amplitude]) in self.bands.iter().enumerate() {
                    let at = channel * 6 + band * 2;
                    let w1 = state.aux[at] as f32;
                    let w2 = state.aux[at + 1] as f32;
                    let w = input - (w1 * a1 + w2 * a2);
                    output += (b0 * w + -b0 * w2) * amplitude;
                    state.aux[at + 1] = f64::from(w1);
                    state.aux[at] = f64::from(w);
                }
                *sample = f64::from(output);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_table_starts_five_rows_in() {
        // FUN_140b054e0: the flag adds 5.0 rows to 4 Talk, so Talk 0 of table 2 is Talk 1.25 of table 1.
        let a = Formant::compile([0., 0.5, 0.5], true, 48_000);
        let b = Formant::compile([1.25, 0.5, 0.5], false, 48_000);
        assert_eq!(a.bands, b.bands);
    }

    #[test]
    fn second_table_row_five_matches_libm_coefficients() {
        let (sharp, size, rate) = (0.5f64, 0.5f64, 48_000f64);
        let f = Formant::compile([0., sharp as f32, size as f32], true, 48_000);
        let t = (2. - sharp) * sharp;
        let radius_scale = 0.9 + 0.1 * t;
        let amplitude_scale = 5. * (1. + t * t);
        for (band, (hz, radius, amplitude)) in
            [(247., 0.9039, 0.153), (698., 0.9233, 0.2353), (2055., 0.9785, 0.3523)]
                .into_iter()
                .enumerate()
        {
            let z = hz * std::f64::consts::TAU / rate * (size + size.max(0.5));
            let g = z.sin() * (1. - radius * radius_scale);
            let [b0, a1, a2, amp] = f.bands[band].map(f64::from);
            assert!((b0 - g / (1. + g)).abs() < 1e-4, "band {band} b0");
            assert!((a1 + 2. * z.cos() / (1. + g)).abs() < 1e-4, "band {band} a1");
            assert!((a2 - (1. - g) / (1. + g)).abs() < 1e-4, "band {band} a2");
            assert!((amp - amplitude * amplitude_scale).abs() < 1e-4, "band {band} amplitude");
        }
    }
}
