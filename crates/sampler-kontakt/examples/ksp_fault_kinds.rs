//! Classify selected script faults in RAM; never export diagnostic text or source.
use std::{collections::BTreeMap, path::Path};

fn reason(message: &str) -> (&'static str, Option<&'static str>) {
    let (kind, name) = if let Some(name) = message.strip_prefix("unknown command ") {
        ("unknown-command", name)
    } else if let Some(name) = message.strip_prefix("unknown function ") {
        ("unknown-function", name)
    } else {
        return ("other-semantic-error", None);
    };
    // Only public catalog names and the async command identified by the audit escape RAM.
    let builtin = std::iter::once("subscribe_async")
        .chain(include_str!("../../../docs/architecture-v2/KSP_SYMBOLS.md").lines())
        .find(|public| *public == name);
    (kind, builtin)
}

fn arguments<'a>(source: &'a str, offset: usize, name: &str) -> Option<Vec<&'a str>> {
    let tail = source.get(offset..)?.strip_prefix(name)?.trim_start();
    let tail = tail.strip_prefix('(')?;
    let (mut depth, mut quoted, mut escaped, mut start) = (0, false, false, 0);
    let mut args = Vec::new();
    for (index, c) in tail.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                quoted = false;
            }
            continue;
        }
        match c {
            '"' => {
                quoted = true;
            }
            '(' | '[' => {
                depth += 1;
            }
            ')' if depth == 0 => {
                let arg = tail[start..index].trim();
                if !arg.is_empty() || !args.is_empty() {
                    args.push(arg);
                }
                return Some(args);
            }
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                args.push(tail[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    None
}

fn arity(source: &str, offset: usize, name: &str) -> Option<usize> {
    arguments(source, offset, name).map(|args| args.len())
}

fn argument_kind(arg: &str) -> &'static str {
    match arg.chars().next() {
        Some('"' | '@') => "text",
        Some('$') => "integer-variable",
        Some('%') => "integer-array",
        Some('~') => "real-variable",
        Some('?') => "real-array",
        Some('!') => "text-array",
        _ if arg.parse::<i32>().is_ok() => "integer-literal",
        _ => "expression",
    }
}

fn argument_facts(arg: &str) -> String {
    let public = include_str!("../../../docs/architecture-v2/KSP_SYMBOLS.md")
        .lines()
        .find(|name| *name == arg)
        .map_or("null".into(), |name| format!("\"{name}\""));
    let integer = arg.parse::<i32>().map_or("null".into(), |n| n.to_string());
    format!(
        "{{\"kind\":\"{}\",\"public_symbol\":{public},\"integer\":{integer}}}",
        argument_kind(arg)
    )
}

fn declaration_facts(source: &str, arg: &str) -> String {
    if !arg.starts_with('$') || argument_kind(arg) != "integer-variable" {
        return "null".into();
    }
    for (index, line) in source.lines().enumerate() {
        let Some(declaration) = line.trim().strip_prefix("declare ") else {
            continue;
        };
        let (head, initial) = declaration
            .split_once(":=")
            .map_or((declaration, None), |(a, b)| (a, Some(b.trim())));
        if !head.split_whitespace().any(|token| token == arg) {
            continue;
        }
        let kind = if head.split_whitespace().any(|token| token == "const") {
            "integer-constant"
        } else {
            "integer-scalar"
        };
        let integer = initial
            .and_then(|value| value.parse::<i32>().ok())
            .map_or("null".into(), |value| value.to_string());
        return format!(
            "{{\"line\":{},\"kind\":\"{kind}\",\"initial_integer\":{integer}}}",
            index + 1
        );
    }
    "null".into()
}

fn main() -> anyhow::Result<()> {
    assert_eq!(
        reason("unknown command private_authored_name"),
        ("unknown-command", None)
    );
    assert_eq!(
        reason("unknown command subscribe_async"),
        ("unknown-command", Some("subscribe_async"))
    );
    assert_eq!(arity("subscribe_async()", 0, "subscribe_async"), Some(0));
    assert_eq!(
        arity("mf_get_first(f(1,2), \"a,b\")", 0, "mf_get_first"),
        Some(2)
    );
    assert_eq!(argument_kind("\"private text\""), "text");
    assert_eq!(argument_kind("$private_name"), "integer-variable");
    assert_eq!(argument_kind("-1"), "integer-literal");
    assert_eq!(
        argument_facts("$private_name"),
        "{\"kind\":\"integer-variable\",\"public_symbol\":null,\"integer\":null}"
    );
    assert_eq!(
        declaration_facts("declare $private := 4", "$private"),
        "{\"line\":1,\"kind\":\"integer-scalar\",\"initial_integer\":4}"
    );
    let public = std::env::args()
        .nth(2)
        .map(std::fs::read_to_string)
        .transpose()?
        .unwrap_or_default();
    let manifest = std::fs::read_to_string(
        std::env::args()
            .nth(1)
            .ok_or_else(|| anyhow::anyhow!("missing numeric manifest"))?,
    )?;
    let mut selected: BTreeMap<(&str, usize), Vec<(&str, u8)>> = BTreeMap::new();
    for line in manifest.lines() {
        let row: Vec<_> = line.split('\t').collect();
        anyhow::ensure!(
            row.len() == 4 && row[0].bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid manifest row"
        );
        selected
            .entry((row[1], row[2].parse()?))
            .or_default()
            .push((row[0], row[3].parse()?));
    }
    for ((path, program), slots) in selected {
        let loaded = sampler_kontakt::read_program(Path::new(path), program)
            .map_err(|_| anyhow::anyhow!("numeric probe decode failed"))?;
        for (item, slot) in slots {
            let behavior = loaded
                .instrument
                .behaviors
                .iter()
                .enumerate()
                .find(|(index, b)| b.slot.unwrap_or(*index as u8) == slot)
                .map(|(_, b)| b)
                .ok_or_else(|| anyhow::anyhow!("numeric probe slot missing"))?;
            let environment = sampler_ksp::Environment {
                slot,
                groups: loaded
                    .instrument
                    .groups
                    .iter()
                    .map(|g| g.name.clone())
                    .collect(),
                ..Default::default()
            };
            sampler_ksp::scan::begin();
            let result = sampler_ksp::initialize(
                &behavior.source,
                sampler_ksp::Limits::LIBRARY,
                &environment,
            )
            .and_then(|initialized| {
                sampler_ksp::compile_initialized(
                    &behavior.source,
                    48000,
                    sampler_ksp::Limits::LIBRARY,
                    &[],
                    initialized,
                )
            });
            let records = sampler_ksp::scan::take();
            match result {
                Ok(_) => println!(
                    "{{\"item\":\"{item}\",\"program\":{program},\"slot\":{slot},\"reason\":\"admitted\"}}"
                ),
                Err(error) => {
                    let phase = records
                        .last()
                        .and_then(|r| r.error.as_ref())
                        .map_or("unknown", |e| e.phase);
                    let (kind, builtin) = reason(&error.message);
                    let raw = error
                        .message
                        .strip_prefix("unknown command ")
                        .or_else(|| error.message.strip_prefix("unknown function "));
                    let args =
                        raw.and_then(|name| self::arguments(&behavior.source, error.offset, name));
                    let arguments = args.as_ref().map(Vec::len);
                    let kinds = args
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .map(|arg| format!("\"{}\"", argument_kind(arg)))
                        .collect::<Vec<_>>()
                        .join(",");
                    let facts = args
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .map(|arg| argument_facts(arg))
                        .collect::<Vec<_>>()
                        .join(",");
                    let declarations = args
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .map(|arg| declaration_facts(&behavior.source, arg))
                        .collect::<Vec<_>>()
                        .join(",");
                    let public_name = raw.filter(|name| public.lines().any(|line| line == *name));
                    let family = match raw {
                        Some(name) if name.starts_with("mf_") => "midi-object",
                        Some(name) if name.starts_with("subscribe_") => "subscription",
                        _ => "other-command",
                    };
                    let declared = raw.is_some_and(|name| {
                        behavior.source.lines().any(|line| {
                            line.trim()
                                .strip_prefix("function ")
                                .is_some_and(|tail| tail.split_whitespace().next() == Some(name))
                        })
                    });
                    let builtin = builtin
                        .or(public_name)
                        .map_or("null".into(), |name| format!("\"{name}\""));
                    let arguments = arguments.map_or("null".into(), |n| n.to_string());
                    println!(
                        "{{\"item\":\"{item}\",\"program\":{program},\"slot\":{slot},\"phase\":\"{phase}\",\"reason\":\"{kind}\",\"builtin\":{builtin},\"arity\":{arguments},\"argument_kinds\":[{kinds}],\"argument_facts\":[{facts}],\"argument_declarations\":[{declarations}],\"name_family\":\"{family}\",\"function_declared\":{declared},\"offset\":{},\"line\":{}}}",
                        error.offset, error.line
                    );
                }
            }
        }
    }
    Ok(())
}
