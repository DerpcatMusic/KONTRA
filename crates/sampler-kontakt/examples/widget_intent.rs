//! One-library source-site probe: emits identifiers and operation classes only.
//! Script bytes, literals and saved values never leave memory.
/// Preserve offsets while excluding comment/string contents from identifiers.
fn code(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let (mut depth, mut quoted, mut escaped) = (0u32, false, false);
    for c in source.chars() {
        if depth > 0 {
            if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
            }
        } else if quoted {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                quoted = false;
            }
        } else if c == '{' {
            depth = 1;
        } else if c == '"' {
            quoted = true;
        } else {
            out.push(c);
            continue;
        }
        if c == '\n' {
            out.push(c);
        } else {
            out.extend(std::iter::repeat_n(' ', c.len_utf8()));
        }
    }
    out
}

fn main() {
    let path = std::env::args_os().nth(1).expect("instrument path");
    let source = match sampler_kontakt::read(std::path::Path::new(&path)) {
        Ok(loaded) => loaded.instrument,
        Err(_) => {
            eprintln!("instrument read failed");
            std::process::exit(1)
        }
    };
    let aliases = [
        "WTSelectAlias",
        "WTShaperAlias",
        "SPLSelectAlias",
        "Shp__SelectAlias",
    ];
    let operations = [
        "set_control_par",
        "set_control_par_arr",
        "hide_part",
        "get_ui_id",
        "move_control",
        "move_control_px",
        "set_knob_label",
        "set_control_par_str",
        "set_control_par_real",
    ];
    for (slot, behavior) in source.behaviors.iter().enumerate() {
        let mut owner = String::new();
        let mut records = Vec::new();
        let mut offset = 0;
        for (line, text) in code(&behavior.source).lines().enumerate() {
            let trimmed = text.trim();
            if trimmed.starts_with("function ") || trimmed.starts_with("on ") {
                owner = trimmed
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .filter(|s| !s.is_empty())
                    .take(if trimmed.starts_with("on ui_control") {
                        3
                    } else {
                        2
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
            }
            let without_literals = text.split('"').step_by(2).collect::<Vec<_>>().join(" ");
            let identifiers = without_literals
                .split(|c: char| {
                    !(c.is_ascii_alphanumeric()
                        || c == '_'
                        || c == '$'
                        || c == '%'
                        || c == '@'
                        || c == '!')
                })
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>();
            records.push((line + 1, offset, owner.clone(), identifiers.clone()));
            for alias in aliases {
                if !text.contains(alias) {
                    continue;
                }
                let tokens = text
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
                    .collect::<Vec<_>>();
                let ops = operations
                    .iter()
                    .filter(|op| tokens.contains(op))
                    .copied()
                    .collect::<Vec<_>>();
                let hide = tokens
                    .iter()
                    .filter(|s| s.starts_with("$HIDE_") || **s == "$CONTROL_PAR_HIDE")
                    .copied()
                    .collect::<Vec<_>>();
                let assignment = text.find(":=");
                let role = match assignment {
                    Some(at) if text[..at].contains(alias) => "assignment_target",
                    Some(_) => "assignment_input",
                    None => "reference",
                };
                let controls = identifiers
                    .iter()
                    .filter(|s| s.starts_with('$') && s.contains("__"))
                    .collect::<Vec<_>>();
                println!(
                    "ALIAS_SITE slot={slot} line={} offset={offset} alias={alias} owner={owner} role={role} operations={ops:?} hide_symbols={hide:?} controls={controls:?}",
                    line + 1
                );
            }
            offset += text.len() + 1;
        }
        for parameter in [
            "$CONTROL_PAR_HIDE",
            "$CONTROL_PAR_Z_LAYER",
            "$CONTROL_PAR_POS_X",
            "$CONTROL_PAR_POS_Y",
        ] {
            let sites = records
                .iter()
                .filter(|(_, _, _, tokens)| tokens.iter().any(|s| s == parameter))
                .count();
            println!("ALIAS_GLOBAL_PARAMETER slot={slot} parameter={parameter} sites={sites}");
        }
        for (line, offset, owner, tokens) in &records {
            let parameters = tokens
                .iter()
                .filter(|s| {
                    matches!(
                        s.as_str(),
                        "$CONTROL_PAR_HIDE"
                            | "$CONTROL_PAR_POS_X"
                            | "$CONTROL_PAR_POS_Y"
                            | "$CONTROL_PAR_Z_LAYER"
                    )
                })
                .collect::<Vec<_>>();
            if parameters.is_empty() {
                continue;
            }
            let controls = tokens
                .iter()
                .filter(|s| s.starts_with('$') && s.contains("__"))
                .collect::<Vec<_>>();
            let hides = tokens
                .iter()
                .filter(|s| s.starts_with("$HIDE_"))
                .collect::<Vec<_>>();
            let op = tokens
                .iter()
                .filter(|s| operations.contains(&s.as_str()))
                .collect::<Vec<_>>();
            println!(
                "APPEARANCE_SITE slot={slot} line={line} offset={offset} owner={owner} parameters={parameters:?} operations={op:?} controls={controls:?} hide_symbols={hides:?}"
            );
        }
        let mut owners = std::collections::BTreeSet::new();
        for (_, _, owner, tokens) in &records {
            if owner != "on init" && tokens.iter().any(|t| aliases.iter().any(|a| t.contains(a))) {
                owners.insert(owner.clone());
            }
        }
        // Metadata only: call edges and native parameter symbols in the owner,
        // then one helper level. Never print expressions or literal arguments.
        for _ in 0..2 {
            let current = owners.clone();
            for (line, offset, owner, tokens) in &records {
                if !current.contains(owner) {
                    continue;
                }
                if let Some(at) = tokens.iter().position(|s| s == "call") {
                    if let Some(callee) = tokens.get(at + 1) {
                        owners.insert(format!("function {callee}"));
                        println!(
                            "ALIAS_CALL slot={slot} line={line} offset={offset} owner={owner} callee={callee}"
                        );
                    }
                }
                if tokens.iter().any(|s| operations.contains(&s.as_str())) {
                    let ops = tokens
                        .iter()
                        .filter(|s| operations.contains(&s.as_str()))
                        .collect::<Vec<_>>();
                    let params = tokens
                        .iter()
                        .filter(|s| s.starts_with("$CONTROL_PAR_") || s.starts_with("$HIDE_"))
                        .collect::<Vec<_>>();
                    let controls = tokens
                        .iter()
                        .filter(|s| s.starts_with('$') && s.contains("__"))
                        .collect::<Vec<_>>();
                    println!(
                        "ALIAS_OPERATION slot={slot} line={line} offset={offset} owner={owner} operations={ops:?} parameters={params:?} controls={controls:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn source_literals_and_nested_comments_never_become_identifier_metadata() {
    let source = "{outer {nested} $secret}\ncall valid \"$literal\"\n{nonascii é} $safe";
    let masked = code(source);
    assert_eq!(source.len(), masked.len());
    assert_eq!(source.lines().count(), masked.lines().count());
    assert!(masked.contains("call valid"));
    assert!(masked.contains("$safe"));
    for private in ["secret", "literal", "nested", "é"] {
        assert!(!masked.contains(private));
    }
}
