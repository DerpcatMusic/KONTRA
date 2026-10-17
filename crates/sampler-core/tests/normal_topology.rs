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
