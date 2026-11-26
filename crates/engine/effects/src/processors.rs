//! Each slot's processor from its IR effect: the wet processors (reverbs,
//! delays, dynamics, drives, a cascade) and the summed EQ and filter stages,
//! with their settings in DSP terms. Not real-time unless a function says so.

use kontra_dsp::delay::{
    BeatMasher, BeatMasherSettings, BeatSlicer, BeatSlicerSettings, CombBank, CombSettings, Gater,
    GaterSettings, MAX_MS, ModDelay, Phaser, PhaserSettings, ReverseGrain, ReverseGrainSettings,
    Scanner, ScannerSettings, Shifter, ShifterSettings, TransposeStretch, TransposeStretchSettings,
};
use kontra_dsp::drive::{
    self, Circuit, Clipper, Converter, Crush, Overdrive, Reduction, Shaper, Voicing,
};
use kontra_dsp::dynamics::{
    Compressor, CompressorSettings, Detector, Limiter, LimiterSettings, Transient,
    TransientSettings,
};
use kontra_dsp::filter::{
    Cascade, CascadeSettings, CascadeStage, Clocked, ClockedShape, NotchSettings, Notches, Svf,
};
use kontra_dsp::reverb::{Combs, Convolver, Fdn, Hadamard, Network, Settings};
use kontra_dsp::spatial::{Ring, RingMod, RingSource, Rotary, RotarySettings};
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
    /// Processes `io` using an external sidechain `key` signal for detection.
    /// Non-sidechain processors ignore `key` and run [`Wet::process`].
    fn process_sidechain(&mut self, io: &mut [[f32; 2]], _key: &[[f32; 2]]) {
        self.process(io);
    }
    /// The slot's settings as scripts left them, at the host's tempo,
    /// before each block. Never allocates.
    fn set(&mut self, _kind: &ir::EffectKind, _tempo: f32) {}
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
    /// Whether its input gains are all 0, so its input reaches neither its
    /// state nor its output: the slot hears it as silence.
    fn deaf(&self) -> bool {
        false
    }
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

// Dynamics follow their slot each span: a script may move any setting
// (Una Corda's compressor amount and transient knobs).
impl Wet for Rated<Compressor> {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        self.0.process(io);
    }

    fn process_sidechain(&mut self, io: &mut [[f32; 2]], key: &[[f32; 2]]) {
        self.0.process_sidechain(io, key);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Compressor(c) = kind {
            self.0.set(compressor(*c), self.1);
        }
    }
}

impl Wet for Rated<Limiter> {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        self.0.process(io);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Limiter(l) = kind {
            self.0.set(limiter(*l), self.1);
        }
    }
}

impl Wet for Transient {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Transient::process(self, io);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Transient(t) = kind {
            Transient::set(self, transient(*t));
        }
    }
}

impl Wet for Cascade {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Cascade::process(self, io);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Cascade(c) = kind {
            Cascade::set(self, cascade(*c));
        }
    }
}

impl Wet for Clocked {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Clocked::process(self, io);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Clocked(c) = kind {
            Clocked::set(self, [c.cutoff, c.resonance]);
        }
    }
}

impl Wet for Fdn {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Fdn::process(self, io);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Reverb(r) = kind {
            Fdn::set(self, settings(r));
        }
    }
}

impl Wet for Combs {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Combs::process(self, io);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Reverb(r) = kind {
            Combs::set(self, settings(r));
        }
    }
}

impl Wet for Hadamard {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Hadamard::process(self, io);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
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

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
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

    fn deaf(&self) -> bool {
        ModDelay::deaf(self)
    }

    fn set(&mut self, kind: &ir::EffectKind, tempo: f32) {
        if let ir::EffectKind::Delay(d) = kind {
            ModDelay::set(self, delay(d, tempo));
        }
    }
}

impl Wet for Scanner {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Scanner::process(self, io);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Scanner(s) = kind {
            Scanner::set(self, scanner(*s));
        }
    }
}

fn scanner(s: ir::Scanner) -> ScannerSettings {
    ScannerSettings {
        sections: usize::from(s.sections),
        henry: s.henry,
        farad: s.farad,
        last_farad: s.last_farad,
        load_ohm: s.load_ohm,
        shunt_ohm: s.shunt_ohm,
        taps: s.taps,
        hz: s.hz,
        lead: s.lead,
        split_hz: s.split_hz,
        high: s.high,
        blend: s.blend,
        mix: s.mix,
    }
}

