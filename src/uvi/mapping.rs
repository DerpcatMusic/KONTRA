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
        for (index, zone) in program.sample_zones.iter().enumerate() {
            layers.entry(zone.layer).or_default().push(index);
        }
        let layer_order = Arc::new(layers.keys().copied().collect());
        let rejections = report["nodes"].as_array().into_iter().flatten().enumerate().flat_map(|(node, entry)|
            (0..entry["preflight_rejections"].as_array().map_or(0, Vec::len)).map(move |reason| (node, reason))).collect();
        Self { stamp, zones: Arc::new(program.sample_zones.clone()), layers: Arc::new(layers), layer_order, report,
            rejections: Arc::new(rejections), channels: None }
    }

    pub(crate) fn decoded(&self, samples: &HashMap<String, Arc<Sample>>) -> Self {
        let channels = samples.iter().map(|(path, sample)| (path.clone(), sample.channels)).collect();
        Self { stamp: self.stamp, zones: self.zones.clone(), layers: self.layers.clone(), layer_order: self.layer_order.clone(), report: self.report.clone(),
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
        let decoded = parsed.decoded(&HashMap::new());
        assert!(Arc::ptr_eq(&parsed.zones, &decoded.zones));
        assert!(Arc::ptr_eq(&parsed.layers, &decoded.layers));
        assert!(Arc::ptr_eq(&parsed.layer_order, &decoded.layer_order));
        assert_eq!(parsed.layer_order.as_slice(), &[zone.layer]);
        assert!(Arc::ptr_eq(&report, &decoded.report));
        assert_eq!(decoded.channels.as_ref().unwrap().get(&zone.sample_path), None);
    }
}
