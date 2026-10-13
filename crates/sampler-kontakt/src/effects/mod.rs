//! Kontakt effect racks: where they live in a program, and module names.
//!
//! A program holds up to three 8-slot racks as `BParamArray<BParFX,8>`
//! children (0x3a: insert, send, main, in that order) and up to 16 instrument
//! buses (0x45, each with its own rack). Groups keep an insert rack in their
//! private data. Each slot's first child is the effect object, whose chunk
//! ID names the module.

use ni_file::kontakt::{
    StructuredObject,
    objects::{BParFX, BParamArrayBParFX8, InsertBus, Program},
};


pub(crate) const RACK: u16 = 0x3a;
pub(crate) const BUS: u16 = 0x45;

/// Module names by serialization ID (Kontakt's `BParFX*` classes).
const MODULES: &[(u16, &str)] = &[
    (0x10, "Delay (legacy)"),
    (0x11, "Chorus (legacy)"),
    (0x12, "Flanger (legacy)"),
    (0x13, "Gainer"),
    (0x14, "Phaser (legacy)"),
    (0x15, "Reverb (legacy)"),
    (0x16, "Convolution"),
    (0x17, "Send Levels"),
    (0x18, "Filter"),
    (0x19, "Compressor"),
    (0x1a, "Inverter"),
    (0x1b, "DYX"),
    (0x1c, "Limiter"),
    (0x1d, "Saturation"),
    (0x1e, "Distortion"),
    (0x1f, "Stereo Modeller"),
    (0x20, "Lo-Fi"),
    (0x21, "Skreamer"),
    (0x22, "Rotator"),
    (0x23, "Twang"),
    (0x24, "Cabinet"),
    (0x42, "Tape Saturator"),
    (0x43, "Transient Master"),
    (0x44, "Solid G-EQ"),
    (0x46, "Solid Bus Comp"),
    (0x4c, "Feedback Compressor"),
    (0x4d, "Jump"),
    (0x52, "Van51"),
    (0x53, "AC Box"),
    (0x54, "Hot Solo"),
    (0x55, "Cat"),
    (0x56, "DStortion"),
    (0x57, "Plate Reverb"),
    (0x58, "Cry Wah"),
    (0x59, "Reverb"),
    (0x5a, "Replika"),
    (0x5b, "Phasis"),
    (0x5c, "Flair"),
    (0x5d, "Choral"),
    (0x5e, "Core Cell"),
    (0x5f, "Hilbert Limiter"),
    (0x60, "Supercharger"),
    (0x61, "Bass Pro"),
    (0x63, "Psyche Delay"),
    (0x64, "Ring Modulator"),
    (0x65, "Raum"),
    (0x66, "Bite"),
    (0x68, "Freak"),
    (0x69, "Vibrato Chorus"),
    (0x6a, "Wow / Flutter"),
    (0x72, "Gater"),
    (0x74, "Reverse Grain"),
];

pub(crate) fn module_name(id: u16) -> String {
    MODULES
        .iter()
        .find(|(m, _)| *m == id)
        .map_or_else(|| format!("unknown effect {id:#04x}"), |(_, n)| (*n).into())
}

/// One occupied rack slot.
pub(crate) struct Slot {
    pub slot: usize,
    pub module: u16,
    pub version: u16,
    pub bypass: bool,
    pub output_gain: f32,
    pub dry_level: f32,
    /// A script wrote the output gain with `set_engine_par`.
    pub output_set: bool,
    pub public: Vec<u8>,
}

pub(crate) fn rack(
    array: &BParamArrayBParFX8,
    mut report: impl FnMut(usize, ni_file::Error),
) -> Vec<Slot> {
    array
        .items
        .iter()
        .enumerate()
        .filter_map(|(slot, chunk)| {
            let chunk = chunk.as_ref()?; // A clear slot flag is ordinary absence.
            let decoded = BParFX::try_from(chunk).and_then(|fx| {
                let state = fx.params()?;
                let effect = fx.effect().ok_or(ni_file::Error::Static(
                    "Missing effect object in occupied rack slot",
                ))?;
                let object = StructuredObject::try_from(effect)?;
                Ok(Slot {
                    slot,
                    module: effect.id,
                    version: object.version,
                    bypass: state.bypass,
                    output_gain: state.output_gain,
                    dry_level: state.dry_level,
                    public: object.public_data,
                    output_set: false,
                })
            });
            match decoded {
                Ok(slot) => Some(slot),
                Err(error) => {
                    report(slot, error);
                    None
                }
            }
        })
        .collect()
}

/// The program's racks with their locations: instrument insert, send and
/// main, then each bus.
pub(crate) fn program_racks(
    program: &Program,
    writes: &[sampler_ksp::EnginePar],
    mut report: impl FnMut(String, ni_file::Error),
) -> Vec<(String, Vec<Slot>)> {
    let names = ["instrument insert", "instrument send", "instrument main"];
    let mut out = Vec::new();
    let mut append =
        |at: String, generic: Option<i32>, array: Result<BParamArrayBParFX8, ni_file::Error>| {
            match array {
                Ok(array) => {
                    let mut slots = rack(&array, |slot, error| {
                        report(format!("{at} slot {slot}"), error)
                    });
                    if let Some(generic) = generic {
                        apply_writes(&mut slots, writes, -1, generic);
                    }
                    out.push((at, slots));
                }
                Err(error) => report(at, error),
            }
        };
    let mut racks = 0;
    let mut buses = 0;
    for child in &program.0.children {
        match child.id {
            RACK => {
                let name = names
                    .get(racks)
                    .map_or_else(|| format!("rack {racks}"), |n| (*n).into());
                racks += 1;
                append(
                    name,
                    [1, 0, 2].get(racks - 1).copied(),
                    BParamArrayBParFX8::try_from(child),
                );
            }
            BUS => {
                let index = buses;
                buses += 1;
                let array = InsertBus::try_from(child).and_then(|bus| {
                    BParamArrayBParFX8::try_from(
                        bus.0
                            .find_first(RACK)
                            .ok_or(ni_file::Error::Static("Missing instrument bus effect rack"))?,
                    )
                });
                append(format!("bus {index}"), Some(1000 + index), array);
            }
            _ => {}
        }
    }
    out
}

/// Engine value (0..1000000) of an effect level as a linear gain: cubic about
/// 396851 (2^(-4/3)) = unity, so 1000000 is +24 dB. Fitted to stored slots whose
/// saved output gain and dry level follow the script's init writes (396280 ->
/// 0.99570, 303068 -> 0.44539, 560434 -> 2.8164, 3305 -> 5.776e-7).
pub(crate) fn engine_gain(value: i32) -> f32 {
    sampler_core::EngineParameterLaw::CubicGain { unity: 396_851.0 }.decode(value) as f32
}

/// Apply the `set_engine_par` writes a script left at init to a rack's slots.
/// `group` is the group index or -1; `generic` selects the instrument rack
/// (`$NI_SEND_BUS` 0, `$NI_INSERT_BUS` 1, `$NI_MAIN_BUS` 2, `$NI_BUS_OFFSET`
/// + n) or is -1 for a group's inserts.
pub(crate) fn apply_writes(
    slots: &mut [Slot],
    writes: &[sampler_ksp::EnginePar],
    group: i32,
    generic: i32,
) {
    for w in writes
        .iter()
        .filter(|w| w.group == group && w.generic == generic)
    {
        let Some(fx) = usize::try_from(w.slot)
            .ok()
            .and_then(|i| slots.iter_mut().find(|fx| fx.slot == i))
        else {
            continue;
        };
        match w.parameter.trim_start_matches('$') {
            "ENGINE_PAR_EFFECT_BYPASS" | "ENGINE_PAR_SEND_EFFECT_BYPASS" => {
                fx.bypass = w.value != 0
            }
            "ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN" | "ENGINE_PAR_SEND_EFFECT_OUTPUT_GAIN" => {
                fx.output_gain = engine_gain(w.value);
                fx.output_set = true;
            }
            "ENGINE_PAR_SEND_EFFECT_DRY_LEVEL" => fx.dry_level = engine_gain(w.value),
            parameter @ ("ENGINE_PAR_SHAPE" | "ENGINE_PAR_DRIVE" | "ENGINE_PAR_DAMPING") => {
                fx.apply_drive_parameter(parameter, w.value)
            }
            "ENGINE_PAR_GAIN1" if group >= 0 && generic == -1 => fx.apply_eq_gain(0, w.value),
            "ENGINE_PAR_GAIN2" if group >= 0 && generic == -1 => fx.apply_eq_gain(1, w.value),
            "ENGINE_PAR_GAIN3" if group >= 0 && generic == -1 => fx.apply_eq_gain(2, w.value),
            _ => {}
        }
    }
}

/// A module's stored parameters, where the layout is known (byte-exact on
/// every local instance in v1's survey).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Params {
    TimeMod(timemod::Unit),
    /// Linear gain.
    Gainer {
        gain: f32,
    },
    LoFi {
        values: [f32; 4],
        flag: bool,
    },
    /// `$ENGINE_PAR_STEREO` (offset from 100% width), `$ENGINE_PAR_STEREO_PAN`,
    /// `$ENGINE_PAR_STEREO_PSEUDO`.
    StereoModeller {
        spread: f32,
        pan: f32,
        pseudo: bool,
    },
    /// `$ENGINE_PAR_PHASE_INVERT`, `$ENGINE_PAR_LR_SWAP`.
    Inverter {
        invert: bool,
        swap: bool,
    },
    /// 0x1d is the Saturation record: signed Shape, then algorithm selector.
    Saturation {
        shape: f32,
        mode: f32,
    },
    Distortion {
        mode: f32,
        drive: f32,
        damping: f32,
    },
    /// `BParFXCompressor`: integer mode (Classic/Enhanced/Pro),
    /// threshold dB, ratio, attack and release ms, stereo link.
    Compressor {
        mode: f32,
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        link: bool,
    },
    /// Linear level into each instrument send slot; a second table (17
    /// levels, 1.0 locally) of unknown meaning.
    SendLevels {
        sends: Vec<f32>,
        outputs: Vec<f32>,
    },
    /// Kontakt filter type (stored twice), normalized cutoff/resonance and
    /// up to three further values.
    Filter {
        kind: i32,
        cutoff: f32,
        resonance: f32,
        extra: Vec<f32>,
    },
    /// 1-3 band EQ (filter types 22..=24): Hz, octaves, dB.
    Eq {
        bands: Vec<[f32; 3]>,
    },
    /// `BParFXGaloisReverb`, the modern Reverb: normalized room type, time,
    /// size, damping, modulation, diffusion, predelay, high cut, low shelf,
    /// stereo (`$ENGINE_PAR_RV2_*` order).
    Reverb([f32; 10]),
    /// `BParFXReverb`, the legacy Reverb: predelay, size, stereo, colour, damping.
    LegacyReverb([f32; 5]),
    /// `BParFXPlateReverb`: decay, low shelf, high damp, predelay, stereo.
    Plate([f32; 5]),
    /// `BParFXIRC`: the impulse response is an index into the preset's other-files table.
    Convolution(Box<Convolution>),
    /// Decoded storage fields whose DSP/physical parameter laws are not yet modeled.
    Fields(Vec<ni_file::kontakt::objects::EffectField>),
}

pub(crate) use space::Convolution;

impl Slot {
    fn apply_drive_parameter(&mut self, parameter: &str, value: i32) {
        let (at, low, high) = match (parameter, self.params()) {
            ("ENGINE_PAR_SHAPE", Some(Params::Saturation { .. })) => (0, -1., 1.),
            ("ENGINE_PAR_DRIVE", Some(Params::Distortion { .. })) => (4, 0., 1.),
            ("ENGINE_PAR_DAMPING", Some(Params::Distortion { .. })) => (8, 0., 1.),
            _ => return,
        };
        let stored = sampler_core::EngineParameterLaw::Linear { low, high }.decode(value) as f32;
        self.public[at..at + 4].copy_from_slice(&stored.to_le_bytes());
    }

    fn apply_eq_gain(&mut self, band: usize, value: i32) {
        let Some(Params::Eq { bands }) = self.params() else {
            return;
        };
        if band >= bands.len() {
            return;
        }
        // Init-only writes have no live control owner. Edit the validated legacy
        // Hz/octaves/dB record before static translation; native gain law is unverified.
        let db = sampler_core::EngineParameterLaw::Linear {
            low: -18.,
            high: 18.,
        }
        .decode(value) as f32;
        let at = 8 + band * 12 + 8;
        self.public[at..at + 4].copy_from_slice(&db.to_le_bytes());
    }

    pub(crate) fn params(&self) -> Option<Params> {
        let mut r = Reader(&self.public);
        let params = match self.module {
            0x10..=0x12 | 0x14 => Params::TimeMod(timemod::read(self.module, self.version, &mut r)?),
            0x13 => Params::Gainer { gain: r.f32()? },
            0x20 => {
                let bits = r.f32()?;
                let frequency = r.f32()?;
                let flag = r.flag()?;
                Params::LoFi {
                    values: [bits, frequency, r.f32()?, r.f32()?],
                    flag,
                }
            }
            0x1f => Params::StereoModeller {
                spread: r.f32()?,
                pan: r.f32()?,
                pseudo: r.flag()?,
            },
            0x59 => {
                // Kontakt 8.13.1 FUN_140d019f0 (v0x10): a validated u32 room type
                // (0 room, 1 hall), then nine floats. Reading it as f32 turned hall
                // (1) into 1.4e-45 and every hall played as a room.
                let mut v = [0.0; 10];
                v[0] = match r.i32()? {
                    t @ (0 | 1) => t as f32,
                    _ => return None,
                };
                for x in &mut v[1..] {
                    *x = r.f32()?;
                }
                Params::Reverb(v)
            }
            0x15 | 0x57 => {
                if self.version != if self.module == 0x15 { 0x50 } else { 0x10 } {
                    return None;
                }
                let v = [r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?];
                if self.module == 0x15 { Params::LegacyReverb(v) } else { Params::Plate(v) }
            }
            0x16 => {
                let decimation = r.f32()?;
                // The block size, stored as an integer.
                r.take::<4>()?;
                let mut v = [0.0; 8];
                for x in &mut v {
                    *x = r.f32()?;
                }
                let [predelay_ms, e_len, e_lo, e_hi, l_len, l_lo, l_hi, xpoint] = v;
                let mut flags = [false; 5];
                for flag in &mut flags {
                    *flag = r.flag()?;
                }
                Params::Convolution(Box::new(Convolution {
                    decimation,
                    predelay_ms,
                    early: [e_len, e_lo, e_hi],
                    late: [l_len, l_lo, l_hi],
                    xpoint,
                    flags,
                    curve_x: r.list()?,
                    curve_db: r.list()?,
                    ir_index: r.i32()?,
                }))
            }
            0x19 => Params::Compressor {
                mode: r.i32()? as f32,
                threshold_db: r.f32()?,
                ratio: r.f32()?,
                attack_ms: r.f32()?,
                release_ms: r.f32()?,
                link: r.flag()?,
            },
            0x1a => Params::Inverter {
                invert: r.flag()?,
                swap: r.flag()?,
            },
            0x1d => Params::Saturation {
                shape: r.f32()?,
                mode: r.i32()? as f32,
            },
            0x1e => Params::Distortion { mode: r.i32()? as f32, drive: r.f32()?, damping: r.f32()? },
            0x17 => Params::SendLevels {
                sends: r.list()?,
                outputs: r.list()?,
            },
            0x18 => {
                let kind = r.i32()?;
                if (30..=41).contains(&kind) {
                    // Port from v1 0cb7a8a0:src/fx/params.rs::parse_versioned.
                    // Exact version framing comes from the shared verified reader.
                    if matches!(self.version, 0x90..=0x92) {
                        let record = ni_file::kontakt::objects::BParFXFilterRecord::read(
                            self.version,
                            &self.public,
                        )
                        .ok()?;
                        return Some(Params::Filter {
                            kind: record.filter_type,
                            cutoff: record.cutoff,
                            resonance: record.resonance,
                            extra: vec![record.leading_value],
                        });
                    }
                    r.skip_repeats(kind);
                } else if r.i32()? != kind {
                    return None;
                }
                if (22..=24).contains(&kind) {
                    let bands = (21..kind)
                        .map(|_| Some([r.f32()?, r.f32()?, r.f32()?]))
                        .collect::<Option<_>>()?;
                    Params::Eq { bands }
                } else {
                    // Daft (70, 71) stores a leading value first.
                    let leading = if matches!(kind, 70 | 71 | 30..=41) {
                        Some(r.f32()?)
                    } else {
                        None
                    };
                    let (cutoff, resonance) = (r.f32()?, r.f32()?);
                    let mut extra: Vec<f32> = leading.into_iter().collect();
                    while !r.0.is_empty() {
                        extra.push(r.f32()?);
                    }
                    Params::Filter {
                        kind,
                        cutoff,
                        resonance,
                        extra,
                    }
                }
            }
            _ => {
                return ni_file::kontakt::objects::EffectParameters::read(
                    self.module,
                    self.version,
                    &self.public,
                )
                .ok()
                .flatten()
                .map(|p| Params::Fields(p.fields));
            }
        };
        r.0.is_empty().then_some(params)
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.0.split_first_chunk::<N>()?;
        self.0 = rest;
        Some(*head)
    }
    fn f32(&mut self) -> Option<f32> {
        self.take()
            .map(f32::from_le_bytes)
            .filter(|x| x.is_finite())
    }
    fn i32(&mut self) -> Option<i32> {
        self.take().map(i32::from_le_bytes)
    }
    /// Consume the subtype repeats and their flag bytes after the first.
    fn skip_repeats(&mut self, kind: i32) {
        let k = kind.to_le_bytes();
        loop {
            if let Some(rest) = self.0.strip_prefix(&k) {
                self.0 = rest;
            } else if let Some(rest) = self.0.strip_prefix(&[0]).and_then(|r| r.strip_prefix(&k)) {
                self.0 = rest;
            } else {
                return;
            }
        }
    }
    fn flag(&mut self) -> Option<bool> {
        self.take::<1>().map(|[b]| b != 0)
    }
    /// `u32` count, then that many `f32`s.
    fn list(&mut self) -> Option<Vec<f32>> {
        let n = u32::from_le_bytes(self.take()?) as usize;
        (n <= self.0.len() / 4).then_some(())?;
        (0..n).map(|_| self.f32()).collect()
    }
}

