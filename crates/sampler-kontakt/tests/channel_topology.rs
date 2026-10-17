//! Normal encoded source -> logical storage -> explicit signal graph -> Runtime.
//! Generic authored graph witnesses, not native UVI topology/projection evidence.
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
use sampler_core::{ControlValue, ControlWrite, Error, Input, Limits, Protocol, Runtime, SourceStorage};
use sampler_core::lower::{lower_sources, LowerError, ir_control_id};
use sampler_ir as ir;
use sampler_kontakt::EncodedChannelReader;
use std::io::Cursor;
use symphonia::core::meta::MetadataOptions;
const N: usize = 512;
fn sample(frame: usize, channel: usize, duplicate: bool) -> i16 {
    let channel = if duplicate && channel == 3 { 2 } else { channel };
    ((frame * (17 + channel * 2) + channel * 73) % 4096) as i16 - 2048
}
fn value(frame: usize, channel: usize, duplicate: bool) -> f32 { f32::from(sample(frame, channel, duplicate)) / 32768. }
fn wav(channels: usize, floating: bool, duplicate: bool, cells: &[f64]) -> Vec<u8> {
    let bytes = if floating { 4 } else { 2 }; let size = N * channels * bytes;
    let mut out = Vec::new(); out.extend_from_slice(b"RIFF"); out.extend_from_slice(&(36 + size as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt "); out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&(if floating { 3u16 } else { 1u16 }).to_le_bytes());
    out.extend_from_slice(&(channels as u16).to_le_bytes()); out.extend_from_slice(&48000u32.to_le_bytes());
    out.extend_from_slice(&(48000 * channels as u32 * bytes as u32).to_le_bytes());
    out.extend_from_slice(&((channels * bytes) as u16).to_le_bytes()); out.extend_from_slice(&((bytes * 8) as u16).to_le_bytes());
    out.extend_from_slice(b"data"); out.extend_from_slice(&(size as u32).to_le_bytes());
    for frame in 0..N { for channel in 0..channels {
        if floating {
            let mut sum = 0f32;
            for input in 0..6 { sum += value(frame, input, duplicate) * cells[channel * 6 + input] as f32; }
            out.extend_from_slice(&sum.to_le_bytes());
        } else { out.extend_from_slice(&sample(frame, channel, duplicate).to_le_bytes()); }
    }} out
}
fn decode(bytes: Vec<u8>) -> SourceStorage {
    EncodedChannelReader::open_legacy(Box::new(Cursor::new(bytes)), &MetadataOptions::default()).unwrap()
        .read_resident_storage(1 << 20).unwrap()
}
fn asset() -> ir::Asset { ir::Asset { location: ir::AssetLocation::Path("authored.wav".into()),
    encoding: ir::Encoding::Wav, residency: ir::AssetResidency::Resident, root_key: None, loops: vec![] } }
fn zone() -> ir::Zone { ir::Zone { pitch: ir::KeyTracking::Fixed, velocity: ir::VelocityResponse::None,
    playback: ir::Playback { looping: ir::Looping::Continuous(ir::LoopRange { start: 0, end: N as u64,
        crossfade: ir::Span::Frames(0), alternating: false }), ..Default::default() },
    ..ir::Zone::new(ir::AssetRef(0)) } }
fn instrument(cells: Vec<f64>, controlled: bool) -> ir::Instrument {
    let mut result = ir::Instrument { assets: vec![asset()], zones: vec![zone()],
        processing_scopes: vec![ir::ProcessingScope { members: vec![ir::ZoneRef(0)],
            sum: ir::ProcessingSumLaw::OrderedF32, cadence: ir::ProcessingCadence::CoreCell64,
            chain: None, amplitude: None, output: None,
            signal: Some(ir::ProcessingSignal { source_law: ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,
                input: ir::SignalLayout { channels: 6 }, nodes: vec![ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix {
                    input: ir::SignalLayout { channels: 6 }, output: ir::SignalLayout { channels: 2 }, cells,
                    arithmetic: ir::MatrixArithmetic::SeparateMulAddF32 })], controls: vec![] }) }],
        ..Default::default() };
    if controlled {
        result.controls.push(ir::Control { key: "upper-cell".into(), label: "Upper cell".into(),
            value: ir::ControlValue::Continuous { min: -1., max: 1., default: 0.25, unit: ir::ControlUnit::None }, automation: ir::Automation::None });
        result.processing_scopes[0].signal.as_mut().unwrap().controls.push(ir::ProcessingCellBinding {
            node: 0, input: 3, output: 0, control: "upper-cell".into() });
    } result
}
fn prepare(ir: &ir::Instrument, source: SourceStorage) -> sampler_core::Prepared {
    lower_sources(ir, 48000, vec![source], |_, p| Ok::<_, LowerError>(p)).unwrap()
}
fn runtime(ir: &ir::Instrument, source: SourceStorage) -> Runtime {
    let prepared = prepare(ir, source); let limits = Limits::for_plan(&prepared, 8, 16);
    Runtime::new(prepared, limits).unwrap()
}
fn reference(cells: &[f64], duplicate: bool) -> Runtime {
    let ir = ir::Instrument { assets: vec![asset()], zones: vec![zone()], ..Default::default() };
    runtime(&ir, decode(wav(2, true, duplicate, cells)))
}
fn input(id: i32) -> Input { Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0,
    key: 60, external_id: Some(id) } }
