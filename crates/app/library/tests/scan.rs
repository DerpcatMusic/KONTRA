//! The library scan on folders made for each test.
#![cfg(test)]

use std::path::{Path, PathBuf};

use kontra_library::{Cache, Folder, Format, Prefs, Progress, Root, clean_name, natural, scan};

/// A fresh folder holding `files` (path, contents).
fn tree(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kontra-library-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

fn programs(bank: &Path) -> Result<Vec<String>, String> {
    match std::fs::read_to_string(bank).unwrap().as_str() {
        "bad" => Err("bank: no header".into()),
        text => Ok(text.lines().map(String::from).collect()),
    }
}

fn run(roots: &[Root], cache: &mut Cache) -> kontra_library::Catalog {
    scan(roots, &Progress::default(), cache, &programs).unwrap()
}

fn root(dir: &Path) -> Root {
    Root {
        path: dir.to_path_buf(),
        single: false,
    }
}

const NICNT: &str = "\u{0}\u{1}<ProductHints><Product><Name>Areia</Name><Company>Audio Imperia</Company></Product></ProductHints>";
const NICNT_INNER: &str = "\u{0}\u{1}<ProductHints><Product><Name>Bundle Library</Name><Company>Audio Imperia</Company></Product></ProductHints>";

#[test]
fn libraries_are_found_named_and_shelved() {
    let mut files = vec![
        // Registered: the .nicnt names it.
        ("Areia 1.2.0 [AI]/Areia.nicnt", NICNT),
        ("Areia 1.2.0 [AI]/Instruments/2 Shorts/Spic.nki", ""),
        ("Areia 1.2.0 [AI]/Instruments/10 Longs/Sus.nki", ""),
        ("Areia 1.2.0 [AI]/Instruments/Legato.nki", ""),
        ("Areia 1.2.0 [AI]/Multis/All.nkm", ""),
        ("Areia 1.2.0 [AI]/Resources/pictures/logo.png", ""),
        ("Areia 1.2.0 [AI]/wallpaper.png", ""),
        ("Areia 1.2.0 [AI]/Samples/x.nki", ""),
        // Unregistered, under a vendor folder: presets beside samples.
        ("Vendor X/Piano_v1.2 [Kontakt]/Piano.nki", ""),
        ("Vendor X/Piano_v1.2 [Kontakt]/Samples/a.wav", ""),
        // A loose bank at the root is a library of its own.
        ("Winds.ufs", "Flute.uvip\nOboe/Oboe.uvip"),
        ("Winds.png", ""),
        ("Broken.ufs", "bad"),
        // A folder with no presets is no library.
        ("Docs/readme.txt", ""),
    ];
    // A folder of only samples is not searched.
    let wavs: Vec<String> = (0..70).map(|n| format!("Loops/Deep/{n}.wav")).collect();
    files.extend(wavs.iter().map(|w| (w.as_str(), "")));
    files.push(("Loops/Deep/Hidden/Hidden.nki", ""));
    let dir = tree("found", &files);
    let catalog = run(&[root(&dir)], &mut Cache::default());

    let names: Vec<_> = catalog
        .libraries
        .iter()
        .map(|l| (l.name.as_str(), l.vendor.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("Areia", "Audio Imperia"),
            ("Piano", "Vendor X"),
            ("Winds", "")
        ]
    );

    let areia = &catalog.libraries[0];
    assert!(areia.registered);
    assert_eq!(areia.format, Format::Kontakt);
    assert_eq!(areia.multis, 1);
    let inside: Vec<_> = areia
        .presets
        .iter()
        .map(|p| p.strip_prefix(&areia.dir).unwrap().to_str().unwrap())
        .collect();
    assert_eq!(
        inside,
        [
            "Instruments/2 Shorts/Spic.nki",
            "Instruments/10 Longs/Sus.nki",
            "Instruments/Legato.nki",
            "Multis/All.nkm"
        ]
    );
    let art: Vec<_> = areia
        .art
        .iter()
        .map(|p| p.file_name().unwrap().to_str().unwrap())
        .collect();
    assert_eq!(art, ["wallpaper.png", "Areia.nicnt", "logo.png"]);

    let tree = areia.tree();
    assert_eq!(tree.count, 4);
    let instruments = &tree.folders[0];
    assert_eq!(
        (
            instruments.name.as_str(),
            instruments.count,
            instruments.presets.len()
        ),
        ("Instruments", 3, 1)
    );
    let sub: Vec<_> = instruments
        .folders
        .iter()
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(sub, ["2 Shorts", "10 Longs"]);

    let winds = &catalog.libraries[2];
    assert_eq!(winds.format, Format::Uvi);
    assert_eq!(
        winds.presets,
        [
            dir.join("Winds.ufs/Flute.uvip"),
            dir.join("Winds.ufs/Oboe/Oboe.uvip")
        ]
    );
    assert_eq!(winds.art, [dir.join("Winds.png")]);
    assert_eq!(
        catalog.of(&winds.presets[1]).map(|l| l.name.as_str()),
        Some("Winds")
    );

    assert_eq!(catalog.issues.len(), 1);
    assert_eq!(catalog.issues[0].path, dir.join("Broken.ufs"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_single_root_is_one_library_and_names_are_told_apart() {
    let dir = tree(
        "single",
        &[
            ("A/Strings/Strings.nki", ""),
            ("B/Strings [Acme]/Strings.nki", ""),
            ("C/Strings/Strings.nki", ""),
        ],
    );
    let roots: Vec<Root> = ["A", "B", "C"]
        .iter()
        .map(|d| Root {
            path: dir.join(d),
            single: false,
        })
        .collect();
    let catalog = run(&roots, &mut Cache::default());
    let names: Vec<_> = catalog.libraries.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Strings", "Strings (Acme)", "Strings 2"]);

    let single = Root {
        path: dir.join("A/Strings"),
        single: true,
    };
    assert_eq!(
        run(&[single.clone(), single], &mut Cache::default())
            .libraries
            .len(),
        1
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn the_cache_reads_a_file_once_and_again_when_it_changes() {
    let dir = tree(
        "cache",
        &[
            ("Lib/Lib.nicnt", NICNT),
            ("Lib/Lib.nki", ""),
            ("Bank.ufs", "One.uvip"),
        ],
    );
    let file = dir.join("cache.json");
    let mut cache = Cache::default();
    run(&[root(&dir)], &mut cache);
    assert_eq!(cache.reads, 2);
    cache.save(&file).unwrap();

    let mut warm = Cache::load(&file);
    let catalog = run(&[root(&dir)], &mut warm);
    assert_eq!((warm.reads, catalog.libraries.len()), (0, 2));

    // A changed bank is read again; a removed one is forgotten.
    std::fs::write(dir.join("Bank.ufs"), "One.uvip\nTwo.uvip").unwrap();
    let catalog = run(&[root(&dir)], &mut warm);
    assert_eq!(warm.reads, 1);
    assert_eq!(
        catalog
            .libraries
            .iter()
            .map(|l| l.presets.len())
            .sum::<usize>(),
        3
    );
    std::fs::remove_file(dir.join("Bank.ufs")).unwrap();
    run(&[root(&dir)], &mut warm);
    warm.save(&file).unwrap();
    assert!(!std::fs::read_to_string(&file).unwrap().contains("Bank.ufs"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn names_lose_versions_brackets_and_packaging() {
    assert_eq!(
        clean_name("Areia 1.2.0 [Audio Imperia]"),
        ("Areia".into(), "Audio Imperia".into())
    );
    assert_eq!(
        clean_name("Shreddage 3.5 E-Upright 1.0.1 [Impact Soundworks] rutracker.org").0,
        "Shreddage E-Upright rutracker.org"
    );
    assert_eq!(clean_name("Una_Corda_Library_v1.1").0, "Una Corda");
    assert_eq!(clean_name("[Only]").0, "[Only]");
    assert!(natural("2 Legato") < natural("10 Shorts"));
    assert!(natural("Legato") < natural("2 Legato"));
    let tree = Folder::tree(Path::new("/x"), [Path::new("/y/outside.nki")]);
    assert_eq!((tree.count, tree.presets.as_slice()), (1, [0].as_slice()));
}

#[test]
fn prefs_keep_stars_and_recent_newest_first() {
    let mut prefs = Prefs::default();
    prefs.add_root(Root {
        path: "/libs".into(),
        single: false,
    });
    prefs.add_root(Root {
        path: "/libs".into(),
        single: true,
    });
    prefs.star(Path::new("/libs/Areia"), true);
    for n in 0..40 {
        prefs.loaded(Path::new(&format!("/libs/{n}.nki")));
    }
    prefs.loaded(Path::new("/libs/5.nki"));
    let back: Prefs = serde_json::from_str(&serde_json::to_string(&prefs).unwrap()).unwrap();
    assert_eq!(back, prefs);
    assert_eq!(back.roots.len(), 1);
    assert!(back.starred(Path::new("/libs/Areia")));
    assert_eq!(
        (
            back.recent.len(),
            back.recent[0].as_path(),
            back.recent[1].as_path()
        ),
        (32, Path::new("/libs/5.nki"), Path::new("/libs/39.nki"))
    );
    let old: Prefs = serde_json::from_str(r#"{"roots":[],"unknown":1}"#).unwrap();
    assert_eq!(old, Prefs::default());
}

/// A library inside a library is a library of its own. Action Strings 2
/// v1.2.0's folder is registered (its own `.nicnt`) and holds a second
/// registered product (`Action Strings 2 Library/Action Strings 2.nicnt`);
/// AudioBro's LASS bundle holds three. Stopping at the outer one listed the
/// inner ones' presets under it — one `Action Strings 2` with 448 presets
/// where Kontakt lists two libraries — so the outer one's walk now stops at
/// the inner one's folder, and each preset is listed under exactly one
/// library.
#[test]
fn a_library_inside_a_library_is_a_library_of_its_own() {
    let dir = tree(
        "nested",
        &[
            ("Bundle 1.2.0/Bundle.nicnt", NICNT),
            ("Bundle 1.2.0/Instruments/Outer.nki", ""),
            (
                "Bundle 1.2.0/Bundle Library/Bundle Library.nicnt",
                NICNT_INNER,
            ),
            ("Bundle 1.2.0/Bundle Library/Instruments/Inner.nki", ""),
            ("Bundle 1.2.0/Samples/a.wav", ""),
        ],
    );
    let catalog = run(&[root(&dir)], &mut Cache::default());
    let names: Vec<_> = catalog
        .libraries
        .iter()
        .map(|l| (l.name.as_str(), l.presets.len()))
        .collect();
    assert_eq!(names, [("Areia", 1), ("Bundle Library", 1)]);
    let listed: Vec<_> = catalog
        .libraries
        .iter()
        .flat_map(|l| l.presets.iter())
        .collect();
    assert_eq!(listed.len(), 2, "each preset under exactly one library");
    // The inner library is its own folder, not the outer one's.
    assert_eq!(
        catalog.libraries[1].dir,
        dir.join("Bundle 1.2.0/Bundle Library")
    );
    assert!(catalog.libraries[1].registered);
    std::fs::remove_dir_all(dir).unwrap();
}

/// Inside a library, a folder of presets is not a library: it is one of the
/// library's own instrument folders. Evolve R2's `Instruments/4 Tonality and
/// FX` holds presets beside a `Samples` folder, which is the *strong* test a
/// root-level folder is recognised by; read inside the library that way it
/// became a library called "Tonality and FX" and took 99 instruments out of
/// Evolve R2. Only a `.nicnt` makes a folder inside a library a library.
#[test]
fn a_preset_folder_inside_a_library_stays_inside_it() {
    let dir = tree(
        "inside",
        &[
            ("Evolve/Evolve.nicnt", NICNT),
            ("Evolve/Instruments/1 Rhythmic/One.nki", ""),
            ("Evolve/Instruments/4 Tonality and FX/Two.nki", ""),
            ("Evolve/Instruments/4 Tonality and FX/Samples/x.wav", ""),
            ("Evolve/Samples/a.wav", ""),
        ],
    );
    let catalog = run(&[root(&dir)], &mut Cache::default());
    assert_eq!(catalog.libraries.len(), 1, "{:?}", catalog.libraries);
    let evolve = &catalog.libraries[0];
    let inside: Vec<_> = evolve
        .presets
        .iter()
        .map(|p| p.strip_prefix(&evolve.dir).unwrap().to_str().unwrap())
        .collect();
    assert_eq!(
        inside,
        [
            "Instruments/1 Rhythmic/One.nki",
            "Instruments/4 Tonality and FX/Two.nki"
        ]
    );
    std::fs::remove_dir_all(dir).unwrap();
}
