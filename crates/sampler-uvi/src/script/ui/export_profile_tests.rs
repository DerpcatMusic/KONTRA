use super::*;
use crate::script::{Config, diagnostics::OwnerAudit};
use std::sync::Arc;

fn host() -> ScriptHost {
    let script = r#"
      getter_calls=0; meta_calls=0
      local k=ParamKnob(Program,'Gain')
      local base=Program.getParameter
      Program.getParameter=function(self,name)
        getter_calls=getter_calls+1
        local d=rawget(k,'__data'); rawset(d,'min',0.125); rawset(d,'tooltip','live after getter')
        return base(self,name)
      end
      for i=1,256 do
        local label=Label('Label'..i)
        setmetatable(rawget(label,'__data'),{__index=function(t,key)
          if key=='fontSize' then meta_calls=meta_calls+1; return 12 end
        end})
      end
    "#;
    let xml = format!(
        "<UVI4><Program Gain='0.5'><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    );
    ScriptHost::new(&xml, (), Config::default()).unwrap()
}

#[test]
fn export_profile_sampling_rotates_without_extra_getters_or_stale_raw_mutations() {
    let ordinary = host();
    let observed = host();
    observed.install_owner_audit(Some(Arc::new(OwnerAudit::default())));
    for expected_samples in [3, 2] {
        let before = observed.owner_audit().unwrap().snapshot();
        let expected = ordinary.interface();
        let actual = observed.interface();
        assert_eq!(actual, expected);
        assert_eq!(actual.widgets[0].tooltip, "live after getter");
        assert!(matches!(&actual.widgets[0].kind,ui::Kind::Knob{range,..} if range.min==0.125));
        assert_eq!(
            ordinary.global_text("getter_calls"),
            observed.global_text("getter_calls")
        );
        assert_eq!(
            ordinary.global_text("meta_calls"),
            observed.global_text("meta_calls")
        );
        assert_eq!(ordinary.findings(), observed.findings());
        let after = observed.owner_audit().unwrap().snapshot();
        for name in [
            "export_widget_sample",
            "export_value_sample",
            "export_inventory_sample",
        ] {
            let calls = |snapshot: &crate::script::diagnostics::OwnerAuditSnapshot| {
                snapshot
                    .stages
                    .iter()
                    .find(|stage| stage.name == name)
                    .unwrap()
                    .calls
            };
            assert_eq!(calls(&after) - calls(&before), expected_samples);
        }
    }
}
