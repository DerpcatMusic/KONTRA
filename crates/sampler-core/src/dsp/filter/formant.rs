//! Kontakt 8 Filter Reference: Formant I and II (Talk, Sharp, Size).
//! Same-SHA Ghidra FUN_140b05ac0/FUN_140b00750, radare2 tables
//! 0x144fcea10/0x144fcea90/0x144fceb10 and polynomials 0x149e745e0/0x149e74600.
//! The tables hold ten rows. The setter FUN_140b05ac0 stores `0.5 < control[3]` in a flag, and
//! FUN_140b054e0 adds 5.0 to the Talk position when that flag is set, selecting rows 5..=9
//! (Formant II, saved type 91, case 0x5b of FUN_1409619b0). The static flag value for type 91
//! is not yet read as a constant (see docs/re/engine/filt-formant-effect.md).
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
    pub(crate) knobs: [f32; 3],
    second: bool,
    rate: u32,
}

/// Control quantum: the wrapper writes its controls and ticks the ramp every 32 frames.
const QUANTUM: u32 = 32;

/// The twelve lanes of the native ramp object: groups z (band frequency), r (radius) and a
/// (amplitude) of four lanes each. Lane 3 of every group stays zero: the native targets are
/// written for three bands only.
///
/// `FUN_140acbcb0` on the block at `+0x4a0`: `cur` advances by `delta` every frame; a dirty tick
/// (any setter wrote, `+0x530`) sets `delta = (target - cur) / (N * 32)` and arms `countdown = N`
/// (N = round(15 ms / 32 frames)); a clean tick counts down and snaps `cur` to `target` at zero.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Ramp {
    cur: [f32; 12],
    delta: [f32; 12],
    target: [f32; 12],
    countdown: i32,
    dirty: bool,
    ready: bool,
    /// Frames left in the current control chunk (the wrapper's `+0x1bc`).
    frames: u32,
}

/// The twelve lane targets of the four controls: `FUN_140b054e0` (Talk, flag) and the Size and
/// Sharp setters, which all recompute the same targets from the stored controls.
fn targets([talk, sharp, size]: [f32; 3], second: bool, rate: u32) -> [f32; 12] {
    // FUN_140b054e0: row = (5 if second else 0) + 4 Talk, clamped to 9.0.
    let position = (if second { 5. } else { 0. } + talk * 4.).min(9.);
    let row = position as usize;
    let fraction = position - row as f32;
    let interpolate = |table: [[f32; 3]; 10], band: usize| {
        if row >= 9 {
            table[9][band]
        } else {
            table[row][band] + (table[row + 1][band] - table[row][band]) * fraction
        }
    };
    let frequency_scale = (1. / rate as f32) * std::f32::consts::TAU;
    let size_scale = size + size.max(0.5);
    let sharpness = (2. - sharp) * sharp;
    let radius_scale = 0.8999999761581421 + 0.10000002384185791 * sharpness;
    let amplitude_scale = (1. + sharpness * sharpness) * 5.;
    let mut out = [0.; 12];
    for band in 0..3 {
        out[band] = (interpolate(FREQUENCIES, band) * frequency_scale) * size_scale;
        out[4 + band] = interpolate(RADII, band) * radius_scale;
        out[8 + band] = interpolate(AMPLITUDES, band) * amplitude_scale;
    }
    out
}

/// One band's `[b0, a1, a2]` from its frequency `z` and radius `r` (b2 = -b0). The native
/// coefficient block (`FUN_140b00750`, every fourth frame): g = S(z)(1 - r), b0 = g/(1 + g),
/// a1 = -2 C(z)/(1 + g), a2 = (1 - g)/(1 + g), with S and C the native sine and cosine polynomials.
fn coefficients(z: f32, r: f32) -> [f32; 3] {
    let z2 = z * z;
    let sine = (((((-2.39e-8 * z2 + 2.7526e-6) * z2 - 0.000198409) * z2 + 0.00833333) * z2
        - 0.166667)
        * z2
        + 1.)
        * z;
    let cosine =
        ((((-2.605e-7 * z2 + 2.47609e-5) * z2 - 0.00138884) * z2 + 0.0416666) * z2 - 0.499923) * z2
            + 1.;
    let g = sine * (1. - r);
    let inverse = 1. / (1. + g);
    [inverse * g, cosine * -inverse * 2., (1. - g) * inverse]
}

