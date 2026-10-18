//! Structural/early-admission proof for source-native recipes. No native data.
mod support;
use sampler_core::{Pcm,Error,lower::{lower,LowerError,Stage}};
use sampler_ir as ir;
fn owner(id:u32,parent:Option<u32>,kind:ir::SourceOwnerKind,children:&[u32],zones:&[usize])->ir::SourceOwner{
    ir::SourceOwner{id:ir::SourceOwnerId::UviNode(id),parent:parent.map(ir::SourceOwnerId::UviNode),kind,children:children.iter().map(|id|ir::SourceOwnerId::UviNode(*id)).collect(),groups:if zones.is_empty(){vec![]}else{vec![ir::GroupRef(0)]},zones:zones.iter().map(|z|ir::ZoneRef(*z)).collect(),predicates:vec![],bypass:None,mute:None,event_processors:vec![]}
}
fn graph(compiled:bool)->ir::Instrument{
    let mut instrument=ir::Instrument{source:ir::SourceFormat::Uvi,groups:vec![ir::Group::default()],..Default::default()};
    for n in 0..2{
        instrument.assets.push(ir::Asset{location:ir::AssetLocation::Path(format!("authored{n}.wav")),encoding:ir::Encoding::Wav,residency:ir::AssetResidency::Resident,root_key:None,loops:vec![]});
        let mut zone=ir::Zone::new(ir::AssetRef(n));zone.keys=ir::KeyRange{low:60,high:60};zone.group=Some(ir::GroupRef(0));instrument.zones.push(zone);
    }
    instrument.source_indices.owners=vec![owner(0,None,ir::SourceOwnerKind::Instrument,&[1],&[]),owner(1,Some(0),ir::SourceOwnerKind::Keygroup,&[2,3],&[0,1]),owner(2,Some(1),ir::SourceOwnerKind::Player,&[],&[0]),owner(3,Some(1),ir::SourceOwnerKind::Player,&[],&[1])];
    instrument.source_indices.processing.push(ir::SourceProcessingRecipe{name:"authored keygroup".into(),owner:ir::SourceOwnerId::UviNode(1),lifetime:ir::SourceProcessingLifetime::UviKeygroupActivation,members:vec![ir::ZoneRef(0),ir::ZoneRef(1)],chain:None,amplitude:None,output:None,compiled:compiled.then_some(0),parameter_roles:vec![],sources:vec![],routes:vec![],source_gate:None});
    if compiled{instrument.processing_scopes.push(ir::ProcessingScope{members:vec![ir::ZoneRef(0),ir::ZoneRef(1)],sum:ir::ProcessingSumLaw::OrderedF32,cadence:ir::ProcessingCadence::CoreCell64,chain:None,amplitude:None,output:None,signal:None});}instrument
}
fn pcm()->Vec<Pcm>{(0..2).map(|_|Pcm::new(48000,vec![[0.1;2];128].into_boxed_slice()).unwrap()).collect()}
#[test]fn unresolved_source_recipe_refuses_before_behavior_binding(){
    let mut ir=graph(false);ir.behaviors.push(ir::Behavior{name:"not-run".into(),language:ir::Language::Ksp,source:String::new(),slot:None,state:vec![],requires:vec![]});let bound=std::cell::Cell::new(false);
    let result=lower(&ir,48000,pcm(),|_,plan|{bound.set(true);Ok(plan)});assert!(matches!(result,Err(LowerError::Core{stage:Stage::Regions,error:Error::UnsupportedSourceLayout,..})));assert!(!bound.get());
}
#[test]fn resolved_owner_and_order_must_match_actual_scope(){
    let mut ir=graph(true);assert!(ir.validate().is_ok());ir.processing_scopes[0].members.swap(0,1);assert!(ir.validate().is_err());
    let mut ir=graph(true);ir.source_indices.processing[0].owner=ir::SourceOwnerId::UviNode(0);assert!(ir.validate().is_err());
}
#[test]fn narrowing_remaps_recipe_and_scope_without_erasing_native_owners(){
    let mut ir=graph(true);let ids:Vec<_>=ir.source_indices.owners.iter().map(|owner|owner.id).collect();ir.retain_zones(|zone|zone.asset==Some(ir::AssetRef(1)));assert_eq!(ir.source_indices.owners.iter().map(|owner|owner.id).collect::<Vec<_>>(),ids);assert_eq!(ir.source_indices.processing[0].members,vec![ir::ZoneRef(0)]);assert_eq!(ir.source_indices.processing[0].compiled,Some(0));assert_eq!(ir.processing_scopes[0].members,vec![ir::ZoneRef(0)]);assert!(ir.validate().is_ok());
}
#[test]fn source_send_cannot_disappear_through_group_tap_rewriting(){
    let mut ir=graph(true);ir.buses.push(ir::Bus{name:"actual-send".into(),chain:None,sends:vec![],output:ir::Output::Master,gain:ir::Gain::UNITY});ir.groups[0].sends.push(ir::GroupSend{to:ir::BusRef(0),gain:ir::Gain::Linear(0.5),pre_fader:true});let result=lower(&ir,48000,pcm(),|_,plan|Ok(plan));assert!(matches!(result,Err(LowerError::Core{stage:Stage::Regions,error:Error::UnsupportedSourceLayout,..})));
}
#[test]fn source_owner_bus_destination_is_one_owned_emission(){
    let mut ir=graph(true);ir.buses.push(ir::Bus{name:"destination".into(),chain:None,sends:vec![],output:ir::Output::Master,gain:ir::Gain::UNITY});ir.groups[0].output=ir::Output::Bus(ir::BusRef(0));ir.processing_scopes[0].output=Some(ir::BusRef(0));ir.source_indices.processing[0].output=Some(ir::BusRef(0));let plan=lower(&ir,48000,pcm(),|_,plan|Ok(plan)).unwrap();assert_eq!(plan.processing_scope_count(),1);
}

#[test]fn dormant_named_player_retains_scope_without_inventing_source_geometry(){
    let mut ir=graph(true);ir.zones[0].asset=None;ir.source_indices.sample_targets.push(ir::SourceSampleTarget{node:2,zones:vec![ir::ZoneRef(0)]});
    assert!(ir.validate().is_ok());let plan=lower(&ir,48000,pcm(),|_,plan|Ok(plan)).unwrap();assert_eq!(plan.processing_scope_count(),1);
    let limits=sampler_core::Limits::for_plan(&plan,4,4);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();let input=sampler_core::Input{protocol:sampler_core::Protocol::Native,port:0,group:0,channel:0,key:60,external_id:None};let note=rt.trigger(input,60,1.).unwrap();// Observe ownership before the authored128-frame source reaches EOF.
    let mut out=[[0.;2];64];rt.render(&mut out).unwrap();assert_eq!(rt.voice_count(),1);assert_eq!(rt.processing_owner_count(),1);assert_eq!(rt.stats().nonfinite_frames,0);rt.render(&mut out).unwrap();assert_eq!(rt.voice_count(),0);assert_eq!(rt.processing_owner_count(),0);rt.release(note).unwrap();
}

#[test]fn all_dormant_named_players_keep_shared_definition_without_fake_pcm(){
    let mut ir=graph(true);for (index,zone) in ir.zones.iter_mut().enumerate(){zone.asset=None;ir.source_indices.sample_targets.push(ir::SourceSampleTarget{node:2+index as u32,zones:vec![ir::ZoneRef(index)]});}
    assert!(ir.validate().is_ok());let plan=lower(&ir,48000,pcm(),|_,plan|Ok(plan)).unwrap();assert_eq!(plan.processing_scope_count(),1);
    let limits=sampler_core::Limits::for_plan(&plan,4,4);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();let input=sampler_core::Input{protocol:sampler_core::Protocol::Native,port:0,group:0,channel:0,key:60,external_id:None};let note=rt.trigger(input,60,1.).unwrap();let mut out=[[0.;2];128];rt.render(&mut out).unwrap();assert_eq!(out,[[0.;2];128]);assert_eq!(rt.voice_count(),0);assert_eq!(rt.stats().nonfinite_frames,0);rt.release(note).unwrap();
}

#[test]fn compiled_scope_uses_the_same_canonical_source_owner_table(){
    let ir=graph(true);let plan=lower(&ir,48000,pcm(),|_,plan|Ok(plan)).unwrap();let id=ir::SourceOwnerId::UviNode(1);let owner=plan.source_owner_index(id).unwrap();assert_eq!(plan.source_owners().get(owner).unwrap().id,id);assert_eq!(plan.processing_scope_source_owner(0),Some(id));assert_eq!(plan.processing_scope_source_owner(1),None);
}
#[test]fn generic_scope_has_no_invented_source_owner_identity(){
    let mut ir=graph(true);ir.source_indices.processing.clear();ir.source_indices.owners.clear();let plan=lower(&ir,48000,pcm(),|_,plan|Ok(plan)).unwrap();assert_eq!(plan.processing_scope_count(),1);assert_eq!(plan.processing_scope_source_owner(0),None);
}


/// The UVI crate forbids unsafe allocator hooks. Its seven normal-reader PCM
/// causals stay functional; this existing Core harness owns callback counting.
#[test]
fn n_port_matrix_mix_onepole_callbacks_and_live_edits_use_no_heap() {
    use sampler_core::{ChannelCount,SourceStorageFamily,ControlValue,ControlWrite,Input,Limits,Protocol,Runtime,SlotKind};
    let mut instrument=graph(true);
    let outer=ir::SlotAddress{group:-1,slot:-1,generic:1200};
    let matrix_mix=ir::SlotAddress{group:-1,slot:-1,generic:1201};
    // These are ordinary real slot definitions, not fabricated registration.
    instrument.chains.push(ir::Chain{scope:ir::Scope::Voice,pre_amplitude:vec![
        ir::Processor::Mix{count:0,address:outer,dry:0.,wet:1.,bypass:false},
        ir::Processor::Mix{count:0,address:matrix_mix,dry:0.,wet:1.,bypass:false},
    ],post_amplitude:vec![]});
    instrument.controls.push(ir::Control{key:"heap:upper".into(),label:"upper matrix cell".into(),
        value:ir::ControlValue::Continuous{min:-1.,max:1.,default:0.,unit:ir::ControlUnit::None},automation:ir::Automation::None});
    let mut cells=vec![0.;36];for channel in 0..6{cells[channel*6+channel]=1.;}
    instrument.processing_scopes[0].signal=Some(ir::ProcessingSignal{
        source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:6},
        nodes:vec![
            ir::ProcessingSignalNode::Mix(ir::ChannelMix{count:4,address:outer,dry:0.,wet:1.,bypass:false}),
            ir::ProcessingSignalNode::Mix(ir::ChannelMix{count:1,address:matrix_mix,dry:0.,wet:1.,bypass:false}),
            ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{input:ir::SignalLayout{channels:6},output:ir::SignalLayout{channels:6},cells,arithmetic:ir::MatrixArithmetic::SeparateMulAddF32}),
            ir::ProcessingSignalNode::Rectifier(ir::Rectifier::Full),
            ir::ProcessingSignalNode::OnePole(ir::ChannelOnePole{cutoff_hz:ir::SignalParameter{constant:1800.,control:None,expression:None},mode:ir::ChannelOnePoleMode::LowPass,arithmetic:ir::ChannelOnePoleArithmetic::CoreOnePoleF64,tail_frames:0}),
            ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{input:ir::SignalLayout{channels:6},output:ir::SignalLayout{channels:2},cells:vec![1.,0.,0.,0.,0.5,0.,0.,1.,0.,0.,0.,0.5],arithmetic:ir::MatrixArithmetic::SeparateMulAddF32}),
        ],controls:vec![ir::ProcessingCellBinding{node:2,input:2,output:4,control:"heap:upper".into()}],
    });
    let sources=(0..2).map(|member|{
        let packs=(0..3).map(|pack|Pcm::new(48000,(0..2048).map(|frame|{
            let sign=if frame%32<16{1.}else{-1.};
            [sign*(member+1) as f32*(pack*2+1) as f32/64.,sign*(member+1) as f32*(pack*2+2) as f32/64.]
        }).collect::<Vec<_>>().into_boxed_slice()).unwrap()).collect();
        SourceStorageFamily::new_resident(ChannelCount::new(6).unwrap(),packs).unwrap().into()
    }).collect();
    let plan=sampler_core::lower::lower_sources(&instrument,48000,sources,|_,plan|Ok(plan)).unwrap();
    let limits=Limits::for_plan(&plan,8,16);let mut rt=Runtime::new(plan,limits).unwrap();
    let input=Input{protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:Some(10)};
    let mut note=None;support::without_heap(||note=Some(rt.trigger(input,60,0.75).unwrap()));let note=note.unwrap();
    let plan=rt.active_plan();let upper=sampler_core::lower::ir_control_id("heap:upper");
    let inner=sampler_core::slot_control(SlotKind::Bypass,-1,-1,1201);
    let outer=sampler_core::slot_control(SlotKind::Bypass,-1,-1,1200);
    let mut block=[[0.;2];128];support::without_heap(||rt.render(&mut block).unwrap());
    assert!(block.iter().flatten().any(|sample|sample.abs()>1e-5));
    let mut owners=Vec::new();rt.visit_processing_owners(|owner|owners.push((owner.id,owner.note,owner.plan,owner.template,owner.sources)));
    for (id,value) in [(upper,0.25),(inner,1.),(inner,0.),(outer,1.),(outer,0.),(upper,-0.25)] {
        support::without_heap(||{
            rt.edit_controls(plan,None,&[ControlWrite{id,value:ControlValue::Real(value)}]).unwrap();
            for size in [31,64,33]{rt.render(&mut block[..size]).unwrap();}
        });
        assert!(block.iter().flatten().all(|sample|sample.is_finite()));
    }
    let mut after=Vec::new();rt.visit_processing_owners(|owner|after.push((owner.id,owner.note,owner.plan,owner.template,owner.sources)));assert_eq!(owners,after);
    support::without_heap(||{rt.release(note).unwrap();for _ in 0..32{rt.render(&mut block).unwrap();}});
    rt.flush_ended(|_|true);assert_eq!(rt.voice_count(),0);assert_eq!(rt.processing_owner_count(),0);
    let health=rt.stats();assert_eq!(health.nonfinite_frames,0);assert_eq!(health.stream_underruns,0);assert_eq!(health.refused_starts,0);assert_eq!(health.voice_drops,0);
}


fn scalar_pan_graph(expression_home:bool,ramp_frames:u32)->ir::Instrument {
    let mut instrument=graph(true);let pan=ir::Pan{position:0.25,law:ir::PanLaw::RaisedCosineCoreF64};
    instrument.chains.push(ir::Chain{scope:ir::Scope::Voice,pre_amplitude:vec![if expression_home{ir::Processor::ExpressionPan(ir::OwnedPan{pan,offset:0.})}else{ir::Processor::Pan(pan)}],post_amplitude:vec![]});
    instrument.controls.push(ir::Control{key:"scalar-pan".into(),label:"Pan".into(),value:ir::ControlValue::Continuous{min:-1.,max:1.,default:0.25,unit:ir::ControlUnit::None},automation:ir::Automation::None});
    instrument.processor_controls.push(ir::ProcessorControl{control:ir::ControlRef(0),chain:ir::ChainRef(0),index:0,parameter:ir::ProcessorParameter::Pan,ramp:ir::Time::Seconds(f64::from(ramp_frames)/48000.)});
    instrument.processing_scopes[0].chain=Some(ir::ChainRef(0));instrument.source_indices.processing[0].chain=Some(ir::ChainRef(0));instrument
}
fn scalar_pan_runtime(home:bool,ramp:u32)->sampler_core::Runtime {
    let instrument=scalar_pan_graph(home,ramp);
    let sources=(0..2).map(|_|Pcm::new(48000,vec![[0.1,0.2];2048].into_boxed_slice()).unwrap()).collect();
    let plan=lower(&instrument,48000,sources,|_,plan|Ok(plan)).unwrap();let limits=sampler_core::Limits::for_plan(&plan,8,16);sampler_core::Runtime::new(plan,limits).unwrap()
}
fn pan_input(external_id:i32)->sampler_core::Input {sampler_core::Input{protocol:sampler_core::Protocol::Native,port:0,group:0,channel:0,key:60,external_id:Some(external_id)}}
fn pan_oracle(position:f64,gain:f64)->[f32;2] {
    let far=(position.abs()*std::f64::consts::FRAC_PI_2).cos().powi(2);
    let coefficients=if position>0. {[far,1.]}else if position<0. {[1.,far]}else{[1.;2]};
    std::array::from_fn(|channel|((f64::from([0.2f32,0.4][channel])*coefficients[channel]) as f32)*gain as f32)
}
#[test]
fn shared_scalar_pan_ramps_position_before_curve_and_complete_recall() {
    use sampler_core::{ControlValue,ControlWrite,Expression};
    let mut rt=scalar_pan_runtime(true,4);let note=rt.trigger(pan_input(20),60,1.).unwrap();let expression=rt.expression_id(note).unwrap();rt.set_expression(expression,Expression{pan:0.2,..Default::default()}).unwrap();
    let plan=rt.active_plan();let id=sampler_core::lower::ir_control_id("scalar-pan");let empty=ControlWrite{id,value:ControlValue::Real(0.)};let mut snapshot=vec![empty;rt.control_definitions(plan).unwrap().len()];let (count,revision)=rt.capture_controls(plan,&mut snapshot).unwrap();assert_eq!(count,snapshot.len());
    let mut output=[[0.;2];8];support::without_heap(||{rt.edit_controls(plan,Some(revision),&[ControlWrite{id,value:ControlValue::Real(-0.5)}]).unwrap();rt.render(&mut output).unwrap();});
    for (frame,value) in output.iter().enumerate(){let p=0.25+(-0.75)*(frame.min(4) as f64/4.)+0.2;let expected=pan_oracle(p,1.);for channel in 0..2{assert!((value[channel]-expected[channel]).abs()<2e-6,"frame={frame} channel={channel}: {} vs {}",value[channel],expected[channel]);}}
    support::without_heap(||{rt.recall_controls(plan,None,&snapshot).unwrap();rt.render(&mut output).unwrap();});
    for (frame,value) in output.iter().enumerate(){let p=-0.5+0.75*(frame.min(4) as f64/4.)+0.2;let expected=pan_oracle(p,1.);for channel in 0..2{assert!((value[channel]-expected[channel]).abs()<2e-6);}}
    let revision=rt.control_revision(plan).unwrap();let now=rt.now();assert_eq!(rt.edit_controls(plan,None,&[ControlWrite{id,value:ControlValue::Real(1.01)}]),Err(Error::InvalidInput));assert_eq!(rt.now(),now);assert_eq!(rt.control_revision(plan).unwrap(),revision);
    rt.release(note).unwrap();rt.render(&mut [[0.;2];128]).unwrap();assert_eq!(rt.stats().nonfinite_frames,0);
}
#[test]
fn shared_pan_home_keeps_same_key_expression_ids_independent() {
    use sampler_core::Expression;
    let mut rt=scalar_pan_runtime(true,0);let a=rt.trigger(pan_input(21),60,1.).unwrap();let b=rt.trigger(pan_input(22),60,1.).unwrap();let a_expr=rt.expression_id(a).unwrap();let b_expr=rt.expression_id(b).unwrap();assert_ne!(a_expr,b_expr);
    rt.set_expressions(&[(a_expr,Expression{pan:0.25,..Default::default()}),(b_expr,Expression{pan:-0.5,..Default::default()})]).unwrap();let mut output=[[0.;2];64];support::without_heap(||rt.render(&mut output).unwrap());let x=pan_oracle(0.5,1.);let y=pan_oracle(-0.25,1.);
    for value in output{for channel in 0..2{assert!((value[channel]-(x[channel]+y[channel])).abs()<2e-6);}}
    rt.release(a).unwrap();rt.render(&mut [[0.;2];64]).unwrap();rt.set_expression(b_expr,Expression{pan:0.5,..Default::default()}).unwrap();rt.render(&mut output).unwrap();let expected=pan_oracle(0.75,1.);for value in output{for channel in 0..2{assert!((value[channel]-expected[channel]).abs()<2e-6);}}
    rt.release(b).unwrap();rt.render(&mut [[0.;2];128]).unwrap();assert_eq!(rt.stats().nonfinite_frames,0);
}
#[test]
fn ordinary_pan_insert_does_not_claim_canonical_expression_home() {
    use sampler_core::Expression;
    let mut rt=scalar_pan_runtime(false,0);let note=rt.trigger(pan_input(23),60,1.).unwrap();let expression=rt.expression_id(note).unwrap();rt.set_expression(expression,Expression{pan:0.25,..Default::default()}).unwrap();let mut out=[[0.;2];8];rt.render(&mut out).unwrap();let pan=pan_oracle(0.25,1.);for frame in out{assert!((frame[0]-pan[0]*0.75).abs()<2e-6);assert!((frame[1]-pan[1]).abs()<2e-6);}
    let mut instrument=scalar_pan_graph(true,0);let duplicate=instrument.chains[0].pre_amplitude[0];instrument.chains[0].pre_amplitude.push(duplicate);assert!(instrument.validate().is_err());
    let mut instrument=scalar_pan_graph(true,0);instrument.processing_scopes.clear();instrument.source_indices.processing.clear();instrument.zones[0].chain=Some(ir::ChainRef(0));assert!(instrument.validate().is_err());
}


#[test]
fn constant_pan_offset_and_base_are_independent_across_sibling_scopes() {
    use sampler_core::{ControlValue,ControlWrite};
    let mut instrument=scalar_pan_graph(true,0);
    instrument.source_indices.processing.clear();instrument.source_indices.owners.clear();
    let ir::Processor::ExpressionPan(home)=&mut instrument.chains[0].pre_amplitude[0] else{panic!("actual home")};home.offset=0.2;
    instrument.groups.push(ir::Group::default());
    for index in 0..2 {let mut asset=instrument.assets[index].clone();asset.location=ir::AssetLocation::Path(format!("sibling{index}.wav"));instrument.assets.push(asset);let mut zone=instrument.zones[index].clone();zone.asset=Some(ir::AssetRef(index+2));zone.group=Some(ir::GroupRef(1));zone.keys=ir::KeyRange{low:61,high:61};zone.pitch=ir::KeyTracking::Fixed;instrument.zones.push(zone);}
    let mut chain=instrument.chains[0].clone();let ir::Processor::ExpressionPan(home)=&mut chain.pre_amplitude[0] else{panic!("actual sibling home")};home.pan.position=-0.25;home.offset=-0.1;instrument.chains.push(chain);
    instrument.controls.push(ir::Control{key:"sibling-pan".into(),label:"Sibling Pan".into(),value:ir::ControlValue::Continuous{min:-1.,max:1.,default:-0.25,unit:ir::ControlUnit::None},automation:ir::Automation::None});
    instrument.processor_controls.push(ir::ProcessorControl{control:ir::ControlRef(1),chain:ir::ChainRef(1),index:0,parameter:ir::ProcessorParameter::Pan,ramp:ir::Time::ZERO});
    let mut scope=instrument.processing_scopes[0].clone();scope.members=vec![ir::ZoneRef(2),ir::ZoneRef(3)];scope.chain=Some(ir::ChainRef(1));instrument.processing_scopes.push(scope);
    let sources=(0..4).map(|_|Pcm::new(48000,vec![[0.1,0.2];2048].into_boxed_slice()).unwrap()).collect();let plan=lower(&instrument,48000,sources,|_,p|Ok(p)).unwrap();let limits=sampler_core::Limits::for_plan(&plan,8,16);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();
    let a=rt.trigger(pan_input(31),60,1.).unwrap();let b=rt.trigger(sampler_core::Input{key:61,..pan_input(32)},61,1.).unwrap();let mut out=[[0.;2];64];
    support::without_heap(||rt.render(&mut out).unwrap());let check=|out:&[[f32;2]],left:f64,right:f64|{let a=pan_oracle(left,1.);let b=pan_oracle(right,1.);for frame in out{for c in 0..2{assert!((frame[c]-a[c]-b[c]).abs()<2e-6);}}};check(&out,0.45,-0.35);
    let active=rt.active_plan();let id=sampler_core::lower::ir_control_id("scalar-pan");support::without_heap(||{rt.edit_controls(active,None,&[ControlWrite{id,value:ControlValue::Real(-0.5)}]).unwrap();rt.render(&mut out).unwrap();});check(&out,-0.3,-0.35);
    rt.release(a).unwrap();rt.render(&mut [[0.;2];64]).unwrap();rt.render(&mut out).unwrap();let expected=pan_oracle(-0.35,1.);for frame in out{for c in 0..2{assert!((frame[c]-expected[c]).abs()<2e-6);}}rt.release(b).unwrap();rt.render(&mut [[0.;2];128]).unwrap();assert_eq!(rt.stats().nonfinite_frames,0);
}

#[test]
fn static_unbound_pan_keeps_independent_matrix_pcm_and_no_heap() {
    for law in [ir::PanLaw::Balance,ir::PanLaw::EqualPower,ir::PanLaw::RaisedCosineCoreF64] {
        let position=0.375;
        let gains=match law{
            ir::PanLaw::Balance=>[1.-position,1.],
            ir::PanLaw::EqualPower=>{let angle=(position+1.)*std::f64::consts::FRAC_PI_4;[angle.cos(),angle.sin()]},
            ir::PanLaw::RaisedCosineCoreF64=>[(position*std::f64::consts::FRAC_PI_2).cos().powi(2),1.],
        };
        let runtime=|reference:bool|{
            let mut instrument=scalar_pan_graph(false,0);
            instrument.controls.clear();instrument.processor_controls.clear();
            instrument.chains[0].pre_amplitude[0]=if reference{ir::Processor::StereoMatrix([[gains[0],0.],[0.,gains[1]]])}else{ir::Processor::Pan(ir::Pan{position,law})};
            let sources=(0..2).map(|_|Pcm::new(48000,(0..2048).map(|frame|{let x=if frame%7<3{0.1}else{-0.2};[x,-x*0.5]}).collect::<Vec<_>>().into_boxed_slice()).unwrap()).collect();
            let plan=lower(&instrument,48000,sources,|_,p|Ok(p)).unwrap();let limits=sampler_core::Limits::for_plan(&plan,8,16);sampler_core::Runtime::new(plan,limits).unwrap()
        };
        let mut actual=runtime(false);let mut reference=runtime(true);let a=actual.trigger(pan_input(51),60,1.).unwrap();let b=reference.trigger(pan_input(51),60,1.).unwrap();
        let mut output=[[0.;2];128];let mut expected=[[0.;2];128];
        support::without_heap(||{actual.render(&mut output).unwrap();reference.render(&mut expected).unwrap();});
        for(a,b)in output.iter().zip(expected){for channel in 0..2{assert!((a[channel]-b[channel]).abs()<2e-6);}}
        assert!(output.iter().flatten().any(|value|value.abs()>0.01));
        support::without_heap(||{actual.release(a).unwrap();reference.release(b).unwrap();actual.render(&mut output).unwrap();reference.render(&mut expected).unwrap();});
        assert_eq!(actual.stats().nonfinite_frames,0);assert_eq!(reference.stats().nonfinite_frames,0);
    }
}

#[test]
fn pan_curve_reuse_preserves_exact_ramp_plateau_and_next_expression_event() {
    use sampler_core::{ControlValue,ControlWrite,Expression};
    let mut rt=scalar_pan_runtime(true,8);let note=rt.trigger(pan_input(52),60,1.).unwrap();let expression=rt.expression_id(note).unwrap();
    rt.set_expression(expression,Expression{pan:0.125,..Default::default()}).unwrap();
    let plan=rt.active_plan();let id=sampler_core::lower::ir_control_id("scalar-pan");let mut out=[[0.;2];64];
    support::without_heap(||{rt.edit_controls(plan,None,&[ControlWrite{id,value:ControlValue::Real(0.75)}]).unwrap();rt.render(&mut out).unwrap();});
    for(frame,value)in out.iter().enumerate(){let p=0.25+(0.5)*(frame.min(8)as f64/8.)+0.125;let expected=pan_oracle(p,1.);for channel in 0..2{assert!((value[channel]-expected[channel]).abs()<2e-6);}}
    // A fresh render call must read the new canonical expression, never reuse
    // the former call's last combined scalar or retain a mutable target cache.
    support::without_heap(||{rt.set_expression(expression,Expression{pan:-0.5,..Default::default()}).unwrap();rt.render(&mut out).unwrap();});
    let expected=pan_oracle(0.25,1.);for value in out{for channel in 0..2{assert!((value[channel]-expected[channel]).abs()<2e-6);}}
    rt.release(note).unwrap();rt.render(&mut [[0.;2];128]).unwrap();assert_eq!(rt.stats().nonfinite_frames,0);
}

