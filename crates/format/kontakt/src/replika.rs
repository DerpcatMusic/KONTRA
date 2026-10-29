//! Replika (module 0x5a, saved v0x12) in its Modern mode onto [`ir::Delay`].
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
//! A synced TIME (KSP units 5 and up) is a count of note values, 1 +
//! round(11x), at most 2000 ms. Kontakt saves the count in TIME, the beats
//! per unit after the flags (−1 when free) and the free ms after that.
//! Oracle at 120 BPM, 2026-10-29: quarter at x = 0, 0.1 → 1, 2 beats; 8th at
//! 0.5 → 7 eighths; 16th at 0.75 → 9 sixteenths; quarter at 0.5 and 1, and
//! quarter triplet at 0.5, → 2000 ms. Saves: quarter at 0, 2/11, 1 → counts
//! 1, 3, 12. Conflux's table writes k/11 for k + 1 units.

use kontra_ir as ir;

use crate::script_knobs::SlotKnob;

/// The saved fields the Modern mode reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Replika {
    /// 0 Modern; Analogue, Tape, Vintage and Diffuse are not run.
    pub mode: i32,
    /// TIME: ms when free, the count of units when synced.
    pub ms: f32,
    /// The free ms a synced TIME keeps.
    pub free_ms: f32,
    pub feedback: f32,
    pub low_cut: f32,
    pub high_cut: f32,
    /// The modulation's depth; its law is unmeasured.
    pub depth: f32,
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
    free_ms: 0.0,
    feedback: 0.46,
    low_cut: 0.0,
    high_cut: 0.95,
    depth: 0.0,
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
        free_ms: f(51)?,
        feedback: f(8)?,
        low_cut: f(12)?,
        high_cut: f(16)?,
        depth: f(36)?,
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
        (r.depth > 0.0, "Replika modulation"),
        (!r.stereo && !r.pingpong, "Replika mono mode"),
        (r.pingpong && r.flip, "Replika ping-pong flip"),
    ]
    .into_iter()
    .filter_map(|(on, what)| on.then_some(what))
}

/// The HIGHCUT shelf's corner by knob (measured every 0.1; open at 1).
const HIGH_CUT_HZ: [f32; 11] = [
    203.0, 2201.0, 4115.0, 6212.0, 8169.0, 10103.0, 11958.0, 13822.0, 15567.0, 16849.0, 22000.0,
];

/// The longest TIME, free or synced.
const MAX_MS: f32 = 2000.0;

/// A synced TIME knob's count of units.
fn count(x: f32) -> f32 {
    1.0 + (11.0 * x.clamp(0.0, 1.0)).round()
}

/// Beats per unit by KSP's `$NI_SYNC_UNIT_*` (0..=4 free; 11 and 14 have no
/// constant): the saved beats Kontakt writes for units 5, 6, 7, 8, 10, 16
/// and 20 (docs/re/kontakt-bindings), the rest by their note names.
const UNIT_BEATS: [(u8, f32); 15] = [
    (5, 4.0),
    (6, 8.0 / 3.0),
    (7, 2.0),
    (8, 4.0 / 3.0),
    (9, 1.0),
    (10, 2.0 / 3.0),
    (12, 0.5),
    (13, 1.0 / 3.0),
    (15, 0.25),
    (16, 1.0 / 6.0),
    (17, 0.125),
    (18, 1.0 / 12.0),
    (19, 0.0625),
    (20, 1.0 / 24.0),
    (21, 1.0 / 64.0),
];

/// A law that holds each of `steps` from its `x` to the next one's.
fn steps(steps: impl Iterator<Item = (f32, f32)>) -> ir::Law {
    let steps: Vec<_> = steps.collect();
    let ends = steps.windows(2).flat_map(|w| match w {
        [(a, v), (b, _)] => [(*a, *v, 0.0), (b.next_down(), *v, 0.0)],
        _ => [(0.0, 0.0, 0.0); 2],
    });
    let last = steps.last().map(|&(a, v)| [(a, v, 0.0), (1.0, v, 0.0)]);
    ir::Law::Breakpoints(ends.chain(last.into_iter().flatten()).collect())
}

