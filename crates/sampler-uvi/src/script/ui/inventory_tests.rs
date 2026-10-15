use super::*;
use crate::script::Config;

#[path = "inventory_fixture.rs"]
mod fixture;

const DOCUMENTED: &[&str] = &[
    "textColourOn",
    "textColourOff",
    "outlineColour",
    "arrowColour",
    "0dBColour",
    "fillStyle",
    "displayAsMono",
    "showPopupDisplay",
    "displayArrow",
    "hierarchical",
    "popupBackgroundColour",
    "popupTextColour",
    "popupArrowColour",
    "popupOutlineColour",
    "popupTextColourHighlight",
    "popupBackgroundColourHighlight",
    "showScrollBar",
    "scrollBarColour",
    "waveColour",
    "cursorColour",
    "gridColour",
];

fn host(script: &str) -> ScriptHost {
    host_config(script, Config::default())
}

fn host_config(script: &str, config: Config) -> ScriptHost {
    let xml = format!(
        "<UVI4><Program Gain='0.5'><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    );
    ScriptHost::new(&xml, (), config).unwrap()
}

#[test]
fn classifier_recognizes_exactly_the_documented_property_names() {
    for key in DOCUMENTED {
        assert!(documented_property(key));
        for other in [
            key.to_lowercase(),
            format!("_{key}"),
            format!("{key}x"),
            format!("{key}\0"),
        ] {
            if !DOCUMENTED.contains(&other.as_str()) {
                assert!(!documented_property(&other), "{other:?}");
            }
        }
    }
    for key in [
        "kind",
        "textColour",
        "visible",
        "changed",
        "",
        "popupHighlightedTextColour",
        "日本語",
        "privateData",
    ] {
        assert!(!documented_property(key), "{key:?}");
    }
}

#[test]
fn authored_render_bridge_roundtrips_complete_labels_and_rejects_extra_fields() {
    let h = host(
        "setSize(720,480); local w=Label('Bridge'); w.fontSize=12; w.text='Authored'; w.fillStyle='solid'",
    );
    let face = h.interface();
    let mut value = fixture::encode(&face);
    assert_eq!(fixture::decode(&value).unwrap(), face);
    value["widgets"][0]["unrepresented_field"] = serde_json::Value::Null;
    assert!(fixture::decode(&value).is_err());
}

#[test]
fn property_inventory_preserves_complete_models_for_kinds_and_value_types() {
    let keys = serde_json::to_string(DOCUMENTED).unwrap();
    let keys = keys.trim_start_matches('[').trim_end_matches(']');
    let script = format!(
        r#"
      local keys={{{keys}}}
      local values={{true,false,1,'#ffffff','invalid','solid',{{}},function() end}}
      for _,kind in ipairs({{'Label','Button','Menu','MultiStateButton','Knob','Table'}}) do
        for n,value in ipairs(values) do
          local w=_G[kind](kind..n)
          local d=rawget(w,'__data')
          for _,key in ipairs(keys) do rawset(d,key,value) end
          rawset(d,'privateTable',{{nested=42}})
          rawset(d,'privateFunction',function() error('must not execute') end)
          if kind=='Menu' or kind=='MultiStateButton' then d.items={{'Group/One','Group/Two'}} end
        end
      end
    "#
    );
    let h = host(&script);
    let reference = h.interface_classified::<0>();
    let candidate = h.interface_classified::<2>();
    assert_eq!(candidate, reference);
    assert_eq!(h.interface_classified::<3>(), reference);
    assert_eq!(
        h.interface(),
        reference,
        "production preserves the canonical model"
    );
    assert_eq!(
        h.interface_classified::<1>(),
        reference,
        "classifier experiment remains test-only"
    );
    assert_eq!(candidate.widgets.len(), 48);
    assert!(!candidate.unsupported.is_empty());
    assert!(
        !candidate
            .unsupported
            .iter()
            .any(|row| row.feature.contains("private"))
    );
    assert_eq!(
        ScriptHost::control_values_from(&candidate),
        ScriptHost::control_values_from(&reference)
    );
    assert!(h.findings().is_empty());
}

#[test]
fn inventory_preserves_live_getter_order_raw_nested_edits_and_commands() {
    let script = r#"
      trace=''; calls=0; pair_calls=0
      local k=ParamKnob(Program,'Gain')
      local b=Button('Button')
      local menu=Menu('Menu',{'Group/One','Group/Two'})
      menu.hierarchical=true
      local cells=Table('Cells',3,0,0,1)
      local label=Label('Label')
      local kd=rawget(k,'__data')
      local ld=rawget(label,'__data')
      local get=Program.getParameter
      Program.getParameter=function(self,key)
        trace=trace..'value;'; calls=calls+1
        rawset(kd,'min',0.125); rawset(kd,'tooltip','getter mutation')
        rawset(ld,'text','value '..calls)
        return get(self,key)
      end
      setmetatable(ld,{
        __index=function(_,key)
          trace=trace..key..';'
          if key=='fontSize' then
            postEvent{type=Event.Controller,controller=1,value=17}
            return 12
          end
        end,
        __pairs=function() pair_calls=pair_calls+1; error('raw inventory required') end
      })
      function onNote(e)
        rawset(rawget(b,'__data'),'textColourOff',e.note==60 and '#ffffff' or false)
        rawget(menu,'__data').items[1]=e.note==60 and 'Other/One' or 'Group/One'
        rawget(cells,'__data').values[2]=e.note/127
        rawset(ld,'fillStyle',e.note==60 and 'solid' or 'gloss')
      end
      function onSave() return {trace=trace,calls=calls,pair_calls=pair_calls} end
    "#;
    let mut reference = host(script);
    let mut candidate = host(script);
    for note in [None, Some(60), Some(61)] {
        if let Some(note) = note {
            reference.note_on(1, note, 64, 0);
            candidate.note_on(1, note, 64, 0);
        }
        assert_eq!(
            candidate.interface_classified::<2>(),
            reference.interface_classified::<0>()
        );
        assert_eq!(
            candidate.global_text("trace"),
            reference.global_text("trace")
        );
        assert_eq!(candidate.global_text("pair_calls"), "0");
        assert_eq!(candidate.save_ui_state(), reference.save_ui_state());
        assert_eq!(candidate.take_commands(), reference.take_commands());
        assert_eq!(candidate.findings(), reference.findings());
    }
}

#[test]
fn iterator_preserves_work_budget_faults_and_restores_callback_allowance() {
    let script = r#"
      local w=Label('Budget')
      setmetatable(rawget(w,'__data'),{__index=function(_,key)
        if key=='fontSize' then
          local n=0; for i=1,10000 do n=n+i end
          return 12+n*0
        end
      end})
    "#;
    let config = Config {
        load_work: 1024,
        ..Default::default()
    };
    let reference = host_config(script, config);
    let candidate = host_config(script, config);
    reference.shared.remaining_work.set(7);
    candidate.shared.remaining_work.set(7);
    assert_eq!(
        candidate.interface_classified::<2>(),
        reference.interface_classified::<0>()
    );
    assert_eq!(candidate.findings(), reference.findings());
    assert!(
        candidate
            .findings()
            .iter()
            .any(|row| row.value.contains("budget"))
    );
    assert_eq!(
        candidate.shared.vm_checkpoints.get(),
        reference.shared.vm_checkpoints.get()
    );
    assert_eq!(candidate.shared.remaining_work.get(), 7);
    assert_eq!(reference.shared.remaining_work.get(), 7);
}

#[test]
fn raw_iterator_reentry_and_in_place_mutation_preserve_sequence_and_model() {
    let script = r#"
      visits=0; trace=''; meta_calls=0
      local w=Label('Reentrant')
      mutation=rawget(w,'__data')
      mutation.fillStyle='solid'; mutation.gridColour='#ffffff'
      mutation.private1={1,2,3}; mutation.private2=function() end
      setmetatable(mutation,{__index=function(_,key)
        meta_calls=meta_calls+1
        if key=='fontSize' then return 12 end
      end})
      function observe(key,value)
        visits=visits+1
        trace=trace..key..':'..type(value)..';'
        if visits==2 then
          mutation.text='raw edit during iteration'
          mutation.private1=nil
          mutation.inserted={nested=true}
        end
        if visits==4 then mutation.fillStyle='gloss' end
        local size=w.fontSize
        for k,v in pairs({size=size,kind=type(value)}) do local ignored=tostring(v) end
      end
    "#;
    let reference = host(script);
    let candidate = host(script);
    let walk = |h: &ScriptHost, fast: bool| {
        let table = h.lua.globals().raw_get::<Table>("mutation").unwrap();
        let observe = h.lua.globals().raw_get::<Function>("observe").unwrap();
        let mut record = Vec::new();
        let mut visit = |key: String, value: Value| -> mlua::Result<()> {
            record.push((key.clone(), value.type_name()));
            observe.call::<()>((key, value))
        };
        if fast {
            table.for_each::<String, Value>(&mut visit).unwrap();
        } else {
            for pair in table.pairs::<String, Value>() {
                let (key, value) = pair.unwrap();
                visit(key, value).unwrap();
            }
        }
        record
    };
    assert_eq!(walk(&candidate, true), walk(&reference, false));
    assert_eq!(
        candidate.global_text("trace"),
        reference.global_text("trace")
    );
    assert_eq!(
        candidate.global_text("meta_calls"),
        reference.global_text("meta_calls")
    );
    assert_eq!(
        candidate.shared.vm_checkpoints.get(),
        reference.shared.vm_checkpoints.get()
    );
    assert_eq!(candidate.interface(), reference.interface_classified::<0>());
    assert_eq!(candidate.findings(), reference.findings());
}

#[test]
fn inventory_retains_raw_iteration_order_and_first_malformed_key_cutoff() {
    for malformed in ["true", "{}", "function() end", "string.char(255)"] {
        let script = format!(
            r#"
          local w=Label('Malformed')
          local d=rawget(w,'__data')
          d.fillStyle='solid'; d.gridColour='#ffffff'; d.cursorColour='#ff0000'
          d[17]='numeric key'; d[{malformed}]=true
        "#
        );
        let h = host(&script);
        let data = widgets(&h)[0].raw_get::<Table>("__data").unwrap();
        let mut prefix = Vec::new();
        let mut failed = false;
        for pair in data.pairs::<String, Value>() {
            match pair {
                Ok((key, _)) if documented_property(&key) => prefix.push(format!("Label.{key}")),
                Ok(_) => {}
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        assert!(failed, "the fixture contains a key String cannot decode");
        let reference = h.interface_classified::<0>();
        let candidate = h.interface_classified::<2>();
        assert_eq!(candidate, reference);
        assert_eq!(h.interface_classified::<3>(), reference);
        assert_eq!(
            candidate
                .unsupported
                .iter()
                .map(|row| row.feature.clone())
                .collect::<Vec<_>>(),
            prefix
        );
    }
}

#[test]
fn plain_projection_matches_scalar_kinds_assets_styles_and_documented_types() {
    let keys = serde_json::to_string(DOCUMENTED).unwrap();
    let keys = keys.trim_start_matches('[').trim_end_matches(']');
    let script = format!(
        r#"
      local keys={{{keys}}}
      local values={{true,false,1,'#ffffff','invalid','solid',{{}},function() end}}
      for _,kind in ipairs({{'Panel','Frame','ParameterValue','Knob','ParamKnob',
          'Slider','ParamSlider','Button','OnOffButton','ParamOnOffButton',
          'NumBox','ParamNumBox','Label','Text'}}) do
        for n,value in ipairs(values) do
          local w=Label(kind..n)
          local d=rawget(w,'__data')
          d.kind=kind; d.x=n*3; d.y=n*5; d.width=80; d.height=20
          d.value=0.25; d.default=0.5; d.min=-1; d.max=1; d.integer=false
          d.visible=n%2==0; d.enabled=n%3==0; d.alpha=0.625
          d.interceptsMouseClicks=false; d.exported=true; d.paramId=37
          d.text='caption '..n; d.tooltip='tip '..n; d.displayName='name '..n
          d.displayText=n%2==0 and 'readout '..n or ''; d.unit='Decibels'; d.mapper=2
          d.vertical=true; d.showLabel=true; d.showValue=true
          d.image='authored/base.png'; d.backgroundImage='authored/base.png'
          d.normalImage='authored/normal.png'; d.stripImage='authored/strip.png'
          d.pressedImage='authored/pressed.png'; d.overImage='authored/over.png'
          d.overPressedImage='authored/over-pressed.png'; d.handleImage='authored/handle.png'
          d.frames=7; d.stripHorizontal=true
          d.backgroundColour='#123456'; d.backgroundColourOn='#234567'
          d.backgroundColourOff='#345678'; d.sliderColour='#456789'; d.fillColour='#56789a'
          d.font=n%2==0 and 'authored/font.ttf' or ''; d.fontSize=13
          d.textColour='#abcdef'; d.align='right'
          for _,key in ipairs(keys) do rawset(d,key,value) end
          d.privateFunction=function() error('never execute') end
          d.privateTable={{nested=n}}; d[17]='numeric key'
        end
      end
    "#
    );
    let h = host(&script);
    assert!(widgets(&h).iter().all(|w| {
        InputRoute::for_table(&w.raw_get::<Table>("__data").unwrap()) == InputRoute::Plain
    }));
    let reference = h.interface_classified::<2>();
    let candidate = h.interface_classified::<3>();
    assert_eq!(candidate, reference);
    assert_eq!(candidate.widgets.len(), 112);
    assert_eq!(candidate.assets.len(), 8);
    assert!(!candidate.styles.is_empty());
    assert_eq!(candidate.widgets[0].tooltip, "tip 1");
    assert!(candidate.widgets[0].hidden);
    assert_eq!(candidate.widgets[0].images.len(), 8);
    assert_eq!(
        ScriptHost::control_values_from(&candidate),
        ScriptHost::control_values_from(&reference)
    );
    assert!(h.findings().is_empty());
}

#[test]
fn projection_rebuilds_at_declaration_position_around_getter_mutations() {
    let script = r#"
      calls=0; trace=''
      local before=Label('Before')
      local bound=ParamKnob(Program,'Gain')
      local after=Label('After')
      local meta=Label('Meta')
      local bd=rawget(before,'__data'); local ad=rawget(after,'__data')
      local base=Program.getParameter
      Program.getParameter=function(self,key)
        calls=calls+1; trace=trace..'value;'
        rawset(bd,'text','before '..calls); rawset(ad,'text','after '..calls)
        rawset(ad,'fontSize',12+calls); rawset(ad,'fillStyle','solid '..calls)
        return base(self,key)
      end
      setmetatable(rawget(meta,'__data'),{__index=function(_,key)
        trace=trace..key..';'
        if key=='fontSize' then
          rawset(bd,'tooltip','late '..calls)
          postEvent{type=Event.Controller,controller=1,value=17}
          return 12
        end
      end})
      function onNote(e)
        rawset(ad,'displayText','raw '..e.note)
        rawget(before,'__data').private={nested=e.note}
      end
      function onSave() return {calls=calls,trace=trace} end
    "#;
    let mut reference = host(script);
    let mut candidate = host(script);
    for note in [None, Some(60), Some(61)] {
        if let Some(note) = note {
            reference.note_on(1, note, 64, 0);
            candidate.note_on(1, note, 64, 0);
        }
        let expected = reference.interface_classified::<2>();
        let actual = candidate.interface_classified::<3>();
        assert_eq!(actual, expected);
        assert_eq!(
            candidate.global_text("trace"),
            reference.global_text("trace")
        );
        assert_eq!(candidate.save_ui_state(), reference.save_ui_state());
        assert_eq!(candidate.take_commands(), reference.take_commands());
        assert_eq!(candidate.findings(), reference.findings());
        assert_eq!(candidate.ui_revision(), reference.ui_revision());
    }
}

#[test]
fn projection_fallback_keeps_nested_reads_and_budget_fault_order() {
    let script = r#"
      local plain=Label('Plain')
      local menu=Menu('Menu',{'Group/One','Group/Two'})
      menu.hierarchical=true
      local cells=Table('Cells',3,0,0,1)
      local view=Viewport('Viewport'); view.viewPosition={3,4}
      local image=Image('authored/image.png')
      local xy=XY('Plain','Missing')
      setmetatable(rawget(plain,'__data'),{__index=function(_,key)
        if key=='fontSize' then
          rawget(menu,'__data').items[1]='Changed/One'
          rawget(cells,'__data').values[2]=0.75
          local n=0; for i=1,10000 do n=n+i end
          return 12+n*0
        end
      end})
    "#;
    let config = Config {
        load_work: 1024,
        ..Default::default()
    };
    let reference = host_config(script, config);
    let candidate = host_config(script, config);
    reference.shared.remaining_work.set(7);
    candidate.shared.remaining_work.set(7);
    assert_eq!(
        candidate.interface_classified::<3>(),
        reference.interface_classified::<2>()
    );
    assert_eq!(candidate.findings(), reference.findings());
    assert!(
        candidate
            .findings()
            .iter()
            .any(|row| row.value.contains("budget"))
    );
    assert_eq!(
        candidate.shared.vm_checkpoints.get(),
        reference.shared.vm_checkpoints.get()
    );
    assert_eq!(candidate.shared.remaining_work.get(), 7);
    assert_eq!(reference.shared.remaining_work.get(), 7);
}

#[cfg(feature = "scan")]
#[test]
fn opt_in_export_route_counts_cover_each_widget_without_extra_getters() {
    use crate::script::diagnostics::OwnerAudit;
    use std::sync::Arc;
    let script = r#"
      getter_calls=0
      local label=Label('Plain')
      local knob=Knob('Knob')
      local meta=Label('Meta')
      setmetatable(rawget(meta,'__data'),{__index=function(_,key)
        if key=='fontSize' then getter_calls=getter_calls+1; return 12 end
      end})
      local bound=ParamKnob(Program,'Gain')
      local menu=Menu('Menu',{'One','Two'})
      local image=Image('authored/image.png')
    "#;
    for projection in [false, true] {
        let ordinary = host(script);
        let observed = host(script);
        let audit = Arc::new(OwnerAudit::default());
        observed.install_owner_audit(Some(audit.clone()));
        let expected = ordinary.interface_classified::<2>();
        let actual = if projection {
            observed.interface_classified::<3>()
        } else {
            observed.interface_classified::<2>()
        };
        assert_eq!(actual, expected);
        assert_eq!(
            observed.global_text("getter_calls"),
            ordinary.global_text("getter_calls")
        );
        assert_eq!(observed.findings(), ordinary.findings());
        let snapshot = audit.snapshot();
        let count = |name| {
            snapshot
                .counters
                .iter()
                .find(|row| row.name == name)
                .unwrap()
                .value
        };
        assert_eq!(count("export_plain_scalar_widgets"), 2);
        assert_eq!(count("export_metatable_widgets"), 1);
        assert_eq!(count("export_bound_widgets"), 1);
        assert_eq!(count("export_indirect_kind_widgets"), 2);
    }
}

#[cfg(all(feature = "scan", target_os = "linux"))]
fn caller_cpu_ns() -> u64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    u64::try_from(t.tv_sec).unwrap() * 1_000_000_000 + u64::try_from(t.tv_nsec).unwrap()
}

#[cfg(all(feature = "scan", target_os = "linux"))]
#[test]
#[ignore = "bounded manual paired export benchmark; optionally export authored IR to KONTRA_UVI_INVENTORY_ORACLE_DIR"]
fn paired_full_export_inventory_benchmark() {
    paired_export_benchmark::<0, 2>("for_each_canonical_membership");
}

#[cfg(all(feature = "scan", target_os = "linux"))]
#[test]
#[ignore = "bounded manual projection benchmark; optionally export authored IR to KONTRA_UVI_INVENTORY_ORACLE_DIR"]
fn paired_full_export_projection_benchmark() {
    paired_export_benchmark::<2, 3>("fresh_plain_scalar_projection");
}

#[cfg(all(feature = "scan", target_os = "linux"))]
fn paired_export_benchmark<const REFERENCE: u8, const CANDIDATE: u8>(experiment: &str) {
    use std::time::Instant;
    let artifact =
        std::env::var_os("KONTRA_UVI_INVENTORY_ORACLE_DIR").map(std::path::PathBuf::from);
    if let Some(dir) = &artifact {
        assert!(
            dir.is_absolute() && dir.starts_with("/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts")
        );
        assert!(!dir.exists(), "use a new private artifact directory");
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(dir).unwrap();
    }
    let mut rows = Vec::new();
    let mut fixtures = vec![
        ("ordinary", ""),
        (
            "sparse_documented",
            "if i%4==0 then d.fillStyle='solid' end",
        ),
        (
            "all_documented",
            "for _,key in ipairs(documented) do d[key]='#888888' end",
        ),
    ];
    if CANDIDATE == 3 {
        fixtures.extend([
            (
                "mixed_fallback",
                "if i%4==0 then setmetatable(d,{__index=function() end}) elseif i%4==1 then d.kind='Menu'; d.items={'One','Two'}; d.value=1 end",
            ),
            (
                "all_metatable_fallback",
                "setmetatable(d,{__index=function() end})",
            ),
        ]);
    }
    for (name, properties) in fixtures {
        let keys = serde_json::to_string(DOCUMENTED).unwrap();
        let keys = keys.trim_start_matches('[').trim_end_matches(']');
        let script = format!(
            r#"
          setSize(720,480)
          local documented={{{keys}}}
          for i=1,10145 do
            local w=Label('Widget'..i)
            local d=rawget(w,'__data')
            d.x=(i%8)*80; d.y=(i%20)*22; d.width=78; d.height=20
            d.visible=i<=24; d.text='Value '..i; d.fontSize=12
            d.privateTable={{i}}; d.privateFlag=true; d.privateNumber=i
            d.privateFunction=function() end
            {properties}
          end
        "#
        );
        let h = host(&script);
        let eligible = widgets(&h)
            .iter()
            .filter(|w| {
                InputRoute::for_table(&w.raw_get::<Table>("__data").unwrap()) == InputRoute::Plain
            })
            .count();
        let expected = h.interface_classified::<REFERENCE>();
        assert_eq!(expected.widgets.len(), 10145);
        assert_eq!(h.interface_classified::<CANDIDATE>(), expected);
        for round in 0..8 {
            for candidate in if round % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            } {
                let wall = Instant::now();
                let cpu = caller_cpu_ns();
                let face = if candidate {
                    h.interface_classified::<CANDIDATE>()
                } else {
                    h.interface_classified::<REFERENCE>()
                };
                let cpu = caller_cpu_ns() - cpu;
                let wall = wall.elapsed().as_nanos() as u64;
                assert_eq!(face, expected);
                rows.push(serde_json::json!({"fixture":name,"round":round,"candidate":candidate,"wall_ns":wall,"caller_cpu_ns":cpu,"widgets":face.widgets.len(),"eligible_plain_widgets":eligible,"unsupported":face.unsupported.len()}));
            }
        }
        if name == "sparse_documented"
            && let Some(dir) = &artifact
        {
            for (name, face) in [
                ("reference", expected),
                ("candidate", h.interface_classified::<CANDIDATE>()),
            ] {
                let bytes = serde_json::to_vec(&fixture::encode(&face)).unwrap();
                assert!(bytes.len() <= 64 << 20);
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let file = options.open(dir.join(format!("{name}.json"))).unwrap();
                use std::io::Write;
                (&file).write_all(&bytes).unwrap();
            }
        }
    }
    let receipt = serde_json::json!({"schema":"kontra-uvi-inventory-paired-v1","experiment":experiment,"scope":"authored 10145-widget full export, no owner audit; caller CPU excludes model comparison and serialization","rows":rows});
    if let Some(dir) = &artifact {
        std::fs::write(
            dir.join("timings.json"),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
    }
    println!("{receipt}");
}
