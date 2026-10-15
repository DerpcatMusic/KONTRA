//! Admission results for bank Lua occurrences. No source or access values are
//! included in these diagnostics; successfully decoded source stays in Scripts.

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum ScriptReadError {
    UnresolvedPath,
    Ambiguous,
    UnsupportedProtection(u8),
    AccessRequired,
    Limit,
    InvalidRange,
    Read,
    InvalidUtf8 { valid_up_to: usize, error_len: Option<usize> },
}

impl ScriptReadError {
    /// Stable, message-free category for inspection and load summaries.
    pub fn category(self) -> &'static str {
        match self {
            Self::UnresolvedPath => "unresolved_path",
            Self::Ambiguous => "ambiguous",
            Self::UnsupportedProtection(_) => "unsupported_protection",
            Self::AccessRequired => "access_required",
            Self::Limit => "limit",
            Self::InvalidRange => "invalid_range",
            Self::Read => "read_failed",
            Self::InvalidUtf8 { .. } => "unsupported_encoding",
        }
    }
}

impl std::fmt::Display for ScriptReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnresolvedPath => f.write_str("UVI Lua member has no resolved path"),
            Self::Ambiguous => f.write_str("ambiguous UVI Lua member"),
            Self::UnsupportedProtection(_) => f.write_str("unsupported UVI Lua member protection"),
            Self::AccessRequired => f.write_str("UVI Lua member requires content access"),
            Self::Limit => f.write_str("UVI Lua member exceeds the resource byte limit"),
            Self::InvalidRange => f.write_str("UVI Lua member range exceeds its container"),
            Self::Read => f.write_str("UVI Lua member read failed"),
            Self::InvalidUtf8 { .. } => f.write_str("unsupported UVI Lua source encoding: invalid UTF-8"),
        }
    }
}
impl std::error::Error for ScriptReadError {}

/// One `.lua` occurrence, including unlinked, ambiguous and unreadable ones.
/// Index and record offset identify the occurrence even when its path is absent.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ScriptOccurrence {
    pub member_index: usize,
    pub record_offset: u64,
    pub path: Option<String>,
    pub bytes: u64,
    pub outcome: Result<(), ScriptReadError>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script::{Files, Scripts};

    fn failed(files: &mut Scripts, path: &str, error: ScriptReadError) {
        files.record(ScriptOccurrence {
            member_index: files.occurrences().len(), record_offset: 320,
            path: Some(path.into()), bytes: 1, outcome: Err(error),
        }, None);
    }

    #[test]
    fn exact_failure_cannot_fall_through_to_an_alias_or_builtin() {
        let mut files = Scripts::default();
        files.insert("other/module.lua", "return true".into());
        failed(&mut files, "module.lua", ScriptReadError::Read);
        assert_eq!(files.script_result("module"), Err(ScriptReadError::Read));
        assert_eq!(files.script_result("other.module"), Ok(Some("return true".into())));
        failed(&mut files, "uvi.ChordRec.lua", ScriptReadError::AccessRequired);
        let xml = "<Program><EventProcessors><ScriptProcessor><script>require('uvi.ChordRec')</script></ScriptProcessor></EventProcessors></Program>";
        let host = crate::script::ScriptHost::new(xml, files, Default::default()).unwrap();
        assert!(host.findings().iter().any(|f| f.value.contains("requires content access")));
    }

    #[test]
    fn failed_alias_remains_unknown_instead_of_becoming_an_identical_source_alias() {
        let mut files = Scripts::default();
        files.insert("one/module.lua", "return true".into());
        failed(&mut files, "two/module.lua", ScriptReadError::Read);
        assert_eq!(files.script_result("module"), Err(ScriptReadError::Read));
        assert_eq!(files.script_result("one/module"), Ok(Some("return true".into())));
        assert_eq!(files.script_result("missing"), Ok(None));
    }
}
