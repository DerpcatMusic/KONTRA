use crate::{Bytes, Error, ErrorKind, Reader};

/// Borrowed non-monolithic NKS 4.2 framing (format word 0x0110).
/// Header/metadata bytes remain opaque. Checksums are retained, not authenticated;
/// source decoding alone does not admit an instrument for execution.
#[derive(Clone, Copy, Debug)]
pub struct Nks42<'a> {
    pub header: Bytes<'a>,
    pub metadata: Bytes<'a>,
    compressed: Bytes<'a>,
    expanded_bytes: usize,
}

impl<'a> Nks42<'a> {
    pub fn parse(bytes: &'a [u8], max_bytes: usize) -> Result<Self, Error> {
        let source = Bytes {
            data: bytes,
            offset: 0,
        };
        if bytes.len() > max_bytes {
            return Err(source.error(ErrorKind::Limit));
        }
        let mut r = Reader(source);
        let header = r.take(222)?;
        let mut h = Reader(header);
        if !matches!(h.u32()?, 0xb36ee55e | 0x7fa89012 | 0xa4d6e55a | 0x10874353) {
            return Err(header.error(ErrorKind::InvalidMagic));
        }
        let compressed_bytes = h.u32()? as usize;
        let version_at = h.0;
        let version = h.u16()?;
        if version != 0x0110 {
            return Err(version_at.error(ErrorKind::UnsupportedVersion(u32::from(version))));
        }
        let marker_at = h.0;
        if h.u32()? != 0xea37631a {
            return Err(marker_at.error(ErrorKind::InvalidMagic));
        }
        h.take(28)?;
        let monolith_at = h.0;
        if h.u32()? != 0 {
            return Err(monolith_at.error(ErrorKind::UnsupportedLayout));
        }
        h.take(140)?;
        let expanded_bytes = h.u32()? as usize;
        let compressed = r.take(compressed_bytes)?;
        let metadata = r.0;
        if r.u32()? != 0xb00ee1ae {
            return Err(metadata.error(ErrorKind::InvalidMagic));
        }
        r.take(4)?; // Preserve the rest; no claim to interpret the metadata schema.
        Ok(Self {
            header,
            metadata,
            compressed,
            expanded_bytes,
        })
    }
    pub fn compressed(self) -> Bytes<'a> {
        self.compressed
    }
    pub fn expanded_bytes(self) -> usize {
        self.expanded_bytes
    }

    /// Control/worker only. Validate every token, reference and exact output size
    /// BEFORE allocating the destination; a tiny malformed input cannot force a
    /// large claimed-size allocation. The second pass fills one bounded buffer.
    pub fn expand(self, max_bytes: usize) -> Result<Vec<u8>, Error> {
        expand(self.compressed, self.expanded_bytes, max_bytes)
    }
}

pub(crate) fn expand(
    compressed: Bytes<'_>,
    expected: usize,
    max_bytes: usize,
) -> Result<Vec<u8>, Error> {
    if expected > max_bytes {
        return Err(compressed.error(ErrorKind::Limit));
    }
    fastlz(compressed, expected, None)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(expected)
        .map_err(|_| compressed.error(ErrorKind::Allocation))?;
    bytes.resize(expected, 0);
    fastlz(compressed, expected, Some(&mut bytes))?;
    Ok(bytes)
}

// FastLZ levels 1/2 wire rules, reviewed against Ariya Hidayat's MIT-licensed
// reference b1342dabcf5257ab303743c9332fe75e9147a011. See FASTLZ_NOTICE.txt.
// Existing fastlz 0.1.0 uses C with unchecked compressed-input reads; lz77 0.1.0
// silently accepts truncated controls and has no output budget. Neither is used.
fn fastlz(input: Bytes<'_>, expected: usize, mut output: Option<&mut [u8]>) -> Result<(), Error> {
    let mut r = Reader(input);
    let first = r.u8()?;
    let level = first >> 5;
    if level > 1 {
        return Err(input.error(ErrorKind::InvalidCompression));
    }
    let mut control = first & 31;
    let mut at = input;
    let mut produced = 0;
    loop {
        if control < 32 {
            let count = usize::from(control) + 1;
            if count > expected - produced {
                return Err(at.error(ErrorKind::LengthMismatch));
            }
            let literal = r.take(count)?;
            if let Some(out) = &mut output {
                out[produced..produced + count].copy_from_slice(literal.data);
            }
            produced += count;
        } else {
            let mut count = usize::from(control >> 5) + 2;
            if control >> 5 == 7 {
                loop {
                    let extra = r.u8()?;
                    count = count
                        .checked_add(usize::from(extra))
                        .ok_or_else(|| at.error(ErrorKind::LengthMismatch))?;
                    if count > expected - produced {
                        return Err(at.error(ErrorKind::LengthMismatch));
                    }
                    if level == 0 || extra != 255 {
                        break;
                    }
                }
            }
            let low = r.u8()?;
            let mut distance = (usize::from(control & 31) << 8) + usize::from(low) + 1;
            if level == 1 && control & 31 == 31 && low == 255 {
                distance = 8192 + (usize::from(r.u8()?) << 8) + usize::from(r.u8()?);
            }
            if distance > produced {
                return Err(at.error(ErrorKind::InvalidCompression));
            }
            if count > expected - produced {
                return Err(at.error(ErrorKind::LengthMismatch));
            }
            if let Some(out) = &mut output {
                // Forward byte order is required for overlapping dictionary runs.
                for i in 0..count {
                    out[produced + i] = out[produced - distance + i];
                }
            }
            produced += count;
        }
        if r.0.data.is_empty() {
            break;
        }
        at = r.0;
        control = r.u8()?;
    }
    if produced != expected {
        return Err(r.0.error(ErrorKind::LengthMismatch));
    }
    Ok(())
}
