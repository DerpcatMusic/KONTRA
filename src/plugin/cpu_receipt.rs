//! Numeric whole-callback witness using KONTRA's actual busy/span meter inputs.
//! Owned resident PCM, exact host owners, no DAW, UI painter or authored library.
use super::*;
use sampler_core::{Envelope, Loop, LoopMode, LoopShape, Pcm, Playback, Prepared, Region, Runtime};

impl CallbackAudit {
    fn receipt(self, busy_ns: u64, callbacks: usize, block: usize) -> serde_json::Value {
        let chunks = block.div_ceil(MAX_BLOCK) as u64;
        let expected = [1, 1, chunks + 1, chunks, chunks, chunks, 1, 1, 1, 1];
        for (calls, expected) in self.calls.iter().zip(expected) {
            assert_eq!(*calls, expected * callbacks as u64);
        }
        let wall_sum = self.wall_ns.iter().sum::<u64>();
        assert!(wall_sum <= busy_ns);
        let names = [
            "adoption_polling",
            "begin_transport_controls_keyboard",
            "host_events",
            "audition_master_gain",
            "core_render_inclusive",
            "output_mix_scope",
            "ui_effect_readback",
            "end_block_retirement",
            "meter_publication",
            "voice_readback",
        ];
        serde_json::json!({
            "scope":"test-only actual process groups; elapsed includes descheduling/probes; caller CPU excludes workers and outside observers; core group overlaps nested V2 stages",
            "caller_cpu_available":cfg!(target_os="linux"),
            "groups": names.into_iter().enumerate().map(|(i,name)| serde_json::json!({
                "name":name,"calls":self.calls[i],"wall_ns":self.wall_ns[i],"caller_thread_cpu_ns":self.caller_cpu_ns[i],
            })).collect::<Vec<_>>(),
            "timed_disjoint_wall_sum_ns":wall_sum,
            "caller_thread_cpu_sum_ns":self.caller_cpu_ns.iter().sum::<u64>(),
            "callback_unattributed_wall_ns_including_final_probe":busy_ns-wall_sum,
        })
    }
}

