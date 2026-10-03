//! Saved version-0x50 Reverb. The later RV2 processor is a different effect.
//! All delay storage is prepared with the rack, never in `process`.

use super::params::LegacyReverb as Params;

// Public-domain Freeverb tuning (Jezar, 2000). The native network retains
// these delays, but uses true allpasses and its own input damping filter.
const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const RING: usize = 8192;
const QUIET: f32 = 13.0 / 65536.0;

pub(super) struct Reverb {
    channels: [Channel; 2],
    width: f32,
    feedback: f32,
    color: f32,
    predelay: usize,
    position: usize,
    active_channels: i32,
    damping: Damping,
}

struct Channel {
    predelay: Box<[f32]>,
    combs: [Delay; 8],
    allpasses: [Delay; 4],
    color: [f32; 8],
    filter: [f32; 4],
    /// Last active position on the 8192-frame clock. None means quiet.
    active: Option<usize>,
}

struct Delay {
    data: Box<[f32]>,
    offset: usize,
}

impl Delay {
    fn new(tuning: usize, channel: usize, rate: f32) -> Self {
        // Native lines share the fixed 8192-frame clock. Integer-boundary
        // rounding is observable: division by 44.1 changes some lengths.
        let shift = (23000.0 / 44100.0) as f32 * channel as f32;
        let frames = (tuning as f64 * 1000.0 / 44100.0 + shift as f64) * (rate as f64 * 0.001);
        // cvttsd2si returns INT_MIN outside its signed conversion range;
        // INT_MIN masks to zero on this clock, as do full-ring lengths.
        let offset = if frames >= 2147483648.0 {
            0
        } else {
            frames as usize & (RING - 1)
        };
        Self {
            data: vec![0.0; RING].into_boxed_slice(),
            offset,
        }
    }

    fn read(&self, position: usize) -> f32 {
        self.data[position]
    }

    fn write(&mut self, position: usize, value: f32) {
        self.data[(position + self.offset) & (RING - 1)] = value;
    }

    fn clear(&mut self) {
        self.data.fill(0.0);
    }
}

impl Reverb {
    pub(super) fn new(p: &Params, rate: f32) -> Option<Self> {
        if !p.supported() || !rate.is_finite() || rate <= 0.0 {
            return None;
        }
        Some(Self {
            channels: std::array::from_fn(|channel| Channel {
                predelay: vec![0.0; RING].into_boxed_slice(),
                combs: std::array::from_fn(|n| Delay::new(COMBS[n], channel, rate)),
                allpasses: std::array::from_fn(|n| Delay::new(ALLPASSES[n], channel, rate)),
                color: [0.0; 8],
                filter: [0.0; 4],
                active: None,
            }),
            // Retain the saved-to-runtime conversion before the channel
            // mix. Simplifying this to the saved width changes f32 rounding.
            width: (p.stereo + 1.0) * 0.5,
            feedback: 0.65 + 0.3 * p.size,
            color: 1.0 - (1.0 - p.color) * 0.35,
            predelay: ((p.predelay_ms * rate) * 0.001).min(RING as f32) as usize & (RING - 1),
            position: 0,
            active_channels: 0,
            damping: Damping::new(p.damping, rate),
        })
    }

    pub(super) fn awake(&self) -> bool {
        self.active_channels > 0
    }

    pub(super) fn clear(&mut self) {
        self.position = 0;
        self.active_channels = 0;
        self.damping.reset();
        for ch in &mut self.channels {
            ch.predelay.fill(0.0);
            ch.combs.iter_mut().for_each(Delay::clear);
            ch.allpasses.iter_mut().for_each(Delay::clear);
            ch.color.fill(0.0);
            ch.filter.fill(0.0);
            ch.active = None;
        }
    }

