//! Kontakt 8 `native_ui`: its modules rewritten to Luau ([`transform`]), run
//! in a sandboxed state with the same limits as a UVI script's, and rendered
//! to a tree of primitive [`Node`]s for the host to lay out. Runs on the
//! control thread, never the audio thread.

pub mod transform;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use mlua::{Function, Lua, Table, Value};

const PRELUDE: &str = include_str!("nui/prelude.lua");

/// Why a page shows nothing: the module, its line when known, the reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub module: String,
    pub line: Option<u32>,
    pub message: String,
}

/// A primitive the page draws, with its properties, its modifiers in order
/// (`frame`, `padding`, ...) and its children.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub kind: String,
    pub props: Vec<(String, Prop)>,
    pub mods: Vec<(String, Vec<Prop>)>,
    pub children: Vec<Node>,
    /// The path of the component instance this is the root of: where
    /// [`Page::update`] puts that instance back when it renders again.
    pub instance: Option<String>,
    /// The parameters that instance read, by id.
    pub reads: Vec<String>,
}

impl Node {
    /// Property `name`.
    #[must_use]
    pub fn prop(&self, name: &str) -> Option<&Prop> {
        self.props.iter().find(|(k, _)| k == name).map(|(_, v)| v)
    }
}

/// A property or modifier argument.
#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    Number(f64),
    Text(String),
    Bool(bool),
    /// 0..255 channels, alpha 0..1.
    Color([f64; 4]),
    Node(Box<Node>),
    /// Named arguments (`.frame(width = 1)`) or a plain table.
    Table(Vec<(String, Prop)>),
    /// A function the page holds (a gesture), by number for [`Page::call`].
    Callback(u32),
}

impl Prop {
    #[must_use]
    pub fn number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// Field `name` of a table.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&Prop> {
        match self {
            Self::Table(t) => t.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// A control the page connects to by name, as the host drives it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Control {
    pub name: String,
    /// Its text (KSP `$CONTROL_PAR_TEXT`, the page's property 0).
    pub label: String,
    /// Its value as shown (`$CONTROL_PAR_LABEL`, the page's property 14).
    pub value_text: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    /// Whether the page reads it 0..1 across `min..max` (a knob, slider or
    /// value edit), not raw (a menu, a button). An index (`value(i)`,
    /// `set_value(v, i)`) reads and writes raw either way.
    pub normalized: bool,
    /// A menu's entries: text and value.
    pub entries: Vec<(String, f64)>,
}

/// A loaded page: its state, the count its interrupt stops at, its arena.
pub struct Page {
    lua: Lua,
    checkpoints: Arc<AtomicU32>,
    root: Function,
    // Declared last: dropped after the state.
    _arena: crate::alloc::Claim,
}

impl std::fmt::Debug for Page {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("nui::Page")
    }
}

/// Loads the page `entry` (`"main"`) from `modules`: each `.nui` path under
/// the library's `native_ui` folder, and its source.
///
/// # Errors
/// The first module the rewrite does not cover, or the entry's load error.
pub fn load(modules: &[(String, String)], entry: &str) -> Result<Page, Diagnostic> {
    let mut sources = HashMap::new();
    for (path, src) in modules {
        let name = path.trim_end_matches(".nui").replace('\\', "/");
        let lua = transform::transform(src).map_err(|e| Diagnostic {
            module: path.clone(),
            line: Some(e.line),
            message: e.message,
        })?;
        sources.insert(name, lua);
    }
    let fail = |module: &str| {
        let module = module.to_owned();
        move |e: mlua::Error| Diagnostic {
            module: module.clone(),
            line: None,
            message: e.to_string(),
        }
    };
    let arena = crate::alloc::Claim::arena();
    let load = arena.load();
    let (lua, checkpoints, limit) = crate::sandbox().map_err(fail("native_ui"))?;
    lua.load(PRELUDE)
        .set_name("native_ui")
        .exec()
        .map_err(fail("native_ui"))?;
    let require = require(&lua, sources).map_err(fail("native_ui"))?;
    lua.globals()
        .set("require", require.clone())
        .map_err(fail("native_ui"))?;
    let root = require.call::<Function>(entry).map_err(fail(entry))?;
    limit.store(crate::CHECKPOINTS, Ordering::Relaxed);
    drop(load);
    Ok(Page {
        lua,
        checkpoints,
        root,
        _arena: arena,
    })
}