/// A report entry: slot, feature, value, reason.
pub(crate) type Note = (usize, String, String, sampler_ir::Reason);
type Notes = Vec<(String, String, sampler_ir::Reason)>;

type Matrix = [[f64; 2]; 2];
const IDENTITY: Matrix = [[1.0, 0.0], [0.0, 1.0]];

/// A color unit at the end of the chain, addressed by its slot when a script may write it.
fn push_color(
    out: &mut Chain,
    slot: usize,
    dynamic: Option<(i32, i32)>,
    config: sampler_ir::fx::color::Config,
) {
    let config = match dynamic {
        Some((group, generic)) => {
            out.lane_slots.push((slot, out.processors.len()));
            config.at(sampler_ir::SlotAddress { group, slot: slot as i32, generic })
        }
        None => config,
    };
    out.processors.push(sampler_ir::Processor::Color(config));
}

fn product(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|i| std::array::from_fn(|j| a[i][0] * b[0][j] + a[i][1] * b[1][j]))
}

/// The stereo matrix of a linear module, and what it leaves out.
fn matrix(params: &Params, _notes: &mut Notes) -> Option<Matrix> {
    Some(match *params {
        Params::Inverter { invert, swap } => {
            // Kontakt 8 Effect Reference: Inverter. Reader FUN_140d01db0,
            // assignment FUN_1409619b0 at 0x140961f91, audio FUN_140908e60:
            // saved +8 is phase invert (+0x1c0), saved +9 is swap (+0x1c1).
            let sign = if invert { -1.0 } else { 1.0 };
            if swap {
                [[0.0, sign], [sign, 0.0]]
            } else {
                [[sign, 0.0], [0.0, sign]]
            }
        }
        _ => return None,
    })
}

/// Where a rack's processors will run.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Scope {
    /// Per voice (group inserts): no memory-heavy modules.
    Voice,
    /// On a summed bus.
    Bus,
}

/// The Kontakt filter type table (names, laws, coverage) lives in `filter.rs`.
pub(crate) use filter::cutoff_octaves;
use sampler_ir::fx::filter::{Gap, Topology};

/// Report a filter that is configured but not fully proved, so a stand-in never passes silently.
fn note_fidelity(notes: &mut Notes, kind: i32, mapped: &filter::Mapped) {
    if mapped.fidelity != sampler_ir::fx::filter::Fidelity::Exact {
        notes.push((
            "filter law".into(),
            format!("{} ({:?}): {}", filter::name(kind).unwrap_or("?"), mapped.fidelity, mapped.caveat),
            sampler_ir::Reason::UnknownLaw,
        ));
    }
}

/// A Filter slot as IR processors, or why its type cannot be configured.
fn filter(
    kind: i32,
    cutoff: f32,
    resonance: f32,
    extra: Option<f32>,
) -> Result<(Vec<sampler_ir::Processor>, filter::Mapped), Gap> {
    let mapped = filter::map(kind, cutoff, resonance, extra)?;
    // The native kernels (Ladder, AR, Daft, Formant I) are admitted by their own arms, which
    // check the record version, the leading Gain value and the knob ranges.
    if !matches!(mapped.config.topology, Topology::Svf | Topology::FormantII) {
        return Err(Gap {
            name: filter::name(kind).unwrap_or("filter"),
            reason: "the stored record does not meet the native kernel's admission",
        });
    }
    let processors = mapped.config.processors(None)?;
    Ok((processors, mapped))
}

/// A rack as serial processors plus the levels its Send Levels slots feed
/// into the instrument's send slots, and what it leaves out.
#[derive(Default)]
pub(crate) struct Chain {
    pub processors: Vec<sampler_ir::Processor>,
    pub sends: Vec<f32>,
    pub notes: Vec<Note>,
    /// Each translated filter/EQ band slot and its index in `processors`, for
    /// modulation targets that name a module slot.
    pub filter_slots: Vec<(usize, usize)>,
    /// Each script-addressed effect module (distortion, saturation, formant,
    /// lo-fi) slot and its index in `processors`: modulation targets name them.
    pub lane_slots: Vec<(usize, usize)>,
    pub send_taps: Vec<SendTap>,
}

pub(crate) struct SendTap {
    pub slot: usize,
    pub position: sampler_ir::VoiceSendPosition,
    pub levels: Vec<f32>,
    pub bypass: bool,
}

/// Keep the amplifier boundary at its physical slot, even when either side
/// has holes or linear processors that can otherwise collapse together.
pub(crate) fn voice_chain(
    slots: &[Slot],
    split: i32,
    dynamic: Option<(i32, i32)>,
    group: i32,
    live_eq: &[usize],
) -> (Chain, usize) {
    let valid = (0..=8).contains(&split);
    let cut = if valid {
        slots.partition_point(|slot| slot.slot < split as usize)
    } else {
        slots.len()
    };
    let mut before = chain_with(
        &slots[..cut],
        Scope::Voice,
        None,
        dynamic,
        (group, -1),
        live_eq,
    );
    let after = chain_with(
        &slots[cut..],
        Scope::Voice,
        None,
        dynamic,
        (group, -1),
        live_eq,
    );
    let boundary = before.processors.len();
    before.processors.extend(after.processors);
    before
        .send_taps
        .extend(after.send_taps.into_iter().map(|mut tap| {
            let sampler_ir::VoiceSendPosition::BeforeAmplitude(n) = tap.position else {
                unreachable!()
            };
            tap.position = sampler_ir::VoiceSendPosition::AfterAmplitude(n);
            tap
        }));
    before.notes.extend(after.notes);
    before.filter_slots.extend(
        after
            .filter_slots
            .into_iter()
            .map(|(slot, index)| (slot, index + boundary)),
    );
    before.lane_slots.extend(
        after
            .lane_slots
            .into_iter()
            .map(|(slot, index)| (slot, index + boundary)),
    );
    if !valid {
        before.notes.push((
            0,
            "amplifier split point".into(),
            split.to_string(),
            sampler_ir::Reason::InvalidValue,
        ));
    }
    (before, boundary)
}

/// Translate one rack. Each slot scales its output by its output gain. The
/// slot's dry level is not mixed back: local presets store 1.0 on Stereo
/// Modeller, Inverter and EQ slots used as gain trims (output gains in
/// whole-dB steps), where an added dry path would contradict the trim.
/// Linear stereo stages fold into one matrix; EQ bands act alike on both
/// channels, so they commute with it.
#[cfg(test)]
pub(crate) fn chain(slots: &[Slot], scope: Scope) -> Chain {
    chain_with(slots, scope, None, None, (-1, 1), &[])
}

/// A decoded impulse response: its sample rate and frames.
pub(crate) type Decoded = (u32, Vec<[f32; 2]>);

/// Where convolution slots find their impulse responses: `load` decodes the
/// response an other-files index names, `store` collects the shaped ones.
pub(crate) struct Impulses<'a> {
    pub store: &'a mut Vec<sampler_ir::Impulse>,
    pub recipes: Option<&'a mut Vec<Convolution>>,
    pub load: &'a mut dyn FnMut(i32) -> Result<Decoded, String>,
}

