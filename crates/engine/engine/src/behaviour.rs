//! Live behaviour state: articulation switch, controller values, round-robin
//! counters. Deterministic: the same input gives the same voices, every run.

use crate::plan::{Control, Plan, RoundRobin, RrCounter, Zone};

/// Round-robin state of one counter.
#[derive(Clone, Copy, Default)]
struct Counter {
    count: u32,
    last_random: Option<u8>,
    /// The random step this note-on chose; becomes `last_random` when it ends.
    pending: Option<u8>,
    /// Set by a note-on that read this counter; it steps once afterwards.
    used: bool,
}

/// The round-robin values one note-on read, kept so its release triggers
/// pick the same step as its attack.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RrDraw {
    global: u32,
    key: u32,
    random: u32,
}

pub(crate) struct Behaviour {
    pub(crate) keyswitch: Option<u8>,
    cc: [[u8; 128]; 16],
    global: Counter,
    keys: [Counter; 128],
    /// (group, counter) for every group a round-robin zone names; built at prepare.
    groups: Vec<(u32, Counter)>,
    rng: u64,
}

impl Behaviour {
    pub(crate) fn new(plan: &Plan) -> Self {
        let mut groups: Vec<(u32, Counter)> = Vec::new();
        for zone in plan.zones.iter().filter(|z| z.round_robin.is_some()) {
            if !groups.iter().any(|(g, _)| *g == zone.group) {
                groups.push((zone.group, Counter::default()));
            }
        }
        let mut cc = [[0u8; 128]; 16];
        for channel in &mut cc {
            // MIDI defaults a fresh instrument expects: volume 100, expression full.
            channel[7] = 100;
            channel[11] = 127;
        }
        Self {
            keyswitch: plan.default_keyswitch,
            cc,
            global: Counter::default(),
            keys: [Counter::default(); 128],
            groups,
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }

    pub(crate) fn cc(&self, channel: u8, controller: u8) -> u8 {
        self.cc
            .get(usize::from(channel & 15))
            .and_then(|c| c.get(usize::from(controller & 127)))
            .copied()
            .unwrap_or(0)
    }

    pub(crate) fn set_cc(&mut self, channel: u8, controller: u8, value: u8) {
        if let Some(v) = self
            .cc
            .get_mut(usize::from(channel & 15))
            .and_then(|c| c.get_mut(usize::from(controller & 127)))
        {
            *v = value;
        }
    }

    /// One draw per note-on: the counters it reads and a fresh random value.
    pub(crate) fn draw(&mut self, key: u8) -> RrDraw {
        // xorshift64*: deterministic, allocation-free.
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let random = (self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32;
        RrDraw {
            global: self.global.count,
            key: self.keys.get(usize::from(key)).map_or(0, |c| c.count),
            random,
        }
    }

    fn counter(&mut self, rr: &RoundRobin, zone: &Zone, key: u8) -> Option<&mut Counter> {
        match rr.counter {
            RrCounter::Global => Some(&mut self.global),
            RrCounter::Key => self.keys.get_mut(usize::from(key)),
            RrCounter::Group => self
                .groups
                .iter_mut()
                .find(|(g, _)| *g == zone.group)
                .map(|(_, c)| c),
        }
    }

    /// Whether `zone` plays on this draw. With `step`, marks its counter to step
    /// after the note-on; release triggers pass false to re-read the attack's draw.
    pub(crate) fn round_robin_plays(
        &mut self,
        zone: &Zone,
        key: u8,
        draw: RrDraw,
        step: bool,
    ) -> bool {
        let Some(rr) = zone.round_robin else {
            return true;
        };
        let length = rr.length.max(1);
        let Some(counter) = self.counter(&rr, zone, key) else {
            return true;
        };
        let advance = step;
        counter.used |= advance;
        let step = if rr.random {
            let mut step = (draw.random % u32::from(length)) as u8;
            if length > 1 && counter.last_random == Some(step) {
                step = (step + 1 + ((draw.random >> 16) % u32::from(length - 1)) as u8) % length;
            }
            step
        } else {
            let count = match rr.counter {
                RrCounter::Global => draw.global,
                RrCounter::Key => draw.key,
                RrCounter::Group if advance => counter.count,
                RrCounter::Group => counter.count.wrapping_sub(1),
            };
            (count % u32::from(length)) as u8
        };
        if rr.random && advance {
            counter.pending = Some(step);
        }
        step == rr.position
    }

    /// Steps every counter a note-on read, once.
    pub(crate) fn advance(&mut self) {
        let all = std::iter::once(&mut self.global)
            .chain(self.keys.iter_mut())
            .chain(self.groups.iter_mut().map(|(_, c)| c));
        for counter in all.filter(|c| c.used) {
            counter.used = false;
            counter.count = counter.count.wrapping_add(1);
            if let Some(step) = counter.pending.take() {
                counter.last_random = Some(step);
            }
        }
    }

    /// The value a crossfade or range reads for a note.
    pub(crate) fn control(&self, control: Control, channel: u8, velocity: u8) -> u8 {
        match control {
            Control::Velocity => velocity,
            Control::Cc(n) => self.cc(channel, n),
        }
    }
}
