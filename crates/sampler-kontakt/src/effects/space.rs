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
//! - 0x16 Convolution: Reverse, early/late filters and composition, the
//!   volume envelope, predelay and the wrapper's Auto Gain (`0x140ab6740`) are
//!   applied; IR size and decimation (unextracted resamplers) are reported
//!   unmodeled.
//! - 0x65 Raum, 0x7a Reverb Delight: field bindings measured, cores not
//!   extracted; reported unmodeled with the reason in the slot's `effect` note.

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
                "early {} late {}: Kontakt resamples the response by the size (0x1419b3450) through a band-limited resampler whose kernel (0x1419be690) is not extracted",
                c.early[0], c.late[0]
            ),
            NotModeled,
        ));
    }
    if c.decimation > 1.0 {
        notes.push((
            "IR sample-rate decimation".into(),
            format!("{}: the wet path's down/up resampler pair (0x140a84580) is not extracted", c.decimation),
            NotModeled,
        ));
    }
    let mut left: Vec<f32> = frames.iter().map(|f| f[0]).collect();
    let mut right: Vec<f32> = frames.iter().map(|f| f[1]).collect();
    if c.flags[0] {
        left.reverse();
        right.reverse();
    }
    // Saved [1] is the low cut and [2] the high cut: the corpus's defaults
    // are 20 Hz and 20 kHz there.
    let filtered = |x: &[f32], part: &[f32; 3]| {
        let mut y = x.to_vec();
        butterworth(&mut y, rate, part[2], false);
        butterworth(&mut y, rate, part[1], true);
        y
    };
    if c.early[1..] != c.late[1..] {
        let len = left.len();
        let at = (c.xpoint >= 0.0).then(|| crossover(len, c.xpoint, rate));
        match at {
            Some(x) => {
                left = compose(&filtered(&left, &c.early), &filtered(&left, &c.late), x, rate);
                right = compose(&filtered(&right, &c.early), &filtered(&right, &c.late), x, rate);
            }
            None => {
                notes.push((
                    "IR early/late crossover".into(),
                    format!("crossover {} unset: the native default position is not read; late filters applied throughout", c.xpoint),
                    NotModeled,
                ));
                left = filtered(&left, &c.late);
                right = filtered(&right, &c.late);
            }
        }
    } else {
        left = filtered(&left, &c.late);
        right = filtered(&right, &c.late);
    }
    let mut shape = sampler_core::neutral_impulse_shape();
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
    let (mut left, mut right) = sampler_core::shape_impulse(rate, &left, &right, &shape);
    // Wrapper predelay `(int)((ps·ms)·0.001)`; Auto Gain's f32 energy sum
    // (which includes these zeros) is unchanged by them.
    let pre = ((rate as f32 * c.predelay_ms.max(0.0)) * 0.001) as usize;
    for ch in [&mut left, &mut right] {
        ch.splice(0..0, std::iter::repeat_n(0.0, pre));
    }
    impulses.store.push(sampler_ir::Impulse { rate, left, right, asset: None });
    if let Some(recipes) = impulses.recipes.as_deref_mut() {
        recipes.push(c.clone());
    }
    Ok(sampler_ir::ImpulseRef(impulses.store.len() - 1))
}

/// CConvolver's early/late filter (`0x1419b27a0`, coefficients `0x1419b7fb0`,
/// tick `0x1419b77f0`): a bilinear 2nd-order Butterworth low-pass (bypassed
/// above 0.45 of the rate) or high-pass (bypassed below 0.01), one causal pass.
fn butterworth(x: &mut [f32], rate: u32, hz: f32, high_pass: bool) {
    let w = hz / rate as f32;
    if (!high_pass && w > 0.45) || (high_pass && w < 0.01) || !w.is_finite() {
        return;
    }
    const SQRT2: f32 = 1.414_213_5;
    let k = (w * 3.141_592_7).tan();
    let k2 = k * k;
    let n = 1.0 / (SQRT2 * k + 1.0 + k2);
    let b = if high_pass { [n, -2.0 * n, n] } else { [k2 * n, 2.0 * k2 * n, k2 * n] };
    let a = [-(2.0 * (k2 - 1.0) * n), -((1.0 - SQRT2 * k + k2) * n)];
    let (mut x1, mut x2, mut y1, mut y2) = (0f32, 0f32, 0f32, 0f32);
    for v in x {
        let y = b[0] * *v + b[1] * x1 + b[2] * x2 + 1e-24 + a[0] * y1 + a[1] * y2;
        (x2, x1, y2, y1) = (x1, *v, y1, y);
        *v = y;
    }
}

/// The crossover sample (`0x1419b63d0`, then clamped by `0x1419b2870`), at
/// unit decimation.
fn crossover(len: usize, x: f32, rate: u32) -> usize {
    let r = rate as f32;
    ((((len as f32 * x) / r) * r + 0.5) as usize).min(len)
}

/// CConvolver's early/late composition (`0x1419b1e30`) at unit sizes: early
/// up to the crossover, late after it, a cos² crossfade 50 ms wide.
fn compose(early: &[f32], late: &[f32], x: usize, rate: u32) -> Vec<f32> {
    let len = early.len();
    let w = ((rate as f32 * 0.05 + 0.5) as i64) as f32;
    let h = w / 2.0;
    let start = (x as f32 - h + 0.5) as i64;
    let end = ((x as f32 + h + 0.5) as i64).min(len as i64).max(0) as usize;
    let first = start.max(0) as usize;
    let mut out: Vec<f32> = early[..end].iter().chain(&late[end..]).copied().collect();
    for (n, k) in (first..end).zip(first as i64 - start..) {
        let c = (k as f32 * (std::f32::consts::FRAC_PI_2 / w)).cos();
        let wgt = c * c;
        out[n] = (1.0 - wgt) * late[n] + wgt * out[n];
    }
    out
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
        0x65 => ": Raum field binding measured (fx_raum_delight_psyche_vibrato_phasis_replika.tsv); its modern reverb core is not extracted",
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

    /// CConvolver's filter vectors (`0x1419b7fb0`/`0x1419b77f0`, 48 kHz, 4.8 kHz).
    #[test]
    fn early_late_filters_match_native_vectors() {
        let run = |hp| {
            let mut x = [1.0, 0.0, 0.0, 0.0];
            butterworth(&mut x, 48_000, 4_800.0, hp);
            x
        };
        let close = |a: [f32; 4], b: [f32; 4]| a.iter().zip(&b).all(|(a, b)| (a - b).abs() < 1e-6);
        assert!(close(run(false), [0.067_455_28, 0.212_010_62, 0.281_933_64, 0.234_726_31]), "{:?}", run(false));
        assert!(close(run(true), [0.638_945_5, -0.547_588_8, -0.250_695_56, -0.060_494_572]), "{:?}", run(true));
        let mut x = [1.0, 0.5];
        butterworth(&mut x, 44_100, 20_000.0, false);
        butterworth(&mut x, 44_100, 20.0, true);
        assert_eq!(x, [1.0, 0.5], "both bypass at their defaults");
    }

    /// `0x1419b1e30` vector: rate 40, ten samples, crossover 5.
    #[test]
    fn composition_crossfades_early_into_late() {
        let out = compose(&[1.0; 10], &[0.0; 10], crossover(10, 0.5, 40), 40);
        assert_eq!(out, [1.0, 1.0, 1.0, 1.0, 1.0, 0.499_999_97, 0.0, 0.0, 0.0, 0.0]);
    }
}
