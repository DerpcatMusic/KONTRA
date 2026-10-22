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

/// `144f38790`: the spread below which the caller zeroes the spread and the depth.
const SPREAD_ZERO: f64 = f64::from_bits(0x3ee4_f8b5_88e3_68f1);
/// `14756bdf8`: the spread clamp (4000).
const SPREAD_MAX: f64 = f64::from_bits(0x40af_4000_0000_0000);
/// `1479ab4e8` and `1479ab508`: the skew map `125 - (1 - skew) 250`.
const SKEW_BASE: f64 = f64::from_bits(0x405f_4000_0000_0000);
const SKEW_SLOPE: f64 = f64::from_bits(0x406f_4000_0000_0000);
/// `144762eb8`: one third, the ratio's weight in the head positions.
const THIRD: f64 = f64::from_bits(0x3fd5_5555_5555_5555);
/// `144f38910`: the lower clamp of the limiter parameter (1/12).
const LIMITER_FLOOR: f64 = f64::from_bits(0x3fb5_5555_5555_5555);

/// The spread inputs of the caller `141abd290`: the width, skew, depth and ratio of the live source (`+0x2c`,
/// `+0x34`, `+0x38`, `+0x30` of the source object) or of the stored doubles (`C+0x14210`, `+0x14238`, `+0x14288`,
/// `+0x14260`). Which one is used is set by `C+0x938`; the caller reads it, so it is not part of this struct.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spread {
    pub width: f64,
    pub skew: f64,
    pub depth: f64,
    pub ratio: f64,
}

/// What the caller sends on: `(t, s)` for heads 1, 2 and 3, and parameter 1 of the limiter `C+0x13f80`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Positions {
    pub heads: [(f32, f32); 3],
    pub limiter: f32,
}

/// `141abd290`: the head delay positions from the spread, the bounds `[lower, upper]` (`C+0xad8`, `C+0xad0`), the flag
/// `C+0x600` and the path `C+0x604`. Path 2 and path 3 also run `141aca380` and `141acb160`, which are not ported,
/// so they return `None`. The flag 1 or 2 mutes the depth terms (heads 1 and 2 and head 3 get s = 0).
/// GAP-B: the bounds are read as doubles at `+0xad0`/`+0xad8`, which alias an int table; the port takes the
/// values as given.
pub fn positions(sp: Spread, bounds: [f64; 2], flag: i32, path: i32) -> Option<Positions> {
    if path == 2 || path == 3 {
        return None;
    }
    let (mut width, mut skew, mut depth) = (sp.width, sp.skew, sp.depth);
    if width <= SPREAD_ZERO {
        depth = 0.;
        width = 0.;
        skew = 0.;
    } else {
        if width >= SPREAD_MAX {
            width = SPREAD_MAX;
        }
        skew = SKEW_BASE - (1. - skew) * SKEW_SLOPE;
    }
    let [lo, hi] = bounds;
    let k5 = sp.ratio * THIRD + 1.;
    let mut d8 = if lo <= width { width } else { lo };
    if hi <= d8 {
        d8 = hi;
    }
    let mut d6 = k5 * width + skew;
    if d6 <= lo {
        d6 = lo;
    }
    d8 = (d8 - lo) / (hi - lo);
    if hi <= d6 {
        d6 = hi;
    }
    let d5 = k5 * d8;
    d8 -= sp.ratio * THIRD * d8;

    let muted = flag == 1 || flag == 2;
    let (s1, s2, s3) = if muted {
        (0., 0., 0.)
    } else {
        ((depth * -0.5) as f32, depth as f32, (-depth) as f32)
    };
    let mut limiter = d5;
    if limiter <= d8 {
        limiter = d8;
    }
    if limiter <= LIMITER_FLOOR {
        limiter = LIMITER_FLOOR;
    }
    Some(Positions {
        heads: [
            (((d6 - lo) / (hi - lo)) as f32, s1),
            (d8 as f32, s2),
            (d5 as f32, s3),
        ],
        limiter: limiter as f32,
    })
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

    fn caller_vector() -> serde_json::Value {
        let path = format!(
            "{}/../../docs/re/engine/vectors/replika-native-delay-caller.json",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("vector file"))
            .expect("vector json")
    }

    #[test]
    fn caller_matches_native_vectors() {
        use serde_json::Value;
        let doc = caller_vector();
        let num = |v: &Value, k: &str| v[k].as_f64().unwrap_or_else(|| panic!("field {k}"));
        for case in doc["cases"].as_array().expect("cases") {
            let tag = format!(
                "seed {} path {} flag {}",
                case["seed"], case["path"], case["ctl600"]
            );
            let sp = Spread {
                width: num(case, "spread"),
                skew: num(case, "skew"),
                depth: num(case, "depth"),
                ratio: num(case, "ratio"),
            };
            let bounds = case["bounds"].as_array().expect("bounds");
            let bounds = [bounds[0].as_f64().unwrap(), bounds[1].as_f64().unwrap()];
            let pos = positions(
                sp,
                bounds,
                num(case, "ctl600") as i32,
                num(case, "path") as i32,
            )
            .expect("paths 0, 1 and 4");
            let mut heads: Vec<DelayPos> = case["heads_in"]
                .as_array()
                .expect("heads")
                .iter()
                .map(|h| DelayPos {
                    lo: num(h, "lo") as i32,
                    hi: num(h, "hi") as i32,
                    mode: num(h, "mode") as i32,
                    base: num(h, "base"),
                    r58: num(h, "r58"),
                    r60: num(h, "r60"),
                    a: num(h, "a"),
                    b: num(h, "b"),
                    flag: num(h, "flag") != 0.0,
                })
                .collect();
            for (h, &(t, s)) in heads.iter_mut().zip(pos.heads.iter()) {
                h.set(t, s);
            }
            for (i, (h, want)) in heads
                .iter()
                .zip(case["heads"].as_array().unwrap())
                .enumerate()
            {
                for (k, got) in [("a", h.a), ("b", h.b), ("r58", h.r58), ("r60", h.r60)] {
                    let w = num(want, k);
                    assert!(
                        (got - w).abs() <= 1e-5 * w.abs().max(1e-9),
                        "{tag} head {i} {k}: got {got} want {w}"
                    );
                }
                assert_eq!(
                    h.flag,
                    want["flag"].as_f64().unwrap() != 0.0,
                    "{tag} head {i} flag"
                );
            }
            let limiter = num(case, "limiter");
            assert!(
                (f64::from(pos.limiter) - limiter).abs() <= 1e-6 * limiter.abs().max(1e-6),
                "{tag} limiter: got {} want {limiter}",
                pos.limiter
            );
        }
    }
}
