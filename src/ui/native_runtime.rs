//! Bounded legacy .nui execution over the published UI IR. No filesystem Lua API.
use super::pictures::Source;
use crate::support::MutexExt;
use crate::plugin::ui_activity::{Activity, Phase};
use mlua::{Function, Lua, Table, UserData, UserDataFields, UserDataMethods, Value as LuaValue};
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
    font_names: Vec<(String, u16, Font)>,
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
    #[cfg(test)]
    pub(super) fn presentation_fixture() -> Self {
        // No worker: tests publish completion at a deterministic frame edge.
        let (request, _jobs) = std::sync::mpsc::sync_channel(1);
        Self {
            request,
            cache: Arc::new(Mutex::new(ImageCache {
                loaded: HashMap::new(),
                pending: BTreeSet::new(),
                touch: HashMap::new(),
                tick: 0,
                bytes: 0,
                #[cfg(feature = "shots")]
                scan: Default::default(),
            })),
        }
    }
    #[cfg(test)]
    pub(super) fn presentation_complete(&self, name: &str, image: Arc<Image>) {
        let mut cache = self.cache.lock_unpoisoned();
        cache.bytes += image.rgba.len();
        if let Some(Some(old)) = cache.loaded.insert(name.into(), Some(image)) {
            cache.bytes -= old.rgba.len();
        }
        drop(cache);
        super::picture_worker::completed();
    }
    pub fn get(&self, name: &str) -> Option<Arc<Image>> {
        if name.len() > 4096 {
            return None;
        }
        let name = name.replace('\\', "/").to_lowercase();
        if name.split('/').any(|s| s == ".." || s.is_empty())
            || name.starts_with('/')
            || name.contains(':')
        {
            return None;
        }
        let mut cache = self.cache.lock_unpoisoned();
        cache.tick = cache.tick.wrapping_add(1);
        let tick = cache.tick;
        if let Some(image) = cache.loaded.get(&name) {
            let image = image.clone();
            cache.touch.insert(name, tick);
            return image;
        }
        if !cache.pending.contains(&name) && self.request.try_send(name.clone()).is_ok() {
            cache.pending.insert(name.clone());
            cache.touch.insert(name, tick);
        }
        None
    }
    pub fn pending(&self) -> usize {
        self.cache.lock_unpoisoned().pending.len()
    }
    pub fn bytes(&self) -> usize {
        self.cache.lock_unpoisoned().bytes
    }
    #[cfg(feature = "shots")]
    pub fn scan(&self) -> super::pictures::Scan {
        self.cache.lock_unpoisoned().scan
    }
    #[cfg(feature = "shots")]
    pub fn failures(&self) -> Vec<String> {
        self.cache
            .lock_unpoisoned()
            .loaded
            .iter()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| blake3::hash(k.as_bytes()).to_hex().to_string())
            .collect()
    }
}
impl Package {
    #[cfg(test)]
    pub(super) fn audit_bytes(&self) -> usize {
        self.members.values().map(|b| b.len()).sum::<usize>()
            + self.fonts.values().map(|f| f.as_ref().len()).sum::<usize>()
    }
    #[cfg(test)]
    pub(super) fn audit_token_count(&self, token: &str) -> usize {
        self.members
            .values()
            .filter_map(|bytes| std::str::from_utf8(bytes).ok())
            .map(|source| source.matches(token).count())
            .sum()
    }
    pub fn font(&self, name: &str, bold: bool) -> Option<Font> {
        let name = name.to_lowercase();
        self.fonts
            .iter()
            .find(|(path, _)| {
                *path == &name
                    || path.rsplit('/').next().is_some_and(|p| {
                        p == name
                            || p.strip_suffix(".ttf") == Some(name.as_str())
                            || p.strip_suffix(".otf") == Some(name.as_str())
                    })
            })
            .map(|(_, f)| f.clone())
            .or_else(|| {
                self.font_names
                    .iter()
                    .filter(|(family, _, _)| family.eq_ignore_ascii_case(&name))
                    .min_by_key(|(_, weight, _)| weight.abs_diff(if bold { 700 } else { 400 }))
                    .map(|(_, _, font)| font.clone())
            })
    }
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        Self::load_cancel(path, || false)
    }
    pub fn load_cancel(path: &Path, canceled: impl Fn() -> bool) -> anyhow::Result<Self> {
        anyhow::ensure!(!canceled(), "NativeUI preparation canceled");
        let mut source = Source::of(path);
        let mut members = BTreeMap::new();
        let mut fonts = BTreeMap::new();
        let mut font_names = Vec::new();
        let mut total = 0;
        let mut resource_paths = BTreeMap::new();
        for name in source.native_names() {
            anyhow::ensure!(!canceled(), "NativeUI preparation canceled");
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
                .read_result(&name)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "NativeUI resource {}",
                        super::pictures::resource_category(e)
                    )
                })?
                .ok_or_else(|| anyhow::anyhow!("NativeUI member unreadable"))?;
            anyhow::ensure!(!canceled(), "NativeUI preparation canceled");
            total += bytes.len();
            anyhow::ensure!(
                total <= 16 << 20,
                "NativeUI sources and fonts exceed 16 MiB"
            );
            if relative.ends_with(".nui") {
                members.insert(relative, Arc::from(bytes));
            } else {
                let font =
                    Font::new(bytes).map_err(|_| anyhow::anyhow!("NativeUI font invalid"))?;
                let (names, weight) = font_metadata(&font);
                total += names
                    .iter()
                    .map(|n| n.len() + std::mem::size_of::<(String, u16, Font)>())
                    .sum::<usize>();
                anyhow::ensure!(
                    total <= 16 << 20,
                    "NativeUI sources and fonts exceed 16 MiB"
                );
                font_names.extend(names.into_iter().map(|name| (name, weight, font.clone())));
                fonts.insert(relative, font);
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
                    let mut cache = cache.lock_unpoisoned();
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
            font_names,
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

// Read names only from the already validated, bounded supplied font. This does
// not discover system fonts or substitute a family absent from the package.
fn font_metadata(font: &Font) -> (Vec<String>, u16) {
    let bytes = font.as_ref();
    let u16_at = |at: usize| -> Option<u16> {
        Some(u16::from_be_bytes(
            bytes.get(at..at.checked_add(2)?)?.try_into().ok()?,
        ))
    };
    let u32_at = |at: usize| -> Option<usize> {
        Some(u32::from_be_bytes(bytes.get(at..at.checked_add(4)?)?.try_into().ok()?) as usize)
    };
    let read = || -> Option<(Vec<String>, u16)> {
        let base = if bytes.starts_with(b"ttcf") {
            u32_at(12 + font.index() as usize * 4)?
        } else {
            0
        };
        let table = |tag: &[u8]| -> Option<(usize, usize)> {
            for n in 0..usize::from(u16_at(base.checked_add(4)?)?) {
                let at = base.checked_add(12)?.checked_add(n.checked_mul(16)?)?;
                if bytes.get(at..at.checked_add(4)?)? == tag {
                    let (start, len) = (u32_at(at + 8)?, u32_at(at + 12)?);
                    bytes.get(start..start.checked_add(len)?)?;
                    return Some((start, len));
                }
            }
            None
        };
        let weight = table(b"OS/2")
            .and_then(|(at, len)| (len >= 6).then(|| u16_at(at + 4)).flatten())
            .unwrap_or(400);
        let (at, len) = table(b"name")?;
        if len < 6 {
            return None;
        }
        let count = usize::from(u16_at(at + 2)?);
        let strings = usize::from(u16_at(at + 4)?);
        if 6usize.checked_add(count.checked_mul(12)?)? > len {
            return None;
        }
        let mut names = Vec::new();
        for n in 0..count {
            let record = at + 6 + n * 12;
            if !matches!(u16_at(record + 6)?, 1 | 4 | 6 | 16) {
                continue;
            }
            let start = strings.checked_add(usize::from(u16_at(record + 10)?))?;
            let end = start.checked_add(usize::from(u16_at(record + 8)?))?;
            if end > len {
                return None;
            }
            let data = bytes.get(at + start..at + end)?;
            let name = match u16_at(record)? {
                0 | 3 if data.len() % 2 == 0 => String::from_utf16(
                    &data
                        .chunks_exact(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect::<Vec<_>>(),
                )
                .ok(),
                1 if data.is_ascii() => String::from_utf8(data.to_vec()).ok(),
                _ => None,
            };
            if let Some(name) = name.filter(|s| !s.is_empty() && s.len() <= 1024) {
                if !names.contains(&name) {
                    names.push(name);
                }
                // ponytail: 32 supplied aliases per face; no unbounded localized-name cache.
                if names.len() == 32 {
                    break;
                }
            }
        }
        names.sort();
        names.dedup();
        Some((names, weight))
    };
    read().unwrap_or_default()
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
    /// Bumped whenever a value Lua can read changes: the render cache key.
    rev: u64,
    /// Bumped when a meter level changes.
    meter_rev: u64,
    rendering: bool,
    /// Dependency ids (`binding * 4 + kind`) read since the Lua side last took
    /// them: value/properties 0, meter level 1, touch 2.
    reads: Vec<u32>,
    /// Dependency ids whose value moved since the last render.
    changed: BTreeSet<u32>,
}
const DEP_VALUE: u32 = 0;
const DEP_METER: u32 = 1;
const DEP_TOUCH: u32 = 2;
impl Bridge {
    fn read(&mut self, binding: usize, kind: u32) {
        if self.rendering && binding < self.controls.len() {
            let id = binding as u32 * 4 + kind;
            if self.reads.last() != Some(&id) {
                self.reads.push(id);
            }
        }
    }
    fn moved(&mut self, binding: usize, kind: u32) {
        if binding < self.controls.len() {
            self.changed.insert(binding as u32 * 4 + kind);
        }
    }
}
#[derive(Clone)]
struct Parameter {
    binding: usize,
    identifier: String,
    boolean: bool,
    bridge: Arc<Mutex<Bridge>>,
}
#[cfg(test)]
fn trace_parameter(lua: &Lua, parameter: &Parameter, access: &str) -> mlua::Result<()> {
    if let Ok(trace) = lua.globals().get::<Function>("__audit_parameter") {
        trace.call::<()>((
            parameter.identifier.clone(),
            parameter.binding as i64,
            lua.globals()
                .get::<String>("__audit_path")
                .unwrap_or_default(),
            access,
        ))?;
    }
    Ok(())
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
        ir::Value::Text(s) | ir::Value::DropPath { path: s, .. } => {
            LuaValue::String(lua.create_string(s)?)
        }
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
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("connected", |_, this| {
            Ok(this
                .bridge
                .lock_unpoisoned()
                .controls
                .get(this.binding)
                .is_some())
        });
    }
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("value", |lua, this, index: Option<usize>| {
            #[cfg(test)]
            trace_parameter(lua, this, "value")?;
            let mut bridge = this.bridge.lock_unpoisoned();
            bridge.read(this.binding, DEP_VALUE);
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
            |_lua, this, (v, index): (LuaValue, Option<usize>)| {
                #[cfg(test)]
                trace_parameter(_lua, this, "write")?;
                let mut bridge = this.bridge.lock_unpoisoned();
                let Some(widget) = bridge.controls.get(this.binding) else {
                    return Ok(());
                };
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
                #[cfg(test)]
                trace_parameter(lua, this, "property")?;
                let mut bridge = this.bridge.lock_unpoisoned();
                bridge.read(this.binding, DEP_VALUE);
                let Some(w) = bridge.controls.get(this.binding) else {
                    return Ok(match property {
                        0 | 1 | 14 | 18 => LuaValue::String(lua.create_string("")?),
                        17 => LuaValue::Integer(0),
                        20 => LuaValue::Boolean(false),
                        _ => LuaValue::Nil,
                    });
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
            let mut bridge = this.bridge.lock_unpoisoned();
            if bridge.controls.get(this.binding).is_some() {
                bridge.touches.insert((this.binding, index.unwrap_or(0)));
                bridge.rev += 1;
                bridge.moved(this.binding, DEP_TOUCH);
            }
            Ok(())
        });
        methods.add_method("end_touch", |_, this, index: Option<usize>| {
            let mut bridge = this.bridge.lock_unpoisoned();
            bridge.touches.remove(&(this.binding, index.unwrap_or(0)));
            bridge.rev += 1;
            bridge.moved(this.binding, DEP_TOUCH);
            Ok(())
        });
        methods.add_method("is_touch_active", |_, this, index: Option<usize>| {
            let mut bridge = this.bridge.lock_unpoisoned();
            bridge.read(this.binding, DEP_TOUCH);
            Ok(bridge.touches.contains(&(this.binding, index.unwrap_or(0))))
        });
        methods.add_method("is_midi_learn_active", |_, _, _: Option<usize>| Ok(false));
        for name in ["begin_midi_learn", "end_midi_learn"] {
            methods.add_method(name, |_, this, _: Option<usize>| {
                if this
                    .bridge
                    .lock_unpoisoned()
                    .controls
                    .get(this.binding)
                    .is_none()
                {
                    return Ok(());
                };
                Err::<(), _>(mlua::Error::external("NativeUI MIDI learn is unavailable"))
            });
        }
    }
}

pub(super) fn update_widget(
    widget: &mut ir::Widget,
    authored: Option<&ir::Widget>,
    typed: Option<&ir::Value>,
) -> bool {
    let mut changed = false;
    if let Some(authored) = authored
        && &*widget != authored
    {
        // A live readback alone must not recopy all authored metadata.
        // shortcut: divergent authored heap values still clone here; revisit if profiling warrants it.
        let previous = std::mem::replace(&mut widget.value, authored.value.clone());
        if &*widget != authored {
            widget.clone_from(authored);
        }
        if previous.is_some() && previous.as_ref() == typed {
            widget.value = previous;
        }
        changed = true;
    }
    if let Some(value) = typed
        && widget.value.as_ref() != Some(value)
    {
        widget.value = Some(value.clone());
        changed = true;
    }
    changed
}

/// One editor-owned Lua VM. Audio receives only the bounded numeric edits.
struct CachedGraph {
    graph: Table,
    rev: u64,
    meter_rev: u64,
    calls: u64,
    state_rev: u64,
    generation: u64,
    live: bool,
    texts: Vec<String>,
}
pub struct Session {
    lua: Lua,
    root: Function,
    activity: Option<Arc<Activity>>,
    bridge: Arc<Mutex<Bridge>>,
    fuel: Arc<AtomicUsize>,
    /// The last graph and what it was rendered from: while none of it moves
    /// the Lua tree is the same, so the 30 ms resolve is skipped.
    cached: std::cell::RefCell<Option<CachedGraph>>,
    /// Lua callbacks run (events, text edits); each may change script state.
    calls: std::cell::Cell<u64>,
    /// Counts fresh renders: a lowered tree is good while it is unchanged.
    generation: std::cell::Cell<u64>,
    /// The last graph holds a text field or an open popover: lowering them
    /// has side effects (editing, wheel capture), so it must run each frame.
    live: std::cell::Cell<bool>,
    /// Paths of the last graph's text fields: only a focused one edits, so
    /// only then must lowering run each frame.
    texts: std::cell::RefCell<Vec<String>>,
    /// The root table of the last fresh generation.
    last: std::cell::RefCell<Option<Table>>,
    #[cfg(test)]
    event_payloads: std::cell::Cell<usize>,
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
        // Bound Luau call/backedge checkpoints; scheduler time cannot invalidate UI work.
        let fuel = Arc::new(AtomicUsize::new(500_000));
        let hook_fuel = fuel.clone();
        lua.set_interrupt(move |lua| {
            // One thread owns the VM, so a plain load and store replace the
            // read-modify-write loop run at every call and loop back-edge.
            let left = hook_fuel.load(Ordering::Relaxed);
            if left > 0 {
                hook_fuel.store(left - 1, Ordering::Relaxed);
            } else {
                let site = lua
                    .inspect_stack(0, |debug| {
                        let source = debug.source();
                        let source = source.source.as_deref().unwrap_or("");
                        format!(
                            "{}:{}:{}:{}",
                            usize::from(source == "NativeUI host"),
                            debug.current_line().unwrap_or(0),
                            &blake3::hash(source.as_bytes()).to_hex()[..16],
                            lua.globals().get::<u32>("__native_nodes").unwrap_or(0)
                        )
                    })
                    .unwrap_or_default();
                return Err(mlua::Error::external(format!(
                    "NativeUI interrupt budget exceeded; site {site}"
                )));
            }
            Ok(mlua::VmState::Continue)
        });
        #[cfg(test)]
        let runtime = include_str!("native_runtime/runtime.lua")
            .replace("local props=element.properties()", "_G.__audit_path=path; local props=element.properties()")
            .replace("    reading=old_reading\n", "    reading=old_reading; _G.__audit_path=old_path\n");
        #[cfg(not(test))]
        let runtime = include_str!("native_runtime/runtime.lua");
        lua.load(runtime).set_name("NativeUI host").exec()?;
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
        // KSP expose_controls: duplicate identifiers use the first script slot,
        // regardless of the order in which source interfaces are published.
        let names = {
            let bridge = bridge.lock_unpoisoned();
            let mut names = HashMap::new();
            let priority = |index: usize| match bridge.locations[index].0 {
                ir::Source::Ksp { slot } => (slot, index),
                _ => (u8::MAX, index),
            };
            for (index, control) in bridge.controls.iter().enumerate() {
                names
                    .entry(
                        control
                            .name
                            .trim_start_matches(['$', '%', '@', '~', '?', '!'])
                            .to_owned(),
                    )
                    .and_modify(|previous| {
                        if priority(index) < priority(*previous) {
                            *previous = index
                        }
                    })
                    .or_insert(index);
            }
            names
        };
        let meter_names = names.clone();
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
                let binding = meter_names.get(&identifier).copied();
                let bridge = meter_bridge.clone();
                let meter = lua.create_table()?;
                meter.set("connected", binding.is_some())?;
                meter.set(
                    "level_value",
                    lua.create_function(move |_, _: LuaValue| {
                        Ok(binding
                            .and_then(|binding| {
                                let mut bridge = bridge.lock_unpoisoned();
                                bridge.read(binding, DEP_METER);
                                bridge.meters.get(&binding).copied()
                            })
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
        let reads = bridge.clone();
        lua.globals().set(
            "__take_reads",
            lua.create_function(move |lua, ()| {
                let mut bridge = reads.lock_unpoisoned();
                if bridge.reads.is_empty() {
                    return Ok(None);
                }
                let ids = lua.create_sequence_from(bridge.reads.iter().copied())?;
                bridge.reads.clear();
                Ok(Some(ids))
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
            activity: None,
            bridge,
            fuel,
            cached: Default::default(),
            calls: Default::default(),
            generation: Default::default(),
            live: Default::default(),
            texts: Default::default(),
            last: Default::default(),
            #[cfg(test)]
            event_payloads: std::cell::Cell::new(0),
        })
    }
    pub(super) fn set_activity(&mut self, activity: Option<Arc<Activity>>) {
        self.activity = activity;
    }
    pub fn update_view(
        &self,
        face: &ir::Interface,
        values: &super::ir_view::Values,
        typed: &HashMap<ir::WidgetRef, ir::Value>,
        meters: &HashMap<ir::WidgetRef, f64>,
    ) {
        let mut bridge = self.bridge.lock_unpoisoned();
        for at in 0..bridge.controls.len() {
            let (source, index) = bridge.locations[at];
            if source != face.source {
                continue;
            }
            let typed = typed.get(&ir::WidgetRef(index));
            let mut changed = update_widget(&mut bridge.controls[at], face.widgets.get(index), typed);
            if let Some(level) = meters.get(&ir::WidgetRef(index))
                && bridge.meters.insert(at, *level) != Some(*level)
            {
                bridge.meter_rev += 1;
                bridge.moved(at, DEP_METER);
            }
            let w = &mut bridge.controls[at];
            if typed.is_none()
                && matches!(
                    w.value,
                    None | Some(ir::Value::Integer(_) | ir::Value::Real(_))
                )
                && let ir::Binding::Control(c) = w.binding
                && let Some(&n) = values.get(&c)
            {
                let next = if matches!(w.value, Some(ir::Value::Real(_))) {
                    ir::Value::Real(n)
                } else {
                    ir::Value::Integer(n.round() as i32)
                };
                if w.value.as_ref() != Some(&next) {
                    w.value = Some(next);
                    changed = true;
                }
            }
            bridge.rev += u64::from(changed);
            if changed {
                bridge.moved(at, DEP_VALUE);
            }
        }
    }
    pub fn update_meters(&self, mut meter: impl FnMut(&ir::Widget) -> f64) {
        let mut bridge = self.bridge.lock_unpoisoned();
        let values = bridge
            .controls
            .iter()
            .enumerate()
            .filter(|(_, w)| w.meter.is_some() || matches!(w.kind, ir::Kind::LevelMeter { .. }))
            .map(|(n, w)| (n, meter(w)))
            .collect::<Vec<_>>();
        for (at, level) in values {
            if bridge.meters.insert(at, level) != Some(level) {
                bridge.meter_rev += 1;
                bridge.moved(at, DEP_METER);
            }
        }
    }
    /// Forget the cached graph: script state changed behind the host's back.
    #[cfg(test)]
    pub(super) fn invalidate(&self) {
        *self.cached.borrow_mut() = None;
        if let Ok(reset) = self.lua.globals().get::<Function>("__reset") {
            let _ = reset.call::<()>(());
        }
    }
    /// Render, ignoring the cache: for tests that change globals directly.
    #[cfg(test)]
    pub(super) fn fresh(&self) -> Table {
        self.fresh_result().unwrap()
    }
    #[cfg(test)]
    pub(super) fn fresh_result(&self) -> anyhow::Result<Table> {
        self.invalidate();
        self.render()
    }
    pub fn render(&self) -> anyhow::Result<Table> {
        let _script = self.activity.as_ref().map(|a| a.span(Phase::UiNativeScript));
        self.fuel.store(1_000_000, Ordering::Relaxed);
        if let Ok(error) = self.lua.globals().get::<String>("__canvas_error") {
            anyhow::bail!("NativeUI canvas: {error}");
        }
        let state_rev = self.lua.globals().get::<u64>("__rev").unwrap_or(0);
        let calls = self.calls.get();
        let (rev, meter_rev) = {
            let bridge = self.bridge.lock_unpoisoned();
            (bridge.rev, bridge.meter_rev)
        };
        if let Some(c) = &*self.cached.borrow()
            && c.rev == rev
            && c.calls == calls
            && c.state_rev == state_rev
            && c.meter_rev == meter_rev
        {
            self.generation.set(c.generation);
            self.live.set(c.live);
            *self.texts.borrow_mut() = c.texts.clone();
            return Ok(c.graph.clone());
        }
        let changed = {
            let mut bridge = self.bridge.lock_unpoisoned();
            bridge.rendering = true;
            bridge.reads.clear();
            let changed: Vec<u32> = std::mem::take(&mut bridge.changed).into_iter().collect();
            self.lua.create_sequence_from(changed)?
        };
        let graph: Table = self
            .lua
            .globals()
            .get::<Function>("__render")?
            .call((self.root.clone(), changed))
            .inspect_err(|_| {
                self.bridge.lock_unpoisoned().rendering = false;
                // Half a render leaves the memo inconsistent: start over.
                if let Ok(reset) = self.lua.globals().get::<Function>("__reset") {
                    let _ = reset.call::<()>(());
                }
            })?;
        let mut bridge = self.bridge.lock_unpoisoned();
        bridge.rendering = false;
        // The same root table means nothing below it changed: the lowered tree
        // and the event index stay good.
        if self.last.borrow().as_ref().is_none_or(|last| last.to_pointer() != graph.to_pointer()) {
            self.generation.set(self.generation.get() + 1);
            *self.last.borrow_mut() = Some(graph.clone());
        }
        let live = self.lua.globals().get::<bool>("__live").unwrap_or(true);
        self.live.set(live);
        let texts: Vec<String> = self
            .lua
            .globals()
            .get::<Table>("__texts")
            .map(|t| t.sequence_values::<String>().flatten().collect())
            .unwrap_or_default();
        *self.texts.borrow_mut() = texts.clone();
        let stable = self.lua.globals().get::<u64>("__rev").unwrap_or(0) == state_rev
            && bridge.rev == rev
            && self.calls.get() == calls;
        *self.cached.borrow_mut() = stable.then(|| CachedGraph {
            graph: graph.clone(),
            rev,
            meter_rev,
            calls,
            state_rev,
            generation: self.generation.get(),
            live,
            texts,
        });
        Ok(graph)
    }
    /// The key the last `render`'s lowering stays valid under, or `None` when
    /// lowering must run every frame (text fields and popups have side
    /// effects). Canvas callbacks read meters even when the graph did not, so
    /// the meter revision is part of it.
    /// Audit: the first difference between the memoized graph and one resolved
    /// from scratch, or `None` when they agree.
    #[cfg(any(test, feature = "shots"))]
    pub(super) fn verify(&self, graph: &Table) -> Option<String> {
        self.fuel.store(100_000_000, Ordering::Relaxed);
        self.lua
            .globals()
            .get::<Function>("__verify")
            .ok()?
            .call::<Option<String>>((self.root.clone(), graph.clone()))
            .unwrap_or_else(|error| Some(error.to_string()))
    }
    pub(super) fn generation(&self) -> u64 {
        self.generation.get()
    }
    pub(super) fn text_fields(&self) -> Vec<String> {
        self.texts.borrow().clone()
    }
    pub(super) fn lowering_key(&self) -> Option<(u64, u64, u64, u64)> {
        // A canvas paints from the live controls, which the graph need not
        // read: any change to them also ends the lowered tree.
        (!self.live.get()).then(|| {
            let bridge = self.bridge.lock_unpoisoned();
            let state_rev = self.lua.globals().get::<u64>("__rev").unwrap_or(0);
            (self.generation.get(), bridge.rev, bridge.meter_rev, state_rev)
        })
    }
    /// The handler-bearing nodes of the last graph in event order, or `None`
    /// when a popover below needs the full walk.
    pub(super) fn event_nodes(&self) -> Option<Vec<Table>> {
        match self.lua.globals().get::<LuaValue>("__targets") {
            Ok(LuaValue::Table(list)) => list.sequence_values::<Table>().collect::<Result<_, _>>().ok(),
            _ => None,
        }
    }
    #[cfg(test)]
    pub fn work_remaining(&self) -> usize {
        self.fuel.load(Ordering::Relaxed)
    }
    #[cfg(any(test, feature = "shots"))]
    pub fn graph_work(&self) -> (usize, usize) {
        (
            self.lua
                .globals()
                .get::<usize>("__native_nodes")
                .unwrap_or(0),
            1_000_000usize.saturating_sub(self.fuel.load(Ordering::Relaxed)),
        )
    }
    pub(super) fn activity(&self) -> Option<Arc<Activity>> {
        self.activity.clone()
    }
    pub fn lua(&self) -> &Lua {
        &self.lua
    }
    pub fn paint_callback(&self, paint: Function) -> mlua::Result<Function> {
        let fuel = self.fuel.clone();
        let activity = self.activity.clone();
        self.lua.create_function(move |_, args: mlua::MultiValue| {
            let _script = activity.as_ref().map(|a| a.span(Phase::UiNativeScript));
            // Deferred Canvas starts its own work allowance after layout.
            fuel.store(100_000, Ordering::Relaxed);
            paint.call::<()>(args)
        })
    }
    pub fn call<A: mlua::IntoLuaMulti>(&self, function: Function, args: A) -> mlua::Result<()> {
        let _script = self.activity.as_ref().map(|a| a.span(Phase::UiNativeScript));
        self.fuel.store(100_000, Ordering::Relaxed);
        self.calls.set(self.calls.get() + 1);
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
        #[cfg(test)]
        self.event_payloads.set(self.event_payloads.get() + 1);
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
    #[cfg(test)]
    pub(super) fn event_payload_count(&self) -> usize {
        self.event_payloads.get()
    }
    pub fn take_edits(&self) -> Vec<Edit> {
        self.bridge.lock_unpoisoned().edits.drain(..).collect()
    }
    pub fn unavailable_controls(&self) -> Vec<String> {
        self.bridge
            .lock_unpoisoned()
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
    fn editor_memory_drops_unresolved_child_factories() {
        let lua = Lua::new();
        lua.load("package={loaded={}} ").exec().unwrap();
        lua.load(include_str!("native_runtime/runtime.lua"))
            .exec()
            .unwrap();
        let (graph, watched): (Table, Table) = lua.load(r#"
            local watched=setmetatable({}, {__mode='v'})
            local function root()
                local child=__node('Text',function() return {text='visible'} end)
                watched[1]=child
                return __node('VStack',function() return {[1]=child, [3]=__node('Text',function() return {text='sparse'} end)} end)
            end
            return __render(root),watched
        "#).eval().unwrap();
        // The component memo keeps the elements it can reuse; the resolved
        // graph itself must not, so dropping the memo frees them.
        lua.globals().get::<Function>("__reset").unwrap().call::<()>(()).unwrap();
        lua.gc_collect().unwrap();
        lua.gc_collect().unwrap();
        assert!(
            watched.get::<LuaValue>(1).unwrap().is_nil(),
            "resolved graph retained an unused child factory"
        );
        let children: Table = graph.get("children").unwrap();
        assert_eq!(children.raw_len(), 2);
        for (n, text) in [(1, "visible"), (2, "sparse")] {
            assert_eq!(
                children
                    .get::<Table>(n)
                    .unwrap()
                    .get::<Table>("props")
                    .unwrap()
                    .get::<String>("text")
                    .unwrap(),
                text
            );
        }
        lua.load(
            r#"
            local child=__node('Text',function() return {text='reused'} end)
            local props={[1]=child}
            local callback=function() return props[1]==child end
            props.on_change=callback
            local root=function() return __node('VStack',function() return props end) end
            for _=1,2 do
                local graph=__render(root)
                assert(graph.children[1].props.text=='reused')
                assert(graph.props.on_change())
                assert(props[1]==child and props.on_change==callback)
            end
        "#,
        )
        .exec()
        .unwrap();
    }
    #[test]
    fn component_memo_re_resolves_only_what_read_the_change() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Resources/native_ui")).unwrap();
        std::fs::write(
            dir.path().join("Resources/native_ui/main.nui"),
            br#"local ui=require("native_ui")
                local kontakt=require("kontakt")
                local gain=kontakt.connect_parameter("gain")
                local meter=kontakt.connect_level_meter("gain")
                local function count(name) _G[name]=(_G[name] or 0)+1 end
                local function Param(props)
                    count("param")
                    local value=gain:value()
                    return @ui.Text {text=tostring(value)}
                end
                local function Level(props)
                    count("level")
                    return @ui.Rectangle {}.frame(width=4+meter:level_value()*10,height=4)
                end
                local function Plain(props)
                    count("plain")
                    return @ui.Rectangle {}.frame(width=5,height=5)
                end
                return function()
                    count("root")
                    local get,set=ui.create_state(0)
                    _G.bump=function() set(get()+1) end
                    local n=get()
                    return @ui.VStack {
                        @Param {}, @Level {}, @Plain {},
                        @ui.ForEach {data={1,2,3}, content=function(i) return @ui.Text {text=tostring(i+n)} end},
                    }
                end"#,
        ).unwrap();
        let package = Arc::new(Package::load(&dir.path().join("fixture.nki")).unwrap());
        let mut widget = ir::Widget::new(
            "$gain",
            ir::PageRef(0),
            Default::default(),
            ir::Kind::Knob { range: ir::Range { min: 0., max: 100., default: 0., step: Some(1.) }, display: Default::default() },
        );
        widget.value = Some(ir::Value::Integer(20));
        let source = ir::Source::Ksp { slot: 0 };
        let session = Session::new(package, "main", vec![(source, 0, widget.clone())]).unwrap();
        let runs = || ["root", "param", "level", "plain"].map(|name| session.lua().globals().get::<u32>(name).unwrap_or(0));
        let view = |value: i32, level: f64| {
            let mut widget = widget.clone();
            widget.value = Some(ir::Value::Integer(value));
            let face = ir::Interface { source, widgets: vec![widget], ..Default::default() };
            session.update_view(&face, &Default::default(), &Default::default(), &HashMap::from([(ir::WidgetRef(0), level)]));
        };
        // How many times each body ran for this render.
        let render = |expected: [u32; 4], why: &str| {
            let before = runs();
            let graph = session.render().unwrap();
            let after = runs();
            assert_eq!(std::array::from_fn(|n| after[n] - before[n]), expected, "{why}");
            // A resolve from scratch must produce the same graph.
            assert_eq!(session.verify(&graph), None, "{why}");
        };
        view(20, 0.);
        render([1, 1, 1, 1], "first render");
        view(20, 0.5);
        render([0, 0, 1, 0], "a meter level moves: only the component that read it");
        view(60, 0.5);
        render([0, 1, 0, 0], "a control value moves: only the component that read it");
        view(60, 0.5);
        render([0, 0, 0, 0], "nothing moved");
        session.lua().globals().get::<Function>("bump").unwrap().call::<()>(()).unwrap();
        render([1, 1, 1, 1], "a state write re-runs the component that read it and what it built");
    }
    #[test]
    fn native_readback_reuses_unchanged_widget_storage_and_keeps_source_semantics() {
        let (request, _jobs) = std::sync::mpsc::sync_channel(1);
        let package = Arc::new(Package {
            members: BTreeMap::from([(
                "main.nui".into(),
                Arc::from(b"return function() return {} end".as_slice()),
            )]),
            fonts: BTreeMap::new(),
            font_names: Vec::new(),
            images: Images {
                request,
                cache: Arc::new(Mutex::new(ImageCache {
                    loaded: HashMap::new(),
                    pending: BTreeSet::new(),
                    touch: HashMap::new(),
                    tick: 0,
                    bytes: 0,
                    #[cfg(feature = "shots")]
                    scan: Default::default(),
                })),
            },
        });
        let mut widget = ir::Widget::new(
            "$readback",
            ir::PageRef(0),
            Default::default(),
            ir::Kind::Label,
        );
        widget.binding = ir::Binding::Control(ir::ControlId(42));
        widget.value = Some(ir::Value::Integer(20));
        widget.text = "Authored caption".into();
        widget.tooltip = "Authored help".into();
        let mut face = ir::Interface {
            source: ir::Source::Ksp { slot: 2 },
            widgets: vec![widget.clone()],
            ..Default::default()
        };
        let session = Session::new(
            package,
            "main",
            vec![
                (face.source, 0, widget.clone()),
                (ir::Source::Ksp { slot: 4 }, 0, widget),
            ],
        )
        .unwrap();
        let values = HashMap::from([(ir::ControlId(42), 75.25)]);
        let meters = HashMap::from([(ir::WidgetRef(0), 0.625)]);
        for value in [
            ir::Value::Text("Callback readback".into()),
            ir::Value::Integers(vec![1, 2, 3]),
            ir::Value::Reals(vec![0.25, 0.5]),
            ir::Value::Integer(75),
            ir::Value::Real(75.25),
        ] {
            let typed = HashMap::from([(ir::WidgetRef(0), value.clone())]);
            session.update_view(&face, &values, &typed, &meters);
            let heap_calls = crate::plugin::tests::allocations(|| {
                for _ in 0..64 {
                    session.update_view(&face, &values, &typed, &meters);
                }
            });
            assert_eq!(
                heap_calls, 0,
                "unchanged Native readback recopied {value:?}"
            );
            let bridge = session.bridge.lock_unpoisoned();
            assert_eq!(bridge.controls[0].value.as_ref(), Some(&value));
            assert_eq!(bridge.controls[1].value, Some(ir::Value::Integer(20)));
            assert_eq!(bridge.meters.get(&0), Some(&0.625));
        }
        let typed = HashMap::from([(
            ir::WidgetRef(0),
            ir::Value::Text("Callback readback".into()),
        )]);
        session.update_view(&face, &values, &typed, &meters);
        face.widgets[0].text = "Changed caption".into();
        face.widgets[0].hidden = true;
        face.widgets[0].value = Some(ir::Value::Real(2.));
        session.update_view(&face, &values, &typed, &meters);
        {
            let bridge = session.bridge.lock_unpoisoned();
            assert_eq!(bridge.controls[0].text, "Changed caption");
            assert!(bridge.controls[0].hidden);
            assert_eq!(
                bridge.controls[0].value.as_ref(),
                typed.get(&ir::WidgetRef(0))
            );
        }
        session.update_view(&face, &values, &Default::default(), &meters);
        assert_eq!(
            session.bridge.lock_unpoisoned().controls[0].value,
            Some(ir::Value::Real(75.25)),
            "typed removal uses the current authored numeric type"
        );
        for value in [
            ir::Value::Text("Authored text".into()),
            ir::Value::Integers(vec![9, 8]),
            ir::Value::Reals(vec![0.75, 0.25]),
        ] {
            face.widgets[0].value = Some(value.clone());
            session.update_view(&face, &values, &Default::default(), &meters);
            assert_eq!(
                session.bridge.lock_unpoisoned().controls[0].value.as_ref(),
                Some(&value),
                "scalar telemetry cannot replace an authored typed value"
            );
        }
    }
    #[test]
    fn unchanged_inputs_reuse_the_rendered_graph() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Resources/native_ui")).unwrap();
        std::fs::write(
            dir.path().join("Resources/native_ui/main.nui"),
            br#"local ui=require("native_ui")
                local kontakt=require("kontakt")
                local gain=kontakt.connect_parameter("gain")
                local meter=kontakt.connect_level_meter("gain")
                return function()
                    local get,set=ui.create_state(0)
                    _G.bump=function() set(get()+1) end
                    _G.renders=(_G.renders or 0)+1
                    local text=get()..":"..gain:value()..":"..meter:level_value()
                    return @ui.Text {text=text}
                end"#,
        ).unwrap();
        let package = Arc::new(Package::load(&dir.path().join("fixture.nki")).unwrap());
        let mut widget = ir::Widget::new(
            "$gain",
            ir::PageRef(0),
            Default::default(),
            ir::Kind::Knob { range: ir::Range { min: 0., max: 100., default: 0., step: Some(1.) }, display: Default::default() },
        );
        widget.value = Some(ir::Value::Integer(20));
        let source = ir::Source::Ksp { slot: 0 };
        let session = Session::new(package, "main", vec![(source, 0, widget.clone())]).unwrap();
        let renders = || session.lua().globals().get::<u32>("renders").unwrap();
        let view = |value: i32, meter: f64| {
            let mut widget = widget.clone();
            widget.value = Some(ir::Value::Integer(value));
            let face = ir::Interface { source, widgets: vec![widget], ..Default::default() };
            session.update_view(&face, &Default::default(), &Default::default(), &HashMap::from([(ir::WidgetRef(0), meter)]));
        };
        view(20, 0.);
        let first = session.render().unwrap();
        session.render().unwrap();
        assert_eq!(renders(), 1, "nothing changed: the Lua tree is reused");
        session.lua().globals().get::<Function>("bump").unwrap().call::<()>(()).unwrap();
        session.render().unwrap();
        assert_eq!(renders(), 2, "a state write re-resolves what read it");
        let noop: Function = session.lua().load("return function() end").eval().unwrap();
        session.call(noop, ()).unwrap();
        let same = session.render().unwrap();
        assert_eq!(renders(), 2, "a callback that moved nothing tracked leaves the tree alone");
        assert!(same.equals(&session.render().unwrap()).unwrap());
        view(33, 0.);
        let moved = session.render().unwrap();
        assert_eq!(renders(), 3, "a changed control value re-resolves what read it");
        assert!(!first.equals(&moved).unwrap());
        view(33, 0.5);
        session.render().unwrap();
        assert_eq!(renders(), 4, "a meter level re-resolves what read it");
        view(33, 0.5);
        session.render().unwrap();
        assert_eq!(renders(), 4, "the same meter level changes nothing");
    }
    #[test]
    fn native_profiler_counts_connected_script_and_deferred_paint_calls() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Resources/native_ui")).unwrap();
        std::fs::write(
            dir.path().join("Resources/native_ui/main.nui"),
            br#"local ui=require("native_ui")
                return function() return @ui.Rectangle {}.frame(width=8,height=8) end"#,
        ).unwrap();
        let package = Arc::new(Package::load(&dir.path().join("fixture.nki")).unwrap());
        let mut session = Session::new(package, "main", vec![]).unwrap();
        // The ordinary disabled session renders without collecting timings.
        session.render().unwrap();
        assert!(session.activity().is_none());
        let activity = Arc::new(Activity::new(true));
        session.set_activity(Some(activity.clone()));
        session.render().unwrap();
        let failed: Function = session.lua().load("return function() error('owned failure') end").eval().unwrap();
        assert!(session.call(failed, ()).is_err());
        let paint: Function = session.lua().load("return function() end").eval().unwrap();
        let deferred = session.paint_callback(paint).unwrap();
        deferred.call::<()>(()).unwrap();
        let values = activity.phase_snapshot().unwrap();
        let at = Phase::UiNativeScript as usize * 3;
        assert_eq!(values[at], 3, "render, failed event and deferred paint all close spans");
        assert!(values[at + 1] >= values[at + 2], "total includes maximum");
        assert!(values.iter().enumerate().all(|(n, v)| (at..at + 3).contains(&n) || *v == 0));
        assert_eq!(activity.snapshot().unwrap(), [0; crate::plugin::ui_activity::FIELDS.len()]);
    }

    #[test]
    fn legacy_component_reads_the_published_ir_and_produces_a_typed_edit() {
        assert_eq!(
            Package::load_cancel(Path::new("/unopened/synthetic.nki"), || true)
                .err()
                .unwrap()
                .to_string(),
            "NativeUI preparation canceled"
        );
        let (request, _jobs) = std::sync::mpsc::sync_channel(1);
        let package=Arc::new(Package{members:BTreeMap::from([("main.nui".into(),Arc::from(br#"local ui=require("native_ui")
            local kontakt=require("kontakt")
            local p=kontakt.connect_parameter("gain")
            local meter=kontakt.connect_level_meter("gain")
            return function()
                p:set_value(0.75)
                return @ui.ZStack { @ui.Text {text=tostring(meter:level_value()),}, @ui.Rectangle {color=ui.Color(12,24,48)}.frame(width=10,height=20) }.frame(width=80,height=60)
            end"#.as_slice()))]),fonts:BTreeMap::new(),font_names:Vec::new(),images:Images{request,cache:Arc::new(Mutex::new(ImageCache{loaded:HashMap::new(),pending:BTreeSet::new(),touch:HashMap::new(),tick:0,bytes:0,#[cfg(feature="shots")] scan:Default::default()}))}});
        let font = Font::new(super::super::theme::NOTO_SANS).unwrap();
        let (names, weight) = font_metadata(&font);
        assert!(names.iter().any(|n| n == "Noto Sans"));
        let mut package = package;
        assert!(package.images.get("queued.png").is_none());
        for n in 0..5000 {
            assert!(package.images.get(&format!("rejected-{n}.png")).is_none());
        }
        assert_eq!(
            package.images.cache.lock_unpoisoned().touch.len(),
            1,
            "a full worker queue must not retain rejected image-name metadata"
        );
        let supplied = Arc::get_mut(&mut package).unwrap();
        supplied
            .fonts
            .insert("unrelated-file.ttf".into(), font.clone());
        supplied.font_names = names
            .into_iter()
            .map(|n| (n, weight, font.clone()))
            .collect();
        assert_eq!(supplied.font("Noto Sans", false).unwrap().id(), font.id());
        assert!(supplied.font("an absent authored family", false).is_none());
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
            vec![
                (ir::Source::Ksp { slot: 4 }, 0, widget.clone()),
                (ir::Source::Ksp { slot: 2 }, 0, widget.clone()),
                (ir::Source::Ksp { slot: 3 }, 0, widget),
            ],
        )
        .unwrap();
        let source = ir::Interface {
            source: ir::Source::Ksp { slot: 2 },
            widgets: vec![session.bridge.lock_unpoisoned().controls[1].clone()],
            ..Default::default()
        };
        session.update_view(
            &source,
            &HashMap::from([(ir::ControlId(42), 75.)]),
            &HashMap::from([(
                ir::WidgetRef(0),
                ir::Value::Text("callback readback".into()),
            )]),
            &Default::default(),
        );
        assert_eq!(
            session.bridge.lock_unpoisoned().controls[1].value,
            Some(ir::Value::Text("callback readback".into()))
        );
        assert_eq!(
            session.bridge.lock_unpoisoned().controls[0].value,
            Some(ir::Value::Integer(20)),
            "another source is not overwritten by this source's scalar fallback"
        );
        let mut typed_source = source.clone();
        for saved in [
            ir::Value::Text("published text".into()),
            ir::Value::Integers(vec![1, 2, 3]),
            ir::Value::Reals(vec![0.25, 0.5]),
        ] {
            typed_source.widgets[0].value = Some(saved.clone());
            session.update_view(
                &typed_source,
                &HashMap::from([(ir::ControlId(42), 75.)]),
                &Default::default(),
                &Default::default(),
            );
            assert_eq!(
                session.bridge.lock_unpoisoned().controls[1].value,
                Some(saved),
                "scalar telemetry must preserve declared text/array values"
            );
        }
        session.update_view(
            &source,
            &Default::default(),
            &HashMap::from([(ir::WidgetRef(0), ir::Value::Integer(20))]),
            &HashMap::from([(ir::WidgetRef(0), 0.625)]),
        );
        let graph = session.render().unwrap();
        assert_eq!(graph.get::<String>("kind").unwrap(), "ZStack");
        let meter: Table = graph.get::<Table>("children").unwrap().get(1).unwrap();
        assert_eq!(
            meter
                .get::<Table>("props")
                .unwrap()
                .get::<String>("text")
                .unwrap(),
            "0.625"
        );
        let edits = session.take_edits();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].value, ir::Value::Integer(75));
        assert_eq!(edits[0].source_id, Some(7));
        assert_eq!(edits[0].source, ir::Source::Ksp { slot: 2 });
        assert!(session.unavailable_controls().is_empty());
        // Requested faces may contain bindings absent from this instrument.
        session
            .lua()
            .load(
                r#"
            local kontakt=require('kontakt')
            local missing=kontakt.connect_parameter('undeclared')
            local missing_bool=kontakt.connect_parameter('undeclared_bool','bool')
            local meter=kontakt.connect_level_meter('undeclared_meter')
            assert(missing.connected==false and missing_bool.connected==false)
            assert(meter.connected==false and meter:level_value()==0)
            assert(missing:value()==0 and missing_bool:value()==false)
            for _,property in ipairs({0,1,14,18}) do
                assert(missing:ksp_control_property(property)=='')
            end
            assert('caption:'..missing:ksp_control_property(0)=='caption:')
            assert(missing:ksp_control_property(17)==0 and missing:ksp_control_property(20)==false)
            missing:set_value(0.75)
            missing:update_touch()
            assert(missing:is_touch_active()==false)
            missing:end_touch()
        "#,
            )
            .exec()
            .unwrap();
        assert!(
            session.take_edits().is_empty(),
            "unconnected bindings cannot edit a declared control"
        );
        assert!(session.bridge.lock_unpoisoned().touches.is_empty());
        assert!(
            session
                .lua()
                .globals()
                .get::<LuaValue>("dofile")
                .unwrap()
                .is_nil()
        );
        // Context modifiers affect descendants without leaking into siblings.
        let context_source = syntax::translate(r#"
            local ui=require('native_ui')
            local key=ui.create_context()
            local child=function() local value=ui.use_context(key); return @ui.Text {text=value() or 'base'} end
            return function() return @ui.ZStack {
                @child {}.context(key=key,value='nested')
                @child {}
            } end
        "#).unwrap();
        let context_root: Function = session.lua().load(&context_source).eval().unwrap();
        let render: Function = session.lua().globals().get("__render").unwrap();
        let graph: Table = render.call(context_root).unwrap();
        let children: Table = graph.get("children").unwrap();
        let nested: Table = children
            .get::<Table>(1)
            .unwrap()
            .get::<Table>("children")
            .unwrap()
            .get(1)
            .unwrap();
        let sibling: Table = children.get(2).unwrap();
        assert_eq!(
            nested
                .get::<Table>("props")
                .unwrap()
                .get::<String>("text")
                .unwrap(),
            "nested"
        );
        assert_eq!(
            sibling
                .get::<Table>("props")
                .unwrap()
                .get::<String>("text")
                .unwrap(),
            "base"
        );
        let forever: Function = session
            .lua()
            .load("return function() while true do end end")
            .eval()
            .unwrap();
        assert!(
            session
                .call(forever, ())
                .unwrap_err()
                .to_string()
                .contains("NativeUI interrupt budget exceeded")
        );
        assert_eq!(
            session.work_remaining(),
            0,
            "pathological loops must exhaust deterministic work"
        );
    }
}
