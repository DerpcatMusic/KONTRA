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
        Op::CaptureSnapshotFence(_) => ("capture_snapshot_fence", 0, None),
        Op::NativeSnapshotOverlay(overlay) => ("native_snapshot_overlay", overlay.state.values.len() + overlay.native.len(), None),
        Op::Edit(edits) => ("edit", edits.len(), edits.first().map(|e| (e.id.0, 0))),
        Op::Recall(edits) => ("recall", edits.len(), edits.first().map(|e| (e.id.0, 0))),
        Op::Capture(edits) => ("capture", edits.len(), edits.first().map(|e| (e.id.0, 0))),
    };
    let value_kind = match &reply.command.operation {
        Op::InvokeWidget(_, edits) | Op::CaptureWidget(edits) => {
            edits.first().map(|edit| match edit.value {
                sampler_core::WidgetValue::Integer(_) => "integer",
                sampler_core::WidgetValue::Real(_) => "real",
                sampler_core::WidgetValue::Text(_) => "text",
                sampler_core::WidgetValue::DropPath { .. } => "drop_path",
            })
        }
        _ => None,
    };
    let admission = reply.admission_failure.as_ref().map(|a| serde_json::json!({
        "required_callbacks":a.required_callbacks,"used":a.used,"capacity":a.capacity,
        "available":a.available,"reserved":a.reserved,"scanned":a.scanned,"scan_truncated":a.scan_truncated,
        "note_owned":a.note_owned,"plan_owned":a.plan_owned,"waiting":a.waiting,"preempted":a.preempted,
        "terminal":a.terminal,"finished":a.finished,"yielded_queue":a.yielded_queue,
        "pending_commands":a.pending_commands,"reserved_commands":a.reserved_commands,
        "stalled_effects":a.stalled_effects,"outbox_used":a.outbox_used,"outbox_capacity":a.outbox_capacity,
        "owners_omitted":a.owners_omitted,
        "owners":a.owners.iter().flatten().map(|owner| serde_json::json!({
            "owner":format!("{:?}",owner.owner),"plan":format!("{:?}",owner.plan),
            "program":owner.program,"source_slot":owner.source_slot,"stage":owner.stage,"pc":owner.pc,
            "waiting":owner.waiting,"preempted":owner.preempted,
            "outcome":owner.outcome.map(|outcome|format!("{outcome:?}")),
        })).collect::<Vec<_>>()
    }));
    Some(serde_json::json!({
        "request": reply.request,
        "reason": format!("{error:?}"),
        "operation": operation,
        "plan": format!("{:?}", reply.command.plan),
        "expected_revision": reply.command.expected_revision,
        "edit_count": count,
        "first_widget_id": first_widget.map(|(id, _)| format!("0x{id:032x}")),
        "first_widget_index": first_widget.map(|(_, index)| index),
        "first_widget_value_kind": value_kind,
        "callback_admission": admission,
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
            admission_failure: None,
            command: ControlRequest {
                plan: runtime.active_plan(),
                expected_revision: Some(9),
                operation: Op::CaptureWidget(vec![WidgetEdit {
                    id: sampler_core::ControlId(u128::MAX - 650),
                    index: 2,
                    value: WidgetValue::Integer(987654321),
                    interaction: Default::default(),
                }]),
            },
            result: Err(Error::Capacity),
        };
        let record = rejection(&reply).unwrap();
        assert_eq!(record["operation"], "capture_widget");
        assert_eq!(record["first_widget_id"], format!("0x{:032x}", u128::MAX - 650));
        assert_eq!(record["first_widget_index"], 2);
        assert_eq!(record["expected_revision"], 9);
        assert_eq!(record["first_widget_value_kind"], "integer");
        assert!(record["callback_admission"].is_null());
        assert!(!record.to_string().contains("987654321"));
        let mut success = reply;
        success.result = Ok((1, 10));
        assert_eq!(rejection(&success), None);
    }
}
