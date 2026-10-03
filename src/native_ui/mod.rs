//! Embedded legacy native UI subset. Loading, Lua and scene evaluation are off audio.
//! Unsupported syntax/APIs/images fail explicitly; exported KSP views remain usable.
mod frontend;
mod drops;
#[cfg(test)]
mod test_host;
pub(crate) use drops::Payload as DropPayload;
use crate::ksp::{Live, Runtime};
use anyhow::{Context, Result, bail, ensure};
use mlua::{Function, HookTriggers, Lua, LuaOptions, StdLib, Table, Value, VmState};
use moose::mui::mui::{layout::Insets, prelude::Align};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Paint {
    Stack,
    HStack(f64),
    VStack(f64),
    Spacer(f64),
    Text(String),
    Rectangle([f32; 4], f64),
    Ring { color: [f32; 4], start: f64, angle: f64, thickness: f64 },
    Asset(crate::native_assets::AssetRef),
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Node {
    pub id: usize,
    pub paint: Paint,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub minimum: [Option<f64>; 2],
    pub maximum: [Option<f64>; 2],
    pub text_size: f64,
    pub text_color: [f32; 4],
    pub text_bold: bool,
    pub text_lines: Option<usize>,
    pub alignment: (Align, Align),
    pub anchor: Option<(Align, Align)>,
    transparent: bool,
    pub(crate) spacer_scope: bool,
    pub padding: Insets,
    pub offset: [f64; 2],
    pub rotation: Option<f64>,
    pub hidden: bool,
    pub disabled: bool,
    pub opacity: f32,
    pub background: bool,
    pub overlay: bool,
    pub tap: bool,
    pub hover: bool,
    pub drop: bool,
    pub children: Vec<Node>,
}
impl Node {
    // Placement travels through modifier/conditional scopes, not through a
    // frame or an authored container alignment. Its outward anchor stays
    // distinct from the alignment used inside a real frame.
    fn inherit_placement(&mut self, guide: (Align, Align)) {
        if !self.transparent { return; }
        let guide = self.anchor.unwrap_or(guide);
        self.alignment = guide;
        for child in &mut self.children { child.inherit_placement(guide); }
    }
    fn text_bytes(&self) -> usize {
        let own = match &self.paint { Paint::Text(text) => text.len(), _ => 0 };
        self.children.iter().fold(own, |sum, child| sum.saturating_add(child.text_bytes()))
    }
    pub(crate) fn asset_names(&self) -> BTreeSet<String> {
        fn collect(node: &Node, names: &mut BTreeSet<String>) {
            if let Paint::Asset(asset) = &node.paint {
                names.insert(asset.resource.clone());
            }
            for child in &node.children {
                collect(child, names);
            }
        }
        let mut names = BTreeSet::new();
        collect(self, &mut names);
        names
    }
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Edit {
    pub slot: usize,
    pub control: usize,
    pub value: i32,
}
#[derive(Clone, Copy, PartialEq)]
struct HoverInput {
    position: moose::mui::mui::prelude::Point,
    frame: moose::mui::mui::layout::Frame,
    pose: moose::mui::mui::scene::LocalPose,
    mods: moose::mui::mui::prelude::Mods,
}
struct Hover {
    callbacks: [Option<Function>; 3],
    present: bool,
    enabled: bool,
    inside: bool,
    last: Option<HoverInput>,
}
#[derive(Clone, Default)]
struct PointerCallbacks {
    tap: [Option<Function>; 3], // start, double, complete
    drag: [Option<Function>; 3], // start, update, complete
    threshold: f64,
}
struct PointerCapture {
    callbacks: PointerCallbacks,
    origin: moose::mui::mui::prelude::Point,
    last: HoverInput,
    delivered: moose::mui::mui::prelude::Point,
    dragging: bool,
}
struct PointerGesture {
    callbacks: PointerCallbacks,
    present: bool,
    enabled: bool,
    capture: Option<PointerCapture>,
}
struct DropTarget {
    action: Function,
    formats: Vec<String>,
    present: bool,
    enabled: bool,
}
struct Binding {
    kind: Box<str>,
    slot: usize,
    control: usize,
    value: i32,
    meter_control: bool,
    meter: Option<crate::fx::MeterAttachment>,
    range: Option<(i32,i32)>,
    properties: crate::ksp::NativeProperties,
}
struct MeterBinding { slot: usize, control: usize, reader: Arc<crate::fx::MeterReader> }
impl Binding {
    fn normalized_range(&self) -> mlua::Result<(i32, i32)> {
        let (minimum, maximum) = self
            .range
            .ok_or_else(|| invalid("Native normalized parameter has no declared range"))?;
        let span = maximum
            .checked_sub(minimum)
            .filter(|span| *span > 0)
            .ok_or_else(|| {
                invalid("Unsupported degenerate, reversed or overflowing native normalized range")
            })?;
        Ok((minimum, span))
    }
    fn lua_value(&self) -> mlua::Result<Value> {
        Ok(match self.kind.as_ref() {
            "ui_button" | "ui_switch" => Value::Boolean(self.value != 0),
            "ui_knob" | "ui_slider" => {
                let (minimum, span) = self.normalized_range()?;
                let value = self.value.checked_sub(minimum).ok_or_else(|| {
                    invalid("Native normalized value overflows its declared range")
                })?;
                // The native legacy provider preserves its float32 variant.
                // Reads do not clamp; raw properties and discrete controls stay integers.
                Value::Number(f64::from(value as f32 / span as f32))
            }
            _ => Value::Integer(i64::from(self.value)),
        })
    }
    fn assigned_value(&self, value: Value) -> mlua::Result<i32> {
        match self.kind.as_ref() {
            "ui_button" | "ui_switch" => match value {
                Value::Boolean(value) => Ok(i32::from(value)),
                _ => Err(invalid("Boolean native parameter requires a Boolean value")),
            },
            "ui_knob" | "ui_slider" => {
                let value = match value {
                    Value::Integer(value) => value as f64,
                    Value::Number(value) if value.is_finite() => value,
                    _ => {
                        return Err(invalid(
                            "Normalized native parameter requires a finite number",
                        ));
                    }
                };
                let (minimum, span) = self.normalized_range()?;
                let normalized = (value as f32).clamp(0., 1.);
                // Native scalar writes narrow and clamp before mapping, then
                // round half away from zero. No display-step lattice is applied.
                Ok((f64::from(minimum) + f64::from(span) * f64::from(normalized)).round() as i32)
            }
            _ => match value {
                Value::Integer(value) => i32::try_from(value)
                    .map_err(|_| invalid("Native integer parameter exceeds i32")),
                Value::Number(value)
                    if value.is_finite()
                        && value.fract() == 0.
                        && (i32::MIN as f64..=i32::MAX as f64).contains(&value) =>
                {
                    Ok(value as i32)
                }
                _ => Err(invalid(
                    "Native integer parameter requires a finite whole value",
                )),
            },
        }
    }
}
#[derive(Default)]
struct Parameters {
    bindings: BTreeMap<String, Binding>,
    unsupported: BTreeSet<String>,
    missing_reads: BTreeSet<u64>,
    warnings: Vec<String>,
    meters: BTreeMap<String, MeterBinding>,
    edits: Vec<Edit>,
    gesture_active: bool,
}
/// Kept only in editor/worker-owned PartView, never handed to the audio thread.
pub(crate) struct Scene {
    lua: Lua,
    root: Table,
    parameters: Arc<Mutex<Parameters>>,
    instructions: Arc<AtomicUsize>,
    revision: Arc<AtomicU64>,
    output_meters: Arc<crate::fx::OutputMeters>,
    gestures: Vec<Option<PointerGesture>>,
    hovers: Vec<Option<Hover>>,
    drops: Vec<Option<DropTarget>>,
    identities: BTreeMap<String, usize>,
    pub(crate) assets: Arc<crate::native_assets::Assets>,
    asset_pending: BTreeSet<String>,
    raster_waker: Option<Arc<dyn Fn() + Send + Sync>>,
    raster_pending: Arc<AtomicBool>,
    pub identity: String,
    pub target_version: Option<String>,
    pub resources: Vec<(String, Vec<u8>)>,
}
impl std::fmt::Debug for Scene {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeUiScene")
            .field("identity", &self.identity)
            .field("target_version", &self.target_version)
            .finish_non_exhaustive()
    }
}
fn invalid(reason: impl ToString) -> mlua::Error {
    mlua::Error::RuntimeError(reason.to_string())
}
fn module_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && name.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
}
impl Scene {
    pub(crate) fn prepare(instrument: &Path, rt: &Runtime) -> Result<Option<Self>> {
        let Some((name, exposed)) = rt.native_ui_request()? else {
            return Ok(None);
        };
        // load_native_ui establishes legacy intent for a bare module name;
        // explicit typed entries are resolved and rejected before Lua sees bytes.
        let extension = name.rsplit('.').next().unwrap_or_default();
        let requested =
            if extension.eq_ignore_ascii_case("nui") || extension.eq_ignore_ascii_case("kscript") {
                name.to_owned()
            } else {
                format!("{name}.nui")
            };
        let entry = crate::resources::resolve_ui_entry(instrument, &requested, 1024 * 1024)
            .map_err(anyhow::Error::msg)?
            .with_context(|| format!("Native UI entry missing: {requested}"))?;
        ensure!(
            entry.dialect == crate::resources::UiDialect::LegacyLua,
            "Unsupported typed Komplete UI entry {} (target version {:?}); legacy Lua execution is unavailable for .kscript",
            entry.file,
            entry.target_version
        );
        let root_name = entry
            .file
            .strip_suffix(".nui")
            .context("Legacy native UI entry extension")?
            .replace('/', ".");
        ensure!(
            module_name(&root_name),
            "Invalid native UI root name {name:?}"
        );
        let identity = format!("native_ui/{}", entry.file);
        let target_version = entry.target_version.clone();
        ensure!(
            exposed,
            "Native UI controls have not been exposed with expose_controls"
        );
        ensure!(rt.native_control_identifiers().count() <= 4096, "Native UI registry exceeds 4096 controls");
        let mut live = rt.live();
        rt.prepare_native_controls(&mut live);
        let mut p = Parameters::default();
        // NI expose_controls resolves identical identifiers to the first slot.
        for c in &live.native_controls {
            p.bindings.entry(c.name.clone()).or_insert(Binding {
                kind: c.kind.clone(),
                slot: c.slot,
                control: c.control,
                value: c.value,
                meter_control: c.kind.as_ref() == "ui_level_meter",
                meter: c.meter,
                range: c.range,
                properties: c.properties.clone(),
            });
        }
        p.unsupported.extend(rt.native_control_identifiers()
            .filter(|name| !p.bindings.contains_key(*name)).map(str::to_owned));
        ensure!(
            p.bindings.len() + p.unsupported.len() <= 4096,
            "Native UI scalar registry exceeds 4096 controls"
        );
        let parameters = Arc::new(Mutex::new(p));
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
            LuaOptions::default(),
        )?;
        lua.set_memory_limit(64 * 1024 * 1024)?;
        for name in ["dofile", "loadfile", "load", "collectgarbage", "xpcall"] {
            lua.globals().set(name, Value::Nil)?;
        }
        let instructions = Arc::new(AtomicUsize::new(0));
        let budget = instructions.clone();
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(1000),
            move |_, _| {
                if budget.fetch_add(1000, Ordering::Relaxed) >= 100_000 {
                    return Err(invalid("Native UI instruction budget exceeded"));
                }
                Ok(VmState::Continue)
            },
        )?;
        let revision = Arc::new(AtomicU64::new(1));
        let budget = instructions.clone();
        lua.globals().set(
            "native_budget_check",
            lua.create_function(move |_, ()| {
                if budget.load(Ordering::Relaxed) >= 100_000 {
                    return Err(invalid("Native UI instruction budget exceeded"));
                }
                Ok(())
            })?,
        )?;
        let changed = revision.clone();
        lua.globals().set(
            "native_changed",
            lua.create_function(move |_, ()| {
                changed.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })?,
        )?;
        let p = parameters.clone();
        lua.globals().set(
            "native_connect",
            lua.create_function(move |_, name: String| {
                let p = p.lock().unwrap();
                let b = p.bindings.get(&name).ok_or_else(|| {
                    invalid(format!(
                        "Unknown or unsupported exposed scalar control: {name}"
                    ))
                })?;
                Ok((b.slot, b.control))
            })?,
        )?;
        let p = parameters.clone();
        let source = instrument.to_owned();
        lua.globals().set("native_read", lua.create_function(move |_, name: String| {
            let mut p = p.lock().unwrap();
            if let Some(b) = p.bindings.get(&name) {
                let value = b.lua_value()?;
                return Ok((value, Some(b.slot), Some(b.control)));
            }
            if p.unsupported.contains(&name) {
                return Err(invalid(format!("Unsupported exposed control connection kind: {name}")));
            }
            // The native legacy registry returns an i32-zero variant on an
            // exact-name miss. Do not invent a control owner or accept a write.
            use std::hash::{Hash, Hasher};
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            name.hash(&mut hash);
            let key = hash.finish();
            let warning = if p.missing_reads.contains(&key) { None }
                else if p.missing_reads.len() < 64 {
                    p.missing_reads.insert(key);
                    Some(format!("Missing exposed scalar binding {:?}: legacy reads return integer 0; writes remain unavailable", name.chars().take(320).collect::<String>()))
                } else if p.warnings.len() == 64 {
                    Some("Further missing native UI binding warnings omitted (64 distinct names retained)".into())
                } else { None };
            if let Some(warning) = warning {
                p.warnings.push(warning.clone());
                drop(p);
                crate::diagnostics::resource(&source, "native_ui/parameter", &warning);
            }
            Ok((Value::Integer(0), None, None))
        })?)?;
        let p = parameters.clone();
        lua.globals().set("native_control_property", lua.create_function(move |lua, (name, property, index): (String, Value, Option<Value>)| {
            fn integer(value: Value) -> mlua::Result<i32> {
                match value {
                    Value::Integer(v) => i32::try_from(v).map_err(|_| invalid("Native control property argument exceeds i32")),
                    Value::Number(v) if v.is_finite() && v.fract()==0. && (i32::MIN as f64..=i32::MAX as f64).contains(&v) => Ok(v as i32),
                    _ => Err(invalid("Native control property arguments require finite whole integers")),
                }
            }
            let property=integer(property)?;
            let index=integer(index.unwrap_or(Value::Integer(0)))?;
            if !matches!(property,0|14|16|17|18|19|20|21|22) {
                return Err(invalid(format!("Unsupported native KSP control property: {property}")));
            }
            let p=p.lock().unwrap();
            let Some(b)=p.bindings.get(&name) else {
                drop(p);
                // This registry's miss branch uses the same integer-zero
                // variant as scalar reads. Reuse their bounded named warning;
                // declared unsupported kinds still raise instead of becoming0.
                let (value,_,_)=lua.globals().get::<Function>("native_read")?.call::<(Value,Option<usize>,Option<usize>)>(name)?;
                return Ok(value);
            };
            let props=&b.properties;
            let item=usize::try_from(index).ok().filter(|i|*i<props.menu_len).and_then(|i|props.menu.get(i));
            // Legacy Lua uses this enum, distinct from synthetic KSP symbol IDs.
            // Native common-getter dispatch and actual resource callers establish
            // these contracts; labels stay authored strings, never value formats.
            match property {
                0 => Ok(Value::String(lua.create_string(&props.text)?)),
                14 => Ok(Value::String(lua.create_string(&props.label)?)),
                16 => Ok(Value::Integer(i64::from(props.default_value))),
                17 => Ok(Value::Integer(props.menu_len as i64)),
                18 => Ok(Value::String(lua.create_string(item.map_or("",|m|m.text.as_str()))?)),
                19 => Ok(Value::Integer(i64::from(item.map_or(0,|m|m.value)))),
                20 => Ok(Value::Boolean(item.is_some_and(|m|m.visible))),
                21 => Ok(Value::Integer(i64::from(b.range.map_or(0,|r|r.0)))),
                22 => Ok(Value::Integer(i64::from(b.range.map_or(0,|r|r.1)))),
                _ => unreachable!(),
            }
        })?)?;
        let p = parameters.clone();
        let levels = rt.output_meters.clone();
        lua.globals().set("native_connect_level_meter", lua.create_function(move |lua, name: String| {
            let mut p = p.lock().unwrap();
            let binding = p.bindings.get(&name).ok_or_else(|| invalid(format!("Unknown exposed level meter: {name}")))?;
            if !binding.meter_control { return Err(invalid(format!("Level meter connection requires ui_level_meter: {name}"))); }
            let identity = (binding.slot, binding.control);
            let attachment = binding.meter;
            if !p.meters.contains_key(&name) {
                if p.meters.len() >= 256 { return Err(invalid("Native UI meter subscription count exceeds 256")); }
                let reader = attachment.and_then(|target| levels.subscribe(target)).ok_or_else(|| invalid(format!("Unsupported or unattached KSP meter tap: {name}, {attachment:?}")))?;
                p.meters.insert(name.clone(), MeterBinding { slot: identity.0, control: identity.1, reader });
            }
            let reader = p.meters[&name].reader.clone();
            drop(p);
            let object = lua.create_table()?;
            object.set("level_value", lua.create_function(move |_, _: Value| {
                reader.level().ok_or_else(|| invalid(format!("KSP meter {name} was reattached to an unsupported tap")))
            })?)?;
            Ok(object)
        })?)?;
        let p = parameters.clone();
        lua.globals().set(
            "native_value",
            lua.create_function(move |_, (slot, control): (usize, usize)| {
                let p = p.lock().unwrap();
                p.bindings
                    .values()
                    .find(|b| b.slot == slot && b.control == control)
                    .ok_or_else(|| invalid("Native UI control identity changed"))?
                    .lua_value()
            })?,
        )?;
        let p = parameters.clone();
        let changed = revision.clone();
        lua.globals().set(
            "native_set",
            lua.create_function(move |_, (slot, control, value): (usize, usize, Value)| {
                let mut p = p.lock().unwrap();
                if !p.gesture_active {
                    return Err(invalid(
                        "Native UI parameter writes require a gesture callback",
                    ));
                }
                if p.edits.len() >= 64 {
                    return Err(invalid("Native UI edit batch exceeds 64"));
                }
                let b = p
                    .bindings
                    .values_mut()
                    .find(|b| b.slot == slot && b.control == control)
                    .ok_or_else(|| invalid("Native UI control identity changed"))?;
                let value = b.assigned_value(value)?;
                b.value = value;
                p.edits.push(Edit {
                    slot,
                    control,
                    value,
                });
                changed.fetch_add(1, Ordering::Relaxed);
                Ok(())
            })?,
        )?;
        lua.globals().set(
            "native_component_validate",
            lua.create_function(|_, target: Function| {
                let info = target.info();
                if info.what != "Lua" {
                    return Err(invalid("Native component targets require Lua functions"));
                }
                Ok(())
            })?,
        )?;
        #[cfg(not(test))]
        let host = include_str!("host.lua");
        #[cfg(test)]
        let host = test_host::load_from_environment()?;
        #[cfg(test)]
        let host = host.text.as_ref();
        lua.load(host)
            .set_name("@kontra/native-ui-host")
            .exec()?;
        let loaded = lua.create_table()?;
        loaded.set("native_ui", lua.globals().get::<Table>("native_ui_host")?)?;
        loaded.set("kontakt", lua.globals().get::<Table>("kontakt_host")?)?;
        loaded.set("native_ui_util", lua.globals().get::<Table>("native_ui_util_host")?)?;
        let loading = Arc::new(Mutex::new(BTreeSet::<String>::new()));
        struct Modules {
            resources: crate::resources::Resources,
            entry: Option<crate::resources::UiEntry>,
            count: usize,
            bytes: usize,
            sealed: bool,
            used: Vec<(String, Vec<u8>)>,
        }
        let modules = Arc::new(Mutex::new(Modules {
            resources: crate::resources::Resources::of(instrument, "native_ui"),
            entry: Some(entry),
            count: 0,
            bytes: 0,
            sealed: false,
            used: Vec::new(),
        }));
        let loader = modules.clone();
        let cache = loaded.clone();
        lua.globals().set(
            "require",
            lua.create_function(move |lua, name: String| {
                if !module_name(&name) {
                    return Err(invalid("Invalid native UI module name"));
                }
                let cached = cache.get::<Value>(name.as_str())?;
                if !matches!(cached, Value::Nil) {
                    return Ok(cached);
                }
                if !loading.lock().unwrap().insert(name.clone()) {
                    return Err(invalid(format!("Circular native UI module: {name}")));
                }
                let result = (|| {
                    let path = name.replace('.', "/");
                    let mut file = format!("{path}.nui");
                    let mut modules = loader.lock().unwrap();
                    if modules.sealed {
                        return Err(invalid(format!(
                            "Late native UI module loading is unavailable: {name}"
                        )));
                    }
                    if modules.count >= 256 || modules.bytes >= 8 * 1024 * 1024 {
                        return Err(invalid("Native UI resource budget exceeded"));
                    }
                    let limit = (8 * 1024 * 1024 - modules.bytes).min(1024 * 1024);
                    let bytes = if modules
                        .entry
                        .as_ref()
                        .is_some_and(|entry| entry.file == file)
                    {
                        modules.entry.take().unwrap().source
                    } else {
                        match modules
                            .resources
                            .read_limited(&file, limit)
                            .map_err(invalid)?
                        {
                            Some(bytes) => bytes,
                            None => {
                                file = format!("{path}/init.nui");
                                modules
                                    .resources
                                    .read_limited(&file, limit)
                                    .map_err(invalid)?
                                    .ok_or_else(|| {
                                        invalid(format!(
                                            "Native UI module missing: {path}.nui or {file}"
                                        ))
                                    })?
                            }
                        }
                    };
                    if bytes.len() > 1024 * 1024
                        || modules.count >= 256
                        || modules.bytes + bytes.len() > 8 * 1024 * 1024
                    {
                        return Err(invalid("Native UI resource budget exceeded"));
                    }
                    modules.count += 1;
                    modules.bytes += bytes.len();
                    modules.used.push((file.clone(), bytes.clone()));
                    drop(modules);
                    let source = std::str::from_utf8(&bytes).map_err(invalid)?;
                    let lowered = frontend::lower(source).map_err(|error| {
                        invalid(format!("Native UI resource {file}: {error:#}"))
                    })?;
                    let value = lua
                        .load(&lowered)
                        .set_name(format!("@native_ui/{file}.lowered.lua"))
                        .set_mode(mlua::chunk::ChunkMode::Text)
                        .eval::<Value>()?;
                    let value = if matches!(value, Value::Nil) {
                        Value::Boolean(true)
                    } else {
                        value
                    };
                    cache.set(name.as_str(), value.clone())?;
                    Ok(value)
                })();
                loading.lock().unwrap().remove(&name);
                result
            })?,
        )?;
        let module: Value = lua.globals().get::<Function>("require")?.call(root_name)?;
        let root = match module {
            Value::Function(f) => f.call::<Table>(())?,
            Value::Table(t) => t,
            _ => bail!("Native UI root must return a scene or component function"),
        };
        let mut scene = Self {
            lua,
            root,
            parameters,
            instructions,
            revision,
            output_meters: rt.output_meters.clone(),
            gestures: Vec::new(),
            hovers: Vec::new(),
            drops: Vec::new(),
            identities: BTreeMap::new(),
            assets: Arc::default(),
            asset_pending: BTreeSet::new(),
            raster_waker: None,
            raster_pending: Arc::new(AtomicBool::new(false)),
            identity,
            target_version,
            resources: Vec::new(),
        };
        // Authored component bodies expand lazily within their context. Their
        // first require calls still belong to this off-thread preparation.
        let root = scene.snapshot_raw().context("Native UI scene validation")?;
        scene.resources = {
            let mut m = modules.lock().unwrap();
            m.sealed = true;
            std::mem::take(&mut m.used)
        };
        let names = root.asset_names();
        ensure!(names.len() <= 256, "Native UI asset count exceeds 256");
        scene.assets = Arc::new(crate::native_assets::Assets::prepare_bounded(
            instrument,
            names,
            128 << 20,
        ));
        crate::ui::native_scene::validate_root(&root, &scene.assets)?;
        Ok(Some(scene))
    }
    pub(crate) fn reserve_assets(&mut self, names: BTreeSet<String>) -> Result<Vec<String>> {
        let missing: Vec<_> = names
            .into_iter()
            .filter(|name| {
                !self.assets.entries.contains_key(name) && !self.asset_pending.contains(name)
            })
            .collect();
        ensure!(
            self.assets.entries.len() + self.asset_pending.len() + missing.len() <= 256,
            "Native UI asset count exceeds 256"
        );
        self.asset_pending.extend(missing.iter().cloned());
        Ok(missing)
    }
    pub(crate) fn remaining_asset_bytes(&self) -> usize {
        (128usize << 20).saturating_sub(
            self.assets
                .entries
                .values()
                .filter_map(|result| result.as_ref().ok())
                .map(|asset| asset.retained_bytes())
                .fold(0usize, usize::saturating_add),
        )
    }
    pub(crate) fn cancel_assets(&mut self, names: &[String]) {
        for name in names {
            self.asset_pending.remove(name);
        }
    }
    pub(crate) fn install_assets(&mut self, ready: crate::native_assets::Assets) {
        let mut all = (*self.assets).clone();
        for (name, result) in ready.entries {
            if let Some(wake) = &self.raster_waker
                && let Ok(asset) = &result
            {
                asset.set_raster_waker(wake.clone());
            }
            self.asset_pending.remove(&name);
            all.entries.insert(name, result);
        }
        self.assets = Arc::new(all);
        self.revision.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn set_raster_waker(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        if self.raster_waker.is_some() {
            return;
        }
        let pending = self.raster_pending.clone();
        let revision = self.revision.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            if !pending.swap(true, Ordering::AcqRel) {
                revision.fetch_add(1, Ordering::Relaxed);
                wake();
            }
        });
        for asset in self
            .assets
            .entries
            .values()
            .filter_map(|result| result.as_ref().ok())
        {
            asset.set_raster_waker(wake.clone());
        }
        self.raster_waker = Some(wake);
    }
    pub(crate) fn rasters_pending(&self) -> bool {
        self.raster_pending.load(Ordering::Acquire)
    }
    pub(crate) fn begin_rasters(&self) -> Arc<crate::native_assets::Assets> {
        self.raster_pending.store(false, Ordering::Release);
        self.assets.clone()
    }
    pub(crate) fn rasters_ready(&self) {
        self.revision.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn has_control(&self, slot: usize, control: usize) -> bool {
        self.parameters
            .lock()
            .unwrap()
            .bindings
            .values()
            .any(|b| b.slot == slot && b.control == control)
    }
    pub(crate) fn revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed).wrapping_add(self.output_meters.revision())
    }
    pub(crate) fn connection_warnings(&self) -> Vec<String> {
        self.parameters.lock().unwrap().warnings.clone()
    }
    pub(crate) fn drop_source_warning(&self) -> Option<&'static str> {
        self.drops.iter().flatten().any(|target| target.formats.iter().any(|format|format!="text/uri-list"))
            .then_some("Native UI file drops supply text/uri-list; other MIME formats require a matching typed drag source. The NI internal clipboard producer is unavailable.")
    }
    pub(crate) fn observe(&self, live: &Live) {
        let mut p = self.parameters.lock().unwrap();
        for c in &live.native_controls {
            if let Some(b) = p.bindings.get_mut(&c.name)
                && b.slot == c.slot
                && b.control == c.control
                && (b.value != c.value || b.meter != c.meter || b.range != c.range || b.properties != c.properties)
            {
                b.value = c.value;
                b.meter = c.meter;
                b.range = c.range;
                if b.properties != c.properties { b.properties = c.properties.clone(); }
                self.revision.fetch_add(1, Ordering::Relaxed);
            }
            if let Some(meter) = p.meters.get(&c.name)
                && meter.slot == c.slot && meter.control == c.control
            { meter.reader.reattach(c.meter); }
        }
    }
    pub(crate) fn snapshot(&mut self) -> Result<Node> {
        let node = self.snapshot_raw()?;
        crate::ui::native_scene::validate_root(&node, &self.assets)?;
        Ok(node)
    }
    fn snapshot_raw(&mut self) -> Result<Node> {
        self.instructions.store(0, Ordering::Relaxed);
        for gesture in self.gestures.iter_mut().flatten() { gesture.present = false; }
        for target in self.drops.iter_mut().flatten() { target.present = false; }
        for hover in self.hovers.iter_mut().flatten() {
            hover.present = false;
        }
        let mut seen = BTreeSet::new();
        let root = self.root.clone();
        let mut retained = Vec::new();
        let node = self.node(
            root,
            self.lua.create_table()?,
            (14., [1.; 4], false, false, 1., false, None),
            "0".into(),
            0,
            &mut seen,
            &mut retained,
        )?;
        ensure!(node.text_bytes()<=1<<20,"Native UI text layout exceeds 1 MiB");
        Ok(node)
    }
    fn node(
        &mut self,
        node: Table,
        context: Table,
        style: (f64, [f32; 4], bool, bool, f32, bool, Option<usize>),
        path: String,
        depth: usize,
        seen: &mut BTreeSet<usize>,
        retained: &mut Vec<Table>,
    ) -> Result<Node> {
        ensure!(
            depth <= 64 && seen.len() < 4096,
            "Native UI scene budget exceeded"
        );
        ensure!(
            seen.insert(node.to_pointer() as usize),
            "Aliased native UI node identity"
        );
        // Keep visited tables alive so Lua GC cannot reuse a checked pointer.
        retained.push(node.clone());
        let expand: Function = self.lua.globals().get("native_expand")?;
        let (node, context): (Table, Table) = expand.call((node, path.clone(), context))?;
        // The existing bounded component state owner distinguishes a replaced
        // component at the same child path; its old hover exit stays owned by it.
        let path = if let Some(owner) = node.get::<Option<usize>>("owner")? {
            format!("{path}@{owner}")
        } else {
            path
        };
        ensure!(
            self.identities.len() < 4096 || self.identities.contains_key(&path),
            "Native UI identity budget exceeded"
        );
        let next = self.identities.len();
        let id = *self.identities.entry(path.clone()).or_insert(next);
        let props: Table = node.get("props")?;
        let modifiers: Table = node.get("modifiers")?;
        let background = modifiers.get::<Option<Table>>("background")?.is_some();
        let overlay = modifiers.get::<Option<Table>>("overlay")?.is_some();
        let rotation = if let Some(modifier) = modifiers.get::<Option<Table>>("rotation")? {
            let value = match modifier.get::<Value>("value")? {
                Value::Function(f) => f.call::<Value>(())?, value => value,
            };
            let degrees = match value {
                Value::Integer(value) => value as f64,
                Value::Number(value) => value,
                _ => bail!("Native UI rotation requires a number"),
            };
            ensure!(degrees.is_finite(), "Invalid native UI rotation");
            // Legacy .nui uses degrees. MUI rotates the entire wrapper subtree,
            // retaining its layout box and the authored modifier order.
            Some((degrees % 360.).to_radians())
        } else { None };
        let (mut text_size, mut text_color, mut hidden, mut disabled, mut opacity, mut text_bold, mut text_lines) = style;
        if let Some(modifier) = modifiers.get::<Option<Table>>("opacity")? {
            let value = match modifier.get::<Value>("value")? {
                Value::Function(f) => f.call::<Value>(())?, value => value,
            };
            let value = match value {
                Value::Integer(value) => value as f64,
                Value::Number(value) => value,
                _ => bail!("Native UI opacity requires a number"),
            };
            ensure!(value.is_finite() && (0. ..=1.).contains(&value), "Invalid native UI opacity");
            opacity *= value as f32;
        }
        if let Some(modifier) = modifiers.get::<Option<Table>>("hidden")? {
            let value = match modifier.get::<Value>("value")? {
                Value::Function(f) => f.call::<Value>(())?, value => value,
            };
            let Value::Boolean(value) = value else { bail!("Native UI hidden requires a boolean"); };
            hidden |= value;
        }
        if let Some(modifier) = modifiers.get::<Option<Table>>("disabled")? {
            let value = match modifier.get::<Value>("value")? {
                Value::Function(f) => f.call::<Value>(())?, value => value,
            };
            let Value::Boolean(value) = value else { bail!("Native UI disabled requires a boolean"); };
            disabled |= value;
        }
        if let Some(modifier) = modifiers.get::<Option<Table>>("bold")? {
            let value = match modifier.get::<Value>("value")? {
                Value::Function(f) => f.call::<Value>(())?, value => value,
            };
            match value { Value::Nil => (), Value::Boolean(value) => text_bold=value,
                _ => bail!("Native UI bold requires a boolean"), }
        }
        if let Some(modifier) = modifiers.get::<Option<Table>>("line_limit")? {
            let value = match modifier.get::<Value>("value")? {
                Value::Function(f) => f.call::<Value>(())?, value => value,
            };
            match value { Value::Nil => (), Value::Integer(value) if (1..=256).contains(&value) => text_lines=Some(value as usize),
                Value::Number(value) if value.is_finite() && value.fract()==0. && (1. ..=256.).contains(&value) => text_lines=Some(value as usize),
                _ => bail!("Invalid native UI line_limit bounds"), }
        }
        let mut offset = [0.; 2];
        if let Some(modifier) = modifiers.get::<Option<Table>>("offset")? {
            for (index, name) in ["x", "y"].into_iter().enumerate() {
                let value = match modifier.get::<Value>(name)? {
                    Value::Function(f) => f.call::<Value>(())?, value => value,
                };
                offset[index] = match value {
                    Value::Nil => 0., Value::Integer(value) => value as f64,
                    Value::Number(value) => value,
                    _ => bail!("Native UI offset requires numeric coordinates"),
                };
            }
        }
        ensure!(offset.iter().all(|v|v.is_finite() && (-4096. ..=4096.).contains(v)), "Invalid native UI offset bounds");
        let padding = if let Some(p) = modifiers.get::<Option<Table>>("padding")? {
            Insets { left: p.get("left")?, right: p.get("right")?, top: p.get("top")?, bottom: p.get("bottom")? }
        } else { Insets::ZERO };
        ensure!([padding.left,padding.right,padding.top,padding.bottom].iter().all(|v|v.is_finite() && (0. ..=4096.).contains(v)),"Invalid native UI padding");
        for name in ["font_size", "foreground_color"] {
            if let Some(modifier) = modifiers.get::<Option<Table>>(name)? {
                let value = modifier.get::<Value>("value")?;
                let value = match value { Value::Function(f) => f.call::<Value>(())?, v => v };
                if matches!(value, Value::Nil) { continue; }
                if name == "font_size" {
                    text_size = match value {
                        Value::Integer(value) => value as f64,
                        Value::Number(value) => value,
                        _ => bail!("Native UI font_size requires a number"),
                    };
                    ensure!(text_size.is_finite() && (0. ..=4096.).contains(&text_size), "Invalid native UI font size");
                } else {
                    let Value::Table(color) = value else { bail!("Native UI foreground_color requires a color"); };
                    text_color = [color.get::<f32>("r")? / 255., color.get::<f32>("g")? / 255.,
                        color.get::<f32>("b")? / 255., color.get::<Option<f32>>("a")?.unwrap_or(1.)];
                    ensure!(text_color.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v)), "Invalid native UI foreground color");
                }
            }
        }
        let paint = match node.get::<String>("kind")?.as_str() {
            "ZStack" => Paint::Stack,
            "Spacer" => {
                let value = match props.get::<Value>("min_length")? {
                    Value::Function(f) => f.call::<Value>(())?, value => value,
                };
                let minimum = match value {
                    Value::Nil => 4., Value::Integer(value) => value as f64,
                    Value::Number(value) => value,
                    _ => bail!("Native UI spacer min_length requires a number"),
                };
                ensure!(minimum.is_finite() && (0. ..=4096.).contains(&minimum), "Invalid native UI spacer min_length");
                Paint::Spacer(minimum)
            },
            kind @ ("HStack" | "VStack") => {
                let spacing = match props.get::<Value>("spacing")? {
                    Value::Function(f) => f.call::<Value>(())?, value => value,
                };
                let spacing = match spacing {
                    Value::Nil => 0., Value::Integer(value) => value as f64,
                    Value::Number(value) => value,
                    _ => bail!("Native UI stack spacing requires a number"),
                };
                ensure!(spacing.is_finite() && (0. ..=4096.).contains(&spacing), "Invalid or unsupported negative native UI stack spacing");
                if kind == "HStack" { Paint::HStack(spacing) } else { Paint::VStack(spacing) }
            }
            "Text" => {
                let value = props.get::<Value>("text")?;
                let value = match value {
                    Value::Function(f) => f.call::<Value>(())?,
                    v => v,
                };
                let text = match value {
                    Value::String(s) => s.to_str()?.to_string(),
                    _ => bail!("Native UI Text requires string content"),
                };
                ensure!(text.len() <= 8192, "Native UI text exceeds 8 KiB");
                Paint::Text(text)
            }
            "Rectangle" => {
                let color: Table = props.get("color")?;
                let radius = props.get::<Option<f64>>("corner_radius")?.unwrap_or(0.);
                ensure!(radius.is_finite() && (0. ..=4096.).contains(&radius),"Invalid native UI corner radius");
                Paint::Rectangle([
                    color.get::<f32>("r")? / 255.,
                    color.get::<f32>("g")? / 255.,
                    color.get::<f32>("b")? / 255.,
                    color.get::<Option<f32>>("a")?.unwrap_or(1.),
                ], radius)
            }
            "Ring" => {
                let color: Table = props.get("color")?;
                let start = props.get::<Option<f64>>("start_angle")?.unwrap_or(0.);
                let angle = props.get::<Option<f64>>("angle")?.unwrap_or(360.);
                let thickness = props.get::<Option<f64>>("thickness")?.unwrap_or(4.);
                ensure!(start.is_finite() && angle.is_finite() && (-360. ..=360.).contains(&angle), "Invalid or unsupported native UI Ring angle");
                ensure!(thickness.is_finite() && (0. ..=4096.).contains(&thickness), "Invalid native UI Ring thickness");
                Paint::Ring { color: [color.get::<f32>("r")? / 255., color.get::<f32>("g")? / 255., color.get::<f32>("b")? / 255., color.get::<Option<f32>>("a")?.unwrap_or(1.)], start: start.rem_euclid(360.), angle, thickness }
            }
            "Image" => {
                let value = props.get::<Value>("file")?;
                let value = match value {
                    Value::Function(f) => f.call::<Value>(())?,
                    v => v,
                };
                let Value::String(file) = value else {
                    bail!("Native UI Image.file requires a string");
                };
                let resource = file.to_str()?.to_string();
                ensure!(
                    !resource.is_empty() && resource.len() <= 1024,
                    "Invalid native UI Image.file length"
                );
                Paint::Asset(crate::native_assets::AssetRef {
                    resource,
                    resizable: props.get::<Option<bool>>("resizable")?.unwrap_or(false),
                })
            }
            kind => bail!("Unsupported native UI primitive: {kind}"),
        };
        let frame: Option<Table> = modifiers.get("frame")?;
        let mut alignment = (Align::Center, Align::Center);
        if let Some(name) = frame
            .as_ref()
            .map(|frame| frame.get::<Option<String>>("alignment"))
            .transpose()?
            .flatten()
            .or(props.get::<Option<String>>("alignment")?)
        {
            let (parsed, horizontal, vertical) = parse_alignment(&name)?;
            ensure!(!matches!(paint, Paint::HStack(_)) || !horizontal, "Native UI HStack requires vertical alignment");
            ensure!(!matches!(paint, Paint::VStack(_)) || !vertical, "Native UI VStack requires horizontal alignment");
            alignment = parsed;
        }
        let width = frame
            .as_ref()
            .map(|t| t.get::<Option<f64>>("width"))
            .transpose()?
            .flatten();
        let height = frame
            .as_ref()
            .map(|t| t.get::<Option<f64>>("height"))
            .transpose()?
            .flatten();
        ensure!(
            width
                .into_iter()
                .chain(height)
                .all(|v| v.is_finite() && (0. ..=4096.).contains(&v)),
            "Invalid native UI frame"
        );
        let mut minimum = [None; 2];
        let mut maximum = [None; 2];
        if let Some(frame) = &frame {
            for (axis,name) in ["width","height"].into_iter().enumerate() {
                minimum[axis]=frame.get::<Option<f64>>(format!("min_{name}"))?;
                maximum[axis]=frame.get::<Option<f64>>(format!("max_{name}"))?;
                ensure!(minimum[axis].is_none_or(|v|v.is_finite() && (0. ..=4096.).contains(&v)),"Invalid native UI minimum frame bound");
                // Authentic legacy modules use math.huge only as an unbounded
                // maximum. Normalize it to absence; measured roots remain4096.
                if maximum[axis]==Some(f64::INFINITY) { maximum[axis]=None; }
                ensure!(maximum[axis].is_none_or(|v|v.is_finite() && (0. ..=4096.).contains(&v)),"Invalid native UI maximum frame bound");
                ensure!(!frame.contains_key(name)? || (!frame.contains_key(format!("min_{name}"))? && !frame.contains_key(format!("max_{name}"))?),"Native UI frame cannot mix fixed and ranged dimensions on one axis");
                ensure!(minimum[axis].unwrap_or(0.) <= maximum[axis].unwrap_or(4096.),"Conflicting native UI frame bounds");
            }
        }
        if let Paint::Rectangle(color, _) | Paint::Ring { color, .. } = &paint {
            ensure!(
                color
                    .iter()
                    .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
                "Invalid native UI color"
            );
        }
        let tap: Option<Table> = modifiers.get("on_tap_gesture")?;
        let drag: Option<Table> = modifiers.get("on_drag_gesture")?;
        let has_tap = (tap.is_some() || drag.is_some()) && !hidden && !disabled;
        if tap.is_some() || drag.is_some() {
            let callbacks = PointerCallbacks {
                tap: if let Some(t) = tap { [t.get("start")?, t.get("double")?, t.get("complete")?] } else { [None,None,None] },
                drag: if let Some(t) = &drag { [t.get("start")?, t.get("update")?, t.get("complete")?] } else { [None,None,None] },
                threshold: drag.map(|t|t.get::<Option<f64>>("threshold")).transpose()?.flatten().unwrap_or(1.),
            };
            ensure!(callbacks.threshold.is_finite() && (0. ..=4096.).contains(&callbacks.threshold), "Invalid native UI drag threshold");
            if self.gestures.len() <= id { self.gestures.resize_with(id+1,||None); }
            let entry = self.gestures[id].get_or_insert(PointerGesture { callbacks: PointerCallbacks::default(), present: true, enabled: false, capture: None });
            entry.callbacks = callbacks;
            entry.present = true;
            entry.enabled = has_tap;
        }
        let hover: Option<Table> = modifiers.get("on_hover_gesture")?;
        let has_hover = hover.is_some() && !hidden && !disabled;
        if let Some(hover) = hover {
            let callbacks = [hover.get("enter")?, hover.get("move")?, hover.get("exit")?];
            if self.hovers.len() <= id {
                self.hovers.resize_with(id + 1, || None);
            }
            let entry = self.hovers[id].get_or_insert(Hover {
                callbacks: [None, None, None],
                present: true,
                enabled: false,
                inside: false,
                last: None,
            });
            entry.callbacks = callbacks;
            entry.present = true;
            entry.enabled = has_hover;
        }
        let children: Table = node.get("children")?;
        let drop:Option<Table>=modifiers.get("on_drop")?;
        let has_drop=drop.is_some() && !hidden && !disabled;
        if let Some(drop)=drop {
            let formats=drop.get::<Table>("of")?.sequence_values::<String>().collect::<mlua::Result<Vec<_>>>()?;
            ensure!(!formats.is_empty() && formats.len()<=16 && formats.iter().all(|format|drops::valid_format(format)), "Invalid native UI drop formats");
            let unique:BTreeSet<_>=formats.iter().collect();
            ensure!(unique.len()==formats.len(), "Duplicate native UI drop format");
            let target=DropTarget {action:drop.get("action")?,formats,present:true,enabled:has_drop};
            if self.drops.len()<=id {self.drops.resize_with(id+1,||None);}
            self.drops[id]=Some(target);
        }
        let mut nodes = Vec::new();
        let expand_children: Function = self.lua.globals().get("native_children")?;
        let (children, groups): (Table, Table) = expand_children.call((children, path))?;
        for group in groups.sequence_values::<Table>() {
            let group = group?;
            ensure!(seen.len() < 4096, "Native UI scene budget exceeded");
            ensure!(seen.insert(group.to_pointer() as usize), "Aliased native UI node identity");
            retained.push(group);
        }
        for child in children.sequence_values::<Table>() {
            let child = child?;
            nodes.push(self.node(
                child.get("node")?,
                context.clone(),
                (text_size, text_color, hidden, disabled, opacity, text_bold, text_lines),
                child.get("path")?,
                depth + 1,
                seen,
                retained,
            )?);
        }
        ensure!((!background && !overlay) || nodes.len()==2,"Invalid native UI decoration wrapper");
        let anchor = if let Some(modifier) = modifiers.get::<Option<Table>>("align")? {
            let value = match modifier.get::<Value>("value")? {
                Value::Function(f) => f.call::<Value>(())?, value => value,
            };
            let Value::String(name) = value else { bail!("Native UI align requires an alignment string"); };
            Some(parse_alignment(&name.to_str()?)?.0)
        } else if frame.is_none() && matches!(paint, Paint::Stack) && nodes.len()==1 {
            // Transparent modifier scopes retain the child's placement guide.
            // A frame consumes it for its child's placement, then forms a new box.
            nodes[0].anchor
        } else { None };
        let transparent = frame.is_none() && matches!(paint, Paint::Stack)
            && nodes.len()==1 && !background && !overlay && !props.contains_key("alignment")?;
        if let Some(guide) = anchor {
            for child in &mut nodes { child.inherit_placement(guide); }
        }
        Ok(Node {
            id,
            paint,
            width,
            height,
            minimum,
            maximum,
            text_size,
            text_color,
            text_bold,
            text_lines,
            alignment,
            anchor,
            transparent,
            // A frame/component modifier wraps a Spacer but does not change the
            // stack whose remaining main-axis space it consumes. Explicit
            // frame dimensions still bound expansion in the render adapter.
            spacer_scope: transparent || (frame.is_some() && nodes.len()==1 && !background && !overlay),
            padding,
            offset,
            rotation,
            hidden,
            disabled,
            opacity,
            background,
            overlay,
            tap: has_tap,
            hover: has_hover,
            drop: has_drop,
            children: nodes,
        })
    }
    pub(crate) fn take_edits(&self) -> Vec<Edit> {
        std::mem::take(&mut self.parameters.lock().unwrap().edits)
    }
    /// Follow the real displayed target ancestry; only a matching enabled
    /// owner can consume a drop. Layout/rotation/clipping stay owned by MUI.
    pub(crate) fn drop_target(
        &self,
        ui: &moose::mui::mui::Ui,
        prefix: &str,
        key: &str,
        payload: &DropPayload,
    ) -> Option<usize> {
        let scene = ui.scene()?;
        let mut surface = scene.surface(key)?;
        for _ in 0..=64 {
            let id = surface
                .key
                .as_str()
                .strip_prefix(prefix)?
                .strip_prefix('-')?
                .parse::<usize>()
                .ok()?;
            if !surface.disabled
                && self
                    .drops
                    .get(id)
                    .and_then(Option::as_ref)
                    .is_some_and(|target| {
                        target.present && target.enabled && payload.matches(&target.formats)
                    })
            {
                return Some(id);
            }
            surface = scene.surface(surface.parent.as_ref()?.as_str())?;
        }
        None
    }
    pub(crate) fn perform_drop(
        &mut self,
        ui: &moose::mui::mui::Ui,
        prefix: &str,
        id: usize,
        position: moose::mui::mui::prelude::Point,
        payload: DropPayload,
    ) -> Result<Vec<Edit>> {
        self.instructions.store(0, Ordering::Relaxed);
        self.parameters.lock().unwrap().edits.clear();
        let target = self
            .drops
            .get(id)
            .and_then(Option::as_ref)
            .filter(|target| target.present && target.enabled && payload.matches(&target.formats))
            .context("Native UI drop owner is unavailable or does not accept this format")?;
        let surface = ui
            .scene()
            .and_then(|scene| scene.surface(&format!("{prefix}-{id}")))
            .context("Native UI displayed drop frame missing")?;
        ensure!(!surface.disabled, "Native UI drop owner is disabled");
        let event = drops::Event {
            payload,
            position: surface.local(position),
            frame: surface.frame,
        };
        self.parameters.lock().unwrap().gesture_active = true;
        let result = self
            .lua
            .create_userdata(event)
            .and_then(|event| target.action.call::<()>(event));
        self.parameters.lock().unwrap().gesture_active = false;
        result?;
        Ok(self.take_edits())
    }
    /// Observe the actual displayed MUI scene. Parent and child hover regions
    /// each track their own enter/move/exit, including ancestor clipping.
    pub(crate) fn hover(&mut self, ui: &moose::mui::mui::Ui, prefix: &str) -> Result<Vec<Edit>> {
        self.instructions.store(0, Ordering::Relaxed);
        self.parameters.lock().unwrap().edits.clear();
        let pointer = ui.pointer();
        let mut hit = moose::mui::mui::input::Hit::default();
        // Retired owners exit before replacements enter at the same location.
        for retiring in [true, false] {
            for (id, entry) in self.hovers.iter_mut().enumerate() {
                let Some(hover) = entry else {
                    continue;
                };
                if (!hover.present || !hover.enabled) != retiring {
                    continue;
                }
                let surface = ui
                    .scene()
                    .and_then(|scene| scene.surface(&format!("{prefix}-{id}")));
                let input = pointer
                    .pos
                    .zip(surface)
                    .map(|(position, surface)| HoverInput {
                        position,
                        frame: surface.frame,
                        pose: surface.local_pose(),
                        mods: pointer.mods,
                    });
                let inside = if hover.present && hover.enabled {
                    if let (Some(input), Some(surface)) = (input, surface) {
                        hit.clear();
                        hit.push_placed(
                            "hover",
                            None,
                            &surface.path,
                            surface.offset,
                            surface.clip,
                            surface.clip_paths(),
                        )
                        .map_err(|error| {
                            anyhow::anyhow!("Native UI hover hit geometry: {error}")
                        })?;
                        !surface.disabled && hit.at(input.position).is_some()
                    } else {
                        false
                    }
                } else {
                    false
                };
                let phase = match (hover.inside, inside) {
                    (false, true) => Some(0),
                    (true, false) => Some(2),
                    (true, true) if input.zip(hover.last).is_some_and(|(now, last)| now.position != last.position) => Some(1),
                    _ => None,
                };
                hover.inside = inside;
                // Leaving the editor has no pointer position. Preserve the last
                // real position/frame, while using current keyboard modifiers.
                let event_input = input.or(hover.last.map(|mut last| {
                    if let Some(position) = pointer.pos { last.position = position; }
                    last.mods = pointer.mods;
                    last
                }));
                if inside {
                    hover.last = input;
                }
                if let Some(phase) = phase {
                    if let (Some(callback), Some(input)) = (&hover.callbacks[phase], event_input) {
                        let event =
                            hover_event(&self.lua, input, ["enter", "move", "exit"][phase])?;
                        self.parameters.lock().unwrap().gesture_active = true;
                        let result = callback.call::<()>(event);
                        self.parameters.lock().unwrap().gesture_active = false;
                        result?;
                    }
                }
                if !hover.present {
                    *entry = None;
                }
            }
        }
        Ok(self.take_edits())
    }
    /// Deliver the maintained MUI surface capture, never fabricate a pointer
    /// position from the node's bounds. Retain press-time closures for the whole
    /// drag: their fractional accumulators must survive reactive snapshots.
    pub(crate) fn pointer_gestures(&mut self, ui: &moose::mui::mui::Ui, prefix: &str) -> Result<Vec<Edit>> {
        self.instructions.store(0, Ordering::Relaxed);
        self.parameters.lock().unwrap().edits.clear();
        let pointer = ui.pointer();
        // Named paint/layout descendants are real MUI capture surfaces. Bubble
        // that one response to the nearest enabled native gesture owner, never
        // synthesize another hit or notify every ancestor independently.
        let mut responses = BTreeMap::new();
        if let Some(scene) = ui.scene() {
            for surface in scene.surfaces() {
                let response = ui.get(surface.key.as_str());
                if !response.pressed && !response.held && !response.released { continue; }
                let mut current = Some(surface);
                for _ in 0..=64 {
                    let Some(surface) = current else { break; };
                    if let Some(id) = surface.key.as_str().strip_prefix(prefix)
                        .and_then(|key|key.strip_prefix('-')).and_then(|key|key.parse::<usize>().ok()) {
                        if self.gestures.get(id).and_then(Option::as_ref)
                            .is_some_and(|gesture|gesture.present && gesture.enabled) {
                            responses.insert(id,response);
                            break;
                        }
                    } else { break; }
                    current = surface.parent.as_ref().and_then(|parent|scene.surface(parent.as_str()));
                }
            }
        }
        for retiring in [true,false] {
            for (id, entry) in self.gestures.iter_mut().enumerate() {
                let Some(gesture) = entry else { continue; };
                let retired = !gesture.present || !gesture.enabled;
                if retired != retiring { continue; }
                let key = format!("{prefix}-{id}");
                let response = responses.get(&id).copied().unwrap_or_default();
                let surface = ui.scene().and_then(|scene|scene.surface(&key));
                let current = pointer.pos.zip(surface).map(|(position,surface)|HoverInput { position, frame:surface.frame, pose:surface.local_pose(), mods:pointer.mods });
                let mut calls = Vec::new();
                if !retired && response.pressed && gesture.capture.is_none() {
                    if let Some(input) = current {
                        let callbacks = gesture.callbacks.clone();
                        if let Some(callback) = &callbacks.tap[0] { calls.push((callback.clone(),input,"start",[0.,0.],input.position)); }
                        if response.double_clicked {
                            if let Some(callback) = &callbacks.tap[1] { calls.push((callback.clone(),input,"double",[0.,0.],input.position)); }
                        }
                        gesture.capture = Some(PointerCapture { callbacks, origin:input.position, last:input, delivered:input.position, dragging:false });
                    }
                }
                if let Some(capture) = &mut gesture.capture {
                    let input = HoverInput {
                        position:current.map_or(capture.last.position,|input|input.position),
                        frame:capture.last.frame,
                        pose:capture.last.pose,
                        mods:pointer.mods,
                    };
                    let finish = retired || response.released || (!response.held && !response.pressed);
                    if !retired && (response.held || response.released) {
                        let travel = input.position-capture.origin;
                        if !capture.dragging && travel.hypot() >= capture.callbacks.threshold && travel.hypot()>0. {
                            capture.dragging = true;
                            if let Some(callback) = &capture.callbacks.drag[0] { calls.push((callback.clone(),input,"start",[travel.x,travel.y],capture.origin)); }
                        }
                        if capture.dragging && input.position != capture.delivered {
                            let delta = input.position-capture.delivered;
                            if let Some(callback) = &capture.callbacks.drag[1] { calls.push((callback.clone(),input,"update",[delta.x,delta.y],capture.origin)); }
                            capture.delivered = input.position;
                        }
                    }
                    capture.last = input;
                    if finish {
                        if capture.dragging {
                            if let Some(callback) = &capture.callbacks.drag[2] { calls.push((callback.clone(),input,"complete",[0.,0.],capture.origin)); }
                        }
                        if let Some(callback) = &capture.callbacks.tap[2] { calls.push((callback.clone(),input,"complete",[0.,0.],capture.origin)); }
                        gesture.capture = None;
                    }
                }
                for (callback,input,phase,delta,origin) in calls {
                    self.parameters.lock().unwrap().gesture_active = true;
                    let result = pointer_event(&self.lua,input,phase,delta,origin).and_then(|event|callback.call::<()>(event));
                    self.parameters.lock().unwrap().gesture_active = false;
                    result?;
                }
                if !gesture.present && gesture.capture.is_none() { *entry = None; }
            }
        }
        Ok(self.take_edits())
    }
    pub(crate) fn tap(&mut self, id: usize) -> Result<Vec<Edit>> {
        self.instructions.store(0, Ordering::Relaxed);
        self.parameters.lock().unwrap().edits.clear();
        let callback = self
            .gestures
            .get(id)
            .and_then(Option::as_ref)
            .filter(|gesture|gesture.present && gesture.enabled)
            .and_then(|gesture|gesture.callbacks.tap[0].as_ref())
            .context("Native UI gesture identity missing")?;
        let event = self.lua.create_table()?;
        event.set("kind", "tap")?;
        self.parameters.lock().unwrap().gesture_active = true;
        let result = callback.call::<()>(event);
        self.parameters.lock().unwrap().gesture_active = false;
        result?;
        Ok(self.take_edits())
    }
}