#[test]
#[ignore = "optimized CPU receipt: run with no competing builds or benchmarks"]
fn whole_callback_300_voice_cpu_receipt() {
    let block: usize = std::env::var("KONTRA_CPU_RECEIPT_BLOCK")
        .unwrap_or("256".into())
        .parse()
        .unwrap();
    let blocks: usize = std::env::var("KONTRA_CPU_RECEIPT_BLOCKS")
        .unwrap_or("1000".into())
        .parse()
        .unwrap();
    let layout = std::env::var("KONTRA_CPU_RECEIPT_LAYOUT").unwrap_or("distinct".into());
    let workload = std::env::var("KONTRA_CPU_RECEIPT_WORKLOAD").unwrap_or("fractional".into());
    // Companion attribution only: stage clocks perturb the callback. Keep the
    // ordinary meter receipt uninstrumented unless explicitly requested.
    let stage_audit = match std::env::var("KONTRA_CPU_RECEIPT_AUDIT").as_deref() {
        Err(_) | Ok("0") => false,
        Ok("1") => true,
        _ => panic!("KONTRA_CPU_RECEIPT_AUDIT must be 0 or 1"),
    };
    assert!([64, 256].contains(&block) && blocks >= 100);
    assert!(matches!(layout.as_str(), "shared" | "distinct"));
    assert!(matches!(
        workload.as_str(),
        "plain" | "fractional" | "eq-stereo"
    ));
    const VOICES: usize = 300;
    const FRAMES: usize = 4096;
    let count = if layout == "shared" { 1 } else { VOICES };
    let assets = (0..count)
        .map(|asset| {
            Pcm::new(
                48000,
                (0..FRAMES)
                    .map(|i| {
                        let x = (i as f32 * (0.013 + asset as f32 * 0.0001)).sin() * 0.2;
                        [x, (i as f32 * 0.017).cos() * 0.15]
                    })
                    .collect(),
            )
            .unwrap()
        })
        .collect();
    let velocity = |id: usize| (id as f64 + 0.5) / VOICES as f64;
    let regions = (0..count)
        .map(|id| Region {
            sample: id,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: if count == 1 {
                0.
            } else {
                id as f64 / VOICES as f64
            },
            velocity_high: if count == 1 {
                1.
            } else {
                (id + 1) as f64 / VOICES as f64
            },
            gain: 0.008,
            envelope: Envelope::new(64, 0, 128, 0.8, 256).unwrap(),
            playback: Playback {
                transpose_semitones: if workload == "plain" { 0. } else { 3.17 },
                loop_range: Some(Loop {
                    start: 0,
                    end: FRAMES,
                    passes: None,
                    shape: LoopShape::Wrap,
                    mode: LoopMode::Continuous,
                }),
                ..Default::default()
            },
        })
        .collect();
    let mut plan = Prepared::new(48000, assets, regions, VOICES).unwrap();
    if workload == "eq-stereo" {
        use sampler_core::{
            ColorSettings, EqBand, Parameter, Processor, StereoSettings, VoiceChain,
        };
        let mut stages = Vec::new();
        for (frequency, gain) in [(0.3, 6.), (0.55, -4.), (0.7, 3.)] {
            let mut eq = EqBand::rbj(sampler_ir::fx::eq::Kind::Peak, frequency, 0.5, gain);
            eq.law = sampler_ir::fx::eq::Law::KontaktLegacy;
            stages.push(Processor::Eq(eq));
        }
        stages.push(Processor::Color(ColorSettings {
            stereo: Some(StereoSettings {
                width: Parameter::Constant(0.7),
                pan: Parameter::Constant(0.2),
                pseudo: false,
            }),
            ..ColorSettings::IDENTITY
        }));
        plan = plan
            .with_voice_chains(
                vec![VoiceChain::new(vec![], stages, 128).unwrap()],
                vec![Some(0); count],
            )
            .unwrap();
    }
    // Use the production pool budget, including its independent note/family
    // capacities. Generic core defaults only reserve 256 families.
    let (limits, ceiling) = crate::sound::v2::limits(&plan);
    let voice_state_bytes = plan.voice_state_bytes();
    let runtime = Runtime::new(plan, limits).unwrap();
    let runtime_bus_count = runtime.bus_count();
    assert_eq!(
        runtime_bus_count, 0,
        "this fixture installs no runtime bus graph"
    );
    let part = CorePart::new(runtime, MixTree::instrument("owned CPU receipt")).unwrap();
    let p = SamplerParams::new();
    p.shared.ensure_parts(1);
    let mut dsp = Dsp::default();
    dsp.core = V2Core::with_parts(1, 48000.);
    dsp.core.install(0, Some(Box::new(part)));
    let events_for = |kind| {
        let mut events = EventList::with_capacity(VOICES);
        for id in 0..VOICES {
            events
                .try_push_exact(ExactEvent::new(
                    0,
                    ExactEventBody::Note {
                        kind,
                        address: ExactNoteAddress::from_raw_signed(0, 0, 60, id as i32),
                        velocity: velocity(id),
                    },
                ))
                .unwrap();
        }
        events
    };
    let on = events_for(ExactNoteKind::On);
    let off = events_for(ExactNoteKind::Off);
    let empty = EventList::with_capacity(0);
    let mut output_events = EventList::with_capacity(VOICES * 2);
    let transport = TransportInfo::default();
    let (mut left, mut right) = (vec![0f32; block], vec![0f32; block]);
    let mut run = |events: &EventList, dsp: &mut Dsp| {
        let mut channels = [left.as_mut_slice(), right.as_mut_slice()];
        let mut buffer = AudioBuffer::from_slices_checked(&[], &mut channels, block);
        let mut cx = ProcessContext::new(&transport, 48000., block, &mut output_events);
        assert_eq!(
            Sampler::process(dsp, &p, &mut buffer, events, &mut cx),
            ProcessStatus::Normal
        );
        if !stage_audit {
            assert!(
                dsp.callback_audit.is_none(),
                "disabled audit must remain absent"
            );
        }
        let mut energy = 0.;
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for &sample in left.iter().chain(&right) {
            assert!(sample.is_finite());
            energy += f64::from(sample).powi(2);
            hash = (hash ^ u64::from(sample.to_bits())).wrapping_mul(0x100_0000_01b3);
        }
        (energy, hash)
    };
    if stage_audit {
        let _ = callback_thread_nanos(); // Resolve the diagnostic clock before the heap guard.
        dsp.callback_audit = Some(CallbackAudit::default());
    }
    let start = Instant::now();
    let admission_busy_before = p.shared.busy_ns.load(Ordering::Relaxed);
    assert_eq!(
        tests::allocations(|| {
            run(&on, &mut dsp);
        }),
        0
    );
    let admission_wall_ns = start.elapsed().as_nanos();
    let admission_busy_ns = p.shared.busy_ns.load(Ordering::Relaxed) - admission_busy_before;
    let admission_groups = dsp
        .callback_audit
        .take()
        .map_or(serde_json::Value::Null, |a| {
            a.receipt(admission_busy_ns, 1, block)
        });
    assert_eq!(dsp.core.voices().active, VOICES);
    for _ in 0..512 {
        run(&empty, &mut dsp);
    }
    assert_eq!(dsp.core.voices().audible, VOICES);
    assert_eq!(
        tests::allocations(|| {
            run(&empty, &mut dsp);
        }),
        0
    );
    if stage_audit {
        // Enable/reset existing test-only V2 stage clocks after setup/warmup.
        // JSON construction and state inspection occur outside process().
        let _ = dsp.core.onset_audit();
        dsp.callback_audit = Some(CallbackAudit::default());
    }
    let before_busy = p.shared.busy_ns.load(Ordering::Relaxed);
    let before_span = p.shared.span_ns.load(Ordering::Relaxed);
    let mut durations = Vec::with_capacity(blocks);
    let (mut energy, mut hash) = (0., 0_u64);
    for _ in 0..blocks {
        let busy = p.shared.busy_ns.load(Ordering::Relaxed);
        let (e, h) = run(&empty, &mut dsp);
        durations.push(p.shared.busy_ns.load(Ordering::Relaxed) - busy);
        energy += e;
        hash = hash.rotate_left(1) ^ h;
        assert_eq!(dsp.core.voices().active, VOICES);
        assert_eq!(dsp.core.voices().audible, VOICES);
    }
    let busy = p.shared.busy_ns.load(Ordering::Relaxed) - before_busy;
    let span = p.shared.span_ns.load(Ordering::Relaxed) - before_span;
    let callback_groups = dsp
        .callback_audit
        .take()
        .map_or(serde_json::Value::Null, |a| a.receipt(busy, blocks, block));
    let stages = if stage_audit {
        let raw = dsp.core.onset_audit();
        let values = raw["stage_ns"].as_array().expect("audit was enabled");
        let value = |i: usize| values[i].as_u64().unwrap();
        // Index 4 is a counter, not elapsed nanoseconds. The other six
        // intervals are disjoint inside V2::render_chunk; runtime_render_ns
        // is inclusive of source/modulation/FX/sum and its own telemetry.
        let timed = [0, 1, 2, 3, 5, 6].into_iter().map(value).sum::<u64>();
        assert!(timed <= busy);
        serde_json::json!({
            "scope":"test-only instrumented companion; clocks perturb busy/span; runtime stage is inclusive",
            "control_poll_ns":value(0), "script_controllers_ns":value(1),
            "optional_stream_service_ns":value(2), "runtime_render_ns":value(3),
            "stream_pending_observations":value(4), "persistence_ns":value(5),
            "optional_pre_stream_empty_render_ns":value(6),
            "timed_disjoint_sum_ns":timed,
            "callback_unattributed_including_probe_overhead_ns":busy - timed,
            "raw":raw,
        })
    } else {
        serde_json::Value::Null
    };
    let problems = dsp.core.problems(0);
    assert_eq!(problems, RuntimeProblems::default());
    assert!(energy > 0.);
    durations.sort_unstable();
    let budget = block as f64 * 1e9 / 48000.;
    let mut receipt = serde_json::json!({
        "scope":"Sampler::process; owned resident PCM; no exported host, UI painter or authored scripts",
        "debug_assertions":cfg!(debug_assertions),"callback_heap_guards_passed":true,
        "block":block,"blocks":blocks,"voices":VOICES,"sources":count,"workload":workload,
        "initial_voice_capacity":limits.voices,"voice_ceiling":ceiling,"note_capacity":limits.notes,
        "runtime_bus_count":runtime_bus_count,"prepared_voice_state_bytes":voice_state_bytes,
        "frontend_max_render_frames":MAX_BLOCK,"stage_audit_enabled":stage_audit,"v2_stages":stages,
        "admission_callback_groups":admission_groups,"steady_callback_groups":callback_groups,
        "admission_plus_observer_wall_ns":admission_wall_ns,
        "admission_callback_busy_ns":admission_busy_ns,
        "kontra_meter_unsmoothed_percent":100. * busy as f64 / span as f64,
        "busy_ns":busy,"span_ns":span,"p50_us":durations[blocks/2] as f64 / 1000.,
        "p99_us":durations[blocks*99/100] as f64 / 1000.,"max_us":durations[blocks-1] as f64 / 1000.,
        "deadline_misses":durations.iter().filter(|&&d| d as f64 > budget).count(),
        "pcm_energy":energy,"pcm_hash":format!("{hash:016x}"),"problems":problems
    });
    if stage_audit {
        dsp.callback_audit = Some(CallbackAudit::default());
    }
    let release_busy_before = p.shared.busy_ns.load(Ordering::Relaxed);
    let mut note_off_busy = 0;
    assert_eq!(
        tests::allocations(|| {
            run(&off, &mut dsp);
            note_off_busy = p.shared.busy_ns.load(Ordering::Relaxed) - release_busy_before;
            // Include tail completion and NOTE_END, not just the key-up callback.
            for _ in 0..32 {
                run(&empty, &mut dsp);
            }
        }),
        0
    );
    let release_busy = p.shared.busy_ns.load(Ordering::Relaxed) - release_busy_before;
    receipt["release_callback_groups"] = dsp
        .callback_audit
        .take()
        .map_or(serde_json::Value::Null, |a| {
            a.receipt(release_busy, 33, block)
        });
    receipt["note_off_callback_busy_ns"] = note_off_busy.into();
    receipt["release_phase_callbacks"] = 33.into();
    assert_eq!(dsp.core.voices().active, 0, "all exact host IDs release");
    assert_eq!(dsp.core.problems(0), RuntimeProblems::default());
    println!("WHOLE_CALLBACK_CPU {receipt}");
}
