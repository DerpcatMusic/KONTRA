//! Computed KSP command operands must not change an already evaluated target.
//! NI examples use set_engine_par(..., $Knob * 1000, ...) and
//! purge_group(..., 1 - $purge); exercise their readback and audible effects.
use sampler_core::*;
use sampler_ksp::{Environment, Script};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn runtime(source: &str) -> Runtime {
    let script: Script = sampler_ksp::compile_with(
        source,
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
        &Environment {
            groups: vec!["Group 0".into(), "Group 1".into()],
            ..Default::default()
        },
    )
    .unwrap();
    let samples = [0.5, 0.25].map(|value| Pcm::new(48000, Box::from([[value; 2]; 48000])).unwrap());
    let regions = (0..2)
        .map(|sample| Region {
            sample,
            key_low: 60 + sample as u8,
            key_high: 60 + sample as u8,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        })
        .collect();
    let prepared = Prepared::new(48000, samples.into(), regions, 2)
        .unwrap()
        .with_groups(2, vec![Some(0), Some(1)])
        .unwrap();
    let plan = script.bind(prepared).unwrap();
    let limits = Limits::for_plan(&plan, 8, 8);
    Runtime::new(plan, limits).unwrap()
}

fn trigger(rt: &mut Runtime, key: u8) {
    rt.trigger(
        Input {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
            key,
            external_id: Some(i32::from(key)),
        },
        key,
        1.,
    )
    .unwrap();
}

fn cell(rt: &Runtime, index: u32) -> i64 {
    rt.script_cell(rt.active_plan(), ScriptInstanceId(0), index)
        .unwrap()
}

fn close(actual: [f32; 2], expected: [f32; 2]) {
    assert!(
        actual
            .into_iter()
            .zip(expected)
            .all(|(a, b)| (a - b).abs() < 1e-5),
        "actual {actual:?}, expected {expected:?}"
    );
}

#[test]
fn computed_engine_value_keeps_its_group_parameter_target() {
    let mut rt = runtime(
        "on init declare $value := 250000 declare $group := 1
           declare $out0 declare $out1 end on
         on note
           set_engine_par($ENGINE_PAR_PAN, $value * 4, $group, -1, -1)
           $out0 := get_engine_par($ENGINE_PAR_PAN, 0, -1, -1)
           $out1 := get_engine_par($ENGINE_PAR_PAN, 1, -1, -1)
         end on",
    );
    support::without_heap(|| {
        trigger(&mut rt, 61);
        assert_eq!(cell(&rt, 2), 500000);
        assert_eq!(cell(&rt, 3), 1000000);
        let mut audio = [[0.; 2]; 64];
        rt.render(&mut audio).unwrap();
        close(audio[63], [0., 0.25]);
        trigger(&mut rt, 60);
        rt.render(&mut audio).unwrap();
        close(audio[63], [0.5, 0.75]);
    });
}

#[test]
fn engine_value_with_nested_query_and_array_target_keeps_outer_address() {
    let mut rt = runtime(
        "on init declare %parameters[2] := ($ENGINE_PAR_VOLUME, $ENGINE_PAR_PAN)
           declare %address[3] := (1, -1, -1)
           declare $parameter := 1 declare $adjust declare $out0 declare $out1 end on
         on note
           set_engine_par(%parameters[$parameter],
             (get_engine_par($ENGINE_PAR_PAN, 0, -1, -1) + $adjust) * 2,
             %address[0], %address[1], %address[2])
           $out0 := get_engine_par($ENGINE_PAR_PAN, 0, -1, -1)
           $out1 := get_engine_par($ENGINE_PAR_PAN, 1, -1, -1)
         end on",
    );
    support::without_heap(|| {
        trigger(&mut rt, 61);
        assert_eq!(cell(&rt, 7), 500000);
        assert_eq!(cell(&rt, 8), 1000000);
        let mut audio = [[0.; 2]; 64];
        rt.render(&mut audio).unwrap();
        close(audio[63], [0., 0.25]);
    });
}

