//! Bounded legacy .nui execution over the published UI IR. No filesystem Lua API.
use super::pictures::Source;
use mlua::{Function, Lua, Table, UserData, UserDataMethods, Value as LuaValue};
use moose::mui::mui::{prelude::Font, scene::Image};
use sampler_ui_ir as ir;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
mod syntax;

pub(super) struct Package {
    members: BTreeMap<String, Arc<[u8]>>,
    pub fonts: BTreeMap<String, Font>,
    pub images: Images,
}
struct ImageCache {
    loaded: HashMap<String, Option<Arc<Image>>>,
    pending: BTreeSet<String>,
    touch: HashMap<String, u64>,
    tick: u64,
    bytes: usize,
    #[cfg(feature = "shots")]
    scan: super::pictures::Scan,
}
pub(super) struct Images {
    cache: Arc<Mutex<ImageCache>>,
    request: std::sync::mpsc::SyncSender<String>,
}
impl Images {
    pub fn get(&self, name: &str) -> Option<Arc<Image>> {
        let name = name.replace('\\', "/").to_lowercase();
        if name.split('/').any(|s| s == ".." || s.is_empty())
            || name.starts_with('/')
            || name.contains(':')
        {
            return None;
        }
        let mut cache = self.cache.lock().ok()?;
        cache.tick = cache.tick.wrapping_add(1);
        let tick = cache.tick;
        cache.touch.insert(name.clone(), tick);
        if let Some(image) = cache.loaded.get(&name) {
            return image.clone();
        }
        if !cache.pending.contains(&name) && self.request.try_send(name.clone()).is_ok() {
            cache.pending.insert(name);
        }
        None
    }
    pub fn pending(&self) -> usize {
        self.cache.lock().map_or(0, |c| c.pending.len())
    }
    pub fn bytes(&self) -> usize {
        self.cache.lock().map_or(0, |c| c.bytes)
    }
    #[cfg(feature = "shots")]
    pub fn scan(&self) -> super::pictures::Scan {
        self.cache.lock().map_or(Default::default(), |c| c.scan)
    }
    #[cfg(feature = "shots")]
    pub fn failures(&self) -> Vec<String> {
        self.cache.lock().map_or(Vec::new(), |c| {
            c.loaded
                .iter()
                .filter(|(_, v)| v.is_none())
                .map(|(k, _)| blake3::hash(k.as_bytes()).to_hex().to_string())
                .collect()
        })
    }
}
impl Package {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let mut source = Source::of(path);
        let mut members = BTreeMap::new();
        let mut fonts = BTreeMap::new();
        let mut total = 0;
        let mut resource_paths = BTreeMap::new();
        for name in source.native_names() {
            let relative = name
                .strip_prefix("resources/native_ui/")
                .or_else(|| name.strip_prefix("native_ui/"))
                .unwrap_or(&name)
                .to_lowercase();
            resource_paths.insert(relative.clone(), name.clone());
            if !(relative.ends_with(".nui")
                || relative.ends_with(".ttf")
                || relative.ends_with(".otf"))
            {
                continue;
            }
            let bytes = source
                .read(&name)
                .ok_or_else(|| anyhow::anyhow!("NativeUI member unreadable"))?;
            total += bytes.len();
            anyhow::ensure!(
                total <= 16 << 20,
                "NativeUI sources and fonts exceed 16 MiB"
            );
            if relative.ends_with(".nui") {
                members.insert(relative, Arc::from(bytes));
            } else {
                fonts.insert(
                    relative,
                    Font::new(bytes).map_err(|_| anyhow::anyhow!("NativeUI font invalid"))?,
                );
            }
        }
        anyhow::ensure!(!members.is_empty(), "No readable legacy .nui resources");
        let (request, jobs) = std::sync::mpsc::sync_channel::<String>(64);
        let cache = Arc::new(Mutex::new(ImageCache {
            loaded: HashMap::new(),
            pending: BTreeSet::new(),
            touch: HashMap::new(),
            tick: 0,
            bytes: 0,
            #[cfg(feature = "shots")]
            scan: source.scan,
        }));
        let worker_cache = Arc::downgrade(&cache);
        std::thread::Builder::new()
            .name("native-art".into())
            .spawn(move || {
                while let Ok(name) = jobs.recv() {
                    let asset = ir::Asset {
                        path: resource_paths
                            .get(&name)
                            .cloned()
                            .unwrap_or_else(|| format!("Resources/native_ui/{name}")),
                        kind: ir::AssetKind::Image(ir::ImageMeta::default()),
                    };
                    let image = source
                        .load_frame(&asset, 0, [u32::MAX; 2], None, || {
                            worker_cache.strong_count() == 0
                        })
                        .and_then(|p| p.frames.first().cloned());
                    let Some(cache) = worker_cache.upgrade() else {
                        return;
                    };
                    let Ok(mut cache) = cache.lock() else { return };
                    let bytes = image.as_ref().map_or(0, |i| i.rgba.len());
                    while cache.bytes + bytes > 48 << 20 || cache.loaded.len() >= 4096 {
                        let Some(old) = cache
                            .loaded
                            .keys()
                            .min_by_key(|k| cache.touch.get(*k).copied().unwrap_or(0))
                            .cloned()
                        else {
                            break;
                        };
                        if let Some(Some(image)) = cache.loaded.remove(&old) {
                            cache.bytes -= image.rgba.len();
                        }
                        cache.touch.remove(&old);
                    }
                    #[cfg(feature = "shots")]
                    {
                        cache.scan = source.scan;
                    }
                    cache.bytes += bytes;
                    cache.pending.remove(&name);
                    cache.loaded.insert(name, image);
                    drop(cache);
                    super::picture_worker::completed();
                }
            })?;
        Ok(Self {
            members,
            fonts,
            images: Images { cache, request },
        })
    }
    fn source(&self, module: &str) -> anyhow::Result<&str> {
        let name = module
            .strip_suffix(".nui")
            .unwrap_or(module)
            .replace('.', "/")
            .to_lowercase();
        anyhow::ensure!(
            !name.starts_with('/')
                && !name.contains(':')
                && !name.split('/').any(|s| s == ".." || s.is_empty()),
            "Invalid NativeUI module path"
        );
        let bytes = self
            .members
            .get(&(name.clone() + ".nui"))
            .or_else(|| self.members.get(&(name + "/init.nui")))
            .ok_or_else(|| anyhow::anyhow!("NativeUI module absent"))?;
        Ok(std::str::from_utf8(bytes)?)
    }
}

