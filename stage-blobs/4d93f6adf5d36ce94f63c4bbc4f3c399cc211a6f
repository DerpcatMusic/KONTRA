use crate::read_bytes::ReadBytesError;

#[derive(thiserror::Error, Debug)]
pub enum NKSError {
    #[error("{context}: {source}")]
    Context {
        context: String,
        #[source]
        source: Box<NKSError>,
    },
    #[error("Invalid magic number. Expected: 0x7FA89012, 0x5EE56EB3, got: 0x{0:x}")]
    InvalidMagicNumber(u32),

    #[error("Invalid NKS metadata footer magic: expected 0xb00ee1ae, got 0x{0:08x}")]
    InvalidMetadataMagic(u32),

    #[error(transparent)]
    IO(#[from] std::io::Error),

    #[error(transparent)]
    ReadBytesError(#[from] ReadBytesError),

    #[error("Decompression error: {0}")]
    Decompression(String),
}

impl NKSError {
    pub(crate) fn context(context: String, source: impl Into<Self>) -> Self {
        Self::Context { context, source: Box::new(source.into()) }
    }
}