impl Wet for Shifter {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Shifter::process(self, io);
    }

    fn memory(&self) -> Option<usize> {
        Some(Shifter::memory(self))
    }

    fn written(&self) -> f32 {
        Shifter::written(self)
    }

    fn idle(&mut self, silence: &mut [[f32; 2]]) {
        Shifter::idle(self, silence);
    }

    fn set(&mut self, kind: &ir::EffectKind, tempo: f32) {
        if let ir::EffectKind::Shifter(s) = kind {
            Shifter::set(self, shifter(*s, tempo));
        }
    }
}

impl Wet for CombBank {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        CombBank::process(self, io);
    }

    fn memory(&self) -> Option<usize> {
        Some(CombBank::memory(self))
    }

    fn written(&self) -> f32 {
        CombBank::written(self)
    }

    fn idle(&mut self, silence: &mut [[f32; 2]]) {
        CombBank::idle(self, silence.len());
    }

    fn set(&mut self, kind: &ir::EffectKind, tempo: f32) {
        if let ir::EffectKind::CombBank(c) = kind {
            CombBank::set(self, comb_bank(*c, tempo));
        }
    }
}

impl Wet for Phaser {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Phaser::process(self, io);
    }

    fn set(&mut self, kind: &ir::EffectKind, tempo: f32) {
        if let ir::EffectKind::Phaser(p) = kind {
            Phaser::set(self, phaser(*p, tempo));
        }
    }
}

impl Wet for Notches {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Notches::process(self, io);
    }

    fn memory(&self) -> Option<usize> {
        Some(Notches::memory(self))
    }

    fn written(&self) -> f32 {
        Notches::written(self)
    }

    fn idle(&mut self, silence: &mut [[f32; 2]]) {
        Notches::idle(self, silence.len());
    }

    fn set(&mut self, kind: &ir::EffectKind, tempo: f32) {
        if let ir::EffectKind::Notches(n) = kind {
            Notches::set(self, notches(*n, tempo));
        }
    }
}

impl Wet for Rated<Shaper> {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        self.0.process(io);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        // A strip's compressor is lost on a slot built without one.
        if let ir::EffectKind::Shaper(s) | ir::EffectKind::Strip(ir::Strip { shaper: s, .. }) = kind
        {
            self.0.retune(curve(&s.curve), tone(s), self.1);
        }
    }

    // A memoryless curve between filters whose slowest pole (an 8 Hz
    // one-pole high-pass) is down 120 dB after 0.28 s: silent input for a
    // third of a second leaves it silent.
    fn memory(&self) -> Option<usize> {
        Some(self.1 as usize / 3)
    }
}

/// [`ir::Strip`]: a compressor into a waveshaper, and the rate both retune
/// at. A slot built as one takes a plain shaper's settings too (its
/// compressor then passes), so a script may move either way.
struct Strip(Compressor, Shaper, u32);

impl Wet for Strip {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        self.0.process(io);
        self.1.process(io);
    }

    fn process_sidechain(&mut self, io: &mut [[f32; 2]], key: &[[f32; 2]]) {
        self.0.process_sidechain(io, key);
        self.1.process(io);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        let (c, s) = match kind {
            ir::EffectKind::Strip(ir::Strip { compressor, shaper }) => (*compressor, shaper),
            ir::EffectKind::Shaper(s) => (PASS, s),
            _ => return,
        };
        self.0.set(compressor(c), self.2);
        self.1.retune(curve(&s.curve), tone(s), self.2);
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

/// A drive unit on summed audio and the rate its settings retune at.
struct Rated<T>(T, u32);

impl Wet for Rated<Overdrive> {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        self.0.process(io);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Overdrive(o) = kind {
            self.0.set(voicing(o), self.1);
        }
    }
}

impl Wet for Rated<Crush> {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        self.0.process(io);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Crush(c) = kind {
            self.0.set(reduction(*c, self.1));
        }
    }
}

impl Wet for Clipper {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Clipper::process(self, io);
    }
}

