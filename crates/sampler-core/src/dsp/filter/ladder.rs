//! Shared parameter lanes and worker-reserved state for the pinned v1 Ladder kernel. The
//! magnitude preview is this Rust kernel's small-signal response, not a native transfer
//! measurement (CORE-FILTERS-053).
use super::super::control::{ControlRamp, ControlRange, Parameter, PreparedParameter};
use super::super::kaiser::{self, Kaiser};
use super::super::{BLOCK, Planar, ProcessorState};
use super::ladder_kernel;

#[derive(Clone, Copy, Debug)]
pub struct LadderSettings {
    pub gain: Parameter,
    pub cutoff: Parameter,
    pub resonance: Parameter,
    /// Alternative cutoff path (saved flag 0x49).
    pub cutoff_path: bool,
    /// Output selector: saved Morphology type minus 29 (1..=12).
    pub taps: u8,
    /// Native HQ: the core runs at twice the rate inside the shared 2x Kaiser oversampler, with
    /// its controls sampled once per 32 host frames.
    pub high_quality: bool,
}

impl LadderSettings {
    /// Native normalized cutoff in hertz, for the control-thread editor.
    pub fn cutoff_hz(normalized: f32) -> f32 {
        ladder_kernel::prepare();
        ladder_kernel::cutoff(normalized)
    }

    /// Small-signal response of the same LP4 kernel used by playback.
    pub fn magnitude(knobs: [f32; 3], hz: f32, rate: u32) -> f32 {
        ladder_kernel::prepare();
        ladder_kernel::Ladder::magnitude(knobs, hz, rate as f32)
    }

    pub(crate) fn valid(self) -> bool {
        (1..=12).contains(&self.taps)
            && [self.cutoff, self.resonance].iter().all(|p| {
                p.valid()
                    && !matches!(p, Parameter::Expression { .. })
                    && p.bounds().iter().all(|v| (0.0..=1.0).contains(v))
            })
            && self.gain.valid()
            && !matches!(self.gain, Parameter::Expression { .. })
            && self.gain.bounds().iter().all(|v| (-1.0..=1.0).contains(v))
    }

    pub(crate) fn compile(self, rate: u32, bindings: &mut Vec<ControlRange>) -> Ladder {
        ladder_kernel::prepare();
        let internal = if self.high_quality { 2 * rate } else { rate };
        Ladder {
            rate: internal as f32,
            parameters: [self.cutoff, self.resonance, self.gain].map(|p| p.compile(bindings)),
            cutoff_path: self.cutoff_path,
            taps: self.taps,
            os: self.high_quality.then(|| Kaiser::new(2)),
        }
    }
}

/// Kernel state cells; the HQ oversampler's registers follow them.
pub(crate) const CELLS: usize = 19;
/// Native control cadence: setters and the control tick run once per 32 host frames, in HQ too.
/// (Non-HQ gets the same cadence from the kernel's own 32-sample phase clock.)
const CONTROL_FRAMES: u64 = 32;

pub(crate) struct Ladder {
    rate: f32,
    parameters: [PreparedParameter; 3],
    cutoff_path: bool,
    taps: u8,
    os: Option<Kaiser>,
}

/// Retune the kernel when a control changed (or was never set).
fn tune(kernel: &mut ladder_kernel::Ladder, cells: &mut [[f64; 2]], value: [f32; 3], rate: f32) {
    let previous = [cells[17][0] as f32, cells[17][1] as f32, cells[18][0] as f32];
    if cells[18][1] == 0.0 || value != previous {
        kernel.tune(value, rate);
        cells[17] = [f64::from(value[0]), f64::from(value[1])];
        cells[18] = [f64::from(value[2]), 1.0];
    }
}

impl Ladder {
    /// Cells to reserve: the kernel's, plus both channels' oversampler registers in HQ.
    pub(crate) fn cells(&self) -> usize {
        CELLS + if self.os.is_some() { kaiser::WORDS } else { 0 }
    }

