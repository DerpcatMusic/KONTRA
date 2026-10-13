//! Kontakt `eq` effects onto `sampler_ir::fx::eq`.
//!
//! - Filter 0x18 kind 22 (legacy one-band EQ): exact native law (`Law::KontaktLegacy`).
//! - Filter 0x18 kinds 23/24 (multiband): the native law is unproved (`FilterEQ2/3` classes exist,
//!   topology, stereo and clocks do not), so `mod.rs` keeps the labelled v1/generic fallback.
//! - Solid G-EQ 0x44 (`BParFXSSLGEQ`, reader 0x140d029f0, v0x10 12 fields, v0x11 four more):
//!   saved knobs through the proved spline tables (`Law::Solid`); band topology generic.
use super::{Notes, Reader};
use sampler_ir::fx::eq::{Band, Config, Kind, Law, SolidSection};
use sampler_ir::{Processor, Reason};

/// One legacy EQ band (Hz, octaves, dB); live owners retain flat bands.
pub(super) fn legacy_band(
    [hz, octaves, db]: [f32; 3],
    keep_flat: bool,
    notes: &mut Notes,
) -> Option<Processor> {
    if db == 0. && !keep_flat {
        return None;
    }
    if ![hz, octaves, db].iter().all(|v| v.is_finite()) || hz <= 0. || octaves <= 0. {
        notes.push(("EQ band".into(), format!("{hz} Hz {octaves} oct {db} dB"), Reason::InvalidValue));
        return None;
    }
    Some(Processor::Eq(Config::band(Band {
        law: Law::KontaktLegacy,
        frequency: sampler_core::KontaktLegacyEqSettings::frequency_knob(hz),
        q: sampler_core::KontaktLegacyEqSettings::bandwidth_knob(octaves),
        gain_db: db.clamp(-18., 18.),
        ..Band::new(Kind::Peak, 0., 0., 0.)
    })))
}

/// One band of legacy EQ kinds 23/24 as a generic RBJ peak. GENERIC FALLBACK: the native
/// multiband law is unproved (FilterEQ2/3 class and lifecycle shapes are known; topology, stereo
/// and clocks are not). The gain scale and width->Q mapping are the measured one-band
/// observations (centre gain 2^(3g)), not a multiband law.
pub(super) fn generic_band([hz, octaves, db]: [f32; 3], notes: &mut Notes) -> Option<Band> {
    if db == 0.0 {
        return None;
    }
    if !(hz.is_finite() && octaves.is_finite() && db.is_finite() && hz > 0.0 && octaves > 0.0) {
        notes.push(("EQ band".into(), format!("{hz} Hz {octaves} oct {db} dB"), Reason::InvalidValue));
        return None;
    }
    Some(Band::new(
        Kind::Peak,
        hz,
        sampler_core::PeakingEq::q_of_octaves(f64::from(octaves)) as f32,
        (f64::from(db) * sampler_core::PeakingEq::GAIN_SCALE) as f32,
    ))
}

/// The saved Solid G-EQ knobs, in serialized order (FFBFx8BFFBB).
struct Saved {
    lf: [f32; 2],
    lf_bell: bool,
    lmf: [f32; 3],
    hmf: [f32; 3],
    hf: [f32; 2],
    hf_bell: bool,
    /// v0x11 only: high-pass and low-pass frequency, then their enables (LP first).
    filters: Option<([f32; 2], [bool; 2])>,
}

fn read(version: u16, public: &[u8]) -> Option<Saved> {
    let mut r = Reader(public);
    let lf = [r.f32()?, r.f32()?];
    let lf_bell = r.flag()?;
    let lmf = [r.f32()?, r.f32()?, r.f32()?];
    let hmf = [r.f32()?, r.f32()?, r.f32()?];
    let hf = [r.f32()?, r.f32()?];
    let hf_bell = r.flag()?;
    let filters = match version {
        0x10 => None,
        0x11 => Some(([r.f32()?, r.f32()?], [r.flag()?, r.flag()?])),
        _ => return None,
    };
    r.0.is_empty().then_some(Saved { lf, lf_bell, lmf, hmf, hf, hf_bell, filters })
}

