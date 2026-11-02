#![cfg(test)]
//! KSP source compiled and run on the VM with a recording host.

use kontra_ksp::{CallbackKind, Cells, FIRST_UI_ID, Persistence, Script, compile};
use kontra_vm::{Action, HostApi, HostCall, HostValue, Instance, Step};

#[derive(Default)]
struct Host {
    messages: Vec<String>,
    calls: Vec<(Action, Vec<i64>)>,
    note: i32,
    velocity: i32,
}

impl HostApi for Host {
    fn call(&mut self, call: HostCall<'_>) -> HostValue {
        let b = call.action;
        match b {
            Action::Message => {
                self.messages.push(call.texts[0].as_str().to_owned());
                HostValue::Nothing
            }
            Action::EventNote => HostValue::Int(self.note),
            Action::EventVelocity => HostValue::Int(self.velocity),
            // A write to `$EVENT_VELOCITY` compiles to `change_velo($EVENT_ID, v)`,
            // and the manual's rule is that the written value *becomes* the new
            // `$EVENT_VELOCITY`: "Once the velocity of a particular note event is
            // changed, it becomes the new $EVENT_VELOCITY". A host that recorded
            // the call without moving the event would let a readback assertion
            // pass whatever the write did.
            Action::ChangeVelo => {
                self.calls.push((b, call.ints.to_vec()));
                if let Some(&v) = call.ints.get(1) {
                    self.velocity = i32::try_from(v).unwrap_or(self.velocity);
                }
                HostValue::Nothing
            }
            Action::EventId => HostValue::Int(1000),
            Action::PlayNote => {
                self.calls.push((b, call.ints.to_vec()));
                HostValue::Int(2000)
            }
            _ => {
                self.calls.push((b, call.ints.to_vec()));
                HostValue::Nothing
            }
        }
    }
}

struct Run {
    script: Script,
    vm: Instance,
    host: Host,
}

impl Run {
    fn new(source: &str) -> Self {
        let script = compile(source, &[]).unwrap_or_else(|e| {
            let (line, column) = e.line_column(source);
            panic!("{line}:{column}: {}", e.message)
        });
        let vm = Instance::new(&script.program, 4);
        let mut run = Self {
            script,
            vm,
            host: Host::default(),
        };
        if run.script.callback(CallbackKind::Init, None).is_some() {
            assert_eq!(run.callback(CallbackKind::Init, None), Step::Done);
        }
        run
    }

    fn callback(&mut self, kind: CallbackKind, control: Option<u32>) -> Step {
        let entry = self.script.callback(kind, control).unwrap();
        let co = self.vm.start(entry, 0).unwrap();
        self.vm.run(&self.script.program, co, &mut self.host)
    }

    fn cells(&self, name: &str) -> (u32, u32) {
        let v = self
            .script
            .variables
            .iter()
            .find(|v| &*v.name == name)
            .unwrap();
        match v.cells {
            Cells::Int { base, len } | Cells::Real { base, len } | Cells::Text { base, len } => {
                (base, len)
            }
            other => panic!("{name}: {other:?}"),
        }
    }

    fn int(&self, name: &str) -> i32 {
        self.vm.cell(self.cells(name).0 as usize).unwrap() as i32
    }

    fn ints(&self, name: &str) -> Vec<i32> {
        let (base, len) = self.cells(name);
        (base..base + len)
            .map(|n| self.vm.cell(n as usize).unwrap() as i32)
            .collect()
    }

    /// The catalog name a script's opaque symbol value stands for.
    fn symbol(&self, value: i64) -> Option<&str> {
        self.script.symbol(i32::try_from(value).ok()?)
    }

    fn text(&self, name: &str) -> String {
        self.vm
            .text(self.cells(name).0 as usize)
            .unwrap()
            .as_str()
            .to_owned()
    }
}

