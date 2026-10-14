//! Kontakt `space` effects onto sampler_ir::fx::space: legacy Reverb (0x15),
//! Convolution (0x16), Plate Reverb (0x57), Reverb / Galois (0x59), Raum (0x65)
//! and Reverb Delight (0x7a).
//!
//! What is proved and what is not, per effect:
//! - 0x59 Galois: the native core (`sampler_core` `dsp/space/galois.rs`),
//!   verified against the emulated kernel; only its unsaved dry/wet lane is
//!   assumed (fully wet).
//! - 0x15 legacy: exact network and setter laws (`FUN_140aa3d10`,
//!   `FUN_14094c760`, case 0x3e of `FUN_1409619b0`): Freeverb delay lengths,
//!   feedback, damping, channel mix, predelay in ms. Only the input lowpass's
//!   cutoff-to-pole law is stood in (reported as an UnknownLaw note).
//! - 0x57 Plate: the native core (`sampler_core` `dsp/space/design/plate.rs`),
//!   matched to the emulated core's impulse response.
//! - 0x16 Convolution: Reverse, early/late filters and composition, the
//!   volume envelope, predelay and the wrapper's Auto Gain (`0x140ab6740`) are
//!   applied; IR size (an STFT time-stretch engine) and S.Rate (IR
//!   preparation at the inner rate untraced) are reported unmodeled.
//! - 0x65 Raum: Grounded and Airy drive the native core ([`Topology::Raum`],
//!   `sampler_core` `dsp/space/design/raum.rs`); Cosmic is reported unmodeled.
//! - 0x7a Reverb Delight: field binding measured, cores not extracted;
//!   reported unmodeled with the reason in the slot's `effect` note.

use super::{Impulses, Notes};
use sampler_ir::Reason::{NotModeled, UnknownLaw};
use sampler_ir::fx::space::{Config, Freeverb, Galois, Network, Topology};

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
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

/// The slot's impulse response, prepared as Kontakt's wrapper does
/// (`0x140abd100` → `0x140ab5e00`): reverse, early/late filters, early/late
/// composition, volume envelope, predelay, Auto Gain. The host rate of the
/// native preparation is taken to be the file's rate (Kontakt first
/// resamples the file to the host rate; filter cutoffs keep their Hz).
/// What it cannot prepare is reported in `notes`.
pub(super) fn convolution(
    c: &Convolution,
    impulses: &mut Impulses,
    notes: &mut Notes,
) -> Result<sampler_ir::ImpulseRef, String> {
    let (rate, frames) = (impulses.load)(c.ir_index)?;
    if frames.is_empty() || rate == 0 {
        return Err("the impulse response is empty".into());
    }
    // 0x1419b6750 accepts sizes in [0.5, 1.5] only; others leave 1.
    let size = |s: f32| if (0.5..=1.5).contains(&s) { s } else { 1.0 };
    if size(c.early[0]) != 1.0 || size(c.late[0]) != 1.0 {
        notes.push((
            "IR size".into(),
            format!(
                "early {} late {}: Kontakt time-stretches the response by the size without changing its pitch (0x1419b3450 drives an STFT time-stretch engine, setup 0x1419be690, 0x1419baf70..); the engine is not extracted",
                c.early[0], c.late[0]
            ),
            NotModeled,
        ));
    }
    if c.decimation > 1.0 {
        notes.push((
            "IR sample-rate decimation".into(),
            format!("{}: the wet path runs at (int)(rate/S.Rate) between a resampler pair (0x140a84580; the Kaiser law is in sampler-core space/resample.rs), but how the IR is prepared at that inner rate (the IR-rate rescale under flag +0x354) is not traced", c.decimation),
            NotModeled,
        ));
    }
    let left: Vec<f32> = frames.iter().map(|f| f[0]).collect();
    let right: Vec<f32> = frames.iter().map(|f| f[1]).collect();
    // Saved [1] is the low cut and [2] the high cut: the corpus's defaults
    // are 20 Hz and 20 kHz there.
    let crossover = (c.xpoint >= 0.0).then_some(c.xpoint);
    if c.early[1..] != c.late[1..] && crossover.is_none() {
        notes.push((
            "IR early/late crossover".into(),
            format!("crossover {} unset: the native default position is not read; late filters applied throughout", c.xpoint),
            NotModeled,
        ));
    }
    let mut shape = sampler_core::neutral_impulse_shape();
    shape.reverse = c.flags[0];
    shape.filters = Some(sampler_ir::ImpulseFilters { early: [c.early[1], c.early[2]], late: [c.late[1], c.late[2]], crossover });
    shape.auto_gain = c.flags[1];
    // Wrapper predelay `(int)((ps·ms)·0.001)`, after Auto Gain.
    (shape.pre_delay_ms, shape.truncate_pre_delay) = (f64::from(c.predelay_ms), true);
    if c.flags[4] {
        if let (Ok(x), Ok(db)) = (
            <[f32; 8]>::try_from(c.curve_x.as_slice()),
            <[f32; 8]>::try_from(c.curve_db.as_slice()),
        ) {
            shape.envelope = Some(std::array::from_fn(|i| [x[i], db[i]]));
        } else {
            notes.push(("IR volume envelope".into(), "not eight knots".into(), NotModeled));
        }
    }
    let (left, right) = sampler_core::shape_impulse(rate, &left, &right, &shape);
    impulses.store.push(sampler_ir::Impulse { rate, left, right, asset: None });
    if let Some(recipes) = impulses.recipes.as_deref_mut() {
        recipes.push(c.clone());
    }
    Ok(sampler_ir::ImpulseRef(impulses.store.len() - 1))
}

