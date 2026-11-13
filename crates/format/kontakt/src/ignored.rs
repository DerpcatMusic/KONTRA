//! What the reader parsed and the translator drops, as `ignored …`
//! diagnostics (one per field or chunk, counted, first value kept). A field
//! or chunk that is only the editor's state is listed as cosmetic, and
//! stays quiet; everything else is a gap a census can rank.

use ni_file::kontakt::{Chunk, StructuredObject};

use kontra_format::Notes;

/// Chunk `child` of `level`'s object: its reader's name, or `unknown`
/// when the reader skips it.
fn name(child: &Chunk) -> String {
    match child.into_object() {
        Ok(o) => format!("{o:?}")
            .split(['(', ' ', '{'])
            .next()
            .unwrap_or("?")
            .to_string(),
        Err(_) => "unknown".into(),
    }
}

/// Reports each child chunk of `object` the translator does not read.
pub(crate) fn chunks(
    notes: &mut Notes,
    level: &str,
    object: &StructuredObject,
    used: &[u16],
    cosmetic: &[u16],
) {
    for child in &object.children {
        if !used.contains(&child.id) && !cosmetic.contains(&child.id) {
            let what = format!("{level} chunk 0x{:02x} {}", child.id, name(child));
            notes.ignored(&what, format!("{} bytes", child.data.len()));
        }
    }
}
