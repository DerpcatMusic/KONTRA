//! One build identity for plugin descriptors, user interfaces and support reports.

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct BuildInfo {
    pub version: &'static str,
    /// The exact checked-out repository revision; never substituted with an export's upstream SHA.
    pub revision: &'static str,
    pub source_revision: &'static str,
    pub built_at_utc: &'static str,
    pub source_date_epoch: u64,
    pub target: &'static str,
    pub profile: &'static str,
    pub features: &'static [&'static str],
    /// None when source archives have no Git working tree.
    pub dirty: Option<bool>,
    pub build_hash: &'static str,
    pub import_hash: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/build_identity.rs"));

/// Exact manifest produced alongside this binary, suitable for package contents.
pub const MANIFEST_JSON: &str = include_str!(env!("KONTRA_BUILD_MANIFEST"));

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_build_identity_matches_cargo_and_manifest() {
        let manifest: serde_json::Value = serde_json::from_str(super::MANIFEST_JSON).unwrap();
        assert_eq!(manifest, serde_json::to_value(super::BUILD).unwrap());
        assert_eq!(super::BUILD.version, env!("CARGO_PKG_VERSION"));
        assert!(super::SUMMARY.contains(super::BUILD.revision));
        assert!(super::SUMMARY.contains(super::BUILD.built_at_utc));
        #[cfg(feature = "plugin")]
        {
            use moose::core::plugin::PluginRuntime as _;
            assert_eq!(crate::Plugin::info().version, super::BUILD.version);
        }
    }
}
