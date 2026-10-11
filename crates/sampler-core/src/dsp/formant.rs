//! Kontakt 8 Filter Reference: Formant I (Talk, Sharp, Size).
//! Same-SHA Ghidra FUN_140b05ac0/FUN_140b00750, radare2 tables
//! 0x144fcea10/0x144fcea90/0x144fceb10 and polynomials 0x149e745e0/0x149e74600.
use super::{Planar, ProcessorState};

#[rustfmt::skip]
const FREQUENCIES: [[f32; 3]; 5] = [
    [233., 326., 1914.], [317., 451., 2270.], [726., 1190., 2383.],
    [429., 1920., 2987.], [370., 2578., 4731.],
];
#[rustfmt::skip]
const RADII: [[f32; 3]; 5] = [
    [0.9039, 0.9233, 0.9785], [0.9039, 0.9233, 0.9339], [0.9436, 0.9613, 0.9132],
    [0.9573, 0.9573, 0.9556], [0.9436, 0.9706, 0.9666],
];
#[rustfmt::skip]
const AMPLITUDES: [[f32; 3]; 5] = [
    [0.44669, 0.044976, 0.029107], [0.21727, 0.27102, 0.10339], [0.5176, 0.4384, 0.3303],
    [0.56235, 0.71781, 0.37498], [0.45, 0.6675, 0.5295],
];

pub(crate) struct Formant {
    bands: [[f32; 4]; 3], // b0, a1, a2, amplitude (b2 = -b0)
    pub(crate) knobs: [f32; 3],
}

impl Formant {
    // shortcut: saved controls only; add native 15ms ramps when live Formant controls are bound.
    pub(super) fn compile([talk, sharp, size]: [f32; 3], rate: u32) -> Self {
        let position = talk * 4.;
        let row = (position as usize).min(3);
        let fraction = position - row as f32;
        let interpolate = |table: [[f32; 3]; 5], band| {
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

    pub(super) fn process(&self, state: &mut ProcessorState, block: &mut Planar, len: usize) {
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
