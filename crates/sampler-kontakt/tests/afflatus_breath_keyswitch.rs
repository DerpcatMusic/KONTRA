//! Afflatus 2 Horns Repetitions: the breath key (91) is a keyswitch. Its
//! `on note` branch disallows the short groups and plays the breath note
//! itself, so a press starts different groups than a plain note does.
//! Skips when the library is not installed.
use sampler_core::{Input, Protocol, Runtime};
use std::{collections::BTreeSet, path::PathBuf};

const NKI: &str = "Afflatus Chapter II Brass/Instruments/1. Ensembles/Single Instruments/2 Horns/2 Horns Repetitions.nki";
const BREATH_KEY: u8 = 91;
const PLAIN_KEY: u8 = 60;

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
    Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key, external_id: None }
}

/// Groups that start a source for one press of `key`.
fn started_groups(path: &std::path::Path, key: u8) -> BTreeSet<u32> {
    let options = sampler_kontakt::Options { rate: 48000, keys: 0..=127, ..Default::default() };
    let loaded = sampler_kontakt::load(path, &options, |_| {}).unwrap();
    let limits = sampler_core::Limits::for_plan(&loaded.plan, 16, 256);
    let mut rt = Runtime::new(loaded.plan, limits).unwrap();
    rt.record_selections(true);
    rt.trigger(input(key), key, 0.8).unwrap();
    let mut out = vec![[0.0f32; 2]; 24000];
    for block in out.chunks_mut(64) {
        rt.render(block).unwrap();
    }
    rt.take_selection_records()
        .iter()
        .flat_map(|r| r.candidates.iter())
        .filter(|c| c.started.is_some())
        .filter_map(|c| c.group)
        .collect()
}

#[test]
fn breath_keyswitch_starts_the_breath_groups_not_the_plain_ones() {
    let Some(path) = find() else {
        eprintln!("skipped: {NKI} is not installed");
        return;
    };
    let plain = started_groups(&path, PLAIN_KEY);
    let breath = started_groups(&path, BREATH_KEY);
    assert!(!plain.is_empty(), "a plain note must start a source");
    assert!(!breath.is_empty(), "the breath key must start a source");
    assert!(
        plain.is_disjoint(&breath),
        "breath groups {breath:?} must not overlap plain groups {plain:?}"
    );
}
