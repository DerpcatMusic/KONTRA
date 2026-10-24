//! Throwaway: dump each multi slot's public bytes and child chunk ids.
use ni_file::kontakt::objects::Bank;
use std::path::Path;

fn main() {
    let path = std::env::args().nth(1).expect("usage: multi_slots FILE.nkm");
    let chunks = sampler_kontakt::read_chunks(Path::new(&path)).expect("chunks");
    let bank = Bank::try_from(chunks.find_first(3).expect("bank")).expect("bank parse");
    let mut slots: Vec<_> = bank.slot_list().expect("slots").slots.into_iter().collect();
    slots.sort_by_key(|(s, _)| *s);
    for (slot, container) in slots {
        let so = &container.0;
        let head: Vec<String> = so.public_data.iter().take(48).map(|b| format!("{b:02x}")).collect();
        println!("slot {slot} v{} public[{}] {}", so.version, so.public_data.len(), head.join(" "));
        for c in &so.children {
            let hex: Vec<String> = c.data.iter().map(|b| format!("{b:02x}")).collect();
            println!("   child {:#06x} len {} {}", c.id, c.data.len(), hex.join(" "));
        }
    }
}