impl Wet for Rated<Ring> {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        self.0.process(io);
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::RingMod(r) = kind {
            self.0.set(ring(*r), self.1);
        }
    }
}

impl Wet for Rotary {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Rotary::process(self, io);
    }

    fn memory(&self) -> Option<usize> {
        Some(Rotary::memory(self))
    }

    fn written(&self) -> f32 {
        Rotary::written(self)
    }

    fn idle(&mut self, silence: &mut [[f32; 2]]) {
        Rotary::idle(self, silence.len());
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::Rotary(r) = kind {
            Rotary::set(self, rotary(*r));
        }
    }
}

fn rotary(r: ir::Rotary) -> RotarySettings {
    RotarySettings {
        fast: r.fast,
        horn_slow_hz: r.horn_slow_hz,
        horn_fast_hz: r.horn_fast_hz,
        drum_slow_hz: r.drum_slow_hz,
        drum_fast_hz: r.drum_fast_hz,
        accel_hi: r.accel_hi,
        accel_lo: r.accel_lo,
        crossover_hz: r.crossover_hz,
        balance: r.balance,
        distance: r.distance,
        mix: r.mix,
    }
}

impl Wet for Gater {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Gater::process(self, io);
    }

    fn written(&self) -> f32 {
        Gater::written(self)
    }

    fn idle(&mut self, silence: &mut [[f32; 2]]) {
        Gater::idle(self, silence.len());
    }

    fn set(&mut self, kind: &ir::EffectKind, tempo: f32) {
        if let ir::EffectKind::Gater(g) = kind {
            Gater::set(self, gater(*g, tempo));
        }
    }
}

fn gater(g: ir::Gater, tempo: f32) -> GaterSettings {
    GaterSettings {
        rate_hz: hz(g.rate, tempo),
        shape: g.shape,
        noise: g.noise,
        mix: g.mix,
        stutter: g.stutter,
        mute_input: g.mute_input,
        active: g.active,
    }
}

impl Wet for BeatMasher {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        BeatMasher::process(self, io);
    }

    fn memory(&self) -> Option<usize> {
        Some(BeatMasher::memory(self))
    }

    fn written(&self) -> f32 {
        BeatMasher::written(self)
    }

    fn idle(&mut self, silence: &mut [[f32; 2]]) {
        BeatMasher::idle(self, silence.len());
    }

    fn set(&mut self, kind: &ir::EffectKind, tempo: f32) {
        if let ir::EffectKind::BeatMasher(m) = kind {
            BeatMasher::set(self, beat_masher(*m, tempo));
        }
    }
}

fn beat_masher(m: ir::BeatMasher, tempo: f32) -> BeatMasherSettings {
    BeatMasherSettings {
        tempo_bpm: tempo,
        length_beats: m.length_beats,
        rotate: m.rotate,
        gate: m.gate,
        mix: m.mix,
        reverse: m.reverse,
        wrap: m.wrap,
        mash: m.mash,
    }
}

impl Wet for BeatSlicer {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        BeatSlicer::process(self, io);
    }

    fn memory(&self) -> Option<usize> {
        Some(BeatSlicer::memory(self))
    }

    fn written(&self) -> f32 {
        BeatSlicer::written(self)
    }

    fn idle(&mut self, silence: &mut [[f32; 2]]) {
        BeatSlicer::idle(self, silence.len());
    }

    fn set(&mut self, kind: &ir::EffectKind, tempo: f32) {
        if let ir::EffectKind::BeatSlicer(s) = kind {
            BeatSlicer::set(self, beat_slicer(*s, tempo));
        }
    }
}

fn beat_slicer(s: ir::BeatSlicer, tempo: f32) -> BeatSlicerSettings {
    BeatSlicerSettings {
        tempo_bpm: tempo,
        bars: s.bars,
        pattern: s.pattern,
        style: s.style,
        buzz: s.buzz,
        gate: s.gate,
        slice: s.slice,
        mix: s.mix,
    }
}

