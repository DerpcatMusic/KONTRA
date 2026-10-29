//! Instrument and bus racks run in slot order on their summed audio, so a
//! slot with state (an EQ, a convolution) sits where the format put it.
//! Group racks fold into each voice's matrix instead (voice_gain.rs).

use std::sync::Arc;

use kontra_dsp::delay::{MAX_MS, ModDelay, Phaser, PhaserSettings};
use kontra_dsp::drive::{Circuit, Clipper, Shaper};
use kontra_dsp::dynamics::{
    Compressor, CompressorSettings, Detector, Limiter, LimiterSettings, Transient,
    TransientSettings,
};
use kontra_dsp::filter::{Cascade, CascadeSettings, CascadeStage};
use kontra_dsp::reverb::{Combs, Convolver, Fdn, Hadamard, Network, Settings};
use kontra_ir as ir;
use kontra_voice::Gain;

use crate::behaviour::Behaviour;
use crate::effects;
use crate::filters;
use crate::ledger::{self, Ramp};
use crate::voice_rack;
use crate::{Engine, MAX_SPAN, Response, SampleTime};

pub(crate) enum Dsp {
    None,
    Eq(Box<[filters::Stage]>),
    /// An EQ routes move: all its bands, retuned each span.
    Bells(Box<filters::VoiceEq>),
    /// A wet path (convolver or reverb) and a span of its output.
    Wet(Box<dyn Wet>, Box<[[f32; 2]]>),
}

/// A processor whose span replaces the slot's output; the slot's dry level
/// mixes the input back.
pub(crate) trait Wet: Send {
    fn process(&mut self, io: &mut [[f32; 2]]);
    /// The slot's settings as scripts left them, at the host's tempo,
    /// before each block. Never allocates.
    fn set(&mut self, _kind: ir::EffectKind, _tempo: f32) {}
}

impl Wet for Convolver {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Convolver::process(self, io);
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
            self.set_curve(voice_rack::curve(s.curve));
        }
    }
}

impl Wet for Clipper {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Clipper::process(self, io);
    }
}

/// A convolution playing `r`. Not real-time.
pub(crate) fn convolver(r: &Response) -> Dsp {
    wet(Box::new(Convolver::new(&r.left, &r.right)))
}

fn wet(processor: Box<dyn Wet>) -> Dsp {
    Dsp::Wet(processor, vec![[0.0; 2]; MAX_SPAN as usize].into())
}

pub(crate) struct Rack {
    /// The plan's slots as scripts have changed them.
    pub(crate) slots: Box<[Option<ir::Effect>]>,
    dsp: Box<[Dsp]>,
    /// Routes moving the slots ([`ir::Bus::mods`]), the blends they read,
    /// and each route's amount for this span.
    mods: Box<[ir::ModKind]>,
    blends: Box<[ir::Blend]>,
    amounts: Box<[f32]>,
    rate: u32,
}

/// The sources a rack's routes read: those no note owns.
pub(crate) struct Live<'a> {
    pub(crate) behaviour: &'a Behaviour,
    pub(crate) scripts: &'a [Option<(u16, Ramp)>],
    pub(crate) now: SampleTime,
}

impl Live<'_> {
    /// Controllers and pitch bend come from the channel last played.
    fn read(&self, source: ir::Source, blends: &[ir::Blend]) -> Option<f32> {
        let (b, channel) = (self.behaviour, self.behaviour.channel);
        Some(match source {
            ir::Source::Controller(cc) => f32::from(b.cc(channel, cc)) / 127.0,
            ir::Source::ControllerFrom { cc, start } => {
                let arrived = b.cc_arrived(channel, cc);
                f32::from(if arrived { b.cc(channel, cc) } else { start }) / 127.0
            }
            ir::Source::PitchBend => b.bend(channel),
            ir::Source::Script(i) => {
                ledger::ramp_at(self.scripts, i, self.now).unwrap_or(0) as f32 / 1_000_000.0
            }
            ir::Source::Constant => 1.0,
            ir::Source::Blend(n) => {
                let b = blends.get(usize::from(n))?;
                let mut sum = b.base;
                for t in &b.terms {
                    let mut x = t.amount * self.through(&t.input)?;
                    for f in &t.factors {
                        x *= f.at(self.through(&f.input)?);
                    }
                    sum += x;
                }
                sum
            }
            _ => return None,
        })
    }

    /// `t`'s source through its table; a bipolar source spans a table as
    /// `(x + 1)/2`. No blend: no recursion.
    fn through(&self, t: &ir::Transfer) -> Option<f32> {
        let x = self.read(t.source, &[])?;
        let bipolar = matches!(t.source, ir::Source::PitchBend) && !t.table.is_empty();
        Some(t.apply(if bipolar { (x + 1.0) / 2.0 } else { x }))
    }
}

