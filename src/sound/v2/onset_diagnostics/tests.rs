use super::*;
#[path = "fixtures.rs"]
mod fixtures;

fn load(path: &std::path::Path, traced: bool) -> (V2Core, ScriptUi, Option<Owner>) {
    let request = LoadRequest {
        path: path.to_owned(),
        sample_rate: 48000.,
        ..Default::default()
    };
    let (mut loaded, reader) = if traced {
        let (loaded, reader) = V2Loader
            .prepare_onset(
                &request,
                8192,
                sampler_core::trace::SignalTraceScope::new(60, 60, 8192).unwrap(),
                &|| false,
            )
            .unwrap();
        (loaded, Some(reader))
    } else {
        (
            crate::sound::CoreLoader::prepare(&V2Loader, &request, &mut |_| {}, &|| false).unwrap(),
            None,
        )
    };
    let mut part = loaded.part.take().unwrap();
    part.epoch = 73;
    let plan = part.runtime.active_plan();
    let mut core = V2Core::with_parts(1, 48000.);
    drop(core.install(0, Some(part)));
    (
        core,
        loaded.scripts,
        reader.map(|reader| Owner {
            epoch: 73,
            plan,
            reader,
            canceled: false,
        }),
    )
}
fn block(core: &mut V2Core, scripts: &mut ScriptUi) -> Vec<f32> {
    core.service_ui_read_only(0, &mut scripts.views).unwrap();
    core.begin_block(&BlockInfo {
        frames: 128,
        offline: false,
        ..Default::default()
    });
    let audio = core.render(128).buses[0][0][..128].to_vec();
    core.end_block(128, &mut |_| true);
    core.service_ui_read_only(0, &mut scripts.views).unwrap();
    audio
}

#[test]
fn production_wav_loader_owned_trace_preserves_every_pcm_frame_and_reports_loss() {
    let dir = fixtures::directory();
    let path = dir.path().join("owned.wav");
    fixtures::wav(&path);
    let (mut plain, mut a, _) = load(&path, false);
    let (mut traced, mut b, owner) = load(&path, true);
    let mut owner = owner.unwrap();
    let mut capture = capture::Capture::new(8192);
    let calls = crate::plugin::tests::allocations(|| {
        traced.begin_block(&BlockInfo {
            frames: 128,
            offline: false,
            ..Default::default()
        });
        traced.play(0, Event::midi1(0x90, 60, 100));
        traced.render(128);
        traced.end_block(128, &mut |_| true);
    });
    assert_eq!(
        calls, 0,
        "owned-trace production callback must not allocate or deallocate"
    );
    // Reinstall a fresh owner so the matched PCM pair starts at the same frame.
    let (fresh, views, new_owner) = load(&path, true);
    traced = fresh;
    b = views;
    owner = new_owner.unwrap();
    for core in [&mut plain, &mut traced] {
        core.play(0, Event::midi1(0x90, 60, 100));
    }
    let mut nonzero = false;
    for _ in 0..16 {
        let off = block(&mut plain, &mut a);
        let on = block(&mut traced, &mut b);
        assert_eq!(off, on);
        nonzero |= on.iter().any(|v| *v != 0.);
        owner.drain(&traced, &mut capture).unwrap();
    }
    assert!(nonzero);
    assert_eq!(capture.birth_count(), 1);
    assert_eq!(capture.report()["births_without_signal_rows"], 0);
    assert_eq!(capture.report()["source_and_output_all_zero"], false);
    assert_eq!(capture.report()["signal_dropped"], 0);
    // Bounded off-audio reduction overflow stays distinct from ring overflow.
    let (mut traced, mut scripts, owner) = load(&path, true);
    let mut owner = owner.unwrap();
    traced.play(0, Event::midi1(0x90, 60, 100));
    block(&mut traced, &mut scripts);
    let mut tiny = capture::Capture::new(1);
    assert!(owner.drain(&traced, &mut tiny).is_err());
    assert!(tiny.report()["reduction_overflow"].as_u64().unwrap() > 0);
}

#[test]
fn replaced_or_canceled_owner_cannot_read_even_terminal_rows() {
    let dir = fixtures::directory();
    let path = dir.path().join("owned.wav");
    fixtures::wav(&path);
    let (mut core, mut scripts, owner) = load(&path, true);
    let mut owner = owner.unwrap();
    core.play(0, Event::midi1(0x90, 60, 100));
    block(&mut core, &mut scripts);
    let (mut replacement, _, _) = load(&path, false);
    let mut next = replacement.parts[0].take().unwrap();
    next.epoch = 73;
    drop(core.install(0, Some(next))); // same epoch still rejects actual PlanId mismatch.
    let mut capture = capture::Capture::new(8192);
    assert!(owner.drain(&core, &mut capture).is_err());
    assert!(owner.canceled);
    assert_eq!(capture.birth_count(), 0);
    let (core, _, owner) = load(&path, true);
    let mut owner = owner.unwrap();
    owner.canceled = true;
    assert!(owner.check(&core).is_err());
}