    pub(crate) fn trace_parameters(&self) -> [(&'static str, PreparedParameter); 3] {
        [
            ("cutoff", self.parameters[0]),
            ("resonance", self.parameters[1]),
            ("gain", self.parameters[2]),
        ]
    }
    pub(crate) fn process(
        &self,
        state: &mut ProcessorState,
        cells: &mut [[f64; 2]],
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
        modulation: [f64; 4],
    ) -> bool {
        if state.aux[0] == 0.0 {
            cells.fill([0.0; 2]);
            state.aux[0] = 1.0;
        }
        let mut kernel = ladder_kernel::Ladder::restore(cells);
        kernel.set_cutoff_path(self.cutoff_path);
        kernel.set_taps(self.taps);
        kernel.enabled_modulation(modulation[3] != 0.);
        let values = |frame| {
            let mut values = self
                .parameters
                .map(|p| p.value(parameters, frame, None) as f32);
            values[0] = (values[0] + modulation[0] as f32).clamp(0., 1.);
            values[1] = (values[1] + modulation[1] as f32).clamp(0., 1.);
            values[2] += modulation[2] as f32;
            // v1 filter.rs: enabled Gain routes clamp even when their delta is zero.
            if (modulation[3] as u8) & 4 != 0 {
                values[2] = values[2].clamp(0., 1.);
            }
            values
        };
        let first = values(at);
        let last = values(at + len.saturating_sub(1) as u64);
        let mut scratch: [[f32; BLOCK]; 2] = std::array::from_fn(|c| block[c].map(|v| v as f32));
        let constant = first == last;
        let [left, right] = &mut scratch;
        match &self.os {
            Some(os) => {
                // Per host frame: 2x up, the kernel at twice the rate, 2x down (the native order).
                // Controls sample only at 32-frame boundaries, as the native host does; the first
                // call of a voice snapshots its targets (tune's unset-cell check).
                if cells[18][1] == 0.0 {
                    tune(&mut kernel, cells, first, self.rate);
                }
                let mut states: [kaiser::State; 2] = Default::default();
                for (c, s) in states.iter_mut().enumerate() {
                    s.load(&cells[CELLS..].as_flattened()[c * kaiser::WORDS..(c + 1) * kaiser::WORDS]);
                }
                for i in 0..len {
                    let frame = at + i as u64;
                    if frame % CONTROL_FRAMES == 0 {
                        tune(&mut kernel, cells, values(frame), self.rate);
                    }
                    let (mut l, mut r) = ([0f32; 2], [0f32; 2]);
                    os.up(&mut states[0], left[i], &mut l);
                    os.up(&mut states[1], right[i], &mut r);
                    kernel.process(&mut l, &mut r);
                    left[i] = os.down(&mut states[0], &l);
                    right[i] = os.down(&mut states[1], &r);
                }
                let flat = cells[CELLS..].as_flattened_mut();
                for (c, s) in states.iter().enumerate() {
                    s.save(&mut flat[c * kaiser::WORDS..(c + 1) * kaiser::WORDS]);
                }
            }
            None if constant => {
                tune(&mut kernel, cells, first, self.rate);
                kernel.process(&mut left[..len], &mut right[..len]);
            }
            None => {
                for i in 0..len {
                    tune(&mut kernel, cells, values(at + i as u64), self.rate);
                    kernel.process(&mut left[i..i + 1], &mut right[i..i + 1]);
                }
            }
        }
        kernel.retain(cells);
        for c in 0..2 {
            for i in 0..len {
                block[c][i] = f64::from(scratch[c][i]);
            }
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
            let settings = LadderSettings {
                cutoff: Parameter::Constant(0.64),
                resonance: Parameter::Constant(0.7),
                gain: Parameter::Constant(0.2),
                cutoff_path: true,
                taps: 4,
                high_quality: false,
            };
            let prepared = settings.compile(rate, &mut Vec::new());
            let source: [[f64; BLOCK]; 2] = std::array::from_fn(|c| {
                std::array::from_fn(|i| ((i + 11 * c) as f64 * 0.31).sin() * 0.01)
            });
            let mut native = ladder_kernel::Ladder::default();
            native.set_cutoff_path(true);
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
                for c in 0..2 {
                    fragment[c][..end - start].copy_from_slice(&source[c][start..end]);
                }
                assert!(!prepared.process(
                    &mut state,
                    &mut cells,
                    &[],
                    &mut fragment,
                    end - start,
                    start as u64,
                    [0.; 4]
                ));
                for c in 0..2 {
                    split[c][start..end].copy_from_slice(&fragment[c][..end - start]);
                }
            }
            assert_eq!(split, whole);
        }
    }

