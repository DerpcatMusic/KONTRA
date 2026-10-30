//! Hot Solo (module 0x54, version 0x10) as the engine's amp stage: its saved
//! record (reader 0x140d01b50) and the tone response measured through Kontakt.
//!
//! What is modelled is the module's **linear** tone stage at one measured
//! operating point: Electric Mint's saved record, which is what the parity
//! render compares. The DSP class (`.?AVBEffectHotSolo@@`, vtable
//! 0x144fc0900) and its block process (slot 23, 0x140aa2ae0) are located, and
//! the per-block structure is read — 32-frame chunks, a pending-recalc flag at
//! +0x715c run through 0x140ad5440, a second at +0x715d through 0x140ad44f0
//! (whose body is the state shuffle, not a coefficient law), and the output
//! tail 0x140ac21f0 that ramps one gain per sample by `(target − now)/180`
//! (dat_144f38638 = 1/180) before the bus sum — but the coefficient law in
//! 0x140ad5440 and the piecewise shaper in kernel 0x140aab1e0 are not
//! recovered, so the bands below are the measured transfer function, not a
//! reconstruction of that code.
//!
//! The measurement (docs/re/engine/k-hotsolo.md): the same note through the
//! same instrument in Kontakt and in KONTRA, whose only unmodelled insert is
//! this one, band by band. Two independent pairs agree to 0.32 dB RMS and the
//! fitted stage matches them to 0.81 dB RMS (1.44 dB worst band).

use kontra_ir as ir;

/// The record's eight floats and two flags (reader 0x140d01b50, v0x10).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Saved {
    pub prenormal: f32,
    pub preoverdrive: f32,
    pub master: f32,
    pub presence: f32,
    pub depth: f32,
    pub treble: f32,
    pub mid: f32,
    pub bass: f32,
    /// The overdrive channel select (the record's byte at +0x28).
    pub overdrive: bool,
    /// Mono (the record's byte at +0x29).
    pub mono: bool,
}

impl Saved {
    /// The saved record, or none when it is shorter than its reader needs.
    pub(crate) fn read(public: &[u8]) -> Option<Self> {
        let f = |n: usize| {
            let bytes = public.get(n..n + 4)?.try_into().ok()?;
            Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
        };
        Some(Self {
            prenormal: f(0)?,
            preoverdrive: f(4)?,
            master: f(8)?,
            presence: f(12)?,
            depth: f(16)?,
            treble: f(20)?,
            mid: f(24)?,
            bass: f(28)?,
            overdrive: public.get(32).is_some_and(|&b| b != 0),
            mono: public.get(33).is_some_and(|&b| b != 0),
        })
    }
}

/// The record the bands were measured at: Electric Mint's insert slot 3, as the
/// file stores it (the bits are the record's own; two knobs near 0.603 differ in
/// the last places, which is why comparisons below carry a tolerance).
pub(crate) const MEASURED: Saved = Saved {
    prenormal: f32::from_bits(0x3eaa_8ae7),
    preoverdrive: f32::from_bits(0x3ef0_eb46),
    master: f32::from_bits(0x3ed7_485e),
    presence: f32::from_bits(0x3f76_51a0),
    depth: f32::from_bits(0x3f1a_3476),
    treble: f32::from_bits(0x3f1a_8145),
    mid: f32::from_bits(0x3f40_5e92),
    bass: f32::from_bits(0x3f24_9a24),
    overdrive: false,
    mono: false,
};

/// The measured tone stage, in series: low shelf, presence/upper-mid peak,
/// treble shelf. A RBJ shelf's `q` is its slope (1 the steepest knee).
pub(crate) const BANDS: [ir::Bell; 4] = [
    ir::Bell {
        shape: ir::Shape::LowShelf,
        hz: 40.0,
        q: 1.0,
        gain_db: -6.67,
    },
    ir::Bell {
        shape: ir::Shape::Peak,
        hz: 2_903.0,
        q: 0.374,
        gain_db: 13.69,
    },
    ir::Bell {
        shape: ir::Shape::HighShelf,
        hz: 4_539.0,
        q: 1.0,
        gain_db: 8.77,
    },
    ir::Bell::FLAT,
];

/// The stage's own gain at [`MEASURED`], linear (+2.17 dB): the record's
/// prenormal and master make up more than they take away.
const MEASURED_GAIN: f32 = 1.283;

