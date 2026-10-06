use std::io::{self, Read, Seek};

#[derive(thiserror::Error, Debug)]
pub enum ReadBytesError {
    #[error("Generic error: {0}")]
    Generic(String),

    #[error(transparent)]
    IO(#[from] std::io::Error),
}

pub trait FromBytes: Sized {
    fn from_be_bytes(bytes: &[u8]) -> Self;
    fn from_le_bytes(bytes: &[u8]) -> Self;
}

macro_rules! impl_from_bytes {
    ($($t:ty),*) => {
        $(
            impl FromBytes for $t {
                fn from_be_bytes(bytes: &[u8]) -> Self {
                    let mut array = [0u8; std::mem::size_of::<Self>()];
                    array.copy_from_slice(bytes);
                    <$t>::from_be_bytes(array)
                }

                fn from_le_bytes(bytes: &[u8]) -> Self {
                    let mut array = [0u8; std::mem::size_of::<Self>()];
                    array.copy_from_slice(bytes);
                    <$t>::from_le_bytes(array)
                }
            }
        )*
    };
}

impl_from_bytes!(u8, i8, u16, i16, u32, i32, u64, i64, f32, f64);

pub enum Endian {
    LE,
    BE,
}

/// Extensions to io::Read for simplifying reading bytes.
pub trait ReadBytesExt: Read + Seek {
    /// Read a number of bytes (failable)
    fn read_bytes(&mut self, bytes: usize) -> Result<Vec<u8>, ReadBytesError> {
        let position = self.stream_position()?;
        let end = self.seek(std::io::SeekFrom::End(0))?;
        self.seek(std::io::SeekFrom::Start(position))?;
        if bytes as u64 > end.saturating_sub(position) {
            return Err(ReadBytesError::Generic(format!(
                "Read at offset {position}: declared {bytes} bytes, available {} bytes",
                end.saturating_sub(position)
            )));
        }
        let mut buf = Vec::new();
        buf.try_reserve_exact(bytes).map_err(|e| {
            ReadBytesError::Generic(format!("Unable to allocate {bytes} bytes: {e}"))
        })?;
        buf.resize(bytes, 0);
        self.read_exact(&mut buf).map_err(|e| {
            ReadBytesError::IO(io::Error::new(e.kind(), format!("Read at offset {position}: required {bytes} bytes: {e}")))
        })?;
        Ok(buf)
    }

    /// Read stream to end (failable)
    fn read_all(&mut self) -> Result<Vec<u8>, ReadBytesError> {
        let mut compressed_data = Vec::new();
        self.read_to_end(&mut compressed_data)?;
        Ok(compressed_data)
    }

    fn read_endian<T: FromBytes>(&mut self, endian: Endian) -> io::Result<T> {
        let size = std::mem::size_of::<T>();
        let mut stack = [0; 8];
        let mut heap = Vec::new();
        let bytes = if size <= stack.len() {
            &mut stack[..size]
        } else {
            heap.try_reserve_exact(size).map_err(io::Error::other)?;
            heap.resize(size, 0);
            heap.as_mut_slice()
        };
        self.read_exact(bytes).map_err(|e| {
            let at = self.stream_position().ok();
            io::Error::new(e.kind(), format!("Scalar read failed at cursor {at:?}: required {size} bytes: {e}"))
        })?;
        Ok(match endian {
            Endian::LE => T::from_le_bytes(bytes),
            Endian::BE => T::from_be_bytes(bytes),
        })
    }

    /// Read a generic big-endian type
    fn read_be<T: FromBytes>(&mut self) -> io::Result<T> {
        self.read_endian(Endian::BE)
    }

    /// Read a generic little-endian type
    fn read_le<T: FromBytes>(&mut self) -> io::Result<T> {
        self.read_endian(Endian::LE)
    }

    fn read_bool(&mut self) -> io::Result<bool> {
        // TODO: return an error if not 1 or 0
        Ok(ReadBytesExt::read_le::<u8>(self)? == 1)
    }

