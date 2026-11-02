//! Kontakt's Reverb (0x59): its control laws and its Room and Hall networks.
//! The tables are read from the binary (setter `0x141aea370`, line-length
//! law `0x141aebb00`, output taps `0x141ae0160`, units `0x141ae92b0`; v2
//! sampler-core space/design.rs, which matches the emulated native kernel
//! to 1e-5).

use kontra_ir as ir;
use std::sync::LazyLock;

const LINES: usize = ir::NETWORK_LINES;
/// Per-line offsets into the prime table.
const BASE: [u8; LINES] = [
    4, 3, 5, 5, 5, 5, 2, 1, 1, 1, 4, 5, 1, 5, 1, 4, 3, 4, 2, 4, 1, 2, 2, 5, 4, 2, 3, 1, 3, 0, 3,
];
/// Per-line size slopes, Room and Hall.
const SLOPE: [[u8; LINES]; 2] = [
    [
        0, 1, 7, 8, 20, 21, 6, 2, 3, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 4, 5, 22, 23, 24,
        25, 26, 27, 28, 29, 30,
    ],
    [
        0, 1, 6, 7, 4, 5, 2, 3, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30,
    ],
];
/// The lines the left and right inputs feed, and the sign.
const IN_L: [(u8, f32); 10] = [
    (0, 1.),
    (2, 1.),
    (4, 1.),
    (6, -1.),
    (10, -1.),
    (14, 1.),
    (18, 1.),
    (22, -1.),
    (26, -1.),
    (28, -1.),
];
const IN_R: [(u8, f32); 10] = [
    (1, 1.),
    (3, 1.),
    (5, 1.),
    (7, -1.),
    (11, -1.),
    (15, -1.),
    (19, -1.),
    (23, -1.),
    (27, 1.),
    (29, 1.),
];
/// Output taps (line, sign), Room and Hall.
const OUT: [[[(u8, f32); 8]; 2]; 2] = [
    [
        [
            (0, 1.),
            (3, -1.),
            (4, 1.),
            (8, -1.),
            (12, -1.),
            (16, 1.),
            (20, -1.),
            (24, 1.),
        ],
        [
            (1, 1.),
            (2, -1.),
            (5, 1.),
            (9, -1.),
            (13, -1.),
            (17, 1.),
            (21, -1.),
            (25, 1.),
        ],
    ],
    [
        [
            (0, 1.),
            (3, -1.),
            (6, -1.),
            (9, 1.),
            (12, -1.),
            (17, 1.),
            (20, -1.),
            (25, 1.),
        ],
        [
            (1, 1.),
            (2, -1.),
            (7, 1.),
            (8, -1.),
            (13, 1.),
            (16, -1.),
            (21, 1.),
            (24, -1.),
        ],
    ],
];
/// Hall tap weights at full diffusion: the first pair, then the rest
/// (`1/√5`, `√(19/15)`).
const HALL_FAR: [f32; 2] = [0.447_213_6, 1.125_462_9];
/// Unit delays, Room (four units) and Hall (two).
const UNITS: [[[u16; 3]; 4]; 2] = [
    [[31, 79, 41], [43, 73, 53], [59, 61, 67], [71, 47, 37]],
    [[73, 181, 97], [103, 179, 127], [59, 61, 67], [71, 47, 37]],
];

