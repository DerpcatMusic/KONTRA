use sampler_uvi::script::{Config, ScriptHost};

#[test]
fn step_envelope_reads_saved_indexed_levels_and_native_defaults() {
    let xml = r#"<UVI4><Program><ControlSignalSources>
      <StepEnvelope Levels='0,25 0.75 -0.5' NumSteps='2'/><StepEnvelope/>
      </ControlSignalSources><EventProcessors><ScriptProcessor><script>
      local saved=Program.modulations[1]
      assert(saved:getParameter('Level0')==0.25)
      assert(saved:getParameter('Level1')==0.75)
      assert(saved:getParameter('Level2')==-0.5)
      local defaults=Program.modulations[2]
      for i=0,127 do
        local expected=(i==0 or i==4 or i==8 or i==10 or i==12) and 1 or 0
        assert(defaults:getParameter('Level'..i)==expected,'default '..i)
      end
      completed=true
      </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let host = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert!(host.fault_counts().init.is_empty(), "{:?}", host.fault_counts());
    assert_eq!(host.global_text("completed"), "true");
    assert!(host.findings().is_empty(), "{:?}", host.findings());
}
