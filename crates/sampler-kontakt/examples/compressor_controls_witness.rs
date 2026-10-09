//! An authored compressor and gate sample; decoded PCM and A/B output stay in RAM.
use anyhow::{Context, Result, ensure};
use sampler_core::{
    ControlValue, ControlWrite, Input, Limits, ParameterRole, Pcm, Protocol, Runtime,
};
use sampler_ir as ir;
use std::path::Path;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod heap;

fn main() -> Result<()> {
    let path = std::env::args().nth(1).context("NKI path")?;
    let mut library = sampler_kontakt::read(Path::new(&path))
        .map_err(|_| anyhow::anyhow!("instrument read failed; authored diagnostics omitted"))?;
    let (chain, index, settings) = library
        .instrument
        .chains
        .iter()
        .enumerate()
        .find_map(|(chain, c)| {
            c.pre_amplitude
                .iter()
                .chain(&c.post_amplitude)
                .enumerate()
                .find_map(|(index, p)| match p {
                    ir::Processor::Compressor(s) => Some((chain, index, *s)),
                    _ => None,
                })
        })
        .context("no authored compressor")?;
    let (zone, asset) = library
        .instrument
        .zones
        .iter()
        .enumerate()
        .find(|(_, z)| z.keys.low <= 60 && z.keys.high >= 60 && z.velocities.high >= 64)
        .map(|(n, z)| (n, z.asset.0))
        .context("no gate sample")?;
    let decoded = library.samples.decode(&library.locations[asset])?;
    let peak = decoded
        .frames
        .iter()
        .flatten()
        .map(|v| v.abs())
        .fold(0_f32, f32::max);
    ensure!(peak > 1e-6, "source sample silent");
    // Known probe drive exposes gain reduction even for a quiet authored sample.
    println!(
        "COMPRESSOR_OWNER {{\"source_scope\":\"{:?}\",\"threshold_db\":{},\"ratio\":{},\"attack_seconds\":{},\"release_seconds\":{},\"probe_input_scale\":{},\"isolated\":true,\"native_parity\":\"UNVERIFIED\"}}",
        library.instrument.chains[chain].scope,
        settings.threshold_db,
        settings.ratio,
        settings.attack.seconds(),
        settings.release.seconds(),
        0.8 / peak
    );
    let pcm = Pcm::new(
        decoded.rate,
        decoded
            .frames
            .into_iter()
            .map(|f| f.map(|v| v * (0.8 / peak)))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )?;
    let mut instrument = ir::Instrument {
        assets: vec![library.instrument.assets[asset].clone()],
        zones: vec![ir::Zone {
            chain: Some(ir::ChainRef(0)),
            pitch: ir::KeyTracking::Fixed,
            velocity: ir::VelocityResponse::None,
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        chains: vec![ir::Chain {
            scope: ir::Scope::Voice,
            pre_amplitude: vec![],
            post_amplitude: vec![ir::Processor::Compressor(settings)],
        }],
        ..Default::default()
    };
    instrument.register_compressor_controls();
    let options = sampler_kontakt::Options {
        scripts: false,
        mpe: None,
        ..Default::default()
    };
    let mut baseline: Vec<[f32; 2]> = Vec::new();
    for changed in [
        None,
        Some(ParameterRole::Threshold),
        Some(ParameterRole::Ratio),
        Some(ParameterRole::Attack),
        Some(ParameterRole::Release),
    ] {
        let loaded = sampler_kontakt::finish(
            instrument.clone(),
            vec![pcm.clone()],
            vec!["RAM witness".into()],
            &options,
        )
        .map_err(|_| {
            anyhow::anyhow!("instrument preparation failed; authored diagnostics omitted")
        })?;
        let lane = changed.map(|role| {
            loaded
                .plan
                .parameter_registry()
                .descriptors()
                .find(|d| d.role == role)
                .unwrap()
                .clone()
        });
        let limits = Limits::for_plan(&loaded.plan, 8, 32);
        let mut rt = Runtime::new(loaded.plan, limits)?;
        let mut output = vec![[0.; 2]; 48000];
        heap::without_heap(|| {
            rt.trigger(
                Input {
                    protocol: Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: 60,
                    external_id: None,
                },
                60,
                1.,
            )
            .unwrap();
            for block in output[..1024].chunks_mut(64) {
                rt.render(block).unwrap();
            }
            if let Some(d) = &lane {
                let value = if d.default == d.range[1] {
                    d.range[0]
                } else {
                    d.range[1]
                };
                rt.edit_controls(
                    rt.active_plan(),
                    None,
                    &[ControlWrite {
                        id: d.control,
                        value: ControlValue::Real(value),
                    }],
                )
                .unwrap();
            }
            for block in output[1024..].chunks_mut(64) {
                rt.render(block).unwrap();
            }
        });
        ensure!(rt.stats().nonfinite_frames == 0, "nonfinite runtime");
        let power = output
            .iter()
            .flatten()
            .map(|v| f64::from(*v).powi(2))
            .sum::<f64>();
        ensure!(power > 1e-12, "silent render");
        if let Some(role) = changed {
            let diff = baseline
                .iter()
                .flatten()
                .zip(output.iter().flatten())
                .map(|(a, b)| f64::from(*a - *b).powi(2))
                .sum::<f64>();
            let base = baseline
                .iter()
                .flatten()
                .map(|v| f64::from(*v).powi(2))
                .sum::<f64>();
            println!(
                "COMPRESSOR_SIDE {{\"role\":\"{role:?}\",\"source_chain\":{chain},\"source_processor\":{index},\"source_zone\":{zone},\"difference_power_ratio\":{},\"level_delta_db\":{},\"heap_calls\":0,\"nonfinite\":0}}",
                diff / base,
                10. * (power / base).log10()
            );
            ensure!(
                diff / base > 1e-10,
                "authored compressor lane has no audible effect"
            );
        } else {
            baseline = output;
        }
    }
    Ok(())
}
