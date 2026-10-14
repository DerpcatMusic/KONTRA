//! Sample math shared by scalar and lane layouts; callers own timing and state.
#[inline(always)]
pub(super) fn gain(input: f64, gain: f64) -> f64 {
    input * gain
}

#[inline(always)]
pub(super) fn matrix([l, r]: [f64; 2], m: [[f64; 2]; 2]) -> [f64; 2] {
    [m[0][0] * l + m[0][1] * r, m[1][0] * l + m[1][1] * r]
}

#[inline(always)]
pub(super) fn one_pole32(state: f32, input: f32, coefficient: f32) -> f32 {
    state + (input - state) * coefficient
}

#[inline(always)]
pub(super) fn gainer(current: f32, target: f32, dry: f64, k: f32) -> (f64, f32) {
    (dry + f64::from(current), one_pole32(current, target, k))
}

#[inline(always)]
pub(super) fn mix_gains(dry: f64, wet: f64, bypass: f64) -> [f64; 2] {
    [dry * (1. - bypass) + bypass, wet * (1. - bypass)]
}

#[inline(always)]
pub(super) fn mix(input: f64, wet: f64, [direct, through]: [f64; 2], off: bool) -> f64 {
    let wet_part = if off { 0. } else { through * wet };
    direct * input + wet_part
}

/// The legacy cutoff table (`0x14156bbe0`, index `400 + 400·c` at
/// `0x142819740`) and the RBJ design with resonance 0 (`0x1412dc110`
/// low-pass for `c < 0`, `0x141336700` high-pass for `c > 0`), fixed
/// 22050 Hz scale: `b0 b1 b2 a1 a2`, or `None` at 0.
pub(crate) fn uvi_legacy_cutoff(c: f32, rate: f32) -> Option<[f32; 5]> {
    if c == 0.0 || !c.is_finite() {
        return None;
    }
    let idx = (400 + (400.0 * c.clamp(-1.0, 1.0)) as i32) as f32;
    let (hz, high) = if c < 0.0 {
        (22_050.0 * (0.995 * (idx / 400.0).powi(2) + 0.005).clamp(0.001, 0.999), false)
    } else {
        (22_050.0 * (0.3 * ((idx - 400.0) / 400.0).powi(4)).clamp(0.001, 0.999), true)
    };
    let w = 2.0 * std::f32::consts::PI * (hz / rate).min(0.499);
    let (sn, cs) = w.sin_cos();
    let a0 = 1.0 + sn;
    let (b0, b1) = if high { ((1.0 + cs) / 2.0, -(1.0 + cs)) } else { ((1.0 - cs) / 2.0, 1.0 - cs) };
    Some([b0 / a0, b1 / a0, b0 / a0, -2.0 * cs / a0, (1.0 - sn) / a0])
}
