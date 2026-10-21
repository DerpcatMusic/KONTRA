//! Documented initial SamplePlayer/Program memory authority. This is not a
//! guessed Part inheritance rule or implementation of live parameter writes.
use roxmltree::Node;
use sampler_ir::AssetResidency;

fn flag(node: Node<'_, '_>, name: &str) -> Result<bool, String> {
    crate::record_read(node, name);
    let Some(value) = node.attribute(name) else {
        return Ok(true);
    };
    match value {
        "false" => Ok(false),
        "true" => Ok(true),
        other => match other.parse::<f64>() {
            Ok(0.0) => Ok(false),
            Ok(1.0) => Ok(true),
            _ => Err(format!(
                "unsupported {}.{name} boolean representation",
                node.tag_name().name()
            )),
        },
    }
}

/// Missing flags use the native parameter table's true default. A resident
/// requirement dominates whenever either documented permission is disabled.
pub(crate) fn sample(player: Node<'_, '_>) -> Result<AssetResidency, String> {
    if !player.has_tag_name("SamplePlayer") {
        return Err("residency policy requires a SamplePlayer".into());
    }
    let program = player
        .ancestors()
        .find(|node| node.has_tag_name("Program"))
        .ok_or("sample player lacks a Program residency authority")?;
    let oscillator_allows = flag(player, "AllowStreaming")?;
    let program_allows = flag(program, "Streaming")?;
    Ok(if oscillator_allows && program_allows {
        AssetResidency::Streamable
    } else {
        AssetResidency::Resident
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn documented_program_and_oscillator_flags_require_memory_independently() {
        for (program, oscillator, resident) in [
            ("", "", false),
            (" Streaming='1'", " AllowStreaming='1'", false),
            (" Streaming='0'", " AllowStreaming='1'", true),
            (" Streaming='1'", " AllowStreaming='0'", true),
            (" Streaming='0'", " AllowStreaming='0'", true),
        ] {
            let xml = format!(
                "<Program{program}><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer{oscillator}/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"
            );
            let doc = crate::parse_program_xml(&xml).unwrap();
            let player = doc
                .descendants()
                .find(|n| n.has_tag_name("SamplePlayer"))
                .unwrap();
            assert_eq!(
                sample(player).unwrap() == AssetResidency::Resident,
                resident
            );
        }
    }
    #[test]
    fn malformed_flags_are_not_replaced_with_streaming_defaults() {
        for value in ["nan", "0.5", "-1", "2", "", "disabled"] {
            let xml = format!("<Program><SamplePlayer AllowStreaming='{value}'/></Program>");
            let doc = crate::parse_program_xml(&xml).unwrap();
            assert!(
                sample(
                    doc.descendants()
                        .find(|n| n.has_tag_name("SamplePlayer"))
                        .unwrap()
                )
                .is_err()
            );
        }
    }
}
