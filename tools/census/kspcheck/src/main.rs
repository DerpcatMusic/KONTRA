//! Compile every KSP file named on stdin; print `<ok|err>\t<path>\t<message>`.
use std::io::{self, BufRead};

fn main() {
    let stdin = io::stdin();
    for line in stdin.lock().lines() {
        let Ok(path) = line else { continue };
        let Ok(src) = std::fs::read_to_string(&path) else {
            println!("err\t{path}\tunreadable");
            continue;
        };
        match kontra_ksp::compile(&src, &[]) {
            Ok(_) => println!("ok\t{path}\t"),
            Err(e) => println!("err\t{path}\t{}", e.to_string().replace('\n', " ")),
        }
    }
}
