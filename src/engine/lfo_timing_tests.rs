//! Saved timing crosses the actual NKI reader, cache, and resident preparation.
use crate::{
    engine::{
        Bank, Engine, HostExpression, HostNote, HostPattern, MEMORY_LIMIT, Streaming,
        params::Address,
    },
    import::Instrument,
};
use ni_file::{
    kontakt::{
        Chunk, StructuredObject,
        objects::{Lfo, LfoRecord},
    },
    nis::{ItemContainer, ItemType, PresetChunkItemProperties, SubtreeItem},
};
use std::io::Cursor;
const OFF: usize = 6144;
const END: usize = OFF + 4096;
fn object(id: u16, version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Chunk {
    let mut data = vec![1];
    data.extend(version.to_le_bytes());
    for part in [private, public, children] {
        data.extend((part.len() as u32).to_le_bytes());
        data.extend(part);
    }
    Chunk { id, data }
}
fn saved(id: u16, raw: &StructuredObject) -> Chunk {
    let mut children = Vec::new();
    for child in &raw.children {
        child.write(&mut children).unwrap();
    }
    object(
        id,
        raw.version,
        &raw.private_data,
        &raw.public_data,
        &children,
    )
}
fn name(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend((value.len() as u32).to_le_bytes());
    bytes.extend(value.as_bytes());
}
fn find<'a>(item: &'a mut ItemContainer, kind: &ItemType) -> Option<&'a mut ItemContainer> {
    if item.id() == *kind {
        return Some(item);
    }
    item.children.iter_mut().find_map(|item| find(item, kind))
}
fn fixture(sync: bool, unsupported: bool) -> (tempfile::TempDir, Instrument) {
    fixture_fade(sync, unsupported, None)
}
fn fixture_fade(sync: bool, unsupported: bool, fade_mode: Option<bool>) -> (tempfile::TempDir, Instrument) {
    fixture_timing(sync, unsupported, fade_mode, None)
}
fn fixture_timing(sync: bool, unsupported: bool, fade_mode: Option<bool>, fade_note: Option<f32>) -> (tempfile::TempDir, Instrument) {
    fixture_count(sync, unsupported, fade_mode, fade_note, None)
}
fn fixture_count(sync: bool, unsupported: bool, fade_mode: Option<bool>, fade_note: Option<f32>, count: Option<f32>) -> (tempfile::TempDir, Instrument) {
    fixture_source(sync, unsupported, fade_mode, fade_note, count, false)
}
fn fixture_source(sync: bool, unsupported: bool, fade_mode: Option<bool>, fade_note: Option<f32>, count: Option<f32>, shared: bool) -> (tempfile::TempDir, Instrument) {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("Source");
    std::fs::create_dir(&input).unwrap();
    let wave = input.join("Clock_C3.wav");
    let mut wav = hound::WavWriter::create(
        &wave,
        hound::WavSpec {
            channels: 2,
            sample_rate: 48000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for i in 0..8192 {
        let phase = std::f64::consts::TAU * i as f64 / 200.;
        let value = (0.04 * phase.sin() + 0.01 * (phase * 2.).sin()) as f32;
        wav.write_sample(value).unwrap();
        wav.write_sample(value).unwrap();
    }
    wav.finalize().unwrap();
    let created = crate::creator::create(
        &crate::creator::Options {
            source: input,
            name: "Timing".into(),
            vendor: "Test".into(),
            out: dir.path().join("Library"),
            kontra: false,
            kontakt: true,
        },
        &|_| {},
    )
    .unwrap();
    assert_eq!(created.instruments.len(), 1);
    let instrument_name = created.instruments[0].name.clone();
    let library = created.kontakt.unwrap();
    let path = library.join(format!("Instruments/{instrument_name}.nki"));
    // Replace the writer's default amplitude modulator with a literal saved
    // retriggered Digital source. No prepared PitchLfo is supplied to playback.
    let raw = Lfo {
        structured: false,
        version: if fade_mode.is_some() { 0x73 } else { 0x71 },
        waveform: 6,
        initial_values: [
            if fade_note.is_some() { 4. } else if fade_mode.is_some() { 40. } else { 0. },
            if unsupported {
                300.
            } else {
                count.unwrap_or(if sync { 3. } else { 3.75 })
            },
            0.5,
            0.23,
        ],
        records: [
            LfoRecord {
                flag: false,
                values: [if sync { 0.25 } else { 1e-9 }, 0., 0.],
            },
            LfoRecord {
                flag: true,
                values: [fade_note.unwrap_or(-1.), 0., 0.],
            },
        ],
        trailing_flag: false,
        trailing_values: Some([0., 0., 1., 0., 0.]),
        additional_flag: fade_mode,
    };
    let mut child = Vec::new();
    raw.to_chunk().unwrap().write(&mut child).unwrap();
    let mut private = 1u32.to_le_bytes().to_vec();
    name(&mut private, "pitch");
    private.extend(0.125f32.to_le_bytes());
    private.extend((-1i16).to_le_bytes());
    private.push(0x10);
    private.extend(0u16.to_le_bytes());
    name(&mut private, "Pitch");
    private.extend([0, 0, 0, 0, u8::from(!shared), 0]);
    private.extend(0u32.to_le_bytes());
    name(&mut private, "Saved Timing");
    private.extend(1u32.to_le_bytes());
    let mut slots = vec![1];
    object(0x0d, 0x80, &private, &[], &child)
        .write(&mut slots)
        .unwrap();
    slots.extend([0; 15]);
    let ni_file::NIFile::NISoundContainer(mut outer) =
        ni_file::NIFile::read(std::fs::File::open(&path).unwrap()).unwrap()
    else {
        panic!("writer must produce NIS");
    };
    let frame = find(&mut outer, &ItemType::EncryptionItem)
        .unwrap()
        .data
        .inner
        .as_mut()
        .unwrap();
    let tree = SubtreeItem::read(Cursor::new(&frame.data)).unwrap();
    let mut inner = ItemContainer::read(Cursor::new(tree.inner_data)).unwrap();
    let preset = find(&mut inner, &ItemType::PresetChunkItem).unwrap();
    let bytes = PresetChunkItemProperties::try_from(&preset.data).unwrap().0;
    let mut cursor = Cursor::new(&bytes);
    let mut program = StructuredObject::try_from(&Chunk::read(&mut cursor).unwrap()).unwrap();
    let groups = program
        .children
        .iter_mut()
        .find(|chunk| chunk.id == 0x33)
        .unwrap();
    assert_eq!(&groups.data[..4], &1u32.to_le_bytes());
    let mut group = StructuredObject::read(Cursor::new(&groups.data[4..])).unwrap();
    group
        .children
        .iter_mut()
        .find(|chunk| chunk.id == 0x3b)
        .unwrap()
        .data = object(0x3b, 0x10, &[], &slots, &[]).data;
    let mut changed = 1u32.to_le_bytes().to_vec();
    changed.extend(saved(0, &group).data);
    groups.data = changed;
    let mut changed = Vec::new();
    saved(0x28, &program).write(&mut changed).unwrap();
    changed.extend(&bytes[cursor.position() as usize..]);
    preset
        .data
        .data
        .splice(20..20 + bytes.len(), changed.iter().copied());
    preset.data.data[12..20].copy_from_slice(&(changed.len() as u64).to_le_bytes());
    let mut encoded = Vec::new();
    inner.write(&mut encoded).unwrap();
    frame.data.clear();
    SubtreeItem {
        inner_data: encoded,
    }
    .write_with_key(&mut frame.data, true, None)
    .unwrap();
    outer
        .write(&mut std::fs::File::create(&path).unwrap())
        .unwrap();
    let imported = crate::import::read(&path).unwrap();
    assert!(imported.missing_samples.is_empty());
    assert_eq!(
        imported.groups[0].pitch_lfos.len(),
        usize::from(!unsupported)
    );
    if !unsupported {
        let source = &imported.groups[0].pitch_lfos[0];
        assert_eq!(source.note_value, if sync { 0.25 } else { 1e-9 });
        assert_eq!(source.count.to_bits(), count.unwrap_or(if sync { 3. } else { 3.75 }).to_bits());
        assert_eq!(source.start_phase, 0.23);
        assert_eq!(source.shared, shared);
        assert_eq!(source.extended_fade, fade_mode == Some(true));
        assert_eq!(source.fade_sync.map(|s| s.note_value), fade_note);
        assert_eq!(source.fade_ms, if fade_note.is_some() { 0. } else if fade_mode.is_some() { 40. } else { 0. });
    } else {
        assert!(
            imported
                .warnings
                .iter()
                .any(|w| w.contains("pitch not applied"))
        );
    }
    let cache = dir.path().join("cache");
    crate::cache::store(&cache, &path, 0, &imported);
    let restored = crate::cache::load(&cache, &path, 0).unwrap();
    assert_eq!(
        serde_json::to_value(&restored.groups).unwrap(),
        serde_json::to_value(&imported.groups).unwrap()
    );
    (dir, restored)
}

fn engine(cache: &std::path::Path, instrument: &Instrument, plain: bool) -> Engine {
    let mut instrument = crate::cache::load(cache, &instrument.path, 0).unwrap();
    if plain {
        instrument.groups[0].pitch_lfos.clear();
    }
    let bank = Bank::load_counting(
        &instrument,
        MEMORY_LIMIT,
        Streaming::RamOnly,
        &[],
        &std::sync::atomic::AtomicU32::new(0),
    )
    .unwrap();
    let mut e = Engine::default();
    e.reset(48000.);
    e.attack = 0.;
    e.release = 0.05;
    e.set_bank(Some(Box::new(bank)));
    e
}
// Independent period arithmetic followed by the traced Digital phase quantum.
// We integrate phase by counts of old/new emissions, not by the production
// retained Clock or its phase recurrence. A triangle avoids sine-table claims.
fn frequency(sync: bool, tempo: f32) -> f32 {
    if !sync {
        return 3.75;
    }
    let quarters = 0.25f32 * 60.;
    let seconds = (quarters / tempo) * 3.;
    seconds.recip()
}
fn exterior(frame: usize, sync: bool, tempo_change: bool, unsupported: bool, fade_mode: Option<bool>) -> f32 {
    if unsupported || (137..229).contains(&frame) {
        return 0.;
    }
    let step = |tempo| f64::from(frequency(sync, tempo) * (1500f64.recip() as f32));
    let old_step = step(120.);
    let new_step = step(if tempo_change { 83. } else { 120. });
    let knot = |tick: usize| {
        // No points are emitted during the all-source bypass at160/192/224.
        let active = if tick < 5 {
            tick
        } else if tick < 8 {
            4
        } else {
            tick - 3
        };
        let old = active.min(14);
        let phase = (f64::from(0.23f32) + old as f64 * old_step + (active - old) as f64 * new_step)
            .rem_euclid(1.);
        let triangle = if phase < 0.5 {
            4. * (phase - 0.25)
        } else {
            4. * (0.75 - phase)
        };
        let depth = if tick >= 10 { 0.25 } else { 0.125 };
        let fade = if let Some(extended) = fade_mode {
            // Independent native f32 state recurrence, never production Fade.
            // Forty ms at the1500Hz source rate scales exactly60 points.
            let factor = (1. + 1. / f64::from(0.3f32)).powf(1. / 60.) as f32;
            let mut value = 0.3f32;
            for _ in 0..active.min(60) {
                value = (value * factor).clamp(if extended { 0.3 } else { 0. },
                    if extended { 1.3 } else { 1. });
            }
            if active >= 60 { 1. } else { value - 0.3 }
        } else { 1. };
        ((triangle as f32) * fade) * (12. * depth)
    };
    if frame < 32 {
        return knot(0);
    }
    let tick = frame / 32;
    // Resume229 still interpolates retained tick3→4 until the new point256.
    let (previous, current) = if (229..256).contains(&frame) {
        (knot(3), knot(4))
    } else {
        (knot(tick - 1), knot(tick))
    };
    previous + (current - previous) * (frame % 32) as f32 / 32.
}
fn performance(
    cache: &std::path::Path,
    instrument: &Instrument,
    sync: bool,
    unsupported: bool,
    tempo_change: bool,
    fade_mode: Option<bool>,
    fragments: &[usize],
) -> ([Vec<f32>; 2], u64) {
    let (mut actual, mut expected) = (engine(cache, instrument, false), engine(cache, instrument, true));
    let root = HostNote {
        port: 0,
        channel: 4,
        key: 60,
        id: 92,
        clap: true,
    };
    let pattern = HostPattern {
        port: 0,
        channel: 4,
        key: 60,
        id: 92,
        clap: true,
    };
    let mut pcm = [vec![0.; END], vec![0.; END]];
    let mut reference = [vec![0.; END], vec![0.; END]];
    let (mut at, mut block) = (0, 0);
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            for e in [&mut actual, &mut expected] {
                e.set_transport(true, 120., 0., (4, 4));
                assert!(e.host_note_on(root, 4, 60, 127, 0.));
            }
            while at < END {
                if !unsupported {
                    if at == 137 || at == 229 {
                        assert!(actual.write(
                            Address::PitchSourceBypass(0, 0),
                            if at == 137 { 1. } else { 0. }
                        ));
                    }
                    if at == 311 {
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
                }
                if at == 529 && tempo_change {
                    for e in [&mut actual, &mut expected] {
                        e.set_transport(true, 83., 0., (4, 4));
                    }
                }
                if at == 711 || at == 1037 {
                    for e in [&mut actual, &mut expected] {
                        e.host_expression(
                            pattern,
                            HostExpression::Gain(if at == 711 { 0. } else { 1. }),
                        );
                    }
                }
                if at == OFF {
                    for e in [&mut actual, &mut expected] {
                        e.host_note_off(pattern);
                    }
                }
                let boundary = [137, 229, 311, 529, 711, 1037, OFF, END]
                    .into_iter()
                    .filter(|b| *b > at)
                    .min()
                    .unwrap();
                let n = fragments[block % fragments.len()].min(boundary - at);
                let [l, r] = &mut pcm;
                actual
                    .render_checked(&mut l[at..at + n], &mut r[at..at + n])
                    .unwrap();
                for frame in at..at + n {
                    expected.host_expression(
                        pattern,
                        HostExpression::Tune(exterior(frame, sync, tempo_change, unsupported, fade_mode)),
                    );
                    let [l, r] = &mut reference;
                    expected
                        .render_checked(&mut l[frame..frame + 1], &mut r[frame..frame + 1])
                        .unwrap();
                }
                at += n;
                block += 1;
            }
            actual.mark_host_notes();
            expected.mark_host_notes();
        }),
        0
    );
    let error = pcm
        .iter()
        .flatten()
        .zip(reference.iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0_f32, f32::max);
    assert!(
        error < 3e-5,
        "saved timing full waveform error {error}, sync={sync}, unsupported={unsupported}"
    );
    assert!(pcm[0][32..711].iter().any(|x| x.abs() > 0.01));
    assert!(
        pcm.iter()
            .all(|channel| channel[END - 128..].iter().all(|x| *x == 0.))
    );
    let end = expected.host_note_end_at(root).unwrap();
    assert_eq!(actual.host_note_end_at(root), Some(end));
    assert!(
        end < (OFF + 3201) as u64,
        "finite known source ends before ordinary release floor"
    );
    (pcm, end)
}
#[test]
fn saved_digital_hz_and_sync_timing_survive_cache_and_complete_pcm_without_heap() {
    for (sync, unsupported) in [(false, false), (true, false), (false, true)] {
        let (files, instrument) = fixture(sync, unsupported);
        let cache = files.path().join("cache");
        let (whole, end) = performance(&cache, &instrument, sync, unsupported, true, None, &[128]);
        let (fragmented, fragmented_end) =
            performance(&cache, &instrument, sync, unsupported, true, None, &[1, 7, 31, 127]);
        assert_eq!(end, fragmented_end);
        let error = whole
            .iter()
            .flatten()
            .zip(fragmented.iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0_f32, f32::max);
        assert!(
            error < 3e-5,
            "host partitions changed saved timing waveform by{error}"
        );
        let (fixed_tempo, fixed_end) = performance(&cache, &instrument, sync, unsupported, false, None, &[128]);
        if !sync {
            assert_eq!(
                whole, fixed_tempo,
                "unsynchronized Hz must ignore host tempo"
            );
            assert_eq!(end, fixed_end);
        } else {
            assert!(
                whole[0][529..OFF]
                    .iter()
                    .zip(&fixed_tempo[0][529..OFF])
                    .any(|(a, b)| (a - b).abs() > 0.005),
                "sync timing change must move actual source phase"
            );
        }
    }
}