// Global buses retain their own history across overlapping note lifetimes.
// The six-plane path uses an authored expansion and explicit output selection;
// its independent stereo reference has neither matrix nor six-plane workspace.
fn global_channel_runtime(wide:bool,filter:bool,pre_post:bool)->sampler_core::Runtime{
    global_channel_controlled_runtime(wide,filter,pre_post,None)
}
fn global_channel_controlled_runtime(wide:bool,filter:bool,pre_post:bool,mute:Option<bool>)->sampler_core::Runtime{
    global_channel_runtime_filter(wide,filter,pre_post,mute,1800.)
}
fn global_channel_runtime_filter(wide:bool,filter:bool,pre_post:bool,mute:Option<bool>,filter_hz:f64)->sampler_core::Runtime{
    let mut instrument=graph(true);
    instrument.source_indices=Default::default();instrument.processing_scopes.clear();
    for (index,zone) in instrument.zones.iter_mut().enumerate(){zone.keys=ir::KeyRange{low:60+index as u8,high:60+index as u8};zone.pitch=ir::KeyTracking::Fixed;}
    instrument.groups[0].output=ir::Output::Bus(ir::BusRef(0));
    instrument.buses.push(ir::Bus{name:"one real global owner".into(),chain:None,
        sends:if pre_post{vec![ir::Send{to:ir::Output::Master,gain:ir::Gain::UNITY,position:ir::SendPosition::PreChain},ir::Send{to:ir::Output::Master,gain:ir::Gain::UNITY,position:ir::SendPosition::PostChain}]}else{vec![]},
        output:ir::Output::Master,gain:if pre_post{ir::Gain::Linear(0.)}else{ir::Gain::UNITY}});
    instrument.controls.push(ir::Control{key:"global:gain".into(),label:"Global Gain".into(),value:ir::ControlValue::Continuous{min:0.,max:1.,default:1.,unit:ir::ControlUnit::None},automation:ir::Automation::None});
    let mut nodes=vec![];
    if wide{nodes.push(ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{input:ir::SignalLayout{channels:2},output:ir::SignalLayout{channels:6},cells:vec![0.25,0.,0.5,0.,1.,0.,0.,1.,0.,0.5,0.,0.25],arithmetic:ir::MatrixArithmetic::SeparateMulAddF32}));}
    nodes.push(ir::ProcessingSignalNode::Rectifier(ir::Rectifier::Full));
    if filter{nodes.push(ir::ProcessingSignalNode::OnePole(ir::ChannelOnePole{cutoff_hz:ir::SignalParameter{constant:filter_hz,control:None,expression:None},mode:ir::ChannelOnePoleMode::LowPass,arithmetic:ir::ChannelOnePoleArithmetic::CoreOnePoleF64,tail_frames:128}));}
    nodes.push(ir::ProcessingSignalNode::Gain(ir::ChannelGain{gain:ir::SignalParameter{constant:1.,control:Some("global:gain".into()),expression:None},arithmetic:ir::ChannelGainArithmetic::CoreGainF64ThenF32}));
    if wide{nodes.push(ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{input:ir::SignalLayout{channels:6},output:ir::SignalLayout{channels:2},cells:vec![0.,0.,1.,0.,0.,0.,0.,0.,0.,1.,0.,0.],arithmetic:ir::MatrixArithmetic::SeparateMulAddF32}));}
    if let Some(muted)=mute {
        instrument.controls.push(ir::Control{key:"global:mute".into(),label:"Global outlet Mute".into(),
            value:ir::ControlValue::Toggle{default:muted},automation:ir::Automation::None});
        nodes.push(ir::ProcessingSignalNode::Mute(ir::ChannelMute{muted,control:Some("global:mute".into())}));
    }
    instrument.processing_buses.push(ir::ProcessingBus{bus:ir::BusRef(0),owner:None,signal:ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:2},nodes,controls:vec![]},output:ir::SignalLayout{channels:2}});
    let sources=[0.1,-0.08].into_iter().map(|value|Pcm::new(48000,vec![[value;2];2048].into_boxed_slice()).unwrap()).collect();
    let plan=lower(&instrument,48000,sources,|_,p|Ok(p)).unwrap();let limits=sampler_core::Limits::for_plan(&plan,8,16);sampler_core::Runtime::new(plan,limits).unwrap()
}
#[test]
fn global_six_plane_history_is_one_owner_across_note_release_and_live_control(){
    use sampler_core::{ControlValue,ControlWrite};
    let mut actual=global_channel_runtime(true,true,false);let mut reference=global_channel_runtime(false,true,false);
    let a=actual.trigger(pan_input(41),60,1.).unwrap();let ar=reference.trigger(pan_input(41),60,1.).unwrap();
    let b=actual.trigger(sampler_core::Input{key:61,..pan_input(42)},61,1.).unwrap();let br=reference.trigger(sampler_core::Input{key:61,..pan_input(42)},61,1.).unwrap();
    let mut out=[[0.;2];64];let mut expected=[[0.;2];64];
    let equal=|a:&[[f32;2]],b:&[[f32;2]]|{for (a,b)in a.iter().zip(b){for c in 0..2{assert!((a[c]-b[c]).abs()<2e-6);}}};
    support::without_heap(||{actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();});equal(&out,&expected);
    // Rectification follows the note sum: separate rectifiers would approach
    // .18 instead of this roughly .02 plateau.
    assert!(out[63][0]>0.015 && out[63][0]<0.025);
    support::without_heap(||{actual.release(a).unwrap();reference.release(ar).unwrap();actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();});equal(&out,&expected);
    assert!(out[0][0]>0.015 && out[0][0]<0.08);assert!(out[63][0]>0.07);
    let id=sampler_core::lower::ir_control_id("global:gain");let ap=actual.active_plan();let rp=reference.active_plan();
    support::without_heap(||{actual.edit_controls(ap,None,&[ControlWrite{id,value:ControlValue::Real(0.5)}]).unwrap();reference.edit_controls(rp,None,&[ControlWrite{id,value:ControlValue::Real(0.5)}]).unwrap();actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();});equal(&out,&expected);assert!(out[63][0]>0.035 && out[63][0]<0.045);
    support::without_heap(||{actual.release(b).unwrap();reference.release(br).unwrap();actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();});equal(&out,&expected);assert!(out[0][0]>0.);
    for _ in 0..4{actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();equal(&out,&expected);}
    assert_eq!(actual.voice_count(),0);assert_eq!(actual.stats().nonfinite_frames,0);assert_eq!(reference.stats().nonfinite_frames,0);
}
#[test]
fn global_pre_chain_and_post_chain_taps_are_distinct_without_changing_source_clock(){
    for (key,value)in [(60u8,0.1f32),(61,-0.08)]{
        let mut rt=global_channel_runtime(false,false,true);let note=rt.trigger(sampler_core::Input{key,..pan_input(43)},key,1.).unwrap();let mut out=[[0.;2];64];let now=rt.now();
        support::without_heap(||rt.render(&mut out).unwrap());assert_eq!(rt.now(),now+64);
        let expected=value+value.abs();for frame in out{for channel in frame{assert!((channel-expected).abs()<2e-6);}}
        rt.release(note).unwrap();rt.render(&mut [[0.;2];64]).unwrap();assert_eq!(rt.stats().nonfinite_frames,0);
    }
}

fn bus_marker_graph()->ir::Instrument{
    let mut instrument=graph(true);instrument.source_indices=Default::default();instrument.processing_scopes.clear();
    instrument.groups[0].output=ir::Output::Bus(ir::BusRef(0));
    instrument.chains.push(ir::Chain{scope:ir::Scope::Bus(ir::BusRef(0)),pre_amplitude:vec![ir::Processor::Gain(ir::Gain::Linear(0.5))],post_amplitude:vec![]});
    instrument.buses.push(ir::Bus{name:"actual bus owner".into(),chain:Some(ir::ChainRef(0)),sends:vec![],output:ir::Output::Master,gain:ir::Gain::UNITY});
    instrument.processing_buses.push(ir::ProcessingBus{bus:ir::BusRef(0),owner:None,signal:ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:2},nodes:vec![ir::ProcessingSignalNode::BusChain],controls:vec![]},output:ir::SignalLayout{channels:2}});instrument
}
#[test]
fn bus_chain_marker_rejects_wrong_or_hidden_ownership_before_behavior_binding(){
    let valid=bus_marker_graph();assert!(valid.validate().is_ok());
    let mut cases=vec![];
    let mut duplicate=valid.clone();duplicate.processing_buses[0].signal.nodes.push(ir::ProcessingSignalNode::BusChain);cases.push(duplicate);
    let mut foreign=valid.clone();foreign.chains[0].scope=ir::Scope::Bus(ir::BusRef(1));cases.push(foreign);
    let mut missing=valid.clone();missing.processing_buses[0].signal.nodes=vec![ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{input:ir::SignalLayout{channels:2},output:ir::SignalLayout{channels:2},cells:vec![1.,0.,0.,1.],arithmetic:ir::MatrixArithmetic::SeparateMulAddF32})];cases.push(missing);
    let mut hidden=valid.clone();hidden.processing_buses[0].signal.nodes.insert(0,ir::ProcessingSignalNode::Mix(ir::ChannelMix{count:1,address:ir::SlotAddress{group:-1,slot:-1,generic:1300},dry:0.,wet:1.,bypass:false}));cases.push(hidden);
    let mut width=valid.clone();width.processing_buses[0].signal.input.channels=6;cases.push(width);
    let mut source=graph(true);source.processing_scopes[0].signal=Some(ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:2},nodes:vec![ir::ProcessingSignalNode::BusChain],controls:vec![]});cases.push(source);
    for mut instrument in cases{
        instrument.behaviors.push(ir::Behavior{name:"must not bind".into(),language:ir::Language::Ksp,source:String::new(),slot:None,state:vec![],requires:vec![]});
        let called=std::cell::Cell::new(false);assert!(instrument.validate().is_err());
        let result=lower(&instrument,48000,pcm(),|_,plan|{called.set(true);Ok(plan)});assert!(matches!(result,Err(LowerError::Invalid(_))));assert!(!called.get());
    }
}

fn bus_convolution_runtime(mode:u8)->sampler_core::Runtime {
    let mut instrument=bus_marker_graph();
    for(index,zone)in instrument.zones.iter_mut().enumerate(){zone.keys=ir::KeyRange{low:60+index as u8,high:60+index as u8};zone.pitch=ir::KeyTracking::Fixed;}
    let mut left=vec![0.;256];left[3]=0.5;left[70]=-0.25;left[190]=0.125;
    let mut right=vec![0.;256];right[3]=-0.25;right[71]=0.5;right[200]=-0.125;
    instrument.impulses.push(ir::Impulse{rate:48000,left,right,asset:None});
    instrument.chains[0].pre_amplitude=vec![ir::Processor::Mix{count:1,address:ir::SlotAddress{group:-1,slot:-1,generic:1500},dry:0.,wet:1.,bypass:false},ir::Processor::Space(ir::fx::space::Config::convolution(ir::ImpulseRef(0),0.25,0.75))];
    let expansion=||ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{input:ir::SignalLayout{channels:2},output:ir::SignalLayout{channels:6},cells:vec![0.25,0.,0.5,0.,1.,0.,0.,1.,0.,0.5,0.,0.25],arithmetic:ir::MatrixArithmetic::SeparateMulAddF32});
    let projection=||ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{input:ir::SignalLayout{channels:6},output:ir::SignalLayout{channels:2},cells:vec![0.,0.,1.,0.,0.,0.,0.,0.,0.,1.,0.,0.],arithmetic:ir::MatrixArithmetic::SeparateMulAddF32});
    if mode==1{instrument.processing_buses[0].signal.nodes=vec![expansion(),projection(),ir::ProcessingSignalNode::BusChain,expansion(),projection()];}
    else{instrument.processing_buses.clear();}
    if mode==2{instrument.groups[0].output=ir::Output::Master;instrument.buses.clear();instrument.chains.clear();instrument.impulses.clear();}
    let sources=(0..2).map(|member|Pcm::new(48000,(0..128).map(|frame|if frame<32{if member==0{[0.25,-0.125]}else{[-0.125,0.0625]}}else{[0.;2]}).collect::<Vec<_>>().into_boxed_slice()).unwrap()).collect();
    let plan=lower(&instrument,48000,sources,|_,p|Ok(p)).unwrap();let limits=sampler_core::Limits::for_plan(&plan,8,16);sampler_core::Runtime::new(plan,limits).unwrap()
}
#[test]
fn same_bus_chain_marker_preserves_convolution_controls_tails_idle_and_no_heap(){
    use sampler_core::{ControlValue,ControlWrite};
    let mut actual=bus_convolution_runtime(1);let mut reference=bus_convolution_runtime(0);let mut dry=bus_convolution_runtime(2);
    let n=actual.control_definitions(actual.active_plan()).unwrap().len();assert!(n>0);assert_eq!(n,reference.control_definitions(reference.active_plan()).unwrap().len());
    let seed=ControlWrite{id:sampler_core::ControlId(0),value:ControlValue::Real(0.)};let mut actual_saved=vec![seed;n];let mut reference_saved=vec![seed;n];
    actual.capture_controls(actual.active_plan(),&mut actual_saved).unwrap();reference.capture_controls(reference.active_plan(),&mut reference_saved).unwrap();
    let mut output=vec![[0.;2];32768];let mut expected=output.clone();let mut clean=output.clone();
    let a=actual.trigger(pan_input(61),60,1.).unwrap();let ar=reference.trigger(pan_input(61),60,1.).unwrap();let ad=dry.trigger(pan_input(61),60,1.).unwrap();let mut b=None;let mut br=None;let mut bd=None;
    let ap=actual.active_plan();let rp=reference.active_plan();let bypass=sampler_core::slot_control(sampler_core::SlotKind::Bypass,-1,-1,1500);
    support::without_heap(||{
        for block in 0..512 {
            if block==1{b=Some(actual.trigger(sampler_core::Input{key:61,..pan_input(62)},61,1.).unwrap());br=Some(reference.trigger(sampler_core::Input{key:61,..pan_input(62)},61,1.).unwrap());bd=Some(dry.trigger(sampler_core::Input{key:61,..pan_input(62)},61,1.).unwrap());}
            if block==2{actual.release(a).unwrap();reference.release(ar).unwrap();dry.release(ad).unwrap();actual.edit_controls(ap,None,&[ControlWrite{id:bypass,value:ControlValue::Real(1.)}]).unwrap();reference.edit_controls(rp,None,&[ControlWrite{id:bypass,value:ControlValue::Real(1.)}]).unwrap();}
            if block==3{actual.release(b.unwrap()).unwrap();reference.release(br.unwrap()).unwrap();dry.release(bd.unwrap()).unwrap();actual.recall_controls(ap,None,&actual_saved).unwrap();reference.recall_controls(rp,None,&reference_saved).unwrap();}
            actual.render(&mut output[block*64..block*64+64]).unwrap();reference.render(&mut expected[block*64..block*64+64]).unwrap();dry.render(&mut clean[block*64..block*64+64]).unwrap();
        }
    });
    for(frame,(a,b))in output.iter().zip(&expected).enumerate(){for c in 0..2{assert!((a[c]-b[c]).abs()<1e-5,"frame={frame} channel={c}: {} vs {}",a[c],b[c]);}}
    // Before live bypass starts, compare with a literal independent direct
    // convolution of the dry source PCM, not the shared chain implementation.
    let taps=[[(3usize,0.5f32),(70,-0.25),(190,0.125)],[(3,-0.25),(71,0.5),(200,-0.125)]];
    for frame in 0..128{for c in 0..2{let mut value=0.25*clean[frame][c];for(k,h)in taps[c]{if let Some(at)=frame.checked_sub(k){value+=0.75*h*clean[at][c];}}assert!((output[frame][c]-value).abs()<1e-5);}}
    assert!(output[128..400].iter().flatten().any(|value|value.abs()>1e-5));assert!(output[31744..].iter().flatten().all(|value|value.abs()<1e-8));assert_eq!(actual.voice_count(),0);assert_eq!(actual.stats().nonfinite_frames,0);assert_eq!(reference.stats().nonfinite_frames,0);
    let mut fresh=bus_convolution_runtime(0);let mut restarted=[[0.;2];128];let mut fresh_output=[[0.;2];128];
    actual.trigger(pan_input(63),60,1.).unwrap();fresh.trigger(pan_input(63),60,1.).unwrap();
    support::without_heap(||{actual.render(&mut restarted).unwrap();fresh.render(&mut fresh_output).unwrap();});
    for(a,b)in restarted.iter().zip(fresh_output){for c in 0..2{assert!((a[c]-b[c]).abs()<1e-5);}}
}


#[test]
fn output_mute_keeps_same_held_source_and_history_without_overwriting_gain(){
    use sampler_core::{ControlValue,ControlWrite};
    let mut actual=global_channel_runtime_filter(true,true,false,Some(true),200.);
    let mut reference=global_channel_runtime_filter(false,true,false,None,200.);
    let note=actual.trigger(pan_input(71),60,1.).unwrap();let other=reference.trigger(pan_input(71),60,1.).unwrap();
    let mut out=[[0.;2];64];let mut expected=out;
    let plan=actual.active_plan();let rp=reference.active_plan();
    let mute=sampler_core::lower::ir_control_id("global:mute");let gain=sampler_core::lower::ir_control_id("global:gain");
    let same=|a:&[[f32;2]],b:&[[f32;2]]|{for(a,b)in a.iter().zip(b){for c in 0..2{assert!((a[c]-b[c]).abs()<2e-6);}}};
    support::without_heap(||{actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();});
    assert_eq!(out,[[0.;2];64]);assert!(expected.iter().flatten().any(|v|v.abs()>0.05));assert_eq!(actual.voice_count(),1);
    let mut before=Vec::new();actual.visit_stream_demand(1,|demand|{before.push((demand.voice,demand.source_frame,demand.key.asset));true}).unwrap();assert_eq!(before.len(),1);
    let now=actual.now();let revision=actual.control_revision(plan).unwrap();
    assert_eq!(actual.edit_controls(plan,None,&[ControlWrite{id:mute,value:ControlValue::Real(0.)}]),Err(Error::InvalidInput));
    assert_eq!(actual.now(),now);assert_eq!(actual.control_revision(plan).unwrap(),revision);
    support::without_heap(||{
        actual.edit_controls(plan,None,&[ControlWrite{id:gain,value:ControlValue::Real(0.5)},ControlWrite{id:mute,value:ControlValue::Toggle(false)}]).unwrap();
        reference.edit_controls(rp,None,&[ControlWrite{id:gain,value:ControlValue::Real(0.5)}]).unwrap();
    });
    let mut after=Vec::new();actual.visit_stream_demand(1,|demand|{after.push((demand.voice,demand.source_frame,demand.key.asset));true}).unwrap();assert_eq!(before,after);
    support::without_heap(||{actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();});same(&out,&expected);
    assert!(out[63][0]>0.045 && out[63][0]<0.055);
    assert_eq!(actual.control_value(plan,gain),Ok(ControlValue::Real(0.5)));
    support::without_heap(||{
        actual.edit_controls(plan,None,&[ControlWrite{id:mute,value:ControlValue::Toggle(true)}]).unwrap();
        actual.release(note).unwrap();reference.release(other).unwrap();
        actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();
    });
    assert_eq!(out,[[0.;2];64]);assert!(expected[0][0]>0.);
    assert_eq!(actual.control_value(plan,gain),Ok(ControlValue::Real(0.5)));
    support::without_heap(||{actual.edit_controls(plan,None,&[ControlWrite{id:mute,value:ControlValue::Toggle(false)}]).unwrap();actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();});same(&out,&expected);assert!(out[0][0]>0.005);
    for _ in 0..4{actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();same(&out,&expected);}
    assert_eq!(actual.voice_count(),0);assert_eq!(actual.stats().nonfinite_frames,0);assert_eq!(actual.stats().stream_underruns,0);
}

#[test]
fn outlet_mute_position_leaves_explicit_pre_chain_send_independent(){
    use sampler_core::{ControlValue,ControlWrite};
    for(key,value)in [(60u8,0.1f32),(61,-0.08)]{
        let mut rt=global_channel_controlled_runtime(false,false,true,Some(true));
        let note=rt.trigger(sampler_core::Input{key,..pan_input(72)},key,1.).unwrap();let mut out=[[0.;2];64];
        support::without_heap(||rt.render(&mut out).unwrap());
        // Own main is zero-fader; explicit PreChain sends raw PCM while the
        // post-chain outlet is silenced. This is a declared Core send policy.
        for frame in out{for sample in frame{assert!((sample-value).abs()<2e-6);}}
        let plan=rt.active_plan();let id=sampler_core::lower::ir_control_id("global:mute");
        support::without_heap(||{rt.edit_controls(plan,None,&[ControlWrite{id,value:ControlValue::Toggle(false)}]).unwrap();rt.render(&mut out).unwrap();});
        for frame in out{for sample in frame{assert!((sample-(value+value.abs())).abs()<2e-6);}}
        rt.release(note).unwrap();rt.render(&mut [[0.;2];64]).unwrap();assert_eq!(rt.stats().nonfinite_frames,0);
    }
}

#[test]
fn outlet_mute_requires_real_toggle_domain_before_binding(){
    let valid=bus_marker_graph();
    for value in [ir::ControlValue::Continuous{min:0.,max:1.,default:0.,unit:ir::ControlUnit::None},ir::ControlValue::Integer{min:0,max:1,default:0}] {
        let mut instrument=valid.clone();instrument.controls.push(ir::Control{key:"not-toggle".into(),label:"wrong domain".into(),value,automation:ir::Automation::None});
        instrument.processing_buses[0].signal.nodes.push(ir::ProcessingSignalNode::Mute(ir::ChannelMute{muted:false,control:Some("not-toggle".into())}));
        assert!(instrument.validate().is_err());let called=std::cell::Cell::new(false);
        assert!(matches!(lower(&instrument,48000,pcm(),|_,plan|{called.set(true);Ok(plan)}),Err(LowerError::Invalid(_))));assert!(!called.get());
    }
}


#[test]
fn outlet_mute_and_gain_recall_restore_distinct_canonical_values(){
    use sampler_core::{ControlValue,ControlWrite};
    let mut actual=global_channel_controlled_runtime(false,false,false,Some(false));
    let mut reference=global_channel_runtime(false,false,false);
    let note=actual.trigger(pan_input(73),60,1.).unwrap();let other=reference.trigger(pan_input(73),60,1.).unwrap();
    let plan=actual.active_plan();let mute=sampler_core::lower::ir_control_id("global:mute");let gain=sampler_core::lower::ir_control_id("global:gain");
    let n=actual.control_definitions(plan).unwrap().len();let mut saved=vec![ControlWrite{id:gain,value:ControlValue::Real(0.)};n];
    let(count,_)=actual.capture_controls(plan,&mut saved).unwrap();assert_eq!(count,n);
    assert!(saved.iter().any(|write|write.id==mute && write.value==ControlValue::Toggle(false)));
    let mut out=[[0.;2];64];let mut expected=out;
    support::without_heap(||{
        actual.edit_controls(plan,None,&[ControlWrite{id:gain,value:ControlValue::Real(0.5)},ControlWrite{id:mute,value:ControlValue::Toggle(true)}]).unwrap();
        actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();
    });assert_eq!(out,[[0.;2];64]);assert_eq!(actual.control_value(plan,gain),Ok(ControlValue::Real(0.5)));
    support::without_heap(||{actual.recall_controls(plan,None,&saved).unwrap();actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();});
    assert_eq!(actual.control_value(plan,gain),Ok(ControlValue::Real(1.)));assert_eq!(actual.control_value(plan,mute),Ok(ControlValue::Toggle(false)));
    for(a,b)in out.iter().zip(expected){for c in 0..2{assert!((a[c]-b[c]).abs()<2e-6);}}
    actual.release(note).unwrap();reference.release(other).unwrap();actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();assert_eq!(actual.stats().nonfinite_frames,0);
}


fn channel_setup_plan(source:bool)->sampler_core::Prepared{
    use sampler_core::{Prepared,Region,Envelope,Playback,Bus,BusSend};
    let samples=vec![Pcm::new(48000,vec![[0.125,-0.25];512].into_boxed_slice()).unwrap()];
    let regions=if source{vec![Region{sample:0,key_low:60,key_high:60,root_key:None,
        velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::default(),playback:Playback::default()}]}else{vec![]};
    Prepared::new(48000,samples,regions,1).unwrap().with_buses(vec![Bus{processors:vec![],
        sends:vec![BusSend{bus:None,gain:1.}],tail_frames:0}],if source{vec![Some(0)]}else{vec![]}).unwrap()
}
fn channel_setup_definition(input:usize)->sampler_core::ChannelBus{
    let mut cells=vec![0.;input*2];cells[0]=0.5;cells[input+1]=0.5;
    sampler_core::ChannelBus{bus:0,owner:None,signal:ir::ProcessingSignal{
        source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,
        input:ir::SignalLayout{channels:input},nodes:vec![ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{
            input:ir::SignalLayout{channels:input},output:ir::SignalLayout{channels:2},cells,
            arithmetic:ir::MatrixArithmetic::SeparateMulAddF32})],controls:vec![]},
        output:sampler_core::ChannelCount::new(2).unwrap()}
}
#[test]
fn changing_pre_send_side_validates_actual_width_in_either_builder_order(){
    let typed=channel_setup_plan(false).with_channel_buses(vec![channel_setup_definition(6)],64*1024*1024).unwrap();
    // The post-chain6→2 conversion is valid. A raw PreChain send would feed
    // six planes to the stereo Master and is now rejected by the builder too.
    assert!(matches!(typed.with_bus_pre_sends(vec![(0,0)]),Err(Error::UnsupportedSourceLayout)));
    let tapped=channel_setup_plan(false).with_bus_pre_sends(vec![(0,0)]).unwrap();
    assert!(matches!(tapped.with_channel_buses(vec![channel_setup_definition(6)],64*1024*1024),Err(Error::UnsupportedSourceLayout)));
    // A truly matching stereo edge is still legal in both setup orders.
    channel_setup_plan(false).with_channel_buses(vec![channel_setup_definition(2)],64*1024*1024).unwrap().with_bus_pre_sends(vec![(0,0)]).unwrap();
    channel_setup_plan(false).with_bus_pre_sends(vec![(0,0)]).unwrap().with_channel_buses(vec![channel_setup_definition(2)],64*1024*1024).unwrap();
}
#[test]
fn stereo_frame_input_rejects_wide_declared_input_in_either_builder_order(){
    let typed=channel_setup_plan(false).with_channel_buses(vec![channel_setup_definition(6)],64*1024*1024).unwrap();
    assert!(matches!(typed.with_input_bus(0),Err(Error::UnsupportedSourceLayout)));
    let input=channel_setup_plan(false).with_input_bus(0).unwrap();
    assert!(matches!(input.with_channel_buses(vec![channel_setup_definition(6)],64*1024*1024),Err(Error::UnsupportedSourceLayout)));
}
#[test]
fn late_input_selection_reprices_against_retained_channel_workspace_ceiling(){
    let quoted=channel_setup_plan(false).with_channel_buses(vec![channel_setup_definition(2)],64*1024*1024).unwrap().channel_bus_workspace_bytes();
    assert!(quoted>0);
    let typed=channel_setup_plan(false).with_channel_buses(vec![channel_setup_definition(2)],quoted).unwrap();
    assert!(matches!(typed.with_input_bus(0),Err(Error::Capacity)));
    let input=channel_setup_plan(false).with_input_bus(0).unwrap();
    assert!(matches!(input.with_channel_buses(vec![channel_setup_definition(2)],quoted),Err(Error::Capacity)));
    let after=channel_setup_plan(false).with_channel_buses(vec![channel_setup_definition(2)],64*1024*1024).unwrap().with_input_bus(0).unwrap();
    let before=channel_setup_plan(false).with_input_bus(0).unwrap().with_channel_buses(vec![channel_setup_definition(2)],64*1024*1024).unwrap();
    assert!(after.channel_bus_workspace_bytes()>quoted);assert_eq!(before.channel_bus_workspace_bytes(),after.channel_bus_workspace_bytes());
}
#[test]
fn actual_stereo_frame_input_pcm_survives_both_legal_setup_orders_without_heap(){
    use sampler_core::{Limits,Runtime};
    for first in [true,false]{
        let plan=channel_setup_plan(true);
        let plan=if first{plan.with_input_bus(0).unwrap().with_channel_buses(vec![channel_setup_definition(2)],64*1024*1024).unwrap()}
            else{plan.with_channel_buses(vec![channel_setup_definition(2)],64*1024*1024).unwrap().with_input_bus(0).unwrap()};
        let limits=Limits::for_plan(&plan,4,8);let mut rt=Runtime::new(plan,limits).unwrap();
        let note=rt.trigger(pan_input(81),60,1.).unwrap();let mut out=[[0.;2];64];let start=rt.now();
        support::without_heap(||rt.render(&mut out).unwrap());assert_eq!(rt.now(),start+64);assert_eq!(rt.voice_count(),1);
        // Literal independent reference: ordinary Frame input [1/8,-1/4]
        // passes one explicitly authored diagonal1/2 matrix, with Tone bypassed.
        for frame in out{assert!((frame[0]-0.0625).abs()<2e-7);assert!((frame[1]+0.125).abs()<2e-7);}
        support::without_heap(||{rt.release(note).unwrap();rt.render(&mut out).unwrap();});
        assert_eq!(rt.stats().nonfinite_frames,0);assert_eq!(rt.stats().stream_underruns,0);
    }
}


fn global_schema_plan(source:bool)->sampler_core::Prepared{
    use sampler_core::{ControlDefinition,ControlDomain,ControlValue};
    let mute=sampler_core::lower::ir_control_id("api:mute");let cell=sampler_core::lower::ir_control_id("api:matrix");
    let plan=channel_setup_plan(source).with_controls(vec![
        ControlDefinition{id:mute,domain:ControlDomain::Toggle,default:ControlValue::Toggle(false)},
        ControlDefinition{id:cell,domain:ControlDomain::Real{min:-1.,max:1.},default:ControlValue::Real(0.5)},
    ]).unwrap();
    let mut definition=channel_setup_definition(2);
    definition.signal.nodes.push(ir::ProcessingSignalNode::Mute(ir::ChannelMute{muted:false,control:Some("api:mute".into())}));
    definition.signal.controls.push(ir::ProcessingCellBinding{node:0,input:0,output:0,control:"api:matrix".into()});
    plan.with_channel_buses(vec![definition],64*1024*1024).unwrap()
}
#[test]
fn replacing_schema_cannot_change_actual_global_signal_kind_or_range(){
    use sampler_core::{ControlDomain,ControlValue};
    let mute=sampler_core::lower::ir_control_id("api:mute");let cell=sampler_core::lower::ir_control_id("api:matrix");
    for case in 0..3 {
        let plan=global_schema_plan(false);let mut schema=plan.controls().to_vec();
        for row in &mut schema {
            if case==0 && row.id==mute{row.domain=ControlDomain::Real{min:0.,max:1.};row.default=ControlValue::Real(0.);}
            if case==1 && row.id==cell{row.domain=ControlDomain::Toggle;row.default=ControlValue::Toggle(false);}
            if case==2 && row.id==cell{row.domain=ControlDomain::Real{min:-2.,max:2.};row.default=ControlValue::Real(0.5);}
        }
        // Replacement owns an offaudio candidate. No Runtime/behavior binder
        // exists when a changed compiled global domain is refused.
        assert!(matches!(plan.with_controls(schema),Err(Error::InvalidInput)));
    }
}
#[test]
fn same_schema_new_defaults_preserve_typed_global_controls_and_literal_pcm(){
    use sampler_core::{ControlValue,ControlWrite,Limits,Runtime};
    let plan=global_schema_plan(true);let mute=sampler_core::lower::ir_control_id("api:mute");let cell=sampler_core::lower::ir_control_id("api:matrix");
    let mut schema=plan.controls().to_vec();
    for row in &mut schema{if row.id==mute{row.default=ControlValue::Toggle(true);}if row.id==cell{row.default=ControlValue::Real(-0.25);}}
    let plan=plan.with_controls(schema).unwrap().with_input_bus(0).unwrap();let limits=Limits::for_plan(&plan,4,8);let mut rt=Runtime::new(plan,limits).unwrap();let active=rt.active_plan();
    assert_eq!(rt.control_value(active,mute),Ok(ControlValue::Toggle(true)));assert_eq!(rt.control_value(active,cell),Ok(ControlValue::Real(-0.25)));
    let note=rt.trigger(pan_input(82),60,1.).unwrap();let mut out=[[0.;2];64];
    support::without_heap(||rt.render(&mut out).unwrap());assert_eq!(out,[[0.;2];64]);
    support::without_heap(||{rt.edit_controls(active,None,&[ControlWrite{id:mute,value:ControlValue::Toggle(false)}]).unwrap();rt.render(&mut out).unwrap();});
    for frame in out{assert!((frame[0]+0.03125).abs()<2e-7);assert!((frame[1]+0.125).abs()<2e-7);}
    assert_eq!(rt.control_value(active,cell),Ok(ControlValue::Real(-0.25)));rt.release(note).unwrap();rt.render(&mut out).unwrap();assert_eq!(rt.stats().nonfinite_frames,0);
}


