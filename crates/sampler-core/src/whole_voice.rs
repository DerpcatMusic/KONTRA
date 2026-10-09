//! Production adapter for the pinned v1 controls; prepared on the control thread.
use crate::v1_voice_controls::{self as v1, ControlPlan, ControlState};
use crate::voice_mod::{Inputs, ModSource};
use sampler_ir as ir;

#[derive(Clone, Copy, Default)]
pub(crate) enum Route {
    #[default]
    Residual,
    External(u16),
    Flex,
}
pub(crate) struct Program {
    pub plan: ControlPlan,
    pub routes: Vec<Route>,
    pub flex_source: Option<usize>,
}

// Original source indices are stable; route lists may differ between zones.
pub(crate) fn prepare(
    instrument: &ir::Instrument,
    zone: &ir::Zone,
    program: &crate::ModProgram,
    rate: u32,
) -> Option<Program> {
    use ir::kontakt::{
        ExternalSource as External, InternalSource as Internal, ModulationSource as Raw,
    };
    let group = zone.group?;
    let original = instrument
        .source_indices
        .groups
        .iter()
        .position(|g| *g == Some(group))
        .unwrap_or(group.0);
    let mut records: Vec<_> = instrument
        .source_indices
        .modulators
        .iter()
        .filter(|m| m.group == original)
        .collect();
    records.sort_by_key(|m| (m.external, m.slot));
    let mut owners = records
        .iter()
        .copied()
        .filter(|m| !m.external && zone.amplitude.is_some() && m.runtime == zone.amplitude);
    let amplitude = owners.next();
    if owners.next().is_some() || zone.playback.looping == ir::Looping::OneShot {
        return None;
    }
    if let Some(amplitude) = zone.amplitude {
        let ir::ModulationSource::Envelope(e) = &instrument.modulators[amplitude.0].source else {
            return None;
        };
        if e.delay != ir::Time::ZERO || e.one_shot {
            return None;
        }
    }
    let mut description = v1::ControlDescription::default();
    if let Some(amplitude) = amplitude {
        let mut raw = instrument
            .source_indices
            .ahdsrs
            .iter()
            .filter(|a| a.group == original && a.slot == amplitude.slot);
        let ahdsr = raw.next()?;
        if raw.next().is_some() {
            return None;
        }
        description.amplitude = v1::Ahdsr::from(ahdsr);
        description.native_amplitude = ahdsr.native_amplitude;
    }
    let mut source_indices = std::collections::HashMap::new();
    for &route in &zone.routes {
        let route = &instrument.routes[route.0];
        if route.target == ir::Target::Pitch
            && matches!(
                instrument.modulators[route.source.0].source,
                ir::ModulationSource::PitchBend
            )
        {
            continue;
        }
        for source in [Some(route.source), route.scale.map(|s| s.source)]
            .into_iter()
            .flatten()
        {
            let index = source_indices.len();
            source_indices.entry(source).or_insert(index);
        }
    }
    let mut assignments = Vec::new();
    let mut routed = vec![Route::Residual; program.routes.len()];
    let mut flex_source = None;
    for record in records {
        let Some(settings) = record.settings.as_ref() else {
            if amplitude.is_some_and(|a| a.slot == record.slot) && !record.external {
                continue;
            }
            return None;
        };
        match &settings.source {
            Raw::Internal { flags, source, .. } => {
                if flags[1] != 0 || settings.targets.is_empty() {
                    continue;
                }
                if amplitude.is_some_and(|a| a.slot == record.slot) {
                    if !matches!(source, Internal::Ahdsr { .. }) || !plain_volume(&settings.targets)
                    {
                        return None;
                    }
                    continue;
                }
                let Internal::Flex {
                    points, sustain, ..
                } = source
                else {
                    return None;
                };
                if !plain_volume(&settings.targets) || description.flex.is_some() {
                    return None;
                }
                description.flex = Some(v1::Flex::from_kontakt(points, *sustain).ok()?);
                let runtime = record.runtime?;
                let ir::ModulationSource::Breakpoints(current) =
                    &instrument.modulators[runtime.0].source
                else {
                    return None;
                };
                if !flex_agrees(current, points, *sustain) {
                    return None;
                }
                if amplitude.is_none()
                    && let Some(gate) = zone.amplitude
                {
                    let release = points
                        .iter()
                        .skip(*sustain as usize + 1)
                        .map(|p| p.time_ms)
                        .sum::<f32>();
                    let expected = ir::Envelope {
                        release: ir::Time::Milliseconds(f64::from(release)),
                        release_shape: ir::Curve::Step,
                        ..Default::default()
                    };
                    if instrument.modulators[gate.0].source
                        != ir::ModulationSource::Envelope(expected)
                    {
                        return None;
                    }
                }
                let source = *source_indices.get(&runtime)?;
                if !matches!(program.sources.get(source), Some(ModSource::Breakpoints(_))) {
                    return None;
                }
                // The corresponding source must have exactly one unity amplitude route.
                let matches: Vec<_> = zone
                    .routes
                    .iter()
                    .filter_map(|r| {
                        let r = &instrument.routes[r.0];
                        (r.source == runtime
                            && r.target == ir::Target::Amplitude
                            && r.depth == ir::Depth::Normalized(1.))
                        .then_some(r)
                    })
                    .collect();
                if matches.len() != 1 {
                    return None;
                }
                for (i, r) in program.routes.iter().enumerate() {
                    if r.source == source {
                        if r.target != crate::ModTarget::Attenuate
                            || r.depth != 1.
                            || r.scale.is_some()
                        {
                            return None;
                        }
                        routed[i] = Route::Flex;
                    }
                }
                flex_source = Some(source);
            }
            Raw::External { source, .. } => {
                let source = match source {
                    External::Velocity => v1::Source::Velocity,
                    External::KeyPosition => v1::Source::Key,
                    External::Constant => v1::Source::Constant,
                    External::MidiCc(cc) if *cc < 128 => v1::Source::Cc(*cc),
                    External::PitchBend => v1::Source::Bend,
                    External::MonoAftertouch | External::PolyAftertouch => v1::Source::Pressure,
                    External::Unassigned => {
                        for target in &settings.targets {
                            assignments.push(unexecuted(target));
                        }
                        continue;
                    }
                    _ => return None,
                };
                for target in &settings.targets {
                    let kind = match target.param.as_str() {
                        "volume" if target.slot.is_none() => v1::Target::Volume,
                        "pitch" if target.slot.is_none() => v1::Target::Pitch,
                        _ if target.slot.is_some() => v1::Target::Module,
                        _ => return None,
                    };
                    // Expression owns bend-to-pitch; the importer folds plain velocity into gain.
                    if source == v1::Source::Bend && kind == v1::Target::Pitch {
                        assignments.push(unexecuted(target));
                        continue;
                    }
                    if record.runtime.is_none()
                        && source == v1::Source::Velocity
                        && kind == v1::Target::Volume
                        && plain_volume(std::slice::from_ref(target))
                    {
                        assignments.push(unexecuted(target));
                        continue;
                    }
                    let runtime = record.runtime?;
                    let src = *source_indices.get(&runtime)?;
                    if !source_matches(*program.sources.get(src)?, source) {
                        return None;
                    }
                    let curve = curve(target)?;
                    if target.invert && curve.is_none() {
                        return None;
                    }
                    let module = if kind == v1::Target::Module {
                        Some(module_target(instrument, original, target)?)
                    } else {
                        None
                    };
                    let depth = target.signed_intensity();
                    let indices: Vec<_> = program
                        .routes
                        .iter()
                        .enumerate()
                        .filter_map(|(i, r)| {
                            (r.source == src
                                && match kind {
                                    v1::Target::Volume => r.target == crate::ModTarget::Attenuate,
                                    v1::Target::Pitch => r.target == crate::ModTarget::Pitch,
                                    v1::Target::Module => Some(r.target) == module,
                                    _ => false,
                                })
                            .then_some(i)
                        })
                        .collect();
                    if indices.len() != 1 {
                        return None;
                    }
                    let index = indices[0];
                    let r = program.routes[index];
                    let scale = if kind == v1::Target::Pitch { 12. } else { 1. };
                    let active_depth = (r.depth / scale) as f32;
                    let expected = scale * f64::from(active_depth);
                    // The translated route is the init/snapshot authority; admit only a lossless original-unit projection.
                    if r.depth != expected
                        || (active_depth != 0.
                            && active_depth.is_sign_negative() != depth.is_sign_negative())
                        || r.invert
                        || r.scale.is_some()
                        || r.lag
                            != (f64::from(target.lag_ms) * f64::from(rate) / 1000.).round() as u32
                        || !shape_agrees(instrument, program, r.shape, target)
                        || !zone
                            .routes
                            .iter()
                            .any(|i| instrument.routes[i.0].source == runtime)
                    {
                        return None;
                    }
                    if !matches!(routed[index], Route::Residual) {
                        return None;
                    }
                    let assignment = u16::try_from(assignments.len()).ok()?;
                    assignments.push(v1::Mod {
                        route: Some((source, kind)),
                        intensity: active_depth,
                        lag: f32::from(target.lag_ms) / 1000.,
                        curve,
                    });
                    routed[index] = Route::External(assignment);
                }
            }
        }
    }
    if amplitude.is_none() && description.flex.is_none() {
        return None;
    }
    // No authored dynamic source may escape the whole-voice owner.
    if program.sources.iter().enumerate().any(|(i, s)| {
        matches!(
            s,
            ModSource::Envelope(_) | ModSource::Lfo(_) | ModSource::Breakpoints(_)
        ) && flex_source != Some(i)
    }) {
        return None;
    }
    description.mods = v1::ModTable::new(assignments.into_boxed_slice()).ok()?;
    Some(Program {
        plan: ControlPlan::prepare(description, rate as f32).ok()?,
        routes: routed,
        flex_source,
    })
}
// Check the translator's saved projection so changed Flex geometry stays on its current runtime owner.
fn flex_agrees(current: &ir::Breakpoints, raw: &[ir::kontakt::FlexPoint], sustain: u32) -> bool {
    current.sustain == Some(sustain as usize)
        && current.points.len() == raw.len()
        && current.points.iter().zip(raw).all(|(a, b)| {
            let curve = f64::from(b.curve) - 0.5;
            let shape = if curve.abs() < 1e-4 {
                ir::Curve::Linear
            } else {
                ir::Curve::Exponential(-17. * curve)
            };
            a.time == ir::Time::Milliseconds(f64::from(b.time_ms))
                && a.level == f64::from(b.level)
                && a.shape == shape
        })
}
fn unexecuted(target: &ir::kontakt::ModulationTarget) -> v1::Mod {
    v1::Mod {
        route: None,
        intensity: target.signed_intensity(),
        lag: f32::from(target.lag_ms) / 1000.,
        curve: None,
    }
}
fn plain_volume(targets: &[ir::kontakt::ModulationTarget]) -> bool {
    matches!(targets, [t] if t.param == "volume" && t.slot.is_none() && t.signed_intensity() == 1. && !t.invert && t.lag_ms == 0 && !t.shaper.as_ref().is_some_and(|s| s.enabled))
}
fn source_matches(source: ModSource, native: v1::Source) -> bool {
    matches!(
        (source, native),
        (ModSource::Velocity, v1::Source::Velocity)
            | (ModSource::Key, v1::Source::Key)
            | (ModSource::Constant, v1::Source::Constant)
            | (ModSource::PitchBend, v1::Source::Bend)
            | (ModSource::Pressure, v1::Source::Pressure)
    ) || matches!((source,native),(ModSource::Controller(a),v1::Source::Cc(b)) if a == b)
}
pub(crate) fn read(inputs: &Inputs<'_>, source: v1::Source, _target: v1::Target) -> f32 {
    let unipolar = |raw: u32| {
        let midi = ((u64::from(raw) * 127 + u64::from(u32::MAX) / 2) / u64::from(u32::MAX)) as u8;
        let roundtrip = ((u64::from(midi) * u64::from(u32::MAX) + 63) / 127) as u32;
        if raw == roundtrip || raw == (u64::from(midi) * u64::from(u32::MAX) / 127) as u32 {
            f32::from(midi) / 127.
        } else {
            (f64::from(raw) / f64::from(u32::MAX)) as f32
        }
    };
    match source {
        v1::Source::Velocity => {
            let midi = (inputs.velocity * 127.).round();
            if inputs.velocity == midi / 127. {
                midi as f32 / 127.
            } else {
                inputs.velocity as f32
            }
        }
        v1::Source::Key => (inputs.key * 127.).round() as f32 / 127.,
        v1::Source::Constant => 1.,
        v1::Source::Cc(cc) => unipolar(inputs.controllers[cc as usize]),
        v1::Source::Pressure => unipolar(inputs.pressure),
        v1::Source::Bend => (inputs.bend as f32 + 1.) * 0.5,
        v1::Source::Counter => 0.,
    }
}

