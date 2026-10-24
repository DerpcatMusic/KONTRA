//! Equivalence contract: the Luau path must reproduce the interpreter's `on init`
//! state and the frame at which each `wait` resumes.
use super::{host::Host, lower};
use sampler_core::{Envelope, Input, Playback, Pcm, Prepared, Protocol, Region, Runtime, ScriptInstanceId};

/// Run `source` on both paths and compare every script cell, text and engine write.
fn assert_init_equivalent(source: &str) {
    let lowered = lower(source).unwrap_or_else(|e| panic!("front end: {e}"));
    if let Err(e) = super::check_init(&lowered) {
        panic!("{e}\n{}", lowered.compiled.source);
    }
}

#[test]
fn integer_arithmetic_wraps_like_the_interpreter() {
    assert_init_equivalent(
        "on init
        declare $a := 2147483647
        declare $b := -2147483648
        declare $c := 0
        declare $d := 0
        declare $e := 0
        declare $f := 0
        declare $g := 0
        declare $h := 0
        declare $i := 0
        declare $j := 0
        declare $k := 0
        declare $l := 0
        $c := $a + 1
        $d := $b - 1
        $e := $a * $a
        $f := $b / -1
        $g := $b mod 7
        $h := 7 / 0
        $i := $a .and. 1234
        $j := $b .or. 5
        $k := $a .xor. $b
        $l := -$b
        end on",
    );
}

#[test]
fn multiplication_and_division_match_on_extreme_operands() {
    assert_init_equivalent(
        "on init
        declare $x := 65537
        declare $y := -65535
        declare $p := 0
        declare $q := 0
        declare $r := 0
        declare $s := 0
        $p := $x * $y
        $q := (-2147483647 - 1) / 3
        $r := 2147483647 * 2147483647
        $s := -1234567 mod -1000
        end on",
    );
}

#[test]
fn real_arithmetic_and_truncating_stores() {
    assert_init_equivalent(
        "on init
        declare ~r := 1.5
        declare $n := 0
        declare ~m := 0.0
        declare ~q := 0.0
        declare ~z := 0.0
        declare ~w := 0.0
        declare ~big := 0.0
        ~m := ~r * 0.0
        ~q := ~r * 2.25 + ~r
        ~z := -7.9
        ~w := ~z
        ~big := 1.0e30
        end on",
    );
}

#[test]
fn control_flow_and_inc_dec() {
    // Kontakt rejects `call` in on init, so the loop body is inlined here.
    assert_init_equivalent(
        "on init
        declare $i := 0
        declare $s := 0
        declare $t := 0
        while ($i < 10)
          inc($i)
          if ($i mod 2 = 0) continue end if
          $t := $t + $i
        end while
        while (1 = 1)
          inc($s)
          if ($s = 5) exit end if
        end while
        select($t)
        case 0 $t := 100
        case 1 to 20 $t := $t + 1
        case 21 to 99 $t := $t + 2
        end select
        end on",
    );
}

#[test]
fn arrays_fill_and_out_of_range_access() {
    assert_init_equivalent(
        "on init
        declare %a[4] := (1, 2)
        declare ?r[3] := (0.5)
        declare $x := 0
        declare ~y := 0.0
        %a[1] := 9
        %a[7] := 5
        $x := %a[2] + %a[9]
        ~y := ?r[1] * 4.0
        ?r[0] := ?r[0] + 0.25
        inc(%a[3])
        end on",
    );
}

#[test]
fn engine_parameter_writes_return_minus_one() {
    assert_init_equivalent(
        "on init
        declare $r := 0
        $r := set_engine_par(0, 12, 3, 4, 5)
        end on",
    );
}

