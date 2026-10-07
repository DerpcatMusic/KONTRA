//! A real keyswitched Kontakt instrument driven by CC 32 instead of its keys.
//! Skips unless `KONTRA_KONTAKT_LIBRARIES` holds the library.
use sampler_core::{Limits, Runtime};
use sampler_ir as ir;
use sampler_midi::{Articulator, Ingress, Intercept, Packets, Version};

#[path = "support/reference.rs"]
mod reference;

const INSTRUMENT: &str =
    "Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/2 Horns KS.nki";
const KEY: u8 = 60;

fn load(driver: ir::Driver) -> Option<sampler_kontakt::Loaded> {
    let root = std::env::var_os("KONTRA_KONTAKT_LIBRARIES")?;
    let path = std::env::split_paths(&root)
        .map(|r| r.join(INSTRUMENT))
        .find(|p| p.is_file())?;
    Some(load_at(&path, KEY, driver))
}

/// Load only the zones under `key`, re-driven by `driver` (keys swallowed).
fn load_at(path: &std::path::Path, key: u8, driver: ir::Driver) -> sampler_kontakt::Loaded {
    decoded(path, key).drive(driver)
}

/// One instrument's zones under `key`, decoded once (in memory) and re-lowered
/// per driver; `Pcm` clones share the samples.
struct Decoded {
    instrument: ir::Instrument,
    pcm: Vec<sampler_core::Pcm>,
    labels: Vec<String>,
    options: sampler_kontakt::Options,
}

fn decoded(path: &std::path::Path, key: u8) -> Decoded {
    let mut kontakt = sampler_kontakt::read(path).unwrap();
    let mut instrument = kontakt.instrument;
    let kept = instrument.retain_zones(|z| z.keys.low <= key && z.keys.high >= key);
    let pcm = kept
        .iter()
        .map(|&a| {
            let decoded = kontakt.samples.decode(&kontakt.locations[a]).unwrap();
            sampler_core::Pcm::new(decoded.rate, decoded.frames.into_boxed_slice()).unwrap()
        })
        .collect();
    Decoded {
        instrument,
        pcm,
        labels: kept.iter().map(|a| a.to_string()).collect(),
        options: sampler_kontakt::Options {
            library: Some(path.to_owned()),
            ..reference::options(key..=key)
        },
    }
}

impl Decoded {
    fn drive(&self, driver: ir::Driver) -> sampler_kontakt::Loaded {
        let mut instrument = self.instrument.clone();
        instrument.switching.driver = driver;
        instrument.switching.keys = ir::SwitchKeys::Swallow;
        sampler_kontakt::finish(
            instrument,
            self.pcm.clone(),
            self.labels.clone(),
            &self.options,
        )
        .unwrap()
    }
}

/// Render `words` (one per 64-frame step) through the articulator and ingress.
fn render(loaded: sampler_kontakt::Loaded, words: &[u32]) -> Vec<[f32; 2]> {
    let plan = loaded.plan;
    reference::assert_matched(&plan);
    let limits = Limits {
        notes: 64,
        channels: 16,
        performances: 1,
        expressions: 64,
        families: 64,
        decisions: 256,
        voices: 512,
        commands: 256,
        behaviors: 16,
        behavior_fuel: 1 << 20,
        behavior_cells: plan.behavior_local_count().saturating_mul(16),
        note_cells: plan.note_cell_count().saturating_mul(64),
    };
    let mut rt = Runtime::new(plan, limits).unwrap();
    let mut groups = [None; 16];
    groups[0] = Some(Version::Midi1);
    let mut ingress = Ingress::new(0, groups);
    let mut articulator = Articulator::new(&rt, rt.performance(0).unwrap(), 0).unwrap();
    let mut out = vec![[0.0; 2]; 64 * words.len() + 24000];
    for (i, &word) in words.iter().enumerate() {
        let words = [word];
        let packet = Packets::new(&words).next().unwrap().unwrap();
        if articulator.intercept(&mut rt, packet).unwrap() == Intercept::Forward {
            let _ = ingress.apply(&mut rt, packet);
        }
        rt.render(&mut out[i * 64..(i + 1) * 64]).unwrap();
    }
    let tail = 64 * words.len();
    rt.render(&mut out[tail..]).unwrap();
    out
}

#[test]
fn afflatus_cc_selects_what_its_keyswitch_selects() {
    let note = 0x0090_0000 | u32::from(KEY) << 8 | 100;
    let Some(keys) = load(ir::Driver::Keys) else {
        eprintln!("skipped: {INSTRUMENT} is not installed");
        return;
    };
    let failed: Vec<_> = keys
        .instrument
        .unsupported
        .iter()
        .filter(|u| u.feature == "script")
        .collect();
    assert!(failed.is_empty(), "switching script must bind: {failed:?}");
    // Marcato is articulation 2: key 26, or CC 32 value 2.
    let by_key = render(keys, &[0x2090_1a64, 0x2080_1a00, 0x2000_0000 | note]);
    let by_cc = render(
        load(ir::Driver::Controller).unwrap(),
        &[0x20b0_2002, 0x2000_0000, 0x2000_0000 | note],
    );
    let default = render(
        load(ir::Driver::Controller).unwrap(),
        &[0x2000_0000, 0x2000_0000, 0x2000_0000 | note],
    );
    let peak = |out: &[[f32; 2]]| out.iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
    assert!(peak(&by_cc) > 0.01, "audible: {}", peak(&by_cc));
    assert_eq!(by_cc, by_key, "CC 32 = 2 plays what key 26 selects");
    assert_ne!(by_cc, default, "and differs from the default articulation");
}