/// `require` over the page's modules: `a.b` is `a/b.nui`, else
/// `a/b/init.nui`; `native_ui`, `native_ui_util` and `kontakt` are built in.
fn require(lua: &Lua, sources: HashMap<String, String>) -> mlua::Result<Function> {
    let loaded = lua.create_table()?;
    let builtins: Table = lua.globals().get("__builtins")?;
    lua.create_function(move |lua, name: String| {
        if let Ok(v) = builtins.get::<Value>(name.as_str())
            && !v.is_nil()
        {
            return Ok(v);
        }
        let path = name.replace('.', "/");
        let init = format!("{path}/init");
        let (key, text) = [&path, &init]
            .into_iter()
            .find_map(|k| sources.get(k.as_str()).map(|t| (k.clone(), t)))
            .ok_or_else(|| mlua::Error::runtime(format!("module {name} not found")))?;
        if let Ok(v) = loaded.get::<Value>(key.as_str())
            && !v.is_nil()
        {
            return Ok(v);
        }
        let value: Value = lua.load(text.as_str()).set_name(&key).call(())?;
        let value = if value.is_nil() {
            Value::Boolean(true)
        } else {
            value
        };
        loaded.set(key, value.clone())?;
        Ok(value)
    })
}

impl Page {
    /// The controls the page's parameters read their properties from, over
    /// those it had.
    ///
    /// # Errors
    /// Only when the state is out of memory.
    pub fn set_controls(&mut self, controls: &[Control]) -> Result<(), Diagnostic> {
        let fail = |e: mlua::Error| Diagnostic {
            module: "native_ui".into(),
            line: None,
            message: e.to_string(),
        };
        let lua = &self.lua;
        let all: Table = lua.globals().get("__controls").map_err(fail)?;
        for c in controls {
            let t = lua.create_table().map_err(fail)?;
            t.set("label", c.label.as_str()).map_err(fail)?;
            t.set("value_text", c.value_text.as_str()).map_err(fail)?;
            t.set("min", c.min).map_err(fail)?;
            t.set("max", c.max).map_err(fail)?;
            t.set("default", c.default).map_err(fail)?;
            t.set("normalized", c.normalized).map_err(fail)?;
            let entries = lua.create_table().map_err(fail)?;
            for (n, (text, value)) in (1..).zip(&c.entries) {
                let e = lua.create_table().map_err(fail)?;
                e.set("text", text.as_str()).map_err(fail)?;
                e.set("value", *value).map_err(fail)?;
                e.set("shown", true).map_err(fail)?;
                entries.set(n, e).map_err(fail)?;
            }
            t.set("entries", entries).map_err(fail)?;
            all.set(c.name.as_str(), t).map_err(fail)?;
        }
        Ok(())
    }

    /// The tree as it stands, with `params` (control name and value) read
    /// by the page's parameters.
    ///
    /// # Errors
    /// The page's runtime error, as Luau reports it.
    pub fn render(&mut self, params: &[(String, f64)]) -> Result<Option<Node>, Diagnostic> {
        let fail = |e: mlua::Error| Diagnostic {
            module: "native_ui".into(),
            line: None,
            message: e.to_string(),
        };
        self.checkpoints.store(0, Ordering::Relaxed);
        let globals = self.lua.globals();
        let table: Table = globals.get("__params").map_err(fail)?;
        for (name, value) in params {
            table.set(name.as_str(), *value).map_err(fail)?;
        }
        let render: Function = globals.get("__render").map_err(fail)?;
        let tree: Value = render.call(self.root.clone()).map_err(fail)?;
        Ok(node(&tree))
    }