    fn read_u16_le(&mut self) -> io::Result<u16> {
        ReadBytesExt::read_le::<u16>(self)
    }

    fn read_u8(&mut self) -> io::Result<u8> {
        ReadBytesExt::read_le::<u8>(self)
    }

    fn read_i8(&mut self) -> io::Result<i8> {
        ReadBytesExt::read_le::<i8>(self)
    }

    fn read_u16_be(&mut self) -> io::Result<u16> {
        ReadBytesExt::read_be::<u16>(self)
    }

    fn read_i16_le(&mut self) -> io::Result<i16> {
        ReadBytesExt::read_le::<i16>(self)
    }

    fn read_u32_le(&mut self) -> io::Result<u32> {
        ReadBytesExt::read_le::<u32>(self)
    }

    fn read_i32_be(&mut self) -> io::Result<i32> {
        ReadBytesExt::read_be::<i32>(self)
    }

    fn read_u32_be(&mut self) -> io::Result<u32> {
        ReadBytesExt::read_be::<u32>(self)
    }

    fn read_i32_le(&mut self) -> io::Result<i32> {
        ReadBytesExt::read_le::<i32>(self)
    }

    fn read_f32_le(&mut self) -> io::Result<f32> {
        ReadBytesExt::read_le::<f32>(self)
    }

    fn read_f64_le(&mut self) -> io::Result<f64> {
        ReadBytesExt::read_le::<f64>(self)
    }

    fn read_u64_le(&mut self) -> io::Result<u64> {
        ReadBytesExt::read_le::<u64>(self)
    }

    fn read_u64_be(&mut self) -> io::Result<u64> {
        ReadBytesExt::read_be::<u64>(self)
    }

    fn read_string_utf8(&mut self) -> io::Result<String> {
        let mut bytes = Vec::new();
        loop {
            let mut byte = [0];
            self.read_exact(&mut byte)?;
            match byte {
                [0] => break,
                _ => bytes.push(byte[0]),
            }
        }
        String::from_utf8(bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.utf8_error()))
    }

    fn read_optional_sized_utf8(&mut self) -> Result<Option<String>, ReadBytesError> {
        let length = self.read_u32_le()?;

        if length == 0xFFFFFFFF {
            return Ok(None);
        }

        if length == 0 {
            return Ok(Some(String::new()));
        }

        let bytes = self.read_bytes(length as usize)?;

        String::from_utf8(bytes)
            .map_err(|e| ReadBytesError::Generic(format!("Error converting bytes to UTF8: {e}")))
            .map(Some)
    }

    fn read_sized_utf8(&mut self) -> Result<String, ReadBytesError> {
        let size_field = self.read_u32_le()?;
        if size_field == 0 {
            return Ok(String::new());
        }
        let bytes = self.read_bytes(size_field as usize)?;

        String::from_utf8(bytes)
            .map_err(|e| ReadBytesError::Generic(format!("Error converting bytes to UTF8: {e}")))
    }

    fn read_widestring_utf16(&mut self) -> Result<String, ReadBytesError> {
        let size_field = self.read_u32_le()?;
        if size_field == 0 {
            return Ok(String::new());
        }

        let buf = self.read_bytes(size_field as usize * 2)?;

        let bytes: Vec<u16> = buf
            .as_chunks::<2>().0.iter()
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();

        // String::from_utf16(bytes.as_slice())
        //     .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))

        String::from_utf16(bytes.as_slice()).map_err(|e| {
            ReadBytesError::Generic(format!("Error converting bytes to UTF16: {e}, {bytes:?}"))
        })
    }
}
impl<R: Read + Seek + ?Sized> ReadBytesExt for R {}

#[cfg(test)]
mod tests {
    use super::{FromBytes, ReadBytesExt};
    use std::io;

