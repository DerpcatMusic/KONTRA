//! Effect parameter layouts. Every layout was checked for exact byte length
//! against all local instances; names and confidence are in `audits/EFFECTS.md`.

use super::Kind;
use crate::audio::Sample;
use serde::{Deserialize, Serialize};
use std::{fmt, sync::Arc};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Params {
    Gainer(Gainer),
    SendLevels(SendLevels),
    StereoModeller(StereoModeller),
    Reverb(Reverb),
    Convolution(Box<Convolution>),
    Filter(Filter),
    Eq(Eq),
    /// Layout known, no DSP: named in serialization order.
    Fields(Vec<Field>),
    /// Layout not identified for this kind/length.
    Opaque {
        bytes: usize,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Gainer {
    /// Linear gain (1.0011 and 2.0 locally).
    pub gain: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendLevels {
    /// Linear level into instrument send slot `n`.
    pub sends: Vec<f32>,
    /// A second table of 17 levels, 1.0 everywhere locally; meaning unknown.
    pub outputs: Vec<f32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct StereoModeller {
    /// Stored 0.0 locally; read as offset from 100% width (-1 mono, +1 200%).
    pub spread: f32,
    /// -1 left .. 1 right.
    pub pan: f32,
    pub pseudo_stereo: bool,
}

/// `BParFXFilter` with a filter type: normalized knobs, see `audits/EFFECTS.md`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Filter {
    /// Kontakt's internal filter type id (stored twice).
    pub filter_type: i32,
    /// 0..=1, `$ENGINE_PAR_CUTOFF` / 1e6.
    pub cutoff: f32,
    /// 0..=1, `$ENGINE_PAR_RESONANCE` / 1e6.
    pub resonance: f32,
    /// Further knobs some types store after resonance (normalized; up to
    /// three are kept): drive, formant talk/size... See `audits/EFFECTS.md`.
    #[serde(default)]
    pub extra: [f32; 3],
}

/// `BParFXFilter` with an EQ type (22/23/24 = 1/2/3 bands), stored in
/// physical units.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eq {
    pub bands: Vec<EqBand>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EqBand {
    pub freq_hz: f32,
    pub bandwidth_oct: f32,
    pub gain_db: f32,
}

/// Filter type ids of the 1-, 2- and 3-band EQs.
pub const EQ_TYPES: std::ops::RangeInclusive<i32> = 22..=24;

/// `BParFXGaloisReverb`: Kontakt's modern "Reverb" (`$EFFECT_TYPE_REVERB2`).
/// Ten normalized values in `$ENGINE_PAR_RV2_*` order, then the freeze
/// switch, which presets do not store.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Reverb {
    /// 0 Room, 1 Hall.
    pub room_type: f32,
    pub time: f32,
    pub size: f32,
    pub damping: f32,
    pub modulation: f32,
    pub diffusion: f32,
    pub predelay: f32,
    pub high_cut: f32,
    pub low_shelf: f32,
    pub stereo: f32,
    /// `$ENGINE_PAR_RV2_FREEZE`: 1.0 holds the tail and mutes the input.
    #[serde(default)]
    pub freeze: f32,
}

impl Reverb {
    /// The values most local presets store, for a Reverb a script loads.
    pub const DEFAULT: Self = Self {
        room_type: 0.0,
        time: 0.37,
        size: 0.5,
        damping: 0.5,
        modulation: 0.5,
        diffusion: 0.5,
        predelay: 0.0,
        high_cut: 0.0,
        low_shelf: 0.0,
        stereo: 1.0,
        freeze: 0.0,
    };

    /// Value `i` in `$ENGINE_PAR_RV2_*` order (see the fields).
    pub fn field(&mut self, i: u8) -> Option<&mut f32> {
        Some(match i {
            0 => &mut self.room_type,
            1 => &mut self.time,
            2 => &mut self.size,
            3 => &mut self.damping,
            4 => &mut self.modulation,
            5 => &mut self.diffusion,
            6 => &mut self.predelay,
            7 => &mut self.high_cut,
            8 => &mut self.low_shelf,
            9 => &mut self.stereo,
            10 => &mut self.freeze,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct IrBand {
    pub length_ratio: f32,
    pub low_cut_hz: f32,
    pub high_cut_hz: f32,
}

/// Script values (0..1): predelay, early size, late size. Until the saved
/// early/late boundary is identified, the last size edit stretches the whole IR.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IrSettings {
    pub values: [f32; 3],
    pub size: f32,
}

impl IrSettings {
    pub const DEFAULT: Self = Self { values: [0., 0.5, 0.5], size: 0.5 };
    pub fn from_convolution(p: &Convolution) -> Self {
        let values = [
            ((p.predelay_ms.max(0.) / 2. + 1.).ln() / 151f32.ln()).clamp(0., 1.),
            (p.early.length_ratio - 0.5).clamp(0., 1.),
            (p.late.length_ratio - 0.5).clamp(0., 1.),
        ];
        Self { values, size: values[2] }
    }

    pub fn set(&mut self, field: u8, value: f32) -> bool {
        let Some(v) = self.values.get_mut(field as usize) else { return false };
        let next = value.clamp(0., 1.);
        // Replaying unchanged script values after a rate rebuild must retain
        // which band was last edited for the uniform-size fallback.
        if *v != next && field != 0 { self.size = next; }
        *v = next;
        true
    }

    pub fn predelay_ms(value: f32) -> f32 {
        // Una Corda's authored millisecond table follows the same logarithmic
        // time law as the envelopes, with a 300 ms maximum and 2 ms offset.
        2. * (151f32.powf(value.clamp(0., 1.)) - 1.)
    }

    pub(super) fn apply(self, p: &mut Convolution) {
        p.predelay_ms = Self::predelay_ms(self.values[0]);
        p.early.length_ratio = 0.5 + self.size;
        p.late.length_ratio = p.early.length_ratio;
    }
}

/// `BParFXIRC`. The impulse response is an index into the preset's
/// "other files" table, not an inline path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Convolution {
    pub unknown: [f32; 2],
    pub predelay_ms: f32,
    pub early: IrBand,
    pub late: IrBand,
    pub unknown_9: f32,
    /// Always `[false, true, true, true, false]` locally; meaning unknown.
    pub flags: [bool; 5],
    /// An 8-point curve (x 0..1, y 0..-79 dB), identical in every local preset.
    pub curve_x: Vec<f32>,
    pub curve_db: Vec<f32>,
    pub ir_index: i32,
    pub ir_file: Option<String>,
    pub ir_error: Option<String>,
    #[serde(skip)]
    pub ir: Option<Impulse>,
}

/// Decoded impulse response, shared between slots using the same file.
#[derive(Clone)]
pub struct Impulse(pub Arc<Sample>);

impl fmt::Debug for Impulse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Impulse({} frames @ {} Hz)",
            self.0.frames.len(),
            self.0.rate
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Field {
    pub name: &'static str,
    pub value: Value,
}

/// A [`Field`] read back (from the instrument cache), before its name is
/// matched to its copy in the layouts.
#[derive(Deserialize)]
struct OwnedField {
    name: String,
    value: Value,
}

// By hand: derived, the `&'static str` would need `'de: 'static`.
impl<'de> Deserialize<'de> for Field {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let f = OwnedField::deserialize(d)?;
        let name = (super::kind::TABLE.iter())
            .filter_map(|&(_, kind, _)| layout(kind))
            .flatten()
            .map(|&(name, _)| name)
            .find(|&name| name == f.name)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown effect field {}", f.name)))?;
        Ok(Self { name, value: f.value })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Number(f32),
    Flag(bool),
}

/// A freshly loaded `kind` (`$ENGINE_PAR_EFFECT_TYPE`). Kontakt's own
/// defaults are not stored anywhere; these are the values local presets
/// keep. Kinds without DSP pass audio through.
pub(super) fn defaults(kind: Kind) -> Params {
    let band = IrBand { length_ratio: 1.0, low_cut_hz: 20.0, high_cut_hz: 20_000.0 };
    match kind {
        Kind::Gainer => Params::Gainer(Gainer { gain: 1.0 }),
        Kind::SendLevels => Params::SendLevels(SendLevels { sends: vec![0.0; 8], outputs: Vec::new() }),
        Kind::StereoModeller => Params::StereoModeller(StereoModeller { spread: 0.0, pan: 0.0, pseudo_stereo: false }),
        Kind::Reverb => Params::Reverb(Reverb::DEFAULT),
        // Passes audio through until a script loads an impulse response.
        Kind::Convolution => Params::Convolution(Box::new(Convolution {
            unknown: [-1.0, 0.0],
            predelay_ms: 0.0,
            early: band,
            late: band,
            unknown_9: -1.0,
            flags: [false, true, true, true, false],
            curve_x: Vec::new(),
            curve_db: Vec::new(),
            ir_index: -1,
            ir_file: None,
            ir_error: None,
            ir: None,
        })),
        _ => match (layout(kind), super::blocks::defaults(kind)) {
            (Some(layout), Some(values)) => Params::Fields(
                layout
                    .iter()
                    .zip(values)
                    .map(|(&(name, ty), &v)| Field {
                        name,
                        value: match ty {
                            Ty::F => Value::Number(v),
                            Ty::B => Value::Flag(v >= 0.5),
                        },
                    })
                    .collect(),
            ),
            _ => Params::Opaque { bytes: 0 },
        },
    }
}

pub(super) fn parse(kind: Kind, data: &[u8]) -> Params {
    let mut r = Reader(data);
    let typed = match kind {
        Kind::Gainer => r.f32().map(|gain| Params::Gainer(Gainer { gain })),
        Kind::SendLevels => send_levels(&mut r),
        Kind::StereoModeller => stereo_modeller(&mut r),
        Kind::Convolution => convolution(&mut r),
        Kind::Filter => filter(&mut r),
        Kind::Reverb => r.array().map(|[a, b, c, d, e, f, g, h, i, j]| {
            Params::Reverb(Reverb {
                room_type: a,
                time: b,
                size: c,
                damping: d,
                modulation: e,
                diffusion: f,
                predelay: g,
                high_cut: h,
                low_shelf: i,
                stereo: j,
                freeze: 0.0,
            })
        }),
        _ => layout(kind).and_then(|layout| fields(&mut r, layout)),
    };
    match typed {
        Some(params) if r.0.is_empty() => params,
        _ => Params::Opaque { bytes: data.len() },
    }
}

fn send_levels(r: &mut Reader) -> Option<Params> {
    Some(Params::SendLevels(SendLevels {
        sends: r.list()?,
        outputs: r.list()?,
    }))
}

fn stereo_modeller(r: &mut Reader) -> Option<Params> {
    Some(Params::StereoModeller(StereoModeller {
        spread: r.f32()?,
        pan: r.f32()?,
        pseudo_stereo: r.flag()?,
    }))
}

fn filter(r: &mut Reader) -> Option<Params> {
    let filter_type = r.i32()?;
    (r.i32()? == filter_type).then_some(())?;
    if EQ_TYPES.contains(&filter_type) {
        let bands = (21..filter_type)
            .map(|_| {
                let [freq_hz, bandwidth_oct, gain_db] = r.array()?;
                Some(EqBand { freq_hz, bandwidth_oct, gain_db })
            })
            .collect::<Option<_>>()?;
        return Some(Params::Eq(Eq { bands }));
    }
    let [cutoff, resonance] = r.array()?;
    let mut extra = [0.0; 3];
    for x in extra.iter_mut().take(r.0.len() / 4) {
        *x = r.f32()?;
    }
    Some(Params::Filter(Filter { filter_type, cutoff, resonance, extra }))
}

fn convolution(r: &mut Reader) -> Option<Params> {
    let [
        u0,
        u1,
        predelay_ms,
        e_len,
        e_lo,
        e_hi,
        l_len,
        l_lo,
        l_hi,
        unknown_9,
    ] = r.array()?;
    let mut flags = [false; 5];
    for flag in &mut flags {
        *flag = r.flag()?;
    }
    Some(Params::Convolution(Box::new(Convolution {
        unknown: [u0, u1],
        predelay_ms,
        early: IrBand {
            length_ratio: e_len,
            low_cut_hz: e_lo,
            high_cut_hz: e_hi,
        },
        late: IrBand {
            length_ratio: l_len,
            low_cut_hz: l_lo,
            high_cut_hz: l_hi,
        },
        unknown_9,
        flags,
        curve_x: r.list()?,
        curve_db: r.list()?,
        ir_index: r.i32()?,
        ir_file: None,
        ir_error: None,
        ir: None,
    })))
}

#[derive(Clone, Copy)]
enum Ty {
    F,
    B,
}

fn fields(r: &mut Reader, layout: &[(&'static str, Ty)]) -> Option<Params> {
    layout
        .iter()
        .map(|&(name, ty)| {
            let value = match ty {
                Ty::F => Value::Number(r.f32()?),
                Ty::B => Value::Flag(r.flag()?),
            };
            Some(Field { name, value })
        })
        .collect::<Option<_>>()
        .map(Params::Fields)
}

/// Names follow `$ENGINE_PAR_*` order where the value count matches it;
/// `param_n`/`flag_n` mark positions whose meaning is not established.
fn layout(kind: Kind) -> Option<&'static [(&'static str, Ty)]> {
    use Ty::{B, F};
    Some(match kind {
        Kind::Inverter => &[("flag_0", B), ("flag_1", B)],
        Kind::Delay => &[
            ("time_ms", F),
            ("damping", F),
            ("pan", F),
            ("feedback", F),
            ("time_unit", F),
            ("time_free_ms", F),
            ("param_6", F),
            ("flag_7", B),
        ],
        Kind::Chorus => &[
            ("depth", F),
            ("speed", F),
            ("phase", F),
            ("speed_unit", F),
            ("speed_free", F),
            ("param_5", F),
            ("flag_6", B),
        ],
        Kind::Flanger => &[
            ("depth", F),
            ("speed", F),
            ("phase", F),
            ("feedback", F),
            ("color", F),
            ("speed_unit", F),
            ("speed_free", F),
            ("param_7", F),
            ("flag_8", B),
        ],
        Kind::Phaser => &[
            ("depth", F),
            ("param_1", F),
            ("speed", F),
            ("param_3", F),
            ("speed_unit", F),
            ("speed_free", F),
            ("param_6", F),
            ("flag_7", B),
        ],
        Kind::Compressor => &[
            ("param_0", F),
            ("threshold_db", F),
            ("ratio", F),
            ("attack_ms", F),
            ("release_ms", F),
            ("link", B),
        ],
        // Saturation: `$ENGINE_PAR_SHAPE` (-1..=1 stored) is the first.
        Kind::SurroundPanner => &[("param_0", F), ("param_1", F)],
        // Input gain (dB) and release (ms): ANALOG STRINGS stores 0.0005 and 10 (medium).
        Kind::Limiter => &[("in_gain_db", F), ("release_ms", F)],
        Kind::Distortion => &[("param_0", F), ("drive", F), ("damping", F)],
        Kind::LoFi => &[
            ("bits", F),
            ("frequency", F),
            ("noise_level", F),
            ("flag_3", B),
            ("noise_color", F),
        ],
        Kind::Skreamer => &[
            ("tone", F),
            ("drive", F),
            ("bass", F),
            ("bright", F),
            ("mix", F),
        ],
        Kind::Rotator => &[
            ("speed", F),
            ("balance", F),
            ("accel_hi", F),
            ("accel_lo", F),
            ("distance", F),
            ("mix", F),
        ],
        Kind::TapeSaturator => &[
            ("gain", F),
            ("warmth", F),
            ("hf_rolloff", F),
            ("quality", B),
        ],
        Kind::TransientMaster => &[("input", F), ("attack", F), ("sustain", F), ("smooth", F)],
        Kind::SolidGeq => &[
            ("lf_gain", F),
            ("lf_freq", F),
            ("lf_bell", B),
            ("lmf_gain", F),
            ("lmf_freq", F),
            ("lmf_q", F),
            ("hmf_gain", F),
            ("hmf_freq", F),
            ("hmf_q", F),
            ("hf_gain", F),
            ("hf_freq", F),
            ("hf_bell", B),
        ],
        Kind::SolidBusComp => &[
            ("threshold", F),
            ("ratio", F),
            ("attack", F),
            ("release", F),
            ("makeup", F),
            ("mix", F),
            ("link", B),
            ("flag_7", B),
            ("param_8", F),
        ],
        Kind::FeedbackCompressor => &[
            ("input", F),
            ("ratio", F),
            ("attack", F),
            ("release", F),
            ("makeup", F),
            ("mix", F),
            ("param_6", F),
            ("hq_mode", B),
            ("link", B),
            ("flag_9", B),
        ],
        _ => return None,
    })
}

/// Field names of `kind`'s layout.
#[cfg(test)]
pub(crate) fn layout_names(kind: Kind) -> Option<Vec<&'static str>> {
    Some(layout(kind)?.iter().map(|(name, _)| *name).collect())
}

/// Little-endian cursor; `None` on truncation.
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.0.split_first_chunk::<N>()?;
        self.0 = rest;
        Some(*head)
    }

    fn f32(&mut self) -> Option<f32> {
        self.take().map(f32::from_le_bytes)
    }

    fn i32(&mut self) -> Option<i32> {
        self.take().map(i32::from_le_bytes)
    }

    fn flag(&mut self) -> Option<bool> {
        self.take::<1>().map(|[b]| b != 0)
    }

    fn array<const N: usize>(&mut self) -> Option<[f32; N]> {
        let mut out = [0.0; N];
        for v in &mut out {
            *v = self.f32()?;
        }
        Some(out)
    }

    /// `u32` count followed by that many `f32`s.
    fn list(&mut self) -> Option<Vec<f32>> {
        let n = u32::from_le_bytes(self.take()?) as usize;
        if n > self.0.len() / 4 {
            return None;
        }
        (0..n).map(|_| self.f32()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floats(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn typed_layouts_require_exact_length() {
        let Params::Gainer(g) = parse(Kind::Gainer, &floats(&[2.0])) else {
            panic!("gainer")
        };
        assert_eq!(g.gain, 2.0);
        assert!(matches!(
            parse(Kind::Gainer, &floats(&[2.0, 1.0])),
            Params::Opaque { bytes: 8 }
        ));
        assert!(matches!(
            parse(Kind::Reverb, &floats(&[0.5; 9])),
            Params::Opaque { .. }
        ));
    }

    #[test]
    fn send_levels_and_convolution() {
        let mut data = 2u32.to_le_bytes().to_vec();
        data.extend(floats(&[0.25, 1.0]));
        data.extend(1u32.to_le_bytes());
        data.extend(floats(&[1.0]));
        let Params::SendLevels(s) = parse(Kind::SendLevels, &data) else {
            panic!("send levels")
        };
        assert_eq!((s.sends, s.outputs), (vec![0.25, 1.0], vec![1.0]));

        let mut data = floats(&[-1.0, 0.0, 40.0, 1.0, 20.0, 20e3, 1.0, 20.0, 20e3, -1.0]);
        data.extend([0, 1, 1, 1, 0]);
        data.extend(1u32.to_le_bytes());
        data.extend(floats(&[0.0]));
        data.extend(1u32.to_le_bytes());
        data.extend(floats(&[-40.0]));
        data.extend(3i32.to_le_bytes());
        let Params::Convolution(c) = parse(Kind::Convolution, &data) else {
            panic!("convolution")
        };
        assert_eq!((c.predelay_ms, c.ir_index), (40.0, 3));
        assert_eq!(c.early.high_cut_hz, 20e3);
        // A lying count must not over-read.
        let mut bad = data.clone();
        bad[45] = 0xff;
        assert!(matches!(
            parse(Kind::Convolution, &bad),
            Params::Opaque { .. }
        ));
    }

    #[test]
    fn field_layouts_decode_names() {
        let mut data = floats(&[0.0, -24.0, 0.25, 50.0, 300.0]);
        data.push(1);
        let Params::Fields(f) = parse(Kind::Compressor, &data) else {
            panic!("compressor")
        };
        assert_eq!(f[1].name, "threshold_db");
        assert!(matches!(f[5].value, Value::Flag(true)));
        assert!(matches!(parse(Kind::Twang, &data), Params::Opaque { .. }));
    }
}
