//! Sixteen independently routed engines mixed onto eight stereo buses.

use super::{Engine, MAX_BLOCK, voice::balance};

pub const RACK_SLOTS: usize = 16;
pub const BUSES: usize = 8;

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
    part: Block,
    buses: [Block; BUSES],
}

impl Default for Rack {
    fn default() -> Self {
        Self {
            parts: std::array::from_fn(|_| Engine::default()),
            controls: [PartControls::default(); RACK_SLOTS],
            part: [[0.0; MAX_BLOCK]; 2],
            buses: [[[0.0; MAX_BLOCK]; 2]; BUSES],
        }
    }
}

impl Rack {
    pub fn reset(&mut self, rate: f64) {
        for e in &mut self.parts {
            e.reset(rate);
        }
    }

    pub fn set_controls(&mut self, controls: [PartControls; RACK_SLOTS]) {
        for ((engine, old), new) in self.parts.iter_mut().zip(&self.controls).zip(&controls) {
            if (old.port, old.channel) != (new.port, new.channel) {
                let rate = engine.rate();
                engine.reset(rate);
            }
        }
        self.controls = controls;
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

    pub fn pitch_bend(&mut self, channel: u8, value: u16) {
        for e in &mut self.parts {
            e.pitch_bend(channel, value);
        }
    }

    /// Render `frames` (at most [`MAX_BLOCK`]) into the bus blocks.
    pub fn render(&mut self, frames: usize) -> &[Block; BUSES] {
        let n = frames.min(MAX_BLOCK);
        for bus in &mut self.buses {
            bus[0][..n].fill(0.0);
            bus[1][..n].fill(0.0);
        }
        let solo = self.controls.iter().any(|c| c.solo);
        for (engine, c) in self.parts.iter_mut().zip(&self.controls) {
            let [left, right] = &mut self.part;
            engine.tune = c.tune;
            engine.render(&mut left[..n], &mut right[..n]);
            if c.mute || (solo && !c.solo) {
                continue;
            }
            let [gl, gr] = balance(c.gain, c.pan);
            let [bus_l, bus_r] = &mut self.buses[(c.output as usize).min(BUSES - 1)];
            for (out, x) in bus_l[..n].iter_mut().zip(&left[..n]) {
                *out += x * gl;
            }
            for (out, x) in bus_r[..n].iter_mut().zip(&right[..n]) {
                *out += x * gr;
            }
        }
        &self.buses
    }
}
