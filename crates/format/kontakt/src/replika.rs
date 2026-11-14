//! Replika (module 0x5a, saved v0x12), its Modern, Analogue, Tape and
//! Vintage modes, onto [`ir::Delay`].
//! Laws measured on Kontakt 8.13.1 through the oracle (scratch rig, INIT
//! Replika, 2026-10-29) and kemu (docs/re/engine/k-replika-binding.md):
//! - the line is TIME ms (+2 frames); in stereo mode the left runs
//!   12.5 ms·LR_OFFSET short and the right as much long;
//! - each repeat is scaled by 1.2·FEEDBACK (−18.42, −5.35, −0.35 dB at 0.1,
//!   0.45, 0.8);
//! - LOWCUT is a Q ½ high-pass at 10·100^x Hz (off at 0), HIGHCUT a −10 dB high shelf
//!   (slope 0.75) at the corners below, open at 1; both sit in the loop, so
//!   every echo is cut again;
//! - ping-pong sends (l+r)/2 into the left line, left to right at 1, right
//!   to left at the feedback;
//! - out L = 2(1−PAN)·(M + 2·WIDTH·S), out R = 2·PAN·(M − 2·WIDTH·S).
//!
//!
//! The other modes, measured wet only on Kontakt 8.13.1 (2026-11-14,
//! docs/re/engine/fx-measured/replika-modes.md): each has its own body
//! inside the loop ([`body`]); Analogue and Vintage swing a smoothed
//! triangle, Tape a FLUTTER sine; Tape's loop gain is FEEDBACK, not 1.2×.
//! Diffuse is not run. HIGHCUT and LOWCUT are taken as Modern's in every
//! mode (unmeasured there).
//!
//! A synced TIME (units 5 and up) saves beats per unit and a count in TIME
//! (docs/re/kontakt-bindings, RDL_TIME_UNIT); played as count × beats,
//! unmeasured, so still reported.

use kontra_ir as ir;

use crate::script_knobs::SlotKnob;

/// The saved fields the Modern mode reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Replika {
    /// 0 Modern, 1 Analogue, 2 Tape, 3 Vintage; 4 Diffuse is not run.
    pub mode: i32,
    pub ms: f32,
    pub feedback: f32,
    pub low_cut: f32,
    pub high_cut: f32,
    /// Tape's age (its body) and flutter, Vintage's quality (its low-pass).
    pub tape_age: f32,
    pub flutter: f32,
    pub quality: f32,
    /// The modulation's depth and rate knobs (Modern: a sine, ±8 ms·depth at 0.1·100^rate Hz).
    pub depth: f32,
    pub rate: f32,
    pub stereo: bool,
    pub pingpong: bool,
    pub flip: bool,
    /// A synced TIME's beats per unit (the field is −1 when free); TIME is
    /// then a count of units.
    pub beats: Option<f32>,
    pub pan: f32,
    pub width: f32,
    pub offset: f32,
}

/// Kontakt's INIT Replika (oracle save, 2026-10-29).
pub(crate) const INIT: Replika = Replika {
    mode: 0,
    ms: 500.0,
    feedback: 0.46,
    low_cut: 0.0,
    high_cut: 0.95,
    tape_age: 0.25,
    flutter: 0.3,
    quality: 1.0,
    depth: 0.0,
    rate: 0.1,
    stereo: true,
    pingpong: false,
    flip: false,
    beats: None,
    pan: 0.5,
    width: 0.5,
    offset: 0.3,
};

/// The record (reader layout: i32 mode, ten floats from TIME to RATE, the
/// stereo, noise and ping-pong bytes, then floats packed after them).
pub(crate) fn saved(version: u16, public: &[u8]) -> Option<Replika> {
    if version != 0x12 {
        return None;
    }
    let f = |n: usize| {
        let bytes = public.get(n..n + 4)?.try_into().ok()?;
        Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
    };
    let flag = |n: usize| public.get(n).map(|&b| b != 0);
    Some(Replika {
        mode: i32::from_le_bytes(public.get(..4)?.try_into().ok()?),
        ms: f(4)?,
        feedback: f(8)?,
        low_cut: f(12)?,
        high_cut: f(16)?,
        tape_age: f(24)?,
        flutter: f(28)?,
        quality: f(32)?,
        depth: f(36)?,
        rate: f(40)?,
        stereo: flag(44)?,
        pingpong: flag(46)?,
        beats: Some(f(47)?).filter(|&b| b >= 0.0),
        pan: f(89)?,
        width: f(93)?,
        offset: f(97)?,
        flip: flag(113)?,
    })
}

/// What the Modern mode does that is not run, for the gap log.
pub(crate) fn lost(r: Replika) -> impl Iterator<Item = &'static str> {
    [
        (r.beats.is_some(), "Replika tempo sync"),
        (!r.stereo && !r.pingpong, "Replika mono mode"),
        (r.pingpong && r.flip, "Replika ping-pong flip"),
    ]
    .into_iter()
    .filter_map(|(on, what)| on.then_some(what))
}