#[test]
fn authored_nki_loader_observes_real_controller_callback_and_residency_progress() {
    let dir = fixtures::directory();
    let source = "on init declare $fixture_base := 12 declare ui_switch $selector set_controller(12,31) end on\n\
        on persistence_changed wait(4000) set_controller(12,97) purge_group(0,1) end on\n\
        on ui_control($selector) set_controller(13,53) end on";
    let path = fixtures::nki(dir.path(), source);
    let (mut core, mut scripts, owner) = load(&path, true);
    let mut owner = owner.unwrap();
    assert_eq!(
        scripts.views.len(),
        1,
        "real compiled KSP ledger is required"
    );
    let before = state(owner.check(&core).unwrap()).unwrap();
    assert!(before["effective_controllers"][12].as_u64().unwrap() > 0);
    let mut callbacks = Callbacks::default();
    let mut saw_finite = false;
    let mut committed = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        core.begin_block(&BlockInfo {
            frames: 128,
            offline: false,
            ..Default::default()
        });
        core.render(128);
        callbacks.observe(owner.check(&core).unwrap(), &scripts.views);
        saw_finite |= callbacks.finite != 0;
        core.end_block(128, &mut |_| true);
        core.service_ui_read_only(0, &mut scripts.views).unwrap();
        let part = owner.check(&core).unwrap();
        let p = part.runtime.residency_progress(owner.plan).unwrap();
        if p.accepted > 0 && p.pending == 0 && p.committed > 0 && callbacks.finite == 0 {
            committed = true;
            break;
        }
        std::thread::yield_now();
    }
    assert!(
        saw_finite,
        "authored suspended persistence callback must be observed"
    );
    assert!(committed, "real accepted residency completion must commit");
    let actual = state(owner.check(&core).unwrap()).unwrap();
    assert!(
        actual["effective_controllers"][12].as_u64().unwrap()
            > before["effective_controllers"][12].as_u64().unwrap()
    );
    let c=Config::parse(&json!({"schema":1,"opt_in":true,"path":path,"state_origin":"saved_state_diagnostic",
        "input_origin":"synthetic_native","warm_intervention":"exact_first_windows",
        "graph_scope":{"prepared_key_low":60,"prepared_key_high":60,"max_nodes":8192},
        "selection":{"slot":0,"articulation":0,"selector_ordinal":0,"selector_source":32768,"keyswitch":12,
            "rr_cycle":1,"keyswitch_base_field":"$fixture_base"},"rate":48000,"block":128,"key":60,"velocity":100,
        "hold_ms":500,"spacing_ms":3000,"startup_ms":30000,"phase_ms":10000,"total_ms":120000,
        "tail_ms":5000,"pacing_tolerance_us":1000,"row_capacity":8192})).unwrap();
    let selected = selector(core.parts[0].as_deref_mut().unwrap(), &scripts, &c, true).unwrap();
    assert_eq!(selected["base"], 12);
    let mut wrong = c.clone();
    wrong.selection.selector_source += 1;
    assert!(
        selector(
            core.parts[0].as_deref_mut().unwrap(),
            &scripts,
            &wrong,
            true
        )
        .is_err()
    );
    wrong = c.clone();
    wrong.selection.keyswitch = 13;
    assert!(
        selector(
            core.parts[0].as_deref_mut().unwrap(),
            &scripts,
            &wrong,
            true
        )
        .is_err()
    );
    for _ in 0..4 {
        block(&mut core, &mut scripts);
    }
    core.play(0, Event::midi1(0x90, 60, 100));
    let mut capture = capture::Capture::new(8192);
    let mut sounded = false;
    for _ in 0..8 {
        sounded |= block(&mut core, &mut scripts).iter().any(|v| *v != 0.);
        owner.drain(&core, &mut capture).unwrap();
    }
    assert!(sounded);
    assert_eq!(capture.birth_count(), 1);
    capture.mark_first_trial(0);
    let warm = capture
        .warm_first_windows(core.parts[0].as_deref_mut().unwrap(), owner.plan, true)
        .unwrap();
    assert_eq!(warm["verified_ready"], true);
}

