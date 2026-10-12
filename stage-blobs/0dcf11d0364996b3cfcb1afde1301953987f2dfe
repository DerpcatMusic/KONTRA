//! Native private-record policy, tested at the plugin's audible boundary.
use super::*;
use ni_file::kontakt::objects::ProgramDataPrivateParams;

const READBACK: &str = "on init\ndeclare %seen[3]\nmake_persistent(%seen)\n%seen[0] := get_engine_par($ENGINE_PAR_VOLUME,-1,-1,-1)\nend on\non controller\nif ($CC_NUM=1)\nset_engine_par($ENGINE_PAR_VOLUME,500000,-1,-1,-1)\n%seen[1] := get_engine_par($ENGINE_PAR_VOLUME,-1,-1,-1)\nend if\nif ($CC_NUM=3)\nset_engine_par($ENGINE_PAR_VOLUME,250000,-1,-1,-1)\n%seen[1] := get_engine_par($ENGINE_PAR_VOLUME,-1,-1,-1)\nend if\nif ($CC_NUM=2)\n%seen[2] := get_engine_par($ENGINE_PAR_VOLUME,-1,-1,-1)\nend if\nend on";

fn private(version: u16, enabled: bool, maximum_db: i32) -> ProgramDataPrivateParams {
    ProgramDataPrivateParams {
        version,
        data: [
            0u32.to_le_bytes().as_slice(),
            &[u8::from(enabled), 1],
            maximum_db.to_le_bytes().as_slice(),
            &[0xa5, 0, 0xff],
        ]
        .concat(),
    }
}

struct Source {
    _dir: tempfile::TempDir,
    zone: Zone,
}

impl Source {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gain-240-hz.wav");
        let mut wav = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 2,
                sample_rate: RATE as u32,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        // The same independently known source used by Performance; forty
        // complete periods also make every loop wrap phase-continuous.
        for i in 0..8_000 {
            let phase = std::f64::consts::TAU * i as f64 / 200.;
            let value = (0.02 * phase.sin() + 0.006 * (2. * phase).sin()) as f32;
            wav.write_sample(value).unwrap();
            wav.write_sample(value).unwrap();
        }
        wav.finalize().unwrap();
        Self {
            _dir: dir,
            zone: Zone {
                sample: path,
                root: 60,
                loop_range: Some(Loop {
                    start: 0,
                    end: 8_000,
                    crossfade: 0,
                    until_release: false,
                    alternating: false,
                }),
                ..Zone::default()
            },
        }
    }

    fn instrument(
        &self,
        record: Option<ProgramDataPrivateParams>,
        scripted: bool,
    ) -> crate::import::Instrument {
        crate::import::Instrument {
            program: crate::import::ProgramControls {
                gain: 0.25,
                pan: 0.,
                tune: 0.,
            },
            native_program_private: record,
            groups: vec![Group::default()],
            zones: vec![self.zone.clone()],
            scripts: if scripted {
                vec![READBACK.into()]
            } else {
                vec![]
            },
            ..Default::default()
        }
    }

    fn performance(
        &self,
        partition: &'static [usize],
        record: Option<ProgramDataPrivateParams>,
        scripted: bool,
    ) -> Performance {
        let mut p = Performance::new(false, partition);
        let instrument = self.instrument(record, scripted);
        p.dsp.rack.parts[0].instrument_mut().unwrap().set_bank(Some(Box::new(Bank::load(&instrument).unwrap())));
        if scripted {
            let (runtime, errors) =
                crate::engine::load_scripts(&instrument, Vec::new(), RATE as f64);
            assert!(errors.is_empty(), "{errors:?}");
            p.dsp.rack.parts[0].instrument_mut().unwrap().set_script(runtime);
        }
        p
    }
}

fn seen(p: &Performance, index: usize) -> i32 {
    let saved = p.dsp.rack.parts[0].instrument().unwrap().script().unwrap().persistence();
    let crate::ksp::Value::IntArray(values) = &saved[0]["%seen"] else {
        panic!("Volume readback")
    };
    values[index]
}

fn check_level(p: &Performance, reference: &Performance, gain: f64) {
    for channel in 0..2 {
        ratio(
            rms(p.measured())[channel] / rms(reference.measured())[channel],
            gain,
            "private-record instrument gain must reach settled plugin RMS",
        );
        assert!((pcm_fundamental(p.measured(), channel) - 240.).abs() < 2e-5);
    }
    let error = p
        .measured()
        .iter()
        .zip(reference.measured())
        .flat_map(|(a, b)| [0, 1].map(move |ch| (f64::from(a[ch]) - f64::from(b[ch]) * gain).abs()))
        .fold(0f64, f64::max);
    assert!(
        error < 2e-7,
        "settled source PCM is not the independently expected gain: {error}"
    );
}

// Test-only exterior program scalar. Protocol conversion expectations remain
// supplied by each scenario; neither target authority nor emitted state is read
// from the production amplifier. Each Sampler callback is its actual host span.
#[derive(Default)]
struct NativeOracle {
    stage: usize,
    block: usize,
    current: Option<[f32; 2]>,
    previous_target: Option<[f32; 2]>,
    fresh: bool,
}
impl NativeOracle {
    fn check(&mut self, p: &Performance, reference: &Performance, target: [f32; 2]) {
        assert_eq!(p.pcm.len(), reference.pcm.len());
        let latest = p.stages.len() - 1;
        for stage in self.stage..p.stages.len() {
            let start = p.stages[stage]["start_frame"].as_u64().unwrap() as usize;
            let end = p.stages[stage]["end_frame"].as_u64().unwrap() as usize;
            let value = if stage == latest {
                target
            } else {
                self.previous_target.unwrap_or([0.25, 0.])
            };
            if stage == latest && self.fresh {
                self.current = None;
                self.fresh = false;
            }
            let mut at = start;
            while at < end {
                let n = p.partition[self.block % p.partition.len()].min(end - at);
                self.block += 1;
                let retained = self.current.get_or_insert(value);
                let ratio = (retained[0] + 1e-12).abs() / (value[0] + 1e-12).abs();
                let gain_moves = ratio > 1.001 || ratio < 0.999;
                let pan_moves =
                    (retained[1] - value[1]).abs() > (10f32 / RATE as f32) + (10f32 / RATE as f32);
                for i in at..at + n {
                    let gain = if gain_moves { retained[0] } else { value[0] };
                    let pan = if pan_moves { retained[1] } else { value[1] };
                    if stage == latest {
                        for ch in 0..2 {
                            let balance = if ch == 0 {
                                1. - pan.max(0.)
                            } else {
                                1. + pan.min(0.)
                            };
                            let expected = reference.pcm[i][ch] * gain * balance;
                            assert!(
                                (p.pcm[i][ch] - expected).abs() < 2e-7,
                                "native program waveform frame{i} channel{ch}: {} != {expected}",
                                p.pcm[i][ch]
                            );
                        }
                    }
                    if gain_moves {
                        retained[0] += (value[0] - retained[0]) * (1f32 / 1800.);
                    }
                    if pan_moves {
                        retained[1] = if retained[1] <= value[1] {
                            (retained[1] + ((10f32 / RATE as f32) + (10f32 / RATE as f32)))
                                .min(value[1])
                        } else {
                            (retained[1] - 20f32 / RATE as f32).max(value[1])
                        };
                    }
                }
                at += n;
            }
            self.previous_target = Some(value);
        }
        self.stage = p.stages.len();
    }
}
fn check_native_level(
    p: &Performance,
    reference: &Performance,
    gain: f64,
    oracle: &mut NativeOracle,
) {
    oracle.check(p, reference, [gain as f32, 0.]);
}
fn check_native_pan(p: &Performance, reference: &Performance, pan: f64, oracle: &mut NativeOracle) {
    oracle.check(p, reference, [0.25, pan as f32]);
}

