//! Kontakt Saturator / Shaper modes 1 (Enhanced) and 2 (Drums).
//!
//! Native `BParFXShaper` audio 0x140aa4d20 dispatches on the mode at +0x1dc to
//! 0x140ab27f0 (Enhanced) and 0x140ab3310 (Drums); mode 0 is the Classic curve in
//! `shaping.rs`. The sample-rate setter 0x1408ddfc0 derives the three smoothing
//! coefficients; the shape setter (0x1409619b0 case 7) derives the row/index
//! target. The lookup tables are the contiguous read-only floats at
//! 0x149e63ee0..0x149e6ace0 of Kontakt 8.13.1, saved as `tables/saturator.f32le`:
//! Enhanced curve rows 0..51 (128 columns), the makeup column (0x149e6a4e0), then
//! the Drums `k1`, `mix` and `k2` columns (0x149e6a6e0, 8e0, aae0).
use super::super::Planar;
use std::sync::OnceLock;

const TABLE_BYTES: &[u8; 28160] = include_bytes!("tables/saturator.f32le");
const MAKEUP: usize = 6528;
const DRUM_K1: usize = 6656;
const DRUM_MIX: usize = 6784;
const DRUM_K2: usize = 6912;
const STRIDE: usize = 128;
/// `-log2(100)`: bits 0xc0d49a78.
const DECAY: f32 = f32::from_bits(0xc0d4_9a78);
/// Bypass at or below this clamped shape (bits 0xbf7ff972).
const BYPASS_BELOW: f32 = f32::from_bits(0xbf7f_f972);

fn tables() -> &'static [f32] {
    static TABLES: OnceLock<Vec<f32>> = OnceLock::new();
    TABLES.get_or_init(|| {
        TABLE_BYTES.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
    })
}

/// Build the table once, off the audio thread.
pub(super) fn prepare() {
    tables();
}

/// `1 - 2^(-log2(100) / max(rate * scale - 1, 1))`, evaluated in binary32.
fn coefficient(rate: f32, scale: f32) -> f32 {
    let mut samples = rate * scale - 1.0;
    if samples <= 1.0 {
        samples = 1.0;
    }
    1.0 - 2f32.powf(DECAY / samples)
}

/// The sample-rate dependent coefficients: row/index tracking (50 ms),
/// gain smoothing (2.68 ms), peak release (25.5 ms).
#[derive(Clone, Copy, Debug)]
pub(super) struct Rates {
    track: f32,
    smooth: f32,
    release: f32,
}

impl Rates {
    pub(super) fn new(rate: u32) -> Self {
        let rate = rate as f32;
        Self {
            track: coefficient(rate, f32::from_bits(0x3d4c_cccd)),
            smooth: coefficient(rate, f32::from_bits(0x3b2f_8dcd)),
            release: coefficient(rate, f32::from_bits(0x3cd0_b3e3)),
        }
    }
}

/// Persistent registers of both modes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct State {
    ready: bool,
    moving: bool,
    target: f32,
    current: f32,
    /// Enhanced: peak follower (+0x208), smoothed gain (+0x220), makeup (+0x258),
    /// table row offsets (+0x240/+0x244) and row fraction (+0x234).
    env: f32,
    gain: f32,
    makeup: f32,
    rows: [usize; 2],
    row_fraction: f32,
    /// Drums: `k1` (+0x25c), `mix` (+0x260), `k2` (+0x258).
    drums: [f32; 3],
}

impl State {
    pub(super) fn finite(&self) -> bool {
        [self.target, self.current, self.env, self.gain, self.makeup, self.row_fraction]
            .iter()
            .chain(&self.drums)
            .all(|v| v.is_finite())
    }
}

/// The clamped shape and whether the module passes the signal through untouched.
fn setup(shape: f32, span: f32, middle: f32, state: &mut State) -> bool {
    let s = if shape <= -1.0 { -1.0 } else if shape >= 1.0 { 1.0 } else { shape };
    let target = (s * 0.5) * span + middle;
    if !state.ready {
        *state = State { ready: true, moving: true, target, current: target, env: 1.0, gain: 1.0, ..State::default() };
    } else if target != state.target {
        state.target = target;
        state.moving = true;
    }
    s <= BYPASS_BELOW
}

/// Advance the tracked index toward the target, one frame.
fn track(rates: Rates, state: &mut State) -> f32 {
    let mut next = state.target;
    let step = (next - state.current) * rates.track;
    if step.abs() >= 1e-4 {
        next = state.current + step;
    } else {
        state.moving = false;
    }
    state.current = next;
    next
}

fn lerp(table: &[f32], at: usize, fraction: f32) -> f32 {
    let a = table.get(at).copied().unwrap_or(0.0);
    let b = table.get(at + 1).copied().unwrap_or(0.0);
    (b - a) * fraction + a
}

