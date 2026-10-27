#![cfg(test)]
//! KSP source compiled and run on the VM with a recording host.

use kontra_ksp::{CallbackKind, Cells, FIRST_UI_ID, Persistence, Script, compile};
use kontra_vm::{Action, HostApi, HostCall, HostValue, Instance, Step};

#[derive(Default)]
struct Host {
    messages: Vec<String>,
    calls: Vec<(Action, Vec<i64>)>,
    note: i32,
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
    assert_eq!(r.int("$a"), 5);
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

/// The 275k-line Areia script compiles, when the library dump is present.
#[test]
fn areia_compiles() {
    let path = "/data/dev/workspace/PluginArtifacts/layer-onset-20261024/areia-scripts/778d5bb709976a72.ksp";
    let Ok(source) = std::fs::read_to_string(path) else {
        return;
    };
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
fn areia_init_runs() {
    let path = "/data/dev/workspace/PluginArtifacts/layer-onset-20261024/areia-scripts/778d5bb709976a72.ksp";
    let Ok(source) = std::fs::read_to_string(path) else {
        return;
    };
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
