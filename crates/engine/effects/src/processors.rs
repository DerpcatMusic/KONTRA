//! Each slot's processor from its IR effect: the wet processors (reverbs,
//! delays, dynamics, drives, a cascade) and the summed EQ and filter stages,
//! with their settings in DSP terms. Not real-time unless a function says so.

use kontra_dsp::delay::{MAX_MS, ModDelay, Phaser, PhaserSettings};
use kontra_dsp::drive::{self, Circuit, Clipper, Crush, Overdrive, Reduction, Shaper, Voicing};
use kontra_dsp::dynamics::{
    Compressor, CompressorSettings, Detector, Limiter, LimiterSettings, Transient,
    TransientSettings,
};
use kontra_dsp::filter::{Cascade, CascadeSettings, CascadeStage, Clocked, ClockedShape, Svf};
use kontra_dsp::reverb::{Combs, Convolver, Fdn, Hadamard, Network, Settings};
use kontra_ir as ir;
use kontra_voice::Gain;

use crate::{MAX_SPAN, Response, matrix as effects, summed as filters};

pub enum Dsp {
    None,
    Eq(Box<[filters::Stage]>),
    /// An EQ routes move: all its bands, retuned each span.
    Bells(Box<filters::VoiceEq>),
    /// A filter, following its slot each span (a script may move or
    /// reshape it).
    Filter(Box<Svf>),
    /// A wet path (convolver or reverb) and a span of its output.
    Wet(Box<dyn Wet>, Box<[[f32; 2]]>),
}

/// A processor whose span replaces the slot's output; the slot's dry level
/// mixes the input back.
pub trait Wet: Send {
    fn process(&mut self, io: &mut [[f32; 2]]);
    /// The slot's settings as scripts left them, at the host's tempo,
    /// before each block. Never allocates.
    fn set(&mut self, _kind: ir::EffectKind, _tempo: f32) {}
    /// Frames after which silent input leaves it silent, once nothing
    /// louder than silence went into its state for as long; `None`: it
    /// always runs.
    fn memory(&self) -> Option<usize> {
        None
    }
    /// The loudest value the last [`Wet::process`] wrote into its state
    /// (its lines, rings and filters), heard or not: a muted output may
    /// hide a tail a later setting lets out.
    fn written(&self) -> f32 {
        0.0
    }
    /// `silence.len()` silent frames pass unprocessed: its clocks (an LFO,
    /// a glide) move on as [`Wet::process`] would move them. `silence` is
    /// scratch. Only called on processors with a [`Wet::memory`].
    fn idle(&mut self, _silence: &mut [[f32; 2]]) {}
}

impl Wet for Convolver {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Convolver::process(self, io);
    }

    // No clocks and no feedback: its state is its last `taps` inputs (and
    // the sums made of them), so silent input alone settles it; the slot's
    // output gain sits after it, in the rack. `written` stays 0.
    fn memory(&self) -> Option<usize> {
        Some(self.taps())
    }
}

impl Wet for Compressor {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Compressor::process(self, io);
    }
}

impl Wet for Limiter {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Limiter::process(self, io);
    }
}

impl Wet for Transient {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Transient::process(self, io);
    }
}

impl Wet for Cascade {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Cascade::process(self, io);
    }

    fn set(&mut self, kind: ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Cascade(c) = kind {
            Cascade::set(self, cascade(c));
        }
    }
}

impl Wet for Clocked {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Clocked::process(self, io);
    }

    fn set(&mut self, kind: ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Clocked(c) = kind {
            Clocked::set(self, [c.cutoff, c.resonance]);
        }
    }
}

impl Wet for Fdn {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Fdn::process(self, io);
    }

    fn set(&mut self, kind: ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Reverb(r) = kind {
            Fdn::set(self, settings(r));
        }
    }
}

impl Wet for Combs {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Combs::process(self, io);
    }

    fn set(&mut self, kind: ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Reverb(r) = kind {
            Combs::set(self, settings(r));
        }
    }
}

impl Wet for Hadamard {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Hadamard::process(self, io);
    }

    fn set(&mut self, kind: ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Reverb(r) = kind {
            Hadamard::set(self, settings(r));
        }
    }
}

