use super::*;
use crate::{
    creator::nki,
    engine::Bank,
    import::{Group, Zone},
};
use ni_file::{
    kontakt::{Chunk, KontaktChunks},
    nis::{ItemContainer, ItemType, PresetChunkItemProperties, SubtreeItem},
};
use std::{
    io::Cursor,
    path::{Path, PathBuf},
};

fn find<'a>(item: &'a mut ItemContainer, kind: &ItemType) -> Option<&'a mut ItemContainer> {
    if item.id() == *kind {
        return Some(item);
    }
    item.children.iter_mut().find_map(|c| find(c, kind))
}
fn object(id: u16, version: u16, private: Vec<u8>, public: Vec<u8>, children: Vec<Chunk>) -> Chunk {
    let mut child_bytes = Vec::new();
    for child in children {
        child.write(&mut child_bytes).unwrap();
    }
    let mut data = vec![1];
    data.extend(version.to_le_bytes());
    for bytes in [private, public, child_bytes] {
        data.extend((bytes.len() as u32).to_le_bytes());
        data.extend(bytes);
    }
    Chunk { id, data }
}
fn raw(id: u16, version: u16, public: Vec<u8>) -> Chunk {
    let mut data = vec![0];
    data.extend(version.to_le_bytes());
    data.extend(public);
    Chunk { id, data }
}
fn text(bytes: &mut Vec<u8>, s: &str) {
    bytes.extend((s.encode_utf16().count() as u32).to_le_bytes());
    bytes.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
}

