//! Kontakt's predefined names: constants with a value, and catalog names
//! scripts use only by identity (symbols, numbered per script from [`SYMBOL_BASE`]).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// First symbol value. Symbols are opaque: the host maps them back by name.
pub const SYMBOL_BASE: i32 = 0x0100_0000;
/// The first UI control's id; controls are numbered in declaration order.
pub const FIRST_UI_ID: i32 = 32768;

/// The value of a predefined integer constant.
pub fn int(name: &str) -> Option<i32> {
    static TABLE: OnceLock<HashMap<&'static str, i32>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        include_str!("../data/constants.tsv")
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| {
                let (name, value) = l.split_once('\t')?;
                Some((name, value.parse().ok()?))
            })
            .collect()
    });
    table.get(name).copied()
}

pub fn real(name: &str) -> Option<f64> {
    match name {
        "~NI_MATH_PI" => Some(std::f64::consts::PI),
        "~NI_MATH_E" => Some(std::f64::consts::E),
        _ => None,
    }
}

/// Whether `name` is in Kontakt's catalog of predefined names. Digit runs
/// match `#`, so numbered families (`$ENGINE_PAR_SENDLEVEL_3`) match one entry.
pub fn is_catalog(name: &str) -> bool {
    static CATALOG: OnceLock<HashSet<&'static str>> = OnceLock::new();
    let catalog = CATALOG.get_or_init(|| {
        include_str!("../data/vendor_constants.txt")
            .lines()
            .collect()
    });
    let mut key = String::with_capacity(name.len());
    let mut digits = false;
    for c in name.chars() {
        if !c.is_ascii_digit() {
            key.push(c);
        } else if !digits {
            key.push('#');
        }
        digits = c.is_ascii_digit();
    }
    catalog.contains(key.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_and_catalog() {
        assert_eq!(int("$EVENT_PAR_TUNE"), Some(6));
        assert_eq!(int("$MARK_3"), Some(8));
        assert_eq!(int("$EFFECT_TYPE_NONE"), Some(0));
        assert_eq!(int("$ENGINE_PAR_VOLUME"), None);
        assert!(is_catalog("$ENGINE_PAR_VOLUME"));
        assert!(is_catalog("$CONTROL_PAR_VALUE"));
        assert!(!is_catalog("$MY_OWN_THING"));
    }
}
