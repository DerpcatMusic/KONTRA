use super::*;

#[test]
fn identical_scalar_presentation_writes_do_not_republish_an_unchanged_face() {
    let xml = r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
        makePerformanceView()
        labels = {}
        for i=1,256 do
            local label=Label('label'..i)
            label.text='value'..i
            label.visible=true
            label.enabled=true
            label.alpha=1
            labels[i]=label
        end
        function onController(e)
            for i,label in ipairs(labels) do
                label.text='value'..i
                label.visible=true
                label.enabled=true
                label.alpha=1
            end
        end
    ]]></script></ScriptProcessor></EventProcessors></Program>"#;
    let mut host = ScriptHost::new(xml, (), Config::default()).unwrap();
    let face = host.interface();
    let before = host.ui_revision();
    for _ in 0..8 {
        host.controller(1, 64, 0);
        assert_eq!(host.interface(), face);
    }
    assert_eq!(
        host.ui_revision(),
        before,
        "8,192 identical scalar writes must not request another complete UI export"
    );
}

#[test]
fn real_scalar_changes_and_reassigned_mutable_menu_items_remain_visible() {
    let xml = r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
        label=Label('label')
        label.text='before'
        menu=Menu('menu', {'one','two'}, 1)
    ]]></script></ScriptProcessor></EventProcessors></Program>"#;
    let host = ScriptHost::new(xml, (), Config::default()).unwrap();
    let before = host.ui_revision();
    host.lua.load("label.text='after'; label.visible=false; local items=menu.items; items[1]='edited'; menu.items=items").exec().unwrap();
    assert_eq!(host.ui_revision(), before + 3);
    let face = host.interface();
    assert!(face.widgets[0].hidden);
    assert_eq!(
        host.lua
            .load("return label.text, menu.items[1]")
            .eval::<(String, String)>()
            .unwrap(),
        ("after".into(), "edited".into())
    );
    // Reassigning a mutable object may deliberately signal an in-place edit.
    let before = host.ui_revision();
    host.lua.load("menu.items=menu.items").exec().unwrap();
    assert_eq!(host.ui_revision(), before + 1);
}

#[test]
fn scalar_invalidation_preserves_types_signed_zero_and_nan_writes() {
    let host = ScriptHost::new("<Program><EventProcessors><ScriptProcessor><script>label=Label('label')</script></ScriptProcessor></EventProcessors></Program>", (), Config::default()).unwrap();
    host.lua.load("label.observation=0").exec().unwrap();
    let before = host.ui_revision();
    host.lua
        .load("label.observation=-1/math.huge")
        .exec()
        .unwrap();
    assert_eq!(
        host.ui_revision(),
        before + 1,
        "signed zero remains observable"
    );
    let before = host.ui_revision();
    host.lua
        .load("label.observation=-1/math.huge")
        .exec()
        .unwrap();
    assert_eq!(host.ui_revision(), before);
    let before = host.ui_revision();
    host.lua
        .load("label.observation=false; label.observation=0; label.observation=nil")
        .exec()
        .unwrap();
    assert_eq!(host.ui_revision(), before + 3);
    let before = host.ui_revision();
    host.lua.load("label.observation=nil").exec().unwrap();
    assert_eq!(host.ui_revision(), before);
    let before = host.ui_revision();
    host.lua
        .load("label.observation=0/0; label.observation=0/0")
        .exec()
        .unwrap();
    assert_eq!(
        host.ui_revision(),
        before + 2,
        "nonfinite writes must not hide invalid state"
    );
}

#[test]
fn presentation_data_with_authored_metamethods_keeps_its_write_side_effects() {
    let host = ScriptHost::new("<Program><EventProcessors><ScriptProcessor><script>label=Label('label')</script></ScriptProcessor></EventProcessors></Program>", (), Config::default()).unwrap();
    host.lua.load("writes=0; setmetatable(rawget(label,'__data'), {__newindex=function(t,k,v) writes=writes+1; rawset(t,k,v) end})").exec().unwrap();
    let before = host.ui_revision();
    host.lua
        .load("label.observation=nil; label.observation=nil")
        .exec()
        .unwrap();
    assert_eq!(host.lua.globals().get::<u32>("writes").unwrap(), 2);
    assert_eq!(host.ui_revision(), before + 2);
}