struct Fixture {
    _dir: tempfile::TempDir,
    path: PathBuf,
    programs: [crate::import::Instrument; 2],
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let sources = [
            ("a.wav", 64u64, 0.25f32),
            ("ar.wav", 128, 0.125),
            ("b.wav", 256, -0.2),
            ("br.wav", 96, -0.1),
        ];
        for (name, n, value) in sources {
            let mut w = hound::WavWriter::create(
                dir.path().join(name),
                hound::WavSpec {
                    channels: 2,
                    sample_rate: 48000,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                },
            )
            .unwrap();
            for i in 0..n {
                w.write_sample(value + (i % 7) as f32 / 1024.).unwrap();
                w.write_sample(value * 0.75 - (i % 11) as f32 / 2048.)
                    .unwrap();
            }
            w.finalize().unwrap();
        }
        let files: Vec<_> = sources
            .map(|(name, n, _)| nki::SampleFile {
                relative: name.into(),
                rate: 48000,
                channels: 2,
                bits: 32,
                frames: n,
                modified: 0,
            })
            .into();
        let private = ni_file::kontakt::objects::ProgramDataPrivateParams {
            version: 0xae,
            data: [
                0u32.to_le_bytes().as_slice(),
                &[0, 1],
                0i32.to_le_bytes().as_slice(),
            ]
            .concat(),
        };
        let mut nkis = Vec::new();
        for (name, key, a, r) in [("A", 60, "a.wav", "ar.wav"), ("B", 61, "b.wav", "br.wav")] {
            let groups = [
                Group {
                    volume_env: Some(crate::import::Ahdsr {
                        attack_curve: 1.,
                        attack_ms: 0.,
                        hold_ms: 0.,
                        decay_ms: 0.,
                        sustain: 1.,
                        release_ms: 1000.,
                        unknown_flag: 0,
                        unknown_tail: Vec::new(),
                    }),
                    ..Group::default()
                },
                Group {
                    release_trigger: true,
                    volume_env: Some(crate::import::Ahdsr {
                        attack_curve: 1.,
                        attack_ms: 0.,
                        hold_ms: 0.,
                        decay_ms: 0.,
                        sustain: 1.,
                        release_ms: 1000.,
                        unknown_flag: 0,
                        unknown_tail: Vec::new(),
                    }),
                    ..Group::default()
                },
            ];
            let mut zones = vec![
                Zone {
                    sample: a.into(),
                    root: key,
                    low_key: key,
                    high_key: key,
                    ..Zone::default()
                },
                Zone {
                    group: 1,
                    sample: r.into(),
                    root: key,
                    low_key: key,
                    high_key: key,
                    ..Zone::default()
                },
            ];
            if key == 61 {
                zones.push(Zone {
                    group: 1,
                    sample: r.into(),
                    root: 62,
                    low_key: 62,
                    high_key: 62,
                    ..Zone::default()
                });
            }
            let path = dir.path().join(format!("{name}.nki"));
            nki::write(
                &path,
                &nki::Program {
                    name,
                    author: "Bank proof",
                    groups: &groups,
                    zones: &zones,
                    samples: &files,
                    script: None,
                    native_private: Some(&private),
                    default_key_switch: None,
                },
            )
            .unwrap();
            nkis.push(path);
        }
        let programs = nkis
            .iter()
            .map(|p| crate::import::read(p).unwrap())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        let chunks: Vec<_> = nkis
            .iter()
            .map(|p| {
                KontaktChunks::read(Cursor::new(
                    ni_file::NIFile::read(std::fs::File::open(p).unwrap())
                        .unwrap()
                        .inner_preset()
                        .unwrap(),
                ))
                .unwrap()
            })
            .collect();
        let mut list = 2u16.to_le_bytes().to_vec();
        for (slot, c) in [7i16, 119].into_iter().zip(&chunks) {
            list.extend(slot.to_le_bytes());
            list.extend(&c.find_first(0x28).unwrap().data);
        }
        let mut rule = Vec::new();
        text(&mut rule, "Nonbinding shared cap");
        rule.extend(0u16.to_le_bytes());
        rule.push(1);
        rule.extend(2048u32.to_le_bytes());
        rule.extend(5u32.to_le_bytes());
        rule.extend((-1i32).to_le_bytes());
        let mut private = 2u32.to_le_bytes().to_vec();
        private.extend([0; 4]);
        private.extend(7u16.to_le_bytes());
        private.extend(0u32.to_le_bytes());
        private.extend(0x20000u32.to_le_bytes());
        let mut public = Vec::new();
        text(&mut public, "Actual sparse bank");
        public.extend(0.5f32.to_le_bytes());
        public.extend(0.25f32.to_le_bytes());
        let container = object(
            0x29,
            0x51,
            private,
            public,
            vec![
                raw(0x2b, 0x60, rule),
                Chunk {
                    id: 0x36,
                    data: list,
                },
            ],
        );
        let mut slots = vec![1 << 5, 0, 0, 0, 0, 0, 0, 0];
        container.write(&mut slots).unwrap();
        let mut settings = Vec::new();
        for _ in 0..64 {
            settings.extend(5u16.to_le_bytes());
            settings.extend([0; 2]);
            settings.extend(0u32.to_le_bytes());
        }
        let mut output = 2u32.to_le_bytes().to_vec();
        output.extend(0u32.to_le_bytes());
        output.extend(1u32.to_le_bytes());
        output.extend([1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        output.push(0);
        output.extend(0x70u16.to_le_bytes());
        text(&mut output, "Main stereo");
        output.extend(2u16.to_le_bytes());
        output.extend((-1i16).to_le_bytes());
        output.extend(1f32.to_le_bytes());
        output.extend(0u32.to_le_bytes());
        let mut public = 1f32.to_le_bytes().to_vec();
        public.extend(1f32.to_le_bytes());
        public.extend(137500i32.to_le_bytes());
        text(&mut public, "Bank parent");
        let bank = object(
            3,
            0x73,
            Vec::new(),
            public,
            vec![
                Chunk {
                    id: 0x37,
                    data: slots,
                },
                Chunk {
                    id: 0x48,
                    data: settings,
                },
                raw(0x3e, 0, output),
            ],
        );
        let mut saved = KontaktChunks(vec![bank]);
        for c in &chunks[0].0 {
            if c.id != 0x28 {
                saved.0.push(Chunk {
                    id: c.id,
                    data: c.data.clone(),
                });
            }
        }
        let path = dir.path().join("bank.nkm");
        rewrite(&nkis[0], &path, &saved);
        Self {
            _dir: dir,
            path,
            programs,
        }
    }
    fn bank(&self) -> ProgramBank {
        ProgramBank::load(&self.path, 5, 2, 48000., MainStereo).unwrap()
    }
    fn imported_with_budget(&self, max: u32) -> crate::import::InstrumentBank {
        let mut imported = crate::import::read_instrument_bank(&self.path, 5).unwrap();
        let rule = imported
            .container
            .0
            .children
            .iter_mut()
            .find(|c| c.id == 0x2b)
            .unwrap();
        let at = 3 + 4 + "Nonbinding shared cap".encode_utf16().count() * 2 + 3;
        rule.data[at..at + 4].copy_from_slice(&max.to_le_bytes());
        imported
    }
    fn references(&self) -> [Engine; 2] {
        self.programs.each_ref().map(|p| {
            let mut e = Engine::default();
            e.set_bank(Some(Box::new(Bank::load(p).unwrap())));
            e.set_home_channel(4);
            e.tune = 1.;
            e.set_transport(false, 137.5, 0., (4, 4));
            e
        })
    }
}
fn rewrite(base: &Path, path: &Path, chunks: &KontaktChunks) {
    let ni_file::NIFile::NISoundContainer(mut outer) =
        ni_file::NIFile::read(std::fs::File::open(base).unwrap()).unwrap()
    else {
        panic!("NIS")
    };
    let frame = find(&mut outer, &ItemType::EncryptionItem)
        .unwrap()
        .data
        .inner
        .as_mut()
        .unwrap();
    let sub = SubtreeItem::read(Cursor::new(&frame.data)).unwrap();
    let mut inner = ItemContainer::read(Cursor::new(sub.inner_data)).unwrap();
    let preset = find(&mut inner, &ItemType::PresetChunkItem).unwrap();
    let previous = PresetChunkItemProperties::try_from(&preset.data).unwrap().0;
    let tail = preset.data.data[20 + previous.len()..].to_vec();
    let mut bytes = Vec::new();
    chunks.write(&mut bytes).unwrap();
    preset.data.data.truncate(12);
    preset.data.data.extend((bytes.len() as u64).to_le_bytes());
    preset.data.data.extend(bytes);
    preset.data.data.extend(tail);
    let mut bytes = Vec::new();
    inner.write(&mut bytes).unwrap();
    frame.data.clear();
    SubtreeItem { inner_data: bytes }
        .write_with_key(&mut frame.data, true, None)
        .unwrap();
    outer
        .write(&mut std::fs::File::create(path).unwrap())
        .unwrap();
}
fn note(key: u8) -> HostNote {
    HostNote {
        port: 2,
        channel: 4,
        key,
        id: 9,
        clap: true,
    }
}
fn off(n: HostNote) -> HostPattern {
    HostPattern {
        port: i32::from(n.port),
        channel: i32::from(n.channel),
        key: i32::from(n.key),
        id: n.id,
        clap: n.clap,
    }
}
fn cc(e: &mut Engine, n: u8, v: u8) {
    e.control_from(4, 4, Control::Cc(n, ControlValue::from_midi1(v)));
}

/// Independent unity-inner oracle for the native common-fresh contract. The
/// standalone engines supply source PCM/work receipts, not bank amplifier state.
#[derive(Default)]
struct ReferenceAmplifiers {
    gain: [f32; 2],
    moving: [bool; 2],
    window: [Option<(u64, u64)>; 2],
    active: bool,
}
impl ReferenceAmplifiers {
    fn render(&mut self, engines: &mut [Engine; 2], left: &mut [f32], right: &mut [f32]) {
        let frames = left.len();
        for e in engines.iter_mut() {
            e.render_begin(frames);
        }
        let mut at = 0;
        loop {
            for e in engines.iter_mut() {
                e.render_boundary(at);
            }
            if at == frames {
                break;
            }
            let end = engines
                .iter()
                .map(|e| e.render_next_boundary(at))
                .min()
                .unwrap()
                .min(frames);
            let mut any_work = false;
            for (index, e) in engines.iter_mut().enumerate() {
                let (mut l, mut r) = ([0.; 128], [0.; 128]);
                let work = e.render_interval(at, &mut l[..end - at], &mut r[..end - at]);
                any_work |= work;
                if !work {
                    continue;
                }
                let window = e.player.release_counter.span();
                if !self.active {
                    self.gain[index] = 1.;
                    self.moving[index] = false;
                } else if self.window[index] != window {
                    let ratio = (self.gain[index] + 1e-12).abs() / (1_f32 + 1e-12);
                    self.moving[index] = ratio < f32::from_bits(0x3f7fbe77);
                }
                self.window[index] = window;
                for i in 0..end - at {
                    let gain = if self.moving[index] {
                        self.gain[index]
                    } else {
                        1.
                    };
                    left[at + i] += (l[i] * gain) * 0.375;
                    right[at + i] += (r[i] * gain) * 0.5;
                    if self.moving[index] {
                        self.gain[index] += (1. - self.gain[index]) * f32::from_bits(0x3a11a2b4);
                    }
                }
            }
            self.active = any_work;
            at = end;
        }
        for e in engines {
            e.render_finish();
        }
    }
}

#[test]
fn actual_sparse_bank_keeps_original_roots_release_samples_decoder_and_exact_ends_without_heap() {
    let fixture = Fixture::new();
    for sizes in [&[128][..], &[1, 7, 31, 127, 13, 64][..]] {
        let mut bank = fixture.bank();
        let mut references = fixture.references();
        let mut reference_amplifiers = ReferenceAmplifiers::default();
        assert_eq!(bank.selected(), 7);
        assert_eq!(bank.saved_tempo, 137.5);
        assert!(bank.set_transport(false, None, 0., (4, 4)));
        let mut actual = [vec![0.; 768], vec![0.; 768]];
        let mut expected = [vec![0.; 768], vec![0.; 768]];
        let mut ends = Vec::with_capacity(4);
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                // RPN selector/coarse is physical wire state across program selection.
                for bytes in [[0xb4, 101, 0], [0xb4, 100, 2], [0xb4, 6, 65]] {
                    assert_eq!(bank.midi1(2, &bytes), Admission::Accepted);
                }
                assert_eq!(
                    bank.host_on(note(60), Velocity::from_midi1(127), 0.),
                    Admission::Accepted
                );
                assert!(references[0].host_note_on(note(60), 4, 60, 127, 0.));
                assert_eq!(bank.midi1(2, &[0xb4, 64, 127]), Admission::Accepted);
                cc(&mut references[0], 64, 127);
                let mut at = 0;
                let mut fragment = 0;
                for end in [
                    32, 96, 112, 128, 144, 160, 176, 192, 256, 300, 320, 384, 400, 620, 640, 768,
                ] {
                    while at < end {
                        if at % 256 == 0 {
                            bank.begin_audio_block(256, false);
                            for e in &mut references {
                                e.begin_audio_block(256, 2, false);
                            }
                        }
                        let n = sizes[fragment % sizes.len()]
                            .min(end - at)
                            .min(256 - at % 256);
                        let [l, r] = &mut actual;
                        bank.render(&mut l[at..at + n], &mut r[at..at + n]);
                        let [l, r] = &mut expected;
                        reference_amplifiers.render(
                            &mut references,
                            &mut l[at..at + n],
                            &mut r[at..at + n],
                        );
                        at += n;
                        fragment += 1;
                    }
                    match end {
                        32 => {
                            assert_eq!(bank.midi1(2, &[0xc4, 119]), Admission::Accepted);
                            assert_eq!(bank.midi1(2, &[0xb4, 38, 0]), Admission::Accepted);
                            assert_eq!(
                                bank.programs[1].engine.player.midi.tuning[4].ratio(),
                                1.,
                                "RPN2 ignores fine Data Entry after selection"
                            );
                            assert_eq!(bank.midi1(2, &[0xb4, 6, 65]), Admission::Accepted);
                            assert_eq!(
                                bank.programs[1].engine.player.midi.tuning[4].ratio(),
                                2f64.powf(1. / 12.),
                                "shared RPN2 selection reaches the newly selected program"
                            );
                            assert_eq!(
                                bank.host_on(note(61), Velocity::from_midi1(127), 0.),
                                Admission::Accepted
                            );
                            assert!(references[1].host_note_on(note(61), 4, 61, 127, 0.));
                            assert_eq!(
                                bank.host_on(note(60), Velocity::from_midi1(127), 0.),
                                Admission::Exhausted
                            );
                        }
                        96 => {
                            bank.host_off(off(note(60)), Velocity::from_midi1(64));
                            references[0].host_note_off(off(note(60)));
                        }
                        112 => {
                            bank.host_off(off(note(61)), Velocity::from_midi1(64));
                            references[1].host_note_off(off(note(61)));
                        }
                        128 => {
                            assert_eq!(bank.midi1(2, &[0xb4, 64, 0]), Admission::Accepted);
                            cc(&mut references[0], 64, 0);
                            cc(&mut references[1], 64, 0);
                        }
                        144 => {
                            assert_eq!(bank.midi1(2, &[0xb4, 88, 127]), Admission::Accepted);
                            assert_eq!(bank.midi1(2, &[0xc4, 42]), Admission::Empty);
                            assert_eq!(bank.midi1(2, &[0x94, 60, 127]), Admission::Empty);
                        }
                        160 => {
                            assert_eq!(bank.midi1(2, &[0xc4, 7]), Admission::Accepted);
                            assert_eq!(bank.midi1(2, &[0x94, 60, 64]), Admission::Accepted);
                            references[0].note_on(4, 60, 64);
                        }
                        176 => {
                            assert_eq!(bank.midi1(2, &[0xc4, 119]), Admission::Accepted);
                            assert_eq!(bank.midi1(2, &[0x94, 61, 64]), Admission::Accepted);
                            references[1].note_on(4, 61, 64);
                        }
                        192 => {
                            assert_eq!(bank.midi1(2, &[0x84, 60, 64]), Admission::Accepted);
                            references[0].note_off(4, 60);
                        }
                        300 => {
                            bank.finish_host_notes(|n, t| {
                                ends.push((n, t));
                                false
                            });
                            assert!(ends.contains(&(note(60), 249)));
                            assert!(ends.contains(&(note(61), 274)), "captured ends: {ends:?}");
                            assert_eq!(
                                bank.host_on(note(61), Velocity::from_midi1(127), 0.),
                                Admission::Exhausted
                            );
                        }
                        320 => {
                            bank.finish_host_notes(|_, _| true);
                            for (e, (n, terminal)) in references
                                .iter_mut()
                                .zip([(note(60), 249), (note(61), 274)])
                            {
                                e.mark_host_notes();
                                assert_eq!(e.host_note_end_at(n), Some(terminal));
                                e.retire_host_note(n);
                            }
                            assert_eq!(bank.midi1(2, &[0x84, 61, 64]), Admission::Accepted);
                            references[1].note_off(4, 61);
                        }
                        384 => {
                            assert_eq!(
                                bank.host_on(note(61), Velocity::from_midi1(127), 0.),
                                Admission::Accepted
                            );
                            assert!(references[1].host_note_on(note(61), 4, 61, 127, 0.));
                            bank.host_expression(off(note(60)), HostExpression::Gain(0.));
                            bank.host_off(off(note(60)), Velocity::from_midi1(127));
                        }
                        400 => {
                            bank.host_off(off(note(61)), Velocity::from_midi1(64));
                            references[1].host_note_off(off(note(61)));
                        }
                        620 => {
                            assert_eq!(
                                bank.host_on(note(62), Velocity::from_midi1(127), 0.),
                                Admission::Accepted
                            );
                            assert!(references[1].host_note_on(note(62), 4, 62, 127, 0.));
                        }
                        640 => {
                            bank.host_off(off(note(62)), Velocity::from_midi1(64));
                            references[1].host_note_off(off(note(62)));
                        }
                        _ => {}
                    }
                }
                bank.finish_host_notes(|n, t| {
                    ends.push((n, t));
                    true
                });
                assert!(ends.contains(&(note(61), 626)));
                assert!(ends.contains(&(note(62), 731)));
                assert_eq!(bank.active_voices(), 0);
            }),
            0
        );
        let error = actual
            .iter()
            .flatten()
            .zip(expected.iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(error < 2e-6, "bank PCM {error}");
        assert!(actual[0][200].abs() > 0.01);
        assert!(
            actual[0][700] < 0.,
            "only program119's negative release-only sample is alive"
        );
    }
}

