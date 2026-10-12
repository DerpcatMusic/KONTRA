//! Aggregate DSP identities only; never writes decoded presets, scripts or samples.
//! Run through kontakto-heavy: items.tsv CACHE_DIR START END. Each shard stops after 240 s.
//! Cache contains per-item aggregate counts only; no decoded objects or paths.
use ni_file::kontakt::{StructuredObject, objects::*};
use sampler_ir as ir;
use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{Hash, Hasher},
    path::Path,
    time::{Duration, Instant},
};

#[derive(Default)]
struct Census(BTreeMap<(String, String), (usize, BTreeSet<usize>)>);
impl Census {
    fn add(&mut self, category: &str, identity: impl Into<String>, file: usize) {
        let row = self
            .0
            .entry((category.into(), identity.into()))
            .or_default();
        row.0 += 1;
        row.1.insert(file);
    }
    fn rack(&mut self, array: BParamArrayBParFX8, file: usize) {
        for c in array.items.iter().flatten() {
            let Ok(fx) = BParFX::try_from(c) else {
                self.add("error", "fx", file);
                continue;
            };
            let (Ok(p), Some(e)) = (fx.params(), fx.effect()) else {
                self.add("error", "fx state", file);
                continue;
            };
            let category = if p.bypass {
                "slot_bypassed"
            } else {
                "slot_enabled"
            };
            let mut identity = format!("0x{:02x}", e.id);
            if e.id == 0x18 {
                if let Ok(o) = StructuredObject::try_from(e) {
                    if let Some(b) = o.public_data.get(..4) {
                        identity = format!("Filter:{}", u32::from_le_bytes(b.try_into().unwrap()));
                    }
                }
            }
            self.add(category, identity, file);
        }
    }
    fn program(&mut self, p: &Program, file: usize) {
        for c in &p.0.children {
            if c.id == 0x3a {
                if let Ok(a) = BParamArrayBParFX8::try_from(c) {
                    self.rack(a, file);
                }
            } else if c.id == 0x45 {
                if let Ok(b) = InsertBus::try_from(c) {
                    if let Some(c) = b.0.find_first(0x3a) {
                        if let Ok(a) = BParamArrayBParFX8::try_from(c) {
                            self.rack(a, file);
                        }
                    }
                }
            }
        }
        let Some(c) = p.0.find_first(0x33) else {
            self.add("error", "groups absent", file);
            return;
        };
        let Ok(gs) = GroupList::try_from(c) else {
            self.add("error", "groups", file);
            return;
        };
        for g in &gs.groups {
            let Ok(params) = g.params() else {
                self.add("error", "group params", file);
                continue;
            };
            self.add(
                "group",
                if params.muted { "muted" } else { "enabled" },
                file,
            );
            if params.muted {
                continue;
            }
            match g.source_identity() {
                Ok(s) => self.add("source_mode", s.mode.to_string(), file),
                Err(_) => self.add("error", "source identity", file),
            }
            self.add("group_key_tracking", params.key_tracking.to_string(), file);
            self.add("group_gain_units", "linear amplitude", file);
            if let Ok(a) = g.insert_fx() {
                self.rack(a, file);
            }
            if let Some(c) = g.0.find_first(0x3b) {
                if let Ok(a) = InternalModArray16::try_from(c) {
                    if let Ok(ms) = a.slots() {
                        for (_, m) in ms {
                            let Ok(p) = m.params() else {
                                self.add("error", "internal mod", file);
                                continue;
                            };
                            let enabled = p.unknown_flags[1] == 0 && !p.targets.is_empty();
                            let id = match &p.modulator {
                                Modulator::Ahdsr(e) => {
                                    if enabled {
                                        self.add(
                                            "attack_curve_saved",
                                            e.attack_curve.to_string(),
                                            file,
                                        );
                                    }
                                    "AHDSR".into()
                                }
                                Modulator::Lfo(l) => format!("LFO:{}", l.waveform),
                                Modulator::Flex(_) => "Flex".into(),
                                Modulator::Other { chunk_id } => format!("Other:{chunk_id:#x}"),
                            };
                            self.add(
                                if enabled {
                                    "internal_enabled"
                                } else {
                                    "internal_disabled"
                                },
                                id,
                                file,
                            );
                        }
                    }
                }
            }
            if let Some(c) = g.0.find_first(0x3c) {
                if let Ok(a) = ExternalModArray32::try_from(c) {
                    if let Ok(ms) = a.slots() {
                        for (_, m) in ms {
                            let Ok(p) = m.params() else {
                                self.add("error", "external mod", file);
                                continue;
                            };
                            self.add("external_saved", format!("{:?}", p.source), file);
                            for t in p.targets {
                                if matches!(p.source, ModSource::Velocity | ModSource::KeyPosition)
                                {
                                    self.add("performance_route", format!("{:?} -> {} depth={} invert={} lag={} flags={:#x} shaper={}", p.source, t.param, t.intensity, t.invert, t.lag_ms, t.unknown_flags, t.shaper.as_ref().is_some_and(|s| s.enabled)), file);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    fn ir(&mut self, i: &ir::Instrument, file: usize) {
        for g in &i.groups {
            self.add("ir_pan_law", format!("{:?}", g.pan.law), file);
        }
        for z in &i.zones {
            self.add("ir_velocity", format!("{:?}", z.velocity), file);
            self.add(
                "ir_key_tracking",
                match z.pitch {
                    ir::KeyTracking::Fixed => "None",
                    _ => "tracked",
                },
                file,
            );
        }
        for m in &i.modulators {
            let name = format!("{:?}", m.source);
            self.add(
                "ir_modulator",
                name.split(['(', '{']).next().unwrap().trim(),
                file,
            );
            if let ir::ModulationSource::Envelope(e) = &m.source {
                for (phase, c) in [
                    ("attack", e.attack_shape),
                    ("decay", e.decay_shape),
                    ("release", e.release_shape),
                ] {
                    self.add("ir_envelope_curve", format!("{phase}:{c:?}"), file);
                }
            }
        }
        for c in &i.chains {
            for p in c.pre_amplitude.iter().chain(&c.post_amplitude) {
                self.add(
                    "ir_processor",
                    format!("{:?}", p).split(['(', '{']).next().unwrap().trim(),
                    file,
                );
            }
        }
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let list = std::fs::read_to_string(args.get(1).expect("items.tsv")).unwrap();
    let cache = Path::new(args.get(2).expect("CACHE_DIR"));
    let start: usize = args.get(3).expect("START").parse().unwrap();
    let end: usize = args.get(4).expect("END").parse().unwrap();
    std::fs::create_dir_all(cache).unwrap();
    let started = Instant::now();
    let mut check = Census::default();
    check.add("x", "y", 1);
    check.add("x", "y", 1);
    check.add("x", "y", 2);
    assert_eq!(check.0[&("x".into(), "y".into())].0, 3);
    assert_eq!(check.0[&("x".into(), "y".into())].1.len(), 2);
    let mut total = Census::default();
    for (file, line) in list.lines().enumerate() {
        let Some((kind, path)) = line.split_once('\t') else {
            continue;
        };
        if !matches!(kind, "kontakt" | "kontakt-multi") {
            continue;
        }
        let path = Path::new(path);
        let mut key = std::collections::hash_map::DefaultHasher::new();
        line.hash(&mut key);
        if let Ok(m) = path.metadata() {
            m.len().hash(&mut key);
            m.modified().ok().hash(&mut key);
        }
        let header = format!("item-v1:{:016x}", key.finish());
        let result = cache.join(format!("{file}.tsv"));
        let saved = std::fs::read_to_string(&result).ok();
        if let Some(saved) = saved.filter(|s| s.lines().next() == Some(header.as_str())) {
            for row in saved.lines().skip(1) {
                let fields: Vec<_> = row.split('\t').collect();
                assert_eq!(fields.len(), 3);
                let entry = total
                    .0
                    .entry((fields[0].into(), fields[1].into()))
                    .or_default();
                entry.0 += fields[2].parse::<usize>().unwrap();
                entry.1.insert(file);
            }
            continue;
        }
        if file < start || file >= end {
            continue;
        }
        if started.elapsed() >= Duration::from_secs(240) {
            break;
        }
        let mut census = Census::default();
        census.add("files", kind, file);
        if kind == "kontakt-multi" {
            match sampler_kontakt::read_multi(path) {
                Ok(m) => {
                    for (index, (_, p)) in m.programs.iter().enumerate() {
                        census.add("programs", "parsed", file);
                        census.program(p, file);
                        match sampler_kontakt::read_program(path, index) {
                            Ok(k) => {
                                census.add("programs", "IR", file);
                                census.ir(&k.instrument, file);
                            }
                            Err(_) => census.add("error", "IR multi", file),
                        }
                    }
                }
                Err(_) => census.add("error", "multi read", file),
            }
        } else {
            match sampler_kontakt::read_chunks(path)
                .ok()
                .and_then(|cs| cs.find_first(0x28).and_then(|c| Program::try_from(c).ok()))
            {
                Some(p) => {
                    census.add("programs", "parsed", file);
                    census.program(&p, file);
                }
                None => census.add("error", "program read", file),
            }
            match sampler_kontakt::read(path) {
                Ok(k) => {
                    census.add("programs", "IR", file);
                    census.ir(&k.instrument, file);
                }
                Err(_) => census.add("error", "IR read", file),
            }
        }
        let mut saved = format!("{header}\n");
        for ((category, identity), (objects, _)) in &census.0 {
            assert!(!identity.contains(['\t', '\n']));
            saved.push_str(&format!("{category}\t{identity}\t{objects}\n"));
            let entry = total
                .0
                .entry((category.clone(), identity.clone()))
                .or_default();
            entry.0 += objects;
            entry.1.insert(file);
        }
        let temporary = result.with_extension("tmp");
        std::fs::write(&temporary, saved).unwrap();
        std::fs::rename(temporary, result).unwrap();
        eprintln!("cached item {file}");
    }
    let mut rows: Vec<_> = total.0.into_iter().collect();
    rows.sort_by(|a, b| {
        a.0.0
            .cmp(&b.0.0)
            .then(b.1.1.len().cmp(&a.1.1.len()))
            .then(a.0.1.cmp(&b.0.1))
    });
    println!("category\tidentity\tobjects\tfiles");
    for ((category, identity), (objects, files)) in rows {
        println!("{category}\t{identity}\t{objects}\t{}", files.len());
    }
}