impl Wet for Network {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Network::process(self, io);
    }

    fn memory(&self) -> Option<usize> {
        Some(Network::memory(self))
    }

    fn written(&self) -> f32 {
        Network::written(self)
    }

    fn idle(&mut self, silence: &mut [[f32; 2]]) {
        Network::idle(self, silence);
    }

    fn set(&mut self, kind: ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Reverb(r) = kind {
            Network::set(self, settings(r));
        }
    }
}

impl Wet for ModDelay {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        ModDelay::process(self, io);
    }

    fn memory(&self) -> Option<usize> {
        Some(ModDelay::memory(self))
    }

    fn written(&self) -> f32 {
        ModDelay::written(self)
    }

    fn idle(&mut self, silence: &mut [[f32; 2]]) {
        ModDelay::idle(self, silence.len());
    }

    fn set(&mut self, kind: ir::EffectKind, tempo: f32) {
        if let ir::EffectKind::Delay(d) = kind {
            ModDelay::set(self, delay(d, tempo));
        }
    }
}

impl Wet for Phaser {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Phaser::process(self, io);
    }

    fn set(&mut self, kind: ir::EffectKind, tempo: f32) {
        if let ir::EffectKind::Phaser(p) = kind {
            Phaser::set(self, phaser(p, tempo));
        }
    }
}

impl Wet for Shaper {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Shaper::process(self, io);
    }

    fn set(&mut self, kind: ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Shaper(s) = kind {
            self.set_curve(curve(s.curve));
        }
    }
}

/// A drive unit on summed audio and the rate its settings retune at.
struct Rated<T>(T, u32);

impl Wet for Rated<Overdrive> {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        self.0.process(io);
    }

    fn set(&mut self, kind: ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Overdrive(o) = kind {
            self.0.set(voicing(o), self.1);
        }
    }
}

impl Wet for Rated<Crush> {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        self.0.process(io);
    }

    fn set(&mut self, kind: ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Crush(c) = kind {
            self.0.set(reduction(c, self.1));
        }
    }
}

impl Wet for Clipper {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Clipper::process(self, io);
    }
}

/// A convolution playing `r`. Not real-time.
pub fn convolver(r: &Response) -> Dsp {
    wet(Box::new(Convolver::new(&r.left, &r.right)))
}

