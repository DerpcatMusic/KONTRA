//! Multi-stage amplifier and distortion/overdrive pedal processor.
//!
//! Pre-allocated, allocation-free on the audio thread, denormal-safe, and
//! smoothed across parameter changes.

use std::f64::consts::TAU;

use crate::stage::{DcBlocker, ToneStack3};
use kontra_dsp_core::{Biquad, Halfband, Side, high_pass, shelf};

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

use kontra_dsp_core::flush_f32 as flush32;

use kontra_dsp_core::math::db_to_gain_f32 as db_to_lin;

use kontra_dsp_core::peak;

use kontra_dsp_core::low_pass;

/// Smooth saturator with unit small-signal slope at `x = 0` and level-dependent
/// cathode self-bias (`bias_env`) that shifts the duty cycle under overdrive,
/// producing authentic even harmonics (H2) without altering small-signal gain.
#[inline]
fn soft_stage(x: f32, ceiling: f32, asym: f32, env: f32) -> f32 {
    let c = ceiling.max(1e-3);
    // Self-bias grows smoothly with drive level (zero in small-signal regime).
    let drive_ratio = (env / (env + c)).clamp(0.0, 1.0);
    let shift = asym * env * drive_ratio;
    ((x - shift) / c).tanh() * c
}

/// Hard-clipping diode/op-amp stage with unit small-signal slope and flat ceiling `threshold`.
#[inline]
fn diode_clip(x: f32, threshold: f32) -> f32 {
    let t = threshold.max(1e-3);
    let u = x / t;
    let clipped = u / (1.0 + u.abs().powf(6.0)).powf(1.0 / 6.0);
    clipped * t
}

/// Multi-stage amplifier / overdrive / distortion processor.
#[derive(Clone, Debug)]
pub struct AmpStack {
    s: AmpStackSettings,
    rate: u32,
    pre_hp: Biquad,
    bright_shelf: Biquad,
    pre_mid: Biquad,
    tone: ToneStack3,
    presence: Biquad,
    resonance: Biquad,
    tone_cut: Biquad,
    geq: [Biquad; 9],
    /// DC blocker state per channel.
    dc: DcBlocker,
    /// Preamp envelope follower per channel for cathode self-bias (H2 asymmetry).
    bias_env: [f32; 2],
    /// Smoothed preamp gain, secondary drive, master gain, tremolo depth.
    preamp_cur: f32,
    drive_cur: f32,
    master_cur: f32,
    trem_depth_cur: f32,
    smooth: f32,
    /// Optical tremolo LFO phase in cycles (`0..1`).
    trem_phase: f64,
    /// 2x polyphase IIR halfband oversampler around the nonlinear stage.
    oversample: Halfband,
}

