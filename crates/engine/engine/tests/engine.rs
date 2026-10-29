use std::sync::Arc;

use kontra_engine::{Engine, Plan, Timed, Zone};
use kontra_events::{Event, HostNote, HostPattern};
use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt, Store, StoreConfig};
use kontra_time::SampleTime;
use kontra_voice::Region;

const RATE: u32 = 48_000;
const FMT: SampleFmt = SampleFmt {
    encoding: Encoding::F32,
    channels: 1,
};

/// A 0.5-amplitude noise-like sample, pinned whole, so onsets are unambiguous.
fn region(store: &mut Store, name: &str, frames: u64) -> std::io::Result<Region> {
    let bytes = (0..frames)
        .flat_map(|n| (0.25 + 0.25 * ((n * 7919 % 101) as f32 / 101.0)).to_le_bytes())
        .collect();
    let key = BlobKey {
        file: name.into(),
        offset: 0,
        fmt: FMT,
    };
    let sample = store.load(
        &key,
        Arc::new(MemSource::new(FMT, bytes)),
        frames,
        std::slice::from_ref(&(0..frames)),
    )?;
    Ok(Region {
        mix: None,
        table: None,
        sample,
        start: 0,
        end: frames,
        loops: vec![],
        reverse: false,
    })
}

fn zone(region: Region, pan: f32) -> Zone {
    Zone {
        pan,
        ..Zone::new(region, 60, 44_100)
    }
}

fn plan(zones: Vec<Zone>, voices: usize) -> Arc<Plan> {
    let mut plan = Plan {
        notes: 64,
        queue: 256,
        ..Plan::new(zones, voices)
    };
    for g in &mut plan.groups {
        g.envelope.release = 2_000;
    }
    Arc::new(plan)
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
    let pattern = HostPattern {
        port: 0,
        channel: 0,
        key: i32::from(key),
        id: -1,
        clap: true,
    };
    Timed {
        at: SampleTime(at),
        body: Event::NoteOff(pattern),
    }
}

fn cc(at: u64, controller: u8, value: u8) -> Timed<Event> {
    Timed {
        at: SampleTime(at),
        body: Event::midi1(0xB0, controller, value),
    }
}

/// Renders `frames` at host block size `block`, handing each block its events.
fn render(
    engine: &mut Engine,
    events: &[Timed<Event>],
    frames: usize,
    block: usize,
) -> Vec<[f32; 2]> {
    let mut out = vec![[0.0; 2]; frames];
    for (i, chunk) in out.chunks_mut(block).enumerate() {
        let (from, to) = ((i * block) as u64, (i * block + chunk.len()) as u64);
        let input: Vec<_> = events
            .iter()
            .copied()
            .filter(|e| (from..to).contains(&e.at.0))
            .collect();
        engine.process(chunk, &input);
    }
    out
}

fn first_sound(out: &[[f32; 2]], channel: usize) -> Option<usize> {
    out.iter()
        .position(|f| f.get(channel).is_some_and(|v| *v != 0.0))
}

#[test]
fn output_is_identical_at_every_host_block_size() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let plan = plan(
        vec![zone(region(&mut store, "a", 20_000).unwrap(), 0.0)],
        16,
    );
    let events = [
        on(10, 60),
        on(333, 67),
        cc(400, 64, 127),
        off(500, 60),
        on(777, 71),
        off(901, 67),
        cc(1500, 64, 0),
    ];
    let reference = render(
        &mut Engine::new(plan.clone(), store.client(), RATE),
        &events,
        6000,
        128,
    );
    for block in [1, 32, 64, 97, 512, 1024] {
        let out = render(
            &mut Engine::new(plan.clone(), store.client(), RATE),
            &events,
            6000,
            block,
        );
        assert!(out == reference, "block {block} differs");
    }
}

#[test]
fn every_layer_of_a_note_starts_on_the_notes_sample() {
    // Two mic layers, hard left and hard right: both channels must start on frame 77.
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![
        zone(region(&mut store, "close", 5000).unwrap(), -1.0),
        zone(region(&mut store, "room", 5000).unwrap(), 1.0),
    ];
    let mut engine = Engine::new(plan(zones, 8), store.client(), RATE);
    let out = render(&mut engine, &[on(77, 60)], 512, 64);
    assert_eq!(first_sound(&out, 0), Some(77));
    assert_eq!(first_sound(&out, 1), Some(77));
    assert_eq!(engine.active_voices(), 2);
}

#[test]
fn the_pedal_holds_released_notes_until_it_lifts() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut z = zone(region(&mut store, "loop", 1000).unwrap(), 0.0);
    z.region.loops = vec![kontra_voice::Loop {
        start: 0,
        end: 1000,
        crossfade: 0,
        ..Default::default()
    }];
    let mut engine = Engine::new(plan(vec![z], 8), store.client(), RATE);
    render(
        &mut engine,
        &[cc(0, 64, 127), on(1, 60), off(100, 60)],
        4096,
        128,
    );
    assert_eq!(engine.active_voices(), 1, "held by the pedal");
    render(&mut engine, &[cc(4096, 64, 0)], 8192, 128);
    assert_eq!(engine.active_voices(), 0, "released when the pedal lifts");
}

#[test]
fn a_released_note_ends_and_is_reported() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut engine = Engine::new(
        plan(vec![zone(region(&mut store, "a", 48_000).unwrap(), 0.0)], 8),
        store.client(),
        RATE,
    );
    render(&mut engine, &[on(0, 60), off(64, 60)], 128, 128);
    let mut ended = Vec::new();
    for _ in 0..200 {
        engine.process(&mut [[0.0; 2]; 128], &[]);
        ended.extend_from_slice(engine.ended());
    }
    assert_eq!(ended, vec![host(60)]);
}

