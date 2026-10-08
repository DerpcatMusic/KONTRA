//! Port from v1 0cb7a8a0:src/audio.rs: predictive preload blocks, with safe scalar reads.
use super::I16 as I16_SCALE;
use crate::Frame;
use std::mem::size_of_val;

/// Frames per [`Packed`] block.
const BLOCK: usize = 64;
/// Block header: two bit widths, then per channel the first two samples as
/// 24-bit integers.
const HEADER: usize = 2 + 4 * 3;

/// Lossless 16/24-bit PCM in independently decodable blocks of [`BLOCK`]
/// frames. Per channel, each sample is predicted from the two before it
/// (`2x[n-1] - x[n-2]`) and the residuals are bit-packed at the block's
/// widest: about two thirds of the raw size on orchestral recordings, so
/// preloads and sample-start ranges take a third less RAM. Decoding a block
/// is one sequential pass; voices read resident data only until streaming
/// takes over.
#[derive(Clone, Debug)]
pub(super) struct Compressed {
    frames: usize,
    /// Integer full scale: 2^15 or 2^23.
    scale: f32,
    /// Start of each block in `data`.
    offsets: Box<[u32]>,
    /// Blocks, then eight zero bytes so every bit read is one unaligned u64 load.
    data: Box<[u8]>,
}

impl Compressed {
    /// Pack `frames` if `quantize` turns every block into integers at
    /// `scale` (returning false if it cannot) and packing saves at least a
    /// tenth.
    #[inline(always)]
    pub(super) fn new(frames: &[Frame], scale: f32) -> Option<Self> {
        Self::encode(frames, scale, quantize(scale))
    }

    fn encode<T: Copy>(
        frames: &[[T; 2]],
        scale: f32,
        quantize: impl for<'a> Fn(
            &'a [[T; 2]; BLOCK],
            &'a mut [i32; 2 * BLOCK],
        ) -> Option<&'a [i32; 2 * BLOCK]>,
    ) -> Option<Self> {
        let raw = frames.len() * if scale == I16_SCALE { 4 } else { 6 };
        let blocks = frames.len().div_ceil(BLOCK);
        if blocks == 0 || raw > u32::MAX as usize {
            return None;
        }
        let mut data = Vec::with_capacity(raw * 3 / 4);
        let mut offsets = Vec::with_capacity(blocks);
        // Interleaved throughout: both channels in the same pass, which
        // vectorizes.
        let mut scratch = [0i32; 2 * BLOCK];
        let mut pad: [[T; 2]; BLOCK];
        for chunk in frames.chunks(BLOCK) {
            offsets.push(data.len() as u32);
            // The last block repeats its final frame: cheap to pack.
            let src: &[[T; 2]; BLOCK] = match chunk.try_into() {
                Ok(full) => full,
                Err(_) => {
                    pad = [chunk[chunk.len() - 1]; BLOCK];
                    pad[..chunk.len()].copy_from_slice(chunk);
                    &pad
                }
            };
            let q = quantize(src, &mut scratch)?;
            // Per channel, two zero fields round it up to whole groups of eight.
            let mut residuals = [[0i32; BLOCK]; 2];
            let mut magnitude = [0u32; 2];
            for (i, q) in q.as_chunks::<2>().0.windows(3).enumerate() {
                for c in 0..2 {
                    let r = q[2][c]
                        .wrapping_sub(q[1][c].wrapping_mul(2))
                        .wrapping_add(q[0][c]);
                    residuals[c][i] = r;
                    magnitude[c] |= (r ^ (r >> 31)) as u32;
                }
            }
            let widths = magnitude.map(|m| {
                if m == 0 {
                    0
                } else {
                    33 - m.leading_zeros() as u8
                }
            });
            data.extend(widths);
            for c in 0..2 {
                data.extend(&q[c].to_le_bytes()[..3]);
                data.extend(&q[2 + c].to_le_bytes()[..3]);
            }
            for (channel, &w) in residuals.iter().zip(&widths) {
                let end = data.len() + ((BLOCK - 2) * usize::from(w)).div_ceil(8);
                pack_fields(channel, w, &mut data);
                data.truncate(end);
            }
            if data.len() + 8 >= raw * 9 / 10 {
                return None;
            }
        }
        data.extend([0; 8]);
        Some(Self {
            frames: frames.len(),
            scale,
            offsets: offsets.into(),
            data: data.into(),
        })
    }

    pub(super) fn bytes(&self) -> usize {
        size_of_val(&*self.offsets) + size_of_val(&*self.data)
    }

    /// Decode frames `[at, at + out.len())`; false if out of range.
    #[inline(always)]
    pub(super) fn decode(&self, at: usize, out: &mut [Frame]) -> bool {
        if at + out.len() > self.frames {
            return false;
        }
        let mut block = [[0i32; BLOCK]; 2];
        let (mut done, gain) = (0, 1.0 / self.scale);
        while done < out.len() {
            let frame = at + done;
            let (index, skip) = (frame / BLOCK, frame % BLOCK);
            let n = (BLOCK - skip).min(out.len() - done);
            self.block(index, skip + n, &mut block);
            for (i, o) in out[done..done + n].iter_mut().enumerate() {
                *o = [
                    block[0][skip + i] as f32 * gain,
                    block[1][skip + i] as f32 * gain,
                ];
            }
            done += n;
        }
        true
    }

    /// Decode frames `..len` of block `index` into `out`.
    #[inline(always)]
    fn block(&self, index: usize, len: usize, out: &mut [[i32; BLOCK]; 2]) {
        let data = &self.data[self.offsets[index] as usize..];
        let word = |at: usize| i32::from_le_bytes([0, data[at], data[at + 1], data[at + 2]]) >> 8;
        let mut at = HEADER;
        let n = len.max(2) - 2;
        for (c, x) in out.iter_mut().enumerate() {
            let w = data[c];
            let bits = &data[at..];
            at += ((BLOCK - 2) * usize::from(w)).div_ceil(8);
            let before = [word(2 + 6 * c), word(5 + 6 * c)];
            (x[0], x[1]) = (before[0], before[1]);
            integrate(bits, w, before, &mut x[2..2 + n]);
        }
    }
}

