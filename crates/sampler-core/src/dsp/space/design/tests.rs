use super::super::common::rt60;
use super::super::network::{Network as Engine, apply};
use super::*;
use sampler_ir::fx::space::DecayBands;

#[path = "galois_vectors.rs"]
mod vectors;

fn engine(n: &Network, rate: u32) -> Engine {
    Engine::new(spec(n, rate).unwrap())
}

/// Left impulse 1.0 at 0, right 0.5 at 5, in `block`-frame calls.
fn impulse(e: &mut Engine, frames: usize, block: usize) -> [Vec<f32>; 2] {
    let mut l = vec![0.0; frames];
    let mut r = vec![0.0; frames];
    l[0] = 1.0;
    r[5] = 0.5;
    for (a, b) in l.chunks_mut(block).zip(r.chunks_mut(block)) {
        e.process(a, b);
    }
    [l, r]
}

fn galois_net(hall: bool, controls: [f32; 10]) -> Network {
    Network { topology: Topology::Galois, galois: Some(Galois { hall, controls }), ..Network::DEFAULT }
}

// ------------------------------------------------------------------ Galois

/// The native kernel's impulse responses (`0x141ae2c60`, run in the emulator).
#[test]
fn galois_matches_native_impulse_responses() {
    for (name, (hall, controls, want)) in [
        ("room a", vectors::ROOM_A),
        ("hall a", vectors::HALL_A),
        ("room b", vectors::ROOM_B),
        ("hall b", vectors::HALL_B),
    ] {
        let got = impulse(&mut engine(&galois_net(hall, controls), 44_100), 8192, 32);
        let peak = want.iter().flatten().fold(0f32, |m, v| m.max(v.abs()));
        for ch in 0..2 {
            for (k, &w) in want[ch].iter().enumerate() {
                let g = got[ch][k * 64];
                assert!((g - w).abs() <= 1e-5 * peak.max(1e-3), "{name} ch{ch} sample {}: {g} vs native {w}", k * 64);
            }
        }
    }
}

/// `0x141aebb00`: lengths are primes picked by the size law.
#[test]
fn galois_line_lengths_follow_the_prime_law() {
    use galois_law::*;
    assert_eq!(primes(10), [2, 3, 5, 7, 11, 13, 17, 19, 23, 29]);
    let room = lengths(false, 0.0);
    assert_eq!(room[0], primes(95)[94]);
    assert_eq!(room[30], primes(394)[393]);
    let hall = lengths(true, 1.0);
    let k = ((30f32 * 0.031_25).powf(1.61) * 2944.0 + 93.0) as usize;
    assert_eq!(hall[30], primes(k + 1)[k]);
}

/// The Walsh–Hadamard path equals the dense circulant of the law, and that
/// matrix is orthogonal.
#[test]
fn galois_circulant_equals_the_dense_matrix() {
    use galois_law::*;
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
    let s = galois(&Galois { hall: false, controls: [0.5; 10] }, 44_100.0);
    let v: [f32; 32] = std::array::from_fn(|j| if j < LINES { (j as f32 * 0.37).sin() } else { 0.0 });
    let mut fast = [0f32; 32];
    apply(&s.matrix, &v, LINES, &mut fast);
    for i in 0..LINES {
        let dense: f32 = (0..LINES).map(|j| m(i, j) * v[j]).sum();
        assert!((fast[i] - dense).abs() < 1e-5, "row {i}: {} vs {dense}", fast[i]);
    }
}

/// Kontakt measurements (KONTAKT_REFERENCE s.21): at default damping the
/// RT60 is about 0.82 of the displayed Time (Room 3.2 s → 2.60 s, 1 s →
/// 0.84 s; Hall 3.2 s → 2.61 s), measured at 48 kHz where the 44.1 kHz core
/// is not decimated.
#[test]
fn galois_decay_matches_the_measured_rt60() {
    for (hall, time, rt60) in [(false, 3.2f32, 2.60f32), (false, 1.0, 0.84), (true, 3.2, 2.61)] {
        let x = (time * 2.0).ln() / 3.7;
        let mut e = engine(&galois_net(hall, [x, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 1.0, 1.0]), 48_000);
        let mut l = vec![0.0; 48_000 * 6];
        let mut r = vec![0.0; 48_000 * 6];
        l[0] = 1.0;
        for (a, b) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
            e.process(a, b);
        }
        let mut en: Vec<f64> = l.iter().rev().scan(0.0, |s, v| { *s += f64::from(v * v); Some(*s) }).collect();
        en.reverse();
        let db = |i: usize| 10.0 * (en[i] / en[0]).log10();
        let at = |d: f64| (0..en.len()).find(|&i| db(i) <= d).unwrap() as f32;
        let measured = (at(-35.0) - at(-5.0)) / 48_000.0 * 2.0;
        assert!((measured - rt60).abs() < 0.06 * rt60, "hall {hall} time {time}: RT60 {measured} vs Kontakt {rt60}");
    }
}

