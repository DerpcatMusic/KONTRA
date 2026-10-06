//! One call from an NKI path to a playable plan: translate to the IR, decode
//! the samples it needs, compile its scripts and lower it. What could not be
//! carried over is returned with the plan, never dropped silently.

use crate::{Kontakt, LoadError, Resources, read};
use sampler_ksp::model::Value;
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
    /// The instrument's file, whose library holds its script pictures;
    /// [`load`] fills it in when unset.
    pub library: Option<std::path::PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            rate: 48000,
            keys: 0..=127,
            scripts: true,
            library: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Progress<'a> {
    Translated {
        zones: usize,
        assets: usize,
    },
    Decoding {
        done: usize,
        total: usize,
        sample: &'a Path,
    },
    Lowering,
}

pub struct Loaded {
    /// The instrument as translated; its `unsupported` list is the report,
    /// including scripts that did not compile.
    pub instrument: ir::Instrument,
    pub plan: Prepared,
    /// The bound scripts' interfaces, in script order. Image assets carry
    /// the library's picture layouts when [`Options::library`] is known.
    pub interfaces: Vec<sampler_ui_ir::Interface>,
}

/// Load the Kontakt instrument at `path` as a plan at `options.rate`.
pub fn load(
    path: &Path,
    options: &Options,
    progress: impl FnMut(Progress),
) -> Result<Loaded, LoadError> {
    load_cancelable(path, options, progress, || false)
}

/// [`load`], stopping with [`LoadError::Canceled`] once `canceled` returns true;
/// it is polled before each sample is decoded and before lowering.
pub fn load_cancelable(
    path: &Path,
    options: &Options,
    progress: impl FnMut(Progress),
    canceled: impl Fn() -> bool,
) -> Result<Loaded, LoadError> {
    let options = Options {
        library: options.library.clone().or_else(|| Some(path.into())),
        ..options.clone()
    };
    load_read(read(path)?, &options, progress, canceled)
}

/// [`load_cancelable`] for an instrument already [`read`], so a caller can
/// reshape its IR (say, give each group a bus) before samples are decoded.
pub fn load_read(
    kontakt: Kontakt,
    options: &Options,
    mut progress: impl FnMut(Progress),
    canceled: impl Fn() -> bool,
) -> Result<Loaded, LoadError> {
    let Kontakt {
        mut instrument,
        locations,
        mut samples,
    } = kontakt;
    let (low, high) = (*options.keys.start(), *options.keys.end());
    let kept = instrument.retain_zones(|z| z.keys.low <= high && z.keys.high >= low);
    progress(Progress::Translated {
        zones: instrument.zones.len(),
        assets: kept.len(),
    });
    let mut pcm = Vec::with_capacity(kept.len());
    for (done, &asset) in kept.iter().enumerate() {
        if canceled() {
            return Err(LoadError::Canceled);
        }
        let sample = &locations[asset];
        progress(Progress::Decoding {
            done,
            total: kept.len(),
            sample,
        });
        let decoded = samples.decode(sample)?;
        let frames = decoded.frames.into_boxed_slice();
        pcm.push(
            Pcm::new(decoded.rate, frames).map_err(|e| LoadError::Invalid {
                path: sample.clone(),
                reason: e.to_string(),
            })?,
        );
    }
    if canceled() {
        return Err(LoadError::Canceled);
    }
    progress(Progress::Lowering);
    let labels = kept
        .iter()
        .map(|&a| locations[a].display().to_string())
        .collect();
    finish(instrument, pcm, labels, options)
}

