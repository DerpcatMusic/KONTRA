mod container;
mod data;
mod header;
mod uuid;

pub use container::*;
pub use data::*;
pub use header::*;
pub use uuid::*;

// Advance within one declared body, so nested objects cannot consume sibling bytes.
fn read_slice<'a>(
    reader: &mut std::io::Cursor<&'a [u8]>,
    length: usize,
) -> Result<&'a [u8], crate::Error> {
    let start = usize::try_from(reader.position())
        .map_err(|_| crate::Error::Static("Invalid NIS cursor position"))?;
    let end = start
        .checked_add(length)
        .ok_or(crate::Error::Static("NIS body length overflow"))?;
    let bytes = reader
        .get_ref()
        .get(start..end)
        .ok_or(crate::Error::Static("Truncated NIS body"))?;
    reader.set_position(end as u64);
    Ok(bytes)
}