// GBR05: runtime redirect must use the actual first-send tap's width.
fn redirected_pre_chain_runtime()->sampler_core::Runtime {
    use sampler_core::{Bus,BusSend,Prepared,Region,Envelope,Playback,Runtime,Limits,ChannelBus,ChannelCount};
    let source=Pcm::new(48000,vec![[0.125,-0.25];512].into_boxed_slice()).unwrap();
    let region=Region{sample:0,key_low:60,key_high:60,root_key:None,velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::default(),playback:Playback::default()};
    let buses=vec![Bus{processors:vec![],sends:vec![BusSend{bus:Some(1),gain:1.}],tail_frames:0},Bus{processors:vec![],sends:vec![BusSend{bus:Some(2),gain:1.}],tail_frames:0},Bus{processors:vec![],sends:vec![BusSend{bus:None,gain:1.}],tail_frames:0}];
    let matrix=|input:usize,output:usize,cells:Vec<f64>|ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{input:ir::SignalLayout{channels:input},output:ir::SignalLayout{channels:output},cells,arithmetic:ir::MatrixArithmetic::SeparateMulAddF32});
    let mut expansion=vec![0.;12];expansion[0]=1.;expansion[3]=1.;
    let mut projection=vec![0.;12];projection[0]=0.5;projection[7]=0.5;
    let definitions=vec![(0,2,6,expansion),(1,6,2,projection.clone()),(2,6,2,projection)].into_iter().map(|(bus,input,output,cells)|ChannelBus{bus,owner:None,signal:ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:input},nodes:vec![matrix(input,output,cells)],controls:vec![]},output:ChannelCount::new(output).unwrap()}).collect();
    let plan=Prepared::new(48000,vec![source],vec![region],1).unwrap().with_buses(buses,vec![Some(0)]).unwrap().with_bus_pre_sends(vec![(1,0)]).unwrap().with_input_bus(0).unwrap().with_channel_buses(definitions,64*1024*1024).unwrap();
    let limits=Limits::for_plan(&plan,4,8);Runtime::new(plan,limits).unwrap()
}
#[test]
fn redirect_refuses_six_plane_pre_chain_tap_without_mutating_stereo_route(){
    let mut rt=redirected_pre_chain_runtime();let now=rt.now();
    assert_eq!(rt.set_bus_mix(1,sampler_core::BusMix{gain:[1.;2],output:Some(0)}),Err(Error::UnsupportedSourceLayout));assert_eq!(rt.now(),now);
    let note=rt.trigger(pan_input(91),60,1.).unwrap();let mut out=[[0.;2];64];support::without_heap(||rt.render(&mut out).unwrap());
    // Only bus2's diagonal half-gain projection is on the audible PreChain route.
    for frame in out{assert!((frame[0]-0.0625).abs()<2e-7);assert!((frame[1]+0.125).abs()<2e-7);}assert_eq!(rt.stats().nonfinite_frames,0);rt.release(note).unwrap();
}

fn malformed_cell_definition(case:u8)->sampler_core::ChannelBus {
    let mut definition=channel_setup_definition(2);
    let node=match case{0=>99,1=>{definition.signal.nodes.push(ir::ProcessingSignalNode::Rectifier(ir::Rectifier::Full));1},_=>0};
    definition.signal.controls.push(ir::ProcessingCellBinding{node,input:0,output:0,control:"checked-cell".into()});
    if case==2{definition.signal.controls.push(ir::ProcessingCellBinding{node,input:0,output:0,control:"other-cell".into()});}
    definition
}
fn checked_cell_plan()->sampler_core::Prepared {
    use sampler_core::{ControlDefinition,ControlDomain,ControlValue};
    channel_setup_plan(true).with_controls(["checked-cell","other-cell"].into_iter().map(|key|ControlDefinition{id:sampler_core::lower::ir_control_id(key),domain:ControlDomain::Real{min:-1.,max:1.},default:ControlValue::Real(0.5)}).collect()).unwrap()
}
#[test]fn channel_builder_rejects_cell_binding_to_missing_node(){assert!(matches!(checked_cell_plan().with_channel_buses(vec![malformed_cell_definition(0)],64*1024*1024),Err(Error::InvalidInput)));}
#[test]fn channel_builder_rejects_cell_binding_to_non_matrix_node(){assert!(matches!(checked_cell_plan().with_channel_buses(vec![malformed_cell_definition(1)],64*1024*1024),Err(Error::InvalidInput)));}
#[test]fn channel_builder_rejects_duplicate_cell_without_last_writer_wins(){assert!(matches!(checked_cell_plan().with_channel_buses(vec![malformed_cell_definition(2)],64*1024*1024),Err(Error::InvalidInput)));}
#[test]
fn channel_builder_accepts_unique_cell_and_preserves_literal_pcm(){
    let mut definition=channel_setup_definition(2);definition.signal.controls.push(ir::ProcessingCellBinding{node:0,input:0,output:0,control:"checked-cell".into()});
    let plan=checked_cell_plan().with_channel_buses(vec![definition],64*1024*1024).unwrap().with_input_bus(0).unwrap();let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();
    rt.trigger(pan_input(92),60,1.).unwrap();let mut out=[[0.;2];64];support::without_heap(||rt.render(&mut out).unwrap());for frame in out{assert!((frame[0]-0.0625).abs()<2e-7);assert!((frame[1]+0.125).abs()<2e-7);}assert_eq!(rt.stats().nonfinite_frames,0);
}

fn independent_space_fault_runtime(typed:bool)->sampler_core::Runtime {
    use sampler_core::{Bus,BusSend,Prepared,Region,Envelope,Playback,Runtime,Limits,Processor,Impulse,ChannelBus,ChannelCount};
    let samples=vec![Pcm::new(48000,vec![[0.25;2]].into_boxed_slice()).unwrap(),Pcm::new(48000,vec![[0.125;2]].into_boxed_slice()).unwrap()];
    let regions=(0..2).map(|sample|Region{sample,key_low:60+sample as u8,key_high:60+sample as u8,root_key:None,velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::default(),playback:Playback::default()}).collect();
    let mut h=vec![0.;513];h[400]=1.;let impulse=Impulse::new(h.clone(),h).unwrap();
    let buses=vec![Bus{processors:vec![Processor::Gain(f64::MAX)],sends:vec![BusSend{bus:None,gain:1.}],tail_frames:0},Bus{processors:vec![Processor::Space(ir::fx::space::Config::convolution(ir::ImpulseRef(0),0.,1.))],sends:vec![BusSend{bus:None,gain:1.}],tail_frames:513}];
    let mut plan=Prepared::new(48000,samples,regions,2).unwrap().with_impulses(vec![impulse]).with_buses(buses,vec![Some(0),Some(1)]).unwrap();
    if typed{let definitions=(0..2).map(|bus|ChannelBus{bus,owner:None,signal:ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:2},nodes:vec![ir::ProcessingSignalNode::BusChain],controls:vec![]},output:ChannelCount::new(2).unwrap()}).collect();plan=plan.with_channel_buses(definitions,64*1024*1024).unwrap();}
    let limits=Limits::for_plan(&plan,4,8);Runtime::new(plan,limits).unwrap()
}
fn assert_fault_preserves_other_bus(typed:bool){
    let mut actual=independent_space_fault_runtime(typed);let mut reference=independent_space_fault_runtime(typed);let source=sampler_core::Input{key:61,..pan_input(93)};
    actual.trigger(source,61,1.).unwrap();reference.trigger(source,61,1.).unwrap();let mut out=[[0.;2];64];let mut expected=out;support::without_heap(||{actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();});
    actual.trigger(pan_input(94),60,1.).unwrap();let mut witnessed=false;
    support::without_heap(||{for block in 1..12{actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();for (f,(a,b))in out.iter().zip(expected).enumerate(){for c in 0..2{assert!((a[c]-b[c]).abs()<2e-6,"typed={typed} frame={} c={c}: {} vs {}",block*64+f,a[c],b[c]);}}if block==6{assert!((expected[16][0]-0.125).abs()<2e-6);witnessed=true;}}});
    assert!(witnessed);assert!(actual.stats().nonfinite_frames>0);assert_eq!(reference.stats().nonfinite_frames,0);assert_eq!(actual.voice_count(),0);
}
#[test]fn legacy_bus_fault_preserves_independently_seeded_convolution(){assert_fault_preserves_other_bus(false);}
#[test]fn bus_chain_fault_preserves_independently_seeded_convolution(){assert_fault_preserves_other_bus(true);}

#[test]
fn swapping_long_impulse_keeps_active_upstream_filter_tail_until_delayed_output(){
    use sampler_core::{Bus,BusSend,Prepared,Region,Envelope,Playback,Runtime,Limits,Processor,Impulse,ConvolutionUpload,ChannelBus,ChannelCount};
    let sample=Pcm::new(48000,vec![[0.25;2]].into_boxed_slice()).unwrap();let region=Region{sample:0,key_low:60,key_high:60,root_key:None,velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::default(),playback:Playback::default()};
    let short=Impulse::new(vec![1.],vec![1.]).unwrap();let plan=Prepared::new(48000,vec![sample],vec![region],1).unwrap().with_impulses(vec![short]).with_buses(vec![Bus{processors:vec![Processor::Space(ir::fx::space::Config::convolution(ir::ImpulseRef(0),0.,1.))],sends:vec![BusSend{bus:None,gain:1.}],tail_frames:1}],vec![Some(0)]).unwrap();
    let definition=ChannelBus{bus:0,owner:None,signal:ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:2},nodes:vec![ir::ProcessingSignalNode::OnePole(ir::ChannelOnePole{cutoff_hz:ir::SignalParameter{constant:100.,control:None,expression:None},mode:ir::ChannelOnePoleMode::LowPass,arithmetic:ir::ChannelOnePoleArithmetic::CoreOnePoleF64,tail_frames:1024}),ir::ProcessingSignalNode::BusChain],controls:vec![]},output:ChannelCount::new(2).unwrap()};
    let plan=plan.with_channel_buses(vec![definition],64*1024*1024).unwrap();let limits=Limits::for_plan(&plan,4,8);let mut rt=Runtime::new(plan,limits).unwrap();rt.trigger(pan_input(95),60,1.).unwrap();let mut audio=vec![[0.;2];4096];
    support::without_heap(||rt.render(&mut audio[..256]).unwrap());let mut h=vec![0.;2049];h[2048]=1.;let impulse=Impulse::new(h.clone(),h).unwrap();let mut upload=ConvolutionUpload::new(&impulse,0.,1.);
    support::without_heap(||{rt.swap_convolution(0,&mut upload).unwrap();rt.render(&mut audio[256..]).unwrap();});
    // Independent scalar recurrence and pure delayed FIR. New Space is empty
    // at F256; it must still receive this same owner's decaying OnePole state.
    let b=-(-std::f64::consts::TAU*100./48000.).exp_m1();let mut state=0.;let mut filtered=vec![0f32;4096];
    for (frame,value)in filtered.iter_mut().enumerate(){let input=if frame==0{0.25}else{0.};state+=(input-state)*b;*value=state as f32;}
    assert!(filtered[256]>1e-5);for frame in 2304..2560{for c in 0..2{assert!((audio[frame][c]-filtered[frame-2048]).abs()<3e-7,"frame={frame}: {} vs {}",audio[frame][c],filtered[frame-2048]);}}
    assert_eq!(rt.voice_count(),0);assert_eq!(rt.stats().nonfinite_frames,0);
    let mut idle=[[0.;2];4096];support::without_heap(||rt.render(&mut idle).unwrap());assert!(idle.iter().flatten().all(|value|value.abs()<1e-8));
}


fn stereo_pre_tap_with_wide_post_runtime()->sampler_core::Runtime {
    use sampler_core::{Bus,BusSend,ChannelBus,ChannelCount};
    let samples=vec![Pcm::new(48000,vec![[0.125,-0.25];512].into_boxed_slice()).unwrap()];
    let region=sampler_core::Region{sample:0,key_low:60,key_high:60,root_key:None,velocity_low:0.,velocity_high:1.,gain:1.,envelope:Default::default(),playback:Default::default()};
    let buses=vec![Bus{processors:vec![],sends:vec![BusSend{bus:Some(1),gain:1.},BusSend{bus:Some(2),gain:1.}],tail_frames:0},Bus{processors:vec![],sends:vec![BusSend{bus:None,gain:1.}],tail_frames:0},Bus{processors:vec![],sends:vec![BusSend{bus:None,gain:1.}],tail_frames:0}];
    let mut expand=vec![0.;12];expand[0]=1.;expand[3]=1.;let mut project=vec![0.;12];project[0]=0.25;project[7]=0.25;
    let definitions=vec![(0,2,6,expand),(1,2,2,vec![0.5,0.,0.,0.5]),(2,6,2,project)].into_iter().map(|(bus,input,output,cells)|ChannelBus{bus,owner:None,signal:ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:input},nodes:vec![ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{input:ir::SignalLayout{channels:input},output:ir::SignalLayout{channels:output},cells,arithmetic:ir::MatrixArithmetic::SeparateMulAddF32})],controls:vec![]},output:ChannelCount::new(output).unwrap()}).collect();
    let plan=sampler_core::Prepared::new(48000,samples,vec![region],1).unwrap().with_buses(buses,vec![Some(0)]).unwrap().with_bus_pre_sends(vec![(0,0)]).unwrap().with_input_bus(0).unwrap().with_channel_buses(definitions,64*1024*1024).unwrap();let limits=sampler_core::Limits::for_plan(&plan,4,8);sampler_core::Runtime::new(plan,limits).unwrap()
}
#[test]
fn stereo_pre_chain_redirect_accepts_wide_post_sibling_without_dropping_it(){
    let mut rt=stereo_pre_tap_with_wide_post_runtime();assert_eq!(rt.set_bus_mix(0,sampler_core::BusMix{gain:[0.5,1.],output:Some(0)}),Err(Error::UnsupportedSourceLayout));
    rt.set_bus_mix(0,sampler_core::BusMix{gain:[1.;2],output:Some(0)}).unwrap();rt.trigger(pan_input(96),60,1.).unwrap();let mut main=[[0.;2];64];let mut direct=main;
    support::without_heap(||rt.render_split(&mut main,&mut [&mut direct]).unwrap());
    for frame in main{assert!((frame[0]-0.03125).abs()<2e-7);assert!((frame[1]+0.0625).abs()<2e-7);}for frame in direct{assert!((frame[0]-0.125).abs()<2e-7);assert!((frame[1]+0.25).abs()<2e-7);}assert_eq!(rt.stats().nonfinite_frames,0);
}
#[test]
fn out_of_range_stereo_pre_redirect_keeps_original_edge_and_wide_sibling(){
    let mut rt=stereo_pre_tap_with_wide_post_runtime();rt.set_bus_mix(0,sampler_core::BusMix{gain:[1.;2],output:Some(99)}).unwrap();rt.trigger(pan_input(97),60,1.).unwrap();let mut main=[[0.;2];64];let mut direct=main;
    support::without_heap(||rt.render_split(&mut main,&mut [&mut direct]).unwrap());
    for frame in main{assert!((frame[0]-0.09375).abs()<2e-7);assert!((frame[1]+0.1875).abs()<2e-7);}assert_eq!(direct,[[0.;2];64]);assert_eq!(rt.stats().nonfinite_frames,0);
}


fn lower_final_wide_graph(pre_send:bool,input_bus:bool)->sampler_core::Runtime {
    let mut instrument=graph(true);instrument.source_indices=Default::default();instrument.groups[0].output=ir::Output::Bus(ir::BusRef(0));instrument.processing_scopes[0].output=Some(ir::BusRef(0));
    let mut expand=vec![0.;12];expand[0]=1.;expand[3]=1.;
    instrument.processing_scopes[0].signal=Some(ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:2},nodes:vec![ir::ProcessingSignalNode::Pan(ir::ChannelPan{offset:0.,position:ir::SignalParameter{constant:0.,control:None,expression:None},law:ir::PanLaw::Balance,expression:true}),ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{input:ir::SignalLayout{channels:2},output:ir::SignalLayout{channels:6},cells:expand,arithmetic:ir::MatrixArithmetic::SeparateMulAddF32})],controls:vec![]});
    instrument.buses.push(ir::Bus{name:"wide final input".into(),chain:None,sends:if pre_send{vec![ir::Send{to:ir::Output::Bus(ir::BusRef(1)),gain:ir::Gain::UNITY,position:ir::SendPosition::PreChain}]}else{vec![]},output:ir::Output::Master,gain:ir::Gain::UNITY});
    instrument.buses.push(ir::Bus{name:"wide pre tap".into(),chain:None,sends:vec![],output:ir::Output::Master,gain:ir::Gain::UNITY});
    instrument.buses.push(ir::Bus{name:"real independent stereo host port".into(),chain:None,sends:vec![],output:ir::Output::Master,gain:ir::Gain::UNITY});
    for (bus,gain)in [(0,0.5),(1,0.25)]{let mut cells=vec![0.;12];cells[0]=gain;cells[7]=gain;instrument.processing_buses.push(ir::ProcessingBus{bus:ir::BusRef(bus),owner:None,signal:ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:6},nodes:vec![ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{input:ir::SignalLayout{channels:6},output:ir::SignalLayout{channels:2},cells,arithmetic:ir::MatrixArithmetic::SeparateMulAddF32})],controls:vec![]},output:ir::SignalLayout{channels:2}});}
    if input_bus{instrument.input_bus=Some(ir::BusRef(2));}
    assert!(instrument.validate().is_ok());let plan=lower(&instrument,48000,pcm(),|_,p|Ok(p)).unwrap();let limits=sampler_core::Limits::for_plan(&plan,4,8);sampler_core::Runtime::new(plan,limits).unwrap()
}
fn assert_lower_wide_pcm(pre:bool,input:bool){
    let mut rt=lower_final_wide_graph(pre,input);rt.trigger(pan_input(98),60,1.).unwrap();let mut out=[[0.;2];64];support::without_heap(||rt.render(&mut out).unwrap());let expected=if pre{0.15}else{0.1};for frame in out{for c in frame{assert!((c-expected).abs()<3e-7);}}assert_eq!(rt.stats().nonfinite_frames,0);assert_eq!(rt.voice_count(),2);
}
#[test]fn normal_lower_wide_scope_final_graph_without_extra_configuration_is_positive(){assert_lower_wide_pcm(false,false);}
#[test]fn normal_lower_wide_scope_and_pre_tap_are_admitted_as_one_final_graph(){assert_lower_wide_pcm(true,false);}
#[test]fn normal_lower_wide_scope_and_stereo_host_input_are_admitted_as_one_final_graph(){assert_lower_wide_pcm(false,true);}


// A valid finite post-Space gain can fault only at the typed f32 outlet.
// The faulting owner's Space must be reset just as on the legacy bus path.
fn outer_fault_space_recovery_runtime(typed:bool)->sampler_core::Runtime {
    let mut instrument=ir::Instrument{source:ir::SourceFormat::Uvi,..Default::default()};
    for bus in 0..2{instrument.groups.push(ir::Group{output:ir::Output::Bus(ir::BusRef(bus)),..Default::default()});}
    for sample in 0..3{
        instrument.assets.push(ir::Asset{location:ir::AssetLocation::Path(format!("outer-fault-{sample}.wav")),encoding:ir::Encoding::Wav,residency:ir::AssetResidency::Resident,root_key:None,loops:vec![]});
        let mut zone=ir::Zone::new(ir::AssetRef(sample));zone.keys=ir::KeyRange{low:60+sample as u8,high:60+sample as u8};zone.pitch=ir::KeyTracking::Fixed;zone.velocity=ir::VelocityResponse::None;zone.group=Some(ir::GroupRef(if sample==2{1}else{0}));instrument.zones.push(zone);
    }
    let mut a=vec![0.;513];a[0]=1.;a[400]=1.;let mut b=vec![0.;513];b[400]=1.;
    for h in [a,b]{instrument.impulses.push(ir::Impulse{rate:48000,left:h.clone(),right:h,asset:None});}
    let key="outer-fault-scalar";
    instrument.controls.push(ir::Control{key:key.into(),label:"Actual finite recovery scalar".into(),value:ir::ControlValue::Continuous{min:0.,max:f64::from(f32::MAX),default:f64::from(f32::MAX),unit:ir::ControlUnit::None},automation:ir::Automation::None});
    for bus in 0..2{
        let mut processors=vec![ir::Processor::Space(ir::fx::space::Config::convolution(ir::ImpulseRef(bus),0.,1.))];
        if bus==0&&!typed{processors.extend([ir::Processor::Gain(ir::Gain::UNITY),ir::Processor::Gain(ir::Gain::Linear(8.))]);}
        instrument.chains.push(ir::Chain{scope:ir::Scope::Bus(ir::BusRef(bus)),pre_amplitude:processors,post_amplitude:vec![]});
        instrument.buses.push(ir::Bus{name:format!("independent Space owner{bus}"),chain:Some(ir::ChainRef(bus)),sends:vec![],output:ir::Output::Master,gain:ir::Gain::UNITY});
    }
    if typed{
        instrument.processing_buses.push(ir::ProcessingBus{bus:ir::BusRef(0),owner:None,signal:ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:2},nodes:vec![
            ir::ProcessingSignalNode::BusChain,
            ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{input:ir::SignalLayout{channels:2},output:ir::SignalLayout{channels:2},cells:vec![1.,0.,0.,1.],arithmetic:ir::MatrixArithmetic::SeparateMulAddF32}),
            ir::ProcessingSignalNode::Gain(ir::ChannelGain{gain:ir::SignalParameter{constant:8.,control:None,expression:None},arithmetic:ir::ChannelGainArithmetic::CoreGainF64ThenF32}),
        ],controls:vec![ir::ProcessingCellBinding{node:1,input:0,output:0,control:key.into()},ir::ProcessingCellBinding{node:1,input:1,output:1,control:key.into()}]},output:ir::SignalLayout{channels:2}});
    }else{
        instrument.processor_controls.push(ir::ProcessorControl{control:ir::ControlRef(0),chain:ir::ChainRef(0),index:1,parameter:ir::ProcessorParameter::Gain,ramp:ir::Time::ZERO});
    }
    assert!(instrument.validate().is_ok());
    let sources=vec![Pcm::new(48000,vec![[0.25;2]].into_boxed_slice()).unwrap(),Pcm::new(48000,vec![[0.;2];512].into_boxed_slice()).unwrap(),Pcm::new(48000,vec![[0.125;2]].into_boxed_slice()).unwrap()];
    let plan=lower(&instrument,48000,sources,|_,plan|Ok(plan)).unwrap();let limits=sampler_core::Limits::for_plan(&plan,8,8);sampler_core::Runtime::new(plan,limits).unwrap()
}
fn assert_outer_fault_clears_only_its_space(typed:bool){
    use sampler_core::{ControlValue,ControlWrite};
    let mut actual=outer_fault_space_recovery_runtime(typed);let mut reference=outer_fault_space_recovery_runtime(typed);
    let bad=actual.trigger(pan_input(141),60,1.).unwrap();let independent=sampler_core::Input{key:62,..pan_input(142)};
    actual.trigger(independent,62,1.).unwrap();reference.trigger(independent,62,1.).unwrap();
    let mut out=[[0.;2];64];let mut expected=out;
    support::without_heap(||{actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();});
    assert_eq!(out,[[0.;2];64]);assert_eq!(expected,[[0.;2];64]);assert!(actual.stats().nonfinite_frames>0,"the accepted finite fault wave must actually reach checked output refusal");assert_eq!(reference.stats().nonfinite_frames,0);
    assert!(actual.note(bad).unwrap().2,"physical parent remains held even though its one-frame source ended");
    let plan=actual.active_plan();let control=sampler_core::lower::ir_control_id("outer-fault-scalar");
    actual.edit_controls(plan,None,&[ControlWrite{id:control,value:ControlValue::Real(1.)}]).unwrap();assert_eq!(actual.control_value(plan,control),Ok(ControlValue::Real(1.)));
    let recovery=sampler_core::Input{key:61,..pan_input(143)};let held_zero=actual.trigger(recovery,61,1.).unwrap();reference.trigger(recovery,61,1.).unwrap();
    let faults=actual.stats().nonfinite_frames;let mut delayed_witness=false;
    support::without_heap(||{
        for block in 1..12{
            actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();
            for frame in 0..64{for channel in 0..2{
                let absolute=block*64+frame;let literal=if absolute==400{0.125}else{0.};
                assert!((expected[frame][channel]-literal).abs()<2e-6,"independent literal FIR reference frame={absolute}");
                assert!((out[frame][channel]-expected[frame][channel]).abs()<2e-6,"faulting Space history replayed: typed={typed} frame={absolute} c={channel} actual={} expected={}",out[frame][channel],expected[frame][channel]);
                if absolute==400{delayed_witness=true;}
            }}
            assert!(actual.note(held_zero).unwrap().2,"continuing zero source must not be replaced or retriggered to hide history");
        }
        let again=sampler_core::Input{external_id:Some(144),..recovery};actual.trigger(again,61,1.).unwrap();reference.trigger(again,61,1.).unwrap();
        for _ in 0..12{actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();for(frame,want)in out.iter().zip(expected){for c in 0..2{assert!((frame[c]-want[c]).abs()<2e-6);assert!(frame[c].abs()<2e-6);}}}
    });
    assert!(delayed_witness);assert_eq!(actual.stats().nonfinite_frames,faults,"successful recovery must not repeatedly fault on hidden old history");assert_eq!(reference.stats().nonfinite_frames,0);assert_eq!(actual.now(),reference.now());
    actual.panic();reference.panic();actual.flush_ended(|_|true);reference.flush_ended(|_|true);
}
#[test]fn legacy_fault_reset_clears_own_space_and_preserves_independent_tail_on_future_input(){assert_outer_fault_clears_only_its_space(false);}
#[test]fn typed_outer_fault_reset_clears_own_space_and_preserves_independent_tail_on_future_input(){assert_outer_fault_clears_only_its_space(true);}


fn explicit_output_instrument(fx:bool,declare:bool)->ir::Instrument {
    let mut instrument=graph(true);instrument.source_indices.processing.clear();instrument.processing_scopes.clear();instrument.groups[0].output=ir::Output::Bus(ir::BusRef(0));
    instrument.controls.push(ir::Control{key:"declared-owner-gain".into(),label:"actual operand".into(),value:ir::ControlValue::Continuous{min:0.,max:1.,default:0.5,unit:ir::ControlUnit::None},automation:ir::Automation::None});
    instrument.controls.push(ir::Control{key:"unrelated-fx-gain".into(),label:"not this operand".into(),value:ir::ControlValue::Continuous{min:0.,max:1.,default:0.75,unit:ir::ControlUnit::None},automation:ir::Automation::None});
    instrument.buses.push(ir::Bus{name:"same actual owner".into(),chain:None,sends:vec![],output:ir::Output::Master,gain:ir::Gain::UNITY});
    let mut nodes=vec![];if fx{nodes.push(ir::ProcessingSignalNode::Rectifier(ir::Rectifier::Full));}
    nodes.push(ir::ProcessingSignalNode::Gain(ir::ChannelGain{gain:ir::SignalParameter{constant:0.5,control:Some("declared-owner-gain".into()),expression:None},arithmetic:ir::ChannelGainArithmetic::CoreGainF64ThenF32}));
    instrument.processing_buses.push(ir::ProcessingBus{bus:ir::BusRef(0),owner:Some(ir::SourceOwnerId::UviNode(0)),signal:ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:2},nodes,controls:vec![]},output:ir::SignalLayout{channels:2}});
    if declare{instrument.source_indices.output_controls.push(ir::SourceOwnerOutputControl{owner:ir::SourceOwnerId::UviNode(0),field:ir::SourceOwnerOutputField::Gain,target:ir::SourceOwnerControlTarget::Bus{bus:ir::BusRef(0),node:usize::from(fx)},control:ir::ControlRef(0)});}instrument
}
fn explicit_output_plan(fx:bool)->sampler_core::Prepared{let instrument=explicit_output_instrument(fx,false);lower(&instrument,48000,pcm(),|_,p|Ok(p)).unwrap()}
fn explicit_output_definition()->sampler_core::SourceOwnerOutputControl{sampler_core::SourceOwnerOutputControl{owner:ir::SourceOwnerId::UviNode(0),field:sampler_core::SourceOwnerControl::Gain,target:sampler_core::SourceOwnerControlTarget::Bus{bus:0,node:0},control:sampler_core::lower::ir_control_id("declared-owner-gain")}}
#[test]
fn owned_fx_gain_without_declared_output_role_never_registers_scope_gain(){
    let plan=explicit_output_plan(false);let limits=sampler_core::Limits::for_plan(&plan,4,8);let rt=sampler_core::Runtime::new(plan,limits).unwrap();assert_eq!(rt.source_owner_control(rt.active_plan(),ir::SourceOwnerId::UviNode(0),sampler_core::SourceOwnerControl::Gain),Err(Error::InvalidInput));
}
#[test]
fn explicit_output_home_public_builder_rejects_foreign_missing_mismatched_and_duplicate_roles(){
    for case in 0..5{let plan=explicit_output_plan(false);let mut definition=explicit_output_definition();let mut definitions=vec![];
        match case{0=>definition.owner=ir::SourceOwnerId::UviNode(1),1=>definition.target=sampler_core::SourceOwnerControlTarget::Bus{bus:0,node:99},2=>definition.control=sampler_core::lower::ir_control_id("unrelated-fx-gain"),3=>definition.field=sampler_core::SourceOwnerControl::OutputMute,4=>definitions.push(definition),_=>unreachable!()};definitions.push(definition);
        assert!(matches!(plan.with_source_owner_output_controls(definitions,1024*1024),Err(Error::InvalidInput)),"case={case}");
    }
    let mut definition=explicit_output_definition();definition.target=sampler_core::SourceOwnerControlTarget::Bus{bus:0,node:1};
    assert!(matches!(explicit_output_plan(true).with_source_owner_output_controls(vec![definition],1024*1024),Err(Error::InvalidInput)),"nonlinear FX bus is not a declared memoryless outlet");
    assert!(matches!(explicit_output_plan(false).with_source_owner_output_controls(vec![explicit_output_definition()],0),Err(Error::Capacity)));
}
#[test]
fn malformed_ir_output_role_refuses_before_behavior_binding(){
    for case in 0..5{let mut instrument=explicit_output_instrument(false,true);let original=instrument.source_indices.output_controls[0];
        match case{0=>instrument.source_indices.output_controls.push(original),1=>instrument.source_indices.output_controls[0].owner=ir::SourceOwnerId::UviNode(1),2=>instrument.source_indices.output_controls[0].control=ir::ControlRef(1),3=>instrument.source_indices.output_controls[0].target=ir::SourceOwnerControlTarget::Bus{bus:ir::BusRef(0),node:99},4=>{instrument.processing_buses[0].signal.nodes.insert(0,ir::ProcessingSignalNode::Rectifier(ir::Rectifier::Full));instrument.source_indices.output_controls[0].target=ir::SourceOwnerControlTarget::Bus{bus:ir::BusRef(0),node:1};},_=>unreachable!()};
        assert!(instrument.validate().is_err(),"case={case}");let called=std::cell::Cell::new(false);assert!(matches!(lower(&instrument,48000,pcm(),|_,p|{called.set(true);Ok(p)}),Err(LowerError::Invalid(_))));assert!(!called.get());
    }
}
#[test]
fn declared_normal_output_gain_reaches_only_actual_operand_with_literal_pcm_and_no_heap(){
    use sampler_core::{ControlValue,ControlWrite,SourceOwnerControl};
    let instrument=explicit_output_instrument(false,true);assert!(instrument.validate().is_ok());let plan=lower(&instrument,48000,pcm(),|_,p|Ok(p)).unwrap();assert!(plan.source_owner_output_binding_bytes()>0);let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();let plan=rt.active_plan();let owner=ir::SourceOwnerId::UviNode(0);let id=rt.source_owner_control(plan,owner,SourceOwnerControl::Gain).unwrap();let unrelated=sampler_core::lower::ir_control_id("unrelated-fx-gain");assert_ne!(id,unrelated);assert_eq!(rt.source_owner_control(plan,owner,SourceOwnerControl::Pan),Err(Error::InvalidInput));rt.trigger(pan_input(99),60,1.).unwrap();let mut out=[[0.;2];32];support::without_heap(||rt.render(&mut out).unwrap());for frame in out{for value in frame{assert!((value-0.1).abs()<3e-7);}}
    support::without_heap(||{rt.edit_controls(plan,None,&[ControlWrite{id,value:ControlValue::Real(0.25)}]).unwrap();rt.render(&mut out).unwrap();});for frame in out{for value in frame{assert!((value-0.05).abs()<3e-7);}}assert_eq!(rt.control_value(plan,unrelated),Ok(ControlValue::Real(0.75)));assert_eq!(rt.stats().nonfinite_frames,0);assert_eq!(rt.voice_count(),2);
}
#[test]
fn explicit_role_does_not_implicitly_alias_generic_control_references(){
    let instrument=explicit_output_instrument(false,true);let plan=lower(&instrument,48000,pcm(),|_,p|Ok(p)).unwrap();let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();let plan=rt.active_plan();let source=rt.source_owner_control(plan,ir::SourceOwnerId::UviNode(0),sampler_core::SourceOwnerControl::Gain).unwrap();let other=sampler_core::lower::ir_control_id("unrelated-fx-gain");rt.edit_controls(plan,None,&[sampler_core::ControlWrite{id:other,value:sampler_core::ControlValue::Real(0.)}]).unwrap();assert_eq!(rt.control_value(plan,source),Ok(sampler_core::ControlValue::Real(0.5)));assert_eq!(rt.control_value(plan,other),Ok(sampler_core::ControlValue::Real(0.)));
}


