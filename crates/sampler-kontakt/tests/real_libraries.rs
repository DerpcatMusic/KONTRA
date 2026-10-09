//! Real installed libraries. Each test skips when its library is absent: the
//! roots come from `KONTRA_KONTAKT_LIBRARIES` (path-list separated) or the
//! player's settings (`~/.config/kontra/settings.json`), as v1 finds them.
//! Nothing read here is written anywhere.

use sampler_core::{Input, Limits, Protocol, Runtime, Stealing};
use std::path::{Path, PathBuf};

#[path = "../../sampler-native/tests/support/reference.rs"]
mod reference;

#[path = "../../sampler-core/tests/support/mod.rs"]
mod heap;

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
    let authored = ir.kontakt_objects.as_ref().unwrap();
    let voices = authored.voice_groups.as_ref().unwrap();
    assert_eq!(voices.program.max_num_voices, 240);
    assert_eq!(voices.groups.len(), 128);
    assert_eq!(voices.groups.iter().flatten().count(), 2);
    for slot in 0..2 {
        let limit = voices.groups[slot].as_ref().unwrap();
        assert_eq!((limit.max_num_voices, limit.ms_fade_time), (20, 50));
    }
    assert!(authored.groups.iter().any(|g| g.voice_group_index == 1));
    assert!(authored.groups.iter().any(|g| g.voice_group_index == 2));
    assert!(authored.zones.len() >= ir.zones.len());
    println!(
        "Una Corda: authored_groups={} authored_zones={} active_zones={} peak={peak}",
        authored.groups.len(),
        authored.zones.len(),
        ir.zones.len()
    );
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
    let mut translated = sampler_kontakt::read(&path).unwrap();
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
    // Use the production compiler and capture the actual native text banks.
    #[cfg(feature = "scan")]
    sampler_ksp::scan::begin();
    let (scripts, _, _) = sampler_kontakt::compile_ui(
        &mut translated.instrument,
        &sampler_kontakt::Options {
            library: Some(path),
            mpe: None,
            ..Default::default()
        },
    );
    println!(
        "CONFLUX_NATIVE scripts={} persistent={} text_arrays={}",
        scripts.len(),
        scripts
            .iter()
            .map(|s| s.model().persistent.len())
            .sum::<usize>(),
        scripts
            .iter()
            .map(|s| s
                .model()
                .persistent
                .iter()
                .filter(|p| p.name.starts_with('!'))
                .count())
            .sum::<usize>()
    );
    #[cfg(feature = "scan")]
    for observation in sampler_ksp::scan::take() {
        println!("CONFLUX_SCRIPT_PHASE {observation:?}");
    }
    let views: Vec<_> = scripts.iter().map(|s| s.view()).collect();
    let mut capture = sampler_ksp::persistent_state_buffer(&views).unwrap();
    let plan = sampler_ksp::bind_modules(
        scripts,
        sampler_core::Prepared::new(48000, vec![], vec![], 0).unwrap(),
    )
    .unwrap();
    let limits = Limits::for_plan(&plan, 4, 4);
    let runtime = Runtime::new(plan, limits).unwrap();
    runtime
        .capture_script_state(runtime.active_plan(), &mut capture)
        .unwrap();
    let mut restored = 0;
    for (instance, view) in views.iter().enumerate() {
        let behavior = translated
            .instrument
            .behaviors
            .iter()
            .find(|b| b.slot.unwrap_or(0) == view.slot())
            .unwrap();
        for persistent in view
            .model()
            .persistent
            .iter()
            .filter(|p| p.name.starts_with('!'))
        {
            let sampler_ksp::model::Location::Texts { offset, len } = persistent.location else {
                panic!("text array needs native text storage")
            };
            let Some(sampler_ir::Saved::Texts(expected)) = behavior
                .state
                .iter()
                .find(|(name, _)| name == &persistent.name)
                .map(|(_, v)| v)
            else {
                panic!("saved text array missing")
            };
            for (index, text) in expected.iter().take(len as usize).enumerate() {
                let address = sampler_core::ScriptStateAddress::Text {
                    instance: sampler_core::ScriptInstanceId(instance as u16),
                    index: offset + index as u32,
                };
                let value = capture
                    .values
                    .iter()
                    .find(|v| v.address == address)
                    .unwrap()
                    .value;
                assert!(
                    value
                        == sampler_core::ScriptStateValue::Text(
                            sampler_core::Text::try_new(text).unwrap()
                        ),
                    "authored text array must reach native storage"
                );
            }
            restored += 1;
        }
    }
    assert_eq!(
        restored, 13,
        "all saved string arrays restored into native banks"
    );
}