/// Legacy Reverb 0x15: the stored values are predelay (milliseconds, 0 to 180),
/// size, stereo, colour, damping, each 0..1. The setter (case 0x3e of
/// `FUN_1409619b0`) turns them into: comb feedback `size*0.3 + 0.65`, comb
/// damping coefficient `1 - (1-colour)*0.35`, channel-mix weight
/// `(stereo+1)/2`, an input lowpass at normalized cutoff
/// `(1-damping)*0.75 + 0.25`, fixed allpass gain 0.5 and input gain 0.015.
pub(super) fn legacy(v: &[f32; 5], notes: &mut Notes) -> Config {
    let predelay_ms = f64::from(v[0].clamp(0.0, 180.0));
    let [_, size, stereo, colour, damping] = v.map(|x| x.clamp(0.0, 1.0));
    let feedback = size * 0.3 + 0.65;
    let damp = 1.0 - (1.0 - colour) * 0.35;
    let width = (stereo + 1.0) * 0.5;
    let cutoff = (1.0 - damping) * 0.75 + 0.25;
    notes.push((
        "legacy reverb input lowpass".into(),
        format!(
            "normalized cutoff {cutoff}: Kontakt runs a shared biquad-prototype filter whose prototype coefficients are not in the static image; a one-pole at 20 Hz * 1000^cutoff stands in"
        ),
        UnknownLaw,
    ));
    Config::network(Network {
        topology: Topology::Freeverb,
        predelay_seconds: predelay_ms * 0.001,
        input_cutoff_hz: 20.0 * 1000f64.powf(f64::from(cutoff)),
        freeverb: Some(Freeverb {
            feedback: f64::from(feedback),
            damp: f64::from(damp),
            allpass: 0.5,
            input_gain: f64::from(0.015f32),
            width: f64::from(width),
        }),
        ..Network::DEFAULT
    })
}

/// Plate Reverb 0x57: decay, low shelf, high damp, predelay, stereo, each
/// 0..1, drive the native core ([`Topology::KontaktPlate`], `sampler_core`
/// `dsp/space/design/plate.rs`). The physical fields only describe it.
pub(super) fn plate(v: &[f32; 5], _notes: &mut Notes) -> Config {
    let c = v.map(|x| x.clamp(0.0, 1.0));
    Config::network(Network {
        topology: Topology::KontaktPlate,
        decay_seconds: f64::from(2.993 * c[0] + 1.007),
        predelay_seconds: f64::from(c[3]) * 0.25,
        width: f64::from(c[4]),
        kontakt_plate: Some(c),
        ..Network::DEFAULT
    })
}

/// Kontakt Reverb (Galois, 0x59): the saved type (1 Hall, 0 Room) and nine
/// normalized controls drive the native core ([`Topology::Galois`]). The
/// physical fields are filled for display and tail length only: decay
/// `0.5·e^(3.7·time)` s (setter `0x141aea370` case 0), predelay
/// `11025·x` samples at the core's 44.1 kHz.
///
/// The core's own dry/wet control (dispatcher lane 8) has no saved field and
/// no binding found; it is set fully wet, so the slot's Return/Dry carry the
/// mix (probe: send a unit impulse through a default Reverb insert and read
/// the direct-path level).
pub(super) fn galois(v: &[f32; 10], _notes: &mut Notes) -> Config {
    let [kind, time, size, damping, modulation, diffusion, predelay, high_cut, low_shelf, stereo] = v.map(|x| x.clamp(0.0, 1.0));
    Config::network(Network {
        topology: Topology::Galois,
        decay_seconds: f64::from((time * 3.7).exp() * 0.5),
        predelay_seconds: f64::from(predelay) * 0.25,
        galois: Some(Galois {
            hall: kind >= 0.5,
            controls: [time, size, damping, modulation, diffusion, predelay, high_cut, low_shelf, 1.0, stereo],
        }),
        ..Network::DEFAULT
    })
}

