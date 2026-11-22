//! End-to-end tests for the native `.kontra` format (TOML, JSON, and binary).
#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]

use std::path::Path;

use kontra_format_native::{
    from_ir, load, parse_bytes, parse_json, parse_toml, to_binary, to_json, to_toml,
};
use kontra_ir as ir;

fn write_wav(path: &Path, frames: u32) {
    let rate = 48_000u32;
    let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    for field in [16u32, 1 | (1 << 16), rate, rate * 2, 2 | (16 << 16)] {
        wav.extend(field.to_le_bytes());
    }
    wav.extend(b"data");
    wav.extend((frames * 2).to_le_bytes());
    for i in 0..frames {
        let s = (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin();
        wav.extend(((s * 16_000.0) as i16).to_le_bytes());
    }
    let riff = (wav.len() - 8) as u32;
    wav.splice(4..8, riff.to_le_bytes());
    std::fs::write(path, wav).unwrap_or_else(|e| panic!("write wav: {e}"));
}

#[test]
fn native_kontra_round_trips_toml_json_and_binary_with_full_vnext_features() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("kontra_native_roundtrip");
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("mkdir: {e}"));
    write_wav(&dir.join("tone.wav"), 12_000);

    let toml_text = r#"
version = 1
name = "Native Strings"
voices = 32

[[groups]]
name = "Main"
keytrack = 1.0

[groups.envelope]
delay = 0.02
attack = 0.01
decay = 0.25
sustain = 0.7
release = 0.4
decay_shape = 2.0
release_shape = -3.0

[[groups.start]]
kind = "round_robin"
scope = "key"
position = 0
length = 2

[[groups.start]]
kind = "random"
lo = 0
hi = 5000

[[groups.mods]]
name = "vibrato_pitch"
target = "pitch"
source = "note_expr"
expr = "vibrato"
semitones = 1.0

[[groups.mods]]
name = "pnc_vol"
target = "volume"
source = "per_note_cc"
registered = true
index = 74
intensity = 1.0
curve = "depth"
power = 1

[[groups.zones]]
sample = "tone.wav"
keys = [48, 72]
velocities = [1, 127]
root = 60

[[groups.zones.crossfades]]
control = "cc"
cc = 1
fade_in = [20, 80]
law = "equal_power"
"#;

    let path = dir.join("NativeStrings.kontra");
    std::fs::write(&path, toml_text).unwrap_or_else(|e| panic!("write kontra: {e}"));

    let loaded = load(&path).unwrap_or_else(|e| panic!("load: {e}"));
    assert!(
        loaded.diagnostics.is_empty(),
        "expected zero diagnostics, got {:?}",
        loaded.diagnostics
    );
    assert_eq!(loaded.name, "Native Strings");
    assert_eq!(loaded.voices, 32);
    assert_eq!(loaded.samples.len(), 1);
    assert_eq!(loaded.samples[0].frames, 12_000);
    assert_eq!(loaded.groups.len(), 1);

    let g = &loaded.groups[0];
    assert_eq!(g.envelope.delay, 0.02);
    assert_eq!(g.envelope.decay_shape, 2.0);
    assert_eq!(g.envelope.release_shape, -3.0);
    assert_eq!(
        g.start,
        vec![
            (
                ir::Join::And,
                ir::StartRule::RoundRobin {
                    scope: ir::RrScope::Key,
                    position: 0,
                    length: 2,
                    random: false,
                }
            ),
            (ir::Join::And, ir::StartRule::Random { lo: 0, hi: 5000 }),
        ]
    );
    assert_eq!(g.zones[0].crossfades.len(), 1);
    assert_eq!(g.zones[0].crossfades[0].law, ir::CrossfadeLaw::EqualPower);

    // Export back to TOML, JSON, and binary and verify identical round-trip.
    let spec = from_ir(&loaded, Some(&dir)).unwrap_or_else(|e| panic!("from_ir: {e}"));
    let as_toml = to_toml(&spec).unwrap_or_else(|e| panic!("to_toml: {e}"));
    let as_json = to_json(&spec).unwrap_or_else(|e| panic!("to_json: {e}"));
    let as_bin = to_binary(&spec).unwrap_or_else(|e| panic!("to_binary: {e}"));

    let from_toml = parse_toml(&as_toml, &dir).unwrap_or_else(|e| panic!("parse_toml: {e}"));
    let from_json = parse_json(&as_json, &dir).unwrap_or_else(|e| panic!("parse_json: {e}"));
    let from_bin = parse_bytes(&as_bin, &dir).unwrap_or_else(|e| panic!("parse_bytes: {e}"));

    assert_eq!(from_toml.groups, loaded.groups);
    assert_eq!(from_json.groups, loaded.groups);
    assert_eq!(from_bin.groups, loaded.groups);
}

