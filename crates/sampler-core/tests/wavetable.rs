use sampler_core::{
    Envelope, Input, Limits, Pcm, Playback, Prepared, Protocol, Region, Runtime, Wavetable,
};
mod support;
const CYCLE: usize = 2048;
// Literal v1 0cb7a8a0:src/engine/wavetable.rs + voice.rs::hermite oracle.
fn oracle(data: &[[f32; 2]], source: Wavetable, mut phase: f64, step: f64, out: &mut [[f32; 2]]) {
    fn warp(phase: f32, amount: f32, kind: u8) -> f32 {
        if kind != 16 {
            return phase;
        }
        let offset = (amount.clamp(0., 1.) - 0.5) * 1.96;
        if f64::from(phase) < 0.5 + f64::from(offset * 0.5) {
            phase / (1. + offset)
        } else {
            (phase - 1.) / (1. - offset) + 1.
        }
    }
    let position = f64::from(source.position.clamp(0., 1.)) * (data.len() / CYCLE - 1) as f64;
    let lo = position as usize;
    let hi = (lo + 1).min(data.len() / CYCLE - 1);
    let blend = position.fract() as f32;
    let read = |first: usize, phase: f64| {
        let index = phase as usize;
        let q: [[f32; 2]; 4] =
            std::array::from_fn(|i| data[first + (index + CYCLE + i - 1) % CYCLE]);
        let t = phase.fract() as f32;
        std::array::from_fn::<_, 2, _>(|c| {
            let (xm1, x0, x1, x2) = (q[0][c], q[1][c], q[2][c], q[3][c]);
            let c1 = 0.5 * (x1 - xm1);
            let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
            let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
            ((c3 * t + c2) * t + c1) * t + x0
        })
    };
    for frame in out {
        let read_phase = warp(
            warp(
                (phase / CYCLE as f64) as f32,
                source.form1,
                source.form1_type,
            ),
            source.form2,
            source.form2_type,
        ) as f64
            * CYCLE as f64;
        let a = read(lo * CYCLE, read_phase);
        let b = if lo == hi || blend == 0. {
            a
        } else {
            read(hi * CYCLE, read_phase)
        };
        *frame = std::array::from_fn(|c| a[c] + (b[c] - a[c]) * blend);
        phase = (phase + step).rem_euclid(CYCLE as f64);
    }
}
fn runtime(
    data: &[[f32; 2]],
    source: Wavetable,
    sample_rate: u32,
    root: u8,
    key: u8,
    rate: u32,
) -> Runtime {
    let plan = Prepared::new(
        rate,
        vec![Pcm::new(sample_rate, data.to_vec().into_boxed_slice()).unwrap()],
        vec![Region {
            sample: 0,
            key_low: key,
            key_high: key,
            root_key: Some(root),
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::new(0, 0, 0, 1., 0).unwrap(),
            playback: Playback {
                wavetable: Some(source),
                ..Default::default()
            },
        }],
        1,
    )
    .unwrap();
    Runtime::new(
        plan,
        Limits {
            notes: 1,
            performances: 1,
            families: 1,
            expressions: 1,
            voices: 1,
            commands: 4,
            channels: 0,
            decisions: 0,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap()
}
fn input(key: u8) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: Some(1),
    }
}
#[test]
fn resident_cycle_morph_and_both_forms_match_literal_v1_without_heap_or_block_resets() {
    let data: Vec<_> = (0..3 * CYCLE)
        .map(|i| [(i as f32 * 0.0017).sin(), (i as f32 * 0.0023).cos()])
        .collect();
    for position in [0., 0.375, 1.] {
        for (form1_type, form1, form2_type, form2) in [
            (0, 0., 0, 0.),
            (16, 0., 0, 0.),
            (16, 0.5, 0, 0.),
            (16, 1., 16, 0.75),
        ] {
            let source = Wavetable {
                position,
                phase: 0.9998779,
                form1_type,
                form1,
                form2_type,
                form2,
            };
            let mut rt = runtime(&data, source, 22050, 69, 69, 48000);
            let mut actual = [[0.; 2]; 1024];
            let mut expected = actual;
            oracle(
                &data,
                source,
                f64::from(source.phase).rem_euclid(1.) * CYCLE as f64,
                440. * CYCLE as f64 / 48000.,
                &mut expected,
            );
            support::without_heap(|| {
                rt.trigger(input(69), 69, 1.).unwrap();
                let mut start = 0;
                for n in [1, 127, 3, 128, 17, 64, 256, 428] {
                    rt.render(&mut actual[start..start + n]).unwrap();
                    start += n;
                }
            });
            assert_eq!(
                actual, expected,
                "position{position} forms{form1_type}/{form2_type}"
            );
        }
    }
}
#[test]
fn oscillator_fundamental_uses_note_and_output_rate_and_sustains_past_all_cycles() {
    let data: Vec<_> = (0..CYCLE)
        .map(|i| [(std::f32::consts::TAU * i as f32 / CYCLE as f32).sin(); 2])
        .collect();
    for (key, hz) in [(57, 220.), (69, 440.), (81, 880.), (127, 12543.8539514)] {
        for rate in [44100, 48000] {
            let mut rt = runtime(&data, Wavetable::default(), 96000, 12, key, rate);
            let mut audio = vec![[0.; 2]; rate as usize];
            support::without_heap(|| {
                rt.trigger(input(key), key, 1.).unwrap();
                for block in audio.chunks_mut(64) {
                    rt.render(block).unwrap();
                }
            });
            let rises = audio
                .windows(2)
                .filter(|q| q[0][0] <= 0. && q[1][0] > 0.)
                .count();
            assert!(
                (rises as f64 - hz).abs() <= 1.,
                "note{key} rate{rate} frequency{rises}"
            );
            assert!(audio[audio.len() - 128..].iter().any(|f| f[0].abs() > 0.5));
        }
    }
}
#[test]
fn incomplete_cycles_and_unproved_forms_are_rejected_at_preparation() {
    let data = vec![[0.; 2]; CYCLE + 1];
    let region = Region {
        sample: 0,
        key_low: 69,
        key_high: 69,
        root_key: Some(69),
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback {
            wavetable: Some(Wavetable::default()),
            ..Default::default()
        },
    };
    assert!(
        Prepared::new(
            48000,
            vec![Pcm::new(48000, data.into_boxed_slice()).unwrap()],
            vec![region],
            1
        )
        .is_err()
    );
}

