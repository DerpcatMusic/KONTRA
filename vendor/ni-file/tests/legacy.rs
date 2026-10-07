//! Authored legacy wrapper fixtures; extraction is not XML semantic translation.
use flate2::{write::ZlibEncoder, Compression};
use ni_file::{
    kontakt::schemas::{KontaktV1, KontaktV2, XMLDocument},
    nks::container::NKSContainer,
};
use std::io::{Cursor, Write};
fn zlib(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}
#[test]
fn v1_offset_v2_xml_and_expansion_budgets() {
    let xml = b"<?xml version=\"1.0\"?><AuthoredFixture/>";
    let compressed = zlib(xml);
    let mut v1 = vec![0; 48];
    v1[..4].copy_from_slice(&0xb36ee55eu32.to_le_bytes());
    v1[4..8].copy_from_slice(&48u32.to_le_bytes());
    v1[8..10].copy_from_slice(&80u16.to_le_bytes());
    v1.extend(&compressed);
    let mut v2 = vec![0; 170];
    v2[..4].copy_from_slice(&0x7fa89012u32.to_le_bytes());
    v2[4..8].copy_from_slice(&(compressed.len() as u32).to_le_bytes());
    v2[8..10].copy_from_slice(&0x100u16.to_le_bytes());
    v2[10..14].copy_from_slice(&0x3e012a72u32.to_le_bytes());
    v2.extend(&compressed);
    for bytes in [&v1, &v2] {
        let nks = NKSContainer::read(Cursor::new(bytes)).unwrap();
        assert_eq!(nks.decompressed_preset_bounded(xml.len()).unwrap(), xml);
        assert!(nks.decompressed_preset_bounded(xml.len() - 1).is_err());
        assert!(nks.preset().is_ok());
    }
    v1[4..8].copy_from_slice(&35u32.to_le_bytes());
    assert!(NKSContainer::read(Cursor::new(v1)).is_err());
    assert!(KontaktV1::read(Cursor::new([0xff])).is_err());
    assert!(KontaktV2::read(Cursor::new([0xff])).is_err());
    assert!(XMLDocument::from_compressed_data(&zlib(&[0xff])).is_err());
}
