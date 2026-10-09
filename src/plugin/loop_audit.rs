//! Retained UI-loop admission and source-generation regression witnesses.
impl PartShared {
    pub(crate) fn loop_audit_install_ingress(&self, ingress:Option<crate::sound::v2::ControlIngress>) {
        *self.ingress.lock().unwrap()=ingress;
    }
}

impl Shared {
    pub(crate) fn widget_gate_install(&self, core: &mut V2Core) {
        while let Some((slot, _, part)) = self.ready.pop() { core.install(slot, part); }
    }
    /// Same audio readback/effect publication as Process, for Original gestures.
    pub(crate) fn widget_gate_readback(&self, core: &mut V2Core) {
        for slot in 0..core.parts() {
            let Some(atoms) = self.part(slot) else { continue };
            atoms.refresh_controls(|id| core.control_value(slot, id));
            let epoch = core.epoch(slot);
            core.take_effects(slot, &mut |instance, effect| self.effects.push((slot, epoch, instance, *effect)).is_ok());
        }
        self.apply_effects();
    }
}

impl PartShared {
    pub(crate) fn widget_gate_callback(&self, program: usize) -> String {
        sampler_ksp::callback_of(&self.scripts.lock().unwrap().views, program)
    }
    pub(crate) fn widget_gate_uvi_value(&self, id: sampler_ui_ir::ControlId) -> Option<f64> {
        self.scripts.lock().unwrap().uvi.as_ref().and_then(|uvi| uvi.value(id))
    }
}

use super::*;

impl ControlCell {
    pub(crate) fn loop_audit_new(id: sampler_ui_ir::ControlId, value: f64) -> Self {
        Self {
            id,
            value: AtomicU64::new(value.to_bits()),
        }
    }
}