#[test]
fn declared_output_gain_rejects_signed_domain_before_behavior_binding_but_generic_gain_remains_valid(){
    let mut instrument=explicit_output_instrument(false,true);
    instrument.controls[0].value=ir::ControlValue::Continuous{min:-1.,max:1.,default:0.5,unit:ir::ControlUnit::None};
    assert!(instrument.validate().is_err());
    let bound=std::cell::Cell::new(false);
    let result=lower(&instrument,48000,pcm(),|_,plan|{bound.set(true);Ok(plan)});
    assert!(matches!(result,Err(LowerError::Invalid(_))));assert!(!bound.get());
    instrument.source_indices.output_controls.clear();
    assert!(instrument.validate().is_ok(),"generic signed Gain has no source-output role restriction");
    let plan=lower(&instrument,48000,pcm(),|_,plan|Ok(plan)).unwrap();
    let limits=sampler_core::Limits::for_plan(&plan,4,8);
    let mut runtime=sampler_core::Runtime::new(plan,limits).unwrap();let plan=runtime.active_plan();
    runtime.edit_controls(plan,None,&[sampler_core::ControlWrite{id:sampler_core::lower::ir_control_id("declared-owner-gain"),value:sampler_core::ControlValue::Real(-0.25)}]).unwrap();
    runtime.trigger(pan_input(100),60,1.).unwrap();let mut output=[[0.;2];32];
    support::without_heap(||runtime.render(&mut output).unwrap());
    for frame in output{for value in frame{assert!((value+0.05).abs()<3e-7);}}
    assert_eq!(runtime.stats().nonfinite_frames,0);
}

// Scope roles are source identities joined to exact operands, not inferred
// from the presence of a Gain/Pan effect on an owned chain.
fn scoped_modulated_graph()->ir::Instrument {
    let mut instrument=scalar_pan_graph(true,0);
    instrument.chains[0].pre_amplitude.insert(0,ir::Processor::Color(ir::fx::color::Config::rectifier(ir::Rectifier::Full)));
    instrument.chains[0].pre_amplitude.insert(0,ir::Processor::Gain(ir::Gain::Linear(2.)));
    instrument.processor_controls[0].index=2;
    instrument.controls.push(ir::Control{key:"owner-gain".into(),label:"actual KG Gain".into(),value:ir::ControlValue::Continuous{min:0.,max:4.,default:2.,unit:ir::ControlUnit::None},automation:ir::Automation::None});
    instrument.processor_controls.push(ir::ProcessorControl{control:ir::ControlRef(1),chain:ir::ChainRef(0),index:0,parameter:ir::ProcessorParameter::Gain,ramp:ir::Time::ZERO});
    for zone in &mut instrument.zones{zone.velocity=ir::VelocityResponse::None;}
    for (field,index,control)in [(ir::SourceOwnerOutputField::Gain,0,1),(ir::SourceOwnerOutputField::Pan,2,0)] {
        instrument.source_indices.processing[0].parameter_roles.push(ir::SourceScopeControlRole{field,chain_index:index,control:ir::ControlRef(control),compiled:Some(ir::ScopeControlSite::ChainPre(index))});
        instrument.source_indices.output_controls.push(ir::SourceOwnerOutputControl{owner:ir::SourceOwnerId::UviNode(1),field,target:ir::SourceOwnerControlTarget::ProcessingScope{scope:0,site:ir::ScopeControlSite::ChainPre(index)},control:ir::ControlRef(control)});
    }
    instrument.modulators.push(ir::Modulator{scope:ir::Scope::Voice,source:ir::ModulationSource::Velocity});
    for (target,depth)in [(ir::Target::Pan,0.5),(ir::Target::Amplitude,1.)]{
        let index=instrument.routes.len();instrument.routes.push(ir::Route{source:ir::ModulatorRef(0),target,depth:ir::Depth::Normalized(depth),invert:false,shape:None,smoothing:ir::Time::ZERO,scale:None});
        instrument.source_indices.processing[0].routes.push(ir::RouteRef(index));
    }instrument
}
fn scoped_pcm()->Vec<Pcm>{vec![Pcm::new(48000,vec![[0.25,-0.375];4096].into_boxed_slice()).unwrap(),Pcm::new(48000,vec![[-0.125,0.125];4096].into_boxed_slice()).unwrap()]}
fn scoped_plan()->sampler_core::Prepared {
    sampler_core::lower::lower_with(&scoped_modulated_graph(),48000,scoped_pcm(),&sampler_core::lower::Options{mpe:None},|_,plan|Ok(plan)).unwrap()
}
fn scoped_oracle(velocity:f64,base_pan:f64,base_gain:f64)->[f32;2] {
    let sum:[f32;2]=[0.25f32+-0.125f32,-0.375f32+0.125f32];
    let pan=(base_pan+velocity*0.5).clamp(-1.,1.);
    let far=(pan.abs()*std::f64::consts::FRAC_PI_2).cos().powi(2);
    let gains=if pan>0.{[far,1.]}else if pan<0.{[1.,far]}else{[1.;2]};
    std::array::from_fn(|c|f64::from(((f64::from(sum[c])*base_gain*velocity)as f32).abs()).mul_add(gains[c],0.)as f32)
}
fn close_to(frame:[f32;2],expected:[f32;2]){for c in 0..2{assert!((frame[c]-expected[c]).abs()<3e-7,"channel {c}: {} vs {}",frame[c],expected[c]);}}
#[test]
fn shared_modulation_uses_one_real_owner_after_sum_and_public_absolute_homes() {
    use sampler_core::{ControlValue,ControlWrite,SourceOwnerControl};
    let plan=scoped_plan();let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();
    let owner=ir::SourceOwnerId::UviNode(1);let plan=rt.active_plan();
    assert_eq!(rt.source_owner_control(plan,owner,SourceOwnerControl::Pan).unwrap(),sampler_core::lower::ir_control_id("scalar-pan"));
    let note=rt.trigger(pan_input(401),60,0.5).unwrap();let mut out=[[0.;2];64];
    support::without_heap(||{rt.render(&mut out).unwrap();});for frame in out{close_to(frame,scoped_oracle(0.5,0.25,2.));}
    assert_eq!(rt.voice_count(),2);assert_eq!(rt.processing_owner_count(),1);
    let mut owners=Vec::new();rt.visit_processing_owners(|o|owners.push((o.id,o.note,o.plan)));
    let id=rt.source_owner_control(plan,owner,SourceOwnerControl::Pan).unwrap();
    support::without_heap(||{rt.edit_controls(plan,None,&[ControlWrite{id,value:ControlValue::Real(-0.75)}]).unwrap();rt.render(&mut out).unwrap();});
    for frame in out{close_to(frame,scoped_oracle(0.5,-0.75,2.));}
    let mut after=Vec::new();rt.visit_processing_owners(|o|after.push((o.id,o.note,o.plan)));assert_eq!(owners,after);
    assert_eq!(rt.stats().nonfinite_frames,0);rt.release(note).unwrap();rt.render(&mut out).unwrap();assert_eq!(rt.processing_owner_count(),0);
}
#[test]
fn same_key_scope_inputs_are_distinct_and_scope_without_amplitude_releases_and_reuses() {
    let plan=scoped_plan();let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();let mut out=[[0.;2];64];
    let a=rt.trigger(pan_input(402),60,0.5).unwrap();rt.render(&mut out[..31]).unwrap();
    let b=rt.trigger(pan_input(403),60,1.).unwrap();assert_ne!(a,b);rt.render(&mut out).unwrap();
    let first=scoped_oracle(0.5,0.25,2.);let second=scoped_oracle(1.,0.25,2.);let sum=std::array::from_fn(|c|first[c]+second[c]);
    for frame in out{close_to(frame,sum);}assert_eq!(rt.processing_owner_count(),2);
    rt.release(b).unwrap();rt.render(&mut out).unwrap();for frame in out{close_to(frame,first);}assert_eq!(rt.processing_owner_count(),1);
    rt.flush_ended(|_|true);let c=rt.trigger(pan_input(404),60,0.25).unwrap();rt.render(&mut out).unwrap();let third=scoped_oracle(0.25,0.25,2.);
    for frame in out{close_to(frame,std::array::from_fn(|c|first[c]+third[c]));}
    rt.release(c).unwrap();rt.release(a).unwrap();rt.render(&mut out).unwrap();assert_eq!(rt.processing_owner_count(),0);assert_eq!(rt.stats().nonfinite_frames,0);
}
#[test]
fn source_scoped_field_roles_reject_foreign_hidden_duplicate_and_wrong_domains_before_binder() {
    for malformed in 0..4 {
        let mut instrument=scoped_modulated_graph();match malformed {
            0=>instrument.source_indices.output_controls[0].owner=ir::SourceOwnerId::UviNode(2),
            1=>{let row=instrument.source_indices.output_controls[0];instrument.source_indices.output_controls.push(row);},
            2=>instrument.controls[1].value=ir::ControlValue::Continuous{min:-1.,max:4.,default:2.,unit:ir::ControlUnit::None},
            _=>{instrument.source_indices.output_controls[1].target=ir::SourceOwnerControlTarget::ProcessingScope{scope:0,site:ir::ScopeControlSite::ChainPre(1)};},
        }
        instrument.behaviors.push(ir::Behavior{name:"never".into(),language:ir::Language::Ksp,source:String::new(),slot:None,state:vec![],requires:vec![]});
        let calls=std::cell::Cell::new(0);
        assert!(sampler_core::lower::lower_with(&instrument,48000,scoped_pcm(),&sampler_core::lower::Options{mpe:None},|_,plan|{calls.set(calls.get()+1);Ok(plan)}).is_err());assert_eq!(calls.get(),0);
    }
}
#[test]
fn scope_state_survives_physical_growth_and_old_plan_adoption_without_growing_owner_arena() {
    let plan=scoped_plan();let limits=sampler_core::Limits::for_plan(&plan,8,4);let(mut rt,mut control)=sampler_core::Runtime::with_plan_updates(plan,limits,3,2).unwrap();
    let original_capacity=rt.processing_owner_capacity();let old=rt.active_plan();let a=rt.trigger(pan_input(405),60,0.5).unwrap();let mut out=[[0.;2];64];rt.render(&mut out[..31]).unwrap();
    control.grow_voices(16).unwrap();support::without_heap(||rt.render(&mut out).unwrap());assert_eq!(rt.voice_capacity(),16);assert_eq!(rt.processing_owner_capacity(),original_capacity);for frame in out{close_to(frame,scoped_oracle(0.5,0.25,2.));}
    control.collect_growth_returns();let request=control.submit(Box::new(scoped_plan())).unwrap();rt.render(&mut out).unwrap();assert_eq!(rt.poll_plan_update(),Ok(Some(request)));assert_ne!(rt.active_plan(),old);assert_eq!(rt.processing_owner_capacity(),original_capacity);
    for frame in out{close_to(frame,scoped_oracle(0.5,0.25,2.));}
    let b=rt.trigger(pan_input(406),60,1.).unwrap();rt.render(&mut out).unwrap();let p=scoped_oracle(0.5,0.25,2.);let q=scoped_oracle(1.,0.25,2.);for frame in out{close_to(frame,std::array::from_fn(|c|p[c]+q[c]));}
    rt.release(a).unwrap();rt.release(b).unwrap();rt.render(&mut out).unwrap();rt.flush_ended(|_|true);assert_eq!(rt.processing_owner_count(),0);assert_eq!(rt.stats().nonfinite_frames,0);
}

#[test]
fn scope_modulator_gate_releases_after_last_source_without_an_amplitude_stage() {
    let mut instrument=scoped_modulated_graph();
    instrument.chains[0].pre_amplitude.insert(0,ir::Processor::Delay{time:ir::Time::Seconds(256./48000.),feedback:0.,mix:1.});
    for binding in &mut instrument.processor_controls{binding.index+=1;}
    for role in &mut instrument.source_indices.processing[0].parameter_roles{role.chain_index+=1;role.compiled=Some(ir::ScopeControlSite::ChainPre(role.chain_index));}
    for home in &mut instrument.source_indices.output_controls{if let ir::SourceOwnerControlTarget::ProcessingScope{site:ir::ScopeControlSite::ChainPre(index),..}=&mut home.target{*index+=1;}}
    let envelope=instrument.modulators.len();instrument.modulators.push(ir::Modulator{scope:ir::Scope::Voice,source:ir::ModulationSource::Envelope(ir::Envelope{release:ir::Time::Seconds(128./48000.),..Default::default()})});
    let route=instrument.routes.len();instrument.routes.push(ir::Route{source:ir::ModulatorRef(envelope),target:ir::Target::Amplitude,depth:ir::Depth::Normalized(1.),invert:false,shape:None,smoothing:ir::Time::ZERO,scale:None});instrument.source_indices.processing[0].routes.push(ir::RouteRef(route));
    assert!(instrument.processing_scopes[0].amplitude.is_none());
    let create=||{
        let pcm=vec![Pcm::new(48000,vec![[0.25,-0.375];16].into_boxed_slice()).unwrap(),Pcm::new(48000,vec![[-0.125,0.125];16].into_boxed_slice()).unwrap()];
        let plan=sampler_core::lower::lower_with(&instrument,48000,pcm,&sampler_core::lower::Options{mpe:None},|_,plan|Ok(plan)).unwrap();let limits=sampler_core::Limits::for_plan(&plan,4,8);sampler_core::Runtime::new(plan,limits).unwrap()
    };
    let mut released=create();let mut held=create();let note=released.trigger(pan_input(407),60,0.5).unwrap();held.trigger(pan_input(407),60,0.5).unwrap();
    let mut prefix=[[0.;2];20];released.render(&mut prefix).unwrap();held.render(&mut prefix).unwrap();assert_eq!(released.voice_count(),0);assert_eq!(released.processing_owner_count(),1,"actual shared delayed history still owns the logical note");
    released.release(note).unwrap();let mut a=[[0.;2];280];let mut b=[[0.;2];280];support::without_heap(||{released.render(&mut a).unwrap();held.render(&mut b).unwrap();});
    assert!(b[236..252].iter().flatten().any(|sample|sample.abs()>1e-3),"independent held gate must reveal the delayed impulse");
    assert!(a[236..252].iter().flatten().all(|sample|sample.abs()<1e-7),"released scope envelope must silence its delayed output even with has_amplitude=false");
    assert_eq!(released.processing_owner_count(),0);assert_eq!(released.stats().nonfinite_frames,0);
}

#[test]
fn scoped_sine_scalar_interpolates_before_pan_law_at_absolute_cell_points() {
    use sampler_core::{Lfo,LfoRate,LfoShape,ModProgram,ModRoute,ModSource,ModTarget};
    let instrument=scoped_modulated_graph();let plan=sampler_core::lower::lower_with(&instrument,48000,scoped_pcm(),&sampler_core::lower::Options{mpe:None},|_,plan|Ok(plan)).unwrap();
    let program=ModProgram{sources:vec![ModSource::Lfo(Lfo{shape:LfoShape::Sine,rate:LfoRate::Hertz(23.7),phase:0.,delay:0,fade:0,retrigger:true,shared:false})],routes:vec![ModRoute::new(0,ModTarget::Pan,0.6)],..Default::default()};
    let plan=plan.with_processing_modulation(vec![program],vec![Some(0)],1<<20).unwrap();let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();rt.trigger(pan_input(408),60,0.5).unwrap();
    let mut actual=[[0.;2];509];let mut at=0;for n in [17,64,3,127,31,64,64,64,64,11]{let end=(at+n).min(actual.len());if at<end{rt.render(&mut actual[at..end]).unwrap();at=end;}}assert_eq!(at,actual.len());
    // This is a math oracle, not a call to the production LFO/router/curve.
    for (at,frame) in actual.into_iter().enumerate(){
        let begin=at/64*64;let end=begin+64;let f=|t:usize|0.6*(std::f64::consts::TAU*23.7*t as f64/48000.).sin();
        let routed=f(begin)+(f(end)-f(begin))*(at-begin)as f64/64.;
        let p=(0.25+routed).clamp(-1.,1.);let far=(p.abs()*std::f64::consts::FRAC_PI_2).cos().powi(2);let gains=if p>0.{[far,1.]}else if p<0.{[1.,far]}else{[1.;2]};
        let sum:[f32;2]=[0.125,-0.25];let expected=std::array::from_fn(|c|(f64::from((sum[c]*2.).abs())*gains[c])as f32);
        close_to(frame,expected);
    }
    assert_eq!(rt.processing_owner_count(),1);assert_eq!(rt.stats().nonfinite_frames,0);
}

fn scope_schema_plan()->sampler_core::Prepared {
    use sampler_core::{ControlDefinition,ControlDomain,ControlId,ControlValue,Envelope,ModProgram,ModRoute,ModSource,ModTarget};
    let plan=scoped_plan();let mut definitions=plan.controls().to_vec();
    for (id,max,default)in [(11,1.,0.5),(13,1.,0.),(17,128.,64.)]{definitions.push(ControlDefinition{id:ControlId(id),domain:ControlDomain::Real{min:0.,max},default:ControlValue::Real(default)});}
    let plan=plan.with_controls(definitions).unwrap();
    let program=ModProgram{sources:vec![ModSource::Envelope(Envelope::new(0,0,0,1.,64).unwrap())],routes:vec![ModRoute::new(0,ModTarget::Pan,1.)],
        controls:vec![(0,[ControlId(11),ControlId(13)])],envelope_controls:vec![(0,[None,None,None,None,Some(ControlId(17)),None])],..Default::default()};
    plan.with_processing_modulation(vec![program],vec![Some(0)],1<<20).unwrap()
}
#[test]
fn late_schema_replacement_remaps_scoped_route_and_envelope_controls_by_identity() {
    use sampler_core::{ControlDefinition,ControlDomain,ControlId,ControlValue};
    let plan=scope_schema_plan();let mut definitions=plan.controls().to_vec();
    definitions.push(ControlDefinition{id:ControlId(0),domain:ControlDomain::Real{min:-8.,max:8.},default:ControlValue::Real(7.)});definitions.reverse();
    let plan=plan.with_controls(definitions).unwrap();let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();rt.trigger(pan_input(409),60,0.5).unwrap();
    let mut out=[[0.;2];64];support::without_heap(||rt.render(&mut out).unwrap());for frame in out{close_to(frame,scoped_oracle(1.,0.25,2.));}
    assert_eq!(rt.stats().nonfinite_frames,0);
    for id in [11,13,17]{
        let plan=scope_schema_plan();let mut definitions=plan.controls().to_vec();let row=definitions.iter_mut().find(|row|row.id==ControlId(id)).unwrap();row.domain=ControlDomain::Toggle;row.default=ControlValue::Toggle(false);
        assert!(matches!(plan.with_controls(definitions),Err(Error::InvalidInput)),"scoped real input {id} cannot silently become another typed cell");
    }
    let plan=scope_schema_plan();let definitions=plan.controls().iter().filter(|row|row.id!=ControlId(17)).copied().collect();assert!(matches!(plan.with_controls(definitions),Err(Error::InvalidInput)));
}
#[test]
fn scoped_modulation_workspace_limit_refuses_before_state_allocation_and_does_not_touch_live_plan() {
    use sampler_core::{ModProgram,ModRoute,ModSource,ModTarget};
    let plan=scoped_plan();let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();let active=rt.active_plan();rt.trigger(pan_input(410),60,0.5).unwrap();let mut out=[[0.;2];64];rt.render(&mut out).unwrap();
    let program=ModProgram{sources:vec![ModSource::Velocity],routes:vec![ModRoute::new(0,ModTarget::Pan,1.)],..Default::default()};
    assert!(matches!(scoped_plan().with_processing_modulation(vec![program],vec![Some(0)],0),Err(Error::Capacity)));
    rt.render(&mut out).unwrap();assert_eq!(rt.active_plan(),active);for frame in out{close_to(frame,scoped_oracle(0.5,0.25,2.));}
}


fn scope_source_gate_plan(sustain:f32,release:u32)->sampler_core::Prepared {
    use sampler_core::{Envelope,ModProgram,ModRoute,ModSource,ModTarget,ProcessingModulationGate,VelocityCurve};
    let plan=scoped_plan();
    let program=ModProgram{sources:vec![ModSource::Envelope(Envelope::new(0,0,0,sustain,release).unwrap())],routes:vec![ModRoute::new(0,ModTarget::Attenuate,1.)],..Default::default()};
    plan.with_processing_modulation_gates(vec![program],vec![Some(0)],vec![Some(ProcessingModulationGate{source:0,route:0,velocity:VelocityCurve::Constant})],1<<20).unwrap()
}
fn scope_gate_literal(level:f64)->[f32;2] {
    // Sum the two ordinary physical sources, multiply at the shared Gain,
    // then rectify and apply the independently evaluated static Pan law.
    let far=(0.25*std::f64::consts::FRAC_PI_2).cos().powi(2);
    [((0.125f64*2.*level)as f32).abs()*far as f32,((-0.25f64*2.*level)as f32).abs()]
}
#[test]
fn source_gate_release_uses_same_absolute_control_end_for_all_fragment_boundaries() {
    for prefix in [1usize,17,31,32,33,63,64,65] {for release in [0u32,33,97,128] {
        let plan=scope_source_gate_plan(1.,release);let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();
        let input=pan_input(420);let note=rt.trigger(input,60,1.).unwrap();let mut audio=[[0.;2];384];
        support::without_heap(||rt.render(&mut audio[..prefix]).unwrap());
        let mut observed=0;assert!(rt.visit_onset_voices(|voice|{assert_eq!(voice.cursor.traversal,prefix as u64);assert_eq!(voice.cursor.frame,Some(prefix as u64));observed+=1;true}));assert_eq!(observed,2);
        support::without_heap(||rt.release(note).unwrap());
        // Existing Core modulation evaluates one future absolute64 point. The
        // release updates that SAME EG state, with no prior-block source read.
        let anchor=prefix.div_ceil(64)*64;let end=(anchor+(release as usize).max(1)).div_ceil(64)*64;
        support::without_heap(||{let mut at=prefix;for width in [1usize,17,31,32,33,63,64,65].into_iter().cycle(){if at==end{break;}let n=width.min(end-at);rt.render(&mut audio[at..at+n]).unwrap();at+=n;}});
        assert_eq!(rt.now(),end as u64);assert_eq!(rt.voice_count(),0,"source rows must retire at exact same-cell EG endpoint prefix={prefix} release={release}");assert_eq!(rt.processing_owner_count(),0);
        let mut endings=0;rt.flush_ended(|ended|{assert_eq!(ended,input);endings+=1;true});assert_eq!(endings,1);rt.flush_ended(|_|{endings+=1;true});assert_eq!(endings,1);
        support::without_heap(||rt.render(&mut audio[end..]).unwrap());
        for (frame,actual)in audio.into_iter().enumerate(){
            let level=if frame<anchor{1.}else if frame>=end{0.}else{
                let begin=frame/64*64;let point=|at:usize|if at==anchor{1.}else if release==0{0.}else{(1.-(at-anchor)as f64/f64::from(release)).max(0.)};
                point(begin)+(point(begin+64)-point(begin))*(frame-begin)as f64/64.
            };
            close_to(actual,scope_gate_literal(level));
        }assert_eq!(rt.stats().nonfinite_frames,0);
    }}
}
#[test]
fn zero_sustain_scope_gate_stays_alive_and_ended_closed_birth_never_reads_source() {
    let plan=scope_source_gate_plan(0.,0);let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut silent=sampler_core::Runtime::new(plan,limits).unwrap();let note=silent.trigger(pan_input(421),60,1.).unwrap();let mut audio=[[0.;2];65];
    support::without_heap(||silent.render(&mut audio).unwrap());assert_eq!(audio,[[0.;2];65]);assert_eq!(silent.voice_count(),2);assert_eq!(silent.processing_owner_count(),1,"zero gain is not an ended envelope phase");
    let mut seen=0;assert!(silent.visit_onset_voices(|voice|{assert_eq!(voice.cursor.traversal,65);seen+=1;true}));assert_eq!(seen,2);
    silent.release(note).unwrap();
    // SAME absolute CELL64 endpoint law as the neighboring source-gate test:
    // F65 has already forecast anchor128; release0 completes at ceil((128+1)/64)*64.
    let endpoint=(65usize.div_ceil(64)*64+1).div_ceil(64)*64;
    support::without_heap(||{let mut frames=endpoint-1-65;while frames>0{let n=frames.min(audio.len());silent.render(&mut audio[..n]).unwrap();frames-=n;}});
    assert_eq!(silent.now(),(endpoint-1)as u64);assert_eq!(silent.voice_count(),2);
    let mut alive=0;assert!(silent.visit_onset_voices(|voice|{assert_eq!(voice.cursor.traversal,(endpoint-1)as u64);alive+=1;true}));assert_eq!(alive,2);
    support::without_heap(||silent.render(&mut audio[..1]).unwrap());assert_eq!(silent.now(),endpoint as u64);assert_eq!(silent.voice_count(),0);assert_eq!(silent.processing_owner_count(),0);
    let plan=scope_source_gate_plan(1.,0);let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut closed=sampler_core::Runtime::new(plan,limits).unwrap();let note=closed.trigger(pan_input(422),60,1.).unwrap();closed.release(note).unwrap();
    support::without_heap(||closed.render(&mut audio[..1]).unwrap());assert_eq!(audio[0],[0.;2]);assert_eq!(closed.voice_count(),0);assert_eq!(closed.processing_owner_count(),0);let mut ended=0;closed.flush_ended(|input|{assert_eq!(input,pan_input(422));ended+=1;true});assert_eq!(ended,1);assert_eq!(closed.stats().nonfinite_frames,0);
}

#[test]
fn member_eof_does_not_reset_shared_scope_phase_or_borrow_another_note_input() {
    use sampler_core::{Lfo,LfoRate,LfoShape,ModProgram,ModRoute,ModSource,ModTarget};
    let sources=vec![Pcm::new(48000,vec![[0.25,-0.375];17].into_boxed_slice()).unwrap(),Pcm::new(48000,vec![[-0.125,0.125];4096].into_boxed_slice()).unwrap()];
    let plan=sampler_core::lower::lower_with(&scoped_modulated_graph(),48000,sources,&sampler_core::lower::Options{mpe:None},|_,plan|Ok(plan)).unwrap();
    let program=ModProgram{sources:vec![ModSource::Lfo(Lfo{shape:LfoShape::Sine,rate:LfoRate::Hertz(23.7),phase:0.,delay:0,fade:0,retrigger:true,shared:false})],routes:vec![ModRoute::new(0,ModTarget::Pan,0.6)],..Default::default()};
    let plan=plan.with_processing_modulation(vec![program],vec![Some(0)],1<<20).unwrap();let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();let note=rt.trigger(pan_input(424),60,0.5).unwrap();
    let mut output=[[0.;2];193];support::without_heap(||{rt.render(&mut output[..17]).unwrap();});assert_eq!(rt.voice_count(),1);assert_eq!(rt.processing_owner_count(),1);
    support::without_heap(||{rt.render(&mut output[17..65]).unwrap();rt.render(&mut output[65..]).unwrap();});
    for (frame,actual)in output.into_iter().enumerate(){let begin=frame/64*64;let point=|at:usize|0.6*(std::f64::consts::TAU*23.7*at as f64/48000.).sin();let route=point(begin)+(point(begin+64)-point(begin))*(frame-begin)as f64/64.;let pan=(0.25+route).clamp(-1.,1.);let far=(pan.abs()*std::f64::consts::FRAC_PI_2).cos().powi(2);let sum:[f32;2]=if frame<17{[0.125,-0.25]}else{[-0.125,0.125]};let expected=[(f64::from((sum[0]*2.).abs())*far)as f32,(sum[1]*2.).abs()];close_to(actual,expected);}
    assert_eq!(rt.voice_count(),1);assert_eq!(rt.processing_owner_count(),1);assert_eq!(rt.stats().nonfinite_frames,0);rt.release(note).unwrap();let mut drain=[[0.;2];64];rt.render(&mut drain).unwrap();assert_eq!(rt.voice_count(),0);assert_eq!(rt.processing_owner_count(),0);
}


