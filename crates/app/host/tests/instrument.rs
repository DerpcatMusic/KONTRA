//! Real instruments end to end: file → IR → plan → engine → sound.
#![cfg(test)]

use std::path::Path;
use std::sync::Arc;

use kontra_engine::{Engine, Event, HostNote, HostPattern, SampleTime, Timed};
use kontra_stream::{Store, StoreConfig};

const RATE: u32 = 48_000;
const AREIA: &str = "/data/main/Libraries/Kontakt/Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/01 Areia - 16 Violins - Core Techniques.nki";

fn note(key: u8, on: bool) -> Timed<Event> {
    let body = if on {
        Event::NoteOn {
            note: HostNote {
                port: 0,
                channel: 0,
                key,
                id: -1,
                clap: true,
            },
            velocity: 0.8,
            tune: 0.0,
        }
    } else {
        Event::NoteOff(HostPattern {
            port: 0,
            channel: 0,
            key: i32::from(key),
            id: -1,
            clap: true,
        })
    };
    Timed {
        at: SampleTime(0),
        body,
    }
}

#[test]
fn areia_violins_sound_on_the_fresh_engine() {
    let path = Path::new(AREIA);
    if !path.exists() {
        return;
    }
    let t = std::time::Instant::now();
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let loaded = kontra_host::instrument::load(path, RATE, &mut store).unwrap();
    let load = t.elapsed();
    eprintln!(
        "{}: {} zones, {} scripts, loaded in {load:?}",
        loaded.name,
        loaded.plan.zones.len(),
        loaded.plan.scripts.len()
    );
    for d in &loaded.diagnostics {
        eprintln!("  {:?} {}: {}", d.impact, d.feature, d.detail);
    }
    let mut e = Engine::new(Arc::new(loaded.plan), store.client(), RATE);
    let mut peak = 0f32;
    let mut out = vec![[0.0; 2]; 512];
    e.process(&mut out, &[note(60, true)]);
    for _ in 0..(RATE as usize / 512) {
        e.process(&mut out, &[]);
        peak = out
            .iter()
            .fold(peak, |p, f| p.max(f[0].abs()).max(f[1].abs()));
    }
    e.process(&mut out, &[note(60, false)]);
    eprintln!(
        "peak {peak}, stats {:?}, faults {:?}",
        e.stats(),
        e.script_faults()
    );
    assert!(peak > 1e-3, "a held C4 sounds");
}

#[test]
fn a_dspreset_loads_and_sounds() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("dspreset_e2e");
    std::fs::create_dir_all(&dir).unwrap();
    let frames = 24_000u32;
    let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    for field in [16u32, 1 | (1 << 16), RATE, RATE * 2, 2 | (16 << 16)] {
        wav.extend(field.to_le_bytes());
    }
    wav.extend(b"data");
    wav.extend((frames * 2).to_le_bytes());
    for i in 0..frames {
        let s = (std::f32::consts::TAU * 440.0 * i as f32 / RATE as f32).sin();
        wav.extend(((s * 20_000.0) as i16).to_le_bytes());
    }
    let riff = (wav.len() - 8) as u32;
    wav.splice(4..8, riff.to_le_bytes());
    std::fs::write(dir.join("tone.wav"), wav).unwrap();
    let preset = dir.join("Tone.dspreset");
    std::fs::write(
        &preset,
        r#"<DecentSampler><groups><group attack="0.001" release="0.05">
  <sample path="tone.wav" rootNote="A4" loNote="0" hiNote="127"/>
</group></groups></DecentSampler>"#,
    )
    .unwrap();

    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let loaded = kontra_host::instrument::load(&preset, RATE, &mut store).unwrap();
    assert_eq!(loaded.name, "Tone");
    let mut e = Engine::new(Arc::new(loaded.plan), store.client(), RATE);
    let mut out = vec![[0.0; 2]; 512];
    let mut peak = 0f32;
    e.process(&mut out, &[note(69, true)]);
    for _ in 0..8 {
        e.process(&mut out, &[]);
        peak = out
            .iter()
            .fold(peak, |p, f| p.max(f[0].abs()).max(f[1].abs()));
    }
    assert!(peak > 1e-3, "a held A4 sounds, peak {peak}");
}

