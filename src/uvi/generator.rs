//! Native Program oscillators, with per-note phase state and resident resources.
//!
//! Parameter names/units: https://lua.uvi.net/_elements.html (Analog/Wavetable).
//! Single-voice sine amplitude, polarity, phase and tuning were compared with
//! authored fixtures in official UVI Workstation 4.0.9, without license changes.
//! The discontinuous-wave anti-aliasing below is an original polynomial law,
//! not a reconstruction of UVI's minimum-phase BLEP implementation.

use super::{dsp::Frame, program::ProgramNode, sample::Sample};
use anyhow::{Context, Result, ensure};
use std::{f64::consts::TAU, sync::Arc};

pub const FIDELITY_DIAGNOSTIC: &str = "Analog sine/PWM and fixed-phase unison phase/detune/gain/stereo laws, and filename-based wavetable slice morphing were measured against authored native fixtures; polynomial anti-aliasing, hard-sync edge treatment, noise sequence and linear wavetable readout are original laws, with numerical parity unverified";

pub fn supports(kind: &str) -> bool {
    matches!(kind, "MinBlepGenerator" | "WaveTableOscillator")
}

fn number(node: &ProgramNode, name: &str, default: f64) -> Result<f64> {
    let value = node
        .attributes
        .get(name)
        .map(|s| s.parse::<f64>())
        .transpose()
        .with_context(|| format!("Invalid oscillator {name}"))?
        .unwrap_or(default);
    ensure!(value.is_finite(), "Nonfinite oscillator {name}");
    Ok(value)
}

pub fn validate(node: &ProgramNode) -> Result<()> {
    ensure!(supports(&node.kind), "Unsupported UVI generator");
    let n = |name, default| number(node, name, default);
    let voices = if node.kind == "MinBlepGenerator" {
        "NumOscillators"
    } else {
        "NumOscs"
    };
    let count = n(voices, 1.)?;
    ensure!(
        count.fract() == 0. && (1. ..=8.).contains(&count),
        "Invalid oscillator unison count"
    );
    ensure!(
        count == 1. || (node.kind == "MinBlepGenerator" && n("PhaseSpread", 1.)? == 0.),
        "Only fixed-phase Analog unison is implemented"
    );
    ensure!(
        (0. ..=1.).contains(&n("StartPhase", 0.)?),
        "Invalid oscillator start phase"
    );
    ensure!(
        [0., 1.].contains(&n("Stereo", 0.)?),
        "Invalid generator Stereo flag"
    );
    ensure!(
        [0., 1.].contains(&n("PhaseSpread", 1.)?),
        "Random oscillator phase is not implemented"
    );
    if node.kind == "MinBlepGenerator" {
        ensure!(
            (0. ..=1.).contains(&n("MultiOscSpread", 0.1)?),
            "Invalid Analog detune spread"
        );
        ensure!(
            [0., 1.].contains(&n("DetuneMode", 0.)?),
            "Invalid Analog detune mode"
        );
        ensure!(
            (0. ..=1.).contains(&n("StereoSpread", 0.1)?),
            "Invalid Analog stereo spread"
        );
        ensure!(
            [0., 1.].contains(&n("StereoSpreadMode", 0.)?),
            "Invalid Analog stereo spread mode"
        );
        ensure!(
            count <= 3.
                || n("Stereo", 0.)? == 0.
                || n("StereoSpread", 0.1)? == 0.
                || n("StereoSpreadMode", 0.)? == 0.,
            "Alternating stereo unison phase assignment is not implemented"
        );
        let wave = n("Waveform", 4.)?;
        ensure!(
            wave.fract() == 0. && (1. ..=5.).contains(&wave),
            "Unsupported Analog waveform (pulse-train law is unverified)"
        );
        ensure!(
            (0.01..=0.99).contains(&n("Pwm", 0.5)?),
            "Invalid Analog pulse width"
        );
        ensure!(
            [0., 1.].contains(&n("Polarity", 0.)?),
            "Invalid Analog polarity"
        );
        ensure!(
            [0., 1.].contains(&n("HardSync", 0.)?),
            "Invalid Analog hard sync"
        );
        ensure!(
            (0. ..=36.).contains(&n("HardSyncShift", 0.)?),
            "Invalid Analog hard-sync shift"
        );
    } else {
        ensure!(
            node.attributes
                .get("WavetablePath")
                .is_some_and(|s| !s.is_empty()),
            "A resolved external wavetable is required; factory tables are not bundled"
        );
        let mode = n("PhaseDistortionMode", 3.)?;
        let amount = n("PhaseDistortionAmount", 0.)?;
        ensure!(
            (mode == 3. && amount == 0.) || (mode == 0. && amount == 0.5),
            "Active wavetable phase distortion is not implemented"
        );
        ensure!(n("EnableFM", 0.)? == 0., "Wavetable FM is not implemented");
        ensure!(
            n("WaveIndexSpread", 0.)? == 0.,
            "Wavetable unison index spread is not implemented"
        );
    }
    Ok(())
}

