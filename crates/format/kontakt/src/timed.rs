//! Timed modulation translation and its law provenance.

use kontra_format::Notes;
use kontra_ir as ir;
use ni_file::kontakt::objects::{InternalModParams, Modulator};

use crate::laws;

/// Records the selected law and its input and output when tracing is enabled.
fn trace_timed(
    id: &'static str,
    group: usize,
    slot: u16,
    field: &str,
    input: &impl std::fmt::Debug,
    output: &impl std::fmt::Debug,
) {
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId(id),
        location: format!("groups/{group}/routes/{slot}/{field}"),
        input: format!("{input:?}"),
        output: format!("{output:?}"),
    });
}

/// Routes a timed envelope or LFO and reports unmapped targets and cutoff slots.
pub(crate) fn timed(
    out: &mut ir::Group,
    p: &InternalModParams,
    group_index: usize,
    slot: u16,
    notes: &mut Notes,
    unmapped: &mut Vec<u8>,
) {
    // The modulator's flags: [router open, bypass, retrigger, unknown].
    const BYPASS: usize = 1;
    const RETRIGGER: usize = 2;
    let report = |notes: &mut Notes, unmapped: &mut Vec<u8>, what: &str| {
        for t in &p.targets {
            if t.param == "filterCutoff" {
                unmapped.extend(t.slot);
            }
            let feature = format!("internal modulator {what} -> {}", t.param);
            notes.add(ir::Impact::Audible, &feature, &p.name);
        }
    };
    let mut envelope = |e: ir::Breakpoints| {
        let n = u8::try_from(out.envelopes.len()).ok()?;
        out.envelopes.push(e);
        Some(ir::Source::Envelope(n))
    };
    let (source, what) = match &p.modulator {
        Modulator::Flex(e) => {
            let result = laws::flex(e);
            trace_timed(
                "kontakt.flex_envelope.v1",
                group_index,
                slot,
                "envelope",
                e,
                &result,
            );
            (envelope(result), "flex envelope")
        }
        Modulator::Ahdsr(e) => {
            let result = laws::ahdsr(e);
            trace_timed(
                "kontakt.breakpoint_ahdsr.v1",
                group_index,
                slot,
                "envelope",
                e,
                &result,
            );
            (envelope(result), "AHDSR")
        }
        Modulator::Lfo(l) => {
            let clock = if p.unknown_flags.get(RETRIGGER).is_some_and(|&f| f != 0) {
                ir::Clock::Voice
            } else {
                ir::Clock::Free
            };
            let result = laws::lfo(l, clock);
            trace_timed(
                "kontakt.lfo.v1",
                group_index,
                slot,
                "lfo",
                &(l, clock),
                &result,
            );
            let (Some(lfo), Ok(n)) = (result, u8::try_from(out.lfos.len())) else {
                return report(notes, unmapped, "LFO (synced fade or unknown wave)");
            };
            out.lfos.push(lfo);
            (Some(ir::Source::Lfo(n)), "LFO")
        }
        _ => return,
    };
    let Some(source) = source else {
        return report(notes, unmapped, what);
    };
    let flex = what == "flex envelope";
    let amp = out
        .mods
        .iter()
        .any(|m| matches!(m.kind, ir::ModKind::Envelope));
    for (target, t) in p.targets.iter().enumerate() {
        let kind = match (t.param.as_str(), t.slot) {
            // An LFO's volume route is linear in its unipolar swing (v2
            // sampler-core v1_voice_controls volume target; the range conversion is not yet isolated).
            ("volume", None) => ir::ModKind::Volume {
                source,
                intensity: t.signed_intensity(),
                curve: ir::Curve::Depth { power: 1 },
                // ponytail: an LFO or envelope route's lag is not read (only
                // controller routes are measured); read it if one is saved.
                tau_ms: 0.0,
            },
            ("pitch", None) => ir::ModKind::Pitch {
                source,
                semitones: laws::PITCH_SEMITONES,
                depth: t.signed_intensity(),
                tau_ms: 0.0,
            },
            ("pan", None) => ir::ModKind::Pan {
                source,
                amount: t.signed_intensity(),
            },
            ("filterCutoff", Some(insert)) => ir::ModKind::Cutoff {
                source,
                insert,
                depth: t.signed_intensity(),
            },
            ("filterQ", Some(insert)) => ir::ModKind::Resonance {
                source,
                insert,
                depth: t.signed_intensity(),
            },
            (param, Some(insert))
                if crate::provenance::eq_band_index(
                    param,
                    group_index,
                    usize::from(slot),
                    target,
                )
                .is_some() =>
            {
                let result = laws::eq_gain(source, insert, param, t.signed_intensity());
                kontra_law::record(|| kontra_law::Application {
                    id: kontra_law::LawId("kontakt.eq_gain_route.v1"),
                    location: format!("groups/{group_index}/routes/{slot}/targets/{target}"),
                    input: format!("{:?}", (source, insert, param, t.signed_intensity())),
                    output: format!("{result:?}"),
                });
                match result {
                    Some(kind) => kind,
                    None => continue,
                }
            }
            (param, Some(insert))
                if let Some(kind) =
                    crate::drive::route(source, insert, param, t.signed_intensity()) =>
            {
                kind
            }
            _ => {
                let to = if t.slot.is_some() { "module " } else { "" };
                let feature = format!("internal modulator {what} -> {to}{}", t.param);
                notes.add(ir::Impact::Audible, &feature, &p.name);
                continue;
            }
        };
        if flex
            && !amp
            && matches!(kind, ir::ModKind::Volume { .. })
            && let Some(e) = out.envelopes.last()
        {
            out.envelope = laws::flex_gate(e);
            trace_timed(
                "kontakt.flex_gate.v1",
                group_index,
                slot,
                "amp_envelope",
                e,
                &out.envelope,
            );
        }
        out.mods.push(ir::Mod {
            name: p.name.clone(),
            slot,
            target: u16::try_from(target).unwrap_or(u16::MAX),
            bypass: p.unknown_flags.get(BYPASS).is_some_and(|&f| f != 0),
            kind,
        });
    }
}
