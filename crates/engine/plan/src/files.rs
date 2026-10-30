//! Array files a script reads by path (`.nka`: the array's name on the
//! first line, then one value per line), through a reader the host gives.

use kontra_vm::Text;

/// Reads the files a script names. The engine asks only while it is built,
/// off the audio thread.
pub trait Files: Send + Sync + std::fmt::Debug {
    /// The bytes of the file at `path`, or None when it cannot be read.
    fn read(&self, path: &str) -> Option<Vec<u8>>;
}

/// How an array's cells hold their values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayKind {
    Int,
    Real,
    Text,
}

impl ArrayKind {
    /// The kind a compiled array builtin passes after the array: 0 integer,
    /// 1 real, 2 text.
    #[must_use]
    pub fn of_code(code: i64) -> Self {
        match code {
            1 => Self::Real,
            2 => Self::Text,
            _ => Self::Int,
        }
    }
}

/// Fills up to `len` cells from `base` with the values of array file
/// `bytes`; a short file fills a prefix. Returns how many it filled; None,
/// and nothing written, when the file names another array than `name`,
/// holds more values than the array, or a value does not parse (Kontakt:
/// "The array from NKA file is larger than the destination array").
pub fn fill(
    bytes: &[u8],
    name: &str,
    (base, len): (usize, usize),
    kind: ArrayKind,
    cells: &mut [i64],
    texts: &mut [Text],
) -> Option<usize> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some(name) {
        return None;
    }
    if lines.clone().count() > len {
        return None;
    }
    // Text values keep their spaces; numbers may be padded.
    let values = lines;
    if kind == ArrayKind::Text {
        let cells = texts.get_mut(base..).unwrap_or_default();
        let filled = cells
            .iter_mut()
            .zip(values)
            .map(|(c, v)| *c = Text::from_str_cut(v).0);
        return Some(filled.count());
    }
    let parse = |v: &str| match kind {
        ArrayKind::Real => v.trim().parse::<f64>().ok().map(|r| r.to_bits() as i64),
        _ => v.trim().parse::<i32>().ok().map(i64::from),
    };
    let parsed = values.map(parse).collect::<Option<Vec<i64>>>()?;
    let cells = cells.get_mut(base..).unwrap_or_default();
    Some(cells.iter_mut().zip(parsed).map(|(c, v)| *c = v).count())
}

/// An array file as Kontakt writes one: the array's name, then one value
/// per line.
#[must_use]
pub fn write(name: &str, kind: ArrayKind, ints: &[i64], texts: &[Text]) -> String {
    let mut out = format!("{name}\n");
    let line = |out: &mut String, v: &dyn std::fmt::Display| {
        use std::fmt::Write as _;
        let _ = writeln!(out, "{v}");
    };
    match kind {
        ArrayKind::Text => texts.iter().for_each(|t| line(&mut out, &t.as_str())),
        ArrayKind::Real => ints
            .iter()
            .for_each(|&v| line(&mut out, &f64::from_bits(v as u64))),
        ArrayKind::Int => ints.iter().for_each(|v| line(&mut out, v)),
    }
    out
}

/// A script's array load or save by path, for the control side to carry
/// out off the audio thread.
#[derive(Clone, Copy, Debug)]
pub struct FileRequest {
    pub slot: u8,
    /// Its async id: `on async_complete` follows the answer.
    pub id: i32,
    pub base: u32,
    pub len: u32,
    pub kind: ArrayKind,
    pub path: Text,
    /// The array's name, the file's first line.
    pub name: Text,
    pub save: bool,
}

/// The control side's answer to a [`FileRequest`]: a load's values (None
/// when the file could not be read), or, for a save, room the engine fills
/// with the array's values and then, written, the outcome.
#[derive(Debug)]
pub struct FileReply {
    pub request: FileRequest,
    pub loaded: Option<(Vec<i64>, Vec<Text>)>,
    pub ints: Vec<i64>,
    pub texts: Vec<Text>,
    /// A save's values once the control side wrote them: whether it could.
    pub written: Option<bool>,
}

impl crate::ScriptPlan {
    /// Script slot `slot`'s request for the array file at `path`: cells
    /// `at` (first, count) of kind `code` ([`ArrayKind::of_code`]).
    #[must_use]
    pub fn file_request(
        &self,
        slot: u8,
        path: Text,
        (base, len): (u32, u32),
        code: i64,
        save: bool,
    ) -> FileRequest {
        let kind = ArrayKind::of_code(code);
        let text = kind == ArrayKind::Text;
        let name = self.arrays.iter().find(|a| (a.0, a.1) == (base, text));
        let name = Text::from_str_cut(name.map_or("", |a| &a.2)).0;
        let id = 0;
        FileRequest {
            slot,
            id,
            base,
            len,
            kind,
            path,
            name,
            save,
        }
    }

    /// Loads `request`'s file into the cells at once (while the engine is
    /// built, off the audio thread); false, nothing written, when it fails.
    pub fn load(&self, request: &FileRequest, (cells, texts): (&mut [i64], &mut [Text])) -> bool {
        let r = request;
        let bytes = self.files.as_ref().and_then(|f| f.read(r.path.as_str()));
        let at = (r.base as usize, r.len as usize);
        let name = r.name.as_str();
        bytes
            .and_then(|b| fill(&b, name, at, r.kind, cells, texts))
            .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_array_file_fills_a_prefix_of_its_kind_and_never_overflows() {
        let mut cells = [0i64; 4];
        let mut texts = [Text::EMPTY; 3];
        assert!(
            fill(
                b"%a\n 3\n-4\n",
                "%a",
                (1, 3),
                ArrayKind::Int,
                &mut cells,
                &mut texts
            )
            .is_some()
        );
        assert_eq!(cells, [0, 3, -4, 0]);
        assert!(
            fill(
                b"%r\n1.5\n",
                "%r",
                (0, 1),
                ArrayKind::Real,
                &mut cells,
                &mut texts
            )
            .is_some()
        );
        assert_eq!(f64::from_bits(cells[0] as u64), 1.5);
        assert!(
            fill(
                b"!t\r\n x \r\ny\nz\n",
                "!t",
                (0, 3),
                ArrayKind::Text,
                &mut cells,
                &mut texts
            ) == Some(3)
        );
        assert_eq!(texts.map(|t| t.as_str().to_owned()), [" x ", "y", "z"]);
        assert!(
            fill(
                b"%a\nnope\n",
                "%a",
                (0, 2),
                ArrayKind::Int,
                &mut cells,
                &mut texts
            )
            .is_none()
        );
        assert!(
            fill(
                b"%b\n1\n",
                "%a",
                (0, 2),
                ArrayKind::Int,
                &mut cells,
                &mut texts
            )
            .is_none()
        ); // More values than the array holds: nothing loads.
        assert!(
            fill(
                b"%a\n1\n2\n3\n",
                "%a",
                (0, 2),
                ArrayKind::Int,
                &mut cells,
                &mut texts
            )
            .is_none()
        );
    }
}