fn same_performance(a: &Performance, b: &Performance) {
    assert_eq!(a.pcm.len(), b.pcm.len());
    // Host-window branch selection differs with these callback geometries.
    // Each native stage is checked against its own full PCM oracle above.
    assert_eq!(a.ended, b.ended);
    assert_eq!(a.ended_frames, b.ended_frames);
}

fn coefficients(
    source: &Source,
    partition: &'static [usize],
    maximum_db: i32,
    gain: f64,
    readback: i32,
) -> Performance {
    let mut amp = NativeOracle::default();
    let mut p = source.performance(partition, Some(private(0xae, true, maximum_db)), true);
    let mut reference = Performance::new(false, partition);
    let mut events = EventList::with_capacity(8);
    note(&mut events, ExactNoteKind::On, 410, 4, 60);
    p.stage(
        "native private saved gain",
        "saved 0.25 instrument gain is audible before physical CC7",
        &events,
    );
    reference.stage(
        "gain oracle onset",
        "known 240/480 Hz source at unity instrument gain",
        &events,
    );
    check_native_level(&p, &reference, 0.25, &mut amp);
    assert!(
        (seen(&p, 0) - 396850).abs() <= 1,
        "init getter must encode saved native gain"
    );

    events.clear();
    cc(&mut events, 4, 7, 127);
    cc(&mut events, 4, 2, 0);
    p.stage(
        "native CC7 coefficient",
        "physical CC7 replaces the instrument amplifier, including the selected maximum",
        &events,
    );
    reference.stage(
        "gain oracle coefficient",
        "phase matched unity source",
        &EventList::with_capacity(0),
    );
    check_native_level(&p, &reference, gain, &mut amp);
    assert!(
        (seen(&p, 2) - readback).abs() <= 1,
        "getter must read effective physical native gain"
    );

    events.clear();
    cc(&mut events, 4, 7, 127);
    cc(&mut events, 4, 1, 0);
    cc(&mut events, 4, 2, 0);
    p.stage(
        "physical then authored Volume",
        "later authored 500000 clears physical override and produces gain0.5",
        &events,
    );
    reference.stage(
        "gain oracle authored",
        "phase matched unity source",
        &EventList::with_capacity(0),
    );
    check_native_level(&p, &reference, 0.5, &mut amp);
    assert_eq!((seen(&p, 1), seen(&p, 2)), (500000, 500000));

    events.clear();
    cc(&mut events, 4, 1, 0);
    cc(&mut events, 4, 7, 127);
    cc(&mut events, 4, 2, 0);
    p.stage(
        "authored then physical Volume",
        "later physical CC7 regains authority in the same callback",
        &events,
    );
    reference.stage(
        "gain oracle physical",
        "phase matched unity source",
        &EventList::with_capacity(0),
    );
    check_native_level(&p, &reference, gain, &mut amp);
    assert!((seen(&p, 2) - readback).abs() <= 1);

    events.clear();
    for controller in [7, 1, 7, 3, 2] {
        cc(
            &mut events,
            4,
            controller,
            if controller == 7 { 127 } else { 0 },
        );
    }
    p.stage(
        "interleaved physical and authored Volume",
        "coalesced earlier writes retain the last authored250000 authority",
        &events,
    );
    reference.stage(
        "gain oracle coalesced",
        "phase matched unity source",
        &EventList::with_capacity(0),
    );
    check_native_level(&p, &reference, 0.0625, &mut amp);
    assert_eq!((seen(&p, 1), seen(&p, 2)), (250000, 250000));
    events.clear();
    cc(&mut events, 4, 7, 127);
    cc(&mut events, 4, 2, 0);
    p.stage(
        "physical Volume after coalescing",
        "new CC7 replaces the final authored write",
        &events,
    );
    reference.stage(
        "gain oracle post coalescing",
        "phase matched unity source",
        &EventList::with_capacity(0),
    );
    check_native_level(&p, &reference, gain, &mut amp);
    assert!((seen(&p, 2) - readback).abs() <= 1);

    events.clear();
    note(&mut events, ExactNoteKind::Off, 410, 4, 60);
    assert!(
        p.stage(
            "native gain exact Off",
            "original exact owner ends and settles to silence",
            &events
        )
        .iter()
        .all(|v| *v < 1e-8)
    );
    reference.stage("gain oracle Off", "same exact release clock", &events);
    amp.check(&p, &reference, amp.previous_target.unwrap());
    assert_eq!(p.ended, [address(410, 4, 60)]);
    assert_eq!(p.ended_frames, reference.ended_frames);
    p
}

