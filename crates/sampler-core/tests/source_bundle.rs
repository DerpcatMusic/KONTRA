//! Staged semantic/admission tests, not claims of multichannel audio GREEN.
use sampler_core::lower::{lower, Feature, LowerError};
use sampler_core::{Pcm, SourceBundleRefusal as R};
use sampler_ir as ir;
use std::cell::Cell;

fn asset(index: usize) -> ir::Asset {
    ir::Asset { location: ir::AssetLocation::Path(format!("authored-{index}.wav")),
        encoding: ir::Encoding::Wav, residency: Default::default(), root_key: None, loops: vec![] }
}
fn fixture() -> ir::Instrument {
    let mut zone = ir::Zone::new(ir::AssetRef(0));
    zone.keys = ir::KeyRange { low: 60, high: 60 };
    zone.chain = Some(ir::ChainRef(0));
    let mut instrument = ir::Instrument {
        assets: (0..3).map(asset).collect(), zones: vec![zone],
        chains: vec![ir::Chain { scope: ir::Scope::Voice,
            pre_amplitude: vec![ir::Processor::StereoMatrix([[1., 0.], [0., 1.]])], post_amplitude: vec![] }],
        ..Default::default()
    };
    instrument.source_indices.source_bundles.push(ir::SourceBundle {
        zone: ir::ZoneRef(0), packs: [ir::AssetRef(0), ir::AssetRef(1), ir::AssetRef(2)],
        geometry: ir::SourceBundleGeometry { rate: 48000, frames: 64 },
        player: 700, logical_asset: 40, origin: [17; 32],
        boundary: ir::SourceMatrixBoundary { node: 710, chain: ir::ChainRef(0),
            section: ir::SourceBoundarySection::BeforeAmplitude, index: 0,
            cells: std::array::from_fn(|n| {
                let (input, output) = ((n / 2) as u8, (n % 2) as u8);
                ir::SourceMatrixCell { input, output,
                    coefficient: if input == output { 1. } else { 0. },
                    control: None, ramp: ir::Time::ZERO }
            }),
        },
    });
    instrument
}
fn pcm() -> Vec<Pcm> {
    (0..3).map(|n| Pcm::new(48000, vec![[(n + 1) as f32 / 8.; 2]; 64].into_boxed_slice()).unwrap()).collect()
}
fn refusal(instrument: &ir::Instrument, sources: Vec<Pcm>, expected: R) {
    let result = lower(instrument, 48000, sources, |_, _| panic!("refused bundle reached behavior binding"));
    assert!(matches!(result, Err(LowerError::Unsupported {
        feature: Feature::SourceBundle(actual), ..
    }) if actual == expected));
}

#[test]
fn helper_packs_survive_narrowing_in_original_physical_order() {
    let mut instrument = fixture();
    instrument.assets.insert(0, asset(99));
    instrument.zones[0].asset = Some(ir::AssetRef(1));
    instrument.source_indices.source_bundles[0].packs = [ir::AssetRef(1), ir::AssetRef(2), ir::AssetRef(3)];
    assert_eq!(instrument.validate(), Ok(()));
    let before = instrument.source_indices.source_bundles[0].clone();
    assert_eq!(instrument.retain_zones(|_| true), [1, 2, 3]);
    let after = &instrument.source_indices.source_bundles[0];
    assert_eq!(after.packs, [ir::AssetRef(0), ir::AssetRef(1), ir::AssetRef(2)]);
    assert_eq!(instrument.zones[0].asset, Some(ir::AssetRef(0)));
    assert_eq!((after.player, after.logical_asset, after.origin), (before.player, before.logical_asset, before.origin));
    assert_eq!(after.boundary, before.boundary);
    assert_eq!(instrument.validate(), Ok(()));
}

#[test]
fn shared_immutable_packs_keep_one_dependency_set_after_owner_compaction() {
    let mut instrument = fixture();
    let mut second = instrument.zones[0].clone();
    second.keys = ir::KeyRange { low: 61, high: 61 };
    instrument.zones.push(second);
    let mut bundle = instrument.source_indices.source_bundles[0].clone();
    bundle.zone = ir::ZoneRef(1); bundle.player = 701;
    instrument.source_indices.source_bundles.push(bundle);
    assert_eq!(instrument.validate(), Ok(()));
    assert_eq!(instrument.retain_zones(|z| z.keys.low == 61), [0, 1, 2]);
    assert_eq!(instrument.zones.len(), 1);
    assert_eq!(instrument.source_indices.source_bundles.len(), 1);
    assert_eq!(instrument.source_indices.source_bundles[0].zone, ir::ZoneRef(0));
    assert_eq!(instrument.source_indices.source_bundles[0].player, 701);
    assert_eq!(instrument.validate(), Ok(()));
}

