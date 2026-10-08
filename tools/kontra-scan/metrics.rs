use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

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
            let token = &source[start..i];
            if token.starts_with("ui_")
                || token.starts_with("$CONTROL_PAR_")
                || token.starts_with("set_ui_")
                || token.starts_with("set_control_par")
                || token.starts_with("get_control_par")
                || token.starts_with("set_key_")
                || matches!(
                    token,
                    "make_perfview"
                        | "set_script_title"
                        | "hide_part"
                        | "get_ui_id"
                        | "move_control"
                        | "move_control_px"
                        | "set_skin_offset"
                        | "load_performance_view"
                        | "load_komplete_ui"
                        | "make_persistent"
                        | "read_persistent_var"
                        | "set_text"
                        | "add_text_line"
                        | "add_menu_item"
                        | "set_menu_item_str"
                        | "set_menu_item_visibility"
                        | "set_table_steps_shown"
                        | "attach_zone"
                        | "set_bounds"
                        | "set_value"
                        | "load_font"
                        | "set_listener"
                )
            {
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

#[cfg(test)]
mod tests {
    #[test]
    fn scanner_metrics_skip_source_text_and_detect_uniform_render() {
        let c = super::symbols(
            "{ui_knob $CONTROL_PAR_HIDE} \"ui_slider\" declare ui_knob $k\nset_control_par($k,$CONTROL_PAR_HIDE,0)",
        );
        assert_eq!(c.get("ui_knob"), Some(&1));
        assert!(!c.contains_key("ui_slider"));
        assert_eq!(c.get("$CONTROL_PAR_HIDE"), Some(&1));
        assert_eq!(super::pixels(&[255; 16])["uniform"], true);
        assert_eq!(
            super::pixels(&[0, 0, 0, 255, 255, 255, 255, 255])["uniform"],
            false
        );
    }
}
