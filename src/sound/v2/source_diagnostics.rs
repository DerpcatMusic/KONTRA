//! Opt-in off-audio observations of committed homes and actual occupied voices.
use super::V2Core;
use serde_json::{Value, json};

const WT_CONTROL_LIMIT: usize = 512;

// Prepared::with_engine_parameters orders the immutable slice by physical
// address, whose first field is parameter. Query exact WT ranges instead of
// truncating unrelated volume/filter bindings preceding them.
fn wavetable_bindings(
    bindings: &[sampler_core::EngineParameterBinding],
) -> (Vec<&sampler_core::EngineParameterBinding>, usize) {
    let mut selected = Vec::new();
    let mut total = 0;
    for (parameter, name) in sampler_core::ENGINE_PARAMETER_NAMES.iter().enumerate() {
        if !name.trim_start_matches('$').starts_with("ENGINE_PAR_WT_") {
            continue;
        }
        let Ok(parameter) = u16::try_from(parameter) else {
            continue;
        };
        let start = bindings.partition_point(|b| b.address.parameter < parameter);
        let end = bindings.partition_point(|b| b.address.parameter <= parameter);
        total += end - start;
        selected.extend(
            bindings[start..end]
                .iter()
                .take(WT_CONTROL_LIMIT - selected.len()),
        );
    }
    (selected, total)
}