fn render(runtime: &mut Runtime, count: usize, fragment: usize) -> Vec<[f32; 2]> {
    let mut out = vec![[0.; 2]; count]; for block in out.chunks_mut(fragment) { runtime.render(block).unwrap(); }
    assert_eq!((runtime.nonfinite_frames(), runtime.stream_underruns()), (0, 0)); out
}
fn exact(a: &[[f32; 2]], b: &[[f32; 2]]) {
    assert_eq!(a.len(), b.len());
    for (i, (a, b)) in a.iter().zip(b).enumerate() { assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits), "frame {i}"); }
}
#[test]
fn every_upper_original_channel_reaches_each_stereo_output_through_normal_runtime() {
    for upper in 2..6 { for output in 0..2 {
        let mut cells = vec![0.; 12]; cells[output * 6 + upper] = 1.; cells[(1 - output) * 6] = 0.5;
        let mut actual = runtime(&instrument(cells.clone(), false), decode(wav(6, false, false, &[])));
        let mut expected = reference(&cells, false);
        actual.trigger(input(1), 60, 1.).unwrap(); expected.trigger(input(1), 60, 1.).unwrap();
        exact(&render(&mut actual, 1024, 17), &render(&mut expected, 1024, 257));
        assert_eq!((actual.voice_count(), actual.processing_owner_count()), (1, 1));
    }}
}
#[test]
fn signed_multi_input_matrix_and_exact_cancellation_keep_a_nonzero_witness() {
    for (duplicate, cells) in [(false, vec![0.25,-0.5,1.,0.25,-0.25,0.5, -1.,0.25,0.,0.5,-0.25,0.25]),
        (true, vec![0.,0.,1.,-1.,0.,0., 0.,0.,0.,0.,0.,1.])] {
        let mut actual = runtime(&instrument(cells.clone(), false), decode(wav(6, false, duplicate, &[])));
        let mut expected = reference(&cells, duplicate);
        actual.trigger(input(1), 60, 1.).unwrap(); expected.trigger(input(1), 60, 1.).unwrap();
        let out = render(&mut actual, 1024, 31); exact(&out, &render(&mut expected, 1024, 64));
        assert!(out.iter().any(|frame| frame[1] != 0.));
        if duplicate { assert!(out.iter().all(|frame| frame[0] == 0.)); }
    }
}
#[test]
fn live_upper_cell_changes_the_same_held_source_and_invalid_type_keeps_own_state() {
    let mut cells = vec![0.; 12]; cells[3] = 0.25; cells[11] = 1.;
    let mut actual = runtime(&instrument(cells.clone(), true), decode(wav(6, false, false, &[])));
    let note = actual.trigger(input(1), 60, 1.).unwrap();
    let mut before = reference(&cells, false); before.trigger(input(1), 60, 1.).unwrap();
    exact(&render(&mut actual, 123, 7), &render(&mut before, 123, 64));
    cells[3] = -0.5;
    let mut after = reference(&cells, false); after.trigger(input(1), 60, 1.).unwrap(); render(&mut after, 123, 17);
    let mut owners = vec![]; actual.visit_processing_owners(|owner| owners.push(owner.id));
    let plan = actual.active_plan(); let id = ir_control_id("upper-cell"); let revision = actual.control_revision(plan).unwrap();
    actual.edit_controls(plan, Some(revision), &[ControlWrite { id, value: ControlValue::Real(-0.5) }]).unwrap();
    let retained = actual.control_revision(plan).unwrap();
    assert_eq!(actual.edit_controls(plan, Some(retained), &[ControlWrite { id, value: ControlValue::Toggle(false) }]), Err(Error::InvalidInput));
    assert_eq!(actual.control_revision(plan).unwrap(), retained);
    assert_eq!(actual.control_value(plan, id), Ok(ControlValue::Real(-0.5)));
    exact(&render(&mut actual, 333, 1), &render(&mut after, 333, 257));
    let mut kept = vec![]; actual.visit_processing_owners(|owner| kept.push(owner.id)); assert_eq!(owners, kept);
    assert_eq!(actual.note_plan(note), Ok(plan)); assert_eq!((actual.voice_count(), actual.processing_owner_count()), (1, 1));
}
#[test]
fn wide_default_members_keep_reading_through_shared_release_and_retire() {
    let mut cells = vec![0.; 12]; cells[3] = 1.; cells[11] = 1.;
    let mut ir = instrument(cells, false);
    ir.modulators.push(ir::Modulator { scope: ir::Scope::Voice,
        source: ir::ModulationSource::Envelope(ir::Envelope { release: ir::Time::Milliseconds(1.), ..Default::default() }) });
    ir.processing_scopes[0].amplitude = Some(ir::ModulatorRef(0));
    let mut actual = runtime(&ir, decode(wav(6, false, false, &[]))); let note = actual.trigger(input(1), 60, 1.).unwrap();
    render(&mut actual, 123, 17); actual.note_off_exact(note, None).unwrap();
    let out = render(&mut actual, 64, 7);
    assert!(out[..16].iter().flatten().any(|value| *value != 0.));
    assert!(out[16..47].iter().flatten().any(|value| *value != 0.));
    assert!(out[49..].iter().all(|frame| *frame == [0.; 2]));
    assert_eq!((actual.voice_count(), actual.processing_owner_count()), (0, 0));
}
#[test]
fn later_builder_cannot_unbind_or_change_a_matrix_control_domain() {
    for mode in 0..3 {
        let mut cells = vec![0.; 12]; cells[3] = 0.25; cells[11] = 1.;
        let prepared = prepare(&instrument(cells, true), decode(wav(6, false, false, &[])));
        let mut controls = prepared.controls().to_vec();
        match mode { 0 => controls.clear(), 1 => { controls[0].domain = sampler_core::ControlDomain::Toggle;
            controls[0].default = ControlValue::Toggle(false); }, _ => controls[0].domain = sampler_core::ControlDomain::Real { min: -2., max: 2. } }
        assert!(matches!(prepared.with_controls(controls), Err(Error::InvalidInput)));
    }
}