fn pointer_event(lua: &Lua, input: HoverInput, phase: &str, delta: [f64;2], origin: moose::mui::mui::prelude::Point) -> mlua::Result<Table> {
    let event = hover_event(lua,input,phase)?;
    let start = lua.create_table()?;
    let origin = input.pose.local(origin);
    start.set("x",origin.x)?; start.set("y",origin.y)?;
    event.set("start_position",start)?;
    let movement = lua.create_table()?;
    let delta = input.pose.delta(moose::mui::mui::prelude::Vec2::new(delta[0],delta[1]));
    movement.set("x",delta.x)?; movement.set("y",delta.y)?;
    event.set("delta",movement)?;
    Ok(event)
}
fn hover_event(lua: &Lua, input: HoverInput, phase: &str) -> mlua::Result<Table> {
    let event = lua.create_table()?;
    event.set("kind", phase)?;
    let position = lua.create_table()?;
    let local = input.pose.local(input.position);
    position.set("x", local.x)?;
    position.set("y", local.y)?;
    event.set("position", position)?;
    let frame = lua.create_table()?;
    frame.set("x", input.frame.x)?;
    frame.set("y", input.frame.y)?;
    frame.set("width", input.frame.size.width)?;
    frame.set("height", input.frame.size.height)?;
    event.set("frame", frame)?;
    let modifiers = lua.create_table()?;
    modifiers.set("alt", input.mods.alt)?;
    modifiers.set("shift", input.mods.shift)?;
    modifiers.set(
        "control",
        if cfg!(target_os = "macos") {
            input.mods.cmd
        } else {
            input.mods.ctrl
        },
    )?;
    event.set("modifiers", modifiers)?;
    Ok(event)
}

