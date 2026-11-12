//! Phasis's output pinned bit for bit, and its cost.

use kontra_dsp_filter::{NotchSettings, Notches};

/// A swept phaser as Phasis plays: six notches, the LFO moving the corner
/// and the damping, the loop fed back.
fn settings(stages: u8) -> NotchSettings {
    NotchSettings {
        stages,
        hz: 700.0,
        octaves: 3.5,
        spread: 0.3,
        spread_swing: 1.2,
        spread_limit: 2.7,
        feedback: 0.6,
        loop_width: 1.6,
        loop_top: 0.4,
        lfo_hz: 0.7,
        rounding: 0.3,
        depth: 1.0,
        warp: 0.4,
        phase: 0.1,
        sign: 1.0,
        mix: 0.8,
        low_cut_hz: 5.0,
        internal_hz: 90_000.0,
    }
}

/// `seconds` of a two-sine input at 48 kHz in 512-frame blocks, with the
/// stage count moved part way through.
fn render(seconds: usize) -> Vec<[f32; 2]> {
    render_at(seconds, 0.7)
}

fn render_at(seconds: usize, lfo_hz: f32) -> Vec<[f32; 2]> {
    let at = |stages| NotchSettings {
        lfo_hz,
        ..settings(stages)
    };
    let mut n = Notches::new(at(6), 48_000);
    let mut io: Vec<[f32; 2]> = (0..seconds * 48_000)
        .map(|i| {
            let t = i as f32 / 48_000.0;
            let l = 0.5 * (std::f32::consts::TAU * 220.0 * t).sin();
            let r = 0.3 * (std::f32::consts::TAU * 1330.0 * t).sin();
            [l, l + r]
        })
        .collect();
    for (k, block) in io.chunks_mut(512).enumerate() {
        if k == 40 {
            n.set(at(12));
        }
        if k == 80 {
            n.set(at(4));
        }
        n.process(block);
    }
    io
}

fn bits_hash(x: &[[f32; 2]]) -> u64 {
    x.iter().flatten().fold(0xcbf2_9ce4_8422_2325, |h, v| {
        (h ^ u64::from(v.to_bits())).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Pinned with the LFO and coefficients held for 8 ticks at the middle of
/// their run: against computing them every tick it differs by −84.5 dB RMS
/// here (−63.6 dB with a 10 Hz LFO).
#[test]
fn the_reference_render_is_unchanged() {
    assert_eq!(
        bits_hash(&render(3)),
        3_104_610_534_117_950_367,
        "render changed"
    );
}

/// `perf stat` this for the cost: 20 s, 1875 blocks of 512.
#[test]
#[ignore = "a benchmark"]
fn notches_speed() {
    std::hint::black_box(render(20));
}
