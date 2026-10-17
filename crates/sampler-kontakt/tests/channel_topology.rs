//! Normal encoded source -> logical storage -> explicit signal graph -> Runtime.
//! Generic authored graph witnesses, not native UVI topology/projection evidence.
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
use sampler_core::lower::{LowerError, ir_control_id, lower_sources};
use sampler_core::{
    ControlValue, ControlWrite, Error, Input, Limits, Protocol, Runtime, SourceStorage,
};
use sampler_ir as ir;
use sampler_kontakt::EncodedChannelReader;
use std::io::Cursor;
use symphonia::core::meta::MetadataOptions;
const N: usize = 512;
fn sample(frame: usize, channel: usize, duplicate: bool) -> i16 {
    let channel = if duplicate && channel == 3 {
        2
    } else {
        channel
    };
    ((frame * (17 + channel * 2) + channel * 73) % 4096) as i16 - 2048
}
fn value(frame: usize, channel: usize, duplicate: bool) -> f32 {
    f32::from(sample(frame, channel, duplicate)) / 32768.
}
fn wav(channels: usize, floating: bool, duplicate: bool, cells: &[f64]) -> Vec<u8> {
    let bytes = if floating { 4 } else { 2 };
    let size = N * channels * bytes;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + size as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&(if floating { 3u16 } else { 1u16 }).to_le_bytes());
    out.extend_from_slice(&(channels as u16).to_le_bytes());
    out.extend_from_slice(&48000u32.to_le_bytes());
    out.extend_from_slice(&(48000 * channels as u32 * bytes as u32).to_le_bytes());
    out.extend_from_slice(&((channels * bytes) as u16).to_le_bytes());
    out.extend_from_slice(&((bytes * 8) as u16).to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    for frame in 0..N {
        for channel in 0..channels {
            if floating {
                let mut sum = 0f32;
                for input in 0..6 {
                    sum += value(frame, input, duplicate) * cells[channel * 6 + input] as f32;
                }
                out.extend_from_slice(&sum.to_le_bytes());
            } else {
                out.extend_from_slice(&sample(frame, channel, duplicate).to_le_bytes());
            }
        }
    }
    out
}
fn decode(bytes: Vec<u8>) -> SourceStorage {
    EncodedChannelReader::open_legacy(Box::new(Cursor::new(bytes)), &MetadataOptions::default())
        .unwrap()
        .read_resident_storage(1 << 20)
        .unwrap()
}
fn asset() -> ir::Asset {
    ir::Asset {
        location: ir::AssetLocation::Path("authored.wav".into()),
        encoding: ir::Encoding::Wav,
        residency: ir::AssetResidency::Resident,
        root_key: None,
        loops: vec![],
    }
}
fn zone() -> ir::Zone {
    ir::Zone {
        pitch: ir::KeyTracking::Fixed,
        velocity: ir::VelocityResponse::None,
        playback: ir::Playback {
            looping: ir::Looping::Continuous(ir::LoopRange {
                start: 0,
                end: N as u64,
                crossfade: ir::Span::Frames(0),
                alternating: false,
            }),
            ..Default::default()
        },
        ..ir::Zone::new(ir::AssetRef(0))
    }
}
fn instrument(cells: Vec<f64>, controlled: bool) -> ir::Instrument {
    let mut result = ir::Instrument {
        assets: vec![asset()],
        zones: vec![zone()],
        processing_scopes: vec![ir::ProcessingScope {
            members: vec![ir::ZoneRef(0)],
            sum: ir::ProcessingSumLaw::OrderedF32,
            cadence: ir::ProcessingCadence::CoreCell64,
            chain: None,
            amplitude: None,
            output: None,
            signal: Some(ir::ProcessingSignal {
                source_law: ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,
                input: ir::SignalLayout { channels: 6 },
                nodes: vec![ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix {
                    input: ir::SignalLayout { channels: 6 },
                    output: ir::SignalLayout { channels: 2 },
                    cells,
                    arithmetic: ir::MatrixArithmetic::SeparateMulAddF32,
                })],
                controls: vec![],
            }),
        }],
        ..Default::default()
    };
    if controlled {
        result.controls.push(ir::Control {
            key: "upper-cell".into(),
            label: "Upper cell".into(),
            value: ir::ControlValue::Continuous {
                min: -1.,
                max: 1.,
                default: 0.25,
                unit: ir::ControlUnit::None,
            },
            automation: ir::Automation::None,
        });
        result.processing_scopes[0]
            .signal
            .as_mut()
            .unwrap()
            .controls
            .push(ir::ProcessingCellBinding {
                node: 0,
                input: 3,
                output: 0,
                control: "upper-cell".into(),
            });
    }
    result
}
fn prepare(ir: &ir::Instrument, source: SourceStorage) -> sampler_core::Prepared {
    lower_sources(ir, 48000, vec![source], |_, p| Ok::<_, LowerError>(p)).unwrap()
}
fn runtime(ir: &ir::Instrument, source: SourceStorage) -> Runtime {
    let prepared = prepare(ir, source);
    let limits = Limits::for_plan(&prepared, 8, 16);
    Runtime::new(prepared, limits).unwrap()
}
fn reference(cells: &[f64], duplicate: bool) -> Runtime {
    let ir = ir::Instrument {
        assets: vec![asset()],
        zones: vec![zone()],
        ..Default::default()
    };
    runtime(&ir, decode(wav(2, true, duplicate, cells)))
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
fn render(runtime: &mut Runtime, count: usize, fragment: usize) -> Vec<[f32; 2]> {
    let mut out = vec![[0.; 2]; count];
    for block in out.chunks_mut(fragment) {
        runtime.render(block).unwrap();
    }
    assert_eq!(
        (runtime.nonfinite_frames(), runtime.stream_underruns()),
        (0, 0)
    );
    out
}
fn exact(a: &[[f32; 2]], b: &[[f32; 2]]) {
    assert_eq!(a.len(), b.len());
    for (i, (a, b)) in a.iter().zip(b).enumerate() {
        assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits), "frame {i}");
    }
}
#[test]
fn every_upper_original_channel_reaches_each_stereo_output_through_normal_runtime() {
    for upper in 2..6 {
        for output in 0..2 {
            let mut cells = vec![0.; 12];
            cells[output * 6 + upper] = 1.;
            cells[(1 - output) * 6] = 0.5;
            let mut actual = runtime(
                &instrument(cells.clone(), false),
                decode(wav(6, false, false, &[])),
            );
            let mut expected = reference(&cells, false);
            actual.trigger(input(1), 60, 1.).unwrap();
            expected.trigger(input(1), 60, 1.).unwrap();
            exact(
                &render(&mut actual, 1024, 17),
                &render(&mut expected, 1024, 257),
            );
            assert_eq!(
                (actual.voice_count(), actual.processing_owner_count()),
                (1, 1)
            );
        }
    }
}
#[test]
fn signed_multi_input_matrix_and_exact_cancellation_keep_a_nonzero_witness() {
    for (duplicate, cells) in [
        (
            false,
            vec![
                0.25, -0.5, 1., 0.25, -0.25, 0.5, -1., 0.25, 0., 0.5, -0.25, 0.25,
            ],
        ),
        (true, vec![0., 0., 1., -1., 0., 0., 0., 0., 0., 0., 0., 1.]),
    ] {
        let mut actual = runtime(
            &instrument(cells.clone(), false),
            decode(wav(6, false, duplicate, &[])),
        );
        let mut expected = reference(&cells, duplicate);
        actual.trigger(input(1), 60, 1.).unwrap();
        expected.trigger(input(1), 60, 1.).unwrap();
        let out = render(&mut actual, 1024, 31);
        exact(&out, &render(&mut expected, 1024, 64));
        assert!(out.iter().any(|frame| frame[1] != 0.));
        if duplicate {
            assert!(out.iter().all(|frame| frame[0] == 0.));
        }
    }
}
#[test]
fn live_upper_cell_changes_the_same_held_source_and_invalid_type_keeps_own_state() {
    let mut cells = vec![0.; 12];
    cells[3] = 0.25;
    cells[11] = 1.;
    let mut actual = runtime(
        &instrument(cells.clone(), true),
        decode(wav(6, false, false, &[])),
    );
    let note = actual.trigger(input(1), 60, 1.).unwrap();
    let mut before = reference(&cells, false);
    before.trigger(input(1), 60, 1.).unwrap();
    exact(&render(&mut actual, 123, 7), &render(&mut before, 123, 64));
    cells[3] = -0.5;
    let mut after = reference(&cells, false);
    after.trigger(input(1), 60, 1.).unwrap();
    render(&mut after, 123, 17);
    let mut owners = vec![];
    actual.visit_processing_owners(|owner| owners.push(owner.id));
    let plan = actual.active_plan();
    let id = ir_control_id("upper-cell");
    let revision = actual.control_revision(plan).unwrap();
    actual
        .edit_controls(
            plan,
            Some(revision),
            &[ControlWrite {
                id,
                value: ControlValue::Real(-0.5),
            }],
        )
        .unwrap();
    let retained = actual.control_revision(plan).unwrap();
    assert_eq!(
        actual.edit_controls(
            plan,
            Some(retained),
            &[ControlWrite {
                id,
                value: ControlValue::Toggle(false)
            }]
        ),
        Err(Error::InvalidInput)
    );
    assert_eq!(actual.control_revision(plan).unwrap(), retained);
    assert_eq!(actual.control_value(plan, id), Ok(ControlValue::Real(-0.5)));
    exact(&render(&mut actual, 333, 1), &render(&mut after, 333, 257));
    let mut kept = vec![];
    actual.visit_processing_owners(|owner| kept.push(owner.id));
    assert_eq!(owners, kept);
    assert_eq!(actual.note_plan(note), Ok(plan));
    assert_eq!(
        (actual.voice_count(), actual.processing_owner_count()),
        (1, 1)
    );
}
#[test]
fn wide_default_members_keep_reading_through_shared_release_and_retire() {
    let mut cells = vec![0.; 12];
    cells[3] = 1.;
    cells[11] = 1.;
    let mut ir = instrument(cells, false);
    ir.modulators.push(ir::Modulator {
        scope: ir::Scope::Voice,
        source: ir::ModulationSource::Envelope(ir::Envelope {
            release: ir::Time::Milliseconds(1.),
            ..Default::default()
        }),
    });
    ir.processing_scopes[0].amplitude = Some(ir::ModulatorRef(0));
    let mut actual = runtime(&ir, decode(wav(6, false, false, &[])));
    let note = actual.trigger(input(1), 60, 1.).unwrap();
    render(&mut actual, 123, 17);
    actual.note_off_exact(note, None).unwrap();
    let out = render(&mut actual, 64, 7);
    assert!(out[..16].iter().flatten().any(|value| *value != 0.));
    assert!(out[16..47].iter().flatten().any(|value| *value != 0.));
    assert!(out[49..].iter().all(|frame| *frame == [0.; 2]));
    assert_eq!(
        (actual.voice_count(), actual.processing_owner_count()),
        (0, 0)
    );
}
#[test]
fn later_builder_cannot_unbind_or_change_a_matrix_control_domain() {
    for mode in 0..3 {
        let mut cells = vec![0.; 12];
        cells[3] = 0.25;
        cells[11] = 1.;
        let prepared = prepare(&instrument(cells, true), decode(wav(6, false, false, &[])));
        let mut controls = prepared.controls().to_vec();
        match mode {
            0 => controls.clear(),
            1 => {
                controls[0].domain = sampler_core::ControlDomain::Toggle;
                controls[0].default = ControlValue::Toggle(false);
            }
            _ => controls[0].domain = sampler_core::ControlDomain::Real { min: -2., max: 2. },
        }
        assert!(matches!(
            prepared.with_controls(controls),
            Err(Error::InvalidInput)
        ));
    }
}

