//! Deterministic lifecycle boundary tests for KONTRA Task #139:
//! - Generation fencing across preset swaps (Control, Mix, Hold, Remove, SetParam, MacroTarget)
//! - Zero head-of-line blocking when `looks` or `captured` rings are full
//! - Zero idle `Command::Look` and `Command::Capture` across 100 audio blocks
//! - 1-block event-driven wake & `applied_seq` acknowledgement

use kontra_host::rack::{
    Command, Control, MacroTarget, Mix, Rack, RackAudio, Scale, Store, StoreConfig, ir,
};
use kontra_host::view::Interfaces;
use kontra_ui_model::UiModifiers;

fn ksp_instrument(name: &str, source: &str) -> ir::Instrument {
    let mut inst = ir::Instrument {
        name: name.into(),
        ..ir::Instrument::DEFAULT
    };
    inst.scripts.push(ir::Script {
        name: "main".into(),
        slot: 0,
        dialect: ir::Dialect::Ksp,
        source: source.into(),
        saved: Vec::new(),
        data: Vec::new(),
        links: Vec::new(),
        container: None,
        player: false,
    });
    inst
}

fn settle(rack: &mut Rack, audio: &mut RackAudio) {
    for _ in 0..3 {
        rack.tick();
        audio.process(64, &[]);
    }
    rack.tick();
    let _ = audio.take_wake();
}

/// AC5(a): Stale commands (`Control`, `Mix`, `Hold`, `Remove`, `SetParam`,
/// and `MacroTarget`) stamped with an old generation after a preset swap MUST
/// be discarded on the audio thread without mutating the new generation.
#[test]
fn lifecycle_stale_commands_after_preset_swap_are_discarded() {
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (mut rack, mut audio) = Rack::new(48_000, store, 256);

    let inst_a = ksp_instrument(
        "PresetA",
        "on init
            make_perfview
            declare ui_knob $k (0, 100, 1)
            $k := 10
            make_persistent($k)
        end on",
    );
    rack.load_ir(0, inst_a, None).unwrap();
    settle(&mut rack, &mut audio);
    let gen_a = rack.part_generation(0).unwrap();
    let widget_a = rack.parts()[0]
        .as_ref()
        .unwrap()
        .view
        .as_ref()
        .unwrap()
        .control(0, 0.0)
        .unwrap();

    // Bind a host macro to generation A on the audio thread.
    rack.set_macro(
        0,
        Some(MacroTarget {
            part: 0,
            generation: gen_a,
            slot: widget_a.0,
            control: Control::Integer(widget_a.1),
            scale: Scale::Range {
                min: 0.0,
                max: 100.0,
            },
        }),
    );
    settle(&mut rack, &mut audio);

    // Swap preset B into part 0 (advancing generation to gen_b > gen_a).
    let inst_b = ksp_instrument(
        "PresetB",
        "on init
            make_perfview
            declare ui_knob $k (0, 100, 1)
            $k := 50
            make_persistent($k)
        end on",
    );
    rack.load_ir(0, inst_b, None).unwrap();
    settle(&mut rack, &mut audio);
    let gen_b = rack.part_generation(0).unwrap();
    assert!(gen_b > gen_a, "preset swap must advance generation");

    // Inject stale generation-A commands directly into the audio command ring:
    // Control, Mix, Hold, Remove, SetParam — plus a stale generation-A macro move.
    let cmds = &rack.channels().commands;
    assert!(
        cmds.send(Command::Control {
            part: 0,
            generation: gen_a,
            ui_seq: 99,
            slot: widget_a.0,
            id: widget_a.1,
            cell: 0,
            value: 99,
            keys: UiModifiers::default(),
        })
        .is_ok()
    );
    assert!(
        cmds.send(Command::Mix {
            part: 0,
            generation: gen_a,
            mix: Mix {
                channel: 7,
                output: 3,
                gain: 0.1,
                pan: -0.5,
                silent: true,
            },
        })
        .is_ok()
    );
    assert!(
        cmds.send(Command::Hold {
            part: 0,
            generation: gen_a,
            hold: kontra_host::rack::Hold::flat(128),
        })
        .is_ok()
    );
    assert!(
        cmds.send(Command::SetParam {
            part: 0,
            generation: gen_a,
            ui_seq: 100,
            scope: ir::Scope::Instrument,
            addr: ir::ParamAddr::Volume,
            value: 0.0,
        })
        .is_ok()
    );
    assert!(
        cmds.send(Command::Remove {
            part: 0,
            generation: gen_a,
        })
        .is_ok()
    );

    // Also attempt a stale macro move targeting gen_a while gen_b is live.
    assert!(
        !audio.move_macro(0, 0.99),
        "stale MacroTarget for gen_a must be rejected once gen_b is installed"
    );

    audio.process(64, &[]);
    // Force a fresh Look on gen_b to inspect the live engine state.
    rack.show_interfaces(Interfaces::Hidden);
    rack.show_interfaces(Interfaces::Shown);
    rack.tick();
    audio.process(64, &[]);
    rack.tick();

    let part_b = rack.parts()[0].as_ref().expect("part 0 must still exist");
    let view_b = part_b.view.as_ref().unwrap();
    assert_eq!(
        view_b.applied_seq, 0,
        "stale gen_a Control/SetParam must not advance gen_b's applied_seq"
    );
    assert_eq!(
        view_b.tree().widgets[0].number(),
        50.0,
        "stale gen_a Control and MacroTarget must not mutate gen_b's knob"
    );
}

