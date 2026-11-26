//! Multi-stage amplifier and distortion/overdrive pedal processor.
//!
//! Pre-allocated, allocation-free on the audio thread, denormal-safe, and
//! smoothed across parameter changes.

use std::f64::consts::TAU;

use kontra_dsp_core::{Biquad, Side, high_pass, shelf};

/// Graphic EQ band center frequencies (Hz) for 9-band bass amplifiers.
pub const GEQ_BANDS_HZ: [f32; 9] = [
    40.0, 90.0, 180.0, 300.0, 500.0, 1000.0, 2000.0, 4000.0, 10_000.0,
];

/// Nonlinear circuit voicing topology.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AmpVoicing {
    /// Clean combo with bright shelf and warm asymmetrical tube breakup.
    CleanCombo,
    /// British high-gain cascaded triodes with strong upper-mid bite.
    BritishHighGain,
    /// Modern high-gain dual-channel preamp with tight low-end resonance.
    ModernHighGain,
    /// Class-A chimey combo with interactive tone stack and power-amp cut.
    ClassACombo,
    /// Lead high-gain preamp with smooth singing sustain and depth control.
    LeadHighGain,
    /// Hard-clipping op-amp distortion pedal with reverse low-pass filter.
    HardClipPedal,
    /// Tube bass amplifier with sweepable mid frequency and 9-band graphic EQ.
    BassTube,
    /// Mid-humped soft-clipping overdrive pedal with 3 clipping modes (`0..=2`).
    MidOverdrive(u8),
}

/// Physical settings for [`AmpStack`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AmpStackSettings {
    pub voicing: AmpVoicing,
    pub preamp_gain: f32,
    pub drive: f32,
    pub master: f32,
    pub high_gain: bool,
    pub bright: bool,
    pub low_boost: bool,
    pub high_boost: bool,
    pub bass_db: f32,
    pub mid_db: f32,
    pub mid_hz: f32,
    pub treble_db: f32,
    pub presence_db: f32,
    pub resonance_db: f32,
    pub tone_cut_hz: f32,
    pub tremolo_hz: f32,
    pub tremolo_depth: f32,
    pub geq_enabled: bool,
    pub geq_db: [f32; 9],
    pub geq_gain_db: f32,
    pub mono: bool,
}

#[inline]
fn flush32(x: f32) -> f32 {
    if x.abs() < 1e-20 || !x.is_finite() {
        0.0
    } else {
        x
    }
}