    #[test]
    fn scalar_endianness_widths_custom_types_and_truncation() {
        macro_rules! check {
            ($t:ty, $value:expr) => {
                let value: $t = $value;
                let le = value.to_le_bytes();
                let be = value.to_be_bytes();
                assert_eq!(
                    ReadBytesExt::read_le::<$t>(&mut io::Cursor::new(le)).unwrap(),
                    value
                );
                assert_eq!(
                    ReadBytesExt::read_be::<$t>(&mut io::Cursor::new(be)).unwrap(),
                    value
                );
                assert!(
                    ReadBytesExt::read_le::<$t>(&mut io::Cursor::new(&le[..le.len() - 1])).is_err()
                );
                assert!(
                    ReadBytesExt::read_be::<$t>(&mut io::Cursor::new(&be[..be.len() - 1])).is_err()
                );
            };
        }
        check!(u8, 0xa5);
        check!(i8, -12);
        check!(u16, 0xabcd);
        check!(i16, -1234);
        check!(u32, 0xabcdef12);
        check!(i32, -1234567);
        check!(u64, 0xabcdef1234567890);
        check!(i64, -1234567890123);
        check!(f32, 1.25);
        check!(f64, -123.5);
        assert_eq!(
            io::Cursor::new(0x12345678u32.to_be_bytes())
                .read_u32_be()
                .unwrap(),
            0x12345678
        );
        assert_eq!(
            io::Cursor::new((-1234567i32).to_be_bytes())
                .read_i32_be()
                .unwrap(),
            -1234567
        );
        #[derive(Debug, PartialEq)]
        struct Wide([u8; 16]);
        impl FromBytes for Wide {
            fn from_le_bytes(bytes: &[u8]) -> Self {
                Self(bytes.try_into().unwrap())
            }
            fn from_be_bytes(bytes: &[u8]) -> Self {
                let mut bytes: [u8; 16] = bytes.try_into().unwrap();
                bytes.reverse();
                Self(bytes)
            }
        }
        let bytes = std::array::from_fn::<_, 16, _>(|i| i as u8);
        assert_eq!(
            ReadBytesExt::read_le::<Wide>(&mut io::Cursor::new(bytes)).unwrap(),
            Wide(bytes)
        );
        let mut reversed = bytes;
        reversed.reverse();
        assert_eq!(
            ReadBytesExt::read_be::<Wide>(&mut io::Cursor::new(bytes)).unwrap(),
            Wide(reversed)
        );
        assert!(io::Cursor::new([0; 3]).read_bytes(4).is_err());
    }

    #[test]
    fn test_read_u32_le() {
        let bytes: &[u8] = &[32_u8, 1, 4, 56, 6, 6, 90, 4, 7];
        let mut cursor = io::Cursor::new(bytes);

        let num = cursor.read_u32_le().unwrap();
        assert_eq!(num, 939786528);

        let num = cursor.read_u32_le().unwrap();
        assert_eq!(num, 73008646);
    }

    // #[test]
    // fn test_read_sized_data() {
    //     let bytes: &[u8] = &[9, 0, 0, 0, 0, 0, 0, 0, 4, 5];
    //     let mut cursor = io::Cursor::new(bytes);
    //     let content = cursor.read_sized_data().unwrap();
    //
    //     assert_eq!(content, [9, 0, 0, 0, 0, 0, 0, 0, 4]);
    //     assert_eq!(bytes, [5]);
    //
    //     // test two
    //     let bytes = [
    //         12_u64.to_le_bytes().to_vec(),
    //         64_u32.to_le_bytes().to_vec(),
    //         24_u32.to_le_bytes().to_vec(),
    //     ]
    //     .concat();
    //     assert_eq!(
    //         io::Cursor::new(bytes).read_sized_data().unwrap(),
    //         [12_u64.to_le_bytes().to_vec(), 64_u32.to_le_bytes().to_vec()].concat()
    //     );
    // }
}
