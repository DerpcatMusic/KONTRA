//! The player's live edits of a library's envelope, filter and EQ values,
//! kept as a layer over the bank (never changing what was imported), and
//! the lock-free telemetry the editor draws its playheads from.
//!
//! Precedence: an override adds its offset to the parameter's normalized
//! (0..=1, KSP) value, whatever the library or its scripts set, clamped;
//! scripts keep moving the parameter and the player's edit rides on top.
//!
//! [`Bank::base`](super::Bank) holds the library's values as the scripts
//! set them; [`Bank::settings`](super::Bank) what voices play, the base with
//! every override applied. Scripts write and read the base.

use super::GroupSettings;
use super::filter::{Knob, Shape};
use super::params::{self, Address, Stage, UNIT};
use super::Engine;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};

/// Overrides one engine holds; further ones are ignored.
pub const MAX_OVERRIDES: usize = 256;

/// A parameter the player can edit. Filter and EQ knobs name their group
/// insert slot; EQ bands count from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Param {
    Attack,
    /// Attack shape, -1..=1.
    Curve,
    Hold,
    Decay,
    Sustain,
    Release,
    Cutoff(u8),
    Resonance(u8),
    Freq(u8, u8),
    Bandwidth(u8, u8),
    Gain(u8, u8),
}

impl Param {
    pub const ENVELOPE: [Self; 6] =
        [Self::Attack, Self::Curve, Self::Hold, Self::Decay, Self::Sustain, Self::Release];

    pub(crate) fn address(self, group: u16) -> Address {
        let knob = |slot, knob| Address::Filter(group, slot, knob);
        match self {
            Self::Attack => Address::Envelope(group, Stage::Attack),
            Self::Curve => Address::Envelope(group, Stage::Curve),
            Self::Hold => Address::Envelope(group, Stage::Hold),
            Self::Decay => Address::Envelope(group, Stage::Decay),
            Self::Sustain => Address::Envelope(group, Stage::Sustain),
            Self::Release => Address::Envelope(group, Stage::Release),
            Self::Cutoff(s) => knob(s, Knob::Cutoff),
            Self::Resonance(s) => knob(s, Knob::Resonance),
            Self::Freq(s, b) => knob(s, Knob::Freq(b)),
            Self::Bandwidth(s, b) => knob(s, Knob::Bandwidth(b)),
            Self::Gain(s, b) => knob(s, Knob::Gain(b)),
        }
    }

    /// The group and parameter an address edits, if the player can edit it.
    pub(crate) fn of(address: Address) -> Option<(u16, Self)> {
        Some(match address {
            Address::Envelope(g, stage) => (
                g,
                match stage {
                    Stage::Attack => Self::Attack,
                    Stage::Curve => Self::Curve,
                    Stage::Hold => Self::Hold,
                    Stage::Decay => Self::Decay,
                    Stage::Sustain => Self::Sustain,
                    Stage::Release => Self::Release,
                    Stage::AhdOnly => return None,
                },
            ),
            Address::Filter(g, s, knob) => (
                g,
                match knob {
                    Knob::Cutoff => Self::Cutoff(s),
                    Knob::Resonance => Self::Resonance(s),
                    Knob::Freq(b) => Self::Freq(s, b),
                    Knob::Bandwidth(b) => Self::Bandwidth(s, b),
                    Knob::Gain(b) => Self::Gain(s, b),
                    _ => return None,
                },
            ),
            _ => return None,
        })
    }

    /// A physical value (seconds, level, curve, normalized knob) as its
    /// normalized KSP value, 0..=1.
    pub fn norm(self, value: f32) -> f32 {
        self.address(0).encode(value) as f32 / UNIT
    }

    /// Inverse of [`norm`](Self::norm).
    pub fn value(self, norm: f32) -> f32 {
        self.address(0).decode((norm.clamp(0.0, 1.0) * UNIT).round() as i32)
    }

    /// What plays: `base` with `offset` added to its normalized value.
    pub fn apply(self, base: f32, offset: f32) -> f32 {
        if offset == 0.0 { base } else { self.value(self.norm(base) + offset) }
    }
}

/// One edit: `offset` on `param`'s normalized value, in one group or in all
/// of them (`None`). Offsets for all groups and for one group add up.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Override {
    pub group: Option<u16>,
    pub param: Param,
    pub offset: f32,
}

/// A part's overrides as saved with it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Edits(pub Vec<Override>);

impl Edits {
    /// The offset stored for exactly `group` and `param`.
    pub fn get(&self, group: Option<u16>, param: Param) -> f32 {
        self.0.iter().find(|o| o.group == group && o.param == param).map_or(0.0, |o| o.offset)
    }

