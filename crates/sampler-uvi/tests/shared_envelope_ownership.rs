//! Safety of source qualification at real shared output consumers.
//! These do not turn the existing AHD functional positives into refusal tests.

use sampler_ir as ir;
use std::path::Path;

fn xml(sources: &str, program_connections: &str, keygroup_sources: &str, connections: &str) -> String {
    format!(r#"<Program Name="Qualification"><ControlSignalSources>{sources}</ControlSignalSources>
      <Connections>{program_connections}</Connections><Layers><Layer Name="L"><Keygroups>
      <Keygroup Name="K"><ControlSignalSources>{keygroup_sources}</ControlSignalSources>
      <Connections>{connections}</Connections><Oscillators><SamplePlayer Name="P"/></Oscillators>
      </Keygroup></Keygroups></Layer></Layers></Program>"#)
}

fn refused(text: &str) -> String {
    match sampler_uvi::translate(text, Path::new(".")) {
        Err(sampler_uvi::Translate::Invalid(reason)) => reason,
        Err(other) => panic!("expected semantic refusal, got {other}"),
        Ok(_) => panic!("an unqualified output source was admitted"),
    }
}

fn ahd_boundary(reason: &str) {
    assert!(reason.contains("shared output source node"), "{reason}");
    assert!(reason.ends_with("AHD note-off release is unproved"), "{reason}");
}

#[test]
fn ahd_output_qualification_survives_non_output_cache_fill_and_order() {
    let source = r#"<AHD Name="Steps"/>"#;
    let pitch = r#"<SignalConnection Source="$Program/Steps" Destination="Pitch" Ratio="0.1"/>"#;
    let gain = r#"<SignalConnection Source="$Program/Steps" Destination="Gain" Ratio="0.5"/>"#;
    // The normal Program source-domain connection runs before the KG output.
    // Before the correction it consumes both warnings and primes the cache.
    for (prefix, connections) in [
        (pitch, gain.to_owned()),
        ("", gain.to_owned()),
        ("", format!("{pitch}{gain}")),
        ("", format!("{gain}{pitch}")),
    ] {
        ahd_boundary(&refused(&xml(source, prefix, "", &connections)));
    }
}

#[test]
fn cached_ahd_pan_has_the_same_missing_law_as_gain() {
    let source = r#"<AHD Name="Steps"/>"#;
    let pitch = r#"<SignalConnection Source="$Program/Steps" Destination="Pitch" Ratio="0.1"/>"#;
    for destination in ["Gain", "Pan"] {
        let output = format!(r#"<SignalConnection Source="$Program/Steps" Destination="{destination}" Ratio="0.5"/>"#);
        ahd_boundary(&refused(&xml(source, pitch, "", &output)));
    }
}

#[test]
fn cached_ratio_dependency_cannot_hide_the_ahd_limitation() {
    let source = r#"<AHD Name="Steps"/>"#;
    let pitch = r#"<SignalConnection Source="$Program/Steps" Destination="Pitch" Ratio="0.1"/>"#;
    let output = r#"<SignalConnection Source="@MIDI CC 1" Destination="Gain" Ratio="0.5">
      <Connections><SignalConnection Source="$Program/Steps" Destination="Ratio" Ratio="0.5"/></Connections>
      </SignalConnection>"#;
    for prefix in [pitch, ""] {
        ahd_boundary(&refused(&xml(source, prefix, "", output)));
    }
}

#[test]
fn cached_depth_dependency_omits_the_unsupported_composite_output_and_reports_it() {
    let source = r#"<AHD Name="Steps"/><LFO Name="Composite" Freq="3" Retrigger="0">
      <Connections><SignalConnection Source="$Program/Steps" Destination="Depth" Ratio="0.5"/></Connections></LFO>"#;
    let pitch = r#"<SignalConnection Source="$Program/Steps" Destination="Pitch" Ratio="0.1"/>"#;
    let output = r#"<SignalConnection Source="$Program/Composite" Destination="Gain" Ratio="0.5"/>"#;
    for prefix in [pitch, ""] {
        let instrument = sampler_uvi::translate(&xml(source, prefix, "", output), Path::new(".")).unwrap().instrument;
        instrument.validate().unwrap();
        assert!(instrument.unsupported.iter().any(|u| u.location.contains("SignalConnection")), "{:?}", instrument.unsupported);
        assert!(instrument.source_indices.processing.iter().all(|recipe| recipe.routes.is_empty()), "no exact output route");
    }
}

#[test]
fn plain_dahdsr_retains_the_real_gate_and_its_velocity_law_after_cache_fill() {
    let sources = r#"<DAHDSR Name="Amp" AttackTime="0.01" DecayTime="0.2" SustainLevel="0.5"
      ReleaseTime="0.3" VelocityAmount="1" VelocitySens="0.5" DecayCurve="0.5"/>"#;
    let pitch = r#"<SignalConnection Source="$Program/Amp" Destination="Pitch" Ratio="0.1"/>"#;
    let gain = r#"<SignalConnection Source="$Program/Amp" Destination="Gain" Ratio="1"/>"#;
    for prefix in [pitch, ""] {
        let instrument = sampler_uvi::translate(&xml(sources, prefix, "", gain), Path::new(".")).unwrap().instrument;
        instrument.validate().unwrap();
        assert_eq!(instrument.source_indices.processing.len(), 1);
        let recipe = &instrument.source_indices.processing[0];
        let gate = recipe.source_gate.expect("one real plain DAHDSR output gate");
        assert_eq!(gate.velocity, ir::VelocityResponse::Power(2.));
        assert_eq!(recipe.members, vec![ir::ZoneRef(0)]);
        assert!(recipe.sources.contains(&gate.source));
        assert!(recipe.routes.contains(&gate.route));
        assert!(instrument.zones[0].amplitude.is_none(), "no physical envelope copy");
        assert_eq!(instrument.zones[0].velocity, ir::VelocityResponse::None);
        assert!(!instrument.zones[0].routes.contains(&gate.route));
        let route = &instrument.routes[gate.route.0];
        assert_eq!((route.source, route.target, route.depth), (gate.source, ir::Target::Amplitude, ir::Depth::Normalized(1.)));
        let ir::ModulationSource::Envelope(env) = instrument.modulators[gate.source.0].source else { panic!("actual envelope source"); };
        assert_eq!((env.attack, env.decay, env.sustain, env.release),
            (ir::Time::Seconds(0.01), ir::Time::Seconds(0.2), 0.5, ir::Time::Seconds(0.3)));
        assert_eq!(env.decay_shape, ir::Curve::Exponential(2. * 3f64.ln()));
    }
}

#[test]
fn cached_partial_dahdsr_is_not_requalified_as_a_plain_gate() {
    let source = r#"<DAHDSR Name="Amp" VelocityAmount="0.5" VelocitySens="0.5"/>"#;
    let pitch = r#"<SignalConnection Source="$Program/Amp" Destination="Pitch" Ratio="0.1"/>"#;
    let output = r#"<SignalConnection Source="$Program/Amp" Destination="Gain" Ratio="1"/>"#;
    for prefix in [pitch, ""] {
        let reason = refused(&xml(source, prefix, "", output));
        assert!(reason.contains("shared output source node"), "{reason}");
        assert!(reason.ends_with("envelope partial velocity amount is approximated"), "{reason}");
    }
}

#[test]
fn repeated_actual_definition_reuses_one_home_but_equal_named_owners_do_not_alias() {
    let text = r#"<Program Name="Identity"><ControlSignalSources><DAHDSR Name="Same"/></ControlSignalSources>
      <Layers><Layer Name="L"><Keygroups><Keygroup Name="K">
      <ControlSignalSources><DAHDSR Name="Same"/></ControlSignalSources>
      <Oscillators><SamplePlayer Name="P"><Connections>
      <SignalConnection Source="$Program/Same" Destination="Pitch" Ratio="0.1"/>
      <SignalConnection Source="$Program/Same" Destination="Pitch" Ratio="0.2"/>
      <SignalConnection Source="$Keygroup/Same" Destination="Pitch" Ratio="0.3"/>
      </Connections></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#;
    let instrument = sampler_uvi::translate(text, Path::new(".")).unwrap().instrument;
    instrument.validate().unwrap();
    let routes = &instrument.zones[0].routes;
    assert_eq!(routes.len(), 3);
    let sources: Vec<_> = routes.iter().map(|route| instrument.routes[route.0].source).collect();
    assert_eq!(sources[0], sources[1], "one actual Program definition");
    assert_ne!(sources[0], sources[2], "different actual owner/node despite equal name and shape");
    assert_eq!(instrument.modulators[sources[0].0], instrument.modulators[sources[2].0]);
    assert_eq!(instrument.modulators.len(), 2);
}

#[test]
fn supported_builtin_ratio_dependency_still_builds_one_exact_scaled_route() {
    let output = r#"<SignalConnection Source="@MIDI CC 1" Destination="Gain" Ratio="0.5">
      <Connections><SignalConnection Source="@MIDI CC 2" Destination="Ratio" Ratio="0.5"/></Connections>
      </SignalConnection>"#;
    let instrument = sampler_uvi::translate(&xml("", "", "", output), Path::new(".")).unwrap().instrument;
    instrument.validate().unwrap();
    let recipe = &instrument.source_indices.processing[0];
    assert_eq!(recipe.routes.len(), 1);
    let route = &instrument.routes[recipe.routes[0].0];
    assert_eq!(instrument.modulators[route.source.0].source, ir::ModulationSource::Controller(1));
    let scale = route.scale.expect("the actual Ratio dependency remains live");
    assert_eq!(instrument.modulators[scale.source.0].source, ir::ModulationSource::Controller(2));
    assert_eq!(route.target, ir::Target::Amplitude);
}

#[test]
fn promoted_constant_property_representation_keeps_its_separate_publication_law() {
    let source = r#"<ConstantModulation Name="Property" ScriptLive="1" Value="0.25"/>"#;
    let output = r#"<SignalConnection Source="$Program/Property" Destination="Gain" Ratio="0.5"/>"#;
    let instrument = sampler_uvi::translate(&xml(source, "", "", output), Path::new(".")).unwrap().instrument;
    instrument.validate().unwrap();
    assert_eq!(instrument.source_indices.constant_properties.len(), 1);
    let property = &instrument.source_indices.constant_properties[0];
    // Default: Core law, reported as an approximation (not omitted).
    assert_eq!(property.law, ir::SourceConstantPropertyLaw::DeclaredCoreTargetRead);
    assert!(instrument.unsupported.iter().any(|u| u.feature.contains("Constant cadence") && u.reason == ir::Reason::UnknownLaw));
    let recipe = &instrument.source_indices.processing[0];
    let route = &instrument.routes[recipe.routes[0].0];
    assert!(matches!(instrument.modulators[route.source.0].source,
        ir::ModulationSource::ConstantProperty { node, value, style, .. }
        if node == property.node && value == property.value && style == property.style));
}

#[test]
fn non_plain_dahdsr_output_translates_with_an_activation_ownership_warning() {
    let source = r#"<DAHDSR Name="Amp"/>"#;
    let pitch = r#"<SignalConnection Source="$Program/Amp" Destination="Pitch" Ratio="0.1"/>"#;
    let output = r#"<SignalConnection Source="$Program/Amp" Destination="Gain" Ratio="0.5"/>"#;
    for prefix in [pitch, ""] {
        let instrument = sampler_uvi::translate(&xml(source, prefix, "", output), Path::new(".")).unwrap().instrument;
        instrument.validate().unwrap();
        // The shared source is approximated per voice and owned by the routing keygroup.
        let warnings: Vec<_> = instrument.unsupported.iter()
            .filter(|item| item.value.contains("activation ownership is unproved"))
            .collect();
        assert_eq!(warnings.len(), 1, "{:?}", instrument.unsupported);
        assert_eq!(warnings[0].reason, ir::Reason::UnknownLaw);
    }
}

#[test]
fn bypassed_and_inert_ahd_connections_do_not_invent_an_active_source_requirement() {
    for (source, output, factor) in [
        (r#"<AHD Name="Steps" Bypass="1"/>"#,
            r#"<SignalConnection Source="$Program/Steps" Destination="Gain" Ratio="0.5"/>"#, 0.5),
        (r#"<AHD Name="Steps"/>"#,
            r#"<SignalConnection Source="$Program/Steps" Destination="Gain" Ratio="0"/>"#, 1.),
    ] {
        let instrument = sampler_uvi::translate(&xml(source, "", "", output), Path::new(".")).unwrap().instrument;
        instrument.validate().unwrap();
        assert!(instrument.modulators.is_empty(), "no actual source evaluation");
        assert!(instrument.source_indices.processing[0].sources.is_empty());
        assert!(instrument.source_indices.processing[0].routes.is_empty());
        let recipe = &instrument.source_indices.processing[0];
        let chain = &instrument.chains[recipe.chain.expect("real gain owner").0];
        assert!(chain.pre_amplitude.iter().any(|processor|
            matches!(processor, ir::Processor::Gain(ir::Gain::Linear(value)) if *value == factor)),
            "the original fixed source multiplier remains on the actual output");
    }
}