#[test]
fn queued_recall_scheduled_write_and_snapshot_use_the_same_upper_cell_lane() {
    let mut cells = vec![0.; 12]; cells[3] = 0.25; cells[11] = 1.;
    let actual = runtime(&instrument(cells.clone(), true), decode(wav(6, false, false, &[])));
    let (mut actual, mut queue) = actual.with_control_updates(1, 1).unwrap();
    let plan = actual.active_plan(); let id = ir_control_id("upper-cell");
    let note = actual.trigger(input(1), 60, 1.).unwrap();
    let mut expected = reference(&cells, false); expected.trigger(input(1), 60, 1.).unwrap();
    exact(&render(&mut actual, 64, 13), &render(&mut expected, 64, 64));
    let mut snapshot = [ControlWrite { id, value: ControlValue::Real(99.) }];
    assert_eq!(actual.capture_controls(plan, &mut snapshot), Ok((1, 0)));
    assert_eq!(snapshot[0].value, ControlValue::Real(0.25));
    queue.submit(sampler_core::ControlRequest { plan, expected_revision: Some(0),
        operation: sampler_core::ControlOperation::Recall(Box::from([ControlWrite { id, value: ControlValue::Real(-0.5) }])) }).unwrap();
    assert_eq!(actual.poll_control_update(), Ok(Some(1)));
    assert_eq!(queue.reply().unwrap().result, Ok((1, 1)));
    cells[3] = -0.5;
    let mut changed = reference(&cells, false); changed.trigger(input(1), 60, 1.).unwrap(); render(&mut changed, 64, 17);
    exact(&render(&mut actual, 59, 7), &render(&mut changed, 59, 64));
    actual.schedule_event(160, sampler_core::Event::Control(plan, snapshot[0])).unwrap();
    exact(&render(&mut actual, 37, 31), &render(&mut changed, 37, 64));
    let mut original = reference(&{ let mut cells = cells.clone(); cells[3] = 0.25; cells }, false);
    original.trigger(input(1), 60, 1.).unwrap(); render(&mut original, 160, 17);
    exact(&render(&mut actual, 353, 1), &render(&mut original, 353, 257));
    assert_eq!(actual.note_plan(note), Ok(plan));
    assert_eq!(actual.control_value(plan, id), Ok(snapshot[0].value));
    assert_eq!(actual.control_revision(plan), Ok(2));
}

