//! Times one UVI program's open, read and script-init, then prints the init stage profile (JSON).
//! Usage: init_profile BANK.ufs PROGRAM
use std::{path::Path, time::Instant};

use sampler_uvi::script::{Config, ScriptHost};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let t = Instant::now();
    let _meta = sampler_uvi::Bank::open_metadata(Path::new(&args[1]))?;
    eprintln!("bank_metadata_ms={:.1}", t.elapsed().as_secs_f64() * 1e3);
    let t = Instant::now();
    let bank = sampler_uvi::Bank::open(Path::new(&args[1]))?;
    eprintln!("bank_open_ms={:.1}", t.elapsed().as_secs_f64() * 1e3);
    let t = Instant::now();
    let (xml, _) = bank.program(&args[2])?;
    eprintln!("program_read_ms={:.1} xml_bytes={}", t.elapsed().as_secs_f64() * 1e3, xml.len());
    let t = Instant::now();
    let host = ScriptHost::new_profiled(&xml, bank.scripts(), Config::default(), false)?;
    eprintln!("script_host_ms={:.1}", t.elapsed().as_secs_f64() * 1e3);
    println!("{}", host.initialization_profile());
    Ok(())
}
