use kontakto::{
    audio::Sample,
    engine::{
        Ahdsr, Bank, BusControls, Engine, EventChange, MAX_BLOCK, MAX_VOICES, Mix, NoteEvent,
        PRELOAD_FRAMES, Rack, Streaming,
        effects, load_scripts,
    },
    fx,
    import::{
        FlexEnvelope, FlexPoint, Group, Instrument, Loop, ModAssignment, ModSource, ModTarget,
        Modulator, Resolver, VoiceLimit, Zone,
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
    render(&mut e, 480);
    assert_eq!(e.active_voices(), 0);
}

#[test]
fn root_pitch_octave_reverse_and_end_bounds() {
    // Long enough that the end's declick ramp stays clear of the checks.
    let ramp = || Sample {
        rate: 48000,
        frames: (0..1000).map(|i| [i as f32 / 100.0; 2]).collect(),
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
    render(&mut a, 500);
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
    assert!((last(&mut b, 11)[0] - 9.89).abs() < 1e-5);
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
    render(&mut e, 480);

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

/// A release-trigger group with T = 1000 ms and RTC_VOLUME at full intensity
/// without a shaper: the release plays at the counter's remaining share,
/// 1 - held / T.
fn counted_release() -> Group {
    Group {
        release_trigger: true,
        release_counter_ms: 1000,
        mods: vec![ModAssignment {
            name: "RTC_VOLUME".into(),
            source: ModSource::ReleaseTriggerCounter,
            target: ModTarget::Volume,
            intensity: 1.0,
            invert: false,
            lag_ms: 0,
            shaper: None,
        }],
        ..Group::default()
    }
}

#[test]
fn release_trigger_counter_scales_release_volume_by_held_time() {
    let mut e = engine_with(layered(vec![counted_release()], &[0.4]));
    let mut release_after = |held: usize, pedal: bool| {
        e.cc(0, 120, 0);
        render(&mut e, 480);
        e.cc(0, 64, if pedal { 127 } else { 0 });
        e.note_on(0, 60, 100);
        render(&mut e, held);
        e.note_off(0, 60);
        if pedal {
            // The counter stops at the key release, not at pedal up.
            render(&mut e, 24000);
            e.cc(0, 64, 0);
        }
        last(&mut e, 200)[0]
    };
    let short = release_after(480, false);
    let half = release_after(24000, false);
    let long = release_after(96000, false);
    assert!((short - 0.4 * 0.99).abs() < 1e-4, "10 ms held: {short}");
    assert!((half - 0.2).abs() < 1e-4, "500 ms held: {half}");
    assert!(long.abs() < 1e-6, "held past T: {long}");
    let pedalled = release_after(24000, true);
    assert!((pedalled - 0.2).abs() < 1e-4, "pedal-deferred: {pedalled}");
}

#[test]
fn reset_rls_trig_counter_restarts_the_count() {
    let mut i = instrument(vec![counted_release()], Vec::new());
    i.scripts = vec![
        "on init\nend on\non note\nwait(500000)\nreset_rls_trig_counter($EVENT_NOTE)\nend on"
            .into(),
    ];
    let (rt, errors) = load_scripts(&i, Vec::new(), 48000.0);
    assert!(errors.is_empty(), "{errors:?}");
    let mut e = engine_with(layered(i.groups, &[0.4]));
    assert!(e.set_script(rt).is_none());
    e.note_on(0, 60, 100);
    render(&mut e, 28800);
    e.note_off(0, 60);
    // Held 600 ms, counted from the reset at 500 ms: 100 ms.
    let out = last(&mut e, 200)[0];
    assert!((out - 0.4 * 0.9).abs() < 1e-3, "{out}");
}

/// Pacific's release groups send the counter to sample start (RTC_PITCH): the
/// release skips the sustain pre-roll its sample opens with. Dropping this
/// route made the library audit about 4 dB louder, not more correct.
#[test]
fn release_trigger_counter_moves_release_sample_start() {
    let mut group = counted_release();
    group.mods[0].name = "RTC_PITCH".into();
    group.mods[0].target = ModTarget::SampleStart;
    let zone = Zone { start_mod: Some(1000), ..Zone::default() };
    let ramp = Sample { rate: 48000, frames: (0..2000).map(|i| [i as f32 / 2000.0; 2]).collect() };
    let bank = Bank::from_samples(vec![group], vec![zone], vec![(PathBuf::new(), ramp)]).unwrap();
    let mut e = engine_with(bank);
    e.note_on(0, 60, 100);
    render(&mut e, 24000);
    e.note_off(0, 60);
    // Held 500 ms of T = 1000: x = 0.5, so 500 of the 1000 start-mod frames.
    let out = render(&mut e, 60)[59][0];
    assert!((out - 559.0 / 2000.0).abs() < 2e-3, "{out}");
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
fn flex_envelope_shapes_group_volume() {
    let point = |time_ms, level, curve| FlexPoint {
        time_ms,
        level,
        curve,
    };
    let group = Group {
        flex_env: Some(FlexEnvelope {
            // Silent 5 ms, linear rise over 10 ms, sustain, 10 ms release
            // bulging below the line (fast start).
            points: vec![
                point(5.0, 0.0, 0.5),
                point(10.0, 1.0, 0.5),
                point(10.0, 0.0, 0.0),
            ],
            sustain: 1,
            unknown_index: 0,
            unknown_tail: Vec::new(),
        }),
        ..Group::default()
    };
    let bank = Bank::from_samples(
        vec![group],
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
    let mut e = engine_with(bank);
    e.note_on(0, 60, 127);
    let out = render(&mut e, 4800);
    assert_eq!(out[200][0], 0.0, "delay segment");
    assert!((out[240 + 239][0] - 0.5).abs() < 0.01, "{}", out[479][0]);
    assert!((out[4799][0] - 1.0).abs() < 1e-5, "sustain point");
    e.note_off(0, 60);
    let out = render(&mut e, 480);
    // A quarter of the way: linear would still be at 0.75.
    assert!(out[119][0] < 0.3, "convex release: {}", out[119][0]);
    render(&mut e, 64);
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
    // Past the 1 ms declick ramp of a mid-sample start.
    let out = render(&mut e, 60);
    assert!(
        (out[59][0] - gain * 659.0 / 2000.0).abs() < 1e-4,
        "offset 600 frames at crossfade gain: {}",
        out[59][0]
    );
    e.change_event(id, EventChange::Volume(0.5));
    let out = render(&mut e, 256);
    assert!((out[255][0] - 0.5 * gain * (659.0 + 256.0) / 2000.0).abs() < 1e-4);
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
        ..Default::default()
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
        let progress = std::sync::atomic::AtomicU32::new(0);
        let mut streamed =
            Bank::load_counting(&instrument, kontakto::engine::MEMORY_LIMIT, Streaming::Auto, &[], &progress).unwrap();
        assert_eq!(
            progress.into_inner(),
            kontakto::engine::LOAD_DONE,
            "a load ends at done"
        );
        if shrink.is_some() {
            streamed = Bank::load_within(&instrument, streamed.planned - 1).unwrap();
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

/// A part's tune transposes everything it plays, running voices included,
/// in semitones and cents.
#[test]
fn rack_part_tune_transposes_in_semitones_and_cents() {
    let ramp = || Sample {
        rate: 48000,
        frames: (0..1000).map(|i| [i as f32 / 1000.0; 2]).collect(),
    };
    let bank = || {
        Bank::from_samples(vec![Group::default()], vec![Zone::default()], vec![(PathBuf::new(), ramp())])
            .unwrap()
    };
    let mut rack = Rack::default();
    rack.parts[0] = engine_with(bank());
    rack.controls[0].tune = 12.0;
    rack.note_on(0, 60, 127);
    assert!((rack_frame(&mut rack, 11)[0] - 0.02).abs() < 1e-6, "an octave up steps two frames");
    // Down an octave, while the voice plays: half a frame a frame.
    rack.controls[0].tune = -12.0;
    let before = rack_frame(&mut rack, 1)[0];
    let after = rack_frame(&mut rack, 10)[0];
    assert!((after - before - 0.005).abs() < 1e-5, "{before} → {after}");
    // Cents: +7 st 2 ct is the fifth's ratio to within a cent.
    let mut rack = Rack::default();
    rack.parts[0] = engine_with(bank());
    rack.controls[0].tune = 7.02;
    rack.note_on(0, 60, 127);
    let at = rack_frame(&mut rack, 101)[0] * 1000.0;
    assert!((at - 150.0).abs() < 0.2, "a fifth steps 1.5 frames: {at}");
}

/// Output buses: faders scale what reaches them, solo and mute work across
/// buses, an aux send copies a part to a second bus post-fader, and the
/// peaks follow each stage.
#[test]
fn rack_bus_faders_solos_sends_and_peaks() {
    let mut rack = Rack::default();
    rack.parts[0] = engine();
    rack.parts[1] = engine();
    rack.controls[1].output = 2;
    rack.controls[1].gain = 0.5;
    rack.controls[1].aux = 5;
    rack.controls[1].aux_gain = 0.25;
    rack.bus_controls[2].gain = 0.5;
    rack.note_on(0, 60, 127);
    let out = rack.render(21);
    assert_eq!([out[0][0][20], out[0][1][20]], [0.5, 0.25]);
    assert_eq!([out[2][0][20], out[2][1][20]], [0.125, 0.0625], "part 0.5 × bus 0.5");
    assert_eq!([out[5][0][20], out[5][1][20]], [0.0625, 0.03125], "send 0.25 of the part's 0.25");
    let peaks = std::mem::take(&mut rack.peaks);
    assert_eq!(peaks.parts[0], [0.5, 0.25]);
    assert_eq!(peaks.parts[1], [0.25, 0.125]);
    assert_eq!(peaks.buses[2], [0.125, 0.0625]);
    assert_eq!(peaks.parts[2], [0.0; 2], "an empty slot reads silent");
    rack.bus_controls[2].solo = true;
    let out = rack.render(1);
    assert_eq!([out[0][0][0], out[5][0][0], out[2][0][0]], [0.0, 0.0, 0.125]);
    rack.bus_controls[2].mute = true;
    assert_eq!(rack_frame(&mut rack, 1), [0.0, 0.0]);
    rack.bus_controls[2] = BusControls::on(2);
    rack.bus_controls[0].pan = 1.0;
    let out = rack.render(1);
    assert_eq!([out[0][0][0], out[0][1][0]], [0.0, 0.25]);
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
    rack.set_controls(Mix {
        parts: controls,
        buses: rack.bus_controls,
    });
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
            flex: false,
            envelope: None,
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

/// Areia 16 Violins keeps a 12000-frame start-offset range for each of
/// 24576 samples: 1.8 GiB resident. Tight budgets shed resident
/// start-offset range, then preload, then load over budget: never fail.
/// Offsets past the resident range stream and play as they do from RAM.
#[test]
fn tight_budgets_stream_start_offsets_instead_of_failing() {
    let dir = std::env::temp_dir().join(format!("kontakto-cover-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (path, frames) = (dir.join("offset.wav"), 60_000);
    write_wav_bits(&path, frames, 24);
    let zone = Zone {
        sample: path.clone(),
        start_mod: Some(20_000),
        ..Zone::default()
    };
    let instrument = instrument(vec![Group::default()], vec![zone.clone()]);
    let full = Bank::load(&instrument).unwrap();
    assert_eq!(full.warning, None);
    // Room for the preload but only part of the start-offset range.
    let capped = Bank::load_within(&instrument, full.planned - 10_000 * 6).unwrap();
    assert!(capped.preload >= 1024 && capped.cover < 20_000);
    assert!(capped.warning.as_ref().unwrap().contains("offsets"));
    let starved = Bank::load_within(&instrument, 1).unwrap();
    assert_eq!(starved.cover, 0);
    assert!(starved.warning.as_ref().unwrap().contains("over"));
    for bank in [capped, starved] {
        let decoded = kontakto::audio::decode(&path, frames).unwrap();
        let ram = Bank::from_samples(
            vec![Group::default()],
            vec![zone.clone()],
            vec![(path.clone(), decoded)],
        )
        .unwrap();
        let (mut a, mut b) = (engine_with(bank), engine_with(ram));
        a.blocking_streams = true;
        let mut note = NoteEvent::new(0, 60, 100);
        // 17640 frames: past both banks' resident offset range.
        note.offset_us = 400_000;
        a.start_event(&note).unwrap();
        b.start_event(&note).unwrap();
        // Let the streamer fill its whole ring ahead before the first block:
        // it must still keep the frame before the start (the cubic's left tap).
        std::thread::sleep(std::time::Duration::from_millis(200));
        let mut heard = 0f32;
        for block in 0..200 {
            let (x, y) = (render(&mut a, 128), render(&mut b, 128));
            let at = x.iter().zip(&y).position(|(p, q)| p != q);
            assert!(x == y, "diverges in block {block} at frame {at:?}");
            heard = x.iter().fold(heard, |m, f| m.max(f[0].abs()));
        }
        assert!(heard > 0.01);
        assert_eq!(a.underruns(), 0);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// A voice muted a while stops streaming (scripts mute crossfade layers
/// and mic positions no one hears) and streams again from where it is when
/// heard: the same audio as a voice that never stopped, from RAM.
#[test]
fn muted_streamed_voices_pause_and_resume_exactly() {
    let dir = std::env::temp_dir().join(format!("kontakto-mute-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (path, frames) = (dir.join("muted.wav"), 200_000);
    write_wav_bits(&path, frames, 24);
    let zone = Zone {
        sample: path.clone(),
        ..Zone::default()
    };
    let instrument = instrument(vec![Group::default()], vec![zone.clone()]);
    let streamed = Bank::load_within(&instrument, 1).unwrap();
    let decoded = kontakto::audio::decode(&path, frames).unwrap();
    let ram = Bank::from_samples(vec![Group::default()], vec![zone], vec![(path, decoded)]).unwrap();
    let (mut a, mut b) = (engine_with(streamed), engine_with(ram));
    a.blocking_streams = true;
    let note = NoteEvent::new(0, 60, 100);
    let (id_a, id_b) = (a.start_event(&note).unwrap(), b.start_event(&note).unwrap());
    let mut compare = |a: &mut Engine, b: &mut Engine, blocks: usize| {
        for block in 0..blocks {
            assert!(render(a, 128) == render(b, 128), "diverges in block {block}");
        }
    };
    compare(&mut a, &mut b, 20);
    a.change_event(id_a, EventChange::Volume(0.0));
    b.change_event(id_b, EventChange::Volume(0.0));
    // 300 ms muted: past the pause.
    compare(&mut a, &mut b, 113);
    assert!(!a.voice_census()[0].streams, "a muted voice stops streaming");
    a.change_event(id_a, EventChange::Volume(1.0));
    b.change_event(id_b, EventChange::Volume(1.0));
    compare(&mut a, &mut b, 100);
    assert!(a.voice_census()[0].streams);
    assert_eq!(a.underruns(), 0);
    std::fs::remove_dir_all(dir).unwrap();
}

/// RAM only loads a sample whole even past the budget: nothing streams,
/// and it plays as the streamed bank does.
#[test]
fn ram_only_loads_samples_whole() {
    let dir = std::env::temp_dir().join(format!("kontakto-ram-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (path, frames) = (dir.join("whole.wav"), 200_000);
    write_wav_bits(&path, frames, 24);
    let zone = Zone {
        sample: path.clone(),
        ..Zone::default()
    };
    let instrument = instrument(vec![Group::default()], vec![zone]);
    let progress = Default::default();
    let load = |streaming| Bank::load_counting(&instrument, 1, streaming, &[], &progress).unwrap();
    let (streamed, whole) = (load(Streaming::Auto), load(Streaming::RamOnly));
    assert_eq!((streamed.streamed_samples(), whole.streamed_samples()), (1, 0));
    assert!(whole.warning.is_none() && whole.bytes >= frames as usize * 3);
    let (mut a, mut b) = (engine_with(streamed), engine_with(whole));
    a.blocking_streams = true;
    let note = NoteEvent::new(0, 60, 100);
    a.start_event(&note).unwrap();
    b.start_event(&note).unwrap();
    for block in 0..400 {
        assert!(render(&mut a, 128) == render(&mut b, 128), "diverges in block {block}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// The RAM-only fill finishing mid-note: a voice started on the streamed
/// bank carries on from the resident one, as if it had played from RAM
/// all along; and the other way round, from RAM onto a stream.
#[test]
fn voices_carry_over_when_the_ram_fill_lands() {
    let dir = std::env::temp_dir().join(format!("kontakto-fill-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (path, frames) = (dir.join("fill.wav"), 200_000);
    write_wav_bits(&path, frames, 24);
    let zone = Zone {
        sample: path.clone(),
        ..Zone::default()
    };
    let instrument = instrument(vec![Group::default()], vec![zone]);
    let progress = Default::default();
    let load = |streaming| Bank::load_counting(&instrument, 1, streaming, &[], &progress).unwrap();
    for (from, to) in [(Streaming::Auto, Streaming::RamOnly), (Streaming::RamOnly, Streaming::Auto)] {
        let (mut a, mut b) = (engine_with(load(from)), engine_with(load(Streaming::RamOnly)));
        a.blocking_streams = true;
        let note = NoteEvent::new(0, 60, 100);
        a.start_event(&note).unwrap();
        b.start_event(&note).unwrap();
        for block in 0..400 {
            if block == 150 {
                assert!(a.upgrade_bank(Box::new(load(to))).is_some());
                assert_eq!(a.voice_census().len(), 1, "the voice plays on");
            }
            assert!(render(&mut a, 128) == render(&mut b, 128), "{from:?} -> {to:?} diverges in block {block}");
        }
        assert_eq!(a.underruns(), 0);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// A start offset the scripts pin with a controller (Areia's CC113) stays
/// resident when the whole offset range does not fit: the voice starts from
/// RAM, without waiting for the disk.
#[test]
fn tight_budgets_keep_the_scripted_start_offset_resident() {
    let dir = std::env::temp_dir().join(format!("kontakto-reach-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (path, frames) = (dir.join("offset.wav"), 60_000);
    write_wav_bits(&path, frames, 24);
    let group = Group {
        mods: vec![ModAssignment {
            name: "CC_START".into(),
            source: ModSource::MidiCc(113),
            target: ModTarget::SampleStart,
            intensity: 1.0,
            invert: false,
            lag_ms: 0,
            shaper: None,
        }],
        ..Group::default()
    };
    let zone = Zone {
        sample: path.clone(),
        start_mod: Some(20_000),
        ..Zone::default()
    };
    let instrument = instrument(vec![group.clone()], vec![zone.clone()]);
    let full = Bank::load(&instrument).unwrap();
    let progress = Default::default();
    let bank =
        Bank::load_counting(&instrument, full.planned - 5000 * 6, Streaming::Auto, &[(113, 120)], &progress).unwrap();
    assert_eq!(bank.preload, PRELOAD_FRAMES);
    assert!(bank.warning.as_ref().unwrap().contains("controller settings"));
    let decoded = kontakto::audio::decode(&path, frames).unwrap();
    let ram = Bank::from_samples(vec![group], vec![zone], vec![(path.clone(), decoded)]).unwrap();
    let (mut a, mut b) = (engine_with(bank), engine_with(ram));
    for e in [&mut a, &mut b] {
        e.cc(0, 113, 120);
        e.note_on(0, 60, 100);
    }
    // Within the preload past the offset: no disk read is needed yet.
    for block in 0..12 {
        assert!(render(&mut a, 128) == render(&mut b, 128), "diverges in block {block}");
    }
    assert_eq!(a.underruns(), 0);
    std::fs::remove_dir_all(dir).unwrap();
}

// Sampler edge cases. Each states the Kontakt behaviour it pins down.

/// A key re-struck under the sustain pedal starts a new voice and the
/// sustained one keeps ringing; pedal-up releases both.
#[test]
fn restruck_notes_under_the_pedal_layer_until_pedal_up() {
    let mut e = engine();
    e.note_on(0, 60, 127);
    e.cc(0, 64, 127);
    e.note_off(0, 60);
    e.note_on(0, 60, 127);
    assert_eq!(last(&mut e, 64), [1.0, 0.5], "both voices sound");
    e.note_off(0, 60);
    assert_eq!(last(&mut e, 1000), [1.0, 0.5], "the pedal holds both");
    e.cc(0, 64, 0);
    render(&mut e, 1000);
    assert_eq!(e.active_voices(), 0);
}

/// Sostenuto (CC66) holds only the notes down when it is pressed.
#[test]
fn sostenuto_holds_only_the_notes_down_when_pressed() {
    let mut e = engine();
    e.note_on(0, 60, 127);
    e.cc(0, 66, 127);
    e.note_off(0, 60);
    e.note_on(0, 62, 127);
    e.note_off(0, 62);
    render(&mut e, 1000);
    assert_eq!(e.active_voices(), 1, "62 was not latched");
    assert_eq!(last(&mut e, 1), [0.5, 0.25]);
    e.cc(0, 66, 0);
    render(&mut e, 1000);
    assert_eq!(e.active_voices(), 0);
    // A latched key still down at sostenuto-up keeps sounding.
    e.note_on(0, 64, 127);
    e.cc(0, 66, 127);
    e.cc(0, 66, 0);
    assert_eq!(last(&mut e, 1000), [0.5, 0.25]);
}

/// Note-on with velocity 0 is a note-off; a note-off before the note's first
/// render still ends it; duplicate note-ons all end at the key's note-off.
#[test]
fn velocity_zero_early_note_off_and_duplicate_note_ons_leave_no_stuck_voices() {
    let mut e = engine();
    e.note_on(0, 60, 100);
    e.note_on(0, 60, 0);
    render(&mut e, 1000);
    assert_eq!(e.active_voices(), 0, "velocity 0 releases");
    e.note_on(0, 61, 100);
    e.note_on(0, 61, 90);
    e.note_off(0, 61);
    render(&mut e, 1000);
    assert_eq!(e.active_voices(), 0, "one note-off ends duplicate note-ons");
    let mut s = scripted("on init\nend on\non note\nend on");
    s.note_on(0, 60, 100);
    s.note_off(0, 60);
    render(&mut s, 48000);
    assert_eq!(s.active_voices(), 0, "a note-off before the first render ends it");
}

/// All-sound-off (CC120) silences every voice at once, with a click-free
/// few-ms fade; all-notes-off (CC123) releases keys but respects the pedal.
#[test]
fn all_sound_off_fades_fast_and_all_notes_off_respects_the_pedal() {
    let zone = Zone {
        loop_range: Some(Loop {
            start: 0,
            end: 4800,
            until_release: false,
            crossfade: 0,
        }),
        ..Zone::default()
    };
    let bank = Bank::from_samples(
        vec![Group::default()],
        vec![zone],
        vec![(PathBuf::new(), sine(100.0, 4800))],
    )
    .unwrap();
    let mut e = engine_with(bank);
    e.release = 5.0;
    e.note_on(0, 60, 127);
    // Cut at the sine's peak: a hard stop would step by 1.
    let mut out = render(&mut e, 1025);
    e.cc(0, 120, 0);
    out.extend(render(&mut e, 480));
    assert!(max_step(&out[100..]) < 0.1, "cut step {}", max_step(&out[100..]));
    assert_eq!(e.active_voices(), 0, "gone within 10 ms despite a 5 s release");
    e.note_on(0, 60, 127);
    e.cc(0, 64, 127);
    e.cc(0, 123, 0);
    render(&mut e, 4800);
    assert_eq!(e.active_voices(), 1, "the pedal holds notes through all-notes-off");
}

/// A voice that starts mid-waveform (start offset) or whose sample ends
/// mid-waveform ramps in and out instead of clicking.
#[test]
fn voices_ramp_at_mid_waveform_starts_and_sample_ends() {
    let period = 100.0;
    let play = |start_mod: Option<u32>, offset_us: u64| {
        let zone = Zone {
            start_mod,
            ..Zone::default()
        };
        let bank = Bank::from_samples(
            vec![Group::default()],
            vec![zone],
            // Ends at a peak of the sine: a hard stop would jump by 1.
            vec![(PathBuf::new(), sine(period, 4825))],
        )
        .unwrap();
        let mut e = engine_with(bank);
        e.attack = 0.0;
        let mut note = NoteEvent::new(0, 60, 127);
        note.offset_us = offset_us;
        e.start_event(&note).unwrap();
        render(&mut e, 6000)
    };
    let smooth = TAU_OVER(period) * 1.1;
    // 25 frames in: the sine's peak.
    let offset = play(Some(1000), 25 * 1_000_000 / 48000 + 1);
    assert!(offset[0][0].abs() < 0.2, "starts at {}", offset[0][0]);
    assert!(max_step(&offset) < smooth, "start step {}", max_step(&offset));
    let whole = play(None, 0);
    assert!(max_step(&whole) < smooth, "end step {}", max_step(&whole));
}

/// Changing the host block size mid-session changes nothing audible, and a
/// sample-rate change keeps pitch and envelope times.
#[test]
fn block_size_and_sample_rate_changes_keep_playback_intact() {
    let bank = || {
        let zone = Zone {
            loop_range: Some(Loop {
                start: 0,
                end: 4800,
                until_release: false,
                crossfade: 0,
            }),
            ..Zone::default()
        };
        Bank::from_samples(
            vec![Group::default()],
            vec![zone],
            vec![(PathBuf::new(), sine(100.0, 4800))],
        )
        .unwrap()
    };
    let (mut a, mut b) = (engine_with(bank()), engine_with(bank()));
    a.note_on(0, 60, 127);
    b.note_on(0, 60, 127);
    let steady = render(&mut a, 9000);
    let mut varied = Vec::new();
    for n in [1, 7, 128, 300, 1000, 64, 5000, 1500].into_iter().cycle() {
        if varied.len() >= 9000 {
            break;
        }
        varied.extend(render(&mut b, n.min(9000 - varied.len())));
    }
    let worst = steady
        .iter()
        .zip(&varied)
        .map(|(x, y)| (x[0] - y[0]).abs())
        .fold(0.0, f32::max);
    assert!(worst < 1e-4, "block size changed the output by {worst}");
    // At 96 kHz the 48 kHz sine's period doubles.
    b.reset(96000.0);
    b.note_on(0, 60, 127);
    let out = render(&mut b, 4000);
    let rising = |o: &[Frame]| {
        o.windows(2)
            .filter(|w| w[0][0] < 0.0 && w[1][0] >= 0.0)
            .count()
    };
    assert_eq!(rising(&out[100..]), 19, "3900 frames of a 200-frame period");
}

/// Tiny samples and loops shorter than the interpolator's four taps play
/// finitely; reversed short samples too.
#[test]
fn tiny_samples_and_loops_play_finitely() {
    for (frames, looped, reverse) in [
        (1, None, false),
        (2, Some((0, 1)), false),
        (3, Some((1, 3)), false),
        (5, Some((2, 4)), true),
        (4, None, true),
    ] {
        let zone = Zone {
            loop_range: looped.map(|(start, end)| Loop {
                start,
                end,
                until_release: false,
                crossfade: 0,
            }),
            ..Zone::default()
        };
        let group = Group {
            reverse,
            ..Group::default()
        };
        let bank = Bank::from_samples(
            vec![group],
            vec![zone],
            vec![(PathBuf::new(), constant([0.5, -0.5], frames))],
        )
        .unwrap();
        let mut e = engine_with(bank);
        e.note_on(0, 60, 127);
        let out = render(&mut e, 2000);
        assert!(out.iter().all(|f| f[0].is_finite() && f[1].is_finite()));
        if looped.is_some() && !reverse {
            assert!((out[1500][0] - 0.5).abs() < 1e-3, "{frames}-frame loop holds its level");
        }
    }
    let empty = Bank::from_samples(
        vec![Group::default()],
        vec![Zone::default()],
        vec![(PathBuf::new(), constant([0.5; 2], 0))],
    );
    assert!(empty.is_err(), "a zero-length sample has nothing to play");
}

/// A sample recorded at 96 kHz plays at its own pitch at 48 kHz.
#[test]
fn samples_at_other_rates_keep_their_pitch() {
    let mut sample = sine(200.0, 48000);
    sample.rate = 96000;
    let bank = Bank::from_samples(vec![Group::default()], vec![Zone::default()], vec![(PathBuf::new(), sample)]).unwrap();
    let mut e = engine_with(bank);
    e.note_on(0, 60, 127);
    let out = render(&mut e, 4000);
    let rising = out[100..]
        .windows(2)
        .filter(|w| w[0][0] < 0.0 && w[1][0] >= 0.0)
        .count();
    assert_eq!(rising, 38, "a 200-frame period at 96 kHz is 100 frames at 48 kHz");
}

/// Scripted NaN or infinite gains and pans silence their voice instead of
/// poisoning the mix; other voices keep playing.
#[test]
fn non_finite_script_values_never_reach_the_output() {
    let mut e = engine();
    e.note_on(0, 60, 127);
    let bad = e.start_event(&NoteEvent::new(0, 62, 127)).unwrap();
    e.change_event(bad, EventChange::Pan(f32::NAN));
    render(&mut e, 64);
    assert_eq!(last(&mut e, 64), [0.5, 0.25], "only the NaN voice is silent");
    e.change_event(bad, EventChange::Pan(0.0));
    e.change_event(bad, EventChange::Volume(f32::INFINITY));
    let out = render(&mut e, 256);
    assert!(out.iter().flatten().all(|x| x.is_finite()));
    assert_eq!(out[255], [0.5, 0.25]);
}

/// A decaying tail (here the Tone low-pass after the note) flushes to zero
/// instead of crawling through subnormals, which cost many times more per
/// sample on x86.
#[test]
fn decaying_tails_flush_denormals_to_zero() {
    let mut e = engine();
    e.cutoff = 100.0;
    e.note_on(0, 60, 127);
    render(&mut e, 1000);
    e.note_off(0, 60);
    let out = render(&mut e, 20000);
    let subnormal = out.iter().flatten().filter(|x| x.is_subnormal()).count();
    assert_eq!(subnormal, 0);
}

#[test]
fn overload_fades_released_voices_quietest_first() {
    // Every key plays a loud group and one at −70 dB; releases ring on.
    let quiet = Group {
        gain: 3e-4,
        ..Group::default()
    };
    let mut e = engine_with(layered(vec![Group::default(), quiet], &[0.5, 0.5]));
    e.release = 10.0;
    e.note_on(0, 60, 127);
    e.note_on(0, 62, 127);
    render(&mut e, 480);
    e.note_off(0, 60);
    render(&mut e, 480);
    assert_eq!(e.active_voices(), 4, "no load: releases ring");
    e.load = 0.85;
    render(&mut e, 64);
    assert_eq!(e.active_voices(), 4, "fading, not cut");
    render(&mut e, 480);
    assert_eq!(e.active_voices(), 3, "the inaudible release ended");
    e.load = 1.0;
    render(&mut e, 480);
    assert_eq!(e.active_voices(), 2, "near the deadline the quietest release goes too");
    e.load = 0.0;
    render(&mut e, 4800);
    assert_eq!(e.active_voices(), 2, "held notes are never shed");
}

/// Voices that resample alike (same step and position fraction) sum in one
/// lane before interpolating. Only the rounding of the sums moves: layered,
/// panned, pitched and releasing voices stay within -120 dBFS of rendering
/// each alone.
#[test]
fn lanes_match_voices_rendered_alone_within_120_db() {
    let noise = |seed: u32, bits: i32| {
        let mut x = seed;
        let scale = 2f32.powi(bits - 1);
        let frames = (0..20_000)
            .map(|_| {
                let mut next = || {
                    x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    ((x >> 8) as f32 / (1 << 23) as f32 - 1.0) * 0.99
                };
                [next(), next()].map(|v| (v * scale).round() / scale)
            })
            .collect();
        Sample { rate: 48000, frames }
    };
    let play = |lanes: bool| {
        let groups: Vec<Group> = (0..8)
            .map(|g| Group { gain: 0.02 + 0.01 * g as f32, pan: g as f32 / 8.0 - 0.4, ..Group::default() })
            .collect();
        let zones = (0..8)
            .map(|g| Zone {
                group: g,
                sample: PathBuf::from(g.to_string()),
                loop_range: Some(Loop { start: 1000, end: 19_000, until_release: false, crossfade: 300 }),
                ..Zone::default()
            })
            .collect();
        // 16-bit, 24-bit and float storage.
        let samples = (0..8).map(|g| (PathBuf::from(g.to_string()), noise(g as u32 + 1, [16, 24, 32][g % 3]))).collect();
        let mut e = engine_with(Bank::from_samples(groups, zones, samples).unwrap());
        (e.attack, e.release) = (0.005, 0.25);
        e.set_lanes(lanes);
        // On the root (whole steps), pitched, and an octave up.
        for (note, velocity) in [(60, 100), (55, 90), (72, 127), (61, 60)] {
            e.note_on(0, note, velocity);
            render(&mut e, 1234);
        }
        let mut out = render(&mut e, 9000);
        e.note_off(0, 60);
        e.note_off(0, 55);
        out.extend(render(&mut e, 6000));
        e.note_off(0, 72);
        e.note_off(0, 61);
        out.extend(render(&mut e, 12_000));
        out
    };
    let (alone, laned) = (play(false), play(true));
    let peak = alone.iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
    let error = alone.iter().flatten().zip(laned.iter().flatten()).fold(0f32, |e, (a, b)| e.max((a - b).abs()));
    assert!((0.5..=1.0).contains(&peak), "near full scale: {peak}");
    assert!(error > 0.0, "lanes were taken");
    assert!(error < 1e-6, "{:.1} dB", 20.0 * error.log10());
}

/// Voices whose group filters are held at equal settings, of any group,
/// share one filter run on their sum, each voice's own state moving on
/// alongside; lanes of them also resample once. EQs, low passes, Stereo
/// Modellers, flat bands, velocity- and controller-driven knobs (moved
/// mid-note), attacks and releases stay within -120 dBFS of filtering each
/// voice alone.
#[test]
fn shared_filters_match_voices_filtered_alone_within_120_db() {
    use fx::{Chain, Effect, Kind, Params, params};
    let noise = |seed: u32| {
        let mut x = seed;
        let frames = (0..20_000)
            .map(|_| {
                let mut next = || {
                    x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    ((x >> 8) as f32 / (1 << 23) as f32 - 1.0) * 0.99
                };
                [next(), next()]
            })
            .collect();
        Sample { rate: 48000, frames }
    };
    let effect = |slot: usize, params: Params| Effect {
        slot,
        kind: Kind::Filter,
        version: 0,
        bypass: false,
        output_gain: 1.0,
        dry_level: 0.0,
        params,
    };
    let band = |freq_hz, gain_db| params::EqBand { freq_hz, bandwidth_oct: 1.0, gain_db };
    let eq = |bands| Params::Eq(params::Eq { bands });
    let low = |cutoff| Params::Filter(params::Filter { filter_type: 5, cutoff, resonance: 0.3 });
    let knob = |source, param: &str, slot| ModAssignment {
        name: String::new(),
        source,
        target: ModTarget::Module { param: param.into(), slot },
        intensity: 0.3,
        invert: false,
        lag_ms: 20,
        shaper: None,
    };
    let play = |lanes: bool| {
        let chains = [
            // Equal EQs and modellers across groups; one with a flat middle band.
            vec![effect(0, eq(vec![band(400.0, -6.0), band(3000.0, 4.0)]))],
            vec![effect(0, eq(vec![band(400.0, -6.0), band(3000.0, 4.0)]))],
            vec![effect(0, eq(vec![band(400.0, -6.0), band(1000.0, 0.0), band(3000.0, 4.0)]))],
            vec![
                effect(0, eq(vec![band(400.0, -6.0), band(3000.0, 4.0)])),
                effect(1, Params::StereoModeller(params::StereoModeller { spread: -0.4, pan: 0.3, pseudo_stereo: false })),
            ],
            vec![effect(0, low(0.6))],
            vec![effect(0, low(0.5)), effect(1, eq(vec![band(200.0, 3.0)]))],
            vec![effect(0, eq(vec![band(800.0, -9.0)]))],
            vec![],
        ];
        let mods = |g: usize| match g {
            4 => vec![knob(ModSource::Velocity, "filterCutoff", 0)],
            6 => vec![knob(ModSource::MidiCc(1), "eqGain1", 0)],
            _ => vec![],
        };
        let groups: Vec<Group> = chains
            .into_iter()
            .enumerate()
            .map(|(g, slots)| Group {
                gain: 0.02 + 0.01 * g as f32,
                pan: g as f32 / 8.0 - 0.4,
                fx: Chain { slots },
                mods: mods(g),
                ..Group::default()
            })
            .collect();
        let zones = (0..8)
            .map(|g| Zone {
                group: g,
                sample: PathBuf::from((g % 4).to_string()),
                loop_range: Some(Loop { start: 1000, end: 19_000, until_release: false, crossfade: 300 }),
                ..Zone::default()
            })
            .collect();
        let samples = (0..4).map(|s| (PathBuf::from(s.to_string()), noise(s as u32 + 1))).collect();
        let mut e = engine_with(Bank::from_samples(groups, zones, samples).unwrap());
        (e.attack, e.release) = (0.005, 0.25);
        e.set_lanes(lanes);
        e.cc(0, 1, 30);
        // On the root (whole steps), pitched, an octave up, and one note twice.
        for (note, velocity) in [(60, 100), (55, 90), (72, 127), (61, 60), (60, 70)] {
            e.note_on(0, note, velocity);
            render(&mut e, 1234);
        }
        let mut out = render(&mut e, 5000);
        // The controller glides one filter's knob for every voice at once.
        e.cc(0, 1, 110);
        out.extend(render(&mut e, 4000));
        e.note_off(0, 60);
        e.note_off(0, 55);
        out.extend(render(&mut e, 6000));
        e.note_off(0, 72);
        e.note_off(0, 61);
        out.extend(render(&mut e, 12_000));
        out
    };
    let (alone, shared) = (play(false), play(true));
    let peak = alone.iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
    let error = alone.iter().flatten().zip(shared.iter().flatten()).fold(0f32, |e, (a, b)| e.max((a - b).abs()));
    assert!((0.5..=1.0).contains(&peak), "near full scale: {peak}");
    assert!(error > 0.0, "filters were shared");
    assert!(error < 1e-6, "{:.1} dB", 20.0 * error.log10());
}

/// Two groups whose output taps send slot 0 (a Reverb) through a Send
/// Levels slot at insert 7, as Audio Imperia and Solo store them.
fn reverb_send(script: &str) -> (Engine, Box<Runtime>) {
    use fx::{Chain, Effect, Kind, Params, params};
    let effect = |slot, kind, params| Effect { slot, kind, version: 0, bypass: false, output_gain: 1.0, dry_level: 0.0, params };
    let mut i = two_groups();
    let reverb = params::Reverb::DEFAULT;
    let levels = params::SendLevels { sends: vec![1.0; 8], outputs: Vec::new() };
    i.fx.insert = Chain { slots: vec![effect(7, Kind::SendLevels, Params::SendLevels(levels))] };
    i.fx.send = Chain { slots: vec![effect(0, Kind::Reverb, Params::Reverb(reverb))] };
    i.scripts = vec![script.to_owned()];
    let (rt, errors) = load_scripts(&i, Vec::new(), 48000.0);
    assert!(errors.is_empty(), "{errors:?}");
    let mut e = engine_with(layered(i.groups.clone(), &[0.1, 0.2]));
    e.set_fx(effects(&i, rt.as_deref(), e.rate() as f32));
    (e, rt.unwrap())
}

/// Energy after the voices end: the reverb tail.
fn tail_energy(e: &mut Engine) -> f32 {
    e.note_on(0, 60, 127);
    render(e, 4800);
    e.note_off(0, 60);
    render(e, 48000).iter().skip(480).map(|f| f[0] * f[0] + f[1] * f[1]).sum()
}

/// The framework scripts' `on init`: name the effects already loaded, then
/// set the reverb. Naming the loaded type keeps the effect, reading it back
/// gives `$EFFECT_TYPE_*`, and `$ENGINE_PAR_RV2_*` and the send level reach
/// the sound: a longer reverb rings longer, a closed send leaves no tail.
#[test]
fn scripts_name_loaded_effects_and_set_the_reverb_and_send() {
    let script = |time: &str, send: &str| {
        format!(
            "on init
declare %type_ok[1]
set_engine_par($ENGINE_PAR_SEND_EFFECT_TYPE, $EFFECT_TYPE_REVERB2, -1, 0, $NI_SEND_BUS)
set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_SEND_LEVELS, -1, 7, $NI_INSERT_BUS)
set_engine_par($ENGINE_PAR_RV2_TYPE, $NI_REVERB2_TYPE_HALL, -1, 0, $NI_SEND_BUS)
set_engine_par($ENGINE_PAR_RV2_TIME, {time}, -1, 0, $NI_SEND_BUS)
set_engine_par($ENGINE_PAR_SENDLEVEL_0, {send}, -1, 7, $NI_INSERT_BUS)
{{ Out of bounds, and so a diagnostic, unless the types read back. }}
%type_ok[get_engine_par($ENGINE_PAR_SEND_EFFECT_TYPE, -1, 0, $NI_SEND_BUS) - $EFFECT_TYPE_REVERB2] := 1
%type_ok[get_engine_par($ENGINE_PAR_EFFECT_TYPE, -1, 7, $NI_INSERT_BUS) - $EFFECT_TYPE_SEND_LEVELS] := 1
%type_ok[get_engine_par($ENGINE_PAR_RV2_TYPE, -1, 0, $NI_SEND_BUS) - $NI_REVERB2_TYPE_HALL] := 1
end on"
        )
    };
    let run = |time: &str, send: &str| {
        let (mut e, rt) = reverb_send(&script(time, send));
        assert_eq!(rt.diagnostics(), Vec::<String>::new());
        assert!(e.set_script(Some(rt)).is_none());
        tail_energy(&mut e)
    };
    let (short, long, closed) = (run("0", "630859"), run("1000000", "630859"), run("1000000", "0"));
    assert!(long > 4.0 * short, "RV2_TIME: {short} vs {long}");
    assert!(closed < 1e-9, "SENDLEVEL_0 at 0 still feeds the reverb: {closed}");

    // `on init` loads other effects, built off the audio thread with the
    // rest: a Gainer in place of the reverb leaves no tail, and a Reverb
    // loaded into an empty send slot at its defaults rings, set by RV2_*.
    let swap = |load: &str| {
        let (mut e, rt) = reverb_send(&format!(
            "on init
declare %type_ok[1]
{load}
%type_ok[get_engine_par($ENGINE_PAR_SEND_EFFECT_TYPE, -1, 0, $NI_SEND_BUS) - $EFFECT_TYPE_GAINER] := 1
end on"
        ));
        assert_eq!(rt.diagnostics(), Vec::<String>::new());
        assert!(e.set_script(Some(rt)).is_none());
        tail_energy(&mut e)
    };
    let gainer = "set_engine_par($ENGINE_PAR_SEND_EFFECT_TYPE, $EFFECT_TYPE_GAINER, -1, 0, $NI_SEND_BUS)";
    assert!(swap(gainer) < 1e-9);
    let moved = swap(&format!(
        "{gainer}
set_engine_par($ENGINE_PAR_SEND_EFFECT_TYPE, $EFFECT_TYPE_REVERB2, -1, 1, $NI_SEND_BUS)
set_engine_par($ENGINE_PAR_RV2_TIME, 1000000, -1, 1, $NI_SEND_BUS)
set_engine_par($ENGINE_PAR_SENDLEVEL_1, 630859, -1, 7, $NI_INSERT_BUS)"
    ));
    assert!(moved > 4.0 * short, "reverb loaded into send slot 1: {moved} vs {short}");

    // While playing, another effect would allocate on the audio thread: it
    // is refused, and says so.
    let (mut e, rt) = reverb_send("on init\nend on\non note\nset_engine_par($ENGINE_PAR_SEND_EFFECT_TYPE, $EFFECT_TYPE_GAINER, -1, 0, $NI_SEND_BUS)\nend on");
    assert!(e.set_script(Some(rt)).is_none());
    e.note_on(0, 60, 127);
    render(&mut e, 480);
    let diagnostics = e.script().unwrap().diagnostics();
    assert!(diagnostics.iter().any(|d| d.contains("during on init")), "{diagnostics:?}");
}

/// `load_ir_sample` in `on init`: a name without extension or case found in
/// the library's `Resources/ir_samples` loads into an empty convolution
/// insert, and the effects then convolve with it; `on async_complete`
/// reports 1 for it and 0 for a file that is not there.
#[test]
fn load_ir_sample_fills_the_convolution_slot_and_reports_status() {
    use fx::{Chain, Effect, Kind, Params, params};
    let dir = std::env::temp_dir().join(format!("kontakto-ir-{}", std::process::id()));
    let irs = dir.join("Resources").join("IR_Samples");
    std::fs::create_dir_all(&irs).unwrap();
    write_wav(&irs.join("Room.WAV"), 4410);
    let band = params::IrBand { length_ratio: 1.0, low_cut_hz: 20.0, high_cut_hz: 20_000.0 };
    let conv = params::Convolution {
        unknown: [0.0; 2],
        predelay_ms: 0.0,
        early: band,
        late: band,
        unknown_9: 0.0,
        flags: [false; 5],
        curve_x: Vec::new(),
        curve_db: Vec::new(),
        ir_index: -1,
        ir_file: None,
        ir_error: None,
        ir: None,
    };
    let mut i = two_groups();
    i.path = dir.join("Instrument.nki");
    i.fx.insert = Chain {
        slots: vec![Effect {
            slot: 0,
            kind: Kind::Convolution,
            version: 0,
            bypass: false,
            output_gain: 1.0,
            dry_level: 0.0,
            params: Params::Convolution(Box::new(conv)),
        }],
    };
    i.scripts = vec!["on init
declare $found
declare $missing
declare $done
declare %ok[1]
$found := load_ir_sample(\"room\", 0, $NI_INSERT_BUS)
$missing := load_ir_sample(\"nothing\", 0, $NI_INSERT_BUS)
end on
on async_complete
{ Out of bounds, and so a diagnostic, unless the status is right. }
if ($NI_ASYNC_ID = $found)
  %ok[$NI_ASYNC_EXIT_STATUS - 1] := 1
end if
if ($NI_ASYNC_ID = $missing)
  %ok[$NI_ASYNC_EXIT_STATUS] := 1
end if
inc($done)
message($done)
end on"
        .into()];
    let tail = |with_ir: bool| {
        let (rt, errors) = load_scripts(&i, Vec::new(), 48000.0);
        assert!(errors.is_empty(), "{errors:?}");
        let rt = rt.unwrap();
        assert_eq!(rt.init_irs.len(), 1);
        let mut e = engine_with(layered(i.groups.clone(), &[0.1, 0.2]));
        e.set_fx(kontakto::engine::effects(&i, with_ir.then_some(&*rt), e.rate() as f32));
        e.set_script(Some(rt));
        let energy = tail_energy(&mut e);
        let rt = e.script().unwrap();
        assert_eq!(rt.last_message(), "2");
        let diagnostics = rt.diagnostics();
        assert!(diagnostics.iter().all(|d| !d.contains("out of bounds")), "{diagnostics:?}");
        assert!(diagnostics.iter().any(|d| d.contains("load_ir_sample: file not found")), "{diagnostics:?}");
        energy
    };
    let (dry, wet) = (tail(false), tail(true));
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(dry < 1e-9, "no impulse response, yet a tail: {dry}");
    assert!(wet > 1e-3, "the loaded impulse response leaves no tail: {wet}");
}
