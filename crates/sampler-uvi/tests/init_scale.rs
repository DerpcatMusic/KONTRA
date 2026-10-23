//! Regression guard for UVI script-init cost at Octave-Breath scale: ~20k graph
//! nodes and ~200k `getParameter` calls must initialize well inside the load
//! ceiling. The ceiling is generous (debug builds are ~10x slower than release);
//! it catches a return to per-call allocation or linear lookups, not noise.
use sampler_uvi::script::{Config, ScriptHost};
use std::time::Instant;

#[test]
fn twenty_thousand_nodes_and_two_hundred_thousand_parameter_reads_stay_bounded() {
    const LAYERS: usize = 4;
    const KEYGROUPS: usize = 5_000;
    const READS: usize = 10;
    let mut xml = String::from("<UVI4><Program><Layers>");
    for _ in 0..LAYERS {
        xml.push_str("<Layer><Keygroups>");
        for _ in 0..KEYGROUPS {
            xml.push_str(r#"<Keygroup Gain="0.5" Pan="0.25"/>"#);
        }
        xml.push_str("</Keygroups></Layer>");
    }
    xml.push_str(&format!(
        r#"</Layers><EventProcessors><ScriptProcessor><script><![CDATA[
local total=0
for _,layer in ipairs(Program.layers) do
  for _,kg in ipairs(layer.keygroups) do
    for _=1,{READS} do total=total+kg:getParameter('Gain') end
    -- Materializes each element's definitions table, as authored UI helpers do.
    total=total+(#kg.parameterDefinitions>0 and 0 or 1)
  end
end
proof=total
]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"#
    ));
    let started = Instant::now();
    let host = ScriptHost::new(&xml, (), Config::default()).unwrap();
    let elapsed = started.elapsed();
    let expected = (LAYERS * KEYGROUPS * READS) as f64 * 0.5;
    assert_eq!(host.global_text("proof").parse::<f64>().unwrap(), expected);
    assert!(
        elapsed.as_secs_f64() < 30.0,
        "script init took {elapsed:?} for {} nodes",
        LAYERS * KEYGROUPS + LAYERS + 1
    );
}
