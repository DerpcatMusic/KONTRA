#![cfg(feature = "library-access")]

use sampler_ir as ir;
use sampler_kontakt::{ResourceContainer, Resources};
use std::{io::Cursor, path::PathBuf};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kontra-resource-record-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("Instruments")).unwrap();
        Self(root)
    }

    fn container(&self, members: &[(u64, &str, &[u8])]) -> PathBuf {
        // Same existing FileContainer framing as tests/resources.rs. The
        // opaque nonzero metadata has no asserted UUID/type/version meaning.
        let mut bytes = b"/\\ NI FC MTD  /\\".to_vec();
        bytes.extend([0; 256]);
        bytes.extend((members.len() as u64).to_le_bytes());
        bytes.extend((members.iter().map(|m| m.2.len()).sum::<usize>() as u64).to_le_bytes());
        bytes.extend(b"/\\ NI FC TOC  /\\");
        bytes.extend([0; 600]);
        let mut end = 0u64;
        for &(id, name, data) in members {
            bytes.extend(id.to_le_bytes());
            bytes.extend([0x80; 16]);
            let mut filename: Vec<_> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
            assert!(filename.len() < 600);
            filename.resize(600, 0);
            bytes.extend(filename);
            bytes.extend(u64::MAX.to_le_bytes());
            end += data.len() as u64;
            bytes.extend(end.to_le_bytes());
        }
        bytes.extend([0xf1; 8]);
        bytes.extend([0; 16]);
        bytes.extend(b"/\\ NI FC TOC  /\\");
        bytes.extend([0; 592]);
        for member in members {
            bytes.extend(member.2);
        }
        let parsed = ni_file::file_container::NIFileContainer::read(Cursor::new(&bytes)).unwrap();
        for (item, &(id, name, data)) in parsed.items.iter().zip(members) {
            assert_eq!((item.index, item.filename.as_str()), (id, name));
            assert_eq!(item.opaque_metadata, [0x80; 16]);
            assert_eq!(item.unknown_word, u64::MAX);
            assert_eq!(item.file_size, data.len() as u64);
        }
        // NICNT has an outer product header before its embedded FileContainer.
        let mut nicnt = b"/\\ NI FC MTD  /\\".to_vec();
        nicnt.extend([0; 300]);
        nicnt.extend(bytes);
        let path = self.0.join("authored.nicnt");
        std::fs::write(&path, nicnt).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn versioned_resource_names_and_opaque_bytes_reach_typed_nckp_picture_consumer() {
    let fixture = Fixture::new();
    let view = br#"{"value":{"performanceView":{"size":{"width":640,"height":300},
        "controls":[{"index":3,"value":{"common":{"id":"Knob"},
        "image":"knob_v8.1","value":{"min":0,"max":100,"default":42},"ratio":1}}]}}}"#;
    // Header-only PNG: this tests metadata, not pixel decoding or painting.
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend(40u32.to_be_bytes());
    png.extend(310u32.to_be_bytes());
    let raw = [0xff, 0x00, 0x80, 0x01];
    let members: &[(u64, &str, &[u8])] = &[
        (
            u64::MAX,
            "Resources|performance_view|main_v8.1.nckp",
            view,
        ),
        (900, "Resources|pictures|knob_v8.1.png", &png),
        (
            42,
            "Resources|pictures|knob_v8.1.txt",
            b"Number of Animations: 31\n",
        ),
        (1234, "Resources|opaque|attachment.bin", &raw),
    ];
    let path = fixture.container(members);
    let mut container = ResourceContainer::open(&path).unwrap();
    for &(_, name, data) in members {
        assert!(container.names().contains(&name));
        assert_eq!(container.read(name).unwrap().as_deref(), Some(data));
    }
    let mut resources = Resources::of(&fixture.0.join("Instruments/Authored.nki"));
    assert_eq!(
        resources
            .read_result("Resources/opaque/attachment.bin")
            .unwrap(),
        Some(raw.to_vec())
    );
    let mut instrument = ir::Instrument {
        behaviors: vec![ir::Behavior {
            name: "authored resource record".into(),
            language: ir::Language::Ksp,
            source: "on init load_performance_view(\"main_v8.1\") end on".into(),
            slot: Some(0),
            state: Vec::new(),
            requires: Vec::new(),
        }],
        ..Default::default()
    };
    let (scripts, interfaces, _) = sampler_kontakt::compile_ui(
        &mut instrument,
        &sampler_kontakt::Options {
            library: Some(fixture.0.join("Instruments/Authored.nki")),
            ..Default::default()
        },
    );
    assert!(
        instrument.unsupported.is_empty(),
        "{:?}",
        instrument.unsupported
    );
    assert_eq!(scripts.len(), 1);
    assert_eq!(interfaces.len(), 1);
    assert_eq!(
        interfaces[0].widgets[0].value,
        Some(sampler_ui_ir::Value::Integer(42))
    );
    let asset = interfaces[0]
        .assets
        .iter()
        .find(|a| a.path == "Resources/pictures/knob_v8.1.png")
        .unwrap();
    let sampler_ui_ir::AssetKind::Image(meta) = asset.kind else {
        panic!("expected image metadata")
    };
    assert_eq!(meta.frames, 31);
    assert_eq!(meta.size.map(|s| (s.width, s.height)), Some((40.0, 10.0)));
}

