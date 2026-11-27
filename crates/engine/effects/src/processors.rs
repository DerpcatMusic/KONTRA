//! Each slot's processor from its IR effect: static-dispatch [`DspUnit`]
//! (built on unified [`Space`], [`Dynamics`], [`Drive`], filter, and time-mod
//! families — zero trait-object dispatch) and the summed EQ/filter stages,
//! with their settings in DSP terms. Not real-time unless a function says so.

use kontra_dsp::delay::{
    BeatMasher, BeatMasherSettings, BeatSlicer, BeatSlicerSettings, CombBank, CombSettings,
    DUAL_MAX_MS, DualDelay, DualDelaySettings, Gater, GaterSettings, MAX_MS, ModDelay, Phaser,
    PhaserSettings, ReverseGrain, ReverseGrainSettings, Scanner, ScannerSettings, Shifter,
    ShifterSettings, TransposeStretch, TransposeStretchSettings,
};
use kontra_dsp::drive::{
    self, AmpStack, AmpStackSettings, AmpVoicing, Cabinet, CabinetSettings, Circuit, Clipper,
    ColorSat, ColorSatSettings, Converter, Crush, Drive, Overdrive, Reduction, Shaper, Voicing,
};
use kontra_dsp::dynamics::{
    Compressor, CompressorSettings, Detector, Dynamics, Limiter, LimiterSettings, Transient,
    TransientSettings,
};
use kontra_dsp::filter::{
    Cascade, CascadeSettings, CascadeStage, Clocked, ClockedShape, NotchSettings, Notches,
    SpectralMorph, SpectralMorphSettings, Svf, Wah, WahSettings,
};
use kontra_dsp::reverb::{Convolver, Settings, Space};
use kontra_dsp::spatial::{
    Ring, RingMod, RingSource, Rotary, RotarySettings, StereoDetune, StereoDetuneSettings,
};
use kontra_ir as ir;
use kontra_voice::Gain;

use crate::ensemble::Ensemble;
use crate::{MAX_SPAN, Response, matrix as effects, summed as filters};

/// Copies matching fields from an IR settings struct into its DSP counterpart,
/// with optional computed overrides placed before `;`.
macro_rules! map_fields {
    ($src:ident => $($Dst:ident)::+ { $( $k:ident : $v:expr ),+ $(,)? ; $( $f:ident ),* $(,)? }) => {
        $($Dst)::+ { $( $k: $v, )+ $( $f: $src.$f, )* }
    };
    ($src:ident => $($Dst:ident)::+ { $( $f:ident ),* $(,)? }) => {
        $($Dst)::+ { $( $f: $src.$f, )* }
    };
}

pub enum Dsp {
    None,
    Eq(Box<[filters::Stage]>),
    /// An EQ routes move: all its bands, retuned each span.
    Bells(Box<filters::VoiceEq>),
    /// A filter, following its slot each span (a script may move or
    /// reshape it).
    Filter(Box<Svf>),
    /// A wet path (static-dispatch [`DspUnit`]) and a span of its output.
    Wet(DspUnit, Box<[[f32; 2]]>),
}

/// Static-dispatch wet DSP processor for audio-thread slot execution.
/// Large processors are boxed inside their variants so `DspUnit` stays small
/// while eliminating all vtable indirection on the audio thread.
pub enum DspUnit {
    Space(Space),
    Dynamics(Dynamics),
    Drive(Drive),
    /// [`ir::Strip`]: a compressor into a waveshaper, and the rate both retune
    /// at. Accepts a plain shaper's settings too (its compressor then passes).
    Strip(Box<Compressor>, Box<Shaper>, u32),
    Cascade(Box<Cascade>),
    Clocked(Box<Clocked>),
    Notches(Box<Notches>),
    Wah(Box<Wah>),
    SpectralMorph(Box<SpectralMorph>),
    ModDelay(Box<ModDelay>),
    DualDelay(Box<DualDelay>),
    Scanner(Box<Scanner>),
    Shifter(Box<Shifter>),
    Phaser(Box<Phaser>),
    Ensemble(Box<Ensemble>),
    CombBank(Box<CombBank>),
    Ring(Box<Ring>, u32),
    Rotary(Box<Rotary>),
    StereoDetune(Box<StereoDetune>),
    Gater(Box<Gater>),
    BeatMasher(Box<BeatMasher>),
    BeatSlicer(Box<BeatSlicer>),
    ReverseGrain(Box<ReverseGrain>),
    TransposeStretch(Box<TransposeStretch>),
}

