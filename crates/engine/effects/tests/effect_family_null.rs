//! Null tests between effect kinds that overlap inside one `EffectFamily`
//! (13-IR-VNEXT Phase 3): each pair renders the same impulse, sine and noise
//! through both kinds' processors and measures the residual, so folding
//! `EffectKind` into families rests on measured numbers. The table prints
//! with `--nocapture` and is recorded in
//! `docs/architecture-v3/effect-family-null-report.md`.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::print_stdout)]

use std::borrow::Cow;

use kontra_effects::processors::{Dsp, Responses, apply, processor};
use kontra_effects::{MAX_SPAN, matrix, summed};
use kontra_ir as ir;

const RATE: u32 = 48_000;
const FRAMES: usize = RATE as usize / 2;

/// Renders `signal` through the processor the rack would build for `e`,
/// span by span: the wet path only (a rack mixes `dry` and `output` after).
fn render(e: &ir::Effect, signal: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut dsp = processor(Some(e), RATE, &Responses(&[], &[]));
    let mut out = signal.to_vec();
    for span in out.chunks_mut(MAX_SPAN as usize) {
        match &mut dsp {
            Dsp::None => apply(matrix::matrix(e), span),
            Dsp::Eq(stages) => summed::run(stages, span),
            Dsp::Bells(eq) => eq.process(span),
            Dsp::Filter(svf) => svf.process(span),
            Dsp::Wet(p, _) => {
                p.set(&e.kind, 120.0);
                p.process(span);
            }
        }
    }
    assert!(
        out.iter().flatten().all(|sample| sample.is_finite()),
        "non-finite render from {:?}",
        e.kind
    );
    out
}

fn signals() -> [(&'static str, Vec<[f32; 2]>); 3] {
    let mut impulse = vec![[0.0; 2]; FRAMES];
    impulse[0] = [1.0; 2];
    let sine = (0..FRAMES)
        .map(|n| {
            let x = 0.5 * (std::f32::consts::TAU * 1_000.0 * n as f32 / RATE as f32).sin();
            [x; 2]
        })
        .collect();
    // A fixed LCG: the same noise every run, decorrelated left and right.
    let mut seed = 0x1234_5678_u32;
    let mut next = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (f64::from(seed >> 8) / f64::from(1_u32 << 24)) as f32 - 0.5
    };
    let noise = (0..FRAMES).map(|_| [next(), next()]).collect();
    [("impulse", impulse), ("sine", sine), ("noise", noise)]
}

/// Residual `b − a` relative to `a`, in dB: `(rms, peak)`. −∞ is a
/// bit-exact null. Unaligned: latency is part of what a kind does.
fn null_db(a: &[[f32; 2]], b: &[[f32; 2]]) -> (f64, f64) {
    let (mut ref_sq, mut diff_sq, mut ref_pk, mut diff_pk) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    for (x, y) in a.iter().flatten().zip(b.iter().flatten()) {
        let (x, d) = (f64::from(*x), f64::from(*y) - f64::from(*x));
        ref_sq += x * x;
        diff_sq += d * d;
        ref_pk = ref_pk.max(x.abs());
        diff_pk = diff_pk.max(d.abs());
    }
    let db = |num: f64, den: f64| 20.0 * (num / den.max(f64::MIN_POSITIVE)).log10();
    (db(diff_sq.sqrt(), ref_sq.sqrt()), db(diff_pk, ref_pk))
}

fn wet(kind: ir::EffectKind) -> ir::Effect {
    ir::Effect {
        kind,
        bypass: false,
        output: 1.0,
        dry: 0.0,
    }
}

const COMP: ir::Compressor = ir::Compressor {
    threshold_db: -18.0,
    ratio: 4.0,
    knee_db: 0.0,
    attack_ms: 5.0,
    release_ms: 80.0,
    makeup_db: 0.0,
    makeup_slope: 0.0,
    link: true,
    mix: 1.0,
    detector: ir::Detector::Input,
    sidechain_hz: None,
};

fn shaper(curve: ir::DriveCurve) -> ir::Shaper {
    ir::Shaper {
        pre_lowpass_hz: None,
        curve,
        lowpass_hz: None,
        highpass_hz: None,
        sections: ir::Sections::NONE,
    }
}

fn filter(ladder: Option<ir::Ladder>) -> ir::EffectKind {
    ir::EffectKind::Filter(ir::Filter {
        pass: ir::Pass::Low,
        hz: 1_000.0,
        stages: 2,
        ladder,
        damping: [std::f32::consts::SQRT_2; 3],
        gain: 1.0,
        octaves: 0.0,
        q_octaves: 0.0,
    })
}

