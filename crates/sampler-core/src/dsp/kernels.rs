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

// Callers retain multiplication order (Lo-Fi puts its coefficient first).
#[inline(always)]
pub(super) fn biased_one_pole32(state: f32, increment: f32) -> f32 {
    state + (increment + 1e-20)
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