/// Mode 1: peak-linked, table-driven level shaper.
pub(super) fn enhanced(shape: f32, rates: Rates, state: &mut State, block: &mut Planar, len: usize) {
    if setup(shape, 50.0, 25.0, state) {
        return;
    }
    let table = tables();
    for i in 0..len {
        let mut peak = 0f32;
        for channel in block.iter() {
            let a = (channel[i] as f32).abs();
            if a > peak {
                peak = a;
            }
        }
        let mut level = peak;
        let drop = level - state.env;
        if drop < 0.0 {
            level = drop * rates.release + state.env;
        }
        state.env = level;
        let mut g = level + f32::from_bits(0x3727_c5ac);
        if g >= 1.0 {
            g = 1.0;
        }
        let position = g * 127.0;
        let column = position as usize;
        let fraction = position - column as f32;
        if state.moving {
            let row_position = track(rates, state);
            let row = row_position as usize;
            state.row_fraction = row_position - row as f32;
            state.rows = [STRIDE * row, STRIDE * row + STRIDE];
            state.makeup = lerp(table, MAKEUP + row, state.row_fraction);
        }
        let low = lerp(table, state.rows[0] + column, fraction);
        let high = lerp(table, state.rows[1] + column, fraction);
        let shaped = ((high - low) * state.row_fraction + low) / g;
        state.gain = (shaped - state.gain) * rates.smooth + state.gain;
        let scale = state.gain * state.makeup;
        for channel in block.iter_mut() {
            channel[i] = f64::from(scale * (channel[i] as f32));
        }
    }
}

/// Mode 2: soft clipper whose drive, blend and trim follow a table index.
pub(super) fn drums(shape: f32, rates: Rates, state: &mut State, block: &mut Planar, len: usize) {
    if setup(shape, 127.0, 63.5, state) {
        return;
    }
    let table = tables();
    for i in 0..len {
        if state.moving {
            let position = track(rates, state);
            let index = position as usize;
            let fraction = position - index as f32;
            state.drums = [
                lerp(table, DRUM_K1 + index, fraction),
                lerp(table, DRUM_MIX + index, fraction),
                lerp(table, DRUM_K2 + index, fraction),
            ];
        }
        let [k1, mix, k2] = state.drums;
        for channel in block.iter_mut() {
            let x = channel[i] as f32;
            let mut driven = x * k1;
            if driven <= -8.0 {
                driven = -8.0;
            }
            if driven >= 8.0 {
                driven = 8.0;
            }
            channel[i] = f64::from((((1.0 - driven.abs() * 0.0625) * driven * k2) - x) * mix + x);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::BLOCK;
    use super::*;

    #[test]
    fn coefficients_follow_the_native_setter() {
        // 1 - 2^(-6.643856 / (48000 * 0.05 - 1)) = 0.0019177.
        let rates = Rates::new(48_000);
        assert!((rates.track - 0.001_917_7).abs() < 1e-6, "{}", rates.track);
        assert!(rates.smooth > rates.release);
    }

    #[test]
    fn tables_hold_the_extracted_curve_ends() {
        let t = tables();
        assert_eq!(t.len(), 7040);
        // Every Enhanced row maps 0 -> 0 and 1 -> 1 at the loud end of row 50.
        assert_eq!((t[0], t[50 * 128], t[50 * 128 + 127]), (0.0, 0.0, 1.0));
        // Drums k1 starts at unity, the blend at zero.
        assert_eq!((t[DRUM_K1], t[DRUM_MIX]), (1.0, 0.0));
    }

    #[test]
    fn drums_at_the_centre_index_is_a_fixed_vector() {
        // shape 0 -> index 63.5 (settled): k1,mix,k2 from the tables, then one sample.
        let mut state = State::default();
        let mut block = [[0.25; BLOCK]; 2];
        let rates = Rates::new(48_000);
        drums(0.0, rates, &mut state, &mut block, 1);
        let t = tables();
        let k = [lerp(t, DRUM_K1 + 63, 0.5), lerp(t, DRUM_MIX + 63, 0.5), lerp(t, DRUM_K2 + 63, 0.5)];
        let driven = (0.25f32 * k[0]).clamp(-8.0, 8.0);
        let want = (((1.0 - driven.abs() * 0.0625) * driven * k[2]) - 0.25) * k[1] + 0.25;
        assert_eq!(block[0][0], f64::from(want));
        assert_eq!(block[0][0], block[1][0]);
    }

    #[test]
    fn enhanced_is_silent_and_finite_and_bypasses_at_minus_one() {
        let rates = Rates::new(48_000);
        let mut state = State::default();
        let mut quiet = [[0.0; BLOCK]; 2];
        enhanced(0.5, rates, &mut state, &mut quiet, BLOCK);
        assert!(quiet.iter().flatten().all(|v| *v == 0.0) && state.finite());
        let mut loud = [[0.9; BLOCK]; 2];
        enhanced(-1.0, rates, &mut State::default(), &mut loud, BLOCK);
        assert_eq!(loud[0][0], 0.9);
        let mut shaped = [[0.9; BLOCK]; 2];
        enhanced(0.5, rates, &mut state, &mut shaped, BLOCK);
        assert!(shaped[0][BLOCK - 1] > 0.0 && shaped[0][BLOCK - 1] < 2.0 && state.finite());
    }
}
