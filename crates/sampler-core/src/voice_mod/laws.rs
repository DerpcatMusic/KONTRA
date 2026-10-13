//! Kontakt 8.13.1 modulation helper laws, binary32, in native operation order
//! (separate multiply/add, never fused). Source: wave-3 spec
//! `modulation-helpers.md`; addresses are VAs in Kontakt8.exe. Own code; the
//! open items are listed in [`UNSUPPORTED`] and deliberately not modelled.

/// Sign-bit flip of `x` when evaluator flag +0x6d bit 0x02 (`v`) is set
/// (fcinv_neg, 0x14071a7c0): keeps magnitude and payload, flips signed zero.
pub fn fcinv_neg(x: f32, v: bool) -> f32 {
    if v { f32::from_bits(x.to_bits() ^ 0x8000_0000) } else { x }
}
/// `1 - x` when the flag is set (fcinv_flip, 0x14071a760). No clamp.
pub fn fcinv_flip(x: f32, v: bool) -> f32 {
    if v { 1.0 - x } else { x }
}
/// `(x + 1) * 0.5` (frange_uni, 0x14071a880). No clamp.
pub fn frange_uni(x: f32) -> f32 {
    (x + 1.0) * 0.5
}
/// `2 * x - 1` (frange_bi, 0x14071a840). No clamp.
pub fn frange_bi(x: f32) -> f32 {
    2.0 * x - 1.0
}
/// `(x * x) * x` (fcubic, 0x14071a800). Not a saturator.
pub fn fcubic(x: f32) -> f32 {
    (x * x) * x
}
/// Clear the sign bit (fabs, 0x14071a730).
pub fn fabs(x: f32) -> f32 {
    f32::from_bits(x.to_bits() & 0x7fff_ffff)
}

/// One `ftable` register: enabled flag plus 129 slots (index 128 is read by the
/// native lookup even though its fraction is zero).
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub enabled: bool,
    pub t: [f32; 129],
}

impl Table {
    /// FUN_140741500 initial contents `T[k] = k/128` (1/128 = 0x144e79a00),
    /// replaced by compiler assignments afterwards. Not an identity over [0,1].
    pub fn initial() -> Self {
        Self { enabled: true, t: std::array::from_fn(|k| k as f32 / 128.0) }
    }
    /// Compiler-sampled curve (FUN_140873040 0x140874534..0x14087458a, 128
    /// samples taken at [`curve_position`]); `T[128] = T[127]` (0x140874a36..a67).
    pub fn from_curve_samples(samples: &[f32; 128]) -> Self {
        Self { enabled: true, t: std::array::from_fn(|k| samples[k.min(127)]) }
    }
    /// `ftable(register, x)` (FUN_14071add0, 0x14071add0..0x14071aea1).
    /// Disabled: 0. Else clamp to [0,1] (NaN and -inf -> 0, +inf -> 1), scale by
    /// 127 (not 128), linear between adjacent slots. Entries are not clamped.
    pub fn lookup(&self, x: f32) -> f32 {
        if !self.enabled {
            return 0.0;
        }
        let c = if !(x > 0.0) { 0.0 } else if x > 1.0 { 1.0 } else { x };
        let p = 127.0 * c;
        let j = p as usize;
        let t = p - j as f32;
        ((self.t[j + 1] - self.t[j]) * t) + self.t[j]
    }
}

/// Abscissa of curve sample `k`: `k * float32(1/127)` (constant 0x1447148b0).
pub fn curve_position(k: u32) -> f32 {
    k as f32 * 0.007874015718698502_f32
}

/// `C(z)`: native lower-bound compare then `min(.,1)`; NaN becomes 0.
fn unit_clamp(z: f32) -> f32 {
    (if z > 0.0 { z } else { 0.0 }).min(1.0)
}
const LN2_F: f32 = 0.6931471824645996;

