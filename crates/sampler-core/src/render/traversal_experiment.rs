//! Authored source-selection fixture for bounded render traversal experiments.
//! No clocks or candidate fast path; the actual callback owns meter acceptance.
use crate::{Input, Limits, Pcm, Prepared, Protocol, Runtime};
use sampler_ir as ir;

const VOICES: usize = 300;
const FRAMES: usize = 4096;

fn authored_plan() -> Prepared {
    let instrument = ir::Instrument {
        name: "authored 300 distinct resident sources".into(),
        assets: (0..VOICES)
            .map(|id| ir::Asset {
                location: ir::AssetLocation::Path(format!("owned-{id}.wav")),
                encoding: ir::Encoding::Wav,
                residency: ir::AssetResidency::Resident,
                root_key: None,
                loops: vec![],
            })
            .collect(),
        zones: (0..VOICES)
            .map(|id| {
                let key = (id % 100) as u8;
                let layer = (id / 100) as u8;
                ir::Zone {
                    keys: ir::KeyRange {
                        low: key,
                        high: key,
                    },
                    velocities: ir::VelocityRange {
                        low: 1 + 42 * layer,
                        high: if layer == 2 { 127 } else { 42 * (layer + 1) },
                    },
                    pitch: ir::KeyTracking::Fixed,
                    velocity: ir::VelocityResponse::None,
                    gain: ir::Gain::Linear(0.008),
                    playback: ir::Playback {
                        looping: ir::Looping::Continuous(ir::LoopRange {
                            start: 0,
                            end: FRAMES as u64,
                            crossfade: ir::Span::ZERO,
                            alternating: false,
                        }),
                        ..Default::default()
                    },
                    ..ir::Zone::new(ir::AssetRef(id))
                }
            })
            .collect(),
        ..Default::default()
    };
    let pcm = (0..VOICES)
        .map(|asset| {
            Pcm::new(
                48000,
                (0..FRAMES)
                    .map(|i| {
                        [
                            (i as f32 * (0.013 + asset as f32 * 0.0001)).sin() * 0.2,
                            (i as f32 * (0.017 + asset as f32 * 0.00007)).cos() * 0.15,
                        ]
                    })
                    .collect(),
            )
            .unwrap()
        })
        .collect();
    crate::lower::lower(&instrument, 48000, pcm, |_, _| {
        unreachable!("fixture declares no behaviors")
    })
    .unwrap()
}

fn input(id: usize) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: (id % 100) as u8,
        external_id: Some(id as i32),
    }
}

fn sounding_runtime() -> Runtime {
    let plan = authored_plan();
    assert_eq!(plan.pcm.len(), VOICES);
    let assets: std::collections::HashSet<_> = plan.pcm.iter().map(Pcm::asset_id).collect();
    assert_eq!(assets.len(), VOICES, "independent immutable source assets");
    let mut runtime = Runtime::new(
        plan,
        Limits {
            notes: VOICES,
            families: VOICES,
            expressions: VOICES,
            voices: VOICES,
            channels: 0,
            performances: 1,
            decisions: VOICES,
            commands: VOICES,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap();
    for id in 0..VOICES {
        let velocity = f64::from(17 + 42 * (id / 100) as u8) / 127.;
        runtime.trigger(input(id), input(id).key, velocity).unwrap();
    }
    assert_eq!(runtime.voice_count(), VOICES);
    assert_eq!(runtime.note_count(), VOICES);
    let selected: std::collections::HashSet<_> = runtime
        .voices
        .slots
        .iter()
        .filter_map(|slot| slot.value.as_ref().map(|voice| voice.sample))
        .collect();
    assert_eq!(
        selected.len(),
        VOICES,
        "each exact host owner selects its own authored source"
    );
    runtime
}

#[test]
fn authored_300_distinct_sources_keep_pcm_cursor_envelope_and_exact_release() {
    let mut whole = sounding_runtime();
    let mut fragmented = sounding_runtime();
    let mut expected = [[0.; 2]; 256];
    let mut actual = [[0.; 2]; 256];
    let mut energy = 0.;
    for _ in 0..40 {
        whole.render(&mut expected).unwrap();
        let mut offset = 0;
        // Deliberately cross source wrap boundaries and non-control-grid host fragments.
        for len in [1, 7, 64, 65, 119] {
            fragmented
                .render(&mut actual[offset..offset + len])
                .unwrap();
            offset += len;
        }
        assert_eq!(
            actual.map(|frame| frame.map(f32::to_bits)),
            expected.map(|frame| frame.map(f32::to_bits))
        );
        assert_eq!(whole.voice_count(), VOICES);
        assert_eq!(whole.audible_voice_count(), VOICES);
        for (a, b) in whole
            .voices
            .slots
            .iter()
            .zip(fragmented.voices.slots.iter())
        {
            let (a, b) = (a.value.as_ref().unwrap(), b.value.as_ref().unwrap());
            assert_eq!(format!("{:?}", a.cursor), format!("{:?}", b.cursor));
            assert_eq!(format!("{:?}", a.envelope), format!("{:?}", b.envelope));
        }
        for sample in actual.iter().flatten() {
            assert!(sample.is_finite());
            energy += f64::from(*sample).powi(2);
        }
    }
    assert!(energy > 0.);
    for id in 0..VOICES {
        whole.note_off(input(id), None).unwrap();
        fragmented.note_off(input(id), None).unwrap();
    }
    whole.render(&mut expected).unwrap();
    fragmented.render(&mut actual).unwrap();
    assert_eq!(actual, [[0.; 2]; 256]);
    assert_eq!(actual, expected);
    for runtime in [&whole, &fragmented] {
        assert_eq!(runtime.voice_count(), 0);
        let stats = runtime.stats();
        assert_eq!(stats.voice_drops, 0);
        assert_eq!(stats.stream_underruns, 0);
        assert_eq!(stats.nonfinite_frames, 0);
    }
}
