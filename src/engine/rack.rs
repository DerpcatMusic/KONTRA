//! Independently routed engines mixed onto sixteen stereo output
//! buses (Kontakt's st.1…st.16), each with its own fader and host port.

use super::{RackPart, MAX_BLOCK, voice::balance, SourceDelay};
use crate::fx::OUTS;

/// Initial rack storage for existing sessions; this is not a part-count limit.
pub const RACK_SLOTS: usize = 16;
pub const BUSES: usize = 16;
/// [`PartControls::aux`] when the part sends nowhere.
pub const NO_AUX: u8 = u8::MAX;

/// One stereo block: `[left, right]`.
pub type Block = [[f32; MAX_BLOCK]; 2];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartControls {
    pub port: u8,
    pub output: u8,
    /// MIDI channel, or −1 for omni.
    pub channel: i16,
    pub gain: f32,
    pub pan: f32,
    /// Semitones, cents as the fraction, within ±[`TUNE_RANGE`].
    pub tune: f32,
    pub mute: bool,
    pub solo: bool,
    /// Bus the part also sends to, post-fader, or [`NO_AUX`].
    pub aux: u8,
    /// Linear gain of that send.
    pub aux_gain: f32,
    /// Per output channel the instrument plays to past its own output (a
    /// mic mixer's "Out 2"), the bus it goes to; [`NO_AUX`] joins `output`.
    pub outs: [u8; OUTS],
}

/// An output bus's fader: what it does to everything routed to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BusControls {
    /// Linear.
    pub gain: f32,
    /// Balance, −1..=1.
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// Host stereo output port (0..[`BUSES`]) the bus plays through.
    pub port: u8,
}

impl BusControls {
    /// Unity, centred, on host port `port`.
    pub fn on(port: u8) -> Self {
        Self {
            gain: 1.0,
            pan: 0.0,
            mute: false,
            solo: false,
            port,
        }
    }
}

/// Everything the mixer sets, handed to the audio thread in one piece.
#[derive(Clone, Debug, PartialEq)]
pub struct Mix {
    pub parts: Vec<PartControls>,
    pub buses: [BusControls; BUSES],
}

impl Default for Mix {
    fn default() -> Self {
        Self {
            parts: vec![PartControls::default(); RACK_SLOTS],
            buses: std::array::from_fn(|n| BusControls::on(n as u8)),
        }
    }
}

/// Absolute sample peaks `[left, right]` since last taken: parts post-fader,
/// buses post-fader.
#[derive(Clone, Debug, PartialEq)]
pub struct Peaks {
    pub parts: Vec<[f32; 2]>,
    pub buses: [[f32; 2]; BUSES],
}

impl Default for Peaks {
    fn default() -> Self {
        Self { parts: vec![[0.0; 2]; RACK_SLOTS], buses: [[0.0; 2]; BUSES] }
    }
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}

/// How far a part tunes, in semitones either way.
pub const TUNE_RANGE: f32 = 36.0;

impl Default for PartControls {
    fn default() -> Self {
        Self {
            port: 0,
            output: 0,
            channel: -1,
            gain: 1.0,
            pan: 0.0,
            tune: 0.0,
            mute: false,
            solo: false,
            aux: NO_AUX,
            aux_gain: 0.0,
            outs: [NO_AUX; OUTS],
        }
    }
}

impl PartControls {
    fn hears(&self, port: u8, channel: u8) -> bool {
        self.port == port && (self.channel < 0 || self.channel == i16::from(channel))
    }
}

