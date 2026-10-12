//! Public access failures contain sanitized diagnostics, never private state.
#[derive(Debug)]
pub enum AccessError {
    Reader(String),
    Bank(String),
    Content(String),
    Program(String),
    Resource(String),
    Audio(String),
    Disabled,
}

impl std::fmt::Display for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (stage, reason) = match self {
            Self::Reader(reason) => ("installed UVI reader", reason.as_str()),
            Self::Bank(reason) => ("UVI bank", reason.as_str()),
            Self::Content(reason) => ("UVI content access", reason.as_str()),
            Self::Program(reason) => ("UVI program", reason.as_str()),
            Self::Resource(reason) => ("UVI resource", reason.as_str()),
            Self::Audio(reason) => ("UVI audio", reason.as_str()),
            Self::Disabled => {
                return f.write_str("UVI bank access needs the library-access feature");
            }
        };
        write!(f, "{stage}: {reason}")
    }
}

impl std::error::Error for AccessError {}
