//! Legacy Kontakt NativeUI packages. All resource IO and Lua execution stay off audio.
use std::path::Path;

pub fn module_source(instrument: &Path, module: &str) -> anyhow::Result<String> {
    let name = module.trim_end_matches(".nui").replace('.', "/");
    let mut resources = crate::resources::Resources::of(instrument, "native_ui");
    let bytes = match resources
        .read(&(name.clone() + ".nui"))
        .map_err(anyhow::Error::msg)?
    {
        Some(bytes) => bytes,
        None => resources
            .read(&(name + "/init.nui"))
            .map_err(anyhow::Error::msg)?
            .ok_or_else(|| anyhow::anyhow!("NativeUI module {module:?} missing"))?,
    };
    Ok(String::from_utf8(bytes)?)
}

pub fn package_members(instrument: &Path) -> Vec<String> {
    crate::resources::Resources::of(instrument, "native_ui").names()
}

#[derive(Clone, Debug, Default)]
pub struct Package {
    pub members: std::collections::BTreeMap<String, std::sync::Arc<[u8]>>,
    #[cfg(feature = "plugin")]
    pub images: std::collections::BTreeMap<String, Arc<moose::mui::mui::scene::Image>>,
    #[cfg(feature = "plugin")]
    pub fonts: std::collections::BTreeMap<String, mui_text::Font>,
}

impl Package {
    pub fn load(instrument: &Path) -> anyhow::Result<Self> {
        let mut resources = crate::resources::Resources::of(instrument, "native_ui");
        let mut members = std::collections::BTreeMap::<String, Arc<[u8]>>::new();
        let mut total = 0usize;
        for name in resources.names() {
            let bytes = resources
                .read(&name)
                .map_err(anyhow::Error::msg)?
                .ok_or_else(|| anyhow::anyhow!("NativeUI resource {name} missing"))?;
            total = total
                .checked_add(bytes.len())
                .ok_or_else(|| anyhow::anyhow!("NativeUI package size overflow"))?;
            anyhow::ensure!(
                total <= 512 * 1024 * 1024,
                "NativeUI package exceeds 512 MiB"
            );
            members.insert(name.to_lowercase(), bytes.into());
        }
        #[cfg(feature = "plugin")]
        {
            let mut images = std::collections::BTreeMap::new();
            let mut fonts = std::collections::BTreeMap::new();
            let mut decoded = 0usize;
            for (name, bytes) in &members {
                if [".png", ".jpg", ".jpeg", ".webp", ".svg"]
                    .iter()
                    .any(|ext| name.ends_with(ext))
                {
                    let image = crate::artwork::decode_native(bytes)
                        .map_err(|e| anyhow::anyhow!("NativeUI image {name}: {e}"))?;
                    decoded = decoded
                        .checked_add(image.rgba.len())
                        .ok_or_else(|| anyhow::anyhow!("NativeUI artwork size overflow"))?;
                    anyhow::ensure!(
                        decoded <= 512 * 1024 * 1024,
                        "NativeUI artwork exceeds 512 MiB"
                    );
                    images.insert(name.clone(), Arc::new(image));
                } else if name.ends_with(".ttf") || name.ends_with(".otf") {
                    let font = mui_text::Font::new(bytes.clone())
                        .map_err(|e| anyhow::anyhow!("NativeUI font {name}: {e}"))?;
                    fonts.insert(name.clone(), font);
                }
            }
            return Ok(Self {
                members,
                images,
                fonts,
            });
        }
        #[cfg(not(feature = "plugin"))]
        Ok(Self { members })
    }

    fn source(&self, module: &str) -> anyhow::Result<&str> {
        let name = module
            .trim_end_matches(".nui")
            .replace('.', "/")
            .to_lowercase();
        let bytes = self
            .members
            .get(&(name.clone() + ".nui"))
            .or_else(|| self.members.get(&(name + "/init.nui")))
            .ok_or_else(|| anyhow::anyhow!("NativeUI module {module:?} missing"))?;
        Ok(std::str::from_utf8(bytes)?)
    }
}

mod syntax;
use crate::ksp::{ExposedControl, Value};
use mlua::{Function, Lua, Table, UserData, UserDataMethods, Value as LuaValue};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone, Debug)]
pub struct Edit {
    pub slot: usize,
    pub control: usize,
    pub index: usize,
    pub value: f64,
    pub text: Option<String>,
    pub midi_learn: Option<bool>,
}