// ---------------------------------------------------------------- Freeverb

fn freeverb_net(width: f64, predelay: f64) -> Network {
    Network {
        topology: Topology::Freeverb,
        predelay_seconds: predelay,
        input_cutoff_hz: 192_000.0,
        freeverb: Some(Freeverb { feedback: 0.8, damp: 0.7, allpass: 0.5, input_gain: 0.015, width }),
        ..Network::DEFAULT
    }
}

fn left_impulse(n: &Network, rate: u32, frames: usize) -> [Vec<f32>; 2] {
    let mut e = engine(n, rate);
    let mut l = vec![0f32; frames];
    let mut r = vec![0f32; frames];
    l[0] = 1.0;
    e.process(&mut l, &mut r);
    [l, r]
}

/// Native vector: at 44.1 kHz the first comb (1116 samples) returns the
/// input scaled by 0.015 through four allpasses of g = 0.5, each passing
/// `-g` straight through.
#[test]
fn freeverb_first_arrival_and_lengths() {
    let [l, r] = left_impulse(&freeverb_net(1.0, 0.0), 44_100, 4000);
    assert!(l[..1116].iter().all(|v| *v == 0.0));
    let first = 0.015f32 * 0.5f32.powi(4);
    assert!((l[1116] - first).abs() < 1e-9, "{}", l[1116]);
    // Width 1 keeps the channels apart: the right input is silent.
    assert!(r.iter().all(|v| *v == 0.0));
}

#[test]
fn freeverb_predelay_shifts_the_arrival() {
    // 10 ms at 44.1 kHz is 441 samples.
    let [l, _] = left_impulse(&freeverb_net(1.0, 0.010), 44_100, 2000);
    assert_eq!(l[..1116 + 441].iter().position(|v| *v != 0.0), None);
    assert!(l[1116 + 441] != 0.0);
}

/// Width 0.5 mixes the channels fully: both outputs carry the mean; the
/// right channel's lines are 23 samples longer.
#[test]
fn freeverb_channel_mix_and_spread() {
    let [l, r] = left_impulse(&freeverb_net(0.5, 0.0), 44_100, 2000);
    assert!((l[1116] - 0.015 * 0.5 * 0.5f32.powi(4)).abs() < 1e-9);
    assert!(r[1116 + 23] != 0.0);
    assert!(r[1116 + 22] == 0.0);
}

// ------------------------------------------------------ UVI legacy reverbs

fn jezar_net(decay_time: f32, cutoff: f32) -> Network {
    Network { topology: Topology::Jezar, uvi_reverb: Some(UviReverb { decay_time, high_damp: 0.0, cutoff }), ..Network::DEFAULT }
}

/// `(int)(rate·k)` truncates the f32 constants below their tuning at 44.1 kHz.
#[test]
fn jezar_lengths_and_feedback_match_native_vectors() {
    use jezar_law::*;
    let at = |l| length(l, 44_100.0);
    assert_eq!([at(1116), at(1580), at(1617), at(464), at(441)], [1116, 1579, 1616, 463, 441]);
    assert!((feedback(2.0) - 0.825_219_8).abs() < 1e-6, "{}", feedback(2.0));
}

/// Cutoff -0.5 is a 5595.19 Hz low-pass, 0.5 a 413.44 Hz high-pass.
#[test]
fn jezar_cutoff_follows_the_legacy_table() {
    let hz = |c: [f32; 5]| (((1.0 - c[4]) / (1.0 + c[4])).asin() * 44_100.0 / std::f32::consts::TAU, (c[0] + c[1] + c[2]) / (1.0 + c[3] + c[4]));
    let (lp, dc) = hz(jezar_law::cutoff(-0.5, 44_100.0).unwrap());
    assert!((lp - 5595.19).abs() < 0.05 && (dc - 1.0).abs() < 1e-5, "{lp} {dc}");
    let (hp, dc) = hz(jezar_law::cutoff(0.5, 44_100.0).unwrap());
    assert!((hp - 413.44).abs() < 0.05 && dc.abs() < 1e-5, "{hp} {dc}");
    assert!(jezar_law::cutoff(0.0, 44_100.0).is_none());
}

