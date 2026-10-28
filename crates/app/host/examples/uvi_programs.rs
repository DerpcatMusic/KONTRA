//! Lists the programs in UVI banks, one `bank.ufs/Program.uvip` path a line,
//! for corpus surveys (`tools/v3-survey`).
//! `cargo run --release -p kontra-host --example uvi_programs -- <bank.ufs>...`

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for bank in std::env::args_os().skip(1) {
        let bank = std::path::PathBuf::from(bank);
        for program in kontra_format_uvi::programs(&bank)? {
            println!("{}", bank.join(program).display());
        }
    }
    Ok(())
}