#[test]
fn decoded_private_cc7_coefficients_and_authored_volume_chronology_reach_complete_plugin_pcm() {
    let source = Source::new();
    // Recovered multiplier/converter results; the oracle never calls the
    // production CC7 converter or the native Volume codec.
    for (db, gain, value) in [
        (-12, 0.24717724323272705, 395351),
        (-6, 0.49999696016311646, 499999),
        (0, 0.99739009141922, 629412),
        (6, 1.9936602115631104, 792861),
        (12, 3.9999756813049316, 999998),
    ] {
        let block = coefficients(&source, &[128], db, gain, value);
        let fragmented = coefficients(&source, &[1, 7, 31, 127], db, gain, value);
        same_performance(&block, &fragmented);
        block.export(&format!("native-gain-max-{db}"));
    }
}

fn precision(source: &Source, partition: &'static [usize], paired: bool) -> Performance {
    let mut amp = NativeOracle::default();
    let mut p = source.performance(partition, Some(private(0x80, true, 0)), false);
    let mut reference = Performance::new(false, partition);
    let mut events = EventList::with_capacity(8);
    note(&mut events, ExactNoteKind::On, 411, 4, 60);
    p.stage("native precision onset", "saved gain0.25", &events);
    reference.stage("precision oracle onset", "unity source", &events);
    check_native_level(&p, &reference, 0.25, &mut amp);
    let cases = if paired {
        [(8193u32, 0.12474226951599121), (8240, 0.12690138816833496)]
    } else {
        [
            (2147913144, 0.12474857270717621),
            (2160363545, 0.12693049013614655),
        ]
    };
    let mut levels = [0.; 2];
    for (i, (wire, gain)) in cases.into_iter().enumerate() {
        let normalized = f64::from(wire) / if paired { 16383. } else { u32::MAX as f64 };
        assert_eq!(
            (normalized * 127.).round() as u8,
            64,
            "both values have the same seven-bit projection"
        );
        events.clear();
        if paired {
            cc(&mut events, 4, 7, (wire >> 7) as u8);
            cc(&mut events, 4, 39, (wire & 127) as u8);
        } else {
            events.push(Event::on_port(
                0,
                0,
                EventBody::ControlChange2 {
                    group: 0,
                    channel: 4,
                    cc: 7,
                    value: wire,
                },
            ));
        }
        p.stage(
            "native continuous CC7",
            "same-seven-bit inputs retain distinct native instrument gain",
            &events,
        );
        levels[i] = rms(&p.pcm[p.pcm.len() - 800..])[0];
        reference.stage(
            "precision oracle continuous",
            "phase matched unity source",
            &EventList::with_capacity(0),
        );
        check_native_level(&p, &reference, gain, &mut amp);
    }
    assert!(
        levels[1] / levels[0] > 1.01,
        "low bits must be audible, beyond seven-bit quantization"
    );
    events.clear();
    cc(&mut events, 4, 121, 0);
    p.stage(
        "native continuous CC121",
        "reset retains volume precision",
        &events,
    );
    reference.stage(
        "precision oracle reset",
        "phase matched unity source",
        &EventList::with_capacity(0),
    );
    check_native_level(&p, &reference, cases[1].1, &mut amp);
    events.clear();
    note(&mut events, ExactNoteKind::Off, 411, 4, 60);
    assert!(
        p.stage(
            "native precision exact Off",
            "exact owner settles silent",
            &events
        )
        .iter()
        .all(|v| *v < 1e-8)
    );
    reference.stage("precision oracle Off", "same exact end clock", &events);
    amp.check(&p, &reference, amp.previous_target.unwrap());
    assert_eq!(p.ended, [address(411, 4, 60)]);
    assert_eq!(p.ended_frames, reference.ended_frames);
    p
}

#[test]
fn paired14_and_native32_volume_low_bits_change_actual_plugin_pcm_without_callback_heap() {
    let source = Source::new();
    for paired in [false, true] {
        let block = precision(&source, &[128], paired);
        let fragmented = precision(&source, &[1, 7, 31, 127], paired);
        same_performance(&block, &fragmented);
        block.export(if paired {
            "native-gain-paired14"
        } else {
            "native-gain-native32"
        });
    }
}

