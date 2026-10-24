//! Prints every group's modulation routes that target the sample start (`playPos`), for one NKI.
//! Usage: sample_start_routes INSTRUMENT.nki
use ni_file::kontakt::objects::{ExternalModArray32, GroupList, InternalModArray16, Program};
use std::path::Path;

pub fn main() {
    let path = std::env::args().nth(1).expect("usage: sample_start_routes INSTRUMENT.nki");
    let chunks = sampler_kontakt::read_chunks(Path::new(&path)).unwrap();
    let program = Program::try_from(chunks.find_first(0x28).unwrap()).unwrap();
    let groups = GroupList::try_from(program.0.find_first(0x33).unwrap()).unwrap();
    for (id, group) in groups.groups.iter().enumerate() {
        if let Some(c) = group.0.find_first(0x3b) {
            for (slot, m) in InternalModArray16::try_from(c).unwrap().slots().unwrap() {
                let p = m.params().unwrap();
                if p.targets.iter().any(|t| t.param == "playPos") {
                    println!("group {id} internal slot {slot}: {:?}", p);
                }
            }
        }
        if let Some(c) = group.0.find_first(0x3c) {
            for (slot, m) in ExternalModArray32::try_from(c).unwrap().slots().unwrap() {
                let p = m.params().unwrap();
                if p.targets.iter().any(|t| t.param == "playPos") {
                    println!("group {id} external slot {slot}: {:?}", p);
                }
            }
        }
    }
}