#[test]
fn ir_lowering_reaches_the_oscillator_with_tune_and_ignores_saved_root_and_loops() {
    use sampler_ir as ir;
    let data: Vec<_> = (0..CYCLE)
        .map(|i| [(std::f32::consts::TAU * i as f32 / CYCLE as f32).sin(); 2])
        .collect();
    let mut zone = ir::Zone::new(ir::AssetRef(0));
    zone.group = Some(ir::GroupRef(0));
    zone.pitch = ir::KeyTracking::Tracked { root: 12 };
    zone.tune = ir::Pitch::Semitones(12.);
    zone.playback.looping = ir::Looping::Continuous(ir::LoopRange {
        start: 3,
        end: 19,
        crossfade: ir::Span::Frames(0),
        alternating: false,
    });
    let instrument = ir::Instrument {
        assets: vec![ir::Asset {
            location: ir::AssetLocation::Path("synthetic.wav".into()),
            encoding: ir::Encoding::Wav,
            root_key: None,
            loops: vec![],
        }],
        groups: vec![ir::Group {
            wavetable: Some(Wavetable::default()),
            ..Default::default()
        }],
        zones: vec![zone],
        ..Default::default()
    };
    let plan = sampler_core::lower::lower(
        &instrument,
        48000,
        vec![Pcm::new(8000, data.into_boxed_slice()).unwrap()],
        |_, plan| Ok(plan),
    )
    .unwrap();
    let mut rt = Runtime::new(
        plan,
        Limits {
            notes: 1,
            channels: 0,
            performances: 1,
            families: 1,
            expressions: 1,
            voices: 1,
            decisions: 0,
            commands: 4,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap();
    let mut audio = [[0.; 2]; 4800];
    support::without_heap(|| {
        rt.trigger(input(69), 69, 1.).unwrap();
        for block in audio.chunks_mut(127) {
            rt.render(block).unwrap();
        }
    });
    let rises = audio
        .windows(2)
        .filter(|q| q[0][0] <= 0. && q[1][0] > 0.)
        .count();
    assert!(
        (rises as i32 - 88).abs() <= 1,
        "authored +12 semitones gives880Hz, crossings{rises}"
    );
}

fn addressed_wave_plan(data: &[[f32; 2]], source: Wavetable, chain: bool) -> Prepared {
    use sampler_ir as ir;
    let mut zone = ir::Zone::new(ir::AssetRef(0));
    zone.group = Some(ir::GroupRef(0));
    zone.velocity = ir::VelocityResponse::None;
    let mut instrument = ir::Instrument {
        assets: vec![ir::Asset { location: ir::AssetLocation::Path("synthetic.wav".into()),
            encoding: ir::Encoding::Wav, root_key: None, loops: vec![] }],
        groups: vec![ir::Group { wavetable: Some(source), ..Default::default() }],
        zones: vec![zone], ..Default::default()
    };
    instrument.source_indices.groups = vec![None; 8];
    instrument.source_indices.groups[7] = Some(ir::GroupRef(0));
    let plan = sampler_core::lower::lower(&instrument, 48000,
        vec![Pcm::new(48000, data.to_vec().into_boxed_slice()).unwrap()], |_, p| Ok(p)).unwrap();
    if chain {
        plan.with_voice_chains(vec![sampler_core::VoiceChain::new(
            vec![sampler_core::Processor::Gain(1.)], vec![], 0).unwrap()], vec![Some(0)]).unwrap()
    } else { plan }
}
fn wave_address(name: &str, group: i32) -> sampler_core::EngineParameterAddress {
    sampler_core::EngineParameterAddress { parameter: sampler_core::engine_parameter_id(name).unwrap(),
        group, slot: -1, generic: -1 }
}
#[test]
fn live_wave_writes_reach_active_voices_in_single_batch_and_parallel_paths_without_resetting_phase() {
    use sampler_core::Threads;
    let data: Vec<_> = (0..2*CYCLE).map(|i| [
        (std::f32::consts::TAU * (i % CYCLE) as f32 / CYCLE as f32).sin()
            * if i < CYCLE { 1. } else { 0.25 }; 2]).collect();
    for threads in [1, 2] {
        for chain in [false, true] {
            for (name, value) in [("ENGINE_PAR_WT_POSITION", 730_000),
                ("ENGINE_PAR_WT_FORM", 230_000), ("ENGINE_PAR_WT_FORM2", 870_000)] {
                let mut source = Wavetable { phase: 0.17, form1_type: 16, form1: 0.5,
                    form2_type: 16, form2: 0.5, ..Default::default() };
                let plan = addressed_wave_plan(&data, source, chain);
                let address = wave_address(name, 7);
                let mut rt = Runtime::new(plan, Limits { notes: 64, performances: 1,
                    families: 64, expressions: 64, voices: 64, commands: 4, channels: 0,
                    decisions: 0, behaviors: 0, behavior_fuel: 0, behavior_cells: 0, note_cells: 0 }).unwrap()
                    .with_threads(Threads::Fixed(threads));
                let mut first = [[0.; 2]; 64];
                support::without_heap(|| {
                    for id in 0..64 {
                        let mut note = input(69); note.external_id = Some(id);
                        rt.trigger(note, 69, 1.).unwrap();
                    }
                    rt.render(&mut first).unwrap();
                    assert!(rt.set_engine_parameter(wave_address(name, 0), value).is_err(), "dense address must not alias physical7");
                    rt.set_engine_parameter(address, value).unwrap();
                    assert_eq!(rt.engine_parameter(address).unwrap(), value);
                });
                match name { "ENGINE_PAR_WT_POSITION" => source.position = 0.73,
                    "ENGINE_PAR_WT_FORM" => source.form1 = 0.23, _ => source.form2 = 0.87 }
                let step = 440. * CYCLE as f64 / 48000.;
                let mut phase = f64::from(source.phase) * CYCLE as f64;
                for _ in 0..64 { phase = (phase + step).rem_euclid(CYCLE as f64); }
                let mut expected = [[0.; 2]; 64];
                oracle(&data, source, phase, step, &mut expected);
                for frame in &mut expected { for value in frame { let single = *value; *value = 0.;
                    for _ in 0..64 { *value += single; } } }
                let mut actual = [[0.; 2]; 64];
                support::without_heap(|| rt.render(&mut actual).unwrap());
                assert_eq!(actual, expected, "{name} chain{chain} threads{threads}");
                if threads == 2 { assert!(rt.parallel_blocks() > 0); }
            }
        }
    }
}

#[test]
fn wave_control_schema_cannot_replace_amounts_with_integer_values() {
    let data = vec![[0.; 2]; CYCLE];
    let plan = addressed_wave_plan(&data, Wavetable::default(), false);
    assert_eq!(plan.engine_parameter_bindings().len(), 3);
    let mut controls = plan.controls().to_vec();
    controls[0].domain = sampler_core::ControlDomain::Integer { min: 0, max: 1 };
    controls[0].default = sampler_core::ControlValue::Integer(0);
    assert!(plan.with_controls(controls).is_err());
}
