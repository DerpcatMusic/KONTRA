//! Structural/early-admission proof for source-native recipes. No native data.
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