fn shared_chain() -> ir::Chain {
    ir::Chain { scope: ir::Scope::Voice,
        pre_amplitude: vec![ir::Processor::Color(ir::fx::color::Config::rectifier(ir::Rectifier::Full)),
            ir::Processor::Filter(ir::Filter { kind: ir::FilterKind::LowPass { poles: 2 },
                cutoff: ir::Frequency::Hertz(2300.), resonance: ir::Resonance::Q(0.65) })],
        post_amplitude: vec![] }
}
fn close(actual: &[[f32; 2]], expected: &[[f32; 2]]) {
    assert_eq!(actual.len(), expected.len());
    for (frame, (a, b)) in actual.iter().zip(expected).enumerate() {
        for channel in 0..2 { assert!((a[channel] - b[channel]).abs() <= 2e-6,
            "frame {frame} channel {channel}: {} != {}", a[channel], b[channel]); }
    }
}
#[test]
fn three_wide_sources_keep_independent_clocks_before_one_shared_section_per_attack() {
    let mut cells = vec![0.; 12]; cells[2] = 1.; cells[11] = 1.;
    let mut authored = instrument(cells.clone(), false);
    // Three source instances share one immutable logical source/storage identity,
    // while start/tune and advancing cursor state remain independent.
    authored.zones = vec![zone(), zone(), zone()];
    authored.zones[1].playback.start = 5;
    authored.zones[1].tune = ir::Pitch::Semitones(12.);
    authored.zones[2].playback.start = 17;
    authored.zones[2].tune = ir::Pitch::Semitones(-12.);
    authored.processing_scopes[0].members = vec![ir::ZoneRef(2), ir::ZoneRef(0), ir::ZoneRef(1)];
    authored.chains = vec![shared_chain()]; authored.processing_scopes[0].chain = Some(ir::ChainRef(0));
    let mut stereo = authored.clone(); stereo.processing_scopes[0].signal = None;
    let sources = vec![decode(wav(6, false, false, &[]))];
    let expected_sources = vec![decode(wav(2, true, false, &cells))];
    let prepared = lower_sources(&authored, 48000, sources, |_, p| Ok::<_, LowerError>(p)).unwrap();
    let limits = Limits::for_plan(&prepared, 16, 32); let mut actual = Runtime::new(prepared, limits).unwrap();
    let prepared = lower_sources(&stereo, 48000, expected_sources, |_, p| Ok::<_, LowerError>(p)).unwrap();
    let limits = Limits::for_plan(&prepared, 16, 32); let mut expected = Runtime::new(prepared, limits).unwrap();
    actual.trigger(input(1), 60, 1.).unwrap(); expected.trigger(input(1), 60, 1.).unwrap();
    close(&render(&mut actual, 97, 13), &render(&mut expected, 97, 64));
    assert_eq!((actual.voice_count(), actual.processing_owner_count()), (3, 1));
    actual.trigger(input(2), 60, 1.).unwrap(); expected.trigger(input(2), 60, 1.).unwrap();
    close(&render(&mut actual, 1024, 31), &render(&mut expected, 1024, 127));
    assert_eq!((actual.voice_count(), actual.processing_owner_count()), (6, 2));
}

#[test]
fn explicit_six_channel_matrix_retains_width_until_an_authored_second_projection() {
    let mut cells = vec![0.; 12]; cells[3] = 0.5; cells[11] = -0.25;
    let mut authored = instrument(cells.clone(), false);
    let signal = authored.processing_scopes[0].signal.as_mut().unwrap();
    let mut identity = vec![0.; 36]; for channel in 0..6 { identity[channel * 6 + channel] = 1.; }
    signal.nodes.insert(0, ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix {
        input: ir::SignalLayout { channels: 6 }, output: ir::SignalLayout { channels: 6 },
        cells: identity, arithmetic: ir::MatrixArithmetic::SeparateMulAddF32 }));
    let mut actual = runtime(&authored, decode(wav(6, false, false, &[])));
    let mut expected = reference(&cells, false);
    actual.trigger(input(1), 60, 1.).unwrap(); expected.trigger(input(1), 60, 1.).unwrap();
    exact(&render(&mut actual, 1024, 23), &render(&mut expected, 1024, 256));
}

