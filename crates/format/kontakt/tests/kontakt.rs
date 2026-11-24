//! Real instruments, skipped where the library is not installed.
#![cfg(test)]

use std::path::Path;

const AREIA: &str = "/data/main/Libraries/Kontakt/Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/01 Areia - 16 Violins - Core Techniques.nki";

#[test]
fn areia_violins_load_with_their_samples_and_script() {
    let path = Path::new(AREIA);
    if !path.exists() {
        return;
    }
    let i = kontra_format_kontakt::load(path).unwrap();
    let zones: usize = i.groups.iter().map(|g| g.zones.len()).sum();
    eprintln!(
        "{}: {} groups, {} zones, {} samples, {} scripts, stream {:?}",
        i.name,
        i.groups.len(),
        zones,
        i.samples.len(),
        i.scripts.len(),
        i.groups.first().and_then(|g| g.stream)
    );
    for d in &i.diagnostics {
        eprintln!("  {:?} {}: {}", d.impact, d.feature, d.detail);
    }
    if let Some(s) = i.samples.first() {
        eprintln!("  first sample {s:?}");
    }
    assert!(zones > 0 && !i.samples.is_empty() && !i.scripts.is_empty());
    assert!(!i.diagnostics.iter().any(|d| d.feature == "missing sample"));
}

const ANALOG: &str =
    "/data/main/Libraries/Kontakt/ANALOG STRINGS/Snapshots/ANALOG STRINGS/Factory/9001 Punch.nksn";

#[test]
fn an_analog_strings_snapshot_is_its_instrument_with_its_saved_values() {
    let path = Path::new(ANALOG);
    if !path.exists() {
        return;
    }
    let snap = kontra_format_kontakt::load(path).unwrap();
    let base = Path::new("/data/main/Libraries/Kontakt/ANALOG STRINGS/Instruments");
    let nki = std::fs::read_dir(base).unwrap().flatten().map(|e| e.path());
    let nki = nki
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e == "nki"))
        .unwrap();
    let base = kontra_format_kontakt::load(&nki).unwrap();
    assert_eq!(snap.name, "9001 Punch");
    assert_eq!(snap.groups.len(), base.groups.len());
    let saved = |i: &kontra_ir::Instrument| {
        i.scripts
            .iter()
            .map(|s| s.saved.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(snap.scripts.len(), base.scripts.len());
    assert_ne!(saved(&snap), saved(&base));
    // B P0-9: ANALOG STRINGS saves fx_idx_amp_split_point = 6; carried into IR, not ignored.
    assert!(
        base.groups.iter().any(|g| g.amp_split == 6),
        "expected ANALOG STRINGS groups to carry amp_split = 6"
    );
    assert!(
        !base
            .diagnostics
            .iter()
            .any(|d| d.detail.contains("fx_idx_amp_split_point")
                || d.feature.contains("fx_idx_amp_split_point")),
        "fx_idx_amp_split_point must not be logged as ignored"
    );
}

const MORPHOLOGY: &str = "/data/main/Libraries/Kontakt/Morphology Evolved [Zero-G] rutracker.org";

/// A snapshot keeps its groups' volumes: Morphology's script picks one of
/// 30 wavetable groups and leaves its level to the snapshot (Fizzy plays
/// group 921, which the instrument itself keeps silent).
#[test]
fn a_snapshot_plays_its_groups_at_the_volumes_it_saved() {
    let root = Path::new(MORPHOLOGY);
    if !root.exists() {
        return;
    }
    let snapshots = root.join("Snapshots/Morphology Evolved");
    let base = kontra_format_kontakt::load(&root.join("Morphology Evolved.nki")).unwrap();
    let volumes = |i: &kontra_ir::Instrument| {
        (i.groups.iter())
            .map(|g| g.volume_db.max(-200.0).round() as i32)
            .collect::<Vec<_>>()
    };
    let default = kontra_format_kontakt::load(&snapshots.join("Default.nksn")).unwrap();
    assert_eq!(volumes(&default), volumes(&base));
    let fizzy = kontra_format_kontakt::load(&snapshots.join("Wavetables/Fizzy.nksn")).unwrap();
    let audible = |i: &kontra_ir::Instrument, g: usize| i.groups[g].volume_db > -20.0;
    assert!(audible(&fizzy, 921) && !audible(&base, 921));
    assert!(!audible(&fizzy, 903) && audible(&base, 903));
}

/// The members of one archive list in its own order, the same on every
/// read: a hash index's order changes per map, so a listing of
/// `ir_samples` (a response's sample index) changed between two loads.
#[test]
fn members_list_in_the_archives_order_on_every_read() {
    let offsets = [
        700u32, 100, 900, 300, 500, 200, 800, 400, 600, 1000, 1100, 1200,
    ];
    let mut bytes = 0x5e70ac54u32.to_le_bytes().to_vec();
    bytes.extend(0x110u16.to_le_bytes());
    bytes.extend([0; 8]);
    bytes.extend((offsets.len() as u32).to_le_bytes());
    bytes.extend([0; 4]);
    for (n, offset) in offsets.iter().enumerate() {
        bytes.extend(16u16.to_le_bytes());
        bytes.extend(offset.to_le_bytes());
        bytes.extend(0u16.to_le_bytes());
        for c in format!("m{n:02}").encode_utf16().chain([0]) {
            bytes.extend(c.to_le_bytes());
        }
    }
    let listed = || -> Vec<(u64, String)> {
        let archive =
            ni_file::nkr::Archive::read_index(std::io::Cursor::new(bytes.clone())).unwrap();
        archive
            .members()
            .map(|e| (e.header_offset, e.name.clone()))
            .collect()
    };
    let first = listed();
    let mut sorted = first.clone();
    sorted.sort();
    assert_eq!(first, sorted, "by header offset");
    for _ in 0..8 {
        assert_eq!(listed(), first);
    }
}

/// An instrument reads the same every time: an A/B or a saved project
/// relies on its IR, sample order included, being the file's own.
#[test]
fn an_instrument_loads_the_same_twice() {
    let root = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tools/kontakt-reference/scenarios"
    );
    for name in ["calibration.nki", "cutoff_mod.nki"] {
        let path = Path::new(root).join(name);
        let read = || format!("{:?}", kontra_format_kontakt::load(&path).unwrap());
        assert_eq!(read(), read(), "{name}");
    }
}