#[test]
fn a_note_off_among_controllers_in_one_block_still_releases_the_note() {
    // Controllers at the note-off's own sample and either side of it, all
    // in the block that carries it: the note must still end.
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut engine = Engine::new(
        plan(
            vec![zone(region(&mut store, "a", 480_000).unwrap(), 0.0)],
            8,
        ),
        store.client(),
        RATE,
    );
    let events = [
        on(0, 60),
        cc(599, 1, 64),
        cc(600, 11, 100),
        off(600, 60),
        cc(600, 7, 90),
        cc(601, 74, 20),
    ];
    render(&mut engine, &events, 1024, 1024);
    let mut ended = Vec::new();
    for _ in 0..400 {
        engine.process(&mut [[0.0; 2]; 128], &[]);
        ended.extend_from_slice(engine.ended());
    }
    assert_eq!(engine.active_voices(), 0);
    assert_eq!(ended, vec![host(60)]);
}

#[test]
fn a_full_pool_fades_the_oldest_voice_and_counts_it() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut engine = Engine::new(
        plan(vec![zone(region(&mut store, "a", 48_000).unwrap(), 0.0)], 2),
        store.client(),
        RATE,
    );
    render(&mut engine, &[on(0, 60), on(10, 62), on(20, 64)], 128, 128);
    assert_eq!(engine.playing_voices(), 2);
    assert_eq!(
        engine.active_voices(),
        3,
        "the stolen voice fades out, no click"
    );
    render(&mut engine, &[], 1024, 128);
    assert_eq!(engine.active_voices(), 2);
    assert_eq!(engine.stats().voices_stolen, 1);
}

#[test]
fn a_plan_that_steals_whole_groups_fades_a_notes_every_voice_in_the_group() {
    // A note sounds two zones of one group: the third voice steals both of the
    // oldest note's, not half of it.
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let a = region(&mut store, "a", 48_000).unwrap();
    let b = region(&mut store, "b", 48_000).unwrap();
    for (whole, stolen, playing) in [(false, 1, 3), (true, 2, 2)] {
        let mut p = Arc::unwrap_or_clone(plan(vec![zone(a.clone(), 0.0), zone(b.clone(), 0.0)], 3));
        p.steal_whole_groups = whole;
        let mut engine = Engine::new(Arc::new(p), store.client(), RATE);
        render(&mut engine, &[on(0, 60), on(10, 62)], 128, 128);
        let stats = engine.stats();
        assert_eq!(
            (stats.voices_stolen, engine.playing_voices()),
            (stolen, playing),
            "whole {whole}"
        );
    }
}

#[test]
fn a_zone_outside_the_velocity_range_is_silent() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut z = zone(region(&mut store, "soft", 1000).unwrap(), 0.0);
    z.velocities = 1..=40;
    let mut engine = Engine::new(plan(vec![z], 4), store.client(), RATE);
    let out = render(&mut engine, &[on(0, 60)], 256, 128);
    assert_eq!(first_sound(&out, 0), None);
}

/// 512 MIDI events in one block at production sizes (128 notes): 256 short
/// notes over 64 voices. None is refused; only the voice limit steals.
#[test]
fn a_burst_of_512_events_in_one_block_loses_no_note() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(
        vec![zone(region(&mut store, "a", 48_000).unwrap(), 0.0)],
        64,
    );
    for g in &mut plan.groups {
        g.envelope.release = 200_000;
    }
    let mut engine = Engine::new(Arc::new(plan), store.client(), RATE);
    let events: Vec<_> = (0..256u64)
        .flat_map(|n| {
            [
                on(2 * n, 36 + (n % 64) as u8),
                off(2 * n + 1, 36 + (n % 64) as u8),
            ]
        })
        .collect();
    render(&mut engine, &events, 512, 512);
    let stats = engine.stats();
    assert_eq!(
        (stats.notes_refused, stats.events_refused),
        (0, 0),
        "{stats:?}"
    );
    // Only the voice limit steals; a burst past the fade reserve ends some
    // of those fades early (voices_cut), never a note still owed a voice.
    assert_eq!(stats.voices_stolen, 256 - 64, "{stats:?}");
    assert_eq!(engine.playing_voices(), 64);
}

/// An instrument allowing 512 voices holds 300 keys at once: the note
/// table grows with the voice limit, so none is refused or stolen.
#[test]
fn the_note_table_follows_the_instruments_voice_limit() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let plan = Plan::new(
        vec![zone(region(&mut store, "a", 48_000).unwrap(), 0.0)],
        512,
    );
    let mut engine = Engine::new(Arc::new(plan), store.client(), RATE);
    // 300 held notes over 16 channels; none is released.
    let events: Vec<_> = (0..300u64)
        .map(|n| {
            let mut e = on(n, 30 + (n % 60) as u8);
            if let Event::NoteOn { note, .. } = &mut e.body {
                note.channel = (n / 60) as u8;
            }
            e
        })
        .collect();
    render(&mut engine, &events, 512, 512);
    let stats = engine.stats();
    assert_eq!(
        (stats.notes_refused, stats.voices_stolen),
        (0, 0),
        "{stats:?}"
    );
    assert_eq!(engine.playing_voices(), 300);
}
