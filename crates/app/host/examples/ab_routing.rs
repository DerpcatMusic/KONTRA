//! A/B of the engine's routing on one instrument against a reference
//! render: two engines render the same events in lockstep (the second is
//! the noise floor: streaming and scripts must make it bit-exact), and
//! `KONTRA_AB_REF=<dir>` compares the render with `<dir>/<name>-rack.wav`,
//! the per-bus rack path's render saved before it was deleted. Every
//! requested page is delivered before the next block, so the render does
//! not depend on disk timing. Spec: docs/specs/effect-racks-on-audiograph.md.
//!
//! `cargo run --release -p kontra-host --example ab_routing -- <file> [keys] [seconds]`
//! `keys` is a comma list (default 48,55,60,64), held for 1.5 s; `seconds` is
//! the whole render (default 4). Prints one line: levels and the residuals
//! relative to the rack render's peak, in dB.

#![allow(clippy::disallowed_types, clippy::print_stdout)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use kontra_engine::{Engine, Event, HostNote, HostPattern, Plan, Prepared, SampleTime, Timed};
use kontra_stream::{Store, StoreConfig, StreamClient};

const RATE: u32 = 48_000;
const BLOCK: usize = 512;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path: PathBuf = args
        .next()
        .ok_or("usage: ab_routing <file> [keys] [seconds]")?
        .into();
    let keys: Vec<u8> = args
        .next()
        .and_then(|k| k.into_string().ok())
        .unwrap_or_else(|| "48,55,60,64".into())
        .split(',')
        .filter_map(|k| k.trim().parse().ok())
        .collect();
    let seconds: f32 = args
        .next()
        .and_then(|s| s.into_string().ok()?.parse().ok())
        .unwrap_or(4.0);

    let mut store = Store::new(StoreConfig::DEFAULT)?;
    let ir = kontra_host::instrument::read(&path)?;
    let loaded = kontra_host::instrument::build(ir, RATE, &mut store);
    let stream = store.client();
    // KONTRA_AB_BUSES=1: what each bus carries, the features a graph bus must match.
    if std::env::var_os("KONTRA_AB_BUSES").is_some() {
        for (n, b) in loaded.plan.buses.iter().enumerate() {
            let kinds: Vec<_> = b
                .inserts
                .iter()
                .flatten()
                .map(|e| format!("{:?}", e.kind).chars().take(24).collect::<String>())
                .collect();
            println!(
                "bus {n} {:?}: gain {} pan {} to_main {} enabled {} sends {:?} mods {} lfos {} inserts {kinds:?}",
                b.name,
                b.gain,
                b.pan,
                b.to_main,
                b.enabled,
                b.sends,
                b.mods.len(),
                b.lfos.len()
            );
        }
        let groups = loaded
            .plan
            .groups
            .iter()
            .filter(|g| g.inserts.iter().any(Option::is_some))
            .count();
        println!(
            "{groups} groups with inserts; instrument inserts {} sends {} main {}",
            loaded.plan.inserts.iter().flatten().count(),
            loaded.plan.sends.iter().flatten().count(),
            loaded.plan.main.iter().flatten().count()
        );
    }
    let plan = Arc::new(loaded.plan);
    let mut engines = [engine(&plan, &stream), engine(&plan, &stream)];
    let note = |key: u8, on: bool| Timed {
        at: SampleTime(0),
        body: if on {
            Event::NoteOn {
                note: HostNote {
                    port: 0,
                    channel: 0,
                    key,
                    id: -1,
                    clap: true,
                },
                velocity: (100.0 / 127.0f32).into(),
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
        },
    };
    let ons: Vec<_> = keys.iter().map(|&k| note(k, true)).collect();
    let offs: Vec<_> = keys.iter().map(|&k| note(k, false)).collect();
    let (lead, hold) = (RATE as usize / 4 / BLOCK, RATE as usize * 3 / 2 / BLOCK);
    let total = (seconds * RATE as f32) as usize / BLOCK;
    let mut audio: [Vec<[f32; 2]>; 2] = Default::default();
    let mut block = vec![[0.0f32; 2]; BLOCK];
    for n in 0..lead + total {
        let events: &[Timed<Event>] = if n == lead {
            &ons
        } else if n == lead + hold {
            &offs
        } else {
            &[]
        };
        for (e, out) in engines.iter_mut().zip(audio.iter_mut()) {
            block.fill([0.0; 2]);
            e.process(&mut block, events);
            install_requests(e, &plan);
            if n >= lead {
                out.extend_from_slice(&block);
            }
        }
        // Every page asked for this block arrives before the next one.
        stream.settle(Duration::from_secs(5));
    }
    let [a, again] = &audio;
    let reference = match std::env::var_os("KONTRA_AB_REF") {
        Some(dir) => Some(read_wav(
            &PathBuf::from(dir).join(format!("{}-rack.wav", loaded.name)),
        )?),
        None => None,
    };
    let peak = |x: &[[f32; 2]]| x.iter().flatten().fold(0f32, |p, s| p.max(s.abs()));
    let rms = |x: &[f32]| {
        (x.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
    };
    let residual = |x: &[[f32; 2]], y: &[[f32; 2]]| -> (f64, f32) {
        let d: Vec<f32> = x
            .iter()
            .flatten()
            .zip(y.iter().flatten())
            .map(|(p, q)| q - p)
            .collect();
        (rms(&d), d.iter().fold(0f32, |m, s| m.max(s.abs())))
    };
    let db = |v: f64| {
        if v > 0.0 {
            format!("{:.1}", 20.0 * v.log10())
        } else {
            "-inf".into()
        }
    };
    let ref_peak = f64::from(peak(reference.as_deref().unwrap_or(a))).max(1e-12);
    let (floor_rms, floor_peak) = residual(a, again);
    let flat = |x: &[[f32; 2]]| x.iter().flatten().copied().collect::<Vec<f32>>();
    let versus = reference.as_deref().map_or_else(String::new, |r| {
        let (ab_rms, ab_peak) = residual(r, a);
        let n = if r.len() == a.len() {
            String::new()
        } else {
            format!(" LENGTH {} vs {}", r.len(), a.len())
        };
        format!(
            "; vs reference rms {} peak {} dB{n}",
            db(ab_rms / ref_peak),
            db(f64::from(ab_peak) / ref_peak)
        )
    });
    println!(
        "{}: keys {keys:?} {seconds} s; peak {} dBFS rms {} dBFS; floor rms {} peak {} dB{versus} (relative to the reference peak)",
        loaded.name,
        db(f64::from(peak(a))),
        db(rms(&flat(a))),
        db(floor_rms / ref_peak),
        db(f64::from(floor_peak) / ref_peak),
    );
    if let Some(dir) = std::env::var_os("KONTRA_AB_WAV") {
        write_wav(
            &PathBuf::from(&dir).join(format!("{}-graph.wav", loaded.name)),
            a,
        )?;
    }
    Ok(())
}

fn engine(plan: &Arc<Plan>, stream: &StreamClient) -> Engine {
    let mut engine = Engine::new(Arc::clone(plan), stream.clone(), RATE);
    if let Some(p) = kontra_prepare::tables(engine.plan(), stream) {
        engine.swap_plan(&mut Arc::new(p), &[]);
    }
    engine
}

/// Effect loads the scripts ask for, installed as the host does.
fn install_requests(engine: &mut Engine, plan: &Plan) {
    let mut asked = Vec::new();
    engine.drain_requests(|r| asked.push(r));
    for r in asked {
        engine.install(Prepared::new(r, Some(plan), RATE));
    }
}

/// A float stereo WAV as [`write_wav`] writes it.
fn read_wav(path: &std::path::Path) -> std::io::Result<Vec<[f32; 2]>> {
    let bytes = std::fs::read(path)?;
    let data = bytes.get(44..).unwrap_or_default();
    let (frames, _) = data.as_chunks::<8>();
    Ok(frames
        .iter()
        .map(|&[a, b, c, d, e, f, g, h]| {
            [
                f32::from_le_bytes([a, b, c, d]),
                f32::from_le_bytes([e, f, g, h]),
            ]
        })
        .collect())
}

fn write_wav(path: &std::path::Path, audio: &[[f32; 2]]) -> std::io::Result<()> {
    let data = u32::try_from(audio.len() * 8).unwrap_or(u32::MAX);
    let mut bytes = Vec::with_capacity(44 + audio.len() * 8);
    for chunk in [
        &b"RIFF"[..],
        &(36 + data).to_le_bytes(),
        b"WAVEfmt ",
        &16u32.to_le_bytes(),
    ] {
        bytes.extend_from_slice(chunk);
    }
    for v in [3u16, 2] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    for v in [RATE, RATE * 8] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    for v in [8u16, 32] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    for s in audio.iter().flatten() {
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, bytes)
}