    pub(super) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let fed = left
            .iter()
            .chain(right.iter())
            .any(|v| v.abs() > super::processor::SILENCE);
        if fed {
            self.active_channels = 2;
            let marker = self.position.wrapping_sub(1) & (RING - 1);
            for ch in &mut self.channels {
                ch.active = Some(marker);
            }
        } else if !self.awake() {
            left.fill(0.0);
            right.fill(0.0);
            return;
        }
        // The native quiet test accumulates magnitude within each dispatch.
        // It is intentionally not a per-sample threshold or peak detector.
        let mut magnitude = [0.0f32; 2];
        let mix = (1.0 - self.width) / 0.5;
        let own = 1.0 - mix;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.damping.tick();
            let input = if mix > 1e-12 {
                let mean = (0.5 * *r) + ((0.5 * *l) + 0.0);
                [mix * mean + own * *l, mix * mean + own * *r]
            } else {
                [*l, *r]
            };
            for (n, out) in [&mut *l, &mut *r].into_iter().enumerate() {
                let ch = &mut self.channels[n];
                let filtered = self.damping.sample(input[n] * 0.015, &mut ch.filter);
                ch.predelay[(self.position + self.predelay) & (RING - 1)] = filtered;
                let input = ch.predelay[self.position];
                let mut comb = [0.0; 8];
                for (i, delay) in ch.combs.iter_mut().enumerate() {
                    let delayed = delay.read(self.position);
                    ch.color[i] = (1.0 - self.color) * ch.color[i] + self.color * delayed;
                    delay.write(self.position, self.feedback * ch.color[i] + input);
                    comb[i] = delay.read(self.position);
                }
                let mut signal = ((comb[3] + comb[2]) + (comb[1] + comb[0]))
                    + ((comb[7] + comb[6]) + (comb[5] + comb[4]));
                let mut ap = [0.0; 4];
                for (i, delay) in ch.allpasses.iter_mut().enumerate() {
                    ap[i] = delay.read(self.position);
                    let y = -0.5 * signal + ap[i];
                    delay.write(self.position, 0.5 * y + signal);
                    signal = y;
                }
                *out = signal;
                let c = comb.map(f32::abs);
                let a = ap.map(f32::abs);
                let sum = ((((input.abs() + magnitude[n]) + c[0]) + c[1]) + c[2]) + c[3];
                magnitude[n] =
                    (sum + ((c[5] + c[6]) + c[4])) + (((a[3] + a[2]) + a[1]) + (c[7] + a[0]));
                if ch.active == Some(self.position) {
                    ch.active = None;
                    self.active_channels -= 1;
                }
                if magnitude[n] > QUIET {
                    ch.active = Some(self.position);
                }
            }
            self.position = (self.position + 1) & (RING - 1);
        }
    }
}

impl Params {
    pub(super) fn supported(&self) -> bool {
        self.predelay_ms.is_finite()
            && self.predelay_ms >= 0.0
            && [self.size, self.stereo, self.color, self.damping]
                .into_iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(&v))
    }
}

/// The input filter retains its own 32-frame parameter clock and ordinary
/// four-value direct-form history. Saved controls initialize without a ramp.
struct Damping {
    current: f32,
    target: f32,
    rate: f32,
    cached: f32,
    phase: u8,
    coefficients: [f32; 5], // b1, b2, a1, a2, b0
}

impl Damping {
    fn new(damping: f32, rate: f32) -> Self {
        let cutoff = 0.75 * (1.0 - damping) + 0.25;
        Self {
            current: cutoff,
            target: cutoff,
            rate,
            cached: cutoff,
            phase: 0,
            coefficients: coefficients(cutoff, rate),
        }
    }

    fn reset(&mut self) {
        self.current = self.target;
        self.phase = 0;
        self.cached = self.current;
        self.coefficients = coefficients(self.current, self.rate);
    }

    fn tick(&mut self) {
        if self.phase == 0 {
            let delta = self.target - self.current;
            if delta.abs() > 1e-4 {
                let next = self.current + delta * 0.015;
                if (self.cached as f64 - next as f64).abs() >= 1e-6 {
                    self.coefficients = coefficients(next, self.rate);
                    self.cached = next;
                }
                self.current = next;
            }
        }
        self.phase = (self.phase + 1) & 31;
    }

