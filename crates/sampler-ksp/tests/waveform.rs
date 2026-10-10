//! Waveform operand/validation regressions. Authored source; NOT_RUN in this lane.
//! Vendor evidence and runtime gates: docs/KSP_WAVEFORM_OPERANDS.md.
use sampler_ksp::model::{Request, Value};

fn compile(body: &str) -> sampler_ksp::Script {
    sampler_ksp::compile(
        &format!("on init declare ui_waveform $w(1,1) {body} end on"),
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap()
}

fn wave(script: &sampler_ksp::Script) -> sampler_ui_ir::Waveform {
    script.ui(&|_| None).unwrap().widgets[0]
        .waveform
        .clone()
        .unwrap()
}

#[test]
fn waveform_four_operand_values_and_slice_indices_are_distinct() {
    let script = compile(
        "attach_zone($w,27,3)
         set_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,0,12000)
         set_ui_wf_property($w,$UI_WF_PROP_FLAGS,0,11)
         set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,3,77)
         set_ui_wf_property($w,$UI_WF_PROP_TABLE_IDX_HIGHLIGHT,3,1)
         set_ui_wf_property($w,$UI_WF_PROP_MIDI_DRAG_START_NOTE,0,65)",
    );
    assert_eq!(
        wave(&script),
        sampler_ui_ir::Waveform {
            zone: 27,
            flags: 11,
            cursor_us: 12000,
            table: vec![0, 0, 0, 77],
            highlighted: Some(3),
            midi_start_note: 65,
        }
    );
    // The request recorder must preserve the vendor's index-before-value order.
    let table = script.model().requests.iter().find(|r| {
        r.command == "set_ui_wf_property"
            && r.args.get(1) == Some(&Value::Text("$UI_WF_PROP_TABLE_VAL".into()))
    }).unwrap();
    assert_eq!(&table.args[2..], &[Value::Int(3), Value::Int(77)]);
    assert!(wave(&script).table.get(77).is_none());
}

#[test]
fn waveform_slice_index_bounds_do_not_alias_value_or_allocate_from_value() {
    let script = compile(
        "attach_zone($w,27,3)
         set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,3,77)
         set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,-1,9)
         set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,65536,8)
         set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,0,2147483647)",
    );
    // 65536 is an owned storage limit, not a vendor slice-count claim.
    assert_eq!(wave(&script).table, vec![i32::MAX, 0, 0, 77]);
}

#[test]
fn waveform_requests_do_not_cross_widget_source_identity() {
    let script = compile(
        "declare ui_waveform $other(1,1)
         attach_zone($w,27,3)
         attach_zone($other,91,0)
         set_ui_wf_property($other,$UI_WF_PROP_PLAY_CURSOR,0,24000)
         set_ui_wf_property($other,$UI_WF_PROP_TABLE_VAL,3,99)
         set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,3,77)",
    );
    let ui = script.ui(&|_| None).unwrap();
    let w = ui.widgets[0].waveform.as_ref().unwrap();
    let other = ui.widgets[1].waveform.as_ref().unwrap();
    assert_ne!(ui.widgets[0].source_id, ui.widgets[1].source_id);
    assert_eq!((w.zone, w.cursor_us, &w.table), (27, 0, &vec![0, 0, 0, 77]));
    assert_eq!((other.zone, other.cursor_us, &other.table), (91, 24000, &vec![0, 0, 0, 99]));
}

#[test]
fn waveform_decoder_rejects_malformed_and_unknown_property_requests() {
    let script = compile("attach_zone($w,27,3) set_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,0,12000)");
    let before = script.ui(&|_| None).unwrap();
    let mut model = script.model().clone();
    let id = model.interface.widgets[0].ui_id;
    let cursor = Value::Text("$UI_WF_PROP_PLAY_CURSOR".into());
    for args in [
        vec![Value::Int(id), cursor.clone(), Value::Int(99)],
        vec![Value::Int(id), cursor.clone(), Value::Int(0), Value::Text("bad".into())],
        vec![Value::Int(id), cursor.clone(), Value::Text("bad".into()), Value::Int(99)],
        vec![Value::Int(id), Value::Int(0), Value::Int(0), Value::Int(99)],
        vec![Value::Int(id), Value::Text("$UI_WF_PROP_UNKNOWN".into()), Value::Int(0), Value::Int(99)],
        vec![Value::Int(id), cursor, Value::Int(0), Value::Int(99), Value::Int(1)],
    ] {
        model.requests.push(Request { command: "set_ui_wf_property", args });
    }
    model.requests.push(Request {
        command: "attach_zone",
        args: vec![Value::Int(id), Value::Text("bad".into()), Value::Int(3)],
    });
    assert_eq!(sampler_ksp::ui::interface(&model, 0, &|_| None).unwrap(), before);
}

#[test]
fn waveform_setter_requires_four_operands_despite_vendor_getter_example() {
    assert!(sampler_ksp::compile(
        "on init declare ui_waveform $w(1,1) set_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,12000) end on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    ).is_err());
}

#[test]
#[ignore = "UNIMPLEMENTED: init waveform getter has no owned property state; NOT_RUN"]
fn waveform_init_getter_roundtrip_requirement() {
    let script = compile(
        "declare ui_table %read[4](1,1,1000000)
         attach_zone($w,27,3)
         set_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,0,12000)
         set_ui_wf_property($w,$UI_WF_PROP_FLAGS,0,11)
         set_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,3,77)
         set_ui_wf_property($w,$UI_WF_PROP_MIDI_DRAG_START_NOTE,0,65)
         %read[0] := get_ui_wf_property($w,$UI_WF_PROP_PLAY_CURSOR,0)
         %read[1] := get_ui_wf_property($w,$UI_WF_PROP_FLAGS,0)
         %read[2] := get_ui_wf_property($w,$UI_WF_PROP_TABLE_VAL,3)
         %read[3] := get_ui_wf_property($w,$UI_WF_PROP_MIDI_DRAG_START_NOTE,0)",
    );
    assert_eq!(
        script.ui(&|_| None).unwrap().widgets[1].value,
        Some(sampler_ui_ir::Value::Integers(vec![12000, 11, 77, 65]))
    );
}
