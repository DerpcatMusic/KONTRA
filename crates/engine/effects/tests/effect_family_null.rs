//! Null tests between effect kinds that overlap inside one `EffectFamily`
//! (13-IR-VNEXT Phase 3): each pair renders the same impulse, sine and noise
//! through both kinds' processors and measures the residual, so folding
//! `EffectKind` into families rests on measured numbers. The table prints
//! with `--nocapture` and is recorded in
//! `docs/architecture-v3/effect-family-null-report.md`.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::print_stdout)]

use std::borrow::Cow;

use kontra_dsp::delay::{Comb as DspComb, line_frames};
use kontra_effects::processors::{Dsp, Responses, apply, processor};
use kontra_effects::{MAX_SPAN, Response, matrix, summed};
use kontra_ir as ir;

const RATE: u32 = 48_000;
const FRAMES: usize = RATE as usize / 2;

/// Synthetic convolution responses used by the Convolution null pairs:
/// index 0 is a unit delta `[1.0]` (bit-exact pass-through); index 1 is a
/// short exponentially decaying stereo impulse response.
fn test_impulses() -> [Option<Response>; 2] {
    let unit = Response {
        left: vec![1.0],
        right: vec![1.0],
    };
    let len = 2_048usize;
    let mut left = Vec::with_capacity(len);
    let mut right = Vec::with_capacity(len);
    let mut seed = 0x51a7_c0de_u32;
    let mut next = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (f64::from(seed >> 8) / f64::from(1_u32 << 24)) as f32 * 2.0 - 1.0
    };
    for i in 0..len {
        let env = (-5.0 * i as f32 / len as f32).exp() * 0.25;
        left.push(next() * env);
        right.push(next() * env);
    }
    [Some(unit), Some(Response { left, right })]
}

