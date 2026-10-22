//! Kontakt 8 Filter Reference: Formant I and II (Talk, Sharp, Size).
//! Same-SHA Ghidra FUN_140b05ac0/FUN_140b00750, radare2 tables
//! 0x144fcea10/0x144fcea90/0x144fceb10 and polynomials 0x149e745e0/0x149e74600.
//! The tables hold ten rows. The setter FUN_140b05ac0 stores `0.5 < control[3]` in a flag, and
//! FUN_140b054e0 adds 5.0 to the Talk position when that flag is set, selecting rows 5..=9
//! (Formant II, saved type 91, case 0x5b of FUN_1409619b0). The static flag value for type 91
//! is not yet read as a constant (see docs/re/engine/filt-formant-effect.md).
use super::super::{BLOCK, Planar, ProcessorState};

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

    /// Kontakt 8 magnitude responses (Wine oracle, `tools/kontakt-oracle/eqrender.py`, 48 kHz,
    /// impulse through the LL channel against the bypass reference). Rows: Talk, Sharp, Size,
    /// second table, dB at 30..15000 Hz. The first step of the render (Talk 0 on table 1) reads
    /// back as the Talk 0.5 response (its first write was lost), so it is not used.
    #[test]
    fn formant_responses_match_kontakt_measurements() {
        const PROBES: [f64; 10] = [30., 60., 100., 200., 500., 1000., 2000., 5000., 10000., 15000.];
        const KONTAKT: [(f32, f32, f32, bool, [f64; 10]); 17] = [
    (0.25,0.5,0.5, false, [-23.28, -17.08, -12.20, -3.70, 5.18, -10.38, -9.24, -17.06, -24.96, -30.90]),
    (0.5,0.5,0.5, false, [-27.01, -20.95, -16.44, -10.06, 1.35, -2.15, -0.77, -5.56, -13.75, -19.73]),
    (0.75,0.5,0.5, false, [-24.93, -18.81, -14.14, -6.89, 3.53, -8.48, 13.38, -2.83, -11.75, -17.83]),
    (1.0,0.5,0.5, false, [-25.16, -18.99, -14.20, -6.22, -2.66, -18.52, 1.45, 10.12, -8.22, -14.88]),
    (0.5,0.0,0.5, false, [-25.28, -19.22, -14.71, -8.34, 2.71, 2.73, -0.09, -4.55, -12.59, -18.55]),
    (0.5,1.0,0.5, false, [-28.00, -21.94, -17.43, -11.04, 0.44, -5.85, 0.01, -5.99, -14.26, -20.25]),
    (0.5,0.5,0.0, false, [-20.95, -14.79, -10.05, -2.30, -2.11, -0.67, -2.32, -12.66, -19.99, -25.83]),
    (0.5,0.5,1.0, false, [-33.10, -27.07, -22.61, -16.50, -7.89, 1.29, -2.33, 8.24, -6.75, -13.37]),
    (0.0,0.5,0.5, true, [-25.29, -19.00, -13.88, -2.97, -7.56, -7.88, 7.19, -14.72, -22.62, -28.56]),
    (0.25,0.5,0.5, true, [-22.65, -16.10, -10.09, -9.88, -3.49, 0.80, 11.45, -6.63, -14.69, -20.66]),
    (0.5,0.5,0.5, true, [-26.75, -20.69, -16.19, -9.83, 1.83, 10.22, 4.13, -1.83, -10.32, -16.34]),
    (0.75,0.5,0.5, true, [-27.91, -21.86, -17.36, -11.05, 0.03, -6.39, 5.06, -1.43, -9.91, -15.93]),
    (1.0,0.5,0.5, true, [-26.09, -19.33, -12.72, -7.43, -18.80, -7.82, 9.76, -2.86, -12.69, -18.89]),
    (0.5,0.0,0.5, true, [-24.95, -18.89, -14.39, -8.05, 2.97, 9.81, 3.56, -1.03, -9.35, -15.35]),
    (0.5,1.0,0.5, true, [-27.79, -21.74, -17.23, -10.88, 0.94, 9.25, 4.98, -2.10, -10.67, -16.71]),
    (0.5,0.5,0.0, true, [-20.68, -14.54, -9.81, -2.08, 10.24, 4.22, 1.71, -9.22, -16.61, -22.47]),
    (0.5,0.5,1.0, true, [-32.86, -26.83, -22.37, -16.27, -7.71, 1.76, 10.13, 13.37, -3.04, -9.89]),
        ];
        const N: usize = 16384;
        for (talk, sharp, size, second, db) in KONTAKT {
            let f = Formant::compile([talk, sharp, size], second, 48_000);
            let mut state = ProcessorState::default();
            let mut ir = vec![0f64; N];
            for (block, out) in ir.chunks_mut(BLOCK).enumerate() {
                let mut planar = [[0f64; BLOCK]; 2];
                if block == 0 {
                    planar[0][0] = 1.;
                }
                f.process(&mut state, &mut planar, BLOCK);
                out.copy_from_slice(&planar[0]);
            }
            for (probe, want) in PROBES.iter().zip(db) {
                let (mut re, mut im) = (0f64, 0f64);
                for (n, v) in ir.iter().enumerate() {
                    let phase = std::f64::consts::TAU * probe * n as f64 / 48_000.;
                    re += v * phase.cos();
                    im -= v * phase.sin();
                }
                let got = 20. * (re * re + im * im).sqrt().log10();
                assert!((got - want).abs() < 0.1, "talk {talk} sharp {sharp} size {size} second {second} at {probe} Hz: {got} dB vs {want}");
            }
        }
    }
}