#[test]
fn queued_recall_scheduled_write_and_snapshot_use_the_same_upper_cell_lane() {
    let mut cells = vec![0.; 12];
    cells[3] = 0.25;
    cells[11] = 1.;
    let actual = runtime(
        &instrument(cells.clone(), true),
        decode(wav(6, false, false, &[])),
    );
    let (mut actual, mut queue) = actual.with_control_updates(1, 1).unwrap();
    let plan = actual.active_plan();
    let id = ir_control_id("upper-cell");
    let note = actual.trigger(input(1), 60, 1.).unwrap();
    let mut expected = reference(&cells, false);
    expected.trigger(input(1), 60, 1.).unwrap();
    exact(&render(&mut actual, 64, 13), &render(&mut expected, 64, 64));
    let mut snapshot = [ControlWrite {
        id,
        value: ControlValue::Real(99.),
    }];
    assert_eq!(actual.capture_controls(plan, &mut snapshot), Ok((1, 0)));
    assert_eq!(snapshot[0].value, ControlValue::Real(0.25));
    queue
        .submit(sampler_core::ControlRequest {
            plan,
            expected_revision: Some(0),
            operation: sampler_core::ControlOperation::Recall(Box::from([ControlWrite {
                id,
                value: ControlValue::Real(-0.5),
            }])),
        })
        .unwrap();
    assert_eq!(actual.poll_control_update(), Ok(Some(1)));
    assert_eq!(queue.reply().unwrap().result, Ok((1, 1)));
    cells[3] = -0.5;
    let mut changed = reference(&cells, false);
    changed.trigger(input(1), 60, 1.).unwrap();
    render(&mut changed, 64, 17);
    exact(&render(&mut actual, 59, 7), &render(&mut changed, 59, 64));
    actual
        .schedule_event(160, sampler_core::Event::Control(plan, snapshot[0]))
        .unwrap();
    exact(&render(&mut actual, 37, 31), &render(&mut changed, 37, 64));
    let mut original = reference(
        &{
            let mut cells = cells.clone();
            cells[3] = 0.25;
            cells
        },
        false,
    );
    original.trigger(input(1), 60, 1.).unwrap();
    render(&mut original, 160, 17);
    exact(
        &render(&mut actual, 353, 1),
        &render(&mut original, 353, 257),
    );
    assert_eq!(actual.note_plan(note), Ok(plan));
    assert_eq!(actual.control_value(plan, id), Ok(snapshot[0].value));
    assert_eq!(actual.control_revision(plan), Ok(2));
}

fn shared_chain() -> ir::Chain {
    ir::Chain {
        scope: ir::Scope::Voice,
        pre_amplitude: vec![
            ir::Processor::Color(ir::fx::color::Config::rectifier(ir::Rectifier::Full)),
            ir::Processor::Filter(ir::Filter {
                kind: ir::FilterKind::LowPass { poles: 2 },
                cutoff: ir::Frequency::Hertz(2300.),
                resonance: ir::Resonance::Q(0.65),
            }),
        ],
        post_amplitude: vec![],
    }
}
#[track_caller]
fn close(actual: &[[f32; 2]], expected: &[[f32; 2]]) {
    assert_eq!(actual.len(), expected.len());
    for (frame, (a, b)) in actual.iter().zip(expected).enumerate() {
        for channel in 0..2 {
            assert!(
                (a[channel] - b[channel]).abs() <= 2e-6,
                "frame {frame} channel {channel}: {} != {}",
                a[channel],
                b[channel]
            );
        }
    }
}
#[test]
fn three_wide_sources_keep_independent_clocks_before_one_shared_section_per_attack() {
    let mut cells = vec![0.; 12];
    cells[2] = 1.;
    cells[11] = 1.;
    let mut authored = instrument(cells.clone(), false);
    // Three source instances share one immutable logical source/storage identity,
    // while start/tune and advancing cursor state remain independent.
    authored.zones = vec![zone(), zone(), zone()];
    authored.zones[1].playback.start = 5;
    authored.zones[1].tune = ir::Pitch::Semitones(12.);
    authored.zones[2].playback.start = 17;
    authored.zones[2].tune = ir::Pitch::Semitones(-12.);
    authored.processing_scopes[0].members = vec![ir::ZoneRef(2), ir::ZoneRef(0), ir::ZoneRef(1)];
    authored.chains = vec![shared_chain()];
    authored.processing_scopes[0].chain = Some(ir::ChainRef(0));
    let mut stereo = authored.clone();
    stereo.processing_scopes[0].signal = None;
    let sources = vec![decode(wav(6, false, false, &[]))];
    let expected_sources = vec![decode(wav(2, true, false, &cells))];
    let prepared = lower_sources(&authored, 48000, sources, |_, p| Ok::<_, LowerError>(p)).unwrap();
    let limits = Limits::for_plan(&prepared, 16, 32);
    let mut actual = Runtime::new(prepared, limits).unwrap();
    let prepared = lower_sources(&stereo, 48000, expected_sources, |_, p| {
        Ok::<_, LowerError>(p)
    })
    .unwrap();
    let limits = Limits::for_plan(&prepared, 16, 32);
    let mut expected = Runtime::new(prepared, limits).unwrap();
    actual.trigger(input(1), 60, 1.).unwrap();
    expected.trigger(input(1), 60, 1.).unwrap();
    close(&render(&mut actual, 97, 13), &render(&mut expected, 97, 64));
    assert_eq!(
        (actual.voice_count(), actual.processing_owner_count()),
        (3, 1)
    );
    actual.trigger(input(2), 60, 1.).unwrap();
    expected.trigger(input(2), 60, 1.).unwrap();
    close(
        &render(&mut actual, 1024, 31),
        &render(&mut expected, 1024, 127),
    );
    assert_eq!(
        (actual.voice_count(), actual.processing_owner_count()),
        (6, 2)
    );
}

#[test]
fn explicit_six_channel_matrix_retains_width_until_an_authored_second_projection() {
    let mut cells = vec![0.; 12];
    cells[3] = 0.5;
    cells[11] = -0.25;
    let mut authored = instrument(cells.clone(), false);
    let signal = authored.processing_scopes[0].signal.as_mut().unwrap();
    let mut identity = vec![0.; 36];
    for channel in 0..6 {
        identity[channel * 6 + channel] = 1.;
    }
    signal.nodes.insert(
        0,
        ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix {
            input: ir::SignalLayout { channels: 6 },
            output: ir::SignalLayout { channels: 6 },
            cells: identity,
            arithmetic: ir::MatrixArithmetic::SeparateMulAddF32,
        }),
    );
    let mut actual = runtime(&authored, decode(wav(6, false, false, &[])));
    let mut expected = reference(&cells, false);
    actual.trigger(input(1), 60, 1.).unwrap();
    expected.trigger(input(1), 60, 1.).unwrap();
    exact(
        &render(&mut actual, 1024, 23),
        &render(&mut expected, 1024, 256),
    );
}

#[test]
fn resident_wide_trigger_control_render_release_and_retirement_do_not_touch_the_heap() {
    let mut cells = vec![0.; 12];
    cells[3] = 0.25;
    cells[11] = 1.;
    let mut authored = instrument(cells, true);
    authored.modulators.push(ir::Modulator {
        scope: ir::Scope::Voice,
        source: ir::ModulationSource::Envelope(ir::Envelope {
            release: ir::Time::Milliseconds(1.),
            ..Default::default()
        }),
    });
    authored.processing_scopes[0].amplitude = Some(ir::ModulatorRef(0));
    let mut actual = runtime(&authored, decode(wav(6, false, false, &[])));
    let plan = actual.active_plan();
    let id = ir_control_id("upper-cell");
    let mut out = [[0.; 2]; 128];
    support::without_heap(|| {
        let note = actual.trigger(input(1), 60, 1.).unwrap();
        actual.render(&mut out[..97]).unwrap();
        actual
            .edit_controls(
                plan,
                Some(0),
                &[ControlWrite {
                    id,
                    value: ControlValue::Real(-0.5),
                }],
            )
            .unwrap();
        actual.render(&mut out[..123]).unwrap();
        assert!(out[..123].iter().flatten().any(|value| *value != 0.));
        actual.note_off_exact(note, None).unwrap();
        actual.render(&mut out).unwrap();
        assert_eq!(
            (actual.voice_count(), actual.processing_owner_count()),
            (0, 0)
        );
    });
    assert_eq!(
        (actual.nonfinite_frames(), actual.stream_underruns()),
        (0, 0)
    );
}