#[test]
fn malformed_plan_is_rejected_before_touching_an_authored_or_real_path() {
    assert!(run(&json!({"path":"/never-open-this.nki"})).is_err());
}

#[test]
fn exact_cold_window_intervention_uses_actual_asset_and_real_page_completion() {
    use sampler_core::{Envelope, Limits, Pcm, Playback, Prepared, Region, Runtime, StreamCache};
    let pcm = Pcm::streamed(48000, 4096).unwrap();
    let plan = Prepared::new(
        48000,
        vec![pcm.clone()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        }],
        1,
    )
    .unwrap();
    let limits = Limits::for_plan(&plan, 8, 16);
    let (plan, reader) = plan.with_signal_trace_reader(8192).unwrap();
    let (cache, mut worker) = StreamCache::new(4).unwrap();
    let mut runtime = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
    runtime.set_cold_starts(true);
    let mut part = Part::new(
        runtime,
        super::super::MixTree::instrument("authored warm window"),
    )
    .unwrap();
    part.epoch = 91;
    part.horizon = Some(128);
    part.waveform_sources
        .insert(1, crate::sound::waveform::Source { pcm, stream: None });
    let mut owner = Owner {
        epoch: 91,
        plan: part.runtime.active_plan(),
        reader,
        canceled: false,
    };
    let mut core = V2Core::with_parts(1, 48000.);
    drop(core.install(0, Some(Box::new(part))));
    core.play(0, Event::midi1(0x90, 60, 100));
    core.render(128);
    let mut capture = capture::Capture::new(8192);
    owner.drain(&core, &mut capture).unwrap();
    assert_eq!(capture.birth_count(), 1);
    capture.mark_first_trial(0);
    let before = capture
        .warm_first_windows(core.parts[0].as_deref_mut().unwrap(), owner.plan, true)
        .unwrap();
    assert_eq!(before["verified_ready"], false);
    let mut job = worker
        .next_job()
        .expect("an actual page request is required");
    job.frames_mut().fill([0.25; 2]);
    worker.complete(job, Ok(())).unwrap();
    core.render(128);
    let after = capture
        .warm_first_windows(core.parts[0].as_deref_mut().unwrap(), owner.plan, false)
        .unwrap();
    assert_eq!(after["verified_ready"], true);
    core.parts[0]
        .as_deref_mut()
        .unwrap()
        .waveform_sources
        .clear();
    assert!(
        capture
            .warm_first_windows(core.parts[0].as_deref_mut().unwrap(), owner.plan, false)
            .is_err(),
        "missing exact asset association must refuse rather than guess a matching sample"
    );
}

