//! Replika mode-1 deposit (`141accfb0`, 337 B): writes a quadratic window into the ring behind the write head.
//!
//! Native (decompile `0141ac0000.c`, listing `docs/re/engine/disasm/replika-3b-listings.txt`): the object shifts a
//! three-sample history (`+0x84` x_n, `+0x88` x_(n-1), `+0x8c` x_(n-2)) and then, for `i` in `0..count`, overwrites
//! the ring tap `(write - i) & mask` with `(t A + B) t + x_n`, where `t = -(i + off)` and A, B are built from the
//! history and the rates a = -`+0x90`, b = `+0x98`. The coefficient order is the native f32 order.
//!
//! Not ported: the driver that writes `write`, `off` and `count` (`141ab8c60`).
//!
//! Vectors: `docs/re/engine/vectors/replika-native-deposit.json` (kemu, 3 seeds, ring snapshots per call).

/// One deposit object. Field comments give the offsets in the kernel object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Deposit {
    /// `+0x08`: write index.
    pub write: i32,
    /// `+0x10`: sub-sample offset.
    pub off: f64,
    /// `+0x18`: ring mask.
    pub mask: i32,
    /// `+0x1c`: taps written per call.
    pub count: i32,
    /// `+0x84`, `+0x88`, `+0x8c`: x_n, x_(n-1), x_(n-2).
    pub hist: [f32; 3],
    /// `+0x90` (a) and `+0x98` (b): the rates; the node position is -a.
    pub a: f64,
    pub b: f64,
}

impl Deposit {
    /// `141accfb0` with input `x`: shifts the history and writes `count` taps of `ring`.
    pub fn push(&mut self, ring: &mut [f32], x: f32) {
        let s84 = self.hist[0];
        let s88 = self.hist[1];
        self.hist = [x, s84, s88];

        let v7 = (-self.a) as f32;
        let v5 = (f64::from(v7) - self.b) as f32;
        let v6 = x / (v5 * v7);
        let v10 = s84 / ((v7 - v5) * v7);
        let v9 = -((v5 + v7) * v6);
        let v8 = s88 / ((v5 - v7) * v5);
        let sum = v10 + v6 + v8;
        let rest = (v9 - v10 * v5) - v8 * v7;

        for i in 0..self.count {
            let slot = (self.write - i) & self.mask;
            let t = (-(f64::from(i) + self.off)) as f32;
            ring[slot as usize] = (t * sum + rest) * t + x;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn vector() -> Value {
        let path = format!(
            "{}/../../docs/re/engine/vectors/replika-native-deposit.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("vector file"))
            .expect("vector json")
    }

    fn words(v: &Value) -> Vec<u32> {
        v.as_array()
            .expect("words")
            .iter()
            .map(|w| w.as_u64().expect("u32") as u32)
            .collect()
    }

    fn f64_at(w: &[u32], off: usize) -> f64 {
        f64::from_bits(u64::from(w[off / 4 + 1]) << 32 | u64::from(w[off / 4]))
    }

    /// Relative to the magnitude of the expected value (the ring reaches tens in these vectors).
    fn close(label: &str, got: f32, want: f32) {
        let tol = 1e-5 * want.abs().max(1.0);
        assert!((got - want).abs() <= tol, "{label}: got {got} want {want}");
    }

    #[test]
    fn deposit_matches_native_vectors() {
        let doc = vector();
        for case in doc["cases"].as_array().expect("cases") {
            let init = words(&case["init_words"]);
            let mut dep = Deposit {
                write: init[0x08 / 4] as i32,
                off: f64_at(&init, 0x10),
                mask: init[0x18 / 4] as i32,
                count: init[0x1c / 4] as i32,
                hist: [0.0; 3],
                a: f64_at(&init, 0x90),
                b: f64_at(&init, 0x98),
            };
            let mut ring: Vec<f32> = words(&case["ring0"])
                .into_iter()
                .map(f32::from_bits)
                .collect();
            let xs = case["xs"].as_array().expect("xs");
            for (k, (x, snapshot)) in xs
                .iter()
                .zip(case["ring_steps"].as_array().unwrap())
                .enumerate()
            {
                dep.push(&mut ring, x.as_f64().expect("x") as f32);
                for (i, want) in words(snapshot).into_iter().enumerate() {
                    close(
                        &format!("seed {} call {k} ring[{i}]", case["seed"]),
                        ring[i],
                        f32::from_bits(want),
                    );
                }
            }
            let end = words(&case["end_words"]);
            for (j, off) in [0x84, 0x88, 0x8c].into_iter().enumerate() {
                close("history", dep.hist[j], f32::from_bits(end[off / 4]));
            }
        }
    }
}