impl AmpStack {
    #[must_use]
    pub fn new(s: AmpStackSettings, rate: u32) -> Self {
        let rate = rate.max(8_000);
        let smooth = 1.0 - (-1.0 / (0.008 * rate as f32)).exp();
        let mut amp = Self {
            s,
            rate,
            pre_hp: high_pass(60.0, 0.707, rate),
            bright_shelf: shelf(2400.0, 0.0, 0.7, Side::High, rate),
            pre_mid: peak(1000.0, 1.0, 0.0, rate),
            tone: ToneStack3::new(120.0, 1000.0, 3200.0, rate),
            presence: shelf(4200.0, 0.0, 0.7, Side::High, rate),
            resonance: shelf(100.0, 0.0, 0.7, Side::Low, rate),
            tone_cut: low_pass(18_000.0, 0.707, rate),
            geq: GEQ_BANDS_HZ.map(|hz| peak(hz, 1.4, 0.0, rate)),
            dc: DcBlocker::new(0.998),
            bias_env: [0.0; 2],
            preamp_cur: s.preamp_gain.max(0.0),
            drive_cur: s.drive.max(0.0),
            master_cur: s.master.max(0.0),
            trem_depth_cur: s.tremolo_depth.clamp(0.0, 1.0),
            smooth,
            trem_phase: 0.0,
            oversample: Halfband::new(rate),
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

    /// Clears all filter, oversampler, DC-blocker, and envelope states.
    pub fn reset(&mut self) {
        self.pre_hp.reset();
        self.bright_shelf.reset();
        self.pre_mid.reset();
        self.tone.reset();
        self.presence.reset();
        self.resonance.reset();
        self.tone_cut.reset();
        for band in &mut self.geq {
            band.reset();
        }
        self.dc.reset();
        self.bias_env = [0.0; 2];
        self.preamp_cur = self.s.preamp_gain.max(0.0);
        self.drive_cur = self.s.drive.max(0.0);
        self.master_cur = self.s.master.max(0.0);
        self.trem_depth_cur = self.s.tremolo_depth.clamp(0.0, 1.0);
        self.trem_phase = 0.0;
        self.oversample.reset();
    }

    fn retune_filters(&mut self, s: AmpStackSettings) {
        let rate = self.rate;
        let (hp_hz, pre_mid_hz, pre_mid_db, bright_hz, bright_db, bass_hz, treble_hz) =
            match s.voicing {
                AmpVoicing::CleanCombo => (
                    35.0,
                    2200.0,
                    2.1,
                    2200.0,
                    if s.bright { 3.0 } else { 0.0 },
                    180.0,
                    1600.0,
                ),
                AmpVoicing::BritishHighGain => (
                    55.0,
                    1800.0,
                    0.0,
                    850.0,
                    if s.high_gain { 2.5 } else { 14.0 } + if s.bright { 3.0 } else { 0.0 },
                    140.0,
                    1800.0,
                ),
                AmpVoicing::ModernHighGain => (
                    90.0,
                    1100.0,
                    0.0,
                    2200.0,
                    if s.bright { 4.0 } else { -2.4 },
                    130.0,
                    2000.0,
                ),
                AmpVoicing::ClassACombo => (
                    40.0,
                    1200.0,
                    0.0,
                    2000.0,
                    if s.bright { 4.5 } else { -2.6 },
                    140.0,
                    1800.0,
                ),
                AmpVoicing::LeadHighGain => (
                    80.0,
                    1500.0,
                    0.0,
                    1800.0,
                    if s.bright { 7.5 } else { 4.5 },
                    130.0,
                    2000.0,
                ),
                AmpVoicing::HardClipPedal => (
                    165.0,
                    1000.0,
                    0.0,
                    1800.0,
                    if s.bright { 0.0 } else { -5.8 },
                    130.0,
                    2000.0,
                ),
                AmpVoicing::BassTube => (
                    55.0,
                    240.0,
                    if s.low_boost { -3.0 } else { 0.0 },
                    1600.0,
                    5.9 + (if s.low_boost { 3.5 } else { 0.0 })
                        + (if s.high_boost { 3.8 } else { 0.0 })
                        + (if s.bright { 4.0 } else { 0.0 }),
                    100.0,
                    2200.0,
                ),
                AmpVoicing::MidOverdrive(mode) => (
                    135.0,
                    720.0,
                    0.0,
                    1600.0,
                    match mode {
                        1 => 6.0,
                        2 => 1.2,
                        _ => -5.0,
                    } + if s.bright { 3.0 } else { 0.0 },
                    120.0,
                    1800.0,
                ),
            };

        let low_extra = if s.low_boost {
            match s.voicing {
                AmpVoicing::BassTube => 0.0,
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
        self.tone.retune(
            bass_hz,
            (s.bass_db + low_extra).clamp(-20.0, 20.0),
            s.mid_hz.clamp(160.0, 3600.0),
            0.9,
            s.mid_db.clamp(-20.0, 20.0),
            treble_hz,
            s.treble_db.clamp(-20.0, 20.0),
            rate,
        );
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
    fn shape(s: &AmpStackSettings, x: f32, preamp: f32, drive: f32, env: f32) -> f32 {
        let g = preamp.clamp(0.0, 256.0);
        let d = drive.clamp(0.0, 24.0);
        match s.voicing {
            AmpVoicing::CleanCombo => soft_stage(x * g, 0.220, 0.04, env * g),
            AmpVoicing::BritishHighGain => {
                let boost = if s.high_gain { 30.0 } else { 1.0 };
                let ceiling = if s.high_gain { 0.162 } else { 0.26 };
                let asym = if s.high_gain { 0.16 } else { 0.04 };
                soft_stage(x * g * boost, ceiling, asym, env * g * boost)
            }
            AmpVoicing::ModernHighGain => {
                let boost =
                    (if s.high_gain { 2.65 } else { 1.0 }) * (if s.low_boost { 1.4 } else { 1.0 });
                let asym = if s.high_boost { 0.15 } else { 0.03 };
                soft_stage(x * g * boost, 0.420, asym, env * g * boost)
            }
            AmpVoicing::ClassACombo => soft_stage(x * g, 0.210, 0.05, env * g),
            AmpVoicing::LeadHighGain => {
                let boost = if s.high_gain { 10.5 } else { 1.0 };
                let asym = if s.high_gain { 0.18 } else { 0.03 };
                soft_stage(x * g * boost, 0.62, asym, env * g * boost)
            }
            AmpVoicing::HardClipPedal => diode_clip(x * g, 0.025),
            AmpVoicing::BassTube => {
                let drive_pre = 1.0 + 0.38 * (d - 0.5);
                let ceiling = (0.76 - 0.58 * (d - 0.5).max(0.0)).max(0.35);
                soft_stage(x * g * drive_pre, ceiling, 0.08, env * g * drive_pre)
            }
            AmpVoicing::MidOverdrive(mode) => {
                // Non-inverting overdrive topology: clean feed + clipped gain stage.
                let (clean_mix, ceiling, asym) = match mode {
                    1 => (0.12, 0.095, 0.03),
                    2 => (0.14, 0.090, 0.04),
                    _ => (0.28, 0.130, 0.03),
                };
                let sat_gain = (g - clean_mix).max(0.05);
                clean_mix * x + soft_stage(x * sat_gain, ceiling, asym, env * sat_gain)
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

        // 2x oversampled nonlinear preamp + power-amp waveshaping + DC block.
        let os_rate = 2.0 * self.rate as f32;
        let smooth_os = 1.0 - (-1.0 / (0.008 * os_rate)).exp();
        let env_att = 1.0 - (-1.0 / (0.004 * os_rate)).exp();
        let env_rel = 1.0 - (-1.0 / (0.040 * os_rate)).exp();
        let s = self.s;
        let preamp_cur = &mut self.preamp_cur;
        let drive_cur = &mut self.drive_cur;
        let dc = &mut self.dc;
        let bias_env = &mut self.bias_env;
        self.oversample.process(io, |mut frame| {
            *preamp_cur += (preamp_tgt - *preamp_cur) * smooth_os;
            *drive_cur += (drive_tgt - *drive_cur) * smooth_os;
            let preamp = *preamp_cur;
            let drive = *drive_cur;
            for (ch, (sample, env)) in frame.iter_mut().zip(bias_env.iter_mut()).enumerate() {
                let a = sample.abs();
                let c = if a > *env { env_att } else { env_rel };
                *env = flush32(*env + (a - *env) * c);
                let shaped = Self::shape(&s, *sample, preamp, drive, *env);
                *sample = dc.tick_ch(ch, shaped);
            }
            frame
        });

        // Tone stack + power-amp presence/resonance + tone cut.
        self.tone.process(io);
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
            frame[0] = flush32(3.0 * (frame[0] * gain / 3.0).tanh());
            frame[1] = flush32(3.0 * (frame[1] * gain / 3.0).tanh());
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