impl V2Core {
    /// Off-audio pure actual target/voice/control reads. No worker or source service.
    pub(crate) fn diagnostic_live_graph_runtime(
        &self, slot: usize, epoch: u64, players: &[u32], nodes: &[u32],
        mixes: &[(i32, i32, i32)],
    ) -> Result<Value, &'static str> {
        fn unique(ids: &[u32], limit: usize) -> bool {
            ids.len() <= limit && ids.iter().enumerate().all(|(i,n)| !ids[..i].contains(n))
        }
        if !unique(players,3) || players.is_empty() || !unique(nodes,128) || mixes.len()>32
            || mixes.iter().enumerate().any(|(i,m)| mixes[..i].contains(m)
                || u32::try_from(m.2).ok().is_none_or(|n| !nodes.contains(&n))) {
            return Err("live_graph_query_bounds");
        }
        let part=self.parts.get(slot).and_then(Option::as_ref).ok_or("live_graph_part_unavailable")?;
        if part.epoch!=epoch { return Err("live_graph_stale_epoch"); }
        let at_before=part.runtime.now();
        let plan=part.runtime.active_plan();
        let bindings=part.runtime.engine_parameter_bindings(plan).map_err(|_|"live_graph_stale_plan")?;
        if bindings.len()>262144 { return Err("live_graph_binding_scan_bound"); }
        let mut targets=Vec::new();
        for &node in players {
            let row=match part.runtime.sample_target_asset(sampler_core::SampleTargetId(node)) {
                Ok(pcm)=> {
                    let(regions,count)=part.waveform_sources.asset_regions(pcm.asset_id(),64);
                    json!({"player_node":node,"status":"observed","current_pcm_revision_asset_id":pcm.asset_id(),
                        "source_rate_hz":pcm.sample_rate(),"frames":pcm.frame_count(),
                        "matching_regions":regions,"matching_region_count":count,"region_matches_complete":count<=64})
                },
                Err(e)=>json!({"player_node":node,"status":"unavailable","error":format!("{e:?}")}),
            };
            targets.push(row);
        }
        let mut voices=Vec::new();
        let complete=part.runtime.visit_onset_voices(|voice| {
            if voices.len()==8 { return false; }
            let(regions,count)=part.waveform_sources.asset_regions(voice.asset,64);
            voices.push(json!({"voice":voice,"gain_state":part.runtime.diagnostic_voice_gain(voice.voice),
                "matching_regions":regions,"matching_region_count":count,"region_matches_complete":count<=64}));
            true
        });
        let control=|id: sampler_core::ControlId| {
            fn value(v:Result<sampler_core::ControlValue,sampler_core::Error>)->Value {
                match v {
                    Ok(sampler_core::ControlValue::Integer(v))=>json!({"type":"integer","value":v}),
                    Ok(sampler_core::ControlValue::Real(v))=>json!({"type":"real","value":v,"bits":v.to_bits()}),
                    Ok(sampler_core::ControlValue::Toggle(v))=>json!({"type":"toggle","value":v}),
                    Err(e)=>json!({"status":"unavailable","error":format!("{e:?}")}),
                }
            }
            let control_id=id.0; // ControlId is packed; copy before borrowing for formatting.
            json!({"control_id":control_id.to_string(),
                "base":value(part.runtime.control_base_value(plan,id)),
                "current_target":value(part.runtime.control_value(plan,id)),
                "scope":"Existing base/offset control targets, not instantaneous ramped DSP"})
        };
        let mut controls=Vec::new();
        for b in bindings {
            if u32::try_from(b.address.generic).ok().is_some_and(|n|nodes.contains(&n)) {
                if controls.len()==512 { return Err("live_graph_selected_binding_bound"); }
                controls.push(json!({"parameter":b.address.parameter,"group":b.address.group,
                    "slot":b.address.slot,"generic":b.address.generic,"law":b.law,"state":control(b.control)}));
            }
        }
        let bypass:Vec<_>=mixes.iter().map(|&(group,slot,generic)|json!({"group":group,"slot":slot,"generic":generic,
            "state":control(sampler_core::slot_control(sampler_core::SlotKind::Bypass,group,slot,generic))})).collect();
        Ok(json!({"epoch":epoch,"at":part.runtime.now(),"at_before":at_before,"at_after":part.runtime.now(),"plan_token":format!("{plan:?}"),"targets":targets,
            "voices":voices,"voices_complete":complete,"prepared_controls":controls,"mix_bypass_controls":bypass,
            "scope":"Pure actual committed Pcm and generation-fenced occupied voices. Static region matches may not resolve dynamic provenance. No IO, source read, render, service or time advance."}))
    }

    pub(crate) fn diagnostic_group_gains(&self,slot:usize,epoch:u64,groups:&[u32]) -> Result<Value,&'static str> {
        if groups.len()>8 || groups.iter().enumerate().any(|(i,g)|*g>65535 || groups[..i].contains(g)) {
            return Err("Gain observation group limit exceeded");
        }
        let part=self.parts.get(slot).and_then(Option::as_ref).ok_or("Gain owner unavailable")?;
        if part.epoch!=epoch { return Err("Gain observation owner is stale"); }
        let host=self.mix.parts.get(slot).map(|p|json!({"gain":p.gain,"pan":p.pan,
            "mute":p.mute,"solo":p.solo,"output":p.output,"aux":p.aux,"aux_gain":p.aux_gain}));
        let buses:Vec<_>=self.mix.buses.iter().enumerate().map(|(index,b)|json!({"bus":index,
            "gain":b.gain,"pan":b.pan,"mute":b.mute,"solo":b.solo,"port":b.port})).collect();
        Ok(json!({"at":part.runtime.now(),"epoch":epoch,"host_part":host,"host_buses":buses,
            "groups":groups.iter().map(|&g|json!({"group":g,"state":part.runtime.diagnostic_group_gain(g)})).collect::<Vec<_>>(),
            "scope":"Pure read of current committed compiled group indices, not native group identities; absent group/fader remains null"}))
    }
    pub(crate) fn diagnostic_sources(
        &self,
        slot: usize,
        epoch: u64,
        ids: &[u32],
    ) -> Result<Value, &'static str> {
        if ids.len() > 32 {
            return Err("Source observation ID limit exceeded");
        }
        let part = self
            .parts
            .get(slot)
            .and_then(Option::as_ref)
            .ok_or("Source owner unavailable")?;
        if part.epoch != epoch {
            return Err("Source observation owner is stale");
        }
        let plan = part.runtime.active_plan();
        let homes: Vec<_> = ids
            .iter()
            .map(|&id| {
                let home: Result<Vec<_>, _> = [
                    sampler_core::ZoneParameter::Group,
                    sampler_core::ZoneParameter::LowKey,
                    sampler_core::ZoneParameter::HighKey,
                ]
                .map(|parameter| part.runtime.zone_parameter_in(plan, id, parameter))
                .into_iter()
                .collect();
                match home {
                    Ok(home) => json!({"native_id":id,"status":"observed","home":home}),
                    Err(error) => {
                        json!({"native_id":id,"status":"unavailable","error":format!("{error:?}")})
                    }
                }
            })
            .collect();
        let mut voices = Vec::new();
        let mut gain_rows=0usize;
        let complete = part.runtime.visit_onset_voices(|voice| {
            if voices.len() == 256 {
                return false;
            }
            let (regions, count) = part.waveform_sources.asset_regions(voice.asset, 64);
            let gain_state=if gain_rows<32 {
                gain_rows+=1;
                part.runtime.diagnostic_voice_gain(voice.voice)
            } else { None };
            voices.push(
                json!({"voice":voice,"gain_state":gain_state,"matching_regions":regions,"matching_region_count":count,
                "region_matches_complete":count<=64}),
            );
            true
        });
        let routing = part.runtime.zone_routing_report(plan).map_err(|_| "Source plan is stale")?
            .map(|r| json!({"zones":r.zones,"templates":r.templates,"job_capacity":r.job_capacity,"pending_jobs":r.pending_jobs,
                "refusals":r.refusals,"last_refusal":r.last_refusal.map(|e|format!("{e:?}")),
                "unavailable_destinations":r.unavailable_destinations,"catalog_only_initial_homes":r.catalog_only_initial_homes,
                "partial_initial_homes":r.partial_initial_homes,"partial_initial_keys":r.partial_initial_keys}));
        let bindings = part
            .runtime
            .engine_parameter_bindings(plan)
            .map_err(|_| "Source plan is stale")?;
        let (selected, wt_total) = wavetable_bindings(bindings);
        let wt_controls: Vec<_> = selected
            .into_iter()
            .filter_map(|binding| {
                let name = sampler_core::engine_parameter_name(binding.address.parameter)?;
                if !name.trim_start_matches('$').starts_with("ENGINE_PAR_WT_") {
                    return None;
                }
                let value = match part.runtime.control_base_value(plan, binding.control) {
                    Ok(sampler_core::ControlValue::Integer(v)) => {
                        json!({"type":"integer","value":v})
                    }
                    Ok(sampler_core::ControlValue::Real(v)) => {
                        json!({"type":"real","value":v,"bits":v.to_bits()})
                    }
                    Ok(sampler_core::ControlValue::Toggle(v)) => json!({"type":"toggle","value":v}),
                    Err(error) => json!({"error":format!("{error:?}")}),
                };
                Some(
                    json!({"name":name,"group":binding.address.group,"slot":binding.address.slot,
                "generic":binding.address.generic,"base":value,"law":binding.law}),
                )
            })
            .collect();
        Ok(
            json!({"epoch":epoch,"native_homes":homes,"voices":voices,"voices_complete":complete,
            "gain_readback_limit":32,"gain_readbacks_omitted":voices.len().saturating_sub(gain_rows),
            "routing":routing,"wavetable_controls":wt_controls,"engine_binding_scan_truncated":false,
            "engine_binding_count":bindings.len(),"wavetable_control_count":wt_total,
            "wavetable_control_limit_reached":wt_total>WT_CONTROL_LIMIT,
            "wavetable_controls_omitted":wt_total.saturating_sub(WT_CONTROL_LIMIT),
            "engine_binding_query":"Exact parameter ranges in the immutable prepared address ordering; no prefix scan",
            "scope":"Read-only current committed native homes and occupied voices; voice.zone is legacy selection identity, not native ID. Asset-to-prepared-region matches can be ambiguous, and absent matches stay explicit. No engine service, IO or time advancement."}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::{
        Core,
        tree::MixTree,
        v2::Part,
        waveform::{Source, Sources},
    };
    use sampler_core::{
        Envelope, Input, Limits, Pcm, Playback, Prepared, Protocol, Region, Runtime,
        SourceZoneHome, SourceZoneIdentity,
    };

    #[test]
    fn wavetable_query_finds_controls_beyond_unrelated_binding_prefix() {
        use sampler_core::{
            ControlId, EngineParameterAddress, EngineParameterBinding, EngineParameterLaw,
        };
        let other = sampler_core::engine_parameter_id("ENGINE_PAR_VOLUME").unwrap();
        let wt = sampler_core::engine_parameter_id("ENGINE_PAR_WT_POSITION").unwrap();
        assert!(other < wt);
        let make = |parameter, group| EngineParameterBinding {
            address: EngineParameterAddress {
                parameter,
                group,
                slot: -1,
                generic: -1,
            },
            control: ControlId(group as u128),
            law: EngineParameterLaw::Linear { low: 0., high: 1. },
        };
        let mut bindings: Vec<_> = (0..6000).map(|group| make(other, group)).collect();
        bindings.extend((0..3).map(|group| make(wt, group)));
        bindings.sort_by_key(|b| b.address);
        let (found, count) = wavetable_bindings(&bindings);
        assert_eq!(count, 3);
        assert_eq!(
            found.iter().map(|b| b.address.group).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        bindings.extend((3..513).map(|group| make(wt, group)));
        bindings.sort_by_key(|b| b.address);
        let (found, count) = wavetable_bindings(&bindings);
        assert_eq!(count, 513);
        assert_eq!(found.len(), WT_CONTROL_LIMIT);
        assert_eq!(found.last().unwrap().address.group, 511);
    }

    #[test]
    fn source_observer_fences_epoch_and_joins_real_voice_asset_without_native_id_aliasing() {
        let pcm = Pcm::new(48000, vec![[0.5; 2]; 4096].into()).unwrap();
        let prepared = Prepared::new(
            48000,
            vec![pcm.clone()],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: None,
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Envelope::default(),
                playback: Playback::default(),
            }],
            1,
        )
        .unwrap()
        .with_source_zone_identities(
            vec![SourceZoneIdentity {
                id: Some(0),
                home: SourceZoneHome {
                    group: 42,
                    low_key: 0,
                    high_key: 127,
                },
                region: None,
            }],
            1,
            4096,
        )
        .unwrap();
        let limits = Limits::for_plan(&prepared, 4, 4);
        let mut runtime = Runtime::new(prepared, limits).unwrap();
        runtime
            .trigger(
                Input {
                    protocol: Protocol::Clap,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 60,
                    external_id: Some(1),
                },
                60,
                1.,
            )
            .unwrap();
        runtime.render(&mut [[0.; 2]; 1]).unwrap();
        let mut part = Part::new(runtime, MixTree::instrument("source witness")).unwrap();
        part.epoch = 8;
        let mut sources = Sources::default();
        sources.insert_region(19, Some(0), Source { pcm, stream: None });
        part.waveform_sources = sources;
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, Some(Box::new(part)));
        assert!(core.diagnostic_sources(0, 7, &[0]).is_err());
        assert!(core.diagnostic_sources(0, 8, &[0; 33]).is_err());
        let first = core.diagnostic_sources(0, 8, &[0, 19]).unwrap();
        assert_eq!(first["native_homes"][0]["home"], json!([42, 0, 127]));
        assert_eq!(first["native_homes"][1]["status"], "unavailable");
        assert_eq!(first["voices"][0]["matching_regions"], json!([19]));
        assert_eq!(first["voices_complete"], true);
        assert_eq!(first, core.diagnostic_sources(0, 8, &[0, 19]).unwrap());
    }
    #[test]
    fn live_graph_read_is_pure_and_separates_current_revision_from_old_voice() {
        use sampler_core::{SampleTarget,SampleTargetId,SampleRevisionLimits,PreparedSampleRevision,SampleTask};
        let old=Pcm::new(48000,vec![[0.4;2];4096].into()).unwrap();
        let new=Pcm::new(48000,vec![[0.7;2];4096].into()).unwrap();
        let old_id=old.asset_id();let new_id=new.asset_id();
        let prepared=Prepared::new(48000,vec![old.clone()],vec![Region{
            sample:0,key_low:60,key_high:60,root_key:Some(60),velocity_low:0.,velocity_high:1.,gain:1.,
            envelope:Envelope::default(),playback:Playback::default()}],1).unwrap()
            .with_sample_targets(vec![SampleTarget{id:SampleTargetId(9019),regions:vec![0].into()}],
                SampleRevisionLimits{slots:4,resident_bytes:1<<20}).unwrap();
        let limits=Limits::for_plan(&prepared,4,4);
        let(mut runtime,mut producer)=Runtime::new(prepared,limits).unwrap().with_sample_updates(2).unwrap();
        runtime.trigger(Input{protocol:Protocol::Clap,port:0,group:0,channel:0,key:60,external_id:Some(1)},60,1.).unwrap();
        runtime.render(&mut [[0.;2];1]).unwrap();
        producer.submit(PreparedSampleRevision::new(SampleTargetId(9019),new,Playback::default(),48000).unwrap(),
            std::sync::Arc::new(SampleTask::default())).unwrap();
        runtime.poll_sample_update().unwrap();assert!(producer.reply().unwrap().result.is_ok());
        let mut part=Part::new(runtime,MixTree::instrument("numeric fixture")).unwrap();part.epoch=8;
        let mut sources=Sources::default();sources.insert_region(0,None,Source{pcm:old,stream:None});part.waveform_sources=sources;
        let mut core=V2Core::with_parts(1,48000.);core.install(0,Some(Box::new(part)));
        assert!(core.diagnostic_live_graph_runtime(0,7,&[9019],&[],&[]).is_err());
        assert!(core.diagnostic_live_graph_runtime(0,8,&[9019;4],&[],&[]).is_err());
        assert!(core.diagnostic_live_graph_runtime(0,8,&[9019],&[8985],&[(-1,-1,9003)]).is_err());
        let a=core.diagnostic_live_graph_runtime(0,8,&[9019],&[8985],&[]).unwrap();
        assert_eq!(a["targets"][0]["current_pcm_revision_asset_id"],json!(new_id));
        assert_eq!(a["targets"][0]["matching_region_count"],0);
        assert_eq!(a["voices"][0]["voice"]["asset"],json!(old_id));
        assert_eq!(a["voices"][0]["matching_regions"],json!([0]));
        assert_eq!(a,core.diagnostic_live_graph_runtime(0,8,&[9019],&[8985],&[]).unwrap());
    }

}
