//! Headless host adapter for the same lossless effect outbox as a product host.
//! Ported from sampler-native::render_kontakt::service_ui_effects: callbacks
//! suspended by a full outbox must resume after the host drains and renders.
//! This changes host servicing, not any Kontakt builtin or DSP law.
use std::io;

/// Apply each owned UI effect to its retained model. Array-file jobs are
/// serviced read-only (load_array reads the named file; save_array completes
/// as failed, never writing). MIDI jobs need an actual service host: reject and
/// retain them, never fabricate success. The host must render again to resume
/// a callback that was back-pressured or waits on a completed job.
pub fn service_ui_effects(
    runtime: &mut sampler_core::Runtime,
    scripts: &mut [crate::ScriptView],
) -> io::Result<usize> {
    let plan = runtime.active_plan();
    let mut accepted = 0;
    let mut failure = None;
    let mut array_jobs = Vec::new();
    runtime.drain_effects(|effect| {
        if effect.service == sampler_core::ARRAY_FILE_SERVICE {
            array_jobs.push(*effect);
            return true;
        }
        let reason = match effect.service {
            sampler_core::MIDI_SERVICE => Some("headless renderer needs a MIDI service host"),
            _ => None,
        };
        if let Some(reason) = reason {
            failure = Some(reason);
            return false;
        }
        let Some(instance) = effect.instance else {
            failure = Some("script effect has no owner");
            return false;
        };
        let Some(script) = scripts.get_mut(usize::from(instance.0)) else {
            failure = Some("script effect owner is unavailable");
            return false;
        };
        if effect.plan != plan || script.service(effect.service).is_none() {
            failure = Some("script effect does not belong to this plan and service table");
            return false;
        }
        script.apply_ui_effect_for(plan, instance, effect);
        accepted += 1;
        true
    });
    for effect in &array_jobs {
        service_array_file(runtime, effect)?;
    }
    match failure {
        Some(reason) => Err(io::Error::new(io::ErrorKind::Unsupported, reason)),
        None => Ok(accepted),
    }
}

/// One load_array/save_array job through the runtime's own capture and
/// completion, as the plugin's array worker does, but never writing.
fn service_array_file(runtime: &mut sampler_core::Runtime, effect: &sampler_core::Effect) -> io::Result<()> {
    let fail = |e: sampler_core::Error| io::Error::other(format!("array-file job: {e:?}"));
    let mut job = sampler_core::ArrayFileCompletion::from_effect(effect).map_err(fail)?;
    runtime.capture_array_file(effect.plan, &mut job).map_err(fail)?;
    // A failed read or a refused save completes the job with failure status.
    let _ = job.perform_read_only();
    runtime.complete_array_file(effect.plan, &mut job).map_err(fail)
}