#[test]
fn dropping_last_owner_releases_all_three_dependencies() {
    let mut instrument = fixture();
    assert!(instrument.retain_zones(|_| false).is_empty());
    assert!(instrument.assets.is_empty());
    assert!(instrument.source_indices.source_bundles.is_empty());
    assert_eq!(instrument.validate(), Ok(()));
}

#[test]
fn dangling_public_owner_is_not_erased_by_narrowing() {
    let mut instrument = fixture();
    instrument.source_indices.source_bundles[0].zone = ir::ZoneRef(99);
    instrument.retain_zones(|_| true);
    assert_eq!(instrument.source_indices.source_bundles.len(), 1);
    assert!(matches!(instrument.validate(), Err(ir::ValidationError::Dangling { reference: ir::Reference::Zone(99), .. })));
}

#[test]
fn dangling_helper_is_not_indexed_or_substituted_during_narrowing() {
    let mut instrument = fixture();
    instrument.source_indices.source_bundles[0].packs[2] = ir::AssetRef(99);
    instrument.retain_zones(|_| true);
    assert_eq!(instrument.source_indices.source_bundles[0].packs[2], ir::AssetRef(99));
    assert!(matches!(instrument.validate(), Err(ir::ValidationError::Dangling { reference: ir::Reference::Asset(99), .. })));
}

#[test]
fn duplicate_owner_pack_and_cell_coordinates_are_structural_errors() {
    let original = fixture();
    let mut instrument = original.clone();
    instrument.source_indices.source_bundles.push(instrument.source_indices.source_bundles[0].clone());
    assert!(matches!(instrument.validate(), Err(ir::ValidationError::SourceBundle { .. })));
    instrument = original.clone();
    instrument.source_indices.source_bundles[0].packs[2] = ir::AssetRef(1);
    assert!(matches!(instrument.validate(), Err(ir::ValidationError::SourceBundle { .. })));
    instrument = original.clone();
    instrument.source_indices.source_bundles[0].boundary.cells[11] = instrument.source_indices.source_bundles[0].boundary.cells[10];
    assert!(matches!(instrument.validate(), Err(ir::ValidationError::SourceBundle { .. })));
    for (input, output) in [(6, 0), (0, 2), (255, 255)] {
        instrument = original.clone();
        let cell = &mut instrument.source_indices.source_bundles[0].boundary.cells[11];
        cell.input = input; cell.output = output;
        assert!(matches!(instrument.validate(), Err(ir::ValidationError::SourceBundle { .. })));
    }
}

#[test]
fn signed_cells_preserve_stereo_projection_and_exact_typed_defaults() {
    let mut instrument = fixture();
    for cell in &mut instrument.source_indices.source_bundles[0].boundary.cells {
        if cell.input >= 2 { cell.coefficient = if cell.input % 2 == 0 { -0.25 } else { 0.5 }; }
        cell.control = Some(ir::ControlRef(instrument.controls.len()));
        instrument.controls.push(ir::Control {
            key: format!("authored-cell-{}-{}", cell.input, cell.output), label: "Cell".into(),
            value: ir::ControlValue::Continuous { min: -1., max: 1., default: cell.coefficient, unit: ir::ControlUnit::None },
            automation: ir::Automation::None,
        });
    }
    assert_eq!(instrument.validate(), Ok(()));
    instrument.source_indices.source_bundles[0].boundary.cells[4].coefficient = 0.25;
    assert!(matches!(instrument.validate(), Err(ir::ValidationError::SourceBundle { .. })));
    instrument.source_indices.source_bundles[0].boundary.cells[4].coefficient = -0.25;
    instrument.source_indices.source_bundles[0].boundary.cells[0].coefficient = 0.5;
    assert!(matches!(instrument.validate(), Err(ir::ValidationError::SourceBundle { .. })));
}

