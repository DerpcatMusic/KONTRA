use super::*;

fn text(v: &Value) -> String {
    match v {
        Value::Int(n) => n.to_string(),
        Value::Real(n) => n.to_string(),
        Value::Text(s) => s.clone(),
        Value::Array(_) => "[array]".into(),
    }
}

fn prop(ui: &Interface, control: usize, name: &str) -> String {
    text(&ui.controls[control].properties[name])
}

fn label(source: &str) -> String {
    prop(&initialize(source, 0, 0).unwrap(), 0, "$CONTROL_PAR_TEXT")
}

// ---- Initialization (ported from the init-only interpreter) ---------------------------

#[test]
fn control_ids_follow_declaration_order() {
    let source = "on init\ndeclare const $prefix:=0\ndeclare ui_knob $first(0,100,1)\ndeclare $gap\ndeclare ui_knob $second(0,100,1)\ndeclare ui_label $label(1,1)\nset_control_par(32769,$CONTROL_PAR_VALUE,17)\nset_control_par(get_ui_id($first)+2,$CONTROL_PAR_VALUE,29)\nset_text($label,get_ui_id($prefix) & \":\" & get_control_par(get_ui_id($second),$CONTROL_PAR_VALUE))\nset_text($label,get_control_par_str(get_ui_id($label),$CONTROL_PAR_TEXT) & \":ok\")\nset_knob_label($first,\"dB\")\nset_knob_unit($first,$KNOB_UNIT_DB)\nset_control_help($first,\"Mic volume\")\nend on";
    let ui = initialize(source, 0, 0).unwrap();
    assert_eq!(prop(&ui, 0, "$CONTROL_PAR_LABEL"), "dB");
    assert_eq!(prop(&ui, 0, "$CONTROL_PAR_UNIT"), "$KNOB_UNIT_DB");
    assert_eq!(prop(&ui, 0, "$CONTROL_PAR_HELP"), "Mic volume");
    assert_eq!(prop(&ui, 0, "$CONTROL_PAR_VALUE"), "17");
    assert_eq!(prop(&ui, 1, "$CONTROL_PAR_VALUE"), "29");
    assert_eq!(prop(&ui, 2, "$CONTROL_PAR_TEXT"), "32768:29:ok");
    for bad in [
        "set_control_par(get_ui_id($gap),$CONTROL_PAR_VALUE,1)",
        "set_control_par(0,$CONTROL_PAR_VALUE,1)",
    ] {
        // Like Kontakt, a bad control ID is reported but does not stop the script.
        let ui = initialize(&source.replace("end on", &format!("{bad}\nend on")), 0, 0).unwrap();
        assert!(
            ui.diagnostics
                .iter()
                .any(|d| d.contains("ID does not refer")),
            "{bad}"
        );
    }
}

#[test]
fn shared_host_services_are_scoped_and_transactional() {
    let mut host = HostState::default();
    let first = "on init\npgs_create_key(MIC_LEVEL,2)\npgs_set_key_val(MIC_LEVEL,1,73)\npgs_create_str_key(PRESET_NAME)\npgs_set_str_key_val(PRESET_NAME,\"Warm\")\nset_key_pressed(60,1)\nset_key_pressed_support(1)\nset_key_pressed(61,1)\nset_key_name(61,\"Keyswitch\")\nset_key_color(61,$KEY_COLOR_RED)\nset_key_type(61,$NI_KEY_TYPE_CONTROL)\nset_listener($NI_SIGNAL_TIMER_MS,1000)\nchange_listener_par($NI_SIGNAL_TIMER_MS,2000)\nend on";
    let ui = initialize_with_host(first, 0, 0, &mut host).unwrap();
    assert_eq!(ui.listeners["$NI_SIGNAL_TIMER_MS"], 2000);
    assert!(!host.keyboard.contains_key(&60));
    assert!(host.keyboard[&61].pressed);
    assert_eq!(host.keyboard[&61].name, "Keyswitch");
    let next = "on init\npgs_create_key(MIC_LEVEL,2)\ndeclare ui_label $label(1,1)\nset_text($label,pgs_get_str_key_val(PRESET_NAME) & pgs_get_key_val(MIC_LEVEL,1) & get_key_name(61) & get_key_triggerstate(61))\nend on";
    let ui = initialize_with_host(next, 0, 0, &mut host).unwrap();
    assert_eq!(prop(&ui, 0, "$CONTROL_PAR_TEXT"), "Warm73Keyswitch1");
    assert!(initialize(next, 0, 0).is_err());
    let failed = "on init\npgs_set_key_val(MIC_LEVEL,1,0)\nset_key_pressed_support(2)\nend on";
    assert!(initialize_with_host(failed, 0, 0, &mut host).is_err());
    assert_eq!(host.pgs_ints[0].1[1], 73);
    for bad in [
        "pgs_create_key(TOO_BIG,257)",
        "pgs_set_key_val(MIC_LEVEL,2,1)",
        "pgs_create_key(MIC_LEVEL,3)",
        "set_key_pressed_support(2)",
        "set_key_name(128,\"bad\")",
        "set_listener($NI_SIGNAL_TIMER_MS,999)",
        "set_listener($NI_SIGNAL_TIMER_BEAT,25)",
    ] {
        assert!(
            initialize_with_host(&format!("on init\n{bad}\nend on"), 0, 0, &mut host).is_err(),
            "{bad}"
        );
    }
    let source = "on init\nset_listener($NI_SIGNAL_TIMER_BEAT,4)\nchange_listener_par($NI_SIGNAL_TIMER_BEAT,0)\nend on";
    assert!(
        initialize_with_host(source, 0, 0, &mut host)
            .unwrap()
            .listeners
            .is_empty()
    );
}

#[test]
fn real_expressions_and_arrays() {
    let source = "on init\ndeclare ?a[2] := (2.5, -3.0)\ndeclare ~x := ?a[0]*2.0\ndeclare ui_label $label(1,1)\nif (~x=5.0)\nset_text($label,int(round(~x+real(2))) & \":\" & int(abs(?a[1])))\nend if\nend on";
    assert_eq!(label(source), "7:3");
    // Like Kontakt's doubles, nonfinite reals are reported and kept; the
    // callback goes on. Integer conversion saturates.
    for (expression, shown) in [
        ("1.0/0.0", "inf"),
        ("sqrt(-1.0)", "NaN"),
        ("exp(1000.0)", "inf"),
        ("real(int(2147483648.0))", "2147483647"),
    ] {
        let ui = initialize(
            &format!("on init\ndeclare ~x := {expression}\ndeclare ui_label $l(1,1)\nset_text($l, ~x)\nend on"),
            0,
            0,
        )
        .unwrap();
        assert_eq!(prop(&ui, 0, "$CONTROL_PAR_TEXT"), shown, "{expression}");
        assert!(!ui.diagnostics.is_empty(), "{expression}");
    }
    assert!(initialize("on init\ndeclare ~x := 1.0+1\nend on", 0, 0).is_err());
    let ok = "on init\ndeclare ~x := 1.0e-3\ndeclare $bits := sh_right(sh_left(3.and.1,4),2)\ndeclare $cast := real_to_int(int_to_real(4))\nend on";
    assert!(initialize(ok, 0, 0).is_ok());
    let fill = "on init\ndeclare %a[4] := (1, 2)\ndeclare ui_label $l(1,1)\nset_text($l, %a[3] & min(4, 2) & max(1.5, 2.5))\nend on";
    assert_eq!(label(fill), "222.5");
    let special = "on init\ndeclare ui_label $l(1,1)\nset_text($l, int(~NI_MATH_PI * 100.0) & get_font_id(\"7\") & %GROUPS_SELECTED[0])\nend on";
    assert_eq!(label(special), "31470");
}

