use kontra_dsp_dynamics::{Compressor, CompressorSettings, Limiter, LimiterSettings};

const RATE: u32 = 48_000;

fn sine(amplitude: f32, frames: usize) -> Vec<[f32; 2]> {
    (0..frames)
        .map(|i| {
            let v =
                amplitude * (i as f32 * 2.0 * std::f32::consts::PI * 1_000.0 / RATE as f32).sin();
            [v, v]
        })
        .collect()
}

fn peak_db(io: &[[f32; 2]]) -> f32 {
    let p = io
        .iter()
        .map(|f| f[0].abs().max(f[1].abs()))
        .fold(0.0, f32::max);
    20.0 * p.log10()
}

fn settings() -> CompressorSettings {
    CompressorSettings {
        threshold_db: -20.0,
        ratio: 4.0,
        knee_db: 0.0,
        attack_ms: 1.0,
        release_ms: 100.0,
        makeup_db: 0.0,
        link: true,
        mix: 1.0,
    }
}

#[test]
fn a_steady_tone_settles_to_the_ratio() {
    // 0 dBFS peak, 20 dB over a -20 dB threshold at 4:1: 15 dB of reduction.
    let mut c = Compressor::new(settings(), RATE);
    let mut io = sine(1.0, RATE as usize);
    c.process(&mut io);
    let tail = io.get(RATE as usize / 2..).unwrap();
    let out = peak_db(tail);
    assert!(
        (out - -15.0).abs() < 0.5,
        "settled at {out} dB, expected -15"
    );
}

#[test]
fn attack_follows_its_time_constant() {
    let mut s = settings();
    s.attack_ms = 10.0;
    let mut c = Compressor::new(s, RATE);
    // A step 20 dB over: after one attack time, 1 - 1/e of the 15 dB.
    let frames = (s.attack_ms * 0.001 * RATE as f32) as usize;
    c.process(&mut vec![[1.0, 1.0]; frames]);
    let got = c.reduction()[0];
    let want = 15.0 * (1.0 - (-1.0f32).exp());
    assert!(
        (got - want).abs() < 0.2 * want,
        "reduction {got} dB after one attack, want {want}"
    );
}

#[test]
fn below_the_threshold_and_dry_mix_pass_unchanged() {
    let mut c = Compressor::new(settings(), RATE);
    let quiet = sine(0.05, 4_800);
    let mut io = quiet.clone();
    c.process(&mut io);
    assert_eq!(io, quiet);
    let mut s = settings();
    s.mix = 0.0;
    let mut dry = Compressor::new(s, RATE);
    let loud = sine(1.0, 4_800);
    let mut io = loud.clone();
    dry.process(&mut io);
    assert_eq!(io, loud);
}

#[test]
fn a_soft_knee_starts_below_the_threshold() {
    let mut s = settings();
    s.knee_db = 12.0;
    s.attack_ms = 0.0;
    let mut c = Compressor::new(s, RATE);
    // 3 dB under the threshold, inside the 12 dB knee.
    let level = 10f32.powf(-23.0 / 20.0);
    c.process(&mut vec![[level, level]; 64]);
    assert!(c.reduction()[0] > 0.0);
}

#[test]
fn the_limiter_never_passes_its_ceiling_and_releases() {
    let mut l = Limiter::new(
        LimiterSettings {
            input_db: 12.0,
            ceiling_db: -1.0,
            release_ms: 50.0,
        },
        RATE,
    );
    let mut io = sine(1.0, RATE as usize / 4);
    l.process(&mut io);
    assert!(peak_db(&io) <= -1.0 + 1e-4, "peak {} dB", peak_db(&io));
    // After the loud part, a quiet tone comes back to its own level.
    let mut quiet = sine(0.01, RATE as usize);
    l.process(&mut quiet);
    let tail = quiet.get(RATE as usize / 2..).unwrap();
    let out = peak_db(tail);
    assert!((out - (-40.0 + 12.0)).abs() < 0.5, "quiet tail at {out} dB");
}
