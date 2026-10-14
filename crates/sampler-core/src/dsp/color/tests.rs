//! Preset-level tests: each native effect, as configured on the superset, equals
//! its frozen single-pass law bit for bit, and block partitioning never matters.
use super::*;
use super::Settings as ColorSettings;
use crate::dsp::BLOCK;

fn signal(seed: f64, len: usize) -> Planar {
    let mut block = [[0.; BLOCK]; 2];
    for c in 0..2 {
        for i in 0..len {
            block[c][i] = ((i as f64 * 0.37 + seed + c as f64 * 1.3).sin() * 0.8)
                + ((i as f64 * 0.11).cos() * 0.15);
        }
    }
    block
}

fn color(settings: ColorSettings) -> Color {
    assert!(settings.valid());
    settings.compile(48_000, &mut Vec::new())
}

fn run(color: &Color, state: &mut State, block: &mut Planar, len: usize) {
    color.process(state, &[], block, len, 0, &mut []);
}

/// Output of `settings` over `blocks` consecutive blocks of `len` frames.
fn render(settings: ColorSettings, blocks: usize, len: usize) -> Vec<[f64; 2]> {
    let color = color(settings);
    let mut state = State::default();
    let mut out = Vec::new();
    for b in 0..blocks {
        let mut block = signal(b as f64 * 7.0, len);
        run(&color, &mut state, &mut block, len);
        out.extend((0..len).map(|i| [block[0][i], block[1][i]]));
    }
    out
}

fn knob(v: f64) -> Parameter {
    Parameter::Constant(v)
}

#[test]
fn gainer_matches_the_frozen_slew_equation() {
    for dry in [0., 0.5] {
        let settings = ColorSettings {
            gainer: Some(GainerSettings { gain: knob(0.25), dry }),
            ..ColorSettings::IDENTITY
        };
        let got = render(settings, 3, 40);
        // y = x * (dry + g); g starts at the first target and stays (target is constant).
        let mut expected = Vec::new();
        for b in 0..3 {
            let input = signal(b as f64 * 7.0, 40);
            for i in 0..40 {
                let m = dry + f64::from(0.25f32);
                expected.push([input[0][i] * m, input[1][i] * m]);
            }
        }
        assert_eq!(got, expected);
    }
}

#[test]
fn gainer_slews_toward_a_moving_target_with_the_native_coefficient() {
    // A control ramp from 1.0 down to 0.0: g[n+1] = g[n] + (t - g[n]) * 0.00055555...
    let mut bindings = Vec::new();
    let settings = ColorSettings {
        gainer: Some(GainerSettings {
            gain: Parameter::Control(ControlRange {
                control: crate::ControlId(0),
                low: 0.,
                high: 1.,
                ramp_frames: 0,
            }),
            dry: 0.,
        }),
        ..ColorSettings::IDENTITY
    };
    let color = settings.compile(48_000, &mut bindings);
    let ramps = [ControlRamp::test_ramp(1., 0., 0, 64)];
    let mut state = State::default();
    let mut block = [[1.; BLOCK]; 2];
    color.process(&mut state, &ramps, &mut block, BLOCK, 0, &mut []);
    let mut g = 1f32;
    for i in 0..BLOCK {
        let target = ramps[0].value(i as u64) as f32;
        assert_eq!(block[0][i], f64::from(g), "frame {i}");
        g += (target - g) * f32::from_bits(0x3a11a2b4);
    }
}

/// The pre-wave-4 fused Lo-Fi loop (hold, quantize, noise per frame, channel by
/// channel), kept as the frozen law the split stages must equal.
fn frozen_lofi(
    (bits, frequency, noise, color): (f32, f32, f32, f32),
    blocks: usize,
    len: usize,
) -> Vec<[f64; 2]> {
    let active = f32::from_bits(0x3f7f_ff58);
    let quant = (bits < active).then(|| {
        let q = 2f64.powf(f64::from((16.0 * bits).max(1.0))) as f32;
        (q, 1.0 / q)
    });
    let fs = 48_000f32;
    let period = (frequency < active).then(|| (fs / (fs * frequency).max(50.0)).max(1.0));
    let (noise_active, gain, alpha) = (
        noise >= 0.001,
        (f64::from(noise) * 0.075) as f32,
        1.0 - 0.9 * color.clamp(0.0, 1.0),
    );
    let (mut held, mut h, mut phase0, mut seed) = ([0f32; 2], [0f32; 2], 0f32, 0x9E37_79B9u32);
    let mut out = Vec::new();
    for b in 0..blocks {
        let mut block = signal(b as f64 * 7.0, len);
        let start = phase0;
        let mut end = start;
        for ch in 0..2 {
            let mut phase = start;
            for i in 0..len {
                let mut y = block[ch][i] as f32;
                if let Some(p) = period {
                    if phase >= p {
                        phase -= p;
                        held[ch] = y;
                    } else {
                        phase += 1.0;
                    }
                    y = held[ch];
                }
                if let Some((q, inverse)) = quant {
                    y = ((y * q) as i32) as f32 * inverse;
                }
                if noise_active {
                    seed = seed.wrapping_mul(0x0bb3_8435).wrapping_add(0x3619_636b);
                    let u = f32::from_bits((seed & 0x007f_ffff) | 0x4000_0000) - 3.0;
                    h[ch] += alpha * (u - h[ch]);
                    y += h[ch] * gain;
                }
                block[ch][i] = f64::from(y);
            }
            end = phase;
        }
        phase0 = end;
        out.extend((0..len).map(|i| [block[0][i], block[1][i]]));
    }
    out
}

