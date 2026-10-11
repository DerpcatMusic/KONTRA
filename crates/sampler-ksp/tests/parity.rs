use sampler_ksp::{Environment, Limits, RuntimeCoverage, parity_report};
use sampler_parity::{Status, TokenClass};

#[test]
fn report_classifies_implemented_stub_unsupported_and_tokens_without_source() {
    let source = "{private comment}\non init\n declare $x := abs(-7)\n set_global_key(1)\n nope(\"private literal\")\nend on";
    let r = parity_report(source, "ksp:slot0", "test");
    let find = |name| r.constructs.iter().find(|c| c.name == name).unwrap();
    assert_eq!(find("abs").status, Status::Implemented);
    assert_eq!(find("set_global_key").status, Status::Stub);
    assert_eq!(find("nope").status, Status::Unsupported);
    assert_eq!(find("nope").span.line, 5);
    let token = |text| {
        r.tokens
            .iter()
            .find(|t| &source[t.span.start as usize..t.span.end as usize] == text)
            .unwrap()
            .class
    };
    assert_eq!(token("{private comment}"), TokenClass::Comment);
    assert_eq!(token("on"), TokenClass::Keyword);
    assert_eq!(token("init"), TokenClass::Callback);
    assert_eq!(token("abs"), TokenClass::Builtin);
    assert_eq!(token("$x"), TokenClass::Variable);
    assert_eq!(token("7"), TokenClass::Number);
    assert_eq!(token("\"private literal\""), TokenClass::String);
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("private literal"));
    assert!(!json.contains("private comment"));
}

#[test]
fn lowering_reports_statements_and_context_stubs_without_values() {
    let source = "on init declare $x end on\non note\n $x := abs(-7)\n set_script_title(\"private title\")\nend on";
    let r = parity_report(source, "ksp:slot0", "test");
    assert_eq!(
        r.constructs
            .iter()
            .find(|c| c.name == "set_script_title")
            .unwrap()
            .status,
        Status::Stub
    );
    assert!(
        r.lowered
            .iter()
            .any(|l| l.span.line == 3 && !l.ops.is_empty())
    );
    assert!(!serde_json::to_string(&r).unwrap().contains("private title"));
}

#[test]
fn ledger_and_doc_are_in_sync() {
    assert_eq!(
        sampler_ksp::parity::ledger_markdown(),
        include_str!("../../../docs/architecture-v2/KSP_PARITY_LEDGER.md")
    );
}

#[test]
fn evaluator_coverage_is_opt_in_and_records_hits_and_fault_spans() {
    let coverage = RuntimeCoverage::new("ksp:slot0");
    let source = "on init declare $x := 0 while $x < 2 inc($x) end while end on";
    let env = Environment {
        coverage: Some(coverage.clone()),
        ..Default::default()
    };
    sampler_ksp::initialize(source, Limits::LIBRARY, &env).unwrap();
    assert_eq!(coverage.snapshot().script_id, "ksp:slot0");
    let at = source.find("inc").unwrap() as u32;
    assert!(coverage.snapshot().hits.contains(&(at, 2))); // a statement/call sharing one span counts once
    let bad = "on init declare $x := 1 end on";
    let no_fuel = Environment {
        evaluation_budget: Some(0),
        ..env
    };
    assert!(sampler_ksp::initialize(bad, Limits::LIBRARY, &no_fuel).is_err());
    assert!(!coverage.snapshot().faults.is_empty());
    sampler_ksp::initialize(source, Limits::LIBRARY, &Environment::default()).unwrap();
}

#[test]
fn empty_slot_has_no_gap_and_user_commands_use_resolved_functions() {
    assert!(
        parity_report("", "ksp:slot0", "empty")
            .constructs
            .is_empty()
    );
    assert!(
        parity_report(" \n\t", "ksp:slot0", "empty")
            .constructs
            .is_empty()
    );
    let source = "on init declare const $c := 2 end on function f message($c) end function on note call f end on";
    let r = parity_report(source, "ksp:slot0", "test");
    assert!(r.constructs.iter().any(|c| c.name == "f"
        && c.kind == sampler_parity::ConstructKind::Command
        && c.status == Status::Implemented));
}

#[test]
fn constants_and_init_function_statements_keep_their_source_spans() {
    let source = "on init declare const $c := 2 call f end on function f message($c) end function";
    let r = parity_report(source, "ksp:slot0", "test");
    assert!(
        r.tokens
            .iter()
            .filter(|t| &source[t.span.start as usize..t.span.end as usize] == "$c")
            .all(|t| t.class == TokenClass::Constant)
    );
    assert!(
        r.lowered
            .iter()
            .any(|l| l.span.start as usize == source.find("message").unwrap())
    );
}