#[test]
fn whitespace_and_comments() {
    let source = "on\tinit\ndeclare{separator}ui_label $label(1,1)\ndeclare $n:=0\nwhile($n<1)\nif(1)\nselect($n)\ncase\t0\nset_text($label,\" keep  := spaces \")\nend\t select\nend  if\ninc($n)\nend\twhile\nend  on";
    assert_eq!(label(source), " keep  := spaces ");
    // `iffy` is a name, not `if`: an unknown function.
    assert!(
        initialize("on init\niffy(1)\nend on", 0, 0)
            .unwrap()
            .diagnostics
            .iter()
            .any(|d| d.contains("iffy"))
    );
}

#[test]
fn select_and_broken_callbacks() {
    let source = "on init\ndeclare ui_label $label(1,1)\ndeclare $n:=4\nselect ($n)\ncase 0\nmessage(0)\ncase 3 to 5\nselect (1)\ncase 1\nset_text($label,\"selected\")\nend select\ncase 4\nmessage(4)\nend select\nend on\nfunction unused\nunsupported syntax here\nend function\non note\nunsupported playback syntax\nend on";
    let ui = initialize(source, 0, 0).unwrap();
    assert_eq!(prop(&ui, 0, "$CONTROL_PAR_TEXT"), "selected");
    assert!(
        ui.diagnostics
            .iter()
            .any(|d| d.starts_with("Callback disabled"))
    );
    assert!(
        initialize(
            &source.replace("declare $n:=4", "call unused\ndeclare $n:=4"),
            0,
            0
        )
        .is_err()
    );
    let array = format!(
        "on init\ndeclare %a[1000] := ({})\nend on",
        vec!["1"; 1000].join(",")
    );
    assert!(initialize(&array, 0, 0).is_ok());
    let long = format!(
        "on init\ndeclare $x := {}\nend on",
        vec!["1"; 10000].join("+")
    );
    assert!(
        initialize(&long, 0, 0)
            .unwrap_err()
            .to_string()
            .contains("nesting limit")
    );
    assert!(initialize("on init\nend on\non note\non release\nend on", 0, 0).is_err());
    assert!(
        initialize(
            "on init\ncall loop\nend on\nfunction loop\ncall loop\nend function",
            0,
            0
        )
        .is_err()
    );
}

#[test]
fn a_menu_selects_its_first_item_for_init_to_read() {
    // Afflatus picks its wallpaper by its menu's value minus one in `on init`.
    let source = "on init\ndeclare !walls[2]\n!walls[0] := \"first\"\n!walls[1] := \"second\"\ndeclare ui_menu $look\nadd_menu_item($look, \"A\", 1)\nadd_menu_item($look, \"B\", 2)\nmake_persistent($look)\nread_persistent_var($look)\nset_control_par_str($INST_WALLPAPER_ID, $CONTROL_PAR_PICTURE, !walls[$look - 1])\nend on";
    assert_eq!(initialize(source, 0, 0).unwrap().wallpaper, "first");
    let chosen = source.replace("make_persistent($look)", "$look := 2");
    assert_eq!(initialize(&chosen, 0, 0).unwrap().wallpaper, "second", "a value it has stays");
}

#[test]
fn computed_ui_and_execution_limits() {
    let source = r#"on init
 declare $i
 declare %ids[2]
 declare ui_switch $a
 declare ui_switch $b
 declare @picture := "My" & " UI"
 set_control_par_str($INST_WALLPAPER_ID,$CONTROL_PAR_PICTURE,@picture)
 while ($i<2)
 %ids[$i] := get_ui_id($a)+$i
 set_control_par(%ids[$i],$CONTROL_PAR_POS_X,10+$i*90)
 set_control_par_str(%ids[$i],$CONTROL_PAR_TEXT,"Mic " & ($i+1))
 inc($i)
 end while
 if ($NUM_GROUPS>1 and $i=2)
 set_control_par(get_ui_id($b),$CONTROL_PAR_HIDE,$HIDE_WHOLE_CONTROL)
 end if
 end on"#;
    let inventory = requirements(&format!(
        "{source}\non note\nplay_note($EVENT_NOTE,100,0,-1)\nend on\n{{ fake_call() }}"
    ))
    .unwrap();
    assert_eq!(inventory["callbacks"], serde_json::json!(["init", "note"]));
    let calls = inventory["calls"].as_array().unwrap();
    assert!(calls.contains(&serde_json::json!("play_note")));
    assert!(!calls.contains(&serde_json::json!("fake_call")));
    let names =
        requirements("on init\nmake_perfview\nif (1 and (2))\nexit\nend if\nend on").unwrap();
    assert_eq!(names["calls"], serde_json::json!(["exit", "make_perfview"]));
    let ui = initialize(source, 2, 8).unwrap();
    assert_eq!(ui.wallpaper, "My UI");
    assert_eq!(prop(&ui, 1, "$CONTROL_PAR_POS_X"), "100");
    assert_eq!(prop(&ui, 1, "$CONTROL_PAR_TEXT"), "Mic 2");
    assert_eq!(prop(&ui, 1, "$CONTROL_PAR_HIDE"), "16");
    // A runaway `on init` is stopped, not the script: what it declared stays.
    let runaway = initialize("on init\ndeclare ui_knob $k(0,1,1)\nwhile (1)\nend while\nend on", 0, 0).unwrap();
    assert_eq!(runaway.controls.len(), 1);
    assert!(runaway.diagnostics.iter().any(|d| d.contains("budget")), "{:?}", runaway.diagnostics);
    let oob = initialize("on init\ndeclare %a[1]\n%a[2] := 1\nend on", 0, 0).unwrap();
    assert!(oob.diagnostics.iter().any(|d| d.contains("out of bounds")));
    // Unknown functions do nothing and yield 0; the rest of the script runs.
    let unknown = initialize("on init\ndeclare ui_label $l(1,1)\nunknown_function(1)\nset_text($l, unknown_value($l) & get_unknown_name() & \"!\")\nend on", 0, 0).unwrap();
    assert_eq!(prop(&unknown, 0, "$CONTROL_PAR_TEXT"), "0!");
    assert!(unknown.diagnostics.iter().any(|d| d.starts_with("Unsupported KSP function: unknown_function (line 3)")), "{:?}", unknown.diagnostics);
    // Integer division by zero is 0, reported, and init goes on.
    let div = initialize("on init\ndeclare $z\ndeclare $a := 1/$z\nend on", 0, 0).unwrap();
    assert!(div.diagnostics.iter().any(|d| d.contains("division by zero")));
    assert!(
        initialize(
            "on init\ndeclare @s := \"x\"\nwhile (1)\n@s := @s & @s\nend while\nend on",
            0,
            0
        )
        .is_err()
    );
    assert!(
        initialize(
            "on init\ndeclare ui_button $a\nmove_control($a,2147483647,-2147483647-1)\nend on",
            0,
            0
        )
        .is_ok()
    );
    let bits = "on init\ndeclare ui_label $x(1,1)\nset_text($x, ...\n (0FFh .and. 15) .or. (.not. 0FFFFFFF0h) & \":\" & 080000000h)\nend on";
    assert_eq!(label(bits), format!("15:{}", i32::MIN));
    // Large sources (the biggest shipped scripts are ~19 MB) initialize.
    let large = format!(
        "{{ {} }}\non init\nend on\non note\nplay_note(60,100,0,-1)\nend on",
        "x".repeat(17 << 20)
    );
    assert!(requirements(&large).is_ok());
    assert!(initialize(&large, 0, 0).is_ok());
    let called = "on init\ndeclare ui_label $a(1,1)\ncall label\nend on\nfunction label\nset_text($a,\"{quoted}\")\nend function";
    assert_eq!(label(called), "{quoted}");
}

