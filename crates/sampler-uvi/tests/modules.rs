use sampler_uvi::script::{Config, ScriptHost, Scripts};

fn host(script: &str, modules: Scripts) -> ScriptHost {
    ScriptHost::new(
        &format!("<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"),
        modules,
        Config::default(),
    ).unwrap()
}

#[test]
fn v1_chord_rec_is_real_cached_module_with_chroma_and_inversions() {
    let h = host(
        r#"
      assert(require('uvi.ChordRec')==true)
      local module=ChordRec; assert(require('uvi.ChordRec')==true and module==ChordRec)
      local root,kind,bass=ChordRec.chordKind({60,64,67})
      assert(root==0 and kind=='M' and bass==0)
      root,kind,bass=ChordRec.chordKind({64,67,72})
      assert(root==0 and kind=='M' and bass==4)
      root,kind,bass=ChordRec.chordKind({60,63,67})
      assert(root==0 and kind=='m' and bass==0)
      assert(ChordRec.getChromaString(ChordRec.getChroma(60,{48,60,64,67,72}))=='100010010000')
      assert(ChordRec.chordKind({60,61,62})==nil)
      loaded=true
    "#,
        Scripts::default(),
    );
    assert!(h.findings().is_empty(), "{:?}", h.findings());
    assert_eq!(h.global_text("loaded"), "true");
}

#[test]
fn v1_embedded_uvi_module_overrides_host_module_and_false_is_not_cached() {
    let mut modules = Scripts::default();
    modules.insert(
        "uvi.ChordRec.lua",
        "calls=(calls or 0)+1; return false".into(),
    );
    let h = host(
        "assert(require('uvi.ChordRec')==false); assert(require('uvi.ChordRec')==false); assert(calls==2); loaded=true",
        modules,
    );
    assert!(h.findings().is_empty(), "{:?}", h.findings());
    assert_eq!(h.global_text("loaded"), "true");
}

#[test]
fn v1_module_cycles_fail_at_require_and_release_loading_marker() {
    let mut modules = Scripts::default();
    modules.insert(
        "loop.lua",
        "calls=(calls or 0)+1; return require('loop')".into(),
    );
    let h = host(
        r#"
      for i=1,2 do
        local ok,err=pcall(require,'loop')
        assert(not ok and string.find(tostring(err),'cycle'))
      end
      assert(calls==2); loaded=true
    "#,
        modules,
    );
    assert!(h.findings().is_empty(), "{:?}", h.findings());
    assert_eq!(h.global_text("loaded"), "true");
}

#[test]
fn v1_require_rejects_invalid_names_and_unknown_uvi_modules() {
    let h = host(
        r#"
      for _,name in ipairs({'',string.rep('a',257),'bad\0name',{}}) do
        assert(not pcall(require,name))
      end
      local ok,err=pcall(require,'uvi.NotAnAuthoredModule')
      assert(not ok and string.find(tostring(err),'not found'))
      loaded=true
    "#,
        Scripts::default(),
    );
    assert!(h.findings().is_empty(), "{:?}", h.findings());
    assert_eq!(h.global_text("loaded"), "true");
}

#[test]
fn v1_async_updater_is_real_userdata_and_coalesces_on_the_host_clock() {
    let mut h = host(
        r#"
      assert(require('uvi.AsyncUpdater')==true and type(AsyncUpdater)=='userdata')
      local factory=AsyncUpdater; require('uvi.AsyncUpdater'); assert(factory==AsyncUpdater)
      calls=0
      local updater
      updater=AsyncUpdater(function(...) assert(select('#',...)==0); calls=calls+1; updater:trigger(99) end)
      assert(type(updater)=='userdata' and type(updater.trigger)=='function')
      assert(updater.pending==nil and updater.cancel==nil)
      assert(not pcall(function() updater.callback=42 end))
      assert(not pcall(function() updater.unknown=function() end end))
      function onController(e) updater:trigger(20) end
      loaded=true
    "#,
        Scripts::default(),
    );
    assert!(h.findings().is_empty(), "{:?}", h.findings());
    assert_eq!(h.global_text("loaded"), "true");
    h.controller(1, 64, 0);
    h.controller(1, 65, 0);
    h.advance(19.);
    assert_eq!(h.global_text("calls"), "0");
    h.advance(20.);
    assert_eq!(h.global_text("calls"), "1");
    h.controller(1, 66, 0);
    h.advance(40.);
    assert_eq!(h.global_text("calls"), "2");
    assert!(h.findings().is_empty(), "{:?}", h.findings());
}

#[test]
fn v1_module_aliases_are_unambiguous_and_exact_members_win() {
    let mut modules = Scripts::default();
    let source = "calls=(calls or 0)+1; return {name=...,count=calls}";
    modules.insert("Scripts/MIDI Scripts/_Folder/Counter.lua", source.into());
    modules.insert("ApprovedAlias/_Folder/Counter.lua", source.into());
    modules.insert("First/_Conflict/Main.lua", "return 'first'".into());
    modules.insert("Second/_Conflict/Main.lua", "return 'second'".into());
    modules.insert("_Conflict/Main.lua", "return 'exact'".into());
    modules.insert(
        "42.lua",
        "numericCalls=(numericCalls or 0)+1; return {}".into(),
    );
    let h = host(
        r#"
      local module=require('_Folder.Counter')
      assert(module==require('_Folder.Counter') and module.count==1 and module.name=='_Folder.Counter')
      assert(require('_Conflict/Main')=='exact')
      assert(not pcall(require,'_Conflict.Main'))
      assert(require(42)==require('42') and numericCalls==1)
      loaded=true
    "#,
        modules,
    );
    assert!(h.findings().is_empty(), "{:?}", h.findings());
    assert_eq!(h.global_text("loaded"), "true");
}
