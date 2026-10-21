mod support;
use sampler_core::{Error, HeadBudget, Pcm};

#[test]
fn resident_copy_is_bit_exact_for_every_packed_width_and_never_partially_writes() {
    for (scale, stereo) in [(32768.,false),(32768.,true),(8388608.,false),(8388608.,true),(7.,false),(7.,true)] {
        let frames: Vec<_> = (0..64).map(|i| [(i as f32-32.)/scale, if stereo {(31.-i as f32)/scale} else {(i as f32-32.)/scale}]).collect();
        let pcm = Pcm::streamed(48000, 128).unwrap();
        pcm.set_ranges(vec![(4,frames[..32].into()),(36,frames[32..].into())]).unwrap();
        let mut out = [[f32::NAN;2];64];
        support::without_heap(|| pcm.copy_resident_range(4,&mut out).unwrap());
        for (got,expected) in out.iter().zip(&frames) { assert_eq!(got.map(f32::to_bits),expected.map(f32::to_bits)); }
        for (start,len,error) in [(3,2,Error::NotReady),(66,3,Error::NotReady),(128,1,Error::InvalidInput),(usize::MAX,2,Error::InvalidInput)] {
            let mut out = vec![[123.;2];len];
            support::without_heap(|| assert_eq!(pcm.copy_resident_range(start,&mut out),Err(error)));
            assert_eq!(out,vec![[123.;2];len]);
        }
        pcm.copy_resident_range(128,&mut []).unwrap();
    }
    let full=Pcm::new(44100,vec![[0.25,-0.5];64].into()).unwrap();
    let mut out=[[0.;2];32]; full.copy_resident_range(7,&mut out).unwrap();
    assert_eq!(out,[[0.25,-0.5];32]);
}

#[test]
fn resident_copy_pins_one_generation_and_old_guards_keep_physical_leases() {
    let budget=HeadBudget::new(64*1024);
    let pcm=Pcm::streamed(48000,64).unwrap();
    pcm.bind_head_budget(&budget).unwrap();
    let mut builder=pcm.head_preparer().builder(); builder.push(0,&[[0.25;2];64]).unwrap();
    pcm.publish_ranges(builder.finish()).unwrap();
    let before=budget.status().held_bytes;
    let mut builder=pcm.head_preparer().builder(); builder.push(0,&[[0.5;2];64]).unwrap();
    let old=pcm.publish_ranges(builder.finish()).unwrap();
    assert!(budget.status().held_bytes>before);
    let mut out=[[0.;2];64]; pcm.copy_resident_range(0,&mut out).unwrap(); assert_eq!(out,[[0.5;2];64]);
    assert!(!old.is_empty(),"old generation remains pinned and immutable");
    drop(old);
    let writer=pcm.clone();
    let handle=std::thread::spawn(move|| {
        for i in 0..64 { writer.set_ranges(vec![(0,vec![[if i%2==0 {0.25}else{0.5};2];64].into())]).unwrap(); }
    });
    for _ in 0..256 {
        support::without_heap(|| pcm.copy_resident_range(0,&mut out).unwrap());
        assert!(out[0]==[0.25;2]||out[0]==[0.5;2]);
        assert!(out.iter().all(|f|f.map(f32::to_bits)==out[0].map(f32::to_bits)),"copy must not mix publications");
    }
    handle.join().unwrap();
}