/// The plan's responses, then those scripts loaded ([`Engine::load_ir_sample`]).
#[derive(Clone, Copy)]
pub(crate) struct Responses<'a>(
    pub(crate) &'a [Option<Response>],
    pub(crate) &'a [(String, ir::Impulse, Response)],
);

impl Responses<'_> {
    fn get(&self, i: usize) -> Option<&Response> {
        match i.checked_sub(self.0.len()) {
            None => self.0.get(i)?.as_ref(),
            Some(j) => self.1.get(j).map(|(_, _, r)| r),
        }
    }
}

impl Engine {
    /// `load_ir_sample`: the library response `name` (its extension and
    /// folder dropped) into the convolution at `slot` of the `generic` rack,
    /// prepared as the response it replaces. The answer: 0 when it plays now
    /// (in the init callback, decoded at once: the racks are rebuilt after
    /// it), or the async id of a load the host decodes off the audio thread
    /// ([`Engine::request_response`]); `None` when it cannot load.
    pub(crate) fn load_ir_sample(&mut self, name: &str, slot: i32, generic: i32) -> Option<i32> {
        // Borrowed, matched without case: this may run on the audio thread.
        let stem = stem(name);
        let plan = Arc::clone(&self.plan);
        let found = (plan.responses.iter()).position(|(n, _)| n.eq_ignore_ascii_case(stem))?;
        let (_, sample) = plan.responses.get(found)?;
        let &ir::Effect {
            kind: ir::EffectKind::Convolution { impulse },
            ..
        } = self.effect_mut(-1, slot, generic).map(|e| &*e)?
        else {
            return None;
        };
        let at = usize::from(impulse);
        let (playing, law) = match at.checked_sub(plan.impulses.len()) {
            None => plan.impulse_laws.get(at).map(|(n, law)| (n, *law)),
            Some(j) => self.loaded.get(j).map(|(n, law, _)| (n, *law)),
        }
        .unzip();
        if playing.is_some_and(|n| n.eq_ignore_ascii_case(stem)) {
            return Some(0);
        }
        let law = law?;
        if self.running.kind != crate::script::Kind::Init {
            let index = u16::try_from(found).ok()?;
            return self.request_response(slot, generic, index, law);
        }
        let known =
            (self.loaded.iter()).position(|(n, l, _)| n.eq_ignore_ascii_case(stem) && *l == law);
        let index = match known {
            Some(j) => j,
            None => {
                let r = crate::response(sample, &law, self.rate).ok()?;
                self.loaded.push((stem.to_lowercase(), law, r));
                // Room for loads after init, which must not allocate.
                self.loaded.reserve(crate::fx_types::REQUESTS);
                self.loaded.len() - 1
            }
        };
        let index = u16::try_from(plan.impulses.len() + index).ok()?;
        if let Some(e) = self.effect_mut(-1, slot, generic) {
            e.kind = ir::EffectKind::Convolution { impulse: index };
        }
        Some(0)
    }
}

/// A response file's name as [`Plan::responses`] keys it (there in
/// lowercase): without its folder and extension.
pub fn stem(name: &str) -> &str {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    file.rsplit_once('.').map_or(file, |(s, _)| s)
}

impl Rack {
    /// `impulses` are the plan's convolution responses. Not real-time.
    pub(crate) fn new(
        slots: &[Option<ir::Effect>],
        rate: u32,
        impulses: &[Option<Response>],
    ) -> Self {
        Self::routed(slots, rate, Responses(impulses, &[]), &[], &[])
    }

