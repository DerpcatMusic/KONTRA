//! Real-file checks on the installed Modo Drum packs. Skipped when they are not installed.

use sampler_ikmpak::{Mic, Pak, parse_member};
use std::collections::HashSet;
use std::fs::File;
use std::path::{Path, PathBuf};

const DIR: &str = "/data/main/Libraries/Modo Drum";

fn packs() -> Option<Vec<(PathBuf, usize)>> {
    let dir = Path::new(DIR);
    if !dir.is_dir() {
        eprintln!("skipping: {DIR} not installed");
        return None;
    }
    Some(vec![
        (dir.join("MODO DRUM Brit Custom.pak"), 1450),
        (dir.join("MODO DRUM Metal.pak"), 1510),
        (dir.join("MODO DRUM Silver.pak"), 1774),
    ])
}

#[test]
fn every_member_parses_and_is_plain_wav() {
    let Some(packs) = packs() else { return };
    for (path, count) in packs {
        let mut pak = Pak::open(File::open(&path).unwrap()).unwrap();
        assert_eq!(pak.entries().len(), count, "{}", path.display());
        let mut keys = HashSet::new();
        let mut dry = 0;
        for (i, e) in pak.entries().to_vec().iter().enumerate() {
            let m = parse_member(&e.path).unwrap_or_else(|| panic!("no grammar match: {}", e.path));
            assert!(
                keys.insert((m.folders, m.note, m.velocity, m.round_robin, m.mic)),
                "duplicate key: {}",
                e.path
            );
            dry += (m.mic == Mic::Dry) as usize;
            let f = pak.wav_format(i).unwrap();
            // Mono and stereo, 48 or 96 kHz, 24-bit PCM. The mix is listed in docs/re/formats/ikmpak.md.
            assert_eq!(f.format_tag, 1, "{}", e.path);
            assert!(
                (1..=2).contains(&f.channels) && [48000, 96000].contains(&f.sample_rate),
                "{}",
                e.path
            );
            assert_eq!(f.bits_per_sample, 24, "{}", e.path);
            assert!(f.data_frames > 0, "{}", e.path);
        }
        assert!(dry > 0);
    }
}

#[test]
fn smallest_payload_is_whole_riff_file() {
    let Some(packs) = packs() else { return };
    let (path, _) = &packs[0];
    let mut pak = Pak::open(File::open(path).unwrap()).unwrap();
    let (i, e) = pak
        .entries()
        .iter()
        .enumerate()
        .min_by_key(|(_, e)| e.size)
        .map(|(i, e)| (i, e.clone()))
        .unwrap();
    let bytes = pak.payload(i).unwrap();
    assert_eq!(bytes.len() as u64, e.size);
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(
        u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as u64 + 8,
        e.size
    );
}
