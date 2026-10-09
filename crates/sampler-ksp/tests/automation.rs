//! Saved automation enters the same typed widget callback path as the UI.
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
use sampler_core::*;
use sampler_ksp::{Environment, Limits as KspLimits};

fn runtime(soft_takeover: bool) -> Runtime {
    let script = sampler_ksp::compile_with(
        "on init
        declare ui_label $label (1,1)
        declare ui_slider $unused (0,127)
        declare ui_slider $target (20,100)
        $target := 90
        declare $observed := 0
        declare $note_value := 0
        declare $seen_cc := 0
        end on
        on ui_control ($target) $observed := $target $seen_cc := %CC[1] end on
        on note $note_value := $observed end on",
        48000,
        KspLimits::LIBRARY,
        &[],
        &Environment {
            slot: 3,
            ..Default::default()
        },
    )
    .unwrap();
    let plan = script
        .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
        .unwrap()
        .with_automation_bindings(vec![
            AutomationBinding {
                source: AutomationSource::Controller(1),
                source_slot: 3,
                ui_id: 32770,
                low: 0.,
                high: 1.,
                soft_takeover,
            },
            AutomationBinding {
                source: AutomationSource::HostParameter(10),
                source_slot: 3,
                ui_id: 32770,
                low: 0.,
                high: 1.,
                soft_takeover: false,
            },
        ])
        .unwrap();
    let limits = Limits::for_plan(&plan, 4, 8);
    Runtime::new(plan, limits).unwrap()
}
fn input() -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: None,
    }
}
fn context(rt: &Runtime) -> ControlContext {
    ControlContext {
        performance: rt.performance(0).unwrap(),
        origin: input().channel_address(),
        channels: 1,
    }
}
fn cc(rt: &mut Runtime, value: u8) {
    support::without_heap(|| {
        rt.dispatch_controller(
            context(rt).performance,
            input().channel_address(),
            1,
            1,
            (u64::from(u32::MAX) * u64::from(value) / 127) as u32,
        )
        .unwrap();
    });
}
fn observed(rt: &Runtime) -> i64 {
    rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 1)
        .unwrap()
}
#[test]
fn cc_widget_callback_precedes_the_note_at_the_same_timestamp_and_host_uses_the_same_path() {
    let mut rt = runtime(false);
    cc(&mut rt, 64);
    assert_eq!(observed(&rt), 60);
    assert_eq!(
        rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 3),
        Ok(64)
    );
    rt.trigger(input(), 60, 1.).unwrap();
    assert_eq!(
        rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 2),
        Ok(60)
    );
    rt.dispatch_host_parameter(context(&rt), 10, 0.25).unwrap();
    assert_eq!(observed(&rt), 40);
}
#[test]
fn soft_takeover_requires_crossing_and_rearms_after_an_external_widget_edit() {
    let mut rt = runtime(true);
    cc(&mut rt, 64); // 60 is below the restored 90.
    assert_eq!(observed(&rt), 0);
    cc(&mut rt, 127); // crosses 90 and takes ownership.
    assert_eq!(observed(&rt), 100);
    cc(&mut rt, 64);
    assert_eq!(observed(&rt), 60);
    rt.dispatch_host_parameter(context(&rt), 10, 0.25).unwrap(); // external source sets 40.
    cc(&mut rt, 80); // remains above 40: takeover must rearm.
    assert_eq!(observed(&rt), 40);
    cc(&mut rt, 0); // crosses 40.
    assert_eq!(observed(&rt), 20);
}

#[test]
fn full_callback_capacity_rejects_cc_before_any_widget_or_controller_write() {
    let script = sampler_ksp::compile_with(
        "on init declare ui_slider $target(0,127) $target := 90 end on on ui_control($target) wait(1000000) end on on controller wait(1000000) end on",
        48000, KspLimits::LIBRARY, &[], &Environment::default(),
    ).unwrap();
    let plan = script
        .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
        .unwrap()
        .with_automation_bindings(vec![AutomationBinding {
            source: AutomationSource::Controller(1),
            source_slot: 0,
            ui_id: 32768,
            low: 0.,
            high: 1.,
            soft_takeover: false,
        }])
        .unwrap();
    let mut limits = Limits::for_plan(&plan, 4, 8);
    limits.behaviors = 1;
    let mut rt = Runtime::new(plan, limits).unwrap();
    let c = context(&rt);
    let id = rt.widget_id(rt.active_plan(), 0, 32768).unwrap();
    assert_eq!(
        rt.dispatch_controller(c.performance, c.origin, 1, 1, 0),
        Err(Error::Capacity)
    );
    assert_eq!(
        rt.widget_value(rt.active_plan(), id, 0),
        Ok(WidgetValue::Integer(90))
    );
    assert_eq!(rt.input_controller(c.performance, 1), Ok(0));
}

#[test]
fn pedal_domain_capacity_rejects_before_saved_widget_callback() {
    let script = sampler_ksp::compile_with(
        "on init declare ui_slider $target(0,127) $target := 90 declare $called end on on ui_control($target) inc($called) end on",
        48000, KspLimits::LIBRARY, &[], &Environment::default(),
    ).unwrap();
    let plan = script
        .bind(Prepared::new(48000, vec![], vec![], 0).unwrap())
        .unwrap()
        .with_automation_bindings(vec![AutomationBinding {
            source: AutomationSource::Controller(64),
            source_slot: 0,
            ui_id: 32768,
            low: 0.,
            high: 1.,
            soft_takeover: false,
        }])
        .unwrap();
    let mut limits = Limits::for_plan(&plan, 4, 8);
    limits.channels = 0;
    let mut rt = Runtime::new(plan, limits).unwrap();
    let c = context(&rt);
    let id = rt.widget_id(rt.active_plan(), 0, 32768).unwrap();
    assert_eq!(
        rt.dispatch_controller(c.performance, c.origin, 1, 64, u32::MAX),
        Err(Error::Capacity)
    );
    assert_eq!(
        rt.widget_value(rt.active_plan(), id, 0),
        Ok(WidgetValue::Integer(90))
    );
    assert_eq!(
        rt.script_cell(rt.active_plan(), ScriptInstanceId(0), 0),
        Ok(0)
    );
    assert_eq!(rt.input_controller(c.performance, 64), Ok(0));
}
