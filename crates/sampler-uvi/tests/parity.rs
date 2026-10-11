use sampler_parity::{Status, TokenClass};
use sampler_uvi::{
    parity_reports,
    script::{Config, ScriptHost},
};

fn xml(source: &str, modules: &str) -> String {
    format!(
        r#"<UVI4><Program Name="P"><Inserts>{modules}</Inserts><EventProcessors><ScriptProcessor Name="S"><script><![CDATA[{source}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"#
    )
}

#[test]
fn reports_calls_parameters_and_highlighting_without_source() {
    let source = "--[==[ long\ncomment ]==]\nlocal text = [=[private payload]=]\nlocal matrix = Program.inserts[1]\nmatrix:setParameter('Gain_3_1', 0.5)\nmatrix:getParameter('Gain_1_1')\nwait(1)\nloadSample('private path')\nunknownApi()\nfunction onNote(e) end\nlocal n=0x1p+2 + 2.5e-3 + 1..2";
    let reports = parity_reports(&xml(source, "<GainMatrix/>"));
    let report = &reports[0];
    let at = |name: &str| report.constructs.iter().find(|c| c.name == name).unwrap();
    assert_eq!(at("wait").status, Status::Implemented);
    assert_eq!(at("setParameter GainMatrix.Gain_3_1").status, Status::Stub);
    assert_eq!(at("loadSample").status, Status::Unsupported);
    assert_eq!(at("unknownApi").status, Status::Unsupported);
    assert_eq!(
        at("getParameter GainMatrix.Gain_1_1").kind,
        sampler_parity::ConstructKind::LuaParameter
    );
    assert!(
        report
            .lowered
            .iter()
            .flat_map(|l| &l.ops)
            .any(|s| s.contains("unbound (stereo core)"))
    );
    let token = |text: &str| {
        report
            .tokens
            .iter()
            .find(|t| &source[t.span.start as usize..t.span.end as usize] == text)
            .unwrap()
    };
    assert_eq!(
        token("--[==[ long\ncomment ]==]").class,
        TokenClass::Comment
    );
    assert_eq!(token("[=[private payload]=]").class, TokenClass::String);
    assert_eq!(token("onNote").class, TokenClass::Callback);
    assert_eq!(token("Program").class, TokenClass::Builtin);
    assert_eq!(token("0x1p+2").class, TokenClass::Number);
    assert_eq!(token("2.5e-3").class, TokenClass::Number);
    assert_eq!(token("matrix").span.line, 4);
    let json = serde_json::to_string(report).unwrap();
    assert!(!json.contains("private payload") && !json.contains("private path"));
    assert!(
        report
            .id
            .ends_with("/EventProcessors[0]/ScriptProcessor[0]")
    );
}

#[test]
fn concrete_bindings_and_dynamic_targets_are_honest() {
    let source = "local filter=Program.inserts[1]\nfilter:setParameter('Freq',2000)\nfilter:setParameter('Bypass',true)\nProgram:setParameter('Gain',0.5)\nProgram.inserts[2]:setParameter('Freq',900)\nProgram.inserts[index]:setParameter('Freq',800)";
    let reports = parity_reports(&xml(source, "<OnePole/><OnePole KeyTracking='1'/>"));
    let writes: Vec<_> = reports[0]
        .constructs
        .iter()
        .filter(|c| c.name.starts_with("setParameter"))
        .collect();
    assert_eq!(writes.len(), 5);
    assert_eq!(
        writes.iter().map(|c| c.status).collect::<Vec<_>>(),
        vec![
            Status::Approximate,
            Status::Approximate,
            Status::Approximate,
            Status::Stub,
            Status::Stub
        ]
    );
    assert!(reports[0].lowered[0].ops[0].contains("core engine-parameter write"));
    assert!(reports[0].lowered[1].ops[0].contains("slot-bypass"));
    assert!(writes[4].name.contains("dynamic module"));
}