/// Rack storage is prepared outside rendering and adopted without allocating.
pub struct Rack {
    pub parts: Vec<RackPart>,
    pub controls: Vec<PartControls>,
    pub bus_controls: [BusControls; BUSES],
    /// Accumulated by [`render`](Self::render); take them to publish.
    pub peaks: Peaks,
    /// The slot whose post-fader signal [`render`](Self::render) copies to
    /// `tapped` (mono) for a spectrum; `None` copies nothing.
    pub tap: Option<usize>,
    pub tapped: [f32; MAX_BLOCK],
    part: Block,
    // Prepared per-slot storage lets external renderers run once per host span.
    external: Vec<Option<Block>>,
    buses: [Block; BUSES],
    /// Per bus, frames holding signal; beyond them it is all zeros.
    written: [usize; BUSES],
    /// Absolute frame shared by every Rack-owned instrument.
    now: u64,
}

impl Default for Rack {
    fn default() -> Self {
        Self::with_slots(RACK_SLOTS)
    }
}

impl Rack {
    /// Allocate on the worker before handing this storage to audio.
    pub fn with_slots(slots: usize) -> Self {
        Self {
            parts: (0..slots).map(|_| RackPart::default()).collect(),
            controls: vec![PartControls::default(); slots],
            bus_controls: std::array::from_fn(|n| BusControls::on(n as u8)),
            peaks: Peaks { parts: vec![[0.0; 2]; slots], buses: [[0.0; 2]; BUSES] },
            tap: None,
            tapped: [0.0; MAX_BLOCK],
            part: [[0.0; MAX_BLOCK]; 2],
            external: vec![None; slots],
            buses: [[[0.0; MAX_BLOCK]; 2]; BUSES],
            written: [0; BUSES],
            now: 0,
        }
    }
}

impl Rack {
    /// Adopt larger worker-prepared storage, keeping every current engine and
    /// its voices. Return the old containers in `prepared` for worker disposal.
    pub fn adopt_parts(&mut self, prepared: &mut Self) {
        assert!(prepared.parts.len() >= self.parts.len());
        for engine in &mut prepared.parts[self.parts.len()..] {
            engine.initialize_frame(self.now);
        }
        for (current, next) in self.parts.iter_mut().zip(&mut prepared.parts) {
            std::mem::swap(current, next);
        }
        prepared.controls[..self.controls.len()].copy_from_slice(&self.controls);
        prepared.peaks.parts[..self.peaks.parts.len()].copy_from_slice(&self.peaks.parts);
        std::mem::swap(&mut self.parts, &mut prepared.parts);
        std::mem::swap(&mut self.controls, &mut prepared.controls);
        std::mem::swap(&mut self.peaks.parts, &mut prepared.peaks.parts);
        std::mem::swap(&mut self.external, &mut prepared.external);
    }

    /// Invoke one part with an immutable census of its peers at this DSP frame.
    /// Direct access through `parts` does not establish a Rack-wide KSP context.
    pub(crate) fn with_part<R>(&mut self, slot: usize, f: impl FnOnce(&mut RackPart) -> R) -> Option<R> {
        let index = slot.min(self.parts.len());
        let (before, rest) = self.parts.split_at_mut(index);
        let (engine, after) = rest.split_first_mut()?;
        let peers = before.iter().chain(after.iter()).fold(0usize,
            |total, peer| total.saturating_add(peer.active_voices()));
        Some(engine.with_peer_voices(peers, f))
    }

    /// Instrument-only operations cannot mutate a bank's selected program.
    #[cfg(test)]
    pub(crate) fn with_instrument<R>(&mut self, slot: usize, f: impl FnOnce(&mut super::Engine) -> R) -> Option<R> {
        self.with_part(slot, |part| part.instrument_mut().map(f)).flatten()
    }

    // One census per ordered phase; each invocation may change only its own roster.
    fn for_each_part(&mut self, mut f: impl FnMut(usize, &mut RackPart)) {
        let mut total = self.parts.iter().fold(0usize, |sum, e| sum.saturating_add(e.active_voices()));
        for (slot, engine) in self.parts.iter_mut().enumerate() {
            let peers = total.saturating_sub(engine.active_voices());
            engine.with_peer_voices(peers, |e| f(slot, e));
            total = peers.saturating_add(engine.active_voices());
        }
    }

