//! Independently authored FileContainers; no commercial preset/sample fixtures.
use sampler_kontakt::{
    Options, SampleReader, decode, load, read, read_chunks, read_multi, read_program,
};
#[path = "support/nis.rs"]
mod nis_wire;
#[path = "support/chunks.rs"]
mod wire;
use wire::{chunk, object};

fn wide(name: &str) -> Vec<u8> {
    let mut b = (name.encode_utf16().count() as u32).to_le_bytes().to_vec();
    b.extend(name.encode_utf16().flat_map(u16::to_le_bytes));
    b
}
fn preset(sample: &str, multi: bool) -> Vec<u8> {
    let mut group = wide("Authored group");
    for x in [1f32, 0., 1.] {
        group.extend(x.to_le_bytes());
    }
    group.extend([1, 0, 0, 0]);
    group.extend(0i32.to_le_bytes());
    group.extend((-1i16).to_le_bytes());
    for x in [0i32, 0] {
        group.extend(x.to_le_bytes());
    }
    group.extend([0, 0]);
    group.extend(0i32.to_le_bytes());
    let mut groups = 1u32.to_le_bytes().to_vec();
    groups.extend(object(0x95, &[], &group, &chunk(0x38, &0u32.to_le_bytes())));
    let mut zone = vec![0; 12];
    for x in [1i16, 127, 60, 60, 0, 0, 0, 0, 60] {
        zone.extend(x.to_le_bytes());
    }
    for x in [1f32, 0., 1.] {
        zone.extend(x.to_le_bytes());
    }
    zone.extend([0; 6]);
    zone.extend(0i32.to_le_bytes());
    let mut zones = 1u32.to_le_bytes().to_vec();
    zones.extend(0u32.to_le_bytes());
    zones.extend(object(0x9a, &[], &zone, &[]));
    let mut public = wide("Authored monolith");
    public.extend(0f64.to_le_bytes());
    public.push(0);
    for x in [1f32, 0., 1.] {
        public.extend(x.to_le_bytes());
    }
    public.extend([1, 127, 0, 127]);
    public.extend((-1i16).to_le_bytes());
    public.extend([0; 16]);
    public.push(0);
    public.extend(0i32.to_le_bytes());
    for _ in 0..3 {
        public.extend(wide(""));
    }
    public.extend([0; 6]);
    let mut children = chunk(0x33, &groups);
    children.extend(chunk(0x34, &zones));
    let program = object(0xae, &[], &public, &children);
    let mut payload = if multi {
        let mut list = 1i16.to_le_bytes().to_vec();
        list.extend(0i16.to_le_bytes());
        list.extend(program);
        let pc = object(0x51, &[], &[], &chunk(0x36, &list));
        let mut slots = vec![0x20, 0, 0, 0, 0, 0, 0, 0];
        slots.extend(chunk(0x29, &pc));
        chunk(3, &object(0x73, &[], &[], &chunk(0x37, &slots)))
    } else {
        chunk(0x28, &program)
    };
    // Pre-K5.1 table: no special files, one filename, timestamp, no other files.
    let mut table = vec![0; 4];
    table.extend(1u32.to_le_bytes());
    table.extend(1i32.to_le_bytes());
    table.push(4);
    table.extend(wide(sample));
    table.extend([0; 8]);
    table.extend([0; 4]);
    payload.extend(chunk(0x3d, &table));
    let packed: Vec<u8> = payload
        .chunks(32)
        .flat_map(|b| std::iter::once((b.len() - 1) as u8).chain(b.iter().copied()))
        .collect();
    let mut nks = vec![0; 222];
    nks[..4].copy_from_slice(&0x7fa89012u32.to_le_bytes());
    nks[4..8].copy_from_slice(&(packed.len() as u32).to_le_bytes());
    nks[8..10].copy_from_slice(&0x110u16.to_le_bytes());
    nks[10..14].copy_from_slice(&0xea37631au32.to_le_bytes());
    nks[186..190].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    nks.extend(packed);
    nks.extend(0xb00ee1aeu32.to_le_bytes());
    nks.extend([1, 1, 12, 0]);
    nks
}
fn file_container(items: &[(&str, &[u8])]) -> Vec<u8> {
    let mut b = b"/\\ NI FC MTD  /\\".to_vec();
    b.extend([0; 256]);
    b.extend((items.len() as u64).to_le_bytes());
    b.extend((items.iter().map(|(_, b)| b.len()).sum::<usize>() as u64).to_le_bytes());
    b.extend(b"/\\ NI FC TOC  /\\");
    b.extend([0; 600]);
    let mut end = 0u64;
    for (i, (name, data)) in items.iter().enumerate() {
        b.extend((100 + i as u64).to_le_bytes());
        b.extend([0; 16]);
        let mut name: Vec<_> = name
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        name.resize(600, 0);
        b.extend(name);
        b.extend(0u64.to_le_bytes());
        end += data.len() as u64;
        b.extend(end.to_le_bytes());
    }
    b.extend([0xf1; 8]);
    b.extend([0; 16]);
    b.extend(b"/\\ NI FC TOC  /\\");
    b.extend([0; 592]);
    for (_, data) in items {
        b.extend(*data);
    }
    b
}

