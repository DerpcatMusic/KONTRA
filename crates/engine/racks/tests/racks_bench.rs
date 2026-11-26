//! Reproducible rack & audio graph benchmarks (48 kHz, 256-frame blocks).
//!
//! Run in release mode:
//! `build-slot cargo test --manifest-path plugins/kontra/Cargo.toml -p kontra-racks --release --test racks_bench -- --ignored --nocapture`

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::borrow::Cow;
use std::hint::black_box;
use std::time::Instant;

use kontra_ir as ir;
use kontra_racks::graph::GraphRuntime;
use kontra_racks::rack::Rack;

const RATE: u32 = 48_000;
const BLOCK: usize = 256;

fn test_signal() -> Vec<[f32; 2]> {
    (0..BLOCK)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let l = 0.4 * (std::f32::consts::TAU * 220.0 * t).sin();
            let r = 0.4 * (std::f32::consts::TAU * 330.0 * t).sin();
            [l, r]
        })
        .collect()
}

fn heavy_slots() -> Vec<Option<ir::Effect>> {
    let diag = |g| [[g, 0.0], [0.0, g]];
    vec![
        Some(ir::Effect {
            kind: ir::EffectKind::Filter(ir::Filter {
                pass: ir::Pass::Low,
                hz: 2400.0,
                octaves: 4.0,
                q_octaves: 2.0,
                stages: 2,
                ladder: None,
                damping: [0.707; 3],
                gain: 1.0,
            }),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        }),
        Some(ir::Effect {
            kind: ir::EffectKind::Eq([
                ir::Bell {
                    hz: 120.0,
                    q: 1.0,
                    gain_db: 2.0,
                    shape: ir::Shape::Peak,
                },
                ir::Bell {
                    hz: 800.0,
                    q: 1.2,
                    gain_db: -1.5,
                    shape: ir::Shape::Peak,
                },
                ir::Bell {
                    hz: 3200.0,
                    q: 0.9,
                    gain_db: 1.5,
                    shape: ir::Shape::Peak,
                },
                ir::Bell {
                    hz: 10000.0,
                    q: 0.7,
                    gain_db: 0.0,
                    shape: ir::Shape::Peak,
                },
            ]),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        }),
        Some(ir::Effect {
            kind: ir::EffectKind::Compressor(ir::Compressor {
                threshold_db: -18.0,
                ratio: 4.0,
                knee_db: 6.0,
                attack_ms: 5.0,
                release_ms: 60.0,
                makeup_db: 2.0,
                makeup_slope: 0.0,
                link: true,
                mix: 1.0,
                detector: ir::Detector::Input,
                sidechain_hz: None,
            }),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        }),
        Some(ir::Effect {
            kind: ir::EffectKind::Delay(ir::Delay {
                direct: 0.0,
                time: ir::DelayTime::Ms(15.0),
                depth_ms: 4.0,
                second_head: true,
                input: diag(1.0),
                feedback: diag(0.3),
                output: diag(1.0),
                damping_hz: 8000.0,
                low_cut_hz: 100.0,
                steep_low_cut: false,
                body: Cow::Borrowed(&[]),
                triangle_s: None,
                shape: Cow::Borrowed(&[]),
                noise: [0.0; 2],
                high_shelf: None,
                spread_ms: 1.0,
                rate: ir::Rate::Hz(0.9),
                phase: 0.25,
            }),
            bypass: false,
            output: 0.4,
            dry: 0.6,
        }),
        Some(ir::Effect {
            kind: ir::EffectKind::Reverb(ir::Reverb {
                seconds: 2.0,
                size: 1.0,
                damping: 0.4,
                diffusion: 0.7,
                predelay_ms: 15.0,
                high_cut_hz: 8000.0,
                low_shelf_db: 1.0,
                stereo: 1.0,
                modulation_ms: 1.2,
                modulation_hz: 0.6,
                network: None,
            }),
            bypass: false,
            output: 0.3,
            dry: 0.7,
        }),
    ]
}

