//! A trill script plays only while a neighbour key is held: `on note` reads
//! `%KEY_DOWN` and calls `play_note(key, interval)`, passing the interval
//! index as the velocity. Skips when the library is not installed.
use sampler_core::{Input, Protocol, Runtime};
use std::path::{Path, PathBuf};

const NKI: &str = "Pacific Ensemble Strings/Instruments/16 Violins/Pacific - Ens Strings - 16 Violins - Trills.nki";

fn find() -> Option<PathBuf> {
    let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES")
        .map(|paths| std::env::split_paths(&paths).collect())
        .unwrap_or_else(|| vec![PathBuf::from("/data/main/Libraries/Kontakt")]);
    roots.iter().find_map(|root| {
        [root.join(NKI), root.join("Kontakt").join(NKI)]
            .into_iter()
            .find(|p| p.is_file())
    })
}

fn input(key: u8) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: None,
    }
}

/// Peak of 0.5 s with `held` (if any) pressed first, then `key`, and the
/// selection records of those attacks.
fn render(path: &Path, held: Option<u8>, key: u8) -> (f32, Vec<sampler_core::SelectionRecord>) {
    let options = sampler_kontakt::Options {
        rate: 48000,
        keys: 0..=127,
        ..Default::default()
    };
    let loaded = sampler_kontakt::load(path, &options, |_| {}).unwrap();
    let limits = sampler_core::Limits::for_plan(&loaded.plan, 16, 256);
    let mut rt = Runtime::new(loaded.plan, limits).unwrap();
    rt.record_selections(true);
    if let Some(held) = held {
        rt.trigger(input(held), held, 0.8).unwrap();
    }
    rt.trigger(input(key), key, 0.8).unwrap();
    let mut out = vec![[0.0f32; 2]; 24000];
    for block in out.chunks_mut(64) {
        rt.render(block).unwrap();
    }
    let peak = out.iter().flatten().fold(0f32, |m, x| m.max(x.abs()));
    (peak, rt.take_selection_records())
}

#[test]
fn held_neighbour_turns_a_trill_into_play_note() {
    let Some(path) = find() else {
        eprintln!("skipped: {NKI} is not installed");
        return;
    };
    let key = 60;

    // Alone, the script ignores the key: no play_note child, no sound.
    let (peak, records) = render(&path, None, key);
    assert_eq!(peak, 0.0);
    assert!(records.iter().all(|r| r.parent_event.is_none()));

    // With the key above held, the script plays the trill note itself via
    // play_note(60, 1): velocity 1 (the interval), and a zone starts.
    let (peak, records) = render(&path, Some(key + 1), key);
    let child = records
        .iter()
        .find(|r| r.parent_event.is_some() && r.key == key)
        .expect("play_note creates a child event for the trill key");
    assert!(
        (child.velocity - 1.0 / 127.0).abs() < 1e-9,
        "velocity {}",
        child.velocity
    );
    assert!(
        child.candidates.iter().any(|c| c.started.is_some()),
        "no region started"
    );
    assert!(peak > 1e-4, "peak {peak}");
}
