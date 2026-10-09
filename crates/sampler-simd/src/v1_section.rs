//! Literal v1 0cb7a8a0:src/engine/filter.rs Section::process_body.
//! Only the typed planar f64 boundary is adapted; all recurrence work is f32.

pub fn process(c: [f32; 6], history: &mut [f32; 4], left: &mut [f64], right: &mut [f64]) {
    assert_eq!(left.len(), right.len());
    kernel::process_body(c, history, left, right);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_matches_baseline_from_every_dispatch_context() {
        eprintln!("section dispatch context: {:?}", crate::level());
        for c in [
            [0.91, 0.073, 0.006, 1., 0.22, 0.],
            [0.23, 0.37, 0.59, 1., -0.72, 0.11],
        ] {
            for len in [0, 1, 2, 3, 7, 31, 32, 33, 63, 64, 65, 256] {
                let mut baseline = ([0.11, -0.02, 0.21, -0.03], [0.; 256], [0.; 256]);
                let mut dispatched = baseline;
                for block in 0..3 {
                    for (i, (l, r)) in baseline.1.iter_mut().zip(&mut baseline.2).enumerate() {
                        *l = f64::from(((i + block * 3) as f32 * 0.13).sin() * 0.25);
                        *r = -*l;
                    }
                    dispatched.1 = baseline.1;
                    dispatched.2 = baseline.2;
                    kernel::process_body(
                        c,
                        &mut baseline.0,
                        &mut baseline.1[..len],
                        &mut baseline.2[..len],
                    );
                    crate::dispatch(
                        #[inline(always)]
                        || {
                            process(
                                c,
                                &mut dispatched.0,
                                &mut dispatched.1[..len],
                                &mut dispatched.2[..len],
                            )
                        },
                    );
                    assert_eq!(baseline.0.map(f32::to_bits), dispatched.0.map(f32::to_bits));
                    assert_eq!(baseline.1.map(f64::to_bits), dispatched.1.map(f64::to_bits));
                    assert_eq!(baseline.2.map(f64::to_bits), dispatched.2.map(f64::to_bits));
                }
            }
        }
    }
}