    /// What applies to `group`: its own offset plus the all-groups one.
    pub fn offset(&self, group: u16, param: Param) -> f32 {
        let one = |g| self.get(g, param);
        one(None) + one(Some(group))
    }

    /// Store `o`, replacing its group and parameter's; offset 0 removes it.
    /// False when it changed nothing or there is no room.
    pub fn set(&mut self, o: Override) -> bool {
        let at = self.0.iter().position(|e| e.group == o.group && e.param == o.param);
        match at {
            Some(i) if o.offset == 0.0 => {
                self.0.swap_remove(i);
            }
            Some(i) if self.0[i].offset != o.offset => self.0[i].offset = o.offset,
            None if o.offset != 0.0 && self.0.len() < MAX_OVERRIDES => self.0.push(o),
            _ => return false,
        }
        true
    }

    /// Remove every override of `param` for all groups and for each one.
    pub fn reset(&mut self, param: Param) {
        self.0.retain(|o| o.param != param);
    }
}

impl Engine {
    /// Set one override (offset 0 removes it) and apply it now.
    pub fn set_override(&mut self, o: Override) {
        if self.overrides.set(o) {
            self.refresh(o.group, o.param);
        }
    }

    /// Remove every override: the bank plays the library's values again.
    pub fn clear_overrides(&mut self) {
        while let Some(o) = self.overrides.0.pop() {
            self.refresh(o.group, o.param);
        }
    }

    /// Re-apply every override, as to a newly installed bank.
    pub(super) fn refresh_overrides(&mut self) {
        for i in 0..self.overrides.0.len() {
            let o = self.overrides.0[i];
            self.refresh(o.group, o.param);
        }
    }

    /// Recompute what plays of `param` in `group` (or all) from the base.
    fn refresh(&mut self, group: Option<u16>, param: Param) {
        self.player.touch();
        let Some(bank) = self.bank.as_deref_mut() else {
            return;
        };
        let groups = match group {
            Some(g) => g as usize..(g as usize + 1).min(bank.base.len()),
            None => 0..bank.base.len(),
        };
        for g in groups {
            let address = param.address(g as u16);
            if let Some(base) = params::read(&bank.base, address) {
                let value = param.apply(base, self.overrides.offset(g as u16, param));
                params::write(&mut bank.settings, address, value);
            }
        }
    }

    /// Apply a script's group-level write: to the base, then what plays.
    pub(super) fn write_group(&mut self, address: Address, value: f32) -> bool {
        let Some(bank) = self.bank.as_deref_mut() else {
            return false;
        };
        if !params::write(&mut bank.base, address, value) {
            return false;
        }
        let value = match Param::of(address) {
            Some((g, p)) => {
                let base = params::read(&bank.base, address).unwrap_or(value);
                p.apply(base, self.overrides.offset(g, p))
            }
            None => value,
        };
        params::write(&mut bank.settings, address, value)
    }

    /// Publish what the editor watches: `group`'s envelope and filter
    /// values (playing and base) and this engine's first voices. Relaxed
    /// stores only; no allocation. False without a bank.
    pub fn publish(&self, group: usize, probe: &Probe) -> bool {
        let Some(bank) = self.bank.as_deref() else {
            return false;
        };
        for (layer, settings) in probe.values.iter().zip([&bank.settings, &bank.base]) {
            let mut values = [f32::NAN; PROBE_VALUES];
            if let Some(s) = settings.get(group) {
                if let Some(e) = s.envelope {
                    values[..6].copy_from_slice(&[e.attack, e.curve, e.hold, e.decay, e.sustain, e.release]);
                }
                let units = s.filter.as_deref().map_or(&[][..], |f| f.units());
                for (chunk, unit) in values[6..].chunks_mut(KNOBS).zip(units) {
                    chunk.copy_from_slice(&unit.knobs[..KNOBS]);
                }
            }
            for (slot, v) in layer.iter().zip(values) {
                slot.store(v.to_bits(), Relaxed);
            }
        }
        let mut voices = self.player.voices.iter();
        for slot in &probe.voices {
            let tap = voices.next().map_or(0, |v| {
                Tap {
                    group: v.group as u16,
                    note: v.note,
                    velocity: v.velocity,
                    phase: v.env.phase() as u8,
                    level: v.env.level(),
                }
                .pack()
            });
            slot.store(tap, Relaxed);
        }
        true
    }
}

impl Param {
    /// This parameter's value in one group's settings, if it has it.
    pub fn read(self, s: &GroupSettings) -> Option<f32> {
        params::read(std::slice::from_ref(s), self.address(0))
    }

