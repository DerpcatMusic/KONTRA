use sampler_ui_ir::{Binding, InterfacePatch, Kind, Rgba, UviWaveViewSample, WidgetRef};
use sampler_uvi::script::ScriptHost;

const XML: &str = include_str!("fixtures/wave_view_request.uvip");
fn host(xml: &str) -> ScriptHost {
    ScriptHost::new(xml, (), Default::default()).unwrap()
}

#[test]
fn wave_view_manual_request_updates_and_clears_through_original_menu_control() {
    let mut host = host(XML);
    let base = host.interface();
    let Binding::Control(control) = base.widgets[0].binding else {
        panic!("menu control")
    };
    let request = base.widgets[1].uvi_wave_view_request.as_ref().unwrap();
    assert_eq!(request.sample, UviWaveViewSample::Unspecified);
    assert_eq!(request.colors.playback_cursor, Some(Rgba::rgb(0xff0000)));
    assert_eq!(request.colors.high_wave, Some(Rgba::rgb(0x00ff00)));
    assert_eq!(request.colors.low_wave, Some(Rgba::rgb(0x00ff00)));
    assert_eq!(request.colors.axis, Some(Rgba::rgb(0x777777)));
    assert_eq!(request.colors.channel_separator, Some(Rgba::rgb(0x888888)));
    assert_eq!(request.colors.sample_name, Some(Rgba::rgb(0xffffff)));
    assert_eq!(request.colors.sample_name_background, Some(Rgba::rgb(0)));
    assert!(!base.visible(WidgetRef(1)));
    let mut published = base.clone();
    let mut previous = InterfacePatch::default();
    for (step, (selection, sample, visible)) in [
        (2., UviWaveViewSample::Manual("wave-a.wav".into()), true),
        (3., UviWaveViewSample::Manual("wave-b.wav".into()), true),
        (4., UviWaveViewSample::Cleared, true),
        (2., UviWaveViewSample::Manual("wave-a.wav".into()), true),
        (1., UviWaveViewSample::Cleared, false),
    ]
    .into_iter()
    .enumerate()
    {
        host.set_control(control, selection).unwrap();
        let current = host.interface();
        assert_eq!(
            current.widgets[1]
                .uvi_wave_view_request
                .as_ref()
                .unwrap()
                .sample,
            sample
        );
        assert_eq!(current.visible(WidgetRef(1)), visible);
        assert_eq!(host.global_text("picked"), format!("{}", selection as i32));
        assert_eq!(host.global_text("calls"), format!("{}", step + 1));
        for at in 0..2 {
            assert_eq!(current.widgets[at].source_id, base.widgets[at].source_id);
            assert_eq!(current.widgets[at].binding, base.widgets[at].binding);
            assert_eq!(current.widgets[at].components, base.widgets[at].components);
            assert_eq!(current.widgets[at].rect, base.widgets[at].rect);
        }
        assert_eq!(current.assets, base.assets);
        assert!(
            current.widgets[1].waveform.is_none(),
            "do not invent a Kontakt zone"
        );
        assert_eq!(current.widgets[1].colors, base.widgets[1].colors);
        assert!(
            current
                .unsupported
                .iter()
                .any(|u| u.feature == "WaveView service" && u.widget == Some(WidgetRef(1)))
        );
        let patch = InterfacePatch::between(&base, &current);
        patch.apply(&base, &previous, &mut published);
        assert_eq!(published, current);
        previous = patch;
    }
    assert!(matches!(
        published.widgets[1]
            .uvi_wave_view_request
            .as_ref()
            .unwrap()
            .sample,
        UviWaveViewSample::Cleared
    ));
    assert_eq!(published.widgets[1].rect, base.widgets[1].rect);
    assert_eq!(
        host.sample_load_status().requested,
        0,
        "display assignment submits no oscillator sample transaction"
    );
}

#[test]
fn wave_view_absent_empty_invalid_and_unknown_colors_do_not_invent_native_defaults() {
    for (expression, expected, invalid) in [
        ("nil", UviWaveViewSample::Cleared, false),
        ("''", UviWaveViewSample::Manual(String::new()), false),
        (
            "42",
            UviWaveViewSample::Unsupported {
                type_name: "integer".into(),
            },
            true,
        ),
        (
            "true",
            UviWaveViewSample::Unsupported {
                type_name: "boolean".into(),
            },
            true,
        ),
        (
            "{}",
            UviWaveViewSample::Unsupported {
                type_name: "table".into(),
            },
            true,
        ),
    ] {
        let xml = format!(
            "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[local w=WaveView('00'); w.sample={expression}; w.hiWaveColour='unrecognized'; w.lowWaveColour=42]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
        );
        let face = host(&xml).interface();
        let request = face.widgets[0].uvi_wave_view_request.as_ref().unwrap();
        // Luau may represent literal 42 as number rather than integer. Preserve
        // its observed type rather than attaching a numeric engine meaning.
        if expression == "42" {
            assert!(
                matches!(&request.sample, UviWaveViewSample::Unsupported { type_name } if type_name == "integer" || type_name == "number")
            );
        } else {
            assert_eq!(request.sample, expected);
        }
        assert_eq!(request.colors, Default::default());
        assert_eq!(
            face.unsupported
                .iter()
                .any(|u| u.feature == "WaveView.sample"),
            invalid
        );
        for feature in [
            "WaveView service",
            "WaveView.hiWaveColour",
            "WaveView.lowWaveColour",
        ] {
            assert!(face.unsupported.iter().any(|u| u.feature == feature));
        }
    }
}

