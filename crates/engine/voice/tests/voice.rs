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
fn ramp_region(store: &mut Store, frames: u64, pinned: u64) -> std::io::Result<Region> {
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
        mix: None,
        table: None,
        sample,
        start: 0,
        end: frames,
        loops: vec![],
        reverse: false,
    })
}

fn start(region: Region, step: f64, envelope: Ahdsr) -> VoiceStart {
    VoiceStart {
        region,
        step,
        gain: [[1.0, 0.0], [0.0, 1.0]],
        lag: kontra_voice::Lag::NONE,
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
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let mut voice = Voice::allocate();
    voice.start(start(
        ramp_region(&mut store, 4096, 4096).unwrap(),
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
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let mut voice = Voice::allocate();
    voice.start(start(
        ramp_region(&mut store, 2048, 2048).unwrap(),
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
    let mut store = Store::new(StoreConfig {
        request_capacity: 1,
        ..StoreConfig::DEFAULT
    })
    .unwrap();
    let client = store.client();
    let mut voice = Voice::allocate();
    voice.start(start(
        ramp_region(&mut store, 8 * PAGE_FRAMES, PAGE_FRAMES).unwrap(),
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
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let mut region = ramp_region(&mut store, 1000, 1000).unwrap();
    region.loops = vec![Loop {
        start: 500,
        end: 600,
        crossfade: 0,
        ..Loop::default()
    }];
    let mut voice = Voice::allocate();
    voice.start(start(region, 1.0, Ahdsr::GATE));
    let out = render(&mut voice, &client, 5000);
    assert!(voice.is_active());
    // Frame 4950 is 4450 past the loop start: 4450 % 100 = 50 → source frame 550.
    assert_eq!(out[4950], 550.0 / 65_536.0);
}

/// Kontakt's loop X-Fade: the last frames of the loop fade linearly into the
/// frames before its start, on every pass.
#[test]
fn a_loop_crossfade_blends_the_tail_into_the_audio_before_the_start() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let mut region = ramp_region(&mut store, 1000, 1000).unwrap();
    region.loops = vec![Loop {
        start: 500,
        end: 600,
        crossfade: 40,
        ..Loop::default()
    }];
    let mut voice = Voice::allocate();
    voice.start(start(region, 1.0, Ahdsr::GATE));
    let out = render(&mut voice, &client, 800);
    let frame = |n: f32| n / 65_536.0;
    assert_eq!(out[559], frame(559.0), "before the fade");
    // Halfway through the fade: half of 580, half of 580 - 100.
    assert_eq!(out[580], frame(530.0));
    assert_eq!(out[680], frame(530.0), "every pass");
    assert_eq!(
        out[600],
        frame(500.0),
        "the wrap continues where the fade ended"
    );
}

#[test]
fn reverse_plays_from_the_end() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let mut region = ramp_region(&mut store, 1000, 1000).unwrap();
    region.reverse = true;
    let mut voice = Voice::allocate();
    voice.start(start(region, 1.0, Ahdsr::GATE));
    let out = render(&mut voice, &client, 10);
    assert_eq!(out[0], 999.0 / 65_536.0);
    assert_eq!(out[9], 990.0 / 65_536.0);
}

#[test]
fn release_ends_the_voice() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let mut region = ramp_region(&mut store, 1000, 1000).unwrap();
    region.loops = vec![Loop {
        start: 0,
        end: 1000,
        crossfade: 0,
        ..Loop::default()
    }];
    let mut voice = Voice::allocate();
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

/// A 1000-frame ramp region with `loops`, rendered `frames` long.
fn looped_output(store: &mut Store, loops: Vec<Loop>, frames: usize) -> std::io::Result<Vec<f32>> {
    let client = store.client();
    let mut region = ramp_region(store, 1000, 1000)?;
    region.loops = loops;
    let mut voice = Voice::allocate();
    voice.start(start(region, 1.0, Ahdsr::GATE));
    Ok(render(&mut voice, &client, frames))
}

/// Source frame `n` as the ramp reads it.
fn at(n: u32) -> f32 {
    n as f32 / 65_536.0
}

#[test]
fn loops_play_in_sequence() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let out = looped_output(
        &mut store,
        vec![
            Loop {
                start: 100,
                end: 200,
                count: 1,
                ..Loop::default()
            },
            Loop {
                start: 300,
                end: 400,
                count: 1,
                ..Loop::default()
            },
        ],
        700,
    )
    .unwrap();
    assert_eq!(out[150], at(150), "first pass to the first loop's end");
    // One repeat of 100..200, then on to the second loop's end at 400.
    assert_eq!(out[200], at(100), "repeat of the first loop");
    assert_eq!(out[299], at(199));
    assert_eq!(
        out[300],
        at(200),
        "after the first loop, forward to the second"
    );
    assert_eq!(
        out[500],
        at(300),
        "second loop's repeat starts at its arrival"
    );
    assert_eq!(out[600], at(400), "then forward to the region's end");
}

#[test]
fn a_counted_loop_repeats_that_many_times() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let out = looped_output(
        &mut store,
        vec![Loop {
            start: 100,
            end: 200,
            count: 2,
            ..Loop::default()
        }],
        500,
    )
    .unwrap();
    assert_eq!(out[199], at(199));
    assert_eq!(out[200], at(100), "first repeat");
    assert_eq!(out[300], at(100), "second repeat");
    assert_eq!(out[400], at(200), "then plays on past the loop");
}

#[test]
fn an_alternating_loop_plays_backward_on_each_turn() {
    // Body 100..110: a turn runs 108 down to 101, then 100 up to 109, and the
    // loop's end frame 109 is not repeated (docs/ALTERNATING_LOOPS.md).
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let out = looped_output(
        &mut store,
        vec![Loop {
            start: 100,
            end: 110,
            count: 1,
            alternating: true,
            ..Loop::default()
        }],
        130,
    )
    .unwrap();
    assert_eq!(out[109], at(109), "the forward arrival");
    assert_eq!(out[110], at(108), "backward after the arrival");
    assert_eq!(out[117], at(101));
    assert_eq!(out[118], at(100), "forward again from the start");
    assert_eq!(out[127], at(109));
    assert_eq!(
        out[128],
        at(110),
        "forward past the loop when the turn count ends"
    );
}

#[test]
fn an_until_release_loop_repeats_until_release_then_plays_on() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let mut region = ramp_region(&mut store, 1000, 1000).unwrap();
    region.loops = vec![Loop {
        start: 100,
        end: 200,
        until_release: true,
        ..Loop::default()
    }];
    let mut voice = Voice::allocate();
    voice.start(start(
        region,
        1.0,
        Ahdsr {
            // Long enough that the release fade is far under the tolerance below.
            release: 1_000_000,
            ..Ahdsr::GATE
        },
    ));
    let mut out = vec![[0.0; 2]; 1400];
    voice.render(&client, 0, &mut out[..500]);
    voice.release();
    voice.render(&client, 500, &mut out[500..]);
    let out: Vec<f32> = out.iter().map(|f| f[0]).collect();
    // Values are checked to 0.2%: the release fade scales them slightly.
    let near = |got: f32, want: f32| assert!((got - want).abs() <= want * 0.002, "{got} vs {want}");
    near(out[450], at(150));
    // The repeat in progress at 500 is the last one: it ends at 500, then 200 on.
    near(out[499], at(199));
    near(out[500], at(200));
    near(out[600], at(300));
}

/// Two cycles, a sine then its negation: a table voice plays the sine's
/// frequency for any sample length, and a position halfway cancels them.
#[test]
fn a_table_plays_its_cycle_at_the_step_and_morphs_by_position() {
    use kontra_voice::{CYCLE, LEVELS, PERIOD, Read, Table, Warp};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let frames = 2 * CYCLE;
    let bytes = (0..frames)
        .flat_map(|n| {
            let sine = (std::f32::consts::TAU * (n % CYCLE) as f32 / CYCLE as f32).sin();
            if n < CYCLE { sine } else { -sine }.to_le_bytes()
        })
        .collect();
    let key = BlobKey {
        file: "table".into(),
        offset: 0,
        fmt: FMT,
    };
    let sample = store
        .load(
            &key,
            Arc::new(MemSource::new(FMT, bytes)),
            frames,
            std::slice::from_ref(&(0..frames)),
        )
        .unwrap();
    // Each cycle's every level is the one cycle.
    let data: Arc<[i16]> = (0..frames)
        .map(|n| {
            let sine = (std::f32::consts::TAU * (n % CYCLE) as f32 / CYCLE as f32).sin();
            (if n < CYCLE { sine } else { -sine } * 32767.0) as i16
        })
        .collect();
    let spans: Arc<[(u32, u32)]> = (0..2 * LEVELS)
        .map(|i| (if i < LEVELS { 0 } else { CYCLE as u32 }, CYCLE as u32))
        .collect();
    let play = |position| {
        let read = Read {
            position,
            phase: 0.0,
            warps: [Warp::OFF; 2],
        };
        let region = Region {
            mix: None,
            table: Table::new((Arc::clone(&data), 1.0 / 32767.0), Arc::clone(&spans), read),
            sample: Arc::clone(&sample),
            start: 0,
            end: frames,
            loops: vec![],
            reverse: false,
        };
        let mut voice = Voice::allocate();
        // 480 Hz at 48 kHz: a cycle each 100 frames.
        voice.start(start(region, PERIOD / 100.0, Ahdsr::GATE));
        render(&mut voice, &client, 10_000)
    };
    let first = play(0.0);
    for (n, v) in first.iter().enumerate() {
        let want = (std::f32::consts::TAU * n as f32 / 100.0).sin();
        assert!((v - want).abs() < 1e-3, "frame {n}: {v} vs {want}");
    }
    assert!(play(0.5).iter().all(|v| v.abs() < 1e-6));
    assert!(
        (play(1.0)[25] + 1.0).abs() < 1e-3,
        "the second cycle is the negated sine"
    );
}

#[cfg(feature = "trace")]
#[test]
fn trace_peak_is_isolated_from_existing_mix_and_resets_each_render() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let mut voice = Voice::allocate();
    voice.start(start(
        ramp_region(&mut store, 4096, 4096).unwrap(),
        1.0,
        Ahdsr::GATE,
    ));
    let mut block = [[1.0; 2]; 128];
    voice.render(&client, 0, &mut block);
    let expected = 127.0 / 65_536.0;
    assert!((voice.last_peak() - expected).abs() < 1e-7);
    let mut next = [[0.0; 2]; 128];
    voice.render(&client, 128, &mut next);
    assert!((voice.last_peak() - 255.0 / 65_536.0).abs() < 1e-7);
}

#[cfg(feature = "trace")]
#[test]
fn offline_render_waits_for_the_interpolation_page_at_the_preload_boundary() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let mut region = ramp_region(&mut store, 4096, 1024).unwrap();
    region.start = 896;
    let mut voice = Voice::allocate();
    voice.set_offline_wait(true);
    voice.start(start(region, 1.0, Ahdsr::GATE));
    let mut out = [[0.0; 2]; 128];
    voice.render(&client, 0, &mut out);
    assert_eq!(voice.offline_wait_timeouts(), 0);
    assert_eq!(voice.missing_frames(), 0);
    for (n, frame) in out.iter().enumerate() {
        assert_eq!(frame[0], (896 + n) as f32 / 65_536.0, "frame {n}");
    }
}