/// A snapshot kept in Kontakt's user folder (`User Content/<instrument>/`)
/// loads as the same snapshot in its library does, its base found among
/// the instruments the player has.
#[test]
fn a_user_snapshot_binds_to_its_instrument_by_name() {
    let factory = Path::new(ANALOG);
    let nki =
        Path::new("/data/main/Libraries/Kontakt/ANALOG STRINGS/Instruments/ANALOG STRINGS.nki");
    if !factory.exists() || !nki.exists() {
        return;
    }
    let root = std::env::temp_dir().join(format!("kontra-user-content-{}", std::process::id()));
    let user = root.join("User Content/ANALOG STRINGS/9001 Punch.nksn");
    std::fs::create_dir_all(user.parent().unwrap()).unwrap();
    std::fs::copy(factory, &user).unwrap();
    let known = [Path::new("/elsewhere/Piano.nki").to_owned(), nki.to_owned()];
    let alone = kontra_format_kontakt::load(&user);
    let found = kontra_format_kontakt::preset(&user, &known).map(|p| p.base);
    let loaded = kontra_format_kontakt::load_known(&user, &known);
    std::fs::remove_dir_all(&root).unwrap();
    assert!(alone.is_err(), "no base near it");
    assert_eq!(found.unwrap(), nki);
    // What a snapshot sets: its base, script values and effects.
    let set = |i: &kontra_ir::Instrument| {
        let groups: Vec<_> = i.groups.iter().map(|g| &g.inserts).collect();
        let saved: Vec<_> = i.scripts.iter().map(|s| &s.saved).collect();
        format!(
            "{:?}",
            (
                &i.name,
                &i.source_path,
                saved,
                groups,
                &i.inserts,
                &i.sends,
                &i.main,
                &i.buses
            )
        )
    };
    let original = kontra_format_kontakt::load(factory).unwrap();
    assert_eq!(set(&loaded.unwrap()), set(&original));
}

/// A multi Kontakt 8 saved (native oracle, `probes/multi_slots.lua`): three
/// empty instruments set to MIDI channels Omni, 3 and 18, outputs 0, 1 and
/// 0, -6, 0 and +3.5 dB, pans -50, 0 and 25 %.
#[test]
fn a_multis_slots_keep_the_channel_output_volume_and_pan_kontakt_saved() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/three-slots.nkm");
    let (slots, failed) = kontra_format_kontakt::multi(&path, None, &[]).unwrap();
    assert!(failed.is_empty(), "{failed:?}");
    let got: Vec<_> = (slots.iter())
        .map(|s| (s.index, s.channel, s.output))
        .collect();
    assert_eq!(got, [(0, 0, 0), (1, 3, 1), (2, 18, 0)]);
    let levels: Vec<_> = (slots.iter())
        .map(|s| {
            (
                (s.instrument.output.volume_db * 10.0).round(),
                s.instrument.output.pan,
            )
        })
        .collect();
    assert_eq!(levels, [(-60.0, -0.5), (0.0, 0.0), (35.0, 0.25)]);
}

