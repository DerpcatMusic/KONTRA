//! Discovery's parsers on fixture text, and the machine walk on folders
//! made for each test.
#![cfg(test)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use kontra_library::discover::{
    self, Hit, Limits, flatpak_hint, flatpak_id, group, hfs_path, merge, mount_points, paths_in,
    plist_dirs, reg_file, reg_query, service_center_xml, wine_path,
};
use kontra_library::{Progress, Root};

fn tree(name: &str, files: &[&str]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kontra-discover-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for path in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }
    dir
}

#[test]
fn reg_query_output_gives_content_dirs() {
    let text = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Native Instruments\\Abbey Road 50s Drummer\r\n    ContentDir    REG_SZ    D:\\Libraries\\Abbey Road 50s Drummer Library\\\r\n\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Native Instruments\\Kontakt 7\r\n    InstallDir    REG_SZ    C:\\Program Files\\Native Instruments\\Kontakt 7\\\r\n\r\nEnd of search: 1 match(es) found.\r\n";
    assert_eq!(
        reg_query(text),
        ["D:\\Libraries\\Abbey Road 50s Drummer Library\\"]
    );
}

#[test]
fn wine_reg_files_give_ni_content_dirs_unescaped() {
    let text = r#"WINE REGISTRY Version 2
;; All keys relative to \\Machine

[Software\\Native Instruments\\Analog Strings] 1700000000
#time=1da0000000000
"ContentDir"="Z:\\mnt\\hdd\\Kontakt\\Analog Strings\\"
"InstallDir"="C:\\Program Files\\Native Instruments\\"

[Software\\Other Vendor\\Thing] 1700000000
"ContentDir"="Z:\\nope\\"
"#;
    assert_eq!(reg_file(text), ["Z:\\mnt\\hdd\\Kontakt\\Analog Strings\\"]);
}

#[test]
fn service_center_xml_and_plists_give_content_dirs() {
    let xml = r#"<?xml version="1.0"?><ProductHints><Product version="1"><Name>Analog Strings</Name><ContentDir>C:\Libraries\Analog Strings &amp; Co\</ContentDir></Product></ProductHints>"#;
    assert_eq!(
        service_center_xml(xml),
        ["C:\\Libraries\\Analog Strings & Co\\"]
    );
    let plist = "<plist version=\"1.0\"><dict>\n\t<key>ContentDir</key>\n\t<string>Macintosh HD:Users:Shared:Analog Strings:</string>\n\t<key>InstallDir</key><string>/x</string></dict></plist>";
    assert_eq!(
        plist_dirs(plist),
        ["Macintosh HD:Users:Shared:Analog Strings:"]
    );
}

#[test]
fn hfs_and_wine_paths_map_to_host_paths() {
    assert_eq!(
        hfs_path("Macintosh HD:Users:Shared:Analog Strings:"),
        [
            PathBuf::from("/Volumes/Macintosh HD/Users/Shared/Analog Strings"),
            PathBuf::from("/Users/Shared/Analog Strings"),
        ]
    );
    assert_eq!(
        hfs_path("/Users/Shared/Lib"),
        [PathBuf::from("/Users/Shared/Lib")]
    );
    let prefix = Path::new("/home/u/.wine");
    assert_eq!(
        wine_path(prefix, "Z:\\mnt\\hdd\\Kontakt\\Analog Strings\\"),
        Path::new("/mnt/hdd/Kontakt/Analog Strings")
    );
    assert_eq!(
        wine_path(prefix, "C:\\Users\\Public\\Documents\\Native Instruments"),
        Path::new("/home/u/.wine/drive_c/Users/Public/Documents/Native Instruments")
    );
    assert_eq!(
        wine_path(prefix, "d:/Libs"),
        Path::new("/home/u/.wine/dosdevices/d:/Libs")
    );
    assert_eq!(wine_path(prefix, "Z:\\"), Path::new("/"));
}

#[test]
fn paths_are_scraped_from_pref_text() {
    let text =
        "<Path value=\"D:\\UVI\\Soundbanks\\\"/>\nlibrary=/mnt/hdd/UVI Soundbanks/\nname=Falcon";
    assert_eq!(
        paths_in(text),
        ["D:\\UVI\\Soundbanks", "/mnt/hdd/UVI Soundbanks"]
    );
}

#[test]
fn flatpak_info_names_the_app_and_the_hint_is_exact() {
    let info = "[Application]\nname=com.bitwig.BitwigStudio\nruntime=runtime/org.freedesktop.Platform/x86_64/23.08\n\n[Instance]\ninstance-id=1\n";
    assert_eq!(flatpak_id(info).as_deref(), Some("com.bitwig.BitwigStudio"));
    assert_eq!(flatpak_id("[Instance]\nname=x\n"), None);
    assert_eq!(
        flatpak_hint(Path::new("/mnt/hdd"), "fm.reaper.Reaper"),
        "Sandboxed (Flatpak): run  flatpak override --user --filesystem=/mnt/hdd fm.reaper.Reaper  then restart the DAW."
    );
}

