//! Sixteen independently routed engines mixed onto sixteen stereo output
//! buses (Kontakt's st.1…st.16), each with its own fader and host port.

use super::{Engine, MAX_BLOCK, voice::balance};
use crate::fx::OUTS;

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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mix {
    pub parts: [PartControls; RACK_SLOTS],
    pub buses: [BusControls; BUSES],
}

impl Default for Mix {
    fn default() -> Self {
        Self {
            parts: [PartControls::default(); RACK_SLOTS],
            buses: std::array::from_fn(|n| BusControls::on(n as u8)),
        }
    }
}

/// Absolute sample peaks `[left, right]` since last taken: parts post-fader,
/// buses post-fader.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Peaks {
    pub parts: [[f32; 2]; RACK_SLOTS],
    pub buses: [[f32; 2]; BUSES],
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

/// Fixed engines keep voice allocation and ownership changes outside rendering.
pub struct Rack {
    pub parts: [Engine; RACK_SLOTS],
    pub controls: [PartControls; RACK_SLOTS],
    pub bus_controls: [BusControls; BUSES],
    /// Accumulated by [`render`](Self::render); take them to publish.
    pub peaks: Peaks,
    /// The slot whose post-fader signal [`render`](Self::render) copies to
    /// `tapped` (mono) for a spectrum; `None` copies nothing.
    pub tap: Option<usize>,
    pub tapped: [f32; MAX_BLOCK],
    part: Block,
    buses: [Block; BUSES],
    /// Per bus, frames holding signal; beyond them it is all zeros.
    written: [usize; BUSES],
}

impl Default for Rack {
    fn default() -> Self {
        Self {
            parts: std::array::from_fn(|_| Engine::default()),
            controls: [PartControls::default(); RACK_SLOTS],
            bus_controls: Mix::default().buses,
            peaks: Peaks::default(),
            tap: None,
            tapped: [0.0; MAX_BLOCK],
            part: [[0.0; MAX_BLOCK]; 2],
            buses: [[[0.0; MAX_BLOCK]; 2]; BUSES],
            written: [0; BUSES],
        }
    }
}

impl Rack {
    pub fn reset(&mut self, rate: f64) {
        for e in &mut self.parts {
            e.reset(rate);
        }
    }

    pub fn set_controls(&mut self, mix: Mix) {
        let Mix {
            parts: controls,
            buses,
        } = mix;
        self.bus_controls = buses;
        for ((engine, old), new) in self.parts.iter_mut().zip(&self.controls).zip(&controls) {
            if (old.port, old.channel) != (new.port, new.channel) {
                let rate = engine.rate();
                engine.reset(rate);
            }
        }
        self.controls = controls;
    }

    /// One host timing snapshot reaches every instrument before its MIDI input.
    pub fn set_transport(&mut self, playing: bool, tempo: f64, beats: f64, signature: (u8, u8)) {
        for engine in &mut self.parts { engine.set_transport(playing, tempo, beats, signature); }
    }

    pub fn note_on(&mut self, channel: u8, note: u8, velocity: u8) {
        self.note_on_port(0, channel, note, velocity);
    }

    pub fn note_on_port(&mut self, port: u8, channel: u8, note: u8, velocity: u8) {
        for (e, c) in self.parts.iter_mut().zip(&self.controls) {
            if c.hears(port, channel) {
                e.note_on(channel, note, velocity);
            }
        }
    }

    pub fn note_off_port(&mut self, port: u8, channel: u8, note: u8) {
        for (e, c) in self.parts.iter_mut().zip(&self.controls) {
            if c.port == port {
                e.note_off(channel, note);
            }
        }
    }

    /// Instrument volume and pan (CC7/CC10) reach only parts listening on the
    /// channel; other controllers reach the whole port so pedals never stick.
    pub fn cc_port(&mut self, port: u8, channel: u8, cc: u8, value: u8) {
        for (e, c) in self.parts.iter_mut().zip(&self.controls) {
            let instrument_level = matches!(cc, 7 | 10);
            if c.port == port && (!instrument_level || c.hears(port, channel)) {
                e.cc(channel, cc, value);
            }
        }
    }

