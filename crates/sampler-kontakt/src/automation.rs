//! Program-private automation framing verified in Kontakt 8.13.1.
use crate::{Bytes, Error, ErrorKind, Limits, Reader};
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AutomationRecord<'a> {
    pub offset: usize,
    pub version: u16,
    pub mode: u32,
    pub soft_takeover: bool,
    pub address: u16,
    pub id: i32,
    pub secondary_id: Option<i32>,
    pub low: f32,
    pub high: f32,
    pub tag: Bytes<'a>,
}
pub fn program_automation(
    bytes: &[u8],
    version: u16,
    limits: Limits,
) -> Result<Vec<AutomationRecord<'_>>, Error> {
    let raw = Bytes {
        data: bytes,
        offset: 0,
    };
    let legacy = match version {
        0x91 | 0x92 | 0xa0..=0xa8 => true,
        0xa9..=0xb5 => false,
        _ => return Err(raw.error(ErrorKind::UnsupportedVersion(u32::from(version)))),
    };
    if bytes.len() > limits.bytes {
        return Err(raw.error(ErrorKind::Limit));
    }
    let mut r = Reader(raw);
    r.take(61)?;
    for array in 0..2 {
        let count = bounded_count(&mut r, limits.records)?;
        for _ in 0..count {
            header(&mut r, &[0x50])?;
            if array == 0 {
                r.i32()?;
                wide(&mut r)?;
                r.take(15)?;
            } else {
                let at = r.0;
                if !matches!(r.i32()?, 1 | 2) {
                    return Err(at.error(ErrorKind::UnsupportedLayout));
                }
                r.i32()?;
                wide(&mut r)?;
                let count = bounded_count(&mut r, 64)?;
                r.take(
                    count
                        .checked_mul(4)
                        .ok_or_else(|| r.0.error(ErrorKind::Limit))?,
                )?;
            }
        }
    }
    let outer = bounded_count(&mut r, limits.records)?;
    if outer == 0 {
        return Ok(Vec::new());
    }
    let inner = bounded_count(&mut r, limits.records)?;
    let count = if legacy { inner.min(outer) } else { inner };
    let mut records = Vec::new();
    records
        .try_reserve_exact(count)
        .map_err(|_| r.0.error(ErrorKind::Allocation))?;
    for _ in 0..count {
        let offset = r.0.offset;
        let version = header(&mut r, &[0x70, 0x71])?;
        let at = r.0;
        let mode = r.u32()?;
        let soft_takeover = match r.u8()? {
            0 => false,
            1 => true,
            _ => return Err(at.error(ErrorKind::InvalidBoolean)),
        };
        if version == 0x70 {
            r.u8()?;
        }
        let address = r.u16()?;
        let id = r.i32()?;
        let secondary_id = if version == 0x71 {
            Some(r.i32()?)
        } else {
            None
        };
        let low = r.f32()?;
        let high = r.f32()?;
        if !low.is_finite() || !high.is_finite() {
            return Err(at.error(ErrorKind::UnsupportedLayout));
        }
        let len = r.u32()? as usize;
        let tag = r.take(len)?;
        records.push(AutomationRecord {
            offset,
            version,
            mode,
            soft_takeover,
            address,
            id,
            secondary_id,
            low,
            high,
            tag,
        });
    }
    // Later Program-private fields belong to other readers; do not consume them.
    Ok(records)
}
fn bounded_count(r: &mut Reader<'_>, limit: usize) -> Result<usize, Error> {
    let at = r.0;
    let count = r.u32()? as usize;
    if count > limit {
        return Err(at.error(ErrorKind::Limit));
    }
    Ok(count)
}
fn header(r: &mut Reader<'_>, accepted: &[u16]) -> Result<u16, Error> {
    let at = r.0;
    if r.u8()? != 0 {
        return Err(at.error(ErrorKind::UnsupportedLayout));
    }
    let version = r.u16()?;
    if !accepted.contains(&version) {
        return Err(at.error(ErrorKind::UnsupportedVersion(u32::from(version))));
    }
    Ok(version)
}
fn wide(r: &mut Reader<'_>) -> Result<(), Error> {
    let at = r.0;
    let units = r.u32()? as usize;
    r.take(
        units
            .checked_mul(2)
            .ok_or_else(|| at.error(ErrorKind::Limit))?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn u32v(b: &mut Vec<u8>, n: u32) {
        b.extend(n.to_le_bytes());
    }
    fn wide(b: &mut Vec<u8>, s: &str) {
        let units: Vec<_> = s.encode_utf16().collect();
        u32v(b, units.len() as u32);
        for u in units {
            b.extend(u.to_le_bytes());
        }
    }
    fn fixture() -> (Vec<u8>, usize) {
        let mut b = vec![0; 61];
        u32v(&mut b, 1);
        b.extend([0, 0x50, 0]);
        u32v(&mut b, 7);
        wide(&mut b, "A🦀");
        b.extend([0; 15]); // three scalars, u8, u16
        u32v(&mut b, 1);
        b.extend([0, 0x50, 0]);
        u32v(&mut b, 2);
        u32v(&mut b, 9);
        wide(&mut b, "Bé");
        u32v(&mut b, 2);
        u32v(&mut b, 3);
        u32v(&mut b, 4);
        u32v(&mut b, 1);
        u32v(&mut b, 1);
        let at = b.len();
        record(&mut b, 0x71);
        (b, at)
    }
    fn record(b: &mut Vec<u8>, version: u16) {
        b.push(0);
        b.extend(version.to_le_bytes());
        u32v(b, 1);
        b.push(0);
        if version == 0x70 {
            b.push(0);
        }
        b.extend(21u16.to_le_bytes());
        u32v(b, 0);
        if version == 0x71 {
            u32v(b, u32::MAX);
        }
        b.extend(0f32.to_le_bytes());
        b.extend(1f32.to_le_bytes());
        let tag = b"pts_script_slider_3_2";
        u32v(b, tag.len() as u32);
        b.extend(tag);
    }
    const LIMITS: Limits = Limits {
        bytes: 1024,
        records: 64,
    };
    #[test]
    fn nonempty_utf16_arrays_preserve_exact_automation_offset() {
        let (b, at) = fixture();
        assert_eq!(at, 140);
        let r = program_automation(&b, 0xae, LIMITS).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].offset, 140);
        assert_eq!(r[0].address, 21);
        assert_eq!(r[0].tag.data(), b"pts_script_slider_3_2");
        for cut in 0..b.len() {
            assert!(program_automation(&b[..cut], 0xae, LIMITS).is_err());
        }
    }
    #[test]
    fn legacy_capacity_and_versions_are_explicit() {
        let mut b = vec![0; 61];
        u32v(&mut b, 0);
        u32v(&mut b, 0);
        u32v(&mut b, 1);
        u32v(&mut b, 2);
        record(&mut b, 0x70);
        record(&mut b, 0x71);
        assert_eq!(program_automation(&b, 0x91, LIMITS).unwrap().len(), 1);
        assert_eq!(program_automation(&b, 0xa9, LIMITS).unwrap().len(), 2);
        for version in [0x80, 0x82, 0x90, 0x93, 0x9f, 0xb6] {
            assert_eq!(
                program_automation(&b, version, LIMITS).unwrap_err().kind,
                ErrorKind::UnsupportedVersion(u32::from(version))
            );
        }
    }
    #[test]
    fn invalid_array_headers_and_large_b_array_are_rejected() {
        let (mut b, _) = fixture();
        b[65] = 1;
        assert_eq!(program_automation(&b, 0xae, LIMITS).unwrap_err().offset, 65);
        let (mut b, _) = fixture();
        b[66] = 0x51;
        assert!(program_automation(&b, 0xae, LIMITS).is_err());
        let mut b = vec![0; 61];
        u32v(&mut b, 0);
        u32v(&mut b, 1);
        b.extend([0, 0x50, 0]);
        u32v(&mut b, 1);
        u32v(&mut b, 0);
        wide(&mut b, "");
        u32v(&mut b, 65);
        assert_eq!(
            program_automation(&b, 0xae, LIMITS).unwrap_err().kind,
            ErrorKind::Limit
        );
    }
}
