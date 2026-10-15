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
    assert!(owner.observe(&core, &mut capture).is_err());
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

fn render_without_heap(core: &mut V2Core, event: Option<Event>) -> [[f32; 2]; 128] {
    let mut pcm = [[0.; 2]; 128];
    let calls = crate::plugin::tests::allocations(|| {
        core.begin_block(&BlockInfo {
            frames: 128,
            offline: false,
            ..Default::default()
        });
        if let Some(event) = event {
            core.play(0, event);
        }
        let bus = &core.render(128).buses[0];
        for (i, frame) in pcm.iter_mut().enumerate() {
            *frame = [bus[0][i], bus[1][i]];
        }
        core.end_block(128, &mut |_| true);
    });
    assert_eq!(
        calls, 0,
        "production cold/ready/retirement callback alloc or free"
    );
    pcm
}

#[test]
fn staggered_distinct_asset_cold_ready_retirement_observations_preserve_exact_pcm() {
    let exercise = |traced| {
        let (mut core, mut owner, mut worker, assets) = fixtures::streamed_layers(traced, 8192);
        let mut capture = capture::Capture::new(8192);
        let mut output = Vec::new();
        output.extend(render_without_heap(
            &mut core,
            Some(Event::midi1(0x90, 60, 127)),
        ));
        if let Some(owner) = &mut owner {
            owner.drain(&core, &mut capture).unwrap();
            owner.observe(&core, &mut capture).unwrap();
        }
        let first = worker.next_job().unwrap();
        let second = worker.next_job().unwrap();
        assert!(worker.next_job().is_none());
        let mut jobs = if first.key().asset == assets[0].asset_id() {
            [Some(first), Some(second)]
        } else {
            [Some(second), Some(first)]
        };
        for block in 1..9 {
            if block == 1 || block == 3 {
                let asset = usize::from(block == 3);
                let mut job = jobs[asset].take().unwrap();
                assert_eq!(job.key().asset, assets[asset].asset_id());
                for (i, frame) in job.frames_mut().iter_mut().enumerate() {
                    *frame = [0.; 2];
                    if i == 0 {
                        frame[asset] = 0.5;
                    }
                }
                worker.complete(job, Ok(())).unwrap();
            }
            output.extend(render_without_heap(&mut core, None));
            if let Some(owner) = &mut owner {
                owner.drain(&core, &mut capture).unwrap();
                owner.observe(&core, &mut capture).unwrap();
            }
        }
        assert_eq!(core.parts[0].as_ref().unwrap().runtime.voice_count(), 0);
        (output, capture.report())
    };
    let (plain, _) = exercise(false);
    let (traced, report) = exercise(true);
    assert_eq!(
        plain, traced,
        "observer must preserve every production PCM frame"
    );
    assert_eq!(traced.iter().position(|f| f[0] != 0.), Some(128));
    assert_eq!(traced.iter().position(|f| f[1] != 0.), Some(384));
    let births = report["births"].as_array().unwrap();
    assert_eq!(births.len(), 2);
    assert_eq!(births[0]["event"], births[1]["event"]);
    assert_eq!(births[0]["family"], births[1]["family"]);
    assert_ne!(births[0]["asset"], births[1]["asset"]);
    let edges = report["voice_state_observations"].as_array().unwrap();
    assert_eq!(
        edges.len(),
        6,
        "two cold, two independently ready, two retired"
    );
    let ready: Vec<_> = edges
        .iter()
        .filter(|e| e["current_observation"]["cold"] == false)
        .collect();
    assert_eq!(ready.len(), 2);
    assert_eq!(ready[0]["interval_start"], 128);
    assert_eq!(ready[0]["current_observation"]["at"], 256);
    assert_eq!(ready[1]["interval_start"], 384);
    assert_eq!(ready[1]["current_observation"]["at"], 512);
    for row in edges {
        assert_eq!(row["sample_exact_transition"], false);
        if row["kind"] == "not_present_after_block" {
            assert!(row["termination_reason"].is_null());
        }
    }
    assert_eq!(report["births_without_between_block_snapshot"], 0);
    assert_eq!(report["voices_present_at_last_observation"], 0);
    assert_eq!(report["voice_observation_loss"], 0);
    assert_eq!(report["graph_coverage_status"], "covered_observed_births");
}