#[test]
fn line_coverage_is_real_cumulative_and_off_by_default() {
    let source = "local total=0\nfunction onNote(e)\n  total=total+1\n  for i=1,3 do\n    total=total+1\n  end\n  error('focused runtime fault')\nend";
    let program = xml(source, "");
    let mut ordinary = ScriptHost::new(&program, (), Config::default()).unwrap();
    ordinary.note_on(1, 60, 100, 0);
    assert!(ordinary.coverage().is_empty());
    let mut host = ScriptHost::new_inspected(&program, (), Config::default()).unwrap();
    host.note_on(1, 60, 100, 0);
    let report = host.coverage().remove(0);
    let loop_line = source.find("    total").unwrap() as u32;
    assert_eq!(
        report
            .hits
            .iter()
            .find(|(offset, _)| *offset == loop_line)
            .map(|(_, hits)| *hits),
        Some(3)
    );
    assert_eq!(report.faults.len(), 1);
    assert_eq!(report.faults[0].0, source.find("  error").unwrap() as u32);
    assert!(report.faults[0].1.contains("focused runtime fault"));
    host.note_on(2, 62, 100, 0);
    let next = host.coverage().remove(0);
    assert_eq!(
        next.hits
            .iter()
            .find(|(offset, _)| *offset == loop_line)
            .map(|(_, hits)| *hits),
        Some(6)
    );
    assert_eq!(next.faults.len(), 2);
}

#[test]
fn ledger_matches_generated_table() {
    assert!(
        sampler_uvi::parity::ledger::markdown()
            == include_str!("../../../docs/architecture-v2/UVI_PARITY_LEDGER.md"),
        "regenerate UVI_PARITY_LEDGER.md from parity_summary --ledger"
    );
}

#[test]
fn aliases_do_not_cross_local_or_callback_parameter_scopes() {
    let source = "local filter=Program.inserts[1]\nfunction helper(filter)\n filter:setParameter('Freq',10)\nend\ndo\n local filter=Program.inserts[2]\n filter:setParameter('Freq',20)\nend\nfilter:setParameter('Freq',30)\nlocal object={}\nfunction object:custom() end\nobject:custom()\nmath.random()";
    let report = parity_reports(&xml(source, "<OnePole/><OnePole KeyTracking='1'/>")).remove(0);
    let writes: Vec<_> = report
        .constructs
        .iter()
        .filter(|c| c.name.starts_with("setParameter"))
        .collect();
    assert_eq!(
        writes.iter().map(|c| c.status).collect::<Vec<_>>(),
        vec![Status::Stub, Status::Stub, Status::Approximate]
    );
    assert!(
        !report
            .constructs
            .iter()
            .any(|c| c.name == "custom" || c.name == "random")
    );
}

#[test]
fn embedded_modules_share_report_and_coverage_ids() {
    let mut files = sampler_uvi::script::Scripts::default();
    files.insert(
        "Scripts/helpers.lua",
        "setBackground('panel.png')\nfunction onNote(e)\n  local n=1\nend".into(),
    );
    let program = xml("require('helpers')", "");
    let reports = sampler_uvi::parity::parity_reports_with_files(&program, &files);
    assert_eq!(reports.len(), 2);
    let mut host = ScriptHost::new_inspected(&program, files, Config::default()).unwrap();
    host.note_on(1, 60, 100, 0);
    assert!(
        host.interface()
            .assets
            .iter()
            .any(|a| a.path == "/scripts/panel.png")
    );
    let coverage = host.coverage();
    assert!(
        coverage
            .iter()
            .any(|c| c.script_id == "uvi:module:helpers" && !c.hits.is_empty())
    );
    assert!(reports.iter().any(|r| r.id == "uvi:module:helpers"));
}

#[test]
fn a_lane_without_a_translated_slot_is_a_stub() {
    let program = xml(
        "Program.inserts[1]:setParameter('Bypass',true)",
        "<TrackDelay DelayTime='0'/>",
    );
    let report = parity_reports(&program).remove(0);
    assert_eq!(
        report
            .constructs
            .iter()
            .find(|c| c.name == "setParameter TrackDelay.Bypass")
            .unwrap()
            .status,
        Status::Stub
    );
}