/// AC5(d): Full `looks` and `captured` return rings MUST NOT block
/// `Command::Control` or `Command::Panic`, even when a `Command::Look` is
/// already queued ahead of them.
#[test]
fn lifecycle_full_looks_and_captured_rings_do_not_block_control_or_panic() {
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (mut rack, mut audio) = Rack::new(48_000, store, 256);

    let inst = ksp_instrument(
        "HoLTest",
        "on init
            make_perfview
            declare ui_knob $k (0, 100, 1)
            $k := 10
            make_persistent($k)
        end on",
    );
    rack.load_ir(0, inst, None).unwrap();
    settle(&mut rack, &mut audio);
    let generation = rack.part_generation(0).unwrap();
    let (slot, id, _) = rack.parts()[0]
        .as_ref()
        .unwrap()
        .view
        .as_ref()
        .unwrap()
        .control(0, 0.0)
        .unwrap();

    // Fill `looks` and `captured` rings completely to capacity.
    let mut dummy_captures = 0;
    while !rack.channels().captured.is_full() {
        let vars = &rack.parts()[0].as_ref().unwrap().script.vars;
        let c = kontra_host::persist::Capture::new(0, generation, vars);
        assert!(rack.channels().captured.send(c).is_ok());
        dummy_captures += 1;
    }
    assert!(rack.channels().captured.is_full() && dummy_captures > 0);

    // Fill `looks` ring completely to capacity using distinct LookFrames.
    let mut filler_frames = Vec::new();
    for _ in 0..32 {
        let tmp_store = Store::new(StoreConfig::DEFAULT).unwrap();
        let (mut tmp_rack, mut tmp_audio) = Rack::new(48_000, tmp_store, 64);
        let tmp_inst = ksp_instrument(
            "Filler",
            "on init
                make_perfview
                declare ui_knob $k (0, 100, 1)
            end on",
        );
        tmp_rack.load_ir(0, tmp_inst, None).unwrap();
        tmp_rack.tick();
        tmp_audio.process(64, &[]);
        if let Some(frame) = tmp_rack.channels().looks.recv() {
            filler_frames.push(frame);
        }
    }
    for frame in filler_frames {
        let _ = rack.channels().looks.send(frame);
    }
    assert!(
        rack.channels().looks.is_full(),
        "looks ring must be completely full for HoL test"
    );

    // Queue a `Control` (ui_seq=42, value=77) and `Panic` while BOTH `looks`
    // and `captured` are 100% full, with a `Look` queued in front of them!
    let extra_store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (mut extra_rack, mut extra_audio) = Rack::new(48_000, extra_store, 64);
    extra_rack
        .load_ir(
            0,
            ksp_instrument(
                "StalledLook",
                "on init\n make_perfview\n declare ui_knob $k (0, 100, 1)\n end on",
            ),
            None,
        )
        .unwrap();
    extra_rack.tick();
    extra_audio.process(64, &[]);
    let stalled_look = extra_rack.channels().looks.recv().unwrap();

    assert!(
        rack.channels()
            .commands
            .send(Command::Look(stalled_look))
            .is_ok()
    );
    assert!(
        rack.channels()
            .commands
            .send(Command::Control {
                part: 0,
                generation,
                ui_seq: 42,
                slot,
                id,
                cell: 0,
                value: 77,
                keys: UiModifiers::default(),
            })
            .is_ok()
    );
    assert!(rack.channels().commands.send(Command::Panic).is_ok());

    // Process ONE audio block while `looks` and `captured` are STILL full.
    let _ = audio.take_wake();
    audio.process(64, &[]);
    assert!(
        audio.take_wake(),
        "Control executed despite full looks & captured rings and woke Tick"
    );

    // Now drain the filler entries from `looks` and `captured`, then run one
    // Look round-trip to verify `Control` (value=77, ui_seq=42) ALREADY ran!
    while rack.channels().looks.recv().is_some() {}
    while rack.channels().captured.recv().is_some() {}
    rack.show_interfaces(Interfaces::Hidden);
    rack.show_interfaces(Interfaces::Shown);
    rack.tick();
    audio.process(64, &[]);
    rack.tick();

    let view = rack.parts()[0].as_ref().unwrap().view.as_ref().unwrap();
    assert_eq!(
        view.applied_seq, 42,
        "Control with ui_seq=42 must have executed immediately without HoL stall"
    );
    assert_eq!(view.tree().widgets[0].number(), 77.0);
}

