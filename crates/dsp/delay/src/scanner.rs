//! A scanner vibrato: the band below a split runs down a lumped LC delay
//! line (equal series inductors, shunt capacitors to ground, a resistive
//! load at its end), and a pickup rotating across its taps reads it. As the
//! pickup moves from tap to tap the delay it reads changes: the vibrato.
//! The band above the split passes around the line.
//!
//! The line is the circuit itself, integrated by the trapezoidal rule (the
//! same discretization a wave-digital ladder with bilinear ports makes):
//! one tridiagonal solve per frame, its factors fixed until the settings
//! change. A mismatched load reflects along the line, its resonances the
//! circuit's own.

use std::f32::consts::PI;

/// The most sections a line holds.
pub const SCANNER_SECTIONS: usize = 36;
/// The most taps the pickup crosses, in order, one way.
pub const SCANNER_TAPS: usize = 9;
/// The pickup's stations per turn: out across the taps and back.
const STATIONS: usize = 2 * (SCANNER_TAPS - 1);

/// A scanner vibrato in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScannerSettings {
    /// The line's sections, at most [`SCANNER_SECTIONS`].
    pub sections: usize,
    /// Each section's series inductance, H.
    pub henry: f32,
    /// Each section's shunt capacitance, F; the last section's is `last_farad`.
    pub farad: f32,
    pub last_farad: f32,
    /// The resistance loading the line's end, Ω.
    pub load_ohm: f32,
    /// Resistances to ground at the first nodes after the input, Ω.
    pub shunt_ohm: [f32; 6],
    /// The nodes the pickup crosses (0 is the line's input).
    pub taps: [u8; SCANNER_TAPS],
    /// The pickup's turns per second.
    pub hz: f32,
    /// The right pickup's lead over the left, in turns.
    pub lead: f32,
    /// Where the line's band ends: a second-order Butterworth split, Hz.
    pub split_hz: f32,
    /// The gain of the band above the split, passed around the line.
    pub high: f32,
    /// The line's input mixed into the pickup's signal (0..1: at 1 half each).
    pub blend: f32,
    /// The effect against the input (0: the input alone).
    pub mix: f32,
}

/// One trapezoidal state-variable filter (lowpass and highpass outputs).
#[derive(Clone, Copy, Debug, Default)]
struct Split {
    g: f32,
    d: f32,
    s1: f32,
    s2: f32,
}

impl Split {
    const K: f32 = std::f32::consts::SQRT_2;

    fn new(hz: f32, rate: f32) -> Self {
        let g = (PI * hz.clamp(10.0, 0.49 * rate) / rate).tan();
        Self {
            g,
            d: 1.0 / (1.0 + g * (g + Self::K)),
            s1: 0.0,
            s2: 0.0,
        }
    }

    fn tick(&mut self, x: f32) -> (f32, f32) {
        let v1 = (self.g * (x - self.s2) + self.s1) * self.d;
        let v2 = self.s2 + self.g * v1;
        self.s1 = 2.0 * v1 - self.s1;
        self.s2 = 2.0 * v2 - self.s2;
        (v2, x - Self::K * v1 - v2)
    }
}

/// One node after the line's input: the capacitor (and shunts) to ground
/// there, the inductor into it, and its share of the factored solve.
#[derive(Clone, Copy, Debug, Default)]
struct Node {
    /// The capacitor's conductance `2C/T`.
    gc: f32,
    /// The elimination's pivot inverse and ratio (swept from the line's end).
    pivot: f32,
    ratio: f32,
    /// The inductor's and the capacitor's history currents.
    hl: f32,
    hc: f32,
    /// The swept right-hand side, then the node's voltage.
    d: f32,
    v: f32,
}

/// The line's circuit: its nodes after the input, factored once.
#[derive(Clone, Debug)]
struct Line {
    n: usize,
    /// The inductors' conductance `T/2L`.
    gl: f32,
    nodes: [Node; SCANNER_SECTIONS],
    /// The input node's voltage.
    input: f32,
}

impl Line {
    fn new(s: &ScannerSettings, rate: f32) -> Self {
        let n = s.sections.clamp(1, SCANNER_SECTIONS);
        let gl = 1.0 / (2.0 * s.henry.max(1e-9) * rate);
        let mut nodes = [Node::default(); SCANNER_SECTIONS];
        // Eliminated from the far end: each pivot sees the ratio beyond it.
        let mut beyond = 0.0;
        for (i, node) in nodes.iter_mut().enumerate().take(n).rev() {
            let last = i + 1 == n;
            let farad = if last { s.last_farad } else { s.farad };
            node.gc = 2.0 * farad.max(0.0) * rate;
            let shunt = s.shunt_ohm.get(i).map_or(0.0, |r| 1.0 / r.max(1.0));
            let load = if last { 1.0 / s.load_ohm.max(1.0) } else { 0.0 };
            let ahead = if last { 0.0 } else { gl };
            let diag = gl + ahead + node.gc + shunt + load;
            node.pivot = 1.0 / (diag - gl * beyond);
            node.ratio = gl * node.pivot;
            beyond = node.ratio;
        }
        Self {
            n,
            gl,
            nodes,
            input: 0.0,
        }
    }

