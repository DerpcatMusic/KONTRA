use std::sync::Arc;

use kontra_stream::{
    BlobKey, Encoding, MemSource, PAGE_FRAMES, SampleFmt, Store, StoreConfig, StreamClient,
};
use kontra_voice::{Ahdsr, FadeTarget, KeyUp, Loop, Region, Scratch, Voice, VoiceStart};

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
        loops: Default::default(),
        reverse: false,
    })
}

fn start(region: Region, step: f64, envelope: Ahdsr) -> VoiceStart {
    VoiceStart {
        speed: 1.0,
        region,
        loop_shift: [0.0; 2],
        mix: None,
        read: None,
        oscillator: None,
        granular: None,
        step,
        gain: [[1.0, 0.0], [0.0, 1.0]],
        lag: [kontra_voice::Lag::NONE; kontra_voice::LAGS],
        envelope,
        fade: None,
    }
}

fn render(voice: &mut Voice, client: &StreamClient, frames: usize) -> Vec<f32> {
    let mut out = vec![[0.0; 2]; frames];
    for (i, block) in out.chunks_mut(128).enumerate() {
        voice.render(&mut Scratch::allocate(), client, (i * 128) as u64, block);
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
    voice.render(&mut Scratch::allocate(), &client, 0, &mut out);
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
    region.loops = Region::pack_loops([Loop {
        start: 500,
        end: 600,
        crossfade: 0,
        ..Loop::default()
    }]);
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
    region.loops = Region::pack_loops([Loop {
        start: 500,
        end: 600,
        crossfade: 40,
        ..Loop::default()
    }]);
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
    region.loops = Region::pack_loops([Loop {
        start: 0,
        end: 1000,
        crossfade: 0,
        ..Loop::default()
    }]);
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
    region.loops = Region::pack_loops(loops);
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
                count: 2,
                ..Loop::default()
            },
            Loop {
                start: 300,
                end: 400,
                count: 2,
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
            count: 3,
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
            count: 2,
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
    region.loops = Region::pack_loops([Loop {
        start: 100,
        end: 200,
        until_release: true,
        ..Loop::default()
    }]);
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
    voice.render(&mut Scratch::allocate(), &client, 0, &mut out[..500]);
    voice.release();
    voice.render(&mut Scratch::allocate(), &client, 500, &mut out[500..]);
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
            phase_random: 0.0,
            gain: 1.0,
            inharmonic: kontra_voice::Inharmonic::OFF,
            modosc: kontra_voice::ModOsc::OFF,
            rate: 48_000.0,
            // MEDIUM reads the table as it is: HIGH filters and delays it.
            quality: kontra_voice::Quality::Medium,
            warps: [Warp::OFF; 2],
        };
        let region = Region {
            mix: None,
            table: Table::new((Arc::clone(&data), 1.0 / 32767.0), Arc::clone(&spans), read)
                .map(Arc::new),
            sample: Arc::clone(&sample),
            start: 0,
            end: frames,
            loops: Default::default(),
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
    voice.render(&mut Scratch::allocate(), &client, 0, &mut block);
    let expected = 127.0 / 65_536.0;
    assert!((voice.last_peak() - expected).abs() < 1e-7);
    let mut next = [[0.0; 2]; 128];
    voice.render(&mut Scratch::allocate(), &client, 128, &mut next);
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
    voice.set_render_mode(kontra_voice::RenderMode::Offline);
    voice.start(start(region, 1.0, Ahdsr::GATE));
    let mut out = [[0.0; 2]; 128];
    voice.render(&mut Scratch::allocate(), &client, 0, &mut out);
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
        loops: Default::default(),
        reverse: false,
    };
    let mut voice = Voice::allocate();
    voice.set_render_mode(kontra_voice::RenderMode::Offline);
    voice.start(start(region, 1.0, Ahdsr::GATE));
    let mut out = [[0.0; 2]; 128];
    voice.render(&mut Scratch::allocate(), &client, 0, &mut out);
    assert!(voice.missing_frames() > 0);
    assert!(client.stats().read_errors > 0);
}

