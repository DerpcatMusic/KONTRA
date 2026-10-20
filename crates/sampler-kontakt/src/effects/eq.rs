//! Kontakt `eq` effects onto `sampler_ir::fx::eq`.
//!
//! - Filter 0x18 kind 22 (legacy one-band EQ): exact native law (`Law::KontaktLegacy`).
//! - Filter 0x18 kinds 23/24 (`BEQ2Band` / `BEQ3Band`, vtables 0x144f137e8 / 0x144f13a48): PROVED
//!   serial. Scalar process 0x140aeb240 / 0x140aeb8e0 (cascades 0x140ae87a0 / 0x140ae9c00) feed
//!   band n's DF1 output into band n+1, per channel, coefficients shared by all channels; a band
//!   whose gain is within ±9.54e-7 dB and unmodulated is skipped. Wrappers 0x140904780 /
//!   0x140904790 (cadence 0x140a6f2f0) run each band's own one-band smoothers and coefficient
//!   generator 0x1409591c0 on one shared 32-frame counter. So one `legacy_band` processor per
//!   band, in band order, is the native topology; flat bands are dropped as natively.
//! - Solid G-EQ 0x44 (`BParFXSSLGEQ`, reader 0x140d029f0, v0x10 12 fields, v0x11 four more):
//!   saved knobs into the native G_EQ law (`Law::Solid`, sampler-core eq_family/solid.rs),
//!   one one-band `Config` per saved band so each band keeps its own `$ENGINE_PAR_SEQ_*`
//!   lanes (`docs/re/kontakt-solid-geq-live-control.md`).
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

/// Where one saved Solid G-EQ field sits in the serialized record: `at` is the
/// byte offset of the f32 knob (or flag), `switch` the boolean fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SolidField {
    Knob(usize),
    Switch(usize),
}

/// The saved record order (packed one-byte flags): LF gain, freq, bell; LMF gain,
/// freq, Q; HMF gain, freq, Q; HF gain, freq, bell; then v0x11 HP freq, LP freq,
/// LP on, HP on. Engine-parameter names and laws are `filters_eq.tsv`.
fn solid_field(version: u16, parameter: &str) -> Option<SolidField> {
    use SolidField::{Knob, Switch};
    Some(match (parameter.trim_start_matches('$'), version) {
        ("ENGINE_PAR_SEQ_LF_GAIN", _) => Knob(0),
        ("ENGINE_PAR_SEQ_LF_FREQ", _) => Knob(4),
        ("ENGINE_PAR_SEQ_LF_BELL", _) => Switch(8),
        ("ENGINE_PAR_SEQ_LMF_GAIN", _) => Knob(9),
        ("ENGINE_PAR_SEQ_LMF_FREQ", _) => Knob(13),
        ("ENGINE_PAR_SEQ_LMF_Q", _) => Knob(17),
        ("ENGINE_PAR_SEQ_HMF_GAIN", _) => Knob(21),
        ("ENGINE_PAR_SEQ_HMF_FREQ", _) => Knob(25),
        ("ENGINE_PAR_SEQ_HMF_Q", _) => Knob(29),
        ("ENGINE_PAR_SEQ_HF_GAIN", _) => Knob(33),
        ("ENGINE_PAR_SEQ_HF_FREQ", _) => Knob(37),
        ("ENGINE_PAR_SEQ_HF_BELL", _) => Switch(41),
        ("ENGINE_PAR_SEQ_HP_FREQ", 0x11) => Knob(42),
        ("ENGINE_PAR_SEQ_LP_FREQ", 0x11) => Knob(46),
        ("ENGINE_PAR_SEQ_LP", 0x11) => Switch(50),
        ("ENGINE_PAR_SEQ_HP", 0x11) => Switch(51),
        _ => return None,
    })
}

/// Apply one recorded script write (0..1000000) to the saved record: knobs are
/// the identity law, switches store 1 when the written value is at least 1.
pub(super) fn solid_store(version: u16, public: &mut [u8], parameter: &str, value: i32) -> bool {
    match solid_field(version, parameter) {
        Some(SolidField::Knob(at)) => match public.get_mut(at..at + 4) {
            Some(field) => {
                let stored = (f64::from(value) / 1_000_000.).clamp(0., 1.) as f32;
                field.copy_from_slice(&stored.to_le_bytes());
                true
            }
            None => false,
        },
        Some(SolidField::Switch(at)) => match public.get_mut(at) {
            Some(field) => {
                *field = u8::from(value >= 1);
                true
            }
            None => false,
        },
        None => false,
    }
}