#[test]
fn late_nonunit_inner_uses_zero_start_gain_and_pan_recurrence_without_heap() {
    let fixture = Fixture::new();
    let mut imported = fixture.imported_with_budget(2048);
    imported.programs[1].1.program.gain = 0.4;
    imported.programs[1].1.program.pan = -0.7;
    let mut bank = ProgramBank::prepare(imported, 2, 48000., MainStereo).unwrap();
    let [mut a, mut b] = fixture.references();
    // This scenario has no RPN2 coarse-tuning transaction.
    a.tune = 0.;
    b.tune = 0.;
    let (mut l, mut r, mut al, mut ar, mut bl, mut br) =
        ([0.; 32], [0.; 32], [0.; 32], [0.; 32], [0.; 32], [0.; 32]);
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            bank.begin_audio_block(256, false);
            for e in [&mut a, &mut b] {
                e.begin_audio_block(256, 2, false);
            }
            bank.host_on(note(60), Velocity::from_midi1(127), 0.);
            a.host_note_on(note(60), 4, 60, 127, 0.);
            bank.render(&mut l, &mut r);
            a.render(&mut al, &mut ar);
            b.render(&mut bl, &mut br);
            bank.midi1(2, &[0xc4, 119]);
            bank.host_on(note(61), Velocity::from_midi1(127), 0.);
            b.host_note_on(note(61), 4, 61, 127, 0.);
            bank.render(&mut l, &mut r);
            a.render(&mut al, &mut ar);
            b.render(&mut bl, &mut br);
            let (mut gain, mut pan) = (0_f32, 0_f32);
            for i in 0..32 {
                let el = (al[i] + bl[i] * gain) * 0.375;
                let er = (ar[i] + (br[i] * gain) * (1. + pan)) * 0.5;
                assert!(
                    (l[i] - el).abs() < 2e-6 && (r[i] - er).abs() < 2e-6,
                    "frame {i}: actual [{}, {}], oracle [{el}, {er}], gain {gain}, pan {pan}",
                    l[i],
                    r[i]
                );
                gain += (0.4 - gain) * f32::from_bits(0x3a11a2b4);
                pan += -20_f32 / 48000.;
            }
        }),
        0
    );
}

