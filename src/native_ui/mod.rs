//! Embedded legacy native UI subset. Loading, Lua and scene evaluation are off audio.
//! Unsupported syntax/APIs/images fail explicitly; exported KSP views remain usable.
mod frontend;
use crate::ksp::{Live, Runtime};
use anyhow::{Context, Result, bail, ensure};
use mlua::{Function, HookTriggers, Lua, LuaOptions, StdLib, Table, Value, VmState};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Paint {
    Stack,
    Text(String),
    Rectangle([f32; 3]),
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Node {
    pub id: usize,
    pub paint: Paint,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub tap: bool,
    pub children: Vec<Node>,
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
}
#[derive(Default)]
struct Parameters {
    bindings: BTreeMap<String, Binding>,
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
    gestures: Vec<Option<Function>>,
    identities: BTreeMap<String, usize>,
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
            gestures: Vec::new(),
            identities: BTreeMap::new(),
            identity,
            target_version,
            resources,
        };
        let root = scene.snapshot().context("Native UI scene validation")?;
        ensure!(
            root.width.is_some_and(|v| v > 0.) && root.height.is_some_and(|v| v > 0.),
            "Native UI root requires an explicit positive frame"
        );
        Ok(Some(scene))
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
        self.revision.load(Ordering::Relaxed)
    }
    pub(crate) fn observe(&self, live: &Live) {
        let mut p = self.parameters.lock().unwrap();
        for c in &live.native_controls {
            if let Some(b) = p.bindings.get_mut(&c.name)
                && b.slot == c.slot
                && b.control == c.control
                && b.value != c.value
            {
                b.value = c.value;
                self.revision.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
    pub(crate) fn snapshot(&mut self) -> Result<Node> {
        self.instructions.store(0, Ordering::Relaxed);
        self.gestures.clear();
        let mut seen = BTreeSet::new();
        let root = self.root.clone();
        let mut retained = Vec::new();
        self.node(
            root,
            self.lua.create_table()?,
            "0".into(),
            0,
            &mut seen,
            &mut retained,
        )
    }
    fn node(
        &mut self,
        node: Table,
        context: Table,
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
        let paint = match node.get::<String>("kind")?.as_str() {
            "ZStack" => Paint::Stack,
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
                Paint::Rectangle([
                    color.get::<f32>("r")? / 255.,
                    color.get::<f32>("g")? / 255.,
                    color.get::<f32>("b")? / 255.,
                ])
            }
            kind => bail!("Unsupported native UI primitive: {kind}"),
        };
        let frame: Option<Table> = modifiers.get("frame")?;
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
        if let Paint::Rectangle(color) = &paint {
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
        let has_tap = gesture.is_some();
        if let Some(g) = gesture {
            self.gestures[id] = Some(g);
        }
        let children: Table = node.get("children")?;
        let mut nodes = Vec::new();
        for (index, child) in children.sequence_values::<Table>().enumerate() {
            nodes.push(self.node(
                child?,
                context.clone(),
                format!("{path}/{index}"),
                depth + 1,
                seen,
                retained,
            )?);
        }
        Ok(Node {
            id,
            paint,
            width,
            height,
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
