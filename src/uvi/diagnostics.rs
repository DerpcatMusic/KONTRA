//! Static graph evidence only: parsing and preflight do not prove execution,
//! resource availability, audible output or native numerical fidelity.
use super::{
    playback,
    program::{NodeId, Program},
};
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct Counts {
    pub nodes: usize,
    /// Distinct parsed nodes with one or more static preflight rejections.
    pub static_rejected_nodes: usize,
    pub sample_zones: usize,
    pub connections: usize,
    pub script_processors: usize,
}

#[derive(Clone, Serialize)]
pub struct NodeReport {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub kind: String,
    /// Bounded display label only; attributes and embedded source are omitted.
    pub name: Option<String>,
    /// Source value, not current runtime bypass. None means an invalid value.
    pub initially_bypassed: Option<bool>,
    pub preflight_rejections: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct Report {
    pub parsed: bool,
    /// Whole static graph only. Admission is not initialization or activation.
    pub preflight_admitted: bool,
    pub counts: Counts,
    pub nodes: Vec<NodeReport>,
    pub runtime_evidence: &'static str,
    /// Renderer-wide limitations, not a claim that these families were used.
    pub renderer_fidelity_caveats: Vec<&'static str>,
}

/// Shared constants used by the renderer's diagnostics; none assert overall parity.
pub fn fidelity_diagnostics() -> Vec<&'static str> {
    vec![
        playback::FIDELITY_DIAGNOSTIC,
        super::dsp::FIDELITY_DIAGNOSTIC,
        super::filter::FIDELITY_DIAGNOSTIC,
        super::time_effects::FIDELITY_DIAGNOSTIC,
        super::waveshaper::FIDELITY_DIAGNOSTIC,
        super::maximizer::FIDELITY_DIAGNOSTIC,
        super::sparkverb::FIDELITY_DIAGNOSTIC,
        super::phasor::FIDELITY_DIAGNOSTIC,
        super::biquad::FIDELITY_DIAGNOSTIC,
        super::compexp::FIDELITY_DIAGNOSTIC,
        super::effects::FIDELITY_DIAGNOSTIC,
        super::exciter::FIDELITY_DIAGNOSTIC,
        super::modulation::FIDELITY_DIAGNOSTIC,
        super::generator::FIDELITY_DIAGNOSTIC,
    ]
}

/// Does not open samples, construct a renderer, run scripts or inspect voices.
/// Node IDs and parents are the retained parser graph, including XML wrappers.
pub fn report(program: &Program) -> Report {
    report_preflighted(&playback::ProgramPreflight::new(program))
}

pub(crate) fn report_preflighted(preflight: &playback::ProgramPreflight<'_>) -> Report {
    let program = preflight.program();
    let mut nodes = program
        .nodes
        .iter()
        .enumerate()
        .map(|(id, node)| NodeReport {
            id,
            parent: node.parent,
            kind: node.kind.clone(),
            name: node
                .name
                .as_ref()
                .map(|name| name.chars().filter(|c| !c.is_control()).take(160).collect()),
            initially_bypassed: match node.attributes.get("Bypass") {
                None => Some(false),
                Some(value) => match value.parse::<f64>() {
                    Ok(0.) => Some(false),
                    Ok(1.) => Some(true),
                    _ => None,
                },
            },
            preflight_rejections: Vec::new(),
        })
        .collect::<Vec<_>>();
    let rejected = preflight.unsupported();
    let preflight_admitted = rejected.is_empty();
    for rejection in rejected {
        let node = &mut nodes[rejection.node];
        node.preflight_rejections
            .push(if rejection.kind == node.kind {
                rejection.reason.clone()
            } else {
                format!("{}: {}", rejection.kind, rejection.reason)
            });
    }
    let static_rejected_nodes = nodes.iter().filter(|node| !node.preflight_rejections.is_empty()).count();
    Report {
        parsed: true,
        preflight_admitted,
        counts: Counts {
            nodes: nodes.len(),
            static_rejected_nodes,
            sample_zones: program.sample_zones.len(),
            connections: program.connections.len(),
            script_processors: program
                .nodes
                .iter()
                .filter(|node| node.kind == "ScriptProcessor")
                .count(),
        },
        nodes,
        runtime_evidence: "not_inspected",
        renderer_fidelity_caveats: fidelity_diagnostics(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uvi::program::parse_program;

    #[test]
    fn parsed_unknown_and_bypassed_unknown_processors_are_rejected() {
        for bypass in ["0", "1"] {
            let program = parse_program(&format!("<Program><Inserts><UnknownFX Name=\"Unknown\" Bypass=\"{bypass}\"/></Inserts></Program>")).unwrap();
            let report = report(&program);
            assert!(report.parsed && !report.preflight_admitted);
            assert_eq!(report.counts.static_rejected_nodes, 1);
            let node = report
                .nodes
                .iter()
                .find(|node| node.kind == "UnknownFX")
                .unwrap();
            assert_eq!(node.id, 2);
            assert_eq!(node.parent, Some(1));
            assert_eq!(node.initially_bypassed, Some(bypass == "1"));
            assert!(
                node.preflight_rejections
                    .iter()
                    .any(|reason| reason.contains("not executable"))
            );
        }
    }

    #[test]
    fn sample_admission_does_not_claim_activity_or_dump_source_and_attributes() {
        let program = parse_program(r#"<Program Name="Report"><EventProcessors><ScriptProcessor Name="Control"><script><![CDATA[private_authored_source='SECRET']]> </script></ScriptProcessor></EventProcessors><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="PRIVATE_ASSET.wav"/></Oscillators><Inserts><GainMatrix Gain_1_1="1"/></Inserts></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let report = report(&program);
        assert!(report.parsed && report.preflight_admitted);
        assert_eq!(report.counts.static_rejected_nodes, 0);
        assert_eq!(report.runtime_evidence, "not_inspected");
        assert_eq!(
            (
                report.counts.nodes,
                report.counts.sample_zones,
                report.counts.connections,
                report.counts.script_processors
            ),
            (program.nodes.len(), 1, 0, 1)
        );
        assert!(
            report
                .nodes
                .iter()
                .all(|node| node.preflight_rejections.is_empty())
        );
        let json = serde_json::to_string(&report).unwrap();
        for forbidden in [
            "private_authored_source",
            "SECRET",
            "PRIVATE_ASSET",
            "SamplePath",
            "Gain_1_1",
            "\"active\"",
            "attributes",
            "CDATA",
        ] {
            assert!(
                !json.contains(forbidden),
                "unexpected diagnostic disclosure or activity claim: {forbidden}"
            );
        }
        assert!(
            report
                .renderer_fidelity_caveats
                .contains(&playback::FIDELITY_DIAGNOSTIC)
        );
    }
}