#[test]
fn nonfinite_cells_and_boolean_numeric_bindings_are_refused() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1.001, -1.001] {
        let mut instrument = fixture();
        instrument.source_indices.source_bundles[0].boundary.cells[11].coefficient = value;
        assert!(instrument.validate().is_err());
    }
    let mut instrument = fixture();
    instrument.controls.push(ir::Control { key: "bool".into(), label: "Bool".into(),
        value: ir::ControlValue::Toggle { default: false }, automation: ir::Automation::None });
    instrument.source_indices.source_bundles[0].boundary.cells[11].control = Some(ir::ControlRef(0));
    assert!(matches!(instrument.validate(), Err(ir::ValidationError::SourceBundle { .. })));
}

#[test]
fn every_pack_geometry_is_checked_before_execution_refusal() {
    for pack in 0..3 {
        for (rate, frames) in [(44100, 64), (48000, 63), (48000, 65)] {
            let mut sources = pcm();
            sources[pack] = Pcm::new(rate, vec![[0.; 2]; frames].into_boxed_slice()).unwrap();
            refusal(&fixture(), sources, R::Geometry);
        }
    }
}

#[test]
fn physical_aliases_refuse_even_with_distinct_asset_indices() {
    let mut sources = pcm(); sources[2] = sources[1].clone();
    refusal(&fixture(), sources, R::PhysicalIdentity);
}

#[test]
fn cross_bundle_channel_role_aliasing_refuses_atomically() {
    let mut instrument = fixture();
    let mut second = instrument.zones[0].clone(); second.asset = Some(ir::AssetRef(1));
    instrument.zones.push(second);
    let mut bundle = instrument.source_indices.source_bundles[0].clone();
    bundle.zone = ir::ZoneRef(1); bundle.player = 701; bundle.logical_asset = 41;
    bundle.packs = [ir::AssetRef(1), ir::AssetRef(0), ir::AssetRef(2)];
    instrument.source_indices.source_bundles.push(bundle);
    assert_eq!(instrument.validate(), Ok(()));
    refusal(&instrument, pcm(), R::ConflictingPackRole);
}

#[test]
fn initial_decode_and_pack_staging_share_one_finite_budget() {
    let mut instrument = fixture();
    // Metadata-only Pcm: no giant PCM allocation, resource, worker or decoder.
    let frames = 12_000_000;
    instrument.source_indices.source_bundles[0].geometry.frames = frames as u64;
    let sources = (0..3).map(|_| Pcm::streamed(48000, frames).unwrap()).collect();
    refusal(&instrument, sources, R::InitialMemoryLimit);
}

#[test]
fn valid_bundle_is_honestly_blocked_before_behavior_binding() {
    let mut instrument = fixture();
    instrument.behaviors.push(ir::Behavior { name: "must not bind".into(), language: ir::Language::Lua,
        source: String::new(), slot: None, state: vec![], requires: vec![] });
    assert_eq!(instrument.validate(), Ok(()));
    let bound = Cell::new(false);
    let result = lower(&instrument, 48000, pcm(), |_, plan| { bound.set(true); Ok(plan) });
    assert!(!bound.get());
    assert!(matches!(result, Err(LowerError::Unsupported { feature: Feature::SourceBundle(R::ExecutionPending), .. })));
}

#[test]
fn boundary_is_not_moved_across_gain_or_amplitude_envelope() {
    let mut instrument = fixture();
    instrument.chains[0].pre_amplitude.insert(0, ir::Processor::Gain(ir::Gain::Linear(0.5)));
    instrument.source_indices.source_bundles[0].boundary.index = 1;
    assert_eq!(instrument.validate(), Ok(()));
    refusal(&instrument, pcm(), R::BoundaryPlacement);
    let mut instrument = fixture();
    instrument.chains[0].post_amplitude = std::mem::take(&mut instrument.chains[0].pre_amplitude);
    instrument.source_indices.source_bundles[0].boundary.section = ir::SourceBoundarySection::AfterAmplitude;
    assert_eq!(instrument.validate(), Ok(()));
    refusal(&instrument, pcm(), R::BoundaryPlacement);
}

#[test]
fn legacy_source_still_reaches_ordinary_runtime() {
    let mut instrument = fixture(); instrument.source_indices.source_bundles.clear();
    let plan = lower(&instrument, 48000, pcm(), |_, _| panic!("no behaviors")).unwrap();
    let limits = sampler_core::Limits::for_plan(&plan, 4, 4);
    let mut runtime = sampler_core::Runtime::new(plan, limits).unwrap();
    runtime.trigger(sampler_core::Input { protocol: sampler_core::Protocol::Native,
        port: 0, group: 0, channel: 0, key: 60, external_id: None }, 60, 1.).unwrap();
    runtime.render(&mut [[0.; 2]; 8]).unwrap();
    assert_eq!(runtime.voice_count(), 1);
}

