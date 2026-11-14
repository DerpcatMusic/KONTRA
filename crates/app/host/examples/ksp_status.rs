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
        ("set_engine_par", "set_engine_par($ENGINE_PAR_VOLUME, 0, 0, -1, -1)"),
        ("get_engine_par", "$x := get_engine_par($ENGINE_PAR_VOLUME, 0, -1, -1)"),
        ("get_engine_par_disp", "@t := get_engine_par_disp($ENGINE_PAR_VOLUME, 0, -1, -1)"),
        ("get_engine_par_disp_ext", "@t := get_engine_par_disp_ext($ENGINE_PAR_VOLUME, 0, 0, -1, -1)"),
        ("get_voice_limit", "$x := get_voice_limit($NI_VL_TMPRO_STANDARD)"),
    ];
    let answered_by_script = |call: &str| -> bool {
        let src = format!("on init\n declare $x\n declare @t\n {call}\nend on");
        let Ok(script) = kontra_ksp::compile(&src, &[]) else {
            return false;
        };
        let Ok(mut store) = Store::new(StoreConfig::DEFAULT) else {
            return false;
        };
        let mut plan = kontra_host::instrument::build(ir::Instrument::DEFAULT, RATE, &mut store).plan;
        plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
        Engine::new(Arc::new(plan), store.client(), RATE).unhandled_actions().is_empty()
    };
    for b in Builtin::ALL {
        if let Some((_, call)) = by_script.iter().find(|(n, _)| *n == b.name()) {
            let status = if answered_by_script(call) { "answered" } else { "missing" };
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
