//! Script-facing zone metadata & editing: saved zones live in the compact SoA
//! [`crate::soa::ZoneTable`] (zero duplicated per-zone structs, zero per-zone
//! `loop_pars`), while script-created user zones (`set_num_user_zones`) live
//! sparsely in [`UserZone`] (0 bytes when unused).

use std::ops::RangeInclusive;
use std::sync::Arc;

use kontra_ir::{self as ir, ZonePar};
use kontra_voice::Region;

use crate::soa::SavedZoneDesc;
use crate::{Atom, RrCounter, Trigger, Zone};

/// One script-created user zone (`set_num_user_zones`): carries its editable
/// KSP parameters, optional sample, and 8-slot [`LoopPar`] table. Saved zones
/// never allocate a `UserZone`.
#[derive(Clone, Debug, PartialEq)]
pub struct UserZone {
    pub id: u32,
    /// Its sample's file name without folder or extension.
    pub name: Box<str>,
    pub group: u32,
    pub keys: RangeInclusive<u8>,
    pub velocities: RangeInclusive<u8>,
    pub root: u8,
    pub start: u64,
    pub end: u64,
    pub rate: u32,
    /// Its sample (`get_sample`, `detect_*`, and `prepare::user_zones`).
    pub source: Option<Arc<ir::Sample>>,
    /// KSP units: hundredths of a dB, thousandths of full pan, cents.
    pub volume: i32,
    pub pan: i32,
    pub tune: i32,
    /// Fade lengths: low key, high key, low velocity, high velocity.
    pub fades: [i32; 4],
    /// User loop parameters (`set_loop_par` / `get_loop_par`): allocated only for user zones.
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

impl UserZone {
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
            ZonePar::StartRange => 0,
            ZonePar::SampleRate => i32::try_from(self.rate).unwrap_or(i32::MAX),
            ZonePar::Selected => 0,
            ZonePar::Bpm => match &self.source {
                Some(s) => guess_bpm(s.frames, s.rate),
                None => 0,
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

    /// `get_loop_par` for a user zone: -1 past the eighth loop.
    #[must_use]
    pub fn loop_par(&self, index: i32, par: i32) -> i32 {
        let Ok(i) = usize::try_from(index) else {
            return -1;
        };
        let Some(l) = self.loop_pars.get(i) else {
            return -1;
        };
        match par {
            520 => l.mode,
            521 => l.start,
            522 => l.length,
            523 => l.xfade,
            524 => l.count,
            525 => l.tuning,
            _ => 0,
        }
    }

    /// `set_zone_par` on a user zone, clamped as Kontakt holds it.
    fn set(&mut self, par: ZonePar, value: i32, group: Option<u32>) -> bool {
        let key = u8::try_from(value.clamp(0, 127)).unwrap_or(0);
        let width = i32::from(*self.keys.end()) - i32::from(*self.keys.start()) + 1;
        let vwidth = i32::from(*self.velocities.end()) - i32::from(*self.velocities.start()) + 1;
        let frames = self.source.as_ref().map_or(self.end, |s| s.frames);
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
            ZonePar::FadeLowKey => share(&mut self.fades, 0, value, width),
            ZonePar::FadeHighKey => share(&mut self.fades, 1, value, width),
            ZonePar::FadeLowVelocity => share(&mut self.fades, 2, value, vwidth),
            ZonePar::FadeHighVelocity => share(&mut self.fades, 3, value, vwidth),
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
    pub repeat_cuts: bool,
    pub offset_limited: bool,
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
        repeat_cuts: false,
        offset_limited: false,
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
        zone.repeat_cuts = self.repeat_cuts;
        zone.offset_limited = self.offset_limited;
        zone.one_shot = self.one_shot;
    }
}

/// Resolved post-clamp values used to sync playable [`Zone`]s inside [`crate::Plan::edit_zone`].
#[derive(Clone, Copy)]
struct SyncValues {
    root: u8,
    gain: f32,
    pan: f32,
    tune: f64,
    fades: [(u8, u8); 2],
    start: u64,
    end: u64,
}

impl crate::Plan {
    /// Appends a single saved zone row to [`Self::zone_table`] (helper for tests
    /// and programmatic plans).
    pub fn push_saved_zone(&mut self, desc: SavedZoneDesc<'_>, sample: Option<Arc<ir::Sample>>) {
        let table = Arc::make_mut(&mut self.zone_table);
        let sample_idx = if let Some(s) = sample {
            let mut samples = table.samples.to_vec();
            let idx = u32::try_from(samples.len()).unwrap_or(u32::MAX);
            samples.push(s);
            table.samples = Arc::from(samples);
            Some(idx)
        } else {
            desc.sample_idx
        };
        table.push_saved(SavedZoneDesc { sample_idx, ..desc });
    }