/// Envelope A unit -> native domain (fconv_u2n_env_a, 0x14071a980). Clamps first.
pub fn u2n_env_a(u: f32) -> f32 {
    (unit_clamp(u) * 8.922792434692383_f32 + LN2_F).exp() - 2.0
}
/// Envelope A native -> unit (fconv_n2u_env_a, 0x14071a8c0). Clamps last.
pub fn n2u_env_a(n: f32) -> f32 {
    unit_clamp(((n + 2.0).ln() - LN2_F) * 0.11207254230976105_f32)
}
/// Envelope D unit -> native (fconv_u2n_env_d, 0x14071a9e0).
pub fn u2n_env_d(u: f32) -> f32 {
    (unit_clamp(u) * 9.433565139770508_f32 + LN2_F).exp() - 2.0
}
/// Envelope D native -> unit (fconv_n2u_env_d, 0x14071a920).
pub fn n2u_env_d(n: f32) -> f32 {
    unit_clamp(((n + 2.0).ln() - LN2_F) * 0.10600446164608002_f32)
}

// Family membership: compiler FUN_140873040. D, H, F are report notation.
/// Signed/additive source-domain targets.
pub fn is_d(id: u16) -> bool {
    matches!(id, 3 | 6 | 7 | 8 | 0xb..=0x13 | 0x15 | 0x1a | 0x1b | 0x2a | 0x2d | 0x30 | 0x5e | 0x5f
        | 0x62 | 0x6c..=0x6e | 0xb0 | 0xc2 | 0xd8..=0xdf)
}
/// Complement family (0x140875b21..0x140875b99); 0x19 only with route bit 0x04 clear.
pub fn is_h(id: u16, route_flags: u8) -> bool {
    matches!(id, 2 | 0x44 | 0x71..=0x89 | 0xb8..=0xbb | 0xbd | 0xbf | 0xc0 | 0xc4..=0xcb)
        || (id == 0x19 && route_flags & 4 == 0)
}
pub fn is_f(id: u16) -> bool {
    matches!(id, 0x1f..=0x26 | 0x28 | 0x29 | 0x2b | 0x2c | 0x2e | 0x2f | 0x37..=0x3a | 0x4b..=0x56
        | 0x65 | 0x66 | 0x68 | 0x8d..=0xac)
}
/// The source is prepared in its unipolar view unless the target is D or F.
pub fn need_uni(id: u16) -> bool {
    !(is_d(id) || is_f(id))
}

/// Dimensionless source scale after any table (0x140875bcf..0x140875cbc): eq
/// frequency/bandwidth 0.5, eq gain parsed-binary32 0.666666666666667, and
/// `x3` (`x9` for ids 6 and 0x11) for the internal type-2/source-2 special case
/// (`special3`). Not a clamp and not a unit conversion.
pub fn target_scale(id: u16, special3: bool) -> f32 {
    match id {
        0x28 | 0x29 | 0x2b | 0x2c | 0x2e | 0x2f => 0.5,
        0x2a | 0x2d | 0x30 => 0.666666666666667_f32,
        6 | 0x11 if special3 => 9.0,
        _ if special3 => 3.0,
        _ => 1.0,
    }
}

/// Source-preparation inputs. Unresolved source-type identities stay explicit
/// booleans for the caller (FUN_140973970 0x140977561..0x14097786a).
#[derive(Clone, Copy)]
pub struct Prep<'a> {
    pub target: u16,
    /// Source-domain argument `q` (true: source is bipolar).
    pub q: bool,
    pub table: Option<&'a Table>,
    /// External source type 7 with a shaper.
    pub external_type7: bool,
    /// Internal special source: omits the range conversion.
    pub omit_range: bool,
    /// Internal type +0x53c=2 with source +0x600=2.
    pub special3: bool,
    /// Evaluator invert flag `v`.
    pub invert: bool,
}