fn modern_preset(nks: &[u8]) -> Vec<u8> {
    use nis_wire::{encryption, item, layer};
    let chunks = ni_file::NIFile::read(std::io::Cursor::new(nks))
        .unwrap()
        .inner_preset()
        .unwrap();
    let mut properties = 1u32.to_le_bytes().to_vec();
    properties.extend(0u32.to_le_bytes());
    properties.extend(1u32.to_le_bytes());
    properties.extend((chunks.len() as u64).to_le_bytes());
    properties.extend(chunks);
    let base = layer(b"NISD", 1, &[], &[]);
    let payload = item(&layer(b"NISD", 0x6d, &properties, &base), &[]);
    let preset = item(
        &layer(b"NIK4", 3, &[], &base),
        &[encryption(&payload, true, false)],
    );
    item(&layer(b"NISD", 0x76, &[], &base), &[preset])
}
fn wav() -> Vec<u8> {
    let mut b = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0".to_vec();
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(48000u32.to_le_bytes());
    b.extend(96000u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(8192u32.to_le_bytes());
    for _ in 0..4096 {
        b.extend(16384i16.to_le_bytes());
    }
    let size = b.len() as u32 - 8;
    b[4..8].copy_from_slice(&size.to_le_bytes());
    b
}
fn aiff(codec: Option<&[u8; 4]>, bits: u16, sample: &[u8], reverse_chunks: bool) -> Vec<u8> {
    let mut comm = 1u16.to_be_bytes().to_vec();
    comm.extend(4096u32.to_be_bytes());
    comm.extend(bits.to_be_bytes());
    comm.extend([0x40, 0x0e, 0xbb, 0x80, 0, 0, 0, 0, 0, 0]); // IEEE 80-bit 48000.
    if let Some(c) = codec {
        comm.extend(c);
        comm.extend([0, 0]);
    }
    let mut ssnd = 3u32.to_be_bytes().to_vec();
    ssnd.extend(0u32.to_be_bytes());
    ssnd.extend([0; 3]);
    for _ in 0..4096 {
        ssnd.extend(sample);
    }
    fn iff(id: &[u8], body: &[u8]) -> Vec<u8> {
        let mut b = id.to_vec();
        b.extend((body.len() as u32).to_be_bytes());
        b.extend(body);
        if body.len() % 2 != 0 {
            b.push(0);
        }
        b
    }
    let (comm, ssnd) = (iff(b"COMM", &comm), iff(b"SSND", &ssnd));
    let mut b = b"FORM\0\0\0\0".to_vec();
    b.extend(if codec.is_some() { b"AIFC" } else { b"AIFF" });
    for c in if reverse_chunks {
        [&ssnd, &comm]
    } else {
        [&comm, &ssnd]
    } {
        b.extend(c);
    }
    let size = b.len() as u32 - 8;
    b[4..8].copy_from_slice(&size.to_be_bytes());
    b
}

#[test]
fn embedded_wav_aiff_ncw_share_translation_and_random_access_playback() {
    let root = std::env::temp_dir().join(format!("kontra-monolith-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let ncw = ncw::encode_pcm(
        &vec![16384; 4096],
        ncw::PcmSpec {
            channels: 1,
            bits_per_sample: 16,
            sample_rate: 48000,
        },
        ncw::StereoMode::Direct,
    )
    .unwrap();
    let fixtures = [
        ("tone.wav", wav()),
        ("tone.ncw", ncw),
        ("tone.aiff", aiff(None, 16, &16384i16.to_be_bytes(), true)),
        (
            "tone.aifc",
            aiff(Some(b"sowt"), 16, &16384i16.to_le_bytes(), false),
        ),
        (
            "float.aifc",
            aiff(Some(b"fl32"), 32, &0.5f32.to_be_bytes(), false),
        ),
        ("byte.aiff", aiff(None, 8, &[0x40], false)),
        ("wide.aiff", aiff(None, 24, &[0x40, 0, 0], false)),
    ];
    for (i, (name, audio)) in fixtures.iter().enumerate() {
        let patch = preset(name, false);
        let patch = if i % 2 == 0 {
            modern_preset(&patch)
        } else {
            patch
        };
        let path = root.join(format!("{i}.nki"));
        let container = file_container(&[
            ("Instrument.nki", &patch),
            (&format!("Samples|{name}"), audio),
        ]);
        std::fs::write(&path, container).unwrap();
        let mut k = read(&path).unwrap();
        assert_eq!(k.instrument.zones.len(), 1);
        let location = k.locations[0].clone();
        assert_eq!(k.samples.frames(&location).unwrap(), 4096);
        assert_eq!(
            k.samples.decode(&location).unwrap().frames,
            vec![[0.5, 0.5]; 4096]
        );
        let mut reader = SampleReader::open(&k.samples.source(&location).unwrap()).unwrap();
        let mut out = [[0.; 2]; 17];
        reader.read(2000, &mut out).unwrap();
        assert_eq!(out, [[0.5, 0.5]; 17]);
        let l = load(
            &path,
            &Options {
                scripts: false,
                ..Default::default()
            },
            |_| {},
        )
        .unwrap();
        assert_eq!(l.instrument.zones.len(), 1);
        let limits = sampler_core::Limits::for_plan(&l.plan, 4, 16);
        let mut runtime = sampler_core::Runtime::new(l.plan, limits).unwrap();
        runtime
            .trigger(
                sampler_core::Input {
                    protocol: sampler_core::Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 60,
                    external_id: None,
                },
                60,
                1.0,
            )
            .unwrap();
        let mut rendered = [[0.0; 2]; 64];
        runtime.render(&mut rendered).unwrap();
        assert!(rendered.iter().flatten().all(|v| v.is_finite()));
        assert!(rendered.iter().flatten().any(|v| v.abs() > 0.01));
        // Large sample-bearing files bypass the preset-only 128 MiB bound.
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(129 << 20)
            .unwrap();
        assert!(read_chunks(&path).is_ok());
    }
    let patch = preset("tone.wav", true);
    let audio = wav();
    let path = root.join("multi.nkm");
    std::fs::write(
        &path,
        file_container(&[("Multi.nkm", &patch), ("tone.wav", &audio)]),
    )
    .unwrap();
    let multi = read_multi(&path).unwrap();
    assert_eq!(multi.programs[0].0, 5);
    assert_eq!(read_program(&path, 0).unwrap().instrument.zones.len(), 1);
    assert!(read_program(&path, 1).is_err());
    for items in [
        vec![("missing.txt", b"x".as_slice())],
        vec![("a.nki", patch.as_slice()), ("b.nki", patch.as_slice())],
    ] {
        std::fs::write(&path, file_container(&items)).unwrap();
        assert!(read_chunks(&path).is_err());
    }
    let patch = preset("tone.wav", false);
    std::fs::write(
        &path,
        file_container(&[
            ("Instrument.nki", &patch),
            ("a/tone.wav", &audio),
            ("b/tone.wav", &audio),
        ]),
    )
    .unwrap();
    assert!(read(&path).is_err());
    for member in ["../tone.wav", "/tone.wav"] {
        std::fs::write(
            &path,
            file_container(&[("Instrument.nki", &patch), (member, &audio)]),
        )
        .unwrap();
        assert!(read(&path).is_err());
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn aiff_rejects_bad_lengths_offsets_rates_codecs_and_truncated_audio() {
    let audio = aiff(None, 16, &16384i16.to_be_bytes(), false);
    assert_eq!(decode(&audio).unwrap().frames[0], [0.5, 0.5]);
    for end in 0..audio.len() {
        assert!(decode(&audio[..end]).is_err(), "truncated at {end}");
    }
    for (at, bytes) in [(4, 0u32.to_be_bytes()), (28, [0xff; 4]), (46, [0xff; 4])] {
        let mut bad = audio.clone();
        bad[at..at + 4].copy_from_slice(&bytes);
        assert!(decode(&bad).is_err());
    }
    assert!(decode(&aiff(Some(b"junk"), 16, &[0, 0], false)).is_err());
    let path = std::env::temp_dir().join(format!("kontra-bad-aiff-{}", std::process::id()));
    let mut bad = audio.clone();
    bad[4..8].copy_from_slice(&(audio.len() as u32).to_be_bytes());
    std::fs::write(&path, &bad).unwrap();
    let source = sampler_kontakt::Samples::new(path.parent().unwrap())
        .source(&path)
        .unwrap();
    assert!(SampleReader::open(&source).is_err());
    std::fs::remove_file(path).unwrap();
}