/// The Modern mode's delay swing at full depth, ms either side (Kontakt 8.13.1, 1 kHz tone through DEPTH 0.25, 0.5 and
/// 1 at RATE 0.5: the wet signal's phase swings 4.00, 8.00 and 16.00 ms peak to peak; the same at RATE 0.2).
const MODERN_DEPTH_MS: f32 = 8.0;
/// The Modern mode's LFO rate at RATE 0, Hz: 0.1·100^RATE (RATE 0.5 measured 1.000 Hz, RATE 0.8 3.98 Hz).
const MODERN_RATE_HZ: f32 = 0.1;

/// Analogue and Vintage: a triangle of ±19.7 ms·DEPTH averaged over 0.196 s
/// (Kontakt 8.13.1, 1 kHz tone, wet only: the swing's fundamental is the
/// triangle's 8/π² times sinc(π·f·0.196 s) at 0.1, 0.25, 0.63, 1, 1.58,
/// 2.51, 3.98 and 10 Hz, within 0.4 ms; linear in DEPTH at 1 Hz), at
/// Modern's 0.1·100^RATE Hz (0.102 to 10.0 Hz measured).
const BBD_DEPTH_MS: f32 = 19.7;
const BBD_SMOOTH_S: f32 = 0.196;
/// Tape: a 0.875 Hz sine of 4.68 ms·FLUTTER³ (0.127, 1.017, 4.68 ms at 0.3,
/// 0.6, 1; DEPTH, RATE and TAPEAGE move nothing).
const FLUTTER_MS: f32 = 4.68;
const FLUTTER_HZ: f32 = 0.875;

/// A second-order low-pass and high-pass at `hz`, `q`, scaled by `gain`.
const fn low_pass(hz: f32, q: f32, gain: f32) -> ir::Section {
    ir::Section {
        hz,
        num: [gain, 0.0, 0.0],
        den: [1.0, 1.0 / q, 1.0],
    }
}
const fn high_pass(hz: f32, q: f32, gain: f32) -> ir::Section {
    ir::Section {
        hz,
        num: [0.0, 0.0, gain],
        den: [1.0, 1.0 / q, 1.0],
    }
}

/// Analogue's body, fitted to a −20 dB sweep through FEEDBACK 0 (within
/// 0.45 dB rms, 2.3 dB at most, 30 Hz..16 kHz; the same at −60 dB within
/// 0.3 dB): a 6.6 Hz high-pass, a first-order shelf to −9.4 dB from 1.48 kHz,
/// and the bucket brigade's steep low-pass above 8 kHz (−47 dB at 12 kHz).
static ANALOGUE: [ir::Section; 4] = [
    high_pass(6.583, 0.236, 1.0),
    ir::Section {
        hz: 1480.0,
        num: [1.0, 0.3402, 0.0],
        den: [1.0, 1.0, 0.0],
    },
    ir::Section {
        hz: 11_207.0,
        num: [1.0, 0.0, 0.969],
        den: [1.0, 1.0 / 0.705, 1.0],
    },
    low_pass(8418.0, 1.482, 1.0),
];

/// Tape's body at TAPEAGE 0, 0.25 and 1: a gain (+1.57, +1.84, +3.40 dB) on
/// a high-pass, and a low-pass, each fitted to a sweep through FEEDBACK 0
/// (within 0.1, 2.3 and 0.2 dB; the 0.25 miss is below 50 Hz). Between them
/// the nearest measured age plays.
static TAPE: [[ir::Section; 2]; 3] = [
    [
        high_pass(12.48, 0.379, 1.1987),
        low_pass(8861.0, 0.493, 1.0),
    ],
    [
        high_pass(35.12, 0.388, 1.2352),
        low_pass(6134.0, 0.375, 1.0),
    ],
    [
        high_pass(32.15, 0.141, 1.4794),
        low_pass(3568.0, 0.236, 1.0),
    ],
];

/// Vintage's low-pass at QUALITY 0 and 1 (−0.07 dB; within 1 dB to
/// 16 kHz); between them the nearer one plays.
static VINTAGE: [[ir::Section; 1]; 2] = [
    [low_pass(9221.0, 0.438, 0.9924)],
    [low_pass(13_291.0, 0.402, 0.9924)],
];

/// A mode's body and loop gain per FEEDBACK (`None`: Diffuse, not run).
fn body(r: Replika) -> Option<(&'static [ir::Section], f32)> {
    let [young, init, old] = &TAPE;
    let [low, high] = &VINTAGE;
    match r.mode {
        0 => Some((&[], 1.2)),
        1 => Some((&ANALOGUE, 1.2)),
        2 => Some((
            match r.tape_age {
                a if a < 0.125 => young,
                a if a < 0.625 => init,
                _ => old,
            },
            1.0,
        )),
        3 => Some((if r.quality < 0.5 { low } else { high }, 1.2)),
        _ => None,
    }
}