    /// Channel pressure, for scripts of parts listening on the channel.
    pub fn channel_pressure_port(&mut self, port: u8, channel: u8, value: u8) {
        for (e, c) in self.parts.iter_mut().zip(&self.controls) {
            if c.hears(port, channel) {
                e.channel_pressure(channel, value);
            }
        }
    }

    /// Polyphonic key pressure, for scripts of parts listening on the channel.
    pub fn poly_pressure_port(&mut self, port: u8, channel: u8, note: u8, value: u8) {
        for (e, c) in self.parts.iter_mut().zip(&self.controls) {
            if c.hears(port, channel) {
                e.poly_pressure(channel, note, value);
            }
        }
    }

    pub fn pitch_bend_port(&mut self, port: u8, channel: u8, value: u16) {
        for (e, c) in self.parts.iter_mut().zip(&self.controls) {
            if c.port == port {
                e.pitch_bend(channel, value);
            }
        }
    }

    /// Releases always reach every engine: changing routing while a key is held must not stick it.
    pub fn note_off(&mut self, channel: u8, note: u8) {
        for e in &mut self.parts {
            e.note_off(channel, note);
        }
    }

    pub fn cc(&mut self, channel: u8, cc: u8, value: u8) {
        for e in &mut self.parts {
            e.cc(channel, cc, value);
        }
    }
    pub fn panic(&mut self) {
        for e in &mut self.parts { e.panic(); }
    }

    pub fn pitch_bend(&mut self, channel: u8, value: u16) {
        for e in &mut self.parts {
            e.pitch_bend(channel, value);
        }
    }

    /// Render `frames` (at most [`MAX_BLOCK`]) into the bus blocks, after
    /// each bus's fader; [`BusControls::port`] says where each one plays.
    pub fn render(&mut self, frames: usize) -> &[Block; BUSES] {
        self.render_live(frames).0
    }

    /// [`render`](Self::render), also saying which buses carry signal: the
    /// others are silent and need no copying out.
    pub fn render_live(&mut self, frames: usize) -> (&[Block; BUSES], [bool; BUSES]) {
        let n = frames.min(MAX_BLOCK);
        for (bus, written) in self.buses.iter_mut().zip(&mut self.written) {
            bus[0][..*written].fill(0.0);
            bus[1][..*written].fill(0.0);
            *written = 0;
        }
        if self.tap.is_some() {
            self.tapped[..n].fill(0.0);
        }
        let solo = self.controls.iter().any(|c| c.solo);
        for (slot, ((engine, c), meter)) in self
            .parts
            .iter_mut()
            .zip(&self.controls)
            .zip(&mut self.peaks.parts)
            .enumerate()
        {
            let [left, right] = &mut self.part;
            engine.tune = c.tune;
            engine.render(&mut left[..n], &mut right[..n]);
            let (left, right) = (&mut left[..n], &mut right[..n]);
            if c.mute || (solo && !c.solo) {
                continue;
            }
            let [gl, gr] = balance(c.gain, c.pan);
            // Output channels past the instrument's own, after the part's fader.
            for (out, l, r) in engine.fx().direct_outs(n) {
                let bus = match c.outs[out] {
                    NO_AUX => c.output,
                    b => b,
                } as usize;
                let bus = bus.min(BUSES - 1);
                self.written[bus] = n;
                let [bus_l, bus_r] = &mut self.buses[bus];
                for (o, x) in bus_l[..n].iter_mut().zip(l) {
                    *o += x * gl;
                }
                for (o, x) in bus_r[..n].iter_mut().zip(r) {
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
                for ((t, l), r) in self.tapped[..n].iter_mut().zip(&*left).zip(&*right) {
                    *t = (l + r) * 0.5;
                }
            }
            let aux = (c.aux as usize) < BUSES && c.aux != c.output && c.aux_gain != 0.0;
            for (bus, gain) in [(c.output as usize, 1.0), (c.aux as usize, c.aux_gain)]
                .into_iter()
                .take(1 + usize::from(aux))
            {
                let bus = bus.min(BUSES - 1);
                self.written[bus] = n;
                let [bus_l, bus_r] = &mut self.buses[bus];
                for (out, x) in bus_l[..n].iter_mut().zip(&*left) {
                    *out += x * gain;
                }
                for (out, x) in bus_r[..n].iter_mut().zip(&*right) {
                    *out += x * gain;
                }
            }
        }
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

