//! Native FilterDJ controls and worker-reserved stereo state.
//! Test-only candidate: no processor, lowerer or importer admission.
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

#[derive(Clone, Copy, Debug, Default, PartialEq)]
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
            self.remaining = 0;
        }
    }
    fn advance(&mut self, request: Option<f32>, quanta: u32) {
        // Native pending requests take priority over completion of the old ramp.
        if let Some(target) = request {
            self.request(target, quanta);
        } else {
            self.clock();
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
                    for (i, ramp) in ramps.iter_mut().enumerate() {
                        let requested = if i < 2 { cutoff } else { resonance };
                        ramp.advance(requested.then_some(target[i]), self.quanta);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ar_pending_request_precedes_old_ramp_completion() {
        let mut ramp = Ramp {
            current: f32::from_bits(0x3effffff),
            target: 0.5,
            delta: 0.01,
            remaining: 1,
            active: true,
        };
        let current = ramp.current;
        ramp.advance(Some(0.75), 2);
        assert_eq!(ramp.current, current);
        assert_eq!(ramp.delta, (0.75 - current) * (1. / 64.));
        assert_eq!(ramp.remaining, 2);
        assert!(ramp.active);
        ramp.advance(None, 2);
        assert_eq!(ramp.current, current);
        ramp.advance(None, 2);
        assert_eq!(ramp.current, 0.75);
        assert_eq!(ramp.delta, 0.);
        assert!(!ramp.active);
    }

    #[test]
    fn ar_unchanged_requests_restart_and_tiny_steps_snap() {
        let mut ramp = Ramp {
            current: 0.25,
            target: 0.5,
            remaining: 1,
            active: true,
            ..Default::default()
        };
        ramp.advance(Some(0.5), 3);
        assert_eq!(ramp.current, 0.25);
        assert_eq!(ramp.remaining, 3);
        assert_eq!(ramp.delta, 0.25 * (1. / 96.));
        let tiny = f32::from_bits(ramp.current.to_bits() + 1);
        ramp.advance(Some(tiny), 3);
        assert_eq!(ramp.current, tiny);
        assert_eq!(ramp.remaining, 0);
        assert_eq!(ramp.delta, 0.);
        assert!(!ramp.active);
    }

    fn render(mode: u8, rate: u32, mask: u8, partition: usize) -> (Vec<[f32; 2]>, [[f64; 2]; CELLS], Vec<[Ramp; 3]>) {
        let mut ar = ArSettings {
            mode,
            cutoff: Parameter::Constant(0.5135),
            resonance: Parameter::Constant(0.7),
        }.compile(rate, &mut Vec::new());
        let mut state = ProcessorState::default();
        let mut cells = [[0.; 2]; CELLS];
        let mut audio = Vec::new();
        let mut snapshots = Vec::new();
        for (quantum, knobs) in [[0.5135, 0.7], [0.35, 0.3], [0.35, 0.3], [0.8, 0.9]].into_iter().enumerate() {
            ar.parameters = knobs.map(PreparedParameter::Constant);
            let mut offset = 0;
            while offset < QUANTUM as usize {
                let len = partition.min(QUANTUM as usize - offset);
                let mut block = [[0.; super::super::BLOCK]; 2];
                for ch in 0..2 {
                    for i in 0..len {
                        let at = quantum * QUANTUM as usize + offset + i;
                        block[ch][i] = f64::from((0.1 * (((at + 1) as f64 * 0.17) + ch as f64 * 0.4).sin()) as f32);
                    }
                }
                assert!(!ar.process(&mut state, &mut cells, &[], &mut block, len,
                    (quantum * QUANTUM as usize + offset) as u64, [0., 0., 0., f64::from(mask)]));
                for i in 0..len {
                    audio.push([block[0][i] as f32, block[1][i] as f32]);
                }
                offset += len;
            }
            let read = |i: usize| cells[i / 2][i % 2] as f32;
            snapshots.push(std::array::from_fn(|i| {
                let at = 22 + i * 5;
                Ramp {
                    current: read(at), target: read(at + 1), delta: read(at + 2),
                    remaining: read(at + 3) as u32, active: read(at + 4) != 0.,
                }
            }));
        }
        (audio, cells, snapshots)
    }

    #[test]
    fn ar_test_only_runtime_matches_natural_native_wrapper_checkpoints() {
        let native: serde_json::Value = serde_json::from_str(include_str!("ar_runtime_vectors.json")).unwrap();
        assert_eq!(native["binary_sha256"].as_str().unwrap(),
            "0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8");
        let cases = native["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 180);
        for case in cases {
            let mode = case["mode"].as_u64().unwrap() as u8;
            let rate = case["rate"].as_u64().unwrap() as u32;
            let mask = case["mask"].as_u64().unwrap() as u8;
            let (audio, _, ramps) = render(mode, rate, mask, 32);
            for quantum in 0..4 {
                for (point, at) in [0, 1, 7, 15, 31].into_iter().enumerate() {
                    for ch in 0..2 {
                        let actual = audio[quantum * 32 + at][ch];
                        let expected = case["checkpoints"][quantum][point][ch].as_f64().unwrap() as f32;
                        assert!((actual - expected).abs() <= 2e-6,
                            "mode={mode} rate={rate} mask={mask} quantum={quantum} frame={at} ch={ch}: {actual} vs {expected}");
                    }
                }
                for lane in 0..3 {
                    let expected = &case["ramps"][quantum][lane];
                    let actual = ramps[quantum][lane];
                    assert_eq!([actual.current, actual.target, actual.delta],
                        ["current", "target", "delta"].map(|key| expected[key].as_f64().unwrap() as f32),
                        "mode={mode} rate={rate} mask={mask} quantum={quantum} lane={lane}");
                    // Native inactive idle countdown is -1; ours has no active work at 0.
                    assert_eq!(actual.remaining, expected["remaining"].as_i64().unwrap().max(0) as u32);
                    assert_eq!(actual.active, expected["active"].as_u64().unwrap() != 0);
                }
            }
        }
    }

    #[test]
    fn ar_test_only_runtime_preserves_quantum_state_across_partitions() {
        for mode in 0..9 {
            for rate in [8000, 44100, 48000, 96000, 192000] {
                for mask in 0..4 {
                    let whole = render(mode, rate, mask, 32);
                    for partition in [1, 7, 15, 31] {
                        assert_eq!(render(mode, rate, mask, partition), whole,
                            "mode={mode} rate={rate} mask={mask} partition={partition}");
                    }
                    assert!(whole.0.iter().flatten().all(|v| v.is_finite()));
                }
            }
        }
    }
}
