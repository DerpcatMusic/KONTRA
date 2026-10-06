//! Independently routed engines mixed onto sixteen stereo output
//! buses (Kontakt's st.1…st.16), each with its own fader and host port.

use super::{Engine, MAX_BLOCK, voice::balance};

pub use crate::sound::{BUSES, Block, RACK_SLOTS, TUNE_RANGE};
pub use crate::sound::mix::{BusControls, Mix, NO_AUX, PartControls, Peaks};

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}


impl PartControls {
    fn hears(&self, port: u8, channel: u8) -> bool {
        self.port == port && (self.channel < 0 || self.channel == i16::from(channel))
    }
}

/// Rack storage is prepared outside rendering and adopted without allocating.
pub struct Rack {
    pub parts: Vec<Engine>,
    pub controls: Vec<PartControls>,
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
        Self::with_slots(RACK_SLOTS)
    }
}

impl Rack {
    /// Allocate on the worker before handing this storage to audio.
    pub fn with_slots(slots: usize) -> Self {
        Self {
            parts: (0..slots).map(|_| Engine::default()).collect(),
            controls: vec![PartControls::default(); slots],
            bus_controls: std::array::from_fn(|n| BusControls::on(n as u8)),
            peaks: Peaks { parts: vec![[0.0; 2]; slots], buses: [[0.0; 2]; BUSES] },
            tap: None,
            tapped: [0.0; MAX_BLOCK],
            part: [[0.0; MAX_BLOCK]; 2],
            buses: [[[0.0; MAX_BLOCK]; 2]; BUSES],
            written: [0; BUSES],
        }
    }
}

impl Rack {
    /// Adopt larger worker-prepared storage, keeping every current engine and
    /// its voices. Return the old containers in `prepared` for worker disposal.
    pub fn adopt_parts(&mut self, prepared: &mut Self) {
        assert!(prepared.parts.len() >= self.parts.len());
        for (current, next) in self.parts.iter_mut().zip(&mut prepared.parts) {
            std::mem::swap(current, next);
        }
        prepared.controls[..self.controls.len()].copy_from_slice(&self.controls);
        prepared.peaks.parts[..self.peaks.parts.len()].copy_from_slice(&self.peaks.parts);
        std::mem::swap(&mut self.parts, &mut prepared.parts);
        std::mem::swap(&mut self.controls, &mut prepared.controls);
        std::mem::swap(&mut self.peaks.parts, &mut prepared.peaks.parts);
    }

    pub fn reset(&mut self, rate: f64) {
        for e in &mut self.parts {
            e.reset(rate);
        }
    }

    pub fn set_controls(&mut self, mix: &Mix) {
        self.bus_controls = mix.buses;
        for (slot, (engine, old)) in self.parts.iter_mut().zip(&mut self.controls).enumerate() {
            let new = mix.parts.get(slot).copied().unwrap_or_default();
            if (old.port, old.channel) != (new.port, new.channel) {
                let rate = engine.rate();
                engine.reset(rate);
            }
            engine.set_home_channel(new.channel.clamp(0, 15) as u8);
            *old = new;
        }
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

