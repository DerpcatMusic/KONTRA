//! Real installed libraries. Each test skips when its library is absent: the
//! roots come from `KONTRA_KONTAKT_LIBRARIES` (path-list separated) or the
//! player's settings (`~/.config/kontra/settings.json`), as v1 finds them.
//! Nothing read here is written anywhere.

use sampler_core::{Input, Limits, Protocol, Runtime, Stealing};
use std::path::{Path, PathBuf};

#[path = "../../sampler-native/tests/support/reference.rs"]
mod reference;

fn roots() -> Vec<PathBuf> {
    if let Some(paths) = std::env::var_os("KONTRA_KONTAKT_LIBRARIES") {
        return std::env::split_paths(&paths).collect();
    }
    let Some(home) = std::env::var_os("HOME") else {
        return Vec::new();
    };
    let settings = std::fs::read_to_string(Path::new(&home).join(".config/kontra/settings.json"))
        .unwrap_or_default();
    // Each root is `{"path": "...", ...}`; only roots precede other keys holding paths.
    let roots = settings
        .split("\"roots\"")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .unwrap_or("");
    roots
        .split("\"path\"")
        .skip(1)
        .filter_map(|s| s.split('"').nth(1))
        .map(PathBuf::from)
        .collect()
}

fn find(relative: &str) -> Option<PathBuf> {
    let found = roots()
        .into_iter()
        .map(|root| root.join(relative))
        .find(|p| p.is_file());
    if found.is_none() {
        eprintln!("skipped: {relative} is not installed");
    }
    found
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

fn limits() -> Limits {
    Limits {
        notes: 16,
        channels: 1,
        performances: 1,
        families: 16,
        expressions: 16,
        voices: 256,
        decisions: 256,
        commands: 64,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    }
}

/// Load `keys` of an installed instrument without scripts, play its middle key
/// at `velocity` and return `frames` stereo frames.
fn render_frames(
    relative: &str,
    keys: std::ops::RangeInclusive<u8>,
    velocity: f64,
    frames: usize,
) -> Option<(sampler_ir::Instrument, Vec<[f32; 2]>)> {
    let path = find(relative)?;
    let options = sampler_kontakt::Options {
        scripts: false,
        ..reference::options(keys.clone())
    };
    let loaded = sampler_kontakt::load(&path, &options, |_| {}).unwrap();
    reference::assert_matched(&loaded.plan);
    let mut rt = Runtime::new(loaded.plan, limits()).unwrap();
    let key = (keys.start() + keys.end()) / 2;
    rt.trigger(input(key), key, velocity).unwrap();
    let mut out = vec![[0.0; 2]; frames];
    rt.render(&mut out).unwrap();
    Some((loaded.instrument, out))
}

/// [`render_frames`] for half a second at velocity 0.8: the instrument and its peak.
fn render(
    relative: &str,
    keys: std::ops::RangeInclusive<u8>,
) -> Option<(sampler_ir::Instrument, f32)> {
    let (ir, out) = render_frames(relative, keys, 0.8, 24000)?;
    let peak = out.iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
    Some((ir, peak))
}

#[test]
fn una_corda_pure_renders_from_its_encrypted_monolith() {
    let Some((ir, peak)) = render("Una Corda Library/Instruments/Una Corda Pure.nki", 60..=60)
    else {
        return;
    };
    assert_eq!(ir.name, "Una Corda Pure");
    assert!(
        ir.zones.len() > 1 && !ir.modulators.is_empty(),
        "{} zones",
        ir.zones.len()
    );
    assert!(
        ir.behaviors
            .iter()
            .all(|b| b.language == sampler_ir::Language::Ksp && !b.source.is_empty())
    );
    assert!(
        ir.unsupported.iter().any(|u| u.feature == "script"),
        "uncompiled scripts are reported"
    );
    // Two 20-voice, 50 ms voice groups, each used by one group (1-based).
    assert_eq!(ir.voice_limits.len(), 2);
    let mut used: Vec<_> = ir.groups.iter().filter_map(|g| g.voice_limit).collect();
    used.sort();
    assert_eq!(used, [0, 1]);
    assert_eq!(ir.voice_limit.map(|l| l.voices), Some(240));
    assert!(peak > 0.01, "audible: peak {peak}");
}

/// Isolate the production source path; decoded PCM and output stay in memory.
/// This proves the reflection implementation, not Kontakt interpolation parity.
#[test]
#[ignore = "requires installed Una Corda; run through kontakto-heavy"]
fn actual_una_corda_alternating_slots_match_independently_unrolled_pcm() {
    use sampler_ir as ir;
    for preset in ["Pure", "Felt", "Cotton"] {
        let path = find(&format!(
            "Una Corda Library/Instruments/Una Corda {preset}.nki"
        ))
        .expect("installed Una Corda is required for this proof");
        let mut library = sampler_kontakt::read(&path).unwrap();
        let (index, original, slot) = library
            .instrument
            .zones
            .iter()
            .enumerate()
            .find_map(|(i, z)| {
                let ir::Looping::Slots(slots) = z.playback.looping else {
                    return None;
                };
                let active: Vec<_> = slots.iter().flatten().copied().collect();
                (active.len() == 1 && active[0].range.alternating
                    && active[0].range.start == 33761 && active[0].range.end == 462673)
                    .then(|| (i, z.clone(), active[0]))
            })
            .unwrap();
        assert_eq!(
            (slot.count, slot.tuning, slot.until_release),
            (0, 1., false)
        );
        assert_eq!((slot.range.start, slot.range.end), (33761, 462673));
        assert_eq!(slot.range.crossfade.frames(48000.), 0);
        assert!(!original.playback.reverse);
        let decoded = library
            .samples
            .decode(&library.locations[original.asset.0])
            .unwrap();
        assert_eq!((decoded.rate, decoded.frames.len()), (48000, 475512));
        let start = slot.range.start as usize;
        let end = slot.range.end as usize;
        // Independent PCM oracle: initial forward pass, then endpoint-once
        // reverse/forward legs. Four periods provide the interpolation guards.
        let mut unrolled = decoded.frames[..end].to_vec();
        for _ in 0..4 {
            unrolled.extend(decoded.frames[start..end - 1].iter().rev().copied());
            unrolled.extend_from_slice(&decoded.frames[start + 1..end]);
        }
        unrolled.extend_from_slice(&decoded.frames[end..]);
        let mut zone = original.clone();
        zone.asset = ir::AssetRef(0);
        zone.group = Some(ir::GroupRef(0));
        zone.trigger = ir::Trigger::Attack;
        zone.selection = None;
        zone.articulation = None;
        zone.axes.clear();
        zone.conditions.clear();
        zone.routes.clear();
        zone.chain = None;
        zone.amplitude = None;
        let authored_group = &library.instrument.groups[original.group.unwrap().0];
        let mut isolated = ir::Instrument {
            assets: vec![library.instrument.assets[original.asset.0].clone()],
            groups: vec![ir::Group {
                gain: authored_group.gain,
                pan: authored_group.pan,
                tune: authored_group.tune,
                ..Default::default()
            }],
            zones: vec![zone],
            ..Default::default()
        };
        let prepare = |ir: &ir::Instrument, pcm| {
            sampler_core::lower::lower(ir, 48000, vec![pcm], |_, p| Ok(p)).unwrap()
        };
        let actual = prepare(
            &isolated,
            sampler_core::Pcm::new(decoded.rate, decoded.frames.into_boxed_slice()).unwrap(),
        );
        isolated.zones[0].playback.looping = ir::Looping::None;
        isolated.zones[0].playback.end = Some(unrolled.len() as u64);
        let expected = prepare(
            &isolated,
            sampler_core::Pcm::new(48000, unrolled.into_boxed_slice()).unwrap(),
        );
        let mut actual = Runtime::new(actual, limits()).unwrap();
        let mut expected = Runtime::new(expected, limits()).unwrap();
        let key = match original.pitch {
            ir::KeyTracking::Tracked { root } => root,
            _ => original.keys.low,
        };
        actual.trigger(input(key), key, 1.).unwrap();
        expected.trigger(input(key), key, 1.).unwrap();
        let frames = end + 3 * 2 * (end - start - 1);
        let mut a = [[0.; 2]; 257];
        let mut b = a;
        let mut peak = 0f32;
        for at in (0..frames).step_by(a.len()) {
            let count = (frames - at).min(a.len());
            actual.render(&mut a[..count]).unwrap();
            expected.render(&mut b[..count]).unwrap();
            assert_eq!(
                a[..count],
                b[..count],
                "{preset} source zone {index}, output frame {at}"
            );
            peak = a[..count]
                .iter()
                .flatten()
                .fold(peak, |p, x| p.max(x.abs()));
        }
        assert!(peak > 0.);
        eprintln!("loop_proof\t{preset}\t{index}\t{frames}\t{start}\t{end}\t0\t{peak}");
    }
}

#[test]
fn conflux_renders_its_tracked_zones_within_the_runtime_pitch_range() {
    let Some((ir, peak)) = render(
        "Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki",
        48..=72,
    ) else {
        return;
    };
    assert!(
        ir.unsupported
            .iter()
            .any(|u| u.feature == "wavetable source")
    );
    assert!(peak > 0.01, "audible: peak {peak}");
}

#[test]
fn conflux_admits_all_257_saved_values_including_13_string_arrays() {
    let Some(path) = find("Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki") else {
        return;
    };
    use ni_file::kontakt::objects::{BParScript, Program};
    let chunks = sampler_kontakt::read_chunks(&path).unwrap();
    let program = Program::try_from(chunks.find_first(0x28).unwrap()).unwrap();
    let raw: Vec<_> = program
        .0
        .children
        .iter()
        .filter(|c| c.id == 6)
        .map(|c| BParScript::try_from(c).unwrap().params().unwrap())
        .filter(|s| !s.bypass)
        .flat_map(|s| s.persistent)
        .collect();
    assert_eq!(raw.len(), 257);
    assert_eq!(raw.iter().filter(|s| s.starts_with('!')).count(), 13);
    let translated = sampler_kontakt::read(&path).unwrap();
    let saved: Vec<_> = translated
        .instrument
        .behaviors
        .iter()
        .flat_map(|s| &s.state)
        .collect();
    assert_eq!(saved.len(), 257);
    assert_eq!(
        saved
            .iter()
            .filter(|(_, v)| matches!(v, sampler_ir::Saved::Texts(_)))
            .count(),
        13
    );
    for raw in raw {
        let name = raw.split_once(' ').unwrap().0;
        assert!(
            saved.iter().any(|(n, _)| n == name),
            "saved value was not admitted"
        );
    }
}

#[test]
fn streamed_real_instrument_installs_physical_engine_bindings_and_lookups() {
    let Some(path) = find("Una Corda Library/Instruments/Una Corda Pure.nki") else { return };
    let streamed = sampler_kontakt::load_streamed(&path, &sampler_kontakt::Options {
        keys: 60..=60, mpe: None, ..Default::default()
    }, &Default::default(), |_| {}).unwrap();
    let loaded = streamed.loaded;
    let bindings = loaded.plan.engine_parameter_bindings();
    assert!(!bindings.is_empty(), "production preparation needs authored native lanes");
    assert!(!loaded.plan.engine_lookups().is_empty(), "production needs physical names");
    for binding in bindings {
        assert!(loaded.instrument.source_indices.modulators.iter().any(|source|
            source.group as i32 == binding.address.group && source.slot as i32 == binding.address.slot
                && source.runtime.is_some() && !source.external));
        assert!(loaded.plan.controls().iter().any(|control|control.id == binding.control));
    }
    let attack = bindings.iter().find(|b|sampler_core::engine_parameter_name(b.address.parameter)==Some("$ENGINE_PAR_ATTACK")).unwrap().address;
    let plan=loaded.plan;
    let limits=Limits::for_plan(&plan,16,16);
    let mut runtime=Runtime::new(plan,limits).unwrap();
    runtime.set_engine_parameter(attack,200809).unwrap();
    assert!((runtime.engine_parameter(attack).unwrap()-200809).abs()<=1);
    println!("PRODUCTION_ENGINE_BINDINGS controls={} lookups={}",runtime.control_definitions(runtime.active_plan()).unwrap().len(),loaded.instrument.source_indices.engine_lookups.len());
}

#[test]
fn vista_cellos_render_from_loose_ncw_samples() {
    let Some((ir, out)) = render_frames(
        "Performance Samples Vista/Instruments/Vista - 3 Cellos.nki",
        48..=48,
        100.0 / 127.0,
        144_000,
    ) else {
        return;
    };
    let ncw = ir
        .assets
        .iter()
        .filter(|a| a.encoding == sampler_ir::Encoding::Ncw)
        .count();
    assert_eq!(ncw, ir.assets.len());
    // Kontakt 8, scripts on, nothing sent, key 48 vel 100, scripts decide
    // nothing KONTRA changes here: max(|L|, |R|) peak over 0.3-3 s is
    // -38.6 dBFS (KONTAKT_REFERENCE.md s.13; the older -45.0 is a (L+R)/2
    // mono mix). KONTRA measures +0.9 dB; the open residual is under 1 dB.
    let db = reference::levels(&out, 0.3, 3.0).max_peak();
    assert!(
        (db + 38.6).abs() < 1.0,
        "{db:.1} dBFS max-channel peak against Kontakt's -38.6"
    );
}

/// The first instrument of every installed library translates, or
/// fails only for want of local access data. Sample audio is not decoded.
#[test]
fn installed_instruments_translate_or_report_why_not() {
    let mut translated = 0;
    for root in roots() {
        let Ok(libraries) = std::fs::read_dir(&root) else {
            continue;
        };
        for library in libraries.flatten() {
            let mut instruments = Vec::new();
            collect(&library.path().join("Instruments"), &mut instruments);
            instruments.sort();
            for path in instruments.iter().take(1) {
                match sampler_kontakt::read(path) {
                    Ok(kontakt) => {
                        translated += 1;
                        eprintln!(
                            "{}: {} groups, {} zones, {} unsupported",
                            path.display(),
                            kontakt.instrument.groups.len(),
                            kontakt.instrument.zones.len(),
                            kontakt.instrument.unsupported.len()
                        );
                    }
                    Err(error @ sampler_kontakt::LoadError::Access { .. }) => eprintln!("{error}"),
                    Err(error) => panic!("{error}"),
                }
            }
        }
    }
    eprintln!("{translated} instruments translated");
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nki"))
        {
            out.push(path);
        }
    }
}

