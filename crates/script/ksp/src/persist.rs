//! Kontakt's saved persistent values (`"<name> <value…>"`, one per variable)
//! as writes to the variables' cells, applied after `on init`.

use crate::{Cells, Script};

/// Cell writes that restore saved values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Restore {
    pub ints: Vec<(u32, i64)>,
    pub texts: Vec<(u32, String)>,
    /// Entries naming no variable of this script, or unreadable.
    pub skipped: Vec<String>,
}

impl Script {
    /// What `saved` writes into this script's variables. Arrays saved shorter
    /// than declared repeat their last value.
    #[must_use]
    pub fn restore(&self, saved: &[String]) -> Restore {
        let bare = |name: &str| {
            name.trim_start_matches(['$', '%', '~', '?', '@', '!'])
                .to_ascii_lowercase()
        };
        let mut out = Restore::default();
        for entry in saved {
            let Some((name, payload)) = entry.split_once(' ') else {
                out.skipped.push(entry.clone());
                continue;
            };
            let key = bare(name);
            let Some(var) = self.variables.iter().find(|v| bare(&v.name) == key) else {
                out.skipped.push(name.to_owned());
                continue;
            };
            let real = |t: &str| t.parse::<f64>().ok().map(|v| v.to_bits() as i64);
            let int = |t: &str| t.parse::<i32>().ok().map(i64::from);
            let ok = match var.cells {
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
        out
    }
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
        let saved = ["$a 7", "%B 1 2", "~c 0.5", "@d hello world", "$gone 1"].map(String::from);
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
}