/// The mode's LFO: swing ms, rate Hz and the triangle's smoothing.
fn modulation(r: Replika) -> (f32, f32, Option<f32>) {
    let depth = r.depth.clamp(0.0, 1.0);
    let hz = MODERN_RATE_HZ * 100f32.powf(r.rate.clamp(0.0, 1.0));
    match r.mode {
        0 => (MODERN_DEPTH_MS * depth, hz, None),
        2 => (
            FLUTTER_MS * r.flutter.clamp(0.0, 1.0).powi(3),
            FLUTTER_HZ,
            None,
        ),
        _ => (BBD_DEPTH_MS * depth, hz, Some(BBD_SMOOTH_S)),
    }
}

/// The HIGHCUT shelf's corner by knob (measured every 0.1; open at 1).
const HIGH_CUT_HZ: [f32; 11] = [
    203.0, 2201.0, 4115.0, 6212.0, 8169.0, 10103.0, 11958.0, 13822.0, 15567.0, 16849.0, 22000.0,
];

/// The mode's delay; `None` for Diffuse.
pub(crate) fn delay(r: Replika) -> Option<ir::Delay> {
    let (body, loop_gain) = body(r)?;
    let (depth_ms, lfo_hz, triangle_s) = modulation(r);
    let g = loop_gain * r.feedback.clamp(0.0, 1.0);
    let (input, feedback, spread_ms) = if r.pingpong {
        ([[0.5, 0.5], [0.0, 0.0]], [[0.0, g], [1.0, 0.0]], 0.0)
    } else {
        (
            [[1.0, 0.0], [0.0, 1.0]],
            [[g, 0.0], [0.0, g]],
            12.5 * r.offset,
        )
    };
    let (p, w) = (r.pan.clamp(0.0, 1.0), r.width.clamp(0.0, 1.0));
    let (l, rr) = (2.0 * (1.0 - p), 2.0 * p);
    let x = r.high_cut.clamp(0.0, 1.0);
    let points = HIGH_CUT_HZ
        .iter()
        .zip(0u8..)
        .map(|(&hz, n)| (f32::from(n) / 10.0, hz, 0.0));
    Some(ir::Delay {
        time: match r.beats {
            Some(beats) => ir::DelayTime::Beats(r.ms.max(0.0) * beats),
            None => ir::DelayTime::Ms(r.ms.max(0.0)),
        },
        depth_ms,
        second_head: false,
        input,
        feedback,
        output: [
            [l * (0.5 + w), l * (0.5 - w)],
            [rr * (0.5 - w), rr * (0.5 + w)],
        ],
        damping_hz: f32::INFINITY,
        // Off at 0: the echo keeps 30 Hz there (−0.9 dB at 10 Hz).
        low_cut_hz: if r.low_cut > 0.0 {
            kontra_law::kontakt::REPLIKA_LOW_CUT.eval(r.low_cut)
        } else {
            0.0
        },
        steep_low_cut: true,
        body,
        triangle_s,
        high_shelf: Some(ir::Bell {
            shape: ir::Shape::HighShelf,
            hz: ir::Law::Breakpoints(points.collect()).eval(x),
            q: 0.75,
            gain_db: if x < 1.0 { -10.0 } else { 0.0 },
        }),
        spread_ms,
        rate: ir::Rate::Hz(lfo_hz),
        phase: 0.0,
    })
}

