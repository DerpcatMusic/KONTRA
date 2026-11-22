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
    let spec = from_ir(&loaded, Some(&dir));
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