    /// Whether a state changed since the page last rendered: an
    /// [`Page::update`] has instances to render again.
    #[must_use]
    pub fn dirty(&self) -> bool {
        self.lua.globals().get("__dirty").unwrap_or(false)
    }

    /// Runs the page's callback `n` (a gesture's) with an event at
    /// `position` in a `frame` (width, height) large, still; what it
    /// wrote, as (parameter id, value).
    ///
    /// # Errors
    /// The page's runtime error, as Luau reports it.
    pub fn call(
        &mut self,
        n: u32,
        position: [f64; 2],
        frame: [f64; 2],
    ) -> Result<Vec<(String, f64)>, Diagnostic> {
        let fail = |e: mlua::Error| Diagnostic {
            module: "native_ui".into(),
            line: None,
            message: e.to_string(),
        };
        self.checkpoints.store(0, Ordering::Relaxed);
        let lua = &self.lua;
        let pair = |a: &str, b: &str, [x, y]: [f64; 2]| -> mlua::Result<Table> {
            let t = lua.create_table()?;
            t.set(a, x)?;
            t.set(b, y)?;
            Ok(t)
        };
        let event = lua.create_table().map_err(fail)?;
        event
            .set("position", pair("x", "y", position).map_err(fail)?)
            .map_err(fail)?;
        event
            .set("start_position", pair("x", "y", position).map_err(fail)?)
            .map_err(fail)?;
        event
            .set("delta", pair("x", "y", [0.; 2]).map_err(fail)?)
            .map_err(fail)?;
        event
            .set("frame", pair("width", "height", frame).map_err(fail)?)
            .map_err(fail)?;
        event
            .set("modifiers", lua.create_table().map_err(fail)?)
            .map_err(fail)?;
        let call: Function = lua.globals().get("__call").map_err(fail)?;
        let writes: Table = call.call((n, event)).map_err(fail)?;
        Ok(writes
            .sequence_values::<Table>()
            .flatten()
            .filter_map(|w| Some((w.get::<String>(1).ok()?, w.get::<f64>(2).ok()?)))
            .collect())
    }

    /// Takes `params` (control name and value) and `controls` (their
    /// properties, which count as changed) and renders again only the
    /// component instances that read one that changed, or whose state
    /// changed: each instance's path and its new tree, outermost only.
    /// Nothing changed is nothing to do. The root's path is `""`.
    ///
    /// # Errors
    /// The page's runtime error, as Luau reports it.
    pub fn update(
        &mut self,
        params: &[(String, f64)],
        controls: &[Control],
    ) -> Result<Vec<(String, Option<Node>)>, Diagnostic> {
        let fail = |e: mlua::Error| Diagnostic {
            module: "native_ui".into(),
            line: None,
            message: e.to_string(),
        };
        let globals = self.lua.globals();
        let table: Table = globals.get("__params").map_err(fail)?;
        let changed = self.lua.create_table().map_err(fail)?;
        for (name, value) in params {
            if table.get::<Option<f64>>(name.as_str()).map_err(fail)? != Some(*value) {
                table.set(name.as_str(), *value).map_err(fail)?;
                changed.set(name.as_str(), true).map_err(fail)?;
            }
        }
        self.set_controls(controls)?;
        for c in controls {
            changed.set(c.name.as_str(), true).map_err(fail)?;
        }
        let dirty: bool = globals.get("__dirty").map_err(fail)?;
        if changed.is_empty() && !dirty {
            return Ok(Vec::new());
        }
        self.checkpoints.store(0, Ordering::Relaxed);
        let update: Function = globals.get("__update").map_err(fail)?;
        let out: Table = update.call(changed).map_err(fail)?;
        Ok(out
            .sequence_values::<Table>()
            .flatten()
            .filter_map(|r| {
                let path = r.get::<String>("path").ok()?;
                Some((path, node(&r.get::<Value>("tree").ok()?)))
            })
            .collect())
    }
}