impl Ramp {
    /// `FUN_140acbcb0`: one 32-frame control tick. `step` is 1/(N * 32) and `reload` is N.
    fn tick(&mut self, step: f32, reload: i32) {
        if self.dirty {
            self.dirty = false;
            let mut motion = false;
            for i in 0..12 {
                let delta = (self.target[i] - self.cur[i]) * step;
                if f64::from(delta * delta) < 1e-15 {
                    self.delta[i] = 0.;
                    self.cur[i] = self.target[i];
                } else {
                    self.delta[i] = delta;
                    motion = true;
                }
            }
            if motion {
                self.countdown = reload;
            }
        } else {
            self.countdown -= 1;
            if self.countdown == 0 {
                self.cur = self.target;
                self.delta = [0.; 12];
            }
        }
    }
}

/// One Formant process call. Live lanes (`live`) are written on every 32-frame chunk, as the
/// native wrapper does for enabled routes: the setter sets the dirty flag, then the tick runs. Saved
/// knobs are written once, before the first tick, as the native prepare does. Controls are clamped
/// to [0, 1]; the ramp advances per frame and the coefficients refresh every fourth frame of the
/// call. `controls` gives the knobs at a frame offset of the call; `aux` holds the band histories
/// (w1, w2 per band and channel).
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_ramp(
    ramp: &mut Ramp,
    aux: &mut [f64],
    second: bool,
    rate: u32,
    live: bool,
    block: &mut Planar,
    len: usize,
    controls: impl Fn(usize) -> [f32; 3],
) {
    let reload = ((rate as f32 * 0.015 / QUANTUM as f32 + 0.5) as i32).max(1);
    let step = 1. / (reload * QUANTUM as i32) as f32;
    let mut at = 0;
    while at < len {
        if ramp.frames == 0 {
            ramp.frames = QUANTUM;
            if live || !ramp.ready {
                let knobs = controls(at).map(|v| v.clamp(0., 1.));
                ramp.target = targets(knobs, second, rate);
                if !ramp.ready {
                    // Prepared state: the current values are the targets (native prepare after the
                    // controls are written; the Kontakt measurements show no start-up ramp).
                    ramp.cur = ramp.target;
                    ramp.ready = true;
                }
                ramp.dirty = true;
            }
            ramp.tick(step, reload);
        }
        let n = (ramp.frames as usize).min(len - at);
        let mut coefficients_now = [[0f32; 3]; 3];
        for i in 0..n {
            for lane in 0..12 {
                ramp.cur[lane] += ramp.delta[lane];
            }
            if i % 4 == 0 {
                for band in 0..3 {
                    coefficients_now[band] = coefficients(ramp.cur[band], ramp.cur[4 + band]);
                }
            }
            for (channel, samples) in block.iter_mut().enumerate() {
                let input = samples[at + i] as f32;
                let mut output = 0_f32;
                for band in 0..3 {
                    let [b0, a1, a2] = coefficients_now[band];
                    let index = channel * 6 + band * 2;
                    let w1 = aux[index] as f32;
                    let w2 = aux[index + 1] as f32;
                    let w = input - (w1 * a1 + w2 * a2);
                    output += (b0 * w + -b0 * w2) * ramp.cur[8 + band];
                    aux[index + 1] = f64::from(w1);
                    aux[index] = f64::from(w);
                }
                samples[at + i] = f64::from(output);
            }
        }
        ramp.frames -= n as u32;
        at += n;
    }
}

impl Formant {
    pub(crate) fn compile(knobs: [f32; 3], second: bool, rate: u32) -> Self {
        Self {
            knobs,
            second,
            rate,
        }
    }

