//! Bounded UVI multichannel primitives, independent of containers and Lua.
//!
//! Parameter names, ranges and units: https://lua.uvi.net/_elements.html
//! Matrix orientation and OnePole's default lowpass mode: UVI Falcon manual,
//! https://uvi.s3.us-east-1.amazonaws.com/UVIFC/falcon_manual_PRINT.pdf
//! These are original mathematical implementations, not vendor DSP code.
//! OnePole's exponential RC law and float32 update were measured against
//! UVI Workstation 4.0.9 Windows x64 on 2026-10-03, at 48 kHz with original
//! synthetic impulses (100, 1000 and 10000 Hz low/highpass; 256-frame blocks).
//! TrackDelay's ceil-to-frame conversion, zero delay, and beat ratios at
//! 120 BPM were also measured. Bypass-state transitions are not native-verified;
//! callers must retain [`FIDELITY_DIAGNOSTIC`].

use anyhow::{Result, bail, ensure};

pub const MAX_CHANNELS: usize = 12;
pub type Frame = [f32; MAX_CHANNELS];
pub const FIDELITY_DIAGNOSTIC: &str = "UVI DSP bypass-state transitions are not native-verified; nonzero OnePole key tracking is unsupported";
const MAX_DELAY_SECONDS: f64 = 5.;
const MAX_DELAY_BYTES: usize = 64 << 20;

fn channels(value: usize) -> Result<usize> {
    ensure!(
        (1..=MAX_CHANNELS).contains(&value),
        "UVI DSP needs 1..12 channels"
    );
    Ok(value)
}

fn rate(value: f64) -> Result<f64> {
    ensure!(
        value.is_finite() && (8_000. ..=192_000.).contains(&value),
        "Invalid UVI DSP sample rate"
    );
    Ok(value)
}

fn scalar(value: f64, low: f64, high: f64) -> Result<f64> {
    ensure!(
        value.is_finite() && (low..=high).contains(&value),
        "UVI DSP parameter outside {low}..{high}"
    );
    Ok(value)
}

fn boolean(value: f64) -> Result<bool> {
    ensure!(value == 0. || value == 1., "UVI DSP boolean must be 0 or 1");
    Ok(value == 1.)
}

/// Gain_i_j routes input i to output j; indices in serialized names are 1-based.
pub struct GainMatrix {
    input_channels: usize,
    output_channels: usize,
    coefficients: [[f32; MAX_CHANNELS]; MAX_CHANNELS],
    bypass: bool,
}

impl GainMatrix {
    pub fn new(input_channels: usize, output_channels: usize) -> Result<Self> {
        let mut coefficients = [[0.; MAX_CHANNELS]; MAX_CHANNELS];
        for (i, row) in coefficients.iter_mut().enumerate() {
            row[i] = 1.;
        }
        Ok(Self {
            input_channels: channels(input_channels)?,
            output_channels: channels(output_channels)?,
            coefficients,
            bypass: false,
        })
    }

    pub fn channel_counts(&self) -> (usize, usize) {
        (self.input_channels, self.output_channels)
    }

    fn index(name: &str) -> Result<(usize, usize)> {
        let (input, output) = name
            .strip_prefix("Gain_")
            .and_then(|s| s.split_once('_'))
            .ok_or_else(|| anyhow::anyhow!("Unsupported GainMatrix parameter {name}"))?;
        let (input, output) = (input.parse::<usize>()?, output.parse::<usize>()?);
        channels(input)?;
        channels(output)?;
        Ok((input - 1, output - 1))
    }

    pub fn parameter(&self, name: &str) -> Result<f64> {
        if name == "Bypass" {
            return Ok(u8::from(self.bypass) as f64);
        }
        let (input, output) = Self::index(name)?;
        Ok(self.coefficients[input][output] as f64)
    }

    pub fn set_parameter(&mut self, name: &str, value: f64) -> Result<()> {
        if name == "Bypass" {
            self.bypass = boolean(value)?;
        } else {
            let (input, output) = Self::index(name)?;
            self.coefficients[input][output] = scalar(value, -1., 1.)? as f32;
        }
        Ok(())
    }