/// Key-up, then a steal: the voice ends, counted as ending once, and keeps
/// the frame it began releasing (its modulators' age).
#[test]
fn a_released_voice_that_is_stolen_keeps_its_release_time() {
    let mut voice = Voice::allocate();
    voice.key_up(10, KeyUp::Release);
    assert!(voice.is_released());
    assert!(voice.fade(FadeTarget::End, 4));
    assert!(
        !voice.fade(FadeTarget::End, 2),
        "a second fade is not a second end"
    );
    assert!(voice.is_ending() && !voice.is_released());
    assert_eq!(voice.released_at(), Some(10));
}

/// A stretching voice plays the sample at its pitch (a ramp keeps its slope
/// from the first frame) while its length follows the speed.
#[test]
fn a_stretching_voice_keeps_its_pitch_and_takes_its_speeds_time() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    for (speed, ends) in [(2.0, 4096), (0.5, 16_384)] {
        let mut voice = Voice::allocate();
        let mut s = start(
            ramp_region(&mut store, 8192, 8192).unwrap(),
            1.0,
            Ahdsr::GATE,
        );
        s.speed = speed;
        voice.start(s);
        let out = render(&mut voice, &client, ends - 512);
        assert_eq!(
            out[1],
            1.0 / 65_536.0,
            "speed {speed}: the start plays as recorded"
        );
        assert!(
            voice.is_active(),
            "speed {speed}: still playing before its end"
        );
        render(&mut voice, &client, 1024);
        assert!(!voice.is_active(), "speed {speed}: ended on time");
    }
}

#[test]
fn starting_inside_a_loop_plays_to_loop_end_and_wraps() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let mut region = ramp_region(&mut store, 1000, 1000).unwrap();
    region.start = 550;
    region.loops = kontra_voice::Region::pack_loops(vec![Loop {
        start: 500,
        end: 600,
        ..Loop::default()
    }]);
    let mut voice = Voice::allocate();
    voice.start(start(region, 1.0, Ahdsr::GATE));
    let out = render(&mut voice, &client, 200);
    assert_eq!(out[0], at(550));
    assert_eq!(out[49], at(599));
    assert_eq!(out[50], at(500), "wrapped to loop start");
    assert_eq!(out[149], at(599));
    assert_eq!(out[150], at(500), "second wrap");
}

#[test]
fn finite_loop_count_is_total_passes() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    // count = 1: one pass through 100..200 (0 wraps), continues past 200.
    let one = looped_output(
        &mut store,
        vec![Loop {
            start: 100,
            end: 200,
            count: 1,
            ..Loop::default()
        }],
        300,
    )
    .unwrap();
    assert_eq!(one[199], at(199));
    assert_eq!(one[200], at(200), "count=1 exits without wrapping");

    // count = 2: two passes through 100..200 (1 wrap), then continues past 200.
    let two = looped_output(
        &mut store,
        vec![Loop {
            start: 100,
            end: 200,
            count: 2,
            ..Loop::default()
        }],
        400,
    )
    .unwrap();
    assert_eq!(two[200], at(100), "count=2 wraps once");
    assert_eq!(two[300], at(200), "count=2 exits after second pass");
}