#[test]
fn normal_voice_growth_preserves_live_channel_state_and_prepares_new_source_slots() {
    let mut cells = vec![0.; 12];
    cells[3] = 0.25;
    cells[11] = 1.;
    let mut authored = instrument(cells.clone(), false);
    authored.zones.push(zone());
    authored.processing_scopes[0].members.push(ir::ZoneRef(1));
    let prepared = prepare(&authored, decode(wav(6, false, false, &[])));
    let limits = Limits::for_plan(&prepared, 16, 2);
    let (mut actual, mut control) = Runtime::with_plan_updates(prepared, limits, 2, 1).unwrap();
    let reference_plan = prepare(&authored, decode(wav(6, false, false, &[])));
    let limits = Limits::for_plan(&reference_plan, 8, 16);
    let mut expected = Runtime::new(reference_plan, limits).unwrap();
    actual.trigger(input(1), 60, 1.).unwrap();
    expected.trigger(input(1), 60, 1.).unwrap();
    exact(&render(&mut actual, 97, 13), &render(&mut expected, 97, 64));
    assert_eq!(actual.voice_count(), 2);
    assert_eq!(control.grow_voices(4), Ok(4));
    support::without_heap(|| {
        actual.render(&mut []).unwrap();
    });
    assert_eq!(actual.voice_capacity(), 4);
    actual.trigger(input(2), 60, 1.).unwrap();
    expected.trigger(input(2), 60, 1.).unwrap();
    exact(
        &render(&mut actual, 1024, 31),
        &render(&mut expected, 1024, 257),
    );
    assert_eq!(
        (actual.voice_count(), actual.processing_owner_count()),
        (4, 2)
    );
}

#[test]
fn retained_generation_owns_its_original_family_and_matrix_after_new_plan_publication() {
    let mut cells = vec![0.; 12];
    cells[3] = 0.25;
    cells[11] = 1.;
    let mut authored = instrument(cells.clone(), true);
    authored.modulators.push(ir::Modulator {
        scope: ir::Scope::Voice,
        source: ir::ModulationSource::Envelope(ir::Envelope {
            release: ir::Time::Milliseconds(1.),
            ..Default::default()
        }),
    });
    authored.processing_scopes[0].amplitude = Some(ir::ModulatorRef(0));
    let prepared = prepare(&authored, decode(wav(6, false, false, &[])));
    let limits = Limits::for_plan(&prepared, 8, 16);
    let (mut actual, mut control) = Runtime::with_plan_updates(prepared, limits, 2, 1).unwrap();
    let old = actual.active_plan();
    let old_note = actual.trigger(input(1), 60, 1.).unwrap();
    let mut old_reference = runtime(&authored, decode(wav(6, false, false, &[])));
    old_reference.trigger(input(1), 60, 1.).unwrap();
    exact(
        &render(&mut actual, 97, 13),
        &render(&mut old_reference, 97, 64),
    );
    let mut incoming = authored.clone();
    if let ir::ControlValue::Continuous { default, .. } = &mut incoming.controls[0].value {
        *default = -0.5;
    }
    control
        .submit(Box::new(prepare(
            &incoming,
            decode(wav(6, false, false, &[])),
        )))
        .unwrap();
    support::without_heap(|| {
        actual.poll_plan_update().unwrap();
    });
    let new = actual.active_plan();
    assert_ne!(old, new);
    assert_eq!(actual.note_plan(old_note), Ok(old));
    assert_eq!(actual.collect_retired_plans(), 0);
    actual.trigger(input(2), 60, 1.).unwrap();
    let mut new_reference = runtime(&incoming, decode(wav(6, false, false, &[])));
    new_reference.trigger(input(2), 60, 1.).unwrap();
    let a = render(&mut old_reference, 400, 17);
    let b = render(&mut new_reference, 400, 31);
    let expected: Vec<_> = a
        .iter()
        .zip(b)
        .map(|(a, b)| [a[0] + b[0], a[1] + b[1]])
        .collect();
    close(&render(&mut actual, 400, 23), &expected);
    assert_eq!(
        actual.control_value(old, ir_control_id("upper-cell")),
        Ok(ControlValue::Real(0.25))
    );
    assert_eq!(
        actual.control_value(new, ir_control_id("upper-cell")),
        Ok(ControlValue::Real(-0.5))
    );
    actual.note_off_exact(old_note, None).unwrap();
    render(&mut actual, 64, 7);
    actual.flush_ended(|_| true);
    assert_eq!(actual.collect_retired_plans(), 1);
    assert_eq!(
        (actual.voice_count(), actual.processing_owner_count()),
        (1, 1)
    );
}

#[test]
fn unsupported_source_pan_is_rejected_before_any_retained_note_or_group_write() {
    let mut cells = vec![0.; 12];
    cells[3] = 1.;
    cells[11] = 1.;
    let authored = instrument(cells.clone(), false);
    let mut actual = runtime(&authored, decode(wav(6, false, false, &[])));
    let mut expected = reference(&cells, false);
    let note = actual.trigger(input(1), 60, 1.).unwrap();
    expected.trigger(input(1), 60, 1.).unwrap();
    exact(&render(&mut actual, 97, 13), &render(&mut expected, 97, 64));
    assert_eq!(
        actual.set_note_param(note, sampler_core::ModTarget::Pan, 0.5, false),
        Err(Error::UnsupportedSourceLayout)
    );
    assert_eq!(
        actual.set_group_param(-1, sampler_core::ModTarget::Pan, -0.5, false),
        Err(Error::UnsupportedSourceLayout)
    );
    assert_eq!(actual.now(), 97);
    exact(
        &render(&mut actual, 512, 17),
        &render(&mut expected, 512, 257),
    );
    assert_eq!(
        (actual.voice_count(), actual.processing_owner_count()),
        (1, 1)
    );
}

#[test]
fn unsupported_source_section_fails_before_behavior_binding_and_cannot_publish_a_partial_graph() {
    let mut cells = vec![0.; 12];
    cells[3] = 1.;
    cells[11] = 1.;
    let mut authored = instrument(cells, false);
    authored.chains.push(shared_chain());
    authored.zones[0].chain = Some(ir::ChainRef(0));
    let called = std::cell::Cell::new(false);
    let result = lower_sources(
        &authored,
        48000,
        vec![decode(wav(6, false, false, &[]))],
        |_, p| {
            called.set(true);
            Ok::<_, LowerError>(p)
        },
    );
    assert!(result.is_err());
    assert!(!called.get());
}

#[test]
fn wide_source_capacity_failure_keeps_normal_note_family_owner_and_clock_state_unpublished() {
    let mut cells = vec![0.; 12];
    cells[3] = 1.;
    cells[11] = 1.;
    let mut authored = instrument(cells, false);
    authored.zones.push(zone());
    authored.zones.push(zone());
    authored.processing_scopes[0].members = vec![ir::ZoneRef(0), ir::ZoneRef(1), ir::ZoneRef(2)];
    let prepared = prepare(&authored, decode(wav(6, false, false, &[])));
    let limits = Limits::for_plan(&prepared, 16, 2);
    let mut actual = Runtime::new(prepared, limits).unwrap();
    let before = (
        actual.note_count(),
        actual.family_count(),
        actual.expression_count(),
        actual.voice_count(),
        actual.processing_owner_count(),
        actual.now(),
        actual.pending_commands(),
    );
    assert_eq!(actual.trigger(input(1), 60, 1.), Err(Error::Capacity));
    assert_eq!(
        (
            actual.note_count(),
            actual.family_count(),
            actual.expression_count(),
            actual.voice_count(),
            actual.processing_owner_count(),
            actual.now(),
            actual.pending_commands()
        ),
        before
    );
    assert!(
        render(&mut actual, 64, 17)
            .iter()
            .all(|frame| *frame == [0.; 2])
    );
}

#[test]
fn pending_event_key_change_revalidates_retained_source_pan_before_graph_publication() {
    let mut cells = vec![0.; 12];
    cells[3] = 1.;
    cells[11] = 1.;
    let mut authored = instrument(cells, false);
    authored.zones[0].keys = ir::KeyRange { low: 61, high: 61 };
    let mut actual = runtime(&authored, decode(wav(6, false, false, &[])));
    let note = actual.note_on(input(1), 60, 1.).unwrap();
    // At key60 there is no wide source candidate. This is a legal retained
    // stereo operand until the actual pending event is redirected to key61.
    actual
        .set_note_param(note, sampler_core::ModTarget::Pan, 0.5, false)
        .unwrap();
    let mut event = actual.note_event(note).unwrap();
    event.pitch = sampler_core::NotePitch::Key(61);
    actual.edit_note_event(note, event).unwrap();
    let before = (
        actual.note_count(),
        actual.family_count(),
        actual.expression_count(),
        actual.voice_count(),
        actual.processing_owner_count(),
        actual.now(),
        actual.pending_commands(),
    );
    assert_eq!(
        actual.forward_attack(note),
        Err(Error::UnsupportedSourceLayout)
    );
    assert_eq!(
        (
            actual.note_count(),
            actual.family_count(),
            actual.expression_count(),
            actual.voice_count(),
            actual.processing_owner_count(),
            actual.now(),
            actual.pending_commands()
        ),
        before
    );
    assert_eq!(
        (actual.nonfinite_frames(), actual.stream_underruns()),
        (0, 0)
    );
    actual
        .set_note_param_with_immediate(note, sampler_core::ModTarget::Pan, 0., false, true)
        .unwrap();
    assert_eq!(actual.forward_attack(note), Ok(true));
    let mut expected = runtime(&authored, decode(wav(6, false, false, &[])));
    expected.trigger(input(2), 61, 1.).unwrap();
    exact(
        &render(&mut actual, 1024, 13),
        &render(&mut expected, 1024, 257),
    );
    assert_eq!(
        (actual.voice_count(), actual.processing_owner_count()),
        (1, 1)
    );
}

