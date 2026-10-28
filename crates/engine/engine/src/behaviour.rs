//! Live behaviour state: articulation switch, controller values, round-robin
//! counters. Deterministic: the same input gives the same voices, every run.

use kontra_ir::Join;

use crate::plan::{Atom, Control, Plan, RoundRobin, RrCounter, Zone};

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
    /// The last key pressed inside a switch range.
    pub(crate) keyswitch: Option<u8>,
    /// One latch per [`Plan::switches`] range.
    switches: Vec<bool>,
    cc: [[u8; 128]; 16],
    /// Per channel, the controllers a message has set (bit `cc`).
    cc_set: [u128; 16],
    /// Pitch bend per channel, −1..=1.
    bend: [f32; 16],
    global: Counter,
    keys: [Counter; 128],
    /// (group, counter) for every group a round-robin zone names; built at prepare.
    groups: Vec<(u32, Counter)>,
    rng: u64,
}

impl Behaviour {
    pub(crate) fn new(plan: &Plan) -> Self {
        let mut groups: Vec<(u32, Counter)> = Vec::new();
        for zone in &plan.zones {
            let cycles = zone
                .start
                .iter()
                .any(|(_, a)| matches!(a, Atom::RoundRobin(_)));
            if cycles && !groups.iter().any(|(g, _)| *g == zone.group) {
                groups.push((zone.group, Counter::default()));
            }
        }
        // Every controller reads 0 until a message sets it, as in a fresh
        // Kontakt 8 (measured: %CC 1, 2, 7, 10, 11, 21, 64, 100 all 0); a
        // source that starts elsewhere says so (`Source::ControllerFrom`).
        let cc = [[0u8; 128]; 16];
        let mut behaviour = Self {
            keyswitch: None,
            switches: vec![false; plan.switches.len()],
            cc,
            cc_set: [0; 16],
            bend: [0.0; 16],
            global: Counter::default(),
            keys: [Counter::default(); 128],
            groups,
            rng: 0x9E37_79B9_7F4A_7C15,
        };
        if let Some(key) = plan.default_keyswitch {
            behaviour.press(plan, key);
        }
        behaviour
    }

    /// Kontakt's key latch (KONTAKT_START_CONDITIONS, 0x144f17d28): a key
    /// inside the span of all switch ranges sets the ranges holding it and
    /// clears the rest, even in a gap; a key outside the span changes nothing.
    pub(crate) fn press(&mut self, plan: &Plan, key: u8) {
        let lo = plan.switches.iter().map(|r| *r.start()).min();
        let hi = plan.switches.iter().map(|r| *r.end()).max();
        if lo.is_none_or(|lo| key < lo) || hi.is_none_or(|hi| key > hi) {
            return;
        }
        for (latch, range) in self.switches.iter_mut().zip(&plan.switches) {
            *latch = range.contains(&key);
        }
        if plan.switches.iter().any(|r| r.contains(&key)) {
            self.keyswitch = Some(key);
        }
    }

    /// Whether `zone`'s start conditions hold for this note.
    pub(crate) fn starts(
        &mut self,
        zone: &Zone,
        channel: u8,
        key: u8,
        draw: RrDraw,
        step: bool,
    ) -> bool {
        let mut result = true;
        for (join, atom) in &zone.start {
            let value = match atom {
                Atom::Switch(i) => self.switches.get(usize::from(*i)).copied() == Some(true),
                Atom::Cc(cc, range) => range.contains(&self.cc(channel, *cc)),
                Atom::RoundRobin(rr) => self.round_robin_plays(rr, zone.group, key, draw, step),
            };
            result = match join {
                Join::And => result && value,
                Join::AndNot => result && !value,
                Join::Or => result || value,
            };
        }
        result
    }

    pub(crate) fn cc(&self, channel: u8, controller: u8) -> u8 {
        self.cc
            .get(usize::from(channel & 15))
            .and_then(|c| c.get(usize::from(controller & 127)))
            .copied()
            .unwrap_or(0)
    }

    /// Whether a message has set `controller` on `channel`.
    pub(crate) fn cc_arrived(&self, channel: u8, controller: u8) -> bool {
        self.cc_set
            .get(usize::from(channel & 15))
            .is_some_and(|bits| bits >> (controller & 127) & 1 == 1)
    }

    pub(crate) fn bend(&self, channel: u8) -> f32 {
        self.bend
            .get(usize::from(channel & 15))
            .copied()
            .unwrap_or(0.0)
    }

    pub(crate) fn set_bend(&mut self, channel: u8, value: f32) {
        if let Some(v) = self.bend.get_mut(usize::from(channel & 15)) {
            *v = value;
        }
    }

    pub(crate) fn set_cc(&mut self, channel: u8, controller: u8, value: u8) {
        if let Some(v) = self
            .cc
            .get_mut(usize::from(channel & 15))
            .and_then(|c| c.get_mut(usize::from(controller & 127)))
        {
            *v = value;
        }
        if let Some(bits) = self.cc_set.get_mut(usize::from(channel & 15)) {
            *bits |= 1 << (controller & 127);
        }
    }

    /// One draw per note-on: the counters it reads and a fresh random value.
    pub(crate) fn draw(&mut self, key: u8) -> RrDraw {
        let random = self.random();
        RrDraw {
            global: self.global.count,
            key: self.keys.get(usize::from(key)).map_or(0, |c| c.count),
            random,
        }
    }

    /// xorshift64*: deterministic, allocation-free.
    pub(crate) fn random(&mut self) -> u32 {
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        (self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }

    fn counter(&mut self, rr: &RoundRobin, group: u32, key: u8) -> Option<&mut Counter> {
        match rr.counter {
            RrCounter::Global => Some(&mut self.global),
            RrCounter::Key => self.keys.get_mut(usize::from(key)),
            RrCounter::Group => self
                .groups
                .iter_mut()
                .find(|(g, _)| *g == group)
                .map(|(_, c)| c),
        }
    }

    /// Whether `zone` plays on this draw. With `step`, marks its counter to step
    /// after the note-on; release triggers pass false to re-read the attack's draw.
    fn round_robin_plays(
        &mut self,
        rr: &RoundRobin,
        group: u32,
        key: u8,
        draw: RrDraw,
        step: bool,
    ) -> bool {
        let length = rr.length.max(1);
        let Some(counter) = self.counter(rr, group, key) else {
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

    /// Steps every counter a note-on read, once. The global counter is
    /// Kontakt's: it steps on every note-on, matched or not.
    pub(crate) fn advance(&mut self) {
        self.global.used = true;
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
    pub(crate) fn control(&self, control: Control, channel: u8, velocity: u8, key: u8) -> u8 {
        match control {
            Control::Velocity => velocity,
            Control::Key => key,
            Control::Cc(n) => self.cc(channel, n),
        }
    }
}
