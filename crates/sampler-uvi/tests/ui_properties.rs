use sampler_ui_ir::{Binding, Rgba};
use sampler_uvi::script::{Config, ScriptHost};

const XML: &str = include_str!("../../../tools/fixtures/authored-ui-properties.uvip");

#[test]
fn authored_styles_are_consumed_and_changed_callbacks_refresh_the_model() {
    let mut host = ScriptHost::new(XML, (), Config::default()).unwrap();
    let face = host.interface();
    assert!(host.fault_counts().init.is_empty());
    assert!(face.unsupported.is_empty(), "{:?}", face.unsupported);
    let button = &face.widgets[0];
    for state in [0, 2, 4] {
        assert_eq!(
            face.styles[button.state_styles[state].unwrap().0].color,
            Rgba::rgb(0x00ff00)
        );
    }
    for state in [1, 3, 5] {
        assert_eq!(
            face.styles[button.state_styles[state].unwrap().0].color,
            Rgba::rgb(0xff0000)
        );
    }
    assert_eq!(face.widgets[1].colors.arrow, Some(Rgba::rgb(0x00ff00)));
    assert_eq!(face.widgets[1].colors.outline, Some(Rgba::rgb(0xff0000)));
    assert!(face.widgets[1].menu_arrow);
    assert_eq!(face.widgets[2].popup_display, Some(true));
    assert_eq!(face.widgets[3].popup_display, Some(false));
    let Binding::Control(control) = button.binding else {
        panic!("missing button binding")
    };
    host.set_control(control, 1.).unwrap();
    let changed = host.interface();
    assert!(!changed.widgets[1].menu_arrow);
    assert_eq!(
        changed.styles[changed.widgets[0].state_styles[0].unwrap().0].color,
        Rgba::rgb(0x0000ff)
    );
}

#[test]
fn invalid_colour_and_unconsumed_widget_properties_remain_visible_findings() {
    let xml = XML
        .replace(
            "button.textColourOff='#00ff00'",
            "button.textColourOff='unmodeled-colour'",
        )
        .replace(
            "knob.showPopupDisplay=false",
            "knob.showPopupDisplay='not a boolean' knob.arrowColour='red'",
        );
    let host = ScriptHost::new(&xml, (), Config::default()).unwrap();
    let face = host.interface();
    assert!(
        face.unsupported
            .iter()
            .any(|u| u.feature == "Button.textColourOff")
    );
    assert!(
        face.unsupported
            .iter()
            .any(|u| u.feature == "Knob.arrowColour")
    );
    assert!(face.unsupported.iter().any(|u| u.feature == "Knob.showPopupDisplay"));
    assert_eq!(face.widgets[3].popup_display,None);
}