fn parse_alignment(name: &str) -> Result<((Align, Align), bool, bool)> {
    let (mut x, mut y) = (None, None);
    ensure!(name.len() <= 32 && (1..=2).contains(&name.split_whitespace().count()),
        "Invalid native UI alignment: {name}");
    for token in name.split_whitespace() {
        match token {
            "left" | "right" => {
                ensure!(x.is_none(), "Conflicting native UI alignment: {name}");
                x = Some(if token == "left" { Align::Start } else { Align::End });
            }
            "top" | "bottom" => {
                ensure!(y.is_none(), "Conflicting native UI alignment: {name}");
                y = Some(if token == "top" { Align::Start } else { Align::End });
            }
            "center" => (),
            _ => bail!("Unsupported native UI alignment: {name}"),
        }
    }
    Ok(((x.unwrap_or(Align::Center), y.unwrap_or(Align::Center)), x.is_some(), y.is_some()))
}

#[cfg(test)]
pub(crate) fn frontend_test_invalid_sources() -> bool {
    [
        "@",
        "@ui.",
        "@ui.Text{text='unterminated}",
        "@ui.Text{text='a',text='b'}",
        "@ui.Text{}.frame(width=)",
    ]
    .iter()
    .all(|s| frontend::lower(s).is_err())
}