fn chorus(depth_ms: f32, feedback: f32) -> ir::EffectKind {
    let diagonal = |g| [[g, 0.0], [0.0, g]];
    ir::EffectKind::Delay(ir::Delay {
        direct: 0.0,
        time: ir::DelayTime::Ms(15.0),
        depth_ms,
        second_head: false,
        input: diagonal(1.0),
        feedback: diagonal(feedback),
        output: diagonal(1.0),
        damping_hz: 20_000.0,
        low_cut_hz: 0.0,
        steep_low_cut: false,
        body: Cow::Borrowed(&[]),
        triangle_s: None,
        shape: Cow::Borrowed(&[]),
        noise: [0.0; 2],
        high_shelf: None,
        spread_ms: 0.0,
        rate: ir::Rate::Hz(0.9),
        phase: 0.0,
    })
}

const PHASER: ir::Phaser = ir::Phaser {
    stages: 6,
    triangle: false,
    hz: 400.0,
    spread_hz: 400.0,
    exponent: 1.0,
    feedback: 0.5,
    rate: ir::Rate::Hz(0.9),
    phase: 0.0,
};

const NOTCHES: ir::Notches = ir::Notches {
    stages: 6,
    hz: 622.0,
    octaves: 4.6,
    spread: 0.0,
    spread_swing: 1.0,
    spread_limit: 2.7,
    feedback: 0.5,
    loop_width: 8.0,
    loop_top: 0.1,
    rate: ir::Rate::Hz(0.9),
    rounding: 0.0,
    depth: 1.0,
    warp: 0.1,
    phase: 0.0,
    polarity: ir::Polarity::Normal,
    mix: 1.0,
    low_cut_hz: 5.0,
    internal_hz: 90_000.0,
};

/// What a pair is expected to measure: the same processing (a fold the
/// family can make without changing a sample), or different processing (a
/// distinct recurrence the family must keep).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Expect {
    /// The rms null on every signal at or below this, dB.
    Same(f64),
    /// The rms null on some signal at or above this, dB.
    Distinct(f64),
}

struct Pair {
    family: &'static str,
    name: &'static str,
    a: ir::EffectKind,
    /// Rendered in series.
    b: Vec<ir::EffectKind>,
    expect: Expect,
}

fn pairs() -> Vec<Pair> {
    use Expect::{Distinct, Same};
    use ir::EffectKind as K;
    vec![
        // Utility: the matrix kinds are one 2×2 gain.
        Pair {
            family: "Utility",
            name: "Gain(0.5) vs Matrix(0.5·I)",
            a: K::Gain(0.5),
            b: vec![K::Matrix([[0.5, 0.0], [0.0, 0.5]])],
            expect: Same(-120.0),
        },
        Pair {
            family: "Utility",
            name: "Stereo{0.5, 0} vs Matrix(I)",
            a: K::Stereo {
                width: 0.5,
                pan: 0.0,
            },
            b: vec![K::Matrix([[1.0, 0.0], [0.0, 1.0]])],
            expect: Same(-120.0),
        },
        Pair {
            family: "Utility",
            name: "Invert{swap} vs Matrix(swap)",
            a: K::Invert {
                phase: false,
                swap: true,
            },
            b: vec![K::Matrix([[0.0, 1.0], [1.0, 0.0]])],
            expect: Same(-120.0),
        },
        // FilterEq.
        Pair {
            family: "FilterEq",
            name: "Eq(flat) vs Gain(1)",
            a: K::Eq([ir::Bell::FLAT; 4]),
            b: vec![K::Gain(1.0)],
            expect: Same(-120.0),
        },
        Pair {
            family: "FilterEq",
            name: "SVF LP2 vs ZeroDelay ladder LP2",
            a: filter(None),
            b: vec![filter(Some(ir::Ladder::ZeroDelay(2)))],
            expect: Distinct(-60.0),
        },
        Pair {
            family: "FilterEq",
            name: "SVF LP2 vs OnePole",
            a: filter(None),
            b: vec![filter(Some(ir::Ladder::OnePole))],
            expect: Distinct(-20.0),
        },
        // Dynamics.
        Pair {
            family: "Dynamics",
            name: "Compressor vs Strip(Compressor, Soft(0))",
            a: K::Compressor(COMP),
            b: vec![K::Strip(ir::Strip {
                compressor: COMP,
                shaper: shaper(ir::DriveCurve::Soft(0.0)),
            })],
            // The strip's shaper is not transparent at Soft(0): its own
            // resampling path (see "Shaper Soft(0) vs Gain(1)").
            expect: Distinct(-20.0),
        },
        Pair {
            family: "Dynamics",
            name: "Strip(Compressor, Tanh(2)) vs Compressor → Shaper Tanh(2)",
            a: K::Strip(ir::Strip {
                compressor: COMP,
                shaper: shaper(ir::DriveCurve::Tanh(2.0)),
            }),
            b: vec![
                K::Compressor(COMP),
                K::Shaper(shaper(ir::DriveCurve::Tanh(2.0))),
            ],
            expect: Same(-100.0),
        },
        Pair {
            family: "Dynamics",
            name: "Compressor(feed-forward) vs Compressor(feedback)",
            a: K::Compressor(COMP),
            b: vec![K::Compressor(ir::Compressor {
                detector: ir::Detector::Output,
                ..COMP
            })],
            expect: Distinct(-40.0),
        },
        Pair {
            family: "Dynamics",
            name: "Compressor(∞:1, −18 dB) vs Limiter(−18 dB)",
            a: K::Compressor(ir::Compressor {
                threshold_db: -18.0,
                ratio: 1_000.0,
                attack_ms: 0.01,
                release_ms: 50.0,
                ..COMP
            }),
            b: vec![K::Limiter(ir::Limiter {
                ceiling_db: -18.0,
                release_ms: 50.0,
                ..ir::Limiter::PEAK
            })],
            expect: Same(-40.0),
        },
        // Drive.
        Pair {
            family: "Drive",
            name: "Shaper Tanh(2) vs Atan(2)",
            a: K::Shaper(shaper(ir::DriveCurve::Tanh(2.0))),
            b: vec![K::Shaper(shaper(ir::DriveCurve::Atan(2.0)))],
            expect: Distinct(-40.0),
        },
        Pair {
            family: "Drive",
            name: "Shaper Tanh(2) vs Saturate(2)",
            a: K::Shaper(shaper(ir::DriveCurve::Tanh(2.0))),
            b: vec![K::Shaper(shaper(ir::DriveCurve::Saturate(2.0)))],
            expect: Distinct(-40.0),
        },
        Pair {
            family: "Drive",
            name: "Shaper Soft(0) vs Gain(1)",
            a: K::Shaper(shaper(ir::DriveCurve::Soft(0.0))),
            b: vec![K::Gain(1.0)],
            expect: Distinct(-20.0),
        },
        // DelayMod.
        Pair {
            family: "DelayMod",
            name: "Delay(chorus) vs Delay(flanger, fb 0.7)",
            a: chorus(5.0, 0.0),
            b: vec![chorus(5.0, 0.7)],
            expect: Distinct(-20.0),
        },
        Pair {
            family: "DelayMod",
            name: "Phaser vs Notches",
            a: K::Phaser(PHASER),
            b: vec![K::Notches(NOTCHES)],
            expect: Distinct(-20.0),
        },
        Pair {
            family: "DelayMod",
            name: "Delay(chorus) vs Phaser",
            a: chorus(5.0, 0.0),
            b: vec![K::Phaser(PHASER)],
            expect: Distinct(-20.0),
        },
    ]
}

