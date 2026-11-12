#![cfg(test)]

use kontra_dsp_reverb::{Network, Settings};
use kontra_law::ReverbNetwork;

const RATE: u32 = 48_000;

/// A small network: 31 lines on an m-sequence (x⁵ + x² + 1), two units a side.
fn law() -> &'static ReverbNetwork {
    let mut sequence = [0u8; 31];
    let mut s = 1u8;
    for bit in &mut sequence {
        *bit = s & 1;
        s = (s >> 1) | ((s ^ (s >> 2)) & 1) << 4;
    }
    let pad =
        |pairs: &[(u8, f32)]| std::array::from_fn(|i| pairs.get(i).copied().unwrap_or((0, 0.0)));
    let tap = |a: u8| {
        std::array::from_fn(|i| (a + i as u8 * 3, if i % 2 == 0 { 0.4 } else { -0.4 }, 0.3))
    };
    Box::leak(Box::new(ReverbNetwork {
        growth: std::array::from_fn(|i| 20.0 + i as f32 * 3.0),
        offset: std::array::from_fn(|i| 40.0 + i as f32 * 17.0),
        sequence,
        inputs: [
            pad(&[(0, 1.0), (2, -1.0), (4, 1.0)]),
            pad(&[(1, 1.0), (3, -1.0), (5, 1.0)]),
        ],
        inject_lead: 0,
        taps: [tap(6), tap(7)],
        units: [[29, 37, 41], [43, 47, 53], [59, 61, 67], [71, 73, 79]],
        unit_count: 2,
        unit_feedback: 0.9,
        hf: [0.5, 0.95, 0.27],
        level: [4.0, 0.5],
        shelf_hz: 200.0,
        rate: 44_100.0,
        max_rate: 48_000.0,
        glide_s: [0.05, 0.2],
    }))
}

fn settings(size: f32) -> Settings {
    Settings {
        seconds: 4.0,
        size,
        damping: 0.3,
        diffusion: 0.5,
        predelay_ms: 0.0,
        high_cut_hz: 20_000.0,
        low_shelf_db: 0.0,
        stereo: 1.0,
        modulation_ms: 0.0,
        modulation_hz: 0.0,
    }
}

fn run(n: &mut Network, frames: usize, impulse: bool) -> Vec<[f32; 2]> {
    let mut io = vec![[0.0f32; 2]; frames];
    if impulse {
        io[0] = [1.0, 1.0];
    }
    for block in io.chunks_mut(64) {
        n.process(block);
    }
    io
}

/// RMS of each millisecond, dB.
fn ms_levels(x: &[[f32; 2]]) -> Vec<f32> {
    x.chunks(RATE as usize / 1000)
        .map(|c| {
            let e = c.iter().map(|f| f[0] * f[0] + f[1] * f[1]).sum::<f32>() / c.len() as f32;
            10.0 * (e + 1e-30).log10()
        })
        .collect()
}

/// A size change rides the tail: the lines glide to their new lengths, the
/// wet output never fades out around it (Kontakt's Reverb, P5.33 oracle).
#[test]
fn a_live_size_change_glides_through_the_tail() {
    let mut n = Network::new(law(), settings(1.0), RATE);
    let before = run(&mut n, RATE as usize / 2, true);
    let level = ms_levels(&before[before.len() - 4800..])
        .iter()
        .sum::<f32>()
        / 100.0;
    n.set(settings(2.0));
    let after = run(&mut n, RATE as usize / 4, false);
    assert!(after.iter().flatten().all(|v| v.is_finite()));
    let low = ms_levels(&after[..2400])
        .into_iter()
        .fold(f32::MAX, f32::min);
    assert!(low > level - 12.0, "dips to {low} dB from {level} dB");
    // Mid-glide a change back starts from where the lines are, still no dip.
    n.set(settings(1.0));
    let back = run(&mut n, RATE as usize / 20, false);
    let low = ms_levels(&back).into_iter().fold(f32::MAX, f32::min);
    assert!(low > level - 24.0, "dips to {low} dB from {level} dB");
}