    #[test]
    fn high_quality_is_the_shared_2x_oversampler_around_the_kernel() {
        for rate in [44_100, 48_000] {
            let knobs = [0.64, 0.7, 0.2];
            let settings = LadderSettings {
                cutoff: Parameter::Constant(knobs[0]),
                resonance: Parameter::Constant(knobs[1]),
                gain: Parameter::Constant(knobs[2]),
                cutoff_path: false,
                taps: 4,
                high_quality: true,
            };
            let prepared = settings.compile(rate, &mut Vec::new());
            let source: [[f64; BLOCK]; 2] = std::array::from_fn(|c| {
                std::array::from_fn(|i| ((i + 7 * c) as f64 * 0.23).sin() * 0.05)
            });
            // Reference composition: up, kernel at twice the rate, down, one host frame at a time.
            let os = Kaiser::new(2);
            let mut kernel = ladder_kernel::Ladder::default();
            kernel.set_cutoff_path(false);
            kernel.set_taps(4);
            kernel.tune(knobs.map(|v| v as f32), 2.0 * rate as f32);
            let mut states: [kaiser::State; 2] = Default::default();
            let mut expected = source;
            for i in 0..BLOCK {
                let (mut l, mut r) = ([0f32; 2], [0f32; 2]);
                os.up(&mut states[0], source[0][i] as f32, &mut l);
                os.up(&mut states[1], source[1][i] as f32, &mut r);
                kernel.process(&mut l, &mut r);
                expected[0][i] = f64::from(os.down(&mut states[0], &l));
                expected[1][i] = f64::from(os.down(&mut states[1], &r));
            }
            let mut cells = [[0.0; 2]; CELLS + kaiser::WORDS];
            assert_eq!(prepared.cells(), cells.len());
            let mut whole = source;
            assert!(!prepared.process(&mut ProcessorState::default(), &mut cells, &[], &mut whole, BLOCK, 0, [0.; 4]));
            assert_eq!(whole, expected);
            // Fragments carry the oversampler's registers across calls.
            let mut state = ProcessorState::default();
            cells = [[0.0; 2]; CELLS + kaiser::WORDS];
            let mut split = source;
            for (start, end) in [(0, 5), (5, 32), (32, BLOCK)] {
                let mut fragment = [[0.0; BLOCK]; 2];
                for c in 0..2 {
                    fragment[c][..end - start].copy_from_slice(&source[c][start..end]);
                }
                assert!(!prepared.process(&mut state, &mut cells, &[], &mut fragment, end - start, start as u64, [0.; 4]));
                for c in 0..2 {
                    split[c][start..end].copy_from_slice(&fragment[c][..end - start]);
                }
            }
            assert_eq!(split, expected);
        }
    }

