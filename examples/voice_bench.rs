//! Voice render throughput at high polyphony: pitched (resampled) voices
//! with and without per-voice group filters. Setup is untimed.
//! `cargo run --profile ci --no-default-features --example voice_bench [-- voices]`
//! prints median/p99 microseconds per block and the median's share of the
//! 48 kHz deadline.

use kontakto::{
    audio::Sample,
    engine::{Bank, Engine},
    fx::{Chain, Effect, Kind, Params, params},
    import::{Group, Loop, ModAssignment, ModSource, ModTarget, Zone},
    modulation::{Ahdsr, ModEnvelope},
};
use std::path::PathBuf;

const RATE: f64 = 48_000.0;
const BLOCKS: usize = 400;

fn effect(slot: usize, params: Params) -> Effect {
    Effect { slot, kind: Kind::Filter, version: 0, bypass: false, output_gain: 1.0, dry_level: 0.0, params }
}

fn filter(filter_type: i32, cutoff: f32) -> Params {
    Params::Filter(params::Filter { filter_type, cutoff, resonance: 0.4, extra: [0.0; 3], native_flag: None })
}

#[derive(Clone, Copy, PartialEq)]
enum Fx {
    None,
    /// LP 4-pole SVF, HP 2-pole SVF, a two-band EQ: five sections, held.
    Svf,
    /// The same chain with its cutoff swept by a module envelope.
    SvfSwept,
    /// Nonlinear 4-pole ladder.
    Ladder,
}

fn group(fx: Fx) -> Group {
    let band = |freq_hz, gain_db| params::EqBand { freq_hz, bandwidth_oct: 1.0, gain_db };
    let slots = match fx {
        Fx::None => vec![],
        Fx::Svf | Fx::SvfSwept => vec![
            effect(0, filter(5, 0.6)),
            effect(1, filter(3, 0.2)),
            effect(2, Params::Eq(params::Eq { bands: vec![band(400.0, -6.0), band(3000.0, 4.0)] })),
        ],
        Fx::Ladder => vec![effect(0, filter(103, 0.6))],
    };
    let (mods, envelopes) = if fx == Fx::SvfSwept {
        let target = ModAssignment {
            name: "Envelope".into(),
            source: ModSource::Unassigned,
            target: ModTarget::Module { param: "filterCutoff".into(), slot: 0 },
            intensity: 0.5,
            invert: false,
            lag_ms: 0,
            shaper: None,
        };
        // Attack longer than the run: the cutoff moves every control tick.
        let env = Ahdsr { attack_curve: 0., attack_ms: 60_000., hold_ms: 0., decay_ms: 0.,
            sustain: 1., release_ms: 100., unknown_flag: 0, unknown_tail: Vec::new() };
        (vec![target.clone()], vec![ModEnvelope { env, targets: vec![target] }])
    } else {
        (vec![], vec![])
    };
    Group { gain: 0.01, fx: Chain { slots }, mods, envelopes, ..Group::default() }
}

fn engine(fx: Fx, voices: usize, lanes: bool) -> Engine {
    // 44.1 kHz source in a 48 kHz engine: every voice resamples.
    let mut x = 1u32;
    let frames = (0..96_000)
        .map(|_| {
            let mut next = || {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (x >> 8) as f32 / (1 << 23) as f32 - 1.0
            };
            [next(), next()]
        })
        .collect();
    let zone = Zone {
        low_key: 0,
        high_key: 127,
        sample: PathBuf::from("noise"),
        loop_range: Some(Loop { start: 1000, end: 90_000, alternating: false, until_release: false, crossfade: 0 }),
        ..Zone::default()
    };
    let mut bank = Bank::from_samples(vec![group(fx)], vec![zone], vec![(PathBuf::from("noise"), Sample { rate: 44_100, frames })]).unwrap();
    bank.set_polyphony(voices);
    let mut e = Engine::default();
    e.reset(RATE);
    (e.attack, e.release) = (0.001, 0.5);
    e.set_bank(Some(Box::new(bank)));
    e.set_lanes(lanes);
    // Staggered starts over 16 channels and four octaves: no two voices
    // share a step and phase unless the instrument allows it.
    let (mut l, mut r) = (vec![0.0; 7], vec![0.0; 7]);
    for v in 0..voices {
        e.note_on((v % 16) as u8, 36 + (v * 7 % 48) as u8, 100);
        e.render(&mut l, &mut r);
    }
    e
}

fn measure(name: &str, fx: Fx, voices: usize, lanes: bool, block: usize) {
    let mut e = engine(fx, voices, lanes);
    let (mut l, mut r) = (vec![0.0f32; block], vec![0.0f32; block]);
    for _ in 0..20 {
        e.render(&mut l, &mut r);
    }
    let active = e.active_voices();
    let mut times: Vec<f64> = (0..BLOCKS)
        .map(|_| {
            let t = std::time::Instant::now();
            e.render(&mut l, &mut r);
            t.elapsed().as_secs_f64() * 1e6
        })
        .collect();
    assert!(l.iter().chain(&r).all(|x| x.is_finite()));
    times.sort_by(f64::total_cmp);
    let (median, p99) = (times[BLOCKS / 2], times[BLOCKS * 99 / 100]);
    let deadline = block as f64 / RATE * 1e6;
    println!(
        "{name:<14} lanes={:<5} block={block:<3} voices={active:<4} median {median:>8.1} us  p99 {p99:>8.1} us  {:>5.1}% of deadline",
        lanes,
        median / deadline * 100.0
    );
}

fn main() {
    let voices: usize = std::env::args().nth(1).map_or(1024, |s| s.parse().expect("voice count"));
    for (name, fx) in [("pitched", Fx::None), ("svf x5", Fx::Svf), ("svf x5 swept", Fx::SvfSwept), ("ladder", Fx::Ladder)] {
        for lanes in [true, false] {
            for block in [64, 128] {
                measure(name, fx, voices, lanes, block);
            }
        }
    }
}
