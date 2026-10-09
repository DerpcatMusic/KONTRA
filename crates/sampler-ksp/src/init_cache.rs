//! Product-cache metadata beside native owned script-state capture.
use crate::{Environment, Error, Initialized, Limits};
use serde::{Deserialize, Serialize};

impl<'de> Deserialize<'de> for crate::model::Request {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Request {
            command: String,
            args: Vec<crate::model::Value>,
        }
        let r = Request::deserialize(d)?;
        let command = crate::builtins::Builtin::from_name(&r.command)
            .ok_or_else(|| serde::de::Error::custom("unknown cached builtin"))?
            .name();
        Ok(Self {
            command,
            args: r.args,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) enum CachedCompletion {
    NotPresent,
    Scheduled,
    Completed,
    Failed {
        category: crate::model::EvaluationFailure,
        offset: u32,
        builtin: Option<String>,
    },
}
impl From<crate::model::PersistenceCompletion> for CachedCompletion {
    fn from(value: crate::model::PersistenceCompletion) -> Self {
        use crate::model::PersistenceCompletion as C;
        match value {
            C::NotPresent => Self::NotPresent,
            C::Scheduled => Self::Scheduled,
            C::Completed => Self::Completed,
            C::Failed {
                category,
                offset,
                builtin,
            } => Self::Failed {
                category,
                offset,
                builtin: builtin.map(str::to_owned),
            },
        }
    }
}
impl TryFrom<CachedCompletion> for crate::model::PersistenceCompletion {
    type Error = String;
    fn try_from(value: CachedCompletion) -> Result<Self, String> {
        Ok(match value {
            CachedCompletion::NotPresent => Self::NotPresent,
            CachedCompletion::Scheduled => Self::Scheduled,
            CachedCompletion::Completed => Self::Completed,
            CachedCompletion::Failed {
                category,
                offset,
                builtin,
            } => Self::Failed {
                category,
                offset,
                builtin: builtin
                    .map(|name| {
                        crate::builtins::Builtin::from_name(&name)
                            .map(|b| b.name())
                            .ok_or_else(|| "unknown cached builtin".to_owned())
                    })
                    .transpose()?,
            },
        })
    }
}

impl Serialize for crate::model::PersistenceCompletion {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        CachedCompletion::from(*self).serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for crate::model::PersistenceCompletion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        CachedCompletion::deserialize(d)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// Initializer state uses the native owned-buffer address/value schema.
/// Frontend/model metadata remains rate-independent; no programs or PCM are cached.
#[derive(Serialize, Deserialize)]
pub struct CachedInit {
    groups: Vec<String>,
    slot: u8,
    performance_view: crate::model::PerformanceView,
    engine_values: Vec<([i32; 4], i32)>,
    engine_lookups: Vec<sampler_core::EngineLookup>,
    state: Box<serde_json::value::RawValue>,
    cells: u32,
    texts: u32,
    controls: usize,
    entries: usize,
    persistence: Vec<crate::hir::Persistence>,
    model: crate::model::Model,
    engine: Vec<([i32; 4], i32)>,
    properties: Vec<((i32, i32), i32)>,
    text_properties: Vec<((i32, i32), String)>,
    indexed_properties: Vec<((i32, i32, i32), crate::model::Value)>,
    warnings: Vec<(u32, u32, Option<String>, String)>,
}

fn controls(
    hir: &crate::hir::Hir,
    slot: u8,
) -> std::collections::BTreeMap<sampler_core::ControlId, usize> {
    hir.uis
        .iter()
        .enumerate()
        .filter_map(|(i, ui)| {
            let var = &hir.vars[ui.var.0 as usize];
            matches!(var.home, crate::hir::Home::Control(_))
                .then(|| (crate::derived_control_id(slot, &var.name), i))
        })
        .collect()
}

struct NativeValues<'a>(&'a Initialized);
impl NativeValues<'_> {
    fn len(&self) -> usize {
        let init = &self.0.init;
        controls(&self.0.hir, self.0.environment.slot)
            .values()
            .filter(|&&i| init.controls[i] != 0)
            .count()
            + init.cells.iter().filter(|&&v| v != 0).count()
            + init.texts.iter().filter(|v| !v.is_empty()).count()
    }
}
impl Serialize for NativeValues<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use sampler_core::{
            ScriptStateAddress as A, ScriptStateEntry as Entry, ScriptStateValue as V,
        };
        use serde::ser::SerializeSeq;
        let init = &self.0.init;
        let controls = controls(&self.0.hir, self.0.environment.slot);
        let mut seq = serializer.serialize_seq(Some(self.len()))?;
        for (id, i) in controls {
            if init.controls[i] == 0 {
                continue;
            }
            seq.serialize_element(&Entry {
                address: A::Control(id),
                value: V::Control(sampler_core::ControlValue::Integer(i64::from(
                    init.controls[i],
                ))),
            })?;
        }
        let instance = sampler_core::ScriptInstanceId(0);
        for (index, &value) in init.cells.iter().enumerate() {
            if value == 0 {
                continue;
            }
            seq.serialize_element(&Entry {
                address: A::Cell {
                    instance,
                    index: index as u32,
                },
                value: V::Cell(value),
            })?;
        }
        for (index, value) in init.texts.iter().enumerate() {
            if value.is_empty() {
                continue;
            }
            let text = sampler_core::Text::try_new(value)
                .map_err(|_| serde::ser::Error::custom("cached text exceeds native capacity"))?;
            seq.serialize_element(&Entry {
                address: A::Text {
                    instance,
                    index: index as u32,
                },
                value: V::Text(text),
            })?;
        }
        seq.end()
    }
}

