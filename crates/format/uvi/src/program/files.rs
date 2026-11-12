//! Files a program's scripts read after they load (`loadData`, `loadState`):
//! the bank's script presets (a ScriptProcessor's saved widget values and
//! state, VWinds' `.VHFPreset`) and JSON data (Augmented Orchestra's `.arp`),
//! carried as Lua modules so the script host answers them from memory, never
//! from the disk on the audio thread.

use kontra_format::Notes;
use kontra_ir as ir;

use super::load::lua_literal;

/// The largest file carried; a script's preset is a few kB.
pub(crate) const FILE_LIMIT: usize = 256 << 10;
/// The most bytes carried per program.
pub(crate) const TOTAL_LIMIT: usize = 4 << 20;
/// Module names of carried files start with this; the rest is the file's
/// bank path, lower case.
pub(crate) const FILE: &str = "__file/";
/// The module naming a script's folder in the bank, for relative paths.
pub(crate) const DIR: &str = "__dir.lua";

/// Members a script never reads as data: code, programs, audio, pictures.
const MEDIA: [&str; 15] = [
    "lua", "uvip", "wav", "aif", "aiff", "flac", "ogg", "ncw", "png", "jpg", "jpeg", "svg", "ttf",
    "otf", "asd",
];

/// Whether the member at `path` may be data a script reads.
pub(crate) fn may_be_data(path: &str) -> bool {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    ext.is_some_and(|e| !MEDIA.contains(&e.as_str()))
}

/// The bank's data files as script modules (`__file/<path>`), within
/// [`TOTAL_LIMIT`]; what is left out (here, or with no bytes by the bank's
/// budget) is noted.
pub(super) fn modules(
    files: &[(String, Option<Vec<u8>>)],
    notes: &mut Notes,
) -> Vec<(String, Vec<u8>)> {
    let mut total = 0;
    let mut out = Vec::new();
    for (path, bytes) in files {
        let Some(bytes) = bytes else {
            notes.add(ir::Impact::Audible, "script preset file not carried", path);
            continue;
        };
        let Some(text) = literal(bytes) else { continue };
        total += text.len();
        if total > TOTAL_LIMIT {
            notes.add(ir::Impact::Audible, "script preset file not carried", path);
            continue;
        }
        out.push((format!("{FILE}{}", path.to_lowercase()), text.into_bytes()));
    }
    out
}

/// The module naming a script's folder, `dir`, as its bank path.
pub(super) fn dir_module(dir: &str) -> (String, Vec<u8>) {
    (DIR.into(), format!("return {}", quote(dir)).into_bytes())
}

/// `bytes` as a Lua module: `{values = {...}, state = ...}` for a saved
/// ScriptProcessor, `{data = ...}` for JSON; `None` for anything else.
pub(crate) fn literal(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let text = text.trim_start_matches('\u{feff}').trim_start();
    match text.as_bytes().first()? {
        b'<' => processor(text),
        // `{` or `[`: JSON (escaped, so the debt scan's bracket count holds).
        b'\x7b' | b'\x5b' => Some(format!("return {{data={}}}", body(&lua_literal(text)))),
        _ => None,
    }
}

/// A saved ScriptProcessor: its widget values by name, and its state.
fn processor(text: &str) -> Option<String> {
    let doc = roxmltree::Document::parse(text).ok()?;
    let node = doc
        .descendants()
        .find(|n| n.has_tag_name("ScriptProcessor"))?;
    let data = node.children().find(|c| c.has_tag_name("ScriptData"));
    let mut out = String::from("return {values={");
    for a in node
        .attributes()
        .chain(data.into_iter().flat_map(|d| d.attributes()))
    {
        if !matches!(a.name(), "Name" | "Bypass" | "API_version") {
            out.push_str(&format!("[{}]={},", quote(a.name()), quote(a.value())));
        }
    }
    out.push('}');
    let state = node.children().find(|c| c.has_tag_name("state"));
    if let Some(state) = state
        .and_then(|s| s.text())
        .filter(|s| !s.trim().is_empty())
    {
        out.push_str(&format!(",state={}", body(&lua_literal(state))));
    }
    out.push('}');
    Some(out)
}

/// A module's value without its `return`.
fn body(module: &str) -> &str {
    module.strip_prefix("return ").unwrap_or(module)
}

/// `s` as a Lua string literal.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => out.push_str(&format!("\\{}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_processor_becomes_its_values_and_state() {
        let xml = r#"<UVI4><ScriptProcessor controllerType="1" Name="x" naturalAir="1.5"><ScriptData t="0,5 1,0"/><state><![CDATA[{"presetText":"Studio \"1\"*"}]]></state></ScriptProcessor></UVI4>"#;
        assert_eq!(
            literal(xml.as_bytes()).unwrap(),
            r#"return {values={["controllerType"]="1",["naturalAir"]="1.5",["t"]="0,5 1,0",},state={["presetText"]="Studio \"1\"*"}}"#
        );
    }

    #[test]
    fn json_becomes_data_and_other_files_nothing() {
        assert_eq!(
            literal(br#"{"a":[1,2],"b":null}"#).unwrap(),
            r#"return {data={["a"]={1,2},["b"]=nil}}"#
        );
        assert!(literal(b"plain text").is_none());
        assert!(literal(&[0xff, 0xfe]).is_none());
        assert!(may_be_data("JSON/Arpeggiator/Frames.arp"));
        assert!(!may_be_data("Samples/a.WAV"));
    }
}
