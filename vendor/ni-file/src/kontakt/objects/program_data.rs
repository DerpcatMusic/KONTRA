use crate::{read_bytes::ReadBytesExt, Error};

#[derive(Debug)]
pub struct PrivParsV80(pub super::ProgramDataPrivateParams);

impl PrivParsV80 {
    pub fn read<R: ReadBytesExt>(reader: R) -> Result<Self, Error> {
        Ok(Self(super::ProgramDataPrivateParams::read(reader, 0x80)?))
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use crate::Error;

    use super::*;

    #[test]
    fn test_private_parameters() -> Result<(), Error> {
        // Version 0x80
        let file = File::open("tests/patchdata/KontaktV42/priv_params/4.2.2.4504/000")?;
        PrivParsV80::read(file)?;

        Ok(())
    }
}