#[test]
fn dolce_init_getter_feedback_preserves_authored_envelope_lanes() {
    let Some(path) = find(
        "Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 03 7 1st Violins - Sustained Con Sordino.nki",
    ) else {
        return;
    };
    let streamed = sampler_kontakt::load_streamed(
        &path,
        &sampler_kontakt::Options {
            keys: 60..=60,
            mpe: None,
            ..Default::default()
        },
        &Default::default(),
        |_| {},
    )
    .unwrap_or_else(|_| panic!("production load failed; authored diagnostics omitted"));
    let plan = streamed.loaded.plan;
    let addresses: Vec<_> = plan
        .engine_parameter_bindings()
        .iter()
        .filter(|binding| {
            binding.address.slot == 1
                && [
                    "$ENGINE_PAR_ATTACK",
                    "$ENGINE_PAR_RELEASE",
                    "$ENGINE_PAR_SUSTAIN",
                ]
                .contains(&sampler_core::engine_parameter_name(binding.address.parameter).unwrap())
        })
        .map(|binding| {
            (
                binding.address,
                binding.law.encode(
                    match plan
                        .controls()
                        .iter()
                        .find(|control| control.id == binding.control)
                        .unwrap()
                        .default
                    {
                        sampler_core::ControlValue::Real(value) => value,
                        _ => panic!("envelope lane requires a native real value"),
                    },
                ),
            )
        })
        .collect();
    assert!(
        !addresses.is_empty(),
        "physical envelope bindings must be installed"
    );
    let limits = Limits::for_plan(&plan, 128, 16);
    let mut runtime = Runtime::new(plan, limits).unwrap();
    for (address, authored) in &addresses {
        assert!(*authored > 0);
        assert!(
            (runtime.engine_parameter(*address).unwrap() - authored).abs() <= 1,
            "init getter feedback must retain authored envelope at physical group {} slot {} parameter {}",
            address.group,
            address.slot,
            address.parameter
        );
    }
    println!("DOLCE_AUTHORED_ENVELOPE retained_lanes={}", addresses.len());
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
    println!("Vista: max_channel_peak_dbfs={db:.3} native_reference_dbfs=-38.6");
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

/// W8's embedded-NKM witness: script init must preserve audible program 1.
#[test]
fn big_screen_embedded_program_one_remains_audible_after_script_init() {
    let Some(path) = find("Conflux 1.1.0 [Native Instruments]/Multis/Big Screen.nkm") else {
        return;
    };
    let translated = sampler_kontakt::read_program(&path, 1).unwrap();
    let options = sampler_kontakt::Options {
        keys: 60..=64,
        library: Some(path),
        mpe: None,
        ..Default::default()
    };
    let loaded = sampler_kontakt::load_read(translated, &options, |_| {}, || false).unwrap();
    assert!(!loaded.scripts.is_empty(), "embedded scripts must bind");
    assert!(!loaded.plan.engine_parameter_bindings().is_empty());
    assert!(!loaded.plan.engine_lookups().is_empty());
    let mut limits = Limits::for_plan(&loaded.plan, 32, 256);
    limits.behaviors = limits.behaviors.max(256);
    limits.behavior_cells = limits.behaviors * loaded.plan.behavior_local_count();
    let mut runtime = Runtime::new(loaded.plan, limits).unwrap();
    let performance = runtime.performance(0).unwrap();
    let origin = sampler_core::ChannelAddress {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
    };
    for (cc, value) in [(1u8, 100u8), (11, 127)] {
        runtime
            .dispatch_controller(
                performance,
                origin,
                1,
                cc,
                ((u64::from(value) * u64::from(u32::MAX)) / 127) as u32,
            )
            .unwrap();
    }
    for key in [60, 64] {
        runtime.trigger(input(key), key, 100.0 / 127.0).unwrap();
    }
    let mut peak = 0.0f32;
    let mut output = [[0.0; 2]; 128];
    for _ in 0..188 {
        runtime.render(&mut output).unwrap();
        peak = output.iter().flatten().fold(peak, |p, x| p.max(x.abs()));
        runtime.flush_behaviors(|_, _, _| true);
    }
    println!("BIG_SCREEN_PROGRAM_1 peak={peak:.8}");
    assert!(
        peak.is_finite() && peak > 1e-5,
        "authored init must preserve audible output"
    );
}

#[test]
#[ignore = "requires installed Morphology; run through kontakto-heavy"]
fn w15_authored_pan_offline_ab_changes_channel_balance() {
    use sampler_ir as ir;
    let Some(path) = find("Morphology Evolved [Zero-G] rutracker.org/Morphology Evolved.nki") else { return; };
    let render = |enabled| {
        let library = sampler_kontakt::read(&path).unwrap();
        let (zone, route) = library.instrument.zones.iter().find_map(|z| {
            z.routes.iter().find_map(|r| {
                let route = library.instrument.routes[r.0];
                (route.target == ir::Target::Pan
                    && matches!(route.depth, ir::Depth::Normalized(d) if d.abs() > 0.01)
                    && matches!(library.instrument.modulators[route.source.0].source, ir::ModulationSource::Envelope(_)))
                    .then(|| (z.clone(), *r))
            })
        }).expect("gate item must retain its authored AHDSR -> Pan route");
        let mut isolated = zone;
        isolated.routes = if enabled { vec![route] } else { vec![] };
        isolated.chain = None;
        reference::levels(&w15_render_one_authored_zone(library, isolated), 0.1, 0.45)
    };
    let dry = render(false);
    let wet = render(true);
    let balance_delta = (wet.rms[1] - wet.rms[0]) - (dry.rms[1] - dry.rms[0]);
    println!("W15 pan dry_rms={:?} wet_rms={:?} balance_delta_db={balance_delta}", dry.rms, wet.rms);
    assert!(dry.max_peak() > -80. && wet.max_peak() > -80.);
    assert!(balance_delta.abs() > 0.05, "the retained route must reach actual audio");
}

fn w15_render_one_authored_zone(mut library: sampler_kontakt::Kontakt, mut zone: sampler_ir::Zone) -> Vec<[f32; 2]> {
    use sampler_ir as ir;
    let native_filter = zone.chain.is_some_and(|c| library.instrument.chains[c.0].pre_amplitude.iter()
        .chain(&library.instrument.chains[c.0].post_amplitude).any(|p| matches!(p, ir::Processor::LadderLP4(_) | ir::Processor::Daft(_))));
    let key = 60u8.clamp(zone.keys.low, zone.keys.high);
    zone.selection = None;
    zone.articulation = None;
    zone.axes.clear();
    zone.conditions.clear();
    zone.trigger = ir::Trigger::Attack;
    zone.velocities = ir::VelocityRange { low: 0, high: 127 };
    let original = &library.instrument;
    let mut modulators = Vec::new();
    let mut routes = Vec::new();
    for route in &mut zone.routes {
        let mut r = original.routes[route.0];
        if let ir::Target::Processor { chain, .. } = &mut r.target {
            *chain = ir::ChainRef(0);
        }
        modulators.push(original.modulators[r.source.0].clone());
        r.source = ir::ModulatorRef(modulators.len() - 1);
        if let Some(scale) = &mut r.scale {
            modulators.push(original.modulators[scale.source.0].clone());
            scale.source = ir::ModulatorRef(modulators.len() - 1);
        }
        *route = ir::RouteRef(routes.len());
        routes.push(r);
    }
    if let Some(amplitude) = &mut zone.amplitude {
        modulators.push(original.modulators[amplitude.0].clone());
        *amplitude = ir::ModulatorRef(modulators.len() - 1);
    }
    let chains = zone.chain.map(|chain| {
        zone.chain = Some(ir::ChainRef(0));
        original.chains[chain.0].clone()
    }).into_iter().collect();
    let groups = zone.group.map(|group| {
        let mut g = original.groups[group.0].clone();
        zone.group = Some(ir::GroupRef(0));
        g.chain = None;
        g.start.clear();
        g.tap = None;
        g.output = ir::Output::Master;
        g.sends.clear();
        g.voice_limit = None;
        g
    }).into_iter().collect();
    library.instrument = ir::Instrument {
        name: original.name.clone(), assets: original.assets.clone(),
        shapes: original.shapes.clone(), zones: vec![zone], groups,
        chains, routes, modulators, ..Default::default()
    };
    let loaded = sampler_kontakt::load_read(library, &sampler_kontakt::Options {
        keys: key..=key, scripts: false, mpe: None, ..Default::default()
    }, |_| {}, || false).unwrap();
    assert!(loaded.plan.region_count() > 0, "authored gate zone was dropped before rendering: {:?}", loaded.instrument.unsupported);
    let fitted = loaded.instrument.zones[0].keys;
    let key = key.clamp(fitted.low, fitted.high);
    let plan = if native_filter { loaded.plan.with_signal_trace(8192).unwrap() } else { loaded.plan };
    let mut rt = Runtime::new(plan, limits()).unwrap();
    rt.trigger(input(key), key, 1.).unwrap();
    if native_filter {
        assert!(rt.voice_count() > 0, "native witness not admitted: {:?}, fitted_keys={fitted:?} key={key}", rt.take_silent_note());
    }
    let mut out = vec![[0.; 2]; 24_000];
    heap::without_heap(|| rt.render(&mut out).unwrap());
    if native_filter && out.iter().flatten().all(|v| *v == 0.) {
        let reader = rt.signal_trace_reader().unwrap();
        let rows = reader.drain();
        println!("W15 silent witness graph_nodes={} rows={} voices={}", reader.graph.nodes.len(), rows.len(), rt.voice_count());
        let mut seen = std::collections::BTreeSet::new();
        for row in rows.into_iter().filter(|r| seen.insert(r.node)) {
            let node = &reader.graph.nodes[row.node];
            println!("W15 silent witness node={} processor={} input_rms={:?} output_rms={:?} gain={:?} region_gain={} envelope={}",
                node.kind, node.processor, row.input.rms, row.output.rms, row.gain, row.identity.region_gain, row.identity.envelope_level);
        }
    }
    out
}

#[test]
#[ignore = "requires installed Vista Harp; run through kontakto-heavy"]
fn vista_harp_mode3_nulls_against_frozen_v1() {
    use sampler_ir as ir;
    let path = find("Performance Samples Vista/Instruments/Bonus/Vista - Harp.nki")
        .expect("installed Vista Harp is required");
    let reference = std::fs::read(std::env::var_os("KONTRA_W15_V1_REFERENCE")
        .expect("frozen v1 --dry --no-script group 0, key 60, velocity 127 reference")).unwrap();
    assert_eq!(&reference[..4], b"RIFF");
    assert_eq!(&reference[8..12], b"WAVE");
    let mut offset = 12;
    let mut format = None;
    let mut data = None;
    while offset + 8 <= reference.len() {
        let size = u32::from_le_bytes(reference[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let body = reference.get(offset + 8..offset + 8 + size).expect("complete WAV chunk");
        match &reference[offset..offset + 4] {
            b"fmt " => format = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        offset += 8 + size + (size & 1);
    }
    let format = format.expect("WAV format");
    assert!(format.len() >= 16);
    assert_eq!(u16::from_le_bytes(format[..2].try_into().unwrap()), 3, "float PCM");
    assert_eq!(u16::from_le_bytes(format[2..4].try_into().unwrap()), 2);
    assert_eq!(u32::from_le_bytes(format[4..8].try_into().unwrap()), reference::RATE);
    assert_eq!(u16::from_le_bytes(format[14..16].try_into().unwrap()), 32);
    let data = data.expect("WAV data");
    assert!(data.len() >= 24_000 * 8 && data.len() % 8 == 0);
    let mut library = sampler_kontakt::read(&path).unwrap();
    let objects = library.instrument.kontakt_objects.as_ref().unwrap();
    assert_eq!(objects.groups.len(), 20);
    assert!(objects.groups.iter().all(|g| g.source.as_ref().is_some_and(|s| s.mode == 3)));
    library.instrument.zones.retain(|z| z.group == Some(ir::GroupRef(0)));
    for group in &mut library.instrument.groups {
        group.output = ir::Output::Master;
        group.sends.clear();
    }
    library.instrument.buses.clear();
    library.instrument.input_bus = None;
    let loaded = sampler_kontakt::load_read(library, &sampler_kontakt::Options {
        keys: 60..=60, scripts: false, mpe: None, ..Default::default()
    }, |_| {}, || false).unwrap();
    let mut runtime = Runtime::new(loaded.plan, limits()).unwrap();
    runtime.trigger(input(60), 60, 1.).unwrap();
    let mut out = vec![[0.; 2]; 24_000];
    heap::without_heap(|| runtime.render(&mut out).unwrap());
    let mut peak = 0f64;
    let mut residual = 0f64;
    let mut power = 0f64;
    for (got, bytes) in out.iter().flatten().zip(data.chunks_exact(4)) {
        // v1's CLI applies a final 0.25 gain outside its engine.
        let want = f64::from(f32::from_le_bytes(bytes.try_into().unwrap())) * 4.;
        assert!(want.is_finite() && got.is_finite());
        let difference = f64::from(*got) - want;
        peak = peak.max(difference.abs());
        residual += difference * difference;
        power += want * want;
    }
    assert!(power > 1e-8, "the reference must sound");
    println!("W15_HARP_MODE3 peak_error={peak} residual_db={} voices={}",
        10. * (residual / power).log10(), runtime.voice_count());
    assert!(peak <= 1e-6, "mode-3 playback must null against frozen v1");
}

#[test]
#[ignore = "requires installed Vista Harp; run through kontakto-heavy"]
fn vista_legacy_lowpass_filters_the_authored_damping_release() {
    use sampler_ir as ir;
    let path = find("Performance Samples Vista/Instruments/Bonus/Vista - Harp.nki")
        .expect("installed Vista Harp is required for this witness");
    let render = |enabled, modulated| {
        let mut library = sampler_kontakt::read(&path).unwrap();
        assert!(!library.instrument.unsupported.iter().any(|u|
            matches!(u.feature.as_str(), "Filter: filter type" | "effect" | "modulation of a module parameter")),
            "Vista's four filter/effect/route omissions must all be closed");
        for group in [8, 9, 18, 19] {
            let zone = library.instrument.zones.iter().find(|z| z.group == Some(ir::GroupRef(group))).unwrap();
            let chain = &library.instrument.chains[zone.chain.expect("legacy slot must survive").0];
            assert!(chain.pre_amplitude.iter().chain(&chain.post_amplitude).any(|p|
                matches!(p, ir::Processor::Filter(ir::Filter { kind: ir::FilterKind::LowPass { poles: 2 }, .. }))));
            assert!(zone.routes.iter().map(|r| library.instrument.routes[r.0]).any(|r|
                matches!(r.target, ir::Target::Processor { parameter: ir::ProcessorParameter::Cutoff, .. })
                    && matches!(r.depth, ir::Depth::Pitch(p) if (p.semitones() - 12. * 8.96).abs() < 1e-6)),
                "the full-depth release envelope must use v1's cutoff knob span");
        }
        let mut zone = library.instrument.zones.iter().find(|z| z.group == Some(ir::GroupRef(8))
            && z.keys.low <= 60 && z.keys.high >= 60).expect("middle-C damping release").clone();
        let chain = &library.instrument.chains[zone.chain.unwrap().0];
        let filters = chain.pre_amplitude.iter().chain(&chain.post_amplitude).filter(|p|
            matches!(p, ir::Processor::Filter(ir::Filter { kind: ir::FilterKind::LowPass { poles: 2 }, .. })))
            .copied().collect::<Vec<_>>();
        assert_eq!(filters.len(), 1);
        // Isolate this authored release sample and its saved slot, with no script/selection variation.
        zone.routes.retain(|r| modulated && matches!(library.instrument.routes[r.0].target,
            ir::Target::Processor { parameter: ir::ProcessorParameter::Cutoff, .. }));
        let new_chain = ir::ChainRef(library.instrument.chains.len());
        for route in &zone.routes {
            library.instrument.routes[route.0].target = ir::Target::Processor {
                chain: new_chain, index: 0, parameter: ir::ProcessorParameter::Cutoff };
        }
        zone.chain = Some(new_chain);
        library.instrument.chains.push(ir::Chain { scope: ir::Scope::Voice,
            pre_amplitude: if enabled { filters } else { vec![] }, post_amplitude: vec![] });
        w15_render_one_authored_zone(library, zone)
    };
    let dry = render(false, false);
    let wet = render(true, false);
    let modulated = render(true, true);
    let energy = |frames: &[[f32; 2]]| frames.iter().flatten().map(|v| f64::from(*v).powi(2)).sum::<f64>();
    let derivative = |frames: &[[f32; 2]]| frames.windows(2).flat_map(|w|
        (0..2).map(move |c| f64::from(w[1][c] - w[0][c]).powi(2))).sum::<f64>();
    let residual = dry.iter().zip(&wet).flat_map(|(a,b)|
        (0..2).map(move |c| f64::from(a[c] - b[c]).powi(2))).sum::<f64>();
    let hf_db = 10. * ((derivative(&wet) / energy(&wet)) / (derivative(&dry) / energy(&dry))).log10();
    let residual_db = 10. * (residual / energy(&dry)).log10();
    let modulation_residual = wet.iter().zip(&modulated).flat_map(|(a,b)|
        (0..2).map(move |c| f64::from(a[c] - b[c]).powi(2))).sum::<f64>();
    let modulation_db = 10. * (modulation_residual / energy(&wet)).log10();
    println!("VISTA_LEGACY_LP dry_rms={:?} wet_rms={:?} normalized_hf_db={hf_db} residual_db={residual_db} modulation_residual_db={modulation_db}",
        reference::levels(&dry, 0., 0.5).rms, reference::levels(&wet, 0., 0.5).rms);
    assert!(energy(&dry) > 1e-8 && energy(&wet) > 1e-8);
    assert!(wet.iter().flatten().all(|v| v.is_finite()));
    assert!(hf_db < 0., "the saved lowpass must darken the damping-release sample");
    assert!(residual_db > -40., "the authored slot must reach audio");
    assert!(modulation_db > -40., "the authored cutoff envelope must reach audio");
}

#[test]
#[ignore = "requires installed Vista violin overlay; run through kontakto-heavy"]
fn vista_legacy_highpass_filters_the_authored_legato_transition() {
    use sampler_ir as ir;
    let path = find("Performance Samples Vista/Instruments/Bonus/Vista - 3 Violins FFF Overlay.nki")
        .expect("installed Vista violin overlay is required for this witness");
    let render = |enabled, modulated| {
        let mut library = sampler_kontakt::read(&path).unwrap();
        assert!(!library.instrument.unsupported.iter().any(|u|
            matches!(u.feature.as_str(), "Filter: filter type" | "effect")),
            "Vista's eight legacy highpass slots must survive translation");
        let hp_chains = library.instrument.chains.iter().enumerate().filter_map(|(i,c)|
            c.pre_amplitude.iter().chain(&c.post_amplitude).any(|p|
                matches!(p, ir::Processor::Filter(ir::Filter { kind: ir::FilterKind::HighPass { poles: 2 }, .. })))
                .then_some(ir::ChainRef(i))).collect::<Vec<_>>();
        assert_eq!(hp_chains.len(), 8, "all eight saved HP slots must survive, including empty groups");
        for chain in &hp_chains {
            assert!(library.instrument.routes.iter().any(|r|
                matches!(r.target, ir::Target::Processor { chain: c, parameter: ir::ProcessorParameter::Cutoff, .. } if c == *chain)
                    && matches!(r.depth, ir::Depth::Pitch(p) if (p.semitones() - 12. * 8.96).abs() < 1e-6)),
                "legacy HP cutoff envelopes must retain v1's knob span");
        }
        let mut zone = library.instrument.zones.iter().find(|z| z.chain.is_some_and(|c| hp_chains.contains(&c))
            && z.keys.low <= 60 && z.keys.high >= 60).expect("populated middle-C legato transition").clone();
        println!("VISTA_LEGACY_HP witness_group={} hp_chains={}", zone.group.unwrap().0, hp_chains.len());
        let chain = &library.instrument.chains[zone.chain.unwrap().0];
        let filters = chain.pre_amplitude.iter().chain(&chain.post_amplitude).filter(|p|
            matches!(p, ir::Processor::Filter(ir::Filter { kind: ir::FilterKind::HighPass { poles: 2 }, .. })))
            .copied().collect::<Vec<_>>();
        assert_eq!(filters.len(), 1);
        zone.routes.retain(|r| modulated && matches!(library.instrument.routes[r.0].target,
            ir::Target::Processor { parameter: ir::ProcessorParameter::Cutoff, .. }));
        let new_chain = ir::ChainRef(library.instrument.chains.len());
        for route in &zone.routes {
            library.instrument.routes[route.0].target = ir::Target::Processor {
                chain: new_chain, index: 0, parameter: ir::ProcessorParameter::Cutoff };
        }
        zone.chain = Some(new_chain);
        library.instrument.chains.push(ir::Chain { scope: ir::Scope::Voice,
            pre_amplitude: if enabled { filters } else { vec![] }, post_amplitude: vec![] });
        w15_render_one_authored_zone(library, zone)
    };
    let dry = render(false, false);
    let wet = render(true, false);
    let modulated = render(true, true);
    let energy = |frames: &[[f32; 2]]| frames.iter().flatten().map(|v| f64::from(*v).powi(2)).sum::<f64>();
    let derivative = |frames: &[[f32; 2]]| frames.windows(2).flat_map(|w|
        (0..2).map(move |c| f64::from(w[1][c] - w[0][c]).powi(2))).sum::<f64>();
    let residual = |a: &[[f32; 2]], b: &[[f32; 2]]| a.iter().zip(b).flat_map(|(a,b)|
        (0..2).map(move |c| f64::from(a[c] - b[c]).powi(2))).sum::<f64>();
    let hf_db = 10. * ((derivative(&wet) / energy(&wet)) / (derivative(&dry) / energy(&dry))).log10();
    let residual_db = 10. * (residual(&dry, &wet) / energy(&dry)).log10();
    let modulation_db = 10. * (residual(&wet, &modulated) / energy(&wet)).log10();
    println!("VISTA_LEGACY_HP dry_rms={:?} wet_rms={:?} normalized_hf_db={hf_db} residual_db={residual_db} modulation_residual_db={modulation_db}",
        reference::levels(&dry, 0., 0.5).rms, reference::levels(&wet, 0., 0.5).rms);
    assert!(energy(&dry) > 1e-8 && energy(&wet) > 1e-8 && energy(&modulated) > 1e-8);
    assert!(wet.iter().chain(&modulated).flatten().all(|v| v.is_finite()));
    assert!(hf_db > 0., "the saved highpass must remove low-frequency energy");
    assert!(residual_db > -40., "the authored HP slot must reach audio");
    assert!(modulation_db > -40., "the authored cutoff envelope must reach audio");
}

#[test]
#[ignore = "requires installed Analog Strings; run through kontakto-heavy"]
fn w15_authored_formant_offline_ab_changes_the_gate_spectrum() {
    use sampler_ir as ir;
    let Some(path) = find("ANALOG STRINGS/Instruments/ANALOG STRINGS.nki") else { return; };
    let render = |enabled| {
        let mut library = sampler_kontakt::read(&path).unwrap();
        let (mut zone, filters) = library.instrument.zones.iter().find_map(|z| {
            let chain = &library.instrument.chains[z.chain?.0];
            let filters: Vec<_> = chain.pre_amplitude.iter().chain(&chain.post_amplitude)
                .filter(|p| matches!(p, ir::Processor::Filter(ir::Filter { kind: ir::FilterKind::Peak { gain: ir::Gain::Decibels(15.) }, .. })))
                .cloned().collect();
            (filters.len() == 3).then(|| (z.clone(), filters))
        }).expect("gate item must execute its three-band Formant I model");
        zone.routes.clear();
        zone.chain = Some(ir::ChainRef(library.instrument.chains.len()));
        library.instrument.chains.push(ir::Chain { scope: ir::Scope::Voice,
            pre_amplitude: if enabled { filters.into_iter().chain([ir::Processor::Gain(ir::Gain::Linear(0.25))]).collect() } else { vec![] },
            post_amplitude: vec![] });
        w15_render_one_authored_zone(library, zone)
    };
    let dry = render(false);
    let wet = render(true);
    let energy = |frames: &[[f32; 2]]| frames.iter().flatten().map(|v| f64::from(*v).powi(2)).sum::<f64>();
    let derivative = |frames: &[[f32; 2]]| frames.windows(2).map(|w| (0..2).map(|c| f64::from(w[1][c]-w[0][c]).powi(2)).sum::<f64>()).sum::<f64>();
    let dry_shape = derivative(&dry) / energy(&dry).max(1e-30);
    let wet_shape = derivative(&wet) / energy(&wet).max(1e-30);
    let shape_delta_db = 10. * (wet_shape / dry_shape).log10();
    println!("W15 formant dry_rms={:?} wet_rms={:?} normalized_hf_delta_db={shape_delta_db}",
        reference::levels(&dry, 0.1, 0.45).rms, reference::levels(&wet, 0.1, 0.45).rms);
    assert!(energy(&dry) > 1e-8 && energy(&wet) > 1e-8);
    assert!(shape_delta_db.is_finite() && shape_delta_db.abs() > 0.05, "Formant must change spectral shape, independent of level");
}

#[test]
#[ignore = "requires installed Analog Strings; run through kontakto-heavy"]
fn w15_authored_lofi_offline_ab_measures_reduction_on_the_gate_sample() {
    use sampler_ir as ir;
    let Some(path) = find("ANALOG STRINGS/Instruments/ANALOG STRINGS.nki") else { return; };
    let render = |enabled| {
        let mut library = sampler_kontakt::read(&path).unwrap();
        let (mut zone, mut effect) = library.instrument.zones.iter().find_map(|z| {
            let chain = &library.instrument.chains[z.chain?.0];
            chain.pre_amplitude.iter().chain(&chain.post_amplitude)
                .find(|p| matches!(p, ir::Processor::LoFi { .. }))
                .map(|p| (z.clone(), p.clone()))
        }).expect("gate item must retain its authored Lo-Fi slot");
        // Saved defaults are pristine; exercise the same authored slot's Bits control.
        let ir::Processor::LoFi { ref mut bits, ref mut frequency, .. } = effect else { unreachable!() };
        *bits = 0.1;
        *frequency = 1.;
        zone.routes.clear();
        zone.chain = Some(ir::ChainRef(library.instrument.chains.len()));
        library.instrument.chains.push(ir::Chain { scope: ir::Scope::Voice,
            pre_amplitude: if enabled { vec![effect] } else { vec![] }, post_amplitude: vec![] });
        w15_render_one_authored_zone(library, zone)
    };
    let dry = render(false);
    let wet = render(true);
    let dry_levels = reference::levels(&dry, 0.1, 0.45);
    let wet_levels = reference::levels(&wet, 0.1, 0.45);
    let energy = |frames: &[[f32; 2]]| frames.iter().flatten().map(|v| f64::from(*v).powi(2)).sum::<f64>();
    let residual = dry.iter().zip(&wet).map(|(a,b)| (0..2).map(|c| f64::from(a[c]-b[c]).powi(2)).sum::<f64>()).sum::<f64>();
    let residual_db = 10. * (residual / energy(&dry).max(1e-30)).log10();
    println!("W15 lofi dry_rms={:?} wet_rms={:?} residual_relative_db={residual_db}", dry_levels.rms, wet_levels.rms);
    assert!(dry_levels.max_peak() > -80.);
    assert!(residual_db.is_finite() && residual_db > -40., "Bits must change the authored gate sample");
    assert!(wet.iter().flatten().all(|v| v.is_finite()));
}

#[test]
#[ignore = "requires installed Conflux and Analog Strings; run through kontakto-heavy"]
fn w15_authored_native_cutoff_offline_ab_reaches_ladder_and_daft() {
    use sampler_ir as ir;
    for ladder in [true, false] {
        let path = find(if ladder { "Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki" }
            else { "ANALOG STRINGS/Instruments/ANALOG STRINGS.nki" }).expect("installed gate item");
        let render = |enabled| {
            let mut library = sampler_kontakt::read(&path).unwrap();
            let (mut zone, route, processor) = library.instrument.zones.iter().find_map(|z| {
                z.routes.iter().find_map(|r| {
                    let route = library.instrument.routes[r.0];
                    let ir::Target::Processor { chain, index, parameter: ir::ProcessorParameter::Cutoff } = route.target else { return None; };
                    let c = &library.instrument.chains[chain.0];
                    let p = *c.pre_amplitude.iter().chain(&c.post_amplitude).nth(index)?;
                    let native = if ladder { matches!(p, ir::Processor::LadderLP4(_)) } else { matches!(p, ir::Processor::Daft(_)) };
                    let source = &library.instrument.modulators[route.source.0].source;
                    let executable = if ladder { matches!(source, ir::ModulationSource::Envelope(_)) }
                        else { matches!(source, ir::ModulationSource::Constant) };
                    (native && executable && matches!(route.depth, ir::Depth::Normalized(_))).then(|| (z.clone(), *r, p))
                })
            }).expect("gate must retain an authored native cutoff route");
            let saved = match processor { ir::Processor::LadderLP4(p) => p.cutoff, ir::Processor::Daft(p) => p.cutoff, _ => unreachable!() };
            let r = &mut library.instrument.routes[route.0];
            r.target = ir::Target::Processor { chain: ir::ChainRef(0), index: 0, parameter: ir::ProcessorParameter::Cutoff };
            // Exercise the authored route's amount; zero saved amounts remain enabled.
            r.depth = ir::Depth::Normalized(if enabled { if saved > 0.6 { -0.25 } else { 0.25 } } else { 0. });
            zone.routes = vec![route];
            zone.chain = Some(ir::ChainRef(library.instrument.chains.len()));
            library.instrument.chains.push(ir::Chain { scope: ir::Scope::Voice,
                pre_amplitude: vec![processor], post_amplitude: vec![] });
            w15_render_one_authored_zone(library, zone)
        };
        let dry = render(false);
        let wet = render(true);
        let energy = |x: &[[f32; 2]]| x.iter().flatten().map(|v| f64::from(*v).powi(2)).sum::<f64>();
        let derivative = |x: &[[f32; 2]]| x.windows(2).flat_map(|w| (0..2).map(move |c| f64::from(w[1][c] - w[0][c]).powi(2))).sum::<f64>();
        let residual = dry.iter().zip(&wet).flat_map(|(a,b)| (0..2).map(move |c| f64::from(a[c] - b[c]).powi(2))).sum::<f64>();
        let residual_db = 10. * (residual / energy(&dry).max(1e-30)).log10();
        let hf_delta_db = 10. * ((derivative(&wet) / energy(&wet)) / (derivative(&dry) / energy(&dry))).log10();
        println!("W15 native_cutoff ladder={ladder} dry_rms={:?} wet_rms={:?} residual_db={residual_db} hf_delta_db={hf_delta_db}",
            reference::levels(&dry, 0.1, 0.45).rms, reference::levels(&wet, 0.1, 0.45).rms);
        assert!(energy(&dry) > 1e-8 && energy(&wet) > 1e-8);
        assert!(wet.iter().flatten().all(|v| v.is_finite()));
        assert!(residual_db > -40., "authored normalized cutoff route must change audio");
    }
}

#[test]
#[ignore = "requires installed Morphology; run through kontakto-heavy"]
fn w15_zero_multi_offline_ab_reaches_the_bipolar_volume_consumer() {
    use sampler_ir as ir;
    let Some(path) = find("Morphology Evolved [Zero-G] rutracker.org/Morphology Evolved.nki") else { return; };
    let render = |enabled| {
        let mut library = sampler_kontakt::read(&path).unwrap();
        let (mut zone, mut route) = library.instrument.zones.iter().find_map(|z| {
            z.routes.iter().find_map(|r| {
                let route = library.instrument.routes[r.0];
                matches!(library.instrument.modulators[route.source.0].source,
                    ir::ModulationSource::Lfo(lfo) if lfo.shape == ir::LfoShape::Zero)
                    .then(|| (z.clone(), route))
            })
        }).expect("the authored zero-wave LFO must have a retained runtime source");
        // Exercise this saved source through the native bipolar volume law.
        route.target = ir::Target::Amplitude; route.depth = ir::Depth::Normalized(1.);
        route.invert = false; route.shape = None; route.scale = None; route.smoothing = ir::Time::ZERO;
        zone.routes = if enabled { vec![ir::RouteRef(library.instrument.routes.len())] } else { vec![] };
        library.instrument.routes.push(route); zone.chain = None;
        w15_render_one_authored_zone(library, zone)
    };
    let dry = render(false); let wet = render(true);
    let energy = |v: &[[f32;2]]| v.iter().flatten().map(|v| f64::from(*v).powi(2)).sum::<f64>();
    let delta_db = 10. * (energy(&wet) / energy(&dry)).log10();
    println!("W15 zero_multi volume_delta_db={delta_db}");
    assert!(energy(&dry) > 1e-8);
    assert!((delta_db - 20. * 0.5f64.log10()).abs() < 1e-5);
    assert!(dry.iter().zip(&wet).all(|(d,w)| (0..2).all(|c| w[c] == d[c] * 0.5)));
}

#[test]
#[ignore = "requires installed gate libraries; run through kontakto-heavy"]
fn w15_authored_native_q_gain_offline_ab_reaches_ladder_and_daft() {
    use sampler_ir as ir;
    for ladder in [true, false] {
      for parameter in [ir::ProcessorParameter::Resonance, ir::ProcessorParameter::Gain] {
        let path = find(if ladder { "Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki" }
            else { "ANALOG STRINGS/Instruments/ANALOG STRINGS.nki" }).expect("installed gate item");
        let render = |enabled| {
            let mut library = sampler_kontakt::read(&path).unwrap();
            let (mut zone, route, processor) = [parameter, ir::ProcessorParameter::Cutoff].into_iter().find_map(|wanted| library.instrument.zones.iter().find_map(|z| {
                z.routes.iter().find_map(|r| {
                    let route = library.instrument.routes[r.0];
                    let ir::Target::Processor { chain, index, parameter: target } = route.target else { return None; };
                    if target != wanted { return None; }
                    let c = &library.instrument.chains[chain.0];
                    let p = *c.pre_amplitude.iter().chain(&c.post_amplitude).nth(index)?;
                    let native = if ladder { matches!(p, ir::Processor::LadderLP4(_)) } else { matches!(p, ir::Processor::Daft(_)) };
                    (native && matches!(route.depth, ir::Depth::Normalized(_))).then(|| (z.clone(), *r, p))
                })
            })).expect("gate must retain an authored native route for this filter");
            let saved = match (processor, parameter) {
                (ir::Processor::LadderLP4(p), ir::ProcessorParameter::Gain) => p.gain,
                (ir::Processor::LadderLP4(p), _) => p.resonance,
                (ir::Processor::Daft(p), ir::ProcessorParameter::Gain) => p.gain,
                (ir::Processor::Daft(p), _) => p.resonance,
                _ => unreachable!(),
            };
            let r = &mut library.instrument.routes[route.0];
            println!("W15 native_q_gain ladder={ladder} exercised={parameter:?} saved_target={:?}", r.target);
            r.target = ir::Target::Processor { chain: ir::ChainRef(0), index: 0, parameter };
            r.depth = ir::Depth::Normalized(if enabled { if saved > 0.6 { -0.4 } else { 0.4 } } else { 0. });
            zone.routes = vec![route];
            zone.chain = Some(ir::ChainRef(library.instrument.chains.len()));
            library.instrument.chains.push(ir::Chain { scope: ir::Scope::Voice, pre_amplitude: vec![processor], post_amplitude: vec![] });
            w15_render_one_authored_zone(library, zone)
        };
        let dry = render(false); let wet = render(true);
        let energy = |x: &[[f32; 2]]| x.iter().flatten().map(|v| f64::from(*v).powi(2)).sum::<f64>();
        let residual = dry.iter().zip(&wet).flat_map(|(a,b)| (0..2).map(move |c| f64::from(a[c] - b[c]).powi(2))).sum::<f64>();
        let level_delta_db = 10. * (energy(&wet) / energy(&dry)).log10();
        let residual_db = 10. * (residual / energy(&dry).max(1e-30)).log10();
        println!("W15 native_q_gain ladder={ladder} parameter={parameter:?} level_delta_db={level_delta_db} residual_db={residual_db}");
        assert!(energy(&dry) > 1e-8 && energy(&wet) > 1e-8);
        assert!(wet.iter().flatten().all(|v| v.is_finite()));
        assert!(residual_db > -40., "native Q/Gain route must affect audio");
      }
    }
}
