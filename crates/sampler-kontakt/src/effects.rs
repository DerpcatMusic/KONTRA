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

mod formant;

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
    (0x1d, "Surround Panner"),
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
    Ok(out)
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
            _ => {}
        }
    }
}

/// A module's stored parameters, where the layout is known (byte-exact on
/// every local instance in v1's survey).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Params {
    /// Linear gain.
    Gainer { gain: f32 },
    LoFi { values: [f32; 4], flag: bool },
    /// `$ENGINE_PAR_STEREO` (offset from 100% width), `$ENGINE_PAR_STEREO_PAN`,
    /// `$ENGINE_PAR_STEREO_PSEUDO`.
    StereoModeller { spread: f32, pan: f32, pseudo: bool },
    /// `$ENGINE_PAR_PHASE_INVERT`, `$ENGINE_PAR_LR_SWAP`.
    Inverter { invert: bool, swap: bool },
    /// `BParFXCompressor`: the first stored value (mode, Classic/Enhanced/Pro),
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
    SendLevels { sends: Vec<f32>, outputs: Vec<f32> },
    /// Kontakt filter type (stored twice), normalized cutoff/resonance and
    /// up to three further values.
    Filter {
        kind: i32,
        cutoff: f32,
        resonance: f32,
        extra: Vec<f32>,
    },
    /// 1-3 band EQ (filter types 22..=24): Hz, octaves, dB.
    Eq { bands: Vec<[f32; 3]> },
    /// `BParFXGaloisReverb`, the modern Reverb: normalized room type, time,
    /// size, damping, modulation, diffusion, predelay, high cut, low shelf,
    /// stereo (`$ENGINE_PAR_RV2_*` order).
    Reverb([f32; 10]),
    /// `BParFXIRC`: the impulse response is an index into the preset's other-files table.
    Convolution(Box<Convolution>),
    /// Decoded storage fields whose DSP/physical parameter laws are not yet modeled.
    Fields(Vec<ni_file::kontakt::objects::EffectField>),
}

#[derive(serde::Serialize, serde::Deserialize)]
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Convolution {
    /// Sample-rate decimation factor (1 or negative: none).
    pub decimation: f32,
    pub predelay_ms: f32,
    /// Early and late parts: length ratio, low cut Hz, high cut Hz.
    pub early: [f32; 3],
    pub late: [f32; 3],
    /// Early/late crossover as a fraction of the response.
    pub xpoint: f32,
    /// Reverse, auto gain, preserve length, bypass latency compensation,
    /// volume envelope.
    pub flags: [bool; 5],
    pub curve_x: Vec<f32>,
    pub curve_db: Vec<f32>,
    pub ir_index: i32,
}