    fn sample(&self, x: f32, history: &mut [f32; 4]) -> f32 {
        let [b1, b2, a1, a2, b0] = self.coefficients;
        let [x1, x2, y1, y2] = *history;
        let a = (y1 * a1 + y2 * a2) + x2 * b2;
        let b = x1 * b1 + x * b0;
        let y = a + b;
        *history = [x, x1, y, y1];
        y
    }
}

/// Eighth-order exponential and seventh-order sine approximants, preserving
/// the effect's float intermediates and double coefficient normalization.
/// An ideal tangent/RBJ filter is not interchangeable with this response.
fn coefficients(cutoff: f32, rate: f32) -> [f32; 5] {
    let x = (cutoff.clamp(0.0, 1.0) as f64 * 6.15) as f32;
    let exp = if x.abs() < 1e-4 {
        1.0
    } else {
        let x2 = x * x;
        let x3 = x2 * x;
        let x4 = x3 * x;
        let x5 = x4 * x;
        let x6 = x5 * x;
        let x7 = x6 * x;
        let x8 = x7 * x;
        let a = x2 * 0.5 + x;
        let b = x3 * (1.0 / 6.0) + 1.0;
        let c = x4 * (1.0 / 24.0) + x5 * (1.0 / 120.0);
        let d = x8 * (1.0 / 40320.0) + x7 * (1.0 / 5040.0);
        ((d + x6 * (1.0 / 720.0)) + c) + (a + b)
    };
    let normalized = (((exp - 1.0) * 0.0025728316977620125f32).clamp(0.0, 1.0) + 0.002)
        * ((44100.0 / 1.002) / rate as f64) as f32;
    let theta = (normalized.clamp(0.0, 1.0) as f64 * std::f64::consts::FRAC_PI_2) as f32;
    let theta = (theta as f64 * 0.99) as f32;
    let theta = (theta as f64 * 0.9987809049669) as f32;
    let polynomial = |x: f32| {
        let x3 = (x * x) * x;
        let x5 = (x3 * x) * x;
        let x6 = x5 * x;
        (
            x5 * (1.0 / 120.0) - x6 * (x * (1.0 / 5040.0)),
            x3 * (1.0 / 6.0),
        )
    };
    let upper = 0.5 - theta;
    let numerator = if upper.abs() < 1e-4 {
        0.0
    } else {
        let (a, b) = polynomial(upper);
        a + (upper - b)
    };
    let (a, b) = polynomial(0.5 + theta);
    let w = numerator / ((a + 0.5) + (theta - b));

    // Two-pole Butterworth prototype at angle .5 radians. This follows the
    // polynomial's analog prototype and mixed-precision warp; it does not
    // replace the native prewarp with tan(theta).
    let k = 0.5f64.tan();
    let pole = std::f64::consts::FRAC_PI_4;
    let (imag, real) = (-pole.sin(), pole.cos());
    let q = real * real + imag * imag;
    let k4 = 4.0 * (k * k);
    let gain = 1.0 / ((4.0 - imag * (8.0 * k)) + q * k4);
    let b0 = (gain * k4) as f32;
    let b1 = b0 + b0;
    let b2 = b0;
    let a1 = ((8.0 - q * (2.0 * k4)) * gain) as f32;
    let a2 = (((-4.0 - imag * (8.0 * k)) - q * k4) * gain) as f32;
    let (b0, b1, b2, a1, a2, w) = (
        b0 as f64, b1 as f64, b2 as f64, a1 as f64, a2 as f64, w as f64,
    );
    let w2 = w * w;
    let inverse = 1.0 / ((a1 * w + 1.0) - w2 * a2);
    let rounded = |n: f64| ((n as f32) as f64 * inverse) as f32;
    let mut c = [
        rounded(((b1 - (b0 + b0) * w) + w2 * b1) - (b2 + b2) * w),
        rounded((w2 * b0 - b1 * w) + b2),
        rounded(((w2 + 1.0) * a1 + (w + w)) - (a2 + a2) * w),
        rounded(a2 - (a1 * w + w2)),
        rounded((b0 - b1 * w) + w2 * b2),
    ];
    let dc = (1.0 - (c[3] + c[2]) as f64) / ((c[0] + c[4]) + c[1]) as f64;
    for i in [0, 1, 4] {
        c[i] = (c[i] as f64 * dc) as f32;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> Params {
        Params {
            predelay_ms: 0.0,
            size: 0.5,
            stereo: 1.0,
            color: 0.5,
            damping: 0.5,
        }
    }

    #[test]
    fn legacy_reverb_f32_filter_and_public_delay_reference() {
        // Independent saved-to-runtime scalar calculations preserve the
        // rounding boundary before width mixing and feedback coloration.
        for (stereo, color, width_bits, color_bits) in [
            (0.0, 0.0, 1056964608, 1059481190),
            (0.123456789, 0.123456789, 1058000239, 1060206132),
            (0.375, 0.875, 1060110336, 1064619213),
            (0.99999994, 0.99999994, 1065353216, 1065353216),
            (1.0, 1.0, 1065353216, 1065353216),
        ] {
            let rv = Reverb::new(
                &Params {
                    stereo,
                    color,
                    ..params()
                },
                48000.0,
            )
            .unwrap();
            assert_eq!(rv.width.to_bits(), width_bits);
            assert_eq!(rv.color.to_bits(), color_bits);
        }
        // Independently evaluated scalar exp/sine series, prototype and
        // mixed-precision warp. These are authored calculation results.
        for (rate, damping, bits) in [
            (
                32000.0,
                0.0,
                [1073370404, 1064981796, 3220849940, 3212102248, 1064981796],
            ),
            (
                32000.0,
                0.125,
                [1068126498, 1059737890, 3214642519, 3202634678, 1059737890],
            ),
            (
                32000.0,
                0.5,
                [1035771464, 1027382856, 1067936688, 3204185571, 1027382856],
            ),
            (
                32000.0,
                0.875,
                [997306752, 988918144, 1072692505, 3210861973, 988918144],
            ),
            (
                32000.0,
                1.0,
                [982923008, 974534400, 1073164330, 3211720331, 974534400],
            ),
            (
                44100.0,
                0.0,
                [1073370404, 1064981796, 3220849940, 3212102248, 1064981796],
            ),
            (
                44100.0,
                0.125,
                [1061467095, 1053078487, 3199218799, 3192206810, 1053078487],
            ),
            (
                44100.0,
                0.5,
                [1029097112, 1020708504, 1069485446, 3206083551, 1020708504],
            ),
            (
                44100.0,
                0.875,
                [989946240, 981557632, 1072979996, 3211379451, 981557632],
            ),
            (
                44100.0,
                1.0,
                [975153664, 966765056, 1073322707, 3212019067, 966765056],
            ),
            (
                48000.0,
                0.0,
                [1070669985, 1062281377, 3217870015, 3207260421, 1062281377],
            ),
            (
                48000.0,
                0.125,
                [1059834377, 1051445769, 3190400622, 3191122393, 1051445769],
            ),
            (
                48000.0,
                0.5,
                [1027113264, 1018724656, 1069823298, 3206511274, 1018724656],
            ),
            (
                48000.0,
                0.875,
                [987444480, 979055872, 1073041827, 3211492987, 979055872],
            ),
            (
                48000.0,
                1.0,
                [973539328, 965150720, 1073356750, 3212084000, 965150720],
            ),
            (
                96000.0,
                0.0,
                [1057031700, 1048643092, 1043113651, 3191134036, 1048643092],
            ),
            (
                96000.0,
                0.125,
                [1046872012, 1038483404, 1062912637, 3197863111, 1038483404],
            ),
            (
                96000.0,
                0.5,
                [1011668544, 1003279936, 1071763963, 3209300648, 1003279936],
            ),
            (
                96000.0,
                0.875,
                [970963968, 962575360, 1073391684, 3212150903, 962575360],
            ),
            (
                96000.0,
                1.0,
                [956862464, 948473856, 1073549263, 3212456112, 948473856],
            ),
        ] {
            let p = Params {
                damping,
                ..params()
            };
            let rv = Reverb::new(&p, rate).unwrap();
            assert_eq!(rv.damping.coefficients.map(f32::to_bits), bits);
            let c = rv.damping.coefficients;
            let dc = ((c[0] + c[4]) + c[1]) as f64 / (1.0 - (c[2] + c[3]) as f64);
            assert!((dc - 1.0).abs() < 1e-6);
        }
        let rv = Reverb::new(&params(), 48000.0).unwrap();
        assert_eq!(
            rv.channels[0].combs.each_ref().map(|d| d.offset),
            [1214, 1293, 1389, 1475, 1547, 1622, 1694, 1760]
        );
        assert_eq!(
            rv.channels[1].combs.each_ref().map(|d| d.offset),
            [1239, 1318, 1414, 1500, 1572, 1647, 1719, 1785]
        );
        let mut filter = Damping::new(0.5, 48000.0);
        filter.target = 0.25;
        let mut current = filter.current;
        for i in 0..257 {
            if i % 32 == 0 && (filter.target - current).abs() > 1e-4 {
                current += (filter.target - current) * 0.015;
            }
            filter.tick();
            assert_eq!(filter.current.to_bits(), current.to_bits());
        }
    }

    #[test]
    fn legacy_reverb_impulse_predelay_fragmentation_quiet_and_reset() {
        let frames = 32000;
        let render = |p: Params, parts: &[usize]| {
            let mut rv = Reverb::new(&p, 44100.0).unwrap();
            let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
            l[0] = 1.0;
            let mut at = 0;
            let mut part = 0;
            while at < frames {
                let end = (at + parts[part % parts.len()]).min(frames);
                rv.process(&mut l[at..end], &mut r[at..end]);
                at = end;
                part += 1;
            }
            (rv, l, r)
        };
        let (mut rv, l, r) = render(params(), &[128]);
        let (_, fragmented, fr) = render(params(), &[17, 111]);
        assert_eq!(l, fragmented);
        assert_eq!(r, fr);
        assert!(r.iter().all(|v| *v == 0.0));
        assert!(l[..1116].iter().all(|v| *v == 0.0));
        let first = (0.015 * rv.damping.coefficients[4]) * 0.0625;
        assert!(
            (l[1116] - first).abs() < 1e-9,
            "true-allpass direct feedthrough"
        );
        let (_, delayed, _) = render(
            Params {
                predelay_ms: 10.0,
                ..params()
            },
            &[128],
        );
        assert_eq!(&delayed[441..], &l[..frames - 441]);
        let capped = Reverb::new(
            &Params {
                predelay_ms: 1000.0,
                ..params()
            },
            44100.0,
        )
        .unwrap();
        assert_eq!(
            capped.predelay, 0,
            "native8192 ring mask aliases the upper cap"
        );
        assert!(rv.awake());
        let mut zero = [0.0f32; 128];
        let mut right = [0.0f32; 128];
        for _ in 0..4000 {
            zero.fill(0.0);
            right.fill(0.0);
            rv.process(&mut zero, &mut right);
            assert!(zero.iter().chain(right.iter()).all(|v| v.is_finite()));
            if !rv.awake() {
                break;
            }
        }
        assert!(
            !rv.awake(),
            "native cumulative quiet tracker eventually drains"
        );
        rv.clear();
        let mut impulse = [0.0; 128];
        impulse[0] = 1.0;
        rv.process(&mut impulse, &mut right);
        assert!(rv.awake());
        assert!(impulse.iter().all(|v| *v == 0.0));
        let mut invalid = params();
        invalid.damping = f32::NAN;
        assert!(Reverb::new(&invalid, 44100.0).is_none());
    }
}