/// [`chain`], translating convolutions when `impulses` is given (bus scope).
/// With `dynamic` (the rack's `(group, generic)` address) every slot a script
/// may write at runtime becomes a [`sampler_ir::Processor::Mix`] block,
/// bypassed ones included.
pub(crate) fn chain_with(
    slots: &[Slot],
    scope: Scope,
    mut impulses: Option<&mut Impulses>,
    dynamic: Option<(i32, i32)>,
    physical: (i32, i32),
    live_eq: &[usize],
) -> Chain {
    let mut out = Chain::default();
    let mut combined = IDENTITY;
    let mut filters = Vec::new();
    let flush =
        |combined: &mut Matrix, filters: &mut Vec<sampler_ir::Processor>, out: &mut Chain| {
            out.processors.append(filters);
            if *combined != IDENTITY {
                out.processors
                    .push(sampler_ir::Processor::StereoMatrix(*combined));
                *combined = IDENTITY;
            }
        };
    for fx in slots.iter().filter(|fx| dynamic.is_some() || !fx.bypass) {
        let name = module_name(fx.module);
        let mut notes = Vec::new();
        let params = fx.params();
        // Send Levels feed buses, not the signal; everything else a script
        // may bypass or trim at runtime runs inside its own Mix block.
        let mix = dynamic.filter(|_| !matches!(params, Some(Params::SendLevels { .. })));
        let begin = if mix.is_some() {
            flush(&mut combined, &mut filters, &mut out);
            out.processors.len()
        } else {
            0
        };
        // Every module's Output reaches the signal, an Inverter that changes nothing
        // included: Una Corda's tone groups store +6 dB there against -6 dB on their
        // instrument bus (net 0, KONTAKT_REFERENCE.md s.19a/s.24), and its Resonance
        // group's Stereo Modeller +7 dB shows in full (g94 -25.2 dBFS RMS).
        let wet = f64::from(fx.output_gain);
        let gain = if mix.is_some() {
            IDENTITY
        } else {
            [[wet, 0.0], [0.0, wet]]
        };
        // An EQ has no Output control. Of 12,918 EQ slots in the corpus 12,917 store
        // output 1 and dry 1; the one stored 0 and 0 (ANALOG STRINGS' insert rack) is
        // audible in Kontakt and its script never writes the slot's output gain. So
        // the stored value counts for an EQ only when a script wrote it.
        // ponytail: a guess from that corpus count; confirm against Kontakt output.
        let eq_unset = fx.output_gain == 0.0 && !fx.output_set;
        let eq_gain = if eq_unset { IDENTITY } else { gain };
        let mut modelled = true;
        match &params {
            // Solid G-EQ (0x44): `params()` only yields its raw fields; eq.rs owns the mapping.
            _ if fx.module == 0x44 => {
                flush(&mut combined, &mut filters, &mut out);
                if dynamic.is_some() {
                    notes.push(("live control".into(), "scripts cannot address Solid G-EQ knobs".into(), sampler_ir::Reason::NotModeled));
                }
                match eq::solid_geq(fx.version, &fx.public, &mut notes) {
                    Some(p) => {
                        out.processors.push(p);
                        combined = gain;
                    }
                    None => modelled = false,
                }
            }
            Some(Params::Distortion { mode, drive, damping }) => match color::distortion(*mode, *drive, *damping) {
                Some(config) => {
                    flush(&mut combined, &mut filters, &mut out);
                    push_color(&mut out, fx.slot, dynamic, config);
                    combined = gain;
                }
                None => {
                    notes.push(("distortion parameters".into(), format!("{mode}/{drive}/{damping}"), sampler_ir::Reason::InvalidValue));
                    modelled = false;
                }
            },
            // 0x1d is also the Surround Panner (version 0x70): not a shaper.
            Some(Params::Saturation { .. }) if fx.version == 0x70 => modelled = false,
            Some(Params::Saturation { shape, mode }) => match color::saturation(*shape, *mode) {
                Some(config) => {
                    flush(&mut combined, &mut filters, &mut out);
                    push_color(&mut out, fx.slot, dynamic, config);
                    combined = gain;
                }
                None => {
                    notes.push((
                        "saturation algorithm/shape".into(),
                        format!("{mode}/{shape}"),
                        sampler_ir::Reason::UnknownLaw,
                    ));
                    modelled = false;
                }
            },
            Some(Params::TimeMod(unit)) => {
                flush(&mut combined, &mut filters, &mut out);
                // The slot's dry level is the core's dry only outside a Mix block (as for convolution).
                let dry = if mix.is_some() || matches!(unit, timemod::Unit::Phaser { .. }) { 0. } else { f64::from(fx.dry_level) };
                match timemod::config(unit, dry, &mut notes) {
                    Some(config) => {
                        out.processors.push(sampler_ir::Processor::TimeMod(config));
                        combined = gain;
                    }
                    None => modelled = false,
                }
            }
            Some(Params::Filter { kind: 13, cutoff, resonance, .. }) => {
                flush(&mut combined, &mut filters, &mut out);
                let knobs = [*cutoff, *resonance];
                if knobs.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)) {
                    out.processors.push(sampler_ir::Processor::Phaser(knobs));
                    combined = gain;
                } else {
                    notes.push(("Phaser filter parameters".into(), format!("{knobs:?}"), sampler_ir::Reason::InvalidValue));
                    modelled = false;
                }
            }
            Some(Params::Filter {
                kind: kind @ (90 | 91),
                cutoff,
                resonance,
                extra,
            }) => {
                flush(&mut combined, &mut filters, &mut out);
                // Kontakt 8 Filter Reference: Formant I. FUN_1409a80a0 maps
                // Talk/Sharp to core 0/2; FUN_140aeffe0 maps saved Size to core 1.
                // Formant II is the same core on its second table (filter.rs).
                let knobs = extra.first().map(|&size| [*cutoff, *resonance, size]);
                if let Some(knobs) = knobs.filter(|v| v.iter().all(|x| x.is_finite() && (0.0..=1.0).contains(x))) {
                    if *kind == 91 {
                        if dynamic.is_some() {
                            notes.push(("Formant II live controls".into(), "script/modulation lanes exist for Formant I only".into(), sampler_ir::Reason::NotModeled));
                        }
                        match filter(91, knobs[0], knobs[1], Some(knobs[2])) {
                            Ok((processors, mapped)) => {
                                out.processors.extend(processors);
                                note_fidelity(&mut notes, 91, &mapped);
                                combined = gain;
                            }
                            Err(gap) => {
                                notes.push((gap.name.into(), gap.reason.into(), sampler_ir::Reason::NotModeled));
                                modelled = false;
                            }
                        }
                    } else if let Some((group, generic)) = dynamic {
                        out.lane_slots.push((fx.slot, out.processors.len()));
                        out.processors.push(sampler_ir::Processor::LiveFormant {
                            knobs,
                            address: sampler_ir::SlotAddress { group, slot: fx.slot as i32, generic },
                        });
                        combined = gain;
                    } else {
                        out.processors.push(sampler_ir::Processor::Formant(knobs));
                        combined = gain;
                    }
                } else {
                    notes.push(("Formant parameters".into(), "missing Size or outside normalized range".into(), sampler_ir::Reason::InvalidValue));
                    modelled = false;
                }
            }
            Some(Params::LoFi { values, flag }) => {
                flush(&mut combined, &mut filters, &mut out);
                if let Some(config) = color::lofi(values) {
                    push_color(&mut out, fx.slot, dynamic, config);
                    combined = product(gain, combined);
                    if *flag {
                        notes.push((
                            "Lo-Fi third field flag".into(),
                            "true".into(),
                            sampler_ir::Reason::UnknownLaw,
                        ));
                    }
                } else {
                    notes.push((
                        "Lo-Fi parameters".into(),
                        "non-finite or outside normalized range".into(),
                        sampler_ir::Reason::InvalidValue,
                    ));
                    modelled = false;
                }
            }
            Some(Params::Eq { bands }) => {
                let live =
                    scope == Scope::Voice && (dynamic.is_some() || live_eq.contains(&fx.slot));
                if bands.len() == 1 {
                    flush(&mut combined, &mut filters, &mut out);
                    if let Some(p) = eq::legacy_band(bands[0], live, &mut notes) {
                        if live { out.filter_slots.push((fx.slot, out.processors.len())); }
                        out.processors.push(p);
                    } else if bands[0][2] != 0. || live {
                        modelled = false;
                    }
                    notes.push(("native legacy EQ lifecycle".into(),
                        "initial dirty state, modulation buffers, host reset/latency/tail unverified".into(),
                        sampler_ir::Reason::UnknownLaw));
                } else if live {
                    flush(&mut combined, &mut filters, &mut out);
                    let live_bands: Option<Vec<_>> = bands
                        .iter()
                        .map(|band| eq_band(*band, true, &mut notes))
                        .collect();
                    if let Some(live_bands) = live_bands {
                        for p in live_bands {
                            out.filter_slots.push((fx.slot, out.processors.len()));
                            out.processors.push(p);
                        }
                    } else {
                        modelled = false;
                    }
                } else {
                    let generic: Vec<_> = bands.iter().filter_map(|band| eq::generic_band(*band, &mut notes)).collect();
                    filters.extend(
                        generic
                            .chunks(sampler_ir::fx::eq::MAX_BANDS)
                            .filter_map(sampler_ir::fx::eq::Config::new)
                            .map(sampler_ir::Processor::Eq),
                    );
                }
                if bands.len() > 1 {
                    notes.push(("native multiband EQ".into(), "v1/generic fallback; native multiband processing and clocks unverified".into(), sampler_ir::Reason::UnknownLaw));
                }
                combined = product(eq_gain, combined);
            }
            Some(Params::SendLevels { sends, .. }) => {
                flush(&mut combined, &mut filters, &mut out);
                let sends: Vec<_> = sends
                    .iter()
                    .map(|&level| {
                        if level.is_finite() && level >= 0.0 {
                            level
                        } else {
                            notes.push((
                                "send level".into(),
                                "non-finite or negative".into(),
                                sampler_ir::Reason::InvalidValue,
                            ));
                            0.0
                        }
                    })
                    .collect();
                if scope == Scope::Bus && out.sends.is_empty() {
                    out.sends = sends.clone();
                }
                out.send_taps.push(SendTap {
                    slot: fx.slot,
                    position: sampler_ir::VoiceSendPosition::BeforeAmplitude(out.processors.len()),
                    levels: sends,
                    bypass: fx.bypass,
                });
                combined = product(gain, combined);
            }
            Some(Params::Filter {
                kind: kind @ 30..=41,
                cutoff,
                resonance,
                extra,
            }) if !extra.is_empty() && matches!(fx.version, 0x90..=0x92 | 0x95) => {
                // Kontakt 8 Filter Reference: Ladder filters. FUN_140cf4c80
                // maps saved 30..=41 to the 12 rows in FUN_140b00b80.
                // Gain is signed normalized (12 dB per unit); the native kernel
                // retains the record-version cutoff clock.
                let values = [extra[0], *cutoff, *resonance];
                if values.iter().all(|v| v.is_finite())
                    && (-1.0..=1.0).contains(&values[0])
                    && values[1..].iter().all(|v| (0.0..=1.0).contains(v))
                {
                    out.filter_slots
                        .push((fx.slot, out.processors.len() + filters.len()));
                    let mapped = filter::template(*kind).expect("ladder rows are in the table");
                    let mut config = mapped.config;
                    config.drive = f64::from(values[0]);
                    config.cutoff = f64::from(values[1]);
                    config.resonance = f64::from(values[2]);
                    config.binding = Some(sampler_ir::fx::filter::Binding {
                        address: sampler_ir::SlotAddress {
                            group: physical.0,
                            slot: fx.slot as i32,
                            generic: physical.1,
                        },
                        record_version: fx.version,
                    });
                    filters.extend(config.processors(None).expect("ladder rows configure"));
                    note_fidelity(&mut notes, *kind, &mapped);
                    combined = product(gain, combined);
                } else {
                    notes.push((
                        "Ladder LP4 parameters".into(),
                        "non-finite or outside native range".into(),
                        sampler_ir::Reason::InvalidValue,
                    ));
                    modelled = false;
                }
            }
            Some(Params::Filter {
                kind: kind @ 100..=108,
                cutoff,
                resonance,
                ..
            }) if matches!(fx.version, 0x90..=0x92)
                && [*cutoff, *resonance]
                    .iter()
                    .all(|v| (0.0..=1.0).contains(v)) =>
            {
                // Adaptive-resonance family: saved 100..=108 are modes 0..=8 of the
                // native FilterDJ (docs/audit-2026-10-10/w15-ar-native.md). The kernel
                // is checked against original-instruction vectors for all nine modes;
                // host recording acceptance still requires calibrated paired captures.
                out.filter_slots
                    .push((fx.slot, out.processors.len() + filters.len()));
                let mapped = filter::template(*kind).expect("AR rows are in the table");
                let mut config = mapped.config;
                config.cutoff = f64::from(*cutoff);
                config.resonance = f64::from(*resonance);
                filters.extend(config.processors(None).expect("AR rows configure"));
                note_fidelity(&mut notes, *kind, &mapped);
                combined = product(gain, combined);
            }
            Some(Params::Filter {
                kind,
                cutoff,
                resonance,
                extra,
            }) if matches!(kind, 70 | 71) && !extra.is_empty() => {
                // The Daft (stored 70 low pass, 71 high pass): DSP_SYSTEM_INVENTORY
                // "Daft parameter laws and scheduling". The leading value is the
                // Gain control; modulation adds to the saved normalized knob.
                // ponytail: unverified - 70/71 as Daft rests on v1's stored-ID table.
                out.filter_slots
                    .push((fx.slot, out.processors.len() + filters.len()));
                let mapped = filter::template(*kind).expect("Daft rows are in the table");
                let mut config = mapped.config;
                config.drive = f64::from(extra[0]).clamp(0.0, 1.0);
                config.cutoff = f64::from(*cutoff).clamp(0.0, 1.0);
                config.resonance = f64::from(*resonance).clamp(0.0, 1.0);
                filters.extend(config.processors(None).expect("Daft rows configure"));
                note_fidelity(&mut notes, *kind, &mapped);
                combined = product(gain, combined);
            }
            Some(Params::Filter {
                kind,
                cutoff,
                resonance,
                ..
            }) => match filter(*kind, *cutoff, *resonance, None) {
                Ok((processors, mapped)) => {
                    out.filter_slots
                        .push((fx.slot, out.processors.len() + filters.len()));
                    filters.extend(processors);
                    note_fidelity(&mut notes, *kind, &mapped);
                    combined = product(gain, combined);
                }
                Err(gap) => {
                    notes.push((
                        format!("filter type ({}: {})", gap.name, gap.reason),
                        format!("{kind} cutoff {cutoff} resonance {resonance}"),
                        sampler_ir::Reason::NotModeled,
                    ));
                    modelled = false;
                }
            },
            Some(Params::Compressor { mode, threshold_db, ratio, attack_ms, release_ms, link }) => {
                flush(&mut combined, &mut filters, &mut out);
                match dynamics::compressor(*mode, *threshold_db, *ratio, *attack_ms, *release_ms, *link) {
                    Ok(processor) => {
                        out.processors.push(processor);
                        combined = gain;
                    }
                    Err(note) => {
                        notes.push(note);
                        modelled = false;
                    }
                }
            }
            Some(p @ (Params::Reverb(_) | Params::LegacyReverb(_) | Params::Plate(_))) if scope == Scope::Bus => {
                flush(&mut combined, &mut filters, &mut out);
                out.processors.push(sampler_ir::Processor::Space(match p {
                    Params::Reverb(v) => space::galois(v, &mut notes),
                    Params::LegacyReverb(v) => space::legacy(v, &mut notes),
                    Params::Plate(v) => space::plate(v, &mut notes),
                    _ => unreachable!(),
                }));
                combined = gain;
            }
            Some(Params::Convolution(c)) if scope == Scope::Bus && impulses.is_some() => {
                let source = impulses.as_deref_mut().expect("checked above");
                match space::convolution(c, source, &mut notes) {
                    Ok(impulse) => {
                        flush(&mut combined, &mut filters, &mut out);
                        out.processors.push(sampler_ir::Processor::Space(
                            sampler_ir::fx::space::Config::convolution(
                                impulse,
                                if mix.is_some() { 0.0 } else { f64::from(fx.dry_level) },
                                if mix.is_some() { 1.0 } else { wet },
                            ),
                        ));
                    }
                    Err(why) => {
                        notes.push((
                            "impulse response".into(),
                            why,
                            sampler_ir::Reason::NotModeled,
                        ));
                        modelled = false;
                    }
                }
            }
            // A Gainer mixes its slot's dry level with the gained signal,
            // `dry + out * g` (KONTAKT_REFERENCE s.25 measured a fresh module at
            // 0.5 + 0.5 g). Every stored Gainer in the local corpus has dry 0 and
            // output 1, so they stay plain `g`; a nonzero stored dry is honoured.
            Some(Params::Gainer { gain: parameter }) => {
                flush(&mut combined, &mut filters, &mut out);
                out.processors.push(sampler_ir::Processor::Color(color::gainer(
                    f64::from(*parameter) * if mix.is_some() { 1.0 } else { wet },
                    if mix.is_some() { 0.0 } else { f64::from(fx.dry_level) },
                )));
            }
            Some(Params::StereoModeller {
                spread,
                pan,
                pseudo,
            }) => {
                flush(&mut combined, &mut filters, &mut out);
                out.processors
                    .push(sampler_ir::Processor::Color(color::stereo_modeller(*spread, *pan, *pseudo)));
                combined = gain;
            }
            Some(p) => match matrix(p, &mut notes) {
                Some(m) => combined = product(product(gain, m), combined),
                None => modelled = false,
            },
            None => {
                modelled = false;
                // Preserve framing failures instead of reporting them only as missing DSP.
                // Filter subtypes have a separate versioned reader and admission path.
                if fx.module != 0x18
                    && let Err(error) = ni_file::kontakt::objects::EffectParameters::read(
                        fx.module,
                        fx.version,
                        &fx.public,
                    )
                {
                    notes.push((
                        "parameter framing".into(),
                        error.to_string(),
                        sampler_ir::Reason::InvalidValue,
                    ));
                }
            }
        }
        if let Some((group, generic)) = mix {
            flush(&mut combined, &mut filters, &mut out);
            // A modelled module that changes only its level still owns the slot's
            // Output and Bypass.
            if modelled && out.processors.len() == begin {
                out.processors
                    .push(sampler_ir::Processor::StereoMatrix(IDENTITY));
            }
            let count = out.processors.len() - begin;
            if count > 0 {
                let convolution = matches!(params, Some(Params::Convolution(_)));
                let wet = if matches!(params, Some(Params::Eq { .. })) && eq_unset {
                    1.0
                } else {
                    wet
                };
                out.processors.insert(
                    begin,
                    sampler_ir::Processor::Mix {
                        count: count as u16,
                        address: sampler_ir::SlotAddress {
                            group,
                            slot: fx.slot as i32,
                            generic,
                        },
                        dry: if convolution || matches!(fx.module, 0x10..=0x13) {
                            f64::from(fx.dry_level)
                        } else {
                            0.0
                        },
                        wet,
                        bypass: fx.bypass,
                    },
                );
                for (_, at) in out.filter_slots.iter_mut().chain(&mut out.lane_slots) {
                    if *at >= begin {
                        *at += 1;
                    }
                }
            }
        }
        if !modelled {
            out.notes.push((
                fx.slot,
                "effect".into(),
                match color::unproved(fx.module) {
                    Some(why) => format!("{name} v{:#x} len {}: {why}{}", fx.version, fx.public.len(), space::why_unbound(fx.module)),
                    None => format!("{name} v{:#x} len {}{}", fx.version, fx.public.len(), space::why_unbound(fx.module)),
                },
                sampler_ir::Reason::NotModeled,
            ));
        }
        out.notes.extend(
            notes
                .into_iter()
                .map(|(f, v, r)| (fx.slot, format!("{name}: {f}"), v, r)),
        );
    }
    flush(&mut combined, &mut filters, &mut out);
    out
}

/// An instrument bus (`$NI_BUS_OFFSET` + `index`) with the groups routed to it.
pub(crate) struct BusPlan {
    pub index: usize,
    /// The bus fader (linear) and pan.
    pub volume: f32,
    pub pan: f32,
    pub groups: Vec<sampler_ir::GroupRef>,
}

/// The program's instrument buses with the groups routed to them (`routes`
/// pairs a group with its 0-based bus).
pub(crate) fn bus_plans(program: &Program, routes: &[(sampler_ir::GroupRef, u8)]) -> Vec<BusPlan> {
    program
        .0
        .children
        .iter()
        .filter(|child| child.id == BUS)
        .enumerate()
        .filter_map(|(index, child)| {
            let params = InsertBus::try_from(child).ok()?.params().ok()?;
            Some(BusPlan {
                index,
                volume: params.volume,
                pan: params.pan,
                groups: routes
                    .iter()
                    .filter(|&&(_, bus)| usize::from(bus) == index)
                    .map(|&(group, _)| group)
                    .collect(),
            })
        })
        .collect()
}

/// The instrument-level racks as buses: every group feeds an insert bus
/// (the insert rack), whose Send Levels slots feed one bus per send slot
/// (the send rack's effects), and the main rack follows both. A group routed
/// to an instrument bus (`buses`) feeds that bus instead, whose fader and rack
/// run before the insert bus. Changes nothing when the racks do nothing.
pub(crate) fn instrument_buses(
    ir: &mut sampler_ir::Instrument,
    racks: &[(String, Vec<Slot>)],
    buses: &[BusPlan],
    dynamic: bool,
    load: &mut dyn FnMut(i32) -> Result<Decoded, String>,
) -> (Vec<(String, Note)>, Vec<(usize, sampler_ir::BusRef)>) {
    instrument_buses_with_recipes(ir, racks, buses, dynamic, load, None)
}