/// Every non-key driver, through the real script, plays the same audio as the
/// articulation's own keyswitch: velocity splits, channels, CC 32 and programs.
#[test]
fn afflatus_every_driver_plays_what_its_keyswitch_plays() {
    let Some(root) = std::env::var_os("KONTRA_KONTAKT_LIBRARIES") else {
        return;
    };
    let Some(path) = std::env::split_paths(&root)
        .map(|r| r.join(INSTRUMENT))
        .find(|p| p.is_file())
    else {
        return;
    };
    let decoded = decoded(&path, KEY);
    let articulations = decoded.instrument.articulations.clone();
    let peak = |out: &[[f32; 2]]| out.iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
    let mut distinct = std::collections::HashSet::new();
    for (i, a) in articulations.iter().enumerate() {
        let tap = u32::from(a.switch_keys[0]);
        let alt = a.alternatives;
        let velocity = u32::from(alt.velocities.unwrap().low);
        let on = |channel: u32, velocity: u32| {
            0x2090_0000 | channel << 16 | u32::from(KEY) << 8 | velocity
        };
        // The reference: this articulation's keyswitch, then the same note.
        let reference = render(
            decoded.drive(ir::Driver::Keys),
            &[
                0x2090_0064 | tap << 8,
                0x2080_0000 | tap << 8,
                on(0, velocity),
            ],
        );
        assert!(peak(&reference) > 1e-4, "{} is silent", a.name);
        distinct.insert(
            reference
                .iter()
                .map(|f| f[0].to_bits())
                .fold(0u64, |h, b| h.wrapping_mul(0x100_0000_01b3) ^ u64::from(b)),
        );
        let cc = alt.controller.unwrap();
        for (driver, words) in [
            (
                ir::Driver::Velocity,
                vec![0x2000_0000, 0x2000_0000, on(0, velocity)],
            ),
            (
                ir::Driver::Channel,
                vec![
                    0x2000_0000,
                    0x2000_0000,
                    on(u32::from(alt.channel.unwrap()), velocity),
                ],
            ),
            (
                ir::Driver::Controller,
                vec![
                    0x20b0_0000 | u32::from(cc.controller) << 8 | u32::from(cc.low),
                    0x2000_0000,
                    on(0, velocity),
                ],
            ),
            (
                ir::Driver::Program,
                vec![
                    0x20c0_0000 | u32::from(alt.program.unwrap()) << 8,
                    0x2000_0000,
                    on(0, velocity),
                ],
            ),
        ] {
            assert_eq!(
                render(decoded.drive(driver), &words),
                reference,
                "{driver:?} articulation {i} {}",
                a.name
            );
        }
    }
    assert!(
        distinct.len() > 1,
        "articulations must differ to prove anything"
    );
}

fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
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

