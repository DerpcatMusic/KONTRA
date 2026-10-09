//! Untimed native-slot census. Emits metadata only; never writes library payloads.
#[path = "../../../src/ui/scan_coverage.rs"]
mod coverage;
use serde_json::json;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let item = std::env::args()
        .nth(1)
        .ok_or("expected instrument path or bank::program")?;
    let mut programs = Vec::new();
    if let Some((path, member)) = item.split_once("::") {
        let bank = sampler_uvi::Bank::open(Path::new(path))?;
        let ir = sampler_uvi::translate_program(&bank, member)?;
        programs.push(json!({"program":0,"loaded":true,"dsp_slots":coverage::slots(&ir),"family_native":coverage::native_family(&ir,None,None)}));
    } else if Path::new(&item)
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("nkm"))
    {
        for program in 0..64 {
            match sampler_kontakt::read_program(Path::new(&item),program) {
                Ok(native)=>programs.push(json!({"program":program,"loaded":true,"dsp_slots":coverage::slots(&native.instrument),"family_native":coverage::native_family(&native.instrument,None,None)})),
                Err(e) if program>0 && matches!(e.cause(),sampler_kontakt::LoadError::Invalid{reason,..} if reason=="the multi has no such program")=>break,
                Err(e)=>return Err(Box::new(e)),
            }
        }
    } else {
        let native = sampler_kontakt::read(Path::new(&item))?;
        programs.push(json!({"program":0,"loaded":true,"dsp_slots":coverage::slots(&native.instrument),"family_native":coverage::native_family(&native.instrument,None,None)}));
    }
    println!(
        "{}",
        json!({"basis":"untimed-native-slot-census","programs":programs})
    );
    Ok(())
}