#[test]
fn legacy_onset_snapshot_reports_wide_sources_incomplete_without_projecting_a_pack() {
    let mut cells = vec![0.; 12];
    cells[3] = 1.;
    cells[11] = 1.;
    let mut actual = runtime(&instrument(cells, false), decode(wav(6, false, false, &[])));
    actual.trigger(input(1), 60, 1.).unwrap();
    render(&mut actual, 17, 7);
    let before = (
        actual.voice_count(),
        actual.processing_owner_count(),
        actual.now(),
    );
    let mut visited = 0;
    assert!(!actual.visit_onset_voices(|_| {
        visited += 1;
        true
    }));
    assert_eq!(
        visited, 0,
        "a physical pack must never stand in for a logical source"
    );
    assert_eq!(
        (
            actual.voice_count(),
            actual.processing_owner_count(),
            actual.now()
        ),
        before
    );

    let source = decode(wav(2, false, false, &[]));
    let expected_asset = source.as_source().stereo().unwrap().asset_id();
    let authored = ir::Instrument {
        assets: vec![asset()],
        zones: vec![zone()],
        ..Default::default()
    };
    let mut stereo = runtime(&authored, source);
    stereo.trigger(input(2), 60, 1.).unwrap();
    render(&mut stereo, 17, 7);
    let mut assets = vec![];
    assert!(stereo.visit_onset_voices(|row| {
        assets.push(row.asset);
        true
    }));
    assert_eq!(assets, vec![expected_asset]);
    assert!(!stereo.visit_onset_voices(|_| false));
}

#[test]
fn retained_stereo_trace_does_not_force_a_legacy_asset_projection_for_an_untraced_wide_generation()
{
    let old_ir = ir::Instrument {
        assets: vec![asset()],
        zones: vec![zone()],
        ..Default::default()
    };
    let (old, _reader) = prepare(&old_ir, decode(wav(2, false, false, &[])))
        .with_signal_trace_reader(65536)
        .unwrap();
    let limits = Limits::for_plan(&old, 8, 16);
    let (mut actual, mut control) = Runtime::with_plan_updates(old, limits, 2, 1).unwrap();
    let old_plan = actual.active_plan();
    let old_note = actual.trigger(input(1), 60, 1.).unwrap();
    let (old_reference, _reference_reader) = prepare(&old_ir, decode(wav(2, false, false, &[])))
        .with_signal_trace_reader(65536)
        .unwrap();
    let mut old_reference = Runtime::new(old_reference, limits).unwrap();
    old_reference.trigger(input(1), 60, 1.).unwrap();
    exact(
        &render(&mut actual, 97, 13),
        &render(&mut old_reference, 97, 64),
    );

    let mut cells = vec![0.; 12];
    cells[3] = 1.;
    cells[11] = 1.;
    let incoming = instrument(cells, false);
    control
        .submit(Box::new(prepare(
            &incoming,
            decode(wav(6, false, false, &[])),
        )))
        .unwrap();
    actual.poll_plan_update().unwrap();
    assert_ne!(actual.active_plan(), old_plan);
    assert_eq!(actual.note_plan(old_note), Ok(old_plan));
    assert_eq!(actual.collect_retired_plans(), 0);
    actual.trigger(input(2), 60, 1.).unwrap();
    let mut new_reference = runtime(&incoming, decode(wav(6, false, false, &[])));
    new_reference.trigger(input(2), 60, 1.).unwrap();
    let old = render(&mut old_reference, 400, 17);
    let new = render(&mut new_reference, 400, 31);
    let expected: Vec<_> = old
        .iter()
        .zip(new)
        .map(|(a, b)| [a[0] + b[0], a[1] + b[1]])
        .collect();
    exact(&render(&mut actual, 400, 23), &expected);
    assert_eq!(
        (actual.voice_count(), actual.processing_owner_count()),
        (2, 1)
    );
    let mut projected = 0;
    assert!(!actual.visit_onset_voices(|_| {
        projected += 1;
        true
    }));
    assert_eq!(
        projected, 0,
        "mixed generations must not publish a misleading partial asset inventory"
    );
}

fn held_timbre(runtime: &mut Runtime, note: sampler_core::NoteId, timbre: u32) {
    let id = runtime.expression_id(note).unwrap();
    let mut expression = runtime.expression(id).unwrap();
    expression.timbre = timbre;
    runtime.set_expression(id, expression).unwrap();
}

#[test]
fn held_mpe_timbre_uses_existing_tone_on_every_selected_original_channel() {
    for (left, right) in [(2, 3), (3, 5), (5, 2)] {
        let mut cells = vec![0.; 12];
        cells[left] = 1.;
        cells[6 + right] = 1.;
        let authored = instrument(cells.clone(), false);
        let mut actual = runtime(&authored, decode(wav(6, false, false, &[])));
        let mut expected = reference(&cells, false);
        let a = actual.trigger(input(1), 60, 1.).unwrap();
        let b = expected.trigger(input(1), 60, 1.).unwrap();
        let mut owners = vec![];
        actual.visit_processing_owners(|row| owners.push(row.id));
        exact(
            &render(&mut actual, 123, 7),
            &render(&mut expected, 123, 64),
        );
        held_timbre(&mut actual, a, 0);
        held_timbre(&mut expected, b, 0);
        let dark = render(&mut actual, 333, 13);
        exact(&dark, &render(&mut expected, 333, 257));
        let mut open = reference(&cells, false);
        open.trigger(input(3), 60, 1.).unwrap();
        render(&mut open, 123, 64);
        assert!(
            dark.iter()
                .zip(render(&mut open, 333, 64))
                .any(|(a, b)| a != &b)
        );
        held_timbre(&mut actual, a, 0x8000_0000);
        held_timbre(&mut expected, b, 0x8000_0000);
        exact(
            &render(&mut actual, 177, 17),
            &render(&mut expected, 177, 64),
        );
        held_timbre(&mut actual, a, 0);
        held_timbre(&mut expected, b, 0);
        exact(
            &render(&mut actual, 257, 31),
            &render(&mut expected, 257, 1),
        );
        let mut kept = vec![];
        actual.visit_processing_owners(|row| kept.push(row.id));
        assert_eq!(owners, kept);
        assert_eq!(
            (actual.voice_count(), actual.processing_owner_count()),
            (1, 1)
        );
    }
}

#[test]
fn channel_tone_growth_retains_history_and_reused_sources_begin_with_zero_integrators() {
    let mut cells = vec![0.; 12];
    cells[3] = 1.;
    cells[11] = 1.;
    let authored = instrument(cells.clone(), false);
    let prepared = prepare(&authored, decode(wav(6, false, false, &[])));
    let limits = Limits::for_plan(&prepared, 16, 2);
    let (mut actual, mut control) = Runtime::with_plan_updates(prepared, limits, 2, 1).unwrap();
    let mut expected = reference(&cells, false);
    let a = actual.trigger(input(1), 60, 1.).unwrap();
    let b = expected.trigger(input(1), 60, 1.).unwrap();
    held_timbre(&mut actual, a, 0);
    held_timbre(&mut expected, b, 0);
    exact(&render(&mut actual, 97, 13), &render(&mut expected, 97, 64));
    assert_eq!(control.grow_voices(4), Ok(4));
    support::without_heap(|| {
        actual.render(&mut []).unwrap();
    });
    assert_eq!(actual.voice_capacity(), 4);
    exact(
        &render(&mut actual, 333, 31),
        &render(&mut expected, 333, 257),
    );
    actual.note_off_exact(a, None).unwrap();
    expected.note_off_exact(b, None).unwrap();
    render(&mut actual, 128, 17);
    render(&mut expected, 128, 64);
    assert_eq!(
        (actual.voice_count(), actual.processing_owner_count()),
        (0, 0)
    );
    let a = actual.trigger(input(2), 60, 1.).unwrap();
    let mut fresh = reference(&cells, false);
    render(&mut fresh, actual.now() as usize, 64);
    let b = fresh.trigger(input(2), 60, 1.).unwrap();
    held_timbre(&mut actual, a, 0);
    held_timbre(&mut fresh, b, 0);
    exact(&render(&mut actual, 512, 7), &render(&mut fresh, 512, 64));
}

