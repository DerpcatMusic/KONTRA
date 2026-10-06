use std::{fs, process::Command, time::SystemTime};

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