/// Prepared source `E` (emission 0x140875b21..0x140876391; literals
/// 0x144f1d6f8..0x144f1d800). Shaping, then target scale, then inversion.
pub fn prepare_source(p: &Prep, s: f32) -> f32 {
    let uni = need_uni(p.target);
    let shaped = p.table.is_some();
    let mut x = match p.table {
        None => s,
        Some(t) => {
            let at = if p.external_type7 {
                (12.0 * s + 60.0) / 127.0
            } else if p.q && !p.omit_range {
                frange_uni(s)
            } else {
                s
            };
            let y = t.lookup(at);
            if p.external_type7 || (p.q && !uni) { frange_bi(y) } else { y }
        }
    };
    x *= target_scale(p.target, p.special3);
    if shaped || !p.q {
        if uni { fcinv_flip(x, p.invert) } else { fcinv_neg(x, p.invert) }
    } else if uni {
        let n = fcinv_neg(x, p.invert);
        if p.omit_range { n } else { frange_uni(n) }
    } else {
        fcinv_neg(x, p.invert)
    }
}

/// Envelope conversion pair selected by the compiler (0x14087670b..0x140876736).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EnvPair {
    A,
    D,
}
/// A for ids 0xb8,0xb9; D for 0xba,0xbb,0xbf,0xc0; only with special flag +0x53d.
pub fn env_pair(id: u16, special_flag: bool) -> Option<EnvPair> {
    match id {
        0xb8 | 0xb9 if special_flag => Some(EnvPair::A),
        0xba | 0xbb | 0xbf | 0xc0 if special_flag => Some(EnvPair::D),
        _ => None,
    }
}

/// Source/route context for [`combine`].
#[derive(Clone, Copy)]
pub struct CombineCtx {
    pub target: u16,
    /// Route flag byte (bit 0x04 selects ID25 H vs I+).
    pub route_flags: u8,
    /// External special flag +0x53d.
    pub env_special: bool,
    /// Internal type 2, +0x600<2, byte +0x548 != 0 (ids 2, 0x71..=0x89).
    pub direct_special: bool,
    /// External types 7..=13 (ids 2, 0x71..=0x89).
    pub cubic_inner: bool,
}

/// Output combination (selection 0x140876710..0x140876952): incoming target
/// value `x`, prepared source `e`, intensity `i`.
pub fn combine(c: &CombineCtx, x: f32, e: f32, i: f32) -> f32 {
    let id = c.target;
    let inner = 1.0 - (1.0 - e) * i; // 0x144f1daa0
    let direct_id = id == 2 || (0x71..=0x89).contains(&id);
    if let Some(pair) = env_pair(id, c.env_special) {
        return match pair {
            EnvPair::A => n2u_env_a(u2n_env_a(x) * fcubic(inner)),
            EnvPair::D => n2u_env_d(u2n_env_d(x) * fcubic(inner)),
        };
    }
    if id == 0x19 && c.route_flags & 4 != 0 {
        1.0 - (1.0 - x) * (1.0 - e * i) // I+ 0x144f1daf0
    } else if (0x8d..=0xac).contains(&id) || id == 0xc2 {
        x * e * i // MUL 0x144f1db40
    } else if (0xce..=0xd5).contains(&id) {
        fcubic(inner) // CUBIC 0x144f1db90
    } else if direct_id && c.direct_special {
        x * e * i // 0x144f1da68
    } else if direct_id && c.cubic_inner {
        x * fcubic(inner) // 0x144f1da0c
    } else if is_h(id, c.route_flags) {
        x * inner
    } else {
        x + e * i // ADD 0x144f1db68
    }
}

/// True when [`combine`] reduces to the plain `x + E*i` branch for this target.
pub fn is_add(id: u16, route_flags: u8) -> bool {
    !is_h(id, route_flags)
        && !(0x8d..=0xac).contains(&id)
        && id != 0xc2
        && !(0xce..=0xd5).contains(&id)
        && !(id == 0x19 && route_flags & 4 != 0)
}

