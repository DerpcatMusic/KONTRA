//! Offline production-loader/playback audit. Numeric audio evidence, no PCM export.
use super::{V2Core, V2Loader};
use crate::sound::{BlockInfo, Core, CoreLoader, LoadRequest, ScriptUi, event::Event};
use serde_json::{Value, json};
use std::{path::Path, time::Instant};

fn pump(core: &mut V2Core, scripts: &mut ScriptUi) {
    let mut jobs = Vec::new();
    core.take_effects(0, &mut |instance, effect| {
        if matches!(
            effect.service,
            sampler_core::MIDI_SERVICE | sampler_core::ARRAY_FILE_SERVICE
        ) {
            jobs.push(*effect);
        } else {
            scripts.apply(instance, effect);
        }
        true
    });
    for effect in &jobs {
        if effect.service == sampler_core::ARRAY_FILE_SERVICE {
            if let Some(Some(part)) = core.parts.get_mut(0)
                && let Ok(mut job) = sampler_core::ArrayFileCompletion::from_effect(effect)
                && part
                    .runtime
                    .capture_array_file(effect.plan, &mut job)
                    .is_ok()
            {
                let _ = job.perform_read_only();
                let _ = part.runtime.complete_array_file(effect.plan, &mut job);
            }
        } else if effect.args[0] == 4 {
            // A diagnostic load may read library state but never writes MIDI files.
            if let Some(Some(part)) = core.parts.get_mut(0)
                && let Some(instance) = effect.instance
            {
                let mut job = sampler_core::MidiCompletion::empty(effect.args[1] as i32, instance);
                job.success = false;
                let _ = part.runtime.complete_midi(effect.plan, &mut job);
            }
        } else {
            core.service_script_io(0, effect);
        }
    }
    core.settle_script_io(0);
}

pub fn run(path: &Path) -> Value {
    let started = Instant::now();
    let mut trace = crate::diagnostics::LoadTrace::new(path, 0, Some(0));
    let request = LoadRequest {
        path: path.into(),
        sample_rate: 48_000.,
        dynamics_start: Some(100),
        ..Default::default()
    };
    let loaded = V2Loader.prepare(&request, &mut |_| {}, &|| false);
    let mut loaded = match loaded {
        Ok(loaded) => loaded,
        Err(error) => {
            trace.issue("prepare", "failed", error.to_string());
            return json!({"status":"load_failed","path":path,"diagnostics":trace.finish("failed").as_ref(),
                "scope":"production V2Loader; no runtime installed","elapsed_ms":started.elapsed().as_secs_f64()*1000.});
        }
    };
    for missing in &loaded.report.missing {
        trace.missing(missing, loaded.instrument.as_deref());
    }
    if let Some(instrument) = loaded.instrument.as_deref() {
        trace.ksp_symbols(&sampler_kontakt::parity_summaries(instrument));
    }
    if let Some(startup) = &loaded.report.stream_startup {
        trace.detail(
            "stream_startup",
            serde_json::to_value(startup).unwrap_or_default(),
        );
        if let Some(message) = startup.warning() {
            trace.issue("prepare", "attack_preload_budget", message);
        }
    }
    let load_ms = started.elapsed().as_secs_f64() * 1000.;
    let mut runtime_log = trace.runtime_log();
    let mut core = V2Core::with_parts(1, 48_000.);
    core.install(0, loaded.part);
    let mut samples = 0usize;
    let mut peak = 0f32;
    let mut nonfinite = 0usize;
    let mut fault_count = 0usize;
    let mut tick = |core: &mut V2Core, frames: usize, audition: bool| {
        core.begin_block(&BlockInfo {
            frames,
            offline: true,
            ..Default::default()
        });
        let audio = core.render(frames);
        if audition {
            for value in audio
                .buses
                .iter()
                .flat_map(|bus| bus.iter().flat_map(|channel| channel.iter().take(frames)))
            {
                if value.is_finite() {
                    peak = peak.max(value.abs());
                } else {
                    nonfinite += 1;
                }
            }
            samples += frames;
        }
        pump(core, &mut loaded.scripts);
        if let Some(Some(part)) = core.parts.get_mut(0) {
            part.runtime
                .flush_behavior_diagnostics(|_, _, outcome, program, context| {
                    if matches!(
                        outcome,
                        sampler_core::Outcome::Fault(_) | sampler_core::Outcome::FuelExhausted
                    ) {
                        fault_count += 1;
                        runtime_log.fault_context(
                            program,
                            outcome,
                            context.as_ref(),
                            loaded.instrument.as_deref(),
                        );
                    }
                    true
                });
        }
        core.end_block(frames, &mut |_| true);
        std::thread::yield_now();
    };
    // Four seconds is a declared activation budget; periodic listeners need not terminate.
    for _ in 0..1500 {
        tick(&mut core, 128, false);
    }
    let keys = loaded
        .instrument
        .as_ref()
        .map(|instrument| {
            let mut candidates: Vec<_> = (24u8..=96)
                .map(|key| {
                    let count = instrument
                        .zones
                        .iter()
                        .filter(|zone| zone.keys.low <= key && key <= zone.keys.high)
                        .count();
                    (std::cmp::Reverse(count), key.abs_diff(60), key)
                })
                .filter(|(count, _, _)| count.0 != 0)
                .collect();
            candidates.sort();
            candidates
                .into_iter()
                .map(|(_, _, key)| key)
                .take(1)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let key = keys.first().copied().unwrap_or(60);
    core.event(0, Event::midi1(0xb0, 1, 100));
    core.event(0, Event::midi1(0xb0, 11, 127));
    core.event(0, Event::midi1(0x90, key, 100));
    for _ in 0..375 {
        tick(&mut core, 128, true);
    }
    core.event(0, Event::midi1(0x80, key, 0));
    for _ in 0..375 {
        tick(&mut core, 128, true);
    }
    drop(tick);
    let lua = loaded
        .scripts
        .uvi
        .as_ref()
        .map(|bridge| bridge.fault_counts());
    if let Some(faults) = &lua {
        runtime_log.lua(faults);
    }
    let problems = core.problems(0);
    runtime_log.counters(problems);
    trace.detail("runtime_diagnostics", runtime_log.summary());
    let diagnostics = trace.finish("loaded");
    let mut decoded = loaded.report.decoded.clone();
    let keys_hex = format!("{:032x}", decoded.keys);
    decoded.keys = 0;
    let mut decoded = serde_json::to_value(decoded).unwrap_or_default();
    decoded["keys_hex"] = json!(keys_hex);
    if let Some(object) = decoded.as_object_mut() {
        object.remove("keys");
    }
    json!({"status":"executed","path":path,"name":loaded.report.name,
        "scope":"production V2Loader, plan activation, CC1/11, one covered note and release; discarded PCM; no DAW/UI gesture/native equivalence claim",
        "load_ms":load_ms,"elapsed_ms":started.elapsed().as_secs_f64()*1000.,
        "decoded":decoded,"conversion_gap_count":loaded.report.missing.len(),
        "diagnostics":diagnostics.as_ref(),"runtime_problems":problems,"runtime_fault_count":fault_count,"lua_faults":lua,
        "note":{"key":key,"velocity":100,"selection":if keys.is_empty(){"fallback"}else{"source zone coverage"},
            "frames":samples,"peak":peak,"nonfinite_samples":nonfinite},
        "activation_pending":core.scripts_running(0)})
}
