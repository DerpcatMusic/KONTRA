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

/// A note callback can enable zones absent at load time. Still audition a safe key.
pub fn fallback_note(invalid: &std::collections::BTreeSet<u8>) -> Option<(u8, u8)> {
    (0..=127u8)
        .filter(|key| !invalid.contains(key))
        .min_by_key(|key| key.abs_diff(60))
        .map(|key| (key, 64))
}

pub fn planned_keyswitch(program: u32) -> Option<Option<u8>> {
    let path = std::env::var_os("KONTRA_SCAN_NOTE_PLAN")?;
    let value: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    let key = value["programs"][program.to_string()].get("keyswitch")?;
    Some(if key.is_null() { None } else { Some(u8::try_from(key.as_u64()?).ok()?.min(127)) })
}

/// Explicit held input from the native-intent audition plan; never guess a neighbor.
pub fn planned_held_key(program: u32) -> Result<Option<u8>, &'static str> {
    let Some(path) = std::env::var_os("KONTRA_SCAN_NOTE_PLAN") else { return Ok(None) };
    let bytes = std::fs::read(path).map_err(|_| "invalid-note-plan")?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid-note-plan")?;
    held_key(&value["programs"][program.to_string()])
}
fn held_key(program: &Value) -> Result<Option<u8>, &'static str> {
    match program.get("held_key") {
        None | Some(Value::Null) => Ok(None),
        Some(key) => key.as_u64().filter(|key| *key < 128).map(|key| Some(key as u8)).ok_or("invalid-held-key"),
    }
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
    fn held_note_plan_rejects_malformed_midi_instead_of_clamping() {
        use serde_json::json;
        assert_eq!(super::held_key(&json!({})), Ok(None));
        assert_eq!(super::held_key(&json!({"held_key":null})), Ok(None));
        assert_eq!(super::held_key(&json!({"held_key":61})), Ok(Some(61)));
        for key in [json!(-1),json!(128),json!(true),json!("61"),json!(61.5)] {
            assert_eq!(super::held_key(&json!({"held_key":key})), Err("invalid-held-key"));
        }
    }

    #[test]
    fn scanner_metrics_skip_source_text_and_detect_uniform_render() {
        assert_eq!(super::fallback_note(&Default::default()), Some((60,64)));
        assert!(!super::nonzero([0.,-0.,f32::NAN,f32::INFINITY]));
        assert!(super::nonzero([0.,1e-12]));
        assert_eq!(super::fallback_note(&[60].into()), Some((59,64)));
        assert_eq!(super::fallback_note(&(0..=127).collect()), None);
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
        let mut table = u32::MAX.to_le_bytes().to_vec();
        table.extend([0;3]);table.extend(0u32.to_le_bytes());
        table.extend(u32::MAX.to_le_bytes());table.extend(u32::MAX.to_le_bytes());
        assert_eq!(crate::ui::scan::strict_table(&table,0x50).0,"absent");
        table.extend(1u32.to_le_bytes());table.extend(2u32.to_le_bytes());table.extend(b"!x");
        let parsed=crate::ui::scan::strict_table(&table,0x50);
        assert_eq!(parsed.0,"decoded");assert_eq!(parsed.1.get("!"),Some(&1));
        table.pop();assert_eq!(crate::ui::scan::strict_table(&table,0x50).0,"malformed");
        sampler_ksp::scan::begin();
        // Full-suite peers also compile scripts into the process-wide capture.
        sampler_ksp::scan::attempt("scanner-phase-test");
        let take = || sampler_ksp::scan::take().into_iter()
            .filter(|o| o.attempt == "scanner-phase-test").collect::<Vec<_>>();
        sampler_ksp::compile(
            "on init\ndeclare $x := 1\nend on",
            48000,
            sampler_ksp::Limits::LIBRARY,
            &[],
        )
        .unwrap();
        let ok = take();
        assert_eq!(ok.len(), 1);
        assert!(ok.iter().any(|o| o.compile_ok && o.init_ok == Some(true)));
        assert!(
            sampler_ksp::compile("this is not KSP", 48000, sampler_ksp::Limits::LIBRARY, &[])
                .is_err()
        );
        let failed = take();
        assert_eq!(failed.len(), 1);
        assert!(failed.iter().any(|o| !o.compile_ok && o.init_ok.is_none()));
        sampler_ksp::compile(
            "on init\ndeclare $x\nend on\non persistence_changed\nwhile(1)\nend while\nend on",
            48000,
            sampler_ksp::Limits::LIBRARY,
            &[],
        )
        .unwrap();
        let callback = take();
        assert_eq!(callback.len(), 1);
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
        sampler_ksp::scan::attempt("standalone");
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

// Onset uses exact finite nonzero output; audible audition retains its separate 1e-5 threshold.
pub fn nonzero(samples: impl IntoIterator<Item=f32>) -> bool {samples.into_iter().any(|x|x.is_finite() && x!=0.)}