#[test]
fn arithmetic_loops_select_and_text() {
    let r = Run::new(
        "on init
            declare $i
            declare $sum
            declare $big := 2147483647
            declare $pick
            declare @s
            declare ~r := 1.5
            while ($i < 10)
                inc($i)
                if ($i mod 2 = 0)
                    continue
                end if
                $sum := $sum + $i
            end while
            $big := $big + 1
            select ($sum)
                case 0 to 10
                    $pick := 1
                case 25
                    $pick := 2
                case 26 to 100
                    $pick := 3
            end select
            @s := \"sum \" & $sum & \" r \" & ~r * 2.0
            $i := sh_left(1, 4) .or. 1
            message(@s)
        end on",
    );
    assert_eq!(r.int("$sum"), 25);
    assert_eq!(r.int("$big"), i32::MIN);
    assert_eq!(r.int("$pick"), 2);
    assert_eq!(r.int("$i"), 17);
    assert_eq!(r.text("@s"), "sum 25 r 3.000000");
    assert_eq!(r.host.messages, ["sum 25 r 3.000000"]);
}

#[test]
fn a_runaway_loop_stops_even_around_an_inner_loop() {
    // Each outer turn exits an inner loop and moves $k, so only the turn
    // count across both loops can end it, wherever the limit lands.
    for inner in [1, 2] {
        let r = Run::new(&format!(
            "on init
                declare $j
                declare $k
                while (1 = 1)
                    $j := 0
                    while ($j < {inner})
                        inc($j)
                    end while
                    inc($k)
                end while
            end on"
        ));
        assert!(r.int("$k") > 0 && r.int("$k") < 10_000_001);
    }
}

#[test]
fn functions_called_before_definition_and_by_name() {
    let r = Run::new(
        "function double
            $x := $x * 2
            call bump
        end function
        on init
            declare $x := 3
            double
            call double
        end on
        function bump
            inc($x)
        end function",
    );
    assert_eq!(r.int("$x"), 15);
    assert_eq!(r.script.program.max_depth, 3);
    let err = compile(
        "function a\n call b\nend function\nfunction b\n call a\nend function",
        &[],
    )
    .unwrap_err();
    assert_eq!(err.message, "recursive function call");
}

#[test]
fn arrays_fill_search_sort_and_size() {
    let r = Run::new(
        "on init
            declare const $N := 4 * 5
            declare %a[$N] := (5, 3, 9)
            declare %b[4] := (1, 0)
            declare %c[3]
            declare $where
            declare $count
            $count := num_elements(%a)
            $where := search(%a, 9)
            %c[0] := 7
            %c[2] := 2
            sort(%c, 0)
        end on",
    );
    let a = r.ints("%a");
    assert_eq!(&a[..4], [5, 3, 9, 9]);
    assert!(a[3..].iter().all(|&v| v == 9));
    assert_eq!(r.ints("%b"), [1, 0, 0, 0]);
    assert_eq!(r.int("$count"), 20);
    assert_eq!(r.int("$where"), 2);
    assert_eq!(r.ints("%c"), [0, 2, 7]);
}

/// `num_elements` resolves a name the way a read of it does: an exactly
/// spelled system array is the system array (KSP reference, "Variables and
/// Constants": names are case-sensitive), and a declaration that happens to
/// match case-insensitively is a different name. This is the same rule `load`
/// follows, and it is what kept Harpsichord's `$event_id` from hiding
/// `$EVENT_ID`.
#[test]
fn num_elements_prefers_the_exactly_spelled_system_array() {
    let r = Run::new(
        "on init
            declare %key_down[3]
            declare $system
            declare $own
            $system := num_elements(%KEY_DOWN)
            $own := num_elements(%key_down)
        end on",
    );
    assert_eq!(r.int("$system"), 128, "the manual's keyboard array");
    assert_eq!(r.int("$own"), 3, "the script's own array");
}

