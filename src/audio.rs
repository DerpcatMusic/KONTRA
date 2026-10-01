//! Sample decoding with random access by frame.
//!
//! Every format goes through [`SampleReader`], so the loader decodes only the
//! resident spans of a sample and the streamer fetches the rest on demand.
//! Archive members are decrypted in place while reading: the NKX resource
//! cipher is position-relative, so any byte offset is addressable.

use anyhow::{Context, Result, bail, ensure};
use ni_file::{
    nis::LibraryKey,
    nkr::Archive,
};
use std::{
    collections::HashMap,
    ffi::OsString,
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{self, DecoderOptions},
    errors::Error as SymphoniaError,
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo},
    io::{MediaSource, MediaSourceStream},
    meta::MetadataOptions,
    probe::Hint,
};

/// Return freed heap memory to the system. Loading allocates and frees far
/// more than it keeps (decompressed presets, parse trees, decode buffers on
/// every core), and glibc keeps freed pages of its per-thread arenas
/// resident; call once a load finished. A no-op elsewhere.
pub fn trim_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        // SAFETY: glibc's malloc_trim is thread-safe and has no preconditions.
        unsafe { malloc_trim(0) };
    }
}

/// One stereo frame. Mono sources are duplicated to both channels.
pub type Frame = [f32; 2];

/// Resident frames in the narrowest format that holds them exactly: 16- and
/// 24-bit sources keep their own resolution (4 or 6 bytes per stereo frame
/// instead of 8), losslessly [`Packed`] when that is clearly smaller, and
/// the voice kernel converts on the fly. Samples are interleaved left/right.
pub enum Pcm {
    F32(Box<[Frame]>),
    I16(Box<[i16]>),
    /// The top 16 bits and, in their own plane, the low byte: decoding stays
    /// an elementwise pass the compiler vectorizes.
    I24(Box<[i16]>, Box<[u8]>),
    Packed(Packed),
}

const I16_SCALE: f32 = 32768.0;
const I24_SCALE: f32 = 8388608.0;

impl Pcm {
    /// Bytes per frame [`Pcm::pack`] uses at most for a source of `bits`
    /// resolution (32-bit and float sources stay f32).
    pub fn frame_bytes(bits: Option<u16>) -> usize {
        match bits {
            Some(..=16) => 4,
            Some(..=24) => 6,
            _ => 8,
        }
    }

    /// Store `frames` in the narrowest exact format; `compress` allows
    /// [`Packed`], which costs more to decode (for data voices loop in
    /// indefinitely, leave it off).
    pub fn pack(frames: &[Frame], compress: bool) -> Self {
        let samples = frames.as_flattened();
        // Chunked so the check vectorizes yet fails fast on wider data.
        let exact = |scale: f32| {
            samples
                .chunks(256)
                .all(|c| c.iter().fold(true, |ok, &x| ok & exact(x, scale)))
        };
        // Encoding checks exactness itself and gives up at the first
        // inexact block, so no separate scan precedes it.
        for scale in [I16_SCALE, I24_SCALE] {
            if compress && let Some(packed) = Packed::encode(frames, scale, quantize(scale)) {
                return Self::Packed(packed);
            }
        }
        if exact(I16_SCALE) {
            return Self::I16(samples.iter().map(|&x| (x * I16_SCALE) as i16).collect());
        }
        if exact(I24_SCALE) {
            let q = |x: f32| (x * I24_SCALE) as i32;
            return Self::I24(
                samples.iter().map(|&x| (q(x) >> 8) as i16).collect(),
                samples.iter().map(|&x| q(x) as u8).collect(),
            );
        }
        Self::F32(frames.into())
    }

    /// [`Pcm::pack`] for integer frames of a `bits`-deep source, skipping
    /// the float round trip. `None` unless 16- or 24-bit and in range.
    /// Uses an AVX2 build when the CPU has it.
    pub fn pack_ints(frames: &[[i32; 2]], bits: u16, compress: bool) -> Option<Self> {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the running CPU supports AVX2.
            return unsafe { Self::pack_ints_avx2(frames, bits, compress) };
        }
        Self::pack_ints_body(frames, bits, compress)
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    fn pack_ints_avx2(frames: &[[i32; 2]], bits: u16, compress: bool) -> Option<Self> {
        Self::pack_ints_body(frames, bits, compress)
    }

    #[inline(always)]
    fn pack_ints_body(frames: &[[i32; 2]], bits: u16, compress: bool) -> Option<Self> {
        let scale = match bits {
            16 => I16_SCALE,
            24 => I24_SCALE,
            _ => return None,
        };
        let limit = scale as i32;
        let (lo, hi) = frames.as_flattened().iter().fold((0, 0), |(lo, hi), &x| (x.min(lo), x.max(hi)));
        if lo < -limit || hi >= limit {
            return None;
        }
        if compress && let Some(packed) = Packed::encode(frames, scale, in_place) {
            return Some(Self::Packed(packed));
        }
        let samples = frames.as_flattened();
        Some(if bits == 16 {
            Self::I16(samples.iter().map(|&x| x as i16).collect())
        } else {
            Self::I24(
                samples.iter().map(|&x| (x >> 8) as i16).collect(),
                samples.iter().map(|&x| x as u8).collect(),
            )
        })
    }