    pub fn process(&self, input: &[Frame], output: &mut [Frame]) -> Result<()> {
        ensure!(
            input.len() == output.len(),
            "UVI matrix buffer lengths differ"
        );
        for (source, destination) in input.iter().zip(output) {
            if self.bypass {
                *destination = *source;
                continue;
            }
            *destination = [0.; MAX_CHANNELS];
            for (i, sample) in source[..self.input_channels].iter().enumerate() {
                for (j, out) in destination[..self.output_channels].iter_mut().enumerate() {
                    *out += sample * self.coefficients[i][j];
                }
            }
        }
        Ok(())
    }
}

pub struct Gain {
    channels: usize,
    volume: f64,
    bypass: bool,
}

impl Gain {
    pub fn new(channel_count: usize) -> Result<Self> {
        Ok(Self {
            channels: channels(channel_count)?,
            volume: 1.,
            bypass: false,
        })
    }

    pub fn parameter(&self, name: &str) -> Result<f64> {
        match name {
            "Volume" => Ok(self.volume),
            "Bypass" => Ok(u8::from(self.bypass) as f64),
            _ => bail!("Unsupported Gain parameter {name}"),
        }
    }

    pub fn set_parameter(&mut self, name: &str, value: f64) -> Result<()> {
        match name {
            "Volume" => self.volume = scalar(value, 0., f64::from(10f32.powf(12. / 20.)))?,
            "Bypass" => self.bypass = boolean(value)?,
            _ => bail!("Unsupported Gain parameter {name}"),
        }
        Ok(())
    }

    /// Control modulation can exceed the stored +12 dB parameter range.
    pub fn set_effective_volume(&mut self, value: f64) -> Result<()> {
        self.volume = scalar(value, 0., f64::from(f32::MAX))?;
        Ok(())
    }

    pub fn process(&self, frames: &mut [Frame]) {
        if self.bypass {
            return;
        }
        for frame in frames {
            for sample in &mut frame[..self.channels] {
                *sample *= self.volume as f32;
            }
        }
    }
}

/// Native exponential RC low/highpass; state belongs to one graph instance.
pub struct OnePole {
    channels: usize,
    rate: f64,
    frequency: f64,
    coefficient: Option<(u64, f32)>,
    highpass: bool,
    bypass: bool,
    lowpass: [f32; MAX_CHANNELS],
}

impl OnePole {
    pub fn new(channel_count: usize, sample_rate: f64) -> Result<Self> {
        Ok(Self {
            channels: channels(channel_count)?,
            rate: rate(sample_rate)?,
            frequency: 1000.,
            coefficient: None,
            highpass: false,
            bypass: false,
            lowpass: [0.; MAX_CHANNELS],
        })
    }

    pub fn parameter(&self, name: &str) -> Result<f64> {
        match name {
            "Freq" => Ok(self.frequency),
            "Mode" => Ok(u8::from(self.highpass) as f64),
            "KeyTracking" => Ok(0.),
            "Bypass" => Ok(u8::from(self.bypass) as f64),
            _ => bail!("Unsupported OnePole parameter {name}"),
        }
    }

    pub fn set_parameter(&mut self, name: &str, value: f64) -> Result<()> {
        match name {
            "Freq" => self.frequency = scalar(value, 20., 20_000.)?,
            "Mode" => self.highpass = boolean(value)?,
            "KeyTracking" => ensure!(
                value == 0.,
                "OnePole key tracking requires per-note filter state"
            ),
            "Bypass" => self.bypass = boolean(value)?,
            _ => bail!("Unsupported OnePole parameter {name}"),
        }
        Ok(())
    }

    pub fn clear(&mut self) {
        self.lowpass.fill(0.);
    }

