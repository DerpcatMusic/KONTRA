//! Live behaviour state: articulation switch, controller values, round-robin
//! counters. Deterministic: the same input gives the same voices, every run.

use kontra_ir::Join;
use kontra_plan::{Atom, Control, Plan, RoundRobin, RrCounter, Zone};

/// What a zone's start does to its round-robin counter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// An attack: the counter steps once after the note-on.
    Advance,
    /// A release trigger: it re-reads the attack's draw. A per-group counter
    /// reads its current count less one, so a note on the same group between
    /// the attack and the release moves it.
    // ponytail: the group count is not kept per note; store it in `RrDraw`
    // if a library's release samples must follow interleaved notes.
    Reuse,
}

/// Slots that may hold a loaded effect type at once.
pub const LOADED: usize = 256;

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
/// pick the same step as its attack; also the note's random modulation value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RrDraw {
    global: u32,
    key: u32,
    random: u32,
}

impl RrDraw {
    /// The note's random modulation value, 0..=1: the draw through the
    /// splitmix64 finalizer, so it does not follow the round-robin choice
    /// and the generator's sequence stays as it was.
    #[must_use]
    pub fn unit(self) -> f32 {
        let mut z = u64::from(self.random).wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
    }
}

pub struct Behaviour {
    /// The last key pressed inside a switch range.
    pub keyswitch: Option<u8>,
    /// One latch per [`Plan::switches`] range.
    switches: Vec<bool>,
    cc: [[u8; 128]; 16],
    /// Per channel, the controllers a message has set (bit `cc`).
    cc_set: [u128; 16],
    /// Pitch bend per channel, −1..=1.
    bend: [f32; 16],
    /// The channel the latest note, controller or bend came on: what routes
    /// no note owns read (the engine plays every channel).
    pub channel: u8,
    global: Counter,
    keys: [Counter; 128],
    /// (group, counter) for every group a round-robin zone names; built at prepare.
    groups: Vec<(u32, Counter)>,
    rng: u64,
    /// Per key, the sample its release-trigger counter last restarted: every
    /// attack, and `reset_rls_trig_counter` (Kontakt's system script runs it
    /// on each note-on that is not a release trigger).
    pub rls_reset: [u64; 128],
    /// Per [`ir::SlotShapes::slots`] entry, the shape index it
    /// last switched to.
    pub shapes: Vec<Option<u16>>,
    /// Slots and the [`ir::SlotShapes::types`] entry each was last loaded with.
    pub loaded: Vec<(kontra_ir::RackAt, u8, u16)>,
    /// Each slot with a [`kontra_ir::KnobModel`] and its knobs' positions
    /// as last set: the saved slots', then those of types a script loaded.
    pub knobs: Vec<(kontra_ir::RackAt, u8, Positions)>,
}

/// A [`kontra_ir::KnobModel`]'s live positions.
pub type Positions = [f32; kontra_ir::KnobModel::MAX];

/// `model`'s saved positions, as live ones.
#[must_use]
pub fn positions(model: &kontra_ir::KnobModel) -> Positions {
    let mut out = [0.0; kontra_ir::KnobModel::MAX];
    out.iter_mut().zip(&model.saved).for_each(|(o, s)| *o = *s);
    out
}