impl DspUnit {
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        match self {
            Self::Space(u) => u.process(io),
            Self::Dynamics(u) => u.process(io),
            Self::Drive(u) => u.process(io),
            Self::Strip(c, s, _) => {
                c.process(io);
                s.process(io);
            }
            Self::Cascade(u) => u.process(io),
            Self::Clocked(u) => u.process(io),
            Self::Notches(u) => u.process(io),
            Self::Wah(u) => u.process(io),
            Self::SpectralMorph(u) => u.process(io),
            Self::ModDelay(u) => u.process(io),
            Self::DualDelay(u) => u.process(io),
            Self::Scanner(u) => u.process(io),
            Self::Shifter(u) => u.process(io),
            Self::Phaser(u) => u.process(io),
            Self::Ensemble(u) => u.process(io),
            Self::CombBank(u) => u.process(io),
            Self::Ring(u, _) => u.process(io),
            Self::Rotary(u) => u.process(io),
            Self::StereoDetune(u) => u.process(io),
            Self::Gater(u) => u.process(io),
            Self::BeatMasher(u) => u.process(io),
            Self::BeatSlicer(u) => u.process(io),
            Self::ReverseGrain(u) => u.process(io),
            Self::TransposeStretch(u) => u.process(io),
        }
    }

    /// Processes `io` using an external sidechain `key` signal for detection.
    /// Non-sidechain processors ignore `key` and run [`Self::process`].
    pub fn process_sidechain(&mut self, io: &mut [[f32; 2]], key: &[[f32; 2]]) {
        match self {
            Self::Dynamics(u) => u.process_sidechain(io, key),
            Self::Strip(c, s, _) => {
                c.process_sidechain(io, key);
                s.process(io);
            }
            _ => self.process(io),
        }
    }

    /// Updates parameters from `kind` at `tempo` before a block. Never allocates.
    pub fn set(&mut self, kind: &ir::EffectKind, tempo: f32) {
        match (self, kind) {
            (Self::Space(u), ir::EffectKind::Reverb(r)) => u.set(settings(r)),
            (Self::Dynamics(Dynamics::Compressor(c, rate)), ir::EffectKind::Compressor(k)) => {
                c.set(compressor(*k), *rate);
            }
            (Self::Dynamics(Dynamics::Limiter(l, rate)), ir::EffectKind::Limiter(k)) => {
                l.set(limiter(*k), *rate);
            }
            (Self::Dynamics(Dynamics::Transient(t)), ir::EffectKind::Transient(k)) => {
                t.set(transient(*k));
            }
            (
                Self::Drive(Drive::Shaper(s, rate)),
                ir::EffectKind::Shaper(k) | ir::EffectKind::Strip(ir::Strip { shaper: k, .. }),
            ) => s.retune(curve(&k.curve), tone(k), *rate),
            (Self::Drive(Drive::Overdrive(o, rate)), ir::EffectKind::Overdrive(k)) => {
                o.set(voicing(k), *rate);
            }
            (Self::Drive(Drive::Crush(c, rate)), ir::EffectKind::Crush(k)) => {
                c.set(reduction(*k, *rate));
            }
            (Self::Drive(Drive::AmpStack(a)), ir::EffectKind::AmpStack(k)) => a.set(amp_stack(*k)),
            (Self::Drive(Drive::ColorSat(c)), ir::EffectKind::ColorSat(k)) => c.set(color_sat(*k)),
            (Self::Drive(Drive::Cabinet(c)), ir::EffectKind::Cabinet(k)) => c.set(cabinet(*k)),
            (
                Self::Strip(c, s, rate),
                ir::EffectKind::Strip(ir::Strip {
                    compressor: ck,
                    shaper: sk,
                }),
            ) => {
                c.set(compressor(*ck), *rate);
                s.retune(curve(&sk.curve), tone(sk), *rate);
            }
            (Self::Strip(c, s, rate), ir::EffectKind::Shaper(sk)) => {
                c.set(compressor(PASS), *rate);
                s.retune(curve(&sk.curve), tone(sk), *rate);
            }
            (Self::Cascade(u), ir::EffectKind::Cascade(k)) => u.set(cascade(*k)),
            (Self::Clocked(u), ir::EffectKind::Clocked(k)) => u.set([k.cutoff, k.resonance]),
            (Self::Notches(u), ir::EffectKind::Notches(k)) => u.set(notches(*k, tempo)),
            (Self::Wah(u), ir::EffectKind::Wah(k)) => u.set(wah(*k)),
            (Self::SpectralMorph(u), ir::EffectKind::SpectralMorph(k)) => {
                u.set(spectral_morph(*k));
            }
            (Self::ModDelay(u), ir::EffectKind::Delay(k)) => u.set(delay(k, tempo)),
            (Self::DualDelay(u), ir::EffectKind::DualDelay(k)) => u.set(dual_delay(*k, tempo)),
            (Self::Scanner(u), ir::EffectKind::Scanner(k)) => u.set(scanner(*k)),
            (Self::Shifter(u), ir::EffectKind::Shifter(k)) => u.set(shifter(*k, tempo)),
            (Self::Phaser(u), ir::EffectKind::Phaser(k)) => u.set(phaser(*k, tempo)),
            (Self::Ensemble(u), kind @ ir::EffectKind::Ensemble(_)) => u.set(kind, tempo),
            (Self::CombBank(u), ir::EffectKind::CombBank(k)) => u.set(comb_bank(*k, tempo)),
            (Self::Ring(u, rate), ir::EffectKind::RingMod(k)) => u.set(ring(*k, tempo), *rate),
            (Self::Rotary(u), ir::EffectKind::Rotary(k)) => u.set(rotary(*k)),
            (Self::StereoDetune(u), ir::EffectKind::StereoDetune(k)) => u.set(stereo_detune(*k)),
            (Self::Gater(u), ir::EffectKind::Gater(k)) => u.set(gater(*k, tempo)),
            (Self::BeatMasher(u), ir::EffectKind::BeatMasher(k)) => u.set(beat_masher(*k, tempo)),
            (Self::BeatSlicer(u), ir::EffectKind::BeatSlicer(k)) => u.set(beat_slicer(*k, tempo)),
            (Self::ReverseGrain(u), ir::EffectKind::ReverseGrain(k)) => u.set(reverse_grain(*k)),
            (Self::TransposeStretch(u), ir::EffectKind::TransposeStretch(k)) => {
                u.set(transpose_stretch(*k, tempo));
            }
            _ => {}
        }
    }

    /// Frames after which silent input leaves it silent; `None` means it always runs.
    #[must_use]
    pub fn memory(&self) -> Option<usize> {
        match self {
            Self::Space(u) => u.memory(),
            Self::Drive(Drive::Shaper(_, rate)) => Some(*rate as usize / 3),
            Self::Notches(u) => Some(u.memory()),
            Self::ModDelay(u) => Some(u.memory()),
            Self::DualDelay(u) => Some(u.memory()),
            Self::Shifter(u) => Some(u.memory()),
            Self::Ensemble(u) => u.memory(),
            Self::CombBank(u) => Some(u.memory()),
            Self::Rotary(u) => Some(u.memory()),
            Self::BeatMasher(u) => Some(u.memory()),
            Self::BeatSlicer(u) => Some(u.memory()),
            Self::ReverseGrain(u) => Some(u.memory()),
            Self::TransposeStretch(u) => Some(u.memory()),
            _ => None,
        }
    }

    /// Loudest value written into state during the last [`Self::process`].
    #[must_use]
    pub fn written(&self) -> f32 {
        match self {
            Self::Space(u) => u.written(),
            Self::Notches(u) => u.written(),
            Self::ModDelay(u) => u.written(),
            Self::DualDelay(u) => u.written(),
            Self::Shifter(u) => u.written(),
            Self::Ensemble(u) => u.written(),
            Self::CombBank(u) => u.written(),
            Self::Rotary(u) => u.written(),
            Self::Gater(u) => u.written(),
            Self::BeatMasher(u) => u.written(),
            Self::BeatSlicer(u) => u.written(),
            Self::ReverseGrain(u) => u.written(),
            Self::TransposeStretch(u) => u.written(),
            _ => 0.0,
        }
    }

    /// Advances internal clocks over `silence.len()` silent frames.
    pub fn idle(&mut self, silence: &mut [[f32; 2]]) {
        match self {
            Self::Space(u) => u.idle(silence),
            Self::Notches(u) => u.idle(silence.len()),
            Self::ModDelay(u) => u.idle(silence.len()),
            Self::DualDelay(u) => u.idle(silence.len()),
            Self::Shifter(u) => u.idle(silence),
            Self::Ensemble(u) => u.idle(silence),
            Self::CombBank(u) => u.idle(silence.len()),
            Self::Rotary(u) => u.idle(silence.len()),
            Self::Gater(u) => u.idle(silence.len()),
            Self::BeatMasher(u) => u.idle(silence.len()),
            Self::BeatSlicer(u) => u.idle(silence.len()),
            Self::ReverseGrain(u) => u.idle(silence.len()),
            Self::TransposeStretch(u) => u.idle(silence.len()),
            _ => {}
        }
    }

    /// Whether input gains are all 0 so input reaches neither state nor output.
    #[must_use]
    pub fn deaf(&self) -> bool {
        matches!(self, Self::ModDelay(u) if u.deaf())
    }
}