/// The time, feedback, low-cut and high-cut knobs, each sampled with the
/// others at `r`.
pub(crate) fn knobs(r: Replika) -> Vec<SlotKnob> {
    use ir::EffectParam as E;
    use ir::SlotKnob as K;
    let cells: Vec<_> = (0..4).map(E::DelayLoop).collect();
    type Knob<'a> = (K, &'a [E], fn(&mut Replika, f32));
    let knobs: [Knob; 4] = [
        (K::DelayTime, &[E::DelayMs], |r, x| r.ms = 2000.0 * x),
        (K::DelayFeedback, &cells, |r, x| r.feedback = x),
        (K::DelayLowCut, &[E::DelayLowCutHz], |r, x| r.low_cut = x),
        (
            K::DelayHighCut,
            &[E::DelayShelfHz, E::DelayShelfDb],
            |r, x| r.high_cut = x,
        ),
    ];
    let mut out = Vec::new();
    for (knob, fields, set) in knobs {
        out.extend(crate::script_knobs::sampled(knob, fields, &|x| {
            let mut r = r;
            set(&mut r, x);
            delay(r).map(ir::EffectKind::Delay)
        }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The INIT record as Kontakt 8.13.1 saves it.
    const INIT_RECORD: &str = "000000000000fa431f85eb3e000000003333733fcdcccc3d0000803e9a99993e0000803f00000000cdcccc3d010000000080bf000000000000803f000000803f0000003f0000003f0000003f01000000000000003f0000003f0000003f0000003f9a99993e00000000000000000000000000";

    fn bytes(hex: &str) -> Vec<u8> {
        let hex: String = hex.split_whitespace().collect();
        (0..hex.len() / 2)
            .filter_map(|i| u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok())
            .collect()
    }

    #[test]
    fn the_init_record_reads_as_kontakts_defaults() {
        assert_eq!(saved(0x12, &bytes(INIT_RECORD)), Some(INIT));
    }

    #[test]
    fn modern_lands_on_the_measured_laws() {
        let d = delay(INIT).expect("modern");
        assert_eq!(d.time, ir::DelayTime::Ms(500.0));
        // 0.46 feedback: 1.2x a repeat; LR offset 0.3: 3.75 ms either side.
        assert!((d.feedback[0][0] - 0.552).abs() < 1e-6 && d.feedback[0][1] == 0.0);
        assert!((d.spread_ms - 3.75).abs() < 1e-6);
        assert_eq!(d.output, [[1.0, 0.0], [0.0, 1.0]]);
        assert_eq!(d.low_cut_hz, 0.0);
        let shelf = d.high_shelf.expect("shelf");
        assert!((shelf.hz - 19424.5).abs() < 1.0 && shelf.gain_db == -10.0);
        let open = delay(Replika {
            high_cut: 1.0,
            low_cut: 0.5,
            ..INIT
        })
        .expect("modern");
        assert_eq!(open.high_shelf.map(|b| b.gain_db), Some(0.0));
        assert!((open.low_cut_hz - 100.0).abs() < 1e-3);
        let pp = delay(Replika {
            pingpong: true,
            ..INIT
        })
        .expect("modern");
        assert_eq!(
            (pp.input[0], pp.feedback[1], pp.spread_ms),
            ([0.5, 0.5], [1.0, 0.0], 0.0)
        );
        assert_eq!(delay(Replika { mode: 4, ..INIT }), None);
    }

    #[test]
    fn modern_modulation_is_a_sine_of_8_ms_at_a_log_rate() {
        // Kontakt 8.13.1: DEPTH 0.25 / 0.5 / 1 swing the wet phase 4 / 8 / 16 ms peak to peak; RATE 0.5 runs
        // 1.000 Hz and 0.8 3.98 Hz (docs/re/engine/fx-measured/replika-modes.md).
        let d = |depth, rate| {
            delay(Replika {
                depth,
                rate,
                ..INIT
            })
            .expect("modern")
        };
        assert_eq!(d(0.5, 0.5).depth_ms, 4.0);
        assert_eq!(d(1.0, 0.5).depth_ms, 8.0);
        assert!(matches!(d(1.0, 0.5).rate, ir::Rate::Hz(hz) if (hz - 1.0).abs() < 1e-5));
        assert!(matches!(d(1.0, 0.8).rate, ir::Rate::Hz(hz) if (hz - 3.981).abs() < 1e-3));
        assert_eq!(d(0.0, 0.5).depth_ms, 0.0);
    }

    #[test]
    fn the_other_modes_carry_their_measured_laws() {
        // docs/re/engine/fx-measured/replika-modes.md: Analogue and Vintage a smoothed
        // triangle of 19.7 ms at the Modern rate, Tape a 0.875 Hz flutter of 4.68 ms·FLUTTER³
        // and a loop gain of FEEDBACK.
        let d = |mode, r: Replika| {
            delay(Replika {
                mode,
                depth: 1.0,
                rate: 0.5,
                ..r
            })
            .expect("mode")
        };
        for mode in [1, 3] {
            let v = d(mode, INIT);
            assert_eq!((v.depth_ms, v.triangle_s), (19.7, Some(0.196)));
            assert!(matches!(v.rate, ir::Rate::Hz(hz) if (hz - 1.0).abs() < 1e-5));
            assert!((v.feedback[0][0] - 0.552).abs() < 1e-6);
        }
        let tape = d(
            2,
            Replika {
                flutter: 0.6,
                ..INIT
            },
        );
        assert!((tape.depth_ms - 4.68 * 0.216).abs() < 1e-5 && tape.rate == ir::Rate::Hz(0.875));
        assert!((tape.feedback[0][0] - 0.46).abs() < 1e-6 && tape.triangle_s.is_none());
        // TAPEAGE at a measured point lands on its fit.
        let old = d(
            2,
            Replika {
                tape_age: 1.0,
                ..INIT
            },
        );
        assert!(matches!(old.body, [_, s] if (s.hz - 3568.0).abs() < 0.5));
    }
}
