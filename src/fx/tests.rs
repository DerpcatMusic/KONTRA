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
        b.extend([0, 1, 1, 1, 0]);
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
            sends: vec![0.0, 0.5],
            outputs: Vec::new(),
        }),
    );
    levels.slot = 7;
    let mut unfed = gainer(1.0);
    unfed.slot = 0;
    let mut fed = gainer(3.0);
    fed.slot = 1;
    let mut fx = ProgramFx {
        insert: Chain {
            slots: vec![levels],
        },
        send: Chain {
            slots: vec![unfed, fed],
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