#[test]
fn nonunit_inner_with_returned_sends_is_rejected_during_preparation() {
    let fixture = Fixture::new();
    let mut imported = fixture.imported_with_budget(2048);
    imported.programs[1].1.program.gain = 0.4;
    imported.programs[1]
        .1
        .fx
        .send
        .slots
        .push(crate::fx::Effect {
            slot: 0,
            kind: crate::fx::Kind::Gainer,
            version: 0,
            bypass: false,
            output_gain: 1.,
            dry_level: 0.,
            params: crate::fx::Params::Gainer(crate::fx::params::Gainer { gain: 1. }),
        });
    let error = ProgramBank::prepare(imported, 2, 48000., MainStereo)
        .err()
        .unwrap();
    assert!(
        error
            .to_string()
            .contains("nonunit amplifier with returned-send FX")
    );
}

#[test]
fn max_one_budget_selects_victims_only_in_the_original_inner_program_without_heap() {
    let fixture = Fixture::new();
    let mut bank =
        ProgramBank::prepare(fixture.imported_with_budget(1), 2, 48000., MainStereo).unwrap();
    let a = note(60);
    let b = note(61);
    let next_a = HostNote { id: 10, ..a };
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            bank.begin_audio_block(256, false);
            assert_eq!(
                bank.host_on(a, Velocity::from_midi1(127), 0.),
                Admission::Accepted
            );
            bank.render(&mut [0.; 16], &mut [0.; 16]);
            assert_eq!(bank.midi1(2, &[0xc4, 119]), Admission::Accepted);
            assert_eq!(
                bank.host_on(b, Velocity::from_midi1(127), 0.),
                Admission::Accepted
            );
            bank.render(&mut [0.; 16], &mut [0.; 16]);
            assert_eq!(bank.programs[0].engine.active_voices(), 1);
            assert_eq!(bank.programs[1].engine.active_voices(), 1);
            assert_eq!(bank.midi1(2, &[0xc4, 7]), Admission::Accepted);
            assert_eq!(
                bank.host_on(next_a, Velocity::from_midi1(127), 0.),
                Admission::Accepted
            );
            bank.render(&mut [0.; 32], &mut [0.; 32]);
            // A's victim can remain in its fade; B never enters A's census.
            assert!(bank.host_note_pending(b));
            assert!(bank.host_note_pending(next_a));
            assert_eq!(bank.programs[1].engine.active_voices(), 1);
        }),
        0
    );
    assert!(ProgramBank::prepare(fixture.imported_with_budget(0), 2, 48000., MainStereo).is_err());
}