    pub fn len(&self) -> usize {
        match self {
            Self::F32(d) => d.len(),
            Self::I16(d) | Self::I24(d, _) => d.len() / 2,
            Self::Packed(p) => p.frames,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn bytes(&self) -> usize {
        match self {
            Self::F32(d) => size_of_val(&**d),
            Self::I16(d) => size_of_val(&**d),
            Self::I24(high, low) => size_of_val(&**high) + size_of_val(&**low),
            Self::Packed(p) => p.bytes(),
        }
    }

    /// Frames `[at, at + buf.len())`: borrowed when stored as f32, otherwise
    /// decoded into `buf`. `None` if out of range.
    #[inline]
    pub fn window<'a>(&'a self, at: usize, buf: &'a mut [Frame]) -> Option<&'a [Frame]> {
        match self {
            Self::F32(d) => d.get(at..at + buf.len()),
            _ => self.decode(at, buf).then_some(buf),
        }
    }

    /// Decode frames `[at, at + out.len())` into `out`; false if out of range.
    /// Uses an AVX2 build when the CPU has it.
    #[inline]
    pub fn decode(&self, at: usize, out: &mut [Frame]) -> bool {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the running CPU supports AVX2.
            return unsafe { self.decode_avx2(at, out) };
        }
        self.decode_body(at, out)
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    fn decode_avx2(&self, at: usize, out: &mut [Frame]) -> bool {
        self.decode_body(at, out)
    }

    /// Add frames `[at, at + acc.len())`, times `weights` per channel, to
    /// `acc` without decoding them to memory first; false (and `acc`
    /// untouched) if out of range. The same sums as decoding, then
    /// weighting: the integer scales are powers of two.
    #[inline]
    pub fn accumulate(&self, at: usize, weights: [f32; 2], acc: &mut [Frame]) -> bool {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the running CPU supports AVX2.
            return unsafe { self.accumulate_avx2(at, weights, acc) };
        }
        self.accumulate_body(at, weights, acc)
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    fn accumulate_avx2(&self, at: usize, weights: [f32; 2], acc: &mut [Frame]) -> bool {
        use std::arch::x86_64::*;
        let Self::I16(d) = self else {
            return self.accumulate_body(at, weights, acc);
        };
        let Some(xs) = d.get(2 * at..2 * (at + acc.len())) else {
            return false;
        };
        // Eight frames a pass: sixteen samples widened to i32, then to f32,
        // multiplied, then added (never fused, as the decoded path does).
        let w = weights.map(|w| w * (1.0 / I16_SCALE));
        let (chunks, tail) = acc.as_flattened_mut().as_chunks_mut::<16>();
        let (xchunks, xtail) = xs.as_chunks::<16>();
        // SAFETY: AVX2 is enabled; every load and store stays within a chunk.
        unsafe {
            let wv = _mm256_setr_ps(w[0], w[1], w[0], w[1], w[0], w[1], w[0], w[1]);
            for (a, x) in chunks.iter_mut().zip(xchunks) {
                let x = _mm256_loadu_si256(x.as_ptr().cast());
                let lo = _mm256_cvtepi32_ps(_mm256_cvtepi16_epi32(_mm256_castsi256_si128(x)));
                let hi = _mm256_cvtepi32_ps(_mm256_cvtepi16_epi32(_mm256_extracti128_si256(x, 1)));
                let p = a.as_mut_ptr();
                _mm256_storeu_ps(p, _mm256_add_ps(_mm256_loadu_ps(p), _mm256_mul_ps(wv, lo)));
                _mm256_storeu_ps(p.add(8), _mm256_add_ps(_mm256_loadu_ps(p.add(8)), _mm256_mul_ps(wv, hi)));
            }
        }
        for (k, (a, &x)) in tail.iter_mut().zip(xtail).enumerate() {
            *a += w[k & 1] * f32::from(x);
        }
        true
    }

    #[inline(always)]
    fn accumulate_body(&self, at: usize, weights: [f32; 2], acc: &mut [Frame]) -> bool {
        if at + acc.len() > self.len() {
            return false;
        }
        let samples = 2 * at..2 * (at + acc.len());
        // Sixteen samples (eight frames) a pass, weights alternating by channel.
        #[inline(always)]
        fn add<T: Copy>(acc: &mut [Frame], xs: &[T], w: [f32; 2], f: impl Fn(T) -> f32) {
            let w: [f32; 16] = std::array::from_fn(|k| w[k & 1]);
            let (chunks, tail) = acc.as_flattened_mut().as_chunks_mut::<16>();
            let (xs, xtail) = xs.as_chunks::<16>();
            for (a, x) in chunks.iter_mut().zip(xs) {
                for k in 0..16 {
                    a[k] += w[k] * f(x[k]);
                }
            }
            for (k, (a, &x)) in tail.iter_mut().zip(xtail).enumerate() {
                *a += w[k] * f(x);
            }
        }
        match self {
            Self::F32(d) => match d.as_flattened().get(samples) {
                Some(s) => add(acc, s, weights, |x| x),
                None => return false,
            },
            Self::I16(d) => match d.get(samples) {
                Some(s) => add(acc, s, weights.map(|w| w * (1.0 / I16_SCALE)), f32::from),
                None => return false,
            },
            Self::I24(high, low) => match (high.get(samples.clone()), low.get(samples)) {
                (Some(high), Some(low)) => {
                    let w = weights.map(|w| w * (1.0 / I24_SCALE));
                    for (a, (h, l)) in acc.iter_mut().zip(high.as_chunks::<2>().0.iter().zip(low.as_chunks::<2>().0)) {
                        for c in 0..2 {
                            a[c] += w[c] * (i32::from(h[c]) << 8 | i32::from(l[c])) as f32;
                        }
                    }
                }
                _ => return false,
            },
            Self::Packed(p) => {
                // Decoded in stack-sized chunks: no allocation on the audio thread.
                let mut frames = [[0.0; 2]; 64];
                for (c, chunk) in acc.chunks_mut(frames.len()).enumerate() {
                    let frames = &mut frames[..chunk.len()];
                    if !p.decode(at + c * 64, frames) {
                        return false;
                    }
                    add(chunk, frames.as_flattened(), weights, |x| x);
                }
            }
        }
        true
    }

    #[inline(always)]
    fn decode_body(&self, at: usize, out: &mut [Frame]) -> bool {
        if let Self::Packed(p) = self {
            return p.decode(at, out);
        }
        let samples = 2 * at..2 * (at + out.len());
        let out = out.as_flattened_mut();
        match self {
            Self::F32(d) => match d.as_flattened().get(samples) {
                Some(s) => out.copy_from_slice(s),
                None => return false,
            },
            Self::I16(d) => match d.get(samples) {
                Some(s) => {
                    for (o, &s) in out.iter_mut().zip(s) {
                        *o = f32::from(s) * (1.0 / I16_SCALE);
                    }
                }
                None => return false,
            },
            Self::I24(high, low) => match (high.get(samples.clone()), low.get(samples)) {
                (Some(high), Some(low)) => {
                    for ((o, &h), &l) in out.iter_mut().zip(high).zip(low) {
                        *o = (i32::from(h) << 8 | i32::from(l)) as f32 * (1.0 / I24_SCALE);
                    }
                }
                _ => return false,
            },
            Self::Packed(_) => unreachable!(),
        }
        true
    }
}

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
pub struct Packed {
    frames: usize,
    /// Integer full scale: 2^15 or 2^23.
    scale: f32,
    /// Start of each block in `data`.
    offsets: Box<[u32]>,
    /// Blocks, then eight zero bytes so every bit read is one unaligned u64 load.
    data: Box<[u8]>,
}

impl Packed {
    /// Pack `frames` if `quantize` turns every block into integers at
    /// `scale` (returning false if it cannot) and packing saves at least a
    /// tenth.
    #[inline(always)]
    fn encode<T: Copy>(
        frames: &[[T; 2]],
        scale: f32,
        quantize: impl for<'a> Fn(&'a [[T; 2]; BLOCK], &'a mut [i32; 2 * BLOCK]) -> Option<&'a [i32; 2 * BLOCK]>,
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
                    let r = q[2][c].wrapping_sub(q[1][c].wrapping_mul(2)).wrapping_add(q[0][c]);
                    residuals[c][i] = r;
                    magnitude[c] |= (r ^ (r >> 31)) as u32;
                }
            }
            let widths = magnitude.map(|m| if m == 0 { 0 } else { 33 - m.leading_zeros() as u8 });
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

    fn bytes(&self) -> usize {
        size_of_val(&*self.offsets) + size_of_val(&*self.data)
    }

    /// Decode frames `[at, at + out.len())`; false if out of range.
    #[inline(always)]
    fn decode(&self, at: usize, out: &mut [Frame]) -> bool {
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
            let w = u32::from(data[c]);
            let bits = &data[at..];
            at += ((BLOCK - 2) * w as usize).div_ceil(8);
            // A `w`-bit field sign-extends by shifting it to the top and back.
            let shift = 64 - w.max(1);
            let keep = if w == 0 { 0 } else { u64::MAX };
            // Every field's eight-byte read stays in bounds: blocks are
            // followed by the next block or the zero tail.
            assert!((n.saturating_sub(1) * w as usize) / 8 + 8 <= bits.len());
            let (first, mut prev) = (word(2 + 6 * c), word(5 + 6 * c));
            let mut delta = prev.wrapping_sub(first);
            (x[0], x[1]) = (first, prev);
            for (i, y) in x[2..2 + n].iter_mut().enumerate() {
                let bit = i * w as usize;
                // SAFETY: `bit / 8 + 8 <= bits.len()` by the assertion above.
                let raw = u64::from_le(unsafe { bits.as_ptr().add(bit / 8).cast::<u64>().read_unaligned() });
                let r = ((((raw >> (bit % 8)) << shift) as i64 >> shift) as u64 & keep) as i32;
                delta = delta.wrapping_add(r);
                prev = prev.wrapping_add(delta);
                *y = prev;
            }
        }
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
    let groups = fields.as_chunks::<8>().0.iter().zip(packed.chunks_exact_mut(W));
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

/// Integer frames as they are: a copy would cost more than packing them,
/// its stores stalling the loads of the residual pass.
fn in_place<'a>(src: &'a [[i32; 2]; BLOCK], _: &'a mut [i32; 2 * BLOCK]) -> Option<&'a [i32; 2 * BLOCK]> {
    src.as_flattened().first_chunk()
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

/// Whether `x * scale` is an integer in `-scale..scale`.
#[inline]
fn exact(x: f32, scale: f32) -> bool {
    exact_at(x * scale, scale)
}

/// Whether `q` is an integer in `-scale..scale`.
#[inline]
fn exact_at(q: f32, scale: f32) -> bool {
    ((q as i32) as f32 == q) & (q >= -scale) & (q < scale)
}

/// A fully decoded sample.
pub struct Sample {
    pub rate: u32,
    pub frames: Vec<Frame>,
}

/// Decode a whole sample, refusing anything longer than `max_frames`.
pub fn decode(path: &Path, max_frames: usize) -> Result<Sample> {
    let mut reader = Sources::default().source(path)?.open()?;
    ensure!(
        reader.frames <= max_frames as u64,
        "Sample exceeds remaining memory budget"
    );
    let mut frames = vec![[0.0; 2]; reader.frames as usize];
    reader.read(0, &mut frames)?;
    Ok(Sample {
        rate: reader.rate,
        frames,
    })
}

pub(crate) type SourceVersion = (Option<(u64, u128)>, u64);

/// Where a sample's bytes live: a plain file or an archive member.
#[derive(Clone)]
pub struct Source {
    /// Virtual path; its extension selects the codec.
    path: PathBuf,
    handle: Arc<File>,
    pub(crate) version: SourceVersion,
    offset: u64,
    len: Option<u64>,
    key: Option<Arc<LibraryKey>>,
}

/// Resolves sample paths, caching archive indexes and library keys.
#[derive(Default)]
pub struct Sources {
    /// By the path's bytes: hashing a `Path` walks its components, one
    /// hasher write each, and every sample looks its archive up.
    archives: HashMap<OsString, Indexed>,
}

/// An archive's directory, open file and (once needed) library key.
struct Indexed {
    index: Archive,
    file: Arc<File>,
    key: OnceLock<Result<Option<Arc<LibraryKey>>, String>>,
    version: SourceVersion,
}

impl Sources {
    /// The source of `path`, reading its archive's directory once per archive.
    pub fn source(&mut self, path: &Path) -> Result<Source> {
        // An archive already read is a file: no stat per member.
        let archives = &self.archives;
        let found = crate::import::archive_member_where(path, |p| archives.contains_key(p.as_os_str()) || p.is_file());
        let Some((archive, member)) = found else {
            return Ok(Source {
                path: path.into(),
                handle: Arc::new(File::open(path)?),
                version: (crate::cache::version(path), 0),
                offset: 0,
                len: None,
                key: None,
            });
        };
        if !self.archives.contains_key(archive.as_os_str()) {
            let mut file = File::open(&archive)?;
            let index = Archive::read_index(&mut file)
                .with_context(|| format!("Archive {}", archive.display()))?;
            let key = OnceLock::new();
            use std::hash::{Hash, Hasher};
            let mut access = std::collections::hash_map::DefaultHasher::new();
            for path in crate::import::library_metadata(&archive) {
                path.hash(&mut access);
                crate::cache::version(&path).hash(&mut access);
            }
            let version = (crate::cache::version(&archive), access.finish());
            self.archives.insert(archive.clone().into(), Indexed { index, file: Arc::new(file), key, version });
        }
        let indexed = &self.archives[archive.as_os_str()];
        let entry = indexed
            .index
            .member(FileAt { file: &indexed.file, pos: 0 }, &member)?
            .context("Archive member not found")?;
        ensure!(
            entry.valid,
            "{}",
            entry.issue.unwrap_or("Invalid archive member")
        );
        let key = if entry.encoded && entry.key_index != 0xff {
            ensure!(entry.key_index == 0x100, "Unsupported legacy NKX cipher");
            let key = indexed.key.get_or_init(|| {
                crate::import::library_key(&archive)
                    .map(|key| key.map(Arc::new))
                    .map_err(|e| format!("{e:#}"))
            });
            Some(
                key.clone()
                    .map_err(anyhow::Error::msg)?
                    .context("Encrypted archive member needs local library access data")?,
            )
        } else {
            None
        };
        Ok(Source {
            path: path.into(),
            handle: indexed.file.clone(),
            version: indexed.version,
            offset: entry.offset,
            len: Some(entry.size),
            key,
        })
    }
}

impl Source {
    /// The sample path this source was resolved from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn open(&self) -> Result<SampleReader> {
        self.open_counted(false)
    }

    /// [`Source::open`] for the streamer: its reads count in [`DISK_READ`].
    pub fn open_stream(&self) -> Result<SampleReader> {
        self.open_counted(true)
    }

    fn open_counted(&self, counted: bool) -> Result<SampleReader> {
        SampleReader::open(self, counted).with_context(|| {
            let why = if self.is_unwritten() {
                "its data reads back as zeros (a filesystem read problem or an incomplete download)"
            } else {
                "decoding failed"
            };
            format!("{}: {why}", self.path.display())
        })
    }

    /// Whether the sample's first bytes on disk are all zero: an interrupted
    /// download preallocates archives and leaves the unfetched rest zeroed.
    fn is_unwritten(&self) -> bool {
        let mut head = [0u8; 64];
        let len = self.len.map_or(head.len() as u64, |l| l.min(head.len() as u64)) as usize;
        FileAt { file: &self.handle, pos: self.offset }.read_exact(&mut head[..len])
            .is_ok_and(|()| len > 0 && head[..len].iter().all(|&b| b == 0))
    }

    fn bytes(&self, counted: bool) -> Result<Bytes> {
        let len = match self.len {
            Some(len) => len,
            None => self.handle.metadata()?.len(),
        };
        Ok(Bytes {
            file: self.handle.clone(),
            base: self.offset,
            len,
            pos: 0,
            key: self.key.clone(),
            counted,
        })
    }
}

/// Read and Seek over a shared file with a private position: positional
/// reads, so threads can share one handle and no seek is a syscall.
pub(crate) struct FileAt<'a> {
    pub file: &'a File,
    pub pos: u64,
}

impl Read for FileAt<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        #[cfg(unix)]
        let n = std::os::unix::fs::FileExt::read_at(self.file, buf, self.pos)?;
        #[cfg(windows)]
        let n = std::os::windows::fs::FileExt::seek_read(self.file, buf, self.pos)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for FileAt<'_> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.pos = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::Current(n) => self.pos.checked_add_signed(n),
            SeekFrom::End(n) => self.file.metadata()?.len().checked_add_signed(n),
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before start"))?;
        Ok(self.pos)
    }
}

