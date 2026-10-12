//! Native FilterDJ controls and worker-reserved stereo state.
//! Disconnected until the original-byte admission checks pass.
use super::ar_kernel::{self, ArKernel};
use super::control::{ControlRamp, ControlRange, Parameter, PreparedParameter};
use super::{Planar, ProcessorState};

const QUANTUM: u32 = 32;
pub(super) const CELLS: usize = 22;

#[derive(Clone, Copy, Debug)]
pub struct ArSettings {
    pub mode: u8,
    pub cutoff: Parameter,
    pub resonance: Parameter,
}

impl ArSettings {
    pub(super) fn valid(self) -> bool {
        self.mode < 9
            && [self.cutoff, self.resonance].iter().all(|p| {
                p.valid()
                    && !matches!(p, Parameter::Expression { .. })
                    && p.bounds().iter().all(|v| (0.0..=1.0).contains(v))
            })
    }

    pub(super) fn compile(self, rate: u32, bindings: &mut Vec<ControlRange>) -> Ar {
        ar_kernel::prepare();
        let rate = rate as f32;
        Ar {
            mode: self.mode,
            parameters: [self.cutoff, self.resonance].map(|p| p.compile(bindings)),
            rate,
            quanta: ((rate * 0.001 / QUANTUM as f32 + 0.5) as u32).max(1),
            modulation_index: usize::MAX,
        }
    }
}

pub(crate) struct Ar {
    mode: u8,
    parameters: [PreparedParameter; 2],
    rate: f32,
    quanta: u32,
    pub(crate) modulation_index: usize,
}

#[derive(Clone, Copy, Default)]
struct Ramp {
    current: f32,
    target: f32,
    delta: f32,
    remaining: u32,
    active: bool,
}

impl Ramp {
    fn request(&mut self, target: f32, quanta: u32) {
        self.target = target;
        self.remaining = quanta;
        self.delta = (target - self.current) * (1. / (quanta * QUANTUM) as f32);
        self.active = self.delta * self.delta >= 1e-15;
        if !self.active {
            self.current = target;
            self.delta = 0.;
        }
    }
    fn clock(&mut self) {
        if self.remaining > 0 {
            self.remaining -= 1;
            if self.remaining == 0 {
                self.current = self.target;
                self.delta = 0.;
                self.active = false;
            }
        }
    }
}

impl Ar {
    pub(crate) fn trace_parameters(&self) -> [(&'static str, PreparedParameter); 2] {
        [
            ("cutoff", self.parameters[0]),
            ("resonance", self.parameters[1]),
        ]
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn process(
        &self,
        state: &mut ProcessorState,
        cells: &mut [[f64; 2]],
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
        modulation: [f64; 4],
    ) -> bool {
        if len == 0 {
            return false;
        }
        if state.aux[0] == 0. {
            cells.fill([0.; 2]);
            state.aux[0] = 1.;
        }
        let read = |i: usize| cells[i / 2][i % 2] as f32;
        let mut kernel = ArKernel {
            channels: std::array::from_fn(|ch| std::array::from_fn(|i| read(ch * 9 + i))),
            detector: read(18),
            adaptation: read(19),
            feedback: read(20),
            cap: read(21),
        };
        let mut ramps: [Ramp; 3] = std::array::from_fn(|i| {
            let at = 22 + i * 5;
            Ramp {
                current: read(at),
                target: read(at + 1),
                delta: read(at + 2),
                remaining: read(at + 3) as u32,
                active: read(at + 4) != 0.,
            }
        });
        let mut countdown = read(37) as u32;
        let mut seeded = read(38) != 0.;
        let mut previous = [read(39), read(40)];
        let mut dirty = read(41) != 0.;
        let mut position = 0;
        while position < len {
            if countdown == 0 {
                countdown = QUANTUM;
                let knobs = std::array::from_fn(|i| {
                    (self.parameters[i].value(parameters, at + position as u64, None) as f32
                        + modulation[i] as f32)
                        .clamp(0., 1.)
                });
                let target = ar_kernel::targets(knobs[0], knobs[1], self.rate);
                if !seeded {
                    ramps = target.map(|current| Ramp {
                        current,
                        target: current,
                        ..Default::default()
                    });
                    seeded = true;
                    dirty = true;
                } else {
                    // Active routes re-request every quantum, including zero depth.
                    let mask = modulation[3] as u8;
                    let cutoff = knobs[0] != previous[0] || mask & 1 != 0;
                    let resonance = knobs[1] != previous[1] || mask & 2 != 0;
                    for ramp in &mut ramps {
                        ramp.clock();
                    }
                    if cutoff {
                        ramps[0].request(target[0], self.quanta);
                        ramps[1].request(target[1], self.quanta);
                    }
                    if resonance {
                        ramps[2].request(target[2], self.quanta);
                    }
                }
                previous = knobs;
            }
            let run = (countdown as usize).min(len - position);
            let ramping = dirty || ramps.iter().any(|r| r.active);
            for i in position..position + run {
                if ramping {
                    for r in &mut ramps {
                        r.current += r.delta;
                    }
                }
                let output = kernel.tick(
                    [block[0][i] as f32, block[1][i] as f32],
                    self.mode,
                    ramps.map(|r| r.current),
                    1. / self.rate,
                    ramping,
                );
                for ch in 0..2 {
                    block[ch][i] = f64::from(output[ch]);
                }
            }
            dirty = false;
            countdown -= run as u32;
            position += run;
        }
        let mut write = |i: usize, v: f32| cells[i / 2][i % 2] = f64::from(v);
        for ch in 0..2 {
            for i in 0..9 {
                write(ch * 9 + i, kernel.channels[ch][i]);
            }
        }
        for (i, v) in [
            kernel.detector,
            kernel.adaptation,
            kernel.feedback,
            kernel.cap,
        ]
        .into_iter()
        .enumerate()
        {
            write(18 + i, v);
        }
        for (i, r) in ramps.into_iter().enumerate() {
            for (j, v) in [
                r.current,
                r.target,
                r.delta,
                r.remaining as f32,
                u8::from(r.active) as f32,
            ]
            .into_iter()
            .enumerate()
            {
                write(22 + i * 5 + j, v);
            }
        }
        write(37, countdown as f32);
        write(38, u8::from(seeded) as f32);
        write(39, previous[0]);
        write(40, previous[1]);
        write(41, u8::from(dirty) as f32);
        !cells.iter().flatten().all(|v| v.is_finite())
    }
}
