use ni_file::kontakt::{
    StructuredObject,
    objects::{Bank, ProgramContainer, Zone},
};

fn object(
    children: Vec<ni_file::kontakt::Chunk>,
    version: u16,
    public_data: Vec<u8>,
) -> StructuredObject {
    StructuredObject {
        version,
        children,
        public_data,
        private_data: Vec::new(),
    }
}

#[test]
fn container_children_are_found_by_id_and_truncated_references_are_errors() {
    let bank = Bank(object(
        vec![ni_file::kontakt::Chunk {
            id: 0x37,
            data: vec![0; 8],
        }],
        0x73,
        vec![],
    ));
    assert!(bank.slot_list().unwrap().slots.is_empty());
    assert!(Bank(object(vec![], 0x73, vec![])).slot_list().is_err());
    assert!(
        ProgramContainer(object(vec![], 0x51, vec![]))
            .program_list()
            .is_err()
    );
    for version in [0x95, 0x99, 0x9a, 0xa0] {
        let at = if version >= 0x9a { 48 } else { 42 };
        let mut public = vec![0; at];
        public.extend(123i32.to_le_bytes());
        let mut zone = Zone(object(vec![], version, public));
        assert_eq!(zone.filename_id().unwrap(), 123);
        zone.0.public_data.pop();
        assert!(zone.filename_id().is_err());
    }
}

#[test]
#[cfg(feature = "library-access")]
fn installed_multi_opens_without_single_instrument_translation() {
    let root = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|root| root.join("Conflux 1.1.0 [Native Instruments]/Multis/Hybrid Walk.nkm"))
            .find(|path| path.is_file())
    });
    let Some(path) = root else {
        eprintln!("skipped: installed Conflux multi is absent");
        return;
    };
    let multi = sampler_kontakt::read_multi(&path).unwrap();
    assert!(!multi.programs.is_empty() && !multi.sample_names.is_empty());
}

#[test]
fn ncw_signature_failure_identifies_the_block_and_channel() {
    let mut bytes = ncw::encode_pcm(
        &vec![0; 513],
        ncw::PcmSpec {
            channels: 1,
            bits_per_sample: 16,
            sample_rate: 48000,
        },
        ncw::StereoMode::Direct,
    )
    .unwrap();
    let reader = ncw::NcwReader::read(std::io::Cursor::new(&bytes)).unwrap();
    let at = reader.header.data_offset as usize + reader.block_offsets[1] as usize;
    bytes[at] ^= 1;
    let mut reader = ncw::NcwReader::read(std::io::Cursor::new(&bytes)).unwrap();
    assert!(matches!(
        reader.decode_samples(),
        Err(ncw::NcwError::InvalidBlockSignatureAt {
            block: 1,
            channel: 0
        })
    ));
}
