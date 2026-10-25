//! Ratchet for `debt` and `size`: a counter may stay or shrink against its committed JSON
//! baseline, never grow. `--update` rewrites the baseline; the ratchet only tightens.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub type Counts = BTreeMap<String, usize>;

/// A counter that moved against its baseline.
#[derive(Debug, PartialEq, Eq)]
pub struct Change {
    pub key: String,
    pub before: usize,
    pub after: usize,
}

/// Counters above their baseline. A key the baseline lacks counts as zero there.
pub fn grown(current: &Counts, baseline: &Counts) -> Vec<Change> {
    changes(current, baseline, |after, before| after > before)
}

/// Counters below their baseline, including keys that vanished (they now count as zero).
pub fn shrunk(current: &Counts, baseline: &Counts) -> Vec<Change> {
    changes(current, baseline, |after, before| after < before)
}

fn changes(
    current: &Counts,
    baseline: &Counts,
    moved: impl Fn(usize, usize) -> bool,
) -> Vec<Change> {
    let keys: BTreeSet<&String> = current.keys().chain(baseline.keys()).collect();
    keys.into_iter()
        .filter_map(|key| {
            let after = current.get(key).copied().unwrap_or(0);
            let before = baseline.get(key).copied().unwrap_or(0);
            moved(after, before).then(|| Change {
                key: key.clone(),
                before,
                after,
            })
        })
        .collect()
}

pub fn read(path: &Path) -> Result<Counts, String> {
    let text = fs::read_to_string(path).map_err(|error| {
        format!(
            "cannot read {}: {error} (create it with --update)",
            path.display()
        )
    })?;
    serde_json::from_str(&text).map_err(|error| format!("cannot parse {}: {error}", path.display()))
}

pub fn write(path: &Path, counts: &Counts) -> Result<(), String> {
    let json = serde_json::to_string_pretty(counts)
        .map_err(|error| format!("cannot encode counts: {error}"))?;
    fs::write(path, json + "\n")
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(pairs: &[(&str, usize)]) -> Counts {
        pairs.iter().map(|(key, n)| (key.to_string(), *n)).collect()
    }

    #[test]
    fn a_key_missing_from_the_baseline_counts_as_zero() {
        let current = counts(&[("a", 2), ("b", 1)]);
        let baseline = counts(&[("a", 2)]);
        assert_eq!(
            grown(&current, &baseline),
            vec![Change {
                key: "b".into(),
                before: 0,
                after: 1
            }]
        );
        assert!(shrunk(&current, &baseline).is_empty());
    }

    #[test]
    fn shrunk_reports_lower_and_vanished_keys_but_not_equal_ones() {
        let current = counts(&[("a", 1), ("same", 4)]);
        let baseline = counts(&[("a", 3), ("gone", 2), ("same", 4)]);
        assert_eq!(
            shrunk(&current, &baseline),
            vec![
                Change {
                    key: "a".into(),
                    before: 3,
                    after: 1
                },
                Change {
                    key: "gone".into(),
                    before: 2,
                    after: 0
                },
            ]
        );
        assert!(grown(&current, &baseline).is_empty());
    }
}