/// A compressor that passes its input.
const PASS: ir::Compressor = ir::Compressor {
    threshold_db: 0.0,
    ratio: 1.0,
    knee_db: 0.0,
    attack_ms: 1.0,
    release_ms: 1.0,
    makeup_db: 0.0,
    makeup_slope: 0.0,
    link: true,
    mix: 0.0,
    detector: ir::Detector::Input,
    sidechain_hz: None,
};

fn scanner(s: ir::Scanner) -> ScannerSettings {
    map_fields!(s => ScannerSettings {
        sections: usize::from(s.sections);
        henry, farad, last_farad, load_ohm, shunt_ohm, taps, hz, lead, split_hz, high, blend, mix
    })
}

fn rotary(r: ir::Rotary) -> RotarySettings {
    map_fields!(r => RotarySettings {
        fast, horn_slow_hz, horn_fast_hz, drum_slow_hz, drum_fast_hz, accel_hi, accel_lo,
        crossover_hz, balance, distance, mix
    })
}

fn gater(g: ir::Gater, tempo: f32) -> GaterSettings {
    map_fields!(g => GaterSettings {
        rate_hz: hz(g.rate, tempo);
        shape, noise, mix, stutter, mute_input, active
    })
}

fn beat_masher(m: ir::BeatMasher, tempo: f32) -> BeatMasherSettings {
    map_fields!(m => BeatMasherSettings {
        tempo_bpm: tempo;
        length_beats, rotate, gate, mix, reverse, wrap, mash
    })
}

