use sampler_kontakt::{
    Bank, Chunks, FileTable, Limits, ProgramList, QuickBrowse, SaveSettings, SlotList,
};

const LIMITS: Limits = Limits {
    bytes: 4096,
    records: 64,
};
fn chunk(id: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = id.to_le_bytes().to_vec();
    bytes.extend((body.len() as u32).to_le_bytes());
    bytes.extend(body);
    bytes
}
fn object(version: u16, public: &[u8]) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend(version.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    bytes.extend((public.len() as u32).to_le_bytes());
    bytes.extend(public);
    bytes.extend(0u32.to_le_bytes());
    bytes
}
fn wide(text: &str) -> Vec<u8> {
    let units: Vec<_> = text.encode_utf16().collect();
    let mut bytes = (units.len() as u32).to_le_bytes().to_vec();
    for unit in units {
        bytes.extend(unit.to_le_bytes());
    }
    bytes
}
fn filename() -> Vec<u8> {
    let mut bytes = 3i32.to_le_bytes().to_vec();
    bytes.push(11);
    bytes.push(2);
    bytes.extend(wide("Samples"));
    bytes.push(4);
    bytes.extend(wide("𝄞.ncw"));
    bytes
}

#[test]
fn filename_versions_retain_segments_metadata_and_extensions() {
    let name = filename();
    let mut v2 = 2u16.to_le_bytes().to_vec();
    v2.extend(0u32.to_le_bytes());
    v2.extend(1u32.to_le_bytes());
    v2.extend(&name);
    v2.extend(u64::MAX.to_le_bytes());
    v2.extend(0x12345678u32.to_le_bytes());
    v2.extend(0u32.to_le_bytes());
    let required = v2.len();
    v2.extend([9, 8]);
    let bytes = chunk(0x4b, &v2);
    let table = FileTable::parse(
        Chunks::parse(&bytes, LIMITS)
            .unwrap()
            .iter()
            .next()
            .unwrap(),
        LIMITS,
    )
    .unwrap();
    assert_eq!(table.samples[0].timestamp, Some(u64::MAX));
    assert_eq!(table.samples[0].unknown_record, Some(0x12345678));
    assert_eq!(
        table.samples[0]
            .filename
            .segments()
            .map(|s| s.kind)
            .collect::<Vec<_>>(),
        [11, 2, 4]
    );
    assert_eq!(table.extension.data(), [9, 8]);
    for end in 0..required {
        let bytes = chunk(0x4b, &v2[..end]);
        assert!(
            FileTable::parse(
                Chunks::parse(&bytes, LIMITS)
                    .unwrap()
                    .iter()
                    .next()
                    .unwrap(),
                LIMITS
            )
            .is_err(),
            "prefix {end}"
        );
    }
    let mut v3 = 3u16.to_le_bytes().to_vec();
    v3.extend(1u32.to_le_bytes());
    v3.extend([7; 8]);
    v3.extend(&name);
    v3.extend([8; 20]);
    let bytes = chunk(0x4b, &v3);
    let table = FileTable::parse(
        Chunks::parse(&bytes, LIMITS)
            .unwrap()
            .iter()
            .next()
            .unwrap(),
        LIMITS,
    )
    .unwrap();
    assert!(table.samples.is_empty());
    assert_eq!(table.flat[0].prefix.unwrap().data(), [7; 8]);
    assert_eq!(table.flat[0].suffix.unwrap().data(), [8; 20]);
    let mut bad = v3.clone();
    bad[..2].copy_from_slice(&4u16.to_le_bytes());
    let bytes = chunk(0x4b, &bad);
    assert!(
        FileTable::parse(
            Chunks::parse(&bytes, LIMITS)
                .unwrap()
                .iter()
                .next()
                .unwrap(),
            LIMITS
        )
        .is_err()
    );
}

