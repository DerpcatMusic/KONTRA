//! Print every group's Time Machine source record (stored mode, version, fields) of .nki files.
//!
//!   tm_source_dump FILE.nki...    TSV: file, group, mode, version, field, offset, value
use ni_file::kontakt::objects::{GroupList, Program, SourceValue};
use std::path::Path;

fn main() {
    for arg in std::env::args().skip(1) {
        let path = Path::new(&arg);
        let chunks = sampler_kontakt::read_chunks(path).expect("read chunks");
        let program = Program::try_from(chunks.find_first(0x28).expect("program")).expect("program");
        let Some(list) = program.0.find_first(0x33) else { continue };
        let groups = GroupList::try_from(list).expect("group list");
        for (g, group) in groups.groups.iter().enumerate() {
            let Ok((params, _)) = group.source_params() else { continue };
            for f in &params.fields {
                let v = match f.value {
                    SourceValue::Float(v) => format!("{v}"),
                    SourceValue::Integer(v) => v.to_string(),
                    SourceValue::Flag(v) => v.to_string(),
                };
                println!("{arg}\t{g}\t{}\t{:x}\t{}\t{}\t{v}", params.mode, params.version, f.name, f.offset);
            }
        }
    }
}
