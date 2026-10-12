//! Shared parameter lanes and worker-reserved state for the pinned v1 LP4.
use super::{BLOCK, Planar, ProcessorState, ladder_kernel};
use super::control::{ControlRamp, ControlRange, Parameter, PreparedParameter};

#[derive(Clone, Copy, Debug)]
pub struct LadderSettings {
    pub gain: Parameter,
    pub cutoff: Parameter,
    pub resonance: Parameter,
    pub record_version: u16,
}

impl LadderSettings {
    pub(super) fn valid(self) -> bool {
        [self.cutoff, self.resonance].iter().all(|p| {
            p.valid() && !matches!(p, Parameter::Expression { .. })
                && p.bounds().iter().all(|v| (0.0..=1.0).contains(v))
        }) && self.gain.valid() && !matches!(self.gain, Parameter::Expression { .. })
            && self.gain.bounds().iter().all(|v| (-1.0..=1.0).contains(v))
    }

    pub(super) fn compile(self, rate: u32, bindings: &mut Vec<ControlRange>) -> Ladder {
        ladder_kernel::prepare();
        Ladder {
            rate: rate as f32,
            parameters: [self.cutoff, self.resonance, self.gain].map(|p| p.compile(bindings)),
            record_version: self.record_version,
            modulation_index: usize::MAX,
        }
    }
}

pub(super) const CELLS: usize = 19;

pub(crate) struct Ladder {
    rate: f32,
    parameters: [PreparedParameter; 3],
    record_version: u16,
    pub(crate) modulation_index: usize,
}

impl Ladder {
    pub(crate) fn trace_parameters(&self) -> [(&'static str, PreparedParameter); 3] { [("cutoff",self.parameters[0]),("resonance",self.parameters[1]),("gain",self.parameters[2])] }
    pub(super) fn process(
        &self, state: &mut ProcessorState, cells: &mut [[f64; 2]],
        parameters: &[ControlRamp], block: &mut Planar, len: usize, at: u64,
        modulation: [f64; 4],
    ) -> bool {
        if state.aux[0] == 0.0 {
            cells.fill([0.0; 2]);
            state.aux[0] = 1.0;
        }
        let mut kernel = ladder_kernel::Ladder::restore(cells);
        kernel.record_version(self.record_version);
        kernel.enabled_modulation(modulation[3] != 0.);
        let values = |frame| {
            let mut values = self.parameters.map(|p| p.value(parameters, frame, None) as f32);
            values[0] = (values[0] + modulation[0] as f32).clamp(0., 1.);
            values[1] = (values[1] + modulation[1] as f32).clamp(0., 1.);
            values[2] += modulation[2] as f32;
            // v1 filter.rs: enabled Gain routes clamp even when their delta is zero.
            if (modulation[3] as u8) & 4 != 0 { values[2] = values[2].clamp(0., 1.); }
            values
        };
        let first = values(at);
        let last = values(at + len.saturating_sub(1) as u64);
        let mut scratch: [[f32; BLOCK]; 2] = std::array::from_fn(|c| block[c].map(|v| v as f32));
        let mut update = |kernel: &mut ladder_kernel::Ladder, value: [f32; 3]| {
            let previous = [cells[17][0] as f32, cells[17][1] as f32, cells[18][0] as f32];
            if cells[18][1] == 0.0 || value != previous {
                kernel.tune(value, self.rate);
                cells[17] = [f64::from(value[0]), f64::from(value[1])];
                cells[18] = [f64::from(value[2]), 1.0];
            }
        };
        let [left, right] = &mut scratch;
        if first == last {
            update(&mut kernel, first);
            kernel.process(&mut left[..len], &mut right[..len]);
        } else {
            for i in 0..len {
                update(&mut kernel, values(at + i as u64));
                kernel.process(&mut left[i..i + 1], &mut right[i..i + 1]);
            }
        }
        kernel.retain(cells);
        for c in 0..2 {
            for i in 0..len { block[c][i] = f64::from(scratch[c][i]); }
        }
        !cells.iter().flatten().all(|v| v.is_finite())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_lanes_retain_pinned_kernel_across_fragments_and_voice_reuse() {
        for rate in [32_000, 44_100, 48_000, 96_000] {
            let settings = LadderSettings { cutoff: Parameter::Constant(0.64),
                resonance: Parameter::Constant(0.7), gain: Parameter::Constant(0.2), record_version: 0x92 };
            let prepared = settings.compile(rate, &mut Vec::new());
            let source: [[f64; BLOCK]; 2] = std::array::from_fn(|c| std::array::from_fn(|i|
                ((i + 11 * c) as f64 * 0.31).sin() * 0.01));
            let mut native = ladder_kernel::Ladder::default();
            native.record_version(0x92);
            native.tune([0.64, 0.7, 0.2], rate as f32);
            let mut expected = source.map(|ch| ch.map(|v| v as f32));
            let [l, r] = &mut expected;
            native.process(l, r);
            let mut cells = [[0.0; 2]; CELLS];
            let mut state = ProcessorState::default();
            let mut whole = source;
            assert!(!prepared.process(&mut state, &mut cells, &[], &mut whole, BLOCK, 0, [0.; 4]));
            assert_eq!(whole, expected.map(|ch| ch.map(f64::from)));
            state = ProcessorState::default(); // recycled voice, same reserved storage
            let mut split = source;
            for (start, end) in [(0, 1), (1, 23), (23, 32), (32, 64)] {
                let mut fragment = [[0.0; BLOCK]; 2];
                for c in 0..2 { fragment[c][..end-start].copy_from_slice(&source[c][start..end]); }
                assert!(!prepared.process(&mut state, &mut cells, &[], &mut fragment, end-start, start as u64, [0.; 4]));
                for c in 0..2 { split[c][start..end].copy_from_slice(&fragment[c][..end-start]); }
            }
            assert_eq!(split, whole);
        }
    }
}