#[test]
fn natural_inspection_does_not_request_poll_protect_or_advance_clock() {
    use sampler_core::{PageKey, PageStatus, Pcm};
    let (mut core, owner, mut worker, assets) = fixtures::streamed_layers(true, 8192);
    let mut owner = owner.unwrap();
    render_without_heap(&mut core, Some(Event::midi1(0x90, 60, 100)));
    let mut capture = capture::Capture::new(8192);
    owner.drain(&core, &mut capture).unwrap();
    capture.mark_first_trial(0);
    let part = core.parts[0].as_deref_mut().unwrap();
    let at = part.runtime.now();
    for _ in 0..2 {
        let mut job = worker.next_job().unwrap();
        job.frames_mut().fill([0.25; 2]);
        worker.complete(job, Ok(())).unwrap();
    }
    assert!(worker.next_job().is_none());
    let inspection = capture.inspect_first_windows(part, owner.plan).unwrap();
    assert_eq!(inspection["requested_pages"], 0);
    assert_eq!(inspection["page_protection_requested"], false);
    assert_eq!(inspection["production_service_requested"], false);
    assert_eq!(inspection["declared_intervention"], false);
    assert_eq!(inspection["sample_clock_advanced"], false);
    assert_eq!(
        inspection["verified_ready"], false,
        "queued replies must remain unpublished"
    );
    assert_eq!(part.runtime.now(), at);
    assert!(
        worker.next_job().is_none(),
        "inspection must enqueue no worker job"
    );
    for pcm in &assets {
        assert_eq!(
            part.runtime.stream_cache_mut().unwrap().status(PageKey {
                asset: pcm.asset_id(),
                index: 0
            }),
            PageStatus::Pending
        );
    }
    let forced = capture.warm_first_windows(part, owner.plan, true).unwrap();
    assert_eq!(forced["production_service_requested"], true);
    assert_eq!(part.runtime.now(), at);
    assert_eq!(
        capture.inspect_first_windows(part, owner.plan).unwrap()["verified_ready"],
        true
    );
    let cache = part.runtime.stream_cache_mut().unwrap();
    cache.begin_epoch().unwrap();
    // Protection is observable through eviction availability, independent of
    // the diagnostic's self-reported booleans.
    assert_eq!(
        capture.inspect_first_windows(part, owner.plan).unwrap()["verified_ready"],
        true
    );
    for _ in 0..2 {
        let other = Pcm::streamed(48000, 512).unwrap();
        part.runtime
            .stream_cache_mut()
            .unwrap()
            .request(&other, 0, at)
            .unwrap();
    }
    for pcm in &assets {
        assert_eq!(
            part.runtime.stream_cache_mut().unwrap().status(PageKey {
                asset: pcm.asset_id(),
                index: 0
            }),
            PageStatus::Missing
        );
    }
    // Missing historical windows stay missing; inspection must not request.
    for _ in 0..2 {
        assert!(worker.next_job().is_some());
    }
    assert!(worker.next_job().is_none());
    assert_eq!(
        capture.inspect_first_windows(part, owner.plan).unwrap()["verified_ready"],
        false
    );
    assert!(worker.next_job().is_none());
}

#[test]
fn voice_observer_capacity_loss_is_sticky_and_stale_owner_cannot_observe() {
    let (mut core, owner, _worker, _) = fixtures::streamed_layers(true, 8192);
    let mut owner = owner.unwrap();
    render_without_heap(&mut core, Some(Event::midi1(0x90, 60, 100)));
    let mut tiny = capture::Capture::new(1);
    assert!(owner.observe(&core, &mut tiny).is_err());
    assert_eq!(tiny.report()["voice_observation_loss"], 1);
    assert!(owner.observe(&core, &mut tiny).is_err());
    assert_eq!(tiny.report()["voice_observation_loss"], 1);
    assert_eq!(tiny.report()["graph_coverage_status"], "partial");
    let mut normal = capture::Capture::new(8192);
    owner.canceled = true;
    assert!(owner.observe(&core, &mut normal).is_err());
    assert_eq!(normal.report()["voice_sample_ticks"], 0);
}