fn replacement(source: &Source, partition: &'static [usize]) -> Performance {
    let mut amp = NativeOracle::default();
    let mut p = source.performance(partition, Some(private(0x80, true, 0)), false);
    let mut reference = Performance::new(false, partition);
    let mut events = EventList::with_capacity(8);
    cc(&mut events, 4, 7, 127);
    note(&mut events, ExactNoteKind::On, 420, 4, 60);
    p.stage(
        "native replacement onset",
        "full native CC7 uses gain0.99739009",
        &events,
    );
    events.clear();
    note(&mut events, ExactNoteKind::On, 420, 4, 60);
    reference.stage("replacement oracle onset", "unity source", &events);
    check_native_level(&p, &reference, 0.99739009141922, &mut amp);

    // Only metadata changes. Identical sample topology must not qualify as
    // a RAM-fill upgrade when controller behavior/provenance changes.
    let records = [
        Some(private(0x80, false, 0)),
        Some(private(0x80, true, 0)),
        Some(private(0x81, true, 0)),
        Some(private(0x80, true, 0)),
        Some(ProgramDataPrivateParams {
            version: 0x80,
            data: vec![0],
        }),
        Some(private(0x80, true, 0)),
        None,
    ];
    for (i, record) in records.into_iter().enumerate() {
        let old_id = 420 + i as i32;
        let new_id = old_id + 1;
        let incoming = Box::new(Bank::load(&source.instrument(record, false)).unwrap());
        let boundary = p.pcm.len();
        let mut retired = None;
        assert_eq!(
            super::super::tests::allocations(|| {
                retired = p.dsp.rack.parts[0].instrument_mut().unwrap().upgrade_bank(incoming);
            }),
            0,
            "prepared policy replacement allocated or freed"
        );
        drop(retired);
        amp.fresh = true;
        events.clear();
        p.stage(
            "private policy replacement ends",
            "replacing policy stops old exact owner before new On",
            &events,
        );
        assert!(
            p.measured()
                .iter()
                .all(|frame| frame.iter().all(|sample| sample.abs() < 1e-8))
        );
        assert!(p.ended.contains(&address(old_id, 4, 60)));
        assert_eq!(
            *p.ended_frames.last().unwrap(),
            boundary,
            "replacement end belongs to the callback boundary"
        );
        // Install a separately prepared unity source: replacement retires
        // immediately, whereas a Choke intentionally keeps a 5 ms audio fade.
        let mut prepared = Performance::new(false, partition);
        let incoming = prepared.dsp.rack.parts[0].instrument_mut().unwrap().set_bank(None).unwrap();
        let mut retired = None;
        assert_eq!(super::super::tests::allocations(|| {
            retired = reference.dsp.rack.parts[0].instrument_mut().unwrap().set_bank(Some(incoming));
        }), 0);
        drop(retired);
        events.clear();
        reference.stage(
            "replacement oracle stop",
            "unity oracle owner retires at identical boundary",
            &events,
        );

        events.clear();
        note(&mut events, ExactNoteKind::On, new_id, 4, 60);
        p.stage(
            "new private policy onset",
            "fresh owner uses decoded native or compatibility fallback",
            &events,
        );
        reference.stage("new policy oracle onset", "fresh unity source", &events);
        if i < 2 || i == 3 || i == 5 {
            check_native_level(
                &p,
                &reference,
                if i % 2 == 1 { 0.99739009141922 } else { 0.25 },
                &mut amp,
            );
        } else {
            check_level(
                &p,
                &reference,
                if i % 2 == 1 { 0.99739009141922 } else { 0.25 },
            );
        };
        events.clear();
        cc(&mut events, 4, 7, 64);
        p.stage("new private policy CC7", "disabled ignores physical gain; native replaces amplifier; compatibility squares output", &events);
        reference.stage(
            "new policy oracle controller",
            "phase matched unity source",
            &EventList::with_capacity(0),
        );
        let gain = match i {
            0 => 0.25,
            1 | 3 | 5 => 0.12764206528663635,
            _ => 0.25 * (64f64 / 127.).powi(2),
        };
        if i < 2 || i == 3 || i == 5 {
            check_native_level(&p, &reference, gain, &mut amp);
        } else {
            check_level(&p, &reference, gain);
        }
        // Retain a full physical volume for the next bank's fresh owner.
        events.clear();
        cc(&mut events, 4, 7, 127);
        p.stage(
            "retained private policy CC7",
            "physical state survives preparation and replacement",
            &events,
        );
        reference.stage(
            "retained policy oracle",
            "phase matched unity source",
            &EventList::with_capacity(0),
        );
        if i < 2 || i == 3 || i == 5 {
            check_native_level(
                &p,
                &reference,
                if i == 0 { 0.25 } else { 0.99739009141922 },
                &mut amp,
            );
        } else {
            check_level(&p, &reference, 0.25);
        }
    }
    events.clear();
    note(&mut events, ExactNoteKind::Off, 427, 4, 60);
    assert!(
        p.stage(
            "replacement final exact Off",
            "last fallback owner releases to silence",
            &events
        )
        .iter()
        .all(|v| *v < 1e-8)
    );
    reference.stage(
        "replacement oracle final Off",
        "same exact release clock",
        &events,
    );
    assert_eq!(
        p.ended,
        (420..=427).map(|id| address(id, 4, 60)).collect::<Vec<_>>()
    );
    assert_eq!(p.ended_frames, reference.ended_frames);
    p
}

#[test]
fn private_policy_disable_enable_and_fallback_replacements_retire_exact_plugin_owners() {
    let source = Source::new();
    let block = replacement(&source, &[128]);
    let fragmented = replacement(&source, &[1, 7, 31, 127]);
    same_performance(&block, &fragmented);
    block.export("native-gain-policy-replacements");
}

#[test]
fn compatibility_volume_keeps_existing_authored_curve_and_squared_physical_output() {
    let source = Source::new();
    for partition in [&[128][..], &[1, 7, 31, 127][..]] {
        let mut p = source.performance(partition, None, true);
        let mut reference = Performance::new(false, partition);
        let mut events = EventList::with_capacity(8);
        note(&mut events, ExactNoteKind::On, 430, 4, 60);
        p.stage(
            "compatibility saved gain",
            "ordinary fixture retains saved program gain0.25",
            &events,
        );
        reference.stage("compatibility oracle onset", "unity source", &events);
        check_level(&p, &reference, 0.25);
        events.clear();
        cc(&mut events, 4, 1, 0);
        cc(&mut events, 4, 7, 64);
        cc(&mut events, 4, 2, 0);
        p.stage(
            "compatibility authored plus CC7",
            "authored 500000 uses3.981 curve and independent squared physical output",
            &events,
        );
        reference.stage(
            "compatibility oracle controller",
            "phase matched unity source",
            &EventList::with_capacity(0),
        );
        check_level(&p, &reference, 0.4976339638233185 * (64f64 / 127.).powi(2));
        assert_eq!((seen(&p, 1), seen(&p, 2)), (500000, 500000));
        events.clear();
        note(&mut events, ExactNoteKind::Off, 430, 4, 60);
        assert!(
            p.stage(
                "compatibility exact Off",
                "same exact owner retires",
                &events
            )
            .iter()
            .all(|v| *v < 1e-8)
        );
        reference.stage("compatibility oracle Off", "same exact clock", &events);
        assert_eq!(p.ended_frames, reference.ended_frames);
    }
}

#[test]
fn instrument_loader_init_volume_uses_private_policy_before_the_first_note() {
    let source = Source::new();
    for (record, gain) in [
        (Some(private(0xae, true, 0)), 0.5),
        (None, 0.4976339638233185),
    ] {
        let mut instrument = source.instrument(record, true);
        instrument.scripts[0] = READBACK.replacen(
            "%seen[0] :=",
            "set_engine_par($ENGINE_PAR_VOLUME,500000,-1,-1,-1)\n%seen[0] :=",
            1,
        );
        let mut p = Performance::new(false, &[1, 7, 31, 127]);
        p.dsp.rack.parts[0].instrument_mut().unwrap().set_bank(Some(Box::new(Bank::load(&instrument).unwrap())));
        let (runtime, errors) = crate::engine::load_scripts(&instrument, Vec::new(), RATE as f64);
        assert!(errors.is_empty(), "{errors:?}");
        p.dsp.rack.parts[0].instrument_mut().unwrap().set_script(runtime);
        assert_eq!(
            seen(&p, 0),
            500000,
            "init authored readback retains its exact units"
        );
        let mut reference = Performance::new(false, &[1, 7, 31, 127]);
        let mut events = EventList::with_capacity(2);
        note(&mut events, ExactNoteKind::On, 431, 4, 60);
        p.stage(
            "loader init authored Volume",
            "init native gain0.5 or existing compatibility3.981 curve reaches first note",
            &events,
        );
        reference.stage("init gain oracle onset", "unity source", &events);
        check_level(&p, &reference, gain);
        events.clear();
        note(&mut events, ExactNoteKind::Off, 431, 4, 60);
        assert!(
            p.stage(
                "loader init exact Off",
                "initialized owner releases silent",
                &events
            )
            .iter()
            .all(|v| *v < 1e-8)
        );
        reference.stage("init gain oracle Off", "same end clock", &events);
        assert_eq!(p.ended, [address(431, 4, 60)]);
        assert_eq!(p.ended_frames, reference.ended_frames);
    }
}