/// Raum v0x10 after its type enum (reader `0x140ac5cc0`): floats predelay
/// (+8), feedback, low shelf, high cut, reverb, diffusion, decay, mod, size,
/// damping, rate, mix (+0x34); bytes freeze, sparse; the predelay time
/// object's unit (+0x60) and two floats; a byte. Returns the core controls
/// (apply `0x140abb190`) and whether the predelay is tempo-synced.
pub(super) fn read_raum(r: &mut super::Reader) -> Option<([f32; 20], bool)> {
    let kind = r.i32()? as f32;
    let mut f = [0f32; 12];
    for x in &mut f {
        *x = r.f32()?;
    }
    let (freeze, sparse) = (r.flag()?, r.flag()?);
    let unit = r.f32()?;
    r.f32()?;
    r.f32()?;
    r.flag()?;
    let [predelay, feedback, low_shelf, high_cut, reverb, diffusion, decay, modulation, size, damping, rate, mix] = f;
    // Field ms → core 1: raw up to 1, else `√(0.0125t)` below 80 ms or
    // `√((t - 80)·0.00052083) + 1`, halved and capped at 1.
    let t = predelay;
    let pre = if t <= 1.0 {
        t
    } else {
        let s = if t < 80.0 { (f32::from_bits(0x3c4c_cccd) * t).sqrt() } else { ((t - 80.0) * f32::from_bits(0x3a08_8889)).sqrt() + 1.0 };
        (s / 2.0).min(1.0)
    };
    let b = |v: bool| if v { 1.0 } else { 0.0 };
    let core = [mix, pre, 0.0, 0.25, 0.0625, 0.0, feedback, high_cut, low_shelf, kind * 0.5, decay, size, damping, modulation, rate, diffusion, b(freeze), reverb, b(sparse), 1.0];
    Some((core.map(|x| if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 }), unit > f32::from_bits(0x3089_705f)))
}

/// Raum (0x65) Grounded or Airy on the native core; its own mix law carries
/// the dry path. Input gain (core 19) has no saved field and stays 1.
pub(super) fn raum(core: &[f32; 20], synced: bool, notes: &mut Notes) -> Config {
    if synced {
        notes.push(("predelay sync".into(), "Raum's tempo-synced predelay (core 2..5, `240·n/(f·m·bpm)`) is not bound; the saved time is read as milliseconds".into(), UnknownLaw));
    }
    if core[6] > 0.0 {
        notes.push(("feedback limiter".into(), "Raum's feedback limiter (NI::DSP::Limiter `0x141ae3ba0`) is reduced to its ±2.5 clamp: its gain table is not extracted".into(), UnknownLaw));
    }
    let x = f64::from(core[1]);
    Config::network(Network {
        topology: Topology::Raum,
        decay_seconds: 199.75 * f64::from(core[10]) / (75.0 * (1.0 - f64::from(core[10])) + 1.0) + 0.25,
        predelay_seconds: if x <= 0.5 { 0.08 * (2.0 * x).powi(2) } else { 1.92 * (2.0 * x - 1.0).powi(2) + 0.08 },
        raum: Some(*core),
        ..Network::DEFAULT
    })
}

/// Why a reverb with a recovered reader but no recovered parameters is not
/// translated; empty for every other module.
pub(super) fn why_unbound(module: u16) -> &'static str {
    match module {
        0x65 => ": Raum Cosmic: its eight-stage allpass core is not built (Grounded and Airy are)",
        0x7a => ": Reverb Delight field binding measured (MONO unproved: field 11 bytes unstable); its amp, tremolo and spring-reverb cores are not extracted",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_predelay_is_stored_milliseconds() {
        let mut notes = Notes::new();
        let c = legacy(&[90.0, 0.5, 1.0, 0.5, 0.5], &mut notes);
        let n = c.network.unwrap();
        assert!((n.predelay_seconds - 0.09).abs() < 1e-12);
        assert_eq!(notes.len(), 1);
        // size .5 -> 0.8, colour .5 -> 1 - .5*.35, stereo 1 -> width 1.
        let f = n.freeverb.unwrap();
        assert!((f.feedback - 0.8).abs() < 1e-6 && (f.damp - 0.825).abs() < 1e-6 && f.width == 1.0);
    }

    #[test]
    fn legacy_reverb_lanes_round_trip_through_the_setter() {
        let mut c = legacy(&[90.0, 0.5, 0.25, 0.5, 0.75], &mut Notes::new());
        c.address = Some(sampler_ir::SlotAddress { group: -1, slot: 7, generic: 1003 });
        let lanes = c.lanes();
        let stored: Vec<f64> = lanes.iter().map(|l| (l.value * 1e4).round() / 1e4).collect();
        assert_eq!(stored, [90.0, 0.5, 0.25, 0.5, 0.75]);
        let mut rebuilt = c;
        for lane in &lanes {
            rebuilt.set(lane.role, lane.value);
        }
        let (a, b) = (rebuilt.network.unwrap(), c.network.unwrap());
        assert!((a.predelay_seconds - b.predelay_seconds).abs() < 1e-9 && (a.input_cutoff_hz / b.input_cutoff_hz - 1.).abs() < 1e-5);
        assert_eq!(a.freeverb, b.freeverb);
        // RV_SIZE at 1: comb feedback 0.95.
        rebuilt.set(sampler_ir::ProcessorParameter::Size, 1.0);
        assert!((rebuilt.network.unwrap().freeverb.unwrap().feedback - 0.95).abs() < 1e-6);
    }
}
