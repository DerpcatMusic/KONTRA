use sampler_ui_ir::{Binding, Kind};
use sampler_uvi::script::{Config, ScriptHost, control_id};

fn host(script: &str) -> ScriptHost {
    ScriptHost::new(&format!("<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"),(),Config::default()).unwrap()
}

#[test]
fn hierarchical_menu_preserves_original_indices_and_rebuilds_live_item_paths() {
    let mut host = host(
        "local m=Menu('Routes',{'A/First','B/Middle','A/Last'},1) m.hierarchical=true m.changed=function(self) picked=self.value text=self:getText(self.value) end local b=Button('Repath') b.changed=function() m:setItem(2,'A/Moved') end",
    );
    let face = host.interface();
    assert!(face.unsupported.is_empty(), "{:?}", face.unsupported);
    let tree = face.widgets[0].menu_hierarchy.as_ref().unwrap();
    assert_eq!(
        tree[0]
            .children
            .iter()
            .map(|n| n.item_index)
            .collect::<Vec<_>>(),
        [Some(0), Some(2)]
    );
    let Binding::Control(control) = face.widgets[0].binding else {
        panic!("control missing")
    };
    host.set_control(control, 3.).unwrap();
    assert_eq!(host.global_text("picked"), "3");
    assert_eq!(host.global_text("text"), "A/Last");
    host.set_control(control_id(2, 0), 1.).unwrap();
    let changed = host.interface();
    let tree = changed.widgets[0].menu_hierarchy.as_ref().unwrap();
    assert_eq!(tree.len(), 1);
    assert_eq!(
        tree[0]
            .children
            .iter()
            .map(|n| n.item_index)
            .collect::<Vec<_>>(),
        [Some(0), Some(1), Some(2)]
    );
    assert!(matches!(&changed.widgets[0].kind,Kind::Menu {items} if items[2].value==3));
}

#[test]
fn false_policy_is_flat_and_unverified_or_ambiguous_cases_stay_diagnosed() {
    let flat = host("local m=Menu('Flat',{'a/b','a'},1) m.hierarchical=false").interface();
    assert!(flat.widgets[0].menu_hierarchy.is_none());
    assert!(flat.unsupported.is_empty());
    for body in [
        "local m=Menu('Prefix',{'a','a/b'},1) m.hierarchical=true",
        "local m=Menu('Duplicate',{'a/b','a/b'},1) m.hierarchical=true",
        "local m=Menu('Empty',{'a//b'},1) m.hierarchical=true",
        "local m=Menu('Type',{'a/b'},1) m.hierarchical='true'",
        "local m=MultiStateButton('Cycle',{'a/b'},1) m.hierarchical=true",
    ] {
        let face = host(body).interface();
        assert!(face.widgets[0].menu_hierarchy.is_none());
        assert_eq!(
            face.unsupported
                .iter()
                .filter(|u| u.feature.ends_with(".hierarchical"))
                .count(),
            1
        );
    }
}
