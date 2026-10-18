//! Common source-owner hierarchy through ordinary authored NKI loading.
//! Native Kontakt Group voice chains are not UVI keygroup shared-sum instances.
use sampler_ir as ir;
#[path="support/zone_routing.rs"]
pub mod support;
fn row(instrument:&ir::Instrument,id:ir::SourceOwnerId)->&ir::SourceOwner{
    instrument.source_indices.owners.iter().find(|owner|owner.id==id).unwrap()
}
#[test]fn normal_kontakt_uses_physical_source_ordinals_and_keeps_route_variants_distinct(){
    let fixture=support::fixture();let _=(&fixture.a,&fixture.b,&fixture.directory);
    let kontakt=sampler_kontakt::read(&fixture.base).unwrap();let instrument=&kontakt.instrument;
    assert!(instrument.validate().is_ok());let root=row(instrument,ir::SourceOwnerId::KontaktInstrument);
    assert_eq!(root.children,vec![ir::SourceOwnerId::KontaktGroup(0),ir::SourceOwnerId::KontaktGroup(1)]);
    assert_eq!(root.event_processors.len(),1);assert_eq!(root.event_processors[0].id,ir::SourceEventProcessorId::KontaktScriptSlot(0));assert_eq!(root.event_processors[0].owner,root.id);assert_eq!(root.event_processors[0].ordinal,0);
    assert_eq!(instrument.source_indices.zone_catalog[1].id,Some(7));
    let zone=row(instrument,ir::SourceOwnerId::KontaktZone(1));assert_eq!(zone.parent,Some(ir::SourceOwnerId::KontaktGroup(instrument.source_indices.zone_catalog[1].home.group)));
    assert!(!instrument.source_indices.owners.iter().any(|owner|owner.id==ir::SourceOwnerId::KontaktZone(7)),"native ZID7 must not alias physical table ordinal1");
    let expected:Vec<_>=instrument.zone_routing.iter().filter(|routing|routing.source_zone==7).flat_map(|routing|routing.variants.iter().filter_map(|variant|variant.zone)).collect();assert!(expected.iter().all(|region|zone.zones.contains(region)));
    assert!(instrument.source_indices.processing.is_empty());assert!(instrument.processing_scopes.is_empty(),"Kontakt Group retains its existing native per-voice processor owner");
    let static_path=support::static_base(fixture.directory.path(),0);let plain=sampler_kontakt::read(&static_path).unwrap();assert!(row(&plain.instrument,ir::SourceOwnerId::KontaktInstrument).event_processors.is_empty());assert!(plain.instrument.processing_scopes.is_empty());
}
#[test]fn missing_sample_does_not_erase_native_source_owner_or_create_a_region_alias(){
    let fixture=support::fixture();let _=(&fixture.a,&fixture.b,&fixture.directory);std::fs::remove_file(fixture.directory.path().join("seven.wav")).unwrap();
    let kontakt=sampler_kontakt::read(&fixture.base).unwrap();let instrument=&kontakt.instrument;assert!(instrument.validate().is_ok());assert_eq!(instrument.source_indices.zone_catalog[1].id,Some(7));assert_eq!(instrument.source_indices.zones[1],None);let owner=row(instrument,ir::SourceOwnerId::KontaktZone(1));assert!(owner.zones.is_empty());assert!(owner.predicates.iter().any(|predicate|matches!(predicate,ir::SourceInputPredicate::Unsupported{field} if field=="kontakt-zone-not-compiled")));assert_eq!(owner.parent,Some(ir::SourceOwnerId::KontaktGroup(instrument.source_indices.zone_catalog[1].home.group)));
}
