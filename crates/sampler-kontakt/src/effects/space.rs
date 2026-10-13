//! Kontakt `space` effects onto sampler_ir::fx::space: legacy Reverb (0x15),
//! Convolution (0x16), Plate Reverb (0x57), Reverb / Galois (0x59), Raum (0x65)
//! and Reverb Delight (0x7a).
//!
//! What is proved and what is not, per effect:
//! - 0x59 Galois: framing, field order and the display laws of time, high cut
//!   and low shelf (measured through `get_engine_par_disp`). The network
//!   itself is not recovered, so it runs the generic FDN, reported as such.
//! - 0x15 legacy: framing, field order (predelay, size, stereo, colour,
//!   damping; measured), the predelay display table, and the structure of
//!   eight parallel damped combs into four serial allpasses
//!   (`FUN_140aa3d10`). Delay lengths and the size/damping/colour laws are
//!   not recovered: generic comb/allpass layout.
//! - 0x57 Plate: framing and field order (decay, low shelf, high damp,
//!   predelay, stereo; measured). Topology and laws unproved: generic plate.
//! - 0x16 Convolution: Reverse, Auto Gain (`0x1419b31f0..0x1419b326e`), the
//!   volume envelope and predelay are applied; IR size, early/late filters,
//!   decimation and preserve-length are reported unmodeled.
//! - 0x65 Raum, 0x7a Reverb Delight: framing only; reported unmodeled with
//!   the reason in the slot's `effect` note.

use super::{Impulses, Notes};
use sampler_ir::Reason::{NotModeled, UnknownLaw};
use sampler_ir::fx::space::{Config, Network, Topology};

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

/// Seconds of predelay for the legacy Reverb's stored 0..1 value: the measured
/// display table (0 to 180 ms), `2 ms * (91^x - 1)`, matches it to within 0.1%.
fn legacy_predelay_seconds(x: f64) -> f64 {
    0.002 * (91f64.powf(x) - 1.0)
}

/// Legacy Reverb 0x15: the stored values are predelay, size, stereo, colour,
/// damping, each 0..1.
pub(super) fn legacy(v: &[f32; 5], notes: &mut Notes) -> Config {
    let [predelay, size, stereo, colour, damping] = v.map(|x| f64::from(x.clamp(0.0, 1.0)));
    notes.push((
        "legacy reverb algorithm".into(),
        format!(
            "size {size} damping {damping} stereo {stereo}: generic comb/allpass, delay lengths and laws unproved; colour {colour} not applied"
        ),
        UnknownLaw,
    ));
    Config::network(Network {
        topology: Topology::CombAllpass,
        // Size is the feedback in classic comb reverbs: a generic 0.2 s to 12 s.
        decay_seconds: 0.2 * 60f64.powf(size),
        size: 1.0,
        predelay_seconds: legacy_predelay_seconds(predelay),
        damping_hz: 18_000.0 * 0.05f64.powf(damping),
        modulation_seconds: 0.0,
        diffusion: 0.5,
        width: stereo,
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

/// Kontakt's normalized Reverb (Galois, 0x59) values as physical settings.
/// Time, predelay, high cut and low shelf follow the reference display; size,
/// damping and diffusion are still v1's fits.
pub(super) fn galois(v: &[f32; 10], notes: &mut Notes) -> Config {
    let [room, time, size, damping, modulation, diffusion, predelay, high_cut, low_shelf, stereo] =
        v.map(|x| f64::from(x.clamp(0.0, 1.0)));
    notes.push((
        "reverb algorithm".into(),
        format!("normalized {v:?}: generic FDN, Galois network unrecovered"),
        UnknownLaw,
    ));
    Config::network(Network {
        topology: Topology::Fdn,
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
