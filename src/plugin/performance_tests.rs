//! A realtime host performance with independently known audio expectations.
//! CPU telemetry must preserve complete PCM and exact note-end timing.
use super::*;
use crate::{
    audio::Sample,
    import::{Group, Loop, Zone},
};
use moose::params::Params;
use serde_json::{Value, json};

mod relative;
mod native_gain;
mod vista_pedal_tests;
mod mpe_cleanup;
mod event_ids;
mod mixed_protocol;
mod port_replacement;
mod program_change;

const RATE: usize = 48_000;
const WINDOW: usize = 12_800; // 64 complete periods of the 240 Hz source.
const SETTLE: usize = 4_800; // Twice the configured 50 ms release.

struct Performance {
    dsp: Dsp,
    params: SamplerParams,
    partition: &'static [usize],
    process_mode: moose::core::ProcessMode,
    reported_loads: &'static [f32],
    block: usize,
    pcm: Vec<[f32; 2]>,
    stages: Vec<Value>,
    input: Vec<Value>,
    ended: Vec<ExactNoteAddress>,
    ended_frames: Vec<usize>,
}

impl Performance {
    fn new(scripted: bool, partition: &'static [usize]) -> Self {
        Self::with_group(scripted, partition, Group::default())
    }

    fn with_group(scripted: bool, partition: &'static [usize], group: Group) -> Self {
        let params = SamplerParams::new();
        params.attack.set_value(0.0001);
        params.release.set_value(0.05);
        params.snap_smoothers();
        let mut dsp = Dsp { until_poll: usize::MAX, ..Dsp::default() };
        let engine = dsp.rack.parts[0].instrument_mut().unwrap();
        engine.reset(RATE as f64);
        let frames = (0..8_000)
            .map(|i| {
                let phase = std::f64::consts::TAU * i as f64 / 200.;
                [(0.02 * phase.sin() + 0.006 * (2. * phase).sin()) as f32; 2]
            })
            .collect();
        let zone = Zone {
            root: 60,
            loop_range: Some(Loop {
                start: 0,
                end: 8_000,
                crossfade: 0,
                until_release: false,
                alternating: false,
            }),
            ..Zone::default()
        };
        engine.set_bank(Some(Box::new(
            Bank::from_samples(
                vec![group],
                vec![zone],
                vec![(
                    PathBuf::new(),
                    Sample {
                        rate: RATE as u32,
                        frames,
                    },
                )],
            )
            .unwrap(),
        )));
        if scripted {
            let (runtime, errors) = Runtime::with_scripts(
                &["on note\nend on\non release\nend on"],
                &mut crate::ksp::LogEngine::default(),
                0,
                Vec::new(),
            );
            assert!(errors.iter().all(Option::is_none), "{errors:?}");
            engine.set_script(Some(Box::new(runtime)));
        }
        Self {
            dsp,
            params,
            partition,
            process_mode: moose::core::ProcessMode::Realtime,
            reported_loads: &[],
            block: 0,
            pcm: Vec::new(),
            stages: Vec::new(),
            input: Vec::new(),
            ended: Vec::new(),
            ended_frames: Vec::new(),
        }
    }

    fn stage(&mut self, name: &str, expected: &str, events: &EventList) -> [f64; 2] {
        self.stage_for(name, expected, events, SETTLE + WINDOW, SETTLE)
    }

    fn stage_for(&mut self, name: &str, expected: &str, events: &EventList, frames: usize, settle: usize) -> [f64; 2] {
        assert!(settle < frames);
        let start = self.pcm.len();
        for event in events.lossless_iter() {
            self.input
                .push(json!({ "frame": start, "stage": name, "event": format!("{event:?}") }));
        }
        let mut remaining = frames;
        let mut first = true;
        let empty = EventList::with_capacity(0);
        let mut output = EventList::with_capacity(128);
        let transport = TransportInfo::default();
        while remaining != 0 {
            let len = self.partition[self.block % self.partition.len()].min(remaining);
            if !self.reported_loads.is_empty() {
                self.dsp.rack.parts[0].instrument_mut().unwrap().load = self.reported_loads[self.block % self.reported_loads.len()];
            }
            self.block += 1;
            let incoming = if first { events } else { &empty };
            // This is the format-wrapper contract: CLAP's apply_pending_events
            // writes plain parameter events before entering this DSP sub-block.
            // Sampler::process alone does not perform this wrapper step.
            for event in incoming.iter() {
                if let EventBody::ParamChange { id, value } = event.body {
                    self.params.set_plain(id, value);
                }
            }
            let (mut left, mut right) = ([0.; 128], [0.; 128]);
            let mut channels = [&mut left[..len], &mut right[..len]];
            let mut buffer = AudioBuffer::from_slices_checked(&[], &mut channels, len);
            let mut cx = ProcessContext::new(&transport, RATE as f64, len, &mut output)
                .with_process_mode(self.process_mode);
            assert_eq!(
                super::tests::allocations(|| {
                    Sampler::process(&mut self.dsp, &self.params, &mut buffer, incoming, &mut cx);
                }),
                0,
                "{name}: process allocated or freed"
            );
            for event in output.lossless_iter() {
                if let LosslessEventRef::Exact(event) = event
                    && let ExactEventBody::Note {
                        kind: ExactNoteKind::End,
                        address,
                        ..
                    } = *event.body()
                {
                    assert!(
                        !self.ended.contains(&address),
                        "duplicate NOTE_END for {address:?}"
                    );
                    self.ended.push(address);
                    self.ended_frames.push(self.pcm.len() + event.sample_offset() as usize);
                }
            }
            output.clear();
            self.pcm
                .extend(left[..len].iter().zip(&right[..len]).map(|(&l, &r)| [l, r]));
            remaining -= len;
            first = false;
        }
        let measured = &self.pcm[start + settle..];
        let rms = rms(measured);
        self.stages.push(
            json!({ "name": name, "expected": expected, "start_frame": start,
            "measured_start_frame": start + settle, "end_frame": self.pcm.len(), "rms": rms,
            "spectral_amplitude": ([240.,480.,960.,1920.,3840.,7680.].map(|hz| json!({
                "hz": hz, "left": amplitude(measured,0,hz), "right": amplitude(measured,1,hz) }))),
            "note_ends_so_far": self.ended.iter().map(|a| format!("{a:?}")).collect::<Vec<_>>() }),
        );
        eprintln!(
            "{name}: RMS L={:.8} R={:.8}; exact ends={}",
            rms[0],
            rms[1],
            self.ended.len()
        );
        rms
    }

    fn measured(&self) -> &[[f32; 2]] {
        &self.pcm[self.pcm.len() - WINDOW..]
    }

    fn export(&self, name: &str) {
        let Some(dir) = std::env::var_os("KONTRA_PERFORMANCE_ARTIFACT_DIR") else {
            return;
        };
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: RATE as u32,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut wav = hound::WavWriter::create(dir.join(format!("{name}.wav")), spec).unwrap();
        for frame in &self.pcm {
            for sample in frame {
                wav.write_sample(*sample).unwrap();
            }
        }
        wav.finalize().unwrap();
        std::fs::write(dir.join(format!("{name}.json")), serde_json::to_vec_pretty(&json!({
            "sample_rate": RATE, "render_mode": "realtime; resident source, CPU telemetry does not alter playback",
            "reported_load_override_cycle": self.reported_loads,
            "source": "240 Hz sine at 0.02 + 480 Hz harmonic at 0.006; continuous loop",
            "master_db_initial": -12, "partition": self.partition, "input": self.input,
            "stages": self.stages,
            "note_ends": self.ended.iter().zip(&self.ended_frames).map(|(address, frame)|
                json!({ "address": format!("{address:?}"), "frame": frame })).collect::<Vec<_>>(),
            "parameter_contract": "ParamChange applied through Params::set_plain before Sampler::process, matching CLAP wrapper; native wrapper not instantiated",
        })).unwrap()).unwrap();
    }
}

fn rms(frames: &[[f32; 2]]) -> [f64; 2] {
    [0, 1].map(|ch| {
        (frames.iter().map(|f| f64::from(f[ch]).powi(2)).sum::<f64>() / frames.len() as f64).sqrt()
    })
}

fn amplitude(frames: &[[f32; 2]], ch: usize, hz: f64) -> f64 {
    let (mut re, mut im) = (0., 0.);
    for (i, frame) in frames.iter().enumerate() {
        let angle = std::f64::consts::TAU * hz * i as f64 / RATE as f64;
        re += f64::from(frame[ch]) * angle.cos();
        im += f64::from(frame[ch]) * angle.sin();
    }
    2. * re.hypot(im) / frames.len() as f64
}

fn address(id: i32, channel: i16, key: i16) -> ExactNoteAddress {
    ExactNoteAddress::from_raw_signed(0, channel, key, id)
}

fn note(events: &mut EventList, kind: ExactNoteKind, id: i32, channel: i16, key: i16) {
    events
        .try_push_exact(ExactEvent::new(
            0,
            ExactEventBody::Note {
                kind,
                address: address(id, channel, key),
                velocity: 1.,
            },
        ))
        .unwrap();
}

fn expression(events: &mut EventList, id: i32, channel: i16, expression_id: i32, value: f64) {
    events
        .try_push_exact(ExactEvent::new(
            0,
            ExactEventBody::NoteExpression {
                expression_id,
                address: address(id, channel, 60),
                value,
            },
        ))
        .unwrap();
}

fn cc(events: &mut EventList, channel: u8, controller: u8, value: u8) {
    events.push(Event::on_port(
        0,
        0,
        EventBody::ControlChange {
            group: 0,
            channel,
            cc: controller,
            value,
        },
    ));
}

fn ratio(actual: f64, expected: f64, reason: &str) {
    assert!(
        (actual - expected).abs() < 0.003,
        "{reason}: expected {expected}, observed {actual}"
    );
}

fn chord(scripted: bool, partition: &'static [usize], reported_loads: &'static [f32]) -> Performance {
    let mut p = Performance::new(scripted, partition);
    p.reported_loads = reported_loads;
    let mut events = EventList::with_capacity(32);
    // Four identical, phase-aligned owners per octave. Dropping three per
    // pitch divides every frequency component (and RMS) by exactly four.
    for pitch in 0..5 {
        for owner in 0..4 {
            note(
                &mut events,
                ExactNoteKind::On,
                pitch * 4 + owner,
                if owner % 2 == 0 { 4 } else { 7 },
                60 + 12 * pitch as i16,
            );
        }
    }
    let twenty = p.stage(
        "20 voices",
        "five octaves, four exact owners each; equal stereo RMS",
        &events,
    );
    events.clear();
    for pitch in 0..5 {
        for owner in 0..3 {
            note(
                &mut events,
                ExactNoteKind::Off,
                pitch * 4 + owner,
                if owner % 2 == 0 { 4 } else { 7 },
                60 + 12 * pitch as i16,
            );
        }
    }
    let five = p.stage(
        "release 15, hold 5",
        "RMS is one quarter of 20 voices after the release tails",
        &events,
    );
    let ends_after_15 = p.ended.len();
    events.clear();
    cc(&mut events, 4, 64, 127);
    cc(&mut events, 7, 64, 127);
    p.stage(
        "sustain down",
        "five held owners continue sounding",
        &events,
    );
    events.clear();
    for pitch in 0..5 {
        note(
            &mut events,
            ExactNoteKind::Off,
            pitch * 4 + 3,
            7,
            60 + 12 * pitch as i16,
        );
    }
    let sustained = p.stage(
        "release held 5 under sustain",
        "same RMS; no extra NOTE_END while sustain holds",
        &events,
    );
    let ends_under_sustain = p.ended.len();
    events.clear();
    cc(&mut events, 4, 64, 0);
    cc(&mut events, 7, 64, 0);
    let released = p.stage(
        "sustain up",
        "silence after 50 ms release; all 20 exact owners end",
        &events,
    );
    events.clear();
    note(&mut events, ExactNoteKind::On, 100, 4, 60);
    p.stage("new note before stop", "a new owner is audible", &events);
    events.clear();
    note(&mut events, ExactNoteKind::Choke, 100, 4, 60);
    let stopped = p.stage("stop", "silence after choke; owner 100 ends once", &events);
    events.clear();
    expression(&mut events, 100, 4, 0, 4.);
    expression(&mut events, 100, 4, 2, 12.);
    events.push(Event::new(
        0,
        EventBody::ParamChange {
            id: p.params.volume.id(),
            value: 6.,
        },
    ));
    cc(&mut events, 4, 11, 127);
    cc(&mut events, 7, 64, 127);
    let late = p.stage(
        "late automation and expression",
        "ended owners never resurrect even at +6 dB",
        &events,
    );
    let mut name = format!(
        "chord-{}-{}",
        if scripted { "script" } else { "dry" },
        if partition.len() == 1 {
            "128"
        } else {
            "irregular"
        }
    );
    if !reported_loads.is_empty() {
        name.push_str(if reported_loads == [0.] { "-load-zero" } else { "-load-varies" });
    }
    p.export(&name);
    assert!(
        twenty[0] > 0.01,
        "the known source chord must actually sound"
    );
    ratio(twenty[1] / twenty[0], 1., "stereo symmetry");
    ratio(five[0] / twenty[0], 0.25, "15 released voices leave five");
    ratio(
        sustained[0] / five[0],
        1.,
        "sustain preserves the remaining five",
    );
    assert_eq!(ends_after_15, 15);
    assert_eq!(ends_under_sustain, 15);
    assert_eq!(p.ended.len(), 21);
    for pitch in 0..5 {
        for owner in 0..4 {
            assert!(p.ended.contains(&address(
                pitch * 4 + owner,
                if owner % 2 == 0 { 4 } else { 7 },
                60 + 12 * pitch as i16
            )));
        }
    }
    for (name, value) in [
        ("released", released),
        ("stopped", stopped),
        ("late events", late),
    ] {
        assert!(
            value.iter().all(|r| *r < 1e-8),
            "{name} should be silent: {value:?}"
        );
    }
    p
}

#[test]
fn twenty_notes_release_fifteen_sustain_five_and_stop_through_plugin_pcm() {
    for scripted in [false, true] {
        let fixed = chord(scripted, &[128], &[]);
        let irregular = chord(scripted, &[1, 7, 31, 127, 13, 64], &[]);
        let max = fixed
            .pcm
            .iter()
            .zip(&irregular.pcm)
            .flat_map(|(a, b)| [(a[0] - b[0]).abs(), (a[1] - b[1]).abs()])
            .fold(0f32, f32::max);
        eprintln!("scripted={scripted}: fixed/irregular maximum PCM difference={max}");
        assert!(
            max < 2e-5,
            "host block partition changed this performance by {max}"
        );
    }
}

#[test]
fn reported_cpu_load_preserves_complete_plugin_performance_and_note_ends() {
    for scripted in [false, true] {
        let reference = chord(scripted, &[128], &[0.]);
        let loaded = chord(scripted, &[128], &[0.69, 0.7, 0.85, 0.9, 1., 2., 0.]);
        assert!(reference.pcm == loaded.pcm,
            "CPU telemetry changed the complete performance, scripted={scripted}");
        assert_eq!(reference.ended, loaded.ended,
            "CPU telemetry changed exact note retirement");
        assert_eq!(reference.ended_frames, loaded.ended_frames,
            "CPU telemetry changed the timing of exact note ends");
    }
}

