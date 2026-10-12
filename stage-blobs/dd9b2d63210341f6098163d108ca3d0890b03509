//! Prepared native Group system-slot predicates. No zone query advances clocks.
use super::GroupMask;
use crate::import::Group;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Predicate {
    #[default]
    Always,
    Key(i16, i16),
    Controller(u8, i16, i16),
    RoundRobin(i32),
    Random(u32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Row { predicate: Predicate, next: i32 }

#[derive(Debug, PartialEq, Eq)]
struct Criteria { rows: [Row; 4], unsupported: bool, release: bool }

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Prepared {
    groups: Vec<Criteria>,
    native: bool,
    key_span: Option<(i16, i16)>,
    initial_key: i16,
    round_robin_count: u32,
    random_count: u32,
}

impl Prepared {
    pub(super) fn new(groups: &[Group], default_key: Option<i16>, native_program: bool,
        mut diagnostic: impl FnMut(usize, &'static str)) -> Self {
        let mut prepared = Self { groups: Vec::with_capacity(groups.len()), native: native_program,
            key_span: None, initial_key: default_key.unwrap_or(-1), round_robin_count: 0, random_count: 0 };
        for (index, group) in groups.iter().enumerate() {
            let list = &group.start_criteria;
            prepared.native |= list.mask != 0 || !list.items.is_empty() || !list.unknown_tail.is_empty();
            let mut criteria = Criteria { rows: [Row::default(); 4], unsupported: false, release: group.release_trigger };
            let mut reject = |reason| { criteria.unsupported = true; diagnostic(index, reason); };
            if list.mask > 15 || list.mask.count_ones() as usize != list.items.len() {
                reject("record mask and row count disagree");
            }
            if !list.unknown_tail.is_empty() { reject("opaque trailing start-criteria data"); }
            let mut items = list.items.iter();
            for slot in 0..4 {
                if list.mask & (1 << slot) == 0 { continue; }
                let Some(row) = items.next() else { break; };
                if !(0..=2).contains(&row.next_criteria) { reject("unknown Boolean operator"); }
                let predicate = match row.mode {
                    0 => Predicate::Always,
                    1 => {
                        prepared.key_span = Some(prepared.key_span.map_or((row.key_min, row.key_max), |(lo, hi)|
                            (lo.min(row.key_min), hi.max(row.key_max))));
                        Predicate::Key(row.key_min, row.key_max)
                    },
                    2 if (0..130).contains(&row.controller) => Predicate::Controller(row.controller as u8, row.cc_min, row.cc_max),
                    2 => { reject("controller index outside the raw CC bank"); Predicate::Always },
                    3 if row.cycle_class >= 0 => {
                        prepared.round_robin_count = prepared.round_robin_count.max(row.cycle_class as u32 + 1);
                        Predicate::RoundRobin(row.cycle_class)
                    },
                    3 => { reject("negative round-robin class"); Predicate::Always },
                    4 => {
                        let ordinal = prepared.random_count;
                        prepared.random_count += 1;
                        Predicate::Random(ordinal)
                    },
                    5 => { reject("Slice trigger requires unsupported private event and sequencer state"); Predicate::Always },
                    _ => { reject("unknown start-criteria mode"); Predicate::Always },
                };
                criteria.rows[slot] = Row { predicate, next: row.next_criteria };
            }
            if criteria.rows[0].predicate == Predicate::Always
                && criteria.rows[1..].iter().any(|row| row.predicate != Predicate::Always) {
                reject("leading Always or unoccupied slot before an active predicate");
            }
            prepared.groups.push(criteria);
        }
        if prepared.initial_key < 0 && let Some((minimum, _)) = prepared.key_span && minimum < 128 {
            prepared.initial_key = minimum;
        }
        prepared
    }

    pub(super) fn state(&self) -> Clock { Clock { last_key: self.initial_key, ..Clock::default() } }

    pub(super) fn select(&self, clock: &mut Clock, cc: &[i16; 130], note: u8,
        native_release: bool, release_prelude: bool, conditions: bool, mut mask: GroupMask) -> GroupMask {
        if !self.native { return mask; }
        if self.native && release_prelude {
            if native_release {
                mask = GroupMask::none();
                for (index, group) in self.groups.iter().enumerate() {
                    if group.release { mask.set(index, true); }
                }
            } else {
                for (index, group) in self.groups.iter().enumerate() {
                    if group.release { mask.set(index, false); }
                }
            }
        }
        if !conditions { return mask; }
        let note = i16::from(note);
        if self.key_span.is_some_and(|(lo, hi)| lo <= note && note <= hi) { clock.last_key = note; }
        if !native_release && self.round_robin_count > 0 {
            clock.round_robin = ((i64::from(clock.round_robin) + 1) % i64::from(self.round_robin_count)) as i32;
        }
        let random = if self.random_count > 0 { clock.random(self.random_count) } else { 0 };
        for (index, group) in self.groups.iter().enumerate() {
            if group.unsupported || !group.matches(clock, cc, random) { mask.set(index, false); }
        }
        mask
    }
}

impl Criteria {
    fn matches(&self, clock: &Clock, cc: &[i16; 130], random: u32) -> bool {
        let mut any = false;
        let mut chain = None;
        let mut connector = 0;
        for row in self.rows.iter().filter(|row| row.predicate != Predicate::Always) {
            let value = match row.predicate {
                Predicate::Key(lo, hi) => lo <= clock.last_key && clock.last_key <= hi,
                Predicate::Controller(index, lo, hi) => lo <= cc[index as usize] && cc[index as usize] <= hi,
                Predicate::RoundRobin(class) => clock.round_robin == class,
                Predicate::Random(ordinal) => random == ordinal,
                Predicate::Always => true,
            };
            chain = Some(match chain {
                None => value,
                Some(previous) if connector == 2 => { any |= previous; value },
                Some(previous) => previous && if connector == 1 { !value } else { value },
            });
            connector = row.next;
        }
        any || chain.unwrap_or(true)
    }
}

/// The library receiver's raw channel bank, separate from accepted DSP state.
/// Native construction starts CC/pressure at zero and bend at signed zero.
pub(super) struct Receiver { pub(super) cc: [[i16; 130]; 16] }

impl Default for Receiver {
    fn default() -> Self { Self { cc: [[0; 130]; 16] } }
}

impl Receiver {
    pub(super) fn clear(&mut self) { self.cc = [[0; 130]; 16]; }

    pub(super) fn control(&mut self, channel: u8, control: crate::midi::Control) {
        if channel >= 16 || !control.valid() { return; }
        let (cc, value) = control.legacy();
        if cc < 130 { self.cc[channel as usize][cc as usize] = value as i16; }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Clock { last_key: i16, round_robin: i32, rng: u64 }

impl Default for Clock {
    fn default() -> Self { Self { last_key: -1, round_robin: -1, rng: 0x9E37_79B9_7F4A_7C15 } }
}

impl Clock {
    fn random(&mut self, count: u32) -> u32 {
        // One shared selector draw per eligible note, including native releases.
        // This deterministic PRNG is local policy, not Kontakt CRT sequence parity.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        ((u128::from(self.rng) * u128::from(count)) >> 64) as u32
    }
}

#[cfg(all(test, feature = "plugin"))]
#[path = "group_start_tests.rs"]
mod group_start_tests;