#[test]
fn native_kontra_round_trips_phase3_oscillator_and_audio_graph() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("kontra_native_phase3");
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("mkdir: {e}"));
    let toml = r#"
version = 1
name = "Phase3Synth"
voices = 16

[[graph.effects]]
kind = "gain"
gain = 0.5

[[graph.nodes]]
kind = "group_input"
group = 0

[[graph.nodes]]
kind = "split"
pre_amp = [0]
post_amp = []
amp_gain = 0.75
amp_pan = -0.25

[[graph.nodes]]
kind = "output"
bus_index = 0
layout = "stereo"

[[graph.nodes]]
kind = "output"
bus_index = 1
layout = "surround51"

[[graph.edges]]
from = 0
to = 1
gain = 1.0
post_fader = true

[[graph.edges]]
from = 1
to = 2
gain = 1.0
post_fader = true

[[graph.edges]]
from = 1
to = 3
gain = 0.5
post_fader = false

[[groups]]
name = "Pulse"

[groups.generator]
kind = "oscillator"
wave = "square"
width = 0.25
tune_semitones = -12.0
phase = 0.125

[[groups.zones]]
keys = [24, 96]
velocities = [1, 127]
"#;
    let inst = parse_toml(toml, &dir).unwrap_or_else(|e| panic!("parse_toml: {e}"));
    assert!(inst.diagnostics.is_empty(), "{:?}", inst.diagnostics);
    assert!(inst.samples.is_empty());
    assert!(matches!(
        inst.groups[0].generator,
        Some(ir::Generator::Oscillator {
            wave: ir::Wave::Square { width },
            tune_semitones,
            phase,
        }) if (width - 0.25).abs() < 1e-6
            && (tune_semitones + 12.0).abs() < 1e-6
            && (phase - 0.125).abs() < 1e-6
    ));
    let graph = inst.graph.as_ref().expect("audio graph");
    assert_eq!(graph.nodes.len(), 4);
    assert_eq!(graph.edges.len(), 3);
    assert_eq!(graph.effects.len(), 1);

    let spec = from_ir(&inst, Some(&dir)).unwrap();
    let rt_toml = parse_toml(&to_toml(&spec).unwrap(), &dir).unwrap();
    let rt_json = parse_json(&to_json(&spec).unwrap(), &dir).unwrap();
    let rt_bin = parse_bytes(&to_binary(&spec).unwrap(), &dir).unwrap();
    for rt in [&rt_toml, &rt_json, &rt_bin] {
        assert_eq!(rt.groups, inst.groups);
        assert_eq!(rt.graph, inst.graph);
        assert!(rt.diagnostics.is_empty(), "{:?}", rt.diagnostics);
    }
}