/// Unpack `out.len()` residuals of `w` bits from `bits` and integrate them
/// twice onto the two samples `before`.
#[inline(always)]
fn integrate(bits: &[u8], w: u8, [first, mut prev]: [i32; 2], out: &mut [i32]) {
    let w = u32::from(w);
    // A `w`-bit field sign-extends by shifting it to the top and back.
    let shift = 64 - w.max(1);
    let keep = if w == 0 { 0 } else { u64::MAX };
    // Every field's eight-byte read stays in bounds: blocks are followed by
    // the next block or the zero tail.
    assert!((out.len().saturating_sub(1) * w as usize) / 8 + 8 <= bits.len());
    let mut delta = prev.wrapping_sub(first);
    for (i, y) in out.iter_mut().enumerate() {
        let bit = i * w as usize;
        let at = bit / 8;
        let raw = u64::from_le_bytes(bits[at..at + 8].try_into().unwrap());
        let r = ((((raw >> (bit % 8)) << shift) as i64 >> shift) as u64 & keep) as i32;
        delta = delta.wrapping_add(r);
        prev = prev.wrapping_add(delta);
        *y = prev;
    }
}

/// Append `fields` as little-endian `width`-bit fields (0..=33), eight to
/// every `width` bytes.
fn pack_fields(fields: &[i32; BLOCK], width: u8, out: &mut Vec<u8>) {
    // One copy per width: every shift and offset becomes a constant, and the
    // eight fields of a group combine independently, not through one
    // serial accumulator.
    macro_rules! widths {
        ($($w:literal)*) => {
            match width {
                $($w => pack_fixed::<$w>(fields, out),)*
                _ => {}
            }
        };
    }
    widths!(1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 33)
}

/// [`pack_fields`] at a fixed width.
fn pack_fixed<const W: usize>(fields: &[i32; BLOCK], out: &mut Vec<u8>) {
    let mask = (1u64 << W) - 1;
    // Groups land in a stack buffer and append once: a length check per
    // block rather than per group.
    let mut packed = [0u8; BLOCK / 8 * 33];
    let groups = fields
        .as_chunks::<8>()
        .0
        .iter()
        .zip(packed.chunks_exact_mut(W));
    for (group, out) in groups {
        let mut words = [0u64; 5];
        for (k, &v) in group.iter().enumerate() {
            let (v, bit) = (v as u64 & mask, k * W);
            words[bit / 64] |= v << (bit % 64);
            if bit % 64 + W > 64 {
                words[bit / 64 + 1] |= v >> (64 - bit % 64);
            }
        }
        let mut bytes = [0u8; 40];
        for (b, w) in bytes.as_chunks_mut::<8>().0.iter_mut().zip(words) {
            *b = w.to_le_bytes();
        }
        out.copy_from_slice(&bytes[..W]);
    }
    out.extend_from_slice(&packed[..BLOCK / 8 * W]);
}

/// A block of float frames as integers at `scale` in `q`; `None` if any is
/// not exactly one in `-scale..scale`.
fn quantize(
    scale: f32,
) -> impl for<'a> Fn(&'a [Frame; BLOCK], &'a mut [i32; 2 * BLOCK]) -> Option<&'a [i32; 2 * BLOCK]> {
    // In f64, adding 2^52 + 2^51 leaves an integer exactly and its two's
    // complement in the low bits, and rounds away any fraction: an
    // exactness test and conversion that vectorize, unlike `as`.
    const ROUND: f64 = 6755399441055744.0;
    let scale = f64::from(scale);
    move |src, q| {
        let mut inexact = false;
        for (q, &x) in q.iter_mut().zip(src.as_flattened()) {
            let (x, t) = (f64::from(x) * scale, f64::from(x) * scale + ROUND);
            *q = t.to_bits() as i32;
            inexact |= !((t - ROUND == x) & (x >= -scale) & (x < scale));
        }
        (!inexact).then_some(q)
    }
}