impl Slot {
    pub(crate) fn params(&self) -> Option<Params> {
        let mut r = Reader(&self.public);
        let params = match self.module {
            0x13 => Params::Gainer { gain: r.f32()? },
            0x20 => {
                let first = [r.f32()?, r.f32()?, r.f32()?];
                let flag = r.flag()?;
                Params::LoFi { values: [first[0], first[1], first[2], r.f32()?], flag }
            },
            0x1f => Params::StereoModeller {
                spread: r.f32()?,
                pan: r.f32()?,
                pseudo: r.flag()?,
            },
            0x59 => {
                let mut v = [0.0; 10];
                for x in &mut v {
                    *x = r.f32()?;
                }
                Params::Reverb(v)
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
                mode: r.f32()?,
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
                        let record = ni_file::kontakt::objects::BParFXFilterRecord::read(self.version, &self.public).ok()?;
                        return Some(Params::Filter { kind: record.filter_type, cutoff: record.cutoff,
                            resonance: record.resonance, extra: vec![record.leading_value] });
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

fn product(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|i| std::array::from_fn(|j| a[i][0] * b[0][j] + a[i][1] * b[1][j]))
}

/// The stereo matrix of a linear module, and what it leaves out.
fn matrix(params: &Params, notes: &mut Notes) -> Option<Matrix> {
    use sampler_ir::Reason::{NotModeled, UnknownLaw};
    Some(match *params {
        Params::Gainer { gain } => [[f64::from(gain), 0.0], [0.0, f64::from(gain)]],
        Params::Inverter { invert, swap } => {
            if invert || swap {
                // Field order follows the KSP parameter list; not verified
                // against a rendering.
                notes.push((
                    "inverter flag order".into(),
                    format!("invert {invert} swap {swap}"),
                    UnknownLaw,
                ));
            }
            let sign = if invert { -1.0 } else { 1.0 };
            if swap {
                [[0.0, sign], [sign, 0.0]]
            } else {
                [[sign, 0.0], [0.0, sign]]
            }
        }
        Params::StereoModeller {
            spread,
            pan,
            pseudo,
        } => {
            if pseudo {
                notes.push((
                    "stereo modeller pseudo stereo".into(),
                    "on".into(),
                    NotModeled,
                ));
            }
            // KONTAKT_REFERENCE.md s.20 (measured 2x2 fits, pure matrix): spread
            // s in -1..=1 (GUI percent / 100). s < 0: M/S width 1 + s, mono at
            // -1. s > 0: [[1+s, -s], [-s, 1+s]], clamped at 1. Pan p is a
            // linear balance: the opposite channel is scaled by 1 - |p|.
            let s = f64::from(spread).clamp(-1.0, 1.0);
            let pan = f64::from(pan);
            let gains = [(1.0 - pan).clamp(0.0, 1.0), (1.0 + pan).clamp(0.0, 1.0)];
            let (same, other) = if s < 0.0 {
                ((2.0 + s) / 2.0, -s / 2.0)
            } else {
                (1.0 + s, -s)
            };
            [
                [same * gains[0], other * gains[0]],
                [other * gains[1], same * gains[1]],
            ]
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

/// How one Kontakt filter type (the `kind` stored in a Filter slot) maps to
/// an IR filter: the response, and the laws taking its normalized cutoff and
/// resonance (0..=1) to Hz and Q.
struct FilterType {
    kind: sampler_ir::FilterKind,
    hertz: fn(f32) -> f64,
    q: fn(f32) -> f64,
}

/// Kontakt's SV filters: the knob is 25 Hz * 800^x, and resonance r gives
/// k = 1/Q = (2 - 0.013) * (1 - r)^3.1 + 0.013 (measured on SV LP2 against
/// Kontakt 8). The IR has no passband-gain field, so the measured loss at
/// high resonance (-6 dB at r = 1) is not reproduced.
fn sv_hertz(x: f32) -> f64 {
    25.0 * 800f64.powf(f64::from(x))
}

fn sv_q(r: f32) -> f64 {
    1.0 / ((2.0 - 0.013) * (1.0 - f64::from(r)).powf(3.1) + 0.013)
}

/// The types with a map to a Kontakt 8 filter name (read from its GUI):
/// 52 SV LP2, 54 SV HP2, 55 SV LP4, 57 SV HP4. Only SV LP2's laws were
/// measured; the other three are taken to share them (same family, same
/// knobs). Other types are reported by number, not guessed: 3 is "Legacy
/// HP1" whose stored cutoff is 0 and which a modulator or script drives, 90 is
/// Formant I (not a low pass), and 106 "AR LP2/4" has a cutoff law that
/// differs from SV (stored 0.5135 reads 603 Hz, SV would give 774 Hz).
const FILTER_TYPES: &[(i32, FilterType)] = &[
    (
        52,
        FilterType {
            kind: sampler_ir::FilterKind::LowPass { poles: 2 },
            hertz: sv_hertz,
            q: sv_q,
        },
    ),
    (
        54,
        FilterType {
            kind: sampler_ir::FilterKind::HighPass { poles: 2 },
            hertz: sv_hertz,
            q: sv_q,
        },
    ),
    (
        55,
        FilterType {
            kind: sampler_ir::FilterKind::LowPass { poles: 4 },
            hertz: sv_hertz,
            q: sv_q,
        },
    ),
    (
        57,
        FilterType {
            kind: sampler_ir::FilterKind::HighPass { poles: 4 },
            hertz: sv_hertz,
            q: sv_q,
        },
    ),
];

fn filter_type(kind: i32) -> Option<&'static FilterType> {
    FILTER_TYPES
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, t)| t)
}

/// A Filter slot as an IR filter, or `None` for a type without a table entry.
fn filter(kind: i32, cutoff: f32, resonance: f32) -> Option<sampler_ir::Processor> {
    let t = filter_type(kind)?;
    Some(sampler_ir::Processor::Filter(sampler_ir::Filter {
        kind: t.kind,
        cutoff: sampler_ir::Frequency::Hertz((t.hertz)(cutoff)),
        resonance: sampler_ir::Resonance::Q((t.q)(resonance)),
    }))
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
    let mut before = chain_with(&slots[..cut], Scope::Voice, None, dynamic, (group, -1), live_eq);
    let after = chain_with(&slots[cut..], Scope::Voice, None, dynamic, (group, -1), live_eq);
    let boundary = before.processors.len();
    before.processors.extend(after.processors);
    before.send_taps.extend(after.send_taps.into_iter().map(|mut tap| {
        let sampler_ir::VoiceSendPosition::BeforeAmplitude(n) = tap.position else { unreachable!() };
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
            Some(Params::Filter { kind: 90, cutoff, resonance, extra }) => {
                flush(&mut combined, &mut filters, &mut out);
                match extra.first().and_then(|&size| formant::sections([*cutoff, *resonance, size])) {
                    Some(sections) => {
                        out.processors.extend(sections);
                        combined = product([[0.25, 0.], [0., 0.25]], product(gain, combined));
                        notes.push(("Formant I vowel model".into(), "v1 three-band proxy; native coefficients unverified".into(), sampler_ir::Reason::UnknownLaw));
                    }
                    None => {
                        notes.push(("Formant I parameters".into(), "missing Size or outside normalized range".into(), sampler_ir::Reason::InvalidValue));
                        modelled = false;
                    }
                }
            }
            Some(Params::LoFi { values, flag }) => {
                flush(&mut combined, &mut filters, &mut out);
                if values.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)) {
                    out.processors.push(sampler_ir::Processor::LoFi {
                        bits: values[0], frequency: values[1], noise: values[2], color: values[3],
                    });
                    combined = product(gain, combined);
                    if *flag { notes.push(("Lo-Fi fourth field flag".into(), "true".into(), sampler_ir::Reason::UnknownLaw)); }
                } else {
                    notes.push(("Lo-Fi parameters".into(), "non-finite or outside normalized range".into(), sampler_ir::Reason::InvalidValue));
                    modelled = false;
                }
            }
            Some(Params::Eq { bands }) => {
                let live = scope == Scope::Voice && (dynamic.is_some() || live_eq.contains(&fx.slot));
                if live {
                    flush(&mut combined, &mut filters, &mut out);
                    let live_bands: Option<Vec<_>> = bands.iter()
                        .map(|band| eq_band(*band, true, &mut notes)).collect();
                    if let Some(live_bands) = live_bands {
                        for p in live_bands {
                            out.filter_slots.push((fx.slot, out.processors.len()));
                            out.processors.push(p);
                        }
                    } else { modelled = false; }
                } else {
                    filters.extend(bands.iter().filter_map(|band| eq_band(*band, false, &mut notes)));
                }
                combined = product(eq_gain, combined);
            }
            Some(Params::SendLevels { sends, .. }) => {
                flush(&mut combined, &mut filters, &mut out);
                let sends: Vec<_> = sends.iter().map(|&level| {
                    if level.is_finite() && level >= 0.0 { level } else {
                        notes.push(("send level".into(), "non-finite or negative".into(), sampler_ir::Reason::InvalidValue));
                        0.0
                    }
                }).collect();
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
            Some(Params::Filter { kind: 33, cutoff, resonance, extra }) if !extra.is_empty() && matches!(fx.version, 0x90..=0x92) => {
                // Port from v1 0cb7a8a0:src/engine/filter.rs::proto.
                // Gain is signed normalized (12 dB per unit); the native kernel
                // retains the record-version cutoff clock.
                let values = [extra[0], *cutoff, *resonance];
                if values.iter().all(|v| v.is_finite()) && (-1.0..=1.0).contains(&values[0])
                    && values[1..].iter().all(|v| (0.0..=1.0).contains(v)) {
                    out.filter_slots.push((fx.slot, out.processors.len() + filters.len()));
                    filters.push(sampler_ir::Processor::LadderLP4(sampler_ir::LadderLP4 {
                        address: Some(sampler_ir::SlotAddress { group: physical.0, slot: fx.slot as i32, generic: physical.1 }),
                        gain: f64::from(values[0]), cutoff: f64::from(values[1]), resonance: f64::from(values[2]),
                        record_version: fx.version,
                    }));
                    combined = product(gain, combined);
                } else {
                    notes.push(("Ladder LP4 parameters".into(), "non-finite or outside native range".into(), sampler_ir::Reason::InvalidValue));
                    modelled = false;
                }
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
                out.filter_slots.push((fx.slot, out.processors.len() + filters.len()));
                filters.push(sampler_ir::Processor::Daft(sampler_ir::Daft {
                    gain: f64::from(extra[0]).clamp(0.0, 1.0),
                    cutoff: f64::from(*cutoff).clamp(0.0, 1.0),
                    resonance: f64::from(*resonance).clamp(0.0, 1.0),
                    highpass: *kind == 71,
                }));
                combined = product(gain, combined);
            }
            Some(Params::Filter {
                kind,
                cutoff,
                resonance,
                ..
            }) => match filter(*kind, *cutoff, *resonance) {
                Some(f) => {
                    out.filter_slots
                        .push((fx.slot, out.processors.len() + filters.len()));
                    filters.push(f);
                    combined = product(gain, combined);
                }
                None => {
                    notes.push((
                        "filter type".into(),
                        format!("{kind} cutoff {cutoff} resonance {resonance}"),
                        sampler_ir::Reason::NotModeled,
                    ));
                    modelled = false;
                }
            },
            Some(Params::Compressor {
                mode,
                threshold_db,
                ratio,
                attack_ms,
                release_ms,
                link,
            }) => {
                // Group inserts are per voice; instrument/bus inserts see their
                // corresponding sum. The chain's scope preserves that distinction.
                // DSP_SYSTEM_INVENTORY "Subtype selection and compressor linking":
                // the linked detector is the signed channel mean. The level law
                // is the textbook one (ir::Compressor); the stored units are the
                // importer's labels.
                // KONTAKT_REFERENCE (ANALOG STRINGS C4/E4/G4, compressor on vs
                // bypassed): Kontakt +8.7 dB RMS, KONTRA +8.2..8.4 dB, i.e. the +9 dB
                // output gain with about 0.3-0.7 dB of reduction. Only mode 0
                // (Classic) is taken to share the kernel.
                if *mode != 0.0 {
                    notes.push((
                        "compressor mode".into(),
                        format!("{mode}"),
                        sampler_ir::Reason::UnknownLaw,
                    ));
                }
                flush(&mut combined, &mut filters, &mut out);
                out.processors
                    .push(sampler_ir::Processor::Compressor(sampler_ir::Compressor {
                        threshold_db: f64::from(*threshold_db),
                        // Stored as the inverse ratio (Analog Strings: 0.501 beside a
                        // -14.2 dB threshold reads 2:1). ponytail: unverified; a stored
                        // slope 1 - 1/ratio would read the same here.
                        ratio: (1.0 / f64::from(*ratio).clamp(0.01, 1.0)),
                        attack: sampler_ir::Time::Milliseconds(f64::from(*attack_ms).max(0.0)),
                        release: sampler_ir::Time::Milliseconds(f64::from(*release_ms).max(0.0)),
                        makeup: sampler_ir::Gain::UNITY,
                        link: *link,
                    }));
                combined = gain;
            }
            Some(Params::Reverb(values)) if scope == Scope::Bus => {
                flush(&mut combined, &mut filters, &mut out);
                out.processors
                    .push(sampler_ir::Processor::Reverb(reverb(values, &mut notes)));
                combined = gain;
            }
            Some(Params::Convolution(c)) if scope == Scope::Bus && impulses.is_some() => {
                let source = impulses.as_deref_mut().expect("checked above");
                match convolution(c, source, &mut notes) {
                    Ok(impulse) => {
                        flush(&mut combined, &mut filters, &mut out);
                        out.processors.push(sampler_ir::Processor::Convolution {
                            impulse,
                            dry: if mix.is_some() {
                                0.0
                            } else {
                                f64::from(fx.dry_level)
                            },
                            wet: if mix.is_some() { 1.0 } else { wet },
                        });
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
                out.processors.push(sampler_ir::Processor::Gainer {
                    gain: sampler_ir::Gain::Linear(
                        f64::from(*parameter) * if mix.is_some() { 1.0 } else { wet },
                    ),
                    dry: if mix.is_some() {
                        0.0
                    } else {
                        f64::from(fx.dry_level)
                    },
                });
            }
            Some(Params::StereoModeller {
                spread,
                pan,
                pseudo,
            }) => {
                flush(&mut combined, &mut filters, &mut out);
                out.processors.push(sampler_ir::Processor::StereoModeller {
                    width: (f64::from(*spread).clamp(-1.0, 1.0) + 1.0) * 0.5,
                    pan: f64::from(*pan).clamp(-1.0, 1.0),
                    pseudo: *pseudo,
                });
                combined = gain;
            }
            Some(p) => match matrix(p, &mut notes) {
                Some(m) => combined = product(product(gain, m), combined),
                None => modelled = false,
            },
            None => modelled = false,
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
                        dry: if convolution || fx.module == 0x13 {
                            f64::from(fx.dry_level)
                        } else {
                            0.0
                        },
                        wet,
                        bypass: fx.bypass,
                    },
                );
                for (_, at) in &mut out.filter_slots {
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
                format!("{name} v{:#x} len {}", fx.version, fx.public.len()),
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

/// The slot's impulse response, shaped as Kontakt's controls ask, in `impulses`.
/// What it cannot shape is reported in `notes`.
fn convolution(
    c: &Convolution,
    impulses: &mut Impulses,
    notes: &mut Notes,
) -> Result<sampler_ir::ImpulseRef, String> {
    use sampler_ir::Reason::NotModeled;
    let (rate, frames) = (impulses.load)(c.ir_index)?;
    if frames.is_empty() || rate == 0 {
        return Err("the impulse response is empty".into());
    }
    let mut channels: [Vec<f32>; 2] = std::array::from_fn(|ch| {
        let mut x: Vec<f32> = frames.iter().map(|f| f[ch]).collect();
        if c.flags[0] {
            x.reverse();
        }
        x
    });
    if c.early[0] != 1.0 || c.late[0] != 1.0 {
        notes.push((
            "IR size".into(),
            format!("early {} late {}", c.early[0], c.late[0]),
            NotModeled,
        ));
    }
    if c.early[1..] != c.late[1..] || c.late[1] > 20.0 || c.late[2] < 20_000.0 {
        notes.push((
            "IR early/late filtering".into(),
            format!("early {:?} late {:?} Hz", &c.early[1..], &c.late[1..]),
            NotModeled,
        ));
    }
    if c.decimation > 1.0 {
        notes.push((
            "IR sample-rate decimation".into(),
            c.decimation.to_string(),
            NotModeled,
        ));
    }
    if c.flags[4] {
        let ok = c.curve_x.len() == 8 && c.curve_db.len() == 8;
        if ok {
            // Eight knots, sorted in time, interpolated in amplitude.
            let mut knots: Vec<(f32, f32)> = c
                .curve_x
                .iter()
                .zip(&c.curve_db)
                .map(|(x, db)| (*x, (db * 0.05 * std::f32::consts::LN_10).exp()))
                .collect();
            knots.sort_by(|a, b| a.0.total_cmp(&b.0));
            let len = channels[0].len();
            for pair in knots.windows(2) {
                let at = |x: f32| (len as f32 * x.clamp(0.0, 1.0) + 0.5) as usize;
                let (start, end) = (at(pair[0].0), at(pair[1].0));
                for n in start..end.min(len) {
                    let g = pair[0].1
                        + (pair[1].1 - pair[0].1) * (n - start) as f32 / (end - start) as f32;
                    channels.iter_mut().for_each(|ch| ch[n] *= g);
                }
            }
        } else {
            notes.push((
                "IR volume envelope".into(),
                "not eight knots".into(),
                NotModeled,
            ));
        }
    }
    let pre = (c.predelay_ms.max(0.0) * 0.001 * rate as f32) as usize;
    for ch in &mut channels {
        ch.splice(0..0, std::iter::repeat_n(0.0, pre));
    }
    if c.flags[1] {
        // Auto Gain: the loudest channel's energy to 0.5, at most +6 dB.
        let energy = channels
            .iter()
            .map(|ch| ch.iter().map(|x| x * x).sum::<f32>())
            .fold(0.0, f32::max);
        let gain = if energy >= 0.001 {
            (0.5 / energy).sqrt().min(2.0)
        } else {
            1.0
        };
        channels.iter_mut().flatten().for_each(|x| *x *= gain);
    }
    let [left, right] = channels;
    impulses.store.push(sampler_ir::Impulse {
        rate,
        left,
        right,
        asset: None,
    });
    if let Some(recipes)=impulses.recipes.as_deref_mut() { recipes.push(c.clone()); }
    Ok(sampler_ir::ImpulseRef(impulses.store.len() - 1))
}

/// Kontakt's normalized Reverb values as physical settings. Time, predelay,
/// high cut and low shelf follow the reference display; size, damping and
/// diffusion are still v1's fits.
fn reverb(v: &[f32; 10], notes: &mut Notes) -> sampler_ir::Reverb {
    let [
        room,
        time,
        size,
        damping,
        modulation,
        diffusion,
        predelay,
        high_cut,
        low_shelf,
        stereo,
    ] = v.map(|x| f64::from(x.clamp(0.0, 1.0)));
    notes.push((
        "reverb algorithm".into(),
        format!("normalized {v:?}"),
        sampler_ir::Reason::UnknownLaw,
    ));
    sampler_ir::Reverb {
        // KONTAKT_REFERENCE s.21 and the display read by get_engine_par_disp:
        // Time shows 500 ms * 40.4^x and the measured RT60 is 0.82 x that.
        decay_seconds: 0.82 * 0.5 * 40.4f64.powf(time),
        size: (0.5 + size) * if room >= 0.5 { 1.0 } else { 0.55 },
        damping_hz: 18_000.0 * 0.05f64.powf(damping),
        modulation_seconds: modulation * 0.0015,
        diffusion: 0.75 * diffusion,
        predelay_seconds: predelay * 0.25,
        // High Cut shows 21 kHz - 19 kHz * x (decreasing, linear in Hz).
        input_cutoff_hz: 21_000.0 - 19_000.0 * high_cut,
        // Low Shelf shows 0 to -12 dB, linear.
        low_shelf_db: -12.0 * low_shelf,
        width: stereo,
    }
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
    instrument_buses_with_recipes(ir,racks,buses,dynamic,load,None)
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
        generic(1), (-1, 1),
        &[],
    );
    take("instrument insert", &insert);
    let main = chain_with(
        rack("instrument main"),
        Scope::Bus,
        Some(&mut source),
        generic(2), (-1, 2),
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
            generic(0), (-1, 0),
            &[],
        );
        // Port from v1 0cb7a8a0:src/fx/processor.rs (process returns and
        // SendInputs::tap): bypassed returns contribute no dry signal.
        // Keep the real slot control so scripts can re-enable the return.
        if dynamic && !c.processors.is_empty() {
            c.processors.push(sampler_ir::Processor::SendReturnGate {
                address: sampler_ir::SlotAddress { group: -1, slot: slot.slot as i32, generic: 0 },
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
    let mut panned = Vec::new();
    for bus in buses.iter().filter(|b| dynamic || !b.groups.is_empty()) {
        let name = format!("bus {}", bus.index);
        let c = chain_with(
            rack(&name),
            Scope::Bus,
            Some(&mut source),
            generic(1000 + bus.index as i32), (-1, 1000 + bus.index as i32),
            &[],
        );
        take(&name, &c);
        if bus.pan.abs() > 0.01 {
            panned.push((name, bus.pan));
        }
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
            instrument.push((bus, processors));
        } else if !processors.is_empty() || (bus.volume - 1.0).abs() > 1e-4 {
            instrument.push((bus, processors));
        }
    }
    for (name, pan) in panned {
        report.push((
            name,
            (
                0,
                "instrument bus pan".into(),
                format!("{pan}"),
                sampler_ir::Reason::NotModeled,
            ),
        ));
    }
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
    let send_buses: Vec<_> = sends.iter().enumerate().map(|(i, (slot, _, _))| (*slot, BusRef(i + 1))).collect();
    let entry = if chained {
        let mut segments = Vec::new();
        let mut cursor = 0;
        for tap in &insert.send_taps {
            let sampler_ir::VoiceSendPosition::BeforeAmplitude(n) = tap.position else { unreachable!() };
            let feeds = if tap.bypass { Vec::new() } else {
                send_buses.iter().filter_map(|&(slot, bus)| tap.levels.get(slot).map(|&level| Send {
                    to: Output::Bus(bus), gain: sampler_ir::Gain::Linear(f64::from(level)),
                    position: SendPosition::PostChain,
                })).collect()
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
        let next = if segments.peek().is_some() { Output::Bus(BusRef(continuation)) } else { target };
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
            let next = if segments.peek().is_some() { Output::Bus(BusRef(continuation + i)) } else { target };
            add(ir, format!("insert continuation {i}"), processors, feeds, next, unity);
        }
        for group in &mut ir.groups {
            group.output = Output::Bus(BusRef(0));
        }
        Output::Bus(BusRef(0))
    } else {
        Output::Master
    };
    ir.input_bus = if let Output::Bus(bus) = entry { Some(bus) } else { None };
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

/// One EQ band (Hz, octaves, dB); live owners retain flat bands.
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
    // A peaking biquad whose bandwidth spans `octaves` (RBJ). Kontakt's own
    // band shape is not verified against a rendering.
    notes.push((
        "EQ band shape".into(),
        format!("{hz} Hz {octaves} oct {db} dB"),
        sampler_ir::Reason::UnknownLaw,
    ));
    let q = 1.0 / (2.0 * (std::f64::consts::LN_2 / 2.0 * f64::from(octaves)).sinh());
    Some(sampler_ir::Processor::Filter(sampler_ir::Filter {
        kind: sampler_ir::FilterKind::Peak {
            gain: sampler_ir::Gain::Decibels(f64::from(db)),
        },
        cutoff: sampler_ir::Frequency::Hertz(f64::from(hz)),
        resonance: sampler_ir::Resonance::Q(q),
    }))
}

#[cfg(test)]
mod tests {
    use ni_file::kontakt::Chunk;
    #[test]
    fn live_eq_retains_flat_bands_and_their_physical_indices() {
        let mut public = Vec::new();
        for kind in [24i32, 24] { public.extend(kind.to_le_bytes()); }
        for values in [[300f32, 1., 0.], [1000., 1., 12.], [6000., 1., 0.]] {
            for value in values { public.extend(value.to_le_bytes()); }
        }
        let slot = super::Slot { slot: 5, module: 0x18, version: 0x92,
            bypass: false, output_gain: 1., dry_level: 0., output_set: false, public };
        let fixed = super::chain(std::slice::from_ref(&slot), super::Scope::Voice);
        assert_eq!(fixed.processors.iter().filter(|p| matches!(p, sampler_ir::Processor::Filter(_))).count(), 1,
            "the static unmodulated path keeps its existing arithmetic");
        let live = super::chain_with(std::slice::from_ref(&slot), super::Scope::Voice,
            None, Some((7, -1)), (7, -1), &[]);
        let routed = super::chain_with(std::slice::from_ref(&slot), super::Scope::Voice,
            None, None, (7, -1), &[5]);
        assert_eq!(routed.filter_slots, vec![(5, 0), (5, 1), (5, 2)]);
        assert_eq!(live.filter_slots.len(), 3, "live EQ must retain physical band owners, including flat bands");
        for (band, &(physical, index)) in live.filter_slots.iter().enumerate() {
            assert_eq!(physical, 5);
            assert!(matches!(live.processors[index], sampler_ir::Processor::Filter(sampler_ir::Filter {
                kind: sampler_ir::FilterKind::Peak { .. }, .. })), "band {band} must address its actual EQ processor");
        }
    }

    #[test]
    fn ladder_and_daft_cutoff_routes_retain_the_authored_physical_slot() {
        for kind in [33i32, 70, 71] {
            let mut public = kind.to_le_bytes().to_vec();
            if kind == 33 { public.push(0); }
            public.extend(kind.to_le_bytes());
            for value in [0.2f32, 0.5, 0.3] { public.extend(value.to_le_bytes()); }
            let slot = super::Slot { slot: 5, module: 0x18, version: 0x92,
                bypass: false, output_gain: 1., dry_level: 0., output_set: false, public };
            for dynamic in [None, Some((7, -1))] {
                let chain = super::chain_with(std::slice::from_ref(&slot), super::Scope::Voice, None, dynamic, (7, -1), &[]);
                let index = usize::from(dynamic.is_some());
                assert_eq!(chain.filter_slots, vec![(5, index)], "native filter {kind} loses its addressed cutoff consumer");
                assert!(matches!(chain.processors[index], sampler_ir::Processor::LadderLP4(_) | sampler_ir::Processor::Daft(_)));
            }
        }
    }

    #[test]
    fn native_ladder_lp4_import_preserves_signed_gain_and_record_version() {
        let mut public = 33i32.to_le_bytes().to_vec();
        public.push(0);
        public.extend(33i32.to_le_bytes());
        for value in [-0.25f32, 0.5, 0.3] { public.extend(value.to_le_bytes()); }
        let slot = super::Slot { slot: 3, module: 0x18, version: 0x92,
            bypass: false, output_gain: 1., dry_level: 0., output_set: false, public };
        let chain = super::chain_with(&[slot], super::Scope::Voice, None, Some((7, -1)), (7, -1), &[]);
        assert!(chain.notes.is_empty());
        assert!(chain.processors.iter().any(|p| matches!(p,
            sampler_ir::Processor::LadderLP4(d) if d.gain == -0.25
                && d.cutoff == 0.5 && (d.resonance - 0.3).abs() < 1e-7
                && d.record_version == 0x92)));
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
                [sampler_ir::Processor::Gainer {
                    gain: sampler_ir::Gain::Linear(2.0),
                    dry: 1.0
                }]
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
    fn sv_filters_follow_the_measured_laws() {
        let sampler_ir::Processor::Filter(f) = filter(52, 0.293, 0.0).unwrap() else {
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
        assert!(filter(3, 0.0, 0.0).is_none() && filter(106, 0.5, 0.5).is_none());
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

    #[test]
    fn unsupported_effect_diagnostics_group_without_public_payload_bytes() {
        let mut values = Vec::new();
        for byte in [0xa1, 0xb2] {
            let mut authored = slot(0xfe, vec![byte; 64], 1.0);
            authored.slot = 6;
            let out = chain(&[authored], Scope::Voice);
            let (physical_slot, feature, value, reason) = out.notes.iter()
                .find(|(_, feature, _, _)| feature == "effect").expect("missing effect diagnostic");
            assert_eq!((*physical_slot, feature.as_str(), *reason),
                (6, "effect", sampler_ir::Reason::NotModeled));
            assert!(value.contains("unknown effect 0xfe") && value.contains("v0x50"));
            assert!(value.contains("len 64"));
            assert!(!value.contains("head") && !value.contains(&format!("{byte:02x}")));
            values.push(value.clone());
        }
        assert_eq!(values[0], values[1], "diagnostic groups must not depend on payload content");
    }

    #[test]
    fn authored_lofi_slot_is_an_executable_processor_in_both_scopes() {
        let mut payload: Vec<u8> = [0.4f32, 0.2, 0.0]
            .into_iter().flat_map(f32::to_le_bytes).collect();
        payload.push(0); // typed fourth field, between NoiseLevel and NoiseColor
        payload.extend(0.5f32.to_le_bytes());
        for scope in [Scope::Voice, Scope::Bus] {
            let out = chain(&[slot(0x20, payload.clone(), 1.)], scope);
            assert!(out.notes.is_empty(), "{:?}", out.notes);
            assert!(!out.processors.is_empty(), "Lo-Fi cannot disappear");
        }
    }

    #[test]
    fn v1_formant_slot_is_an_executable_processor_in_both_scopes() {
        let mut payload = 90i32.to_le_bytes().repeat(2);
        for value in [0.25f32, 0.5, 0.5] { payload.extend(value.to_le_bytes()); }
        for scope in [Scope::Voice, Scope::Bus] {
            let out = chain(&[slot(0x18, payload.clone(), 1.)], scope);
            assert!(!out.processors.is_empty(), "v1 executes Formant I");
            assert!(!out.notes.iter().any(|(_,_,_,reason)| *reason == sampler_ir::Reason::NotModeled));
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
        let mut writes = vec![sampler_ksp::EnginePar {
            parameter: "$ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN".into(),
            value: 560_434, group: -1, slot: 1, generic: 1,
        }, sampler_ksp::EnginePar {
            parameter: "$ENGINE_PAR_EFFECT_BYPASS".into(),
            value: 1, group: -1, slot: 1, generic: 1,
        }];
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
        assert!(script.writes_effect_slots(), "shared engine writes must retain live slot lanes");
    }

    #[test]
    fn input_tone_marker_names_instrument_insert_entry() {
        let mut i=sampler_ir::Instrument::default(); i.groups.push(Default::default());
        instrument_buses(&mut i,&[("instrument insert".into(),vec![slot(0x13,1.0f32.to_le_bytes().to_vec(),1.)])],&[],false,&mut |_|Err("none".into()));
        assert_eq!(i.input_bus,Some(sampler_ir::BusRef(0)));
        assert_eq!(i.groups[0].output,sampler_ir::Output::Bus(i.input_bus.unwrap()));
        i.validate().unwrap();
    }

    #[test]
    fn dynamic_bypassed_send_returns_add_no_dry_copy() {
        use sampler_ir as ir;
        let render = |dynamic| {
            let mut instrument = ir::Instrument::default();
            instrument.assets.push(ir::Asset { location: ir::AssetLocation::Path("probe".into()),
                encoding: ir::Encoding::Wav, root_key: None, loops: Vec::new() });
            instrument.groups.push(ir::Group::default());
            let mut zone = ir::Zone::new(ir::AssetRef(0));
            zone.group = Some(ir::GroupRef(0));
            zone.keys = ir::KeyRange { low: 60, high: 60 };
            zone.velocity = ir::VelocityResponse::None;
            instrument.zones.push(zone);
            let mut send = slot(0x13, 1.0f32.to_le_bytes().to_vec(), 1.0);
            send.bypass = true;
            send.dry_level = 0.0;
            instrument_buses(&mut instrument, &[("instrument send".into(), vec![send])], &[], dynamic,
                &mut |_| Err("no impulse".into()));
            let pcm = sampler_core::Pcm::new(48000, vec![[0.25; 2]; 4096].into_boxed_slice()).unwrap();
            let plan = sampler_core::lower::lower(&instrument, 48000, vec![pcm], |_, plan| Ok(plan)).unwrap();
            let mut rt = sampler_core::Runtime::new(plan, sampler_core::Limits {
                notes: 4, channels: 1, performances: 1, families: 4, expressions: 4,
                voices: 4, decisions: 8, commands: 8, behaviors: 0, behavior_fuel: 0,
                behavior_cells: 0, note_cells: 0,
            }).unwrap();
            rt.trigger(sampler_core::Input { protocol: sampler_core::Protocol::Native,
                port: 0, group: 0, channel: 0, key: 60, external_id: None }, 60, 1.0).unwrap();
            let mut output = [[0.0; 2]; 64];
            rt.render(&mut output).unwrap();
            let initial = output[32][0];
            if dynamic {
                let address = sampler_core::EngineParameterAddress {
                    parameter: sampler_core::engine_parameter_id("ENGINE_PAR_SEND_EFFECT_BYPASS").unwrap(),
                    group: -1, slot: 0, generic: 0,
                };
                let mut settled = [[0.0; 2]; 512];
                rt.set_engine_parameter(address, 0).unwrap();
                rt.render(&mut settled).unwrap();
                assert!((settled[511][0] - initial * 2.0).abs() < 1e-6, "live send did not return: {} vs {}", settled[511][0], initial * 2.0);
                rt.set_engine_parameter(address, 1).unwrap();
                rt.render(&mut settled).unwrap();
                assert!((settled[511][0] - initial).abs() < 1e-6, "live bypass did not mute return");
            }
            initial
        };
        let (saved, live) = (render(false), render(true));
        assert!(saved > 0.0);
        assert!((live / saved - 1.0).abs() < 1e-6, "bypassed send changed dry gain: {saved} -> {live}");
    }

    #[test]
    #[ignore = "installed library metadata probe; run through kontakto-heavy"]
    fn analog_saved_compressor_state() {
        let path = std::path::Path::new("/mnt/MAIN_STORAGE/Libraries/Kontakt/ANALOG STRINGS/Instruments/ANALOG STRINGS.nki");
        if !path.exists() { return; }
        let chunks = crate::read_chunks(path).unwrap();
        let program = Program::try_from(chunks.find_first(0x28).unwrap()).unwrap();
        let racks = program_racks(&program, &[], |_, _| panic!("rack decode failed"));
        let (_, insert) = racks.iter().find(|(name, _)| name == "instrument insert").unwrap();
        let compressors: Vec<_> = insert.iter().filter(|slot| slot.module == 0x19).collect();
        assert_eq!(compressors.len(), 1);
        let fx = compressors[0];
        eprintln!("saved compressor slot={} bypass={} output_gain={} output_db={}",
            fx.slot, fx.bypass, fx.output_gain, 20.0 * f64::from(fx.output_gain).log10());
        assert_eq!(fx.slot, 1);
        assert!(!fx.bypass);
        let v1_gain = 16.0 * (560_434.0f64 / 1_000_000.0).powi(3);
        assert!((f64::from(fx.output_gain) - v1_gain).abs() < 1e-6);
        assert!((20.0 * (f64::from(fx.output_gain) / f64::from(engine_gain(560_434))).log10()).abs() < 0.00005);
    }

    #[test]
    fn reverb_time_high_cut_and_low_shelf_follow_the_reference_display() {
        let r = |time: f32, cut: f32, shelf: f32| {
            reverb(
                &[0.5, time, 0.5, 0.5, 0.5, 0.5, 0.5, cut, shelf, 1.0],
                &mut Vec::new(),
            )
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
        let [sampler_ir::Processor::Gainer { gain, dry }] = c.processors[..] else {
            panic!("{:?}", c.processors)
        };
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
                .any(|p| matches!(p, sampler_ir::Processor::Compressor(_))),
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
            sampler_ir::Processor::Gainer {
                gain: sampler_ir::Gain::Linear(2.0),
                dry: 0.0
            }
        );
        assert_eq!(
            chain.processors[1],
            sampler_ir::Processor::Gainer {
                gain: sampler_ir::Gain::Linear(3.0),
                dry: 0.0
            }
        );
    }

    #[test]
    fn dynamic_slots_run_in_mix_blocks_bypassed_ones_included() {
        let mut gainer = slot(0x13, 2.0f32.to_le_bytes().to_vec(), 1.0);
        gainer.slot = 3;
        gainer.bypass = true;
        let plain = chain_with(std::slice::from_ref(&gainer), Scope::Bus, None, None, (-1, 1), &[]);
        assert!(plain.processors.is_empty());
        let c = chain_with(
            std::slice::from_ref(&gainer),
            Scope::Bus,
            None,
            Some((-1, 1)), (-1, 1),
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
                    sampler_ir::Processor::Gainer { .. }
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
                    sampler_ir::Processor::StereoModeller {
                        width,
                        pan: imported_pan,
                        pseudo: false,
                    },
                ] => {
                    assert_eq!(*width, (f64::from(spread).clamp(-1.0, 1.0) + 1.0) * 0.5);
                    assert_eq!(*imported_pan, f64::from(pan));
                    matrix(
                        &Params::StereoModeller {
                            spread,
                            pan,
                            pseudo: false,
                        },
                        &mut Vec::new(),
                    )
                    .unwrap()
                }
                other => panic!("{other:?}"),
            }
        };
        let near = |m: [[f64; 2]; 2], want: [[f64; 2]; 2]| {
            for (a, b) in m.iter().flatten().zip(want.iter().flatten()) {
                assert!((a - b).abs() < 1e-6, "{m:?} against {want:?}");
            }
        };
        // KONTAKT_REFERENCE.md s.20, divided by the base gain 0.3838 (vel 100):
        // 100%: 0.7677 / -0.3838; 50%: 0.5757 / -0.1919; -50%: 0.2879 / 0.0960.
        near(modeller(1.0, 0.0), [[2.0, -1.0], [-1.0, 2.0]]);
        near(modeller(0.5, 0.0), [[1.5, -0.5], [-0.5, 1.5]]);
        near(modeller(1.5, 0.0), [[2.0, -1.0], [-1.0, 2.0]]);
        near(modeller(-0.5, 0.0), [[0.75, 0.25], [0.25, 0.75]]);
        near(modeller(-1.0, 0.0), [[0.5, 0.5], [0.5, 0.5]]);
        // Pan -50: R x0.5.
        near(modeller(0.0, -0.5), [[1.0, 0.0], [0.0, 0.5]]);
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
    fn compressor_slot_becomes_a_compressor_with_its_link_flag() {
        let mut bytes = Vec::new();
        for x in [0.0f32, -18.0, 0.25, 10.0, 120.0] {
            bytes.extend(x.to_le_bytes());
        }
        bytes.push(1);
        let built = chain(&[slot(0x19, bytes, 1.0)], Scope::Bus);
        let [sampler_ir::Processor::Compressor(c), ..] = built.processors[..] else {
            panic!("{:?}", built.processors)
        };
        assert_eq!((c.threshold_db, c.ratio, c.link), (-18.0, 4.0, true));
        assert_eq!(c.attack.seconds(), 0.01);
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
                sampler_ir::Processor::Gainer {
                    gain: sampler_ir::Gain::Linear(2.0),
                    dry: 0.0
                },
                sampler_ir::Processor::StereoMatrix([[0.0, -0.5], [-0.5, 0.0]]),
                sampler_ir::Processor::StereoModeller {
                    width: 0.5,
                    pan: 0.0,
                    pseudo: false
                },
                sampler_ir::Processor::StereoMatrix([[2.0, 0.0], [0.0, 2.0]]),
            ]
        );
        assert_eq!(notes.len(), 1, "{notes:?}");
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
                    sampler_ir::Processor::Filter(f),
                    sampler_ir::Processor::StereoMatrix([[2.0, 0.0], [0.0, 2.0]])
                ] if f.cutoff == sampler_ir::Frequency::Hertz(1000.0)
                    && matches!(f.resonance, sampler_ir::Resonance::Q(q) if (q - std::f64::consts::SQRT_2).abs() < 1e-9)
            ),
            "{processors:?}"
        );
        // Unity Gainer retains its stateful parameter owner.
        let mut unity = slot(0x13, 1.0f32.to_le_bytes().to_vec(), 1.0);
        unity.dry_level = 0.0;
        let processors = chain(&[unity], Scope::Voice).processors;
        assert_eq!(
            processors,
            vec![sampler_ir::Processor::Gainer {
                gain: sampler_ir::Gain::UNITY,
                dry: 0.0
            }]
        );
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
                start: Vec::new(),
                name: "g".into(),
                gain: ir::Gain::UNITY,
                pan: ir::Pan::default(),
                tune: ir::Pitch::Semitones(0.0),
                chain: None,
                output: ir::Output::Master,
                voice_limit: None,
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
            [ir::Processor::Reverb(_)]
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
        let (_, returns) = instrument_buses(&mut sparse, &[
            ("instrument insert".into(), vec![slot(0x17, zero, 1.0)]),
            ("instrument send".into(), vec![return_slot]),
        ], &[], false, &mut |_| Err("none".into()));
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
        assert_eq!(chain.send_taps[0].position, sampler_ir::VoiceSendPosition::BeforeAmplitude(0));
        assert_eq!(chain.send_taps[1].slot, 6);
        assert_eq!(chain.send_taps[1].position, sampler_ir::VoiceSendPosition::AfterAmplitude(0));
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
        instrument_buses(&mut ir, &[
            ("instrument insert".into(), vec![before, tap, after]),
            ("instrument send".into(), vec![slot(0x13, 0.5f32.to_le_bytes().to_vec(), 1.0)]),
        ], &[], false, &mut |_| Err("none".into()));
        assert_eq!(ir.buses.len(), 3);
        assert_eq!(ir.buses[0].output, ir::Output::Bus(ir::BusRef(2)));
        assert_eq!(ir.buses[0].sends[0].to, ir::Output::Bus(ir::BusRef(1)));
        assert_eq!(ir.buses[0].sends[0].gain, ir::Gain::Linear(0.5));
        for (bus, gain) in [(0, 2.0), (2, 3.0)] {
            let stages = &ir.chains[ir.buses[bus].chain.unwrap().0].pre_amplitude;
            assert!(matches!(stages[..], [ir::Processor::Gainer { gain: ir::Gain::Linear(g), dry: 0.0 }] if g == gain));
        }
        ir.validate().unwrap();
    }
}

/// Rebuild an impulse with the same shaping path; cache metadata carries no audio.
pub(crate) fn restore_impulse(c:&Convolution,decoded:Decoded)->Result<sampler_ir::Impulse,String> {
    let mut store=Vec::new(); let mut decoded=Some(decoded);
    let mut load=|_|decoded.take().ok_or_else(||"impulse already consumed".to_owned());
    convolution(c,&mut Impulses {store:&mut store,load:&mut load,recipes:None},&mut Vec::new())?;
    store.pop().ok_or_else(||"missing shaped impulse".into())
}
