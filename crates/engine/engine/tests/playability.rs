//! Playability end to end: what a live keyboard and a sequencer do to a
//! freshly loaded instrument whose samples are not in memory yet. Dense
//! chords and repeats far past the voice limit, notes at odd frames inside
//! host blocks, data that arrives late. Every note starts on its own frame
//! and keeps its own timeline; late data plays as silence in place, never
//! later; voices are stolen only for the notes past the limit; nothing is
//! dropped.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use kontra_engine::{Engine, Plan, Timed, Zone};
use kontra_events::{Event, HostNote, HostPattern};
use kontra_stream::{BlobKey, Encoding, SampleFmt, Source, Store, StoreConfig};
use kontra_time::SampleTime;
use kontra_voice::Region;

const RATE: u32 = 48_000;
const BLOCK: usize = 512;
const VOICES: usize = 8;
const FRAMES: u64 = 4 * RATE as u64;
const FMT: SampleFmt = SampleFmt {
    encoding: Encoding::F32,
    channels: 1,
};

/// A disk that answers only while its gate is open.
struct Gated(Arc<AtomicBool>);

impl Source for Gated {
    fn read(&self, first: u64, out: &mut [u8]) -> std::io::Result<()> {
        while !self.0.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(1));
        }
        for (n, frame) in out.chunks_exact_mut(4).enumerate() {
            frame.copy_from_slice(&value(first + n as u64).to_le_bytes());
        }
        Ok(())
    }
}

/// Opens the gate when dropped, so a failing test never leaves the store's
/// readers waiting on it.
struct Opens(Arc<AtomicBool>);

impl Drop for Opens {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

/// Every sample's frame `n`: noise-like, never zero.
fn value(n: u64) -> f32 {
    0.25 + 0.25 * ((n * 7919 % 101) as f32 / 101.0)
}

/// A zone on `key` alone, at the engine's rate with its root on the key:
/// its cursor moves one frame a frame, so it says when its note started.
fn zone(store: &mut Store, gate: &Arc<AtomicBool>, key: u8, pan: f32, cold: bool) -> Zone {
    let file = format!("{}{key}", if cold { "cold" } else { "warm" });
    let pin = if cold { 0..0 } else { 0..FRAMES };
    if !cold {
        gate.store(true, Ordering::Release);
    }
    let sample = store
        .load(
            &BlobKey {
                file: file.into(),
                offset: 0,
                fmt: FMT,
            },
            Arc::new(Gated(Arc::clone(gate))),
            FRAMES,
            std::slice::from_ref(&pin),
        )
        .unwrap();
    gate.store(false, Ordering::Release);
    let region = Region {
        mix: None,
        table: None,
        sample,
        start: 0,
        end: FRAMES,
        loops: vec![],
        reverse: false,
    };
    Zone {
        keys: key..=key,
        pan,
        ..Zone::new(region, key, RATE)
    }
}

fn host(key: u8) -> HostNote {
    HostNote {
        port: 0,
        channel: 0,
        key,
        id: -1,
        clap: true,
    }
}

fn on(at: u64, key: u8) -> Timed<Event> {
    Timed {
        at: SampleTime(at),
        body: Event::NoteOn {
            note: host(key),
            velocity: 0.8,
            tune: 0.0,
        },
    }
}

fn off(at: u64, key: u8) -> Timed<Event> {
    Timed {
        at: SampleTime(at),
        body: Event::NoteOff(HostPattern {
            port: 0,
            channel: 0,
            key: i32::from(key),
            id: -1,
            clap: true,
        }),
    }
}

/// Renders the blocks from frame `from` up to `to`, each with its events,
/// checking after each that every voice's cursor sits exactly as far past
/// one of its key's note-ons as the block end is.
fn play(
    engine: &mut Engine,
    events: &[Timed<Event>],
    from: u64,
    to: u64,
    mut after: impl FnMut(&Engine),
) -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    let mut block = [[0.0; 2]; BLOCK];
    let mut at = from;
    while at < to {
        let end = at + BLOCK as u64;
        let input: Vec<_> = (events.iter().copied())
            .filter(|e| (at..end).contains(&e.at.0))
            .collect();
        engine.process(&mut block, &input);
        for v in engine.inspect_voices() {
            let key = KEYS.get(v.zone).copied().unwrap_or(0);
            let started = (events.iter())
                .filter(|e| matches!(e.body, Event::NoteOn { note, .. } if note.key == key))
                .any(|e| (end - e.at.0) as f64 == v.position);
            assert!(
                started,
                "a voice on key {key} at {} frames by {end}: no note-on that far back",
                v.position
            );
        }
        after(engine);
        out.extend_from_slice(&block);
        at = end;
    }
    out
}

