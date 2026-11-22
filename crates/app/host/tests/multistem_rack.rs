//! End-to-end RackAudio multi-stem AudioGraph output routing and live
//! `Rack::set_param(Scope, ParamAddr)` tests.
#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]

use std::path::Path;

use kontra_host::rack::{Event, HostNote, Mix, Rack, Store, StoreConfig, ir};

const RATE: u32 = 48_000;

fn note_on(key: u8) -> (u32, Event) {
    (
        0,
        Event::NoteOn {
            note: HostNote {
                port: 0,
                channel: 0,
                key,
                id: -1,
                clap: true,
            },
            velocity: 1.0,
            tune: 0.0,
        },
    )
}

#[test]
fn rack_routes_multi_stem_audio_graph_to_host_outputs_and_honors_set_param() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("kontra_rack_multistem");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("MultiStem.kontra");
    let toml = r#"
version = 1
name = "MultiStem"
voices = 8

[[graph.effects]]
kind = "gain"
gain = 0.5

[[graph.effects]]
kind = "gain"
gain = 0.5

[[graph.nodes]]
kind = "group_input"
group = 0

[[graph.nodes]]
kind = "split"
pre_amp = [0]
post_amp = [1]
amp_gain = 0.5
amp_pan = 0.0

[[graph.nodes]]
kind = "output"
bus_index = 0

[[graph.nodes]]
kind = "output"
bus_index = 1

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
gain = 1.0
post_fader = false

[[groups]]
name = "Lead"

[groups.generator]
kind = "oscillator"
wave = "sine"

[groups.envelope]
attack = 0.0
sustain = 1.0
release = 0.05

[[groups.zones]]
keys = [0, 127]
velocities = [1, 127]
"#;
    std::fs::write(&path, toml).unwrap();

    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (mut rack, mut audio) = Rack::new(RATE, store, 512);
    let part = 0;
    rack.load(part, &path).unwrap();

    audio.process(512, &[note_on(69)]);
    for _ in 0..8 {
        audio.process(512, &[]);
    }

    let main_peak = audio
        .output(0, 512)
        .iter()
        .map(|f| f[0].abs())
        .fold(0.0_f32, f32::max);
    let aux_peak = audio
        .output(1, 512)
        .iter()
        .map(|f| f[0].abs())
        .fold(0.0_f32, f32::max);
    assert!(
        main_peak > 0.05 && aux_peak > 0.2,
        "expected both host stems audible: main={main_peak}, aux={aux_peak}"
    );
    let ratio = aux_peak / main_peak;
    assert!(
        (ratio - 4.0).abs() < 0.05,
        "expected aux/main ratio ~4.0, got {ratio}"
    );

    // Live ParamAddr write via Rack::set_param scales instrument output cleanly.
    rack.set_param(part, ir::Scope::Instrument, ir::ParamAddr::Volume, 0.25);
    for _ in 0..8 {
        audio.process(512, &[]);
    }
    let scaled_main = audio
        .output(0, 512)
        .iter()
        .map(|f| f[0].abs())
        .fold(0.0_f32, f32::max);
    assert!(
        scaled_main < main_peak * 0.4,
        "set_param Volume=0.25 should reduce Main Out: before={main_peak}, after={scaled_main}"
    );

    // Shifting the part's base output pair to 2 moves stems to host outputs 2 and 3.
    rack.set_mix(
        part,
        Mix {
            output: 2,
            ..Mix::DEFAULT
        },
    );
    audio.process(512, &[]);
    let out0 = audio
        .output(0, 512)
        .iter()
        .map(|f| f[0].abs())
        .fold(0.0_f32, f32::max);
    let out2 = audio
        .output(2, 512)
        .iter()
        .map(|f| f[0].abs())
        .fold(0.0_f32, f32::max);
    let out3 = audio
        .output(3, 512)
        .iter()
        .map(|f| f[0].abs())
        .fold(0.0_f32, f32::max);
    assert!(out0 < 1e-6, "output 0 should now be silent: {out0}");
    assert!(
        out2 > 0.01 && out3 > 0.2,
        "stems should land on outputs 2 & 3: {out2}, {out3}"
    );
}