fn ingress(source: &Source, partition: &'static [usize], mode: u8) -> Performance {
    let record = Some(private(0xae, true, 0));
    let mut amp = NativeOracle::default();
    let mut p = source.performance(partition, record.clone(), false);
    if mode != 0 {
        // ignore_controller is KSP's controller-event suppression primitive.
        // The native system amplifier has already run when user KSP begins.
        let action = match mode {
            1 => "set_engine_par($ENGINE_PAR_VOLUME,500000,-1,-1,-1)",
            2 => "ignore_controller",
            3 => "wait(20000)",
            _ => unreachable!(),
        };
        let script = format!(
            "on init\ndeclare %seen[3]\nmake_persistent(%seen)\nend on\non controller\nif ($CC_NUM=7)\nif (%CC[7]=64)\n%seen[0] := get_engine_par($ENGINE_PAR_VOLUME,-1,-1,-1)\n{action}\n%seen[1] := get_engine_par($ENGINE_PAR_VOLUME,-1,-1,-1)\nelse\n%seen[2] := get_engine_par($ENGINE_PAR_VOLUME,-1,-1,-1)\n{}\nend if\nend if\nend on",
            if mode == 1 { action } else { "" }
        );
        let mut instrument = source.instrument(record, false);
        instrument.scripts = vec![script];
        let (runtime, errors) = crate::engine::load_scripts(&instrument, Vec::new(), RATE as f64);
        assert!(errors.is_empty(), "{errors:?}");
        p.dsp.rack.parts[0].instrument_mut().unwrap().set_script(runtime);
    }
    let mut reference = Performance::new(false, partition);
    let mut events = EventList::with_capacity(4);
    note(&mut events, ExactNoteKind::On, 440, 4, 60);
    p.stage(
        "native ingress owner onset",
        "saved gain0.25 before physical system action",
        &events,
    );
    reference.stage(
        "native ingress unity oracle",
        "known phase matched source",
        &events,
    );
    check_native_level(&p, &reference, 0.25, &mut amp);

    events.clear();
    cc(&mut events, 4, 7, 64);
    p.stage_for(
        "native CC7 before callback wait",
        "system amplifier arrives before user wait or ignore",
        &events,
        864,
        640,
    );
    reference.stage_for(
        "native ingress early oracle",
        "phase matched source before20ms continuation",
        &EventList::with_capacity(0),
        864,
        640,
    );
    let gain = if mode == 1 { 0.5 } else { 0.12764206528663635 };
    check_native_level(&p, &reference, gain, &mut amp);
    if mode != 0 {
        assert!(
            (seen(&p, 0) - 317184).abs() <= 1,
            "the first user callback must already read physical system gain"
        );
        if mode == 1 {
            assert_eq!(seen(&p, 1), 500000);
        }
        if mode == 2 {
            assert!((seen(&p, 1) - 317184).abs() <= 1);
        }
        if mode == 3 {
            assert_eq!(seen(&p, 1), 0, "20ms continuation must still be waiting");
        }
    }

    events.clear();
    cc(&mut events, 4, 7, 127);
    p.stage(
        "newer native CC7 before old wait resumes",
        "new physical gain arrives and old continuation cannot rewind it",
        &events,
    );
    reference.stage(
        "native ingress newer oracle",
        "phase matched unity source",
        &EventList::with_capacity(0),
    );
    check_native_level(
        &p,
        &reference,
        if mode == 1 { 0.5 } else { 0.99739009141922 },
        &mut amp,
    );
    if mode != 0 {
        assert!(
            (seen(&p, 2) - 629412).abs() <= 1,
            "new callback reads the new system action"
        );
        if mode == 3 {
            assert!(
                (seen(&p, 1) - 629412).abs() <= 1,
                "old waited callback sees current gain, without stale amplifier replay"
            );
        }
    }
    events.clear();
    note(&mut events, ExactNoteKind::Off, 440, 4, 60);
    assert!(
        p.stage(
            "native ingress exact Off",
            "ignored or waited controller cannot orphan original exact note",
            &events
        )
        .iter()
        .all(|v| *v < 1e-8)
    );
    reference.stage("native ingress oracle Off", "same exact end clock", &events);
    amp.check(&p, &reference, amp.previous_target.unwrap());
    assert_eq!(p.ended, [address(440, 4, 60)]);
    assert_eq!(p.ended_frames, reference.ended_frames);
    p
}

#[test]
fn native_system_cc7_precedes_user_ksp_and_cannot_be_ignored_delayed_or_replayed() {
    let source = Source::new();
    let mut dry = None;
    for mode in 0..=3 {
        let block = ingress(&source, &[128], mode);
        let fragmented = ingress(&source, &[1, 7, 31, 127], mode);
        same_performance(&block, &fragmented);
        if mode == 0 {
            dry = Some(block);
        } else if mode != 1 {
            same_performance(dry.as_ref().unwrap(), &block);
            assert!(
                dry.as_ref()
                    .unwrap()
                    .pcm
                    .iter()
                    .zip(&block.pcm)
                    .all(|(a, b)| (a[0] - b[0]).abs() < 2e-7 && (a[1] - b[1]).abs() < 2e-7),
                "ignore/wait changed native system PCM within identical host windows"
            );
        }
    }
}

fn pan_pcm(actual: &[[f32; 2]], reference: &[[f32; 2]], pan: f64) {
    // Independent linear stereo balance, including the native endpoint's
    // small nonzero left channel. Saved instrument gain remains0.25.
    let gains = [0.25 * (1. - pan.max(0.)), 0.25 * (1. + pan.min(0.))];
    let (level, unit) = (rms(actual), rms(reference));
    for ch in 0..2 {
        assert!(
            (level[ch] / unit[ch] - gains[ch]).abs() < 1e-7,
            "native CC10 stereo RMS channel{ch}: observed{}, expected{}",
            level[ch] / unit[ch],
            gains[ch]
        );
        if gains[ch] > 0. && actual.len() >= WINDOW {
            assert!((pcm_fundamental(actual, ch) - 240.).abs() < 2e-5);
        }
    }
    assert!(
        actual.iter().zip(reference).all(|(a, b)| [0, 1]
            .into_iter()
            .all(|ch| (f64::from(a[ch]) - f64::from(b[ch]) * gains[ch]).abs() < 2e-7)),
        "native instrument pan differs from independent stereo source PCM"
    );
}