#[test]
fn native_loops_match_interpreted_semantics() {
    let source = r#"on init
declare %a[8] := (5, 1, 3, 7, 3, 9, 2, 3)
declare %b[8]
declare $i
declare $n
declare $v := 3
declare @s
declare ui_label $l(1,1)
while ($i < 8)
 %b[$i] := %a[$i]
 inc($i)
end while
$i := 0
while ($i < 7)
 %b[$i + 1] := %b[$i]
 inc($i)
end while
$i := 0
while ($i < 8)
 if (%a[$i] = $v)
 inc($n)
 end if
 inc($i)
end while
$i := 0
while ($i <= 7)
 if (%a[$i] > 6 and $i > 3)
 @s := @s & $i
 end if
 inc($i)
end while
$i := 0
while ($i < 7)
 %a[$i] := %a[$i + 1]
 inc($i)
end while
set_text($l, %b[7] & ":" & $n & ":" & @s & ":" & %a[0] & %a[6] & %a[7])
end on"#;
    let setup = compile::Setup {
        groups: 0,
        outputs: 0,
    };
    assert_eq!(compile::compile(source, &setup).unwrap().loops.len(), 5);
    // A forward-unsafe copy spreads %b[0]; the shift is a memmove.
    assert_eq!(label(source), "5:3:5:133");
    let oob = "on init\ndeclare %a[4]\ndeclare $i\nwhile ($i < 5)\n%a[$i] := %a[0]\ninc($i)\nend while\nend on";
    let ui = initialize(oob, 0, 0).unwrap();
    assert!(ui.diagnostics.iter().any(|d| d.contains("out of bounds")));
    let spin = "on init\ndeclare %a[4]\ndeclare $i\nwhile (1)\n$i := 0\nwhile ($i < 4)\nif (%a[$i] = 1)\nend if\ninc($i)\nend while\nend while\nend on";
    let ui = initialize(spin, 0, 0).unwrap();
    assert!(ui.diagnostics.iter().any(|d| d.contains("budget")), "{:?}", ui.diagnostics);
}

#[test]
fn saved_persistence_decodes_every_kind() {
    let entries = [
        "$a 5",
        "%b 1 2 3",
        "~c 1.5",
        "@d two words",
        "!e x\ny\n",
        "$bad x",
    ]
    .map(String::from);
    let saved = saved_persistence(&entries);
    assert_eq!(saved["$a"], Value::Int(5));
    assert_eq!(
        saved["%b"],
        Value::Array(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
    );
    assert_eq!(saved["~c"], Value::Real(1.5));
    assert_eq!(saved["@d"], Value::Text("two words".into()));
    assert_eq!(
        saved["!e"],
        Value::Array(["x", "y", ""].map(|s| Value::Text(s.into())).to_vec())
    );
    assert!(!saved.contains_key("$bad"));
    let script = "on init\ndeclare %b[3]\nmake_persistent(%b)\nread_persistent_var(%b)\ndeclare ui_label $l(1,1)\nset_text($l, %b[2])\nend on";
    let mut engine = LogEngine::new(Vec::new(), 48_000.0);
    let (rt, _) = Runtime::with_scripts(&[script], &mut engine, 8, vec![saved]);
    assert_eq!(prop(&rt.interface(0), 0, "$CONTROL_PAR_TEXT"), "3");
}

// ---- Runtime --------------------------------------------------------------------------

struct Rig {
    rt: Runtime,
    engine: LogEngine,
}

impl Rig {
    fn new(scripts: &[&str]) -> Self {
        let mut engine = LogEngine::new(vec!["a".into(), "b".into(), "c".into()], 48_000.0);
        let (rt, errors) = Runtime::with_scripts(scripts, &mut engine, 8, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        assert!(rt.diagnostics().is_empty(), "{:?}", rt.diagnostics());
        engine.calls.clear();
        Self { rt, engine }
    }

    fn on(&mut self, at: u32, note: u8) -> &mut Self {
        self.rt.note_on(&mut self.engine, at, note, 100);
        self
    }

    fn off(&mut self, at: u32, note: u8) -> &mut Self {
        self.rt.note_off(&mut self.engine, at, note);
        self
    }

    fn block(&mut self, frames: u32) -> &mut Self {
        self.rt.process(&mut self.engine, frames);
        self.engine.block_start = self.rt.now();
        self
    }

    /// Compact engine log, e.g. `play 60@0 [0,1,2]`, `off 1@10`.
    fn log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.engine.calls)
            .into_iter()
            .filter_map(|c| match c {
                EngineCall::PlayNote {
                    time,
                    voice,
                    note,
                    groups,
                    length,
                    ..
                } => {
                    let sample = if length == NoteLength::Sample {
                        " sample"
                    } else {
                        ""
                    };
                    Some(format!("play {note}@{time} v{voice} {groups:?}{sample}"))
                }
                EngineCall::NoteOff { time, voice } => Some(format!("off v{voice}@{time}")),
                EngineCall::Fade { time, voice, fade } => {
                    Some(format!("fade v{voice}@{time} {fade:?}"))
                }
                EngineCall::SetPar {
                    voice, par, value, ..
                } => Some(format!("par v{voice} {par:?}={value}")),
                EngineCall::Controller { time, cc, value } => {
                    Some(format!("cc {cc}={value}@{time}"))
                }
                EngineCall::SetEnginePar { .. } => None,
            })
            .collect()
    }
}

const PASS: &str = "on init\nend on";

#[test]
fn events_pass_through_slots_in_order() {
    let transpose = "on init\nend on\non note\nchange_note($EVENT_ID, $EVENT_NOTE + 12)\nend on";
    let harmony = "on init\nend on\non note\nignore_event($EVENT_ID)\nplay_note($EVENT_NOTE + 1, $EVENT_VELOCITY, 0, -1)\nend on";
    let mut rig = Rig::new(&[transpose, PASS, harmony]);
    rig.on(0, 60).off(10, 60);
    assert_eq!(rig.log(), ["play 73@0 v1 [0, 1, 2]", "off v1@10"]);
    // Ignored in the first slot: later slots never see the note.
    let mut rig = Rig::new(&[
        "on init\nend on\non note\nignore_event($EVENT_ID)\nend on",
        harmony,
    ]);
    rig.on(0, 60).off(5, 60);
    assert!(rig.log().is_empty());
}

#[test]
fn legato_retriggers_on_overlapping_notes() {
    let legato = "on init\ndeclare $last := 0\nend on\non note\nignore_event($EVENT_ID)\nif ($last # 0)\nnote_off($last)\nend if\n$last := play_note($EVENT_NOTE, $EVENT_VELOCITY, 0, -1)\nend on\non release\nif ($NOTE_HELD = 0)\nend if\nend on";
    let mut rig = Rig::new(&[legato]);
    rig.on(0, 60).on(100, 62).off(120, 60).off(300, 62);
    assert_eq!(
        rig.log(),
        [
            "play 60@0 v1 [0, 1, 2]",
            "off v1@100",
            "play 62@100 v2 [0, 1, 2]",
            "off v2@300"
        ]
    );
}