    /// Total zones visible to scripts (saved + user, including purged/dormant).
    #[must_use]
    pub fn zone_count(&self) -> usize {
        (self.zone_table.len() + self.user_zones.len()).max(self.zones.len())
    }

    /// Zone ID at `index` in instrument order (saved zones first, then user zones).
    #[must_use]
    pub fn zone_id_at(&self, index: usize) -> Option<u32> {
        let saved = self.zone_table.len();
        if index < saved {
            self.zone_table.ids.get(index).copied()
        } else {
            self.user_zones.get(index - saved).map(|u| u.id)
        }
    }

    /// First zone whose name equals `name` exactly (`find_zone`). Zero allocation.
    #[must_use]
    pub fn find_zone(&self, name: &str) -> Option<u32> {
        self.zone_table.find_by_name(name).or_else(|| {
            self.user_zones
                .iter()
                .find(|u| &*u.name == name)
                .map(|u| u.id)
        })
    }

    /// Reads `$ZONE_PAR_*` for zone `id` from the SoA table or sparse user zones.
    #[must_use]
    pub fn zone_par(&self, id: u32, par: ZonePar) -> Option<i32> {
        if let Some(row) = self.zone_table.row_of(id) {
            return self.zone_table.get_par(row, par);
        }
        self.user_zones
            .iter()
            .find(|u| u.id == id)
            .map(|u| u.get(par))
    }

    /// Group index of zone `id`, if present.
    #[must_use]
    pub fn zone_group(&self, id: u32) -> Option<u32> {
        if let Some(row) = self.zone_table.row_of(id) {
            return self.zone_table.matches.get(row).map(|m| u32::from(m.group));
        }
        self.user_zones.iter().find(|u| u.id == id).map(|u| u.group)
    }

    /// Sample descriptor backing zone `id`, if any (zero allocation).
    #[must_use]
    pub fn zone_sample(&self, id: u32) -> Option<&Arc<ir::Sample>> {
        if let Some(row) = self.zone_table.row_of(id) {
            return self.zone_table.sample_at(row);
        }
        self.user_zones
            .iter()
            .find(|u| u.id == id)
            .and_then(|u| u.source.as_ref())
    }

    /// Whole sample length in microseconds for zone `id` (`get_sample_length`).
    #[must_use]
    pub fn zone_sample_length_us(&self, id: u32) -> Option<i32> {
        if let Some(row) = self.zone_table.row_of(id) {
            let play = self.zone_table.play.get(row)?;
            let frames = self
                .zone_table
                .sample_at(row)
                .map_or_else(|| u64::from(play.end), |s| s.frames);
            return Some(
                i32::try_from(frames * 1_000_000 / u64::from(play.sample_rate.max(1)))
                    .unwrap_or(i32::MAX),
            );
        }
        let u = self.user_zones.iter().find(|u| u.id == id)?;
        let frames = u.source.as_ref().map_or(u.end, |s| s.frames);
        Some(i32::try_from(frames * 1_000_000 / u64::from(u.rate.max(1))).unwrap_or(i32::MAX))
    }

    /// Attaches strictly increasing slice boundary frames to saved zone `id`.
    pub fn set_zone_slices(&mut self, id: u32, markers: &[u32]) {
        if let Some(row) = self.zone_table.row_of(id) {
            Arc::make_mut(&mut self.zone_table).set_slices(row, markers);
        }
    }