#[test]
fn a_native_kontra_instrument_loads_and_sounds() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("kontra_native_e2e");
    std::fs::create_dir_all(&dir).unwrap();
    let frames = 24_000u32;
    let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    for field in [16u32, 1 | (1 << 16), RATE, RATE * 2, 2 | (16 << 16)] {
        wav.extend(field.to_le_bytes());
    }
    wav.extend(b"data");
    wav.extend((frames * 2).to_le_bytes());
    for i in 0..frames {
        let s = (std::f32::consts::TAU * 440.0 * i as f32 / RATE as f32).sin();
        wav.extend(((s * 20_000.0) as i16).to_le_bytes());
    }
    let riff = (wav.len() - 8) as u32;
    wav.splice(4..8, riff.to_le_bytes());
    std::fs::write(dir.join("tone.wav"), wav).unwrap();
    let preset = dir.join("NativeTone.kontra");
    std::fs::write(
        &preset,
        r#"version = 1
name = "Native Tone"
[[groups]]
name = "Main"
[groups.envelope]
attack = 0.001
release = 0.05
[[groups.zones]]
sample = "tone.wav"
keys = [0, 127]
root = 69
"#,
    )
    .unwrap();

    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let loaded = kontra_host::instrument::load(&preset, RATE, &mut store).unwrap();
    assert_eq!(loaded.name, "Native Tone");
    assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
    let mut e = Engine::new(Arc::new(loaded.plan), store.client(), RATE);
    let mut out = vec![[0.0; 2]; 512];
    let mut peak = 0f32;
    e.process(&mut out, &[note(69, true)]);
    for _ in 0..8 {
        e.process(&mut out, &[]);
        peak = out
            .iter()
            .fold(peak, |p, f| p.max(f[0].abs()).max(f[1].abs()));
    }
    assert!(peak > 1e-3, "a held A4 sounds, peak {peak}");
}

/// A SoundFont bank: one preset, one instrument, one looped 480 Hz sine
/// rooted at A4, its PCM read straight from the bank's `smpl` chunk.
#[test]
fn an_sf2_preset_loads_and_sounds_at_pitch() {
    fn chunk(id: &[u8], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }
    fn list(kind: &[u8], chunks: &[Vec<u8>]) -> Vec<u8> {
        chunk(b"LIST", &[kind.to_vec(), chunks.concat()].concat())
    }
    fn name(s: &str) -> Vec<u8> {
        let mut b = s.as_bytes().to_vec();
        b.resize(20, 0);
        b
    }
    fn words(ws: &[u16]) -> Vec<u8> {
        ws.iter().flat_map(|w| w.to_le_bytes()).collect()
    }
    const FRAMES: u32 = 4_800;
    let mut smpl: Vec<u8> = (0..FRAMES)
        .flat_map(|i| {
            let s = (std::f32::consts::TAU * 480.0 * i as f32 / RATE as f32).sin();
            ((s * 16_384.0) as i16).to_le_bytes()
        })
        .collect();
    smpl.resize(smpl.len() + 2 * 46, 0);
    let header = |n: &str, start: u32, end: u32, loop_: (u32, u32), rate: u32, kind: u16| {
        let mut h = name(n);
        for v in [start, end, loop_.0, loop_.1, rate] {
            h.extend(v.to_le_bytes());
        }
        h.extend([69, 0, 0, 0]); // original pitch A4, no correction, no link
        h.extend(kind.to_le_bytes());
        h
    };
    let phdr = [
        [name("Sine"), words(&[0, 0, 0]), vec![0; 12]].concat(),
        [name("EOP"), words(&[0, 0, 1]), vec![0; 12]].concat(),
    ]
    .concat();
    let pdta = list(
        b"pdta",
        &[
            chunk(b"phdr", &phdr),
            chunk(b"pbag", &words(&[0, 0, 1, 0])),
            chunk(b"pmod", &[0; 10]),
            chunk(b"pgen", &words(&[41, 0, 0, 0])), // instrument 0
            chunk(
                b"inst",
                &[name("Sine"), words(&[0]), name("EOI"), words(&[1])].concat(),
            ),
            chunk(b"ibag", &words(&[0, 0, 2, 0])),
            chunk(b"imod", &[0; 10]),
            chunk(b"igen", &words(&[54, 1, 53, 0, 0, 0])), // loop, sample 0
            chunk(
                b"shdr",
                &[
                    header("sine", 0, FRAMES, (100, 4_700), RATE, 1),
                    header("EOS", 0, 0, (0, 0), 0, 0),
                ]
                .concat(),
            ),
        ],
    );
    let body = [
        b"sfbk".to_vec(),
        list(
            b"INFO",
            &[chunk(b"ifil", &words(&[2, 1])), chunk(b"INAM", b"Test\0\0")],
        ),
        list(b"sdta", &[chunk(b"smpl", &smpl)]),
        pdta,
    ]
    .concat();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("sf2_e2e");
    std::fs::create_dir_all(&dir).unwrap();
    let bank = dir.join("Sine.sf2");
    std::fs::write(&bank, chunk(b"RIFF", &body)).unwrap();

    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let loaded = kontra_host::instrument::load(&bank, RATE, &mut store).unwrap();
    for d in &loaded.diagnostics {
        eprintln!("  {:?} {}: {}", d.impact, d.feature, d.detail);
    }
    let mut e = Engine::new(Arc::new(loaded.plan), store.client(), RATE);
    let mut out = vec![[0.0; 2]; 512];
    e.process(&mut out, &[note(69, true)]);
    // Past the attack, well into the loop: 0.5 s of held note.
    let mut held = vec![];
    for block in 0..(RATE as usize / 512) {
        e.process(&mut out, &[]);
        if block >= 20 {
            held.extend(out.iter().map(|f| f[0]));
        }
    }
    let peak = held.iter().fold(0f32, |p, s| p.max(s.abs()));
    let rises = held
        .windows(2)
        .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
        .count();
    let hz = rises as f32 * RATE as f32 / held.len() as f32;
    eprintln!("sf2: peak {peak}, {hz} Hz");
    assert!(
        (470.0..490.0).contains(&hz),
        "A4 plays the 480 Hz root, got {hz} Hz"
    );
    // The half-scale sine at velocity 0.8, squared by the velocity curve.
    assert!(
        (0.30..0.34).contains(&peak),
        "0.5 × 0.8² = 0.32, peak {peak}"
    );
}