pub(crate) struct Render<'a> {
    pub plan: &'a ControlPlan,
    pub state: &'a mut ControlState,
    pub blocks: &'a std::sync::atomic::AtomicU64,
}
impl Render<'_> {
    pub fn levels(
        &mut self,
        voice: &mut crate::Voice,
        len: usize,
        held: usize,
    ) -> [f64; crate::dsp::BLOCK] {
        if len > held {
            self.blocks
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        let mut amplitude = [0.; crate::dsp::BLOCK];
        let mut flex = [0.; crate::dsp::BLOCK];
        self.state.render_levels(
            self.plan,
            &mut amplitude[held..len],
            Some(&mut flex[held..len]),
        );
        if !voice.envelope.admits_v1_controls() {
            let levels = crate::dsp::levels(voice, len, held);
            return std::array::from_fn(|i| levels[i] * f64::from(flex[i]));
        }
        voice
            .envelope
            .set_v1_amplitude(self.state.amplitude(), self.plan.rate());
        if self.state.done() {
            voice.envelope.choke(0);
        }
        std::array::from_fn(|i| f64::from(voice.gain) * f64::from(amplitude[i] * flex[i]))
    }
}

fn module_target(
    instrument: &ir::Instrument,
    group: usize,
    target: &ir::kontakt::ModulationTarget,
) -> Option<crate::ParameterAddress> {
    let (prefix, parameter) = [
        ("eqGain", "GAIN"),
        ("eqFreq", "FREQ"),
        ("eqBandwidth", "BW"),
    ]
    .into_iter()
    .find(|(p, _)| target.param.starts_with(p))?;
    let band: u8 = target.param.strip_prefix(prefix)?.parse().ok()?;
    if !(1..=3).contains(&band) {
        return None;
    }
    let parameter = format!("ENGINE_PAR_{parameter}{band}");
    let slot = i32::from(target.slot?);
    let alias = instrument.source_indices.control_aliases.iter().find(|a| {
        a.parameter == parameter && a.address.group == group as i32 && a.address.slot == slot
    })?;
    Some(crate::ParameterAddress {
        scope: crate::ParameterScope::Voice,
        node: alias.control.0 as u32,
        parameter: 0,
    })
}
// Port v1 0cb7a8a0:src/engine/params.rs::Mod::from and ni-file ShaperCurve::evaluate.
fn curve(target: &ir::kontakt::ModulationTarget) -> Option<Option<std::sync::Arc<[f32; 128]>>> {
    let Some(shaper) = target.shaper.as_ref().filter(|s| s.enabled) else {
        return Some(None);
    };
    let ir::kontakt::ShaperCurve::Table(table) = &shaper.curve else {
        return None;
    };
    if table.len() != 128 || table.iter().any(|v| !v.is_finite()) {
        return None;
    }
    Some(Some(std::sync::Arc::new(std::array::from_fn(|i| {
        let x = i as f32 / 127.;
        let position = x * 127.;
        let index = (position.floor() as usize).min(127);
        let next = (index + 1).min(127);
        table[index] + (table[next] - table[index]) * (position - index as f32)
    }))))
}
fn shape_agrees(
    _instrument: &ir::Instrument,
    program: &crate::ModProgram,
    shape: Option<usize>,
    target: &ir::kontakt::ModulationTarget,
) -> bool {
    match (shape, target.shaper.as_ref().filter(|s| s.enabled)) {
        (None, None) => true,
        (Some(index), Some(shaper)) => {
            if let ir::kontakt::ShaperCurve::Table(table) = &shaper.curve {
                let points = &program.shapes[index];
                points.len() == table.len()
                    && points
                        .iter()
                        .zip(table)
                        .enumerate()
                        .all(|(i, (&(x, y), v))| x == i as f64 / 127. && y == f64::from(*v))
            } else {
                false
            }
        }
        _ => false,
    }
}
