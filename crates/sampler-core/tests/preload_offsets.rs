use sampler_core::*;

#[test]
fn preload_reach_uses_initialized_controllers_and_preserves_per_region_frame_scales() {
    let pcm = Pcm::new(48000,vec![[0.;2];20000].into_boxed_slice()).unwrap();
    let region = Region { sample:0,key_low:60,key_high:72,root_key:None,velocity_low:0.,velocity_high:1.,
        gain:1.,envelope:Envelope::new(0,0,0,1.,0).unwrap(),playback:Playback::default() };
    let program = ModProgram { sources:vec![ModSource::Controller(113)],
        routes:vec![ModRoute::new(0,ModTarget::SampleStart,1.)],..Default::default() };
    let plan = Prepared::new(48000,vec![pcm],vec![region,region],26).unwrap()
        .with_voice_modulation(vec![program],vec![Some(0),Some(0)],vec![12000,6000]).unwrap()
        .with_initial_controllers(&[(113,127)]);
    assert_eq!(plan.preload_start_offsets(),vec![(12000,12000),(6000,6000)]);
}

#[test]
fn a_nonmonotonic_key_shape_is_swept_but_unpinned_script_offsets_keep_their_full_reach() {
    let pcm = Pcm::new(48000,vec![[0.;2];20000].into_boxed_slice()).unwrap();
    let region = Region { sample:0,key_low:60,key_high:72,root_key:None,velocity_low:0.,velocity_high:1.,
        gain:1.,envelope:Envelope::new(0,0,0,1.,0).unwrap(),playback:Playback::default() };
    let mut key = ModRoute::new(0,ModTarget::SampleStart,1.);key.shape=Some(0);
    let key = ModProgram { sources:vec![ModSource::Key],routes:vec![key],
        shapes:vec![vec![(0.,0.),(60./127.,0.),(66./127.,1.),(72./127.,0.),(1.,0.)]],..Default::default() };
    let script = ModProgram { sources:vec![ModSource::Script(1)],
        routes:vec![ModRoute::new(0,ModTarget::SampleStart,1.)],..Default::default() };
    let plan = Prepared::new(48000,vec![pcm],vec![region,region],26).unwrap()
        .with_voice_modulation(vec![key,script],vec![Some(0),Some(1)],vec![12000,6000]).unwrap();
    let reach=plan.preload_start_offsets();
    assert_eq!(reach[0].0,0);
    assert!((11999..=12000).contains(&reach[0].1));
    assert_eq!(reach[1],(0,6000));
}