/// Sample bytes streamed from disk so far (loading reads are not counted):
/// the editor's disk readout is its rate of change.
pub static DISK_READ: AtomicU64 = AtomicU64::new(0);

/// A byte window of a file, decrypted on the fly when keyed.
struct Bytes {
    file: Arc<File>,
    base: u64,
    len: u64,
    pos: u64,
    key: Option<Arc<LibraryKey>>,
    /// Count reads in [`DISK_READ`].
    counted: bool,
}

impl Read for Bytes {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let room = self.len.saturating_sub(self.pos).min(buf.len() as u64) as usize;
        let n = FileAt {
            file: &self.file,
            pos: self.base + self.pos,
        }
        .read(&mut buf[..room])?;
        if self.counted {
            DISK_READ.fetch_add(n as u64, Ordering::Relaxed);
        }
        if let Some(key) = &self.key {
            key.apply_at(self.pos, &mut buf[..n]);
        }
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Bytes {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let pos = match to {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(n) => self.len.checked_add_signed(n),
            SeekFrom::Current(n) => self.pos.checked_add_signed(n),
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before start"))?;
        self.base
            .checked_add(pos)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek overflow"))?;
        self.pos = pos;
        Ok(pos)
    }
}

impl MediaSource for Bytes {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

/// Random-access frame reader for WAV/AIFF (symphonia) and NCW (block codec).
pub struct SampleReader {
    pub rate: u32,
    pub frames: u64,
    /// Declared bits per sample.
    pub bits: Option<u16>,
    codec: Codec,
}

enum Codec {
    Ncw(Box<NcwCodec>),
    Pcm(Box<PcmCodec>),
}

struct NcwCodec {
    reader: ncw::NcwReader<BufReader<Bytes>>,
    channels: usize,
    float: bool,
    scale: f32,
    /// Cached decoded block: index and frames.
    block: Option<usize>,
    frames: Vec<Frame>,
}

struct PcmCodec {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn codecs::Decoder>,
    track: u32,
    /// Decoded frames starting at sample frame `start`.
    buffer: Vec<Frame>,
    start: u64,
}

/// Forward gaps up to this many frames are decoded through instead of seeking.
const SKIP_AHEAD: u64 = 16384;

impl SampleReader {
    fn open(source: &Source, counted: bool) -> Result<Self> {
        let bytes = source.bytes(counted)?;
        let ncw = source
            .path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ncw"));
        let reader = if ncw {
            Self::ncw(bytes)?
        } else {
            Self::pcm(source, bytes)?
        };
        ensure!(
            reader.rate > 0 && reader.frames > 0,
            "Empty sample or invalid rate"
        );
        Ok(reader)
    }