#[test]
fn resident_wide_trigger_control_render_release_and_retirement_do_not_touch_the_heap() {
    let mut cells = vec![0.; 12]; cells[3] = 0.25; cells[11] = 1.;
    let mut authored = instrument(cells, true);
    authored.modulators.push(ir::Modulator { scope: ir::Scope::Voice,
        source: ir::ModulationSource::Envelope(ir::Envelope { release: ir::Time::Milliseconds(1.), ..Default::default() }) });
    authored.processing_scopes[0].amplitude = Some(ir::ModulatorRef(0));
    let mut actual = runtime(&authored, decode(wav(6, false, false, &[])));
    let plan = actual.active_plan(); let id = ir_control_id("upper-cell");
    let mut out = [[0.; 2]; 128];
    support::without_heap(|| {
        let note = actual.trigger(input(1), 60, 1.).unwrap();
        actual.render(&mut out[..97]).unwrap();
        actual.edit_controls(plan, Some(0), &[ControlWrite { id, value: ControlValue::Real(-0.5) }]).unwrap();
        actual.render(&mut out[..123]).unwrap();
        assert!(out[..123].iter().flatten().any(|value| *value != 0.));
        actual.note_off_exact(note, None).unwrap();
        actual.render(&mut out).unwrap();
        assert_eq!((actual.voice_count(), actual.processing_owner_count()), (0, 0));
    });
    assert_eq!((actual.nonfinite_frames(), actual.stream_underruns()), (0, 0));
}

#[test]
fn normal_voice_growth_preserves_live_channel_state_and_prepares_new_source_slots() {
    let mut cells = vec![0.; 12]; cells[3] = 0.25; cells[11] = 1.;
    let mut authored = instrument(cells.clone(), false);
    authored.zones.push(zone()); authored.processing_scopes[0].members.push(ir::ZoneRef(1));
    let prepared = prepare(&authored, decode(wav(6, false, false, &[])));
    let limits = Limits::for_plan(&prepared, 16, 2);
    let (mut actual, mut control) = Runtime::with_plan_updates(prepared, limits, 2, 1).unwrap();
    let reference_plan = prepare(&authored, decode(wav(6, false, false, &[])));
    let limits = Limits::for_plan(&reference_plan, 8, 16); let mut expected = Runtime::new(reference_plan, limits).unwrap();
    actual.trigger(input(1), 60, 1.).unwrap(); expected.trigger(input(1), 60, 1.).unwrap();
    exact(&render(&mut actual, 97, 13), &render(&mut expected, 97, 64));
    assert_eq!(actual.voice_count(), 2); assert_eq!(control.grow_voices(4), Ok(4));
    support::without_heap(|| { actual.render(&mut []).unwrap(); });
    assert_eq!(actual.voice_capacity(), 4);
    actual.trigger(input(2), 60, 1.).unwrap(); expected.trigger(input(2), 60, 1.).unwrap();
    exact(&render(&mut actual, 1024, 31), &render(&mut expected, 1024, 257));
    assert_eq!((actual.voice_count(), actual.processing_owner_count()), (4, 2));
}

#[test]
fn retained_generation_owns_its_original_family_and_matrix_after_new_plan_publication() {
    let mut cells = vec![0.; 12]; cells[3] = 0.25; cells[11] = 1.;
    let mut authored = instrument(cells.clone(), true);
    authored.modulators.push(ir::Modulator { scope: ir::Scope::Voice,
        source: ir::ModulationSource::Envelope(ir::Envelope { release: ir::Time::Milliseconds(1.), ..Default::default() }) });
    authored.processing_scopes[0].amplitude = Some(ir::ModulatorRef(0));
    let prepared = prepare(&authored, decode(wav(6, false, false, &[])));
    let limits = Limits::for_plan(&prepared, 8, 16);
    let (mut actual, mut control) = Runtime::with_plan_updates(prepared, limits, 2, 1).unwrap();
    let old = actual.active_plan(); let old_note = actual.trigger(input(1), 60, 1.).unwrap();
    let mut old_reference = runtime(&authored, decode(wav(6, false, false, &[]))); old_reference.trigger(input(1), 60, 1.).unwrap();
    exact(&render(&mut actual, 97, 13), &render(&mut old_reference, 97, 64));
    let mut incoming = authored.clone();
    if let ir::ControlValue::Continuous { default, .. } = &mut incoming.controls[0].value { *default = -0.5; }
    control.submit(Box::new(prepare(&incoming, decode(wav(6, false, false, &[]))))).unwrap();
    support::without_heap(|| { actual.poll_plan_update().unwrap(); });
    let new = actual.active_plan(); assert_ne!(old, new); assert_eq!(actual.note_plan(old_note), Ok(old));
    assert_eq!(actual.collect_retired_plans(), 0);
    actual.trigger(input(2), 60, 1.).unwrap();
    let mut new_reference = runtime(&incoming, decode(wav(6, false, false, &[]))); new_reference.trigger(input(2), 60, 1.).unwrap();
    let a = render(&mut old_reference, 400, 17); let b = render(&mut new_reference, 400, 31);
    let expected: Vec<_> = a.iter().zip(b).map(|(a,b)| [a[0] + b[0], a[1] + b[1]]).collect();
    close(&render(&mut actual, 400, 23), &expected);
    assert_eq!(actual.control_value(old, ir_control_id("upper-cell")), Ok(ControlValue::Real(0.25)));
    assert_eq!(actual.control_value(new, ir_control_id("upper-cell")), Ok(ControlValue::Real(-0.5)));
    actual.note_off_exact(old_note, None).unwrap(); render(&mut actual, 64, 7);
    actual.flush_ended(|_| true); assert_eq!(actual.collect_retired_plans(), 1);
    assert_eq!((actual.voice_count(), actual.processing_owner_count()), (1, 1));
}

