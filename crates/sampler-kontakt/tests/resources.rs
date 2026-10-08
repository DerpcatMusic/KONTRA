use sampler_kontakt::ResourceContainer;

#[cfg(feature = "library-access")]
#[test]
fn authored_resource_containers_are_bounded_and_case_insensitive() {
    let root = std::env::temp_dir().join(format!("v2-resource-containers-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let picture = b"\x89PNG\r\n\x1a\nauthored picture bytes";
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
    for version in [0x110u16, 0x111] {
        let name = "Wallpaper.png";
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
        nkr.extend(0xffu32.to_le_bytes());
        nkr.extend((picture.len() as u32).to_le_bytes());
        nkr.extend([0; 4]);
        nkr.extend(picture);
        let path = root.join(format!("authored-{version}.nkr"));
        std::fs::write(&path, nkr).unwrap();
        let mut resources = ResourceContainer::open(&path).unwrap();
        assert_eq!(resources.read("wallpaper.PNG").unwrap().unwrap(), picture);
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
fn declared_container_relocates_within_library_and_missing_preserves_loose_resources() {
    fn archive(name: &str, bytes: &[u8]) -> Vec<u8> {
        let version = 0x110u16;
        let mut nkr = 0x5e70ac54u32.to_le_bytes().to_vec();
        nkr.extend(version.to_le_bytes());
        nkr.extend([0; 8]);
        nkr.extend(1u32.to_le_bytes());
        nkr.extend([0; 4]);
        let entry_len = 8 + (name.len() + 1) * 2;
        nkr.extend((entry_len as u16).to_le_bytes());
        nkr.extend((22 + entry_len as u32).to_le_bytes());
        nkr.extend(0u16.to_le_bytes());
        for word in name.encode_utf16().chain([0]) { nkr.extend(word.to_le_bytes()); }
        nkr.extend(0x2ae905fau32.to_le_bytes());
        nkr.extend(version.to_le_bytes());
        nkr.extend([0; 4]);
        nkr.extend(0xffu32.to_le_bytes());
        nkr.extend((bytes.len() as u32).to_le_bytes());
        nkr.extend([0; 4]);
        nkr.extend(bytes);
        nkr
    }
    let root = std::env::temp_dir().join(format!("program-resource-authority-{}", std::process::id()));
    std::fs::create_dir_all(root.join("Instruments")).unwrap();
    std::fs::create_dir_all(root.join("Samples")).unwrap();
    std::fs::create_dir_all(root.join("Resources/scripts")).unwrap();
    let key = "linked.txt";
    std::fs::write(root.join("a-foreign.nkr"), archive(key, b"foreign")).unwrap();
    std::fs::write(root.join("z-own.nkr"), archive(key, b"own")).unwrap();
    std::fs::write(root.join("Resources/scripts/loose.txt"), b"loose").unwrap();
    let path = root.join("Instruments/rack.nkm");
    let mut own = sampler_kontakt::Resources::of_declared(&path, Some("Z-OWN.NKR"));
    assert_eq!(own.read_result(key).unwrap(), Some(b"own".to_vec()));
    let mut relocated = sampler_kontakt::Resources::of_declared(&path, Some(r"C:\Author\Obsolete.nkr"));
    assert!(relocated.container_status().is_ok());
    assert_eq!(relocated.read_result(key).unwrap(), Some(b"foreign".to_vec()));
    let sibling = root.join("Kontakt/EmptyLibrary");
    std::fs::create_dir_all(sibling.join("Samples")).unwrap();
    std::fs::create_dir_all(sibling.join("Resources/scripts")).unwrap();
    std::fs::write(sibling.join("Resources/scripts/loose.txt"), b"loose").unwrap();
    let mut missing = sampler_kontakt::Resources::of_declared(&sibling.join("rack.nkm"), Some("absent.nkr"));
    let error = missing.read_result(key).unwrap_err();
    assert_eq!(error.kind(), sampler_kontakt::Kind::ResourceContainerMissing);
    assert_eq!(error.stage(), Some(sampler_kontakt::Stage::ResourceResolve));
    assert!(missing.script("linked.txt").is_none());
    assert_eq!(missing.read_result("Resources/scripts/loose.txt").unwrap(), Some(b"loose".to_vec()));
    assert!(missing.names("Resources/native_ui/").is_empty());
    std::fs::write(root.join("broken.nkr"), b"broken").unwrap();
    let mut broken = sampler_kontakt::Resources::of_declared(&path, Some("broken.nkr"));
    assert_eq!(broken.read_result(key).unwrap_err().kind(), sampler_kontakt::Kind::Invalid);
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(feature = "library-access")]
#[test]
#[ignore = "requires locally owned Native corpus; only fixed categories and counts leave RAM"]
fn native_corpus_declared_containers() {
    let list = std::fs::read_to_string(std::env::var_os("KONTRA_RESOURCE_SURVEY_LIST").unwrap()).unwrap();
    let inventory = std::process::Command::new("rg").args(["--files", "/mnt/MAIN_STORAGE/Libraries", "--iglob", "*.nkr", "--iglob", "*.nicnt"]).output().unwrap();
    assert!(inventory.status.success());
    let inventory = String::from_utf8(inventory.stdout).unwrap();
    for (index, line) in list.lines().enumerate() {
        let (_, path) = line.split_once('\t').unwrap();
        if !path.ends_with(".nkm") { continue; }
        let path = std::path::Path::new(path);
        let chunks = sampler_kontakt::read_chunks(path).unwrap_or_else(|_| panic!("container parse failed"));
        let files = chunks.filename_tables().unwrap().unwrap().other_filetable;
        let multi = sampler_kontakt::read_multi(path).unwrap_or_else(|_| panic!("multi parse failed"));
        let record = multi.programs[0].1.public_record().unwrap();
        let declared = record.resource_container_filename_ref.filter(|&n| n>=0).and_then(|n| files.get(&(n as u32))).unwrap();
        let normalized = declared.replace(['\\','|'], "/");
        let basename = normalized.rsplit('/').next().unwrap();
        let global_matches = inventory.lines().filter(|p| p.rsplit('/').next().is_some_and(|n| n.eq_ignore_ascii_case(basename))).count();
        let source = sampler_kontakt::Resources::of_declared(path, Some(declared));
        let category = match source.container_status() {
            Ok(()) => "resolved-own-container",
            Err(e) if e.kind()==sampler_kontakt::Kind::ResourceContainerMissing => "resource_container_missing",
            Err(_) => "other",
        };
        println!("RESOURCE_AUTHORITY item_index={index} program=0 category={category} global_basename_matches={global_matches}");
    }
}
