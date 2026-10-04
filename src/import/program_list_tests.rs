use ni_file::kontakt::{Chunk, KontaktChunks, objects::ProgramList};

fn object(id: u16, version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Chunk {
    let mut data = vec![1];
    data.extend(version.to_le_bytes());
    for part in [private, public, children] {
        data.extend((part.len() as u32).to_le_bytes());
        data.extend(part);
    }
    Chunk { id, data }
}

fn list(slots: &[i16]) -> Chunk {
    let mut data = (slots.len() as i16).to_le_bytes().to_vec();
    for (index, slot) in slots.iter().enumerate() {
        let mut children = Vec::new();
        Chunk {
            id: 0x7fff,
            data: vec![index as u8, 0x80],
        }
        .write(&mut children)
        .unwrap();
        let program = object(
            0x28,
            0x80 + index as u16,
            &[0xff, index as u8],
            &[index as u8, 0, 0x7f],
            &children,
        );
        data.extend(slot.to_le_bytes());
        // ProgramList stores a StructuredObject directly, without its Chunk header.
        data.extend(program.data);
    }
    Chunk { id: 0x36, data }
}

fn multi(slots: &[i16]) -> KontaktChunks {
    let mut children = Vec::new();
    list(slots).write(&mut children).unwrap();
    let container = object(0x29, 0x51, &[], &[], &children);
    let mut outer = vec![1 << 5, 0, 0, 0, 0, 0, 0, 0];
    container.write(&mut outer).unwrap();
    let mut children = Vec::new();
    Chunk {
        id: 0x37,
        data: outer,
    }
    .write(&mut children)
    .unwrap();
    let mut public = Vec::new();
    public.extend(1f32.to_le_bytes());
    public.extend(0f32.to_le_bytes());
    public.extend(120i32.to_le_bytes());
    public.extend(4u32.to_le_bytes());
    public.extend("Bank".encode_utf16().flat_map(u16::to_le_bytes));
    KontaktChunks(vec![object(3, 0x50, &[], &public, &children)])
}

#[test]
fn serialized_program_slots_preserve_payload_and_outer_container_selection() {
    let mut bytes = Vec::new();
    list(&[7, 119, -3]).write(&mut bytes).unwrap();
    let chunk = Chunk::read(std::io::Cursor::new(bytes)).unwrap();
    let parsed = ProgramList::try_from(&chunk).unwrap();
    assert_eq!(
        parsed
            .programs
            .iter()
            .map(|(slot, _)| *slot)
            .collect::<Vec<_>>(),
        [7, 119, -3]
    );
    for (index, (_, program)) in parsed.programs.into_iter().enumerate() {
        assert_eq!(program.0.version, 0x80 + index as u16);
        assert_eq!(program.0.private_data, [0xff, index as u8]);
        assert_eq!(program.0.public_data, [index as u8, 0, 0x7f]);
        assert_eq!(program.0.children.len(), 1);
        assert_eq!(program.0.children[0].id, 0x7fff);
        assert_eq!(program.0.children[0].data, [index as u8, 0x80]);
    }
    let mut bytes = Vec::new();
    multi(&[119]).write(&mut bytes).unwrap();
    let chunks = KontaktChunks::read(std::io::Cursor::new(bytes)).unwrap();
    let (name, programs) = super::multi_programs(&chunks).unwrap();
    assert_eq!(name, "Bank");
    assert_eq!(programs.len(), 1);
    assert_eq!(
        programs[0].0, 5,
        "saved Part.program remains the outer container, not inner119"
    );
    assert_eq!(programs[0].1.0.private_data, [0xff, 0]);
    let error = super::multi_programs(&multi(&[7, 119])).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Instrument banks with program switching are not supported"
    );
}