/// The zones' keys, zone by zone: key 60 warm and cold, 48 cold keys
/// around it, then 90 for the repeats.
const KEYS: [u8; 51] = {
    let mut keys = [0; 51];
    keys[0] = 60;
    keys[1] = 60;
    let mut n = 2;
    while n < 50 {
        keys[n] = 34 + n as u8 + (n >= 26) as u8;
        n += 1;
    }
    keys[50] = 90;
    keys
};

#[test]
fn a_cold_instrument_under_dense_input_starts_every_note_on_its_frame() {
    let gate = Arc::new(AtomicBool::new(false));
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let _opens = Opens(Arc::clone(&gate));
    // Key 60 twice: hard left in memory, hard right on the slow disk.
    let zones = (KEYS.iter().enumerate())
        .map(|(n, &key)| {
            zone(
                &mut store,
                &gate,
                key,
                if n == 0 { -1.0 } else { 1.0 },
                n > 0,
            )
        })
        .collect();
    let mut plan = Plan::new(zones, VOICES);
    for g in &mut plan.groups {
        g.mods.clear();
        g.envelope.release = 2_000;
    }
    // Just loaded: the first block already plays, mid-block.
    let mut engine = Engine::new(Arc::new(plan), store.client(), RATE);

    // 1. A note with no data yet, then the disk catches up.
    let first = [on(77, 60), off(20_000, 60)];
    let closed = play(&mut engine, &first, 0, 5 * BLOCK as u64, |_| {});
    let missing = engine.stats().missing_frames;
    assert!(missing > 0, "the cold layer had data at once");
    assert!(closed.iter().all(|f| f[1] == 0.0), "the cold layer sounded");
    assert_eq!(closed.iter().position(|f| f[0] != 0.0), Some(77));
    gate.store(true, Ordering::Release);
    let start = 94 * BLOCK as u64;
    let open = play(&mut engine, &first, 5 * BLOCK as u64, start, |_| {
        assert!(store.client().settle(Duration::from_secs(5)));
    });
    let arrived = open
        .iter()
        .position(|f| f[1] != 0.0)
        .expect("data never came");
    // In place, not late: once there it is the warm layer, frame for frame.
    for (n, f) in open.iter().enumerate().skip(arrived) {
        assert!(
            (f[0] - f[1]).abs() < 1e-6,
            "frame {n}: late data played {} where the note is at {}",
            f[1],
            f[0]
        );
    }
    assert_eq!(engine.active_voices(), 0);

    // 2. The disk stalls again: 48 held notes on fresh keys in chords of
    // six, MIDI-staggered at odd frames, then 24 repeats of one key.
    gate.store(false, Ordering::Release);
    let mut dense = Vec::new();
    for chord in 0..8u64 {
        let at = start + chord * 1_537 + chord * 37 % 511;
        for n in 0..6u64 {
            dense.push(on(at + n * 3, KEYS[(2 + chord * 6 + n) as usize]));
        }
    }
    for n in 0..24u64 {
        dense.push(on(start + 14_000 + n * 613, 90));
    }
    let end = start + 14_000 + 24 * 613 + 4 * BLOCK as u64;
    let mut peak = 0;
    play(&mut engine, &dense, start, end, |e| {
        peak = peak.max(e.playing_voices());
    });
    gate.store(true, Ordering::Release);
    let stats = engine.stats();
    eprintln!(
        "playability: {} notes at a limit of {VOICES}: peak {peak} playing, {} stolen, {} cut, \
         {} refused, {} events refused, {} frames missing",
        dense.len(),
        stats.voices_stolen,
        stats.voices_cut,
        stats.notes_refused,
        stats.events_refused,
        stats.missing_frames - missing,
    );
    assert_eq!(peak, VOICES);
    assert_eq!(stats.voices_stolen as usize, dense.len() - VOICES);
    assert_eq!((stats.notes_refused, stats.events_refused), (0, 0));
    assert!(stats.missing_frames > missing);
}
