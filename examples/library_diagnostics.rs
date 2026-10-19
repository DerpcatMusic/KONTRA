use std::path::Path;
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, bank] if command == "catalog" => {
            println!(
                "{}",
                serde_json::to_string(&sampler_uvi::Bank::catalog(Path::new(bank))?)?
            );
        }
        [command, path, output] if command == "probe" => {
            let path = path.clone();
            let report = std::thread::Builder::new()
                .stack_size(32 << 20)
                .spawn(move || kontakto::offline_library_diagnostics(Path::new(&path)))?
                .join()
                .map_err(|_| anyhow::anyhow!("probe worker panicked"))?;
            std::fs::write(output, serde_json::to_vec_pretty(&report)?)?;
        }
        _ => anyhow::bail!(
            "usage: library_diagnostics catalog BANK.ufs | probe INPUT.nki-or-BANK.ufs/PROGRAM.uvip OUTPUT.json"
        ),
    }
    Ok(())
}
