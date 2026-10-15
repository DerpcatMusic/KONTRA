use super::*;

fn instrument(reverse: bool, range: u64, active: bool) -> ir::Instrument {
    let mut zone = ir::Zone::new(ir::AssetRef(0));
    zone.keys = ir::KeyRange { low: 60, high: 60 };
    zone.pitch = ir::KeyTracking::Fixed;
    zone.playback = ir::Playback {
        start: 100,
        end: Some(2000),
        reverse,
        start_range: range,
        ..Default::default()
    };
    if active {
        zone.routes.push(ir::RouteRef(0));
    }
    ir::Instrument {
        assets: vec![ir::Asset {
            location: ir::AssetLocation::Path("numeric-fixture".into()),
            encoding: ir::Encoding::Unknown,
            residency: ir::AssetResidency::Streamable,
            root_key: None,
            loops: vec![],
        }],
        zones: vec![zone],
        modulators: vec![ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Constant,
        }],
        routes: vec![ir::Route {
            source: ir::ModulatorRef(0),
            target: ir::Target::SampleStart,
            depth: ir::Depth::Normalized(1.),
            invert: false,
            shape: None,
            smoothing: ir::Time::Milliseconds(0.),
            scale: None,
        }],
        ..Default::default()
    }
}

#[test]
fn planned_heads_cover_every_admitted_sample_start_offset_in_both_directions() {
    let pcm = Pcm::streamed(48000, 2048).unwrap();
    let policy = StreamPolicy {
        headroom: 0.,
        max_step: 1.,
        ..Default::default()
    };
    for reverse in [false, true] {
        for range in [0, 32, 512, u64::MAX] {
            let ir = instrument(reverse, range, true);
            let planned = start_ranges(&ir, std::slice::from_ref(&pcm), 48000, 64, &policy);
            for offset in 0..=range.min(1900) {
                let onset = if reverse { 1999 - offset } else { 100 + offset };
                if onset >= 2000 {
                    continue;
                }
                assert!(
                    planned[0].iter().any(|r| r.contains(&(onset as usize))),
                    "reverse={reverse} offset={offset} ranges={planned:?}"
                );
            }
            assert!(planned[0].iter().all(|r| r.start < r.end && r.end <= 2048));
        }
        let base = start_ranges(
            &instrument(reverse, 0, false),
            std::slice::from_ref(&pcm),
            48000,
            64,
            &policy,
        );
        let unused = start_ranges(
            &instrument(reverse, u64::MAX, false),
            std::slice::from_ref(&pcm),
            48000,
            64,
            &policy,
        );
        assert_eq!(
            base, unused,
            "unused modulation extent must not increase residency"
        );
    }
}

#[test]
fn actual_streamed_modulated_start_matches_resident_without_waiting_for_tail_pages() {
    struct Source;
    impl AssetSource for Source {
        fn header(&self) -> Option<(u32, usize)> {
            Some((48000, 2048))
        }
        fn open(&self) -> io::Result<SampleReader> {
            Ok(SampleReader::custom(48000, 2048, |start, out| {
                for (i, frame) in out.iter_mut().enumerate() {
                    *frame = [(start + i) as f32 / 2048., -0.25];
                }
                Ok(())
            }))
        }
    }
    for reverse in [false, true] {
        let ir = instrument(reverse, 512, true);
        let pcm = Pcm::new(
            48000,
            (0..2048).map(|i| [i as f32 / 2048., -0.25]).collect(),
        )
        .unwrap();
        let resident =
            sampler_core::lower::lower(&ir, 48000, vec![pcm], |_, plan| Ok(plan)).unwrap();
        let streamed = crate::stream_instrument(
            ir,
            vec![Arc::new(Source)],
            vec!["numeric-fixture".into()],
            &crate::Options {
                rate: 48000,
                ..Default::default()
            },
            &StreamPolicy {
                headroom: 0.,
                max_step: 1.,
                voices: 2,
                decoders: 1,
                head_budget: 32768,
                slack: Duration::ZERO,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            streamed
                .streamer
                .head_readiness(&streamed.assets)
                .complete()
        );
        let limits = sampler_core::Limits::for_plan(&resident, 4, 4);
        let mut expected = sampler_core::Runtime::new(resident, limits).unwrap();
        let mut actual = sampler_core::Runtime::new(streamed.loaded.plan, limits)
            .unwrap()
            .with_stream_cache(streamed.cache);
        actual.set_cold_starts(true);
        let input = sampler_core::Input {
            protocol: sampler_core::Protocol::Clap,
            port: 0,
            group: 0,
            channel: 0,
            key: 60,
            external_id: Some(1),
        };
        expected.trigger(input, 60, 1.).unwrap();
        actual.trigger(input, 60, 1.).unwrap();
        let mut a = [[0.; 2]; 64];
        let mut b = a;
        expected.render(&mut a).unwrap();
        actual.render(&mut b).unwrap();
        assert_eq!(a, b);
        assert!(a.iter().any(|f| f[0] != 0.));
        assert_eq!(actual.stats().cold_starts, 0);
        assert_eq!(actual.stream_underruns(), 0);
    }
}
