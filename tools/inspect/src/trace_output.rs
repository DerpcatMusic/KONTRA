//! Conversion of fixed-size engine events to the inspector's JSON schema.
use kontra_engine::{Plan, RouteUnit, RouteValue, TraceEvent};
use serde_json::{Value, json};

pub fn event(e: &TraceEvent, plan: &Plan, samples: &[Option<String>], rate: u32) -> Value {
    match e {
        TraceEvent::Callback {
            frame,
            slot,
            kind,
            id,
            note_id,
            controller,
            control,
            resume,
        } => json!({"type":"callback", "frame":frame,
            "script_slot":slot, "callback_type":kind, "callback_name":callback_name(*kind), "callback_id":id, "note_id":note_id,
            "controller": if !resume && *kind == 3 {Some(*controller)} else {None},
            "control": if matches!(*kind, 6 | 15) {Some(*control)} else {None}, "resume":resume}),
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
            chunk,
            len,
            value,
        } => {
            let bytes = value.get(..usize::from(*len)).unwrap_or_default();
            json!({"type":"script_action_text", "call_id":call_id, "index":index, "chunk":chunk, "value":String::from_utf8_lossy(bytes)})
        }
        TraceEvent::RouteOnset {
            frame,
            voice_slot,
            zone,
            mod_slot,
            target,
            kind,
            source,
        } => json!({
            "type":"route_onset", "frame":frame, "voice_slot":voice_slot, "zone":zone,
            "mod_slot":mod_slot, "target":target, "route":super::routes::route(*kind), "source_at_onset":source,
        }),
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

/// A contribution captured in the engine while a controller refreshed a voice.
pub fn route(value: &RouteValue, plan: &Plan, samples: &[Option<String>]) -> Value {
    let group = plan
        .zones
        .get(value.zone)
        .and_then(|z| usize::try_from(z.group).ok())
        .and_then(|i| plan.groups.get(i));
    let names: Vec<_> = group
        .into_iter()
        .flat_map(|g| &g.mods)
        .filter(|m| {
            m.slot == value.mod_slot
                && m.target == value.target
                && std::mem::discriminant(&m.kind) == std::mem::discriminant(&value.kind)
        })
        .map(|m| m.name.as_str())
        .collect();
    let name = if names.len() == 1 {
        names.first().copied()
    } else {
        None
    };
    let input_domain = match value.unit {
        RouteUnit::GainMultiplier
        | RouteUnit::ResonanceMultiplier
        | RouteUnit::IntensityMultiplier => "unipolar source",
        RouteUnit::Pan
        | RouteUnit::Semitones
        | RouteUnit::CutoffDepth
        | RouteUnit::ResonanceDepth
        | RouteUnit::EqDecibels => "intensity-scaled source",
        _ => "raw source",
    };
    json!({"frame":value.frame,"voice_slot":value.voice_slot,"zone":value.zone,"sample":samples.get(value.zone),
        "controller":value.controller,"direct_controller_source":value.direct_controller_source,"script_callback":value.script_callback,
        "evaluation":if value.timed_refresh {"timed_span_after_controller"} else {"controller_refresh"},
        "mod_slot":value.mod_slot,"target":value.target,"name":name,"route":super::routes::route(value.kind),
        "input":value.source,"input_domain":input_domain,"contribution":value.value,"unit":format!("{:?}",value.unit),
        "meaning":"contribution evaluated in controller-triggered voice refresh; before smoothing, combining and final clamping"})
}
