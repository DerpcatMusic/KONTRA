//! An instrument bus: what it carries mixed, then its rack, fader, sends,
//! balance and meter. The audio graph runs it as an
//! [`ir::GraphNodeKind::InstrumentBus`] node.

use kontra_effects::MAX_SPAN;
use kontra_effects::matrix::balance;
use kontra_ir as ir;
use kontra_plan as plan;

use crate::rack::{Live, Rack};

/// A bus: what it carries mixed, then its rack, fader and balance.
pub struct BusState {
    pub gain: f32,
    pub enabled: bool,
    /// Its rack, on what it carries.
    pub rack: Rack,
    /// -1..=1 balance after the rack.
    pub pan: f32,
    /// Pan and volume changes glide as measured natively: each channel's
    /// balance gain at most 1 per 50 ms, the volume a one-pole of 3.7 ms.
    /// An effect's bypass switches at once.
    pub balance: kontra_dsp::core::Fader,
    pub fader: kontra_dsp::core::Fader,
    /// Later buses it feeds, and their levels.
    pub sends: Box<[ir::BusSend]>,
    pub to_main: bool,
    pub mix: Box<[[f32; 2]]>,
    /// A send's scale at each frame of the span, where an oscillator moves it.
    pub scales: Box<[f32]>,
    /// Its output's left and right peaks since they were last taken.
    pub peak: [f32; 2],
    /// The gain and pan its faders last aimed at, and the frames they have
    /// held since; past `settle` frames both faders rest.
    aim: [f32; 2],
    still: u32,
    settle: u32,
}

/// Longer than either fader takes to land (the 3.7 ms volume one-pole
/// reaches its fixed point, the balance moves at most 1 per 50 ms).
const SETTLE_S: f32 = 0.25;

impl BusState {
    #[must_use]
    pub fn new(b: &plan::Bus, rate: u32) -> Self {
        Self {
            gain: b.gain,
            enabled: b.enabled,
            rack: Rack::routed(
                &b.inserts,
                rate,
                kontra_effects::processors::Responses(&[], &[]),
                &b.mods,
                &b.blends,
            )
            .with_lfos(&b.lfos),
            pan: b.pan,
            balance: kontra_dsp::core::Fader::new(rate, 0.0, 50.0),
            fader: kontra_dsp::core::Fader::new(rate, 3.7, 0.0),
            sends: b.sends.clone().into(),
            to_main: b.to_main,
            mix: vec![[0.0; 2]; MAX_SPAN as usize].into(),
            scales: vec![1.0; MAX_SPAN as usize].into(),
            peak: [0.0; 2],
            aim: [b.gain, b.pan],
            still: 0,
            settle: (SETTLE_S * rate as f32) as u32,
        }
    }

    /// Whether this span of `len` frames leaves the bus as it is: its mix
    /// (after the rack) is silent and its faders rest, so the fader, sends,
    /// balance and meter would change neither a sample nor their state.
    /// The meter here is only the peak since the last take (a silent span
    /// leaves it as it is); its fall is the editor's, fed by those takes,
    /// so an idle bus's meter keeps falling.
    pub fn idle(&mut self, len: usize) -> bool {
        let mix = self.mix.get(..len).unwrap_or_default();
        let aim = [self.gain, self.pan];
        if aim != self.aim {
            (self.aim, self.still) = (aim, 0);
        }
        let resting = self.still >= self.settle;
        let frames = u32::try_from(len).unwrap_or(u32::MAX);
        self.still = self.still.saturating_add(frames);
        resting && mix.iter().all(|m| *m == [0.0; 2])
    }
}

/// Runs bus `at` of `buses` over the first `len` frames of its mix: its
/// rack (writing the instrument's send `taps`), then, unless it idles,
/// pre-fader sends, the fader's glide, post-fader sends, the balance and the
/// meter. A send reaches only a later bus (an earlier one would hear it a
/// span late: none). Allocates nothing.
pub fn run(
    buses: &mut [BusState],
    at: usize,
    live: &Live,
    taps: &mut [Box<[[f32; 2]]>],
    rate: u32,
    len: usize,
) {
    let (done, later) = buses.split_at_mut((at + 1).min(buses.len()));
    let Some(bus) = done.get_mut(at) else {
        return;
    };
    if let Some(mix) = bus.mix.get_mut(..len) {
        bus.rack.follow(live);
        bus.rack.process(mix, taps, live.tempo);
    }
    if bus.idle(len) {
        return;
    }
    let Some(mix) = bus.mix.get_mut(..len) else {
        return;
    };
    for post in [false, true] {
        if post {
            bus.fader.process(mix, bus.gain, [1.0; 2]);
        }
        let sends = bus.sends.iter().enumerate();
        for (n, send) in sends.filter(|(_, s)| s.post_fader == post) {
            let Some(to) = send.bus.checked_sub(at + 1).and_then(|k| later.get_mut(k)) else {
                continue;
            };
            if bus.rack.send_moves(n) {
                // An oscillator moves the level every frame, so the result
                // does not depend on where blocks split.
                let scales = bus.scales.get_mut(..len).unwrap_or_default();
                bus.rack.send_scales(live, n, 1.0 / rate as f32, scales);
                for ((o, m), g) in to.mix.iter_mut().zip(mix.iter()).zip(scales.iter()) {
                    let level = send.level * g;
                    o[0] += m[0] * level;
                    o[1] += m[1] * level;
                }
                continue;
            }
            // ponytail: other moved levels step once a span; ramp if a ride zippers.
            let level = send.level * bus.rack.send_scale(n);
            for (o, m) in to.mix.iter_mut().zip(mix.iter()) {
                o[0] += m[0] * level;
                o[1] += m[1] * level;
            }
        }
    }
    let [[l, _], [_, r]] = balance(bus.pan);
    bus.balance.process(mix, 1.0, [l, r]);
    for m in mix.iter() {
        bus.peak = [bus.peak[0].max(m[0].abs()), bus.peak[1].max(m[1].abs())];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bus_idles_only_on_silence_once_its_faders_rest() {
        let plan = plan::Plan::new(Vec::new(), 1);
        let rate = 48_000;
        let mut bus = BusState::new(plan.buses.first().expect("one bus"), rate);
        let len = MAX_SPAN as usize;
        let rest = (SETTLE_S * rate as f32) as usize / len + 1;
        assert!(
            (0..rest).all(|_| !bus.idle(len)),
            "idles before the faders land"
        );
        assert!(bus.idle(len));
        bus.mix.fill([0.5; 2]);
        assert!(!bus.idle(len), "sound passes");
        bus.mix.fill([0.0; 2]);
        bus.gain = 0.5;
        assert!(!bus.idle(len), "a moved fader glides first");
    }
}
