//! The comb network: its tail falls at the seconds it is set to, and the
//! width mixes the channels toward their mean.
#![cfg(test)]

use kontra_dsp_reverb::{Combs, Settings};
use kontra_law::CombNetwork;

static LAW: CombNetwork = CombNetwork {
    combs_ms: [25.3, 26.9, 29.0, 30.7, 32.2, 33.8, 35.3, 36.7],
    allpasses_ms: [12.6, 10.0, 7.7, 5.1],
    spread_ms: 0.52,
    input_gain: 0.015,
};

fn settings(seconds: f32, stereo: f32) -> Settings {
    Settings {
        seconds,
        size: 1.0,
        damping: 0.0,
        diffusion: 0.5,
        predelay_ms: 0.0,
        high_cut_hz: 24_000.0,
        low_shelf_db: 0.0,
        stereo,
        modulation_ms: 0.0,
        modulation_hz: 0.0,
    }
}

/// RMS of the left channel over `frames`, after an impulse into `left`.
fn tail(seconds: f32, stereo: f32, input: [f32; 2], frames: std::ops::Range<usize>) -> [f64; 2] {
    let mut r = Combs::new(&LAW, settings(seconds, stereo), 48_000);
    let mut io = vec![[0.0f32; 2]; frames.end];
    io[0] = input;
    r.process(&mut io);
    let rms = |c: usize| {
        (io[frames.clone()]
            .iter()
            .map(|f| f64::from(f[c]).powi(2))
            .sum::<f64>()
            / frames.len() as f64)
            .sqrt()
    };
    [rms(0), rms(1)]
}

#[test]
fn the_tail_falls_60_db_in_its_seconds() {
    let early = tail(1.0, 1.0, [1.0, 1.0], 4_800..9_600)[0];
    let late = tail(1.0, 1.0, [1.0, 1.0], 52_800..57_600)[0];
    let db = 20.0 * (late / early).log10();
    assert!((db + 60.0).abs() < 6.0, "{db}");
}

#[test]
fn full_width_keeps_the_channels_apart_and_none_mixes_them() {
    let [l, r] = tail(1.0, 1.0, [1.0, 0.0], 0..9_600);
    assert!(l > 0.0 && r == 0.0, "{l} {r}");
    let [l, r] = tail(1.0, 0.0, [1.0, 0.0], 0..9_600);
    assert!(r > 0.5 * l, "{l} {r}");
}
