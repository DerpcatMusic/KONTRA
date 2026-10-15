use sampler_core::*;
mod support;

fn prepared(program: ModProgram, start_range: u32, reverse: bool) -> Prepared {
    Prepared::new(
        48000,
        vec![Pcm::new(48000, (0..8192).map(|i| [i as f32 / 8192.; 2]).collect()).unwrap()],
        vec![Region {
            sample: 0, key_low: 60, key_high: 60, root_key: None,
            velocity_low: 0., velocity_high: 1., gain: 1., envelope: Envelope::default(),
            playback: Playback { start: 137, end: Some(7000),
                direction: if reverse { Direction::Reverse } else { Direction::Forward },
                ..Playback::default() },
        }], 1,
    ).unwrap().with_velocity_curves(vec![VelocityCurve::Constant]).unwrap()
        .with_voice_modulation(vec![program], vec![Some(0)], vec![start_range]).unwrap()
}

fn program(sources: Vec<ModSource>, routes: Vec<ModRoute>) -> ModProgram {
    ModProgram { sources, routes, ..ModProgram::default() }
}

fn input() -> Input {
    Input { protocol: Protocol::Clap, port: 0, group: 0, channel: 0, key: 60, external_id: Some(1) }
}

#[test]
fn constant_hint_matches_actual_start_and_pcm_under_host_partitions() {
    let shape = vec![(0., 0.25), (1., 0.75)];
    let cases = [
        (vec![ModRoute::new(0, ModTarget::SampleStart, 0.125)], vec![], 125),
        (vec![ModRoute { invert: true, ..ModRoute::new(0, ModTarget::SampleStart, 0.75) }], vec![], 0),
        (vec![ModRoute { shape: Some(0), ..ModRoute::new(0, ModTarget::SampleStart, 0.5) }], vec![shape.clone()], 375),
        (vec![ModRoute::new(0, ModTarget::SampleStart, -0.25)], vec![], 0),
        (vec![ModRoute::new(0, ModTarget::SampleStart, 0.75), ModRoute::new(0, ModTarget::SampleStart, 0.5)], vec![], 1000),
        (vec![ModRoute { scale: Some(ModScale { source: 1, shape: Some(0), law: ModScaleLaw::Multiply, lag: 100 }),
            more_scales: std::array::from_fn(|i| (i == 0).then_some(ModScale { source: 1, shape: Some(0), law: ModScaleLaw::Multiply, lag: 700 })),
            lag: 100, ..ModRoute::new(0, ModTarget::SampleStart, 0.5) }], vec![shape], 281),
    ];
    for (routes, shapes, expected) in cases {
        for reverse in [false, true] {
            let make = || prepared(ModProgram { shapes: shapes.clone(),
                ..program(vec![ModSource::Constant; 2], routes.clone()) }, 1000, reverse);
            let p = make();
            support::without_heap(|| {
                assert_eq!(p.constant_start_offset(0), Some(expected));
                assert_eq!(p.constant_start_offset(1), None);
            });
            let position = if reverse { 6999 - expected } else { 137 + expected };
            let mut rt = Runtime::new(p, Limits::for_plan(&make(), 4, 4)).unwrap();
            rt.record_selections(true);
            rt.trigger(input(), 60, 0.25).unwrap();
            let records = rt.take_selection_records();
            assert_eq!(records[0].candidates[0].started.unwrap().frame, u64::from(position));
            for block in [1, 7, 64, 137] {
                let p = make();
                let mut rt = Runtime::new(p, Limits::for_plan(&make(), 4, 4)).unwrap();
                let mut audio = [[0.; 2]; 160];
                support::without_heap(|| {
                    rt.trigger(input(), 60, 0.75).unwrap();
                    for chunk in audio.chunks_mut(block) { rt.render(chunk).unwrap(); }
                });
                for (i, frame) in audio.iter().enumerate() {
                    let at = if reverse { position as usize - i } else { position as usize + i };
                    assert_eq!(*frame, [at as f32 / 8192.; 2]);
                }
            }
        }
    }
}

#[test]
fn any_dependent_start_route_or_scale_is_unknown_even_at_zero_depth() {
    let lfo = Lfo { shape: LfoShape::Zero, rate: LfoRate::Hertz(1.), phase: 0., delay: 0,
        fade: 0, retrigger: true, shared: false };
    for source in [ModSource::Velocity, ModSource::Key, ModSource::Controller(1), ModSource::Random,
        ModSource::Pressure, ModSource::Timbre, ModSource::PitchBend, ModSource::Script(0),
        ModSource::ReleaseCounter { frames: 100 }, ModSource::Envelope(Envelope::default()), ModSource::Lfo(lfo)] {
        for depth in [0., 0.5] {
            let p = prepared(program(vec![ModSource::Constant, source], vec![
                ModRoute::new(0, ModTarget::SampleStart, 0.25),
                ModRoute::new(1, ModTarget::SampleStart, depth),
            ]), 1000, false);
            assert_eq!(p.constant_start_offset(0), None);
            let p = prepared(program(vec![ModSource::Constant, source], vec![ModRoute {
                scale: Some(ModScale { source: 1, shape: None, law: ModScaleLaw::Multiply, lag: 0 }),
                ..ModRoute::new(0, ModTarget::SampleStart, depth)
            }]), 1000, false);
            assert_eq!(p.constant_start_offset(0), None);
        }
        let p = prepared(program(vec![ModSource::Constant, source], vec![
            ModRoute::new(0, ModTarget::SampleStart, 0.125), ModRoute::new(1, ModTarget::Pan, 0.5),
        ]), 1000, false);
        assert_eq!(p.constant_start_offset(0), Some(125), "unrelated routes do not affect the proof");
    }
}

#[test]
fn hint_keeps_current_native_metadata_law_and_fractional_truncation() {
    for flags in [0, 2, 4, 0x10, 0x14] {
        let mut p = program(vec![ModSource::Constant], vec![ModRoute {
            native: Some(ModNative { target: 5, flags, source_type: 0 }),
            ..ModRoute::new(0, ModTarget::SampleStart, 0.0041315858)
        }]);
        // This proves query/runtime equality, not Kontakt calibration of native flags.
        p.routes[0].unipolar = true;
        let p = prepared(p, 180479, false);
        assert_eq!(p.constant_start_offset(0), Some(745));
        let limits = Limits::for_plan(&p, 4, 4);
        let mut rt = Runtime::new(p, limits).unwrap();
        let mut audio = [[0.; 2]; 1];
        support::without_heap(|| { rt.trigger(input(), 60, 1.).unwrap(); rt.render(&mut audio).unwrap(); });
        assert_eq!(audio, [[(137 + 745) as f32 / 8192.; 2]]);
    }
    let p = prepared(program(vec![ModSource::Velocity], vec![ModRoute::new(0, ModTarget::Pan, 1.)]), 1000, false);
    assert_eq!(p.constant_start_offset(0), Some(0));
}
