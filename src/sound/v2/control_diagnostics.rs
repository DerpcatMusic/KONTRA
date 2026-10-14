//! Off-audio rejection context. Never retain control values, paths or text.
use sampler_core::{ControlOperation as Op, ControlReply};

pub(super) fn rejection(reply: &ControlReply) -> Option<serde_json::Value> {
    let error = reply.result.as_ref().err()?;
    let (operation, count, first_widget) = match &reply.command.operation {
        Op::ArrayFileCapture(_) => ("array_file_capture", 1, None),
        Op::ArrayFileComplete(_) => ("array_file_complete", 1, None),
        Op::MidiCapture(_) => ("midi_capture", 1, None),
        Op::MidiComplete(_) => ("midi_complete", 1, None),
        Op::Invoke(_, write) => ("invoke", 1, Some((write.id.0, 0))),
        Op::HostParameter(_, _, _) => ("host_parameter", 1, None),
        Op::InvokeWidget(_, edits) => (
            "invoke_widget",
            edits.len(),
            edits.first().map(|e| (e.id.0, e.index)),
        ),
        Op::CaptureWidget(edits) => (
            "capture_widget",
            edits.len(),
            edits.first().map(|e| (e.id.0, e.index)),
        ),
        Op::CaptureScriptState(state) => (
            "capture_script_state",
            state.values.len().saturating_add(state.callbacks.len()),
            None,
        ),
        Op::RestoreScriptState(state) => (
            "restore_script_state",
            state.values.len().saturating_add(state.callbacks.len()),
            None,
        ),
        Op::Edit(edits) => ("edit", edits.len(), edits.first().map(|e| (e.id.0, 0))),
        Op::Recall(edits) => ("recall", edits.len(), edits.first().map(|e| (e.id.0, 0))),
        Op::Capture(edits) => ("capture", edits.len(), edits.first().map(|e| (e.id.0, 0))),
    };
    Some(serde_json::json!({
        "request": reply.request,
        "reason": format!("{error:?}"),
        "operation": operation,
        "plan": format!("{:?}", reply.command.plan),
        "expected_revision": reply.command.expected_revision,
        "edit_count": count,
        "first_widget_id": first_widget.map(|(id, _)| id),
        "first_widget_index": first_widget.map(|(_, index)| index),
        "behavior": reply.behavior.map(|id| format!("{id:?}")),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_core::{ControlRequest, Error, Limits, Prepared, Runtime, WidgetEdit, WidgetValue};

    #[test]
    fn rejection_identifies_operation_without_exporting_values() {
        let prepared = Prepared::new(48000, vec![], vec![], 0).unwrap();
        let limits = Limits::for_plan(&prepared, 8, 0);
        let runtime = Runtime::new(prepared, limits).unwrap();
        let reply = ControlReply {
            request: 157,
            behavior: None,
            command: ControlRequest {
                plan: runtime.active_plan(),
                expected_revision: Some(9),
                operation: Op::CaptureWidget(vec![WidgetEdit {
                    id: sampler_core::ControlId(650),
                    index: 2,
                    value: WidgetValue::Integer(987654321),
                    interaction: Default::default(),
                }]),
            },
            result: Err(Error::Capacity),
        };
        let record = rejection(&reply).unwrap();
        assert_eq!(record["operation"], "capture_widget");
        assert_eq!(record["first_widget_id"], 650);
        assert_eq!(record["first_widget_index"], 2);
        assert_eq!(record["expected_revision"], 9);
        assert!(!record.to_string().contains("987654321"));
        let mut success = reply;
        success.result = Ok((1, 10));
        assert_eq!(rejection(&success), None);
    }
}