/// Fit each zone to its decoded audio (`pcm[i]` and `labels[i]` belong to
/// asset `i`), drop zones left with nothing to play, and prepare the plan.
/// Every adjustment is added to `instrument.unsupported`.
pub fn finish(
    mut instrument: ir::Instrument,
    pcm: Vec<Pcm>,
    labels: Vec<String>,
    options: &Options,
) -> Result<Loaded, LoadError> {
    let mut playable = vec![true; instrument.zones.len()];
    for (index, zone) in instrument.zones.iter_mut().enumerate() {
        let mut report = Vec::new();
        let audio = &pcm[zone.asset.0];
        let group_tune = zone
            .group
            .map_or(0.0, |g| instrument.groups[g.0].tune.semitones());
        let ratio = f64::from(audio.sample_rate()) / f64::from(options.rate);
        playable[index] = fit(&mut zone.playback, audio.frame_count() as u64, &mut report)
            && fit_keys(zone, group_tune, ratio, &mut report);
        let location = format!("zone {index} ({})", labels[zone.asset.0]);
        instrument
            .unsupported
            .extend(report.into_iter().map(|(feature, value)| ir::Unsupported {
                location: location.clone(),
                feature: feature.into(),
                value,
                reason: ir::Reason::InvalidValue,
            }));
    }
    let mut index = 0;
    let used = instrument.retain_zones(|_| {
        index += 1;
        playable[index - 1]
    });
    let pcm = pcm
        .into_iter()
        .enumerate()
        .filter(|(i, _)| used.binary_search(i).is_ok())
        .map(|(_, p)| p)
        .collect();
    let resources = options.library.as_deref().map(Resources::of);
    prepare_with(instrument, options.rate, pcm, options.scripts, resources)
}

