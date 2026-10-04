use super::*;

#[test]
fn saved_bypassed_primary_envelopes_keep_raw_identity_and_only_sibling_pcm() {
    use crate::{audio::Sample, engine::{Bank, Engine, Streaming, MEMORY_LIMIT},
        import::{Ahdsr, FlexEnvelope, FlexPoint, Group, Zone}};
    use ni_file::{kontakt::{Chunk, StructuredObject},
        nis::{ItemContainer, ItemType, PresetChunkItemProperties, SubtreeItem}};
    fn find_mut<'a>(item: &'a mut ItemContainer, kind: &ItemType) -> Option<&'a mut ItemContainer> {
        if item.id() == *kind { return Some(item); }
        item.children.iter_mut().find_map(|item| find_mut(item, kind))
    }
    fn source(param: &str, label: &str, bypass: bool, kind: u32, envelope: &[u8]) -> Vec<u8> {
        let mut private = 1u32.to_le_bytes().to_vec();
        for text in [param] { private.extend((text.len() as u32).to_le_bytes()); private.extend(text.as_bytes()); }
        private.extend(1f32.to_le_bytes()); private.extend((-1i16).to_le_bytes());
        private.push(0x10); private.extend(0u16.to_le_bytes());
        private.extend((label.len() as u32).to_le_bytes()); private.extend(label.as_bytes());
        private.extend([0, 0]); // No inversion or shaper.
        private.extend([0, u8::from(bypass), 1, 0]);
        private.extend(0u32.to_le_bytes());
        private.extend((label.len() as u32).to_le_bytes()); private.extend(label.as_bytes());
        private.extend(2u32.to_le_bytes());
        let wrapper = chunk(7, &structured(0x90, &[], &kind.to_le_bytes(), envelope));
        chunk(0x0d, &structured(0x80, &private, &[], &wrapper))
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("Samples")).unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let sample = dir.path().join("bypass.wav");
    let frames: Vec<_> = (0..4096).map(|i| {
        let level = 0.2 + (i % 37) as f32 / 1024.; [level, -level * 0.75]
    }).collect();
    let mut wav = hound::WavWriter::create(&sample, hound::WavSpec { channels: 2,
        sample_rate: 48000, bits_per_sample: 32, sample_format: hound::SampleFormat::Float }).unwrap();
    for frame in &frames { for value in frame { wav.write_sample(*value).unwrap(); } }
    wav.finalize().unwrap();
    let envelope = Ahdsr { attack_curve: 1., attack_ms: 0., hold_ms: 0., decay_ms: 0.,
        sustain: 1., release_ms: 20., unknown_flag: 0,
        unknown_tail: [0, 0, 128, 191, 0, 0, 0, 0, 0, 0, 128, 63, 0].repeat(4) };
    let mut ahdsr = Vec::new(); envelope.write(&mut ahdsr).unwrap();
    let flex = FlexEnvelope { points: vec![
        FlexPoint { time_ms: 1., level: 1., curve: 0.5 },
        FlexPoint { time_ms: 4., level: 0.5, curve: 0.5 },
        FlexPoint { time_ms: 8., level: 0., curve: 0.5 },
    ], sustain: 1, unknown_index: 0,
        unknown_tail: [vec![0, 1], vec![0, 0, 128, 191, 0, 0, 0, 0, 0, 0, 128, 63, 0]].concat() };
    let mut flex_bytes = Vec::new(); flex.write(&mut flex_bytes).unwrap();
    // Group1 starts with a bypassed non-volume Flex source. Only its later
    // selected volume source owns amplitude and may quarantine the group.
    let rows = [
        vec![source("volume", "BYPASSED_AHDSR", true, 0, &ahdsr)],
        vec![source("pitch", "NONVOLUME_FLEX", true, 1, &flex_bytes),
            source("volume", "BYPASSED_FLEX", true, 1, &flex_bytes)],
        vec![source("volume", "ACTIVE_AHDSR", false, 0, &ahdsr),
            source("pitch", "BYPASSED_PITCH", true, 0, &ahdsr),
            source("pitch", "SIBLING_NONVOLUME_FLEX", true, 1, &flex_bytes)],
    ];
    let groups: Vec<_> = (0..3).map(|group| Group { name: format!("Group{group}"), ..Default::default() }).collect();
    let zones: Vec<_> = (0..3).map(|group| Zone { group, low_key: 60 + group as u8,
        high_key: 60 + group as u8, sample: "bypass.wav".into(), ..Default::default() }).collect();
    let program = Program { name: "Saved amplitude bypass", author: "Test", groups: &groups, zones: &zones,
        samples: &[SampleFile { relative: "bypass.wav".into(), rate: 48000, channels: 2,
            bits: 32, frames: 4096, modified: 0 }], script: None, native_private: None, default_key_switch: None };
    let ni_file::NIFile::NISoundContainer(mut outer) = ni_file::NIFile::read(
        std::io::Cursor::new(container(&program, "bypass.nki").unwrap())).unwrap()
        else { panic!("creator writes NIS") };
    let frame = find_mut(&mut outer, &ItemType::EncryptionItem).unwrap().data.inner.as_mut().unwrap();
    let subtree = SubtreeItem::read(std::io::Cursor::new(&frame.data)).unwrap();
    let mut inner = ItemContainer::read(std::io::Cursor::new(subtree.inner_data)).unwrap();
    let preset = find_mut(&mut inner, &ItemType::PresetChunkItem).unwrap();
    let bytes = PresetChunkItemProperties::try_from(&preset.data).unwrap().0;
    let mut cursor = std::io::Cursor::new(&bytes);
    let mut saved = StructuredObject::try_from(&Chunk::read(&mut cursor).unwrap()).unwrap();
    let list = saved.children.iter_mut().find(|child| child.id == 0x33).unwrap();
    let mut input = std::io::Cursor::new(&list.data[4..]);
    let mut changed = 3u32.to_le_bytes().to_vec();
    for row in &rows {
        let mut group = StructuredObject::read(&mut input).unwrap();
        let mut slots = Vec::new();
        for source in row { slots.push(1); slots.extend(source); }
        slots.resize(slots.len() + 16 - row.len(), 0);
        group.children.iter_mut().find(|child| child.id == 0x3b).unwrap().data = structured(0x10, &[], &slots, &[]);
        let children: Vec<_> = group.children.iter().flat_map(|child| chunk(child.id, &child.data)).collect();
        changed.extend(structured(group.version, &group.private_data, &group.public_data, &children));
    }
    list.data = changed;
    let children: Vec<_> = saved.children.iter().flat_map(|child| chunk(child.id, &child.data)).collect();
    let mut changed = chunk(0x28, &structured(saved.version, &saved.private_data, &saved.public_data, &children));
    changed.extend(&bytes[cursor.position() as usize..]);
    preset.data.data.splice(20..20 + bytes.len(), changed.iter().copied());
    preset.data.data[12..20].copy_from_slice(&(changed.len() as u64).to_le_bytes());
    let mut encoded = Vec::new(); inner.write(&mut encoded).unwrap();
    frame.data.clear(); SubtreeItem { inner_data: encoded }.write_with_key(&mut frame.data, true, None).unwrap();
    let path = dir.path().join("bypass.nki");
    outer.write(&mut std::fs::File::create(&path).unwrap()).unwrap();
    let imported = crate::import::read(&path).unwrap();
    assert!(imported.groups[0].modulators[0].volume_env && imported.groups[0].modulators[0].bypassed);
    assert_eq!(imported.groups[0].volume_env, Some(envelope.clone()));
    assert!(imported.groups[1].modulators[0].flex && !imported.groups[1].modulators[0].primary_flex);
    assert!(imported.groups[1].modulators[1].primary_flex && imported.groups[1].modulators[1].bypassed);
    assert_eq!(imported.groups[1].flex_env, Some(flex));
    assert!(imported.groups[2].modulators[1].bypassed && !imported.groups[2].modulators[1].volume_env);
    assert!(imported.groups[2].modulators[2].bypassed && imported.groups[2].modulators[2].flex
        && !imported.groups[2].modulators[2].primary_flex,
        "non-volume Flex bypass must not quarantine the playable sibling");
    let cache = cache_dir.path();
    assert!(crate::cache::current(&imported.dependencies));
    crate::cache::store(cache, &path, 0, &imported);
    assert!(crate::cache::current(&imported.dependencies), "cache writes must not change library dependencies");
    let restored = crate::cache::load(cache, &path, 0).unwrap();
    assert_eq!(serde_json::to_value(&restored.groups).unwrap(), serde_json::to_value(&imported.groups).unwrap(),
        "cache retains full source records and exact selected ownership");
    assert_eq!(restored.zones, imported.zones);
    let mut legacy = serde_json::to_value(&restored.groups[1].modulators[1]).unwrap();
    legacy.as_object_mut().unwrap().remove("primary_flex");
    assert!(!serde_json::from_value::<crate::modulation::Modulator>(legacy).unwrap().primary_flex);
    // Keep the saved NI source profile: key62 travels fractionally above root60,
    // so Compatibility Hermite is not an interchangeable sibling reference.
    let resident = || Bank::from_samples_with_profile(restored.groups.clone(), restored.zones.clone(),
        vec![(sample.clone(), Sample { rate: 48000, frames: frames.clone() })], restored.sampling_profile).unwrap();
    let rejected = Bank::from_samples(restored.groups[..2].to_vec(), restored.zones[..2].to_vec(),
        vec![(sample.clone(), Sample { rate: 48000, frames: frames.clone() })]);
    assert!(format!("{:#}", rejected.err().unwrap()).contains("No playable zones"),
        "an instrument containing only unsupported amplitude owners cannot enter playback");
    let banks = [Bank::load_counting(&imported, MEMORY_LIMIT, Streaming::RamOnly, &[],
        &std::sync::atomic::AtomicU32::new(0)).unwrap(), Bank::load_bare(&restored).unwrap(), resident(),
        resident().with_prepared_settings(|settings| {
            settings[0].envelope = Some(crate::engine::Ahdsr::UNITY);
            settings[1].flex = None;
        }).unwrap()];
    for bank in &banks {
        assert_eq!((bank.zones().len(), bank.skipped_zones, bank.zone_skip_counts.unsupported_source), (1, 2, 2));
        assert_eq!(bank.zones()[0].group, 2);
        assert_eq!(serde_json::to_value(bank.groups()).unwrap(), serde_json::to_value(&restored.groups).unwrap());
        for name in ["BYPASSED_AHDSR", "BYPASSED_FLEX"] {
            assert!(bank.issues.iter().any(|issue| issue.contains(name) && issue.contains("zone excluded")));
        }
    }
    // A valid sibling cannot be remapped into an unsupported source group
    // through the same off-thread preparation used by zone edit workers.
    let mut remap = crate::import::read(&path).unwrap();
    remap.scripts = vec!["on init\nset_snapshot_type(3)\nset_zone_par(2,$ZONE_PAR_GROUP,0)\nend on".into()];
    let (runtime, errors) = crate::engine::load_scripts(&remap, vec![], 48000.);
    assert!(errors.is_empty(), "{errors:?}");
    let runtime = runtime.unwrap();
    assert!(runtime.diagnostics().is_empty(), "{:?}", runtime.diagnostics());
    assert_eq!(runtime.init_zone_edits.len(), 1, "init must submit an actual remap before checking admission");
    let edit = runtime.init_zone_edits[0];
    assert_eq!((edit.zone, edit.par, edit.value, edit.id),
        (2, crate::ksp::engine::ZonePar::Group, 0, -1));
    let mut guarded = Bank::load_bare(&remap).unwrap();
    let error = guarded.apply_script_zone_init(&runtime).unwrap_err();
    assert!(format!("{error:#}").contains("saved bypassed primary amplitude source"));
    assert_eq!(guarded.zones()[0].group, 2);
    let mut ordinary = crate::import::read(&path).unwrap(); ordinary.zones.retain(|zone| zone.group == 2);
    let reference = Bank::load_counting(&ordinary, MEMORY_LIMIT, Streaming::RamOnly, &[],
        &std::sync::atomic::AtomicU32::new(0)).unwrap();
    let mut engines: Vec<_> = banks.into_iter().chain([reference]).map(|bank| {
        let mut engine = Engine::default(); engine.reset(48000.); engine.attack = 0.;
        engine.blocking_streams = true; engine.set_bank(Some(Box::new(bank))); engine
    }).collect();
    let mut pcm = vec![[[0.; 2560]; 2]; engines.len()];
    assert_eq!(crate::plugin::tests::allocations(|| {
        for (index, engine) in engines.iter_mut().enumerate() {
            for note in [60, 61] { engine.note_on(0, note, 127); }
            let mut at = 0; let mut block = 0;
            while at < 2560 {
                if at == 256 {
                    assert_eq!(engine.active_voices(), 0, "quarantined sources cannot sound before the sibling");
                    engine.note_on(0, 62, 127);
                }
                if at == 1024 { engine.note_off(0, 62); }
                let mut count = if index % 2 == 0 { 128 } else { [1, 7, 31, 127][block % 4] };
                count = count.min(2560 - at);
                for boundary in [256, 1024] { if at < boundary { count = count.min(boundary - at); } }
                let [left, right] = &mut pcm[index];
                engine.render(&mut left[at..at + count], &mut right[at..at + count]);
                at += count; block += 1;
            }
        }
    }), 0);
    assert!(engines.iter().all(|engine| engine.active_voices() == 0), "all retained sibling release tails must complete");
    let reference = pcm.last().unwrap();
    assert!(reference.iter().all(|channel| channel[..256].iter().all(|&value| value == 0.)));
    assert!(reference.iter().all(|channel| channel[2304..].iter().all(|&value| value == 0.)));
    assert!(reference[0][256..1024].iter().any(|value| value.abs() > 0.01));
    for (index, actual) in pcm[..pcm.len() - 1].iter().enumerate() {
        let error = actual.iter().zip(reference).flat_map(|(actual, reference)| actual.iter().zip(reference))
            .map(|(actual, reference)| (actual - reference).abs()).fold(0., f32::max);
        assert!(error < 2e-5,
            "every complete PCM frame must match the independently retained sibling-only bank: engine {index}, error {error}");
    }
}