/// What a skipped stretch of silence leaves: idling lands the modulation,
/// the glides and (above the law's rate) the resampler's phase where
/// processing silence would, so the next note's tail matches an unskipped run.
#[test]
fn an_idled_network_plays_on_in_step_with_one_that_ran() {
    for rate in [48_000, 96_000] {
        let s = Settings {
            seconds: 0.3,
            modulation_ms: 3.0,
            modulation_hz: 0.7,
            ..settings(1.0)
        };
        let (mut ran, mut idled) = (Network::new(law(), s, rate), Network::new(law(), s, rate));
        let block =
            |n: &mut Network, io: &mut [[f32; 2]]| io.chunks_mut(64).for_each(|b| n.process(b));
        for n in [&mut ran, &mut idled] {
            let mut io = vec![[0.0; 2]; 3 * rate as usize];
            io[0] = [1.0, 1.0];
            block(n, &mut io);
            // A new size while silent: the lines glide there unheard.
            n.set(settings(1.5));
        }
        let quiet = 20_011;
        block(&mut ran, &mut vec![[0.0; 2]; quiet]);
        let mut scratch = vec![[0.0; 2]; quiet];
        scratch.chunks_mut(64).for_each(|b| idled.idle(b));
        let mut a = vec![[0.0; 2]; rate as usize / 2];
        a[0] = [1.0, 1.0];
        let mut b = a.clone();
        block(&mut ran, &mut a);
        block(&mut idled, &mut b);
        let worst = (a.iter().flatten().zip(b.iter().flatten()))
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-6, "{rate}: {worst}");
    }
}

/// A modulated four-unit network on 2 s of noise, its settings moved half
/// way: the reference render that every speed-up must reproduce.
fn reference_render() -> Vec<[f32; 2]> {
    render(Box::leak(Box::new(ReverbNetwork {
        unit_count: 4,
        ..*law()
    })))
}

/// The reference with the input landing short of every line's reads and
/// units longer than a span.
fn far_input_render() -> Vec<[f32; 2]> {
    render(Box::leak(Box::new(ReverbNetwork {
        inject_lead: 150,
        units: [
            [89, 97, 101],
            [103, 107, 109],
            [113, 127, 131],
            [137, 139, 149],
        ],
        unit_count: 3,
        ..*law()
    })))
}

/// 2 s of noise through `law`, its size and damping changed at 0.96 s.
fn render(law: &'static ReverbNetwork) -> Vec<[f32; 2]> {
    let s = Settings {
        seconds: 2.5,
        damping: 0.6,
        predelay_ms: 12.0,
        high_cut_hz: 9_000.0,
        low_shelf_db: -4.0,
        stereo: 0.7,
        modulation_ms: 2.0,
        modulation_hz: 0.6,
        ..settings(1.0)
    };
    let mut n = Network::new(law, s, RATE);
    let mut seed = 0x2545_f491_u32;
    let mut noise = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5
    };
    let mut io: Vec<[f32; 2]> = (0..2 * RATE as usize).map(|_| [noise(), noise()]).collect();
    for (k, block) in io.chunks_mut(512).enumerate() {
        if k == 90 {
            n.set(Settings {
                size: 1.4,
                damping: 0.2,
                ..s
            });
        }
        n.process(block);
    }
    io
}

/// FNV-1a over the output's bits.
fn bits_hash(x: &[[f32; 2]]) -> u64 {
    x.iter().flatten().fold(0xcbf2_9ce4_8422_2325, |h, v| {
        (h ^ u64::from(v.to_bits())).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Speed-ups keep the sound bit for bit.
#[test]
fn the_reference_render_is_unchanged() {
    let out = reference_render();
    if let Some(path) = std::env::var_os("KONTRA_NETWORK_REFERENCE") {
        let bytes: Vec<u8> = out.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
        std::fs::write(path, bytes).expect("reference written");
    }
    assert_eq!(bits_hash(&out), 8_123_125_815_312_325_928, "render changed");
    assert_eq!(
        bits_hash(&far_input_render()),
        17_184_959_724_877_187_036,
        "far-input render changed"
    );
}

/// Nanoseconds per frame of the reference render (release, `--ignored`).
#[test]
#[ignore = "timing"]
fn network_speed() {
    let started = std::time::Instant::now();
    let runs = 5;
    for _ in 0..runs {
        std::hint::black_box(reference_render());
    }
    let ns = started.elapsed().as_nanos() as f64 / f64::from(runs) / f64::from(2 * RATE);
    eprintln!(
        "network: {ns:.1} ns per frame, {:.1} us per 512-frame block",
        ns * 0.512
    );
}