#[test]
fn settings_and_quick_browse_reject_invalid_flags_and_preserve_extensions() {
    let mut settings = vec![0, 0x10, 0];
    settings.extend(42u32.to_le_bytes());
    settings.extend((-1i32).to_le_bytes());
    settings.extend((-1i32).to_le_bytes());
    settings.extend([1, 0, 1]);
    let required = settings.len();
    settings.extend([0xab]);
    let bytes = chunk(0x47, &settings);
    let parsed = SaveSettings::parse(
        Chunks::parse(&bytes, LIMITS)
            .unwrap()
            .iter()
            .next()
            .unwrap(),
        LIMITS,
    )
    .unwrap();
    assert_eq!(parsed.flags, [true, false, true]);
    assert_eq!(parsed.translated, 42);
    assert_eq!(parsed.original, -1);
    assert_eq!(parsed.unknown, -1);
    assert_eq!(parsed.extension.data(), [0xab]);
    for end in 0..required {
        let bytes = chunk(0x47, &settings[..end]);
        assert!(
            SaveSettings::parse(
                Chunks::parse(&bytes, LIMITS)
                    .unwrap()
                    .iter()
                    .next()
                    .unwrap(),
                LIMITS
            )
            .is_err()
        );
    }
    settings[required - 1] = 2;
    let bytes = chunk(0x47, &settings);
    assert!(
        SaveSettings::parse(
            Chunks::parse(&bytes, LIMITS)
                .unwrap()
                .iter()
                .next()
                .unwrap(),
            LIMITS
        )
        .is_err()
    );
    let bytes = chunk(0x4e, &[0, 1, 0, 7, 0, 0, 0, 9]);
    let parsed = QuickBrowse::parse(
        Chunks::parse(&bytes, LIMITS)
            .unwrap()
            .iter()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(parsed.unknown, 7);
    assert_eq!(parsed.extension.data(), [9]);
}

#[test]
fn bank_lists_preserve_program_numbers_and_sparse_slot_identity() {
    let mut public = 0.5f32.to_le_bytes().to_vec();
    public.extend(1.0f32.to_le_bytes());
    public.extend(120i32.to_le_bytes());
    public.extend(wide("Bank"));
    public.extend([8, 9]);
    for version in [0x73, 0x76] {
        let bytes = chunk(3, &object(version, &public));
        let bank = Bank::parse(
            Chunks::parse(&bytes, LIMITS)
                .unwrap()
                .iter()
                .next()
                .unwrap(),
        )
        .unwrap();
        assert_eq!((bank.volume, bank.tune, bank.tempo), (0.5, 1.0, 120));
        assert_eq!(bank.extension.data(), [8, 9]);
    }
    let mut programs = 2i16.to_le_bytes().to_vec();
    for number in [3i16, 127] {
        programs.extend(number.to_le_bytes());
        programs.extend(object(0xaf, &[]));
    }
    let bytes = chunk(0x36, &programs);
    let parsed = ProgramList::parse(
        Chunks::parse(&bytes, LIMITS)
            .unwrap()
            .iter()
            .next()
            .unwrap(),
        LIMITS,
    )
    .unwrap();
    assert_eq!(
        parsed.0.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        [3, 127]
    );
    let mut slots = ((1u64 << 1) | (1u64 << 63)).to_le_bytes().to_vec();
    for _ in 0..2 {
        slots.extend(chunk(0x29, &object(0x51, &[])));
    }
    let bytes = chunk(0x37, &slots);
    let parsed = SlotList::parse(
        Chunks::parse(&bytes, LIMITS)
            .unwrap()
            .iter()
            .next()
            .unwrap(),
        LIMITS,
    )
    .unwrap();
    assert_eq!(
        parsed.0.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        [1, 63]
    );
    slots.push(0);
    let bytes = chunk(0x37, &slots);
    assert!(
        SlotList::parse(
            Chunks::parse(&bytes, LIMITS)
                .unwrap()
                .iter()
                .next()
                .unwrap(),
            LIMITS
        )
        .is_err()
    );
    let bytes = chunk(0x36, &(-1i16).to_le_bytes());
    assert!(
        ProgramList::parse(
            Chunks::parse(&bytes, LIMITS)
                .unwrap()
                .iter()
                .next()
                .unwrap(),
            LIMITS
        )
        .is_err()
    );
}

#[test]
fn program_resource_references_follow_variable_length_metadata() {
    let mut public = wide("𝄞 Piano");
    public.extend([0; 48]);
    for text in ["Credits", "Author", "https://example.test"] {
        public.extend(wide(text));
    }
    public.extend([0; 6]);
    for reference in [3u32, u32::MAX, 7, 11] {
        public.extend(reference.to_le_bytes());
    }
    let required = public.len();
    public.extend([9, 8]);
    for version in [0xa8, 0xab, 0xae, 0xb1, 0xb3, 0xb5] {
        let bytes = chunk(0x28, &object(version, &public));
        let program = sampler_kontakt::ProgramResources::parse(
            Chunks::parse(&bytes, LIMITS)
                .unwrap()
                .iter()
                .next()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            (
                program.container,
                program.snapshot_directory,
                program.full_path,
                program.wallpaper
            ),
            (3, u32::MAX, 7, 11)
        );
        assert_eq!(program.extension.data(), [9, 8]);
    }
    for end in 0..required {
        let bytes = chunk(0x28, &object(0xae, &public[..end]));
        assert!(
            sampler_kontakt::ProgramResources::parse(
                Chunks::parse(&bytes, LIMITS)
                    .unwrap()
                    .iter()
                    .next()
                    .unwrap()
            )
            .is_err()
        );
    }
}
