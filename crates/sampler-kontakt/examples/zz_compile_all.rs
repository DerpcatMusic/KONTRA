//! Local debug probe (untracked): compile every script slot of each .nki given.
//! Usage: zz_compile_all FILE.nki...
fn main() {
    for path in std::env::args().skip(1) {
        let Ok(loaded) = sampler_kontakt::read(std::path::Path::new(&path)) else {
            println!("{path}\tDECODE FAILED");
            continue;
        };
        for (index, b) in loaded.instrument.behaviors.iter().enumerate() {
            let slot = b.slot.unwrap_or(index as u8);
            let environment = sampler_ksp::Environment {
                slot,
                groups: loaded.instrument.groups.iter().map(|g| g.name.clone()).collect(),
                ..Default::default()
            };
            let result = sampler_ksp::initialize(&b.source, sampler_ksp::Limits::LIBRARY, &environment)
                .and_then(|i| sampler_ksp::compile_initialized(&b.source, 48000, sampler_ksp::Limits::LIBRARY, &[], i));
            let verdict = match result {
                Ok(_) => "ok".to_string(),
                Err(e) => format!("{}:{}: {}", e.line, e.column, e.message),
            };
            println!("{}\t{slot}\t{}\t{verdict}", path.rsplit('/').next().unwrap(), b.name);
        }
    }
}