/// A voice index divided by an off switch: integer division and modulo by zero
/// give 0 and the callback goes on to play, as in Kontakt.
#[test]
fn integer_division_by_zero_yields_zero_and_continues() {
    let script = "on init\ndeclare $split := 0\ndeclare %prev[4] := (-1)\ndeclare $voice\nend on\non note\nignore_event($EVENT_ID)\n$voice := $EVENT_VELOCITY / (128 / 2 * $split) + $EVENT_NOTE mod $split\nif (%prev[$voice] = -1)\n%prev[$voice] := $EVENT_NOTE\nplay_note($EVENT_NOTE, $EVENT_VELOCITY, 0, -1)\nend if\nend on";
    let mut rig = Rig::new(&[script]);
    rig.on(0, 60).block(16);
    assert_eq!(rig.log(), ["play 60@0 v1 [0, 1, 2]"]);
    let d = rig.rt.diagnostics();
    assert!(d.iter().all(|l| l.contains("division by zero")), "{d:?}");
}

#[test]
fn wait_resumes_sample_accurately() {
    let delayed = "on init\nend on\non note\nwait(10000)\nplay_note(72, 90, 0, 0)\nend on";
    let mut rig = Rig::new(&[delayed]);
    rig.on(16, 60);
    assert_eq!(rig.log(), ["play 60@16 v1 [0, 1, 2]"]);
    rig.block(256).block(256);
    // 10 ms at 48 kHz = 480 samples after the note at offset 16.
    assert_eq!(rig.log(), ["play 72@496 v2 [0, 1, 2] sample"]);
    let ticks = "on init\nend on\non note\nwait_ticks(960)\nplay_note(72, 90, 0, 0)\nend on";
    let mut rig = Rig::new(&[ticks]);
    rig.on(0, 60).block(24_000).block(24_000);
    assert_eq!(rig.log()[1], "play 72@24000 v2 [0, 1, 2] sample");
}

#[test]
fn fades_and_durations() {
    let script = "on init\ndeclare $id\nend on\non note\nfade_in($EVENT_ID, 2000)\nwait(1000)\nfade_out($EVENT_ID, 5000, 1)\n$id := play_note(40, 100, 0, 20000)\nend on";
    let mut rig = Rig::new(&[script]);
    rig.on(0, 60);
    assert_eq!(
        rig.log(),
        [
            "play 60@0 v1 [0, 1, 2]",
            "fade v1@0 In { duration_us: 2000 }"
        ]
    );
    rig.block(128);
    assert_eq!(
        rig.log(),
        [
            "fade v1@48 Out { duration_us: 5000, stop: true }",
            "play 40@48 v2 [0, 1, 2]"
        ]
    );
    // A positive duration releases the note after that many microseconds (960 samples).
    rig.block(2048);
    assert_eq!(rig.log(), ["off v2@1008"]);
}

#[test]
fn group_masks_and_voice_parameters() {
    let script = "on init\nend on\non note\ndisallow_group($ALL_GROUPS)\nallow_group(1)\nchange_vol($EVENT_ID, -6000, 0)\nwait(1)\nchange_tune($EVENT_ID, 100, 1)\nend on";
    let mut rig = Rig::new(&[script]);
    rig.on(0, 60).block(64);
    assert_eq!(rig.log(), ["play 60@0 v1 [1]", "par v1 TuneMc=100"]);
}

#[test]
fn event_par_array_addresses_the_current_event() {
    let script = "on init\nend on\non note\n%EVENT_PAR[$EVENT_PAR_1] := 5\nmessage(get_event_par($EVENT_ID, $EVENT_PAR_1) + %EVENT_PAR[$EVENT_PAR_1])\nend on";
    let mut rig = Rig::new(&[script]);
    rig.on(0, 60);
    assert_eq!(rig.rt.last_message(), "10");
}

#[test]
fn sample_length_notes_stay_addressable_and_recycle() {
    let script = "on init\ndeclare $id\nend on\non note\nignore_event($EVENT_ID)\n$id := play_note($EVENT_NOTE, 100, 0, 0)\nwait(1)\nmessage(event_status($id))\nend on";
    let mut rig = Rig::new(&[script]);
    rig.on(0, 60).block(64);
    assert_eq!(rig.rt.last_message(), "1");
    // One-shots are never released, yet the pool keeps working past its capacity.
    for i in 0..5000 {
        rig.on(0, (i % 100) as u8).off(0, (i % 100) as u8).block(64);
    }
    assert!(
        rig.rt.diagnostics().is_empty(),
        "{:?}",
        rig.rt.diagnostics()
    );
    assert_eq!(
        rig.log().iter().filter(|c| c.starts_with("play")).count(),
        5001
    );
}

#[test]
fn zone_id_is_zero_once_the_voice_is_gone() {
    let script = "on init\ndeclare $id\nend on\non note\nif ($EVENT_NOTE = 60)\n$id := $EVENT_ID\nend if\nmessage(get_event_par($EVENT_ID, $EVENT_PAR_ZONE_ID))\nwait(1)\nmessage(get_event_par($id, $EVENT_PAR_ZONE_ID))\nend on";
    let mut rig = Rig::new(&[script]);
    rig.on(0, 60);
    // No voice before the event reaches the engine.
    assert_eq!(rig.rt.last_message(), "0");
    rig.block(64);
    assert_eq!(rig.rt.last_message(), "1");
    // A later note sees the released one as gone.
    rig.off(0, 60).block(64).on(0, 62).block(64);
    assert_eq!(rig.rt.last_message(), "0");
}

#[test]
fn release_callback_and_note_held() {
    let script = "on init\ndeclare $held\nend on\non note\n$held := $NOTE_HELD\nend on\non release\nplay_note(80, 100, 0, 0)\nmessage($held & \":\" & $NOTE_HELD)\nend on";
    let mut rig = Rig::new(&[script]);
    rig.on(0, 60).off(50, 60);
    // The release callback runs before the release moves on to the engine.
    assert_eq!(
        rig.log(),
        [
            "play 60@0 v1 [0, 1, 2]",
            "play 80@50 v2 [0, 1, 2] sample",
            "off v1@50"
        ]
    );
    assert_eq!(rig.rt.last_message(), "1:0");
    // Ignoring the release keeps the voice sounding.
    let keep = "on init\nend on\non release\nignore_event($EVENT_ID)\nend on";
    let mut rig = Rig::new(&[keep]);
    rig.on(0, 60).off(50, 60);
    assert_eq!(rig.log(), ["play 60@0 v1 [0, 1, 2]"]);
}

#[test]
fn controllers_can_be_filtered_and_remapped() {
    let script = "on init\nend on\non controller\nif ($CC_NUM = 1)\nignore_controller\nset_controller(7, %CC[1])\nend if\nif ($CC_NUM = $VCC_PITCH_BEND)\nmessage($PITCH_BEND)\nend if\nend on";
    let mut rig = Rig::new(&[script]);
    rig.rt.controller(&mut rig.engine, 3, 1, 64);
    rig.rt.controller(&mut rig.engine, 4, 11, 20);
    rig.rt.pitch_bend(&mut rig.engine, 5, -100);
    assert_eq!(rig.log(), ["cc 7=64@3", "cc 11=20@4", "cc 128=-100@5"]);
    assert_eq!(rig.rt.last_message(), "-100");
}

