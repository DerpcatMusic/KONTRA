//! The per-block staging and output path of the reference process block
//! (entry 0x890a00..0x893bd7), as far as it is decoded.
//!
//! What is here, and where it comes from:
//! - [`align16`]: the workspace bump in 0x8ad640 rounds each offset up to a
//!   multiple of 16 floats (`(x - (x & 0xf)) + 0x10`, or `x` when already
//!   aligned).
//! - [`Ring`]: the ring structs at `obj+0xbff8`, `+0xc088`, `+0xc118`. Fields
//!   as read from 0x893de0 and 0x8973f0: channels at `+0x8c` (8 bytes each),
//!   length in floats at `+0x88` (a power of two, so the mask is `len - 1`),
//!   write position at `+0x80`. [`Ring::stage`] is the copy through 0x89ba70,
//!   which splits at the write position. [`Ring::advance`] is the masked
//!   update at the tail of 0x893de0.
//! - [`IdentityDelay`]: the output path for the identity band state. Its delay
//!   is the reported latency (`ReferencePlan::latency`), and it reproduces the
//!   emulated process block to float tolerance (test below).
//!
//! The hop stages themselves (window, FFTs, multiply, overlap-add) and the
//! per-bin multiplier boundary `BinGain` are in `super::hop`; the zero-latency
//! workers are in `super::zerolatency`. `IdentityDelay` remains the identity-state
//! model for every rate and mode (including zero latency at 88.2 kHz and up, whose
//! resampler around the zero-latency workers is not translated yet).
//! Nothing here changes `dsp.rs`.

use crate::reference_plan::ReferencePlan;

/// Round `x` up to a multiple of 16, as the workspace bump in 0x8ad640 does.
/// Offsets in floats; the byte offset is `4 * align16(x)`.
///
/// Verified against the arithmetic of 0x8ad640 (`x & 0xf` test, add 0x10).
pub fn align16(x: usize) -> usize {
    (x + 15) & !15
}

/// One ring struct: a power-of-two circular buffer per channel with a shared
/// write position. Field offsets are relative to the struct base
/// (`obj+0xbff8` etc.).
#[derive(Clone, Debug)]
pub struct Ring {
    /// Per-channel buffers, each `len` floats long (`+0x88`).
    channels: Vec<Vec<f32>>,
    /// Write position in floats (`+0x80`), always `< len`.
    write: usize,
}

impl Ring {
    /// A ring of `channels` buffers of `len` floats each. `len` must be a power
    /// of two, because the reference masks positions with `len - 1`.
    pub fn new(channels: usize, len: usize) -> Self {
        assert!(len.is_power_of_two(), "ring length {len} is not a power of two");
        Self {
            channels: vec![vec![0.0; len]; channels],
            write: 0,
        }
    }

    /// Length in floats (`+0x88`).
    pub fn len(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Channel count (`+0x8c`).
    pub fn channels(&self) -> usize {
        self.channels.len()
    }

    /// Write position in floats (`+0x80`).
    pub fn write_position(&self) -> usize {
        self.write
    }

    /// Copy one frame into the ring at the write position, wrapping at the
    /// end. `frame[c]` holds `n` samples for channel `c`; `n` must not exceed
    /// the ring length. Does not move the write position (see [`Ring::advance`]).
    ///
    /// Equivalent to 0x8973f0 through 0x89ba70: the copy is split at the write
    /// position, so each channel is at most two contiguous runs.
    pub fn stage(&mut self, frame: &[&[f32]]) {
        assert_eq!(frame.len(), self.channels.len(), "frame channel count");
        let len = self.len();
        let mask = len - 1;
        for (dst, src) in self.channels.iter_mut().zip(frame) {
            assert!(src.len() <= len, "frame longer than the ring");
            for (i, &s) in src.iter().enumerate() {
                dst[(self.write + i) & mask] = s;
            }
        }
    }

    /// Advance the write position by `n` samples, masked: `write = (write + n) & (len - 1)`.
    /// This is the tail of 0x893de0.
    pub fn advance(&mut self, n: usize) {
        self.write = (self.write + n) & (self.len() - 1);
    }

    /// Read `out.len()` samples of channel `c`, starting `delay` samples
    /// behind `start`. `start` is a position in floats (usually the write
    /// position before the staging call).
    pub fn read_back(&self, c: usize, start: usize, delay: usize, out: &mut [f32]) {
        let mask = self.len() - 1;
        let buf = &self.channels[c];
        for (i, o) in out.iter_mut().enumerate() {
            *o = buf[(start + i).wrapping_sub(delay) & mask];
        }
    }
}

/// The identity-state output path: the input delayed by the reported latency.
///
/// Built from the same ring structure as the reference. The ring is
/// `groups << 12` floats (the constructor's ring size, see
/// `research/re-latency-fft-scale.md` 17.1). With the zero band state the
/// emulated block is this delay, to about 3e-7 (`oracle/process_fixture_*`).
#[derive(Clone, Debug)]
pub struct IdentityDelay {
    ring: Ring,
    latency: usize,
}

impl IdentityDelay {
    pub fn new(plan: &ReferencePlan, channels: usize) -> Self {
        let len = plan.groups << 12;
        let latency = plan.latency();
        assert!(latency < len, "latency {latency} must fit the ring of {len}");
        Self {
            ring: Ring::new(channels, len),
            latency,
        }
    }