#[test]
fn shared_function_after_wait_and_fuel_yields_preserves_engine_target() {
    for fuel in [1, 2, 7, 64] {
        let mut rt = runtime(
            "on init declare $value := 250000 declare $group := 1
               declare $out0 declare $out1 declare $done end on
             function apply
               set_engine_par($ENGINE_PAR_PAN, $value * 4, $group, -1, -1)
             end function
             on note wait(1000) call apply
               $out0 := get_engine_par($ENGINE_PAR_PAN, 0, -1, -1)
               $out1 := get_engine_par($ENGINE_PAR_PAN, 1, -1, -1)
               $done := 1
             end on",
        );
        rt.set_behavior_block_fuel(fuel);
        support::without_heap(|| {
            trigger(&mut rt, 61);
            let mut audio = [[0.; 2]; 8];
            for _ in 0..256 {
                rt.render(&mut audio).unwrap();
                if cell(&rt, 4) == 1 {
                    break;
                }
            }
            assert_eq!(cell(&rt, 4), 1, "fuel {fuel}");
            assert_eq!(cell(&rt, 2), 500000, "fuel {fuel}");
            assert_eq!(cell(&rt, 3), 1000000, "fuel {fuel}");
            rt.render(&mut audio).unwrap();
            close(audio[7], [0., 0.25]);
        });
    }
}

#[test]
fn computed_purge_mode_changes_only_the_selected_group_in_persistence_callback() {
    for group in [0, 1] {
        let mut rt = runtime(&format!(
            "on init declare $group := {group} declare $purge := 1
               declare $out0 declare $out1 declare $done end on
             on persistence_changed wait(1000)
               purge_group($group, abs(1 - $purge))
               $out0 := get_purge_state(0)
               $out1 := get_purge_state(1)
               $done := 1
             end on"
        ));
        support::without_heap(|| {
            let mut audio = [[0.; 2]; 64];
            rt.render(&mut audio).unwrap();
            assert_eq!(cell(&rt, 4), 1);
            assert_eq!(cell(&rt, 2), i64::from(group != 0));
            assert_eq!(cell(&rt, 3), i64::from(group != 1));
            trigger(&mut rt, 60);
            trigger(&mut rt, 61);
            rt.render(&mut audio).unwrap();
            close(audio[63], [if group == 0 { 0.25 } else { 0.5 }; 2]);
        });
    }
}

#[test]
fn ui_computed_engine_value_reaches_live_audio_without_changing_other_group() {
    let mut rt = runtime(
        "on init declare ui_slider $pan(-500, 500)
           declare $out0 declare $out1 end on
         on ui_control($pan)
           set_engine_par($ENGINE_PAR_PAN, ($pan + 500) * 1000, 1, -1, -1)
           $out0 := get_engine_par($ENGINE_PAR_PAN, 0, -1, -1)
           $out1 := get_engine_par($ENGINE_PAR_PAN, 1, -1, -1)
         end on",
    );
    let context = ControlContext {
        performance: rt.performance(0).unwrap(),
        origin: ChannelAddress {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
        },
        channels: 1,
    };
    let plan = rt.active_plan();
    let id = sampler_ksp::derived_control_id(0, "$pan");
    support::without_heap(|| {
        trigger(&mut rt, 60);
        trigger(&mut rt, 61);
        let mut audio = [[0.; 2]; 64];
        rt.render(&mut audio).unwrap();
        close(audio[63], [0.75; 2]);
        rt.invoke_control(
            context,
            plan,
            None,
            ControlWrite {
                id,
                value: ControlValue::Integer(500),
            },
        )
        .unwrap();
        assert_eq!(cell(&rt, 0), 500000);
        assert_eq!(cell(&rt, 1), 1000000);
        rt.render(&mut audio).unwrap();
        close(audio[63], [0.5, 0.75]);
    });
}
