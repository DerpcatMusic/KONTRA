//! Creator Tools performance views become ordinary KSP declarations at load time.
//! Resource I/O and JSON parsing happen before compilation, never in a callback.
use super::lexer::{Punct, Tok, Tokens, kw, lex};
use super::parser::preprocess;
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};
use std::fmt::Write;
use std::path::Path;

pub(super) struct Prepared {
    pub tokens: Tokens,
    pub conditions: BTreeSet<String>,
    /// Exact resource content participates in the shared compiled-program key.
    pub resource: Vec<u8>,
}

pub(super) fn prepare(
    source: &str,
    inherited: &BTreeSet<String>,
    instrument: Option<&Path>,
) -> Result<Prepared> {
    let mut tokens = lex(source)?;
    let conditions = preprocess(&mut tokens, inherited)?;
    let mut resource = Vec::new();
    let mut init = false;
    let mut found = false;
    let mut explicit_perfview = false;
    let mut at = 0;
    while at < tokens.toks.len() {
        if tokens.toks[at] == Tok::Ident(kw::ON) {
            init = matches!(tokens.toks.get(at + 1), Some(Tok::Ident(s)) if tokens.syms.name(*s) == "init");
        }
        if tokens.toks[at] == Tok::Ident(kw::END)
            && tokens.toks.get(at + 1) == Some(&Tok::Ident(kw::ON))
        {
            init = false;
        }
        explicit_perfview |= init
            && matches!(tokens.toks[at], Tok::Ident(s) if tokens.syms.name(s) == "make_perfview");
        let is_load = matches!(tokens.toks[at], Tok::Ident(s) if tokens.syms.name(s) == "load_performance_view")
            && tokens.toks.get(at + 1) == Some(&Tok::Punct(Punct::LParen));
        if !is_load {
            at += 1;
            continue;
        }
        let line = tokens.lines[at];
        ensure!(
            init,
            "load_performance_view is only available during on init (line {line})"
        );
        ensure!(
            !found,
            "Only one performance view can be loaded per script slot (line {line})"
        );
        found = true;
        let Some(Tok::Str(name)) = tokens.toks.get(at + 2) else {
            bail!("load_performance_view requires a literal resource name (line {line})");
        };
        ensure!(
            tokens.toks.get(at + 3) == Some(&Tok::Punct(Punct::RParen))
                && matches!(tokens.toks.get(at + 4), Some(Tok::Newline | Tok::Eof))
                && (at == 0 || tokens.toks[at - 1] == Tok::Newline),
            "load_performance_view must be an init statement (line {line})"
        );
        let name = tokens.syms.name(*name);
        ensure!(
            !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != "..",
            "Invalid performance-view resource name at line {line}"
        );
        let instrument = instrument.context(format!(
            "Performance view {name:?} requires an instrument resource path (line {line})"
        ))?;
        let file = if name.ends_with(".nckp") {
            name.to_owned()
        } else {
            format!("{name}.nckp")
        };
        let bytes = crate::resources::Resources::of(instrument, "performance_view")
            .read(&file)
            .map_err(anyhow::Error::msg)?
            .with_context(|| format!("Performance view {file:?} was not found (line {line})"))?;
        let view = View::read(&bytes)
            .with_context(|| format!("Performance view {file:?} at line {line}"))?;
        let mut extra = lex(&view.source)?;
        extra.toks.pop(); // The original source owns EOF and callback boundaries.
        let mapped: Vec<_> = extra
            .toks
            .into_iter()
            .map(|t| match t {
                Tok::Ident(s) => Tok::Ident(tokens.syms.intern(extra.syms.name(s))),
                Tok::Var(s) => Tok::Var(tokens.syms.intern(extra.syms.name(s))),
                Tok::Str(s) => {
                    let name = extra.syms.name(s);
                    let text = name
                        .strip_prefix("__nckp_string_")
                        .and_then(|n| n.parse::<usize>().ok())
                        .and_then(|n| view.strings.get(n))
                        .map_or(name, String::as_str);
                    Tok::Str(tokens.syms.intern(text))
                }
                Tok::Real(r) => {
                    tokens.reals.push(extra.reals[r as usize]);
                    Tok::Real(tokens.reals.len() as u32 - 1)
                }
                other => other,
            })
            .collect();
        let len = mapped.len();
        tokens.toks.splice(at..at + 4, mapped);
        tokens
            .lines
            .splice(at..at + 4, std::iter::repeat_n(line, len));
        resource = bytes;
        at += len;
    }
    ensure!(
        !found || !explicit_perfview,
        "load_performance_view cannot be combined with make_perfview"
    );
    Ok(Prepared {
        tokens,
        conditions,
        resource,
    })
}

