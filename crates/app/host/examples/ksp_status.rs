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
    for b in Builtin::ALL {
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