enum Source {
    Analog,
    Table { sample: Arc<Sample>, cycle: usize },
}

pub struct Generator {
    source: Source,
    rate: f64,
    phase: [f64; 8],
    master_phase: [f64; 8],
    oscillators: usize,
    noise: u32,
    stereo: bool,
}

impl Generator {
    pub fn new(node: &ProgramNode, rate: f64, table: Option<Arc<Sample>>) -> Result<Self> {
        validate(node)?;
        ensure!(
            rate.is_finite() && (8_000. ..=192_000.).contains(&rate),
            "Invalid oscillator sample rate"
        );
        let source = if node.kind == "MinBlepGenerator" {
            Source::Analog
        } else {
            let table = table.context("Missing resolved external wavetable")?;
            ensure!(
                table.channels == 1
                    && table.frames >= 4
                    && table.frames <= 1 << 20
                    && table.interleaved.len() == table.frames
                    && table.interleaved.iter().all(|n| n.is_finite()),
                "Only bounded mono external wavetables are implemented"
            );
            let cycle = node
                .attributes
                .get("WavetablePath")
                .and_then(|p| p.rsplit(['/', '\\']).next())
                .and_then(|p| p.rsplit_once('.').map(|(stem, _)| stem))
                .and_then(|p| p.rsplit_once('_').map(|(_, size)| size))
                .and_then(|size| size.parse::<usize>().ok())
                .unwrap_or(table.frames);
            ensure!(
                cycle >= 4 && cycle <= table.frames && table.frames % cycle == 0,
                "Invalid external wavetable cycle geometry"
            );
            // Native external import recognizes a numeric filename suffix.
            // Retained RIFF clm metadata did not change authored native import;
            // bank-resource-specific clm handling remains unverified.
            Source::Table {
                sample: table,
                cycle,
            }
        };
        let oscillators = number(
            node,
            if node.kind == "MinBlepGenerator" {
                "NumOscillators"
            } else {
                "NumOscs"
            },
            1.,
        )? as usize;
        let start = number(node, "StartPhase", 0.)?;
        Ok(Self {
            source,
            rate,
            phase: std::array::from_fn(|i| {
                (start + i as f64 / (2 * oscillators) as f64).rem_euclid(1.)
            }),
            master_phase: [0.; 8],
            oscillators,
            noise: 0x91e1_0da5,
            stereo: number(node, "Stereo", 0.)? != 0.,
        })
    }

    pub fn channels(&self) -> usize {
        if self.stereo { 2 } else { 1 }
    }

