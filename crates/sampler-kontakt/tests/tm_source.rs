//! Time Machine source-record census over an installed Conflux. Prints
//! aggregates of the stored source fields only; no library payloads.
use ni_file::kontakt::objects::{GroupList, Program, SourceValue};
use std::{collections::BTreeMap, path::Path};

#[test]
#[ignore = "requires installed Conflux; run through kontakto-heavy"]
fn conflux_source_record_census() {
    let path = Path::new(
        "/mnt/MAIN_STORAGE/Libraries/Kontakt/Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki",
    );
    assert!(path.is_file(), "required installed gate absent");
    let chunks = sampler_kontakt::read_chunks(path).unwrap();
    let program = Program::try_from(chunks.find_first(0x28).unwrap()).unwrap();
    let groups = GroupList::try_from(program.0.find_first(0x33).unwrap()).unwrap();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for group in &groups.groups {
        let (params, _) = group.source_params().unwrap();
        for f in &params.fields {
            let v = match f.value {
                SourceValue::Float(v) => format!("{v:.4}"),
                SourceValue::Integer(v) => v.to_string(),
                SourceValue::Flag(v) => v.to_string(),
            };
            *seen
                .entry(format!("v{:x} mode{} @{} {} = {v}", params.version, params.mode, f.offset, f.name))
                .or_default() += 1;
        }
        *seen
            .entry(format!("v{:x} mode{} groups", params.version, params.mode))
            .or_default() += 1;
    }
    for (k, n) in seen {
        println!("{k}  x{n}");
    }
}

#[test]
#[ignore = "requires installed Conflux; run through kontakto-heavy"]
fn conflux_script_source_parameter_usage() {
    let path = Path::new(
        "/mnt/MAIN_STORAGE/Libraries/Kontakt/Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki",
    );
    let instrument = sampler_kontakt::read(path).unwrap().instrument;
    for token in [
        "ENGINE_PAR_SPEED",
        "ENGINE_PAR_SPEED_UNIT",
        "ENGINE_PAR_SMOOTH",
        "ENGINE_PAR_GRAIN_LENGTH",
        "ENGINE_PAR_TRANSIENT_SIZE",
        "ENGINE_PAR_TM_LEGATO",
        "ENGINE_PAR_TMPRO_KEEP_FORMANTS",
        "ENGINE_PAR_FORMANT_SHIFT",
        "ENGINE_PAR_SOURCE_MODE",
        "NI_SOURCE_MODE",
        "TIME_MACHINE",
    ] {
        let n: usize = instrument.behaviors.iter().map(|b| b.source.matches(token).count()).sum();
        println!("{token}: {n}");
    }
}
