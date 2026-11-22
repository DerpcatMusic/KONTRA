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
