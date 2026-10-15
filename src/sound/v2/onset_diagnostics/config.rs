//! Shared helper/root contract. Parsing performs no I/O or loader access.
use serde_json::Value;
use std::{path::PathBuf, time::Duration};

#[derive(Clone, Debug)]
pub(super) struct Selection {
    pub slot: u8,
    pub articulation: u32,
    pub selector_ordinal: usize,
    pub selector_source: i32,
    pub keyswitch: u8,
    pub rr_cycle: u8,
    pub keyswitch_base_field: String,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub(super) path: PathBuf,
    pub(super) selection: Selection,
    pub(super) rate: u32,
    pub(super) block: usize,
    pub(super) key: u8,
    pub(super) velocity: u8,
    pub(super) hold: Duration,
    pub(super) spacing: Duration,
    pub(super) startup_deadline: Duration,
    pub(super) phase_deadline: Duration,
    pub(super) total_deadline: Duration,
    pub(super) tail_deadline: Duration,
    pub(super) pacing_tolerance: Duration,
    pub(super) row_capacity: usize,
    pub(super) graph_scope: sampler_core::trace::SignalTraceScope,
}
fn integer(v: &Value, key: &str, low: u64, high: u64) -> Result<u64, &'static str> {
    v.get(key)
        .and_then(Value::as_u64)
        .filter(|n| (low..=high).contains(n))
        .ok_or("Missing, noninteger or out-of-range numeric configuration")
}
fn fields(v: &Value, expected: &[&str]) -> Result<(), &'static str> {
    let object = v.as_object().ok_or("Configuration must be an object")?;
    if object.len() != expected.len() || object.keys().any(|k| !expected.contains(&k.as_str())) {
        return Err("Missing or unknown configuration field");
    }
    Ok(())
}
impl Config {
    /// Validate every typed limit before any library access or worker creation.
    pub fn parse(v: &Value) -> Result<Self, &'static str> {
        fields(
            v,
            &[
                "schema",
                "opt_in",
                "path",
                "state_origin",
                "input_origin",
                "selection",
                "rate",
                "block",
                "key",
                "velocity",
                "hold_ms",
                "spacing_ms",
                "startup_ms",
                "phase_ms",
                "total_ms",
                "tail_ms",
                "pacing_tolerance_us",
                "row_capacity",
                "warm_intervention",
                "graph_scope",
            ],
        )?;
        if v["schema"] != 1
            || v["opt_in"] != true
            || v["state_origin"] != "saved_state_diagnostic"
            || v["input_origin"] != "synthetic_native"
            || v["warm_intervention"] != "exact_first_windows"
        {
            return Err(
                "Explicit saved-state, synthetic-native and exact-window diagnostic opt-in is required",
            );
        }
        let path_text = v["path"].as_str().ok_or("Input path is required")?;
        if path_text.len() > 4096 || path_text.contains('\0') {
            return Err("Input path exceeds its bound");
        }
        let path = PathBuf::from(path_text);
        if !path.is_absolute()
            || !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("nki"))
        {
            return Err("This version requires an absolute Kontakt NKI path");
        }
        let s = &v["selection"];
        fields(
            s,
            &[
                "slot",
                "articulation",
                "selector_ordinal",
                "selector_source",
                "keyswitch",
                "rr_cycle",
                "keyswitch_base_field",
            ],
        )?;
        let base = s["keyswitch_base_field"]
            .as_str()
            .ok_or("An explicit integer keyswitch-base setup field is required")?;
        if base.is_empty() || base.len() > 128 || !base.is_ascii() || base.contains('\0') {
            return Err("Keyswitch-base field exceeds its bound");
        }
        let selection = Selection {
            slot: integer(s, "slot", 0, 4)? as u8,
            articulation: integer(s, "articulation", 0, 127)? as u32,
            selector_ordinal: integer(s, "selector_ordinal", 0, 65535)? as usize,
            selector_source: integer(s, "selector_source", 0, i32::MAX as u64)? as i32,
            keyswitch: integer(s, "keyswitch", 0, 127)? as u8,
            rr_cycle: integer(s, "rr_cycle", 1, 32)? as u8,
            keyswitch_base_field: base.to_owned(),
        };
        let graph = &v["graph_scope"];
        fields(
            graph,
            &["prepared_key_low", "prepared_key_high", "max_nodes"],
        )?;
        let graph_scope = sampler_core::trace::SignalTraceScope::new(
            integer(graph, "prepared_key_low", 0, 127)? as u8,
            integer(graph, "prepared_key_high", 0, 127)? as u8,
            integer(graph, "max_nodes", 1, 65536)? as usize,
        )
        .map_err(|_| "Invalid prepared-region diagnostic graph scope")?;
        let ms = |key, low, high| integer(v, key, low, high).map(Duration::from_millis);
        let c = Self {
            path,
            selection,
            graph_scope,
            rate: integer(v, "rate", 48000, 48000)? as u32,
            block: integer(v, "block", 128, 128)? as usize,
            key: integer(v, "key", 0, 127)? as u8,
            velocity: integer(v, "velocity", 1, 127)? as u8,
            hold: ms("hold_ms", 1, 3000)?,
            spacing: ms("spacing_ms", 1000, 5000)?,
            startup_deadline: ms("startup_ms", 1, 30000)?,
            phase_deadline: ms("phase_ms", 1, 30000)?,
            total_deadline: ms("total_ms", 1, 120000)?,
            tail_deadline: ms("tail_ms", 1, 10000)?,
            pacing_tolerance: Duration::from_micros(integer(v, "pacing_tolerance_us", 1, 100000)?),
            row_capacity: integer(v, "row_capacity", 128, 65536)? as usize,
        };
        if c.key == c.selection.keyswitch
            || c.hold >= c.spacing
            || c.phase_deadline < c.spacing
            || c.startup_deadline
                + c.phase_deadline
                + c.spacing * u32::from(c.selection.rr_cycle)
                + c.hold
                + c.tail_deadline
                > c.total_deadline
        {
            return Err("Declared phase schedule cannot fit the total deadline");
        }
        Ok(c)
    }
    pub fn total_deadline(&self) -> Duration {
        self.total_deadline
    }
}