/// The channel mean × 0.015 reaches the first comb of each side; four
/// Jezar allpasses pass `-x` straight through.
#[test]
fn jezar_first_arrival() {
    let [l, r] = left_impulse(&jezar_net(2.0, 0.0), 44_100, 2000);
    assert_eq!(l.iter().position(|v| *v != 0.0), Some(1116));
    assert_eq!(r.iter().position(|v| *v != 0.0), Some(1139));
    assert_eq!((l[1116], r[1139]), (0.0075, 0.0075));
}

// --------------------------------------------------------------- SparkVerb

fn spark_controls() -> SparkVerb {
    SparkVerb {
        quality: 3,
        room_m: 20.0,
        shape: 0.0,
        decay: 1.0,
        decay_low: 2.0,
        decay_high: 0.5,
        freq_low: 250.0,
        freq_high: 12_000.0,
        low_cut: false,
        high_cut: false,
        mod_cents: 4.0,
        mod_hz: 1.0,
        mode: 1,
        freeze: false,
        diffuser: Some((0.618, 5.0)),
        width: 0.5,
        rolloff_hz: 20_000.0,
        predelay_seconds: 0.0,
    }
}

/// `0x14167f190` vectors (RoomSize, Shape, rate, N).
#[test]
fn spark_lengths_match_native_vectors() {
    use spark_law::lengths;
    assert_eq!(lengths(8, 20.0, 0.0, 48_000.0), [1811, 1993, 2203, 2437, 2683, 2963, 3271, 3613]);
    assert_eq!(lengths(8, 20.0, 0.0, 44_100.0), [1663, 1831, 2027, 2237, 2467, 2729, 3011, 3319]);
    assert_eq!(lengths(8, 20.0, 0.3, 48_000.0), [1439, 1693, 1973, 2311, 2711, 3181, 3719, 4357]);
    assert_eq!(
        lengths(16, 20.0, 0.0, 48_000.0),
        [1877, 1973, 2063, 2153, 2267, 2371, 2473, 2591, 2713, 2843, 2999, 3119, 3271, 3433, 3581, 3761]
    );
    assert_eq!(lengths(4, 4.0, 0.0, 48_000.0), [347, 431, 541, 683]);
}

/// `0x141649120` vectors and the decay / input-gain / modulation / output
/// laws at 48 kHz, RoomSize 20, Decay 1, N 8.
#[test]
fn spark_coefficients_match_native_vectors() {
    use spark_law::shelf;
    let close = |a: f64, b: f64, tol: f64| assert!((a - b).abs() < tol, "{a} vs {b}");
    let s = shelf(250.0 / 48_000.0, 0.5, 1.0);
    [0.988_691, -0.966_072, -0.954_763].iter().zip(s).for_each(|(&b, a)| close(a, b, 2e-6));
    let s = shelf(0.25, 1.0, 0.5);
    [0.707_107, 0.121_320, -0.171_573].iter().zip(s).for_each(|(&b, a)| close(a, b, 2e-6));
    let v = spark(&spark_controls(), 48_000.0);
    close(f64::from(v.gain), 0.429_786_3, 1e-6);
    close(f64::from(v.lines[0].a) * 8f64.sqrt(), 0.877_822, 1e-6);
    close(v.lines[0].depth, 10.6552, 1e-4);
    close(v.lines[7].depth, 21.2575, 1e-4);
    close(f64::from(v.out[0][0]), 0.5 * 0.923_88, 1e-6);
    close(f64::from(v.out[0][1]), 0.5 * 0.382_683, 1e-6);
    assert_eq!(v.diffuser[0].iter().map(|a| a.len).collect::<Vec<_>>(), [240, 400, 592, 953]);
    assert_eq!(v.diffuser[1].iter().map(|a| a.len).collect::<Vec<_>>(), [263, 377, 615, 930]);
    let x = -600.0 * 1811.0 / (48_000.0 * 20.0);
    close(x, -1.131_875, 1e-6);
    close(10f64.powf(0.05 * x / 2.0), 0.936_921, 1e-6);
    close(10f64.powf(0.05 * x / 0.5), 0.770_571, 1e-6);
}

