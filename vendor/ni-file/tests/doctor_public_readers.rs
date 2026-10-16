//! Causal regressions using only the pre-remediation public API.
//! Apply this file alone to published503 for RED, then keep it byte-identical
//! while applying vendor-remediation.patch for GREEN. No new error variants.
use std::io::Cursor;

use ni_file::{
    fm8::FM8Preset,
    kontakt::objects::SaveSettings,
    nis::{Authorization, RepositoryRoot},
};

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn fm8_header(version: u32) -> Vec<u8> {
    words(&[u32::from_be_bytes(*b"FM8E"), version])
}

#[test]
fn causal_fm8_rejects_bad_magic() {
    assert!(FM8Preset::read(Cursor::new([0; 8])).is_err());
}

#[test]
fn causal_fm8_rejects_unsupported_version() {
    assert!(FM8Preset::read(Cursor::new(fm8_header(0xD0))).is_err());
}

#[test]
fn causal_save_settings_refuses_unsupported_structure() {
    assert!(SaveSettings::read(Cursor::new([1])).is_err());
}

#[test]
fn causal_save_settings_rejects_invalid_boolean_tag() {
    assert!(SaveSettings::read(Cursor::new([2])).is_err());
}

#[test]
fn causal_authorization_rejects_version() {
    assert!(Authorization::read(Cursor::new(words(&[2, 0, 0, 1, 0]))).is_err());
}

#[test]
fn causal_authorization_refuses_nonempty_license_info() {
    // version, has LicenseInfo, one SNP ID, one UTF-16 code unit
    let mut bytes = words(&[1, 1, 1, 1]);
    bytes.extend(u16::from(b'A').to_le_bytes());
    bytes.extend(words(&[0, 1, 0]));
    assert!(Authorization::read(Cursor::new(bytes)).is_err());
}

#[test]
fn causal_repository_rejects_item_version() {
    assert!(RepositoryRoot::read(Cursor::new(words(&[2, 0x10700E, 0, 2, 1, 0]))).is_err());
}

#[test]
fn causal_repository_rejects_segments_version() {
    assert!(RepositoryRoot::read(Cursor::new(words(&[1, 0x10700E, 0, 2, 2, 0]))).is_err());
}

#[test]
fn control_supported_reader_layouts_still_accept_and_consume() {
    let mut save = Cursor::new([0]);
    SaveSettings::read(&mut save).unwrap();
    assert_eq!(save.position(), 1);
    for bytes in [words(&[1, 0, 0, 24, 7]), words(&[1, 1, 1, 0, 0, 24, 7])] {
        let mut reader = Cursor::new(&bytes);
        Authorization::read(&mut reader).unwrap();
        assert_eq!(reader.position() as usize, bytes.len());
    }
    // One nonempty UTF-16 segment (including a surrogate pair).
    let segment = "音楽🎹";
    let mut bytes = words(&[1, 0x10700E, 0, 2, 1, 1, segment.encode_utf16().count() as u32]);
    for unit in segment.encode_utf16() { bytes.extend(unit.to_le_bytes()); }
    let mut reader = Cursor::new(&bytes);
    let parsed = RepositoryRoot::read(&mut reader).unwrap();
    assert_eq!(parsed.segments, [segment]);
    assert_eq!(reader.position() as usize, bytes.len());
    let mut encoded = words(&[1, 0x10700E, parsed.repository_magic, parsed.repository_type,
                              1, parsed.segments.len() as u32]);
    for s in parsed.segments {
        encoded.extend((s.encode_utf16().count() as u32).to_le_bytes());
        for unit in s.encode_utf16() { encoded.extend(unit.to_le_bytes()); }
    }
    assert_eq!(encoded, bytes);
}

#[test]
fn control_supported_fm8_layouts_still_accept_and_reparse() {
    for version in [0xC9, 0xCA, 0xCF] {
        let mut bytes = fm8_header(version);
        bytes.resize(64 * 1024, 0);
        let mut reader = Cursor::new(bytes);
        FM8Preset::read(&mut reader).unwrap();
        let end = reader.position() as usize;
        assert!(end > 8 && end < reader.get_ref().len());
        let exact = &reader.get_ref()[..end];
        let mut reader = Cursor::new(exact);
        FM8Preset::read(&mut reader).unwrap();
        assert_eq!(reader.position() as usize, exact.len());
        assert!(FM8Preset::read(Cursor::new(&exact[..end-1])).is_err());
    }
}

#[test]
fn control_truncated_public_inputs_return_errors() {
    for n in 0..4 {
        assert!(FM8Preset::read(Cursor::new(vec![0; n])).is_err());
        assert!(Authorization::read(Cursor::new(vec![0; n])).is_err());
        assert!(RepositoryRoot::read(Cursor::new(vec![0; n])).is_err());
    }
    assert!(SaveSettings::read(Cursor::new([])).is_err());
}