fn beat_slicer(s: ir::BeatSlicer, tempo: f32) -> BeatSlicerSettings {
    map_fields!(s => BeatSlicerSettings {
        tempo_bpm: tempo;
        bars, pattern, style, buzz, gate, slice, mix
    })
}

fn reverse_grain(r: ir::ReverseGrain) -> ReverseGrainSettings {
    map_fields!(r => ReverseGrainSettings {
        speed, grain_ms, pitch_ratio, mix, forward, invert_grains, reverse
    })
}

fn transpose_stretch(t: ir::TransposeStretch, tempo: f32) -> TransposeStretchSettings {
    map_fields!(t => TransposeStretchSettings {
        tempo_bpm: tempo;
        semitones, amount, grain_ms, mix, stretch, grain_mode, bars
    })
}

fn ring(r: ir::RingMod, tempo: f32) -> RingMod {
    let source = match r.source {
        ir::RingSource::Oscillator => RingSource::Oscillator,
        ir::RingSource::Sidechain {
            contour,
            attack,
            release,
            bandpass_hz,
        } => RingSource::Sidechain {
            contour,
            attack,
            release,
            bandpass_hz,
        },
        ir::RingSource::Radio {
            gain,
            tuned_hz,
            damping,
            makeup,
            product,
            noise,
        } => RingSource::Radio {
            gain,
            tuned_hz,
            damping,
            makeup,
            product,
            noise,
        },
        ir::RingSource::Modulated {
            edge,
            fm_ms,
            lfo_rate,
            lfo_depth,
            lfo_square,
        } => RingSource::Modulated {
            edge,
            fm_ms,
            lfo_hz: hz(lfo_rate, tempo),
            lfo_depth,
            lfo_square,
        },
    };
    map_fields!(r => RingMod {
        source: source;
        hz, spread, bias, quadrature, drive, highpass_hz, gate, feedback, wet, dry
    })
}

fn delay_ms(t: ir::DelayTime, tempo: f32) -> f32 {
    match t {
        ir::DelayTime::Ms(ms) => ms,
        ir::DelayTime::Beats(beats) => beats * 60_000.0 / tempo.max(1.0),
    }
}

fn dual_delay(d: ir::DualDelay, tempo: f32) -> DualDelaySettings {
    DualDelaySettings {
        predelay_ms: [
            delay_ms(d.predelay_l, tempo).clamp(0.0, DUAL_MAX_MS),
            delay_ms(d.predelay_r, tempo).clamp(0.0, DUAL_MAX_MS),
        ],
        time_ms: [
            delay_ms(d.time_l, tempo).clamp(0.0, DUAL_MAX_MS),
            delay_ms(d.time_r, tempo).clamp(0.0, DUAL_MAX_MS),
        ],
        feedback: [d.feedback_l, d.feedback_r],
        cross_feedback: d.cross_feedback,
        level: [d.level_l, d.level_r],
        width: d.width,
    }
}

fn stereo_detune(s: ir::StereoDetune) -> StereoDetuneSettings {
    map_fields!(s => StereoDetuneSettings { split_hz, ratio, spread, mix })
}

pub fn wah(w: ir::Wah) -> WahSettings {
    map_fields!(w => WahSettings {
        peak_hz, peak_q, peak_gain, low_hz, low_gain, mid_hz, mid_gain, high_hz, high_gain, mono
    })
}

pub fn amp_stack(a: ir::AmpStack) -> AmpStackSettings {
    let voicing = match a.voicing {
        ir::AmpVoicing::CleanCombo => AmpVoicing::CleanCombo,
        ir::AmpVoicing::BritishHighGain => AmpVoicing::BritishHighGain,
        ir::AmpVoicing::ModernHighGain => AmpVoicing::ModernHighGain,
        ir::AmpVoicing::ClassACombo => AmpVoicing::ClassACombo,
        ir::AmpVoicing::LeadHighGain => AmpVoicing::LeadHighGain,
        ir::AmpVoicing::HardClipPedal => AmpVoicing::HardClipPedal,
        ir::AmpVoicing::BassTube => AmpVoicing::BassTube,
        ir::AmpVoicing::MidOverdrive(mode) => AmpVoicing::MidOverdrive(mode),
    };
    map_fields!(a => AmpStackSettings {
        voicing: voicing;
        preamp_gain, drive, master, high_gain, bright, low_boost, high_boost,
        bass_db, mid_db, mid_hz, treble_db, presence_db, resonance_db, tone_cut_hz,
        tremolo_hz, tremolo_depth, geq_enabled, geq_db, geq_gain_db, mono
    })
}