#[test]
fn unsupported_source_pan_is_rejected_before_any_retained_note_or_group_write() {
    let mut cells = vec![0.; 12]; cells[3] = 1.; cells[11] = 1.;
    let authored = instrument(cells.clone(), false);
    let mut actual = runtime(&authored, decode(wav(6, false, false, &[])));
    let mut expected = reference(&cells, false);
    let note = actual.trigger(input(1), 60, 1.).unwrap(); expected.trigger(input(1), 60, 1.).unwrap();
    exact(&render(&mut actual, 97, 13), &render(&mut expected, 97, 64));
    assert_eq!(actual.set_note_param(note, sampler_core::ModTarget::Pan, 0.5, false), Err(Error::UnsupportedSourceLayout));
    assert_eq!(actual.set_group_param(-1, sampler_core::ModTarget::Pan, -0.5, false), Err(Error::UnsupportedSourceLayout));
    assert_eq!(actual.now(), 97);
    exact(&render(&mut actual, 512, 17), &render(&mut expected, 512, 257));
    assert_eq!((actual.voice_count(), actual.processing_owner_count()), (1, 1));
}

#[test]
fn unsupported_source_section_fails_before_behavior_binding_and_cannot_publish_a_partial_graph() {
    let mut cells = vec![0.; 12]; cells[3] = 1.; cells[11] = 1.;
    let mut authored = instrument(cells, false);
    authored.chains.push(shared_chain()); authored.zones[0].chain = Some(ir::ChainRef(0));
    let called = std::cell::Cell::new(false);
    let result = lower_sources(&authored, 48000, vec![decode(wav(6, false, false, &[]))], |_, p| {
        called.set(true); Ok::<_, LowerError>(p)
    });
    assert!(result.is_err()); assert!(!called.get());
}

#[test]
fn wide_source_capacity_failure_keeps_normal_note_family_owner_and_clock_state_unpublished() {
    let mut cells = vec![0.; 12]; cells[3] = 1.; cells[11] = 1.;
    let mut authored = instrument(cells, false);
    authored.zones.push(zone()); authored.zones.push(zone());
    authored.processing_scopes[0].members = vec![ir::ZoneRef(0), ir::ZoneRef(1), ir::ZoneRef(2)];
    let prepared = prepare(&authored, decode(wav(6, false, false, &[])));
    let limits = Limits::for_plan(&prepared, 16, 2); let mut actual = Runtime::new(prepared, limits).unwrap();
    let before = (actual.note_count(), actual.family_count(), actual.expression_count(), actual.voice_count(),
        actual.processing_owner_count(), actual.now(), actual.pending_commands());
    assert_eq!(actual.trigger(input(1), 60, 1.), Err(Error::Capacity));
    assert_eq!((actual.note_count(), actual.family_count(), actual.expression_count(), actual.voice_count(),
        actual.processing_owner_count(), actual.now(), actual.pending_commands()), before);
    assert!(render(&mut actual, 64, 17).iter().all(|frame| *frame == [0.; 2]));
}

