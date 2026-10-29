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
            "script_slot":slot, "callback_type":kind, "callback_name":callback_name(*kind), "callback_id":id, "resume":resume}),
        TraceEvent::Action {
            frame,
            slot,
            callback,
            action,
            call_id,
            args,
            len,
            integer_count,
            text_count,
        } => json!({"type":"script_action",
            "frame":frame, "script_slot":slot, "callback_id":callback, "action":format!("{action:?}"),
            "call_id":call_id, "args":args.get(..usize::from(*len)), "integer_count":integer_count, "text_count":text_count}),
        TraceEvent::ActionInteger {
            call_id,
            index,
            value,
        } => {
            json!({"type":"script_action_integer", "call_id":call_id, "index":index, "value":value})
        }
        TraceEvent::ActionText {
            call_id,
            index,
            value,
        } => {
            json!({"type":"script_action_text", "call_id":call_id, "index":index, "value":value.as_str()})
        }
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

fn callback_name(kind: u8) -> Option<&'static str> {
    match kind {
        1 => Some("note"),
        2 => Some("release"),
        3 => Some("controller"),
        5 => Some("init"),
        6 => Some("ui_control"),
        8 => Some("pgs"),
        12 => Some("listener"),
        13 => Some("async_complete"),
        15 => Some("ui_controls"),
        _ => None,
    }
}
