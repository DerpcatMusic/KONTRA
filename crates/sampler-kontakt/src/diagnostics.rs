//! Select safe semantic detail for compact loader messages; opaque payloads stay private.
pub fn diagnostic_detail<'a>(feature: &str, value: &'a str) -> Option<&'a str> {
    if value.is_empty() {
        return None;
    }
    match feature {
        "modulation target" | "modulation of a module parameter" => Some(
            value
                .split_once(" intensity ")
                .map_or(value, |(target, _)| target),
        ),
        "effect" | "Filter: filter type" | "Compressor: compressor mode" => Some(value),
        _ if feature.starts_with("lua ") || feature.starts_with("external modulation from ") => {
            Some(value)
        }
        _ if feature.starts_with("script ")
            && (value.starts_with("array index ")
                || value.starts_with("find_group: group not found; ")
                || feature.split_once(": ").is_some_and(|(_, builtin)| {
                    value
                        .strip_prefix(builtin)
                        .is_some_and(|rest| rest.starts_with(' ') || rest.starts_with(':'))
                })) =>
        {
            Some(value)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_details_name_effects_targets_and_actual_script_indices() {
        assert_eq!(
            diagnostic_detail("effect", "unknown effect 0xfe v0x50 len 64"),
            Some("unknown effect 0xfe v0x50 len 64")
        );
        assert_eq!(
            diagnostic_detail(
                "modulation target",
                "wavetablePosition (WT POS) destination slot 3 intensity 0.2"
            ),
            Some("wavetablePosition (WT POS) destination slot 3")
        );
        assert_eq!(
            diagnostic_detail(
                "script Warning: array index N out of bounds",
                "array index 9 out of bounds"
            ),
            Some("array index 9 out of bounds")
        );
        assert_eq!(
            diagnostic_detail("opaque effect payload", "private bytes"),
            None
        );
        assert_eq!(
            diagnostic_detail("script Warning: first warning", "private authored value"),
            None
        );
        assert_eq!(diagnostic_detail("modulation target", ""), None);
    }
}
