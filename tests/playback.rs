use kontakto::{
    audio::Sample,
    engine::{
        Ahdsr, Bank, Engine, EventChange, MAX_BLOCK, MAX_VOICES, NoteEvent, PRELOAD_FRAMES, Rack,
        load_scripts,
    },
    fx,
    import::{
        Group, Instrument, Loop, ModAssignment, ModSource, ModTarget, Modulator, Resolver,
        VoiceLimit, Zone,
    },
    ksp::{Runtime, Value, settle_persistence},
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::path::{Path, PathBuf};

type Frame = [f32; 2];

fn render(e: &mut Engine, frames: usize) -> Vec<Frame> {
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    e.render(&mut left, &mut right);
    left.into_iter().zip(right).map(|(l, r)| [l, r]).collect()
}

fn last(e: &mut Engine, frames: usize) -> Frame {
    *render(e, frames).last().unwrap()
}

fn close(a: Frame, b: Frame) -> bool {
    (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5
}

fn engine_with(bank: Bank) -> Engine {
    let mut e = Engine::default();
    e.attack = 0.0001;
    e.release = 0.001;
    e.set_bank(Some(Box::new(bank)));
    e
}

fn constant(value: Frame, frames: usize) -> Sample {
    Sample {
        rate: 48000,
        frames: vec![value; frames],
    }
}

/// One group, one looping zone on keys 60–72 over a constant sample.
fn engine() -> Engine {
    let zone = Zone {
        low_key: 60,
        high_key: 72,
        low_velocity: 10,
        loop_range: Some(Loop {
            start: 0,
            end: 100,
            until_release: false,
            crossfade: 0,
        }),
        ..Zone::default()
    };
    engine_with(
        Bank::from_samples(
            vec![Group::default()],
            vec![zone],
            vec![(PathBuf::new(), constant([0.5, 0.25], 100))],
        )
        .unwrap(),
    )
}

/// Groups each playing their own constant sample on every key.
fn layered(groups: Vec<Group>, values: &[f32]) -> Bank {
    let zones = (0..groups.len())
        .map(|g| Zone {
            group: g,
            sample: PathBuf::from(g.to_string()),
            loop_range: Some(Loop {
                start: 0,
                end: 100,
                until_release: false,
                crossfade: 0,
            }),
            ..Zone::default()
        })
        .collect();
    let samples = values
        .iter()
        .enumerate()
        .map(|(g, &v)| (PathBuf::from(g.to_string()), constant([v, v], 100)))
        .collect();
    Bank::from_samples(groups, zones, samples).unwrap()
}

#[test]
fn mapped_polyphony_sustain_channels_and_release() {
    let mut e = engine();
    e.note_on(0, 59, 100);
    e.note_on(0, 60, 9);
    assert_eq!(last(&mut e, 1), [0.0, 0.0]);
    e.note_on(0, 60, 127);
    assert_eq!(last(&mut e, 21), [0.5, 0.25]);
    e.note_on(1, 60, 127);
    assert_eq!(last(&mut e, 21), [1.0, 0.5]);
    e.cc(0, 64, 127);
    e.note_off(0, 60);
    e.note_off(1, 60);
    assert_eq!(
        last(&mut e, 1000),
        [0.5, 0.25],
        "channel 0 sustains, channel 1 releases"
    );
    e.cc(0, 64, 0);
    assert_eq!(last(&mut e, 1000), [0.0, 0.0]);
    assert_eq!(e.active_voices(), 0);
    for _ in 0..2000 {
        e.note_on(0, 60, 127);
    }
    assert_eq!(
        e.active_voices(),
        MAX_VOICES,
        "stolen voices fade out instead of being cut"
    );
    render(&mut e, 480);
    assert_eq!(e.active_voices(), 512, "default polyphony");
    e.cc(0, 120, 0);
    assert_eq!(e.active_voices(), 0);
}

#[test]
fn root_pitch_octave_reverse_and_end_bounds() {
    let ramp = || Sample {
        rate: 48000,
        frames: (0..100).map(|i| [i as f32 / 100.0; 2]).collect(),
    };
    let mut a = engine_with(
        Bank::from_samples(
            vec![Group::default()],
            vec![Zone::default()],
            vec![(PathBuf::new(), ramp())],
        )
        .unwrap(),
    );
    a.note_on(0, 72, 127);
    assert!(
        (last(&mut a, 11)[0] - 0.2).abs() < 1e-6,
        "an octave up steps two frames"
    );
    render(&mut a, 100);
    assert_eq!(a.active_voices(), 0, "voice ends at the sample end");
    let reverse = Group {
        reverse: true,
        ..Group::default()
    };
    let mut b = engine_with(
        Bank::from_samples(
            vec![reverse],
            vec![Zone::default()],
            vec![(PathBuf::new(), ramp())],
        )
        .unwrap(),
    );
    b.note_on(0, 60, 127);
    assert!((last(&mut b, 11)[0] - 0.89).abs() < 1e-6);
}

#[test]
fn every_group_layers_with_mute_solo_and_allow_mask() {
    let groups = vec![
        Group::default(),
        Group {
            gain: 0.5,
            ..Group::default()
        },
        Group {
            release_trigger: true,
            ..Group::default()
        },
        Group {
            muted: true,
            ..Group::default()
        },
    ];
    let mut e = engine_with(layered(groups.clone(), &[0.1, 0.2, 0.4, 0.8]));
    e.note_on(0, 60, 127);
    assert!(
        close(last(&mut e, 20), [0.2; 2]),
        "groups 0 and 1 layer; release and muted groups do not"
    );
    e.set_group_allowed(0, false);
    e.note_on(1, 60, 127);
    assert!(
        close(last(&mut e, 20), [0.3; 2]),
        "the allow mask filters new notes only"
    );

    let mut solo = groups;
    solo[1].soloed = true;
    let mut e = engine_with(layered(solo, &[0.1, 0.2, 0.4, 0.8]));
    e.note_on(0, 60, 127);
    assert!(close(last(&mut e, 20), [0.1; 2]), "only soloed groups play");
}

#[test]
fn release_triggers_fire_on_release_including_after_the_pedal() {
    let velocity_volume = ModAssignment {
        name: "VEL_VOLUME".into(),
        source: ModSource::Velocity,
        target: ModTarget::Volume,
        intensity: 1.0,
        invert: false,
        lag_ms: 0,
        shaper: None,
    };
    let groups = vec![
        Group {
            mods: vec![velocity_volume.clone()],
            ..Group::default()
        },
        Group {
            release_trigger: true,
            mods: vec![velocity_volume],
            ..Group::default()
        },
    ];
    let mut e = engine_with(layered(groups, &[0.1, 0.4]));
    let release = 0.4 * 64.0 / 127.0;
    e.note_on(0, 60, 64);
    e.note_off(0, 60);
    assert!(
        close(last(&mut e, 200), [release; 2]),
        "note-off starts the release sample at the note-on velocity"
    );
    e.cc(0, 120, 0);

    e.cc(0, 64, 127);
    e.note_on(0, 60, 64);
    e.note_off(0, 60);
    assert!(
        close(last(&mut e, 200), [0.1 * 64.0 / 127.0; 2]),
        "the pedal holds the note and defers its release sample"
    );
    e.cc(0, 64, 0);
    assert!(
        close(last(&mut e, 200), [release; 2]),
        "pedal up releases the note and fires the release sample"
    );
}

fn sine(period: f32, frames: usize) -> Sample {
    Sample {
        rate: 48000,
        frames: (0..frames)
            .map(|i| [(i as f32 * std::f32::consts::TAU / period).sin(); 2])
            .collect(),
    }
}

fn max_step(out: &[Frame]) -> f32 {
    out.windows(2)
        .map(|w| (w[1][0] - w[0][0]).abs())
        .fold(0.0, f32::max)
}

#[test]
fn loop_crossfade_is_continuous_at_the_wrap() {
    let period = 97.3;
    let source = sine(period, 4000);
    let (start, end) = (1000, 3000);
    assert!(
        (source.frames[end][0] - source.frames[start][0]).abs() > 0.5,
        "the raw loop must click"
    );
    let play = |crossfade| {
        let zone = Zone {
            loop_range: Some(Loop {
                start,
                end,
                until_release: false,
                crossfade,
            }),
            ..Zone::default()
        };
        let bank = Bank::from_samples(
            vec![Group::default()],
            vec![zone],
            vec![(PathBuf::new(), sine(period, 4000))],
        )
        .unwrap();
        let mut e = engine_with(bank);
        e.note_on(0, 60, 127);
        render(&mut e, 12000)
    };
    let smooth = TAU_OVER(period);
    assert!(
        max_step(&play(0)[100..]) > 0.5,
        "without a crossfade the wrap is discontinuous"
    );
    assert!(
        max_step(&play(500)[100..]) < smooth * 1.1,
        "the crossfade blends into the frames before the loop start"
    );
}

#[allow(non_snake_case)]
fn TAU_OVER(period: f32) -> f32 {
    std::f32::consts::TAU / period
}

#[test]
fn stolen_voices_fade_instead_of_clicking() {
    let zone = Zone {
        loop_range: Some(Loop {
            start: 0,
            end: 4800,
            until_release: false,
            crossfade: 0,
        }),
        ..Zone::default()
    };
    let mut bank = Bank::from_samples(
        vec![Group::default()],
        vec![zone],
        vec![(PathBuf::new(), sine(100.0, 4800))],
    )
    .unwrap();
    bank.set_polyphony(1);
    let mut e = engine_with(bank);
    e.attack = 0.002;
    e.note_on(0, 60, 127);
    let mut out = render(&mut e, 1000);
    e.note_on(0, 67, 127);
    out.extend(render(&mut e, 1000));
    assert!(
        max_step(&out[100..]) < 0.12,
        "steal step {}",
        max_step(&out[100..])
    );
    assert_eq!(e.active_voices(), 1);
}

#[test]
fn voice_groups_limit_and_choke_their_members() {
    let dir = std::env::temp_dir().join(format!("kontakto-vg-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tone.wav");
    write_wav(&path, 1000);
    let groups = (0..3)
        .map(|voice_group| Group {
            voice_group: Some(voice_group),
            ..Group::default()
        })
        .collect();
    let loop_range = Some(Loop {
        start: 0,
        end: 1000,
        until_release: false,
        crossfade: 0,
    });
    let zones = (0..3)
        .map(|group| Zone {
            group,
            sample: path.clone(),
            loop_range: loop_range.clone(),
            ..Zone::default()
        })
        .collect();
    let mono = VoiceLimit {
        max_voices: 1,
        kill_mode: 1,
        prefer_released: true,
        fade_ms: 10,
        exclusion_group: -1,
    };
    let mut i = instrument(groups, zones);
    let hat = VoiceLimit {
        max_voices: 8,
        exclusion_group: 5,
        ..mono
    };
    i.voice_groups = vec![Some(mono), Some(hat), Some(hat)];
    let mut e = engine_with(Bank::load(&i).unwrap());
    for note in [60, 62, 64] {
        e.note_on(0, note, 127);
    }
    render(&mut e, 1000);
    assert_eq!(
        e.active_voices(),
        2,
        "group 0 keeps one voice; groups 1 and 2 choke each other"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn envelope_follows_group_ahdsr() {
    let mut bank = Bank::from_samples(
        vec![Group::default()],
        vec![Zone {
            loop_range: Some(Loop {
                start: 0,
                end: 100,
                until_release: false,
                crossfade: 0,
            }),
            ..Zone::default()
        }],
        vec![(PathBuf::new(), constant([1.0; 2], 100))],
    )
    .unwrap();
    bank.settings[0].envelope = Some(Ahdsr {
        attack: 0.01,
        curve: 0.0,
        hold: 0.0,
        decay: 0.05,
        sustain: 0.5,
        release: 0.05,
    });
    let mut e = engine_with(bank);
    e.note_on(0, 60, 127);
    let out = render(&mut e, 4800 + 240);
    assert!(
        (out[239][0] - 0.5).abs() < 0.01,
        "linear attack halfway: {}",
        out[239][0]
    );
    assert!((out[479][0] - 1.0).abs() < 0.01);
    assert!(
        (out[480 + 2400][0] - 0.5005).abs() < 0.001,
        "decay reaches −60 dB of its distance: {}",
        out[480 + 2400][0]
    );
    assert!((out[5000][0] - 0.5).abs() < 1e-4, "sustain");
    e.note_off(0, 60);
    let out = render(&mut e, 2400);
    assert!(
        (out[2399][0] - 0.0005).abs() < 0.0002,
        "release: {}",
        out[2399][0]
    );
    render(&mut e, 2400);
    assert_eq!(e.active_voices(), 0);
}

#[test]
fn zone_crossfades_velocity_curve_and_start_offset() {
    let zone = Zone {
        low_velocity: 10,
        fade_low_velocity: 20,
        start_mod: Some(1000),
        ..Zone::default()
    };
    let ramp = Sample {
        rate: 48000,
        frames: (0..2000).map(|i| [i as f32 / 2000.0; 2]).collect(),
    };
    // No velocity assignment: velocity leaves the level alone.
    let bank = Bank::from_samples(
        vec![Group::default()],
        vec![zone],
        vec![(PathBuf::new(), ramp)],
    )
    .unwrap();
    let mut e = engine_with(bank);
    let mut event = NoteEvent::new(0, 60, 15);
    event.offset_us = 12500;
    let id = e.start_event(&event).unwrap();
    let gain = (6.0 / 21.0 * std::f32::consts::FRAC_PI_2).sin();
    let out = render(&mut e, 10);
    assert!(
        (out[9][0] - gain * 609.0 / 2000.0).abs() < 1e-4,
        "offset 600 frames at crossfade gain: {}",
        out[9][0]
    );
    e.change_event(id, EventChange::Volume(0.5));
    let out = render(&mut e, 256);
    assert!((out[255][0] - 0.5 * gain * (609.0 + 256.0) / 2000.0).abs() < 1e-4);
    e.fade_event(id, 0.001, 0.0, true);
    render(&mut e, 128);
    assert!(!e.event_active(id));
}

#[test]
fn instrument_volume_pan_and_bend_range() {
    let mut e = engine();
    e.note_on(0, 60, 127);
    render(&mut e, 128);
    e.cc(0, 7, 64);
    e.cc(0, 10, 127);
    let out = last(&mut e, 256);
    let volume = (64.0f32 / 127.0).powi(2);
    assert!(close(out, [0.0, 0.25 * volume]), "{out:?}");
    assert_eq!(e.cc_state()[0][7], 64);
    assert!(e.key_down(0, 60));
    e.note_off(0, 60);
    assert!(!e.key_down(0, 60));

    let ramp = Sample {
        rate: 48000,
        frames: (0..48000).map(|i| [i as f32 / 48000.0; 2]).collect(),
    };
    // Intensity 1.0 bends an octave.
    let bend = ModAssignment {
        name: "PB_PITCH".into(),
        source: ModSource::PitchBend,
        target: ModTarget::Pitch,
        intensity: 1.0,
        invert: false,
        lag_ms: 0,
        shaper: None,
    };
    let group = Group {
        mods: vec![bend],
        ..Group::default()
    };
    let bank = Bank::from_samples(
        vec![group],
        vec![Zone::default()],
        vec![(PathBuf::new(), ramp)],
    )
    .unwrap();
    let mut e = engine_with(bank);
    e.pitch_bend(0, 16383);
    e.note_on(0, 60, 127);
    let out = render(&mut e, 101);
    let expected = 100.0 * 2f32.powf(8191.0 / 8192.0) / 48000.0;
    assert!(
        (out[100][0] - expected).abs() < 1e-4,
        "{} vs {expected}",
        out[100][0]
    );
}

/// A float WAV long enough to stream, with non-periodic content.
fn write_wav(path: &Path, frames: usize) {
    write_wav_bits(path, frames, 32);
}

/// A stereo test signal: float for 32 bits, otherwise integer.
fn write_wav_bits(path: &Path, frames: usize, bits: u16) {
    let float = bits == 32;
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 44100,
        bits_per_sample: bits,
        sample_format: if float {
            hound::SampleFormat::Float
        } else {
            hound::SampleFormat::Int
        },
    };
    let scale = 2f32.powi(i32::from(bits) - 1) - 1.0;
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..frames {
        let t = i as f32;
        for x in [
            (t * 0.031).sin() * (t * 0.00037).cos(),
            (t * 0.017 + (t * 0.001).sin()).sin(),
        ] {
            if float {
                w.write_sample(x).unwrap();
            } else {
                w.write_sample((x * scale) as i32).unwrap();
            }
        }
    }
    w.finalize().unwrap();
}

fn instrument(groups: Vec<Group>, zones: Vec<Zone>) -> Instrument {
    Instrument {
        path: PathBuf::new(),
        name: "streamed".into(),
        groups,
        zones,
        warnings: Vec::new(),
        missing_samples: Vec::new(),
        scripts: Vec::new(),
        voice_limit: None,
        voice_groups: Vec::new(),
        fx: Default::default(),
        script_state: Vec::new(),
    }
}

#[test]
fn streamed_playback_matches_ram_playback() {
    let dir = std::env::temp_dir().join(format!("kontakto-stream-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let frames = 120_000;
    let (path, path24) = (dir.join("long.wav"), dir.join("long24.wav"));
    write_wav(&path, frames);
    write_wav_bits(&path24, frames, 24);
    let loop_range = Some(Loop {
        start: 30_000,
        end: 90_000,
        until_release: true,
        crossfade: 3000,
    });
    let cases = [
        (
            Group::default(),
            Zone {
                sample: path.clone(),
                ..Zone::default()
            },
        ),
        (
            Group::default(),
            Zone {
                sample: path.clone(),
                tune: 1.37,
                start: 500,
                end: -2000,
                ..Zone::default()
            },
        ),
        (
            Group::default(),
            Zone {
                sample: path.clone(),
                loop_range: loop_range.clone(),
                start_mod: Some(4000),
                ..Zone::default()
            },
        ),
        (
            Group {
                reverse: true,
                ..Group::default()
            },
            Zone {
                sample: path.clone(),
                ..Zone::default()
            },
        ),
    ];
    // Float and 24-bit sources, the latter also with a budget that shrinks the preload.
    let runs = cases.iter().flat_map(|case| {
        [(&path, None), (&path24, None), (&path24, Some(()))].map(|run| (case, run))
    });
    for (n, ((group, zone), (path, shrink))) in runs.enumerate() {
        let zone = Zone {
            sample: path.clone(),
            ..zone.clone()
        };
        let instrument = instrument(vec![group.clone()], vec![zone.clone()]);
        let mut streamed = Bank::load(&instrument).unwrap();
        if shrink.is_some() {
            streamed = Bank::load_within(&instrument, streamed.bytes - 1).unwrap();
            assert!(streamed.preload < PRELOAD_FRAMES, "case {n}");
        }
        assert_eq!(
            streamed.streamed_samples(),
            1,
            "case {n} streams instead of loading fully"
        );
        let group = group.clone();
        let decoded = kontakto::audio::decode(path, frames).unwrap();
        let ram =
            Bank::from_samples(vec![group], vec![zone], vec![(path.clone(), decoded)]).unwrap();
        let (mut a, mut b) = (engine_with(streamed), engine_with(ram));
        a.blocking_streams = true;
        let mut note = NoteEvent::new(0, 60, 100);
        note.offset_us = 50_000;
        let (ia, ib) = (a.start_event(&note).unwrap(), b.start_event(&note).unwrap());
        for block in 0..600 {
            if block == 400 {
                a.release_event(ia);
                b.release_event(ib);
            }
            let (x, y) = (render(&mut a, 128), render(&mut b, 128));
            assert!(x == y, "case {n} diverges in block {block}");
        }
        assert_eq!(a.underruns(), 0, "case {n}");
        assert!(render(&mut b, 1).iter().all(|f| f[0].is_finite()));
    }
    // Long enough to have exercised the streamer, not just the preload.
    assert!(frames as u64 > 4 * PRELOAD_FRAMES);
    std::fs::remove_dir_all(dir).unwrap();
}

/// 512 voices streaming from 32 files with the minimum preload, rendered at
/// real-time pace without waiting for the disk: the streamer keeps up.
#[test]
#[ignore = "timing-sensitive: run on an idle machine"]
fn streams_keep_up_at_real_time_pace() {
    let dir = std::env::temp_dir().join(format!("kontakto-pace-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (files, frames) = (32, 132_300);
    let (mut groups, mut zones) = (Vec::new(), Vec::new());
    for group in 0..files {
        let sample = dir.join(format!("{group}.wav"));
        write_wav_bits(&sample, frames, 24);
        groups.push(Group::default());
        zones.push(Zone {
            group,
            sample,
            ..Zone::default()
        });
    }
    let instrument = instrument(groups, zones);
    let full = Bank::load(&instrument).unwrap().bytes;
    let bank = Bank::load_within(&instrument, full - 1).unwrap();
    assert_eq!(bank.streamed_samples(), files);
    let mut e = engine_with(bank);
    let block = std::time::Duration::from_secs_f64(MAX_BLOCK as f64 / 48_000.0);
    let (mut late, mut peak) = (0, 0);
    let start = std::time::Instant::now();
    for b in 0..900u32 {
        // 16 notes of 32 voices each, one every 60 ms.
        if b % 22 == 0 && b / 22 < 16 {
            e.note_on(0, 48 + (b / 22) as u8, 100);
        }
        render(&mut e, MAX_BLOCK);
        peak = peak.max(e.active_voices());
        match (start + block * (b + 1)).checked_duration_since(std::time::Instant::now()) {
            Some(wait) => std::thread::sleep(wait),
            None => late += 1,
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
    assert_eq!(peak, 512);
    assert_eq!(e.underruns(), 0, "{late} late blocks");
}

#[test]
fn damaged_zones_are_skipped_not_fatal() {
    let dir = std::env::temp_dir().join(format!("kontakto-skip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let good = dir.join("good.wav");
    write_wav(&good, 1000);
    // Truncated data: the header opens but decoding fails partway.
    let truncated = dir.join("truncated.wav");
    write_wav(&truncated, 4000);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(&truncated)
        .unwrap();
    file.set_len(44 + 2000 * 8).unwrap();
    let zones = vec![
        Zone {
            sample: good.clone(),
            ..Zone::default()
        },
        Zone {
            sample: dir.join("absent.wav"),
            ..Zone::default()
        },
        Zone {
            sample: good,
            available: false,
            ..Zone::default()
        },
        Zone {
            sample: truncated.clone(),
            ..Zone::default()
        },
        Zone {
            sample: truncated,
            low_key: 70,
            ..Zone::default()
        },
    ];
    let bank = Bank::load(&instrument(vec![Group::default()], zones)).unwrap();
    assert_eq!((bank.zones().len(), bank.skipped_zones), (1, 4));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn wav_decode_and_case_insensitive_resolution() {
    let dir = std::env::temp_dir().join(format!("kontakto-check-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("Samples")).unwrap();
    let path = dir.join("Samples/piano.wav");
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(&path, spec).unwrap();
    for _ in 0..128 {
        w.write_sample(16384i16).unwrap();
        w.write_sample(-8192i16).unwrap();
    }
    w.finalize().unwrap();
    let mut r = Resolver::new(&dir);
    assert_eq!(r.resolve(&dir, "samples/PIANO.WAV").unwrap().unwrap(), path);
    let sample = kontakto::audio::decode(&path, 128).unwrap();
    assert_eq!(
        (sample.rate, sample.frames.len(), sample.frames[0]),
        (44100, 128, [0.5, -0.25])
    );
    assert!(kontakto::audio::decode(&path, 127).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

fn rack_frame(rack: &mut Rack, frames: usize) -> Frame {
    let buses = rack.render(frames);
    let i = frames - 1;
    buses.iter().fold([0.0; 2], |acc, bus| {
        [acc[0] + bus[0][i], acc[1] + bus[1][i]]
    })
}

#[test]
fn rack_routes_layers_mutes_solos_and_releases_after_channel_changes() {
    let mut rack = Rack::default();
    rack.parts[0] = engine();
    rack.parts[1] = engine();
    rack.controls[0].channel = 0;
    rack.controls[1].channel = 1;
    rack.note_on(0, 60, 127);
    assert_eq!(rack_frame(&mut rack, 21), [0.5, 0.25]);
    assert_eq!(rack.parts[1].active_voices(), 0);
    rack.note_on(1, 60, 127);
    assert_eq!(rack_frame(&mut rack, 21), [1.0, 0.5]);
    rack.controls[0].mute = true;
    assert_eq!(rack_frame(&mut rack, 1), [0.5, 0.25]);
    rack.controls[0].mute = false;
    rack.controls[1].solo = true;
    assert_eq!(rack_frame(&mut rack, 1), [0.5, 0.25]);
    rack.controls[1].pan = 1.0;
    rack.controls[1].gain = 0.5;
    assert_eq!(rack_frame(&mut rack, 1), [0.0, 0.125]);
    rack.controls[0].channel = 4;
    rack.note_off(0, 60);
    rack.note_off(1, 60);
    for _ in 0..8 {
        rack_frame(&mut rack, 128);
    }
    assert_eq!(rack_frame(&mut rack, 1), [0.0, 0.0]);
    rack.controls[1].solo = false;
    rack.controls[0].channel = -1;
    rack.controls[1].channel = -1;
    rack.controls[1].gain = 1.0;
    rack.controls[1].pan = 0.0;
    rack.note_on(3, 60, 127);
    assert_eq!(rack_frame(&mut rack, 21), [1.0, 0.5]);
}

#[test]
fn rack_midi_ports_audio_buses_and_route_changes_are_isolated() {
    let mut rack = Rack::default();
    rack.parts[0] = engine();
    rack.parts[1] = engine();
    rack.controls[1].port = 1;
    rack.controls[1].output = 3;
    rack.note_on_port(1, 0, 60, 127);
    let out = rack.render(21);
    assert_eq!([out[0][0][20], out[0][1][20]], [0.0, 0.0]);
    assert_eq!([out[3][0][20], out[3][1][20]], [0.5, 0.25]);
    rack.note_off_port(0, 0, 60);
    let out = rack.render(100);
    assert_eq!([out[3][0][99], out[3][1][99]], [0.5, 0.25]);
    let mut controls = rack.controls;
    controls[1].port = 2;
    rack.set_controls(controls);
    assert_eq!(rack_frame(&mut rack, 1), [0.0, 0.0]);
    rack.note_on_port(1, 0, 60, 127);
    assert_eq!(rack.parts[1].active_voices(), 0);
    rack.note_on_port(2, 0, 60, 127);
    assert_eq!(rack.parts[1].active_voices(), 1);
    rack.note_off_port(2, 0, 60);
    for _ in 0..8 {
        rack.render(128);
    }
    assert_eq!(rack.parts[1].active_voices(), 0);
}

fn library(path: &str) -> PathBuf {
    PathBuf::from(kontakto::import::LIBRARY_ROOT).join(path)
}

fn render_real(bank: Bank, note: u8) -> (f32, u64) {
    let mut e = Engine::default();
    e.blocking_streams = true;
    e.set_bank(Some(Box::new(bank)));
    e.note_on(0, note, 100);
    let mut peak = 0f32;
    for block in 0..750 {
        if block == 500 {
            e.note_off(0, note);
        }
        for f in render(&mut e, 128) {
            assert!(f[0].is_finite() && f[1].is_finite());
            peak = peak.max(f[0].abs());
        }
    }
    (peak, e.underruns())
}

#[test]
#[ignore = "uses the owner's installed library; never redistributes samples"]
fn vista_harp_real_instrument() {
    let path = library("Performance Samples Vista/Instruments/Bonus/Vista - Harp.nki");
    let source = kontakto::import::source_inventory(&path).unwrap();
    assert!(!source["chunks"].as_object().unwrap().is_empty());
    assert!(
        source["chunks"]
            .as_object()
            .unwrap()
            .values()
            .all(|v| v.get("inspection_error").is_none())
    );
    let i = kontakto::import::read(&path).unwrap();
    assert_eq!((i.groups.len(), i.zones.len()), (20, 2000));
    assert!(i.missing_samples.is_empty());
    assert!(!i.scripts.is_empty());
    let bank = Bank::load(&i).unwrap();
    assert_eq!(bank.skipped_zones, 0);
    assert_eq!(bank.zones().len(), 2000, "every group loads");
    let (peak, underruns) = render_real(bank, 60);
    assert!(peak > 0.001, "peak {peak}");
    assert_eq!(underruns, 0);
}

#[test]
#[ignore = "uses the owner's installed library; never redistributes samples"]
fn vista_harp_modulation_is_decoded() {
    use kontakto::import::{ModSource, ModTarget};

    let path = PathBuf::from(kontakto::import::LIBRARY_ROOT)
        .join("Performance Samples Vista/Instruments/Bonus/Vista - Harp.nki");
    let instrument = kontakto::import::read(&path).unwrap();
    assert!(
        !instrument
            .warnings
            .iter()
            .any(|w| w.contains("modulation not imported"))
    );
    for group in &instrument.groups {
        let env = group
            .volume_env
            .as_ref()
            .expect("every group has a volume AHDSR");
        assert!(env.attack_ms >= 0.0 && (0.0..=1.0).contains(&env.sustain));
        assert_eq!(group.cc_volume().map(|(cc, _)| cc), Some(11));
        assert!(
            group
                .modulation(ModSource::KeyPosition, &ModTarget::Volume)
                .is_some()
        );
    }
}

#[test]
#[ignore = "requires the owner's local Una Corda library"]
fn encrypted_una_corda_uses_local_access_data() {
    let i = kontakto::import::read(&library(
        "Una Corda Library/Instruments/Una Corda Cotton.nki",
    ))
    .unwrap();
    assert_eq!(
        i.missing_samples.len(),
        i.missing_samples
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
    );
    assert_eq!(i.name, "Una Corda Cotton");
    assert!(i.zones.len() > 4000);
    assert!(!i.scripts.is_empty());
    let bank = Bank::load(&i).unwrap();
    assert!(!bank.zones().is_empty());
    let (peak, _) = render_real(bank, 60);
    assert!(peak > 0.0001, "peak {peak}");
}

/// Gainer ×2 then a convolution whose IR is a unit impulse plus an echo.
fn fx_description(echo: usize) -> fx::ProgramFx {
    use fx::{Chain, Effect, Kind, Params, params};
    let effect = |slot, kind, params| Effect {
        slot,
        kind,
        version: 0,
        bypass: false,
        output_gain: 1.0,
        dry_level: 0.0,
        params,
    };
    let band = params::IrBand {
        length_ratio: 1.0,
        low_cut_hz: 20.0,
        high_cut_hz: 20_000.0,
    };
    let mut ir = vec![[0.0; 2]; echo + 1];
    (ir[0], ir[echo]) = ([1.0; 2], [0.5; 2]);
    let convolution = params::Convolution {
        unknown: [0.0; 2],
        predelay_ms: 0.0,
        early: band,
        late: band,
        unknown_9: 0.0,
        flags: [false; 5],
        curve_x: Vec::new(),
        curve_db: Vec::new(),
        ir_index: 0,
        ir_file: None,
        ir_error: None,
        ir: Some(params::Impulse(std::sync::Arc::new(Sample {
            rate: 48000,
            frames: ir,
        }))),
    };
    fx::ProgramFx {
        insert: Chain {
            slots: vec![
                effect(
                    0,
                    Kind::Gainer,
                    Params::Gainer(params::Gainer { gain: 2.0 }),
                ),
                effect(
                    1,
                    Kind::Convolution,
                    Params::Convolution(Box::new(convolution)),
                ),
            ],
        },
        ..Default::default()
    }
}

#[test]
fn effects_process_the_output_and_tails_outlive_the_voices() {
    const ECHO: usize = 2400;
    let (mut dry, mut wet) = (engine(), engine());
    wet.set_fx(fx_description(ECHO).processor(wet.rate() as f32, MAX_BLOCK));
    let play = |e: &mut Engine| {
        e.note_on(0, 60, 127);
        let mut out = render(e, 4800);
        e.note_off(0, 60);
        out.extend(render(e, 4800));
        out
    };
    let (dry_out, out) = (play(&mut dry), play(&mut wet));
    for i in [100usize, 3000, 4799, 5000] {
        for ch in 0..2 {
            let echo = i.checked_sub(ECHO).map_or(0.0, |j| dry_out[j][ch]);
            let want = 2.0 * dry_out[i][ch] + echo;
            assert!(
                (out[i][ch] - want).abs() < 1e-4,
                "{i}/{ch}: {} vs {want}",
                out[i][ch]
            );
        }
    }
    // The voice has ended, but the echo of its last moments still sounds.
    assert_eq!(wet.active_voices(), 0);
    let voice_end = dry_out.iter().rposition(|f| f[0] != 0.0).unwrap();
    assert!(voice_end < 4800 + 200, "release ended at {voice_end}");
    assert!(
        out[voice_end + ECHO / 2][0] > 0.1,
        "the tail stopped with the voices"
    );
    assert!(
        out[voice_end + ECHO + 1..]
            .iter()
            .all(|f| f[0].abs() < 1e-6)
    );

    // A reset silences the tail.
    wet.note_on(0, 60, 127);
    render(&mut wet, 1000);
    wet.reset(wet.rate());
    assert!(render(&mut wet, 3000).iter().all(|f| f[0] == 0.0));
}

/// Counts allocations and frees made by threads that armed it, so script
/// handoff and playback can prove they stay allocation-free.
struct CountingAlloc;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    // Per thread, so tests running in parallel do not count each other.
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn count() {
    if COUNTING.with(Cell::get) {
        ALLOCATIONS.with(|n| n.set(n.get() + 1));
    }
}

// SAFETY: forwards every call unchanged to the system allocator.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        count();
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

/// Allocations and frees `f` makes on this thread.
fn allocations(f: impl FnOnce()) -> usize {
    let before = ALLOCATIONS.with(Cell::get);
    COUNTING.with(|c| c.set(true));
    f();
    COUNTING.with(|c| c.set(false));
    ALLOCATIONS.with(Cell::get) - before
}

// Runtimes are built on the loader thread and handed to the audio thread.
const _: () = {
    const fn send<T: Send>() {}
    send::<Runtime>();
};

/// Two layered groups (0.1 and 0.2) on every key.
fn two_groups() -> Instrument {
    instrument(vec![Group::default(), Group::default()], Vec::new())
}

fn runtime(script: &str) -> Option<Box<Runtime>> {
    let mut i = two_groups();
    i.scripts = vec![script.to_owned()];
    let (rt, errors) = load_scripts(&i, Vec::new(), 48000.0);
    assert!(errors.is_empty(), "{errors:?}");
    rt
}

/// The two layered groups, driven by `script`.
fn scripted(script: &str) -> Engine {
    let mut e = engine_with(layered(two_groups().groups, &[0.1, 0.2]));
    assert!(e.set_script(runtime(script)).is_none());
    e
}

/// First frame with sound.
fn onset(out: &[Frame]) -> Option<usize> {
    out.iter().position(|f| f[0] != 0.0)
}

#[test]
fn ignored_notes_replayed_by_scripts_reach_the_voices() {
    let mut e = scripted(
        "on init\nend on\non note\nignore_event($EVENT_ID)\nplay_note($EVENT_NOTE + 2, $EVENT_VELOCITY, 0, -1)\nend on",
    );
    e.note_on(0, 60, 127);
    let out = render(&mut e, 64);
    assert!(
        onset(&out).is_some_and(|n| n <= 1),
        "plays at the note's frame"
    );
    assert!(
        close(out[63], [0.3; 2]),
        "both groups play the script's note"
    );
    e.note_off(0, 60);
    render(&mut e, 480);
    assert_eq!(
        e.active_voices(),
        0,
        "the replayed note follows its parent's release"
    );
}

#[test]
fn waits_delay_script_notes_to_the_exact_frame() {
    let mut e = scripted(
        "on init\nend on\non note\nignore_event($EVENT_ID)\nwait(1000)\nplay_note($EVENT_NOTE, $EVENT_VELOCITY, 0, -1)\nend on",
    );
    e.note_on(0, 60, 127);
    // 1 ms at 48 kHz is 48 frames; the render splits the voices there.
    let start = onset(&render(&mut e, 256)).expect("the delayed note plays");
    assert!((48..=49).contains(&start), "starts at frame {start}");
}

#[test]
fn fade_out_stops_the_voice() {
    let mut e =
        scripted("on init\nend on\non note\nwait(1000)\nfade_out($EVENT_ID, 1000, 1)\nend on");
    e.note_on(0, 60, 127);
    let out = render(&mut e, 200);
    assert!(close(out[40], [0.3; 2]), "plays until the fade");
    assert!(out[48..96].windows(2).all(|w| w[1][0] <= w[0][0]), "fades");
    assert!(close(out[199], [0.0; 2]));
    assert_eq!(e.active_voices(), 0, "the fade stops the voices");
}

#[test]
fn disallowed_groups_do_not_play() {
    let mut e = scripted("on init\nend on\non note\ndisallow_group(0)\nend on");
    e.note_on(0, 60, 127);
    assert!(close(last(&mut e, 64), [0.2; 2]), "only group 1 plays");
}

#[test]
fn note_durations_release_on_their_own() {
    let mut e = scripted(
        "on init\nend on\non note\nignore_event($EVENT_ID)\nplay_note($EVENT_NOTE, $EVENT_VELOCITY, 0, 5000)\nend on",
    );
    e.note_on(0, 60, 127);
    let out = render(&mut e, 1024);
    assert!(close(out[200], [0.3; 2]), "sounds for its duration");
    assert!(
        close(out[1023], [0.0; 2]),
        "released after 5 ms without a note-off"
    );
    assert_eq!(e.active_voices(), 0);
}

#[test]
fn script_handoff_and_playback_do_not_allocate() {
    let script = "on init\ndeclare $count\nmake_persistent($count)\nend on\non note\nignore_event($EVENT_ID)\ninc($count)\nwait(500)\nplay_note($EVENT_NOTE, $EVENT_VELOCITY, 0, 20000)\nend on";
    let mut e = scripted(script);
    let next = runtime(script);
    let mut snapshot = next.as_ref().unwrap().persistence();
    let mut retired = None;
    let (mut left, mut right) = (vec![0.0; 512], vec![0.0; 512]);
    let count = allocations(|| {
        retired = e.set_script(next);
        for note in 60..64 {
            e.note_on(0, note, 100);
            e.render(&mut left, &mut right);
            e.note_off(0, note);
        }
        e.render(&mut left, &mut right);
        e.script().unwrap().refresh_persistence(&mut snapshot);
    });
    assert_eq!(count, 0, "the audio thread allocated");
    assert!(
        retired.is_some(),
        "the old runtime returns for disposal elsewhere"
    );
    assert!(left.iter().any(|x| *x != 0.0));
    assert_eq!(snapshot[0]["$count"], Value::Int(4));
}

/// Volume modulation of `source` at full intensity, without a shaper.
fn volume_mod(source: ModSource, lag_ms: u16) -> ModAssignment {
    ModAssignment {
        name: String::new(),
        source,
        target: ModTarget::Volume,
        intensity: 1.0,
        invert: false,
        lag_ms,
        shaper: None,
    }
}

#[test]
fn cc_volume_follows_the_controller_with_lag_without_a_script() {
    let group = Group {
        mods: vec![volume_mod(ModSource::MidiCc(11), 100)],
        ..Group::default()
    };
    let mut e = engine_with(layered(vec![group], &[0.5]));
    e.note_on(0, 60, 127);
    assert!(close(last(&mut e, 480), [0.5; 2]), "CC11 starts at 127");
    // One lag time constant after CC11 drops to 0, 63% of the way down.
    e.cc(0, 11, 0);
    let out = render(&mut e, 4800);
    let expected = 0.5 * (-1.0f32).exp();
    assert!((out[4799][0] - expected).abs() < 0.01, "{:?}", out[4799]);
    assert!(max_step(&out) < 1e-3, "the lag smooths the drop");
    render(&mut e, 48000);
    assert!(last(&mut e, 64)[0] < 1e-4);
}

/// Group 0 scales with velocity (`VEL_VOLUME`); group 1 is plain. Bus 0
/// holds a ×2 gainer and its fader is at 0.25.
fn modulated_groups() -> Instrument {
    let velocity = Group {
        mods: vec![volume_mod(ModSource::Velocity, 0)],
        modulators: vec![Modulator {
            name: "VEL_VOLUME".into(),
            targets: vec![String::new()],
            assignments: Some(0),
            volume_env: false,
        }],
        ..Group::default()
    };
    let mut i = instrument(vec![velocity, Group::default()], Vec::new());
    i.fx.buses = vec![fx::Bus {
        index: 0,
        name: "bus".into(),
        volume: 0.25,
        pan: 0.0,
        output: -1,
        chain: fx::Chain {
            slots: vec![fx::Effect {
                slot: 0,
                kind: fx::Kind::Gainer,
                version: 0,
                bypass: false,
                output_gain: 1.0,
                dry_level: 0.0,
                params: fx::Params::Gainer(fx::params::Gainer { gain: 2.0 }),
            }],
        },
    }];
    i
}

#[test]
fn scripts_set_intensity_output_bus_and_volume_sample_accurately() {
    let mut i = modulated_groups();
    i.scripts = vec![
        "on init
set_engine_par($ENGINE_PAR_MOD_TARGET_INTENSITY, 0, 0, find_mod(0, \"VEL_VOLUME\"), -1)
set_engine_par($ENGINE_PAR_OUTPUT_CHANNEL, $NI_BUS_OFFSET, 1, -1, -1)
end on
on note
wait(10000)
set_engine_par($ENGINE_PAR_VOLUME, 0, 1, -1, -1)
end on"
            .into(),
    ];
    let (rt, errors) = load_scripts(&i, Vec::new(), 48000.0);
    assert!(errors.is_empty(), "{errors:?}");
    let mut e = engine_with(layered(i.groups.clone(), &[0.1, 0.2]));
    assert!(e.set_script(rt).is_none());
    e.set_fx(i.fx.processor(e.rate() as f32, MAX_BLOCK));

    // Group 0 ignores velocity (intensity 0): 0.1. Group 1 goes through bus
    // 0: 0.2 × 2 × 0.25 = 0.1. Wrong routing or intensity gives 0.05–0.3.
    e.note_on(0, 60, 64);
    let out = render(&mut e, 1024);
    assert!(close(out[400], [0.2; 2]), "{:?}", out[400]);
    // 10 ms in, the script silences group 1, smoothed over the rest.
    assert!(close(out[479], [0.2; 2]), "{:?}", out[479]);
    assert!(out[481][0] < 0.2 && out[481][0] > 0.19, "{:?}", out[481]);
    assert!(close(out[1023], [0.1; 2]), "{:?}", out[1023]);

    // Steady state with queued writes and a bus does not allocate.
    let (mut left, mut right) = (vec![0.0; 512], vec![0.0; 512]);
    let count = allocations(|| {
        for note in 61..65 {
            e.note_on(0, note, 100);
            e.render(&mut left, &mut right);
            e.note_off(0, note);
            e.render(&mut left, &mut right);
        }
    });
    assert_eq!(count, 0, "the audio thread allocated");
    assert!(left.iter().any(|x| *x != 0.0));
}

#[test]
fn controllers_set_while_loading_drive_modulation() {
    let group = Group {
        mods: vec![volume_mod(ModSource::MidiCc(11), 0)],
        ..Group::default()
    };
    let mut i = instrument(vec![group], Vec::new());
    i.scripts =
        vec!["on init\nend on\non persistence_changed\nset_controller(11, 0)\nend on".into()];
    let (rt, errors) = load_scripts(&i, Vec::new(), 48000.0);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(rt.as_ref().unwrap().init_controllers, vec![(11, 0)]);
    let mut e = engine_with(layered(i.groups.clone(), &[0.5]));
    assert!(e.set_script(rt).is_none());
    e.note_on(0, 60, 127);
    assert!(close(last(&mut e, 480), [0.0; 2]), "CC11 starts at 0");
    // A reset keeps the controllers the script set.
    e.reset(e.rate());
    e.note_on(0, 60, 127);
    assert!(close(last(&mut e, 480), [0.0; 2]));
}

#[test]
fn set_controller_in_init_reaches_the_engine() {
    let mut i = instrument(vec![Group::default()], Vec::new());
    i.scripts = vec!["on init\nset_controller(1, 90)\nplay_note(60, 100, 0, -1)\nend on".into()];
    let (rt, errors) = load_scripts(&i, Vec::new(), 48000.0);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(rt.unwrap().init_controllers, vec![(1, 90)]);
}

#[test]
fn steady_scripted_playback_with_diagnostics_does_not_allocate() {
    // After a warm-up note sizes the string buffers, every note runs a native
    // scan, builds an 80-byte persistent string and, from the first counted
    // note on, faults (out of bounds) and notes a diagnostic (note 200).
    let script = "on init\ndeclare %a[4]\ndeclare %seen[128]\ndeclare $i\ndeclare $n\ndeclare @log\nmake_persistent(@log)\nend on\non note\nif ($EVENT_NOTE > 60)\n%a[$EVENT_NOTE] := 1\nplay_note(200, 100, 0, -1)\nend if\n$i := 0\nwhile ($i < 128)\nif (%seen[$i] = $EVENT_NOTE)\ninc($n)\nend if\ninc($i)\nend while\n%seen[$EVENT_NOTE] := $EVENT_NOTE\n@log := \"0123456789012345678901234567890123456789\" & \"0123456789012345678901234567890123456789\"\nend on";
    let rt = runtime(script);
    let mut snapshot = rt.as_ref().unwrap().persistence();
    let mut e = engine_with(layered(two_groups().groups, &[0.1, 0.2]));
    assert!(e.set_script(rt).is_none());
    let (mut left, mut right) = (vec![0.0; 512], vec![0.0; 512]);
    let mut play = |e: &mut Engine, note| {
        e.note_on(0, note, 100);
        e.render(&mut left, &mut right);
        e.note_off(0, note);
        e.render(&mut left, &mut right);
    };
    play(&mut e, 60);
    assert!(e.script().unwrap().diagnostics().is_empty());
    let count = allocations(|| {
        for note in 61..69 {
            play(&mut e, note);
        }
        e.script().unwrap().refresh_persistence(&mut snapshot);
    });
    assert_eq!(count, 0, "the audio thread allocated");
    let diagnostics = e.script().unwrap().diagnostics().join("\n");
    assert!(diagnostics.contains("out of bounds"), "{diagnostics}");
    assert!(diagnostics.contains("play_note"), "{diagnostics}");
    // The string outgrew its snapshot buffer: cut, reported, whole once regrown.
    assert!(!settle_persistence(&mut snapshot));
    e.script().unwrap().refresh_persistence(&mut snapshot);
    assert!(settle_persistence(&mut snapshot));
    let whole = "0123456789012345678901234567890123456789".repeat(2);
    assert_eq!(snapshot[0]["@log"], Value::Text(whole));
}
