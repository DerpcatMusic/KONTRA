//! Explicit, source-free initialization clocks and bounded VM safepoint samples.
mod lookups;

use mlua::{Lua, Value as LuaValue};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub(crate) struct Profile(Option<Rc<Data>>);

struct Data {
    started: Instant,
    stages: RefCell<BTreeMap<&'static str, (u64, Duration)>>,
    next_sample: Cell<Instant>,
    samples: RefCell<BTreeMap<(String, Option<usize>, Option<String>, String), (u64, u64)>>,
    sample_count: Cell<u64>,
    dropped: Cell<u64>,
    dropped_self: Cell<u64>,
    unavailable_top: Cell<u64>,
    sampling_ns: Cell<u128>,
    trusted_chunks: RefCell<BTreeSet<String>>,
    dropped_chunk_names: Cell<u64>,
    lookups: RefCell<lookups::Lookups>,
    lookup_collection_ns: Cell<u128>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampled_chunk_names_require_bounded_explicit_registration() {
        for trusted in [false, true] {
            let lua = Lua::new();
            let profile = Profile::new(true);
            if trusted {
                profile.register_chunk("Scripts/fixture.lua");
            }
            let p = profile.clone();
            lua.globals()
                .set(
                    "probe",
                    lua.create_function(move |lua, ()| {
                        p.sample(lua, 0);
                        Ok(())
                    })
                    .unwrap(),
                )
                .unwrap();
            lua.load("probe()")
                .set_name("Scripts/fixture.lua")
                .exec()
                .unwrap();
            let report = profile.report(false);
            let frames = report["lua_samples"].as_array().unwrap();
            assert!(frames.iter().any(|frame| frame["source"]
                == if trusted {
                    "Scripts/fixture.lua"
                } else {
                    "<dynamic>"
                }));
        }
        let profile = Profile::new(true);
        profile.register_chunk("private\nbody");
        profile.register_chunk(&"x".repeat(241));
        for index in 0..513 {
            profile.register_chunk(&format!("trusted-{index}"));
        }
        let report = profile.report(false);
        assert_eq!(report["trusted_chunk_count"], 512);
        assert_eq!(report["dropped_chunk_names"], 3);
        assert!(!report.to_string().contains("private"));
    }

    #[test]
    fn profiled_lookup_reads_current_fields_and_keeps_first_match_and_parse_rules() {
        let lua = Lua::new();
        let profile = Profile::new(true);
        let fields = RefCell::new(vec![vec![
            ("Freq".into(), "100".into()),
            ("Freq".into(), "999".into()),
            ("Bypass".into(), "TRUE".into()),
        ]]);
        assert!(matches!(
            profile
                .parameter(&lua, "OnePole", "Freq", &fields, 0)
                .unwrap(),
            LuaValue::Number(100.)
        ));
        assert!(matches!(
            profile
                .parameter(&lua, "OnePole", "Bypass", &fields, 0)
                .unwrap(),
            LuaValue::Boolean(false)
        ));
        fields.borrow_mut()[0] = vec![("Freq".into(), "200".into())];
        assert!(matches!(
            profile
                .parameter(&lua, "OnePole", "Freq", &fields, 0)
                .unwrap(),
            LuaValue::Number(200.)
        ));
        fields.borrow_mut()[0].clear();
        assert!(matches!(
            profile
                .parameter(&lua, "OnePole", "Freq", &fields, 0)
                .unwrap(),
            LuaValue::Number(1000.)
        ));
        assert!(matches!(
            profile
                .parameter(&lua, "OnePole", "Bypass", &fields, 0)
                .unwrap(),
            LuaValue::Boolean(false)
        ));
        assert!(matches!(
            profile
                .parameter(&lua, "PRIVATE_KIND", "Absent", &fields, 9)
                .unwrap(),
            LuaValue::Nil
        ));
        let report = profile.report(false);
        assert!(!report.to_string().contains("PRIVATE_KIND"));
        let kinds = report["parameter_lookups"]["kinds"].as_array().unwrap();
        let one = kinds.iter().find(|kind| kind["kind"] == "OnePole").unwrap();
        assert_eq!(one["retained_fields"]["comparisons"], 5);
        assert_eq!(one["retained_fields"]["hits"], 3);
    }

    #[test]
    fn profiled_text_allocation_failure_stays_an_error_and_releases_field_borrow() {
        let lua = Lua::new();
        let profile = Profile::new(true);
        let fields = RefCell::new(vec![vec![(
            "PrivateField".into(),
            "private-value".repeat(100000),
        )]]);
        lua.set_memory_limit(lua.used_memory() + 1024).unwrap();
        assert!(matches!(
            profile.parameter(&lua, "OnePole", "PrivateField", &fields, 0),
            Err(mlua::Error::MemoryError(_))
        ));
        assert!(fields.try_borrow_mut().is_ok());
        let report = profile.report(false);
        assert_eq!(
            report["parameter_lookups"]["kinds"][0]["allocation_failures"],
            1
        );
        assert!(!report.to_string().contains("private-value"));
    }
}

