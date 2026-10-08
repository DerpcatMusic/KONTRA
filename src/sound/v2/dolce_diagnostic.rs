//! One privacy-safe frozen-checkpoint witness using the production host seam.
use super::*;
use serde_json::json;

#[test]
#[ignore = "requires an explicit instrument path and fresh report directory"]
fn frozen_dolce_signal_graph() {
    let path = std::env::var_os("KONTRA_DOLCE_PATH").expect("instrument path required");
    let root = std::path::PathBuf::from(
        std::env::var_os("KONTRA_REPORT_DIR").expect("report directory required"),
    );
    let key: u8 = std::env::var("KONTRA_DOLCE_KEY")
        .unwrap_or_else(|_| "60".into())
        .parse()
        .unwrap();
    let request = LoadRequest {
        path: path.into(),
        sample_rate: 48000.,
        dynamics_start: Some(100),
        threads: None,
        ..Default::default()
    };
    let loaded = match V2Loader.prepare(&request, &mut |_| {}, &|| false) {
        Ok(loaded) => loaded,
        Err(_) => panic!("production load failed; authored diagnostics omitted"),
    };
    let instrument = loaded.instrument.as_ref().expect("Kontakt IR required");
    let candidates: Vec<_> = instrument
        .zones
        .iter()
        .enumerate()
        .filter(|(_, z)| {
            (z.keys.low..=z.keys.high).contains(&key)
                && (z.velocities.low..=z.velocities.high).contains(&64)
        })
        .map(|(index, z)| {
            json!({"zone":index,"group":z.group.map(|g|g.0),"asset":z.asset.0,
            "gain":z.gain.linear(),"source_start":z.playback.start})
        })
        .collect();
    let routing: Vec<_> = loaded
        .tree
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            json!({
                "node":index,"parent":node.parent,"kind":format!("{:?}",node.kind),
                "sends":node.sends,"insert_count":node.inserts.len(),
            })
        })
        .collect();
    let stream = loaded.stream.clone().expect("streamed loader required");
    let head_frames_before: usize = stream.assets.iter().map(Pcm::head_frames).sum();
    let group_count = instrument.groups.len();
    let zone_count = instrument.zones.len();
    let authored_envelopes: Vec<_> = instrument
        .source_indices
        .modulators
        .iter()
        .filter_map(|source| {
            if source.external {
                return None;
            }
            let ir::ModulationSource::Envelope(e) =
                &instrument.modulators[source.runtime?.0].source
            else {
                return None;
            };
            Some((source.group, source.slot, *e))
        })
        .collect();
    let modulations: Vec<_> = instrument.zones.iter().enumerate().filter(|(_,z)| (z.keys.low..=z.keys.high).contains(&key)).map(|(index,z)| {
        let routes:Vec<_> = z.routes.iter().map(|r| {
            let r=&instrument.routes[r.0];
            json!({"source":format!("{:?}",instrument.modulators[r.source.0].source),"target":format!("{:?}",r.target),"depth":format!("{:?}",r.depth),"invert":r.invert,"shape":r.shape.map(|s|s.0)})
        }).collect();
        json!({"zone":index,"amplitude":z.amplitude.map(|m|format!("{:?}",instrument.modulators[m.0].source)),"routes":routes})
    }).collect();
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(0, loaded.part);
    let runtime = &mut core.parts[0].as_mut().unwrap().runtime;
    runtime.record_selections(true);
    let group_values = |runtime: &mut Runtime| -> Vec<_> {
        (0..group_count)
            .map(|group| {
                let parameters: Vec<_> = ["ENGINE_PAR_VOLUME", "ENGINE_PAR_ATTACK", "ENGINE_PAR_HOLD", "ENGINE_PAR_DECAY", "ENGINE_PAR_SUSTAIN", "ENGINE_PAR_RELEASE"].into_iter().flat_map(|name| {
                    let slots = if name == "ENGINE_PAR_VOLUME" { vec![-1] } else { vec![0,1] };
                    slots.into_iter().map(|slot| {
                        let address = sampler_core::EngineParameterAddress {parameter:sampler_core::engine_parameter_id(name).unwrap(), group:group as i32,slot,generic:-1};
                        json!({"parameter":name,"slot":slot,"value":runtime.engine_parameter(address).ok()})
                    }).collect::<Vec<_>>()
                }).collect();
                json!({"group":group,"parameters":parameters})
            })
            .collect()
    };
    let initial_groups = group_values(runtime);
    let restore = std::env::var_os("KONTRA_DOLCE_RESTORE_SUSTAIN").is_some();
    if restore {
        for group in 0..group_count {
            for slot in 0..2 {
                let address = sampler_core::EngineParameterAddress {
                    parameter: sampler_core::engine_parameter_id("ENGINE_PAR_SUSTAIN").unwrap(),
                    group: group as i32,
                    slot,
                    generic: -1,
                };
                if runtime.engine_parameter(address).is_ok() {
                    runtime.set_engine_parameter(address, 1_000_000).unwrap();
                }
            }
        }
    }
    let restore_envelope = std::env::var_os("KONTRA_DOLCE_RESTORE_ENVELOPE").is_some();
    if restore_envelope {
        for (group, slot, e) in &authored_envelopes {
            for (name, value) in [
                ("ENGINE_PAR_ATTACK", e.attack.seconds() * 48000.),
                ("ENGINE_PAR_HOLD", e.hold.seconds() * 48000.),
                ("ENGINE_PAR_DECAY", e.decay.seconds() * 48000.),
                ("ENGINE_PAR_SUSTAIN", e.sustain),
                ("ENGINE_PAR_RELEASE", e.release.seconds() * 48000.),
            ] {
                let address = sampler_core::EngineParameterAddress {
                    parameter: sampler_core::engine_parameter_id(name).unwrap(),
                    group: *group as i32,
                    slot: *slot as i32,
                    generic: -1,
                };
                let law = if name == "ENGINE_PAR_SUSTAIN" {
                    sampler_core::EngineParameterLaw::CubicGain { unity: 1_000_000. }
                } else {
                    sampler_core::EngineParameterLaw::ShiftedExponential {
                        low: 96.,
                        high: if name == "ENGINE_PAR_ATTACK" {
                            720096.
                        } else {
                            1200096.
                        },
                        offset: 96.,
                    }
                };
                if runtime.engine_parameter(address).is_ok() {
                    runtime
                        .set_engine_parameter(address, law.encode(value))
                        .unwrap();
                }
            }
        }
    }
    core.event(0, Event::midi1(0xb0, 1, 100));
    core.event(0, Event::midi1(0xb0, 11, 127));
    core.event(0, Event::midi1(0x90, key, 64));
    let mut blocks = Vec::new();
    let mut writes = Vec::new();
    for block in 0..180 {
        std::thread::sleep(std::time::Duration::from_millis(3));
        let audio = core.render(128);
        let mut peak = 0.0f64;
        let mut energy = 0.0;
        for bus in audio.buses.iter() {
            for channel in bus {
                for &x in &channel[..128] {
                    peak = peak.max(f64::from(x.abs()));
                    energy += f64::from(x).powi(2);
                }
            }
        }
        let part = core.parts[0].as_mut().unwrap();
        let stats = part.runtime.stats();
        blocks.push(json!({"at":block*128,"peak":peak,"energy":energy,
            "voices":stats.voices,"cold_starts":stats.cold_starts,"refused_starts":stats.refused_starts,
            "voice_drops":stats.voice_drops,"underruns":stats.stream_underruns}));
        while let Some(event) = part.runtime.take_engine_parameter_outcome() {
            if let Some(address) = event.address {
                if event.write {
                    writes.push(json!({"at":block*128,"parameter":sampler_core::engine_parameter_name(address.parameter),
                        "group":address.group,"slot":address.slot,"generic":address.generic,
                        "ok":event.result.is_ok(),"value":part.runtime.engine_parameter(address).ok()}));
                }
            }
        }
    }
    let part = core.parts[0].as_mut().unwrap();
    let selections: Vec<_> = part.runtime.take_selection_records().into_iter().map(|selection| json!({
        "at":selection.at,"key":selection.key,"velocity":selection.velocity,"suppressed":selection.suppressed,
        "candidates":selection.candidates.into_iter().map(|c|json!({"region":c.region,"group":c.group,
            "rejected":c.rejected.map(|r|format!("{:?}",r))})).collect::<Vec<_>>(),
    })).collect();
    let final_groups = group_values(&mut part.runtime);
    let decoded: Vec<_> = candidates.iter().filter_map(|candidate| {
        let index = candidate["asset"].as_u64()? as usize;
        let pcm = stream.assets.get(index)?;
        let source = stream.streamer.source(pcm.asset_id())?;
        let mut reader = source.open().ok()?;
        let mut samples = vec![[0.;2];reader.frames().min(16384)];
        let ok = reader.read(0, &mut samples).is_ok();
        let peak = samples.iter().flatten().fold(0.0f32,|peak,x|peak.max(x.abs()));
        Some(json!({"asset":index,"frames":reader.frames(),"rate":reader.rate(),
            "head_frames":pcm.head_frames(),"decode_ok":ok,"first_frames_probed":samples.len(),"peak":peak}))
    }).collect();
    let summary = json!({"product_base":"9993db691a5f69d31980357694a678e358785e5e",
        "observation_overlay":true,"key":key,"velocity":64,"cc1":100,"cc11":127,"keyswitch":null,
        "audio_frames":180*128,"block_frames":128,"wall_sleep_ms":3,"zone_count":zone_count,
        "group_count":group_count,"head_frames_before":head_frames_before,"candidates":candidates,"restored_sustain":restore,"restored_envelope":restore_envelope,
        "routing":routing,"initial_groups":initial_groups,"final_groups":final_groups,"modulation":modulations,
        "blocks":blocks,"selections":selections,"engine_writes":writes,"decoded_candidates":decoded});
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("witness.json"),
        serde_json::to_vec_pretty(&summary).unwrap(),
    )
    .unwrap();
    drop(core);
    drop(stream);
    assert!(sampler_core::trace_report::flush(
        std::time::Duration::from_secs(10)
    ));
}