    pub fn process(&mut self, frames: &mut [Frame]) -> Result<()> {
        if self.bypass {
            return Ok(());
        }
        // Rate is immutable per instance. Repeated control writes may retain
        // the exact frequency; prepare its coefficient at the original call site.
        let frequency = self.frequency.to_bits();
        let a = if let Some((prepared, a)) = self.coefficient
            && prepared == frequency
        {
            a
        } else {
            let a = 1. - (-std::f32::consts::TAU * self.frequency as f32 / self.rate as f32).exp();
            self.coefficient = Some((frequency, a));
            a
        };
        for frame in frames {
            for (channel, sample) in frame[..self.channels].iter_mut().enumerate() {
                let x = *sample;
                // This operation order also matches native float32 rounding.
                self.lowpass[channel] += a * (x - self.lowpass[channel]);
                *sample = if self.highpass {
                    x - self.lowpass[channel]
                } else {
                    self.lowpass[channel]
                };
            }
        }
        Ok(())
    }
}

/// DelayTime retains seconds, or quarter-note beat ratio with SyncToHost enabled.
pub struct TrackDelay {
    channels: usize,
    rate: f64,
    delay_time: f64,
    sync: bool,
    tempo: f64,
    bypass: bool,
    buffer: Vec<f32>,
    position: usize,
}

