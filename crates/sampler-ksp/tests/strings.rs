//! Runtime strings: names built while a callback runs reach group lookup,
//! `find_mod` and `set_text`, in bounded text cells.
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
use sampler_core::{
    Envelope, Input, Limits, Pcm, Playback, Prepared, Protocol, Region, Runtime, ScriptInstanceId,
};

fn run(source: &str, groups: &[&str]) -> Runtime {
    let environment = sampler_ksp::Environment {
        groups: groups.iter().map(|g| (*g).to_owned()).collect(),
        ..Default::default()
    };
    run_in(source, environment)
}

fn run_in(source: &str, environment: sampler_ksp::Environment) -> Runtime {
    run_shaped(source, environment, Ok)
}

fn run_shaped(
    source: &str,
    environment: sampler_ksp::Environment,
    shape: impl FnOnce(Prepared) -> Result<Prepared, sampler_core::Error>,
) -> Runtime {
    let script = sampler_ksp::compile_with(
        source,
        48000,
        sampler_ksp::Limits {
            source_bytes: 65536,
            instructions: 4096,
            variables: 16,
            array_cells: 16,
        },
        &[],
        &environment,
    )
    .unwrap();
    let pcm = vec![Pcm::new(48000, Box::from([[1.0; 2]; 4800])).unwrap()];
    let region = Region {
        sample: 0,
        key_low: 0,
        key_high: 127,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback::default(),
    };
    let prepared = Prepared::new(48000, pcm, vec![region], 128)
        .unwrap()
        .with_engine_parameters(vec![], environment.engine_lookups.clone())
        .unwrap();
    let prepared = shape(prepared).unwrap();
    let note_cells = script.note_cells() * 8;
    let plan = script.bind(prepared).unwrap();
    let behavior_cells = plan.behavior_local_count() * 8;
    let mut rt = Runtime::new(
        plan,
        Limits {
            notes: 8,
            channels: 1,
            performances: 1,
            families: 8,
            voices: 8,
            expressions: 8,
            decisions: 0,
            commands: 16,
            behaviors: 8,
            behavior_fuel: 4096,
            behavior_cells,
            note_cells,
        },
    )
    .unwrap();
    let input = Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(60),
    };
    support::without_heap(|| {
        rt.trigger(input, 60, 1.).unwrap();
        rt.render(&mut [[0.; 2]; 64]).unwrap();
    });
    rt
}

fn cell(rt: &Runtime, n: u32) -> i64 {
    rt.script_cell(rt.active_plan(), ScriptInstanceId(0), n)
        .unwrap()
}