    /// Frequency already includes note tracking and tuning; playback applies Gain.
    /// Parameter lookup must use the current voice's effective modulation values.
    pub fn next(
        &mut self,
        numeric: impl Fn(&str, f64) -> Result<f64>,
        frequency_hz: f64,
    ) -> Result<Frame> {
        ensure!(
            frequency_hz.is_finite() && frequency_hz >= 0.,
            "Invalid oscillator frequency"
        );
        let voices = if matches!(self.source, Source::Analog) {
            "NumOscillators"
        } else {
            "NumOscs"
        };
        ensure!(
            numeric(voices, 1.)? == self.oscillators as f64,
            "Generator unison count changed; note retrigger required"
        );
        ensure!(
            numeric("Stereo", 0.)? == f64::from(u8::from(self.stereo)),
            "Generator Stereo changes bus width; rebuild graph required"
        );
        ensure!(
            [0., 1.].contains(&numeric("PhaseSpread", 1.)?),
            "Random oscillator phase is not implemented"
        );
        ensure!(
            self.oscillators == 1 || numeric("PhaseSpread", 1.)? == 0.,
            "Only fixed-phase Analog unison is implemented"
        );
        let spread = numeric("MultiOscSpread", 0.1)?;
        let detune_mode = numeric("DetuneMode", 0.)?;
        let stereo_spread = numeric("StereoSpread", 0.1)?;
        let stereo_mode = numeric("StereoSpreadMode", 0.)?;
        ensure!(
            (0. ..=1.).contains(&spread) && [0., 1.].contains(&detune_mode),
            "Invalid generator detune"
        );
        ensure!(
            (0. ..=1.).contains(&stereo_spread) && [0., 1.].contains(&stereo_mode),
            "Invalid generator stereo spread"
        );
        ensure!(
            self.oscillators <= 3 || !self.stereo || stereo_spread == 0. || stereo_mode == 0.,
            "Alternating stereo unison phase assignment is not implemented"
        );
        let mut frame = [0.; super::dsp::MAX_CHANNELS];
        for oscillator in 0..self.oscillators {
            let position = unison_position(oscillator, self.oscillators);
            let detune = if detune_mode == 0. {
                50. * spread * position
            } else {
                100. * spread.powi(3) * position * (self.oscillators / 2) as f64
            };
            let step = frequency_hz * 2f64.powf(detune / 1200.) / self.rate;
            let value = match &self.source {
                Source::Table {
                    sample: table,
                    cycle,
                } => {
                    let mode = numeric("PhaseDistortionMode", 3.)?;
                    let amount = numeric("PhaseDistortionAmount", 0.)?;
                    ensure!(
                        (mode == 3. && amount == 0.) || (mode == 0. && amount == 0.5),
                        "Active wavetable phase distortion is not implemented"
                    );
                    ensure!(
                        numeric("EnableFM", 0.)? == 0.,
                        "Wavetable FM is not implemented"
                    );
                    ensure!(
                        numeric("WaveIndexSpread", 0.)? == 0.,
                        "Wavetable unison index spread is not implemented"
                    );
                    let index = numeric("WaveIndex", 0.)?;
                    let fade = numeric("FadeWaveIndex", 1.)?;
                    ensure!(
                        (0. ..=1.).contains(&index) && [0., 1.].contains(&fade),
                        "Invalid wavetable wave index"
                    );
                    let index = index * (table.frames / cycle - 1) as f64;
                    let low = if fade == 0. {
                        index.round() as usize
                    } else {
                        index as usize
                    };
                    let high = (low + 1).min(table.frames / cycle - 1);
                    let read = |slice: usize| {
                        let at = self.phase[oscillator] * *cycle as f64;
                        let lo = at as usize % cycle;
                        let hi = (lo + 1) % cycle;
                        let a = table
                            .interleaved
                            .value(slice * cycle + lo)
                            .expect("validated wavetable bounds");
                        let b = table
                            .interleaved
                            .value(slice * cycle + hi)
                            .expect("validated wavetable bounds");
                        f64::from(a) + f64::from(b - a) * at.fract()
                    };
                    let a = read(low);
                    if fade != 0. {
                        a + (read(high) - a) * index.fract()
                    } else {
                        a
                    }
                }
                Source::Analog => {
                    let wave = numeric("Waveform", 4.)?;
                    ensure!(
                        wave.fract() == 0. && (1. ..=5.).contains(&wave),
                        "Unsupported Analog waveform"
                    );
                    let sync = numeric("HardSync", 0.)?;
                    ensure!([0., 1.].contains(&sync), "Invalid Analog hard sync");
                    let shift = numeric("HardSyncShift", 0.)?;
                    ensure!(
                        (0. ..=36.).contains(&shift),
                        "Invalid Analog hard-sync shift"
                    );
                    let ratio = if sync != 0. {
                        2f64.powf(shift / 12.)
                    } else {
                        1.
                    };
                    let increment = step * ratio;
                    let edge_step = increment.min(0.5);
                    let pwm = numeric("Pwm", 0.5)?;
                    ensure!((0.01..=0.99).contains(&pwm), "Invalid Analog pulse width");
                    let polarity = numeric("Polarity", 0.)?;
                    ensure!([0., 1.].contains(&polarity), "Invalid Analog polarity");
                    let value = match wave as u8 {
                        1 => {
                            let p = self.phase[oscillator];
                            (if p < 1. - pwm {
                                p / (2. * (1. - pwm))
                            } else {
                                (p - 1.) / (2. * pwm)
                            }) - 0.5 * blep((p + pwm).rem_euclid(1.), edge_step)
                        }
                        2 => {
                            let p = self.phase[oscillator];
                            (if p < pwm { 0.5 } else { -0.5 }) + 0.5 * blep(p, edge_step)
                                - 0.5 * blep((p - pwm).rem_euclid(1.), edge_step)
                        }
                        3 => {
                            let p = self.phase[oscillator];
                            if p < pwm * 0.5 {
                                p / pwm
                            } else if p < 1. - pwm * 0.5 {
                                0.5 - (p - pwm * 0.5) / (1. - pwm)
                            } else {
                                (p - 1.) / pwm
                            }
                        }
                        4 => -0.5 * (TAU * sine_phase(self.phase[oscillator], pwm)).sin(),
                        _ => {
                            self.noise ^= self.noise << 13;
                            self.noise ^= self.noise >> 17;
                            self.noise ^= self.noise << 5;
                            f64::from(self.noise) / f64::from(u32::MAX) - 0.5
                        }
                    };
                    self.phase[oscillator] = (self.phase[oscillator] + increment).rem_euclid(1.);
                    self.master_phase[oscillator] += step;
                    if self.master_phase[oscillator] >= 1. {
                        self.master_phase[oscillator] =
                            self.master_phase[oscillator].rem_euclid(1.);
                        if sync != 0. {
                            // ponytail: direct phase reset; add a measured sync-edge
                            // BLEP when native hard-sync residual comparisons exist.
                            self.phase[oscillator] =
                                (self.master_phase[oscillator] * ratio).rem_euclid(1.);
                        }
                    }
                    if polarity != 0. { -value } else { value }
                }
            };
            if matches!(self.source, Source::Table { .. }) {
                self.phase[oscillator] = (self.phase[oscillator] + step).rem_euclid(1.);
            }
            let value = value / (self.oscillators as f64).sqrt();
            if self.stereo {
                let angle = (position * stereo_spread + 1.) * TAU / 8.;
                frame[0] += (value * angle.cos().powi(2)) as f32;
                frame[1] += (value * angle.sin().powi(2)) as f32;
            } else {
                frame[0] += value as f32;
            }
        }
        Ok(frame)
    }
}