#[test]
fn a_canceled_load_stops_before_decoding() {
    let Some(path) = find("Una Corda Library/Instruments/Una Corda Pure.nki") else {
        return;
    };
    let mut decoded = 0;
    let result = sampler_kontakt::load_cancelable(
        &path,
        &sampler_kontakt::Options::default(),
        |p| decoded += matches!(p, sampler_kontakt::Progress::Decoding { .. }) as usize,
        || true,
    );
    assert!(matches!(result, Err(sampler_kontakt::LoadError::Canceled)));
    assert_eq!(decoded, 0);
}

/// Every installed instrument with authored modulation loads, lowers and
/// renders; prints per-feature report counts. Slow: run with --ignored.
#[test]
#[ignore]
fn survey_modulated_instruments_lower_and_render() {
    let mut instruments = Vec::new();
    for root in roots() {
        collect(&root, &mut instruments);
    }
    instruments.sort();
    let (mut loaded, mut routed, mut failed) = (0, 0, 0);
    let mut features = std::collections::BTreeMap::<String, usize>::new();
    for path in &instruments {
        let Ok(kontakt) = sampler_kontakt::read(path) else {
            continue;
        };
        for u in &kontakt.instrument.unsupported {
            if u.feature.contains("modulat")
                || u.feature.contains("LFO")
                || u.feature.contains("envelope")
            {
                *features
                    .entry(format!("{:?} {}", u.reason, u.feature))
                    .or_default() += 1;
            }
        }
        if kontakt.instrument.routes.is_empty() {
            continue;
        }
        routed += 1;
        let options = sampler_kontakt::Options {
            keys: 60..=60,
            scripts: false,
            ..Default::default()
        };
        match sampler_kontakt::load(path, &options, |_| {}) {
            Ok(loaded_plan) => {
                loaded += 1;
                let mut rt = match Runtime::new(loaded_plan.plan, limits()) {
                    Ok(rt) => rt,
                    Err(error) => {
                        println!("RUNTIME {}: {error:?}", path.display());
                        continue;
                    }
                };
                // As the product plays: steal at capacity rather than reject.
                rt.set_voice_stealing(Some(Stealing::for_limits(48000, limits().voices)))
                    .unwrap();
                if let Err(error) = rt.trigger(input(60), 60, 0.8) {
                    println!("TRIGGER {}: {error:?}", path.display());
                    continue;
                }
                let mut out = vec![[0.0; 2]; 9600];
                rt.render(&mut out).unwrap();
                let peak = out.iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
                assert!(peak.is_finite());
                println!(
                    "ok {peak:.3} {} routes {}",
                    path.display(),
                    loaded_plan.instrument.routes.len()
                );
            }
            Err(error) => {
                failed += 1;
                println!("FAIL {}: {error}", path.display());
            }
        }
    }
    for (feature, count) in &features {
        println!("{count:6} {feature}");
    }
    println!("{routed} instruments with routes: {loaded} loaded, {failed} failed");
}

