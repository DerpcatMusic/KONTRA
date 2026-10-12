//! Native713 live units, paired source ownership and retained phase.
use super::*;

const FRAMES: usize = 6144;

fn oracle(sync: bool) -> (Vec<f32>, Vec<f32>) {
    let (mut tune, mut gain) = (vec![0.; FRAMES], vec![1.; FRAMES]);
    let mut phase = 0.125f64;
    let (mut previous, mut current, mut volume_previous, mut volume_current) = (0f32, 0f32, 1f32, 1f32);
    let mut lag = 0f32;
    let alpha = (1. - (-(2. * std::f64::consts::LN_10) / 22.5).exp()) as f32;
    for frame in 0..FRAMES {
        if (497..701).contains(&frame) { continue; }
        if frame % 32 == 0 {
            // Exterior triangle geometry; no Clock/View/parameter conversion.
            let wave = if phase < 0.5 { (4. * phase - 1.) as f32 }
                else { (3. - 4. * phase) as f32 };
            previous = current;
            current = wave * 1.5;
            let endpoint = (1. - wave) * 0.5;
            if frame == 0 { lag = endpoint; }
            else if (endpoint - lag).abs() >= 1e-5 { lag += (endpoint - lag) * alpha; }
            volume_previous = volume_current;
            volume_current = 1. - (1. - lag) * 0.125;
            if frame == 0 { previous = current; volume_previous = volume_current; }
            // Literal native instruction receipts, not production codec output.
            // The529 authored write prepares at raw120.0005 despite the global
            // BPM refresh threshold retaining120. The source is paused then.
            let bits = if sync {
                if frame < 137 { 0x3f48f75220000000 }
                else if frame < 311 { 0x3f75d867c0000000 }
                else if frame < 529 { 0x3f3d208a60000000 }
                else if frame < 1237 { 0x3f55d86dc0000000 }
                else { 0x3f48f758c0000000 }
            } else {
                if frame < 137 { 0x3f50484180000000 }
                else if frame < 311 { 0x3fc2302ba0000000 }
                else if frame < 529 { 0x3edd1fa8a0000000 }
                else if frame < 1237 { 0x3f883900a0000000 }
                else { 0x3f50484180000000 }
            };
            phase += f64::from_bits(bits);
            if phase > 1. { phase -= 1.; }
        }
        let fraction = (frame % 32) as f32 / 32.;
        tune[frame] = previous + (current - previous) * fraction;
        gain[frame] = volume_previous + (volume_current - volume_previous) * fraction;
    }
    (tune, gain)
}