/// AC1 & AC2: With an open editor and idle instrument, `Command::Look` and
/// `Command::Capture` counts stay at 0 across 100 blocks, and a single UI
/// control edit completes round-trip + wakes `Tick` in 1 audio block.
#[test]
fn lifecycle_zero_idle_look_and_capture_across_100_blocks() {
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (mut rack, mut audio) = Rack::new(48_000, store, 256);

    let inst = ksp_instrument(
        "IdleCheck",
        "on init
            make_perfview
            declare ui_slider $s (0, 100)
            declare ui_label $lbl (1, 1)
            $s := 20
            set_text($lbl, \"20\")
            make_persistent($s)
        end on
        on ui_control ($s)
            set_text($lbl, $s)
        end on",
    );
    rack.show_interfaces(Interfaces::Shown);
    rack.load_ir(0, inst, None).unwrap();
    settle(&mut rack, &mut audio);

    let looks_before = rack.looks_sent();
    let captures_before = rack.captures_sent();

    for i in 0..100 {
        audio.process(64, &[]);
        assert!(
            !audio.take_wake(),
            "idle block {i} must not spuriously wake Tick"
        );
        rack.tick();
    }
    assert_eq!(
        rack.looks_sent(),
        looks_before,
        "0 Command::Look must be sent across 100 idle blocks"
    );
    assert_eq!(
        rack.captures_sent(),
        captures_before,
        "0 Command::Capture must be sent across 100 idle blocks"
    );

    // One UI drag move -> pipelines Control + Look in 1 audio block + immediate wake!
    rack.control_with_seq(0, 0, None, 88.0, UiModifiers::default(), 7);
    rack.tick();
    assert_eq!(
        rack.looks_sent(),
        looks_before + 1,
        "UI control edit pipelines exactly one Command::Look"
    );
    audio.process(64, &[]);
    assert!(
        audio.take_wake(),
        "audio must immediately signal wake_tick on LookFrame return"
    );
    rack.tick();

    let view = rack.parts()[0].as_ref().unwrap().view.as_ref().unwrap();
    assert_eq!(view.applied_seq, 7);
    assert_eq!(view.tree().widgets[0].number(), 88.0);
    assert_eq!(view.tree().widgets[1].text, "88");

    // And returns to 0 idle Look/Capture immediately afterwards!
    let looks_after = rack.looks_sent();
    let captures_after = rack.captures_sent();
    for i in 0..20 {
        audio.process(64, &[]);
        assert!(!audio.take_wake(), "post-drag idle block {i} stays quiet");
        rack.tick();
    }
    assert_eq!(rack.looks_sent(), looks_after);
    assert_eq!(rack.captures_sent(), captures_after);
}
