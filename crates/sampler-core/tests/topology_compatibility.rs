mod support;
use sampler_core::lower::{LowerError, lower};
use sampler_core::{Input, Limits, Pcm, Prepared, Protocol, Runtime};
use sampler_ir as ir;
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
fn asset() -> ir::Asset {
    ir::Asset {
        location: ir::AssetLocation::Path("authored.wav".into()),
        encoding: ir::Encoding::Wav,
        residency: ir::AssetResidency::Resident,
        root_key: None,
        loops: vec![],
    }
}
fn source(frames: usize) -> Pcm {
    Pcm::new(
        48000,
        (0..frames)
            .map(|i| [if i == 0 { 0.5 } else { 0.125 }; 2])
            .collect(),
    )
    .unwrap()
}
fn prepare(shared: bool, frames: usize, delay: bool) -> Prepared {
    let chain = ir::Chain {
        scope: ir::Scope::Voice,
        pre_amplitude: vec![],
        post_amplitude: if delay {
            vec![ir::Processor::Delay {
                time: ir::Time::Milliseconds(1.),
                feedback: 0.,
                mix: 1.,
            }]
        } else {
            vec![ir::Processor::Filter(ir::Filter {
                kind: ir::FilterKind::LowPass { poles: 2 },
                cutoff: ir::Frequency::Hertz(1300.),
                resonance: ir::Resonance::Q(0.7),
            })]
        },
    };
    let mut ir = ir::Instrument {
        assets: vec![asset()],
        zones: vec![ir::Zone {
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        chains: vec![chain],
        ..Default::default()
    };
    if shared {
        ir.processing_scopes.push(ir::ProcessingScope {
            signal: None,
            members: vec![ir::ZoneRef(0)],
            sum: ir::ProcessingSumLaw::OrderedF32,
            cadence: ir::ProcessingCadence::CoreCell64,
            chain: Some(ir::ChainRef(0)),
            amplitude: None,
            output: None,
        });
    } else {
        ir.zones[0].chain = Some(ir::ChainRef(0));
    }
    lower(&ir, 48000, vec![source(frames)], |_, p| {
        Ok::<_, LowerError>(p)
    })
    .unwrap()
}
fn render(rt: &mut Runtime, n: usize, block: usize) -> Vec<[f32; 2]> {
    let mut out = vec![[0.; 2]; n];
    for x in out.chunks_mut(block) {
        rt.render(x).unwrap();
    }
    out
}
#[test]
fn singleton_stereo_legacy_specialization_preserves_typed_filter_pcm_and_partitions() {
    for block in [1, 17, 64, 257] {
        let p = prepare(false, 4096, false);
        assert_eq!(p.processing_scope_count(), 0);
        let l = Limits::for_plan(&p, 8, 16);
        let mut a = Runtime::new(p, l).unwrap();
        let p = prepare(true, 4096, false);
        let l = Limits::for_plan(&p, 8, 16);
        let mut b = Runtime::new(p, l).unwrap();
        a.trigger(input(1), 60, 1.).unwrap();
        b.trigger(input(1), 60, 1.).unwrap();
        assert_eq!(render(&mut a, 1024, block), render(&mut b, 1024, block));
    }
}
#[test]
fn shared_zero_input_tail_survives_last_source_and_retires_exactly_once() {
    let p = prepare(true, 16, true);
    let l = Limits::for_plan(&p, 8, 16);
    let mut rt = Runtime::new(p, l).unwrap();
    let note = rt.trigger(input(1), 60, 1.).unwrap();
    let prefix = render(&mut rt, 16, 16);
    assert!(prefix.iter().all(|x| *x == [0.; 2]));
    assert_eq!(rt.voice_count(), 0);
    assert_eq!(rt.processing_owner_count(), 1);
    assert_eq!(rt.note_plan(note), Ok(rt.active_plan()));
    let tail = render(&mut rt, 48, 7);
    assert!(tail.iter().any(|x| x[0] > 0.1));
    assert_eq!(rt.processing_owner_count(), 0);
    assert!(render(&mut rt, 64, 17).iter().all(|x| *x == [0.; 2]));
    rt.note_off_exact(note, None).unwrap();
    rt.flush_ended(|_| true);
    assert_eq!(rt.note_count(), 0);
}
#[test]
fn old_generation_shared_tail_holds_its_actual_typed_state_after_replacement() {
    let old = prepare(true, 16, true);
    let l = Limits::for_plan(&old, 8, 16);
    let (mut rt, mut control) = Runtime::with_plan_updates(old, l, 3, 2).unwrap();
    let note = rt.trigger(input(1), 60, 1.).unwrap();
    let old_plan = rt.active_plan();
    render(&mut rt, 16, 16);
    let incoming = control
        .submit(Box::new(prepare(false, 4096, false)))
        .unwrap();
    assert_eq!(rt.poll_plan_update(), Ok(Some(incoming)));
    assert_ne!(rt.active_plan(), old_plan);
    assert_eq!(rt.note_plan(note), Ok(old_plan));
    assert_eq!(rt.collect_retired_plans(), 0);
    let tail = render(&mut rt, 48, 11);
    assert!(tail.iter().any(|x| x[0] > 0.1));
    assert_eq!(rt.processing_owner_count(), 0);
    rt.note_off_exact(note, None).unwrap();
    rt.flush_ended(|_| true);
    assert_eq!(rt.collect_retired_plans(), 1);
    assert_eq!(control.retired().unwrap().request, 0);
}
#[test]
fn panic_retires_owner_only_tail_and_source_dependencies_without_heap_calls() {
    let p = prepare(true, 16, true);
    let l = Limits::for_plan(&p, 8, 16);
    let mut rt = Runtime::new(p, l).unwrap();
    rt.trigger(input(1), 60, 1.).unwrap();
    render(&mut rt, 16, 16);
    assert_eq!((rt.voice_count(), rt.processing_owner_count()), (0, 1));
    support::without_heap(|| rt.panic());
    assert_eq!((rt.voice_count(), rt.processing_owner_count()), (0, 0));
    assert!(render(&mut rt, 64, 17).iter().all(|x| *x == [0.; 2]));
}
#[test]
fn ir_membership_remap_preserves_order_and_rejects_duplicate_owners() {
    let mut ir = ir::Instrument {
        assets: vec![asset(), asset(), asset()],
        zones: (0..3).map(|n| ir::Zone::new(ir::AssetRef(n))).collect(),
        processing_scopes: vec![ir::ProcessingScope {
            signal: None,
            members: vec![ir::ZoneRef(2), ir::ZoneRef(0)],
            sum: ir::ProcessingSumLaw::OrderedF32,
            cadence: ir::ProcessingCadence::CoreCell64,
            chain: None,
            amplitude: None,
            output: None,
        }],
        ..Default::default()
    };
    assert!(ir.validate().is_ok());
    ir.retain_zones(|z| z.asset != Some(ir::AssetRef(1)));
    assert_eq!(
        ir.processing_scopes[0].members,
        vec![ir::ZoneRef(1), ir::ZoneRef(0)]
    );
    assert!(ir.validate().is_ok());
    ir.processing_scopes.push(ir.processing_scopes[0].clone());
    assert!(ir.validate().is_err());
}

#[test]
fn owner_tail_capacity_is_reserved_before_any_new_note_or_member_publication() {
    let p = prepare(true, 1, true);
    let mut l = Limits::for_plan(&p, 8, 2);
    l.voices = 2;
    let mut rt = Runtime::new(p, l).unwrap();
    rt.trigger(input(1), 60, 1.).unwrap();
    render(&mut rt, 1, 1);
    rt.trigger(input(2), 60, 1.).unwrap();
    render(&mut rt, 1, 1);
    assert_eq!((rt.voice_count(), rt.processing_owner_count()), (0, 2));
    let note_count = rt.note_count();
    let now = rt.now();
    assert_eq!(
        rt.trigger(input(3), 60, 1.),
        Err(sampler_core::Error::Capacity)
    );
    assert_eq!(
        (
            rt.note_count(),
            rt.voice_count(),
            rt.processing_owner_count(),
            rt.now()
        ),
        (note_count, 0, 2, now)
    );
    rt.panic();
    assert_eq!(rt.processing_owner_count(), 0);
}
