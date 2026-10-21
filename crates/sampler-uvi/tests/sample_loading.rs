//! Authored scripts and the real decode/transaction/commit/callback path.
use sampler_core::{
    Envelope, Input, Limits, Pcm, Playback, Prepared, Protocol, Region, Runtime,
    SampleRevisionLimits, SampleTarget, SampleTargetId,
};
use sampler_uvi::script::{Config, Files, SampleResource, SampleSource, ScriptHost};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
struct Sources;
impl Files for Sources {
    fn script(&self, _: &str) -> Option<String> {
        None
    }
    fn sample_source(&self) -> Option<SampleSource> {
        Some(Arc::new(|request, _explicit| {
            let path = request.path;
            if path == "missing.wav" {
                return Err("authored sample is missing".into());
            }
            struct Source(f32);
            impl sampler_kontakt::AssetSource for Source {
                fn open(&self) -> std::io::Result<sampler_kontakt::SampleReader> {
                    let v = self.0;
                    Ok(sampler_kontakt::SampleReader::custom(
                        48000,
                        256,
                        move |_, out| {
                            out.fill([v; 2]);
                            Ok(())
                        },
                    ))
                }
            }
            Ok(SampleResource {
                source: Arc::new(Source(if path == "second.wav" { 0.75 } else { 0.25 })),
                reader: None,
                playback: Playback::default(),
                base_note: 42,
                fine_tune: -7.,
                geometry_error: None,
                format_code: 0,
                channels: 1,
                bits: 16,
                tempo: 0.,
                timings: Default::default(),
            })
        }))
    }
}
fn host(script: &str) -> (ScriptHost, Runtime) {
    host_with_files(script, Sources)
}
fn host_with_files(script: &str, files: impl Files + 'static) -> (ScriptHost, Runtime) {
    let xml = format!(
        "<UVI4><Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer BaseNote='60'/></Oscillators></Keygroup></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    );
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let target = doc
        .descendants()
        .find(|n| n.has_tag_name("SamplePlayer"))
        .unwrap()
        .id()
        .get_usize() as u32;
    let plan = Prepared::new(
        48000,
        vec![Pcm::new(48000, vec![[0.1; 2]; 256].into()).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: Some(60),
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::new(0, 0, 0, 1., 1).unwrap(),
            playback: Playback::default(),
        }],
        1,
    )
    .unwrap()
    .with_sample_targets(
        vec![SampleTarget {
            id: SampleTargetId(target),
            regions: vec![0].into(),
        }],
        SampleRevisionLimits {
            slots: 4,
            resident_bytes: 1 << 20,
        },
    )
    .unwrap();
    let lim = Limits::for_plan(&plan, 8, 8);
    let (rt, control) = Runtime::new(plan, lim)
        .unwrap()
        .with_sample_updates(4)
        .unwrap();
    let mut host = ScriptHost::new(&xml, files, Config::default()).unwrap();
    host.bind_sample_control(control).unwrap();
    (host, rt)
}
fn input(id: i32) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(id),
    }
}
fn pump(host: &mut ScriptHost, rt: &mut Runtime, done: &str) {
    let until = Instant::now() + Duration::from_secs(2);
    let mut ms = 0.;
    while host.global_text(done) != "true" {
        host.advance(ms);
        rt.poll_sample_update().unwrap();
        host.advance(ms);
        ms += 1.;
        assert!(
            Instant::now() < until,
            "sample task timed out; {:?}; {:?}",
            host.fault_counts(),
            host.findings()
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
#[test]
fn callback_and_poll_share_identity_and_success_requires_actual_commit() {
    let (mut host, mut rt) = host(
        r#"
      local osc=Program.layers[1].keygroups[1].oscillators[1]
      calls=0
      function onInit()
        task=loadSample(osc,'first.wav',function(t)
          assert(rawequal(t,task));assert(t.success and t.finished);calls=calls+1
          local info=osc.sampleInfo
          assert(info.basenote==42 and info.finetune==-7 and info.samplerate==48000)
          assert(info.duration==256*1000/48000 and info.channels==1 and info.bits==16)
          assert(osc:getParameter('BaseNote')==60)
          completed=true
        end)
        assert(not task.finished and not task.success)
      end
    "#,
    );
    assert_eq!(host.global_text("completed"), "nil");
    assert!(
        host.fault_counts().init.is_empty(),
        "{:?}",
        host.fault_counts()
    );
    // Decoding and owner polling alone cannot claim successful audio publication.
    for ms in 0..5 {
        host.advance(ms as f64);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(host.global_text("calls"), "0");
    pump(&mut host, &mut rt, "completed");
    assert_eq!(host.global_text("calls"), "1");
    rt.trigger(input(1), 60, 1.).unwrap();
    let mut out = [[0.; 2]; 4];
    rt.render(&mut out).unwrap();
    assert_eq!(out, [[0.25; 2]; 4]);
    host.advance(20.);
    assert_eq!(host.global_text("calls"), "1");
    assert!(
        host.fault_counts().runtime.is_empty(),
        "{:?}",
        host.fault_counts()
    );
}
#[test]
fn no_callback_still_loads_and_missing_cancelled_tasks_never_replace_source() {
    let (mut host, mut rt) = host(
        r#"
      local osc=Program.layers[1].keygroups[1].oscillators[1]
      function onInit()
        assert(loadSample(osc,'second.wav')==nil)
        spawn(function() while osc.sampleInfo==nil do wait(1) end;ready=true end)
      end
      function onNote(e)
        failed=loadSample(osc,'missing.wav',function(t)
          assert(t.finished and not t.success);failureDone=true
          cancelled=loadSample(osc,'first.wav',function(t) assert(t.finished and not t.success);cancelDone=true end)
          cancelled:cancel()
        end)
      end
    "#,
    );
    pump(&mut host, &mut rt, "ready");
    rt.trigger(input(1), 60, 1.).unwrap();
    let mut out = [[0.; 2]; 2];
    rt.render(&mut out).unwrap();
    assert_eq!(out, [[0.75; 2]; 2]);
    host.note_on(1, 60, 100, 0);
    pump(&mut host, &mut rt, "cancelDone");
    pump(&mut host, &mut rt, "failureDone");
    rt.render(&mut out).unwrap();
    assert_eq!(out, [[0.75; 2]; 2]);
    assert!(
        host.fault_counts().runtime.is_empty(),
        "{:?}",
        host.fault_counts()
    );
}

#[test]
fn nested_lua_module_keeps_raw_sample_path_and_exact_script_context_until_worker_resolution() {
    struct ContextSources;
    impl Files for ContextSources {
        fn script(&self, name: &str) -> Option<String> {
            (name=="loader").then(|| "function requestSample(osc) loadSample(osc,'./../Samples/tone.wav',function(t) assert(t.success); done=true end) end".into())
        }
        fn script_path(&self, _: &str) -> Option<String> { Some("Scripts/Engine/loader.lua".into()) }
        fn sample_source(&self) -> Option<SampleSource> {
            let base = Sources.sample_source().unwrap();
            Some(Arc::new(move |request, explicit| {
                assert_eq!(request.path, "./../Samples/tone.wav", "retain caller path for fallback contexts");
                assert_eq!(request.script_directory, Some("Scripts/Engine"));
                base(sampler_uvi::script::SamplePath { path:"first.wav", script_directory:None }, explicit)
            }))
        }
    }
    let (mut h, mut rt) = host_with_files("require('loader'); function onInit() requestSample(Program.layers[1].keygroups[1].oscillators[1]) end", ContextSources);
    pump(&mut h, &mut rt, "done");
    let status = h.sample_load_status();
    assert_eq!((status.requested,status.successful,status.failed,status.pending), (1,1,0,0));
    for stage in [status.timings.metadata_queue,status.timings.resolve_and_headers,status.timings.prepare_decode,status.timings.worker_delivery,status.timings.ready_submit,status.timings.commit_ack,status.timings.total] {
        assert_eq!(stage.count,1);
        assert_eq!(stage.histogram.iter().sum::<u64>(),1);
    }
}