pub fn cabinet(c: ir::Cabinet) -> CabinetSettings {
    map_fields!(c => CabinetSettings { model, size, air, bass_db, treble_db })
}

pub fn color_sat(c: ir::ColorSat) -> ColorSatSettings {
    map_fields!(c => ColorSatSettings {
        character, drive, boost, boost_db, bass_saver, lpf_hz, hpf_hz,
        bass_db, mid_db, mid_hz, mid_q, treble_db, mix
    })
}

pub fn spectral_morph(m: ir::SpectralMorph) -> SpectralMorphSettings {
    map_fields!(m => SpectralMorphSettings { morph, amount, bank, tracking })
}

fn transient(t: ir::Transient) -> TransientSettings {
    map_fields!(t => TransientSettings { attack, sustain, gain_db, clip })
}

/// A convolution playing `r`. Not real-time.
pub fn convolver(r: &Response) -> Dsp {
    wet(DspUnit::Space(Space::convolver(Convolver::new(
        &r.left, &r.right,
    ))))
}

fn wet(unit: DspUnit) -> Dsp {
    Dsp::Wet(unit, vec![[0.0; 2]; MAX_SPAN as usize].into())
}

/// The plan's responses, then those scripts loaded ([`Engine::load_ir_sample`]).
#[derive(Clone, Copy)]
pub struct Responses<'a>(
    pub &'a [Option<Response>],
    pub &'a [(String, ir::Impulse, Response)],
);

impl Responses<'_> {
    fn get(&self, i: usize) -> Option<&Response> {
        match i.checked_sub(self.0.len()) {
            None => self.0.get(i)?.as_ref(),
            Some(j) => self.1.get(j).map(|(_, _, r)| r),
        }
    }
}

/// A response file's name as [`Plan::responses`] keys it (there in
/// lowercase): without its folder and extension.
pub fn stem(name: &str) -> &str {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    file.rsplit_once('.').map_or(file, |(s, _)| s)
}

/// A compressor's settings in DSP terms.
pub fn compressor(c: ir::Compressor) -> CompressorSettings {
    let detector = match c.detector {
        ir::Detector::Input => Detector::Input,
        ir::Detector::Output => Detector::Output,
        ir::Detector::Trigger => Detector::Trigger,
        ir::Detector::Enhanced => Detector::Enhanced,
        ir::Detector::Bus { auto_release } => Detector::Bus { auto_release },
    };
    map_fields!(c => CompressorSettings {
        makeup_db: c.makeup(),
        detector: detector;
        threshold_db, ratio, knee_db, attack_ms, release_ms, link, mix, sidechain_hz
    })
}

fn limiter(l: ir::Limiter) -> LimiterSettings {
    map_fields!(l => LimiterSettings {
        input_db, ceiling_db, release_ms, knee_db, attack_ms, smooth_ms, slow, lookahead_ms, clip
    })
}

/// LFO cycles per second at `tempo`.
fn hz(rate: ir::Rate, tempo: f32) -> f32 {
    match rate {
        ir::Rate::Hz(hz) => hz,
        ir::Rate::Beats(beats) => tempo / 60.0 / beats.max(1e-3),
        ir::Rate::Steps { beats, count } => tempo / 60.0 / (beats * count).max(1e-3),
    }
}

fn shifter(s: ir::Shifter, tempo: f32) -> ShifterSettings {
    let ms = delay_ms(s.time, tempo);
    map_fields!(s => ShifterSettings { ms: [ms, ms * s.right]; speed, feedback, cross })
}

fn delay(d: &ir::Delay, tempo: f32) -> kontra_dsp::delay::Settings {
    map_fields!(d => kontra_dsp::delay::Settings {
        ms: delay_ms(d.time, tempo).clamp(0.0, MAX_MS),
        body: d.body.clone(),
        shape: d.shape.clone(),
        high_shelf: d.high_shelf.filter(|b| b.gain_db != 0.0).map(|b| kontra_dsp::delay::Shelf {
            hz: b.hz,
            gain_db: b.gain_db,
            slope: b.q,
        }),
        lfo_hz: hz(d.rate, tempo);
        depth_ms, second_head, input, feedback, output, damping_hz, low_cut_hz, steep_low_cut,
        triangle_s, noise, spread_ms, phase, direct
    })
}

