//! The reference's plan geometry: how many groups it runs and how many samples
//! of delay it reports, as the constructor (0x88fa90) and getLatency (0x88f610)
//! compute them.
//!
//! Evidence: `research/re-latency-fft-scale.md` sections 12-17 and the oracle
//! rows in `research/oracle/`. Groups and the reported latency are checked
//! against those rows; the impulse delay of the process block (0x890a00)
//! equals this latency at every rate and both zero-latency states.
//!
//! This is not wired into `Engine` yet. The engine still delays by its own
//! `n - 1`, so reporting this latency would move PDC away from the audio.

/// Latency per group: `f(this+0x5f8)` in getLatency (0x894d20). It is 2304 at
/// every rate and both analysis modes in the oracle.
pub const GROUP_LATENCY: usize = 2304;

/// The reference's geometry for one sample rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReferencePlan {
    /// `max(1, trunc(rate / 44100))`.
    pub groups: usize,
    /// Bin/filter vector count, `48 * groups` (`obj+0x248`).
    pub vec_count: usize,
    /// Base latency: `48 * groups` for two or more groups, else 0.
    pub base: usize,
    /// The zero-latency byte (`obj+0x237` or `obj+0xab`) is set. Latency is
    /// then the base alone.
    pub zero_latency: bool,
}

impl ReferencePlan {
    pub fn new(sample_rate: f64, zero_latency: bool) -> Self {
        let groups = ((sample_rate / 44_100.0) as usize).max(1);
        let base = if groups >= 2 { 48 * groups } else { 0 };
        Self {
            groups,
            vec_count: 48 * groups,
            base,
            zero_latency,
        }
    }

    /// The delay the reference reports, and the delay its impulse comes out at.
    pub fn latency(&self) -> usize {
        if self.zero_latency {
            self.base
        } else {
            self.groups * GROUP_LATENCY + self.base
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (rate, groups, vec_count, base, latency) from `research/oracle/ctor_latency_mode4.json`.
    /// The analysis-mode-1 file gives the same rows at the rates it has.
    const CONSTRUCTOR_ROWS: [(f64, usize, usize, usize, usize); 9] = [
        (44_100.0, 1, 48, 0, 2304),
        (48_000.0, 1, 48, 0, 2304),
        (64_000.0, 1, 48, 0, 2304),
        (88_200.0, 2, 96, 96, 4704),
        (96_000.0, 2, 96, 96, 4704),
        (110_250.0, 2, 96, 96, 4704),
        (132_300.0, 3, 144, 144, 7056),
        (176_400.0, 4, 192, 192, 9408),
        (192_000.0, 4, 192, 192, 9408),
    ];

    /// (rate, zero_latency, latency) from `research/oracle/process_block_<rate>.json`:
    /// the impulse delay of the process block equals this at each rate.
    const IMPULSE_ROWS: [(f64, bool, usize); 8] = [
        (44_100.0, false, 2304),
        (44_100.0, true, 0),
        (48_000.0, false, 2304),
        (48_000.0, true, 0),
        (96_000.0, false, 4704),
        (96_000.0, true, 96),
        (192_000.0, false, 9408),
        (192_000.0, true, 192),
    ];

    /// Groups, vector count, base and latency at each rate in the constructor oracle.
    #[test]
    fn geometry_matches_the_constructor_rows() {
        for (rate, groups, vecs, base, latency) in CONSTRUCTOR_ROWS {
            let plan = ReferencePlan::new(rate, false);
            assert_eq!(plan.groups, groups, "groups at {rate}");
            assert_eq!(plan.vec_count, vecs, "vec_count at {rate}");
            assert_eq!(plan.base, base, "base at {rate}");
            assert_eq!(plan.latency(), latency, "latency at {rate}");
        }
    }

    #[test]
    fn reported_latency_matches_the_process_block_impulse_delay() {
        for (rate, zero, latency) in IMPULSE_ROWS {
            assert_eq!(
                ReferencePlan::new(rate, zero).latency(),
                latency,
                "latency at {rate}, zero_latency={zero}"
            );
        }
    }
}