/// Narrow a tracked zone to the keys the runtime can pitch its audio to
/// (`ratio` is the asset rate over the output rate). `false` when none remain.
fn fit_keys(
    zone: &mut ir::Zone,
    group_tune: f64,
    ratio: f64,
    report: &mut Vec<(&'static str, String)>,
) -> bool {
    let semitones = zone.tune.semitones() + group_tune;
    let limit = |step: f64| 12.0 * (step / ratio).log2() - semitones;
    let (low, high) = (
        limit(*sampler_core::lower::PITCH_STEPS.start()),
        limit(*sampler_core::lower::PITCH_STEPS.end()),
    );
    let (lowest, highest) = match zone.pitch {
        ir::KeyTracking::Tracked { root } => (f64::from(root) + low, f64::from(root) + high),
        ir::KeyTracking::Fixed if (low..=high).contains(&0.0) => return true,
        ir::KeyTracking::Fixed => (f64::INFINITY, f64::NEG_INFINITY),
        ir::KeyTracking::Scaled { .. } => return true, // Lowering decides.
    };
    let keys = (
        f64::from(zone.keys.low).max(lowest.ceil()),
        f64::from(zone.keys.high).min(highest.floor()),
    );
    if keys.0 > keys.1 {
        report.push((
            "no key within the runtime's pitch range, zone dropped",
            format!("{:?}", zone.keys),
        ));
        return false;
    }
    let keys = ir::KeyRange {
        low: keys.0 as u8,
        high: keys.1 as u8,
    };
    if keys != zone.keys {
        report.push((
            "keys beyond the runtime's pitch range, narrowed",
            format!("{:?} to {keys:?}", zone.keys),
        ));
        zone.keys = keys;
    }
    true
}

/// Fit authored sample positions to the decoded audio the way Kontakt
/// tolerates them: clamp the end and loop crossfades, drop a loop outside the
/// played range. `false` when nothing of the zone is left to play.
fn fit(playback: &mut ir::Playback, frames: u64, report: &mut Vec<(&'static str, String)>) -> bool {
    let end = playback.end.unwrap_or(frames);
    if end > frames {
        report.push((
            "sample end beyond the audio, clamped",
            format!("{end} > {frames}"),
        ));
        playback.end = Some(frames);
    }
    let end = end.min(frames);
    if playback.start >= end {
        report.push((
            "empty played range, zone dropped",
            format!("{}..{end}", playback.start),
        ));
        return false;
    }
    if let ir::Looping::Continuous(range) | ir::Looping::UntilRelease(range) = &mut playback.looping
    {
        if range.start < playback.start || range.end > end || range.start >= range.end {
            report.push((
                "loop starting before the zone start or past its end, dropped",
                format!(
                    "{}..{} in {}..{end}",
                    range.start, range.end, playback.start
                ),
            ));
            playback.looping = ir::Looping::None;
            return true;
        }
        let room = if playback.reverse {
            end - range.end
        } else {
            range.start - playback.start
        };
        let limit = (range.end - range.start).min(room);
        if let ir::Span::Frames(crossfade) = range.crossfade
            && crossfade > limit
        {
            report.push((
                "loop crossfade longer than its room, clamped",
                format!("{crossfade} > {limit}"),
            ));
            range.crossfade = ir::Span::Frames(limit);
        }
    }
    true
}

/// Lower a translated instrument whose asset audio is `pcm`, binding its
/// scripts when `scripts` is set. Each script that compiles is bound and
/// hands back its interface; one that fails is reported and left out.
pub fn prepare(
    instrument: ir::Instrument,
    rate: u32,
    pcm: Vec<Pcm>,
    scripts: bool,
) -> Result<Loaded, LoadError> {
    prepare_with(instrument, rate, pcm, scripts, None)
}

/// [`prepare`], reading the interfaces' pictures (frames, frame size) from
/// `resources`; without them image assets keep default metadata.
pub fn prepare_with(
    mut instrument: ir::Instrument,
    rate: u32,
    pcm: Vec<Pcm>,
    scripts: bool,
    resources: Option<Resources>,
) -> Result<Loaded, LoadError> {
    let limits = sampler_ksp::Limits::LIBRARY;
    let mut compiled = Vec::new();
    let mut names = Vec::new();
    let groups: Vec<String> = instrument.groups.iter().map(|g| g.name.clone()).collect();
    for (index, behavior) in instrument.behaviors.iter().enumerate() {
        let environment = sampler_ksp::Environment {
            groups: groups.clone(),
            slot: behavior.slot.unwrap_or(index.min(u8::MAX.into()) as u8),
            persisted: behavior
                .state
                .iter()
                .map(|(name, saved)| {
                    let value = match saved {
                        ir::Saved::Int(n) => Value::Int(*n as i32),
                        ir::Saved::Real(r) => Value::Real(*r),
                        ir::Saved::Text(t) => Value::Text(t.clone()),
                    };
                    (name.clone(), value)
                })
                .collect(),
            ..Default::default()
        };
        let result = match behavior.language {
            _ if !scripts => Err("scripts disabled".to_string()),
            ir::Language::Ksp => {
                sampler_ksp::compile_with(&behavior.source, rate, limits, &[], &environment)
                    .map_err(|e| e.to_string())
            }
            ref other => Err(format!("{other:?} has no frontend")),
        };
        match result {
            Ok(script) => {
                instrument.unsupported.extend(script.warnings().iter().map(|w| ir::Unsupported {
                    location: format!("{} line {}", behavior.name, w.line),
                    feature: match w.builtin {
                        Some(builtin) => format!("script {:?}: {builtin}", w.kind),
                        None => format!("script {:?}", w.kind),
                    },
                    value: w.message.clone(),
                    reason: ir::Reason::NotModeled,
                }));
                names.push(behavior.name.clone());
                compiled.push(script);
            }
            Err(error) => instrument.unsupported.push(ir::Unsupported {
                location: behavior.name.clone(),
                feature: "script".into(),
                value: error,
                reason: ir::Reason::NotModeled,
            }),
        }
    }
    let resources = resources.map(std::cell::RefCell::new);
    let picture = |path: &str| resources.as_ref()?.borrow_mut().picture(path);
    let mut interfaces = Vec::new();
    for (script, name) in compiled.iter().zip(&names) {
        match script.ui(&picture) {
            Ok(ui) => interfaces.push(ui),
            Err(e) => instrument.unsupported.push(ir::Unsupported {
                location: name.clone(),
                feature: "script interface".into(),
                value: format!("{e:?}"),
                reason: ir::Reason::InvalidValue,
            }),
        }
    }
    // ponytail: lowering hands the closure every behavior but binding uses
    // only the compiled ones; failed scripts simply have no module.
    let lowered = sampler_core::lower::lower(&instrument, rate, pcm, |_, plan| {
        if compiled.is_empty() {
            return Ok(plan);
        }
        sampler_ksp::bind_modules(compiled, plan).map_err(|e| LowerError::Behavior {
            module: "KSP".into(),
            message: e.to_string(),
        })
    });
    Ok(Loaded {
        plan: lowered.map_err(LoadError::Lower)?,
        instrument,
        interfaces,
    })
}
