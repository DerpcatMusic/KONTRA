//! A face's pictures: the one place a tree asset path becomes bytes, and the
//! reason it could not, so a page drawn without pictures says why.
#![cfg(test)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use kontra_host::ui::{Library, Loader, Missing, found, open_any};

fn instrument() -> Option<PathBuf> {
    const LIBRARY: &str = "Armory/VSTs/Kontakt1/Native Instruments Session Guitarist Electric Mint 1.1.0/Session Guitarist - Electric Mint Library/Instruments";
    let path = std::env::var_os("KONTRA_EMINT").map_or_else(
        || {
            std::env::var_os("HOME")
                .map_or_else(|| PathBuf::from("/home"), PathBuf::from)
                .join(LIBRARY)
                .join("Electric Mint.nki")
        },
        PathBuf::from,
    );
    if !path.exists() {
        eprintln!("skipped: no {}", path.display());
        return None;
    }
    Some(path)
}

/// Everything a face's reader was told it could not read.
type Heard = Arc<Mutex<Vec<(Missing, String)>>>;

/// A reader over `paths` that keeps what it was told.
fn loader(paths: [Option<PathBuf>; 2]) -> (Loader, Heard) {
    let heard = Heard::default();
    let sink = {
        let heard = Arc::clone(&heard);
        Box::new(move |missing: Missing, asset: &str| {
            heard
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((missing, asset.to_owned()));
        })
    };
    (Loader::new(paths, sink), heard)
}

/// A part with no file has no library, and every picture says so rather than
/// quietly returning nothing.
#[test]
fn a_face_with_no_instrument_paths_says_so_for_every_picture() {
    let (loader, heard) = loader([None, None]);
    for asset in ["Resources/pictures/bg-images.png", "pictures/knob.png"] {
        assert!(loader.read(asset).is_none());
    }
    let heard = heard.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(heard.len(), 2, "one reason per picture: {heard:?}");
    assert!(
        heard.iter().all(|(missing, _)| *missing == Missing::NoPath),
        "{heard:?}"
    );
    assert_eq!(heard[1].1, "pictures/knob.png", "the asset path is said");
}

/// An instrument outside any library says that, rather than "not found".
#[test]
fn an_instrument_outside_its_library_says_that() {
    let (loader, heard) = loader([
        Some(std::env::temp_dir().join("kontra-nowhere/Some.nki")),
        None,
    ]);
    assert!(loader.read("Resources/pictures/knob.png").is_none());
    let heard = heard.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0].0, Missing::NoLibrary);
}

/// The library of the instrument the part came from is used when the part's
/// own file has none: a preset kept outside its library still draws.
#[test]
fn a_preset_outside_its_library_reads_the_pictures_of_its_instrument() {
    let Some(path) = instrument() else {
        return;
    };
    let (loader, heard) = loader([
        Some(std::env::temp_dir().join("kontra-nowhere/Electric Mint.nki")),
        Some(path.clone()),
    ]);
    let bytes = loader
        .read("Resources/pictures/bg-images.png")
        .expect("the wallpaper reads from the instrument's library");
    assert!(bytes.starts_with(b"\x89PNG"), "{} bytes", bytes.len());
    // A picture the library does not have is reported as such.
    assert!(
        loader
            .read("Resources/pictures/no-such-picture.png")
            .is_none()
    );
    let heard = heard.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(
        heard.iter().map(|(m, _)| *m).collect::<Vec<_>>(),
        vec![Missing::NoFile]
    );
}

/// `Library::open_any` takes the first path that has resources.
#[test]
fn the_first_path_with_resources_is_the_library() {
    let Some(path) = instrument() else {
        return;
    };
    let missing = std::env::temp_dir().join("kontra-nowhere/Some.nki");
    let library = open_any([None, Some(missing.clone())]);
    assert!(library.is_none(), "{missing:?} has no library");
    let library =
        open_any([Some(missing), None, Some(path.clone())]).expect("the instrument's library");
    assert!(found(&library));
    let mut library = library;
    assert!(
        library.read("Resources/pictures/bg-images.png").is_some(),
        "{path:?}"
    );
    assert!(found(&Library::open(&path)));
}
