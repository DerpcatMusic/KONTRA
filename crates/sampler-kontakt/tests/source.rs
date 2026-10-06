use sampler_kontakt::{Chunks, ErrorKind, Limits, Script};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

const LIMITS: Limits = Limits {
    bytes: 65536,
    records: 128,
};

// Authored wire bytes, no legacy decoder/writer or commercial library fixture.
fn sized(data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_le_bytes().to_vec();
    out.extend(data);
    out
}
fn object(version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Vec<u8> {
    let mut out = vec![1];
    out.extend(version.to_le_bytes());
    for data in [private, public, children] {
        out.extend(sized(data));
    }
    out
}
fn chunk(id: u16, data: &[u8]) -> Vec<u8> {
    let mut out = id.to_le_bytes().to_vec();
    out.extend(sized(data));
    out
}
fn script(text: Option<&[u8]>, state: &[u8]) -> Vec<u8> {
    let mut public = text.map_or_else(|| u32::MAX.to_le_bytes().to_vec(), sized);
    public.extend([0, 1, 0]);
    public.extend(sized(&[0xff, 0x80])); // Hash and string bytes stay opaque.
    public.extend(sized(&[])); // Empty description is present.
    public.extend(u32::MAX.to_le_bytes()); // No linked script.
    public.extend(state);
    chunk(6, &object(0x60, &[0xa5], &public, &chunk(0xffff, &[9])))
}

fn unstructured_script(text: &[u8]) -> Vec<u8> {
    let mut body = vec![0, 0x60, 0];
    body.extend(sized(text));
    body.extend([0, 0, 0]);
    body.extend(0u32.to_le_bytes());
    body.extend(u32::MAX.to_le_bytes());
    body.extend(u32::MAX.to_le_bytes());
    body.extend(0u32.to_le_bytes());
    chunk(6, &body)
}

#[test]
fn unstructured_script_uses_its_chunk_boundary_but_cannot_swallow_array_peers() {
    let first = unstructured_script(b"on note end on");
    let second = unstructured_script(b"on release end on");
    let bytes = [first.as_slice(), second.as_slice()].concat();
    support::without_heap(|| {
        let mut chunks = Chunks::parse(&bytes, LIMITS).unwrap().iter();
        for text in [b"on note end on".as_slice(), b"on release end on"] {
            let script = Script::parse(chunks.next().unwrap(), LIMITS).unwrap();
            assert!(!script.object.is_structured);
            assert_eq!(script.text.unwrap().data(), text);
            assert!(script.object.private.data().is_empty());
            assert_eq!(script.object.children(LIMITS).unwrap().iter().count(), 0);
        }
        assert!(chunks.next().is_none());
    });
    let mut array = 2u32.to_le_bytes().to_vec();
    array.extend(&first[6..]);
    array.extend(object(0x95, &[], &[], &[]));
    let bytes = chunk(0x33, &array);
    let root = Chunks::parse(&bytes, LIMITS)
        .unwrap()
        .iter()
        .next()
        .unwrap();
    assert_eq!(
        root.records(LIMITS).unwrap_err().kind,
        ErrorKind::UnsupportedLayout
    );
}

#[test]
fn source_views_preserve_wire_identity_and_all_unknown_bytes_without_allocating() {
    let mut children = chunk(0x9000, &[1, 2, 3]);
    let mut group_list = 2u32.to_le_bytes().to_vec();
    group_list.extend(object(0x102, &[8], &[7], &[]));
    group_list.extend(object(0xbeef, &[6], &[5], &[]));
    children.extend(chunk(0x33, &group_list));
    let mut zones = 1u32.to_le_bytes().to_vec();
    zones.extend(0xffffffffu32.to_le_bytes()); // Source ID, not guessed/normalized.
    zones.extend(object(0x9a, &[4], &[3], &[]));
    children.extend(chunk(0x34, &zones));
    let mut state = 2u32.to_le_bytes().to_vec();
    state.extend(sized(b"$value 7"));
    state.extend(sized(&[0xff, 0, 0x81]));
    state.extend([0xde, 0xad]);
    children.extend(script(Some(&[0xef, 0xbb, 0xbf, 0xff]), &state));
    children.extend(chunk(0x9000, &[4, 5]));
    let bytes = chunk(
        0x28,
        &object(0xaf, &[7, 8], b"opaque program params", &children),
    );
    support::without_heap(|| {
        let roots = Chunks::parse(&bytes, LIMITS).unwrap();
        assert_eq!(roots.raw().data(), bytes);
        let root = roots.iter().next().unwrap();
        let program = root.structured().unwrap();
        assert_eq!(program.version, 0xaf);
        assert_eq!(program.private.data(), [7, 8]);
        let mut children = program.children(LIMITS).unwrap().iter();
        assert_eq!(children.next().unwrap().id, 0x9000);
        let groups = children.next().unwrap().records(LIMITS).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups.iter().nth(1).unwrap().object.version, 0xbeef);
        let zones = children.next().unwrap().records(LIMITS).unwrap();
        assert_eq!(zones.iter().next().unwrap().group, Some(u32::MAX));
        let source = Script::parse(children.next().unwrap(), LIMITS).unwrap();
        assert_eq!(source.text.unwrap().data(), [0xef, 0xbb, 0xbf, 0xff]);
        assert!(source.touched_but_not_applied);
        assert_eq!(source.description.unwrap().data(), []);
        assert!(source.textfile_name.is_none());
        assert_eq!(
            source.persistent.unwrap().iter().nth(1).unwrap().data(),
            [0xff, 0, 0x81]
        );
        assert_eq!(source.extension.data(), [0xde, 0xad]);
        let hash = source.password_hash;
        assert_eq!(
            &bytes[hash.offset()..hash.offset() + hash.data().len()],
            hash.data()
        );
        assert_eq!(
            source
                .object
                .children(LIMITS)
                .unwrap()
                .iter()
                .next()
                .unwrap()
                .id,
            0xffff
        );
        assert_eq!(children.next().unwrap().id, 0x9000);
        assert!(children.next().is_none());
    });
}