pub(crate) fn instrument_buses_with_recipes(
    ir: &mut sampler_ir::Instrument,
    racks: &[(String, Vec<Slot>)],
    buses: &[BusPlan],
    dynamic: bool,
    load: &mut dyn FnMut(i32) -> Result<Decoded, String>,
    recipes: Option<&mut Vec<Convolution>>,
) -> (Vec<(String, Note)>, Vec<(usize, sampler_ir::BusRef)>) {
    use sampler_ir::{BusRef, ChainRef, Output, Scope as IrScope, Send, SendPosition};
    let rack = |name: &str| {
        racks
            .iter()
            .find(|(n, _)| n == name)
            .map_or(&[][..], |(_, s)| s.as_slice())
    };
    let mut report = Vec::new();
    let mut take = |name: &str, c: &Chain| {
        report.extend(c.notes.iter().map(|n| (name.to_string(), n.clone())));
    };
    let mut store = std::mem::take(&mut ir.impulses);
    let mut source = Impulses {
        store: &mut store,
        recipes,
        load,
    };
    // `$NI_INSERT_BUS` 1, `$NI_SEND_BUS` 0, `$NI_MAIN_BUS` 2.
    let generic = |n| dynamic.then_some((-1, n));
    let insert = chain_with(
        rack("instrument insert"),
        Scope::Bus,
        Some(&mut source),
        generic(1),
        (-1, 1),
        &[],
    );
    take("instrument insert", &insert);
    let main = chain_with(
        rack("instrument main"),
        Scope::Bus,
        Some(&mut source),
        generic(2),
        (-1, 2),
        &[],
    );
    take("instrument main", &main);
    // A send slot's effect runs on its own bus, fed at the Send Levels slot's level.
    let mut sends = Vec::new();
    for slot in rack("instrument send")
        .iter()
        .filter(|s| dynamic || !s.bypass)
    {
        let mut c = chain_with(
            std::slice::from_ref(slot),
            Scope::Bus,
            Some(&mut source),
            generic(0),
            (-1, 0),
            &[],
        );
        // Port from v1 0cb7a8a0:src/fx/processor.rs (process returns and
        // SendInputs::tap): bypassed returns contribute no dry signal.
        // Keep the real slot control so scripts can re-enable the return.
        if dynamic && !c.processors.is_empty() {
            c.processors.push(sampler_ir::Processor::SendReturnGate {
                address: sampler_ir::SlotAddress {
                    group: -1,
                    slot: slot.slot as i32,
                    generic: 0,
                },
            });
        }
        take("instrument send", &c);
        let level = insert.sends.get(slot.slot).copied().unwrap_or(1.0);
        if !c.processors.is_empty() {
            sends.push((slot.slot, c.processors, f64::from(level)));
        }
    }
    // Instrument buses: only the ones a group feeds and that do something.
    // `$NI_BUS_OFFSET` + the bus number.
    let mut instrument = Vec::new();
    let mut invalid_pan = Vec::new();
    for bus in buses.iter().filter(|b| dynamic || !b.groups.is_empty()) {
        let name = format!("bus {}", bus.index);
        let c = chain_with(
            rack(&name),
            Scope::Bus,
            Some(&mut source),
            generic(1000 + bus.index as i32),
            (-1, 1000 + bus.index as i32),
            &[],
        );
        take(&name, &c);
        let mut processors = c.processors;
        if dynamic {
            // A script sets the bus volume and routes groups to any bus at run
            // time, so every bus exists and its volume is a Mix block's wet level.
            processors.push(sampler_ir::Processor::Mix {
                count: 1,
                address: sampler_ir::SlotAddress {
                    group: -1,
                    slot: sampler_core::BUS_VOLUME_SLOT,
                    generic: 1000 + bus.index as i32,
                },
                dry: 0.0,
                wet: f64::from(bus.volume),
                bypass: false,
            });
            processors.push(sampler_ir::Processor::StereoMatrix(IDENTITY));
        }
        if bus.pan.is_finite() {
            if bus.pan != 0. {
                // NI Bus Insert Effects amplifier: BInsertBus audio 0x140824910
                // calls 0x140823d70 with channel flags 0/1: independent linear balance.
                let left = 1. - bus.pan.clamp(0., 1.);
                let right = (bus.pan + 1.).clamp(0., 1.);
                processors.push(sampler_ir::Processor::StereoMatrix([
                    [f64::from(left), 0.], [0., f64::from(right)],
                ]));
            }
        } else {
            invalid_pan.push((name, (0, "instrument bus pan".into(), bus.pan.to_string(),
                sampler_ir::Reason::InvalidValue)));
        }
        if dynamic || !processors.is_empty() || (bus.volume - 1.0).abs() > 1e-4 {
            instrument.push((bus, processors));
        }
    }
    report.extend(invalid_pan);
    ir.impulses = store;
    let chained = !(insert.processors.is_empty() && sends.is_empty() && main.processors.is_empty());
    if !chained && instrument.is_empty() {
        ir.input_bus = None;
        return (report, Vec::new());
    }
    // Bus order: insert, sends, main, then the instrument buses.
    let main_bus = (!main.processors.is_empty()).then_some(sends.len() + 1);
    let target = main_bus.map_or(Output::Master, |i| Output::Bus(BusRef(i)));
    let add = |ir: &mut sampler_ir::Instrument,
               name: String,
               processors: Vec<sampler_ir::Processor>,
               sends,
               output,
               gain| {
        let chain = (!processors.is_empty()).then(|| {
            let index = ir.buses.len();
            ir.chains.push(sampler_ir::Chain {
                scope: IrScope::Bus(BusRef(index)),
                pre_amplitude: processors,
                post_amplitude: Vec::new(),
            });
            ChainRef(ir.chains.len() - 1)
        });
        ir.buses.push(sampler_ir::Bus {
            name,
            chain,
            sends,
            output,
            gain,
        });
    };
    let unity = sampler_ir::Gain::UNITY;
    let feeds: Vec<_> = sends
        .iter()
        .enumerate()
        .map(|(i, (_, _, level))| Send {
            to: Output::Bus(BusRef(i + 1)),
            gain: sampler_ir::Gain::Linear(*level),
            position: SendPosition::PostChain,
        })
        .collect();
    let send_buses: Vec<_> = sends
        .iter()
        .enumerate()
        .map(|(i, (slot, _, _))| (*slot, BusRef(i + 1)))
        .collect();
    let entry = if chained {
        let mut segments = Vec::new();
        let mut cursor = 0;
        for tap in &insert.send_taps {
            let sampler_ir::VoiceSendPosition::BeforeAmplitude(n) = tap.position else {
                unreachable!()
            };
            let feeds = if tap.bypass {
                Vec::new()
            } else {
                send_buses
                    .iter()
                    .filter_map(|&(slot, bus)| {
                        tap.levels.get(slot).map(|&level| Send {
                            to: Output::Bus(bus),
                            gain: sampler_ir::Gain::Linear(f64::from(level)),
                            position: SendPosition::PostChain,
                        })
                    })
                    .collect()
            };
            segments.push((insert.processors[cursor..n].to_vec(), feeds));
            cursor = n;
        }
        if segments.is_empty() {
            segments.push((insert.processors, feeds));
        } else if cursor < insert.processors.len() {
            segments.push((insert.processors[cursor..].to_vec(), Vec::new()));
        }
        // Each continuation is another summed bus, so its history and send
        // position use the existing preallocated bus DAG.
        let continuation = sends.len() + 1 + usize::from(main_bus.is_some());
        let mut segments = segments.into_iter().enumerate().peekable();
        let (_, (processors, feeds)) = segments.next().expect("insert segment");
        let next = if segments.peek().is_some() {
            Output::Bus(BusRef(continuation))
        } else {
            target
        };
        add(ir, "insert".into(), processors, feeds, next, unity);
        for (slot, processors, _) in sends {
            add(
                ir,
                format!("send {slot}"),
                processors,
                Vec::new(),
                target,
                unity,
            );
        }
        if main_bus.is_some() {
            add(
                ir,
                "main".into(),
                main.processors,
                Vec::new(),
                Output::Master,
                unity,
            );
        }
        while let Some((i, (processors, feeds))) = segments.next() {
            let next = if segments.peek().is_some() {
                Output::Bus(BusRef(continuation + i))
            } else {
                target
            };
            add(
                ir,
                format!("insert continuation {i}"),
                processors,
                feeds,
                next,
                unity,
            );
        }
        for group in &mut ir.groups {
            group.output = Output::Bus(BusRef(0));
        }
        Output::Bus(BusRef(0))
    } else {
        Output::Master
    };
    ir.input_bus = if let Output::Bus(bus) = entry {
        Some(bus)
    } else {
        None
    };
    for (bus, processors) in instrument {
        let at = BusRef(ir.buses.len());
        add(
            ir,
            format!("bus {}", bus.index),
            processors,
            Vec::new(),
            entry,
            if dynamic {
                unity
            } else {
                sampler_ir::Gain::Linear(f64::from(bus.volume))
            },
        );
        ir.bus_addresses.push((1000 + bus.index as i32, at));
        for group in &bus.groups {
            ir.groups[group.0].output = Output::Bus(at);
        }
    }
    (report, send_buses)
}

