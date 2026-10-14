//! Explicit, source-free initialization clocks and bounded VM safepoint samples.
use mlua::Lua;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub(crate) struct Profile(Option<Rc<Data>>);

struct Data {
    started: Instant,
    stages: RefCell<BTreeMap<&'static str, (u64, Duration)>>,
    next_sample: Cell<Instant>,
    samples: RefCell<BTreeMap<(String, Option<usize>, Option<String>, String), u64>>,
    sample_count: Cell<u64>,
    dropped: Cell<u64>,
    sampling_ns: Cell<u128>,
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
                sampling_ns: Cell::new(0),
            })
        }))
    }
    pub(crate) fn enabled(&self) -> bool {
        self.0.is_some()
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
                let source = debug.source();
                let source = source.short_src.as_deref().unwrap_or("<unknown>");
                let source = if source.contains('\n') || source.starts_with("[string ") {
                    "<dynamic>"
                } else {
                    source
                };
                let info = debug.source();
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
                break;
            };
            let mut samples = data.samples.borrow_mut();
            if samples.len() < 512 || samples.contains_key(&frame) {
                *samples.entry(frame).or_default() += 1;
            } else {
                data.dropped.set(data.dropped.get() + 1);
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
        let samples:Vec<_>=data.samples.borrow().iter().map(|((source,line,name,what),samples)|json!({"source":source,"defined_line":line,"function":name,"kind":what,"inclusive_samples":samples})).collect();
        json!({"coverage":coverage,"collection_window_ms":data.started.elapsed().as_secs_f64()*1000.,"stages":stages,
            "lua_samples":samples,"sample_count":data.sample_count.get(),"dropped_frames":data.dropped.get(),"sampling_overhead_ms":data.sampling_ns.get() as f64/1_000_000.,
            "sample_scope":"Opportunistic VM safepoint stacks, at most 500Hz; up to four inclusive frames. Counts do not measure exclusive CPU time or time spent blocked in native callbacks",
            "timing_scope":"Inclusive wall clocks; nested compilation/require/body/API phases overlap. Collection window extends through any UI export/render before this report; host_create measures initializer construction. No script text, argument values, stack bytes or heap contents are retained"})
    }
}