#[allow(unsafe_code)]
mod kernel {
    // Keep v1's SSE2 operation order outside callers' AVX2/FMA dispatch contexts.
    #[inline(never)]
    pub(super) fn process_body(
        c: [f32; 6],
        history: &mut [f32; 4],
        left: &mut [f64],
        right: &mut [f64],
    ) {
        let [a1, a2, a3, m0, m1, m2] = c;
        let [mut l1, mut l2, mut r1, mut r2] = *history;
        // The loop is latency-bound. The state update `s' = 2v - s` is
        // expanded so each sample's dependency chain is a subtract, a
        // multiply and an add.
        let (b1, b2, b3) = (2.0 * a1 - 1.0, 2.0 * a2, 2.0 * a3);
        #[cfg(target_arch = "x86_64")]
        {
            // As a state-space system per channel, `s' = s + E s + B x` and
            // `y = C s + D x`, which steps two frames as
            // `s'' = s + (E² + 2E) s + (B + E B) x + B x'`. The two low lanes
            // of an SSE register step left and right one frame, the high two
            // step them two frames from the same state, so the loop-carried
            // chain (a multiply and two adds) runs once per two frames.
            // Increments on `s` rather than `A = I + E` keep low cutoffs,
            // where `E` is tiny, as exact as the one-frame update.
            use std::arch::x86_64::*;
            let (e11, e12, e21, e22) = (b1 - 1.0, -b2, b2, -b3);
            let f11 = e11 * e11 + e12 * e21 + 2.0 * e11;
            let f12 = e11 * e12 + e12 * e22 + 2.0 * e12;
            let f21 = e21 * e11 + e22 * e21 + 2.0 * e21;
            let f22 = e21 * e12 + e22 * e22 + 2.0 * e22;
            let (g1, g2) = (b2 + e11 * b2 + e12 * b3, b3 + e21 * b2 + e22 * b3);
            let (c1, c2, d) = (
                m1 * a1 + m2 * a2,
                m2 * (1.0 - a3) - m1 * a2,
                m0 + m1 * a2 + m2 * a3,
            );
            // SAFETY: SSE2 is baseline on x86_64; stores cover four local f32 values.
            // Slice chunks bound every planar input read.
            unsafe {
                let pair = |one: f32, two: f32| _mm_setr_ps(one, one, two, two);
                let (p11, p12, p21, p22) = (
                    pair(e11, f11),
                    pair(e12, f12),
                    pair(e21, f21),
                    pair(e22, f22),
                );
                let (q1, q2, r1_, r2_) = (pair(b2, g1), pair(b3, g2), pair(0.0, b2), pair(0.0, b3));
                let (c1, c2, d) = (_mm_set1_ps(c1), _mm_set1_ps(c2), _mm_set1_ps(d));
                let (mut s1, mut s2) = (_mm_setr_ps(l1, r1, l1, r1), _mm_setr_ps(l2, r2, l2, r2));
                // Both states from `s` and the inputs' terms `u`.
                let step = |s1: __m128, s2: __m128, u1: __m128, u2: __m128| {
                    let t1 = _mm_add_ps(
                        _mm_add_ps(_mm_mul_ps(p11, s1), u1),
                        _mm_add_ps(_mm_mul_ps(p12, s2), s1),
                    );
                    let t2 = _mm_add_ps(
                        _mm_add_ps(_mm_mul_ps(p21, s1), u2),
                        _mm_add_ps(_mm_mul_ps(p22, s2), s2),
                    );
                    (t1, t2)
                };
                let out = |s1: __m128, s2: __m128, x: __m128| {
                    _mm_add_ps(
                        _mm_add_ps(_mm_mul_ps(c1, s1), _mm_mul_ps(c2, s2)),
                        _mm_mul_ps(d, x),
                    )
                };
                let n = left.len().min(right.len());
                let (ls, l_rest) = left[..n].as_chunks_mut::<2>();
                let (rs, r_rest) = right[..n].as_chunks_mut::<2>();
                for (l, r) in ls.iter_mut().zip(rs) {
                    // Adapt the prepared v2 planar f64 boundary to v1's f32 input.
                    let x = _mm_setr_ps(l[0] as f32, r[0] as f32, l[1] as f32, r[1] as f32);
                    let (x0, x1) = (_mm_movelh_ps(x, x), _mm_movehl_ps(x, x));
                    let u1 = _mm_add_ps(_mm_mul_ps(q1, x0), _mm_mul_ps(r1_, x1));
                    let u2 = _mm_add_ps(_mm_mul_ps(q2, x0), _mm_mul_ps(r2_, x1));
                    let (t1, t2) = step(s1, s2, u1, u2);
                    // States at both frames, then the outputs [l0, l1, r0, r1].
                    let y = out(_mm_movelh_ps(s1, t1), _mm_movelh_ps(s2, t2), x);
                    let y = _mm_shuffle_ps(y, y, 0b11_01_10_00);
                    let mut output = [0.0; 4];
                    _mm_storeu_ps(output.as_mut_ptr(), y);
                    (l[0], l[1], r[0], r[1]) = (
                        f64::from(output[0]),
                        f64::from(output[1]),
                        f64::from(output[2]),
                        f64::from(output[3]),
                    );
                    (s1, s2) = (_mm_movehl_ps(t1, t1), _mm_movehl_ps(t2, t2));
                }
                // An odd last frame steps once, on the low lanes.
                if let (Some(l), Some(r)) = (l_rest.first_mut(), r_rest.first_mut()) {
                    let x = _mm_setr_ps(*l as f32, *r as f32, 0.0, 0.0);
                    let x0 = _mm_movelh_ps(x, x);
                    let (t1, t2) = step(s1, s2, _mm_mul_ps(q1, x0), _mm_mul_ps(q2, x0));
                    let y = out(s1, s2, x);
                    let mut output = [0.0; 4];
                    _mm_storeu_ps(output.as_mut_ptr(), y);
                    (*l, *r) = (f64::from(output[0]), f64::from(output[1]));
                    (s1, s2) = (_mm_movelh_ps(t1, t1), _mm_movelh_ps(t2, t2));
                }
                let (mut a, mut b) = ([0.0; 4], [0.0; 4]);
                _mm_storeu_ps(a.as_mut_ptr(), s1);
                _mm_storeu_ps(b.as_mut_ptr(), s2);
                (l1, r1, l2, r2) = (a[0], a[1], b[0], b[1]);
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (x, y) = (*l as f32, *r as f32);
            let v3 = x - l2;
            let v1 = a1 * l1 + a2 * v3;
            let v2 = l2 + a2 * l1 + a3 * v3;
            (l1, l2) = (b1 * l1 + b2 * v3, l2 + b2 * l1 + b3 * v3);
            *l = f64::from(m0 * x + m1 * v1 + m2 * v2);
            let v3 = y - r2;
            let v1 = a1 * r1 + a2 * v3;
            let v2 = r2 + a2 * r1 + a3 * v3;
            (r1, r2) = (b1 * r1 + b2 * v3, r2 + b2 * r1 + b3 * v3);
            *r = f64::from(m0 * y + m1 * v1 + m2 * v2);
        }
        // Flush denormals once per call: decaying states would otherwise slow down.
        *history = [l1, l2, r1, r2].map(|v| if v.abs() < 1e-20 { 0.0 } else { v });
    }
}
