//! One call from an NKI path to a playable plan: translate to the IR, decode
//! the samples it needs, compile its scripts and lower it. What could not be
//! carried over is returned with the plan, never dropped silently.

use crate::{Kontakt, LoadError, Resources, read};
use sampler_core::{Pcm, Prepared, lower::LowerError};
use sampler_ir as ir;
use sampler_ksp::model::Value;
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
    /// Native per-note pressure/timbre routing added to every zone; `None`
    /// leaves expression to authored modulation (pitch bend stays native).
    pub mpe: Option<sampler_core::lower::MpeDefaults>,
    /// Value (0..=127) the dynamics controllers other than CC11 start at when
    /// the host has not sent them; `None` is Kontakt's power-on state (CC11
    /// full, the rest 0, so a CC1 instrument is near-silent until it moves).
    pub dynamics_start: Option<u8>,
    /// Host-saved KSP scalar values by stable identity; menus use item values.
    pub control_values: Vec<(sampler_core::ControlId, i32)>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            rate: 48000,
            keys: 0..=127,
            scripts: true,
            library: None,
            mpe: Some(Default::default()),
            dynamics_start: None,
            control_values: Vec::new(),
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
    /// Every bound script's interface model, indexed by its
    /// [`sampler_core::ScriptInstanceId`]: apply the runtime's UI effects to
    /// it and rebuild the interface with [`Loaded::resources`].
    pub scripts: Vec<sampler_ksp::ScriptView>,
    /// The library's pictures and resources, when [`Options::library`] is known.
    pub resources: Option<Resources>,
    dynamics: Vec<(u8, f64)>,
}

/// How well an instrument's keyswitches reached the articulation map.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArticulationMigration {
    /// Articulations the source defined through switch keys.
    pub switches_found: usize,
    /// Articulations that select zones, so a velocity, channel, CC or program
    /// driver can stand in for the keys.
    pub migrated: usize,
    /// Why the rest did not translate, from the load report.
    pub unrecognised: Vec<String>,
}

/// Count keyswitch articulations that carry zones, and list the report
/// entries that name what was not recognised.
pub fn articulation_migration(ir: &ir::Instrument) -> ArticulationMigration {
    let tagged = |i: usize| {
        ir.zones
            .iter()
            .any(|z| z.articulation.is_some_and(|a| a.0 == i))
    };
    let switched = |a: &&ir::Articulation| !a.switch_keys.is_empty();
    ArticulationMigration {
        switches_found: ir.articulations.iter().filter(switched).count(),
        migrated: ir
            .articulations
            .iter()
            .enumerate()
            .filter(|(i, a)| switched(a) && tagged(*i))
            .count(),
        unrecognised: ir
            .unsupported
            .iter()
            .filter(|u| {
                let f = u.feature.to_lowercase();
                f.contains("keyswitch") || f.contains("start") || f.contains("articulation")
            })
            .map(|u| format!("{}: {} {}", u.location, u.feature, u.value))
            .collect(),
    }
}

impl Loaded {
    /// The instrument volume's controller and its starting linear gain (the
    /// saved volume until the controller arrives; then `(cc/127)^3`).
    pub fn host_volume(&self) -> Option<ir::HostVolume> {
        self.instrument.host_volume
    }

    /// The controllers that drive loudness, most used first, each with its
    /// value before any is received (CC11 full, the rest 0; Kontakt's
    /// power-on state). A host can show "dynamics: CC1 (now 0)" on load.
    pub fn dynamics(&self) -> Vec<(u8, f64)> {
        self.dynamics.clone()
    }

