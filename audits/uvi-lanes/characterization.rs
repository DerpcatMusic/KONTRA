//! Detached CURRENT13 characterization, NOT_RUN / NOT_COMPILED.
//! Intended future placement: a cfg(test) child module of sampler-uvi's
//! engine_parameters module. Not included in any product target or manifest.
//! Only synthetic XML and generated zero PCM; no files, bank, reader or render.

use super::{binding, bindings};
use crate::script::{Command, Config, ScriptHost};
use crate::scripted::{Driver, HostInput};
use sampler_core::{EngineParameterBinding, Error, Limits, Pcm, Prepared, Runtime};
use std::path::Path;

const RATE: u32 = 48_000;

fn xml(kind: &str, attributes: &str, name: &str, after: f64) -> String {
    format!(
        r#"<UVI4><Program><EventProcessors><ScriptProcessor><script>
        function onController(e)
          local f=Program.layers[1].keygroups[1].inserts[1]
          f:setParameter('{name}',{after})
          assert(f:getParameter('{name}')=={after},'queue-admitted readback')
        end
        </script></ScriptProcessor></EventProcessors>
        <Layers><Layer><Keygroups><Keygroup><Inserts><{kind} {attributes}/></Inserts>
        <Oscillators><SamplePlayer SamplePath="synthetic.wav" BaseNote="60"/></Oscillators>
        </Keygroup></Keygroups></Layer></Layers></Program></UVI4>"#
    )
}

fn address(xml: &str, kind: &str, name: &str) -> EngineParameterBinding {
    let doc = crate::parse_program_xml(xml).unwrap();
    let node = doc.descendants().find(|n| n.has_tag_name(kind)).unwrap();
    binding(node.id().get_usize(), kind, name).unwrap()
}

fn plan(xml: &str, keep_zones: bool) -> Prepared {
    let mut instrument = crate::translate(xml, Path::new(".")).unwrap().instrument;
    assert_eq!(instrument.assets.len(), 1);
    assert_eq!(instrument.zones.len(), 1);
    // Lua is run by the actual ScriptHost below, not the core behavior binder.
    instrument.behaviors.clear();
    if !keep_zones {
        instrument.retain_zones(|_| false);
        assert!(instrument.zones.is_empty());
    }
    let native = bindings(&instrument);
    let pcm = instrument
        .assets
        .iter()
        .map(|_| Pcm::new(RATE, vec![[0.; 2]; 64].into_boxed_slice()).unwrap())
        .collect();
    sampler_core::lower::lower(&instrument, RATE, pcm, |_, plan| Ok(plan))
        .unwrap()
        // Same final native-binding attachment used by UVI assemble/assemble_streamed.
        .with_engine_parameters(native, Vec::new())
        .unwrap()
}

fn runtime(plan: Prepared) -> Runtime {
    let limits = Limits::for_plan(&plan, 4, 4);
    Runtime::new(plan, limits).unwrap()
}

#[test]
fn queue_admitted_readback_precedes_missing_registered_lane() {
    for (kind, attributes, name, after) in [
        ("OnePole", "Freq=\"1000\" Bypass=\"1\"", "Freq", 600.),
        ("OnePole", "Freq=\"1000\" KeyTracking=\"1\"", "Freq", 600.),
        ("OnePole", "Freq=\"25000\"", "Freq", 600.),
        ("Gain", "Volume=\"1\" Bypass=\"1\"", "Volume", 0.5),
        ("Gain", "Volume=\"1000\"", "Volume", 0.5),
    ] {
        let xml = xml(kind, attributes, name, after);
        let b = address(&xml, kind, name);
        let plan = plan(&xml, true);
        assert!(
            !plan
                .engine_parameter_bindings()
                .iter()
                .any(|p| p.address == b.address)
        );
        let mut host = ScriptHost::new(&xml, (), Config::default()).unwrap();
        host.controller(1, 64, 0);
        assert!(host.findings().is_empty(), "{:?}", host.findings());
        assert_eq!(
            host.insert_overrides(),
            vec![(b.address.generic as usize, name.into(), after.to_string())]
        );
        let commands = host.take_commands();
        let [Command::EngineParameter { address, value }] = commands.as_slice() else {
            panic!("one queued insert write expected");
        };
        assert_eq!(*address, b.address);
        assert_eq!(*value, b.law.normalized_value(after).unwrap());
        let mut rt = runtime(plan);
        assert_eq!(
            rt.set_engine_parameter(*address, *value),
            Err(Error::InvalidInput)
        );
        assert_eq!(rt.engine_parameter(*address), Err(Error::InvalidInput));
    }
}

#[test]
fn driver_reports_missing_lane_without_setter_acknowledgement() {
    let xml = xml("OnePole", "Freq=\"1000\" KeyTracking=\"1\"", "Freq", 600.);
    let b = address(&xml, "OnePole", "Freq");
    let mut rt = runtime(plan(&xml, true));
    let host = ScriptHost::new(&xml, (), Config::default()).unwrap();
    let mut driver = Driver::new(host, Vec::new(), RATE);
    for _ in 0..2 {
        driver.input(
            &rt,
            HostInput::Controller {
                cc: 1,
                value: 64,
                channel: 0,
            },
        );
        driver.wake(&mut rt).unwrap();
    }
    assert_eq!(driver.unmodeled(), ["insert parameter without a DSP lane"]);
    assert_eq!(rt.engine_parameter(b.address), Err(Error::InvalidInput));
    // Core behavior-op outcome records are not produced by the direct UVI call.
    assert!(rt.take_engine_parameter_outcome().is_none());
}

#[test]
fn supported_inserts_keep_exact_bindings_but_late_native_aliases_are_absent() {
    for (kind, attributes, name, after) in [
        ("OnePole", "Freq=\"1000\"", "Freq", 600.),
        ("Gain", "Volume=\"1\"", "Volume", 0.5),
    ] {
        let xml = xml(kind, attributes, name, after);
        let b = address(&xml, kind, name);
        let plan = plan(&xml, true);
        assert!(plan.engine_parameter_bindings().contains(&b));
        assert!(
            plan.parameter_registry()
                .descriptors()
                .any(|d| d.control == b.control)
        );
        // The registry was built by lower before UVI added these bindings.
        assert!(
            plan.parameter_registry()
                .resolve_native(b.address)
                .is_none()
        );
        let mut rt = runtime(plan);
        let value = b.law.normalized_value(after).unwrap();
        assert_eq!(rt.set_engine_parameter(b.address, value), Ok(()));
        assert_eq!(rt.engine_parameter(b.address), Ok(value));
    }
}

#[test]
fn dropped_voice_chain_keeps_descriptor_and_accepts_an_unused_control() {
    let xml = xml("OnePole", "Freq=\"1000\"", "Freq", 600.);
    let b = address(&xml, "OnePole", "Freq");
    let plan = plan(&xml, false);
    assert!(plan.engine_parameter_bindings().contains(&b));
    assert!(plan.controls().iter().any(|d| d.id == b.control));
    assert!(
        plan.parameter_registry()
            .descriptors()
            .any(|d| d.control == b.control)
    );
    // No zones -> lower's zone loop never consumes this voice chain. A
    // descriptor and control existence do not establish a prepared DSP lane.
    let mut rt = runtime(plan);
    let value = b.law.normalized_value(600.).unwrap();
    assert_eq!(rt.set_engine_parameter(b.address, value), Ok(()));
    assert_eq!(rt.engine_parameter(b.address), Ok(value));
}