#[cfg(test)]
mod library_probe {
    #[test]
    #[ignore = "requires an explicitly supplied owner's local native UI library"]
    fn native_ui_library_scene_resource_probe() {
        let path = std::path::PathBuf::from(
            std::env::var_os("KONTRA_NATIVE_UI_PROBE_INSTRUMENT")
                .expect("set KONTRA_NATIVE_UI_PROBE_INSTRUMENT"),
        );
        let instrument =
            if let Some(snapshot) = std::env::var_os("KONTRA_NATIVE_UI_PROBE_SNAPSHOT") {
                crate::import::read_snapshot(&path, std::path::Path::new(&snapshot))
            } else {
                crate::import::read(&path)
            }
            .expect("actual instrument/resource import");
        let (rt, errors) =
            crate::engine::load_scripts(&instrument, instrument.script_state.clone(), 48000.);
        assert!(
            errors.is_empty(),
            "actual KSP initialization errors: {errors:?}"
        );
        let rt = rt.expect("instrument has KSP scripts");
        eprintln!(
            "Native UI intent {:?}, successful performance pages {:?}",
            rt.native_ui_request(),
            rt.performance_slots().collect::<Vec<_>>()
        );
        let mut scene = super::Scene::prepare(&path, &rt)
            .unwrap_or_else(|error| panic!("actual native scene preparation: {error:#}"))
            .expect("native UI requested");
        let root = scene.snapshot().expect("actual native scene evaluation");
        eprintln!(
            "Native scene identity {:?}, modules {}, root {}x{}",
            scene.identity,
            scene.resources.len(),
            root.width.unwrap_or(0.),
            root.height.unwrap_or(0.)
        );
        fn count(node: &super::Node) -> usize {
            1 + node.children.iter().map(count).sum::<usize>()
        }
        eprintln!(
            "Native evaluated nodes {} (metadata only; no PCM/render parity claim)",
            count(&root)
        );
        if let Some(path)=std::env::var_os("KONTRA_NATIVE_UI_PROBE_SHOT") {
            crate::ui::audit::native_scene_shot(&mut scene,std::path::Path::new(&path),2.)
                .expect("genuine native scene CPU capture");
            eprintln!("Native scene capture uses actual evaluated Lua and local assets at 2x; no Kontakt parity claim");
        }
    }
}
