//! `kontra-inspect law [NAME]`: the named law table, or one law with its source and
//! sample points.

use std::fmt::Write as _;

use anyhow::{Result, bail};

/// Points sampled across a law's domain.
const SAMPLES: u16 = 8;

/// The table when `name` is `None`; otherwise every law whose name contains `name`
/// (case-insensitive).
pub(crate) fn run(name: Option<&str>) -> Result<String> {
    let all = kontra_law::catalog();
    let mut out = String::new();
    let Some(name) = name else {
        for n in &all {
            writeln!(out, "{:<42} {}", n.name, n.source)?;
        }
        return Ok(out);
    };
    let wanted = name.to_lowercase();
    let hits: Vec<_> = all
        .iter()
        .filter(|n| n.name.to_lowercase().contains(&wanted))
        .collect();
    if hits.is_empty() {
        bail!("no law named like {name}; `kontra-inspect law` lists them");
    }
    for n in hits {
        writeln!(
            out,
            "{}  ({})\n  {}",
            n.name,
            n.source,
            crate::load::law(&n.law)
        )?;
        let [lo, hi] = n.domain;
        for k in 0..=SAMPLES {
            let x = lo + (hi - lo) * f32::from(k) / f32::from(SAMPLES);
            writeln!(out, "  {x:>12.4} -> {:.6}", n.law.eval_unclamped(x))?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn a_named_law_prints_its_source_and_samples() {
        let out = run(Some("attack_curve")).unwrap_or_default();
        assert!(out.contains("0x140ae4958"), "{out}");
        assert_eq!(out.lines().filter(|l| l.contains("->")).count(), 9, "{out}");
        assert!(run(Some("no such law")).is_err());
        assert!(run(None).is_ok_and(|t| t.contains("kontakt::FLEX_SHAPE")));
    }
}
