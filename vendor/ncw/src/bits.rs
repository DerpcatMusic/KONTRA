//! Bit-level primitives shared by the block decoders.

/// Unpack little-endian bit-packed signed integers of `bits` width (1..=32).
pub fn packed_values(data: &[u8], bits: usize) -> PackedValues<'_> {
    debug_assert!((1..=32).contains(&bits));
    PackedValues {
        data: data.iter(),
        bits,
        mask: (1u64 << bits) - 1,
        accumulator: 0,
        available: 0,
    }
}

pub struct PackedValues<'a> {
    data: std::slice::Iter<'a, u8>,
    bits: usize,
    mask: u64,
    accumulator: u64,
    available: usize,
}

impl Iterator for PackedValues<'_> {
    type Item = i32;

    fn next(&mut self) -> Option<i32> {
        while self.available < self.bits {
            let &byte = self.data.next()?;
            self.accumulator |= (byte as u64) << self.available;
            self.available += 8;
        }
        let value = sign_extend((self.accumulator & self.mask) as u32, self.bits);
        self.accumulator >>= self.bits;
        self.available -= self.bits;
        Some(value)
    }
}

/// Bytes of the widest block body plus room for the unaligned word reads of
/// [`unpack_block`].
pub const BLOCK_BYTES: usize = 32 * crate::SAMPLES_PER_BLOCK / 8 + 8;

/// Unpack one block body of `bits`-wide (1..=32) little-endian signed values
/// into `out`. With `base`, the values are deltas: each is replaced by the
/// running sum before it, starting at `base` (wrapping). Each value is read
/// with one unaligned 64-bit load, so there is no per-bit loop; `data` past
/// the body must be zero padding.
pub fn unpack_block(
    data: &[u8; BLOCK_BYTES],
    bits: usize,
    base: Option<i32>,
    out: &mut [i32; crate::SAMPLES_PER_BLOCK],
) {
    debug_assert!((1..=32).contains(&bits));
    // One copy per width: every shift and offset becomes a constant.
    macro_rules! widths {
        ($($b:literal)*) => {
            match (bits, base) {
                $(($b, None) => unpack_fixed::<$b, false>(data, 0, out),)*
                $(($b, Some(base)) => unpack_fixed::<$b, true>(data, base, out),)*
                _ => out.fill(0),
            }
        };
    }
    widths!(1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32)
}

/// [`unpack_block`] at a fixed width: eight values fill exactly `B` bytes.
/// Summing the deltas in the same pass keeps them in registers.
fn unpack_fixed<const B: usize, const DELTA: bool>(
    data: &[u8; BLOCK_BYTES],
    mut current: i32,
    out: &mut [i32; crate::SAMPLES_PER_BLOCK],
) {
    let mask = (1u64 << B) - 1;
    for (group, values) in out.as_chunks_mut::<8>().0.iter_mut().enumerate() {
        for (k, value) in values.iter_mut().enumerate() {
            let bit = k * B;
            // Never clamps (at most 63 * 32 + 28); it drops the bounds check.
            let byte = (group * B + bit / 8).min(BLOCK_BYTES - 8);
            let word = u64::from_le_bytes(data[byte..byte + 8].try_into().unwrap_or_default());
            let v = sign_extend(((word >> (bit % 8)) & mask) as u32, B);
            *value = if DELTA {
                let before = current;
                current = current.wrapping_add(v);
                before
            } else {
                v
            };
        }
    }
}

/// Sign-extend the low `bits` bits of `raw` to an i32.
pub fn sign_extend(raw: u32, bits: usize) -> i32 {
    let shift = 32 - bits as u32;
    ((raw << shift) as i32) >> shift
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unpack(data: &[u8], bits: usize) -> Vec<i32> {
        packed_values(data, bits).collect()
    }

    #[test]
    fn packed_values_sign_extend() {
        // Two 4-bit values: 0x7 and 0xF (-1), then 0x8 (-8) and 0x0.
        assert_eq!(unpack(&[0xF7, 0x08], 4), vec![7, -1, -8, 0]);
    }

    #[test]
    fn packed_values_full_width() {
        assert_eq!(unpack(&(-24i32).to_le_bytes(), 32), vec![-24]);
        assert_eq!(unpack(&(-24i16).to_le_bytes(), 16), vec![-24]);
    }

    #[test]
    fn packed_values_unaligned_width() {
        // 5-bit values 1, 2, 3 packed LSB-first: 0b00011_00010_00001 = 0x0C41
        assert_eq!(unpack(&[0x41, 0x0C], 5), vec![1, 2, 3]);
    }

    #[test]
    fn unpack_block_matches_packed_values() {
        for bits in 1..=32 {
            let len = bits * crate::SAMPLES_PER_BLOCK / 8;
            let mut data = [0u8; BLOCK_BYTES];
            for (i, byte) in data[..len].iter_mut().enumerate() {
                *byte = (i as u32).wrapping_mul(2654435761).rotate_right(13) as u8;
            }
            let expected: Vec<_> = packed_values(&data[..len], bits).collect();
            let mut out = [0; crate::SAMPLES_PER_BLOCK];
            unpack_block(&data, bits, None, &mut out);
            assert_eq!(out.to_vec(), expected, "{bits} bits");

            let mut current = i32::MAX - 5;
            let deltas: Vec<_> = expected
                .iter()
                .map(|&v| {
                    let before = current;
                    current = current.wrapping_add(v);
                    before
                })
                .collect();
            unpack_block(&data, bits, Some(i32::MAX - 5), &mut out);
            assert_eq!(out.to_vec(), deltas, "{bits} bits, delta");
        }
    }

    #[test]
    fn packed_values_ignore_trailing_partial_value() {
        // 3 bits over one byte: two full values, two leftover bits dropped.
        assert_eq!(unpack(&[0b11_010_001], 3), vec![1, 2]);
    }
}
