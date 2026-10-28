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
pub(crate) fn reverb(kind: i32, v: [f32; 9]) -> ir::Reverb {
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
    let [
        seconds,
        size_law,
        damping_law,
        diffusion_law,
        predelay_law,
        high_cut_law,
        low_shelf_law,
        stereo_law,
    ] = reverb_laws();
    ir::Reverb {
        seconds: seconds.eval(time),
        size: size_law.eval(size),
        network: Some(if kind == 1 { &*HALL } else { &*ROOM }),
        damping: damping_law.eval(damping),
        diffusion: diffusion_law.eval(diffusion),
        predelay_ms: predelay_law.eval(predelay),
        high_cut_hz: high_cut_law.eval(high_cut),
        low_shelf_db: low_shelf_law.eval(low_shelf),
        stereo: stereo_law.eval(stereo),
        modulation_ms: modulation * 10.0 / 44.1,
        modulation_hz: 2.0,
    }
}

/// The reverb's controls as KSP's `$ENGINE_PAR_RV2_*` set them (value ÷ 1e6 is
/// the knob position), in [`ir::ReverbPar::ALL`] order.
pub fn reverb_laws() -> [ir::Law; 8] {
    [
        // 0.5·e^(3.7x) s
        ir::Law::Exp {
            lo: 0.5,
            hi: 0.5 * 3.7f32.exp(),
        },
        // 3x + 1
        ir::Law::Linear { lo: 1.0, hi: 4.0 },
        ir::Law::Power {
            lo: 0.0,
            hi: 1.0,
            exponent: 0.25,
        },
        ir::Law::Linear { lo: 0.0, hi: 1.0 },
        // 11025x samples at 44.1 kHz
        ir::Law::Linear { lo: 0.0, hi: 250.0 },
        // 19000(1−x)+2000 Hz
        ir::Law::Linear {
            lo: 21_000.0,
            hi: 2_000.0,
        },
        ir::Law::Linear { lo: 0.0, hi: -12.0 },
        ir::Law::Linear { lo: 0.0, hi: 1.0 },
    ]
}

#[cfg(test)]
#[path = "reverb_vectors.rs"]
mod vectors;

#[cfg(test)]
mod tests {
    use super::*;
    use kontra_dsp_reverb::{Network, Settings};

    fn engine(kind: i32, v: [f32; 9], rate: u32) -> Network {
        let r = reverb(kind, v);
        let s = Settings {
            seconds: r.seconds,
            size: r.size,
            damping: r.damping,
            diffusion: r.diffusion,
            predelay_ms: r.predelay_ms,
            high_cut_hz: r.high_cut_hz,
            low_shelf_db: r.low_shelf_db,
            stereo: r.stereo,
            modulation_ms: r.modulation_ms,
            modulation_hz: r.modulation_hz,
        };
        Network::new(r.network.unwrap(), s, rate)
    }

    /// The emulated native kernel (`0x141ae2c60`) at 44.1 kHz: left impulse
    /// 1.0 at 0, right 0.5 at 5, its own mix `m` (`sin(mπ/2)` wet, `cos` dry).
    #[test]
    fn room_and_hall_match_the_native_kernel() {
        for (name, (hall, c, want)) in [
            ("room a", vectors::ROOM_A),
            ("hall a", vectors::HALL_A),
            ("room b", vectors::ROOM_B),
            ("hall b", vectors::HALL_B),
        ] {
            let v = [c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[9]];
            let (wet, dry) = (c[8] * std::f32::consts::FRAC_PI_2).sin_cos();
            let mut io = vec![[0.0f32; 2]; 8192];
            io[0][0] = 1.0;
            io[5][1] = 0.5;
            let input = io.clone();
            engine(i32::from(hall), v, 44_100).process(&mut io);
            let peak = want.iter().flatten().fold(0f32, |m, v| m.max(v.abs()));
            let mut worst = 0f32;
            for ch in 0..2 {
                for (k, &w) in want[ch].iter().enumerate() {
                    let g = io[k * 64][ch] * wet + input[k * 64][ch] * dry;
                    worst = worst.max((g - w).abs() / peak.max(1e-3));
                }
            }
            // The controls pass through physical units (ms, the Exp law), an
            // ulp off the native's own f32 arithmetic; Room B drifts most.
            assert!(worst <= 2e-5, "{name}: {worst:e} of peak");
        }
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