#[derive(Clone, Debug)]
pub(super) struct Edit {
    pub source: ir::Source,
    pub widget: ir::WidgetRef,
    pub source_id: Option<i32>,
    pub id: Option<ir::ControlId>,
    pub index: Option<usize>,
    pub value: ir::Value,
}
#[derive(Default)]
struct Bridge {
    controls: Vec<ir::Widget>,
    locations: Vec<(ir::Source, usize)>,
    edits: Vec<Edit>,
    touches: BTreeSet<(usize, usize)>,
    unavailable: BTreeSet<String>,
    meters: HashMap<usize, f64>,
}
#[derive(Clone)]
struct Parameter {
    binding: usize,
    identifier: String,
    boolean: bool,
    bridge: Arc<Mutex<Bridge>>,
}
fn bounds(widget: &ir::Widget) -> ir::Range {
    match widget.kind {
        ir::Kind::Knob { range, .. }
        | ir::Kind::Slider { range, .. }
        | ir::Kind::ValueEdit { range, .. } => range,
        _ => ir::Range {
            min: 0.,
            max: 1.,
            ..Default::default()
        },
    }
}
fn value(lua: &Lua, value: &ir::Value, index: Option<usize>) -> mlua::Result<LuaValue> {
    Ok(match value {
        ir::Value::Integer(n) => LuaValue::Integer(*n as i64),
        ir::Value::Real(n) => LuaValue::Number(*n),
        ir::Value::Text(s) | ir::Value::DropPath {path:s,..} => LuaValue::String(lua.create_string(s)?),
        ir::Value::Integers(a) if index.is_some() => {
            LuaValue::Integer(a.get(index.unwrap()).copied().unwrap_or(0) as i64)
        }
        ir::Value::Reals(a) if index.is_some() => {
            LuaValue::Number(a.get(index.unwrap()).copied().unwrap_or(0.))
        }
        ir::Value::Integers(a) => LuaValue::Table(lua.create_sequence_from(a.iter().copied())?),
        ir::Value::Reals(a) => LuaValue::Table(lua.create_sequence_from(a.iter().copied())?),
    })
}
impl UserData for Parameter {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("value", |lua, this, index: Option<usize>| {
            let mut bridge = this.bridge.lock().unwrap();
            let Some(widget) = bridge.controls.get(this.binding) else {
                if bridge.unavailable.len() < 128 {
                    bridge.unavailable.insert(this.identifier.clone());
                }
                return Ok(if this.boolean {
                    LuaValue::Boolean(false)
                } else {
                    LuaValue::Number(0.)
                });
            };
            let v = widget
                .value
                .as_ref()
                .map_or(Ok(LuaValue::Number(widget.initial_value)), |v| {
                    value(lua, v, index)
                })?;
            let numeric = match v {
                LuaValue::Integer(n) => Some(n as f64),
                LuaValue::Number(n) => Some(n),
                _ => None,
            };
            if this.boolean {
                return Ok(LuaValue::Boolean(numeric.is_some_and(|n| n != 0.)));
            }
            if matches!(widget.kind, ir::Kind::Knob { .. } | ir::Kind::Slider { .. })
                && let Some(n) = numeric
            {
                let range = bounds(widget);
                return Ok(LuaValue::Number(if range.max == range.min {
                    0.
                } else {
                    ((n - range.min) / (range.max - range.min)).clamp(0., 1.)
                }));
            }
            Ok(v)
        });
        methods.add_method(
            "set_value",
            |_, this, (v, index): (LuaValue, Option<usize>)| {
                let mut bridge = this.bridge.lock().unwrap();
                let widget = bridge
                    .controls
                    .get(this.binding)
                    .ok_or_else(|| mlua::Error::external("NativeUI control unavailable"))?;
                let value = match v {
                    LuaValue::String(s) => {
                        let s = s.to_str()?.to_owned();
                        if s.len() > 1024 {
                            return Err(mlua::Error::external("NativeUI text exceeds 1024 bytes"));
                        }
                        ir::Value::Text(s)
                    }
                    v => {
                        let mut n = match v {
                            LuaValue::Integer(n) => n as f64,
                            LuaValue::Number(n) => n,
                            LuaValue::Boolean(n) => {
                                if n {
                                    1.
                                } else {
                                    0.
                                }
                            }
                            _ => {
                                return Err(mlua::Error::external(
                                    "NativeUI value type unsupported",
                                ));
                            }
                        };
                        if !n.is_finite() {
                            return Err(mlua::Error::external("Nonfinite NativeUI value"));
                        }
                        if matches!(widget.kind, ir::Kind::Knob { .. } | ir::Kind::Slider { .. }) {
                            let r = bounds(widget);
                            n = r.min + n.clamp(0., 1.) * (r.max - r.min);
                        }
                        if matches!(widget.value, Some(ir::Value::Real(_) | ir::Value::Reals(_))) {
                            ir::Value::Real(n)
                        } else {
                            ir::Value::Integer(n.round() as i32)
                        }
                    }
                };
                let id = match widget.binding {
                    ir::Binding::Control(c) => Some(c),
                    _ => None,
                };
                if bridge.edits.len() >= 256 {
                    return Err(mlua::Error::external("NativeUI edit queue full"));
                }
                let (source, widget) = bridge.locations[this.binding];
                let source_id = bridge.controls[this.binding].source_id;
                bridge.edits.push(Edit {
                    source,
                    widget: ir::WidgetRef(widget),
                    source_id,
                    id,
                    index,
                    value,
                });
                Ok(())
            },
        );
        methods.add_method(
            "ksp_control_property",
            |lua, this, (property, index): (i32, Option<usize>)| {
                let bridge = this.bridge.lock().unwrap();
                let Some(w) = bridge.controls.get(this.binding) else {
                    return Ok(LuaValue::Nil);
                };
                let range = bounds(w);
                Ok(match property {
                    0 => LuaValue::String(lua.create_string(&w.text)?),
                    1 => LuaValue::String(lua.create_string(&w.tooltip)?),
                    14 => {
                        LuaValue::String(lua.create_string(w.value_text.as_deref().unwrap_or(""))?)
                    }
                    16 => LuaValue::Number(range.default),
                    21 => LuaValue::Number(range.min),
                    22 => LuaValue::Number(range.max),
                    17 => LuaValue::Integer(match &w.kind {
                        ir::Kind::Menu { items } => items.len() as i64,
                        _ => 0,
                    }),
                    18 | 19 | 20 => {
                        if let ir::Kind::Menu { items } = &w.kind
                            && let Some(item) = items.get(index.unwrap_or(0))
                        {
                            match property {
                                18 => LuaValue::String(lua.create_string(&item.text)?),
                                19 => LuaValue::Integer(item.value as i64),
                                _ => LuaValue::Boolean(item.visible),
                            }
                        } else {
                            LuaValue::Nil
                        }
                    }
                    _ => LuaValue::Nil,
                })
            },
        );
        methods.add_method("update_touch", |_, this, index: Option<usize>| {
            this.bridge
                .lock()
                .unwrap()
                .touches
                .insert((this.binding, index.unwrap_or(0)));
            Ok(())
        });
        methods.add_method("end_touch", |_, this, index: Option<usize>| {
            this.bridge
                .lock()
                .unwrap()
                .touches
                .remove(&(this.binding, index.unwrap_or(0)));
            Ok(())
        });
        methods.add_method("is_touch_active", |_, this, index: Option<usize>| {
            Ok(this
                .bridge
                .lock()
                .unwrap()
                .touches
                .contains(&(this.binding, index.unwrap_or(0))))
        });
        methods.add_method("is_midi_learn_active", |_, _, _: Option<usize>| Ok(false));
        for name in ["begin_midi_learn", "end_midi_learn"] {
            methods.add_method(name, |_, _, _: Option<usize>| {
                Err::<(), _>(mlua::Error::external("NativeUI MIDI learn is unavailable"))
            });
        }
    }
}