#[test]
fn lofi_preset_equals_the_frozen_fused_loop_for_every_block_size() {
    for knobs in [
        (0.4, 0.2, 0.3, 0.5),
        (0.0, 0.5, 1.0, 0.0),
        (1.0, 1.0, 0.0, 0.7), // all controls at their inactive end
        (0.25, 1.0, 0.5, 1.0),
        (1.0, 0.05, 0.0005, 0.2),
    ] {
        let (bits, frequency, noise, color) = knobs;
        let settings = ColorSettings {
            hold: Some(HoldSettings::Native(knob(f64::from(frequency)))),
            quantizer: Some(QuantizerSettings::Native(knob(f64::from(bits)))),
            noise: Some(NoiseSettings {
                level: knob(f64::from(noise)),
                color: knob(f64::from(color)),
            }),
            ..ColorSettings::IDENTITY
        };
        for len in [1, 7, 33, BLOCK] {
            let got = render(settings, 5, len);
            let want = frozen_lofi(knobs, 5, len);
            // Inactive stages skip the binary32 rounding the fused loop always applies.
            for (g, w) in got.iter().zip(&want) {
                for c in 0..2 {
                    assert!((g[c] - w[c]).abs() <= 1e-7, "{knobs:?} len {len}: {} vs {}", g[c], w[c]);
                    if knobs.0 < 1.0 || knobs.1 < 1.0 || knobs.2 >= 0.001 {
                        assert_eq!(g[c].to_bits(), w[c].to_bits(), "{knobs:?} len {len}");
                    }
                }
            }
        }
    }
}

/// The pre-wave-4 fused Distortion loop, frozen.
fn frozen_distortion(
    transistor: bool,
    drive: f32,
    damping: f32,
    blocks: usize,
    len: usize,
) -> Vec<[f64; 2]> {
    let shape = if transistor {
        distortion::Shape::transistor(drive)
    } else {
        distortion::Shape::tube(drive)
    };
    let pole = distortion::distortion_damping(damping, 48_000.);
    let [b0, b1, b2, a1, a2] = distortion::distortion_dc(48_000.);
    let b = 0.5 * (1. - pole);
    let mut hist = [[0f32; 5]; 2];
    let mut out = Vec::new();
    for blk in 0..blocks {
        let mut block = signal(blk as f64 * 7.0, len);
        for ch in 0..2 {
            let [mut previous, mut lp, mut lp2, mut hp, mut hp2] = hist[ch];
            for i in 0..len {
                let y = shape.apply(block[ch][i] as f32);
                let damped = b * y + b * previous + pole * lp + 1e-20;
                let output = (((damped * b0 + lp * b1) + lp2 * b2) + hp * a1) + hp2 * a2;
                (previous, lp2, lp, hp2, hp) = (y, lp, damped, hp, output);
                block[ch][i] = f64::from(output);
            }
            hist[ch] = [previous, lp, lp2, hp, hp2];
        }
        out.extend((0..len).map(|i| [block[0][i], block[1][i]]));
    }
    out
}

#[test]
fn distortion_preset_equals_the_frozen_fused_loop() {
    for (transistor, drive, damping) in
        [(false, 0.523648, 0.0), (false, 1.0, 0.5), (true, 0.8, 1.0), (true, 0.0, 0.3)]
    {
        let settings = ColorSettings {
            curve: if transistor {
                CurveSettings::Transistor(knob(f64::from(drive)))
            } else {
                CurveSettings::Tube(knob(f64::from(drive)))
            },
            damping: Some(knob(f64::from(damping))),
            ..ColorSettings::IDENTITY
        };
        for len in [1, 5, BLOCK] {
            assert_eq!(render(settings, 4, len), frozen_distortion(transistor, drive, damping, 4, len));
        }
    }
}