#[test]
fn persistence_round_trips() {
    let script = "on init\ndeclare ui_knob $gain(0, 100, 1)\nmake_persistent($gain)\n$gain := 10\ndeclare ui_label $l(1,1)\nend on\non persistence_changed\nset_text($l, \"gain \" & $gain)\nend on\non ui_control($gain)\nset_text($l, \"moved \" & $gain)\nend on";
    let mut rig = Rig::new(&[script]);
    rig.rt.ui_control(&mut rig.engine, 0, 0, 42);
    assert_eq!(
        prop(&rig.rt.interface(0), 1, "$CONTROL_PAR_TEXT"),
        "moved 42"
    );
    let saved = rig.rt.persistence();
    assert_eq!(saved[0]["$gain"], Value::Int(42));
    let mut engine = LogEngine::new(Vec::new(), 48_000.0);
    let (rt, _) = Runtime::with_scripts(&[script], &mut engine, 8, saved);
    assert_eq!(prop(&rt.interface(0), 1, "$CONTROL_PAR_TEXT"), "gain 42");
}

#[test]
fn runaway_callbacks_are_bounded() {
    let script = "on init\ndeclare $n\nend on\non note\nwhile (1)\ninc($n)\nend while\nend on";
    let mut rig = Rig::new(&[script]);
    rig.on(0, 60);
    for _ in 0..8 {
        rig.block(256);
    }
    // The callback is preempted every block (4096 instructions a frame) and cut
    // off after 5M; only then does its note move on to the engine, at the
    // start of the fourth block.
    assert_eq!(rig.log(), ["play 60@768 v1 [0, 1, 2]"]);
    assert!(
        rig.rt
            .diagnostics()
            .iter()
            .any(|d| d.contains("instruction budget"))
    );
}

/// `cargo test --release --lib ksp::tests::note_on_cost -- --ignored --nocapture`
#[test]
#[ignore]
fn note_on_cost() {
    let script = "on init\ndeclare $last\ndeclare %vel[128]\nend on\non note\nignore_event($EVENT_ID)\nif ($last # 0)\nnote_off($last)\nend if\n$last := play_note($EVENT_NOTE, %vel[$EVENT_NOTE] + $EVENT_VELOCITY, 0, -1)\nchange_vol($last, -600, 0)\nend on\non release\nend on";
    let mut rig = Rig::new(&[script]);
    let n = 200_000u32;
    let start = std::time::Instant::now();
    for i in 0..n {
        let note = 36 + (i % 48) as u8;
        rig.rt.note_on(&mut rig.engine, 0, note, 100);
        rig.rt.note_off(&mut rig.engine, 1, note);
        if i % 64 == 0 {
            rig.engine.calls.clear();
        }
    }
    let ns = start.elapsed().as_nanos() as f64 / f64::from(n);
    println!("{ns:.0} ns per note-on + note-off pair (both callbacks, logging engine)");
}

#[test]
fn live_view_follows_ui_control() {
    let script = "on init\nmake_perfview\ndeclare ui_switch $legato\ndeclare ui_label $l(1,1)\nset_text($l, \"Sustain\")\nset_key_color(36, $KEY_COLOR_RED)\nend on\non ui_control($legato)\nset_text($l, \"Legato\")\nset_key_color(36, $KEY_COLOR_BLUE)\nset_key_name(37, \"Legato\")\nset_control_par(get_ui_id($l), $CONTROL_PAR_HIDE, $HIDE_WHOLE_CONTROL)\nend on";
    let mut rig = Rig::new(&[script]);
    let mut live = rig.rt.live();
    let ui = live.interface.as_ref().unwrap();
    assert_eq!(prop(ui, 1, "$CONTROL_PAR_TEXT"), "Sustain");
    assert_eq!(
        live.keys[&36].color,
        Some(Value::Text("$KEY_COLOR_RED".into()))
    );
    assert_eq!(live.keys[&37].name, "");
    let buffer = live.keys[&37].name.as_ptr();

    rig.rt.ui_control(&mut rig.engine, 0, 0, 1);
    rig.rt.refresh_live(&mut live);
    let ui = live.interface.as_ref().unwrap();
    assert_eq!(
        ui.controls[0].properties["$CONTROL_PAR_VALUE"],
        Value::Int(1)
    );
    assert_eq!(prop(ui, 1, "$CONTROL_PAR_TEXT"), "Legato");
    assert_eq!(
        ui.controls[1].properties["$CONTROL_PAR_HIDE"],
        Value::Int(16)
    );
    assert_eq!(
        live.keys[&36].color,
        Some(Value::Text("$KEY_COLOR_BLUE".into()))
    );
    assert_eq!(live.keys[&37].name, "Legato");
    assert_eq!(live.keys[&37].name.as_ptr(), buffer, "refreshed in place");
    assert_eq!(live, {
        let mut l = rig.rt.live();
        rig.rt.refresh_live(&mut l);
        l
    });
}

#[test]
fn variable_names_match_without_case() {
    // Una Corda persists `$swiNoiToEq` after declaring `$swiNoiToEQ`; Kontakt runs it.
    // Areia sizes arrays with `$max_num_groups` after declaring `$MAX_NUM_GROUPS`.
    // A user `%cc` never captures Kontakt's `%CC`.
    let source = "on init\ndeclare const $MAX_N := 2\ndeclare %cc[$max_n * 1]\ndeclare ui_switch $swiNoiToEQ\nmake_persistent($swiNoiToEq)\n$SWINOITOEQ := 1\ndeclare ui_label $l(1,1)\nset_text($l, $swinoitoeq & %CC[0])\nmake_perfview\nend on\non ui_control($swiNoiToEq)\nend on";
    let ui = initialize(source, 0, 0).unwrap();
    assert!(ui.performance);
    assert_eq!(prop(&ui, 0, "$CONTROL_PAR_VALUE"), "1");
    assert_eq!(prop(&ui, 1, "$CONTROL_PAR_TEXT"), "10");
}

#[test]
fn library_tab_ids_are_not_faults() {
    let source = "on init\nset_control_par_str($INST_LIB_PIC_ONE_ID,$CONTROL_PAR_PICTURE,\"logo\")\nset_control_par_str($INST_LIB_DESCRIPTION_ID,$CONTROL_PAR_TEXT,\"Compiled\")\nset_control_par($INST_LIB_COPYRIGHT_ID,$CONTROL_PAR_HIDE,0)\nmessage(get_control_par_str($INST_LIB_PIC_TWO_ID,$CONTROL_PAR_TEXT))\nend on";
    let ui = initialize(source, 0, 0).unwrap();
    assert!(ui.diagnostics.iter().all(|d| !d.contains("ID does not")), "{:?}", ui.diagnostics);
}

#[test]
fn faults_name_their_own_line() {
    let source = "on init\ndeclare !names[2]\ndeclare ui_label $l(1,1)\nset_control_par(get_ui_id($l),$CONTROL_PAR_WIDTH,1)\nset_control_par(get_ui_id($l),$CONTROL_PAR_HEIGHT,1)\n\nset_text($l, \"a\" & !names[6])\nend on";
    let ui = initialize(source, 0, 0).unwrap();
    assert!(ui.diagnostics.iter().any(|d| d.contains("line 7:")), "{:?}", ui.diagnostics);
}

