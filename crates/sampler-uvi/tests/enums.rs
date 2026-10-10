use sampler_uvi::script::{Config, ScriptHost};

#[test]
fn real_unit_and_mapper_enum_ids_preserve_authored_values_and_normalized_edits() {
    let script = r#"
      assert(Unit.Generic==0 and Unit.Percent==1 and Unit.PercentNormalized==2)
      assert(Unit.Seconds==3 and Unit.MilliSeconds==5 and Unit.Hertz==7)
      assert(Unit.Decibels==9 and Unit.UviFilter==10 and Unit.LinearGain==11)
      assert(Unit.Pan==12 and Unit.Megabyte==13 and Unit.SemiTones==14)
      assert(Unit.Cents==15 and Unit.MidiKey==16 and Unit.Invented==nil)
      assert(Mapper.Linear==0 and Mapper.Exponential==1 and Mapper.QuinticRoot==2)
      assert(Mapper.QuarticRoot==3 and Mapper.CubeRoot==4 and Mapper.SquareRoot==5)
      assert(Mapper.Quadratic==6 and Mapper.Cubic==7 and Mapper.Quartic==8 and Mapper.Quintic==9)
      assert(Mapper.Invented==nil)
      local percent=Knob{'Percent',0.25,0,1,unit=Unit.PercentNormalized}
      assert(percent.value==0.25 and percent.min==0 and percent.max==1)
      local exponential=Knob{'Exp',1,1,10000,mapper=Mapper.Exponential}
      exponential:setValueNormalized(0.5,false)
      assert(math.abs(exponential.value-100)<1e-9)
      assert(math.abs(exponential:getValueNormalized()-0.5)<1e-9)
      local root=Knob{'Root',0,0,1,mapper=Mapper.QuinticRoot}
      root:setValueNormalized(0.03125,false);assert(math.abs(root.value-0.5)<1e-9)
      local legacy=Knob{'Legacy',1,1,10000,mapper='Exponential'}
      legacy:setValueNormalized(0.5,false);assert(math.abs(legacy.value-100)<1e-9)
      Knob{'Ms',1500,0,2000,unit=Unit.MilliSeconds}
      Knob{'Hz',1500,0,2000,unit=Unit.Hertz}
      Knob{'Gain',0.25,0,1,unit=Unit.LinearGain}
      Knob{'ZeroGain',0,0,1,unit=Unit.LinearGain}
      Knob{'Pan',0,-1,1,unit=Unit.Pan}
      Knob{'Custom',0.25,0,1,unit=Unit.PercentNormalized,displayText='Authored'}
      loaded=true
    "#;
    let xml = format!(
        "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    );
    let host = ScriptHost::new(&xml, (), Config::default()).unwrap();
    assert!(host.findings().is_empty(), "{:?}", host.findings());
    assert_eq!(host.global_text("loaded"), "true");
    let face = host.interface();
    assert_eq!(face.widgets[0].initial_value, 0.25);
    assert_eq!(face.widgets[0].value_text.as_deref(), Some("25 %"));
    for (i, text) in [
        (4, "1.5 s"),
        (5, "1.5 kHz"),
        (6, "-12.041 dB"),
        (7, "-inf dB"),
        (8, "Center"),
        (9, "Authored"),
    ] {
        assert_eq!(face.widgets[i].value_text.as_deref(), Some(text));
    }
    assert_eq!(face.widgets[1].mapper.as_deref(), Some("Exponential"));
    assert_eq!(face.widgets[2].mapper.as_deref(), Some("QuinticRoot"));
}

#[test]
fn numeric_units_and_shared_renderer_formatters_agree_without_rescaling() {
    for (id, name, value) in [
        (0, "Generic", 0.125), (1, "Percent", 12.5),
        (2, "PercentNormalized", 0.25), (3, "Seconds", 0.5),
        (5, "MilliSeconds", 1500.), (7, "Hertz", 1500.),
        (9, "Decibels", -12.), (10, "UviFilter", 0.5),
        (11, "LinearGain", 0.25), (12, "Pan", 0.),
        (13, "Megabyte", 1.25), (14, "SemiTones", 12.),
        (15, "Cents", 25.), (16, "MidiKey", 60.),
    ] {
        let xml = format!("<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[Knob{{'K',{value},-10000,10000,unit={id}}}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>");
        let host = ScriptHost::new(&xml, (), Config::default()).unwrap();
        assert!(host.findings().is_empty(), "{:?}", host.findings());
        let face = host.interface();
        let widget = &face.widgets[0];
        let expected = sampler_ui_ir::Display { unit: name.into(), ..Default::default() }
            .format_value(value, false, sampler_ui_ir::Source::FalconLua);
        assert_eq!(widget.initial_value, value);
        assert_eq!(widget.value_text.as_deref(), Some(expected.as_str()), "{id}: {name}");
    }
}