#[test]
fn group_names_and_lookups_take_computed_indices_and_names() {
    let rt = run_in(
        "on init declare $a declare $b declare $c declare $d declare $e declare $i declare @n end on
         on note
           $i := 2
           @n := group_name($i)
           $a := find_group(@n)
           $b := find_group(\"gr\" & (($i - 1) mod 3 + 1))
           $c := find_mod(0, \"ENV_\" & \"AHDSR\")
           $d := find_group(\"missing\")
           $e := find_mod(0, \"missing\")
         end on",
        sampler_ksp::Environment {
            groups: ["gr1", "gr2", "gr3"].map(str::to_owned).into(),
            engine_lookups: vec![sampler_core::EngineLookup {
                group: 0,
                owner: -1,
                target: false,
                name: "ENV_AHDSR".into(),
                index: 5,
            }],
            ..Default::default()
        },
    );
    assert_eq!(cell(&rt, 0), 2);
    assert_eq!(cell(&rt, 1), 1);
    assert_eq!(cell(&rt, 2), 5);
    assert_eq!(cell(&rt, 3), -1);
    assert_eq!(cell(&rt, 4), -1);
}

#[test]
fn text_longer_than_a_cell_is_cut_and_counted() {
    let rt = run(
        "on init declare $a declare @n end on
         on note
           @n := \"x\"
           $a := 0
           while ($a < 40)
             @n := @n & \"0123456789\"
             inc($a)
           end while
         end on",
        &[],
    );
    assert!(rt.truncated_texts() > 0);
}

/// `set_controller` in `on init` is the controller's value before any input.
#[test]
fn set_controller_in_init_sets_the_starting_controller_value() {
    let rt = run(
        "on init declare $a set_controller(111, 100) end on
         on note $a := %CC[111] end on",
        &[],
    );
    assert_eq!(cell(&rt, 0), 100);
}

/// Saved persistent arrays come back before the first callback, as Solo's
/// articulation on/off switches do: without them every note was gated off.
#[test]
fn saved_persistent_arrays_are_restored() {
    use sampler_ksp::model::Value;
    let environment = sampler_ksp::Environment {
        persisted_arrays: [(
            "%on".to_owned(),
            [3, 0, 7, 9].map(Value::Int).to_vec(), // one more than declared
        )]
        .into(),
        ..Default::default()
    };
    let rt = run_in(
        "on init declare $a declare %on[3] make_persistent(%on) read_persistent_var(%on) end on
         on note $a := %on[0] + %on[2] * 10 end on",
        environment,
    );
    assert_eq!(cell(&rt, 0), 73);
}

/// An ordinary array is saved only up to its last change: `%on 7 -3 0 ` for
/// `%on[6]` means `[7, -3, 0, 0, 0, 0]`, and a prefix ending in 1 stays 1.
#[test]
fn saved_array_tail_repeats_its_last_value() {
    use sampler_ksp::model::Value;
    let environment = sampler_ksp::Environment {
        persisted_arrays: [("%on".to_owned(), [4, 1].map(Value::Int).to_vec())].into(),
        ..Default::default()
    };
    let rt = run_in(
        "on init declare $a declare %on[5] make_persistent(%on) read_persistent_var(%on) end on
         on note $a := %on[0] * 100 + %on[4] end on",
        environment,
    );
    assert_eq!(cell(&rt, 0), 401);
}

/// Kontakt ignores a script's call on a note that already ended: a gate-linked
/// note played after its parent was released is dropped, not a fault.
#[test]
fn playing_from_a_released_note_is_ignored_not_a_fault() {
    let mut rt = run(
        "on init declare $a end on
         on note
           wait(100000)
           $a := play_note(60, 100, 0, -1)
         end on",
        &[],
    );
    rt.note_off(
        sampler_core::Input {
            protocol: sampler_core::Protocol::Clap,
            port: 0,
            group: 0,
            channel: 0,
            key: 60,
            external_id: Some(60),
        },
        None,
    )
    .unwrap();
    let mut faults = Vec::new();
    for _ in 0..200 {
        rt.render(&mut [[0.; 2]; 64]).unwrap();
        rt.flush_behaviors(|_, _, outcome| {
            if !matches!(
                outcome,
                sampler_core::Outcome::Finished | sampler_core::Outcome::Cancelled
            ) {
                faults.push(format!("{outcome:?}"));
            }
            true
        });
    }
    assert_eq!(faults, Vec::<String>::new());
}

/// `$EVENT_PAR_0..3` carry a script's own values from a note to its release
/// callback, as Analog Strings tags the notes it owns; large ids survive.
#[test]
fn user_event_parameters_reach_the_release_callback() {
    let mut rt = run(
        "on init declare $a declare $b end on
         on note
           ignore_event($EVENT_ID)
           set_event_par($EVENT_ID, 0, 1234567)
           set_event_par($EVENT_ID, 3, -5)
         end on
         on release
           $a := get_event_par($EVENT_ID, 0)
           $b := get_event_par($EVENT_ID, 3)
         end on",
        &[],
    );
    rt.note_off(
        sampler_core::Input {
            protocol: sampler_core::Protocol::Clap,
            port: 0,
            group: 0,
            channel: 0,
            key: 60,
            external_id: Some(60),
        },
        None,
    )
    .unwrap();
    for _ in 0..10 {
        rt.render(&mut [[0.; 2]; 64]).unwrap();
        rt.flush_behaviors(|_, _, _| true);
    }
    assert_eq!((cell(&rt, 0), cell(&rt, 1)), (1234567, -5));
}

/// `get_event_par` reports a sounding event's zone as nonzero and an ignored
/// one as 0, and the event's MIDI channel (Solo clears finished events by it).
#[test]
fn event_zone_id_is_nonzero_only_while_the_event_sounds() {
    let rt = run(
        "on init declare $sounding declare $ch declare $ignored end on
         on note
           wait(500)
           $sounding := get_event_par($EVENT_ID, $EVENT_PAR_ZONE_ID)
           $ch := get_event_par($EVENT_ID, $EVENT_PAR_MIDI_CHANNEL) + 10
           $ignored := get_event_par(99999, $EVENT_PAR_ZONE_ID) + 7
         end on",
        &[],
    );
    assert_ne!(cell(&rt, 0), 0);
    assert_eq!((cell(&rt, 1), cell(&rt, 2)), (10, 7));
}

/// Another event's key and velocity read back through its id (legato scripts
/// keep them in arrays of ids).
#[test]
fn another_events_key_and_velocity_read_through_its_id() {
    let rt = run(
        "on init declare $id declare $k declare $v end on
         on note
           ignore_event($EVENT_ID)
           $id := play_note(61, 100, 0, 0)
           wait(100)
           $k := get_event_par($id, $EVENT_PAR_NOTE)
           $v := get_event_par($id, $EVENT_PAR_VELOCITY)
         end on",
        &[],
    );
    assert_eq!((cell(&rt, 1), cell(&rt, 2)), (61, 100));
}

#[test]
fn runtime_menu_getters_read_authored_and_live_items() {
    let rt = run("on init declare $before declare $after declare @before declare @after declare ui_menu $m add_menu_item($m,\"first\",17) end on
        on note
            $before := get_menu_item_value(get_ui_id($m),0)
            @before := get_menu_item_str(get_ui_id($m),0)
            set_menu_item_value(get_ui_id($m),0,93)
            set_menu_item_str(get_ui_id($m),0,\"changed\")
            $after := get_menu_item_value(get_ui_id($m),0)
            @after := get_menu_item_str(get_ui_id($m),0)
        end on", &[]);
    assert_eq!(cell(&rt, 0), 17);
    assert_eq!(cell(&rt, 1), 93);
    assert_eq!(
        rt.script_text(rt.active_plan(), ScriptInstanceId(0), 0)
            .unwrap()
            .as_str(),
        "first"
    );
    assert_eq!(
        rt.script_text(rt.active_plan(), ScriptInstanceId(0), 1)
            .unwrap()
            .as_str(),
        "changed"
    );
}

#[test]
fn runtime_menu_add_and_invalid_indexes_follow_native_defaults() {
    let rt = run("on init declare $count declare $value declare $invalid declare $visible declare @text declare ui_menu $m end on
        on note
            add_menu_item($m,\"added\",-2147483648)
            set_menu_item_value(get_ui_id($m),-1,73)
            set_menu_item_str(get_ui_id($m),5,\"invalid\")
            set_menu_item_visibility(get_ui_id($m),0,7)
            $count := get_num_menu_items(get_ui_id($m))
            $value := get_menu_item_value(get_ui_id($m),0)
            $invalid := get_menu_item_value(get_ui_id($m),-1)
            $visible := get_menu_item_visibility(get_ui_id($m),0)
            @text := \"prefix:\" & get_menu_item_str(get_ui_id($m),0) & get_menu_item_str(get_ui_id($m),5)
        end on", &[]);
    assert_eq!(cell(&rt, 0), 1);
    assert_eq!(cell(&rt, 1), i64::from(i32::MIN));
    assert_eq!(cell(&rt, 2), 0);
    assert_eq!(cell(&rt, 3), 1);
    assert_eq!(
        rt.script_text(rt.active_plan(), ScriptInstanceId(0), 0)
            .unwrap()
            .as_str(),
        "prefix:added"
    );
}

#[test]
fn runtime_zone_getter_preserves_source_ids_and_dynamic_parameter_identity() {
    let rt = run_shaped("on init declare $group declare $low declare $high declare $missing declare $parameter declare %pars[3] := ($ZONE_PAR_GROUP,$ZONE_PAR_LOW_KEY,$ZONE_PAR_HIGH_KEY) end on
        on note
            $parameter := %pars[0]
            $group := get_zone_par(73,$parameter)
            $low := get_zone_par(73,%pars[1])
            $high := get_zone_par(73,%pars[2])
            $missing := get_zone_par(1,$ZONE_PAR_HIGH_KEY)
        end on", sampler_ksp::Environment::default(), |_| {
            Prepared::new(48000,vec![Pcm::new(48000,Box::from([[1.;2];64])).unwrap()],vec![Region {
                sample:0,key_low:7,key_high:94,root_key:None,velocity_low:0.,velocity_high:1.,gain:1.,
                envelope:Envelope::default(),playback:Playback::default()
            }],128)?.with_groups(3,vec![Some(2)])?.with_source_zones(vec![73])
        });
    assert_eq!(cell(&rt, 0), 2);
    assert_eq!(cell(&rt, 1), 7);
    assert_eq!(cell(&rt, 2), 94);
    assert_eq!(
        cell(&rt, 3),
        0,
        "source hole must not resolve to runtime region zero"
    );
}

#[test]
fn init_zone_getter_reads_authored_physical_zone_fields() {
    let rt = run_in(
        "on init declare $g := get_zone_par(73,$ZONE_PAR_GROUP) declare $lo := get_zone_par(73,$ZONE_PAR_LOW_KEY) declare $hi := get_zone_par(73,$ZONE_PAR_HIGH_KEY) declare $missing := get_zone_par(1,$ZONE_PAR_HIGH_KEY) end on",
        sampler_ksp::Environment {
            zones: [(73, [2, 7, 94])].into(),
            ..Default::default()
        },
    );
    assert_eq!(
        [cell(&rt, 0), cell(&rt, 1), cell(&rt, 2), cell(&rt, 3)],
        [2, 7, 94, 0]
    );
}

#[test]
fn v1_midi_bytes_and_signed_zero_are_executable_in_callbacks() {
    let rt = run(
        "on init declare $msb declare $lsb declare $negative declare ~zero := -0.0 end on
        on note $msb := msb(16383) $lsb := lsb(130) $negative := signbit(~zero) end on",
        &[],
    );
    assert_eq!((cell(&rt, 0), cell(&rt, 1), cell(&rt, 2)), (127, 2, 1));
}

#[test]
fn ui_command_readback_is_current_before_host_effects_are_drained() {
    let rt = run(
        "on init declare ui_knob $knob(0,100,1)
        declare $hidden declare $default declare $layer
        declare @text declare @label declare @help end on
        on note
            hide_part($knob,$HIDE_WHOLE_CONTROL)
            set_knob_defval($knob,31)
            set_control_par(get_ui_id($knob),$CONTROL_PAR_Z_LAYER,3)
            set_text($knob,\"changed\")
            set_knob_label($knob,\"label\")
            set_control_help($knob,\"help\")
            $hidden := get_control_par(get_ui_id($knob),$CONTROL_PAR_HIDE)
            $default := get_control_par(get_ui_id($knob),$CONTROL_PAR_DEFAULT_VALUE)
            $layer := get_control_par(get_ui_id($knob),$CONTROL_PAR_Z_LAYER)
            @text := get_control_par_str(get_ui_id($knob),$CONTROL_PAR_TEXT)
            @label := get_control_par_str(get_ui_id($knob),$CONTROL_PAR_LABEL)
            @help := get_control_par_str(get_ui_id($knob),$CONTROL_PAR_HELP)
        end on",
        &[],
    );
    assert_eq!((cell(&rt, 0), cell(&rt, 1), cell(&rt, 2)), (16, 31, 3));
    for (index, expected) in ["changed", "label", "help"].into_iter().enumerate() {
        assert_eq!(
            rt.script_text(rt.active_plan(), ScriptInstanceId(0), index as u32)
                .unwrap()
                .as_str(),
            expected
        );
    }
}

#[test]
fn menu_control_properties_report_selected_index_and_live_item_count() {
    let rt = run(
        "on init declare ui_menu $m add_menu_item($m,\"first\",17) add_menu_item($m,\"second\",93)
        $m := 93 declare $id declare $selected declare $count declare $index declare $changed end on
        on note
            $id := get_ui_id($m)
            $selected := get_control_par($id,$CONTROL_PAR_VALUE)
            $count := get_control_par($id,$CONTROL_PAR_NUM_ITEMS)
            $index := get_control_par($id,$CONTROL_PAR_SELECTED_ITEM_IDX)
            set_menu_item_value($id,1,101)
            $changed := get_control_par($id,$CONTROL_PAR_VALUE)
        end on",
        &[],
    );
    assert_eq!(
        (cell(&rt, 1), cell(&rt, 2), cell(&rt, 3), cell(&rt, 4)),
        (1, 2, 1, -1)
    );
}

#[test]
fn real_xy_getters_read_authored_and_live_indexed_values() {
    let rt = run(
        "on init declare ui_xy ?xy[2] declare ~before declare ~after
        set_control_par_real_arr(get_ui_id(?xy),$CONTROL_PAR_VALUE,0.25,0) end on
        on note
            ~before := get_control_par_real_arr(get_ui_id(?xy),$CONTROL_PAR_VALUE,0)
            set_control_par_real_arr(get_ui_id(?xy),$CONTROL_PAR_VALUE,0.75,1)
            ~after := get_control_par_real_arr(get_ui_id(?xy),$CONTROL_PAR_VALUE,1)
        end on",
        &[],
    );
    assert_eq!(f64::from_bits(cell(&rt, 2) as u64), 0.25);
    assert_eq!(f64::from_bits(cell(&rt, 3) as u64), 0.75);
}

#[test]
fn init_font_ids_keep_v1_numeric_and_invalid_name_semantics() {
    let rt = run("on init declare $font declare $empty declare $path declare $out
        $font := get_font_id(\"12\") $empty := get_font_id(\"\") $path := get_font_id(\"foo/bar\") end on
        on note $out := $font end on", &[]);
    assert_eq!(
        (cell(&rt, 0), cell(&rt, 1), cell(&rt, 2), cell(&rt, 3)),
        (12, 0, 0, 12)
    );
}

#[test]
fn init_menu_control_value_reports_index_without_changing_item_value() {
    let rt = run(
        "on init declare ui_menu $m add_menu_item($m,\"first\",17) add_menu_item($m,\"second\",93)
        $m := 93 declare $index := get_control_par(get_ui_id($m),$CONTROL_PAR_VALUE)
        declare $selected := get_control_par(get_ui_id($m),$CONTROL_PAR_SELECTED_ITEM_IDX)
        declare $raw declare $count := get_control_par(get_ui_id($m),$CONTROL_PAR_NUM_ITEMS) end on
        on note $raw := $m end on",
        &[],
    );
    assert_eq!(
        (cell(&rt, 0), cell(&rt, 1), cell(&rt, 2), cell(&rt, 3)),
        (1, 1, 93, 2)
    );
}

#[test]
fn named_ui_text_read_modify_write_emits_the_same_value_it_stores() {
    let mut rt = run(
        "on init declare ui_knob $knob(0,100,1) set_text($knob,\"A\") declare @text end on
        on note set_text($knob,get_control_par_str(get_ui_id($knob),$CONTROL_PAR_TEXT) & \"!\")
        @text := get_control_par_str(get_ui_id($knob),$CONTROL_PAR_TEXT) end on",
        &[],
    );
    assert_eq!(
        rt.script_text(rt.active_plan(), ScriptInstanceId(0), 0)
            .unwrap()
            .as_str(),
        "A!"
    );
    let mut effects = 0;
    rt.drain_effects(|effect| {
        if let Some(text) = &effect.text {
            assert_eq!(text.as_str(), "A!");
            effects += 1;
        }
        true
    });
    assert_eq!(effects, 1);
}