fn expressions(scripted: bool, partition: &'static [usize]) -> Performance {
    let mut p = Performance::new(scripted, partition);
    let mut events = EventList::with_capacity(16);
    note(&mut events, ExactNoteKind::On, 40, 4, 60);
    note(&mut events, ExactNoteKind::On, 41, 7, 60);
    expression(&mut events, 40, 4, 1, 0.); // CLAP 0=left, 1=right.
    expression(&mut events, 41, 7, 1, 1.);
    let original = p.stage(
        "two same-pitch owners",
        "240 Hz owners independently panned left and right",
        &events,
    );
    events.clear();
    expression(&mut events, 40, 4, 2, 12.);
    p.stage(
        "tune left owner",
        "left fundamental doubles to 480 Hz; right remains 240 Hz",
        &events,
    );
    let left_480 = amplitude(p.measured(), 0, 480.);
    let left_240 = amplitude(p.measured(), 0, 240.);
    let right_240 = amplitude(p.measured(), 1, 240.);
    events.clear();
    expression(&mut events, 40, 4, 0, 0.25);
    let gain = p.stage(
        "quarter left gain",
        "left RMS divides by four; right RMS unchanged",
        &events,
    );
    events.clear();
    cc(&mut events, 4, 1, 127);
    cc(&mut events, 7, 11, 0);
    cc(&mut events, 7, 74, 0);
    let controllers = p.stage(
        "unassigned library controllers",
        "unassigned CC1, CC11, CC74 do not change the dry sample amplifier",
        &events,
    );
    events.clear();
    events.push(Event::new(
        0,
        EventBody::ParamChange {
            id: p.params.volume.id(),
            value: -18.020599913279625,
        },
    ));
    let automated = p.stage(
        "host master minus 6.0206 dB",
        "both channels halve through a real ParamChange applied at the wrapper boundary",
        &events,
    );
    events.clear();
    note(&mut events, ExactNoteKind::Off, 40, 4, 60);
    note(&mut events, ExactNoteKind::On, 42, 4, 60);
    expression(&mut events, 40, 4, 0, 0.);
    expression(&mut events, 40, 4, 2, 24.);
    expression(&mut events, 42, 4, 1, 0.);
    let reused = p.stage(
        "release and reuse same channel and pitch",
        "new owner 42 keeps 240 Hz, full gain; old owner 40 expressions cannot affect it",
        &events,
    );
    let reused_240 = amplitude(p.measured(), 0, 240.);
    let reused_960 = amplitude(p.measured(), 0, 960.);
    p.export(&format!(
        "expressions-{}-{}",
        if scripted { "script" } else { "dry" },
        if partition.len() == 1 {
            "128"
        } else {
            "irregular"
        }
    ));
    assert!(
        left_480 > 0.003 && right_240 > 0.003,
        "expected fundamentals are missing"
    );
    assert!(
        left_240 < left_480 * 0.001,
        "tuning failed to remove the left 240 Hz fundamental"
    );
    ratio(gain[0] / original[0], 0.25, "exact left-owner gain");
    ratio(gain[1] / original[1], 1., "right owner isolation");
    for ch in 0..2 {
        ratio(controllers[ch] / gain[ch], 1., "unassigned library CC");
        ratio(automated[ch] / gain[ch], 0.5, "master automation");
    }
    ratio(
        reused[0] / original[0],
        0.5,
        "new same-channel owner keeps full gain under half master",
    );
    assert!(
        reused_240 > 0.001 && reused_960 < reused_240 * 0.001,
        "old expression retuned new owner"
    );
    assert_eq!(p.ended, [address(40, 4, 60)]);
    p
}

#[test]
fn same_pitch_exact_expressions_and_master_automation_have_audible_scopes() {
    for scripted in [false, true] {
        let fixed = expressions(scripted, &[128]);
        let irregular = expressions(scripted, &[1, 7, 31, 127, 13, 64]);
        let max = fixed
            .pcm
            .iter()
            .zip(&irregular.pcm)
            .flat_map(|(a, b)| [(a[0] - b[0]).abs(), (a[1] - b[1]).abs()])
            .fold(0f32, f32::max);
        eprintln!("expressions scripted={scripted}: fixed/irregular maximum PCM difference={max}");
        assert!(
            max < 2e-5,
            "host block partition changed expression ramps by {max}"
        );
    }
}

#[test]
fn normalized_onset_velocities_keep_independent_audible_precision() {
    use crate::modulation::{ModAssignment, ModSource, ModTarget};
    let velocities = [0.5001_f64, 0.5030];
    assert_eq!((velocities[0] * 127.).round(), (velocities[1] * 127.).round());
    let precise_note = |events: &mut EventList, id, channel, velocity| {
        events.try_push_exact(ExactEvent::new(0, ExactEventBody::Note {
            kind: ExactNoteKind::On, address: address(id, channel, 60), velocity,
        })).unwrap();
    };
    for scripted in [false, true] {
        let group = Group {
            mods: vec![ModAssignment { cc_default: None, name: "LINEAR_VELOCITY_VOLUME".into(),
                source: ModSource::Velocity, target: ModTarget::Volume, intensity: 1.,
                invert: false, lag_ms: 0, shaper: None }],
            ..Group::default()
        };
        let mut p = Performance::with_group(scripted, &[128], group);
        let mut events = EventList::with_capacity(16);
        precise_note(&mut events, 90, 4, 1.);
        precise_note(&mut events, 91, 7, 1.);
        expression(&mut events, 90, 4, 1, 0.);
        expression(&mut events, 91, 7, 1, 1.);
        let full = p.stage("full velocity references", "same source independently panned at unit velocity", &events);
        assert!(full.iter().all(|v| *v > 0.001), "full velocity reference must be audible");
        events.clear();
        note(&mut events, ExactNoteKind::Off, 90, 4, 60);
        note(&mut events, ExactNoteKind::Off, 91, 7, 60);
        p.stage("retire velocity references", "both references release completely", &events);
        events.clear();
        precise_note(&mut events, 100, 4, velocities[0]);
        precise_note(&mut events, 101, 7, velocities[1]);
        expression(&mut events, 100, 4, 1, 0.);
        expression(&mut events, 101, 7, 1, 1.);
        let actual = p.stage("two sub-byte onset velocities", "linear library modulation uses each original normalized velocity", &events);
        for ch in 0..2 {
            let gain = actual[ch] / full[ch];
            assert!((gain - velocities[ch]).abs() < 1e-4,
                "scripted={scripted}, channel={ch}: original onset velocity {} became audible gain {gain}", velocities[ch]);
        }
        assert!((actual[0] / actual[1] - velocities[0] / velocities[1]).abs() < 1e-4,
            "distinct normalized velocities sharing a MIDI1 byte must retain their independent RMS ratio");
        events.clear();
        note(&mut events, ExactNoteKind::Off, 100, 4, 60);
        note(&mut events, ExactNoteKind::Off, 101, 7, 60);
        let released = p.stage("release precise owners", "no owner survives its explicit note off", &events);
        assert!(released.iter().all(|v| *v < 1e-8));
        assert_eq!(p.ended, [address(90,4,60), address(91,7,60), address(100,4,60), address(101,7,60)]);
        events.clear();
        events.push(Event::on_port(0,0,EventBody::NoteOn2 {
            group:0, channel:4, note:60, velocity:0, attribute_type:0, attribute:0,
        }));
        let zero_modulated = p.stage("MIDI2 zero authored velocity gain", "linear velocity-volume assignment receives exactly zero", &events);
        assert!(zero_modulated.iter().all(|v| *v < 1e-8), "MIDI2 zero must not become MIDI1 velocity one");
        events.clear();
        events.push(Event::on_port(0,0,EventBody::NoteOff2 {
            group:0, channel:4, note:60, velocity:0, attribute_type:0, attribute:0,
        }));
        p.stage("release MIDI2 zero modulated voice", "the silent attack still accepts its explicit release", &events);

        // Without an authored velocity assignment, zero is a valid exact
        // onset value; it must still start a voice and have an exact lifecycle.
        let mut zero = Performance::new(scripted, &[128]);
        events.clear();
        for (id, velocity) in [(110,f64::NAN), (111,f64::INFINITY), (112,f64::NEG_INFINITY)] {
            precise_note(&mut events, id, 4, velocity);
        }
        let malformed = zero.stage("reject nonfinite onsets", "malformed exact velocities produce no PCM or note ownership", &events);
        assert!(malformed.iter().all(|v| *v < 1e-8));
        assert!(zero.ended.is_empty());
        events.clear();
        precise_note(&mut events, 113, 4, 0.);
        let sounding = zero.stage("exact zero velocity onset", "zero starts an audible unassigned-velocity voice", &events);
        assert!(sounding.iter().all(|v| *v > 0.001));
        events.clear();
        note(&mut events, ExactNoteKind::Off, 113, 4, 60);
        let silent = zero.stage("end exact zero velocity owner", "explicit note off silences the voice and emits one exact end", &events);
        assert!(silent.iter().all(|v| *v < 1e-8));
        assert_eq!(zero.ended, [address(113,4,60)]);
        events.clear();
        events.push(Event::on_port(0,0,EventBody::NoteOn2 {
            group:0, channel:4, note:60, velocity:0, attribute_type:0, attribute:0,
        }));
        let midi2_on = zero.stage("MIDI2 zero attack", "zero is a valid attack with no authored velocity-volume assignment", &events);
        assert!(midi2_on.iter().all(|v| *v > 0.001));
        events.clear();
        events.push(Event::on_port(0,0,EventBody::NoteOff2 {
            group:0, channel:4, note:60, velocity:0, attribute_type:0, attribute:0,
        }));
        let midi2_off = zero.stage("MIDI2 zero explicit release", "the matching MIDI2 off ends the audible zero-velocity attack", &events);
        assert!(midi2_off.iter().all(|v| *v < 1e-8));
        events.clear();
        events.push(Event::on_port(0,0,EventBody::NoteOn { group:0,channel:4,note:60,velocity:100 }));
        let midi1_on = zero.stage("MIDI1 attack reference", "ordinary nonzero MIDI1 attack is audible", &events);
        assert!(midi1_on.iter().all(|v| *v > 0.001));
        events.clear();
        events.push(Event::on_port(0,0,EventBody::NoteOn { group:0,channel:4,note:60,velocity:0 }));
        let midi1_zero = zero.stage("MIDI1 zero remains note off", "MIDI1 velocity zero ends its preceding attack", &events);
        assert!(midi1_zero.iter().all(|v| *v < 1e-8));
    }
}

fn release_performance(partition: &'static [usize], script: Option<&str>) -> Performance {
    use crate::modulation::{ModAssignment, ModSource, ModTarget};
    let mut p = Performance::new(false, partition);
    let attack_path = PathBuf::from("attack-240");
    let release_path = PathBuf::from("release-960");
    let samples = [(attack_path.clone(), 8_000, 240.), (release_path.clone(), 18_000, 960.)]
        .into_iter().map(|(path, len, hz)| (path, Sample { rate: RATE as u32,
            frames: (0..len).map(|i| [(0.02 * (std::f64::consts::TAU * hz * i as f64 / RATE as f64).sin()) as f32; 2]).collect(),
        })).collect();
    let groups = vec![Group::default(), Group { release_trigger: true,
        mods: vec![ModAssignment { cc_default: None, name:"LINEAR_RELEASE_VELOCITY_VOLUME".into(),
            source:ModSource::ReleaseVelocity, target:ModTarget::Volume, intensity:1.,
            invert:false, lag_ms:0, shaper:None }], ..Group::default() }];
    let zones = vec![Zone { group:0, root:60, sample:attack_path,
        loop_range:Some(Loop { start:0,end:8_000,crossfade:0,until_release:false,alternating:false }),
        ..Zone::default() }, Zone { group:1,root:60,sample:release_path,..Zone::default() }];
    p.dsp.rack.parts[0].instrument_mut().unwrap().set_bank(Some(Box::new(Bank::from_samples(groups,zones,samples).unwrap())));
    if let Some(script) = script {
        let (rt, errors) = Runtime::with_scripts(&[script], &mut crate::ksp::LogEngine::new(Vec::new(),RATE as f64),2,Vec::new());
        assert!(errors.iter().all(Option::is_none),"{errors:?}");
        p.dsp.rack.parts[0].instrument_mut().unwrap().set_script(Some(Box::new(rt)));
    }
    p
}

fn precise_release_pair(partition: &'static [usize], script: Option<&str>, velocities: [f64;2]) -> (Performance,[f64;2]) {
    let mut p = release_performance(partition,script);
    let mut events = EventList::with_capacity(16);
    for (id,channel,onset,pan) in [(120,4,0.71,0.),(121,7,0.93,1.)] {
        events.try_push_exact(ExactEvent::new(0,ExactEventBody::Note { kind:ExactNoteKind::On,
            address:address(id,channel,60),velocity:onset })).unwrap();
        expression(&mut events,id,channel,1,pan);
        cc(&mut events,channel as u8,64,127);
    }
    let attack = p.stage("release owners under sustain","two same-key independent attack owners",&events);
    assert!(attack.iter().all(|v| *v > 0.001));
    events.clear();
    for (index,(id,channel)) in [(120,4),(121,7)].into_iter().enumerate() {
        events.try_push_exact(ExactEvent::new(0,ExactEventBody::Note { kind:ExactNoteKind::Off,
            address:address(id,channel,60),velocity:velocities[index] })).unwrap();
        // Subsequent releases and controllers cannot replace the physical
        // release snapshot while pedals or script waits defer its sound.
        events.try_push_exact(ExactEvent::new(0,ExactEventBody::Note { kind:ExactNoteKind::Off,
            address:address(id,channel,60),velocity:0.99 })).unwrap();
        cc(&mut events,channel as u8,1,127);
        cc(&mut events,channel as u8,11,0);
    }
    p.stage("deferred physical releases","sustain holds attack; no release sample starts yet",&events);
    for ch in 0..2 {
        assert!(amplitude(p.measured(),ch,960.) < 1e-8,"release sample escaped sustain");
    }
    assert!(p.ended.is_empty(),"physical off under sustain cannot retire an audible owner");
    events.clear();
    cc(&mut events,4,64,0);
    cc(&mut events,7,64,0);
    let released = p.stage("precise release samples after pedal up","960 Hz release PCM keeps each owner's original release velocity",&events);
    assert!(released.iter().all(|v| *v > 0.001));
    assert!(p.ended.is_empty(),"exact owners remain live throughout their release samples");
    events.clear();
    let ended = p.stage("release sample completion","one exact end per owner after its sample ends",&events);
    assert!(ended.iter().all(|v| *v < 1e-8));
    assert_eq!(p.ended,[address(120,4,60),address(121,7,60)]);
    (p,released)
}

