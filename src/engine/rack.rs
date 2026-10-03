//! Independently routed engines mixed onto sixteen stereo output
//! buses (Kontakt's st.1…st.16), each with its own fader and host port.

use super::{Engine, MAX_BLOCK, voice::balance};
use super::source_delay::SourceDelay;
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
        self.render_live_with(frames, |_, _, _| false)
    }

    /// Render through the shared mixer, allowing a caller to replace each
    /// slot's stereo source. The callback receives cleared buffers and returns
    /// true when it rendered the slot; false uses the engine and its direct
    /// outputs. Muted and unsoloed sources still advance before mixing.
    pub fn render_live_with(
        &mut self,
        frames: usize,
        render_slot: impl FnMut(usize, &mut [f32], &mut [f32]) -> bool,
    ) -> (&[Block; BUSES], [bool; BUSES]) {
        self.render_live_with_delay(frames, render_slot, &mut [])
    }

    /// As [`render_live_with`](Self::render_live_with), with prepared latency
    /// for each legacy source, including its independent direct outputs.
    /// External sources already carry their latency and bypass these delays.
    /// Slots without supplied delay storage keep their original timing.
    pub fn render_live_with_delay(
        &mut self,
        frames: usize,
        mut render_slot: impl FnMut(usize, &mut [f32], &mut [f32]) -> bool,
        delays: &mut [SourceDelay],
    ) -> (&[Block; BUSES], [bool; BUSES]) {
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
            left[..n].fill(0.0);
            right[..n].fill(0.0);
            let external = render_slot(slot, &mut left[..n], &mut right[..n]);
            if !external {
                engine.render(&mut left[..n], &mut right[..n]);
            }
            let delay = delays.get_mut(slot).filter(|delay| !external && delay.delay() != 0);
            let delay = delay.map(|delay| {
                delay.process(&mut left[..n], &mut right[..n], engine.fx().direct_outs(n));
                delay
            });
            let (left, right) = (&mut left[..n], &mut right[..n]);
            if c.mute || (solo && !c.solo) {
                continue;
            }
            let [gl, gr] = balance(c.gain, c.pan);
            // Output channels past the instrument's own, after the part's fader.
            let direct = engine.fx().direct_outs(n).filter(|_| !external && delay.is_none())
                .chain(delay.as_deref().into_iter().flat_map(|delay| delay.direct_outs(n)));
            for (out, l, r) in direct {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_stereo_uses_part_bus_send_meter_and_tap_once() {
        let mut rack = Rack::with_slots(2);
        rack.controls[0] = PartControls {
            output: 2,
            gain: 0.5,
            pan: 0.5,
            aux: 5,
            aux_gain: 0.25,
            ..Default::default()
        };
        rack.bus_controls[2].gain = 0.5;
        rack.bus_controls[2].pan = -0.5;
        rack.bus_controls[2].port = 7;
        rack.tap = Some(0);
        let mut calls = 0;
        let (buses, live) = rack.render_live_with(MAX_BLOCK + 1, |slot, left, right| {
            calls += 1;
            assert_eq!(left.len(), MAX_BLOCK);
            assert!(left.iter().chain(&*right).all(|&x| x == 0.0));
            if slot == 0 {
                left.fill(0.8);
                right.fill(-0.4);
                true
            } else {
                // A declined override cannot leak into the legacy renderer.
                left.fill(99.0);
                right.fill(99.0);
                false
            }
        });
        assert_eq!(calls, 2);
        assert_eq!(live, std::array::from_fn(|bus| bus == 2 || bus == 5));
        assert_eq!([buses[2][0][0], buses[2][1][0]], [0.1, -0.05]);
        assert_eq!([buses[5][0][0], buses[5][1][0]], [0.05, -0.05]);
        assert_eq!(rack.bus_controls[2].port, 7);
        assert_eq!(rack.peaks.parts, vec![[0.2, 0.2], [0.0, 0.0]]);
        assert_eq!(rack.peaks.buses[2], [0.1, 0.05]);
        assert_eq!(rack.peaks.buses[5], [0.05, 0.05]);
        assert!(rack.tapped.iter().all(|&x| x == 0.0));

        // A shorter silent block also clears the previously written tail.
        let (buses, live) = rack.render_live_with(3, |_, _, _| true);
        assert_eq!(live, [false; BUSES]);
        assert!(buses.iter().flatten().flatten().all(|&x| x == 0.0));
    }

    #[test]
    fn external_sources_advance_through_part_and_bus_mute_solo() {
        let mut rack = Rack::with_slots(3);
        for (slot, controls) in rack.controls.iter_mut().enumerate() {
            controls.output = slot as u8;
        }
        rack.controls[1].solo = true;
        rack.tap = Some(1);
        let source = |_: usize, left: &mut [f32], right: &mut [f32]| {
            left.fill(0.5);
            right.fill(0.25);
            true
        };
        let mut calls = 0;
        let (buses, live) = rack.render_live_with(4, |slot, left, right| {
            calls += 1;
            source(slot, left, right)
        });
        assert_eq!(calls, 3, "unsoloed sources keep their clocks running");
        assert_eq!(live, std::array::from_fn(|bus| bus == 1));
        assert_eq!([buses[1][0][3], buses[1][1][3]], [0.5, 0.25]);
        assert_eq!(rack.tapped[..4], [0.375; 4]);

        rack.controls[1].mute = true;
        let (_, live) = rack.render_live_with(4, source);
        assert_eq!(live, [false; BUSES]);
        assert_eq!(rack.tapped[..4], [0.0; 4]);

        rack.controls[1].mute = false;
        rack.controls[1].solo = false;
        rack.bus_controls[2].solo = true;
        let (buses, _) = rack.render_live_with(4, source);
        assert_eq!(
            [buses[0][0][0], buses[1][0][0], buses[2][0][0]],
            [0.0, 0.0, 0.5]
        );
        rack.bus_controls[2].mute = true;
        let (buses, _) = rack.render_live_with(4, source);
        assert!(buses.iter().flatten().flatten().all(|&x| x == 0.0));
    }

    fn direct_engine() -> Engine {
        routed_engine(false)
    }

    fn routed_engine(with_main: bool) -> Engine {
        use crate::{
            audio::Sample,
            engine::Bank,
            import::{Group, Loop, Zone},
            ksp::{LogEngine, Runtime},
        };
        let group_count = 1 + usize::from(with_main);
        let bank = Bank::from_samples(
            vec![Group::default(); group_count],
            (0..group_count)
                .map(|group| Zone {
                    group,
                    loop_range: Some(Loop {
                        start: 0,
                        end: 64,
                        alternating: false,
                        until_release: false,
                        crossfade: 0,
                    }),
                    ..Default::default()
                })
                .collect(),
            vec![(
                std::path::PathBuf::new(),
                Sample {
                    rate: 48000,
                    frames: vec![[0.5, 0.25]; 64],
                },
            )],
        )
        .unwrap();
        let (script, errors) = Runtime::with_scripts(
            &["on note\nset_engine_par($ENGINE_PAR_OUTPUT_CHANNEL,1,0,-1,-1)\nend on"],
            &mut LogEngine::default(),
            2,
            Vec::new(),
        );
        assert!(errors.iter().all(Option::is_none));
        let mut engine = Engine::default();
        engine.attack = 0.0001;
        engine.set_bank(Some(Box::new(bank)));
        engine.set_fx(crate::fx::ProgramFx::default().processor(48000.0, MAX_BLOCK));
        engine.set_script(Some(Box::new(script)));
        engine.note_on(0, 60, 127);
        engine
    }

    #[test]
    fn declined_external_source_preserves_kontakt_direct_outputs() {
        let setup = || {
            let mut rack = Rack::with_slots(1);
            rack.parts[0] = direct_engine();
            rack.controls[0].gain = 0.5;
            rack.controls[0].outs[1] = 4;
            rack.controls[0].output = 3;
            rack.tap = Some(0);
            rack
        };
        let (mut legacy, mut hooked) = (setup(), setup());
        for frames in [32, 7, 0, MAX_BLOCK] {
            let (expected, expected_live) = legacy.render_live(frames);
            let (actual, live) = hooked.render_live_with(frames, |_, left, right| {
                left.fill(99.0);
                right.fill(99.0);
                false
            });
            assert_eq!(actual, expected);
            assert_eq!(live, expected_live);
            if frames > 0 {
                assert_eq!(
                    [actual[4][0][frames - 1], actual[4][1][frames - 1]],
                    [0.25, 0.125]
                );
            }
            assert_eq!(hooked.peaks, legacy.peaks);
            assert_eq!(hooked.tapped, legacy.tapped);
        }
        // Old Kontakt direct buffers must not accompany the replacement.
        let (buses, live) = hooked.render_live_with(8, |_, left, right| {
            left.fill(0.8);
            right.fill(0.4);
            true
        });
        assert_eq!(live, std::array::from_fn(|bus| bus == 3));
        assert_eq!([buses[3][0][0], buses[3][1][0]], [0.4, 0.2]);
        assert_eq!([buses[4][0][0], buses[4][1][0]], [0.0, 0.0]);
    }

    #[cfg(feature = "plugin")]
    #[test]
    fn external_mix_callback_does_not_allocate() {
        let mut rack = Rack::with_slots(2);
        rack.controls[0].aux = 1;
        rack.controls[0].aux_gain = 0.25;
        rack.tap = Some(0);
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                for _ in 0..8 {
                    rack.render_live_with(MAX_BLOCK, |_, left, right| {
                        left.fill(0.5);
                        right.fill(0.25);
                        true
                    });
                }
            }),
            0
        );
    }

    #[test]
    fn legacy_source_delay_aligns_main_direct_aux_meters_and_tap() {
        let setup = || {
            let mut rack = Rack::with_slots(1);
            rack.parts[0] = routed_engine(true);
            rack.controls[0] = PartControls {
                output: 2,
                gain: 0.5,
                aux: 5,
                aux_gain: 0.25,
                ..Default::default()
            };
            rack.controls[0].outs[1] = 4;
            rack.bus_controls[2].gain = 0.5;
            rack.tap = Some(0);
            rack
        };
        let (mut reference, mut delayed) = (setup(), setup());
        let mut delays = [SourceDelay::new(17).unwrap()];
        assert!(delays[0].set_delay(17));
        let mut expected = Vec::new();
        let mut actual = Vec::new();
        for n in [1, 7, MAX_BLOCK, 3, MAX_BLOCK] {
            let buses = reference.render(n);
            for i in 0..n {
                expected.push([
                    buses[2][0][i],
                    buses[2][1][i],
                    buses[4][0][i],
                    buses[4][1][i],
                    buses[5][0][i],
                    buses[5][1][i],
                ]);
            }
            let (buses, _) = delayed.render_live_with_delay(n, |_, _, _| false, &mut delays);
            for i in 0..n {
                actual.push([
                    buses[2][0][i],
                    buses[2][1][i],
                    buses[4][0][i],
                    buses[4][1][i],
                    buses[5][0][i],
                    buses[5][1][i],
                ]);
            }
        }
        assert!(actual[..17].iter().all(|v| *v == [0.0; 6]));
        assert_eq!(actual[17..], expected[..expected.len() - 17]);
        assert_eq!(delayed.peaks, reference.peaks);
        assert_eq!(delayed.tapped[MAX_BLOCK - 1], 0.1875);
        assert_eq!(
            actual.last().unwrap(),
            &[0.125, 0.0625, 0.25, 0.125, 0.0625, 0.03125]
        );

        // Muting advances queued sources; it does not pause the FIFO.
        delayed.controls[0].mute = true;
        delayed.parts[0].panic();
        for _ in 0..32 {
            delayed.render_live_with_delay(MAX_BLOCK, |_, _, _| false, &mut delays);
        }
        delayed.controls[0].mute = false;
        let (buses, live) = delayed.render_live_with_delay(MAX_BLOCK, |_, _, _| false, &mut delays);
        assert_eq!(live, [false; BUSES]);
        assert!(buses.iter().flatten().flatten().all(|&x| x == 0.0));
    }

    #[test]
    fn zero_delay_is_legacy_identical_and_external_latency_is_not_added_twice() {
        let setup = || {
            let mut rack = Rack::with_slots(1);
            rack.parts[0] = routed_engine(true);
            rack.controls[0].outs[1] = 4;
            rack.tap = Some(0);
            rack
        };
        let (mut legacy, mut supplied) = (setup(), setup());
        let mut delays = [SourceDelay::new(513).unwrap()];
        for n in [1, 32, 0, MAX_BLOCK, 7] {
            let (expected, expected_live) = legacy.render_live(n);
            let (actual, live) = supplied.render_live_with_delay(n, |_, _, _| false, &mut delays);
            for (a, b) in actual
                .iter()
                .flatten()
                .flatten()
                .zip(expected.iter().flatten().flatten())
            {
                assert_eq!(a.to_bits(), b.to_bits());
            }
            assert_eq!(live, expected_live);
            assert_eq!(supplied.peaks, legacy.peaks);
            assert_eq!(supplied.tapped, legacy.tapped);
        }
        assert!(delays[0].set_delay(513));
        let (buses, live) = supplied.render_live_with_delay(
            1,
            |_, left, right| {
                left[0] = 0.75;
                right[0] = -0.25;
                true
            },
            &mut delays,
        );
        assert_eq!([buses[0][0][0], buses[0][1][0]], [0.75, -0.25]);
        assert_eq!(live, std::array::from_fn(|bus| bus == 0));
    }

    #[cfg(feature = "plugin")]
    #[test]
    fn prepared_legacy_source_and_direct_delay_render_without_allocations() {
        let mut rack = Rack::with_slots(1);
        rack.parts[0] = routed_engine(true);
        rack.controls[0].aux = 2;
        rack.controls[0].aux_gain = 0.25;
        rack.controls[0].outs[1] = 4;
        rack.tap = Some(0);
        let mut delays = [SourceDelay::new(513).unwrap()];
        assert!(delays[0].set_delay(513));
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                for _ in 0..16 {
                    rack.render_live_with_delay(MAX_BLOCK, |_, _, _| false, &mut delays);
                }
            }),
            0
        );
    }
}