/// The record as the engine's amp. `lost` names what the record asks for and
/// this model does not carry.
pub(crate) fn amp(saved: Saved, at: &str) -> (ir::Amp, Option<String>) {
    // Prenormal and master are gains, and the record's own run through them is
    // in the measured bands: their law is the ratio to the measured point.
    // Native evidence: master 0.5 -> 1.0 raised level 5 dB, where a
    // proportional law predicts 6 (that render clipped at 1.0); both mute the
    // output at 0.
    let gain = if saved.prenormal > 0.0 && saved.master > 0.0 {
        MEASURED_GAIN * (saved.prenormal / MEASURED.prenormal) * (saved.master / MEASURED.master)
    } else {
        0.0
    };
    let value = ir::Amp {
        input: 1.0,
        bands: BANDS,
        output: gain,
    };
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId("kontakt.hot_solo.v1"),
        location: format!("{at}/hot_solo"),
        input: format!("{saved:?}"),
        output: format!("{value:?}"),
    });
    let mut lost = Vec::new();
    let tone = [
        ("presence", saved.presence, MEASURED.presence),
        ("depth", saved.depth, MEASURED.depth),
        ("treble", saved.treble, MEASURED.treble),
        ("mid", saved.mid, MEASURED.mid),
        ("bass", saved.bass, MEASURED.bass),
    ];
    let off: Vec<_> = tone
        .iter()
        .filter(|(_, value, measured)| (value - measured).abs() > 1e-3)
        .map(|(name, _, _)| *name)
        .collect();
    if !off.is_empty() {
        lost.push(format!(
            "Hot Solo tone knobs not modelled ({})",
            off.join(", ")
        ));
    }
    if saved.overdrive {
        // Pre-overdrive is inert while that channel is off (measured 2026-10-21:
        // 0, 0.5 and 1 give identical output), so only the channel itself is lost.
        lost.push("Hot Solo overdrive channel".to_owned());
    }
    if saved.mono {
        lost.push("Hot Solo mono".to_owned());
    }
    (value, lost.into_iter().reduce(|a, b| format!("{a}; {b}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The record as Electric Mint stores it (module 0x54, v0x10, insert slot 3).
    fn electric_mint() -> Vec<u8> {
        let mut out = Vec::new();
        for value in [
            MEASURED.prenormal,
            MEASURED.preoverdrive,
            MEASURED.master,
            MEASURED.presence,
            MEASURED.depth,
            MEASURED.treble,
            MEASURED.mid,
            MEASURED.bass,
        ] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn the_reader_takes_the_eight_floats_and_two_flags() {
        let saved = Saved::read(&electric_mint()).unwrap();
        assert_eq!(saved, MEASURED);
        assert!(!saved.overdrive && !saved.mono);
        assert_eq!(
            Saved::read(&[0; 31]),
            None,
            "eight floats are the reader's minimum"
        );
        assert!(
            !Saved::read(&[0; 32]).unwrap().overdrive,
            "a missing flag reads as off"
        );
    }

    #[test]
    fn the_measured_record_is_the_engine_amp_with_no_note() {
        let (amp, lost) = amp(MEASURED, "rack");
        assert_eq!(amp.bands, BANDS);
        assert!((amp.gain() - MEASURED_GAIN).abs() < 1e-6);
        assert_eq!(lost, None);
    }

    #[test]
    fn a_record_at_another_tone_is_reported_not_silently_retuned() {
        let other = Saved {
            presence: 0.2,
            bass: 1.0,
            ..MEASURED
        };
        let (amp, lost) = amp(other, "rack");
        assert_eq!(amp.bands, BANDS, "the bands are the measured ones");
        let lost = lost.unwrap();
        assert!(lost.contains("presence") && lost.contains("bass"), "{lost}");
        assert!(!lost.contains("treble"), "{lost}");
    }

    #[test]
    fn the_overdrive_channel_is_reported() {
        let (_, lost) = amp(
            Saved {
                overdrive: true,
                ..MEASURED
            },
            "rack",
        );
        assert!(lost.unwrap().contains("overdrive channel"));
    }

    #[test]
    fn a_muted_gain_silences_the_stage() {
        for saved in [
            Saved {
                prenormal: 0.0,
                ..MEASURED
            },
            Saved {
                master: 0.0,
                ..MEASURED
            },
        ] {
            assert_eq!(amp(saved, "rack").0.gain(), 0.0);
        }
    }

    #[test]
    fn the_gain_follows_the_record_through_the_measured_point() {
        let louder = Saved {
            master: 2.0 * MEASURED.master,
            ..MEASURED
        };
        assert!((amp(louder, "rack").0.gain() - 2.0 * MEASURED_GAIN).abs() < 1e-5);
    }
}