fn runtime(source: &str, rate: u32) -> Runtime {
    let script = crate::compile(
        source,
        rate,
        crate::Limits { source_bytes: 4096, instructions: 4096, variables: 8, array_cells: 16 },
        &[],
    )
    .unwrap();
    let plan = script
        .bind(
            Prepared::new(
                rate,
                vec![Pcm::new(rate, Box::from([[1.; 2]; 256])).unwrap()],
                vec![Region {
                    sample: 0,
                    key_low: 60,
                    key_high: 61,
                    root_key: None,
                    velocity_low: 0.,
                    velocity_high: 1.,
                    gain: 1.,
                    envelope: Envelope::default(),
                    playback: Playback::default(),
                }],
                2,
            )
            .unwrap(),
        )
        .unwrap();
    let cells = plan.behavior_local_count() * 2;
    Runtime::new(
        plan,
        sampler_core::Limits {
            notes: 4,
            channels: 0,
            performances: 1,
            families: 4,
            expressions: 4,
            voices: 4,
            decisions: 0,
            commands: 8,
            behaviors: 2,
            behavior_fuel: 64,
            behavior_cells: cells,
            note_cells: 4,
        },
    )
    .unwrap()
}

fn input() -> Input {
    Input { protocol: Protocol::Clap, port: 0, group: 0, channel: 0, key: 60, external_id: Some(1) }
}

/// Frame at which the interpreter sets `$x` (cell 0) after a note at frame 0.
fn interpreter_frame(source: &str, rate: u32, limit: u64) -> Option<u64> {
    let mut rt = runtime(source, rate);
    rt.trigger(input(), 60, 1.).unwrap();
    let plan = rt.active_plan();
    let mut block = vec![[0.; 2]; 1];
    let mut frame = 0;
    while frame < limit {
        rt.render(&mut block).unwrap();
        frame += 1;
        if rt.script_cell(plan, ScriptInstanceId(0), 0) == Ok(1) {
            return Some(frame);
        }
    }
    None
}

/// Frame at which the Luau host sets cell 0, rendered one frame at a time.
fn luau_frame(source: &str, rate: u32, limit: u64) -> Option<u64> {
    let lowered = lower(source).unwrap();
    assert!(lowered.compiled.is_complete(), "{:?}", lowered.compiled.unsupported);
    let mut host = Host::new(&lowered.compiled, lowered.cells, lowered.texts, lowered.note_cells, rate).unwrap();
    host.init().unwrap();
    host.note_on(1, 60, 1.).unwrap();
    for rendered in 1..=limit {
        host.advance_to(rendered).unwrap();
        if host.cells(lowered.cells)[0] == 1.0 {
            return Some(rendered);
        }
    }
    None
}