fn exact_scope_homes()->Vec<sampler_core::SourceOwnerOutputControl>{
    [(sampler_core::SourceOwnerControl::Gain,0,"owner-gain"),(sampler_core::SourceOwnerControl::Pan,2,"scalar-pan")].into_iter().map(|(field,index,key)|sampler_core::SourceOwnerOutputControl{owner:ir::SourceOwnerId::UviNode(1),field,target:sampler_core::SourceOwnerControlTarget::ProcessingScope{scope:0,site:sampler_core::ScopeControlSite::ChainPre(index)},control:sampler_core::lower::ir_control_id(key)}).collect()
}
#[test]
fn late_scope_role_removal_cannot_silently_discard_compiled_gain_or_pan() {
    for remove in 0..3{let mut homes=exact_scope_homes();if remove==0{homes.clear();}else{homes.remove(remove-1);}
        assert!(matches!(scoped_plan().with_source_owner_output_controls(homes,1<<20),Err(Error::InvalidInput)),"compiled program requires every actual scope home");
    }
}
fn scope_program_with_real_alternative_bus()->sampler_core::Prepared {
    use sampler_core::{Bus,BusSend,ChannelBus,ChannelCount,ModProgram,ModRoute,ModSource,ModTarget};
    let mut instrument=scoped_modulated_graph();instrument.source_indices.processing[0].routes.clear();
    let plan=sampler_core::lower::lower_with(&instrument,48000,scoped_pcm(),&sampler_core::lower::Options{mpe:None},|_,plan|Ok(plan)).unwrap();
    let plan=plan.with_buses(vec![Bus{processors:vec![],sends:vec![BusSend{bus:None,gain:1.}],tail_frames:0}],vec![None;2]).unwrap();
    let param=|constant,key:&str|ir::SignalParameter{constant,control:Some(key.into()),expression:None};
    let bus=ChannelBus{bus:0,owner:Some(ir::SourceOwnerId::UviNode(1)),signal:ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:2},nodes:vec![ir::ProcessingSignalNode::Gain(ir::ChannelGain{gain:param(2.,"owner-gain"),arithmetic:ir::ChannelGainArithmetic::CoreGainF64ThenF32}),ir::ProcessingSignalNode::Pan(ir::ChannelPan{position:param(0.25,"scalar-pan"),offset:0.,law:ir::PanLaw::RaisedCosineCoreF64,expression:false})],controls:vec![]},output:ChannelCount::new(2).unwrap()};
    let plan=plan.with_channel_buses(vec![bus],1<<20).unwrap().with_source_owner_output_controls(exact_scope_homes(),1<<20).unwrap();
    let program=ModProgram{sources:vec![ModSource::Velocity],routes:vec![ModRoute::new(0,ModTarget::Attenuate,1.),ModRoute::new(0,ModTarget::Pan,0.5)],..Default::default()};
    plan.with_processing_modulation(vec![program],vec![Some(0)],1<<20).unwrap()
}
#[test]
fn valid_bus_field_cannot_replace_the_same_owners_required_processing_scope_home() {
    let mut bus_homes=exact_scope_homes();for (index,home)in bus_homes.iter_mut().enumerate(){home.target=sampler_core::SourceOwnerControlTarget::Bus{bus:0,node:index};}
    assert!(matches!(scope_program_with_real_alternative_bus().with_source_owner_output_controls(bus_homes,1<<20),Err(Error::InvalidInput)));
}
#[test]
fn reordered_actual_scope_homes_keep_compiled_modulation_and_absolute_control_pcm() {
    use sampler_core::{ControlValue,ControlWrite};let mut homes=exact_scope_homes();homes.reverse();let plan=scope_program_with_real_alternative_bus().with_source_owner_output_controls(homes,1<<20).unwrap();let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();let note=rt.trigger(pan_input(430),60,0.5).unwrap();let mut out=[[0.;2];65];
    support::without_heap(||rt.render(&mut out).unwrap());for frame in out{close_to(frame,scoped_oracle(0.5,0.25,2.));}
    support::without_heap(||{rt.edit_controls(rt.active_plan(),None,&[ControlWrite{id:sampler_core::lower::ir_control_id("scalar-pan"),value:ControlValue::Real(-0.75)}]).unwrap();rt.render(&mut out).unwrap();});for frame in out{close_to(frame,scoped_oracle(0.5,-0.75,2.));}assert_eq!(rt.stats().nonfinite_frames,0);rt.release(note).unwrap();rt.render(&mut out).unwrap();
}
#[test]
fn final_runtime_rejects_program_homes_erased_by_an_unrelated_late_bus_rebuild() {
    use sampler_core::{Bus,BusSend};
    let candidate=scoped_plan().with_buses(vec![Bus{processors:vec![],sends:vec![BusSend{bus:None,gain:1.}],tail_frames:0}],vec![None;2]);
    match candidate{Err(Error::InvalidInput)=>{},Ok(plan)=>{let limits=sampler_core::Limits::for_plan(&plan,4,8);assert!(matches!(sampler_core::Runtime::new(plan,limits),Err(Error::InvalidInput)));},Err(error)=>panic!("wrong refusal phase: {error:?}")}
}

fn shared_restart_failure_plan()->sampler_core::Prepared {
    use sampler_core::{Lfo,LfoRate,LfoShape,ModProgram,ModRoute,ModSource,ModTarget};
    let mut instrument=scoped_modulated_graph();instrument.source_indices.processing[0].routes.clear();
    for n in 2..4{instrument.assets.push(ir::Asset{location:ir::AssetLocation::Path(format!("later-member-{n}.wav")),encoding:ir::Encoding::Wav,residency:ir::AssetResidency::Streamable,root_key:None,loops:vec![]});let mut zone=instrument.zones[n-2].clone();zone.asset=Some(ir::AssetRef(n));zone.keys=ir::KeyRange{low:61,high:61};zone.pitch=ir::KeyTracking::Fixed;instrument.zones.push(zone);}
    let mut scope=instrument.processing_scopes[0].clone();scope.members=vec![ir::ZoneRef(2),ir::ZoneRef(3)];instrument.processing_scopes.push(scope);
    let mut recipe=instrument.source_indices.processing[0].clone();recipe.owner=ir::SourceOwnerId::UviNode(4);recipe.members=vec![ir::ZoneRef(2),ir::ZoneRef(3)];recipe.compiled=Some(1);instrument.source_indices.processing.push(recipe);
    instrument.source_indices.owners[0].children.push(ir::SourceOwnerId::UviNode(4));instrument.source_indices.owners.extend([owner(4,Some(0),ir::SourceOwnerKind::Keygroup,&[5,6],&[2,3]),owner(5,Some(4),ir::SourceOwnerKind::Player,&[],&[2]),owner(6,Some(4),ir::SourceOwnerKind::Player,&[],&[3])]);
    let additions:Vec<_>=instrument.source_indices.output_controls.iter().copied().map(|mut home|{home.owner=ir::SourceOwnerId::UviNode(4);if let ir::SourceOwnerControlTarget::ProcessingScope{scope,..}=&mut home.target{*scope=1;}home}).collect();instrument.source_indices.output_controls.extend(additions);
    // The final B member has real geometry but no actual cache/head. A's two
    // resident sources and B's first member are independently ready.
    let mut sources=scoped_pcm();sources.push(Pcm::new(48000,vec![[0.125;2];4096].into_boxed_slice()).unwrap());sources.push(Pcm::streamed(48000,4096).unwrap());
    let plan=sampler_core::lower::lower_with(&instrument,48000,sources,&sampler_core::lower::Options{mpe:None},|_,plan|Ok(plan)).unwrap();
    let program=ModProgram{sources:vec![ModSource::Lfo(Lfo{shape:LfoShape::Square,rate:LfoRate::Hertz(48.),phase:0.,delay:0,fade:0,retrigger:true,shared:true})],routes:vec![ModRoute::new(0,ModTarget::Pan,0.6)],..Default::default()};
    // Deliberate generic shared-retrigger domain across two actual scope
    // definitions; public scalar base aliases are explicit author intent.
    plan.with_processing_modulation(vec![program],vec![Some(0),Some(0)],1<<20).unwrap()
}
#[test]
fn failed_final_member_public_onset_cannot_restart_a_published_shared_domain() {
    let make=||{let plan=shared_restart_failure_plan();let limits=sampler_core::Limits::for_plan(&plan,8,8);sampler_core::Runtime::new(plan,limits).unwrap()};let mut actual=make();let mut reference=make();let a=actual.trigger(pan_input(431),60,1.).unwrap();reference.trigger(pan_input(431),60,1.).unwrap();
    let mut prefix=[[0.;2];600];support::without_heap(||{actual.render(&mut prefix).unwrap();reference.render(&mut prefix).unwrap();});let before=(actual.now(),actual.voice_count(),actual.processing_owner_count(),actual.expression(actual.expression_id(a).unwrap()).unwrap());
    let mut owners=Vec::new();actual.visit_processing_owners(|row|owners.push((row.id,row.note,row.plan,row.template,row.sources)));
    support::without_heap(||{assert_eq!(actual.trigger(sampler_core::Input{key:61,..pan_input(432)},61,1.),Err(Error::NotReady));});assert_eq!((actual.now(),actual.voice_count(),actual.processing_owner_count(),actual.expression(actual.expression_id(a).unwrap()).unwrap()),before);
    let mut after=Vec::new();actual.visit_processing_owners(|row|after.push((row.id,row.note,row.plan,row.template,row.sources)));assert_eq!(owners,after);
    let mut out=[[0.;2];400];let mut expected=out;support::without_heap(||{actual.render(&mut out[..17]).unwrap();actual.render(&mut out[17..82]).unwrap();actual.render(&mut out[82..]).unwrap();reference.render(&mut expected).unwrap();});assert_eq!(out,expected);
    // At F700 the original48Hz square is negative, while a false restart at
    // F600 would still be positive. Thus equality includes an audible witness.
    let witness=out[100];assert!(witness[0]>witness[1]*0.5,"expected original negative-pan branch");assert_eq!((actual.voice_count(),actual.processing_owner_count()),(2,1));assert_eq!(actual.stats().nonfinite_frames,0);actual.release(a).unwrap();
}

#[test]
fn n_signal_moving_scope_gain_and_absolute_base_writes_follow_independent_odd_fragment_math() {
    use sampler_core::{ChannelCount,SourceStorageFamily,Lfo,LfoRate,LfoShape,ModProgram,ModRoute,ModSource,ModTarget,ControlValue,ControlWrite};
    let mut instrument=scoped_modulated_graph();instrument.source_indices.processing[0].routes.clear();instrument.processing_scopes[0].chain=None;
    let param=|constant,key:&str|ir::SignalParameter{constant,control:Some(key.into()),expression:None};let mut cells=vec![0.;12];cells[0]=1.;cells[4]=0.5;cells[7]=1.;cells[11]=0.5;
    instrument.processing_scopes[0].signal=Some(ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:6},nodes:vec![ir::ProcessingSignalNode::Gain(ir::ChannelGain{gain:param(2.,"owner-gain"),arithmetic:ir::ChannelGainArithmetic::CoreGainF64ThenF32}),ir::ProcessingSignalNode::Rectifier(ir::Rectifier::Full),ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{input:ir::SignalLayout{channels:6},output:ir::SignalLayout{channels:2},cells,arithmetic:ir::MatrixArithmetic::SeparateMulAddF32}),ir::ProcessingSignalNode::Pan(ir::ChannelPan{position:param(0.25,"scalar-pan"),offset:0.,law:ir::PanLaw::RaisedCosineCoreF64,expression:true})],controls:vec![]});
    for home in &mut instrument.source_indices.output_controls{home.target=ir::SourceOwnerControlTarget::ProcessingScope{scope:0,site:ir::ScopeControlSite::SignalNode(if home.field==ir::SourceOwnerOutputField::Gain{0}else{3})};}
    for role in &mut instrument.source_indices.processing[0].parameter_roles{role.compiled=Some(ir::ScopeControlSite::SignalNode(if role.field==ir::SourceOwnerOutputField::Gain{0}else{3}));}
    let samples=[[[0.25,-0.375],[0.5,-0.25],[0.125,-0.125]],[[-0.125,0.125],[-0.25,0.125],[0.125,0.25]]];
    let sources=samples.into_iter().map(|packs|SourceStorageFamily::new_resident(ChannelCount::new(6).unwrap(),packs.into_iter().map(|value|Pcm::new(48000,vec![value;4096].into_boxed_slice()).unwrap()).collect()).unwrap().into()).collect();
    let plan=sampler_core::lower::lower_sources_with(&instrument,48000,sources,&sampler_core::lower::Options{mpe:None},|_,plan|Ok(plan)).unwrap();
    let program=ModProgram{sources:vec![ModSource::Lfo(Lfo{shape:LfoShape::Sine,rate:LfoRate::Hertz(13.7),phase:0.,delay:0,fade:0,retrigger:true,shared:false})],routes:vec![ModRoute::new(0,ModTarget::Attenuate,1.)],..Default::default()};
    let plan=plan.with_processing_modulation(vec![program],vec![Some(0)],1<<20).unwrap();let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();rt.trigger(pan_input(433),60,1.).unwrap();let plan=rt.active_plan();let gain=sampler_core::lower::ir_control_id("owner-gain");let events=[(17usize,0.25),(63,1.5),(64,0.5),(65,2.),(129,0.125)];let mut audio=[[0.;2];384];let mut at=0;let mut event=0;
    support::without_heap(||{for width in [1usize,17,63,64,65].into_iter().cycle(){if at==audio.len(){break;}if event<events.len()&&events[event].0==at{rt.edit_controls(plan,None,&[ControlWrite{id:gain,value:ControlValue::Real(events[event].1)}]).unwrap();event+=1;}let next=events.get(event).map_or(audio.len(),|row|row.0);let end=(at+width).min(next).min(audio.len());rt.render(&mut audio[at..end]).unwrap();at=end;}});
    let sum:[f32;6]=[0.125,-0.25,0.25,-0.125,0.25,0.125];let far=(0.25*std::f64::consts::FRAC_PI_2).cos().powi(2);
    for (frame,actual)in audio.into_iter().enumerate(){let base=events.iter().rev().find(|(at,_)|frame>=*at).map_or(2.,|(_,value)|*value);let begin=frame/64*64;let point=|at:usize|0.5+0.5*(std::f64::consts::TAU*13.7*at as f64/48000.).sin();let modulation=point(begin)+(point(begin+64)-point(begin))*(frame-begin)as f64/64.;let planes:[f32;6]=std::array::from_fn(|c|((f64::from(sum[c])*base*modulation)as f32).abs());let projected=[planes[0]+planes[4]*0.5,planes[1]+planes[5]*0.5];close_to(actual,[(f64::from(projected[0])*far)as f32,projected[1]]);}
    assert_eq!(rt.control_value(plan,gain),Ok(ControlValue::Real(0.125)));assert_eq!((rt.voice_count(),rt.processing_owner_count()),(2,1));assert_eq!(rt.stats().nonfinite_frames,0);
}


// Provider33: the comparison uses the established ordinary per-voice source
// path, not the new provider evaluator. The actual mapped slot deliberately
// describes Constant: cloning that descriptor cannot produce the reference.
fn provider_wave()->sampler_core::ModSource {
    use sampler_core::{Lfo,LfoRate,LfoShape,ModSource};
    ModSource::Lfo(Lfo{shape:LfoShape::Sine,rate:LfoRate::Hertz(23.7),phase:0.13,delay:0,fade:0,retrigger:true,shared:false})
}
fn provider_pcm()->Vec<Pcm>{
    (0..2).map(|member|Pcm::new(48000,(0..8192).map(|frame|{
        let a=((frame*17+member*31)%127)as f32/1024.;
        let b=((frame*11+member*23)%113)as f32/1024.;[a,b]
    }).collect::<Vec<_>>().into_boxed_slice()).unwrap()).collect()
}
fn provider_plan(provided:bool,onset:bool)->sampler_core::Prepared {provider_plan_from(provided,onset,provider_pcm())}
fn provider_plan_from(provided:bool,onset:bool,sources:Vec<Pcm>)->sampler_core::Prepared {
    use sampler_core::{ModProgram,ModRoute,ModSource,ModTarget,ModProvider,ProcessingSourceRef};
    let mut instrument=scoped_modulated_graph();instrument.source_indices.processing[0].routes.clear();
    let plan=sampler_core::lower::lower_with(&instrument,48000,sources,&sampler_core::lower::Options{mpe:None},|_,plan|Ok(plan)).unwrap();
    let owner=ModProgram{sources:vec![provider_wave(),ModSource::Velocity],routes:vec![ModRoute::new(0,ModTarget::Pan,0.4)],..Default::default()};
    let plan=plan.with_processing_modulation(vec![owner],vec![Some(0)],1<<20).unwrap();
    let mut routes=vec![ModRoute::new(0,ModTarget::Pitch,3.),ModRoute::new(0,ModTarget::Tone,24.)];
    if onset{routes.push(ModRoute::new(1,ModTarget::SampleStart,1.));}
    let program=ModProgram{sources:if provided{vec![ModSource::Constant,ModSource::Constant]}else{vec![provider_wave(),ModSource::Velocity]},routes,..Default::default()};
    if provided{plan.with_voice_modulation_providers(vec![program],vec![Some(0);2],vec![64;2],
        vec![ModProvider{program:0,source:0,provider:ProcessingSourceRef{scope:0,source:0}},ModProvider{program:0,source:1,provider:ProcessingSourceRef{scope:0,source:1}}]).unwrap()
    }else{plan.with_voice_modulation(vec![program],vec![Some(0);2],vec![64;2]).unwrap()}
}
fn provider_runtime(provided:bool,onset:bool)->sampler_core::Runtime {
    let plan=provider_plan(provided,onset);let limits=sampler_core::Limits::for_plan(&plan,4,8);sampler_core::Runtime::new(plan,limits).unwrap()
}
fn provider_audio_equal(actual:&[[f32;2]],reference:&[[f32;2]]){
    assert_eq!(actual.len(),reference.len());let mut energy=0.;let mut error=0.;let mut largest=0f32;
    for (a,b)in actual.iter().flatten().zip(reference.iter().flatten()){
        energy+=f64::from(*b).powi(2);error+=(f64::from(*a)-f64::from(*b)).powi(2);largest=largest.max((a-b).abs());
    }
    assert!(energy>1e-7,"independent ordinary source path must sound");
    assert!(error/energy<1e-10&&largest<3e-6,"provider difference energy={} largest={largest}",error/energy);
}
#[test]
fn provider_shared_sine_pitch_tone_and_pan_matches_independent_ordinary_sources_and_same_frame_reentry(){
    let mut actual=provider_runtime(true,false);let mut reference=provider_runtime(false,false);
    let input=pan_input(601);let note=actual.trigger(input,60,0.75).unwrap();reference.trigger(input,60,0.75).unwrap();
    let mut a=[[0.;2];1025];let mut b=a;let mut at=0;
    support::without_heap(||{for width in [1usize,17,31,63,64,65].into_iter().cycle(){if at==a.len(){break;}let end=(at+width).min(a.len());
        actual.render(&mut []).unwrap();actual.render(&mut a[at..end]).unwrap();reference.render(&mut b[at..end]).unwrap();at=end;
    }});provider_audio_equal(&a,&b);assert_eq!((actual.voice_count(),actual.processing_owner_count()),(2,1));
    let mut actual_cursors=Vec::new();let mut reference_cursors=Vec::new();
    assert!(actual.visit_onset_voices(|v|{actual_cursors.push(v.cursor);true}));assert!(reference.visit_onset_voices(|v|{reference_cursors.push(v.cursor);true}));
    for (a,b)in actual_cursors.iter().zip(&reference_cursors){assert_eq!(a.frame,b.frame);assert_eq!(a.traversal,b.traversal);}
    actual.release(note).unwrap();actual.render(&mut [[0.;2];128]).unwrap();actual.flush_ended(|_|true);assert_eq!(actual.processing_owner_count(),0);assert_eq!(actual.stats().nonfinite_frames,0);
}
#[test]
fn provider_sample_start_reads_real_owner_velocity_before_cursor_admission(){
    let mut actual=provider_runtime(true,true);let mut reference=provider_runtime(false,true);
    actual.trigger(pan_input(602),60,0.75).unwrap();reference.trigger(pan_input(602),60,0.75).unwrap();
    let mut seen=0;assert!(actual.visit_onset_voices(|v|{assert_eq!(v.cursor.frame,Some(48),"actual .75 owner velocity, not Constant descriptor64");assert_eq!(v.cursor.traversal,48);seen+=1;true}));assert_eq!(seen,2);
    let mut a=[[0.;2];385];let mut b=a;support::without_heap(||{actual.render(&mut a[..17]).unwrap();actual.render(&mut a[17..82]).unwrap();actual.render(&mut a[82..]).unwrap();reference.render(&mut b).unwrap();});provider_audio_equal(&a,&b);assert_eq!(actual.stats().refused_starts,0);
}
#[test]
fn provider_invalid_actual_scope_and_source_refuses_before_live_owner_or_cursor_changes(){
    use sampler_core::{ModProgram,ModSource,ModRoute,ModTarget,ModProvider,ProcessingSourceRef};
    let mut live=provider_runtime(true,false);live.trigger(pan_input(603),60,0.75).unwrap();live.render(&mut [[0.;2];17]).unwrap();let before=live.now();let active=live.active_plan();
    for (scope,source)in [(1usize,0usize),(0,2),(usize::MAX,0),(0,usize::MAX)]{
        let program=ModProgram{sources:vec![ModSource::Constant],routes:vec![ModRoute::new(0,ModTarget::Pitch,3.)],..Default::default()};
        assert!(matches!(provider_plan(true,false).with_voice_modulation_providers(vec![program],vec![Some(0);2],vec![0;2],vec![ModProvider{program:0,source:0,provider:ProcessingSourceRef{scope,source}}]),Err(Error::InvalidInput)));
        assert_eq!(live.now(),before);assert_eq!(live.active_plan(),active);assert_eq!((live.voice_count(),live.processing_owner_count()),(2,1));
    }
    let mut out=[[0.;2];65];support::without_heap(||live.render(&mut out).unwrap());assert!(out.iter().flatten().any(|v|v.abs()>1e-5));assert_eq!(live.stats().nonfinite_frames,0);
}
#[test]
fn provider_old_generation_and_physical_growth_keep_fixed_owner_and_identical_source_phase(){
    let plan=provider_plan(true,true);let limits=sampler_core::Limits::for_plan(&plan,4,8);let(mut actual,mut control)=sampler_core::Runtime::with_plan_updates(plan,limits,3,2).unwrap();let capacity=actual.processing_owner_capacity();let old=actual.active_plan();
    let mut reference=provider_runtime(false,true);let note=actual.trigger(pan_input(604),60,0.75).unwrap();reference.trigger(pan_input(604),60,0.75).unwrap();let mut a=[[0.;2];257];let mut b=a;
    support::without_heap(||{actual.render(&mut a[..17]).unwrap();reference.render(&mut b[..17]).unwrap();});control.grow_voices(16).unwrap();
    support::without_heap(||{actual.render(&mut a[17..82]).unwrap();reference.render(&mut b[17..82]).unwrap();});assert_eq!(actual.processing_owner_capacity(),capacity);assert_eq!(actual.voice_capacity(),16);control.collect_growth_returns();
    let request=control.submit(Box::new(provider_plan(true,false))).unwrap();assert_eq!(actual.poll_plan_update().unwrap(),Some(request));assert_ne!(actual.active_plan(),old);
    support::without_heap(||{actual.render(&mut a[82..]).unwrap();reference.render(&mut b[82..]).unwrap();});provider_audio_equal(&a,&b);
    let mut owners=0;actual.visit_processing_owners(|owner|{assert_eq!(owner.plan,old);owners+=1;});assert_eq!(owners,1);assert_eq!(actual.processing_owner_capacity(),capacity);
    actual.release(note).unwrap();actual.render(&mut [[0.;2];128]).unwrap();actual.flush_ended(|_|true);assert_eq!(actual.processing_owner_count(),0);assert_eq!(actual.stats().nonfinite_frames,0);
}

fn provider_dependency_plan(provided:bool)->sampler_core::Prepared {
    use sampler_core::{ModProgram,ModRoute,ModSource,ModTarget,ModProvider,ProcessingSourceRef,Lfo,LfoRate,LfoShape};
    let mut instrument=scoped_modulated_graph();instrument.source_indices.processing[0].routes.clear();
    let plan=sampler_core::lower::lower_with(&instrument,48000,provider_pcm(),&sampler_core::lower::Options{mpe:None},|_,plan|Ok(plan)).unwrap();
    let sources=vec![ModSource::Lfo(Lfo{shape:LfoShape::Sine,rate:LfoRate::Hertz(1.),phase:0.,delay:0,fade:0,retrigger:true,shared:false}),ModSource::Controller(1),ModSource::Constant];
    let incoming=vec![ModRoute::new(1,ModTarget::SourceRate(0),0.05),ModRoute::new(2,ModTarget::SourcePhase(0),0.2)];
    let mut owner_routes=incoming.clone();owner_routes.push(ModRoute::new(0,ModTarget::Pan,0.4));
    let plan=plan.with_processing_modulation(vec![ModProgram{sources:sources.clone(),routes:owner_routes,..Default::default()}],vec![Some(0)],1<<20).unwrap();
    let mut routes=if provided{vec![]}else{incoming};routes.extend([ModRoute::new(0,ModTarget::Pitch,3.),ModRoute::new(0,ModTarget::Tone,-24.)]);
    let program=ModProgram{sources:if provided{vec![ModSource::Constant;3]}else{sources},routes,..Default::default()};
    if provided{plan.with_voice_modulation_providers(vec![program],vec![Some(0);2],vec![0;2],(0..3).map(|source|ModProvider{program:0,source,provider:ProcessingSourceRef{scope:0,source}}).collect()).unwrap()}
    else{plan.with_voice_modulation(vec![program],vec![Some(0);2],vec![0;2]).unwrap()}
}
#[test]
fn provider_owned_rate_phase_and_controller_dependencies_match_independent_ordinary_closure(){
    let make=|provided|{let plan=provider_dependency_plan(provided);let limits=sampler_core::Limits::for_plan(&plan,4,8);sampler_core::Runtime::new(plan,limits).unwrap()};
    let mut actual=make(true);let mut reference=make(false);actual.trigger(pan_input(605),60,1.).unwrap();reference.trigger(pan_input(605),60,1.).unwrap();
    let address=sampler_core::ChannelAddress{protocol:sampler_core::Protocol::Native,port:0,group:0,channel:0};
    let mut a=[[0.;2];513];let mut b=a;let mut at=0;
    for (end,value)in [(17usize,0x4321_7654),(63,u32::MAX),(64,0),(65,0x8000_0001),(129,u32::MAX),(513,0)]{
        support::without_heap(||{actual.render(&mut a[at..end]).unwrap();reference.render(&mut b[at..end]).unwrap();
            let performance=actual.performance(0).unwrap();actual.dispatch_controller(performance,address,1,1,value).unwrap();
            let performance=reference.performance(0).unwrap();reference.dispatch_controller(performance,address,1,1,value).unwrap();
        });at=end;
    }provider_audio_equal(&a,&b);assert_eq!(actual.processing_owner_count(),1);assert_eq!(actual.stats().nonfinite_frames,0);
}
#[test]
fn provider_pending_failed_source_family_cannot_publish_restart_to_existing_owner(){
    use sampler_core::{ModProgram,ModRoute,ModSource,ModTarget,ModProvider,ProcessingSourceRef};
    let make=||{
        let plan=shared_restart_failure_plan();
        let program=ModProgram{sources:vec![ModSource::Constant],routes:vec![ModRoute::new(0,ModTarget::Pitch,0.2)],..Default::default()};
        // Both actual scopes share the SAME prepared restart domain but have
        // separate owner/source rows, and physical bindings name that scope.
        let plan=plan.with_voice_modulation_providers(vec![program.clone(),program],vec![Some(0),Some(0),Some(1),Some(1)],vec![0;4],vec![
            ModProvider{program:0,source:0,provider:ProcessingSourceRef{scope:0,source:0}},ModProvider{program:1,source:0,provider:ProcessingSourceRef{scope:1,source:0}}]).unwrap();
        let limits=sampler_core::Limits::for_plan(&plan,8,8);sampler_core::Runtime::new(plan,limits).unwrap()
    };
    let mut actual=make();let mut reference=make();actual.trigger(pan_input(606),60,1.).unwrap();reference.trigger(pan_input(606),60,1.).unwrap();let mut prefix=[[0.;2];600];actual.render(&mut prefix).unwrap();reference.render(&mut prefix).unwrap();
    let before=(actual.now(),actual.voice_count(),actual.processing_owner_count());
    support::without_heap(||assert_eq!(actual.trigger(sampler_core::Input{key:61,..pan_input(607)},61,1.),Err(Error::NotReady)));
    assert_eq!((actual.now(),actual.voice_count(),actual.processing_owner_count()),before);
    let mut a=[[0.;2];400];let mut b=a;support::without_heap(||{actual.render(&mut a[..17]).unwrap();actual.render(&mut a[17..82]).unwrap();actual.render(&mut a[82..]).unwrap();reference.render(&mut b).unwrap();});assert_eq!(a,b);assert_eq!(actual.stats().nonfinite_frames,0);
}

