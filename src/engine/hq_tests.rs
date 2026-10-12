use super::*;

include!("hq_native_pcm.rs");

fn source(i: usize) -> Frame { [(((i * 73) % 257) as f32 - 128.) / 1024.; 2] }

#[test]
fn native_convolution_matches_captured_high_ultra_boundary_and_wide_points() {
    // Literal output of pinned native 0x14086eb50, real 0x140828350 and SIMD
    // dot product. Only raw source fetching/cache residency were supplied by
    // the analysis harness; negative/EOF tap gates ran in the native code.
    let rows = [
        (1,0,0.25,0.25,0xbe01691f),(1,0,0.5,1.25,0xbdcd54f2),
        (1,1,0.75,2.3,0xbc8bcc8f),(1,32,0.25,64.25,0x3a9a5b77),
        (1,128,0.75,1.25,0x3cf8a2ea),(1,4000,0.75,16.25,0xbc0173b3),
        (1,4095,0.75,1.25,0xbd1fd53d),
        (2,0,0.25,0.25,0xbe0057b5),(2,0,0.5,1.25,0xbde3d36f),
        (2,1,0.75,2.3,0xbc5e08b2),(2,32,0.25,64.25,0x3aaf6e59),
        (2,128,0.75,1.25,0x3d361707),(2,4000,0.75,16.25,0xbbe78894),
        (2,4095,0.75,1.25,0xbd266844),
    ];
    for (quality, floor, fraction, step, expected) in rows {
        let actual = sample(quality, Point { floor, fraction, step, resample:true, ended:false, domain:None }, |first, frames| {
            for (i, frame) in frames.iter_mut().enumerate() {
                let at = first + i as i64;
                *frame = if (0..4096).contains(&at) { source(at as usize) } else { [0.; 2] };
            }
            Ok(())
        }).unwrap();
        assert_eq!(actual.map(f32::to_bits), [expected; 2], "quality{quality} floor{floor} frac{fraction} step{step}");
    }
}

#[test]
fn native_float_distance_keeps_repeated_add_rounding_without_a_tap_footprint_limit() {
    for scale in [128.,64.,128./2.3,128./63.75,128./513.,128./100000.,1e-20,1e-40] {
        for origin in [0.,0.731 * scale,12.3,255.999,1023.999] {
            let mut expected = origin;
            for n in 0..=20000 {
                if [0,1,2,3,7,15,31,127,511,2047,8191,20000].contains(&n) {
                    // With no table endpoint reached, a stalled exact walk is
                    // reported rather than using a lower-quality kernel.
                    match distance(origin, scale, n, f32::MAX) {
                        Ok(actual) => assert_eq!(actual.to_bits(), expected.to_bits()),
                        Err(StreamErrorKind::UnrepresentablePitch) => {
                            let mut slow = origin;
                            assert!((0..n).any(|_| { let next=slow+scale; let stuck=next<=slow;slow=next;stuck }));
                        }
                        Err(other) => panic!("unexpected source failure {other:?}"),
                    }
                }
                expected += scale;
            }
        }
    }
}

#[test]
fn native_high_ultra_complete_pcm_matches_independently_captured_native_source_in_ram_stream_and_filter_lanes() {
    use crate::{audio::Sample, engine::{Bank, Engine, envelope::Ahdsr, instrument::ControllerPolicy},
        import::{Group, Instrument, Zone}, fx::{Chain, Effect, Kind, Params, params::Filter}};
    const N: usize = 256;
    let dir = tempfile::tempdir().unwrap();
    // Exact binary DC survives the authored lowpass at both source ratios.
    // Capture this source independently; do not normalize table phase gain.
    let original: Vec<_> = (0..65536).map(|i| source(i).map(|v| v + 1. / 32.)).collect();
    for (quality, rate, expected) in [
        (1,60000,&NATIVE_1_NORMAL),(1,3084000,&NATIVE_1_WIDE),
        (2,60000,&NATIVE_2_NORMAL),(2,3084000,&NATIVE_2_WIDE),
    ] {
        let path = dir.path().join(format!("native-hq-{quality}-{rate}.wav"));
        let mut wav = hound::WavWriter::create(&path, hound::WavSpec { channels:2,
            sample_rate:rate, bits_per_sample:32, sample_format:hound::SampleFormat::Float }).unwrap();
        for frame in &original { for &channel in frame { wav.write_sample(channel).unwrap(); } }
        wav.finalize().unwrap();
        let mut reference = vec![[0.; 2]; 65536];
        for (frame, &bits) in reference.iter_mut().zip(expected) { *frame=[f32::from_bits(bits); 2]; }
        for filtered in [false,true] {
            let mut group = Group { interp_quality:quality, ..Default::default() };
            if filtered {
                group.fx = Chain { slots:vec![Effect { slot:0,kind:Kind::Filter,version:0,bypass:false,
                    output_gain:1.,dry_level:0.,params:Params::Filter(Filter { filter_type:2,
                        cutoff:0.2,resonance:0.1,extra:[0.;3],native_flag:None }) }] };
            }
            let make = |kind| {
                let mut bank = if kind == 4 {
                    let mut group = group.clone(); group.interp_quality=0;
                    Bank::from_samples(vec![group],vec![Zone::default()],
                        vec![(Default::default(),Sample { rate:48000,frames:reference.clone() })]).unwrap()
                } else if kind >= 2 {
                    Bank::load_bare(&Instrument { groups:vec![group.clone()],
                        sampling_profile: crate::import::SamplingProfile::Kontakt,
                        zones:vec![Zone { sample:path.clone(), ..Default::default() }], ..Default::default() }).unwrap()
                } else {
                    Bank::from_samples_with_profile(vec![group.clone()],vec![Zone { sample:path.clone(), ..Default::default() }],
                        vec![(path.clone(),Sample { rate,frames:original.clone() })],crate::import::SamplingProfile::Kontakt).unwrap()
                }.with_prepared_settings(|rows| rows[0].envelope=Some(Ahdsr::UNITY)).unwrap();
                bank.initial_state.as_mut().unwrap().controller_policy=ControllerPolicy::Native;
                let mut engine=Engine::default();engine.reset(48000.);engine.attack=0.;engine.blocking_streams=true;
                engine.set_lanes(kind != 1 && kind != 3);engine.set_bank(Some(Box::new(bank)));
                for _ in 0..4 { engine.note_on(0,60,127); }
                engine
            };
            let mut engines=[make(0),make(1),make(2),make(3),make(4)];
            let mut pcm=[[[0.; N]; 2]; 5];
            assert_eq!(crate::plugin::tests::allocations(|| {
                for (index, engine) in engines.iter_mut().enumerate() {
                    let mut at=0;let mut block=0;
                    while at<N {
                        let n=if index%2==0 { 128 } else { [1,7,31,127][block%4] }.min(N-at);
                        let [left,right]=&mut pcm[index];
                        engine.render_checked(&mut left[at..at+n],&mut right[at..at+n]).unwrap();
                        at+=n;block+=1;
                    }
                    assert_eq!(engine.underruns(),0);
                }
            }),0);
            let reference_peak=pcm[4].iter().flatten().map(|v|v.abs()).fold(0.,f32::max);
            assert!(reference_peak>0.001,"captured reference quality{quality} rate{rate} filter{filtered}: peak{reference_peak}");
            for index in 0..4 {
                let error=pcm[index].iter().flatten().zip(pcm[4].iter().flatten())
                    .map(|(a,b)|(a-b).abs()).fold(0.,f32::max);
                assert!(error<2e-6,"native captured PCM quality{quality} rate{rate} filter{filtered} engine{index}: {error}");
            }
        }
    }
}