/// A multi-preset SoundFont bank addressed by virtual slot path
/// (`Bank.sf2/000:001 OctaveUp.sf2p`) loads the requested preset, not just the first.
#[test]
fn an_sf2_slot_path_loads_a_specific_preset() {
    fn chunk(id: &[u8], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }
    fn list(kind: &[u8], chunks: &[Vec<u8>]) -> Vec<u8> {
        chunk(b"LIST", &[kind.to_vec(), chunks.concat()].concat())
    }
    fn name(s: &str) -> Vec<u8> {
        let mut b = s.as_bytes().to_vec();
        b.resize(20, 0);
        b
    }
    fn words(ws: &[u16]) -> Vec<u8> {
        ws.iter().flat_map(|w| w.to_le_bytes()).collect()
    }
    const FRAMES: u32 = 4_800;
    let mut smpl: Vec<u8> = (0..FRAMES)
        .flat_map(|i| {
            let s = (std::f32::consts::TAU * 480.0 * i as f32 / RATE as f32).sin();
            ((s * 16_384.0) as i16).to_le_bytes()
        })
        .collect();
    smpl.resize(smpl.len() + 2 * 46, 0);
    let header = |n: &str, start: u32, end: u32, loop_: (u32, u32), rate: u32, kind: u16| {
        let mut h = name(n);
        for v in [start, end, loop_.0, loop_.1, rate] {
            h.extend(v.to_le_bytes());
        }
        h.extend([69, 0, 0, 0]);
        h.extend(kind.to_le_bytes());
        h
    };
    // Preset 0:0 -> inst 0 (480 Hz); Preset 0:1 -> inst 0 with coarseTune +12 st (960 Hz).
    let phdr = [
        [name("Base"), words(&[0, 0, 0]), vec![0; 12]].concat(),
        [name("OctaveUp"), words(&[1, 0, 1]), vec![0; 12]].concat(),
        [name("EOP"), words(&[0, 0, 2]), vec![0; 12]].concat(),
    ]
    .concat();
    let pdta = list(
        b"pdta",
        &[
            chunk(b"phdr", &phdr),
            chunk(b"pbag", &words(&[0, 0, 1, 0, 3, 0])),
            chunk(b"pmod", &[0; 10]),
            chunk(
                b"pgen",
                &words(&[
                    41, 0, // preset 0:0 -> instrument 0
                    51, 12, // preset 0:1 -> coarseTune +12 semitones
                    41, 0, // preset 0:1 -> instrument 0
                    0, 0,
                ]),
            ),
            chunk(
                b"inst",
                &[name("Sine"), words(&[0]), name("EOI"), words(&[1])].concat(),
            ),
            chunk(b"ibag", &words(&[0, 0, 2, 0])),
            chunk(b"imod", &[0; 10]),
            chunk(b"igen", &words(&[54, 1, 53, 0, 0, 0])),
            chunk(
                b"shdr",
                &[
                    header("sine", 0, FRAMES, (100, 4_700), RATE, 1),
                    header("EOS", 0, 0, (0, 0), 0, 0),
                ]
                .concat(),
            ),
        ],
    );
    let body = [
        b"sfbk".to_vec(),
        list(
            b"INFO",
            &[chunk(b"ifil", &words(&[2, 1])), chunk(b"INAM", b"Multi\0")],
        ),
        list(b"sdta", &[chunk(b"smpl", &smpl)]),
        pdta,
    ]
    .concat();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("sf2_slot_e2e");
    std::fs::create_dir_all(&dir).unwrap();
    let bank = dir.join("Multi.sf2");
    std::fs::write(&bank, chunk(b"RIFF", &body)).unwrap();

    let slot_path = bank.join("000:001 OctaveUp.sf2p");
    assert_eq!(
        kontra_host::instrument::sf2_slot(&slot_path),
        Some((bank.as_path(), 0, 1))
    );

    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let loaded = kontra_host::instrument::load(&slot_path, RATE, &mut store).unwrap();
    assert_eq!(loaded.name, "OctaveUp");
    let mut e = Engine::new(Arc::new(loaded.plan), store.client(), RATE);
    let mut out = vec![[0.0; 2]; 512];
    e.process(&mut out, &[note(69, true)]);
    let mut held = vec![];
    for block in 0..(RATE as usize / 512) {
        e.process(&mut out, &[]);
        if block >= 20 {
            held.extend(out.iter().map(|f| f[0]));
        }
    }
    let rises = held
        .windows(2)
        .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
        .count();
    let hz = rises as f32 * RATE as f32 / held.len() as f32;
    assert!(
        (940.0..980.0).contains(&hz),
        "preset 0:1 should sound an octave up (~960 Hz), got {hz} Hz"
    );
}

