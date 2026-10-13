//! Headless host adapter for the same lossless effect outbox as a product host.
//! Ported from sampler-native::render_kontakt::service_ui_effects: callbacks
//! suspended by a full outbox must resume after the host drains and renders.
//! This changes host servicing, not any Kontakt builtin or DSP law.
use std::io;

/// Apply each owned UI effect to its retained model. External MIDI/array jobs
/// need an actual service host: reject and retain them, never fabricate success.
/// The host must render again to resume a callback that was back-pressured.
pub fn service_ui_effects(
    runtime: &mut sampler_core::Runtime,
    scripts: &mut [crate::ScriptView],
) -> io::Result<usize> {
    let plan = runtime.active_plan();
    let mut accepted = 0;
    let mut failure = None;
    runtime.drain_effects(|effect| {
        let reason = match effect.service {
            sampler_core::MIDI_SERVICE => Some("headless renderer needs a MIDI service host"),
            sampler_core::ARRAY_FILE_SERVICE => {
                Some("headless renderer needs an array-file service host")
            }
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
    match failure {
        Some(reason) => Err(io::Error::new(io::ErrorKind::Unsupported, reason)),
        None => Ok(accepted),
    }
}
