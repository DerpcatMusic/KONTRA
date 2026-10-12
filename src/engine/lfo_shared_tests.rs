//! Shared saved sources against exterior per-owner tuning and gain trajectories.
use crate::{
    audio::Sample,
    engine::{Bank, Engine, HostExpression, HostNote, HostPattern, params::Address},
    import::{Group, Instrument, Zone},
};
use ni_file::kontakt::{
    Chunk, StructuredObject,
    objects::{Group as RawGroup, Lfo, LfoRecord},
};
const END: usize = 2502;
const WINDOWS: [usize; 10] = [0, 257, 450, 961, 1217, 1474, 1731, 1988, 2245, END];
const STARTS: [usize; 5] = [0, 37, 73, 1217, 1474];
const GROUPS: [usize; 5] = [0, 0, 1, 0, 0];
fn object(id: u16, version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Chunk {
    let mut data = vec![1];
    data.extend(version.to_le_bytes());
    for part in [private, public, children] {
        data.extend((part.len() as u32).to_le_bytes());
        data.extend(part);
    }
    Chunk { id, data }
}
fn name(out: &mut Vec<u8>, text: &str) {
    out.extend((text.len() as u32).to_le_bytes());
    out.extend(text.as_bytes());
}
fn imported() -> (tempfile::TempDir, Instrument) {
    let raw = Lfo {
        structured: false,
        version: 0x71,
        waveform: 6,
        initial_values: [0., 3.75, 0.5, 0.23],
        records: [
            LfoRecord {
                flag: false,
                values: [0., 0., 0.],
            },
            LfoRecord {
                flag: true,
                values: [-1., 0., 0.],
            },
        ],
        trailing_flag: false,
        trailing_values: Some([0., 0., 1., 0., 0.]),
        additional_flag: None,
    };
    let mut source = Vec::new();
    raw.to_chunk().unwrap().write(&mut source).unwrap();
    let mut private = 2u32.to_le_bytes().to_vec();
    for (param, depth) in [("pitch", 0.125f32), ("volume", 0.6)] {
        name(&mut private, param);
        private.extend(depth.to_le_bytes());
        private.extend((-1i16).to_le_bytes());
        private.push(0x10);
        private.extend(0u16.to_le_bytes());
        name(&mut private, param);
        private.extend([0, 0]);
    }
    private.extend([0, 0, 0, 0]);
    private.extend(0u32.to_le_bytes());
    name(&mut private, "Shared Clock");
    private.extend(1u32.to_le_bytes());
    let mut slots = vec![1];
    object(0x0d, 0x80, &private, &[], &source)
        .write(&mut slots)
        .unwrap();
    slots.extend([0; 15]);
    let chunk = object(0x3b, 0x10, &[], &slots, &[]);
    let decoded = crate::modulation::read_group(&RawGroup(StructuredObject {
        version: 0x95,
        private_data: vec![],
        public_data: vec![],
        children: vec![chunk],
    }))
    .unwrap();
    assert_eq!(decoded.pitch_lfos.len(), 1);
    assert_eq!(decoded.volume_lfos.len(), 1);
    assert!(decoded.pitch_lfos[0].shared && decoded.volume_lfos[0].source.shared);
    assert_eq!(
        decoded.pitch_lfos[0].start_phase, 0.23,
        "saved reset phase stays in metadata"
    );
    let group = Group {
        pitch_lfos: decoded.pitch_lfos,
        volume_lfos: decoded.volume_lfos,
        modulators: decoded.modulators,
        ..Default::default()
    };
    let instrument = Instrument {
        groups: vec![group.clone(), group],
        zones: vec![
            Zone {
                group: 0,
                low_key: 60,
                high_key: 60,
                ..Default::default()
            },
            Zone {
                group: 1,
                root: 61,
                low_key: 61,
                high_key: 61,
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("literal-source.nki");
    // The component was parsed above; this preset identity exercises the actual
    // binary cache, without claiming it is a complete serialized NKI container.
    std::fs::write(&path, &source).unwrap();
    let cache = dir.path().join("cache");
    crate::cache::store(&cache, &path, 0, &instrument);
    let restored = crate::cache::load(&cache, &path, 0).unwrap();
    assert_eq!(
        serde_json::to_value(&restored.groups).unwrap(),
        serde_json::to_value(&instrument.groups).unwrap()
    );
    (dir, restored)
}
fn bank(i: &Instrument, plain: bool) -> Box<Bank> {
    let mut groups = i.groups.clone();
    if plain {
        for group in &mut groups {
            group.pitch_lfos.clear();
            group.volume_lfos.clear();
        }
    }
    Box::new(
        Bank::from_samples(
            groups,
            i.zones.clone(),
            vec![(
                Default::default(),
                Sample {
                    rate: 48000,
                    frames: (0..32768)
                        .map(|n| {
                            let p = std::f64::consts::TAU * n as f64 / 200.;
                            [(0.025 * p.sin() + 0.007 * (p * 2.).sin()) as f32; 2]
                        })
                        .collect(),
                },
            )],
        )
        .unwrap(),
    )
}
fn engine(i: &Instrument, plain: bool) -> Engine {
    let mut e = Engine::default();
    e.reset(48000.);
    e.attack = 0.;
    e.release = 0.005;
    e.set_bank(Some(bank(i, plain)));
    e
}
fn host(index: usize) -> HostNote {
    HostNote {
        port: 0,
        channel: if index == 1 || index == 2 { 7 } else { 4 },
        key: if index == 2 { 61 } else { 60 },
        id: 100 + index as i32,
        clap: true,
    }
}
fn pattern(n: HostNote) -> HostPattern {
    HostPattern {
        port: 0,
        channel: i32::from(n.channel),
        key: i32::from(n.key),
        id: n.id,
        clap: true,
    }
}
fn step() -> f64 {
    f64::from(3.75f32 * (1500f64.recip() as f32))
}
// Closed-form phase integrals and once-selected native receiver suffixes. Neither
// per-voice sampling nor MIDI subdivisions advance a shared source a second time.
fn raw(group: usize, frame: usize, receiver_entry: usize, cursor: usize) -> f32 {
    let (base, end, elapsed): (usize, usize, usize) = match frame {
        0..=256 if group == 0 => (0, 257, 0),
        73..=256 => (73, 257, 0),
        257..=449 => (257, 450, if group == 0 { 257 } else { 184 }),
        450..=960 => (450, 961, if group == 0 { 450 } else { 377 }),
        1217..=1473 if group == 0 => (1217, 1474, 961),
        1474..=1730 if group == 0 => (1474, 1731, 0),
        1731..=1987 if group == 0 => (1731, 1988, 257),
        1988..=2244 if group == 0 => (1988, 2245, 514),
        _ => return 0.,
    };
    let count = (end - base).div_ceil(32);
    let effective_entry = receiver_entry.max(base);
    let suffix = count - (end - effective_entry).div_ceil(32);
    let phase = ((elapsed as f64 / 32. + (suffix + cursor) as f64) * step()).rem_euclid(1.);
    if phase < 0.5 {
        (4. * (phase - 0.25)) as f32
    } else {
        (4. * (0.75 - phase)) as f32
    }
}
fn trajectories() -> [[Vec<f32>; 2]; 5] {
    std::array::from_fn(|index| {
        let mut out = [vec![0.; END], vec![1.; END]];
        let (mut old_pitch, mut pitch, mut old_gain, mut gain) = (0., 0., 1., 1.);
        let mut initialized = false;
        let (mut receiver_entry, mut first_boundary) = (STARTS[index], STARTS[index]);
        for frame in STARTS[index]..END {
            let bypass = GROUPS[index] == 0 && (117..181).contains(&frame);
            let age = frame - STARTS[index];
            if frame == STARTS[index] || WINDOWS.contains(&frame) {
                receiver_entry = frame;
                let effective_entry = if GROUPS[index] == 1 && frame < 73 {
                    73
                } else {
                    frame
                };
                first_boundary =
                    effective_entry + ((32 - ((effective_entry - STARTS[index]) & 31)) & 31);
            }
            if !bypass && (age % 32 == 0 || !initialized) {
                old_pitch = pitch;
                old_gain = gain;
                let cursor = frame.saturating_sub(first_boundary) / 32;
                let source = raw(GROUPS[index], frame, receiver_entry, cursor);
                let depth = if GROUPS[index] == 0 && (221..1474).contains(&frame) {
                    0.25
                } else {
                    0.125
                };
                pitch = source * (12. * depth);
                gain = 0.7 + 0.3 * source;
                if !initialized {
                    old_pitch = pitch;
                    old_gain = gain;
                    initialized = true;
                }
            }
            let fraction = (age % 32) as f32 / 32.;
            out[0][frame] = if bypass {
                0.
            } else {
                old_pitch + (pitch - old_pitch) * fraction
            };
            out[1][frame] = if bypass {
                1.
            } else {
                old_gain + (gain - old_gain) * fraction
            };
            if index == 0 && (101..149).contains(&frame) {
                out[1][frame] = 0.;
            }
        }
        out
    })
}
fn performance(i: &Instrument, fragments: &[usize]) -> ([Vec<f32>; 2], [Option<u64>; 5]) {
    let (mut actual, mut expected) = (engine(i, false), engine(i, true));
    let reference = trajectories();
    let mut incoming = Some(bank(i, false));
    let mut reset = Some(bank(i, false));
    let mut reset_plain = Some(bank(i, true));
    let (mut retired_fill, mut retired_reset, mut retired_plain) = (None, None, None);
    let mut pcm = [vec![0.; END], vec![0.; END]];
    let mut oracle = [vec![0.; END], vec![0.; END]];
    let (mut at, mut fragment, mut window) = (0, 0, 0);
    let mut first_admission = None;
    let mut phase_before_idle = 0.;
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            while at < END {
                if at == WINDOWS[window] {
                    let width = WINDOWS[window + 1] - at;
                    actual.begin_audio_block(width, 1, false);
                    expected.begin_audio_block(width, 1, false);
                    window += 1;
                }
                if at == 450 {
                    let before = actual.state.shared_lfos.rows[0].state.get().phase;
                    retired_fill = actual.upgrade_bank(incoming.take().unwrap());
                    assert_eq!(
                        actual.state.shared_lfos.rows[0].state.get().phase,
                        before,
                        "compatible fill keeps the shared source owner"
                    );
                }
                if at == 961 {
                    phase_before_idle = actual.state.shared_lfos.rows[0].state.get().phase;
                }
                if at == 1217 {
                    assert_eq!(
                        actual.state.shared_lfos.rows[0].state.get().phase,
                        phase_before_idle,
                        "idle windows do not consume a source with no eligible voice"
                    );
                    assert!((phase_before_idle - 961f64 / 32. * step()).abs() < 1e-14);
                }
                if at == 1474 {
                    retired_reset = actual.set_bank(reset.take());
                    retired_plain = expected.set_bank(reset_plain.take());
                    assert_eq!(
                        actual.state.shared_lfos.rows[0].state.get().phase,
                        0.,
                        "destructive installation resets shared source phase"
                    );
                    actual.mark_host_notes();
                    expected.mark_host_notes();
                    assert_eq!(actual.host_note_end_at(host(3)), Some(1474));
                    assert_eq!(expected.host_note_end_at(host(3)), Some(1474));
                }
                for (index, start) in STARTS.into_iter().enumerate() {
                    if at == start {
                        for e in [&mut actual, &mut expected] {
                            assert!(e.host_note_on(
                                host(index),
                                host(index).channel,
                                host(index).key,
                                127,
                                0.
                            ));
                            e.host_expression(
                                pattern(host(index)),
                                HostExpression::Pan(if index == 1 { 1. } else { -1. }),
                            );
                        }
                        if index == 0 {
                            first_admission = Some(actual.player.voices[0].admission);
                        }
                    }
                }
                if at == 101 || at == 149 {
                    actual.host_expression(
                        pattern(host(0)),
                        HostExpression::Gain(if at == 101 { 0. } else { 1. }),
                    );
                }
                if at == 117 || at == 181 {
                    assert!(actual.write(
                        Address::PitchSourceBypass(0, 0),
                        if at == 117 { 1. } else { 0. }
                    ));
                }
                if at == 221 {
                    assert!(actual.write(
                        Address::PitchSourceIntensity {
                            group: 0,
                            slot: 0,
                            target: 0,
                            bipolar: false
                        },
                        0.25
                    ));
                }
                for (index, off) in [(0, 450), (1, 512), (2, 512), (3, 1320), (4, 1731)] {
                    if at == off {
                        for e in [&mut actual, &mut expected] {
                            e.host_note_off(pattern(host(index)));
                        }
                    }
                }
                let phase = actual.state.shared_lfos.rows[0].state.get().phase;
                actual.render_checked(&mut [], &mut []).unwrap();
                assert_eq!(
                    actual.state.shared_lfos.rows[0].state.get().phase,
                    phase,
                    "empty fragments cannot commit source phase"
                );
                let stop = [
                    37, 73, 101, 117, 149, 181, 221, 257, 450, 512, 961, 1217, 1320, 1474, 1731,
                    1988, 2245, END,
                ]
                .into_iter()
                .filter(|stop| *stop > at)
                .min()
                .unwrap();
                let n = fragments[fragment % fragments.len()].min(stop - at);
                let [l, r] = &mut pcm;
                actual
                    .render_checked(&mut l[at..at + n], &mut r[at..at + n])
                    .unwrap();
                if at == 0 {
                    let row = actual.state.shared_lfos.rows[0].state.get();
                    assert_eq!(row.producer, first_admission);
                    assert!(row.committed);
                    assert_eq!((row.base, row.end, row.count), (0, 257, 9));
                    assert_eq!(
                        row.points[0], -1.,
                        "free-running ignores saved reset phase0.23"
                    );
                    assert!(
                        (row.phase - 257f64 / 32. * step()).abs() < 1e-14,
                        "one producer commits the complete padded-window phase once"
                    );
                }
                for frame in at..at + n {
                    for index in 0..5 {
                        if frame >= STARTS[index] {
                            expected.host_expression(
                                pattern(host(index)),
                                HostExpression::Tune(reference[index][0][frame]),
                            );
                            expected.host_expression(
                                pattern(host(index)),
                                HostExpression::Gain(reference[index][1][frame]),
                            );
                        }
                    }
                    let [l, r] = &mut oracle;
                    expected
                        .render_checked(&mut l[frame..frame + 1], &mut r[frame..frame + 1])
                        .unwrap();
                }
                at += n;
                fragment += 1;
            }
            actual.mark_host_notes();
            expected.mark_host_notes();
        }),
        0
    );
    let actual_ends = std::array::from_fn(|index| actual.host_note_end_at(host(index)));
    let expected_ends = std::array::from_fn(|index| expected.host_note_end_at(host(index)));
    assert_eq!(actual_ends, expected_ends);
    assert!(actual_ends.iter().all(Option::is_some));
    let error = pcm
        .iter()
        .flatten()
        .zip(oracle.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0_f32, f32::max);
    assert!(error < 3e-5, "shared raw window complete PCM error {error}");
    assert!(pcm[0][181..450].iter().any(|x| x.abs() > 0.01));
    assert!(
        pcm.iter()
            .all(|channel| channel[END - 128..].iter().all(|x| *x == 0.))
    );
    drop(retired_fill);
    drop(retired_reset);
    drop(retired_plain);
    (pcm, actual_ends)
}
#[test]
fn shared_source_windows_overlap_pitch_volume_idle_and_bank_handoffs_preserve_complete_pcm_without_heap()
 {
    let (_cache, instrument) = imported();
    let (whole, whole_ends) = performance(&instrument, &[128]);
    let (fragmented, fragmented_ends) = performance(&instrument, &[1, 7, 31, 127]);
    assert_eq!(whole_ends, fragmented_ends);
    let error = whole
        .iter()
        .flatten()
        .zip(fragmented.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0_f32, f32::max);
    assert!(
        error < 3e-5,
        "MIDI/render subdivision changed the balanced host-window PCM by{error}"
    );
}

#[derive(Default)]
struct ColdReference {
    generation: usize,
    cursor: usize,
    tail: usize,
    initialized: bool,
    previous_pitch: f32,
    pitch: f32,
    previous_gain: f32,
    gain: f32,
}
impl ColdReference {
    fn point(&mut self, frame: usize, born: usize) -> (f32, f32) {
        let (base, end, phase, count) = if frame < 257 {
            (1, 257, 0., 8)
        } else if frame < 450 {
            (257, 450, 8., 7)
        } else {
            (450, 707, 14.03125, 9)
        };
        if !self.initialized || self.generation != base {
            self.generation = base;
            self.cursor = 0;
            self.tail = count - (end - frame).div_ceil(32);
        }
        let age = frame - born;
        if !self.initialized || age % 32 == 0 {
            let phase = (phase + (self.tail + self.cursor) as f64) * step();
            let phase = phase.rem_euclid(1.);
            let raw = if phase < 0.5 {
                (4. * (phase - 0.25)) as f32
            } else {
                (4. * (0.75 - phase)) as f32
            };
            self.previous_pitch = self.pitch;
            self.previous_gain = self.gain;
            self.pitch = 1.5 * raw;
            self.gain = 0.7 + 0.3 * raw;
            if !self.initialized {
                self.previous_pitch = self.pitch;
                self.previous_gain = self.gain;
                self.initialized = true;
            }
            self.cursor += 1;
        }
        let fraction = (age % 32) as f32 / 32.;
        (
            self.previous_pitch + (self.pitch - self.previous_pitch) * fraction,
            self.previous_gain + (self.gain - self.previous_gain) * fraction,
        )
    }
}
#[test]
fn older_actual_cold_stream_cannot_nominate_before_ready_younger_shared_voice() {
    use crate::engine::stream::tests::ManualWorker;
    let (_cache, mut instrument) = imported();
    let files = tempfile::tempdir().unwrap();
    let paths = [
        files.path().join("cold.wav"),
        files.path().join("ready.wav"),
    ];
    let frames: [Vec<[f32; 2]>; 2] = std::array::from_fn(|index| {
        (0..if index == 0 { 131072 } else { 32768 })
            .map(|n| {
                let p = std::f64::consts::TAU * n as f64 / 200.;
                [(0.025 * p.sin()) as f32; 2]
            })
            .collect()
    });
    for (path, frames) in paths.iter().zip(&frames) {
        let mut wav = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 48000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for frame in frames {
            for value in frame {
                wav.write_sample(*value).unwrap();
            }
        }
        wav.finalize().unwrap();
    }
    instrument.groups.truncate(1);
    instrument.zones = vec![
        Zone {
            group: 0,
            sample: paths[0].clone(),
            root: 59,
            low_key: 59,
            high_key: 59,
            ..Default::default()
        },
        Zone {
            group: 0,
            sample: paths[1].clone(),
            root: 60,
            low_key: 60,
            high_key: 60,
            ..Default::default()
        },
    ];
    let streamed = Bank::load_bare(&instrument).unwrap();
    let mut worker = ManualWorker::paused(&streamed);
    let mut plain = instrument.groups.clone();
    plain[0].pitch_lfos.clear();
    plain[0].volume_lfos.clear();
    let resident = Bank::from_samples(
        plain,
        instrument.zones.clone(),
        paths
            .into_iter()
            .zip(frames)
            .map(|(path, frames)| {
                (
                    path,
                    Sample {
                        rate: 48000,
                        frames,
                    },
                )
            })
            .collect(),
    )
    .unwrap();
    let (mut actual, mut expected) = (Engine::default(), Engine::default());
    for e in [&mut actual, &mut expected] {
        e.reset(48000.);
        e.attack = 0.;
        e.release = 0.005;
    }
    actual.set_bank(Some(Box::new(streamed)));
    expected.set_bank(Some(Box::new(resident)));
    let cold = HostNote {
        key: 59,
        id: 201,
        ..host(0)
    };
    let ready = HostNote { id: 202, ..host(1) };
    let mut pcm = [[0.; 707]; 2];
    let mut oracle = [[0.; 707]; 2];
    let (mut old_admission, mut young_admission, mut cold_slot) = (None, None, None);
    let mut cold_started = None;
    let (mut cold_curve, mut ready_curve) = (ColdReference::default(), ColdReference::default());
    for frame in 0..707 {
        if frame == 0 || frame == 257 || frame == 450 {
            let width = if frame == 0 {
                257
            } else if frame == 257 {
                193
            } else {
                257
            };
            for e in [&mut actual, &mut expected] {
                e.begin_audio_block(width, 1, false);
            }
        }
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                if frame == 0 {
                    // Live high tuning demands actual taps outside the resident
                    // head; it is not a forged Pending enum or fabricated packet.
                    assert!(actual.host_note_on(cold, 4, 59, 127, 72.));
                    actual.host_expression(pattern(cold), HostExpression::Pan(-1.));
                    old_admission = Some(actual.player.voices[0].admission);
                    cold_slot = Some(actual.player.voices[0].stream.unwrap().slot);
                }
                if frame == 1 {
                    for e in [&mut actual, &mut expected] {
                        assert!(e.host_note_on(ready, 7, 60, 127, 0.));
                        e.host_expression(pattern(ready), HostExpression::Pan(1.));
                    }
                    young_admission = Some(actual.player.voices[1].admission);
                }
                if frame == 129 {
                    for e in [&mut actual, &mut expected] {
                        e.host_note_off(pattern(cold));
                        e.host_note_off(pattern(ready));
                    }
                }
            }),
            0
        );
        // Decoder publication belongs to the fixture's sole off-callback
        // worker. Until64 only the resident younger voice can become ready.
        if frame >= 64 {
            worker.serve(cold_slot.unwrap());
        }
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                let [l, r] = &mut pcm;
                actual
                    .render_checked(&mut l[frame..frame + 1], &mut r[frame..frame + 1])
                    .unwrap();
                if frame == 0 {
                    assert!(actual.player.voices[0].onset == crate::engine::voice::Onset::Waiting);
                    let owner = actual.state.shared_lfos.rows[0].state.get();
                    assert!(!owner.committed);
                    assert_eq!(owner.phase, 0.);
                    assert_eq!(owner.producer, None);
                }
                if frame == 1 {
                    let owner = actual.state.shared_lfos.rows[0].state.get();
                    assert_eq!(owner.producer, young_admission);
                    assert_ne!(young_admission, old_admission);
                    assert_eq!((owner.base, owner.end, owner.count), (1, 257, 8));
                    assert!((owner.phase - 8. * step()).abs() < 1e-14);
                }
                if cold_started.is_none()
                    && actual.player.voices.iter().any(|voice| {
                        voice.note == 59 && voice.onset == crate::engine::voice::Onset::Started
                    })
                {
                    assert!(
                        (64..129).contains(&frame),
                        "actual decoder publication controls readiness"
                    );
                    cold_started = Some(frame);
                    // I/O completion is observed, not inferred from source clocks.
                    // From this onset onward a full resident source supplies an
                    // independent musical reference with the same exact root.
                    assert!(expected.host_note_on(cold, 4, 59, 127, 0.));
                    // Independent octave ratio stays separate from the small
                    // per-sample expression; adding72 in f32 would discard
                    // precision that the actual source clock retains.
                    expected.player.voices.last_mut().unwrap().tune *= 64.;
                    expected.host_expression(pattern(cold), HostExpression::Pan(-1.));
                }
                if frame >= 1 {
                    let (tune, gain) = ready_curve.point(frame, 1);
                    expected.host_expression(pattern(ready), HostExpression::Tune(tune));
                    expected.host_expression(pattern(ready), HostExpression::Gain(gain));
                }
                if let Some(start) = cold_started {
                    let (tune, gain) = cold_curve.point(frame, start);
                    expected.host_expression(pattern(cold), HostExpression::Tune(tune));
                    expected.host_expression(pattern(cold), HostExpression::Gain(gain));
                }
                if frame == 257 {
                    let owner = actual.state.shared_lfos.rows[0].state.get();
                    assert_eq!(
                        owner.producer, old_admission,
                        "the older now-Started owner wins only the next generation"
                    );
                    assert!((owner.phase - (8. + 193f64 / 32.) * step()).abs() < 1e-14);
                }
                let [l, r] = &mut oracle;
                expected
                    .render_checked(&mut l[frame..frame + 1], &mut r[frame..frame + 1])
                    .unwrap();
            }),
            0
        );
    }
    assert!(cold_started.is_some());
    actual.mark_host_notes();
    expected.mark_host_notes();
    for owner in [cold, ready] {
        assert_eq!(
            actual.host_note_end_at(owner),
            expected.host_note_end_at(owner)
        );
        assert!(actual.host_note_end_at(owner).is_some());
    }
    let error = pcm
        .iter()
        .flatten()
        .zip(oracle.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0_f32, f32::max);
    assert!(
        error < 3e-5,
        "actual cold/shared nomination complete waveform error {error}"
    );
    assert!(pcm[0][64..129].iter().any(|x| x.abs() > 0.01));
    assert!(pcm[1][1..64].iter().any(|x| x.abs() > 0.01));
    assert!(
        pcm.iter()
            .all(|channel| channel[579..].iter().all(|x| *x == 0.))
    );
}