#[test]
fn controls_ids_callbacks_and_persistence() {
    let mut r = Run::new(
        "on init
            declare ui_knob $volume(0, 1000, 1)
            declare ui_button $mute
            declare ui_table %steps[8](2, 2, 100)
            declare $last
            make_persistent($volume)
            set_control_par(get_ui_id($mute), $CONTROL_PAR_HIDE, 1)
        end on
        on ui_control($mute)
            $last := get_ui_id($mute) - get_ui_id($volume)
        end on",
    );
    assert_eq!(r.script.controls.len(), 3);
    assert_eq!(r.script.controls[0].params, [0, 1000, 1]);
    let volume = r
        .script
        .variables
        .iter()
        .find(|v| &*v.name == "$volume")
        .unwrap();
    assert_eq!(volume.persistence, Persistence::Snapshot);
    let (b, ints) = &r.host.calls[0];
    assert_eq!(*b, Action::SetControlPar);
    assert_eq!(ints[0], i64::from(FIRST_UI_ID + 1));
    assert_eq!(r.script.symbol(ints[1] as i32), Some("$CONTROL_PAR_HIDE"));
    assert_eq!(r.callback(CallbackKind::UiControl, Some(1)), Step::Done);
    assert_eq!(r.int("$last"), 1);
}

#[test]
fn note_callback_plays_waits_and_reads_event() {
    let mut r = Run::new(
        "on init
            declare polyphonic $id
        end on
        on note
            ignore_event($EVENT_ID)
            $id := play_note($EVENT_NOTE + 12, 100, 0, -1)
            wait(5000)
            change_tune($id, 100, 0)
            note_off($id)
        end on",
    );
    r.host.note = 60;
    let entry = r.script.callback(CallbackKind::Note, None).unwrap();
    let co = r.vm.start(entry, 0).unwrap();
    assert_eq!(
        r.vm.run(&r.script.program, co, &mut r.host),
        Step::Wait(5000)
    );
    let names: Vec<_> = r.host.calls.iter().map(|c| c.0).collect();
    assert_eq!(names, [Action::IgnoreEvent, Action::PlayNote]);
    assert_eq!(r.host.calls[0].1, [1000]);
    assert_eq!(r.host.calls[1].1, [72, 100, 0, -1]);
    assert_eq!(r.vm.run(&r.script.program, co, &mut r.host), Step::Done);
    assert_eq!(r.host.calls[2], (Action::ChangeTune, vec![2000, 100, 0]));
    // Left-out optional arguments pass zero.
    assert_eq!(r.host.calls[3], (Action::NoteOff, vec![2000, 0]));
}

/// KSP names are case-sensitive (KSP reference, "Variables and Constants"), so
/// a declaration that differs only in case does not hide a system variable:
/// Harpsichord declares `$event_id` and then calls `ignore_event($EVENT_ID)`,
/// and native Kontakt ignores the host note there.
#[test]
fn a_declaration_in_another_case_is_not_a_system_variable() {
    let mut r = Run::new(
        "on init
            declare $event_id
        end on
        on note
            ignore_event($EVENT_ID)
            set_event_par($EVENT_ID, $EVENT_PAR_0, 1)
        end on",
    );
    assert_eq!(r.callback(CallbackKind::Note, None), Step::Done);
    assert_eq!(
        r.host.calls,
        [
            (Action::IgnoreEvent, vec![1000]),
            (Action::SetEventPar, vec![1000, 0, 1]),
        ]
    );
}

#[test]
fn constants_symbols_and_conditions() {
    let source = "on init
        declare $a := $EVENT_PAR_TUNE
        declare $b := $ENGINE_PAR_VOLUME
        declare $c
        USE_CODE_IF(LEGACY)
        $c := 1
        END_USE_CODE
        USE_CODE_IF_NOT(LEGACY)
        $c := 2
        END_USE_CODE
    end on";
    let r = Run::new(source);
    assert_eq!(r.int("$a"), 6);
    assert_eq!(r.script.symbol(r.int("$b")), Some("$ENGINE_PAR_VOLUME"));
    assert_eq!(r.int("$c"), 2);
    let script = compile(source, &["LEGACY"]).unwrap();
    assert_eq!(script.conditions, ["LEGACY"]);
}

