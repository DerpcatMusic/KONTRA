//! Public UVI element facts from https://lua.uvi.net/_elements.html (2026-10-07).
//! IDs are stable ordinals within an element; DSP bindings use original XML IDs.
use std::{collections::{BTreeMap, HashMap}, sync::OnceLock};

pub struct Parameter {
    pub name: &'static str,
    pub kind: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub unit: &'static str,
}

type Catalog = BTreeMap<&'static str, Vec<Parameter>>;

fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut out = BTreeMap::<_, Vec<_>>::new();
        for line in include_str!("parameters.tsv").lines() {
            let p: Vec<_> = line.split('\t').collect();
            out.entry(p[0]).or_default().push(Parameter {
                name: p[1],
                kind: p[2],
                min: p[3].parse().unwrap(),
                max: p[4].parse().unwrap(),
                default: p[5].parse().unwrap(),
                unit: p[6],
            });
        }
        out
    })
}

pub fn definitions(kind: &str) -> &'static [Parameter] {
    catalog().get(kind).map_or(&[], Vec::as_slice)
}

/// The definition named `name` of `kind`, by hash lookup rather than a scan of
/// the kind's rows (a hot `getParameter` path runs this for every read).
pub fn definition(kind: &str, name: &str) -> Option<&'static Parameter> {
    type Index = HashMap<&'static str, HashMap<&'static str, &'static Parameter>>;
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX
        .get_or_init(|| {
            catalog()
                .iter()
                .map(|(kind, rows)| {
                    // First row wins, matching the linear `find` this replaces.
                    let mut names = HashMap::new();
                    for p in rows {
                        names.entry(p.name).or_insert(p);
                    }
                    (*kind, names)
                })
                .collect()
        })
        .get(kind)?
        .get(name)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_lookup_returns_the_first_row_the_linear_scan_would() {
        for line in include_str!("parameters.tsv").lines() {
            let name = line.split('\t').nth(1).unwrap();
            let kind = line.split('\t').next().unwrap();
            let linear = definitions(kind).iter().find(|p| p.name == name).unwrap();
            let hashed = definition(kind, name).unwrap();
            assert!(std::ptr::eq(linear, hashed), "{kind}.{name}");
        }
        assert!(definition("NoSuchKind", "Gain").is_none());
        assert!(definition("Keygroup", "NoSuchParameter").is_none());
    }
}