#[test]
fn pending_event_key_change_revalidates_retained_source_pan_before_graph_publication() {
    let mut cells = vec![0.; 12]; cells[3] = 1.; cells[11] = 1.;
    let mut authored = instrument(cells, false);
    authored.zones[0].keys = ir::KeyRange { low: 61, high: 61 };
    let mut actual = runtime(&authored, decode(wav(6, false, false, &[])));
    let note = actual.note_on(input(1), 60, 1.).unwrap();
    // At key60 there is no wide source candidate. This is a legal retained
    // stereo operand until the actual pending event is redirected to key61.
    actual.set_note_param(note, sampler_core::ModTarget::Pan, 0.5, false).unwrap();
    let mut event = actual.note_event(note).unwrap(); event.pitch = sampler_core::NotePitch::Key(61);
    actual.edit_note_event(note, event).unwrap();
    let before = (actual.note_count(), actual.family_count(), actual.expression_count(),
        actual.voice_count(), actual.processing_owner_count(), actual.now(), actual.pending_commands());
    assert_eq!(actual.forward_attack(note), Err(Error::UnsupportedSourceLayout));
    assert_eq!((actual.note_count(), actual.family_count(), actual.expression_count(),
        actual.voice_count(), actual.processing_owner_count(), actual.now(), actual.pending_commands()), before);
    assert_eq!((actual.nonfinite_frames(), actual.stream_underruns()), (0, 0));
    actual.set_note_param_with_immediate(note, sampler_core::ModTarget::Pan, 0., false, true).unwrap();
    assert_eq!(actual.forward_attack(note), Ok(true));
    let mut expected = runtime(&authored, decode(wav(6, false, false, &[])));
    expected.trigger(input(2), 61, 1.).unwrap();
    exact(&render(&mut actual, 1024, 13), &render(&mut expected, 1024, 257));
    assert_eq!((actual.voice_count(), actual.processing_owner_count()), (1, 1));
}


#[test]
fn legacy_onset_snapshot_reports_wide_sources_incomplete_without_projecting_a_pack() {
    let mut cells = vec![0.; 12]; cells[3] = 1.; cells[11] = 1.;
    let mut actual = runtime(&instrument(cells, false), decode(wav(6, false, false, &[])));
    actual.trigger(input(1), 60, 1.).unwrap();
    render(&mut actual, 17, 7);
    let before = (actual.voice_count(), actual.processing_owner_count(), actual.now());
    let mut visited = 0;
    assert!(!actual.visit_onset_voices(|_| { visited += 1; true }));
    assert_eq!(visited, 0, "a physical pack must never stand in for a logical source");
    assert_eq!((actual.voice_count(), actual.processing_owner_count(), actual.now()), before);

    let source = decode(wav(2, false, false, &[]));
    let expected_asset = source.as_source().stereo().unwrap().asset_id();
    let authored = ir::Instrument { assets: vec![asset()], zones: vec![zone()], ..Default::default() };
    let mut stereo = runtime(&authored, source);
    stereo.trigger(input(2), 60, 1.).unwrap();
    render(&mut stereo, 17, 7);
    let mut assets = vec![];
    assert!(stereo.visit_onset_voices(|row| { assets.push(row.asset); true }));
    assert_eq!(assets, vec![expected_asset]);
    assert!(!stereo.visit_onset_voices(|_| false));
}


#[test]
fn retained_stereo_trace_does_not_force_a_legacy_asset_projection_for_an_untraced_wide_generation() {
    let old_ir = ir::Instrument { assets: vec![asset()], zones: vec![zone()], ..Default::default() };
    let (old, _reader) = prepare(&old_ir, decode(wav(2, false, false, &[])))
        .with_signal_trace_reader(65536).unwrap();
    let limits = Limits::for_plan(&old, 8, 16);
    let (mut actual, mut control) = Runtime::with_plan_updates(old, limits, 2, 1).unwrap();
    let old_plan = actual.active_plan();
    let old_note = actual.trigger(input(1), 60, 1.).unwrap();
    let (old_reference, _reference_reader) = prepare(&old_ir, decode(wav(2, false, false, &[])))
        .with_signal_trace_reader(65536).unwrap();
    let mut old_reference = Runtime::new(old_reference, limits).unwrap();
    old_reference.trigger(input(1), 60, 1.).unwrap();
    exact(&render(&mut actual, 97, 13), &render(&mut old_reference, 97, 64));

    let mut cells = vec![0.; 12]; cells[3] = 1.; cells[11] = 1.;
    let incoming = instrument(cells, false);
    control.submit(Box::new(prepare(&incoming, decode(wav(6, false, false, &[]))))).unwrap();
    actual.poll_plan_update().unwrap();
    assert_ne!(actual.active_plan(), old_plan);
    assert_eq!(actual.note_plan(old_note), Ok(old_plan));
    assert_eq!(actual.collect_retired_plans(), 0);
    actual.trigger(input(2), 60, 1.).unwrap();
    let mut new_reference = runtime(&incoming, decode(wav(6, false, false, &[])));
    new_reference.trigger(input(2), 60, 1.).unwrap();
    let old = render(&mut old_reference, 400, 17);
    let new = render(&mut new_reference, 400, 31);
    let expected: Vec<_> = old.iter().zip(new).map(|(a, b)| [a[0] + b[0], a[1] + b[1]]).collect();
    exact(&render(&mut actual, 400, 23), &expected);
    assert_eq!((actual.voice_count(), actual.processing_owner_count()), (2, 1));
    let mut projected = 0;
    assert!(!actual.visit_onset_voices(|_| { projected += 1; true }));
    assert_eq!(projected, 0, "mixed generations must not publish a misleading partial asset inventory");
}


