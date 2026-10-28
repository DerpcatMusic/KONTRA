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
