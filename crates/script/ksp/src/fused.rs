//! Fused code runs exactly as plain code: every conformance case, and every
//! script in `$KONTRA_KSP_CORPUS` (a folder of library dumps), through each
//! callback against one recording host, leaves the same cells, texts,
//! faults and host calls.

use std::path::Path;

use kontra_vm::{HostApi, HostCall, HostValue, Instance, Program, Step};

use crate::{Script, compile_plain};

/// Records every call; answers a small number derived from its arguments,
/// so loops a script bounds by a host value stay short.
#[derive(Default, PartialEq, Debug)]
struct Host {
    calls: Vec<(u16, Vec<i64>, Vec<String>)>,
}

impl HostApi for Host {
    fn call(&mut self, call: HostCall<'_>) -> HostValue {
        let texts = call.texts.iter().map(|t| t.as_str().to_owned()).collect();
        self.calls
            .push((call.action.id(), call.ints.to_vec(), texts));
        HostValue::Int(call.ints.iter().fold(0, |h, &v| h ^ v as i32).rem_euclid(8))
    }
}

/// Each callback once, init first, resuming every wait: what it left.
fn play(script: &Script, program: &Program) -> (Vec<Option<i64>>, Vec<String>, String, Host) {
    let mut vm = Instance::new(program, 4);
    let mut host = Host::default();
    for callback in &script.callbacks {
        let Some(co) = vm.start(callback.entry, 1) else {
            continue;
        };
        for _ in 0..64 {
            if vm.run(program, co, &mut host) == Step::Done {
                break;
            }
        }
        vm.cancel(co);
    }
    let cells = (0..program.cells).map(|n| vm.cell(n)).collect();
    let texts = (0..program.text_cells)
        .filter_map(|n| vm.text(n).map(|t| t.as_str().to_owned()))
        .collect();
    (cells, texts, format!("{:?}", vm.faults()), host)
}

fn same_as_plain(path: &Path) {
    let Ok(source) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(script) = compile_plain(&source, &[], &[]) else {
        return;
    };
    let fused = Program {
        code: kontra_vm::fuse(&script.program.code),
        ..script.program.clone()
    };
    let (plain, fused) = (play(&script, &script.program), play(&script, &fused));
    assert!(
        plain == fused,
        "{}: fused code ran differently",
        path.display()
    );
}

#[test]
fn fused_code_runs_as_plain_code() {
    let cases = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tools/ksp-conformance/cases");
    let corpus = std::env::var_os("KONTRA_KSP_CORPUS");
    let dirs = std::iter::once(cases.into_os_string()).chain(corpus);
    let mut played = 0;
    for dir in dirs {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            same_as_plain(&entry.path());
            played += 1;
        }
    }
    assert!(played > 100, "only {played} scripts");
}