fn pan_ingress(source: &Source, partition: &'static [usize], mode: u8) -> Performance {
    let record = Some(private(0xae, true, 0));
    let mut amp = NativeOracle::default();
    let mut p = source.performance(partition, record.clone(), false);
    if mode != 0 {
        let action = match mode {
            1 => "set_engine_par($ENGINE_PAR_PAN,750000,-1,-1,-1)",
            2 => "ignore_controller",
            3 => "wait(20000)",
            _ => unreachable!(),
        };
        let script = format!(
            "on init\ndeclare %seen[3]\nmake_persistent(%seen)\nend on\non controller\nif ($CC_NUM=10)\nif (%CC[10]=63)\n%seen[0] := get_engine_par($ENGINE_PAR_PAN,-1,-1,-1)\n{action}\n%seen[1] := get_engine_par($ENGINE_PAR_PAN,-1,-1,-1)\nelse\n%seen[2] := get_engine_par($ENGINE_PAR_PAN,-1,-1,-1)\n{}\nend if\nend if\nend on",
            if mode == 1 { action } else { "" }
        );
        let mut instrument = source.instrument(record, false);
        instrument.scripts = vec![script];
        let (runtime, errors) = crate::engine::load_scripts(&instrument, Vec::new(), RATE as f64);
        assert!(errors.is_empty(), "{errors:?}");
        p.dsp.rack.parts[0].instrument_mut().unwrap().set_script(runtime);
    }
    let mut reference = Performance::new(false, partition);
    let mut events = EventList::with_capacity(4);
    note(&mut events, ExactNoteKind::On, 450, 4, 60);
    p.stage(
        "native pan owner onset",
        "saved authored pan0 centers both channels",
        &events,
    );
    reference.stage(
        "native pan unity oracle",
        "phase matched stereo source",
        &events,
    );
    check_native_pan(&p, &reference, 0., &mut amp);
    events.clear();
    cc(&mut events, 4, 10, 63);
    p.stage_for(
        "native pan before user wait",
        "native byte63 negative pan arrives before user script suppression or wait",
        &events,
        864,
        640,
    );
    reference.stage_for(
        "native pan early oracle",
        "phase matched source before20ms continuation",
        &EventList::with_capacity(0),
        864,
        640,
    );
    check_native_pan(
        &p,
        &reference,
        if mode == 1 {
            0.5
        } else {
            -0.007875978946685791
        },
        &mut amp,
    );
    if mode != 0 {
        assert_eq!(
            seen(&p, 0),
            496062,
            "native system pan precedes user callback getter"
        );
        match mode {
            1 => assert_eq!(seen(&p, 1), 750000, "callback authored Pan wins exactly"),
            2 => assert_eq!(
                seen(&p, 1),
                496062,
                "ignore does not undo native system pan"
            ),
            3 => assert_eq!(seen(&p, 1), 0, "pan continuation still waits"),
            _ => unreachable!(),
        }
    }
    events.clear();
    cc(&mut events, 4, 10, 127);
    p.stage(
        "newer native pan before waited callback",
        "old waited byte63 cannot rewind later byte127 pan",
        &events,
    );
    reference.stage(
        "native pan newer oracle",
        "phase matched source",
        &EventList::with_capacity(0),
    );
    check_native_pan(
        &p,
        &reference,
        if mode == 1 { 0.5 } else { 0.9999959468841553 },
        &mut amp,
    );
    if mode != 0 {
        assert_eq!(
            seen(&p, 2),
            999998,
            "later callback sees native instrument pan getter"
        );
        if mode == 3 {
            assert_eq!(
                seen(&p, 1),
                999998,
                "waited getter sees current pan, without stale replay"
            );
        }
    }
    events.clear();
    cc(&mut events, 4, 121, 0);
    p.stage(
        "native pan CC121",
        "reset retains current native pan authority",
        &events,
    );
    reference.stage(
        "native pan reset oracle",
        "phase matched source",
        &EventList::with_capacity(0),
    );
    check_native_pan(
        &p,
        &reference,
        if mode == 1 { 0.5 } else { 0.9999959468841553 },
        &mut amp,
    );
    events.clear();
    note(&mut events, ExactNoteKind::Off, 450, 4, 60);
    assert!(
        p.stage(
            "native pan exact Off",
            "controller callback does not orphan note owner",
            &events
        )
        .iter()
        .all(|v| *v < 1e-8)
    );
    reference.stage("native pan oracle Off", "same exact release clock", &events);
    amp.check(&p, &reference, amp.previous_target.unwrap());
    assert_eq!(p.ended, [address(450, 4, 60)]);
    assert_eq!(p.ended_frames, reference.ended_frames);
    p
}

#[test]
fn native_cc10_system_pan_precedes_ksp_and_retains_stereo_pcm_through_ignore_wait_and_reset() {
    let source = Source::new();
    let mut dry = None;
    for mode in 0..=3 {
        let block = pan_ingress(&source, &[128], mode);
        let fragmented = pan_ingress(&source, &[1, 7, 31, 127], mode);
        same_performance(&block, &fragmented);
        if mode == 0 {
            dry = Some(block);
        } else if mode != 1 {
            same_performance(dry.as_ref().unwrap(), &block);
            assert!(
                dry.as_ref()
                    .unwrap()
                    .pcm
                    .iter()
                    .zip(&block.pcm)
                    .all(|(a, b)| (a[0] - b[0]).abs() < 2e-7 && (a[1] - b[1]).abs() < 2e-7),
                "ignore/wait changed native system PCM within identical host windows"
            );
        }
    }
}

