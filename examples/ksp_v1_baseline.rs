//! Baseline: how many corpus scripts the v1 KSP runtime compiles and initializes.
//! `cargo run --release --example ksp_v1_baseline -- <corpus-dir>`
fn main() {
    let dir = std::env::args().nth(1).expect("corpus dir");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    names.sort();
    let (mut ok, mut total) = (0, 0);
    for path in names {
        let source = std::fs::read_to_string(&path).unwrap_or_default();
        total += 1;
        match kontakto::ksp::initialize(&source, 64, 8) {
            Ok(ui) => {
                ok += 1;
                println!(
                    "ok\t{}\t{} controls\t{} diagnostics",
                    path.display(),
                    ui.controls.len(),
                    ui.diagnostics.len()
                );
            }
            Err(error) => println!("fail\t{}\t{error:#}", path.display()),
        }
    }
    println!("{ok}/{total} initialized");
}