/// Solid G-EQ -> one `Config`. Saved field order (bindings: filters_eq.tsv): LF gain, freq, bell;
/// LMF gain, freq, Q; HMF gain, freq, Q; HF gain, freq, bell; HP freq, LP freq, LP on, HP on.
pub(super) fn solid_geq(version: u16, public: &[u8], notes: &mut Notes) -> Option<Processor> {
    let Some(s) = read(version, public) else {
        notes.push(("Solid G-EQ fields".into(), format!("v{version:#x} len {}", public.len()), Reason::InvalidValue));
        return None;
    };
    let all = s.lf.iter().chain(&s.lmf).chain(&s.hmf).chain(&s.hf).chain(s.filters.iter().flat_map(|f| &f.0));
    if !all.into_iter().all(|v| (0. ..=1.).contains(v)) {
        notes.push(("Solid G-EQ knobs".into(), "outside the normalized range".into(), Reason::InvalidValue));
        return None;
    }
    let band = |kind, section, frequency, q, gain| Band {
        law: Law::Solid(section),
        frequency,
        q,
        gain_db: gain,
        ..Band::new(kind, 0., 0., 0.)
    };
    let mut bands = vec![
        band(if s.lf_bell { Kind::Peak } else { Kind::LowShelf }, SolidSection::LowFrequency, s.lf[1], 0., s.lf[0]),
        band(Kind::Peak, SolidSection::LowMid, s.lmf[1], s.lmf[2], s.lmf[0]),
        band(Kind::Peak, SolidSection::HighMid, s.hmf[1], s.hmf[2], s.hmf[0]),
        band(if s.hf_bell { Kind::Peak } else { Kind::HighShelf }, SolidSection::HighFrequency, s.hf[1], 0., s.hf[0]),
    ];
    if let Some(([hp, lp], [lp_on, hp_on])) = s.filters {
        if hp_on {
            bands.push(band(Kind::HighPass, SolidSection::HighPass, hp, 0., 0.));
        }
        if lp_on {
            bands.push(band(Kind::LowPass, SolidSection::LowPass, lp, 0., 0.));
        }
    }
    notes.push((
        "Solid G-EQ band topology".into(),
        "knob->Hz/Q/dB splines are exact (original-instruction vectors); the biquads they drive are generic RBJ, \
         and shelf slope, Q unit (read as 1/2q), HP/LP order, bank 0 vs 16, smoothing, latency are unproved"
            .into(),
        Reason::UnknownLaw,
    ));
    Config::new(&bands).map(Processor::Eq)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(version: u16) -> Vec<u8> {
        let f = |v: f32| v.to_le_bytes().to_vec();
        let mut b = vec![];
        b.extend(f(0.5)); // LF gain
        b.extend(f(0.5)); // LF freq
        b.push(1); // LF bell
        for v in [0.75, 0.5, 0.5, 0.25, 0.5, 0.5, 0.5, 0.5] {
            b.extend(f(v));
        }
        b.push(0); // HF shelf
        if version == 0x11 {
            b.extend(f(0.5));
            b.extend(f(0.5));
            b.extend([1, 0]); // LP on, HP off
        }
        b
    }

    #[test]
    fn solid_geq_v11_has_four_bands_and_the_enabled_low_pass() {
        let mut notes = vec![];
        let Some(Processor::Eq(c)) = solid_geq(0x11, &saved(0x11), &mut notes) else { panic!() };
        let kinds: Vec<_> = c.bands().iter().map(|b| b.kind).collect();
        assert_eq!(kinds, [Kind::Peak, Kind::Peak, Kind::Peak, Kind::HighShelf, Kind::LowPass]);
        assert_eq!(c.bands()[1].law, Law::Solid(SolidSection::LowMid));
        assert_eq!(c.bands()[1].gain_db, 0.75);
        assert!(!notes.is_empty());
    }

    #[test]
    fn solid_geq_v10_has_no_pass_filters_and_bad_fields_are_reported() {
        let mut notes = vec![];
        let Some(Processor::Eq(c)) = solid_geq(0x10, &saved(0x10), &mut notes) else { panic!() };
        assert_eq!(c.count, 4);
        let mut short = saved(0x11);
        short.pop();
        assert!(solid_geq(0x11, &short, &mut vec![]).is_none());
    }

    #[test]
    fn legacy_band_is_the_native_law() {
        let Some(Processor::Eq(c)) = legacy_band([1000., 1., 9.], false, &mut vec![]) else { panic!() };
        let b = c.bands()[0];
        assert_eq!(b.law, Law::KontaktLegacy);
        assert_eq!(b.frequency, sampler_core::KontaktLegacyEqSettings::frequency_knob(1000.));
        assert!(legacy_band([1000., 1., 0.], false, &mut vec![]).is_none());
    }
}