struct View {
    source: String,
    strings: Vec<String>,
    names: HashSet<String>,
}

fn int(v: &Value, key: &str) -> Result<i32> {
    v.get(key)
        .and_then(Value::as_i64)
        .and_then(|n| i32::try_from(n).ok())
        .with_context(|| format!("Missing or invalid integer {key}"))
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("Missing or invalid string {key}"))
}
fn boolean(v: &Value, key: &str) -> Result<bool> {
    v.get(key)
        .and_then(Value::as_bool)
        .with_context(|| format!("Missing or invalid boolean {key}"))
}

impl View {
    fn read(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= 8 * 1024 * 1024,
            "Performance view exceeds 8 MiB"
        );
        let json: Value = serde_json::from_slice(bytes).context("Invalid NCKP JSON")?;
        ensure!(
            int(&json, "formatVersion")? == 0
                && string(&json["type"], "name")? == "UI"
                && int(&json["type"], "version")? == 0,
            "Unsupported NCKP format/type version"
        );
        let perf = &json["value"]["performanceView"];
        let mut view = Self {
            source: String::new(),
            strings: Vec::new(),
            names: HashSet::new(),
        };
        let width = int(&perf["size"], "width")?;
        let height = int(&perf["size"], "height")?;
        ensure!(
            (1..=4096).contains(&width) && (1..=4096).contains(&height),
            "Invalid performance-view size"
        );
        writeln!(
            view.source,
            "make_perfview\nset_ui_width_px({width})\nset_ui_height_px({height})"
        )?;
        if let Some(color) = perf["background"].get("color") {
            let color = color
                .as_i64()
                .filter(|n| (0..=0xffffff).contains(n))
                .context("Invalid view background color")?;
            writeln!(view.source, "set_ui_color({color})")?;
        }
        view.optional_str(
            "$INST_WALLPAPER_ID",
            "PICTURE",
            &perf["background"],
            "image",
        )?;
        view.optional_str("$INST_ICON_ID", "PICTURE", &perf["icon"], "image")?;
        view.controls(
            perf.get("controls")
                .context("Missing performance-view controls")?,
            "",
            None,
            0,
        )?;
        Ok(view)
    }

    // Intern real strings separately: JSON text need not be representable as a
    // KSP source literal (quotes, newlines and backslashes remain exact).
    fn quoted(&mut self, text: &str) -> Result<String> {
        ensure!(
            text.len() <= 65536,
            "Performance-view string exceeds 64 KiB"
        );
        let n = self.strings.len();
        self.strings.push(text.to_owned());
        Ok(format!("\"__nckp_string_{n}\""))
    }
    fn par(&mut self, id: &str, par: &str, value: impl std::fmt::Display) -> Result<()> {
        writeln!(
            self.source,
            "set_control_par({id},$CONTROL_PAR_{par},{value})"
        )?;
        Ok(())
    }
    fn par_str(&mut self, id: &str, par: &str, value: &str) -> Result<()> {
        let value = self.quoted(value)?;
        writeln!(
            self.source,
            "set_control_par_str({id},$CONTROL_PAR_{par},{value})"
        )?;
        Ok(())
    }
    fn optional_int(&mut self, id: &str, par: &str, v: &Value, key: &str) -> Result<()> {
        if v.get(key).is_some() {
            self.par(id, par, int(v, key)?)?;
        }
        Ok(())
    }
    fn optional_str(&mut self, id: &str, par: &str, v: &Value, key: &str) -> Result<()> {
        if v.get(key).is_some() {
            self.par_str(id, par, string(v, key)?)?;
        }
        Ok(())
    }
    fn font(&mut self, id: &str, par: &str, font: &Value) -> Result<()> {
        if font.is_null() {
            return Ok(());
        }
        let custom = string(font, "custom")?;
        if custom.is_empty() {
            self.par(id, par, int(font, "type")?)?;
        } else {
            let text = self.quoted(custom)?;
            self.par(id, par, format!("get_font_id({text})"))?;
        }
        Ok(())
    }
    fn controls(
        &mut self,
        rows: &Value,
        prefix: &str,
        parent: Option<&str>,
        depth: usize,
    ) -> Result<()> {
        ensure!(depth <= 32, "Performance-view panel nesting exceeds 32");
        let rows = rows
            .as_array()
            .context("Invalid performance-view control list")?;
        for row in rows {
            ensure!(self.names.len() < 16384, "Performance-view control limit");
            let index = int(row, "index")?;
            let v = &row["value"];
            let c = &v["common"];
            let name = string(c, "id")?;
            ensure!(
                !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
                "Invalid control identifier {name:?}"
            );
            let name = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}_{name}")
            };
            ensure!(
                name.len() <= 1024 && self.names.insert(name.clone()),
                "Duplicate or oversized control identifier {name:?}"
            );
            let (kind, sigil) = match index {
                0 => ("ui_panel", '$'),
                1 => ("ui_button", '$'),
                3 => ("ui_knob", '$'),
                4 => ("ui_label", '$'),
                5 => ("ui_level_meter", '$'),
                6 => ("ui_menu", '$'),
                7 => ("ui_slider", '$'),
                8 => ("ui_switch", '$'),
                9 => ("ui_table", '%'),
                10 => ("ui_text_edit", '@'),
                11 => ("ui_value_edit", '$'),
                _ => bail!("Unsupported NCKP control family {index} ({name})"),
            };
            let var = format!("{sigil}{name}");
            let id = format!("get_ui_id({var})");
            write!(self.source, "declare {kind} {var}")?;
            match index {
                3 | 7 | 11 => {
                    let min = int(&v["value"], "min")?;
                    let max = int(&v["value"], "max")?;
                    ensure!(min <= max, "Invalid control range ({name})");
                    let ratio = v
                        .get("ratio")
                        .map(|_| int(v, "ratio"))
                        .transpose()?
                        .unwrap_or(1);
                    ensure!(ratio > 0, "Invalid control display ratio ({name})");
                    if index == 7 {
                        write!(self.source, "({min},{max})")?;
                    } else {
                        write!(self.source, "({min},{max},{ratio})")?;
                    }
                }
                4 => write!(self.source, "(1,1)")?,
                9 => {
                    let total = int(&v["steps"], "total")?;
                    let max = int(v, "maxValue")?;
                    ensure!(
                        (1..=1_000_000).contains(&total) && max > 0,
                        "Invalid table size/range ({name})"
                    );
                    let range = if boolean(v, "bipolar")? { -max } else { max };
                    write!(self.source, "[{total}](1,1,{range})")?;
                }
                _ => {}
            }
            self.source.push('\n');
            for (par, val) in [
                ("POS_X", int(&c["position"], "x")?),
                ("POS_Y", int(&c["position"], "y")?),
                ("WIDTH", int(&c["size"], "width")?),
                ("HEIGHT", int(&c["size"], "height")?),
                ("Z_LAYER", int(c, "zLayer")?),
                ("HIDE", if boolean(c, "show")? { 0 } else { 16 }),
            ] {
                ensure!(
                    !matches!(par, "WIDTH" | "HEIGHT") || val >= 0,
                    "Negative control size ({name})"
                );
                self.par(&id, par, val)?;
            }
            if let Some(parent) = parent {
                self.par(&id, "PARENT_PANEL", format!("get_ui_id(${parent})"))?;
            }
            self.optional_str(&id, "HELP", c, "infoPaneText")?;
            self.optional_str(&id, "PICTURE", v, "image")?;
            self.optional_str(&id, "PICTURE", &v["background"], "image")?;
            self.optional_int(&id, "PICTURE_STATE", &v["background"], "frameIndex")?;
            self.optional_str(&id, "TEXT", &v["text"], "string")?;
            self.optional_int(&id, "TEXT_ALIGNMENT", &v["text"], "horizontalAlignment")?;
            self.optional_int(&id, "TEXTPOS_Y", &v["text"], "positionY")?;
            if let Some(shift) = v["text"].get("shiftOnPress") {
                self.par(
                    &id,
                    "DISABLE_TEXT_SHIFTING",
                    i32::from(!shift.as_bool().context("Invalid text shift flag")?),
                )?;
            }
            self.font(&id, "FONT_TYPE", &v["text"]["font"])?;
            for (key, par) in [
                ("off", "FONT_TYPE"),
                ("on", "FONT_TYPE_ON"),
                ("pressedOff", "FONT_TYPE_OFF_PRESSED"),
                ("pressedOn", "FONT_TYPE_ON_PRESSED"),
                ("hoverOff", "FONT_TYPE_OFF_HOVER"),
                ("hoverOn", "FONT_TYPE_ON_HOVER"),
            ] {
                self.font(&id, par, &v["text"]["fonts"][key])?;
            }
            if let Some(default) = v["value"].get("default") {
                let default = default
                    .as_i64()
                    .and_then(|n| i32::try_from(n).ok())
                    .context("Invalid control default")?;
                ensure!(
                    default >= int(&v["value"], "min")? && default <= int(&v["value"], "max")?,
                    "Default outside control range ({name})"
                );
                writeln!(self.source, "{var} := {default}")?;
                self.par(&id, "DEFAULT_VALUE", default)?;
            }
            if index == 10 {
                let value = self.quoted(string(&v["text"], "string")?)?;
                writeln!(self.source, "{var} := {value}")?;
            }
            if let Some(a) = v.get("automation") {
                self.par(
                    &id,
                    "ALLOW_AUTOMATION",
                    i32::from(!boolean(a, "disableUserAssign")?),
                )?;
                self.par(
                    &id,
                    "AUTOMATION_ID",
                    if boolean(&a["hostAutomation"], "on")? {
                        int(&a["hostAutomation"], "id")?
                    } else {
                        -1
                    },
                )?;
                self.optional_str(&id, "AUTOMATION_NAME", a, "name")?;
            }
            if index == 3 {
                self.optional_int(&id, "UNIT", v, "unit")?;
                self.optional_str(&id, "TEXT", v, "name")?;
            }
            if index == 11 {
                self.par(&id, "SHOW_ARROWS", i32::from(boolean(v, "showArrows")?))?;
            }
            if index == 7 {
                self.optional_int(&id, "MOUSE_BEHAVIOUR", &v["mouse"], "scale")?;
            }
            if index == 5 {
                self.optional_int(&id, "VERTICAL", v, "orientation")?;
                for (key, par) in [
                    ("active", "ON_COLOR"),
                    ("background", "BG_COLOR"),
                    ("inactive", "OFF_COLOR"),
                    ("overload", "OVERLOAD_COLOR"),
                    ("peak", "PEAK_COLOR"),
                ] {
                    self.optional_int(&id, par, &v["colors"], key)?;
                }
            }
            if index == 9 {
                let visible = int(&v["steps"], "visible")?;
                ensure!(
                    visible > 0 && visible <= int(&v["steps"], "total")?,
                    "Invalid visible table steps ({name})"
                );
                writeln!(self.source, "set_table_steps_shown({var},{visible})")?;
                self.optional_int(&id, "BAR_COLOR", &v["colors"], "bars")?;
                self.optional_int(&id, "ZERO_LINE_COLOR", &v["colors"], "zeroLine")?;
            }
            if index == 6 {
                let entries = v["entries"].as_array().context("Invalid menu entries")?;
                ensure!(entries.len() <= 4096, "Menu item limit");
                for (i, entry) in entries.iter().enumerate() {
                    let text = self.quoted(string(entry, "string")?)?;
                    let value = int(entry, "value")?;
                    writeln!(self.source, "add_menu_item({var},{text},{value})")?;
                    if !boolean(entry, "show")? {
                        writeln!(self.source, "set_menu_item_visibility({id},{i},0)")?;
                    }
                }
            }
            if index == 0 {
                self.controls(
                    v.get("controls").context("Missing panel controls")?,
                    &name,
                    Some(&name),
                    depth + 1,
                )?;
            }
        }
        Ok(())
    }
}