#[test]
fn classic_curve_and_rectifier_are_memoryless() {
    let settings = ColorSettings { curve: CurveSettings::Classic(knob(0.5)), ..ColorSettings::IDENTITY };
    assert!(settings.memoryless());
    let got = render(settings, 1, 16);
    let input = signal(0., 16);
    for i in 0..16 {
        assert_eq!(got[i][0], shaping::saturation(input[0][i], 0.5));
    }
    let settings =
        ColorSettings { curve: CurveSettings::Rectifier(Rectifier::Half), ..ColorSettings::IDENTITY };
    let got = render(settings, 1, 16);
    for i in 0..16 {
        assert_eq!(got[i][1], input[1][i].max(0.));
    }
}

/// UVI BitCrusher kernel 0x1412d1770, as verify_shape_decimation.py computes it.
fn frozen_crusher(bits: f64, rate_hz: f64, drive: f64, mix: f64, len: usize) -> Vec<[f64; 2]> {
    let scale = 2f64.powf(bits) as f32;
    let reciprocal = 1.0 / scale;
    let scaled_drive = (drive as f32) * 0.95f32;
    let amount = scaled_drive / (1.0 - scaled_drive);
    let normalization = (amount + 1.0) * 0.5;
    let interval = (48_000f32) / (rate_hz as f32);
    let mix = mix as f32;
    let input = signal(3.0, len);
    let mut out = vec![[0.; 2]; len];
    for c in 0..2 {
        let (mut held, mut phase) = (0f32, 0f32);
        for i in 0..len {
            let sample = input[c][i] as f32;
            if i as f32 >= phase {
                held = sample;
                phase += interval;
            }
            let q = held * scale;
            let q = q.floor() + f32::from(u8::from(q < 0.0));
            let value = q * reciprocal;
            let value = value + value;
            let value = value / (value.abs() * amount + 1.0) * normalization;
            out[i][c] = f64::from(value * mix + (1.0 - mix) * sample);
        }
    }
    out
}

#[test]
fn uvi_bit_crusher_preset_equals_the_native_kernel() {
    for (bits, rate_hz, drive, mix) in
        [(2., 14_000., 0.5, 0.5), (8., 48_000., 0., 1.), (16., 14_000., 0.5, 0.), (4., 6_000., 0.9, 0.75)]
    {
        let settings = ColorSettings {
            hold: Some(HoldSettings::Rate(rate_hz)),
            quantizer: Some(QuantizerSettings::Floor(bits)),
            curve: CurveSettings::Destructor(drive),
            mix,
            ..ColorSettings::IDENTITY
        };
        let color = color(settings);
        let mut state = State::default();
        let mut block = signal(3.0, 33);
        run(&color, &mut state, &mut block, 33);
        let want = frozen_crusher(bits, rate_hz, drive, mix, 33);
        for i in 0..33 {
            assert_eq!([block[0][i], block[1][i]], want[i], "bits {bits} rate {rate_hz} frame {i}");
        }
    }
}

#[test]
fn invalid_settings_are_rejected() {
    let bad = [
        ColorSettings { curve: CurveSettings::Tube(knob(1.5)), ..ColorSettings::IDENTITY },
        ColorSettings { mix: 1.5, ..ColorSettings::IDENTITY },
        ColorSettings { hold: Some(HoldSettings::Rate(0.0)), ..ColorSettings::IDENTITY },
        ColorSettings { quantizer: Some(QuantizerSettings::Floor(64.0)), ..ColorSettings::IDENTITY },
        ColorSettings { curve: CurveSettings::Classic(knob(-2.0)), ..ColorSettings::IDENTITY },
    ];
    for settings in bad {
        assert!(!settings.valid());
    }
    assert!(ColorSettings::IDENTITY.valid());
}

#[test]
fn only_stateless_units_batch() {
    let batched = ColorSettings {
        gainer: Some(GainerSettings { gain: knob(1.), dry: 0. }),
        curve: CurveSettings::Classic(knob(0.2)),
        ..ColorSettings::IDENTITY
    };
    assert!(color(batched).batches());
    let held = ColorSettings { hold: Some(HoldSettings::Native(knob(0.5))), ..ColorSettings::IDENTITY };
    assert!(!color(held).batches());
}

