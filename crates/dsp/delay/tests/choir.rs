//! Choral's choir pinned bit for bit, and its cost.

use kontra_dsp_core::Section;
use kontra_dsp_delay::{Choir, ChoirSettings};

/// Seven swept lines as Choral plays them: rest delays 7..25 ms, triangle
/// sweeps (two with the cosine on top), rounded, low-passed inputs, fed
/// back through the circulant, spread across the field.
fn settings(feedback: f32) -> ChoirSettings {
    let low = Section {
        hz: 6000.0,
        num: [1.0, 0.0, 0.0],
        den: [1.0, std::f32::consts::SQRT_2, 1.0],
    };
    let rate = [0.31, 0.47, 0.53, 0.71, 0.83, 0.97, 1.13];
    ChoirSettings {
        delay_s: [0.007, 0.011, 0.013, 0.017, 0.019, 0.023, 0.025],
        swing_s: rate.map(|hz| 0.002 / hz),
        lfo_hz: rate,
        start: [0.0, 0.14, 0.29, 0.43, 0.57, 0.71, 0.86],
        bend: [0.2; 7],
        wobble_hz: [0.0, 0.0, 1.7, 0.0, 0.0, 2.3, 0.0],
        wobble: [0.0, 0.0, 0.3, 0.0, 0.0, 0.2, 0.0],
        smooth_hz: [20.0; 7],
        filter: [low; 7],
        loop_gain: [feedback; 7],
        scatter: [0.0, 0.4, 0.1, 0.2, 0.1, 0.1, 0.1],
        sign: 1.0,
        input: 0.5,
        level: [0.4; 7],
        angle: [-1.2, -0.8, -0.4, 0.0, 0.4, 0.8, 1.2],
        bus: Section {
            hz: 200.0,
            num: [1.0, 1.0, 1.0],
            den: [1.0, 1.0, 1.0],
        },
        gain: 0.9,
    }
}

/// `seconds` of a two-sine input at 48 kHz in 512-frame blocks, the
/// feedback moved part way through.
fn render(seconds: usize) -> Vec<[f32; 2]> {
    let mut c = Choir::new(settings(0.3), 48_000);
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
            c.set(settings(0.6));
        }
        c.process(block);
    }
    io
}

fn bits_hash(x: &[[f32; 2]]) -> u64 {
    x.iter().flatten().fold(0xcbf2_9ce4_8422_2325, |h, v| {
        (h ^ u64::from(v.to_bits())).wrapping_mul(0x0100_0000_01b3)
    })
}

#[test]
fn the_reference_render_is_unchanged() {
    assert_eq!(
        bits_hash(&render(3)),
        16_864_327_713_616_686_334,
        "render changed"
    );
}

/// `perf stat` this for the cost: 20 s, 1875 blocks of 512.
#[test]
#[ignore = "a benchmark"]
fn choir_speed() {
    std::hint::black_box(render(20));
}