impl Wet for ReverseGrain {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        ReverseGrain::process(self, io);
    }

    fn memory(&self) -> Option<usize> {
        Some(ReverseGrain::memory(self))
    }

    fn written(&self) -> f32 {
        ReverseGrain::written(self)
    }

    fn idle(&mut self, silence: &mut [[f32; 2]]) {
        ReverseGrain::idle(self, silence.len());
    }

    fn set(&mut self, kind: &ir::EffectKind, _tempo: f32) {
        if let ir::EffectKind::ReverseGrain(r) = kind {
            ReverseGrain::set(self, reverse_grain(*r));
        }
    }
}

fn reverse_grain(r: ir::ReverseGrain) -> ReverseGrainSettings {
    ReverseGrainSettings {
        speed: r.speed,
        grain_ms: r.grain_ms,
        pitch_ratio: r.pitch_ratio,
        mix: r.mix,
        forward: r.forward,
        invert_grains: r.invert_grains,
        reverse: r.reverse,
    }
}

impl Wet for TransposeStretch {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        TransposeStretch::process(self, io);
    }

    fn memory(&self) -> Option<usize> {
        Some(TransposeStretch::memory(self))
    }

    fn written(&self) -> f32 {
        TransposeStretch::written(self)
    }

    fn idle(&mut self, silence: &mut [[f32; 2]]) {
        TransposeStretch::idle(self, silence.len());
    }

    fn set(&mut self, kind: &ir::EffectKind, tempo: f32) {
        if let ir::EffectKind::TransposeStretch(t) = kind {
            TransposeStretch::set(self, transpose_stretch(*t, tempo));
        }
    }
}

fn transpose_stretch(t: ir::TransposeStretch, tempo: f32) -> TransposeStretchSettings {
    TransposeStretchSettings {
        tempo_bpm: tempo,
        semitones: t.semitones,
        amount: t.amount,
        grain_ms: t.grain_ms,
        mix: t.mix,
        stretch: t.stretch,
        grain_mode: t.grain_mode,
        bars: t.bars,
    }
}

fn ring(r: ir::RingMod) -> RingMod {
    RingMod {
        hz: r.hz,
        spread: r.spread,
        bias: r.bias,
        quadrature: r.quadrature,
        drive: r.drive,
        highpass_hz: r.highpass_hz,
        gate: r.gate,
        feedback: r.feedback,
        source: match r.source {
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
        },
        wet: r.wet,
        dry: r.dry,
    }
}

