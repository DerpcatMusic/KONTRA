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
        $i := $a and 1234
        $j := $b or 5
        $k := $a xor $b
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
fn control_flow_functions_and_inc_dec() {
    assert_init_equivalent(
        "function bump
          inc($s)
          if ($s = 3) exit end if
        end function
        on init
        declare $i := 0
        declare $s := 0
        declare $t := 0
        while ($i < 10)
          inc($i)
          if ($i mod 2 = 0) continue end if
          $t := $t + $i
        end while
        while (1)
          call bump()
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