#[test]
fn provider_cold_onset_commits_once_at_actual_readiness_and_sample_start_is_not_reapplied(){
    let pcm=provider_pcm();let cold:Vec<_>=(0..2).map(|_|Pcm::streamed(48000,8192).unwrap()).collect();
    let plan=provider_plan_from(true,true,cold.clone());let limits=sampler_core::Limits::for_plan(&plan,4,8);let(cache,_worker)=sampler_core::StreamCache::new(16).unwrap();
    let mut actual=sampler_core::Runtime::new(plan,limits).unwrap().with_stream_cache(cache);actual.set_stream_horizon(128).unwrap();actual.set_cold_starts(true);
    let mut reference=provider_runtime(false,true);actual.trigger(pan_input(608),60,0.75).unwrap();
    let mut prefix=[[0.;2];256];support::without_heap(||{for block in prefix.chunks_mut(17){actual.render(block).unwrap();}reference.render(&mut [[0.;2];256]).unwrap();});assert_eq!(prefix,[[0.;2];256]);
    let mut held=0;assert!(actual.visit_onset_voices(|voice|{assert_eq!(voice.cursor.frame,Some(48));assert_eq!(voice.cursor.traversal,48);held+=1;true}));assert_eq!(held,2);assert_eq!(actual.processing_owner_count(),1);
    // This publication is offaudio; the next real pinned onset attempts atF256.
    for (asset,source)in cold.iter().zip(&pcm){let mut frames=vec![[0.;2];8192];source.copy_resident_range(0,&mut frames).unwrap();drop(asset.set_ranges(vec![(0,frames.into_boxed_slice())]).unwrap());}
    reference.trigger(pan_input(608),60,0.75).unwrap();let mut a=[[0.;2];1025];let mut b=a;
    support::without_heap(||{let mut at=0;for width in [1usize,17,63,64,65].into_iter().cycle(){if at==a.len(){break;}let end=(at+width).min(a.len());actual.render(&mut a[at..end]).unwrap();reference.render(&mut b[at..end]).unwrap();at=end;}});
    // Preserve the existing cold-source fade; compare after its actual64-frame
    // ramp and the ordinary Tone integrators settle, not by moving either cursor.
    provider_audio_equal(&a[256..],&b[256..]);let mut ac=Vec::new();let mut rc=Vec::new();actual.visit_onset_voices(|v|{ac.push((v.cursor.traversal,v.cursor.frame));true});reference.visit_onset_voices(|v|{rc.push((v.cursor.traversal,v.cursor.frame));true});assert_eq!(ac,rc);
    assert_eq!(actual.stats().stream_underruns,0);assert_eq!(actual.stats().refused_starts,0);assert_eq!(actual.stats().nonfinite_frames,0);assert_eq!(actual.processing_owner_count(),1);
}
#[test]
fn provider_release_uses_same_owning_envelope_for_physical_pitch_tone_and_source_gate(){
    use sampler_core::{Envelope,ModProgram,ModSource,ModRoute,ModTarget,ModProvider,ProcessingSourceRef};
    let make=|provided|{
        let plan=scope_source_gate_plan(1.,97);
        let program=ModProgram{sources:vec![if provided{ModSource::Constant}else{ModSource::Envelope(Envelope::new(0,0,0,1.,97).unwrap())}],routes:vec![ModRoute::new(0,ModTarget::Pitch,2.),ModRoute::new(0,ModTarget::Tone,-12.)],..Default::default()};
        let plan=if provided{plan.with_voice_modulation_providers(vec![program],vec![Some(0);2],vec![0;2],vec![ModProvider{program:0,source:0,provider:ProcessingSourceRef{scope:0,source:0}}]).unwrap()}else{plan.with_voice_modulation(vec![program],vec![Some(0);2],vec![0;2]).unwrap()};
        let limits=sampler_core::Limits::for_plan(&plan,4,8);sampler_core::Runtime::new(plan,limits).unwrap()
    };
    for prefix in [1usize,17,63,64,65]{let mut actual=make(true);let mut reference=make(false);let note=actual.trigger(pan_input(609),60,1.).unwrap();let other=reference.trigger(pan_input(609),60,1.).unwrap();let mut a=[[0.;2];384];let mut b=a;
        support::without_heap(||{actual.render(&mut a[..prefix]).unwrap();reference.render(&mut b[..prefix]).unwrap();actual.release(note).unwrap();reference.release(other).unwrap();actual.render(&mut a[prefix..]).unwrap();reference.render(&mut b[prefix..]).unwrap();});provider_audio_equal(&a,&b);
        assert_eq!((actual.voice_count(),actual.processing_owner_count()),(0,0));let mut endings=0;actual.flush_ended(|source|{assert_eq!(source,pan_input(609));endings+=1;true});assert_eq!(endings,1);assert_eq!(actual.stats().nonfinite_frames,0);
    }
}

#[test]
fn provider_source_replacement_revalidates_real_dependencies_before_prepared_publication(){
    use sampler_core::{ModProgram,ModSource};
    // Physical onset source1 cannot outlive a replacement owner that has only
    // source0. This is a true dependency, even if its saved route is inert.
    assert!(matches!(provider_plan(true,true).with_processing_modulation(vec![ModProgram{sources:vec![ModSource::Constant],..Default::default()}],vec![Some(0)],1<<20),Err(Error::InvalidInput)));
    let mut instrument=graph(true);instrument.source_indices.processing[0].sources=vec![ir::ModulatorRef(0)];assert!(instrument.validate().is_err());
    instrument.modulators.push(ir::Modulator{scope:ir::Scope::Voice,source:ir::ModulationSource::Constant});assert!(instrument.validate().is_ok());
    instrument.source_indices.processing[0].sources.push(ir::ModulatorRef(0));assert!(instrument.validate().is_err());
    instrument.source_indices.processing[0].sources.pop();instrument.retain_zones(|zone|zone.asset==Some(ir::AssetRef(1)));
    assert_eq!(instrument.source_indices.processing[0].sources,vec![ir::ModulatorRef(0)]);assert_eq!(instrument.source_indices.processing[0].members,vec![ir::ZoneRef(0)]);assert!(instrument.validate().is_ok());
}


// Provider35 independent ordinary-reference pair. The rate input and its live
// depth are accepted atF0 AFTER trigger, without a single PCM frame in between.
fn admission_forecast_plan(provided:bool)->sampler_core::Prepared {
    use sampler_core::{ControlDefinition,ControlDomain,ControlId,ControlValue,ModProgram,ModRoute,ModSource,ModTarget,ModProvider,ProcessingSourceRef,Lfo,LfoRate,LfoShape};
    let mut instrument=scoped_modulated_graph();instrument.source_indices.processing[0].routes.clear();
    let plan=sampler_core::lower::lower_with(&instrument,48000,provider_pcm(),&sampler_core::lower::Options{mpe:None},|_,plan|Ok(plan)).unwrap();
    let mut definitions=plan.controls().to_vec();for(id,max,default)in[(11,0.25,0.05),(13,1.,0.)]{definitions.push(ControlDefinition{id:ControlId(id),domain:ControlDomain::Real{min:0.,max},default:ControlValue::Real(default)});}
    let plan=plan.with_controls(definitions).unwrap();
    let sources=vec![ModSource::Lfo(Lfo{shape:LfoShape::Sine,rate:LfoRate::Hertz(1.),phase:0.,delay:0,fade:0,retrigger:true,shared:false}),ModSource::Controller(1),ModSource::Constant,ModSource::Velocity];
    let incoming=vec![ModRoute::new(1,ModTarget::SourceRate(0),0.05),ModRoute::new(2,ModTarget::SourcePhase(0),0.2)];let mut owner_routes=incoming.clone();owner_routes.push(ModRoute::new(0,ModTarget::Pan,0.4));
    let plan=plan.with_processing_modulation(vec![ModProgram{sources:sources.clone(),routes:owner_routes,controls:vec![(0,[ControlId(11),ControlId(13)])],..Default::default()}],vec![Some(0)],1<<20).unwrap();
    let program=if provided{ModProgram{sources:vec![ModSource::Constant;2],routes:vec![ModRoute::new(0,ModTarget::Pitch,3.),ModRoute::new(1,ModTarget::SampleStart,1.)],..Default::default()}}
        else{let mut routes=incoming;routes.extend([ModRoute::new(0,ModTarget::Pitch,3.),ModRoute::new(3,ModTarget::SampleStart,1.)]);ModProgram{sources,routes,controls:vec![(0,[ControlId(11),ControlId(13)])],..Default::default()}};
    if provided{plan.with_voice_modulation_providers(vec![program],vec![Some(0);2],vec![64;2],vec![ModProvider{program:0,source:0,provider:ProcessingSourceRef{scope:0,source:0}},ModProvider{program:0,source:1,provider:ProcessingSourceRef{scope:0,source:3}}]).unwrap()}
        else{plan.with_voice_modulation(vec![program],vec![Some(0);2],vec![64;2]).unwrap()}
}
#[test]
fn admission_onset_defers_first_forecast_until_same_frame_cc_and_live_rate_depth_are_accepted(){
    use sampler_core::{ChannelAddress,ControlId,ControlValue,ControlWrite,Protocol};
    for live_depth in [false,true]{
        let make=|provided|{let plan=admission_forecast_plan(provided);let limits=sampler_core::Limits::for_plan(&plan,4,8);sampler_core::Runtime::new(plan,limits).unwrap()};
        let mut actual=make(true);let mut reference=make(false);let address=ChannelAddress{protocol:Protocol::Native,port:0,group:0,channel:0};
        if live_depth{for rt in [&mut actual,&mut reference]{let performance=rt.performance(0).unwrap();rt.dispatch_controller(performance,address,1,1,u32::MAX).unwrap();}}
        actual.trigger(pan_input(610),60,0.75).unwrap();reference.trigger(pan_input(610),60,0.75).unwrap();
        for rt in [&mut actual,&mut reference]{
            if live_depth{let plan=rt.active_plan();rt.edit_controls(plan,None,&[ControlWrite{id:ControlId(11),value:ControlValue::Real(0.2)}]).unwrap();}
            else{let performance=rt.performance(0).unwrap();rt.dispatch_controller(performance,address,1,1,u32::MAX).unwrap();}
            assert_eq!(rt.now(),0);let mut seen=0;assert!(rt.visit_onset_voices(|voice|{assert_eq!(voice.cursor.frame,Some(48));assert_eq!(voice.cursor.traversal,48);seen+=1;true}));assert_eq!(seen,2);
        }
        let mut a=[[0.;2];513];let mut b=a;support::without_heap(||{
            actual.render(&mut []).unwrap();actual.render(&mut []).unwrap();reference.render(&mut []).unwrap();
            let mut at=0;for width in [1usize,17,63,64,65].into_iter().cycle(){if at==a.len(){break;}let end=(at+width).min(a.len());actual.render(&mut a[at..end]).unwrap();at=end;}
            reference.render(&mut b).unwrap();
        });provider_audio_equal(&a,&b);assert_eq!((actual.voice_count(),actual.processing_owner_count()),(2,1));assert_eq!(actual.stats().nonfinite_frames,0);
    }
}

#[test]
fn admission_onset_same_frame_key_up_keeps_closed_birth_source_read_boundary(){
    use sampler_core::{ModProgram,ModRoute,ModSource,ModTarget,ModProvider,ProcessingSourceRef};
    let plan=scope_source_gate_plan(1.,0).with_voice_modulation_providers(vec![ModProgram{sources:vec![ModSource::Constant],routes:vec![ModRoute::new(0,ModTarget::Pitch,2.)],..Default::default()}],vec![Some(0);2],vec![0;2],vec![ModProvider{program:0,source:0,provider:ProcessingSourceRef{scope:0,source:0}}]).unwrap();
    let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();let source=pan_input(611);let note=rt.trigger(source,60,1.).unwrap();assert_eq!(rt.now(),0);
    support::without_heap(||rt.release(note).unwrap());let mut audio=[[1.;2];1];support::without_heap(||rt.render(&mut audio).unwrap());assert_eq!(audio,[[0.;2];1]);assert_eq!((rt.voice_count(),rt.processing_owner_count()),(0,0));
    let mut endings=0;rt.flush_ended(|input|{assert_eq!(input,source);endings+=1;true});assert_eq!(endings,1);rt.flush_ended(|_|{endings+=1;true});assert_eq!(endings,1);assert_eq!(rt.stats().nonfinite_frames,0);
}

fn native65_kernel_instrument(channels:usize,key:u8,high:bool,tracking:f64)->ir::Instrument{
    let mut instrument=ir::Instrument::default();
    instrument.assets.push(ir::Asset{location:ir::AssetLocation::Path("native65-authored.wav".into()),encoding:ir::Encoding::Wav,residency:ir::AssetResidency::Resident,root_key:None,loops:vec![]});
    let mut zone=ir::Zone::new(ir::AssetRef(0));zone.keys=ir::KeyRange{low:key,high:key};zone.pitch=ir::KeyTracking::Fixed;zone.velocity=ir::VelocityResponse::None;instrument.zones.push(zone);
    instrument.controls.push(ir::Control{key:"native65:freq".into(),label:"frequency".into(),value:ir::ControlValue::Continuous{min:20.,max:20000.,default:1800.,unit:ir::ControlUnit::Hertz},automation:ir::Automation::None});
    let mut nodes=vec![ir::ProcessingSignalNode::UviOnePole(ir::UviNativeOnePole{frequency:ir::SignalParameter{constant:1800.,control:Some("native65:freq".into()),expression:None},tracking,mode:if high{ir::ChannelOnePoleMode::HighPass}else{ir::ChannelOnePoleMode::LowPass},cadence:ir::UviOnePoleCadence::FirstSetup32,tail_frames:0})];
    if channels==6 {nodes.push(ir::ProcessingSignalNode::UviLayout(ir::UviNativeLayoutConvert{source:ir::UviNativeLayout::Code6,destination:ir::UviNativeLayout::Code2,pan:ir::SignalParameter{constant:0.,control:None,expression:None},pan_offset:0.}));}
    instrument.processing_scopes.push(ir::ProcessingScope{members:vec![ir::ZoneRef(0)],sum:ir::ProcessingSumLaw::OrderedF32,cadence:ir::ProcessingCadence::CoreCell64,chain:None,amplitude:None,output:None,
        signal:Some(ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels},nodes,controls:vec![]})});instrument
}
fn native65_pcm(frames:usize,channels:usize)->Vec<Vec<f32>>{(0..channels).map(|channel|(0..frames).map(|frame|if(frame/(channel+3))%2==0{(channel+1)as f32/64.}else{-((channel+1)as f32)/128.}).collect()).collect()}
fn native65_plan(instrument:&ir::Instrument,values:&[Vec<f32>])->sampler_core::Prepared{
    let channels=values.len();let frames=values[0].len();
    let source=if channels==2{Pcm::new(48000,(0..frames).map(|frame|[values[0][frame],values[1][frame]]).collect::<Vec<_>>().into_boxed_slice()).unwrap().into()}
        else{let packs=(0..channels/2).map(|pack|Pcm::new(48000,(0..frames).map(|frame|[values[pack*2][frame],values[pack*2+1][frame]]).collect::<Vec<_>>().into_boxed_slice()).unwrap()).collect();sampler_core::SourceStorageFamily::new_resident(sampler_core::ChannelCount::new(channels).unwrap(),packs).unwrap().into()};
    sampler_core::lower::lower_sources(instrument,48000,vec![source],|_,plan|Ok(plan)).unwrap()
}
fn native65_coefficient(note:f64,tracking:f64,hz:f32)->f32{
    let octave=(((note as f32)-60.)*(tracking as f32))*(1.0f32/12.);
    let tracked=(2.0f32.powf(octave)*hz).min(24000.);
    (-(std::f64::consts::TAU*f64::from(tracked)/48000.)).exp()as f32
}
#[test]fn native65_six_channel_onepole_tracking_low_and_high_match_independent_f32_recurrence(){
    for high in [false,true]{
        let values=native65_pcm(4096,6);let instrument=native65_kernel_instrument(6,72,high,1.);let plan=native65_plan(&instrument,&values);
        let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();
        let input=sampler_core::Input{protocol:sampler_core::Protocol::Native,port:0,group:0,channel:0,key:72,external_id:Some(650)};
        let note=rt.trigger(input,72,1.).unwrap();let mut output=vec![[0.;2];1024];
        let mut at=0;for size in [1,17,31,32,33,63,64,65].into_iter().cycle(){if at==output.len(){break;}let end=(at+size).min(output.len());support::without_heap(||rt.render(&mut output[at..end]).unwrap());at=end;}
        let a=native65_coefficient(72.,1.,1800.);let b=1.0f32-a;let mut state=[0f32;6];let weight=f32::from_bits(0x3f13cd3a);
        for frame in 0..output.len(){let mut filtered=[0.;6];for channel in 0..6{let original=values[channel][frame];let difference=original-state[channel];state[channel]+=difference*b;filtered[channel]=if high{original-state[channel]}else{state[channel]};}
            let expected=[filtered[0]*weight+filtered[4]*weight,filtered[1]*weight+filtered[5]*weight];assert_eq!(output[frame],expected,"original f32 N law frame{frame} high{high}");}
        rt.release(note).unwrap();let mut drain=[[0.;2];128];rt.render(&mut drain).unwrap();assert_eq!(rt.processing_owner_count(),0);assert_eq!(rt.stats().nonfinite_frames,0);
    }
}
// Independent stereo oracle stores two input/output time rows, rather than
// production's flat history owner, and respects actual Core64 invocation cuts.
fn native65_stereo_reference(values:&[Vec<f32>],ranges:&[(usize,usize)],high:bool)->Vec<[f32;2]>{
    let n=ranges.last().map_or(0,|range|range.1);let mut output=vec![[0.;2];n];let a=native65_coefficient(60.,0.,1800.);let b=1.0f32-a;let aa=a*a;let ab=a*b;
    let mut x=[[0f32;2];2];let mut y=[[0f32;2];2];
    for &(start,end)in ranges{let mut cursor=start;while cursor<end{let stop=end.min((cursor/64+1)*64);for channel in 0..2{
        let mut frame=cursor;
        while frame+4<=stop{let input=[values[channel][frame],values[channel][frame+1],values[channel][frame+2],values[channel][frame+3]];
            let first=(input[0]*b+x[1][channel]*ab)+y[0][channel]*aa;
            let second=(input[1]*b+input[0]*ab)+y[1][channel]*aa;
            let third=(input[1]*ab+input[2]*b)+first*aa;
            let fourth=(input[2]*ab+input[3]*b)+second*aa;
            for(offset,result)in[first,second,third,fourth].into_iter().enumerate(){output[frame+offset][channel]=if high{input[offset]-result}else{result};}
            x[0][channel]=input[2];x[1][channel]=input[3];y[0][channel]=third;y[1][channel]=fourth;frame+=4;
        }
        while frame<stop{let input=values[channel][frame];let result=y[1][channel]+(input-y[1][channel])*b;
            output[frame][channel]=if high{input-result}else{result};x[0][channel]=x[1][channel];x[1][channel]=input;y[0][channel]=y[1][channel];y[1][channel]=result;frame+=1;}
    }cursor=stop;}}
    output
}
#[test]fn native65_stereo_onepole_has_its_actual_tree_and_independent_histories_on_odd_calls(){
    for high in [false,true]{let values=native65_pcm(4096,2);let instrument=native65_kernel_instrument(2,60,high,0.);let plan=native65_plan(&instrument,&values);let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();
        let input=sampler_core::Input{protocol:sampler_core::Protocol::Native,port:0,group:0,channel:0,key:60,external_id:Some(651)};rt.trigger(input,60,1.).unwrap();
        let mut out=vec![[0.;2];512];let mut ranges=Vec::new();let mut at=0;for size in [1,17,31,32,33,63,64,65].into_iter().cycle(){if at==out.len(){break;}let end=(at+size).min(out.len());ranges.push((at,end));support::without_heap(||rt.render(&mut out[at..end]).unwrap());at=end;}
        assert_eq!(out,native65_stereo_reference(&values,&ranges,high));assert_eq!(rt.stats().nonfinite_frames,0);
    }
}
#[test]fn native65_live_native_frequency_changes_same_owner_without_heap_and_reuses_clean_history(){
    let values=native65_pcm(4096,6);let instrument=native65_kernel_instrument(6,60,false,0.);let plan=native65_plan(&instrument,&values);let priced=plan.voice_state_bytes();
    let mut core=instrument.clone();if let ir::ProcessingSignalNode::UviOnePole(filter)=&core.processing_scopes[0].signal.as_ref().unwrap().nodes[0]{let hz=filter.frequency.clone();core.processing_scopes[0].signal.as_mut().unwrap().nodes[0]=ir::ProcessingSignalNode::OnePole(ir::ChannelOnePole{cutoff_hz:hz,mode:ir::ChannelOnePoleMode::LowPass,arithmetic:ir::ChannelOnePoleArithmetic::CoreOnePoleF64,tail_frames:0});}
    assert!(priced>=native65_plan(&core,&values).voice_state_bytes()+64,"native coefficient/note/setup scalars are real priced owner state");
    let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();let input=sampler_core::Input{protocol:sampler_core::Protocol::Native,port:0,group:0,channel:0,key:60,external_id:Some(652)};
    let first=rt.trigger(input,60,1.).unwrap();let mut before=[[0.;2];257];support::without_heap(||rt.render(&mut before).unwrap());let mut owners=Vec::new();rt.visit_processing_owners(|owner|owners.push((owner.id,owner.note,owner.plan)));
    let plan=rt.active_plan();let id=sampler_core::lower::ir_control_id("native65:freq");support::without_heap(||{rt.edit_controls(plan,None,&[sampler_core::ControlWrite{id,value:sampler_core::ControlValue::Real(3600.)}]).unwrap();});let mut after=[[0.;2];257];support::without_heap(||rt.render(&mut after).unwrap());
    let mut current=Vec::new();rt.visit_processing_owners(|owner|current.push((owner.id,owner.note,owner.plan)));assert_eq!(owners,current);
    // One uninterrupted independent recurrence crosses the real edit frame.
    // The reference never reads production histories, controls or coefficients.
    let mut expected_history=[0f32;6];let weight=f32::from_bits(0x3f13cd3a);
    for frame in 0..514{let beta=1.-native65_coefficient(60.,0.,if frame<257{1800.}else{3600.});
        for channel in 0..6{expected_history[channel]+=(values[channel][frame]-expected_history[channel])*beta;}
        let expected=[expected_history[0]*weight+expected_history[4]*weight,expected_history[1]*weight+expected_history[5]*weight];
        assert_eq!(if frame<257{before[frame]}else{after[frame-257]},expected,"live F257 update preserves the SAME prior filter state at frame{frame}");
    }
    rt.release(first).unwrap();rt.render(&mut [[0.;2];128]).unwrap();assert_eq!(rt.processing_owner_count(),0);
    let second=rt.trigger(sampler_core::Input{external_id:Some(653),..input},60,1.).unwrap();let mut fresh=[[0.;2];64];support::without_heap(||rt.render(&mut fresh).unwrap());
    let coefficient=native65_coefficient(60.,0.,3600.);let beta=1.-coefficient;let mut history=[0f32;6];let weight=f32::from_bits(0x3f13cd3a);
    for frame in 0..64{for channel in 0..6{history[channel]+=(values[channel][frame]-history[channel])*beta;}assert_eq!(fresh[frame],[history[0]*weight+history[4]*weight,history[1]*weight+history[5]*weight],"old coefficient and filter histories must not leak into reused activation");}
    rt.release(second).unwrap();assert_eq!(rt.stats().nonfinite_frames,0);
}
#[test]fn native65_forged_header_storage_geometry_refuses_before_behavior_binder(){
    let values=native65_pcm(512,2);let mut instrument=native65_kernel_instrument(6,60,false,0.);let mut members=[None;16];members[0]=Some(ir::UviNativeSourceInput{layout:ir::UviNativeLayout::Code6,storage_channels:6,provenance:ir::UviNativeSourceProvenance::PlainHeader});
    instrument.processing_scopes[0].signal.as_mut().unwrap().source_law=ir::ProcessingSourceLaw::UviC997Layout{members};
    instrument.behaviors.push(ir::Behavior{name:"must-not-bind".into(),language:ir::Language::Ksp,source:String::new(),slot:None,state:vec![],requires:vec![]});let bound=std::cell::Cell::new(false);
    let pcm=Pcm::new(48000,(0..512).map(|frame|[values[0][frame],values[1][frame]]).collect::<Vec<_>>().into_boxed_slice()).unwrap();
    let result=sampler_core::lower::lower_sources(&instrument,48000,vec![pcm.into()],|_,plan|{bound.set(true);Ok(plan)});assert!(result.is_err());assert!(!bound.get());
}

// Supplement67: Core's established permitted allocator covers the actual typed
// native outlet + held canonical-expression edits/render/terminal lifecycle.
fn native67_core_reference(values:&[Vec<f32>],frames:usize)->Vec<[f32;2]>{
    let beta=1.-native65_coefficient(60.,0.,1800.);let weight=f32::from_bits(0x3f13cd3a);let mut history=[0f32;6];
    (0..frames).map(|frame|{for channel in 0..6{history[channel]+=(values[channel][frame]-history[channel])*beta;}[history[0]*weight+history[4]*weight,history[1]*weight+history[5]*weight]}).collect()
}
fn native67_assert_frames(actual:&[[f32;2]],expected:&[[f32;2]]){
    assert_eq!(actual.len(),expected.len());for(frame,(a,b))in actual.iter().zip(expected).enumerate(){for channel in 0..2{assert!((a[channel]-b[channel]).abs()<3e-6,"native expression frame{frame} channel{channel}: {} vs {}",a[channel],b[channel]);}}
}
#[test]
fn native67_typed_wide_expression_edits_preserve_sibling_history_and_reused_note_without_heap(){
    use sampler_core::Expression;
    let values=native65_pcm(4096,6);let mut instrument=native65_kernel_instrument(6,60,false,0.);
    instrument.processing_scopes[0].signal.as_mut().unwrap().nodes.push(ir::ProcessingSignalNode::Pan(ir::ChannelPan{position:ir::SignalParameter{constant:0.,control:None,expression:None},offset:0.,law:ir::PanLaw::RaisedCosineCoreF64,expression:true}));
    let plan=native65_plan(&instrument,&values);let limits=sampler_core::Limits::for_plan(&plan,4,8);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();
    let source=|id|sampler_core::Input{protocol:sampler_core::Protocol::Native,port:0,group:0,channel:0,key:60,external_id:Some(id)};
    let first=rt.trigger(source(670),60,1.).unwrap();let second=rt.trigger(source(671),60,1.).unwrap();let a=rt.expression_id(first).unwrap();let b=rt.expression_id(second).unwrap();assert_ne!(a,b);
    let reference=native67_core_reference(&values,1024);let expected_prefix:Vec<_>=reference[..257].iter().map(|v|[v[0]+v[0],v[1]+v[1]]).collect();let mut prefix=[[0.;2];257];support::without_heap(||rt.render(&mut prefix).unwrap());native67_assert_frames(&prefix,&expected_prefix);
    let mut owners=Vec::new();rt.visit_processing_owners(|v|owners.push((v.id,v.note,v.plan)));assert_eq!(owners.len(),2);let plan=rt.active_plan();let revision=rt.control_revision(plan).unwrap();let clock=rt.now();
    let expected_right:Vec<_>=reference[257..322].iter().map(|v|[v[0],v[1]+v[1]]).collect();let mut right=[[0.;2];65];
    support::without_heap(||{rt.set_expression(a,Expression{pan:1.,..Default::default()}).unwrap();assert_eq!(rt.now(),clock);rt.render(&mut right).unwrap();});native67_assert_frames(&right,&expected_right);
    assert_eq!(rt.expression(b),Ok(Expression::default()));assert_eq!(rt.control_revision(plan),Ok(revision));let mut current=Vec::new();rt.visit_processing_owners(|v|current.push((v.id,v.note,v.plan)));assert_eq!(current,owners);
    let expected_left:Vec<_>=reference[322..419].iter().map(|v|[v[0]+v[0],v[1]]).collect();let mut left=[[0.;2];97];
    support::without_heap(||{rt.set_expression(a,Expression{pan:-1.,..Default::default()}).unwrap();rt.render(&mut left).unwrap();});native67_assert_frames(&left,&expected_left);assert_eq!(rt.now(),419);assert_eq!(rt.note(second),Ok((60,1.,true)));
    let expected_sibling=reference[419..484].to_vec();let mut sibling=[[0.;2];65];support::without_heap(||{rt.release(first).unwrap();rt.render(&mut sibling).unwrap();});native67_assert_frames(&sibling,&expected_sibling);
    assert_eq!((rt.voice_count(),rt.processing_owner_count()),(1,1));let mut endings=[0usize;3];support::without_heap(||rt.flush_ended(|input|{assert_eq!(input,source(670));endings[0]+=1;true}));assert_eq!(endings,[1,0,0]);
    let mut admitted=None;support::without_heap(||{admitted=Some(rt.trigger(source(672),60,1.).unwrap());});let third=admitted.unwrap();let c=rt.expression_id(third).unwrap();assert_ne!(c,b);assert_eq!(rt.expression(c),Ok(Expression::default()));assert_eq!(rt.expression(b),Ok(Expression::default()));
    let expected_fresh:Vec<_>=reference[484..549].iter().zip(&reference[..65]).map(|(held,new)|[held[0]+new[0],held[1]+new[1]]).collect();let mut fresh=[[0.;2];65];support::without_heap(||rt.render(&mut fresh).unwrap());native67_assert_frames(&fresh,&expected_fresh);
    assert_eq!(rt.now(),549);assert_eq!(rt.control_revision(plan),Ok(revision));let mut tail=[[0.;2];128];support::without_heap(||{rt.release(second).unwrap();rt.release(third).unwrap();rt.render(&mut tail).unwrap();rt.flush_ended(|input|{if input==source(671){endings[1]+=1;}else if input==source(672){endings[2]+=1;}else{panic!("duplicate/unexpected terminal: {input:?}");}true});});assert_eq!(endings,[1;3]);assert_eq!((rt.voice_count(),rt.processing_owner_count()),(0,0));assert_eq!(rt.now(),677);
    let stats=rt.stats();assert_eq!(stats.nonfinite_frames,0);assert_eq!(stats.stream_underruns,0);assert_eq!(stats.refused_starts,0);assert_eq!(stats.voice_pool_refusals,0);assert_eq!(stats.voice_drops,0);
}


