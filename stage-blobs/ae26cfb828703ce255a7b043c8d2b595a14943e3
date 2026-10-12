use crate::AccessError;
use std::path::Path;

/// Feature-off stand-in: the public bank API stays available and refuses access.
pub struct Bank;
impl Bank {
    pub fn open(_: &Path) -> Result<Self, AccessError> {
        Err(AccessError::Disabled)
    }
    pub fn programs(&self) -> Vec<String> {
        Vec::new()
    }
    pub fn program(&self, _: &str) -> Result<(String, String), AccessError> {
        Err(AccessError::Disabled)
    }
    pub fn resource(&self, _: &str, _: &str) -> Result<Vec<Vec<u8>>, AccessError> {
        Err(AccessError::Disabled)
    }
    pub fn decode_resource(
        &self,
        _: &str,
        _: &str,
    ) -> Result<sampler_kontakt::Decoded, AccessError> {
        Err(AccessError::Disabled)
    }
}
