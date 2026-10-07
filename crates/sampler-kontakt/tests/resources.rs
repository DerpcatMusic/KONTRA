use sampler_kontakt::ResourceContainer;

#[cfg(feature = "library-access")]
fn authored_png() -> Vec<u8> {
    // A black 1x1 RGBA pixel, using a stored DEFLATE block and Adler checksum.
    let ihdr = [0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0];
    let idat = [0x78, 1, 1, 5, 0, 0xfa, 0xff, 0, 0, 0, 0, 0, 0, 5, 0, 1];
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, data) in [(b"IHDR", ihdr.as_slice()), (b"IDAT", &idat), (b"IEND", &[])] {
        bytes.extend((data.len() as u32).to_be_bytes());
        bytes.extend(kind);
        bytes.extend(data);
        let mut crc = u32::MAX;
        for byte in kind.iter().chain(data) {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb88320 * (crc & 1));
            }
        }
        bytes.extend((!crc).to_be_bytes());
    }
    bytes
}

#[cfg(feature = "library-access")]
#[test]
fn authored_resource_containers_are_bounded_and_case_insensitive() {
    let root = std::env::temp_dir().join(format!("v2-resource-containers-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let png = authored_png();
    let picture = png.as_slice();
    let name = "Resources|pictures|Wallpaper.png";
    let mut container = b"/\\ NI FC MTD  /\\".to_vec();
    container.extend([0; 256]);
    container.extend(1u64.to_le_bytes());
    container.extend((picture.len() as u64).to_le_bytes());
    container.extend(b"/\\ NI FC TOC  /\\");
    container.extend([0; 600]);
    container.extend(0u64.to_le_bytes());
    container.extend([0; 16]);
    let mut filename = Vec::new();
    for word in name.encode_utf16() {
        filename.extend(word.to_le_bytes());
    }
    filename.resize(600, 0);
    container.extend(filename);
    container.extend(0u64.to_le_bytes());
    container.extend((picture.len() as u64).to_le_bytes());
    container.extend([0xf1; 8]);
    container.extend([0; 16]);
    container.extend(b"/\\ NI FC TOC  /\\");
    container.extend([0; 592]);
    container.extend(picture);
    let mut nicnt = b"/\\ NI FC MTD  /\\".to_vec();
    nicnt.extend([0; 300]);
    nicnt.extend(&container);
    let path = root.join("authored.nicnt");
    std::fs::write(&path, &nicnt).unwrap();
    let mut resources = ResourceContainer::open(&path).unwrap();
    assert_eq!(resources.names(), [name]);
    assert_eq!(
        resources
            .read("resources/pictures/wallpaper.PNG")
            .unwrap()
            .unwrap(),
        picture
    );
    assert!(resources.read("missing.png").unwrap().is_none());
    let instrument = root.join("Instruments/Piano.nki");
    std::fs::create_dir_all(instrument.parent().unwrap()).unwrap();
    let mut routed = sampler_kontakt::Resources::of(&instrument);
    assert_eq!(
        routed.read("Resources\\pictures\\Wallpaper.PNG").as_deref(),
        Some(picture.as_slice())
    );
    assert!(
        routed.locations().contains(&path),
        "opened containers retain their diagnostic paths"
    );
    let loose = root.join("Resources/pictures/wallpaper.png");
    std::fs::create_dir_all(loose.parent().unwrap()).unwrap();
    std::fs::write(&loose, b"loose override").unwrap();
    let mut routed = sampler_kontakt::Resources::of(&instrument);
    assert_eq!(
        routed.read("resources|pictures|wallpaper.PNG").as_deref(),
        Some(b"loose override".as_slice())
    );

    // Invalid markers, exaggerated counts/ranges and truncated bodies are errors.
    for at in [0, 272, 632 + 904, 1544] {
        let mut corrupt = container.clone();
        corrupt[at] ^= 0xff;
        assert!(
            ni_file::file_container::NIFileContainer::read(std::io::Cursor::new(corrupt)).is_err()
        );
    }
    container.pop();
    assert!(
        ni_file::file_container::NIFileContainer::read(std::io::Cursor::new(container)).is_err()
    );
    for (name, picture) in [
        ("Wallpaper.png", picture),
        (
            "Wallpaper.txt",
            b"Has Alpha Channel: yes\nNumber of Animations: 1\nFixed Top: 0\nHorizontal Animation: no\n".as_slice(),
        ),
    ] {
        for (version, hint) in [
            (0x110u16, 0xffu32),
            (0x111, 0xff),
            (0x110, 0x100),
            (0x111, 0x100),
        ] {
            let mut nkr = 0x5e70ac54u32.to_le_bytes().to_vec();
            nkr.extend(version.to_le_bytes());
            nkr.extend([0; 8]);
            nkr.extend(1u32.to_le_bytes());
            nkr.extend([0; 4]);
            let entry_len = 8 + (name.len() + 1) * 2;
            nkr.extend((entry_len as u16).to_le_bytes());
            nkr.extend((22 + entry_len as u32).to_le_bytes());
            nkr.extend(0u16.to_le_bytes());
            for word in name.encode_utf16().chain([0]) {
                nkr.extend(word.to_le_bytes());
            }
            nkr.extend(0x2ae905fau32.to_le_bytes());
            nkr.extend(version.to_le_bytes());
            nkr.extend([0; 4]);
            nkr.extend(hint.to_le_bytes());
            nkr.extend((picture.len() as u32).to_le_bytes());
            nkr.extend([0; 4]);
            nkr.extend(picture);
            let path = root.join(format!("authored-{version}.nkr"));
            std::fs::write(&path, &nkr).unwrap();
            let mut resources = ResourceContainer::open(&path).unwrap();
            assert_eq!(resources.read(&name.to_uppercase()).unwrap().unwrap(), picture);
            if hint == 0x100 {
                // Invalid framing/CRCs or UTF-8 cannot override protection.
                *nkr.last_mut().unwrap() ^= if name.ends_with(".txt") { 0x80 } else { 1 };
                std::fs::write(&path, &nkr).unwrap();
                assert!(ResourceContainer::open(&path).unwrap().read(name).is_err());
            }
        }
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "library-access")]
#[test]
fn installed_afflatus_wallpaper_is_a_memory_buffer() {
    let path = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").and_then(|roots| {
        std::env::split_paths(&roots)
            .map(|root| root.join("Afflatus Chapter II Brass/Afflatus Chapter II Brass.nicnt"))
            .find(|path| path.is_file())
    });
    let Some(path) = path else {
        eprintln!("skipped: installed Afflatus NICNT is absent");
        return;
    };
    let mut resources = ResourceContainer::open(&path).unwrap();
    let picture = resources.read(".LibBrowser.png").unwrap().unwrap();
    assert!(picture.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(picture.ends_with(b"IEND\xaeB`\x82"));
    assert!(!resources.names().is_empty());
}

#[cfg(not(feature = "library-access"))]
#[test]
fn resource_access_has_a_disabled_stand_in() {
    assert!(ResourceContainer::open(std::path::Path::new("absent.nicnt")).is_err());
}

#[cfg(feature = "library-access")]
#[test]
fn installed_encrypted_nkr_picture_is_a_memory_buffer() {
    let path = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").and_then(|roots| {
        std::env::split_paths(&roots)
            .map(|root| root.join("Solo/Samples/Pyramid v1.0.6.nkr"))
            .find(|path| path.is_file())
    });
    let Some(path) = path else {
        eprintln!("skipped: installed Solo NKR is absent");
        return;
    };
    let mut resources = ResourceContainer::open(&path).unwrap();
    let name = resources
        .names()
        .into_iter()
        .find(|name| name.to_ascii_lowercase().ends_with(".png"))
        .unwrap()
        .to_owned();
    let picture = resources.read(&name).unwrap().unwrap();
    assert!(picture.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(picture.ends_with(b"IEND\xaeB`\x82"));
    let mut file = std::fs::File::open(&path).unwrap();
    let archive = ni_file::nkr::Archive::read_index(&mut file).unwrap();
    assert!(archive.read_entry(&mut file, &name).is_err());
}