    pub fn write(self, s: &mut GroupSettings, value: f32) -> bool {
        params::write(std::slice::from_mut(s), self.address(0), value)
    }

    /// The parameters a group has, each with its place in
    /// [`Probe::values`]: the envelope's, then each filter's and EQ band's.
    pub fn of_group(s: &GroupSettings) -> Vec<(usize, Self)> {
        let mut out = Vec::new();
        if s.envelope.is_some() {
            out.extend(Self::ENVELOPE.into_iter().enumerate());
        }
        let units = s.filter.as_deref().map_or(&[][..], |f| f.units());
        for (u, unit) in units.iter().enumerate().take(PROBE_UNITS) {
            let at = 6 + u * KNOBS;
            match unit.shape {
                Shape::Filter(_) | Shape::Model(_) => {
                    out.extend([(at, Self::Cutoff(unit.slot)), (at + 1, Self::Resonance(unit.slot))]);
                }
                Shape::Geq => {}
                Shape::Eq => {
                    for b in 0..unit.sections {
                        let k = at + 3 * b as usize;
                        let slot = unit.slot;
                        out.extend([(k, Self::Freq(slot, b)), (k + 1, Self::Bandwidth(slot, b)), (k + 2, Self::Gain(slot, b))]);
                    }
                }
            }
        }
        out
    }
}

/// Knobs per filter unit in [`Probe::values`].
pub const KNOBS: usize = 9;
/// Filter units per group in [`Probe::values`].
pub const PROBE_UNITS: usize = 4;
/// Envelope (attack, curve, hold, decay, sustain, release), then each
/// filter unit's knobs; NaN where the group has none.
pub const PROBE_VALUES: usize = 6 + PROBE_UNITS * KNOBS;
/// Voices [`Probe::voices`] reports.
pub const PROBE_VOICES: usize = 16;

/// What the audio thread tells the editor about one part, as atomics the
/// editor reads at paint time.
pub struct Probe {
    /// Rack slot and group the editor watches, packed by
    /// [`Probe::watching`]; 0 for none. The editor stores it.
    pub watch: AtomicU64,
    /// The `watch` the values were last published for.
    pub published: AtomicU64,
    /// `f32` bits: playing values, then base values.
    pub values: [[AtomicU32; PROBE_VALUES]; 2],
    /// Packed [`Tap`]s, 0 for none.
    pub voices: [AtomicU64; PROBE_VOICES],
}

impl Default for Probe {
    fn default() -> Self {
        Self {
            watch: AtomicU64::new(0),
            published: AtomicU64::new(0),
            values: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU32::new(f32::NAN.to_bits()))),
            voices: std::array::from_fn(|_| AtomicU64::new(0)),
        }
    }
}

impl Probe {
    pub fn watching(slot: usize, group: usize) -> u64 {
        ((slot as u64) << 32 | group as u64) + 1
    }

    /// Slot and group of a nonzero `watch`.
    pub fn watched(watch: u64) -> Option<(usize, usize)> {
        let w = watch.checked_sub(1)?;
        Some(((w >> 32) as usize, (w & 0xffff_ffff) as usize))
    }

    /// Playing (`layer` 0) or base (1) values, when published for `watch`.
    pub fn read(&self, watch: u64, layer: usize) -> Option<[f32; PROBE_VALUES]> {
        (watch != 0 && self.published.load(Relaxed) == watch)
            .then(|| self.values[layer & 1].each_ref().map(|v| f32::from_bits(v.load(Relaxed))))
    }

    pub fn taps(&self) -> impl Iterator<Item = Tap> + '_ {
        self.voices.iter().filter_map(|v| Tap::unpack(v.load(Relaxed)))
    }
}

/// A playing voice as the editor sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tap {
    pub group: u16,
    pub note: u8,
    pub velocity: u8,
    /// [`super::Phase`] as `u8`.
    pub phase: u8,
    /// Envelope level, 0..=1.
    pub level: f32,
}

impl Tap {
    fn pack(self) -> u64 {
        let level = (self.level.clamp(0.0, 1.0) * 65535.0).round() as u64;
        1 << 63
            | u64::from(self.group) << 32
            | u64::from(self.note & 127) << 25
            | u64::from(self.velocity & 127) << 18
            | u64::from(self.phase & 7) << 16
            | level
    }

