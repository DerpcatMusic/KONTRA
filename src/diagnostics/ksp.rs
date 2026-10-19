//! Resolve bounded VM evidence on the loader thread, including source context.
use sampler_core::{BehaviorFault, Instruction as I, Op, SourceSite};
use serde_json::{Value, json};

fn hex_digest(digest: [u8; 32]) -> String {
    use std::fmt::Write;
    let mut text = String::with_capacity(64);
    for byte in digest {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn source_site(site: SourceSite) -> Value {
    json!({"pc":site.pc,"offset":site.offset,"line":site.line,
        "column":site.column,"builtin":site.builtin})
}

pub(super) fn fault_data(
    fault: &BehaviorFault,
    instrument: Option<&sampler_ir::Instrument>,
) -> Value {
    let retained = fault
        .registers
        .get(..fault.register_count)
        .unwrap_or(&fault.registers);
    let registers = retained
        .iter()
        .enumerate()
        .map(|(register, value)| json!({"register":register + usize::from(fault.register_base),"bits":value}))
        .collect::<Vec<_>>();
    let read = |local: u16| {
        let index = usize::from(local).checked_sub(usize::from(fault.register_base))?;
        retained.get(index).copied()
    };
    let at = |base: u16, offset: u16| base.checked_add(offset).and_then(read);
    let operands = match fault.instruction {
        Some(
            I::ReadScriptArray { array, index, .. } | I::WriteScriptArray { array, index, .. },
        ) => {
            json!({"array_offset":array.offset,"length":array.len,"index":read(index),"index_register":index})
        }
        Some(I::WaitMicros { local } | I::MicrosToFrames { local }) => {
            json!({"microseconds":read(local),"register":local,"expected":"nonnegative duration"})
        }
        Some(I::WaitLocal { local }) => {
            json!({"frames":read(local),"register":local,"expected":"0..=4294967295"})
        }
        Some(I::PlayMidi {
            key,
            velocity,
            offset_micros,
            ..
        }) => {
            json!({"key":read(key),"velocity":read(velocity),"offset_microseconds":offset_micros.and_then(read),
                "expected":"key 0..=127, velocity 1..=127, nonnegative offset"})
        }
        Some(I::ReadInputController { controller, .. }) => json!({"controller":read(controller)}),
        Some(I::WriteController { controller, value }) => {
            json!({"controller":read(controller),"value":read(value)})
        }
        Some(I::Op(Op::EngineParameter {
            address,
            local,
            write,
        })) => json!({"parameter":read(address),"group":at(address, 1),"slot":at(address, 2),
                "generic":at(address, 3),"value":read(local),"write":write}),
        Some(I::Op(Op::Store { key, local, write } | Op::SharedStore { key, local, write })) => {
            json!({"key_fields":[read(key),at(key, 1),at(key, 2),at(key, 3)],"value":read(local),"write":write})
        }
        Some(I::Op(Op::Waveform { args, .. } | Op::Midi { args, .. })) => {
            json!({"arguments":[read(args),at(args, 1),at(args, 2),at(args, 3)]})
        }
        _ => json!({}),
    };
    let mut data = json!({"schema":"ksp-fault-v1","pc":fault.pc,
        "source_slot":fault.source_slot,"source_sha256":fault.source_digest.map(|digest|
            digest.iter().map(|byte|format!("{byte:02x}")).collect::<String>()),
        "callback":fault.callback,"callback_id":fault.callback_id,"ui_id":fault.ui_id,
        "site":fault.site.map(source_site),"instruction":fault.instruction.map(|op|format!("{op:?}")),
        "operands":operands,"registers":registers,"registers_omitted":fault.registers_omitted,
        "register_encoding":"raw signed i64; real values are IEEE-754 bits",
        "callers":fault.callers.iter().flatten().copied().map(source_site).collect::<Vec<_>>(),
        "callers_omitted":fault.callers_omitted,"sample_time":fault.sample_time});
    let source = fault.source_slot.and_then(|slot| {
        instrument?
            .behaviors
            .iter()
            .enumerate()
            .find(|(index, behavior)| behavior.slot.unwrap_or(*index as u8) == slot)
            .map(|(_, behavior)| behavior.source.as_str())
    });
    if let (Some(source), Some(digest), Some(site)) = (source, fault.source_digest, fault.site)
        && sampler_ksp::source_fingerprint(source) == digest
        && let Some(excerpt) = super::script_excerpt(
            source,
            u32::from(fault.source_slot.unwrap()) + 1,
            site.line,
            Some(site.column),
        )
    {
        data["source_excerpt"] = excerpt;
    } else {
        data["source_excerpt_unavailable"] =
            json!("Exact source for this runtime generation or its source map is unavailable");
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_core::*;

    #[test]
    fn actual_vm_fault_resolves_source_and_rejects_mismatched_generation() {
        let source = "on init\n declare ui_button $b\nend on\non ui_control($b)\n wait(-1)\nend on";
        let script =
            sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        let prepared = script
            .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
            .unwrap();
        let mut limits = Limits::for_plan(&prepared, 4, 4);
        limits.behaviors = 4;
        limits.behavior_cells = prepared.behavior_local_count() * 4;
        let mut rt = Runtime::new(prepared, limits).unwrap();
        let plan = rt.active_plan();
        rt.invoke_widget(
            ControlContext {
                performance: rt.performance(0).unwrap(),
                origin: ChannelAddress {
                    protocol: Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                },
                channels: 1,
            },
            plan,
            None,
            &[WidgetEdit {
                id: sampler_ksp::derived_control_id(0, "$b"),
                index: 0,
                interaction: Default::default(),
                value: WidgetValue::Integer(1),
            }],
        )
        .unwrap();
        let mut fault = None;
        rt.flush_behavior_diagnostics(|_, _, _, _, f| {
            fault = f;
            true
        });
        let fault = fault.unwrap();
        let mut instrument = sampler_ir::Instrument {
            behaviors: vec![sampler_ir::Behavior {
                name: "synthetic".into(),
                language: sampler_ir::Language::Ksp,
                source: source.into(),
                slot: Some(0),
                state: vec![],
                requires: vec![],
            }],
            ..Default::default()
        };
        let data = fault_data(&fault, Some(&instrument));
        assert_eq!(data["site"]["line"], 5);
        assert_eq!(data["site"]["builtin"], "wait");
        assert_eq!(data["operands"]["microseconds"], -1);
        assert!(
            data["instruction"]
                .as_str()
                .unwrap()
                .starts_with("WaitMicros")
        );
        assert!(
            data["source_excerpt"]["text"]
                .as_str()
                .unwrap()
                .contains("wait(-1)")
        );
        assert_eq!(data["source_sha256"].as_str().unwrap().len(), 64);
        instrument.behaviors[0].source = "on init end on".into();
        let mismatch = fault_data(&fault, Some(&instrument));
        assert!(mismatch["source_excerpt"].is_null());
        assert!(mismatch["source_excerpt_unavailable"].is_string());
    }
}