fn ingress() -> (Shared, crate::sound::v2::Part, sampler_ui_ir::ControlId) {
    let script = sampler_ksp::compile(
        "on init declare ui_knob $k(0,100,1) end on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let id = sampler_ui_ir::ControlId(script.controls()[0].definition.id.0);
    let prepared = sampler_core::Prepared::new(48000, vec![], vec![], 0).unwrap();
    let plan = script.bind(prepared).unwrap();
    let limits = sampler_core::Limits::for_plan(&plan, 8, 0);
    let mut runtime = crate::sound::v2::Part::new(
        sampler_core::Runtime::new(plan, limits).unwrap(),
        MixTree::instrument("test"),
    )
    .unwrap();
    let shared = Shared::default();
    shared.ensure_parts(1);
    let part = shared.part(0).unwrap();
    part.generation.store(1, Ordering::Release);
    *part.controls.lock().unwrap() = vec![ControlCell::loop_audit_new(id, 0.)].into();
    *part.ingress.lock().unwrap() = runtime.ui_controls.take();
    (shared, runtime, id)
}

#[test]
fn loop_audit_full_queue_and_nonfinite_values_publish_unadmitted_state() {
    let (shared, _runtime, id) = ingress();
    let part = shared.part(0).unwrap();
    for _ in 0..256 {
        assert!(shared.set_control(0, id, 0.));
    }
    assert!(!shared.set_control(0, id, 99.));
    assert_eq!(
        part.control_values(),
        [(id, 0.)],
        "queue rejection never changes authoritative state"
    );
    assert!(!shared.set_control(0, id, f64::NAN));
    assert!(part.control_values()[0].1.is_finite());
}

#[test]
fn loop_audit_pending_edit_has_no_source_generation() {
    let (shared, _runtime, id) = ingress();
    let part = shared.part(0).unwrap();
    assert!(shared.set_control_at(0, 1, id, 25.));
    part.generation.store(2, Ordering::Release);
    *part.ingress.lock().unwrap() = None;
    assert!(
        !shared.set_control_at(0, 1, id, 25.),
        "stale face cannot submit into a replacement"
    );
    assert_eq!(part.display_values(), [(id, 0.)]);
}

#[test]
fn publication_is_sparse_and_identical_updates_are_noops() {
    let script = sampler_ksp::compile(
        "on init declare ui_knob $k(0,100,1) end on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let face = script.ui(&|_| None).unwrap();
    let mut part = PartView {
        interfaces: vec![face.clone()].into(),
        ..Default::default()
    };
    let base = part.interfaces.clone();
    assert!(!part.publish_interface(&face));
    let mut changed = face.clone();
    changed.widgets[0].value_text = Some("updated".into());
    assert!(part.publish_interface(&changed));
    let updates = part.updates.clone();
    assert_eq!(part.ui_revision, 1);
    assert_eq!(part.updates[0].widgets.len(), 1);
    assert!(!part.publish_interface(&changed));
    assert!(Arc::ptr_eq(&updates, &part.updates));
    assert!(Arc::ptr_eq(&base, &part.interfaces));
    assert_eq!(part.ui_revision, 1);
    assert!(part.publish_interface(&face));
    assert_eq!(part.updates[0], Default::default());
}

#[test]
fn stale_epoch_effect_replay_cannot_mutate_the_new_source() {
    let script = sampler_ksp::compile("on init declare ui_knob $k(0,100,1) end on on ui_control($k) set_knob_label($k,\"changed\") end on", 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
    let view = script.view();
    let service = (0..1024)
        .find(|&n| view.service(n) == Some("set_knob_label"))
        .unwrap();
    let authored = script.ui(&|_| None).unwrap();
    let ui_id = script.model().interface.widgets[0].ui_id;
    let prepared = sampler_core::Prepared::new(48000, vec![], vec![], 0).unwrap();
    let limits = sampler_core::Limits::for_plan(&prepared, 8, 0);
    let rt = sampler_core::Runtime::new(prepared, limits).unwrap();
    let shared = Shared::default();
    shared.ensure_parts(1);
    let part = shared.part(0).unwrap();
    part.generation.store(2, Ordering::Release);
    part.scripts.lock().unwrap().views.push(view);
    let keys = part.scripts.lock().unwrap().keys();
    shared.view.lock().unwrap().parts[0] = PartView {
        generation: 2,
        keys,
        interfaces: vec![authored].into(),
        ..Default::default()
    };
    let mut effect = sampler_core::Effect {
        plan: rt.active_plan(),
        instance: None,
        service,
        args: [0; sampler_core::EFFECT_ARGS],
        count: 1,
        text: Some(sampler_core::Text::new("changed")),
    };
    effect.args[0] = i64::from(ui_id);
    shared.effects.push((0, 1, 0, effect)).unwrap();
    shared.apply_effects();
    assert_eq!(shared.view.lock().unwrap().parts[0].ui_revision, 0);
    shared.effects.push((0, 2, 0, effect)).unwrap();
    shared.apply_effects();
    let published = shared.view.lock().unwrap();
    assert_eq!(published.parts[0].ui_revision, 1);
    assert_eq!(
        published.parts[0].updates[0].widgets[0]
            .1
            .value_text
            .as_deref(),
        Some("changed")
    );
}

#[test]
fn host_parameter_events_and_epoch_channel_run_the_saved_widget_callback_without_heap() {
    use sampler_core::{AutomationBinding, AutomationSource, ControlOperation, ControlRequest};
    let script = sampler_ksp::compile(
        "on init declare ui_knob $k(0,100,1) declare ui_knob $echo(0,1000,1) end on on ui_control($k) $echo := $k + 1 end on",
        48000, sampler_ksp::Limits::LIBRARY, &[],
    ).unwrap();
    let k = sampler_ui_ir::ControlId(script.controls()[0].definition.id.0);
    let echo = sampler_ui_ir::ControlId(script.controls()[1].definition.id.0);
    let ui_id = script.model().interface.widgets[0].ui_id;
    let prepared = script.bind(sampler_core::Prepared::new(48000, vec![], vec![], 0).unwrap()).unwrap()
        .with_automation_bindings(vec![AutomationBinding {
            source: AutomationSource::HostParameter(2048), source_slot: 0, ui_id,
            low: 0., high: 1., soft_takeover: false,
        }]).unwrap();
    let limits = sampler_core::Limits::for_plan(&prepared, 8, 0);
    let runtime = sampler_core::Runtime::new(prepared, limits).unwrap();
    let plan = runtime.active_plan();
    let revision = runtime.control_revision(plan).unwrap();
    let context = sampler_core::ControlContext { performance: runtime.performance(0).unwrap(),
        origin: sampler_core::ChannelAddress { protocol: sampler_core::Protocol::Midi1, port: 0, group: 0, channel: 0 }, channels: 1 };
    let mut part = CorePart::new(runtime, MixTree::instrument("host automation")).unwrap();
    let p = SamplerParams::new();
    p.shared.ensure_parts(1);
    let atoms = p.shared.part(0).unwrap();
    atoms.generation.store(1, Ordering::Release);
    atoms.loop_audit_install_ingress(part.ui_controls.take());
    let mut dsp = Dsp::default();
    dsp.core = V2Core::with_parts(1, 48000.);
    dsp.core.install(0, Some(Box::new(part)));
    let mut output = EventList::with_capacity(8);
    let transport = TransportInfo::default();
    let mut cx = ProcessContext::new(&transport, 48000., 16, &mut output);
    let event = Event::new(7, EventBody::ParamChange { id: automation::BASE + 2048, value: 0.42 });
    assert_eq!(tests::allocations(|| feed_typed_input(&mut dsp, &p, &event, &mut cx, false)), 0);
    dsp.core.render(16);
    assert_eq!((dsp.core.control_value(0, k), dsp.core.control_value(0, echo)), (Some(42.), Some(43.)));
    assert_eq!(dsp.unsupported, 0);
    assert!(!p.shared.set_host_parameter_at(0, 0, 2048, 0.6));
    assert!(!p.shared.set_host_parameter_at(0, 1, 2049, 0.6));
    assert!(p.shared.set_host_parameter_at(0, 1, 2048, 0.6));
    dsp.core.render(16);
    assert_eq!((dsp.core.control_value(0, k), dsp.core.control_value(0, echo)), (Some(60.), Some(61.)));
    let mut ingress = atoms.ingress.lock().unwrap();
    let ingress = ingress.as_mut().unwrap();
    let reply = ingress.client.reply().unwrap();
    assert!(reply.result.is_ok());
    ingress.client.submit(ControlRequest { plan, expected_revision: Some(revision),
        operation: ControlOperation::HostParameter(context, 2048, 0.8) }).unwrap();
    assert_eq!(tests::allocations(|| { dsp.core.render(16); }), 0);
    assert_eq!(ingress.client.reply().unwrap().result, Err(sampler_core::Error::RevisionConflict));
    assert_eq!(dsp.core.control_value(0, k), Some(60.));
}

#[test]
fn widget_meter_reads_native_bus_channel_without_heap_and_rejects_old_epoch() {
    use sampler_core::*;
    let pcm = Pcm::new(48000, vec![[0.2, 0.6]; 512].into_boxed_slice()).unwrap();
    let region = Region { sample:0,key_low:60,key_high:60,root_key:Some(60),velocity_low:0.,velocity_high:1.,gain:1.,
        envelope:Envelope::default(),playback:Playback::default() };
    let plan = Prepared::new(48000,vec![pcm],vec![region],128).unwrap()
        .with_buses(vec![sampler_core::Bus {processors:vec![],sends:vec![BusSend {bus:None,gain:1.}],tail_frames:0}],vec![Some(0)]).unwrap()
        .with_bus_addresses(vec![(7,0)]);
    let limits = Limits::for_plan(&plan,4,1);
    let mut tree = MixTree::instrument("meter");
    tree.nodes.push(crate::sound::tree::MixNode {name:"bus".into(),kind:crate::sound::tree::NodeKind::Group,parent:Some(0),inserts:vec![],sends:vec![]});
    let part = CorePart::new(Runtime::new(plan,limits).unwrap(),tree).unwrap();
    let mut core = V2Core::with_parts(1,48000.);
    core.install(0,Some(Box::new(part)));
    core.event(0,CoreEvent::Ump([0x2090_3c7f,0]));core.render(128);
    let atoms = PartShared::default();atoms.generation.store(1,Ordering::Release);
    let mut face = sampler_ui_ir::Interface::default();
    for channel in 0..2 {
        let mut widget = sampler_ui_ir::Widget::new("meter",sampler_ui_ir::PageRef(0),Default::default(),sampler_ui_ir::Kind::LevelMeter {orientation:sampler_ui_ir::Orientation::Vertical});
        widget.meter=Some(sampler_ui_ir::MeterAddress {group:999,slot:-1,channel,bus:Some(7)});
        face.widgets.push(widget);
    }
    assert!(atoms.widget_meters(&face,0).is_empty());
    atoms.widget_meters(&face,1);
    assert_eq!(tests::allocations(|| atoms.refresh_widget_meters(1,|address|core.widget_meter(0,address))),0);
    let values=atoms.widget_meters(&face,1);
    assert!(values[&sampler_ui_ir::WidgetRef(0)]>0.);
    assert!(values[&sampler_ui_ir::WidgetRef(1)]>values[&sampler_ui_ir::WidgetRef(0)]);
    let revision=atoms.scalar_revision.load(Ordering::Acquire);assert!(revision>0);
    atoms.refresh_widget_meters(0, |_|Some(0.));
    assert_eq!(atoms.widget_meters(&face,1),values);
    assert_eq!(atoms.scalar_revision.load(Ordering::Acquire),revision);
}

#[test]
fn waveform_provider_keeps_sparse_source_identity_plan_epoch_and_shared_display_bins() {
    use crate::sound::waveform::{Provider, Source};
    let prepared = sampler_core::Prepared::new(48000, vec![], vec![], 1).unwrap();
    let limits = sampler_core::Limits::for_plan(&prepared, 4, 1);
    let runtime = sampler_core::Runtime::new(prepared, limits).unwrap();
    let plan = runtime.active_plan();
    let mut part = CorePart::new(runtime, MixTree::instrument("waveform")).unwrap();
    let atoms = PartShared::default(); atoms.generation.store(1, Ordering::Release);
    atoms.loop_audit_install_ingress(part.ui_controls.take());
    let pcm = sampler_core::Pcm::new(4, vec![[-1.,0.], [0.5,0.],[-0.75,0.75],[0.,1.]].into_boxed_slice()).unwrap();
    let (wake, ready) = std::sync::mpsc::channel();
    *atoms.waveforms.lock().unwrap() = Some(Provider::start(plan, [(3, Source {pcm, stream:None})].into(), move || {wake.send(()).ok();}).unwrap());
    let mut face = sampler_ui_ir::Interface::default();
    face.source = sampler_ui_ir::Source::Ksp {slot:2};
    for zone in [3,3,9] {
        let mut widget = sampler_ui_ir::Widget::new("waveform", sampler_ui_ir::PageRef(0), sampler_ui_ir::Rect::new(0,0,4,20), sampler_ui_ir::Kind::Waveform);
        widget.waveform = Some(sampler_ui_ir::Waveform {zone,flags:0,cursor_us:0,table:vec![],highlighted:None,midi_start_note:0});
        face.widgets.push(widget);
    }
    assert!(atoms.widget_waveforms(&face,0,0.5).is_empty());
    let _ = atoms.widget_waveforms(&face,1,0.5);
    for _ in 0..2 { ready.recv_timeout(std::time::Duration::from_secs(5)).unwrap(); }
    let values = atoms.widget_waveforms(&face,1,0.5);
    assert_eq!(values.len(),2,"hole ID9 must not alias compact zone ordinal");
    assert_eq!(values[0].0,sampler_ui_ir::WidgetRef(0));
    assert_eq!(&*values[0].1.peaks,&[(-1.,0.5),(-0.75,1.)]);
    assert_eq!(values[0].1.duration_us,1_000_000);
    assert!(Arc::ptr_eq(&values[0].1.peaks,&values[1].1.peaks));
    atoms.generation.store(2,Ordering::Release);
    let _ = atoms.widget_waveforms(&face,1,0.5);
    atoms.loop_audit_install_ingress(None);
    assert!(atoms.widget_waveforms(&face,2,0.5).is_empty(),"unmatched Prepared generation cannot publish cached peaks");
}

#[test]
fn host_parameter_callback_changes_audio_at_the_exact_host_sample_offset() {
    use sampler_core::*;
    let script = sampler_ksp::compile("on init declare ui_knob $k(0,100,1) declare ui_knob $echo(0,1000,1) end on on ui_control($k) $echo := $k + 1 end on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
    let ui_id = script.model().interface.widgets[0].ui_id;
    let control = script.controls()[0].definition.id;
    let echo = sampler_ui_ir::ControlId(script.controls()[1].definition.id.0);
    let pcm = Pcm::new(48000,vec![[0.25;2];512].into_boxed_slice()).unwrap();
    let region = Region {sample:0,key_low:60,key_high:60,root_key:Some(60),velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::default(),playback:Playback::default()};
    let prepared = script.bind(Prepared::new(48000,vec![pcm],vec![region],128).unwrap()).unwrap()
        .with_automation_bindings(vec![AutomationBinding {source:AutomationSource::HostParameter(2048),source_slot:0,ui_id,low:0.,high:1.,soft_takeover:false}]).unwrap();
    let prepared = prepared.with_voice_chains(vec![VoiceChain::new(vec![],vec![Processor::ControlGain(ControlRange {control,low:0.,high:1.,ramp_frames:0})],0).unwrap()],vec![Some(0)]).unwrap();
    let limits = Limits::for_plan(&prepared,8,1);
    let part = CorePart::new(Runtime::new(prepared,limits).unwrap(),MixTree::instrument("sample-exact host")).unwrap();
    let p = SamplerParams::new();p.shared.ensure_parts(1);
    let mut dsp = Dsp::default();dsp.core=V2Core::with_parts(1,48000.);dsp.core.install(0,Some(Box::new(part)));
    let mut events = EventList::with_capacity(2);
    events.push(super::Event::new(0,EventBody::NoteOn {group:0,channel:0,note:60,velocity:127}));
    events.push(super::Event::new(7,EventBody::ParamChange {id:automation::BASE+2048,value:0.25}));
    let mut output_events = EventList::with_capacity(8);let transport = TransportInfo::default();
    let mut cx = ProcessContext::new(&transport,48000.,16,&mut output_events);
    let (mut left,mut right)=([0f32;16],[0f32;16]);
    let mut channels=[left.as_mut_slice(),right.as_mut_slice()];
    let mut buffer=AudioBuffer::from_slices_checked(&[],&mut channels,16);
    assert_eq!(tests::allocations(|| {Sampler::process(&mut dsp,&p,&mut buffer,&events,&mut cx);}),0);
    assert_eq!(dsp.core.control_value(0,echo),Some(26.));
    assert!(left[..7].iter().all(|sample|*sample==0.),"host callback must not run at block start");
    assert!(left[7]>0.,"host callback audio must start on sample7, without block quantization: samples={left:?} unsupported={} problems={:?}",dsp.unsupported,dsp.core.problems(0));
    assert!(left[7..].iter().all(|sample|*sample>0.));
    assert_eq!(left,right);
}