/// The Room (`hall` false) or Hall network.
fn network(hall: bool) -> ir::ReverbNetwork {
    let h = usize::from(hall);
    let slope = SLOPE.get(h).copied().unwrap_or_default();
    // Line i is the prime at ⌊slope·10·s + 90 + base⌋ (Room) or
    // ⌊(slope/32)^1.61·736·s + 90 + base⌋ (Hall), s the size 3x + 1.
    let growth = slope.map(|k| {
        let k = f32::from(k);
        if hall {
            (k * 0.031_25).powf(1.61) * 736.0
        } else {
            k * 10.0
        }
    });
    // The m-sequence c[n+5] = c[n] ^ c[n+2] from 11011.
    let mut sequence = [0u8; LINES];
    let mut c = [1u8, 1, 0, 1, 1];
    for s in &mut sequence {
        *s = c[0];
        c = [c[1], c[2], c[3], c[4], c[0] ^ c[2]];
    }
    // The Hall core takes no input on lines 4..=7.
    let input = |taps: [(u8, f32); 10]| {
        taps.map(|(i, s)| (i, if hall && (4..8).contains(&i) { 0.0 } else { s }))
    };
    let taps = OUT.get(h).copied().unwrap_or_default().map(|t| {
        let mut far = [(0u8, 0.0f32, 0.0f32); 8];
        for (k, (out, (i, s))) in far.iter_mut().zip(t).enumerate() {
            let w = if hall {
                HALL_FAR.get(usize::from(k >= 2)).copied().unwrap_or(1.0)
            } else {
                1.0
            };
            *out = (i, s, s * w);
        }
        far.sort_by_key(|t| t.0);
        far
    });
    ir::ReverbNetwork {
        growth,
        offset: BASE.map(|b| 90.0 + f32::from(b)),
        sequence,
        inputs: [input(IN_L), input(IN_R)],
        inject_lead: 49,
        taps,
        units: UNITS.get(h).copied().unwrap_or_default(),
        unit_count: if hall { 2 } else { 4 },
        unit_feedback: 0.9,
        hf: [0.5, 0.95, 1.0 / 3.7],
        level: [4.0, 0.5],
        shelf_hz: 200.0,
        rate: 44_100.0,
        // Wrapper 0x141aa15c7: above 48 kHz (0x473b8000) the core runs at
        // rate / ⌊rate·f32(1/44100)⌋ (0x37be37c6); other divisions run at
        // the host rate (the native quintic mode, not copied).
        max_rate: 48_000.0,
        // Prepare 0x141ae1d40: the control block (+0x6d80) ramps over
        // 0.05 s (0x1447148c0), the line lengths (+0x6e40) over 0.2 s
        // (0x1447148e0), each from its current value (0x1409abfb0).
        glide_s: [0.05, 0.2],
    }
}

static ROOM: LazyLock<ir::ReverbNetwork> = LazyLock::new(|| network(false));
static HALL: LazyLock<ir::ReverbNetwork> = LazyLock::new(|| network(true));

/// Kontakt's Reverb (0x59) from its nine saved controls, each 0..=1:
/// time, size, damping, modulation, diffusion, predelay, high cut, low shelf,
/// stereo; the room type picks the Room (0) or Hall (1) network. Laws from
/// setter `0x141aea370` (docs/architecture-v2/KONTAKT_DSP_LAWS.md): time
/// 0.5·e^(3.7x) s, size 3x+1, damping x^¼, diffusion x (the units take x/2),
/// predelay 11025x frames at 44.1 kHz, high cut 19000(1−x)+2000 Hz, low
/// shelf −12x dB, stereo x. Modulation swings all but the first line by
/// `10x` frames at 44.1 kHz, at 2 Hz (`0x141aec0a0`).
pub fn reverb(kind: i32, v: [f32; 9]) -> ir::Reverb {
    let [
        time,
        size,
        damping,
        modulation,
        diffusion,
        predelay,
        high_cut,
        low_shelf,
        stereo,
    ] = v.map(|x| x.clamp(0.0, 1.0));
    let laws = reverb_laws();
    ir::Reverb {
        seconds: laws.seconds.eval(time),
        size: laws.size.eval(size),
        network: Some(ir::Network::Lines(if kind == 1 { &HALL } else { &ROOM })),
        damping: laws.damping.eval(damping),
        diffusion: laws.diffusion.eval(diffusion),
        predelay_ms: laws.predelay.eval(predelay),
        high_cut_hz: laws.high_cut.eval(high_cut),
        low_shelf_db: laws.low_shelf.eval(low_shelf),
        stereo: laws.stereo.eval(stereo),
        modulation_ms: modulation * 10.0 / 44.1,
        modulation_hz: 2.0,
    }
}

