//! Embedded legacy native UI subset. Loading, Lua and scene evaluation are off audio.
//! Unsupported syntax/APIs/images fail explicitly; exported KSP views remain usable.
mod frontend;
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
    Text(String),
    Rectangle([f32; 4], f64),
    Asset(crate::native_assets::AssetRef),
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Node {
    pub id: usize,
    pub paint: Paint,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub text_size: f64,
    pub text_color: [f32; 4],
    pub alignment: (Align, Align),
    pub padding: Insets,
    pub hidden: bool,
    pub opacity: f32,
    pub background: bool,
    pub tap: bool,
    pub children: Vec<Node>,
}
impl Node {
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
struct Binding {
    slot: usize,
    control: usize,
    value: i32,
    meter_control: bool,
    meter: Option<crate::fx::MeterAttachment>,
}
struct MeterBinding { slot: usize, control: usize, reader: Arc<crate::fx::MeterReader> }
#[derive(Default)]
struct Parameters {
    bindings: BTreeMap<String, Binding>,
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
    gestures: Vec<Option<Function>>,
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
        let mut live = rt.live();
        rt.prepare_native_controls(&mut live);
        let mut p = Parameters::default();
        // NI expose_controls resolves identical identifiers to the first slot.
        for c in &live.native_controls {
            p.bindings.entry(c.name.clone()).or_insert(Binding {
                slot: c.slot,
                control: c.control,
                value: c.value,
                meter_control: c.kind.as_ref() == "ui_level_meter",
                meter: c.meter,
            });
        }
        ensure!(
            p.bindings.len() <= 4096,
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
                    .map(|b| b.value)
                    .ok_or_else(|| invalid("Native UI control identity changed"))
            })?,
        )?;
        let p = parameters.clone();
        let changed = revision.clone();
        lua.globals().set(
            "native_set",
            lua.create_function(move |_, (slot, control, value): (usize, usize, i32)| {
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
            "native_component_key",
            lua.create_function(|_, target: Function| {
                let info = target.info();
                if info.what != "Lua" {
                    return Err(invalid("Native component targets require Lua functions"));
                }
                Ok(format!(
                    "{}:{}",
                    info.source.unwrap_or_default(),
                    info.line_defined.unwrap_or(0)
                ))
            })?,
        )?;
        lua.load(include_str!("host.lua"))
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
        let resources = {
            let mut m = modules.lock().unwrap();
            m.sealed = true;
            std::mem::take(&mut m.used)
        };
        let mut scene = Self {
            lua,
            root,
            parameters,
            instructions,
            revision,
            output_meters: rt.output_meters.clone(),
            gestures: Vec::new(),
            identities: BTreeMap::new(),
            assets: Arc::default(),
            asset_pending: BTreeSet::new(),
            raster_waker: None,
            raster_pending: Arc::new(AtomicBool::new(false)),
            identity,
            target_version,
            resources,
        };
        let root = scene.snapshot_raw().context("Native UI scene validation")?;
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
    pub(crate) fn observe(&self, live: &Live) {
        let mut p = self.parameters.lock().unwrap();
        for c in &live.native_controls {
            if let Some(b) = p.bindings.get_mut(&c.name)
                && b.slot == c.slot
                && b.control == c.control
                && (b.value != c.value || b.meter != c.meter)
            {
                b.value = c.value;
                b.meter = c.meter;
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
        self.gestures.clear();
        let mut seen = BTreeSet::new();
        let root = self.root.clone();
        let mut retained = Vec::new();
        let node = self.node(
            root,
            self.lua.create_table()?,
            (14., [1.; 4], false, 1.),
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
        style: (f64, [f32; 4], bool, f32),
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
        ensure!(
            self.identities.len() < 4096 || self.identities.contains_key(&path),
            "Native UI identity budget exceeded"
        );
        let next = self.identities.len();
        let id = *self.identities.entry(path.clone()).or_insert(next);
        let expand: Function = self.lua.globals().get("native_expand")?;
        let (node, context): (Table, Table) = expand.call((node, path.clone(), context))?;
        let props: Table = node.get("props")?;
        let modifiers: Table = node.get("modifiers")?;
        let background = modifiers.get::<Option<Table>>("background")?.is_some();
        let (mut text_size, mut text_color, mut hidden, mut opacity) = style;
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
            let (mut x, mut y) = (None, None);
            ensure!(
                (1..=2).contains(&name.split_whitespace().count()),
                "Invalid native UI frame alignment: {name}"
            );
            for token in name.split_whitespace() {
                match token {
                    "left" | "right" => {
                        ensure!(
                            x.is_none(),
                            "Conflicting native UI frame alignment: {name}"
                        );
                        x = Some(if token == "left" { Align::Start } else { Align::End });
                    }
                    "top" | "bottom" => {
                        ensure!(
                            y.is_none(),
                            "Conflicting native UI frame alignment: {name}"
                        );
                        y = Some(if token == "top" { Align::Start } else { Align::End });
                    }
                    "center" => (),
                    _ => bail!("Unsupported native UI frame alignment: {name}"),
                }
            }
            ensure!(!matches!(paint, Paint::HStack(_)) || x.is_none(), "Native UI HStack requires vertical alignment");
            ensure!(!matches!(paint, Paint::VStack(_)) || y.is_none(), "Native UI VStack requires horizontal alignment");
            alignment = (x.unwrap_or(Align::Center), y.unwrap_or(Align::Center));
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
        if let Paint::Rectangle(color, _) = &paint {
            ensure!(
                color
                    .iter()
                    .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
                "Invalid native UI color"
            );
        }
        let tap: Option<Table> = modifiers.get("on_tap_gesture")?;
        let gesture = if let Some(t) = tap {
            Some(t.get::<Function>("start")?)
        } else {
            None
        };
        // Construction IDs remain stable across snapshots; gestures use a sparse table.
        if self.gestures.len() <= id {
            self.gestures.resize_with(id + 1, || None);
        }
        let has_tap = gesture.is_some() && !hidden;
        if let Some(g) = gesture.filter(|_| !hidden) {
            self.gestures[id] = Some(g);
        }
        let children: Table = node.get("children")?;
        let mut nodes = Vec::new();
        for (index, child) in children.sequence_values::<Table>().enumerate() {
            nodes.push(self.node(
                child?,
                context.clone(),
                (text_size, text_color, hidden, opacity),
                format!("{path}/{index}"),
                depth + 1,
                seen,
                retained,
            )?);
        }
        ensure!(!background || nodes.len()==2,"Invalid native UI background wrapper");
        Ok(Node {
            id,
            paint,
            width,
            height,
            text_size,
            text_color,
            alignment,
            padding,
            hidden,
            opacity,
            background,
            tap: has_tap,
            children: nodes,
        })
    }
    pub(crate) fn take_edits(&self) -> Vec<Edit> {
        std::mem::take(&mut self.parameters.lock().unwrap().edits)
    }
    pub(crate) fn tap(&mut self, id: usize) -> Result<Vec<Edit>> {
        self.instructions.store(0, Ordering::Relaxed);
        self.parameters.lock().unwrap().edits.clear();
        let callback = self
            .gestures
            .get(id)
            .and_then(Option::as_ref)
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
    }
}
