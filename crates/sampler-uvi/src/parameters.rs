//! Public UVI element facts from https://lua.uvi.net/_elements.html (2026-10-07).
//! IDs are stable ordinals within an element; DSP bindings use original XML IDs.
use std::{collections::BTreeMap, sync::OnceLock};

pub struct Parameter {
    pub name: &'static str,
    pub kind: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub unit: &'static str,
}

pub fn definitions(kind: &str) -> &'static [Parameter] {
    static CATALOG: OnceLock<BTreeMap<&str, Vec<Parameter>>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut out = BTreeMap::<_, Vec<_>>::new();
        for line in include_str!("parameters.tsv").lines() {
            let p: Vec<_> = line.split('\t').collect();
            out.entry(p[0]).or_default().push(Parameter {
                name: p[1], kind: p[2], min: p[3].parse().unwrap(),
                max: p[4].parse().unwrap(), default: p[5].parse().unwrap(), unit: p[6],
            });
        }
        out
    }).get(kind).map_or(&[], Vec::as_slice)
}
