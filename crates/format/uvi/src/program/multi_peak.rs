//! The default MultiLFO peak candidates, native 0x14114a7a0.

use std::cell::RefCell;
use std::collections::HashMap;

pub(super) fn peak(s: f32, t: f32, w: f32, q: f32, width: f32) -> f32 {
    let (sd, td, wd, qd, widthd) = (
        f64::from(s),
        f64::from(t),
        f64::from(w),
        f64::from(q),
        f64::from(width),
    );
    let at = |p: f64| {
        let tri = if p < 0.5 {
            1.0 - 4.0 * p
        } else {
            4.0 * p - 3.0
        };
        let pulse = if p < widthd { 1.0 } else { -1.0 };
        let saw = if p < 0.5 {
            1.0 - 4.0 * p
        } else {
            3.0 - 4.0 * p
        };
        sd * (std::f64::consts::TAU * p).sin() + td * tri + wd * saw + qd * pulse
    };
    // These are the native candidate phases, not the true continuous maximum.
    let extrema = |numerator: f64| {
        let phase = (numerator / (std::f64::consts::TAU * sd)).acos() / std::f64::consts::TAU;
        if phase.is_nan() {
            0.0
        } else {
            phase.clamp(0.0, 1.0)
        }
    };
    thread_local! {
        static PEAKS: RefCell<HashMap<[u32; 5], f32>> = RefCell::new(HashMap::new());
    }
    let key = [s, t, w, q, width].map(f32::to_bits);
    PEAKS.with_borrow_mut(|peaks| {
        *peaks.entry(key).or_insert_with(|| {
            let a = extrema(4.0 * (td + wd));
            let b = extrema(4.0 * (wd - td));
            [
                0.0,
                1.0 - a,
                widthd - 0.001,
                widthd + 0.001,
                0.499,
                0.501,
                b,
                1.0 - b,
                0.999,
                0.0,
            ]
            .into_iter()
            .map(|p| at(p).abs())
            .fold(0.0, f64::max) as f32
        })
    })
}
