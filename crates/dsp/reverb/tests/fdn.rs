use kontra_dsp_reverb::{Fdn, Settings};

const RATE: u32 = 48_000;

fn settings(seconds: f32) -> Settings {
    Settings {
        seconds,
        size: 1.0,
        damping: 0.0,
        diffusion: 1.0,
        predelay_ms: 10.0,
        high_cut_hz: 20_000.0,
        low_shelf_db: 0.0,
        stereo: 1.0,
    }
}

/// Energy of `frames` in dB.
fn db(frames: &[[f32; 2]]) -> f32 {
    let e: f32 = frames.iter().map(|[l, r]| l * l + r * r).sum();
    10.0 * e.max(1e-30).log10()
}

#[test]
fn the_tail_falls_60_db_in_its_time_and_is_stereo_and_finite() {
    let mut fdn = Fdn::new(settings(1.0), RATE);
    let mut io = vec![[0.0f32; 2]; 3 * RATE as usize];
    io[0] = [1.0, 1.0];
    for block in io.chunks_mut(97) {
        fdn.process(block);
    }
    assert!(io.iter().flatten().all(|v| v.is_finite()));
    // Predelay: nothing before 10 ms.
    assert!(io[..479].iter().flatten().all(|&v| v == 0.0));
    let window = |s: f32| {
        let at = (s * RATE as f32) as usize;
        db(&io[at..at + 4800])
    };
    let fall = window(0.2) - window(1.2);
    assert!((50.0..70.0).contains(&fall), "fell {fall} dB in 1 s");
    let side: f32 = io[..RATE as usize].iter().map(|[l, r]| (l - r).abs()).sum();
    assert!(side > 1.0, "the tail is decorrelated");
}

fn run(fdn: &mut Fdn, frames: usize, impulse_at: Option<usize>) -> Vec<[f32; 2]> {
    let mut io = vec![[0.0f32; 2]; frames];
    if let Some(frame) = impulse_at.and_then(|at| io.get_mut(at)) {
        *frame = [1.0, 1.0];
    }
    for block in io.chunks_mut(97) {
        fdn.process(block);
    }
    io
}

#[test]
fn set_matches_new_once_the_tail_has_settled() {
    const RATE: u32 = 8_000;
    let mut before = settings(0.2);
    before.size = 1.5;
    let after = Settings {
        seconds: 0.8,
        size: 2.5,
        damping: 0.4,
        diffusion: 0.3,
        predelay_ms: 120.0,
        high_cut_hz: 6_000.0,
        low_shelf_db: -6.0,
        stereo: 0.5,
    };
    let mut live = Fdn::new(before, RATE);
    run(&mut live, RATE as usize, Some(0));
    // The old tail falls to silence (flushed to zero) well before this.
    run(&mut live, 12 * RATE as usize, None);
    live.set(after);
    let mut fresh = Fdn::new(after, RATE);
    // Past the 10 ms fade in and out, with the same input from here on.
    let (a, b) = (
        run(&mut live, RATE as usize, Some(RATE as usize / 4)),
        run(&mut fresh, RATE as usize, Some(RATE as usize / 4)),
    );
    let worst = a
        .iter()
        .zip(&b)
        .flat_map(|(x, y)| [x[0] - y[0], x[1] - y[1]])
        .fold(0.0f32, |m, d| m.max(d.abs()));
    assert!(worst < 1e-6, "differs by {worst}");
}

#[test]
fn a_changed_decay_time_changes_the_tail() {
    const RATE: u32 = 8_000;
    let window = |io: &[[f32; 2]]| db(&io[RATE as usize / 2..RATE as usize]);
    let mut fdn = Fdn::new(settings(0.2), RATE);
    run(&mut fdn, RATE as usize, Some(0));
    run(&mut fdn, 12 * RATE as usize, None);
    let short = window(&run(&mut fdn, 2 * RATE as usize, Some(0)));
    fdn.set(settings(2.0));
    // The window after the fade: the tail of the longer decay.
    let long = window(&run(&mut fdn, 2 * RATE as usize, Some(0)));
    assert!(long > short + 20.0, "short {short} dB, long {long} dB");
}

#[test]
fn a_sweep_of_the_decay_time_keeps_the_wet_level() {
    const RATE: u32 = 8_000;
    let block = RATE as usize / 400;
    let mut steady = Fdn::new(settings(2.0), RATE);
    let mut swept = Fdn::new(settings(2.0), RATE);
    run(&mut steady, RATE as usize / 2, Some(0));
    run(&mut swept, RATE as usize / 2, Some(0));
    // A new decay time every 2.5 ms block for a quarter second.
    let (mut a, mut b) = (Vec::new(), Vec::new());
    for i in 0..100 {
        a.extend(run(&mut steady, block, None));
        swept.set(settings(2.0 + i as f32 * 0.002));
        b.extend(run(&mut swept, block, None));
    }
    let (a, b) = (db(&a), db(&b));
    assert!((a - b).abs() < 1.0, "steady {a} dB, swept {b} dB");
}

#[test]
fn a_live_low_shelf_change_glides_without_a_step() {
    let mut same = Fdn::new(settings(2.0), RATE);
    let mut moved = Fdn::new(settings(2.0), RATE);
    run(&mut same, RATE as usize / 2, Some(0));
    run(&mut moved, RATE as usize / 2, Some(0));
    let mut louder = settings(2.0);
    louder.low_shelf_db = 12.0;
    moved.set(louder);
    let (a, b) = (run(&mut same, 4800, None), run(&mut moved, 4800, None));
    let diff: Vec<f32> = a.iter().zip(&b).map(|(x, y)| (x[0] - y[0]).abs()).collect();
    let settled = diff[2400..].iter().fold(0.0f32, |m, &d| m.max(d));
    // The first sample after the change moves by a sliver of the full step.
    assert!(
        diff[0] < 0.05 * settled,
        "{} at once, {settled} settled",
        diff[0]
    );
}
