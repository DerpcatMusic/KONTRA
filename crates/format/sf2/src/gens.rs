//! SoundFont generators (§8.1): their numbers, defaults, and how a zone's
//! list folds into values (§9.4).

use kontra_format::Notes;
use kontra_ir as ir;

use crate::parser::Generator;

pub(crate) const START_OFFSET: u16 = 0;
pub(crate) const END_OFFSET: u16 = 1;
pub(crate) const START_LOOP_OFFSET: u16 = 2;
pub(crate) const END_LOOP_OFFSET: u16 = 3;
pub(crate) const START_COARSE_OFFSET: u16 = 4;
pub(crate) const MOD_LFO_TO_PITCH: u16 = 5;
pub(crate) const VIB_LFO_TO_PITCH: u16 = 6;
pub(crate) const MOD_ENV_TO_PITCH: u16 = 7;
pub(crate) const INITIAL_FILTER_FC: u16 = 8;
pub(crate) const INITIAL_FILTER_Q: u16 = 9;
pub(crate) const MOD_LFO_TO_FILTER_FC: u16 = 10;
pub(crate) const MOD_ENV_TO_FILTER_FC: u16 = 11;
pub(crate) const END_COARSE_OFFSET: u16 = 12;
pub(crate) const MOD_LFO_TO_VOLUME: u16 = 13;
pub(crate) const CHORUS_SEND: u16 = 15;
pub(crate) const REVERB_SEND: u16 = 16;
pub(crate) const PAN: u16 = 17;
pub(crate) const DELAY_MOD_LFO: u16 = 21;
pub(crate) const FREQ_MOD_LFO: u16 = 22;
pub(crate) const DELAY_VIB_LFO: u16 = 23;
pub(crate) const FREQ_VIB_LFO: u16 = 24;
pub(crate) const DELAY_MOD_ENV: u16 = 25;
pub(crate) const ATTACK_MOD_ENV: u16 = 26;
pub(crate) const HOLD_MOD_ENV: u16 = 27;
pub(crate) const DECAY_MOD_ENV: u16 = 28;
pub(crate) const SUSTAIN_MOD_ENV: u16 = 29;
pub(crate) const RELEASE_MOD_ENV: u16 = 30;
pub(crate) const KEY_TO_MOD_ENV_HOLD: u16 = 31;
pub(crate) const KEY_TO_MOD_ENV_DECAY: u16 = 32;
pub(crate) const DELAY_VOL_ENV: u16 = 33;
pub(crate) const ATTACK_VOL_ENV: u16 = 34;
pub(crate) const HOLD_VOL_ENV: u16 = 35;
pub(crate) const DECAY_VOL_ENV: u16 = 36;
pub(crate) const SUSTAIN_VOL_ENV: u16 = 37;
pub(crate) const RELEASE_VOL_ENV: u16 = 38;
pub(crate) const KEY_TO_VOL_ENV_HOLD: u16 = 39;
pub(crate) const KEY_TO_VOL_ENV_DECAY: u16 = 40;
pub(crate) const INSTRUMENT: u16 = 41;
pub(crate) const KEY_RANGE: u16 = 43;
pub(crate) const VEL_RANGE: u16 = 44;
pub(crate) const START_LOOP_COARSE_OFFSET: u16 = 45;
pub(crate) const KEYNUM: u16 = 46;
pub(crate) const VELOCITY: u16 = 47;
pub(crate) const INITIAL_ATTENUATION: u16 = 48;
pub(crate) const END_LOOP_COARSE_OFFSET: u16 = 50;
pub(crate) const COARSE_TUNE: u16 = 51;
pub(crate) const FINE_TUNE: u16 = 52;
pub(crate) const SAMPLE_ID: u16 = 53;
pub(crate) const SAMPLE_MODES: u16 = 54;
pub(crate) const SCALE_TUNING: u16 = 56;
pub(crate) const EXCLUSIVE_CLASS: u16 = 57;
pub(crate) const OVERRIDING_ROOT_KEY: u16 = 58;

/// Generators 0..=60 (60 is `endOper`, never stored).
pub(crate) const COUNT: usize = 61;

/// Stored but not carried: each is reported with the first value that
/// differs from its default.
pub(crate) const UNSUPPORTED: [u16; 8] = [
    CHORUS_SEND,
    REVERB_SEND,
    KEY_TO_MOD_ENV_HOLD,
    KEY_TO_MOD_ENV_DECAY,
    KEY_TO_VOL_ENV_HOLD,
    KEY_TO_VOL_ENV_DECAY,
    KEYNUM,
    VELOCITY,
];

const NAMES: [&str; COUNT] = [
    "startAddrsOffset",
    "endAddrsOffset",
    "startloopAddrsOffset",
    "endloopAddrsOffset",
    "startAddrsCoarseOffset",
    "modLfoToPitch",
    "vibLfoToPitch",
    "modEnvToPitch",
    "initialFilterFc",
    "initialFilterQ",
    "modLfoToFilterFc",
    "modEnvToFilterFc",
    "endAddrsCoarseOffset",
    "modLfoToVolume",
    "unused1",
    "chorusEffectsSend",
    "reverbEffectsSend",
    "pan",
    "unused2",
    "unused3",
    "unused4",
    "delayModLFO",
    "freqModLFO",
    "delayVibLFO",
    "freqVibLFO",
    "delayModEnv",
    "attackModEnv",
    "holdModEnv",
    "decayModEnv",
    "sustainModEnv",
    "releaseModEnv",
    "keynumToModEnvHold",
    "keynumToModEnvDecay",
    "delayVolEnv",
    "attackVolEnv",
    "holdVolEnv",
    "decayVolEnv",
    "sustainVolEnv",
    "releaseVolEnv",
    "keynumToVolEnvHold",
    "keynumToVolEnvDecay",
    "instrument",
    "reserved1",
    "keyRange",
    "velRange",
    "startloopAddrsCoarseOffset",
    "keynum",
    "velocity",
    "initialAttenuation",
    "reserved2",
    "endloopAddrsCoarseOffset",
    "coarseTune",
    "fineTune",
    "sampleID",
    "sampleModes",
    "reserved3",
    "scaleTuning",
    "exclusiveClass",
    "overridingRootKey",
    "unused5",
    "endOper",
];

