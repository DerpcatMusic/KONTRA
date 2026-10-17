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
    instrument.source_indices.processing.push(ir::SourceProcessingRecipe{name:"authored keygroup".into(),owner:ir::SourceOwnerId::UviNode(1),lifetime:ir::SourceProcessingLifetime::UviKeygroupActivation,members:vec![ir::ZoneRef(0),ir::ZoneRef(1)],chain:None,amplitude:None,output:None,compiled:compiled.then_some(0)});
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