fn pan_precision(source: &Source, partition: &'static [usize], width: u8) -> Performance {
    let mut amp = NativeOracle::default();
    let mut p = source.performance(partition, Some(private(0x80, true, 0)), false);
    let mut reference = Performance::new(false, partition);
    let mut events = EventList::with_capacity(4);
    note(&mut events, ExactNoteKind::On, 451, 4, 60);
    p.stage("native pan precision onset", "saved pan0", &events);
    reference.stage("native pan precision unity oracle", "unity source", &events);
    check_native_pan(&p, &reference, 0., &mut amp);
    // Independent recovered converter values. The continuous extensions
    // retain low bits despite identical CC byte/upper-seven-bit projections.
    let cases = match width {
        7 => vec![
            (0, -1.),
            (63, -0.007875978946685791),
            (64, 0.00787198543548584),
            (127, 0.9999959468841553),
        ],
        14 => vec![(8193, 0.00018107891082763672), (8240, 0.005918741226196289)],
        _ => vec![
            (2147913144, 0.00019800662994384766),
            (2160363545, 0.005995631217956543),
        ],
    };
    let mut balances = Vec::new();
    for &(raw, pan) in &cases {
        events.clear();
        match width {
            7 => cc(&mut events, 4, 10, raw as u8),
            14 => {
                assert_eq!(raw >> 7, 64);
                cc(&mut events, 4, 10, (raw >> 7) as u8);
                cc(&mut events, 4, 42, (raw & 127) as u8);
            }
            _ => {
                assert_eq!(raw >> 25, 64);
                events.push(Event::on_port(
                    0,
                    0,
                    EventBody::ControlChange2 {
                        group: 0,
                        channel: 4,
                        cc: 10,
                        value: raw,
                    },
                ));
            }
        }
        let level = p.stage(
            "native pan precise controller",
            "recovered native scalar at instrument pan with distinct stereo balance",
            &events,
        );
        reference.stage(
            "native pan precision oracle",
            "phase matched source",
            &EventList::with_capacity(0),
        );
        check_native_pan(&p, &reference, pan, &mut amp);
        balances.push(level[0] / level[1]);
    }
    if width != 7 {
        assert!(
            (balances[0] - balances[1]).abs() > 0.005,
            "same upper-byte pan values must retain audible continuous precision"
        );
    }
    events.clear();
    cc(&mut events, 4, 121, 0);
    p.stage(
        "native pan precision reset",
        "CC121 retains continuous pan bits",
        &events,
    );
    reference.stage(
        "native pan precision reset oracle",
        "phase matched source",
        &EventList::with_capacity(0),
    );
    check_native_pan(&p, &reference, cases.last().unwrap().1, &mut amp);
    events.clear();
    note(&mut events, ExactNoteKind::Off, 451, 4, 60);
    assert!(
        p.stage(
            "native pan precision exact Off",
            "precise pan owner retires",
            &events
        )
        .iter()
        .all(|v| *v < 1e-8)
    );
    reference.stage(
        "native pan precision oracle Off",
        "same exact end clock",
        &events,
    );
    amp.check(&p, &reference, amp.previous_target.unwrap());
    assert_eq!(p.ended, [address(451, 4, 60)]);
    assert_eq!(p.ended_frames, reference.ended_frames);
    p
}

#[test]
fn native_cc10_byte_coefficients_and_continuous14_32_pan_reach_stereo_plugin_pcm() {
    let source = Source::new();
    for width in [7, 14, 32] {
        let block = pan_precision(&source, &[128], width);
        let fragmented = pan_precision(&source, &[1, 7, 31, 127], width);
        same_performance(&block, &fragmented);
        block.export(&format!("native-pan-{width}-bit"));
    }
}

fn pan_replacement(source: &Source, partition: &'static [usize]) -> Performance {
    let mut amp = NativeOracle::default();
    let mut p = source.performance(partition, Some(private(0x80, true, 0)), false);
    let mut reference = Performance::new(false, partition);
    let mut events = EventList::with_capacity(4);
    cc(&mut events, 4, 10, 64);
    note(&mut events, ExactNoteKind::On, 452, 4, 60);
    p.stage(
        "native pan replacement onset",
        "native byte64 is slightly right of center",
        &events,
    );
    events.clear();
    note(&mut events, ExactNoteKind::On, 452, 4, 60);
    reference.stage(
        "pan replacement unity oracle",
        "phase matched source",
        &events,
    );
    check_native_pan(&p, &reference, 0.00787198543548584, &mut amp);
    for (i, record) in [
        private(0x80, false, 0),
        private(0x80, true, 0),
        private(0x81, true, 0),
    ]
    .into_iter()
    .enumerate()
    {
        let old_id = 452 + i as i32;
        let incoming = Box::new(Bank::load(&source.instrument(Some(record), false)).unwrap());
        let boundary = p.pcm.len();
        let mut retired = None;
        assert_eq!(
            super::super::tests::allocations(|| {
                retired = p.dsp.rack.parts[0].instrument_mut().unwrap().upgrade_bank(incoming);
            }),
            0
        );
        drop(retired);
        amp.fresh = true;
        events.clear();
        assert!(
            p.stage(
                "pan policy replacement end",
                "metadata policy replacement retires previous exact owner",
                &events
            )
            .iter()
            .all(|v| *v < 1e-8)
        );
        assert_eq!(*p.ended.last().unwrap(), address(old_id, 4, 60));
        assert_eq!(*p.ended_frames.last().unwrap(), boundary);
        let mut prepared = Performance::new(false, partition);
        let incoming = prepared.dsp.rack.parts[0].instrument_mut().unwrap().set_bank(None).unwrap();
        let mut retired = None;
        assert_eq!(super::super::tests::allocations(|| {
            retired = reference.dsp.rack.parts[0].instrument_mut().unwrap().set_bank(Some(incoming));
        }), 0);
        drop(retired);
        reference.stage(
            "pan replacement oracle end",
            "same old owner boundary",
            &events,
        );
        events.clear();
        note(&mut events, ExactNoteKind::On, old_id + 1, 4, 60);
        p.stage(
            "pan policy replacement fresh owner",
            "disabled authored center, reenabled native bias, or compatibility exact center",
            &events,
        );
        reference.stage("pan replacement fresh oracle", "unity source", &events);
        if i < 2 {
            check_native_pan(
                &p,
                &reference,
                if i == 1 { 0.00787198543548584 } else { 0. },
                &mut amp,
            );
        } else {
            pan_pcm(
                p.measured(),
                reference.measured(),
                if i == 1 { 0.00787198543548584 } else { 0. },
            );
        };
        events.clear();
        cc(&mut events, 4, 10, 63);
        p.stage(
            "pan policy replacement new controller",
            "disabled ignores physical pan; native and compatibility retain their different laws",
            &events,
        );
        reference.stage(
            "pan replacement controller oracle",
            "phase matched source",
            &EventList::with_capacity(0),
        );
        if i < 2 {
            check_native_pan(
                &p,
                &reference,
                match i {
                    0 => 0.,
                    1 => -0.007875978946685791,
                    _ => f64::from(-1f32 / 63.),
                },
                &mut amp,
            );
        } else {
            pan_pcm(
                p.measured(),
                reference.measured(),
                match i {
                    0 => 0.,
                    1 => -0.007875978946685791,
                    _ => f64::from(-1f32 / 63.),
                },
            );
        };
        events.clear();
        cc(&mut events, 4, 10, 64);
        p.stage(
            "pan policy retained physical center",
            "next preparation retains source value64",
            &events,
        );
        reference.stage(
            "pan retained center oracle",
            "phase matched source",
            &EventList::with_capacity(0),
        );
        if i < 2 {
            check_native_pan(
                &p,
                &reference,
                if i == 1 { 0.00787198543548584 } else { 0. },
                &mut amp,
            );
        } else {
            pan_pcm(
                p.measured(),
                reference.measured(),
                if i == 1 { 0.00787198543548584 } else { 0. },
            );
        };
    }
    events.clear();
    note(&mut events, ExactNoteKind::Off, 455, 4, 60);
    assert!(
        p.stage(
            "pan replacement final exact Off",
            "last compatibility owner releases silent",
            &events
        )
        .iter()
        .all(|v| *v < 1e-8)
    );
    reference.stage(
        "pan replacement final oracle Off",
        "same exact release clock",
        &events,
    );
    assert_eq!(
        p.ended,
        (452..=455).map(|id| address(id, 4, 60)).collect::<Vec<_>>()
    );
    assert_eq!(p.ended_frames, reference.ended_frames);
    p
}

