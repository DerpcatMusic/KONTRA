use super::*;

#[path = "vectors.rs"]
mod vectors;

fn run(room: bool, controls: [f32; 10], frames: usize) -> [Vec<f32>; 2] {
    let mut core = GaloisCore::with(&Controls { room, controls }, 44_100.0);
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
    for (name, (room, controls, want)) in [
        ("hall a", vectors::HALL_A),
        ("room a", vectors::ROOM_A),
        ("hall b", vectors::HALL_B),
        ("room b", vectors::ROOM_B),
    ] {
        let got = run(room, controls, 8192);
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
    let hall = lengths(false, 0.0);
    // size 1: idx = slope·10 + 90 + base.
    assert_eq!(hall[0], primes(95)[94]);
    assert_eq!(hall[30], primes(394)[393]);
    let room = lengths(true, 1.0);
    let k = ((30f32 * 0.031_25).powf(1.61) * 2944.0 + 93.0) as usize;
    assert_eq!(room[30], primes(k + 1)[k]);
}

/// The Walsh–Hadamard path equals the dense circulant matrix of the law, and
/// that matrix is orthogonal.
#[test]
fn hadamard_mix_equals_the_dense_matrix() {
    let mut core = GaloisCore::with(&Controls { room: false, controls: [0.5; 10] }, 44_100.0);
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
