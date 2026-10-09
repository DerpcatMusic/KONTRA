//! Original synthetic PCM verifies Lua note edits through the shared core API.
use sampler_core::{Envelope, Input, Limits, Pcm, Playback, Prepared, Protocol, Region, Runtime};
use sampler_uvi::{
    OscGroup,
    script::{Config, ScriptHost},
    scripted::Driver,
};

fn play(body: &str) -> (Vec<[f32; 2]>, usize) {
    let samples =
        [[1., 0.], [0., 1.]].map(|f| Pcm::new(48000, vec![f; 4096].into_boxed_slice()).unwrap());
    let regions = (0..2)
        .map(|sample| Region {
            sample,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        })
        .collect();
    // Deliberately permute the runtime group identities: layer != group index.
    let plan = Prepared::new(48000, samples.into(), regions, 2)
        .unwrap()
        .with_groups(2, vec![Some(1), Some(0)])
        .unwrap();
    let limits = Limits::for_plan(&plan, 4, 4);
    let mut rt = Runtime::new(plan, limits).unwrap();
    let xml = format!(
        "<UVI4><Program><Layers><Layer/><Layer/></Layers><EventProcessors><ScriptProcessor><script>function onNote(e) postEvent(e); {body} end</script></ScriptProcessor></EventProcessors></Program></UVI4>"
    );
    let host = ScriptHost::new(&xml, (), Config::default()).unwrap();
    let groups = (0..2)
        .map(|n| OscGroup {
            layer: n + 1,
            osc: 1,
            group: 1 - n,
            keygroup: n as usize,
            oscillator: n as usize,
        })
        .collect();
    let mut driver = Driver::new(host, groups, 48000);
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    };
    let note = rt.note_on(input, 60, 1.).unwrap();
    driver.note_on(&mut rt, note, 60, 1.).unwrap();
    let mut audio = vec![[0.; 2]; 128];
    rt.render(&mut audio).unwrap();
    (audio, rt.voice_count())
}

#[test]
fn lua_immediate_edits_and_layer_fades_reach_the_addressed_runtime_voices() {
    let (immediate, _) = play("changeVolume(e.id,0.1,false,true)");
    let (smooth, _) = play("changeVolume(e.id,0.1,false,false)");
    assert!(immediate[0][0] < 0.11 && smooth[0][0] > 0.9);
    assert!((immediate[63][0] - smooth[63][0]).abs() < 1e-5);
    let (fade, voices) = play("fadeout(e.id,64/48,true,false,1)");
    assert!(fade[0][0] > 0.9);
    assert!(fade[64..].iter().all(|f| *f == [0., 1.]));
    assert_eq!(voices, 1);
}