#[test]
fn type_and_name_errors() {
    let error = |source: &str| compile(source, &[]).unwrap_err().message;
    assert_eq!(
        error("on init declare $x if ($x) end if end on"),
        "expected a condition, found an integer"
    );
    assert_eq!(
        error("on init declare ~r := 1 end on"),
        "expected a real, found an integer"
    );
    assert_eq!(error("on init $nope := 1 end on"), "$nope is not declared");
    assert_eq!(
        error("on init $EVENT_ID := 1 end on"),
        "$EVENT_ID is read-only"
    );
    assert_eq!(
        error("on init declare $x declare $X end on"),
        "$X is declared twice"
    );
    assert_eq!(error("on wibble end on"), "unknown callback wibble");
    let source = "on init\n  declare $x := 1 +\nend on";
    let e = compile(source, &[]).unwrap_err();
    assert_eq!(e.line_column(source), (3, 1));
}

/// Fredonia Grand Organ writes a quote inside a string as `\"` in its help
/// texts. The backslash ended the string early and the whole 461-control
/// console script failed to compile, so the page it builds was never drawn.
/// This is the shape of the source, reduced.
#[test]
fn a_string_with_an_escaped_quote_compiles_and_reads_back_unescaped() {
    let r = Run::new(
        "on init
            declare @s := \"different divisions live on different keybeds or \\\"manuals\\\" on a real organ\"
            message(@s)
        end on",
    );
    assert_eq!(
        r.host.messages,
        ["different divisions live on different keybeds or \"manuals\" on a real organ"]
    );
}

