//! Print the Kontakt object tree of one .nki: chunk ids, lengths, structured versions and
//! children, with the group and zone records decoded (source fields, public and private
//! bytes). Sample data is never printed, only record bytes.
//!
//!   nki_tree FILE.nki
use ni_file::kontakt::objects::{GroupList, ZoneList};
use ni_file::kontakt::{Chunk, StructuredObject};

fn hex(bytes: &[u8], max: usize) -> String {
    let shown: Vec<String> = bytes.iter().take(max).map(|b| format!("{b:02x}")).collect();
    let more = if bytes.len() > max { " ..." } else { "" };
    format!("{}{more}", shown.join(" "))
}

const DETAIL: [u16; 9] = [0x04, 0x05, 0x2a, 0x2c, 0x39, 0x0e, 0x35, 0x4e, 0x45];

fn records(chunk: &Chunk, depth: usize) {
    let pad = "  ".repeat(depth);
    if chunk.id == 0x33 {
        for (g, group) in GroupList::try_from(chunk)
            .expect("groups")
            .groups
            .iter()
            .enumerate()
        {
            let mode = group.source_identity().map(|s| s.mode);
            println!(
                "{pad}group {g} mode={mode:?} public={} private={}",
                hex(&group.0.public_data, 200),
                hex(&group.0.private_data, 64)
            );
            if let Ok((params, _)) = group.source_params() {
                for f in &params.fields {
                    println!("{pad}  src 0x{:x} {:<22} {:?}", f.offset, f.name, f.value);
                }
            }
        }
    }
    if chunk.id == 0x34 {
        for (z, zone) in ZoneList::try_from(chunk)
            .expect("zones")
            .zones()
            .iter()
            .enumerate()
        {
            println!(
                "{pad}zone {z} version=0x{:x} public={} private={}",
                zone.0.version,
                hex(&zone.0.public_data, 200),
                hex(&zone.0.private_data, 200)
            );
            for child in &zone.0.children {
                dump(child, depth + 1);
            }
        }
    }
}

fn dump(chunk: &Chunk, depth: usize) {
    let pad = "  ".repeat(depth);
    records(chunk, depth);
    let Ok(object) = StructuredObject::try_from(chunk) else {
        println!(
            "{pad}0x{:04x} len={} (not structured)",
            chunk.id,
            chunk.data.len()
        );
        return;
    };
    println!(
        "{pad}0x{:04x} len={} version=0x{:x} private={} public={} children={}",
        chunk.id,
        chunk.data.len(),
        object.version,
        object.private_data.len(),
        object.public_data.len(),
        object.children.len()
    );
    if DETAIL.contains(&chunk.id) {
        println!("{pad}  public:  {}", hex(&object.public_data, 200));
        println!("{pad}  private: {}", hex(&object.private_data, 120));
    }
    if depth < 8 {
        for child in &object.children {
            dump(child, depth + 1);
        }
    }
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: nki_tree FILE.nki");
    let chunks = sampler_kontakt::read_chunks(std::path::Path::new(&path)).expect("read chunks");
    for chunk in &chunks.0 {
        println!("top 0x{:04x} len={}", chunk.id, chunk.data.len());
        if chunk.id == 0x28 {
            dump(chunk, 1);
        }
    }
}
