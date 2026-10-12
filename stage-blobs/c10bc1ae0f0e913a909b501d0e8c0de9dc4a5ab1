//! Host text entry: display text -> plain value.
//!
//! The derive routes every parameter without a `parse_fn` through
//! [`parse_formatted_value`], so hosts' `text -> value` callbacks work
//! for the whole parameter set, including custom `format_fn` fields.

use crate::{ParamInfo, ParamRange, ParamUnit, ParamValueKind};

/// Upper bound on the discrete values scanned per call, so malformed
/// metadata can't turn a host text query into unbounded work.
const MAX_DISCRETE_SCAN: i64 = 4_096;

/// Parse host-entered `text` for the parameter described by `info`.
///
/// 1. Discrete params (bool, int, enum): the first value whose
///    `format` output equals `text` (case-insensitive) wins. This makes
///    `parse(format(v)) == v` hold for enum names and custom formatters.
/// 2. Otherwise the text is read as a number with unit handling (`kHz`,
///    `ms` on seconds params, `%`, pan `L`/`R`/`C`, `on`/`off`) and must
///    format back to the same text.
/// 3. With `lenient` (the built-in formatter, whose units are known),
///    a number that doesn't round-trip exactly (`440` for a `440 Hz`
///    display) is still accepted, clamped to the range. Custom
///    formatters don't get this: a bare number may mean something else
///    in their notation.
pub fn parse_formatted_value(
    info: &ParamInfo,
    text: &str,
    mut format: impl FnMut(f64) -> String,
    lenient: bool,
) -> Option<f64> {
    let text = text.trim();
    let mut matches = |candidate: f64| format(candidate).trim().eq_ignore_ascii_case(text);

    let discrete = match (info.kind, &info.range) {
        (ParamValueKind::Bool, _) => Some((0_i64, 1_i64)),
        (_, ParamRange::Discrete { min, max }) => Some((*min, *max)),
        (_, ParamRange::Enum { count }) => i64::try_from(count.saturating_sub(1))
            .ok()
            .map(|max| (0, max)),
        _ => None,
    };
    if let Some((min, max)) = discrete
        && max >= min
        && max.saturating_sub(min) <= MAX_DISCRETE_SCAN
    {
        #[allow(clippy::cast_precision_loss)] // bounded by MAX_DISCRETE_SCAN around i64 range ends
        let hit = (min..=max).map(|v| v as f64).find(|v| matches(*v));
        if hit.is_some() {
            return hit;
        }
    }

    let candidate = parse_display_number(info, text)?;
    if matches(candidate) {
        return Some(candidate);
    }
    lenient.then(|| candidate.clamp(info.range.min(), info.range.max()))
}

fn parse_display_number(info: &ParamInfo, text: &str) -> Option<f64> {
    let lower = text.to_ascii_lowercase();
    match lower.as_str() {
        "off" | "false" => return Some(0.0),
        "on" | "true" => return Some(1.0),
        "c" | "center" if info.unit == ParamUnit::Pan => return Some(0.0),
        _ => {}
    }

    let mut value: f64 = lower.split_whitespace().find_map(|token| {
        token
            .trim_matches(|c: char| !c.is_ascii_digit() && !matches!(c, '+' | '-' | '.'))
            .parse()
            .ok()
    })?;
    if info.unit == ParamUnit::Pan {
        if lower.ends_with('l') {
            value = -value.abs() / 100.0;
        } else if lower.ends_with('r') {
            value = value.abs() / 100.0;
        }
    } else if lower.contains("khz") {
        value *= 1_000.0;
    } else if lower.ends_with("ms") && info.unit == ParamUnit::Seconds {
        value /= 1_000.0;
    } else if lower.ends_with('%') {
        value /= 100.0;
    }
    value.is_finite().then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParamFlags, format_param_value};

    fn info(kind: ParamValueKind, range: ParamRange, unit: ParamUnit) -> ParamInfo {
        ParamInfo {
            id: 0,
            name: "p",
            short_name: "p",
            group: "",
            range,
            default_plain: 0.0,
            flags: ParamFlags::AUTOMATABLE,
            unit,
            kind,
            midi_map: None,
            midi_channel: None,
        }
    }

    fn parse(info: &ParamInfo, text: &str) -> Option<f64> {
        parse_formatted_value(info, text, |v| format_param_value(info, v), true)
    }

    #[test]
    fn built_in_units_round_trip() {
        let hz = info(
            ParamValueKind::Float,
            ParamRange::Logarithmic {
                min: 20.0,
                max: 20_000.0,
            },
            ParamUnit::Hz,
        );
        assert_eq!(parse(&hz, "2.5 kHz"), Some(2_500.0));
        assert_eq!(parse(&hz, "440 Hz"), Some(440.0));
        // Lenient: a bare number is still read in the display unit.
        assert_eq!(parse(&hz, "440"), Some(440.0));
        assert_eq!(parse(&hz, "99999"), Some(20_000.0));

        let pan = info(
            ParamValueKind::Float,
            ParamRange::Linear {
                min: -1.0,
                max: 1.0,
            },
            ParamUnit::Pan,
        );
        assert_eq!(parse(&pan, "25L"), Some(-0.25));
        assert_eq!(parse(&pan, "C"), Some(0.0));

        let pct = info(
            ParamValueKind::Float,
            ParamRange::Linear { min: 0.0, max: 1.0 },
            ParamUnit::Percent,
        );
        assert_eq!(parse(&pct, "50%"), Some(0.5));
    }

    #[test]
    fn discrete_search_uses_the_formatter() {
        let mode = info(
            ParamValueKind::Enum,
            ParamRange::Enum { count: 3 },
            ParamUnit::None,
        );
        let names = ["Clean", "Warm", "Hot"];
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let fmt = |v: f64| names[v as usize].to_string();
        assert_eq!(parse_formatted_value(&mode, "warm", fmt, false), Some(1.0));
        assert_eq!(parse_formatted_value(&mode, "Nope", fmt, false), None);
    }

    #[test]
    fn custom_formatter_rejects_non_round_trip_numbers() {
        let ratio = info(
            ParamValueKind::Float,
            ParamRange::Linear {
                min: 1.0,
                max: 20.0,
            },
            ParamUnit::None,
        );
        let fmt = |v: f64| format!("x{v:.1}");
        assert_eq!(parse_formatted_value(&ratio, "x4.0", fmt, false), Some(4.0));
        assert_eq!(parse_formatted_value(&ratio, "4", fmt, false), None);
    }
}
