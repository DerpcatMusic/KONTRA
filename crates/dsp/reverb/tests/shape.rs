use kontra_dsp_reverb::shape::{self, Cuts, cut, envelope};

const RATE: u32 = 48_000;

fn tone(hz: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|n| (std::f32::consts::TAU * hz * n as f32 / RATE as f32).sin())
        .collect()
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}

#[test]
fn the_late_part_takes_its_own_cuts_after_the_crossover() {
    let x = tone(8_000.0, RATE as usize);
    let c = Cuts {
        early: [20.0, 20_000.0],
        late: [20.0, 1_000.0],
        crossover: Some(0.5),
    };
    let y = cut(&x, RATE, &c);
    let half = RATE as usize / 2;
    // Early: open; late (past the 50 ms fade): an 8 kHz tone three octaves
    // over a 12 dB/octave cut.
    assert!(peak(&y[1_000..half - 2_000]) > 0.95);
    assert!(
        peak(&y[half + 2_000..]) < 0.03,
        "{}",
        peak(&y[half + 2_000..])
    );
    // No crossover: late cuts throughout.
    let all = cut(
        &x,
        RATE,
        &Cuts {
            crossover: None,
            ..c
        },
    );
    assert!(peak(&all[1_000..]) < 0.03);
    // A 200 Hz low cut, two octaves over a 50 Hz tone.
    let low = Cuts {
        late: [200.0, 20_000.0],
        crossover: None,
        ..c
    };
    assert!(peak(&cut(&tone(50.0, RATE as usize), RATE, &low)[4_800..]) < 0.1);
}

#[test]
fn an_envelope_ramps_in_amplitude_between_its_knots() {
    let (mut l, mut r) = (vec![1.0f32; 1000], vec![1.0f32; 1000]);
    let mut knots = [[1.0, 0.0]; 8];
    knots[0] = [0.0, 0.0];
    knots[1] = [0.5, -6.020_6];
    envelope([&mut l, &mut r], knots);
    assert!(
        (l[0] - 1.0).abs() < 1e-6 && (l[250] - 0.75).abs() < 1e-3 && (r[499] - 0.501).abs() < 2e-3
    );
    // From the half to the end it climbs back to 1.
    assert!((l[750] - 0.75).abs() < 1e-3);
}

/// Peak of `x` past its first and last 2000 frames (the kernel's edges).
fn level(x: &[f32]) -> f32 {
    x[2000..x.len() - 2000]
        .iter()
        .fold(0.0f32, |m, v| m.max(v.abs()))
}

fn sine(hz: f32, rate: u32, frames: usize) -> Vec<f32> {
    (0..frames)
        // f64 phase: an f32 one is 0.016 rad coarse by 96k frames.
        .map(|n| (std::f64::consts::TAU * f64::from(hz) * n as f64 / f64::from(rate)).sin() as f32)
        .collect()
}

/// A 44.1 kHz response's top octave keeps its level at 48 kHz (linear
/// interpolation lost 1.6 dB at 12–16 kHz against Kontakt's own render),
/// and a 96 kHz one loses what 48 kHz cannot hold instead of folding it.
#[test]
fn a_response_changes_rate_band_limited() {
    for hz in [1000.0, 15_000.0, 20_000.0] {
        let y = shape::resample(&sine(hz, 44_100, 44_100), 44_100, 48_000, usize::MAX);
        assert_eq!(y.len(), 48_000);
        let db = 20.0 * level(&y).log10();
        assert!(db.abs() < 0.1, "{hz} Hz: {db} dB");
    }
    let y = shape::resample(&sine(30_000.0, 96_000, 96_000), 96_000, 48_000, usize::MAX);
    let db = 20.0 * level(&y).log10();
    assert!(db < -60.0, "30 kHz folded to {db} dB");
    let kept = shape::resample(&sine(10_000.0, 96_000, 96_000), 96_000, 48_000, usize::MAX);
    assert!((20.0 * level(&kept).log10()).abs() < 0.05);
}