#[test]
fn typed_bank_replacement_retains_terminal_identity_until_ack_without_heap() {
    let fixture = Fixture::new();
    let mut part = super::super::RackPart::from(Box::new(fixture.bank()));
    let [engine, _] = fixture.references();
    let mut replacement = super::super::RackPart::from(engine);
    let mut next_bank = super::super::RackPart::from(Box::new(fixture.bank()));
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            let bank = part.program_bank_mut().unwrap();
            bank.begin_audio_block(256, false);
            assert_eq!(
                bank.host_on(note(60), Velocity::from_midi1(127), 0.),
                Admission::Accepted
            );
            bank.render(&mut [0.; 32], &mut [0.; 32]);
            assert!(part.replace_prepared(&mut replacement));
            assert!(part.instrument().is_some());
            assert_eq!(part.host_note_end_at(note(60)), Some(32));
            assert!(
                !part
                    .instrument_mut()
                    .unwrap()
                    .host_note_on(note(60), 4, 60, 127, 0.),
                "unacknowledged old identity must reject reuse in the new instrument"
            );
            assert!(part.replace_prepared(&mut next_bank));
            assert!(
                part.instrument_mut().is_none(),
                "banks never expose an editable engine"
            );
            assert_eq!(part.host_note_end_at(note(60)), Some(32));
            assert_eq!(
                part.program_bank_mut()
                    .unwrap()
                    .host_on(note(60), Velocity::from_midi1(127), 0.),
                Admission::Exhausted
            );
            part.retire_host_note(note(60));
            assert!(!part.host_note_present(note(60)));
            assert_eq!(
                part.program_bank_mut()
                    .unwrap()
                    .host_on(note(60), Velocity::from_midi1(127), 0.),
                Admission::Accepted
            );
        }),
        0
    );
    // Replaced engines/banks, their FX and pools are freed off the callback.
    drop((replacement, next_bank));
}