#[derive(Default)]
struct Bridge {
    controls: Arc<[ExposedControl]>,
    edits: Vec<Edit>,
    touches: std::collections::BTreeSet<(usize, usize)>,
    unavailable: std::collections::BTreeSet<String>,
}
#[derive(Clone)]
struct Parameter {
    binding: usize,
    identifier: String,
    boolean: bool,
    bridge: Arc<Mutex<Bridge>>,
}
fn bounds(c: &crate::ksp::Control) -> (f64, f64) {
    let number = |name: &str, default| match c.properties.get(name) {
        Some(Value::Int(n)) => *n as f64,
        Some(Value::Real(n)) => *n,
        _ => default,
    };
    (
        number("$CONTROL_PAR_MIN_VALUE", 0.),
        number("$CONTROL_PAR_MAX_VALUE", 1_000_000.),
    )
}
fn normalized(c: &crate::ksp::Control) -> bool {
    matches!(c.kind.as_str(), "ui_knob" | "ui_slider")
        && matches!(
            c.properties.get("$CONTROL_PAR_VALUE"),
            Some(Value::Int(_) | Value::Real(_))
        )
}
fn lua_value(lua: &Lua, value: &Value, index: Option<usize>) -> mlua::Result<LuaValue> {
    Ok(match value {
        Value::Int(n) | Value::NativeInt { native_int: n } => LuaValue::Integer(*n as i64),
        Value::Real(n) => LuaValue::Number(*n),
        Value::Text(s) => LuaValue::String(lua.create_string(s)?),
        Value::IntArray(a) if index.is_some() => {
            LuaValue::Integer(a.get(index.unwrap()).copied().unwrap_or(0) as i64)
        }
        Value::RealArray(a) if index.is_some() => {
            LuaValue::Number(a.get(index.unwrap()).copied().unwrap_or(0.))
        }
        Value::Array(a) if index.is_some() => {
            return a
                .get(index.unwrap())
                .map_or(Ok(LuaValue::Nil), |v| lua_value(lua, v, None));
        }
        Value::IntArray(a) => LuaValue::Table(lua.create_sequence_from(a.iter().copied())?),
        Value::RealArray(a) => LuaValue::Table(lua.create_sequence_from(a.iter().copied())?),
        Value::Array(a) => {
            let table = lua.create_table()?;
            for (i, v) in a.iter().enumerate() {
                table.set(i + 1, lua_value(lua, v, None)?)?;
            }
            LuaValue::Table(table)
        }
    })
}
impl UserData for Parameter {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("value", |lua, this, index: Option<usize>| {
            let mut bridge = this.bridge.lock().unwrap();
            let Some(binding) = bridge.controls.get(this.binding) else {
                if !this.identifier.is_empty() && bridge.unavailable.len() < 128 {
                    bridge.unavailable.insert(this.identifier.clone());
                }
                return Ok(if this.boolean {
                    LuaValue::Boolean(false)
                } else {
                    LuaValue::Number(0.)
                });
            };
            let c = &binding.descriptor;
            let Some(v) = c.properties.get("$CONTROL_PAR_VALUE") else {
                return Ok(LuaValue::Nil);
            };
            let value = lua_value(lua, v, index)?;
            if this.boolean {
                return Ok(LuaValue::Boolean(match value {
                    LuaValue::Integer(n) => n != 0,
                    LuaValue::Number(n) => n != 0.,
                    LuaValue::Boolean(n) => n,
                    _ => false,
                }));
            }
            if normalized(c) {
                let (lo, hi) = bounds(c);
                let n = match value {
                    LuaValue::Integer(n) => n as f64,
                    LuaValue::Number(n) => n,
                    _ => 0.,
                };
                Ok(LuaValue::Number(if hi == lo {
                    0.
                } else {
                    (n - lo) / (hi - lo)
                }))
            } else {
                Ok(value)
            }
        });
        methods.add_method(
            "set_value",
            |_, this, (value, index): (LuaValue, Option<usize>)| {
                if this.identifier.is_empty() {
                    return Ok(());
                }
                let mut bridge = this.bridge.lock().unwrap();
                let binding = bridge.controls.get(this.binding).ok_or_else(|| {
                    mlua::Error::external(format!(
                        "KSP control {:?} is not exposed",
                        this.identifier
                    ))
                })?;
                if let LuaValue::String(text) = &value {
                    if !matches!(
                        binding.descriptor.properties.get("$CONTROL_PAR_VALUE"),
                        Some(Value::Text(_))
                    ) {
                        return Err(mlua::Error::external(
                            "NativeUI text requires a string control",
                        ));
                    }
                    let text = text.to_str()?.to_owned();
                    if text.len() > 1024 {
                        return Err(mlua::Error::external("NativeUI text exceeds 1024 bytes"));
                    }
                    let edit = Edit {
                        slot: binding.slot,
                        control: binding.control,
                        index: index.unwrap_or(0),
                        value: 0.,
                        text: Some(text.clone()),
                        midi_learn: None,
                    };
                    if bridge.edits.len() >= 256 {
                        return Err(mlua::Error::external("NativeUI edit queue full"));
                    }
                    bridge.edits.push(edit);
                    if let Some(Value::Text(target)) = Arc::make_mut(&mut bridge.controls)
                        [this.binding]
                        .descriptor
                        .properties
                        .get_mut("$CONTROL_PAR_VALUE")
                    {
                        *target = text;
                    }
                    return Ok(());
                }
                let mut value = match value {
                    LuaValue::Integer(n) => n as f64,
                    LuaValue::Number(n) => n,
                    LuaValue::Boolean(v) => f64::from(v),
                    _ => {
                        return Err(mlua::Error::external(
                            "NativeUI parameter requires a number or text",
                        ));
                    }
                };
                if !value.is_finite() {
                    return Err(mlua::Error::external(
                        "NativeUI parameter value must be finite",
                    ));
                }
                if normalized(&binding.descriptor) {
                    let (lo, hi) = bounds(&binding.descriptor);
                    value = lo + value.clamp(0., 1.) * (hi - lo);
                }
                let edit = Edit {
                    slot: binding.slot,
                    control: binding.control,
                    index: index.unwrap_or(0),
                    value,
                    text: None,
                    midi_learn: None,
                };
                if bridge.edits.len() >= 256 {
                    return Err(mlua::Error::external("NativeUI edit queue full"));
                }
                bridge.edits.push(edit);
                // Optimistic editor value; the next KSP publication remains authoritative.
                if let Some(c) = Arc::make_mut(&mut bridge.controls).get_mut(this.binding)
                    && let Some(v) = c.descriptor.properties.get_mut("$CONTROL_PAR_VALUE")
                {
                    match v {
                        Value::Int(n) => *n = value.round() as i32,
                        Value::Real(n) => *n = value,
                        Value::IntArray(a) => {
                            if let Some(n) = a.get_mut(index.unwrap_or(0)) {
                                *n = value.round() as i32
                            }
                        }
                        Value::RealArray(a) => {
                            if let Some(n) = a.get_mut(index.unwrap_or(0)) {
                                *n = value
                            }
                        }
                        _ => {}
                    }
                }
                Ok(())
            },
        );
        methods.add_method(
            "ksp_control_property",
            |lua, this, (property, index): (i32, Option<usize>)| {
                if this.identifier.is_empty() {
                    return Ok(if property == 17 {
                        LuaValue::Integer(0)
                    } else {
                        LuaValue::Nil
                    });
                }
                let mut bridge = this.bridge.lock().unwrap();
                let Some(binding) = bridge.controls.get(this.binding) else {
                    if bridge.unavailable.len() < 128 {
                        bridge.unavailable.insert(this.identifier.clone());
                    }
                    return Ok(if property == 17 {
                        LuaValue::Integer(0)
                    } else {
                        LuaValue::Nil
                    });
                };
                let c = &binding.descriptor;
                let name = match property {
                    0 => "$CONTROL_PAR_TEXT",
                    1 => "$CONTROL_PAR_HELP",
                    14 => "$CONTROL_PAR_LABEL",
                    16 => "$CONTROL_PAR_DEFAULT_VALUE",
                    21 => "$CONTROL_PAR_MIN_VALUE",
                    22 => "$CONTROL_PAR_MAX_VALUE",
                    _ => "",
                };
                if let Some(index) = index {
                    if let Some(v) = c.properties.get(&format!("{name}[{index}]")) {
                        return lua_value(lua, v, None);
                    }
                }
                if let Some(v) = c.properties.get(name) {
                    return lua_value(lua, v, index);
                }
                Ok(match property {
                    17 => LuaValue::Integer(c.menu.len() as i64),
                    18 => c
                        .menu
                        .get(index.unwrap_or(0))
                        .map_or(Ok::<LuaValue, mlua::Error>(LuaValue::Nil), |(s, _)| {
                            Ok(LuaValue::String(lua.create_string(s)?))
                        })?,
                    19 => c
                        .menu
                        .get(index.unwrap_or(0))
                        .map_or(LuaValue::Nil, |(_, n)| LuaValue::Integer(*n as i64)),
                    20 => LuaValue::Boolean(
                        bridge.controls[this.binding]
                            .menu_visible
                            .get(index.unwrap_or(0))
                            .copied()
                            .unwrap_or(false),
                    ),
                    0 | 14 => LuaValue::String(lua.create_string("")?),
                    21 => LuaValue::Number(bounds(c).0),
                    22 => LuaValue::Number(bounds(c).1),
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
        methods.add_method("is_midi_learn_active", |_, this, index: Option<usize>| {
            Ok(this
                .bridge
                .lock()
                .unwrap()
                .controls
                .get(this.binding)
                .is_some_and(|c| c.midi_learn == Some(index.unwrap_or(0))))
        });
        for (name, active) in [("begin_midi_learn", true), ("end_midi_learn", false)] {
            methods.add_method(name, move |_, this, index: Option<usize>| {
                let mut bridge = this.bridge.lock().unwrap();
                let binding = bridge.controls.get(this.binding).ok_or_else(|| {
                    mlua::Error::external("NativeUI MIDI learn requires an exposed control")
                })?;
                let index = index.unwrap_or(0);
                let edit = Edit {
                    slot: binding.slot,
                    control: binding.control,
                    index,
                    value: 0.,
                    text: None,
                    midi_learn: Some(active),
                };
                if bridge.edits.len() >= 256 {
                    return Err(mlua::Error::external("NativeUI edit queue full"));
                }
                bridge.edits.push(edit);
                if active {
                    for c in Arc::make_mut(&mut bridge.controls).iter_mut() {
                        c.midi_learn = None;
                    }
                }
                Arc::make_mut(&mut bridge.controls)[this.binding].midi_learn =
                    active.then_some(index);
                Ok(())
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
}
impl Session {
    pub fn new(
        package: Arc<Package>,
        entry: &str,
        controls: Arc<[ExposedControl]>,
    ) -> anyhow::Result<Self> {
        let lua = Lua::new_with(
            mlua::StdLib::TABLE
                | mlua::StdLib::STRING
                | mlua::StdLib::MATH
                | mlua::StdLib::UTF8
                | mlua::StdLib::PACKAGE,
            mlua::LuaOptions::default(),
        )?;
        lua.set_memory_limit(128 * 1024 * 1024)?;
        for name in ["dofile", "loadfile"] {
            lua.globals().set(name, LuaValue::Nil)?;
        }
        let package_api: Table = lua.globals().get("package")?;
        package_api.set("path", "")?;
        package_api.set("cpath", "")?;
        package_api.set("searchers", lua.create_table()?)?;
        package_api.set("loadlib", LuaValue::Nil)?;
        let fuel = Arc::new(AtomicUsize::new(5_000_000));
        let hook_fuel = fuel.clone();
        lua.set_hook(
            mlua::HookTriggers::new().every_nth_instruction(10_000),
            move |_, _| {
                if hook_fuel
                    .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                        n.checked_sub(10_000)
                    })
                    .is_err()
                {
                    return Err(mlua::Error::external(
                        "NativeUI instruction budget exceeded",
                    ));
                }
                Ok(mlua::VmState::Continue)
            },
        )?;
        lua.load(include_str!("native_ui/runtime.lua"))
            .set_name("NativeUI host")
            .exec()?;
        let package_table: Table = lua.globals().get("package")?;
        for key in ["loadlib", "searchers", "path", "cpath"] {
            package_table.set(key, LuaValue::Nil)?;
        }
        let bridge = Arc::new(Mutex::new(Bridge {
            controls,
            edits: Vec::with_capacity(256),
            ..Default::default()
        }));
        let names = bridge
            .lock()
            .unwrap()
            .controls
            .iter()
            .enumerate()
            .map(|(i, c)| (c.identifier.clone(), i))
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
        let meters = lua.create_function(move |lua, identifier: String| {
            let binding = meter_bridge
                .lock()
                .unwrap()
                .controls
                .iter()
                .position(|c| c.identifier == identifier)
                .ok_or_else(|| {
                    mlua::Error::external(format!("Unavailable NativeUI level meter: {identifier}"))
                })?;
            let bridge = meter_bridge.clone();
            let meter = lua.create_table()?;
            meter.set(
                "level_value",
                lua.create_function(move |_, _: LuaValue| {
                    let bridge = bridge.lock().unwrap();
                    Ok(
                        match bridge.controls[binding]
                            .descriptor
                            .properties
                            .get("$CONTROL_PAR_VALUE")
                        {
                            Some(Value::Int(n)) => f64::from(*n) / 1_000_000.,
                            _ => 0.,
                        },
                    )
                })?,
            )?;
            Ok(meter)
        })?;
        kontakt.set("connect_level_meter", meters)?;
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
            let source = syntax::translate(source).map_err(mlua::Error::external)?;
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
        })
    }
    pub fn update(&self, controls: Arc<[ExposedControl]>) {
        self.bridge.lock().unwrap().controls = controls;
    }
    pub fn render(&self) -> anyhow::Result<Table> {
        self.fuel.store(5_000_000, Ordering::Relaxed);
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
        self.fuel.store(5_000_000, Ordering::Relaxed);
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
