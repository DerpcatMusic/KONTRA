use sampler_uvi::script::{Config, ScriptHost};

fn assert_loaded(script: &str) {
    let xml = format!(
        "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    );
    let host = ScriptHost::new(&xml, (), Config::default()).unwrap();
    assert!(host.findings().is_empty(), "{:?}", host.findings());
    assert_eq!(host.global_text("loaded"), "true");
}

#[test]
fn v1_class_instances_keep_authored_fields_methods_and_userdata_identity() {
    assert_loaded(
        r#"
      local extend=class'Authored'
      assert(type(extend)=='function' and type(Authored)=='userdata')
      assert(Authored==Authored)
      Authored.static=7
      function Authored:__init(x) self.x=x end
      function Authored:sum() return self.x+Authored.static end
      local a=Authored(4); a.y=9
      assert(type(a)=='userdata' and a.x==4 and a.y==9 and a:sum()==11)
      assert(a.static==7 and a.missing==nil and a==a)
      assert(a~=Authored)
      assert(not pcall(function() return a==Authored(4) end))
      assert(not pcall(function() return tostring(a) end))
      loaded=true
    "#,
    );
}

#[test]
fn v1_class_inheritance_copies_members_and_requires_its_own_initializer() {
    assert_loaded(
        r#"
      class'Base'
      Base.static=7
      function Base:__init(x) self.x=x end
      function Base:sum() return self.x+Base.static end
      assert(class'Child'(Base)==nil)
      assert(not pcall(function() Child(3) end))
      function Child:__init(x) self.x=x*2 end
      local child=Child(5)
      assert(child.x==10 and child:sum()==17 and child.static==7)
      Base.static=10; function Base:sum() return 1000 end
      assert(child:sum()==20 and child.static==7)
      loaded=true
    "#,
    );
}

#[test]
fn v1_class_rejects_invalid_names_and_non_class_bases() {
    assert_loaded(
        r#"
      for _,name in ipairs({'',string.rep('a',257),'bad\0name'}) do
        assert(not pcall(class,name))
      end
      local extend=class'NeedsInit'
      assert(not pcall(extend,{}))
      require('uvi.AsyncUpdater')
      assert(not pcall(extend,AsyncUpdater))
      assert(not pcall(function() NeedsInit() end))
      loaded=true
    "#,
    );
}