#[test]
fn engine_par_display_follows_kontakt_laws() {
    // Afflatus labels its mic faders `get_engine_par_disp(...) & " dB"`; it read "630000 dB".
    let source = "on init\ndeclare ui_label $l(1,1)\nset_engine_par($ENGINE_PAR_VOLUME, 630000, -1, -1, -1)\nset_engine_par($ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN, 396851, -1, 1, 1)\nset_engine_par($ENGINE_PAR_SEND_EFFECT_DRY_LEVEL, 0, -1, 1, 0)\nset_engine_par($ENGINE_PAR_PAN, 250000, -1, -1, -1)\nset_text($l, get_engine_par_disp($ENGINE_PAR_VOLUME, -1, -1, -1) & \" dB|\" & get_engine_par_disp($ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN, -1, 1, 1) & \"|\" & get_engine_par_disp($ENGINE_PAR_SEND_EFFECT_DRY_LEVEL, -1, 1, 0) & \"|\" & get_engine_par_disp($ENGINE_PAR_PAN, -1, -1, -1) & \"|\" & get_engine_par($ENGINE_PAR_VOLUME, -1, -1, -1))\nend on";
    let ui = initialize(source, 0, 0).unwrap();
    assert_eq!(prop(&ui, 0, "$CONTROL_PAR_TEXT"), "0.0 dB|0.0|-inf|L 50|630000");
}

#[test]
fn reversed_case_ranges_and_effect_loads_compile() {
    // Both from Una Corda: a descending `case` range and an effect load's async ID.
    let source = "on init\ndeclare $v := -7\ndeclare $id\ndeclare ui_label $l(1,1)\nselect($v)\ncase -1 to -50\nset_text($l, \"in\")\nend select\n$id := set_engine_par($ENGINE_PAR_EFFECT_SUBTYPE, 1, -1, 1, 1)\nset_text($l, get_control_par_str(get_ui_id($l), $CONTROL_PAR_TEXT) & ($id # -1) & set_engine_par($ENGINE_PAR_VOLUME, 1, -1, -1, -1))\nend on";
    let ui = initialize(source, 0, 0).unwrap();
    assert_eq!(prop(&ui, 0, "$CONTROL_PAR_TEXT"), "in1-1");
    assert!(ui.diagnostics.iter().all(|d| !d.contains("disabled")), "{:?}", ui.diagnostics);
}

/// Snapshots refreshed a small budget at a time (as the audio thread does,
/// so big persistent tables never stall a block) end up equal to a whole
/// refresh, and say whether anything changed.
#[test]
fn budgeted_persistence_refresh_matches_whole_and_reports_changes() {
    let script = "on init\ndeclare %table[1000]\nmake_persistent(%table)\ndeclare $x\nmake_persistent($x)\ndeclare ui_knob $k(0, 1000, 1)\nmake_persistent($k)\nend on\non ui_control($k)\n%table[$k] := $k + 7\n$x := $k\nend on";
    let mut rig = Rig::new(&[script]);
    let mut saved = rig.rt.persistence();
    let mut at = runtime::Refresh::default();
    let mut blocks = 0;
    while !rig.rt.refresh_persistence_within(&mut saved, &mut at, 64) {
        blocks += 1;
    }
    assert!(blocks >= 15, "1000 values at 64 a block take many blocks: {blocks}");
    assert!(!at.changed, "nothing ran since the snapshot was taken");
    rig.rt.ui_control(&mut rig.engine, 0, 0, 999);
    let mut at = runtime::Refresh::default();
    while !rig.rt.refresh_persistence_within(&mut saved, &mut at, 64) {}
    assert!(at.changed);
    assert!(saved == rig.rt.persistence(), "piecewise equals whole");
    let Value::Array(table) = &saved[0]["%table"] else {
        panic!("a persistent array saves as one")
    };
    assert_eq!((&table[999], &saved[0]["$x"]), (&Value::Int(1006), &Value::Int(999)));
    assert!(!rig.rt.refresh_persistence(&mut saved), "a second refresh finds nothing new");
}

// ---- Builtins added for coverage ---------------------------------------------------

#[test]
fn voice_limits_are_stored_and_completed() {
    let script = "on init\ndeclare ui_label $l(1,1)\ndeclare $id\nset_text($l, get_voice_limit($NI_VL_TMPRO_STANDARD) & \",\" & get_voice_limit($NI_VL_TMPRO_HQ))\nend on\non note\n$id := set_voice_limit($NI_VL_TMPRO_HQ, 6)\nend on\non async_complete\nif ($NI_ASYNC_ID = $id)\nset_text($l, \"hq \" & get_voice_limit($NI_VL_TMPRO_HQ) & \" ok \" & $NI_ASYNC_EXIT_STATUS)\nend if\nend on";
    let mut rig = Rig::new(&[script]);
    assert_eq!(prop(&rig.rt.interface(0), 0, "$CONTROL_PAR_TEXT"), "8,2");
    rig.on(0, 60).block(64);
    assert_eq!(prop(&rig.rt.interface(0), 0, "$CONTROL_PAR_TEXT"), "hq 6 ok 1");
}

#[test]
fn sign_and_root_math() {
    assert_eq!(
        label("on init\ndeclare ui_label $l(1,1)\nset_text($l, sgn(-7) & sgn(0) & sgn(3) & signbit(-1) & signbit(5) & sgn(-0.5) & signbit(2.0) & \":\" & exp2(3.0) & \":\" & cbrt(27.0))\nend on"),
        "-10110-10:8:3"
    );
}

#[test]
fn legacy_and_kontakt7_spellings() {
    // `_pgs_*` resolve to the commands they name; Kontakt 7's `get_*_idx`
    // return `$NI_NOT_FOUND` where `find_*` return 0.
    assert_eq!(
        label("on init\ndeclare ui_label $l(1,1)\n_pgs_create_key(K, 1)\n_pgs_set_key_val(K, 0, 5)\nset_text($l, _pgs_get_key_val(K, 0) & get_group_idx(\"b\") & get_mod_idx(0, \"x\") & find_group(\"b\") & $NI_NOT_FOUND)\nend on"),
        "5-1-10-1"
    );
}

#[test]
fn keyranges_never_overlap() {
    let source = "on init\ndeclare ui_label $l(1,1)\nset_keyrange(36, 47, \"Low\")\nset_keyrange(48, 60, \"Mid\")\nset_keyrange(45, 50, \"Over\")\nset_text($l, get_keyrange_name(36) & \"|\" & get_keyrange_name(40) & \"|\" & get_keyrange_min_note(47) & \"-\" & get_keyrange_max_note(47) & \"|\" & get_keyrange_name(55))\nremove_keyrange(46)\nadd_text_line($l, get_keyrange_name(46) & \".\")\nend on";
    // "Over" replaced both ranges it touched; removing it clears 46.
    assert_eq!(label(source), "||45-50|\n.");
}

#[test]
fn display_of_a_hypothetical_engine_value() {
    let shown = label("on init\ndeclare ui_label $l(1,1)\nset_text($l, get_engine_par_disp_ext($ENGINE_PAR_VOLUME, 500000, 0, -1, -1) & \"|\" & get_engine_par_disp($ENGINE_PAR_VOLUME, 0, -1, -1))\nend on");
    let (ext, current) = shown.split_once('|').unwrap();
    assert!(!ext.is_empty() && ext != current, "{shown}");
}

#[test]
fn ui_and_debugger_commands_are_accepted() {
    let ui = initialize("on init\ndeclare ui_waveform $w(6,6)\ndeclare ui_xy ?xy[2]\ndeclare ui_label $l(1,1)\nattach_level_meter(get_ui_id($l), 0, 0, 0, -1)\nwatch_var($l)\nwatch_array_idx(?xy, 0)\nset_control_par_real_arr(get_ui_id(?xy), $CONTROL_PAR_VALUE, 0.5, 0)\nset_text($l, get_ui_wf_property($w, $UI_WF_PROP_PLAY_CURSOR, 0) & get_control_par_real_arr(get_ui_id(?xy), $CONTROL_PAR_VALUE, 0))\nend on", 0, 0).unwrap();
    assert_eq!(prop(&ui, 2, "$CONTROL_PAR_TEXT"), "00");
    assert!(!ui.diagnostics.iter().any(|d| d.starts_with("Unsupported")), "{:?}", ui.diagnostics);
}

