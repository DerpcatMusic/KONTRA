//! Exhaustively exercise the public RE catalog, without shipping vendor data.
#![cfg(feature = "library-access")]
use sampler_uvi::script::{Config, ScriptHost};

#[test]
#[ignore = "audit: set UVI_AUDIT_CATALOG to the read-only catalog.json"]
fn every_catalog_parameter_has_typed_defaults_and_definitions() {
    let c: serde_json::Value = serde_json::from_slice(
        &std::fs::read(std::env::var("UVI_AUDIT_CATALOG").unwrap()).unwrap(),
    )
    .unwrap();
    let mut rows = 0;
    let mut defaults = 0;
    let mut definitions = 0;
    let mut ranges = 0;
    for e in c["falcon_api"].as_array().unwrap() {
        let kind = e["internal_name"].as_str().unwrap();
        let mut script = String::from(
            "defaults = 0; definitions = 0; ranges = 0; overlays = 0; local e = Program.inserts[1]; ",
        );
        for p in e["parameters"].as_array().unwrap() {
            rows += 1;
            let name = p["name"].as_str().unwrap();
            let default = p["default"].as_str().unwrap();
            let min = p["min"].as_str().unwrap();
            let max = p["max"].as_str().unwrap();
            script.push_str(&format!("if e:getParameter('{name}') == {default} then defaults = defaults + 1 end; for _, d in ipairs(e.parameterDefinitions) do if d.name == '{name}' then if type(d.id) == 'number' and d.type ~= nil then definitions = definitions + 1 end; if d.min == {min} and d.max == {max} then ranges = ranges + 1 end end end; "));
            script.push_str(&format!("e:setParameter('{name}', {default}); if e:getParameter('{name}') == {default} then overlays = overlays + 1 end; "));
        }
        let xml = format!(
            "<UVI4><Program><Inserts><{kind}/></Inserts><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
        );
        let mut h = ScriptHost::new(&xml, (), Config::default()).unwrap();
        assert!(
            h.findings().iter().all(|f| !f.feature.contains("error")),
            "catalog probe errored for {kind}"
        );
        defaults += h.global_text("defaults").parse::<usize>().unwrap();
        definitions += h.global_text("definitions").parse::<usize>().unwrap();
        ranges += h.global_text("ranges").parse::<usize>().unwrap();
        let metadata = serde_json::json!({"type":kind,"rows":e["parameters"].as_array().unwrap().len(),"correct_defaults":h.global_text("defaults").parse::<usize>().unwrap(),"typed_definitions":h.global_text("definitions").parse::<usize>().unwrap(),"correct_ranges":h.global_text("ranges").parse::<usize>().unwrap(),"overlay_readbacks":h.global_text("overlays").parse::<usize>().unwrap(),"parameter_commands":h.take_commands().iter().filter(|c| matches!(c,sampler_uvi::script::Command::Parameter {..})).count()});
        println!("UVI-CATALOG-ELEMENT {metadata}");
    }
    println!(
        "UVI-CATALOG rows={rows} correct_defaults={defaults} typed_numeric_definitions={definitions} correct_ranges={ranges}"
    );
    assert_eq!(rows, 2877);
    assert_eq!((defaults, definitions, ranges), (rows, rows, rows));
}