#[test]
fn live_digital_frequency_uses_native_units_and_keeps_paired_phase_in_complete_pcm_without_heap() {
    use crate::{audio::Sample, engine::{Bank, Engine, HostNote, HostPattern, HostExpression,
        load_scripts, load_scripts_with_state}, import::{Instrument, PitchLfo, VolumeLfo, Zone},
        modulation::DigitalLfo};
    assert_eq!(crate::ksp::engine_par_name(id::INTMOD_FREQUENCY), Some("$ENGINE_PAR_INTMOD_FREQUENCY"));
    for sync in [false, true] {
        let source = PitchLfo { slot: 7, count: if sync { 7. } else { f32::from_bits(0x3fbecec0) },
            note_value: if sync { 0.25 } else { -1. }, sine: 0., start_phase: 0.125,
            fade_ms: 0., fade_sync: None, extended_fade: false, bypassed: false, shared: false,
            depth: 0.125, targets: vec![(2, 0.125)], digital: Some(DigitalLfo {
                rectangle: 0., triangle: 1., saw: 0., normalize: false, pulse_width: 0.5 }) };
        let mut group = Group { pitch_lfos: vec![source.clone()], volume_lfos: vec![VolumeLfo {
            source, target: 4, intensity: 0.125, negative: true, lag_ms: 15 }], ..Default::default() };
        group.modulators.resize_with(8, || Modulator { name: String::new(), targets: vec![],
            assignments: None, volume_env: false, bypassed: false, flex: false,
            primary_flex: false, envelope: None, kind: "unsupported".into() });
        group.modulators[7].kind = "lfo".into();
        group.modulators[7].name = "Digital".into();
        group.modulators[7].targets = vec!["Cutoff".into(), "Other".into(), "Pitch".into(), "Q".into(), "Volume".into()];
        // Existing Group wire/cache ownership retains both original source copies.
        let group: Group = serde_json::from_slice(&serde_json::to_vec(&group).unwrap()).unwrap();
        let par = |slot, generic| EnginePar { id: id::INTMOD_FREQUENCY, group: 0, slot, generic };
        let address = Address::resolve(par(7, -1), std::slice::from_ref(&group)).unwrap();
        for (slot, generic) in [(6, -1), (7, 0), (7, 2), (7, 4)] {
            assert!(Address::resolve(par(slot, generic), std::slice::from_ref(&group)).is_none());
        }
        for mode in [0, 1, 2] {
            let mut unsupported = group.clone();
            for s in unsupported.pitch_lfos.iter_mut().chain(unsupported.volume_lfos.iter_mut().map(|l| &mut l.source)) {
                if mode == 0 { s.shared = true; s.extended_fade = true; } else if mode == 1 { s.digital = None; }
            }
            if mode == 2 { unsupported.modulators[7].kind = "step".into(); }
            assert!(Address::resolve(par(7, -1), &[unsupported]).is_none());
        }
        if !sync {
            for (value, bits, getter) in [(0, 0x3c2aa580, 0), (100000, 0x3ce6d400, 101026),
                (250000, 0x3dff1880, 250149), (500000, 0x3fbecec0, 500072),
                (750000, 0x418dee00, 748822), (900000, 0x429d4b00, 898665),
                (1000000, 0x43552480, 1000000)] {
                assert_eq!(address.decode(value).to_bits(), bits);
                assert_eq!(address.encode(f32::from_bits(bits)), getter);
            }
        } else {
            assert_eq!(address.decode(0), 12.);
            assert_eq!(address.decode(500000), 7., "native roundf half away from zero");
            assert_eq!(address.decode(750000), 4.);
            assert_eq!(address.decode(1000000), 1.);
            for count in [-3.25, -0., 0., 0.25] { assert_eq!(address.encode(count), 1000000); }
        }
        let script = "on init
            declare $seed := get_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,0,7,-1)
            make_persistent($seed)
            declare $after := 0
            make_persistent($after)
            end on
            on controller
            select ($CC_NUM)
            case 1
                set_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,1000000,0,7,-1)
            case 2
                set_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,0,0,7,-1)
            case 3
                set_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,750000,0,7,-1)
            case 4
                set_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,500000,0,7,-1)
            case 5
                set_engine_par($ENGINE_PAR_INTMOD_BYPASS,%CC[5],0,7,-1)
            case 6
                set_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,-1,0,7,-1)
            case 7
                set_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,1000001,0,7,-1)
            end select
            $after := get_engine_par($ENGINE_PAR_INTMOD_FREQUENCY,0,7,-1)
            end on";
        let instrument = Instrument { groups: vec![group.clone()], scripts: vec![script.into()], ..Default::default() };
        let create = |mut group: Group, dry: bool| {
            if dry { group.pitch_lfos.clear(); group.volume_lfos.clear(); }
            let mut e = Engine::default(); e.reset(48_000.); e.attack = 0.; e.release = 0.001;
            e.set_transport(false, 120., 0., (4, 4));
            e.set_bank(Some(Box::new(Bank::from_samples(vec![group], vec![Zone::default()],
                vec![(std::path::PathBuf::new(), Sample { rate: 48_000,
                    frames: (0..16384).map(|n| [((n * 37 % 191) as f32 - 95.) / 700.,
                        ((n * 53 % 211) as f32 - 105.) / 800.]).collect() })]).unwrap())));
            e
        };
        let (mut actual, mut split, mut dry) = (create(group.clone(), false), create(group.clone(), false), create(group.clone(), true));
        for e in [&mut actual, &mut split] {
            let (rt, errors) = load_scripts(&instrument, vec![], 48_000.);
            assert!(errors.is_empty(), "{errors:?}"); e.set_script(rt);
        }
        let root = HostNote { port: 0, channel: 0, key: 60, id: 73, clap: true };
        let pattern = HostPattern { port: 0, channel: 0, key: 60, id: 73, clap: true };
        let (tune, gain) = oracle(sync);
        let (mut pcm, mut fragmented, mut reference) = ([vec![0.; FRAMES], vec![0.; FRAMES]],
            [vec![0.; FRAMES], vec![0.; FRAMES]], [vec![0.; FRAMES], vec![0.; FRAMES]]);
        let events = [(137, 1, 127), (311, 2, 127), (497, 5, 1), (529, 3, 127), (701, 5, 0),
            (1237, 4, 127), (1537, 6, 127), (1693, 7, 127)];
        let boundaries = [0, 137, 311, 497, 529, 701, 811, 1137, 1237, 1537, 1693, 4096, FRAMES];
        let mut snapshot = actual.script().unwrap().native_state.snapshot();
        let mut persistent = actual.script().unwrap().persistence();
        assert_eq!(crate::plugin::tests::allocations(|| {
            for e in [&mut actual, &mut split, &mut dry] { assert!(e.host_note_on(root, 0, 60, 127, 0.)); }
            for range in boundaries.windows(2) {
                let (at, end) = (range[0], range[1]);
                if at == 529 { for e in [&mut actual, &mut split, &mut dry] { e.set_transport(true, 120.0005, 0., (4, 4)); } }
                if at == 1537 || at == 1693 {
                    snapshot.rewind();
                    while !actual.script().unwrap().native_state.refresh(&mut snapshot, 1) {}
                    snapshot.rewind();
                }
                for &(time, cc, value) in &events { if time == at { actual.cc(0, cc, value); split.cc(0, cc, value); } }
                if at == 1537 || at == 1693 {
                    while !actual.script().unwrap().native_state.refresh(&mut snapshot, 1) {}
                    assert!(!snapshot.changed, "invalid live frequency must not change native edit revision/current");
                    let final_count = if sync { 7. } else { f32::from_bits(0x3fbecec0) };
                    assert_eq!(read(&actual.state.base, address), Some(final_count));
                    assert_eq!(read(actual.settings().unwrap(), address), Some(final_count));
                    assert_eq!(address.encode(read(actual.settings().unwrap(), address).unwrap()),
                        if sync { 454545 } else { 500072 });
                    actual.script().unwrap().refresh_persistence(&mut persistent);
                    let crate::ksp::Value::Int(after) = persistent[0]["$after"]
                        else { panic!("missing same-callback frequency readback"); };
                    assert_eq!(after, if sync { 454545 } else { 500072 },
                        "rejected input must not shadow the prior validated pending/current getter");
                }
                if at == 811 || at == 1137 { for e in [&mut actual, &mut split, &mut dry] {
                    e.host_expression(pattern, HostExpression::Gain(if at == 811 { 0. } else { 1. }));
                } }
                if at == 4096 { for e in [&mut actual, &mut split, &mut dry] { e.host_note_off(pattern); } }
                let mut pos = at;
                while pos < end { let next = (pos + 128).min(end); let [l, r] = &mut pcm;
                    actual.render_checked(&mut l[pos..next], &mut r[pos..next]).unwrap(); pos = next;
                }
                let (mut pos, mut part) = (at, 0);
                while pos < end { let next = (pos + [1, 17, 31, 83, 127][part % 5]).min(end); let [l, r] = &mut fragmented;
                    split.render_checked(&mut l[pos..next], &mut r[pos..next]).unwrap(); pos = next; part += 1;
                }
                for frame in at..end { dry.host_expression(pattern, HostExpression::Tune(tune[frame]));
                    let [l, r] = &mut reference; dry.render_checked(&mut l[frame..frame + 1], &mut r[frame..frame + 1]).unwrap();
                    l[frame] *= gain[frame]; r[frame] *= gain[frame];
                }
            }
            for e in [&mut actual, &mut split, &mut dry] { e.mark_host_notes(); }
            while !actual.script().unwrap().native_state.refresh(&mut snapshot, 1) {}
        }), 0);
        for channel in 0..2 { for frame in 0..FRAMES {
            assert!((pcm[channel][frame] - reference[channel][frame]).abs() < 3e-5,
                "native713 sync={sync} channel{channel} frame{frame}: {} vs {}", pcm[channel][frame], reference[channel][frame]);
            assert!((pcm[channel][frame] - fragmented[channel][frame]).abs() < 3e-5);
        } }
        assert!(pcm[0][160..497].iter().any(|v| v.abs() > 0.01));
        assert!(pcm.iter().all(|c| c[1051..1137].iter().all(|v| *v == 0.)));
        assert!(pcm.iter().all(|c| c[4608..].iter().all(|v| *v == 0.)));
        assert_eq!(actual.host_note_end_at(root), dry.host_note_end_at(root));
        assert!(actual.host_note_end_at(root).is_some_and(|end| end > 4096 && end < 4608));
        assert_eq!(split.host_note_end_at(root), actual.host_note_end_at(root));
        let final_count = if sync { 7. } else { f32::from_bits(0x3fbecec0) };
        let settings = &actual.settings().unwrap()[0];
        assert_eq!(settings.pitch_lfos[0].count, final_count);
        assert_eq!(settings.volume_lfos[0].source.count, final_count);
        assert_eq!(read(&actual.state.base, address), Some(final_count));
        assert_eq!(actual.dropped_commands(), 0);
        // The VM retains rejected arguments as unsupported metadata. The live
        // source getter has priority, and neither PCM nor NativeState used it.
        assert_eq!(actual.script().unwrap().env.engine_par(par(7, -1)), Some(1000001));
        let getter = if sync { 454545 } else { 500072 };
        let crate::ksp::Value::Int(after) = actual.script().unwrap().persistence()[0]["$after"] else { panic!("missing readback"); };
        assert_eq!(after, getter);
        let edits = snapshot.saved();
        assert_eq!(edits.iter().filter(|e| e.par.id == id::INTMOD_FREQUENCY).count(), 1);
        assert!(edits.iter().any(|e| e.par == par(7, -1) && e.value == 500000));
        let (rt, errors) = load_scripts_with_state(&instrument, vec![], 48_000., &[], &edits);
        assert!(errors.is_empty(), "{errors:?}");
        let mut restored = create(group, false); restored.set_script(rt);
        assert_eq!(read(restored.settings().unwrap(), address), Some(final_count));
        let crate::ksp::Value::Int(seed) = restored.script().unwrap().persistence()[0]["$seed"] else { panic!("missing seed"); };
        assert_eq!(seed, getter);
    }
}
