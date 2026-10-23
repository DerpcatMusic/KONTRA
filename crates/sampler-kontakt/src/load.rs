//! One call from an NKI path to a playable plan: translate to the IR, decode
//! the samples it needs, compile its scripts and lower it. What could not be
//! carried over is returned with the plan, never dropped silently.

use crate::{Kontakt, LoadError, Resources};
use sampler_core::{Pcm, Prepared, SourceStorage, lower::LowerError};
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
                // "group start" only: a bare "start" also matches unrelated
                // KSP warnings such as "polyphonic variables start at 0".
                f.contains("keyswitch") || f.contains("group start") || f.contains("articulation")
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
/// it is polled before reading the container, before each sample is decoded, and before lowering.
pub fn load_cancelable(
    path: &Path,
    options: &Options,
    progress: impl FnMut(Progress),
    canceled: impl Fn() -> bool,
) -> Result<Loaded, LoadError> {
    if canceled() {
        return Err(LoadError::Canceled);
    }
    let options = Options {
        library: options.library.clone().or_else(|| Some(path.into())),
        ..options.clone()
    };
    load_read(
        crate::read_with_controls(path, &options.control_values)?,
        &options,
        progress,
        canceled,
    )
}

/// [`load_cancelable`] for an instrument already [`crate::read`], so a caller can
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
        initialized,
    } = kontakt;
    let (low, high) = (*options.keys.start(), *options.keys.end());
    let kept = retain_key_window(&mut instrument,low,high);
    progress(Progress::Translated {
        zones: instrument.zones.len(),
        assets: kept.len(),
    });
    let mut sources = Vec::with_capacity(kept.len());
    for (done, &asset) in kept.iter().enumerate() {
        if canceled() {return Err(LoadError::Canceled);}
        let sample=&locations[asset];
        progress(Progress::Decoding{done,total:kept.len(),sample});
        let storage=samples.decode_source(sample).map_err(|e|e.at(crate::Stage::SampleResolve))?;
        if let SourceStorage::Stereo(pcm)=&storage {
            // The old stereo residency recipe and actual AssetId are unchanged.
            let source=samples.source(sample).map_err(|e|e.at(crate::Stage::SampleResolve))?;
            let length=pcm.frame_count();
            pcm.set_residency_loader(move || {
                let mut reader=crate::SampleReader::open(&source).map_err(|_|sampler_core::Error::NotReady)?;
                let mut frames=vec![[0.;2];length];reader.read(0,&mut frames).map_err(|_|sampler_core::Error::NotReady)?;
                Ok(vec![(0,frames.into_boxed_slice())])
            }).map_err(|e|LoadError::Invalid{path:sample.clone(),reason:e.to_string()})?;
        }
        // A resident family retains the complete immutable cohort. Existing
        // normal group/target admission still decides whether it can publish.
        sources.push(storage);
    }
    if canceled() {
        return Err(LoadError::Canceled);
    }
    progress(Progress::Lowering);
    let labels = kept
        .iter()
        .map(|&a| locations[a].display().to_string())
        .collect();
    finish_sources_kept(instrument, sources, labels, options, initialized).map(|(loaded, _)| loaded)
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
    load_read_streamed(
        crate::read_with_controls(path, &options.control_values)?,
        &options,
        policy,
        progress,
    )
}

/// [`load_streamed`] for an instrument already [`crate::read`]; pictures and
/// resources come from [`Options::library`].
pub fn load_read_streamed(
    kontakt: Kontakt,
    options: &Options,
    policy: &crate::StreamPolicy,
    progress: impl FnMut(Progress),
) -> Result<crate::Streamed, LoadError> {
    load_read_streamed_cancelable(kontakt, options, policy, progress, &|| false)
}

/// Streamed load with cancellation polled during source/header startup.
pub fn load_read_streamed_cancelable(
    kontakt: Kontakt,
    options: &Options,
    policy: &crate::StreamPolicy,
    mut progress: impl FnMut(Progress),
    canceled: &(dyn Fn() -> bool + Sync),
) -> Result<crate::Streamed, LoadError> {
    let Kontakt {
        mut instrument,
        locations,
        mut samples,
        initialized,
    } = kontakt;
    let (low, high) = (*options.keys.start(), *options.keys.end());
    let kept = retain_key_window(&mut instrument,low,high);
    progress(Progress::Translated {
        zones: instrument.zones.len(),
        assets: kept.len(),
    });
    let span = crate::audit::Span::new("sample_source_resolve");
    let listed: Vec<_> = kept
        .iter()
        .map(|&asset| locations[asset].as_path())
        .collect();
    let cached = options
        .library
        .as_deref()
        .and_then(|preset| crate::header_cache::load(preset, &listed, &mut samples));
    let hit = cached.is_some();
    let resolved = match cached {
        Some(sources) => sources,
        None => samples
            .sources(&listed, canceled)
            .map_err(|e| e.at(crate::Stage::SampleResolve))?,
    };
    let sources: Vec<_> = resolved.into_iter().map(std::sync::Arc::new).collect();
    let registry = sources
        .iter()
        .cloned()
        .zip(listed.iter().copied())
        .map(|(source, path)| (source as std::sync::Arc<dyn crate::AssetSource>, path))
        .collect();
    drop(samples);
    if std::env::var_os("KONTRA_AUDIT_LOAD").is_some() {
        eprintln!(
            "AUDIT {{\"stage\":\"sample_header_cache\",\"hit\":{}}}",
            hit
        );
    }
    drop(span);
    let requirements=instrument.assets.iter().map(|a|a.residency).collect::<Vec<_>>();
    let opened = crate::stream::Streamer::open_required(
        registry,&requirements,
        options.rate,
        policy,
        32,
        canceled,
        &mut |done, total| {
            if let Some(&sample) = listed.get(done.min(total.saturating_sub(1))) {
                progress(Progress::Decoding {
                    done,
                    total,
                    sample,
                })
            }
        },
    )?;
    if !hit && !canceled() && opened.logical_sources.is_none() {
        if let Some(preset) = options.library.as_deref() {
            crate::header_cache::store(preset, &listed, &sources, &opened.assets);
        }
    }
    drop(sources);
    if canceled() {
        return Err(LoadError::Canceled);
    }
    let logical=opened.logical_sources.clone().unwrap_or_else(||opened.assets.iter().cloned().map(SourceStorage::Stereo).collect());
    progress(Progress::Lowering);
    let labels = kept
        .iter()
        .map(|&a| locations[a].display().to_string())
        .collect();
    let (loaded, kept) = finish_sources_kept(instrument, logical, labels, options, initialized)?;
    crate::Streamed::new_sources(loaded, opened, kept, canceled)
}

/// Stream an instrument translated by another loader: `sources[i]` and
/// `labels[i]` belong to asset `i`. As [`load_streamed`], without the Kontakt
/// container: only zone starts and a page pool are resident.
pub fn stream_instrument(
    instrument: ir::Instrument,
    sources: Vec<std::sync::Arc<dyn crate::AssetSource>>,
    labels: Vec<String>,
    options: &Options,
    policy: &crate::StreamPolicy,
) -> Result<crate::Streamed, LoadError> {
    stream_instrument_cancelable(instrument,sources,labels,options,policy,&||false)
}
pub fn stream_instrument_cancelable(
    mut instrument:ir::Instrument,sources:Vec<std::sync::Arc<dyn crate::AssetSource>>,labels:Vec<String>,
    options:&Options,policy:&crate::StreamPolicy,canceled:&(dyn Fn()->bool+Sync),
)->Result<crate::Streamed,LoadError> {
    if canceled(){return Err(LoadError::Canceled);}
    let (low, high) = (*options.keys.start(), *options.keys.end());
    let kept = retain_key_window(&mut instrument,low,high);
    let listed = kept
        .iter()
        .map(|&asset| (sources[asset].clone(), Path::new(labels[asset].as_str())))
        .collect();
    let requirements=instrument.assets.iter().map(|a|a.residency).collect::<Vec<_>>();
    let opened=crate::stream::Streamer::open_required(listed,&requirements,options.rate,policy,32,canceled,&mut|_,_|())?;
    let logical=opened.logical_sources.clone().unwrap_or_else(||opened.assets.iter().cloned().map(SourceStorage::Stereo).collect());
    let labels = kept.iter().map(|&a| labels[a].clone()).collect();
    let (loaded, kept) = finish_sources_kept(instrument, logical, labels, options, None)?;
    if canceled(){return Err(LoadError::Canceled);}
    crate::Streamed::new_sources(loaded, opened, kept, canceled)
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
    finish_kept(instrument, pcm, labels, options, None).map(|(loaded, _)| loaded)
}

/// [`finish`], also returning the assets the plan kept, in its order.
fn finish_kept(
    instrument: ir::Instrument, pcm: Vec<Pcm>, labels: Vec<String>, options: &Options,
    initialized: Option<ScriptInit>,
) -> Result<(Loaded, Vec<Pcm>), LoadError> {
    let (loaded,kept)=finish_sources_kept(instrument,pcm.into_iter().map(SourceStorage::Stereo).collect(),labels,options,initialized)?;
    let kept=kept.into_iter().map(|source|match source {SourceStorage::Stereo(pcm)=>Ok(pcm),SourceStorage::Channels(_)=>Err(LoadError::Invalid{path:options.library.clone().unwrap_or_default(),reason:"stereo finish retained a channel family".into()})}).collect::<Result<Vec<_>,_>>()?;
    Ok((loaded,kept))
}

