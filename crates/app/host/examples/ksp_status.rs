//! What KONTRA answers of KSP, by name, for `tools/ksp-manual-coverage.py`:
//! one tab-separated line per builtin (`fn`), callback (`cb`) and carried
//! `$ENGINE_PAR_*` name (`par`). A builtin the VM lowers is `vm`; an action
//! the engine answers to a bare probe is `answered`; the rest is `missing`.
//! `cargo run --release -p kontra-host --example ksp_status > status.tsv`

use std::sync::Arc;

use kontra_engine::{Action, Engine, ir};
use kontra_ksp::{Builtin, CallbackKind};
use kontra_stream::{Store, StoreConfig};

const RATE: u32 = 48_000;

fn main() -> std::io::Result<()> {
    let mut store = Store::new(StoreConfig::DEFAULT)?;
    let plan = kontra_host::instrument::build(ir::Instrument::DEFAULT, RATE, &mut store).plan;
    let mut probe = Engine::new(Arc::new(plan), store.client(), RATE);
    // Commands whose answer depends on their arguments (a parameter id, a
    // group) are probed by a script that makes a valid call.
    let by_script: &[(&str, &str)] = &[
        (
            "set_engine_par",
            "set_engine_par($ENGINE_PAR_VOLUME, 0, 0, -1, -1)",
        ),
        (
            "get_engine_par",
            "$x := get_engine_par($ENGINE_PAR_VOLUME, 0, -1, -1)",
        ),
        (
            "get_engine_par_disp",
            "@t := get_engine_par_disp($ENGINE_PAR_VOLUME, 0, -1, -1)",
        ),
        (
            "get_engine_par_disp_ext",
            "@t := get_engine_par_disp_ext($ENGINE_PAR_VOLUME, 0, 0, -1, -1)",
        ),
        // The UI commands: a control to act on, then the call.
        ("set_text", "declare ui_label $l(1,1)\n set_text($l, \"a\")"),
        ("add_text_line", "declare ui_label $l(1,1)\n add_text_line($l, \"a\")"),
        ("hide_part", "declare ui_label $l(1,1)\n hide_part($l, $HIDE_PART_BG)"),
        ("set_control_help", "declare ui_label $l(1,1)\n set_control_help($l, \"a\")"),
        ("set_table_steps_shown", "declare ui_table %t[4](1,1,1)\n set_table_steps_shown(%t, 4)"),
        ("attach_level_meter", "declare ui_level_meter $m\n attach_level_meter($m, 0, 0, 0, -1)"),
        ("get_ui_wf_property", "declare ui_waveform $w(1,1)\n $x := get_ui_wf_property($w, $UI_WF_PROP_FLAGS, 0)"),
        ("move_control", "declare ui_label $l(1,1)\n move_control($l, 1, 1)"),
        ("move_control_px", "declare ui_label $l(1,1)\n move_control_px($l, 1, 1)"),
        ("set_knob_defval", "declare ui_knob $k(0,100,1)\n set_knob_defval($k, 5)"),
        ("set_knob_label", "declare ui_knob $k(0,100,1)\n set_knob_label($k, \"a\")"),
        ("set_knob_unit", "declare ui_knob $k(0,100,1)\n set_knob_unit($k, $KNOB_UNIT_DB)"),
        ("set_ui_height_px", "set_ui_height_px(100)"),
        ("set_ui_width_px", "set_ui_width_px(100)"),
        ("set_ui_height", "set_ui_height(2)"),
        ("set_skin_offset", "set_skin_offset(0)"),
        ("set_ui_color", "set_ui_color($00ff00ff)"),
        ("set_script_title", "set_script_title(\"a\")"),
        // Zone, event and file commands.
        ("set_num_user_zones", "set_num_user_zones(4)"),
        ("get_event_par_arr", "$x := get_event_par_arr(0, $EVENT_PAR_CUSTOM, 0)"),
        ("set_event_par_arr", "set_event_par_arr(0, $EVENT_PAR_CUSTOM, 1, 0)"),
        ("get_folder", "@t := get_folder($GET_FOLDER_LIBRARY_DIR)"),
        ("detect_sample_type", "$x := detect_sample_type(0, @t)"),
        ("detect_drum_type", "$x := detect_drum_type(0, @t)"),
        ("detect_instrument_type", "$x := detect_instrument_type(0, @t)"),
        (
            "get_voice_limit",
            "$x := get_voice_limit($NI_VL_TMPRO_STANDARD)",
        ),
    ];
    let answered_by_script = |call: &str| -> bool {
        let src = format!("on init\n declare $x\n declare @t\n {call}\nend on");
        let Ok(script) = kontra_ksp::compile(&src, &[]) else {
            return false;
        };
        let Ok(mut store) = Store::new(StoreConfig::DEFAULT) else {
            return false;
        };
        let mut plan =
            kontra_host::instrument::build(ir::Instrument::DEFAULT, RATE, &mut store).plan;
        plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
        Engine::new(Arc::new(plan), store.client(), RATE)
            .unhandled_actions()
            .is_empty()
    };
    for b in Builtin::ALL {
        if let Some((_, call)) = by_script.iter().find(|(n, _)| *n == b.name()) {
            let status = if answered_by_script(call) {
                "answered"
            } else {
                "missing"
            };
            println!("fn\t{}\t{status}", b.name());
            continue;
        }
        let status = match b.action() {
            None => "vm",
            Some(a) if Action::is_pure(a) => "vm",
            Some(a) if probe.answers(a) => "answered",
            Some(_) => "missing",
        };
        println!("fn\t{}\t{status}", b.name());
    }
    for c in CallbackKind::ALL {
        println!("cb\t{c:?}\tcompiled");
    }
    for p in kontra_format_kontakt::engine_pars() {
        println!("par\t{}\tcarried", p.name);
    }
    Ok(())
}
