use kontra_dsp_drive::{Crush, Curve, Reduction, Shaper, Tone};

/// Steady-state peak of a sine at `hz` through `s`, in dB.
fn gain_db(mut s: Shaper, hz: f64, rate: u32) -> f64 {
    let n = rate as usize;
    let mut io: Vec<[f32; 2]> = (0..n)
        .map(|i| [(std::f64::consts::TAU * hz * i as f64 / f64::from(rate)).sin() as f32 * 0.01; 2])
        .collect();
    s.process(&mut io);
    let peak = io
        .iter()
        .skip(n / 2)
        .map(|x| x[0].abs())
        .fold(0.0, f32::max);
    20.0 * f64::from(peak / 0.01).log10()
}

#[test]
fn the_curves_match_v1s_measured_transfer_vectors() {
    // Kontakt Saturation (Classic): shape, x, y.
    for (shape, x, y) in [
        (-1.0, 0.5, 0.159_090_909_090_909_1),
        (-0.5, 0.25, 0.173_155_737_704_918_04),
        (0.0, -4.0, -4.0),
        (0.0001, 0.01, 0.01),
        (0.125, 0.25, 0.34375),
        (0.25, 0.5, 0.75),
        (0.5, -0.25, -0.75),
        (1.0, 0.01, 0.0784),
        (1.0, 4.0, 1.0),
    ] {
        let got = f64::from(Curve::Soft(shape).transfer(x as f32));
        assert!((got - y).abs() < 2e-7, "soft {shape} at {x}: {got}");
    }
    // Kontakt Distortion at drive 1: Tube, then Transistor.
    let xs = [-0.25, -0.01, 0.0, 0.01, 0.25];
    let tube = [
        -0.109_394_263_170_87,
        -1.511_801_183_946e-5,
        0.0,
        0.072_360_783_382_485,
        0.857_976_493_794_69,
    ];
    let power = [-0.25, -0.000_039_062_5, 0.0, 0.131_326_390_220_19, 0.25];
    for ((x, t), p) in xs.into_iter().zip(tube).zip(power) {
        assert!(
            (f64::from(Curve::Tube(1.0).transfer(x)) - t).abs() < 2e-7,
            "tube {x}"
        );
        assert!(
            (f64::from(Curve::Power(1.0).transfer(x)) - p).abs() < 2e-7,
            "power {x}"
        );
    }
}

#[test]
fn a_flat_shaper_passes_the_band_and_its_filters_sit_3_db_down() {
    let flat = || Shaper::new(Curve::Soft(0.0), Tone::NONE, 44_100);
    assert!(gain_db(flat(), 1000.0, 44_100).abs() < 0.01);
    assert!(gain_db(flat(), 18_000.0, 44_100).abs() < 0.1);
    for (tone, hz) in [
        (
            Tone {
                lowpass_hz: Some(2200.0),
                highpass_hz: None,
            },
            2200.0,
        ),
        (
            Tone {
                lowpass_hz: None,
                highpass_hz: Some(15.0),
            },
            15.0,
        ),
    ] {
        let s = Shaper::new(Curve::Soft(0.0), tone, 48_000);
        assert!((gain_db(s, hz, 48_000) + 3.01).abs() < 0.05, "{tone:?}");
    }
}

#[test]
fn twice_the_rate_keeps_aliases_40_db_under_the_harmonics() {
    // A full-scale 1.76 kHz tone through Tube(1): −30 dB of aliases at 1x,
    // −42 dB at 2x.
    const RATE: f64 = 48_000.0;
    const N: usize = 1 << 13;
    let hz = RATE * 300.0 / N as f64; // on a bin
    let mut io: Vec<[f32; 2]> = (0..2 * N)
        .map(|i| [(std::f64::consts::TAU * hz * i as f64 / RATE).sin() as f32; 2])
        .collect();
    Shaper::new(Curve::Tube(1.0), Tone::NONE, 48_000).process(&mut io);
    let tail = &io[N..];
    // Energy on the harmonic bins (DC included) against everything else.
    let (mut on, mut off) = (0.0f64, 0.0f64);
    for bin in 0..N / 2 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (n, x) in tail.iter().enumerate() {
            let w = std::f64::consts::TAU * (bin * n % N) as f64 / N as f64;
            re += f64::from(x[0]) * w.cos();
            im -= f64::from(x[0]) * w.sin();
        }
        let p = re * re + im * im;
        if bin % 300 == 0 { on += p } else { off += p }
    }
    let db = 10.0 * (off / on).log10();
    assert!(db < -40.0, "aliases at {db:.1} dB");
}

#[test]
fn the_shaper_is_block_size_free() {
    let input: Vec<[f32; 2]> = (0..600)
        .map(|n| [((n as f32) * 0.37).sin(), ((n as f32) * 0.11).cos()])
        .collect();
    let tone = Tone {
        lowpass_hz: Some(3000.0),
        highpass_hz: Some(15.0),
    };
    let mut whole = input.clone();
    Shaper::new(Curve::Tube(0.7), tone, 48_000).process(&mut whole);
    let mut s = Shaper::new(Curve::Tube(0.7), tone, 48_000);
    let mut parts = input;
    for chunk in parts.chunks_mut(37) {
        s.process(chunk);
    }
    assert_eq!(parts, whole);
}

fn ramp() -> Vec<[f32; 2]> {
    (0..12).map(|i| [(i + 1) as f32; 2]).collect()
}

#[test]
fn the_hold_captures_every_hold_frames() {
    // Kontakt Lo-Fi at frequency 0.5: period 2, a capture every third frame.
    let off = Reduction {
        hold: 3.0,
        bits: None,
        noise: 0.0,
        noise_tone: 1.0,
    };
    let mut io = ramp();
    Crush::new(off, 1).process(&mut io);
    let left: Vec<f32> = io.iter().map(|x| x[0]).collect();
    assert_eq!(&left[..9], &[0., 0., 3., 3., 3., 6., 6., 6., 9.]);
    let mut io = ramp();
    Crush::new(Reduction { hold: 1.0, ..off }, 1).process(&mut io);
    assert_eq!(io, ramp());
}

#[test]
fn the_quantizer_truncates_toward_zero() {
    // 1 bit: 0.9 × 2 = 1.8 truncates to 1 (rounding would give 2).
    let r = Reduction {
        hold: 1.0,
        bits: Some(1.0),
        noise: 0.0,
        noise_tone: 1.0,
    };
    let mut io = vec![[0.9, -0.9]];
    Crush::new(r, 1).process(&mut io);
    assert_eq!(io, vec![[0.5, -0.5]]);
}

#[test]
fn the_noise_is_the_lcg_and_its_seed_decides_it() {
    // Seed 1 → 0x41cce7a0 → u = 0.20163727, white, at Kontakt's level 1 (0.075).
    let r = Reduction {
        hold: 1.0,
        bits: None,
        noise: 0.075,
        noise_tone: 1.0,
    };
    let mut a = vec![[0.0; 2]; 64];
    Crush::new(r, 1).process(&mut a);
    assert!((a[0][0] - 0.015_122_795).abs() < 1e-8, "{}", a[0][0]);
    let mut b = vec![[0.0; 2]; 64];
    Crush::new(r, 2).process(&mut b);
    assert_ne!(a, b);
}