fn notches(n: ir::Notches, tempo: f32) -> NotchSettings {
    map_fields!(n => NotchSettings {
        lfo_hz: hz(n.rate, tempo),
        sign: match n.polarity {
            ir::Polarity::Normal => 1.0,
            ir::Polarity::Inverted => -1.0,
        };
        stages, hz, octaves, spread, spread_swing, spread_limit, feedback, loop_width, loop_top,
        rounding, depth, warp, phase, mix, low_cut_hz, internal_hz
    })
}

fn comb_bank(c: ir::CombBank, tempo: f32) -> CombSettings {
    use kontra_dsp::delay::ScanShape;
    let blend = |on: bool| if on { 1.0 } else { 0.0 };
    map_fields!(c => CombSettings {
        lfo_hz: hz(c.rate, tempo),
        polarity: match c.polarity {
            ir::Polarity::Normal => 1.0,
            ir::Polarity::Inverted => -1.0,
        },
        thru_zero: blend(c.mode == ir::CombMode::ThruZero),
        scan: blend(c.mode == ir::CombMode::Scan),
        scan_shape: match c.scan_shape {
            ir::ScanShape::Triangle => ScanShape::Triangle,
            ir::ScanShape::SawUp => ScanShape::SawUp,
            ir::ScanShape::SawDown => ScanShape::SawDown,
        };
        root_hz, ratio, detune, feedback, voices, damping, depth, offset, scan_depth, width, mix
    })
}

fn phaser(p: ir::Phaser, tempo: f32) -> PhaserSettings {
    map_fields!(p => PhaserSettings {
        lfo_hz: hz(p.rate, tempo);
        stages, triangle, hz, spread_hz, exponent, feedback, phase
    })
}

fn cascade(c: ir::Cascade) -> CascadeSettings {
    let stage = |s: ir::CascadeStage| map_fields!(s => CascadeStage { q, limit, peak, morph, wet });
    map_fields!(c => CascadeSettings { stages: c.stages.map(stage); hz, shifts, gain })
}

fn settings(r: &ir::Reverb) -> Settings {
    map_fields!(r => Settings {
        seconds, size, damping, diffusion, predelay_ms, high_cut_hz, low_shelf_db, stereo,
        modulation_ms, modulation_hz
    })
}

