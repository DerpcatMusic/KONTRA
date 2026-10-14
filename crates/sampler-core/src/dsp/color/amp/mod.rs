//! Kontakt amp, cabinet and pedal models as configurations of the color unit's shaper stage.
//! Each model is its native object's process ported in f32 operation order and checked
//! against kemu vectors of the native ctor, SR setup, control laws and kernel.
mod acbox;
mod cabinet;
mod cat;
mod crywah;
mod dstortion;
mod twang;

use super::super::Planar;
use sampler_ir::fx::color::{Amp, AmpModel};

pub(super) enum Engine {
    Cabinet(cabinet::Engine),
    CryWah(crywah::Engine),
    Twang(twang::Engine),
    AcBox(acbox::Engine),
    Cat(cat::Engine),
    DStortion(dstortion::Engine),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum State {
    #[default]
    None,
    Cabinet(cabinet::State),
    CryWah(crywah::State),
    Twang(twang::State),
    AcBox(acbox::State),
    Cat(cat::State),
    DStortion(dstortion::State),
}

impl State {
    pub(super) fn finite(&self) -> bool {
        match self {
            Self::None => true,
            Self::Cabinet(s) => s.finite(),
            Self::CryWah(s) => s.finite(),
            Self::Twang(s) => s.finite(),
            Self::AcBox(s) => s.finite(),
            Self::Cat(s) => s.finite(),
            Self::DStortion(s) => s.finite(),
        }
    }
}

/// The model's state, reset when the slot holds another model's.
macro_rules! slot {
    ($state:expr, $variant:ident) => {{
        if !matches!($state, State::$variant(_)) {
            *$state = State::$variant(Default::default());
        }
        let State::$variant(s) = $state else { unreachable!() };
        s
    }};
}

impl Engine {
    pub(super) fn new(a: Amp, rate: u32) -> Self {
        let k = a.knobs;
        match a.model {
            AmpModel::Cabinet => Self::Cabinet(cabinet::Engine::new(k[4], k[0], k[1], k[2], k[3], rate)),
            AmpModel::CryWah => Self::CryWah(crywah::Engine::new(k[0], k[1] != 0.0, rate)),
            AmpModel::Twang => Self::Twang(twang::Engine::new([k[0], k[1], k[2], k[3], k[4], k[5]], rate)),
            AmpModel::AcBox => Self::AcBox(acbox::Engine::new([k[0], k[1], k[2], k[3], k[4], k[5], k[6], k[7]], rate)),
            AmpModel::Cat => Self::Cat(cat::Engine::new([k[0], k[1], k[2], k[3], k[4], k[5], k[6], k[7]], rate)),
            AmpModel::DStortion => Self::DStortion(dstortion::Engine::new([k[0], k[1], k[2], k[3], k[4], k[5], k[6]], rate)),
        }
    }

    pub(super) fn process(&self, s: &mut State, block: &mut Planar, len: usize, ring: &mut [[f64; 2]]) {
        match self {
            Self::Cabinet(e) => e.process(slot!(s, Cabinet), block, len, ring),
            Self::CryWah(e) => e.process(slot!(s, CryWah), block, len),
            Self::Twang(e) => e.process(slot!(s, Twang), block, len),
            Self::AcBox(e) => e.process(slot!(s, AcBox), block, len),
            Self::Cat(e) => e.process(slot!(s, Cat), block, len),
            Self::DStortion(e) => e.process(slot!(s, DStortion), block, len),
        }
    }
}

/// Delay storage a model needs.
pub(super) fn frames(a: Amp, rate: u32) -> usize {
    match a.model {
        AmpModel::Cabinet => cabinet::frames(rate),
        AmpModel::CryWah | AmpModel::DStortion | AmpModel::Cat | AmpModel::AcBox | AmpModel::Twang => 0,
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::super::super::{Planar, BLOCK};

    /// ampemu.py `stimulus(1024)`: impulses, an LCG noise burst, then a square.
    pub(in super::super) fn stimulus() -> Vec<[f32; 2]> {
        let mut seed: u64 = 12345;
        (0..1024)
            .map(|i| {
                seed = (seed * 1_103_515_245 + 12345) & 0x7fff_ffff;
                let noise = (seed as f64 / f64::from(0x7fff_ffff) - 0.5) as f32;
                match i {
                    0 => [1.0, 0.0],
                    3 => [0.0, 0.5],
                    64..192 => [noise, (-f64::from(noise) * 0.7) as f32],
                    256.. => {
                        let sq: f32 = if (i / 109) % 2 == 0 { 0.5 } else { -0.5 };
                        [sq, sq * 0.25]
                    }
                    _ => [0.0, 0.0],
                }
            })
            .collect()
    }

    /// Runs `cases` units over the stimulus in 64-frame blocks against interleaved native
    /// vectors, to 1e-6 absolute or relative.
    pub(in super::super) fn check(raw: &[u8], cases: usize, make: impl Fn(usize) -> Box<dyn FnMut(&mut Planar, usize)>) {
        let native: Vec<f32> = raw.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        assert_eq!(native.len(), cases * 2048);
        let input = stimulus();
        for n in 0..cases {
            let mut unit = make(n);
            for start in (0..1024).step_by(BLOCK) {
                let mut block: Planar = [[0.0; BLOCK]; 2];
                for i in 0..BLOCK {
                    block[0][i] = f64::from(input[start + i][0]);
                    block[1][i] = f64::from(input[start + i][1]);
                }
                unit(&mut block, BLOCK);
                for i in 0..BLOCK {
                    for c in 0..2 {
                        let want = native[(n * 1024 + start + i) * 2 + c];
                        let got = block[c][i] as f32;
                        assert!(
                            (got - want).abs() <= 1e-6 * want.abs().max(1.0),
                            "case {n} frame {} ch {c}: {got} vs {want}",
                            start + i
                        );
                    }
                }
            }
        }
    }
}