fn node(v: &Value) -> Option<Node> {
    let Value::Table(t) = v else { return None };
    let kind = t.get::<String>("kind").ok()?;
    let props = t
        .get::<Table>("props")
        .map(|p| fields(&p))
        .unwrap_or_default();
    let mods = t
        .get::<Table>("mods")
        .map(|m| {
            m.sequence_values::<Table>()
                .flatten()
                .filter_map(|m| {
                    let name = m.get::<String>(1).ok()?;
                    let args = m
                        .sequence_values::<Value>()
                        .flatten()
                        .skip(1)
                        .filter_map(|a| prop(&a))
                        .collect();
                    Some((name, args))
                })
                .collect()
        })
        .unwrap_or_default();
    let children = t
        .get::<Table>("children")
        .map(|c| {
            c.sequence_values::<Value>()
                .flatten()
                .filter_map(|c| node(&c))
                .collect()
        })
        .unwrap_or_default();
    Some(Node {
        kind,
        props,
        mods,
        children,
        instance: t.get::<Option<String>>("instance").ok().flatten(),
        reads: t
            .get::<Table>("reads")
            .map(|r| r.sequence_values::<String>().flatten().collect())
            .unwrap_or_default(),
    })
}

fn fields(t: &Table) -> Vec<(String, Prop)> {
    let mut out: Vec<(String, Prop)> = t
        .pairs::<Value, Value>()
        .flatten()
        .filter_map(|(k, v)| {
            let k = match k {
                Value::String(s) => s.to_str().ok()?.to_owned(),
                Value::Integer(i) => i.to_string(),
                Value::Number(n) => n.to_string(),
                _ => return None,
            };
            Some((k, prop(&v)?))
        })
        .collect();
    // Table order is not the source's: sorted, so renders compare.
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn prop(v: &Value) -> Option<Prop> {
    Some(match v {
        Value::Integer(i) => Prop::Number(*i as f64),
        Value::Number(n) => Prop::Number(*n),
        Value::Boolean(b) => Prop::Bool(*b),
        Value::String(s) => Prop::Text(s.to_str().ok()?.to_owned()),
        Value::Table(t) => {
            if t.get::<bool>("__color").unwrap_or(false) {
                let c = |k: &str| t.get::<f64>(k).unwrap_or(0.);
                Prop::Color([c("r"), c("g"), c("b"), c("a")])
            } else if let Ok(n) = t.get::<u32>("__callback") {
                Prop::Callback(n)
            } else if t.contains_key("kind").unwrap_or(false) {
                Prop::Node(Box::new(node(v)?))
            } else {
                Prop::Table(fields(t))
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(modules: &[(&str, &str)]) -> Page {
        let modules: Vec<(String, String)> = modules
            .iter()
            .map(|(p, s)| ((*p).to_owned(), (*s).to_owned()))
            .collect();
        load(&modules, "main").unwrap()
    }

    #[test]
    fn a_page_renders_primitives_through_components_and_contexts() {
        let mut page = page(&[
            (
                "lib/init.nui",
                r#"local ui = require "native_ui"
                local Theme = ui.create_context("light")
                return {
                    Theme = Theme,
                    Label = function(props)
                        local theme = ui.use_context(Theme)().get()
                        return @ui.Text { text = props.text .. "/" .. theme }
                        .frame(width = ui.create_computed(function() return 40 end))
                        .on_tap_gesture(function() end)
                    end,
                }"#,
            ),
            (
                "main.nui",
                r#"local ui = require "native_ui"
                local lib = require "lib"
                local kontakt = require "kontakt"
                return function()
                    local page = kontakt.connect_parameter("Header__Page")
                    local theme, set_theme = ui.create_state("dark")
                    return @ui.VStack {
                        spacing = 2
                        @lib.Label { text = "a" }
                        @{ page:value() == 1 and @ui.Spacer{} }
                        @ui.ForEach {
                            data = { "x" }
                            content = function(item, index) return @ui.Text { text = index } end
                        }
                    }
                    .context(key = lib.Theme, value = { get = theme, set = set_theme })
                end"#,
            ),
        ]);
        let tree = page
            .render(&[("Header__Page".into(), 1.)])
            .unwrap()
            .unwrap();
        assert_eq!(tree.kind, "VStack");
        assert_eq!(tree.prop("spacing"), Some(&Prop::Number(2.)));
        let label = &tree.children[0];
        assert_eq!(label.prop("text"), Some(&Prop::Text("a/dark".into())));
        assert_eq!(label.mods[0].0, "frame");
        // A computed named argument is read; a gesture's stays a callback.
        assert_eq!(label.mods[0].1[0].field("width"), Some(&Prop::Number(40.)));
        assert!(matches!(label.mods[1].1[0], Prop::Callback(_)));
        assert_eq!(tree.children[1].kind, "Spacer");
        // A ForEach's index getter is read, not numbered as a callback.
        let each = &tree.children[2];
        assert_eq!(each.children[0].prop("text"), Some(&Prop::Number(1.)));
        // The parameter moved: the spacer goes.
        let tree = page
            .render(&[("Header__Page".into(), 0.)])
            .unwrap()
            .unwrap();
        assert_eq!(tree.children.len(), 2);
        // The binding's setter was passed, never called: the state holds.
        let text = tree.children[0].prop("text");
        assert_eq!(text, Some(&Prop::Text("a/dark".into())));
    }

    /// A ranged control reads 0..1 across its range, and raw by index (a
    /// table's cell, as Conflux's bend strip reads it); both writes are
    /// raw for the host.
    #[test]
    fn a_ranged_control_reads_0_to_1_and_raw_by_index() {
        let mut page = page(&[(
            "main.nui",
            r#"local ui = require "native_ui"
            local kontakt = require "kontakt"
            return function()
                local pb = kontakt.connect_parameter("PB")
                return @ui.VStack {
                    @ui.Text { text = pb:value() }
                    @ui.Text { text = pb:value(0) }
                }
                .on_tap_gesture(
                    start = function() pb:set_value(0.5) end,
                    complete = function() pb:set_value(-100, 0) end)
            end"#,
        )]);
        let pb = Control {
            name: "PB".into(),
            label: String::new(),
            value_text: String::new(),
            min: -100.,
            max: 100.,
            default: 0.,
            normalized: true,
            entries: Vec::new(),
        };
        page.set_controls(std::slice::from_ref(&pb)).unwrap();
        let tree = page.render(&[("PB".into(), 100.)]).unwrap().unwrap();
        let text = |i: usize| tree.children[i].prop("text").cloned();
        assert_eq!(text(0), Some(Prop::Number(1.)));
        assert_eq!(text(1), Some(Prop::Number(100.)));
        let callback = |k: &str| match tree.mods[0].1[0].field(k) {
            Some(Prop::Callback(n)) => *n,
            _ => 0,
        };
        let (start, complete) = (callback("start"), callback("complete"));
        let writes = page.call(start, [0.; 2], [1.; 2]).unwrap();
        assert_eq!(writes, [("PB".to_owned(), 0.)]);
        let writes = page.call(complete, [0.; 2], [1.; 2]).unwrap();
        assert_eq!(writes, [("PB".to_owned(), -100.)]);
    }

    #[test]
    fn a_module_the_rewrite_does_not_cover_is_named_with_its_line() {
        let modules = vec![("main.nui".to_owned(), "return 1\nreturn @ 3".to_owned())];
        let err = load(&modules, "main").unwrap_err();
        assert_eq!((err.module.as_str(), err.line), ("main.nui", Some(2)));
    }
}

