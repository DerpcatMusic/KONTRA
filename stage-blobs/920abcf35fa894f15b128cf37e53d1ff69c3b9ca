use flate2::bufread::ZlibDecoder;
use std::fmt::Display;
use std::io::Read;

use crate::nks::error::NKSError;

#[derive(Debug)]
pub struct XMLDocument(String);

impl XMLDocument {
    pub fn from_utf8(data: &[u8]) -> Result<Self, std::string::FromUtf8Error> {
        Ok(Self(String::from_utf8(data.to_vec())?))
    }

    pub fn from_compressed_data(data: &[u8]) -> Result<Self, NKSError> {
        let decoder = ZlibDecoder::new(data);
        let mut decompressed = Vec::new();
        decoder
            .take((128 << 20) + 1)
            .read_to_end(&mut decompressed)?;
        if decompressed.len() > 128 << 20 {
            return Err(NKSError::Decompression(
                "Expanded Kontakt XML exceeds decode limit".into(),
            ));
        }

        // let decompressed = miniz_oxide::inflate::decompress_to_vec(data).expect("decompress xml");

        Ok(XMLDocument(String::from_utf8(decompressed).map_err(
            |_| NKSError::Decompression("Invalid Kontakt XML UTF-8".into()),
        )?))
    }
}

impl Display for XMLDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}", self.0))
    }
}
