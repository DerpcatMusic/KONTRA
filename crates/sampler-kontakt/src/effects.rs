//! Kontakt effect racks: where they live in a program, and module names.
//!
//! A program holds up to three 8-slot racks as `BParamArray<BParFX,8>`
//! children (0x3a: insert, send, main, in that order) and up to 16 instrument
//! buses (0x45, each with its own rack). Groups keep an insert rack in their
//! private data. Each slot's first child is the effect object, whose chunk
//! ID names the module.

use ni_file::kontakt::{
    Chunk, StructuredObject,
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
    pub public: Vec<u8>,
    /// Kept for the slot's opaque state; no law reads it yet.
    #[allow(dead_code)]
    pub private: Vec<u8>,
}

pub(crate) fn rack(array: &BParamArrayBParFX8) -> Vec<Slot> {
    array
        .items
        .iter()
        .enumerate()
        .filter_map(|(slot, chunk)| {
            let fx = BParFX::try_from(chunk.as_ref()?).ok()?;
            let state = fx.params().ok()?;
            let effect: &Chunk = fx.effect()?;
            let object = StructuredObject::try_from(effect).ok();
            Some(Slot {
                slot,
                module: effect.id,
                version: object.as_ref().map_or(0, |o| o.version),
                bypass: state.bypass,
                output_gain: state.output_gain,
                dry_level: state.dry_level,
                public: object
                    .as_ref()
                    .map_or_else(Vec::new, |o| o.public_data.clone()),
                private: object.map_or_else(Vec::new, |o| o.private_data),
            })
        })
        .collect()
}

/// The program's racks with their locations: instrument insert, send and
/// main, then each bus.
pub(crate) fn program_racks(program: &Program) -> Vec<(String, Vec<Slot>)> {
    let names = ["instrument insert", "instrument send", "instrument main"];
    let mut out = Vec::new();
    let mut racks = 0;
    let mut buses = 0;
    for child in &program.0.children {
        match child.id {
            RACK => {
                let name = names
                    .get(racks)
                    .map_or_else(|| format!("rack {racks}"), |n| (*n).into());
                racks += 1;
                if let Ok(array) = BParamArrayBParFX8::try_from(child) {
                    out.push((name, rack(&array)));
                }
            }
            BUS => {
                let index = buses;
                buses += 1;
                if let Ok(bus) = InsertBus::try_from(child)
                    && let Some(array) = bus
                        .0
                        .find_first(RACK)
                        .and_then(|c| BParamArrayBParFX8::try_from(c).ok())
                {
                    out.push((format!("bus {index}"), rack(&array)));
                }
            }
            _ => {}
        }
    }
    out
}

/// A module's stored parameters, where the layout is known (byte-exact on
/// every local instance in v1's survey).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Params {
    /// Linear gain.
    Gainer { gain: f32 },
    /// `$ENGINE_PAR_STEREO` (offset from 100% width), `$ENGINE_PAR_STEREO_PAN`,
    /// `$ENGINE_PAR_STEREO_PSEUDO`.
    StereoModeller { spread: f32, pan: f32, pseudo: bool },
    /// `$ENGINE_PAR_PHASE_INVERT`, `$ENGINE_PAR_LR_SWAP`.
    Inverter { invert: bool, swap: bool },
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
}

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
                if r.i32()? != kind {
                    return None;
                }
                if (22..=24).contains(&kind) {
                    let bands = (21..kind)
                        .map(|_| Some([r.f32()?, r.f32()?, r.f32()?]))
                        .collect::<Option<_>>()?;
                    Params::Eq { bands }
                } else {
                    // Ladder (70, 71) stores a leading value first.
                    let leading = if matches!(kind, 70 | 71) {
                        Some(r.f32()?)
                    } else {
                        None
                    };
                    let (cutoff, resonance) = (r.f32()?, r.f32()?);
                    let mut extra: Vec<f32> = leading.into_iter().collect();
                    while let Some(x) = r.f32() {
                        extra.push(x);
                    }
                    Params::Filter {
                        kind,
                        cutoff,
                        resonance,
                        extra,
                    }
                }
            }
            _ => return None,
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
    /// Each translated SV filter slot and its index in `processors`, for
    /// modulation targets that name a module slot.
    pub filter_slots: Vec<(usize, usize)>,
}