impl Initialized {
    /// Capture off audio using native owned-buffer entries, without materializing
    /// one fixed-capacity Text union for every integer cell.
    pub fn capture_initialized(&self) -> Option<CachedInit> {
        // ponytail: fall back to fresh init until the cache carries owned MIDI jobs/events.
        if self.init.midi_object != Default::default()
            || self.environment.midi_object != Default::default()
        {
            return None;
        }
        Some(CachedInit {
            groups: self.environment.groups.clone(),
            slot: self.environment.slot,
            performance_view: self.environment.performance_view.clone(),
            engine_values: self
                .environment
                .engine_values
                .iter()
                .map(|(k, v)| (*k, *v))
                .collect(),
            engine_lookups: self.environment.engine_lookups.clone(),
            state: serde_json::value::to_raw_value(&NativeValues(self)).ok()?,
            cells: self.hir.cells,
            texts: self.hir.texts,
            controls: controls(&self.hir, self.environment.slot).len(),
            entries: NativeValues(self).len(),
            persistence: self.init.persistence.clone(),
            model: self.init.model.clone(),
            engine: self.init.engine.iter().map(|(k, v)| (*k, *v)).collect(),
            properties: self.init.properties.iter().map(|(k, v)| (*k, *v)).collect(),
            text_properties: self
                .init
                .text_properties
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
            indexed_properties: self
                .init
                .indexed_properties
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
            warnings: self
                .init
                .warnings
                .iter()
                .map(|w| {
                    (
                        w.span.start,
                        w.span.end,
                        w.builtin.map(str::to_owned),
                        w.message.clone(),
                    )
                })
                .collect(),
        })
    }
}

struct RestoreValues<'a> {
    initial: &'a mut crate::eval::Initial,
    entries: usize,
    controls: std::collections::BTreeMap<sampler_core::ControlId, usize>,
}
impl<'de> serde::de::DeserializeSeed<'de> for RestoreValues<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}
impl<'de> serde::de::Visitor<'de> for RestoreValues<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("complete native initializer state")
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(mut self, mut seq: A) -> Result<(), A::Error> {
        use sampler_core::{
            ScriptStateAddress as Address, ScriptStateEntry, ScriptStateValue as Value,
        };
        let expected = self.entries;
        let mut count = 0;
        let mut prior = None;
        while let Some(entry) = seq.next_element::<ScriptStateEntry>()? {
            if prior.is_some_and(|p| p >= entry.address) {
                return Err(serde::de::Error::custom("unordered cached state"));
            }
            prior = Some(entry.address);
            match (entry.address, entry.value) {
                (Address::Control(id), Value::Control(sampler_core::ControlValue::Integer(v))) => {
                    let i = self
                        .controls
                        .remove(&id)
                        .ok_or_else(|| serde::de::Error::custom("unknown cached control"))?;
                    self.initial.controls[i] =
                        i32::try_from(v).map_err(serde::de::Error::custom)?;
                }
                (Address::Cell { instance, index }, Value::Cell(v)) if instance.0 == 0 => {
                    *self
                        .initial
                        .cells
                        .get_mut(index as usize)
                        .ok_or_else(|| serde::de::Error::custom("cached cell out of bounds"))? = v;
                }
                (Address::Text { instance, index }, Value::Text(v)) if instance.0 == 0 => {
                    *self
                        .initial
                        .texts
                        .get_mut(index as usize)
                        .ok_or_else(|| serde::de::Error::custom("cached text out of bounds"))? =
                        v.as_str().to_owned();
                }
                _ => return Err(serde::de::Error::custom("invalid cached state type")),
            }
            count += 1;
        }
        if count != expected {
            return Err(serde::de::Error::custom("incomplete cached state"));
        }
        Ok(())
    }
}

