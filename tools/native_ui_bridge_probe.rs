//! Developer-only authored KSP/Lua protocol; no production or renderer integration.
use kontakto::{build_info, engine::{load_scripts, ScriptSetup}, import::Instrument, ksp::Value};
use std::{collections::BTreeMap, io::{BufRead, BufReader, Read, Write}, path::Path,
    process::{Command, Stdio}};

fn value(rt: &kontakto::ksp::Runtime, slot: usize, control: usize) -> i32 {
    match rt.interface(slot).controls[control].properties["$CONTROL_PAR_VALUE"] {
        Value::Int(v) => v,
        _ => panic!("authored probe supports integer scalars only"),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 4, "expected exact revision, tool root, output folder");
    assert_eq!(build_info::BUILD.revision, args[1]);
    assert_eq!(build_info::BUILD.dirty, Some(false));
    let root = Path::new(&args[2]);
    let output = Path::new(&args[3]);
    std::fs::create_dir_all(output)?;
    let instrument = Instrument { scripts: vec![
        "on init\nmake_perfview\ndeclare ui_slider $Amount(0,100)\n$Amount:=17\nend on\non ui_control($Amount)\n$Amount:=$Amount*2\nend on".into(),
        "on init\nmake_perfview\ndeclare ui_slider $Remote(0,100)\n$Remote:=9\ndeclare ui_slider $Amount(0,100)\n$Amount:=90\nend on\non ui_control($Remote)\n$Remote:=$Remote+10\nend on".into(),
    ], ..Default::default() };
    let (runtime, errors) = load_scripts(&instrument, Vec::new(), 48_000.);
    assert!(errors.is_empty(), "{errors:?}");
    let mut runtime = runtime.unwrap();
    let mut host = ScriptSetup::new(&instrument, 48_000.);
    let mut bindings = BTreeMap::new();
    let mut duplicates = Vec::new();
    // NI expose_controls: all slots, identifiers without sigils; first duplicate wins.
    // https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands#expose_controls
    for slot in 0..runtime.slots() {
        for (control, c) in runtime.interface(slot).controls.iter().enumerate() {
            let name = c.variable[1..].to_owned();
            if let std::collections::btree_map::Entry::Vacant(e) = bindings.entry(name.clone()) {
                e.insert((slot, control));
            } else { duplicates.push((name, slot, control)); }
        }
    }
    assert_eq!(duplicates, vec![("Amount".into(), 1, 1)]);
    eprintln!("ignored_duplicate={:?}; documented first slot retained", duplicates[0]);
    let registry = output.join("controls.tsv");
    std::fs::write(&registry, bindings.iter().map(|(name, &(slot, control))|
        format!("{name}\t{slot}\t{control}\t{}\n", value(&runtime, slot, control))).collect::<String>())?;
    let status = Command::new("python3").arg(root.join("tools/native_ui_frontend.py"))
        .arg(root.join("tests/fixtures/native-ui-scene")).arg(output.join("lowered")).status()?;
    assert!(status.success(), "authored DSL lowering failed");
    let mut child = Command::new("python3").arg(root.join("tools/run_native_ui_scene.py"))
        .arg(output.join("lowered")).arg(&registry)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn()?;
    let mut input = child.stdin.take().unwrap();
    let mut stream = BufReader::new(child.stdout.take().unwrap().take(64 * 1024));
    let mut transcript = String::new();
    let (mut requests, mut done) = (0, false);
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut line = String::new();
        while stream.read_line(&mut line)? != 0 {
            transcript.push_str(&line);
            if line.starts_with("REQUEST\t") {
                let fields: Vec<_> = line.trim_end().split('\t').collect();
                assert_eq!(fields.len(), 5);
                let (serial, slot, control, next): (usize,usize,usize,i32) =
                    (fields[1].parse()?,fields[2].parse()?,fields[3].parse()?,fields[4].parse()?);
                requests += 1;
                assert!(requests <= 2 && serial == requests);
                assert!(bindings.values().any(|&address| address == (slot, control)), "unexposed control address");
                runtime.ui_control(&mut host, slot, control, next);
                writeln!(input, "APPLIED\t{serial}\t{}", value(&runtime, slot, control))?;
                input.flush()?;
            }
            if line.starts_with("DONE\t2\t") { done = true; }
            line.clear();
        }
        assert!(done && requests == 2, "bounded scene protocol did not complete");
        Ok(())
    })();
    if result.is_err() { child.kill().ok(); }
    drop(input);
    let status = child.wait()?;
    std::fs::write(output.join("transcript.tsv"), &transcript)?;
    result?;
    assert!(status.success(), "Lua scene process failed");
    assert_eq!((value(&runtime,0,0),value(&runtime,1,0),value(&runtime,1,1)),(44,24,90));
    assert!(transcript.contains("Amount:44:1") && transcript.contains("Remote:24:1"));
    assert!(runtime.diagnostics().is_empty(), "{:?}", runtime.diagnostics());
    println!("PASS authored scene + real callbacks on two KSP slots; duplicate preserved; production=false rendering=false");
    Ok(())
}