#[test]
fn sfz_builtin_saw_oscillator_loads_and_sounds_at_concert_pitch() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("sfz_osc_e2e");
    std::fs::create_dir_all(&dir).unwrap();
    let sfz = dir.join("SawSynth.sfz");
    std::fs::write(
        &sfz,
        "<region> sample=*saw ampeg_attack=0.001 ampeg_release=0.05\n",
    )
    .unwrap();
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let loaded = kontra_host::instrument::load(&sfz, RATE, &mut store).unwrap();
    assert!(
        loaded.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        loaded.diagnostics
    );
    let mut e = Engine::new(Arc::new(loaded.plan), store.client(), RATE);
    let mut out = vec![[0.0; 2]; 512];
    e.process(&mut out, &[note(69, true)]);
    let mut held = Vec::new();
    for block in 0..16 {
        e.process(&mut out, &[]);
        if block >= 2 {
            held.extend(out.iter().map(|f| f[0]));
        }
    }
    let peak = held.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
    assert!(
        peak > 0.2,
        "saw oscillator should sound loudly, peak {peak}"
    );
    let rises = held
        .windows(2)
        .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
        .count();
    let hz = rises as f32 * RATE as f32 / held.len() as f32;
    assert!(
        (430.0..450.0).contains(&hz),
        "MIDI 69 should sound at ~440 Hz, got {hz} Hz"
    );
}

#[test]
fn native_kontra_oscillator_and_multi_stem_audio_graph_render() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("kontra_phase3_e2e");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("SplitSynth.kontra");
    let toml = r#"
version = 1
name = "SplitSynth"
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
name = "SineLead"

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
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let loaded = kontra_host::instrument::load(&path, RATE, &mut store).unwrap();
    assert!(
        loaded.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        loaded.diagnostics
    );
    let mut e = Engine::new(Arc::new(loaded.plan), store.client(), RATE);
    let mut main_out = vec![[0.0; 2]; 512];
    let mut aux_out = vec![[0.0; 2]; 512];
    let mut outs: [&mut [[f32; 2]]; 2] = [&mut main_out, &mut aux_out];
    e.process_multi(&mut outs, &[note(69, true)]);
    for _ in 0..8 {
        let mut outs: [&mut [[f32; 2]]; 2] = [&mut main_out, &mut aux_out];
        e.process_multi(&mut outs, &[]);
    }
    let main_peak = main_out.iter().map(|f| f[0].abs()).fold(0.0_f32, f32::max);
    let aux_peak = aux_out.iter().map(|f| f[0].abs()).fold(0.0_f32, f32::max);
    assert!(
        main_peak > 0.05 && aux_peak > 0.2,
        "main {main_peak} aux {aux_peak}"
    );
    // Main passes pre (0.5) * amp (0.5) * post (0.5) = 0.125; Aux taps pre-fader after pre (0.5).
    // Expected aux / main ratio is 4.0.
    let ratio = aux_peak / main_peak;
    assert!(
        (ratio - 4.0).abs() < 0.05,
        "expected aux/main ratio ~4.0, got {ratio} (aux={aux_peak}, main={main_peak})"
    );
}
