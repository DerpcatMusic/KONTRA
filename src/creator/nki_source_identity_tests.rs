use super::*;

#[test]
fn saved_unsupported_kontakt_sources_retain_identity_cache_and_only_supported_sibling_pcm() {
    use crate::{
        audio::Sample,
        engine::{Ahdsr, Bank, Engine},
        import::{Instrument, SamplingProfile, Wavetable},
    };
    use ni_file::{
        kontakt::{Chunk, StructuredObject, objects::GroupList},
        nis::{ItemContainer, ItemType, PresetChunkItemProperties, SubtreeItem},
    };
    use std::io::Cursor;

    fn find_mut<'a>(item: &'a mut ItemContainer, kind: &ItemType) -> Option<&'a mut ItemContainer> {
        if item.id() == *kind {
            return Some(item);
        }
        item.children
            .iter_mut()
            .find_map(|item| find_mut(item, kind))
    }
    let dir = tempfile::tempdir().unwrap();
    let samples_dir = dir.path().join("Samples");
    std::fs::create_dir(&samples_dir).unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let frames: Vec<_> = (0..512)
        .map(|i| {
            [
                ((i * 17 % 71) as f32 - 35.) / 128.,
                ((i * 11 % 61) as f32 - 30.) / 128.,
            ]
        })
        .collect();
    for (name, bad) in [("unsupported.wav", true), ("sibling.wav", false)] {
        let mut wav = hound::WavWriter::create(
            samples_dir.join(name),
            hound::WavSpec {
                channels: 2,
                sample_rate: 48000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for frame in &frames {
            for value in frame {
                wav.write_sample(if bad { -*value } else { *value })
                    .unwrap();
            }
        }
        wav.finalize().unwrap();
    }
    let groups: Vec<_> = (0..5)
        .map(|i| Group {
            name: format!("Source{i}"),
            source_mode: Some(0),
            ..Default::default()
        })
        .collect();
    let zones: Vec<_> = (0..5)
        .map(|i| Zone {
            group: i,
            root: 60 + i as u8,
            low_key: 60 + i as u8,
            high_key: 60 + i as u8,
            sample: format!(
                "Samples/{}",
                if i == 4 {
                    "sibling.wav"
                } else {
                    "unsupported.wav"
                }
            )
            .into(),
            ..Default::default()
        })
        .collect();
    let samples: Vec<_> = ["unsupported.wav", "sibling.wav"]
        .map(|name| SampleFile {
            relative: format!("Samples/{name}"),
            rate: 48000,
            channels: 2,
            bits: 32,
            frames: 512,
            modified: 0,
        })
        .into_iter()
        .collect();
    let program = Program {
        name: "Saved source admission",
        author: "Test",
        groups: &groups,
        zones: &zones,
        samples: &samples,
        script: None,
        native_private: None,
        default_key_switch: None,
    };
    let ni_file::NIFile::NISoundContainer(mut outer) =
        ni_file::NIFile::read(Cursor::new(container(&program, "sources.nki").unwrap())).unwrap()
    else {
        panic!("creator writes NIS")
    };
    let frame = find_mut(&mut outer, &ItemType::EncryptionItem)
        .unwrap()
        .data
        .inner
        .as_mut()
        .unwrap();
    let subtree = SubtreeItem::read(Cursor::new(&frame.data)).unwrap();
    let mut inner = ItemContainer::read(Cursor::new(subtree.inner_data)).unwrap();
    let preset = find_mut(&mut inner, &ItemType::PresetChunkItem).unwrap();
    let bytes = PresetChunkItemProperties::try_from(&preset.data).unwrap().0;
    let mut cursor = Cursor::new(&bytes);
    let mut saved = StructuredObject::try_from(&Chunk::read(&mut cursor).unwrap()).unwrap();
    let list = saved
        .children
        .iter_mut()
        .find(|child| child.id == 0x33)
        .unwrap();
    let mut input = Cursor::new(&list.data[4..]);
    let mut changed = 5u32.to_le_bytes().to_vec();
    let mut raw_sources = Vec::new();
    for index in 0..5 {
        let mut group = StructuredObject::read(&mut input).unwrap();
        let at = group.private_data.len() - GROUP_TAIL_DEFAULT.len() / 2 + 4;
        let original = group.private_data.clone();
        // Native saved mode5 is retained even though its time-domain sampler
        // does not have a player. v0x104 mode9 has no decoded wavetable record.
        if index == 0 {
            group.private_data[at..at + 4].copy_from_slice(&5u32.to_le_bytes());
        }
        if index == 1 {
            group.private_data[at..at + 4].copy_from_slice(&9u32.to_le_bytes());
        }
        // An opaque source version must remain distinct from a known mode5.
        if index == 2 {
            group.private_data[at - 2..at].copy_from_slice(&0x105u16.to_le_bytes());
        }
        assert_eq!(&group.private_data[..at - 2], &original[..at - 2]);
        assert_eq!(&group.private_data[at + 4..], &original[at + 4..]);
        if index == 3 {
            let at = group.public_data.len() - 4;
            group.public_data[at..].copy_from_slice(&3i32.to_le_bytes());
        }
        raw_sources.push(group.private_data.clone());
        let children: Vec<_> = group
            .children
            .iter()
            .flat_map(|child| chunk(child.id, &child.data))
            .collect();
        changed.extend(structured(
            group.version,
            &group.private_data,
            &group.public_data,
            &children,
        ));
    }
    list.data = changed;
    let children: Vec<_> = saved
        .children
        .iter()
        .flat_map(|child| chunk(child.id, &child.data))
        .collect();
    let mut changed = chunk(
        0x28,
        &structured(
            saved.version,
            &saved.private_data,
            &saved.public_data,
            &children,
        ),
    );
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
    let path = dir.path().join("sources.nki");
    outer
        .write(&mut std::fs::File::create(&path).unwrap())
        .unwrap();
    let file = ni_file::NIFile::read(std::fs::File::open(&path).unwrap()).unwrap();
    let chunks =
        ni_file::kontakt::KontaktChunks::read(Cursor::new(file.inner_preset().unwrap())).unwrap();
    let parsed = chunks.program().unwrap().unwrap();
    let saved = GroupList::try_from(parsed.0.find_first(0x33).unwrap()).unwrap();
    for (group, raw) in saved.groups.iter().zip(raw_sources) {
        assert_eq!(group.0.private_data, raw);
    }
    assert_eq!(saved.groups[0].source_identity().unwrap().mode, 5);
    assert_eq!(saved.groups[1].source_identity().unwrap().mode, 9);
    assert!(saved.groups[2].source_identity().is_err());

    let imported = crate::import::read(&path).unwrap();
    assert_eq!(imported.sampling_profile, SamplingProfile::Kontakt);
    assert_eq!(
        imported
            .groups
            .iter()
            .map(|g| g.source_mode)
            .collect::<Vec<_>>(),
        [Some(5), Some(9), None, Some(0), Some(0)]
    );
    assert_eq!(imported.groups[3].interp_quality, 3);
    assert!(imported.groups.iter().all(|g| g.wavetable.is_none()));
    assert!(crate::cache::current(&imported.dependencies));
    crate::cache::store(cache_dir.path(), &path, 0, &imported);
    let restored = crate::cache::load(cache_dir.path(), &path, 0).unwrap();
    assert_eq!(
        serde_json::to_value(&restored.groups).unwrap(),
        serde_json::to_value(&imported.groups).unwrap()
    );
    assert_eq!(restored.warnings, imported.warnings);
    let banks = [
        Bank::load(&imported).unwrap(),
        Bank::load_bare(&restored).unwrap(),
        Bank::from_samples_with_profile(
            restored.groups.clone(),
            restored.zones.clone(),
            vec![
                (
                    samples_dir.join("unsupported.wav"),
                    Sample {
                        rate: 48000,
                        frames: frames.iter().map(|f| [-f[0], -f[1]]).collect(),
                    },
                ),
                (
                    samples_dir.join("sibling.wav"),
                    Sample {
                        rate: 48000,
                        frames: frames.clone(),
                    },
                ),
            ],
            SamplingProfile::Kontakt,
        )
        .unwrap(),
    ];
    for bank in &banks {
        assert_eq!(
            (
                bank.zones().len(),
                bank.skipped_zones,
                bank.zone_skip_counts.unsupported_source
            ),
            (1, 4, 4)
        );
        assert_eq!(bank.zones()[0].group, 4);
        assert_eq!(
            serde_json::to_value(bank.groups()).unwrap(),
            serde_json::to_value(&restored.groups).unwrap()
        );
        assert!(
            bank.issues
                .iter()
                .any(|issue| issue.contains("interpolation quality 3"))
        );
        for mode in ["Some(5)", "Some(9)", "None"] {
            assert!(bank.issues.iter().any(|issue| issue.contains(mode)
                && issue.contains("decoded wavetable false")
                && issue.contains("zone excluded")));
        }
    }
    // Retain historical Compatibility playback, including absent/opaque source
    // identities. This profile does not promise native source algorithms.
    let compatibility = Bank::from_samples(
        restored.groups.clone(),
        restored.zones.clone(),
        vec![
            (
                samples_dir.join("unsupported.wav"),
                Sample {
                    rate: 48000,
                    frames: frames.clone(),
                },
            ),
            (
                samples_dir.join("sibling.wav"),
                Sample {
                    rate: 48000,
                    frames: frames.clone(),
                },
            ),
        ],
    )
    .unwrap();
    assert_eq!(
        (compatibility.zones().len(), compatibility.skipped_zones),
        (5, 0)
    );

    let mut sibling = crate::import::read(&path).unwrap();
    sibling.zones.retain(|zone| zone.group == 4);
    let reference = Bank::load(&sibling).unwrap();
    let mut engines: Vec<_> = banks
        .into_iter()
        .chain([reference])
        .map(|bank| {
            let bank = bank
                .with_prepared_settings(|settings| {
                    for row in settings {
                        row.envelope = Some(Ahdsr::UNITY);
                        row.interp_quality = 0;
                    }
                })
                .unwrap();
            let mut e = Engine::default();
            e.reset(48000.);
            e.attack = 0.;
            e.blocking_streams = true;
            e.set_bank(Some(Box::new(bank)));
            e
        })
        .collect();
    let mut pcm = vec![[[0.; 768]; 2]; engines.len()];
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            for (index, engine) in engines.iter_mut().enumerate() {
                for key in [60, 61, 62, 63] {
                    engine.note_on(0, key, 127);
                }
                assert_eq!(engine.active_voices(), 0);
                let (mut at, mut block) = (0, 0);
                while at < 768 {
                    if at == 64 {
                        engine.note_on(0, 64, 127);
                    }
                    let mut n = if index % 2 == 0 {
                        128
                    } else {
                        [1, 7, 31, 127][block % 4]
                    };
                    n = n.min(768 - at);
                    if at < 64 {
                        n = n.min(64 - at);
                    }
                    let [l, r] = &mut pcm[index];
                    engine
                        .render_checked(&mut l[at..at + n], &mut r[at..at + n])
                        .unwrap();
                    at += n;
                    block += 1;
                }
                assert_eq!(engine.active_voices(), 0);
            }
        }),
        0
    );
    let reference = pcm.last().unwrap();
    assert!(
        reference
            .iter()
            .all(|row| row[..64].iter().all(|&x| x == 0.))
    );
    assert!(reference[0][64..576].iter().any(|x| x.abs() > 0.01));
    assert!(
        reference
            .iter()
            .all(|row| row[700..].iter().all(|&x| x == 0.))
    );
    for actual in &pcm[..pcm.len() - 1] {
        assert!(
            actual
                .iter()
                .zip(reference)
                .flat_map(|(a, b)| a.iter().zip(b))
                .all(|(a, b)| (a - b).abs() < 2e-6)
        );
    }

    for quality in [-1, 3, i32::MAX] {
        let make = |profile| {
            Bank::from_samples_with_profile(
                vec![Group::default()],
                vec![Zone::default()],
                vec![(
                    Default::default(),
                    Sample {
                        rate: 48000,
                        frames: frames.clone(),
                    },
                )],
                profile,
            )
            .unwrap()
        };
        let error = make(SamplingProfile::Kontakt)
            .with_prepared_settings(|rows| rows[0].interp_quality = quality)
            .err()
            .unwrap();
        assert!(
            format!("{error:#}")
                .contains("Unsupported Kontakt ordinary source interpolation quality")
        );
        assert!(
            make(SamplingProfile::Compatibility)
                .with_prepared_settings(|rows| rows[0].interp_quality = quality)
                .is_ok()
        );
    }

    // No decoded table may escape through another mode or an absent identity,
    // including a later off-thread zone remap into a retained invalid group.
    for mode in [None, Some(5), Some(9), Some(0)] {
        let invalid = Group {
            source_mode: mode,
            wavetable: (mode != Some(9)).then(Wavetable::default),
            ..Default::default()
        };
        let instrument = Instrument {
            sampling_profile: SamplingProfile::Kontakt,
            groups: vec![invalid, Group::default()],
            zones: vec![Zone {
                group: 1,
                sample: samples_dir.join("sibling.wav"),
                ..Default::default()
            }],
            scripts: vec![
                "on init\nset_snapshot_type(3)\nset_zone_par(0,$ZONE_PAR_GROUP,0)\nend on".into(),
            ],
            ..Default::default()
        };
        let (runtime, errors) = crate::engine::load_scripts(&instrument, vec![], 48000.);
        assert!(errors.is_empty(), "{errors:?}");
        let runtime = runtime.unwrap();
        assert!(
            runtime.diagnostics().is_empty(),
            "{:?}",
            runtime.diagnostics()
        );
        assert_eq!(
            runtime.init_zone_edits.len(),
            1,
            "init must submit an actual remap before checking admission"
        );
        let edit = runtime.init_zone_edits[0];
        assert_eq!(
            (edit.zone, edit.par, edit.value, edit.id),
            (0, crate::ksp::engine::ZonePar::Group, 0, -1)
        );
        let mut bank = Bank::load(&instrument).unwrap();
        let error = bank.apply_script_zone_init(&runtime).unwrap_err();
        assert!(format!("{error:#}").contains("Unsupported Kontakt source identity"));
        assert_eq!(bank.zones()[0].group, 1);
    }
}