    fn unpack(x: u64) -> Option<Self> {
        (x >> 63 == 1).then(|| Self {
            group: (x >> 32) as u16,
            note: (x >> 25) as u8 & 127,
            velocity: (x >> 18) as u8 & 127,
            phase: (x >> 16) as u8 & 7,
            level: (x & 0xffff) as f32 / 65535.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Bank;
    use crate::audio::Sample;
    use crate::import::{Group, Zone};

    fn engine() -> Engine {
        let env = crate::modulation::Ahdsr {
            attack_curve: 0.0,
            attack_ms: 10.0,
            decay_ms: 500.0,
            hold_ms: 0.0,
            release_ms: 250.0,
            sustain: 0.5,
            unknown_flag: 0,
            unknown_tail: Vec::new(),
        };
        let group = Group { volume_env: Some(env), ..Group::default() };
        let sample = Sample { rate: 48000, frames: vec![[0.1; 2]; 100] };
        let bank = Bank::from_samples(vec![group.clone(), group], vec![Zone::default()], vec![(Default::default(), sample)]);
        let mut e = Engine::default();
        e.set_bank(Some(Box::new(bank.unwrap())));
        e
    }

    fn sustain(e: &Engine, g: usize) -> (f32, f32) {
        let bank = e.bank().unwrap();
        (bank.settings[g].envelope.unwrap().sustain, bank.base[g].envelope.unwrap().sustain)
    }

    /// The preload landing (`Engine::upgrade_bank`) keeps what scripts set:
    /// overrides then recompute from it, not from the library's stored values.
    #[test]
    fn script_values_survive_a_bank_upgrade() {
        let mut e = engine();
        assert!(e.write(Address::Envelope(0, Stage::Sustain), 0.4));
        e.upgrade_bank(engine().set_bank(None).unwrap());
        e.set_override(Override { group: Some(0), param: Param::Sustain, offset: 0.0 });
        assert_eq!(sustain(&e, 0), (0.4, 0.4));
    }

    /// Overrides ride on what the library and its scripts set, never replace it.
    #[test]
    fn overrides_offset_script_values_and_reset_to_them() {
        let mut e = engine();
        let address = Address::Envelope(0, Stage::Sustain);
        assert!(e.write(address, 0.4), "a script sets sustain");
        assert_eq!(sustain(&e, 0), (0.4, 0.4));
        // The player raises it in every group.
        e.set_override(Override { group: None, param: Param::Sustain, offset: 0.2 });
        let (playing, base) = sustain(&e, 0);
        assert!((playing - 0.6).abs() < 1e-5 && base == 0.4, "{playing} {base}");
        assert!((sustain(&e, 1).0 - 0.7).abs() < 1e-5, "the untouched group rides too");
        // The script moves it again: the edit stays on top.
        assert!(e.write(address, 0.1));
        assert!((sustain(&e, 0).0 - 0.3).abs() < 1e-5);
        // One group's own offset adds to the all-groups one, clamped.
        e.set_override(Override { group: Some(0), param: Param::Sustain, offset: 0.9 });
        assert_eq!(sustain(&e, 0).0, 1.0);
        assert!((sustain(&e, 1).0 - 0.7).abs() < 1e-5);
        // Reset to the library: the script's value plays again.
        e.clear_overrides();
        assert_eq!(sustain(&e, 0), (0.1, 0.1));
        assert_eq!(sustain(&e, 1), (0.5, 0.5));
    }

    /// Offsets are in the normalized domain, so times move along Kontakt's
    /// log law, and a new bank takes the part's overrides with it.
    #[test]
    fn time_offsets_follow_the_ksp_law_and_survive_a_new_bank() {
        let mut e = engine();
        let release = |e: &Engine| e.bank().unwrap().settings[1].envelope.unwrap().release;
        let norm = Param::Release.norm(0.25);
        e.set_override(Override { group: Some(1), param: Param::Release, offset: 0.1 });
        assert!((release(&e) - Param::Release.value(norm + 0.1)).abs() < 1e-6);
        assert!(release(&e) > 0.5, "0.1 of the knob is more than doubling here: {}", release(&e));
        let old = engine().set_bank(None);
        e.set_bank(old);
        assert!((release(&e) - Param::Release.value(norm + 0.1)).abs() < 1e-6);
    }

    #[test]
    fn taps_round_trip() {
        let tap = Tap { group: 4095, note: 127, velocity: 1, phase: 5, level: 0.5 };
        let back = Tap::unpack(tap.pack()).unwrap();
        assert_eq!((back.group, back.note, back.velocity, back.phase), (4095, 127, 1, 5));
        assert!((back.level - 0.5).abs() < 1e-4);
        assert_eq!(Tap::unpack(0), None);
        assert_eq!(Probe::watched(Probe::watching(15, 4000)), Some((15, 4000)));
    }
}
