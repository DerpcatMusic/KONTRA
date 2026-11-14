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
    /// Its sample (`get_sample`, and what the music-information commands read).
    pub source: Option<std::sync::Arc<ir::Sample>>,
    /// Its loops, in play order (`get_loop_par`).
    pub loops: Vec<ir::Loop>,
    /// Made by `set_num_user_zones`: a script gives it a sample and loops.
    pub user: bool,
    /// KSP units: hundredths of a dB, thousandths of full pan, cents.
    pub volume: i32,
    pub pan: i32,
    pub tune: i32,
    /// Fade lengths: low key, high key, low velocity, high velocity.
    pub fades: [i32; 4],
    /// A user zone's eight loops as the script set them (`set_loop_par`).
    pub loop_pars: [LoopPar; 8],
}

/// One loop of a user zone as `set_loop_par` holds it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoopPar {
    pub mode: i32,
    pub start: i32,
    pub length: i32,
    pub xfade: i32,
    pub count: i32,
    pub tuning: i32,
}

/// Kontakt's clamps of the zone parameters (probes/ksp/user_zones_pars.kspp).
pub const VOLUME_RANGE: (i32, i32) = (-3600, 3600);
pub const PAN_RANGE: (i32, i32) = (-1000, 1000);
// Kontakt reads 6204 for 2e6 and i32::MIN for -2e6; the clamp is unmeasured.
pub const TUNE_RANGE: (i32, i32) = (-3600, 3600);
/// A user zone's root key once it holds a sample (measured).
pub const USER_ROOT: u8 = 36;

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
            ZonePar::Bpm => match (&self.source, self.user) {
                (Some(s), true) => guess_bpm(s.frames, s.rate),
                _ => 0,
            },
            ZonePar::Volume => self.volume,
            ZonePar::Pan => self.pan,
            ZonePar::Tune => self.tune,
            ZonePar::FadeLowKey => self.fades[0],
            ZonePar::FadeHighKey => self.fades[1],
            ZonePar::FadeLowVelocity => self.fades[2],
            ZonePar::FadeHighVelocity => self.fades[3],
        }
    }

    /// `get_loop_par`: a user zone's loop as set, a saved zone's as loaded;
    /// -1 past the eighth loop, 0 for a loop the zone lacks.
    #[must_use]
    pub fn loop_par(&self, index: i32, par: i32) -> i32 {
        let Ok(i) = usize::try_from(index).map(|i| (i < 8).then_some(i)) else {
            return -1;
        };
        let Some(i) = i else { return -1 };
        let int = |n: u64| i32::try_from(n).unwrap_or(i32::MAX);
        if self.user {
            let l = self.loop_pars.get(i).copied().unwrap_or_default();
            return match par {
                520 => l.mode,
                521 => l.start,
                522 => l.length,
                523 => l.xfade,
                524 => l.count,
                525 => l.tuning,
                _ => 0,
            };
        }
        let Some(l) = self.loops.get(i) else { return 0 };
        match par {
            // $LOOP_PAR_MODE reads 1 for a loop that exists (the mode laws are unmeasured).
            520 => 1,
            521 => int(l.start),
            522 => int(l.end.saturating_sub(l.start)),
            523 => int(l.crossfade),
            524 => int(l.count.into()),
            _ => 0,
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

impl ZoneRules {
    /// A plain zone's, as [`Zone::new`] makes it.
    pub const PLAIN: Self = Self {
        start: Vec::new(),
        slots: Vec::new(),
        trigger: Trigger::Attack,
        off_by: None,
        voice_group: None,
        keytrack: 1.0,
        release_decay_db: 0.0,
        one_shot: false,
    };

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
    /// a parameter not carried yet. A table follows the zone's group too.
    // ponytail: a zone moved into a cycling group whose slots lack its own
    // reads the cycle as if its slot were there; recount the cycle if a
    // library moves zones between cycles.
    // ponytail: the fade lengths, root, volume, pan, tune, sample bounds and BPM
    // are not settable yet; add each when a library sets it.
    pub fn edit_zone(&mut self, id: u32, par: ZonePar, value: i32) -> bool {
        let group = u32::try_from(value)
            .ok()
            .filter(|&g| (g as usize) < self.groups.len());
        let mut found = false;
        for f in self.zone_facts.iter_mut().filter(|z| z.id == id) {
            found |= f.set(par, value, group);
        }
        let Self {
            zones,
            groups,
            zone_facts,
            ..
        } = self;
        for z in zones.iter_mut().filter(|z| z.id == id) {
            let was = z.group;
            let facts = zone_facts.iter().find(|f| f.id == id);
            let key = u8::try_from(value.clamp(0, 127)).unwrap_or(0);
            let changed = match (par, group, facts) {
                (ZonePar::LowKey | ZonePar::HighKey, _, _) => {
                    set_edge(&mut z.keys, par == ZonePar::LowKey, key);
                    true
                }
                (ZonePar::LowVelocity | ZonePar::HighVelocity, _, _) => {
                    set_edge(&mut z.velocities, par == ZonePar::LowVelocity, key);
                    true
                }
                (ZonePar::Group, Some(to), _) => {
                    z.group = to;
                    true
                }
                // The rest read the value the facts settled on (clamped).
                (ZonePar::RootKey, _, Some(f)) => {
                    if z.region.table.is_none() {
                        z.root = f.root;
                    }
                    true
                }
                (ZonePar::Volume, _, Some(f)) => {
                    z.gain = 10f32.powf(f.volume as f32 / 2000.0);
                    true
                }
                (ZonePar::Pan, _, Some(f)) => {
                    z.pan = f.pan as f32 / 1000.0;
                    true
                }
                (ZonePar::Tune, _, Some(f)) => {
                    z.tune = f64::from(f.tune) / 100.0;
                    true
                }
                (
                    ZonePar::FadeLowKey
                    | ZonePar::FadeHighKey
                    | ZonePar::FadeLowVelocity
                    | ZonePar::FadeHighVelocity,
                    _,
                    Some(f),
                ) => {
                    let n = |v: i32| u8::try_from(v).unwrap_or(u8::MAX);
                    let [lk, hk, lv, hv] = f.fades;
                    z.fades = [(n(lv), n(hv)), (n(lk), n(hk))];
                    true
                }
                (ZonePar::SampleStart | ZonePar::SampleEnd, _, Some(f)) => {
                    (z.region.start, z.region.end) = (f.start, f.end);
                    true
                }
                _ => false,
            };
            if changed {
                // Its fades keep their lengths at the new range.
                z.range_crossfades();
                found = true;
            }
            if let Some(g) = groups.get(z.group as usize).filter(|_| z.group != was) {
                g.zones.apply(z);
                z.member = u16::MAX;
                // A table reads as its group says; outside a wavetable group
                // the zone plays its sample at its own root and rate. A zone
                // entering one without a table gets it built off the audio
                // thread (`kontra_prepare::tables`).
                match (g.wavetable, &mut z.region.table) {
                    (Some(read), Some(table)) => table.read = read,
                    (None, table @ Some(_)) => {
                        *table = None;
                        if let Some(f) = zone_facts.iter().find(|f| f.id == id) {
                            (z.root, z.sample_rate) = (f.root, f.rate);
                        }
                    }
                    _ => {}
                }
            }
        }
        found
    }
}

impl crate::Plan {
    /// The user zones' ids, in the order `set_num_user_zones` made them
    /// (`%NI_USER_ZONE_IDS`).
    pub fn user_zone_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.user_facts().iter().map(|z| z.id)
    }

    /// The user zones' facts: the zones made last, at the end of the list
    /// (a tick of the host reads only these, not a big library's 95,000).
    #[must_use]
    pub fn user_facts(&self) -> &[ZoneFacts] {
        let n = self.zone_facts.iter().rev().take_while(|z| z.user).count();
        self.zone_facts
            .get(self.zone_facts.len() - n..)
            .unwrap_or_default()
    }

    /// `set_num_user_zones(n)`: `n` empty user zones with consecutive ids
    /// after the highest saved one, replacing any made before (Kontakt
    /// 8.13: ids 181963, 181964 after the instrument's 181962; a second
    /// call with 3 left 3 zones from the same first id). All parameters 0
    /// but the root key, 36.
    pub fn make_user_zones(&mut self, n: usize) {
        let user: Vec<u32> = self.user_zone_ids().collect();
        self.zone_facts.retain(|z| !z.user);
        self.zones.retain(|z| !user.contains(&z.id));
        let first = self.zone_facts.iter().map(|z| z.id + 1).max().unwrap_or(0);
        for i in 0..n.min(MAX_USER_ZONES) {
            self.zone_facts.push(ZoneFacts {
                id: first + i as u32,
                name: "".into(),
                group: 0,
                keys: 0..=0,
                velocities: 0..=0,
                root: USER_ROOT,
                start: 0,
                end: 0,
                start_range: 0,
                rate: 0,
                source: None,
                loops: Vec::new(),
                user: true,
                volume: 0,
                pan: 0,
                tune: 0,
                fades: [0; 4],
                loop_pars: [LoopPar::default(); 8],
            });
        }
    }

    /// `set_sample` on a user zone: the zone holds `sample` whole. False
    /// for a zone that is not a user zone.
    pub fn set_user_sample(&mut self, zone: u32, sample: std::sync::Arc<ir::Sample>) -> bool {
        let Some(f) = self.zone_facts.iter_mut().find(|z| z.id == zone && z.user) else {
            return false;
        };
        f.name = zone_name(&sample.name).into();
        (f.start, f.end, f.rate) = (0, sample.frames, sample.rate);
        f.source = Some(sample);
        true
    }

    /// `set_loop_par` on a user zone (clamped as Kontakt 8.13 reads it back:
    /// a length ends at the sample's end). False for a zone that is not a
    /// user zone or a loop past the eighth.
    pub fn set_user_loop(&mut self, zone: u32, index: i32, par: i32, value: i32) -> bool {
        let Some(f) = self.zone_facts.iter_mut().find(|z| z.id == zone && z.user) else {
            return false;
        };
        let frames = i32::try_from(f.source.as_ref().map_or(0, |s| s.frames)).unwrap_or(i32::MAX);
        let Some(l) = usize::try_from(index)
            .ok()
            .and_then(|i| f.loop_pars.get_mut(i))
        else {
            return false;
        };
        match par {
            // Kontakt keeps the old mode for a value it does not know.
            520 if LOOP_MODES.contains(&value) => l.mode = value,
            520 => {}
            // A start past the sample reads 0 in Kontakt (set refused).
            521 if (0..=frames).contains(&value) => l.start = value,
            521 => {}
            522 => l.length = value.clamp(0, (frames - l.start).max(0)),
            523 => l.xfade = value.max(0),
            524 => l.count = value.max(0),
            525 => l.tuning = value.clamp(-14400, 14400),
            _ => return false,
        }
        let loops = user_loops(f);
        let id = f.id;
        for z in self.zones.iter_mut().filter(|z| z.id == id) {
            z.region.loops = loops.clone();
        }
        true
    }
}

/// The tempo ×1000 Kontakt 8.13 reads (`$ZONE_PAR_BPM`) for a user sample of
/// `frames` at `rate`: it assumes 4, 8, 16, … beats and takes the fewest whose
/// tempo reaches 70 BPM (probes/ksp/user_bpm.kspp and user_bpm_edges.kspp: 22 lengths,
/// 0.25 s to 16 s; 68.6 is refused, 70.6 taken). Its float arithmetic reads one
/// thousandth low at 80 and 160 BPM (3, 6, 12 and 1.5 s): matched, not explained.
#[must_use]
pub fn guess_bpm(frames: u64, rate: u32) -> i32 {
    if frames == 0 || rate == 0 {
        return 0;
    }
    let (frames, rate) = (frames as u128, u128::from(rate));
    let mut beats = 4u128;
    // milli-BPM = beats · 60000 · rate / frames
    while beats * 60_000 * rate < 70_000 * frames && beats < 1 << 20 {
        beats *= 2;
    }
    let mut milli = beats * 60_000 * rate / frames;
    if beats * rate * 3 == 4 * frames || beats * rate * 3 == 8 * frames {
        milli -= 1;
    }
    i32::try_from(milli).unwrap_or(i32::MAX)
}

/// Most user zones an instrument can have (the manual).
pub const MAX_USER_ZONES: usize = 1024;

/// The loop modes a user zone accepts (probes/ksp/loop_modes.kspp: 0..=4 read back,
/// anything else leaves the mode as it was).
pub const LOOP_MODES: std::ops::RangeInclusive<i32> = 0..=4;

/// What a user zone's loops play as: each loop whose mode is not 0.
#[must_use]
pub fn user_loops(f: &ZoneFacts) -> kontra_voice::Loops {
    let n = |v: i32| u64::try_from(v).unwrap_or(0);
    f.loop_pars
        .iter()
        .filter(|l| l.mode != 0 && l.length > 0)
        .map(|l| kontra_voice::Loop {
            start: n(l.start),
            end: n(l.start) + n(l.length),
            crossfade: n(l.xfade),
            count: u32::try_from(l.count).unwrap_or(0),
            // Modes 2..=4 are guessed (release, alternating, both): only 0..=4
            // reading back is measured.
            alternating: l.mode >= 3,
            until_release: matches!(l.mode, 2 | 4),
        })
        .collect()
}

/// Moves one edge of a range; the other follows when they would cross
/// (Kontakt 8.13: a high key set below the low key takes the low with it).
fn set_edge(range: &mut RangeInclusive<u8>, low: bool, v: u8) {
    let (lo, hi) = (*range.start(), *range.end());
    *range = if low { v..=hi.max(v) } else { lo.min(v)..=v };
}

impl ZoneFacts {
    /// `set_zone_par` on the facts, the value Kontakt would hold: clamped
    /// (probes/ksp/user_zones_pars.kspp). False for a parameter that cannot
    /// be set or a group that does not exist.
    fn set(&mut self, par: ZonePar, value: i32, group: Option<u32>) -> bool {
        let key = u8::try_from(value.clamp(0, 127)).unwrap_or(0);
        let width = i32::from(*self.keys.end()) - i32::from(*self.keys.start()) + 1;
        let vwidth = i32::from(*self.velocities.end()) - i32::from(*self.velocities.start()) + 1;
        let frames = self.source.as_ref().map_or(self.end, |s| s.frames);
        // A sample's start and end stay 4 frames apart.
        let bound = |v: i32| u64::try_from(v).unwrap_or(0).min(frames);
        match par {
            ZonePar::LowKey => set_edge(&mut self.keys, true, key),
            ZonePar::HighKey => set_edge(&mut self.keys, false, key),
            ZonePar::LowVelocity => set_edge(&mut self.velocities, true, key),
            ZonePar::HighVelocity => set_edge(&mut self.velocities, false, key),
            ZonePar::Group => match group {
                Some(g) => self.group = g,
                None => return false,
            },
            ZonePar::RootKey => self.root = key,
            ZonePar::Volume => self.volume = value.clamp(VOLUME_RANGE.0, VOLUME_RANGE.1),
            ZonePar::Pan => self.pan = value.clamp(PAN_RANGE.0, PAN_RANGE.1),
            ZonePar::Tune => self.tune = value.clamp(TUNE_RANGE.0, TUNE_RANGE.1),
            ZonePar::FadeLowKey => self.fades[0] = value.clamp(0, width),
            ZonePar::FadeHighKey => self.fades[1] = value.clamp(0, width),
            ZonePar::FadeLowVelocity => self.fades[2] = value.clamp(0, vwidth),
            ZonePar::FadeHighVelocity => self.fades[3] = value.clamp(0, vwidth),
            ZonePar::SampleStart => {
                self.start = bound(value).min(self.end.saturating_sub(4));
            }
            ZonePar::SampleEnd => {
                self.end = bound(value).max((self.start + 4).min(frames));
            }
            ZonePar::StartRange | ZonePar::SampleRate | ZonePar::Selected | ZonePar::Bpm => {
                return false;
            }
        }
        true
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

#[cfg(test)]
mod bpm_tests {
    use super::guess_bpm;

    /// Kontakt 8.13 readings for a 44.1 kHz sample of each length in seconds.
    #[test]
    fn the_tempo_guess_matches_kontakts_readings() {
        let secs = |s: f64| guess_bpm((s * 44_100.0).round() as u64, 44_100);
        for (len, bpm) in [
            (0.25, 960_000),
            (1.0, 240_000),
            (2.0, 120_000),
            (3.0, 79_999),
            (4.0, 120_000),
            (12.0, 79_999),
            (16.0, 120_000),
            (1.5, 159_999),
            (1.9, 126_315),
            (3.05, 78_688),
            (3.4, 70_588),
            (5.0, 96_000),
            (7.0, 137_142),
        ] {
            assert_eq!(secs(len), bpm, "{len} s");
        }
    }
}
