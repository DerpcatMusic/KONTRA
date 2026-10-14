use sampler_ui_ir::{Binding, Kind, Orientation, UviMeterScope};
use sampler_uvi::script::{Config, ScriptHost};

fn xml(script: &str) -> String {
    format!(
        "<UVI4><Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer/></Oscillators></Keygroup></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    )
}

#[test]
fn audio_meter_keeps_orientation_bus_point_and_exact_promoted_xml_identity() {
    let source = xml(
        "local layer=Program.layers[1] local kg=layer.keygroups[1] local osc=kg.oscillators[1] AudioMeter('program',Program,false,2,false) local m=AudioMeter('layer',layer,true,0,true) m.displayAsMono=true AudioMeter('keygroup',kg,false,0,true) AudioMeter('oscillator',osc,true,3,false)",
    );
    let host = ScriptHost::new(&source, (), Config::default()).unwrap();
    assert!(host.fault_counts().init.is_empty());
    let face = host.interface();
    let doc = roxmltree::Document::parse(&source).unwrap();
    let scopes = [
        UviMeterScope::Program,
        UviMeterScope::Layer { ordinal: 1 },
        UviMeterScope::Keygroup,
        UviMeterScope::Oscillator,
    ];
    for (index, tag) in ["Program", "Layer", "Keygroup", "SamplePlayer"]
        .iter()
        .enumerate()
    {
        let widget = &face.widgets[index];
        let request = widget.uvi_meter_request.expect("exact request retained");
        assert_eq!(request.element_id, index as u32);
        assert_eq!(
            request.element_node,
            doc.descendants()
                .find(|n| n.has_tag_name(*tag))
                .unwrap()
                .id()
                .get_usize() as u32
        );
        assert_ne!(
            request.element_id, request.element_node,
            "promoted ordinal is not an XML NodeId"
        );
        assert_eq!(request.scope, scopes[index]);
        assert_eq!(request.bus_id, [2, 0, 0, 3][index]);
        assert_eq!(request.use_output, [false, true, true, false][index]);
        assert_eq!(request.display_as_mono, index == 1);
        assert!(
            matches!(widget.kind, Kind::LevelMeter {orientation} if orientation == if index%2==0 {Orientation::Horizontal}else{Orientation::Vertical})
        );
        assert_eq!(
            widget.binding,
            Binding::None,
            "unresolved tap must not use Program output"
        );
    }
    assert_eq!(
        face.unsupported
            .iter()
            .filter(|u| u.feature == "AudioMeter element")
            .count(),
        4
    );
    assert!(
        face.unsupported
            .iter()
            .any(|u| u.feature == "AudioMeter.displayAsMono"),
        "unverified sum law remains diagnosed"
    );
}

#[test]
fn malformed_or_fabricated_meter_requests_remain_unavailable() {
    let host = ScriptHost::new(&xml("AudioMeter('missing',{},true,0,true) AudioMeter('fabricated',{__id=0,type='Program'},true,0,true) AudioMeter('negative',Program,true,-1,true) AudioMeter('fractional',Program,true,0.5,true) AudioMeter('overflow',Program,true,4294967296,true) local m=AudioMeter('mono',Program,true,0,true) m.displayAsMono='true' local n=AudioMeter('point',Program,true,0,true) n.useOutput='true'"), (), Config::default()).unwrap();
    let face = host.interface();
    assert!(host.fault_counts().init.is_empty());
    assert_eq!(face.widgets.len(), 7);
    assert!(
        face.widgets
            .iter()
            .all(|w| w.uvi_meter_request.is_none() && w.binding == Binding::None)
    );
    assert_eq!(
        face.unsupported
            .iter()
            .filter(|u| u.feature == "AudioMeter element")
            .count(),
        7
    );
}
