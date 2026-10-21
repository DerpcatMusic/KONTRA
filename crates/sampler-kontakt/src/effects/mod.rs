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

#[cfg(test)]
mod record_audit;


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
    (0x6b, "Twin Delay"),
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
            parameter if color::skreamer_parameter(parameter).is_some() => {
                fx.apply_skreamer_parameter(parameter, w.value)
            }
            "ENGINE_PAR_GAIN1" if group >= 0 && generic == -1 => fx.apply_eq_gain(0, w.value),
            "ENGINE_PAR_GAIN2" if group >= 0 && generic == -1 => fx.apply_eq_gain(1, w.value),
            "ENGINE_PAR_GAIN3" if group >= 0 && generic == -1 => fx.apply_eq_gain(2, w.value),
            // Solid G-EQ (0x44) knobs and switches: the saved record the reader parses.
            parameter if fx.module == 0x44 => {
                let _ = eq::solid_store(fx.version, &mut fx.public, parameter, w.value);
            }
            _ => {}
        }
    }
}

/// A module's stored parameters, where the layout is known (byte-exact on
/// every local instance in v1's survey).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Params {
    /// A native dynamics law (Limiter 0x1c, Transparent Limiter 0x5f), mapped
    /// at parse time.
    Dynamics(Result<sampler_ir::Processor, dynamics::Note>),
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
    /// `BParFXRaum` Grounded or Airy: the core controls (`sampler_ir`
    /// `Network::raum`) and whether the predelay is tempo-synced.
    Raum { core: [f32; 20], synced: bool },
    /// `BParFXIRC`: the impulse response is an index into the preset's other-files table.
    Convolution(Box<Convolution>),
    /// Decoded storage fields whose DSP/physical parameter laws are not yet modeled.
    Fields(Vec<ni_file::kontakt::objects::EffectField>),
}

pub(crate) use space::Convolution;