/// The multi script Kontakt 8 saved in its first multi script slot.
#[test]
fn a_multis_own_script_is_read_from_its_bank() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/multi-script.nkm");
    let scripts = kontra_format_kontakt::multi_scripts(&path).unwrap();
    let got: Vec<_> = scripts.iter().map(|s| (s.slot, s.name.as_str())).collect();
    assert_eq!(got, [(0, "Channel 1 to 2")]);
    assert!(
        scripts[0].source.contains("set_midi(1, $MIDI_COMMAND"),
        "{}",
        scripts[0].source
    );
}

/// A wavetable group plays the quality it saved: 1..=4 are LOFI..BEST (Kontakt 8.13.1 saved the same patch
/// at each `$NI_WT_QUALITY_*`), and a patch that set none saves 3, HIGH, the default.
#[test]
fn a_wavetable_group_keeps_its_saved_quality() {
    use kontra_ir::Quality::{Best, High, Lofi, Medium};
    for (name, want) in [
        ("lofi", Lofi),
        ("medium", Medium),
        ("high", High),
        ("best", Best),
        ("none", High),
    ] {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/data/wt-quality-{name}.nki"));
        let i = kontra_format_kontakt::load(&path).unwrap();
        let table = i.groups[0].wavetable.as_ref().unwrap();
        assert_eq!(table.quality, want, "{name}");
    }
}

/// A Glide modulator on the source's pitch, saved by Kontakt 8.13 from the
/// probe that measured it (tools/kontakt-oracle/probes/glide.lua): Time mode,
/// 1224.2 ms, full intensity. Kontakt glided 12 semitones in 1208 ms and 4 in
/// 1238 ms, linearly, from the previous key (docs/engine/voices.md).
#[test]
fn a_saved_glide_modulator_is_a_timed_linear_glide_from_the_previous_key() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/glide-time.nki");
    let i = kontra_format_kontakt::load(&path).unwrap();
    let g = i.groups[0].glide.expect("glide");
    assert!((g.seconds - 1.2242).abs() < 1e-3, "{g:?}");
    assert_eq!(
        (g.per_semitone, g.legato, g.resume, g.depth, g.curve),
        (false, false, false, 1.0, 0.0)
    );
    assert!(
        !i.diagnostics.iter().any(|d| d.feature.contains("glide")),
        "{:?}",
        i.diagnostics
    );
}

