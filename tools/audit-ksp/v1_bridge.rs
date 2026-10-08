use std::io::{BufRead, Write};
fn main() {
    let stdin = std::io::stdin();
    let mut out = std::io::BufWriter::new(std::io::stdout());
    for line in stdin.lock().lines() {
        let req: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let source = req["source"].as_str().unwrap();
        let groups = req["groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let compiled = kontakto::ksp::audit_compile(source, groups.len());
        let mut engine = kontakto::ksp::LogEngine::new(groups, 48000.);
        let entries = req["saved"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        let mut rt = kontakto::ksp::Runtime::new(
            Default::default(),
            8,
            vec![kontakto::ksp::saved_persistence(&entries)],
        );
        let started = std::time::Instant::now();
        let init = rt.load(&mut engine, source).is_ok();
        let mut result = serde_json::json!({"compile":compiled.is_some(), "block_errors":compiled.as_ref().map(|c|c.0), "init":init,"us":started.elapsed().as_micros(),"diagnostics":compiled.map(|c|c.1.len()).unwrap_or_default(), "runtime_diagnostics":rt.diagnostics().len()});
        if req["probe"].as_bool() == Some(true) {
            if let Some(actions) = req["actions"].as_array() {
                for a in actions {
                    match a[0].as_str().unwrap() {
                        "note" => rt.note_on(&mut engine, 0, 60, 100),
                        "release" => rt.note_off(&mut engine, 0, 60),
                        "controller" => rt.controller(&mut engine, 0, 1, 127),
                        "process" => {
                            engine.block_start = rt.now();
                            rt.process(&mut engine, a[1].as_u64().unwrap() as u32);
                        }
                        "transport" => rt.set_host_transport(
                            &mut engine,
                            a[1].as_bool().unwrap(),
                            a[2].as_f64().unwrap(),
                            0.,
                            (4, 4),
                        ),
                        _ => panic!("unknown authored action"),
                    }
                }
            }
            result["controls"] = serde_json::to_value(rt.interface(0).controls).unwrap();
            result["calls"] = serde_json::to_value(engine.calls).unwrap();
            result["faults"] = serde_json::json!(rt.faults().count());
        }
        writeln!(out, "{}", result).unwrap();
        out.flush().unwrap();
    }
}
