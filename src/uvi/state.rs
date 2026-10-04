//! Bounded native script state, independent of file-write authority and audio packets.
use super::{
    host::{Host, ParameterValue, ResourceKind},
    program::{NodeId, Program},
};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const MAX_STATE_BYTES: usize = 2 << 20;
const VERSION: u32 = 1;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    version: u32,
    program: [u8; 32],
    processors: Vec<(NodeId, String)>,
    #[serde(default)]
    parameters: Vec<(NodeId, String, ParameterValue)>,
    #[serde(default)]
    resources: Vec<(NodeId, ResourceKind, String)>,
}

/// Opaque native script/widget and original-node parameter/resource deltas.
/// Does not capture voice state, transport or synthetic Part/Synth context. May contain private values. Deliberately no Debug.
#[derive(Clone)]
pub struct SavedState(Payload);

pub(crate) fn fingerprint(program: &Program) -> Result<[u8; 32]> {
    let mut hash = Sha256::new();
    for node in &program.nodes {
        let bytes = serde_json::to_vec(node)?;
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
        hash.update((node.text.len() as u64).to_le_bytes());
        hash.update(node.text.as_bytes());
    }
    Ok(hash.finalize().into())
}

impl SavedState {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_STATE_BYTES,
            "UVI saved state exceeds 2 MiB"
        );
        let state = Self(serde_json::from_slice(bytes)?);
        state.check()?;
        Ok(state)
    }
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.check()?;
        let bytes = serde_json::to_vec(&self.0)?;
        ensure!(
            bytes.len() <= MAX_STATE_BYTES,
            "UVI saved state exceeds 2 MiB"
        );
        Ok(bytes)
    }
    fn check(&self) -> Result<()> {
        ensure!(
            self.0.version == VERSION,
            "Unsupported UVI saved state version"
        );
        ensure!(
            self.0.processors.len() <= 64,
            "UVI saved state exceeds processor limit"
        );
        ensure!(
            self.0
                .processors
                .windows(2)
                .all(|pair| pair[0].0 < pair[1].0),
            "Duplicate or unordered UVI saved processor"
        );
        ensure!(
            self.0
                .parameters
                .len()
                .saturating_add(self.0.resources.len())
                <= 65_536,
            "UVI saved state exceeds override limit"
        );
        ensure!(
            self.0
                .parameters
                .windows(2)
                .all(|p| (&p[0].0, &p[0].1) < (&p[1].0, &p[1].1)),
            "Duplicate or unordered UVI saved parameter"
        );
        ensure!(
            self.0.resources.windows(2).all(|p| p[0].0 < p[1].0),
            "Duplicate or unordered UVI saved resource"
        );
        ensure!(
            self.0
                .parameters
                .iter()
                .all(|(_, _, value)| !matches!(value, ParameterValue::Number(n) if !n.is_finite())),
            "Nonfinite UVI saved parameter"
        );
        for (_, _, path) in &self.0.resources {
            super::host::resource_path(path)?;
        }
        let bytes = self
            .0
            .processors
            .iter()
            .try_fold(0usize, |total, (_, text)| total.checked_add(text.len()));
        ensure!(
            bytes.is_some_and(|n| n <= MAX_STATE_BYTES),
            "UVI saved state exceeds 2 MiB"
        );
        Ok(())
    }
    pub(crate) fn new(
        program: [u8; 32],
        processors: BTreeMap<NodeId, String>,
        host: &Host,
    ) -> Result<Self> {
        let current = host.parameters.borrow();
        let parameters: Vec<_> = host
            .baseline
            .iter()
            .enumerate()
            .flat_map(|(node, baseline)| {
                current[node].iter().filter_map(move |(name, value)| {
                    baseline
                        .get(name)
                        .filter(|old| *old != value)
                        .map(|_| (node, name.clone(), value.clone()))
                })
            })
            .collect();
        let mut resources = host.loaded_resources.borrow().clone();
        // Direct SamplePath edits still require the existing approved audio
        // capability on reload, rather than trusting a serialized path.
        for (node, name, value) in &parameters {
            if name == "SamplePath" {
                let ParameterValue::Text(path) = value else {
                    anyhow::bail!("Invalid saved resource path type")
                };
                let kind = match host.types[*node].as_str() {
                    "SamplePlayer" => ResourceKind::Sample,
                    "Convolver" | "SampledReverb" => ResourceKind::Impulse,
                    _ => anyhow::bail!("Unsupported saved resource target"),
                };
                resources.insert(*node, (kind, path.clone()));
            }
        }
        let resources = resources
            .into_iter()
            .map(|(node, (kind, path))| (node, kind, path))
            .collect();
        let state = Self(Payload {
            version: VERSION,
            program,
            parameters,
            resources,
            processors: processors.into_iter().collect(),
        });
        state.encode()?;
        Ok(state)
    }
    /// Reject structural, version and identity errors before any constructor.
    /// Widget-dependent validation then runs only inside the fresh replacement.
    pub(crate) fn validate(&self, program: &Program) -> Result<()> {
        self.check()?;
        ensure!(
            self.0.program == fingerprint(program)?,
            "UVI saved state belongs to a different Program"
        );
        let processors = program
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(id, node)| (node.kind == "ScriptProcessor").then_some(id));
        ensure!(
            processors.eq(self.0.processors.iter().map(|(id, _)| *id)),
            "UVI saved state processor identities differ"
        );
        let baseline = super::host::source_parameters(program);
        for (node, name, value) in &self.0.parameters {
            let old = baseline
                .get(*node)
                .and_then(|values| values.get(name))
                .ok_or_else(|| anyhow::anyhow!("Unknown UVI saved parameter"))?;
            ensure!(
                std::mem::discriminant(old) == std::mem::discriminant(value),
                "Incompatible UVI saved parameter type"
            );
            if name == "SamplePath" {
                ensure!(
                    self.0
                        .resources
                        .iter()
                        .any(|(target, _, path)| *target == *node
                            && matches!(value, ParameterValue::Text(text) if text == path)),
                    "UVI saved resource path lacks an approved load"
                );
            }
        }
        for (node, kind, _) in &self.0.resources {
            let node = program
                .nodes
                .get(*node)
                .ok_or_else(|| anyhow::anyhow!("Unknown UVI saved resource target"))?;
            ensure!(
                match kind {
                    ResourceKind::Sample => node.kind == "SamplePlayer",
                    ResourceKind::Impulse =>
                        matches!(node.kind.as_str(), "Convolver" | "SampledReverb"),
                },
                "Incompatible UVI saved resource target"
            );
        }
        for (_, text) in &self.0.processors {
            super::host::parse_state(text.as_bytes())?;
        }
        Ok(())
    }
    /// Validate and prepare audio overrides before any authored chunk executes.
    pub(crate) fn prepare_audio(&self, resources: &super::host::Resources) -> Result<()> {
        for (_, kind, path) in &self.0.resources {
            let super::host::ResourceResponse::Audio(info) =
                resources(&super::host::ResourceRequest::ReadAudio {
                    kind: *kind,
                    path: path.clone(),
                })?
            else {
                anyhow::bail!("UVI saved audio capability returned incompatible data")
            };
            ensure!(
                info.rate > 0
                    && info.channels > 0
                    && info.channels <= 64
                    && info.name.len() <= 4096
                    && info.frames > 0,
                "Invalid UVI saved audio metadata"
            );
        }
        Ok(())
    }
    pub(crate) fn prepare_renderer(
        &self,
        renderer: &mut super::playback::Renderer<'_>,
    ) -> Result<()> {
        let commands = self
            .0
            .resources
            .iter()
            .map(|(node, kind, path)| super::host::Command {
                frame: 0,
                action: super::host::Action::LoadResource {
                    node: *node,
                    kind: *kind,
                    path: path.clone(),
                },
            })
            .chain(
                self.0
                    .parameters
                    .iter()
                    .map(|(node, name, value)| super::host::Command {
                        frame: 0,
                        action: super::host::Action::Parameter {
                            node: *node,
                            parameter: name.clone(),
                            value: value.clone(),
                        },
                    }),
            )
            .collect::<Vec<_>>();
        renderer.apply_boundary(&[], None, &commands)
    }

    /// Preload the native Program before its main chunks, onLoad and onInit.
    pub(crate) fn preload(&self, lua: &mlua::Lua, host: &Host) -> Result<()> {
        for (node, kind, path) in &self.0.resources {
            let object = host.objects.raw_get::<mlua::Table>(*node + 1)?;
            let name = match kind {
                ResourceKind::Sample => "loadSample",
                ResourceKind::Impulse => "loadImpulse",
            };
            let task: mlua::Table = lua
                .globals()
                .get::<mlua::Function>(name)?
                .call((object, path.clone()))?;
            ensure!(
                task.get::<bool>("success")?,
                "UVI saved resource is unavailable"
            );
        }
        for (node, name, value) in &self.0.parameters {
            host.parameters.borrow_mut()[*node].insert(name.clone(), value.clone());
            host.commands.borrow_mut().push(super::host::Command {
                frame: 0,
                action: super::host::Action::Parameter {
                    node: *node,
                    parameter: name.clone(),
                    value: value.clone(),
                },
            });
        }
        Ok(())
    }
    pub(crate) fn processor(&self, node: NodeId) -> Option<&str> {
        self.0
            .processors
            .binary_search_by_key(&node, |(id, _)| *id)
            .ok()
            .map(|index| self.0.processors[index].1.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uvi::{
        host::{UiEdit, UiEditValue, UiValue},
        program::parse_program,
        script::Session,
        worker::{Stamp, Worker},
    };
    use std::{
        cell::Cell,
        rc::Rc,
        time::{Duration, Instant},
    };

    fn program() -> Program {
        parse_program(
            r#"<Program><EventProcessors>
        <ScriptProcessor n="0.25"><script><![CDATA[
          order='';n=Knob('n',0,0,1);n.changed=function()order=order..'C'end
          transient=Knob('transient',0.2,0,1);transient.persistent=false
          function onLoad(data)assert(data.marker=='A' and n.value==0);order=order..'L'end
          function onInit()order=order..'I'end
          function onSave()return {marker='A',order=order,nested={true,3}}end
        ]]></script></ScriptProcessor>
        <ScriptProcessor n="0.5"><script><![CDATA[
          order='';n=Knob('n',0,0,1);n.changed=function()order=order..'C'end
          function onLoad(data)assert(data.marker=='B' and n.value==0);order=order..'L'end
          function onInit()order=order..'I'end
          function onSave()return {marker='B',order=order}end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer/></Layers></Program>"#,
        )
        .unwrap()
    }

    #[test]
    fn saved_state_roundtrip_preserves_scopes_and_initialization_order() {
        let program = program();
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), None, 48000).unwrap();
        let processors = program
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(id, node)| (node.kind == "ScriptProcessor").then_some(id))
            .collect::<Vec<_>>();
        for (processor, value) in processors.iter().zip([0.75, 0.875]) {
            let widget = session.ui_snapshot(*processor).unwrap().widgets[0].id;
            session
                .edit_ui(
                    &UiEdit {
                        processor: *processor,
                        widget,
                        value: UiEditValue::Number(value),
                        modifiers: Default::default(),
                    },
                    0,
                )
                .unwrap();
        }
        let saved = session.saved_state(0).unwrap();
        let saved = SavedState::decode(&saved.encode().unwrap()).unwrap();
        assert!(!saved.0.processors[0].1.contains("transient="));
        let mut restored = Session::new_program_chain_with_state(
            &program,
            BTreeMap::new(),
            None,
            48000,
            Some(&saved),
        )
        .unwrap();
        for (processor, value) in processors.iter().zip([0.75, 0.875]) {
            assert!(
                restored.ui_snapshot(*processor).unwrap().widgets[0].value
                    == Some(UiValue::Number(value))
            );
        }
        let after = restored.saved_state(0).unwrap();
        for (_, document) in &after.0.processors {
            let (_, data) = crate::uvi::host::parse_state(document.as_bytes()).unwrap();
            assert_eq!(data.unwrap()["order"], "LCI");
        }
    }

    #[test]
    fn saved_state_rejects_complete_invalid_bundle_before_instrument_callbacks() {
        let program=parse_program(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[loadData('probe.json');n=Knob('n',0,0,1)]]></script></ScriptProcessor><ScriptProcessor><script><![CDATA[n=Knob('n',0,0,1)]]></script></ScriptProcessor></EventProcessors><Layers><Layer/></Layers></Program>"#).unwrap();
        let calls = Rc::new(Cell::new(0));
        let observed = calls.clone();
        let resources: crate::uvi::host::Resources = Rc::new(move |_| {
            observed.set(observed.get() + 1);
            Ok(crate::uvi::host::ResourceResponse::Bytes(b"{}".to_vec()))
        });
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), Some(resources.clone()), 48000)
                .unwrap();
        let saved = session.saved_state(0).unwrap();
        calls.set(0);
        for document in [
            "<invalid/>",
            "<UVI4><ScriptProcessor><ScriptProcessor/></ScriptProcessor></UVI4>",
            "<UVI4><ScriptProcessor><ScriptData/><ScriptData/></ScriptProcessor></UVI4>",
            "<UVI4><ScriptProcessor><ScriptData><state>{}</state></ScriptData></ScriptProcessor></UVI4>",
        ] {
            let mut broken = saved.clone();
            broken.0.processors[1].1 = document.into();
            assert!(
                Session::new_program_chain_with_state(
                    &program,
                    BTreeMap::new(),
                    Some(resources.clone()),
                    48000,
                    Some(&broken)
                )
                .is_err()
            );
            assert_eq!(calls.get(), 0);
        }
        assert!(
            session.ui_snapshot(saved.0.processors[0].0).is_ok(),
            "failed replacement does not mutate the old session"
        );
        let mut broken = saved.clone();
        broken.0.program[0] ^= 1;
        assert!(broken.validate(&program).is_err());
        let mut broken = saved.clone();
        broken.0.processors.push(broken.0.processors[0].clone());
        assert!(broken.encode().is_err());
        let mut broken = saved.clone();
        broken.0.version = 2;
        assert!(broken.encode().is_err());
        assert!(SavedState::decode(&vec![b' '; MAX_STATE_BYTES + 1]).is_err());
        assert!(SavedState::decode(b"{\"version\":1}").is_err());
    }

    #[test]
    fn unavailable_on_init_widget_is_rejected_without_silent_default_restore() {
        let program = parse_program(
            r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
          function onInit()n=Knob('n',0.25,0,1)end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer/></Layers></Program>"#,
        )
        .unwrap();
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), None, 48000).unwrap();
        let processor = session.saved_state(0).unwrap().0.processors[0].0;
        let widget = session.ui_snapshot(processor).unwrap().widgets[0].id;
        session
            .edit_ui(
                &UiEdit {
                    processor,
                    widget,
                    value: UiEditValue::Number(0.75),
                    modifiers: Default::default(),
                },
                0,
            )
            .unwrap();
        let saved = session.saved_state(0).unwrap();
        assert!(
            Session::new_program_chain_with_state(
                &program,
                BTreeMap::new(),
                None,
                48000,
                Some(&saved)
            )
            .is_err()
        );
        assert!(
            session.ui_snapshot(processor).unwrap().widgets[0].value == Some(UiValue::Number(0.75))
        );
    }

    #[test]
    fn measured_missing_gain_pan_defaults_support_setters_and_saved_deltas() {
        let program=parse_program(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
          objects={Program,Program.layers[1],Program.layers[1].keygroups[1]}
          original={};for i,object in ipairs(objects)do original[i]={object:getParameter('Gain'),object:getParameter('Pan')};assert(object:hasParameter('Gain') and object:hasParameter('Pan'))end
          sample=objects[3].oscillators[1];sampleGain=sample:getParameter('Gain');assert(sample:hasParameter('Gain') and not sample:hasParameter('Pan'))
          function onController(e)
            for _,object in ipairs(objects)do
              object:setParameter('Gain',-0.5);assert(object:getParameter('Gain')==-0.5)
              object:setParameter('Gain',2.5);assert(object:getParameter('Gain')==2.5)
              object:setParameter('Pan',-1.5);assert(object:getParameter('Pan')==-1.5)
              object:setParameter('Pan',1.5);assert(object:getParameter('Pan')==1.5)
              object:setParameter('Gain',0.625);object:setParameter('Pan',0.25)
            end
            sample:setParameter('Gain',-0.5);assert(sample:getParameter('Gain')==-0.5)
            sample:setParameter('Gain',2.5);assert(sample:getParameter('Gain')==2.5)
            sample:setParameter('Gain',0.625)
          end
          function onLoad(data)
            assert(data.marker and sampleGain==0.625)
            for _,value in ipairs(original)do assert(value[1]==0.625 and value[2]==0.25)end
          end
          function onSave()return {marker=true}end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="authored.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let baseline = crate::uvi::host::source_parameters(&program);
        for (node, values) in program.nodes.iter().zip(&baseline) {
            if matches!(
                node.kind.as_str(),
                "Program" | "Layer" | "Keygroup" | "SamplePlayer"
            ) {
                assert!(values["Gain"] == ParameterValue::Number(1.));
                assert!(!node.attributes.contains_key("Gain"));
            }
            if matches!(node.kind.as_str(), "Program" | "Layer" | "Keygroup") {
                assert!(values["Pan"] == ParameterValue::Number(0.));
                assert!(!node.attributes.contains_key("Pan"));
            }
        }
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), None, 48000).unwrap();
        let initial = session.saved_state(0).unwrap();
        assert!(
            initial.0.parameters.is_empty(),
            "default maps are not persisted as overrides"
        );
        session
            .input(crate::uvi::script::Input {
                frame: 0,
                kind: crate::uvi::script::InputKind::Controller {
                    channel: 0,
                    controller: 1,
                    value: 127,
                },
            })
            .unwrap();
        let saved = session.saved_state(0).unwrap();
        assert_eq!(saved.0.parameters.len(), 7);
        let saved = SavedState::decode(&saved.encode().unwrap()).unwrap();
        let mut restored = Session::new_program_chain_with_state(
            &program,
            BTreeMap::new(),
            None,
            48000,
            Some(&saved),
        )
        .unwrap();
        assert_eq!(
            restored.saved_state(0).unwrap().encode().unwrap(),
            saved.encode().unwrap()
        );
        let mut rejected = saved.clone();
        let sample = program.sample_zones[0].player;
        rejected
            .0
            .parameters
            .push((sample, "Pan".into(), ParameterValue::Number(0.25)));
        rejected
            .0
            .parameters
            .sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        assert!(
            rejected.validate(&program).is_err(),
            "unmeasured SamplePlayer Pan is not admitted as a default property"
        );
    }

    #[test]
    fn native_bundle_preloads_deltas_resources_before_callbacks_and_on_init_wins() {
        let program = parse_program(r#"<Program Gain="1"><EventProcessors><ScriptProcessor><script><![CDATA[
          loadData('constructor.json');seenGain=Program:getParameter('Gain');loaded=false
          n=Knob('authored_level',0.25,0,1)
          n.changed=function()Program:setParameter('Gain',n.value)end
          function onNote(e)
            n.value=0.75;Program:setParameter('Gain',0.65);Program:setParameter('Bypass',true)
            assert(loadSample(Program.layers[1].keygroups[1].oscillators[1],'alternate.wav').success)
            postEvent(e)
          end
          function onLoad(data)
            loaded=true;assert(data.marker and seenGain==0.65 and n.value==0.25)
            assert(Program:getParameter('Bypass') and Program.layers[1].keygroups[1].oscillators[1]:getParameter('SamplePath')=='alternate.wav')
          end
          function onInit()
            if loaded then assert(n.value==0.75 and Program:getParameter('Gain')==0.75);n.value=0.4 end
          end
          function onSave()return {marker=true}end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="original.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let constructors = Rc::new(Cell::new(0));
        let observed = constructors.clone();
        let resources: crate::uvi::host::Resources = Rc::new(move |request| match request {
            crate::uvi::host::ResourceRequest::ReadData { .. } => {
                observed.set(observed.get() + 1);
                Ok(crate::uvi::host::ResourceResponse::Bytes(b"{}".to_vec()))
            }
            crate::uvi::host::ResourceRequest::ReadAudio { path, .. }
                if path == "alternate.wav" =>
            {
                Ok(crate::uvi::host::ResourceResponse::Audio(
                    crate::uvi::host::ResourceInfo {
                        name: path.clone(),
                        rate: 48000,
                        channels: 1,
                        frames: 256,
                    },
                ))
            }
            _ => Err(mlua::Error::runtime("unauthorized test resource")),
        });
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), Some(resources.clone()), 48000)
                .unwrap();
        session
            .input(crate::uvi::script::Input {
                frame: 0,
                kind: crate::uvi::script::InputKind::NoteOn {
                    channel: 0,
                    note: 60,
                    velocity: 127,
                },
            })
            .unwrap();
        let saved = session.saved_state(0).unwrap();
        assert_eq!(
            saved.0.parameters.len(),
            3,
            "only Gain, defaulted Bypass and SamplePath changed"
        );
        assert_eq!(saved.0.resources.len(), 1);
        constructors.set(0);
        let mut invalid = saved.clone();
        invalid.0.parameters[0].1 = "Invented".into();
        assert!(
            Session::new_program_chain_with_state(
                &program,
                BTreeMap::new(),
                Some(resources.clone()),
                48000,
                Some(&invalid)
            )
            .is_err()
        );
        assert_eq!(constructors.get(), 0);
        let mut invalid = saved.clone();
        invalid.0.resources[0].2 = "unauthorized.wav".into();
        for (_, name, value) in &mut invalid.0.parameters {
            if name == "SamplePath" {
                *value = ParameterValue::Text("unauthorized.wav".into());
            }
        }

        assert!(
            Session::new_program_chain_with_state(
                &program,
                BTreeMap::new(),
                Some(resources.clone()),
                48000,
                Some(&invalid)
            )
            .is_err()
        );
        assert_eq!(constructors.get(), 0);
        let mut restored = Session::new_program_chain_with_state(
            &program,
            BTreeMap::new(),
            Some(resources),
            48000,
            Some(&SavedState::decode(&saved.encode().unwrap()).unwrap()),
        )
        .unwrap();
        assert_eq!(constructors.get(), 1);
        let processor = saved.0.processors[0].0;
        assert!(
            restored.ui_snapshot(processor).unwrap().widgets[0].value
                == Some(UiValue::Number(f64::from(0.4f32)))
        );
        let after = restored.saved_state(0).unwrap();
        assert!(
            matches!(&after.0.parameters[1],(node,name,ParameterValue::Number(n)) if *node==0 && name=="Gain" && *n==f64::from(0.4f32))
        );
    }

    #[test]
    fn worker_state_capture_restore_rejects_stale_generation_and_retains_failed_reply() {
        let (config, _source) = crate::uvi::worker::tests::authored_bank_with_script(
            "loaded=false;seenGain=Program:getParameter('Gain');n=Knob('n',0.25,0,1);function onNote(e)if not loaded then Program:setParameter('Gain',0.75)end;postEvent(e)end;function onSave()return {authored=true}end;function onLoad(s)assert(s.authored and seenGain==0.75);loaded=true end;function onInit()if loaded then assert(Program:getParameter('Gain')==0.75)end end",
        );
        let path = config.bank.clone();
        let mut worker = Worker::start_hosted(config, 7, 9).unwrap();
        worker.wait_ready(Duration::from_secs(3)).unwrap();
        let stamp = Stamp {
            epoch: 7,
            generation: 9,
            frame: 0,
        };
        fn render(worker: &mut Worker, stamp: Stamp, edit: bool) -> crate::uvi::worker::Output {
            use crate::uvi::{
                player::UiInput,
                script::{HostRoot, HostedInput, Input, InputKind},
                worker::{HostedRequest, Request},
            };
            let ui = edit.then(|| UiInput {
                frame: 0,
                edit: UiEdit {
                    processor: worker.ui_processors()[0],
                    widget: 1,
                    value: UiEditValue::Number(0.75),
                    modifiers: Default::default(),
                },
            });
            let request = Request::new_with_ui(stamp, &[], ui.as_slice()).unwrap();
            let on = HostedInput::On {
                root: HostRoot {
                    epoch: stamp.epoch,
                    generation: stamp.generation,
                    token: 1,
                },
                input: Input {
                    frame: 0,
                    kind: InputKind::NoteOn {
                        channel: 0,
                        note: 60,
                        velocity: 127,
                    },
                },
            };
            worker
                .realtime()
                .try_submit_hosted(HostedRequest::new(request, &[on]).unwrap())
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                match worker.realtime().try_receive_available(stamp) {
                    Ok(Some(out)) => return out,
                    Ok(None) => {}
                    Err(error) => panic!("{error}"),
                }
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let original = render(&mut worker, stamp, true);
        assert!(
            worker
                .request_state_snapshot(Stamp {
                    generation: 8,
                    ..stamp
                })
                .is_err()
        );
        worker
            .request_state_snapshot(Stamp {
                frame: 512,
                ..stamp
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(5));
        assert!(
            worker.poll_state_snapshot().is_none(),
            "capture cannot precede requested processed boundary"
        );
        let request = worker
            .request_state_snapshot(Stamp {
                frame: 256,
                ..stamp
            })
            .unwrap();
        assert!(
            worker
                .request_state_snapshot(Stamp {
                    generation: 8,
                    ..stamp
                })
                .is_err()
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        let reply = loop {
            if let Some(reply) = worker.poll_state_snapshot() {
                break reply;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(reply.request, request);
        assert_eq!(
            reply.stamp,
            Stamp {
                frame: 256,
                ..stamp
            }
        );
        let saved = reply.snapshot.unwrap();
        drop(worker);
        let (mut config, _source) = crate::uvi::worker::tests::authored_bank_with_script(
            "loaded=false;seenGain=Program:getParameter('Gain');n=Knob('n',0.25,0,1);function onNote(e)if not loaded then Program:setParameter('Gain',0.75)end;postEvent(e)end;function onSave()return {authored=true}end;function onLoad(s)assert(s.authored and seenGain==0.75);loaded=true end;function onInit()if loaded then assert(Program:getParameter('Gain')==0.75)end end",
        );
        let other = config.bank.clone();
        config.bank = path.clone();
        let mut restored = Worker::start_hosted_with_state(config, 8, 10, saved).unwrap();
        restored.wait_ready(Duration::from_secs(3)).unwrap();
        let audio = render(
            &mut restored,
            Stamp {
                epoch: 8,
                generation: 10,
                frame: 0,
            },
            false,
        );
        assert_eq!(
            audio.audio, original.audio,
            "restored direct Program override prepares the same audible native gain"
        );
        let request = restored
            .request_state_snapshot(Stamp {
                epoch: 8,
                generation: 10,
                frame: 0,
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let reply = loop {
            if let Some(reply) = restored.poll_state_snapshot() {
                break reply;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(reply.request, request);
        assert_eq!(reply.stamp.generation, 10);
        let mut invalid = reply.snapshot.unwrap();
        invalid.0.program[0] ^= 1;
        let (config, _) = crate::uvi::worker::tests::authored_bank_with_script("n=Knob('n',0,0,1)");
        let invalid_path = config.bank.clone();
        let failed = Worker::start_hosted_with_state(config, 9, 11, invalid).unwrap();
        assert!(failed.wait_ready(Duration::from_secs(3)).is_err());
        assert_eq!(failed.status(), crate::uvi::worker::Status::Failed);
        assert_eq!(
            restored.status(),
            crate::uvi::worker::Status::Ready,
            "failed preparation preserves existing controller"
        );
        drop(failed);
        std::fs::remove_file(invalid_path).unwrap();
        drop(restored);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(other).unwrap();

        let (config, _source) = crate::uvi::worker::tests::authored_bank_with_script(
            "count=0;function onSave()count=count+1;if count>1 then error('authored state failure')end;return {count=count}end",
        );
        let path = config.bank.clone();
        let worker = Worker::start_hosted(config, 3, 4).unwrap();
        worker.wait_ready(Duration::from_secs(3)).unwrap();
        worker
            .request_state_snapshot(Stamp {
                epoch: 3,
                generation: 4,
                frame: 0,
            })
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let reply = loop {
            if let Some(reply) = worker.poll_state_snapshot() {
                break reply;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        let retained = reply.snapshot.unwrap();
        let retained_bytes = retained.encode().unwrap();
        worker
            .request_state_snapshot(Stamp {
                epoch: 3,
                generation: 4,
                frame: 0,
            })
            .unwrap();
        let reply = loop {
            if let Some(reply) = worker.poll_state_snapshot() {
                break reply;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        };
        assert!(reply.snapshot.is_err());
        assert_eq!(
            retained.encode().unwrap(),
            retained_bytes,
            "failed onSave cannot alter the previously captured owned payload"
        );
        while worker.status() != crate::uvi::worker::Status::Failed {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        drop(worker);
        std::fs::remove_file(path).unwrap();
    }
}