#[test]
fn native_round_trips_granular_pcm_and_graph_filter_and_rejects_unsupported_exports() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("kontra_native_phase3_strict");
    std::fs::create_dir_all(&dir).unwrap();
    write_wav(&dir.join("unused.wav"), 200);
    write_wav(&dir.join("grain.wav"), 400);

    let mut graph = ir::AudioGraph::default();
    let g_fx = graph.push_effect(ir::Effect {
        kind: ir::EffectKind::Gain(0.5),
        bypass: false,
        output: 0.8,
        dry: 0.25,
    });
    let f_fx = graph.push_effect(ir::Effect {
        kind: ir::EffectKind::Filter(ir::Filter {
            pass: ir::Pass::Low,
            hz: 1200.0,
            stages: 2,
            ladder: None,
            damping: [0.7, 0.7, 0.7],
            gain: 1.0,
            octaves: 4.0,
            q_octaves: 2.0,
        }),
        bypass: false,
        output: 0.9,
        dry: 0.1,
    });
    let g0 = graph.push_node(ir::GraphNodeKind::GroupInput { group: 0 });
    let rack = graph.push_node(ir::GraphNodeKind::Rack {
        effects: vec![g_fx, f_fx],
        layout: ir::ChannelLayout::Stereo,
    });
    let out = graph.push_node(ir::GraphNodeKind::Output {
        bus_index: 0,
        layout: ir::ChannelLayout::Stereo,
    });
    graph.connect(g0, rack, 1.0, true);
    graph.connect(rack, out, 1.0, true);

    let inst = ir::Instrument {
        name: "Strict".into(),
        samples: vec![
            ir::Sample {
                name: "unused.wav".into(),
                file: dir.join("unused.wav"),
                offset: 0,
                len: None,
                channels: 1,
                frames: 200,
                rate: 48_000,
                keystream: None,
                raw: None,
            },
            ir::Sample {
                name: "grain.wav".into(),
                file: dir.join("grain.wav"),
                offset: 0,
                len: None,
                channels: 1,
                frames: 400,
                rate: 48_000,
                keystream: None,
                raw: None,
            },
        ],
        groups: vec![
            ir::Group {
                name: "Cloud".into(),
                generator: Some(ir::Generator::Granular {
                    sample: 1,
                    grain_ms: 40.0,
                    density: 8.0,
                    position: 0.25,
                    pitch_semitones: 2.0,
                    spread: 0.5,
                }),
                zones: vec![ir::Zone {
                    id: 0,
                    sample: 1,
                    keys: 0..=127,
                    velocities: 1..=127,
                    velocity_fade: (0, 0),
                    key_fade: (0, 0),
                    crossfades: Vec::new(),
                    root: 60,
                    tune: 0.0,
                    volume_db: 0.0,
                    pan: 0.0,
                    start: 0,
                    end: 400,
                    start_mod: 0,
                    loops: Vec::new(),
                    reverse: false,
                    mix: Vec::new(),
                    slot: 0,
                    envelopes: Vec::new(),
                }],
                ..ir::Group::DEFAULT
            },
            ir::Group {
                name: "PcmOverride".into(),
                generator: Some(ir::Generator::Pcm {
                    sample: 1,
                    start: 10,
                    end: 200,
                    reverse: true,
                }),
                zones: vec![ir::Zone {
                    id: 1,
                    sample: 1,
                    keys: 0..=127,
                    velocities: 1..=127,
                    velocity_fade: (0, 0),
                    key_fade: (0, 0),
                    crossfades: Vec::new(),
                    root: 60,
                    tune: 0.0,
                    volume_db: 0.0,
                    pan: 0.0,
                    start: 0,
                    end: 400,
                    start_mod: 0,
                    loops: Vec::new(),
                    reverse: false,
                    mix: Vec::new(),
                    slot: 0,
                    envelopes: Vec::new(),
                }],
                ..ir::Group::DEFAULT
            },
        ],
        graph: Some(graph),
        ..ir::Instrument::DEFAULT
    };

    let spec = from_ir(&inst, Some(&dir)).unwrap();
    let rt = parse_toml(&to_toml(&spec).unwrap(), &dir).unwrap();
    assert!(rt.diagnostics.is_empty(), "{:?}", rt.diagnostics);
    // `unused.wav` was omitted, so `grain.wav` is remapped to index 0 everywhere.
    assert_eq!(rt.samples.len(), 1);
    assert_eq!(rt.samples[0].name, "grain.wav");
    assert!(matches!(
        rt.groups[0].generator,
        Some(ir::Generator::Granular { sample: 0, .. })
    ));
    assert!(matches!(
        rt.groups[1].generator,
        Some(ir::Generator::Pcm {
            sample: 0,
            start: 10,
            end: 200,
            reverse: true
        })
    ));
    // Both Gain (with output & dry) and Filter preserved losslessly.
    assert_eq!(rt.graph, inst.graph);

    // Unsupported wavetable override or unsupported graph effect errors explicitly.
    let mut bad_gen = inst.clone();
    bad_gen.groups[0].generator = Some(ir::Generator::Wavetable(ir::Wavetable {
        position: 0.0,
        phase: 0.0,
        phase_random: 0.0,
        gain: 1.0,
        inharmonic: ir::Inharmonic::OFF,
        modosc: ir::ModOsc::OFF,
        quality: ir::Quality::High,
        warps: [ir::Warp::OFF; 2],
    }));
    assert!(matches!(
        from_ir(&bad_gen, Some(&dir)),
        Err(kontra_format_native::Error::UnsupportedExport(_))
    ));

    let mut bad_fx = inst;
    bad_fx.graph.as_mut().unwrap().effects[0].kind = ir::EffectKind::Convolution { impulse: 0 };
    assert!(matches!(
        from_ir(&bad_fx, Some(&dir)),
        Err(kontra_format_native::Error::UnsupportedExport(_))
    ));
}