/// UVI color units configured through their IR presets against the emulated native
/// (uvicolor vectors, 48 kHz, 64-frame calls): impulse 0.5 on the left (both channels
/// checked) and a 440 Hz sine on both.
fn uvi_unit(name: &str, preset: impl Fn(&serde_json::Value) -> sampler_ir::fx::color::Config, tolerance: f64) {
    let vectors: serde_json::Value = serde_json::from_str(include_str!("tables/uvi_color_vectors.json")).unwrap();
    let series = |v: &serde_json::Value| -> Vec<f64> { v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect() };
    let mut failures = Vec::new();
    for case in vectors[name].as_array().unwrap() {
        let p = &case["params"];
        let unit = color(crate::lower::color_settings(preset(p)));
        let impulse: Vec<Vec<f64>> = case["impulse"].as_array().unwrap().iter().map(series).collect();
        let sine_out = series(&case["sine_out"]);
        let sine: Vec<f64> =
            (0..256).map(|i| 0.8 * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 48_000.0).sin()).collect();
        let pulse: Vec<f64> = (0..256).map(|i| if i == 0 { 0.5 } else { 0.0 }).collect();
        for (label, input, native) in [
            ("impulse", [pulse, vec![0.0; 256]], [impulse[0].clone(), impulse[1].clone()]),
            ("sine", [sine.clone(), sine], [sine_out.clone(), sine_out]),
        ] {
            let mut state = State::default();
            let mut out = [vec![], vec![]];
            for start in (0..256).step_by(64) {
                let mut block = [[0.; BLOCK]; 2];
                for c in 0..2 {
                    for i in 0..64 {
                        block[c][i] = f64::from(input[c][start + i] as f32);
                    }
                }
                run(&unit, &mut state, &mut block, 64);
                for c in 0..2 {
                    out[c].extend_from_slice(&block[c][..64]);
                }
            }
            for c in 0..2 {
                let scale = native[c].iter().fold(1e-3f64, |m, v| m.max(v.abs()));
                let (i, err) = out[c]
                    .iter()
                    .zip(&native[c])
                    .map(|(a, b)| (a - b).abs() / scale)
                    .enumerate()
                    .fold((0, 0f64), |m, (i, e)| if e > m.1 { (i, e) } else { m });
                if !(err <= tolerance) {
                    failures.push(format!("{name} {p} {label} ch{c}: err {err:.3e} at {i}: {} vs {}", out[c][i], native[c][i]));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn param(p: &serde_json::Value, key: &str, default: f64) -> f64 {
    p[key].as_f64().unwrap_or(default)
}

#[test]
fn uvi_formant_crusher_matches_the_native() {
    // The emulated unit keeps its constructor's Bite 0.2 and FilterB 1 where a case omits them.
    uvi_unit(
        "FormantCrusher",
        |p| {
            let f = |k, d| param(p, k, d) as f32;
            sampler_ir::fx::color::Config::formant_crusher(
                f("Morph", 0.0),
                f("Q", 10.0),
                f("Formant", 0.0),
                f("Mix", 1.0),
                f("Bite", 0.2),
                f("FilterA", 0.0) as u8,
                f("FilterB", 1.0) as u8,
            )
        },
        1e-6,
    );
}

#[test]
fn uvi_diode_clipper_matches_the_native() {
    uvi_unit(
        "DiodeClipper",
        |p| {
            let f = |k, d| param(p, k, d) as f32;
            let d = sampler_ir::fx::color::Diode {
                drive: f("Drive", 0.0),
                tone: f("Tone", 20_000.0),
                high_pass: f("HighPass", 1.0),
                asymmetry: f("Asymmetry", 0.0),
                output_gain: f("OutputGain", 0.0),
            };
            sampler_ir::fx::color::Config { curve: sampler_ir::fx::color::Curve::Diode(d), ..sampler_ir::fx::color::Config::IDENTITY }
        },
        1e-6,
    );
}

#[test]
fn uvi_tone_stack_matches_the_native() {
    uvi_unit(
        "ToneStack",
        |p| {
            let f = |k, d| param(p, k, d) as f32;
            let s = sampler_ir::fx::color::ToneStack {
                bass: f("Bass", 0.5),
                mid: f("Mid", 0.5),
                treble: f("Treble", 0.5),
                gain: f("Gain", 0.3),
                model: f("Model", 0.0) as u8,
            };
            sampler_ir::fx::color::Config { curve: sampler_ir::fx::color::Curve::ToneStack(s), ..sampler_ir::fx::color::Config::IDENTITY }
        },
        1e-6,
    );
}

#[test]
fn uvi_exciter_matches_the_native() {
    // Tape runs the native vector sinf; ours is the scalar libm one (about 1 ulp).
    uvi_unit(
        "Exciter",
        |p| {
            let f = |k, d| param(p, k, d) as f32;
            let e = sampler_ir::fx::color::Exciter {
                mode: f("Mode", 1.0) as u8,
                input_gain: f("InputGain", 0.0),
                amount: f("Amount", 0.0),
                mix: f("Mix", 100.0),
                output_gain: f("OutputGain", 0.0),
                oversampling: f("Oversampling", 0.0) as u8,
            };
            sampler_ir::fx::color::Config { curve: sampler_ir::fx::color::Curve::Exciter(e), ..sampler_ir::fx::color::Config::IDENTITY }
        },
        2e-6,
    );
}