fn eq_band(
    [hz, octaves, db]: [f32; 3],
    keep_flat: bool,
    notes: &mut Vec<(String, String, sampler_ir::Reason)>,
) -> Option<sampler_ir::Processor> {
    if db == 0.0 && !keep_flat {
        return None;
    }
    if !(hz.is_finite() && octaves.is_finite() && db.is_finite() && hz > 0.0 && octaves > 0.0) {
        notes.push((
            "EQ band".into(),
            format!("{hz} Hz {octaves} oct {db} dB"),
            sampler_ir::Reason::InvalidValue,
        ));
        return None;
    }
    // Measured against Kontakt 8: centre gain is GAIN_SCALE x the displayed dB and
    // alpha = sin(w0) * sinh(ln2 * octaves).
    let q = sampler_core::PeakingEq::q_of_octaves(f64::from(octaves));
    Some(sampler_ir::Processor::Filter(sampler_ir::Filter {
        kind: sampler_ir::FilterKind::Peak {
            gain: sampler_ir::Gain::Decibels(f64::from(db) * sampler_core::PeakingEq::GAIN_SCALE),
        },
        cutoff: sampler_ir::Frequency::Hertz(f64::from(hz)),
        resonance: sampler_ir::Resonance::Q(q),
    }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn compressor_modes_without_a_core_are_not_modeled() {
        for mode in [3i32, -1] {
            let mut bytes = mode.to_le_bytes().to_vec();
            for v in [-12f32, 0.5, 10., 100.] {
                bytes.extend(v.to_le_bytes());
            }
            bytes.push(0);
            let mut fx = slot(0x19, bytes, 1.);
            fx.version = 0x70;
            let built = super::chain(&[fx], super::Scope::Voice);
            assert!(
                built.processors.is_empty(),
                "mode {mode} has no core"
            );
            assert!(built.notes.iter().any(|(_, feature, _, reason)| {
                feature.ends_with("compressor mode") && *reason == sampler_ir::Reason::NotModeled
            }));
        }
    }

    #[test]
    fn live_shape_drive_and_damping_reach_only_the_physical_slot() {
        for (module, payload, parameter, value, expected) in [
            (0x1d, vec![0.0f32, 0.], "ENGINE_PAR_SHAPE", 750_000, 0.75f32),
            (0x1e, vec![0.0f32, 0., 0.], "ENGINE_PAR_DRIVE", 1_000_000, 0.74903756),
            (0x1e, vec![0.0f32, 0., 0.], "ENGINE_PAR_DAMPING", 1_000_000, 0.031603266),
        ] {
            let bytes = payload.iter().flat_map(|v| v.to_le_bytes()).collect();
            let fx = slot(module, bytes, 1.);
            let built = super::chain_with(&[fx], super::Scope::Voice, None,
                Some((0, -1)), (0, -1), &[]);
            assert!(built.notes.is_empty(), "{:?}", built.notes);
            let mut rt = fx_runtime(built.processors, 120., &[[0., 0.], [0.25, -0.25]]);
            let address = sampler_core::EngineParameterAddress {
                parameter: sampler_core::engine_parameter_id(parameter).unwrap(),
                group: 0, slot: 0, generic: -1,
            };
            let mut before = [[0.; 2]; 1]; rt.render(&mut before).unwrap();
            assert!(rt.set_engine_parameter(sampler_core::EngineParameterAddress {
                slot: 1, ..address }, value).is_err());
            rt.set_engine_parameter(address, value).unwrap();
            let mut after = [[0.; 2]; 1]; rt.render(&mut after).unwrap();
            assert!((after[0][0] - expected).abs() < 2e-6,
                "{parameter}: {:?} expected {expected}", after[0]);
        }
    }

    #[test]
    fn instrument_bus_pan_only_uses_native_stereo_balance() {
        use sampler_ir as ir;
        for (pan, expected) in [(-1., [1., 0.]), (-0.5, [1., 0.5]),
            (0., [1., 1.]), (0.5, [0.5, 1.]), (1., [0., 1.])] {
            let mut instrument = ir::Instrument::default();
            instrument.groups.push(ir::Group::default());
            let (notes, _) = super::instrument_buses(&mut instrument, &[],
                &[super::BusPlan { index: 0, volume: 1., pan,
                    groups: vec![ir::GroupRef(0)] }], false, &mut |_| Err("unused".into()));
            assert!(notes.is_empty());
            if pan == 0. { continue; }
            assert_eq!(instrument.buses.len(), 1, "pan-only bus must exist");
            let chain = instrument.buses[0].chain.unwrap();
            let output = fx_probe(instrument.chains[chain.0].pre_amplitude.clone(),
                120., &[[1., 1.]]);
            assert_eq!(output[0], expected);
        }
    }

    #[test]
    fn convolution_auto_gain_keeps_native_double_energy_sum() {
        let recipe = super::Convolution {
            decimation: 1., predelay_ms: 0., early: [1., 20., 20_000.],
            late: [1., 20., 20_000.], xpoint: 0.5,
            flags: [false, true, false, false, false],
            curve_x: Vec::new(), curve_db: Vec::new(), ir_index: 0,
        };
        let mut frames = vec![[0.0001, 0.00005]; 10_001];
        frames[0] = [1., 0.5];
        let impulse = super::restore_impulse(&recipe, (48_000, frames)).unwrap();
        // f32 products, f64 sum, f32 division/sqrt: CConvolver 0x1419b3200..3267.
        assert_eq!(impulse.left[0].to_bits(), 0x3f3502a2);
        assert_eq!(impulse.right[0], impulse.left[0] * 0.5);
    }

    use ni_file::kontakt::Chunk;

    fn legacy_eq_slot(kind: i32) -> super::Slot {
        let mut public = [kind.to_le_bytes(); 2].concat();
        for values in [[300f32, 1., 0.], [1000., 1.5, 0.], [6000., 2., 0.]]
            .into_iter()
            .take((kind - 21) as usize)
        {
            for value in values {
                public.extend(value.to_le_bytes());
            }
        }
        let mut fx = slot(0x18, public, 1.);
        fx.slot = 5;
        fx
    }

    #[test]
    fn init_only_group_eq_gain_writes_reach_static_bands() {
        let initialized = sampler_ksp::initialize(
            "on init\nset_engine_par($ENGINE_PAR_GAIN1, 0, 7, 5, -1)\nset_engine_par($ENGINE_PAR_GAIN1, 250000, 7, 5, -1)\nset_engine_par($ENGINE_PAR_GAIN2, 1000000, 7, 5, -1)\nset_engine_par($ENGINE_PAR_GAIN3, 750000, 7, 5, -1)\nend on",
            sampler_ksp::Limits::LIBRARY,
            &Default::default(),
        ).unwrap();
        assert!(!initialized.writes_effect_slots());
        let writes = initialized.engine_pars();
        for kind in 22..=24 {
            let mut fx = legacy_eq_slot(kind);
            let saved = fx.public.clone();
            apply_writes(std::slice::from_mut(&mut fx), &writes, 7, -1);
            let Some(Params::Eq { bands }) = fx.params() else {
                panic!("legacy EQ record must still decode")
            };
            for (band, values) in bands.iter().enumerate() {
                assert_eq!(values[2], [-9., 18., 9.][band]);
                let at = 8 + band * 12;
                assert_eq!(&fx.public[at..at + 8], &saved[at..at + 8]);
            }
            let built = chain(std::slice::from_ref(&fx), Scope::Voice);
            if kind == 22 {
                let sampler_ir::Processor::Eq(eq) = built.processors[0] else { panic!("native one-band EQ owner"); };
                assert_eq!(eq.bands()[0].gain_db, bands[0][2]);
                assert_eq!(eq.bands()[0].frequency, sampler_core::KontaktLegacyEqSettings::frequency_knob(bands[0][0]));
                continue;
            }
            let eq_bands: Vec<_> = built
                .processors
                .iter()
                .filter_map(|p| match p {
                    sampler_ir::Processor::Eq(c) => Some(c.bands()),
                    _ => None,
                })
                .flatten()
                .collect();
            assert_eq!(eq_bands.len(), bands.len());
            for (band, values) in eq_bands.iter().zip(&bands) {
                assert_eq!(band.law, sampler_ir::fx::eq::Law::Rbj);
                assert_eq!(band.kind, sampler_ir::fx::eq::Kind::Peak);
                assert_eq!(band.frequency, values[0]);
                assert_eq!(band.gain_db, (f64::from(values[2]) * sampler_core::PeakingEq::GAIN_SCALE) as f32);
            }
            assert!(
                built
                    .notes
                    .iter()
                    .all(|n| n.3 == sampler_ir::Reason::UnknownLaw)
            );
        }
    }

    #[test]
    fn init_eq_gain_writes_reject_wrong_owners_and_clamp_own_law() {
        let mut write = sampler_ksp::EnginePar {
            parameter: "$ENGINE_PAR_GAIN1".into(),
            value: 1_000_000,
            group: 7,
            slot: 5,
            generic: -1,
        };
        for (group, physical_slot, generic) in [(8, 5, -1), (7, 4, -1), (7, 5, 1), (-1, 5, 1)] {
            let mut fx = legacy_eq_slot(22);
            let saved = fx.public.clone();
            (write.group, write.slot, write.generic) = (group, physical_slot, generic);
            apply_writes(
                std::slice::from_mut(&mut fx),
                std::slice::from_ref(&write),
                7,
                -1,
            );
            assert_eq!(fx.public, saved);
            // Even an exact instrument rack address must remain outside this route.
            if group == -1 {
                apply_writes(
                    std::slice::from_mut(&mut fx),
                    std::slice::from_ref(&write),
                    group,
                    generic,
                );
                assert_eq!(fx.public, saved);
            }
        }
        (write.group, write.slot, write.generic) = (7, 5, -1);
        for name in [
            "ENGINE_PAR_GAIN2",
            "ENGINE_PAR_GAIN3",
            "ENGINE_PAR_GAIN4",
            "ENGINE_PAR_GAIN01",
        ] {
            let mut fx = legacy_eq_slot(22);
            let saved = fx.public.clone();
            write.parameter = name.into();
            apply_writes(
                std::slice::from_mut(&mut fx),
                std::slice::from_ref(&write),
                7,
                -1,
            );
            assert_eq!(fx.public, saved);
        }
        write.parameter = "ENGINE_PAR_GAIN1".into();
        for value in [i32::MIN, i32::MAX] {
            let mut fx = legacy_eq_slot(22);
            write.value = value;
            apply_writes(
                std::slice::from_mut(&mut fx),
                std::slice::from_ref(&write),
                7,
                -1,
            );
            assert_eq!(
                fx.params(),
                Some(Params::Eq {
                    bands: vec![[300., 1., if value < 0 { -18. } else { 18. }]],
                })
            );
        }
        for variant in 0..5 {
            let mut fx = legacy_eq_slot(22);
            match variant {
                0 => fx.module = 0x44,
                1 => {
                    fx.public.pop();
                }
                2 => fx.public.push(0),
                3 => fx.public[4..8].copy_from_slice(&23i32.to_le_bytes()),
                _ => fx.public[8..12].copy_from_slice(&f32::NAN.to_le_bytes()),
            }
            let saved = fx.public.clone();
            apply_writes(
                std::slice::from_mut(&mut fx),
                std::slice::from_ref(&write),
                7,
                -1,
            );
            assert_eq!(fx.public, saved);
        }
    }

    #[test]
    fn live_eq_retains_flat_bands_and_their_physical_indices() {
        let mut public = Vec::new();
        for kind in [24i32, 24] {
            public.extend(kind.to_le_bytes());
        }
        for values in [[300f32, 1., 0.], [1000., 1., 12.], [6000., 1., 0.]] {
            for value in values {
                public.extend(value.to_le_bytes());
            }
        }
        let slot = super::Slot {
            slot: 5,
            module: 0x18,
            version: 0x92,
            bypass: false,
            output_gain: 1.,
            dry_level: 0.,
            output_set: false,
            public,
        };
        let fixed = super::chain(std::slice::from_ref(&slot), super::Scope::Voice);
        assert_eq!(
            fixed
                .processors
                .iter()
                .map(|p| match p { sampler_ir::Processor::Eq(c) => c.bands().len(), _ => 0 })
                .sum::<usize>(),
            1,
            "the static unmodulated path keeps its existing arithmetic"
        );
        let live = super::chain_with(
            std::slice::from_ref(&slot),
            super::Scope::Voice,
            None,
            Some((7, -1)),
            (7, -1),
            &[],
        );
        let routed = super::chain_with(
            std::slice::from_ref(&slot),
            super::Scope::Voice,
            None,
            None,
            (7, -1),
            &[5],
        );
        assert_eq!(routed.filter_slots, vec![(5, 0), (5, 1), (5, 2)]);
        assert_eq!(
            live.filter_slots.len(),
            3,
            "live EQ must retain physical band owners, including flat bands"
        );
        for (band, &(physical, index)) in live.filter_slots.iter().enumerate() {
            assert_eq!(physical, 5);
            assert!(
                matches!(
                    live.processors[index],
                    sampler_ir::Processor::Filter(sampler_ir::Filter {
                        kind: sampler_ir::FilterKind::Peak { .. },
                        ..
                    })
                ),
                "band {band} must address its actual EQ processor"
            );
        }
    }

    #[test]
    fn ladder_and_daft_cutoff_routes_retain_the_authored_physical_slot() {
        for kind in [33i32, 70, 71] {
            let mut public = kind.to_le_bytes().to_vec();
            if kind == 33 {
                public.push(0);
            }
            public.extend(kind.to_le_bytes());
            for value in [0.2f32, 0.5, 0.3] {
                public.extend(value.to_le_bytes());
            }
            let slot = super::Slot {
                slot: 5,
                module: 0x18,
                version: 0x92,
                bypass: false,
                output_gain: 1.,
                dry_level: 0.,
                output_set: false,
                public,
            };
            for dynamic in [None, Some((7, -1))] {
                let chain = super::chain_with(
                    std::slice::from_ref(&slot),
                    super::Scope::Voice,
                    None,
                    dynamic,
                    (7, -1),
                    &[],
                );
                let index = usize::from(dynamic.is_some());
                assert_eq!(
                    chain.filter_slots,
                    vec![(5, index)],
                    "native filter {kind} loses its addressed cutoff consumer"
                );
                assert!(matches!(
                    chain.processors[index],
                    sampler_ir::Processor::LadderLP4(_) | sampler_ir::Processor::Daft(_)
                ));
            }
        }
    }

    /// A configured-but-unproved filter is reported as an unknown law and nothing else.
    fn only_law_notes(notes: &[(usize, String, String, sampler_ir::Reason)]) -> bool {
        notes.iter().all(|n| n.3 == sampler_ir::Reason::UnknownLaw)
    }

    #[test]
    fn native_ladder_lp4_import_preserves_signed_gain_and_record_version() {
        let mut public = 33i32.to_le_bytes().to_vec();
        public.push(0);
        public.extend(33i32.to_le_bytes());
        for value in [-0.25f32, 0.5, 0.3] {
            public.extend(value.to_le_bytes());
        }
        let slot = super::Slot {
            slot: 3,
            module: 0x18,
            version: 0x92,
            bypass: false,
            output_gain: 1.,
            dry_level: 0.,
            output_set: false,
            public,
        };
        let chain = super::chain_with(
            &[slot],
            super::Scope::Voice,
            None,
            Some((7, -1)),
            (7, -1),
            &[],
        );
        assert!(only_law_notes(&chain.notes), "{:?}", chain.notes);
        assert!(chain.processors.iter().any(|p| matches!(p,
            sampler_ir::Processor::LadderLP4(d) if d.gain == -0.25
                && d.cutoff == 0.5 && (d.resonance - 0.3).abs() < 1e-7
                && d.record_version == 0x92)));
    }

    #[test]
    fn morphology_ladder_lp1_record_selects_the_first_stage() {
        // A saved v0x95 type-30 record: kind repeats, then gain 0, cutoff 1.0, resonance 0.
        let hex = "1e000000001e0000001e0000001e000000001e00000000000000" .to_owned()
            + "0000803f00000000";
        let public: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        let slot = super::Slot {
            slot: 2,
            module: 0x18,
            version: 0x95,
            bypass: false,
            output_gain: 1.,
            dry_level: 0.,
            output_set: false,
            public,
        };
        let chain = super::chain_with(&[slot], super::Scope::Voice, None, Some((7, -1)), (7, -1), &[]);
        assert!(only_law_notes(&chain.notes), "{:?}", chain.notes);
        assert!(chain.processors.iter().any(|p| matches!(p,
            sampler_ir::Processor::LadderLP4(d) if d.taps == 1 && d.cutoff == 1.0
                && d.resonance == 0.0 && d.record_version == 0x95)));
    }

    #[test]
    fn adaptive_resonance_records_become_native_ar_modes() {
        for (kind, mode) in [(100i32, 0u8), (106, 6), (108, 8)] {
            let mut public = [kind.to_le_bytes(), kind.to_le_bytes()].concat();
            for value in [0.5135f32, 0.25] {
                public.extend(value.to_le_bytes());
            }
            let slot = super::Slot {
                slot: 4,
                module: 0x18,
                version: 0x92,
                bypass: false,
                output_gain: 1.,
                dry_level: 0.,
                output_set: false,
                public,
            };
            let chain = super::chain_with(&[slot], super::Scope::Voice, None, None, (7, -1), &[]);
            assert!(chain.notes.is_empty(), "{:?}", chain.notes);
            assert_eq!(chain.filter_slots, vec![(4, 0)]);
            assert!(matches!(chain.processors[0],
                sampler_ir::Processor::Ar(d) if d.mode == mode && d.cutoff == f64::from(0.5135f32)
                    && d.resonance == 0.25));
        }
    }

    #[test]
    fn fx_decode_failures_keep_scope_slots_and_valid_siblings() {
        mod wire {
            include!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/support/chunks.rs"
            ));
        }
        let object = |id, private: &[u8], children: &[u8]| Chunk {
            id,
            data: wire::object(if id == BUS { 0x11 } else { 0x50 }, private, &[], children),
        };
        let mut state = 20u32.to_le_bytes().to_vec();
        state.extend([0; 6]);
        state.extend(1f32.to_le_bytes());
        state.extend(1f32.to_le_bytes());
        state.extend((-1i32).to_le_bytes());
        let mut gain = vec![0, 0x50, 0];
        gain.extend(2f32.to_le_bytes());
        let valid = object(0x25, &state, &wire::chunk(0x13, &gain));
        let array = BParamArrayBParFX8 {
            version: 0x13,
            items: vec![
                None,
                Some(Chunk {
                    id: 0x26,
                    data: vec![],
                }),
                Some(object(0x25, &[], &[])),
                Some(object(0x25, &state, &[])),
                Some(object(0x25, &state, &wire::chunk(0x13, &[0]))),
                Some(Chunk {
                    id: valid.id,
                    data: valid.data.clone(),
                }),
                None,
                None,
            ],
        };
        let encode = |array: &BParamArrayBParFX8| {
            let mut data = vec![0, 0x13, 0];
            data.extend(8u32.to_le_bytes());
            for item in &array.items {
                data.push(u8::from(item.is_some()));
                if let Some(chunk) = item {
                    data.extend(wire::chunk(chunk.id, &chunk.data));
                }
            }
            Chunk { id: RACK, data }
        };
        let empty = BParamArrayBParFX8 {
            version: 0x13,
            items: (0..8).map(|_| None).collect(),
        };
        let mut good = BParamArrayBParFX8 {
            version: 0x13,
            items: (0..8).map(|_| None).collect(),
        };
        good.items[5] = Some(valid);
        let bad = Chunk {
            id: RACK,
            data: vec![0, 0xff, 0xff],
        };
        let program = Program(StructuredObject {
            version: 0xb5,
            private_data: vec![],
            public_data: vec![],
            children: vec![
                encode(&array),
                Chunk {
                    id: bad.id,
                    data: bad.data.clone(),
                },
                encode(&empty),
                Chunk {
                    id: BUS,
                    data: vec![],
                },
                object(BUS, &[], &[]),
                object(BUS, &[], &wire::chunk(bad.id, &bad.data)),
                object(BUS, &[], &wire::chunk(RACK, &encode(&good).data)),
            ],
        });
        let raw: Vec<_> = program.0.children.iter().map(|c| c.data.clone()).collect();
        let mut errors = Vec::new();
        let racks = program_racks(&program, &[], |at, error| {
            errors.push((at, error.to_string()))
        });
        assert_eq!(
            racks.iter().map(|(at, _)| at.as_str()).collect::<Vec<_>>(),
            ["instrument insert", "instrument main", "bus 3"]
        );
        for index in [0, 2] {
            assert_eq!(racks[index].1.len(), 1);
            assert_eq!(
                (racks[index].1[0].slot, racks[index].1[0].module),
                (5, 0x13)
            );
            assert_eq!(
                chain(&racks[index].1, Scope::Voice).processors,
                // Existing rack law: wet Gainer 2 plus saved dry level 1.
                [sampler_ir::Processor::Color(color::gainer(2.0, 1.0))]
            );
        }
        assert_eq!(errors.len(), 8, "{errors:?}");
        for at in [
            "instrument insert slot 1",
            "instrument insert slot 2",
            "instrument insert slot 3",
            "instrument insert slot 4",
            "instrument send",
            "bus 0",
            "bus 1",
            "bus 2",
        ] {
            assert!(
                errors
                    .iter()
                    .any(|(where_, why)| where_ == at && !why.is_empty()),
                "{at}: {errors:?}"
            );
        }
        assert!(
            errors
                .iter()
                .any(|(_, why)| why.contains("Missing effect object"))
        );
        assert!(
            errors
                .iter()
                .any(|(_, why)| why.contains("Missing instrument bus effect rack"))
        );
        assert!(errors.iter().any(|(_, why)| why.contains("ffff")));
        assert_eq!(
            program
                .0
                .children
                .iter()
                .map(|c| c.data.clone())
                .collect::<Vec<_>>(),
            raw
        );
    }

    #[test]
    fn v1_legacy_lowpass_keeps_the_authored_slot_and_knob_laws() {
        let mut payload = 2i32.to_le_bytes().repeat(2);
        for value in [0.5996094f32, 0.] {
            payload.extend(value.to_le_bytes());
        }
        for scope in [Scope::Voice, Scope::Bus] {
            let mut saved = slot(0x18, payload.clone(), 1.);
            saved.slot = 5;
            let out = chain(&[saved], scope);
            assert!(
                !out.notes
                    .iter()
                    .any(|(_, _, _, r)| *r == sampler_ir::Reason::NotModeled),
                "v1 executes legacy type 2: {:?}",
                out.notes
            );
            assert_eq!(out.filter_slots, [(5, 0)]);
            let sampler_ir::Processor::Filter(filter) = out.processors[0] else {
                panic!("missing filter")
            };
            // Saved type 2 is Legacy LP1 (KSP $FILTER_TYPE_LP1POLE order; one pole measured, no resonance).
            assert_eq!(filter.kind, sampler_ir::FilterKind::LowPass { poles: 1 });
            assert_eq!(
                filter.cutoff,
                sampler_ir::Frequency::Hertz(f64::from(43.6f32 * (8.96 * 0.5996094f32).exp2()))
            );
            assert_eq!(
                filter.resonance,
                sampler_ir::Resonance::Q(f64::from(std::f32::consts::FRAC_1_SQRT_2))
            );
        }
        let sampler_ir::Processor::Filter(filter) = filter(2, 0.4, 0.7, None).unwrap().0[0] else {
            panic!("missing filter")
        };
        assert_eq!(
            filter.resonance,
            sampler_ir::Resonance::Q(f64::from(std::f32::consts::FRAC_1_SQRT_2 * 28f32.powf(0.7)))
        );
    }

    #[test]
    fn v1_legacy_highpass_keeps_the_authored_slot_and_knob_laws() {
        let mut payload = 3i32.to_le_bytes().repeat(2);
        for value in [0f32, 0.] {
            payload.extend(value.to_le_bytes());
        }
        for scope in [Scope::Voice, Scope::Bus] {
            let mut saved = slot(0x18, payload.clone(), 1.);
            saved.slot = 5;
            let out = chain(&[saved], scope);
            assert_eq!(
                out.filter_slots,
                [(5, 0)],
                "v1's legacy HP slot was dropped: {:?}",
                out.notes
            );
            let sampler_ir::Processor::Filter(filter) = out.processors[0] else {
                panic!("missing filter")
            };
            assert_eq!(filter.kind, sampler_ir::FilterKind::HighPass { poles: 1 });
            assert_eq!(
                filter.cutoff,
                sampler_ir::Frequency::Hertz(f64::from(43.6f32))
            );
            assert_eq!(
                filter.resonance,
                sampler_ir::Resonance::Q(f64::from(std::f32::consts::FRAC_1_SQRT_2))
            );
        }
        let sampler_ir::Processor::Filter(filter) = filter(3, 0.4, 0.7, None).unwrap().0[0] else {
            panic!("missing filter")
        };
        assert_eq!(
            filter.cutoff,
            sampler_ir::Frequency::Hertz(f64::from(43.6f32 * (8.96 * 0.4f32).exp2()))
        );
        assert_eq!(
            filter.resonance,
            sampler_ir::Resonance::Q(f64::from(std::f32::consts::FRAC_1_SQRT_2 * 28f32.powf(0.7)))
        );
    }

    #[test]
    fn sv_filters_follow_the_measured_laws() {
        let sampler_ir::Processor::Filter(f) = filter(52, 0.293, 0.0, None).unwrap().0[0] else {
            panic!()
        };
        let sampler_ir::Frequency::Hertz(hz) = f.cutoff else {
            panic!()
        };
        assert!((hz / 25.0 / 800f64.powf(0.293) - 1.0).abs() < 1e-6);
        let sampler_ir::Resonance::Q(q) = f.resonance else {
            panic!()
        };
        assert!(
            (q - 1.0 / 2.0).abs() < 1e-9,
            "r = 0 is k = 1.987 + 0.013 = 2"
        );
        // Legacy LP4 needs a Butterworth cascade the IR cannot express; it is a precise gap, not a guess.
        assert!(filter(8, 0.5, 0.5, None).is_err());
        assert!(filter(1, 0.5, 0.5, None).is_err());
    }

    use super::*;

    fn slot(module: u16, public: Vec<u8>, gain: f32) -> Slot {
        Slot {
            slot: 0,
            module,
            version: 0x50,
            bypass: false,
            output_gain: gain,
            dry_level: 1.0,
            output_set: true,
            public,
        }
    }

    // Owned wire fixture: the wrapper's internal enum deliberately differs from the child ID.
    fn effect_fixture(module: u16, version: u16, public: &[u8]) -> BParamArrayBParFX8 {
        mod wire {
            include!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/support/chunks.rs"
            ));
        }
        let mut state = 20u32.to_le_bytes().to_vec();
        state.extend([0; 6]);
        state.extend(1f32.to_le_bytes());
        state.extend(0f32.to_le_bytes());
        state.extend((-1i32).to_le_bytes());
        let child = wire::chunk(module, &wire::object(version, &[], public, &[]));
        let wrapper = wire::object(0x50, &state, &[], &child);
        let mut array = vec![0];
        array.extend(0x13u16.to_le_bytes());
        array.extend(8u32.to_le_bytes());
        for index in 0..8 {
            array.push(u8::from(index == 5));
            if index == 5 {
                array.extend(wire::chunk(0x25, &wrapper));
            }
        }
        BParamArrayBParFX8::try_from(&Chunk {
            id: RACK,
            data: array,
        })
        .unwrap()
    }

    // Authored independently of the decoder schema: native Raum reader
    // 0x140ac5cc0, v0x10, serializes I + 12F + 2B + 3F + B (67 bytes).
    fn raum_payload() -> Vec<u8> {
        // Kontakt 8.13.1 FUN_140ac5cc0 validates the leading u32 as an enum 0..=2.
        let mut bytes = 2i32.to_le_bytes().to_vec();
        bytes.extend(
            [
                0.125f32, 0.25, 0.5, 1., 2., 4., 8., 16., 32., 64., 128., 256.,
            ]
            .into_iter()
            .flat_map(f32::to_le_bytes),
        );
        bytes.extend([0xa5, 0x5a]);
        bytes.extend([-0.5f32, -1., -2.].into_iter().flat_map(f32::to_le_bytes));
        bytes.push(0xff);
        bytes
    }

    #[test]
    fn malformed_effect_payloads_keep_raw_bytes_and_report_physical_framing_errors() {
        use sampler_ir::{Reason, SourceParameterValue};
        let cases = [
            (
                0x13,
                0x50,
                vec![0; 3],
                "Static error: Truncated FX parameter",
            ),
            (
                0x13,
                0x50,
                vec![0; 5],
                "Static error: Trailing FX parameter bytes",
            ),
            (
                0x42,
                0x10,
                vec![0; 12],
                "Static error: Truncated FX parameter",
            ),
            (
                0x17,
                0x51,
                vec![0xff; 4],
                "Static error: Truncated FX parameter list",
            ),
            (
                0x65,
                0x10,
                // Kontakt validates Raum's leading enum before any other read.
                vec![0xde, 0xad, 0xbe, 0xef],
                "Generic error: BParFXRaum 0x10: Rejected",
            ),
            (
                0x65,
                0x10,
                raum_payload()[..66].to_vec(),
                "Static error: Truncated FX parameter",
            ),
            (
                0x65,
                0x10,
                {
                    let mut bytes = raum_payload();
                    bytes.push(0);
                    bytes
                },
                "Static error: Trailing FX parameter bytes",
            ),
        ];
        for (module, version, bytes, expected) in cases {
            let array = effect_fixture(module, version, &bytes);
            let slots = rack(&array, |_, error| panic!("{error}"));
            assert_eq!(slots.len(), 1);
            assert_eq!(
                (slots[0].slot, slots[0].module, slots[0].version),
                (5, module, version)
            );
            assert_eq!(slots[0].public, bytes);
            let records = crate::source_parameters::rack("owned rack", &array).unwrap();
            let saved = records.iter().find(|r| r.object_id == module).unwrap();
            assert_eq!(saved.version, version);
            assert_eq!(saved.location, "owned rack/slot 5/module");
            assert!(saved.fields.iter().any(|f| {
                f.name == "undecoded_public" && f.value == SourceParameterValue::Opaque(bytes.clone())
            }));
            for scope in [Scope::Voice, Scope::Bus] {
                let out = chain_with(&slots, scope, None, Some((7, -1)), (7, -1), &[]);
                assert!(
                    out.processors.is_empty(),
                    "a rejected slot must not become a Mix identity"
                );
                assert!(out.send_taps.is_empty());
                assert_eq!(out.notes.len(), 2, "framing and unsupported DSP are distinct");
                assert!(out.notes.iter().any(|(slot, field, value, reason)| {
                    *slot == 5
                        && field.ends_with(": parameter framing")
                        && value == expected
                        && *reason == Reason::InvalidValue
                }));
                assert!(out.notes.iter().any(|(slot, field, _, reason)| {
                    *slot == 5 && field == "effect" && *reason == Reason::NotModeled
                }));
            }
        }
    }

    #[test]
    fn valid_unimplemented_and_unknown_effects_stay_unadmitted_without_framing_errors() {
        use sampler_ir::{Reason, SourceParameterValue};
        let mut tape: Vec<u8> = [0.5f32, 0.25, 0.75]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        tape.push(0xa5); // Raw byte, not a guessed Boolean or quality ordinal.
        for (module, bytes) in [
            (0x42, tape),
            (0x65, raum_payload()),
            (0xfe, vec![0xde, 0xad, 0xbe, 0xef]),
        ] {
            let array = effect_fixture(module, 0x10, &bytes);
            let slots = rack(&array, |_, error| panic!("{error}"));
            assert_eq!(slots[0].public, bytes);
            let records = crate::source_parameters::rack("owned rack", &array).unwrap();
            let saved = records.iter().find(|r| r.object_id == module).unwrap();
            assert_eq!(
                (saved.version, saved.location.as_str()),
                (0x10, "owned rack/slot 5/module")
            );
            if module == 0x42 {
                assert!(matches!(slots[0].params(), Some(Params::Fields(_))));
                assert!(saved.fields.iter().any(|f| {
                    f.name == "quality" && f.value == SourceParameterValue::Integer(0xa5)
                }));
            } else if module == 0x65 {
                assert_eq!(bytes.len(), 67);
                assert_eq!(&bytes[52..54], &[0xa5, 0x5a]);
                assert_eq!(&bytes[54..58], &(-0.5f32).to_le_bytes());
                assert_eq!(bytes[66], 0xff);
                assert!(
                    matches!(slots[0].params(), Some(Params::Fields(ref fields)) if fields.len() == 19)
                );
                assert_eq!(saved.fields.len(), 19);
                for (name, value) in [
                    ("param_0", SourceParameterValue::Integer(2)),
                    ("param_1", SourceParameterValue::Number(0.125)),
                    ("param_12", SourceParameterValue::Number(256.)),
                    ("param_13", SourceParameterValue::Integer(0xa5)),
                    ("param_14", SourceParameterValue::Integer(0x5a)),
                    ("param_15", SourceParameterValue::Number(-0.5)),
                    ("param_17", SourceParameterValue::Number(-2.)),
                    ("param_18", SourceParameterValue::Integer(0xff)),
                ] {
                    assert!(
                        saved
                            .fields
                            .iter()
                            .any(|f| f.name == name && f.value == value)
                    );
                }
            } else {
                assert!(slots[0].params().is_none());
                assert!(saved.fields.iter().any(|f| {
                    f.name == "undecoded_public"
                        && f.value == SourceParameterValue::Opaque(bytes.clone())
                }));
            }
            for scope in [Scope::Voice, Scope::Bus] {
                let out = chain_with(&slots, scope, None, Some((7, -1)), (7, -1), &[]);
                assert!(out.processors.is_empty());
                assert!(out.send_taps.is_empty());
                assert_eq!(out.notes.len(), 1);
                assert_eq!(
                    (out.notes[0].0, out.notes[0].1.as_str(), out.notes[0].3),
                    (5, "effect", Reason::NotModeled)
                );
            }
        }
    }

    #[test]
    fn unsupported_effect_diagnostics_group_without_public_payload_bytes() {
        let mut values = Vec::new();
        for byte in [0xa1, 0xb2] {
            let mut authored = slot(0xfe, vec![byte; 64], 1.0);
            authored.slot = 6;
            let out = chain(&[authored], Scope::Voice);
            let (physical_slot, feature, value, reason) = out
                .notes
                .iter()
                .find(|(_, feature, _, _)| feature == "effect")
                .expect("missing effect diagnostic");
            assert_eq!(
                (*physical_slot, feature.as_str(), *reason),
                (6, "effect", sampler_ir::Reason::NotModeled)
            );
            assert!(value.contains("unknown effect 0xfe") && value.contains("v0x50"));
            assert!(value.contains("len 64"));
            assert!(!value.contains("head") && !value.contains(&format!("{byte:02x}")));
            values.push(value.clone());
        }
        assert_eq!(
            values[0], values[1],
            "diagnostic groups must not depend on payload content"
        );
    }

    #[test]
    fn native_typed_fx_modes_and_lofi_fields_reach_executable_consumers() {
        // Primitive layouts recovered from gpt-format-fxmod@b7f6af0b dirty
        // native-reader evidence; generated bytes, no installed bank required.
        let mut distortion = 1i32.to_le_bytes().to_vec();
        distortion.extend([0.8f32, 0.2].into_iter().flat_map(f32::to_le_bytes));
        let effect = slot(0x1e, distortion, 1.);
        assert_eq!(effect.params(), Some(Params::Distortion { mode: 1., drive: 0.8, damping: 0.2 }));
        for scope in [Scope::Voice, Scope::Bus] {
            let out = chain(std::slice::from_ref(&effect), scope);
            assert!(out.notes.is_empty(), "{:?}", out.notes);
            assert_eq!(out.processors.first(), Some(&sampler_ir::Processor::Color(color::distortion(1., 0.8, 0.2).unwrap())));
        }
        let mut saturation = 0.5f32.to_le_bytes().to_vec();
        saturation.extend(1i32.to_le_bytes());
        assert_eq!(slot(0x1d, saturation, 1.).params(), Some(Params::Saturation { shape: 0.5, mode: 1. }));
        let mut lofi: Vec<_> = [0.4f32, 0.2].into_iter().flat_map(f32::to_le_bytes).collect();
        lofi.push(0);
        lofi.extend([0.3f32, 0.5].into_iter().flat_map(f32::to_le_bytes));
        let effect = slot(0x20, lofi, 1.);
        assert_eq!(effect.params(), Some(Params::LoFi { values: [0.4, 0.2, 0.3, 0.5], flag: false }));
        for scope in [Scope::Voice, Scope::Bus] {
            let out = chain(std::slice::from_ref(&effect), scope);
            assert!(out.notes.is_empty(), "{:?}", out.notes);
            assert_eq!(out.processors.first(), Some(&sampler_ir::Processor::Color(color::lofi(&[0.4, 0.2, 0.3, 0.5]).unwrap())));
        }
    }

    #[test]
    fn authored_lofi_slot_is_an_executable_processor_in_both_scopes() {
        let mut payload: Vec<u8> = [0.4f32, 0.2]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        payload.push(0); // Native FFBFF: byte before NoiseLevel and NoiseColor.
        payload.extend(0.0f32.to_le_bytes());
        payload.extend(0.5f32.to_le_bytes());
        for scope in [Scope::Voice, Scope::Bus] {
            let out = chain(&[slot(0x20, payload.clone(), 1.)], scope);
            assert!(out.notes.is_empty(), "{:?}", out.notes);
            assert!(!out.processors.is_empty(), "Lo-Fi cannot disappear");
        }
    }

    #[test]
    fn init_shape_drive_and_damping_writes_update_only_addressed_valid_slots() {
        let initialized = sampler_ksp::initialize(
            "on init\nset_engine_par($ENGINE_PAR_SHAPE, 750000, -1, 3, $NI_INSERT_BUS)\nset_engine_par($ENGINE_PAR_DRIVE, 250000, -1, 5, $NI_INSERT_BUS)\nset_engine_par($ENGINE_PAR_DAMPING, 1000000, -1, 5, $NI_INSERT_BUS)\nend on",
            sampler_ksp::Limits::LIBRARY, &Default::default(),
        ).unwrap();
        let writes = initialized.engine_pars();
        let mut saturation = slot(0x1d, [0f32, 0.].into_iter().flat_map(f32::to_le_bytes).collect(), 1.);
        saturation.slot = 3;
        let mut distortion_payload = 1i32.to_le_bytes().to_vec();
        distortion_payload.extend([0.8f32, 0.2].into_iter().flat_map(f32::to_le_bytes));
        let mut distortion = slot(0x1e, distortion_payload, 1.);
        distortion.slot = 5;
        let mut slots = [saturation, distortion];
        let saved: Vec<_> = slots.iter().map(|fx| fx.public.clone()).collect();
        apply_writes(&mut slots, &writes, 0, -1);
        assert_eq!(slots.iter().map(|fx| fx.public.clone()).collect::<Vec<_>>(), saved);
        apply_writes(&mut slots, &writes, -1, 1);
        assert_eq!(slots[0].params(), Some(Params::Saturation { shape: 0.5, mode: 0. }));
        assert_eq!(slots[1].params(), Some(Params::Distortion { mode: 1., drive: 0.25, damping: 1. }));
        slots[1].public.truncate(4);
        apply_writes(&mut slots, &writes, -1, 1);
        assert_eq!(slots[1].public, 1i32.to_le_bytes());
        slots[1].module = 0x1d;
        slots[1].public = saved[0].clone();
        apply_writes(&mut slots, &writes, -1, 1);
        assert_eq!(slots[1].public, saved[0], "Drive/Damping cannot overwrite another module");
    }

    #[test]
    fn classic_saturation_saved_slot_is_admitted_in_both_scopes() {
        assert_eq!(module_name(0x1d), "Saturation");
        let payload: Vec<_> = [0.30065f32, 0.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        for scope in [Scope::Voice, Scope::Bus] {
            let out = chain(&[slot(0x1d, payload.clone(), 0.5)], scope);
            assert!(out.notes.is_empty(), "{:?}", out.notes);
            assert_eq!(out.processors.len(), 2, "saturation and slot output");
        }
    }

    #[test]
    fn analog_distortion_saved_slot_is_admitted_in_both_scopes() {
        let payload: Vec<_> = [0.0f32, 0.523648, 0.0]
            .into_iter().flat_map(f32::to_le_bytes).collect();
        for scope in [Scope::Voice, Scope::Bus] {
            let out = chain(&[slot(0x1e, payload.clone(), 0.5)], scope);
            assert!(out.notes.is_empty(), "{:?}", out.notes);
            assert_eq!(out.processors.len(), 2, "distortion and slot output");
        }
    }

    #[test]
    fn formant_native_parallel_impulse_and_saved_sharp_size_mapping() {
        use sampler_ir as ir;
        for (talk, sharp, size, expected) in [
            (0.0f32, 0.5, 0.5, 0.01663771558f32),
            (0.0, 0.0, 1.0, 0.0361916923f32),
        ] {
            let mut payload = 90i32.to_le_bytes().repeat(2);
            for value in [talk, sharp, size] { payload.extend(value.to_le_bytes()); }
            let built = chain(&[slot(0x18, payload, 1.0)], Scope::Voice);
            assert!(built.notes.is_empty(), "{:?}", built.notes);
            let mut instrument = ir::Instrument::default();
            instrument.assets.push(ir::Asset { location: ir::AssetLocation::Path("probe".into()), encoding: ir::Encoding::Wav, root_key: None, loops: Vec::new() });
            instrument.chains.push(ir::Chain { scope: ir::Scope::Voice, pre_amplitude: built.processors, post_amplitude: vec![] });
            let mut zone = ir::Zone::new(ir::AssetRef(0));
            zone.keys = ir::KeyRange { low: 60, high: 60 };
            zone.chain = Some(ir::ChainRef(0));
            zone.velocity = ir::VelocityResponse::None;
            instrument.zones.push(zone);
            let mut samples = vec![[0.0; 2]; 64]; samples[0] = [1.0, -0.5];
            let pcm = sampler_core::Pcm::new(48000, samples.into_boxed_slice()).unwrap();
            let plan = sampler_core::lower::lower(&instrument, 48000, vec![pcm], |_, plan| Ok(plan)).unwrap();
            let limits = sampler_core::Limits::for_plan(&plan, 4, 4);
            let mut rt = sampler_core::Runtime::new(plan, limits).unwrap();
            rt.trigger(sampler_core::Input { protocol: sampler_core::Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: None }, 60, 1.0).unwrap();
            let mut output = [[0.0; 2]; 8]; rt.render(&mut output).unwrap();
            assert!((output[0][0] - expected).abs() < 1e-7, "{:?} expected {expected}", output[0]);
            assert!((output[0][1] + expected * 0.5).abs() < 1e-7);
        }
    }

    fn fx_runtime(processors: Vec<sampler_ir::Processor>, tempo: f64, input: &[[f32; 2]]) -> sampler_core::Runtime {
        let mut instrument = sampler_ir::Instrument::default();
        instrument.assets.push(sampler_ir::Asset { location: sampler_ir::AssetLocation::Path("probe".into()), encoding: sampler_ir::Encoding::Wav, root_key: None, loops: vec![] });
        instrument.chains.push(sampler_ir::Chain { scope: sampler_ir::Scope::Voice, pre_amplitude: processors, post_amplitude: vec![] });
        let mut zone = sampler_ir::Zone::new(sampler_ir::AssetRef(0));
        zone.keys = sampler_ir::KeyRange { low: 60, high: 60 };
        zone.chain = Some(sampler_ir::ChainRef(0)); zone.velocity = sampler_ir::VelocityResponse::None;
        instrument.zones.push(zone);
        let mut samples = vec![[0.0; 2]; 64]; samples[..input.len()].copy_from_slice(input);
        let pcm = sampler_core::Pcm::new(48000, samples.into_boxed_slice()).unwrap();
        let plan = sampler_core::lower::lower(&instrument, 48000, vec![pcm], |_, p| Ok(p)).unwrap();
        let limits = sampler_core::Limits::for_plan(&plan, 4, 4);
        let mut rt = sampler_core::Runtime::new(plan, limits).unwrap();
        rt.trigger(sampler_core::Input { protocol: sampler_core::Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: None }, 60, 1.0).unwrap();
        rt.set_tempo(tempo).unwrap();
        rt
    }

    fn fx_probe(processors: Vec<sampler_ir::Processor>, tempo: f64, input: &[[f32; 2]]) -> [[f32; 2]; 4] {
        let mut rt = fx_runtime(processors, tempo, input);
        let mut output = [[0.0; 2]; 4]; rt.render(&mut output).unwrap();
        output
    }

    #[test]
    fn enhanced_compressor_uses_native_detector_and_integer_selector() {
        let mut payload = 1i32.to_le_bytes().to_vec();
        for value in [-6.0f32, 0.5, 1.0, 100.0] { payload.extend(value.to_le_bytes()); }
        payload.push(0);
        let mut fx = slot(0x19, payload.clone(), 1.0); fx.version = 0x70;
        let built = chain(&[fx], Scope::Voice);
        assert!(built.notes.is_empty(), "{:?}", built.notes);
        let output = fx_probe(built.processors, 120., &[[1., -0.5]; 4]);
        for (actual, expected) in output.iter().zip([0.999157310f32, 0.997557640, 0.995283782, 0.992414534]) { assert!((actual[0] - expected).abs() < 2e-7, "{actual:?} vs {expected}"); }
        assert_eq!(output[0][1], -0.5);
        *payload.last_mut().unwrap() = 1;
        let mut fx = slot(0x19, payload, 1.0); fx.version = 0x70;
        let linked = chain(&[fx], Scope::Voice);
        assert!(linked.notes.is_empty(), "{:?}", linked.notes);
        assert_eq!(fx_probe(linked.processors, 120., &[[1., -1.]])[0], [1., -1.]);
    }

    #[test]
    fn legacy_phaser_free_speed_phase_and_four_stage_vector() {
        let payload: Vec<_> = [0.5f32, 0.25, 1.0, 1.0, -1.0].into_iter().flat_map(f32::to_le_bytes).collect();
        let built = chain(&[slot(0x14, payload, 1.0)], Scope::Voice);
        assert!(built.notes.is_empty(), "{:?}", built.notes);
        let output = fx_probe(built.processors.clone(), 120., &[[1., -0.5]]);
        let expected = [[0.857850552f32, -0.476720124], [-0.106721222, -0.0511267483], [-0.290372699, 0.0421471819], [-0.274628252, 0.0595008172]];
        for (actual, expected) in output.into_iter().zip(expected) {
            for (a, e) in actual.into_iter().zip(expected) { assert!((a-e).abs() < 2e-7, "{a} vs {e}"); }
        }
        // 120 BPM, two beats per cycle gives the same native 1 Hz oscillator.
        let synced_payload: Vec<_> = [0.5f32, 0.25, 1.0, 1.0, 2.0].into_iter().flat_map(f32::to_le_bytes).collect();
        let synced = chain(&[slot(0x14, synced_payload, 1.0)], Scope::Voice);
        assert!(synced.notes.is_empty(), "{:?}", synced.notes);
        assert_eq!(fx_probe(synced.processors.clone(), 120., &[[1., -0.5]]), output);
        // The native synced getter substitutes 120 BPM below .1 BPM.
        assert_eq!(fx_probe(synced.processors, 0.05, &[[1., -0.5]]), output);
    }

    #[test]
    fn phaser_filter_native_four_stage_impulse() {
        let mut payload = 13i32.to_le_bytes().repeat(2);
        for value in [0.0f32, 0.0] { payload.extend(value.to_le_bytes()); }
        let built = chain(&[slot(0x18, payload, 1.0)], Scope::Voice);
        assert!(built.notes.is_empty(), "{:?}", built.notes);
        let output = fx_probe(built.processors, 120., &[[1., -0.5]]);
        // Cutoff zero: a=(1-.02)/(1+.02); four allpasses, half dry/half wet.
        for (actual, expected) in output.iter().zip([0.926062942f32, -0.136394471, -0.114671811, -0.095316768]) {
            assert!((actual[0] - expected).abs() < 2e-7, "{actual:?} vs {expected}");
            assert!((actual[1] + expected * 0.5).abs() < 2e-7);
        }
    }

    #[test]
    fn v1_formant_slot_is_an_executable_processor_in_both_scopes() {
        let mut payload = 90i32.to_le_bytes().repeat(2);
        for value in [0.25f32, 0.5, 0.5] {
            payload.extend(value.to_le_bytes());
        }
        for scope in [Scope::Voice, Scope::Bus] {
            let out = chain(&[slot(0x18, payload.clone(), 1.)], scope);
            assert!(!out.processors.is_empty(), "v1 executes Formant I");
            assert!(
                !out.notes
                    .iter()
                    .any(|(_, _, _, reason)| *reason == sampler_ir::Reason::NotModeled)
            );
        }
    }

    #[test]
    fn compressor_output_and_bypass_init_writes_use_the_shared_slot_law() {
        // Pinned v1 0cb7a8a0, engine/params.rs: effect_gain = 16*x^3.
        // The shared service's rounded unity differs by less than 0.00005 dB.
        for value in [0, 125_919, 396_851, 560_434, 1_000_000] {
            let native = 16.0 * (f64::from(value) / 1_000_000.0).powi(3);
            let gain = f64::from(engine_gain(value));
            if native > 0.0 {
                assert!((20.0 * (gain / native).log10()).abs() < 0.00005);
            } else {
                assert_eq!(gain, 0.0);
            }
        }
        assert_eq!(engine_gain(-1), 0.0);
        assert_eq!(engine_gain(i32::MAX), engine_gain(1_000_000));
        let mut fx = slot(0x19, Vec::new(), 1.0);
        fx.slot = 1;
        let mut writes = vec![
            sampler_ksp::EnginePar {
                parameter: "$ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN".into(),
                value: 560_434,
                group: -1,
                slot: 1,
                generic: 1,
            },
            sampler_ksp::EnginePar {
                parameter: "$ENGINE_PAR_EFFECT_BYPASS".into(),
                value: 1,
                group: -1,
                slot: 1,
                generic: 1,
            },
        ];
        apply_writes(std::slice::from_mut(&mut fx), &writes, -1, 1);
        assert!(fx.bypass && fx.output_set);
        assert!((20.0 * f64::from(fx.output_gain).log10() - 8.99385).abs() < 0.0001);
        writes[1].value = 0;
        apply_writes(std::slice::from_mut(&mut fx), &writes, -1, 1);
        assert!(!fx.bypass);
    }

    #[test]
    fn addressed_engine_writes_require_dynamic_effect_racks() {
        let script = sampler_ksp::compile_with(
            "on controller\nset_engine_par($ENGINE_PAR_EFFECT_BYPASS, 1, -1, 1, $NI_INSERT_BUS)\nend on",
            48_000, sampler_ksp::Limits::LIBRARY, &[], &Default::default(),
        ).unwrap();
        assert!(
            script.writes_effect_slots(),
            "shared engine writes must retain live slot lanes"
        );
    }

    #[test]
    fn input_tone_marker_names_instrument_insert_entry() {
        let mut i = sampler_ir::Instrument::default();
        i.groups.push(Default::default());
        instrument_buses(
            &mut i,
            &[(
                "instrument insert".into(),
                vec![slot(0x13, 1.0f32.to_le_bytes().to_vec(), 1.)],
            )],
            &[],
            false,
            &mut |_| Err("none".into()),
        );
        assert_eq!(i.input_bus, Some(sampler_ir::BusRef(0)));
        assert_eq!(
            i.groups[0].output,
            sampler_ir::Output::Bus(i.input_bus.unwrap())
        );
        i.validate().unwrap();
    }

    #[test]
    fn dynamic_bypassed_send_returns_add_no_dry_copy() {
        use sampler_ir as ir;
        let render = |dynamic| {
            let mut instrument = ir::Instrument::default();
            instrument.assets.push(ir::Asset {
                location: ir::AssetLocation::Path("probe".into()),
                encoding: ir::Encoding::Wav,
                root_key: None,
                loops: Vec::new(),
            });
            instrument.groups.push(ir::Group::default());
            let mut zone = ir::Zone::new(ir::AssetRef(0));
            zone.group = Some(ir::GroupRef(0));
            zone.keys = ir::KeyRange { low: 60, high: 60 };
            zone.velocity = ir::VelocityResponse::None;
            instrument.zones.push(zone);
            let mut send = slot(0x13, 1.0f32.to_le_bytes().to_vec(), 1.0);
            send.bypass = true;
            send.dry_level = 0.0;
            instrument_buses(
                &mut instrument,
                &[("instrument send".into(), vec![send])],
                &[],
                dynamic,
                &mut |_| Err("no impulse".into()),
            );
            let pcm =
                sampler_core::Pcm::new(48000, vec![[0.25; 2]; 4096].into_boxed_slice()).unwrap();
            let plan =
                sampler_core::lower::lower(&instrument, 48000, vec![pcm], |_, plan| Ok(plan))
                    .unwrap();
            let mut rt = sampler_core::Runtime::new(
                plan,
                sampler_core::Limits {
                    notes: 4,
                    channels: 1,
                    performances: 1,
                    families: 4,
                    expressions: 4,
                    voices: 4,
                    decisions: 8,
                    commands: 8,
                    behaviors: 0,
                    behavior_fuel: 0,
                    behavior_cells: 0,
                    note_cells: 0,
                },
            )
            .unwrap();
            rt.trigger(
                sampler_core::Input {
                    protocol: sampler_core::Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 60,
                    external_id: None,
                },
                60,
                1.0,
            )
            .unwrap();
            let mut output = [[0.0; 2]; 64];
            rt.render(&mut output).unwrap();
            let initial = output[32][0];
            if dynamic {
                let address = sampler_core::EngineParameterAddress {
                    parameter: sampler_core::engine_parameter_id("ENGINE_PAR_SEND_EFFECT_BYPASS")
                        .unwrap(),
                    group: -1,
                    slot: 0,
                    generic: 0,
                };
                let mut settled = [[0.0; 2]; 512];
                rt.set_engine_parameter(address, 0).unwrap();
                rt.render(&mut settled).unwrap();
                assert!(
                    (settled[511][0] - initial * 2.0).abs() < 1e-6,
                    "live send did not return: {} vs {}",
                    settled[511][0],
                    initial * 2.0
                );
                rt.set_engine_parameter(address, 1).unwrap();
                rt.render(&mut settled).unwrap();
                assert!(
                    (settled[511][0] - initial).abs() < 1e-6,
                    "live bypass did not mute return"
                );
            }
            initial
        };
        let (saved, live) = (render(false), render(true));
        assert!(saved > 0.0);
        assert!(
            (live / saved - 1.0).abs() < 1e-6,
            "bypassed send changed dry gain: {saved} -> {live}"
        );
    }

    #[test]
    #[ignore = "installed library metadata probe; run through kontakto-heavy"]
    fn analog_saved_compressor_state() {
        let path = std::path::Path::new(
            "/mnt/MAIN_STORAGE/Libraries/Kontakt/ANALOG STRINGS/Instruments/ANALOG STRINGS.nki",
        );
        if !path.exists() {
            return;
        }
        let chunks = crate::read_chunks(path).unwrap();
        let program = Program::try_from(chunks.find_first(0x28).unwrap()).unwrap();
        let racks = program_racks(&program, &[], |_, _| panic!("rack decode failed"));
        let (_, insert) = racks
            .iter()
            .find(|(name, _)| name == "instrument insert")
            .unwrap();
        let compressors: Vec<_> = insert.iter().filter(|slot| slot.module == 0x19).collect();
        assert_eq!(compressors.len(), 1);
        let fx = compressors[0];
        eprintln!(
            "saved compressor slot={} bypass={} output_gain={} output_db={}",
            fx.slot,
            fx.bypass,
            fx.output_gain,
            20.0 * f64::from(fx.output_gain).log10()
        );
        assert_eq!(fx.slot, 1);
        assert!(!fx.bypass);
        let v1_gain = 16.0 * (560_434.0f64 / 1_000_000.0).powi(3);
        assert!((f64::from(fx.output_gain) - v1_gain).abs() < 1e-6);
        assert!(
            (20.0 * (f64::from(fx.output_gain) / f64::from(engine_gain(560_434))).log10()).abs()
                < 0.00005
        );
    }

    #[test]
    fn reverb_time_high_cut_and_low_shelf_follow_the_reference_display() {
        let r = |time: f32, cut: f32, shelf: f32| {
            space::galois(
                &[0.5, time, 0.5, 0.5, 0.5, 0.5, 0.5, cut, shelf, 1.0],
                &mut Vec::new(),
            )
            .network
            .unwrap()
        };
        // Default Time 3.2 s displays at x = 0.5; RT60 2.6 s (s.21).
        assert!((r(0.5, 0.0, 0.0).decay_seconds - 2.62).abs() < 0.03);
        assert!((r(1.0, 0.0, 0.0).decay_seconds - 16.16).abs() < 0.5);
        let end = r(0.0, 1.0, 1.0);
        assert!(
            (end.input_cutoff_hz - 2000.0).abs() < 1e-6 && (end.low_shelf_db + 12.0).abs() < 1e-9
        );
        assert_eq!(r(0.0, 0.0, 0.0).input_cutoff_hz, 21_000.0);
    }

    #[test]
    fn gainer_mixes_its_stored_dry_level_with_the_gained_signal() {
        // KONTAKT_REFERENCE s.25: a fresh Gainer at -6 dB reads 0.5 + 0.5 * 0.501.
        let mut g = slot(0x13, 0.501f32.to_le_bytes().to_vec(), 0.5);
        g.dry_level = 0.5;
        let c = chain(&[g], Scope::Bus);
        let [sampler_ir::Processor::Color(config)] = c.processors[..] else {
            panic!("{:?}", c.processors)
        };
        let sampler_ir::fx::color::Gainer { gain, dry } = config.gainer.unwrap();
        assert!((dry + gain.linear() - 0.7505).abs() < 1e-6);
    }

    #[test]
    fn group_compressor_and_pseudo_stereo_are_admitted_per_voice() {
        let mut body = 0i32.to_le_bytes().to_vec();
        for value in [-14f32, 0.5, 0., 100.] {
            body.extend(value.to_le_bytes());
        }
        body.push(1);
        let c = chain(&[slot(0x19, body, 1.)], Scope::Voice);
        assert!(
            c.processors
                .iter()
                .any(|p| matches!(p, sampler_ir::Processor::Dynamics(_))),
            "{:?}",
            c.notes
        );
        let mut body = 0f32.to_le_bytes().to_vec();
        body.extend(0f32.to_le_bytes());
        body.push(1);
        let c = chain(&[slot(0x1f, body, 1.)], Scope::Voice);
        assert!(
            !c.notes
                .iter()
                .any(|(_, feature, _, _)| feature.contains("pseudo stereo")),
            "{:?}",
            c.notes
        );
    }

    #[test]
    fn amplifier_split_uses_physical_slots_and_keeps_both_gain_stages() {
        let mut before = slot(0x13, 2f32.to_le_bytes().to_vec(), 1.0);
        let mut after = slot(0x13, 3f32.to_le_bytes().to_vec(), 1.0);
        before.slot = 1;
        after.slot = 7;
        before.dry_level = 0.0;
        after.dry_level = 0.0;
        let (chain, boundary) = voice_chain(&[before, after], 6, None, 0, &[]);
        assert_eq!(boundary, 1);
        assert_eq!(chain.processors.len(), 2);
        assert_eq!(
            chain.processors[0],
            sampler_ir::Processor::Color(color::gainer(2.0, 0.0))
        );
        assert_eq!(
            chain.processors[1],
            sampler_ir::Processor::Color(color::gainer(3.0, 0.0))
        );
    }

    #[test]
    fn dynamic_slots_run_in_mix_blocks_bypassed_ones_included() {
        let mut gainer = slot(0x13, 2.0f32.to_le_bytes().to_vec(), 1.0);
        gainer.slot = 3;
        gainer.bypass = true;
        let plain = chain_with(
            std::slice::from_ref(&gainer),
            Scope::Bus,
            None,
            None,
            (-1, 1),
            &[],
        );
        assert!(plain.processors.is_empty());
        let c = chain_with(
            std::slice::from_ref(&gainer),
            Scope::Bus,
            None,
            Some((-1, 1)),
            (-1, 1),
            &[],
        );
        assert!(
            matches!(
                c.processors[..],
                [
                    sampler_ir::Processor::Mix {
                        count: 1,
                        bypass: true,
                        address: sampler_ir::SlotAddress {
                            group: -1,
                            slot: 3,
                            generic: 1
                        },
                        ..
                    },
                    sampler_ir::Processor::Color(_)
                ]
            ),
            "{:?}",
            c.processors
        );
    }

    #[test]
    fn modeller_follows_the_measured_spread_and_pan_laws_and_the_inverter_output_applies() {
        let modeller = |spread: f32, pan: f32| {
            let mut bytes = spread.to_le_bytes().to_vec();
            bytes.extend(pan.to_le_bytes());
            bytes.push(0);
            match chain(&[slot(0x1f, bytes, 1.0)], Scope::Voice)
                .processors
                .as_slice()
            {
                [
                    sampler_ir::Processor::Color(config),
                ] => {
                    let stereo = config.stereo.unwrap();
                    assert!(!stereo.pseudo);
                    assert_eq!(stereo.width, (f64::from(spread).clamp(-1.0, 1.0) + 1.0) * 0.5);
                    assert_eq!(stereo.pan, f64::from(pan));
                }
                other => panic!("{other:?}"),
            }
        };
        // The width/pan fields reach the kernel; its measured laws (KONTAKT_REFERENCE.md
        // s.20) are tested in sampler-core's color::stereo.
        modeller(1.0, 0.0);
        modeller(1.5, 0.0);
        modeller(-0.5, 0.0);
        modeller(0.0, -0.5);
        // An Inverter that changes nothing still applies its Output (Una's tone
        // groups store +6 dB against -6 dB on their instrument bus).
        let inverter = chain(&[slot(0x1a, vec![0, 0], 2.0)], Scope::Voice).processors;
        assert_eq!(
            inverter,
            vec![sampler_ir::Processor::StereoMatrix([
                [2.0, 0.0],
                [0.0, 2.0]
            ])]
        );
    }

    #[test]
    fn classic_compressor_slot_is_the_native_classic_law_with_its_link_flag() {
        let mut bytes = Vec::new();
        for x in [0.0f32, -18.0, 0.25, 10.0, 120.0] {
            bytes.extend(x.to_le_bytes());
        }
        bytes.push(1);
        let built = chain(&[slot(0x19, bytes, 1.0)], Scope::Bus);
        let [sampler_ir::Processor::Dynamics(c), ..] = built.processors[..] else {
            panic!("{:?}", built.processors)
        };
        use sampler_ir::fx::dynamics::{Law, Link, Native};
        assert_eq!(c.law, Law::KontaktClassic(Native { threshold_db: -18.0, ratio: 0.25 }));
        assert_eq!(c.link, Link::Mean);
        assert_eq!(c.attack.seconds(), 0.01);
        assert_eq!(c.release.seconds(), 0.12);
        assert!(built.notes.is_empty(), "{:?}", built.notes);
    }

    #[test]
    fn pro_compressor_is_the_native_pro_law_and_always_max_linked() {
        let mut bytes = 2i32.to_le_bytes().to_vec();
        for x in [-18.0f32, 0.25, 10.0, 120.0] {
            bytes.extend(x.to_le_bytes());
        }
        bytes.push(1);
        let built = chain(&[slot(0x19, bytes, 1.0)], Scope::Bus);
        let [sampler_ir::Processor::Dynamics(c), ..] = built.processors[..] else {
            panic!("{:?}", built.processors)
        };
        use sampler_ir::fx::dynamics::{Law, Link};
        assert!(matches!(c.law, Law::KontaktPro(_)));
        assert_eq!(c.link, Link::Max);
        assert!(built.notes.is_empty(), "{:?}", built.notes);
    }

    #[test]
    fn ladder_records_with_flag_bytes_parse_to_their_cutoff() {
        // Conflux (v0x92): kind, flag, kind, then leading, cutoff, resonance.
        // Morphology (v0x95): kind, flag, kind, kind, kind, flag, kind, floats.
        let k = 33i32.to_le_bytes();
        let floats: Vec<u8> = [0.0f32, 0.4, 0.25]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect();
        for layout in [&[&k[..], &[0], &k][..], &[&k, &[0], &k, &k, &k, &[0], &k]] {
            let mut bytes = layout.concat();
            bytes.extend(&floats);
            let built = chain(&[slot(0x18, bytes, 1.0)], Scope::Voice);
            let note = &built.notes[1].2;
            assert_eq!(*note, "33 cutoff 0.4 resonance 0.25", "{:?}", built.notes);
        }
    }

    #[test]
    fn daft_filter_slot_keeps_its_normalized_controls() {
        let mut bytes = Vec::new();
        for _ in 0..2 {
            bytes.extend(71i32.to_le_bytes());
        }
        for x in [0.25f32, 0.5, 0.75] {
            bytes.extend(x.to_le_bytes());
        }
        let built = chain(&[slot(0x18, bytes, 1.0)], Scope::Voice);
        let [sampler_ir::Processor::Daft(d), ..] = built.processors[..] else {
            panic!("{:?} {:?}", built.processors, built.notes)
        };
        assert_eq!(
            (d.gain, d.cutoff, d.resonance, d.highpass),
            (0.25, 0.5, 0.75, true)
        );
    }

    #[test]
    fn inverter_saved_flags_match_native_assignment_and_audio() {
        // BPar+8/+9 -> wrapper+0x1c0/+0x1c1, FUN_1409619b0 case 4.
        for (flags, expected) in [(vec![1, 0], [-0.25, 0.5]), (vec![0, 1], [-0.5, 0.25]), (vec![1, 1], [0.5, -0.25])] {
            let built = chain(&[slot(0x1a, flags, 1.0)], Scope::Voice);
            assert!(built.notes.is_empty(), "{:?}", built.notes);
            let [sampler_ir::Processor::StereoMatrix(matrix)] = built.processors.as_slice() else { panic!("{:?}", built.processors) };
            let actual = matrix.map(|row| row[0] * 0.25 + row[1] * -0.5);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn stateful_inserts_keep_their_boundaries_and_slot_gains() {
        let mut gainer = slot(0x13, 2.0f32.to_le_bytes().to_vec(), 1.0);
        gainer.dry_level = 0.0;
        let inverter = slot(0x1a, vec![1, 1], 0.5);
        let mut modeller = 0.0f32.to_le_bytes().to_vec();
        modeller.extend(0.0f32.to_le_bytes());
        modeller.push(0);
        let modeller = slot(0x1f, modeller, 2.0);
        let sampler_kontakt_chain = chain(&[gainer, inverter, modeller], Scope::Voice);
        let (processors, notes) = (
            sampler_kontakt_chain.processors,
            sampler_kontakt_chain.notes,
        );
        // 2 * (swap, inverted) * 0.5 * 2 = swap, inverted, * 2.
        assert_eq!(
            processors,
            vec![
                sampler_ir::Processor::Color(color::gainer(2.0, 0.0)),
                sampler_ir::Processor::StereoMatrix([[0.0, -0.5], [-0.5, 0.0]]),
                sampler_ir::Processor::Color(color::stereo_modeller(0.0, 0.0, false)),
                sampler_ir::Processor::StereoMatrix([[2.0, 0.0], [0.0, 2.0]]),
            ]
        );
        assert!(notes.is_empty(), "{notes:?}");
        // An EQ: its boosted band, then its slot gain.
        let mut eq = Vec::new();
        for x in [24i32, 24] {
            eq.extend(x.to_le_bytes());
        }
        for x in [100.0f32, 1.0, 0.0, 1000.0, 1.0, 6.0, 5000.0, 2.0, 0.0] {
            eq.extend(x.to_le_bytes());
        }
        let processors = chain(&[slot(0x18, eq, 2.0)], Scope::Voice).processors;
        assert!(
            matches!(
                processors.as_slice(),
                [
                    sampler_ir::Processor::Eq(c),
                    sampler_ir::Processor::StereoMatrix([[2.0, 0.0], [0.0, 2.0]])
                ] if c.count == 1
                    && c.bands()[0].frequency == 1000.0
                    && (f64::from(c.bands()[0].q) - 2. / 3.).abs() < 1e-6
            ),
            "{processors:?}"
        );
        // Unity Gainer retains its stateful parameter owner.
        let mut unity = slot(0x13, 1.0f32.to_le_bytes().to_vec(), 1.0);
        unity.dry_level = 0.0;
        let processors = chain(&[unity], Scope::Voice).processors;
        assert_eq!(
            processors,
            vec![sampler_ir::Processor::Color(color::gainer(1.0, 0.0))]
        );
    }

    #[test]
    fn galois_reverb_room_type_is_a_native_u32_enum() {
        // Kontakt 8.13.1 FUN_140d019f0 reads a u32 room type before nine floats.
        // Hall (1) used to decode as the f32 1.4e-45 and play as a room.
        let mut public = 1u32.to_le_bytes().to_vec();
        for x in [0.25f32, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 1.0] {
            public.extend(x.to_le_bytes());
        }
        let mut hall = slot(0x59, public, 1.0);
        hall.version = 0x10;
        let Some(Params::Reverb(v)) = hall.params() else { panic!("hall reverb decodes") };
        assert_eq!(v[0], 1.0);
        assert_eq!(v[1], 0.25);
        let mut notes = Vec::new();
        let size = space::galois(&v, &mut notes).network.unwrap().size;
        assert_eq!(size, 1.0, "hall keeps the full size factor");
        hall.public[..4].copy_from_slice(&2u32.to_le_bytes());
        assert!(hall.params().is_none(), "Kontakt throws on room type 2");
    }

    #[test]
    fn send_rack_reverb_becomes_a_send_bus_fed_by_send_levels() {
        use sampler_ir as ir;
        let mut levels = 2u32.to_le_bytes().to_vec();
        levels.extend([0.5f32, 1.0].iter().flat_map(|x| x.to_le_bytes()));
        levels.extend(0u32.to_le_bytes());
        let mut reverb = Vec::new();
        for x in [0.0f32, 0.5, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 1.0] {
            reverb.extend(x.to_le_bytes());
        }
        let mut instrument = ir::Instrument {
            groups: vec![ir::Group {
                muted: false,
                wavetable: None,
                time_stretch: None,
                start: Vec::new(),
                name: "g".into(),
                gain: ir::Gain::UNITY,
                pan: ir::Pan::default(),
                tune: ir::Pitch::Semitones(0.0),
                chain: None,
                output: ir::Output::Master,
                voice_limit: None,
                source_voice_limit: None,
                monophonic_release: false,
                sends: Vec::new(),
                tap: None,
            }],
            ..Default::default()
        };
        let racks = vec![
            (
                "instrument insert".to_string(),
                vec![slot(0x17, levels, 1.0)],
            ),
            ("instrument send".to_string(), vec![slot(0x59, reverb, 1.0)]),
        ];
        let report = instrument_buses(&mut instrument, &racks, &[], false, &mut |_| {
            Err("none".into())
        });
        assert_eq!(instrument.buses.len(), 2, "{report:?}");
        assert_eq!(instrument.groups[0].output, ir::Output::Bus(ir::BusRef(0)));
        let feed = &instrument.buses[0].sends[0];
        assert_eq!(feed.to, ir::Output::Bus(ir::BusRef(1)));
        assert_eq!(feed.gain, ir::Gain::Linear(0.5));
        let chain = &instrument.chains[instrument.buses[1].chain.unwrap().0];
        assert!(matches!(
            chain.pre_amplitude[..],
            [ir::Processor::Space(_)]
        ));
        instrument.validate().unwrap();
        // A physical return at slot 3 remains addressable even when the
        // instrument level is zero; a group tap can still feed that return.
        let mut sparse = ir::Instrument::default();
        let mut return_slot = slot(0x59, racks[1].1[0].public.clone(), 1.0);
        return_slot.slot = 3;
        let mut zero = 4u32.to_le_bytes().to_vec();
        zero.extend([0.0f32; 4].iter().flat_map(|x| x.to_le_bytes()));
        zero.extend(0u32.to_le_bytes());
        let (_, returns) = instrument_buses(
            &mut sparse,
            &[
                ("instrument insert".into(), vec![slot(0x17, zero, 1.0)]),
                ("instrument send".into(), vec![return_slot]),
            ],
            &[],
            false,
            &mut |_| Err("none".into()),
        );
        assert_eq!(returns, vec![(3, ir::BusRef(1))]);
        assert_eq!(sparse.buses[0].sends[0].gain, ir::Gain::Linear(0.0));
        // Nothing to do: no buses.
        let mut plain = ir::Instrument::default();
        instrument_buses(&mut plain, &[], &[], false, &mut |_| Err("none".into()));
        assert!(plain.buses.is_empty());
        // A routed bus at half volume owns its groups' output; an unrouted one
        // is not built.
        let mut routed = instrument.clone();
        routed.buses.clear();
        routed.chains.clear();
        let plans = [
            BusPlan {
                index: 0,
                volume: 0.5,
                pan: 0.0,
                groups: vec![ir::GroupRef(0)],
            },
            BusPlan {
                index: 1,
                volume: 0.5,
                pan: 0.0,
                groups: Vec::new(),
            },
        ];
        instrument_buses(&mut routed, &[], &plans, false, &mut |_| Err("none".into()));
        assert_eq!(routed.buses.len(), 1);
        assert_eq!(routed.buses[0].gain, ir::Gain::Linear(0.5));
        assert_eq!(routed.buses[0].output, ir::Output::Master);
        assert_eq!(routed.groups[0].output, ir::Output::Bus(ir::BusRef(0)));
        routed.validate().unwrap();
    }

    #[test]
    fn voice_send_levels_are_admitted_on_both_sides_of_the_amplifier() {
        let mut levels = 1u32.to_le_bytes().to_vec();
        levels.extend(0.5f32.to_le_bytes());
        levels.extend(0u32.to_le_bytes());
        let mut before = slot(0x17, levels.clone(), 1.0);
        before.slot = 2;
        let mut after = slot(0x17, levels, 1.0);
        after.slot = 6;
        let (chain, boundary) = voice_chain(&[before, after], 4, None, 0, &[]);
        assert!(chain.notes.is_empty(), "{:?}", chain.notes);
        assert_eq!(boundary, 0);
        assert_eq!(chain.send_taps.len(), 2);
        assert_eq!(chain.send_taps[0].slot, 2);
        assert_eq!(
            chain.send_taps[0].position,
            sampler_ir::VoiceSendPosition::BeforeAmplitude(0)
        );
        assert_eq!(chain.send_taps[1].slot, 6);
        assert_eq!(
            chain.send_taps[1].position,
            sampler_ir::VoiceSendPosition::AfterAmplitude(0)
        );
    }

    #[test]
    fn instrument_send_tap_precedes_later_inserts_on_the_summed_bus() {
        use sampler_ir as ir;
        let mut before = slot(0x13, 2.0f32.to_le_bytes().to_vec(), 1.0);
        before.dry_level = 0.0;
        let mut levels = 1u32.to_le_bytes().to_vec();
        levels.extend(0.5f32.to_le_bytes());
        levels.extend(0u32.to_le_bytes());
        let mut tap = slot(0x17, levels, 1.0);
        tap.slot = 2;
        let mut after = slot(0x13, 3.0f32.to_le_bytes().to_vec(), 1.0);
        after.slot = 3;
        after.dry_level = 0.0;
        let mut ir = ir::Instrument::default();
        instrument_buses(
            &mut ir,
            &[
                ("instrument insert".into(), vec![before, tap, after]),
                (
                    "instrument send".into(),
                    vec![slot(0x13, 0.5f32.to_le_bytes().to_vec(), 1.0)],
                ),
            ],
            &[],
            false,
            &mut |_| Err("none".into()),
        );
        assert_eq!(ir.buses.len(), 3);
        assert_eq!(ir.buses[0].output, ir::Output::Bus(ir::BusRef(2)));
        assert_eq!(ir.buses[0].sends[0].to, ir::Output::Bus(ir::BusRef(1)));
        assert_eq!(ir.buses[0].sends[0].gain, ir::Gain::Linear(0.5));
        for (bus, gain) in [(0, 2.0), (2, 3.0)] {
            let stages = &ir.chains[ir.buses[bus].chain.unwrap().0].pre_amplitude;
            assert!(
                matches!(stages[..], [ir::Processor::Color(c)] if c.gainer.is_some_and(|x| x.gain == ir::Gain::Linear(gain) && x.dry == 0.0))
            );
        }
        ir.validate().unwrap();
    }
}

/// Rebuild an impulse with the same shaping path; cache metadata carries no audio.
pub(crate) fn restore_impulse(
    c: &Convolution,
    decoded: Decoded,
) -> Result<sampler_ir::Impulse, String> {
    let mut store = Vec::new();
    let mut decoded = Some(decoded);
    let mut load = |_| {
        decoded
            .take()
            .ok_or_else(|| "impulse already consumed".to_owned())
    };
    space::convolution(
        c,
        &mut Impulses {
            store: &mut store,
            load: &mut load,
            recipes: None,
        },
        &mut Vec::new(),
    )?;
    store.pop().ok_or_else(|| "missing shaped impulse".into())
}


// Universal effect families (wave 4): each maps native effects onto sampler_ir::fx.
pub(crate) mod dynamics;
pub(crate) mod filter;
pub(crate) mod eq;
pub(crate) mod timemod;
pub(crate) mod space;
pub(crate) mod color;
