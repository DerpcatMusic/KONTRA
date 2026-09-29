//! Static inventory of a script: callbacks, called functions, constants and control
//! kinds. Observed names are an inventory, not a claim that they are implemented.

use anyhow::{Result, ensure};
use std::collections::BTreeSet;

const NOT_CALLS: &[&str] = &["if", "while", "select", "case", "ui_control", "and", "or", "not", "xor", "mod"];
const BARE_CALLS: &[&str] = &["make_perfview", "exit", "ignore_controller", "reset_ksp_timer", "expose_controls"];

pub fn requirements(source: &str) -> Result<serde_json::Value> {
    ensure!(source.len() <= 128 << 20, "KSP static inventory exceeds 128 MiB");
    let [mut callbacks, mut calls, mut constants, mut controls]: [BTreeSet<&str>; 4] = Default::default();
    let bytes = source.as_bytes();
    let mut previous = "";
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'{' | b'"' => {
                let close = if c == b'{' { b'}' } else { b'"' };
                let end = bytes[i + 1..].iter().position(|&b| b == close);
                ensure!(end.is_some(), "Unterminated KSP comment/string");
                i += end.unwrap_or(0) + 2;
                previous = "";
                continue;
            }
            _ if c.is_ascii_alphanumeric() || b"_$%@!~?".contains(&c) || c >= 0x80 => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || b"_$%@!~?".contains(&bytes[i]) || bytes[i] >= 0x80) {
                    i += 1;
                }
                let token = &source[start..i];
                match previous {
                    "on" => {
                        callbacks.insert(token);
                    }
                    "declare" if token.starts_with("ui_") => {
                        controls.insert(token);
                    }
                    _ => {}
                }
                if token.starts_with('$') && token.as_bytes().get(1).is_some_and(u8::is_ascii_uppercase) {
                    constants.insert(token);
                }
                if BARE_CALLS.contains(&token) {
                    calls.insert(token);
                }
                previous = token;
                continue;
            }
            b'(' if previous.starts_with(|c: char| c.is_ascii_alphabetic()) && !NOT_CALLS.contains(&previous) => {
                calls.insert(previous);
            }
            _ => {}
        }
        if c != b' ' && c != b'\t' && c != b'\r' {
            previous = "";
        }
        i += 1;
    }
    Ok(serde_json::json!({
        "callbacks": callbacks,
        "calls": calls,
        "constants": constants,
        "controls": controls,
        "runtime_callbacks_implemented": true,
    }))
}
