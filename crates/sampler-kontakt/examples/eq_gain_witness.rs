//! Isolate one authored gate-item zone; decoded PCM and both renders stay in RAM.
use anyhow::{Context, Result, ensure};
use sampler_core::{Input, Limits, Pcm, Protocol, Runtime};
use sampler_ir as ir;
use std::path::Path;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod heap;

fn main() -> Result<()> {
    let path = std::env::args().nth(1).context("NKI path")?;
    let key: u8 = std::env::args().nth(2).unwrap_or("60".into()).parse()?;
    let mut library = sampler_kontakt::read(Path::new(&path))?;
    let is_eq = |r: &ir::Route| match r.target {
        ir::Target::Processor {
            chain,
            index,
            parameter: ir::ProcessorParameter::Gain,
        } => library.instrument.chains[chain.0]
            .pre_amplitude
            .iter()
            .chain(&library.instrument.chains[chain.0].post_amplitude)
            .nth(index)
            .is_some_and(|p| {
                matches!(
                    p,
                    ir::Processor::Filter(ir::Filter {
                        kind: ir::FilterKind::Peak { .. },
                        ..
                    })
                )
            }),
        _ => false,
    };
    let eq_routes: Vec<_> = library
        .instrument
        .routes
        .iter()
        .enumerate()
        .filter_map(|(i, r)| is_eq(r).then_some(ir::RouteRef(i)))
        .collect();
    let selected = library
        .instrument
        .zones
        .iter()
        .position(|z| {
            z.keys.low <= key
                && key <= z.keys.high
                && z.velocities.low <= 64
                && 64 <= z.velocities.high
                && z.trigger == ir::Trigger::Attack
                && z.routes.iter().any(|r| {
                    eq_routes.contains(r)
                        && matches!(
                            library.instrument.modulators[library.instrument.routes[r.0].source.0]
                                .source,
                            ir::ModulationSource::Velocity | ir::ModulationSource::Constant
                        )
                })
        })
        .context("no matching authored velocity/constant EQ gain zone")?;
    let z = &library.instrument.zones[selected];
    println!(
        r#"EQ_SELECTION {{"zone":{selected},"velocity_low":{},"velocity_high":{},"conditions":{},"axis_choices":{},"group_start_conditions":{}}}"#,
        z.velocities.low,
        z.velocities.high,
        z.conditions.len(),
        z.axes.len(),
        z.group
            .map_or(0, |g| library.instrument.groups[g.0].start.len())
    );
    let mut eq_slots = Vec::new();
    for route in z.routes.iter().filter(|r| eq_routes.contains(r)) {
        if let ir::Target::Processor {
            chain,
            index,
            parameter,
        } = library.instrument.routes[route.0].target
            && let Some(binding) = library
                .instrument
                .processor_controls
                .iter()
                .find(|b| b.chain == chain && b.index == index && b.parameter == parameter)
            && let Some(alias) = library
                .instrument
                .source_indices
                .control_aliases
                .iter()
                .find(|a| a.control == binding.control)
            && !eq_slots.contains(&alias.address)
        {
            eq_slots.push(alias.address);
        }
    }
    let mut ordinal = 0;
    let kept = library.instrument.retain_zones(|_| {
        let keep = ordinal == selected;
        ordinal += 1;
        keep
    });
    let mut pcm = Vec::new();
    for old in kept {
        let decoded = library.samples.decode(&library.locations[old])?;
        pcm.push(Pcm::new(decoded.rate, decoded.frames.into_boxed_slice())?);
    }
    let options = sampler_kontakt::Options {
        keys: key..=key,
        scripts: false,
        mpe: None,
        ..Default::default()
    };
    let mut outputs = Vec::new();
    for enabled in [false, true] {
        let mut instrument = library.instrument.clone();
        if !enabled {
            instrument.zones[0]
                .routes
                .retain(|r| !eq_routes.contains(r));
        }
        let loaded = sampler_kontakt::finish(
            instrument,
            pcm.clone(),
            vec!["RAM witness".into(); pcm.len()],
            &options,
        )?;
        let mut activation = Vec::new();
        for slot in &eq_slots {
            let address = sampler_core::EngineParameterAddress {
                parameter: sampler_core::engine_parameter_id("ENGINE_PAR_EFFECT_BYPASS").unwrap(),
                group: slot.group,
                slot: slot.slot,
                generic: slot.generic,
            };
            if loaded.instrument.chains.iter().any(|chain| {
                chain
                    .pre_amplitude
                    .iter()
                    .chain(&chain.post_amplitude)
                    .any(|p| matches!(p, ir::Processor::Mix { address, .. } if address == slot))
            }) {
                activation.push(address);
            }
        }
        let plan = loaded.plan.with_signal_trace(32768)?;
        let limits = Limits::for_plan(&plan, 16, 256);
        let mut rt = Runtime::new(plan, limits)?;
        let reader = rt.signal_trace_reader().context("trace reader")?;
        let mut out = vec![[0.; 2]; 24000];
        heap::without_heap(|| {
            for address in &activation {
                rt.set_engine_parameter(*address, 0).unwrap();
            }
            rt.trigger(
                Input {
                    protocol: Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key,
                    external_id: None,
                },
                key,
                64. / 127.,
            )
            .unwrap();
            for block in out.chunks_mut(64) {
                rt.render(block).unwrap();
            }
        });
        let stats = rt.stats();
        ensure!(
            stats.nonfinite_frames == 0 && stats.voice_drops == 0 && stats.refused_starts == 0,
            "runtime problem counters"
        );
        let rows = reader.drain();
        println!(
            "EQ_RUNTIME {{\"enabled\":{enabled},\"voices\":{},\"trace_rows\":{}}}",
            stats.voices,
            rows.len()
        );
        ensure!(reader.dropped() == 0, "trace capacity loss");
        for node in reader
            .graph
            .nodes
            .iter()
            .filter(|n| n.processor == "v1_peaking_eq")
        {
            if let Some(row) = rows
                .iter()
                .find(|r| r.node == node.id && r.input.rms[0] > 1e-8 && !r.bypassed && r.enabled)
            {
                println!(
                    "EQ_STAGE {{\"enabled\":{enabled},\"node\":{},\"gain_db\":{},\"input_rms\":{},\"output_rms\":{}}}",
                    node.id, row.values[2], row.input.rms[0], row.output.rms[0]
                );
            }
        }
        let power = out
            .iter()
            .flatten()
            .map(|x| f64::from(*x).powi(2))
            .sum::<f64>();
        ensure!(power > 1e-12, "isolated authored zone is silent");
        println!(
            "EQ_SIDE {{\"enabled\":{enabled},\"source_zone\":{selected},\"eq_routes\":{},\"slots_activated\":{},\"power\":{power},\"samples\":48000,\"heap_calls\":0,\"nonfinite\":0}}",
            eq_routes.len(),
            activation.len()
        );
        outputs.push(out);
    }
    let power = outputs[0]
        .iter()
        .flatten()
        .map(|x| f64::from(*x).powi(2))
        .sum::<f64>();
    let delta = outputs[0]
        .iter()
        .flatten()
        .zip(outputs[1].iter().flatten())
        .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
        .sum::<f64>();
    ensure!(
        delta / power > 1e-10,
        "authored EQ routes must affect the rendered output"
    );
    println!(
        "EQ_AB {{\"relative_difference_power\":{},\"difference_db\":{},\"native_parity\":\"UNVERIFIED\",\"quiet_cpu\":\"UNKNOWN\"}}",
        delta / power,
        10. * (delta / power).log10()
    );
    Ok(())
}
