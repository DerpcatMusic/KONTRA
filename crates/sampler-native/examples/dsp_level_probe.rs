//! Metadata-only level/state probe. Run through kontakto-heavy; no payload or PCM is written.
use sampler_core::{EngineParameterAddress, Limits, Runtime, engine_parameter_id};
use sampler_ir::Processor;
use sampler_midi::{Ingress, Packets, TimedPacket, Version};
use std::path::Path;

// Strip comments and text in memory; retain only public parameter names and counts.
fn setter_sites(source: &str) -> serde_json::Value {
    let mut code = String::with_capacity(source.len());
    let (mut comment, mut quoted) = (0usize, false);
    for c in source.chars() {
        if !quoted && c == '{' {
            comment += 1;
        }
        if comment > 0 {
            if c == '}' {
                comment -= 1;
            }
            code.push(' ');
        } else if c == '"' {
            quoted = !quoted;
            code.push(' ');
        } else {
            code.push(if quoted { ' ' } else { c });
        }
    }
    let mut targets = std::collections::BTreeMap::<String, usize>::new();
    let (mut total, mut dynamic, mut physical_slot) = (0, 0, 0);
    for (at, _) in code.match_indices("set_engine_par") {
        if at > 0
            && (code.as_bytes()[at - 1].is_ascii_alphanumeric() || code.as_bytes()[at - 1] == b'_')
        {
            continue;
        }
        let rest = code[at + "set_engine_par".len()..].trim_start();
        if !rest.starts_with('(') {
            continue;
        }
        let mut depth = 0;
        let mut start = 1;
        let mut args = Vec::new();
        for (i, c) in rest.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        args.push(rest[start..i].trim());
                        break;
                    }
                }
                ',' if depth == 1 => {
                    args.push(rest[start..i].trim());
                    start = i + 1;
                }
                _ => {}
            }
        }
        if args.len() != 5 {
            continue;
        }
        total += 1;
        if let Some(id) = engine_parameter_id(args[0]) {
            let name = sampler_core::engine_parameter_name(id).unwrap().to_owned();
            *targets.entry(name).or_default() += 1;
        } else {
            dynamic += 1;
        }
        if args[2] == "-1" && args[3] == "1" && matches!(args[4], "1" | "$NI_INSERT_BUS") {
            physical_slot += 1;
        }
    }
    serde_json::json!({"set_engine_par_sites":total,"public_targets":targets,
        "dynamic_parameter_sites":dynamic,"literal_instrument_insert_slot_1_sites":physical_slot})
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        if let Some(location) = info.location() {
            eprintln!("probe failure at authored code line {}", location.line());
        }
    }));
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("NKI path");
    let mode = args.next();
    if mode.as_deref() == Some("--layers") {
        let loaded = sampler_kontakt::load(
            Path::new(&path),
            &sampler_kontakt::Options {
                keys: 60..=60,
                library: Some(Path::new(&path).into()),
                ..Default::default()
            },
            |_| {},
        )
        .expect("load");
        let ir = &loaded.instrument;
        let candidates:Vec<_>=ir.zones.iter().enumerate().filter(|(_,z)|z.keys.low<=60 && z.keys.high>=60 && z.velocities.low<=100 && z.velocities.high>=100).map(|(i,z)| {
            let routes:Vec<_>=z.routes.iter().filter_map(|r| {
                let r=&ir.routes[r.0];
                let sampler_ir::ModulationSource::Controller(cc)=ir.modulators[r.source.0].source else {return None};
                Some(serde_json::json!({"controller":cc,"target":format!("{:?}",r.target),"depth":format!("{:?}",r.depth),"invert":r.invert,"shape":r.shape.map(|s|s.0),"smoothing":format!("{:?}",r.smoothing)}))
            }).collect();
            serde_json::json!({"zone_index":i,"group":z.group.map(|g|g.0),"gain":z.gain.linear(),"velocity":format!("{:?}",z.velocity),"fades":format!("{:?}",z.fades),"routes":routes})
        }).collect();
        let widgets:Vec<_>=loaded.scripts.iter().flat_map(|s|s.model().interface.widgets.iter()).filter_map(|w| {
            let name=w.name.to_ascii_lowercase();
            let category=if name.contains("volume") {"volume"}else if name.contains("velocity"){"velocity"}else if name.contains("cross")||name.contains("xfade"){"crossfade"}else if name.contains("dynamic"){"dynamics"}else if name.contains("cc")||name.contains("modwheel"){"controller"}else{return None};
            let sampler_ksp::model::WidgetValue::Int(value)=w.value else{return None};
            Some(serde_json::json!({"ui_id":w.ui_id,"category":category,"layer":if name.contains("l1"){Some(1)}else if name.contains("l2"){Some(2)}else{None},"value":value,"persistent":format!("{:?}",w.persistence)}))
        }).collect();
        let omitted:Vec<_>=ir.unsupported.iter().filter_map(|u| {
            let group=u.location.strip_prefix("group ")?.split_whitespace().next()?.parse::<usize>().ok()?;
            if !candidates.iter().any(|c|c["group"].as_u64()==Some(group as u64)) {return None;}
            let numeric:Vec<f64>=u.value.split_whitespace().filter_map(|v|v.parse().ok()).collect();
            Some(serde_json::json!({"group":group,"slot":u.location.rsplit_once("slot ").and_then(|(_,s)|s.split_whitespace().next()).and_then(|n|n.trim_end_matches(':').parse::<usize>().ok()),"feature":u.feature,"target":if u.value.starts_with("filterCutoff"){Some("cutoff")}else{None},"numeric_values":numeric,"reason":format!("{:?}",u.reason)}))
        }).collect();
        let shapes: Vec<_> = ir
            .shapes
            .iter()
            .enumerate()
            .map(|(i, s)| serde_json::json!({"index":i,"debug":format!("{:?}",s)}))
            .collect();
        let init_writes:Vec<_>=loaded.scripts.iter().flat_map(|view|view.model().requests.iter().filter(|r|r.command=="set_engine_par").filter_map(move |r| {
            let name=match r.args.first()? {sampler_ksp::model::Value::Text(n)=>n.clone(),sampler_ksp::model::Value::Int(v)=>view.symbol(*v)?,_=>return None};
            let id=engine_parameter_id(&name)?;
            let ints:Option<Vec<_>>=r.args[1..].iter().map(|v|match v {sampler_ksp::model::Value::Int(v)=>Some(*v),_=>None}).collect();let ints=ints?;
            if ints.len()!=4 {return None;}
            Some(serde_json::json!({"target":sampler_core::engine_parameter_name(id),"value":ints[0],"group":ints[1],"slot":ints[2],"generic":ints[3]}))
        })).collect();
        let modulator_sources:Vec<_>=ir.source_indices.modulators.iter().filter(|m|candidates.iter().any(|c|c["group"].as_u64()==Some(m.group as u64))).map(|m| {
            serde_json::json!({"group":m.group,"slot":m.slot,"external":m.external,"source":m.runtime.and_then(|r|ir.modulators.get(r.0)).map(|m|format!("{:?}",m.source))})
        }).collect();
        let host_volume = ir
            .host_volume
            .map(|v| serde_json::json!({"controller":v.controller,"saved_gain":v.saved}));
        let group_count = ir.groups.len();
        let plan = loaded.plan;
        let rate = plan.sample_rate();
        let limits = Limits {
            notes: 64,
            channels: 16,
            performances: 1,
            expressions: 64,
            families: 64,
            decisions: 256,
            voices: 512,
            commands: 256,
            behaviors: 16,
            behavior_fuel: 1 << 20,
            behavior_cells: plan.behavior_local_count() * 16,
            note_cells: plan.note_cell_count() * 64,
        };
        let mut rt = Runtime::new(plan, limits).unwrap();
        let mut versions = [None; 16];
        versions[0] = Some(Version::Midi1);
        let mut ingress = Ingress::new(0, versions);
        let mut states = Vec::new();
        for cc in [0u8, 64, 127] {
            while rt.take_engine_parameter_outcome().is_some() {}
            let word = [0x20b00100 | u32::from(cc)];
            let packet = TimedPacket {
                offset: 0,
                packet: Packets::new(&word).next().unwrap().unwrap(),
            };
            ingress
                .render(&mut rt, &mut [[0.; 2]; 64], &[packet], 1, |_, r| {
                    assert!(r.is_ok())
                })
                .unwrap();
            rt.flush_behaviors(|_, _, _| true);
            for _ in 0..rate / 128 {
                ingress
                    .render(&mut rt, &mut [[0.; 2]; 64], &[], 0, |_, r| {
                        assert!(r.is_ok())
                    })
                    .unwrap();
                rt.flush_behaviors(|_, _, _| true);
            }
            let mut writes = Vec::new();
            while let Some(event) = rt.take_engine_parameter_outcome() {
                if let Some(a) = event.address {
                    if event.write {
                        writes.push(serde_json::json!({"target":sampler_core::engine_parameter_name(a.parameter),"group":a.group,"slot":a.slot,"generic":a.generic,"succeeded":event.result.is_ok(),"value":rt.engine_parameter(a).ok()}));
                    }
                }
            }
            let groups: Vec<_> = (0..group_count)
                .filter_map(|g| {
                    let a = EngineParameterAddress {
                        parameter: engine_parameter_id("ENGINE_PAR_VOLUME").unwrap(),
                        group: g as i32,
                        slot: -1,
                        generic: -1,
                    };
                    rt.engine_parameter(a)
                        .ok()
                        .map(|value| serde_json::json!({"group":g,"volume":value}))
                })
                .collect();
            let volumes:Vec<_>=init_writes.iter().filter(|w|w["target"].as_str().is_some_and(|t|t.ends_with("ENGINE_PAR_VOLUME"))).map(|w| {
                let a=EngineParameterAddress {parameter:engine_parameter_id("ENGINE_PAR_VOLUME").unwrap(),group:w["group"].as_i64().unwrap() as i32,slot:w["slot"].as_i64().unwrap() as i32,generic:w["generic"].as_i64().unwrap() as i32};
                serde_json::json!({"group":a.group,"slot":a.slot,"generic":a.generic,"readback":rt.engine_parameter(a).ok()})
            }).collect();
            states.push(serde_json::json!({"cc1":cc,"group_values":groups,"volume_readbacks":volumes,"writes":writes}));
        }
        println!("{}",serde_json::to_string_pretty(&serde_json::json!({"rate":rate,"key":60,"velocity":100,"candidates":candidates,"omitted_active_groups":omitted,"modulator_sources":modulator_sources,"host_volume":host_volume,"initial_engine_writes":init_writes,"shapes":shapes,"widgets":widgets,"settle_seconds":0.5,"states":states})).unwrap());
        return;
    }
    if mode.as_deref() == Some("--intent") {
        let mut kontakt = sampler_kontakt::read(Path::new(&path)).expect("read");
        let (scripts, _, _) = sampler_kontakt::compile_ui(
            &mut kontakt.instrument,
            &sampler_kontakt::Options {
                library: Some(Path::new(&path).into()),
                ..Default::default()
            },
        );
        let results: Vec<_> = scripts.iter().map(|script| {
            let widgets: Vec<_> = script.model().interface.widgets.iter()
                .filter(|w| w.name.to_ascii_lowercase().contains("compressor"))
                .filter_map(|w| match w.value { sampler_ksp::model::WidgetValue::Int(value) =>
                    Some(serde_json::json!({"ui_id":w.ui_id,"kind":format!("{:?}",w.kind),
                        "value":value,"persistent":format!("{:?}",w.persistence)})), _ => None }).collect();
            let writes: Vec<_> = script.model().requests.iter().filter(|r| r.command == "set_engine_par")
                .filter_map(|r| {
                    let name = match r.args.first()? {
                        sampler_ksp::model::Value::Text(name) => name.clone(),
                        sampler_ksp::model::Value::Int(value) => script.view().symbol(*value)?,
                        _ => return None,
                    };
                    let id = engine_parameter_id(&name)?;
                    let ints: Option<Vec<_>> = r.args[1..].iter().map(|v| match v {
                        sampler_ksp::model::Value::Int(n) => Some(*n), _ => None }).collect();
                    let ints = ints?;
                    if ints.len() != 4 || ints[1..] != [-1,1,1] { return None; }
                    Some(serde_json::json!({"target":sampler_core::engine_parameter_name(id),
                        "value":ints[0],"group":ints[1],"slot":ints[2],"generic":ints[3]}))
                }).collect();
            let sites = kontakt.instrument.behaviors.iter().find(|b| b.slot == Some(script.view().slot()))
                .map(|b| setter_sites(&b.source));
            serde_json::json!({"script_slot":script.view().slot(),"sites":sites,"persistence":format!("{:?}",script.model().persistence_completion),
                "compressor_widgets":widgets,"insert_slot_1_init_writes":writes})
        }).collect();
        println!("{}", serde_json::to_string_pretty(&results).unwrap());
        return;
    }
    let static_address = mode.map(|group| sampler_ir::SlotAddress {
        group: group.parse().unwrap(),
        slot: args.next().unwrap().parse().unwrap(),
        generic: args.next().unwrap().parse().unwrap(),
    });
    let mut results = Vec::new();
    for (bypass, unity_output) in [(false, false), (true, false), (false, true)] {
        let mut kontakt = sampler_kontakt::read(Path::new(&path)).expect("read");
        let sites: Vec<_> = kontakt.instrument.behaviors.iter().enumerate()
            .map(|(slot, behavior)| serde_json::json!({"behavior_index":slot,"script_slot":behavior.slot,"sites":setter_sites(&behavior.source)}))
            .collect();
        if let Some(address) = static_address {
            // Explicit metadata-verified physical address: expose a static
            // compressor's existing output trim through the production Mix
            // lowering so the probe can bypass both, without changing defaults.
            let mut found = 0;
            for chain in &mut kontakt.instrument.chains {
                for stages in [&mut chain.pre_amplitude, &mut chain.post_amplitude] {
                    if let Some(i) = stages
                        .iter()
                        .position(|p| matches!(p, Processor::Compressor(_)))
                    {
                        let Processor::StereoMatrix(m) = stages[i + 1] else {
                            panic!("no output trim")
                        };
                        assert!(m[0][1] == 0.0 && m[1][0] == 0.0 && m[0][0] == m[1][1]);
                        stages.remove(i + 1);
                        stages.insert(
                            i,
                            Processor::Mix {
                                count: 1,
                                address,
                                dry: 0.0,
                                wet: m[0][0],
                                bypass: false,
                            },
                        );
                        found += 1;
                    }
                }
            }
            assert_eq!(found, 1);
        }
        let loaded = sampler_kontakt::load_read(
            kontakt,
            &sampler_kontakt::Options {
                keys: 60..=60,
                library: Some(Path::new(&path).into()),
                ..Default::default()
            },
            |_| {},
            || false,
        )
        .expect("load");
        let mut addresses = Vec::new();
        for chain in &loaded.instrument.chains {
            for stages in [&chain.pre_amplitude, &chain.post_amplitude] {
                for (i, stage) in stages.iter().enumerate() {
                    if let Processor::Mix { count, address, .. } = stage {
                        if stages[i + 1..i + 1 + usize::from(*count)]
                            .iter()
                            .any(|p| matches!(p, Processor::Compressor(_)))
                        {
                            addresses.push(EngineParameterAddress {
                                parameter: engine_parameter_id("ENGINE_PAR_EFFECT_BYPASS").unwrap(),
                                group: address.group,
                                slot: address.slot,
                                generic: address.generic,
                            });
                        }
                    }
                }
            }
        }
        addresses.sort();
        addresses.dedup();
        if addresses.is_empty() {
            let static_compressors = loaded
                .instrument
                .chains
                .iter()
                .flat_map(|c| c.pre_amplitude.iter().chain(&c.post_amplitude))
                .filter(|p| matches!(p, Processor::Compressor(_)))
                .count();
            results.push(serde_json::json!({"status":"no-addressed-compressors",
                "static_compressors":static_compressors}));
            break;
        }
        let plan = loaded.plan;
        let rate = plan.sample_rate();
        let limits = Limits {
            notes: 64,
            channels: 16,
            performances: 1,
            expressions: 64,
            families: 64,
            decisions: 256,
            voices: 512,
            commands: 256,
            behaviors: 16,
            behavior_fuel: 1 << 20,
            behavior_cells: plan.behavior_local_count().saturating_mul(16),
            note_cells: plan.note_cell_count().saturating_mul(64),
        };
        let mut runtime = Runtime::new(plan, limits).unwrap();
        let mut groups = [None; 16];
        groups[0] = Some(Version::Midi1);
        let mut ingress = Ingress::new(0, groups);
        let words = [[0x20b00164], [0x20b00b7f], [0x20903c40]];
        let packets: Vec<_> = words
            .iter()
            .map(|w| TimedPacket {
                offset: 0,
                packet: Packets::new(w).next().unwrap().unwrap(),
            })
            .collect();
        let mut power = 0.0;
        let mut frames = 0;
        let mut initial = Vec::new();
        let mut slot_outcomes = Vec::new();
        let mut buffer = [[0.0; 2]; 64];
        for at in (0..rate as usize / 2).step_by(buffer.len()) {
            if at != 0 && (bypass || unity_output) {
                for &address in &addresses {
                    if bypass {
                        runtime.set_engine_parameter(address, 1).unwrap();
                    } else {
                        runtime
                            .set_engine_parameter(
                                EngineParameterAddress {
                                    parameter: engine_parameter_id(
                                        "ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN",
                                    )
                                    .unwrap(),
                                    ..address
                                },
                                396_851,
                            )
                            .unwrap();
                    }
                }
            }
            ingress
                .render(
                    &mut runtime,
                    &mut buffer,
                    if at == 0 { &packets } else { &[] },
                    packets.len(),
                    |_, result| {
                        assert!(result.is_ok());
                    },
                )
                .unwrap();
            if at == 0 {
                initial = addresses.iter().map(|&a| serde_json::json!({
                    "group":a.group,"slot":a.slot,"generic":a.generic,
                    "bypass":runtime.engine_parameter(a).unwrap(),
                    "output_gain":runtime.engine_parameter(EngineParameterAddress {
                        parameter: engine_parameter_id("ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN").unwrap(), ..a
                    }).ok(),
                })).collect();
            }
            for (i, sample) in buffer.iter().enumerate() {
                if at + i >= rate as usize / 10 && at + i < rate as usize * 45 / 100 {
                    assert!(sample.iter().all(|x| x.is_finite()));
                    power += sample.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>() / 2.0;
                    frames += 1;
                }
            }
            runtime.flush_behaviors(|_, _, _| true);
            runtime.flush_ended(|_| true);
            while let Some(event) = runtime.take_engine_parameter_outcome() {
                if let Some(address) = event.address
                    && address.group == -1
                    && address.slot == 1
                    && address.generic == 1
                {
                    slot_outcomes.push(serde_json::json!({"write":event.write,"succeeded":event.result.is_ok(),
                        "target":sampler_core::engine_parameter_name(address.parameter),"group":address.group,
                        "slot":address.slot,"generic":address.generic,"readback":runtime.engine_parameter(address).ok()}));
                }
            }
        }
        let rms = (power / frames as f64).sqrt();
        assert!(rms > 1e-8);
        results.push(
            serde_json::json!({"forced_compressor_bypass":bypass,"forced_unity_output":unity_output,"initial_compressors":initial,
            "script_sites":sites,"slot_outcomes":slot_outcomes,
            "rate":rate,"key":60,"velocity":64,"cc1":100,"cc11":127,"frames":frames,"rms":rms}),
        );
    }
    println!("{}", serde_json::to_string_pretty(&results).unwrap());
}

#[cfg(test)]
mod tests {
    #[test]
    fn setter_inventory_excludes_comments_text_and_nested_argument_commas() {
        let sites = super::setter_sites(
            r#"{ set_engine_par($ENGINE_PAR_PAN, 1, -1, 1, 1) }
            set_text($x, "set_engine_par($ENGINE_PAR_PAN, 1, -1, 1, 1)")
            _set_engine_par($ENGINE_PAR_PAN, 1, -1, 1, 1)
            set_engine_par ($ENGINE_PAR_EFFECT_BYPASS, f(1, 2), -1, 1, $NI_INSERT_BUS)
            set_engine_par($dynamic, 2, 0, 1, -1)"#,
        );
        assert_eq!(sites["set_engine_par_sites"], 2);
        assert_eq!(sites["dynamic_parameter_sites"], 1);
        assert_eq!(sites["literal_instrument_insert_slot_1_sites"], 1);
        assert_eq!(sites["public_targets"]["$ENGINE_PAR_EFFECT_BYPASS"], 1);
    }
}