/// Native canonical target registry (initializer 0x140a6d410..0x140a6e3fd,
/// inserter FUN_140981410): serialized target name -> native id. 166 entries.
const TARGET_IDS: &[(&str, u16)] = &[
    ("volume", 2), ("pan", 3), ("pitch", 6), ("loopStart", 7),
    ("loopLength", 8), ("warpFactor", 11), ("warpFactor2", 12), ("wavetablePosition", 13),
    ("wavetableInharmonic", 14), ("wavetableModAmount", 15), ("wavetableModFrequency", 16), ("formantShift", 17),
    ("overlap", 18), ("grainSize", 19), ("grainSpeed", 21), ("intensity", 25),
    ("frequency", 26), ("pulseWidth", 27), ("filterCutoff", 31), ("filterQ", 32),
    ("filterFM", 33), ("filterFeedback", 34), ("filterBandwidth", 35), ("formantTalk", 36),
    ("formantSize", 37), ("formantSharp", 38), ("eqFreq1", 40), ("eqBandWidth1", 41),
    ("eqGain1", 42), ("eqFreq2", 43), ("eqBandWidth2", 44), ("eqGain2", 45),
    ("eqFreq3", 46), ("eqBandWidth3", 47), ("eqGain3", 48), ("vfShift", 51),
    ("vfType", 52), ("vfBypass", 53), ("cutoffLP", 55), ("cutoffHP", 56),
    ("resoLP", 57), ("resoHP", 58), ("crywahPedal", 61), ("distortionIntensity", 64),
    ("dyx_morph", 67), ("dyx_dry", 68), ("skr_tone", 71), ("skr_drive", 72),
    ("sslgeq_lf_gain", 75), ("sslgeq_lf_freq", 76), ("sslgeq_lmf_gain", 77), ("sslgeq_lmf_freq", 78),
    ("sslgeq_lmf_q", 79), ("sslgeq_hmf_gain", 80), ("sslgeq_hmf_freq", 81), ("sslgeq_hmf_q", 82),
    ("sslgeq_hf_gain", 83), ("sslgeq_hf_freq", 84), ("sslgeq_hp_freq", 85), ("sslgeq_lp_freq", 86),
    ("tape_gain", 89), ("tape_warmth", 90), ("tape_hfrolloff", 91), ("stereoSpreadIntensity", 94),
    ("stereoPan", 95), ("shaper", 98), ("bitdepth", 101), ("downsample", 102),
    ("noiselevel", 103), ("noisecolor", 104), ("off_distance", 107), ("off_azimuth", 108),
    ("off_x", 109), ("off_y", 110), ("sendLevel0", 113), ("sendLevel1", 114),
    ("sendLevel2", 115), ("sendLevel3", 116), ("sendLevel4", 117), ("sendLevel5", 118),
    ("sendLevel6", 119), ("sendLevel7", 120), ("scLevelB00", 121), ("scLevelB01", 122),
    ("scLevelB02", 123), ("scLevelB03", 124), ("scLevelB04", 125), ("scLevelB05", 126),
    ("scLevelB06", 127), ("scLevelB07", 128), ("scLevelB08", 129), ("scLevelB09", 130),
    ("scLevelB10", 131), ("scLevelB11", 132), ("scLevelB12", 133), ("scLevelB13", 134),
    ("scLevelB14", 135), ("scLevelB15", 136), ("scLevelPGM", 137), ("arp32step_v1", 141),
    ("arp32step_v2", 142), ("arp32step_v3", 143), ("arp32step_v4", 144), ("arp32step_v5", 145),
    ("arp32step_v6", 146), ("arp32step_v7", 147), ("arp32step_v8", 148), ("arp32step_v9", 149),
    ("arp32step_v10", 150), ("arp32step_v11", 151), ("arp32step_v12", 152), ("arp32step_v13", 153),
    ("arp32step_v14", 154), ("arp32step_v15", 155), ("arp32step_v16", 156), ("arp32step_v17", 157),
    ("arp32step_v18", 158), ("arp32step_v19", 159), ("arp32step_v20", 160), ("arp32step_v21", 161),
    ("arp32step_v22", 162), ("arp32step_v23", 163), ("arp32step_v24", 164), ("arp32step_v25", 165),
    ("arp32step_v26", 166), ("arp32step_v27", 167), ("arp32step_v28", 168), ("arp32step_v29", 169),
    ("arp32step_v30", 170), ("arp32step_v31", 171), ("playPos", 176), ("playDirection", 177),
    ("startPhase", 180), ("ahdsr_attack", 184), ("ahdsr_hold", 185), ("ahdsr_decay", 186),
    ("ahdsr_release", 187), ("ahdsr_sustain", 189), ("dbd_decay1", 191), ("dbd_decay2", 192),
    ("dbd_break", 194), ("fm7_level1", 196), ("fm7_level2", 197), ("fm7_level3", 198),
    ("fm7_level4", 199), ("fm7_level5", 200), ("fm7_level6", 201), ("fm7_level7", 202),
    ("fm7_level8", 203), ("fm7_time1", 206), ("fm7_time2", 207), ("fm7_time3", 208),
    ("fm7_time4", 209), ("fm7_time5", 210), ("fm7_time6", 211), ("fm7_time7", 212),
    ("fm7_time8", 213), ("fm7_slope1", 216), ("fm7_slope2", 217), ("fm7_slope3", 218),
    ("fm7_slope4", 219), ("fm7_slope5", 220), ("fm7_slope6", 221), ("fm7_slope7", 222),
    ("fm7_slope8", 223), ("legacyAddIntensity", 244),
];

