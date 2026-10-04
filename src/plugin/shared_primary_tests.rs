//! Real plugin ingress, with separately authored cached points baked into dry samples.
use super::*;
use crate::{
    audio::Sample,
    import::{Ahdsr, Group, Zone},
};
use moose::params::Params;
use ni_file::kontakt::{Chunk, StructuredObject, objects::Group as RawGroup};

#[path = "shared_primary_mpe_tests.rs"]
mod shared_primary_mpe_tests;

const FINISH: usize = 2561;
// Identical caller-owned buffers in both runs. The 1025-frame buffer is
// independently known to contain balanced native spans of 513 and 512.
const HOST: [usize; 14] = [
    0, 128, 256, 384, 512, 640, 768, 896, 1024, 1152, 1280, 1408, 2433, FINISH,
];
const ON: [usize; 7] = [0, 17, 512, 768, 785, 1152, 1408];
const END: [usize; 7] = [256, 384, 640, 1024, 896, 1408, 2433];
const CHANNEL: [u8; 7] = [4, 7, 4, 4, 7, 7, 4];
const KEY: [u8; 7] = [60, 60, 60, 60, 62, 62, 60];

fn object(id: u16, version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Chunk {
    let mut data = vec![1];
    data.extend(version.to_le_bytes());
    for bytes in [private, public, children] {
        data.extend((bytes.len() as u32).to_le_bytes());
        data.extend(bytes);
    }
    Chunk { id, data }
}
fn name(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend((value.len() as u32).to_le_bytes());
    bytes.extend(value.as_bytes());
}
fn saved(shared: bool, bypass: bool) -> Group {
    saved_flags(shared, bypass, false)
}
fn saved_flags(shared: bool, bypass: bool, router_open: bool) -> Group {
    let mut source = Vec::new();
    Ahdsr {
        attack_curve: 1.,
        attack_ms: 0.,
        hold_ms: 0.,
        decay_ms: 0.,
        sustain: 1.,
        release_ms: 2.,
        unknown_flag: 0,
        unknown_tail: [0, 0, 128, 191, 0, 0, 0, 0, 0, 0, 128, 63, 0].repeat(4),
    }
    .write(&mut source)
    .unwrap();
    let mut wrapper = Vec::new();
    object(7, 0x90, &[], &0u32.to_le_bytes(), &source)
        .write(&mut wrapper)
        .unwrap();
    let mut private = 1u32.to_le_bytes().to_vec();
    name(&mut private, "volume");
    private.extend(1f32.to_le_bytes());
    private.extend((-1i16).to_le_bytes());
    private.push(0x10);
    private.extend(0u16.to_le_bytes());
    name(&mut private, "Primary");
    private.extend([
        0,
        0,
        u8::from(router_open),
        u8::from(bypass),
        u8::from(!shared),
        0,
    ]);
    private.extend(0u32.to_le_bytes());
    name(&mut private, "Saved sole source");
    private.extend(2u32.to_le_bytes());
    let mut slots = vec![1];
    object(0x0d, 0x80, &private, &[], &wrapper)
        .write(&mut slots)
        .unwrap();
    slots.extend([0; 15]);
    let raw = RawGroup(StructuredObject {
        version: 0x95,
        private_data: vec![],
        public_data: vec![],
        children: vec![object(0x3b, 0x10, &[], &slots, &[])],
    });
    let decoded = crate::modulation::read_group(&raw).unwrap();
    assert_eq!(
        decoded.native_shared_volume_slot,
        (shared && !bypass).then_some(0)
    );
    assert_eq!(decoded.modulators[0].bypassed, bypass);
    Group {
        volume_env: decoded.volume_env,
        native_volume_env: decoded.native_volume_env,
        native_shared_volume_slot: decoded.native_shared_volume_slot,
        native_primary_bypass_slot: decoded.native_primary_bypass_slot,
        modulators: decoded.modulators,
        source_mode: Some(0),
        ..Default::default()
    }
}

fn address(i: usize) -> ExactNoteAddress {
    ExactNoteAddress::from_raw_signed(0, CHANNEL[i] as i16, KEY[i] as i16, 100 + i as i32)
}
fn push_note(events: &mut EventList, origin: usize, at: usize, i: usize, kind: ExactNoteKind) {
    events
        .try_push_exact(ExactEvent::new(
            (at - origin) as u32,
            ExactEventBody::Note {
                kind,
                address: address(i),
                velocity: 1.,
            },
        ))
        .unwrap();
}
fn push_cc(events: &mut EventList, origin: usize, at: usize, ch: u8, cc: u8, value: u8) {
    events.push(Event::on_port(
        (at - origin) as u32,
        0,
        EventBody::ControlChange {
            group: 0,
            channel: ch,
            cc,
            value,
        },
    ));
}

fn levels(i: usize) -> Vec<f32> {
    // Release publishes exactly three points before Done. Their scalar law is
    // native 43/3, rounded once to f32 and then multiplied iteratively.
    let r = (43f64 / 3.).powf(-1. / 3.) as f32;
    let r2 = 1.075f32 * r * r - 0.075; // Published second release recurrence, not r² gain.
    let r1 = 1.075f32 * r - 0.075;
    // Absolute consumer point times, handwritten per physical trajectory.
    // The first source is overwritten by the later-admitted Off112, so its
    // release begins at source point2 instead of point1 (the earlier Off48).
    let points: Vec<(usize, f32)> = match i {
        0 => vec![
            (0, 1.),
            (32, 1.),
            (64, 1.),
            (96, r1),
            (128, r2),
            (160, 0.),
            (192, 0.),
        ],
        1 => vec![
            (17, 1.),
            (49, 1.),
            (81, 1.),
            (113, r1),
            (145, r2),
            (177, 0.),
            (209, 0.),
        ],
        2 => vec![(512, 1.), (544, 1.), (576, r1), (608, r2)],
        3 => vec![
            (768, 1.),
            (800, 1.),
            (832, 1.),
            (864, 1.),
            (896, 1.),
            (928, r1),
            (960, r2),
            (992, 0.),
        ],
        4 => vec![(785, 1.), (817, 1.), (849, r1), (881, r2)],
        5 => vec![
            (1152, 1.),
            (1184, 1.),
            (1216, 1.),
            (1248, 1.),
            (1280, r1),
            (1312, r2),
            (1344, 0.),
            (1376, 0.),
        ],
        6 => vec![
            (1408, 1.),
            (2240, 1.),
            (2272, r1),
            (2304, r2),
            (2336, 0.),
            (2368, 0.),
        ],
        _ => unreachable!(),
    };
    let mut output = vec![0.; FINISH];
    for frame in ON[i]..END[i] {
        let index = points.iter().rposition(|p| p.0 <= frame).unwrap();
        let current = points[index].1.max(0.);
        let previous = points[index.saturating_sub(1)].1.max(0.);
        output[frame] = previous + (current - previous) * ((frame - ON[i]) & 31) as f32 / 32.;
    }
    output
}

fn signal(key: u8, age: usize) -> [f32; 2] {
    let bias = if key == 60 { 0.12 } else { -0.07 };
    [
        bias + (age % 37) as f32 / 1024.,
        -bias - (age % 53) as f32 / 2048.,
    ]
}
fn dsp(reference: Option<usize>) -> Dsp {
    let mut dsp = Dsp::default();
    dsp.until_poll = usize::MAX;
    dsp.rack.reset(48_000.);
    let keys = reference.map_or(vec![60, 62], |i| vec![KEY[i]]);
    let groups: Vec<_> = keys
        .iter()
        .map(|_| saved(reference.is_none(), false))
        .collect();
    let zones: Vec<_> = keys
        .iter()
        .enumerate()
        .map(|(group, &key)| Zone {
            group,
            sample: format!("source-{group}").into(),
            low_key: key,
            high_key: key,
            root: key,
            ..Default::default()
        })
        .collect();
    let gain = reference.map(levels);
    let samples = keys
        .iter()
        .enumerate()
        .map(|(group, &key)| {
            let frames = (0..8192)
                .map(|age| {
                    let mut sample = signal(key, age);
                    if let Some(i) = reference {
                        let factor = gain
                            .as_ref()
                            .unwrap()
                            .get(ON[i] + age)
                            .copied()
                            .unwrap_or(0.);
                        sample.iter_mut().for_each(|x| *x *= factor);
                    }
                    sample
                })
                .collect();
            (
                format!("source-{group}").into(),
                Sample {
                    rate: 48_000,
                    frames,
                },
            )
        })
        .collect();
    let bank = Bank::from_samples(groups, zones, samples).unwrap();
    dsp.rack.parts[0].instrument_mut().unwrap().set_bank(Some(Box::new(bank)));
    dsp
}
fn params() -> SamplerParams {
    let p = SamplerParams::new();
    p.volume.set_value(0.);
    p.snap_smoothers();
    p
}

fn performance(split: bool) -> Vec<[f32; 2]> {
    let mut actual = dsp(None);
    let params = params();
    let mut dry: Vec<_> = (0..7).map(|i| (dsp(Some(i)), self::params())).collect();
    let mut pcm = vec![[0.; 2]; FINISH];
    let mut expected = vec![[0.; 2]; FINISH];
    let mut ended = Vec::with_capacity(7);
    let transport = TransportInfo::default();
    for pair in HOST.windows(2) {
        let (origin, end) = (pair[0], pair[1]);
        let n = end - origin;
        let mut events = EventList::with_capacity(1024);
        // Build all events outside the audio allocation guard and sort by
        // authored frame. No provider candidate, phase or producer is read.
        for at in origin..end {
            for i in 0..7 {
                if at == ON[i] {
                    push_note(&mut events, origin, at, i, ExactNoteKind::On);
                }
            }
            for (frame, i) in [
                (48, 0),
                (112, 1),
                (560, 2),
                (816, 3),
                (848, 4),
                (1184, 5),
                (2240, 6),
            ] {
                if at == frame {
                    push_note(&mut events, origin, at, i, ExactNoteKind::Off);
                }
            }
            for (frame, ch, cc, value) in [
                (800, 4, 66, 127),
                (912, 4, 66, 0),
                (1168, 7, 64, 127),
                (1216, 4, 64, 0),
                (1248, 7, 64, 0),
            ] {
                if at == frame {
                    push_cc(&mut events, origin, at, ch, cc, value);
                }
            }
            if split && [1, 8, 39, 166].contains(&((at - origin) % 173)) {
                push_cc(&mut events, origin, at, 15, 1, 0);
            }
        }
        let mut output = EventList::with_capacity(32);
        let (mut l, mut r) = (vec![0.; n], vec![0.; n]);
        if matches!(origin, 128 | 256) {
            let value = if origin == 128 { -60. } else { 0. };
            params.volume.set_value(value);
            params.snap_smoothers();
            for (_, p) in &dry {
                p.volume.set_value(value);
                p.snap_smoothers();
            }
        }
        assert_eq!(
            super::tests::allocations(|| {
                let mut channels = [l.as_mut_slice(), r.as_mut_slice()];
                let mut audio = AudioBuffer::from_slices_checked(&[], &mut channels, n);
                let mut cx = ProcessContext::new(&transport, 48_000., n, &mut output)
                    .with_process_mode(moose::core::ProcessMode::Realtime);
                Sampler::process(&mut actual, &params, &mut audio, &events, &mut cx);
            }),
            0
        );
        assert_eq!(actual.rack.parts[0].instrument().unwrap().shared_source_reason(), None);
        for (offset, (left, right)) in l.into_iter().zip(r).enumerate() {
            pcm[origin + offset] = [left, right];
        }
        for event in output.lossless_iter() {
            if let LosslessEventRef::Exact(event) = event {
                if let ExactEventBody::Note {
                    kind: ExactNoteKind::End,
                    address,
                    ..
                } = *event.body()
                {
                    ended.push((address, origin + event.sample_offset() as usize));
                }
            }
        }
        for (i, (dsp, p)) in dry.iter_mut().enumerate() {
            let mut input = EventList::with_capacity(1);
            if (origin..end).contains(&ON[i]) {
                push_note(&mut input, origin, ON[i], i, ExactNoteKind::On);
            }
            let mut out = EventList::with_capacity(0);
            let (mut l, mut r) = (vec![0.; n], vec![0.; n]);
            let mut channels = [l.as_mut_slice(), r.as_mut_slice()];
            let mut audio = AudioBuffer::from_slices_checked(&[], &mut channels, n);
            let mut cx = ProcessContext::new(&transport, 48_000., n, &mut out)
                .with_process_mode(moose::core::ProcessMode::Realtime);
            assert_eq!(
                super::tests::allocations(|| {
                    assert_eq!(
                        Sampler::process(dsp, p, &mut audio, &input, &mut cx),
                        ProcessStatus::Normal
                    );
                }),
                0
            );
            for frame in 0..n {
                expected[origin + frame][0] += l[frame];
                expected[origin + frame][1] += r[frame];
            }
        }
    }
    assert_eq!(ended.len(), 7);
    for i in 0..7 {
        assert_eq!(
            ended.iter().find(|x| x.0 == address(i)).map(|x| x.1),
            Some(END[i])
        );
    }
    let error = pcm
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    assert!(
        error < 3e-6,
        "shared source exterior complete PCM error {error}"
    );
    assert!(pcm[32..128].iter().flatten().any(|x| x.abs() > 0.01));
    assert!(
        pcm[608..640].iter().flatten().any(|x| x.abs() > 0.001),
        "source-wave completion cuts a genuinely audible final endpoint"
    );
    assert!(pcm[2433..].iter().flatten().all(|x| *x == 0.));
    pcm
}

#[test]
fn accepted_shared_primary_plugin_pcm_keeps_overwritten_off_pedals_restart_and_range_ends_without_heap()
 {
    let whole = performance(false);
    let split = performance(true);
    assert!(
        whole
            .iter()
            .flatten()
            .zip(split.iter().flatten())
            .all(|(a, b)| (a - b).abs() < 3e-6)
    );
}

#[test]
fn unsupported_shared_primary_rosters_reject_before_plugin_audio_or_root_publication() {
    let mut empty_block = dsp(None);
    let p = params();
    let mut on = EventList::with_capacity(1);
    push_note(&mut on, 0, 0, 0, ExactNoteKind::On);
    let mut empty_output = EventList::with_capacity(4);
    let transport = TransportInfo::default();
    assert_eq!(
        super::tests::allocations(|| {
            let (mut l, mut r) = ([0.; 0], [0.; 0]);
            let mut channels = [l.as_mut_slice(), r.as_mut_slice()];
            let mut audio = AudioBuffer::from_slices_checked(&[], &mut channels, 0);
            let mut cx = ProcessContext::new(&transport, 48_000., 0, &mut empty_output)
                .with_process_mode(moose::core::ProcessMode::Realtime);
            assert_eq!(
                Sampler::process(&mut empty_block, &p, &mut audio, &on, &mut cx),
                ProcessStatus::Error
            );
        }),
        0
    );
    assert_eq!(empty_block.rack.parts[0].active_voices(), 0);
    assert_eq!(empty_output.lossless_iter().count(), 0);

    // These are real raw bypass/retrigger flags, not a prepared capability edit.
    let zone = Zone {
        root: 60,
        low_key: 60,
        high_key: 60,
        ..Default::default()
    };
    let sample = || Sample {
        rate: 48_000,
        frames: (0..8192).map(|age| signal(60, age)).collect(),
    };
    // Router-open is flags[0]; it is legitimate UI state, not bypass.
    assert!(
        Bank::from_samples(
            vec![saved_flags(true, false, true)],
            vec![zone.clone()],
            vec![(Default::default(), sample())]
        )
        .is_ok()
    );
    assert!(
        Bank::from_samples(
            vec![saved(true, true)],
            vec![zone.clone()],
            vec![(Default::default(), sample())]
        )
        .is_err()
    );
    let mut looped = zone.clone();
    looped.loop_range = Some(crate::import::Loop {
        start: 0,
        end: 8192,
        crossfade: 0,
        until_release: false,
        alternating: false,
    });
    assert!(
        Bank::from_samples(
            vec![saved(true, false)],
            vec![looped],
            vec![(Default::default(), sample())]
        )
        .is_err()
    );

    // A release-trigger sibling is a runtime range rejection: the saved shared
    // source itself remains valid, but no clone/root is published atomically.
    let mut release = saved(false, false);
    release.release_trigger = true;
    let bank = Bank::from_samples(
        vec![saved(true, false), release],
        vec![zone.clone(), Zone { group: 1, ..zone }],
        vec![(Default::default(), sample())],
    )
    .unwrap();
    let mut actual = Dsp::default();
    actual.until_poll = usize::MAX;
    actual.rack.reset(48_000.);
    actual.rack.parts[0].instrument_mut().unwrap().set_bank(Some(Box::new(bank)));
    let p = params();
    let mut input = EventList::with_capacity(2);
    push_note(&mut input, 0, 0, 0, ExactNoteKind::On);
    push_note(&mut input, 0, 48, 0, ExactNoteKind::Off);
    let mut output = EventList::with_capacity(4);
    let (mut l, mut r) = ([0.; 128], [0.; 128]);
    let transport = TransportInfo::default();
    assert_eq!(
        super::tests::allocations(|| {
            let mut channels = [l.as_mut_slice(), r.as_mut_slice()];
            let mut audio = AudioBuffer::from_slices_checked(&[], &mut channels, 128);
            let mut cx = ProcessContext::new(&transport, 48_000., 128, &mut output)
                .with_process_mode(moose::core::ProcessMode::Realtime);
            assert_eq!(
                Sampler::process(&mut actual, &p, &mut audio, &input, &mut cx),
                ProcessStatus::Error
            );
        }),
        0
    );
    assert!(actual.rack.parts[0].instrument().unwrap().shared_source_reason().is_some());
    assert_eq!(actual.rack.parts[0].active_voices(), 0);
    assert!(l.iter().chain(&r).all(|x| *x == 0.));
    assert_eq!(output.lossless_iter().count(), 0);
}