#[cfg(feature = "trace")]
#[test]
fn offline_wait_reports_a_failed_page_as_missing() {
    struct FailingSource;
    impl kontra_stream::Source for FailingSource {
        fn read(&self, _first: u64, _out: &mut [u8]) -> std::io::Result<()> {
            Err(std::io::ErrorKind::UnexpectedEof.into())
        }
    }
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let key = BlobKey {
        file: "failing-source".into(),
        offset: 0,
        fmt: FMT,
    };
    let sample = store
        .load(&key, Arc::new(FailingSource), 4096, &[])
        .unwrap();
    let region = Region {
        mix: None,
        table: None,
        sample,
        start: 0,
        end: 4096,
        loops: vec![],
        reverse: false,
    };
    let mut voice = Voice::allocate();
    voice.set_offline_wait(true);
    voice.start(start(region, 1.0, Ahdsr::GATE));
    let mut out = [[0.0; 2]; 128];
    voice.render(&client, 0, &mut out);
    assert!(voice.missing_frames() > 0);
    assert!(client.stats().read_errors > 0);
}

/// Key-up, then a steal: the voice ends, counted as ending once, and keeps
/// the frame it began releasing (its modulators' age).
#[test]
fn a_released_voice_that_is_stolen_keeps_its_release_time() {
    let mut voice = Voice::allocate();
    voice.key_up(10, false);
    assert!(voice.is_released());
    assert!(voice.fade(4));
    assert!(!voice.fade(2), "a second fade is not a second end");
    assert!(voice.is_ending() && !voice.is_released());
    assert_eq!(voice.released_at(), Some(10));
}