#[test]
fn wait_resumes_on_the_interpreter_frame() {
    // 1000 us at 48 kHz is exactly 48 frames: the write lands in frame index 48, so
    // 49 frames have been rendered when it is first visible. 333 us x3 = 48 frames too.
    for (source, expected) in [
        ("on init declare $x := 0 end on
          on note wait(1000) $x := 1 end on", 49),
        ("on init declare $x := 0 end on
          on note wait(333) wait(333) wait(333) $x := 1 end on", 49),
    ] {
        let interp = interpreter_frame(source, 48_000, 200).expect("interpreter sets the cell");
        let luau = luau_frame(source, 48_000, 200).expect("luau sets the cell");
        assert_eq!(interp, luau, "{source}");
        assert_eq!(interp, expected, "{source}");
    }
}

#[test]
fn polyphonic_wait_keeps_each_note_on_its_own_timeline() {
    let source = "on init declare $x := 0 end on
        on note wait(2000) $x := $x + 1 end on";
    let lowered = lower(source).unwrap();
    let mut host = Host::new(&lowered.compiled, lowered.cells, lowered.texts, lowered.note_cells, 48_000).unwrap();
    host.init().unwrap();
    host.note_on(1, 60, 1.).unwrap();
    host.advance_to(50).unwrap();
    host.note_on(2, 62, 1.).unwrap();
    host.advance_to(96).unwrap();
    let lowered = lower(source).unwrap();
    assert_eq!(host.cells(lowered.cells)[0], 0.0, "first note's wait lands in frame 96");
    host.advance_to(97).unwrap();
    assert_eq!(host.cells(lowered.cells)[0], 1.0, "first note's wait lands in frame 96");
    host.advance_to(147).unwrap();
    assert_eq!(host.cells(lowered.cells)[0], 2.0, "second note (started at 50) lands in frame 146");
}

/// Block size must not move any resumption: the same event trace at any block split
/// yields identical cell state at each block boundary.
#[test]
fn resumption_frames_do_not_depend_on_block_size() {
    let source = "on init declare $x := 0 end on
        on note wait(500) $x := $x + 1 wait(700) $x := $x + 10 end on";
    let lowered = lower(source).unwrap();
    let mut seen = Vec::new();
    for block in [32_u64, 64, 512, 4096] {
        let mut host = Host::new(&lowered.compiled, lowered.cells, lowered.texts, lowered.note_cells, 48_000).unwrap();
        host.init().unwrap();
        host.note_on(1, 60, 1.).unwrap();
        let mut frame = 0;
        let mut trace = Vec::new();
        while frame < 4096 {
            frame = (frame + block).min(4096);
            host.advance_to(frame).unwrap();
            trace.push((frame, host.cells(lowered.cells)[0]));
        }
        seen.push(trace);
    }
    // At 4096 frames every size agrees. The first write lands in index 24 (500 us) and the
    // second in index 58 (500 + 700 us = 24 + 34 frames); index i is visible after advance_to(i + 1).
    for trace in &seen {
        assert_eq!(trace.last().unwrap().1, 11.0);
    }
    let fine = lowered_fine_trace(&lowered);
    assert_eq!(fine.iter().position(|v| *v == 1.0), Some(24));
    assert_eq!(fine.iter().position(|v| *v == 11.0), Some(58));
}

fn lowered_fine_trace(lowered: &super::Lowered) -> Vec<f64> {
    let mut host = Host::new(&lowered.compiled, lowered.cells, lowered.texts, lowered.note_cells, 48_000).unwrap();
    host.init().unwrap();
    host.note_on(1, 60, 1.).unwrap();
    (1..=100)
        .map(|frame| {
            host.advance_to(frame).unwrap();
            host.cells(lowered.cells)[0]
        })
        .collect()
}



#[test]
fn ui_controls_clamp_and_read_back_like_the_interpreter() {
    assert_init_equivalent(
        "on init
        declare ui_slider $gain (0, 100)
        declare ui_knob $k (-5, 5)
        declare ui_switch $sw
        declare $a := 0
        declare $b := 0
        declare $c := 0
        declare $id := 0
        $gain := 250
        $k := -9
        $sw := 3
        inc($sw)
        $a := $gain
        $id := get_ui_id($gain)
        set_control_par($id, $CONTROL_PAR_VALUE, 42)
        set_control_par($id, $CONTROL_PAR_HIDE, 1)
        $b := get_control_par($id, $CONTROL_PAR_VALUE)
        $c := get_control_par($id, $CONTROL_PAR_MAX_VALUE) + get_control_par($id, $CONTROL_PAR_MIN_VALUE) + get_control_par($id, $CONTROL_PAR_HIDE)
        end on",
    );
}

#[test]
fn control_properties_are_mirrored_for_non_controls_too() {
    assert_init_equivalent(
        "on init
        declare $id := 0
        declare $t := 0
        declare $n := 0
        $id := 12345
        set_control_par($id, $CONTROL_PAR_LABEL, 7)
        set_control_par_str($id, $CONTROL_PAR_TEXT, \"hello\")
        set_control_par_real($id, $CONTROL_PAR_WIDTH, 2.9)
        $t := get_control_par($id, $CONTROL_PAR_LABEL)
        $n := get_control_par($id, $CONTROL_PAR_WIDTH) + get_control_par($id, $CONTROL_PAR_HEIGHT)
        end on",
    );
}

/// Run `on init`, then a user change of `ui` to `value`, on both paths; compare the state.
fn assert_ui_equivalent(source: &str, ui: usize, value: i32) {
    let lowered = lower(source).unwrap_or_else(|e| panic!("front end: {e}"));
    if let Err(e) = super::check_ui(&lowered, ui, value) {
        panic!("{e}\n{}", lowered.compiled.source);
    }
}

#[test]
fn ui_control_and_update_callbacks_dispatch_like_the_runtime() {
    let source = "on init
        declare ui_slider $gain (0, 100)
        declare ui_knob $k (-5, 5)
        declare ui_switch $sw
        declare $seen := 0
        declare $last := 0
        declare $ids := 0
        end on
        on ui_control($gain)
          $seen := $seen + 1
          $last := $gain + $seen
        end on
        on ui_control($sw)
          $ids := $ids + 100
        end on
        on ui_update
          $ids := $ids + 1000
        end on
        on ui_controls
          $ids := $ids + $NI_UI_ID
        end on";
    for (ui, value) in [(0, 250), (0, -3), (1, 4), (2, 1)] {
        assert_ui_equivalent(source, ui, value);
    }
}

#[test]
fn ui_change_with_wait_is_refused_not_guessed() {
    let source = "on init
        declare ui_slider $gain (0, 100)
        declare $x := 0
        end on
        on ui_control($gain)
          wait(1000)
          $x := 1
        end on";
    let lowered = lower(source).unwrap();
    assert!(super::check_ui(&lowered, 0, 5).unwrap_err().contains("waits"));
}

#[test]
fn pgs_keys_and_script_title_match_the_interpreter() {
    assert_init_equivalent(
        "on init
        declare $a := 0
        declare $b := 0
        declare $c := 0
        pgs_create_key(TEST, 4)
        pgs_set_key_val(TEST, 2, 77)
        pgs_set_key_val(TEST, 9, 5)
        pgs_set_key_val(TEST, -1, 5)
        $a := pgs_get_key_val(TEST, 2) + pgs_get_key_val(TEST, 9)
        $b := pgs_get_key_val(MISSING, 0)
        if (pgs_key_exists(TEST)) $b := $b + 10 end if
        if (pgs_key_exists(MISSING)) $b := $b + 100 end if
        pgs_create_key(TEST, 99)
        pgs_create_key(BIG, 1000)
        $c := pgs_get_key_val(TEST, 3) + pgs_get_key_val(BIG, 255) + pgs_get_key_val(BIG, 256)
        pgs_create_str_key(NAME)
        pgs_set_str_key_val(NAME, \"hello\")
        pgs_set_str_key_val(OTHER, \"lost\")
        set_script_title(\"Demo\")
        end on",
    );
}

#[test]
fn ui_record_builtins_match_the_interpreter() {
    assert_init_equivalent(
        "on init
        declare ui_knob $k (0, 10)
        declare ui_slider $s (0, 100)
        declare $n := 0
        set_text($k, \"text\")
        set_knob_label($k, \"label\")
        set_control_help($k, \"help\")
        add_text_line($k, \"one\")
        add_text_line($k, \"two\")
        set_knob_unit($k, 3)
        set_knob_defval($s, 40)
        hide_part($s, 1)
        move_control($k, 10, 20)
        move_control_px($k, 500, 300)
        move_control_px($s, -1, 5)
        set_ui_height_px(300)
        set_ui_width_px(400)
        set_ui_height(4)
        set_skin_offset(2)
        make_perfview
        message(\"hi\")
        message(\"there\")
        expose_controls
        $n := 1
        end on",
    );
}

#[test]
fn integer_builtins_match_the_interpreter() {
    // in_range and pgs_key_exists return booleans: Kontakt rejects them as numbers
    // (conformance `bool_inrange_arith`, `bool_pgs_arith`), so they are tested with `if`.
    assert_init_equivalent(
        "on init
        declare $a := 0
        declare $b := 0
        declare $c := 0
        declare $d := 0
        declare $e := 0
        declare $f := 0
        declare $g := 0
        declare $h := 0
        declare $i := 0
        declare $j := 0
        declare $k := 0
        declare %t[7] := (1)
        declare $l := 0
        $a := abs(-2147483648)
        $b := sgn(-17) + sgn(0) * 10 + sgn(3) * 100
        $c := sh_left(3, 33) + sh_right(-64, 3) + sh_left(1, -1)
        $d := msb(-1000) * 1000 + lsb(-1000)
        $e := ms_to_ticks(2147483647) + ticks_to_ms(-2000)
        if (in_range(5, 1, 5)) $f := $f + 1 end if
        if (in_range(6, 1, 5)) $f := $f + 10 end if
        if (in_range(-1, -1, 0)) $f := $f + 100 end if
        $g := min(3, -4) + max(3, -4) * 10
        $h := real_to_int(-7.9) + int(2.5) * 10 + real_to_int(1.0e30)
        $i := num_elements(%t) + 0
        $j := sgn(-0.0) + abs(-3) * 10
        $k := min(3, 2) + ($j mod 3)
        $l := 0
        end on",
    );
}

#[test]
fn real_builtins_match_the_interpreter() {
    assert_init_equivalent(
        "on init
        declare ~a := 0.0
        declare ~b := 0.0
        declare ~c := 0.0
        declare ~d := 0.0
        declare ~e := 0.0
        declare ~f := 0.0
        declare ~g := 0.0
        declare ~h := 0.0
        declare ~i := 0.0
        declare ~j := 0.0
        declare ~k := 0.0
        declare $n := 0
        ~a := round(2.5) + round(-2.5) * 10.0 + round(0.49999999999999994)
        ~b := sqrt(2.0) + exp(1.5) + log(10.0) + log10(1000.0)
        ~c := sin(0.5) + cos(0.5) + tan(0.5) + atan(0.5) + asin(0.5) + acos(0.5)
        ~d := pow(2.0, 0.5) + floor(-1.5) + ceil(-1.5)
        ~e := abs(-1.25) + int_to_real(7) + real(-3)
        ~f := min(1.5, -2.5) + max(1.5, -2.5)
        ~g := 1.0 / 3.0 + int_to_real(int(-0.5))
        ~h := log(0.0) * 0.0
        ~i := int_to_real(sgn(-2.5))
        ~j := int_to_real(in_range(0.25, 0.0, 0.5))
        ~k := ~a + ~b
        $n := sgn(~k)
        end on",
    );
}

#[test]
fn search_and_sort_match_the_interpreter() {
    assert_init_equivalent(
        "on init
        declare %a[6] := (5, 3, 9, 3, -1, 7)
        declare %b[4] := (4, 2)
        declare $i := 0
        declare $j := 0
        declare $k := 0
        declare $m := 0
        $i := search(%a, 3)
        $j := search(%a, 3, 2, 5)
        $k := search(%a, 42)
        $m := search(%b, 0)
        sort(%a, 0, 1, 4)
        sort(%b, 1)
        end on",
    );
}

#[test]
fn messages_are_recorded_in_call_order() {
    assert_init_equivalent(
        "on init
        declare $x := 4
        message(\"first\")
        message($x)
        message(\"\")
        end on",
    );
}

#[test]
fn key_switch_state_matches_the_interpreter() {
    assert_init_equivalent(
        "on init
        declare $c := 0
        declare $t := 0
        declare $n := 0
        set_key_color(60, 5)
        set_key_type(60, 2)
        set_key_name(60, \"C3\")
        set_key_pressed(61, 1)
        set_key_color(200, 7)
        set_key_pressed_support(1)
        set_keyrange(36, 48, \"low\")
        set_keyrange(40, 80, \"mid\")
        set_keyrange(90, 100, \"high\")
        remove_keyrange(44)
        $c := get_key_color(60) + get_key_color(0) * 100
        $t := get_key_type(60) + get_key_type(1) * 10
        $n := 0
        end on",
    );
}

/// Records the engine calls the Luau host makes, shared with the test.
struct Log(std::rc::Rc<std::cell::RefCell<Vec<crate::engine::Call>>>);

impl crate::engine::EngineSink for Log {
    fn play(&mut self, request: crate::engine::Play) -> Option<crate::engine::EventId> {
        self.0.borrow_mut().push(crate::engine::Call::Play(request));
        Some(2)
    }
    fn note_off(&mut self, event: crate::engine::EventId, delay: Option<u64>) -> bool {
        self.0.borrow_mut().push(crate::engine::Call::NoteOff { event, delay });
        true
    }
    fn set_param(&mut self, event: crate::engine::EventId, target: sampler_core::ModTarget, value: i64, relative: bool) -> bool {
        self.0.borrow_mut().push(crate::engine::Call::Param { event, target, value, relative });
        true
    }
    fn suppress_attack(&mut self, event: crate::engine::EventId) -> bool {
        self.0.borrow_mut().push(crate::engine::Call::SuppressAttack { event });
        true
    }
}

#[test]
fn play_note_and_note_off_reach_the_engine_sink() {
    use crate::engine::{Call, Duration, Play};
    let source = "on init declare $c := 0 end on
        on note
        $c := play_note(61, 100, 0, 250000)
        $c := play_note(63, 90, 0, -1)
        $c := play_note(62, 0, 0, 1000)
        note_off($EVENT_ID, 1000)
        end on";
    let lowered = lower(source).unwrap_or_else(|e| panic!("front end: {e}"));
    let mut host = Host::new(&lowered.compiled, lowered.cells, lowered.texts, lowered.note_cells, 48_000).unwrap();
    let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    host.set_sink(Box::new(Log(log.clone())));
    host.init().unwrap();
    host.note_on(1, 60, 100.0).unwrap();
    // Velocity 0 is outside KSP's 1..127 for play_note: refused without a call.
    assert_eq!(
        *log.borrow(),
        vec![
            Call::Play(Play { parent: 1, key: 61, velocity: 100.0 / 127.0, duration: Duration::Frames(12_000) }),
            Call::Play(Play { parent: 1, key: 63, velocity: 90.0 / 127.0, duration: Duration::Gate }),
            Call::NoteOff { event: 1, delay: Some(48) },
        ],
    );
}

#[test]
fn persistence_declarations_and_the_load_time_persistence_body_match_the_interpreter() {
    // `on persistence_changed` runs right after init on a fresh instrument; the saved-value
    // restore is a no-op there, so the declarations only mark storage.
    assert_init_equivalent(
        "on init declare $a := 3 make_persistent($a) declare ~r := 1.5 make_instr_persistent(~r) \
         declare @s @s := \"x\" make_persistent(@s) read_persistent_var($a) $a := $a * 2 end on \
         on persistence_changed $a := $a + 5 ~r := ~r * 2.0 end on",
    );
}

#[test]
fn menu_items_and_selection_match_the_interpreter() {
    assert_init_equivalent(
        "on init declare ui_menu $m add_menu_item($m, \"a\", 20) add_menu_item($m, \"b\", 80) \
         declare $id := get_ui_id($m) set_menu_item_str($id, 1, \"bb\") \
         set_menu_item_visibility($id, 0, 0) set_menu_item_value($id, 1, 90) $m := 90 \
         declare $n := get_num_menu_items($id) declare $v := get_menu_item_value($id, 1) \
         declare $vis := get_menu_item_visibility($id, 0) declare $sel := get_control_par($id, $CONTROL_PAR_SELECTED_ITEM_IDX) \
         declare $bad := get_menu_item_value($id, 7) declare @t := get_menu_item_str($id, 1) end on",
    );
}

#[test]
fn host_requests_match_the_interpreter() {
    assert_init_equivalent(
        "on init declare %a[4] declare ?r[2] declare $x := 2 save_array(%a, 1) set_ui_color(3) \
         set_voice_limit(4, 8) load_ir_sample(\"ir.wav\", 1, 0) fs_navigate(0, 1) \
         set_nks_nav_name(1, 2, \"n\") set_nks_nav_par(1, 2, 3) reset_nks_nav() \
         attach_level_meter(1, 2, 3, 4, 5) save_array(?r, 0) load_native_ui(\"x\") end on",
    );
}

#[test]
fn event_volume_tune_pan_and_ignore_reach_the_engine_sink() {
    use crate::engine::Call;
    use sampler_core::ModTarget;
    let source = "on init declare $c := 0 end on
        on note
        change_vol($EVENT_ID, -6000, 0)
        change_tune($EVENT_ID, 1200, 1)
        change_pan(7, -250, 1)
        ignore_event($EVENT_ID)
        end on";
    let lowered = lower(source).unwrap_or_else(|e| panic!("front end: {e}"));
    assert!(lowered.compiled.is_complete(), "{:?}", lowered.compiled.unsupported);
    let mut host = Host::new(&lowered.compiled, lowered.cells, lowered.texts, lowered.note_cells, 48_000).unwrap();
    let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    host.set_sink(Box::new(Log(log.clone())));
    host.init().unwrap();
    host.note_on(1, 60, 100.0).unwrap();
    assert_eq!(
        *log.borrow(),
        vec![
            Call::Param { event: 1, target: ModTarget::Decibels, value: -6000, relative: false },
            Call::Param { event: 1, target: ModTarget::Pitch, value: 1200, relative: true },
            Call::Param { event: 7, target: ModTarget::Pan, value: -250, relative: true },
            Call::SuppressAttack { event: 1 },
        ],
    );
}
