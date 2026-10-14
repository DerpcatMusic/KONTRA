use sampler_uvi::script::{Config, ScriptHost};

#[test]
fn opt_in_top_frame_samples_preserve_state_and_account_every_observation() {
    let xml = r#"<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[
local private_marker='AUTHORED_BODY_MUST_NOT_BE_RETAINED'
local function leaf(n)
  local sum=0
  for i=1,n do sum=sum+i end
  return sum
end
local function caller() return leaf(1000000)+1 end
proof=caller()
function onInit()
  local menu=Menu('Profile fixture',{'a/one','b/two'},1)
  menu.hierarchical=true
  initialized=private_marker
end
]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let ordinary = ScriptHost::new(xml, (), Config::default()).unwrap();
    let profiled = ScriptHost::new_profiled(xml, (), Config::default(), false).unwrap();
    assert_eq!(ordinary.interface(), profiled.interface());
    assert_eq!(ordinary.global_text("proof"), profiled.global_text("proof"));
    assert_eq!(
        ordinary.global_text("initialized"),
        profiled.global_text("initialized")
    );
    assert!(ordinary.initialization_profile().is_null());
    assert!(profiled.coverage().is_empty());
    let report = profiled.initialization_profile();
    let frames = report["lua_samples"].as_array().unwrap();
    assert!(report["sample_count"].as_u64().unwrap() > 0);
    assert!(frames.len() <= 512);
    let own: u64 = frames
        .iter()
        .map(|frame| frame["self_samples"].as_u64().unwrap())
        .sum();
    assert_eq!(
        own + report["dropped_self_samples"].as_u64().unwrap()
            + report["unavailable_top_frames"].as_u64().unwrap(),
        report["sample_count"].as_u64().unwrap()
    );
    assert!(
        frames
            .iter()
            .any(|frame| frame["self_samples"].as_u64().unwrap() > 0 && frame["kind"] == "Lua")
    );
    assert!(frames.iter().any(|frame| frame["source"] == "script"));
    for frame in frames {
        assert!(
            frame["inclusive_samples"].as_u64().unwrap() >= frame["self_samples"].as_u64().unwrap()
        );
        assert!(frame["source"].as_str().unwrap().chars().count() <= 240);
        assert!(
            frame["function"]
                .as_str()
                .is_none_or(|name| name.chars().count() <= 120)
        );
    }
    assert!(
        !report
            .to_string()
            .contains("AUTHORED_BODY_MUST_NOT_BE_RETAINED")
    );
    assert!(!report.to_string().contains("for i=1,n"));
}

#[test]
fn lookup_probe_preserves_live_getter_results_without_retaining_fields_or_values() {
    let xml = r#"<UVI4><Program><Inserts><OnePole Bypass="true" Freq="2345" PRIVATE_FIELD="PRIVATE_VALUE" NumericField="1.25"/></Inserts><EventProcessors><ScriptProcessor><script><![CDATA[
local filter=Program.inserts[1]
proof=table.concat({tostring(filter:getParameter('Bypass')),tostring(filter:getParameter('Freq')),
  tostring(filter:getParameter('Mode')),tostring(filter:getParameter('PRIVATE_FIELD')),
  tostring(filter:getParameter('NumericField')),tostring(filter:getParameter('Unrecognized'))},'|')
function onInit() setSize(320,200) Knob('Fixture',filter:getParameter('Freq'),20,20000) end
]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let ordinary = ScriptHost::new(xml, (), Config::default()).unwrap();
    let profiled = ScriptHost::new_profiled(xml, (), Config::default(), false).unwrap();
    assert_eq!(ordinary.global_text("proof"), profiled.global_text("proof"));
    assert_eq!(ordinary.interface(), profiled.interface());
    let report = profiled.initialization_profile();
    let kinds = report["parameter_lookups"]["kinds"].as_array().unwrap();
    let one_pole = kinds.iter().find(|kind| kind["kind"] == "OnePole").unwrap();
    assert_eq!(one_pole["schema"]["calls"], 7);
    assert_eq!(one_pole["schema"]["hits"], 4);
    assert_eq!(one_pole["schema"]["max_length"], 4);
    for (class, expected) in [
        ("saved_boolean", 1),
        ("saved_number", 3),
        ("default_number", 1),
        ("saved_text", 1),
        ("missing", 1),
    ] {
        assert_eq!(one_pole["value_classes"][class], expected);
    }
    assert_eq!(report["parameter_lookups"]["dropped_calls"], 0);
    assert!(!report.to_string().contains("PRIVATE_FIELD"));
    assert!(!report.to_string().contains("PRIVATE_VALUE"));
}

#[test]
fn profile_names_embedded_modules_from_the_resource_authority() {
    let mut files = sampler_uvi::script::Scripts::default();
    files.insert("Scripts/ProfileFixture.lua", "local function calculate() local n=0 for i=1,1000000 do n=n+i end return n end return calculate()".into());
    let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[proof=require('ProfileFixture')]]></script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let host = ScriptHost::new_profiled(xml, files, Config::default(), false).unwrap();
    let report = host.initialization_profile();
    assert!(
        report["lua_samples"]
            .as_array()
            .unwrap()
            .iter()
            .any(|frame| frame["source"] == "scripts/profilefixture.lua")
    );
    assert!(!report.to_string().contains("local n=0"));
    assert_eq!(report["dropped_chunk_names"], 0);
}

#[cfg(feature = "library-access")]
#[test]
fn configured_inspection_without_a_seed_keeps_legacy_default_behavior() {
    use sampler_uvi::script::Scripts;
    use sampler_uvi::{
        ScriptedOrigin, inspect_decoded_scripted_program_with_config,
        inspect_decoded_scripted_program_with_options,
    };
    let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[setSize(320,200) local k=Knob('Default',7,0,10) initialized=7]]></script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let path = std::path::Path::new("fixture.uvip");
    let (_, legacy) = inspect_decoded_scripted_program_with_options(
        xml.into(),
        Scripts::default(),
        ScriptedOrigin::Loose(path),
        false,
        false,
    )
    .unwrap();
    let (_, configured) = inspect_decoded_scripted_program_with_config(
        xml.into(),
        Scripts::default(),
        ScriptedOrigin::Loose(path),
        false,
        false,
        Config::default(),
    )
    .unwrap();
    assert_eq!(legacy.interface(), configured.interface());
    assert_eq!(legacy.control_values(), configured.control_values());
    assert_eq!(
        legacy.global_text("initialized"),
        configured.global_text("initialized")
    );
    assert!(
        legacy.initialization_profile().is_null() && configured.initialization_profile().is_null()
    );
}