#[test]
fn unsupported_builtins_degrade_per_call() {
    // `get_zone_par` is not implemented: init runs on, reads give 0, and an
    // unknown built-in array reads 0 instead of failing the callback.
    let ui = initialize("on init\ndeclare ui_label $l(1,1)\nset_text($l, \"z\" & get_zone_par(0, $ZONE_PAR_VOLUME) & %NI_FUTURE_ARRAY[3])\nend on\non note\nset_zone_par(0, 0, 0)\nend on", 0, 0).unwrap();
    assert_eq!(prop(&ui, 0, "$CONTROL_PAR_TEXT"), "z00");
    for name in ["get_zone_par", "set_zone_par"] {
        assert!(ui.diagnostics.iter().any(|d| d.starts_with(&format!("Unsupported KSP function: {name}"))), "{:?}", ui.diagnostics);
    }
    assert!(ui.diagnostics.iter().any(|d| d.starts_with("Unsupported KSP variable: %NI_FUTURE_ARRAY")));
}

#[test]
fn ui_controls_runs_before_the_control_callback() {
    let script = "on init\nmake_perfview\ndeclare ui_knob $k(0, 10, 1)\ndeclare ui_label $l(1,1)\nend on\non ui_controls\nset_text($l, \"all \" & ($NI_UI_ID = get_ui_id($k)) & \" \" & $k)\nend on\non ui_control($k)\nadd_text_line($l, \"own\")\nend on";
    let mut rig = Rig::new(&[script]);
    // Out-of-range host values are clamped to the knob's range.
    rig.rt.ui_control(&mut rig.engine, 0, 0, 99);
    assert_eq!(prop(&rig.rt.interface(0), 1, "$CONTROL_PAR_TEXT"), "all 1 10\nown");
}

#[test]
fn move_control_to_zero_hides_and_back_shows() {
    let ui = initialize("on init\ndeclare ui_knob $a(0,1,1)\ndeclare ui_knob $b(0,1,1)\nmove_control($a, 0, 0)\nmove_control($b, 0, 0)\nmove_control($b, 2, 1)\nend on", 0, 0).unwrap();
    assert_eq!(prop(&ui, 0, "$CONTROL_PAR_HIDE"), "16");
    assert_eq!(prop(&ui, 1, "$CONTROL_PAR_HIDE"), "0");
}

// ---- Runaway scripts ---------------------------------------------------------------

#[test]
fn runaway_scripts_stay_alive_and_bounded() {
    // An endless `on init` loop is stopped; the script and its controls stay,
    // and its other callbacks still run.
    let script = "on init\ndeclare ui_label $l(1,1)\nset_text($l, \"before\")\nwhile (1)\nend while\nend on\non note\nset_text($l, \"note\")\nend on\non ui_control($l)\nwhile (1)\nend while\nend on";
    let mut engine = LogEngine::new(vec!["a".into()], 48_000.0);
    let (mut rt, errors) = Runtime::with_scripts(&[script], &mut engine, 8, Vec::new());
    assert!(errors.iter().all(Option::is_none), "{errors:?}");
    assert!(rt.diagnostics().iter().any(|d| d.contains("on init exceeded")));
    assert_eq!(prop(&rt.interface(0), 0, "$CONTROL_PAR_TEXT"), "before");
    rt.note_on(&mut engine, 0, 60, 100);
    assert_eq!(prop(&rt.interface(0), 0, "$CONTROL_PAR_TEXT"), "note");
    // An endless UI callback costs a bounded share of each block, then stops.
    rt.ui_control(&mut engine, 0, 0, 1);
    let start = std::time::Instant::now();
    for _ in 0..64 {
        rt.process(&mut engine, 128);
    }
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    assert!(rt.diagnostics().iter().any(|d| d.contains("instruction budget")));
    rt.note_on(&mut engine, 0, 61, 100);
    assert_eq!(prop(&rt.interface(0), 0, "$CONTROL_PAR_TEXT"), "note");
}

#[test]
fn pgs_and_async_ping_pong_cannot_hang() {
    // Each answers a change with another change (and queued work, or a new
    // asynchronous call): delivery is spread over blocks instead of looping.
    let pgs = "on init\npgs_create_key(K, 1)\nend on\non pgs_changed\npgs_set_key_val(K, 0, pgs_get_key_val(K, 0) + 1)\nset_controller(1, 1)\nend on\non note\npgs_set_key_val(K, 0, 1)\nend on";
    let retry = "on init\ndeclare $id\nend on\non note\n$id := load_ir_sample(\"x.wav\", 0, 0)\nend on\non async_complete\n$id := load_ir_sample(\"x.wav\", 0, 0)\nend on";
    let mut engine = LogEngine::new(vec!["a".into()], 48_000.0);
    let (mut rt, _) = Runtime::with_scripts(&[pgs, retry], &mut engine, 8, Vec::new());
    rt.note_on(&mut engine, 0, 60, 100);
    for _ in 0..4 {
        rt.process(&mut engine, 128);
    }
}