/// rms level, dBFS.
fn level_db(x: &[[f32; 2]]) -> f64 {
    let sq: f64 = x.iter().flatten().map(|v| f64::from(*v).powi(2)).sum();
    10.0 * (sq / x.len().max(1) as f64 / 2.0).log10()
}

type Row = (&'static str, &'static str, &'static str, f64, f64, f64, f64);

/// `(family, pair, signal, rms null dB, peak null dB, a dBFS, b dBFS)` for
/// every pair and signal.
fn measure() -> Vec<Row> {
    let signals = signals();
    let mut rows = Vec::new();
    for p in pairs() {
        for (signal, input) in &signals {
            let a = render(&wet(p.a.clone()), input);
            let b =
                p.b.iter()
                    .fold(input.clone(), |x, k| render(&wet(k.clone()), &x));
            let (rms, peak) = null_db(&a, &b);
            rows.push((
                p.family,
                p.name,
                *signal,
                rms,
                peak,
                level_db(&a),
                level_db(&b),
            ));
        }
    }
    rows
}

#[test]
fn effect_family_null_table() {
    let rows = measure();
    println!("| family | pair | signal | rms null dB | peak null dB | a dBFS | b dBFS |");
    println!("|---|---|---|---:|---:|---:|---:|");
    for (family, name, signal, rms, peak, a, b) in &rows {
        println!("| {family} | {name} | {signal} | {rms:.1} | {peak:.1} | {a:.1} | {b:.1} |");
    }
    let mut failed = Vec::new();
    for p in pairs() {
        let nulls = || rows.iter().filter(|r| r.1 == p.name).map(|r| r.3);
        let worst = nulls().fold(f64::NEG_INFINITY, f64::max);
        let fails = match p.expect {
            Expect::Same(at_most) => worst > at_most,
            Expect::Distinct(at_least) => worst < at_least,
        };
        if fails {
            failed.push(format!(
                "{}: worst null {worst:.1} dB, expected {:?}",
                p.name, p.expect
            ));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
fn renders_are_deterministic() {
    let input = &signals()[2].1;
    for p in pairs() {
        for kind in std::iter::once(p.a).chain(p.b) {
            let e = wet(kind);
            assert_eq!(render(&e, input), render(&e, input), "{:?}", e.kind);
        }
    }
}