    fn ncw(bytes: Bytes) -> Result<Self> {
        let reader = ncw::NcwReader::read(BufReader::new(bytes))?;
        let header = &reader.header;
        ensure!(
            (1..=2).contains(&header.channels),
            "Only mono/stereo samples are supported"
        );
        let (rate, frames) = (header.sample_rate, u64::from(header.num_samples));
        let bits = Some(header.bits_per_sample);
        let codec = NcwCodec {
            channels: header.channels as usize,
            scale: 2f32.powi(i32::from(header.bits_per_sample) - 1),
            float: reader.sample_format == ncw::SampleFormat::Float,
            reader,
            block: None,
            frames: Vec::new(),
        };
        Ok(Self {
            rate,
            frames,
            bits,
            codec: Codec::Ncw(Box::new(codec)),
        })
    }

    fn pcm(source: &Source, bytes: Bytes) -> Result<Self> {
        let mut hint = Hint::new();
        if let Some(extension) = source.path.extension().and_then(|s| s.to_str()) {
            hint.with_extension(extension);
        }
        let stream = MediaSourceStream::new(Box::new(bytes), Default::default());
        let format = symphonia::default::get_probe()
            .format(
                &hint,
                stream,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )?
            .format;
        let track = format.default_track().context("No audio track")?;
        let params = &track.codec_params;
        let frames = params.n_frames.context("Sample length is not declared")?;
        let rate = params.sample_rate.context("Sample rate is not declared")?;
        let bits = params.bits_per_sample.and_then(|b| u16::try_from(b).ok());
        if let Some(channels) = params.channels {
            ensure!(
                (1..=2).contains(&channels.count()),
                "Only mono/stereo samples are supported"
            );
        }
        let decoder = symphonia::default::get_codecs().make(params, &DecoderOptions::default())?;
        let track = track.id;
        let codec = PcmCodec {
            format,
            decoder,
            track,
            buffer: Vec::new(),
            start: 0,
        };
        Ok(Self {
            rate,
            frames,
            bits,
            codec: Codec::Pcm(Box::new(codec)),
        })
    }

