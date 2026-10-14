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