#[test]
fn generated_outside_scope_birth_is_partial_while_full_production_pcm_is_unchanged() {
    use sampler_core::trace::SignalTraceScope;
    use sampler_core::{
        Envelope, Instruction, Limits, Pcm, Playback, Prepared, Program, Region, Runtime,
    };
    let make = |high: Option<u8>| {
        let assets = vec![
            Pcm::new(48000, vec![[0.125, 0.]; 512].into()).unwrap(),
            Pcm::new(48000, vec![[0., 0.25]; 512].into()).unwrap(),
        ];
        let region = |sample, key| Region {
            sample,
            key_low: key,
            key_high: key,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        };
        let play = |transpose| Instruction::Play {
            transpose,
            velocity: sampler_core::Velocity::Scale(1.),
            inheritance: sampler_core::Inheritance::Linked,
            duration: sampler_core::Duration::FramesOrGate(512),
        };
        let plan = Prepared::new(48000, assets, vec![region(0, 60), region(1, 72)], 2)
            .unwrap()
            .with_programs(
                vec![Program::new(vec![play(0), play(12), Instruction::End]).unwrap()],
                Some(0),
            )
            .unwrap();
        let limits = Limits::for_plan(&plan, 8, 16);
        let (plan, reader) = if let Some(high) = high {
            let (plan, reader) = plan
                .with_scoped_signal_trace_reader(
                    8192,
                    SignalTraceScope::new(60, high, 128).unwrap(),
                )
                .unwrap();
            (plan, Some(reader))
        } else {
            (plan, None)
        };
        let mut part = Part::new(
            Runtime::new(plan, limits).unwrap(),
            super::super::MixTree::instrument("source-free graph scope"),
        )
        .unwrap();
        part.epoch = 101;
        let plan = part.runtime.active_plan();
        let mut core = V2Core::with_parts(1, 48000.);
        drop(core.install(0, Some(Box::new(part))));
        (
            core,
            reader.map(|reader| Owner {
                epoch: 101,
                plan,
                reader,
                canceled: false,
            }),
        )
    };
    let render = |core: &mut V2Core| {
        let mut output = vec![[0f32; 2]; 512];
        let calls = crate::plugin::tests::allocations(|| {
            for (index, chunk) in output.chunks_mut(128).enumerate() {
                core.begin_block(&BlockInfo {
                    frames: 128,
                    offline: false,
                    ..Default::default()
                });
                if index == 0 {
                    core.play(0, Event::midi1(0x90, 60, 127));
                }
                let bus = &core.render(128).buses[0];
                for (i, frame) in chunk.iter_mut().enumerate() {
                    *frame = [bus[0][i], bus[1][i]];
                }
                core.end_block(128, &mut |_| true);
            }
        });
        assert_eq!(calls, 0, "generated in/out-scope callback alloc/free");
        output
    };
    let (mut plain, _) = make(None);
    let off = render(&mut plain);
    for (high, missing, status) in [(60, 1, "partial"), (72, 0, "covered_observed_births")] {
        let (mut core, owner) = make(Some(high));
        let mut owner = owner.unwrap();
        let on = render(&mut core);
        assert_eq!(off, on);
        assert!(on.iter().any(|f| f[0] != 0.));
        assert!(on.iter().any(|f| f[1] != 0.));
        let mut capture = capture::Capture::new(8192);
        owner.drain(&core, &mut capture).unwrap();
        let report = capture.report();
        assert_eq!(report["births"].as_array().unwrap().len(), 2);
        assert_eq!(report["first_windows"].as_array().unwrap().len(), 2);
        assert_eq!(report["births_without_contribution_graph"], missing);
        assert_eq!(report["births_without_signal_rows"], missing);
        assert_eq!(report["graph_coverage_status"], status);
        assert_eq!(report["signal_dropped"], 0);
        assert_eq!(report["onset_dropped"], 0);
        if high == 60 {
            let rows = report["birth_graph_coverage"].as_array().unwrap();
            let outside = rows.iter().find(|r| r["actual_birth_key"] == 72).unwrap();
            assert_eq!(outside["status"], "partial");
            assert_eq!(
                outside["reason"],
                "selected_birth_has_no_contribution_graph"
            );
            assert!(outside["contribution_node"].is_null());
            assert!(report["source_and_output_all_zero"].is_null());
        }
    }
}

#[test]
fn warm_revisit_compares_resolved_rr_assets_instead_of_assuming_second_note() {
    use sampler_core::{
        Envelope, Input, Limits, Pcm, Playback, Prepared, Protocol, Region, Runtime, Sequence,
        SequenceScope, Take, TakePolicy, trace,
    };
    let region = |sample| Region {
        sample,
        key_low: 60,
        key_high: 60,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback::default(),
    };
    let plan = Prepared::new(
        48000,
        (0..2)
            .map(|i| Pcm::new(48000, vec![[0.125 * (i + 1) as f32; 2]; 128].into()).unwrap())
            .collect(),
        vec![region(0), region(1)],
        2,
    )
    .unwrap()
    .with_variation(
        vec![Sequence {
            takes: 2,
            policy: TakePolicy::Sequential,
            scope: SequenceScope::Global,
            capacity: 1,
        }],
        vec![
            Some(Take {
                sequence: 0,
                index: 0,
            }),
            Some(Take {
                sequence: 0,
                index: 1,
            }),
        ],
        1,
        0,
    )
    .unwrap();
    let limits = Limits::for_plan(&plan, 8, 16);
    let (plan, reader) = plan
        .with_scoped_signal_trace_reader(8192, trace::SignalTraceScope::new(60, 60, 128).unwrap())
        .unwrap();
    let mut runtime = Runtime::new(plan, limits).unwrap();
    let mut capture = capture::Capture::new(8192);
    let mut trials = Vec::new();
    for trial in 0..3 {
        let start = capture.birth_count();
        runtime
            .trigger(
                Input {
                    protocol: Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 60,
                    external_id: Some(trial),
                },
                60,
                1.,
            )
            .unwrap();
        runtime.render(&mut [[0.; 2]; 128]).unwrap();
        capture.drain(&reader).unwrap();
        trials.push(json!({"birth_start":start,"birth_end":capture.birth_count()}));
    }
    assert_eq!(
        capture.compare_trials(&trials[..2])["births_and_cursors_match"],
        false
    );
    assert_eq!(
        capture.compare_trials(&trials)["births_and_cursors_match"],
        true
    );
}