    /// As [`Self::new`], its slots moved by `mods` reading `blends`.
    pub(crate) fn routed(
        slots: &[Option<ir::Effect>],
        rate: u32,
        impulses: Responses<'_>,
        mods: &[ir::ModKind],
        blends: &[ir::Blend],
    ) -> Self {
        let moved = |n: usize| {
            mods.iter().any(
                |m| matches!(m, ir::ModKind::EffectParam { insert, .. } if usize::from(*insert) == n),
            )
        };
        let dsp = slots
            .iter()
            .enumerate()
            .map(|(n, slot)| processor(*slot, moved(n), rate, &impulses))
            .collect();
        Self {
            slots: slots.into(),
            dsp,
            mods: mods.into(),
            blends: blends.into(),
            amounts: vec![0.0; mods.len()].into(),
            rate,
        }
    }

    /// Puts `effect` and its processor in slot `n`; the old pair comes back.
    pub(crate) fn install(
        &mut self,
        n: usize,
        effect: Option<ir::Effect>,
        dsp: Dsp,
    ) -> Result<(Option<ir::Effect>, Dsp), Dsp> {
        let (Some(slot), Some(old)) = (self.slots.get_mut(n), self.dsp.get_mut(n)) else {
            return Err(dsp);
        };
        Ok((std::mem::replace(slot, effect), std::mem::replace(old, dsp)))
    }

    /// Reads its routes' sources for the next span.
    pub(crate) fn follow(&mut self, live: &Live) {
        for (m, amount) in self.mods.iter().zip(self.amounts.iter_mut()) {
            if let ir::ModKind::EffectParam { source, depth, .. } = *m {
                *amount = live.read(source, &self.blends).unwrap_or(0.0) * depth;
            }
        }
    }

    /// Processes `io`; a send slot adds it to `taps` (one buffer per send slot).
    pub(crate) fn process(
        &mut self,
        io: &mut [[f32; 2]],
        taps: &mut [Box<[[f32; 2]]>],
        tempo: f32,
    ) {
        for (n, (slot, dsp)) in self.slots.iter().zip(self.dsp.iter_mut()).enumerate() {
            let Some(mut e) = slot.filter(|e| !e.bypass) else {
                continue;
            };
            for (m, &amount) in self.mods.iter().zip(self.amounts.iter()) {
                if let ir::ModKind::EffectParam { insert, param, .. } = *m
                    && usize::from(insert) == n
                {
                    param.add(&mut e.kind, amount);
                }
            }
            match (dsp, e.kind) {
                (Dsp::Eq(f), _) => filters::run(f, io),
                // ponytail: retuned once a span; per-sample if a sweep zippers.
                (Dsp::Bells(eq), ir::EffectKind::Eq(bells)) => {
                    eq.retune(bells, self.rate);
                    eq.process(io);
                }
                // The wet path from the input, then the dry level, then the wet at the output gain.
                (Dsp::Wet(processor, wet), kind) => {
                    let Some(wet) = wet.get_mut(..io.len()) else {
                        continue;
                    };
                    // A script or the tempo may have moved it since the last block.
                    processor.set(kind, tempo);
                    wet.copy_from_slice(io);
                    processor.process(wet);
                    for (x, w) in io.iter_mut().zip(wet.iter()) {
                        x[0] = x[0] * e.dry + w[0] * e.output;
                        x[1] = x[1] * e.dry + w[1] * e.output;
                    }
                    continue;
                }
                (_, ir::EffectKind::Sends(levels)) => {
                    for (tap, level) in taps.iter_mut().zip(levels) {
                        for (t, x) in tap.iter_mut().zip(io.iter()) {
                            t[0] += x[0] * level;
                            t[1] += x[1] * level;
                        }
                    }
                }
                _ => {}
            }
            apply(effects::matrix(&e), io);
        }
    }

