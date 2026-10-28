use kontra_dsp_reverb::shape::{Cuts, cut, envelope};

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
