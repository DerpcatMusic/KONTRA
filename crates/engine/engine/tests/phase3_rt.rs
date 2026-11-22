//! Verifies zero audio-thread heap allocations when rendering Phase 3
//! real-time oscillators through a multi-stem [`kontra_ir::AudioGraph`].

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use kontra_engine::{Engine, Event, HostNote, Plan, SampleTime, Timed, Zone};
use kontra_ir as ir;
use kontra_stream::{BlobKey, Encoding, LoadRequest, MemSource, SampleFmt, Store, StoreConfig};
use kontra_voice::{OSC_PERIOD, OscWave, Oscillator, Region};

struct Counting;
thread_local! { static COUNT: Cell<Option<usize>> = const { Cell::new(None) }; }
#[global_allocator]
static ALLOCATOR: Counting = Counting;

// SAFETY: forwards every allocation unchanged to the system allocator; thread-local counter never allocates.
#[expect(unsafe_code, reason = "isolated test allocator forwards to System")]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNT.with(|n| n.set(n.get().map(|v| v + 1)));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        COUNT.with(|n| n.set(n.get().map(|v| v + 1)));
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[test]
fn oscillator_and_multi_stem_audio_graph_allocate_nothing_on_audio_thread() {
    const RATE: u32 = 48_000;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let fmt = SampleFmt {
        encoding: Encoding::F32,
        channels: 1,
    };
    let sentinel = store
        .load_many_now(vec![LoadRequest {
            key: BlobKey {
                file: "<osc>".into(),
                offset: 0,
                fmt,
            },
            source: Arc::new(MemSource::new(fmt, vec![0u8; 4])),
            frames: 1,
            pin: std::iter::once(0..1).collect(),
        }])
        .into_iter()
        .next()
        .unwrap()
        .unwrap();

    let region = Region {
        sample: sentinel,
        start: 0,
        end: 1,
        loops: None,
        mix: None,
        table: None,
        reverse: false,
    };
    let zone = Zone::new(region, 69, (440.0 * OSC_PERIOD) as u32);
    let mut plan = Plan::new(vec![zone], 8);
    plan.groups[0].oscillator = Some(Oscillator::new(OscWave::Saw, 0.0));

    let mut graph = ir::AudioGraph::default();
    let pre_fx = graph.push_effect(ir::Effect {
        kind: ir::EffectKind::Gain(0.5),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    });
    let g0 = graph.push_node(ir::GraphNodeKind::GroupInput { group: 0 });
    let split = graph.push_node(ir::GraphNodeKind::Split {
        pre_amp: vec![pre_fx],
        post_amp: Vec::new(),
        amp_gain: 0.5,
        amp_pan: 0.0,
    });
    let out0 = graph.push_node(ir::GraphNodeKind::Output {
        bus_index: 0,
        layout: ir::ChannelLayout::Stereo,
    });
    let out1 = graph.push_node(ir::GraphNodeKind::Output {
        bus_index: 1,
        layout: ir::ChannelLayout::Stereo,
    });
    graph.connect(g0, split, 1.0, true);
    graph.connect(split, out0, 1.0, true);
    graph.connect(split, out1, 1.0, false);
    plan.graph = Some(graph);

    let mut engine = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut main_buf = vec![[0.0; 2]; 256];
    let mut aux_buf = vec![[0.0; 2]; 256];
    let note_on = [Timed {
        at: SampleTime(0),
        body: Event::NoteOn {
            note: HostNote {
                port: 0,
                channel: 0,
                key: 69,
                id: -1,
                clap: true,
            },
            velocity: 1.0,
            tune: 0.0,
        },
    }];

    COUNT.with(|n| n.set(Some(0)));
    {
        let mut outs: [&mut [[f32; 2]]; 2] = [&mut main_buf, &mut aux_buf];
        engine.process_multi(&mut outs, &note_on);
    }
    for _ in 0..8 {
        let mut outs: [&mut [[f32; 2]]; 2] = [&mut main_buf, &mut aux_buf];
        engine.process_multi(&mut outs, &[]);
    }
    let allocated = COUNT.with(|n| n.replace(None));

    assert_eq!(allocated, Some(0), "Phase 3 audio-thread allocations");
    let main_peak = main_buf.iter().map(|f| f[0].abs()).fold(0.0_f32, f32::max);
    let aux_peak = aux_buf.iter().map(|f| f[0].abs()).fold(0.0_f32, f32::max);
    assert!(main_peak > 0.1 && aux_peak > main_peak * 1.8);
}

