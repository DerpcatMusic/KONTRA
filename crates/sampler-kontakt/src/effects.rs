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

const RACK: u16 = 0x3a;
const BUS: u16 = 0x45;

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

type Matrix = [[f64; 2]; 2];
const IDENTITY: Matrix = [[1.0, 0.0], [0.0, 1.0]];

fn product(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|i| std::array::from_fn(|j| a[i][0] * b[0][j] + a[i][1] * b[1][j]))
}

/// The stereo matrix of a linear module, and what it leaves out.
fn matrix(
    params: &Params,
    notes: &mut Vec<(String, String, sampler_ir::Reason)>,
) -> Option<Matrix> {
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
            if spread != 0.0 || pan != 0.0 {
                // Mid/side width 1 + spread and balance pan: v1's law, not
                // verified against Kontakt.
                notes.push((
                    "stereo modeller width/pan law".into(),
                    format!("spread {spread} pan {pan}"),
                    UnknownLaw,
                ));
            }
            let width = (1.0 + f64::from(spread)).clamp(0.0, 2.0);
            let pan = f64::from(pan);
            let gains = [(1.0 - pan).clamp(0.0, 1.0), (1.0 + pan).clamp(0.0, 1.0)];
            let (same, other) = ((1.0 + width) / 2.0, (1.0 - width) / 2.0);
            [
                [same * gains[0], other * gains[0]],
                [other * gains[1], same * gains[1]],
            ]
        }
        _ => return None,
    })
}

/// A group insert rack as voice-scope processors (Kontakt runs group
/// inserts per voice, before the amplifier), plus what it leaves out.
///
/// Each slot scales its output by the slot's output gain. The slot's dry
/// level is not mixed back: local presets store 1.0 on Stereo Modeller,
/// Inverter and EQ slots used as gain trims (output gains in whole-dB steps),
/// where an added dry path would contradict the trim.
pub(crate) fn group_inserts(slots: &[Slot]) -> (Vec<sampler_ir::Processor>, Vec<Note>) {
    let mut notes = Vec::new();
    let mut combined = IDENTITY;
    let mut filters = Vec::new();
    for fx in slots.iter().filter(|fx| !fx.bypass) {
        let name = module_name(fx.module);
        let mut slot_notes = Vec::new();
        let params = fx.params();
        let wet = f64::from(fx.output_gain);
        let gain = [[wet, 0.0], [0.0, wet]];
        if let Some(Params::Eq { bands }) = &params {
            filters.extend(
                bands
                    .iter()
                    .filter_map(|band| eq_band(*band, &mut slot_notes)),
            );
            combined = product(gain, combined);
            notes.extend(
                slot_notes
                    .into_iter()
                    .map(|(f, v, r)| (fx.slot, format!("{name}: {f}"), v, r)),
            );
            continue;
        }
        match params.as_ref().and_then(|p| matrix(p, &mut slot_notes)) {
            Some(m) => combined = product(product(gain, m), combined),
            // Linear filters applied alike to both channels commute with
            // the matrices, so leaving one out does not reorder the rest.
            None => notes.push((
                fx.slot,
                "effect".into(),
                format!("{name} v{:#x} {params:?}", fx.version),
                sampler_ir::Reason::NotModeled,
            )),
        }
        notes.extend(
            slot_notes
                .into_iter()
                .map(|(f, v, r)| (fx.slot, format!("{name}: {f}"), v, r)),
        );
    }
    // Filters alike on both channels commute with the matrix.
    let mut processors = filters;
    if combined != IDENTITY {
        processors.push(sampler_ir::Processor::StereoMatrix(combined));
    }
    (processors, notes)
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
    fn linear_inserts_fold_into_one_matrix_with_slot_gains() {
        let gainer = slot(0x13, 2.0f32.to_le_bytes().to_vec(), 1.0);
        let inverter = slot(0x1a, vec![1, 1], 0.5);
        let mut modeller = 0.0f32.to_le_bytes().to_vec();
        modeller.extend(0.0f32.to_le_bytes());
        modeller.push(0);
        let modeller = slot(0x1f, modeller, 2.0);
        let (processors, notes) = group_inserts(&[gainer, inverter, modeller]);
        // 2 * (swap, inverted, * 0.5) * 2 = swap, inverted, * 2.
        assert_eq!(
            processors,
            vec![sampler_ir::Processor::StereoMatrix([
                [0.0, -2.0],
                [-2.0, 0.0]
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
        let (processors, _) = group_inserts(&[slot(0x18, eq, 2.0)]);
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
        let (processors, _) = group_inserts(&[slot(0x13, 1.0f32.to_le_bytes().to_vec(), 1.0)]);
        assert!(processors.is_empty());
    }
}
