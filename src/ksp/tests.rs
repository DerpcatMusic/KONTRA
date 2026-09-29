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
    let failed = "on init\npgs_set_key_val(MIC_LEVEL,1,0)\ndeclare $a := 1/0\nend on";
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
    for expression in [
        "1.0/0.0",
        "sqrt(-1.0)",
        "exp(1000.0)",
        "int(2147483648.0)",
        "1.0+1",
    ] {
        assert!(
            initialize(
                &format!("on init\ndeclare ~x := {expression}\nend on"),
                0,
                0
            )
            .is_err(),
            "{expression}"
        );
    }
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
    assert!(
        initialize("on init\niffy(1)\nend on", 0, 0)
            .unwrap_err()
            .to_string()
            .contains("iffy")
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
    assert_eq!(prop(&ui, 1, "$CONTROL_PAR_HIDE"), "1");
    assert!(
        initialize("on init\nwhile (1)\nend while\nend on", 0, 0)
            .unwrap_err()
            .to_string()
            .contains("budget")
    );
    let oob = initialize("on init\ndeclare %a[1]\n%a[2] := 1\nend on", 0, 0).unwrap();
    assert!(oob.diagnostics.iter().any(|d| d.contains("out of bounds")));
    assert!(initialize("on init\nunknown_function()\nend on", 0, 0).is_err());
    assert!(initialize("on init\ndeclare $a := 1/0\nend on", 0, 0).is_err());
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
    // The callback is preempted every block (2M instructions) and cut off after 5M;
    // only then does its note move on to the engine, at the start of the third block.
    assert_eq!(rig.log(), ["play 60@512 v1 [0, 1, 2]"]);
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