#[test]
fn mounts_keep_disk_filesystems_only() {
    let mounts = "proc /proc proc rw 0 0\n/dev/nvme0n1p2 / btrfs rw 0 0\n/dev/sda1 /mnt/HDD\\040Samples ntfs3 rw 0 0\n/dev/sdb1 /run/media/u/USB exfat rw 0 0\n/dev/nvme0n1p1 /boot vfat rw 0 0\ntmpfs /tmp tmpfs rw 0 0\n";
    assert_eq!(
        mount_points(mounts),
        [
            PathBuf::from("/"),
            "/mnt/HDD Samples".into(),
            "/run/media/u/USB".into()
        ]
    );
}

/// The walk finds every kind of library by extension, stops at each, skips
/// junk and sample folders, and groups libraries into roots.
#[test]
fn the_machine_walk_finds_libraries_by_extension_and_groups_them() {
    let dir = tree(
        "walk",
        &[
            "hdd/Libraries/Kontakt/Abbey Road 50s Drummer Library/Abbey Road 50s Drummer.nicnt",
            "hdd/Libraries/Kontakt/Abbey Road 50s Drummer Library/Samples/x.ncw",
            "hdd/Libraries/Kontakt/Analog Strings/Instruments/Strings.nki",
            "hdd/Libraries/UVI/Falcon Factory.ufs",
            "hdd/Loose.sf2",
            "hdd/node_modules/pkg/Hidden/Instruments/X.nki",
            "hdd/.trash/Lib/Instruments/X.nki",
            "home/Downloads/Piano/Piano.sfz",
            "home/Music/Decent/Thing.dspreset",
            "home/Music/Mine/Saved.kontra",
            "home/Music/Sf3/Font.sf3",
        ],
    );
    let starts = [dir.join("hdd"), dir.join("home")];
    let progress = Progress::default();
    let hits = discover::walk(&starts, Limits::default(), &progress);
    let rel: Vec<_> = (hits.iter())
        .map(|h| (h.dir.strip_prefix(&dir).unwrap().to_owned(), h.standalone))
        .collect();
    assert_eq!(
        rel,
        [
            (PathBuf::from("hdd"), true),
            (
                "hdd/Libraries/Kontakt/Abbey Road 50s Drummer Library".into(),
                true
            ),
            ("hdd/Libraries/Kontakt/Analog Strings".into(), true),
            ("hdd/Libraries/UVI".into(), true),
            ("home/Downloads/Piano".into(), false),
            ("home/Music/Decent".into(), false),
            ("home/Music/Mine".into(), false),
            ("home/Music/Sf3".into(), true),
        ]
    );
    assert_eq!(progress.found.load(std::sync::atomic::Ordering::Relaxed), 8);
    // Loose files directly in a start (the stray `hdd/Loose.sf2`) are not a
    // root, or the whole drive would be one library. The UVI folder's root
    // covers the Kontakt folder.
    let roots = group(&hits, &starts);
    let rel: Vec<_> = (roots.iter())
        .map(|r| {
            (
                r.path.strip_prefix(&dir).unwrap().to_owned(),
                r.single,
                r.auto,
            )
        })
        .collect();
    assert_eq!(
        rel,
        [
            (PathBuf::from("hdd/Libraries"), false, true),
            ("home/Downloads".into(), false, true),
            ("home/Music".into(), false, true),
        ]
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_library_whose_folder_is_a_start_is_its_own_root() {
    let starts = [PathBuf::from("/home/u")];
    let hits = [
        Hit {
            dir: "/home/u/Piano".into(),
            standalone: false,
        },
        Hit {
            dir: "/home/u/Strings".into(),
            standalone: true,
        },
    ];
    let roots = group(&hits, &starts);
    assert_eq!(roots.len(), 2);
    assert!(roots[0].single && !roots[1].single);
}

#[test]
fn merging_keeps_user_roots_and_drops_hidden_or_covered_auto_roots() {
    let r = |p: &str| Root {
        path: p.into(),
        single: false,
        auto: false,
    };
    let user = [r("/mnt/hdd/Libraries")];
    let auto = [
        r("/mnt/hdd/Libraries/Kontakt"),
        r("/home/u/Music"),
        r("/home/u/Downloads"),
    ];
    let merged = merge(&user, &auto, &[PathBuf::from("/home/u/Downloads")]);
    let paths: Vec<_> = merged
        .iter()
        .map(|r| (r.path.to_str().unwrap(), r.auto))
        .collect();
    assert_eq!(
        paths,
        [("/mnt/hdd/Libraries", false), ("/home/u/Music", true)]
    );
}

#[test]
fn a_limited_walk_stops_at_its_depth() {
    let dir = tree(
        "limit",
        &["a/b/c/d/Lib/Instruments/X.nki", "a/Near/Instruments/Y.nki"],
    );
    let limits = Limits {
        depth: Some(2),
        ..Limits::default()
    };
    let hits = discover::walk(std::slice::from_ref(&dir), limits, &Progress::default());
    assert_eq!(hits.len(), 1);
    assert!(hits[0].dir.ends_with("a/Near"));
    std::fs::remove_dir_all(dir).unwrap();
}

/// The walk's speed on a synthetic tree of 20 000 folders, 400 of them
/// libraries; it prints folders per second (`--nocapture`).
#[test]
fn walk_speed_on_a_synthetic_tree() {
    let dir = std::env::temp_dir().join(format!("kontra-discover-speed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for vendor in 0..20 {
        for collection in 0..20 {
            let base = dir.join(format!("v{vendor}/c{collection}"));
            for junk in 0..48 {
                std::fs::create_dir_all(base.join(format!("misc/f{junk}"))).unwrap();
            }
            let lib = base.join("Lib/Instruments");
            std::fs::create_dir_all(&lib).unwrap();
            std::fs::write(lib.join("A.nki"), "").unwrap();
        }
    }
    let time = |threads_hint: &str| {
        let progress = Progress::default();
        let start = Instant::now();
        let hits = discover::walk(std::slice::from_ref(&dir), Limits::default(), &progress);
        let took = start.elapsed();
        let folders = progress.folders.load(std::sync::atomic::Ordering::Relaxed);
        println!(
            "walk ({threads_hint}, {} threads): {folders} folders, {} libraries in {took:?} = {:.0} folders/s",
            discover::threads(),
            hits.len(),
            folders as f64 / took.as_secs_f64()
        );
        (hits.len(), took)
    };
    let _warm = time("cold");
    let (found, took) = time("warm");
    assert_eq!(found, 400);
    assert!(took.as_secs() < 10, "20k folders took {took:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

/// UVI Workstation keeps the folders it searches for soundbanks in
/// `SoundBanksFolders` under `com.uvisoundsource`, written in two forms
/// (as found in real Wine prefixes); a Wine prefix's `.reg` file names them
/// as Windows paths, and each that exists is found.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn uvi_soundbank_folders_come_from_uvi_s_own_setting() {
    let prefix = tree(
        "uvi-prefix",
        &["system.reg", "Banks A/A.ufs", "Banks B/B.ufs"],
    );
    let z = |dir: &str| format!("Z:{}", prefix.join(dir).display()).replace('/', "\\\\");
    let reg = format!(
        "[Software\\\\com.uvisoundsource\\\\UVIWorkstation] 1794784652\n\
         \"SoundBanksFolders\"=\"<?xml version=\\\"1.0\\\"?>\\n<SearchPaths>\\n\\t<folder path=\\\"{a}\\\" recursive=\\\"false\\\" indexed=\\\"true\\\" />\\n\\t<folder path=\\\"C:\\\\users\\\\u\\\\Documents\\\\UVISoundBanks\\\" />\\n</SearchPaths>\\n\"\n\
         [Software\\\\com.uvisoundsource\\\\UVIWorkstation\\\\VST] 1794784652\n\
         \"SoundBanksFolders\"=\"<SearchPaths>\\n    <folder path=\\\"{b}\\\" recursive=\\\"0\\\"/>\\n</SearchPaths>\\n\"\n\
         [Software\\\\Other] 1\n\
         \"SoundBanksFolders\"=\"<folder path=\\\"Z:\\\\elsewhere\\\" />\"\n",
        a = z("Banks A"),
        b = z("Banks B").replace("\\\\", "/"),
    );
    std::fs::write(prefix.join("user.reg"), &reg).unwrap();
    assert_eq!(
        discover::uvi_reg_file(&reg),
        [
            z("Banks A").replace("\\\\", "\\"),
            "C:\\users\\u\\Documents\\UVISoundBanks".into(),
            z("Banks B").replace("\\\\", "/"),
        ]
    );
    let found = discover::in_prefix(&prefix);
    assert!(found.contains(&prefix.join("Banks A")), "{found:?}");
    assert!(found.contains(&prefix.join("Banks B")), "{found:?}");
    assert!(!found.iter().any(|p| p.ends_with("elsewhere")));
    let query = "\r\nHKEY_CURRENT_USER\\SOFTWARE\\com.uvisoundsource\\Falcon\r\n    SoundBanksFolders    REG_SZ    <SearchPaths>\n    <folder path=\"D:\\UVI &amp; Co\" recursive=\"0\"/>\n</SearchPaths>\n\r\n";
    assert_eq!(discover::uvi_soundbank_folders(query), ["D:\\UVI & Co"]);
    std::fs::remove_dir_all(&prefix).unwrap();
}