#[inline]
fn db_to_lin(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

/// Cookbook peaking EQ biquad.
fn peak(hz: f32, q: f32, gain_db: f32, rate: u32) -> Biquad {
    let r = f64::from(rate.max(1));
    let f = f64::from(hz).clamp(20.0, 0.45 * r);
    let w = TAU * f / r;
    let a = 10.0f64.powf(f64::from(gain_db) / 40.0);
    let alpha = w.sin() / (2.0 * f64::from(q).max(0.1));
    let cos_w = w.cos();
    let b0 = 1.0 + alpha * a;
    let b1 = -2.0 * cos_w;
    let b2 = 1.0 - alpha * a;
    let a0 = 1.0 + alpha / a;
    let a1 = -2.0 * cos_w;
    let a2 = 1.0 - alpha / a;
    Biquad::new([b0, b1, b2, a0, a1, a2])
}

/// Cookbook second-order low-pass biquad.
fn low_pass(hz: f32, q: f32, rate: u32) -> Biquad {
    let r = f64::from(rate.max(1));
    let f = f64::from(hz).clamp(20.0, 0.47 * r);
    let w = TAU * f / r;
    let cos_w = w.cos();
    let alpha = w.sin() / (2.0 * f64::from(q).max(0.1));
    let b0 = 0.5 * (1.0 - cos_w);
    let b1 = 1.0 - cos_w;
    let b2 = b0;
    let a0 = 1.0 + alpha;
    let a1 = -2.0 * cos_w;
    let a2 = 1.0 - alpha;
    Biquad::new([b0, b1, b2, a0, a1, a2])
}

/// Asymmetric triode-like saturation curve with soft ceiling.
#[inline]
fn triode(x: f32, asym: f32) -> f32 {
    let shifted = x + asym;
    if shifted >= 0.0 {
        shifted / (1.0 + shifted * shifted).sqrt()
    } else {
        (shifted * 1.15).tanh() / 1.15
    }
}

/// Hard-clipping diode/op-amp stage with smooth shoulder.
#[inline]
fn diode_clip(x: f32, threshold: f32) -> f32 {
    let t = threshold.max(0.05);
    let u = x / t;
    let clipped = u / (1.0 + u.abs().powf(2.5)).powf(0.4);
    clipped * t
}

/// Multi-stage amplifier / overdrive / distortion processor.
#[derive(Debug)]
pub struct AmpStack {
    s: AmpStackSettings,
    rate: u32,
    pre_hp: Biquad,
    bright_shelf: Biquad,
    pre_mid: Biquad,
    bass: Biquad,
    mid: Biquad,
    treble: Biquad,
    presence: Biquad,
    resonance: Biquad,
    tone_cut: Biquad,
    geq: [Biquad; 9],
    /// DC blocker state per channel (`[x_prev, y_prev]`).
    dc: [[f32; 2]; 2],
    /// Smoothed preamp gain, secondary drive, master gain, tremolo depth.
    preamp_cur: f32,
    drive_cur: f32,
    master_cur: f32,
    trem_depth_cur: f32,
    smooth: f32,
    /// Optical tremolo LFO phase in cycles (`0..1`).
    trem_phase: f64,
}

impl AmpStack {
    #[must_use]
    pub fn new(s: AmpStackSettings, rate: u32) -> Self {
        let rate = rate.max(8_000);
        let smooth = 1.0 - (-1.0 / (0.008 * rate as f32)).exp();
        let flat = peak(1000.0, 1.0, 0.0, rate);
        let mut amp = Self {
            s,
            rate,
            pre_hp: high_pass(60.0, 0.707, rate),
            bright_shelf: shelf(2400.0, 0.0, 0.7, Side::High, rate),
            pre_mid: flat.clone(),
            bass: shelf(120.0, 0.0, 0.7, Side::Low, rate),
            mid: flat.clone(),
            treble: shelf(3200.0, 0.0, 0.7, Side::High, rate),
            presence: shelf(4200.0, 0.0, 0.7, Side::High, rate),
            resonance: shelf(100.0, 0.0, 0.7, Side::Low, rate),
            tone_cut: low_pass(18_000.0, 0.707, rate),
            geq: GEQ_BANDS_HZ.map(|hz| peak(hz, 1.4, 0.0, rate)),
            dc: [[0.0; 2]; 2],
            preamp_cur: s.preamp_gain.max(0.0),
            drive_cur: s.drive.max(0.0),
            master_cur: s.master.max(0.0),
            trem_depth_cur: s.tremolo_depth.clamp(0.0, 1.0),
            smooth,
            trem_phase: 0.0,
        };
        amp.retune_filters(s);
        amp
    }

    /// Updates settings without allocating; filter states and gain smoothers
    /// carry across seamlessly.
    pub fn set(&mut self, s: AmpStackSettings) {
        self.s = s;
        self.retune_filters(s);
    }

    fn retune_filters(&mut self, s: AmpStackSettings) {
        let rate = self.rate;
        let (hp_hz, pre_mid_hz, pre_mid_db, bright_hz, bright_db, bass_hz, treble_hz) =
            match s.voicing {
                AmpVoicing::CleanCombo => (
                    55.0,
                    900.0,
                    -1.5,
                    3200.0,
                    if s.bright { 6.0 } else { 0.0 },
                    110.0,
                    3400.0,
                ),
                AmpVoicing::BritishHighGain => (
                    95.0,
                    1200.0,
                    if s.high_gain { 6.0 } else { 3.0 },
                    2800.0,
                    if s.bright { 4.5 } else { 1.5 },
                    120.0,
                    2800.0,
                ),
                AmpVoicing::ModernHighGain => (
                    110.0,
                    1050.0,
                    if s.high_gain { 5.5 } else { 2.0 },
                    3500.0,
                    if s.bright { 5.0 } else { 0.0 },
                    115.0,
                    3000.0,
                ),
                AmpVoicing::ClassACombo => (
                    70.0,
                    1400.0,
                    2.5,
                    3600.0,
                    if s.bright { 5.5 } else { 1.5 },
                    130.0,
                    3200.0,
                ),
                AmpVoicing::LeadHighGain => (
                    100.0,
                    950.0,
                    if s.high_gain { 6.5 } else { 2.0 },
                    3200.0,
                    if s.bright { 4.5 } else { 0.5 },
                    110.0,
                    2900.0,
                ),
                AmpVoicing::HardClipPedal => (
                    85.0,
                    1100.0,
                    4.0,
                    2500.0,
                    if s.bright { 4.0 } else { 0.0 },
                    125.0,
                    2600.0,
                ),
                AmpVoicing::BassTube => (
                    if s.low_boost { 28.0 } else { 38.0 },
                    450.0,
                    if s.low_boost { -2.5 } else { 0.0 },
                    3800.0,
                    (if s.bright { 5.0 } else { 0.0 }) + (if s.high_boost { 4.0 } else { 0.0 }),
                    90.0,
                    3500.0,
                ),
                AmpVoicing::MidOverdrive(mode) => (
                    match mode {
                        1 => 140.0,
                        2 => 90.0,
                        _ => 220.0,
                    },
                    720.0,
                    match mode {
                        1 => 3.5,
                        2 => 7.0,
                        _ => 5.5,
                    },
                    3000.0,
                    if s.bright { 3.0 } else { 0.0 },
                    120.0,
                    2800.0,
                ),
            };

        let low_extra = if s.low_boost {
            match s.voicing {
                AmpVoicing::BassTube => 5.5,
                AmpVoicing::ModernHighGain => 2.5,
                _ => 3.5,
            }
        } else {
            0.0
        };

        self.pre_hp.retune(&high_pass(hp_hz, 0.707, rate));
        self.bright_shelf
            .retune(&shelf(bright_hz, bright_db, 0.7, Side::High, rate));
        self.pre_mid
            .retune(&peak(pre_mid_hz, 0.9, pre_mid_db, rate));
        self.bass.retune(&shelf(
            bass_hz,
            (s.bass_db + low_extra).clamp(-20.0, 20.0),
            0.7,
            Side::Low,
            rate,
        ));
        self.mid.retune(&peak(
            s.mid_hz.clamp(160.0, 3600.0),
            0.9,
            s.mid_db.clamp(-20.0, 20.0),
            rate,
        ));
        self.treble.retune(&shelf(
            treble_hz,
            s.treble_db.clamp(-20.0, 20.0),
            0.7,
            Side::High,
            rate,
        ));
        self.presence.retune(&shelf(
            4200.0,
            s.presence_db.clamp(-14.0, 14.0),
            0.7,
            Side::High,
            rate,
        ));
        self.resonance.retune(&shelf(
            105.0,
            s.resonance_db.clamp(-14.0, 14.0),
            0.7,
            Side::Low,
            rate,
        ));
        let cut_hz = s.tone_cut_hz.clamp(400.0, 0.45 * rate as f32);
        self.tone_cut.retune(&low_pass(cut_hz, 0.707, rate));

        if s.geq_enabled {
            for (band, (&hz, &db)) in self
                .geq
                .iter_mut()
                .zip(GEQ_BANDS_HZ.iter().zip(s.geq_db.iter()))
            {
                band.retune(&peak(hz, 1.4, db.clamp(-16.0, 16.0), rate));
            }
        }
    }

    #[inline]
    fn shape(s: &AmpStackSettings, x: f32, preamp: f32, drive: f32) -> f32 {
        let boost = if s.high_gain { 2.4 } else { 1.0 };
        let g = (preamp * boost).clamp(0.0, 96.0);
        let d = drive.clamp(0.0, 24.0);
        match s.voicing {
            AmpVoicing::CleanCombo => {
                let s1 = triode(x * (0.4 + g * 2.2), 0.08);
                triode(s1 * (1.0 + d * 0.6), -0.04) * 0.75
            }
            AmpVoicing::BritishHighGain => {
                let s1 = triode(x * (0.6 + g * 4.5), 0.14);
                let s2 = triode(s1 * (1.2 + d * 1.4), -0.10);
                (s2 * 1.35).tanh() * 0.65
            }
            AmpVoicing::ModernHighGain => {
                let crunch = if s.low_boost { 1.6 } else { 1.0 };
                let s1 = triode(x * (0.8 + g * 5.5 * crunch), 0.18);
                let s2 = triode(s1 * (1.5 + d * 1.8), -0.14);
                let s3 = (s2 * 1.6).tanh();
                s3 * 0.6
            }
            AmpVoicing::ClassACombo => {
                let s1 = triode(x * (0.5 + g * 2.8), 0.12);
                let s2 = (s1 * (1.1 + d * 0.9)).tanh();
                s2 * 0.72
            }
            AmpVoicing::LeadHighGain => {
                let s1 = triode(x * (0.7 + g * 5.0), 0.16);
                let s2 = triode(s1 * (1.4 + d * 1.6), -0.12);
                (s2 * 1.45).tanh() * 0.62
            }
            AmpVoicing::HardClipPedal => {
                let u = x * (0.5 + g * 6.0);
                let clipped = diode_clip(u, 0.55);
                let sat = (clipped * (1.0 + d * 1.2)).tanh();
                sat * 0.7
            }
            AmpVoicing::BassTube => {
                let s1 = triode(x * (0.4 + g * 2.6), 0.06);
                let s2 = triode(s1 * (0.9 + d * 1.3), -0.05);
                s2 * 0.78
            }
            AmpVoicing::MidOverdrive(mode) => {
                let u = x * (0.6 + g * 4.2);
                match mode {
                    // Classic symmetric soft diode clipping
                    0 => (u * (1.0 + d * 0.8)).tanh() * 0.75,
                    // Warm asymmetric clipping
                    1 => triode(u * (1.0 + d * 0.9), 0.22) * 0.75,
                    // High-headroom crunchy hard-knee overdrive
                    _ => diode_clip(u * (1.1 + d), 0.65) * 0.8,
                }
            }
        }
    }

    /// Processes stereo audio in place with zero allocations.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        if io.is_empty() {
            return;
        }
        let mono = self.s.mono;
        let geq_on = self.s.geq_enabled;
        let geq_gain = if geq_on {
            db_to_lin(self.s.geq_gain_db.clamp(-24.0, 24.0))
        } else {
            1.0
        };
        let use_cut = self.s.tone_cut_hz < 19_500.0;
        let trem_step = f64::from(self.s.tremolo_hz.clamp(0.0, 25.0)) / f64::from(self.rate);
        let preamp_tgt = self.s.preamp_gain.max(0.0);
        let drive_tgt = self.s.drive.max(0.0);
        let master_tgt = self.s.master.max(0.0);
        let trem_tgt = self.s.tremolo_depth.clamp(0.0, 1.0);
        let smooth = self.smooth;

        if mono {
            for frame in io.iter_mut() {
                let m = 0.5 * (frame[0] + frame[1]);
                *frame = [m, m];
            }
        }

        self.pre_hp.process(io);
        self.bright_shelf.process(io);
        self.pre_mid.process(io);

        // Nonlinear preamp + power-amp waveshaping + DC block.
        let dc_r = 0.996f32;
        for frame in io.iter_mut() {
            self.preamp_cur += (preamp_tgt - self.preamp_cur) * smooth;
            self.drive_cur += (drive_tgt - self.drive_cur) * smooth;
            let preamp = self.preamp_cur;
            let drive = self.drive_cur;
            for (sample, st) in frame.iter_mut().zip(self.dc.iter_mut()) {
                let shaped = Self::shape(&self.s, *sample, preamp, drive);
                let y = flush32(shaped - st[0] + dc_r * st[1]);
                st[0] = shaped;
                st[1] = y;
                *sample = y;
            }
        }

        // Tone stack + power-amp presence/resonance + tone cut.
        self.bass.process(io);
        self.mid.process(io);
        self.treble.process(io);
        self.presence.process(io);
        self.resonance.process(io);
        if use_cut {
            self.tone_cut.process(io);
        }

        // Optional 9-band graphic EQ.
        if geq_on {
            for band in &mut self.geq {
                band.process(io);
            }
        }

        // Optical tremolo + master volume.
        for frame in io.iter_mut() {
            self.master_cur += (master_tgt - self.master_cur) * smooth;
            self.trem_depth_cur += (trem_tgt - self.trem_depth_cur) * smooth;
            let trem = if self.trem_depth_cur > 1e-5 && trem_step > 0.0 {
                let lfo = 0.5 * (1.0 - (TAU * self.trem_phase).cos()) as f32;
                self.trem_phase = (self.trem_phase + trem_step).fract();
                1.0 - 0.85 * self.trem_depth_cur * lfo
            } else {
                1.0
            };
            let gain = self.master_cur * geq_gain * trem;
            frame[0] = flush32(2.5 * (frame[0] * gain * 0.4).tanh());
            frame[1] = flush32(2.5 * (frame[1] * gain * 0.4).tanh());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_settings(voicing: AmpVoicing) -> AmpStackSettings {
        AmpStackSettings {
            voicing,
            preamp_gain: 4.0,
            drive: 2.0,
            master: 1.0,
            high_gain: false,
            bright: false,
            low_boost: false,
            high_boost: false,
            bass_db: 0.0,
            mid_db: 0.0,
            mid_hz: 700.0,
            treble_db: 0.0,
            presence_db: 0.0,
            resonance_db: 0.0,
            tone_cut_hz: 20_000.0,
            tremolo_hz: 0.0,
            tremolo_depth: 0.0,
            geq_enabled: false,
            geq_db: [0.0; 9],
            geq_gain_db: 0.0,
            mono: false,
        }
    }

    #[test]
    fn all_voicings_produce_finite_harmonic_output() {
        let voicings = [
            AmpVoicing::CleanCombo,
            AmpVoicing::BritishHighGain,
            AmpVoicing::ModernHighGain,
            AmpVoicing::ClassACombo,
            AmpVoicing::LeadHighGain,
            AmpVoicing::HardClipPedal,
            AmpVoicing::BassTube,
            AmpVoicing::MidOverdrive(0),
            AmpVoicing::MidOverdrive(1),
            AmpVoicing::MidOverdrive(2),
        ];
        for v in voicings {
            let mut amp = AmpStack::new(base_settings(v), 48_000);
            let mut io: Vec<[f32; 2]> = (0..1024)
                .map(|n| [(std::f32::consts::TAU * 220.0 * n as f32 / 48_000.0).sin() * 0.4; 2])
                .collect();
            let dry = io.clone();
            amp.process(&mut io);
            assert!(io.iter().flatten().all(|x| x.is_finite()), "{v:?}");
            assert_ne!(io, dry, "{v:?} must alter input");
        }
    }
}