fn held_timbre(runtime: &mut Runtime, note: sampler_core::NoteId, timbre: u32) {
    let id = runtime.expression_id(note).unwrap();
    let mut expression = runtime.expression(id).unwrap(); expression.timbre = timbre;
    runtime.set_expression(id, expression).unwrap();
}

#[test]
fn held_mpe_timbre_uses_existing_tone_on_every_selected_original_channel() {
    for (left, right) in [(2, 3), (3, 5), (5, 2)] {
        let mut cells = vec![0.; 12]; cells[left] = 1.; cells[6 + right] = 1.;
        let authored = instrument(cells.clone(), false);
        let mut actual = runtime(&authored, decode(wav(6, false, false, &[])));
        let mut expected = reference(&cells, false);
        let a = actual.trigger(input(1), 60, 1.).unwrap();
        let b = expected.trigger(input(1), 60, 1.).unwrap();
        let mut owners = vec![]; actual.visit_processing_owners(|row| owners.push(row.id));
        exact(&render(&mut actual, 123, 7), &render(&mut expected, 123, 64));
        held_timbre(&mut actual, a, 0); held_timbre(&mut expected, b, 0);
        let dark = render(&mut actual, 333, 13);
        exact(&dark, &render(&mut expected, 333, 257));
        let mut open = reference(&cells, false); open.trigger(input(3), 60, 1.).unwrap();
        render(&mut open, 123, 64);
        assert!(dark.iter().zip(render(&mut open, 333, 64)).any(|(a, b)| a != &b));
        held_timbre(&mut actual, a, 0x8000_0000); held_timbre(&mut expected, b, 0x8000_0000);
        exact(&render(&mut actual, 177, 17), &render(&mut expected, 177, 64));
        held_timbre(&mut actual, a, 0); held_timbre(&mut expected, b, 0);
        exact(&render(&mut actual, 257, 31), &render(&mut expected, 257, 1));
        let mut kept = vec![]; actual.visit_processing_owners(|row| kept.push(row.id));
        assert_eq!(owners, kept);
        assert_eq!((actual.voice_count(), actual.processing_owner_count()), (1, 1));
    }
}

#[test]
fn channel_tone_growth_retains_history_and_reused_sources_begin_with_zero_integrators() {
    let mut cells = vec![0.; 12]; cells[3] = 1.; cells[11] = 1.;
    let authored = instrument(cells.clone(), false);
    let prepared = prepare(&authored, decode(wav(6, false, false, &[])));
    let limits = Limits::for_plan(&prepared, 16, 2);
    let (mut actual, mut control) = Runtime::with_plan_updates(prepared, limits, 2, 1).unwrap();
    let mut expected = reference(&cells, false);
    let a = actual.trigger(input(1), 60, 1.).unwrap();
    let b = expected.trigger(input(1), 60, 1.).unwrap();
    held_timbre(&mut actual, a, 0); held_timbre(&mut expected, b, 0);
    exact(&render(&mut actual, 97, 13), &render(&mut expected, 97, 64));
    assert_eq!(control.grow_voices(4), Ok(4));
    support::without_heap(|| { actual.render(&mut []).unwrap(); });
    assert_eq!(actual.voice_capacity(), 4);
    exact(&render(&mut actual, 333, 31), &render(&mut expected, 333, 257));
    actual.note_off_exact(a, None).unwrap(); expected.note_off_exact(b, None).unwrap();
    render(&mut actual, 128, 17); render(&mut expected, 128, 64);
    assert_eq!((actual.voice_count(), actual.processing_owner_count()), (0, 0));
    let a = actual.trigger(input(2), 60, 1.).unwrap();
    let mut fresh = reference(&cells, false);
    render(&mut fresh, actual.now() as usize, 64);
    let b = fresh.trigger(input(2), 60, 1.).unwrap();
    held_timbre(&mut actual, a, 0); held_timbre(&mut fresh, b, 0);
    exact(&render(&mut actual, 512, 7), &render(&mut fresh, 512, 64));
}
