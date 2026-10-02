use super::*;
use crate::audio::Sample;
use ni_file::kontakt::StructuredObject;

const SR: f32 = 48_000.0;

fn effect(kind: Kind, params: Params) -> Effect {
    Effect {
        slot: 0,
        kind,
        version: 0,
        bypass: false,
        output_gain: 1.0,
        dry_level: 0.0,
        params,
    }
}

fn gainer(gain: f32) -> Effect {
    effect(Kind::Gainer, Params::Gainer(params::Gainer { gain }))
}

fn convolution(ir: &[f32]) -> Effect {
    let bytes = {
        let mut b: Vec<u8> = [-1.0f32, 0.0, 0.0, 1.0, 20.0, 20e3, 1.0, 20.0, 20e3, -1.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        // Raw-gain fixtures deliberately disable native Auto Gain.
        b.extend([0, 0, 1, 1, 0]);
        b.extend([0u8; 8]);
        b.extend(0i32.to_le_bytes());
        b
    };
    let mut fx = effect(Kind::Convolution, params::parse(Kind::Convolution, &bytes));
    let Params::Convolution(c) = &mut fx.params else {
        panic!("convolution layout")
    };
    c.ir = Some(Impulse(Arc::new(Sample {
        rate: SR as u32,
        frames: ir.iter().map(|&v| [v, v]).collect(),
    })));
    fx
}

#[test]
fn convolution_reverse_preserves_asymmetric_ir_gain_predelay_and_rate_without_heap() {
    let source = [[0.125, 2.0], [-0.5, 0.0], [0.0, -0.25], [1.5, 0.0],
                  [0.25, 1.0], [0.0, 0.0], [0.75, -0.125], [-0.125, 0.5]];
    for reverse in [false, true] {
        for rate in [SR, SR / 2.0] {
            let mut fx = convolution(&[1.0]);
            let Params::Convolution(c) = &mut fx.params else { unreachable!() };
            c.flags[0] = reverse;
            c.predelay_ms = 3.0 / SR * 1000.0;
            c.ir = Some(Impulse(Arc::new(Sample { rate: SR as u32, frames: source.to_vec() })));
            assert_eq!(c.reversed(), reverse);
            assert!(!c.auto_gain() && c.preserve_length_ir() && c.bypass_latency_compensation() && !c.envelope_active());
            let fx = ProgramFx { insert: Chain { slots: vec![fx] }, ..Default::default() };
            assert!(fx.warnings().is_empty());
            let mut p = fx.processor(rate, 4);
            let mut output = [[0.0; 2]; 24];
            assert_eq!(crate::plugin::tests::allocations(|| {
                for block in 0..6 {
                    let (mut left, mut right) = ([0.0; 4], [0.0; 4]);
                    if block == 0 { left[0] = 1.0; right[0] = 1.0; }
                    p.process(&mut left, &mut right);
                    for n in 0..4 { output[block * 4 + n] = [left[n], right[n]]; }
                }
            }), 0);
            let stride = (SR / rate) as usize;
            let pre = (3.0 * rate / SR) as usize;
            for (n, actual) in output.iter().enumerate() {
                let index = n.checked_sub(pre).map(|i| i * stride).filter(|&i| i < source.len());
                let expected = index.map_or([0.0; 2], |i| source[if reverse { source.len() - 1 - i } else { i }]);
                for ch in 0..2 {
                    assert!((actual[ch] - expected[ch]).abs() < 1e-6, "reverse={reverse} rate={rate} frame={n} channel={ch}");
                }
            }
        }
    }
    let mut fx = convolution(&[1.0]);
    let Params::Convolution(c) = &mut fx.params else { unreachable!() };
    c.flags[1] = true;
    c.flags[4] = true;
    let fx = ProgramFx { insert: Chain { slots: vec![fx] }, ..Default::default() };
    assert!(fx.warnings().iter().any(|w| w.contains("Auto Gain uses the approximated IR")));
    assert!(fx.warnings().iter().any(|w| w.contains("Volume Envelope is not applied")));
}

#[test]
fn convolution_auto_gain_uses_prepared_stereo_energy_and_preserves_dry_without_heap() {
    // A louder right channel must determine the same wet gain for both sides.
    // Small responses exercise the native low-energy threshold and 2x cap.
    for (source, reference_gain) in [
        (vec![[1.0, 2.0], [0.0, -1.0]], (0.5f32 / 5.0).sqrt()),
        (vec![[0.25, 0.125]], 2.0),
        (vec![[0.01, 0.02]], 1.0),
        (vec![[0.0, 0.0]], 1.0),
        (vec![[0.5, 0.5]], 2.0f32.sqrt()),
        (vec![], 1.0),
    ] {
        for automatic in [false, true] {
            let mut fx = convolution(&[1.0]);
            fx.output_gain = 0.5;
            fx.dry_level = 0.25;
            let Params::Convolution(c) = &mut fx.params else { unreachable!() };
            c.flags[1] = automatic;
            c.ir = Some(Impulse(Arc::new(Sample { rate: SR as u32, frames: source.clone() })));
            let fx = ProgramFx { insert: Chain { slots: vec![fx] }, ..Default::default() };
            assert!(fx.warnings().is_empty());
            let mut p = fx.processor(SR, 4);
            let (mut left, mut right) = ([1.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]);
            assert_eq!(crate::plugin::tests::allocations(|| p.process(&mut left, &mut right)), 0);
            let gain = if automatic { reference_gain } else { 1.0 };
            for n in 0..4 {
                for (ch, out) in [left[n], right[n]].into_iter().enumerate() {
                    let wet = source.get(n).map_or(0.0, |v| v[ch]) * gain * 0.5;
                    let dry = if n == 0 { 0.25 } else { 0.0 };
                    assert!((out - wet - dry).abs() < 1e-6, "automatic={automatic} frame={n} channel={ch}: {out}");
                }
            }
        }
    }
    // At half the source rate, 1.5x size has a 4/3 source-frame stride.
    // Gain must follow this prepared response, before its independent wet mix.
    let source = [[1.0, 0.0], [0.0, 0.0], [0.0, 2.0], [0.0, 0.0]];
    let prepared = [[1.0, 0.0], [0.0, 2.0 / 3.0], [0.0, 2.0 / 3.0]];
    let mut fx = convolution(&[1.0]);
    let Params::Convolution(c) = &mut fx.params else { unreachable!() };
    c.flags[1] = true;
    c.early.length_ratio = 1.5;
    c.late.length_ratio = 1.5;
    c.predelay_ms = 0.1; // 2.4 frames, truncated to two.
    c.ir = Some(Impulse(Arc::new(Sample { rate: SR as u32, frames: source.to_vec() })));
    let fx = ProgramFx { insert: Chain { slots: vec![fx] }, ..Default::default() };
    let mut p = fx.processor(SR / 2.0, 8);
    let (mut left, mut right) = ([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    assert_eq!(crate::plugin::tests::allocations(|| p.process(&mut left, &mut right)), 0);
    for n in 0usize..8 {
        let expected = n.checked_sub(2).and_then(|i| prepared.get(i)).copied().unwrap_or([0.0; 2]);
        for ch in 0..2 {
            let out = [left[n], right[n]][ch];
            assert!((out - expected[ch] * 0.5f32.sqrt()).abs() < 2e-6, "frame={n} channel={ch}: {out}");
        }
    }
}

#[test]
fn convolution_envelope_interpolates_amplitudes_before_auto_gain_and_predelay_without_heap() {
    let times = [0.0, 0.125, 0.25, 0.25, 0.5, 0.625, 0.75, 1.0];
    let gains = [0.25f32, 0.5, 1.0, 2.0, 0.5, 0.25, 1.0, 0.5];
    // Numeric references distinguish amplitude interpolation from dB ramps,
    // and preserve the later endpoint after two knots round to one frame.
    for (rate, size, expected, energy) in [
        (SR, 1.0, vec![0.25,0.5,2.0,1.25,0.5,0.25,1.0,0.75], 31.0f32),
        (SR / 2.0, 1.0, vec![0.25,2.0,0.5,1.0], 21.25),
        (SR / 2.0, 1.5, vec![0.25,0.5,2.0,0.5,0.25,1.0], 22.5),
    ] {
        for active in [false, true] {
            for automatic in [false, true] {
                let mut fx = convolution(&[1.0]);
                let Params::Convolution(c) = &mut fx.params else { unreachable!() };
                c.flags[1] = automatic;
                c.flags[4] = active;
                c.early.length_ratio = size;
                c.late.length_ratio = size;
                c.predelay_ms = 2.25 / rate * 1000.0;
                c.ir = Some(Impulse(Arc::new(Sample { rate: SR as u32, frames: vec![[1.0,2.0];8] })));
                // Deliberately unsorted, with duplicate times kept in order.
                let order = [7,2,0,6,3,1,5,4];
                c.curve_x = order.map(|i| times[i]).to_vec();
                c.curve_db = order.map(|i| 20.0 * gains[i].log10()).to_vec();
                let fx = ProgramFx { insert: Chain { slots: vec![fx] }, ..Default::default() };
                assert!(fx.warnings().is_empty());
                let mut p = fx.processor(rate, 16);
                let (mut left, mut right) = ([0.0;16], [0.0;16]);
                left[0] = 1.0; right[0] = 1.0;
                assert_eq!(crate::plugin::tests::allocations(|| p.process(&mut left, &mut right)), 0);
                let energy = if active { energy } else { 4.0 * expected.len() as f32 };
                let gain = if automatic { (0.5 / energy).sqrt() } else { 1.0 };
                for n in 0usize..16 {
                    let shape = n.checked_sub(2).filter(|&i| i < expected.len())
                        .map_or(0.0, |i| if active { expected[i] } else { 1.0 });
                    for ch in 0..2 {
                        let out = [left[n],right[n]][ch];
                        let want = shape * (ch + 1) as f32 * gain;
                        assert!((out - want).abs() < 3e-6, "rate={rate} size={size} active={active} automatic={automatic} frame={n} channel={ch}: {out} vs {want}");
                    }
                }
            }
        }
    }
    // Native envelope leaves samples before its first/after its last knot
    // unchanged; invalid active records remain explicit instead of guessed.
    let mut fx = convolution(&[1.0;8]);
    let Params::Convolution(c) = &mut fx.params else { unreachable!() };
    c.flags[4] = true;
    c.curve_x = vec![0.25,0.3,0.35,0.4,0.5,0.6,0.7,0.75];
    c.curve_db = vec![20.0 * 0.5f32.log10();8];
    let fx = ProgramFx { insert: Chain { slots: vec![fx] }, ..Default::default() };
    let mut p = fx.processor(SR, 8);
    let (mut left, mut right) = ([1.,0.,0.,0.,0.,0.,0.,0.], [1.,0.,0.,0.,0.,0.,0.,0.]);
    assert_eq!(crate::plugin::tests::allocations(|| p.process(&mut left, &mut right)), 0);
    for (actual, expected) in left.into_iter().zip([1.,1.,0.5,0.5,0.5,0.5,1.,1.]) {
        assert!((actual - expected).abs() < 1e-6);
    }
    let mut bad = fx;
    let Params::Convolution(c) = &mut bad.insert.slots[0].params else { unreachable!() };
    c.curve_x.pop();
    assert!(bad.warnings().iter().any(|w| w.contains("Volume Envelope is not applied")));
}

#[test]
fn convolution_uniform_filters_shape_audio_and_keep_unknown_splits_explicit() {
    let make = |low, high, unequal| {
        let mut fx = convolution(&[1.0]);
        let Params::Convolution(c) = &mut fx.params else { unreachable!() };
        c.early.low_cut_hz = low;
        c.late.low_cut_hz = if unequal { 20.0 } else { low };
        c.early.high_cut_hz = high;
        c.late.high_cut_hz = high;
        ProgramFx { insert: Chain { slots: vec![fx] }, ..Default::default() }
    };
    let gain = |fx: &ProgramFx, hz: f32| {
        let mut p = fx.processor(SR, 64);
        let mut energy = 0.0;
        let mut input_energy = 0.0;
        for block in 0..128 {
            let mut left = std::array::from_fn::<_, 64, _>(|i| {
                (2.0 * std::f32::consts::PI * hz * (block * 64 + i) as f32 / SR).sin()
            });
            let mut right = left;
            let input = left;
            p.process(&mut left, &mut right);
            assert!(left.iter().chain(&right).all(|v| v.is_finite()));
            if block >= 64 {
                energy += left.iter().map(|v| v * v).sum::<f32>();
                input_energy += input.iter().map(|v| v * v).sum::<f32>();
            }
        }
        (energy / input_energy).sqrt()
    };
    let hp = make(1000.0, 20_000.0, false);
    assert!(hp.warnings().is_empty());
    assert!(gain(&hp, 100.0) < 0.02);
    assert!((gain(&hp, 4000.0) - 1.0).abs() < 0.02);
    let lp = make(20.0, 1000.0, false);
    assert!(gain(&lp, 8000.0) < 0.02);
    assert!((gain(&lp, 100.0) - 1.0).abs() < 0.02);
    let split = make(1000.0, 20_000.0, true);
    assert!(split.warnings().iter().any(|w| w.contains("independent early/late IR filtering")));
    assert!((gain(&split, 100.0) - 1.0).abs() < 1e-5, "do not apply one band's cutoff to the other band");
    let default = make(20.0, 20_000.0, false);
    assert!((gain(&default, 100.0) - 1.0).abs() < 1e-5);
    let mut sized = default;
    let Params::Convolution(c) = &mut sized.insert.slots[0].params else { unreachable!() };
    c.early.length_ratio = 1.5;
    c.late.length_ratio = 1.5;
    assert!(sized.warnings().is_empty(), "uniform sizing already plays");
    let Params::Convolution(c) = &mut sized.insert.slots[0].params else { unreachable!() };
    c.early.length_ratio = 1.0;
    assert!(sized.warnings().iter().any(|w| w.contains("independent early/late IR sizing")));
}

#[test]
fn live_ir_swap_keeps_the_slot_mix_and_rejects_other_effects() {
    let fx = ProgramFx { insert: Chain { slots: vec![convolution(&[1.0])] }, ..Default::default() };
    let mut p = fx.processor(SR, 64);
    p.set_param(Rack::Insert, 0, FxParam::Wet, 0.25);
    p.set_param(Rack::Insert, 0, FxParam::Dry, 0.5);
    let loaded = ScriptIr { rack: Rack::Insert, slot: 0, load: Load::Ir {
        file: "other.wav".into(),
        ir: Impulse(Arc::new(Sample { rate: SR as u32, frames: vec![[2.0; 2]] })),
    }};
    let ir = fx.prepare_ir(Rack::Insert, 0, SR, 64, std::slice::from_ref(&loaded)).unwrap();
    let old = p.replace_ir(ir).ok().expect("convolution slot");
    assert_eq!(p.param(Rack::Insert, 0, FxParam::Wet), Some(0.25));
    assert_eq!(p.param(Rack::Insert, 0, FxParam::Dry), Some(0.5));
    let mut left = [1.0; 64];
    let mut right = left;
    p.process(&mut left, &mut right);
    assert!(left.iter().chain(&right).all(|v| (*v - 1.0).abs() < 1e-6));
    let mut gain = chain(vec![gainer(3.0)]);
    assert!(gain.replace_ir(old).is_err(), "a stale IR cannot replace another effect");
    let mut left = [1.0; 64];
    let mut right = left;
    gain.process(&mut left, &mut right);
    assert!(left.iter().chain(&right).all(|v| *v == 3.0));
}

#[test]
fn convolution_size_stretches_reflections_and_predelay_offsets_them() {
    let mut impulse = [0.; 32];
    impulse[8] = 1.;
    let fx = ProgramFx { insert: Chain { slots: vec![convolution(&impulse)] }, ..Default::default() };
    for (size, predelay, peak) in [(0., 0., 4), (0.5, 0., 8), (1., 0., 12), (0.5, 0.25, 248)] {
        let settings = params::IrSettings { values: [predelay, size, size], size, ..params::IrSettings::DEFAULT };
        let loads = [ScriptIr { rack: Rack::Insert, slot: 0, load: Load::Convolution(settings) }];
        let mut p = fx.processor_with(SR, 64, &loads);
        assert_eq!(p.ir_settings(Rack::Insert, 0), Some(params::IrSettings { reverse: Some(false), auto_gain: Some(false), ..settings }));
        let mut output = Vec::new();
        for block in 0..8 {
            let mut left = [0.; 64];
            if block == 0 { left[0] = 1.; }
            let mut right = left;
            p.process(&mut left, &mut right);
            assert!(left.iter().all(|v| v.is_finite()));
            output.extend(left);
        }
        let found = output.iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0;
        assert_eq!(found, peak, "size={size}, predelay={predelay}");
        assert!((output[peak] - 1.).abs() < 1e-5);
    }
    // These points come from Una Corda's authored millisecond lookup table.
    for (raw, ms) in [(81055., 1.), (250000., 5.), (606445., 40.), (783203., 100.)] {
        assert!((params::IrSettings::predelay_ms(raw / 1e6) - ms).abs() < 0.25);
    }
    // Requested split values survive rate rebuilds even though the current
    // renderer uses the last edited size uniformly without an ER/LR boundary.
    let settings = params::IrSettings { values: [0.25, 0.5, 1.], size: 1., ..params::IrSettings::DEFAULT };
    let early_last = params::IrSettings { size: 0.5, ..settings };
    let mut replayed = early_last;
    for (n, value) in settings.values.into_iter().enumerate() { assert!(replayed.set(n as u8, value)); }
    assert_eq!(replayed, early_last, "unchanged parameter replay preserves the last edited size");
    let loads = [ScriptIr { rack: Rack::Insert, slot: 0, load: Load::Convolution(settings) }];
    let p = fx.processor_with(44100., 64, &loads);
    assert_eq!(p.ir_settings(Rack::Insert, 0), Some(params::IrSettings { reverse: Some(false), auto_gain: Some(false), ..settings }));
    for (n, want) in settings.values.iter().enumerate() {
        assert_eq!(p.param(Rack::Insert, 0, FxParam::Convolution(n as u8)), Some(*want));
        assert_eq!(fx.param(Rack::Insert, 0, FxParam::Convolution(n as u8)), Some(params::IrSettings::DEFAULT.values[n]));
    }
}

/// A processor running `slots` as the insert rack, in 64-frame blocks.
fn chain(slots: Vec<Effect>) -> FxProcessor {
    ProgramFx {
        insert: Chain { slots },
        ..Default::default()
    }
    .processor(SR, 64)
}

fn ramp(n: usize) -> (Vec<f32>, Vec<f32>) {
    let l: Vec<f32> = (0..n).map(|i| (i as f32 * 0.37).sin()).collect();
    let r = l.iter().map(|v| -0.5 * v).collect();
    (l, r)
}

#[test]
fn bypass_and_unimplemented_are_identity() {
    let mut bypassed = gainer(2.0);
    bypassed.bypass = true;
    let unknown = effect(Kind::Twang, Params::Opaque { bytes: 3 });
    let mut c = chain(vec![bypassed, unknown]);
    let (mut l, mut r) = ramp(200);
    let (l0, r0) = (l.clone(), r.clone());
    c.process(&mut l, &mut r);
    assert_eq!((l, r), (l0, r0));
}

#[test]
fn gain_and_dry_wet_mix() {
    let mut conv = convolution(&[0.5]);
    conv.output_gain = 0.5;
    conv.dry_level = 1.0;
    let mut c = chain(vec![gainer(2.0), conv]);
    // Longer than max_block: the chain must split it.
    let (mut l, mut r) = ramp(300);
    let (l0, r0) = (l.clone(), r.clone());
    c.process(&mut l, &mut r);
    for (y, x) in l.iter().chain(&r).zip(l0.iter().chain(&r0)) {
        let want = 2.0 * x * (1.0 + 0.25);
        assert!((y - want).abs() < 1e-5, "{y} vs {want}");
    }
}

#[test]
fn send_levels_feed_parallel_send_slots() {
    let mut levels = effect(
        Kind::SendLevels,
        Params::SendLevels(params::SendLevels {
            sends: vec![0.0, 0.5, 1.0],
            outputs: Vec::new(),
        }),
    );
    levels.slot = 7;
    let mut unfed = gainer(1.0);
    unfed.slot = 0;
    let mut fed = gainer(3.0);
    fed.slot = 1;
    // Fed but bypassed: returns nothing.
    let mut off = gainer(1.0);
    off.slot = 2;
    off.bypass = true;
    let mut fx = ProgramFx {
        insert: Chain {
            slots: vec![levels],
        },
        send: Chain {
            slots: vec![unfed, fed, off],
        },
        main: Chain {
            slots: vec![gainer(0.5)],
        },
        ..Default::default()
    }
    .processor(SR, 32);
    let (mut l, mut r) = ramp(100);
    let (l0, r0) = (l.clone(), r.clone());
    fx.process(&mut l, &mut r);
    // (dry + 0.5 * 3 * dry) * main 0.5
    for (y, x) in l.iter().chain(&r).zip(l0.iter().chain(&r0)) {
        assert!((y - x * 2.5 * 0.5).abs() < 1e-5);
    }
}

#[test]
fn stereo_modeller_default_is_identity_and_mono_collapses() {
    let stereo = |spread| {
        effect(
            Kind::StereoModeller,
            Params::StereoModeller(params::StereoModeller {
                spread,
                pan: 0.0,
                pseudo_stereo: false,
            }),
        )
    };
    let (mut l, mut r) = ramp(64);
    let (l0, r0) = (l.clone(), r.clone());
    chain(vec![stereo(0.0)]).process(&mut l, &mut r);
    for (y, x) in l.iter().chain(&r).zip(l0.iter().chain(&r0)) {
        assert!((y - x).abs() < 1e-6);
    }
    chain(vec![stereo(-1.0)]).process(&mut l, &mut r);
    assert_eq!(l, r);
}

#[test]
fn reverb_effect_is_finite_with_hostile_input() {
    let mut rv = effect(
        Kind::Reverb,
        params::parse(Kind::Reverb, &[1.0f32; 10].map(f32::to_le_bytes).concat()),
    );
    rv.dry_level = 1.0;
    let mut c = chain(vec![rv]);
    let mut l: Vec<f32> = (0..4096)
        .map(|i| if i % 7 == 0 { 1.0 } else { -1.0 })
        .collect();
    let mut r = l.clone();
    for _ in 0..50 {
        c.process(&mut l, &mut r);
        assert!(l.iter().chain(&r).all(|v| v.is_finite()));
    }
}

#[test]
fn tail_rings_through_silence_then_idles() {
    let mut ir = vec![0.0; 300];
    ir[250] = 1.0;
    let mut c = chain(vec![convolution(&ir)]);
    let (mut l, mut r) = (vec![1e-7; 64], vec![1e-7; 64]);
    c.process(&mut l, &mut r);
    assert_eq!(l[0], 1e-7, "a fresh chain has nothing ringing, so it sleeps");
    let mut block = |first: f32| {
        let mut l = vec![0.0; 64];
        l[0] = first;
        let mut r = l.clone();
        c.process(&mut l, &mut r);
        l
    };
    let out: Vec<f32> = (0..8)
        .flat_map(|i| block(if i == 0 { 1.0 } else { 0.0 }))
        .collect();
    assert!(
        (out[250] - 1.0).abs() < 1e-5,
        "the echo sounds after the input fell silent"
    );
    for _ in 0..3 {
        block(0.0);
    }
    // Silent longer than the IR: below-threshold input passes untouched.
    assert_eq!(block(1e-7)[0], 1e-7);
}

/// A tail sleeps once it is below −120 dBFS, which a quiet input reaches
/// long before the IR ends: its chain costs nothing sooner, and what it
/// skips is inaudible.
#[test]
fn quiet_tails_sleep_sooner() {
    // A second of −60 dB/s decay.
    let ir: Vec<f32> = (0..48_000).map(|k| 0.001f32.powf(k as f32 / SR) * 1e-5).collect();
    // Frames until the chain sleeps (and passes its silent input through).
    let rings = |click: f32| {
        let mut c = chain(vec![convolution(&ir)]);
        let mut l = vec![0.0; ir.len() + 64];
        l[0] = click;
        let mut r = l.clone();
        for (l, r) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
            c.process(l, r);
        }
        l.iter().rposition(|&x| x != 0.0).map_or(0, |i| i + 1)
    };
    let (loud, quiet) = (rings(1.0), rings(1e-5));
    assert!(loud >= ir.len() && quiet * 3 < loud, "quiet {quiet} frames, loud {loud}");
    let rest = ir[quiet..].iter().map(|x| x.abs()).sum::<f32>() * 1e-5;
    assert!(rest < 1e-6, "the skipped tail could reach {rest}");
}

fn chunk(id: u16, data: Vec<u8>) -> Vec<u8> {
    let mut out = id.to_le_bytes().to_vec();
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    out
}

fn structured(version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Vec<u8> {
    let mut out = vec![1];
    out.extend(version.to_le_bytes());
    for part in [private, public, children] {
        out.extend((part.len() as u32).to_le_bytes());
        out.extend(part);
    }
    out
}

fn slot(effect_type: u32, bypass: bool, output: f32, dry: f32, id: u16, public: &[u8]) -> Vec<u8> {
    let mut private = effect_type.to_le_bytes().to_vec();
    private.extend([0, 0, 0, 0, 0, bypass as u8]);
    private.extend(output.to_le_bytes());
    private.extend(dry.to_le_bytes());
    private.extend((-1i32).to_le_bytes());
    let fx = chunk(id, structured(0x10, &[], public, &[]));
    chunk(0x25, structured(0x50, &private, &[], &fx))
}

fn rack(slots: &[(usize, Vec<u8>)]) -> Vec<u8> {
    let mut data = vec![0, 0x12, 0];
    for i in 0..8 {
        match slots.iter().find(|(s, _)| *s == i) {
            Some((_, bytes)) => {
                data.push(1);
                data.extend(bytes);
            }
            None => data.push(0),
        }
    }
    chunk(0x3a, data)
}

#[test]
fn reads_racks_slots_and_buses_from_program_bytes() {
    let gain = slot(20, false, 1.0, 0.0, 0x13, &2f32.to_le_bytes());
    let reverb = slot(
        39,
        true,
        1.0,
        0.0,
        0x59,
        &[0.5f32; 10].map(f32::to_le_bytes).concat(),
    );
    let mut bus_public = 2u32.to_le_bytes().to_vec();
    bus_public.extend([b'B', 0, b'1', 0]);
    bus_public.extend(1f32.to_le_bytes());
    bus_public.extend(0.25f32.to_le_bytes());
    bus_public.extend((-1i32).to_le_bytes());
    let bus = chunk(0x45, structured(0x11, &[], &bus_public, &rack(&[])));

    let mut children = rack(&[(3, gain)]);
    children.extend(rack(&[(0, reverb)]));
    children.extend(bus);
    let children = {
        let mut r = std::io::Cursor::new(children);
        let mut out = Vec::new();
        while (r.position() as usize) < r.get_ref().len() {
            out.push(Chunk::read(&mut r).unwrap());
        }
        out
    };
    let program = Program(StructuredObject {
        version: 0xa5,
        public_data: Vec::new(),
        private_data: Vec::new(),
        children,
    });
    let fx = ProgramFx::read(&program).unwrap();
    assert_eq!(fx.insert.slots.len(), 1);
    assert_eq!(
        (fx.insert.slots[0].slot, fx.insert.slots[0].kind),
        (3, Kind::Gainer)
    );
    assert!(fx.send.slots[0].bypass);
    assert!(matches!(fx.send.slots[0].params, Params::Reverb(_)));
    assert!(fx.main.slots.is_empty());
    assert_eq!((fx.buses[0].name.as_str(), fx.buses[0].pan), ("B1", 0.25));
    assert!(fx.warnings().is_empty());
    let json = serde_json::to_value(&fx).unwrap();
    assert_eq!(json["insert"]["slots"][0]["params"]["gainer"]["gain"], 2.0);
}

/// CPU cost per effect. Run with
/// `cargo test --release --lib fx::tests::bench -- --ignored --nocapture`;
/// `FX_BENCH=<name part>` runs only matching effects (for `perf stat`).
#[test]
#[ignore = "benchmark"]
fn bench() {
    const BLOCK: usize = 128;
    const BLOCKS: usize = 20_000;
    let noise_ir = |seconds: f32| -> Vec<f32> {
        let mut seed = 1u32;
        (0..(seconds * SR) as usize)
            .map(|i| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((seed >> 8) as f32 / (1 << 24) as f32 - 0.5) * (-(i as f32) / SR * 3.0).exp()
            })
            .collect()
    };
    let reverb = || {
        effect(
            Kind::Reverb,
            params::parse(
                Kind::Reverb,
                &[0.0f32, 0.37, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 1.0]
                    .map(f32::to_le_bytes)
                    .concat(),
            ),
        )
    };
    let with_dry = |mut fx: Effect| {
        fx.output_gain = 0.03;
        fx.dry_level = 1.0;
        fx
    };
    let cases: Vec<(&str, Effect)> = vec![
        ("gainer", gainer(1.001)),
        (
            "stereo modeller",
            effect(
                Kind::StereoModeller,
                Params::StereoModeller(params::StereoModeller {
                    spread: -0.3,
                    pan: 0.1,
                    pseudo_stereo: false,
                }),
            ),
        ),
        ("reverb", reverb()),
        ("convolution 0.5 s", with_dry(convolution(&noise_ir(0.5)))),
        ("convolution 2 s", with_dry(convolution(&noise_ir(2.0)))),
        ("convolution 5 s", with_dry(convolution(&noise_ir(5.0)))),
    ];
    let only = std::env::var("FX_BENCH").unwrap_or_default();
    for (name, fx) in cases.into_iter().filter(|(name, _)| name.contains(only.as_str())) {
        let mut c = chain(vec![fx]);
        let (mut l, mut r) = ramp(BLOCK);
        let (mut worst, mut hash) = (0u128, 0u64);
        let start = std::time::Instant::now();
        for _ in 0..BLOCKS {
            let t = std::time::Instant::now();
            c.process(&mut l, &mut r);
            worst = worst.max(t.elapsed().as_nanos());
            hash = l.iter().chain(&r).fold(hash, |h, v| (h ^ u64::from(v.to_bits())).wrapping_mul(0x100_0000_01b3));
            l.iter_mut().for_each(|v| *v *= 0.5);
        }
        let mean = start.elapsed().as_nanos() / BLOCKS as u128;
        let budget = BLOCK as f64 / SR as f64 * 1e9;
        println!(
            "{name:>18}: {mean:>7} ns/block mean, {worst:>8} ns worst ({:.2}% of a 128-frame 48 kHz block) · output hash {hash:016x}",
            mean as f64 / budget * 100.0
        );
    }
}

#[test]
fn loaded_kinds_replace_fill_and_empty_slots() {
    let mut fx = ProgramFx::default();
    fx.insert.slots.push(gainer(0.5));
    let ty = |p: &FxProcessor, slot| p.param(Rack::Insert, slot, FxParam::Type);
    let p = fx.processor(SR, 64);
    // An empty slot reads `$EFFECT_TYPE_NONE`; no bus 3, no slot 8.
    assert_eq!((ty(&p, 0), ty(&p, 1), ty(&p, 8)), (Some(19.0), Some(0.0), None));
    assert_eq!(p.param(Rack::Bus(3), 0, FxParam::Type), None);
    let load = |slot, kind| ScriptIr { rack: Rack::Insert, slot, load: Load::Kind(kind) };
    let p = fx.processor_with(SR, 64, &[load(0, None), load(2, Some(Kind::Reverb)), load(3, Some(Kind::Delay))]);
    // Types without DSP still read back; the reverb runs at its defaults.
    assert_eq!((ty(&p, 0), ty(&p, 2), ty(&p, 3)), (Some(0.0), Some(89.0), Some(16.0)));
    assert_eq!(p.param(Rack::Insert, 2, FxParam::Reverb(1)), Some(params::Reverb::DEFAULT.time));
    let (mut l, mut r) = (vec![0.0; 64], vec![0.0; 64]);
    l[0] = 1.0;
    let mut p = p;
    p.process(&mut l, &mut r);
    assert!(l.iter().all(|v| v.is_finite()));
    // The stored description is untouched.
    assert_eq!(fx.insert.slots.len(), 1);
}