#[test]
fn saved_v73_fade_clamp_survives_nki_cache_and_complete_pcm_without_heap() {
    let mut legacy = None;
    for extended in [false, true] {
        let (files, instrument) = fixture_fade(false, false, Some(extended));
        let source = &instrument.groups[0].pitch_lfos[0];
        let mut json = serde_json::to_value(source).unwrap();
        json.as_object_mut().unwrap().remove("extended_fade");
        json.as_object_mut().unwrap().remove("fade_sync");
        let old: crate::import::PitchLfo = serde_json::from_value(json).unwrap();
        assert!(!old.extended_fade, "older caches retain the legacy clamp");
        assert_eq!(old.fade_sync, None);
        let cache = files.path().join("cache");
        let (whole, end) = performance(&cache, &instrument, false, false, true, Some(extended), &[128]);
        let (fragmented, fragmented_end) = performance(&cache, &instrument, false, false, true,
            Some(extended), &[1, 7, 31, 127]);
        assert_eq!(end, fragmented_end);
        let error = whole.iter().flatten().zip(fragmented.iter().flatten())
            .map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
        assert!(error < 3e-5, "v73 fade partitions changed completePCM by{error}");
        if let Some(ref previous) = legacy {
            let previous: &[Vec<f32>; 2] = previous;
            assert!(whole.iter().flatten().zip(previous.iter().flatten())
                .any(|(a, b)| (a - b).abs() > 1e-4), "extended clamp must change actual audio");
        } else { legacy = Some(whole); }
    }
}

#[path = "lfo_sync_fade_tests.rs"]
mod sync_fade_tests;

#[path = "lfo_count_floor_tests.rs"]
mod count_floor_tests;

#[path = "lfo_shared_sync_tests.rs"]
mod shared_sync_tests;
