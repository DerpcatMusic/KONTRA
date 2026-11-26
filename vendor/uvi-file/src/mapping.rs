//! Pure UVI XML mapping parser for `Element:loadMapping(path)`.
//!
//! Reads `<SamplePlayer>` / `<Zone>` elements inside UVI mapping XML or `.uvip`
//! fragments and extracts key/velocity ranges, root note, tuning, gain, start
//! offset, reverse flag, and sample path references.

use roxmltree::Node;

/// One sample zone extracted from a UVI XML mapping file.
#[derive(Clone, Debug, PartialEq)]
pub struct MappingEntry {
    pub sample_path: String,
    pub low_key: u8,
    pub high_key: u8,
    pub low_velocity: u8,
    pub high_velocity: u8,
    pub base_note: u8,
    pub tune: f64,
    pub gain: f32,
    pub start: u64,
    pub reverse: bool,
}

fn attr_f(node: Node<'_, '_>, key: &str, default: f64) -> f64 {
    node.attribute(key)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(default)
}

fn attr_u8(node: Node<'_, '_>, key: &str, default: u8) -> u8 {
    attr_f(node, key, f64::from(default))
        .round()
        .clamp(0.0, 127.0) as u8
}

/// Parses a UVI XML mapping document (`.xml`, `.uvim`, or `.uvip`) into raw
/// [`MappingEntry`] descriptors.
#[must_use]
pub fn parse_mapping_xml(text: &str) -> Vec<MappingEntry> {
    let Ok(doc) = crate::parse_program_xml(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for node in doc.descendants().filter(Node::is_element) {
        if !matches!(node.tag_name().name(), "SamplePlayer" | "Zone") {
            continue;
        }
        let Some(sample_path) = node
            .attribute("SamplePath")
            .or_else(|| node.attribute("File"))
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let kg = node
            .ancestors()
            .find(|a| a.has_tag_name("Keygroup"))
            .unwrap_or(node);
        let lo_k = attr_u8(node, "LowKey", attr_u8(kg, "LowKey", 0));
        let hi_k = attr_u8(node, "HighKey", attr_u8(kg, "HighKey", 127));
        let lo_v = attr_u8(node, "LowVelocity", attr_u8(kg, "LowVelocity", 1)).max(1);
        let hi_v = attr_u8(node, "HighVelocity", attr_u8(kg, "HighVelocity", 127)).max(lo_v);
        let base_note = attr_u8(node, "BaseNote", attr_u8(node, "RootKey", 60));
        let tune = attr_f(node, "CoarseTune", 0.0)
            + attr_f(node, "FineTune", 0.0) * 0.01
            + attr_f(node, "Pitch", 0.0);
        let gain = (attr_f(node, "Gain", 1.0) as f32).clamp(0.0, 16.0);
        let start = attr_f(node, "SampleStart", 0.0).max(0.0) as u64;
        let reverse = node
            .attribute("Reverse")
            .is_some_and(|v| v.trim() == "1" || v.eq_ignore_ascii_case("true"));
        out.push(MappingEntry {
            sample_path: sample_path.to_owned(),
            low_key: lo_k.min(hi_k),
            high_key: lo_k.max(hi_k),
            low_velocity: lo_v,
            high_velocity: hi_v,
            base_note,
            tune,
            gain,
            start,
            reverse,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::parse_mapping_xml;

    #[test]
    fn parses_keygroup_and_zone_mapping_attributes() {
        let xml = r#"
<Program>
  <Layers><Layer><Keygroups>
    <Keygroup LowKey="36" HighKey="59" LowVelocity="1" HighVelocity="100">
      <Oscillators>
        <SamplePlayer SamplePath="Samples/Bass.wav" BaseNote="48" CoarseTune="-2" FineTune="25" Gain="0.8" SampleStart="128" Reverse="1"/>
      </Oscillators>
    </Keygroup>
    <Zone File="Samples/Lead.wav" LowKey="60" HighKey="96" RootKey="72"/>
  </Keygroups></Layer></Layers>
</Program>"#;
        let entries = parse_mapping_xml(xml);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].sample_path, "Samples/Bass.wav");
        assert_eq!((entries[0].low_key, entries[0].high_key), (36, 59));
        assert_eq!(
            (entries[0].low_velocity, entries[0].high_velocity),
            (1, 100)
        );
        assert_eq!(entries[0].base_note, 48);
        assert!((entries[0].tune - (-1.75)).abs() < 1e-6);
        assert!((entries[0].gain - 0.8).abs() < 1e-6);
        assert_eq!(entries[0].start, 128);
        assert!(entries[0].reverse);
        assert_eq!(entries[1].sample_path, "Samples/Lead.wav");
        assert_eq!((entries[1].low_key, entries[1].high_key), (60, 96));
        assert_eq!(entries[1].base_note, 72);
    }
}
