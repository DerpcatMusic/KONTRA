//! Explicit local native-record inspection; numeric parameter metadata only.
use super::*;

#[test]
#[ignore = "requires KONTRA_FILTER_AUDIT_PATH; reads one installed instrument without translation or PCM"]
fn installed_filter_record_numeric_audit() {
    let path = std::env::var_os("KONTRA_FILTER_AUDIT_PATH").expect("instrument path required");
    let chunks = crate::read_chunks(Path::new(&path)).unwrap();
    let program = Program::try_from(chunks.find_first(0x28).unwrap()).unwrap();
    let groups =
        ni_file::kontakt::objects::GroupList::try_from(program.0.find_first(0x33).unwrap()).unwrap();
    let mut unusual = 0;
    for (group, object) in groups.groups.iter().enumerate() {
        let Ok(rack) = object.insert_fx() else {
            continue;
        };
        for slot in super::rack(&rack, |_, _| {}) {
            if let Some(Params::Filter {
                kind,
                cutoff,
                resonance,
                extra,
            }) = slot.params()
            {
                if !(0.0..=1.0).contains(&cutoff) || !(0.0..=1.0).contains(&resonance) {
                    unusual += 1;
                    if unusual <= 24 {
                        println!(
                            "FILTER_RECORD {}",
                            serde_json::json!({
                                "group":group,"slot":slot.slot,"version":slot.version,
                                "kind":kind,"bypass":slot.bypass,"cutoff":cutoff,
                                "resonance":resonance,"extra":extra,
                                "public_bytes":slot.public.len(),
                            })
                        );
                    }
                }
            }
        }
    }
    println!(
        "FILTER_RECORD_TOTAL {}",
        serde_json::json!({"groups":groups.groups.len(),"unusual":unusual})
    );
}

use std::path::Path;
