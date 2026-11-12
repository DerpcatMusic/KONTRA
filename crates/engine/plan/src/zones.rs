//! What scripts read of an instrument's zones: each zone's id, name and
//! saved settings, as the format saved them (a wavetable plays its zones at
//! another root and rate; a script still reads the saved ones).

use std::ops::RangeInclusive;

use kontra_ir::{self as ir, ZonePar};

use crate::{Atom, RrCounter, Trigger, Zone};

/// One zone as scripts find and read it.
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneFacts {
    pub id: u32,
    /// Its sample's file name without folder or extension.
    pub name: Box<str>,
    pub group: u32,
    pub keys: RangeInclusive<u8>,
    pub velocities: RangeInclusive<u8>,
    pub root: u8,
    pub start: u64,
    pub end: u64,
    pub start_range: u64,
    pub rate: u32,
}

impl ZoneFacts {
    /// The value `get_zone_par` reads for `par`.
    #[must_use]
    pub fn get(&self, par: ZonePar) -> i32 {
        let frames = |n: u64| i32::try_from(n).unwrap_or(i32::MAX);
        match par {
            ZonePar::LowKey => i32::from(*self.keys.start()),
            ZonePar::HighKey => i32::from(*self.keys.end()),
            ZonePar::LowVelocity => i32::from(*self.velocities.start()),
            ZonePar::HighVelocity => i32::from(*self.velocities.end()),
            ZonePar::RootKey => i32::from(self.root),
            ZonePar::Group => i32::try_from(self.group).unwrap_or(i32::MAX),
            ZonePar::SampleStart => frames(self.start),
            ZonePar::SampleEnd => frames(self.end),
            ZonePar::StartRange => frames(self.start_range),
            ZonePar::SampleRate => i32::try_from(self.rate).unwrap_or(i32::MAX),
            ZonePar::Selected => 0,
        }
    }
}

/// What a group gives each of its zones, as the loader derives it from
/// the group. A zone `set_zone_par` moves takes its new group's.
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneRules {
    /// Start conditions; a cycle's (a [`RrCounter::Group`] round robin)
    /// position is each zone's own, from `slots`.
    pub start: Vec<(ir::Join, Atom)>,
    /// The group's cycle slots, sorted, each once.
    pub slots: Vec<u16>,
    pub trigger: Trigger,
    pub off_by: Option<u32>,
    pub voice_group: Option<u32>,
    pub keytrack: f64,
    pub release_decay_db: f32,
    pub one_shot: bool,
}

impl Default for ZoneRules {
    /// A plain zone's, as [`Zone::new`] makes it.
    fn default() -> Self {
        Self {
            start: Vec::new(),
            slots: Vec::new(),
            trigger: Trigger::Attack,
            off_by: None,
            voice_group: None,
            keytrack: 1.0,
            release_decay_db: 0.0,
            one_shot: false,
        }
    }
}

impl ZoneRules {
    /// Gives `zone` these rules.
    pub fn apply(&self, zone: &mut Zone) {
        let position = self.slots.iter().take_while(|&&s| s < zone.slot).count();
        zone.start.clone_from(&self.start);
        for (_, atom) in &mut zone.start {
            if let Atom::RoundRobin(rr) = atom
                && rr.counter == RrCounter::Group
            {
                rr.position = u8::try_from(position).unwrap_or(u8::MAX);
            }
        }
        zone.trigger = self.trigger;
        zone.off_by = self.off_by;
        zone.voice_group = self.voice_group;
        zone.keytrack = self.keytrack;
        zone.release_decay_db = self.release_decay_db;
        zone.one_shot = self.one_shot;
    }
}

impl crate::Plan {
    /// `set_zone_par(id, par, value)`: zone `id`'s group, key or velocity
    /// range, in the zones that play and in what scripts read. A zone moved
    /// to another group takes that group's [`ZoneRules`]; no mix route of
    /// its new group names it. False for a zone or group the plan lacks, or
    /// a parameter not carried yet.
    // ponytail: a zone moved into a cycling group whose slots lack its own
    // reads the cycle as if its slot were there; recount the cycle if a
    // library moves zones between cycles.
    // ponytail: the fades, root, volume, pan, tune, sample bounds and BPM
    // are not settable yet; add each when a library sets it.
    pub fn edit_zone(&mut self, id: u32, par: ZonePar, value: i32) -> bool {
        let key = u8::try_from(value.clamp(0, 127)).unwrap_or(0);
        let group = u32::try_from(value)
            .ok()
            .filter(|&g| (g as usize) < self.groups.len());
        let edit = |keys: &mut RangeInclusive<u8>, vels: &mut RangeInclusive<u8>, g: &mut u32| {
            match (par, group) {
                (ZonePar::LowKey, _) => *keys = key..=*keys.end(),
                (ZonePar::HighKey, _) => *keys = *keys.start()..=key,
                (ZonePar::LowVelocity, _) => *vels = key..=*vels.end(),
                (ZonePar::HighVelocity, _) => *vels = *vels.start()..=key,
                (ZonePar::Group, Some(to)) => *g = to,
                _ => return false,
            }
            true
        };
        let mut found = false;
        for z in self.zone_facts.iter_mut().filter(|z| z.id == id) {
            found |= edit(&mut z.keys, &mut z.velocities, &mut z.group);
        }
        let Self { zones, groups, .. } = self;
        for z in zones.iter_mut().filter(|z| z.id == id) {
            let was = z.group;
            found |= edit(&mut z.keys, &mut z.velocities, &mut z.group);
            if let Some(g) = groups.get(z.group as usize).filter(|_| z.group != was) {
                g.zones.apply(z);
                z.member = u16::MAX;
            }
        }
        found
    }
}

/// A sample's name as a zone is called: no folder, no extension.
#[must_use]
pub fn zone_name(sample: &str) -> &str {
    let file = sample.rsplit(['/', '\\']).next().unwrap_or(sample);
    file.rsplit_once('.').map_or(file, |(stem, _)| stem)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_zone_is_called_by_its_files_stem() {
        assert_eq!(
            super::zone_name("Resources/wavetables/Warmth.wav"),
            "Warmth"
        );
        assert_eq!(super::zone_name("a\\b\\Buzz Cut.ncw"), "Buzz Cut");
        assert_eq!(super::zone_name("plain"), "plain");
    }
}
