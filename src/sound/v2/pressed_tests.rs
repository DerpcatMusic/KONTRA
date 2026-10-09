//! A pre-reset Lua wait must never resurrect a synthetic voice.
use super::*;

fn late_lua_play(hard_reset: Option<bool>) {
    let xml = "<UVI4><Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer/></Oscillators></Keygroup></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script>function onNote(e) wait(10); playNote(e.note,e.velocity,-1,1) end</script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let (host, _) = sampler_uvi::scripted::ScriptThread::spawn(
        xml.into(),
        (),
        sampler_uvi::script::Config::default(),
    )
    .unwrap();
    let pcm = Pcm::new(48000, vec![[0.1; 2]; 48000].into_boxed_slice()).unwrap();
    let plan = Prepared::new(
        48000,
        vec![pcm],
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
        128,
    )
    .unwrap()
    .with_groups(1, vec![Some(0)])
    .unwrap();
    let limits = Limits::for_plan(&plan, 16, 16);
    let mut part = Part::new(
        Runtime::new(plan, limits).unwrap(),
        MixTree::instrument("Lua reset"),
    )
    .unwrap();
    part.script = Some(Box::new(sampler_uvi::scripted::Driver::new(
        host,
        vec![sampler_uvi::OscGroup {
            layer: 1,
            osc: 1,
            group: 0,
            keygroup: 0,
            oscillator: 0,
        }],
        48000,
    )));
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(0, Some(Box::new(part)));
    core.event(
        0,
        Event::NoteOn {
            note: HostNote {
                port: 0,
                channel: 0,
                key: 60,
                id: 1,
                clap: true,
            },
            velocity: 1.,
            tune: 0.,
        },
    );
    core.render(64);
    std::thread::sleep(std::time::Duration::from_millis(5));
    if let Some(reset) = hard_reset {
        if reset {
            core.reset(48000.);
        } else {
            core.panic();
        }
        assert_eq!(core.voices().active, 0);
    }
    for _ in 0..128 {
        core.render(64);
        core.end_block(64, &mut |_| true);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    if hard_reset.is_none() {
        assert!(
            core.voices().active > 0,
            "calibrate that the deferred Lua play sounds without cancellation"
        );
        core.panic();
    }
    assert_eq!(
        core.voices().active,
        0,
        "pre-reset Lua events/waits must not restart sound"
    );
    assert_eq!(core.problems(0).lua_faults, 0);
}

#[test]
fn hard_reset_cancels_deferred_lua_generated_notes() {
    late_lua_play(Some(true));
}
#[test]
fn explicit_panic_cancels_deferred_lua_generated_notes() {
    late_lua_play(Some(false));
}

#[test]
fn deferred_lua_fixture_really_generates_a_voice() {
    late_lua_play(None);
}