    pub fn latency(&self) -> usize {
        self.latency
    }

    /// Process one block: stage `input`, read `output` `latency` samples behind it.
    /// Every slice holds `n` samples and `n` must not exceed the ring length.
    pub fn process(&mut self, input: &[&[f32]], output: &mut [&mut [f32]]) {
        let n = input.first().map_or(0, |s| s.len());
        let start = self.ring.write_position();
        self.ring.stage(input);
        for (c, out) in output.iter_mut().enumerate() {
            debug_assert_eq!(out.len(), n);
            self.ring.read_back(c, start, self.latency, out);
        }
        self.ring.advance(n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn align16_rounds_up_to_sixteen_floats() {
        for (x, want) in [(0, 0), (1, 16), (15, 16), (16, 16), (17, 32), (2304, 2304)] {
            assert_eq!(align16(x), want, "align16({x})");
        }
    }

    #[test]
    fn ring_stage_wraps_at_the_write_position() {
        let mut ring = Ring::new(1, 8);
        ring.stage(&[&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]]);
        ring.advance(6);
        ring.stage(&[&[7.0, 8.0, 9.0, 10.0]]);
        ring.advance(4);
        assert_eq!(ring.write_position(), 2);
        assert_eq!(ring.channels[0], [9.0, 10.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]);
    }

    #[test]
    fn identity_delay_reads_back_the_latency() {
        let plan = ReferencePlan::new(48_000.0, false);
        let mut d = IdentityDelay::new(&plan, 1);
        let lat = d.latency();
        assert_eq!(lat, 2304);
        let total = 8 * 512;
        let input: Vec<f32> = (0..total).map(|i| (i as f32 * 0.37).sin()).collect();
        let mut got = Vec::new();
        for block in input.chunks(512) {
            let mut out = vec![0.0f32; 512];
            d.process(&[block], &mut [&mut out]);
            got.extend_from_slice(&out);
        }
        for n in lat..total {
            assert_eq!(got[n], input[n - lat], "sample {n}");
        }
        assert!(got[..lat].iter().all(|&v| v == 0.0));
    }

    /// Differential check against the emulated process block. Each fixture
    /// (`research/oracle/process_fixture_<rate>_m<mode>.json`, written by
    /// `research/re-tools/emulate/process_fixture.py`) holds the f32 input and
    /// the emulated output of the identity state. Tolerance: 1e-6 absolute on
    /// inputs in [-0.5, 0.5] (the emulator's own rounding is ~3e-7 at most).
    #[test]
    fn emulated_identity_fixtures() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/research/oracle");
        let mut checked = 0;
        for rate in [44_100.0, 48_000.0, 96_000.0, 192_000.0] {
            for zero in [false, true] {
                let mode = usize::from(zero);
                let path = format!("{dir}/process_fixture_{}_m{mode}.json", rate as u32);
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("read {path}: {e}"));
                let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
                let input: Vec<Vec<f32>> = channels(&doc["input"]);
                let output: Vec<Vec<f32>> = channels(&doc["output"]);
                let plan = ReferencePlan::new(rate, zero);
                let mut d = IdentityDelay::new(&plan, input.len());
                assert_eq!(
                    doc["latency_getter"].as_u64().unwrap() as usize,
                    d.latency(),
                    "latency at {rate}, zero={zero}"
                );
                let n = input[0].len();
                let block = doc["block_size"].as_u64().unwrap() as usize;
                let mut got = vec![vec![0.0f32; n]; input.len()];
                for start in (0..n).step_by(block) {
                    let ins: Vec<&[f32]> = input.iter().map(|c| &c[start..start + block]).collect();
                    let mut outs: Vec<&mut [f32]> = got
                        .iter_mut()
                        .map(|c| &mut c[start..start + block])
                        .collect();
                    d.process(&ins, &mut outs);
                }
                for c in 0..input.len() {
                    for i in 0..n {
                        let diff = (got[c][i] - output[c][i]).abs();
                        assert!(diff <= 1e-6, "rate {rate} zero {zero} ch {c} sample {i}: {diff}");
                    }
                }
                checked += 1;
            }
        }
        assert_eq!(checked, 8);
    }

    fn channels(v: &serde_json::Value) -> Vec<Vec<f32>> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|ch| {
                ch.as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_f64().unwrap() as f32)
                    .collect()
            })
            .collect()
    }
}
