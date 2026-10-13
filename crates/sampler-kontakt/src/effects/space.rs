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
//! - 0x57 Plate: framing and field order (decay, low shelf, high damp,
//!   predelay, stereo; measured). Topology and laws unproved: generic plate.
//! - 0x16 Convolution: Reverse, Auto Gain (`0x1419b31f0..0x1419b326e`), the
//!   volume envelope and predelay are applied; IR size, early/late filters,
//!   decimation and preserve-length are reported unmodeled.
//! - 0x65 Raum, 0x7a Reverb Delight: framing only; reported unmodeled with
//!   the reason in the slot's `effect` note.

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

/// The slot's impulse response, shaped as Kontakt's controls ask, in `impulses`.
/// What it cannot shape is reported in `notes`.
pub(super) fn convolution(
    c: &Convolution,
    impulses: &mut Impulses,
    notes: &mut Notes,
) -> Result<sampler_ir::ImpulseRef, String> {
    let (rate, frames) = (impulses.load)(c.ir_index)?;
    if frames.is_empty() || rate == 0 {
        return Err("the impulse response is empty".into());
    }
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
        notes.push(("IR sample-rate decimation".into(), c.decimation.to_string(), NotModeled));
    }
    let mut shape = sampler_core::neutral_impulse_shape();
    shape.reverse = c.flags[0];
    shape.auto_gain = c.flags[1];
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
    let left: Vec<f32> = frames.iter().map(|f| f[0]).collect();
    let right: Vec<f32> = frames.iter().map(|f| f[1]).collect();
    let (mut left, mut right) = sampler_core::shape_impulse(rate, &left, &right, &shape);
    // Predelay truncates to whole frames and is placed before Auto Gain's
    // energy sum, which zeros do not change.
    let pre = (c.predelay_ms.max(0.0) * 0.001 * rate as f32) as usize;
    for ch in [&mut left, &mut right] {
        ch.splice(0..0, std::iter::repeat_n(0.0, pre));
    }
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

/// Plate Reverb 0x57: decay, low shelf, high damp, predelay, stereo, each 0..1.
pub(super) fn plate(v: &[f32; 5], notes: &mut Notes) -> Config {
    let [decay, low_shelf, damping, predelay, stereo] = v.map(|x| f64::from(x.clamp(0.0, 1.0)));
    notes.push((
        "plate reverb algorithm".into(),
        format!("normalized {v:?}: generic plate, topology and laws unproved"),
        UnknownLaw,
    ));
    Config::network(Network {
        topology: Topology::Plate,
        decay_seconds: 0.82 * 0.5 * 40.4f64.powf(decay),
        size: 1.0,
        predelay_seconds: predelay * 0.25,
        damping_hz: 18_000.0 * 0.05f64.powf(damping),
        modulation_seconds: 0.0005,
        low_shelf_db: -12.0 * low_shelf,
        width: stereo,
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

/// Why a reverb with a recovered reader but no recovered parameters is not
/// translated; empty for every other module.
pub(super) fn why_unbound(module: u16) -> &'static str {
    match module {
        0x65 => ": Raum fields unbound (probe: set each ENGINE_PAR_RAUM_* alone and save)",
        0x7a => ": Reverb Delight fields unbound (amp, tremolo and spring-reverb controls)",
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
}
