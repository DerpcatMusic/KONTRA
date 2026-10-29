//! The Hadamard network: its mid band falls at the seconds it is set to, and
//! the low band at its own decay.
#![cfg(test)]

use kontra_dsp_reverb::{Hadamard, Settings};
use kontra_law::HadamardNetwork;

const RATE: u32 = 48_000;

fn law(decay_low: f32) -> HadamardNetwork {
    HadamardNetwork {
        lines: 8,
        shape: 0.0,
        diffusers: true,
        diffuser_ms: 5.0,
        decay_low,
        decay_high: 1.0,
        low_hz: 250.0,
        high_hz: 12_000.0,
        low_cut: false,
        high_cut: false,
        freeze: false,
    }
}

fn settings(seconds: f32) -> Settings {
    Settings {
        seconds,
        size: 20.0 / 340.0,
        damping: 0.0,
        diffusion: 0.618,
        predelay_ms: 0.0,
        high_cut_hz: 20_000.0,
        low_shelf_db: 0.0,
        stereo: 1.0,
        modulation_ms: 0.5,
        modulation_hz: 1.06,
    }
}

/// Left-channel energy in dB of each 0.2 s window after an impulse.
fn windows(law: HadamardNetwork, seconds: f32, count: usize) -> Vec<f32> {
    let mut h = Hadamard::new(law, settings(seconds), RATE);
    let window = RATE as usize / 5;
    let mut io = vec![[0.0f32; 2]; window * count];
    io[0] = [1.0, 1.0];
    for block in io.chunks_mut(256) {
        h.process(block);
    }
    io.chunks(window)
        .map(|w| 10.0 * w.iter().map(|f| f[0] * f[0]).sum::<f32>().log10())
        .collect()
}

#[test]
fn the_tail_falls_sixty_db_in_its_seconds() {
    for lines in [8, 16] {
        let db = windows(HadamardNetwork { lines, ..law(1.0) }, 2.0, 10);
        // 1 s apart: 30 dB down.
        let drop = db[2] - db[7];
        assert!((drop - 30.0).abs() < 3.0, "{lines}: {db:?}");
    }
}

#[test]
fn a_short_low_decay_thins_the_tail() {
    let even = windows(law(1.0), 2.0, 8);
    let thin = windows(law(0.169), 2.0, 8);
    assert!(thin[6] < even[6] - 3.0, "{even:?} {thin:?}");
    assert!(thin[0].is_finite() && thin[0] > -40.0, "{thin:?}");
}