/// Renders `signal` through the processor the rack would build for `e`,
/// span by span: the wet path only (a rack mixes `dry` and `output` after).
fn render(e: &ir::Effect, signal: &[[f32; 2]], impulses: &[Option<Response>]) -> Vec<[f32; 2]> {
    let mut dsp = processor(Some(e), RATE, &Responses(impulses, &[]));
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
        // Drive (remaining kinds: Overdrive, Clipper, Crush raw & Converter).
        Pair {
            family: "Drive",
            name: "Overdrive vs Shaper Tanh(4)",
            a: K::Overdrive(ir::Overdrive {
                split_hz: [700.0, 2_000.0, 4_000.0],
                smooth_hz: [8_000.0, 6_000.0, 3_000.0],
                drive: 4.0,
                mix: 1.0,
                level: 1.0,
                curve: Cow::Owned(
                    (0..256)
                        .map(|i| ((i as f32 - 128.0) / 32.0).tanh())
                        .collect(),
                ),
            }),
            b: vec![K::Shaper(shaper(ir::DriveCurve::Tanh(4.0)))],
            expect: Distinct(-20.0),
        },
        Pair {
            family: "Drive",
            name: "Clipper(diode) vs Shaper Tanh(4)",
            a: K::Clipper(ir::Clipper {
                drive: 4.0,
                limit: 4.5,
                tone_hz: 8_000.0,
                coupling_hz: 10.0,
                saturation: 5.5462e-6,
                slope: 22.075_055,
                asymmetry: 1.5,
                dc_hz: 1.0,
            }),
            b: vec![K::Shaper(shaper(ir::DriveCurve::Tanh(4.0)))],
            expect: Distinct(-20.0),
        },
        Pair {
            family: "Drive",
            name: "Crush(hold+6bit) vs Gain(1)",
            a: K::Crush(ir::Crush {
                rate: 0.25,
                bits: 6.0,
                noise: 0.0,
                noise_tone: 1.0,
                converter: None,
            }),
            b: vec![K::Gain(1.0)],
            expect: Distinct(-20.0),
        },
        Pair {
            family: "Drive",
            name: "Crush(raw) vs Crush(Converter)",
            a: K::Crush(ir::Crush {
                rate: 0.25,
                bits: 8.0,
                noise: 0.0,
                noise_tone: 1.0,
                converter: None,
            }),
            b: vec![K::Crush(ir::Crush {
                rate: 1.0,
                bits: 16.0,
                noise: 0.0,
                noise_tone: 1.0,
                converter: Some(ir::Converter {
                    clock_hz: 12_000.0,
                    jitter: 0.0,
                    pre_hz: 5_000.0,
                    post_hz: 5_000.0,
                    highpass_hz: 20.0,
                    step: 2.0 / 256.0,
                    mu: 255.0,
                    midrise: false,
                    dither: 0.0,
                    drive_in: 1.0,
                    drive_out: 1.0,
                    soft: 0.1,
                    trim: 1.0,
                    wet: 1.0,
                    dry: 0.0,
                }),
            })],
            expect: Distinct(-20.0),
        },
        // Dynamics (Transient).
        Pair {
            family: "Dynamics",
            name: "Transient(+6/-6 dB) vs Compressor",
            a: K::Transient(ir::Transient {
                attack: 6.0,
                sustain: -6.0,
                gain_db: 0.0,
                clip: false,
            }),
            b: vec![K::Compressor(COMP)],
            expect: Distinct(-20.0),
        },
        // Spatial / Modulation (RingMod modes).
        Pair {
            family: "Modulation",
            name: "RingMod(Oscillator) vs RingMod(Sidechain)",
            a: K::RingMod(ir::RingMod {
                hz: 220.0,
                spread: 0.0,
                bias: 0.0,
                quadrature: 0.0,
                drive: 1.0,
                highpass_hz: 20.0,
                gate: false,
                feedback: 0.0,
                source: ir::RingSource::Oscillator,
                wet: 1.0,
                dry: 0.0,
            }),
            b: vec![K::RingMod(ir::RingMod {
                hz: 220.0,
                spread: 0.0,
                bias: 0.0,
                quadrature: 0.0,
                drive: 1.0,
                highpass_hz: 20.0,
                gate: false,
                feedback: 0.0,
                source: ir::RingSource::Sidechain {
                    contour: 0.5,
                    attack: 0.005,
                    release: 0.05,
                    bandpass_hz: Some(1_000.0),
                },
                wet: 1.0,
                dry: 0.0,
            })],
            expect: Distinct(-20.0),
        },
        Pair {
            family: "Modulation",
            name: "RingMod(Oscillator) vs RingMod(Radio)",
            a: K::RingMod(ir::RingMod {
                hz: 220.0,
                spread: 0.0,
                bias: 0.0,
                quadrature: 0.0,
                drive: 1.0,
                highpass_hz: 20.0,
                gate: false,
                feedback: 0.0,
                source: ir::RingSource::Oscillator,
                wet: 1.0,
                dry: 0.0,
            }),
            b: vec![K::RingMod(ir::RingMod {
                hz: 2_000.0,
                spread: 0.0,
                bias: 0.5,
                quadrature: 0.0,
                drive: 1.0,
                highpass_hz: 20.0,
                gate: false,
                feedback: 0.0,
                source: ir::RingSource::Radio {
                    gain: 1.0,
                    tuned_hz: 2_000.0,
                    damping: 0.5,
                    makeup: 1.0,
                    product: 0.5,
                    noise: 0.0,
                },
                wet: 1.0,
                dry: 0.0,
            })],
            expect: Distinct(-20.0),
        },
        // DelayMod (Ensemble, Scanner, Shifter, CombBank).
        Pair {
            family: "DelayMod",
            name: "Ensemble(7-voice) vs Delay(chorus)",
            a: ensemble_kind(),
            b: vec![chorus(5.0, 0.0)],
            expect: Distinct(-20.0),
        },
        Pair {
            family: "DelayMod",
            name: "Scanner(LC vibrato) vs Delay(chorus)",
            a: K::Scanner(ir::Scanner {
                sections: 9,
                henry: 0.02,
                farad: 1.5e-6,
                last_farad: 0.75e-6,
                load_ohm: 1_500.0,
                shunt_ohm: [10_000.0; 6],
                taps: [0, 1, 2, 3, 4, 5, 6, 7, 8],
                hz: 6.8,
                lead: 0.25,
                split_hz: 2_000.0,
                high: 0.5,
                blend: 0.5,
                mix: 1.0,
            }),
            b: vec![chorus(5.0, 0.0)],
            expect: Distinct(-20.0),
        },
        Pair {
            family: "DelayMod",
            name: "Shifter(1.5x) vs Delay(chorus)",
            a: K::Shifter(ir::Shifter {
                time: ir::DelayTime::Ms(25.0),
                right: 1.0,
                speed: [1.5, 1.5],
                feedback: 0.3,
                cross: 0.0,
            }),
            b: vec![chorus(5.0, 0.0)],
            expect: Distinct(-20.0),
        },
        Pair {
            family: "DelayMod",
            name: "CombBank(4-voice) vs Delay(flanger, fb 0.7)",
            a: K::CombBank(ir::CombBank {
                root_hz: 110.0,
                ratio: [1.0, 1.25, 1.5, 2.0],
                detune: [1.01, 0.99, 0.98, 1.02],
                lowest_hz: 50.0,
                feedback: 0.7,
                voices: 2.0,
                damping: 0.1,
                depth: 0.5,
                rate: ir::Rate::Hz(0.9),
                offset: 0.1,
                polarity: ir::Polarity::Normal,
                mode: ir::CombMode::Scan,
                scan_depth: 0.5,
                scan_shape: ir::ScanShape::Triangle,
                width: 0.5,
                mix: 0.7,
            }),
            b: vec![chorus(5.0, 0.7)],
            expect: Distinct(-20.0),
        },
        // FilterEq (Cascade, Clocked, Formant).
        Pair {
            family: "FilterEq",
            name: "Cascade(3-stage) vs SVF LP2",
            a: K::Cascade(ir::Cascade {
                hz: 1_000.0,
                shifts: [1.0, 1.0],
                stages: [
                    ir::CascadeStage {
                        q: 0.5,
                        limit: 0.9,
                        peak: 1.0,
                        morph: 0.0,
                        wet: 1.0,
                    },
                    ir::CascadeStage {
                        q: 0.5,
                        limit: 0.9,
                        peak: 1.0,
                        morph: 0.5,
                        wet: 0.5,
                    },
                    ir::CascadeStage {
                        q: 0.5,
                        limit: 0.9,
                        peak: 1.0,
                        morph: 1.0,
                        wet: 0.5,
                    },
                ],
                gain: 1.0,
            }),
            b: vec![filter(None)],
            expect: Distinct(-20.0),
        },
        Pair {
            family: "FilterEq",
            name: "Clocked(Allpass) vs Clocked(VowelLow)",
            a: K::Clocked(ir::Clocked {
                shape: ir::ClockedShape::Allpass,
                cutoff: 0.5,
                resonance: 0.5,
            }),
            b: vec![K::Clocked(ir::Clocked {
                shape: ir::ClockedShape::VowelLow,
                cutoff: 0.5,
                resonance: 0.5,
            })],
            expect: Distinct(-20.0),
        },
        Pair {
            family: "FilterEq",
            name: "Clocked(Allpass) vs Phaser",
            a: K::Clocked(ir::Clocked {
                shape: ir::ClockedShape::Allpass,
                cutoff: 0.5,
                resonance: 0.5,
            }),
            b: vec![K::Phaser(PHASER)],
            expect: Distinct(-20.0),
        },
        Pair {
            family: "FilterEq",
            name: "Formant(First) vs Formant(Second)",
            a: K::Formant(ir::Formant {
                talk: 0.5,
                sharp: 0.5,
                size: 0.5,
                set: ir::VowelSet::First,
            }),
            b: vec![K::Formant(ir::Formant {
                talk: 0.5,
                sharp: 0.5,
                size: 0.5,
                set: ir::VowelSet::Second,
            })],
            expect: Distinct(-20.0),
        },
        // Space (Reverb networks + Convolution).
        Pair {
            family: "Space",
            name: "Reverb(FDN) vs Reverb(Hadamard)",
            a: reverb_kind(None),
            b: vec![reverb_kind(Some(ir::Network::Hadamard(
                ir::HadamardNetwork {
                    lines: 8,
                    shape: 0.0,
                    diffusers: true,
                    diffuser_ms: 5.0,
                    decay_low: 1.0,
                    decay_high: 0.8,
                    low_hz: 250.0,
                    high_hz: 10_000.0,
                    low_cut: false,
                    high_cut: true,
                    freeze: false,
                },
            )))],
            expect: Distinct(-20.0),
        },
        Pair {
            family: "Space",
            name: "Reverb(FDN) vs Reverb(Combs)",
            a: reverb_kind(None),
            b: vec![reverb_kind(Some(ir::Network::Combs(ir::Shared::Borrowed(
                &COMBS_NET,
            ))))],
            expect: Distinct(-20.0),
        },
        Pair {
            family: "Space",
            name: "Reverb(FDN) vs Reverb(Lines)",
            a: reverb_kind(None),
            b: vec![reverb_kind(Some(ir::Network::Lines(ir::Shared::Owned(
                lines_net().into(),
            ))))],
            expect: Distinct(-20.0),
        },
        Pair {
            family: "Space",
            name: "Convolution(delta) vs Gain(1)",
            a: K::Convolution { impulse: 0 },
            b: vec![K::Gain(1.0)],
            expect: Same(-120.0),
        },
        Pair {
            family: "Space",
            name: "Convolution(hall IR) vs Reverb(FDN)",
            a: K::Convolution { impulse: 1 },
            b: vec![reverb_kind(None)],
            expect: Distinct(-20.0),
        },
    ]
}