    /// Frames `range` stored as [`Pcm::pack`] would, straight from the
    /// integers where the codec has them. `ints` and `frames` are scratch.
    pub fn read_pcm(
        &mut self,
        range: std::ops::Range<u64>,
        compress: bool,
        ints: &mut Vec<[i32; 2]>,
        frames: &mut Vec<Frame>,
    ) -> Result<Pcm> {
        self.read_pcm_cancelable(range, compress, ints, frames, &|| false)
    }

    pub fn read_pcm_cancelable(
        &mut self,
        range: std::ops::Range<u64>,
        compress: bool,
        ints: &mut Vec<[i32; 2]>,
        frames: &mut Vec<Frame>,
        canceled: &dyn Fn() -> bool,
    ) -> Result<Pcm> {
        ensure!(!canceled(), "Sample load canceled");
        let len = (range.end - range.start) as usize;
        if let (Codec::Ncw(codec), Some(bits)) = (&mut self.codec, self.bits)
            && !codec.float
            && range.end <= self.frames
        {
            ints.clear();
            ints.resize(len, [0; 2]);
            for (i, chunk) in ints.chunks_mut(65536).enumerate() {
                ensure!(!canceled(), "Sample load canceled");
                codec.read_ints(range.start + (i * 65536) as u64, chunk)?;
            }
            if let Some(pcm) = Pcm::pack_ints(ints, bits, compress) {
                return Ok(pcm);
            }
        }
        frames.clear();
        frames.resize(len, [0.0; 2]);
        for (i, chunk) in frames.chunks_mut(65536).enumerate() {
            ensure!(!canceled(), "Sample load canceled");
            self.read(range.start + (i * 65536) as u64, chunk)?;
        }
        Ok(Pcm::pack(frames, compress))
    }