    pub fn reset(&mut self, rate: f64) {
        for slot in 0..self.parts.len() { self.with_part(slot, |e| e.reset(rate)); }
    }

    pub fn set_controls(&mut self, mix: &Mix) {
        self.bus_controls = mix.buses;
        for slot in 0..self.parts.len().min(self.controls.len()) {
            let old = self.controls[slot];
            let new = mix.parts.get(slot).copied().unwrap_or_default();
            let home = new.channel.clamp(0, 15) as u8;
            self.with_part(slot, |engine| {
                if engine.is_instrument_bank() { return; }
                if (old.port, old.channel) != (new.port, new.channel) {
                    let rate = engine.rate();
                    engine.reset_input(rate, home, old.port != new.port);
                } else { engine.set_home_channel(home); }
            });
            self.controls[slot] = new;
        }
    }

    /// One host timing snapshot reaches every instrument before its MIDI input.
    pub fn set_transport(&mut self, playing: bool, tempo: f64, beats: f64, signature: (u8, u8)) {
        for slot in 0..self.parts.len() { self.with_part(slot, |e| e.set_transport(playing, tempo, beats, signature)); }
    }

    /// Program selection belongs only to banks listening at their saved address.
    pub(crate) fn program_change_port(&mut self, port: u8, channel: u8, program: u8) -> bool {
        let mut handled = false;
        for part in &mut self.parts {
            if let Some(bank) = part.program_bank_mut().filter(|b| b.matches_input(port, channel)) {
                bank.midi1(port, &[0xc0 | channel, program]);
                handled = true;
            }
        }
        handled
    }

