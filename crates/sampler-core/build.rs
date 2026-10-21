//! Generates the engine-parameter id table from `src/engine_par_ids.tsv`, the single source.
//! A row with an id binds a name to that engine id; `-` leaves a native name unbound (the
//! diagnostic path). Bound ids must be contiguous from 0 so existing ids never shift.
use std::{env, fs, path::Path};

fn main() {
    println!("cargo:rerun-if-changed=src/engine_par_ids.tsv");
    println!("cargo:rerun-if-changed=build.rs");
    let text = fs::read_to_string("src/engine_par_ids.tsv").expect("engine_par_ids.tsv");
    let mut bound: Vec<(usize, String)> = Vec::new();
    let mut native: Vec<(String, i32)> = Vec::new();
    for line in text.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let cols: Vec<&str> = line.split('\t').collect();
        assert_eq!(cols.len(), 3, "bad row: {line}");
        let (id, name, constant) = (cols[0], cols[1], cols[2]);
        if id != "-" {
            bound.push((id.parse().expect("engine_id"), name.to_owned()));
        }
        if constant != "-" {
            native.push((name.to_owned(), constant.parse().expect("native_constant")));
        }
    }
    bound.sort();
    for (expected, (id, name)) in bound.iter().enumerate() {
        assert_eq!(*id, expected, "engine ids must be contiguous; {name} breaks the run");
    }
    let mut out = String::from("pub const ENGINE_PARAMETER_NAMES: &[&str] = &[\n");
    for (_, name) in &bound {
        out.push_str(&format!("    \"${name}\",\n"));
    }
    out.push_str("];\n\n");
    out.push_str("/// Native script constant of each recovered `$ENGINE_PAR_*` name, bound or not.\n");
    out.push_str("pub const ENGINE_PARAMETER_NATIVE_CONSTANTS: &[(&str, i32)] = &[\n");
    for (name, value) in &native {
        out.push_str(&format!("    (\"{name}\", {value}),\n"));
    }
    out.push_str("];\n");
    let dest = Path::new(&env::var("OUT_DIR").unwrap()).join("engine_parameter_names.rs");
    fs::write(dest, out).unwrap();
}