/// Conditions compiled as branches and the fused index/compare ops give the
/// value form's results: short-circuit `and`/`or`, flattened 2D indexes,
/// and an out-of-bounds read that yields 0 and is reported.
#[test]
fn branch_conditions_and_fused_indexes_match_the_value_form() {
    let source = "on init
declare %a[12] := (5, 0, 7, 1, 0, 3, 9, 2, 0, 4, 6, 8)
declare $i
declare $j
declare $n
declare $hits
declare $gt
declare ui_label $label(1,1)
while ($i < 3 and $n < 100)
  $j := 0
  while ($j < 4)
    if ((%a[4 * $i + $j] > 2 and %a[4 * $i + $j] # 9) or $j = 3)
      inc($hits)
    end if
    if (not (%a[$j] = 0) and ($i = 1 or $j = 0))
      $n := $n + %a[4 * $i + $j]
    end if
    if ($j > $i)
      inc($gt)
    end if
    inc($j)
  end while
  inc($i)
end while
if (%a[$hits + 100] = 0 or 0)
  $n := $n + 1000
end if
set_text($label, $hits & \":\" & $n & \":\" & $gt)
end on";
    let ui = initialize(source, 0, 0).unwrap();
    assert_eq!(prop(&ui, 0, "$CONTROL_PAR_TEXT"), "8:1016:6");
    assert!(ui.diagnostics.iter().any(|d| d.contains("out of bounds")), "{:?}", ui.diagnostics);
}

#[test]
fn built_in_state_variables() {
    let script = "on init\ndeclare ui_label $l(1,1)\nset_text($l, $NI_KONTAKT_IS_HEADLESS & $NI_KONTAKT_IS_STANDALONE & $SIGNATURE_NUM & $SIGNATURE_DENOM & \" \" & ($NI_DATE_YEAR > 2020) & ($NI_DATE_MONTH >= 1) & ($NI_TIME_HOUR < 24))\nend on\non note\nset_text($l, %KEY_DOWN_OCT[0] & %KEY_DOWN_OCT[1] & ($PLAYED_VOICES_INST > 0))\nend on\non release\nset_text($l, %KEY_DOWN_OCT[0])\nend on";
    let mut rig = Rig::new(&[script]);
    let ui = rig.rt.interface(0);
    assert_eq!(prop(&ui, 0, "$CONTROL_PAR_TEXT"), "0044 111");
    assert!(rig.rt.diagnostics().is_empty(), "{:?}", rig.rt.diagnostics());
    rig.on(0, 60).on(0, 72).block(64);
    assert_eq!(prop(&rig.rt.interface(0), 0, "$CONTROL_PAR_TEXT"), "201");
    rig.off(0, 72).block(64);
    assert_eq!(prop(&rig.rt.interface(0), 0, "$CONTROL_PAR_TEXT"), "1");
}

#[test]
fn sine_era_framework_in_slot_two() {
    // The shape of Heavyocity and Orchestral Tools frameworks: the second slot
    // builds meters and loads impulses from nested functions.
    let first = "on init\ndeclare $x\nend on";
    let second = "function style\nset_control_par(get_ui_id($meter), $CONTROL_PAR_BG_COLOR, 0)\nset_control_par(get_ui_id($meter), $CONTROL_PAR_OFF_COLOR, 0)\nset_control_par(get_ui_id($meter), $CONTROL_PAR_ON_COLOR, 0FF00h)\nset_control_par(get_ui_id($meter), $CONTROL_PAR_OVERLOAD_COLOR, 0FF0000h)\nset_control_par(get_ui_id($meter), $CONTROL_PAR_PEAK_COLOR, 0FFFFFFh)\nset_control_par(get_ui_id($meter), $CONTROL_PAR_VERTICAL, 1)\nattach_level_meter(get_ui_id($meter), -1, -1, 0, $NI_BUS_OFFSET + 1)\nend function\nfunction build\ncall style\n$voices := get_voice_limit($NI_VL_TMPRO_STANDARD)\nset_engine_par($ENGINE_PAR_SEND_EFFECT_OUTPUT_GAIN, 500000, -1, 0, $NI_BUS_OFFSET + 1)\nend function\non init\ndeclare ui_label $l(1,1)\ndeclare ui_level_meter $meter\ndeclare ui_waveform $wave(6,6)\ndeclare $id\ndeclare $voices\ncall build\nset_text($l, get_control_par(get_ui_id($meter), $CONTROL_PAR_ON_COLOR) & \" \" & get_control_par(get_ui_id($meter), $CONTROL_PAR_VERTICAL) & \" \" & $voices)\nend on\non note\n$id := load_ir_sample(\"Hall.wav\", 0, $NI_BUS_OFFSET + 1)\nwait_async($id)\nadd_text_line($l, \"ir \" & $NI_ASYNC_EXIT_STATUS & get_engine_par_disp($ENGINE_PAR_SEND_EFFECT_OUTPUT_GAIN, -1, 0, $NI_BUS_OFFSET + 1))\nend on";
    let mut rig = Rig::new(&[first, second]);
    assert_eq!(prop(&rig.rt.interface(1), 0, "$CONTROL_PAR_TEXT"), "65280 1 8");
    assert!(
        !rig.rt.diagnostics().iter().any(|d| d.contains("Unsupported")),
        "{:?}",
        rig.rt.diagnostics()
    );
    rig.on(0, 60).block(64);
    assert!(prop(&rig.rt.interface(1), 0, "$CONTROL_PAR_TEXT").contains("\nir "));
}

#[test]
fn a_script_without_init_says_what_it_holds() {
    for (source, want) in [
        ("  \n", "the script text is empty"),
        ("on note\nend on\nfunction f\nend function", "holds only 2 block(s): on note, function f"),
    ] {
        let e = format!("{:#}", initialize(source, 0, 0).unwrap_err());
        assert!(e.contains(want), "{e}");
    }
}

#[test]
fn group_lookups_match_exactly_and_report_only_near_misses() {
    let run = |names: &str| {
        let script = format!("on init\ndeclare ui_label $l(1,1)\nset_text($l, {names})\nend on");
        let mut engine = LogEngine::new(vec!["a".into(), "Strings Long".into()], 48_000.0);
        let (rt, _) = Runtime::with_scripts(&[script.as_str()], &mut engine, 8, Vec::new());
        (prop(&rt.interface(0), 0, "$CONTROL_PAR_TEXT"), rt.diagnostics())
    };
    // A name the instrument lacks is the script's business, as in Kontakt.
    let (text, diagnostics) = run("find_group(\"Strings Long\") & find_group(\"Brass\") & get_group_idx(\"Brass\")");
    assert_eq!((text.as_str(), diagnostics), ("10-1", Vec::<String>::new()));
    // One that differs only in case or spaces may be decoded wrongly.
    let (text, diagnostics) = run("find_group(\"strings long \")");
    assert_eq!(text, "0");
    assert!(diagnostics.iter().any(|d| d.contains("find_group")), "{diagnostics:?}");
}

/// The two out-of-bounds shapes the local libraries hit, both script bugs
/// Kontakt shares: a label reading one name past a per-instrument name table
/// (Afflatus), and a loop running nine voices over four-voice slots
/// (Dolce, with its saved developer switches on). Each read is empty or 0,
/// is reported, and the callback carries on.
#[test]
fn library_out_of_bounds_shapes_read_empty_and_continue() {
    let source = "on init
declare const $ARTS := 2
declare const $VOICES := 4
declare !names[$ARTS]
!names[0] := \"Rip Slow\"
declare %ids[$ARTS * $VOICES] := (-1)
declare $art
declare $n
declare $found
while ($art < $ARTS)
  $n := 0
  while ($n <= 8)
    if (%ids[$VOICES * $art + $n] # -1)
      inc($found)
    end if
    inc($n)
  end while
  inc($art)
end while
declare ui_label $l(1,1)
set_text($l, \"[\" & !names[6] & \"]\" & $found)
end on";
    let ui = initialize(source, 0, 0).unwrap();
    // The six reads past index 7 yield 0, which is not -1.
    assert_eq!(prop(&ui, 0, "$CONTROL_PAR_TEXT"), "[]6");
    assert!(ui.diagnostics.iter().any(|d| d.contains("out of bounds")), "{:?}", ui.diagnostics);
}

#[test]
fn modulator_lookups_match_exactly_and_report_only_near_misses() {
    let run = |calls: &str| {
        let script = format!("on init\ndeclare ui_label $l(1,1)\nset_text($l, {calls})\nend on");
        let mut engine = LogEngine::new(vec!["a".into()], 48_000.0);
        engine.modulators = vec![vec![
            ("ENV_FLEX".into(), vec!["ENV_FLEX_VOLUME".into()]),
            ("CC_VOLUME".into(), vec!["CC_VOLUME".into(), "Dyn. Range".into()]),
        ]];
        let (rt, _) = Runtime::with_scripts(&[script.as_str()], &mut engine, 8, Vec::new());
        (prop(&rt.interface(0), 0, "$CONTROL_PAR_TEXT"), rt.diagnostics())
    };
    // A framework script asking for an AHDSR the group lacks: Kontakt's business.
    let (text, diagnostics) = run(
        "find_mod(0, \"CC_VOLUME\") & find_target(0, 1, \"Dyn. Range\") & find_mod(0, \"ENV_AHDSR\") & get_mod_idx(0, \"ENV_AHDSR\")",
    );
    assert_eq!((text.as_str(), diagnostics), ("110-1", Vec::<String>::new()));
    let (text, diagnostics) = run("find_target(0, 1, \"dyn. range \")");
    assert_eq!(text, "0");
    assert!(diagnostics.iter().any(|d| d.contains("find_mod/find_target")), "{diagnostics:?}");
}