/// Every generated switch map under the libraries named by
/// `KONTRA_ARTICULATION_LIBRARIES` (substrings of library folder names, default
/// Audio Imperia, Areia and Pacific): its script binds, and each articulation's
/// CC selects exactly what its key selects. Slow: decodes samples at one key.
#[test]
#[ignore]
fn generated_maps_drive_like_their_keyswitches() {
    let Some(roots) = std::env::var_os("KONTRA_KONTAKT_LIBRARIES") else {
        return;
    };
    let names = std::env::var("KONTRA_ARTICULATION_LIBRARIES")
        .unwrap_or_else(|_| "Audio Imperia,Areia,Pacific".into());
    let mut paths = Vec::new();
    for root in std::env::split_paths(&roots) {
        for library in std::fs::read_dir(&root).into_iter().flatten().flatten() {
            let name = library.file_name().to_string_lossy().into_owned();
            if names.split(',').any(|n| name.contains(n)) {
                collect(&library.path(), &mut paths);
            }
        }
    }
    paths.sort();
    // `KONTRA_ARTICULATION_SHARD=i/n` takes every n-th instrument from i, so a
    // crash costs one shard.
    let shard = std::env::var("KONTRA_ARTICULATION_SHARD")
        .ok()
        .and_then(|s| {
            let (i, n) = s.split_once('/')?;
            Some((i.parse::<usize>().ok()?, n.parse::<usize>().ok()?))
        });
    if let Some((i, n)) = shard {
        paths = paths.into_iter().skip(i).step_by(n).collect();
    }
    let (mut maps, mut failures) = (0, Vec::new());
    for path in &paths {
        let Ok(read) = sampler_kontakt::read(path) else {
            continue;
        };
        let ir = read.instrument;
        if ir.articulations.is_empty() {
            continue;
        }
        maps += 1;
        // The key most zones cover, outside the switch keys.
        let switch: Vec<u8> = ir
            .articulations
            .iter()
            .flat_map(|a| a.switch_keys.clone())
            .collect();
        let key = (0..=127u8)
            .filter(|k| !switch.contains(k))
            .max_by_key(|&k| {
                let count = ir
                    .zones
                    .iter()
                    .filter(|z| (z.keys.low..=z.keys.high).contains(&k))
                    .count();
                // Ties go to the key nearest middle C.
                (count, std::cmp::Reverse(k.abs_diff(60)))
            })
            .unwrap();
        let decoded = decoded(path, key);
        let keys = decoded.drive(ir::Driver::Keys);
        let failed: Vec<_> = keys
            .instrument
            .unsupported
            .iter()
            .filter(|u| u.feature == "script")
            .map(|u| u.value.clone())
            .collect();
        if !failed.is_empty() {
            failures.push(format!("{}: script {failed:?}", path.display()));
            continue;
        }
        drop(keys);
        let note = 0x2090_0000 | u32::from(key) << 8 | 100;
        let (mut same, mut distinct) = (0, 0);
        let default = render(
            decoded.drive(ir::Driver::Controller),
            &[0x2000_0000, 0x2000_0000, note],
        );
        let peak = |out: &[[f32; 2]]| out.iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
        if peak(&default) < 1e-4 {
            // Nothing to compare: a silent default makes every map vacuous.
            failures.push(format!("{}: silent at key {key}", path.display()));
            continue;
        }
        for a in &ir.articulations {
            let (Some(&tap), Some(cc)) = (a.switch_keys.first(), a.alternatives.controller) else {
                failures.push(format!("{}: {} has no key or CC", path.display(), a.name));
                continue;
            };
            let by_key = render(
                decoded.drive(ir::Driver::Keys),
                &[
                    0x2090_0064 | u32::from(tap) << 8,
                    0x2080_0000 | u32::from(tap) << 8,
                    note,
                ],
            );
            let by_cc = render(
                decoded.drive(ir::Driver::Controller),
                &[
                    0x20b0_0000 | u32::from(cc.controller) << 8 | u32::from(cc.low),
                    0x2000_0000,
                    note,
                ],
            );
            if by_cc != by_key {
                failures.push(format!(
                    "{}: {} CC differs from key {tap}",
                    path.display(),
                    a.name
                ));
            } else if by_cc == default {
                same += 1;
            } else {
                distinct += 1;
            }
        }
        eprintln!(
            "{} key {key}: {} articulations, {distinct} differ from default, {same} equal it",
            path.display(),
            ir.articulations.len()
        );
    }
    eprintln!("{maps} maps, {} failures", failures.len());
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Diagnostic: `KONTRA_PROBE=<nki>` plays its first free key and prints what
/// the runtime did, to tell a silent patch from a silent harness.
#[test]
#[ignore]
fn probe_instrument() {
    let Some(path) = std::env::var_os("KONTRA_PROBE") else {
        return;
    };
    let path = std::path::Path::new(&path);
    let ir = sampler_kontakt::read(path).unwrap().instrument;
    let switch: Vec<u8> = ir
        .articulations
        .iter()
        .flat_map(|a| a.switch_keys.clone())
        .collect();
    let key = std::env::var("KONTRA_PROBE_KEY")
        .ok()
        .and_then(|k| k.parse().ok())
        .unwrap_or(60u8);
    eprintln!(
        "articulations {} switch keys {switch:?} zones {}",
        ir.articulations.len(),
        ir.zones.len()
    );
    let mut d = decoded(path, key);
    if std::env::var_os("KONTRA_PROBE_NOSCRIPT").is_some() {
        d.instrument.behaviors.clear();
    }
    eprintln!(
        "DYNAMICS amplitude controllers {:?}",
        d.instrument.amplitude_controllers()
    );
    for (i, g) in d.instrument.groups.iter().enumerate() {
        eprintln!(
            "GROUPNAME {i} {:?} gain {:?} out {:?}",
            g.name, g.gain, g.output
        );
    }
    eprintln!("BUSES {:?}", d.instrument.buses);
    for b in &d.instrument.buses {
        if let Some(c) = b.chain {
            eprintln!("BUSCHAIN {:?}", d.instrument.chains[c.0]);
        }
    }
    eprintln!(
        "zones at {key}: {} behaviors {}",
        d.instrument.zones.len(),
        d.instrument.behaviors.len()
    );
    let z = &d.instrument.zones;
    let vel = z
        .iter()
        .filter(|z| (z.velocities.low..=z.velocities.high).contains(&100))
        .count();
    let mut conds = std::collections::BTreeMap::<String, usize>::new();
    for z in z {
        *conds
            .entry(format!("{:?} {:?}", z.trigger, z.conditions))
            .or_default() += 1;
    }
    if let Ok(pat) = std::env::var("KONTRA_PROBE_GREP") {
        for b in &d.instrument.behaviors {
            let lines: Vec<&str> = b.source.lines().collect();
            for (i, l) in lines.iter().enumerate() {
                if pat.split(',').any(|p| l.contains(p))
                    || pat.split(';').any(|r| {
                        r.split_once("..").is_some_and(|(a, b)| {
                            a.parse::<usize>().is_ok_and(|a| {
                                b.parse::<usize>().is_ok_and(|b| (a..b).contains(&i))
                            })
                        })
                    })
                {
                    eprintln!("SRC {i}: {l}");
                }
            }
        }
    }
    for b in &d.instrument.behaviors {
        eprintln!(
            "STATE {} entries: {:?}",
            b.state.len(),
            &b.state[..b.state.len().min(40)]
        );
    }
    if std::env::var_os("KONTRA_PROBE_FULL").is_some() {
        // Whole instrument, scripts on, power-on controllers: L pk / R pk /
        // mono pk (peak 0.3-3 s) and sqrt((L2+R2)/2) RMS (0.5-2 s), dBFS.
        let vel: u8 = std::env::var("KONTRA_PROBE_VEL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100);
        let loaded = sampler_kontakt::finish(
            d.instrument.clone(),
            d.pcm.clone(),
            d.labels.clone(),
            &d.options,
        )
        .unwrap();
        let mut words = vec![
            0x2000_0000,
            0x2090_0000 | u32::from(key) << 8 | u32::from(vel),
        ];
        words.resize(1500, 0x2000_0000);
        let out = render(loaded, &words);
        let sr = 48000;
        let db = |x: f64| 20.0 * (x + 1e-12).log10();
        let pk = |f: &dyn Fn(&[f32; 2]) -> f32| {
            out[sr * 3 / 10..(3 * sr).min(out.len())]
                .iter()
                .fold(0f32, |p, x| p.max(f(x).abs()))
        };
        let seg = &out[sr / 2..2 * sr];
        let rms = (seg
            .iter()
            .map(|f| f64::from(f[0]).powi(2) + f64::from(f[1]).powi(2))
            .sum::<f64>()
            / (2.0 * seg.len() as f64))
            .sqrt();
        eprintln!(
            "FULL key {key} vel {vel}: L {:.1} R {:.1} max {:.1} mono {:.1} rms {:.1}",
            db(f64::from(pk(&|f| f[0]))),
            db(f64::from(pk(&|f| f[1]))),
            db(f64::from(pk(&|f| f[0].max(f[1])))),
            db(f64::from(pk(&|f| (f[0] + f[1]) / 2.0))),
            db(rms)
        );
    }
    if std::env::var_os("KONTRA_PROBE_GAINS").is_some() {
        let vel: u8 = std::env::var("KONTRA_PROBE_VEL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100);
        let ir = &d.instrument;
        eprintln!(
            "GAIN instrument gain? name {} voice_limit {:?}",
            ir.name, ir.voice_limit
        );
        for (i, z) in ir
            .zones
            .iter()
            .enumerate()
            .filter(|(_, z)| (z.velocities.low..=z.velocities.high).contains(&vel))
        {
            let g = z.group.map(|g| &ir.groups[g.0]);
            for r in &z.routes {
                let r = &ir.routes[r.0];
                eprintln!(
                    "GROUTE zone {i} {:?} <- {:?} depth {:?} invert {} shape {:?} smooth {:?}",
                    r.target,
                    ir.modulators[r.source.0].source,
                    r.depth,
                    r.invert,
                    r.shape.map(|s| &ir.shapes[s.0]),
                    r.smoothing
                );
            }
            eprintln!(
                "GAIN zone {i} grp {:?} zone gain {:?} pan {:?} group gain {:?} group pan {:?} vel {:?} keys {:?}-{:?} trig {:?}",
                z.group,
                z.gain,
                z.pan,
                g.map(|g| g.gain),
                g.map(|g| g.pan),
                z.velocity,
                z.keys.low,
                z.keys.high,
                z.trigger
            );
        }
    }
    if std::env::var_os("KONTRA_PROBE_SOLO").is_some() {
        let vel: u8 = std::env::var("KONTRA_PROBE_VEL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100);
        let mut groups: Vec<_> = d
            .instrument
            .zones
            .iter()
            .filter_map(|z| z.group)
            .map(|g| g.0)
            .collect();
        groups.sort();
        groups.dedup();
        for g in groups {
            let mut ir = d.instrument.clone();
            ir.behaviors.clear();
            ir.switching.driver = ir::Driver::Keys;
            let kept = ir.retain_zones(|z| {
                z.group.is_some_and(|x| x.0 == g)
                    && (z.velocities.low..=z.velocities.high).contains(&vel)
                    && z.trigger == ir::Trigger::Attack
            });
            if ir.zones.is_empty() {
                continue;
            }
            let pcm: Vec<_> = kept.iter().map(|&a| d.pcm[a].clone()).collect();
            let labels: Vec<_> = kept.iter().map(|&a| d.labels[a].clone()).collect();
            let zones = ir.zones.len();
            let loaded = sampler_kontakt::finish(ir, pcm, labels, &d.options).unwrap();
            let mut words = vec![0x2000_0000];
            // KONTRA_PROBE_CC="100=64;11=127": controllers sent before the note.
            for cc in std::env::var("KONTRA_PROBE_CC")
                .unwrap_or_default()
                .split(';')
            {
                if let Some((n, v)) = cc.split_once('=')
                    && let (Ok(n), Ok(v)) = (n.parse::<u32>(), v.parse::<u32>())
                {
                    words.push(0x20B0_0000 | n << 8 | v);
                }
            }
            words.push(0x2090_0000 | u32::from(key) << 8 | u32::from(vel));
            words.resize(1500, 0x2000_0000);
            let out = render(loaded, &words);
            let whole = reference::levels(&out, 0.0, 3.0);
            let tail = reference::levels(&out, 0.5, 2.0);
            eprintln!(
                "SOLO group {g} zones {zones} peak {:.1} rms {:.1} L {:.1} R {:.1} rmsL {:.1} rmsR {:.1}",
                whole.max_peak(),
                (tail.rms[0] + tail.rms[1]) / 2.0,
                whole.peak[0],
                whole.peak[1],
                tail.rms[0],
                tail.rms[1]
            );
        }
    }
    let mut tags = std::collections::BTreeMap::<String, usize>::new();
    for z in z
        .iter()
        .filter(|z| (z.velocities.low..=z.velocities.high).contains(&100))
    {
        *tags
            .entry(format!("{:?} grp {:?}", z.articulation, z.group))
            .or_default() += 1;
    }
    eprintln!(
        "velocity-100 articulation tags: {tags:?} owner {:?} default {:?}",
        d.instrument.switching,
        d.instrument
            .articulations
            .iter()
            .map(|a| (a.name.clone(), a.default))
            .collect::<Vec<_>>()
    );
    eprintln!("velocity-100 zones {vel}; trigger/conditions: {conds:?}");
    let loaded = d.drive(ir::Driver::Controller);
    let mut feats = std::collections::BTreeMap::<String, usize>::new();
    for u in &loaded.instrument.unsupported {
        if u.feature != "script" {
            *feats
                .entry(format!("{:?} {}", u.reason, u.feature))
                .or_default() += 1;
        }
    }
    let mut warns = std::collections::BTreeMap::<String, usize>::new();
    for u in &loaded.instrument.unsupported {
        if u.feature.starts_with("script") {
            *warns
                .entry(format!("{} {}", u.feature, u.value))
                .or_default() += 1;
        }
    }
    for (w, n) in warns.iter().take(60) {
        eprintln!("WARN {n} {w}");
    }
    eprintln!("non-script reports: {feats:?}");
    eprintln!(
        "zones after finish: {} pcm {}",
        loaded.instrument.zones.len(),
        d.pcm.len()
    );
    let plan = loaded.plan;
    let limits = Limits {
        notes: 64,
        channels: 16,
        performances: 1,
        expressions: 64,
        families: 64,
        decisions: 256,
        voices: 512,
        commands: 256,
        behaviors: 16,
        behavior_fuel: 1 << 20,
        behavior_cells: plan.behavior_local_count().saturating_mul(16),
        note_cells: plan.note_cell_count().saturating_mul(64),
    };
    let mut rt = Runtime::new(plan, limits).unwrap();
    let mut groups = [None; 16];
    groups[0] = Some(Version::Midi1);
    let mut ingress = Ingress::new(0, groups);
    let mut out = vec![[0.0f32; 2]; 4800];
    rt.render(&mut out).unwrap();
    let words = [0x2090_0064 | u32::from(key) << 8];
    let r = ingress.apply(&mut rt, Packets::new(&words).next().unwrap().unwrap());
    eprintln!("apply {r:?}");
    for block in 0..10 {
        rt.render(&mut out).unwrap();
        let peak = out.iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
        eprintln!(
            "block {block}: voices {} notes {} peak {peak}",
            rt.voice_count(),
            rt.note_count()
        );
    }
}

/// Kontakt 8 reference levels (KONTAKT_REFERENCE.md s.13): scripts on, nothing
/// sent, 3 s held; max(|L|, |R|) peak over 0.3-3 s in dBFS.
/// Library, key, velocity, Kontakt's peak in dBFS and the controllers its
/// recording sent explicitly (CC, value) before the note.
const REFERENCE: &[(&str, u8, u8, f64, &[(u8, u8)])] = &[
    (
        "Performance Samples Vista/Instruments/Vista - 3 Cellos.nki",
        48,
        100,
        -38.6,
        &[],
    ),
    (
        "Una Corda Library/Instruments/Una Corda Cotton.nki",
        60,
        64,
        -24.7,
        &[],
    ),
    (
        "Una Corda Library/Instruments/Una Corda Cotton.nki",
        60,
        100,
        -14.7,
        &[],
    ),
    (
        "Una Corda Library/Instruments/Una Corda Cotton.nki",
        60,
        127,
        -8.6,
        &[],
    ),
    (
        "Afflatus Chapter II Brass/Instruments/3. Curated Ensembles/Barbarian Brass.nki",
        55,
        100,
        -15.9,
        &[],
    ),
];

/// Every reference note within 1 dB of Kontakt. Known gaps while it fails: Una
/// Cotton at vel 100/127 (KONTRA 4.6 dB low), Barbarian (11.4 dB low:
/// the recording sent no CC1 or CC11 (KONTAKT_REFERENCE.md s.13), so Kontakt's
/// script supplies its own CC1 default, read as about 48 with the script
/// bypassed; KONTRA's scripts-on path starts it at 0). Vista is +0.9 dB.
#[test]
#[ignore = "known gaps: Una Cotton 5/11/11 dB low at vel 64/100/127, Barbarian 11 dB low; script-driven, Vista passes"]
fn full_notes_match_kontakt_within_a_decibel() {
    let Some(root) = std::env::var_os("KONTRA_KONTAKT_LIBRARIES") else {
        return;
    };
    let mut off = Vec::new();
    for &(relative, key, vel, kontakt, controllers) in REFERENCE {
        let path = std::path::Path::new(&root).join(relative);
        if !path.exists() {
            continue;
        }
        let d = decoded(&path, key);
        let loaded = sampler_kontakt::finish(
            d.instrument.clone(),
            d.pcm.clone(),
            d.labels.clone(),
            &d.options,
        )
        .unwrap();
        let mut words = vec![0x2000_0000];
        words.extend(reference::controller_words(controllers));
        words.push(0x2090_0000 | u32::from(key) << 8 | u32::from(vel));
        words.resize(1500, 0x2000_0000);
        let out = render(loaded, &words);
        let db = reference::levels(&out, 0.0, 3.0).max_peak();
        if (db - kontakt).abs() >= 1.0 {
            off.push(format!(
                "{relative} key {key} vel {vel}: {db:.1} against {kontakt}"
            ));
        }
    }
    assert!(off.is_empty(), "{off:#?}");
}

#[test]
#[ignore = "probe"]
fn una_velocity_sweep_probe() {
    let Some(root) = std::env::var_os("KONTRA_KONTAKT_LIBRARIES") else {
        return;
    };
    let path =
        std::path::Path::new(&root).join("Una Corda Library/Instruments/Una Corda Cotton.nki");
    for vel in [40u32, 64, 72, 80, 88, 94, 100, 110, 127] {
        let d = decoded(&path, 60);
        let loaded = sampler_kontakt::finish(
            d.instrument.clone(),
            d.pcm.clone(),
            d.labels.clone(),
            &d.options,
        )
        .unwrap();
        let mut words = vec![0x2000_0000, 0x2090_0000 | 60 << 8 | vel];
        words.resize(1500, 0x2000_0000);
        let out = render(loaded, &words);
        eprintln!(
            "PROBE vel {vel}: {:.1}",
            reference::levels(&out, 0.3, 3.0).max_peak()
        );
    }
}

#[test]
#[ignore = "probe"]
fn barbarian_cc1_sweep_probe() {
    let Some(root) = std::env::var_os("KONTRA_KONTAKT_LIBRARIES") else {
        return;
    };
    let path = std::path::Path::new(&root)
        .join("Afflatus Chapter II Brass/Instruments/3. Curated Ensembles/Barbarian Brass.nki");
    for cc1 in [None, Some(0u32), Some(48), Some(127)] {
        let d = decoded(&path, 55);
        if cc1.is_none() {
            for u in &d.instrument.unsupported {
                eprintln!("PROBE unsupported {u:?}");
            }
        }
        let loaded = sampler_kontakt::finish(
            d.instrument.clone(),
            d.pcm.clone(),
            d.labels.clone(),
            &d.options,
        )
        .unwrap();
        let mut words = vec![0x2000_0000];
        if let Some(v) = cc1 {
            words.push(0x20B0_0000 | 1 << 8 | v);
        }
        words.push(0x2090_0000 | 55 << 8 | 100);
        words.resize(1500, 0x2000_0000);
        let out = render(loaded, &words);
        eprintln!(
            "PROBE cc1 {cc1:?}: {:.1}",
            reference::levels(&out, 0.3, 3.0).max_peak()
        );
    }
}

/// Runtime trace of a full note: every selection with its group verdicts and
/// every effect the scripts emitted (set_engine_par and friends, by name).
#[test]
#[ignore = "probe"]
fn full_note_runtime_trace_probe() {
    let Some(root) = std::env::var_os("KONTRA_KONTAKT_LIBRARIES") else {
        return;
    };
    // KONTRA_TRACE="relative path|key" traces one instrument instead of the table.
    let one = std::env::var("KONTRA_TRACE").ok().map(|t| {
        let (path, key) = t.rsplit_once('|').unwrap();
        (path.to_owned(), key.parse::<u8>().unwrap())
    });
    let list: Vec<(String, u8, u8)> = match one {
        Some((p, k)) => vec![(p, k, 100)],
        // vel is read below
        None => REFERENCE
            .iter()
            .filter(|r| !r.0.contains("Vista"))
            .map(|r| (r.0.to_owned(), r.1, r.2))
            .collect(),
    };
    for (relative, key, vel) in list.iter().map(|(r, k, v)| (r.as_str(), *k, *v)) {
        if vel != 100 {
            continue;
        }
        let vel = std::env::var("KONTRA_TRACE_VEL").ok().map_or(vel, |v| v.parse().unwrap());
        let path = std::path::Path::new(&root).join(relative);
        if !path.exists() {
            continue;
        }
        let d = decoded(&path, key);
        let loaded =
            sampler_kontakt::finish(d.instrument.clone(), d.pcm.clone(), d.labels.clone(), &d.options)
                .unwrap();
        let names: Vec<String> = loaded.instrument.groups.iter().map(|g| g.name.clone()).collect();
        if let Some(z) = std::env::var("KONTRA_TRACE_ZONE").ok().and_then(|v| v.parse::<usize>().ok()) {
            let ir = &loaded.instrument;
            for r in &ir.zones[z].routes {
                let route = &ir.routes[r.0];
                println!("ZONE {z} route {:?} src {:?} shape {:?}", route, ir.modulators[route.source.0].source, route.shape.map(|s| ir.shapes[s.0].points.clone()));
            }
        }
        if std::env::var_os("KONTRA_TRACE_UNS").is_some() {
            let mut seen: std::collections::BTreeMap<String, usize> = Default::default();
            for u in &loaded.instrument.unsupported {
                *seen.entry(format!("{} | {:.90}", u.feature, u.value)).or_default() += 1;
            }
            for (k, n) in seen {
                println!("UNS {n} {k}");
            }
        }
        let scripts = loaded.scripts.clone();
        let plan = loaded.plan;
        let limits = Limits {
            notes: 64,
            channels: 16,
            performances: 1,
            expressions: 64,
            families: 64,
            decisions: 256,
            voices: 512,
            commands: 256,
            behaviors: 16,
            behavior_fuel: 1 << 20,
            behavior_cells: plan.behavior_local_count().saturating_mul(16),
            note_cells: plan.note_cell_count().saturating_mul(64),
        };
        let mut rt = Runtime::new(plan, limits).unwrap();
        rt.record_selections(true);
        rt.record_script_writes(true);
        let mut groups = [None; 16];
        groups[0] = Some(Version::Midi1);
        let mut ingress = Ingress::new(0, groups);
        let mut articulator = Articulator::new(&rt, rt.performance(0).unwrap(), 0).unwrap();
        let mut out = vec![[0.0; 2]; 64];
        println!("=== {relative} key {key} vel {vel}");
        let mut words = vec![0x2000_0000, 0x2090_0000 | u32::from(key) << 8 | u32::from(vel)];
        words.resize(1500, 0x2000_0000);
        if let Some(off) = std::env::var("KONTRA_TRACE_OFF").ok().and_then(|v| v.parse::<usize>().ok()) {
            words[off] = 0x2080_0000 | u32::from(key) << 8;
        }
        let trace_steps: usize = std::env::var("KONTRA_TRACE_STEPS").ok().map_or(200, |v| v.parse().unwrap());
        let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
        for (i, &word) in words.iter().enumerate() {
            let w = [word];
            let packet = Packets::new(&w).next().unwrap().unwrap();
            if articulator.intercept(&mut rt, packet).unwrap() == Intercept::Forward {
                let _ = ingress.apply(&mut rt, packet);
            }
            rt.render(&mut out).unwrap();
            let peak = out.iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
            if i % 50 == 0 {
                println!("step {i} peak {:.1} dB voices {}", 20.0 * f64::from(peak).log10(), rt.voice_count());
            }
            for r in rt.take_selection_records() {
                let sounded: Vec<String> = r
                    .candidates
                    .iter()
                    .filter(|c| c.rejected.is_none())
                    .map(|c| {
                        let g = c.group.map_or("?".into(), |g| {
                            format!("{g}:{}", names.get(g as usize).map_or("", |n| n.as_str()))
                        });
                        format!("{g} z{} {:?}", c.region, loaded.instrument.zones[c.region].velocities)
                    })
                    .collect();
                if let Ok(pat) = std::env::var("KONTRA_TRACE_REJ") {
                    let mut by: std::collections::BTreeMap<String, usize> = Default::default();
                    for c in r.candidates.iter().filter(|c| c.rejected.is_some()) {
                        let g = c.group.map_or("?".into(), |g| names.get(g as usize).cloned().unwrap_or_default());
                        if g.contains(&pat) {
                            *by.entry(format!("{g} {:?}", c.rejected)).or_default() += 1;
                        }
                    }
                    for (k, n) in by.iter().take(40) {
                        println!("step {i} REJ {n} {k}");
                    }
                }
                println!(
                    "step {i} SEL key {} vel {} {:?} suppressed {} candidates {} sounded {} {:?}",
                    r.key,
                    r.velocity,
                    r.trigger,
                    r.suppressed,
                    r.candidates.len(),
                    sounded.len(),
                    sounded.iter().take(60).collect::<Vec<_>>()
                );
            }
            for w in rt.take_script_writes() {
                if i < trace_steps {
                    println!("step {i} WRITE {w}");
                }
            }
            rt.drain_effects(|e| {
                let view = e.instance.and_then(|id| scripts.get(usize::from(id.0)));
                let name = view.and_then(|v| v.service(e.service)).unwrap_or("?");
                let args = e.args();
                let shown = match (name, view) {
                    ("set_engine_par", Some(v)) => {
                        let par = args.first().and_then(|&a| v.symbol(a as i32)).unwrap_or_default();
                        format!("{par} {:?}", &args[1.min(args.len())..])
                    }
                    _ => format!("{args:?}"),
                };
                let line = format!("{name} {shown}");
                if i < trace_steps {
                    println!("step {i} EFFECT {line}");
                }
                *counts.entry(name.to_string()).or_default() += 1;
                true
            });
        }
        println!("effect totals {counts:?} dropped {}", rt.dropped_effects());
    }
}

/// Una Cotton key 60 vel 100: the script-played note against the same group
/// played natively with scripts off (peak dBFS over 0.1 s windows).
#[test]
#[ignore = "probe"]
fn una_script_vs_native_probe() {
    let Some(root) = std::env::var_os("KONTRA_KONTAKT_LIBRARIES") else {
        return;
    };
    let path = std::path::Path::new(&root).join("Una Corda Library/Instruments/Una Corda Cotton.nki");
    let windows = |out: &[[f32; 2]]| {
        (0..8)
            .map(|w| {
                let seg = &out[w * 4800..((w + 1) * 4800).min(out.len())];
                let p = seg.iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
                format!("{:.1}", 20.0 * f64::from(p).log10())
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut words = vec![0x2000_0000, 0x2090_0000 | 60 << 8 | 100];
    words.resize(1500, 0x2000_0000);
    for scripts in [true, false] {
        let mut d = decoded(&path, 60);
        if !scripts {
            let keep: Vec<_> = d
                .instrument
                .groups
                .iter()
                .enumerate()
                .filter(|(_, g)| g.name == "DRY_C3")
                .map(|(i, _)| i)
                .collect();
            println!("native groups {keep:?}");
            let kept = d.instrument.retain_zones(|z| z.group.is_some_and(|g| keep.contains(&g.0)));
            d.pcm = kept.iter().map(|&i| d.pcm[i].clone()).collect();
            d.labels = kept.iter().map(|&i| d.labels[i].clone()).collect();
            d.options.scripts = false;
        }
        let loaded = sampler_kontakt::finish(
            d.instrument.clone(),
            d.pcm.clone(),
            d.labels.clone(),
            &d.options,
        )
        .unwrap();
        let out = render(loaded, &words);
        println!("scripts {scripts}: {}", windows(&out));
    }
}

/// Una Cotton group solos played natively (scripts off) against
/// KONTAKT_REFERENCE.md s.19a: g39 v64/100/127 L pk -26.5/-17.3/-9.5, R pk
/// -24.0/-14.4/-6.8, rms L -54.5/-45.3/-39.7, R -51.6/-42.4/-37.2; g94 flat L -14.7 R -15.8.
#[test]
#[ignore = "probe"]
fn una_solo_probe() {
    let Some(root) = std::env::var_os("KONTRA_KONTAKT_LIBRARIES") else {
        return;
    };
    let path = std::path::Path::new(&root).join("Una Corda Library/Instruments/Una Corda Cotton.nki");
    for name in ["DRY_C3", "RESONANCE f"] {
        for vel in [64u32, 100, 127] {
            let mut d = decoded(&path, 60);
            let keep: Vec<_> = d
                .instrument
                .groups
                .iter()
                .enumerate()
                .filter(|(_, g)| g.name == name)
                .map(|(i, _)| i)
                .collect();
            let kept = d.instrument.retain_zones(|z| z.group.is_some_and(|g| keep.contains(&g.0)));
            d.pcm = kept.iter().map(|&i| d.pcm[i].clone()).collect();
            d.labels = kept.iter().map(|&i| d.labels[i].clone()).collect();
            d.options.scripts = false;
            let zones = d.instrument.zones.len();
            let loaded = sampler_kontakt::finish(d.instrument.clone(), d.pcm.clone(), d.labels.clone(), &d.options).unwrap();
            let mut words = vec![0x2000_0000, 0x2090_0000 | 60 << 8 | vel];
            words.resize(3000, 0x2000_0000);
            let out = render(loaded, &words);
            let l = reference::levels(&out, 0.0, 3.0);
            let r = reference::levels(&out, 0.5, 2.0);
            let wins: Vec<String> = (0..12).map(|w| { let seg = &out[w * 4800..(w + 1) * 4800]; let p = seg.iter().flatten().fold(0f32, |p, x| p.max(x.abs())); format!("{:.0}", 20.0 * f64::from(p).log10()) }).collect();
            eprintln!("PROBE windows {}", wins.join(" "));
            eprintln!("PROBE {name} zones {zones} vel {vel}: pk {:.1}/{:.1} rms {:.1}/{:.1}", l.peak[0], l.peak[1], r.rms[0], r.rms[1]);
        }
    }
}
