//! One call from an NKI path to a playable plan: translate to the IR, decode
//! the samples it needs, compile its scripts and lower it. What could not be
//! carried over is returned with the plan, never dropped silently.

use crate::{Kontakt, LoadError, read};
use sampler_core::{Pcm, Prepared, lower::LowerError};
use sampler_ir as ir;
use std::{ops::RangeInclusive, path::Path};

#[derive(Clone, Debug)]
pub struct Options {
    /// Output sample rate of the plan.
    pub rate: u32,
    /// Only zones overlapping these keys are loaded (and their samples decoded).
    pub keys: RangeInclusive<u8>,
    /// Compile and bind the instrument's KSP; when off, scripts are reported.
    pub scripts: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { rate: 48000, keys: 0..=127, scripts: true }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Progress<'a> {
    Translated { zones: usize, assets: usize },
    Decoding { done: usize, total: usize, sample: &'a Path },
    Lowering,
}

pub struct Loaded {
    /// The instrument as translated; its `unsupported` list is the report,
    /// including scripts that did not compile.
    pub instrument: ir::Instrument,
    pub plan: Prepared,
}

/// Load the Kontakt instrument at `path` as a plan at `options.rate`.
pub fn load(path: &Path, options: &Options, mut progress: impl FnMut(Progress)) -> Result<Loaded, LoadError> {
    let Kontakt { mut instrument, locations, mut samples } = read(path)?;
    let (low, high) = (*options.keys.start(), *options.keys.end());
    let kept = instrument.retain_zones(|z| z.keys.low <= high && z.keys.high >= low);
    progress(Progress::Translated { zones: instrument.zones.len(), assets: kept.len() });
    let mut pcm = Vec::with_capacity(kept.len());
    for (done, &asset) in kept.iter().enumerate() {
        let sample = &locations[asset];
        progress(Progress::Decoding { done, total: kept.len(), sample });
        let decoded = samples.decode(sample)?;
        let frames = decoded.frames.into_boxed_slice();
        pcm.push(Pcm::new(decoded.rate, frames).map_err(|e| LoadError::Invalid { path: sample.clone(), reason: e.to_string() })?);
    }
    progress(Progress::Lowering);
    prepare(instrument, options.rate, pcm, options.scripts)
}

/// Lower a translated instrument whose asset audio is `pcm`, binding its
/// scripts when `scripts` is set and every one of them compiles. Scripts
/// interact through shared state, so a partial set is never bound.
pub fn prepare(mut instrument: ir::Instrument, rate: u32, pcm: Vec<Pcm>, scripts: bool) -> Result<Loaded, LoadError> {
    let limits = sampler_ksp::Limits { source_bytes: 4 << 20, instructions: 1 << 20, variables: 1 << 16, array_cells: 1 << 22 };
    let mut compiled = Vec::new();
    let mut failed = Vec::new();
    for behavior in &instrument.behaviors {
        let result = match behavior.language {
            _ if !scripts => Err("scripts disabled".to_string()),
            ir::Language::Ksp => sampler_ksp::compile(&behavior.source, rate, limits, &[]).map_err(|e| e.to_string()),
            ref other => Err(format!("{other:?} has no frontend")),
        };
        match result {
            Ok(script) => compiled.push(script),
            Err(error) => failed.push(ir::Unsupported {
                location: behavior.name.clone(),
                feature: "script".into(),
                value: error,
                reason: ir::Reason::NotModeled,
            }),
        }
    }
    let lowered = if failed.is_empty() {
        sampler_core::lower::lower(&instrument, rate, pcm, |_, plan| {
            sampler_ksp::bind_modules(compiled, plan)
                .map_err(|e| LowerError::Behavior { module: "KSP".into(), message: e.to_string() })
        })
    } else {
        instrument.unsupported.append(&mut failed);
        let behaviors = std::mem::take(&mut instrument.behaviors);
        let lowered = sampler_core::lower::lower(&instrument, rate, pcm, |_, plan| Ok(plan));
        instrument.behaviors = behaviors;
        lowered
    };
    Ok(Loaded { plan: lowered.map_err(LoadError::Lower)?, instrument })
}
