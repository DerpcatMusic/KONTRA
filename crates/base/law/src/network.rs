//! A reverb network's topology as a translator supplies it: which lines, how
//! long, where the input lands and where the output taps.

/// Lines in a [`ReverbNetwork`].
pub const LINES: usize = 31;

/// Prime-length delay lines fed back through the circulant of a ±1
/// sequence, each read through a one-pole damping filter, tapped into two
/// outputs that each run a chain of three-delay allpass units. Lengths are
/// frames at [`rate`](Self::rate); the network runs one frame per frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverbNetwork {
    /// Line `i` is the prime at index `⌊growth[i]·size + offset[i]⌋` (2 is
    /// index 0).
    pub growth: [f32; LINES],
    pub offset: [f32; LINES],
    /// Feedback row `i`, column `j` is `+` where `sequence[(j − i) mod 31]`
    /// is 0, `−` where it is 1; an m-sequence makes it a permuted Hadamard.
    pub sequence: [u8; LINES],
    /// Per input channel, the lines it feeds and the sign; sign 0 feeds none.
    pub inputs: [[(u8, f32); 10]; 2],
    /// The input lands this many frames short of line 0's length into
    /// every line.
    pub inject_lead: u32,
    /// Per output channel, in summing order: the line and its weight at
    /// diffusion 0 and at diffusion 1.
    pub taps: [[(u8, f32, f32); 8]; 2],
    /// Delays of each channel's allpass units, in order; their gain is
    /// diffusion / 2, the sign alternating from −.
    pub units: [[u16; 3]; 4],
    pub unit_count: u8,
    /// The units' inner feedback scale.
    pub unit_feedback: f32,
    /// The high-frequency decay time over the low:
    /// `(1 − d·hf[0])(1 − l·hf[2]) + (1 − d·hf[1])·l·hf[2]`, `l = ln 2t`.
    pub hf: [f32; 3],
    /// The wet level `level[1]·√(1 − 10^(−level[0]/t))`.
    pub level: [f32; 2],
    /// The output low shelf's corner.
    pub shelf_hz: f32,
    /// The rate its frames and filters are designed for.
    pub rate: f32,
    /// Above this host rate it runs at `rate / n`, `n = ⌊host / rate⌋`,
    /// between a resampler pair (when `n` divides the host rate).
    pub max_rate: f32,
}