fn blep(phase: f64, step: f64) -> f64 {
    if step == 0. {
        return 0.;
    }
    if phase < step {
        let t = phase / step;
        2. * t - t * t - 1.
    } else if phase > 1. - step {
        let t = (phase - 1.) / step;
        t * t + 2. * t + 1.
    } else {
        0.
    }
}

/// Odd counts start with the center oscillator, then negative/positive pairs;
/// even counts contain pairs only. Pair magnitudes increase toward both edges.
fn unison_position(index: usize, count: usize) -> f64 {
    if count == 1 || (count % 2 != 0 && index == 0) {
        return 0.;
    }
    let pair = if count % 2 == 0 {
        index / 2 + 1
    } else {
        index.div_ceil(2)
    };
    let negative = if count % 2 == 0 {
        index % 2 == 0
    } else {
        index % 2 != 0
    };
    pair as f64 / (count / 2) as f64 * if negative { -1. } else { 1. }
}

/// Native PWM keeps sine zero crossings fixed while moving its two peaks.
/// The same three segments give the Analog triangle's asymmetric slopes.
fn sine_phase(phase: f64, pwm: f64) -> f64 {
    if phase < pwm * 0.5 {
        phase / (2. * pwm)
    } else if phase < 1. - pwm * 0.5 {
        0.25 + (phase - pwm * 0.5) / (2. * (1. - pwm))
    } else {
        0.75 + (phase - 1. + pwm * 0.5) / (2. * pwm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uvi::program::parse_program;

    #[test]
    fn authored_native_external_table_slices() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><WaveTableOscillator WavetablePath="authored_128.wav" PhaseDistortionMode="3" PhaseDistortionAmount="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let node = p
            .nodes
            .iter()
            .find(|n| n.kind == "WaveTableOscillator")
            .unwrap();
        let table = Arc::new(Sample {
            rate: 44_100,
            channels: 1,
            frames: 256,
            interleaved: super::super::storage::Storage::from_f32(
                [vec![0.125; 128], vec![0.375; 128]].concat(),
            )
            .unwrap(),
            loops: Vec::new(),
            unity_note: None,
            riff_metadata: Vec::new(),
            wavetable_cycle_frames: None,
        });
        let mut g = Generator::new(node, 48_000., Some(table)).unwrap();
        // Native independently authored two-cycle constant WAVs establish both
        // the linear slice morph and nonsmoothed nearest-slice boundary at .5.
        for (index, fade, native) in [
            (0., 1., 0.0625),
            (0.5, 1., 0.125),
            (1., 1., 0.1875),
            (0.49, 0., 0.0625),
            (0.5, 0., 0.1875),
        ] {
            let params = |name: &str, default| match name {
                "WaveIndex" => Ok(index),
                "FadeWaveIndex" => Ok(fade),
                _ => number(node, name, default),
            };
            assert_eq!(g.next(params, 261.6255653005986).unwrap()[0] * 0.5, native);
        }
        assert!(
            g.next(
                |name, default| if name == "WaveIndex" {
                    Ok(1.1)
                } else {
                    number(node, name, default)
                },
                440.
            )
            .is_err()
        );
    }

    #[test]
    fn authored_native_fixed_phase_unison() {
        for (count, native) in [
            (2, [-0.17677669, -0.18272589, -0.18846081]),
            (3, [-0.24999997, -0.25479552, -0.25929227]),
            (4, [-0.30177668, -0.30587977, -0.30962408]),
            (8, [-0.44435823, -0.44712409, -0.44936562]),
        ] {
            let xml = format!(
                r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><MinBlepGenerator Waveform="4" NumOscillators="{count}" MultiOscSpread="0" PhaseSpread="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#
            );
            let p = parse_program(&xml).unwrap();
            let node = p
                .nodes
                .iter()
                .find(|n| n.kind == "MinBlepGenerator")
                .unwrap();
            let mut g = Generator::new(node, 48_000., None).unwrap();
            for reference in native {
                let actual = g
                    .next(
                        |name, default| number(node, name, default),
                        261.6255653005986,
                    )
                    .unwrap()[0]
                    * 0.5;
                assert!((actual - reference).abs() < 0.000002);
            }
        }
        for (count, spread, native) in [
            (2, 0.5, [-0.05177670, -0.30177670]),
            (3, 1., [-0.25000003, -0.24999993]),
            (4, 1., [-0.27588835, -0.32766503]),
        ] {
            let xml = format!(
                r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><MinBlepGenerator Waveform="4" NumOscillators="{count}" MultiOscSpread="1" DetuneMode="0" PhaseSpread="0" Stereo="1" StereoSpread="{spread}" StereoSpreadMode="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#
            );
            let p = parse_program(&xml).unwrap();
            let node = p
                .nodes
                .iter()
                .find(|n| n.kind == "MinBlepGenerator")
                .unwrap();
            let mut g = Generator::new(node, 48_000., None).unwrap();
            let actual = g
                .next(
                    |name, default| number(node, name, default),
                    261.6255653005986,
                )
                .unwrap();
            assert_eq!(g.channels(), 2);
            for channel in 0..2 {
                assert!((actual[channel] - native[channel]).abs() < 0.000002);
            }
        }
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><MinBlepGenerator Waveform="4" NumOscillators="2" MultiOscSpread="0.5" DetuneMode="1" PhaseSpread="0" Stereo="1" StereoSpread="1"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let node = p
            .nodes
            .iter()
            .find(|n| n.kind == "MinBlepGenerator")
            .unwrap();
        let mut g = Generator::new(node, 48_000., None).unwrap();
        let params = |name: &str, default| number(node, name, default);
        g.next(params, 261.6255653005986).unwrap();
        assert!((g.next(params, 261.6255653005986).unwrap()[0] + 0.012018654).abs() < 0.000002);
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><MinBlepGenerator Waveform="4" NumOscillators="4" MultiOscSpread="1" DetuneMode="1" PhaseSpread="0" Stereo="1" StereoSpread="1"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let node = p
            .nodes
            .iter()
            .find(|n| n.kind == "MinBlepGenerator")
            .unwrap();
        let mut g = Generator::new(node, 48_000., None).unwrap();
        let params = |name: &str, default| number(node, name, default);
        g.next(params, 261.6255653005986).unwrap();
        let actual = g.next(params, 261.6255653005986).unwrap();
        for (actual, native) in actual[..2].iter().zip([-0.28359058, -0.32729816]) {
            assert!((actual - native).abs() < 0.000002);
        }
    }

    #[test]
    fn authored_native_sine_observations_and_phase_lifetime() {
        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><MinBlepGenerator Waveform="4" PhaseSpread="0" StartPhase="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let node = p
            .nodes
            .iter()
            .find(|n| n.kind == "MinBlepGenerator")
            .unwrap();
        let mut g = Generator::new(node, 48_000., None).unwrap();
        // Original authored Workstation 4.0.9 note60/Gain1 capture. Native mono
        // KG center contributes .5; omit its two leading callback latency frames.
        let native = [
            -0.000000010927847,
            -0.008560038,
            -0.017110027,
            -0.025639949,
            -0.034139805,
        ];
        let params = |name: &str, default| number(node, name, default);
        for reference in native {
            let actual = g.next(params, 261.6255653005986).unwrap()[0] * 0.5;
            assert!((actual - reference).abs() < 0.000002);
        }
        let before = g.phase[0];
        g.next(params, 523.2511306011972).unwrap();
        assert!((g.phase[0] - before - 523.2511306011972 / 48_000.).abs() < 1e-12);
        let mut fresh = Generator::new(node, 48_000., None).unwrap();
        assert!(fresh.next(params, 261.6255653005986).unwrap()[0].abs() < 1e-7);
        assert_eq!(g.channels(), 1);
        assert!(Generator::new(node, f64::NAN, None).is_err());
        assert!(g.next(params, f64::INFINITY).is_err());

        for (wave, native) in [(1., 0.0027252734), (3., 0.0054505467)] {
            let mut g = Generator::new(node, 48_000., None).unwrap();
            let params = |name: &str, default| {
                if name == "Waveform" {
                    Ok(wave)
                } else {
                    number(node, name, default)
                }
            };
            g.next(params, 261.6255653005986).unwrap();
            assert!(
                (g.next(params, 261.6255653005986).unwrap()[0] * 0.5 - native).abs() < 0.000002
            );
        }
        for (wave, native) in [(1., 0.0018168489), (3., 0.010901064), (4., -0.017109968)] {
            let mut g = Generator::new(node, 48_000., None).unwrap();
            let params = |name: &str, default| match name {
                "Waveform" => Ok(wave),
                "Pwm" => Ok(0.25),
                _ => number(node, name, default),
            };
            g.next(params, 261.6255653005986).unwrap();
            assert!(
                (g.next(params, 261.6255653005986).unwrap()[0] * 0.5 - native).abs() < 0.000002
            );
        }
        for (wave, expected) in [
            (3., [0.23159003, 0.12257910, 0.01356816, -0.09544277]),
            (4., [-0.24832933, -0.17406739, -0.02128702, 0.14109556]),
        ] {
            let mut g = Generator::new(node, 48_000., None).unwrap();
            let params = |name: &str, default| match name {
                "Waveform" => Ok(wave),
                "Pwm" => Ok(0.25),
                _ => number(node, name, default),
            };
            for i in 0..=118 {
                let actual = g.next(params, 261.6255653005986).unwrap()[0] * 0.5;
                if let Some(at) = [28, 58, 88, 118].iter().position(|&n| n == i) {
                    assert!((actual - expected[at]).abs() < 0.000005);
                }
            }
        }
        assert!(
            g.next(
                |name, default| if name == "NumOscillators" {
                    Ok(2.)
                } else {
                    Ok(default)
                },
                440.
            )
            .is_err()
        );

        let p = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><WaveTableOscillator WavetablePath="authored_128.wav" PhaseDistortionMode="3" PhaseDistortionAmount="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let node = p
            .nodes
            .iter()
            .find(|n| n.kind == "WaveTableOscillator")
            .unwrap();
        let table = Arc::new(Sample {
            rate: 44_100,
            channels: 1,
            frames: 128,
            interleaved: super::super::storage::Storage::from_f32(
                (0..128)
                    .map(|i| (16384. * (TAU * i as f64 / 128.).sin()).round() as f32 / 32768.)
                    .collect(),
            )
            .unwrap(),
            loops: Vec::new(),
            unity_note: None,
            riff_metadata: Vec::new(),
            wavetable_cycle_frames: None,
        });
        let mut g = Generator::new(node, 48_000., Some(table)).unwrap();
        // Native authored PCM16 external sine table at neutral phase mode3.
        // A bounded tolerance exposes the stated linear-vs-native readout gap.
        for native in [0., 0.008561726, 0.017110242, 0.025641577, 0.034140345] {
            let actual = g
                .next(
                    |name, default| number(node, name, default),
                    261.6255653005986,
                )
                .unwrap()[0]
                * 0.5;
            assert!((actual - native).abs() < 0.00004);
        }
        for _ in 0..48_000 {
            assert!(
                g.next(
                    |name, default| number(node, name, default),
                    261.6255653005986
                )
                .unwrap()[0]
                    .is_finite()
            );
        }
        assert!(Generator::new(node, 48_000., None).is_err());
        let constant = Arc::new(Sample {
            rate: 44_100,
            channels: 1,
            frames: 128,
            interleaved: super::super::storage::Storage::from_f32(vec![0.125; 128]).unwrap(),
            loops: Vec::new(),
            unity_note: None,
            riff_metadata: Vec::new(),
            wavetable_cycle_frames: None,
        });
        let mut g = Generator::new(node, 48_000., Some(constant)).unwrap();
        // A non-sine authored table distinguishes native resource loading from
        // the oscillator's default sine fallback: native L/R are exactly .0625.
        for _ in 0..200 {
            assert_eq!(
                g.next(
                    |name, default| number(node, name, default),
                    261.6255653005986
                )
                .unwrap()[0]
                    * 0.5,
                0.0625
            );
        }
    }
}