/// Kontakt's own restore of a snapshot (`KONTRA_SNAPSHOT_ORACLE=<dir>`, the
/// files `tools/kontakt-oracle/probes/snapshot_restore.lua` saves: for pair
/// `n`, the instrument as loaded, `oracle.jsonl.<n>.base.nki`, and with the
/// snapshot applied, `oracle.jsonl.<n>.nki`, each in `<dir>/<library>/
/// Instruments/` beside links to the rest of its library) against KONTRA's reading of
/// the same snapshot (`KONTRA_SNAPSHOTS`, the snapshot paths in pair
/// order, `|`-separated): script values, groups' volume, pan and tune, and
/// effects, each counted as equal or not.
#[test]
#[ignore = "reads native oracle output on this machine"]
fn a_snapshot_restores_as_kontakt_restores_it() {
    let (Some(dir), Ok(snapshots)) = (
        std::env::var_os("KONTRA_SNAPSHOT_ORACLE"),
        std::env::var("KONTRA_SNAPSHOTS"),
    ) else {
        return;
    };
    let dir = Path::new(&dir);
    for (n, snapshot) in (1..).zip(snapshots.split('|')) {
        let ours = kontra_format_kontakt::load(Path::new(snapshot)).unwrap();
        // Each library's saved files sit in a folder of their own (an
        // encrypted library's instrument reads beside its `.nicnt`).
        let read = |name: String| {
            let found = std::fs::read_dir(dir).unwrap().flatten().find_map(|lib| {
                let p = lib.path().join("Instruments").join(&name);
                p.exists().then_some(p)
            });
            kontra_format_kontakt::load(&found.unwrap_or_else(|| dir.join(&name))).unwrap()
        };
        let kontakt = read(format!("oracle.jsonl.{n}.nki"));
        let base = read(format!("oracle.jsonl.{n}.base.nki"));
        let values = |i: &kontra_ir::Instrument| {
            (i.scripts.iter())
                .flat_map(|s| s.saved.iter().map(move |v| (s.slot, v.clone())))
                .collect::<Vec<_>>()
        };
        let (ours_v, kontakt_v, base_v) = (values(&ours), values(&kontakt), values(&base));
        let changed: Vec<_> = kontakt_v.iter().filter(|v| !base_v.contains(v)).collect();
        let changed_ours = changed.iter().filter(|v| ours_v.contains(v)).count();
        let differ: Vec<_> = kontakt_v.iter().filter(|v| !ours_v.contains(v)).collect();
        let groups = |i: &kontra_ir::Instrument| {
            (i.groups.iter())
                .map(|g| {
                    (
                        g.volume_db.max(-200.0).round() as i32,
                        (g.pan * 100.0).round() as i32,
                        (g.tune * 100.0).round() as i32,
                    )
                })
                .collect::<Vec<_>>()
        };
        let (og, kg) = (groups(&ours), groups(&kontakt));
        let groups_same = og.iter().zip(&kg).filter(|(a, b)| a == b).count();
        // Kontakt's saved copy names its impulse responses relative to
        // where Kontakt saved it, so a convolution reads as unknown there.
        let wet = |s: String| {
            let mut out = s.clone();
            while let Some(at) = out.find("Convolution { impulse: ") {
                let end = out[at..].find('}').map_or(out.len(), |e| at + e + 1);
                out.replace_range(at..end, "OtherWet");
            }
            out
        };
        let effects = |i: &kontra_ir::Instrument| {
            let mut all = vec![wet(format!("{:?}", (&i.inserts, &i.sends, &i.main)))];
            all.extend(i.groups.iter().map(|g| wet(format!("{:?}", g.inserts))));
            all
        };
        let (oe, ke) = (effects(&ours), effects(&kontakt));
        let effects_same = oe.iter().zip(&ke).filter(|(a, b)| a == b).count();
        println!(
            "{snapshot}\n  values: {} saved by Kontakt, {} differ from ours; {} the snapshot changed, {changed_ours} of them ours too\n  groups: {groups_same} of {} same volume/pan/tune\n  effects: {effects_same} of {} racks same",
            kontakt_v.len(),
            differ.len(),
            changed.len(),
            kg.len(),
            ke.len()
        );
        if std::env::var_os("KONTRA_SNAPSHOT_DETAIL").is_some() {
            println!(
                "    groups ours {:?}\n    kontakt {:?}\n    base {:?}",
                &og[..3],
                &kg[..3],
                &groups(&base)[..3]
            );
            for (a, b) in oe.iter().zip(&ke).filter(|(a, b)| a != b).take(1) {
                let at = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
                let near = |s: &str| {
                    s.chars()
                        .skip(at.saturating_sub(300))
                        .take(500)
                        .collect::<String>()
                };
                println!("    effects ours …{}\n    kontakt …{}", near(a), near(b));
            }
        }
        for v in differ.iter().take(8) {
            let mine = (ours_v.iter())
                .find(|o| o.0 == v.0 && o.1.split(' ').next() == v.1.split(' ').next());
            println!("    kontakt {v:?} ours {mine:?}");
        }
    }
}

/// A snapshot's Convolution records count the base's other files without the
/// instrument's own entry: read as the base's, index 0 named the `.nki` and
/// every ANALOG STRINGS and Una Corda snapshot failed `impulse-open`.
#[test]
fn snapshot_convolutions_name_responses_not_their_base_instrument() {
    for snapshot in [
        "/data/main/Libraries/Kontakt/ANALOG STRINGS/Snapshots/ANALOG STRINGS/Factory/Looking Up.nksn",
        "/data/main/Libraries/Kontakt/Una Corda Library/Snapshots/Una Corda Pure/Pure - In A Cave.nksn",
    ] {
        let path = Path::new(snapshot);
        if !path.exists() {
            continue;
        }
        let i = kontra_format_kontakt::load(path).unwrap();
        assert!(!i.impulses.is_empty(), "{snapshot}: no responses");
        for imp in &i.impulses {
            let name = &i.samples[imp.sample].name;
            assert!(
                !name.to_ascii_lowercase().ends_with(".nki"),
                "{snapshot}: response {name}"
            );
        }
    }
}

/// Pacific Ensemble Strings saves C1 (24) as its default key switch: carried,
/// not reported as ignored.
#[test]
fn a_saved_default_keyswitch_is_carried() {
    let path = Path::new(
        "/data/main/Libraries/Kontakt/Pacific Ensemble Strings/Instruments/16 Violins/Pacific - Ens Strings - 16 Violins - Legato Sustains.nki",
    );
    if !path.exists() {
        return;
    }
    let i = kontra_format_kontakt::load(path).unwrap();
    assert_eq!(i.default_keyswitch, Some(24));
    assert!(
        !i.diagnostics
            .iter()
            .any(|d| d.feature.contains("default_key_switch")),
        "{:?}",
        i.diagnostics
    );
}