#[test]
fn onset_ring_loss_and_within_block_birth_death_remain_explicit() {
    let (mut core, owner, _worker, _) = fixtures::streamed_layers(true, 1);
    let mut owner = owner.unwrap();
    render_without_heap(&mut core, Some(Event::midi1(0x90, 60, 100)));
    let mut capture = capture::Capture::new(8192);
    assert!(owner.drain(&core, &mut capture).is_err());
    assert!(capture.report()["onset_dropped"].as_u64().unwrap() > 0);
    let dir = fixtures::directory();
    let path = dir.path().join("short.wav");
    fixtures::wav_frames(&path, 64);
    let (mut core, mut scripts, owner) = load(&path, true);
    let mut owner = owner.unwrap();
    core.play(0, Event::midi1(0x90, 60, 100));
    block(&mut core, &mut scripts);
    let mut capture = capture::Capture::new(8192);
    owner.drain(&core, &mut capture).unwrap();
    owner.observe(&core, &mut capture).unwrap();
    assert_eq!(capture.report()["births_without_between_block_snapshot"], 1);
    assert_eq!(
        capture.report()["within_block_birth_and_death_observation"],
        false
    );
}

#[test]
fn release_and_delayed_gap_births_are_covered_outside_rr_attack_ranges() {
    use sampler_core::{
        Envelope, Input, Instruction, Limits, Pcm, Playback, Prepared, Program, Protocol, Region,
        ReleaseOptions, Runtime, Trigger, WaitLifetime,
    };
    let region = Region {
        sample: 0,
        key_low: 60,
        key_high: 60,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback::default(),
    };
    let release = Program::new(vec![
        Instruction::ForwardReleaseGroups,
        Instruction::Wait(256),
        Instruction::Play {
            transpose: 0,
            velocity: sampler_core::Velocity::Scale(1.),
            inheritance: sampler_core::Inheritance::Linked,
            // This authored gap note must outlive the already closed parent gate.
            duration: sampler_core::Duration::Frames(1024),
        },
        Instruction::End,
    ])
    .unwrap()
    .with_wait_lifetime(WaitLifetime::Callback);
    let plan = Prepared::new(
        48000,
        vec![Pcm::new(48000, vec![[0.25; 2]; 512].into()).unwrap()],
        vec![region.clone(), region],
        2,
    )
    .unwrap()
    .with_releases(
        vec![Trigger::Attack, Trigger::KeyRelease],
        ReleaseOptions::default(),
        ReleaseOptions::default(),
    )
    .unwrap()
    .with_programs(vec![release], None)
    .unwrap()
    .with_release_program(0)
    .unwrap();
    let limits = Limits::for_plan(&plan, 8, 16);
    let (plan, reader) = plan.with_signal_trace_reader(8192).unwrap();
    let mut runtime = Runtime::new(plan, limits).unwrap();
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(1),
    };
    runtime.trigger(input, 60, 1.).unwrap();
    runtime.render(&mut [[0.; 2]; 128]).unwrap();
    let mut capture = capture::Capture::new(8192);
    capture.drain(&reader).unwrap();
    let attack_end = capture.birth_count();
    assert_eq!(attack_end, 1);
    runtime.note_off(input, None).unwrap();
    for _ in 0..4 {
        runtime.render(&mut [[0.; 2]; 128]).unwrap();
        capture.drain(&reader).unwrap();
    }
    assert_eq!(
        capture.birth_count(),
        3,
        "attack, release, actual delayed callback play"
    );
    let trials = [
        json!({"trial":0,"note_on_frame":0,"note_off_frame":128,"birth_start":0,"birth_end":attack_end}),
    ];
    let phases = capture.birth_phases(&trials);
    let rows = phases.as_array().unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["phase"], "attack_hold");
    assert_eq!(rows[1]["phase"], "release_gap_or_tail");
    assert_eq!(rows[2]["phase"], "release_gap_or_tail");
    assert!(rows[2]["at_frame"].as_u64().unwrap() > rows[1]["at_frame"].as_u64().unwrap());
}