/// Translate one rack. Each slot scales its output by its output gain. The
/// slot's dry level is not mixed back: local presets store 1.0 on Stereo
/// Modeller, Inverter and EQ slots used as gain trims (output gains in
/// whole-dB steps), where an added dry path would contradict the trim.
/// Linear stereo stages fold into one matrix; EQ bands act alike on both
/// channels, so they commute with it.
pub(crate) fn chain(slots: &[Slot], scope: Scope) -> Chain {
    chain_with(slots, scope, None)
}

/// A decoded impulse response: its sample rate and frames.
pub(crate) type Decoded = (u32, Vec<[f32; 2]>);

/// Where convolution slots find their impulse responses: `load` decodes the
/// response an other-files index names, `store` collects the shaped ones.
pub(crate) struct Impulses<'a> {
    pub store: &'a mut Vec<sampler_ir::Impulse>,
    pub load: &'a mut dyn FnMut(i32) -> Result<Decoded, String>,
}

/// [`chain`], translating convolutions when `impulses` is given (bus scope).
pub(crate) fn chain_with(
    slots: &[Slot],
    scope: Scope,
    mut impulses: Option<&mut Impulses>,
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
    for fx in slots.iter().filter(|fx| !fx.bypass) {
        let name = module_name(fx.module);
        let mut notes = Vec::new();
        let params = fx.params();
        // The Inverter's Output knob does not reach the signal: Una g39 and g94
        // (post-amp Inverter, Output +6.0 dB) read -15.8 and -17.7 dBFS in
        // Kontakt 8 at key 60 vel 100, which is KONTRA exactly without it and
        // 6.0 dB louder with it.
        let wet = if fx.module == 0x1a {
            1.0
        } else {
            f64::from(fx.output_gain)
        };
        let gain = [[wet, 0.0], [0.0, wet]];
        let mut modelled = true;
        match &params {
            Some(Params::Eq { bands }) => {
                filters.extend(bands.iter().filter_map(|band| eq_band(*band, &mut notes)));
                combined = product(gain, combined);
            }
            Some(Params::SendLevels { sends, .. }) if scope == Scope::Bus => {
                if out.sends.is_empty() {
                    out.sends = sends.clone();
                } else {
                    modelled = false;
                }
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
                            dry: f64::from(fx.dry_level),
                            wet,
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
            Some(p) => match matrix(p, &mut notes) {
                Some(m) => combined = product(product(gain, m), combined),
                None => modelled = false,
            },
            None => modelled = false,
        }
        if !modelled {
            out.notes.push((
                fx.slot,
                "effect".into(),
                format!("{name} v{:#x} {params:?}", fx.version),
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
    impulses
        .store
        .push(sampler_ir::Impulse { rate, left, right });
    Ok(sampler_ir::ImpulseRef(impulses.store.len() - 1))
}

/// Kontakt's normalized Reverb values as physical settings. Laws are v1's
/// fits, not verified against Kontakt's own rendering.
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
        decay_seconds: 0.2 * 100f64.powf(time),
        size: (0.5 + size) * if room >= 0.5 { 1.0 } else { 0.55 },
        damping_hz: 18_000.0 * 0.05f64.powf(damping),
        modulation_seconds: modulation * 0.0015,
        diffusion: 0.75 * diffusion,
        predelay_seconds: predelay * 0.25,
        input_cutoff_hz: 20_000.0 * 0.025f64.powf(high_cut),
        low_shelf_db: -18.0 * low_shelf,
        width: stereo,
    }
}

/// The instrument-level racks as buses: every group feeds an insert bus
/// (the insert rack), whose Send Levels slots feed one bus per send slot
/// (the send rack's effects), and the main rack follows both. Changes
/// nothing when the racks do nothing.
pub(crate) fn instrument_buses(
    ir: &mut sampler_ir::Instrument,
    racks: &[(String, Vec<Slot>)],
    load: &mut dyn FnMut(i32) -> Result<Decoded, String>,
) -> Vec<(String, Note)> {
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
        load,
    };
    let insert = chain_with(rack("instrument insert"), Scope::Bus, Some(&mut source));
    take("instrument insert", &insert);
    let main = chain_with(rack("instrument main"), Scope::Bus, Some(&mut source));
    take("instrument main", &main);
    // A send slot's effect runs on its own bus, fed at the Send Levels slot's level.
    let mut sends = Vec::new();
    for slot in rack("instrument send").iter().filter(|s| !s.bypass) {
        let c = chain_with(std::slice::from_ref(slot), Scope::Bus, Some(&mut source));
        take("instrument send", &c);
        let level = insert.sends.get(slot.slot).copied().unwrap_or(1.0);
        if !c.processors.is_empty() && level > 0.0 {
            sends.push((c.processors, f64::from(level)));
        }
    }
    ir.impulses = store;
    if insert.processors.is_empty() && sends.is_empty() && main.processors.is_empty() {
        return report;
    }
    // Bus order: insert, sends, main.
    let main_bus = (!main.processors.is_empty()).then_some(sends.len() + 1);
    let target = main_bus.map_or(Output::Master, |i| Output::Bus(BusRef(i)));
    let add = |ir: &mut sampler_ir::Instrument, name: String, processors, sends, output| {
        let chain = (!Vec::<sampler_ir::Processor>::is_empty(&processors)).then(|| {
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
            gain: sampler_ir::Gain::UNITY,
        });
    };
    let feeds = sends
        .iter()
        .enumerate()
        .map(|(i, (_, level))| Send {
            to: Output::Bus(BusRef(i + 1)),
            gain: sampler_ir::Gain::Linear(*level),
            position: SendPosition::PostChain,
        })
        .collect();
    add(ir, "insert".into(), insert.processors, feeds, target);
    for (i, (processors, _)) in sends.into_iter().enumerate() {
        add(ir, format!("send {i}"), processors, Vec::new(), target);
    }
    if main_bus.is_some() {
        add(
            ir,
            "main".into(),
            main.processors,
            Vec::new(),
            Output::Master,
        );
    }
    for group in &mut ir.groups {
        group.output = Output::Bus(BusRef(0));
    }
    report
}

/// One EQ band (Hz, octaves, dB) as a peaking filter; flat bands vanish.
fn eq_band(
    [hz, octaves, db]: [f32; 3],
    notes: &mut Vec<(String, String, sampler_ir::Reason)>,
) -> Option<sampler_ir::Processor> {
    if db == 0.0 {
        return None;
    }
    if !(hz > 0.0 && octaves > 0.0) {
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
            public,
            private: Vec::new(),
        }
    }

    #[test]
    fn modeller_follows_the_measured_spread_and_pan_laws_and_the_inverter_output_is_pending() {
        let modeller = |spread: f32, pan: f32| {
            let mut bytes = spread.to_le_bytes().to_vec();
            bytes.extend(pan.to_le_bytes());
            bytes.push(0);
            match chain(&[slot(0x1f, bytes, 1.0)], Scope::Voice)
                .processors
                .as_slice()
            {
                [sampler_ir::Processor::StereoMatrix(m)] => *m,
                [] => [[1.0, 0.0], [0.0, 1.0]],
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
        // PENDING (reference agent measuring Output at -6/0/+6 dB): Una g39 and
        // g94's post-amp Inverter at +6 dB reads exactly as if its Output were
        // not applied. A single reading; if it fails, apply the gain again.
        let inverter = chain(&[slot(0x1a, vec![0, 0], 2.0)], Scope::Voice).processors;
        assert!(inverter.is_empty(), "{inverter:?}");
    }

    #[test]
    fn linear_inserts_fold_into_one_matrix_with_slot_gains() {
        let gainer = slot(0x13, 2.0f32.to_le_bytes().to_vec(), 1.0);
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
        // 2 * (swap, inverted; its Output is not applied) * 2 = swap, inverted, * 4.
        assert_eq!(
            processors,
            vec![sampler_ir::Processor::StereoMatrix([
                [0.0, -4.0],
                [-4.0, 0.0]
            ])]
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
        // Unity everything: no processor at all.
        let processors = chain(
            &[slot(0x13, 1.0f32.to_le_bytes().to_vec(), 1.0)],
            Scope::Voice,
        )
        .processors;
        assert!(processors.is_empty());
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
        let report = instrument_buses(&mut instrument, &racks, &mut |_| Err("none".into()));
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
        // Nothing to do: no buses.
        let mut plain = ir::Instrument::default();
        instrument_buses(&mut plain, &[], &mut |_| Err("none".into()));
        assert!(plain.buses.is_empty());
    }
}
