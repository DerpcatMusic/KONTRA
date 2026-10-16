//! Authored source/HIR contracts for the finite Doctor correction.
use sampler_core::{Limits as CoreLimits, Outcome, Prepared, Runtime, ScriptInstanceId};
use sampler_ksp::{EntryKind, Environment, Kind, Limits};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

#[test]
fn get_ui_id_rejects_non_ui_declared_variables_at_the_operand_span() {
    for callback in ["init", "persistence_changed"] {
        let source = if callback == "init" {
            "on init\n declare $plain\n message(get_ui_id($plain))\nend on".to_owned()
        } else {
            "on init\n declare $plain\nend on\non persistence_changed\n message(get_ui_id($plain))\nend on".to_owned()
        };
        for error in [
            sampler_ksp::initialize(&source, Limits::LIBRARY, &Environment::default())
                .err()
                .unwrap(),
            sampler_ksp::compile(&source, 48000, Limits::LIBRARY, &[])
                .err()
                .unwrap(),
        ] {
            assert_eq!(error.kind, Kind::Error);
            assert_eq!(error.offset, source.rfind("$plain").unwrap());
            assert_eq!(error.message, "get_ui_id requires a UI control");
        }
    }
}

#[test]
fn consuming_branch_handles_keep_nested_loop_select_and_fallthrough_targets() {
    let source = r#"on init
      declare $result declare $i declare %a[2] := (7,9)
    end on
    on pgs_changed
      $i := 0
      while ($i < 4)
        select ($i)
          case 0
            inc(%a[0])
          case 1 to 2
            if ($i = 1) $result := $result + 10 else $result := $result + 100 end if
          case 3
            dec(%a[1])
        end select
        inc($i)
      end while
      $result := $result + %a[0] + %a[1]
    end on"#;
    let script = sampler_ksp::compile(source, 48000, Limits::LIBRARY, &[]).unwrap();
    let program = script
        .entries()
        .iter()
        .find(|entry| entry.kind == EntryKind::PgsChanged)
        .unwrap()
        .program;
    let prepared = script
        .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
        .unwrap();
    let mut limits = CoreLimits::for_plan(&prepared, 4, 0);
    limits.behavior_fuel = 2048;
    let mut rt = Runtime::new(prepared, limits).unwrap();
    let plan = rt.active_plan();
    // Persistence callbacks are already evaluated during initialization. Use
    // an unfired plan callback so runtime starts from authored cells.
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(0));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 2), Ok(7));
    assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 3), Ok(9));
    support::without_heap(|| {
        let id = rt.start_plan_behavior(plan, program).unwrap();
        assert_eq!(rt.behavior_outcome(id), Ok(Some(Outcome::Finished)));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(126));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(4));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 2), Ok(8));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 3), Ok(8));
        let repeated = rt.start_plan_behavior(plan, program).unwrap();
        assert_eq!(rt.behavior_outcome(repeated), Ok(Some(Outcome::Finished)));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 0), Ok(252));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 1), Ok(4));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 2), Ok(9));
        assert_eq!(rt.script_cell(plan, ScriptInstanceId(0), 3), Ok(7));
    });
}
