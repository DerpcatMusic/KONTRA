use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
pub mod metadata;

pub fn pixels(rgba: &[u8]) -> Value {
    let mut bins = BTreeMap::<u32, usize>::new();
    let mut white = 0;
    for c in rgba.chunks_exact(4) {
        *bins
            .entry(u32::from_le_bytes(c.try_into().unwrap()))
            .or_default() += 1;
        white += usize::from(c[..3].iter().all(|x| *x >= 240));
    }
    let n = rgba.len() / 4;
    let dominant = bins.values().copied().max().unwrap_or(0);
    json!({"blake3":blake3::hash(rgba).to_hex().to_string(), "pixels":n, "colors":bins.len(),
        "dominant_fraction":dominant as f64 / n.max(1) as f64, "white_fraction":white as f64/n.max(1) as f64,
        "uniform":n == 0 || dominant as f64/n as f64 > 0.999})
}

/// Count spec identifiers without storing source. Comments and quoted text are skipped.
pub fn symbols(source: &str) -> BTreeMap<String, usize> {
    let s = source.as_bytes();
    let mut counts = BTreeMap::new();
    let mut i = 0;
    while i < s.len() {
        if matches!(s[i], b'{' | b'"' | b'\'') {
            let end = if s[i] == b'{' { b'}' } else { s[i] };
            i += 1;
            while i < s.len() && s[i] != end {
                if s[i] == b'\\' && end != b'}' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        if s[i].is_ascii_alphabetic() || matches!(s[i], b'$' | b'_') {
            let start = i;
            i += 1;
            while i < s.len() && (s[i].is_ascii_alphanumeric() || s[i] == b'_') {
                i += 1;
            }
            let raw = &source[start..i];
            let token = raw.strip_prefix('_').unwrap_or(raw);
            if include_str!("ui-symbols.txt").lines().any(|n| n == token) {
                *counts.entry(token.into()).or_default() += 1;
            }
        } else {
            i += 1;
        }
    }
    counts
}

pub fn checkpoint(out: &Path, value: &Value) {
    let _ = std::fs::write(
        out.join("progress.json"),
        serde_json::to_vec(value).unwrap(),
    );
}

pub fn error(stage: &str, e: impl std::fmt::Display) -> Value {
    // Error strings may quote decrypted source/values. Keep only a stage and a digest.
    json!({"stage":stage,"error_hash":blake3::hash(e.to_string().as_bytes()).to_hex().to_string()})
}

/// Whitelisted diagnostic classes only: authored identifiers, excerpts and paths never escape.
pub fn message(raw: &str) -> String {
    let lower = raw.to_lowercase();
    let class = [
        "time budget exceeded",
        "source byte budget exceeded",
        "instruction budget exceeded",
        "control binding budget exceeded",
        "array cell budget exceeded",
        "variable budget exceeded",
        "attempt to call a nil value",
        "attempt to index nil",
        "attempt to index a nil value",
        "attempt to perform arithmetic",
        "syntax error",
        "unknown variable",
        "unknown function",
        "division by zero",
        "not found",
        "out of bounds",
        "memory limit",
        "stack overflow",
        "budgetexceeded",
        "the tree exceeds its node or depth limit",
        "layout failed",
        "paint failed",
    ]
    .into_iter()
    .find(|c| lower.contains(c))
    .unwrap_or("script/load diagnostic (private details omitted)");
    format!(
        "{class}; hash {}",
        &blake3::hash(raw.as_bytes()).to_hex()[..16]
    )
}

pub fn note(program: u32) -> Option<(u8, u8)> {
    let path = std::env::var_os("KONTRA_SCAN_NOTE_PLAN")?;
    let value: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let p = &value["programs"][program.to_string()];
    Some((
        u8::try_from(p["key"].as_u64()?).ok()?.min(127),
        u8::try_from(p["velocity"].as_u64()?).ok()?.clamp(1, 127),
    ))
}

pub fn background(rgba: &[u8], colour: Option<[u8; 4]>) -> Value {
    let n = rgba.len() / 4;
    let target = colour.unwrap_or_else(|| {
        let mut bins = BTreeMap::<[u8; 4], usize>::new();
        for c in rgba.chunks_exact(4) {
            *bins.entry(c.try_into().unwrap()).or_default() += 1;
        }
        bins.into_iter()
            .max_by_key(|(_, n)| *n)
            .map_or([0; 4], |(c, _)| c)
    });
    let hits = rgba
        .chunks_exact(4)
        .filter(|c| c.iter().zip(target).all(|(a, b)| a.abs_diff(b) <= 1))
        .count();
    json!({"background_rgba":colour,"observed_background_rgba":target,"plain_background_fraction":hits as f64/n.max(1) as f64,
        "background_method":if colour.is_some(){"declared-color-pixel-match"}else{"modal-pixel-inferred"}})
}

pub fn budget(raw: &str) -> bool {
    let s = raw.to_lowercase();
    s.contains("budgetexceeded") || s.contains("tree exceeds its node or depth limit")
}

#[cfg(test)]
mod tests {
    #[test]
    fn scanner_metrics_skip_source_text_and_detect_uniform_render() {
        let c = super::symbols(
            "{ui_knob $CONTROL_PAR_HIDE} \"ui_slider\" declare ui_knob $k\nset_control_par($k,$CONTROL_PAR_HIDE,0)",
        );
        assert_eq!(c.get("ui_knob"), Some(&1));
        assert!(!c.contains_key("ui_slider"));
        let aliases = super::symbols(
            "_set_text($x, \"private\") make_instr_persistent($x) set_snapshot_type(1)",
        );
        assert_eq!(aliases.get("set_text"), Some(&1));
        assert_eq!(aliases.get("make_instr_persistent"), Some(&1));
        assert_eq!(aliases.get("set_snapshot_type"), Some(&1));
        assert_eq!(c.get("$CONTROL_PAR_HIDE"), Some(&1));
        assert_eq!(super::pixels(&[255; 16])["uniform"], true);
        assert!(
            !super::message("onInit: error 'private_library_source'")
                .contains("private_library_source")
        );
        assert!(super::budget("Layout(BudgetExceeded)"));
        assert_eq!(
            super::pixels(&[0, 0, 0, 255, 255, 255, 255, 255])["uniform"],
            false
        );
    }

    #[test]
    fn scanner_observes_compiler_and_lua_phases() {
        sampler_ksp::scan::begin();
        sampler_ksp::compile(
            "on init\ndeclare $x := 1\nend on",
            48000,
            sampler_ksp::Limits::LIBRARY,
            &[],
        )
        .unwrap();
        let ok = sampler_ksp::scan::take();
        assert!(ok.iter().any(|o| o.compile_ok && o.init_ok == Some(true)));
        assert!(
            sampler_ksp::compile("this is not KSP", 48000, sampler_ksp::Limits::LIBRARY, &[])
                .is_err()
        );
        let failed = sampler_ksp::scan::take();
        assert!(failed.iter().any(|o| !o.compile_ok && o.init_ok.is_none()));
        sampler_ksp::compile(
            "on init\ndeclare $x\nend on\non persistence_changed\nwhile(1)\nend while\nend on",
            48000,
            sampler_ksp::Limits::LIBRARY,
            &[],
        )
        .unwrap();
        let callback = sampler_ksp::scan::take();
        assert_eq!(callback[0].init.completion, "completed");
        assert_eq!(callback[0].persistence_changed.completion, "failed");
        assert_eq!(
            callback[0]
                .persistence_changed
                .fault
                .as_ref()
                .unwrap()
                .category,
            "fuel-budget"
        );
        let xml = "<UVI4><Program><ScriptProcessor><script><![CDATA[function onInit() error('private init') end function onNote(e) error('private runtime') end]]></script></ScriptProcessor></Program></UVI4>";
        let mut host = sampler_uvi::script::ScriptHost::new(xml, (), Default::default()).unwrap();
        assert_eq!(host.scan_faults().init_count, 1);
        host.note_on(1, 60, 64, 0);
        let faults = host.scan_faults();
        assert_eq!(faults.runtime_count, 1);
        assert!(faults.runtime_first.unwrap().contains("private runtime"));
        let xml = "<UVI4><Program><ScriptProcessor><script><![CDATA[function onInit() setKeyColour(60,'#00FFFFFF') setKeyColour(40,'#00000000') end]]></script></ScriptProcessor></Program></UVI4>";
        let host = sampler_uvi::script::ScriptHost::new(xml, (), Default::default()).unwrap();
        assert_eq!(host.scan_faults().native_valid_keys, vec![60]);
        assert_eq!(host.scan_faults().native_invalid_keys, vec![40]);
    }
}