/// The processor slot `slot` runs. Not real-time.
pub fn processor(slot: Option<&ir::Effect>, rate: u32, impulses: &Responses<'_>) -> Dsp {
    match slot.map(|e| &e.kind) {
        Some(ir::EffectKind::Eq(bells)) => {
            Dsp::Bells(Box::new(filters::VoiceEq::new(*bells, rate)))
        }
        Some(ir::EffectKind::EqPass {
            bands,
            hp_hz,
            lp_hz,
        }) => Dsp::Bells(Box::new(filters::VoiceEq::with_pass(
            *bands, *hp_hz, *lp_hz, rate,
        ))),
        Some(ir::EffectKind::Filter(f)) => {
            let mut svf = Svf::new(filters::shape(f));
            svf.set_cutoff(f.hz, rate);
            svf.keep_spare();
            Dsp::Filter(Box::new(svf))
        }
        Some(ir::EffectKind::Formant(_)) => {
            let live = slot.map(|e| ir::Effect {
                bypass: false,
                ..e.clone()
            });
            let sections = filters::sections(&[live]);
            Dsp::Eq(filters::stages(&sections, rate))
        }
        Some(ir::EffectKind::Convolution { impulse }) => {
            match impulses.get(usize::from(*impulse)) {
                Some(r) => convolver(r),
                None => Dsp::None,
            }
        }
        Some(ir::EffectKind::Reverb(r)) => wet(DspUnit::Space(match &r.network {
            Some(ir::Network::Lines(law)) => Space::network(law, settings(r), rate),
            Some(ir::Network::Combs(law)) => Space::combs(law, settings(r), rate),
            Some(ir::Network::Hadamard(law)) => Space::hadamard(*law, settings(r), rate),
            Some(ir::Network::Plate) => Space::plate(settings(r), rate),
            None => Space::fdn(settings(r), rate),
        })),
        Some(ir::EffectKind::Compressor(c)) => wet(DspUnit::Dynamics(Dynamics::Compressor(
            Box::new(Compressor::new(compressor(*c), rate)),
            rate,
        ))),
        Some(ir::EffectKind::Limiter(l)) => wet(DspUnit::Dynamics(Dynamics::Limiter(
            Box::new(Limiter::new(limiter(*l), rate)),
            rate,
        ))),
        Some(ir::EffectKind::Transient(t)) => wet(DspUnit::Dynamics(Dynamics::Transient(
            Box::new(Transient::new(transient(*t), rate)),
        ))),
        Some(ir::EffectKind::Delay(d)) => {
            let max_ms = match d.time {
                ir::DelayTime::Ms(ms) if d.depth_ms > 0.0 => ms,
                _ => MAX_MS,
            };
            wet(DspUnit::ModDelay(Box::new(ModDelay::new(
                delay(d, 120.0),
                rate,
                max_ms,
            ))))
        }
        Some(ir::EffectKind::Shifter(s)) => wet(DspUnit::Shifter(Box::new(Shifter::new(
            shifter(*s, 120.0),
            rate,
        )))),
        Some(ir::EffectKind::Scanner(s)) => {
            wet(DspUnit::Scanner(Box::new(Scanner::new(scanner(*s), rate))))
        }
        Some(ir::EffectKind::Phaser(p)) => wet(DspUnit::Phaser(Box::new(Phaser::new(
            phaser(*p, 120.0),
            rate,
        )))),
        Some(ir::EffectKind::Notches(n)) => wet(DspUnit::Notches(Box::new(Notches::new(
            notches(*n, 120.0),
            rate,
        )))),
        Some(ir::EffectKind::Ensemble(e)) => {
            wet(DspUnit::Ensemble(Box::new(Ensemble::new(e, rate))))
        }
        Some(ir::EffectKind::CombBank(c)) => wet(DspUnit::CombBank(Box::new(CombBank::new(
            comb_bank(*c, 120.0),
            rate,
            c.lowest_hz,
        )))),
        Some(ir::EffectKind::Cascade(c)) => {
            wet(DspUnit::Cascade(Box::new(Cascade::new(cascade(*c), rate))))
        }
        Some(ir::EffectKind::Clocked(c)) => {
            let shape = match c.shape {
                ir::ClockedShape::Allpass => ClockedShape::Allpass,
                ir::ClockedShape::VowelLow => ClockedShape::VowelLow,
                ir::ClockedShape::VowelHigh => ClockedShape::VowelHigh,
            };
            wet(DspUnit::Clocked(Box::new(Clocked::new(
                shape,
                [c.cutoff, c.resonance],
                rate,
            ))))
        }
        Some(ir::EffectKind::Shaper(s)) => wet(DspUnit::Drive(Drive::Shaper(
            Box::new(shaper(s, rate)),
            rate,
        ))),
        Some(ir::EffectKind::Strip(s)) => wet(DspUnit::Strip(
            Box::new(Compressor::new(compressor(s.compressor), rate)),
            Box::new(shaper(&s.shaper, rate)),
            rate,
        )),
        Some(ir::EffectKind::Overdrive(o)) => wet(DspUnit::Drive(Drive::Overdrive(
            Box::new(Overdrive::new(voicing(o), rate)),
            rate,
        ))),
        Some(ir::EffectKind::Crush(c)) => wet(DspUnit::Drive(Drive::Crush(
            Box::new(Crush::new(reduction(*c, rate), 1)),
            rate,
        ))),
        Some(ir::EffectKind::Clipper(c)) => {
            wet(DspUnit::Drive(Drive::Clipper(Box::new(Clipper::new(
                map_fields!(c => Circuit {
                    drive, limit, tone_hz, coupling_hz, saturation, slope, asymmetry, dc_hz
                }),
                rate,
            )))))
        }
        Some(ir::EffectKind::RingMod(r)) => wet(DspUnit::Ring(
            Box::new(Ring::new(ring(*r, 120.0), rate)),
            rate,
        )),
        Some(ir::EffectKind::Rotary(r)) => {
            wet(DspUnit::Rotary(Box::new(Rotary::new(rotary(*r), rate))))
        }
        Some(ir::EffectKind::Gater(g)) => {
            wet(DspUnit::Gater(Box::new(Gater::new(gater(*g, 120.0), rate))))
        }
        Some(ir::EffectKind::BeatMasher(m)) => wet(DspUnit::BeatMasher(Box::new(BeatMasher::new(
            beat_masher(*m, 120.0),
            rate,
        )))),
        Some(ir::EffectKind::BeatSlicer(s)) => wet(DspUnit::BeatSlicer(Box::new(BeatSlicer::new(
            beat_slicer(*s, 120.0),
            rate,
        )))),
        Some(ir::EffectKind::ReverseGrain(r)) => wet(DspUnit::ReverseGrain(Box::new(
            ReverseGrain::new(reverse_grain(*r), rate),
        ))),
        Some(ir::EffectKind::TransposeStretch(t)) => wet(DspUnit::TransposeStretch(Box::new(
            TransposeStretch::new(transpose_stretch(*t, 120.0), rate),
        ))),
        Some(ir::EffectKind::DualDelay(d)) => wet(DspUnit::DualDelay(Box::new(DualDelay::new(
            dual_delay(*d, 120.0),
            rate,
            DUAL_MAX_MS,
        )))),
        Some(ir::EffectKind::StereoDetune(s)) => wet(DspUnit::StereoDetune(Box::new(
            StereoDetune::new(stereo_detune(*s), rate),
        ))),
        Some(ir::EffectKind::Wah(w)) => wet(DspUnit::Wah(Box::new(Wah::new(wah(*w), rate)))),
        Some(ir::EffectKind::AmpStack(a)) => wet(DspUnit::Drive(Drive::AmpStack(Box::new(
            AmpStack::new(amp_stack(*a), rate),
        )))),
        Some(ir::EffectKind::Cabinet(c)) => wet(DspUnit::Drive(Drive::Cabinet(Box::new(
            Cabinet::new(cabinet(*c), rate),
        )))),
        Some(ir::EffectKind::ColorSat(c)) => wet(DspUnit::Drive(Drive::ColorSat(Box::new(
            ColorSat::new(color_sat(*c), rate),
        )))),
        Some(ir::EffectKind::SpectralMorph(m)) => wet(DspUnit::SpectralMorph(Box::new(
            SpectralMorph::new(spectral_morph(*m), rate),
        ))),
        _ => Dsp::None,
    }
}

