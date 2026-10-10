//! Typed forward descriptors; no unit conversion, filtering, or DSP admission.
use ni_file::kontakt::objects as ni;
use sampler_ir::kontakt as ir;

pub(crate) fn internal(version: u16, p: &ni::InternalModParams) -> ir::Modulation {
    let source = match &p.modulator {
        ni::Modulator::Ahdsr(e) => ir::InternalSource::Ahdsr {
            attack_curve: e.attack_curve,
            attack_ms: e.attack_ms,
            hold_ms: e.hold_ms,
            decay_ms: e.decay_ms,
            sustain: e.sustain,
            release_ms: e.release_ms,
            ahd_only: e.unknown_flag,
            unknown_tail: e.unknown_tail.clone(),
        },
        ni::Modulator::Flex(e) => ir::InternalSource::Flex {
            points: e
                .points
                .iter()
                .map(|p| ir::FlexPoint {
                    time_ms: p.time_ms,
                    level: p.level,
                    curve: p.curve,
                })
                .collect(),
            sustain: e.sustain,
            unknown_index: e.unknown_index,
            unknown_tail: e.unknown_tail.clone(),
        },
        ni::Modulator::Lfo(l) => ir::InternalSource::Lfo(ir::Lfo {
            structured: l.structured,
            version: l.version,
            waveform: l.waveform,
            initial_values: l.initial_values,
            records: std::array::from_fn(|i| ir::LfoRecord {
                flag: l.records[i].flag,
                values: l.records[i].values,
            }),
            trailing_flag: l.trailing_flag,
            trailing_values: l.trailing_values,
            additional_flag: l.additional_flag,
        }),
        ni::Modulator::Other { chunk_id } => ir::InternalSource::Other {
            chunk_id: *chunk_id,
        },
    };
    ir::Modulation {
        version,
        source: ir::ModulationSource::Internal {
            flags: p.unknown_flags,
            unknown_id: p.unknown_id,
            source,
        },
        targets: p.targets.iter().map(target).collect(),
    }
}

pub(crate) fn external(version: u16, p: &ni::ExternalModParams) -> ir::Modulation {
    let source = match p.source {
        ni::ModSource::PitchBend => ir::ExternalSource::PitchBend,
        ni::ModSource::PolyAftertouch => ir::ExternalSource::PolyAftertouch,
        ni::ModSource::MonoAftertouch => ir::ExternalSource::MonoAftertouch,
        ni::ModSource::MidiCc(cc) => ir::ExternalSource::MidiCc(cc),
        ni::ModSource::KeyPosition => ir::ExternalSource::KeyPosition,
        ni::ModSource::Velocity => ir::ExternalSource::Velocity,
        ni::ModSource::ReleaseVelocity => ir::ExternalSource::ReleaseVelocity,
        ni::ModSource::ReleaseTriggerCounter => ir::ExternalSource::ReleaseTriggerCounter,
        ni::ModSource::Constant => ir::ExternalSource::Constant,
        ni::ModSource::RandomUnipolar => ir::ExternalSource::RandomUnipolar,
        ni::ModSource::RandomBipolar => ir::ExternalSource::RandomBipolar,
        ni::ModSource::Script(id) => ir::ExternalSource::Script(id),
        ni::ModSource::Unassigned => ir::ExternalSource::Unassigned,
    };
    ir::Modulation {
        version,
        source: ir::ModulationSource::External {
            source,
            unknown_id: p.unknown_id,
            unknown_source_data: p.unknown_source_data.clone(),
            unknown_tail: p.unknown_tail.clone(),
        },
        targets: p.targets.iter().map(target).collect(),
    }
}

fn target(t: &ni::ModTarget) -> ir::ModulationTarget {
    ir::ModulationTarget {
        param: t.param.clone(),
        intensity: t.intensity,
        lag_ms: t.lag_ms,
        name: t.name.clone(),
        slot: t.slot,
        invert: t.invert,
        shaper: t.shaper.as_ref().map(|s| ir::ModulationShaper {
            enabled: s.enabled,
            curve: match &s.curve {
                ni::ShaperCurve::Table(t) => ir::ShaperCurve::Table(t.clone()),
                ni::ShaperCurve::Breakpoints(p) => ir::ShaperCurve::Breakpoints(
                    p.iter()
                        .map(|p| ir::ShaperPoint {
                            x: p.x,
                            y: p.y,
                            curve: p.curve,
                        })
                        .collect(),
                ),
            },
        }),
        unknown_i16: t.unknown_i16,
        flags: t.unknown_flags,
    }
}