// B3: independently evaluated generic core-law stages, not native UVI laws.
fn stage_parameter(value: f64, control: Option<&str>) -> ir::SignalParameter {
    ir::SignalParameter {
        constant: value,
        control: control.map(str::to_owned),
        expression: None,
    }
}
fn owner_gain(value: f64, controlled: bool) -> ir::ProcessingSignalNode {
    ir::ProcessingSignalNode::Gain(ir::ChannelGain {
        gain: stage_parameter(value, controlled.then_some("wide-gain")),
        arithmetic: ir::ChannelGainArithmetic::CoreGainF64ThenF32,
    })
}
fn owner_filter(high: bool, tail_frames: u32, controlled: bool) -> ir::ProcessingSignalNode {
    ir::ProcessingSignalNode::OnePole(ir::ChannelOnePole {
        cutoff_hz: stage_parameter(1379., controlled.then_some("wide-cutoff")),
        mode: if high {
            ir::ChannelOnePoleMode::HighPass
        } else {
            ir::ChannelOnePoleMode::LowPass
        },
        arithmetic: ir::ChannelOnePoleArithmetic::CoreOnePoleF64,
        tail_frames,
    })
}
fn stage_graph(cells: Vec<f64>, high: bool, gain_first: bool, controlled: bool) -> ir::Instrument {
    let mut authored = instrument(cells, false);
    let projection = authored.processing_scopes[0]
        .signal
        .as_mut()
        .unwrap()
        .nodes
        .pop()
        .unwrap();
    let mut identity = vec![0.; 36];
    for i in 0..6 {
        identity[i * 6 + i] = 1.;
    }
    // Nontrivial N->N matrix stays before both stateful and gain stages.
    identity[2 * 6 + 3] = -0.25;
    identity[5 * 6 + 4] = 0.5;
    let mut nodes = vec![ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix {
        input: ir::SignalLayout { channels: 6 },
        output: ir::SignalLayout { channels: 6 },
        cells: identity,
        arithmetic: ir::MatrixArithmetic::SeparateMulAddF32,
    })];
    if gain_first {
        nodes.extend([
            owner_gain(0.375, controlled),
            owner_filter(high, 0, controlled),
        ]);
    } else {
        nodes.extend([
            owner_filter(high, 0, controlled),
            owner_gain(0.375, controlled),
        ]);
    }
    nodes.push(projection);
    authored.processing_scopes[0].signal.as_mut().unwrap().nodes = nodes;
    if controlled {
        for (key, min, max, default) in [
            ("wide-gain", -2., 2., 0.375),
            ("wide-cutoff", 20., 18000., 1379.),
        ] {
            authored.controls.push(ir::Control {
                key: key.into(),
                label: key.into(),
                value: ir::ControlValue::Continuous {
                    min,
                    max,
                    default,
                    unit: ir::ControlUnit::None,
                },
                automation: ir::Automation::None,
            });
        }
    }
    authored
}
fn three_members(authored: &mut ir::Instrument) {
    authored.zones = vec![zone(), zone(), zone()];
    authored.zones[1].playback.start = 5;
    authored.zones[1].tune = ir::Pitch::Semitones(12.);
    authored.zones[2].playback.start = 17;
    authored.zones[2].tune = ir::Pitch::Semitones(-12.);
    authored.processing_scopes[0].members = vec![ir::ZoneRef(2), ir::ZoneRef(0), ir::ZoneRef(1)];
}
/// Three independent matrix-only Runtime projections expose the six ordinary
/// source/sum channels. No OnePole/Gain node under test participates in this input
/// oracle. This isolates stage arithmetic while retaining the real interpolation,
/// source envelopes and independently advancing source clocks under test.
fn raw_six(authored: &ir::Instrument, frames: usize, fragment: usize) -> Vec<[f32; 6]> {
    let mut result = vec![[0.; 6]; frames];
    for pair in 0..3 {
        let mut raw = authored.clone();
        raw.controls.clear();
        let mut cells = vec![0.; 12];
        cells[pair * 2] = 1.;
        cells[6 + pair * 2 + 1] = 1.;
        raw.processing_scopes[0].signal =
            instrument(cells, false).processing_scopes[0].signal.take();
        let mut rt = runtime(&raw, decode(wav(6, false, false, &[])));
        rt.trigger(input(1), 60, 1.).unwrap();
        for (to, from) in result.iter_mut().zip(render(&mut rt, frames, fragment)) {
            to[pair * 2] = from[0];
            to[pair * 2 + 1] = from[1];
        }
    }
    result
}
/// Independent operation tree, using only the documented authored formulas.
/// It never calls the stage compiler/kernel or carries one filter per member.
fn stage_oracle(
    raw: &[[f32; 6]],
    cells: &[f64],
    high: bool,
    gain_first: bool,
    changes: &[(usize, f64, f64)],
) -> Vec<[f32; 2]> {
    let mut state = [0f64; 6];
    let mut gain = 0.375;
    let mut hz = 1379.;
    raw.iter()
        .enumerate()
        .map(|(at, source)| {
            for &(when, next_gain, next_hz) in changes {
                if when == at {
                    gain = next_gain;
                    hz = next_hz;
                }
            }
            let b = -(-std::f64::consts::TAU * hz / 48000.).exp_m1();
            let mut signal = *source;
            signal[2] += source[3] * -0.25;
            signal[5] += source[4] * 0.5;
            for channel in 0..6 {
                let mut x = signal[channel];
                if gain_first {
                    x = (f64::from(x) * gain) as f32;
                }
                let input = f64::from(x);
                let next = state[channel] + (input - state[channel]) * b;
                state[channel] = next;
                x = if high {
                    (input - next) as f32
                } else {
                    next as f32
                };
                if !gain_first {
                    x = (f64::from(x) * gain) as f32;
                }
                signal[channel] = x;
            }
            let mut out = [0f32; 2];
            for row in 0..2 {
                for column in 0..6 {
                    out[row] += signal[column] * cells[row * 6 + column] as f32;
                }
            }
            out
        })
        .collect()
}
#[test]
fn width_preserving_core_stages_match_independent_six_channel_three_source_oracle() {
    for high in [false, true] {
        for upper in 2..6 {
            for output in 0..2 {
                let mut cells = vec![0.; 12];
                cells[output * 6 + upper] = 1.;
                cells[(1 - output) * 6] = -0.25;
                let mut authored = stage_graph(cells.clone(), high, false, false);
                three_members(&mut authored);
                let raw = raw_six(&authored, 1500, 257);
                let expected = stage_oracle(&raw, &cells, high, false, &[]);
                let mut actual = runtime(&authored, decode(wav(6, false, false, &[])));
                actual.trigger(input(1), 60, 1.).unwrap();
                close(&render(&mut actual, 1500, 17), &expected);
                assert_eq!(
                    (actual.voice_count(), actual.processing_owner_count()),
                    (3, 1)
                );
                assert!(expected.iter().any(|v| v[output] != 0.));
            }
        }
    }
}
#[test]
fn gain_filter_order_and_live_cutoff_use_the_same_held_owner_and_typed_value_lanes() {
    let cells = vec![
        0.25, -0.5, 1., 0.25, -0.25, 0.5, -1., 0.25, 0., 0.5, -0.25, 0.25,
    ];
    for gain_first in [false, true] {
        let mut authored = stage_graph(cells.clone(), false, gain_first, true);
        three_members(&mut authored);
        let raw = raw_six(&authored, 1024, 127);
        let expected = stage_oracle(&raw, &cells, false, gain_first, &[(97, -1.25, 4321.)]);
        let opposite = stage_oracle(&raw, &cells, false, !gain_first, &[(97, -1.25, 4321.)]);
        assert!(
            expected
                .iter()
                .zip(&opposite)
                .any(|(a, b)| (a[0] - b[0]).abs() > 1e-5)
        );
        let mut actual = runtime(&authored, decode(wav(6, false, false, &[])));
        let note = actual.trigger(input(1), 60, 1.).unwrap();
        let plan = actual.active_plan();
        close(&render(&mut actual, 97, 7), &expected[..97]);
        let mut owners = vec![];
        actual.visit_processing_owners(|o| owners.push(o.id));
        let mut writes = [
            ControlWrite { id: ir_control_id("wide-gain"), value: ControlValue::Real(-1.25) },
            ControlWrite { id: ir_control_id("wide-cutoff"), value: ControlValue::Real(4321.) },
        ];
        writes.sort_unstable_by_key(|write| write.id);
        actual.edit_controls(plan, None, &writes).unwrap();
        let before = (
            actual.now(),
            actual.control_revision(plan),
            actual.voice_count(),
        );
        let mut invalid = [
            ControlWrite { id: ir_control_id("wide-gain"), value: ControlValue::Real(0.5) },
            ControlWrite { id: ir_control_id("wide-cutoff"), value: ControlValue::Real(6000.) },
        ];
        invalid.sort_unstable_by_key(|write| write.id);
        // The first row is valid and sorted; the HIGHER-ID row fails typed
        // admission. An ordering failure cannot masquerade as this witness.
        invalid[1].value = ControlValue::Toggle(false);
        assert_eq!(actual.edit_controls(plan, None, &invalid), Err(Error::InvalidInput));
        assert_eq!(
            (
                actual.now(),
                actual.control_revision(plan),
                actual.voice_count()
            ),
            before
        );
        assert_eq!(
            actual.control_value(plan, ir_control_id("wide-gain")),
            Ok(ControlValue::Real(-1.25))
        );
        assert_eq!(actual.control_value(plan, ir_control_id("wide-cutoff")), Ok(ControlValue::Real(4321.)));
        close(&render(&mut actual, 927, 31), &expected[97..]);
        let mut after = vec![];
        actual.visit_processing_owners(|o| after.push(o.id));
        assert_eq!(owners, after);
        assert_eq!(actual.note_plan(note), Ok(plan));
    }
}
#[test]
fn staged_onepole_zero_input_tail_continues_after_source_exhaustion_then_retires() {
    let mut cells = vec![0.; 12];
    cells[3] = 1.;
    cells[11] = 1.;
    let mut authored = stage_graph(cells.clone(), false, false, false);
    authored.zones[0].playback = ir::Playback::default();
    if let ir::ProcessingSignalNode::OnePole(filter) =
        &mut authored.processing_scopes[0].signal.as_mut().unwrap().nodes[1]
    {
        filter.cutoff_hz.constant = 200.;
        filter.tail_frames = 128;
    }
    let raw = raw_six(&authored, N + 128, 127);
    let expected = stage_oracle(&raw, &cells, false, false, &[(0, 0.375, 200.)]);
    let mut actual = runtime(&authored, decode(wav(6, false, false, &[])));
    actual.trigger(input(1), 60, 1.).unwrap();
    close(&render(&mut actual, N + 128, 17), &expected);
    assert!(expected[N..N + 64].iter().flatten().any(|v| v.abs() > 1e-8));
    assert_eq!(
        (actual.voice_count(), actual.processing_owner_count()),
        (0, 0)
    );
    assert!(render(&mut actual, 65, 1).iter().all(|v| *v == [0.; 2]));
}
#[test]
fn owner_stage_history_survives_growth_and_independent_overlapping_attacks() {
    let mut cells = vec![0.; 12];
    cells[3] = 0.5;
    cells[11] = -0.25;
    let mut authored = stage_graph(cells, false, false, false);
    three_members(&mut authored);
    let prepared = prepare(&authored, decode(wav(6, false, false, &[])));
    let limits = Limits::for_plan(&prepared, 16, 3);
    let (mut actual, mut control) = Runtime::with_plan_updates(prepared, limits, 2, 1).unwrap();
    let mut first = runtime(&authored, decode(wav(6, false, false, &[])));
    actual.trigger(input(1), 60, 1.).unwrap();
    first.trigger(input(1), 60, 1.).unwrap();
    close(&render(&mut actual, 97, 3), &render(&mut first, 97, 64));
    assert_eq!(control.grow_voices(6), Ok(6));
    support::without_heap(|| {
        actual.render(&mut []).unwrap();
    });
    actual.trigger(input(2), 60, 1.).unwrap();
    let mut second = runtime(&authored, decode(wav(6, false, false, &[])));
    second.trigger(input(2), 60, 1.).unwrap();
    let a = render(&mut first, 900, 65);
    let b = render(&mut second, 900, 32);
    let expected: Vec<_> = a
        .iter()
        .zip(&b)
        .map(|(a, b)| [a[0] + b[0], a[1] + b[1]])
        .collect();
    close(&render(&mut actual, 900, 1), &expected);
    assert_eq!(
        (actual.voice_count(), actual.processing_owner_count()),
        (6, 2)
    );
}
#[test]
fn owner_stage_controls_queue_snapshot_and_scheduled_write_keep_one_history() {
    let mut cells = vec![0.; 12];
    cells[2] = 1.;
    cells[11] = 1.;
    let authored = stage_graph(cells.clone(), false, false, true);
    let raw = raw_six(&authored, 1024, 257);
    let expected = stage_oracle(
        &raw,
        &cells,
        false,
        false,
        &[(64, -0.5, 1379.), (160, 0.375, 1379.)],
    );
    let rt = runtime(&authored, decode(wav(6, false, false, &[])));
    let (mut actual, mut queue) = rt.with_control_updates(1, 2).unwrap();
    let plan = actual.active_plan();
    actual.trigger(input(1), 60, 1.).unwrap();
    close(&render(&mut actual, 64, 13), &expected[..64]);
    let id = ir_control_id("wide-gain");
    let mut snapshot = [ControlWrite {
        id,
        value: ControlValue::Real(0.),
    }; 2];
    assert_eq!(actual.capture_controls(plan, &mut snapshot), Ok((2, 0)));
    let mut recall = snapshot;
    recall.iter_mut().find(|write| write.id == id).unwrap().value = ControlValue::Real(-0.5);
    // Recall is the COMPLETE coherent snapshot in its captured ascending order;
    // the untouched cutoff row remains 1379 Hz.

    queue
        .submit(sampler_core::ControlRequest {
            plan,
            expected_revision: Some(0),
            operation: sampler_core::ControlOperation::Recall(Box::from(recall)),
        })
        .unwrap();
    assert_eq!(actual.poll_control_update(), Ok(Some(1)));
    assert_eq!(queue.reply().unwrap().result, Ok((2, 1)));
    actual
        .schedule_event(
            160,
            sampler_core::Event::Control(
                plan,
                ControlWrite {
                    id,
                    value: ControlValue::Real(0.375),
                },
            ),
        )
        .unwrap();
    close(&render(&mut actual, 960, 31), &expected[64..]);
    assert_eq!(
        actual.control_value(plan, id),
        Ok(ControlValue::Real(0.375))
    );
    assert_eq!(actual.control_value(plan, ir_control_id("wide-cutoff")), Ok(ControlValue::Real(1379.)));
    assert_eq!(actual.control_revision(plan), Ok(2));
}
#[test]
fn owner_stage_callback_render_control_release_and_tail_retirement_are_heap_free() {
    let mut cells = vec![0.; 12];
    cells[3] = 1.;
    cells[11] = 1.;
    let mut authored = stage_graph(cells, false, false, true);
    authored.modulators.push(ir::Modulator {
        scope: ir::Scope::Voice,
        source: ir::ModulationSource::Envelope(ir::Envelope {
            release: ir::Time::Milliseconds(1.),
            ..Default::default()
        }),
    });
    authored.processing_scopes[0].amplitude = Some(ir::ModulatorRef(0));
    let mut actual = runtime(&authored, decode(wav(6, false, false, &[])));
    let plan = actual.active_plan();
    support::without_heap(|| {
        let note = actual.trigger(input(1), 60, 1.).unwrap();
        actual.render(&mut [[0.; 2]; 97]).unwrap();
        actual
            .edit_controls(
                plan,
                None,
                &[ControlWrite {
                    id: ir_control_id("wide-cutoff"),
                    value: ControlValue::Real(6000.),
                }],
            )
            .unwrap();
        actual.render(&mut [[0.; 2]; 65]).unwrap();
        actual.note_off_exact(note, None).unwrap();
        let mut release = [[0.; 2]; 96];
        actual.render(&mut release).unwrap();
        assert!(release[..16].iter().flatten().any(|v| *v != 0.));
        assert!(release[49..].iter().all(|v| *v == [0.; 2]));
    });
    assert_eq!(
        (actual.voice_count(), actual.processing_owner_count()),
        (0, 0)
    );
}
#[test]
fn owner_stage_schema_domains_and_cutoff_geometry_refuse_before_behavior_binding() {
    let mut cells = vec![0.; 12];
    cells[3] = 1.;
    cells[11] = 1.;
    let authored = stage_graph(cells.clone(), false, false, true);
    for mode in 0..3 {
        let prepared = prepare(&authored, decode(wav(6, false, false, &[])));
        let mut controls = prepared.controls().to_vec();
        let target = controls
            .iter_mut()
            .find(|c| c.id == ir_control_id("wide-cutoff"))
            .unwrap();
        match mode {
            0 => target.domain = sampler_core::ControlDomain::Toggle,
            1 => {
                target.domain = sampler_core::ControlDomain::Real {
                    min: 20.,
                    max: 23999.,
                }
            }
            _ => target.default = ControlValue::Toggle(false),
        }
        assert!(matches!(
            prepared.with_controls(controls),
            Err(Error::InvalidInput)
        ));
    }
    let mut invalid = stage_graph(cells, false, false, false);
    if let ir::ProcessingSignalNode::OnePole(filter) =
        &mut invalid.processing_scopes[0].signal.as_mut().unwrap().nodes[1]
    {
        filter.cutoff_hz.constant = 24000.;
    }
    let called = std::cell::Cell::new(false);
    let result = lower_sources(
        &invalid,
        48000,
        vec![decode(wav(6, false, false, &[]))],
        |_, p| {
            called.set(true);
            Ok::<_, LowerError>(p)
        },
    );
    assert!(result.is_err());
    assert!(!called.get());
}
#[test]
fn width_preserving_filter_cancellation_keeps_a_separate_audible_upper_channel() {
    let mut cells = vec![0.; 12];
    cells[2] = 1.;
    cells[3] = -1.;
    cells[11] = 1.;
    let mut authored = stage_graph(cells, false, false, false);
    if let ir::ProcessingSignalNode::Matrix(matrix) =
        &mut authored.processing_scopes[0].signal.as_mut().unwrap().nodes[0]
    {
        matrix.cells.fill(0.);
        for i in 0..6 {
            matrix.cells[i * 6 + i] = 1.;
        }
    }
    let mut actual = runtime(&authored, decode(wav(6, false, true, &[])));
    actual.trigger(input(1), 60, 1.).unwrap();
    let out = render(&mut actual, 1024, 3);
    assert!(out.iter().all(|v| v[0] == 0.));
    assert!(out.iter().any(|v| v[1].abs() > 1e-5));
    assert_eq!(
        (actual.voice_count(), actual.processing_owner_count()),
        (1, 1)
    );
}
#[test]
fn width_stage_tail_overflow_and_matrix_binding_to_a_stage_refuse_without_binding() {
    let mut cells = vec![0.; 12];
    cells[2] = 1.;
    cells[11] = 1.;
    for tail_overflow in [false, true] {
        let mut authored = stage_graph(cells.clone(), false, false, true);
        let signal = authored.processing_scopes[0].signal.as_mut().unwrap();
        if tail_overflow {
            if let ir::ProcessingSignalNode::OnePole(filter) = &mut signal.nodes[1] {
                filter.tail_frames = u32::MAX;
            }
            let mut extra = owner_filter(false, 1, false);
            if let ir::ProcessingSignalNode::OnePole(filter) = &mut extra {
                filter.cutoff_hz.constant = 200.;
            }
            signal.nodes.insert(2, extra);
        } else {
            signal.controls.push(ir::ProcessingCellBinding {
                node: 1,
                input: 0,
                output: 0,
                control: "wide-gain".into(),
            });
        }
        let called = std::cell::Cell::new(false);
        let result = lower_sources(
            &authored,
            48000,
            vec![decode(wav(6, false, false, &[]))],
            |_, p| {
                called.set(true);
                Ok::<_, LowerError>(p)
            },
        );
        assert!(result.is_err());
        assert!(!called.get());
    }
}
#[test]
fn owner_gain_keeps_f64_coefficient_until_the_declared_f32_outlet() {
    let mut cells = vec![0.; 12];
    cells[3] = 1.;
    cells[11] = 1.;
    let mut authored = instrument(cells, false);
    let gain = 1.0 + 2f64.powi(-24);
    authored.processing_scopes[0]
        .signal
        .as_mut()
        .unwrap()
        .nodes
        .insert(0, owner_gain(gain, false));
    let raw = raw_six(&authored, 256, 64);
    let expected: Vec<_> = raw
        .iter()
        .map(|v| {
            [
                (f64::from(v[3]) * gain) as f32,
                (f64::from(v[5]) * gain) as f32,
            ]
        })
        .collect();
    assert!(
        raw.iter()
            .zip(&expected)
            .any(|(raw, expected)| expected[0].to_bits() != (raw[3] * gain as f32).to_bits())
    );
    let mut actual = runtime(&authored, decode(wav(6, false, false, &[])));
    actual.trigger(input(1), 60, 1.).unwrap();
    exact(&render(&mut actual, 256, 17), &expected);
}

