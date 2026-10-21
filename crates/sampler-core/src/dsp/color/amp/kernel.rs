//! Four-lane arithmetic and the 4x polyphase oversampler shared by the amp kernels. Every
//! model that runs its shaper at four times the rate uses these; each keeps its own state
//! layout and filter network.
pub(super) type V = [f32; 4];

pub(super) fn zip(a: V, b: V, f: impl Fn(f32, f32) -> f32) -> V {
    std::array::from_fn(|l| f(a[l], b[l]))
}

pub(super) fn mul(a: V, b: V) -> V {
    zip(a, b, |a, b| a * b)
}

pub(super) fn add(a: V, b: V) -> V {
    zip(a, b, |a, b| a + b)
}

pub(super) fn sub(a: V, b: V) -> V {
    zip(a, b, |a, b| a - b)
}

/// Interpolator phases of the four-times upsampler (Fuzz, Distortion, Cat, Twang, AC Box).
pub(super) const PHASE: [V; 4] = [
    [0.0, 1.0, 0.0, 0.0],
    [-0.070_312_5, 0.867_187_5, 0.226_562_5, -0.023_437_5],
    [-0.062_5, 0.562_5, 0.562_5, -0.062_5],
    [-0.023_437_5, 0.226_562_5, 0.867_187_5, -0.070_312_5],
];

/// Big Fuzz's interpolator phases (its own upsampler matrix).
pub(super) const PHASE_BIG_FUZZ: [V; 4] = [
    [0.0, 1.0, 0.0, 0.0],
    [-0.140_625, 0.867_187_5, 0.226_562_5, -0.023_437_5],
    [-0.125, 0.562_5, 0.562_5, -0.062_5],
    [-0.046_875, 0.226_562_5, 0.867_187_5, -0.070_312_5],
];

/// Decimation taps (all amp models). The native matrix has signed zeros in two places
/// where the others have +0; they only change the sign of a zero product and never a value.
pub(super) const DECIMATE: [V; 4] = [
    [-0.005_859_375, -0.015_625, -0.017_578_125, 0.0],
    [0.056_640_625, 0.140_625, 0.216_796_875, 0.25],
    [0.216_796_875, 0.140_625, 0.056_640_625, 0.0],
    [-0.017_578_125, -0.015_625, -0.005_859_375, 0.0],
];

/// Lookup of a shaper table with linear interpolation, `tab` holding (value, slope) pairs for
/// each integer step of `lo..=hi`: index `i = u clamped - lo`, fraction against the table grid.
pub(super) fn lookup(tab: &[f32], u: V, lo: f32, hi: f32) -> V {
    std::array::from_fn(|l| {
        let i = (u[l].max(lo).min(hi) - lo) as i32;
        let fr = u[l] - (i as f32 + lo);
        let i = i as usize * 2;
        fr * tab[i + 1] + tab[i]
    })
}