    pub fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        self.note_on_port(0, channel, note, velocity);
    }

    pub fn note_on_port(&mut self, port: u8, channel: u8, note: u8, velocity: u8) {
        for slot in 0..self.parts.len().min(self.controls.len()) {
            let c = self.controls[slot];
            if c.hears(port, channel) { self.with_part(slot, |e| { e.note_on(channel, note, velocity); }); }
        }
    }

    pub fn note_off_port(&mut self, port: u8, channel: u8, note: u8) {
        for slot in 0..self.parts.len().min(self.controls.len()) {
            let c = self.controls[slot];
            if c.port == port { self.with_part(slot, |e| { e.note_off(channel, note); }); }
        }
    }

    /// Instrument volume and pan (CC7/CC10) reach only parts listening on the
    /// channel; other controllers reach the whole port so pedals never stick.
    pub fn cc_port(&mut self, port: u8, channel: u8, cc: u8, value: u8) {
        for slot in 0..self.parts.len().min(self.controls.len()) {
            let c = self.controls[slot];
            if c.port == port && (!matches!(cc, 7 | 10) || c.hears(port, channel)) { self.with_part(slot, |e| { e.cc(channel, cc, value); }); }
        }
    }

    /// Channel pressure, for scripts of parts listening on the channel.
    pub fn channel_pressure_port(&mut self, port: u8, channel: u8, value: u8) {
        for slot in 0..self.parts.len().min(self.controls.len()) {
            let c = self.controls[slot];
            if c.hears(port, channel) { self.with_part(slot, |e| { e.channel_pressure(channel, value); }); }
        }
    }

    /// Polyphonic key pressure, for scripts of parts listening on the channel.
    pub fn poly_pressure_port(&mut self, port: u8, channel: u8, note: u8, value: u8) {
        for slot in 0..self.parts.len().min(self.controls.len()) {
            let c = self.controls[slot];
            if c.hears(port, channel) { self.with_part(slot, |e| { e.poly_pressure(channel, note, value); }); }
        }
    }

    pub fn pitch_bend_port(&mut self, port: u8, channel: u8, value: u16) {
        for slot in 0..self.parts.len().min(self.controls.len()) {
            let c = self.controls[slot];
            if c.port == port { self.with_part(slot, |e| { e.pitch_bend(channel, value); }); }
        }
    }

    /// Releases always reach every engine: changing routing while a key is held must not stick it.
    pub fn note_off(&mut self, channel: u8, note: u8) {
        for slot in 0..self.parts.len() { self.with_part(slot, |e| e.note_off(channel, note)); }
    }

    pub fn cc(&mut self, channel: u8, cc: u8, value: u8) {
        for slot in 0..self.parts.len() { self.with_part(slot, |e| e.cc(channel, cc, value)); }
    }
    pub fn panic(&mut self) {
        for slot in 0..self.parts.len() { self.with_part(slot, |e| e.panic()); }
    }

    pub fn pitch_bend(&mut self, channel: u8, value: u16) {
        for slot in 0..self.parts.len() { self.with_part(slot, |e| e.pitch_bend(channel, value)); }
    }

    /// Render `frames` (at most [`MAX_BLOCK`]) into the bus blocks, after
    /// each bus's fader; [`BusControls::port`] says where each one plays.
    pub fn render(&mut self, frames: usize) -> &[Block; BUSES] {
        self.render_live(frames).0
    }

    /// [`render`](Self::render), also saying which buses carry signal: the
    /// others are silent and need no copying out.
    pub fn render_live(&mut self, frames: usize) -> (&[Block; BUSES], [bool; BUSES]) {
        self.render_live_with(frames, |_, _, _| false)
    }

    pub fn render_live_with(&mut self, frames: usize,
        render_slot: impl FnMut(usize, &mut [f32], &mut [f32]) -> bool,
    ) -> (&[Block; BUSES], [bool; BUSES]) {
        self.render_live_with_delay(frames, render_slot, &mut [])
    }

    /// External sources render once; Kontakt sources retain ordered cross-part boundaries.
    pub fn render_live_with_delay(&mut self, frames: usize,
        mut render_slot: impl FnMut(usize, &mut [f32], &mut [f32]) -> bool,
        delays: &mut [SourceDelay],
    ) -> (&[Block; BUSES], [bool; BUSES]) {
        let n = frames.min(MAX_BLOCK);
        for (slot, external) in self.external.iter_mut().enumerate() {
            let [left, right] = &mut self.part;
            left[..n].fill(0.0);
            right[..n].fill(0.0);
            *external = render_slot(slot, &mut left[..n], &mut right[..n]).then_some(self.part);
        }
        for (bus, written) in self.buses.iter_mut().zip(&mut self.written) {
            bus[0][..*written].fill(0.0);
            bus[1][..*written].fill(0.0);
            *written = 0;
        }
        if self.tap.is_some() {
            self.tapped[..n].fill(0.0);
        }
        let solo = self.controls.iter().any(|c| c.solo);
        let _ftz = super::FlushDenormals::new();
        for (engine, c) in self.parts.iter_mut().zip(&self.controls) { engine.set_tune(c.tune); }
        self.for_each_part(|_, engine| engine.render_begin(n));
        let mut at = 0;
        loop {
            // Every instrument has completed preceding DSP before any callback.
            self.for_each_part(|_, engine| engine.render_boundary(at));
            if at == n { break; }
            let end = self.parts.iter().fold(n, |end, engine| end.min(engine.render_next_boundary(at)));
            debug_assert!(end > at, "a settled Rack boundary must advance");
            let len = end - at;
            for (slot, engine) in self.parts.iter_mut().enumerate() {
                let fx_offset = engine.render_output_offset(at);
                let [left, right] = &mut self.part;
                let external = self.external[slot].as_ref();
                if let Some([l, r]) = external {
                    left[at..end].copy_from_slice(&l[at..end]);
                    right[at..end].copy_from_slice(&r[at..end]);
                } else {
                    engine.render_interval(at, &mut left[at..end], &mut right[at..end]);
                }
                let delay = delays.get_mut(slot).filter(|d| external.is_none() && d.delay() != 0);
                let delay = delay.map(|d| {
                    d.process(&mut left[at..end], &mut right[at..end],
                        engine.direct_outs_range(fx_offset..fx_offset + len));
                    d
                });
                let (Some(c), Some(meter)) = (self.controls.get(slot), self.peaks.parts.get_mut(slot)) else { continue };
                let (left, right) = (&mut left[at..end], &mut right[at..end]);
                if c.mute || (solo && !c.solo) {
                    continue;
                }
                let [gl, gr] = balance(c.gain, c.pan);
                // Output channels past the instrument's own, after the part's fader.
                let direct = engine.direct_outs_range(fx_offset..fx_offset + len)
                    .filter(|_| external.is_none() && delay.is_none())
                    .chain(delay.as_deref().into_iter().flat_map(|d| d.direct_outs(len)));
                for (out, l, r) in direct {
                    let bus = match c.outs[out] {
                        NO_AUX => c.output,
                        b => b,
                    } as usize;
                    let bus = bus.min(BUSES - 1);
                    self.written[bus] = end;
                    let [bus_l, bus_r] = &mut self.buses[bus];
                    for (o, x) in bus_l[at..end].iter_mut().zip(l) {
                        *o += x * gl;
                    }
                    for (o, x) in bus_r[at..end].iter_mut().zip(r) {
                        *o += x * gr;
                    }
                    *meter = [meter[0].max(peak(l) * gl), meter[1].max(peak(r) * gr)];
                }
                // Silent parts, idle instruments and empty slots, add nothing.
                if peak(left) == 0.0 && peak(right) == 0.0 {
                    continue;
                }
                for x in left.iter_mut() {
                    *x *= gl;
                }
                for x in right.iter_mut() {
                    *x *= gr;
                }
                *meter = [meter[0].max(peak(left)), meter[1].max(peak(right))];
                if self.tap == Some(slot) {
                    for ((t, l), r) in self.tapped[at..end].iter_mut().zip(&*left).zip(&*right) {
                        *t = (l + r) * 0.5;
                    }
                }
                let aux = (c.aux as usize) < BUSES && c.aux != c.output && c.aux_gain != 0.0;
                for (bus, gain) in [(c.output as usize, 1.0), (c.aux as usize, c.aux_gain)]
                    .into_iter()
                    .take(1 + usize::from(aux))
                {
                    let bus = bus.min(BUSES - 1);
                    self.written[bus] = end;
                    let [bus_l, bus_r] = &mut self.buses[bus];
                    for (out, x) in bus_l[at..end].iter_mut().zip(&*left) {
                        *out += x * gain;
                    }
                    for (out, x) in bus_r[at..end].iter_mut().zip(&*right) {
                        *out += x * gain;
                    }
                }
            }
            at = end;
        }
        self.for_each_part(|_, engine| engine.render_finish());
        self.now = self.now.saturating_add(n as u64);
        let solo = self.bus_controls.iter().any(|c| c.solo);
        for (((bus, c), meter), &written) in self
            .buses
            .iter_mut()
            .zip(&self.bus_controls)
            .zip(&mut self.peaks.buses)
            .zip(&self.written)
        {
            if written == 0 {
                continue;
            }
            let [gl, gr] = if c.mute || (solo && !c.solo) {
                [0.0; 2]
            } else {
                balance(c.gain, c.pan)
            };
            for (x, g) in bus.iter_mut().zip([gl, gr]) {
                if g != 1.0 {
                    for x in &mut x[..n] {
                        *x *= g;
                    }
                }
            }
            *meter = [
                meter[0].max(peak(&bus[0][..n])),
                meter[1].max(peak(&bus[1][..n])),
            ];
        }
        (&self.buses, self.written.map(|w| w > 0))
    }
}

#[cfg(all(test, feature = "plugin"))]
#[path = "rack_timeline_tests.rs"]
mod rack_timeline_tests;