    /// Whether the instrument is near-silent until a host sends a dynamics
    /// controller: one drives loudness and starts at 0. Velocity- or
    /// script-driven loudness is not covered.
    pub fn needs_controller(&self) -> bool {
        self.dynamics.iter().any(|&(_, v)| v == 0.0)
    }
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
        let decoded = samples
            .decode(sample)
            .map_err(|e| e.at(crate::Stage::SampleResolve))?;
        let frames = decoded.frames.into_boxed_slice();
        pcm.push(Pcm::new(decoded.rate, frames).map_err(|e| {
            LoadError::Invalid {
                path: sample.clone(),
                reason: e.to_string(),
            }
            .at(crate::Stage::SampleResolve)
        })?);
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

/// Load the Kontakt instrument at `path` streamed: only the frames where
/// zones start (sized from read latency measured on up to 32 samples) and a
/// page pool are resident.
pub fn load_streamed(
    path: &Path,
    options: &Options,
    policy: &crate::StreamPolicy,
    progress: impl FnMut(Progress),
) -> Result<crate::Streamed, LoadError> {
    let options = Options {
        library: options.library.clone().or_else(|| Some(path.into())),
        ..options.clone()
    };
    load_read_streamed(read(path)?, &options, policy, progress)
}

/// [`load_streamed`] for an instrument already [`read`]; pictures and
/// resources come from [`Options::library`].
pub fn load_read_streamed(
    kontakt: Kontakt,
    options: &Options,
    policy: &crate::StreamPolicy,
    mut progress: impl FnMut(Progress),
) -> Result<crate::Streamed, LoadError> {
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
    let mut sources = Vec::with_capacity(kept.len());
    for &asset in &kept {
        let location = &locations[asset];
        sources.push((
            std::sync::Arc::new(
                samples
                    .source(location)
                    .map_err(|e| e.at(crate::Stage::SampleResolve))?,
            ) as std::sync::Arc<dyn crate::AssetSource>,
            location.as_path(),
        ));
    }
    let opened = crate::stream::Streamer::open(sources, options.rate, policy, 32)?;
    let pcm = opened.assets.clone();
    progress(Progress::Lowering);
    let labels = kept
        .iter()
        .map(|&a| locations[a].display().to_string())
        .collect();
    let (loaded, kept) = finish_kept(instrument, pcm, labels, options)?;
    crate::Streamed::new(loaded, opened, kept)
}

/// Stream an instrument translated by another loader: `sources[i]` and
/// `labels[i]` belong to asset `i`. As [`load_streamed`], without the Kontakt
/// container: only zone starts and a page pool are resident.
pub fn stream_instrument(
    mut instrument: ir::Instrument,
    sources: Vec<std::sync::Arc<dyn crate::AssetSource>>,
    labels: Vec<String>,
    options: &Options,
    policy: &crate::StreamPolicy,
) -> Result<crate::Streamed, LoadError> {
    let (low, high) = (*options.keys.start(), *options.keys.end());
    let kept = instrument.retain_zones(|z| z.keys.low <= high && z.keys.high >= low);
    let listed = kept
        .iter()
        .map(|&asset| (sources[asset].clone(), Path::new(labels[asset].as_str())))
        .collect();
    let opened = crate::stream::Streamer::open(listed, options.rate, policy, 32)?;
    let pcm = opened.assets.clone();
    let labels = kept.iter().map(|&a| labels[a].clone()).collect();
    let (loaded, kept) = finish_kept(instrument, pcm, labels, options)?;
    crate::Streamed::new(loaded, opened, kept)
}

/// Fit each zone to its decoded audio (`pcm[i]` and `labels[i]` belong to
/// asset `i`), drop zones left with nothing to play, and prepare the plan.
/// Every adjustment is added to `instrument.unsupported`.
pub fn finish(
    instrument: ir::Instrument,
    pcm: Vec<Pcm>,
    labels: Vec<String>,
    options: &Options,
) -> Result<Loaded, LoadError> {
    finish_kept(instrument, pcm, labels, options).map(|(loaded, _)| loaded)
}

/// [`finish`], also returning the assets the plan kept, in its order.
fn finish_kept(
    mut instrument: ir::Instrument,
    pcm: Vec<Pcm>,
    labels: Vec<String>,
    options: &Options,
) -> Result<(Loaded, Vec<Pcm>), LoadError> {
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
        .collect::<Vec<_>>();
    let kept = pcm.clone();
    Ok((prepare(instrument, pcm, options)?, kept))
}

/// The instrument volume as a host parameter: zones lose the saved volume and
/// gain a route `(cc/127)^3` from the controller, which starts at
/// `saved^(1/3)` so the unsent state plays the saved value exactly.
fn host_volume(instrument: &mut ir::Instrument) {
    let Some(volume) = instrument.host_volume.filter(|v| v.saved > 0.0) else {
        return;
    };
    if instrument.zones.is_empty() {
        return;
    }
    instrument.modulators.push(ir::Modulator {
        scope: ir::Scope::Voice,
        source: ir::ModulationSource::Controller(volume.controller),
    });
    instrument.shapes.push(ir::Shape {
        points: (0..128)
            .map(|i| {
                let x = f64::from(i) / 127.0;
                (x, x * x * x)
            })
            .collect(),
    });
    let mut route = ir::Route::new(
        ir::ModulatorRef(instrument.modulators.len() - 1),
        ir::Target::Amplitude,
        ir::Depth::Normalized(1.0),
    );
    route.shape = Some(ir::ShapeRef(instrument.shapes.len() - 1));
    instrument.routes.push(route);
    let route = ir::RouteRef(instrument.routes.len() - 1);
    for zone in &mut instrument.zones {
        zone.gain = ir::Gain::Linear(zone.gain.linear() / volume.saved);
        zone.routes.push(route);
    }
}

/// Dynamics controllers with their power-on value: Kontakt's (CC11 full, the
/// rest 0) unless the host asked for `start` on those other than CC11.
fn power_on(instrument: &ir::Instrument, start: Option<u8>) -> Vec<(u8, f64)> {
    let volume = instrument.host_volume.map(|v| v.controller);
    instrument
        .amplitude_controllers()
        .into_iter()
        .filter(|&cc| Some(cc) != volume)
        .map(|cc| {
            let value = match (cc, start) {
                (11, _) => 1.0,
                (_, Some(v)) => f64::from(v.min(127)) / 127.0,
                _ => 0.0,
            };
            (cc, value)
        })
        .collect()
}

fn powered(plan: Prepared, instrument: &ir::Instrument, dynamics: &[(u8, f64)]) -> Prepared {
    let plan = dynamics
        .iter()
        .filter(|&&(cc, v)| cc != 11 && v != 0.0)
        .fold(plan, |plan, &(cc, v)| plan.with_initial_level(cc, v));
    match instrument.host_volume.filter(|v| v.saved > 0.0) {
        Some(v) if !instrument.zones.is_empty() => {
            plan.with_initial_level(v.controller, v.saved.cbrt())
        }
        _ => plan,
    }
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
    if let ir::Looping::Slots(mut slots) = playback.looping {
        for slot in &mut slots {
            if let Some(value) = slot {
                let mut view = *playback;
                view.looping = if value.until_release {
                    ir::Looping::UntilRelease(value.range)
                } else {
                    ir::Looping::Continuous(value.range)
                };
                fit(&mut view, frames, report);
                match view.looping {
                    ir::Looping::Continuous(range) | ir::Looping::UntilRelease(range) => {
                        value.range = range
                    }
                    _ => *slot = None,
                }
            }
        }
        playback.looping = ir::Looping::Slots(slots);
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
/// scripts when `options.scripts` is set. Each script that compiles is bound
/// and hands back its interface, its pictures read from `options.library`;
/// one that fails is reported and left out.
pub fn prepare(
    instrument: ir::Instrument,
    pcm: Vec<Pcm>,
    options: &Options,
) -> Result<Loaded, LoadError> {
    prepare_inner(instrument, pcm, options).map_err(|e| e.at(crate::Stage::Prepare))
}

/// What a script may query while its `on init` runs.
pub(crate) fn script_environment(
    behavior: &ir::Behavior,
    index: usize,
    groups: Vec<String>,
    performance_view: sampler_ksp::model::PerformanceView,
) -> sampler_ksp::Environment {
    sampler_ksp::Environment {
        evaluation_budget: None,
        groups,
        engine_values: Default::default(),
        engine_lookups: Vec::new(),
        slot: behavior.slot.unwrap_or(index.min(u8::MAX.into()) as u8),
        control_values: Default::default(),
        persisted: behavior
            .state
            .iter()
            .filter_map(|(name, saved)| {
                let value = match saved {
                    ir::Saved::Int(n) => Value::Int(*n as i32),
                    ir::Saved::Real(r) => Value::Real(*r),
                    ir::Saved::Text(t) => Value::Text(t.clone()),
                    _ => return None,
                };
                Some((name.clone(), value))
            })
            .collect(),
        persisted_arrays: behavior
            .state
            .iter()
            .filter_map(|(name, saved)| {
                let values = match saved {
                    ir::Saved::Ints(v) => v.iter().map(|n| Value::Int(*n as i32)).collect(),
                    ir::Saved::Reals(v) => v.iter().map(|r| Value::Real(*r)).collect(),
                    ir::Saved::Texts(v) => v.iter().cloned().map(Value::Text).collect(),
                    _ => return None,
                };
                Some((name.clone(), values))
            })
            .collect(),
        performance_view,
    }
}

/// Compile the performance frontends without loading samples or constructing a
/// voice plan. The playable loader and the UI survey use this same path.
pub fn compile_ui(
    instrument: &mut ir::Instrument,
    options: &Options,
) -> (
    Vec<sampler_ksp::Script>,
    Vec<sampler_ui_ir::Interface>,
    Option<Resources>,
) {
    let (rate, scripts) = (options.rate, options.scripts);
    let resources = options.library.as_deref().map(Resources::of);
    let limits = sampler_ksp::Limits::LIBRARY;
    let mut compiled = Vec::new();
    let mut names = Vec::new();
    let resources = resources.map(std::cell::RefCell::new);
    let groups: Vec<String> = instrument.groups.iter().map(|g| g.name.clone()).collect();
    let mut views = Vec::new();
    for (index, behavior) in instrument.behaviors.iter().enumerate() {
        let mut performance_view = Default::default();
        if let Some(name) = sampler_ksp::nckp::view_name(&behavior.source) {
            let path = format!("Resources/performance_view/{name}.nckp");
            let parsed = match resources.as_ref().and_then(|r| r.borrow_mut().read(&path)) {
                Some(bytes) => sampler_ksp::nckp::parse(&bytes),
                None => Err("not found in the library".into()),
            };
            match parsed {
                Ok((view, skipped)) => {
                    performance_view = view;
                    views.extend(skipped.into_iter().map(|value| ir::Unsupported {
                        location: path.clone(),
                        feature: "performance view control".into(),
                        value,
                        reason: ir::Reason::NotModeled,
                    }));
                }
                Err(value) => views.push(ir::Unsupported {
                    location: behavior.name.clone(),
                    feature: "performance view".into(),
                    value: format!("{path}: {value}"),
                    reason: ir::Reason::InvalidValue,
                }),
            }
        }
        let mut environment = script_environment(behavior, index, groups.clone(), performance_view);
        environment.control_values.extend(
            options
                .control_values
                .iter()
                .map(|&(id, value)| (id, Value::Int(value))),
        );
        let result = match behavior.language {
            _ if !scripts => Err("scripts disabled".to_string()),
            ir::Language::Ksp => {
                #[cfg(feature="scan")]
                sampler_ksp::scan::attempt("runtime-preparation");
                sampler_ksp::compile_with(&behavior.source, rate, limits, &[], &environment)
                    .map_err(|e| e.to_string())
            }
            ref other => Err(format!("{other:?} has no frontend")),
        };
        match result {
            Ok(script) => {
                if script.model().requests.iter().any(|r| r.command == "load_native_ui") {
                    instrument.unsupported.push(ir::Unsupported {
                        location: behavior.name.clone(), feature: "native interface".into(),
                        value: "The requested native performance view is unavailable; showing the script controls.".into(),
                        reason: ir::Reason::NotModeled,
                    });
                }
                instrument
                    .unsupported
                    .extend(script.warnings().iter().map(|w| ir::Unsupported {
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
    instrument.unsupported.append(&mut views);
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
    super::keyswitch_ui::normalize(instrument, &interfaces, &compiled);
    (
        compiled,
        interfaces,
        resources.map(std::cell::RefCell::into_inner),
    )
}

fn prepare_inner(
    mut instrument: ir::Instrument,
    pcm: Vec<Pcm>,
    options: &Options,
) -> Result<Loaded, LoadError> {
    let rate = options.rate;
    host_volume(&mut instrument);
    let lower_options = sampler_core::lower::Options { mpe: options.mpe };
    let (compiled, interfaces, resources) = compile_ui(&mut instrument, options);
    // ponytail: lowering hands the closure every behavior but binding uses
    // only the compiled ones; failed scripts simply have no module.
    if compiled.is_empty() && !instrument.behaviors.is_empty() {
        // No script runs: lower without them, and without the switching
        // they own.
        let behaviors = std::mem::take(&mut instrument.behaviors);
        let owned = instrument.switching.owner == ir::SwitchOwner::Behavior;
        let (articulations, switching) = if owned {
            (
                std::mem::take(&mut instrument.articulations),
                std::mem::take(&mut instrument.switching),
            )
        } else {
            Default::default()
        };
        let lowered =
            sampler_core::lower::lower_with(&instrument, rate, pcm, &lower_options, |_, plan| {
                Ok(plan)
            });
        instrument.behaviors = behaviors;
        if owned {
            instrument.articulations = articulations;
            instrument.switching = switching;
        }
        let dynamics = power_on(&instrument, options.dynamics_start);
        return Ok(Loaded {
            plan: powered(lowered.map_err(LoadError::Lower)?, &instrument, &dynamics),
            dynamics,
            instrument,
            interfaces,
            scripts: Vec::new(),
            resources,
        });
    }
    let mut automation = Vec::new();
    for binding in &instrument.script_automation {
        let target = compiled.iter().find(|script| script.view().slot() == binding.source_slot)
            .and_then(|script| script.model().interface.widgets.iter()
                .filter(|w| w.kind == sampler_ksp::model::WidgetKind::Slider)
                .nth(binding.slider as usize));
        if let Some(widget) = target {
            automation.push(sampler_core::AutomationBinding {
                source: match binding.source {
                    ir::ScriptAutomationSource::Controller(cc) => sampler_core::AutomationSource::Controller(cc),
                    ir::ScriptAutomationSource::HostParameter(address) => sampler_core::AutomationSource::HostParameter(address),
                },
                source_slot: binding.source_slot, ui_id: widget.ui_id,
                low: binding.low, high: binding.high, soft_takeover: binding.soft_takeover,
            });
        } else {
            instrument.unsupported.push(ir::Unsupported { location: format!("script slot {}",binding.source_slot),
                feature: "saved automation slider".into(), value: binding.slider.to_string(), reason: ir::Reason::InvalidValue });
        }
    }
    let scripts = compiled.iter().map(sampler_ksp::Script::view).collect();
    let lowered =
        sampler_core::lower::lower_with(&instrument, rate, pcm, &lower_options, |_, plan| {
            sampler_ksp::bind_modules(compiled, plan).and_then(|plan| plan.with_automation_bindings(automation)).map_err(|e| LowerError::Behavior {
                module: "KSP".into(),
                message: e.to_string(),
            })
        });
    let dynamics = power_on(&instrument, options.dynamics_start);
    Ok(Loaded {
        plan: powered(lowered.map_err(LoadError::Lower)?, &instrument, &dynamics),
        dynamics,
        instrument,
        interfaces,
        scripts,
        resources,
    })
}

#[cfg(test)]
mod migration_tests {
    use super::*;

    #[test]
    fn a_switch_without_zones_is_found_but_not_migrated() {
        let mut ir = ir::Instrument::default();
        let a = |name: &str, keys: Vec<u8>| ir::Articulation {
            name: name.into(),
            switch_keys: keys,
            ..Default::default()
        };
        ir.articulations = vec![a("sus", vec![12]), a("stac", vec![13]), a("none", vec![])];
        let mut zone = ir::Zone::new(ir::AssetRef(0));
        zone.articulation = Some(ir::ArticulationRef(0));
        ir.zones.push(zone);
        let m = articulation_migration(&ir);
        assert_eq!((m.switches_found, m.migrated), (2, 1));
    }
}

#[cfg(test)]
mod volume_tests {
    use super::*;
    use sampler_core::{Input, Limits, Protocol, Runtime};

    /// Peak in dB of a full-scale constant sample, instrument volume saved as
    /// `saved`, CC7 `cc` (`None`: never sent).
    fn peak_db(saved: f64, cc: Option<u8>) -> f64 {
        let mut ir = ir::Instrument::default();
        ir.assets.push(ir::Asset {
            location: ir::AssetLocation::Path("x".into()),
            encoding: ir::Encoding::Wav,
            root_key: None,
            loops: Vec::new(),
        });
        let mut zone = ir::Zone::new(ir::AssetRef(0));
        zone.keys = ir::KeyRange { low: 60, high: 60 };
        zone.pitch = ir::KeyTracking::Fixed;
        zone.velocity = ir::VelocityResponse::None;
        zone.gain = ir::Gain::Linear(saved);
        ir.zones.push(zone);
        ir.host_volume = Some(ir::HostVolume {
            controller: 7,
            saved,
        });
        let pcm = Pcm::new(48000, vec![[1.0f32; 2]; 48000].into_boxed_slice()).unwrap();
        let loaded = finish(ir, vec![pcm], vec!["x".into()], &Options::default()).unwrap();
        assert_eq!(loaded.host_volume().map(|v| v.controller), Some(7));
        let limits = Limits {
            notes: 16,
            channels: 16,
            performances: 1,
            expressions: 16,
            families: 16,
            decisions: 16,
            voices: 16,
            commands: 16,
            behaviors: 1,
            behavior_fuel: 1 << 10,
            behavior_cells: 0,
            note_cells: 0,
        };
        let mut rt = Runtime::new(loaded.plan, limits).unwrap();
        if let Some(cc) = cc {
            let id = rt.performance(0).unwrap();
            let value = (u64::from(cc) * u64::from(u32::MAX) / 127) as u32;
            rt.set_controller(id, 7, value).unwrap();
        }
        let input = Input {
            protocol: Protocol::Clap,
            port: 0,
            group: 0,
            channel: 0,
            key: 60,
            external_id: Some(1),
        };
        rt.trigger(input, 60, 1.0).unwrap();
        let mut out = vec![[0.0f32; 2]; 4800];
        rt.render(&mut out).unwrap();
        let peak = out[2400..]
            .iter()
            .flatten()
            .fold(0f32, |p, x| p.max(x.abs()));
        20.0 * f64::from(peak).max(1e-9).log10()
    }

    #[test]
    fn cc7_replaces_the_saved_volume_with_its_cube() {
        let near = |got: f64, want: f64| assert!((got - want).abs() < 0.15, "{got} against {want}");
        // Saved -6 dB (the noise instrument): unsent -6.02, CC7 replaces it.
        near(peak_db(0.5, None), -6.021);
        near(peak_db(0.5, Some(127)), 0.0);
        near(peak_db(0.5, Some(100)), -6.23);
        near(peak_db(0.5, Some(64)), -17.95);
        assert!(peak_db(0.5, Some(0)) < -120.0);
        // Saved 0 dB (Una): unsent and 127 agree.
        near(peak_db(1.0, None), 0.0);
        near(peak_db(1.0, Some(127)), 0.0);
        near(peak_db(1.0, Some(64)), -17.95);
    }
}

#[cfg(test)]
mod dynamics_tests {
    use super::*;

    fn cc1_instrument() -> ir::Instrument {
        let mut ir = ir::Instrument::default();
        ir.modulators.push(ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Controller(1),
        });
        ir.routes.push(ir::Route::new(
            ir::ModulatorRef(0),
            ir::Target::Amplitude,
            ir::Depth::Normalized(1.0),
        ));
        let mut zone = ir::Zone::new(ir::AssetRef(0));
        zone.routes.push(ir::RouteRef(0));
        ir.zones.push(zone);
        ir
    }

    #[test]
    fn the_dynamics_controller_starts_at_zero_unless_the_host_says_otherwise() {
        let ir = cc1_instrument();
        assert_eq!(power_on(&ir, None), vec![(1, 0.0)]);
        assert_eq!(power_on(&ir, Some(127)), vec![(1, 1.0)]);
        assert!(power_on(&ir, Some(64))[0].1 > 0.5);
    }
}

#[cfg(test)]
mod automation_tests {
    use super::*;
    #[test]
    fn saved_target_counts_only_sliders_in_its_physical_slot() {
        let mut instrument = ir::Instrument::default();
        instrument.behaviors.push(ir::Behavior {
            name: "learn".into(), language: ir::Language::Ksp, slot: Some(3), state: vec![], requires: vec![],
            source: "on init declare ui_label $label(1,1) declare ui_slider $unused(0,127) declare ui_knob $knob(0,100,1) declare ui_slider $target(20,100) declare $observed end on on ui_control($target) $observed := $target end on".into(),
        });
        instrument.script_automation.push(ir::ScriptAutomation {
            source: ir::ScriptAutomationSource::Controller(21), source_slot: 3, slider: 1,
            low: 0., high: 1., soft_takeover: false,
        });
        let loaded = prepare(instrument, vec![], &Options::default()).unwrap();
        let limits = sampler_core::Limits::for_plan(&loaded.plan, 4, 8);
        let mut rt = sampler_core::Runtime::new(loaded.plan, limits).unwrap();
        rt.dispatch_controller(rt.performance(0).unwrap(), sampler_core::ChannelAddress {
            protocol: sampler_core::Protocol::Native, port: 0, group: 0, channel: 0,
        }, 1, 21, (u64::from(u32::MAX)*64/127) as u32).unwrap();
        let id = rt.widget_id(rt.active_plan(), 3, 32771).unwrap();
        assert_eq!(rt.widget_value(rt.active_plan(),id,0),Ok(sampler_core::WidgetValue::Integer(60)));
        assert_eq!(rt.script_cell(rt.active_plan(),sampler_core::ScriptInstanceId(0),1),Ok(60));
    }
}