fn wet(processor: Box<dyn Wet>) -> Dsp {
    Dsp::Wet(processor, vec![[0.0; 2]; MAX_SPAN as usize].into())
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

fn compressor(c: ir::Compressor) -> CompressorSettings {
    CompressorSettings {
        threshold_db: c.threshold_db,
        ratio: c.ratio,
        knee_db: c.knee_db,
        attack_ms: c.attack_ms,
        release_ms: c.release_ms,
        makeup_db: c.makeup_db,
        link: c.link,
        mix: c.mix,
        detector: match c.detector {
            ir::Detector::Input => Detector::Input,
            ir::Detector::Output => Detector::Output,
            ir::Detector::Trigger => Detector::Trigger,
        },
    }
}

fn limiter(l: ir::Limiter) -> LimiterSettings {
    LimiterSettings {
        input_db: l.input_db,
        ceiling_db: l.ceiling_db,
        release_ms: l.release_ms,
    }
}

/// LFO cycles per second at `tempo`.
fn hz(rate: ir::Rate, tempo: f32) -> f32 {
    match rate {
        ir::Rate::Hz(hz) => hz,
        ir::Rate::Beats(beats) => tempo / 60.0 / beats.max(1e-3),
        ir::Rate::Steps { beats, count } => tempo / 60.0 / (beats * count).max(1e-3),
    }
}

fn delay(d: ir::Delay, tempo: f32) -> kontra_dsp::delay::Settings {
    kontra_dsp::delay::Settings {
        ms: match d.time {
            ir::DelayTime::Ms(ms) => ms,
            ir::DelayTime::Beats(beats) => beats * 60_000.0 / tempo.max(1.0),
        }
        .clamp(0.0, MAX_MS),
        depth_ms: d.depth_ms,
        second_head: d.second_head,
        input: d.input,
        feedback: d.feedback,
        output: d.output,
        damping_hz: d.damping_hz,
        low_cut_hz: d.low_cut_hz,
        steep_low_cut: d.steep_low_cut,
        high_shelf: d
            .high_shelf
            .filter(|b| b.gain_db != 0.0)
            .map(|b| kontra_dsp::delay::Shelf {
                hz: b.hz,
                gain_db: b.gain_db,
                slope: b.q,
            }),
        spread_ms: d.spread_ms,
        lfo_hz: hz(d.rate, tempo),
        phase: d.phase,
    }
}

fn phaser(p: ir::Phaser, tempo: f32) -> PhaserSettings {
    PhaserSettings {
        stages: p.stages,
        triangle: p.triangle,
        hz: p.hz,
        spread_hz: p.spread_hz,
        exponent: p.exponent,
        feedback: p.feedback,
        lfo_hz: hz(p.rate, tempo),
        phase: p.phase,
    }
}

fn cascade(c: ir::Cascade) -> CascadeSettings {
    let stage = |s: ir::CascadeStage| CascadeStage {
        q: s.q,
        limit: s.limit,
        peak: s.peak,
        morph: s.morph,
        wet: s.wet,
    };
    CascadeSettings {
        hz: c.hz,
        shifts: c.shifts,
        stages: c.stages.map(stage),
        gain: c.gain,
    }
}

fn settings(r: ir::Reverb) -> Settings {
    Settings {
        seconds: r.seconds,
        size: r.size,
        damping: r.damping,
        diffusion: r.diffusion,
        predelay_ms: r.predelay_ms,
        high_cut_hz: r.high_cut_hz,
        low_shelf_db: r.low_shelf_db,
        stereo: r.stereo,
        modulation_ms: r.modulation_ms,
        modulation_hz: r.modulation_hz,
    }
}

/// The processor slot `slot` runs; `moved` when routes move it. Not real-time.
pub fn processor(
    slot: Option<ir::Effect>,
    moved: bool,
    rate: u32,
    impulses: &Responses<'_>,
) -> Dsp {
    match slot.map(|e| e.kind) {
        Some(ir::EffectKind::Eq(bells)) if moved => {
            Dsp::Bells(Box::new(filters::VoiceEq::new(bells, rate)))
        }
        Some(ir::EffectKind::Filter(f)) => {
            let mut svf = Svf::new(filters::shape(&f));
            svf.set_cutoff(f.hz, rate);
            // A script may switch it to any ladder on the audio thread.
            svf.keep_spare();
            Dsp::Filter(Box::new(svf))
        }
        // Built even while bypassed: a script may enable it later.
        Some(ir::EffectKind::Eq(_) | ir::EffectKind::Formant(_)) => {
            let live = slot.map(|e| ir::Effect { bypass: false, ..e });
            let sections = filters::sections(&[live]);
            Dsp::Eq(filters::stages(&sections, rate))
        }
        Some(ir::EffectKind::Convolution { impulse }) => match impulses.get(usize::from(impulse)) {
            Some(r) => convolver(r),
            None => Dsp::None,
        },
        Some(ir::EffectKind::Reverb(r)) => match r.network {
            Some(ir::Network::Lines(law)) => wet(Box::new(Network::new(law, settings(r), rate))),
            Some(ir::Network::Combs(law)) => wet(Box::new(Combs::new(law, settings(r), rate))),
            Some(ir::Network::Hadamard(law)) => {
                wet(Box::new(Hadamard::new(law, settings(r), rate)))
            }
            None => wet(Box::new(Fdn::new(settings(r), rate))),
        },
        Some(ir::EffectKind::Compressor(c)) => wet(Box::new(Compressor::new(compressor(c), rate))),
        Some(ir::EffectKind::Limiter(l)) => wet(Box::new(Limiter::new(limiter(l), rate))),
        // ponytail: an echo gets the longest line, so a script or a
        // tempo can lengthen it; a swept line only what it needs.
        Some(ir::EffectKind::Delay(d)) => {
            let max_ms = match d.time {
                ir::DelayTime::Ms(ms) if d.depth_ms > 0.0 => ms,
                _ => MAX_MS,
            };
            wet(Box::new(ModDelay::new(delay(d, 120.0), rate, max_ms)))
        }
        Some(ir::EffectKind::Phaser(p)) => wet(Box::new(Phaser::new(phaser(p, 120.0), rate))),
        // ponytail: lines for 120 ms, the longest a Kontakt Choral reaches
        // (66 ms base + its widest swing at the slowest rate).
        Some(ir::EffectKind::Ensemble(e)) => {
            wet(Box::new(crate::ensemble::Ensemble::new(e, rate, 120.0)))
        }
        Some(ir::EffectKind::Cascade(c)) => wet(Box::new(Cascade::new(cascade(c), rate))),
        Some(ir::EffectKind::Clocked(c)) => {
            let shape = match c.shape {
                ir::ClockedShape::Allpass => ClockedShape::Allpass,
                ir::ClockedShape::VowelLow => ClockedShape::VowelLow,
                ir::ClockedShape::VowelHigh => ClockedShape::VowelHigh,
            };
            wet(Box::new(Clocked::new(shape, [c.cutoff, c.resonance], rate)))
        }
        // Drive effects run in any rack: on one voice in a group rack, on
        // the sum in a bus or instrument rack.
        Some(ir::EffectKind::Shaper(s)) => wet(Box::new(shaper(s, rate))),
        Some(ir::EffectKind::Overdrive(o)) => {
            wet(Box::new(Rated(Overdrive::new(voicing(o), rate), rate)))
        }
        Some(ir::EffectKind::Crush(c)) => {
            wet(Box::new(Rated(Crush::new(reduction(c, rate), 1), rate)))
        }
        Some(ir::EffectKind::Clipper(c)) => wet(Box::new(Clipper::new(
            Circuit {
                drive: c.drive,
                limit: c.limit,
                tone_hz: c.tone_hz,
                coupling_hz: c.coupling_hz,
                saturation: c.saturation,
                slope: c.slope,
                asymmetry: c.asymmetry,
                dc_hz: c.dc_hz,
            },
            rate,
        ))),
        Some(ir::EffectKind::Transient(t)) => wet(Box::new(Transient::new(
            TransientSettings {
                attack: t.attack,
                sustain: t.sustain,
                gain_db: t.gain_db,
                clip: t.clip,
            },
            rate,
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
pub fn shaper(s: ir::Shaper, rate: u32) -> Shaper {
    let tone = drive::Tone {
        pre_lowpass_hz: s.pre_lowpass_hz,
        lowpass_hz: s.lowpass_hz,
        highpass_hz: s.highpass_hz,
    };
    Shaper::new(curve(s.curve), tone, rate)
}

pub fn curve(c: ir::DriveCurve) -> drive::Curve {
    match c {
        ir::DriveCurve::Soft(x) => drive::Curve::Soft(x),
        ir::DriveCurve::Tube(x) => drive::Curve::Tube(x),
        ir::DriveCurve::Power(x) => drive::Curve::Power(x),
        ir::DriveCurve::Sine(x) => drive::Curve::Sine(x),
        ir::DriveCurve::Fold(x) => drive::Curve::Fold(x),
        ir::DriveCurve::Tanh(x) => drive::Curve::Tanh(x),
        ir::DriveCurve::Atan(x) => drive::Curve::Atan(x),
    }
}

/// Multiband overdrive settings in DSP terms.
pub fn voicing(o: ir::Overdrive) -> Voicing {
    Voicing {
        split_hz: o.split_hz,
        smooth_hz: o.smooth_hz,
        drive: o.drive,
        mix: o.mix,
        level: o.level,
        curve: o.curve,
    }
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
            curve: Box::leak(Box::new(std::array::from_fn(|i| (i as f32 - 128.0) / 64.0))),
        };
        let c = ir::Crush {
            rate: 0.25,
            bits: 8.0,
            noise: 0.0,
            noise_tone: 0.0,
        };
        for kind in [ir::EffectKind::Overdrive(o), ir::EffectKind::Crush(c)] {
            let slot = ir::Effect {
                kind,
                bypass: false,
                output: 1.0,
                dry: 0.0,
            };
            let Dsp::Wet(mut unit, _) = processor(Some(slot), false, 48_000, &Responses(&[], &[]))
            else {
                panic!("{kind:?} has no bus processor");
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
