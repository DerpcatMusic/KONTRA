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