#[test]
fn authored_production_host_executes_cold_natural_and_forced_schedule() {
    let dir = fixtures::directory();
    let path = fixtures::nki(
        dir.path(),
        "on init declare $fixture_base := 0 declare ui_switch $selector end on",
    );
    let plan = json!({"schema":1,"opt_in":true,"path":path,"state_origin":"saved_state_diagnostic",
        "input_origin":"synthetic_native","warm_intervention":"natural_then_exact_first_windows",
        "graph_scope":{"prepared_key_low":60,"prepared_key_high":60,"max_nodes":8192},
        "selection":{"slot":0,"articulation":0,"selector_ordinal":0,"selector_source":32768,"keyswitch":0,
            "rr_cycle":1,"keyswitch_base_field":"$fixture_base"},"rate":48000,"block":128,"key":60,"velocity":100,
        "hold_ms":50,"spacing_ms":1000,"startup_ms":1000,"phase_ms":1000,"total_ms":10000,
        "tail_ms":1000,"pacing_tolerance_us":1000,"row_capacity":8192});
    let report = run(&plan).unwrap();
    assert!(report["error"].is_null(), "{}", report["error"]);
    let details = &report["details"];
    assert_eq!(details["selector_after"]["base"], 0);
    assert_eq!(details["trials"].as_array().unwrap().len(), 3);
    assert_eq!(details["natural_revisit_trial"], 1);
    assert_eq!(details["forced_revisit_trial"], 2);
    assert_eq!(
        details["natural_revisit_readiness"]["declared_intervention"],
        false
    );
    assert_eq!(
        details["natural_observed_selection_match"]["births_and_cursors_match"],
        true
    );
    assert_eq!(details["exact_asset_window_revisit_verified"], true);
    assert_eq!(details["complete_tail"], true);
    assert_eq!(
        details["birth_phase_coverage"].as_array().unwrap().len(),
        report["capture"]["births"].as_array().unwrap().len()
    );
    assert_eq!(report["capture"]["voice_observation_loss"], 0);
}

fn short_authored_plan(path: &std::path::Path, mode: &str) -> Value {
    json!({"schema":1,"opt_in":true,"path":path,"state_origin":"saved_state_diagnostic",
        "input_origin":"synthetic_native","warm_intervention":mode,
        "graph_scope":{"prepared_key_low":60,"prepared_key_high":60,"max_nodes":8192},
        "selection":{"slot":0,"articulation":0,"selector_ordinal":0,"selector_source":32768,"keyswitch":0,
            "rr_cycle":1,"keyswitch_base_field":"$fixture_base"},"rate":48000,"block":128,"key":60,"velocity":100,
        "hold_ms":50,"spacing_ms":1000,"startup_ms":1000,"phase_ms":1000,"total_ms":10000,
        "tail_ms":1000,"pacing_tolerance_us":1000,"row_capacity":8192})
}

#[test]
fn startup_refusal_retains_bounded_actual_finite_callback_evidence() {
    let dir = fixtures::directory();
    let source = "on init declare $fixture_base := 0 declare ui_switch $selector set_listener($NI_SIGNAL_TIMER_MS,1000) end on \
        on persistence_changed wait(5000000) set_controller(12,77) end on \
        on listener set_controller(13,31) end on";
    let path = fixtures::nki(dir.path(), source);
    let (mut core, mut scripts, owner) = load(&path, true);
    let mut owner = owner.unwrap();
    let mut bounded = Callbacks::with_limit(1);
    for _ in 0..4 {
        block(&mut core, &mut scripts);
    }
    bounded.observe(owner.check(&core).unwrap(), &scripts.views);
    let progress = bounded.report();
    assert_eq!(progress["progress_limit"], 1);
    assert_eq!(progress["progress"].as_array().unwrap().len(), 1);
    assert!(progress["progress_omitted"].as_u64().unwrap() > 0);
    assert!(
        progress["finite"].as_u64().unwrap() > 0,
        "omitted rows must still count toward readiness"
    );
    let report = run(&short_authored_plan(
        &path,
        "natural_then_exact_first_windows",
    ))
    .unwrap();
    assert_eq!(report["error"], "Diagnostic wall deadline exceeded");
    let witness = &report["last_settlement"];
    assert_eq!(witness["phase"], "startup");
    assert_eq!(witness["completed"], false);
    assert_eq!(witness["finite_work_quiet"], false);
    assert!(witness["finite_callbacks"].as_u64().unwrap() > 0);
    assert_eq!(witness["stable_tick_count"], 0);
    assert!(witness["at_frame"].as_u64().unwrap() > 0);
    assert_eq!(report["capture"]["births"].as_array().unwrap().len(), 0);
    let rows = report["last_callback_observation"]["progress"]
        .as_array()
        .unwrap();
    assert!(rows.iter().any(|r| r["role"] == "on persistence_changed"
        && r["outcome"] == "pending"
        && r["waiting"] == true
        && r["active_plan_owner"] == true));
    assert_eq!(report["last_callback_observation"]["progress_omitted"], 0);
}

