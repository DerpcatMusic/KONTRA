//! Kontakt's saved persistent values (`"<name> <value…>"`, one per variable)
//! as writes to the variables' cells, applied after `on init`.

use crate::{Cells, Script};

/// Cell writes that restore saved values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Restore {
    /// By cell, so a range of them is found by binary search; a cell saved
    /// twice keeps its writes in saved order.
    pub ints: Vec<(u32, i64)>,
    /// By cell, as `ints`.
    pub texts: Vec<(u32, String)>,
    /// Entries naming no variable of this script, or unreadable.
    pub skipped: Vec<String>,
}

impl Script {
    /// What `saved` writes into this script's variables. Arrays saved shorter
    /// than declared repeat their last value.
    #[must_use]
    pub fn restore(&self, saved: &[String]) -> Restore {
        restore(saved, |key| {
            let var = self.variables.iter().find(|v| named(&v.name, key))?;
            Some(var.cells)
        })
    }
}

/// What `saved` writes into the variables `cells` finds by name, as
/// [`Script::restore`] does: `cells` gets the saved name as written (so
/// `$Tuning` finds `$Tuning`, not an earlier `%Tuning`), then, when that
/// finds nothing, its bare lowercase name (no type sigil). See [`named`].
#[must_use]
pub fn restore(saved: &[String], cells: impl Fn(&str) -> Option<Cells>) -> Restore {
    let mut out = Restore::default();
    for entry in saved {
        let Some((name, payload)) = entry.split_once(' ') else {
            out.skipped.push(entry.clone());
            continue;
        };
        let Some(var) = cells(name).or_else(|| cells(&bare(name))) else {
            out.skipped.push(name.to_owned());
            continue;
        };
        let real = |t: &str| t.parse::<f64>().ok().map(|v| v.to_bits() as i64);
        let int = |t: &str| t.parse::<i32>().ok().map(i64::from);
        let ok = match var {
            Cells::Int { base, len } => fill(&mut out.ints, base, len, payload, int),
            Cells::Real { base, len } => fill(&mut out.ints, base, len, payload, real),
            Cells::Text { base, len } => {
                let lines: Vec<&str> = if len == 1 {
                    vec![payload]
                } else {
                    payload.split('\n').collect()
                };
                for (n, line) in (0..len).zip(lines) {
                    out.texts.push((base + n, line.to_owned()));
                }
                true
            }
            _ => false,
        };
        if !ok {
            out.skipped.push(name.to_owned());
        }
    }
    // Stable: a cell saved twice still ends on its last value.
    out.ints.sort_by_key(|&(cell, _)| cell);
    out.texts.sort_by_key(|&(cell, _)| cell);
    out
}

/// Whether variable `name` is the one a saved `key` names: the same name,
/// any case, or, for a bare key, the same name without its sigil.
#[must_use]
pub fn named(name: &str, key: &str) -> bool {
    name.eq_ignore_ascii_case(key) || bare(name) == key
}

/// A variable's name without its type sigil, lowercase.
fn bare(name: &str) -> String {
    name.trim_start_matches(['$', '%', '~', '?', '@', '!'])
        .to_ascii_lowercase()
}

fn fill(
    out: &mut Vec<(u32, i64)>,
    base: u32,
    len: u32,
    payload: &str,
    parse: impl Fn(&str) -> Option<i64>,
) -> bool {
    let values: Option<Vec<i64>> = payload.split_ascii_whitespace().map(parse).collect();
    let Some(values) = values.filter(|v| !v.is_empty()) else {
        return false;
    };
    let last = values.last().copied().unwrap_or(0);
    for n in 0..len {
        out.push((base + n, values.get(n as usize).copied().unwrap_or(last)));
    }
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn saved_values_land_in_their_variables_cells() {
        let script = crate::compile(
            "on init
                declare $a
                declare %b[4]
                declare ~c
                declare @d
                make_persistent($a)
            end on",
            &[],
        )
        .unwrap();
        let saved = ["~c 0.5", "%B 1 2", "$a 7", "@d hello world", "$gone 1"].map(String::from);
        let r = script.restore(&saved);
        let cell = |name: &str| match script
            .variables
            .iter()
            .find(|v| &*v.name == name)
            .unwrap()
            .cells
        {
            crate::Cells::Int { base, .. }
            | crate::Cells::Real { base, .. }
            | crate::Cells::Text { base, .. } => base,
            other => panic!("{other:?}"),
        };
        let b = cell("%b");
        assert!(r.ints.is_sorted_by_key(|p| p.0), "{:?}", r.ints);
        assert!(r.ints.contains(&(cell("$a"), 7)));
        assert_eq!(
            r.ints
                .iter()
                .filter(|(c, _)| (b..b + 4).contains(c))
                .map(|p| p.1)
                .collect::<Vec<_>>(),
            [1, 2, 2, 2]
        );
        assert!(r.ints.contains(&(cell("~c"), 0.5f64.to_bits() as i64)));
        assert_eq!(r.texts, [(cell("@d"), "hello world".to_owned())]);
        assert_eq!(r.skipped, ["$gone"]);
    }

    /// Una Corda saves `$Tuning 0` and declares `%Tuning[128]` first: the
    /// scalar must not land in the array (it zeroed the stretch tuning).
    #[test]
    fn a_saved_name_finds_its_own_sigil_first() {
        let script = crate::compile(
            "on init
                declare %tuning[3]
                declare $Tuning
                make_persistent($Tuning)
            end on",
            &[],
        )
        .unwrap();
        let base = |name: &str| match script.variables.iter().find(|v| &*v.name == name) {
            Some(v) => match v.cells {
                crate::Cells::Int { base, .. } => base,
                other => panic!("{other:?}"),
            },
            None => panic!("{name}"),
        };
        let r = script.restore(&["$Tuning 1".to_owned(), "tuning 2".to_owned()]);
        assert_eq!(
            r.ints,
            [
                (base("%tuning"), 2),
                (base("%tuning") + 1, 2),
                (base("%tuning") + 2, 2),
                (base("$Tuning"), 1)
            ]
        );
    }
}
