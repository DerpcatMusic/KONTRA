//! UVI Workstation 4.0.9 ToneStack as the color chain's tone shaper (process 0x1412bb540,
//! section 0x1412bbc90, design 0x14158fee0): the Yeh/Smith passive tone stack (treble,
//! bass, mid pots over R1..R4, C1..C3 from the 26-model table 0x141ef0820), bilinear at
//! `2 fs` in f64, run as one third-order f32 section, then `10^Gain` (model
//! uvicolor/tonestack.py, checked against the emulated kernel).
use super::super::Planar;
pub use sampler_ir::fx::color::ToneStack;

/// `[R1, R2, R3, R4, C1, C2, C3]` per model, f32 bits.
const MODELS: [[u32; 7]; 26] = [
    [0x48742400, 0x49742400, 0x46c35000, 0x475ac000, 0x2f89705f, 0x32abcc77, 0x32abcc77], // Bass
    [0x48742400, 0x48742400, 0x46c35000, 0x47c35000, 0x2f89705f, 0x33d6bf94, 0x3349dd0c], // Mesa
    [0x48742400, 0x48742400, 0x461c4000, 0x47c35000, 0x2f03f0ff, 0x33d6bf94, 0x3349dd0c], // Twin
    [0x48742400, 0x48742400, 0x45960000, 0x47c35000, 0x2f89705f, 0x33d6bf94, 0x3349dd0c], // Prince
    [0x4856d800, 0x49742400, 0x46abe000, 0x4700e800, 0x3001314f, 0x32bcfa83, 0x32bcfa83], // JC 800
    [0x48742400, 0x49742400, 0x46c35000, 0x475ac000, 0x3009705f, 0x32bcfa83, 0x32bcfa83], // JC 2000
    [0x48742400, 0x49742400, 0x46c35000, 0x4700e800, 0x2f946f1f, 0x32bcfa83, 0x32bcfa83], // JTM45
    [0x48742400, 0x49742400, 0x46c35000, 0x4700e800, 0x3009705f, 0x32bcfa83, 0x32bcfa83], // Major Lead
    [0x48742400, 0x48742400, 0x46c35000, 0x475ac000, 0x2f89705f, 0x3349dd0c, 0x3349dd0c], // M2199
    [0x49742400, 0x49742400, 0x461c4000, 0x47c35000, 0x2e5be6ff, 0x32bcfa83, 0x32bcfa83], // AC-30
    [0x4856d800, 0x4856d800, 0x4856d800, 0x47c35000, 0x3001314f, 0x33d6bf94, 0x3349dd0c], // AC-15
    [0x48742400, 0x49742400, 0x46c35000, 0x47379800, 0x3001314f, 0x32abcc77, 0x32abcc77], // SLO 100
    [0x48f42400, 0x49742400, 0x461c4000, 0x47379800, 0x3001314f, 0x32bcfa83, 0x32bcfa83], // MIG 100H
    [0x48742400, 0x48742400, 0x469c4000, 0x4784d000, 0x2f946f1f, 0x32bcfa83, 0x32bcfa83], // C20
    [0x48742400, 0x48742400, 0x461c4000, 0x47c35000, 0x2f946f1f, 0x33d6bf94, 0x332bcc77], // GX20
    [0x48742400, 0x48742400, 0x461c4000, 0x47202800, 0x2f83f0ff, 0x330dbbe2, 0x33b017fa], // Cube
    [0x48742400, 0x48742400, 0x461c4000, 0x47c35000, 0x2f24ed3f, 0x33b017fa, 0x3349dd0c], // Jazz chorus
    [0x48742400, 0x49742400, 0x46c35000, 0x46fa0000, 0x3001314f, 0x32bcfa83, 0x32bcfa83], // VL-501
    [0x48742400, 0x49742400, 0x4700e800, 0x47473800, 0x2f71e47f, 0x3280d959, 0x3349dd0c], // Triple Giant
    [0x4856d800, 0x49742400, 0x46abe000, 0x4784d000, 0x3001314f, 0x32bcfa83, 0x32bcfa83], // Trio
    [0x4856d800, 0x4856d800, 0x461c4000, 0x47c35000, 0x2f71e47f, 0x3349dd0c, 0x3349dd0c], // H&K
    [0x48f42400, 0x49742400, 0x46c35000, 0x47379800, 0x2f24ed3f, 0x32bcfa83, 0x32bcfa83], // H&K Tube 0
    [0x48742400, 0x48742400, 0x461c4000, 0x47c35000, 0x2f89705f, 0x33d6bf94, 0x3349dd0c], // F
    [0x48742400, 0x48742400, 0x46c35000, 0x47c35000, 0x2f89705f, 0x32bcfa83, 0x32bcfa83], // F blues
    [0x48742400, 0x48742400, 0x46c35000, 0x47fde800, 0x2f89705f, 0x33d6bf94, 0x32bcfa83], // F Hot Rod
    [0x49742400, 0x49742400, 0x47b79800, 0x4883d600, 0x2ddbe6ff, 0x3380d959, 0x32abcc77], // gs12
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Engine {
    /// `[b0, b1, b2, b3, a1, a2, a3]`.
    c: [f32; 7],
    gain: f32,
}

/// Per channel `[x1, x2, x3, y1, y2, y3]`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct State {
    z: [[f32; 6]; 2],
}