#[test]
fn canonical_root_loan_restores_after_unwind_and_foreign_or_stale_retirement() {
    let f = Fixture::new();
    let mut bank = f.bank();
    assert_eq!(
        bank.host_on(note(60), Velocity::from_midi1(127), 0.),
        Admission::Accepted
    );
    let first = bank.programs[0].engine.player.host_notes.active[0];
    bank.with_program(1, |e| e.player.host_notes.retire(first));
    assert!(bank.with_program(0, |e| e.player.host_notes.get(first).is_some()));
    let ptr = bank.raw.as_ref() as *const Receiver;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        bank.with_program(1, |_| panic!("loan proof"))
    }));
    assert!(result.is_err());
    assert_eq!(ptr, bank.raw.as_ref() as *const Receiver);
    bank.with_program(0, |e| e.player.host_notes.retire(first));
    assert_eq!(
        bank.host_on(note(60), Velocity::from_midi1(127), 0.),
        Admission::Accepted
    );
    let next = bank.programs[0].engine.player.host_notes.active[0];
    assert_ne!(first, next);
    assert!(bank.with_program(0, |e| e.player.host_notes.get(first).is_none()));
    bank.programs[1].engine.player.host_notes.active.push(first);
    bank.with_program(1, |e| e.player.host_notes.reset());
    assert!(bank.with_program(0, |e| e.player.host_notes.get(next).is_some()));
    let mut l = [0.; 128];
    let mut r = [0.; 128];
    bank.render(&mut l, &mut r);
    assert!(l.iter().any(|v| v.abs() > 0.01));
    bank.host_off(off(note(60)), Velocity::from_midi1(64));
    bank.render(&mut l, &mut r);
    let mut end = None;
    bank.finish_host_notes(|_, t| {
        end = Some(t);
        true
    });
    assert_eq!(end, Some(256));
}