/// Afflatus' multi-articulation patch switches in KSP: `on note` reads keys
/// `%KS_keys[$KS_Base]` onward into `$articulation`. The generated map taps
/// those keys, and its alternatives follow key order.
#[test]
fn afflatus_keyswitch_script_yields_an_articulation_map() {
    let Some(path) =
        find("Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/2 Horns KS.nki")
    else {
        return;
    };
    let ir = sampler_kontakt::read(&path).unwrap().instrument;
    for a in &ir.articulations {
        eprintln!(
            "{:>20} keys {:?} default {} -> {:?}",
            a.name, a.switch_keys, a.default, a.alternatives
        );
    }
    assert_eq!(ir.switching.owner, sampler_ir::SwitchOwner::Behavior);
    let names: Vec<_> = ir.articulations.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names.len(), 11);
    assert_eq!(&names[..3], ["Sustain + Legato", "Flutter", "Marcato"]);
    let keys: Vec<_> = ir.articulations.iter().map(|a| a.switch_keys[0]).collect();
    assert_eq!(keys, (24..=34).collect::<Vec<_>>());
    assert!(ir.articulations[0].default);
    let marcato = ir.articulations[2].alternatives;
    assert_eq!(
        marcato.controller.map(|c| (c.controller, c.low)),
        Some((32, 2))
    );
    assert_eq!((marcato.channel, marcato.program), (Some(2), Some(2)));
    let mut driven = ir.clone();
    driven.switching.driver = sampler_ir::Driver::Velocity;
    assert_eq!(driven.validate(), Ok(()));
}