#[test]
fn native_cc10_disable_enable_and_opaque_policy_replacements_end_old_plugin_owners() {
    let source = Source::new();
    let block = pan_replacement(&source, &[128]);
    let fragmented = pan_replacement(&source, &[1, 7, 31, 127]);
    same_performance(&block, &fragmented);
    block.export("native-pan-policy-replacements");
}

fn sound_off_fade(
    source: &Source,
    partition: &'static [usize],
    native: bool,
    path: u8,
) -> Performance {
    let mut p = source.performance(partition, native.then(|| private(0xae, true, 0)), false);
    let mut reference = source.performance(partition, None, false);
    let mut events = EventList::with_capacity(2);
    note(&mut events, ExactNoteKind::On, 480, 4, 60);
    p.stage(
        "sound-off fade onset",
        "settled known source and exact owner",
        &events,
    );
    reference.stage(
        "sound-off live source oracle",
        "same savedgain0.25 without stop",
        &events,
    );
    events.clear();
    p.stage_for(
        "sound-off nonzero source phase",
        "move cut away from sine zero crossing",
        &events,
        37,
        0,
    );
    reference.stage_for(
        "sound-off phase oracle",
        "same source clock",
        &events,
        37,
        0,
    );
    let cut = p.pcm.len();
    let frames = if native && path == 0 { 480 } else { 240 };
    match path {
        0 => cc(&mut events, 4, 120, 0), // actual Router SoundOffFrom ingress
        1 => assert_eq!(
            super::super::tests::allocations(|| p.dsp.rack.parts[0].instrument_mut().unwrap().cc(4, 120, 0)),
            0
        ),
        2 => assert_eq!(
            super::super::tests::allocations(|| p.dsp.rack.parts[0].panic()),
            0
        ),
        _ => unreachable!(),
    }
    p.stage_for(
        "exact sound-off ramp",
        "physicalNative120 fades10ms; compatibility/logicalCC/Panic fade5ms",
        &events,
        frames + 128,
        0,
    );
    reference.stage_for(
        "independent sound-off source",
        "uninterrupted PCM supplies each expected ramp sample",
        &EventList::with_capacity(0),
        frames + 128,
        0,
    );
    for (i, (actual, unit)) in p.pcm[cut..].iter().zip(&reference.pcm[cut..]).enumerate() {
        // Fade::apply decrements before multiplying; final ramp sample is0.
        // The oracle is a linear clock formula, independent of Fade state.
        let gain = (1. - (i + 1) as f64 / frames as f64).max(0.);
        for ch in 0..2 {
            assert!(
                (f64::from(actual[ch]) - f64::from(unit[ch]) * gain).abs() < 1e-7,
                "native{native} path{path} ramp frame{i}: expected independent{frames}-frame decay"
            );
        }
    }
    assert!(
        p.pcm[cut + frames - 2]
            .iter()
            .any(|sample| sample.abs() > 1e-7),
        "penultimate source/ramp sample must remain audible"
    );
    assert!(
        p.pcm[cut + frames - 1..]
            .iter()
            .all(|frame| *frame == [0.; 2]),
        "final ramp sample and every later sample must be zero"
    );
    if path != 2 {
        // Both physical and logical sound-off silence the held host root;
        // Panic additionally releases the input gate.
        assert!(p.ended.is_empty());
        assert!(p.dsp.rack.parts[0].host_key_held(4, 60));
        let release = p.pcm.len();
        events.clear();
        note(&mut events, ExactNoteKind::Off, 480, 4, 60);
        p.stage_for("sound-off physical key-up", "silent owner completes at physical Off", &events, 128, 0);
        assert!(!p.dsp.rack.parts[0].host_key_held(4, 60));
        assert_eq!(p.ended_frames, [release]);
    } else {
        assert_eq!(p.ended_frames, [cut + frames], "logical stop completes at the fade clock");
    }
    assert_eq!(p.ended, [address(480, 4, 60)]);
    p
}

#[test]
fn physical_native_cc120_has_exact10ms_pcm_fade_while_compatibility_and_logical_stops_keep5ms() {
    let source = Source::new();
    for (native, path) in [(true, 0), (false, 0), (true, 1), (true, 2)] {
        let block = sound_off_fade(&source, &[128], native, path);
        let fragmented = sound_off_fade(&source, &[1, 7, 31, 127], native, path);
        same_performance(&block, &fragmented);
        block.export(&format!("sound-off-native-{native}-path-{path}"));
    }
}
