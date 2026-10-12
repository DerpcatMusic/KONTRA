use std::{fs, process::Command, time::SystemTime};
#[path = "../../sampler-kontakt/tests/support/chunks.rs"]
mod chunks;
#[path = "../../sampler-kontakt/tests/support/nis.rs"]
mod nis;

#[test]
fn expanded_source_inspection_preserves_input_and_reports_malformed_records() {
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "sampler-kontakt-inspect-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("expanded.bin");
    // Program v0xaf: empty private/public fields; one opaque child 0xbeef.
    let bytes = [
        0x28, 0, 22, 0, 0, 0, 1, 0xaf, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0xef, 0xbe, 1, 0, 0,
        0, 0xaa,
    ];
    fs::write(&path, bytes).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_sampler-native"))
            .arg("inspect-kontakt-chunks")
            .arg(&path)
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("playback not admitted"));
    assert!(text.contains("program version 0x00af"));
    assert!(text.contains("unmodeled child 0xbeef at 21: 1 bytes retained"));
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let mut nks = vec![0; 222];
    nks[..4].copy_from_slice(&0x7fa89012u32.to_le_bytes());
    nks[4..8].copy_from_slice(&29u32.to_le_bytes());
    nks[8..10].copy_from_slice(&0x110u16.to_le_bytes());
    nks[10..14].copy_from_slice(&0xea37631au32.to_le_bytes());
    nks[186..190].copy_from_slice(&28u32.to_le_bytes());
    nks.push(27); // A single 28-byte FastLZ literal.
    nks.extend(bytes);
    nks.extend(0xb00ee1aeu32.to_le_bytes());
    nks.extend([1, 1, 12, 0]);
    fs::write(&path, &nks).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sampler-native"))
        .arg("inspect-kontakt-nks")
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), text);
    assert_eq!(fs::read(&path).unwrap(), nks);
    let base = nis::layer(b"NISD", 1, &[], &[]);
    let mut properties = 1u32.to_le_bytes().to_vec();
    properties.extend(0u32.to_le_bytes());
    properties.extend(1u32.to_le_bytes());
    properties.extend((bytes.len() as u64).to_le_bytes());
    properties.extend(bytes);
    let payload = nis::item(&nis::layer(b"NISD", 0x6d, &properties, &base), &[]);
    for (compressed, protected, duplicate) in [
        (false, false, false),
        (true, false, false),
        (true, true, false),
        (false, false, true),
    ] {
        let encryption = nis::encryption(&payload, compressed, protected);
        let children = if duplicate {
            vec![encryption.clone(), encryption]
        } else {
            vec![encryption]
        };
        let preset = nis::item(&nis::layer(b"NIK4", 3, &[], &base), &children);
        let document = nis::item(&nis::layer(b"NISD", 0x76, &[], &base), &[preset]);
        fs::write(&path, &document).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_sampler-native"))
            .arg("inspect-kontakt-nis")
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), document);
        if protected || duplicate {
            assert!(!output.status.success());
            assert!(
                String::from_utf8(output.stderr)
                    .unwrap()
                    .contains(if protected {
                        "AccessRequired"
                    } else {
                        "Ambiguous"
                    })
            );
        } else {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(String::from_utf8(output.stdout).unwrap(), text);
        }
    }
    let mut group = vec![0; 40];
    group[4..8].copy_from_slice(&2f32.to_le_bytes());
    group[12..16].copy_from_slice(&0.5f32.to_le_bytes());
    let mut groups = 1u32.to_le_bytes().to_vec();
    groups.extend(chunks::object(0x95, &[], &group, &[]));
    let mut zone = vec![0; 46];
    zone[14..16].copy_from_slice(&127i16.to_le_bytes());
    zone[18..20].copy_from_slice(&127i16.to_le_bytes());
    zone[28..30].copy_from_slice(&60i16.to_le_bytes());
    zone[30..34].copy_from_slice(&1f32.to_le_bytes());
    zone[38..42].copy_from_slice(&1f32.to_le_bytes());
    zone[42..46].copy_from_slice(&17i32.to_le_bytes());
    let mut loop_data = vec![8, 0, 0x60, 0]; // Original slot 3, unstructured v0x60.
    for value in [2i32, 1, 8, 3] {
        loop_data.extend(value.to_le_bytes());
    }
    loop_data.push(1);
    loop_data.extend(0.5f32.to_le_bytes());
    loop_data.extend(2i32.to_le_bytes());
    let mut zones = 1u32.to_le_bytes().to_vec();
    zones.extend(0u32.to_le_bytes());
    zones.extend(chunks::object(
        0x98,
        &[],
        &zone,
        &chunks::chunk(0x39, &loop_data),
    ));
    let mut children = chunks::chunk(0x33, &groups);
    children.extend(chunks::chunk(0x34, &zones));
    let mapping = chunks::chunk(0x28, &chunks::object(0xaf, &[], &[], &children));
    fs::write(&path, &mapping).unwrap();
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("group 0: gain 2, pan 0, tune ratio 0.5"));
    assert!(
        text.contains("zone 0: group 0, sample ID 17, keys [0, 127], velocities [0, 127], root 60")
    );
    assert!(text.contains("loop slot 3: mode 2, start 1, length 8, count 3, alternating true, tune ratio 0.5, crossfade 2"));
    assert_eq!(fs::read(&path).unwrap(), mapping);
    fs::write(&path, &bytes[..bytes.len() - 1]).unwrap();
    let output = run();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Truncated")
    );
    fs::remove_dir_all(directory).unwrap();
}