#[test]
fn nicnt_distinct_names_with_duplicate_ids_read_selected_payloads() {
    use std::io::{Seek, SeekFrom};

    let fixture = Fixture::new();
    let members: &[(u64, &str, &[u8])] = &[
        (42, "Resources|opaque|first.bin", b"first"),
        (42, "Resources|opaque|second.bin", b"second"),
    ];
    let path = fixture.container(members);
    let mut container = ResourceContainer::open(&path).unwrap();
    for (name, data) in [
        ("resources/opaque/FIRST.BIN", b"first".as_slice()),
        ("Resources\\opaque\\second.bin", b"second".as_slice()),
        ("Resources|opaque|first.bin", b"first".as_slice()),
    ] {
        assert_eq!(container.read(name).unwrap().as_deref(), Some(data));
    }
    assert!(container
        .read("Resources/opaque/missing.bin")
        .unwrap()
        .is_none());
    let mut resources = Resources::of(&fixture.0.join("Instruments/Authored.nki"));
    for &(_, name, data) in members {
        assert_eq!(resources.read_result(name).unwrap().as_deref(), Some(data));
    }

    // The embedded container starts after the authored 16 + 300 byte prefix.
    // Duplicate IDs remain ambiguous only for the ID-addressed public API.
    let mut file = std::fs::File::open(&path).unwrap();
    file.seek(SeekFrom::Start(316)).unwrap();
    let parsed = ni_file::file_container::NIFileContainer::read(&mut file).unwrap();
    let error = parsed.read_member(&mut file, 42, 32 << 20).unwrap_err();
    assert!(matches!(
        error,
        ni_file::Error::Static("Ambiguous NI FileContainer member index")
    ));
}

#[test]
fn nicnt_duplicate_normalized_names_remain_ambiguous_with_distinct_ids() {
    let fixture = Fixture::new();
    let path = fixture.container(&[
        (42, "Resources|opaque|same.bin", b"first"),
        (900, "resources\\opaque\\SAME.BIN", b"second"),
    ]);
    let mut container = ResourceContainer::open(&path).unwrap();
    let error = container.read("Resources/opaque/same.bin").unwrap_err();
    assert!(error.to_string().contains("Ambiguous resource name"));
}

#[test]
fn nicnt_selected_item_read_rejects_truncation_after_open() {
    let fixture = Fixture::new();
    let path = fixture.container(&[
        (42, "Resources|opaque|first.bin", b"first"),
        (42, "Resources|opaque|second.bin", b"second"),
    ]);
    let mut container = ResourceContainer::open(&path).unwrap();
    let length = std::fs::metadata(&path).unwrap().len();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(length - 1)
        .unwrap();
    assert_eq!(
        container.read("Resources/opaque/first.bin").unwrap().as_deref(),
        Some(b"first".as_slice())
    );
    assert!(container.read("Resources/opaque/second.bin").is_err());
}

#[test]
fn nicnt_selected_item_read_preserves_exact_32_mib_budget() {
    use std::io::{Seek, SeekFrom, Write};

    // Extend a one-byte authored member as a sparse file, without a large
    // fixture buffer. These are framing offsets, not stored-ID ordinals.
    for size in [(32u64 << 20) - 1, 32 << 20, (32 << 20) + 1] {
        let fixture = Fixture::new();
        let path = fixture.container(&[(42, "Resources|opaque|large.bin", &[0])]);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap();
        for offset in [316 + 280, 316 + 904 + 632] {
            file.seek(SeekFrom::Start(offset)).unwrap();
            file.write_all(&size.to_le_bytes()).unwrap();
        }
        let length = file.metadata().unwrap().len();
        file.set_len(length - 1 + size).unwrap();
        drop(file);
        let mut container = ResourceContainer::open(&path).unwrap();
        let result = container.read("Resources/opaque/large.bin");
        if size <= 32 << 20 {
            let bytes = result.unwrap().unwrap();
            assert_eq!(bytes.len() as u64, size);
            assert!(bytes.iter().all(|&byte| byte == 0));
        } else {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("Resource exceeds 32 MiB"));
        }
    }
}

#[test]
fn resource_importer_reports_unsupported_nkr_directory_version() {
    let fixture = Fixture::new();
    let path = fixture.0.join("unsupported.nkr");
    let mut bytes = 0x5e70ac54u32.to_le_bytes().to_vec();
    bytes.extend(0x112u16.to_le_bytes());
    bytes.extend([0; 16]);
    std::fs::write(&path, bytes).unwrap();
    let error = match ResourceContainer::open(&path) {
        Ok(_) => panic!("unsupported directory version must not be admitted"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains("Unsupported NKX directory version 0x112"),
        "{error}"
    );
}
