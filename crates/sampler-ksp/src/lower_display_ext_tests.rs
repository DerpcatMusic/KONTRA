//! Source regression seams only. Authored tests are NOT_RUN_NOT_COMPILED.
//! Inspect actual lowering because provided-value display does not check bindings.
use super::*;

fn limits() -> crate::Limits {
    crate::Limits {
        source_bytes: 65536,
        instructions: 4096,
        variables: 16,
        array_cells: 16,
    }
}

fn emitted(source: &str) -> Vec<I> {
    // Public initialization performs parsing and semantic admission of all operands.
    let initialized = crate::initialize(source, limits(), &crate::Environment::default()).unwrap();
    let hir = &initialized.hir;
    let mut unit = Unit {
        hir,
        controls: &[],
        groups: &[],
        slot: 0,
        budget: limits().instructions,
        limit: limits().instructions,
        services: Vec::new(),
        array_files: Default::default(),
        coverage: Default::default(),
        warnings: Vec::new(),
        scratch: 0,
        modules: Vec::new(),
        inspection: None,
    };
    let callback = hir
        .callbacks
        .iter()
        .find(|c| c.kind == CallbackKind::Note)
        .unwrap();
    unit.program(
        0,
        &callback.body,
        callback.span,
        Context::Note,
        callback.kind,
        None,
    )
    .unwrap();
    assert_eq!(unit.modules.len(), 1);
    unit.modules.remove(0).3.code
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operand {
    Constant(i64),
    Cell(u32),
}

fn operands(code: &[I]) -> Vec<([Operand; 4], Option<Operand>, bool)> {
    let mut registers = BTreeMap::new();
    let mut calls = Vec::new();
    for instruction in code {
        match *instruction {
            I::SetLocal { local, value } => {
                registers.insert(local, Operand::Constant(value));
            }
            I::ReadScriptCell { local, cell } => {
                registers.insert(local, Operand::Cell(cell));
            }
            I::Op(Op::EngineDisplay { address, value, .. }) => {
                if let Some(value) = value {
                    assert!(value < address || value >= address + 4, "value aliases address");
                }
                calls.push((
                    std::array::from_fn(|i| registers[&(address + i as u16)]),
                    value.map(|r| registers[&r]),
                    false,
                ));
            }
            I::Op(Op::EngineParameter {
                address,
                local,
                write,
            }) => {
                assert!(local < address || local >= address + 4, "value aliases address");
                calls.push((
                    std::array::from_fn(|i| registers[&(address + i as u16)]),
                    write.then(|| registers[&local]),
                    write,
                ));
            }
            _ => {}
        }
    }
    calls
}

#[test]
fn display_ext_constant_operands_keep_value_group_slot_and_generic_distinct() {
    for (group, slot, generic) in [(3, 5, 11), (-1, 2, 1007)] {
        let source = format!(
            "on init declare @text declare $read end on
             on note
               @text := get_engine_par_disp_ext($ENGINE_PAR_PAN,234567,{group},{slot},{generic})
               @text := get_engine_par_disp($ENGINE_PAR_PAN,{group},{slot},{generic})
               $read := get_engine_par($ENGINE_PAR_PAN,{group},{slot},{generic})
               set_engine_par($ENGINE_PAR_PAN,678901,{group},{slot},{generic})
             end on"
        );
        let code = emitted(&source);
        let calls = operands(&code);
        assert_eq!(calls.len(), 4);
        let parameter = calls[0].0[0];
        assert_eq!(parameter, Operand::Constant(i64::from(OPAQUE_BASE)));
        let before_display = code
            .iter()
            .take_while(|i| !matches!(i, I::Op(Op::EngineDisplay { .. })));
        assert_eq!(
            before_display.filter(|i| matches!(i, I::SetLocal { .. })).count(),
            5
        );
        let address = [
            parameter,
            Operand::Constant(group),
            Operand::Constant(slot),
            Operand::Constant(generic),
        ];
        assert_eq!(
            calls,
            vec![
                (address, Some(Operand::Constant(234567)), false),
                (address, None, false),
                (address, None, false),
                (address, Some(Operand::Constant(678901)), true),
            ]
        );
        // Also admit these valid ordinary/Ext calls through the complete public compiler.
        assert!(crate::compile(&source, 48000, limits(), &[]).is_ok());
    }
}

#[test]
fn display_ext_dynamic_operands_are_loaded_once_into_nonaliasing_registers() {
    let code = emitted(
        "on init declare $parameter := $ENGINE_PAR_PAN declare $value := 234567
         declare $group := 3 declare $slot := 5 declare $generic := 11 declare @text end on
         on note @text := get_engine_par_disp_ext($parameter,$value,$group,$slot,$generic) end on"
    );
    for cell in 0..5 {
        assert_eq!(
            code.iter()
                .filter(|i| matches!(i, I::ReadScriptCell { cell: c, .. } if *c == cell))
                .count(),
            1
        );
    }
    assert_eq!(
        operands(&code),
        vec![(
            [Operand::Cell(0), Operand::Cell(2), Operand::Cell(3), Operand::Cell(4)],
            Some(Operand::Cell(1)),
            false,
        )]
    );
}

#[test]
fn display_ext_semantic_admission_still_requires_all_five_integer_operands() {
    for call in [
        "get_engine_par_disp_ext($ENGINE_PAR_PAN,234567,3,5)",
        "get_engine_par_disp_ext($ENGINE_PAR_PAN,234567,3,5,11,12)",
        "get_engine_par_disp_ext($ENGINE_PAR_PAN,234567,3,5,\"bad\")",
    ] {
        let source = format!("on init declare @text end on on note @text := {call} end on");
        assert!(crate::compile(&source, 48000, limits(), &[]).is_err(), "{call}");
    }
}