impl TrackDelay {
    pub fn new(channel_count: usize, sample_rate: f64) -> Result<Self> {
        let (channels, rate) = (channels(channel_count)?, rate(sample_rate)?);
        let length = (MAX_DELAY_SECONDS * rate).ceil() as usize + 2;
        ensure!(
            length * channels * std::mem::size_of::<f32>() <= MAX_DELAY_BYTES,
            "UVI delay exceeds memory limit"
        );
        Ok(Self {
            channels,
            rate,
            delay_time: 0.,
            sync: false,
            tempo: 120.,
            bypass: false,
            buffer: vec![0.; length * channels],
            position: 0,
        })
    }

    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.buffer.capacity() * std::mem::size_of::<f32>()
    }

    fn delay_frames(&self, value: f64, sync: bool, tempo: f64) -> Result<usize> {
        // Native DelayTime is a float32, converted to whole frames by ceiling.
        let value = f64::from(value as f32);
        let seconds = if sync { value * 60. / tempo } else { value };
        ensure!(
            seconds <= MAX_DELAY_SECONDS,
            "Synced UVI delay exceeds five-second DSP capacity"
        );
        Ok((seconds * self.rate).ceil() as usize)
    }

    pub fn parameter(&self, name: &str) -> Result<f64> {
        match name {
            "DelayTime" => Ok(self.delay_time),
            "SyncToHost" => Ok(u8::from(self.sync) as f64),
            "Bypass" => Ok(u8::from(self.bypass) as f64),
            _ => bail!("Unsupported TrackDelay parameter {name}"),
        }
    }

    pub fn set_parameter(&mut self, name: &str, value: f64) -> Result<()> {
        match name {
            "DelayTime" => {
                let value = f64::from(scalar(value, 0., 5.)? as f32);
                self.delay_frames(value, self.sync, self.tempo)?;
                self.delay_time = value;
            }
            "SyncToHost" => {
                let sync = boolean(value)?;
                self.delay_frames(self.delay_time, sync, self.tempo)?;
                self.sync = sync;
            }
            "Bypass" => self.bypass = boolean(value)?,
            _ => bail!("Unsupported TrackDelay parameter {name}"),
        }
        Ok(())
    }

    pub fn set_tempo(&mut self, beats_per_minute: f64) -> Result<()> {
        let tempo = scalar(beats_per_minute, 1., 1000.)?;
        self.delay_frames(self.delay_time, self.sync, tempo)?;
        self.tempo = tempo;
        Ok(())
    }

    pub fn clear(&mut self) {
        self.buffer.fill(0.);
        self.position = 0;
    }

    pub fn process(&mut self, frames: &mut [Frame]) -> Result<()> {
        if self.bypass {
            return Ok(());
        }
        let delay = self.delay_frames(self.delay_time, self.sync, self.tempo)?;
        let length = self.buffer.len() / self.channels;
        for frame in frames {
            let delayed = (self.position + length - delay) % length;
            for (channel, sample) in frame[..self.channels].iter_mut().enumerate() {
                self.buffer[self.position * self.channels + channel] = *sample;
                *sample = self.buffer[delayed * self.channels + channel];
            }
            self.position = (self.position + 1) % length;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Original per-call coefficient evaluation, independent of the cache.
    fn onepole_reference_process(pole: &mut OnePole, frames: &mut [Frame]) {
        if pole.bypass { return; }
        let a = 1. - (-std::f32::consts::TAU * pole.frequency as f32 / pole.rate as f32).exp();
        for frame in frames {
            for (channel, sample) in frame[..pole.channels].iter_mut().enumerate() {
                let x = *sample;
                pole.lowpass[channel] += a * (x - pole.lowpass[channel]);
                *sample = if pole.highpass { x - pole.lowpass[channel] } else { pole.lowpass[channel] };
            }
        }
    }
    fn onepole_assert_bits(actual: &[Frame], expected: &[Frame]) {
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(actual.map(f32::to_bits), expected.map(f32::to_bits));
        }
    }
    #[test]
    fn onepole_cached_coefficient_matches_original_full_and_fragmented_bits() {
        for rate in [8000., 44100., 48000., 96000., 192000.] {
            for channels in [1, 2, 6, 12] {
                let mut full = OnePole::new(channels, rate).unwrap();
                let mut split = OnePole::new(channels, rate).unwrap();
                let mut reference = OnePole::new(channels, rate).unwrap();
                for (step, frequency) in [20., 1000., 1000., 20000., 1234.56789, 1234.56790, 1000.]
                    .into_iter().enumerate()
                {
                    for pole in [&mut full, &mut split, &mut reference] {
                        pole.set_parameter("Freq", frequency).unwrap();
                        pole.set_parameter("Mode", (step % 2) as f64).unwrap();
                        if step == 4 { pole.clear(); }
                    }
                    let source: [Frame; 33] = std::array::from_fn(|frame| {
                        std::array::from_fn(|channel| match (frame + channel) % 6 {
                            0 => 1., 1 => -0.5, 2 => 0., 3 => -0.,
                            4 => f32::from_bits(1), _ => -f32::from_bits(1),
                        })
                    });
                    let mut a = source;
                    let mut b = source;
                    let mut expected = source;
                    full.process(&mut a).unwrap();
                    for chunk in b.chunks_mut(if step % 2 == 0 { 1 } else { 7 }) {
                        split.process(chunk).unwrap();
                    }
                    for chunk in expected.chunks_mut(1) {
                        onepole_reference_process(&mut reference, chunk);
                    }
                    onepole_assert_bits(&a, &expected);
                    onepole_assert_bits(&b, &expected);
                    assert_eq!(full.lowpass.map(f32::to_bits), reference.lowpass.map(f32::to_bits));
                    assert_eq!(split.lowpass.map(f32::to_bits), reference.lowpass.map(f32::to_bits));
                    assert_eq!(full.coefficient.unwrap().0, frequency.to_bits());
                }
                for pole in [&mut full, &mut split, &mut reference] {
                    pole.clear();
                    pole.set_parameter("Mode", 0.).unwrap();
                    pole.set_parameter("Freq", 20.).unwrap();
                }
                let source: [Frame; 33] = std::array::from_fn(|_| {
                    std::array::from_fn(|channel| if channel % 2 == 0 {
                        f32::from_bits(0x0040_0000)
                    } else { -f32::from_bits(0x0040_0000) })
                });
                let mut a = source;
                let mut b = source;
                let mut expected = source;
                full.process(&mut a).unwrap();
                for frame in &mut b { split.process(std::slice::from_mut(frame)).unwrap(); }
                for frame in &mut expected { onepole_reference_process(&mut reference, std::slice::from_mut(frame)); }
                onepole_assert_bits(&a, &expected);
                onepole_assert_bits(&b, &expected);
                assert_eq!(full.lowpass.map(f32::to_bits), reference.lowpass.map(f32::to_bits));
                assert_eq!(split.lowpass.map(f32::to_bits), reference.lowpass.map(f32::to_bits));
            }
        }
    }
    #[test]
    fn onepole_coefficient_keeps_bypass_clear_empty_and_rejected_write_behavior() {
        let mut actual = OnePole::new(6, 48000.).unwrap();
        let mut reference = OnePole::new(6, 48000.).unwrap();
        actual.set_parameter("Bypass", 1.).unwrap();
        reference.set_parameter("Bypass", 1.).unwrap();
        actual.process(&mut []).unwrap();
        assert!(actual.coefficient.is_none());
        for pole in [&mut actual, &mut reference] {
            pole.set_parameter("Freq", 20.).unwrap();
        }
        let mut a = [[-0.; MAX_CHANNELS]];
        let mut b = a;
        actual.process(&mut a).unwrap();
        onepole_reference_process(&mut reference, &mut b);
        onepole_assert_bits(&a, &b);
        assert!(actual.coefficient.is_none());
        for pole in [&mut actual, &mut reference] { pole.set_parameter("Bypass", 0.).unwrap(); }
        actual.process(&mut []).unwrap();
        let prepared = actual.coefficient.unwrap();
        assert_eq!(prepared.0, 20f64.to_bits());
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0., 0., 19.999, 20000.001] {
            let a = actual.set_parameter("Freq", value).unwrap_err();
            let b = reference.set_parameter("Freq", value).unwrap_err();
            assert_eq!(a.to_string(), b.to_string());
            assert_eq!(actual.parameter("Freq").unwrap().to_bits(), 20f64.to_bits());
            assert_eq!(actual.coefficient.unwrap().0, prepared.0);
            assert_eq!(actual.coefficient.unwrap().1.to_bits(), prepared.1.to_bits());
        }
        for (step, (frequency, bypass, clear)) in [
            (20., 0., false), (20., 0., false), (20000., 1., false),
            (1000., 1., true), (1000., 0., false), (1000., 0., true),
        ].into_iter().enumerate() {
            for pole in [&mut actual, &mut reference] {
                pole.set_parameter("Freq", frequency).unwrap();
                pole.set_parameter("Bypass", bypass).unwrap();
                pole.set_parameter("Mode", (step % 2) as f64).unwrap();
                if clear { pole.clear(); }
            }
            let before = actual.coefficient;
            let mut a = [[if step == 0 { 1. } else { -0. }; MAX_CHANNELS]];
            let mut b = a;
            actual.process(&mut a).unwrap();
            onepole_reference_process(&mut reference, &mut b);
            onepole_assert_bits(&a, &b);
            assert_eq!(actual.lowpass.map(f32::to_bits), reference.lowpass.map(f32::to_bits));
            if bypass == 1. { assert_eq!(actual.coefficient, before); }
            else { assert_eq!(actual.coefficient.unwrap().0, frequency.to_bits()); }
        }
    }


    #[test]
    fn gain_accepts_the_full_serialized_twelve_db_endpoint() {
        let mut gain = Gain::new(2).unwrap();
        gain.set_parameter("Volume", 3.9810717).unwrap();
        let mut frames = [[0.125; MAX_CHANNELS]; 1];
        gain.process(&mut frames);
        assert!((frames[0][0] - 0.49763396).abs() < 1e-7);
        assert_eq!(frames[0][1], frames[0][0]);
        assert_eq!(frames[0][2], 0.125);
        assert!(gain.set_parameter("Volume", f64::NAN).is_err());
        gain.set_effective_volume(16.).unwrap();
        let mut frames = [[0.125; MAX_CHANNELS]; 1];
        gain.process(&mut frames);
        assert_eq!(frames[0][0], 2.);
        assert!(gain.set_effective_volume(f64::INFINITY).is_err());
    }

    #[test]
    fn one_pole_matches_native_float32_impulses_and_reset() {
        // Measured from the locally installed official UVI Workstation.
        // Numeric observations only; no vendor code or library audio fixtures.
        let native: [(f64, [f64; 8]); 3] = [
            (
                100.,
                [
                    0.013004660606384277,
                    0.012835539877414703,
                    0.012668618001043797,
                    0.012503867037594318,
                    0.012341258116066456,
                    0.012180764228105545,
                    0.012022357434034348,
                    0.0118660107254982,
                ],
            ),
            (
                1000.,
                [
                    0.12269425392150879,
                    0.10764037072658539,
                    0.09443351626396179,
                    0.08284706622362137,
                    0.0726822093129158,
                    0.06376451998949051,
                    0.05594097822904587,
                    0.049077343195676804,
                ],
            ),
            (
                10000.,
                [
                    0.7299091815948486,
                    0.19714176654815674,
                    0.053246185183525085,
                    0.014381304383277893,
                    0.0038842586800456047,
                    0.0010491025168448687,
                    0.0002833529724739492,
                    0.00007653103966731578,
                ],
            ),
        ];
        for (frequency, reference) in native {
            let mut pole = OnePole::new(6, 48000.).unwrap();
            pole.set_parameter("Freq", frequency).unwrap();
            for mode in [0., 1.] {
                pole.clear();
                pole.set_parameter("Mode", mode).unwrap();
                let mut impulse = [[0.; MAX_CHANNELS]; 8];
                impulse[0][..6].fill(1.);
                pole.process(&mut impulse).unwrap();
                for (n, frame) in impulse.iter().enumerate() {
                    let expected = reference[n] as f32;
                    let expected = if mode == 0. {
                        expected
                    } else if n == 0 {
                        1. - expected
                    } else {
                        -expected
                    };
                    assert!(
                        frame[..6]
                            .iter()
                            .all(|sample| sample.to_bits() == expected.to_bits()),
                        "native OnePole mismatch: frequency {frequency}, mode {mode}, frame {n}"
                    );
                    assert!(frame[6..].iter().all(|sample| *sample == 0.));
                }
            }
        }
        // Bypass freezes this implementation's state; native transitions
        // remain covered by the fidelity diagnostic, not an equivalence claim.
        let mut pole = OnePole::new(1, 48000.).unwrap();
        let mut frame = [[0.; MAX_CHANNELS]; 1];
        frame[0][0] = 1.;
        pole.process(&mut frame).unwrap();
        let first = frame[0][0];
        pole.set_parameter("Bypass", 1.).unwrap();
        frame[0][0] = 7.;
        pole.process(&mut frame).unwrap();
        assert_eq!(frame[0][0], 7.);
        pole.set_parameter("Bypass", 0.).unwrap();
        frame[0][0] = 0.;
        pole.process(&mut frame).unwrap();
        assert_eq!(frame[0][0], first - first * first);
        pole.clear();
        frame[0][0] = 1.;
        pole.process(&mut frame).unwrap();
        assert_eq!(frame[0][0], first);
    }

    #[test]
    fn six_channel_matrix_matches_independent_weighted_impulses() {
        let mut matrix = GainMatrix::new(6, 2).unwrap();
        for input in 1..=6 {
            matrix
                .set_parameter(&format!("Gain_{input}_1"), input as f64 / 6.)
                .unwrap();
            matrix
                .set_parameter(&format!("Gain_{input}_2"), -(input as f64) / 6.)
                .unwrap();
        }
        let mut input = [[0.; MAX_CHANNELS]; 6];
        for (channel, frame) in input.iter_mut().enumerate() {
            frame[channel] = 1.;
        }
        let mut output = [[9.; MAX_CHANNELS]; 6];
        matrix.process(&input, &mut output).unwrap();
        for (channel, frame) in output.iter().enumerate() {
            let weight = (channel + 1) as f32 / 6.;
            assert!((frame[0] - weight).abs() < 1e-6);
            assert!((frame[1] + weight).abs() < 1e-6);
            assert!(frame[2..].iter().all(|x| *x == 0.));
        }
        matrix.set_parameter("Bypass", 1.).unwrap();
        matrix.process(&input, &mut output).unwrap();
        assert_eq!(input, output);
        assert!(matrix.set_parameter("Gain_13_1", 0.).is_err());
        assert!(matrix.set_parameter("Gain_1_1", f64::NAN).is_err());
        assert!(GainMatrix::new(13, 2).is_err());
    }

    #[test]
    fn track_delay_matches_native_float32_duration_rounding() {
        let mut delay = TrackDelay::new(6, 48000.).unwrap();
        // Native synthetic impulse locations, including float32 boundaries.
        for (requested_frames, native_frame) in [
            (0., 0),
            (0.1, 1),
            (1., 1),
            (2., 2),
            (2.1, 3),
            (2.5, 3),
            (2.9, 3),
            (3., 4),
            (3.1, 4),
            (4., 4),
            (48., 49),
        ] {
            delay.clear();
            delay
                .set_parameter("DelayTime", requested_frames / 48000.)
                .unwrap();
            let mut frames = [[0.; MAX_CHANNELS]; 64];
            frames[0][..6].fill(0.125);
            for chunk in frames.chunks_mut(7) {
                delay.process(chunk).unwrap();
            }
            for (n, frame) in frames.iter().enumerate() {
                let expected = if n == native_frame { 0.125 } else { 0. };
                assert!(
                    frame[..6].iter().all(|sample| *sample == expected),
                    "native TrackDelay mismatch: requested {requested_frames}, frame {n}"
                );
            }
        }
        delay.set_parameter("SyncToHost", 1.).unwrap();
        delay.set_tempo(120.).unwrap();
        for (beats, native_frame) in [(0.125, 3000), (0.25, 6000), (0.5, 12000)] {
            delay.clear();
            delay.set_parameter("DelayTime", beats).unwrap();
            let mut frames = vec![[0.; MAX_CHANNELS]; native_frame + 1];
            frames[0][..6].fill(0.125);
            for chunk in frames.chunks_mut(257) {
                delay.process(chunk).unwrap();
            }
            assert!(
                frames[..native_frame]
                    .iter()
                    .all(|frame| frame[..6].iter().all(|sample| *sample == 0.))
            );
            assert!(
                frames[native_frame][..6]
                    .iter()
                    .all(|sample| *sample == 0.125)
            );
            assert_eq!(delay.parameter("DelayTime").unwrap(), beats);
        }
    }

    #[test]
    fn filter_and_delay_state_survive_fragmented_multichannel_processing() {
        let mut source = vec![[0.; MAX_CHANNELS]; 257];
        for (channel, value) in source[0][..6].iter_mut().enumerate() {
            *value = (channel + 1) as f32;
        }
        let mut complete = source.clone();
        let mut fragmented = source.clone();
        let mut filter = OnePole::new(6, 48_000.).unwrap();
        let mut split_filter = OnePole::new(6, 48_000.).unwrap();
        filter.process(&mut complete).unwrap();
        for chunk in fragmented.chunks_mut(7) {
            split_filter.process(chunk).unwrap();
        }
        assert_eq!(complete, fragmented);
        // A one-pole lowpass has unity DC gain, channel-independent.
        for channel in 0..6 {
            let total: f32 = complete.iter().map(|f| f[channel]).sum();
            assert!((total - (channel + 1) as f32).abs() < 1e-5);
        }
        let mut delay = TrackDelay::new(6, 48_000.).unwrap();
        let mut split_delay = TrackDelay::new(6, 48_000.).unwrap();
        for d in [&mut delay, &mut split_delay] {
            d.set_parameter("DelayTime", 2.5 / 48_000.).unwrap();
        }
        delay.process(&mut complete).unwrap();
        for chunk in fragmented.chunks_mut(11) {
            split_delay.process(chunk).unwrap();
        }
        assert_eq!(complete, fragmented);
        let mut impulse = source.clone();
        delay.clear();
        delay.process(&mut impulse).unwrap();
        for channel in 0..6 {
            assert_eq!(impulse[2][channel], 0.);
            assert_eq!(impulse[3][channel], (channel + 1) as f32);
        }
        delay.set_parameter("SyncToHost", 1.).unwrap();
        delay.set_parameter("DelayTime", 0.25).unwrap();
        delay.set_tempo(120.).unwrap();
        assert_eq!(delay.delay_frames(0.25, true, 120.).unwrap(), 6000);
        assert!(delay.set_parameter("DelayTime", 6.).is_err());
        assert!(filter.set_parameter("KeyTracking", 1.).is_err());
        filter.clear();
        filter.set_parameter("Mode", 1.).unwrap();
        let mut dc = vec![[1.; MAX_CHANNELS]; 1000];
        filter.process(&mut dc).unwrap();
        assert!(dc[999][..6].iter().all(|sample| sample.abs() < 1e-6));
        let mut gain = Gain::new(6).unwrap();
        gain.set_parameter("Volume", 0.5).unwrap();
        gain.process(&mut source[..1]);
        assert_eq!(source[0][5], 3.);
    }
}