/// Areia's 275k-line script, from `KONTRA_AREIA_KSP` (a library dump; run
/// with `--ignored`).
fn areia() -> String {
    let path = std::env::var("KONTRA_AREIA_KSP").expect("KONTRA_AREIA_KSP names the script");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The Areia script compiles.
#[test]
#[ignore = "needs the Areia library dump in KONTRA_AREIA_KSP"]
fn areia_compiles() {
    let source = areia();
    let script = compile(&source, &[]).unwrap_or_else(|e| {
        let (line, column) = e.line_column(&source);
        let text = source.lines().nth(line - 1).unwrap_or_default();
        panic!("{line}:{column}: {}\n{text}", e.message)
    });
    assert!(script.callback(CallbackKind::Note, None).is_some());
    assert!(script.controls.len() > 100);
}

/// Areia's `on init` runs to the end on the VM without faults beyond the
/// host builtins this recording host leaves unanswered.
#[test]
#[ignore = "needs the Areia library dump in KONTRA_AREIA_KSP"]
fn areia_init_runs() {
    let source = areia();
    let started = std::time::Instant::now();
    let script = compile(&source, &[]).unwrap();
    let compiled = started.elapsed();
    let mut vm = Instance::new(&script.program, 4);
    let mut host = Host::default();
    let co = vm
        .start(script.callback(CallbackKind::Init, None).unwrap(), 0)
        .unwrap();
    let started = std::time::Instant::now();
    assert_eq!(vm.run(&script.program, co, &mut host), Step::Done);
    eprintln!(
        "compile {compiled:?}, {} ops, {} cells, {} controls; init {:?}, {} host calls, faults {:?}",
        script.program.code.len(),
        script.program.cells,
        script.controls.len(),
        started.elapsed(),
        host.calls.len(),
        vm.faults(),
    );
    assert_eq!(vm.faults().bad_code, 0);
    assert_eq!(vm.faults().stack_overflow, 0);
}

// ---- Bindings the corpus needed: `docs/re/gap-inventory-2026-11-01.md` §5 ----

/// `hide_inst_icon` is Kontakt's command for hiding the icon in its own
/// instrument header; the KSP reference has no entry for it (checked in the
/// 7.10 and 8.0 manuals) and KONTRA's page draws no instrument header, so the
/// binding is a no-op that still reaches the host, and the host answers it
/// (`Action::HideInstIcon`). 188 corpus instruments call it, 187 of them
/// (every VSTBuzz Power Move patch) with it as their only compile error.
#[test]
fn hide_inst_icon_is_a_no_op_command() {
    let r = Run::new(
        "on init
            declare $x := 1
            hide_inst_icon
        end on",
    );
    assert_eq!(r.host.calls, [(Action::HideInstIcon, vec![])]);
    assert_eq!(r.int("$x"), 1, "the declaration after it still ran");
    // It takes no arguments, as in the corpus's 188 call sites.
    assert!(
        compile("on init hide_inst_icon(1) end on", &[]).is_err(),
        "hide_inst_icon takes no arguments"
    );
}

/// `$EVENT_VELOCITY` is writable. The manual's `change_velo()` entry is what a
/// write means: "Once the velocity of a particular note event is changed, it
/// becomes the new $EVENT_VELOCITY" — the variable is the running event's
/// velocity, so a write is `change_velo($EVENT_ID, v)`. 22 corpus instruments
/// (Snare 1 Off and its siblings) clamp it and read it back.
#[test]
fn event_velocity_can_be_written() {
    let mut r = Run::new(
        "on init
            declare $h
        end on
        on note
            if ($EVENT_VELOCITY > 127)
                $EVENT_VELOCITY := 127
            end if
            $h := $EVENT_VELOCITY
        end on",
    );
    r.host.velocity = 200;
    assert_eq!(r.callback(CallbackKind::Note, None), Step::Done);
    assert_eq!(
        r.host.calls,
        [(Action::ChangeVelo, vec![1000, 127])],
        "the write is change_velo($EVENT_ID, 127)"
    );
    assert_eq!(r.int("$h"), 127, "the write is the new $EVENT_VELOCITY");
    // Kontakt's other system variables stay read-only.
    let error = |source: &str| compile(source, &[]).unwrap_err().message;
    assert_eq!(
        error("on init $EVENT_ID := 1 end on"),
        "$EVENT_ID is read-only"
    );
    assert_eq!(
        error("on init $EVENT_NOTE := 1 end on"),
        "$EVENT_NOTE is read-only"
    );
}

/// `$NUM_PRESETS` is a script's own constant, not a Kontakt builtin: the
/// manual's example for `declare const $` is exactly
/// `declare const $NUM_PRESETS := 10` followed by an array sized from it, and
/// six corpus instruments use that example verbatim.
#[test]
fn num_presets_is_a_script_constant() {
    let r = Run::new(
        "on init
            declare const $NUM_PRESETS := 10
            declare const $NUM_PARAMETERS := 5
            declare %preset_data[$NUM_PRESETS * $NUM_PARAMETERS]
            declare $count
            $count := num_elements(%preset_data)
        end on",
    );
    assert_eq!(r.int("$count"), 50);
    // A name Kontakt defines is still refused.
    let error = |source: &str| compile(source, &[]).unwrap_err().message;
    assert_eq!(
        error("on init declare $EVENT_ID end on"),
        "$EVENT_ID is a reserved name"
    );
}

/// A declared name wins over the catalog inside `fold`. `$NUM_PRESETS` is a
/// name a script may declare *and* a name the harvested catalog holds (the
/// catalog records what the corpus uses, not what Kontakt owns), so a fold that
/// asks the catalog first turns the declared number into an opaque symbol id —
/// a wrong constant, silently. The declared value is the one that counts.
#[test]
fn a_declared_name_wins_over_the_catalog_when_folding() {
    let r = Run::new(
        "on init
            declare $NUM_PRESETS := 5
            declare const $N := $NUM_PRESETS * 2
            declare %slots[$NUM_PRESETS] := (1, 2, 3, 4, 5)
            declare $size
            declare $doubled
            $size := num_elements(%slots)
            $doubled := $N
        end on",
    );
    assert_eq!(r.int("$size"), 5, "the declared size, not a symbol id");
    assert_eq!(r.int("$doubled"), 10);
    let error = |source: &str| compile(source, &[]).unwrap_err().message;
    // A declared name that holds no constant is not a constant either: the
    // catalog must not answer for it (`$EVENT_PAR_TOY` is declared by 16 corpus
    // scripts and is a catalog name).
    assert_eq!(
        error("on init declare $EVENT_PAR_TOY declare %a[$EVENT_PAR_TOY] end on"),
        "expected a constant"
    );
    // A system variable is still not declarable.
    assert_eq!(
        error("on init declare $EVENT_VELOCITY := 1 end on"),
        "$EVENT_VELOCITY is a reserved name"
    );
}

/// A catalog name is not a constant. `vendor_constants.txt` records the `$`
/// names the corpus uses, not what kind each one is: it holds system variables
/// (`$BEND_RANGE`, `$BUS_IDX`, `$CHANNEL_L` — values that only exist while a
/// callback runs) beside constants. Folding one of those to an opaque symbol id
/// put a symbol id where an array size or a `const` initialiser belongs, which
/// is a silently wrong constant rather than a refusal.
///
/// A *valued* constant still folds (`$MARK_3` is 8), and a catalog name still
/// reads as a symbolic constant in a value position — 242 corpus scripts
/// declare `$5mo1e := $ENGINE_PAR_EFFECT_BYPASS`, an engine parameter whose id
/// this tree does not hold.
#[test]
fn a_catalog_name_that_is_not_a_constant_is_not_folded() {
    let error = |source: &str| compile(source, &[]).unwrap_err().message;
    for name in [
        "$BEND_RANGE",
        "$BUS_IDX",
        "$CHANNEL_L",
        "$ENGINE_PAR_EFFECT_BYPASS",
    ] {
        assert_eq!(
            error(&format!("on init declare %a[{name}] end on")),
            "expected a constant",
            "an array sized by the value {name}"
        );
        assert_eq!(
            error(&format!("on init declare const $Y := {name} end on")),
            "expected a constant",
            "a constant declared from the value {name}"
        );
    }
    // A constant the tree holds a value for still sizes an array.
    let r = Run::new(
        "on init
            declare %marks[$MARK_3]
            declare $n
            $n := num_elements(%marks)
        end on",
    );
    assert_eq!(r.int("$n"), 8, "$MARK_3 is 8");
    // And a catalog name in a value position still compiles: the 242 scripts
    // that copy an engine-parameter id into a variable do exactly this.
    let r = Run::new(
        "on init
            declare $param := $ENGINE_PAR_EFFECT_BYPASS
            declare $read
            $read := $param
        end on",
    );
    assert!(r.int("$read") > 0, "the symbol id, resolved at run time");
}

/// `redirect_output(<event-id>, <output-type>, <index>)`: the manual documents
/// three arguments and says `<index>` "has no effect" for
/// `$OUTPUT_TYPE_DEFAULT`, so a two-argument call routes to index 0. Eleven
/// corpus instruments call it with two.
#[test]
fn redirect_output_takes_two_or_three_arguments() {
    let mut r = Run::new(
        "on note
            redirect_output($EVENT_ID, $OUTPUT_TYPE_DEFAULT)
            redirect_output($EVENT_ID, $OUTPUT_TYPE_BUS_OUT, 3)
        end on",
    );
    assert_eq!(r.callback(CallbackKind::Note, None), Step::Done);
    assert_eq!(r.host.calls.len(), 2);
    assert_eq!(r.host.calls[0].0, Action::RedirectOutput);
    assert_eq!(r.host.calls[0].1[0], 1000);
    assert_eq!(r.symbol(r.host.calls[0].1[1]), Some("$OUTPUT_TYPE_DEFAULT"));
    assert_eq!(r.host.calls[0].1[2], 0, "the left-out index routes to 0");
    assert_eq!(r.host.calls[1].0, Action::RedirectOutput);
    assert_eq!(r.host.calls[1].1[0], 1000);
    assert_eq!(r.symbol(r.host.calls[1].1[1]), Some("$OUTPUT_TYPE_BUS_OUT"));
    assert_eq!(r.host.calls[1].1[2], 3);
}

/// `hide_part(<variable>, <hide-mask>)`'s first argument is a variable name,
/// not necessarily a control: the manual gives every declaration a UI id
/// ("Even regular variables and constants ... get a UI ID assigned"), and ten
/// corpus instruments hide the background of a plain `declare $x` variable.
#[test]
fn hide_part_takes_a_variable_that_is_not_a_control() {
    let r = Run::new(
        "on init
            declare $plain
            declare ui_knob $knob(0, 100, 1)
            hide_part($plain, $HIDE_PART_BG)
            hide_part($knob, $HIDE_PART_BG)
        end on",
    );
    assert_eq!(r.host.calls.len(), 2);
    assert_eq!(r.host.calls[0].0, Action::HidePart);
    assert_eq!(
        r.host.calls[0].1,
        [i64::from(FIRST_UI_ID), 1],
        "the plain variable's UI id and $HIDE_PART_BG"
    );
    assert_eq!(r.host.calls[1].1, [i64::from(FIRST_UI_ID + 1), 1]);
    // The argument still has to be a variable, as the manual says.
    assert!(compile("on init hide_part(3, 1) end on", &[]).is_err());
    // `on ui_control` still needs a real control.
    assert_eq!(
        compile("on init declare $x end on on ui_control($x) end on", &[])
            .unwrap_err()
            .message,
        "not a UI control"
    );
}

/// A constant initialiser may be a pure number function of constants: the
/// arithmetic functions are the manual's own list and have one value, and 24
/// corpus instruments (Heavyocity's FORZO and NOVO, e-instruments' Session
/// Keys, NI's Scene) declare `const ~LOG10 := log(10.0)`.
#[test]
fn constants_fold_number_functions() {
    let r = Run::new(
        "on init
            declare const ~LOG10 := log(10.0)
            declare const $N := int(sqrt(16.0)) + sgn(-3)
            declare $scaled
            declare $plus
            $scaled := int(~LOG10 * 1000000.0)
            $plus := $N + 1
        end on",
    );
    assert_eq!(r.int("$scaled"), 2_302_585, "ln 10 to six places");
    assert_eq!(r.int("$plus"), 4, "int(sqrt(16.0)) + sgn(-3)");
    // A constant that is not a constant is still refused.
    let error = |source: &str| compile(source, &[]).unwrap_err().message;
    assert_eq!(
        error("on init declare $x declare const $Y := $x end on"),
        "expected a constant"
    );
    assert_eq!(
        error("on init declare $x declare const $Y := random(0, 1) end on"),
        "expected a constant"
    );
}

/// `num_elements(%a)` is a constant when `%a` is: an array's size is fixed
/// where it is declared, so the manual's "number of elements must be defined
/// with a constant value" admits it. Heavyocity sizes
/// `%cycle_copy_buffer[num_elements(%cycle_data) + 5]`, NI's Scene - Lotus
/// `%original_events[num_elements(%artificial_events)]`, and e-instruments a
/// `const` from `num_elements()` of four arrays.
#[test]
fn num_elements_folds_in_a_declaration() {
    let r = Run::new(
        "on init
            declare %data[6] := (1, 2, 3, 4, 5, 6)
            declare %copy[num_elements(%data) + 5] := (-1)
            declare const $TOTAL := num_elements(%copy) * 2
            declare $n
            declare $doubled
            $n := num_elements(%copy)
            $doubled := $TOTAL
        end on",
    );
    assert_eq!(r.ints("%copy"), [-1; 11]);
    assert_eq!(r.int("$n"), 11);
    assert_eq!(r.int("$doubled"), 22);
    // A size read from a variable is still not a constant.
    assert_eq!(
        compile("on init declare $n declare %a[$n] end on", &[])
            .unwrap_err()
            .message,
        "expected a constant"
    );
}