    /// Keeps the states of a line of the same length (a knob move must not
    /// click); a new length starts silent.
    fn keep(&mut self, old: &Self) {
        if old.n == self.n {
            for (node, was) in self.nodes.iter_mut().zip(&old.nodes) {
                (node.hl, node.hc, node.v) = (was.hl, was.hc, was.v);
            }
            self.input = old.input;
        }
    }

    /// Node `at`'s voltage (0 the input).
    fn at(&self, at: usize) -> f32 {
        match at.checked_sub(1) {
            None => self.input,
            Some(i) => self.nodes.get(i).map_or(0.0, |node| node.v),
        }
    }

    fn tick(&mut self, x: f32) {
        let (n, gl) = (self.n, self.gl);
        // Node k: −gl·v₋ + diag·v − gl·v₊ = hl − hl₊ + hc.
        let (mut hl_beyond, mut d_beyond) = (0.0, 0.0);
        for node in self.nodes.iter_mut().take(n).rev() {
            node.d = (node.hl - hl_beyond + node.hc + gl * d_beyond) * node.pivot;
            (hl_beyond, d_beyond) = (node.hl, node.d);
        }
        let mut before = x;
        for node in self.nodes.iter_mut().take(n) {
            node.v = node.d + node.ratio * before;
            node.hl += 2.0 * gl * (before - node.v);
            node.hc = 2.0 * node.gc * node.v - node.hc;
            before = node.v;
        }
        self.input = x;
    }
}

/// A scanner vibrato; see the module docs.
#[derive(Clone, Debug)]
pub struct Scanner {
    s: ScannerSettings,
    rate: f32,
    split: Split,
    line: Line,
    /// The left pickup's place in its turn, 0..1.
    turn: f32,
}

impl Scanner {
    #[must_use]
    pub fn new(s: ScannerSettings, rate: u32) -> Self {
        let rate = rate.max(1) as f32;
        Self {
            s,
            rate,
            split: Split::new(s.split_hz, rate),
            line: Line::new(&s, rate),
            turn: 0.0,
        }
    }

    /// New settings, keeping the line's and the split's states.
    pub fn set(&mut self, s: ScannerSettings) {
        let mut line = Line::new(&s, self.rate);
        line.keep(&self.line);
        let Split { s1, s2, .. } = self.split;
        self.split = Split {
            s1,
            s2,
            ..Split::new(s.split_hz, self.rate)
        };
        (self.s, self.line) = (s, line);
    }

    pub fn reset(&mut self) {
        *self = Self::new(self.s, self.rate as u32);
    }

    /// The pickup at `turn`: two neighbouring stations crossfaded.
    fn pickup(&self, turn: f32) -> f32 {
        let at = turn.rem_euclid(1.0) * STATIONS as f32;
        let i = (at as usize).min(STATIONS - 1);
        let f = at - i as f32;
        let node = |station: usize| {
            let step = station % STATIONS;
            let tap = if step < SCANNER_TAPS {
                step
            } else {
                STATIONS - step
            };
            let at = self.s.taps.get(tap).map_or(0, |&t| usize::from(t));
            self.line.at(at.min(self.line.n))
        };
        (1.0 - f) * node(i) + f * node(i + 1)
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let s = self.s;
        let step = s.hz.max(0.0) / self.rate;
        let (blend, mix) = (0.5 * s.blend.clamp(0.0, 1.0), s.mix.clamp(0.0, 1.0));
        for frame in io {
            let [l, r] = *frame;
            let (low, high) = self.split.tick(0.5 * (l + r));
            self.line.tick(low);
            let input = self.line.input;
            let around = s.high * high;
            let side = |turn: f32| (1.0 - blend) * self.pickup(turn) + blend * input + around;
            let (wl, wr) = (side(self.turn), side(self.turn + s.lead));
            *frame = [mix * wl + (1.0 - mix) * l, mix * wr + (1.0 - mix) * r];
            self.turn = (self.turn + step).rem_euclid(1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;

    fn line(load_ohm: f32) -> ScannerSettings {
        ScannerSettings {
            sections: 18,
            henry: 0.5,
            farad: 4e-9,
            last_farad: 4e-9,
            load_ohm,
            shunt_ohm: [f32::INFINITY; 6],
            taps: [18; SCANNER_TAPS],
            hz: 0.0,
            lead: 0.0,
            split_hz: 20_000.0,
            high: 0.0,
            blend: 0.0,
            mix: 1.0,
        }
    }

    /// A matched line delays a low tone by `n·√(LC)` at unit gain; a light
    /// load doubles it at the end (the open line's reflection).
    #[test]
    fn a_matched_line_delays_and_a_light_load_reflects() {
        let rate = 96_000;
        let level = |load: f32| {
            let mut sc = Scanner::new(line(load), rate);
            let hz = 200.0;
            let mut io: Vec<[f32; 2]> = (0..rate as usize)
                .map(|i| [(TAU * hz * i as f32 / rate as f32).sin(); 2])
                .collect();
            sc.process(&mut io);
            io[rate as usize / 2..]
                .iter()
                .map(|f| f[0].abs())
                .fold(0.0, f32::max)
        };
        let z0 = (0.5f32 / 4e-9).sqrt();
        assert!((level(z0) - 1.0).abs() < 0.05, "{}", level(z0));
        assert!(level(1e7) > 1.7, "{}", level(1e7));
    }
}