    /// Fill `out` with frames from `start`. Frames past the end are silent;
    /// non-finite input decodes as silence.
    pub fn read(&mut self, start: u64, out: &mut [Frame]) -> Result<()> {
        let valid = self.frames.saturating_sub(start).min(out.len() as u64) as usize;
        let (head, tail) = out.split_at_mut(valid);
        tail.fill([0.0; 2]);
        match &mut self.codec {
            // Integer NCW is always finite; float NCW is cleaned per block.
            Codec::Ncw(codec) => codec.read(start, head)?,
            Codec::Pcm(codec) => {
                codec.read(start, head)?;
                for sample in head.as_flattened_mut() {
                    if !sample.is_finite() {
                        *sample = 0.0;
                    }
                }
            }
        }
        Ok(())
    }
}

impl NcwCodec {
    /// Integer frames from `start`, all within the sample.
    fn read_ints(&mut self, mut start: u64, mut out: &mut [[i32; 2]]) -> Result<()> {
        const BLOCK: u64 = ncw::NcwReader::<BufReader<Bytes>>::FRAMES_PER_BLOCK as u64;
        while !out.is_empty() {
            let channels = self.reader.decode_block((start / BLOCK) as usize)?;
            let (left, right) = (&channels[0], &channels[self.channels - 1]);
            let offset = (start % BLOCK) as usize;
            let n = left.len().saturating_sub(offset).min(out.len());
            ensure!(n > 0, "NCW block shorter than declared");
            for ((o, &l), &r) in out[..n].iter_mut().zip(&left[offset..]).zip(&right[offset..]) {
                *o = [l, r];
            }
            out = &mut out[n..];
            start += n as u64;
        }
        Ok(())
    }

    fn read(&mut self, mut start: u64, mut out: &mut [Frame]) -> Result<()> {
        const BLOCK: u64 = ncw::NcwReader::<BufReader<Bytes>>::FRAMES_PER_BLOCK as u64;
        while !out.is_empty() {
            let index = (start / BLOCK) as usize;
            if self.block != Some(index) {
                self.block = None;
                let channels = self.reader.decode_block(index)?;
                let (float, scale) = (self.float, self.scale);
                let convert = |s: i32| {
                    if float {
                        Some(f32::from_bits(s as u32)).filter(|x| x.is_finite()).unwrap_or(0.0)
                    } else {
                        s as f32 / scale
                    }
                };
                let (left, right) = (&channels[0], &channels[self.channels - 1]);
                self.frames.clear();
                self.frames.extend(
                    left.iter()
                        .zip(right)
                        .map(|(&l, &r)| [convert(l), convert(r)]),
                );
                self.block = Some(index);
            }
            let offset = (start % BLOCK) as usize;
            let available = self
                .frames
                .get(offset..)
                .context("NCW block shorter than declared")?;
            ensure!(!available.is_empty(), "NCW block shorter than declared");
            let n = available.len().min(out.len());
            out[..n].copy_from_slice(&available[..n]);
            out = &mut out[n..];
            start += n as u64;
        }
        Ok(())
    }
}

impl PcmCodec {
    fn end(&self) -> u64 {
        self.start + self.buffer.len() as u64
    }

    fn read(&mut self, mut start: u64, mut out: &mut [Frame]) -> Result<()> {
        if start < self.start || start > self.end() + SKIP_AHEAD {
            self.seek(start)?;
        }
        while !out.is_empty() {
            if start < self.end() && start >= self.start {
                let offset = (start - self.start) as usize;
                let n = (self.buffer.len() - offset).min(out.len());
                out[..n].copy_from_slice(&self.buffer[offset..offset + n]);
                out = &mut out[n..];
                start += n as u64;
            } else if start < self.start {
                bail!("Sample seek landed past frame {start}");
            } else if !self.decode_next()? {
                bail!("Sample ended before its declared length");
            }
        }
        Ok(())
    }

    fn seek(&mut self, frame: u64) -> Result<()> {
        let to = SeekTo::TimeStamp {
            ts: frame,
            track_id: self.track,
        };
        let seeked = self.format.seek(SeekMode::Accurate, to)?;
        self.decoder.reset();
        self.buffer.clear();
        self.start = seeked.actual_ts;
        Ok(())
    }

