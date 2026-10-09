// Synthetic source bytes only; no library payloads.
fn descriptor_object(
    id: u16,
    version: u16,
    private: &[u8],
    public: &[u8],
    children: &[ni_file::kontakt::Chunk],
) -> ni_file::kontakt::Chunk {
    let mut nested = Vec::new();
    for child in children {
        child.write(&mut nested).unwrap();
    }
    let mut data = vec![1];
    data.extend(version.to_le_bytes());
    for bytes in [private, public, &nested[..]] {
        data.extend((bytes.len() as u32).to_le_bytes());
        data.extend(bytes);
    }
    ni_file::kontakt::Chunk { id, data }
}
fn descriptor_name(out: &mut Vec<u8>, text: &str) {
    out.extend((text.len() as u32).to_le_bytes());
    out.extend(text.as_bytes());
}
fn descriptor_targets() -> Vec<u8> {
    let mut out = 3u32.to_le_bytes().to_vec();
    // The first unknown destination must keep ordinal 0, not shift pitch/volume.
    for (param, slot, flags, invert) in [
        ("frequency", Some(7u8), 0x10, false),
        ("pitch", None, 0x12, true),
        ("volume", None, 0x10, false),
    ] {
        descriptor_name(&mut out, param);
        out.extend(0.25f32.to_le_bytes());
        out.extend((-1i16).to_le_bytes());
        out.push(flags);
        out.extend(17u16.to_le_bytes());
        descriptor_name(&mut out, "same-name");
        if let Some(slot) = slot {
            out.push(slot);
        }
        out.push(u8::from(invert));
    }
    out.push(0); // No first shaper.
    out.extend([1, 0]); // Disabled table still retains its 128 values.
    for i in 0..128 {
        out.extend((i as f32 / 127.).to_le_bytes());
    }
    out.extend([2, 1, 2]); // Enabled graphical shaper with segment curvature.
    for [x, y, curve] in [[0f32, 0.2, -0.7], [1., 0.8, 0.9]] {
        for value in [x, y, curve] {
            out.extend(value.to_le_bytes());
        }
    }
    out
}
fn descriptor_slots(
    id: u16,
    count: usize,
    items: &[(usize, ni_file::kontakt::Chunk)],
) -> ni_file::kontakt::Chunk {
    let mut public = Vec::new();
    for slot in 0..count {
        let item = items.iter().find(|(i, _)| *i == slot);
        public.push(u8::from(item.is_some()));
        if let Some((_, chunk)) = item {
            chunk.write(&mut public).unwrap();
        }
    }
    descriptor_object(id, 0x10, &[], &public, &[])
}
fn descriptor_group() -> Group {
    use ni_file::kontakt::objects::{EnvelopeFlex, FlexPoint};
    let lfo = Lfo {
        structured: false,
        version: 0x73,
        waveform: 6,
        initial_values: [125., 3., 0.5, 0.375],
        records: [
            LfoRecord {
                flag: true,
                values: [0.25, 13., 0.75],
            },
            LfoRecord {
                flag: true,
                values: [0.5, 17., 0.125],
            },
        ],
        trailing_flag: true,
        trailing_values: Some([-0.3, 0., 0., 0., 0.]),
        additional_flag: Some(false),
    };
    let flex = EnvelopeFlex {
        points: vec![
            FlexPoint {
                time_ms: 125.,
                level: 0.8,
                curve: 0.25,
            },
            FlexPoint {
                time_ms: 25.,
                level: 0.3,
                curve: 0.75,
            },
        ],
        sustain: 1,
        unknown_index: 0,
        unknown_tail: vec![0xA5; 15],
    };
    let internal = |name: &str, source| {
        let mut private = descriptor_targets();
        private.extend([1, 1, 0, 7]);
        private.extend(42u32.to_le_bytes());
        descriptor_name(&mut private, name);
        private.extend(1u32.to_le_bytes());
        descriptor_object(0x0d, 0x81, &private, &[], &[source])
    };
    let external = |source, suffix: &[u8]| {
        let mut private = descriptor_targets();
        descriptor_name(&mut private, "duplicate");
        private.extend(1u32.to_le_bytes());
        private.extend(source);
        private.extend([0x7f, 2, 3, 4]);
        private.extend(99u32.to_le_bytes());
        private.extend(suffix);
        descriptor_object(0x0c, 0x104, &private, &[], &[])
    };
    Group(ni_file::kontakt::StructuredObject {
        version: 0x95,
        public_data: vec![],
        private_data: vec![],
        children: vec![
            descriptor_slots(
                INTERNAL_MODS,
                16,
                &[
                    (7, internal("duplicate", lfo.to_chunk().unwrap())),
                    (12, internal("duplicate", flex.to_chunk().unwrap())),
                ],
            ),
            descriptor_slots(
                EXTERNAL_MODS,
                32,
                &[
                    (3, external(11u32.to_le_bytes().to_vec(), &[8, 9])),
                    (
                        31,
                        external([4u32.to_le_bytes().as_slice(), &[127]].concat(), &[10, 11]),
                    ),
                ],
            ),
        ],
    })
}
#[test]
fn original_modulation_descriptors_keep_clocks_flex_and_external_order() {
    use ir::kontakt::{ExternalSource, InternalSource, ModulationSource as Raw};
    let mut out = translation();
    out.source_modulators(9, &descriptor_group()).unwrap();
    let rows = &out.ir.source_indices.modulators;
    assert_eq!(
        rows.iter()
            .map(|r| (r.group, r.slot, r.external))
            .collect::<Vec<_>>(),
        [(9, 7, false), (9, 12, false), (9, 3, true), (9, 31, true)]
    );
    assert!(rows.iter().all(|r| r.runtime.is_none()));
    let settings = rows[0]
        .settings
        .as_ref()
        .expect("retain unadmitted/bypassed LFO settings");
    assert_eq!(settings.version, 0x81);
    let Raw::Internal {
        flags,
        unknown_id,
        source: InternalSource::Lfo(lfo),
    } = &settings.source
    else {
        panic!("LFO");
    };
    assert_eq!((*flags, *unknown_id), ([1, 1, 0, 7], 42));
    assert_eq!(
        (lfo.version, lfo.waveform, lfo.initial_values),
        (0x73, 6, [125., 3., 0.5, 0.375])
    );
    assert_eq!(lfo.records[0].values, [0.25, 13., 0.75]);
    assert_eq!(lfo.records[1].values, [0.5, 17., 0.125]);
    assert!(lfo.records.iter().all(|r| r.flag) && lfo.trailing_flag);
    assert_eq!(lfo.trailing_values, Some([-0.3, 0., 0., 0., 0.]));
    assert_eq!(lfo.additional_flag, Some(false));
    assert_eq!(
        settings
            .targets
            .iter()
            .map(|t| (t.param.as_str(), t.slot))
            .collect::<Vec<_>>(),
        [("frequency", Some(7)), ("pitch", None), ("volume", None)]
    );
    assert_eq!(settings.targets[1].signed_intensity(), -0.25);
    assert!(settings.targets[1].invert);
    let shaper = settings.targets[1].shaper.as_ref().unwrap();
    assert!(!shaper.enabled);
    let ir::kontakt::ShaperCurve::Table(table) = &shaper.curve else {
        panic!("table");
    };
    assert_eq!((table.len(), table[1], table[127]), (128, 1. / 127., 1.));
    let ir::kontakt::ShaperCurve::Breakpoints(points) =
        &settings.targets[2].shaper.as_ref().unwrap().curve
    else {
        panic!("points");
    };
    assert_eq!((points[0].curve, points[1].curve), (-0.7, 0.9));
    let Raw::Internal {
        source:
            InternalSource::Flex {
                points,
                sustain,
                unknown_index,
                unknown_tail,
            },
        ..
    } = &rows[1].settings.as_ref().unwrap().source
    else {
        panic!("Flex");
    };
    assert_eq!(
        (points[0].time_ms, points[1].time_ms, points[1].curve),
        (125., 25., 0.75)
    );
    assert_eq!((*sustain, *unknown_index, unknown_tail.len()), (1, 0, 15));
    for (row, expected, tail) in [
        (&rows[2], ExternalSource::RandomBipolar, [8, 9]),
        (&rows[3], ExternalSource::MidiCc(127), [10, 11]),
    ] {
        let Raw::External {
            source,
            unknown_id,
            unknown_source_data,
            unknown_tail,
        } = &row.settings.as_ref().unwrap().source
        else {
            panic!("external");
        };
        assert_eq!((*source, *unknown_id), (expected, 99));
        assert_eq!(unknown_source_data, &[0x7f, 2, 3, 4]);
        assert_eq!(unknown_tail, &tail);
        assert_eq!(row.settings.as_ref().unwrap().targets.len(), 3);
    }
}
#[test]
fn malformed_original_modulation_never_manufactures_a_descriptor() {
    let mut group = descriptor_group();
    group.0.children[0].data.pop();
    let mut out = translation();
    assert!(out.source_modulators(0, &group).is_err());
    assert!(out.ir.source_indices.modulators.is_empty());
}