/// One editor-owned Lua VM. Audio receives only the bounded numeric edits.
pub struct Session {
    lua: Lua,
    root: Function,
    bridge: Arc<Mutex<Bridge>>,
    fuel: Arc<AtomicUsize>,
    deadline: Arc<Mutex<std::time::Instant>>,
}
impl Session {
    pub fn new(
        package: Arc<Package>,
        entry: &str,
        controls: Vec<(ir::Source, usize, ir::Widget)>,
    ) -> anyhow::Result<Self> {
        let lua = Lua::new_with(
            mlua::StdLib::TABLE | mlua::StdLib::STRING | mlua::StdLib::MATH | mlua::StdLib::UTF8,
            mlua::LuaOptions::default(),
        )?;
        lua.set_memory_limit(128 * 1024 * 1024)?;
        lua.globals().set(
            "print",
            lua.create_function(|_, _: mlua::MultiValue| Ok(()))?,
        )?;
        for name in ["dofile", "loadfile"] {
            lua.globals().set(name, LuaValue::Nil)?;
        }
        let package_api = lua.create_table()?;
        package_api.set("loaded", lua.create_table()?)?;
        lua.globals().set("package", package_api)?;
        for name in ["loadstring", "collectgarbage", "getfenv", "setfenv"] {
            lua.globals().set(name, LuaValue::Nil)?;
        }
        // Luau interrupts fire at calls/backedges, unlike Lua instruction hooks.
        let fuel = Arc::new(AtomicUsize::new(500_000));
        let deadline = Arc::new(Mutex::new(
            std::time::Instant::now() + std::time::Duration::from_secs(2),
        ));
        let hook_deadline = deadline.clone();
        let hook_fuel = fuel.clone();
        lua.set_interrupt(move |_| {
            if hook_fuel
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
                .is_err()
            {
                return Err(mlua::Error::external("NativeUI interrupt budget exceeded"));
            }
            if hook_fuel.load(Ordering::Relaxed) % 256 == 0
                && std::time::Instant::now() > *hook_deadline.lock().unwrap()
            {
                return Err(mlua::Error::external("NativeUI time budget exceeded"));
            }
            Ok(mlua::VmState::Continue)
        });
        lua.load(include_str!("native_runtime/runtime.lua"))
            .set_name("NativeUI host")
            .exec()?;
        let package_table: Table = lua.globals().get("package")?;
        for key in ["loadlib", "searchers", "path", "cpath"] {
            package_table.set(key, LuaValue::Nil)?;
        }
        let locations = controls.iter().map(|(s, n, _)| (*s, *n)).collect();
        let bridge = Arc::new(Mutex::new(Bridge {
            controls: controls.into_iter().map(|(_, _, w)| w).collect(),
            locations,
            edits: Vec::with_capacity(256),
            ..Default::default()
        }));
        let names = bridge
            .lock()
            .unwrap()
            .controls
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    c.name
                        .trim_start_matches(['$', '%', '@', '~', '?', '!'])
                        .to_owned(),
                    i,
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        let parameters = bridge.clone();
        let kontakt = lua.create_table()?;
        kontakt.set(
            "connect_parameter",
            lua.create_function(move |_, (identifier, kind): (String, Option<String>)| {
                // Connection handles are lazy: shipped libraries may retain unused bindings.
                // Reading or writing an unavailable control still reports its name.
                let binding = names.get(&identifier).copied().unwrap_or(usize::MAX);
                Ok(Parameter {
                    binding,
                    identifier,
                    boolean: kind.as_deref() == Some("bool"),
                    bridge: parameters.clone(),
                })
            })?,
        )?;
        let meter_bridge = bridge.clone();
        kontakt.set(
            "connect_level_meter",
            lua.create_function(move |lua, identifier: String| {
                let binding = meter_bridge
                    .lock()
                    .unwrap()
                    .controls
                    .iter()
                    .position(|w| {
                        w.name.trim_start_matches(['$', '%', '@', '~', '?', '!']) == identifier
                    })
                    .ok_or_else(|| mlua::Error::external("NativeUI meter unavailable"))?;
                let bridge = meter_bridge.clone();
                let meter = lua.create_table()?;
                meter.set(
                    "level_value",
                    lua.create_function(move |_, _: LuaValue| {
                        Ok(bridge
                            .lock()
                            .unwrap()
                            .meters
                            .get(&binding)
                            .copied()
                            .unwrap_or(0.))
                    })?,
                )?;
                Ok(meter)
            })?,
        )?;
        kontakt.set(
            "create_tmp_midi_file",
            lua.create_function(|_, _: LuaValue| {
                Err::<String, _>(mlua::Error::external(
                    "NativeUI MIDI drag export is unavailable",
                ))
            })?,
        )?;
        let loaded: Table = lua.globals().get::<Table>("package")?.get("loaded")?;
        loaded.set("kontakt", kontakt)?;
        let require = lua.create_function(move |lua, name: String| {
            let loaded: Table = lua.globals().get::<Table>("package")?.get("loaded")?;
            let existing: LuaValue = loaded.get(name.as_str())?;
            if !existing.is_nil() {
                return Ok(existing);
            }
            let source = package.source(&name).map_err(mlua::Error::external)?;
            let source = syntax::translate(source).map_err(|error| {
                mlua::Error::external(format!("NativeUI syntax translation: {error}"))
            })?;
            let value: LuaValue = lua.load(&source).set_name(format!("{name}.nui")).eval()?;
            loaded.set(name, value.clone())?;
            Ok(value)
        })?;
        lua.globals().set("require", require.clone())?;
        let root: Function = require.call(entry)?;
        Ok(Self {
            lua,
            root,
            bridge,
            fuel,
            deadline,
        })
    }
    pub fn update_view(
        &self,
        face: &ir::Interface,
        values: &super::ir_view::Values,
        typed: &HashMap<ir::WidgetRef, ir::Value>,
        meters: &HashMap<ir::WidgetRef, f64>,
    ) {
        let mut bridge = self.bridge.lock().unwrap();
        for at in 0..bridge.controls.len() {
            let (source, index) = bridge.locations[at];
            if source == face.source
                && let Some(w) = face.widgets.get(index)
                && &bridge.controls[at] != w
            {
                bridge.controls[at].clone_from(w);
            }
            if source == face.source {
                if let Some(value) = typed.get(&ir::WidgetRef(index)) {
                    bridge.controls[at].value = Some(value.clone());
                }
                if let Some(level) = meters.get(&ir::WidgetRef(index)) {
                    bridge.meters.insert(at, *level);
                }
            }
            let w = &mut bridge.controls[at];
            if let ir::Binding::Control(c) = w.binding
                && let Some(&n) = values.get(&c)
            {
                w.value = Some(if matches!(w.value, Some(ir::Value::Real(_))) {
                    ir::Value::Real(n)
                } else {
                    ir::Value::Integer(n.round() as i32)
                });
            }
        }
    }
    pub fn update_meters(&self, mut meter: impl FnMut(&ir::Widget) -> f64) {
        let mut bridge = self.bridge.lock().unwrap();
        let values = bridge
            .controls
            .iter()
            .enumerate()
            .filter(|(_, w)| w.meter.is_some() || matches!(w.kind, ir::Kind::LevelMeter { .. }))
            .map(|(n, w)| (n, meter(w)))
            .collect::<Vec<_>>();
        bridge.meters.extend(values);
    }
    pub fn render(&self) -> anyhow::Result<Table> {
        self.fuel.store(100_000, Ordering::Relaxed);
        *self.deadline.lock().unwrap() =
            std::time::Instant::now() + std::time::Duration::from_millis(250);
        if let Ok(error) = self.lua.globals().get::<String>("__canvas_error") {
            anyhow::bail!("NativeUI canvas: {error}");
        }
        Ok(self
            .lua
            .globals()
            .get::<Function>("__render")?
            .call(self.root.clone())?)
    }
    pub fn lua(&self) -> &Lua {
        &self.lua
    }
    pub fn call<A: mlua::IntoLuaMulti>(&self, function: Function, args: A) -> mlua::Result<()> {
        self.fuel.store(100_000, Ordering::Relaxed);
        *self.deadline.lock().unwrap() =
            std::time::Instant::now() + std::time::Duration::from_millis(250);
        function.call(args)
    }
    pub fn event(
        &self,
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
        w: f64,
        h: f64,
        shift: bool,
        control: bool,
        alt: bool,
        command: bool,
    ) -> mlua::Result<Table> {
        let event = self.lua.create_table()?;
        for (name, a, b) in [("position", x, y), ("delta", dx, dy), ("frame", w, h)] {
            let t = self.lua.create_table()?;
            t.set(if name == "frame" { "width" } else { "x" }, a)?;
            t.set(if name == "frame" { "height" } else { "y" }, b)?;
            event.set(name, t)?;
        }
        let modifiers = self.lua.create_table()?;
        modifiers.set("shift", shift)?;
        modifiers.set("control", control)?;
        modifiers.set("alt", alt)?;
        modifiers.set("command", command)?;
        event.set("modifiers", modifiers)?;
        Ok(event)
    }
    pub fn take_edits(&self) -> Vec<Edit> {
        self.bridge.lock().unwrap().edits.drain(..).collect()
    }
    pub fn unavailable_controls(&self) -> Vec<String> {
        self.bridge
            .lock()
            .unwrap()
            .unavailable
            .iter()
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_component_reads_the_published_ir_and_produces_a_typed_edit() {
        let (request, _jobs) = std::sync::mpsc::sync_channel(1);
        let package=Arc::new(Package{members:BTreeMap::from([("main.nui".into(),Arc::from(br#"local ui=require("native_ui")
            local kontakt=require("kontakt")
            local p=kontakt.connect_parameter("gain")
            return function()
                p:set_value(0.75)
                return @ui.ZStack { @ui.Text {text="Authored",}, @ui.Rectangle {color=ui.Color(12,24,48)}.frame(width=10,height=20) }.frame(width=80,height=60)
            end"#.as_slice()))]),fonts:BTreeMap::new(),images:Images{request,cache:Arc::new(Mutex::new(ImageCache{loaded:HashMap::new(),pending:BTreeSet::new(),touch:HashMap::new(),tick:0,bytes:0,#[cfg(feature="shots")] scan:Default::default()}))}});
        let mut widget = ir::Widget::new(
            "$gain",
            ir::PageRef(0),
            Default::default(),
            ir::Kind::Knob {
                range: ir::Range {
                    min: 0.,
                    max: 100.,
                    default: 0.,
                    step: Some(1.),
                },
                display: Default::default(),
            },
        );
        widget.source_id = Some(7);
        widget.binding = ir::Binding::Control(ir::ControlId(42));
        widget.value = Some(ir::Value::Integer(20));
        let session = Session::new(
            package,
            "main",
            vec![(ir::Source::Ksp { slot: 2 }, 0, widget)],
        )
        .unwrap();
        let source = ir::Interface {
            source: ir::Source::Ksp { slot: 2 },
            widgets: vec![session.bridge.lock().unwrap().controls[0].clone()],
            ..Default::default()
        };
        session.update_view(
            &source,
            &Default::default(),
            &HashMap::from([(
                ir::WidgetRef(0),
                ir::Value::Text("callback readback".into()),
            )]),
            &Default::default(),
        );
        assert_eq!(
            session.bridge.lock().unwrap().controls[0].value,
            Some(ir::Value::Text("callback readback".into()))
        );
        session.update_view(
            &source,
            &Default::default(),
            &HashMap::from([(ir::WidgetRef(0), ir::Value::Integer(20))]),
            &Default::default(),
        );
        let graph = session.render().unwrap();
        assert_eq!(graph.get::<String>("kind").unwrap(), "ZStack");
        let edits = session.take_edits();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].value, ir::Value::Integer(75));
        assert_eq!(edits[0].source_id, Some(7));
        assert_eq!(edits[0].source, ir::Source::Ksp { slot: 2 });
        assert!(session.unavailable_controls().is_empty());
        assert!(
            session
                .lua()
                .globals()
                .get::<LuaValue>("dofile")
                .unwrap()
                .is_nil()
        );
    }
}