    /// Number of slices in zone `id`: `markers.len() - 1`, or 0 for an
    /// unsliced or absent zone. Zero allocation.
    #[must_use]
    pub fn zone_num_slices(&self, id: u32) -> i32 {
        let Some(row) = self.zone_table.row_of(id) else {
            return 0;
        };
        let markers = self.zone_table.slices_at(row);
        i32::try_from(markers.len().saturating_sub(1)).unwrap_or(i32::MAX)
    }

    /// Start of slice `index` in microseconds, rounded from frames to nearest µs.
    /// Includes the final boundary (`index == num_slices`); out-of-range or
    /// unsliced zones answer 0. Zero allocation.
    #[must_use]
    pub fn zone_slice_start_us(&self, id: u32, index: i32) -> i32 {
        let (Ok(idx), Some(row)) = (usize::try_from(index), self.zone_table.row_of(id)) else {
            return 0;
        };
        let markers = self.zone_table.slices_at(row);
        if markers.len() < 2 {
            return 0;
        }
        let (Some(&frame), Some(play)) = (markers.get(idx), self.zone_table.play.get(row)) else {
            return 0;
        };
        frames_to_us(frame, play.sample_rate)
    }

    /// Length of slice `index` in microseconds: difference between adjacent
    /// marker timestamps; out-of-range or unsliced zones answer 0. Zero allocation.
    #[must_use]
    pub fn zone_slice_length_us(&self, id: u32, index: i32) -> i32 {
        let (Ok(idx), Some(row)) = (usize::try_from(index), self.zone_table.row_of(id)) else {
            return 0;
        };
        let markers = self.zone_table.slices_at(row);
        let (Some(&a), Some(&b), Some(play)) = (
            markers.get(idx),
            markers.get(idx.saturating_add(1)),
            self.zone_table.play.get(row),
        ) else {
            return 0;
        };
        let rate = play.sample_rate;
        frames_to_us(b, rate).saturating_sub(frames_to_us(a, rate))
    }

    /// First slice index of loop `loop_idx`. Unsliced or absent zones answer 0;
    /// sliced zones answer -1 for unused or out-of-range loop slots. Zero allocation.
    #[must_use]
    pub fn zone_slice_loop_start(&self, id: u32, loop_idx: i32) -> i32 {
        self.zone_slice_loop(id, loop_idx, |markers, l| {
            let i = markers
                .partition_point(|&m| u64::from(m) <= l.start)
                .saturating_sub(1);
            i32::try_from(i).unwrap_or(-1)
        })
    }

    /// End slice boundary index of loop `loop_idx`. Unsliced or absent zones
    /// answer 0; sliced zones answer -1 for unused or out-of-range loop slots. Zero allocation.
    #[must_use]
    pub fn zone_slice_loop_end(&self, id: u32, loop_idx: i32) -> i32 {
        self.zone_slice_loop(id, loop_idx, |markers, l| {
            let i = markers
                .partition_point(|&m| u64::from(m) < l.end)
                .min(markers.len().saturating_sub(1));
            i32::try_from(i).unwrap_or(-1)
        })
    }

    /// Repeat count of loop `loop_idx` on a sliced zone. Unsliced or absent
    /// zones answer 0; sliced zones answer -1 for unused or out-of-range loop slots. Zero allocation.
    #[must_use]
    pub fn zone_slice_loop_count(&self, id: u32, loop_idx: i32) -> i32 {
        self.zone_slice_loop(id, loop_idx, |_, l| {
            i32::try_from(l.count).unwrap_or(i32::MAX)
        })
    }

    fn zone_slice_loop(
        &self,
        id: u32,
        loop_idx: i32,
        f: impl FnOnce(&[u32], &ir::Loop) -> i32,
    ) -> i32 {
        let Some(row) = self.zone_table.row_of(id) else {
            return 0;
        };
        let markers = self.zone_table.slices_at(row);
        if markers.len() < 2 {
            return 0;
        }
        let Ok(i) = usize::try_from(loop_idx) else {
            return -1;
        };
        if i >= 8 {
            return -1;
        }
        let Some(l) = self.zone_table.loops_at(row).get(i) else {
            return -1;
        };
        f(markers, l)
    }