impl Slot {
    pub(crate) fn params(&self) -> Option<Params> {
        let mut r = Reader(&self.public);
        let params = match self.module {
            0x10..=0x12 | 0x14 | 0x5d => Params::TimeMod(timemod::read(self.module, self.version, &mut r)?),
            0x63 => Params::TimeMod(timemod::read_psyche(self.version, &mut r)?),
            0x6b => Params::TimeMod(timemod::read_twin(self.version, &mut r)?),
            // Cosmic (type 2) keeps its decoded fields: its core is not built.
            0x65 if self.version == 0x10 && matches!(self.public.get(..4), Some([0 | 1, 0, 0, 0])) => {
                let (core, synced) = space::read_raum(&mut r)?;
                Params::Raum { core, synced }
            }
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
            0x1c => Params::Dynamics(dynamics::limiter(r.f32()?, r.f32()?)),
            0x4c if self.version == 0x10 => Params::Dynamics(dynamics::feedback(
                [r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?],
                [r.flag()?, r.flag()?, r.flag()?],
            )),
            0x43 if self.version == 0x10 => Params::Dynamics(dynamics::transient(r.f32()?, r.f32()?, r.f32()?, r.i32()?)),
            0x46 if (0x10..=0x12).contains(&self.version) => Params::Dynamics(dynamics::bus_comp(
                [r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?, r.f32()?],
                [r.flag()?, if self.version >= 0x11 { r.flag()? } else { false }],
                if self.version == 0x12 { Some(r.f32()?) } else { None },
            )),
            0x5f => Params::Dynamics(dynamics::hilbert(r.f32()?, r.f32()?, r.f32()?, r.flag()?)),
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
                } else if matches!(kind, 120 | 130 | 140..=143) {
                    // KONTAKTFX layouts: Simple LP/HP kind, u8, kind x4, two knobs; Dual SKF
                    // kind x3, five knobs, kind x2; Monark kind x2, i32 variant, five knobs,
                    // kind x3. Knobs go out in record order, Monark's variant last.
                    let kinds = |r: &mut Reader, n: usize| (0..n).all(|_| r.i32() == Some(kind));
                    let (knobs, variant) = match kind {
                        120 => {
                            r.flag()?;
                            kinds(&mut r, 4).then_some(())?;
                            (2, None)
                        }
                        130 => {
                            kinds(&mut r, 2).then_some(())?;
                            (5, None)
                        }
                        _ => {
                            kinds(&mut r, 1).then_some(())?;
                            (5, Some(r.i32()? as f32))
                        }
                    };
                    let values: Vec<f32> = (0..knobs).map(|_| r.f32()).collect::<Option<_>>()?;
                    let mut extra = values[2..].to_vec();
                    extra.extend(variant);
                    return Some(Params::Filter { kind, cutoff: values[0], resonance: values[1], extra });
                } else if (22..=24).contains(&kind) && self.version >= 0x93 {
                    // Later records repeat the EQ kind (v0x95: five times) before the bands.
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
                    // Daft (70, 71) stores a leading value first; SV Par/Ser (63..=65) its
                    // bandwidth (reader 0x140d00db0 into +0x24, before cutoff and resonance).
                    let leading = if matches!(kind, 70 | 71 | 30..=41 | 63..=65) {
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

/// A rack as serial processors plus the levels its Send Levels slots feed
/// into the instrument's send slots, and what it leaves out.
#[derive(Default)]
pub(crate) struct Chain<P = usize> {
    pub processors: Vec<sampler_ir::Processor>,
    pub sends: Vec<f32>,
    pub notes: Vec<Note>,
    /// Each translated filter/EQ band slot and its index in `processors`, for
    /// modulation targets that name a module slot.
    pub filter_slots: Vec<(usize, usize)>,
    /// Each script-addressed effect module (distortion, saturation, formant,
    /// lo-fi) slot and its index in `processors`: modulation targets name them.
    pub lane_slots: Vec<(usize, usize)>,
    pub send_taps: Vec<SendTap<P>>,
}

pub(crate) struct SendTap<P = sampler_ir::VoiceSendPosition> {
    pub slot: usize,
    pub position: P,
    pub levels: Vec<f32>,
    pub bypass: bool,
}

impl<P> Chain<P> {
    /// Place rack-relative indices on one chosen side of the amplifier once.
    fn map_send_positions<Q>(self, mut position: impl FnMut(P) -> Q) -> Chain<Q> {
        Chain {
            processors: self.processors,
            sends: self.sends,
            notes: self.notes,
            filter_slots: self.filter_slots,
            lane_slots: self.lane_slots,
            send_taps: self
                .send_taps
                .into_iter()
                .map(|tap| SendTap {
                    slot: tap.slot,
                    position: position(tap.position),
                    levels: tap.levels,
                    bypass: tap.bypass,
                })
                .collect(),
        }
    }
}

/// Keep the amplifier boundary at its physical slot, even when either side
/// has holes or linear processors that can otherwise collapse together.
pub(crate) fn voice_chain(
    slots: &[Slot],
    split: i32,
    dynamic: Option<(i32, i32)>,
    group: i32,
    live_eq: &[usize],
) -> (Chain<sampler_ir::VoiceSendPosition>, usize) {
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
    )
    .map_send_positions(sampler_ir::VoiceSendPosition::BeforeAmplitude);
    let after = chain_with(
        &slots[cut..],
        Scope::Voice,
        None,
        dynamic,
        (group, -1),
        live_eq,
    )
    .map_send_positions(sampler_ir::VoiceSendPosition::AfterAmplitude);
    let boundary = before.processors.len();
    before.processors.extend(after.processors);
    before.send_taps.extend(after.send_taps);
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

/// A file-table reference belongs to the exact physical effect that authored it.
/// Equal indices in base and overlaid snapshot records are separate identities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ImpulseRequest {
    pub index: i32,
    pub address: sampler_ir::SlotAddress,
}

/// Where convolution slots find their impulse responses: `load` decodes the
/// response an other-files index names, `store` collects the shaped ones.
pub(crate) struct Impulses<'a> {
    pub store: &'a mut Vec<sampler_ir::Impulse>,
    pub recipes: Option<&'a mut Vec<(ImpulseRequest, Convolution)>>,
    pub load: &'a mut dyn FnMut(ImpulseRequest) -> Result<Decoded, String>,
}

/// A rack's translation state: the chain built so far, the stereo matrix still
/// waiting to be applied, and the filters that run before that matrix.
pub(crate) struct Lowering {
    pub out: Chain,
    pub combined: Matrix,
    pub filters: Vec<sampler_ir::Processor>,
}

impl Lowering {
    fn new() -> Self {
        Self { out: Chain::default(), combined: IDENTITY, filters: Vec::new() }
    }

    /// Emit the pending filters, then the pending matrix when it is not the identity.
    fn flush(&mut self) {
        self.out.processors.append(&mut self.filters);
        if self.combined != IDENTITY {
            self.out.processors.push(sampler_ir::Processor::StereoMatrix(self.combined));
            self.combined = IDENTITY;
        }
    }
}

/// One slot as the family translators see it.
pub(crate) struct Site<'a> {
    pub fx: &'a Slot,
    pub scope: Scope,
    /// The rack's `(group, generic)` address when a script may write the slot at runtime.
    pub dynamic: Option<(i32, i32)>,
    /// The rack's physical address, which bound filters and convolutions name.
    pub physical: (i32, i32),
    pub live_eq: &'a [usize],
    /// Inside a Mix block: the slot's live lanes own its level, so no gain is applied here.
    pub mix: bool,
    /// The slot's output gain as a matrix (identity inside a Mix block) and as a scalar.
    pub gain: Matrix,
    pub wet: f64,
    /// An EQ has no Output control: its gain is identity unless a script wrote it.
    pub eq_gain: Matrix,
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
    let mut b = Lowering::new();
    for fx in slots.iter().filter(|fx| dynamic.is_some() || !fx.bypass) {
        let name = module_name(fx.module);
        let mut notes = Vec::new();
        let params = fx.params();
        // Send Levels feed buses, not the signal; everything else a script
        // may bypass or trim at runtime runs inside its own Mix block.
        let mix = dynamic.filter(|_| !matches!(params, Some(Params::SendLevels { .. })));
        let begin = if mix.is_some() {
            b.flush();
            b.out.processors.len()
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
        let site = Site {
            fx,
            scope,
            dynamic,
            physical,
            live_eq,
            mix: mix.is_some(),
            gain,
            wet,
            eq_gain,
        };
        let mut modelled = true;
        // The routing table: each family translates its own slots, in this order.
        match &params {
            // Solid G-EQ (0x44): `params()` only yields its raw fields; eq.rs owns the mapping.
            _ if fx.module == 0x44 => modelled = eq::slot_solid_geq(&mut b, &site, &mut notes),
            Some(Params::Distortion { mode, drive, damping }) => {
                modelled = color::slot_distortion(&mut b, &site, &mut notes, *mode, *drive, *damping)
            }
            // 0x1d is also the Surround Panner (version 0x70): not a shaper.
            Some(Params::Saturation { .. }) if fx.version == 0x70 => modelled = false,
            Some(Params::Saturation { shape, mode }) => {
                modelled = color::slot_saturation(&mut b, &site, &mut notes, *shape, *mode)
            }
            Some(Params::TimeMod(unit)) => modelled = timemod::slot_timemod(&mut b, &site, &mut notes, unit),
            Some(Params::Filter { kind: 13, cutoff, resonance, .. }) => {
                modelled = timemod::slot_phaser(&mut b, &site, &mut notes, *cutoff, *resonance)
            }
            Some(Params::Filter { kind: kind @ (90 | 91), cutoff, resonance, extra }) => {
                modelled = filter::slot_formant(&mut b, &site, &mut notes, *kind, *cutoff, *resonance, extra)
            }
            Some(Params::Fields(fields)) if fx.module == 0x21 => {
                modelled = color::slot_skreamer(&mut b, &site, fields)
            }
            Some(Params::Fields(fields)) if color::TYPED_CORES.contains(&fx.module) => {
                modelled = color::slot_typed(&mut b, &site, &mut notes, fields)
            }
            Some(Params::LoFi { values, flag }) => {
                modelled = color::slot_lofi(&mut b, &site, &mut notes, values, *flag)
            }
            Some(Params::Eq { bands }) => modelled = eq::slot_legacy(&mut b, &site, &mut notes, bands),
            Some(Params::SendLevels { sends, .. }) => {
                modelled = bus::slot_send_levels(&mut b, &site, &mut notes, sends)
            }
            Some(Params::Filter { kind: kind @ 30..=41, cutoff, resonance, extra })
                if !extra.is_empty() && matches!(fx.version, 0x90..=0x92 | 0x95) =>
            {
                modelled = filter::slot_ladder(&mut b, &site, &mut notes, *kind, *cutoff, *resonance, extra)
            }
            // Adaptive-resonance family: saved 100..=108 are modes 0..=8 of the native FilterDJ.
            Some(Params::Filter { kind: kind @ 100..=108, cutoff, resonance, .. })
                if matches!(fx.version, 0x90..=0x92)
                    && [*cutoff, *resonance].iter().all(|v| (0.0..=1.0).contains(v)) =>
            {
                modelled = filter::slot_ar(&mut b, &site, &mut notes, *kind, *cutoff, *resonance)
            }
            Some(Params::Filter { kind, cutoff, resonance, extra })
                if matches!(kind, 70 | 71) && !extra.is_empty() =>
            {
                modelled = filter::slot_daft(&mut b, &site, &mut notes, *kind, *cutoff, *resonance, extra)
            }
            Some(Params::Filter { kind, cutoff, resonance, extra }) => {
                modelled = filter::slot_filter(&mut b, &site, &mut notes, *kind, *cutoff, *resonance, extra)
            }
            Some(Params::Dynamics(result)) => modelled = dynamics::slot_dynamics(&mut b, &site, &mut notes, result),
            Some(Params::Compressor { mode, threshold_db, ratio, attack_ms, release_ms, link }) => {
                modelled = dynamics::slot_compressor(
                    &mut b,
                    &site,
                    &mut notes,
                    *mode,
                    *threshold_db,
                    *ratio,
                    *attack_ms,
                    *release_ms,
                    *link,
                )
            }
            Some(Params::Reverb(v)) if scope == Scope::Bus => {
                modelled = space::slot_galois(&mut b, &site, &mut notes, v)
            }
            Some(Params::LegacyReverb(v)) if scope == Scope::Bus => {
                modelled = space::slot_legacy_reverb(&mut b, &site, &mut notes, v)
            }
            Some(Params::Plate(v)) if scope == Scope::Bus => {
                modelled = space::slot_plate(&mut b, &site, &mut notes, v)
            }
            Some(Params::Raum { core, synced }) if scope == Scope::Bus => {
                modelled = space::slot_raum(&mut b, &site, &mut notes, core, *synced)
            }
            Some(Params::Convolution(c)) if scope == Scope::Bus && impulses.is_some() => {
                modelled = space::slot_convolution(
                    &mut b,
                    &site,
                    &mut notes,
                    c,
                    impulses.as_deref_mut().expect("checked above"),
                )
            }
            // Addressed, so a script's GN_GAIN reaches it (Conflux's Volume knob).
            Some(Params::Gainer { gain: parameter }) => color::slot_gainer(&mut b, &site, *parameter),
            Some(Params::StereoModeller { spread, pan, pseudo }) => {
                color::slot_stereo(&mut b, &site, *spread, *pan, *pseudo)
            }
            Some(p) => match matrix(p, &mut notes) {
                Some(m) => b.combined = product(product(gain, m), b.combined),
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
            b.flush();
            // Every family unit of a script-addressed slot takes the slot's live lanes.
            let address = sampler_ir::SlotAddress { group, slot: fx.slot as i32, generic };
            for p in &mut b.out.processors[begin..] {
                if scope != Scope::Voice || !matches!(p, sampler_ir::Processor::Eq(_)) {
                    *p = p.at(address);
                }
            }
            // A modelled module that changes only its level still owns the slot's
            // Output and Bypass.
            if modelled && b.out.processors.len() == begin {
                b.out.processors.push(sampler_ir::Processor::StereoMatrix(IDENTITY));
            }
            let count = b.out.processors.len() - begin;
            if count > 0 {
                let convolution = matches!(params, Some(Params::Convolution(_)));
                let wet = if matches!(params, Some(Params::Eq { .. })) && eq_unset {
                    1.0
                } else {
                    wet
                };
                b.out.processors.insert(
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
                for (_, at) in b.out.filter_slots.iter_mut().chain(&mut b.out.lane_slots) {
                    if *at >= begin {
                        *at += 1;
                    }
                }
            }
        }
        if !modelled {
            b.out.notes.push((
                fx.slot,
                "effect".into(),
                match color::unproved(fx.module).or(timemod::unproved(fx.module)) {
                    Some(why) => format!("{name} v{:#x} len {}: {why}{}", fx.version, fx.public.len(), space::why_unbound(fx.module)),
                    None => format!("{name} v{:#x} len {}{}", fx.version, fx.public.len(), space::why_unbound(fx.module)),
                },
                sampler_ir::Reason::NotModeled,
            ));
        }
        b.out.notes.extend(
            notes
                .into_iter()
                .map(|(f, v, r)| (fx.slot, format!("{name}: {f}"), v, r)),
        );
    }
    b.flush();
    b.out
}
pub(crate) use bus::{BusPlan, bus_plans, instrument_buses, instrument_buses_with_recipes};
pub(crate) use space::restore_impulse;

// Universal effect families (wave 4): each maps native effects onto sampler_ir::fx.
pub(crate) mod dynamics;
pub(crate) mod filter;
pub(crate) mod eq;
pub(crate) mod timemod;
pub(crate) mod space;
pub(crate) mod color;

#[cfg(test)]
mod test_support;

mod bus;

#[cfg(test)]
mod tests {
    use crate::effects::*;
    use crate::effects::test_support::*;
    use ni_file::kontakt::Chunk;

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
                if module == 0x42 {
                    // Tape is modelled: any nonzero quality byte turns Quality on.
                    let tape = sampler_ir::fx::color::Tape { gain: 0.5, warmth: 0.25, rolloff: 0.75, quality: true };
                    // The unit carries its live slot address; the knobs are the decoded tape.
                    assert!(out.processors.iter().any(|p| matches!(p, sampler_ir::Processor::Color(c)
                        if *c == sampler_ir::fx::color::Config { address: c.address, ..sampler_ir::fx::color::Config::tape(tape) })));
                    continue;
                }
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
                residency: Default::default(),
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
                    && c.bands()[0].law == sampler_ir::fx::eq::Law::KontaktLegacy
                    && c.bands()[0].frequency == sampler_core::KontaktLegacyEqSettings::frequency_knob(1000.0)
                    && c.bands()[0].q == sampler_core::KontaktLegacyEqSettings::bandwidth_knob(1.0)
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
}