/// Solid G-EQ -> one one-band `Config` per saved band, in signal order, each
/// carrying the module's band index. One band per `Config` is what gives every
/// band its own script lanes (`Config::lanes`), and the cascade is unchanged:
/// `eq_family::lower` emits the sections of consecutive bands in order.
pub(super) fn solid_geq(version: u16, public: &[u8], notes: &mut Notes) -> Option<Vec<Processor>> {
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
        (0, band(if s.lf_bell { Kind::Peak } else { Kind::LowShelf }, SolidSection::LowFrequency, s.lf[1], 0., s.lf[0])),
        (1, band(Kind::Peak, SolidSection::LowMid, s.lmf[1], s.lmf[2], s.lmf[0])),
        (2, band(Kind::Peak, SolidSection::HighMid, s.hmf[1], s.hmf[2], s.hmf[0])),
        (3, band(if s.hf_bell { Kind::Peak } else { Kind::HighShelf }, SolidSection::HighFrequency, s.hf[1], 0., s.hf[0])),
    ];
    if let Some(([hp, lp], [lp_on, hp_on])) = s.filters {
        if hp_on {
            bands.push((4, band(Kind::HighPass, SolidSection::HighPass, hp, 0., 0.)));
        }
        if lp_on {
            bands.push((5, band(Kind::LowPass, SolidSection::LowPass, lp, 0., 0.)));
        }
    }
    notes.push((
        "Solid G-EQ smoothing".into(),
        "native coefficients and topology are exact (G_EQ setter vectors); the native 20 ms coefficient \
         smoothing on knob changes is not modelled"
            .into(),
        Reason::UnknownLaw,
    ));
    bands
        .into_iter()
        .map(|(index, band)| {
            let mut config = Config::new(&[band])?;
            config.band = index;
            Some(Processor::Eq(config))
        })
        .collect()
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
        let Some(processors) = solid_geq(0x11, &saved(0x11), &mut notes) else { panic!() };
        let bands: Vec<(u8, Kind, Law)> = processors
            .iter()
            .map(|p| match p {
                Processor::Eq(c) => {
                    assert_eq!(c.count, 1, "one band per config");
                    (c.band, c.bands()[0].kind, c.bands()[0].law)
                }
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            bands,
            [
                (0, Kind::Peak, Law::Solid(SolidSection::LowFrequency)),
                (1, Kind::Peak, Law::Solid(SolidSection::LowMid)),
                (2, Kind::Peak, Law::Solid(SolidSection::HighMid)),
                (3, Kind::HighShelf, Law::Solid(SolidSection::HighFrequency)),
                (5, Kind::LowPass, Law::Solid(SolidSection::LowPass)),
            ]
        );
        let Processor::Eq(low_mid) = processors[1] else { panic!() };
        assert_eq!(low_mid.bands()[0].gain_db, 0.75);
        assert!(!notes.is_empty());
    }

    #[test]
    fn solid_geq_v10_has_no_pass_filters_and_bad_fields_are_reported() {
        let mut notes = vec![];
        let Some(processors) = solid_geq(0x10, &saved(0x10), &mut notes) else { panic!() };
        assert_eq!(processors.len(), 4);
        let mut short = saved(0x11);
        short.pop();
        assert!(solid_geq(0x11, &short, &mut vec![]).is_none());
    }

    #[test]
    fn solid_store_writes_the_saved_fields_the_reader_sees() {
        let mut public = saved(0x11);
        // Identity knobs, the bell switch and the two v0x11 enables.
        assert!(solid_store(0x11, &mut public, "ENGINE_PAR_SEQ_LF_GAIN", 250_000));
        assert!(solid_store(0x11, &mut public, "$ENGINE_PAR_SEQ_LMF_Q", 900_000));
        assert!(solid_store(0x11, &mut public, "ENGINE_PAR_SEQ_HF_FREQ", 1_000_000));
        assert!(solid_store(0x11, &mut public, "ENGINE_PAR_SEQ_LF_BELL", 0));
        assert!(solid_store(0x11, &mut public, "ENGINE_PAR_SEQ_HP", 1));
        assert!(solid_store(0x11, &mut public, "ENGINE_PAR_SEQ_LP", 0));
        let s = read(0x11, &public).expect("still a v0x11 record");
        assert_eq!(s.lf[0], 0.25);
        assert_eq!(s.lmf[2], 0.9);
        assert_eq!(s.hf[1], 1.);
        assert!(!s.lf_bell);
        assert_eq!(s.filters, Some(([0.5, 0.5], [false, true])));
        // A v0x10 record has no pass filters to write, and a short one refuses.
        let mut v10 = saved(0x10);
        assert!(!solid_store(0x10, &mut v10, "ENGINE_PAR_SEQ_HP_FREQ", 500_000));
        assert!(!solid_store(0x11, &mut v10, "ENGINE_PAR_SEQ_SEQ", 1));
        assert_eq!(v10, saved(0x10));
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