/// Finish ordinary resident logical sources. Entry i remains exactly asset i;
/// physical packs never become aliases, extra zones, or implicit projections.
/// Scripts, key/playback fitting and controls use the same pipeline as finish.
pub fn finish_sources(instrument:ir::Instrument,sources:Vec<SourceStorage>,labels:Vec<String>,options:&Options)
    ->Result<Loaded,LoadError> {
    finish_sources_kept(instrument,sources,labels,options,None).map(|(loaded,_)|loaded)
}
fn finish_sources_kept(
    mut instrument:ir::Instrument,sources:Vec<SourceStorage>,labels:Vec<String>,options:&Options,
    initialized:Option<ScriptInit>,
)->Result<(Loaded,Vec<SourceStorage>),LoadError> {
    if sources.len()!=instrument.assets.len() || labels.len()!=sources.len() {
        return Err(LoadError::Invalid{path:options.library.clone().unwrap_or_default(),reason:"logical source/asset/label count mismatch".into()});
    }
    if instrument.zones.iter().any(|zone|zone.asset.is_some_and(|asset|asset.0>=sources.len())
        || zone.group.is_some_and(|group|group.0>=instrument.groups.len())) {
        return Err(LoadError::Invalid{path:options.library.clone().unwrap_or_default(),reason:"logical zone refers to an absent source or group".into()});
    }
    if !instrument.source_indices.processing.is_empty() {
        instrument.validate().map_err(|error| LoadError::Invalid {
            path: options.library.clone().unwrap_or_default(), reason: error.to_string(),
        })?;
    }
    let mut playable = vec![true; instrument.zones.len()];
    for (index, zone) in instrument.zones.iter_mut().enumerate() {
        let mut report = Vec::new();
        let Some(asset) = zone.asset else { continue; };
        let audio = sources[asset.0].as_source();
        let group_tune = zone
            .group
            .map_or(0.0, |g| instrument.groups[g.0].tune.semitones());
        let ratio = f64::from(audio.sample_rate()) / f64::from(options.rate);
        let wavetable = zone
            .group
            .is_some_and(|g| instrument.groups[g.0].wavetable.is_some());
        playable[index] = fit(&mut zone.playback, audio.frame_count() as u64, &mut report)
            && (wavetable || fit_keys(zone, group_tune, ratio, &mut report));
        if playable[index]
            && wavetable
            && !wavetable_cycles_admitted(
                zone.playback.start,
                zone.playback.end.unwrap_or(audio.frame_count() as u64),
            )
        {
            report.push((
                "wavetable requires one 2048-frame cycle, zone dropped",
                "invalid cycle geometry".into(),
            ));
            playable[index] = false;
        }
        let location = format!("zone {index} ({})", labels[asset.0]);
        instrument
            .unsupported
            .extend(report.into_iter().map(|(feature, value)| ir::Unsupported {
                location: location.clone(),
                feature: feature.into(),
                value,
                reason: ir::Reason::InvalidValue,
            }));
    }
    // A source-native shared section is one authored operation. Fitting away a
    // failed member would alter the sum before its nonlinear/shared state and
    // erase the very identity needed to diagnose it. Dormant players (no asset)
    // remain valid; existing unowned legacy zone-drop policy is unchanged.
    if instrument.source_indices.processing.iter().any(|recipe| recipe.members.iter()
        .any(|member| !playable[member.0])) {
        return Err(LoadError::Invalid {
            path: options.library.clone().unwrap_or_default(),
            reason: "shared source member could not preserve its authored playback geometry".into(),
        });
    }
    let mut index = 0;
    let used = instrument.retain_zones(|_| {
        index += 1;
        playable[index - 1]
    });
    let sources = sources
        .into_iter()
        .enumerate()
        .filter(|(i, _)| used.binary_search(i).is_ok())
        .map(|(_, p)| p)
        .collect::<Vec<_>>();
    let kept = sources.clone();
    Ok((
        prepare_sources_inner(instrument, sources, options, initialized)
            .map_err(|e| e.at(crate::Stage::Prepare))?,
        kept,
    ))
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

/// Kontakt's power-on controller values: CC7 volume 100, CC10 pan centre 64,
/// CC11 expression full, CC1 0. Every other controller starts at 0, and pitch
/// bend starts centred (`POWER_ON_BEND`). The probe sends these before every
/// note; the instrument's own saved or scripted values then take over.
pub const POWER_ON: [(u8, u8); 4] = [(1, 0), (7, 100), (10, 64), (11, 127)];
/// Pitch bend centre (14-bit 8192): MIDI 1.0 UMP, channel 0, LSB 0, MSB 0x40.
pub const POWER_ON_BEND: u32 = 0x20E0_0040;

/// `POWER_ON` minus a volume controller the instrument saved its own value on
/// (that saved volume is the instrument's power-on state, not 100).
pub fn power_on_ccs(instrument: &ir::Instrument) -> Vec<(u8, u8)> {
    let saved = instrument
        .host_volume
        .filter(|v| v.saved > 0.0)
        .map(|v| v.controller);
    POWER_ON
        .iter()
        .copied()
        .filter(|&(cc, _)| Some(cc) != saved)
        .collect()
}

/// [`power_on_ccs`] minus the controllers the prepared plan's script init
/// already set: init ran first, so its value is the instrument's power-on state.
pub fn power_on_for(instrument: &ir::Instrument, plan: &Prepared) -> Vec<(u8, u8)> {
    let init: Vec<u8> = plan.initialized_controllers().collect();
    power_on_ccs(instrument)
        .into_iter()
        .filter(|(cc, _)| !init.contains(cc))
        .collect()
}

fn powered(plan: Prepared, instrument: &ir::Instrument, dynamics: &[(u8, f64)]) -> Prepared {
    // Script init wrote these during compile; its values beat the table's.
    let init: Vec<u8> = plan.initialized_controllers().collect();
    let plan = dynamics
        .iter()
        .filter(|&&(cc, v)| cc != 11 && v != 0.0 && !init.contains(&cc))
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
fn retain_key_window(instrument:&mut ir::Instrument,low:u8,high:u8)->Vec<usize> {
    let routed:std::collections::BTreeSet<_>=instrument.zone_routing.iter()
        .flat_map(|route|route.variants.iter().filter_map(|v|v.zone.map(|z|z.0))).collect();
    let mut index=0usize;
    instrument.retain_zones(|zone| {
        let keep=routed.contains(&index)||(zone.keys.low<=high&&zone.keys.high>=low);
        index+=1;
        keep
    })
}

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

/// Engine wavetables read whole 2048-frame cycles from any start and ignore a
/// trailing partial cycle (`wavetable::Table::new`), so one full cycle is required.
fn wavetable_cycles_admitted(start: u64, end: u64) -> bool {
    end.checked_sub(start).is_some_and(|frames| frames >= 2048)
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
        // A forward zone may start inside or past its loop (core enters it mid-cycle or plays
        // straight through, as v1 and Kontakt do); only a reversed one needs the loop inside.
        let before = playback.reverse && range.start < playback.start;
        if before || range.end > end || range.start >= range.end {
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
        // The crossfade blends toward audio before the loop start in the file (forward) or
        // after its end (reverse); the sample start does not limit it.
        let room = if playback.reverse {
            end - range.end
        } else {
            range.start
        };
        let limit = (range.end - range.start).min(room);
        if let ir::Span::Frames(crossfade) = range.crossfade
            && crossfade > limit
        {
            // Audio that is not in the file cannot be blended, in Kontakt either (v1 `play_map`
            // clamped the same way): a loop at the file start has no crossfade. Only a crossfade
            // longer than the loop itself is an authoring fault worth reporting.
            if crossfade > range.end - range.start {
                report.push((
                    "loop crossfade longer than its loop, clamped",
                    format!("{crossfade} > {}", range.end - range.start),
                ));
            }
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
    prepare_inner(instrument, pcm, options, None).map_err(|e| e.at(crate::Stage::Prepare))
}

/// A behavior's saved state as the KSP evaluator's scalar and array values.
fn saved_values(state: &[(String, ir::Saved)]) -> sampler_ksp::SavedValues {
    let (mut scalars, mut arrays) = sampler_ksp::SavedValues::default();
    for (name, saved) in state {
        let name = name.clone();
        match saved {
            ir::Saved::Int(n) => _ = scalars.insert(name, Value::Int(*n as i32)),
            ir::Saved::Real(r) => _ = scalars.insert(name, Value::Real(*r)),
            ir::Saved::Text(t) => _ = scalars.insert(name, Value::Text(t.clone())),
            ir::Saved::Ints(v) => _ = arrays.insert(name, v.iter().map(|n| Value::Int(*n as i32)).collect()),
            ir::Saved::Reals(v) => _ = arrays.insert(name, v.iter().map(|r| Value::Real(*r)).collect()),
            ir::Saved::Texts(v) => _ = arrays.insert(name, v.iter().cloned().map(Value::Text).collect()),
        }
    }
    (scalars, arrays)
}

/// The data-folder `.nka` files a script loads with a literal
/// `load_array(<array>, 2)`, by lowercase array name without its sigil, and the
/// reason for each file the resources could not provide when the host knows one.
fn data_arrays(
    source: &str,
    resources: &mut Resources,
) -> (
    std::collections::BTreeMap<String, Vec<u8>>,
    std::collections::BTreeMap<String, String>,
) {
    let mut files = std::collections::BTreeMap::new();
    let mut reasons = std::collections::BTreeMap::new();
    let stems = source
        .split("load_array(")
        .skip(1)
        .filter_map(|call| {
            let (array, mode) = call.split_once(')')?.0.split_once(',')?;
            let stem = array.trim().get(1..)?.to_lowercase();
            (mode.trim() == "2" && !stem.is_empty()).then_some(stem)
        });
    for stem in stems {
        match resources.read_result(&format!("Resources/data/{stem}.nka")) {
            Ok(Some(bytes)) => {
                files.insert(stem, bytes);
            }
            Ok(None) => {}
            Err(error) => {
                reasons.insert(stem, error.to_string());
            }
        }
    }
    (files, reasons)
}

/// What a script may query while its `on init` runs.
pub(crate) fn script_environment(
    behavior: &ir::Behavior,
    index: usize,
    groups: Vec<String>,
    source: &ir::SourceIndices,
    zones: &[ir::Zone],
    performance_view: sampler_ksp::model::PerformanceView,
) -> sampler_ksp::Environment {
    script_environment_with_zones(behavior, index, groups, source, zones, performance_view, None)
}

fn script_environment_with_zones(
    behavior: &ir::Behavior,
    index: usize,
    groups: Vec<String>,
    source: &ir::SourceIndices,
    zones: &[ir::Zone],
    performance_view: sampler_ksp::model::PerformanceView,
    metadata: Option<&std::collections::BTreeMap<u32, [i32; 3]>>,
) -> sampler_ksp::Environment {
    let (persisted, persisted_arrays) = saved_values(&behavior.state);
    sampler_ksp::Environment {
        evaluation_budget: None,
        coverage: None,
        transport: Some(sampler_ksp::Transport::STANDALONE),
        groups,
        engine_values: source
            .engine_values
            .iter()
            .map(|value| {
                (
                    [
                        i32::from(value.parameter),
                        value.group,
                        value.slot,
                        value.generic,
                    ],
                    value.value,
                )
            })
            .collect(),
        engine_lookups: sampler_core::lower::source_engine_lookups(source),
        snapshot_mode: Default::default(),
        zone_catalog: source.zone_catalog.clone(),
        zones: metadata.cloned().unwrap_or_else(|| first_homes(source.zone_catalog.iter().map(|z| (z, true)))),
        slot: behavior.slot.unwrap_or(index.min(u8::MAX.into()) as u8),
        control_values: Default::default(),
        persisted,
        persisted_arrays,
        instrument_persisted: None,
        performance_view,
        data_arrays: Default::default(),
        data_array_reasons: Default::default(),
        midi_object: Default::default(),
        folders: Default::default(),
    }
}

#[cfg(test)]
thread_local! { static SCRIPT_INIT_RUNS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(all(feature = "scan", not(test)))]
static SCRIPT_INIT_RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Actual initializer count across loader workers in one isolated scanner process.
#[cfg(any(test, feature = "scan"))]
pub fn take_script_init_runs() -> usize {
    #[cfg(test)]
    {
        SCRIPT_INIT_RUNS.replace(0)
    }
    #[cfg(not(test))]
    {
        SCRIPT_INIT_RUNS.swap(0, std::sync::atomic::Ordering::Relaxed)
    }
}

pub(crate) struct ScriptInit {
    pub control_values: Vec<(sampler_core::ControlId, i32)>,
    pub states: Vec<Option<Result<sampler_ksp::Initialized, String>>>,
    pub faults: Vec<Option<sampler_ksp::Error>>,
    pub resources: Option<Resources>,
}

/// Only final physical source identities, independent of dense IR ordering.
/// Raw metadata preserves authored keys when playback admission narrows them.
fn admitted_zone_metadata(instrument: &ir::Instrument) -> std::collections::BTreeMap<u32, [i32; 3]> {
    if instrument.source_indices.zone_catalog.len()!=instrument.source_indices.zones.len() {return Default::default();}
    first_homes(instrument.source_indices.zone_catalog.iter().zip(&instrument.source_indices.zones)
        .map(|(source,entry)| (source, entry.as_ref()
            .and_then(|z| instrument.zones.get(z.0)?.asset).is_some())))
}

/// Home of each identity's first zone (Kontakt's lookup order), if that zone is kept.
pub(crate) fn first_homes<'a>(zones: impl Iterator<Item = (&'a ir::SourceZone, bool)>)
    -> std::collections::BTreeMap<u32, [i32; 3]> {
    let mut homes = std::collections::BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    for (zone, kept) in zones {
        let Some(id) = zone.id.filter(|id| seen.insert(*id)) else { continue };
        if let (true, Ok(group)) = (kept, i32::try_from(zone.home.group)) {
            homes.insert(id, [group, i32::from(zone.home.low_key), i32::from(zone.home.high_key)]);
        }
    }
    homes
}

pub(crate) fn admit_initialized_zone_sources(instrument: &mut ir::Instrument, initialized: &mut ScriptInit) {
    let metadata = admitted_zone_metadata(instrument);
    for (index, state) in initialized.states.iter_mut().enumerate() {
        let Some(Ok(state)) = state else { continue; };
        for rejected in state.admit_source_zones(metadata.clone()) {
            let slot = instrument.behaviors[index].slot.unwrap_or(index.min(u8::MAX.into()) as u8);
            let feature = match rejected.reason {
                sampler_ksp::WaveformSourceReason::Unavailable => "waveform source unavailable after admission",
                sampler_ksp::WaveformSourceReason::IdentityChanged => "waveform source identity changed before admission",
            };
            instrument.unsupported.push(ir::Unsupported {
                location: format!("script slot {slot} waveform {} source zone {}", rejected.ui, rejected.zone),
                feature: feature.into(),
                value: "Attachment and its properties omitted; no source identity was substituted".into(),
                reason: ir::Reason::NotModeled,
            });
        }
    }
}

/// A positioned KSP failure retained from the translated instrument's own initialization.
#[derive(Clone, Debug)]
pub struct ScriptInitFault {
    pub behavior_index: usize,
    pub slot: Option<u8>,
    pub error: sampler_ksp::Error,
}

pub(crate) fn initialize_scripts(
    instrument: &mut ir::Instrument,
    library: Option<&Path>,
    groups: Vec<String>,
    control_values: &[(sampler_core::ControlId, i32)],
) -> ScriptInit {
    let metadata = admitted_zone_metadata(instrument);
    initialize_scripts_with_zones(instrument, library, groups, control_values, &metadata)
}

pub(crate) fn initialize_scripts_with_zones(
    instrument: &mut ir::Instrument,
    library: Option<&Path>,
    groups: Vec<String>,
    control_values: &[(sampler_core::ControlId, i32)],
    metadata: &std::collections::BTreeMap<u32, [i32; 3]>,
) -> ScriptInit {
    let resources = library.map(Resources::of).map(std::cell::RefCell::new);
    let mut views = Vec::new();
    let mut midi_object = sampler_core::MidiObject::default();
    let mut zone_parameters = metadata.clone();
    let mut snapshot_mode = sampler_ksp::model::SnapshotMode::default();
    let mut faults = vec![None; instrument.behaviors.len()];
    let states = instrument
        .behaviors
        .iter()
        .enumerate()
        .map(|(index, behavior)| {
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
            } else if behavior.source.contains("load_performance_view") {
                // Kontakt evaluates the file name at the call; only one literal name
                // in on init is read here, so other forms leave the view empty.
                views.push(ir::Unsupported {
                    location: behavior.name.clone(),
                    feature: "performance view".into(),
                    value: "load_performance_view name is not one literal in on init; no view is read"
                        .into(),
                    reason: ir::Reason::NotModeled,
                });
            }
            let mut environment = script_environment_with_zones(
                behavior,
                index,
                groups.clone(),
                &instrument.source_indices,
                &instrument.zones,
                performance_view,
                Some(&zone_parameters),
            );
            if let Some(resources) = resources.as_ref() {
                (environment.data_arrays, environment.data_array_reasons) =
                    data_arrays(&behavior.source, &mut resources.borrow_mut());
            }
            environment.snapshot_mode = snapshot_mode;
            environment.midi_object = midi_object.clone();
            environment.instrument_persisted =
                instrument.snapshot_base_state.get(index).map(|state| saved_values(state));
            environment.folders = script_folders(library);
            environment.control_values.extend(
                control_values
                    .iter()
                    .map(|&(id, value)| (id, Value::Int(value))),
            );
            (behavior.language == ir::Language::Ksp).then(|| {
                #[cfg(feature = "scan")]
                sampler_ksp::scan::attempt("runtime-preparation");
                #[cfg(test)]
                SCRIPT_INIT_RUNS.set(SCRIPT_INIT_RUNS.get() + 1);
                #[cfg(all(feature = "scan", not(test)))]
                SCRIPT_INIT_RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                sampler_ksp::initialize(
                    &behavior.source,
                    sampler_ksp::Limits::LIBRARY,
                    &environment,
                )

                .inspect(|initialized| {
                    midi_object = initialized.midi_object().clone();
                    zone_parameters.extend(initialized.zone_parameters().iter().map(|(&id,&home)|(id,home)));
                    if let Some(write)=initialized.snapshot_mode_write() {snapshot_mode=write.mode;}
                })
                .map_err(|e| {
                    faults[index] = Some(e.clone());
                    e.to_string()
                })
            })
        })
        .collect();
    instrument.unsupported.append(&mut views);
    ScriptInit {
        control_values: control_values.to_vec(),
        states,
        faults,
        resources: resources.map(std::cell::RefCell::into_inner),
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
    compile_ui_initialized(instrument, options, None)
}

impl Kontakt {
    /// Positioned faults from the KSP initialization performed during translation.
    /// Read this before [`Kontakt::compile_ui`] consumes the initialized state.
    pub fn script_init_faults(&self) -> Vec<ScriptInitFault> {
        let Some(initialized) = &self.initialized else {
            return Vec::new();
        };
        initialized
            .faults
            .iter()
            .enumerate()
            .filter_map(|(behavior_index, error)| {
                let behavior = self.instrument.behaviors.get(behavior_index)?;
                Some(ScriptInitFault {
                    behavior_index,
                    slot: behavior.slot,
                    error: error.as_ref()?.clone(),
                })
            })
            .collect()
    }

    /// Project the UI from a translated instrument, reusing its script init
    /// state once. A different set of host control values reinitializes it.
    pub fn compile_ui(
        &mut self,
        options: &Options,
    ) -> (
        Vec<sampler_ksp::Script>,
        Vec<sampler_ui_ir::Interface>,
        Option<Resources>,
    ) {
        compile_ui_initialized(&mut self.instrument, options, self.initialized.take())
    }
}

fn compile_ui_initialized(
    instrument: &mut ir::Instrument,
    options: &Options,
    initialized: Option<ScriptInit>,
) -> (
    Vec<sampler_ksp::Script>,
    Vec<sampler_ui_ir::Interface>,
    Option<Resources>,
) {
    let (rate, scripts) = (options.rate, options.scripts);
    // The public instrument may have gained/removed behaviors or changed their
    // languages since translation. Reuse only a matching initialization shape;
    // otherwise initialize the current instrument through the ordinary path.
    // Same-shape KSP source changes still fail compile_initialized's digest check.
    let initialized = initialized.filter(|init| {
        init.control_values == options.control_values
            && init.states.len() == instrument.behaviors.len()
            && init
                .states
                .iter()
                .zip(&instrument.behaviors)
                .all(|(state, behavior)| {
                    state.is_some() == (behavior.language == ir::Language::Ksp)
                })
    });
    let mut initialized = initialized.unwrap_or_else(|| {
        let groups = instrument.groups.iter().map(|g| g.name.clone()).collect();
        initialize_scripts(instrument, options.library.as_deref(), groups, &options.control_values)
    });
    admit_initialized_zone_sources(instrument, &mut initialized);
    let ScriptInit {
        mut states,
        faults,
        resources,
        ..
    } = initialized;
    let limits = sampler_ksp::Limits::LIBRARY;
    let mut compiled = Vec::new();
    let mut names = Vec::new();
    let resources = resources.map(std::cell::RefCell::new);
    for (index, behavior) in instrument.behaviors.iter().enumerate() {
        let mut error_line = faults.get(index).and_then(|fault| fault.as_ref()).map(|fault| fault.line);
        let result = match behavior.language {
            _ if !scripts => Err("scripts disabled".to_string()),
            ir::Language::Ksp => states[index]
                .take()
                .expect("KSP initialized")
                .and_then(|init| {
                    sampler_ksp::compile_initialized(&behavior.source, rate, limits, &[], init)
                        .map_err(|e| {
                            error_line = Some(e.line);
                            e.to_string()
                        })
                }),
            ref other => Err(format!("{other:?} has no frontend")),
        };
        match result {
            Ok(script) => {
                if !cfg!(feature = "native-ui")
                    && script
                        .model()
                        .requests
                        .iter()
                        .any(|r| r.command == "load_native_ui")
                {
                    instrument.unsupported.push(ir::Unsupported {
                        location: behavior.name.clone(), feature: "native interface".into(),
                        value: "The requested native performance view is unavailable; showing the script controls.".into(),
                        reason: ir::Reason::NotModeled,
                    });
                }
                instrument
                    .unsupported
                    .extend(script.warnings().iter().map(|w| ir::Unsupported {
                        location: format!("script slot {} {} line {}", behavior.slot.map_or(index, usize::from), behavior.name, w.line),
                        feature: match w.builtin {
                            Some(builtin) => format!("script {:?}: {builtin}", w.kind),
                            None => format!("script {:?}: {}", w.kind, warning_class(&w.message)),
                        },
                        value: w.message.clone(),
                        reason: ir::Reason::NotModeled,
                    }));
                names.push((
                    behavior.slot.map_or(index, usize::from),
                    behavior.name.clone(),
                ));
                compiled.push(script);
            }
            Err(error) => instrument.unsupported.push(ir::Unsupported {
                location: format!("script slot {} {}{}", behavior.slot.map_or(index, usize::from),
                    behavior.name, error_line.map_or_else(String::new, |line| format!(" line {line}"))),
                feature: "script".into(),
                value: error,
                reason: ir::Reason::NotModeled,
            }),
        }
    }
    let picture = |path: &str| resources.as_ref()?.borrow_mut().picture(path);
    let mut interfaces = Vec::new();
    for (script, (slot, name)) in compiled.iter().zip(&names) {
        match script.ui(&picture) {
            Ok(ui) => {
                // A successful projection can still omit source UI meaning. Keep
                // those gaps in the same report exposed by the playable loader.
                instrument
                    .unsupported
                    .extend(ui.unsupported.iter().map(|u| ir::Unsupported {
                        location: match u.widget {
                            Some(widget) => format!("script slot {slot} {name} widget {}", widget.0),
                            None => format!("script slot {slot} {name}"),
                        },
                        feature: format!("script interface: {}", u.feature),
                        value: u.value.clone(),
                        reason: ir::Reason::NotModeled,
                    }));
                interfaces.push(ui);
            }
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

fn prepare_inner(instrument:ir::Instrument,pcm:Vec<Pcm>,options:&Options,initialized:Option<ScriptInit>) ->Result<Loaded,LoadError> {
    prepare_sources_inner(instrument,pcm.into_iter().map(SourceStorage::Stereo).collect(),options,initialized)
}
/// Bind source-native recipes only after ordinary readers supplied actual
/// immutable source geometry. Both resident and streamed loaders reach here.
fn bind_source_processing(instrument: &mut ir::Instrument, sources: &[SourceStorage], options: &Options) -> Result<(), LoadError> {
    let invalid = |reason: &str| LoadError::Invalid {
        path: options.library.clone().unwrap_or_default(), reason: reason.into(),
    };
    instrument.validate().map_err(|error| invalid(&error.to_string()))?;
    // Entire graph and capability batch is pure before changing any roster.
    let mut resolved=Vec::with_capacity(instrument.source_indices.processing.len());
    let mut matrices=Vec::new();
    let mut owner_roles=Vec::new();
    let mut parameter_sites=Vec::new();
    // This off-audio schema batch is independent of the live Instrument until
    // every owner, source and signal definition has been checked.
    let mut control_definitions=instrument.controls.clone();
    let mut next=instrument.processing_scopes.len();
    let mut warnings=Vec::new();
    for recipe in &instrument.source_indices.processing {
        let native=instrument.source_indices.uvi_native_processing.iter().find(|profile|profile.owner==recipe.owner)
            .filter(|profile|profile.requested==Some(ir::UviNativeLayout::Code2)
                && profile.members.iter().any(|(_,member)|member.is_some_and(|member|member.layout==ir::UviNativeLayout::Code6))
                && !instrument.source_indices.processing_ports.iter().any(|port|port.owner==recipe.owner));
        // Native Frequency attachment and parameter cadence are not measured
        // from UVI. Bind the typed parameter to the native one-pole site once
        // per voice, and keep the uncertainty visible as a warning.
        if native.is_some()&&instrument.source_indices.processing_parameters.iter().any(|binding|binding.owner==recipe.owner) {
            warnings.push(ir::Unsupported{location:format!("{:?}",recipe.owner),feature:"native Frequency connection attachment and typed parameter cadence".into(),
                value:"approximated: the parameter binds the native one-pole Freq per voice; UVI's real attachment cadence is unmeasured".into(),reason:ir::Reason::UnknownLaw});
        }
        let mut width=None;
        for (ordinal,member) in recipe.members.iter().enumerate() {
            let Some(asset)=instrument.zones[member.0].asset else {continue;};
            let actual=sources.get(asset.0).ok_or_else(||invalid("source processing asset is absent"))?.as_source().channels().get();
            if let Some(native)=native {
                let &(zone,input)=native.members.get(ordinal).ok_or_else(||invalid("native member ordinal missing"))?;
                let input=input.ok_or_else(||invalid("native source has no genuine decoded header geometry"))?;
                if zone!=*member || input.storage_channels!=actual || input.provenance!=ir::UviNativeSourceProvenance::PlainHeader {
                    return Err(invalid("native source geometry differs from actual retained storage"));
                }
                width=Some(width.unwrap_or(0).max(input.layout.channels()));
            }else if width.replace(actual).is_some_and(|previous|previous!=actual) {
                return Err(invalid("mixed source layouts require explicit per-member layout edges"));
            }
        }
        let scope=if let Some(index)=recipe.compiled {index}else{let index=next;next=next.checked_add(1).ok_or_else(||invalid("source scope capacity overflow"))?;index};
        let mut definition=ir::ProcessingScope {members:recipe.members.clone(),sum:ir::ProcessingSumLaw::OrderedF32,
            cadence:ir::ProcessingCadence::CoreCell64,chain:recipe.chain,amplitude:recipe.amplitude,output:recipe.output,signal:None};
        let mut signal_sites=None;
        if let Some(width)=width.filter(|width|*width!=2) {
            let (signal,bindings,sites)=source_processing_signal(instrument,recipe,scope,width,native,&mut control_definitions).map_err(|reason|invalid(&reason))?;
            definition.chain=None;
            definition.signal=Some(signal);
            signal_sites=Some(sites);
            matrices.extend(bindings);
        } else if width==Some(2) {
            for (index,matrix) in instrument.source_indices.processing_matrices.iter().enumerate().filter(|(_,matrix)|matrix.owner==recipe.owner) {
                if Some(matrix.chain)!=recipe.chain {return Err(invalid("source matrix has no actual chain coordinate"));}
                matrices.push((index,ir::SourceProcessingMatrixBinding{scope,signal_node:None,layout:ir::SignalLayout{channels:2},
                    cells:source_matrix_cells(matrix,2,&mut control_definitions).map_err(|reason|invalid(&reason))?}));
            }
        }
        let mut roles=Vec::new();
        for role in &recipe.parameter_roles {
            let chain=recipe.chain.and_then(|c|instrument.chains.get(c.0)).ok_or_else(||invalid("source scalar has no actual chain"))?;
            let site=if let Some(sites)=&signal_sites {
                ir::ScopeControlSite::SignalNode(*sites.get(role.chain_index).ok_or_else(||invalid("source scalar has no actual emitted signal site"))?)
            }else if role.chain_index<chain.pre_amplitude.len(){ir::ScopeControlSite::ChainPre(role.chain_index)}
            else{ir::ScopeControlSite::ChainPost(role.chain_index-chain.pre_amplitude.len())};
            roles.push(ir::SourceOwnerOutputControl{owner:recipe.owner,field:role.field,target:ir::SourceOwnerControlTarget::ProcessingScope{scope,site},control:role.control});
        }
        owner_roles.extend(roles);
        for (row,binding) in instrument.source_indices.processing_parameters.iter().enumerate().filter(|(_,row)|row.owner==recipe.owner){
            if recipe.chain!=Some(binding.chain){return Err(invalid("shared processor binding belongs to another source chain"));}
            let chain=instrument.chains.get(binding.chain.0).ok_or_else(||invalid("shared processor chain absent"))?;
            let site=if let Some(sites)=&signal_sites {
                ir::ScopeControlSite::SignalNode(*sites.get(binding.index).filter(|site|**site!=usize::MAX).ok_or_else(||invalid("shared processor emitted operand absent"))?)
            }else if binding.index<chain.pre_amplitude.len(){ir::ScopeControlSite::ChainPre(binding.index)}
            else{ir::ScopeControlSite::ChainPost(binding.index-chain.pre_amplitude.len())};
            parameter_sites.push((row,ir::SourceProcessingParameterSite{scope,site}));
        }
        resolved.push((scope,definition));
    }
    // Check the complete destination roster before publishing any schema row.
    // New indices must be the checked append sequence; old bindings must still
    // address an existing scope. Keep malformed plans on the typed error path.
    let existing_scopes=instrument.processing_scopes.len();
    let mut append_scope=existing_scopes;
    for (recipe,(index,_)) in instrument.source_indices.processing.iter().zip(&resolved) {
        if recipe.compiled.is_some() {
            if *index>=existing_scopes {return Err(invalid("compiled source processing scope is absent"));}
        } else {
            if *index!=append_scope {return Err(invalid("source processing append roster is inconsistent"));}
            append_scope=append_scope.checked_add(1).ok_or_else(||invalid("source scope capacity overflow"))?;
        }
    }
    // Re-materialization replaces this recipe's exact roles, never appends a
    // second authority. Preserve unrelated directly authored bus/scope roles.
    instrument.source_indices.output_controls.retain(|home|!instrument.source_indices.processing.iter().any(|recipe|
        recipe.owner==home.owner && matches!(home.target,ir::SourceOwnerControlTarget::ProcessingScope{scope,..}if recipe.compiled==Some(scope))));
    instrument.source_indices.output_controls.extend(owner_roles);
    for (row,site) in parameter_sites {instrument.source_indices.processing_parameters[row].compiled=Some(site);}
    instrument.controls=control_definitions;
    for (recipe,(index,definition)) in instrument.source_indices.processing.iter_mut().zip(resolved) {
        if recipe.compiled.is_some() {instrument.processing_scopes[index]=definition;}
        else {instrument.processing_scopes.push(definition);}
        recipe.compiled=Some(index);
        for role in &mut recipe.parameter_roles {
            role.compiled=instrument.source_indices.output_controls.iter().find_map(|home|
                (home.owner==recipe.owner&&home.field==role.field).then_some(home.target)).and_then(|target|
                    if let ir::SourceOwnerControlTarget::ProcessingScope{scope,site}=target{(scope==index).then_some(site)}else{None});
        }
    }
    instrument.unsupported.extend(warnings);
    for matrix in &mut instrument.source_indices.processing_matrices {matrix.compiled=None;}
    for (index,binding) in matrices {
        instrument.source_indices.processing_matrices[index].controls=binding.cells.clone();
        instrument.source_indices.processing_matrices[index].compiled=Some(binding);
    }
    Ok(())
}

fn source_matrix_cells(
    matrix:&ir::SourceProcessingMatrix,width:usize,controls:&mut Vec<ir::Control>,
)->Result<Vec<ir::SourceProcessingCell>,String> {
    let mut cells=Vec::new();
    for cell in matrix.control_definitions.iter().filter(|cell|cell.input<width && cell.output<width) {
        let control=if let Some(index)=controls.iter().position(|control|control.key==cell.control.key) {
            if controls[index]!=cell.control {
                return Err("source matrix control schema differs from the retained authored definition".into());
            }
            index
        } else {
            let index=controls.len();controls.push(cell.control.clone());index
        };
        cells.push(ir::SourceProcessingCell{input:cell.input,output:cell.output,control:ir::ControlRef(control)});
    }
    Ok(cells)
}

/// The source adapter retains raw coordinates; this ordinary materializer
/// chooses actual geometry. No decoder fold and no source-count/native-enum
/// equivalence is implicit in the concrete caller-supplied output matrix.
fn source_processing_signal(
    instrument:&ir::Instrument,recipe:&ir::SourceProcessingRecipe,scope:usize,width:usize,native:Option<&ir::UviNativeProcessingProfile>,control_definitions:&mut Vec<ir::Control>,
)->Result<(ir::ProcessingSignal,Vec<(usize,ir::SourceProcessingMatrixBinding)>,Vec<usize>),String> {
    if !(1..=12).contains(&width) {return Err("source graph width is outside its admitted channel domain".into());}
    let ports:Vec<_>=instrument.source_indices.processing_ports.iter().filter(|port|port.owner==recipe.owner).collect();
    let port=if native.is_some(){None}else{
        if ports.len()!=1 {return Err("wide source owner requires one explicit caller output matrix and layout provenance".into());}
        let port=&ports[0].conversion;
        if port.input.channels!=width || port.output.channels!=2 {return Err("caller output matrix differs from actual source working layout".into());}Some(port)
    };
    let mut nodes=Vec::new();let mut controls=Vec::new();let mut matrices=Vec::new();let mut boundary_pan=None;let mut sites=Vec::new();
    if let Some(chain_id)=recipe.chain {
        let chain=&instrument.chains[chain_id.0];
        if chain.scope!=ir::Scope::Voice || !chain.post_amplitude.is_empty() {
            return Err("source channel graph requires its explicit amplitude boundary".into());
        }
        for route in instrument.routes.iter().filter(|route|matches!(route.target,ir::Target::Processor{chain,..}if chain==chain_id)) {
            let ir::Target::Processor{index,parameter,..}=route.target else{return Err("source channel route has no retained processor target".into());};
            if !instrument.source_indices.processing_parameters.iter().any(|binding|binding.owner==recipe.owner&&binding.chain==chain_id&&binding.index==index&&binding.parameter==parameter){
                return Err("source channel processor modulation requires an exact owned parameter binding".into());
            }
        }
        sites.resize(chain.pre_amplitude.len(),usize::MAX);
        for (index,processor) in chain.pre_amplitude.iter().enumerate() {
            let parameter=|constant,role|->Result<ir::SignalParameter,String>{
                let bindings:Vec<_>=instrument.processor_controls.iter().filter(|binding|binding.chain==chain_id && binding.index==index).collect();
                if bindings.len()>1 || bindings.first().is_some_and(|binding|binding.parameter!=role) {
                    return Err("source channel parameter has an unsupported or ambiguous control binding".into());
                }
                Ok(ir::SignalParameter{constant,control:bindings.first().map(|binding|instrument.controls[binding.control.0].key.clone()),expression:None})
            };
            if let ir::Processor::ExpressionPan(pan)=*processor {
                if boundary_pan.is_some(){return Err("source owner has multiple expression Pan homes".into());}
                boundary_pan=Some((index,ir::ChannelPan{position:parameter(pan.pan.position,ir::ProcessorParameter::Pan)?,offset:pan.offset,law:pan.pan.law,expression:true}));
                continue;
            }
            let node=match *processor {
                ir::Processor::Mix{count,address,dry,wet,bypass}=>ir::ProcessingSignalNode::Mix(ir::ChannelMix{count,address,dry,wet,bypass}),
                ir::Processor::Gain(gain)=>ir::ProcessingSignalNode::Gain(ir::ChannelGain{
                    gain:parameter(gain.linear(),ir::ProcessorParameter::Gain)?,arithmetic:ir::ChannelGainArithmetic::CoreGainF64ThenF32}),
                ir::Processor::Color(config) if matches!(config.curve,ir::fx::color::Curve::Rectifier(_))=>{
                    let ir::fx::color::Curve::Rectifier(mode)=config.curve else {return Err("source rectifier type is absent".into());};
                    if config!=ir::fx::color::Config::rectifier(mode) {
                        return Err("source rectifier has additional stages requiring their actual N-port kernels".into());
                    }
                    ir::ProcessingSignalNode::Rectifier(mode)
                }
                ir::Processor::Filter(ir::Filter{kind,cutoff:ir::Frequency::Hertz(hz),..})=>{
                    let mode=match kind {ir::FilterKind::LowPass{poles:1}=>ir::ChannelOnePoleMode::LowPass,
                        ir::FilterKind::HighPass{poles:1}=>ir::ChannelOnePoleMode::HighPass,
                        _=>return Err("source channel filter requires its actual N-port kernel".into())};
                    // Preserve ordinary voice-chain's explicit zero added tail
                    // for this processor. This is not a claim that a native IIR
                    // physically has finite duration.
                    if let Some(site)=native.and_then(|profile|profile.one_poles.iter().find(|site|site.chain==chain_id&&site.index==index)) {
                        let key=site.frequency_control.as_ref().ok_or("native OnePole Freq is outside its typed public range")?;
                        let definition=ir::Control{key:key.clone(),label:"Freq".into(),value:ir::ControlValue::Continuous{min:20.,max:20000.,default:site.frequency,unit:ir::ControlUnit::Hertz},automation:ir::Automation::None};
                        if let Some(existing)=control_definitions.iter().find(|control|control.key==*key){
                            if existing!=&definition{return Err("native OnePole Freq schema differs from its actual source control".into());}
                        }else{control_definitions.push(definition);}
                        ir::ProcessingSignalNode::UviOnePole(ir::UviNativeOnePole{frequency:ir::SignalParameter{constant:site.frequency,control:Some(key.clone()),expression:None},tracking:site.tracking,mode:site.mode,
                            cadence:ir::UviOnePoleCadence::FirstSetup32,tail_frames:0})
                    }else{ir::ProcessingSignalNode::OnePole(ir::ChannelOnePole{cutoff_hz:parameter(hz,ir::ProcessorParameter::Cutoff)?,mode,
                        arithmetic:ir::ChannelOnePoleArithmetic::CoreOnePoleF64,tail_frames:0})}
                }
                ir::Processor::StereoMatrix(_)=>{
                    let found:Vec<_>=instrument.source_indices.processing_matrices.iter().enumerate()
                        .filter(|(_,matrix)|matrix.owner==recipe.owner && matrix.chain==chain_id && matrix.index==index).collect();
                    if found.len()!=1 {return Err("wide matrix has no unique full authored source definition".into());}
                    let (matrix_index,matrix)=found[0];
                    if width>matrix.matrix.input.channels || width>matrix.matrix.output.channels {return Err("source matrix is narrower than its actual working port".into());}
                    let mut cells=Vec::with_capacity(width*width);
                    for output in 0..width {for input in 0..width {cells.push(matrix.matrix.cells[output*matrix.matrix.input.channels+input]);}}
                    let bound=source_matrix_cells(matrix,width,control_definitions)?;
                    for cell in &bound {controls.push(ir::ProcessingCellBinding{node:nodes.len(),input:cell.input,output:cell.output,
                        control:control_definitions[cell.control.0].key.clone()});}
                    matrices.push((matrix_index,ir::SourceProcessingMatrixBinding{scope,signal_node:Some(nodes.len()),layout:ir::SignalLayout{channels:width},cells:bound}));
                    ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{input:ir::SignalLayout{channels:width},output:ir::SignalLayout{channels:width},cells,
                        arithmetic:matrix.matrix.arithmetic})
                }
                _=>return Err("source channel processor requires its actual N-port kernel".into()),
            };
            sites[index]=nodes.len();
            nodes.push(node);
        }
    }
    let source_law=if let Some(native)=native {
        let source=ir::UviNativeLayout::plain_header(width).ok_or("native working descriptor is unproved")?;
        let destination=native.requested.ok_or("native caller descriptor missing")?;
        if destination!=ir::UviNativeLayout::Code2{return Err("normal backend requires its real stereo Frame outlet".into());}
        let pan=if let Some((index,pan))=boundary_pan {sites[index]=nodes.len();pan}
            else{ir::ChannelPan{position:ir::SignalParameter{constant:0.,control:None,expression:None},offset:0.,law:ir::PanLaw::RaisedCosineCoreF64,expression:false}};
        nodes.push(ir::ProcessingSignalNode::UviLayout(ir::UviNativeLayoutConvert{source,destination,pan:pan.position,pan_offset:pan.offset}));
        // Explicit user extension home, separate from the native BASE Pan that
        // descriptor6->2 ignores. Center is exactly unity; canonical expression
        // remains per activation BEFORE different notes enter global buses.
        nodes.push(ir::ProcessingSignalNode::Pan(ir::ChannelPan{position:ir::SignalParameter{constant:0.,control:None,expression:None},offset:0.,law:ir::PanLaw::RaisedCosineCoreF64,expression:true}));
        let mut members=[None;16];
        if native.members.len()!=recipe.members.len()||native.members.len()>members.len(){return Err("native source ordinal roster is inconsistent".into());}
        for (ordinal,(zone,input))in native.members.iter().enumerate(){
            if *zone!=recipe.members[ordinal]{return Err("native source ordinal identity is inconsistent".into());}
            members[ordinal]=Some(input.ok_or("native source geometry remains unresolved")?);
        }
        ir::ProcessingSourceLaw::UviC997Layout{members}
    }else{
        nodes.push(ir::ProcessingSignalNode::Matrix(port.ok_or("explicit caller port disappeared")?.clone()));
        if let Some((index,pan))=boundary_pan {sites[index]=nodes.len();nodes.push(ir::ProcessingSignalNode::Pan(pan));}
        ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet
    };
    Ok((ir::ProcessingSignal{source_law,input:ir::SignalLayout{channels:width},nodes,controls},matrices,sites))
}

fn prepare_sources_inner(
    mut instrument:ir::Instrument,sources:Vec<SourceStorage>,options:&Options,initialized:Option<ScriptInit>,
)->Result<Loaded,LoadError> {
    let rate = options.rate;
    bind_source_processing(&mut instrument, &sources, options)?;
    host_volume(&mut instrument);
    let lower_options = sampler_core::lower::Options { mpe: options.mpe };
    let ui_span = crate::audit::Span::new("script_ui_prepare");
    let (compiled, interfaces, resources) =
        compile_ui_initialized(&mut instrument, options, initialized);
    drop(ui_span);
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
            sampler_core::lower::lower_sources_with(&instrument, rate, sources, &lower_options, |_, plan| {
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
        let target = compiled
            .iter()
            .find(|script| script.view().slot() == binding.source_slot)
            .and_then(|script| {
                script
                    .model()
                    .interface
                    .widgets
                    .iter()
                    .filter(|w| {
                        w.kind
                            == if binding.switch {
                                sampler_ksp::model::WidgetKind::Switch
                            } else {
                                sampler_ksp::model::WidgetKind::Slider
                            }
                    })
                    .nth(binding.slider as usize)
            });
        if let Some(widget) = target {
            automation.push(sampler_core::AutomationBinding {
                source: match binding.source {
                    ir::ScriptAutomationSource::Controller(cc) => {
                        sampler_core::AutomationSource::Controller(cc)
                    }
                    ir::ScriptAutomationSource::HostParameter(address) => {
                        sampler_core::AutomationSource::HostParameter(address)
                    }
                },
                source_slot: binding.source_slot,
                ui_id: widget.ui_id,
                low: binding.low,
                high: binding.high,
                soft_takeover: binding.soft_takeover,
            });
        } else {
            instrument.unsupported.push(ir::Unsupported {
                location: format!("script slot {}", binding.source_slot),
                feature: if binding.switch {
                    "saved automation switch"
                } else {
                    "saved automation slider"
                }
                .into(),
                value: binding.slider.to_string(),
                reason: ir::Reason::InvalidValue,
            });
        }
    }
    automation.extend(fixed_automation(&compiled, &automation));
    let scripts = compiled.iter().map(sampler_ksp::Script::view).collect();
    let lower_span = crate::audit::Span::new("core_lower_bindings");
    let lowered =
        sampler_core::lower::lower_sources_with(&instrument, rate, sources, &lower_options, |_, plan| {
            sampler_ksp::bind_modules(compiled, plan)
                .and_then(|plan| plan.with_automation_bindings(automation))
                .map_err(|e| LowerError::Behavior {
                    module: "KSP".into(),
                    message: e.to_string(),
                })
        });
    drop(lower_span);
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
mod cancellation_tests {
    use super::*;

    #[test]
    fn canceled_path_load_does_not_read_a_container_or_emit_progress() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("must-not-be-read.nki");
        let mut progress_calls = 0;
        let result = load_cancelable(
            &missing,
            &Options::default(),
            |_| progress_calls += 1,
            || true,
        );
        assert!(matches!(result, Err(LoadError::Canceled)));
        assert_eq!(progress_calls, 0);
        assert!(!missing.exists());
        assert!(matches!(
            load_cancelable(&missing, &Options::default(), |_| {}, || false),
            Err(LoadError::Io { .. })
        ));
    }
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

    #[test]
    fn unrelated_start_warnings_are_not_articulation_reports() {
        let mut ir = ir::Instrument::default();
        let report = |feature: &str| ir::Unsupported {
            location: "script slot 1".into(),
            feature: feature.into(),
            value: String::new(),
            reason: ir::Reason::NotModeled,
        };
        ir.unsupported = vec![
            report("script Warning: polyphonic variables start at 0; initializer ignored"),
            report("group start join"),
            report("keyswitch script"),
        ];
        let m = articulation_migration(&ir);
        assert_eq!(m.unrecognised.len(), 2);
        assert!(m.unrecognised.iter().all(|u| !u.contains("polyphonic")));
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
            residency: Default::default(),
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

/// Host slots a script fixes with `$CONTROL_PAR_AUTOMATION_ID` (set in `on init` or by a
/// performance view's `hostAutomation`): Kontakt exposes the control at that host address
/// without any saved assignment. A saved assignment of the same control or address wins.
// ponytail: init-time ids only; a later set_control_par(AUTOMATION_ID) rebind isn't followed.
fn fixed_automation(
    compiled: &[sampler_ksp::Script],
    saved: &[sampler_core::AutomationBinding],
) -> Vec<sampler_core::AutomationBinding> {
    let mut out: Vec<sampler_core::AutomationBinding> = Vec::new();
    for script in compiled {
        for w in &script.model().interface.widgets {
            let Some(id) = w.int("$CONTROL_PAR_AUTOMATION_ID").and_then(|n| u16::try_from(n).ok()).filter(|n| *n < 2048) else {
                continue;
            };
            let source = sampler_core::AutomationSource::HostParameter(id);
            let taken = |b: &sampler_core::AutomationBinding| {
                b.source == source || (b.source_slot == script.slot() && b.ui_id == w.ui_id)
            };
            if w.unresolved
                || w.control.is_none()
                || w.int("$CONTROL_PAR_ALLOW_AUTOMATION") == Some(0)
                || saved.iter().chain(&out).any(taken)
            {
                continue;
            }
            out.push(sampler_core::AutomationBinding {
                source,
                source_slot: script.slot(),
                ui_id: w.ui_id,
                low: 0.,
                high: 1.,
                soft_takeover: false,
            });
        }
    }
    out
}

#[cfg(test)]
mod automation_tests {
    use super::*;
    #[test]
    fn automation_id_binds_the_host_slot_and_runs_the_callback() {
        let mut instrument = ir::Instrument::default();
        instrument.behaviors.push(ir::Behavior {
            name: "fixed".into(), language: ir::Language::Ksp, slot: Some(1), state: vec![], requires: vec![],
            source: "on init declare ui_slider $a(0,100) declare ui_slider $b(0,100) declare $seen \
                     set_control_par(get_ui_id($a),$CONTROL_PAR_AUTOMATION_ID,5) \
                     set_control_par(get_ui_id($b),$CONTROL_PAR_AUTOMATION_ID,6) \
                     set_control_par(get_ui_id($b),$CONTROL_PAR_ALLOW_AUTOMATION,0) end on \
                     on ui_control($a) $seen := $a end on on ui_control($b) $seen := 1000 end on".into(),
        });
        let loaded = prepare(instrument, vec![], &Options::default()).unwrap();
        let bound: Vec<_> = loaded.plan.automation_bindings().iter().map(|b| b.source).collect();
        assert_eq!(bound, [sampler_core::AutomationSource::HostParameter(5)]);
        let limits = sampler_core::Limits::for_plan(&loaded.plan, 4, 8);
        let mut rt = sampler_core::Runtime::new(loaded.plan, limits).unwrap();
        let context = sampler_core::ControlContext {
            performance: rt.performance(0).unwrap(),
            origin: sampler_core::ChannelAddress { protocol: sampler_core::Protocol::Native, port: 0, group: 0, channel: 0 },
            channels: 1,
        };
        rt.dispatch_host_parameter(context, 5, 0.5).unwrap();
        assert_eq!(rt.script_cell(rt.active_plan(), sampler_core::ScriptInstanceId(0), 0), Ok(50));
    }
    #[test]
    fn saved_target_counts_only_sliders_in_its_physical_slot() {
        let mut instrument = ir::Instrument::default();
        instrument.behaviors.push(ir::Behavior {
            name: "learn".into(), language: ir::Language::Ksp, slot: Some(3), state: vec![], requires: vec![],
            source: "on init declare ui_label $label(1,1) declare ui_slider $unused(0,127) declare ui_knob $knob(0,100,1) declare ui_slider $target(20,100) declare $observed end on on ui_control($target) $observed := $target end on".into(),
        });
        instrument.script_automation.push(ir::ScriptAutomation {
            source: ir::ScriptAutomationSource::Controller(21),
            switch: false,
            source_slot: 3,
            slider: 1,
            low: 0.,
            high: 1.,
            soft_takeover: false,
        });
        let loaded = prepare(instrument, vec![], &Options::default()).unwrap();
        let limits = sampler_core::Limits::for_plan(&loaded.plan, 4, 8);
        let mut rt = sampler_core::Runtime::new(loaded.plan, limits).unwrap();
        rt.dispatch_controller(
            rt.performance(0).unwrap(),
            sampler_core::ChannelAddress {
                protocol: sampler_core::Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
            },
            1,
            21,
            (u64::from(u32::MAX) * 64 / 127) as u32,
        )
        .unwrap();
        let id = rt.widget_id(rt.active_plan(), 3, 32771).unwrap();
        assert_eq!(
            rt.widget_value(rt.active_plan(), id, 0),
            Ok(sampler_core::WidgetValue::Integer(60))
        );
        assert_eq!(
            rt.script_cell(rt.active_plan(), sampler_core::ScriptInstanceId(0), 1),
            Ok(60)
        );
    }
}

#[cfg(test)]
mod native_lookup_tests {
    use super::*;
    #[test]
    fn init_envelope_write_reaches_pcm_through_production_prepare() {
        let mut instrument = ir::Instrument::default();
        instrument.assets.push(ir::Asset {
            location: ir::AssetLocation::Path("synthetic.wav".into()),
            residency: Default::default(),
            encoding: ir::Encoding::Wav,
            root_key: None,
            loops: vec![],
        });
        instrument.groups.push(ir::Group::default());
        instrument.modulators.push(ir::Modulator {
            scope: ir::Scope::Voice,
            source: ir::ModulationSource::Envelope(ir::Envelope::default()),
        });
        let mut zone = ir::Zone::new(ir::AssetRef(0));
        zone.keys = ir::KeyRange { low: 60, high: 60 };
        zone.group = Some(ir::GroupRef(0));
        zone.amplitude = Some(ir::ModulatorRef(0));
        instrument.zones.push(zone);
        instrument
            .source_indices
            .modulators
            .push(ir::SourceModulator {
                group: 0,
                slot: 9,
                external: false,
                name: "ENV_AHDSR".into(),
                runtime: Some(ir::ModulatorRef(0)),
                settings: None,
            });
        instrument
            .source_indices
            .engine_lookups
            .push(ir::SourceEngineLookup {
                group: 0,
                owner: -1,
                target: false,
                name: "ENV_AHDSR".into(),
                index: 9,
            });
        instrument.behaviors.push(ir::Behavior {name:"synthetic envelope init".into(),language:ir::Language::Ksp,slot:Some(0),state:vec![],requires:vec![],source:"on init set_engine_par($ENGINE_PAR_ATTACK,200809,0,find_mod(0,\"ENV_AHDSR\"),-1) end on".into()});
        let loaded = prepare(
            instrument,
            vec![Pcm::new(48000, vec![[0.5; 2]; 4096].into_boxed_slice()).unwrap()],
            &Options {
                mpe: None,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(loaded.plan.engine_parameter_bindings().len(), 6);
        let limits = sampler_core::Limits::for_plan(&loaded.plan, 4, 4);
        let mut runtime = sampler_core::Runtime::new(loaded.plan, limits).unwrap();
        runtime
            .trigger(
                sampler_core::Input {
                    protocol: sampler_core::Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 60,
                    external_id: None,
                },
                60,
                1.,
            )
            .unwrap();
        let mut out = [[0.; 2]; 257];
        runtime.render(&mut out).unwrap();
        assert!(
            (out[256][0] - 0.5 * 256. / 480.).abs() < 0.01,
            "init engine write must change rendered attack"
        );
        assert!(
            runtime
                .take_engine_parameter_outcome()
                .is_some_and(|outcome| outcome.result.is_ok())
        );
    }
    #[test]
    fn physical_mod_and_target_names_resolve_during_init_and_note() {
        let mut instrument = ir::Instrument::default();
        instrument.groups = ["muted", "hole", "live"]
            .into_iter()
            .map(|name| ir::Group {
                name: name.into(),
                ..Default::default()
            })
            .collect();
        instrument.source_indices.engine_lookups = vec![
            ir::SourceEngineLookup {
                group: 2,
                owner: -1,
                target: false,
                name: "Source".into(),
                index: 12,
            },
            ir::SourceEngineLookup {
                group: 2,
                owner: 12,
                target: true,
                name: "Cutoff".into(),
                index: 3,
            },
        ];
        instrument.behaviors.push(ir::Behavior { name:"lookup".into(),language:ir::Language::Ksp,slot:Some(3),state:vec![],requires:vec![],
            source:"on init declare $mod := get_mod_idx(2,\"source\") declare $target := get_target_idx(2,$mod,\"cutoff\") declare $group := find_group(\"live\") declare $note_mod declare $note_target end on on note $note_mod := get_mod_idx(2,\"SOURCE\") $note_target := get_target_idx(2,$note_mod,\"CUTOFF\") end on".into() });
        let loaded = prepare(instrument, vec![], &Options::default()).unwrap();
        let plan = loaded.plan;
        let limits = sampler_core::Limits::for_plan(&plan, 4, 8);
        let mut rt = sampler_core::Runtime::new(plan, limits).unwrap();
        let cell = |rt: &sampler_core::Runtime, index| {
            rt.script_cell(rt.active_plan(), sampler_core::ScriptInstanceId(0), index)
        };
        assert_eq!(cell(&rt, 0), Ok(12));
        assert_eq!(cell(&rt, 1), Ok(3));
        assert_eq!(cell(&rt, 2), Ok(2));
        rt.trigger(
            sampler_core::Input {
                protocol: sampler_core::Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: None,
            },
            60,
            1.,
        )
        .unwrap();
        assert_eq!(cell(&rt, 3), Ok(12));
        assert_eq!(cell(&rt, 4), Ok(3));
    }
}

#[cfg(test)]
mod fit_tests {
    use super::*;

    fn looped(start: u64, loop_start: u64, loop_end: u64, crossfade: u64) -> ir::Playback {
        ir::Playback {
            start,
            looping: ir::Looping::Continuous(ir::LoopRange {
                start: loop_start,
                end: loop_end,
                crossfade: ir::Span::Frames(crossfade),
                alternating: false,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn saved_modulated_crossfade_survives_the_initial_file_boundary_clamp() {
        let mut playback = looped(0, 0, 4096, 32);
        playback.loop_control_crossfade = Some(ir::Span::Frames(32));
        let mut report = Vec::new();
        assert!(fit(&mut playback, 4096, &mut report));
        assert!(matches!(
            playback.looping,
            ir::Looping::Continuous(ir::LoopRange {
                crossfade: ir::Span::Frames(0),
                ..
            })
        ));
        assert_eq!(playback.loop_control_crossfade, Some(ir::Span::Frames(32)));
        assert!(report.is_empty());
    }

    #[test]
    fn a_loop_before_the_sample_start_is_kept_and_its_crossfade_uses_the_file() {
        let mut report = Vec::new();
        let mut playback = looped(150, 100, 400, 50);
        assert!(fit(&mut playback, 1000, &mut report));
        assert_eq!(playback, looped(150, 100, 400, 50));
        assert!(report.is_empty(), "{report:?}");
        // Nothing precedes the file start: clamped to no crossfade, as in Kontakt, without a report.
        let mut at_start = looped(0, 0, 400, 50);
        assert!(fit(&mut at_start, 1000, &mut report));
        assert_eq!(at_start, looped(0, 0, 400, 0));
        assert!(report.is_empty(), "{report:?}");
    }
}

/// Message with the per-site parts (`$names`, numbers) masked, so each distinct script warning
/// is one feature class in diagnostics instead of a single opaque "script Warning".
fn warning_class(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut chars = message.chars().peekable();
    while let Some(c) = chars.next() {
        if matches!(c, '$' | '%' | '?' | '!' | '~')
            && chars.peek().is_some_and(|c| c.is_alphabetic() || *c == '_') {
            while chars.next_if(|c| c.is_alphanumeric() || *c == '_').is_some() {}
            out.push(c);
            out.push_str("name");
        } else if c.is_ascii_digit() {
            while chars.next_if(|c| c.is_ascii_digit()).is_some() {}
            out.push('N');
        } else {
            out.push(c);
        }
    }
    out
}

/// `get_folder` results (library, install, patch, factory) for an instrument file. The library
/// folder is the one holding its `Instruments` directory; the factory library is not installed
/// with the plugin, so factory paths (and the impulse responses under them) stay empty.
fn script_folders(file: Option<&std::path::Path>) -> [String; 4] {
    let dir = |path: &std::path::Path| format!("{}/", path.display());
    let Some(patch) = file.and_then(std::path::Path::parent) else {
        return Default::default();
    };
    let library = patch
        .ancestors()
        .find(|dir| dir.file_name().is_some_and(|name| name.eq_ignore_ascii_case("Instruments")))
        .and_then(std::path::Path::parent)
        .map_or_else(String::new, dir);
    [library.clone(), library, dir(patch), String::new()]
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn successful_interface_projection_keeps_unsupported_widget_features_in_load_report() {
        let mut instrument = ir::Instrument::default();
        instrument.behaviors.push(ir::Behavior {
            name: "Projection".into(),
            language: ir::Language::Ksp,
            slot: Some(5),
            state: vec![],
            requires: vec![],
            source: "on init\n declare ui_knob $Cut(0, 100, 1)\n set_control_par(get_ui_id($Cut), $CONTROL_PAR_HIDE, $HIDE_PART_MOD_LIGHT)\nend on".into(),
        });
        let (scripts, interfaces, _) = compile_ui(&mut instrument, &Options::default());
        assert_eq!(scripts.len(), 1);
        assert_eq!(interfaces.len(), 1);
        let projected = interfaces[0]
            .unsupported
            .iter()
            .find(|u| u.feature == "$HIDE_PART_MOD_LIGHT")
            .expect("the successful UI projection must record the missing feature");
        assert_eq!(projected.widget, Some(sampler_ui_ir::WidgetRef(0)));
        assert!(
            instrument.unsupported.iter().any(|u| {
                u.location == "script slot 5 Projection widget 0"
                    && u.feature == "script interface: $HIDE_PART_MOD_LIGHT"
                    && u.value == projected.value
                    && u.reason == ir::Reason::NotModeled
            }),
            "a successful projection must retain its UI gap in the instrument load report"
        );
    }

    #[test]
    fn wavetable_admits_partial_trailing_cycle_and_misaligned_start() {
        // corpus gap: Conflux's 249 zones with a misaligned start were dropped here.
        // Cycle k is read from start + k * 2048, so alignment is not needed to play.
        assert!(wavetable_cycles_admitted(0, 3 * 2048 + 100));
        assert!(wavetable_cycles_admitted(4096, 4096 + 2048));
        assert!(wavetable_cycles_admitted(64, 64 + 2 * 2048));
        assert!(!wavetable_cycles_admitted(0, 100));
        assert!(!wavetable_cycles_admitted(4096, 4096));
    }

    #[test]
    fn warning_classes_mask_array_names_and_bounds_but_keep_element_type() {
        assert_eq!(warning_class("array index -1 out of bounds for %private (length 2)"),
            "array index -N out of bounds for %name (length N)");
    }

    #[test]
    fn initialization_warnings_keep_physical_script_slot_line_and_index() {
        let mut instrument = ir::Instrument::default();
        instrument.groups.push(ir::Group { name: "present".into(), ..Default::default() });
        instrument.behaviors.push(ir::Behavior {
            name: "Lookup".into(), language: ir::Language::Ksp, slot: Some(3),
            state: vec![], requires: vec![],
            source: "on init\n declare $g\n declare %a[1]\n declare $x := %a[9]\nend on\non note\n $g := find_group(\"absent\")\nend on".into(),
        });
        let (scripts, _, _) = compile_ui(&mut instrument, &Options::default());
        assert_eq!(scripts.len(), 1);
        let warnings = &instrument.unsupported;
        assert!(warnings.iter().any(|w| w.location == "script slot 3 Lookup line 7" && w.value.contains("find_group") && w.value.contains("was not found. Index will be invalid")), "{warnings:?}");
        assert!(warnings.iter().any(|w| w.location == "script slot 3 Lookup line 4" && w.value.contains("array index 9 out of bounds")), "{warnings:?}");
    }
}

#[cfg(test)]
#[path = "zone_init_tests.rs"]
mod zone_init_tests;

#[cfg(test)]
mod initialization_shape_tests {
    use super::*;

    fn behavior(slot: u8, language: ir::Language) -> ir::Behavior {
        ir::Behavior {
            name: format!("shape slot {slot}"),
            language,
            slot: Some(slot),
            state: vec![],
            requires: vec![],
            source: "on init\n declare ui_knob $Value(0, 127, 1)\nend on".into(),
        }
    }

    fn translated(behaviors: Vec<ir::Behavior>) -> Kontakt {
        let mut instrument = ir::Instrument {
            behaviors,
            ..Default::default()
        };
        let initialized = initialize_scripts(&mut instrument, None, vec![], &[]);
        Kontakt {
            instrument,
            locations: vec![],
            samples: crate::Samples::new(Path::new(".")),
            initialized: Some(initialized),
        }
    }

    #[test]
    fn unchanged_behavior_shape_consumes_existing_initialization_once() {
        take_script_init_runs();
        let mut kontakt = translated(vec![behavior(3, ir::Language::Ksp)]);
        assert_eq!(take_script_init_runs(), 1);
        let (scripts, interfaces, _) = kontakt.compile_ui(&Options::default());
        assert_eq!(scripts.len(), 1);
        assert_eq!(interfaces.len(), 1);
        assert_eq!(scripts[0].slot(), 3);
        assert_eq!(take_script_init_runs(), 0);
        assert!(kontakt.initialized.is_none());
    }

    #[test]
    fn appended_ksp_behavior_reinitializes_before_indexing_cached_states() {
        let mut kontakt = translated(vec![behavior(3, ir::Language::Ksp)]);
        kontakt
            .instrument
            .behaviors
            .push(behavior(4, ir::Language::Ksp));
        take_script_init_runs();
        let (scripts, interfaces, _) = kontakt.compile_ui(&Options::default());
        assert_eq!(
            scripts
                .iter()
                .map(|script| script.slot())
                .collect::<Vec<_>>(),
            [3, 4]
        );
        assert_eq!(interfaces.len(), 2);
        assert_eq!(take_script_init_runs(), 2);
    }

    #[test]
    fn language_change_reinitializes_instead_of_unwrapping_absent_ksp_state() {
        let mut kontakt = translated(vec![behavior(3, ir::Language::Lua)]);
        kontakt.instrument.behaviors[0].language = ir::Language::Ksp;
        take_script_init_runs();
        let (scripts, interfaces, _) = kontakt.compile_ui(&Options::default());
        assert_eq!(scripts.len(), 1);
        assert_eq!(interfaces.len(), 1);
        assert_eq!(scripts[0].slot(), 3);
        assert_eq!(take_script_init_runs(), 1);
    }

    #[test]
    fn removed_behavior_invalidates_the_cached_roster() {
        let mut kontakt = translated(vec![
            behavior(3, ir::Language::Ksp),
            behavior(4, ir::Language::Ksp),
        ]);
        kontakt.instrument.behaviors.remove(0);
        take_script_init_runs();
        let (scripts, interfaces, _) = kontakt.compile_ui(&Options::default());
        assert_eq!(scripts.len(), 1);
        assert_eq!(interfaces.len(), 1);
        assert_eq!(scripts[0].slot(), 4);
        assert_eq!(take_script_init_runs(), 1);
    }

    #[test]
    fn changed_shape_retains_initializer_failure_as_a_script_diagnostic() {
        let mut kontakt = translated(vec![]);
        let mut invalid = behavior(3, ir::Language::Ksp);
        invalid.source = "on init\n get_scale(0, %CC)\nend on".into();
        kontakt.instrument.behaviors.push(invalid);
        take_script_init_runs();
        let (scripts, interfaces, _) = kontakt.compile_ui(&Options::default());
        assert!(scripts.is_empty());
        assert!(interfaces.is_empty());
        assert_eq!(take_script_init_runs(), 1);
        assert!(kontakt.instrument.unsupported.iter().any(|diagnostic| {
            diagnostic.location == "script slot 3 shape slot 3 line 2"
                && diagnostic.feature == "script"
                && diagnostic.value.contains("get_scale")
        }));
    }

    #[test]
    fn same_shape_source_change_still_reports_source_identity_failure() {
        let mut kontakt = translated(vec![behavior(3, ir::Language::Ksp)]);
        kontakt.instrument.behaviors[0].source.push('\n');
        take_script_init_runs();
        let (scripts, interfaces, _) = kontakt.compile_ui(&Options::default());
        assert!(scripts.is_empty());
        assert!(interfaces.is_empty());
        assert_eq!(take_script_init_runs(), 0);
        assert!(kontakt.instrument.unsupported.iter().any(|diagnostic| {
            diagnostic.feature == "script"
                && diagnostic
                    .value
                    .contains("initialized source identity changed")
        }));
    }
}