    /// Decode the next packet into the buffer; false at end of stream.
    fn decode_next(&mut self) -> Result<bool> {
        let packet = loop {
            match self.format.next_packet() {
                Ok(packet) if packet.track_id() == self.track => break packet,
                Ok(_) => continue,
                Err(SymphoniaError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    return Ok(false);
                }
                Err(e) => return Err(e.into()),
            }
        };
        let decoded = self.decoder.decode(&packet)?;
        let spec = *decoded.spec();
        let channels = spec.channels.count();
        ensure!(
            (1..=2).contains(&channels),
            "Only mono/stereo samples are supported"
        );
        let mut pcm = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        pcm.copy_interleaved_ref(decoded);
        self.start = packet.ts();
        self.buffer.clear();
        self.buffer.extend(
            pcm.samples()
                .chunks_exact(channels)
                .map(|s| [s[0], s[channels - 1]]),
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_file_readers_seek_independently_and_decode_can_be_canceled() {
        let path =
            std::env::temp_dir().join(format!("kontra-cancel-read-{}.wav", std::process::id()));
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut wav = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..131072 {
            for x in [i as f32 / 262144.0, -0.5] {
                wav.write_sample(x).unwrap();
            }
        }
        wav.finalize().unwrap();
        let source = Sources::default().source(&path).unwrap();
        let (mut a, mut b) = (source.open().unwrap(), source.open().unwrap());
        let mut out = [[0.0; 2]; 1];
        a.read(90000, &mut out).unwrap();
        b.read(3, &mut out).unwrap();
        assert_eq!(out[0], [3.0 / 262144.0, -0.5]);
        a.read(90001, &mut out).unwrap();
        assert_eq!(out[0], [90001.0 / 262144.0, -0.5]);
        let calls = std::cell::Cell::new(0);
        let cancel = || {
            calls.set(calls.get() + 1);
            calls.get() >= 3
        };
        let mut frames = Vec::new();
        let result = a.read_pcm_cancelable(0..131072, false, &mut Vec::new(), &mut frames, &cancel);
        assert!(result.err().unwrap().to_string().contains("canceled"));
        assert_eq!(frames[65535][1], -0.5);
        assert_eq!(frames[65536], [0.0; 2], "decoded beyond the canceled chunk");
        std::fs::remove_file(path).unwrap();
    }

    /// The disk readout counts streaming, not loading. Library tests never
    /// stream, so only this test's streamed read moves the counter.
    #[test]
    fn only_streaming_reads_count_as_disk_throughput() {
        let path = std::env::temp_dir().join(format!("kontakto-disk-{}.wav", std::process::id()));
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        (0..20_000).for_each(|i| w.write_sample(i as i16).unwrap());
        w.finalize().unwrap();
        let source = Sources::default().source(&path).unwrap();
        let mut out = vec![[0.0; 2]; 10_000];
        let before = DISK_READ.load(Ordering::Relaxed);
        source.open().unwrap().read(0, &mut out).unwrap();
        assert_eq!(DISK_READ.load(Ordering::Relaxed), before, "loading reads");
        source.open_stream().unwrap().read(0, &mut out).unwrap();
        assert!(DISK_READ.load(Ordering::Relaxed) >= before + 40_000, "streamed reads");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn pcm_stores_each_resolution_exactly() {
        let at = |scale: f32| move |i: i32| (i * 7919 % 65536 - 32768) as f32 * scale;
        let i16 = at(1.0 / 32768.0);
        let i24 = at(1.0 / 8388608.0 * 255.0);
        let cases: [(Vec<Frame>, usize); 3] = [
            ((0..1000).map(|i| [i16(i), i16(i + 1)]).collect(), 4),
            ((0..1000).map(|i| [i24(i), -i24(i)]).collect(), 6),
            ((0..1000).map(|i| [(i as f32).sin(), 1.0]).collect(), 8),
        ];
        for (frames, width) in cases {
            let pcm = Pcm::pack(&frames, true);
            assert_eq!(pcm.bytes(), width * frames.len());
            let mut out = vec![[0.0; 2]; 900];
            assert_eq!(pcm.window(100, &mut out).unwrap(), &frames[100..]);
            assert!(pcm.window(101, &mut out).is_none());
            // Weighted sums straight from storage match decoding first, bit for bit.
            for (pcm, len) in [(pcm, 131), (Pcm::pack(&frames, false), 77)] {
                let w = [0.37, -1.3];
                let mut acc: Vec<Frame> = (0..len).map(|i| [i as f32 * 0.01, 0.5]).collect();
                let want: Vec<Frame> = acc.iter().zip(&frames[100..]).map(|(a, x)| [a[0] + w[0] * x[0], a[1] + w[1] * x[1]]).collect();
                assert!(pcm.accumulate(100, w, &mut acc));
                assert_eq!(acc, want);
                assert!(!pcm.accumulate(1000 - len + 1, w, &mut acc));
            }
        }
    }

    #[test]
    fn packed_pcm_round_trips_every_window() {
        // Smooth signals with a little noise pack; 1000 frames leave a partial last block.
        let mut seed = 1u32;
        let mut noise = move || {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            (seed >> 16) as i32 % 64 - 32
        };
        for scale in [I16_SCALE, I24_SCALE] {
            let peak = scale as i32 / 2;
            let q = |x: i32| x as f32 / scale;
            let frames: Vec<Frame> = (0..1000)
                .map(|i| {
                    let x = ((i as f32 * 0.05).sin() * peak as f32) as i32;
                    [q(x + noise()), q(-x / 3 + noise())]
                })
                .collect();
            // Silence packs to headers only; the extremes use the widest residuals.
            let edges: Vec<Frame> = (0..130).map(|i| [0.0, if i % 2 == 0 { -1.0 } else { q(peak * 2 - 1) }]).collect();
            // Full-scale block headers, zero residuals.
            let flat: Vec<Frame> = vec![[-1.0, q(peak * 2 - 1)]; 131];
            for frames in [frames, edges, flat] {
                let pcm = Pcm::pack(&frames, true);
                assert!(matches!(pcm, Pcm::Packed(_)) || frames.len() == 130);
                let n = frames.len();
                for (at, len) in [(0, n), (1, 63), (63, 2), (64, 64), (100, n - 101), (n - 1, 1)] {
                    let mut out = vec![[0.0; 2]; len];
                    assert_eq!(pcm.window(at, &mut out).unwrap(), &frames[at..at + len], "at {at}");
                }
                assert!(pcm.window(frames.len(), &mut [[0.0; 2]; 1]).is_none());
            }
        }
    }

    #[test]
    fn packed_fields_match_a_serial_bit_writer() {
        let mut seed = 7u32;
        for width in 0..=33u8 {
            let mut fields = [0i32; BLOCK];
            for f in &mut fields[..BLOCK - 2] {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                *f = seed as i32;
            }
            let (mut serial, mut acc, mut bits) = (Vec::new(), 0u128, 0);
            for &v in &fields[..BLOCK - 2] {
                acc |= u128::from(v as u64 & ((1u64 << width) - 1)) << bits;
                bits += u32::from(width);
                while bits >= 8 {
                    serial.push(acc as u8);
                    (acc, bits) = (acc >> 8, bits - 8);
                }
            }
            if bits > 0 {
                serial.push(acc as u8);
            }
            let mut grouped = Vec::new();
            pack_fields(&fields, width, &mut grouped);
            grouped.truncate(serial.len());
            assert_eq!(grouped, serial, "width {width}");
        }
    }
}

#[cfg(test)]
mod decode_bench {
    use super::*;

    /// Thread CPU seconds: immune to preemption on a busy machine.
    fn cpu() -> f64 {
        #[repr(C)]
        struct Timespec {
            s: i64,
            ns: i64,
        }
        unsafe extern "C" {
            fn clock_gettime(clock: i32, t: *mut Timespec) -> i32;
        }
        let mut t = Timespec { s: 0, ns: 0 };
        // SAFETY: CLOCK_THREAD_CPUTIME_ID (3) writes one timespec.
        unsafe { clock_gettime(3, &mut t) };
        t.s as f64 + t.ns as f64 * 1e-9
    }

    /// Decode cost per voice-sized window, raw 24-bit against packed:
    /// `cargo test --release --lib decode_speed -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[cfg(target_os = "linux")]
    fn decode_speed() {
        let mut seed = 1u32;
        let frames: Vec<Frame> = (0..1 << 16)
            .map(|i| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let n = (seed >> 16) as i32 % 512 - 256;
                let x = ((i as f32 * 0.01).sin() * 4e6) as i32 + n;
                [x as f32 / I24_SCALE, (x / 2) as f32 / I24_SCALE]
            })
            .collect();
        let mut best = f64::MAX;
        for _ in 0..20 {
            let t = cpu();
            assert!(matches!(Pcm::pack(&frames, true), Pcm::Packed(_)));
            best = best.min(cpu() - t);
        }
        println!("pack: {:.2} ns per frame", best / frames.len() as f64 * 1e9);
        let ints: Vec<[i32; 2]> = (frames.iter())
            .map(|f| f.map(|x| (x * I24_SCALE) as i32))
            .collect();
        let mut best = f64::MAX;
        for _ in 0..40 {
            let t = cpu();
            assert!(matches!(Pcm::pack_ints(&ints, 24, true), Some(Pcm::Packed(_))));
            best = best.min(cpu() - t);
        }
        println!("pack_ints: {:.2} ns per frame", best / frames.len() as f64 * 1e9);
        let packed = Pcm::pack(&frames, true);
        let raw = Pcm::pack(&frames, false);
        assert!(matches!(packed, Pcm::Packed(_)) && matches!(raw, Pcm::I24(..)));
        let mut out = vec![[0f32; 2]; 132];
        for (name, pcm) in [("i24", &raw), ("packed", &packed)] {
            let mut best = f64::MAX;
            for _ in 0..40 {
                let t = cpu();
                let mut sum = 0.0;
                for rep in 0..4 {
                    for at in (rep..frames.len() - 200).step_by(128) {
                        pcm.decode(at, &mut out);
                        sum += out[0][0];
                    }
                }
                best = best.min(cpu() - t);
                assert!(sum.is_finite());
            }
            let calls = 4.0 * ((frames.len() - 200) / 128) as f64;
            println!(
                "{name}: {:.0} ns per 132-frame window, {} bytes",
                best / calls * 1e9,
                pcm.bytes()
            );
        }
    }

    /// Streaming decode cost of a real sample, in the streamer's chunks:
    /// `KONTAKTO_BENCH_SAMPLE=<path> cargo test --release --lib stream_decode_speed -- --ignored --nocapture`
    #[test]
    #[ignore]
    #[cfg(target_os = "linux")]
    fn stream_decode_speed() {
        let Some(path) = std::env::var_os("KONTAKTO_BENCH_SAMPLE") else { return };
        let source = Sources::default().source(Path::new(&path)).unwrap();
        let mut out = vec![[0f32; 2]; 1024];
        let mut best = f64::MAX;
        let mut frames = 0;
        for _ in 0..10 {
            let mut reader = source.open().unwrap();
            frames = reader.frames;
            let t = cpu();
            for at in (0..frames).step_by(out.len()) {
                reader.read(at, &mut out).unwrap();
            }
            best = best.min(cpu() - t);
        }
        println!("{:.1} ns per frame over {frames} frames", best / frames as f64 * 1e9);
    }
}