#[test]
fn spark_decays_and_stays_finite() {
    let s = SparkVerb {
        quality: 4,
        room_m: 50.0,
        shape: 1.0,
        decay: 10.0,
        decay_low: 10.0,
        decay_high: 0.1,
        freq_low: 10.0,
        freq_high: 20_000.0,
        low_cut: true,
        high_cut: true,
        mod_cents: 20.0,
        mod_hz: 4.0,
        mode: 2,
        freeze: false,
        diffuser: Some((1.0, 10.0)),
        width: 0.0,
        rolloff_hz: 2_000.0,
        predelay_seconds: 0.1,
    };
    let mut e = Engine::new(spark(&s, 96_000.0));
    let (mut l, mut r) = (vec![0.0; 96_000], vec![0.0; 96_000]);
    l[0] = 1.0;
    e.process(&mut l, &mut r);
    assert!(l.iter().chain(&r).all(|x| x.is_finite() && x.abs() < 4.0));
    assert!(l[20_000..].iter().any(|x| x.abs() > 1e-6));
}

// --------------------------------------------------------------- Diffusion

/// `0x141310da0` lengths (48 kHz, 5 ms): base [240, 754, 2369, 7442] plus
/// the stereo offsets; the allpass chain keeps an impulse's energy.
#[test]
fn diffusion_lengths_and_energy() {
    let s = diffusion(0.618, 5.0, 48_000.0);
    assert_eq!(s.diffuser[0].iter().map(|a| a.len).collect::<Vec<_>>(), [240, 777, 2369, 7465]);
    assert_eq!(s.diffuser[1].iter().map(|a| a.len).collect::<Vec<_>>(), [263, 754, 2392, 7442]);
    let mut e = Engine::new(s);
    let (mut l, mut r) = (vec![0.0; 96_000], vec![0.0; 96_000]);
    l[0] = 1.0;
    e.process(&mut l, &mut r);
    let energy: f32 = l.iter().map(|v| v * v).sum();
    assert!((energy - 1.0).abs() < 1e-3, "{energy}");
    assert!(r.iter().all(|v| *v == 0.0));
    // Each lattice passes `g·x` straight through.
    assert!((l[0] - 0.618f32.powi(4)).abs() < 1e-6, "{}", l[0]);
}

// --------------------------------------------------------- generic presets

fn stereo_impulse(n: &Network, rate: u32, seconds: usize) -> (Vec<f32>, Vec<f32>) {
    let mut e = engine(n, rate);
    let mut l = vec![0.0; seconds * rate as usize];
    let mut r = l.clone();
    l[0] = 1.0;
    r[0] = 1.0;
    for (a, b) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
        e.process(a, b);
    }
    (l, r)
}

#[test]
fn generic_presets_decay_at_the_requested_rate() {
    for topology in [Topology::Fdn, Topology::CombAllpass, Topology::Plate] {
        for target in [1.0, 3.0] {
            let n = Network { topology, decay_seconds: target, damping_hz: 12_000.0, ..Network::DEFAULT };
            let (l, r) = stereo_impulse(&n, 48_000, 10);
            assert!(l.iter().chain(&r).all(|v| v.is_finite()));
            let got = rt60(&l, 48_000.0);
            assert!((got / target as f32 - 1.0).abs() < 0.4, "{topology:?} target {target} got {got}");
        }
    }
}

#[test]
fn generic_presets_are_stable_at_the_longest_decay() {
    for topology in [Topology::Fdn, Topology::CombAllpass, Topology::Plate] {
        let n = Network { topology, decay_seconds: 60.0, ..Network::DEFAULT };
        let mut e = engine(&n, 44_100);
        let (mut l, mut r) = (vec![0.5; 256], vec![-0.5; 256]);
        for _ in 0..500 {
            e.process(&mut l, &mut r);
            assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 100.0), "{topology:?}");
            l.fill(0.0);
            r.fill(0.0);
        }
    }
}

#[test]
fn generic_low_band_decays_slower_when_asked() {
    let bands = DecayBands { low: 3.0, high: 1.0, low_hz: 400.0, high_hz: 8_000.0 };
    let flat = Network { decay_seconds: 0.6, size: 0.75, ..Network::DEFAULT };
    let (a, _) = stereo_impulse(&flat, 48_000, 3);
    let (b, _) = stereo_impulse(&Network { bands: Some(bands), ..flat }, 48_000, 3);
    let late = |v: &[f32]| v[48_000..].iter().map(|x| x * x).sum::<f32>();
    assert!(late(&b) > late(&a) * 2.0, "{} vs {}", late(&b), late(&a));
}
