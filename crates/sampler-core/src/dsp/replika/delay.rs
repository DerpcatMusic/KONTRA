//! Replika delay position leaf of the mode-1 head (`141abd0b0`, 214 B).
//!
//! `set(t, s)` sets the two delay positions of one object: `mid = (hi - lo) t + lo` (f32), `dl = (mode s) 0.5` (f32),
//! `a = max(mid + dl, 0)` at `+0xc8` and `b = max(mid - dl, 0)` at `+0xd0`. When `a` (`b`) changes it also stores
//! `+0x58 = base / a` (`+0x60 = base / b`, base at `+0x48`) and sets the flag `+0x90`. The native LOCK/UNLOCK pairs
//! are plain stores here (single writer on the audio side).
//!
//! Not ported: the three-object caller `141abd290` (GAP-M), which needs its constants and the live/stored source.
//!
//! Vectors: `docs/re/engine/vectors/replika-native-delay-pos.json` (kemu, 3 seeds, 40 calls each).

/// One delay position object. Field comments give the offsets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DelayPos {
    /// `+0x08` lo and `+0x0c` hi (the range the position moves over).
    pub lo: i32,
    pub hi: i32,
    /// `+0x28`: mode (scales the spread).
    pub mode: i32,
    /// `+0x48`: base; `+0x58` = base / a, `+0x60` = base / b.
    pub base: f64,
    pub r58: f64,
    pub r60: f64,
    /// `+0xc8` a and `+0xd0` b: the two positions.
    pub a: f64,
    pub b: f64,
    /// `+0x90`: set when a position changes.
    pub flag: bool,
}

impl DelayPos {
    /// `141abd0b0` with (t, s).
    pub fn set(&mut self, t: f32, s: f32) {
        let mid = f64::from((self.hi - self.lo) as f32 * t + self.lo as f32);
        let dl = f64::from((self.mode as f32 * s) * 0.5);
        let a = clamp_zero(dl + mid);
        let b = clamp_zero(mid - dl);
        if self.a != a {
            self.a = a;
            self.r58 = self.base / a;
            self.flag = true;
        }
        if self.b != b {
            self.b = b;
            self.r60 = self.base / b;
            self.flag = true;
        }
    }
}

fn clamp_zero(v: f64) -> f64 {
    if v <= 0.0 { 0.0 } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const TOLERANCE: f32 = 1e-5;

    fn vector() -> Value {
        let path = format!(
            "{}/../../docs/re/engine/vectors/replika-native-delay-pos.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("vector file"))
            .expect("vector json")
    }

    /// Relative to the magnitude of the expected value (positions reach about 1300).
    fn close(label: &str, got: f64, want: f64) {
        let tol = f64::from(TOLERANCE) * want.abs().max(1.0);
        assert!((got - want).abs() <= tol, "{label}: got {got} want {want}");
    }

    #[test]
    fn delay_positions_match_native_vectors() {
        let doc = vector();
        for case in doc["cases"].as_array().expect("cases") {
            let i = &case["init"];
            let mut p = DelayPos {
                lo: i["lo"].as_i64().unwrap() as i32,
                hi: i["hi"].as_i64().unwrap() as i32,
                mode: i["mode"].as_i64().unwrap() as i32,
                base: i["base"].as_f64().unwrap(),
                r58: i["r58"].as_f64().unwrap(),
                r60: i["r60"].as_f64().unwrap(),
                a: i["a"].as_f64().unwrap(),
                b: i["b"].as_f64().unwrap(),
                flag: false,
            };
            for (k, c) in case["calls"].as_array().unwrap().iter().enumerate() {
                let t = c["t"].as_f64().unwrap() as f32;
                let s = c["s"].as_f64().unwrap() as f32;
                p.set(t, s);
                let label = format!("seed {} call {k}", case["seed"]);
                close(&format!("{label} a"), p.a, c["a"].as_f64().unwrap());
                close(&format!("{label} b"), p.b, c["b"].as_f64().unwrap());
                close(&format!("{label} r58"), p.r58, c["r58"].as_f64().unwrap());
                close(&format!("{label} r60"), p.r60, c["r60"].as_f64().unwrap());
                assert_eq!(p.flag, c["flag"].as_u64().unwrap() != 0, "{label} flag");
            }
        }
    }
}