fn expressive_bases(rt: &mut Runtime, gain: f64, cutoff: f64) {
    let mut writes=[
        ControlWrite{id:ir_control_id("wide-gain"),value:ControlValue::Real(gain)},
        ControlWrite{id:ir_control_id("wide-cutoff"),value:ControlValue::Real(cutoff)},
    ];
    writes.sort_unstable_by_key(|write|write.id);
    rt.edit_controls(rt.active_plan(),None,&writes).unwrap();
}

fn expressive_graph(cells: Vec<f64>) -> ir::Instrument {
    let mut authored = instrument(cells, false);
    // Explicit enhancement after the authored matrix/projection; this fixture
    // has no subsequent legacy stereo scope.chain to mislabel as post-authored.
    let signal = authored.processing_scopes[0].signal.as_mut().unwrap();
    let mut filter = owner_filter(false, 0, true);
    if let ir::ProcessingSignalNode::OnePole(node) = &mut filter {
        node.cutoff_hz.expression = Some(ir::SignalExpressionBinding {
            source: ir::SignalExpressionSource::Timbre,
            law: ir::SignalExpressionLaw::Octaves { zero: -1., one: 1. },
        });
    }
    let mut gain = owner_gain(0.375, true);
    if let ir::ProcessingSignalNode::Gain(node) = &mut gain {
        node.gain.expression = Some(ir::SignalExpressionBinding {
            source: ir::SignalExpressionSource::Pressure,
            law: ir::SignalExpressionLaw::LinearFactor { zero: 0.25, one: 1.75 },
        });
    }
    signal.nodes.extend([filter, gain]);
    for (key, min, max, default) in [("wide-gain", 0., 2., 0.375), ("wide-cutoff", 500., 8000., 1379.)] {
        authored.controls.push(ir::Control { key: key.into(), label: key.into(),
            value: ir::ControlValue::Continuous { min, max, default, unit: ir::ControlUnit::None },
            automation: ir::Automation::None });
    }
    authored.modulators.push(ir::Modulator { scope: ir::Scope::Voice,
        source: ir::ModulationSource::Envelope(ir::Envelope { release: ir::Time::Milliseconds(1.), ..Default::default() }) });
    authored.processing_scopes[0].amplitude = Some(ir::ModulatorRef(0));
    authored
}
fn stage_expression(rt: &mut Runtime, note: sampler_core::NoteId, pressure: u32, timbre: u32, tune: f64) {
    let id = rt.expression_id(note).unwrap(); let mut value = rt.expression(id).unwrap();
    value.pressure = pressure; value.timbre = timbre; value.pitch_semitones = tune;
    rt.set_expression(id, value).unwrap();
}
fn sum_note_outputs(a: &[[f32; 2]], b: &[[f32; 2]]) -> Vec<[f32; 2]> {
    a.iter().zip(b).map(|(a,b)| [a[0]+b[0], a[1]+b[1]]).collect()
}
fn expression_map(bits: u32, zero: f64, one: f64) -> f64 {
    if bits == 0 { zero } else if bits == u32::MAX { one }
    else { (zero + (one-zero) * (f64::from(bits) / f64::from(u32::MAX))).clamp(zero.min(one),zero.max(one)) }
}
fn expressive_oracle(raw: &[[f32; 2]], state: &mut [f64; 2], pressure: u32, timbre: u32,
    base_gain: f64, base_hz: f64) -> Vec<[f32; 2]> {
    let gain = base_gain * expression_map(pressure, 0.25, 1.75);
    let hz = base_hz * expression_map(timbre, -1., 1.).exp2();
    let b = -(-std::f64::consts::TAU * hz / 48000.).exp_m1();
    raw.iter().map(|frame| {
        let mut out = [0.; 2];
        for channel in 0..2 {
            state[channel] += (f64::from(frame[channel])-state[channel]) * b;
            let filtered = state[channel] as f32;
            out[channel] = (f64::from(filtered) * gain) as f32;
        }
        out
    }).collect()
}
#[test]
fn expressive_stages_match_independent_note_expression_matched_input_oracle() {
    let mut cells = vec![0.; 12]; cells[3] = 1.; cells[11] = 1.;
    let authored = expressive_graph(cells);
    let mut input_ir = authored.clone(); input_ir.processing_scopes[0].signal.as_mut().unwrap().nodes.truncate(1);
    let mut actual = runtime(&authored, decode(wav(6, false, false, &[])));
    let mut input_ref = runtime(&input_ir, decode(wav(6, false, false, &[])));
    let a = actual.trigger(input(1), 60, 1.).unwrap(); let b = input_ref.trigger(input(1), 60, 1.).unwrap();
    let mut state = [0.; 2];
    for (frames, pressure, timbre, tune, base_gain, base_hz) in [
        (97, 0x2000_0123, 0x6000_4567, -1.25, 0.375, 1379.),
        (511, 0xe000_4567, 0xc000_0123, 2.5, 0.5, 3000.),
        (129, u32::MAX, 0, 0., 0.25, 2000.),
    ] {
        stage_expression(&mut actual,a,pressure,timbre,tune); stage_expression(&mut input_ref,b,pressure,timbre,tune);
        expressive_bases(&mut actual,base_gain,base_hz);
        let raw = render(&mut input_ref,frames,64);
        let expected = expressive_oracle(&raw,&mut state,pressure,timbre,base_gain,base_hz);
        close(&render(&mut actual,frames,17),&expected);
        let expression = actual.expression(actual.expression_id(a).unwrap()).unwrap();
        assert_eq!((expression.pressure,expression.timbre,expression.pitch_semitones),(pressure,timbre,tune));
        assert_eq!(actual.control_value(actual.active_plan(),ir_control_id("wide-gain")),Ok(ControlValue::Real(base_gain)));
        assert_eq!(actual.control_value(actual.active_plan(),ir_control_id("wide-cutoff")),Ok(ControlValue::Real(base_hz)));
    }
}
#[test]
fn overlapping_same_key_expressive_notes_keep_independent_filters_and_global_bases() {
    let mut cells = vec![0.; 12]; cells[2] = 1.; cells[11] = 1.; let authored = expressive_graph(cells);
    let mut actual = runtime(&authored,decode(wav(6,false,false,&[])));
    let mut first = runtime(&authored,decode(wav(6,false,false,&[])));
    let mut second = runtime(&authored,decode(wav(6,false,false,&[])));
    let a = actual.trigger(input(101),60,1.).unwrap(); let b = actual.trigger(input(202),60,1.).unwrap();
    let ar = first.trigger(input(101),60,1.).unwrap(); let br = second.trigger(input(202),60,1.).unwrap();
    assert_ne!(actual.expression_id(a),actual.expression_id(b));
    stage_expression(&mut actual,a,0x2000_0123,0x4000_4567,-2.); stage_expression(&mut first,ar,0x2000_0123,0x4000_4567,-2.);
    stage_expression(&mut actual,b,0xe000_4567,0xd000_0123,3.); stage_expression(&mut second,br,0xe000_4567,0xd000_0123,3.);
    let plan = actual.active_plan(); let revision = actual.control_revision(plan).unwrap();
    let second_expression = actual.expression(actual.expression_id(b).unwrap()).unwrap();
    close(&render(&mut actual,97,17),&sum_note_outputs(&render(&mut first,97,64),&render(&mut second,97,31)));
    stage_expression(&mut actual,a,0xa000_0001,0x1000_0001,1.); stage_expression(&mut first,ar,0xa000_0001,0x1000_0001,1.);
    assert_eq!(actual.expression(actual.expression_id(b).unwrap()).unwrap(),second_expression);
    assert_eq!(actual.control_revision(plan),Ok(revision));
    assert_eq!(actual.control_value(plan,ir_control_id("wide-gain")),Ok(ControlValue::Real(0.375)));
    assert_eq!(actual.control_value(plan,ir_control_id("wide-cutoff")),Ok(ControlValue::Real(1379.)));
    for rt in [&mut actual,&mut first,&mut second] {
        expressive_bases(rt,0.75,4000.);
    }
    close(&render(&mut actual,511,3),&sum_note_outputs(&render(&mut first,511,64),&render(&mut second,511,17)));
    actual.note_off_exact(a,None).unwrap(); first.note_off_exact(ar,None).unwrap();
    close(&render(&mut actual,96,7),&sum_note_outputs(&render(&mut first,96,64),&render(&mut second,96,31)));
    assert_eq!((actual.voice_count(),actual.processing_owner_count()),(1,1));
    assert_eq!(actual.note_plan(b),Ok(plan));
}
#[test]
fn expressive_owner_retains_its_generation_mapping_after_new_plan_publication() {
    let mut cells=vec![0.;12];cells[3]=1.;cells[11]=1.;let authored=expressive_graph(cells);
    let p=prepare(&authored,decode(wav(6,false,false,&[])));let limits=Limits::for_plan(&p,8,16);
    let (mut actual,mut control)=Runtime::with_plan_updates(p,limits,2,1).unwrap();
    let old=actual.active_plan();let note=actual.trigger(input(1),60,1.).unwrap();
    let mut first=runtime(&authored,decode(wav(6,false,false,&[])));let first_note=first.trigger(input(1),60,1.).unwrap();
    stage_expression(&mut actual,note,0x4000_0001,0x2000_0001,-1.);stage_expression(&mut first,first_note,0x4000_0001,0x2000_0001,-1.);
    close(&render(&mut actual,97,17),&render(&mut first,97,64));
    let mut incoming=authored.clone();
    if let ir::ProcessingSignalNode::OnePole(filter)=&mut incoming.processing_scopes[0].signal.as_mut().unwrap().nodes[1] {
        filter.cutoff_hz.expression.as_mut().unwrap().law=ir::SignalExpressionLaw::Octaves{zero:-2.,one:0.};
    }
    control.submit(Box::new(prepare(&incoming,decode(wav(6,false,false,&[]))))).unwrap();
    support::without_heap(||{actual.poll_plan_update().unwrap();});let new=actual.active_plan();assert_ne!(old,new);
    assert_eq!(actual.note_plan(note),Ok(old));assert_eq!(actual.collect_retired_plans(),0);
    let next=actual.trigger(input(2),60,1.).unwrap();let mut second=runtime(&incoming,decode(wav(6,false,false,&[])));
    // Match the absolute modulation grid before this reference voice starts.
    // The real new generation starts at 97, not at this fresh Runtime's zero.
    assert_eq!(actual.now(),97);assert_eq!(second.now(),0);
    let silence=render(&mut second,97,31);assert!(silence.iter().flatten().all(|v|*v==0.));
    assert_eq!(second.now(),actual.now());assert_eq!(second.voice_count(),0);
    let second_note=second.trigger(input(2),60,1.).unwrap();
    stage_expression(&mut actual,next,0xf000_0001,0xf000_0001,2.);stage_expression(&mut second,second_note,0xf000_0001,0xf000_0001,2.);
    stage_expression(&mut actual,note,0x8000_0001,0x6000_0001,-0.5);stage_expression(&mut first,first_note,0x8000_0001,0x6000_0001,-0.5);
    close(&render(&mut actual,400,31),&sum_note_outputs(&render(&mut first,400,64),&render(&mut second,400,17)));
    actual.note_off_exact(note,None).unwrap();render(&mut actual,96,7);actual.flush_ended(|_|true);
    assert_eq!(actual.collect_retired_plans(),1);assert_eq!((actual.voice_count(),actual.processing_owner_count()),(1,1));
}
#[test]
fn expressive_endpoints_cross_the_full_global_base_domain_before_publication() {
    let mut cells=vec![0.;12];cells[3]=1.;cells[11]=1.;
    for case in 0..5 {
        let mut authored=expressive_graph(cells.clone());
        let signal=authored.processing_scopes[0].signal.as_mut().unwrap();
        if case==3 {
            if let ir::ProcessingSignalNode::Gain(node)=&mut signal.nodes[2] {
                node.gain.expression.as_mut().unwrap().law=ir::SignalExpressionLaw::Octaves{zero:-1.,one:1.};
            }
        } else if case==4 {
            if let ir::ProcessingSignalNode::Gain(node)=&mut signal.nodes[2] {
                node.gain.expression.as_mut().unwrap().law=ir::SignalExpressionLaw::LinearFactor{zero:0.,one:f64::from(f32::MAX)};
            }
        } else if let ir::ProcessingSignalNode::OnePole(node)=&mut signal.nodes[1] {
            node.cutoff_hz.expression.as_mut().unwrap().law=match case {
                0=>ir::SignalExpressionLaw::Octaves{zero:0.,one:2.}, // 8000*4 exceeds actual Nyquist.
                1=>ir::SignalExpressionLaw::Octaves{zero:-2000.,one:-1999.},
                _=>ir::SignalExpressionLaw::Octaves{zero:-f64::MAX,one:f64::MAX},
            };
        }
        let called=std::cell::Cell::new(false);
        let result=lower_sources(&authored,48000,vec![decode(wav(6,false,false,&[]))],|_,p|{called.set(true);Ok::<_,LowerError>(p)});
        assert!(result.is_err());assert!(!called.get());
    }
}
#[test]
fn expressive_two_note_updates_base_capture_release_and_render_are_heap_free() {
    let mut cells=vec![0.;12];cells[3]=1.;cells[11]=1.;let authored=expressive_graph(cells);
    let mut actual=runtime(&authored,decode(wav(6,false,false,&[])));let plan=actual.active_plan();
    support::without_heap(||{
        let a=actual.trigger(input(1),60,1.).unwrap();let b=actual.trigger(input(2),60,1.).unwrap();
        stage_expression(&mut actual,a,0x2000_0001,0x3000_0001,-1.);stage_expression(&mut actual,b,0xe000_0001,0xc000_0001,2.);
        actual.render(&mut [[0.;2];97]).unwrap();
        actual.edit_controls(plan,None,&[ControlWrite{id:ir_control_id("wide-cutoff"),value:ControlValue::Real(3000.)}]).unwrap();
        let mut captured=[ControlWrite{id:ir_control_id("wide-cutoff"),value:ControlValue::Real(0.)};2];
        assert_eq!(actual.capture_controls(plan,&mut captured),Ok((2,1)));
        stage_expression(&mut actual,a,0xa000_0001,0x8000_0001,0.5);actual.render(&mut [[0.;2];65]).unwrap();
        actual.note_off_exact(a,None).unwrap();actual.note_off_exact(b,None).unwrap();actual.render(&mut [[0.;2];96]).unwrap();
    });
    assert_eq!((actual.voice_count(),actual.processing_owner_count(),actual.nonfinite_frames(),actual.stream_underruns()),(0,0,0,0));
}
#[test]
fn queued_and_scheduled_global_bases_keep_both_note_expression_factors_live() {
    let mut cells=vec![0.;12];cells[3]=1.;cells[11]=1.;let authored=expressive_graph(cells);
    let rt=runtime(&authored,decode(wav(6,false,false,&[])));let (mut actual,mut queue)=rt.with_control_updates(1,2).unwrap();
    let mut first=runtime(&authored,decode(wav(6,false,false,&[])));let mut second=runtime(&authored,decode(wav(6,false,false,&[])));
    let a=actual.trigger(input(1),60,1.).unwrap();let b=actual.trigger(input(2),60,1.).unwrap();
    let ar=first.trigger(input(1),60,1.).unwrap();let br=second.trigger(input(2),60,1.).unwrap();
    stage_expression(&mut actual,a,0x2000_0001,0x3000_0001,-1.);stage_expression(&mut first,ar,0x2000_0001,0x3000_0001,-1.);
    stage_expression(&mut actual,b,0xe000_0001,0xc000_0001,2.);stage_expression(&mut second,br,0xe000_0001,0xc000_0001,2.);
    close(&render(&mut actual,64,13),&sum_note_outputs(&render(&mut first,64,64),&render(&mut second,64,31)));
    let plan=actual.active_plan();let gain=ir_control_id("wide-gain");let cutoff=ir_control_id("wide-cutoff");
    let mut recall=[ControlWrite{id:gain,value:ControlValue::Real(0.)};2];
    assert_eq!(actual.capture_controls(plan,&mut recall),Ok((2,0)));
    recall.iter_mut().find(|write|write.id==gain).unwrap().value=ControlValue::Real(0.75);
    queue.submit(sampler_core::ControlRequest{plan,expected_revision:Some(0),operation:sampler_core::ControlOperation::Recall(Box::from(recall))}).unwrap();
    assert_eq!(actual.poll_control_update(),Ok(Some(1)));assert_eq!(queue.reply().unwrap().result,Ok((2,1)));
    for rt in [&mut first,&mut second] { rt.edit_controls(rt.active_plan(),None,&[ControlWrite{id:gain,value:ControlValue::Real(0.75)}]).unwrap(); }
    for rt in [&mut actual,&mut first,&mut second] {
        rt.schedule_event(160,sampler_core::Event::Control(rt.active_plan(),ControlWrite{id:cutoff,value:ControlValue::Real(5000.)})).unwrap();
    }
    close(&render(&mut actual,256,17),&sum_note_outputs(&render(&mut first,256,64),&render(&mut second,256,31)));
    let mut snapshot=[ControlWrite{id:gain,value:ControlValue::Real(0.)};2];
    assert_eq!(actual.capture_controls(plan,&mut snapshot),Ok((2,2)));
    assert!(snapshot.iter().any(|w|w.id==gain&&w.value==ControlValue::Real(0.75)));
    assert!(snapshot.iter().any(|w|w.id==cutoff&&w.value==ControlValue::Real(5000.)));
}

#[test]
fn ordinary_source_aware_finish_keeps_one_logical_asset_and_the_existing_runtime_laws() {
    let mut cells=vec![0.;12];cells[3]=1.;cells[11]=-0.5;let authored=instrument(cells.clone(),false);
    let source=decode(wav(6,false,false,&[]));let id=source.as_source().id();
    let loaded=sampler_kontakt::finish_sources(authored.clone(),vec![source],vec!["owned.wav".into()],&sampler_kontakt::Options::default()).unwrap();
    assert_eq!(loaded.instrument.assets.len(),1);assert_eq!(loaded.instrument.zones.len(),1);assert_eq!(loaded.plan.source_storage(0).unwrap().id(),id);
    let limits=Limits::for_plan(&loaded.plan,8,16);let mut actual=Runtime::new(loaded.plan,limits).unwrap();
    let mut expected=runtime(&authored,decode(wav(6,false,false,&[])));actual.trigger(input(1),60,1.).unwrap();expected.trigger(input(1),60,1.).unwrap();
    exact(&render(&mut actual,1024,17),&render(&mut expected,1024,64));assert_eq!((actual.voice_count(),actual.processing_owner_count()),(1,1));
}