/// Every report feature across the installed instruments, as (instruments,
/// entries). Translation only, nothing written: run with --ignored.
#[test]
#[ignore]
fn survey_reported_features() {
    let mut instruments = Vec::new();
    for root in roots() {
        collect(&root, &mut instruments);
    }
    instruments.sort();
    let mut features = std::collections::BTreeMap::<String, (usize, usize)>::new();
    for path in &instruments {
        let Ok(kontakt) = sampler_kontakt::read(path) else {
            continue;
        };
        let mut seen = std::collections::HashSet::new();
        for u in &kontakt.instrument.unsupported {
            let key = format!("{:?} {}", u.reason, u.feature);
            let entry = features.entry(key.clone()).or_default();
            entry.1 += 1;
            if seen.insert(key) {
                entry.0 += 1;
            }
        }
    }
    let mut sorted: Vec<_> = features.into_iter().collect();
    sorted.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
    for (feature, (n, entries)) in sorted {
        println!("{n:6} {entries:9} {feature}");
    }
    println!("{} instruments", instruments.len());
}

/// Remapping Afflatus to each driver gives every articulation a selector that
/// taps its own switch key (the script owns the switching), and the driver's
/// value selects it.
#[test]
fn afflatus_remaps_to_every_driver() {
    use sampler_core::{Driver as D, Switch};
    use sampler_ir::Driver;
    let Some(path) =
        find("Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/2 Horns KS.nki")
    else {
        return;
    };
    let mut ir = sampler_kontakt::read(&path).unwrap().instrument;
    ir.assign_alternatives(32);
    for (driver, core) in [
        (Driver::Velocity, D::Velocity),
        (Driver::Channel, D::Channel),
        (Driver::Controller, D::Controller),
        (Driver::Program, D::Program),
    ] {
        ir.switching.driver = driver;
        assert_eq!(ir.validate(), Ok(()));
        let (_, switching) = sampler_core::lower::switching(&ir, ir.switching).unwrap();
        assert_eq!(switching.driver(), core);
        for a in &ir.articulations {
            let alt = a.alternatives;
            let (controller, value) = match driver {
                Driver::Velocity => (0, alt.velocities.unwrap().low),
                Driver::Channel => (0, alt.channel.unwrap()),
                Driver::Controller => (32, alt.controller.unwrap().low),
                _ => (0, alt.program.unwrap()),
            };
            assert_eq!(
                switching.select(controller, value),
                Some(Switch::Tap(a.switch_keys[0])),
                "{driver:?} {}",
                a.name
            );
        }
    }
}