// A logical target identifies its whole original-channel source. The legacy
// single-Pcm transaction must never reinterpret that identity as pack zero.
#[test]
fn resident_family_target_retains_identity_and_refuses_single_pcm_before_queue_or_task_mutation() {
    use sampler_core::{ChannelCount, Error, Input, Limits, Playback, PreparedSampleRevision,
        Protocol, Runtime, SampleTargetId, SampleTask, SourceStorageFamily};
    use std::sync::Arc;
    let family = SourceStorageFamily::new_resident(ChannelCount::new(6).unwrap(), pcm()).unwrap();
    let id = family.id();
    let mut instrument = ir::Instrument { assets: vec![asset(0)],
        zones: vec![ir::Zone { pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None, ..ir::Zone::new(ir::AssetRef(0)) }],
        processing_scopes: vec![ir::ProcessingScope {
            members: vec![ir::ZoneRef(0)], sum: ir::ProcessingSumLaw::OrderedF32,
            cadence: ir::ProcessingCadence::CoreCell64, chain: None, amplitude: None, output: None,
            signal: Some(ir::ProcessingSignal {
                source_law: ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,
                input: ir::SignalLayout { channels: 6 },
                nodes: vec![ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix {
                    input: ir::SignalLayout { channels: 6 }, output: ir::SignalLayout { channels: 2 },
                    cells: vec![0.,0.,1.,0.,0.,0., 0.,0.,0.,0.,1.,0.],
                    arithmetic: ir::MatrixArithmetic::SeparateMulAddF32 })], controls: vec![] }) }],
        ..Default::default() };
    instrument.source_indices.sample_targets.push(ir::SourceSampleTarget {
        node: 711, zones: vec![ir::ZoneRef(0)] });
    let prepared = sampler_core::lower::lower_sources(&instrument, 48000, vec![family.clone().into()],
        |_, plan| Ok::<_, LowerError>(plan)).unwrap();
    let limits = Limits::for_plan(&prepared, 4, 8);
    let (mut runtime, mut control) = Runtime::new(prepared, limits).unwrap().with_sample_updates(2).unwrap();
    assert_eq!(runtime.sample_target_count(), 1);
    let target = SampleTargetId(711);
    assert_eq!(runtime.sample_target_source_storage(target).unwrap().id(), id);
    assert_eq!(runtime.sample_target_asset(target).unwrap_err(), Error::UnsupportedSourceLayout);
    let input = Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: Some(55) };
    runtime.trigger(input, 60, 1.).unwrap();
    let before = (runtime.now(), runtime.note_count(), runtime.voice_count(),
        runtime.sample_physical_budget_status());
    let incoming = Pcm::new(48000, vec![[0.75, -0.75]; 64].into_boxed_slice()).unwrap();
    let incoming_id = incoming.asset_id();
    let task = Arc::new(SampleTask::default());
    let revision = PreparedSampleRevision::new(target, incoming, Playback::default(), 48000).unwrap();
    let (error, returned, returned_task) = control.submit(revision, task.clone()).unwrap_err();
    assert_eq!(error, Error::UnsupportedSourceLayout);
    assert_eq!(returned.asset().asset_id(), incoming_id);
    assert!(Arc::ptr_eq(&task, &returned_task));
    assert!(!task.finished());
    assert!(task.cancel(), "refusal must happen before the commit CAS");
    assert!(!runtime.poll_sample_update().unwrap());
    assert!(control.reply().is_none());
    assert_eq!((runtime.now(), runtime.note_count(), runtime.voice_count(),
        runtime.sample_physical_budget_status()), before);
    assert_eq!(runtime.sample_target_source_storage(target).unwrap().id(), id);
    let mut output = [[0.; 2]; 32]; runtime.render(&mut output).unwrap();
    assert!(output.iter().flatten().any(|x| *x != 0.));
    // Both selected upper inputs survive; no old first-pack stereo substitute.
    assert!(output.iter().all(|frame| frame[1] > frame[0] && frame[0] > 0.));
}