    /// Whether `id` is a user zone.
    #[must_use]
    pub fn is_user_zone(&self, id: u32) -> bool {
        self.user_zones.iter().any(|u| u.id == id)
    }

    /// Whether `id` is an empty user zone (no sample assigned yet).
    #[must_use]
    pub fn is_empty_user_zone(&self, id: u32) -> bool {
        self.user_zones
            .iter()
            .any(|u| u.id == id && u.source.is_none())
    }

    /// `set_zone_par(id, par, value)`: zone `id`'s group, key or velocity
    /// range, in the SoA/user tables and in the zones that play. A zone moved
    /// to another group takes that group's [`ZoneRules`]; no mix route of
    /// its new group names it. False for a zone or group the plan lacks, or
    /// a parameter not carried yet. A table follows the zone's group too.
    pub fn edit_zone(&mut self, id: u32, par: ZonePar, value: i32) -> bool {
        let group = u32::try_from(value)
            .ok()
            .filter(|&g| (g as usize) < self.groups.len());
        let mut found = false;
        let mut synced: Option<SyncValues> = None;

        if self.zone_table.ids.contains(&id) {
            let table = Arc::make_mut(&mut self.zone_table);
            for row in 0..table.len() {
                if table.ids.get(row) == Some(&id) && table.set_par(row, par, value, group) {
                    found = true;
                    if synced.is_none()
                        && let Some(p) = table.play.get(row)
                    {
                        synced = Some(SyncValues {
                            root: p.root,
                            gain: p.gain,
                            pan: p.pan,
                            tune: f64::from(p.tune_semitones),
                            fades: [
                                (p.vel_fade[0], p.vel_fade[1]),
                                (p.key_fade[0], p.key_fade[1]),
                            ],
                            start: u64::from(p.start),
                            end: u64::from(p.end),
                        });
                    }
                }
            }
        }
        for u in self.user_zones.iter_mut().filter(|u| u.id == id) {
            if u.set(par, value, group) {
                found = true;
                if synced.is_none() {
                    let byte = |v: i32| u8::try_from(v).unwrap_or(u8::MAX);
                    let [lk, hk, lv, hv] = u.fades;
                    synced = Some(SyncValues {
                        root: u.root,
                        gain: 10f32.powf(u.volume as f32 / 2000.0),
                        pan: u.pan as f32 / 1000.0,
                        tune: f64::from(u.tune) / 100.0,
                        fades: [(byte(lv), byte(hv)), (byte(lk), byte(hk))],
                        start: u.start,
                        end: u.end,
                    });
                }
            }
        }

        // Fallback lookup for wavetable exit when `par == Group` or `synced` was not populated.
        let restore_root_rate = |table: &crate::soa::ZoneTable, users: &[UserZone]| {
            if let Some(row) = table.row_of(id)
                && let Some(p) = table.play.get(row)
            {
                return Some((p.root, p.sample_rate));
            }
            users.iter().find(|u| u.id == id).map(|u| (u.root, u.rate))
        };

        let Self {
            zones,
            groups,
            zone_table,
            user_zones,
            ..
        } = self;
        for z in zones.iter_mut().filter(|z| z.id == id) {
            let was = z.group;
            let key = u8::try_from(value.clamp(0, 127)).unwrap_or(0);
            let changed = match (par, group, synced) {
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
                (ZonePar::RootKey, _, Some(s)) => {
                    if z.region.table.is_none() {
                        z.root = s.root;
                    }
                    true
                }
                (ZonePar::Volume, _, Some(s)) => {
                    z.gain = s.gain;
                    true
                }
                (ZonePar::Pan, _, Some(s)) => {
                    z.pan = s.pan;
                    true
                }
                (ZonePar::Tune, _, Some(s)) => {
                    z.tune = s.tune;
                    true
                }
                (
                    ZonePar::FadeLowKey
                    | ZonePar::FadeHighKey
                    | ZonePar::FadeLowVelocity
                    | ZonePar::FadeHighVelocity,
                    _,
                    Some(s),
                ) => {
                    z.fades = s.fades;
                    true
                }
                (ZonePar::SampleStart | ZonePar::SampleEnd, _, Some(s)) => {
                    (z.region.start, z.region.end) = (s.start, s.end);
                    true
                }
                _ => false,
            };
            if changed {
                z.range_crossfades();
                found = true;
            }
            if let Some(g) = groups.get(z.group as usize).filter(|_| z.group != was) {
                g.zones.apply(z);
                z.member = u16::MAX;
                match (g.wavetable, &mut z.region.table) {
                    (Some(read), Some(table)) => Arc::make_mut(table).read = read,
                    (None, table @ Some(_)) => {
                        *table = None;
                        if let Some((root, rate)) = restore_root_rate(zone_table, user_zones) {
                            (z.root, z.sample_rate) = (root, rate);
                        }
                    }
                    _ => {}
                }
            }
        }
        found
    }