impl State {
    pub(super) fn finite(&self) -> bool {
        self.z.iter().flatten().all(|v| v.is_finite())
    }
}

/// 0x14158fee0 with `fs`, bass `l`, mid `m`, treble `t`.
fn design(comp: [f64; 7], fs: f64, l: f64, m: f64, t: f64) -> [f32; 7] {
    let [r1, r2, r3, r4, c1, c2, c3] = comp;
    let big_l = 10f64.powf((l - 1.0) * 3.5);
    let s23 = c2 + c1;
    let p12 = c2 * c1;
    let e = c3 * m * r3;
    let p31 = c3 * c1;
    let f = s23 * r2 * big_l;
    let g = (c3 + c2) * c1 * r1;
    let h = r3 * m * m * s23 * c3 * r3;
    let k = c3 * c2 * r3;
    let q = p12 * r1 * r3;
    let w = p12 * c3;
    let r = (p12 + p31) * r2 * r4 + p12 * r1 * r2;
    let u = r3 * big_l * m * s23 * c3 * r2;
    let v = w * r1 * r3;
    let z = p31 * r3;
    let y = r4 * r3 * w * r3 + v * r3;
    let ww = (r4 * r3 * w * r2 + w * r1 * r2 * r3) * big_l * m - y * m * m;
    let a3n = ((c3 * c2 * c1 * t * r4 * r3 * r1 + y * m + ww) - c1 * c2 * t * m * r4 * r3 * r1 * c3)
        + c1 * c2 * t * big_l * r2 * r4 * r1 * c3;
    let v = v * r4;
    let b3n = r2 * r4 * r1 * c3 * c1 * c2 * big_l + v + (y - v) * m + ww;
    let c = fs + fs;
    let cc = c * c;
    let ccc = cc * c;
    let a1 = ((r4 + r3) * c2 + (r3 + r1) * c1 + e + f + r4 * c3) * c;
    let a3 = ccc * b3n;
    let b1 = (s23 * r3 + f + c1 * t * r1 + e) * c;
    let a2 = (((((p31 * r1 * r3 - k * r4) + (z + k) * r3) * m + u) - h)
        + (c3 * c2 * r2 * r4 + r) * big_l
        + (g + p12 * r3 + z) * r4
        + q
        + k * r4)
        * cc;
    let a33 = b3n * 3.0 * ccc;
    let b2 = ((g * r4 * t - h) + r * big_l + (p31 * (r3 + r1) + k) * r3 * m + (z + p12 * r3) * r4 + q + u) * cc;
    let b3 = ccc * a3n;
    let b33 = a3n * 3.0 * ccc;
    let n = 1.0 / ((((-1.0 - a1) - a2) - a3) as f32);
    [
        ((((-b1) - b2) - b3) as f32) * n,
        (((b2 - b1) + b33) as f32) * n,
        (((b2 + b1) - b33) as f32) * n,
        (((b1 - b2) + b3) as f32) * n,
        (((a2 - (a1 + 3.0)) + a33) as f32) * n,
        ((((a2 + a1) - 3.0) - a33) as f32) * n,
        ((((a1 - 1.0) - a2) + a3) as f32) * n,
    ]
}

impl Engine {
    pub(super) fn new(s: ToneStack, rate: u32) -> Self {
        let comp = MODELS[usize::from(s.model.min(25))].map(|b| f64::from(f32::from_bits(b)));
        let c = design(comp, f64::from(rate as f32), f64::from(s.bass), f64::from(s.mid), f64::from(s.treble));
        Self { c, gain: 10f64.powf(f64::from(s.gain)) as f32 }
    }

    pub(super) fn process(&self, state: &mut State, block: &mut Planar, len: usize) {
        let [b0, b1, b2, b3, a1, a2, a3] = self.c;
        for (z, channel) in state.z.iter_mut().zip(block.iter_mut()) {
            let [mut x1, mut x2, mut x3, mut y1, mut y2, mut y3] = *z;
            for v in &mut channel[..len] {
                let x0 = *v as f32;
                let acc = ((x1 * b1) + (x0 * b0)) + ((x2 * b2) + (b3 * x3));
                let y = ((acc - (y1 * a1)) - (y2 * a2)) - (a3 * y3);
                (x3, x2, x1) = (x2, x1, x0);
                (y3, y2, y1) = (y2, y1, y);
                *v = f64::from(if self.gain == 1.0 { y } else { y * self.gain });
            }
            *z = [x1, x2, x3, y1, y2, y3];
        }
    }
}
