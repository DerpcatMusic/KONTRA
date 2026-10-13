use super::*;

#[path = "vectors.rs"]
mod vectors;

fn run(hall: bool, controls: [f32; 10], frames: usize) -> [Vec<f32>; 2] {
    run_at(44_100.0, hall, controls, frames)
}

fn run_at(rate: f64, hall: bool, controls: [f32; 10], frames: usize) -> [Vec<f32>; 2] {
    let mut core = GaloisCore::with(&Controls { hall, controls }, rate);
    let mut l = vec![0.0; frames];
    let mut r = vec![0.0; frames];
    l[0] = 1.0;
    r[5] = 0.5;
    for (a, b) in l.chunks_mut(32).zip(r.chunks_mut(32)) {
        core.process(a, b);
    }
    [l, r]
}

/// The native kernel's impulse responses (`0x141ae2c60`, run in the emulator).
#[test]
fn matches_native_impulse_responses() {
    for (name, (hall, controls, want)) in [
        ("room a", vectors::ROOM_A),
        ("hall a", vectors::HALL_A),
        ("room b", vectors::ROOM_B),
        ("hall b", vectors::HALL_B),
    ] {
        let got = run(hall, controls, 8192);
        let peak = want.iter().flatten().fold(0f32, |m, v| m.max(v.abs()));
        for ch in 0..2 {
            for (k, &w) in want[ch].iter().enumerate() {
                let g = got[ch][k * 64];
                assert!((g - w).abs() <= 1e-5 * peak.max(1e-3), "{name} ch{ch} sample {}: {g} vs native {w}", k * 64);
            }
        }
    }
}

/// `0x141aebb00`: lengths are primes picked by the size law (spot values read
/// from the native object).
#[test]
fn line_lengths_follow_the_prime_law() {
    let p = primes(10);
    assert_eq!(p, [2, 3, 5, 7, 11, 13, 17, 19, 23, 29]);
    let room = lengths(false, 0.0);
    // size 1: idx = slope·10 + 90 + base.
    assert_eq!(room[0], primes(95)[94]);
    assert_eq!(room[30], primes(394)[393]);
    let hall = lengths(true, 1.0);
    let k = ((30f32 * 0.031_25).powf(1.61) * 2944.0 + 93.0) as usize;
    assert_eq!(hall[30], primes(k + 1)[k]);
}

/// The Walsh–Hadamard path equals the dense circulant matrix of the law, and
/// that matrix is orthogonal.
#[test]
fn hadamard_mix_equals_the_dense_matrix() {
    let mut core = GaloisCore::with(&Controls { hall: false, controls: [0.5; 10] }, 44_100.0);
    let c = sequence();
    assert_eq!(c.map(|b| b.to_string()).concat(), "1101110101000010010110011111000");
    let (a, b) = matrix_ab();
    assert!((a - 0.150_221_11).abs() < 1e-7 && (b + 0.203_332_28).abs() < 1e-7);
    let m = |i: usize, j: usize| if c[(j + LINES - i) % LINES] == 0 { a } else { b };
    for i in 0..LINES {
        let dot: f32 = (0..LINES).map(|j| m(i, j) * m((i + 3) % LINES, j)).sum();
        let norm: f32 = (0..LINES).map(|j| m(i, j) * m(i, j)).sum();
        assert!(dot.abs() < 1e-6 && (norm - 1.0).abs() < 1e-6);
    }
    for (j, v) in core.v.iter_mut().take(LINES).enumerate() {
        *v = (j as f32 * 0.37).sin();
    }
    let fast = core.mix();
    for i in 0..LINES {
        let dense: f32 = (0..LINES).map(|j| m(i, j) * core.v[j]).sum();
        assert!((fast[i] - dense).abs() < 1e-5, "row {i}: {} vs {dense}", fast[i]);
    }
}


/// Kontakt measurements (KONTAKT_REFERENCE s.21): at default damping the RT60
/// is about 0.82 of the displayed Time for both modes (Room 3.2 s → 2.60 s,
/// 1 s → 0.84 s; Hall 3.2 s → 2.61 s). The rig ran at 48 kHz, where the
/// core, tuned for 44.1 kHz and not decimated below 2×, decays 44.1/48 faster.
#[test]
fn decay_matches_the_measured_rt60() {
    for (hall, time, rt60) in [(false, 3.2f32, 2.60f32), (false, 1.0, 0.84), (true, 3.2, 2.61)] {
        let x = (time * 2.0).ln() / 3.7;
        let [l, _] = run_at(48_000.0, hall, [x, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 1.0, 1.0], 48_000 * 6);
        // Schroeder integral, -5..-35 dB fit, extrapolated to -60.
        let mut e: Vec<f64> = l.iter().rev().scan(0.0, |s, v| { *s += f64::from(v * v); Some(*s) }).collect();
        e.reverse();
        let db = |i: usize| 10.0 * (e[i] / e[0]).log10();
        let at = |d: f64| (0..e.len()).find(|&i| db(i) <= d).unwrap() as f32;
        let measured = (at(-35.0) - at(-5.0)) / 48_000.0 * 2.0;
        assert!((measured - rt60).abs() < 0.06 * rt60, "hall {hall} time {time}: RT60 {measured} vs Kontakt {rt60}");
    }
}