#[test]
fn wave_view_colours_are_requests_and_other_widget_fallbacks_remain_intact() {
    let source = "local w=WaveView('00'); w.playbackCursorColour='#8011cc33'; w.hiWaveColour='red'";
    let xml = format!(
        "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{source}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    );
    let face = host(&xml).interface();
    let request = face.widgets[0].uvi_wave_view_request.as_ref().unwrap();
    assert_eq!(
        request.colors.playback_cursor,
        Some(Rgba {
            a: 128,
            ..Rgba::rgb(0x11cc33)
        })
    );
    assert_eq!(request.colors.high_wave, Some(Rgba::rgb(0xff0000)));
    assert_eq!(face.widgets[0].colors.wave, None);
    assert_eq!(face.widgets[0].colors.wave_cursor, None);
    assert!(
        face.unsupported
            .iter()
            .any(|u| u.feature == "WaveView service")
    );
    for kind in ["Label", "WaveForm", "FileSelector", "DnDArea", "SVG"] {
        let other = host(&xml.replace("WaveView(", &format!("{kind}("))).interface();
        assert!(other.widgets[0].uvi_wave_view_request.is_none());
        if kind == "WaveForm" {
            assert!(matches!(other.widgets[0].kind, Kind::Waveform));
        }
        if kind != "Label" {
            assert!(
                other
                    .unsupported
                    .iter()
                    .any(|u| u.feature == format!("{kind} service"))
            );
        }
    }
}

#[test]
fn wave_view_requests_keep_canonical_metatable_reads_on_every_export() {
    let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[reads=0; selected=1; local route=Menu('00',{'00','10'},1); route.changed=function(self) selected=self.value end; local w=WaveView('01'); local d=rawget(w,'__data'); setmetatable(d,{__index=function(_,key) if key=='sample' then reads=reads+1; return selected==1 and 'wave-a.wav' or 'wave-b.wav' elseif key=='hiWaveColour' then return 'red' end end})]]></script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let mut host = host(xml);
    let first = host.interface();
    let observed = host.global_text("reads").parse::<usize>().unwrap();
    assert!(observed > 0);
    let again = host.interface();
    assert!(host.global_text("reads").parse::<usize>().unwrap() > observed);
    assert_eq!(
        first, again,
        "equal published metadata does not skip the next Lua read"
    );
    let Binding::Control(control) = first.widgets[0].binding else {
        panic!("menu")
    };
    host.set_control(control, 2.).unwrap();
    let changed = host.interface();
    let request = changed.widgets[1].uvi_wave_view_request.as_ref().unwrap();
    assert_eq!(
        request.sample,
        UviWaveViewSample::Manual("wave-b.wav".into())
    );
    assert_eq!(request.colors.high_wave, Some(Rgba::rgb(0xff0000)));
    assert_eq!(changed.widgets[1].source_id, first.widgets[1].source_id);
    assert!(
        changed
            .unsupported
            .iter()
            .any(|u| u.feature == "WaveView service")
    );
}

#[test]
fn wave_view_manual_sample_stays_separate_from_unsupported_oscillator_binding() {
    let xml = r#"<UVI4><Program><Layers><Layer><Keygroups><Keygroup><Oscillators>
      <SamplePlayer Name="bound" SamplePath="bound.wav"/>
      </Oscillators></Keygroup></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script><![CDATA[
      local oscillator=Program.layers[1].keygroups[1].oscillators[1]
      local w=WaveView('display')
      w:bind(oscillator)
      w.sample='manual-a.wav'
      function onNote(e) w.sample=e.note==60 and 'manual-b.wav' or nil end
      local label=Label('unrelated')
      label.bind=function() unrelated=17 end
      label:bind()
    ]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let mut h = host(xml);
    let a = h.interface();
    let request = a.widgets[0].uvi_wave_view_request.as_ref().unwrap();
    assert!(matches!(
        request.binding,
        sampler_ui_ir::UviWaveViewBinding::Oscillator { .. }
    ));
    assert_eq!(
        request.sample,
        UviWaveViewSample::Manual("manual-a.wav".into())
    );
    assert!(
        a.unsupported
            .iter()
            .any(|u| u.feature == "WaveView.binding")
    );
    assert_eq!(
        h.global_text("unrelated"),
        "17",
        "WaveView bind must not shadow unrelated widget fields"
    );
    for note in [60, 61] {
        h.note_on(1, note, 64, 0);
        let face = h.interface();
        assert_eq!(
            face.widgets[0]
                .uvi_wave_view_request
                .as_ref()
                .unwrap()
                .binding,
            request.binding
        );
        assert_eq!(
            h.sample_load_status().requested,
            0,
            "a display assignment cannot schedule oscillator sample replacement"
        );
    }
}
