use super::*;

#[test]
fn live_volume_lfo_depth_keeps_native_units_lag_phase_and_ordered_restore_in_complete_pcm() {
    use crate::{audio::Sample, engine::{Bank, Engine, Expression, load_scripts, load_scripts_with_state},
        import::{Instrument, PitchLfo, VolumeLfo, Zone}, modulation::DigitalLfo};
    const FRAMES: usize = 4096;
    // Exact native Digital step1/16 at48k; this independent waveform does not
    // call Clock, View, the production converter or the production target.
    const POINTS: [f32; 16] = [-0.5, -0.25, 0., 0.25, 0.5, 0.75, 1., 0.75,
        0.5, 0.25, 0., -0.25, -0.5, -0.75, -1., -0.75];
    let source = PitchLfo { slot: 7, count: 93.75, note_value: -1., sine: 0.,
        start_phase: 0.125, fade_ms: 0., extended_fade: false, fade_sync: None,
        depth: 0., targets: vec![], bypassed: false, shared: false,
        digital: Some(DigitalLfo { rectangle: 0., triangle: 1., saw: 0.,
            normalize: false, pulse_width: 0.5 }) };
    let mut group = Group { volume_lfos: vec![VolumeLfo { source, target: 4,
        intensity: 0.125, negative: true, lag_ms: 15 }], ..Default::default() };
    group.modulators.resize_with(8, || Modulator { name: String::new(), targets: vec![],
        assignments: None, volume_env: false, bypassed: false, flex: false,
        primary_flex: false, envelope: None, kind: "unsupported".into() });
    group.modulators[7].kind = "lfo".into();
    group.modulators[7].name = "Digital".into();
    group.modulators[7].targets = vec!["Cutoff".into(), "Other".into(), "Pitch".into(),
        "Q".into(), "Volume".into()];
    // The retained native source/target identity survives the existing wire cache.
    let group: Group = serde_json::from_slice(&serde_json::to_vec(&group).unwrap()).unwrap();
    let par = |id, target| EnginePar { id, group: 0, slot: 7, generic: target };
    let signed = Address::resolve(par(id::MOD_TARGET_MP_INTENSITY, 4), &[group.clone()]).unwrap();
    let unsigned = Address::resolve(par(id::MOD_TARGET_INTENSITY, 4), &[group.clone()]).unwrap();
    for target in [0, 1, 2, 3, 5] {
        assert!(Address::resolve(par(id::MOD_TARGET_MP_INTENSITY, target), &[group.clone()]).is_none());
    }
    let script = "on init
        declare $seed := get_engine_par($ENGINE_PAR_MOD_TARGET_MP_INTENSITY,0,7,4)
        make_persistent($seed)
        end on
        on controller
        select ($CC_NUM)
        case 1
            set_engine_par($ENGINE_PAR_MOD_TARGET_MP_INTENSITY,250000,0,7,4)
        case 2
            set_engine_par($ENGINE_PAR_MOD_TARGET_INTENSITY,500000,0,7,4)
        case 3
            set_engine_par($ENGINE_PAR_INTMOD_INTENSITY,750000,0,7,4)
        case 4
            set_engine_par($ENGINE_PAR_MOD_TARGET_INTENSITY,750000,0,7,4)
        case 5
            set_engine_par($ENGINE_PAR_INTMOD_BYPASS,%CC[5],0,7,-1)
        end select
        end on";
    let instrument = Instrument { groups: vec![group.clone()], scripts: vec![script.into()],
        ..Default::default() };
    let create = |mut g: Group, dry: bool| {
        if dry { g.volume_lfos.clear(); }
        let mut e = Engine::default(); e.reset(48_000.); e.attack = 0.; e.release = 0.001;
        e.set_transport(false, 120., 0., (4, 4));
        e.set_bank(Some(Box::new(Bank::from_samples(vec![g], vec![Zone::default()],
            vec![(std::path::PathBuf::new(), Sample { rate: 48_000,
                frames: (0..8192).map(|n| [((n * 37 % 191) as f32 - 95.) / 700.,
                    ((n * 53 % 211) as f32 - 105.) / 800.]).collect() })]).unwrap())));
        e
    };
    let (mut full, mut split, mut dry) = (create(group.clone(), false),
        create(group.clone(), false), create(group.clone(), true));
    for e in [&mut full, &mut split] {
        let (rt, errors) = load_scripts(&instrument, vec![], 48_000.);
        assert!(errors.is_empty(), "{errors:?}"); e.set_script(rt);
    }
    let mut snapshot = full.script().unwrap().native_state.snapshot();
    let (mut a, mut ar, mut b, mut br, mut c, mut cr) = ([0.; FRAMES], [0.; FRAMES],
        [0.; FRAMES], [0.; FRAMES], [0.; FRAMES], [0.; FRAMES]);
    let events = [(137, 1, 127), (229, 2, 127), (383, 3, 127), (521, 4, 127),
        (617, 1, 127), (733, 5, 1), (811, 2, 127), (941, 5, 0), (1003, 3, 127),
        (1477, 1, 127), (1541, 2, 127)];
    let boundaries = [0, 137, 229, 383, 521, 617, 733, 811, 941, 1003, 1107,
        1439, 1477, 1541, 2048, FRAMES];
    assert_eq!(crate::plugin::tests::allocations(|| {
        for e in [&mut full, &mut split, &mut dry] { e.note_on(0, 60, 127); }
        for range in boundaries.windows(2) {
            let at = range[0]; let end = range[1];
            for &(time, cc, value) in &events {
                if time == at { full.cc(0, cc, value); split.cc(0, cc, value); }
            }
            if at == 1107 || at == 1439 {
                for e in [&mut full, &mut split, &mut dry] { e.set_expression_on(0, 60, Expression { gain: if at == 1107 { 0. } else { 1. }, ..Default::default() }); }
            }
            if at == 2048 {
                for e in [&mut full, &mut split, &mut dry] { e.note_off(0, 60); }
            }
            let mut pos = at;
            while pos < end {
                let next = (pos + 128).min(end);
                full.render(&mut a[pos..next], &mut ar[pos..next]);
                dry.render(&mut c[pos..next], &mut cr[pos..next]);
                pos = next;
            }
            let mut pos = at; let mut part = 0;
            while pos < end {
                let next = (pos + [1, 17, 31, 83, 127][part % 5]).min(end);
                split.render(&mut b[pos..next], &mut br[pos..next]);
                pos = next; part += 1;
            }
        }
        while !full.script().unwrap().native_state.refresh(&mut snapshot, 1) {}
    }), 0);
    let alpha = (1. - (-(2. * std::f64::consts::LN_10) / 22.5).exp()) as f32;
    let (mut depth, mut negative, mut bypass, mut source_point) = (0.125, true, false, 0);
    let (mut retained, mut previous, mut current) = (0_f32, 1_f32, 1_f32);
    for frame in 0..FRAMES {
        for &(at, cc, _) in &events {
            if at == frame { match cc {
                1 => { depth = 0.125; negative = true; }
                2 => depth = 0.25,
                3 => { depth = 0.125; negative = false; }
                4 => depth = 0.5625,
                5 => bypass = at == 733,
                _ => unreachable!(),
            } }
        }
        if !bypass && frame % 32 == 0 {
            let wave = POINTS[source_point % 16]; source_point += 1;
            let endpoint = (if negative { -wave } else { wave } + 1.) * 0.5;
            if frame == 0 { retained = endpoint; }
            else {
                let delta = endpoint - retained;
                if delta.abs() >= 1e-5 { retained += delta * alpha; }
            }
            previous = current;
            current = 1. - (1. - retained) * depth;
            if frame == 0 { previous = current; }
        }
        let gain = if bypass { 1. } else {
            previous + (current - previous) * ((frame % 32) as f32 / 32.)
        };
        for (actual, expected) in [(a[frame], c[frame] * gain), (ar[frame], cr[frame] * gain)] {
            assert!((actual - expected).abs() < 2e-7,
                "independent volume depth/lag/frame={frame}: {actual} vs {expected}");
        }
        assert!((a[frame] - b[frame]).abs() < 2e-7);
        assert!((ar[frame] - br[frame]).abs() < 2e-7);
    }
    assert!(a[160..700].iter().zip(&c[160..700]).any(|(a, c)| (a - c).abs() > 0.003));
    assert!(a[1347..1439].iter().chain(&ar[1347..1439]).all(|v| *v == 0.));
    assert!(a[2304..].iter().chain(&ar[2304..]).all(|v| *v == 0.));
    assert_eq!((full.active_voices(), split.active_voices(), dry.active_voices()), (0, 0, 0));
    assert_eq!(read(full.settings().unwrap(), signed), Some(-0.25));
    assert_eq!(read(full.settings().unwrap(), unsigned), Some(0.25));
    assert_eq!(signed.encode(-0.125), 250000);
    assert_eq!(unsigned.encode(0.25), 500000);
    assert_eq!(full.dropped_commands(), 0);
    for id in [id::INTMOD_INTENSITY, id::MOD_TARGET_MP_INTENSITY, id::MOD_TARGET_INTENSITY] {
        assert!(full.script().unwrap().env.engine_par(par(id, 4)).is_none(),
            "accepted intensity must not use the unsupported stored-value fallback");
    }
    let edits = snapshot.saved();
    // Last signed write sets negative, then unsigned changes only magnitude.
    // Both must replay in order; collapsing aliases into one loses the sign.
    assert_eq!(edits.iter().filter(|e| e.par.generic == 4).count(), 2);
    let (rt, errors) = load_scripts_with_state(&instrument, vec![], 48_000., &[], &edits);
    assert!(errors.is_empty(), "{errors:?}");
    let mut restored = create(group, false); restored.set_script(rt);
    assert_eq!(read(restored.settings().unwrap(), signed), Some(-0.25));
    assert_eq!(read(restored.settings().unwrap(), unsigned), Some(0.25));
    let crate::ksp::Value::Int(seed) = restored.script().unwrap().persistence()[0]["$seed"] else { panic!("missing init readback"); };
    assert!((seed - 185020).abs() <= 1);
}
