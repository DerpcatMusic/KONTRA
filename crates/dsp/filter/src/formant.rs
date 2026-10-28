//! A vowel formant filter: three resonant band-passes in parallel, their
//! centres, radii and levels read from a vowel table by `talk` and scaled by
//! `size` and `sharp` (docs/re/engine/filt-formant-effect.md). Each band is
//! `g(1 − z⁻²)/((1 + g) − 2cos(ω)z⁻¹ + (1 − g)z⁻²)` with `g = sin(ω)(1 − r)`.
//!
//! ponytail: fixed knobs; a moving talk or size needs the 15 ms target ramp
//! from the same doc.

use crate::Biquad;

#[rustfmt::skip]
const HZ: [[f32; 3]; 10] = [
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
const LEVELS: [[f32; 3]; 10] = [
    [0.44669, 0.044976, 0.029107], [0.21727, 0.27102, 0.10339], [0.5176, 0.4384, 0.3303],
    [0.56235, 0.71781, 0.37498], [0.45, 0.6675, 0.5295],
    [0.153, 0.2353, 0.3523], [0.11831, 0.38681, 0.5389], [0.25792, 0.69024, 0.58885],
    [0.40272, 0.68783, 0.77625], [0.16481, 0.63096, 0.56235],
];

/// Which five vowels of the table `talk` sweeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VowelSet {
    First,
    Second,
}

/// Knobs 0..=1; `set` picks the five vowels `talk` sweeps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vowels {
    pub talk: f32,
    pub sharp: f32,
    pub size: f32,
    pub set: VowelSet,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Formant {
    bands: [Biquad; 3],
}

impl Formant {
    #[must_use]
    pub fn new(v: Vowels, rate: u32) -> Self {
        let [talk, sharp, size] = [v.talk, v.sharp, v.size].map(|x| x.clamp(0.0, 1.0));
        // Five vowels per set, talk sweeping them with linear steps between.
        let at = (match v.set {
            VowelSet::First => 0.0,
            VowelSet::Second => 5.0,
        } + 4.0 * talk)
            .min(9.0);
        let row = at as usize;
        let t = at - row as f32;
        let read = |table: &[[f32; 3]; 10], band: usize| {
            let a = table.get(row).and_then(|r| r.get(band)).copied();
            let b = table.get(row + 1).and_then(|r| r.get(band)).copied();
            match (a, b) {
                (Some(a), Some(b)) => a + (b - a) * t,
                (a, _) => a.unwrap_or(0.0),
            }
        };
        let sharpness = (2.0 - sharp) * sharp;
        let radius = 0.9 + 0.1 * sharpness;
        let level = (1.0 + sharpness * sharpness) * 5.0;
        let scale = std::f32::consts::TAU / rate.max(1) as f32 * (size + size.max(0.5));
        let bands = [0, 1, 2].map(|band| {
            // Kept below Nyquist: past π, `g` turns negative and the poles leave
            // the unit circle (low rates, large size).
            let w = f64::from(read(&HZ, band) * scale).min(0.99 * std::f64::consts::PI);
            let r = f64::from(read(&RADII, band) * radius);
            let g = w.sin() * (1.0 - r);
            let mut b = Biquad::new([g, 0.0, -g, 1.0 + g, -2.0 * w.cos(), 1.0 - g]);
            b.scale(f64::from(read(&LEVELS, band) * level));
            b
        });
        Self { bands }
    }

    /// Filters `io` in place: the sum of the three bands.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        for frame in io {
            let x = *frame;
            *frame = [0.0; 2];
            for band in &mut self.bands {
                let mut y = [x];
                band.process(&mut y);
                frame[0] += y[0][0];
                frame[1] += y[0][1];
            }
        }
    }

    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.bands.iter().all(Biquad::is_silent)
    }
}