#[test]
fn precise_release_velocity_survives_pedals_duplicate_offs_and_block_fragments() {
    let velocities = [0.5001_f64,0.5002];
    assert_eq!((velocities[0]*127.).round(),(velocities[1]*127.).round());
    for script in [None,Some("on note\nend on\non release\nwait(40000)\nend on")] {
        let (_,reference) = precise_release_pair(&[128],script,[1.,1.]);
        let (fixed,actual) = precise_release_pair(&[128],script,velocities);
        let (fragments,fragmented) = precise_release_pair(&[1,7,31,127,13,64],script,velocities);
        for ch in 0..2 {
            assert!((actual[ch]/reference[ch]-velocities[ch]).abs()<1e-5,
                "release channel{ch}: expected {}, heard {}",velocities[ch],actual[ch]/reference[ch]);
            assert!((fragmented[ch]/reference[ch]-velocities[ch]).abs()<1e-5);
        }
        assert!((actual[0]/actual[1]-velocities[0]/velocities[1]).abs()<1e-5,
            "same-MIDI1-byte release velocities must keep their independent RMS ratio");
        assert_eq!(fixed.ended_frames,fragments.ended_frames,"block partition changed release note-end timing");
        assert_eq!(fixed.ended_frames,[53_200,53_200],
            "release starts at pedal-up frame35200 and reaches its exclusive 18000-frame EOF at53200");
        let maximum = fixed.pcm.iter().zip(&fragments.pcm).flat_map(|(a,b)|[(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0_f32,f32::max);
        assert!(maximum<2e-5,"block partition changed precise release PCM by {maximum}");
    }
}

fn release_bridge_pair(partition: &'static [usize], script: Option<&str>, velocities: [f64;2]) -> (Performance,[f64;2]) {
    let mut p = release_performance(partition,script);
    let mut events = EventList::with_capacity(16);
    for (id,channel,onset,pan) in [(130,4,0.71,0.),(131,7,0.93,1.)] {
        events.try_push_exact(ExactEvent::new(0,ExactEventBody::Note { kind:ExactNoteKind::On,
            address:address(id,channel,60),velocity:onset })).unwrap();
        expression(&mut events,id,channel,1,pan);
    }
    p.stage("release bridge attack","two live parents with independently panned inherited children",&events);
    events.clear();
    for (index,(id,channel)) in [(130,4),(131,7)].into_iter().enumerate() {
        for velocity in [velocities[index],0.99] {
            events.try_push_exact(ExactEvent::new(0,ExactEventBody::Note { kind:ExactNoteKind::Off,
                address:address(id,channel,60),velocity })).unwrap();
        }
        cc(&mut events,channel as u8,1,0);
        cc(&mut events,channel as u8,11,0);
    }
    let released = p.stage("release bridge delayed child","authored release velocity override reaches its delayed generated child",&events);
    assert!(released.iter().all(|v| *v > 0.001));
    assert!(p.ended.is_empty(),"parent ownership must outlive its release child");
    events.clear();
    let ended = p.stage("release bridge child completion","both children finish and retire their exact parents once",&events);
    assert!(ended.iter().all(|v| *v < 1e-8));
    assert_eq!(p.ended,[address(130,4,60),address(131,7,60)]);
    (p,released)
}

#[test]
fn ksp_release_velocity_override_reaches_waited_generated_child_pcm() {
    let script = r#"on init
SET_CONDITION(NO_SYS_SCRIPT_RLS_TRIG)
end on
on note
disallow_group($ALL_GROUPS)
allow_group(0)
end on
on release
ignore_event($EVENT_ID)
set_event_par($EVENT_ID,$EVENT_PAR_REL_VELOCITY,127)
if (get_event_par($EVENT_ID,$EVENT_PAR_REL_VELOCITY) # 127)
set_event_par($EVENT_ID,$EVENT_PAR_REL_VELOCITY,0)
end if
wait(40000)
disallow_group($ALL_GROUPS)
allow_group(1)
play_note($EVENT_NOTE,$EVENT_VELOCITY,0,0)
note_off($EVENT_ID)
end on"#;
    let (_,reference) = release_bridge_pair(&[128],None,[1.,1.]);
    let (fixed,actual) = release_bridge_pair(&[128],Some(script),[0.5001,0.5002]);
    let (fragments,fragmented) = release_bridge_pair(&[1,7,31,127,13,64],Some(script),[0.5001,0.5002]);
    for ch in 0..2 {
        assert!((actual[ch]/reference[ch]-1.).abs()<1e-5,
            "Get/Set EVENT_PAR_REL_VELOCITY127 must replace physical release gain for child{ch}: {}",actual[ch]/reference[ch]);
        assert!((fragmented[ch]/reference[ch]-1.).abs()<1e-5);
    }
    assert_eq!(fixed.ended_frames,fragments.ended_frames);
    assert_eq!(fixed.ended_frames,[37_520,37_520],
        "child starts at key-up17600 +1920 wait frames and reaches its exclusive 18000-frame EOF at37520");
    let maximum = fixed.pcm.iter().zip(&fragments.pcm).flat_map(|(a,b)|[(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0_f32,f32::max);
    assert!(maximum<2e-5,"block partition changed delayed release-child PCM by {maximum}");
}

fn sustained_envelope_end(partition: &'static [usize], group: Group, scripted: bool, muted: bool) -> Performance {
    let mut p = Performance::with_group(scripted,partition,group);
    let mut events = EventList::with_capacity(8);
    note(&mut events,ExactNoteKind::On,140,4,60);
    note(&mut events,ExactNoteKind::On,141,7,60);
    expression(&mut events,140,4,1,0.);
    expression(&mut events,141,7,1,1.);
    if muted { expression(&mut events,140,4,0,0.); }
    let held = p.stage("held authored envelopes","right owner remains audible while left may advance unheard",&events);
    assert!(held[1]>0.001);
    assert!(p.ended.is_empty(),"a sustained envelope must not retire before key-up");
    events.clear();
    note(&mut events,ExactNoteKind::Off,140,4,60);
    note(&mut events,ExactNoteKind::Off,141,7,60);
    let ended = p.stage("authored envelope terminal samples","both exact owners stop at the envelope boundary",&events);
    assert!(ended.iter().all(|v|*v<1e-8));
    assert_eq!(p.ended,[address(140,4,60),address(141,7,60)]);
    p
}

#[test]
fn sustained_envelopes_retire_at_authored_frames_independent_of_blocks() {
    use crate::modulation::Ahdsr;
    use crate::import::{FlexEnvelope,FlexPoint};
    let classic = Group { volume_env:Some(Ahdsr { attack_curve:0.,attack_ms:0.,hold_ms:0.,
        decay_ms:0.,sustain:1.,release_ms:10.07,unknown_flag:0,unknown_tail:Vec::new() }),
        ..Group::default() };
    let native = Group { native_volume_env:true,..classic.clone() };
    let flex = Group { flex_env:Some(FlexEnvelope {
        points:vec![FlexPoint {time_ms:1.,level:1.,curve:0.5},
            FlexPoint {time_ms:10.,level:0.,curve:0.5}],
        sustain:0,unknown_index:0,unknown_tail:Vec::new() }),..Group::default() };
    // The ordinary release reaches -60 dB in its authored time and retires
    // below -80 dB: ceil(10.07ms * 48000 * 80/60) = 645 output samples.
    // Flex release has an explicit 10ms endpoint: 480 output samples.
    // The admitted native source uses 1500 control points/second: its 15
    // release points plus final interpolation tick take 16*32=512 samples.
    // Both physical offs occur after the 17600-frame held stage.
    for (name,group,expected) in [("ordinary",classic,18_245),("flex",flex,18_080),("native",native,18_112)] {
        for scripted in [false,true] {
            for muted in [false,true] {
                let fixed = sustained_envelope_end(&[128],group.clone(),scripted,muted);
                let fragments = sustained_envelope_end(&[1,7,31,127,13,64],group.clone(),scripted,muted);
                assert_eq!(fixed.ended_frames,[expected,expected],
                    "{name}, scripted={scripted}, muted={muted}: authored envelope endpoint changed");
                assert_eq!(fragments.ended_frames,fixed.ended_frames,
                    "{name}: host block partition changed envelope retirement");
                if name=="native" {
                    assert!(fixed.pcm[expected-1][1]!=0.,"the final native interpolation sample must remain audible");
                    assert!(fixed.pcm[expected..].iter().all(|frame|*frame==[0.;2]),
                        "native NOTE_END must coincide with its first permanent zero");
                }
                let maximum = fixed.pcm.iter().zip(&fragments.pcm)
                    .flat_map(|(a,b)|[(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0_f32,f32::max);
                assert!(maximum<2e-5,"{name}: block partition changed envelope PCM by {maximum}");
            }
        }
    }
}

#[test]
fn empty_bank_waited_roots_end_at_the_actual_callback_clock() {
    let render = |partition| {
        let mut p = Performance::new(false,partition);
        p.dsp.rack.parts[0].instrument_mut().unwrap().set_bank(None);
        let (rt,errors) = Runtime::with_scripts(&["on note\nignore_event($EVENT_ID)\nwait(400000)\nend on\non release\nend on"],
            &mut crate::ksp::LogEngine::new(Vec::new(),RATE as f64),0,Vec::new());
        assert!(errors.iter().all(Option::is_none),"{errors:?}");
        p.dsp.rack.parts[0].instrument_mut().unwrap().set_script(Some(Box::new(rt)));
        let mut events = EventList::with_capacity(4);
        note(&mut events,ExactNoteKind::On,150,4,60);
        note(&mut events,ExactNoteKind::On,151,7,60);
        p.stage("empty-bank ignored roots","silent owners retain their 400ms callbacks across host blocks",&events);
        assert!(p.ended.is_empty(),"held roots cannot end before their explicit key-up");
        events.clear();
        note(&mut events,ExactNoteKind::Off,150,4,60);
        note(&mut events,ExactNoteKind::Off,151,7,60);
        p.stage("empty-bank pending callbacks","key-up at17600 cannot retire callbacks until400ms at19200",&events);
        assert!(p.pcm.iter().all(|frame|*frame==[0.;2]),"an empty bank must remain exactly silent");
        assert_eq!(p.ended,[address(150,4,60),address(151,7,60)]);
        assert_eq!(p.ended_frames,[19_200,19_200],
            "400ms callback completion uses the same elapsed audio clock with or without a bank");
        p
    };
    let fixed = render(&[128]);
    let fragments = render(&[1,7,31,127,13,64]);
    assert_eq!(fixed.ended_frames,fragments.ended_frames);
    assert_eq!(fixed.pcm,fragments.pcm);
}

// The velocity zones are measurement channels, not velocity modulation: every
// zone has the same unit sample level. This separates an anonymous input from
// two exact owners sharing its logical home without inspecting engine fields.
fn native_per_note_performance(partition: &'static [usize], scripted: bool) -> Performance {
    use crate::modulation::{ModAssignment, ModSource, ModTarget};
    let mut p = Performance::new(false, partition);
    let samples = [("pck-240", 240.), ("pck-960", 960.)].into_iter().map(|(name,hz)|
        (PathBuf::from(name), Sample { rate: RATE as u32, frames: (0..8_000).map(|i|
            [(0.02*(std::f64::consts::TAU*hz*i as f64/RATE as f64).sin()) as f32;2]).collect() })).collect();
    let group = Group { mods: vec![ModAssignment { cc_default: None,
        name:"LINEAR_NATIVE_BRIGHTNESS_VOLUME".into(), source:ModSource::MidiCc(74),
        target:ModTarget::Volume, intensity:1., invert:false, lag_ms:0, shaper:None,
    }], ..Group::default() };
    let zones = [(127, -1., "pck-240"), (64, 1., "pck-240"), (32, 1., "pck-960")]
        .into_iter().map(|(velocity,pan,sample)| Zone { root:60, low_velocity:velocity,
            high_velocity:velocity, pan, sample:PathBuf::from(sample),
            loop_range:Some(Loop {start:0,end:8_000,crossfade:0,until_release:false,alternating:false}),
            ..Zone::default() }).collect();
    p.dsp.rack.parts[0].instrument_mut().unwrap().set_bank(Some(Box::new(Bank::from_samples(vec![group],zones,samples).unwrap())));
    if scripted {
        let (runtime,errors)=Runtime::with_scripts(&[
            "on note\nignore_event($EVENT_ID)\nwait(1000)\nplay_note($EVENT_NOTE,$EVENT_VELOCITY,0,-1)\nend on"],
            &mut crate::ksp::LogEngine::default(),0,Vec::new());
        assert!(errors.iter().all(Option::is_none),"{errors:?}");
        p.dsp.rack.parts[0].instrument_mut().unwrap().set_script(Some(Box::new(runtime)));
    }
    p
}

fn native_brightness(events:&mut EventList, channel:u8, value:f64) {
    events.push(Event::on_port(0,0,EventBody::PerNoteCC {group:0,channel,note:60,
        cc:74,registered:true,value:(value*u32::MAX as f64).round() as u32}));
}

fn native_pck_lifetime(partition:&'static [usize], scripted:bool) -> Performance {
    let mut p=native_per_note_performance(partition,scripted);
    let mut events=EventList::with_capacity(32);
    let onset=|events:&mut EventList,id,channel,velocity:f64| {
        events.try_push_exact(ExactEvent::new(0,ExactEventBody::Note {
            kind:ExactNoteKind::On,address:address(id,channel,60),velocity,
        })).unwrap();
    };
    let anonymous=|events:&mut EventList| events.push(Event::on_port(0,0,EventBody::NoteOn2 {
        group:0,channel:7,note:60,velocity:((32./127.)*65535.) as u16,attribute_type:0,attribute:0,
    }));
    let gains=|p:&Performance,left:f64,right:f64,other:f64,unit:[f64;3]| {
        for (actual,expected) in [
            (amplitude(p.measured(),0,240.)/unit[0],left),
            (amplitude(p.measured(),1,240.)/unit[1],right),
            (amplitude(p.measured(),1,960.)/unit[2],other),
        ] { assert!((actual-expected).abs()<1e-5,"native PCK audible gain {actual}, expected {expected}"); }
    };
    cc(&mut events,4,74,127);
    cc(&mut events,7,74,127);
    native_brightness(&mut events,4,1.);
    native_brightness(&mut events,7,1.);
    onset(&mut events,200,4,1.);
    onset(&mut events,201,4,64./127.);
    anonymous(&mut events);
    p.stage("native PCK unit references","left/right240 and anonymous physical7 right960 have equal source levels",&events);
    let unit=[amplitude(p.measured(),0,240.),amplitude(p.measured(),1,240.),amplitude(p.measured(),1,960.)];
    assert!(unit.iter().all(|v|*v>0.001));
    events.clear();
    note(&mut events,ExactNoteKind::Choke,200,4,60);
    note(&mut events,ExactNoteKind::Choke,201,4,60);
    events.push(Event::on_port(0,0,EventBody::NoteOff2 {group:0,channel:7,note:60,velocity:0,attribute_type:0,attribute:0}));
    p.stage("retire PCK references","reference voices end before lifetime performance",&events);
    events.clear();
    // Both values project to MIDI1 byte64. The independent amplitude oracle
    // is considerably tighter than their difference, so byte rounding fails.
    let precise=[0.5001_f64,0.5002];
    assert_eq!((precise[0]*127.).round(),(precise[1]*127.).round());
    native_brightness(&mut events,4,precise[0]);
    native_brightness(&mut events,7,precise[1]);
    onset(&mut events,210,4,1.);
    anonymous(&mut events);
    p.stage("native PCK before onset","pre-On values reach exact and anonymous roots and waited children",&events);
    gains(&p,precise[0],0.,precise[1],unit);
    events.clear();
    cc(&mut events,4,121,0);
    cc(&mut events,7,121,0);
    p.stage("CC121 preserves native PCK","registered pre-On values survive reset on both physical inputs",&events);
    gains(&p,precise[0],0.,precise[1],unit);
    events.clear();
    cc(&mut events,4,64,127);
    note(&mut events,ExactNoteKind::Off,210,4,60);
    onset(&mut events,211,4,64./127.);
    native_brightness(&mut events,4,0.3);
    p.stage("native PCK after release and reuse","old sustained left and new right share native0.3; physical7 retains its independent precision",&events);
    gains(&p,0.3,0.3,precise[1],unit);
    assert!(!p.ended.contains(&address(210,4,60)),"sustain must retain the old exact owner");
    events.clear();
    expression(&mut events,210,4,5,0.6);
    native_brightness(&mut events,4,0.2);
    p.stage("exact brightness authority","old exact CLAP0.6 overrides native0.2 while reused root follows native0.2",&events);
    gains(&p,0.6,0.2,precise[1],unit);
    events.clear();
    events.push(Event::on_port(0,0,EventBody::PerNoteManagement {group:0,channel:4,note:60,flags:2}));
    native_brightness(&mut events,4,0.4);
    p.stage("detach old PCK generation","D freezes existing right0.2 and exact left0.6; next generation caches0.4",&events);
    gains(&p,0.6,0.2,precise[1],unit);
    events.clear();
    onset(&mut events,212,4,64./127.);
    p.stage("reuse after native detach","new right root inherits0.4; detached right remains0.2",&events);
    gains(&p,0.6,0.6,precise[1],unit);
    events.clear();
    events.push(Event::on_port(0,0,EventBody::PerNoteManagement {group:0,channel:4,note:60,flags:1}));
    p.stage("reset current PCK generation","S restores authored channel fallback1 for current root, detached right remains0.2",&events);
    gains(&p,0.6,1.2,precise[1],unit);
    events.clear();
    native_brightness(&mut events,4,0.7);
    p.stage("native update after S","current root follows0.7, detached right remains0.2",&events);
    gains(&p,0.6,0.9,precise[1],unit);
    events.clear();
    events.push(Event::on_port(0,0,EventBody::PerNoteManagement {group:0,channel:4,note:60,flags:3}));
    onset(&mut events,213,4,1.);
    p.stage("detach then reset future generation","D+S keeps right0.2+0.7, fresh left uses reset default1 plus exact old0.6",&events);
    gains(&p,1.6,0.9,precise[1],unit);
    events.clear();
    native_brightness(&mut events,4,0.4);
    p.stage("update after D+S","only fresh left changes to0.4; detached roots keep all prior values",&events);
    gains(&p,1.,0.9,precise[1],unit);
    events.clear();
    note(&mut events,ExactNoteKind::Choke,210,4,60);
    note(&mut events,ExactNoteKind::Choke,211,4,60);
    note(&mut events,ExactNoteKind::Choke,212,4,60);
    note(&mut events,ExactNoteKind::Choke,213,4,60);
    cc(&mut events,4,64,0);
    events.push(Event::on_port(0,0,EventBody::NoteOff2 {group:0,channel:7,note:60,velocity:0,attribute_type:0,attribute:0}));
    let silence=p.stage("retire native PCK roots","all audio ends; only exact roots emit exact NOTE_END",&events);
    assert!(silence.iter().all(|v|*v<1e-8));
    assert_eq!(p.ended.len(),6,"anonymous roots must never emit exact host ends");
    for id in [200,201,210,211,212,213] { assert!(p.ended.contains(&address(id,4,60)),"missing exact end for {id}"); }
    p
}

#[test]
fn native_per_note_controls_keep_precision_input_scope_and_detached_lifetimes_in_pcm() {
    for scripted in [false,true] {
        let fixed=native_pck_lifetime(&[128],scripted);
        let fragmented=native_pck_lifetime(&[1,7,31,127,13,64],scripted);
        let error=fixed.pcm.iter().zip(&fragmented.pcm).flat_map(|(a,b)|
            [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
        assert!(error<2e-5,"native PCK PCM depends on host partition: {error}");
        assert_eq!(fixed.ended,fragmented.ended);
        assert_eq!(fixed.ended_frames,fragmented.ended_frames);
    }
}

fn native_pitch_performance(partition:&'static [usize], scripted:bool) -> Performance {
    let mut native=Performance::new(false,partition);
    let mut reference=Performance::new(false,partition);
    if scripted {
        for p in [&mut native,&mut reference] {
            let (rt,errors)=Runtime::with_scripts(&[
                "on note\nignore_event($EVENT_ID)\nwait(1000)\nplay_note($EVENT_NOTE,$EVENT_VELOCITY,0,-1)\nend on"],
                &mut crate::ksp::LogEngine::default(),0,Vec::new());
            assert!(errors.iter().all(Option::is_none),"{errors:?}");
            p.dsp.rack.parts[0].instrument_mut().unwrap().set_script(Some(Box::new(rt)));
        }
    }
    let mut wire=EventList::with_capacity(16);
    let mut oracle=EventList::with_capacity(16);
    let compare=|native:&Performance,reference:&Performance| {
        let error=native.measured().iter().zip(reference.measured()).flat_map(|(a,b)|
            [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
        assert!(rms(reference.measured()).iter().all(|v|*v>0.001),"pitched reference must be audible");
        assert!(error<2e-5,"native pitch differs from independent authored semitone reference by {error}");
    };
    // Channel RPN7 uses Q7.25 semitones; attribute pitch uses Q7.9
    // absolute note pitch. These are authored numbers, not engine snapshots.
    wire.push(Event::on_port(0,0,EventBody::RegisteredController {group:0,channel:4,
        bank:0,index:7,value:6u32<<25}));
    wire.push(Event::on_port(0,0,EventBody::PerNotePitchBend {group:0,channel:4,note:60,value:0xc000_0000}));
    wire.push(Event::on_port(0,0,EventBody::NoteOn2 {group:0,channel:4,note:60,velocity:u16::MAX,
        attribute_type:3,attribute:60*512+256}));
    note(&mut oracle,ExactNoteKind::On,230,4,60);
    expression(&mut oracle,230,4,2,3.5);
    native.stage("native attribute and pre-On bend","absolute60.5 plus half of6-semitone range equals source tuning3.5",&wire);
    reference.stage("independent pitch3.5","exact host semitone tuning independently renders the expected pitch",&oracle);
    compare(&native,&reference);
    wire.clear();oracle.clear();
    wire.push(Event::on_port(0,0,EventBody::RegisteredController {group:0,channel:4,
        bank:0,index:7,value:12u32<<25}));
    native.stage("range change after onset","existing native root and its children retain captured6-semitone range",&wire);
    reference.stage("captured pitch reference","held reference stays tuned3.5",&oracle);
    compare(&native,&reference);
    wire.clear();
    cc(&mut wire,4,121,0);
    native.stage("CC121 preserves native pitch","registered pitch range and per-note bend survive controller reset",&wire);
    reference.stage("pitch reset reference","reset preserves independently authored3.5 semitone tuning",&oracle);
    compare(&native,&reference);
    wire.clear();
    wire.push(Event::on_port(0,0,EventBody::NoteOff2 {group:0,channel:4,note:60,velocity:0,attribute_type:0,attribute:0}));
    note(&mut oracle,ExactNoteKind::Off,230,4,60);
    let silence=native.stage("release captured native pitch","physical NoteOff stops native and following children",&wire);
    reference.stage("release captured reference","exact reference ends once",&oracle);
    assert!(silence.iter().all(|v|*v<1e-8));
    assert!(native.ended.is_empty(),"anonymous MIDI2 input must not invent an exact host note end");
    wire.clear();oracle.clear();
    wire.push(Event::on_port(0,0,EventBody::NoteOn2 {group:0,channel:4,note:60,velocity:u16::MAX,
        attribute_type:3,attribute:60*512+256}));
    note(&mut oracle,ExactNoteKind::On,231,4,60);
    expression(&mut oracle,231,4,2,6.5);
    native.stage("new onset captures new range","reused PCK keeps pre-On bend but captures12-semitone range: tuning6.5",&wire);
    reference.stage("independent pitch6.5","new reference is authored6.5 semitones above the source root",&oracle);
    compare(&native,&reference);
    wire.clear();oracle.clear();
    wire.push(Event::on_port(0,0,EventBody::NoteOff2 {group:0,channel:4,note:60,velocity:0,attribute_type:0,attribute:0}));
    note(&mut oracle,ExactNoteKind::Off,231,4,60);
    let silence=native.stage("retire reused native pitch","no retained native audio after final Off",&wire);
    reference.stage("retire reused pitch reference","both exact reference owners have ended",&oracle);
    assert!(silence.iter().all(|v|*v<1e-8));
    assert_eq!(reference.ended,[address(230,4,60),address(231,4,60)]);
    native
}

#[test]
fn native_per_note_pitch_uses_captured_range_attribute_and_pre_on_state_in_pcm() {
    for scripted in [false,true] {
        let fixed=native_pitch_performance(&[128],scripted);
        let fragmented=native_pitch_performance(&[1,7,31,127,13,64],scripted);
        let error=fixed.pcm.iter().zip(&fragmented.pcm).flat_map(|(a,b)|
            [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
        assert!(error<2e-5,"native per-note pitch PCM depends on host block partition: {error}");
    }
}

fn install_performance_script(p:&mut Performance, source:&str) {
    // Use the instrument loader: it collects authored init controller writes
    // for Engine::set_script to replay into every accepted physical row.
    let instrument=crate::import::Instrument {groups:vec![Group::default()],scripts:vec![source.into()],..Default::default()};
    let (runtime,errors)=crate::engine::load_scripts(&instrument,Vec::new(),RATE as f64);
    assert!(errors.is_empty(),"{errors:?}");
    p.dsp.rack.parts[0].instrument_mut().unwrap().set_script(runtime);
}

fn linear_volume_group(source:crate::modulation::ModSource) -> Group {
    use crate::modulation::{ModAssignment,ModTarget};
    Group {mods:vec![ModAssignment { cc_default: None,name:"INDEPENDENT_LINEAR_VOLUME_ORACLE".into(),source,
        target:ModTarget::Volume,intensity:1.,invert:false,lag_ms:0,shaper:None}],..Group::default()}
}

fn paired_cc_performance(partition:&'static [usize], mode:u8) -> Performance {
    use crate::modulation::ModSource;
    let mut p=Performance::with_group(false,partition,linear_volume_group(ModSource::MidiCc(1)));
    if mode==1 { install_performance_script(&mut p,"on controller\nwait(1000)\nend on"); }
    if mode==2 { install_performance_script(&mut p,"on controller\nif ($CC_NUM=33 and %CC[33]=1)\nignore_controller\nelse\nwait(1000)\nend if\nend on"); }
    let mut events=EventList::with_capacity(16);
    for (id,channel,pan) in [(240,4,0.),(241,7,1.)] {
        cc(&mut events,channel as u8,1,127);cc(&mut events,channel as u8,33,127);
        note(&mut events,ExactNoteKind::On,id,channel,60);expression(&mut events,id,channel,1,pan);
    }
    let unit=p.stage("paired MIDI1 CC unit references","opposite-pan same-home owners render full linear gain",&events);
    assert!(unit.iter().all(|v|*v>0.001));
    events.clear();
    cc(&mut events,4,1,64);cc(&mut events,7,1,64);
    cc(&mut events,4,33,1);cc(&mut events,7,33,2);
    let fine=p.stage("paired CC same coarse different fine","physical4 and7 retain independent14bit controller amplitudes through callbacks",&events);
    let expected=[(8192.+if mode==2 {0.} else {1.})/16383.,8194./16383.];
    for ch in 0..2 {assert!((fine[ch]/unit[ch]-expected[ch]).abs()<1e-5,
        "mode{mode} physical channel{ch} paired amplitude lost its fine byte");}
    events.clear();
    cc(&mut events,4,1,64);
    let cleared=p.stage("paired CC MSB clears fine","fresh MSB clears physical4 fine while physical7 retains2",&events);
    assert!((cleared[0]/unit[0]-8192./16383.).abs()<1e-5);
    assert!((cleared[1]/unit[1]-8194./16383.).abs()<1e-5);
    events.clear();
    cc(&mut events,4,121,0);cc(&mut events,7,121,0);
    let reset=p.stage("ordinary paired CC reset","CC121 resets ordinaryCC1 independently on both physical inputs",&events);
    assert!(reset.iter().all(|v|*v<1e-8));
    events.clear();
    note(&mut events,ExactNoteKind::Off,240,4,60);note(&mut events,ExactNoteKind::Off,241,7,60);
    p.stage("paired CC owners end","silent modulated voices still emit their exact terminal events",&events);
    assert_eq!(p.ended,[address(240,4,60),address(241,7,60)]);
    p
}

#[test]
fn midi1_paired_cc_precision_and_forwarding_reach_independent_plugin_pcm() {
    for mode in 0..3 {
        let fixed=paired_cc_performance(&[128],mode);
        let fragmented=paired_cc_performance(&[1,7,31,127,13,64],mode);
        let error=fixed.pcm.iter().zip(&fragmented.pcm).flat_map(|(a,b)|
            [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
        assert!(error<2e-5,"paired MIDI1 CC changed PCM with host partition: {error}");
        assert_eq!(fixed.ended_frames,fragmented.ended_frames);
    }
    for partition in [&[128][..],&[1,7,31,127,13,64][..]] {
        let gate=linear_volume_group(crate::modulation::ModSource::MidiCc(110)).mods.remove(0);
        // A failed raw-array check closes an independent authored gate in the
        // actual controller callback. No retained engine fields are asserted.
        let mut group=linear_volume_group(crate::modulation::ModSource::MidiCc(1));group.mods.push(gate);
        let mut raw=Performance::with_group(false,partition,group);
        install_performance_script(&mut raw,"on init\nset_controller(110,127)\nend on\non controller\nif ($CC_NUM=33)\nif (%CC[1]#0)\nset_controller(110,0)\nend if\nif (%CC[33]#127)\nset_controller(110,0)\nend if\nwait(1000)\nend if\nend on");
        let mut events=EventList::with_capacity(8);
        cc(&mut events,4,1,0);cc(&mut events,4,33,127);note(&mut events,ExactNoteKind::On,242,4,60);
        let level=raw.stage("KSP paired CC raw arrays","CC1 raw0 andCC33 raw127 survive canonical gain127/16383",&events);
        let mut unit=Performance::new(false,partition);events.clear();note(&mut events,ExactNoteKind::On,243,4,60);
        let full=unit.stage("raw CC gain reference","unmodulated source supplies independent unit level",&events);
        for ch in 0..2 {assert!((level[ch]/full[ch]-127./16383.).abs()<1e-5,"KSP raw controller bytes were replaced by paired projections");}
    }
}

#[test]
fn midi1_paired_cc_pitch_matches_independent_authored_semitones_in_pcm() {
    use crate::modulation::{ModAssignment,ModSource,ModTarget};
    for partition in [&[128][..],&[1,7,31,127,13,64][..]] {
        for waited in [false,true] {
            let group=Group {mods:vec![ModAssignment { cc_default: None,name:"PAIRED_CC_PITCH".into(),source:ModSource::MidiCc(1),
                target:ModTarget::Pitch,intensity:0.5,invert:false,lag_ms:0,shaper:None}],..Group::default()};
            let mut paired=Performance::with_group(false,partition,group);
            let mut oracle=Performance::new(false,partition);
            if waited {install_performance_script(&mut paired,"on controller\nwait(1000)\nend on");}
            let mut events=EventList::with_capacity(16);
            for (channel,fine) in [(4,1),(7,2)] {cc(&mut events,channel,1,64);cc(&mut events,channel,33,fine);}
            paired.stage("prepare paired pitch","controller callbacks complete before audible onset",&events);
            events.clear();oracle.stage("prepare authored pitch","matching elapsed time before reference onset",&events);
            for (id,channel,pan) in [(244,4,0.),(245,7,1.)] {
                note(&mut events,ExactNoteKind::On,id,channel,60);expression(&mut events,id,channel,1,pan);
            }
            paired.stage("paired fine pitch","same coarse with independent fine1 and2 tunes independently panned roots",&events);
            for (id,channel,word) in [(244,4,8193.),(245,7,8194.)] {expression(&mut events,id,channel,2,6.*word/16383.);}
            oracle.stage("authored exact pitch reference","independent semitone tuning6 times each14bit controller fraction",&events);
            let error=paired.measured().iter().zip(oracle.measured()).flat_map(|(a,b)|
                [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
            assert!(error<2e-5,"paired CC fine pitch differs from authored semitone PCM: {error}");
            assert!(rms(oracle.measured()).iter().all(|v|*v>0.001));
        }
    }
}

fn midi1_note(events:&mut EventList, channel:u8, on:bool, velocity:u8, raw:bool) {
    let body=if on {EventBody::NoteOn {group:0,channel,note:60,velocity}}
        else {EventBody::NoteOff {group:0,channel,note:60,velocity}};
    if raw {
        let token=events.try_push_exact_token(ExactEvent::new(0,ExactEventBody::Midi1 {port:0,
            message:moose::core::events::RawMidi1::new([if on {0x90} else {0x80}|channel,60,velocity],3).unwrap()})).unwrap();
        events.try_push_exact_companion(token,Event::on_port(0,0,body)).unwrap();
    } else {events.push(Event::on_port(0,0,body));}
}

fn velocity_prefix_performance(partition:&'static [usize], scripted:bool, raw:bool) -> Performance {
    let mut p=Performance::with_group(false,partition,linear_volume_group(crate::modulation::ModSource::Velocity));
    if scripted {install_performance_script(&mut p,"on note\nignore_event($EVENT_ID)\nwait(1000)\nplay_note($EVENT_NOTE,$EVENT_VELOCITY,0,-1)\nend on");}
    let mut events=EventList::with_capacity(16);
    note(&mut events,ExactNoteKind::On,250,4,60);
    let full=p.stage("velocity-prefix unit reference","independent exact velocity1 has unit authored linear gain",&events);
    assert!(full.iter().all(|v|*v>0.001));
    events.clear();note(&mut events,ExactNoteKind::Off,250,4,60);p.stage("end prefix reference","exact unit owner releases",&events);
    let check=|level:[f64;2],expected:f64| {for ch in 0..2 {
        assert!((level[ch]/full[ch]-expected).abs()<1e-5,"CC88 audible velocity {} instead of {expected}",level[ch]/full[ch]);
    }};
    for (name,prefix,channel,expected) in [
        ("prefixed MIDI1 onset",Some(1),4,8193./16383.),
        ("one-shot prefix consumed",None,4,64./127.),
        ("prefix physical isolation",Some(127),7,64./127.),
        ("pending original-channel prefix",None,4,8319./16383.),
    ] {
        events.clear();if let Some(prefix)=prefix {cc(&mut events,4,88,prefix);}
        midi1_note(&mut events,channel,true,64,raw);
        let level=p.stage(name,"one-shot14bit velocity respects original physical input",&events);check(level,expected);
        events.clear();midi1_note(&mut events,channel,false,64,raw);p.stage("release prefix probe","the previous anonymous note ends before next probe",&events);
    }
    // Native MIDI2 and exact host notes preserve a pending MIDI1-only prefix.
    for exact in [false,true] {
        events.clear();cc(&mut events,4,88,2);
        if exact {
            events.try_push_exact(ExactEvent::new(0,ExactEventBody::Note {kind:ExactNoteKind::On,address:address(251,4,60),velocity:0.75})).unwrap();
        } else {events.push(Event::on_port(0,0,EventBody::NoteOn2 {group:0,channel:4,note:60,velocity:u16::MAX,attribute_type:0,attribute:0}));}
        let level=p.stage("non-MIDI1 onset preserves prefix","native and exact semantic attacks do not consume CC88",&events);check(level,if exact {0.75} else {1.});
        events.clear();
        if exact {note(&mut events,ExactNoteKind::Off,251,4,60);} else {events.push(Event::on_port(0,0,EventBody::NoteOff2 {group:0,channel:4,note:60,velocity:0,attribute_type:0,attribute:0}));}
        p.stage("end non-MIDI1 probe","matching semantic release also preserves pending MIDI1 prefix",&events);
        events.clear();midi1_note(&mut events,4,true,64,raw);
        let level=p.stage("MIDI1 consumes preserved prefix","next MIDI1 onset uses fine2 after native/exact lifecycle",&events);check(level,8194./16383.);
        events.clear();midi1_note(&mut events,4,false,64,raw);p.stage("end preserved prefix probe","anonymous prefixed attack retires",&events);
    }
    events.clear();cc(&mut events,4,88,127);cc(&mut events,4,121,0);midi1_note(&mut events,4,true,64,raw);
    let reset=p.stage("CC121 clears velocity prefix","reset cancels pending prefix so attack uses plain MIDI1 velocity",&events);check(reset,64./127.);
    events.clear();cc(&mut events,4,88,127);midi1_note(&mut events,4,true,0,raw);
    let zero=p.stage("zero MIDI1 On ignores prefix","wire zero remains NoteOff and consumes its prefix",&events);assert!(zero.iter().all(|v|*v<1e-8));
    events.clear();midi1_note(&mut events,4,true,64,raw);
    let after_zero=p.stage("prefix consumed by zero On","next onset has unprefixed velocity64",&events);check(after_zero,64./127.);
    events.clear();midi1_note(&mut events,4,false,0,raw);
    let silent=p.stage("final prefix release","explicit zero-velocity Off ends the last anonymous root",&events);assert!(silent.iter().all(|v|*v<1e-8));
    assert_eq!(p.ended, [address(250,4,60),address(251,4,60)]);
    p
}

#[test]
fn midi1_velocity_prefix_is_one_shot_physical_and_preserves_native_lifecycles_in_pcm() {
    for scripted in [false,true] {for raw in [false,true] {
        let fixed=velocity_prefix_performance(&[128],scripted,raw);
        let fragmented=velocity_prefix_performance(&[1,7,31,127,13,64],scripted,raw);
        let error=fixed.pcm.iter().zip(&fragmented.pcm).flat_map(|(a,b)|
            [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
        assert!(error<2e-5,"velocity-prefix PCM depends on host block partition: {error}");
        assert_eq!(fixed.ended_frames,fragmented.ended_frames);
    }}
    let mut authored=Performance::with_group(false,&[128],linear_volume_group(crate::modulation::ModSource::Velocity));
    install_performance_script(&mut authored,"on init\nset_controller(88,127)\nend on");
    let mut events=EventList::with_capacity(4);midi1_note(&mut events,4,true,64,false);
    let level=authored.stage("authored CC88 is not a wire prefix","script set_controller88 cannot arm14bit ingress velocity",&events);
    let mut reference=Performance::with_group(false,&[128],linear_volume_group(crate::modulation::ModSource::Velocity));
    let expected=reference.stage("unprefixed MIDI1 reference","plain MIDI1 velocity64 provides independent expected PCM",&events);
    for ch in 0..2 {assert!((level[ch]-expected[ch]).abs()<1e-6);}
}

#[test]
fn midi1_velocity_prefix_controls_real_release_sample_gain_including_zero_off_shortcuts() {
    for partition in [&[128][..],&[1,7,31,127,13,64][..]] {
        for raw in [false,true] {
            for (on_shortcut,release,fine,expected) in [
                (false,64,1,8193./16383.),(false,0,127,127./16383.),
                (true,0,127,64./127.),
            ] {
                for script in [None,Some("on controller\nif ($CC_NUM=88)\nignore_controller\nend if\nend on\non release\nwait(1000)\nend on")] {
                    let mut actual=release_performance(partition,script);
                    let mut oracle=release_performance(partition,script);
                    let mut wire=EventList::with_capacity(8);let mut exact=EventList::with_capacity(8);
                    midi1_note(&mut wire,4,true,100,raw);note(&mut exact,ExactNoteKind::On,252,4,60);
                    actual.stage("MIDI1 release prefix attack","ordinary attack establishes anonymous physical owner",&wire);
                    oracle.stage("unit release attack reference","exact owner independently establishes unit release gain",&exact);
                    wire.clear();exact.clear();cc(&mut wire,4,88,fine);midi1_note(&mut wire,4,on_shortcut,release,raw);
                    note(&mut exact,ExactNoteKind::Off,252,4,60);
                    actual.stage("MIDI1 prefixed physical release","explicit Off uses14bit prefix; zero On uses mandatory velocity64",&wire);
                    oracle.stage("unit release sample reference","exact normalized release1 supplies independent source level",&exact);
                    for ch in 0..2 {
                        let unit=amplitude(oracle.measured(),ch,960.);let gain=amplitude(actual.measured(),ch,960.)/unit;
                        assert!(unit>0.001);assert!((gain-expected).abs()<1e-5,"prefixed release PCM gain{gain} expected{expected}");
                    }
                    wire.clear();exact.clear();
                    actual.stage("retire prefixed release sample","finite anonymous tail ends without fake exact output",&wire);
                    oracle.stage("retire unit release sample","finite exact sample emits one end",&exact);
                    assert!(actual.ended.is_empty());assert_eq!(oracle.ended,[address(252,4,60)]);
                }
            }
        }
    }
}

#[test]
fn midi1_paired_volume_and_pan_controllers_preserve_precision_across_cc121_in_pcm() {
    use crate::modulation::ModSource;
    for controller in [7,10] {for partition in [&[128][..],&[1,7,31,127,13,64][..]] {
        let mut p=Performance::with_group(false,partition,linear_volume_group(ModSource::MidiCc(controller)));
        let mut canonical=Performance::with_group(false,partition,linear_volume_group(ModSource::MidiCc(controller)));
        let mut events=EventList::with_capacity(8);
        cc(&mut events,4,controller,127);cc(&mut events,4,controller+32,127);
        note(&mut events,ExactNoteKind::On,253,4,60);
        let full=p.stage("preserved paired CC unit","full controller establishes a live source",&events);
        canonical.stage("canonical preserved CC unit","same source and native mixer establish reference",&events);
        events.clear();cc(&mut events,4,controller,64);cc(&mut events,4,controller+32,1);
        let fine=p.stage("preserved paired fine controller","paired fraction8193/16383 reaches authored modulation and native mixer",&events);
        events.clear();events.push(Event::on_port(0,0,EventBody::ControlChange2 {group:0,channel:4,cc:controller,
            value:if controller==10 {0x80000000} else {(8193./16383.*u32::MAX as f64).round() as u32}}));
        canonical.stage("independent canonical fine reference","native32bit volume reference or exactly centered native32bit pan",&events);
        // Pan's paired and native32 centers differ as normalized fractions.
        // Independently retain the authored linear CC gain and project the
        // paired pan around 8192, rather than equating protocol fractions.
        let gains=if controller==10 {
            let authored=(8193_f64/16383.)/(2147483648_f64/4294967295.);
            [authored*(1.-1./8191.),authored]
        } else {[1.;2]};
        let error=p.measured().iter().zip(canonical.measured()).flat_map(|(a,b)|
            [0,1].map(|ch|(a[ch]-(f64::from(b[ch])*gains[ch]) as f32).abs())).fold(0f32,f32::max);
        assert!(error<1e-6,"paired CC{controller} differs from independent canonical fine PCM: {error}");
        assert!(fine.iter().any(|v|*v>0.0001));
        if controller==7 {
            // General MIDI's native volume law is squared, followed here by
            // the authored linear volume assignment: combined gain is cubed.
            for ch in 0..2 {assert!((fine[ch]/full[ch]-(8193_f64/16383.).powi(3)).abs()<1e-5);}
        }
        events.clear();cc(&mut events,4,121,0);
        let reset=p.stage("preserved paired CC121","reset preserves volume/pan paired precision and native mixer",&events);
        for ch in 0..2 {assert!((reset[ch]-fine[ch]).abs()<1e-6,"CC121 altered admitted paired CC{controller} precision");}
        events.clear();note(&mut events,ExactNoteKind::Off,253,4,60);
        p.stage("preserved paired owner release","explicit owner Off still emits exact end",&events);
        assert_eq!(p.ended,[address(253,4,60)]);
    }}
}

#[test]
fn channel_pan_protocol_centers_endpoints_and_fractional_values_reach_plugin_pcm_without_heap() {
    for scripted in [false,true] {for width in [7,14,32] {
        let (center,maximum)=match width {7=>(64,127),14=>(8192,16383),_=>(0x80000000,u32::MAX)};
        let raw_values=[center,0,maximum,center,center/2+1,
            center+center/8+1,center+center/8+center/128+1,center];
        let perform=|partition| {
            let mut p=Performance::with_group(scripted,partition,Group::default());
            let mut events=EventList::with_capacity(4);
            for (stage,raw) in raw_values.into_iter().enumerate() {
                events.clear();
                match width {
                    7=>cc(&mut events,4,10,raw as u8),
                    14=>{cc(&mut events,4,10,(raw>>7) as u8);cc(&mut events,4,42,(raw&127) as u8);}
                    _=>events.push(Event::on_port(0,0,EventBody::ControlChange2 {
                        group:0,channel:4,cc:10,value:raw})),
                }
                if stage==0 {note(&mut events,ExactNoteKind::On,254,4,60);}
                let level=p.stage("protocol pan","raw source center and local endpoint-linear balance reach held PCM",&events);
                let pan=if width==7 {
                    // Keep the previous native MIDI1 float arithmetic exactly.
                    f64::from(((raw as f32/127.*127.-64.)/63.).clamp(-1.,1.))
                } else {
                    (f64::from(raw)-f64::from(center))/if raw<center {f64::from(center)} else {f64::from(maximum-center)}
                };
                let expected=[1.-pan.max(0.),1.+pan.min(0.)];
                let louder=level[0].max(level[1]);
                assert!(louder>0.0001,"pan fixture must remain audible");
                for ch in 0..2 {assert!((level[ch]/louder-expected[ch]).abs()<2e-6,
                    "{width}-bit pan {raw}, channel {ch}: measured {level:?}, expected {expected:?}");}
                if raw==center {
                    assert!(p.measured().iter().all(|frame|frame[0].to_bits()==frame[1].to_bits()),
                        "{width}-bit protocol center did not produce identical L/R PCM");
                }
                if raw==0 || raw==maximum {
                    let muted=usize::from(raw==maximum);
                    assert!(p.measured().iter().all(|frame|frame[1-muted]==0.),
                        "{width}-bit endpoint leaked into the muted channel");
                }
            }
            events.clear();note(&mut events,ExactNoteKind::Off,254,4,60);
            p.stage("pan owner release","held pan transitions preserve exact note end",&events);
            assert_eq!(p.ended,[address(254,4,60)]);
            p
        };
        let whole=perform(&[128]);
        let split=perform(&[1,7,31,127,13,64]);
        assert_eq!(whole.ended_frames,split.ended_frames);
        let error=whole.pcm.iter().zip(&split.pcm).flat_map(|(a,b)|
            [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
        assert!(error<1e-7,"{width}-bit pan transitions depend on render fragments: {error}");
    }}
}

fn native_note_callback_performance(partition:&'static [usize], bend:bool, ignored:bool) -> Performance {
    let mut p=Performance::with_group(false,partition,if bend {Group::default()}
        else {linear_volume_group(crate::modulation::ModSource::MidiCc(74))});
    let index=if bend {512} else {74};let value=if bend {4096} else {64};
    let source=format!("on init\ndeclare %first[128]\nend on\non note\nif (%first[$EVENT_NOTE]=0)\n%first[$EVENT_NOTE] := $EVENT_ID\nend if\nend on\non note_controller\nif ($NC_NUM={index})\nif ($NC_VALUE={value})\n{}wait(10000)\nif ($NC_NOTE=60 and $NC_VALUE={value})\nchange_vol(%first[$NC_NOTE],-12000,0)\nend if\nend if\nend if\nend on",if ignored {"ignore_controller\n"} else {""});
    install_performance_script(&mut p,&source);
    let mut events=EventList::with_capacity(16);
    cc(&mut events,4,74,127);
    events.push(Event::on_port(0,0,EventBody::RegisteredController {group:0,channel:4,bank:0,index:7,value:24u32<<25}));
    note(&mut events,ExactNoteKind::On,260,4,60);expression(&mut events,260,4,1,0.);
    let unit=p.stage("native NC old owner reference","left exact root supplies independent source level",&events);
    let level=amplitude(p.measured(),0,240.);assert!(unit[0]>0.001&&level>0.001);
    events.clear();
    if bend {events.push(Event::on_port(0,0,EventBody::PerNotePitchBend {group:0,channel:4,note:60,value:0xc000_0000}));}
    else {native_brightness(&mut events,4,0.5001);}
    events.push(Event::on_port(0,0,EventBody::PerNoteManagement {group:0,channel:4,note:60,flags:2}));
    if bend {events.push(Event::on_port(0,0,EventBody::PerNotePitchBend {group:0,channel:4,note:60,value:0x4000_0000}));}
    else {native_brightness(&mut events,4,0.75);}
    note(&mut events,ExactNoteKind::On,261,4,60);expression(&mut events,261,4,1,1.);
    // The reset must cancel poly-pressure waits, but this captured native NC
    // continuation is explicitly retained. Its authored gain is audible proof.
    cc(&mut events,4,121,0);
    p.stage("native NC wait detach and reuse","first-wait forwarding retains old authority; delayed script action changes original root only",&events);
    let gain=10f64.powf(-12./20.);
    if bend {
        let hz=if ignored {240.} else {480.};
        assert!((amplitude(p.measured(),0,hz)/level-gain).abs()<1e-4,
            "native bend callback must use index512/value4096 and preserve old root afterD/reset");
        assert!((amplitude(p.measured(),1,120.)/level-1.).abs()<1e-4,
            "new generation must retain its own negative half bend after old callback resumes");
    } else {
        let expected=if ignored {1.} else {0.5001};
        assert!((amplitude(p.measured(),0,240.)/level-expected*gain).abs()<1e-5,
            "registered74 callback did not preserve precise native value or its captured NC context");
        assert!((amplitude(p.measured(),1,240.)/level-0.75).abs()<1e-5,
            "old waiting callback forwarded twice or retargeted new generation");
    }
    events.clear();note(&mut events,ExactNoteKind::Off,260,4,60);note(&mut events,ExactNoteKind::Off,261,4,60);
    let silence=p.stage("native NC exact ends","both original host owners and their native generations retire",&events);
    assert!(silence.iter().all(|v|*v<1e-8));assert_eq!(p.ended,[address(260,4,60),address(261,4,60)]);
    p
}

#[test]
fn native_note_controller_callbacks_keep_raw_precision_original_generation_and_reset_lifetime_in_pcm() {
    for bend in [false,true] {for ignored in [false,true] {
        let fixed=native_note_callback_performance(&[128],bend,ignored);
        let fragmented=native_note_callback_performance(&[1,7,31,127,13,64],bend,ignored);
        let error=fixed.pcm.iter().zip(&fragmented.pcm).flat_map(|(a,b)|
            [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
        assert!(error<2e-5,"native NC callback PCM depends on host block partition: {error}");
        assert_eq!(fixed.ended_frames,fragmented.ended_frames);
    }}
}

#[test]
fn native_assignable_511_is_script_visible_without_inventing_default_dsp_in_pcm() {
    for partition in [&[128][..],&[1,7,31,127,13,64][..]] {
        for scripted in [false,true] {
            let mut p=Performance::new(false,partition);
            if scripted {install_performance_script(&mut p,"on init\ndeclare %first[128]\nend on\non note\n%first[$EVENT_NOTE] := $EVENT_ID\nend on\non note_controller\nif ($NC_NUM=511 and $NC_NOTE=60 and $NC_VALUE=112)\nwait(1000)\nchange_vol(%first[$NC_NOTE],-12000,0)\nend if\nend on");}
            let mut events=EventList::with_capacity(8);note(&mut events,ExactNoteKind::On,262,4,60);
            let unit=p.stage("assignable NC source reference","unassigned source starts with unit sample gain",&events);
            events.clear();events.push(Event::on_port(0,0,EventBody::PerNoteCC {group:0,channel:4,note:60,
                cc:255,registered:false,value:0xe000_0001}));
            let level=p.stage("native assignable controller255","wire assignable255 is scriptindex511/value112; default DSP stays unchanged",&events);
            let expected=if scripted {10f64.powf(-12./20.)} else {1.};
            for ch in 0..2 {assert!((level[ch]/unit[ch]-expected).abs()<1e-5,
                "assignable511 callback missing or unassigned wire control invented amplifier behavior");}
            events.clear();note(&mut events,ExactNoteKind::Off,262,4,60);
            p.stage("assignable owner release","script-visible metadata does not pin ended owner",&events);
            assert_eq!(p.ended,[address(262,4,60)]);
        }
    }
}

fn native_pressure_callback_performance(partition:&'static [usize], reset:bool) -> Performance {
    let mut p=Performance::with_group(false,partition,if reset {Group::default()}
        else {linear_volume_group(crate::modulation::ModSource::MonoAftertouch)});
    install_performance_script(&mut p,"on init\ndeclare %first[128]\nend on\non note\nif (%first[$EVENT_NOTE]=0)\n%first[$EVENT_NOTE] := $EVENT_ID\nend if\nend on\non poly_at\nif ($POLY_AT_NUM=60 and %POLY_AT[$POLY_AT_NUM]=64)\nwait(10000)\nchange_vol(%first[$POLY_AT_NUM],-12000,0)\nend if\nend on");
    let mut events=EventList::with_capacity(16);
    if !reset {events.push(Event::on_port(0,0,EventBody::PolyPressure2 {group:0,channel:4,note:60,pressure:u32::MAX}));}
    note(&mut events,ExactNoteKind::On,263,4,60);expression(&mut events,263,4,1,0.);
    let unit=p.stage("native pressure wait reference","left source has unit sample gain before pressure callback",&events);
    events.clear();events.push(Event::on_port(0,0,EventBody::PolyPressure2 {group:0,channel:4,note:60,pressure:(0.5001*u32::MAX as f64).round() as u32}));
    events.push(Event::on_port(0,0,EventBody::PerNoteManagement {group:0,channel:4,note:60,flags:2}));
    note(&mut events,ExactNoteKind::On,264,4,60);expression(&mut events,264,4,1,1.);
    events.push(Event::on_port(0,0,EventBody::PolyPressure2 {group:0,channel:4,note:60,pressure:(0.75*u32::MAX as f64).round() as u32}));
    if reset {cc(&mut events,4,121,0);}
    let level=p.stage("native pressure wait detach reuse reset","delayed script affects original root; CC121 cancels pressure continuation before gain action",&events);
    let expected=if reset {1.} else {0.5001*10f64.powf(-12./20.)};
    assert!((level[0]/unit[0]-expected).abs()<1e-5,"pressure continuation survived reset or lost original-root action");
    assert!((level[1]/unit[0]-if reset {1.} else {0.75}).abs()<1e-5,"old pressure continuation retargeted reused root");
    events.clear();note(&mut events,ExactNoteKind::Off,263,4,60);note(&mut events,ExactNoteKind::Off,264,4,60);
    let silence=p.stage("native pressure owners end","pressure cancellation does not leak host lifetime",&events);
    assert!(silence.iter().all(|v|*v<1e-8));assert_eq!(p.ended,[address(263,4,60),address(264,4,60)]);
    p
}

#[test]
fn native_poly_pressure_waits_preserve_original_root_and_cc121_cancels_their_pcm_action() {
    for reset in [false,true] {
        let fixed=native_pressure_callback_performance(&[128],reset);
        let fragmented=native_pressure_callback_performance(&[1,7,31,127,13,64],reset);
        let error=fixed.pcm.iter().zip(&fragmented.pcm).flat_map(|(a,b)|
            [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
        assert!(error<2e-5,"native pressure callback PCM depends on host block partition: {error}");
        assert_eq!(fixed.ended_frames,fragmented.ended_frames);
    }
}

#[test]
fn midi2_upper_bit_script_projections_preserve_original_continuous_pcm_values() {
    use crate::modulation::{ModAssignment,ModSource,ModTarget};
    for partition in [&[128][..],&[1,7,31,127,13,64][..]] {
        // At this MIDI2 CC boundary, >>25 is127 while rounding the normalized
        // value times127 produces126. Script choice and continuous amplitude
        // therefore have independently observable, different expectations.
        let mut cc_case=Performance::with_group(false,partition,linear_volume_group(ModSource::MidiCc(74)));
        install_performance_script(&mut cc_case,"on init\ndeclare $first\nend on\non note\n$first := $EVENT_ID\nend on\non controller\nif ($CC_NUM=74 and $first#0)\nif (%CC[74]=127)\nchange_vol($first,-12000,0)\nelse\nchange_vol($first,-60000,0)\nend if\nend if\nend on");
        let mut events=EventList::with_capacity(8);
        events.push(Event::on_port(0,0,EventBody::ControlChange2 {group:0,channel:4,cc:74,value:u32::MAX}));
        note(&mut events,ExactNoteKind::On,270,4,60);
        let unit=cc_case.stage("MIDI2 upper-bit CC reference","unit74 establishes full source before callback choice",&events);
        events.clear();let cc_raw=127u32<<25;
        events.push(Event::on_port(0,0,EventBody::ControlChange2 {group:0,channel:4,cc:74,value:cc_raw}));
        let actual=cc_case.stage("MIDI2 CC upper seven bits","rawupper7=127 selects authored minus12dB while amplifier keeps original32bit fraction",&events);
        let expected=cc_raw as f64/u32::MAX as f64*10f64.powf(-12./20.);
        for ch in 0..2 {assert!((actual[ch]/unit[ch]-expected).abs()<1e-5,"MIDI2 CC projection or continuous arithmetic changed");}
        events.clear();note(&mut events,ExactNoteKind::Off,270,4,60);cc_case.stage("end projected CC owner","exact root ends after boundary-control performance",&events);
        assert_eq!(cc_case.ended,[address(270,4,60)]);

        // raw upper14=8193 is one above center: KSP sees centered1.
        // Old normalized rounding produces centered2. Only1 authors an octave.
        let mut bend=Performance::new(false,partition);
        install_performance_script(&mut bend,"on init\ndeclare $first\ndeclare ui_slider $seen(-8192,8191)\nend on\non note\n$first := $EVENT_ID\nend on\non controller\nif ($CC_NUM=$VCC_PITCH_BEND and $first#0)\n$seen := $PITCH_BEND\nif ($PITCH_BEND=1)\nchange_tune($first,1200000,0)\nelse\nchange_tune($first,2400000,0)\nend if\nend if\nend on");
        events.clear();note(&mut events,ExactNoteKind::On,271,4,60);
        bend.stage("MIDI2 bend projection reference","dry240Hz owner precedes native bend callback",&events);
        let fundamental=amplitude(bend.measured(),0,240.);
        events.clear();let bend_raw=(8193u32<<18)|0x3ffff;
        events.push(Event::on_port(0,0,EventBody::PitchBend2 {group:0,channel:4,value:bend_raw}));
        bend.stage("MIDI2 bend upper fourteen bits","rawupper14=8193 gives KSPcentered1 and selects octave rather than rounded two octaves",&events);
        let crossings:Vec<f64>=bend.measured().windows(2).enumerate().filter(|&(_,p)|
            p[0][0]<=0. && p[1][0]>0.).map(|(i,p)|i as f64-f64::from(p[0][0])/f64::from(p[1][0]-p[0][0])).collect();
        let pcm_f0=(crossings.len()-1) as f64*RATE as f64/(crossings.last().unwrap()-crossings[0]);
        let census=bend.dsp.rack.parts[0].instrument().unwrap().voice_census();
        eprintln!("upper-bit bend diagnostic: raw={bend_raw:#x}, unsigned_upper14={}, expected centered=1, PCM F0={pcm_f0}, authored expected F0=480, RMS={:?}, source step={:?}, amplitudes240/480/960={:?}, script={:?}",
            bend_raw>>18,rms(bend.measured()),census.iter().map(|v|v.step).collect::<Vec<_>>(),
            [240.,480.,960.].map(|hz|amplitude(bend.measured(),0,hz)),bend.dsp.rack.parts[0].instrument().unwrap().script().unwrap().interface(0).controls[0].properties["$CONTROL_PAR_VALUE"]);
        assert!((amplitude(bend.measured(),0,480.)/fundamental-1.).abs()<1e-4,
            "MIDI2 bend upper-bit callback chose wrong authored pitch; measured F0={pcm_f0}");
        // Independently validate the same raw bend's continuous domain with
        // authored linear bend-to-volume, without relying on script projection.
        let group=Group {mods:vec![ModAssignment { cc_default: None,name:"RAW_BEND_CONTINUOUS_VOLUME".into(),source:ModSource::PitchBend,
            target:ModTarget::Volume,intensity:1.,invert:false,lag_ms:0,shaper:None}],..Group::default()};
        let mut continuous=Performance::with_group(false,partition,group);
        events.clear();events.push(Event::on_port(0,0,EventBody::PitchBend2 {group:0,channel:4,value:u32::MAX}));
        note(&mut events,ExactNoteKind::On,272,4,60);
        let full=continuous.stage("continuous bend unit reference","positive full bend supplies near-unit linear modulation",&events);
        events.clear();events.push(Event::on_port(0,0,EventBody::PitchBend2 {group:0,channel:4,value:bend_raw}));
        let level=continuous.stage("continuous bend boundary","modulation retains original32bit centered fraction",&events);
        let fraction=(bend_raw as f64-2147483648.)/2147483648.;
        // A bipolar bend source maps to unipolar volume: center is half gain.
        // The positive endpoint reference is (2^32-1)/2^32, just below unity.
        let expected=((fraction+1.)*0.5)/(1.-2f64.powi(-32));
        for ch in 0..2 {assert!((level[ch]/full[ch]-expected).abs()<1e-7,
            "MIDI2 bend DSP was quantized to its script projection: gain{}, independently expected{expected}",level[ch]/full[ch]);}

        // Native16bit attack32767 projects upper7=63, not rounded64. Both an
        // authored velocity layer and callback choice make the byte audible.
        let mut velocity=Performance::with_group(false,partition,linear_volume_group(ModSource::Velocity));
        let paths=[PathBuf::from("velocity63-240"),PathBuf::from("velocity64-960")];
        let samples=paths.iter().zip([240.,960.]).map(|(path,hz)| (path.clone(),Sample {rate:RATE as u32,
            frames:(0..8_000).map(|i|[(0.02*(std::f64::consts::TAU*hz*i as f64/RATE as f64).sin()) as f32;2]).collect()})).collect();
        let zones=paths.into_iter().zip([63,64]).map(|(sample,v)| Zone {root:60,sample,low_velocity:v,high_velocity:v,
            loop_range:Some(Loop {start:0,end:8_000,crossfade:0,until_release:false,alternating:false}),..Zone::default()}).collect();
        velocity.dsp.rack.parts[0].instrument_mut().unwrap().set_bank(Some(Box::new(Bank::from_samples(vec![linear_volume_group(ModSource::Velocity)],zones,samples).unwrap())));
        install_performance_script(&mut velocity,"on note\nif ($EVENT_VELOCITY=63)\nchange_vol($EVENT_ID,-12000,0)\nelse\nchange_vol($EVENT_ID,-60000,0)\nend if\nend on");
        let mut reference=Performance::new(false,partition);events.clear();note(&mut events,ExactNoteKind::On,273,4,60);
        reference.stage("velocity unit spectral reference","independent source fundamental establishes amplitude0.02",&events);
        events.clear();events.push(Event::on_port(0,0,EventBody::NoteOn2 {group:0,channel:4,note:60,velocity:32767,attribute_type:0,attribute:0}));
        velocity.stage("native velocity upper seven bits","layer63 and callback minus12dB use upperbits while DSP keeps32767/65535",&events);
        let gain=amplitude(velocity.measured(),0,240.)/amplitude(reference.measured(),0,240.);
        assert!((gain-32767./65535.*10f64.powf(-12./20.)).abs()<1e-5,
            "native velocity projection or original16bit amplitude was lost");
        assert!(amplitude(velocity.measured(),0,960.)<1e-6,"rounded velocity incorrectly selected layer64");
        events.clear();events.push(Event::on_port(0,0,EventBody::NoteOff2 {group:0,channel:4,note:60,velocity:0,attribute_type:0,attribute:0}));
        let silence=velocity.stage("native velocity boundary release","selected anonymous layer accepts explicit native Off",&events);
        assert!(silence.iter().all(|v|*v<1e-8));assert!(velocity.ended.is_empty());
    }
}

fn ordinary_rpn0_performance(partition:&'static [usize], scripted:bool) -> Performance {
    let mut actual=Performance::new(false,partition);let mut reference=Performance::new(false,partition);
    if scripted {install_performance_script(&mut actual,"on controller\nwait(1000)\nend on\non rpn\nwait(1000)\nend on");}
    let mut wire=EventList::with_capacity(32);let mut oracle=EventList::with_capacity(16);
    let range=|events:&mut EventList,channel,coarse,cents| {
        for (controller,value) in [(101,0),(100,0),(6,coarse),(38,cents)] {cc(events,channel,controller,value);}
    };
    let native_raw=0xa001_ffffu32;
    let fraction=(native_raw as f64-2147483648.)/2147483648.;
    let bends=|events:&mut EventList| {
        events.push(Event::on_port(0,0,EventBody::PitchBend {group:0,channel:4,value:12288}));
        events.push(Event::on_port(0,0,EventBody::PitchBend2 {group:0,channel:7,value:native_raw}));
    };
    let compare=|a:&Performance,b:&Performance| {
        let error=a.measured().iter().zip(b.measured()).flat_map(|(a,b)|
            [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
        assert!(rms(b.measured()).iter().all(|v|*v>0.001),"RPN0 reference must be audibly pitched");
        assert!(error<2e-5,"ordinary RPN0 source differs from independent semitone reference PCM by{error}");
    };
    range(&mut wire,4,12,50);range(&mut wire,7,6,25);bends(&mut wire);
    for (id,channel,pan,tune) in [(274,4,0.,6.25),(275,7,1.,6.25*fraction)] {
        note(&mut wire,ExactNoteKind::On,id,channel,60);expression(&mut wire,id,channel,1,pan);
        note(&mut oracle,ExactNoteKind::On,id,channel,60);expression(&mut oracle,id,channel,1,pan);expression(&mut oracle,id,channel,2,tune);
    }
    actual.stage("ordinary RPN0 independent physical sensitivity","MIDI1range12.50/native range6.25 affect same-home physical4/7 independently",&wire);
    reference.stage("independent authored RPN0 pitch","left tune6.25 and right tune6.25 times original native fraction",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();range(&mut wire,7,2,75);expression(&mut oracle,275,7,2,2.75*fraction);
    actual.stage("ordinary RPN0 live right change","right sensitivity becomes2.75 while held left remains12.50",&wire);
    reference.stage("independent live sensitivity reference","only right authored semitone tuning changes",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();cc(&mut wire,4,121,0);cc(&mut wire,7,121,0);
    expression(&mut oracle,274,4,2,0.);expression(&mut oracle,275,7,2,0.);
    actual.stage("ordinary RPN0 controller reset","CC121 centers current bend without discarding negotiated sensitivity",&wire);
    reference.stage("independent centered bend reference","both authored roots return to source pitch240Hz",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();bends(&mut wire);
    expression(&mut oracle,274,4,2,6.25);expression(&mut oracle,275,7,2,2.75*fraction);
    actual.stage("ordinary RPN0 bend after reset","same original bend reuses retained12.50 and2.75 sensitivities",&wire);
    reference.stage("independent retained sensitivity reference","restored input fractions produce original left and changed right pitch",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();
    // CC121 clears the received transaction, but a previously accepted range
    // provides the real coarse baseline for a later fine-only RPN0 update.
    cc(&mut wire,7,101,0);cc(&mut wire,7,100,0);cc(&mut wire,7,38,50);
    expression(&mut oracle,275,7,2,2.50*fraction);
    actual.stage("ordinary RPN0 known coarse after reset","reselected fine-only50 retains accepted coarse2 and changes only right sensitivity to2.50",&wire);
    reference.stage("independent accepted coarse reference","left stays6.25 semitones; right equals2.50 times original native fraction",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();range(&mut wire,4,12,99);expression(&mut oracle,274,4,2,12.99*0.5);
    actual.stage("ordinary RPN0 supported cents setup","left accepted coarse12/cents99 prepares a supported cents update",&wire);
    reference.stage("independent cents setup reference","left authored pitch is half12.99 semitones",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();cc(&mut wire,4,96,127);expression(&mut oracle,274,4,2,13.*0.5);
    actual.stage("ordinary RPN0 relative increment","CC96 wire127 advances one cent across12.99 to13.00, not127 steps",&wire);
    reference.stage("independent one-cent reference","left pitch advances to half13.00; right remains unchanged",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();cc(&mut wire,4,38,25);expression(&mut oracle,274,4,2,12.25*0.5);
    actual.stage("ordinary RPN0 fine after supported cents","fine25 retains coarse12 after cents99 became100",&wire);
    reference.stage("independent retained coarse reference","left pitch becomes half12.25 with unchanged right",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();cc(&mut wire,4,97,0);expression(&mut oracle,274,4,2,12.24*0.5);
    actual.stage("ordinary RPN0 relative decrement","CC97 wire0 still decreases one cent to12.24",&wire);
    reference.stage("independent one-cent decrement reference","left pitch becomes half12.24 with unchanged right",&oracle);compare(&actual,&reference);
    // A short musical observation also checks absolute frequency directly;
    // the complete PCM comparison above remains the strict phase oracle.
    let mut first=None;let mut last=0.;let mut crossings=0;
    for (i,pair) in actual.measured()[..RATE/100].windows(2).enumerate() {
        if pair[0][0]<=0. && pair[1][0]>0. {
            let at=i as f64-f64::from(pair[0][0])/f64::from(pair[1][0]-pair[0][0]);
            first.get_or_insert(at);last=at;crossings+=1;
        }
    }
    assert!(crossings>=3,"ten milliseconds must contain multiple audible periods");
    let observed=(crossings-1) as f64*RATE as f64/(last-first.unwrap());
    let expected=240.*2f64.powf((12.24*0.5)/12.);
    assert!((observed-expected).abs()<0.02,"relative decrement ten-ms F0={observed}, independently expected{expected}");
    wire.clear();oracle.clear();cc(&mut wire,4,101,127);cc(&mut wire,4,100,127);cc(&mut wire,4,96,127);
    actual.stage("ordinary RPN0 null relative input","null selection ignores relative operations and retains accepted sensitivity",&wire);
    reference.stage("independent null selection reference","both authored pitches remain unchanged",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();cc(&mut wire,4,99,0);cc(&mut wire,4,98,0);cc(&mut wire,4,97,0);
    actual.stage("ordinary RPN0 NRPN relative input","same address in assignable family cannot alter registered bend sensitivity",&wire);
    reference.stage("independent NRPN isolation reference","both authored pitches remain unchanged",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();
    for (id,channel) in [(274,4),(275,7)] {note(&mut wire,ExactNoteKind::Off,id,channel,60);note(&mut oracle,ExactNoteKind::Off,id,channel,60);}
    let silence=actual.stage("ordinary RPN0 exact ends","received configuration does not leak either root lifetime",&wire);
    reference.stage("ordinary RPN0 reference ends","both independent semitone owners retire",&oracle);
    assert!(silence.iter().all(|v|*v<1e-8));assert_eq!(actual.ended,[address(274,4,60),address(275,7,60)]);
    assert_eq!(actual.ended_frames,reference.ended_frames);
    actual
}

#[test]
fn ordinary_midi1_rpn0_sensitivity_is_live_physical_and_preserved_across_reset_in_plugin_pcm() {
    for scripted in [false,true] {
        let fixed=ordinary_rpn0_performance(&[128],scripted);
        let fragmented=ordinary_rpn0_performance(&[1,7,31,127,13,64],scripted);
        let error=fixed.pcm.iter().zip(&fragmented.pcm).flat_map(|(a,b)|
            [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
        assert!(error<2e-5,"ordinary RPN0 PCM depends on host block partition:{error}");
        assert_eq!(fixed.ended_frames,fragmented.ended_frames);
    }
    use crate::modulation::{ModAssignment,ModSource,ModTarget};
    for partition in [&[128][..],&[1,7,31,127,13,64][..]] {
        let group=Group {mods:vec![ModAssignment { cc_default: None,name:"AUTHORED_FALLBACK_SIX_SEMITONES".into(),source:ModSource::PitchBend,
            target:ModTarget::Pitch,intensity:0.5,invert:false,lag_ms:0,shaper:None}],..Group::default()};
        let mut fine_only=Performance::with_group(false,partition,group);let mut reference=Performance::new(false,partition);
        let mut wire=EventList::with_capacity(8);let mut oracle=EventList::with_capacity(8);
        cc(&mut wire,4,101,0);cc(&mut wire,4,100,0);cc(&mut wire,4,96,127);cc(&mut wire,4,97,0);cc(&mut wire,4,38,50);
        wire.push(Event::on_port(0,0,EventBody::PitchBend {group:0,channel:4,value:12288}));
        note(&mut wire,ExactNoteKind::On,276,4,60);
        note(&mut oracle,ExactNoteKind::On,276,4,60);expression(&mut oracle,276,4,2,3.);
        fine_only.stage("ordinary RPN0 unknown coarse","fine-only entry cannot invent a sensitivity; authored6-semitone fallback remains",&wire);
        reference.stage("independent unknown-coarse reference","half bend times authored6 equals3 semitones",&oracle);
        let error=fine_only.measured().iter().zip(reference.measured()).flat_map(|(a,b)|
            [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
        assert!(error<2e-5,"fine-only RPN0 invented a coarse baseline:{error}");
    }
}

fn extended_rpn0_performance(partition: &'static [usize], scripted: bool, transport: u8) -> Performance {
    let mut actual = Performance::new(false, partition);
    let mut reference = Performance::new(false, partition);
    if scripted {
        install_performance_script(&mut actual, "on controller\nwait(1000)\nend on\non rpn\nwait(1000)\nend on");
    }
    let mut wire = EventList::with_capacity(16);
    let mut oracle = EventList::with_capacity(8);
    let range = |events: &mut EventList, cents: u8| match transport {
        0 => {
            for (controller, value) in [(101, 0), (100, 0), (6, 127), (38, cents)] {
                cc(events, 4, controller, value);
            }
        }
        1 => events.push(Event::on_port(0, 0, EventBody::RegisteredController {
            group: 0, channel: 4, bank: 0, index: 0,
            value: (127 << 25) | (u32::from(cents) << 18) | 0x3ffff,
        })),
        _ => events.try_push_exact(ExactEvent::new(0, ExactEventBody::Ump {
            port: 0,
            packet: moose::core::events::RawUmp::new([
                0x4024_0000, (127 << 25) | (u32::from(cents) << 18) | 0x3ffff, 0, 0,
            ], 2).unwrap(),
        })).unwrap(),
    };
    let compare = |actual: &Performance, reference: &Performance, sensitivity: f64| {
        let error = actual.measured().iter().zip(reference.measured()).flat_map(|(a, b)|
            [(a[0] - b[0]).abs(), (a[1] - b[1]).abs()]).fold(0_f32, f32::max);
        assert!(error < 2e-5, "transport{transport}, range{sensitivity}: extended-cents PCM differs from independent pitch by {error}");
        let expected = 240. * 2_f64.powf(sensitivity / 128. / 12.);
        assert!((pcm_fundamental(actual.measured(), 0) - expected).abs() < 0.02,
            "extended sensitivity must produce its independently expected audible frequency");
    };
    range(&mut wire, 100);
    // A small exact bend keeps the maximum sensitivity within a musical octave.
    wire.push(Event::on_port(0, 0, EventBody::PitchBend { group: 0, channel: 4, value: 8256 }));
    note(&mut wire, ExactNoteKind::On, 277, 4, 60);
    note(&mut oracle, ExactNoteKind::On, 277, 4, 60);
    expression(&mut oracle, 277, 4, 2, 128. / 128.);
    actual.stage("RPN0 absolute cents100", "127 semitones plus100 cents is128, not a rejected parameter", &wire);
    reference.stage("independent extended-cents pitch", "128-semitone range times1/128 bend is one semitone", &oracle);
    compare(&actual, &reference, 128.);
    for (stage, sensitivity) in [(0, 128.27), (1, 128.27), (2, 128.27), (3, 128.26), (4, 128.25)] {
        wire.clear(); oracle.clear();
        match stage {
            0 => range(&mut wire, 127),
            1 => {
                cc(&mut wire, 4, 121, 0);
                wire.push(Event::on_port(0, 0, EventBody::PitchBend { group: 0, channel: 4, value: 8256 }));
            }
            2 => { cc(&mut wire, 4, 101, 0); cc(&mut wire, 4, 100, 0); cc(&mut wire, 4, 96, 127); }
            3 => cc(&mut wire, 4, 97, 0),
            _ => cc(&mut wire, 4, 38, 125),
        }
        expression(&mut oracle, 277, 4, 2, sensitivity / 128.);
        actual.stage("RPN0 extended boundary update", "maximum/CC121/saturating increment/decrement/fine overwrite retain legal coarse127", &wire);
        reference.stage("independent boundary pitch", "authored semitone pitch uses the independently calculated full sensitivity", &oracle);
        compare(&actual, &reference, sensitivity);
    }
    wire.clear(); oracle.clear();
    note(&mut wire, ExactNoteKind::Off, 277, 4, 60);
    note(&mut oracle, ExactNoteKind::Off, 277, 4, 60);
    assert!(actual.stage("extended RPN0 exact end", "parameter configuration cannot retain a finished note", &wire).iter().all(|v| *v < 1e-8));
    reference.stage("extended reference end", "independent owner retires at the same sample", &oracle);
    assert_eq!(actual.ended, [address(277, 4, 60)]);
    assert_eq!(actual.ended_frames, reference.ended_frames);
    actual
}

#[test]
fn rpn0_extended_absolute_cents_and_relative_boundaries_reach_plugin_pcm_without_heap() {
    for scripted in [false, true] {
        for transport in 0..3 {
            let fixed = extended_rpn0_performance(&[128], scripted, transport);
            let fragmented = extended_rpn0_performance(&[1, 7, 31, 127, 13, 64], scripted, transport);
            let error = fixed.pcm.iter().zip(&fragmented.pcm).flat_map(|(a, b)|
                [(a[0] - b[0]).abs(), (a[1] - b[1]).abs()]).fold(0_f32, f32::max);
            assert!(error < 2e-5, "transport{transport}: extended RPN0 depends on host partition by {error}");
            assert_eq!(fixed.ended_frames, fragmented.ended_frames);
        }
    }
}

#[test]
fn ordinary_rpn0_relative_operations_keep_raw_controller_callbacks_without_fabricating_absolute_callbacks() {
    for partition in [&[128][..],&[1,7,31,127,13,64][..]] {
        let mut p=Performance::new(false,partition);
        install_performance_script(&mut p,"on init\ndeclare $first\ndeclare $armed\nend on\non note\n$first := $EVENT_ID\nend on\non controller\nif ($CC_NUM=20)\n$armed := 1\nend if\nif ($CC_NUM=96 and $armed=1)\nif (%CC[96]=127)\nchange_vol($first,-12000,0)\nelse\nchange_vol($first,-60000,0)\nend if\nend if\nif ($CC_NUM=97 and $armed=1)\nif (%CC[97]=0)\nchange_vol($first,-24000,0)\nelse\nchange_vol($first,-60000,0)\nend if\nend if\nend on\non rpn\nif ($armed=1)\nchange_vol($first,-60000,0)\nend if\nend on");
        let mut events=EventList::with_capacity(16);
        for (controller,value) in [(101,0),(100,0),(6,12),(38,99)] {cc(&mut events,4,controller,value);}
        note(&mut events,ExactNoteKind::On,277,4,60);
        let full=p.stage("relative raw callback source","real absolute sensitivity setup completes before callback gate is armed",&events);
        events.clear();cc(&mut events,4,20,1);cc(&mut events,4,96,127);
        let increment=p.stage("relative increment raw callback","wire127 remains in%CC96; relative operation must not synthesize absolute onrpn",&events);
        for ch in 0..2 {assert!((increment[ch]/full[ch]-10f64.powf(-12./20.)).abs()<1e-5,
            "CC96 raw callback lost wire127 or fabricated an absolute RPN callback");}
        events.clear();cc(&mut events,4,97,0);
        let decrement=p.stage("relative decrement raw callback","wire0 remains in%CC97 and still invokes its raw callback without absolute onrpn",&events);
        for ch in 0..2 {assert!((decrement[ch]/full[ch]-10f64.powf(-24./20.)).abs()<1e-5,
            "CC97 raw callback lost wire0 or fabricated an absolute RPN callback");}
        events.clear();note(&mut events,ExactNoteKind::Off,277,4,60);
        p.stage("relative raw callback exact end","controller semantics do not retain the ended exact root",&events);
        assert_eq!(p.ended,[address(277,4,60)]);
    }
}

fn pcm_fundamental(frames:&[[f32;2]], channel:usize) -> f64 {
    let mut first=None;let mut last=0.;let mut count=0;
    for (i,p) in frames.windows(2).enumerate() {
        if p[0][channel]<=0. && p[1][channel]>0. {
            let at=i as f64-f64::from(p[0][channel])/f64::from(p[1][channel]-p[0][channel]);
            first.get_or_insert(at);last=at;count+=1;
        }
    }
    assert!(count>2,"frequency observation requires an audible periodic source");
    (count-1) as f64*RATE as f64/(last-first.unwrap())
}

fn legacy_tuning(events:&mut EventList, channel:u8, parameter:u8, coarse:u8, fine:Option<u8>) {
    for (cc_num,value) in [(101,0),(100,parameter),(6,coarse)] {cc(events,channel,cc_num,value);}
    if let Some(fine)=fine {cc(events,channel,38,fine);}
}

fn ordinary_tuning_performance(partition:&'static [usize], mode:u8) -> Performance {
    let mut actual=Performance::new(false,partition);let mut reference=Performance::new(false,partition);
    if mode!=0 {install_performance_script(&mut actual,if mode==1 {
        "on controller\nwait(1000)\nend on\non rpn\nwait(1000)\nend on"
    } else {"on controller\nif ($CC_NUM=6 or $CC_NUM=38 or $CC_NUM=96 or $CC_NUM=97)\nignore_controller\nend if\nwait(1000)\nend on\non rpn\nignore_controller\nwait(1000)\nend on"});}
    let mut wire=EventList::with_capacity(32);let mut oracle=EventList::with_capacity(16);
    legacy_tuning(&mut wire,4,1,96,Some(0)); // fine +.5 semitones.
    legacy_tuning(&mut wire,4,2,76,Some(127)); // coarse +12; LSB is ignored.
    for (id,channel,pan,tune) in [(280,4,0.,12.5),(281,7,1.,0.)] {
        note(&mut wire,ExactNoteKind::On,id,channel,60);expression(&mut wire,id,channel,1,pan);
        note(&mut oracle,ExactNoteKind::On,id,channel,60);expression(&mut oracle,id,channel,1,pan);expression(&mut oracle,id,channel,2,tune);
    }
    let compare=|a:&Performance,b:&Performance| {
        let error=a.measured().iter().zip(b.measured()).flat_map(|(a,b)|
            [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
        assert!(rms(b.measured()).iter().all(|v|*v>0.001));
        assert!(error<2e-5,"physical channel RPN tuning differs from independent authored semitone PCM:{error}");
    };
    actual.stage("RPN1 and2 physical tuning","left coarse12 plus fine.5 shifts240Hz to494.065Hz; shared-home right remains240Hz",&wire);
    reference.stage("independent channel tuning reference","left authored12.5 semitones and unchanged right",&oracle);compare(&actual,&reference);
    assert!((pcm_fundamental(actual.measured(),0)-240.*2f64.powf(12.5/12.)).abs()<1e-5);
    assert!((pcm_fundamental(actual.measured(),1)-240.).abs()<1e-5);
    wire.clear();oracle.clear();cc(&mut wire,4,101,0);cc(&mut wire,4,100,1);cc(&mut wire,4,38,1);
    expression(&mut oracle,280,4,2,12.+4097./8192.);
    actual.stage("RPN1 live fine-only update","acceptedMSB96 plus fine1 updates original physical root",&wire);
    reference.stage("independent fine-only reference","left coarse12 plus original14bit fine fraction4097/8192",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();cc(&mut wire,4,96,127);expression(&mut oracle,280,4,2,12.+4098./8192.);
    actual.stage("RPN1 relative quantum","wire127 advances fine tuning exactly1/8192 semitone",&wire);
    reference.stage("independent fine relative reference","left authored tuning advances one fine quantum",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();cc(&mut wire,4,101,0);cc(&mut wire,4,100,2);cc(&mut wire,4,96,0);
    expression(&mut oracle,280,4,2,13.+4098./8192.);
    actual.stage("RPN2 relative whole semitone","wire0 still advances recognized coarse tuning by1",&wire);
    reference.stage("independent coarse relative reference","left coarse13 plus unchanged fine fraction; right remains240Hz",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();legacy_tuning(&mut wire,4,0,2,Some(0));
    wire.push(Event::on_port(0,0,EventBody::PitchBend {group:0,channel:4,value:12288}));
    expression(&mut oracle,280,4,2,14.+4098./8192.);
    actual.stage("channel tuning plus independent bend","RPN0 half bend adds1 semitone to retained fine and coarse tuning",&wire);
    reference.stage("independent combined tuning reference","authored coarse13 finefraction and bend1 are added once",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();cc(&mut wire,4,121,0);expression(&mut oracle,280,4,2,13.+4098./8192.);
    actual.stage("CC121 preserves channel tuning","reset centers bend while preserving recognized fine and coarse tuning",&wire);
    reference.stage("independent tuning reset reference","only the additional bend semitone is removed",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();legacy_tuning(&mut wire,4,1,127,Some(127));
    expression(&mut oracle,280,4,2,13.+8191./8192.);
    actual.stage("legacy RPN1 endpoint","MIDI1 fine endpoint is8191/8192 above center",&wire);
    reference.stage("independent legacy endpoint reference","coarse13 plus fine8191/8192",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();cc(&mut wire,4,96,127);
    actual.stage("legacy RPN1 increment endpoint","relative increment saturates original14bit endpoint rather than inventing extra low bits",&wire);
    reference.stage("independent saturated endpoint reference","authored endpoint remains unchanged",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();cc(&mut wire,4,121,0);cc(&mut wire,4,101,0);cc(&mut wire,4,100,1);cc(&mut wire,4,38,50);
    expression(&mut oracle,280,4,2,13.+8114./8192.);
    actual.stage("RPN1 reselect fine after reset","recognizedMSB127 survives transaction reset and supports new fine50",&wire);
    reference.stage("independent retained fine coarse reference","coarse13 plus fine(127*128+50-8192)/8192",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();cc(&mut wire,4,101,127);cc(&mut wire,4,100,127);cc(&mut wire,4,96,127);
    actual.stage("RPN tuning null relative operation","null selection does not alter existing physical tuning",&wire);
    reference.stage("independent null tuning reference","both authored pitches remain unchanged",&oracle);compare(&actual,&reference);
    wire.clear();oracle.clear();
    for (id,channel) in [(280,4),(281,7)] {note(&mut wire,ExactNoteKind::Off,id,channel,60);note(&mut oracle,ExactNoteKind::Off,id,channel,60);}
    let silence=actual.stage("RPN tuning exact owner ends","configuration does not pin released exact owners",&wire);
    reference.stage("RPN tuning reference ends","both independently pitched roots retire",&oracle);
    assert!(silence.iter().all(|v|*v<1e-8));assert_eq!(actual.ended,[address(280,4,60),address(281,7,60)]);
    assert_eq!(actual.ended_frames,reference.ended_frames);
    actual
}

#[test]
fn channel_rpn1_and2_tuning_is_physical_live_precise_and_preserved_across_reset_in_plugin_pcm() {
    for mode in 0..3 {
        let fixed=ordinary_tuning_performance(&[128],mode);
        let fragmented=ordinary_tuning_performance(&[1,7,31,127,13,64],mode);
        let error=fixed.pcm.iter().zip(&fragmented.pcm).flat_map(|(a,b)|
            [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
        assert!(error<2e-5,"channel tuning PCM depends on host block partition:{error}");
        assert_eq!(fixed.ended_frames,fragmented.ended_frames);
    }
}

fn native_channel_tuning_precision(partition:&'static [usize]) -> Performance {
    let mut p=Performance::new(false,partition);let mut unchanged=Performance::new(false,partition);
    let mut events=EventList::with_capacity(16);
    let fine=[0xc000_0001u32,0xc003_ffff];assert_eq!(fine[0]>>18,fine[1]>>18);
    for (id,channel,pan,raw) in [(282,4,0.,fine[0]),(283,7,1.,fine[1])] {
        events.push(Event::on_port(0,0,EventBody::RegisteredController {group:0,channel:channel as u8,bank:0,index:1,value:raw}));
        events.push(Event::on_port(0,0,EventBody::RegisteredController {group:0,channel:channel as u8,bank:0,index:2,value:76u32<<25}));
        note(&mut events,ExactNoteKind::On,id,channel,60);expression(&mut events,id,channel,1,pan);
    }
    p.stage("native channel tuning low bits","sameupper14 fine controls retain different low18 bits on independent physical roots",&events);
    unchanged.stage("native coarse low-bit reference","matching original fine controls and coarse12",&events);
    let expected=fine.map(|raw|240.*2f64.powf((12.+(raw as f64-2147483648.)/2147483648.)/12.));
    for (ch, &expected) in expected.iter().enumerate() {
        let observed=pcm_fundamental(p.measured(),ch);
        assert!((observed-expected).abs()<1e-5,"native fine frequency{observed} expected independent f64 value{}",expected);
    }
    assert!((pcm_fundamental(p.measured(),1)-pcm_fundamental(p.measured(),0)).abs()>0.003,
        "same upper14 native fine words must remain audibly distinct in phase/frequency");
    events.clear();
    for channel in [4,7] {events.push(Event::on_port(0,0,EventBody::RegisteredController {group:0,channel,bank:0,index:2,value:(76u32<<25)|0x1ff_ffff}));}
    p.stage("native coarse ignores lower25","changing low25 bits cannot alter coarse12 pitch",&events);
    events.clear();unchanged.stage("independent unchanged coarse reference","held source continues original fine and coarse trajectory",&events);
    let error=p.measured().iter().zip(unchanged.measured()).flat_map(|(a,b)|
        [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
    assert!(error<1e-6,"native coarse lower25 bits changed PCM:{error}");
    for (ch, &expected) in expected.iter().enumerate() {assert!((pcm_fundamental(p.measured(),ch)-expected).abs()<1e-5);}
    for (id,channel) in [(282,4),(283,7)] {note(&mut events,ExactNoteKind::Off,id,channel,60);}
    let silence=p.stage("native channel tuning exact ends","full32bit configuration does not pin physical roots",&events);
    assert!(silence.iter().all(|v|*v<1e-8));assert_eq!(p.ended,[address(282,4,60),address(283,7,60)]);
    p
}

#[test]
fn native_rpn_fine_low_bits_and_coarse_upper_bits_have_independent_plugin_pcm_frequency() {
    let fixed=native_channel_tuning_precision(&[128]);let fragmented=native_channel_tuning_precision(&[1,7,31,127,13,64]);
    let error=fixed.pcm.iter().zip(&fragmented.pcm).flat_map(|(a,b)|
        [(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0f32,f32::max);
    assert!(error<2e-5,"native channel tuning precision depends on host partition:{error}");
    assert_eq!(fixed.ended_frames,fragmented.ended_frames);
}

#[test]
fn channel_rpn_tuning_modifies_native_absolute_attribute_and_pn3_pitch_once_in_pcm() {
    for partition in [&[128][..],&[1,7,31,127,13,64][..]] {
        let mut p=native_per_note_performance(partition,false);
        let mut events=EventList::with_capacity(16);
        cc(&mut events,4,74,127);cc(&mut events,7,74,127);
        legacy_tuning(&mut events,4,1,96,Some(0));legacy_tuning(&mut events,4,2,76,None);
        events.push(Event::on_port(0,0,EventBody::NoteOn2 {group:0,channel:4,note:60,velocity:u16::MAX,attribute_type:3,attribute:60*512+256}));
        events.push(Event::on_port(0,0,EventBody::NoteOn2 {group:0,channel:7,note:60,velocity:64u16<<9,attribute_type:0,attribute:0}));
        p.stage("channel tuning with native absolute attribute","attribute60.5 plus channel12.5 yields left13 semitones; physical7 stays240Hz",&events);
        assert!((pcm_fundamental(p.measured(),0)-240.*2f64.powf(13./12.)).abs()<1e-5,
            "channel tuning was lost or applied twice around absolute attribute pitch");
        assert!((pcm_fundamental(p.measured(),1)-240.).abs()<1e-5);
        events.clear();events.push(Event::on_port(0,0,EventBody::PerNoteCC {group:0,channel:4,note:60,cc:3,registered:true,value:(61u32<<25)|(1u32<<23)}));
        p.stage("channel tuning with native PN3 absolute pitch","absolutePN3=61.25 plus channel12.5 yields left13.75 semitones",&events);
        assert!((pcm_fundamental(p.measured(),0)-240.*2f64.powf(13.75/12.)).abs()<1e-5,
            "channel tuning was lost or applied twice around PN3 absolute pitch");
        assert!((pcm_fundamental(p.measured(),1)-240.).abs()<1e-5);
        events.clear();
        for (channel,velocity) in [(4,u16::MAX),(7,64u16<<9)] {events.push(Event::on_port(0,0,EventBody::NoteOff2 {group:0,channel,note:60,velocity,attribute_type:0,attribute:0}));}
        let silence=p.stage("native absolute pitch roots end","anonymous roots release without fabricated exact NoteEnds",&events);
        assert!(silence.iter().all(|v|*v<1e-8));assert!(p.ended.is_empty());
    }
}

#[test]
fn authored_set_rpn_notifies_later_script_without_configuring_native_channel_pitch_in_pcm() {
    for partition in [&[128][..],&[1,7,31,127,13,64][..]] {
        let mut p=Performance::new(false,partition);
        let instrument=crate::import::Instrument {groups:vec![Group::default()],scripts:vec![
            "on controller\nif ($CC_NUM=20)\nset_rpn(1,12288)\nend if\nend on".into(),
            "on init\ndeclare %first[128]\nend on\non note\n%first[$EVENT_NOTE] := $EVENT_ID\nend on\non rpn\nif ($RPN_ADDRESS=1 and $RPN_VALUE=12288)\nchange_vol(%first[60],-12000,0)\nend if\nend on".into()],..Default::default()};
        let (runtime,errors)=crate::engine::load_scripts(&instrument,Vec::new(),RATE as f64);
        assert!(errors.is_empty(),"{errors:?}");p.dsp.rack.parts[0].instrument_mut().unwrap().set_script(runtime);
        let mut events=EventList::with_capacity(8);note(&mut events,ExactNoteKind::On,284,4,60);
        let full=p.stage("authored RPN notification source","held root renders240Hz before script-generated parameter notification",&events);
        events.clear();cc(&mut events,4,20,1);
        let level=p.stage("authored RPN later-slot notification","later script hearsRPN1value12288 and authors minus12dB; native pitch stays240Hz",&events);
        for ch in 0..2 {assert!((level[ch]/full[ch]-10f64.powf(-12./20.)).abs()<1e-5,
            "authored RPN notification did not reach the later script");}
        assert!((pcm_fundamental(p.measured(),0)-240.).abs()<1e-5,
            "authored set_rpn incorrectly configured native channel fine tuning");
        events.clear();note(&mut events,ExactNoteKind::Off,284,4,60);
        p.stage("authored RPN notification root end","script notification does not retain host root",&events);
        assert_eq!(p.ended,[address(284,4,60)]);
    }
}

// A separate source-domain oracle: evaluate the cubic interpolant in f64,
// then feed its output to an untuned voice. This reuses the authored amplifier
// while testing every source position, rather than comparing two pitch routes.
fn wide_pitch_reference(source:&[[f32;2]], ratio:f64) -> Vec<[f32;2]> {
    let step=(ratio*4294967296.).floor() as u64;
    let source_duration=source.len() as f64*4294967296./step as f64;
    let end=((source.len() as u64)<<32).div_ceil(step) as usize;
    (0..end).map(|frame| {
        let position=frame as u64*step;
        let index=(position>>32) as i64;
        let t=((position as u32)>>8) as f64/16777216.;
        std::array::from_fn(|channel| {
            let sample=|at:i64|source.get(at as usize).map_or(0.,|v|v[channel] as f64);
            let a=sample(index-1);let b=sample(index);let c=sample(index+1);let d=sample(index+2);
            // Cubic Hermite basis with central-difference endpoint slopes.
            let m0=(c-a)*0.5;let m1=(d-b)*0.5;
            let interpolated=(2.*t.powi(3)-3.*t.powi(2)+1.)*b+(t.powi(3)-2.*t.powi(2)+t)*m0
                +(-2.*t.powi(3)+3.*t.powi(2))*c+(t.powi(3)-t.powi(2))*m1;
            // The finite source fades over its final millisecond, measured in
            // output time. Its fractional duration precedes the integer EOF
            // by less than one frame; compensate the unit reference's own fade.
            let fade=((source_duration-frame as f64)/48.).clamp(0.,1.);
            let reference_fade=((end-frame) as f64/48.).min(1.);
            (interpolated*fade/reference_fade) as f32
        })
    }).collect()
}

fn wide_pitch_performance(partition:&'static [usize], scripted:bool, case:u8) -> Performance {
    use crate::modulation::Ahdsr;
    let (key, semitones, source_hz, source_frames) = match case {
        0 => (127,67.,2.,257_024),
        1 => (60,63.,2.,257_024),
        2 => (60,120.,2.,257_024),
        3 => (60,-120.,8192.,128),
        _ => unreachable!(),
    };
    let ratio=2_f64.powf(semitones/12.);
    let source:Vec<_>=(0..source_frames).map(|frame| {
        [(0.02*(std::f64::consts::TAU*source_hz*frame as f64/RATE as f64).sin()) as f32;2]
    }).collect();
    let reference=wide_pitch_reference(&source,ratio);
    let end=reference.len();
    let group=Group {native_volume_env:true,volume_env:Some(Ahdsr {
        attack_curve:0.,attack_ms:0.,hold_ms:0.,decay_ms:0.,sustain:1.,release_ms:5000.,
        unknown_flag:0,unknown_tail:Vec::new() }),..Group::default()};
    let mut actual=Performance::with_group(scripted,partition,group.clone());
    let mut oracle=Performance::with_group(scripted,partition,group.clone());
    for (performance,frames) in [(&mut actual,source),(&mut oracle,reference)] {
        performance.dsp.rack.parts[0].instrument_mut().unwrap().set_bank(Some(Box::new(Bank::from_samples(
            vec![group.clone()],vec![Zone {root:60,..Zone::default()}],
            vec![(PathBuf::new(),Sample {rate:RATE as u32,frames})]).unwrap())));
    }
    let mut events=EventList::with_capacity(8);
    let mut expected=EventList::with_capacity(8);
    if case==1 {legacy_tuning(&mut events,4,2,127,Some(0));}
    note(&mut events,ExactNoteKind::On,290,4,key);
    note(&mut expected,ExactNoteKind::On,290,4,60);
    if case>=2 {expression(&mut events,290,4,2,semitones);}
    actual.stage_for("wide pitch attack","full authored pitch reaches the finite source",&events,64,0);
    oracle.stage_for("independently resampled attack","unit-pitch reference follows the source-domain polynomial",&expected,64,0);
    assert!(actual.ended.is_empty());
    events.clear();expected.clear();
    note(&mut events,ExactNoteKind::Off,290,4,key);
    note(&mut expected,ExactNoteKind::Off,290,4,60);
    actual.stage_for("wide finite release","source EOF wins before the long release envelope",&events,end+128-64,0);
    oracle.stage_for("independently resampled release","the same amplifier ends at the known output length",&expected,end+128-64,0);
    assert_eq!(actual.ended,[address(290,4,key)]);
    assert_eq!(actual.ended_frames,[end],"case{case}: independently derived fixed-point source EOF");
    assert_eq!(oracle.ended_frames,[end]);
    assert!(actual.pcm[end..].iter().all(|v|*v==[0.;2]),"case{case}: source tail continued past EOF");
    let error=actual.pcm.iter().zip(&oracle.pcm).flat_map(|(a,b)|[(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0_f32,f32::max);
    assert!(error<2e-6,"case{case}: complete wide-pitch PCM differs from independently resampled source by {error}");
    // Linear zero-crossing interpolation is biased by the final millisecond
    // of source fade. Exclude that output-time interval from the observation,
    // while retaining the fade and zero padding in the complete PCM check.
    let unfaded_end=(source_frames as f64/ratio-0.001*RATE as f64).floor() as usize;
    let hz=pcm_fundamental(&actual.pcm[1..unfaded_end],0);
    let expected_hz=source_hz*ratio;
    let tolerance=if case>=2 {0.005}else {0.00001};
    assert!((hz-expected_hz).abs()<tolerance,"case{case}: audible pitch {hz}Hz expected {expected_hz}Hz");
    actual
}

#[test]
fn full_key_coarse_and_exact_pitch_ranges_reach_finite_plugin_pcm_without_heap() {
    for scripted in [false,true] {
        for case in 0..4 {
            let fixed=wide_pitch_performance(&[128],scripted,case);
            let fragments=wide_pitch_performance(&[1,7,31,127,13,64],scripted,case);
            assert_eq!(fixed.ended_frames,fragments.ended_frames);
            let error=fixed.pcm.iter().zip(&fragments.pcm).flat_map(|(a,b)|[(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0_f32,f32::max);
            assert!(error<2e-6,"case{case}, scripted={scripted}: host partition changed wide-pitch audio by {error}");
        }
    }
}