pub fn apply(m: Gain, io: &mut [[f32; 2]]) {
    if m == effects::IDENTITY {
        return;
    }
    let [[ll, lr], [rl, rr]] = m;
    for f in io {
        let [l, r] = *f;
        *f = [ll * l + lr * r, rl * l + rr * r];
    }
}

/// The waveshaper for `s` at `rate`, in a group rack or a bus rack.
pub fn shaper(s: &ir::Shaper, rate: u32) -> Shaper {
    Shaper::new(curve(&s.curve), tone(s), rate)
}

/// The filters around `s`'s curve.
pub fn tone(s: &ir::Shaper) -> drive::Tone {
    map_fields!(s => drive::Tone { pre_lowpass_hz, lowpass_hz, highpass_hz, sections })
}

pub fn curve(c: &ir::DriveCurve) -> drive::Curve {
    match c {
        ir::DriveCurve::Soft(x) => drive::Curve::Soft(*x),
        ir::DriveCurve::Power(x) => drive::Curve::Power(*x),
        ir::DriveCurve::Sine(x) => drive::Curve::Sine(*x),
        ir::DriveCurve::Fold(x) => drive::Curve::Fold(*x),
        ir::DriveCurve::Tanh(x) => drive::Curve::Tanh(*x),
        ir::DriveCurve::Atan(x) => drive::Curve::Atan(*x),
        ir::DriveCurve::Saturate(x) => drive::Curve::Saturate(*x),
        ir::DriveCurve::Table { drive, points } => drive::Curve::Table {
            drive: *drive,
            points: points.clone(),
        },
        ir::DriveCurve::Blend { drive, rows, t } => drive::Curve::Blend {
            drive: *drive,
            rows: rows.clone(),
            t: *t,
        },
        ir::DriveCurve::AsymBlend { rows, t } => drive::Curve::AsymBlend {
            rows: rows.clone(),
            t: *t,
        },
    }
}

/// Multiband overdrive settings in DSP terms.
pub fn voicing(o: &ir::Overdrive) -> Voicing {
    map_fields!(o => Voicing { curve: o.curve.clone(); split_hz, smooth_hz, drive, mix, level })
}

/// Sample-and-bit reduction settings in DSP terms: sample hold, bits and noise.
pub fn reduction(c: ir::Crush, rate: u32) -> Reduction {
    let hold = if c.rate < 1.0 {
        (1.0 / c.rate.max(1e-6)).min(rate as f32 / 50.0) + 1.0
    } else {
        1.0
    };
    Reduction {
        hold,
        bits: (c.bits < 16.0).then_some(c.bits.max(1.0)),
        noise: c.noise,
        noise_tone: c.noise_tone,
        converter: c.converter.map(|v| {
            map_fields!(v => Converter {
                clock_hz, jitter, pre_hz, post_hz, highpass_hz, step, mu, midrise,
                dither, drive_in, drive_out, soft, trim, wet, dry
            })
        }),
        rate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_effects_run_on_summed_audio() {
        let o = ir::Overdrive {
            split_hz: [700.0, 2000.0, 4000.0],
            smooth_hz: [8000.0, 6000.0, 3000.0],
            drive: 4.0,
            mix: 1.0,
            level: 1.0,
            curve: std::borrow::Cow::Owned((0..256).map(|i| (i as f32 - 128.0) / 64.0).collect()),
        };
        let c = ir::Crush {
            rate: 0.25,
            bits: 8.0,
            noise: 0.0,
            noise_tone: 0.0,
            converter: None,
        };
        for kind in [ir::EffectKind::Overdrive(o), ir::EffectKind::Crush(c)] {
            let slot = ir::Effect {
                kind: kind.clone(),
                bypass: false,
                output: 1.0,
                dry: 0.0,
            };
            let Dsp::Wet(mut unit, _) = processor(Some(&slot), 48_000, &Responses(&[], &[])) else {
                panic!("{:?} has no bus processor", slot.kind);
            };
            let mut io: Vec<[f32; 2]> = (0..256)
                .map(|n| [(n as f32 * 0.05).sin() * 0.8; 2])
                .collect();
            let dry = io.clone();
            unit.process(&mut io);
            assert_ne!(io, dry, "{kind:?}");
        }
    }
}
