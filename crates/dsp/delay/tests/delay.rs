use kontra_dsp_delay::{ModDelay, Phaser, PhaserSettings, Settings};

const RATE: u32 = 48_000;

fn echo() -> Settings {
    Settings {
        ms: 10.0,
        depth_ms: 0.0,
        second_head: false,
        input: [1.0, 1.0],
        feedback: 0.5,
        cross: 0.0,
        damping_hz: f32::INFINITY,
        lfo_hz: 0.0,
        phase: 0.0,
    }
}

fn impulse(frames: usize) -> Vec<[f32; 2]> {
    let mut io = vec![[0.0; 2]; frames];
    io[0] = [1.0, 1.0];
    io
}

#[test]
fn an_echo_repeats_at_its_time_and_falls_by_the_feedback() {
    let mut d = ModDelay::new(echo(), RATE, 10.0);
    let mut io = impulse(1500);
    d.process(&mut io);
    let peaks: Vec<_> = io
        .iter()
        .enumerate()
        .filter(|(_, f)| f[0].abs() > 1e-4)
        .collect();
    assert_eq!(peaks.len(), 3, "{peaks:?}");
    for (k, (at, f)) in peaks.iter().enumerate() {
        assert_eq!(*at, 480 * (k + 1));
        assert!((f[0] - 0.5f32.powi(k as i32)).abs() < 1e-6 && f[0] == f[1]);
    }
}

#[test]
fn cross_feedback_ping_pongs_and_damping_rounds_the_repeats() {
    let mut s = echo();
    (s.input, s.feedback, s.cross) = ([1.0, 0.0], 0.0, 0.5);
    let mut io = impulse(1000);
    ModDelay::new(s, RATE, 10.0).process(&mut io);
    assert!((io[480][0] - 1.0).abs() < 1e-6 && io[480][1] == 0.0);
    assert!((io[960][1] - 0.5).abs() < 1e-6 && io[960][0] == 0.0);

    s.damping_hz = 2000.0;
    let mut io = impulse(600);
    ModDelay::new(s, RATE, 10.0).process(&mut io);
    let tail: f32 = io[480..].iter().map(|f| f[0]).sum();
    assert!(
        io[480][0] < 0.5 && (tail - 1.0).abs() < 0.01,
        "a low-pass keeps DC"
    );
}

#[test]
fn a_new_time_glides_without_a_jump() {
    let mut d = ModDelay::new(echo(), RATE, 100.0);
    let mut tone: Vec<[f32; 2]> = (0..4800).map(|i| [(i as f32 * 0.01).sin(); 2]).collect();
    d.process(&mut tone[..2400]);
    d.set(Settings { ms: 50.0, ..echo() });
    d.process(&mut tone[2400..]);
    let jump = tone
        .windows(2)
        .skip(2400)
        .map(|w| (w[1][0] - w[0][0]).abs())
        .fold(0.0, f32::max);

    assert!(jump < 0.05, "largest step {jump}");
}

/// The kemu stimulus: `0.5 sin(0.05 n) + impulse` / `0.5 sin(0.031 n)`.
fn stimulus(n: usize) -> Vec<[f32; 2]> {
    (0..n)
        .map(|i| {
            let t = i as f64;
            [
                ((t * 0.05).sin() * 0.5 + if i == 0 { 1. } else { 0. }) as f32,
                ((t * 0.031).sin() * 0.5) as f32,
            ]
        })
        .collect()
}

/// Kontakt 8.13.1's Legacy Chorus and Flanger audio (0x140aa0510,
/// 0x140aa2350) under tools/re/kemu.py at 48 kHz, wet output (v2
/// sampler-kontakt effects/timemod.rs). Chorus depth .6, speed 1.5 Hz, phase
/// .3: a zero base delay, so two heads a frame or two back, each smoothed by
/// 0.95. Flanger depth .7, feedback .6, 0.8 Hz, phase .4, colour 2.5 ms.
#[test]
fn legacy_chorus_and_flanger_land_near_the_native_vectors() {
    let near = |y: &[[f32; 2]], want: &[(usize, u32, u32)], tolerance: f32| {
        for &(i, l, r) in want {
            let w = [f32::from_bits(l), f32::from_bits(r)];
            let e = (y[i][0] - w[0]).abs().max((y[i][1] - w[1]).abs());
            assert!(e < tolerance, "frame {i}: {:?} vs {w:?}", y[i]);
        }
    };
    let chorus = Settings {
        ms: 0.0,
        depth_ms: 0.0,
        second_head: true,
        input: [1.0, 1.0],
        feedback: 0.0,
        cross: 0.0,
        damping_hz: 22_885.0,
        lfo_hz: 1.5,
        phase: 0.3 * 0.25,
    };
    let mut y = stimulus(2000);
    ModDelay::new(chorus, RATE, 0.0).process(&mut y);
    near(
        &y,
        &[
            (150, 0x3f6e9348, 0xbf87a26f),
            (777, 0x3f6c99aa, 0xbf6cf386),
            (1500, 0xbedffe9a, 0x3f1fce4e),
            (1999, 0xbf1aa44c, 0xbf4d2a8e),
        ],
        0.08,
    );
    let flanger = Settings {
        ms: 2.5,
        depth_ms: 0.7 * 50.0 / 48.0,
        second_head: false,
        input: [1.0, 1.0],
        feedback: -0.6 * 0.95,
        cross: 0.0,
        damping_hz: f32::INFINITY,
        lfo_hz: 0.8,
        phase: 0.4 * 0.25,
    };
    let mut y = stimulus(2000);
    ModDelay::new(flanger, RATE, 2.5).process(&mut y);
    near(
        &y,
        &[
            (150, 0x3efd2f1d, 0x3dfad0db),
            (777, 0x3e93b433, 0x3dc530ac),
            (1500, 0xbe081447, 0xbe84e6f9),
            (1999, 0xbe4acd84, 0x3e0fa710),
        ],
        0.02,
    );
}

#[test]
fn a_phaser_with_feedback_notches_and_stays_bounded() {
    let s = PhaserSettings {
        hz: 292.8,
        spread_hz: 260.7,
        exponent: 1.5,
        feedback: 0.5 * 0.85,
        lfo_hz: 0.5,
        phase: 0.25,
    };
    let mut p = Phaser::new(s, RATE);
    let mut y = stimulus(RATE as usize);
    p.process(&mut y);
    let peak = y.iter().flatten().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak.is_finite() && peak < 4.0 && peak > 0.1, "peak {peak}");
}
