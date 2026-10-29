use kontra_dsp_reverb::Convolver;

/// Deterministic noise in [-1, 1) (LCG), so tests need no RNG crate.
fn noise(seed: &mut u64, len: usize, scale: f32) -> Vec<f32> {
    (0..len)
        .map(|_| {
            *seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((*seed >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0) * scale
        })
        .collect()
}

fn naive(ir: &[f32], x: &[f32]) -> Vec<f32> {
    (0..x.len())
        .map(|n| x.iter().take(n + 1).rev().zip(ir).map(|(v, h)| h * v).sum())
        .collect()
}

/// Runs `frames` through `conv` in blocks of the given sizes, cycling.
fn run(conv: &mut Convolver, frames: &[[f32; 2]], sizes: &[usize]) -> Vec<[f32; 2]> {
    let mut out = frames.to_vec();
    let mut rest = out.as_mut_slice();
    for n in sizes.iter().cycle() {
        if rest.is_empty() {
            break;
        }
        let (block, tail) = rest.split_at_mut((*n).min(rest.len()));
        conv.process(block);
        rest = tail;
    }
    out
}

#[test]
fn matches_naive_convolution() {
    let mut seed = 0x5eed;
    // Every stage boundary: 64, 512, 2048 and 8192.
    for taps in [1, 63, 64, 65, 200, 511, 513, 2047, 2049, 8191, 12_000] {
        let scale = 1.0 / (taps as f32).sqrt();
        let left = noise(&mut seed, taps, scale);
        let right = noise(&mut seed, taps, scale);
        let xl = noise(&mut seed, 16_000, 1.0);
        let xr = noise(&mut seed, 16_000, 1.0);
        let frames: Vec<[f32; 2]> = xl.iter().zip(&xr).map(|(l, r)| [*l, *r]).collect();

        let mut conv = Convolver::new(&left, &right);
        let out = run(&mut conv, &frames, &[1, 7, 64, 100, 257, 13, 4096]);

        let want_l = naive(&left, &xl);
        let want_r = naive(&right, &xr);
        for (n, (frame, (wl, wr))) in out.iter().zip(want_l.iter().zip(&want_r)).enumerate() {
            assert!((frame[0] - wl).abs() < 1e-4, "taps {taps} L at {n}");
            assert!((frame[1] - wr).abs() < 1e-4, "taps {taps} R at {n}");
        }
    }
}

#[test]
fn unit_impulse_is_exact_passthrough() {
    let mut seed = 7;
    let mut conv = Convolver::new(&[1.0], &[1.0]);
    let xl = noise(&mut seed, 1000, 1.0);
    let xr = noise(&mut seed, 1000, 1.0);
    let frames: Vec<[f32; 2]> = xl.iter().zip(&xr).map(|(l, r)| [*l, *r]).collect();
    let out = run(&mut conv, &frames, &[1, 64, 100]);
    assert_eq!(out, frames);
}

#[test]
fn silence_detection_and_reset() {
    let mut seed = 99;
    let mut conv = Convolver::new(&noise(&mut seed, 200, 0.1), &noise(&mut seed, 200, 0.1));
    assert!(!conv.is_silent());

    let loud: Vec<[f32; 2]> = noise(&mut seed, 500, 1.0)
        .into_iter()
        .map(|x| [x, x])
        .collect();
    conv.process(&mut loud.clone());
    assert!(!conv.is_silent());

    let quiet = vec![[0.0f32; 2]; 199];
    conv.process(&mut quiet.clone());
    assert!(!conv.is_silent());
    conv.process(&mut [[0.0f32; 2]]);
    assert!(conv.is_silent());

    conv.reset();
    assert!(!conv.is_silent());
    let mut zeros = vec![[0.0f32; 2]; 300];
    conv.process(&mut zeros);
    assert!(zeros.iter().all(|f| f[0] == 0.0 && f[1] == 0.0));
}

/// Bench: a 3 s decaying response over 10 s of noise in 256-frame blocks;
/// prints ns per stereo frame and the worst error against a direct f64
/// convolution. `cargo test --release -p kontra-dsp-reverb --test convolver
/// bench -- --ignored --nocapture`.
#[test]
#[ignore = "bench"]
fn bench() {
    let mut seed = 0xbe9c;
    let rate = 48_000;
    let taps = std::env::var("BENCH_TAPS").map_or(3 * rate, |t| t.parse().unwrap_or(3 * rate));
    let ir: Vec<f32> = noise(&mut seed, taps, 0.05)
        .iter()
        .enumerate()
        .map(|(n, h)| h * (-6.9 * n as f32 / taps as f32).exp())
        .collect();
    let x = noise(&mut seed, 10 * rate, 0.5);
    let frames: Vec<[f32; 2]> = x.iter().map(|&v| [v, -v]).collect();
    // The best of five, as the machine is shared; and the slowest block.
    let (mut ns, mut worst_block) = (f64::MAX, f64::MAX);
    let mut out = Vec::new();
    for _ in 0..5 {
        let mut conv = Convolver::new(&ir, &ir);
        out = frames.clone();
        let (started, mut slowest) = (std::time::Instant::now(), 0f64);
        for block in out.chunks_mut(256) {
            let t = std::time::Instant::now();
            conv.process(block);
            slowest = slowest.max(t.elapsed().as_secs_f64() * 1e6);
        }
        ns = ns.min(started.elapsed().as_nanos() as f64 / frames.len() as f64);
        worst_block = worst_block.min(slowest);
    }
    let mut worst = 0f64;
    for n in (0..frames.len()).step_by(frames.len() / 97) {
        let direct: f64 = (0..=n.min(ir.len() - 1))
            .map(|k| f64::from(ir[k]) * f64::from(x[n - k]))
            .sum();
        worst = worst.max((f64::from(out[n][0]) - direct).abs());
    }
    println!(
        "{ns:.1} ns/frame, slowest 256-frame block {worst_block:.0} µs, worst error {worst:.2e}"
    );
}