/// Compares observed state directly; no invented identity or state tokens.
pub(super) struct Settlement<T> {
    previous: Option<(u64, T)>,
    quiet_ticks: u8,
}
impl<T> Default for Settlement<T> {
    fn default() -> Self {
        Self {
            previous: None,
            quiet_ticks: 0,
        }
    }
}
impl<T: PartialEq> Settlement<T> {
    pub fn observe(&mut self, tick: u64, state: T, quiet: bool) -> Result<bool, &'static str> {
        if self
            .previous
            .as_ref()
            .is_some_and(|(last, _)| tick <= *last)
        {
            return Err("Serviced control ticks did not advance");
        }
        self.quiet_ticks = if !quiet {
            0
        } else if self
            .previous
            .as_ref()
            .is_some_and(|(_, last)| *last == state)
        {
            self.quiet_ticks.saturating_add(1)
        } else {
            1
        };
        self.previous = Some((tick, state));
        Ok(self.quiet_ticks >= 3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    pub(crate) fn plan() -> Value {
        json!({"schema":1,"opt_in":true,"path":std::env::current_dir().unwrap().join("fixture-input.nki"),
        "state_origin":"saved_state_diagnostic","input_origin":"synthetic_native", "warm_intervention":"exact_first_windows",
        "selection":{"slot":0,"articulation":2,"selector_ordinal":110,"selector_source":32878,"keyswitch":14,"rr_cycle":8,
            "keyswitch_base_field":"$fixture_base"},
        "graph_scope":{"prepared_key_low":60,"prepared_key_high":60,"max_nodes":8192},
        "rate":48000,"block":128,"key":60,"velocity":100,"hold_ms":500,"spacing_ms":3000,
        "startup_ms":30000,"phase_ms":10000,"total_ms":120000,"tail_ms":5000,"pacing_tolerance_us":1000,"row_capacity":8192})
    }
    #[test]
    fn every_limit_is_validated_before_loader_access() {
        assert!(Config::parse(&plan()).is_ok());
        for (field, value) in [
            ("opt_in", json!(false)),
            ("total_ms", json!(120001)),
            ("startup_ms", json!(30001)),
            ("rate", json!(44100)),
            ("block", json!(0)),
            ("input_origin", json!("bitwig_capture")),
            ("state_origin", json!("user_live")),
            ("row_capacity", json!(65537)),
            ("key", json!(128)),
            ("velocity", json!(0)),
            ("hold_ms", json!(5000)),
            ("spacing_ms", json!(999)),
            ("tail_ms", json!(10001)),
            ("pacing_tolerance_us", json!(100001)),
            ("phase_ms", json!(30001)),
            ("warm_intervention", json!("none")),
            ("path", json!("/fixture/input.ufs")),
        ] {
            let mut v = plan();
            v[field] = value;
            assert!(Config::parse(&v).is_err(), "{field}");
        }
        for (field, value) in [
            ("slot", json!(5)),
            ("articulation", json!(128)),
            ("selector_ordinal", json!(65536)),
            ("selector_source", json!(-1)),
            ("keyswitch", json!(128)),
            ("rr_cycle", json!(33)),
            ("keyswitch_base_field", json!("")),
        ] {
            let mut v = plan();
            v["selection"][field] = value;
            assert!(Config::parse(&v).is_err(), "{field}");
        }
        let mut v = plan();
        v["selection"]["rr_cycle"] = json!(32);
        assert!(Config::parse(&v).is_err());
        v = plan();
        v["unknown"] = json!(0);
        assert!(Config::parse(&v).is_err());
    }
    #[test]
    fn graph_scope_is_explicit_and_can_widen_independently_of_input_key() {
        for scope in [
            serde_json::Value::Null,
            json!({"prepared_key_low":61,"prepared_key_high":60,"max_nodes":8192}),
            json!({"prepared_key_low":60,"prepared_key_high":128,"max_nodes":8192}),
            json!({"prepared_key_low":60,"prepared_key_high":60,"max_nodes":65537}),
            json!({"prepared_key_low":60,"prepared_key_high":60,"max_nodes":0}),
            json!({"prepared_key_low":60,"prepared_key_high":60,"max_nodes":8192,"hidden":true}),
        ] {
            let mut v = plan();
            v["graph_scope"] = scope;
            assert!(Config::parse(&v).is_err());
        }
        let mut v = plan();
        v["graph_scope"]["prepared_key_high"] = json!(72);
        assert_eq!(Config::parse(&v).unwrap().graph_scope.prepared_key_high, 72);
        // Input key is no proof of generated/transposed-region graph coverage.
        v["key"] = json!(48);
        assert!(Config::parse(&v).is_ok());
    }
    #[test]
    fn quiet_ticks_require_advancing_actual_equal_state() {
        let mut s = Settlement::default();
        assert_eq!(s.observe(1, vec![1, 2], true), Ok(false));
        assert_eq!(s.observe(2, vec![1, 2], false), Ok(false));
        assert_eq!(s.observe(3, vec![1, 2], true), Ok(false));
        assert_eq!(s.observe(4, vec![1, 2], true), Ok(false));
        assert_eq!(s.observe(5, vec![1, 2], true), Ok(true));
        assert!(s.observe(5, vec![1, 2], true).is_err());
        assert_eq!(s.observe(6, vec![1, 3], true), Ok(false));
    }
}