impl Behaviour {
    pub fn new(plan: &Plan) -> Self {
        // A counter for every group: `set_zone_par` may move a cycling zone
        // into a group that had none.
        let count = (plan.zones.iter().map(|z| z.group + 1).max().unwrap_or(0))
            .max(u32::try_from(plan.groups.len()).unwrap_or(u32::MAX));
        let groups: Vec<(u32, Counter)> = (0..count).map(|g| (g, Counter::default())).collect();
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
            channel: 0,
            global: Counter::default(),
            keys: [Counter::default(); 128],
            groups,
            rng: 0x9E37_79B9_7F4A_7C15,
            rls_reset: [0; 128],
            shapes: vec![None; plan.slot_shapes.slots.len()],
            loaded: Vec::with_capacity(LOADED.max(plan.loaded.len())),
            knobs: Vec::with_capacity(LOADED + plan.slot_models.len() + plan.loaded.len()),
        };
        // Every modelled slot starts at its model's saved positions: the
        // type a recalled project loaded there, else the slot's own.
        let slots = (plan.slot_models.iter().map(|m| (m.0, m.1)))
            .chain(plan.loaded.iter().map(|l| (l.0, l.1)));
        for (at, slot) in slots {
            let ty = plan
                .loaded
                .iter()
                .find(|l| (l.0, l.1) == (at, slot))
                .map(|l| l.2);
            if !behaviour.knobs.iter().any(|k| (k.0, k.1) == (at, slot))
                && let Some(model) = plan.knob_model(at, slot, ty)
            {
                behaviour.knobs.push((at, slot, positions(model)));
            }
        }
        behaviour.loaded.extend_from_slice(&plan.loaded);
        if let Some(key) = plan.default_keyswitch {
            behaviour.press(plan, key);
        }
        behaviour
    }

    /// Kontakt's key latch (KONTAKT_START_CONDITIONS, 0x144f17d28): a key
    /// inside the span of all switch ranges sets the ranges holding it and
    /// clears the rest, even in a gap; a key outside the span changes nothing.
    /// A note-on: its channel is the one last played, its key may switch.
    pub fn play(&mut self, plan: &Plan, key: u8, channel: u8) {
        self.channel = channel;
        self.press(plan, key);
    }

    pub fn press(&mut self, plan: &Plan, key: u8) {
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
    pub fn starts(&mut self, zone: &Zone, channel: u8, key: u8, draw: RrDraw, step: Step) -> bool {
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

    pub fn cc(&self, channel: u8, controller: u8) -> u8 {
        self.cc
            .get(usize::from(channel & 15))
            .and_then(|c| c.get(usize::from(controller & 127)))
            .copied()
            .unwrap_or(0)
    }

    /// Whether a message has set `controller` on `channel`.
    pub fn cc_arrived(&self, channel: u8, controller: u8) -> bool {
        self.cc_set
            .get(usize::from(channel & 15))
            .is_some_and(|bits| bits >> (controller & 127) & 1 == 1)
    }

    pub fn bend(&self, channel: u8) -> f32 {
        self.bend
            .get(usize::from(channel & 15))
            .copied()
            .unwrap_or(0.0)
    }

    pub fn set_bend(&mut self, channel: u8, value: f32) {
        if let Some(v) = self.bend.get_mut(usize::from(channel & 15)) {
            *v = value;
        }
    }

    pub fn set_cc(&mut self, channel: u8, controller: u8, value: u8) {
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
    pub fn draw(&mut self, key: u8) -> RrDraw {
        let random = self.random();
        RrDraw {
            global: self.global.count,
            key: self.keys.get(usize::from(key)).map_or(0, |c| c.count),
            random,
        }
    }

    /// xorshift64*: deterministic, allocation-free.
    pub fn random(&mut self) -> u32 {
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

    /// Whether `zone` plays on this draw. [`Step::Advance`] marks its counter to
    /// step after the note-on; release triggers re-read the attack's draw.
    fn round_robin_plays(
        &mut self,
        rr: &RoundRobin,
        group: u32,
        key: u8,
        draw: RrDraw,
        step: Step,
    ) -> bool {
        let length = rr.length.max(1);
        let Some(counter) = self.counter(rr, group, key) else {
            return true;
        };
        let advance = step == Step::Advance;
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
    pub fn advance(&mut self) {
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
    pub fn control(&self, control: Control, channel: u8, velocity: u8, key: u8) -> u8 {
        match control {
            Control::Velocity => velocity,
            Control::Key => key,
            Control::Cc(n) => self.cc(channel, n),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RrDraw;

    /// The random modulation sources' per-note value covers 0..1 evenly and
    /// does not follow the round-robin draw it is derived from.
    #[test]
    fn note_random_spans_the_unit_range() {
        let units: Vec<f32> = (0..4096u32)
            .map(|i| {
                RrDraw {
                    random: i.wrapping_mul(0x9E37_79B9),
                    ..RrDraw::default()
                }
                .unit()
            })
            .collect();
        assert!(units.iter().all(|u| (0.0..1.0).contains(u)));
        let mean = units.iter().sum::<f32>() / units.len() as f32;
        assert!((mean - 0.5).abs() < 0.02, "mean {mean}");
        for quarter in 0..4 {
            let lo = quarter as f32 / 4.0;
            let n = units
                .iter()
                .filter(|u| (lo..lo + 0.25).contains(*u))
                .count();
            assert!((900..1150).contains(&n), "quarter {quarter}: {n}");
        }
        let low_rr = |i: u32| i.wrapping_mul(0x9E37_79B9) % 2 == 0;
        let even = (0..4096u32)
            .filter(|&i| low_rr(i))
            .map(|i| units[i as usize])
            .sum::<f32>()
            / 2048.0;
        assert!((even - 0.5).abs() < 0.03, "even-step mean {even}");
    }
}