/// Validate and reconstruct the frontend, restoring native state without evaluating init.
pub fn restore_initialized(
    source: &str,
    limits: Limits,
    cached: CachedInit,
) -> Result<Initialized, Error> {
    let error = |message: &str| Error {
        offset: 0,
        line: 1,
        column: 1,
        kind: crate::Kind::Error,
        builtin: None,
        message: message.into(),
    };
    if source.len() > limits.source_bytes {
        return Err(error("source byte budget exceeded"));
    }
    let frontend_begin = std::time::Instant::now();
    let mut syms = crate::lexer::Interner::default();
    let mut toks = crate::lexer::lex(source, &mut syms).map_err(|f| f.locate(source))?;
    let conditions = crate::lexer::preprocess(&mut toks, &syms, &Default::default())
        .map_err(|f| f.locate(source))?;
    let ast = crate::parser::parse(&toks, &syms).map_err(|f| f.locate(source))?;
    let hir = crate::sema::analyze(
        ast,
        &syms,
        crate::sema::Budget {
            variables: limits.variables,
            array_cells: limits.array_cells,
        },
        &cached.performance_view.controls,
    )
    .map_err(|f| f.locate(source))?;
    if std::env::var_os("KONTRA_AUDIT_LOAD").is_some() {
        eprintln!(
            "AUDIT {{\"stage\":\"ksp_cache_frontend\",\"ms\":{}}}",
            frontend_begin.elapsed().as_secs_f64() * 1000.
        );
    }
    let restore_begin = std::time::Instant::now();
    if cached.persistence.len() != hir.vars.len() {
        return Err(error("cached persistence shape mismatch"));
    }
    let ids = controls(&hir, cached.slot);
    if cached.cells != hir.cells
        || cached.texts != hir.texts
        || cached.controls != ids.len()
        || cached.entries > cached.cells as usize + cached.texts as usize + cached.controls
    {
        return Err(error("cached native shape mismatch"));
    }
    // Every restored initializer owns fresh zero/empty banks. Native entry
    // addresses therefore need only describe the nonzero post-init values.
    let mut init = crate::eval::Initial {
        midi_object: Default::default(),
        cells: vec![0; hir.cells as usize],
        texts: vec![String::new(); hir.texts as usize],
        controls: vec![0; hir.uis.len()],
        persistence: cached.persistence,
        model: cached.model,
        warnings: Vec::new(),
        engine: cached.engine.into_iter().collect(),
        properties: cached.properties.into_iter().collect(),
        text_properties: cached.text_properties.into_iter().collect(),
        indexed_properties: cached.indexed_properties.into_iter().collect(),
    };
    use serde::de::DeserializeSeed;
    let mut d = serde_json::Deserializer::from_str(cached.state.get());
    RestoreValues {
        controls: ids,
        entries: cached.entries,
        initial: &mut init,
    }
    .deserialize(&mut d)
    .map_err(|_| error("invalid cached native state"))?;
    d.end().map_err(|_| error("trailing cached state"))?;
    if std::env::var_os("KONTRA_AUDIT_LOAD").is_some() {
        eprintln!(
            "AUDIT {{\"stage\":\"ksp_cache_native_restore\",\"ms\":{}}}",
            restore_begin.elapsed().as_secs_f64() * 1000.
        );
    }
    for (start, end, builtin, message) in cached.warnings {
        if start > end || end as usize > source.len() {
            return Err(error("invalid cached warning span"));
        }
        let builtin = builtin
            .map(|n| {
                crate::builtins::Builtin::from_name(&n)
                    .map(|b| b.name())
                    .ok_or_else(|| error("unknown cached warning builtin"))
            })
            .transpose()?;
        init.warnings.push(crate::diag::Fault {
            span: crate::diag::Span { start, end },
            builtin,
            message,
        });
    }
    let environment = Environment {
        groups: cached.groups,
        slot: cached.slot,
        performance_view: cached.performance_view,
        engine_values: cached.engine_values.into_iter().collect(),
        engine_lookups: cached.engine_lookups,
        ..Default::default()
    };
    #[cfg(feature = "scan")]
    {
        crate::scan::reset_script();
        crate::scan::attempt("product-cache");
    }
    Ok(Initialized {
        hir,
        init,
        conditions,
        environment,
        #[cfg(feature = "scan")]
        observation: crate::scan::checkpoint(),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn cached_waiting_persistence_starts_once_on_live_activation() {
        let source = "on init declare $calls declare $done end on
on persistence_changed inc($calls) wait(1000) inc($done) end on";
        let init = crate::initialize(source, crate::Limits::LIBRARY, &Default::default()).unwrap();
        let bytes = serde_json::to_vec(&init.capture_initialized().unwrap()).unwrap();
        let restored = super::restore_initialized(source, crate::Limits::LIBRARY, serde_json::from_slice(&bytes).unwrap()).unwrap();
        let script = crate::compile_initialized(source, 48000, crate::Limits::LIBRARY, &[], restored).unwrap();
        assert_eq!(script.model().persistence_completion, crate::model::PersistenceCompletion::Scheduled);
        let plan = script.bind(sampler_core::Prepared::new(48000, vec![], vec![], 0).unwrap()).unwrap();
        let limits = sampler_core::Limits::for_plan(&plan, 4, 4);
        let mut runtime = sampler_core::Runtime::new(plan, limits).unwrap();
        let plan = runtime.active_plan();
        for _ in 0..2 { runtime.render(&mut [[0.; 2]; 64]).unwrap(); }
        assert_eq!(runtime.script_cell(plan, sampler_core::ScriptInstanceId(0), 0), Ok(1));
        assert_eq!(runtime.script_cell(plan, sampler_core::ScriptInstanceId(0), 1), Ok(1));
    }
    #[test]
    fn cached_initializer_preserves_native_state_model_and_engine_without_reinit() {
        let source = "on init\ndeclare ui_knob $k(0,100,1)\n$k := 37\ndeclare %a[2] := (4,9)\ndeclare @text\n@text := \"restored\"\nset_engine_par($ENGINE_PAR_VOLUME,12345,-1,-1,-1)\nend on\non note\nmessage(@text)\nend on";
        let init = crate::initialize(source, crate::Limits::LIBRARY, &Default::default()).unwrap();
        let expected = init.engine_pars();
        let cached = init
            .capture_initialized()
            .expect("native initializer capture");
        let bytes = serde_json::to_vec(&cached).unwrap();
        let restored = super::restore_initialized(
            source,
            crate::Limits::LIBRARY,
            serde_json::from_slice(&bytes).unwrap(),
        )
        .unwrap();
        assert_eq!(restored.engine_pars(), expected);
        let script =
            crate::compile_initialized(source, 48000, crate::Limits::LIBRARY, &[], restored)
                .unwrap();
        assert_eq!(
            script.controls()[0].definition.default,
            sampler_core::ControlValue::Integer(37)
        );
        assert!(script.cells.contains(&9));
        assert!(script.resources.texts.iter().any(|t| t == "restored"));
    }
    #[test]
    fn cached_initializer_retains_engine_environment_and_declines_midi_state() {
        let source = "on init\nend on";
        let mut environment = crate::Environment::default();
        environment.engine_values.insert([9, 2, 0, 0], 12345);
        environment.engine_lookups.push(sampler_core::EngineLookup {
            group: 2,
            owner: 0,
            target: false,
            name: "ENV_AHDSR".into(),
            index: 3,
        });
        let init = crate::initialize(source, crate::Limits::LIBRARY, &environment).unwrap();
        let cached = init.capture_initialized().unwrap();
        let restored = super::restore_initialized(source, crate::Limits::LIBRARY, cached).unwrap();
        assert_eq!(
            restored.environment.engine_values,
            environment.engine_values
        );
        assert_eq!(
            restored.environment.engine_lookups,
            environment.engine_lookups
        );
        let midi = crate::initialize(
            "on init\nmf_set_buffer_size(2)\nend on",
            crate::Limits::LIBRARY,
            &Default::default(),
        )
        .unwrap();
        assert!(midi.capture_initialized().is_none());
    }
    #[test]
    fn rejects_incomplete_unordered_and_wrong_type_native_entries() {
        let source = "on init\ndeclare $a := 9\ndeclare $b := 4\nend on";
        let cold = crate::initialize(source, crate::Limits::LIBRARY, &Default::default()).unwrap();
        for malformed in [
            "[]",
            "[{\"address\":{\"Cell\":{\"instance\":0,\"index\":1}},\"value\":{\"Cell\":4}}]",
            "[{\"address\":{\"Cell\":{\"instance\":0,\"index\":0}},\"value\":{\"Text\":\"wrong\"}}]",
        ] {
            let mut cached = cold.capture_initialized().unwrap();
            cached.state = serde_json::value::RawValue::from_string(malformed.into()).unwrap();
            assert!(super::restore_initialized(source, crate::Limits::LIBRARY, cached).is_err());
        }
    }
    #[test]
    fn native_cache_omits_zero_initialized_banks_without_losing_state() {
        let source = "on init\ndeclare %bank[100000]\n%bank[42] := 9\nend on";
        let cold = crate::initialize(source, crate::Limits::LIBRARY, &Default::default()).unwrap();
        let cached = cold.capture_initialized().unwrap();
        assert!(
            cached.state.get().len() < 512,
            "zero banks need no owned entry payload"
        );
        let warm = super::restore_initialized(source, crate::Limits::LIBRARY, cached).unwrap();
        assert_eq!(warm.init.cells, cold.init.cells);
    }
}
