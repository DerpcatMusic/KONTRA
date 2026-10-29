//! Conversion of fixed-size engine events to the inspector's JSON schema.
use kontra_engine::{Plan, TraceEvent};
use serde_json::{Value, json};

pub fn event(e: &TraceEvent, plan: &Plan, samples: &[Option<String>], rate: u32) -> Value {
    match e {
        TraceEvent::Callback {
            frame,
            slot,
            kind,
            id,
            resume,
        } => json!({"type":"callback", "frame":frame,
            "script_slot":slot, "callback_type":kind, "callback_id":id, "resume":resume}),
        TraceEvent::Action {
            frame,
            slot,
            callback,
            action,
            args,
            len,
            truncated,
        } => json!({"type":"script_action",
            "frame":frame, "script_slot":slot, "callback_id":callback, "action":format!("{action:?}"),
            "args":args.get(..usize::from(*len)), "arguments_truncated":truncated}),
        TraceEvent::Voice {
            frame,
            slot,
            zone,
            event,
            position,
            step,
            gain,
        } => {
            let pitch = plan.zones.get(*zone).and_then(|z| {
                let ratio = *step * f64::from(rate) / f64::from(z.sample_rate);
                (ratio.is_finite() && ratio > 0.0).then(|| f64::from(z.root) + 12.0 * ratio.log2())
            });
            json!({"type":"voice", "frame":frame, "slot":slot, "zone":zone, "sample":samples.get(*zone),
                "event":format!("{event:?}"), "position":position,
                "position_reference": if matches!(event, kontra_engine::VoiceEvent::Start) {"sample_start"} else {"relative_cursor"},
                "gain_reference": if matches!(event, kontra_engine::VoiceEvent::Start) {"target_before_envelope"} else {"current_before_sample"}, "source_frames_per_output_frame":step,
                "pitch_midi_relative_to_root":pitch, "gain":gain,
                "time_precision": if matches!(event, kontra_engine::VoiceEvent::End) {"render_span_end"} else {"sample"}})
        }
    }
}
