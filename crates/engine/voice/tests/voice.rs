use std::sync::Arc;

use kontra_stream::{
    BlobKey, Encoding, MemSource, PAGE_FRAMES, SampleFmt, Store, StoreConfig, StreamClient,
};
use kontra_voice::{Ahdsr, Loop, Region, Voice, VoiceStart};

const FMT: SampleFmt = SampleFmt {
    encoding: Encoding::F32,
    channels: 1,
};

/// Frame n holds n / 65536 exactly, so interpolation at integer steps is exact.
fn ramp_region(store: &Store, frames: u64, pinned: u64) -> std::io::Result<Region> {
    let bytes = (0..frames)
        .flat_map(|n| (n as f32 / 65_536.0).to_le_bytes())
        .collect();
    let key = BlobKey {
        file: format!("ramp{frames}-{pinned}").into(),
        offset: 0,
        fmt: FMT,
    };
    let sample = store.load(
        &key,
        Arc::new(MemSource::new(FMT, bytes)),
        frames,
        std::slice::from_ref(&(0..pinned)),
    )?;
    Ok(Region {
        sample,
        start: 0,
        end: frames,
        looped: None,
        reverse: false,
    })
}

fn start(region: Region, step: f64, envelope: Ahdsr) -> VoiceStart {
    VoiceStart {
        region,
        step,
        gain: [1.0, 1.0],
        envelope,
    }
}

fn render(voice: &mut Voice, client: &StreamClient, frames: usize) -> Vec<f32> {
    let mut out = vec![[0.0; 2]; frames];
    for (i, block) in out.chunks_mut(128).enumerate() {
        voice.render(client, (i * 128) as u64, block);
    }
    out.iter().map(|f| f[0]).collect()
}

#[test]
fn unity_pitch_plays_the_sample_frame_for_frame() {
    let store = Store::new(StoreConfig::default()).unwrap();
    let client = store.client();
    let mut voice = Voice::new();
    voice.start(start(
        ramp_region(&store, 4096, 4096).unwrap(),
        1.0,
        Ahdsr::GATE,
    ));
    let out = render(&mut voice, &client, 1000);
    for (n, v) in out.iter().enumerate() {
        assert_eq!(*v, n as f32 / 65_536.0, "frame {n}");
    }
}

#[test]
fn an_octave_up_reads_every_second_frame_and_ends_twice_as_fast() {
    let store = Store::new(StoreConfig::default()).unwrap();
    let client = store.client();
    let mut voice = Voice::new();
    voice.start(start(
        ramp_region(&store, 2048, 2048).unwrap(),
        2.0,
        Ahdsr::GATE,
    ));
    let out = render(&mut voice, &client, 1100);
    assert_eq!(out[100], 200.0 / 65_536.0);
    assert!(!voice.is_active());
    assert!(out[1030..].iter().all(|v| *v == 0.0));
}

#[test]
fn a_page_not_in_memory_is_silent_and_the_cursor_keeps_time() {
    let store = Store::new(StoreConfig {
        request_capacity: 1,
        ..StoreConfig::default()
    })
    .unwrap();
    let client = store.client();
    let mut voice = Voice::new();
    voice.start(start(
        ramp_region(&store, 8 * PAGE_FRAMES, PAGE_FRAMES).unwrap(),
        1.0,
        Ahdsr::GATE,
    ));
    let mut out = vec![[0.0; 2]; 2 * PAGE_FRAMES as usize];
    voice.render(&client, 0, &mut out);
    assert_eq!(out[10][0], 10.0 / 65_536.0);
    assert!(voice.missing_frames() > 0);
    // Late audio never shifts the timeline: the cursor is exactly where the clock is.
    assert_eq!(voice.position(), 2.0 * PAGE_FRAMES as f64);
}

#[test]
fn a_loop_sustains_past_the_sample_end() {
    let store = Store::new(StoreConfig::default()).unwrap();
    let client = store.client();
    let mut region = ramp_region(&store, 1000, 1000).unwrap();
    region.looped = Some(Loop {
        start: 500,
        end: 600,
    });
    let mut voice = Voice::new();
    voice.start(start(region, 1.0, Ahdsr::GATE));
    let out = render(&mut voice, &client, 5000);
    assert!(voice.is_active());
    // Frame 4950 is 4450 past the loop start: 4450 % 100 = 50 → source frame 550.
    assert_eq!(out[4950], 550.0 / 65_536.0);
}

#[test]
fn reverse_plays_from_the_end() {
    let store = Store::new(StoreConfig::default()).unwrap();
    let client = store.client();
    let mut region = ramp_region(&store, 1000, 1000).unwrap();
    region.reverse = true;
    let mut voice = Voice::new();
    voice.start(start(region, 1.0, Ahdsr::GATE));
    let out = render(&mut voice, &client, 10);
    assert_eq!(out[0], 999.0 / 65_536.0);
    assert_eq!(out[9], 990.0 / 65_536.0);
}

#[test]
fn release_ends_the_voice() {
    let store = Store::new(StoreConfig::default()).unwrap();
    let client = store.client();
    let mut region = ramp_region(&store, 1000, 1000).unwrap();
    region.looped = Some(Loop {
        start: 0,
        end: 1000,
    });
    let mut voice = Voice::new();
    voice.start(start(
        region,
        1.0,
        Ahdsr {
            release: 256,
            ..Ahdsr::GATE
        },
    ));
    render(&mut voice, &client, 256);
    voice.release();
    render(&mut voice, &client, 4096);
    assert!(!voice.is_active());
}