#[test]
fn reverse_loops_crossfade_and_start_inside_loop() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();

    // Reverse forward loop 500..600 in a 1000-frame sample: plays 999..500,
    // then wraps back to 599..500.
    let mut region = ramp_region(&mut store, 1000, 1000).unwrap();
    region.reverse = true;
    region.loops = kontra_voice::Region::pack_loops(vec![Loop {
        start: 500,
        end: 600,
        ..Loop::default()
    }]);
    let mut voice = Voice::allocate();
    voice.start(start(region, 1.0, Ahdsr::GATE));
    let out = render(&mut voice, &client, 700);
    assert_eq!(out[0], at(999));
    assert_eq!(out[499], at(500));
    assert_eq!(out[500], at(599), "reverse loop wrapped to physical 599");
    assert_eq!(out[599], at(500));
    assert_eq!(out[600], at(599));

    // Reverse alternating loop: 999..500, then bounces 501..598, then 599..500.
    let mut region = ramp_region(&mut store, 1000, 1000).unwrap();
    region.reverse = true;
    region.loops = kontra_voice::Region::pack_loops(vec![Loop {
        start: 500,
        end: 600,
        alternating: true,
        ..Loop::default()
    }]);
    let mut voice = Voice::allocate();
    voice.start(start(region, 1.0, Ahdsr::GATE));
    let out = render(&mut voice, &client, 700);
    assert_eq!(out[499], at(500));
    assert_eq!(out[500], at(501), "reverse alternating bounces up");
    assert_eq!(out[597], at(598));
    assert_eq!(out[598], at(599));
    assert_eq!(out[599], at(598));

    // Reverse loop with crossfade 40: near physical 500 (logical 480 = physical 519),
    // blends half of 519 with half of 619 (= 569).
    let mut region = ramp_region(&mut store, 1000, 1000).unwrap();
    region.reverse = true;
    region.loops = kontra_voice::Region::pack_loops(vec![Loop {
        start: 500,
        end: 600,
        crossfade: 40,
        ..Loop::default()
    }]);
    let mut voice = Voice::allocate();
    voice.start(start(region, 1.0, Ahdsr::GATE));
    let out = render(&mut voice, &client, 620);
    assert_eq!(out[459], at(540), "before reverse crossfade");
    assert!(
        (out[480] - at(569)).abs() < 1e-6,
        "halfway fade: {}",
        out[480] * 65_536.0
    );
    assert_eq!(out[500], at(599), "wrapped seamlessly");

    // Reverse start inside loop (region.end = 550): plays 549..500, wraps to 599.
    let mut region = ramp_region(&mut store, 1000, 1000).unwrap();
    region.reverse = true;
    region.end = 550;
    region.loops = kontra_voice::Region::pack_loops(vec![Loop {
        start: 500,
        end: 600,
        ..Loop::default()
    }]);
    let mut voice = Voice::allocate();
    voice.start(start(region, 1.0, Ahdsr::GATE));
    let out = render(&mut voice, &client, 160);
    assert_eq!(out[0], at(549));
    assert_eq!(out[49], at(500));
    assert_eq!(out[50], at(599));
}

#[test]
fn tuned_loop_applies_ratio_on_repeats_and_is_block_size_invariant() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let make = |store: &mut Store| {
        let mut region = ramp_region(store, 1000, 1000).unwrap();
        region.loops = kontra_voice::Region::pack_loops(vec![Loop {
            start: 500,
            end: 600,
            tune_ratio: 2.0,
            ..Loop::default()
        }]);
        region
    };
    let run_block = |region: Region, block: usize, ramp: bool| -> Vec<f32> {
        let mut voice = Voice::allocate();
        voice.start(start(region, 1.0, Ahdsr::GATE));
        if ramp {
            voice.set_step_ramp(1.0, 2.0, 600);
        }
        let mut out = vec![[0.0; 2]; 768];
        let mut scratch = Scratch::allocate();
        let mut done = 0;
        while done < out.len() {
            let take = block.min(out.len() - done);
            voice.render(
                &mut scratch,
                &client,
                done as u64,
                &mut out[done..done + take],
            );
            done += take;
        }
        out.iter().map(|f| f[0]).collect()
    };
    let region = make(&mut store);
    let reference = run_block(region.clone(), 128, false);
    // Initial pass (0..600) steps at 1.0; repeats (600+) step at 2.0.
    // Past the anti-aliased loop-wrap discontinuity, the band-limited sinc
    // tracks the linear ramp at exactly 2 source frames per output frame.
    assert_eq!(reference[100], at(100));
    assert_eq!(reference[599], at(599));
    assert!(
        (reference[605] - at(510)).abs() < 1e-5,
        "{}",
        reference[605]
    );
    assert!(
        (reference[606] - at(512)).abs() < 1e-5,
        "{}",
        reference[606]
    );
    assert!(
        (reference[607] - at(514)).abs() < 1e-5,
        "{}",
        reference[607]
    );
    for block in [1, 32, 97] {
        let got = run_block(region.clone(), block, false);
        for (i, (a, b)) in reference.iter().zip(&got).enumerate() {
            assert!((a - b).abs() < 1e-5, "block {block} frame {i}: {a} vs {b}");
        }
    }
    // A one-frame render checks loop tuning at each cursor, independently
    // of the chunk bound. Here the pitch glide crosses the first wrap.
    let per_frame = run_block(region.clone(), 1, true);
    for block in [32, 97, 128, 512] {
        let got = run_block(region.clone(), block, true);
        assert_eq!(got, per_frame, "pitch glide differs at block {block}");
    }
}