// Implementation-dependent binding148 public cases. Baseline-compatible
// importer/product tests remain separately frozen in tests02.
fn binding148_graph(two_scopes:bool)->ir::Instrument{
    let mut instrument=graph(true);
    let filter=ir::Processor::Filter(ir::Filter{kind:ir::FilterKind::LowPass{poles:1},cutoff:ir::Frequency::Hertz(1000.),resonance:ir::Resonance::Q(0.7)});
    instrument.chains.push(ir::Chain{scope:ir::Scope::Voice,pre_amplitude:vec![filter],post_amplitude:vec![]});
    instrument.processing_scopes[0].chain=Some(ir::ChainRef(0));instrument.source_indices.processing[0].chain=Some(ir::ChainRef(0));
    instrument.modulators.push(ir::Modulator{scope:ir::Scope::Voice,source:ir::ModulationSource::Velocity});
    let count=if two_scopes{2}else{1};
    if two_scopes {
        for n in 2..4{instrument.assets.push(ir::Asset{location:ir::AssetLocation::Path(format!("binding148-{n}.wav")),encoding:ir::Encoding::Wav,residency:ir::AssetResidency::Resident,root_key:None,loops:vec![]});let mut z=instrument.zones[n-2].clone();z.asset=Some(ir::AssetRef(n));instrument.zones.push(z);}
        instrument.chains.push(instrument.chains[0].clone());
        let mut scope=instrument.processing_scopes[0].clone();scope.members=vec![ir::ZoneRef(2),ir::ZoneRef(3)];scope.chain=Some(ir::ChainRef(1));instrument.processing_scopes.push(scope);
        let mut recipe=instrument.source_indices.processing[0].clone();recipe.owner=ir::SourceOwnerId::UviNode(4);recipe.members=vec![ir::ZoneRef(2),ir::ZoneRef(3)];recipe.chain=Some(ir::ChainRef(1));recipe.compiled=Some(1);instrument.source_indices.processing.push(recipe);
        instrument.source_indices.owners[0].children.push(ir::SourceOwnerId::UviNode(4));instrument.source_indices.owners.extend([owner(4,Some(0),ir::SourceOwnerKind::Keygroup,&[5,6],&[2,3]),owner(5,Some(4),ir::SourceOwnerKind::Player,&[],&[2]),owner(6,Some(4),ir::SourceOwnerKind::Player,&[],&[3])]);
    }
    for scope in 0..count {
        let control=ir::ControlRef(instrument.controls.len());instrument.controls.push(ir::Control{key:format!("binding148-freq-{scope}"),label:"same Frequency schema".into(),value:ir::ControlValue::Continuous{min:20.,max:20000.,default:1000.,unit:ir::ControlUnit::Hertz},automation:ir::Automation::None});
        instrument.processor_controls.push(ir::ProcessorControl{control,chain:ir::ChainRef(scope),index:0,parameter:ir::ProcessorParameter::Cutoff,ramp:ir::Time::ZERO});
        let owner=instrument.source_indices.processing[scope].owner;
        instrument.source_indices.processing_parameters.push(ir::SourceProcessingParameter{owner,source_node:20+scope as u32,chain:ir::ChainRef(scope),index:0,parameter:ir::ProcessorParameter::Cutoff,control,law:ir::SourceProcessingParameterLaw::LegacyCutoffSemitoneMidpoint,fixed_factors:vec![],compiled:Some(ir::SourceProcessingParameterSite{scope,site:ir::ScopeControlSite::ChainPre(0)})});
        let route=ir::RouteRef(instrument.routes.len());instrument.routes.push(ir::Route::new(ir::ModulatorRef(0),ir::Target::Processor{chain:ir::ChainRef(scope),index:0,parameter:ir::ProcessorParameter::Cutoff},ir::Depth::Pitch(ir::Pitch::Semitones(12.))));
        instrument.source_indices.processing[scope].sources=vec![ir::ModulatorRef(0)];instrument.source_indices.processing[scope].routes=vec![route];
    }instrument
}
fn binding148_core_plan(two:bool)->sampler_core::Prepared{
    let instrument=binding148_graph(two);let pcm=(0..instrument.assets.len()).map(|n|Pcm::new(48000,(0..4096).map(|i|[if i%(31+n*2)<15{0.125}else{-0.125};2]).collect::<Vec<_>>().into_boxed_slice()).unwrap()).collect();
    sampler_core::lower::lower_with(&instrument,48000,pcm,&sampler_core::lower::Options{mpe:None},|_,plan|Ok(plan)).unwrap()
}
#[test]
fn binding148_cross_scope_same_kind_same_schema_target_is_not_current_owner(){
    use sampler_core::{ModProgram,ModSource,ModRoute,ModTarget};let plan=binding148_core_plan(true);
    let wrong=ModProgram{sources:vec![ModSource::Constant],routes:vec![ModRoute::new(0,ModTarget::ProcessingParameter(1,0),12.)],..Default::default()};
    assert!(matches!(plan.with_processing_modulation(vec![wrong],vec![Some(0),Some(0)],1<<20),Err(Error::InvalidInput)),"another real scope's same-kind/same-schema binding is not authority");
}
#[test]
fn binding148_physical_program_cannot_escape_to_valid_real_scope_parameter(){
    use sampler_core::{ModProgram,ModSource,ModRoute,ModTarget};let plan=binding148_core_plan(false);
    let program=ModProgram{sources:vec![ModSource::Constant],routes:vec![ModRoute::new(0,ModTarget::ProcessingParameter(0,0),12.)],..Default::default()};
    assert!(matches!(plan.with_voice_modulation(vec![program],vec![Some(0);2],vec![0;2]),Err(Error::InvalidInput)));
}
#[test]
fn binding148_malformed_original_or_compiled_operand_refuses_before_binder(){
    for malformed in 0..6 {
        let mut instrument=binding148_graph(false);match malformed {
            0=>instrument.source_indices.processing_parameters[0].control=ir::ControlRef(usize::MAX),
            1=>instrument.source_indices.processing_parameters[0].index=99,
            2=>instrument.source_indices.processing_parameters[0].owner=ir::SourceOwnerId::UviNode(2),
            3=>instrument.source_indices.processing_parameters[0].compiled=Some(ir::SourceProcessingParameterSite{scope:0,site:ir::ScopeControlSite::ChainPre(99)}),
            4=>instrument.zones[0].routes.push(instrument.source_indices.processing[0].routes[0]),
            _=>instrument.source_indices.processing_parameters.push(instrument.source_indices.processing_parameters[0].clone()),
        }
        let called=std::cell::Cell::new(false);let result=lower(&instrument,48000,pcm(),|_,plan|{called.set(true);Ok(plan)});assert!(result.is_err());assert!(!called.get());
    }
}
#[test]
fn binding148_shared_processor_control_render_and_release_use_no_heap_and_same_owner(){
    use sampler_core::{ControlValue,ControlWrite};let plan=binding148_core_plan(false);let limits=sampler_core::Limits::for_plan(&plan,8,16);let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();
    let note=rt.trigger(pan_input(681),60,0.5).unwrap();let mut first=[[0.;2];127];support::without_heap(||rt.render(&mut first).unwrap());assert!(first.iter().flatten().any(|v|v.abs()>1e-4));
    let mut identity=Vec::new();rt.visit_processing_owners(|o|identity.push((o.id,o.note,o.plan,o.sources)));let plan=rt.active_plan();let id=sampler_core::lower::ir_control_id("binding148-freq-0");
    support::without_heap(||{rt.edit_controls(plan,None,&[ControlWrite{id,value:ControlValue::Real(4000.)}]).unwrap();rt.render(&mut first).unwrap();});
    let mut after=Vec::new();rt.visit_processing_owners(|o|after.push((o.id,o.note,o.plan,o.sources)));assert_eq!(identity,after);assert_eq!(rt.note_plan(note),Ok(plan));
    support::without_heap(||{rt.release(note).unwrap();rt.render(&mut first).unwrap();rt.flush_ended(|_|true);});assert_eq!(rt.stats().nonfinite_frames,0);assert_eq!(rt.processing_owner_count(),0);
}
#[test]
fn binding148_old_generation_control_projection_cannot_mutate_replacement(){
    use sampler_core::{ControlValue,ControlWrite};let plan=binding148_core_plan(false);let limits=sampler_core::Limits::for_plan(&plan,8,16);let(mut actual,mut transfer)=sampler_core::Runtime::with_plan_updates(plan,limits,3,2).unwrap();
    let old=actual.active_plan();let note=actual.trigger(pan_input(682),60,0.5).unwrap();let mut out=[[0.;2];127];actual.render(&mut out).unwrap();
    let request=transfer.submit(Box::new(binding148_core_plan(false))).unwrap();assert_eq!(actual.poll_plan_update(),Ok(Some(request)));let new=actual.active_plan();assert_ne!(old,new);
    let id=sampler_core::lower::ir_control_id("binding148-freq-0");support::without_heap(||{actual.edit_controls(old,None,&[ControlWrite{id,value:ControlValue::Real(4000.)}]).unwrap();actual.render(&mut out).unwrap();});
    assert_eq!(actual.control_base_value(old,id),Ok(ControlValue::Real(4000.)));assert_eq!(actual.control_base_value(new,id),Ok(ControlValue::Real(1000.)));assert_eq!(actual.note_plan(note),Ok(old));
    let mut owners=0;actual.visit_processing_owners(|o|{assert_eq!(o.plan,old);owners+=1;});assert_eq!(owners,1);assert!(out.iter().flatten().any(|v|v.abs()>1e-4));assert_eq!(actual.stats().nonfinite_frames,0);
    support::without_heap(||{actual.release(note).unwrap();actual.render(&mut out).unwrap();actual.flush_ended(|_|true);});assert_eq!(actual.processing_owner_count(),0);
}

#[test]
fn binding148_same_control_same_coordinate_two_pole_replacement_is_not_the_bound_operand(){
    use sampler_core::{ControlRange,Parameter,Processor,StateVariableFilter,SvfMode,VoiceChain};
    let plan=binding148_core_plan(false);
    let id=sampler_core::lower::ir_control_id("binding148-freq-0");
    let chain=VoiceChain::new(vec![Processor::StateVariable(StateVariableFilter{mode:SvfMode::LowPass,
        cutoff_hz:Parameter::Control(ControlRange{control:id,low:20.,high:20000.,ramp_frames:0}),q:Parameter::Constant(0.7)})],vec![],0);
    // Same real control/schema/chain coordinate, different semantic processor.
    let chain=chain.unwrap();assert!(matches!(plan.with_voice_chains(vec![chain],vec![None;2]),Err(Error::InvalidInput)));
}
#[test]
fn binding148_ordered_fixed_frequency_fold_preserves_raw_control_base_and_live_edits(){
    fn runtime(fold:bool)->sampler_core::Runtime{
        let mut ir=binding148_graph(false);
        if fold {
            ir.source_indices.processing_parameters[0].law=ir::SourceProcessingParameterLaw::HistoricalUviFrequencyThreeDecadeCore;
            ir.source_indices.processing_parameters[0].fixed_factors=vec![ir::SourceFrequencyFold::MultiplyClamp(100.),ir::SourceFrequencyFold::MultiplyClamp(0.01)];
            // The original importer chain has the real ordered fold already;
            // its real raw control remains 1000. Ordered clamps yield 200.
            if let ir::Processor::Filter(f)=&mut ir.chains[0].pre_amplitude[0]{f.cutoff=ir::Frequency::Hertz(200.);}
        }else if let ir::ControlValue::Continuous{default,..}=&mut ir.controls[0].value{*default=200.;}
        let source=(0..2).map(|_|Pcm::new(48000,(0..4096).map(|i|[if i%37<19{0.125}else{-0.1};2]).collect::<Vec<_>>().into_boxed_slice()).unwrap()).collect();
        let plan=sampler_core::lower::lower(&ir,48000,source,|_,plan|Ok(plan)).unwrap();let limits=sampler_core::Limits::for_plan(&plan,8,16);sampler_core::Runtime::new(plan,limits).unwrap()
    }
    let mut target=runtime(true);let mut reference=runtime(false);target.trigger(pan_input(683),60,0.5).unwrap();reference.trigger(pan_input(684),60,0.5).unwrap();
    let id=sampler_core::lower::ir_control_id("binding148-freq-0");let plan=target.active_plan();let other=reference.active_plan();
    let mut got=[[0.;2];257];let mut expected=[[0.;2];257];
    support::without_heap(||{target.render(&mut got).unwrap();reference.render(&mut expected).unwrap();});assert_eq!(got,expected);
    assert_eq!(target.control_base_value(plan,id),Ok(sampler_core::ControlValue::Real(1000.)));
    // 100*100 clamps at10000 then*.01 =>100, not original XML1000
    // and not the folded200 from the previous accepted raw control.
    support::without_heap(||{target.edit_controls(plan,None,&[sampler_core::ControlWrite{id,value:sampler_core::ControlValue::Real(100.)}]).unwrap();
        reference.edit_controls(other,None,&[sampler_core::ControlWrite{id,value:sampler_core::ControlValue::Real(100.)}]).unwrap();
        target.render(&mut got).unwrap();reference.render(&mut expected).unwrap();});assert_eq!(got,expected);
    assert_eq!(target.control_base_value(plan,id),Ok(sampler_core::ControlValue::Real(100.)));assert_eq!(target.stats().nonfinite_frames,0);
}
#[test]
fn binding148_route_slice_and_owner_state_share_one_checked_preparation_allowance(){
    use sampler_core::{ModProgram,ModSource,ModRoute,ModTarget};
    fn attempt(bytes:usize)->Result<sampler_core::Prepared,Error>{let plan=binding148_core_plan(false);
        let p=ModProgram{sources:vec![ModSource::Constant],routes:(0..257).map(|_|ModRoute::new(0,ModTarget::ProcessingParameter(0,0),0.01)).collect(),..Default::default()};
        plan.with_processing_modulation(vec![p],vec![Some(0)],bytes)
    }
    assert!(matches!(attempt(1),Err(Error::Capacity)));let mut low=1usize;let mut high=1<<20;assert!(attempt(high).is_ok());
    // Public preparation boundary, not a duplicate private struct-size formula.
    while high-low>1{let middle=low+(high-low)/2;if attempt(middle).is_ok(){high=middle;}else{low=middle;}}
    assert!(matches!(attempt(high-1),Err(Error::Capacity)));let plan=attempt(high).unwrap();
    let prepared_floor=high;
    let limits=sampler_core::Limits::for_plan(&plan,8,16);assert_eq!(limits.voices,16);
    // Prepared checks one minimum owner row. Runtime reserves its actual16
    // owner rows PLUS generation projection scratch from the SAME allowance.
    assert!(matches!(sampler_core::Runtime::new(plan,limits),Err(Error::Capacity)));
    fn runtime_attempt(bytes:usize)->Result<sampler_core::Runtime,Error>{
        let plan=attempt(bytes)?;let limits=sampler_core::Limits::for_plan(&plan,8,16);assert_eq!(limits.voices,16);
        sampler_core::Runtime::new(plan,limits)
    }
    fn runtime_fits(bytes:usize)->bool{match runtime_attempt(bytes){Ok(_)=>true,Err(error)=>{assert_eq!(error,Error::Capacity);false}}}
    // Test caller admission ceiling, matching the existing normal scoped
    // allowance. It does not raise any production limit or promise a fit.
    const RUNTIME_TEST_CEILING:usize=64*1024*1024;
    let mut runtime_low=prepared_floor;let mut runtime_high=1<<20;
    let mut admitted=runtime_fits(runtime_high);
    // Initial1MiB probe plus at most SIX doublings reaches the finite64MiB
    // ceiling. No acceptance there is a visible genuine test blocker.
    for _ in 0..6{if admitted{break;}runtime_low=runtime_high;
        runtime_high=runtime_high.checked_mul(2).filter(|&n|n<=RUNTIME_TEST_CEILING).expect("bounded runtime search must stay within normal64MiB allowance");
        admitted=runtime_fits(runtime_high);
    }
    assert!(admitted,"Runtime16 has no public admission within the unchanged normal64MiB scoped allowance");
    while runtime_high-runtime_low>1{let middle=runtime_low+(runtime_high-runtime_low)/2;
        if runtime_fits(middle){runtime_high=middle;}else{runtime_low=middle;}
    }
    assert!(runtime_high>prepared_floor);
    assert!(matches!(runtime_attempt(runtime_high-1),Err(Error::Capacity)));
    let mut rt=runtime_attempt(runtime_high).unwrap();let mut output=[[0.;2];65];
    // Offaudio measured public admission receipt, never an input to any law.
    eprintln!("binding148-budget prepared_floor={prepared_floor} runtime_floor={runtime_high} ceiling={RUNTIME_TEST_CEILING} owners=16 sources=1 routes=257 public_modroute_bytes={}",std::mem::size_of::<ModRoute>());
    support::without_heap(||{rt.trigger(pan_input(685),60,0.5).unwrap();rt.render(&mut output).unwrap();});
    assert_eq!(rt.now(),65);assert_eq!((rt.voice_count(),rt.processing_owner_count()),(2,1));
    assert!(output.iter().flatten().any(|v|v.abs()>1e-4));assert_eq!(rt.stats().nonfinite_frames,0);
}

// Correction R148-01: a fixed fold still requires its actual owner projector.
fn binding148_r1_fixed_plan(two:bool,folded_reference:bool)->sampler_core::Prepared{
    let mut ir=binding148_graph(two);
    for recipe in &mut ir.source_indices.processing{recipe.sources.clear();recipe.routes.clear();}
    for row in &mut ir.source_indices.processing_parameters{
        if !folded_reference {row.law=ir::SourceProcessingParameterLaw::HistoricalUviFrequencyThreeDecadeCore;
            row.fixed_factors=vec![ir::SourceFrequencyFold::MultiplyClamp(100.),ir::SourceFrequencyFold::MultiplyClamp(0.01)];}
    }
    if folded_reference{for control in &mut ir.controls{if let ir::ControlValue::Continuous{default,..}=&mut control.value{*default=200.;}}}
    let values=(0..ir.assets.len()).map(|_|Pcm::new(48000,(0..4096).map(|i|[if i%37<19{0.0625}else{-0.0625};2]).collect::<Vec<_>>().into_boxed_slice()).unwrap()).collect();
    sampler_core::lower::lower_with(&ir,48000,values,&sampler_core::lower::Options{mpe:None},|_,plan|Ok(plan)).unwrap()
}
#[test]
fn binding148_r1_zero_cleared_and_missing_program_maps_refuse_before_runtime(){
    use sampler_core::ModProgram;
    for (programs,scopes) in [(vec![],vec![]),(vec![],vec![None]),(vec![ModProgram::default()],vec![None])]{
        let result=binding148_r1_fixed_plan(false,false).with_processing_modulation(programs,scopes,1<<20);
        assert!(matches!(result,Err(Error::InvalidInput)),"retained folded home cannot lose the required actual program at preparation");
    }
    for (programs,scopes) in [(vec![],vec![None,None]),(vec![ModProgram::default()],vec![Some(0),None]),(vec![ModProgram::default()],vec![None,Some(0)])]{
        let result=binding148_r1_fixed_plan(true,false).with_processing_modulation(programs,scopes,1<<20);
        assert!(matches!(result,Err(Error::InvalidInput)),"one mapped scope cannot hide another owner's missing projector");
    }
}
#[test]
fn binding148_r1_explicit_empty_real_program_preserves_static_fold_pcm_and_base(){
    let plan=binding148_r1_fixed_plan(true,false).with_processing_modulation(vec![sampler_core::ModProgram::default()],vec![Some(0),Some(0)],1<<20).unwrap();
    let limits=sampler_core::Limits::for_plan(&plan,8,16);let mut actual=sampler_core::Runtime::new(plan,limits).unwrap();
    let reference=binding148_r1_fixed_plan(true,true);let limits=sampler_core::Limits::for_plan(&reference,8,16);let mut reference=sampler_core::Runtime::new(reference,limits).unwrap();
    actual.trigger(pan_input(686),60,1.).unwrap();reference.trigger(pan_input(687),60,1.).unwrap();
    let mut got=[[0.;2];257];let mut expected=[[0.;2];257];support::without_heap(||{actual.render(&mut got).unwrap();reference.render(&mut expected).unwrap();});assert_eq!(got,expected);
    assert!(got.iter().flatten().any(|v|v.abs()>1e-4));assert_eq!(actual.processing_owner_count(),2);assert_eq!(actual.voice_count(),4);
    for scope in 0..2{let id=sampler_core::lower::ir_control_id(&format!("binding148-freq-{scope}"));assert_eq!(actual.control_base_value(actual.active_plan(),id),Ok(sampler_core::ControlValue::Real(1000.)));}
    assert_eq!(actual.control_revision(actual.active_plan()),Ok(0));assert_eq!(actual.stats().nonfinite_frames,0);
}
fn binding148_r2_runtime(physical_reference:bool,wide:bool,semitones:f64,ramp_frames:u32)->sampler_core::Runtime{
    let mut ir=binding148_graph(false);ir.routes[0].depth=ir::Depth::Pitch(ir::Pitch::Semitones(semitones));ir.processor_controls[0].ramp=ir::Time::Seconds(f64::from(ramp_frames)/48000.);
    let source_count=if physical_reference{1}else{2};
    if physical_reference {
        ir.processing_scopes.clear();ir.source_indices.processing.clear();ir.source_indices.processing_parameters.clear();ir.source_indices.owners.clear();ir.zones.truncate(1);ir.assets.truncate(1);
        ir.zones[0].chain=Some(ir::ChainRef(0));ir.zones[0].routes=vec![ir::RouteRef(0)];
    }else if wide {
        ir.processing_scopes[0].chain=None;
        ir.processing_scopes[0].signal=Some(ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:6},
            nodes:vec![ir::ProcessingSignalNode::OnePole(ir::ChannelOnePole{cutoff_hz:ir::SignalParameter{constant:1000.,control:Some("binding148-freq-0".into()),expression:None},mode:ir::ChannelOnePoleMode::LowPass,arithmetic:ir::ChannelOnePoleArithmetic::CoreOnePoleF64,tail_frames:0}),
                ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{input:ir::SignalLayout{channels:6},output:ir::SignalLayout{channels:2},cells:vec![1.,0.,0.,0.,0.5,0.,0.,1.,0.,0.,0.,0.5],arithmetic:ir::MatrixArithmetic::SeparateMulAddF32})],controls:vec![]});
        ir.source_indices.processing_parameters[0].compiled=Some(ir::SourceProcessingParameterSite{scope:0,site:ir::ScopeControlSite::SignalNode(0)});
    }
    let sources=(0..source_count).map(|_|{
        let magnitude=if physical_reference{if wide{0.1875}else{0.125}}else{0.0625};
        let pcm=||Pcm::new(48000,(0..4096).map(|i|[if i%37<19{magnitude}else{-magnitude};2]).collect::<Vec<_>>().into_boxed_slice()).unwrap();
        if !physical_reference&&wide {sampler_core::SourceStorageFamily::new_resident(sampler_core::ChannelCount::new(6).unwrap(),vec![pcm(),pcm(),pcm()]).unwrap().into()}else{pcm().into()}
    }).collect();
    let plan=sampler_core::lower::lower_sources_with(&ir,48000,sources,&sampler_core::lower::Options{mpe:None},|_,plan|Ok(plan)).unwrap();
    let limits=sampler_core::Limits::for_plan(&plan,8,16);sampler_core::Runtime::new(plan,limits).unwrap()
}
#[test]
fn binding148_r2_moving_base_matches_real_physical_midpoint_or_neutral_law_on_actual_chunks(){
    use sampler_core::{ControlValue,ControlWrite,Event};
    for wide in [false,true]{for ramp in [0,37]{for semitones in [0.,12.]{
        let mut actual=binding148_r2_runtime(false,wide,semitones,ramp);let mut reference=binding148_r2_runtime(true,wide,semitones,ramp);
        actual.trigger(pan_input(688),60,1.).unwrap();reference.trigger(pan_input(689),60,1.).unwrap();let mut warm=[[0.;2];17];let mut other=[[0.;2];17];
        support::without_heap(||{actual.render(&mut warm).unwrap();reference.render(&mut other).unwrap();});
        assert!(other.iter().flatten().any(|v|v.abs()>1e-4),"ordinary physical reference must sound before the moving-base edit");
        assert!(warm.iter().flatten().zip(other.iter().flatten()).all(|(a,b)|(a-b).abs()<3e-7),"initial held history must agree before either live write");
        let id=sampler_core::lower::ir_control_id("binding148-freq-0");let a=actual.active_plan();let b=reference.active_plan();
        let mut owners=Vec::new();actual.visit_processing_owners(|o|owners.push((o.id,o.note,o.plan)));
        support::without_heap(||{
            actual.edit_controls(a,None,&[ControlWrite{id,value:ControlValue::Real(4000.)}]).unwrap();reference.edit_controls(b,None,&[ControlWrite{id,value:ControlValue::Real(4000.)}]).unwrap();
            actual.schedule_event(41,Event::Control(a,ControlWrite{id,value:ControlValue::Real(8000.)})).unwrap();reference.schedule_event(41,Event::Control(b,ControlWrite{id,value:ControlValue::Real(8000.)})).unwrap();
            actual.schedule_event(117,Event::Control(a,ControlWrite{id,value:ControlValue::Real(700.)})).unwrap();reference.schedule_event(117,Event::Control(b,ControlWrite{id,value:ControlValue::Real(700.)})).unwrap();
        });
        let mut got=[[0.;2];513];let mut expected=[[0.;2];513];let mut at=0;
        for size in [1,17,31,7,64,3,29].into_iter().cycle(){if at==got.len(){break;}let end=(at+size).min(got.len());
            support::without_heap(||{actual.render(&mut got[at..end]).unwrap();reference.render(&mut expected[at..end]).unwrap();});at=end;
        }
        let worst=got.iter().flatten().zip(expected.iter().flatten()).map(|(a,b)|(a-b).abs()).fold(0f32,f32::max);
        assert!(worst<3e-7,"actual legacy consumer mismatch wide={wide} ramp={ramp} semitones={semitones} worst={worst}");
        let mut after=Vec::new();actual.visit_processing_owners(|o|after.push((o.id,o.note,o.plan)));assert_eq!(owners,after);
        assert_eq!(actual.now(),530);assert_eq!(reference.now(),530);assert_eq!(actual.stats().nonfinite_frames,0);assert_eq!(actual.control_base_value(a,id),Ok(ControlValue::Real(700.)));
    }}}
}


// Actual retained-Plan input owner -> real post-FX outlet. No note is selected
// to manufacture the Program source input. Explicit initial values belong to
// this fixture; they are not a native constructor-default assertion.
fn element_controller_plan(external:bool)->sampler_core::Prepared {
    let mut instrument=explicit_output_instrument(false,true);
    instrument.modulators.push(ir::Modulator{scope:ir::Scope::Master,source:ir::ModulationSource::Controller(7)});
    instrument.routes.push(ir::Route::new(ir::ModulatorRef(0),ir::Target::Amplitude,ir::Depth::Normalized(1.)));
    instrument.source_indices.global_modulation.push(ir::SourceGlobalModulation{
        owner:ir::SourceOwnerId::UviNode(0),input_owner:ir::SourceOwnerId::UviNode(0),outlet:ir::BusRef(0),
        sources:vec![ir::ModulatorRef(0)],routes:vec![ir::RouteRef(0)]});
    if external{
        instrument.source_indices.owners[0].event_processors.push(ir::SourceEventProcessor{
            id:ir::SourceEventProcessorId::UviNode(40),owner:ir::SourceOwnerId::UviNode(0),ordinal:0,
            kind:"ScriptProcessor".into(),bypass:ir::SourceSwitch{saved:false,control:None}});
    }
    assert!(instrument.validate().is_ok());
    let plan=lower(&instrument,48000,pcm(),|_,p|Ok(p)).unwrap();
    assert_eq!(plan.source_controller_inputs().collect::<Vec<_>>(),vec![(ir::SourceOwnerId::UviNode(0),ir::ModulatorRef(0),7)]);
    if external{
        plan.with_source_controller_entries(vec![ir::SourceEventProcessorId::UviNode(40)],1,1<<20).unwrap()
    }else{plan}
}
fn element_controller_runtime(external:bool)->(sampler_core::Runtime,sampler_core::ReceiverId){
    let plan=element_controller_plan(external);let mut limits=sampler_core::Limits::for_plan(&plan,8,16);limits.channels=48;
    let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();
    let owner=sampler_core::OwnedReceiver::new(&rt,0,0,sampler_core::ReceiverZone::Lower,1,8).unwrap();
    let receiver=rt.bind_receiver(owner).unwrap();let plan=rt.active_plan();
    rt.bind_receiver_source_input(receiver,plan,ir::SourceOwnerId::UviNode(0),&[(ir::ModulatorRef(0),sampler_core::ReceiverValue::Bits32(u32::MAX))]).unwrap();
    (rt,receiver)
}
fn element_wire_cc(rt:&mut sampler_core::Runtime,receiver:sampler_core::ReceiverId,channel:u8,value:u32){
    rt.apply_receiver(receiver,Some(sampler_core::ReceiverChannelVoice{version:sampler_core::ReceiverVersion::Midi1,group:0,channel,
        message:sampler_core::ReceiverMessage::Control{index:7,value:sampler_core::ReceiverValue::Bits32(value)}}),false,None).unwrap();
}
fn element_ramp_literal(out:&[[f32;2]],from:f64,to:f64,begin:u64,end:u64,at:u64){
    for (n,frame)in out.iter().enumerate(){let frame_at=at+n as u64;
        let fraction=if frame_at<=begin{0.}else if frame_at>=end{1.}else{(frame_at-begin)as f64/(end-begin)as f64};
        let expected=(0.1*(from+(to-from)*fraction))as f32;
        for &sample in frame{assert!((sample-expected).abs()<3e-7,"frame={frame_at} actual={sample} expected={expected}");}
    }
}
#[test]
fn real_element_input_accepts_outside_mpe_voice_zone_without_channel_mirroring_or_heap(){
    let(mut rt,receiver)=element_controller_runtime(false);let plan=rt.active_plan();let performance=rt.performance(0).unwrap();
    let note=rt.trigger(pan_input(801),60,1.).unwrap();let address=sampler_core::ChannelAddress{protocol:sampler_core::Protocol::Midi1,port:0,group:0,channel:13};
    let mut output=[[0.;2];64];
    support::without_heap(||{element_wire_cc(&mut rt,receiver,13,0);rt.render(&mut output).unwrap();});
    element_ramp_literal(&output,1.,0.,0,64,0);
    assert_eq!(rt.channel_controller(performance,address,7),Ok(0),"element-only audience must not fabricate a selected-profile channel write");
    assert!(rt.note(note).unwrap().2);assert_eq!(rt.voice_count(),2);assert_eq!(rt.active_plan(),plan);assert_eq!(rt.stats().nonfinite_frames,0);
    support::without_heap(||{rt.note_off_exact(note,None).unwrap();rt.render(&mut output).unwrap();rt.flush_ended(|_|true);});
    assert_eq!(rt.voice_count(),0);
}
#[test]
fn element_only_callback_consume_wait_same_route_edit_and_checked_reroute_keep_real_audiences(){
    use sampler_core::{ExternalOutput as O,ExternalOutputResult as R};
    let(mut rt,receiver)=element_controller_runtime(true);let performance=rt.performance(0).unwrap();
    let note=rt.trigger(pan_input(802),60,1.).unwrap();
    element_wire_cc(&mut rt,receiver,13,0);let consumed=rt.pending_external_turn().unwrap().unwrap();
    assert_eq!(consumed.logical_channel,Some(13));rt.begin_external_turn(consumed.input).unwrap();rt.complete_external_segment(consumed.input,None).unwrap();
    let mut output=[[0.;2];16];support::without_heap(||rt.render(&mut output).unwrap());element_ramp_literal(&output,1.,1.,0,64,0);
    element_wire_cc(&mut rt,receiver,13,0);let waiting=rt.pending_external_turn().unwrap().unwrap();rt.begin_external_turn(waiting.input).unwrap();
    rt.complete_external_segment(waiting.input,Some(32)).unwrap();support::without_heap(||rt.render(&mut output).unwrap());
    rt.service_external_boundary().unwrap();let resumed=rt.pending_external_turn().unwrap().unwrap();assert_eq!(resumed.input.behavior(),waiting.input.behavior());assert_eq!(resumed.resume_frame,32);
    rt.begin_external_turn(resumed.input).unwrap();assert_eq!(rt.post_external_output(resumed.input,O::Controller{number:7,value:0}).unwrap(),R::Forwarded);
    rt.complete_external_segment(resumed.input,None).unwrap();
    let outside=sampler_core::ChannelAddress{protocol:sampler_core::Protocol::Midi1,port:0,group:0,channel:13};
    assert_eq!(rt.channel_controller(performance,outside,7),Ok(0));
    let mut tail=[[0.;2];32];support::without_heap(||rt.render(&mut tail).unwrap());element_ramp_literal(&tail,1.,1.,0,64,32);
    // The existing absolute control-cell law is explicit: the accepted F32
    // value enters the next future point, without priming away any transient.
    support::without_heap(||rt.render(&mut tail).unwrap());element_ramp_literal(&tail,1.,0.,64,128,64);
    element_wire_cc(&mut rt,receiver,13,u32::MAX);let routed=rt.pending_external_turn().unwrap().unwrap();rt.begin_external_turn(routed.input).unwrap();
    assert_eq!(rt.post_external_output(routed.input,O::RoutedController{number:7,value:u32::MAX,channels:1}).unwrap(),R::Forwarded);
    rt.complete_external_segment(routed.input,None).unwrap();
    let manager=sampler_core::ChannelAddress{channel:0,..outside};assert_eq!(rt.channel_controller(performance,manager,7),Ok(u32::MAX));assert_eq!(rt.channel_controller(performance,outside,7),Ok(0));
    assert_eq!(rt.stats().nonfinite_frames,0);assert!(rt.note(note).unwrap().2);
}
#[test]
fn element_source_installation_rejects_incomplete_or_foreign_initialization_before_publication(){
    let plan=element_controller_plan(false);let mut limits=sampler_core::Limits::for_plan(&plan,8,16);limits.channels=48;
    let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();let owner=sampler_core::OwnedReceiver::new(&rt,0,0,sampler_core::ReceiverZone::Lower,1,8).unwrap();let receiver=rt.bind_receiver(owner).unwrap();let plan=rt.active_plan();
    let source=ir::SourceOwnerId::UviNode(0);
    for invalid in [vec![],vec![(ir::ModulatorRef(1),sampler_core::ReceiverValue::Bits32(u32::MAX))],vec![(ir::ModulatorRef(0),sampler_core::ReceiverValue::Bits7(1)),(ir::ModulatorRef(0),sampler_core::ReceiverValue::Bits7(2))]]{
        assert_eq!(rt.bind_receiver_source_input(receiver,plan,source,&invalid),Err(Error::InvalidInput));assert_eq!(rt.now(),0);assert_eq!(rt.active_plan(),plan);assert_eq!(rt.note_count(),0);
    }
    rt.bind_receiver_source_input(receiver,plan,source,&[(ir::ModulatorRef(0),sampler_core::ReceiverValue::Bits32(u32::MAX))]).unwrap();
    assert_eq!(rt.bind_receiver_source_input(receiver,plan,source,&[(ir::ModulatorRef(0),sampler_core::ReceiverValue::Bits7(0))]),Err(Error::Busy));
    rt.trigger(pan_input(803),60,1.).unwrap();let mut output=[[0.;2];32];support::without_heap(||rt.render(&mut output).unwrap());element_ramp_literal(&output,1.,1.,0,64,0);assert_eq!(rt.stats().nonfinite_frames,0);
}