/// The networks `$ENGINE_PAR_RV2_TYPE` picks: Room (`$NI_REVERB2_TYPE_ROOM`,
/// 0) and Hall (1), as the saved room type numbers them (both values measured
/// in Kontakt 8.13.1, ksp_constants.py).
pub fn reverb_networks() -> [ir::Network; 2] {
    [ir::Network::Lines(&ROOM), ir::Network::Lines(&HALL)]
}

/// The reverb's controls as KSP's `$ENGINE_PAR_RV2_*` set them (value ÷ 1e6 is
/// the knob position), keyed by control identity.
pub fn reverb_laws() -> ir::ReverbControls<ir::Law> {
    ir::ReverbControls {
        // 0.5·e^(3.7x) s
        seconds: ir::Law::Exp {
            lo: 0.5,
            hi: 0.5 * 3.7f32.exp(),
        },
        // 3x + 1
        size: ir::Law::Linear { lo: 1.0, hi: 4.0 },
        damping: ir::Law::Power {
            lo: 0.0,
            hi: 1.0,
            exponent: 0.25,
        },
        diffusion: ir::Law::Linear { lo: 0.0, hi: 1.0 },
        // 11025x samples at 44.1 kHz
        predelay: ir::Law::Linear { lo: 0.0, hi: 250.0 },
        // 19000(1−x)+2000 Hz
        high_cut: ir::Law::Linear {
            lo: 21_000.0,
            hi: 2_000.0,
        },
        low_shelf: ir::Law::Linear { lo: 0.0, hi: -12.0 },
        stereo: ir::Law::Linear { lo: 0.0, hi: 1.0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapped_controls_keep_measured_laws_and_vendor_names() {
        use ir::{Drives, EnginePar, ReverbPar as P};
        let cases: [(P, &str, f32, f32, f32); 8] = [
            (
                P::Seconds,
                "$ENGINE_PAR_RV2_TIME",
                0.5,
                0.5 * 1.85_f32.exp(),
                0.5 * 3.7_f32.exp(),
            ),
            (P::Size, "$ENGINE_PAR_RV2_SIZE", 1.0, 2.5, 4.0),
            (
                P::Damping,
                "$ENGINE_PAR_RV2_DAMPING",
                0.0,
                0.5_f32.powf(0.25),
                1.0,
            ),
            (P::Diffusion, "$ENGINE_PAR_RV2_DIFF", 0.0, 0.5, 1.0),
            (P::Predelay, "$ENGINE_PAR_RV2_PREDELAY", 0.0, 125.0, 250.0),
            (
                P::HighCut,
                "$ENGINE_PAR_RV2_HIGHCUT",
                21_000.0,
                11_500.0,
                2_000.0,
            ),
            (P::LowShelf, "$ENGINE_PAR_RV2_LOWSHELF", 0.0, -6.0, -12.0),
            (P::Stereo, "$ENGINE_PAR_RV2_STEREO", 0.0, 0.5, 1.0),
        ];
        let mappings = crate::engine_pars();
        for (par, name, lo, mid, hi) in cases {
            assert!(
                mappings
                    .iter()
                    .any(|row| row.name == name
                        && row.drives == Drives::Engine(EnginePar::Reverb(par))),
                "{name}"
            );
            for (raw, want) in [(0.0, lo), (0.5, mid), (1.0, hi)] {
                let got = reverb_laws().get(par).eval(raw);
                assert!((got - want).abs() < 0.005, "{name} {raw}: {got} != {want}");
            }
        }
        assert!(!mappings.iter().any(|row| row.name == "$ENGINE_PAR_RV2_MOD"));
    }

    #[test]
    fn the_feedback_is_the_m_sequence_and_hall_lines_are_longer() {
        let room = network(false);
        let s: String = room.sequence.iter().map(|b| b.to_string()).collect();
        assert_eq!(s, "1101110101000010010110011111000");
        let hall = network(true);
        let mean = |n: &ir::ReverbNetwork| n.growth.iter().sum::<f32>();
        assert!(mean(&hall) > mean(&room));
        assert_eq!(hall.inputs[0][2], (4, 0.0));
    }
}