static ENSEMBLE_VOICES: ir::EnsembleVoices = ir::EnsembleVoices {
    delay: [0.003, 0.003, 0.007, 0.007, 0.011, 0.011, 0.013],
    rate: [1.0, 1.0, 1.4, 1.4, 1.7, 1.7, 0.7],
    start: [0.0, 0.5, 0.0, 0.5, 0.0, 0.5, 0.0],
    bend: [0.2; 7],
    wobble_rate: [6.0; 7],
    wobble: [0.0, 0.0, 0.0, 0.0, 0.1, 0.1, 0.0],
    smooth_hz: [3_000.0; 7],
    filter: [ir::Section {
        hz: 5_000.0,
        num: [1.0, 0.0, 0.0],
        den: [1.0, 1.414, 1.0],
    }; 7],
};

fn ensemble_kind() -> ir::EffectKind {
    ir::EffectKind::Ensemble(ir::Ensemble {
        voices: ir::Shared::Borrowed(&ENSEMBLE_VOICES),
        span: 1.0,
        hz: 0.25,
        sweep: 0.004,
        levels: [1.0, 1.0, 0.7, 0.7, 0.0, 0.0, 0.0],
        angles: [-0.1, 0.1, -0.1, 0.1, 0.0, 0.0, 0.0],
        scatter: [0.5, -0.2, -0.2, 0.3, -0.2, 0.3, 0.3],
        feedback: 0.5,
        polarity: ir::Polarity::Normal,
        input: 0.9,
        wet: 0.3,
        direct: 0.7,
        shelf: ir::Section {
            hz: 30.0,
            num: [0.5, 1.0, 1.0],
            den: [1.0, 1.414, 1.0],
        },
    })
}