fn transient(t: ir::Transient) -> TransientSettings {
    TransientSettings {
        attack: t.attack,
        sustain: t.sustain,
        gain_db: t.gain_db,
        clip: t.clip,
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

/// A compressor's settings in DSP terms.
pub fn compressor(c: ir::Compressor) -> CompressorSettings {
    CompressorSettings {
        threshold_db: c.threshold_db,
        ratio: c.ratio,
        knee_db: c.knee_db,
        attack_ms: c.attack_ms,
        release_ms: c.release_ms,
        makeup_db: c.makeup(),
        link: c.link,
        mix: c.mix,
        detector: match c.detector {
            ir::Detector::Input => Detector::Input,
            ir::Detector::Output => Detector::Output,
            ir::Detector::Trigger => Detector::Trigger,
        },
        sidechain_hz: c.sidechain_hz,
    }
}

fn limiter(l: ir::Limiter) -> LimiterSettings {
    LimiterSettings {
        input_db: l.input_db,
        ceiling_db: l.ceiling_db,
        release_ms: l.release_ms,
        knee_db: l.knee_db,
        attack_ms: l.attack_ms,
        smooth_ms: l.smooth_ms,
        slow: l.slow,
        lookahead_ms: l.lookahead_ms,
        clip: l.clip,
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

fn shifter(s: ir::Shifter, tempo: f32) -> ShifterSettings {
    let ms = match s.time {
        ir::DelayTime::Ms(ms) => ms,
        ir::DelayTime::Beats(beats) => beats * 60_000.0 / tempo.max(1.0),
    };
    ShifterSettings {
        ms: [ms, ms * s.right],
        speed: s.speed,
        feedback: s.feedback,
        cross: s.cross,
    }
}

fn delay(d: &ir::Delay, tempo: f32) -> kontra_dsp::delay::Settings {
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
        body: d.body.clone(),
        triangle_s: d.triangle_s,
        shape: d.shape.clone(),
        noise: d.noise,
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
        direct: d.direct,
    }
}

fn notches(n: ir::Notches, tempo: f32) -> NotchSettings {
    NotchSettings {
        stages: n.stages,
        hz: n.hz,
        octaves: n.octaves,
        spread: n.spread,
        spread_swing: n.spread_swing,
        spread_limit: n.spread_limit,
        feedback: n.feedback,
        loop_width: n.loop_width,
        loop_top: n.loop_top,
        lfo_hz: hz(n.rate, tempo),
        rounding: n.rounding,
        depth: n.depth,
        warp: n.warp,
        phase: n.phase,
        sign: match n.polarity {
            ir::Polarity::Normal => 1.0,
            ir::Polarity::Inverted => -1.0,
        },
        mix: n.mix,
        low_cut_hz: n.low_cut_hz,
        internal_hz: n.internal_hz,
    }
}

fn comb_bank(c: ir::CombBank, tempo: f32) -> CombSettings {
    use kontra_dsp::delay::ScanShape;
    let blend = |on: bool| if on { 1.0 } else { 0.0 };
    CombSettings {
        root_hz: c.root_hz,
        ratio: c.ratio,
        detune: c.detune,
        feedback: c.feedback,
        voices: c.voices,
        damping: c.damping,
        depth: c.depth,
        lfo_hz: hz(c.rate, tempo),
        offset: c.offset,
        polarity: match c.polarity {
            ir::Polarity::Normal => 1.0,
            ir::Polarity::Inverted => -1.0,
        },
        thru_zero: blend(c.mode == ir::CombMode::ThruZero),
        scan: blend(c.mode == ir::CombMode::Scan),
        scan_depth: c.scan_depth,
        scan_shape: match c.scan_shape {
            ir::ScanShape::Triangle => ScanShape::Triangle,
            ir::ScanShape::SawUp => ScanShape::SawUp,
            ir::ScanShape::SawDown => ScanShape::SawDown,
        },
        width: c.width,
        mix: c.mix,
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

fn settings(r: &ir::Reverb) -> Settings {
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

/// The processor slot `slot` runs. Not real-time.
pub fn processor(slot: Option<&ir::Effect>, rate: u32, impulses: &Responses<'_>) -> Dsp {
    match slot.map(|e| &e.kind) {
        // Retuned from its slot each span: a route or a script may move any
        // band, and one saved flat (no stages) may be raised later.
        Some(ir::EffectKind::Eq(bells)) => {
            Dsp::Bells(Box::new(filters::VoiceEq::new(*bells, rate)))
        }
        Some(ir::EffectKind::Filter(f)) => {
            let mut svf = Svf::new(filters::shape(f));
            svf.set_cutoff(f.hz, rate);
            // A script may switch it to any ladder on the audio thread.
            svf.keep_spare();
            Dsp::Filter(Box::new(svf))
        }
        // Built even while bypassed: a script may enable it later.
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
        Some(ir::EffectKind::Reverb(r)) => match &r.network {
            Some(ir::Network::Lines(law)) => wet(Box::new(Network::new(law, settings(r), rate))),
            Some(ir::Network::Combs(law)) => wet(Box::new(Combs::new(law, settings(r), rate))),
            Some(ir::Network::Hadamard(law)) => {
                wet(Box::new(Hadamard::new(*law, settings(r), rate)))
            }
            None => wet(Box::new(Fdn::new(settings(r), rate))),
        },
        Some(ir::EffectKind::Compressor(c)) => {
            wet(Box::new(Rated(Compressor::new(compressor(*c), rate), rate)))
        }
        Some(ir::EffectKind::Limiter(l)) => {
            wet(Box::new(Rated(Limiter::new(limiter(*l), rate), rate)))
        }
        // ponytail: an echo gets the longest line, so a script or a
        // tempo can lengthen it; a swept line only what it needs.
        Some(ir::EffectKind::Delay(d)) => {
            let max_ms = match d.time {
                ir::DelayTime::Ms(ms) if d.depth_ms > 0.0 => ms,
                _ => MAX_MS,
            };
            wet(Box::new(ModDelay::new(delay(d, 120.0), rate, max_ms)))
        }
        Some(ir::EffectKind::Shifter(s)) => wet(Box::new(Shifter::new(shifter(*s, 120.0), rate))),
        Some(ir::EffectKind::Scanner(s)) => wet(Box::new(Scanner::new(scanner(*s), rate))),
        Some(ir::EffectKind::Phaser(p)) => wet(Box::new(Phaser::new(phaser(*p, 120.0), rate))),
        Some(ir::EffectKind::Notches(n)) => wet(Box::new(Notches::new(notches(*n, 120.0), rate))),
        Some(ir::EffectKind::Ensemble(e)) => wet(Box::new(crate::ensemble::Ensemble::new(e, rate))),
        Some(ir::EffectKind::CombBank(c)) => wet(Box::new(CombBank::new(
            comb_bank(*c, 120.0),
            rate,
            c.lowest_hz,
        ))),
        Some(ir::EffectKind::Cascade(c)) => wet(Box::new(Cascade::new(cascade(*c), rate))),
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
        Some(ir::EffectKind::Shaper(s)) => wet(Box::new(Rated(shaper(s, rate), rate))),
        Some(ir::EffectKind::Strip(s)) => wet(Box::new(Strip(
            Compressor::new(compressor(s.compressor), rate),
            shaper(&s.shaper, rate),
            rate,
        ))),
        Some(ir::EffectKind::Overdrive(o)) => {
            wet(Box::new(Rated(Overdrive::new(voicing(o), rate), rate)))
        }
        Some(ir::EffectKind::Crush(c)) => {
            wet(Box::new(Rated(Crush::new(reduction(*c, rate), 1), rate)))
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
        Some(ir::EffectKind::Transient(t)) => wet(Box::new(Transient::new(transient(*t), rate))),
        Some(ir::EffectKind::RingMod(r)) => wet(Box::new(Rated(Ring::new(ring(*r), rate), rate))),
        Some(ir::EffectKind::Rotary(r)) => wet(Box::new(Rotary::new(rotary(*r), rate))),
        Some(ir::EffectKind::Gater(g)) => wet(Box::new(Gater::new(gater(*g, 120.0), rate))),
        Some(ir::EffectKind::BeatMasher(m)) => {
            wet(Box::new(BeatMasher::new(beat_masher(*m, 120.0), rate)))
        }
        Some(ir::EffectKind::BeatSlicer(s)) => {
            wet(Box::new(BeatSlicer::new(beat_slicer(*s, 120.0), rate)))
        }
        Some(ir::EffectKind::ReverseGrain(r)) => {
            wet(Box::new(ReverseGrain::new(reverse_grain(*r), rate)))
        }
        Some(ir::EffectKind::TransposeStretch(t)) => wet(Box::new(TransposeStretch::new(
            transpose_stretch(*t, 120.0),
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
pub fn shaper(s: &ir::Shaper, rate: u32) -> Shaper {
    Shaper::new(curve(&s.curve), tone(s), rate)
}

/// The filters around `s`'s curve.
pub fn tone(s: &ir::Shaper) -> drive::Tone {
    drive::Tone {
        pre_lowpass_hz: s.pre_lowpass_hz,
        lowpass_hz: s.lowpass_hz,
        highpass_hz: s.highpass_hz,
        sections: s.sections,
    }
}

pub fn curve(c: &ir::DriveCurve) -> drive::Curve {
    match c {
        ir::DriveCurve::Soft(x) => drive::Curve::Soft(*x),
        ir::DriveCurve::Tube(x) => drive::Curve::Tube(*x),
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
    }
}

/// Multiband overdrive settings in DSP terms.
pub fn voicing(o: &ir::Overdrive) -> Voicing {
    Voicing {
        split_hz: o.split_hz,
        smooth_hz: o.smooth_hz,
        drive: o.drive,
        mix: o.mix,
        level: o.level,
        curve: o.curve.clone(),
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
        converter: c.converter.map(|v| Converter {
            clock_hz: v.clock_hz,
            jitter: v.jitter,
            pre_hz: v.pre_hz,
            post_hz: v.post_hz,
            highpass_hz: v.highpass_hz,
            step: v.step,
            mu: v.mu,
            midrise: v.midrise,
            dither: v.dither,
            drive_in: v.drive_in,
            drive_out: v.drive_out,
            soft: v.soft,
            trim: v.trim,
            wet: v.wet,
            dry: v.dry,
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
