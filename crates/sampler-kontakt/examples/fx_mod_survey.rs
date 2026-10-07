//! Metadata-only census. Never emits preset bytes, scripts, samples or access data.
use ni_file::kontakt::{Chunk, StructuredObject, objects::*};
use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{Hash, Hasher},
    path::Path,
};
#[path = "../src/source_parameters.rs"]
#[allow(dead_code)]
mod source_parameters;
use sampler_ir::{SourceParameterRecord, SourceParameterValue};

#[derive(Default)]
struct FieldStats {
    files: BTreeSet<usize>,
    values: BTreeMap<String, (usize, BTreeSet<usize>)>,
}

#[derive(Default)]
struct Survey {
    counts: BTreeMap<String, (usize, BTreeSet<usize>)>,
    fields: BTreeMap<String, FieldStats>,
    file: usize,
}
impl Survey {
    fn count(&mut self, key: impl Into<String>) {
        let e = self.counts.entry(key.into()).or_default();
        e.0 += 1;
        e.1.insert(self.file);
    }
    fn field(&mut self, id: u16, version: u16, name: &str, value: &SourceParameterValue) {
        let signature = match value {
            SourceParameterValue::Number(v) => format!("{v}"),
            SourceParameterValue::Integer(v) => format!("{v}"),
            SourceParameterValue::Boolean(v) => format!("{v}"),
            SourceParameterValue::Text(v) => {
                if v.is_empty() || v == "<none>" {
                    "empty".into()
                } else {
                    "set".into()
                }
            }
            SourceParameterValue::Numbers(v) => {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                for n in v {
                    n.to_bits().hash(&mut h);
                }
                format!("count:{} fingerprint:{:016x}", v.len(), h.finish())
            }
            SourceParameterValue::Opaque(v) => {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                v.hash(&mut h);
                format!("bytes:{} fingerprint:{:016x}", v.len(), h.finish())
            }
            SourceParameterValue::Records(records) => {
                for fields in records {
                    let target =
                        fields.iter().find(|f| f.name == "parameter").and_then(|f| {
                            match &f.value {
                                SourceParameterValue::Text(p) => Some(p.as_str()),
                                _ => None,
                            }
                        });
                    for field in fields {
                        self.field(
                            id,
                            version,
                            &format!(
                                "{name}{}.{}",
                                target.map_or(String::new(), |t| format!(".{t}")),
                                field.name
                            ),
                            &field.value,
                        );
                    }
                }
                format!("records:{}", records.len())
            }
        };
        let stats = self
            .fields
            .entry(format!("0x{id:02x}\t0x{version:x}\t{name}"))
            .or_default();
        stats.files.insert(self.file);
        let value = stats.values.entry(signature).or_default();
        value.0 += 1;
        value.1.insert(self.file);
    }
    fn records(&mut self, records: &[SourceParameterRecord]) {
        for record in records {
            for field in &record.fields {
                self.field(record.object_id, record.version, field.name, &field.value);
            }
        }
    }
    fn object(&mut self, kind: &str, id: u16, object: &StructuredObject) {
        self.count(format!(
            "{kind}\tid=0x{id:02x}\tv=0x{:x}\tprivate={}\tpublic={}",
            object.version,
            object.private_data.len(),
            object.public_data.len()
        ));
    }
    fn targets(&mut self, kind: &str, targets: &[ModTarget]) {
        for target in targets {
            self.count(format!(
                "target\t{kind}\t{}\tslot={}\tsigned={}\tinvert={}\tsmooth={}\tshaper={}",
                target.param,
                target.slot.is_some(),
                target.unknown_flags & 2 != 0,
                target.invert,
                target.lag_ms != 0,
                match &target.shaper {
                    None => "none",
                    Some(s) => match (&s.curve, s.enabled) {
                        (ShaperCurve::Table(_), true) => "table:on",
                        (ShaperCurve::Table(_), false) => "table:off",
                        (_, true) => "points:on",
                        (_, false) => "points:off",
                    },
                }
            ));
        }
    }
    fn rack(&mut self, scope: &str, array: &BParamArrayBParFX8) {
        self.count(format!("rack\t{scope}\tv=0x{:x}", array.version));
        match source_parameters::rack(scope, array) {
            Ok(records) => self.records(&records),
            Err(_) => self.count("error\tfx-field-decode"),
        }
        for chunk in array.items.iter().flatten() {
            let Ok(fx) = BParFX::try_from(chunk) else {
                self.count("error\tfx-wrapper");
                continue;
            };
            self.object("wrapper", 0x25, &fx.0);
            if fx.params().is_err() {
                self.count("error\tfx-state");
            }
            let Some(inner) = fx.effect() else {
                self.count("error\tmissing-effect");
                continue;
            };
            let Ok(o) = StructuredObject::try_from(inner) else {
                self.count("error\tfx-object");
                continue;
            };
            self.object(&format!("fx:{scope}"), inner.id, &o);
            match EffectParameters::read(inner.id, o.version, &o.public_data) {
                Ok(Some(_)) => self.count(format!(
                    "fx-decoded\tid=0x{:02x}\tv=0x{:x}",
                    inner.id, o.version
                )),
                Ok(None) => self.count(format!(
                    "fx-opaque\tid=0x{:02x}\tv=0x{:x}",
                    inner.id, o.version
                )),
                Err(_) => self.count(format!(
                    "fx-layout-mismatch\tid=0x{:02x}\tv=0x{:x}\tpublic={}",
                    inner.id,
                    o.version,
                    o.public_data.len()
                )),
            }
            if inner.id == 0x18 && o.public_data.len() >= 4 {
                let subtype = i32::from_le_bytes(o.public_data[..4].try_into().unwrap());
                self.count(format!(
                    "filter\ttype={subtype}\tv=0x{:x}\tpublic={}",
                    o.version,
                    o.public_data.len()
                ));
            }
        }
    }
    fn program(&mut self, program: &Program) {
        self.count(format!("program\tv=0x{:x}", program.0.version));
        self.children(&program.0.children);
    }
    fn children(&mut self, children: &[Chunk]) {
        let mut rack = 0;
        for c in children {
            match c.id {
                0x3a => {
                    let scope = ["insert", "send", "main"]
                        .get(rack)
                        .copied()
                        .unwrap_or("extra");
                    rack += 1;
                    match BParamArrayBParFX8::try_from(c) {
                        Ok(a) => self.rack(scope, &a),
                        Err(_) => self.count("error\tprogram-rack"),
                    }
                }
                0x45 => match InsertBus::try_from(c) {
                    Ok(bus) => {
                        self.object("bus", c.id, &bus.0);
                        if let Ok(p) = bus.params() {
                            self.field(
                                c.id,
                                bus.0.version,
                                "name",
                                &SourceParameterValue::Text(p.name),
                            );
                            self.field(
                                c.id,
                                bus.0.version,
                                "volume_linear",
                                &SourceParameterValue::Number(f64::from(p.volume)),
                            );
                            self.field(
                                c.id,
                                bus.0.version,
                                "pan",
                                &SourceParameterValue::Number(f64::from(p.pan)),
                            );
                            self.field(
                                c.id,
                                bus.0.version,
                                "output",
                                &SourceParameterValue::Integer(i64::from(p.output)),
                            );
                        } else {
                            self.count("error\tbus-state");
                        }
                        if let Some(c) = bus.0.find_first(0x3a) {
                            match BParamArrayBParFX8::try_from(c) {
                                Ok(a) => self.rack("bus", &a),
                                Err(_) => self.count("error\tbus-rack"),
                            }
                        }
                    }
                    Err(_) => self.count("error\tbus"),
                },
                0x33 => match GroupList::try_from(c) {
                    Ok(groups) => {
                        for g in groups.groups {
                            self.count(format!("group\tv=0x{:x}", g.0.version));
                            match g.insert_fx() {
                                Ok(a) => self.rack("group", &a),
                                Err(_) => self.count("error\tgroup-rack"),
                            }
                            for c in &g.0.children {
                                self.mod_array(c);
                            }
                        }
                    }
                    Err(_) => self.count("error\tgroups"),
                },
                _ => {}
            }
        }
    }
    fn mod_array(&mut self, c: &Chunk) {
        if !matches!(c.id, 0x3b | 0x3c) {
            return;
        }
        let Ok(o) = StructuredObject::try_from(c) else {
            self.count("error\tmod-array-object");
            return;
        };
        self.object("mod-array", c.id, &o);
        let n = if c.id == 0x3b {
            16
        } else if o.version == 0x13 && o.public_data.len() >= 4 {
            u32::from_le_bytes(o.public_data[..4].try_into().unwrap()) as usize
        } else {
            32
        };
        let Ok(slots) = read_param_slots(&o, n) else {
            self.count("error\tmod-array-slots");
            return;
        };
        for (_, inner) in slots {
            let Ok(m) = StructuredObject::try_from(&inner) else {
                self.count("error\tmod-object");
                continue;
            };
            self.object("mod", inner.id, &m);
            if c.id == 0x3b {
                let source = m.children.first().and_then(|c| {
                    if c.id == 7 {
                        StructuredObject::try_from(c)
                            .ok()
                            .and_then(|o| o.children.into_iter().next())
                    } else {
                        Some(Chunk {
                            id: c.id,
                            data: c.data.clone(),
                        })
                    }
                });
                if let Some(source) = source {
                    if let Ok(o) = StructuredObject::try_from(&source) {
                        self.object("internal-source", source.id, &o);
                    }
                }
                match InternalMod::try_from(&inner).and_then(|m| m.params()) {
                    Ok(p) => {
                        if let Ok(m) = InternalMod::try_from(&inner) {
                            match source_parameters::internal("", &m, &p) {
                                Ok(records) => self.records(&records),
                                Err(_) => self.count("error\tinternal-fields"),
                            }
                        }
                        self.targets("internal", &p.targets);
                        self.count(format!("internal-flags\t{:?}", p.unknown_flags));
                    }
                    Err(_) => self.count("error\tinternal-params"),
                }
            } else {
                match ExternalMod::try_from(&inner).and_then(|m| m.params()) {
                    Ok(p) => {
                        if let Ok(m) = ExternalMod::try_from(&inner) {
                            self.records(&[source_parameters::external("", &m, &p)]);
                        }
                        self.count(format!("external-source\t{:?}", p.source));
                        self.targets("external", &p.targets);
                    }
                    Err(_) => self.count("error\texternal-params"),
                }
            }
        }
    }
    fn chunk(&mut self, c: &Chunk) {
        match c.id {
            0x4f => match Snapshot::try_from(c) {
                Ok(snapshot) => {
                    self.count(format!("snapshot\tv=0x{:x}", snapshot.version));
                    self.children(&snapshot.effect_children);
                    match snapshot.group_snapshots() {
                        Ok(groups) => {
                            for (_, group) in groups {
                                self.count(format!("snapshot-group\tv=0x{:x}", group.version));
                                self.rack("snapshot-group", &group.fx);
                                match group.modulation_chunks() {
                                    Ok(chunks) => {
                                        for chunk in chunks {
                                            self.mod_array(&chunk);
                                        }
                                    }
                                    Err(_) => self.count("error\tsnapshot-modulation"),
                                }
                            }
                        }
                        Err(_) => self.count("error\tsnapshot-groups"),
                    }
                }
                Err(_) => self.count("error\tsnapshot"),
            },
            0x28 => match Program::try_from(c) {
                Ok(p) => self.program(&p),
                Err(_) => self.count("error\tprogram"),
            },
            0x36 => match ProgramList::try_from(c) {
                Ok(list) => {
                    for p in list.programs {
                        self.program(&p);
                    }
                }
                Err(_) => self.count("error\tprogram-list"),
            },
            0x03 | 0x29 => {
                if let Ok(o) = StructuredObject::try_from(c) {
                    for c in &o.children {
                        self.chunk(c);
                    }
                    if c.id == 3 {
                        if let Ok(bank) = Bank::try_from(c) {
                            if let Ok(slots) = bank.slot_list() {
                                for (_, container) in slots.slots {
                                    if let Ok(list) = container.program_list() {
                                        for p in list.programs {
                                            self.program(&p);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
impl Survey {
    fn report(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        for (key, (count, files)) in &self.counts {
            writeln!(out, "COUNT\t{count}\t{}\t{key}", files.len()).unwrap();
        }
        for (key, stats) in &self.fields {
            for (value, (count, files)) in &stats.values {
                writeln!(out, "VALUE\t{count}\t{}\t{key}\t{value}", files.len()).unwrap();
            }
        }
        out
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let manifest = args.get(1).expect("items.tsv [start] [limit] [cache-dir]");
    let start: usize = args.get(2).map_or(0, |s| s.parse().unwrap());
    let limit: usize = args.get(3).map_or(usize::MAX, |s| s.parse().unwrap());
    let cache = args.get(4).map(Path::new);
    if let Some(cache) = cache {
        std::fs::create_dir_all(cache).unwrap();
    }
    let began = std::time::Instant::now();
    for (i, line) in std::fs::read_to_string(manifest)
        .unwrap()
        .lines()
        .enumerate()
        .skip(start)
        .take(limit)
    {
        let Some((kind, path)) = line.split_once('\t') else {
            continue;
        };
        if !kind.starts_with("kontakt") {
            continue;
        }
        let output = cache.map(|c| c.join(format!("{i:04}.tsv")));
        if output.as_ref().is_some_and(|p| p.exists()) {
            continue;
        }
        // Leave time for the current item, then release the heavy slot between shards.
        if began.elapsed().as_secs() >= 240 {
            break;
        }
        let mut survey = Survey {
            file: i,
            ..Survey::default()
        };
        survey.count("file\tlisted");
        match sampler_kontakt::read_chunks(Path::new(path)) {
            Ok(chunks) => {
                survey.count("file\tread");
                for c in &chunks.0 {
                    survey.chunk(c);
                }
            }
            Err(_) => survey.count("error\tcontainer"),
        }
        let report = survey.report();
        if let Some(output) = output {
            let temporary = output.with_extension("tmp");
            std::fs::write(&temporary, report).unwrap();
            std::fs::rename(temporary, output).unwrap();
        } else {
            print!("{report}");
        }
        eprintln!("survey item {i} complete");
    }
}