    #[test]
    fn native_kemu_vectors_through_the_production_path() {
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("ladder_kemu_vectors.json")).unwrap();
        let num = |v: &serde_json::Value| v.as_f64().unwrap();
        let mut failures = Vec::new();
        for case in &cases {
            let settings = LadderSettings {
                // Knobs arrive through the modulation lanes once per 32-frame block, as the
                // native setters do; the base values stay zero.
                gain: Parameter::Constant(0.0),
                cutoff: Parameter::Constant(0.0),
                resonance: Parameter::Constant(0.0),
                cutoff_path: case["flag49"].as_u64().unwrap() != 0,
                taps: case["mode"].as_u64().unwrap() as u8 + 1,
                high_quality: case["hq"].as_u64().unwrap_or(0) != 0,
            };
            let prepared = settings.compile(num(&case["rate"]) as u32, &mut Vec::new());
            let schedule: Vec<[f64; 3]> = case["schedule"]
                .as_array()
                .unwrap()
                .iter()
                .map(|k| [num(&k[1]), num(&k[2]), num(&k[0])])
                .collect();
            let stim: Vec<Vec<f64>> = case["stim"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c.as_array().unwrap().iter().map(num).collect())
                .collect();
            let mut cells = vec![[0.0; 2]; prepared.cells()];
            let mut state = ProcessorState::default();
            let mut out: [Vec<f64>; 2] = Default::default();
            for (block, knobs) in schedule.iter().enumerate() {
                let range = block * 32..(block + 1) * 32;
                let mut buffer: Planar = [[0.0; BLOCK]; 2];
                buffer[0][..32].copy_from_slice(&stim[0][range.clone()]);
                buffer[1][..32].copy_from_slice(&stim[1][range]);
                let modulation = [knobs[0], knobs[1], knobs[2], 0.0];
                assert!(!prepared.process(&mut state, &mut cells, &[], &mut buffer, 32, (block * 32) as u64, modulation));
                for c in 0..2 {
                    out[c].extend_from_slice(&buffer[c][..32]);
                }
            }
            let name = case["name"].as_str().unwrap();
            let peak = (0..2)
                .flat_map(|c| case[if c == 0 { "outL" } else { "outR" }].as_array().unwrap().iter().map(num))
                .fold(0f64, |m, v| m.max(v.abs()));
            // Stored native outputs are f32; the bound is relative to the case peak.
            for (c, key) in [(0, "outL"), (1, "outR")] {
                let want: Vec<f64> = case[key].as_array().unwrap().iter().map(num).collect();
                for (i, (&got, &want)) in out[c].iter().zip(&want).enumerate() {
                    if (got - want).abs() > 1e-6 * peak.max(1e-4) {
                        failures.push(format!("{name} {key}[{i}] ours {got:e} native {want:e}"));
                        break;
                    }
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn hq_controls_sample_once_per_32_host_frames_whatever_the_fragmenting() {
        let settings = LadderSettings {
            gain: Parameter::Constant(0.0),
            cutoff: Parameter::Constant(0.0),
            resonance: Parameter::Constant(0.0),
            cutoff_path: false,
            taps: 4,
            high_quality: true,
        };
        let prepared = settings.compile(48_000, &mut Vec::new());
        let knob = |frame: u64| 0.5 + 0.4 * (frame as f64 * 0.37).sin();
        // One frame per call, so the modulation can change on every host frame.
        let run = |sample_at: &dyn Fn(u64) -> u64| -> Vec<f64> {
            let mut cells = vec![[0.0; 2]; prepared.cells()];
            let mut state = ProcessorState::default();
            (0..256u64)
                .map(|frame| {
                    let mut block: Planar = [[0.0; BLOCK]; 2];
                    block[0][0] = (frame as f64 * 0.11).sin() * 0.05;
                    block[1][0] = (frame as f64 * 0.13).cos() * 0.05;
                    let modulation = [knob(sample_at(frame)), 0.3, 0.1, 0.0];
                    assert!(!prepared.process(&mut state, &mut cells, &[], &mut block, 1, frame, modulation));
                    block[0][0]
                })
                .collect()
        };
        let per_frame = run(&|frame| frame);
        let per_block = run(&|frame| frame / 32 * 32);
        assert_eq!(per_frame, per_block);
    }
}