fn bench(label: &str, blocks: usize, mut f: impl FnMut()) {
    for _ in 0..32 {
        f();
    }
    let t0 = Instant::now();
    for _ in 0..blocks {
        f();
    }
    let elapsed_ns = t0.elapsed().as_nanos() as f64;
    let ns_per_frame = elapsed_ns / (blocks * BLOCK) as f64;
    let us_per_block = ns_per_frame * BLOCK as f64 / 1000.0;
    let cpu_pct = (ns_per_frame / (1e9 / f64::from(RATE))) * 100.0;
    println!(
        "{label:<44} | {ns_per_frame:>7.2} ns/f | {us_per_block:>6.2} us/256 | {cpu_pct:>5.3}% CPU"
    );
}

#[test]
#[ignore = "benchmark"]
fn racks_and_graph_suite() {
    println!("\n=== KONTRA RACKS & GRAPH BENCHMARK (48 kHz, 256-frame blocks) ===");
    let src = test_signal();
    let silence = vec![[0.0f32; 2]; BLOCK];

    // 1. Heavy active rack
    let mut rack_active = Rack::new(&heavy_slots(), RATE, &[]);
    let mut io = src.clone();
    bench("rack::heavy_5slot (active audio)", 1000, || {
        io.copy_from_slice(&src);
        rack_active.process(&mut io, &mut [], 120.0);
        black_box(&io);
    });

    // 2. Heavy idle rack (fed silence — tests sleep fast-path across all slot types!)
    let mut rack_idle = Rack::new(&heavy_slots(), RATE, &[]);
    let mut io_sil = silence.clone();
    bench("rack::heavy_5slot (silent input)", 2000, || {
        io_sil.copy_from_slice(&silence);
        rack_idle.process(&mut io_sil, &mut [], 120.0);
        black_box(&io_sil);
    });

    // 3. Multi-bus AudioGraph (4 groups -> 2 buses -> 1 output, active vs silent)
    let mut graph = ir::AudioGraph::default();
    let slots = heavy_slots();
    let fx_ids: Vec<ir::EffectId> = slots
        .into_iter()
        .flatten()
        .map(|e| graph.push_effect(e))
        .collect();
    let g0 = graph.push_node(ir::GraphNodeKind::GroupInput { group: 0 });
    let g1 = graph.push_node(ir::GraphNodeKind::GroupInput { group: 1 });
    let g2 = graph.push_node(ir::GraphNodeKind::GroupInput { group: 2 });
    let g3 = graph.push_node(ir::GraphNodeKind::GroupInput { group: 3 });
    let b0 = graph.push_node(ir::GraphNodeKind::Bus {
        name: "bus0".into(),
        effects: vec![fx_ids[0], fx_ids[1], fx_ids[2]],
        gain_db: 0.0,
        pan: 0.0,
        to_main: true,
        layout: ir::ChannelLayout::Stereo,
    });
    let b1 = graph.push_node(ir::GraphNodeKind::Bus {
        name: "bus1".into(),
        effects: vec![fx_ids[3], fx_ids[4]],
        gain_db: 0.0,
        pan: 0.0,
        to_main: true,
        layout: ir::ChannelLayout::Stereo,
    });
    let out = graph.push_node(ir::GraphNodeKind::Output {
        bus_index: 0,
        layout: ir::ChannelLayout::Stereo,
    });
    graph.connect(g0, b0, 1.0, true);
    graph.connect(g1, b0, 1.0, true);
    graph.connect(g2, b1, 1.0, true);
    graph.connect(g3, b1, 1.0, true);
    graph.connect(b0, b1, 0.3, true);
    graph.connect(b0, out, 1.0, true);
    graph.connect(b1, out, 1.0, true);

    let mut rt = GraphRuntime::new(&graph, RATE, BLOCK, &[]).unwrap();
    let mut out_buf = vec![[0.0f32; 2]; BLOCK];
    bench("graph::4grp_2bus_heavy (active)", 1000, || {
        rt.clear(BLOCK);
        rt.add_group_audio(0, &src);
        rt.add_group_audio(1, &src);
        out_buf.fill([0.0; 2]);
        rt.process(BLOCK, 120.0, None, &mut [&mut out_buf]);
        black_box(&out_buf);
    });

    let mut rt_idle = GraphRuntime::new(&graph, RATE, BLOCK, &[]).unwrap();
    bench("graph::4grp_2bus_heavy (silent)", 2000, || {
        rt_idle.clear(BLOCK);
        out_buf.fill([0.0; 2]);
        rt_idle.process(BLOCK, 120.0, None, &mut [&mut out_buf]);
        black_box(&out_buf);
    });
}