pub(crate) struct Span(Option<(Rc<Data>, &'static str, Instant)>);
impl Drop for Span {
    fn drop(&mut self) {
        if let Some((data, name, started)) = &self.0 {
            let mut stages = data.stages.borrow_mut();
            let record = stages.entry(name).or_default();
            record.0 += 1;
            record.1 += started.elapsed();
        }
    }
}

impl Profile {
    pub(crate) fn new(enabled: bool) -> Self {
        Self(enabled.then(|| {
            Rc::new(Data {
                started: Instant::now(),
                stages: RefCell::new(BTreeMap::new()),
                next_sample: Cell::new(Instant::now()),
                samples: RefCell::new(BTreeMap::new()),
                sample_count: Cell::new(0),
                dropped: Cell::new(0),
                dropped_self: Cell::new(0),
                unavailable_top: Cell::new(0),
                sampling_ns: Cell::new(0),
                trusted_chunks: RefCell::new(BTreeSet::new()),
                dropped_chunk_names: Cell::new(0),
                lookups: RefCell::new(lookups::Lookups::default()),
                lookup_collection_ns: Cell::new(0),
            })
        }))
    }
    pub(crate) fn enabled(&self) -> bool {
        self.0.is_some()
    }
    /// Register an explicit compiler name from the host or resource authority.
    /// Never register names extracted from a debug frame or a script body.
    pub(crate) fn register_chunk(&self, name: &str) {
        let Some(data) = &self.0 else {
            return;
        };
        let mut names = data.trusted_chunks.borrow_mut();
        if names.contains(name) {
            return;
        }
        if name.is_empty()
            || name.chars().take(241).count() > 240
            || name.chars().any(char::is_control)
            || names.len() >= 512
        {
            data.dropped_chunk_names
                .set(data.dropped_chunk_names.get() + 1);
            return;
        }
        names.insert(name.into());
    }
    pub(crate) fn step_envelope_level(&self) {
        if let Some(data) = &self.0 {
            let started = Instant::now();
            data.lookups.borrow_mut().step_envelope_level();
            data.lookup_collection_ns
                .set(data.lookup_collection_ns.get() + started.elapsed().as_nanos());
        }
    }
    /// Diagnostic equivalent of native.param's ordinary live linear lookup.
    /// It deliberately keeps the same clone/parse/default behavior; no values
    /// or mutable field lists are cached.
    pub(crate) fn parameter(
        &self,
        lua: &Lua,
        kind: &str,
        name: &str,
        params: &RefCell<Vec<Vec<(String, String)>>>,
        id: usize,
    ) -> mlua::Result<LuaValue> {
        let definitions = super::parameters::definitions(kind);
        let definition = definitions.iter().enumerate().find(|(_, p)| p.name == name);
        // Release the live field borrow before any Lua string allocation, as
        // the ordinary path does. Even diagnostic collection must not extend
        // a mutable-state borrow across an allocation/possible GC boundary.
        let (found, retained_len, retained_hit) = {
            let params = params.borrow();
            let fields = params.get(id).map_or(&[][..], Vec::as_slice);
            let saved = fields.iter().enumerate().find(|(_, (key, _))| key == name);
            (
                saved.map(|(_, (_, value))| value.clone()),
                fields.len(),
                saved.map(|(index, _)| index),
            )
        };
        let clone_bytes = found.as_ref().map_or(0, String::len);
        let (value, class) = match found {
            None => match definition {
                Some((_, p)) if p.kind == "boolean" => {
                    (Ok(LuaValue::Boolean(p.default != 0.)), "default_boolean")
                }
                Some((_, p)) => (Ok(LuaValue::Number(p.default)), "default_number"),
                None => (Ok(LuaValue::Nil), "missing"),
            },
            Some(value) if definition.is_some_and(|(_, p)| p.kind == "boolean") => (
                Ok(LuaValue::Boolean(value == "1" || value == "true")),
                "saved_boolean",
            ),
            Some(value) => match value.parse::<f64>() {
                Ok(n) => (Ok(LuaValue::Number(n)), "saved_number"),
                Err(_) => (
                    lua.create_string(&value).map(LuaValue::String),
                    "saved_text",
                ),
            },
        };
        if let Some(data) = &self.0 {
            let started = Instant::now();
            data.lookups.borrow_mut().observe(lookups::Observation {
                kind: if definitions.is_empty() {
                    "<uncatalogued>"
                } else {
                    kind
                },
                schema_len: definitions.len(),
                schema_hit: definition.map(|(index, _)| index),
                retained_len,
                retained_hit,
                value_class: class,
                clone_bytes,
                allocation_failed: value.is_err(),
            });
            data.lookup_collection_ns
                .set(data.lookup_collection_ns.get() + started.elapsed().as_nanos());
        }
        value
    }
    pub(crate) fn span(&self, name: &'static str) -> Span {
        Span(
            self.0
                .as_ref()
                .map(|data| (Rc::clone(data), name, Instant::now())),
        )
    }
    pub(crate) fn record(&self, name: &'static str, elapsed_ms: f64) {
        if let Some(data) = &self.0 {
            let mut stages = data.stages.borrow_mut();
            let record = stages.entry(name).or_default();
            record.0 += 1;
            record.1 += Duration::from_secs_f64(elapsed_ms / 1000.);
        }
    }
    pub(crate) fn sample(&self, lua: &Lua, checkpoints: u64) {
        let Some(data) = &self.0 else {
            return;
        };
        // Clock/stack inspection occurs at most every 256 VM checkpoints, then
        // at most once per 2ms. The ordinary constructor never enables it.
        if checkpoints & 255 != 0 {
            return;
        }
        let now = Instant::now();
        if now < data.next_sample.get() {
            return;
        }
        data.next_sample.set(now + Duration::from_millis(2));
        let started = Instant::now();
        data.sample_count.set(data.sample_count.get() + 1);
        for level in 0..4 {
            let Some(frame) = lua.inspect_stack(level, |debug| {
                let info = debug.source();
                let raw_source = info.source.as_deref();
                let trusted = data.trusted_chunks.borrow();
                let source = if info.what == "C" {
                    "[C]"
                } else if let Some(name) = raw_source.filter(|name| trusted.contains(*name)) {
                    name.strip_prefix('@').unwrap_or(name)
                } else {
                    "<dynamic>"
                };
                let name = debug
                    .names()
                    .name
                    .map(|name| name.chars().take(120).collect::<String>());
                (
                    source.chars().take(240).collect::<String>(),
                    info.line_defined,
                    name,
                    info.what.to_owned(),
                )
            }) else {
                if level == 0 {
                    data.unavailable_top.set(data.unavailable_top.get() + 1);
                }
                break;
            };
            let mut samples = data.samples.borrow_mut();
            if samples.len() < 512 || samples.contains_key(&frame) {
                let record = samples.entry(frame).or_default();
                record.0 += 1;
                record.1 += u64::from(level == 0);
            } else {
                data.dropped.set(data.dropped.get() + 1);
                if level == 0 {
                    data.dropped_self.set(data.dropped_self.get() + 1);
                }
            }
        }
        data.sampling_ns
            .set(data.sampling_ns.get() + started.elapsed().as_nanos());
    }
    pub(crate) fn report(&self, coverage: bool) -> Value {
        let Some(data) = &self.0 else {
            return Value::Null;
        };
        let stages:Vec<_>=data.stages.borrow().iter().map(|(phase,(calls,elapsed))|json!({"phase":phase,"calls":calls,"elapsed_ms":elapsed.as_secs_f64()*1000.})).collect();
        let samples:Vec<_>=data.samples.borrow().iter().map(|((source,line,name,what),(inclusive,own))|json!({"source":source,"defined_line":line,"function":name,"kind":what,"inclusive_samples":inclusive,"self_samples":own})).collect();
        let lookups = data
            .lookups
            .borrow()
            .report(data.lookup_collection_ns.get());
        json!({"coverage":coverage,"collection_window_ms":data.started.elapsed().as_secs_f64()*1000.,"stages":stages,
            "parameter_lookups":lookups,"trusted_chunk_count":data.trusted_chunks.borrow().len(),
            "dropped_chunk_names":data.dropped_chunk_names.get(),
            "source_name_scope":"Only bounded compiler names explicitly registered by the host or resolved script resource authority. Unregistered/debug-only names are scrubbed; names identify chunks, not verified source bodies",
            "lua_samples":samples,"sample_count":data.sample_count.get(),"dropped_frames":data.dropped.get(),"sampling_overhead_ms":data.sampling_ns.get() as f64/1_000_000.,
            "dropped_self_samples":data.dropped_self.get(),"unavailable_top_frames":data.unavailable_top.get(),
            "self_sample_scope":"Topmost frame at an observed VM safepoint (Lua or C). Retained self samples plus dropped self samples plus unavailable top frames equal sample_count. These are not exclusive CPU time; a blocking/native callback may be absent from VM safepoint samples",
            "sample_scope":"Opportunistic VM safepoint stacks, at most 500Hz; up to four inclusive frames. Counts do not measure exclusive CPU time or time spent blocked in native callbacks",
            "timing_scope":"Inclusive wall clocks; nested compilation/require/body/API phases overlap. Collection window extends through any UI export/render before this report; host_create measures initializer construction. No script text, argument values, stack bytes or heap contents are retained"})
    }
}
