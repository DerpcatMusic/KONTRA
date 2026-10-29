//! UVI file decoders: UFS soundbank containers, their member encryption,
//! and the program XML inside them. No playback, no engine types.

pub mod access;
pub mod access_error;
pub mod crypto;
pub mod ufs;

pub use access_error::AccessError;

/// Largest program XML accepted (Falcon programs exceed 200,000 nodes).
pub const XML_LIMIT: u64 = 32 << 20;

/// Parses decoded program XML with bounded size and node count and no DTDs.
///
/// # Errors
/// When the text is over [`XML_LIMIT`] or not well-formed XML.
pub fn parse_program_xml(text: &str) -> anyhow::Result<roxmltree::Document<'_>> {
    anyhow::ensure!(text.len() as u64 <= XML_LIMIT, "program exceeds 32 MiB");
    Ok(roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: 1_000_000,
            ..Default::default()
        },
    )?)
}
