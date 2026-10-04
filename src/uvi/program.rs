//! Bounded native UVI Program graph inspection, before execution/lowering.
//!
//! The field names are format observations from locally owned preset XML.
//! Coarse/fine tuning units are also documented in UVI's Falcon manual:
//! https://uvi.s3.us-east-1.amazonaws.com/UVIFC/falcon_manual_PRINT.pdf
//! Graph retention does not imply that its processors have been implemented.

use anyhow::{Context, Result, ensure};
use roxmltree::{Document, Node, ParsingOptions};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

const XML_LIMIT: usize = super::crypto::PROGRAM_XML_LIMIT;
const NODE_LIMIT: u32 = super::crypto::PROGRAM_NODE_LIMIT;
const DEPTH_LIMIT: usize = 64;

/// IDs index `Program::nodes` and are stable for a given input document.
pub type NodeId = usize;

#[derive(Debug, Serialize)]
pub struct ProgramNode {
    pub parent: Option<NodeId>,
    pub kind: String,
    pub name: Option<String>,
    pub attributes: BTreeMap<String, String>,
    /// Script, mapper table and state content belongs to the local program;
    /// reports must not publish embedded commercial source or state.
    #[serde(skip)]
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct SignalConnection {
    pub node: NodeId,
    /// A nested connection can target another connection's Ratio parameter.
    pub owner: NodeId,
    pub source: String,
    pub destination: String,
    pub mapper: String,
    pub ratio: f64,
    pub mode: u32,
    pub bypassed: bool,
    pub inverted: bool,
}

#[derive(Debug, Serialize)]
pub struct SampleZone {
    pub player: NodeId,
    pub keygroup: NodeId,
    pub layer: NodeId,
    /// Unresolved UFS aliases and relative paths are retained verbatim.
    pub sample_path: String,
    pub low_key: u8,
    pub high_key: u8,
    pub low_velocity: u8,
    pub high_velocity: u8,
    pub low_key_fade: u8,
    pub high_key_fade: u8,
    pub low_velocity_fade: u8,
    pub high_velocity_fade: u8,
    pub root_note: u8,
    /// Local oscillator values only; ancestor gains/pans remain in the graph.
    pub gain: f64,
    pub pan: f64,
    pub coarse_semitones: f64,
    pub fine_cents: f64,
    /// Raw UVI modulation pitch and sample-start values, without guessed units.
    pub pitch: f64,
    pub sample_start: f64,
    pub note_tracking: f64,
    pub streaming: bool,
    pub purged: bool,
    pub reverse: bool,
    pub bypassed: bool,
    /// Serialized loop nodes, if present. Absence does not establish no loop:
    /// sample-file metadata can supply loops and must be read by the loader.
    pub loop_nodes: Vec<NodeId>,
}

#[derive(Debug, Serialize)]
pub struct UnsupportedModule {
    pub node: NodeId,
    pub kind: String,
    /// Still required: a script/connection may enable it during playback.
    pub initially_bypassed: bool,
}

#[derive(Debug, Serialize)]
pub struct Program {
    pub root: NodeId,
    pub nodes: Vec<ProgramNode>,
    pub layers: Vec<NodeId>,
    pub sample_zones: Vec<SampleZone>,
    pub connections: Vec<SignalConnection>,
    /// Processors not executable by this parser's sampled-zone model.
    /// A runtime must implement or reject these; never silently drop them.
    pub unsupported_modules: Vec<UnsupportedModule>,
}

fn number(node: Node<'_, '_>, name: &str, default: f64) -> Result<f64> {
    let value = node
        .attribute(name)
        .map(str::parse::<f64>)
        .transpose()
        .with_context(|| format!("Invalid UVI {} attribute {name}", node.tag_name().name()))?
        .unwrap_or(default);
    ensure!(value.is_finite(), "Nonfinite UVI attribute {name}");
    Ok(value)
}

fn integer(node: Node<'_, '_>, name: &str, default: u32, max: u32) -> Result<u32> {
    let value = node
        .attribute(name)
        .map(str::parse::<u32>)
        .transpose()
        .with_context(|| format!("Invalid UVI integer attribute {name}"))?
        .unwrap_or(default);
    ensure!(value <= max, "Out-of-range UVI attribute {name}");
    Ok(value)
}

fn flag(node: Node<'_, '_>, name: &str, default: bool) -> Result<bool> {
    Ok(integer(node, name, u32::from(default), 1)? != 0)
}

fn key(node: Node<'_, '_>, name: &str, default: u32) -> Result<u8> {
    Ok(integer(node, name, default, 127)? as u8)
}

fn ancestor<'a, 'input>(node: Node<'a, 'input>, kind: &str) -> Result<Node<'a, 'input>> {
    node.ancestors()
        .skip(1)
        .find(|n| n.has_tag_name(kind))
        .with_context(|| format!("UVI {} has no {kind} ancestor", node.tag_name().name()))
}

fn structural(kind: &str) -> bool {
    matches!(
        kind,
        "Program"
            | "Layers"
            | "Layer"
            | "Keygroups"
            | "Keygroup"
            | "Oscillators"
            | "SamplePlayer"
            | "Mappers"
            | "Inserts"
            | "Auxs"
            | "ControlSignalSources"
            | "EventProcessors"
            | "Connections"
            | "BusRouters"
            | "Chains"
            | "Properties"
            | "UserTable"
            | "script"
            | "ScriptData"
            | "state"
    )
}

/// Parses one plaintext Program or UVI4/Program without opening any resources.
/// XML entities/DTDs, huge inputs and deep graphs are rejected at the boundary.
pub fn parse_program(text: &str) -> Result<Program> {
    ensure!(
        text.len() <= XML_LIMIT,
        "UVI Program XML exceeds 32 MiB limit"
    );
    let doc = Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: NODE_LIMIT,
            ..Default::default()
        },
    )
    .context("Malformed UVI Program XML")?;
    ensure!(
        doc.descendants()
            .all(|n| n.ancestors().take(DEPTH_LIMIT + 2).count() <= DEPTH_LIMIT + 1),
        "UVI Program XML exceeds depth limit"
    );
    let document_root = doc.root_element();
    let root = if document_root.has_tag_name("Program") {
        document_root
    } else {
        ensure!(
            document_root.has_tag_name("UVI4"),
            "Expected Program or UVI4/Program XML"
        );
        let programs = document_root
            .children()
            .filter(|n| n.has_tag_name("Program"))
            .collect::<Vec<_>>();
        ensure!(programs.len() == 1, "Expected exactly one UVI Program");
        programs[0]
    };
    ensure!(
        !root.attributes().any(|a| matches!(
            a.name().to_ascii_lowercase().as_str(),
            "password" | "passwordv2"
        )),
        "Encrypted UVI Program must be decoded before graph parsing"
    );
    let mut program = Program {
        root: 0,
        nodes: Vec::new(),
        layers: Vec::new(),
        sample_zones: Vec::new(),
        connections: Vec::new(),
        unsupported_modules: Vec::new(),
    };
    let mut ids = HashMap::new();
    for node in root.descendants().filter(Node::is_element) {
        let id = program.nodes.len();
        let parent = node.parent().and_then(|p| ids.get(&p.id()).copied());
        ids.insert(node.id(), id);
        let kind = node.tag_name().name();
        let attributes = node
            .attributes()
            .filter(|a| {
                let key = a.name().to_ascii_lowercase();
                !key.contains("password") && !key.contains("secret")
            })
            .map(|a| (a.name().to_owned(), a.value().to_owned()))
            .collect();
        program.nodes.push(ProgramNode {
            parent,
            kind: kind.into(),
            name: node.attribute("Name").map(str::to_owned),
            attributes,
            text: node
                .children()
                .filter(Node::is_text)
                .filter_map(|n| n.text())
                .collect(),
        });
        if kind == "Layer" {
            program.layers.push(id);
        }
        if !structural(kind) {
            program.unsupported_modules.push(UnsupportedModule {
                node: id,
                kind: kind.into(),
                initially_bypassed: flag(node, "Bypass", false)?,
            });
        }
        if kind == "SignalConnection" {
            let owner = node
                .ancestors()
                .skip(1)
                .find(|n| n.is_element() && !n.has_tag_name("Connections"))
                .context("UVI connection has no owner")?;
            let source = node
                .attribute("Source")
                .context("UVI connection missing Source")?;
            let destination = node
                .attribute("Destination")
                .context("UVI connection missing Destination")?;
            ensure!(
                !source.is_empty() && !destination.is_empty(),
                "UVI connection endpoint is empty"
            );
            program.connections.push(SignalConnection {
                node: id,
                owner: ids[&owner.id()],
                source: source.into(),
                destination: destination.into(),
                mapper: node.attribute("Mapper").unwrap_or_default().into(),
                ratio: number(node, "Ratio", 1.)?,
                mode: integer(node, "ConnectionMode", 0, u32::MAX)?,
                bypassed: flag(node, "Bypass", false)?,
                inverted: flag(node, "Inverted", false)?,
            });
        }
        if kind == "SamplePlayer" {
            let group = ancestor(node, "Keygroup")?;
            let layer = ancestor(node, "Layer")?;
            let path = node
                .attribute("SamplePath")
                .context("UVI SamplePlayer missing SamplePath")?;
            ensure!(!path.is_empty(), "UVI SamplePlayer has empty SamplePath");
            let low_key = key(group, "LowKey", 0)?;
            let high_key = key(group, "HighKey", 127)?;
            let low_velocity = key(group, "LowVelocity", 1)?;
            let high_velocity = key(group, "HighVelocity", 127)?;
            ensure!(
                low_key <= high_key && low_velocity <= high_velocity,
                "Inverted UVI sample zone range"
            );
            program.sample_zones.push(SampleZone {
                player: id,
                keygroup: ids[&group.id()],
                layer: ids[&layer.id()],
                sample_path: path.into(),
                low_key,
                high_key,
                low_velocity,
                high_velocity,
                low_key_fade: key(group, "LowKeyFade", 0)?,
                high_key_fade: key(group, "HighKeyFade", 0)?,
                low_velocity_fade: key(group, "LowVelocityFade", 0)?,
                high_velocity_fade: key(group, "HighVelocityFade", 0)?,
                root_note: key(node, "BaseNote", 60)?,
                gain: number(node, "Gain", 1.)?,
                pan: number(node, "Pan", 0.)?,
                coarse_semitones: number(node, "CoarseTune", 0.)?,
                fine_cents: number(node, "FineTune", 0.)?,
                pitch: number(node, "Pitch", 0.)?,
                sample_start: number(node, "SampleStart", 0.)?,
                note_tracking: number(node, "NoteTracking", 1.)?,
                streaming: flag(node, "AllowStreaming", true)?,
                purged: flag(node, "SamplePurged", false)?,
                reverse: flag(node, "Reverse", false)?,
                bypassed: flag(node, "Bypass", false)?,
                loop_nodes: Vec::new(),
            });
        }
    }
    // Link serialized loop nodes only after all node IDs have been assigned.
    for zone in &mut program.sample_zones {
        for id in (zone.player + 1)..program.nodes.len() {
            let mut parent = program.nodes[id].parent;
            let mut inside = false;
            while let Some(p) = parent {
                if p == zone.player {
                    inside = true;
                    break;
                }
                parent = program.nodes[p].parent;
            }
            if !inside {
                break;
            }
            if program.nodes[id].kind.to_ascii_lowercase().contains("loop") {
                zone.loop_nodes.push(id);
            }
        }
    }
    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_program_graph_retains_mapping_and_rejects_invalid_inputs() {
        let xml = r#"<Program Name="Test" Gain="0.5" SecretToken="private-test">
          <Layers><Layer Name="Layer A" LowKey="40" HighKey="90"><Keygroups>
            <Keygroup Name="Zone A" LowKey="30" HighKey="60" LowVelocity="20" HighVelocity="100" Pan="0.25">
              <Oscillators><SamplePlayer Name="Mic A" SamplePath="$Fixture/a.flac" BaseNote="48" Gain="0.75" FineTune="25">
                <Connections><SignalConnection Name="Gain control" Source="@MIDI CC 1" Destination="Gain" Ratio="0.5">
                  <Connections><SignalConnection Name="Nested" Source="$Program/Control" Destination="Ratio"/></Connections>
                </SignalConnection></Connections>
              </SamplePlayer><SamplePlayer Name="Mic B" SamplePath="b.flac" Bypass="1"/></Oscillators>
              <Inserts><UnimplementedFilter Name="Filter" Bypass="1"/></Inserts>
            </Keygroup>
          </Keygroups></Layer></Layers>
          <EventProcessors><ScriptProcessor Name="Script"><script>-- authored test</script></ScriptProcessor></EventProcessors>
        </Program>"#;
        let graph = parse_program(xml).unwrap();
        assert_eq!(graph.layers.len(), 1);
        assert_eq!(graph.sample_zones.len(), 2);
        let zone = &graph.sample_zones[0];
        assert_eq!(
            (
                zone.low_key,
                zone.high_key,
                zone.low_velocity,
                zone.high_velocity
            ),
            (30, 60, 20, 100)
        );
        assert_eq!(
            (zone.root_note, zone.gain, zone.fine_cents),
            (48, 0.75, 25.)
        );
        assert_eq!(zone.sample_path, "$Fixture/a.flac");
        assert_eq!(graph.nodes[zone.keygroup].attributes["Pan"], "0.25");
        assert_eq!(graph.connections[0].owner, zone.player);
        assert_eq!(graph.connections[1].owner, graph.connections[0].node);
        assert!(graph.sample_zones[1].bypassed);
        assert!(
            graph
                .unsupported_modules
                .iter()
                .any(|m| m.kind == "UnimplementedFilter" && m.initially_bypassed)
        );
        assert!(!graph.nodes[0].attributes.contains_key("SecretToken"));
        let report = serde_json::to_string(&graph).unwrap();
        assert!(!report.contains("private-test") && !report.contains("authored test"));
        assert!(parse_program("<Program>").is_err());
        assert!(parse_program("<Program PasswordV2=\"encoded\"/>").is_err());
        assert!(parse_program("<Program Password=\"encoded\"/>").is_err());
        assert!(
            parse_program(&format!(
                "<Program>{}{}</Program>",
                "<Container>".repeat(64),
                "</Container>".repeat(64)
            ))
            .is_err()
        );
        assert!(parse_program("<!DOCTYPE Program [<!ENTITY x 'bad'>]><Program/>").is_err());
        assert!(parse_program(&xml.replace("Gain=\"0.75\"", "Gain=\"NaN\"")).is_err());
        assert!(parse_program(&xml.replace("LowVelocity=\"20\"", "LowVelocity=\"128\"")).is_err());
        assert!(parse_program(&xml.replace("HighKey=\"60\"", "HighKey=\"20\"")).is_err());
        assert!(parse_program("<UVI4><Program/><Program/></UVI4>").is_err());
    }
    #[test]
    fn authored_large_xml_stays_bounded_without_dropping_whitespace_or_nodes() {
        let xml = format!("<Program><!--{}--></Program>", "x".repeat(16 << 20));
        assert!(parse_program(&xml).is_ok());
        let xml = format!(
            "<Program>{}</Program>",
            "<Properties>\n<x/>\n</Properties>\n".repeat(40_000)
        );
        assert_eq!(parse_program(&xml).unwrap().nodes.len(), 80_001);
        let excessive_nodes = format!("<Program>{}</Program>", "<x/>\n".repeat(125_000));
        assert!(parse_program(&excessive_nodes).is_err());
        assert!(parse_program(&"x".repeat(XML_LIMIT + 1)).is_err());
    }

}