/// The Modern mode's delay; `None` for another mode.
pub(crate) fn delay(r: Replika) -> Option<ir::Delay> {
    if r.mode != 0 {
        return None;
    }
    let g = 1.2 * r.feedback.clamp(0.0, 1.0);
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
        time: ir::DelayTime::Ms(match r.beats {
            Some(_) => r.free_ms.max(0.0),
            None => r.ms.max(0.0),
        }),
        depth_ms: 0.0,
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
            10.0 * 100f32.powf(r.low_cut.min(1.0))
        } else {
            0.0
        },
        steep_low_cut: true,
        high_shelf: Some(ir::Bell {
            shape: ir::Shape::HighShelf,
            hz: ir::Law::Breakpoints(points.collect()).eval(x),
            q: 0.75,
            gain_db: if x < 1.0 { -10.0 } else { 0.0 },
        }),
        spread_ms,
        sync: ir::DelaySync {
            count: match r.beats {
                Some(_) => r.ms.max(1.0),
                None => count(r.ms / MAX_MS),
            },
            unit_beats: r.beats.unwrap_or(0.0),
            max_ms: MAX_MS,
        },
        rate: ir::Rate::Hz(0.0),
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
    let knobs: [Knob; 3] = [
        (K::DelayFeedback, &cells, |r, x| r.feedback = x),
        (K::DelayLowCut, &[E::DelayLowCutHz], |r, x| r.low_cut = x),
        (
            K::DelayHighCut,
            &[E::DelayShelfHz, E::DelayShelfDb],
            |r, x| r.high_cut = x,
        ),
    ];
    // TIME sets both its free ms and its count; the unit picks which plays.
    let half = 0.5 / 11.0;
    let counts = (0..12u8).map(|k| {
        (
            (f32::from(k) / 11.0 - half).max(0.0),
            count(f32::from(k) / 11.0),
        )
    });
    // KSP passes the unit constant raw: unit u is the knob at u·1e-6.
    let free = (0..5u8).map(|u| ((f32::from(u) - 0.5).max(0.0) * 1e-6, 0.0));
    let synced = UNIT_BEATS
        .iter()
        .map(|&(u, b)| ((f32::from(u) - 0.5) * 1e-6, b));
    let mut out = vec![
        (
            K::DelayTime,
            E::DelayMs,
            ir::Law::Linear {
                lo: 0.0,
                hi: MAX_MS,
            },
        ),
        (K::DelayTime, E::DelayCount, steps(counts)),
        (K::DelayUnit, E::DelayUnitBeats, steps(free.chain(synced))),
    ];
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
        assert_eq!(delay(Replika { mode: 2, ..INIT }), None);
    }

    #[test]
    fn a_synced_time_counts_note_values_up_to_two_seconds() {
        // Saved quarter, TIME at 1: twelve units, free ms kept.
        let q = Replika {
            ms: 12.0,
            free_ms: 500.0,
            beats: Some(1.0),
            ..INIT
        };
        let d = delay(q).expect("modern");
        assert_eq!(d.time, ir::DelayTime::Ms(500.0));
        assert_eq!(
            (d.sync.count, d.sync.unit_beats, d.sync.max_ms),
            (12.0, 1.0, 2000.0)
        );
        assert_eq!(delay(INIT).map(|d| d.sync.unit_beats), Some(0.0));
        let law = |field| {
            knobs(INIT)
                .into_iter()
                .find(|&(_, f, _)| f == field)
                .map(|(_, _, law)| law)
                .expect("law")
        };
        let count = law(ir::EffectParam::DelayCount);
        let counts = [0.0, 0.1, 2.0 / 11.0, 0.5, 0.75, 1.0].map(|x| count.eval(x));
        assert_eq!(counts, [1.0, 2.0, 3.0, 7.0, 9.0, 12.0]);
        let unit = law(ir::EffectParam::DelayUnitBeats);
        let beats = [0.0, 4.0, 5.0, 9.0, 11.0, 12.0, 15.0, 21.0].map(|u| unit.eval(u * 1e-6));
        assert_eq!(
            beats,
            [0.0, 0.0, 4.0, 1.0, 2.0 / 3.0, 0.5, 0.25, 1.0 / 64.0]
        );
    }
}