    pub(crate) fn process(&self, state: &mut ProcessorState, block: &mut Planar, len: usize) {
        let knobs = self.knobs;
        process_ramp(
            &mut state.formant,
            &mut state.aux,
            self.second,
            self.rate,
            false,
            block,
            len,
            |_| knobs,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::BLOCK;

    #[test]
    fn second_table_starts_five_rows_in() {
        // FUN_140b054e0: the flag adds 5.0 rows to 4 Talk, so Talk 0 of table 2 is Talk 1.25 of table 1.
        assert_eq!(
            targets([0., 0.5, 0.5], true, 48_000),
            targets([1.25, 0.5, 0.5], false, 48_000)
        );
    }

    /// Kontakt 8 magnitude responses (Wine oracle, `tools/kontakt-oracle/eqrender.py`, 48 kHz,
    /// impulse through the LL channel against the bypass reference). Rows: Talk, Sharp, Size,
    /// second table, dB at 30..15000 Hz. The first step of the render (Talk 0 on table 1) reads
    /// back as the Talk 0.5 response (its first write was lost), so it is not used.
    #[test]
    fn formant_responses_match_kontakt_measurements() {
        const PROBES: [f64; 10] = [
            30., 60., 100., 200., 500., 1000., 2000., 5000., 10000., 15000.,
        ];
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
                assert!(
                    (got - want).abs() < 0.1,
                    "talk {talk} sharp {sharp} size {size} second {second} at {probe} Hz: {got} dB vs {want}"
                );
            }
        }
    }
}

#[cfg(test)]
mod kemu_vectors {
    use super::*;

    /// Native Formant I/II through kemu (`tools/re/formant_kemu_vectors.py`): prepare, setters, the
    /// 32-frame tick and the loop, stereo. Each live block writes all four controls before the
    /// tick, as the native wrapper does for enabled routes; the static lanes are constant.
    #[test]
    fn native_kemu_vectors_through_the_live_path() {
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("formant_kemu_vectors.json")).unwrap();
        let num = |v: &serde_json::Value| v.as_f64().unwrap();
        let mut failures = Vec::new();
        for case in &cases {
            let name = case["name"].as_str().unwrap();
            let rate = num(&case["rate"]) as u32;
            let schedule: Vec<[f32; 4]> = case["schedule"]
                .as_array()
                .unwrap()
                .iter()
                .map(|k| {
                    [
                        num(&k[0]) as f32,
                        num(&k[1]) as f32,
                        num(&k[2]) as f32,
                        num(&k[3]) as f32,
                    ]
                })
                .collect();
            let stim: Vec<f64> = case["stim"].as_array().unwrap().iter().map(num).collect();
            let want: Vec<f64> = case["outL"].as_array().unwrap().iter().map(num).collect();
            let peak = want.iter().fold(0f64, |m, v| m.max(v.abs()));
            let mut ramp = Ramp::default();
            let mut aux = [0f64; 16];
            let mut out = Vec::new();
            for (block, knobs) in schedule.iter().enumerate() {
                let mut buffer: Planar = [[0.; crate::dsp::BLOCK]; 2];
                let range = block * 32..(block + 1) * 32;
                buffer[0][..32].copy_from_slice(&stim[range.clone()]);
                buffer[1][..32].copy_from_slice(&stim[range]);
                // The schedule is in setter-index order (Talk, Size, Sharp, flag); `targets` takes (Talk, Sharp, Size).
                let controls = [knobs[0], knobs[2], knobs[1]];
                let live = case["enabled"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v.as_bool() == Some(true));
                process_ramp(
                    &mut ramp,
                    &mut aux,
                    knobs[3] > 0.5,
                    rate,
                    live,
                    &mut buffer,
                    32,
                    |_| controls,
                );
                out.extend_from_slice(&buffer[0][..32]);
            }
            let worst = out
                .iter()
                .zip(&want)
                .map(|(g, w)| (g - w).abs())
                .fold(0f64, f64::max)
                / peak;
            if worst > 1e-5 {
                failures.push(format!("{name} worst {worst:e} of peak"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