#[test]
fn graph_mode_runs_group_inserts_and_legacy_instrument_racks() {
    const RATE: u32 = 48_000;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let fmt = SampleFmt {
        encoding: Encoding::F32,
        channels: 1,
    };
    let sentinel = store
        .load_many_now(vec![LoadRequest {
            key: BlobKey {
                file: "<osc-group-fx>".into(),
                offset: 0,
                fmt,
            },
            source: Arc::new(MemSource::new(fmt, vec![0u8; 4])),
            frames: 1,
            pin: std::iter::once(0..1).collect(),
        }])
        .into_iter()
        .next()
        .unwrap()
        .unwrap();

    let make_plan = |with_group_filter: bool, with_inst_zero_gain: bool| {
        let region = Region {
            sample: Arc::clone(&sentinel),
            start: 0,
            end: 1,
            loops: None,
            mix: None,
            table: None,
            reverse: false,
        };
        let zone = Zone::new(region, 69, (440.0 * OSC_PERIOD) as u32);
        let mut plan = Plan::new(vec![zone], 8);
        plan.groups[0].oscillator = Some(Oscillator::new(OscWave::Saw, 0.0));
        if with_group_filter {
            plan.groups[0].inserts.push(Some(ir::Effect {
                kind: ir::EffectKind::Filter(ir::Filter {
                    pass: ir::Pass::Low,
                    hz: 120.0,
                    stages: 2,
                    ladder: None,
                    damping: [1.414, 1.414, 1.414],
                    gain: 1.0,
                    octaves: 0.0,
                    q_octaves: 0.0,
                }),
                bypass: false,
                output: 1.0,
                dry: 0.0,
            }));
        }
        if with_inst_zero_gain {
            plan.inserts.push(Some(ir::Effect {
                kind: ir::EffectKind::Gain(0.0),
                bypass: false,
                output: 1.0,
                dry: 0.0,
            }));
        }
        let mut graph = ir::AudioGraph::default();
        let g0 = graph.push_node(ir::GraphNodeKind::GroupInput { group: 0 });
        let out0 = graph.push_node(ir::GraphNodeKind::Output {
            bus_index: 0,
            layout: ir::ChannelLayout::Stereo,
        });
        graph.connect(g0, out0, 1.0, true);
        plan.graph = Some(graph);
        plan
    };

    let render_peak = |plan: Plan| {
        let mut engine = Engine::new(Arc::new(plan), store.client(), RATE);
        let mut out = vec![[0.0; 2]; 512];
        let note_on = [Timed {
            at: SampleTime(0),
            body: Event::NoteOn {
                note: HostNote {
                    port: 0,
                    channel: 0,
                    key: 69,
                    id: -1,
                    clap: true,
                },
                velocity: 1.0,
                tune: 0.0,
            },
        }];
        let mut outs: [&mut [[f32; 2]]; 1] = [&mut out];
        engine.process_multi(&mut outs, &note_on);
        out.iter().map(|f| f[0].abs()).fold(0.0_f32, f32::max)
    };

    let raw_peak = render_peak(make_plan(false, false));
    let filtered_peak = render_peak(make_plan(true, false));
    let muted_peak = render_peak(make_plan(false, true));

    assert!(raw_peak > 0.5, "raw peak={raw_peak}");
    assert!(
        filtered_peak < raw_peak * 0.4,
        "group insert low-pass must attenuate 440 Hz saw in graph mode: raw={raw_peak}, filtered={filtered_peak}"
    );
    assert!(
        muted_peak < 1e-5,
        "legacy instrument insert zero-gain must not be silently bypassed in graph mode: muted={muted_peak}"
    );
}