#[test]
fn authored_listener_exposes_timer_commands_and_real_continuously_changed_domains() {
    let dir = fixtures::directory();
    let path = fixtures::nki(
        dir.path(),
        "on init declare $fixture_base := 0 declare $ticks := 0 \
        declare ui_switch $selector set_listener($NI_SIGNAL_TIMER_MS,5000) end on \
        on listener inc($ticks) if ($ticks > 127) $ticks := 0 end if set_controller(12,$ticks) end on",
    );
    let (mut core, mut scripts, owner) = load(&path, true);
    let mut owner = owner.unwrap();
    let mut callbacks = Callbacks::default();
    let mut previous = None;
    let mut controller_changes = 0;
    let mut parked_command_observed = false;
    for _ in 0..32 {
        core.begin_block(&BlockInfo {
            frames: 128,
            offline: false,
            ..Default::default()
        });
        core.render(128);
        callbacks.observe(owner.check(&core).unwrap(), &scripts.views);
        let actual = state(owner.check(&core).unwrap()).unwrap();
        if let Some(previous) = previous.as_ref() {
            let domains = changed_state_domains(previous, &actual);
            controller_changes += usize::from(domains.contains(&"effective_controllers"));
            assert!(!domains.contains(&"input_controllers"),
                "script controller writes must preserve the admitted host-input state");
            assert!(
                !domains.contains(&"groups"),
                "authored modulation must not invent a residency change"
            );
        }
        previous = Some(actual);
        let part = owner.check(&core).unwrap();
        if callbacks.finite == 0
            && !callbacks.completed_listeners.is_empty()
            && part.runtime.pending_commands() > 0
        {
            parked_command_observed = true;
            assert!(
                callbacks
                    .progress
                    .iter()
                    .any(|r| r.role == Some("listener driver")
                        && r.waiting
                        && r.outcome == "pending"
                        && r.active_plan_owner)
            );
        }
        core.end_block(128, &mut |_| true);
        core.service_ui_read_only(0, &mut scripts.views).unwrap();
    }
    assert!(
        parked_command_observed,
        "finite activation can finish while actual listener Resume timers remain"
    );
    assert!(
        controller_changes > 8,
        "real finite listeners continuously change numeric controllers"
    );
    let refused = run(&short_authored_plan(&path, "exact_first_windows")).unwrap();
    assert_eq!(refused["error"], "Diagnostic wall deadline exceeded");
    assert_eq!(refused["last_settlement"]["completed"], false);
    assert!(refused["last_settlement"]["root_pending"].as_u64().unwrap() > 0);
    assert_eq!(refused["last_settlement"]["finite_callbacks"], 0);
    assert!(
        refused["last_settlement"]["numeric_state_change_ticks"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        refused["last_settlement"]["last_changed_state_domains"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "effective_controllers")
    );
    assert!(
        !refused["last_callback_observation"]["completed_listener_programs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let natural = run(&short_authored_plan(
        &path,
        "natural_then_exact_first_windows",
    ))
    .unwrap();
    assert!(natural["error"].is_null(), "{}", natural["last_settlement"]);
    assert_eq!(natural["details"]["trials"].as_array().unwrap().len(), 3);
    assert_eq!(natural["last_settlement"]["completed"], true);
    assert!(
        natural["last_settlement"]["scheduled_work"]["exact_future_parked_listener_resumes"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        natural["last_settlement"]["scheduled_work"]["blocking_observed"],
        0
    );
    assert_eq!(
        natural["last_settlement"]["scheduled_work"]["truncated"],
        false
    );
    assert!(
        natural["details"]["trials"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["observed_numeric_state_matches_first"] == false)
    );
    assert_eq!(
        natural["details"]["observed_selection_match"]["observed_numeric_state_matches_first"],
        false
    );
    assert_eq!(
        natural["details"]["natural_observed_selection_match"]["observed_selection_and_state_match"],
        false
    );
    assert_eq!(
        natural["details"]["exact_asset_window_revisit_verified"], false,
        "changed full state must never silently produce a verified cold/warm MATCH"
    );
    assert_eq!(natural["details"]["complete_tail"], true);
}
