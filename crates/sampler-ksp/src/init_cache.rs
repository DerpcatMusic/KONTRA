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
    residency_schema: u8,
    async_base: i32,
    async_high: i32,
    residency: Vec<crate::model::InitialResidencyRequest>,
    source_digest: [u8; 32],
    groups: Vec<String>,
    folders: [String; 4],
    slot: u8,
    performance_view: crate::model::PerformanceView,
    engine_values: Vec<([i32; 4], i32)>,
    engine_lookups: Vec<sampler_core::EngineLookup>,
    #[serde(default)]
    zones: Vec<(u32, [i32; 3])>,
    #[serde(default)]
    zone_identity_schema: u8,
    #[serde(default)]
    snapshot_mode: crate::model::SnapshotMode,
    #[serde(default)]
    zone_catalog: Vec<(Option<u32>, u32, u8, u8)>,
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

/// The cache's entry wire shape: the script-state entry as it was before
/// texts moved to a side table, so cached inits stay readable.
mod wire {
    use serde::{Deserialize, Serialize};
    #[derive(Serialize, Deserialize)]
    pub(super) enum ScriptStateValue {
        Control(sampler_core::ControlValue),
        Cell(i64),
        Text(sampler_core::Text),
    }
    #[derive(Serialize, Deserialize)]
    pub(super) struct ScriptStateEntry {
        pub(super) address: sampler_core::ScriptStateAddress,
        pub(super) value: ScriptStateValue,
    }
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
        use sampler_core::ScriptStateAddress as A;
        use wire::{ScriptStateEntry as Entry, ScriptStateValue as V};
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
        if !self.init.midi_object.has_only_async_id_reservations()
            || !self.environment.midi_object.has_only_async_id_reservations()
            || crate::eval::uses_array_file(&self.hir)
            || crate::zone_routing::requirements(&self.hir).writes
            // ponytail: init-time declaration ranges are not cached; cache them if those loads get slow.
            || self.hir.ui_params_at_init
            || self.init.midi_object.async_id_high_water().checked_sub(self.environment.midi_object.async_id_high_water())
                .and_then(|n|usize::try_from(n).ok()) != Some(self.init.residency.len())
        {
            return None;
        }
        Some(CachedInit {
            residency_schema: 1,
            zone_identity_schema: 1,
            snapshot_mode: self.environment.snapshot_mode,
            zone_catalog: self.environment.zone_catalog.iter().map(|z|
                (z.id,z.home.group,z.home.low_key,z.home.high_key)).collect(),
            async_base:self.environment.midi_object.async_id_high_water(),
            async_high:self.init.midi_object.async_id_high_water(),
            residency:self.init.residency.clone(),
            source_digest: self.source_digest,
            groups: self.environment.groups.clone(),
            folders: self.environment.folders.clone(),
            slot: self.environment.slot,
            performance_view: self.environment.performance_view.clone(),
            engine_values: self
                .environment
                .engine_values
                .iter()
                .map(|(k, v)| (*k, *v))
                .collect(),
            engine_lookups: self.environment.engine_lookups.clone(),
            zones: self
                .environment
                .zones
                .iter()
                .map(|(id, values)| (*id, *values))
                .collect(),
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
        use sampler_core::ScriptStateAddress as Address;
        use wire::{ScriptStateEntry, ScriptStateValue as Value};
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
    // Old cached init cannot suppress an explicitly requested external read/write.
    // Conservative lexical rejection also covers calls through init functions.
    if source.contains("load_array_str") || source.contains("save_array_str") {
        return Err(error(
            "explicit-path array scripts require fresh initialization",
        ));
    }
    if source.len() > limits.source_bytes {
        return Err(error("source byte budget exceeded"));
    }
    if cached.residency_schema != 1 || cached.async_base < 0 || cached.async_high < cached.async_base
        || cached.residency.len() > sampler_core::InitialGroupResidency::MAX_REQUESTS {
        return Err(error("invalid cached startup residency identity"));
    }
    let mut prior=cached.async_base;
    for request in &cached.residency {
        if request.async_id <= prior || request.async_id > cached.async_high
            || request.group < 0 || request.source_slot != cached.slot
            || request.offset as usize >= source.len() {
            return Err(error("invalid cached startup residency request"));
        }
        prior=request.async_id;
    }
    if cached.model.requests.iter().filter(|r|r.command=="purge_group").count()!=cached.residency.len() {
        return Err(error("incomplete cached startup residency roster"));
    }
    match cached.model.snapshot_mode_write {
        Some(write)
            if write.mode != cached.model.snapshot_mode || write.offset as usize >= source.len() =>
        {
            return Err(error("invalid cached snapshot policy write"));
        }
        None if cached.model.snapshot_mode != crate::model::SnapshotMode::NativeWithInit => {
            return Err(error("cached snapshot policy has no setter identity"));
        }
        _ => {}
    }
    let mut source_ids=std::collections::BTreeSet::new();
    if cached.zone_catalog.iter().any(|(id,_,low,high)| *low>*high || *high>127
        || id.is_some_and(|id|id>i32::MAX as u32 || !source_ids.insert(id))) {
        return Err(error("invalid cached native zone catalog"));
    }
    let mut captured_ids=std::collections::BTreeSet::new();
    if cached.zones.iter().any(|(id,home)| *id>i32::MAX as u32 || !captured_ids.insert(*id)
        || home[0]<0 || !(0..=127).contains(&home[1]) || !(0..=127).contains(&home[2]) || home[1]>home[2]) {
        return Err(error("invalid cached native zone homes"));
    }
    if cached.zone_identity_schema != 1 {
        return Err(error("cached zone source identity schema is obsolete"));
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
    // Preserve positioned source-admission diagnostics, then reject identity
    // mismatch before any cached values or model state are applied.
    if cached.source_digest != crate::setup::source_digest(source) {
        return Err(error("cached source identity changed"));
    }
    if crate::zone_routing::requirements(&hir).writes {
        return Err(error("zone-writer cache requires fresh physical-source initialization"));
    }
    if hir.ui_params_at_init {
        return Err(error("init-time declaration ranges require fresh initialization"));
    }
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
        residency: cached.residency,
        cells: vec![0; hir.cells as usize],
        texts: vec![String::new(); hir.texts as usize],
        controls: vec![0; hir.uis.len()],
        persistence: cached.persistence,
        model: cached.model,
        warnings: Vec::new(),
        engine: cached.engine.into_iter().collect(),
        zone_parameters: Default::default(),
        properties: cached.properties.into_iter().collect(),
        text_properties: cached.text_properties.into_iter().collect(),
        indexed_properties: cached.indexed_properties.into_iter().collect(),
        ui_params: vec![None; hir.uis.len()],
    };
    init.midi_object.reserve_async_ids_through(cached.async_high).map_err(|_|error("invalid cached async high-water"))?;
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
    let mut initial_midi=sampler_core::MidiObject::default();
    initial_midi.reserve_async_ids_through(cached.async_base).map_err(|_|error("invalid cached async base"))?;
    let environment = Environment {
        midi_object:initial_midi,
        snapshot_mode: cached.snapshot_mode,
        groups: cached.groups,
        folders: cached.folders,
        slot: cached.slot,
        performance_view: cached.performance_view,
        engine_values: cached.engine_values.into_iter().collect(),
        engine_lookups: cached.engine_lookups,
        zones: cached.zones.into_iter().collect(),
        zone_catalog: cached.zone_catalog.into_iter().map(|(id,group,low_key,high_key)|
            sampler_ir::SourceZone {id,home:sampler_ir::SourceZoneHome {group,low_key,high_key}}).collect(),
        ..Default::default()
    };
    #[cfg(feature = "scan")]
    {
        crate::scan::reset_script();
        crate::scan::attempt("product-cache");
    }
    Ok(Initialized {
        source_digest: cached.source_digest,
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
    fn cached_initializer_preserves_callback_folder_lookups() {
        let source = "on init
declare $literal
declare $computed
declare $missing
declare $selector
end on
on note
$literal := find_group(get_folder($GET_FOLDER_LIBRARY_DIR))
$selector := $GET_FOLDER_PATCH_DIR
$computed := find_group(get_folder($selector))
$selector := 99
$missing := find_group(get_folder($selector))
end on";
        let environment = crate::Environment {
            groups: ["/library/", "/patch/", ""].map(str::to_owned).into(),
            folders: ["/library/", "", "/patch/", ""].map(str::to_owned),
            ..Default::default()
        };
        let cold = crate::initialize(source, crate::Limits::LIBRARY, &environment).unwrap();
        let bytes = serde_json::to_vec(&cold.capture_initialized().unwrap()).unwrap();
        let warm = super::restore_initialized(
            source,
            crate::Limits::LIBRARY,
            serde_json::from_slice(&bytes).unwrap(),
        )
        .unwrap();
        for (name, initialized) in [("cold", cold), ("warm", warm)] {
            let script =
                crate::compile_initialized(source, 48000, crate::Limits::LIBRARY, &[], initialized)
                    .unwrap();
            let plan = script
                .bind(sampler_core::Prepared::new(48000, vec![], vec![], 0).unwrap())
                .unwrap();
            let limits = sampler_core::Limits::for_plan(&plan, 4, 4);
            let mut runtime = sampler_core::Runtime::new(plan, limits).unwrap();
            runtime
                .trigger(
                    sampler_core::Input {
                        protocol: sampler_core::Protocol::Native,
                        port: 0,
                        group: 0,
                        channel: 0,
                        key: 60,
                        external_id: Some(60),
                    },
                    60,
                    1.,
                )
                .unwrap();
            runtime.render(&mut [[0.; 2]; 64]).unwrap();
            for (cell, expected) in [(0, 0), (1, 1), (2, 2)] {
                assert_eq!(
                    runtime.script_cell(
                        runtime.active_plan(),
                        sampler_core::ScriptInstanceId(0),
                        cell
                    ),
                    Ok(expected),
                    "{name} callback folder lookup at cell {cell}"
                );
            }
        }
    }

    #[test]
    fn cached_waiting_persistence_starts_once_on_live_activation() {
        let source = "on init declare $calls declare $done end on
on persistence_changed inc($calls) wait(1000) inc($done) end on";
        let init = crate::initialize(source, crate::Limits::LIBRARY, &Default::default()).unwrap();
        let bytes = serde_json::to_vec(&init.capture_initialized().unwrap()).unwrap();
        let restored = super::restore_initialized(
            source,
            crate::Limits::LIBRARY,
            serde_json::from_slice(&bytes).unwrap(),
        )
        .unwrap();
        let script =
            crate::compile_initialized(source, 48000, crate::Limits::LIBRARY, &[], restored)
                .unwrap();
        assert_eq!(
            script.model().persistence_completion,
            crate::model::PersistenceCompletion::Scheduled
        );
        let plan = script
            .bind(sampler_core::Prepared::new(48000, vec![], vec![], 0).unwrap())
            .unwrap();
        let limits = sampler_core::Limits::for_plan(&plan, 4, 4);
        let mut runtime = sampler_core::Runtime::new(plan, limits).unwrap();
        let plan = runtime.active_plan();
        for _ in 0..2 {
            runtime.render(&mut [[0.; 2]; 64]).unwrap();
        }
        assert_eq!(
            runtime.script_cell(plan, sampler_core::ScriptInstanceId(0), 0),
            Ok(1)
        );
        assert_eq!(
            runtime.script_cell(plan, sampler_core::ScriptInstanceId(0), 1),
            Ok(1)
        );
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
        environment.zones.insert(73, [7, 12, 94]);
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
        assert_eq!(restored.environment.zones, environment.zones);
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
