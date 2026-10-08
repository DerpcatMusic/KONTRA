//! Metadata-only level/state probe. Run through kontakto-heavy; no payload or PCM is written.
use sampler_core::{EngineParameterAddress, Limits, Runtime, engine_parameter_id};
use sampler_ir::Processor;
use sampler_midi::{Ingress, Packets, TimedPacket, Version};
use std::path::Path;

fn main() {
    std::panic::set_hook(Box::new(|info| {
        if let Some(location) = info.location() {
            eprintln!("probe failure at authored code line {}", location.line());
        }
    }));
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("NKI path");
    let static_address = args.next().map(|group| sampler_ir::SlotAddress {
        group: group.parse().unwrap(), slot: args.next().unwrap().parse().unwrap(),
        generic: args.next().unwrap().parse().unwrap(),
    });
    let mut results = Vec::new();
    for bypass in [false, true] {
        let mut kontakt = sampler_kontakt::read(Path::new(&path)).expect("read");
        if let Some(address) = static_address {
            // Explicit metadata-verified physical address: expose a static
            // compressor's existing output trim through the production Mix
            // lowering so the probe can bypass both, without changing defaults.
            let mut found = 0;
            for chain in &mut kontakt.instrument.chains {
                for stages in [&mut chain.pre_amplitude, &mut chain.post_amplitude] {
                    if let Some(i) = stages.iter().position(|p| matches!(p, Processor::Compressor(_))) {
                        let Processor::StereoMatrix(m) = stages[i + 1] else { panic!("no output trim") };
                        assert!(m[0][1] == 0.0 && m[1][0] == 0.0 && m[0][0] == m[1][1]);
                        stages.remove(i + 1);
                        stages.insert(i, Processor::Mix { count: 1, address,
                            dry: 0.0, wet: m[0][0], bypass: false });
                        found += 1;
                    }
                }
            }
            assert_eq!(found, 1);
        }
        let loaded = sampler_kontakt::load_read(
            kontakt, &sampler_kontakt::Options {
                keys: 60..=60,
                library: Some(Path::new(&path).into()),
                ..Default::default()
            },
            |_| {}, || false,
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
            let static_compressors = loaded.instrument.chains.iter()
                .flat_map(|c| c.pre_amplitude.iter().chain(&c.post_amplitude))
                .filter(|p| matches!(p, Processor::Compressor(_))).count();
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
        let mut buffer = [[0.0; 2]; 64];
        for at in (0..rate as usize / 2).step_by(buffer.len()) {
            if at != 0 && bypass {
                for &address in &addresses {
                    runtime.set_engine_parameter(address, 1).unwrap();
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
        }
        let rms = (power / frames as f64).sqrt();
        assert!(rms > 1e-8);
        results.push(
            serde_json::json!({"forced_compressor_bypass":bypass,"initial_compressors":initial,
            "rate":rate,"key":60,"velocity":64,"cc1":100,"cc11":127,"frames":frames,"rms":rms}),
        );
    }
    println!("{}", serde_json::to_string_pretty(&results).unwrap());
}
