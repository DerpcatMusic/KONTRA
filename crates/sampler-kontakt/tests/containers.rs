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