#[test]
fn forced_release_ends_a_voice_whose_one_shot_ignores_key_up() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let mut voice = Voice::allocate();
    let mut region = ramp_region(&mut store, 4096, 4096).unwrap();
    region.start = 512;
    voice.start(start(
        region,
        1.0,
        Ahdsr {
            hold: 1024,
            decay: 512,
            release: 64,
            one_shot: true,
            ..Ahdsr::GATE
        },
    ));
    voice.key_up(0, kontra_voice::KeyUp::Release);
    assert_eq!(voice.envelope_stage(), "hold");
    assert!(voice.end_by_release(0));
    assert_eq!(voice.envelope_stage(), "release");
    let out = render(&mut voice, &client, 64);
    assert!(out[0] > 0.0);
    assert_eq!(out[63], 0.0);
    assert!(!voice.is_active());
}

#[test]
fn until_release_released_mid_crossfade_finishes_wrap_without_click() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let mut region = ramp_region(&mut store, 1000, 1000).unwrap();
    region.loops = kontra_voice::Region::pack_loops(vec![Loop {
        start: 500,
        end: 600,
        crossfade: 40,
        until_release: true,
        ..Loop::default()
    }]);
    let mut voice = Voice::allocate();
    voice.start(start(
        region,
        1.0,
        Ahdsr {
            release: 1000,
            ..Ahdsr::GATE
        },
    ));
    let mut out = vec![[0.0; 2]; 800];
    let mut scratch = Scratch::allocate();
    // Render up to frame 580 (halfway into crossfade 560..600), then release.
    voice.render(&mut scratch, &client, 0, &mut out[..580]);
    voice.release();
    voice.render(&mut scratch, &client, 580, &mut out[580..]);
    // Frame 580 must continue the active wrap crossfade (NOT jump back to unblended 580).
    let raw_580 = out[580][0] / (1.0 - 1.0 / 1000.0);
    assert!(
        (raw_580 - at(530)).abs() < 2e-4,
        "frame 580 should be blended (~530), got {}",
        raw_580 * 65_536.0
    );
    // Wraps to 500 at frame 600, then exits cleanly at frame 700 into 600..1000.
    let raw_600 = out[600][0] / (1.0 - 21.0 / 1000.0);
    assert!(
        (raw_600 - at(500)).abs() < 2e-4,
        "frame 600 should wrap to 500, got {}",
        raw_600 * 65_536.0
    );
    let raw_700 = out[700][0] / (1.0 - 121.0 / 1000.0);
    assert!(
        (raw_700 - at(600)).abs() < 2e-4,
        "frame 700 should exit past loop to 600, got {}",
        raw_700 * 65_536.0
    );
}

#[test]
fn silent_tail_culls_releasing_voice_but_not_held_voice() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let frames = 48_000u64;
    let bytes = vec![0u8; frames as usize * 4];
    let key = BlobKey {
        file: "silence".into(),
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
    let region = Region {
        mix: None,
        table: None,
        sample,
        start: 0,
        end: frames,
        loops: Default::default(),
        reverse: false,
    };
    let mut voice = Voice::allocate();
    voice.start(start(
        region,
        1.0,
        Ahdsr {
            release: 48_000,
            ..Ahdsr::GATE
        },
    ));
    // Held voice stays alive even through >512 silent frames.
    render(&mut voice, &client, 1024);
    assert!(voice.is_active(), "held silent voice must not be culled");
    // Once released, 512+ consecutive silent frames cull the voice early.
    voice.release();
    render(&mut voice, &client, 640);
    assert!(
        !voice.is_active(),
        "releasing silent tail should cull early"
    );
}