/// Native id of a serialized target name (`ModTarget::param`).
pub fn target_id(name: &str) -> Option<u16> {
    TARGET_IDS.iter().find(|(n, _)| *n == name).map(|&(_, id)| id)
}

/// Source-domain argument `q` of an external source type (+0x544, values from
/// the name parser FUN_1409250d0: pitchBend 1, polyAfterTouch 2, monoAfterTouch
/// 3, midiCC 4, keyPos 7, velocity 8, releaseVelocity 9, rlsTrigCounter 10,
/// constVal 11, randomValUni 12, randomValBi 13; q from mask 0x252082 tested at
/// 0x1409774fd..0x1409775e4). Only sources whose value domain is also proved
/// are returned; the rest carry the reason they stay on the existing path.
pub fn external_source_q(source_type: u8) -> Result<bool, &'static str> {
    match source_type {
        1 => Ok(true),
        2 | 3 | 4 | 8 => Ok(false),
        7 => Err("keyPos: native value scale only inferred ((12s+60)/127 implies octaves from key 60)"),
        9 | 10 | 11 => Err("source value domain/producer not extracted for this external type"),
        12 | 13 => Err("random source generator, interval, cadence and seed are open"),
        _ => Err("source-type identity not proved"),
    }
}

/// Open laws, reported rather than approximated. Pairs of (law, reason).
pub const UNSUPPORTED: &[(&str, &str)] = &[
    ("RandomBipolar", "external type 13 has a classification path only (FUN_140973970 mask 0x252082); no generator, interval, cadence, RNG or seed is linked"),
    ("source/target clocks", "audio vs control rate and scheduler linkage of the expression engine are not proved"),
    ("D/H/F consumers", "physical units, pan/pitch/filter conversions and downstream clamps of the compiled target values are not proved"),
    ("live setters", "set_engine_par to existing-route writer to evaluator reader is not traced; recompile vs scalar update and smoothing reset unknown"),
    ("curve shaping", "curve control-point to table generation (FUN_14087c7f0) lacks data flow; only ftable lookup and 128-sample emission are proved"),
    ("lag scheduler", "the lag state equations are proved but the execution period is not; lag is left on the existing router law"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f32, b: f32, rel: f32) {
        assert!((a - b).abs() <= rel * b.abs().max(1.0), "{a} vs {b}");
    }

    #[test]
    fn conditional_inversion_flips_only_when_flagged() {
        assert_eq!(fcinv_neg(0.25, false), 0.25);
        assert_eq!(fcinv_neg(0.25, true), -0.25);
        assert!(fcinv_neg(0.0, true).is_sign_negative());
        assert_eq!(fcinv_flip(0.25, false), 0.25);
        assert_eq!(fcinv_flip(0.25, true), 0.75);
        assert_eq!(fcinv_flip(2.0, true), -1.0); // unclamped
    }

    #[test]
    fn range_cubic_abs_have_no_clamp() {
        assert_eq!(frange_uni(-1.0), 0.0);
        assert_eq!(frange_uni(3.0), 2.0);
        assert_eq!(frange_bi(0.25), -0.5);
        assert_eq!(frange_bi(2.0), 3.0);
        assert_eq!(fcubic(-0.5), -0.125);
        assert_eq!(fcubic(3.0), 27.0);
        assert_eq!(fabs(-0.0).to_bits(), 0);
        assert_eq!(fabs(-2.5), 2.5);
    }

    #[test]
    fn table_scales_by_127_clamps_and_reads_the_extra_slot() {
        let mut t = Table::initial();
        // Default slots are k/128: x=1 lands on T[127] (127/128), not 1.
        assert_eq!(t.lookup(1.0), 127.0 / 128.0);
        assert_eq!(t.lookup(0.0), 0.0);
        assert_eq!(t.lookup(f32::NAN), 0.0);
        assert_eq!(t.lookup(f32::NEG_INFINITY), 0.0);
        assert_eq!(t.lookup(f32::INFINITY), t.lookup(1.0));
        assert_eq!(t.lookup(2.0), t.lookup(1.0));
        near(t.lookup(0.5 / 127.0), 0.5 / 128.0, 1e-6);
        // Endpoint still reads T[128], so an infinite entry propagates.
        t.t[128] = f32::INFINITY;
        assert!(t.lookup(1.0).is_nan());
        t.enabled = false;
        assert_eq!(t.lookup(0.5), 0.0);
        let s: [f32; 128] = std::array::from_fn(|k| k as f32);
        let c = Table::from_curve_samples(&s);
        assert_eq!(c.t[128], 127.0);
        assert_eq!(curve_position(127), 127.0_f32 * 0.007874015718698502_f32);
    }

    #[test]
    fn envelope_conversions_use_native_constants_and_clamp_order() {
        assert_eq!(u2n_env_a(-3.0), u2n_env_a(0.0)); // clamp before exp
        assert_eq!(u2n_env_a(7.0), u2n_env_a(1.0));
        near(u2n_env_a(0.0), 0.0, 1e-6);
        near(u2n_env_a(1.0), 15000.02, 1e-5);
        near(u2n_env_d(0.5), 221.61586, 1e-5);
        near(u2n_env_d(1.0), 25000.043, 1e-5);
        near(n2u_env_a(u2n_env_a(0.5)), 0.5, 1e-5);
        near(n2u_env_d(u2n_env_d(0.25)), 0.25, 1e-5);
        assert_eq!(n2u_env_a(1e9), 1.0); // clamp after log
        assert_eq!(n2u_env_d(f32::NAN), 0.0);
        assert_eq!(n2u_env_d(-1.0), 0.0);
    }

    #[test]
    fn family_membership_matches_compiler_sets() {
        assert!(is_d(0x2a) && is_d(0xdf) && !is_d(0xe0) && is_d(0x6e) && !is_d(0x6f));
        assert!(is_h(2, 0) && is_h(0x89, 0) && !is_h(0x8a, 0) && is_h(0xcb, 0));
        assert!(is_h(0x19, 0) && !is_h(0x19, 4));
        assert!(is_f(0x1f) && is_f(0xac) && !is_f(0xad) && is_f(0x2c) && !is_f(0x27));
        assert!(!need_uni(0x2a) && !need_uni(0x28) && need_uni(2));
    }

    #[test]
    fn target_scales_are_literal_not_rounded() {
        assert_eq!(target_scale(0x28, false), 0.5);
        assert_eq!(target_scale(0x2d, false), 0.666666666666667_f32);
        assert_ne!(target_scale(0x2d, false), 0.667);
        assert_eq!(target_scale(6, true), 9.0);
        assert_eq!(target_scale(0x11, true), 9.0);
        assert_eq!(target_scale(7, true), 3.0);
        assert_eq!(target_scale(7, false), 1.0);
    }

    fn prep(target: u16, q: bool, table: Option<&Table>) -> Prep<'_> {
        Prep { target, q, table, external_type7: false, omit_range: false, special3: false, invert: false }
    }

    #[test]
    fn source_preparation_follows_emitted_order() {
        assert_eq!(prepare_source(&prep(2, false, None), 0.3), 0.3);
        let inv = Prep { invert: true, ..prep(2, false, None) };
        assert_eq!(prepare_source(&inv, 0.25), 0.75);
        // Unshaped q=1, unipolar target: neg then range_uni.
        let q = Prep { invert: true, ..prep(2, true, None) };
        assert_eq!(prepare_source(&q, 0.5), 0.25); // (-0.5 + 1) * 0.5
        assert_eq!(prepare_source(&Prep { omit_range: true, ..q }, 0.5), -0.5);
        // D target never takes the unipolar view; eq gain scale applied first.
        let d = Prep { invert: true, ..prep(0x2a, true, None) };
        assert_eq!(prepare_source(&d, 0.75), -(0.75 * 0.666666666666667_f32));
        // Shaped q=1 signed target: lookup of range_uni(s), wrapped in range_bi.
        let t = Table::initial();
        let y = t.lookup(frange_uni(0.5));
        assert_eq!(prepare_source(&prep(3, true, Some(&t)), 0.5), frange_bi(y));
        // External type 7 shaped uses (12 s + 60) / 127.
        let e = Prep { external_type7: true, ..prep(2, true, Some(&t)) };
        assert_eq!(prepare_source(&e, 1.0), frange_bi(t.lookup(72.0 / 127.0)));
    }

    #[test]
    fn output_combination_formulas() {
        let c = |target| CombineCtx { target, route_flags: 0, env_special: false, direct_special: false, cubic_inner: false };
        let (x, e, i) = (0.5_f32, 0.25_f32, 0.5_f32);
        let inner = 1.0 - (1.0 - e) * i; // 0.625
        assert_eq!(combine(&c(0x44), x, e, i), x * inner);
        assert_eq!(combine(&CombineCtx { direct_special: true, ..c(2) }, x, e, i), 0.0625);
        assert_eq!(combine(&CombineCtx { cubic_inner: true, ..c(0x71) }, x, e, i), x * fcubic(inner));
        assert_eq!(combine(&CombineCtx { route_flags: 4, ..c(0x19) }, x, e, i), 1.0 - 0.5 * (1.0 - 0.125));
        assert_eq!(combine(&CombineCtx { route_flags: 0, ..c(0x19) }, x, e, i), x * inner);
        assert_eq!(combine(&c(0x8d), x, e, i), 0.0625);
        assert_eq!(combine(&c(0xc2), x, e, i), 0.0625);
        assert_eq!(combine(&c(0xce), x, e, i), fcubic(inner));
        assert_eq!(combine(&c(0x2a), x, e, i), 0.625);
        let a = CombineCtx { env_special: true, ..c(0xb8) };
        near(combine(&a, 0.5, 1.0, 1.0), 0.5, 1e-5); // inner = 1: pure round trip
        near(combine(&CombineCtx { env_special: true, ..c(0xba) }, 0.5, 1.0, 1.0), 0.5, 1e-5);
        // Without the special flag 0xb8 is ordinary H.
        assert_eq!(combine(&c(0xb8), x, e, i), x * inner);
    }

    #[test]
    fn open_laws_are_reported_with_reasons() {
        assert!(UNSUPPORTED.iter().any(|(n, r)| *n == "RandomBipolar" && r.contains("seed")));
        assert!(UNSUPPORTED.iter().all(|(_, r)| !r.is_empty()));
    }

    #[test]
    fn target_names_and_source_types_resolve() {
        assert_eq!(target_id("volume"), Some(2));
        assert_eq!(target_id("eqGain1"), Some(0x2a));
        assert_eq!(target_id("intensity"), Some(25));
        assert_eq!(target_id("nope"), None);
        assert_eq!(TARGET_IDS.len(), 166);
        assert_eq!(external_source_q(1), Ok(true));
        assert_eq!(external_source_q(8), Ok(false));
        assert!(external_source_q(13).is_err() && external_source_q(7).is_err());
        assert!(is_add(6, 0) && !is_add(2, 0) && !is_add(0x8d, 0) && !is_add(0x19, 4) && is_add(26, 0));
    }
}
