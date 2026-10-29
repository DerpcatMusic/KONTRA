use kontra_dsp_dynamics::{
    Compressor, CompressorSettings, Detector, Limiter, LimiterSettings, Transient,
    TransientSettings,
};

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
        detector: Detector::Input,
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

#[test]
fn silence_after_a_loud_tone_releases_to_exactly_zero() {
    let mut c = Compressor::new(settings(), RATE);
    c.process(&mut sine(1.0, RATE as usize / 10));
    assert!(c.reduction()[0] > 1.0);
    c.process(&mut vec![[0.0; 2]; RATE as usize * 4]);
    assert_eq!(c.reduction(), [0.0; 2]);
}

/// Kontakt 8's Transient Master core under tools/re/kemu.py at 48 kHz,
/// controls .9 .2 .6 without clip (attack 96, sustain −69, +1.2 dB), on
/// bursts of 1.2 and 0.05 every 1024 frames (v2 sampler-core dynamics tests).
#[test]
fn the_transient_shaper_matches_the_native_core() {
    let mut io: Vec<[f32; 2]> = (0..4096)
        .map(|n| {
            let env = if (n / 1024) % 2 == 0 { 1.2 } else { 0.05 };
            let n = f64::from(n);
            [
                (env * (n * 0.0576).sin()) as f32,
                (env * 0.5 * (n * 0.1305).cos()) as f32,
            ]
        })
        .collect();
    let s = TransientSettings {
        attack: 96.0,
        sustain: -69.0,
        gain_db: 1.2,
        clip: false,
    };
    Transient::new(s, RATE).process(&mut io);
    let native: [(usize, f32, f32); 9] = [
        (1, 0.369_388_13, 3.176_668_2),
        (10, 3.247_984_2, 0.756_784),
        (100, -2.667_317_2, 2.334_353_7),
        (1000, 3.002_772_6, 0.229_918_15),
        (1500, -0.117_230_52, 0.009_873_159),
        (2047, -0.071_667_75, -0.028_402_573),
        (2500, -0.991_985_9, 0.922_746),
        (3000, -0.081_362_96, -0.388_658_14),
        (4095, -0.008_966_117, 0.025_019_621),
    ];
    for (n, l, r) in native {
        for (got, want) in io[n].iter().zip([l, r]) {
            assert!(
                (got - want).abs() <= 2e-3 * want.abs().max(0.01),
                "{n}: {:?} vs {l} {r}",
                io[n]
            );
        }
    }
}

#[test]
fn a_feedback_detector_compresses_less_than_feed_forward_at_the_same_ratio() {
    let squash = |detector| {
        let mut c = Compressor::new(
            CompressorSettings {
                detector,
                ..settings()
            },
            RATE,
        );
        let mut io = sine(1.0, RATE as usize / 2);
        c.process(&mut io);
        c.reduction()[0]
    };
    let (forward, back) = (squash(Detector::Input), squash(Detector::Output));
    // 20 dB over at 4:1: 15 dB forward; the output it hears is lower, so less.
    assert!(
        (forward - 15.0).abs() < 0.5 && back > 1.0 && back < forward - 3.0,
        "{forward} {back}"
    );
}

#[test]
fn a_trigger_pulls_any_level_past_the_threshold_to_one_fixed_depth() {
    // -20 dB at 4:1: a fixed 15 dB, however far past the threshold.
    let settled = |amplitude: f32| {
        let mut c = Compressor::new(
            CompressorSettings {
                detector: Detector::Trigger,
                ..settings()
            },
            RATE,
        );
        let mut io = sine(amplitude, RATE as usize);
        c.process(&mut io);
        let tail = io.get(RATE as usize / 2..).unwrap_or_default();
        peak_db(tail) - 20.0 * amplitude.log10()
    };
    let (loud, soft) = (settled(1.0), settled(0.3));
    assert!(
        (loud - -15.0).abs() < 1.0 && (soft - -15.0).abs() < 1.0,
        "{loud} {soft}"
    );
}
