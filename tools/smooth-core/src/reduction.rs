//! Selected finite reductions from the clean mathematical contract. No runtime-dispatch claim.

/// Actual second-array / sum-input phase in float32 elements modulo four.
pub fn phase32(values: &[f32]) -> usize {
    (values.as_ptr() as usize / std::mem::size_of::<f32>()) % 4
}

/// Zero length is an error; callers keep their initialized destination on that error.
pub fn dot32(x: &[f32], y: &[f32], phase: usize) -> Result<f32, &'static str> {
    if x.is_empty() || x.len() != y.len() || phase > 3 {
        return Err("unsupported dot length or phase");
    }
    let n = x.len();
    let result = if n <= 15 {
        let (mut even, mut odd) = (0.0_f32, 0.0_f32);
        let mut k = 0;
        while k + 2 <= n {
            let p0 = x[k] * y[k];
            let p1 = x[k + 1] * y[k + 1];
            even += p0;
            odd += p1;
            k += 2;
        }
        if k < n {
            let last = x[k] * y[k];
            odd += last; // Source short odd tail belongs to O, deliberately.
        }
        odd + even
    } else {
        let mut a = [[0.0_f32; 4]; 4];
        let mut k = (4 - phase) % 4;
        for i in 0..k {
            let product = x[i] * y[i];
            a[0][0] += product;
        }
        while k + 16 <= n {
            for bank in 0..4 {
                for lane in 0..4 {
                    let i = k + bank * 4 + lane;
                    let product = x[i] * y[i];
                    a[bank][lane] += product;
                }
            }
            k += 16;
        }
        while k + 2 <= n {
            let p0 = x[k] * y[k];
            let p1 = x[k + 1] * y[k + 1];
            a[0][0] += p0;
            a[0][1] += p1;
            a[0][2] += 0.0;
            a[0][3] += 0.0;
            k += 2;
        }
        if k < n {
            let product = x[k] * y[k];
            a[0][0] += product;
        }
        let v: [f32; 4] = std::array::from_fn(|i| {
            let left = a[0][i] + a[1][i];
            let right = a[2][i] + a[3][i];
            left + right
        });
        let low = v[0] + v[2];
        let high = v[1] + v[3];
        high + low
    };
    if result.is_finite() {
        Ok(result)
    } else {
        Err("nonfinite dot result")
    }
}

fn sum_part(x: &[f32]) -> [f32; 2] {
    let mut a = [[0.0_f32; 4]; 4];
    let mut k = 0;
    while k + 16 <= x.len() {
        for bank in 0..4 {
            for lane in 0..4 {
                a[bank][lane] += x[k + bank * 4 + lane];
            }
        }
        k += 16;
    }
    let mut v: [f32; 4] = std::array::from_fn(|i| {
        let left = a[0][i] + a[2][i];
        let right = a[1][i] + a[3][i];
        left + right // Crossed tree differs from Dot32.
    });
    while k + 4 <= x.len() {
        for lane in 0..4 {
            v[lane] += x[k + lane];
        }
        k += 4;
    }
    if k + 2 <= x.len() {
        v[0] += x[k];
        v[1] += x[k + 1];
        v[2] += 0.0;
        v[3] += 0.0;
        k += 2;
    }
    if k < x.len() {
        v[0] += x[k];
    }
    [v[2] + v[0], v[3] + v[1]]
}

pub fn accurate_sum32(x: &[f32], phase: usize) -> Result<f32, &'static str> {
    if x.is_empty() || phase > 3 {
        return Err("unsupported accurate sum length or phase");
    }
    let mut k = x.len().min((4 - phase) % 4);
    let (mut low, mut high) = (0.0_f64, 0.0_f64);
    for &value in &x[..k] {
        low += value as f64;
    }
    let mut full_boundary = false;
    while k < x.len() {
        let length = (x.len() - k).min(32768);
        let [l, h] = sum_part(&x[k..k + length]);
        low += l as f64;
        high += h as f64;
        k += length;
        full_boundary = length == 32768;
    }
    if full_boundary {
        let [l, h] = sum_part(&[]); // Preserve the exact full-chunk zero-part call.
        low += l as f64;
        high += h as f64;
    }
    let result = (low + high) as f32;
    if result.is_finite() {
        Ok(result)
    } else {
        Err("nonfinite accurate sum result")
    }
}

pub fn subtract32(destination: &mut [f32], source: &[f32]) {
    assert_eq!(destination.len(), source.len());
    for (value, &other) in destination.iter_mut().zip(source) {
        *value -= other;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clean_reduction_cancellation_phase_tail_and_chunk_fixtures() {
        let mut x = [0.0_f32; 16];
        x[0] = 1.0e8;
        x[4] = -1.0e8;
        x[8] = 1.0;
        x[12] = 1.0;
        for (phase, bits) in [0x40000000, 0, 0x40000000, 0].into_iter().enumerate() {
            assert_eq!(dot32(&x, &[1.0; 16], phase).unwrap().to_bits(), bits);
            assert_eq!(accurate_sum32(&x, phase).unwrap().to_bits(), 0);
        }
        assert_eq!(dot32(&[1.0e8, 1.0, -1.0e8], &[1.0; 3], 0).unwrap(), 0.0);
        assert_eq!(accurate_sum32(&[1.0e8, 1.0, -1.0e8], 0).unwrap(), 1.0);
        for (length, bits) in [
            (32767, 0x46fffe00),
            (32768, 0x47000000),
            (32769, 0x47000100),
        ] {
            assert_eq!(
                accurate_sum32(&vec![1.0; length], 0).unwrap().to_bits(),
                bits
            );
        }
        assert!(dot32(&[], &[], 0).is_err());
        assert!(accurate_sum32(&[], 0).is_err());
        let mut destination = [3.0, -1.0];
        subtract32(&mut destination, &[1.0, 2.0]);
        assert_eq!(destination, [2.0, -3.0]);
    }
}