static COMBS_NET: ir::CombNetwork = ir::CombNetwork {
    combs_ms: [25.3, 26.9, 28.9, 30.7, 32.2, 33.8, 35.3, 36.7],
    allpasses_ms: [5.1, 7.7, 10.0, 12.6],
    spread_ms: 0.52,
    input_gain: 0.15,
};

fn lines_net() -> Box<ir::ReverbNetwork> {
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
    Box::new(ir::ReverbNetwork {
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
    })
}

fn reverb_kind(network: Option<ir::Network>) -> ir::EffectKind {
    ir::EffectKind::Reverb(ir::Reverb {
        seconds: 1.8,
        size: 0.06,
        network,
        damping: 0.4,
        diffusion: 0.7,
        predelay_ms: 0.0,
        high_cut_hz: 16_000.0,
        low_shelf_db: 0.0,
        stereo: 1.0,
        modulation_ms: 0.5,
        modulation_hz: 1.0,
    })
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
    let impulses = test_impulses();
    let mut rows = Vec::new();
    for p in pairs() {
        for (signal, input) in &signals {
            let a = render(&wet(p.a.clone()), input, &impulses);
            let b =
                p.b.iter()
                    .fold(input.clone(), |x, k| render(&wet(k.clone()), &x, &impulses));
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
    let impulses = test_impulses();
    for p in pairs() {
        for kind in std::iter::once(p.a).chain(p.b) {
            let e = wet(kind);
            assert_eq!(
                render(&e, input, &impulses),
                render(&e, input, &impulses),
                "{:?}",
                e.kind
            );
        }
    }
}

/// Voice-level `ir::EffectKind::Comb` harness (runs per voice via `kontra_dsp::delay::Comb`).
fn render_voice_comb(c: ir::Comb, signal: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut comb = DspComb::new(c.hz, c.feedback, c.forward, RATE);
    let mut line = vec![[0.0f32; 2]; line_frames(RATE, ir::COMB_LOWEST_HZ)];
    let mut out = signal.to_vec();
    for span in out.chunks_mut(MAX_SPAN as usize) {
        comb.process(span, &mut line);
    }
    assert!(out.iter().flatten().all(|s| s.is_finite()));
    out
}

#[test]
fn voice_comb_null_table() {
    let signals = signals();
    let fb = ir::Comb {
        hz: 440.0,
        feedback: 0.7,
        forward: false,
        octaves: 10.0,
    };
    let ff = ir::Comb {
        forward: true,
        ..fb
    };
    let dry = ir::Comb {
        feedback: 0.0,
        ..fb
    };
    println!("| family | pair | signal | rms null dB | peak null dB | a dBFS | b dBFS |");
    println!("|---|---|---|---:|---:|---:|---:|");
    let mut worst_fb_ff = f64::NEG_INFINITY;
    let mut worst_dry = f64::NEG_INFINITY;
    for (sig, input) in &signals {
        let a = render_voice_comb(fb, input);
        let b = render_voice_comb(ff, input);
        let (rms, peak) = null_db(&a, &b);
        worst_fb_ff = worst_fb_ff.max(rms);
        println!(
            "| DelayMod | Comb(feedback) vs Comb(feed-forward) | {sig} | {rms:.1} | {peak:.1} | {:.1} | {:.1} |",
            level_db(&a),
            level_db(&b)
        );
        let c = render_voice_comb(dry, input);
        let (rms_d, peak_d) = null_db(&c, input);
        worst_dry = worst_dry.max(rms_d);
        println!(
            "| DelayMod | Comb(fb=0) vs Gain(1) | {sig} | {rms_d:.1} | {peak_d:.1} | {:.1} | {:.1} |",
            level_db(&c),
            level_db(input)
        );
    }
    assert!(
        worst_fb_ff >= -20.0,
        "expected distinct Comb modes: {worst_fb_ff}"
    );
    assert!(
        worst_dry <= -120.0,
        "expected bit-exact Comb(fb=0): {worst_dry}"
    );
    // Determinism check for both modes.
    let noise = &signals[2].1;
    assert_eq!(render_voice_comb(fb, noise), render_voice_comb(fb, noise));
    assert_eq!(render_voice_comb(ff, noise), render_voice_comb(ff, noise));
}