/// The generator's spec name.
pub(crate) fn name(g: u16) -> &'static str {
    NAMES.get(usize::from(g)).copied().unwrap_or("unknown")
}

/// Unused and reserved numbers, and `endOper`: never meaningful in a file.
fn reserved(g: u16) -> bool {
    matches!(g, 14 | 18..=20 | 42 | 49 | 55 | 59 | 60)
}

/// The value a generator has when no zone sets it (§8.1.3).
pub(crate) fn default(g: u16) -> i32 {
    match g {
        INITIAL_FILTER_FC => 13_500,
        DELAY_MOD_LFO | DELAY_VIB_LFO | DELAY_MOD_ENV | ATTACK_MOD_ENV | HOLD_MOD_ENV
        | DECAY_MOD_ENV | RELEASE_MOD_ENV | DELAY_VOL_ENV | ATTACK_VOL_ENV | HOLD_VOL_ENV
        | DECAY_VOL_ENV | RELEASE_VOL_ENV => -12_000,
        KEYNUM | VELOCITY | OVERRIDING_ROOT_KEY => -1,
        SCALE_TUNING => 100,
        _ => 0,
    }
}

/// Generators a preset zone may not set (§8.1.2: sample addresses, fixed
/// key and velocity, loop mode, exclusive class, root key): ignored there.
pub(crate) fn instrument_only(g: u16) -> bool {
    matches!(
        g,
        START_OFFSET
            | END_OFFSET
            | START_LOOP_OFFSET
            | END_LOOP_OFFSET
            | START_COARSE_OFFSET
            | END_COARSE_OFFSET
            | START_LOOP_COARSE_OFFSET
            | KEYNUM
            | VELOCITY
            | END_LOOP_COARSE_OFFSET
            | SAMPLE_MODES
            | EXCLUSIVE_CLASS
            | OVERRIDING_ROOT_KEY
    )
}

/// The generators one zone sets, as stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Gens([Option<u16>; COUNT]);

impl Default for Gens {
    fn default() -> Self {
        Self([None; COUNT])
    }
}

impl Gens {
    /// A zone's list up to its `terminal` generator (`instrument` or
    /// `sampleID`, which must come last, §9.4), and that generator's value.
    /// Records after it and misplaced ranges are ignored; reserved and unknown numbers are reported.
    pub(crate) fn read(
        list: &[Generator],
        terminal: u16,
        notes: &mut Notes,
    ) -> (Self, Option<u16>) {
        let mut gens = Self::default();
        for (i, g) in list.iter().enumerate() {
            if g.oper == terminal {
                if i + 1 < list.len() {
                    notes.add(
                        ir::Impact::Cosmetic,
                        "generators after the terminal one",
                        name(terminal),
                    );
                }
                return (gens, Some(g.amount));
            }
            if (g.oper == KEY_RANGE && i != 0)
                || (g.oper == VEL_RANGE && i != 0 && (i != 1 || gens.raw(KEY_RANGE).is_none()))
            {
                continue;
            }
            if reserved(g.oper) || usize::from(g.oper) >= COUNT {
                notes.add(ir::Impact::Cosmetic, "reserved generator", g.oper);
            } else if let Some(slot) = gens.0.get_mut(usize::from(g.oper)) {
                *slot = Some(g.amount);
            }
        }
        (gens, None)
    }

    /// These values with `local`'s on top: a local zone replaces its global
    /// zone's values (§9.4).
    pub(crate) fn over(&self, local: &Self) -> Self {
        let mut out = *self;
        for (o, l) in out.0.iter_mut().zip(local.0) {
            if l.is_some() {
                *o = l;
            }
        }
        out
    }

    pub(crate) fn raw(&self, g: u16) -> Option<u16> {
        self.0.get(usize::from(g)).copied().flatten()
    }

    /// A signed word.
    pub(crate) fn int(&self, g: u16) -> Option<i32> {
        self.raw(g)
            .map(|a| i32::from(i16::from_le_bytes(a.to_le_bytes())))
    }

    /// A low/high byte range.
    pub(crate) fn range(&self, g: u16) -> Option<(u8, u8)> {
        self.raw(g).map(|a| {
            let [lo, hi] = a.to_le_bytes();
            (lo, hi)
        })
    }

    /// The generators set here, in number order.
    pub(crate) fn set(&self) -> impl Iterator<Item = u16> + '_ {
        (0u16..).zip(self.0).filter_map(|(g, v)| v.map(|_| g))
    }
}

/// A generator's value for one voice: the instrument zone's (or the
/// default) plus the preset zone's offset (§9.4); a preset never moves an
/// instrument-only generator.
pub(crate) fn value(inst: &Gens, preset: &Gens, g: u16) -> i32 {
    let base = inst.int(g).unwrap_or_else(|| default(g));
    if instrument_only(g) {
        base
    } else {
        base + preset.int(g).unwrap_or(0)
    }
}