#[test]
fn element_wire_seven_bit_point_is_native_raw_over127_not_upscaled_bank_fraction(){
    let(mut rt,receiver)=element_controller_runtime(false);rt.trigger(pan_input(804),60,1.).unwrap();
    rt.apply_receiver(receiver,Some(sampler_core::ReceiverChannelVoice{version:sampler_core::ReceiverVersion::Midi1,group:0,channel:13,
        message:sampler_core::ReceiverMessage::Control{index:7,value:sampler_core::ReceiverValue::Bits7(64)}}),false,None).unwrap();
    let mut output=[[0.;2];64];support::without_heap(||rt.render(&mut output).unwrap());
    element_ramp_literal(&output,1.,64./127.,0,64,0);
    let wrong=0.1*(1.+(0.5-1.)*63./64.);assert!((f64::from(output[63][0])-wrong).abs()>3e-4,"oracle must distinguish min-center-max channel-bank normalization");
    assert_eq!(rt.stats().nonfinite_frames,0);
}

#[test]
fn missing_element_binding_refuses_render_before_pcm_cursor_clock_or_due_publication(){
    let plan=element_controller_plan(false);let mut limits=sampler_core::Limits::for_plan(&plan,8,16);limits.channels=48;
    let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();let note=rt.trigger(pan_input(805),60,1.).unwrap();
    let mut output=[[0.75;2];17];assert_eq!(rt.render(&mut output),Err(Error::NotReady));assert_eq!(output,[[0.75;2];17]);assert_eq!(rt.now(),0);assert!(rt.note(note).unwrap().2);
    let mut seen=0;assert!(rt.visit_onset_voices(|voice|{assert_eq!(voice.cursor.traversal,0);seen+=1;true}));assert_eq!(seen,2);
    let state=sampler_core::OwnedReceiver::new(&rt,0,0,sampler_core::ReceiverZone::Lower,1,8).unwrap();let receiver=rt.bind_receiver(state).unwrap();
    rt.bind_receiver_source_input(receiver,rt.active_plan(),ir::SourceOwnerId::UviNode(0),&[(ir::ModulatorRef(0),sampler_core::ReceiverValue::Bits7(127))]).unwrap();
    support::without_heap(||rt.render(&mut output).unwrap());element_ramp_literal(&output,1.,1.,0,64,0);assert_eq!(rt.now(),17);assert_eq!(rt.stats().nonfinite_frames,0);
}

#[test]
fn authored_seven_bit_source_point_is_distinct_from_identical_core32_payload(){
    use sampler_core::ExternalOutput as O;
    let(mut seven,a)=element_controller_runtime(true);let(mut full,b)=element_controller_runtime(true);
    seven.trigger(pan_input(806),60,1.).unwrap();full.trigger(pan_input(806),60,1.).unwrap();
    // Both final channel-bank payloads are exactly identical. Their source
    // provenance is an explicit API fact rather than an equality heuristic.
    let value=sampler_core::ReceiverValue::Bits7(64).full_scale();assert_eq!(value,0x8000_0000);
    for(rt,id,output)in[(&mut seven,a,O::Controller7{number:7,value:64}),(&mut full,b,O::Controller{number:7,value})]{
        element_wire_cc(rt,id,13,value);let turn=rt.pending_external_turn().unwrap().unwrap();rt.begin_external_turn(turn.input).unwrap();
        support::without_heap(||{rt.post_external_output(turn.input,output).unwrap();rt.complete_external_segment(turn.input,None).unwrap();});
    }
    let mut raw=[[0.;2];64];let mut normalized=raw;
    support::without_heap(||{seven.render(&mut raw).unwrap();full.render(&mut normalized).unwrap();});
    element_ramp_literal(&raw,1.,64./127.,0,64,0);element_ramp_literal(&normalized,1.,f64::from(value)/f64::from(u32::MAX),0,64,0);
    assert!((raw[63][0]-normalized[63][0]).abs()>3e-4);
    for(rt,id)in[(&mut seven,a),(&mut full,b)]{
        let perf=rt.performance(0).unwrap();let outside=sampler_core::ChannelAddress{protocol:sampler_core::Protocol::Midi1,port:0,group:0,channel:13};
        assert_eq!(rt.channel_controller(perf,outside,7),Ok(0));
        // Explicit checked reroute changes the audience; same-route replacement
        // above remained element-only even though its Core32 payload was equal.
        element_wire_cc(rt,id,13,value);let turn=rt.pending_external_turn().unwrap().unwrap();rt.begin_external_turn(turn.input).unwrap();
        support::without_heap(||{rt.post_external_output(turn.input,O::RoutedController7{number:7,value:64,channels:1}).unwrap();rt.complete_external_segment(turn.input,None).unwrap();});
        assert_eq!(rt.channel_controller(perf,sampler_core::ChannelAddress{channel:0,..outside},7),Ok(value));
        assert_eq!(rt.channel_controller(perf,outside,7),Ok(0));assert_eq!(rt.stats().nonfinite_frames,0);
    }
}
#[test]
fn malformed_authored_seven_bit_edits_leave_running_turn_owner_and_source_unchanged(){
    use sampler_core::ExternalOutput as O;
    let(mut actual,a)=element_controller_runtime(true);let(mut reference,b)=element_controller_runtime(true);
    let note=actual.trigger(pan_input(807),60,1.).unwrap();reference.trigger(pan_input(807),60,1.).unwrap();
    element_wire_cc(&mut actual,a,13,0);element_wire_cc(&mut reference,b,13,0);
    let turn=actual.pending_external_turn().unwrap().unwrap();actual.begin_external_turn(turn.input).unwrap();
    let expr=actual.expression_id(note).unwrap();let before=(actual.now(),actual.note(note).unwrap(),actual.expression(expr).unwrap(),actual.active_plan());
    for output in[O::Controller7{number:128,value:64},O::Controller7{number:7,value:128},O::RoutedController7{number:7,value:64,channels:0},O::RoutedController7{number:7,value:64,channels:1<<13}]{
        support::without_heap(||assert_eq!(actual.post_external_output(turn.input,output),Err(Error::InvalidInput)));
        assert_eq!((actual.now(),actual.note(note).unwrap(),actual.expression(expr).unwrap(),actual.active_plan()),before);
        assert_eq!(actual.running_external_turn(turn.input).unwrap().input,turn.input);
    }
    actual.post_external_output(turn.input,O::Controller7{number:7,value:64}).unwrap();actual.complete_external_segment(turn.input,None).unwrap();
    let other=reference.pending_external_turn().unwrap().unwrap();reference.begin_external_turn(other.input).unwrap();reference.post_external_output(other.input,O::Controller7{number:7,value:64}).unwrap();reference.complete_external_segment(other.input,None).unwrap();
    let mut out=[[0.;2];65];let mut expected=out;support::without_heap(||{actual.render(&mut out).unwrap();reference.render(&mut expected).unwrap();});assert_eq!(out,expected);element_ramp_literal(&out,1.,64./127.,0,64,0);
}
#[test]
fn authored_seven_bit_mpe_timbre_keeps_real_member_gesture_precision(){
    use sampler_core::{ExternalOutput as O,ReceiverMessage as M,ReceiverValue as V};
    let(mut rt,receiver)=element_controller_runtime(true);
    let start=rt.apply_receiver(receiver,Some(sampler_core::ReceiverChannelVoice{version:sampler_core::ReceiverVersion::Midi1,group:0,channel:1,
        message:M::NoteOn{key:60,velocity:V::Bits7(100),attribute:Default::default()}}),false,None).unwrap();
    let sampler_core::ReceiverApplied::Started(note)=start else{panic!("expected real selected member note")};
    let expr=rt.expression_id(note).unwrap();assert_eq!(rt.expression(expr).unwrap().timbre,V::Bits7(64).full_scale());
    rt.apply_receiver(receiver,Some(sampler_core::ReceiverChannelVoice{version:sampler_core::ReceiverVersion::Midi1,group:0,channel:1,
        message:M::Control{index:74,value:V::Bits7(1)}}),false,None).unwrap();
    let turn=rt.pending_external_turn().unwrap().unwrap();rt.begin_external_turn(turn.input).unwrap();
    support::without_heap(||{rt.post_external_output(turn.input,O::Controller7{number:74,value:33}).unwrap();rt.complete_external_segment(turn.input,None).unwrap();});
    assert_eq!(rt.expression(expr).unwrap().timbre,V::Bits7(33).full_scale());
    assert!(rt.note(note).unwrap().2);assert_eq!(rt.stats().nonfinite_frames,0);
}

#[test]
fn spawned_actual_task_preserves_captured_element_only_audience_without_new_context_storage(){
    use sampler_core::{ExternalOutput as O,SourceTaskFamily as T};
    let plan=element_controller_plan(true).with_source_task_entries(vec![(ir::SourceEventProcessorId::UviNode(40),T::Spawned)],1).unwrap();
    let mut limits=sampler_core::Limits::for_plan(&plan,8,16);limits.channels=48;
    let mut rt=sampler_core::Runtime::new(plan,limits).unwrap();let receiver=rt.bind_receiver(sampler_core::OwnedReceiver::new(&rt,0,0,sampler_core::ReceiverZone::Lower,1,8).unwrap()).unwrap();
    rt.bind_receiver_source_input(receiver,rt.active_plan(),ir::SourceOwnerId::UviNode(0),&[(ir::ModulatorRef(0),sampler_core::ReceiverValue::Bits7(127))]).unwrap();
    let note=rt.trigger(pan_input(808),60,1.).unwrap();element_wire_cc(&mut rt,receiver,13,0);
    let parent=rt.pending_external_turn().unwrap().unwrap();rt.begin_external_turn(parent.input).unwrap();
    let child=rt.spawn_source_task(parent.input,false).unwrap();rt.complete_external_segment(parent.input,None).unwrap();rt.service_external_boundary().unwrap();
    let turn=rt.pending_external_turn().unwrap().unwrap();assert_eq!(turn.input.behavior(),child);rt.begin_external_turn(turn.input).unwrap();
    support::without_heap(||{rt.post_external_output(turn.input,O::Controller{number:7,value:0}).unwrap();rt.complete_external_segment(turn.input,None).unwrap();});
    let outside=sampler_core::ChannelAddress{protocol:sampler_core::Protocol::Midi1,port:0,group:0,channel:13};
    assert_eq!(rt.channel_controller(rt.performance(0).unwrap(),outside,7),Ok(0));assert!(rt.note(note).unwrap().2);
    let mut output=[[0.;2];64];support::without_heap(||rt.render(&mut output).unwrap());element_ramp_literal(&output,1.,0.,0,64,0);
    assert_eq!(rt.stats().nonfinite_frames,0);
}


//42g: actual definition properties use ControlState, never Note.mods/defaults.
fn constant_property_ir(value:f64)->ir::Instrument{
    let mut instrument=graph(true);
    instrument.controls=vec![
        ir::Control{key:"constant:value".into(),label:"Value".into(),value:ir::ControlValue::Continuous{min:f64::MIN,max:f64::MAX,default:value,unit:ir::ControlUnit::None},automation:ir::Automation::None},
        ir::Control{key:"constant:style".into(),label:"Style".into(),value:ir::ControlValue::Toggle{default:false},automation:ir::Automation::None},
    ];
    instrument.source_indices.constant_properties.push(ir::SourceConstantProperty{node:88,owner:ir::SourceOwnerId::UviNode(0),value:ir::ControlRef(0),style:ir::ControlRef(1),bipolar:false,law:ir::SourceConstantPropertyLaw::DeclaredCoreTargetRead});
    instrument.modulators.push(ir::Modulator{scope:ir::Scope::Voice,source:ir::ModulationSource::ConstantProperty{node:88,value:ir::ControlRef(0),style:ir::ControlRef(1),bipolar:false}});
    instrument.routes.push(ir::Route::new(ir::ModulatorRef(0),ir::Target::Amplitude,ir::Depth::Normalized(1.)));
    for zone in &mut instrument.zones{zone.routes.push(ir::RouteRef(0));}
    instrument
}
fn constant_property_pcm()->Vec<Pcm>{(0..2).map(|_|Pcm::new(48000,vec![[0.1;2];4096].into_boxed_slice()).unwrap()).collect()}
fn constant_property_plan(value:f64)->sampler_core::Prepared{
    lower(&constant_property_ir(value),48000,constant_property_pcm(),|_,plan|Ok(plan)).unwrap()
}
fn constant_property_partition_render(runtime:&mut sampler_core::Runtime,output:&mut [[f32;2]],parts:&[usize]){
    let mut offset=0;for &frames in parts{let end=offset+frames;runtime.render(&mut output[offset..end]).unwrap();offset=end;}assert_eq!(offset,output.len());
}
#[test]
fn constant_definition_initial_target_matches_independent_static_gain_and_future_notes(){
    use sampler_core::{Runtime,Limits,ControlValue,SourceConstantField as Field};
    let plan=constant_property_plan(0.25);let limits=Limits::for_plan(&plan,8,8);
    let mut actual=Runtime::new(plan,limits).unwrap();let plan=actual.active_plan();
    let mut reference_ir=graph(true);for zone in &mut reference_ir.zones{zone.gain=ir::Gain::Linear(0.25);}
    let reference_plan=lower(&reference_ir,48000,constant_property_pcm(),|_,plan|Ok(plan)).unwrap();let reference_limits=Limits::for_plan(&reference_plan,8,8);let mut reference=Runtime::new(reference_plan,reference_limits).unwrap();
    assert_eq!(actual.source_constant_property(plan,88).unwrap().value,0.25);
    let split_plan=constant_property_plan(0.25);let split_limits=Limits::for_plan(&split_plan,8,8);let mut split=Runtime::new(split_plan,split_limits).unwrap();let split_plan=split.active_plan();
    let input=pan_input(42_001);let a=actual.trigger(input,60,1.).unwrap();let b=reference.trigger(input,60,1.).unwrap();let split_note=split.trigger(input,60,1.).unwrap();let mut output=[[0.;2];64];let mut expected=output;let mut split_output=output;
    support::without_heap(||{actual.render(&mut output).unwrap();reference.render(&mut expected).unwrap();});
    for (a,b)in output.iter().zip(expected){for c in 0..2{assert!((a[c]-b[c]).abs()<1e-7);}}
    support::without_heap(||constant_property_partition_render(&mut split,&mut split_output,&[1,17,31,15]));assert_eq!(output,split_output);
    let now=actual.now();let revision=actual.source_constant_property(plan,88).unwrap().revision;
    support::without_heap(||{assert!(actual.set_source_constant_property(plan,88,Field::Value,ControlValue::Real(f64::NAN)).is_err());});
    assert_eq!((actual.now(),actual.source_constant_property(plan,88).unwrap().revision),(now,revision));
    support::without_heap(||{actual.set_source_constant_property(plan,88,Field::Value,ControlValue::Real(1.)).unwrap();});
    split.set_source_constant_property(split_plan,88,Field::Value,ControlValue::Real(1.)).unwrap();
    // Ordinary per-voice mix consumes the END of each sample: established
    // envelope_attenuation_ramps_exactly_under_any_partition_without_heap law.
    // This is distinct from the global ControlRamp absolute-time outlet path.
    actual.render(&mut output).unwrap();
    support::without_heap(||constant_property_partition_render(&mut split,&mut split_output,&[1,17,31,15]));assert_eq!(output,split_output);
    for (index,frame)in output.iter().enumerate(){for c in 0..2{let expected=f64::from(expected[0][c])*(1.+3.*(index+1) as f64/64.);assert!((f64::from(frame[c])-expected).abs()<1e-7);}}
    for c in 0..2{assert!((f64::from(output[0][c])-f64::from(expected[0][c])*(1.+3./64.)).abs()<1e-7,"first physical sample consumes one step");assert!((f64::from(output[63][c])-f64::from(expected[0][c])*4.).abs()<1e-7,"64th physical sample reaches target");}
    split.release(split_note).unwrap();split.render(&mut split_output).unwrap();split.flush_ended(|_|true);
    actual.render(&mut output).unwrap();
    actual.release(a).unwrap();reference.release(b).unwrap();actual.render(&mut output).unwrap();reference.render(&mut expected).unwrap();actual.flush_ended(|_|true);reference.flush_ended(|_|true);
    let fresh=actual.trigger(pan_input(42_002),60,1.).unwrap();actual.render(&mut output).unwrap();assert!(output.iter().flatten().any(|v|*v>0.01));
    assert_eq!(actual.source_constant_property(plan,88).unwrap().value,1.);actual.release(fresh).unwrap();
}
#[test]
fn constant_property_batch_invalid_late_field_preserves_raw_target_style_revision_and_pcm(){
    use sampler_core::{Runtime,Limits,ControlValue,ControlWrite,SourceConstantField as Field};
    let prepared=constant_property_plan(0.25);let limits=Limits::for_plan(&prepared,4,4);let mut runtime=Runtime::new(prepared,limits).unwrap();let plan=runtime.active_plan();
    runtime.trigger(pan_input(42_003),60,1.).unwrap();let mut before=[[0.;2];32];runtime.render(&mut before).unwrap();
    let old=runtime.source_constant_property(plan,88).unwrap();let binding=runtime.source_constant_properties(plan).unwrap().next().unwrap();
    let mut writes=[ControlWrite{id:binding.value,value:ControlValue::Real(0.9)},ControlWrite{id:binding.style,value:ControlValue::Real(1.)}];writes.sort_by_key(|write|write.id);
    support::without_heap(||assert!(runtime.edit_controls(plan,None,&writes).is_err()));
    let now=runtime.now();assert_eq!(runtime.source_constant_property(plan,88).unwrap(),sampler_core::SourceConstantSnapshot{frame:now,..old});
    let mut after=[[0.;2];32];runtime.render(&mut after).unwrap();assert_eq!(before,after);
    assert!(runtime.set_source_constant_property(plan,88,Field::Style,ControlValue::Integer(1)).is_err());
    runtime.set_source_constant_property(plan,88,Field::Style,ControlValue::Toggle(true)).unwrap();
    runtime.set_source_constant_property(plan,88,Field::Value,ControlValue::Real(0.5)).unwrap();let mut trajectory=[[0.;2];96];
    let split_prepared=constant_property_plan(0.25);let split_limits=Limits::for_plan(&split_prepared,4,4);let mut split=Runtime::new(split_prepared,split_limits).unwrap();let split_plan=split.active_plan();let split_note=split.trigger(pan_input(42_003),60,1.).unwrap();let mut split_prefix=[[0.;2];64];
    support::without_heap(||constant_property_partition_render(&mut split,&mut split_prefix,&[1,17,31,15]));for frame in split_prefix{assert_eq!(frame,before[0]);}
    split.set_source_constant_property(split_plan,88,Field::Style,ControlValue::Toggle(true)).unwrap();split.set_source_constant_property(split_plan,88,Field::Value,ControlValue::Real(0.5)).unwrap();let mut split_trajectory=trajectory;
    for chunk in trajectory.chunks_mut(32){runtime.render(chunk).unwrap();}
    support::without_heap(||constant_property_partition_render(&mut split,&mut split_trajectory,&[1,17,31,15,1,31]));assert_eq!(trajectory,split_trajectory);
    for (index,frame)in trajectory.iter().enumerate(){for c in 0..2{let expected=f64::from(before[0][c])*(1.-(index+1).min(64) as f64/64.);assert!((f64::from(frame[c])-expected).abs()<1e-7,"declared Core64 route ramp at{index}");}}
    for c in 0..2{assert!((f64::from(trajectory[0][c])-f64::from(before[0][c])*63./64.).abs()<1e-7,"first physical sample consumes one step");}assert_eq!(trajectory[63],[0.;2],"64th physical sample reaches exact zero");
    split.release(split_note).unwrap();split.render(&mut split_prefix).unwrap();split.flush_ended(|_|true);
    assert_eq!(&trajectory[64..],&[[0.;2];32],"Style1 tie is declared-Core0; source remains held");assert_eq!(runtime.voice_count(),2);
    runtime.set_source_constant_property(plan,88,Field::Value,ControlValue::Real(7.)).unwrap();assert_eq!(runtime.source_constant_property(plan,88).unwrap().value,7.,"raw getter is finite authored Value; output clamps separately");
}
#[test]
fn constant_role_schema_remap_missing_domain_and_capacity_are_prepublication_refusals(){
    let prepared=constant_property_plan(0.25);let definitions=prepared.controls().to_vec();let properties=prepared.source_constant_properties().collect::<Vec<_>>();
    assert!(prepared.source_constant_storage_bytes().unwrap()>0);
    assert!(matches!(prepared.with_source_constant_properties(properties,0),Err(Error::Capacity)));
    let mut reversed=definitions.clone();reversed.reverse();assert!(constant_property_plan(0.25).with_controls(reversed).is_ok());
    assert!(constant_property_plan(0.25).with_controls(definitions.iter().copied().take(1).collect()).is_err());
    let mut wrong=definitions;let style=constant_property_plan(0.25).source_constant_properties().next().unwrap().style;
    let field=wrong.iter_mut().find(|field|field.id==style).unwrap();field.domain=sampler_core::ControlDomain::Real{min:0.,max:1.};field.default=sampler_core::ControlValue::Real(0.);
    assert!(constant_property_plan(0.25).with_controls(wrong).is_err());
}
#[test]
fn constant_old_generation_binding_never_retargets_or_prevents_actual_retirement(){
    use sampler_core::{Runtime,Limits,ControlValue,SourceConstantField as Field};
    let prepared=constant_property_plan(0.25);let limits=Limits::for_plan(&prepared,8,8);let(mut runtime,mut control)=Runtime::with_plan_updates(prepared,limits,3,2).unwrap();let old=runtime.active_plan();let note=runtime.trigger(pan_input(42_004),60,1.).unwrap();
    let request=control.submit(Box::new(constant_property_plan(0.75))).unwrap();assert_eq!(runtime.poll_plan_update(),Ok(Some(request)));let new=runtime.active_plan();assert_ne!(old,new);
    runtime.set_source_constant_property(old,88,Field::Value,ControlValue::Real(0.5)).unwrap();assert_eq!(runtime.source_constant_property(new,88).unwrap().value,0.75);
    runtime.release(note).unwrap();let mut output=[[0.;2];128];runtime.render(&mut output).unwrap();runtime.flush_ended(|_|true);assert_eq!(runtime.collect_retired_plans(),1);
    assert_eq!(runtime.source_constant_property(old,88),Err(Error::StaleHandle));assert_eq!(runtime.set_source_constant_property(old,88,Field::Value,ControlValue::Real(1.)),Err(Error::StaleHandle));assert_eq!(runtime.source_constant_property(new,88).unwrap().value,0.75);
    runtime.set_source_constant_property(new,88,Field::Value,ControlValue::Real(0.625)).unwrap();let snapshot=runtime.source_constant_property(new,88).unwrap();let performance=runtime.performance(0).unwrap();
    runtime.reset_protocol_controller_targets(performance,sampler_core::ChannelScope{protocol:sampler_core::Protocol::Midi1,port:0,group:0,channels:1},None).unwrap();assert_eq!(runtime.source_constant_property(new,88).unwrap(),snapshot);
    runtime.panic();assert_eq!(runtime.source_constant_property(new,88).unwrap().value,0.625);
}

#[test]
fn actual_property_only_global_output_starts_without_fictitious_receiver_and_keeps_core_cell_order(){
    use sampler_core::{Runtime,Limits,ControlValue,SourceConstantField as Field};
    let mut instrument=explicit_output_instrument(false,true);let value=ir::ControlRef(instrument.controls.len());let style=ir::ControlRef(value.0+1);
    instrument.controls.push(ir::Control{key:"global-constant:value".into(),label:"actual definition Value".into(),value:ir::ControlValue::Continuous{min:f64::MIN,max:f64::MAX,default:0.25,unit:ir::ControlUnit::None},automation:ir::Automation::None});instrument.controls.push(ir::Control{key:"global-constant:style".into(),label:"actual definition Style".into(),value:ir::ControlValue::Toggle{default:false},automation:ir::Automation::None});
    instrument.source_indices.constant_properties.push(ir::SourceConstantProperty{node:89,owner:ir::SourceOwnerId::UviNode(0),value,style,bipolar:false,law:ir::SourceConstantPropertyLaw::DeclaredCoreTargetRead});
    instrument.modulators.push(ir::Modulator{scope:ir::Scope::Master,source:ir::ModulationSource::ConstantProperty{node:89,value,style,bipolar:false}});instrument.routes.push(ir::Route::new(ir::ModulatorRef(0),ir::Target::Amplitude,ir::Depth::Normalized(1.)));
    instrument.source_indices.global_modulation.push(ir::SourceGlobalModulation{owner:ir::SourceOwnerId::UviNode(0),input_owner:ir::SourceOwnerId::UviNode(0),outlet:ir::BusRef(0),sources:vec![ir::ModulatorRef(0)],routes:vec![ir::RouteRef(0)]});
    let prepared=lower(&instrument,48000,constant_property_pcm(),|_,plan|Ok(plan)).unwrap();assert!(prepared.source_controller_inputs().next().is_none());let limits=Limits::for_plan(&prepared,4,8);let mut rt=Runtime::new(prepared,limits).unwrap();let plan=rt.active_plan();rt.trigger(pan_input(42_005),60,1.).unwrap();let mut first=[[0.;2];32];support::without_heap(||rt.render(&mut first).unwrap());
    for frame in first{for sample in frame{assert!((sample-0.025).abs()<3e-7);}}
    support::without_heap(||{rt.set_source_constant_property(plan,89,Field::Value,ControlValue::Real(0.75)).unwrap();});
    let mut tail=[[0.;2];96];support::without_heap(||{for part in tail.chunks_mut(32){rt.render(part).unwrap();}});
    for (index,frame)in tail.iter().enumerate(){let expected=if index<32{0.025}else{0.025+0.05*(index-32)as f64/64.};for sample in frame{assert!((f64::from(*sample)-expected).abs()<3e-7,"Core cell frame{index}");}}
    assert_eq!(rt.source_constant_property(plan,89).unwrap().value,0.75);assert_eq!(rt.stats().nonfinite_frames,0);assert_eq!(rt.voice_count(),2);
}

#[test]
fn malformed_constant_definition_and_unqualified_native_law_never_reach_behavior_publication(){
    for malformed in 0..4{
        let mut instrument=constant_property_ir(0.25);assert!(instrument.validate().is_ok());
        match malformed{
            0=>instrument.source_indices.constant_properties[0].owner=ir::SourceOwnerId::UviNode(999),
            1=>{let row=instrument.source_indices.constant_properties[0].clone();instrument.source_indices.constant_properties.push(row);},
            2=>if let ir::ModulationSource::ConstantProperty{bipolar,..}=&mut instrument.modulators[0].source{*bipolar=true;},
            _=>instrument.controls[1].value=ir::ControlValue::Continuous{min:0.,max:1.,default:0.,unit:ir::ControlUnit::None},
        }
        assert!(instrument.validate().is_err());let bound=std::cell::Cell::new(false);assert!(lower(&instrument,48000,constant_property_pcm(),|_,plan|{bound.set(true);Ok(plan)}).is_err());assert!(!bound.get());
    }
    let mut instrument=constant_property_ir(0.25);instrument.source_indices.constant_properties[0].law=ir::SourceConstantPropertyLaw::UnqualifiedNative;assert!(instrument.validate().is_ok());instrument.behaviors.push(ir::Behavior{name:"not-run".into(),language:ir::Language::Ksp,source:String::new(),slot:None,state:vec![],requires:vec![]});let bound=std::cell::Cell::new(false);
    let error=lower(&instrument,48000,constant_property_pcm(),|_,plan|{bound.set(true);Ok(plan)}).err().unwrap();assert!(error.to_string().contains("declared-Core target reader"));assert!(!bound.get());
}
