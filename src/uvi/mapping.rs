//! Immutable, local inspection of the initial Program mapping. No PCM, script
//! source or editing/playback authority crosses this publication boundary.
use super::{program::{NodeId, Program, SampleZone}, sample::Sample, worker::Stamp};
use std::{collections::{BTreeMap, HashMap}, sync::Arc};

#[derive(Debug)]
pub struct Inspection {
    pub stamp: Stamp,
    pub zones: Arc<Vec<SampleZone>>,
    pub layers: Arc<BTreeMap<NodeId, Vec<usize>>>,
    /// Sorted once at parse; UI pages index this slice without walking prior layers.
    pub layer_order: Arc<Vec<NodeId>>,
    /// Exact union of authored sampled Keygroup ranges, including edge keys.
    /// This is not a claim about MIDI input ranges or currently sounding voices.
    pub sample_keys: [bool; 128],
    pub has_scripts: bool,
    /// The same static report owner used by worker diagnostics.
    pub report: Arc<serde_json::Value>,
    /// Indices into that report, so UI work never scans the entire graph.
    pub rejections: Arc<Vec<(NodeId, usize)>>,
    /// None before the initial resource decode finishes. Channels are actual
    /// decoded dimensions; an absent path is unknown, never assumed stereo.
    pub channels: Option<Arc<HashMap<String, usize>>>,
}

impl Inspection {
    pub(crate) fn parsed(stamp: Stamp, program: &Program, report: Arc<serde_json::Value>) -> Self {
        let mut layers = BTreeMap::<NodeId, Vec<usize>>::new();
        let mut sample_keys = [false; 128];
        for (index, zone) in program.sample_zones.iter().enumerate() {
            layers.entry(zone.layer).or_default().push(index);
            sample_keys[zone.low_key as usize..=zone.high_key as usize].fill(true);
        }
        let has_scripts = program.nodes.iter().any(|node| node.kind == "ScriptProcessor");
        let layer_order = Arc::new(layers.keys().copied().collect());
        let rejections = report["nodes"].as_array().into_iter().flatten().enumerate().flat_map(|(node, entry)|
            (0..entry["preflight_rejections"].as_array().map_or(0, Vec::len)).map(move |reason| (node, reason))).collect();
        Self { stamp, zones: Arc::new(program.sample_zones.clone()), layers: Arc::new(layers), layer_order, sample_keys, has_scripts, report,
            rejections: Arc::new(rejections), channels: None }
    }

    pub fn key_span(&self) -> Option<(u8, u8)> {
        let low = self.sample_keys.iter().position(|&mapped| mapped)?;
        Some((low as u8, self.sample_keys.iter().rposition(|&mapped| mapped).unwrap_or(low) as u8))
    }

    pub(crate) fn decoded(&self, samples: &HashMap<String, Arc<Sample>>) -> Self {
        let channels = samples.iter().map(|(path, sample)| (path.clone(), sample.channels)).collect();
        Self { stamp: self.stamp, zones: self.zones.clone(), layers: self.layers.clone(), layer_order: self.layer_order.clone(), report: self.report.clone(),
            sample_keys: self.sample_keys, has_scripts: self.has_scripts,
            rejections: self.rejections.clone(),
            channels: Some(Arc::new(channels)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parsed_mapping_is_exact_and_decode_reuses_source_owners() {
        let program = super::super::program::parse_program(r#"<Program><Layers><Layer Name="Reeds"><Keygroups><Keygroup LowKey="48" HighKey="72" LowVelocity="20" HighVelocity="100"><Oscillators><SamplePlayer SamplePath="../Samples/reed.wav" BaseNote="65"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let report = Arc::new(serde_json::json!({"nodes":[]}));
        let parsed = Inspection::parsed(Stamp { epoch: 2, generation: 3, frame: 0 }, &program, report.clone());
        let zone = &parsed.zones[0];
        assert_eq!((zone.low_key, zone.high_key, zone.low_velocity, zone.high_velocity, zone.root_note), (48, 72, 20, 100, 65));
        assert_eq!(zone.sample_path, "../Samples/reed.wav");
        assert!(parsed.channels.is_none());
        assert_eq!(parsed.key_span(), Some((48, 72)));
        assert!(parsed.sample_keys[48..=72].iter().all(|&key| key));
        assert!(!parsed.sample_keys[47] && !parsed.sample_keys[73]);
        let decoded = parsed.decoded(&HashMap::new());
        assert!(Arc::ptr_eq(&parsed.zones, &decoded.zones));
        assert!(Arc::ptr_eq(&parsed.layers, &decoded.layers));
        assert!(Arc::ptr_eq(&parsed.layer_order, &decoded.layer_order));
        assert_eq!(parsed.layer_order.as_slice(), &[zone.layer]);
        assert!(Arc::ptr_eq(&report, &decoded.report));
        assert_eq!(parsed.sample_keys, decoded.sample_keys);
        assert_eq!(parsed.has_scripts, decoded.has_scripts);
        assert_eq!(decoded.channels.as_ref().unwrap().get(&zone.sample_path), None);
    }

    #[test]
    fn sample_mask_preserves_authored_edges_and_gaps_without_inferring_script_inputs() {
        let program = super::super::program::parse_program(r#"<Program><EventProcessors><ScriptProcessor><script>function onNote(e) playNote(75, e.velocity) end</script></ScriptProcessor></EventProcessors><Layers><Layer><Keygroups>
          <Keygroup LowKey="0" HighKey="24"><Oscillators><SamplePlayer SamplePath="edge-low.wav" BaseNote="12"/></Oscillators></Keygroup>
          <Keygroup LowKey="96" HighKey="127"><Oscillators><SamplePlayer SamplePath="edge-high.wav" BaseNote="108"/></Oscillators></Keygroup>
        </Keygroups></Layer></Layers></Program>"#).unwrap();
        let parsed = Inspection::parsed(Stamp { epoch: 2, generation: 3, frame: 0 }, &program, Arc::new(serde_json::json!({"nodes":[]})));
        assert_eq!(parsed.key_span(), Some((0, 127)));
        assert!(parsed.sample_keys[..=24].iter().all(|&key| key));
        assert!(parsed.sample_keys[25..96].iter().all(|&key| !key));
        assert!(parsed.sample_keys[96..].iter().all(|&key| key));
        assert!(parsed.has_scripts);
        assert!(!parsed.sample_keys[75], "script-generated notes do not manufacture sampled zones");
        let decoded = parsed.decoded(&HashMap::new());
        assert_eq!(decoded.sample_keys, parsed.sample_keys);
        assert!(decoded.has_scripts);
    }
}