#[test]
fn upward_pitch_shift_suppresses_aliasing_above_nyquist() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let frames = 32_768u64;
    let rate = 48_000.0f64;

    let render_sine = |store: &mut Store, tag: &str, freq_hz: f64, step: f64| -> Vec<f32> {
        let bytes: Vec<u8> = (0..frames)
            .flat_map(|n| {
                let s = (std::f64::consts::TAU * freq_hz * n as f64 / rate).sin() as f32;
                s.to_le_bytes()
            })
            .collect();
        let key = BlobKey {
            file: tag.into(),
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
        let region = Region {
            mix: None,
            table: None,
            sample,
            start: 0,
            end: frames,
            loops: Default::default(),
            reverse: false,
        };
        let mut voice = Voice::allocate();
        voice.start(start(region, step, Ahdsr::GATE));
        render(&mut voice, &client, 2048)
    };

    // Steady-state RMS over interior frames (avoiding startup/ending edge effects).
    let rms = |buf: &[f32]| -> f32 {
        let slice = &buf[64..1984];
        (slice.iter().map(|x| x * x).sum::<f32>() / slice.len() as f32).sqrt()
    };

    // 18 kHz shifted up +1 octave (step = 2.0) exceeds Nyquist (36 kHz > 24 kHz)
    // and would fold to 12 kHz at ~0 dB under unfiltered Hermite.
    let aliased = render_sine(&mut store, "sine_18k", 18_000.0, 2.0);
    let alias_db = 20.0 * rms(&aliased).max(1e-10).log10() - 20.0 * (0.5f32.sqrt()).log10();
    assert!(
        alias_db < -50.0,
        "18 kHz tone shifted +1 oct should be attenuated >50 dB, got {alias_db:.1} dB"
    );

    // At step 16 the tap radius is capped, but a 2 kHz input still
    // becomes an out-of-band 32 kHz tone rather than a passband tone.
    let capped = render_sine(&mut store, "sine_2k_capped", 2_000.0, 16.0);
    assert!(rms(&capped) < 0.08, "capped filter leaked the 2 kHz tone");

    // 2 kHz shifted up +1 octave -> 4 kHz stays in passband within 0.2 dB.
    let passband = render_sine(&mut store, "sine_2k", 2_000.0, 2.0);
    let pass_db = 20.0 * rms(&passband).max(1e-10).log10() - 20.0 * (0.5f32.sqrt()).log10();
    assert!(
        pass_db.abs() < 0.2,
        "2 kHz tone shifted +1 oct should stay flat in passband, got {pass_db:.3} dB"
    );
}

#[test]
fn gain_smoothing_is_sample_rate_invariant() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let frames = 4096u64;
    let bytes: Vec<u8> = (0..frames).flat_map(|_| 1.0f32.to_le_bytes()).collect();
    let key = BlobKey {
        file: "dc_smooth".into(),
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
    let region = Region {
        mix: None,
        table: None,
        sample,
        start: 0,
        end: frames,
        loops: Default::default(),
        reverse: false,
    };

    let gain_after_1ms = |rate: u32| -> f32 {
        let mut voice = Voice::allocate();
        voice.set_sample_rate(rate);
        let mut vs = start(region.clone(), 1.0, Ahdsr::GATE);
        vs.gain = [[0.0; 2]; 2];
        voice.start(vs);
        voice.set_gain([[1.0, 0.0], [0.0, 1.0]]);
        let n = (rate / 1000) as usize;
        let out = render(&mut voice, &client, n);
        out[n - 1]
    };

    let at_48k = gain_after_1ms(48_000);
    let at_96k = gain_after_1ms(96_000);
    let at_192k = gain_after_1ms(192_000);
    // 1 - e^-1 ≈ 0.6321 across all sample rates.
    let target = 1.0 - (-1.0f32).exp();
    for (rate, got) in [(48_000, at_48k), (96_000, at_96k), (192_000, at_192k)] {
        assert!(
            (got - target).abs() < 0.02,
            "rate {rate}: gain at 1 ms = {got:.4} vs {target:.4}"
        );
    }
    assert!(
        (at_48k - at_192k).abs() < 0.01,
        "48k ({at_48k:.4}) and 192k ({at_192k:.4}) diverged"
    );
}

#[test]
fn live_voice_never_stalls_when_heads_are_pending() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    store.defer_heads(true);
    let frames = 4096u64;
    let bytes: Vec<u8> = (0..frames)
        .flat_map(|n| (n as f32 / 65_536.0).to_le_bytes())
        .collect();
    let key = BlobKey {
        file: "deferred_head".into(),
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
    assert!(sample.heads_pending(), "head load should be deferred");
    let client = store.client();
    let region = Region {
        mix: None,
        table: None,
        sample,
        start: 0,
        end: frames,
        loops: Default::default(),
        reverse: false,
    };
    let mut voice = Voice::allocate();
    voice.start(start(region, 1.0, Ahdsr::GATE));
    let mut out = [[0.0; 2]; 128];
    voice.render(&mut Scratch::allocate(), &client, 0, &mut out);
    // Cursor MUST advance immediately by 128 frames instead of stalling at 0!
    assert_eq!(voice.position(), 128.0);
    // A background prefetch can arrive during render, so misses are not asserted.
}