    /// Whether any slot is active (a send slot with nothing in it returns nothing).
    pub(crate) fn active(&self) -> bool {
        self.slots.iter().flatten().any(|e| !e.bypass)
    }
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
pub(crate) fn processor(
    slot: Option<ir::Effect>,
    moved: bool,
    rate: u32,
    impulses: &Responses<'_>,
) -> Dsp {
    match slot.map(|e| e.kind) {
        Some(ir::EffectKind::Eq(bells)) if moved => {
            Dsp::Bells(Box::new(filters::VoiceEq::new(bells, rate)))
        }
        // Built even while bypassed: a script may enable it later.
        Some(ir::EffectKind::Eq(_) | ir::EffectKind::Filter(_) | ir::EffectKind::Formant(_)) => {
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
        Some(ir::EffectKind::Cascade(c)) => wet(Box::new(Cascade::new(cascade(c), rate))),
        Some(ir::EffectKind::Shaper(s)) => wet(Box::new(voice_rack::shaper(s, rate))),
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

pub(crate) fn apply(m: Gain, io: &mut [[f32; 2]]) {
    if m == effects::IDENTITY {
        return;
    }
    let [[ll, lr], [rl, rr]] = m;
    for f in io {
        let [l, r] = *f;
        *f = [ll * l + lr * r, rl * l + rr * r];
    }
}

impl Engine {
    /// Rebuilds every rack's processors from its slots as the init callback
    /// left them. Not real-time.
    pub(crate) fn rebuild_racks(&mut self) {
        let rate = self.rate;
        let plan = Arc::clone(&self.plan);
        let racks = std::iter::once(&mut self.rack)
            .chain(self.sends.iter_mut())
            .chain(std::iter::once(&mut self.main))
            .chain(self.buses.iter_mut().map(|b| &mut b.rack));
        for rack in racks {
            let (mods, blends) = (
                std::mem::take(&mut rack.mods),
                std::mem::take(&mut rack.blends),
            );
            let impulses = Responses(&plan.impulses, &self.loaded);
            *rack = Rack::routed(&rack.slots, rate, impulses, &mods, &blends);
        }
    }

    /// Adds the buses into `span` through their racks, then runs the
    /// instrument's insert, send and main racks on it.
    pub(crate) fn mix_out(&mut self, span: &mut [[f32; 2]]) {
        let (len, tempo) = (span.len(), self.tempo);
        for tap in &mut self.taps {
            tap.fill([0.0; 2]);
        }
        for at in 0..self.buses.len() {
            let (done, later) = self.buses.split_at_mut(at + 1);
            let Some(bus) = done.last_mut() else {
                continue;
            };
            if let Some(mix) = bus.mix.get_mut(..len) {
                let live = Live {
                    behaviour: &self.behaviour,
                    scripts: &self.script_mods,
                    now: self.now,
                };
                bus.rack.follow(&live);
                bus.rack.process(mix, &mut self.taps, tempo);
                // A send to an earlier bus would come a span late: none.
                for send in &bus.sends {
                    let level = send.level * if send.post_fader { bus.gain } else { 1.0 };
                    let Some(to) = send.bus.checked_sub(at + 1).and_then(|k| later.get_mut(k))
                    else {
                        continue;
                    };
                    for (o, m) in to.mix.iter_mut().zip(mix.iter()) {
                        o[0] += m[0] * level;
                        o[1] += m[1] * level;
                    }
                }
                apply(effects::balance(bus.pan), mix);
            }
            if !bus.to_main {
                continue;
            }
            for (o, m) in span.iter_mut().zip(bus.mix.iter()) {
                o[0] += m[0] * bus.gain;
                o[1] += m[1] * bus.gain;
            }
        }
        self.rack.process(span, &mut self.taps, tempo);
        for (send, tap) in self.sends.iter_mut().zip(self.taps.iter_mut()) {
            if let Some(tap) = tap.get_mut(..len).filter(|_| send.active()) {
                send.process(tap, &mut [], tempo);
                for (o, t) in span.iter_mut().zip(tap.iter()) {
                    o[0] += t[0];
                    o[1] += t[1];
                }
            }
        }
        self.main.process(span, &mut [], tempo);
    }
}