#[test]
fn malformed_lengths_counts_flags_and_saved_tables_fail_at_the_source_boundary() {
    let bytes = script(Some(b"on note end on"), &0u32.to_le_bytes());
    for end in 1..bytes.len() {
        let truncated = &bytes[..end];
        support::without_heap(|| {
            assert!(Chunks::parse(truncated, LIMITS).is_err());
        });
    }
    assert_eq!(
        Chunks::parse(&bytes, Limits { bytes: 1, ..LIMITS })
            .unwrap_err()
            .kind,
        ErrorKind::Limit
    );
    assert_eq!(
        Chunks::parse(
            &bytes,
            Limits {
                records: 0,
                ..LIMITS
            }
        )
        .unwrap_err()
        .kind,
        ErrorKind::Limit
    );
    // Reframe every truncated structured body so the outer framing itself is valid.
    for end in 0..bytes.len() - 6 {
        let framed = chunk(6, &bytes[6..6 + end]);
        support::without_heap(|| {
            let root = Chunks::parse(&framed, LIMITS)
                .unwrap()
                .iter()
                .next()
                .unwrap();
            assert!(Script::parse(root, LIMITS).is_err());
        });
    }
    for state in [
        &[1][..],
        &1u32.to_le_bytes()[..],
        &u32::MAX.to_le_bytes()[..],
    ] {
        let malformed = script(None, state);
        support::without_heap(|| {
            let root = Chunks::parse(&malformed, LIMITS)
                .unwrap()
                .iter()
                .next()
                .unwrap();
            assert!(
                Script::parse(root, LIMITS).is_err(),
                "never discard a damaged persistence table"
            );
        });
    }
    for flag in [2, 255] {
        let mut invalid = bytes.clone();
        invalid[6] = flag;
        let root = Chunks::parse(&invalid, LIMITS)
            .unwrap()
            .iter()
            .next()
            .unwrap();
        assert_eq!(Script::parse(root, LIMITS).unwrap_err().offset, 6);
    }
    for count in [0u32, 2, u32::MAX] {
        let mut records = count.to_le_bytes().to_vec();
        records.extend(object(0x102, &[], &[], &[]));
        let invalid = chunk(0x33, &records);
        let root = Chunks::parse(&invalid, LIMITS)
            .unwrap()
            .iter()
            .next()
            .unwrap();
        assert!(root.records(LIMITS).is_err());
    }
    let absent = script(None, &[]);
    let empty = script(Some(&[]), &0u32.to_le_bytes());
    let decode = |b| {
        Script::parse(
            Chunks::parse(b, LIMITS).unwrap().iter().next().unwrap(),
            LIMITS,
        )
        .unwrap()
    };
    assert!(decode(&absent).text.is_none());
    assert!(decode(&absent).persistent.is_none());
    assert_eq!(decode(&empty).text.unwrap().data(), []);
    assert!(decode(&empty).persistent.unwrap().is_empty());
}

#[test]
fn saved_ksp_source_uses_the_same_native_compiler_and_audio_owners() {
    use sampler_core::*;
    let bytes = unstructured_script(b"on note change_note($EVENT_ID, 60) end on");
    let parsed = Script::parse(
        Chunks::parse(&bytes, LIMITS)
            .unwrap()
            .iter()
            .next()
            .unwrap(),
        LIMITS,
    )
    .unwrap();
    // Authored UTF-8, with no linked resource or saved values. Production admission
    // still has to resolve those requirements and every unmodeled source field.
    let source = std::str::from_utf8(parsed.text.unwrap().data()).unwrap();
    let script = sampler_ksp::compile(
        source,
        48000,
        sampler_ksp::Limits {
            source_bytes: 65536,
            instructions: 1024,
            variables: 16,
            array_cells: 16,
        },
        &[],
    )
    .unwrap();
    drop(bytes); // Prepared behavior owns instructions, never borrowed source bytes.
    let plan = script
        .bind(
            Prepared::new(
                48000,
                vec![Pcm::new(48000, Box::from([[0.25; 2]; 16])).unwrap()],
                vec![Region {
                    sample: 0,
                    key_low: 60,
                    key_high: 60,
                    root_key: None,
                    velocity_low: 0.,
                    velocity_high: 1.,
                    gain: 1.,
                    envelope: Envelope::default(),
                    playback: Playback::default(),
                }],
                1,
            )
            .unwrap(),
        )
        .unwrap();
    let behavior_cells = plan.behavior_local_count() * 4;
    let note_cells = plan.note_cell_count() * 2;
    let mut runtime = Runtime::new(
        plan,
        sampler_core::Limits {
            notes: 2,
            families: 2,
            voices: 2,
            expressions: 2,
            channels: 0,
            performances: 1,
            decisions: 0,
            commands: 4,
            behaviors: 4,
            behavior_fuel: 128,
            behavior_cells,
            note_cells,
        },
    )
    .unwrap();
    support::without_heap(|| {
        let note = runtime
            .trigger(
                Input {
                    protocol: Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 64,
                    external_id: None,
                },
                64,
                1.,
            )
            .unwrap();
        assert_eq!(runtime.voice_count(), 1);
        let mut output = [[0.; 2]; 16];
        runtime.render(&mut output).unwrap();
        assert_eq!(output, [[0.25; 2]; 16]);
        runtime.key_up(note, None).unwrap();
    });
}
