//! Loading an SFZ through the same rack and progress path as other instruments.

use std::sync::Arc;

use kontra_host::rack::{Rack, Store, StoreConfig};
use kontra_protocol::{LoadProgress, LoadStage};

#[test]
fn sfz_loads_a_sample_and_reports_its_preloaded_bytes() {
    let root = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("sfz-load-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let frames = 4096u32;
    let bytes = frames * 2;
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&48_000u32.to_le_bytes());
    wav.extend_from_slice(&96_000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&bytes.to_le_bytes());
    wav.extend(std::iter::repeat_n(0u8, bytes as usize));
    std::fs::write(root.join("sample.wav"), wav).unwrap();
    let path = root.join("instrument.sfz");
    std::fs::write(&path, "<region> sample=sample.wav key=60\n").unwrap();
    let progress = Arc::new(LoadProgress::default());
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    store.set_progress(Arc::clone(&progress));
    let (mut rack, _audio) = Rack::new(48_000, store, 512);
    rack.load(0, &path).unwrap();
    let part = rack.parts().first().and_then(Option::as_ref).unwrap();
    assert_eq!(part.name, "instrument");
    assert_eq!(part.plan.as_ref().unwrap().zones.len(), 1);
    let snapshot = progress.snapshot();
    assert_eq!(snapshot.stage, LoadStage::Interface);
    assert_eq!(snapshot.bytes, u64::from(bytes));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn uvi_loads_use_the_same_progress_path() {
    let path = std::path::Path::new(
        "/data/main/Libraries/UVI/UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs/Presets/00 Orchestra/01 Strings/V Strings Bartok.uvip",
    );
    if !path.ancestors().any(std::path::Path::is_file) {
        eprintln!("skipped: UVI bank not installed");
        return;
    }
    let progress = Arc::new(LoadProgress::default());
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    store.set_progress(Arc::clone(&progress));
    let (mut rack, _audio) = Rack::new(48_000, store, 512);
    rack.load(0, path).unwrap();
    let snapshot = progress.snapshot();
    assert_eq!(snapshot.stage, LoadStage::Interface);
    assert!(snapshot.bytes > 0, "the program preloads its samples");
}