    /// The user zones' ids, in the order `set_num_user_zones` made them
    /// (`%NI_USER_ZONE_IDS`).
    pub fn user_zone_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.user_zones.iter().map(|z| z.id)
    }

    /// `get_loop_par` for zone `id`, reading sparse user loop parameters when
    /// `id` is a user zone and the flat SoA loop arena for saved zones. Zero allocation.
    #[must_use]
    pub fn zone_loop_par(&self, id: u32, index: i32, par: i32) -> i32 {
        if let Some(row) = self.zone_table.row_of(id) {
            return self.zone_table.loop_par(row, index, par);
        }
        if let Some(u) = self.user_zones.iter().find(|u| u.id == id) {
            return u.loop_par(index, par);
        }
        0
    }

    /// The sparse [`LoopPar`] table for user zone `id`, if present.
    #[must_use]
    pub fn user_loop_pars_for(&self, id: u32) -> Option<&[LoopPar; 8]> {
        self.user_zones
            .iter()
            .find(|u| u.id == id)
            .map(|u| &u.loop_pars)
    }

    /// `set_num_user_zones(n)`: `n` empty user zones with consecutive ids
    /// after the highest saved one, replacing any made before (Kontakt
    /// 8.13: ids 181963, 181964 after the instrument's 181962; a second
    /// call with 3 left 3 zones from the same first id). All parameters 0
    /// but the root key, 36.
    pub fn make_user_zones(&mut self, n: usize) {
        let old_ids: Vec<u32> = self.user_zones.iter().map(|z| z.id).collect();
        self.zones.retain(|z| !old_ids.contains(&z.id));
        self.user_zones.clear();
        self.user_loaded.clear();
        let first = self
            .zone_table
            .ids
            .iter()
            .map(|&id| id + 1)
            .max()
            .unwrap_or(0);
        let count = n.min(MAX_USER_ZONES);
        self.user_zones.reserve(count);
        for i in 0..count {
            let id = first + i as u32;
            self.user_zones.push(UserZone {
                id,
                name: "".into(),
                group: 0,
                keys: 0..=0,
                velocities: 0..=0,
                root: USER_ROOT,
                start: 0,
                end: 0,
                rate: 0,
                source: None,
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
    pub fn set_user_sample(&mut self, zone: u32, sample: Arc<ir::Sample>) -> bool {
        let Some(u) = self.user_zones.iter_mut().find(|z| z.id == zone) else {
            return false;
        };
        u.name = zone_name(&sample.name).into();
        (u.start, u.end, u.rate) = (0, sample.frames, sample.rate);
        u.source = Some(sample);
        true
    }

    /// `set_loop_par` on a user zone (clamped as Kontakt 8.13 reads it back:
    /// a length ends at the sample's end). False for a zone that is not a
    /// user zone or a loop past the eighth.
    pub fn set_user_loop(&mut self, zone: u32, index: i32, par: i32, value: i32) -> bool {
        let Some(u) = self.user_zones.iter_mut().find(|z| z.id == zone) else {
            return false;
        };
        let frames = i32::try_from(u.source.as_ref().map_or(0, |s| s.frames)).unwrap_or(i32::MAX);
        let Some(l) = usize::try_from(index)
            .ok()
            .and_then(|i| u.loop_pars.get_mut(i))
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
        let loops = user_loops(&u.loop_pars);
        for z in self.zones.iter_mut().filter(|z| z.id == zone) {
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
pub fn user_loops(pars: &[LoopPar; 8]) -> Option<Arc<[kontra_voice::Loop]>> {
    let n = |v: i32| u64::try_from(v).unwrap_or(0);
    Region::pack_loops(
        pars.iter()
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
                tune_ratio: 2f32.powf(l.tuning as f32 / 1200.0),
            }),
    )
}

/// Moves one edge of a range; the other follows when they would cross
/// (Kontakt 8.13: a high key set below the low key takes the low with it).
fn set_edge(range: &mut RangeInclusive<u8>, low: bool, v: u8) {
    let (lo, hi) = (*range.start(), *range.end());
    *range = if low { v..=hi.max(v) } else { lo.min(v)..=v };
}

/// A sample's name as a zone is called: no folder, no extension.
#[must_use]
pub fn zone_name(sample: &str) -> &str {
    let file = sample.rsplit(['/', '\\']).next().unwrap_or(sample);
    file.rsplit_once('.').map_or(file, |(stem, _)| stem)
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use super::{Region, ir};
    use crate::Zone;
    use crate::soa::{ZoneMatch, ZonePlay};

    #[test]
    fn a_zone_is_called_by_its_files_stem() {
        assert_eq!(
            super::zone_name("Resources/wavetables/Warmth.wav"),
            "Warmth"
        );
        assert_eq!(super::zone_name("a\\b\\Buzz Cut.ncw"), "Buzz Cut");
        assert_eq!(super::zone_name("plain"), "plain");
    }

    #[test]
    fn zone_and_region_footprints_stay_compact() {
        let region = size_of::<Region>();
        let zone = size_of::<Zone>();
        let soa_per_zone =
            size_of::<ZoneMatch>() + size_of::<ZonePlay>() + size_of::<u32>() + size_of::<u32>();
        let effect = size_of::<ir::Effect>();
        assert!(region <= 64, "Region grew to {region} B");
        // 224 B: #93 added `envelopes: Vec<ZoneEnvelope>` (24 B) after the 200 B measure.
        assert!(zone <= 224, "plan::Zone grew to {zone} B");
        assert_eq!(
            soa_per_zone, 56,
            "SoA per-saved-zone footprint grew to {soa_per_zone} B"
        );
        assert!(effect <= 232, "ir::Effect grew to {effect} B");
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

/// Converts a sample frame offset to microseconds, rounded to nearest µs.
fn frames_to_us(frame: u32, rate: u32) -> i32 {
    let rate = u64::from(rate.max(1));
    let us = (u64::from(frame) * 1_000_000 + rate / 2) / rate;
    i32::try_from(us).unwrap_or(i32::MAX)
}

/// Sets fade `i` of `fades` (low key, high key, low velocity, high velocity)
/// within `width`, shortening its pair so the two never overlap: the reference 8.13
/// reads a 12-key zone's fades 12 then 4 and 8 (the user_zones_fades probe).
pub(crate) fn share<T: Copy + Into<i32> + TryFrom<i32>>(
    fades: &mut [T],
    i: usize,
    value: i32,
    width: i32,
) {
    let v = value.clamp(0, width);
    let other = i ^ 1;
    let Some(&held) = fades.get(other) else {
        return;
    };
    let rest = (width - v).min(held.into());
    if let (Ok(v), Ok(rest)) = (T::try_from(v), T::try_from(rest))
        && let Some(slot) = fades.get_mut(i)
    {
        *slot = v;
        if let Some(slot) = fades.get_mut(other) {
            *slot = rest;
        }
    }
}
