use serde::Deserialize;

use crate::Law;

/// Errors from parsing a law file.
#[derive(Debug, thiserror::Error)]
pub enum LawError {
    /// The text is not TOML, or does not match the schema.
    #[error("law file does not parse: {0}")]
    Syntax(#[from] toml::de::Error),
    /// The schema matched, but a value is out of range.
    #[error("invalid law: {0}")]
    Invalid(&'static str),
}

/// A parsed law file: the curve and the measurement tolerance it was fitted to.
#[derive(Clone, Debug, PartialEq)]
pub struct LawSpec {
    /// The curve.
    pub law: Law,
    /// Error in dB that a measured response may show against this law and still count as a match.
    pub tolerance_db: f32,
}

/// A law file embedded in the binary by [`laws!`](crate::laws).
#[derive(Clone, Copy, Debug)]
pub struct LawSource {
    /// Path the file was embedded from, for error messages.
    pub path: &'static str,
    /// Contents of the file.
    pub text: &'static str,
}

impl LawSource {
    /// Parses the embedded text.
    pub fn parse(self) -> Result<LawSpec, LawError> {
        parse(self.text)
    }
}

/// Parses a law file. See `README.md` for the schema.
pub fn parse(text: &str) -> Result<LawSpec, LawError> {
    let file: File = toml::from_str(text)?;
    if !(file.tolerance_db.is_finite() && file.tolerance_db >= 0.0) {
        return Err(LawError::Invalid(
            "tolerance_db must be finite and not negative",
        ));
    }
    Ok(LawSpec {
        law: file.law.into_law()?,
        tolerance_db: file.tolerance_db,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    tolerance_db: f32,
    law: Shape,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Shape {
    Linear { lo: f32, hi: f32 },
    Exp { lo: f32, hi: f32 },
    Db { lo: f32, hi: f32 },
    Table { values: Vec<f32> },
    Breakpoints { points: Vec<[f32; 3]> },
}

impl Shape {
    fn into_law(self) -> Result<Law, LawError> {
        match self {
            Self::Linear { lo, hi } => {
                finite_bounds(lo, hi)?;
                Ok(Law::Linear { lo, hi })
            }
            Self::Exp { lo, hi } => {
                finite_bounds(lo, hi)?;
                if lo <= 0.0 || hi <= 0.0 {
                    return Err(LawError::Invalid("exp bounds must be above zero"));
                }
                Ok(Law::Exp { lo, hi })
            }
            Self::Db { lo, hi } => {
                finite_bounds(lo, hi)?;
                Ok(Law::Db { lo, hi })
            }
            Self::Table { values } => {
                if values.len() < 2 {
                    return Err(LawError::Invalid("table needs at least two values"));
                }
                if !values.iter().all(|v| v.is_finite()) {
                    return Err(LawError::Invalid("table values must be finite"));
                }
                Ok(Law::Table(values.into()))
            }
            Self::Breakpoints { points } => {
                if points.len() < 2 {
                    return Err(LawError::Invalid("breakpoints need at least two points"));
                }
                if !points.iter().flatten().all(|v| v.is_finite()) {
                    return Err(LawError::Invalid("breakpoint values must be finite"));
                }
                if !points
                    .windows(2)
                    .all(|w| matches!(w, [a, b] if a[0] < b[0]))
                {
                    return Err(LawError::Invalid(
                        "breakpoint x values must strictly increase",
                    ));
                }
                let points: Vec<(f32, f32, f32)> =
                    points.into_iter().map(|[x, y, c]| (x, y, c)).collect();
                Ok(Law::Breakpoints(points.into()))
            }
        }
    }
}

fn finite_bounds(lo: f32, hi: f32) -> Result<(), LawError> {
    if lo.is_finite() && hi.is_finite() {
        Ok(())
    } else {
        Err(LawError::Invalid("bounds must be finite"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn parses_every_shape() {
        let cases = [
            (
                "kind = \"linear\"\nlo = 0.0\nhi = 1.0",
                Law::Linear { lo: 0.0, hi: 1.0 },
            ),
            (
                "kind = \"exp\"\nlo = 20.0\nhi = 20000.0",
                Law::Exp {
                    lo: 20.0,
                    hi: 20_000.0,
                },
            ),
            (
                "kind = \"db\"\nlo = -60.0\nhi = 6.0",
                Law::Db { lo: -60.0, hi: 6.0 },
            ),
            (
                "kind = \"table\"\nvalues = [0.0, 0.5, 1.0]",
                Law::Table(Arc::from([0.0, 0.5, 1.0])),
            ),
            (
                "kind = \"breakpoints\"\npoints = [[0.0, 0.0, 0.0], [1.0, 1.0, 2.0]]",
                Law::Breakpoints(Arc::from([(0.0, 0.0, 0.0), (1.0, 1.0, 2.0)])),
            ),
        ];
        for (body, want) in cases {
            let text = format!("tolerance_db = 0.1\n[law]\n{body}\n");
            let spec = parse(&text).unwrap_or_else(|e| panic!("{body}: {e}"));
            assert_eq!(spec.law, want, "{body}");
            assert_eq!(spec.tolerance_db, 0.1);
        }
    }

    #[test]
    fn rejects_out_of_range_values() {
        let cases = [
            (
                "tolerance_db = -1.0\n[law]\nkind = \"linear\"\nlo = 0.0\nhi = 1.0",
                "tolerance",
            ),
            (
                "tolerance_db = nan\n[law]\nkind = \"linear\"\nlo = 0.0\nhi = 1.0",
                "tolerance",
            ),
            (
                "tolerance_db = 0.1\n[law]\nkind = \"exp\"\nlo = 0.0\nhi = 1.0",
                "exp bounds",
            ),
            (
                "tolerance_db = 0.1\n[law]\nkind = \"linear\"\nlo = nan\nhi = 1.0",
                "bounds",
            ),
            (
                "tolerance_db = 0.1\n[law]\nkind = \"table\"\nvalues = [1.0]",
                "two values",
            ),
            (
                "tolerance_db = 0.1\n[law]\nkind = \"breakpoints\"\npoints = [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0]]",
                "strictly increase",
            ),
        ];
        for (text, reason) in cases {
            match parse(text) {
                Err(LawError::Invalid(msg)) => assert!(msg.contains(reason), "{text}: {msg}"),
                other => panic!("{text}: expected invalid, got {other:?}"),
            }
        }
    }

    #[test]
    fn rejects_unknown_keys_and_missing_tolerance() {
        let typo = "tolerence_db = 0.1\n[law]\nkind = \"linear\"\nlo = 0.0\nhi = 1.0";
        assert!(matches!(parse(typo), Err(LawError::Syntax(_))));
        let missing = "[law]\nkind = \"linear\"\nlo = 0.0\nhi = 1.0";
        assert!(matches!(parse(missing), Err(LawError::Syntax(_))));
    }
}
